//! Joint calibrated camera/landmark refinement for FTX-20 (plan §10.2).
//! Fixed metric controls remove the similarity gauge; intrinsics are never optimized.
//! Local nonlinear least squares, not a moving-body model or uncertainty certificate.
//! Formulation: https://ceres-solver.org/nnls_tutorial.html#bundle-adjustment

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Mat3, Refusal, Vec2, Vec3, det};
use serde::{Deserialize, Serialize};

use crate::{
    camera::Camera,
    invalid,
    triangulate::{Capture, FrameId, Partition, check_frames},
};

/// A known control position or an initial free landmark, in metres in the same z-up frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    /// Unique within controls AND landmarks; observation references use this identifier.
    pub id: String,
    /// Fixed for controls; only an initial guess for landmarks.
    pub position_m: Vec3,
}

/// Initial camera pose and fixed lens calibration for one decoded frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    /// Decoded, clock-aligned identity, never a frame ordinal.
    pub frame: FrameId,
    /// All six extrinsic degrees of freedom are refined; intrinsics remain fixed.
    pub camera: Camera,
}

/// A point observation, assigned to fit or held-out before optimization.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Must refer to a supplied view.
    pub frame: FrameId,
    /// Must name exactly one control or landmark.
    pub point: String,
    /// Full-resolution oriented image coordinates, matching the camera calibration.
    pub pixel_px: Vec2,
    /// Held-out observations contribute neither residuals nor derivatives to optimization.
    pub partition: Partition,
    /// Positive per-coordinate pixel standard deviation used only as a residual weight.
    pub sigma_px: f64,
}

/// Bounded, anchored bundle problem. No media is decoded or opened by this library.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleInput {
    /// Must be `ftx-bundle-input/1`.
    pub schema: String,
    /// Receipt identifying the physical reference, its metric scale and the common world frame.
    pub metric_scale_source: String,
    /// Stillness/synchronization is caller-supplied evidence, not inferred from a small residual.
    pub capture: Capture,
    /// 2–8 cameras; each needs at least four distinct fit control observations.
    pub views: Vec<View>,
    /// 4–128 fixed metric controls, e.g. marked points on one stationary sheet.
    pub controls: Vec<Point>,
    /// 1–32 free landmarks, each seen in at least two fit views.
    pub landmarks: Vec<Point>,
    /// At most 2048 distinct (frame, point) observations; no silent outlier removal.
    pub observations: Vec<Observation>,
}

/// Fully accepted geometry only. Rejected/budget-stopped reports do not contain this object.
#[derive(Clone, Debug, Serialize)]
pub struct Solution {
    /// Refined cameras in canonical decoded-frame order.
    pub views: Vec<View>,
    /// Refined landmarks in identifier order, metres and z up.
    pub landmarks: Vec<Point>,
}

/// Per-observation validation, separate from the optimization objective.
#[derive(Clone, Debug, Serialize)]
pub struct Check {
    /// Original observation with its partition, calibration reference and uncertainty weight.
    pub observation: Observation,
    /// Unweighted Euclidean error in pixels; absent when projection is invalid.
    pub error_px: Option<f64>,
    /// Invalid projection reason (e.g. nonpositive depth or a lens-domain violation).
    pub failure: Option<String>,
}

/// A diagnostic result is not permission to use a failed iterate as a measurement.
#[derive(Clone, Debug, Serialize)]
pub struct BundleReport {
    /// `ftx-bundle/1`.
    pub schema: &'static str,
    /// ACCEPTED, REJECTED, UNOBSERVABLE, NOT_CONVERGED or BUDGET_STOP.
    pub status: &'static str,
    /// Present only when rank, convergence, parallax and EVERY reprojection check pass.
    pub solution: Option<Solution>,
    /// Original input, canonically ordered, never replaced with optimized observations.
    pub input: BundleInput,
    /// Initial and accepted-step sums of squared sigma-weighted FIT pixel residuals.
    pub objective_history: Vec<f64>,
    /// Number of accepted steps, not trial solves.
    pub iterations: usize,
    /// Per-view/point diagnostics, including held-out checks, when an iterate was available.
    pub checks: Vec<Check>,
    /// Hard failures; none is silently demoted or excluded.
    pub flags: Vec<String>,
    /// Limitations such as no independent held-out observations.
    pub advisories: Vec<String>,
    /// Boundary of the inference.
    pub no_claim: &'static str,
}

#[derive(Clone)]
struct State {
    cameras: Vec<Camera>,
    points: Vec<Vec3>, // controls first, then free landmarks
}

struct Factor {
    camera: usize,
    point: usize,
}

fn name(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256
}

fn prepare(input: &BundleInput) -> Result<(BundleInput, Vec<Factor>), Refusal> {
    if input.schema != "ftx-bundle-input/1"
        || input.metric_scale_source.trim().is_empty()
        || input.metric_scale_source.len() > 4096
        || !(2..=8).contains(&input.views.len())
        || !(4..=128).contains(&input.controls.len())
        || !(1..=32).contains(&input.landmarks.len())
        || input.observations.is_empty()
        || input.observations.len() > 2048
    {
        return Err(invalid(
            "bundle needs schema /1, a metric-scale receipt, 2–8 views, 4–128 controls, 1–32 landmarks and 1–2048 observations",
        ));
    }
    let mut input = input.clone();
    input.views.sort_by(|a, b| a.frame.cmp(&b.frame));
    input.controls.sort_by(|a, b| a.id.cmp(&b.id));
    input.landmarks.sort_by(|a, b| a.id.cmp(&b.id));
    input
        .observations
        .sort_by(|a, b| (&a.frame, &a.point).cmp(&(&b.frame, &b.point)));
    let mut views = BTreeMap::new();
    for (i, v) in input.views.iter().enumerate() {
        v.camera.validate()?;
        if !name(&v.frame.stream) || views.insert(&v.frame, i).is_some() {
            return Err(invalid(
                "bundle views need unique decoded identities and nonempty streams",
            ));
        }
    }
    check_frames(
        &input.capture,
        &input.views.iter().map(|v| &v.frame).collect::<Vec<_>>(),
    )?;
    let mut points = BTreeMap::new();
    for (i, p) in input.controls.iter().chain(&input.landmarks).enumerate() {
        if !name(&p.id) || !p.position_m.is_finite() || points.insert(&p.id, i).is_some() {
            return Err(invalid(
                "bundle points need finite positions and globally unique identifiers",
            ));
        }
    }
    // Distinct identifiers cannot manufacture additional fixed geometric information.
    for (i, p) in input.controls.iter().enumerate() {
        if input.controls[..i]
            .iter()
            .any(|q| q.position_m == p.position_m)
        {
            return Err(invalid("fixed metric control positions must be distinct"));
        }
    }
    let mut seen = BTreeSet::new();
    let mut factors = Vec::new();
    for o in &input.observations {
        let camera = *views
            .get(&o.frame)
            .ok_or_else(|| invalid("unknown bundle frame"))?;
        let point = *points
            .get(&o.point)
            .ok_or_else(|| invalid("unknown bundle point"))?;
        if !seen.insert((&o.frame, &o.point))
            || !o.sigma_px.is_finite()
            || o.sigma_px < 1e-6
            || o.sigma_px > 1e6
        {
            return Err(invalid(
                "observations must be unique with sigma_px in [1e-6, 1e6]",
            ));
        }
        input.views[camera].camera.ray(o.pixel_px)?;
        factors.push(Factor { camera, point });
    }
    Ok((input, factors))
}

// d pixel / d(world rotation around camera centre) = (X-C) cross d pixel / dX.
// R_new = R * exp(omega); C_new = C + delta_C. No Euler angles or rotation-matrix clamping.
fn camera_rows(c: &Camera, p: Vec3) -> Result<(Vec2, [[f64; 9]; 2]), Refusal> {
    let (pixel, j) = c.project_with_jacobian(p)?;
    Ok((
        pixel,
        std::array::from_fn(|i| {
            let v = Vec3::from(j[i]);
            let w = (p - c.center_m).cross(v);
            [-v.x, -v.y, -v.z, w.x, w.y, w.z, v.x, v.y, v.z]
        }),
    ))
}

struct System {
    rows: Vec<Vec<f64>>,
    residual: Vec<f64>,
    cost: f64,
}

fn system(input: &BundleInput, factors: &[Factor], state: &State) -> Result<System, Refusal> {
    let n = 6 * input.views.len() + 3 * input.landmarks.len();
    let mut s = System {
        rows: Vec::new(),
        residual: Vec::new(),
        cost: 0.0,
    };
    for (o, f) in input.observations.iter().zip(factors) {
        if o.partition != Partition::Fit {
            continue;
        }
        let (pixel, j) = camera_rows(&state.cameras[f.camera], state.points[f.point])?;
        let r = (pixel - o.pixel_px) / o.sigma_px;
        for (k, value) in [r.x, r.y].into_iter().enumerate() {
            let mut row = vec![0.0; n];
            for t in 0..6 {
                row[6 * f.camera + t] = j[k][t] / o.sigma_px;
            }
            if f.point >= input.controls.len() {
                for t in 0..3 {
                    row[6 * input.views.len() + 3 * (f.point - input.controls.len()) + t] =
                        j[k][6 + t] / o.sigma_px;
                }
            }
            s.rows.push(row);
            s.residual.push(value);
            s.cost += value * value;
        }
    }
    if !s.cost.is_finite() || s.rows.iter().flatten().any(|v| !v.is_finite()) {
        return Err(invalid("non-finite bundle objective or derivative"));
    }
    Ok(s)
}

// Small bounded dense LM system (<=144 variables), column-scaled before normal assembly.
// An undamped factorization is REQUIRED separately: damping must not hide missing information.
fn normal(s: &System) -> (Vec<Vec<f64>>, Vec<f64>, Vec<f64>) {
    let n = s.rows[0].len();
    let norms: Vec<f64> = (0..n)
        .map(|i| s.rows.iter().map(|r| r[i] * r[i]).sum::<f64>().sqrt())
        .collect();
    let mut h = vec![vec![0.0; n]; n];
    let mut g = vec![0.0; n];
    for (row, r) in s.rows.iter().zip(&s.residual) {
        // Each residual touches at most six camera and three point variables.
        let active: Vec<_> = row
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0.0)
            .map(|(i, v)| (i, v / norms[i]))
            .collect();
        for &(i, a) in &active {
            g[i] += a * r;
            for &(j, b) in &active {
                h[i][j] += a * b;
            }
        }
    }
    (h, g, norms)
}

pub(crate) fn cholesky(h: &[Vec<f64>], damping: f64, floor: f64) -> Option<Vec<Vec<f64>>> {
    let n = h.len();
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let mut v = h[i][j] + if i == j { damping } else { 0.0 };
            for k in 0..j {
                v -= l[i][k] * l[j][k];
            }
            if !v.is_finite() {
                return None;
            }
            if i == j {
                if v <= floor {
                    return None;
                }
                l[i][j] = v.sqrt();
            } else {
                l[i][j] = v / l[j][j];
            }
        }
    }
    Some(l)
}

pub(crate) fn solve(l: &[Vec<f64>], g: &[f64]) -> Vec<f64> {
    let n = g.len();
    let mut x = vec![0.0; n];
    for i in 0..n {
        x[i] = (-g[i] - (0..i).map(|j| l[i][j] * x[j]).sum::<f64>()) / l[i][i];
    }
    for i in (0..n).rev() {
        x[i] = (x[i] - (i + 1..n).map(|j| l[j][i] * x[j]).sum::<f64>()) / l[i][i];
    }
    x
}

fn advance(state: &State, step: &[f64], controls: usize) -> Option<State> {
    if step.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut out = state.clone();
    for (i, c) in out.cameras.iter_mut().enumerate() {
        c.center_m += Vec3::new(step[6 * i], step[6 * i + 1], step[6 * i + 2]);
        let omega = Vec3::new(step[6 * i + 3], step[6 * i + 4], step[6 * i + 5]);
        let angle = omega.norm();
        if !angle.is_finite() {
            return None;
        }
        if angle > 0.0 {
            c.world_to_camera = c.world_to_camera * Mat3::rotation(omega / angle, angle);
        }
        c.validate().ok()?;
    }
    for (i, p) in out.points[controls..].iter_mut().enumerate() {
        let j = 6 * out.cameras.len() + 3 * i;
        *p += Vec3::new(step[j], step[j + 1], step[j + 2]);
        if !p.is_finite() {
            return None;
        }
    }
    Some(out)
}

fn angle(
    input: &BundleInput,
    factors: &[Factor],
    state: &State,
    point: usize,
    predicted: bool,
) -> f64 {
    let views: Vec<_> = input
        .observations
        .iter()
        .zip(factors)
        .filter(|(o, f)| f.point == point && o.partition == Partition::Fit)
        .collect();
    let mut best = 0.0_f64;
    for (i, (a, fa)) in views.iter().enumerate() {
        for (b, fb) in &views[..i] {
            let ca = &state.cameras[fa.camera];
            let cb = &state.cameras[fb.camera];
            if (ca.center_m - cb.center_m).norm() <= 1e-8 {
                continue;
            }
            let rays = if predicted {
                Ok((
                    (state.points[point] - ca.center_m).normalized(),
                    (state.points[point] - cb.center_m).normalized(),
                ))
            } else {
                ca.ray(a.pixel_px)
                    .and_then(|da| cb.ray(b.pixel_px).map(|db| (da, db)))
            };
            if let Ok((a, b)) = rays {
                best = best.max(det::atan2(a.cross(b).norm(), a.dot(b).abs()).to_degrees());
            }
        }
    }
    best
}

/// Jointly adjust poses and landmarks. `max_iterations` must be in 1..=100 (50 is typical).
/// A fixed-control gauge and the conservative undamped normalized-Hessian pivot check are
/// mandatory. Rank loss, a stalled solve or budget exhaustion never publish partial geometry.
pub fn adjust(input: &BundleInput, max_iterations: usize) -> Result<BundleReport, Refusal> {
    if !(1..=100).contains(&max_iterations) {
        return Err(invalid("bundle iterations must be in 1..=100"));
    }
    let (input, factors) = prepare(input)?;
    let mut report = BundleReport {
        schema: "ftx-bundle/1",
        status: "UNOBSERVABLE",
        solution: None,
        input: input.clone(),
        objective_history: Vec::new(),
        iterations: 0,
        checks: Vec::new(),
        flags: Vec::new(),
        advisories: Vec::new(),
        no_claim: "Local, calibrated, stationary-scene refinement conditional on fixed metric controls and supplied capture evidence. No moving-body fit, global optimum, calibration/scale covariance or real-wearer accuracy claim.",
    };
    if !input
        .observations
        .iter()
        .any(|o| o.partition == Partition::HeldOut)
    {
        report
            .advisories
            .push("No held-out observations; all reprojection checks are fitted-only".into());
    }
    for (i, v) in input.views.iter().enumerate() {
        let count = input
            .observations
            .iter()
            .zip(&factors)
            .filter(|(o, f)| {
                o.partition == Partition::Fit && f.camera == i && f.point < input.controls.len()
            })
            .count();
        if count < 4 {
            report.flags.push(format!(
                "{} at {} us needs four distinct fit controls to fix its metric pose",
                v.frame.stream, v.frame.timestamp_us
            ));
        }
    }
    let origin = input.controls[0].position_m;
    let scale = input
        .controls
        .iter()
        .map(|p| (p.position_m - origin).norm())
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(invalid("metric control extent must be finite and nonzero"));
    }
    let mut state = State {
        cameras: input
            .views
            .iter()
            .map(|v| Camera {
                center_m: (v.camera.center_m - origin) / scale,
                ..v.camera
            })
            .collect(),
        points: input
            .controls
            .iter()
            .chain(&input.landmarks)
            .map(|p| (p.position_m - origin) / scale)
            .collect(),
    };
    if state.points.iter().any(|p| !p.is_finite())
        || state.cameras.iter().any(|c| !c.center_m.is_finite())
    {
        return Err(invalid("normalized bundle geometry overflow"));
    }
    for (i, p) in input.landmarks.iter().enumerate() {
        if angle(&input, &factors, &state, input.controls.len() + i, false) < 3.0 {
            report.flags.push(format!(
                "{} needs two fit views with at least 3 degrees of parallax",
                p.id
            ));
        }
    }
    if !report.flags.is_empty() {
        return Ok(report);
    }
    let mut s = match system(&input, &factors, &state) {
        Ok(s) => s,
        Err(e) => {
            report.status = "REJECTED";
            report.flags.push(e.to_string());
            return Ok(report);
        }
    };
    report.objective_history.push(s.cost);
    let mut damping = 1e-4_f64;
    loop {
        let (h, g, norms) = normal(&s);
        if norms.iter().any(|v| !v.is_finite() || *v == 0.0) || cholesky(&h, 0.0, 1e-10).is_none() {
            report.status = "UNOBSERVABLE";
            report.flags.push("Fit-only bundle Jacobian is rank deficient or too ill-conditioned; damping cannot supply missing geometry".into());
            break;
        }
        if g.iter().all(|v| v.abs() <= 1e-8 * s.cost.sqrt().max(1e-4)) || s.cost <= 1e-16 {
            report.status = "ACCEPTED";
            break;
        }
        if report.iterations == max_iterations {
            report.status = "BUDGET_STOP";
            report
                .flags
                .push("Bundle iteration budget exhausted; no solution published".into());
            break;
        }
        let mut accepted = None;
        for _ in 0..16 {
            if let Some(l) = cholesky(&h, damping, 0.0) {
                let step: Vec<_> = solve(&l, &g)
                    .iter()
                    .zip(&norms)
                    .map(|(v, n)| v / n)
                    .collect();
                if let Some(next) = advance(&state, &step, input.controls.len())
                    && let Ok(next_system) = system(&input, &factors, &next)
                    && next_system.cost < s.cost
                {
                    accepted = Some((next, next_system));
                    break;
                }
            }
            damping *= 10.0;
        }
        let Some((next, next_system)) = accepted else {
            report.status = "NOT_CONVERGED";
            report
                .flags
                .push("Bundle refinement stalled before gradient convergence".into());
            break;
        };
        state = next;
        s = next_system;
        report.iterations += 1;
        report.objective_history.push(s.cost);
        damping = (damping / 3.0).max(1e-12);
    }
    for (i, p) in input.landmarks.iter().enumerate() {
        if angle(&input, &factors, &state, input.controls.len() + i, false) < 3.0
            || angle(&input, &factors, &state, input.controls.len() + i, true) < 3.0
        {
            report
                .flags
                .push(format!("{}: refined parallax is below 3 degrees", p.id));
        }
    }
    let mut solution = Solution {
        views: input.views.clone(),
        landmarks: input.landmarks.clone(),
    };
    for (v, c) in solution.views.iter_mut().zip(&state.cameras) {
        v.camera = *c;
        v.camera.center_m = origin + c.center_m * scale;
    }
    for (p, x) in solution
        .landmarks
        .iter_mut()
        .zip(&state.points[input.controls.len()..])
    {
        p.position_m = origin + *x * scale;
    }
    if solution.views.iter().any(|v| v.camera.validate().is_err())
        || solution.landmarks.iter().any(|p| !p.position_m.is_finite())
    {
        return Err(invalid("refined metric output overflow"));
    }
    // Gate the actual published world coordinates too, not only a better-conditioned internal copy.
    for (o, f) in input.observations.iter().zip(&factors) {
        let p = if f.point < input.controls.len() {
            input.controls[f.point].position_m
        } else {
            solution.landmarks[f.point - input.controls.len()].position_m
        };
        let projection = solution.views[f.camera].camera.project(p);
        let (error, failure) = match projection {
            Ok(p) => {
                let e = p.dist(o.pixel_px);
                (e.is_finite().then_some(e), None)
            }
            Err(e) => (None, Some(e.to_string())),
        };
        if error.is_none_or(|e| e > 3.0) {
            report.flags.push(format!(
                "{} at {} us / {}: invalid projection or error exceeds 3 px",
                o.frame.stream, o.frame.timestamp_us, o.point
            ));
        }
        report.checks.push(Check {
            observation: o.clone(),
            error_px: error,
            failure,
        });
    }
    if report.status == "ACCEPTED" {
        if report.flags.is_empty() {
            report.solution = Some(solution);
        } else {
            report.status = "REJECTED";
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Distortion, Intrinsics};

    #[test]
    fn pose_and_point_derivatives_match_independent_central_differences() {
        let camera = Camera {
            intrinsics: Intrinsics {
                focal_px: Vec2::new(1000.0, 900.0),
                principal_px: Vec2::new(600.0, 400.0),
                distortion: Distortion {
                    radial: [0.1, 0.003, 0.0],
                    tangential: [0.001, -0.002],
                    max_radius: 2.0,
                },
            },
            world_to_camera: Mat3::rotation(Vec3::new(1.0, 2.0, 0.5).normalized(), 0.2),
            center_m: Vec3::new(0.2, -0.3, -3.0),
        };
        let point = Vec3::new(0.1, 0.2, 0.4);
        let (_, j) = camera_rows(&camera, point).unwrap();
        let state = State {
            cameras: vec![camera],
            points: vec![point],
        };
        for k in 0..9 {
            let mut step = vec![0.0; 9];
            step[k] = 1e-6;
            let plus = advance(&state, &step, 0).unwrap();
            step[k] = -1e-6;
            let minus = advance(&state, &step, 0).unwrap();
            let derivative = (plus.cameras[0].project(plus.points[0]).unwrap()
                - minus.cameras[0].project(minus.points[0]).unwrap())
                / 2e-6;
            assert!((derivative.x - j[0][k]).abs() < 1e-6, "x {k}");
            assert!((derivative.y - j[1][k]).abs() < 1e-6, "y {k}");
        }
    }
}
