//! Metric triangulation of one landmark (plan §10.2), conditional on supplied cameras and capture
//! evidence. No motion is inferred: a reviewed still window or synchronized exposure is required.
//! Fit views alone determine the point; every observation, including held-out views, must pass.

use std::collections::BTreeSet;

use ftx_core::{Refusal, Vec2, Vec3, det, linalg::lstsq};
use serde::{Deserialize, Serialize};

use super::{camera::Camera, invalid};

/// Decoded presentation time identifies a frame, never its ordinal in a decoder's output.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameId {
    /// Stable source/stream identifier; no filename or network access is performed.
    pub stream: String,
    /// Microseconds from the declared clock origin, after decoding and clock alignment.
    pub timestamp_us: u64,
}

/// A held-out observation is checked but can never influence the fitted point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Partition {
    /// Used in the geometric least-squares fit.
    Fit,
    /// Used only in the independent reprojection check.
    HeldOut,
}

/// An identified landmark in one EXIF-oriented image, with its own calibrated camera.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Decoded frame identity.
    pub frame: FrameId,
    /// Metric camera pose and oriented-image intrinsics for this frame.
    pub camera: Camera,
    /// Pixel coordinates after the image's recorded orientation has been applied.
    pub pixel_px: Vec2,
    /// Explicit fit/held-out assignment, established before fitting.
    pub partition: Partition,
}

/// Caller-supplied capture evidence. This API verifies time consistency, not actual stillness.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Capture {
    /// One stream over a reviewed interval in which this landmark is stationary.
    StillWindow {
        /// Inclusive beginning on the stream's decoded presentation clock.
        start_us: u64,
        /// Inclusive end on the same clock.
        end_us: u64,
        /// Explanation or local receipt identifier for the stillness determination.
        evidence: String,
    },
    /// One frame per camera, with timestamps aligned to a common clock by the caller.
    Synchronized {
        /// Maximum permitted exposure skew, explicitly chosen for this acquisition.
        max_skew_us: u64,
        /// Explanation or receipt for synchronization and the permitted motion/skew.
        evidence: String,
    },
}

/// One triangulation request. Scale is supplied by calibrated camera centers, never guessed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriangulationInput {
    /// Must be `ftx-triangulation-input/1`.
    pub schema: String,
    /// Receipt or explanation establishing the shared metric scale and world frame.
    pub metric_scale_source: String,
    /// Explicit temporal acquisition assumption.
    pub capture: Capture,
    /// At most 128 observations; frame identities must be unique.
    pub observations: Vec<Observation>,
}

/// One view's check, retaining the partition so fitted errors cannot masquerade as held-out errors.
#[derive(Clone, Debug, Serialize)]
pub struct Reprojection {
    /// The checked frame.
    pub frame: FrameId,
    /// Whether it contributed to fitting.
    pub partition: Partition,
    /// Pixel residual norm; absent when projection is invalid.
    pub error_px: Option<f64>,
    /// Signed optical depth; absent only when it cannot be represented finitely.
    pub depth_m: Option<f64>,
    /// Reason projection was unavailable, such as a point behind the camera.
    pub failure: Option<String>,
}

/// Pixel-space optimization diagnostics, conditional on fixed cameras and metric scale.
#[derive(Clone, Debug, Serialize)]
pub struct Refinement {
    /// Sum of squared pixel residuals in fit views before refinement (not an RMS).
    pub initial_cost_px2: f64,
    /// The same objective after refinement; held-out pixels never contribute.
    pub final_cost_px2: f64,
    /// Number of accepted, strictly objective-decreasing Gauss–Newton steps.
    pub iterations: usize,
    /// Whether the fit reached the residual/gradient stopping criterion within its bounds.
    pub converged: bool,
}

/// Diagnostic result. A gated or unobservable estimate never publishes a usable point.
#[derive(Clone, Debug, Serialize)]
pub struct TriangulationReport {
    /// `ftx-triangulation/1`.
    pub schema: &'static str,
    /// ACCEPTED, REJECTED or UNOBSERVABLE; none is a body-fit or real-wearer accuracy claim.
    pub status: &'static str,
    /// Present only after all geometric and reprojection gates pass, in metres, z up.
    pub point_m: Option<Vec3>,
    /// Complete input receipt in canonical frame order, including calibration and capture evidence.
    pub input: TriangulationInput,
    /// Number of views used in the fit.
    pub fit_views: usize,
    /// Number of views checked without using them in the fit.
    pub held_out_views: usize,
    /// Largest acute angle between fit rays from distinct optical centers, in degrees.
    pub observed_max_angle_deg: f64,
    /// The same angle at the candidate point; absent when no finite candidate could be formed.
    pub fitted_max_angle_deg: Option<f64>,
    /// Absent if ray geometry or projection prevented pixel-space optimization.
    pub refinement: Option<Refinement>,
    /// Checks in canonical frame order; empty when geometry was unobservable before solving.
    pub reprojection: Vec<Reprojection>,
    /// Hard gate failures, never silently trimmed as outliers.
    pub flags: Vec<String>,
    /// Non-gating limitations, including the absence of held-out observations.
    pub advisories: Vec<String>,
    /// The boundary of the inference.
    pub no_claim: &'static str,
}

fn text_present(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= 4096
}

fn check_capture(input: &TriangulationInput) -> Result<(), Refusal> {
    check_frames(
        &input.capture,
        &input
            .observations
            .iter()
            .map(|v| &v.frame)
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn check_frames(capture: &Capture, frames: &[&FrameId]) -> Result<(), Refusal> {
    let streams: BTreeSet<_> = frames.iter().map(|f| &f.stream).collect();
    match capture {
        Capture::StillWindow {
            start_us,
            end_us,
            evidence,
        } => {
            if !text_present(evidence) || start_us > end_us || streams.len() != 1 {
                return Err(invalid(
                    "still_window needs evidence, ordered bounds and exactly one stream",
                ));
            }
            if frames
                .iter()
                .any(|f| f.timestamp_us < *start_us || f.timestamp_us > *end_us)
            {
                return Err(invalid("observation is outside the reviewed still window"));
            }
        }
        Capture::Synchronized {
            max_skew_us,
            evidence,
        } => {
            if !text_present(evidence) || streams.len() != frames.len() {
                return Err(invalid(
                    "synchronized capture needs evidence and one frame per stream",
                ));
            }
            let first = frames.iter().map(|f| f.timestamp_us).min();
            let last = frames.iter().map(|f| f.timestamp_us).max();
            if let (Some(first), Some(last)) = (first, last)
                && last - first > *max_skew_us
            {
                return Err(invalid(
                    "exposure timestamps exceed the declared synchronization skew",
                ));
            }
        }
    }
    Ok(())
}

// Acute line angle, not directed angle: opposed or nearly opposed rays are also degenerate.
fn max_angle(centers: &[Vec3], rays: &[Vec3]) -> f64 {
    let mut angle: f64 = 0.0;
    for i in 0..rays.len() {
        for j in i + 1..rays.len() {
            if (centers[i] - centers[j]).norm() > 1e-8 {
                let radians = det::atan2(rays[i].cross(rays[j]).norm(), rays[i].dot(rays[j]).abs());
                angle = angle.max(radians * (180.0 / std::f64::consts::PI));
            }
        }
    }
    angle
}

struct PixelSystem {
    jacobian: Vec<[f64; 3]>,
    residual: Vec<f64>,
    cost: f64,
}

fn pixel_system(views: &[(Camera, Vec2)], point: Vec3) -> Result<PixelSystem, Refusal> {
    let mut system = PixelSystem {
        jacobian: Vec::with_capacity(2 * views.len()),
        residual: Vec::with_capacity(2 * views.len()),
        cost: 0.0,
    };
    for (camera, observed) in views {
        let (pixel, j) = camera.project_with_jacobian(point)?;
        let r = pixel - *observed;
        system.cost += r.norm2();
        system.jacobian.extend(j);
        system.residual.extend([r.x, r.y]);
    }
    if !system.cost.is_finite() {
        return Err(invalid("non-finite pixel refinement objective"));
    }
    Ok(system)
}

// Cameras and the starting point use the same normalized metric frame, avoiding a large world
// origin inside the iterative projection. Intrinsics, rotations and ALL fit views remain fixed.
fn refine(views: &[(Camera, Vec2)], mut point: Vec3) -> Result<(Vec3, Refinement), Refusal> {
    let mut system = pixel_system(views, point)?;
    let mut report = Refinement {
        initial_cost_px2: system.cost,
        final_cost_px2: system.cost,
        iterations: 0,
        converged: false,
    };
    for _ in 0..=30 {
        // Column-normalized stationarity is invariant to the metric unit. A short line-search
        // step alone is NOT convergence: at a lens boundary it could hide a nonzero gradient.
        let mut stationary = true;
        for k in 0..3 {
            let norm = system
                .jacobian
                .iter()
                .map(|j| j[k] * j[k])
                .sum::<f64>()
                .sqrt();
            let gradient: f64 = system
                .jacobian
                .iter()
                .zip(&system.residual)
                .map(|(j, r)| j[k] * r)
                .sum();
            if !norm.is_finite() || norm == 0.0 || !gradient.is_finite() {
                return Err(invalid("unusable pixel refinement Jacobian"));
            }
            stationary &= gradient.abs() / norm <= 1e-8 * system.cost.sqrt().max(1.0);
        }
        if stationary || system.cost <= 1e-16 {
            report.converged = true;
            break;
        }
        if report.iterations == 30 {
            break;
        }
        let rhs: Vec<_> = system.residual.iter().map(|r| -r).collect();
        let step = Vec3::from(lstsq(&system.jacobian, &rhs));
        if !step.is_finite() {
            return Err(invalid("non-finite pixel refinement step"));
        }
        let mut accepted = None;
        let mut alpha = 1.0;
        for _ in 0..24 {
            let trial = point + step * alpha;
            if let Ok(next) = pixel_system(views, trial)
                && next.cost < system.cost
            {
                accepted = Some((trial, next));
                break;
            }
            alpha *= 0.5;
        }
        let Some((next_point, next_system)) = accepted else {
            break;
        };
        point = next_point;
        system = next_system;
        report.iterations += 1;
        report.final_cost_px2 = system.cost;
    }
    Ok((point, report))
}

/// Initialize from calibrated rays in a normalized metric frame, then minimize equal-weight
/// squared pixel residuals using only fit views. Cameras and metric scale remain fixed; this is
/// point refinement, not joint bundle adjustment or a calibration-uncertainty estimate.
/// The fixed 3-degree parallax and 3-pixel per-view gates still apply, and no view is dropped.
///
/// Invalid documents/calibrations return a typed refusal. Valid but weak or inconsistent geometry
/// returns a diagnostic report with `point_m: null`. Uncertainty in cameras, synchronization and
/// metric scale is not estimated here; small reprojection errors do not establish metric accuracy.
pub fn triangulate(input: &TriangulationInput) -> Result<TriangulationReport, Refusal> {
    if input.schema != "ftx-triangulation-input/1"
        || !text_present(&input.metric_scale_source)
        || input.observations.is_empty()
        || input.observations.len() > 128
    {
        return Err(invalid(
            "triangulation needs schema /1, a metric-scale source and 1..128 views",
        ));
    }
    let mut input = input.clone();
    input.observations.sort_by(|a, b| a.frame.cmp(&b.frame));
    let mut previous = None;
    for view in &input.observations {
        if view.frame.stream.trim().is_empty()
            || view.frame.stream.len() > 256
            || previous == Some(&view.frame)
        {
            return Err(invalid(
                "each view needs a nonempty stream and a unique decoded timestamp",
            ));
        }
        view.camera.validate()?;
        view.camera.ray(view.pixel_px)?;
        previous = Some(&view.frame);
    }
    check_capture(&input)?;
    let fit: Vec<_> = input
        .observations
        .iter()
        .filter(|v| v.partition == Partition::Fit)
        .collect();
    let mut report = TriangulationReport {
        schema: "ftx-triangulation/1",
        status: "UNOBSERVABLE",
        point_m: None,
        input: input.clone(),
        fit_views: fit.len(),
        held_out_views: input.observations.len() - fit.len(),
        observed_max_angle_deg: 0.0,
        fitted_max_angle_deg: None,
        refinement: None,
        reprojection: Vec::new(),
        flags: Vec::new(),
        advisories: Vec::new(),
        no_claim: "Conditional on supplied calibration, metric scale and capture evidence; not a body fit or an uncertainty certificate.",
    };
    if report.held_out_views == 0 {
        report
            .advisories
            .push("No held-out views: reprojection is checked only on fitted observations".into());
    }
    if fit.len() < 2 {
        report.flags.push(
            "At least two fit views are required; held-out views cannot supply fitting geometry"
                .into(),
        );
        return Ok(report);
    }
    let origin = fit[0].camera.center_m;
    let scale = fit
        .iter()
        .map(|v| (v.camera.center_m - origin).norm())
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale == 0.0 {
        report
            .flags
            .push("No finite nonzero metric camera baseline".into());
        return Ok(report);
    }
    let centers: Vec<_> = fit
        .iter()
        .map(|v| (v.camera.center_m - origin) / scale)
        .collect();
    if centers.iter().any(|v| !v.is_finite()) {
        return Err(invalid("camera baseline cannot be represented finitely"));
    }
    let rays: Vec<_> = fit
        .iter()
        .map(|v| v.camera.ray(v.pixel_px))
        .collect::<Result<_, _>>()?;
    report.observed_max_angle_deg = max_angle(&centers, &rays);
    if report.observed_max_angle_deg < 3.0 {
        report
            .flags
            .push("Fit-ray angle is below 3 degrees; increase the camera baseline".into());
        return Ok(report);
    }
    // Stack d x X = d x C. The existing Jacobi SVD avoids squared conditioning. Parallax
    // supplies a full-rank pair; origin/scale normalization avoids cancellation at large origins.
    let mut rows = Vec::with_capacity(3 * fit.len());
    let mut rhs = Vec::with_capacity(3 * fit.len());
    for (&d, &center) in rays.iter().zip(&centers) {
        rows.extend([[0.0, -d.z, d.y], [d.z, 0.0, -d.x], [-d.y, d.x, 0.0]]);
        rhs.extend(<[f64; 3]>::from(d.cross(center)));
    }
    let mut normalized = Vec3::from(lstsq(&rows, &rhs));
    let point = origin + normalized * scale;
    if !point.is_finite() || !normalized.is_finite() {
        report.flags.push("No finite triangulation solution".into());
        return Ok(report);
    }
    let views: Vec<_> = fit
        .iter()
        .zip(&centers)
        .map(|(view, &center)| {
            (
                Camera {
                    center_m: center,
                    ..view.camera
                },
                view.pixel_px,
            )
        })
        .collect();
    match refine(&views, normalized) {
        Ok((candidate, refinement)) => {
            normalized = candidate;
            if !refinement.converged {
                report
                    .flags
                    .push("Pixel refinement did not converge; no usable point".into());
            }
            report.refinement = Some(refinement);
        }
        Err(error) => report
            .flags
            .push(format!("Pixel refinement unavailable: {error}")),
    }
    let point = origin + normalized * scale;
    if !point.is_finite() {
        report
            .flags
            .push("Refined world position cannot be represented finitely".into());
        return Ok(report);
    }
    let predicted: Vec<_> = centers
        .iter()
        .map(|&c| (normalized - c).normalized())
        .collect();
    let angle = max_angle(&centers, &predicted);
    report.fitted_max_angle_deg = Some(angle);
    report.status = "REJECTED";
    if angle < 3.0 {
        report.status = "UNOBSERVABLE";
        report
            .flags
            .push("Reconstructed parallax is below 3 degrees".into());
    }
    for view in &input.observations {
        let depth = view
            .camera
            .world_to_camera
            .mul_vec(point - view.camera.center_m)
            .z;
        let projected = view.camera.project(point);
        let mut check = Reprojection {
            frame: view.frame.clone(),
            partition: view.partition,
            error_px: None,
            depth_m: depth.is_finite().then_some(depth),
            failure: None,
        };
        match projected {
            Ok(pixel) => {
                let error = (pixel - view.pixel_px).norm();
                if error.is_finite() {
                    check.error_px = Some(error);
                }
                if !error.is_finite() || error > 3.0 {
                    report.flags.push(format!(
                        "{} at {} us: reprojection exceeds 3 pixels",
                        view.frame.stream, view.frame.timestamp_us
                    ));
                }
            }
            Err(error) => {
                check.failure = Some(error.to_string());
                report.flags.push(format!(
                    "{} at {} us: projection unavailable",
                    view.frame.stream, view.frame.timestamp_us
                ));
            }
        }
        report.reprojection.push(check);
    }
    if report.flags.is_empty() {
        report.status = "ACCEPTED";
        report.point_m = Some(point);
    }
    Ok(report)
}
