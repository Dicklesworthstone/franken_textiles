//! Independent sheet-corner noise through source-replayed camera poses and triangulation.
//! Lens parameters and metric reference coordinates stay fixed; all camera pose coordinates
//! respond together to each original fitted corner rather than receiving invented pose sigmas.

use ftx_core::{Mat3, Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::{FrameId, Partition, TriangulationInput, TriangulationReport};
use super::sheet::{FrameSheetPose, PreparedSheetPose, PreparedSheetPoses, prepare_sheet_poses};
use crate::invalid;

/// Original camera-pose acquisition and declared isotropic fitted-corner noise for one frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameSheetCornerNoise {
    /// Exact fitting measurement frame, never a held-out or unknown frame.
    pub frame: FrameId,
    /// Complete accepted `ftx-sheet-pose/1` report, replayed against the measurement camera.
    pub report: Value,
    /// Independent x/y standard deviation for every FIT sheet corner in this image, in
    /// oriented pixels. Finite and in (0, 1000]; not inferred from a reprojection residual.
    pub sigma_px: f64,
}

/// Explicit independence between sheet corners, frames, lens calibration and measurement clicks.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetCornerNoiseInput {
    /// Must be `ftx-triangulation-sheet-noise/1`.
    pub schema: String,
    /// Source/method and basis for independent isotropic fitted-corner errors in each frame,
    /// independent frames, and independence from calibration and measurement clicks.
    pub independence_evidence: String,
    /// Exactly one entry per FIT frame. Across frames, at most 256 FIT and 1024 total retained
    /// sheet corners are admitted; CHECK sheet points are replayed but never differentiated.
    pub observations: Vec<FrameSheetCornerNoise>,
}

/// Complete contribution from one original sheet acquisition in canonical fitting-frame order.
#[derive(Clone, Debug, Serialize)]
pub struct SheetFrameSensitivity {
    /// Source fitting frame.
    pub frame: FrameId,
    /// Declared independent isotropic standard deviation for each fitted corner coordinate.
    pub sigma_px: f64,
    /// SHA-256 of canonical parsed sheet report, distinct from original file-byte provenance.
    pub sheet_pose_sha256: String,
    /// Original `input.points` indices, preserving FIT/CHECK separation and source ordering.
    pub fit_point_indices: Vec<usize>,
    /// One 3-by-2 Jacobian per FIT corner, world x/y/z by oriented pixel x/y, metres per pixel.
    pub jacobians_m_per_px: Vec<[[f64; 2]; 3]>,
    /// This independent frame's complete covariance contribution, in square metres.
    pub covariance_m2: Mat3,
}

/// Conditional first-order camera-pose precision from explicitly declared sheet-corner noise.
#[derive(Clone, Debug, Serialize)]
pub struct SheetUncertaintyReport {
    /// `ftx-triangulation-sheet-uncertainty/1`.
    pub schema: &'static str,
    /// AVAILABLE or UNAVAILABLE. No partial or zero covariance substitutes for numerical failure.
    pub status: &'static str,
    /// Complete retained evidence in canonical fitting-frame order.
    pub input: SheetCornerNoiseInput,
    /// SHA-256 of the canonical parsed input, not the caller's original file bytes.
    pub input_sha256: String,
    /// Full symmetric world covariance in square metres; absent unless all frames succeed.
    pub covariance_m2: Option<Mat3>,
    /// Marginal one-sigma x/y/z contributions in metres, not simultaneous confidence bounds.
    pub sigma_m: Option<Vec3>,
    /// Per-frame source, corner sensitivities and covariance; empty when unavailable.
    pub sensitivities: Vec<SheetFrameSensitivity>,
    /// Numerical or original-geometry reasons the requested precision is unavailable.
    pub flags: Vec<String>,
    /// Assumptions and excluded uncertainty sources.
    pub no_claim: &'static str,
}

/// Unchanged triangulation geometry and its separately requested sheet-corner contribution.
#[derive(Clone, Debug, Serialize)]
pub struct SheetUncertainTriangulationReport {
    /// Original point, source, residuals and all gates. The noise model never changes the fit.
    #[serde(flatten)]
    pub triangulation: TriangulationReport,
    /// Conditional source-pose contribution, not total measurement uncertainty.
    pub conditional_sheet_uncertainty: SheetUncertaintyReport,
}

fn canonical_bytes(value: &impl Serialize) -> Result<Vec<u8>, Refusal> {
    serde_json::to_vec(value).map_err(|error| invalid(error.to_string()))
}

fn prepare(
    input: &TriangulationInput,
    noise: &SheetCornerNoiseInput,
) -> Result<(SheetCornerNoiseInput, PreparedSheetPoses), Refusal> {
    if noise.schema != "ftx-triangulation-sheet-noise/1"
        || !super::super::text_present(&noise.independence_evidence)
        || noise.observations.is_empty()
        || noise.observations.len() > 128
        || noise.observations.iter().any(|frame| {
            !frame.sigma_px.is_finite() || frame.sigma_px <= 0. || frame.sigma_px > 1000.
        })
    {
        return Err(invalid(
            "sheet noise needs schema /1, explicit independent-corner evidence, 1–128 FIT frames and finite corner sigmas in (0, 1000]",
        ));
    }
    if canonical_bytes(noise)?.len() > 16 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "retained sheet-noise evidence exceeds 16 MiB",
        ));
    }
    let mut sorted = noise.clone();
    sorted.observations.sort_by(|a, b| a.frame.cmp(&b.frame));
    let poses: Vec<_> = sorted
        .observations
        .iter()
        .map(|frame| FrameSheetPose {
            frame: frame.frame.clone(),
            report: frame.report.clone(),
        })
        .collect();
    let prepared = prepare_sheet_poses(input, &poses)?;
    Ok((sorted, prepared))
}

struct Differential<'a> {
    input: &'a TriangulationInput,
    pose: &'a PreparedSheetPose,
    frame_index: usize,
    origin: Vec3,
    scale: f64,
}

impl Differential<'_> {
    fn derivative(&self, corner: usize, axis: usize, step: f64) -> Result<Vec3, Refusal> {
        let mut plus = self.pose.pixels[corner];
        let mut minus = plus;
        let width = if axis == 0 {
            plus.x += step;
            minus.x -= step;
            plus.x - minus.x
        } else {
            plus.y += step;
            minus.y -= step;
            plus.y - minus.y
        };
        if !width.is_finite() || (width / (2. * step) - 1.).abs() > 1e-3 {
            return Err(super::unavailable(
                "sheet pixels cannot resolve the differential step",
            ));
        }
        self.pose.check_pixel(plus)?;
        self.pose.check_pixel(minus)?;
        let sample = |pixel| {
            let mut pixels = self.pose.pixels.clone();
            pixels[corner] = pixel;
            let camera = self.pose.normalized_camera(
                self.pose.intrinsics,
                &pixels,
                self.origin,
                self.scale,
            )?;
            let mut trial = self.input.clone();
            trial.observations[self.frame_index].camera = camera;
            let report = super::super::triangulate(&trial)?;
            report.point_m.ok_or_else(|| {
                super::unavailable(format!(
                    "sheet differential triangulation did not pass its gates: {}",
                    report.flags.join("; "),
                ))
            })
        };
        let result = (sample(plus)? - sample(minus)?) / width;
        if !result.is_finite() {
            return Err(super::unavailable("non-finite sheet-corner sensitivity"));
        }
        Ok(result)
    }
}

fn propagate(
    input: &TriangulationInput,
    noise: &SheetCornerNoiseInput,
    prepared: &PreparedSheetPoses,
) -> Result<(Mat3, Vec3, Vec<SheetFrameSensitivity>), Refusal> {
    let mut normalized = input.clone();
    normalized
        .observations
        .retain(|view| view.partition == Partition::Fit);
    let origin = normalized.observations[0].camera.center_m;
    let scale = normalized
        .observations
        .iter()
        .map(|view| (view.camera.center_m - origin).norm())
        .fold(0., f64::max);
    if !scale.is_finite() || scale <= 0. {
        return Err(super::unavailable(
            "no finite metric baseline for sheet-corner propagation",
        ));
    }
    for view in &mut normalized.observations {
        view.camera.center_m = (view.camera.center_m - origin) / scale;
    }
    let mut covariance = [[0.; 3]; 3];
    let mut sensitivities = Vec::with_capacity(noise.observations.len());
    for (frame_index, frame) in noise.observations.iter().enumerate() {
        let pose = prepared.pose(&frame.frame)?;
        let differential = Differential {
            input: &normalized,
            pose,
            frame_index,
            origin,
            scale,
        };
        let mut component = [[0.; 3]; 3];
        let mut jacobians = Vec::with_capacity(pose.pixels.len());
        for corner in 0..pose.pixels.len() {
            let mut columns = [Vec3::ZERO; 2];
            for (axis, column) in columns.iter_mut().enumerate() {
                let coarse = differential.derivative(corner, axis, 0.01)?;
                let fine = differential.derivative(corner, axis, 0.005)?;
                let change = (coarse - fine).norm();
                let magnitude = fine.norm().max(1e-12);
                if !change.is_finite() || !magnitude.is_finite() || change > 1e-3 * magnitude {
                    return Err(super::unavailable(
                        "sheet-corner sensitivity did not converge across differential steps",
                    ));
                }
                *column = fine * scale;
                let weighted = *column * frame.sigma_px;
                if !column.is_finite()
                    || !weighted.is_finite()
                    || (0..3).any(|i| column[i] != 0. && weighted[i] == 0.)
                {
                    return Err(super::unavailable(
                        "world sheet-corner sensitivity is not representable",
                    ));
                }
                for (i, row) in component.iter_mut().enumerate() {
                    for (j, value) in row.iter_mut().enumerate() {
                        *value += weighted[i] * weighted[j];
                    }
                }
            }
            jacobians.push(std::array::from_fn(|i| [columns[0][i], columns[1][i]]));
        }
        if component.iter().flatten().any(|value| !value.is_finite())
            || (0..3).any(|i| component[i][i] < 0.)
            || (0..3).all(|i| component[i][i] == 0.)
        {
            return Err(super::unavailable(
                "sheet covariance overflowed or underflowed; no partial covariance was published",
            ));
        }
        for (row, part) in covariance.iter_mut().zip(component) {
            for (value, contribution) in row.iter_mut().zip(part) {
                *value += contribution;
            }
        }
        sensitivities.push(SheetFrameSensitivity {
            frame: frame.frame.clone(),
            sigma_px: frame.sigma_px,
            sheet_pose_sha256: sha256_hex(&canonical_bytes(&frame.report)?),
            fit_point_indices: pose.fit_point_indices.clone(),
            jacobians_m_per_px: jacobians,
            covariance_m2: Mat3::from_rows(component),
        });
    }
    if covariance.iter().flatten().any(|value| !value.is_finite()) {
        return Err(super::unavailable("total sheet covariance overflowed"));
    }
    let sigma = Vec3::new(
        covariance[0][0].sqrt(),
        covariance[1][1].sqrt(),
        covariance[2][2].sqrt(),
    );
    Ok((Mat3::from_rows(covariance), sigma, sensitivities))
}

/// Replay exact source poses and propagate every original FIT sheet-corner coordinate through
/// the same pose and pixel-refined triangulation estimators. The parent point and gates remain
/// unchanged. Two checked differential steps distinguish available precision from numerical
/// failure; held-out sheet and landmark observations never enter these derivatives.
///
/// # Errors
/// Invalid, incomplete, changed, mismatched or oversized source/noise evidence is refused.
/// Rejected parent geometry or an unstable differential instead retains the original geometry
/// report with UNAVAILABLE conditional precision, null covariance and no partial sensitivities.
pub fn triangulate_with_sheet_noise(
    input: &TriangulationInput,
    noise: &SheetCornerNoiseInput,
) -> Result<SheetUncertainTriangulationReport, Refusal> {
    let triangulation = super::super::triangulate(input)?;
    let (input, prepared) = prepare(&triangulation.input, noise)?;
    let mut report = SheetUncertaintyReport {
        schema: "ftx-triangulation-sheet-uncertainty/1",
        status: "UNAVAILABLE",
        input_sha256: sha256_hex(&canonical_bytes(&input)?),
        input,
        covariance_m2: None,
        sigma_m: None,
        sensitivities: Vec::new(),
        flags: Vec::new(),
        no_claim: "First-order conditional camera-pose contribution from declared independent isotropic FIT sheet-corner errors, independent between frames and from lens calibration and measurement clicks. The original pose estimator and triangulation are re-solved with fixed intrinsics and exact metric reference coordinates. Excludes lens uncertainty, measurement-click noise, reference print/flatness/scale error, synchronization, motion, correspondence, lens drift and other systematic errors. Independence is declared, not physically established. Not total measurement uncertainty, real-camera accuracy or body/garment fit.",
    };
    if triangulation.point_m.is_none() {
        report
            .flags
            .push("Triangulation did not publish an accepted point".into());
    } else {
        match propagate(&triangulation.input, &report.input, &prepared) {
            Ok((covariance, sigma, sensitivities)) => {
                report.status = "AVAILABLE";
                report.covariance_m2 = Some(covariance);
                report.sigma_m = Some(sigma);
                report.sensitivities = sensitivities;
            }
            Err(error) => report.flags.push(error.to_string()),
        }
    }
    Ok(SheetUncertainTriangulationReport {
        triangulation,
        conditional_sheet_uncertainty: report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        camera::{Camera, Distortion, Intrinsics},
        triangulate::{Capture, Observation},
    };
    use ftx_core::Vec2;
    use serde_json::json;

    fn fixture(
        point: Vec3,
        scale: f64,
        shift: Vec2,
    ) -> (TriangulationInput, SheetCornerNoiseInput) {
        let intrinsics = Intrinsics {
            focal_px: Vec2::new(800., 800.),
            principal_px: Vec2::new(800., 600.),
            distortion: Distortion::default(),
        };
        let mut observations = Vec::new();
        let mut sources = Vec::new();
        for (i, x) in [-0.5, 0.5, 0.].into_iter().enumerate() {
            let known = Camera {
                intrinsics,
                world_to_camera: Mat3::from_rows([[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]]),
                center_m: Vec3::new(x * scale + shift.x, -2. * scale, shift.y),
            };
            let corners: Vec<_> = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5), (0.1, 0.2)]
                .into_iter()
                .enumerate()
                .map(|(index, (x, z))| {
                    let world = Vec3::new(x * scale + shift.x, 0., z * scale + shift.y);
                    json!({"sheet_xz_m":[world.x, world.z],"pixel_px":known.project(world).unwrap(),"held_out":index == 4})
                })
                .collect();
            let source = json!({"schema":"ftx-sheet-pose-input/1","reference_id":"synthetic-stationary-square",
                "intrinsics":intrinsics,"image_size_px":[1600,1200],"rotation_applied_degrees":0,
                "points":corners});
            let report = Camera::sheet_pose_report(&source).unwrap();
            assert_eq!(report["ok"], true);
            let camera: Camera = serde_json::from_value(report["camera"].clone()).unwrap();
            let frame = FrameId {
                stream: format!("synthetic-sheet-{i}"),
                timestamp_us: 1_000_000,
            };
            observations.push(Observation {
                frame: frame.clone(),
                camera,
                pixel_px: camera
                    .project(point * scale + Vec3::new(shift.x, 0., shift.y))
                    .unwrap(),
                partition: if i == 2 {
                    Partition::HeldOut
                } else {
                    Partition::Fit
                },
            });
            if i != 2 {
                sources.push(FrameSheetCornerNoise {
                    frame,
                    report,
                    sigma_px: 1.,
                });
            }
        }
        (
            TriangulationInput {
                schema: "ftx-triangulation-input/1".into(),
                metric_scale_source: "Synthetic common stationary square in metres; exact scale".into(),
                capture: Capture::Synchronized {
                    max_skew_us: 0,
                    evidence: "Synthetic simultaneous stationary views".into(),
                },
                observations,
            },
            SheetCornerNoiseInput {
                schema: "ftx-triangulation-sheet-noise/1".into(),
                independence_evidence: "Synthetic independent sheet corner coordinates and exposures, independent of landmark clicks and calibration".into(),
                observations: sources,
            },
        )
    }

    fn available(
        input: &TriangulationInput,
        noise: &SheetCornerNoiseInput,
    ) -> SheetUncertaintyReport {
        let result = triangulate_with_sheet_noise(input, noise).unwrap();
        assert_eq!(result.triangulation.status, "ACCEPTED");
        assert_eq!(
            result.conditional_sheet_uncertainty.status, "AVAILABLE",
            "{:?}",
            result.conditional_sheet_uncertainty.flags,
        );
        result.conditional_sheet_uncertainty
    }

    fn assert_scaled(actual: Mat3, expected: Mat3, factor: f64, tolerance: f64) {
        let norm = expected
            .rows
            .iter()
            .flatten()
            .map(|v| v.abs())
            .fold(0., f64::max)
            * factor;
        for (actual, expected) in actual
            .rows
            .iter()
            .flatten()
            .zip(expected.rows.iter().flatten())
        {
            assert!(
                (actual - expected * factor).abs() < tolerance * norm,
                "{actual} != {expected} * {factor}"
            );
        }
    }

    #[test]
    fn square_center_matches_independent_analytic_corner_covariance() {
        let (input, noise) = fixture(Vec3::ZERO, 1., Vec2::ZERO);
        let result = triangulate_with_sheet_noise(&input, &noise).unwrap();
        let report = &result.conditional_sheet_uncertainty;
        assert_eq!(report.status, "AVAILABLE", "{:?}", report.flags);
        // At the center of a square, first-order normalized homography translations are
        // mean(du) + mean(sign(x*z)*dv) and mean(dv) + mean(sign(x*z)*du). Each therefore
        // has variance sigma²/(2*f²), with zero cross covariance. Calibrated pose recovery
        // leaves these center projections unchanged to first order. Stereo then gives the
        // ordinary f=800, baseline=1, depth=2 covariance with half the one-pixel variance.
        let expected =
            Mat3::from_rows([[1.5625e-6, 0., 0.], [0., 2.5e-5, 0.], [0., 0., 1.5625e-6]]);
        assert_scaled(report.covariance_m2.unwrap(), expected, 1., 1e-5);
        assert_eq!(report.sensitivities.len(), 2);
        for frame in &report.sensitivities {
            assert_eq!(frame.fit_point_indices, [0, 1, 2, 3]);
            assert_eq!(frame.jacobians_m_per_px.len(), 4);
        }
        assert_eq!(
            json!(result.triangulation),
            json!(super::super::super::triangulate(&input).unwrap()),
        );
    }

    #[test]
    fn off_sheet_noise_and_metric_scale_preserve_covariance_laws() {
        let point = Vec3::new(0.08, -0.2, 0.15);
        let (input, mut noise) = fixture(point, 1., Vec2::ZERO);
        let base = available(&input, &noise).covariance_m2.unwrap();
        assert!(base.rows[0][1].abs() > 1e-10);
        for frame in &mut noise.observations {
            frame.sigma_px *= 2.;
        }
        let doubled = available(&input, &noise).covariance_m2.unwrap();
        assert_scaled(doubled, base, 4., 1e-10);
        let (scaled, noise) = fixture(point, 7., Vec2::ZERO);
        assert_scaled(
            available(&scaled, &noise).covariance_m2.unwrap(),
            base,
            49.,
            1e-3,
        );
    }

    #[test]
    fn frame_order_and_both_held_out_partitions_do_not_steer_derivatives() {
        let (mut input, mut noise) = fixture(Vec3::new(0.05, -0.2, 0.1), 1., Vec2::ZERO);
        let original = triangulate_with_sheet_noise(&input, &noise).unwrap();
        input.observations.reverse();
        noise.observations.reverse();
        assert_eq!(
            json!(original),
            json!(triangulate_with_sheet_noise(&input, &noise).unwrap())
        );
        input.observations[0].pixel_px.x += 0.2;
        let landmark_check = available(&input, &noise);
        assert_eq!(
            json!(original.conditional_sheet_uncertainty),
            json!(landmark_check)
        );
        let source = &mut noise.observations[0].report["input"];
        let pixel = source["points"][4]["pixel_px"][0].as_f64().unwrap();
        source["points"][4]["pixel_px"][0] = json!(pixel + 0.2);
        noise.observations[0].report = Camera::sheet_pose_report(source).unwrap();
        let sheet_check = available(&input, &noise);
        assert_eq!(sheet_check.covariance_m2, landmark_check.covariance_m2);
        assert_eq!(sheet_check.sigma_m, landmark_check.sigma_m);
        assert_ne!(sheet_check.input_sha256, landmark_check.input_sha256);
    }

    #[test]
    fn invalid_noise_and_incomplete_frame_evidence_refuse() {
        let (input, noise) = fixture(Vec3::ZERO, 1., Vec2::ZERO);
        for sigma in [0., -1., f64::NAN, f64::INFINITY, 1000.01] {
            let mut bad = noise.clone();
            bad.observations[0].sigma_px = sigma;
            assert!(triangulate_with_sheet_noise(&input, &bad).is_err());
        }
        let mut bad = noise.clone();
        bad.observations.pop();
        assert!(triangulate_with_sheet_noise(&input, &bad).is_err());
        let mut bad = noise.clone();
        bad.observations[1].frame = bad.observations[0].frame.clone();
        assert!(triangulate_with_sheet_noise(&input, &bad).is_err());
        let mut bad = noise.clone();
        bad.observations[0].frame = input.observations[2].frame.clone();
        assert!(triangulate_with_sheet_noise(&input, &bad).is_err());
        let mut bad = noise.clone();
        bad.observations[0].frame.stream = "unknown".into();
        assert!(triangulate_with_sheet_noise(&input, &bad).is_err());
        let mut bad = noise.clone();
        bad.independence_evidence.clear();
        assert!(triangulate_with_sheet_noise(&input, &bad).is_err());
        let mut bad = json!(noise);
        bad["invented_covariance"] = json!(0.);
        assert!(serde_json::from_value::<SheetCornerNoiseInput>(bad).is_err());
    }

    #[test]
    fn altered_reports_cameras_and_different_reference_frames_refuse() {
        let (input, noise) = fixture(Vec3::ZERO, 1., Vec2::ZERO);
        for field in ["camera", "reprojection", "flags", "no_claim", "unexpected"] {
            let mut bad = noise.clone();
            bad.observations[0].report[field] = Value::Null;
            assert!(
                triangulate_with_sheet_noise(&input, &bad).is_err(),
                "{field}"
            );
        }
        let mut changed = input.clone();
        changed.observations[0].camera.center_m.x += 1e-7;
        assert!(triangulate_with_sheet_noise(&changed, &noise).is_err());
        let mut bad = noise.clone();
        bad.observations[1].report["input"]["reference_id"] = json!("different-sheet");
        bad.observations[1].report =
            Camera::sheet_pose_report(&bad.observations[1].report["input"]).unwrap();
        assert!(triangulate_with_sheet_noise(&input, &bad).is_err());
        let mut bad = noise;
        let source = &mut bad.observations[0].report["input"];
        source["points"][4]["pixel_px"][0] = json!(0.);
        bad.observations[0].report = Camera::sheet_pose_report(source).unwrap();
        assert_eq!(bad.observations[0].report["ok"], false);
        assert!(triangulate_with_sheet_noise(&input, &bad).is_err());
    }

    #[test]
    fn rejected_parent_and_unrepresentable_precision_never_publish_partial_covariance() {
        let (input, noise) = fixture(Vec3::ZERO, 1., Vec2::ZERO);
        let mut rejected = input.clone();
        rejected.observations[2].pixel_px.x += 100.;
        let result = triangulate_with_sheet_noise(&rejected, &noise).unwrap();
        assert!(result.triangulation.point_m.is_none());
        assert_eq!(result.conditional_sheet_uncertainty.status, "UNAVAILABLE");
        assert!(result.conditional_sheet_uncertainty.covariance_m2.is_none());
        let mut tiny = noise;
        tiny.observations[1].sigma_px = 1e-200;
        let result = triangulate_with_sheet_noise(&input, &tiny).unwrap();
        assert!(result.triangulation.point_m.is_some());
        assert_eq!(result.conditional_sheet_uncertainty.status, "UNAVAILABLE");
        assert!(result.conditional_sheet_uncertainty.covariance_m2.is_none());
        assert!(result.conditional_sheet_uncertainty.sigma_m.is_none());
        assert!(
            result
                .conditional_sheet_uncertainty
                .sensitivities
                .is_empty()
        );
    }

    #[test]
    fn image_boundary_keeps_valid_geometry_but_refuses_corner_differentiation() {
        let (mut input, mut noise) = fixture(Vec3::ZERO, 1., Vec2::ZERO);
        let source = &mut noise.observations[0].report["input"];
        source["intrinsics"]["principal_px"][0] = json!(0.);
        for point in source["points"].as_array_mut().unwrap() {
            point["pixel_px"][0] = json!(point["pixel_px"][0].as_f64().unwrap() - 800.);
        }
        noise.observations[0].report = Camera::sheet_pose_report(source).unwrap();
        input.observations[0].camera =
            serde_json::from_value(noise.observations[0].report["camera"].clone()).unwrap();
        input.observations[0].pixel_px.x -= 800.;
        let result = triangulate_with_sheet_noise(&input, &noise).unwrap();
        assert!(result.triangulation.point_m.is_some());
        assert_eq!(result.conditional_sheet_uncertainty.status, "UNAVAILABLE");
        assert!(result.conditional_sheet_uncertainty.covariance_m2.is_none());
        assert!(
            result
                .conditional_sheet_uncertainty
                .sensitivities
                .is_empty()
        );
    }

    #[test]
    fn complete_retained_and_fit_corner_work_is_bounded_before_refitting() {
        let (mut input, mut noise) = fixture(Vec3::ZERO, 1., Vec2::ZERO);
        for index in 2..65 {
            let mut view = input.observations[0].clone();
            view.frame.stream = format!("synthetic-extra-{index}");
            let mut source = noise.observations[0].clone();
            source.frame = view.frame.clone();
            input.observations.push(view);
            noise.observations.push(source);
        }
        let error = triangulate_with_sheet_noise(&input, &noise).unwrap_err();
        assert_eq!(error.code, RefusalCode::BudgetExceeded);
        let (input, mut noise) = fixture(Vec3::ZERO, 1., Vec2::ZERO);
        let held_out = noise.observations[0].report["input"]["points"][4].clone();
        noise.observations[0].report["input"]["points"]
            .as_array_mut()
            .unwrap()
            .extend(std::iter::repeat_n(held_out, 1024));
        let error = triangulate_with_sheet_noise(&input, &noise).unwrap_err();
        assert_eq!(error.code, RefusalCode::BudgetExceeded);
    }
}
