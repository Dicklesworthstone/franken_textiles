//! Shared pinhole intrinsics from multiple views of a metric plane (plan §10.2, FTX-20).
//! Zhang's orthogonality/equal-length constraints, with zero skew and B33 fixed to one:
//! MSR-TR-98-71 §2.3/3.1, https://www.microsoft.com/en-us/research/publication/a-flexible-new-technique-for-camera-calibration/ .
//! The default is algebraic pinhole calibration. Explicit `brown5_zero_skew` inputs also fit
//! radial/tangential distortion and refine intrinsics/poses in original pixel coordinates.

mod refine;
pub mod uncertainty;
pub use refine::{CornerNoise, DistortionFit, IntrinsicUncertainty, RefinementReport};

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Mat3, Refusal, RefusalCode, Vec2, Vec3, linalg::lstsq};
use serde::{Deserialize, Serialize};

use crate::{
    camera::{Camera, Distortion, Intrinsics},
    invalid,
    triangulate::{FrameId, Partition},
};

use super::geometry;

const MIN_PIVOT_RATIO: f64 = 1e-6;
const MAX_ERROR_PX: f64 = 3.0;

/// A known physical point on one rigid planar reference, in metres, not inferred body dimensions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    /// Reference-wide identifier; duplicated positions cannot supply extra geometric information.
    pub id: String,
    /// Sheet-local x/z axes, with its normal along y. The sheet can move between views.
    pub sheet_xz_m: Vec2,
}

/// An identified corner in a full-resolution, already oriented image.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corner {
    /// Identifier in `controls`.
    pub id: String,
    /// Pixels in the common image geometry; no resizing, cropping or rotation is done here.
    pub pixel_px: Vec2,
    /// Held-out corners influence neither the homography nor the camera pose or intrinsics.
    pub partition: Partition,
}

/// A view of the same planar reference through the same lens and image geometry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    /// Decoded identity, used for canonical order. All views must use one stream.
    pub frame: FrameId,
    /// Held-out views do not determine intrinsics; their fit corners determine only their own pose.
    pub partition: Partition,
    /// At least four fit corners must span the plane; not all controls have to be visible.
    pub corners: Vec<Corner>,
}

/// Same-lens zero-skew calibration. The default requires negligible residual distortion;
/// `brown5_zero_skew` explicitly estimates the shared Brown lens instead.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationInput {
    /// Must be `ftx-camera-calibration-input/1`.
    pub schema: String,
    /// `pinhole_zero_skew`, or `brown5_zero_skew` with explicit `distortion_fit` options.
    pub image_model: String,
    /// Lens/settings identifier. Different lenses, zooms, crops and focus settings need separate fits.
    pub lens_id: String,
    /// Receipt explaining why the supplied pixels follow a pinhole model (e.g. rectified images).
    /// Omitted for Brown fitting, whose model assumptions belong in `distortion_fit.model_evidence`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pinhole_evidence: String,
    /// Optional nonlinear Brown fit. Absent fields retain the original pinhole behavior and JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distortion_fit: Option<DistortionFit>,
    /// Receipt identifying the rigid, flat reference and the source of its physical dimensions.
    pub reference_evidence: String,
    /// One unchanged width/height for the entire acquisition; principal point must lie inside it.
    pub image_size_px: [u32; 2],
    /// Clockwise rotation already applied consistently to all input images.
    pub rotation_cw_deg: u16,
    /// 4–128 distinct reference controls.
    pub controls: Vec<Control>,
    /// 1–32 views. At least three independent fit views are required for accepted calibration.
    pub views: Vec<View>,
}

/// Per-corner residual, including observations that never steered the fit.
#[derive(Clone, Debug, Serialize)]
pub struct CornerCheck {
    /// Original control identifier.
    pub id: String,
    /// Fit versus independently held-out corner.
    pub partition: Partition,
    /// Unweighted Euclidean pixel residual; absent when the view has no valid projection.
    pub error_px: Option<f64>,
}

/// A view's geometric and reprojection checks, without publishing a failed camera.
#[derive(Clone, Debug, Serialize)]
pub struct ViewCheck {
    /// Original decoded identity.
    pub frame: FrameId,
    /// Whether the view contributed to shared intrinsics.
    pub partition: Partition,
    /// Every original corner in identifier order.
    pub corners: Vec<CornerCheck>,
    /// Pose or projection refusal, when present.
    pub failure: Option<String>,
}

/// One fitted pose relative to the calibration sheet, NOT necessarily a shared world frame.
#[derive(Clone, Debug, Serialize)]
pub struct SheetPose {
    /// Decoded view identity.
    pub frame: FrameId,
    /// Pose recovered from this view's fit corners with accepted shared intrinsics.
    pub camera_in_sheet_frame: Camera,
}

/// Published only when all fit and held-out views pass.
#[derive(Clone, Debug, Serialize)]
pub struct CalibrationSolution {
    /// Compatible with the existing sheet-pose, triangulation and body-fit camera representation.
    pub intrinsics: Intrinsics,
    /// A moved calibration sheet makes these different physical coordinate frames.
    pub sheet_poses: Vec<SheetPose>,
}

/// Calibration receipt. A small residual is not an uncertainty or real-wearer accuracy certificate.
#[derive(Clone, Debug, Serialize)]
pub struct CalibrationReport {
    /// Optional declared-noise covariance from the shared explicit-noise API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional_uncertainty: Option<uncertainty::CalibrationUncertainty>,
    /// `ftx-camera-calibration/1`.
    pub schema: &'static str,
    /// ACCEPTED, REJECTED or UNOBSERVABLE.
    pub status: &'static str,
    /// Accepted intrinsics and sheet-local poses, or null on any failed gate.
    pub solution: Option<CalibrationSolution>,
    /// Canonical input, including all evidence and held-out assignments.
    pub input: CalibrationInput,
    /// Smallest/largest residual column norm in pivoted, reorthogonalized QR. Not an SVD condition number.
    pub constraint_pivot_ratio: Option<f64>,
    /// Minimum required QR pivot ratio on the normalized fit-only constraints.
    pub min_constraint_pivot_ratio: f64,
    /// Squared algebraic residual of the normalized intrinsic constraints (fit views only).
    pub constraint_squared_residual: Option<f64>,
    /// Fixed per-corner reprojection gate, independent of the number of observations.
    pub max_error_px: f64,
    /// Checks for all views once intrinsics could be formed. Never trims outliers.
    pub checks: Vec<ViewCheck>,
    /// All hard gate failures.
    pub flags: Vec<String>,
    /// Missing independent checks and physical/model limitations.
    pub advisories: Vec<String>,
    /// Fit-only pixel objective and convergence diagnostics for explicit Brown fitting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refinement: Option<RefinementReport>,
    /// Conditional fit-only intrinsic covariance, only after every calibration gate passes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<IntrinsicUncertainty>,
    /// Scope of the estimate.
    pub no_claim: &'static str,
}

fn name(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256
}
fn evidence(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 4096
}

fn prepare(input: &CalibrationInput) -> Result<CalibrationInput, Refusal> {
    match (&input.distortion_fit, input.image_model.as_str()) {
        (None, "pinhole_zero_skew") if evidence(&input.pinhole_evidence) => {}
        (Some(options), "brown5_zero_skew") if input.pinhole_evidence.is_empty() => {
            options.validate()?;
        }
        _ => {
            return Err(invalid(
                "select pinhole_zero_skew with pinhole_evidence, or brown5_zero_skew with distortion_fit; never mix rectified and raw-lens assumptions",
            ));
        }
    }
    if input.controls.len() > 128
        || input.views.len() > 32
        || input.views.iter().any(|v| v.corners.len() > 128)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "calibration permits at most 128 controls, 32 views and 128 corners per view",
        ));
    }
    if input.schema != "ftx-camera-calibration-input/1"
        || !name(&input.lens_id)
        || !evidence(&input.reference_evidence)
        || input.controls.len() < 4
        || input.views.is_empty()
        || input
            .image_size_px
            .iter()
            .any(|n| !(16..=1_000_000).contains(n))
        || ![0, 90, 180, 270].contains(&input.rotation_cw_deg)
    {
        return Err(invalid(
            "calibration needs schema /1, explicit pinhole/lens/reference evidence, a valid oriented image and at least four controls",
        ));
    }
    let mut input = input.clone();
    input.controls.sort_by(|a, b| a.id.cmp(&b.id));
    let mut ids = BTreeSet::new();
    for (i, p) in input.controls.iter().enumerate() {
        if !name(&p.id)
            || !ids.insert(&p.id)
            || !p.sheet_xz_m.is_finite()
            || p.sheet_xz_m.x.abs().max(p.sheet_xz_m.y.abs()) > 100.0
            || input.controls[..i]
                .iter()
                .any(|q| p.sheet_xz_m == q.sheet_xz_m)
        {
            return Err(invalid(
                "calibration controls need unique IDs and distinct finite sheet coordinates within +/-100 metres",
            ));
        }
    }
    input.views.sort_by(|a, b| a.frame.cmp(&b.frame));
    let stream = input.views[0].frame.stream.clone();
    let mut frames = BTreeSet::new();
    for view in &mut input.views {
        if !name(&view.frame.stream)
            || view.frame.stream != stream
            || !frames.insert(view.frame.clone())
        {
            return Err(invalid(
                "calibration needs unique decoded frame identities from one lens stream",
            ));
        }
        view.corners.sort_by(|a, b| a.id.cmp(&b.id));
        let mut seen = BTreeSet::new();
        for (i, point) in view.corners.iter().enumerate() {
            if !ids.contains(&point.id)
                || !seen.insert(&point.id)
                || !point.pixel_px.is_finite()
                || point.pixel_px.x < 0.0
                || point.pixel_px.y < 0.0
                || point.pixel_px.x > f64::from(input.image_size_px[0] - 1)
                || point.pixel_px.y > f64::from(input.image_size_px[1] - 1)
                || view.corners[..i]
                    .iter()
                    .any(|p| p.pixel_px == point.pixel_px)
            {
                return Err(invalid(
                    "calibration corners need unique known controls and distinct pixels inside the oriented image",
                ));
            }
        }
    }
    Ok(input)
}

// Pivoted twice-reorthogonalized MGS on just four columns. Keep unscaled column norms:
// rescaling an almost-zero perspective column would manufacture observability in frontal views.
fn pivot_ratio(rows: &[[f64; 4]]) -> f64 {
    let mut columns: [Vec<f64>; 4] = std::array::from_fn(|j| rows.iter().map(|r| r[j]).collect());
    let mut largest: f64 = 0.0;
    let mut smallest = f64::INFINITY;
    for i in 0..4 {
        let norms: Vec<_> = columns
            .iter()
            .map(|c| c.iter().map(|v| v * v).sum::<f64>().sqrt())
            .collect();
        let p = (i..4)
            .max_by(|&a, &b| norms[a].total_cmp(&norms[b]).then(b.cmp(&a)))
            .unwrap();
        let norm = norms[p];
        largest = largest.max(norm);
        smallest = smallest.min(norm);
        if !norm.is_finite() || norm == 0.0 {
            return 0.0;
        }
        columns.swap(i, p);
        for v in &mut columns[i] {
            *v /= norm;
        }
        for j in i + 1..4 {
            for _ in 0..2 {
                let dot: f64 = columns[i].iter().zip(&columns[j]).map(|(a, b)| a * b).sum();
                for k in 0..rows.len() {
                    columns[j][k] -= dot * columns[i][k];
                }
            }
        }
    }
    smallest / largest
}

fn conic_row(a: Vec3, b: Vec3) -> [f64; 5] {
    [
        a.x * b.x,
        a.y * b.y,
        a.x * b.z + a.z * b.x,
        a.y * b.z + a.z * b.y,
        a.z * b.z,
    ]
}

/// Estimate fx, fy, cx and cy from three or more tilted fit views. Uses no body-size prior, lens
/// guess, RANSAC or outlier dropping. Held-out corners never enter any fit; held-out views estimate
/// only their own sheet pose after intrinsics have been fixed. All errors must be <=3 pixels.
///
/// Pinhole/zero-skew is an explicit assumption, not inferred from passing residuals. Negligible
/// distortion, flatness, focus stability and accurate physical controls remain caller obligations.
/// Near-degenerate data gives UNOBSERVABLE. An inconsistent fit gives REJECTED. Neither publishes
/// intrinsics or poses. The default estimates neither distortion nor uncertainty. Explicit Brown
/// inputs use this algebraic start followed by joint pixel-error refinement; require four fit
/// views and an independent held-out view, with twelve fit/four held-out corners per view.
pub fn calibrate_camera(input: &CalibrationInput) -> Result<CalibrationReport, Refusal> {
    let input = prepare(input)?;
    let mut report = CalibrationReport {
        conditional_uncertainty: None,
        schema: "ftx-camera-calibration/1",
        status: "UNOBSERVABLE",
        solution: None,
        input: input.clone(),
        constraint_pivot_ratio: None,
        min_constraint_pivot_ratio: MIN_PIVOT_RATIO,
        constraint_squared_residual: None,
        max_error_px: MAX_ERROR_PX,
        checks: Vec::new(),
        flags: Vec::new(),
        advisories: Vec::new(),
        refinement: None,
        uncertainty: None,
        no_claim: if input.distortion_fit.is_some() {
            refine::NO_CLAIM
        } else {
            "Algebraic zero-skew pinhole calibration, conditional on supplied correspondences and lens/reference assumptions; not distortion estimation, an uncertainty certificate or proof of metric accuracy. Sheet-local poses are not a shared world frame when the sheet moves."
        },
    };
    if !input
        .views
        .iter()
        .any(|v| v.partition == Partition::HeldOut)
    {
        report
            .advisories
            .push("No held-out view: all views contribute to shared intrinsics".into());
    }
    if input
        .views
        .iter()
        .any(|v| !v.corners.iter().any(|p| p.partition == Partition::HeldOut))
    {
        report.advisories.push(
            "Some views have no held-out corners; their pose checks reuse fitted observations"
                .into(),
        );
    }
    let controls: BTreeMap<_, _> = input
        .controls
        .iter()
        .map(|p| (p.id.as_str(), p.sheet_xz_m))
        .collect();
    let pairs: Vec<(Vec<Vec2>, Vec<Vec2>)> = input
        .views
        .iter()
        .map(|v| {
            v.corners
                .iter()
                .filter(|p| p.partition == Partition::Fit)
                .map(|p| (controls[p.id.as_str()], p.pixel_px))
                .unzip()
        })
        .collect();
    let fits = input
        .views
        .iter()
        .filter(|v| v.partition == Partition::Fit)
        .count();
    if fits < 3 {
        report.flags.push("At least three fit views at different plane tilts are required; held-out views cannot supply calibration constraints".into());
        return Ok(report);
    }
    let image_center = Vec2::new(
        f64::from(input.image_size_px[0] - 1) * 0.5,
        f64::from(input.image_size_px[1] - 1) * 0.5,
    );
    let image_scale = f64::from(input.image_size_px[0].max(input.image_size_px[1]));
    let (mut rows, mut rhs) = (Vec::new(), Vec::new());
    for (view, (sheet, pixels)) in input.views.iter().zip(&pairs) {
        if view.partition == Partition::HeldOut {
            continue;
        }
        let src: Vec<_> = sheet.iter().map(|p| (*p).into()).collect();
        let dst: Vec<_> = pixels
            .iter()
            .map(|p| ((*p - image_center) / image_scale).into())
            .collect();
        let h = match geometry::fit(&src, &dst) {
            Ok(h) => Mat3::from_rows(h.0),
            Err(e) => {
                report.flags.push(format!(
                    "{} at {} us: {}",
                    view.frame.stream, view.frame.timestamp_us, e
                ));
                return Ok(report);
            }
        };
        let scale = h.col(0).norm().max(h.col(1).norm());
        if !scale.is_finite() || scale == 0.0 {
            report
                .flags
                .push("No finite calibration plane homography".into());
            return Ok(report);
        }
        // One common normalization for both columns preserves their equal-length constraint.
        let (a, b) = (h.col(0) / scale, h.col(1) / scale);
        let aa = conic_row(a, a);
        let bb = conic_row(b, b);
        for row in [conic_row(a, b), std::array::from_fn(|i| aa[i] - bb[i])] {
            rows.push([row[0], row[1], row[2], row[3]]);
            rhs.push(-row[4]);
        }
    }
    let ratio = pivot_ratio(&rows);
    report.constraint_pivot_ratio = Some(ratio);
    if ratio < MIN_PIVOT_RATIO {
        report.flags.push("Plane orientations do not observably determine intrinsics; use different tilts, not only translations or in-plane rotations".into());
        return Ok(report);
    }
    let [a, b, c, d] = lstsq(&rows, &rhs);
    let schur = 1.0 - c * c / a - d * d / b;
    report.status = "REJECTED";
    if [a, b, c, d, schur].iter().any(|v| !v.is_finite()) || a <= 0.0 || b <= 0.0 || schur <= 0.0 {
        report
            .flags
            .push("Calibration constraints do not yield a positive-definite pinhole camera".into());
        return Ok(report);
    }
    let intrinsics = Intrinsics {
        focal_px: Vec2::new(
            (schur / a).sqrt() * image_scale,
            (schur / b).sqrt() * image_scale,
        ),
        principal_px: image_center + Vec2::new(-c / a, -d / b) * image_scale,
        distortion: Distortion::default(),
    };
    if intrinsics.validate().is_err()
        || intrinsics.principal_px.x < 0.0
        || intrinsics.principal_px.y < 0.0
        || intrinsics.principal_px.x > f64::from(input.image_size_px[0] - 1)
        || intrinsics.principal_px.y > f64::from(input.image_size_px[1] - 1)
    {
        report.flags.push("Estimated focal lengths are invalid or principal point lies outside the image; check the pinhole assumption and image geometry".into());
        return Ok(report);
    }
    report.constraint_squared_residual = Some(
        rows.iter()
            .zip(&rhs)
            .map(|(r, y)| {
                let e = r[0] * a + r[1] * b + r[2] * c + r[3] * d - y;
                e * e
            })
            .sum(),
    );
    if input.distortion_fit.is_some() {
        refine::calibrate(&mut report, intrinsics)?;
        return Ok(report);
    }
    let mut poses = Vec::new();
    for (view, (sheet, pixels)) in input.views.iter().zip(&pairs) {
        let mut check = ViewCheck {
            frame: view.frame.clone(),
            partition: view.partition,
            corners: Vec::new(),
            failure: None,
        };
        let camera = Camera::from_sheet(intrinsics, sheet, pixels);
        for point in &view.corners {
            let p = controls[point.id.as_str()];
            let projected = camera
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|c| c.project(Vec3::new(p.x, 0.0, p.y)));
            let error = match projected {
                Ok(p) => {
                    let error = p.dist(point.pixel_px);
                    error.is_finite().then_some(error)
                }
                Err(e) => {
                    check.failure = Some(e.to_string());
                    None
                }
            };
            if error.is_none_or(|e| e > MAX_ERROR_PX) {
                report.flags.push(format!(
                    "{} at {} us, {}: no valid projection within 3 pixels",
                    view.frame.stream, view.frame.timestamp_us, point.id
                ));
            }
            check.corners.push(CornerCheck {
                id: point.id.clone(),
                partition: point.partition,
                error_px: error,
            });
        }
        match camera {
            Ok(c) => poses.push(SheetPose {
                frame: view.frame.clone(),
                camera_in_sheet_frame: c,
            }),
            Err(e) => {
                check.failure = Some(e.to_string());
                report.flags.push(format!(
                    "{} at {} us: sheet pose unavailable",
                    view.frame.stream, view.frame.timestamp_us
                ));
            }
        }
        report.checks.push(check);
    }
    if report.flags.is_empty() {
        report.status = "ACCEPTED";
        report.solution = Some(CalibrationSolution {
            intrinsics,
            sheet_poses: poses,
        });
    }
    Ok(report)
}
