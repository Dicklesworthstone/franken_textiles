//! Fit-only Brown calibration: shared nine-parameter lens, one rigid pose per fit view.
//! Zhang initialization is only a start; acceptance uses original pixel residuals and the
//! shared camera model's certified inverse. No OpenCV optimizer or external numeric runtime.

use ftx_core::{Mat3, Refusal, Vec2, Vec3, det};
use serde::{Deserialize, Serialize};

use super::{CalibrationReport, CalibrationSolution, CornerCheck, SheetPose, ViewCheck, geometry};
use crate::{
    bundle::{cholesky, solve},
    camera::{Camera, Intrinsics},
    invalid,
    triangulate::Partition,
};

const GLOBAL: usize = 9;
const PIVOT_FLOOR: f64 = 1e-10;
pub(super) const NO_CLAIM: &str = "Local nonlinear Brown five-coefficient/zero-skew calibration, conditional on supplied corners and an unchanged rigid reference/lens. Inverse certification is mathematical, not empirical coverage of the lens disk. No intrinsic covariance, global-optimum, real-camera accuracy or real-wearer fit claim. Sheet-local poses are not a common world frame when the sheet moves.";

/// Explicit opt-in to fitting k1/k2/k3/p1/p2 along with fx/fy/cx/cy and sheet poses.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistortionFit {
    /// Why the raw observations share a fixed Brown lens and zero-skew image geometry.
    pub model_evidence: String,
    /// Fixed undistorted normalized disk. No step or output may fold on this entire disk.
    /// Select using capture coverage, not by expanding it after fitting to reach new landmarks.
    pub max_radius: f64,
    /// Maximum accepted LM updates per fit (1–200); exhausted fits publish no solution.
    #[serde(default = "iterations")]
    pub max_iterations: usize,
}

fn iterations() -> usize {
    80
}

impl DistortionFit {
    /// Check explicit evidence, calibrated disk and work budget before processing observations.
    pub fn validate(&self) -> Result<(), Refusal> {
        if !super::evidence(&self.model_evidence)
            || !self.max_radius.is_finite()
            || !(0.0 < self.max_radius && self.max_radius <= 10.0)
            || !(1..=200).contains(&self.max_iterations)
        {
            return Err(invalid(
                "distortion_fit requires model_evidence, 0 < max_radius <= 10 and 1..=200 max_iterations",
            ));
        }
        Ok(())
    }
}

/// Numerical diagnostics, never a statistical precision estimate.
#[derive(Clone, Debug, Serialize)]
pub struct RefinementReport {
    /// Parameters shared only across fitting views.
    pub parameters: [&'static str; 9],
    /// Fit-only half squared pixel residuals, including the initialization.
    pub objective_history: Vec<f64>,
    /// Accepted joint LM updates (does not count separately fitted held-out poses).
    pub iterations: usize,
    /// Infinity norm of the column-normalized pixel gradient at the last joint iterate.
    pub scaled_gradient_inf: Option<f64>,
    /// Smallest allowed undamped normalized-Hessian Cholesky pivot, before damping.
    pub min_normal_pivot: f64,
    /// Minimum/maximum observed fit-point radius in the undistorted normalized camera frame.
    pub fit_radius_range: Option<[f64; 2]>,
    /// Termination reason for the joint fit; held-out reprojection may still reject its result.
    pub termination: String,
}

#[derive(Clone)]
struct Pose {
    r: Mat3,
    t: Vec3,
}

#[derive(Clone)]
struct State {
    k: Intrinsics,
    poses: Vec<Pose>,
}

#[derive(Clone, Copy)]
struct Observation {
    point: Vec3,
    pixel: Vec2,
}

struct System {
    cost: f64,
    h: Vec<Vec<f64>>,
    g: Vec<f64>,
}

fn initial_pose(k: Intrinsics, observations: &[Observation]) -> Result<Pose, Refusal> {
    let src: Vec<_> = observations
        .iter()
        .map(|o| [o.point.x, o.point.z])
        .collect();
    let dst = observations
        .iter()
        .map(|o| k.bearing(o.pixel).map(|p| [p.x / p.z, p.y / p.z]))
        .collect::<Result<Vec<_>, _>>()?;
    let h = Mat3::from_rows(geometry::fit(&src, &dst)?.0);
    let (a, b, t) = (h.col(0), h.col(1), h.col(2));
    let (na, nb) = (a.norm(), b.norm());
    if na <= 1e-12 || nb <= 1e-12 || t.z == 0.0 {
        return Err(invalid("no nondegenerate starting sheet pose"));
    }
    let sign = t.z.signum();
    let rx = a / na * sign;
    let rz = (b - rx * rx.dot(b)).normalized() * sign;
    let pose = Pose {
        r: Mat3::from_cols(rx, rz.cross(rx), rz),
        t: t * (sign * 2.0 / (na + nb)),
    };
    // Unlike from_sheet, initialization does not impose a 3-pixel gate before joint refinement.
    // It is private and never becomes an accepted camera without the final original-pixel checks.
    Camera {
        intrinsics: k,
        world_to_camera: pose.r,
        center_m: -pose.r.transpose().mul_vec(pose.t),
    }
    .validate()?;
    Ok(pose)
}

/// Local coordinates: log fx/fy, cx/cy divided by image_scale, five Brown coefficients,
/// then a left rotation perturbation and additive translation in normalized sheet units.
fn project(
    k: Intrinsics,
    pose: &Pose,
    o: Observation,
    image_scale: f64,
) -> Result<(Vec2, [[f64; 15]; 2]), Refusal> {
    let rp = pose.r.mul_vec(o.point);
    let q = rp + pose.t;
    let camera = Camera {
        intrinsics: k,
        world_to_camera: Mat3::IDENTITY,
        center_m: Vec3::ZERO,
    };
    let (pixel, d) = camera.project_with_jacobian(q)?;
    let (x, y) = (q.x / q.z, q.y / q.z);
    let r2 = x * x + y * y;
    let mut j = [[0.0; 15]; 2];
    j[0][0] = pixel.x - k.principal_px.x;
    j[1][1] = pixel.y - k.principal_px.y;
    j[0][2] = image_scale;
    j[1][3] = image_scale;
    let mut power = r2;
    for c in 4..7 {
        j[0][c] = k.focal_px.x * x * power;
        j[1][c] = k.focal_px.y * y * power;
        power *= r2;
    }
    j[0][7] = k.focal_px.x * 2.0 * x * y;
    j[1][7] = k.focal_px.y * (r2 + 2.0 * y * y);
    j[0][8] = k.focal_px.x * (r2 + 2.0 * x * x);
    j[1][8] = k.focal_px.y * 2.0 * x * y;
    for i in 0..2 {
        j[i][9] = -d[i][1] * rp.z + d[i][2] * rp.y;
        j[i][10] = d[i][0] * rp.z - d[i][2] * rp.x;
        j[i][11] = -d[i][0] * rp.y + d[i][1] * rp.x;
        j[i][12..15].copy_from_slice(&d[i]);
    }
    if j.iter().flatten().any(|v| !v.is_finite()) {
        return Err(invalid("calibration derivative overflow"));
    }
    Ok((pixel, j))
}

fn system(
    state: &State,
    observations: &[Vec<Observation>],
    global: bool,
    image_scale: f64,
) -> Result<System, Refusal> {
    let offset = if global { GLOBAL } else { 0 };
    let n = offset + 6 * state.poses.len();
    let mut s = System {
        cost: 0.0,
        h: vec![vec![0.0; n]; n],
        g: vec![0.0; n],
    };
    for (i, (pose, points)) in state.poses.iter().zip(observations).enumerate() {
        let indices: Vec<_> = (0..offset)
            .chain(offset + 6 * i..offset + 6 * i + 6)
            .collect();
        for &o in points {
            let (pixel, j) = project(state.k, pose, o, image_scale)?;
            let residual = [pixel.x - o.pixel.x, pixel.y - o.pixel.y];
            for row in 0..2 {
                let values: Vec<_> = j[row][..offset]
                    .iter()
                    .chain(&j[row][9..15])
                    .copied()
                    .collect();
                s.cost += 0.5 * residual[row] * residual[row];
                for (a, &ia) in indices.iter().enumerate() {
                    s.g[ia] += values[a] * residual[row];
                    for (b, &ib) in indices[..=a].iter().enumerate() {
                        s.h[ia][ib] += values[a] * values[b];
                    }
                }
            }
        }
    }
    for i in 0..n {
        for j in 0..i {
            s.h[j][i] = s.h[i][j];
        }
    }
    if !s.cost.is_finite() {
        return Err(invalid("calibration objective overflow"));
    }
    Ok(s)
}

fn advance(state: &State, step: &[f64], global: bool, image: [u32; 2]) -> Option<State> {
    if step.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut next = state.clone();
    let offset = if global { GLOBAL } else { 0 };
    if global {
        let scale = f64::from(image[0].max(image[1]));
        next.k.focal_px.x *= det::exp(step[0]);
        next.k.focal_px.y *= det::exp(step[1]);
        next.k.principal_px += Vec2::new(step[2], step[3]) * scale;
        for i in 0..3 {
            next.k.distortion.radial[i] += step[4 + i];
        }
        for i in 0..2 {
            next.k.distortion.tangential[i] += step[7 + i];
        }
    }
    next.k.validate().ok()?;
    if next.k.principal_px.x < 0.0
        || next.k.principal_px.y < 0.0
        || next.k.principal_px.x > f64::from(image[0] - 1)
        || next.k.principal_px.y > f64::from(image[1] - 1)
    {
        return None;
    }
    for (i, p) in next.poses.iter_mut().enumerate() {
        let s = &step[offset + 6 * i..offset + 6 * i + 6];
        let omega = Vec3::new(s[0], s[1], s[2]);
        let angle = omega.norm();
        if !angle.is_finite() {
            return None;
        }
        if angle > 0.0 {
            p.r = Mat3::rotation(omega / angle, angle) * p.r;
        }
        p.t += Vec3::new(s[3], s[4], s[5]);
        if !p.t.is_finite() {
            return None;
        }
    }
    Some(next)
}

fn optimize(
    state: &mut State,
    observations: &[Vec<Observation>],
    global: bool,
    image: [u32; 2],
    options: &DistortionFit,
    telemetry: &mut RefinementReport,
) -> Result<(), Refusal> {
    let scale = f64::from(image[0].max(image[1]));
    let mut s = system(state, observations, global, scale)?;
    telemetry.objective_history.push(s.cost);
    let mut damping = 1e-4_f64;
    loop {
        let norms: Vec<_> = (0..s.g.len()).map(|i| s.h[i][i].sqrt()).collect();
        if norms.iter().any(|v| !v.is_finite() || *v == 0.0) {
            return Err(invalid(
                "UNOBSERVABLE: zero fit-only calibration Jacobian column",
            ));
        }
        let h: Vec<Vec<_>> =
            s.h.iter()
                .enumerate()
                .map(|(i, r)| {
                    r.iter()
                        .enumerate()
                        .map(|(j, v)| v / (norms[i] * norms[j]))
                        .collect()
                })
                .collect();
        let g: Vec<_> = s.g.iter().zip(&norms).map(|(v, n)| v / n).collect();
        telemetry.scaled_gradient_inf = Some(g.iter().map(|v| v.abs()).fold(0.0, f64::max));
        if cholesky(&h, 0.0, PIVOT_FLOOR).is_none() {
            return Err(invalid(
                "UNOBSERVABLE: fit-only lens/pose Jacobian is rank deficient or ill-conditioned; damping cannot supply missing evidence",
            ));
        }
        if telemetry.scaled_gradient_inf.unwrap() <= 1e-8 * s.cost.sqrt().max(1e-4)
            || s.cost <= 1e-16
        {
            telemetry.termination = "gradient_converged".into();
            return Ok(());
        }
        if telemetry.iterations == options.max_iterations {
            return Err(invalid(
                "BUDGET_STOP: Brown calibration iteration limit reached; no solution published",
            ));
        }
        let mut accepted = None;
        for _ in 0..16 {
            if let Some(l) = cholesky(&h, damping, 0.0) {
                let step: Vec<_> = solve(&l, &g)
                    .iter()
                    .zip(&norms)
                    .map(|(v, n)| v / n)
                    .collect();
                if let Some(next) = advance(state, &step, global, image)
                    && let Ok(next_system) = system(&next, observations, global, scale)
                    && next_system.cost < s.cost
                {
                    accepted = Some((next, next_system));
                    break;
                }
            }
            damping *= 10.0;
        }
        let Some((next, next_system)) = accepted else {
            return Err(invalid(
                "NOT_CONVERGED: Brown calibration stalled before gradient convergence",
            ));
        };
        *state = next;
        s = next_system;
        telemetry.iterations += 1;
        telemetry.objective_history.push(s.cost);
        damping = (damping / 3.0).max(1e-12);
    }
}

fn telemetry() -> RefinementReport {
    RefinementReport {
        parameters: ["fx", "fy", "cx", "cy", "k1", "k2", "k3", "p1", "p2"],
        objective_history: Vec::new(),
        iterations: 0,
        scaled_gradient_inf: None,
        min_normal_pivot: PIVOT_FLOOR,
        fit_radius_range: None,
        termination: "not_started".into(),
    }
}

pub(super) fn calibrate(report: &mut CalibrationReport, mut k: Intrinsics) -> Result<(), Refusal> {
    report.no_claim = NO_CLAIM;
    report.status = "REJECTED";
    let input = &report.input;
    let options = input.distortion_fit.as_ref().unwrap();
    let indices: Vec<_> = input
        .views
        .iter()
        .enumerate()
        .filter(|(_, v)| v.partition == Partition::Fit)
        .map(|(i, _)| i)
        .collect();
    if indices.len() < 4
        || indices.len() == input.views.len()
        || input.views.iter().any(|v| {
            v.corners
                .iter()
                .filter(|p| p.partition == Partition::Fit)
                .count()
                < 12
                || v.corners
                    .iter()
                    .filter(|p| p.partition == Partition::HeldOut)
                    .count()
                    < 4
        })
    {
        report.status = "UNOBSERVABLE";
        report.flags.push("Brown calibration requires four fit views and an independent held-out view, with twelve fit and four held-out corners in every view".into());
        return Ok(());
    }
    // A known sheet fixes the metric gauge. Center/scale it before numerical assembly so
    // translation units and a change of sheet origin cannot dominate the normal equations.
    let first = input.controls[0].sheet_xz_m;
    let extent = input
        .controls
        .iter()
        .map(|p| (p.sheet_xz_m - first).norm())
        .fold(0.0, f64::max);
    if extent <= 0.0 || !extent.is_finite() {
        return Err(invalid("invalid calibration sheet extent"));
    }
    let observations: Vec<Vec<_>> = input
        .views
        .iter()
        .map(|v| {
            v.corners
                .iter()
                .filter(|p| p.partition == Partition::Fit)
                .map(|p| {
                    let c = input.controls.iter().find(|c| c.id == p.id).unwrap();
                    let q = (c.sheet_xz_m - first) / extent;
                    Observation {
                        point: Vec3::new(q.x, 0.0, q.y),
                        pixel: p.pixel_px,
                    }
                })
                .collect()
        })
        .collect();
    k.distortion.max_radius = options.max_radius;
    let poses = indices
        .iter()
        .map(|&i| initial_pose(k, &observations[i]))
        .collect::<Result<Vec<_>, _>>();
    let mut diagnostics = telemetry();
    let mut state = match poses {
        Ok(poses) => State { k, poses },
        Err(e) => {
            report.flags.push(e.to_string());
            return Ok(());
        }
    };
    let fit: Vec<_> = indices.iter().map(|&i| observations[i].clone()).collect();
    if let Err(e) = optimize(
        &mut state,
        &fit,
        true,
        input.image_size_px,
        options,
        &mut diagnostics,
    ) {
        diagnostics.termination = e.message.clone();
        report.status = if e.message.starts_with("UNOBSERVABLE") {
            "UNOBSERVABLE"
        } else {
            "REJECTED"
        };
        report.flags.push(e.message);
        report.refinement = Some(diagnostics);
        return Ok(());
    }
    let mut range = [f64::INFINITY, 0.0_f64];
    for (pose, observations) in state.poses.iter().zip(&fit) {
        for o in observations {
            let q = pose.r.mul_vec(o.point) + pose.t;
            let radius = det::hypot(q.x / q.z, q.y / q.z);
            range[0] = range[0].min(radius);
            range[1] = range[1].max(radius);
        }
    }
    diagnostics.fit_radius_range = Some(range);
    report.refinement = Some(diagnostics);
    let mut poses = Vec::new();
    for (i, view) in input.views.iter().enumerate() {
        // Held-out views only fit their own pose with the frozen shared lens. Their held-out
        // corners still cannot enter that fit, initialization or convergence criterion.
        let pose = if let Some(j) = indices.iter().position(|&j| i == j) {
            Ok(state.poses[j].clone())
        } else {
            initial_pose(state.k, &observations[i]).and_then(|pose| {
                let mut local = State {
                    k: state.k,
                    poses: vec![pose],
                };
                optimize(
                    &mut local,
                    &observations[i..i + 1],
                    false,
                    input.image_size_px,
                    options,
                    &mut telemetry(),
                )?;
                Ok(local.poses.remove(0))
            })
        };
        let camera = pose.map(|p| Camera {
            intrinsics: state.k,
            world_to_camera: p.r,
            center_m: Vec3::new(first.x, 0.0, first.y) - p.r.transpose().mul_vec(p.t) * extent,
        });
        let mut check = ViewCheck {
            frame: view.frame.clone(),
            partition: view.partition,
            corners: Vec::new(),
            failure: None,
        };
        for p in &view.corners {
            let c = input.controls.iter().find(|c| c.id == p.id).unwrap();
            let error = camera
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|camer| camer.project(Vec3::new(c.sheet_xz_m.x, 0.0, c.sheet_xz_m.y)))
                .map(|pixel| pixel.dist(p.pixel_px));
            let error_px = match error {
                Ok(e) if e.is_finite() => Some(e),
                Err(e) => {
                    check.failure = Some(e.message);
                    None
                }
                _ => None,
            };
            if error_px.is_none_or(|e| e > super::MAX_ERROR_PX) {
                report.flags.push(format!(
                    "{} at {} us / {}: invalid Brown projection or residual exceeds 3 pixels",
                    view.frame.stream, view.frame.timestamp_us, p.id
                ));
            }
            check.corners.push(CornerCheck {
                id: p.id.clone(),
                partition: p.partition,
                error_px,
            });
        }
        if let Ok(camera_in_sheet_frame) = camera {
            poses.push(SheetPose {
                frame: view.frame.clone(),
                camera_in_sheet_frame,
            });
        }
        report.checks.push(check);
    }
    if report.flags.is_empty() {
        report.status = "ACCEPTED";
        report.solution = Some(CalibrationSolution {
            intrinsics: state.k,
            sheet_poses: poses,
        });
    }
    Ok(())
}

/// Lens block of the inverse *joint* undamped normal matrix, not the inverse lens block.
/// Work in the same normalized sheet and local coordinates as refinement, then convert the
/// marginal covariance back to physical focal/principal/coefficient units.
pub(super) fn covariance(
    report: &CalibrationReport,
    sigma_px: f64,
) -> Result<super::uncertainty::IntrinsicUncertainty, Refusal> {
    use super::uncertainty::{IntrinsicUncertainty, LENS_PARAMETERS};
    let input = &report.input;
    let solution = report
        .solution
        .as_ref()
        .ok_or_else(|| invalid("no calibrated lens"))?;
    let first = input.controls[0].sheet_xz_m;
    let origin = Vec3::new(first.x, 0.0, first.y);
    let extent = input
        .controls
        .iter()
        .map(|p| (p.sheet_xz_m - first).norm())
        .fold(0.0, f64::max);
    let mut state = State {
        k: solution.intrinsics,
        poses: Vec::new(),
    };
    let mut observations = Vec::new();
    for view in input.views.iter().filter(|v| v.partition == Partition::Fit) {
        let camera = &solution
            .sheet_poses
            .iter()
            .find(|p| p.frame == view.frame)
            .ok_or_else(|| invalid("missing fitted pose for covariance"))?
            .camera_in_sheet_frame;
        state.poses.push(Pose {
            r: camera.world_to_camera,
            t: camera.world_to_camera.mul_vec(origin - camera.center_m) / extent,
        });
        observations.push(
            view.corners
                .iter()
                .filter(|p| p.partition == Partition::Fit)
                .map(|p| {
                    let control = input.controls.iter().find(|c| c.id == p.id).unwrap();
                    let q = (control.sheet_xz_m - first) / extent;
                    Observation {
                        point: Vec3::new(q.x, 0.0, q.y),
                        pixel: p.pixel_px,
                    }
                })
                .collect::<Vec<_>>(),
        );
    }
    let image_scale = f64::from(input.image_size_px[0].max(input.image_size_px[1]));
    let s = system(&state, &observations, true, image_scale)?;
    let n = s.h.len();
    let norms: Vec<_> = (0..n).map(|i| s.h[i][i].sqrt()).collect();
    if norms.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(invalid("UNOBSERVABLE: zero covariance Jacobian column"));
    }
    let normal: Vec<Vec<_>> =
        s.h.iter()
            .enumerate()
            .map(|(i, row)| {
                row.iter()
                    .enumerate()
                    .map(|(j, v)| v / (norms[i] * norms[j]))
                    .collect()
            })
            .collect();
    let l = cholesky(&normal, 0.0, PIVOT_FLOOR).ok_or_else(|| {
        invalid("UNOBSERVABLE: undamped joint covariance matrix is ill-conditioned")
    })?;
    let units = [
        state.k.focal_px.x,
        state.k.focal_px.y,
        image_scale,
        image_scale,
        1.0,
        1.0,
        1.0,
        1.0,
        1.0,
    ];
    let mut covariance = [[0.0; GLOBAL]; GLOBAL];
    for j in 0..GLOBAL {
        let mut rhs = vec![0.0; n];
        rhs[j] = -1.0; // bundle::solve solves H x = -rhs.
        let column = solve(&l, &rhs);
        for i in 0..GLOBAL {
            covariance[i][j] =
                column[i] * (units[i] / norms[i]) * (units[j] / norms[j]) * sigma_px.powi(2);
        }
    }
    for i in 0..GLOBAL {
        for j in 0..i {
            let v = (covariance[i][j] + covariance[j][i]) * 0.5;
            covariance[i][j] = v;
            covariance[j][i] = v;
        }
    }
    let sigma: [f64; GLOBAL] = std::array::from_fn(|i| covariance[i][i].sqrt());
    if sigma.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || covariance.iter().flatten().any(|v| !v.is_finite())
    {
        return Err(invalid("nonfinite or nonpositive intrinsic covariance"));
    }
    let correlation: Vec<Vec<_>> = covariance
        .iter()
        .enumerate()
        .map(|(i, r)| {
            r.iter()
                .enumerate()
                .map(|(j, v)| v / (sigma[i] * sigma[j]))
                .collect()
        })
        .collect();
    if cholesky(&correlation, 0.0, 0.0).is_none() {
        return Err(invalid("intrinsic covariance lost positive definiteness"));
    }
    Ok(IntrinsicUncertainty {
        intrinsics: state.k,
        covariance,
        sigma,
        parameters: LENS_PARAMETERS,
        fitting_views: state.poses.len(),
        fitting_scalar_observations: observations.iter().map(|p| 2 * p.len()).sum(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Distortion;

    #[test]
    fn all_fifteen_local_projection_derivatives_match_central_differences() {
        let k = Intrinsics {
            focal_px: Vec2::new(1050.0, 970.0),
            principal_px: Vec2::new(760.0, 590.0),
            distortion: Distortion {
                radial: [0.12, -0.03, 0.01],
                tangential: [0.003, -0.002],
                max_radius: 0.9,
            },
        };
        let state = State {
            k,
            poses: vec![Pose {
                r: Mat3::rotation(Vec3::new(0.0, 1.0, 0.0), 0.3),
                t: Vec3::new(0.15, 0.12, 1.5),
            }],
        };
        let o = Observation {
            point: Vec3::new(0.31, 0.0, -0.14),
            pixel: Vec2::ZERO,
        };
        let (_, j) = project(k, &state.poses[0], o, 1600.0).unwrap();
        for column in 0..15 {
            let mut d = vec![0.0; 15];
            d[column] = 1e-6;
            let plus = advance(&state, &d, true, [1600, 1200]).unwrap();
            d[column] = -1e-6;
            let minus = advance(&state, &d, true, [1600, 1200]).unwrap();
            let a = project(plus.k, &plus.poses[0], o, 1600.0).unwrap().0;
            let b = project(minus.k, &minus.poses[0], o, 1600.0).unwrap().0;
            let numerical = (a - b) / 2e-6;
            assert!(
                (numerical.x - j[0][column]).abs() < 2e-6,
                "column {column}: {numerical:?}"
            );
            assert!(
                (numerical.y - j[1][column]).abs() < 2e-6,
                "column {column}: {numerical:?}"
            );
        }
        let mut d = vec![0.0; 15];
        d[4] = -100.0;
        assert!(
            advance(&state, &d, true, [1600, 1200]).is_none(),
            "a folding lens must never be a valid step"
        );
    }
}
