//! Conditional click-noise propagation through the complete triangulation estimator.
//!
//! Differentiate the actual parent pipeline, including pixel refinement and its gates, rather
//! than maintaining a second estimator or assigning it a different noise-weighted objective.
//! Propagate declared independent click errors with calibration, scale and motion held fixed.

use std::collections::BTreeMap;

use ftx_core::{Mat3, Refusal, RefusalCode, Vec3};
use serde::{Deserialize, Serialize};

use super::{FrameId, Partition, TriangulationInput, TriangulationReport};
use crate::invalid;

/// Declared isotropic one-sigma click uncertainty for one fitting frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameNoise {
    /// Must identify exactly one fitting observation, never a held-out observation.
    pub frame: FrameId,
    /// Independent x/y standard deviation in oriented pixels, finite and in (0, 1000].
    pub sigma_px: f64,
}

/// Explicit click-noise evidence. No sigma is inferred from small fitting residuals.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClickNoiseInput {
    /// Must be `ftx-click-noise/1`.
    pub schema: String,
    /// Source, method or receipt for the declared click uncertainty, not camera calibration.
    pub evidence: String,
    /// Exactly one entry per fitting frame; order does not matter.
    pub observations: Vec<FrameNoise>,
}

/// Sensitivity of the actual triangulation estimate to one fitting click.
#[derive(Clone, Debug, Serialize)]
pub struct FrameSensitivity {
    /// Source fitting frame.
    pub frame: FrameId,
    /// Declared independent isotropic click sigma used in propagation.
    pub sigma_px: f64,
    /// Rows are world x/y/z; columns are oriented pixel x/y, in metres per pixel.
    pub jacobian_m_per_px: [[f64; 2]; 3],
}

/// First-order uncertainty conditioned on the supplied cameras, scale and capture assumptions.
#[derive(Clone, Debug, Serialize)]
pub struct ClickUncertaintyReport {
    /// `ftx-triangulation-click-uncertainty/1`.
    pub schema: &'static str,
    /// AVAILABLE or UNAVAILABLE; an unavailable estimate never publishes zero covariance.
    pub status: &'static str,
    /// Complete noise evidence in canonical fitting-frame order.
    pub input: ClickNoiseInput,
    /// Full symmetric world covariance in square metres; null when unavailable.
    pub covariance_m2: Option<Mat3>,
    /// Marginal one-sigma x/y/z uncertainties in metres, not simultaneous confidence bounds.
    pub sigma_m: Option<Vec3>,
    /// Per-frame derivatives; empty unless the complete estimate is available.
    pub sensitivities: Vec<FrameSensitivity>,
    /// Why uncertainty could not be computed, if applicable.
    pub flags: Vec<String>,
    /// Exclusions that prevent conditional precision from becoming an accuracy claim.
    pub no_claim: &'static str,
}

/// Backward-compatible triangulation report plus an explicitly conditional uncertainty field.
#[derive(Clone, Debug, Serialize)]
pub struct UncertainTriangulationReport {
    /// Original geometry, gates, input receipt and status; the point is not refitted.
    #[serde(flatten)]
    pub triangulation: TriangulationReport,
    /// Optional-invocation click-noise estimate; never total real-world uncertainty.
    pub conditional_click_uncertainty: ClickUncertaintyReport,
}

fn unavailable(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message).repair(
        "improve reference geometry and lens-domain margin; do not interpret missing uncertainty as zero",
    )
}

fn derivative(
    normalized: &TriangulationInput,
    index: usize,
    axis: usize,
    step: f64,
) -> Result<Vec3, Refusal> {
    let pixel = normalized.observations[index].pixel_px;
    let mut plus = pixel;
    let mut minus = pixel;
    if axis == 0 {
        plus.x += step;
        minus.x -= step;
    } else {
        plus.y += step;
        minus.y -= step;
    }
    let width = if axis == 0 {
        plus.x - minus.x
    } else {
        plus.y - minus.y
    };
    if !width.is_finite() || (width / (2.0 * step) - 1.0).abs() > 1e-3 {
        return Err(unavailable(
            "pixel coordinates cannot resolve the differential step",
        ));
    }
    // Reuse the complete parent pipeline so later refinement changes cannot silently make
    // these derivatives describe an obsolete initializer. This private input has fit views
    // only: held-out reprojections must not influence differential availability either.
    let sample = |pixel| {
        let mut perturbed = normalized.clone();
        perturbed.observations[index].pixel_px = pixel;
        let result = super::triangulate(&perturbed)?;
        result.point_m.ok_or_else(|| {
            unavailable(format!(
                "differential triangulation did not pass its gates: {}",
                result.flags.join("; ")
            ))
        })
    };
    let high = sample(plus)?;
    let low = sample(minus)?;
    let out = (high - low) / width;
    if !out.is_finite() {
        return Err(unavailable("non-finite click sensitivity"));
    }
    Ok(out)
}

fn propagate(
    input: &TriangulationInput,
    noise: &[FrameNoise],
) -> Result<(Mat3, Vec3, Vec<FrameSensitivity>), Refusal> {
    let mut normalized = input.clone();
    normalized
        .observations
        .retain(|v| v.partition == Partition::Fit);
    // An accepted parent solve has at least two fit views. Normalize its *camera centers*
    // before each re-solve, not two large reconstructed world positions after subtracting.
    let origin = normalized.observations[0].camera.center_m;
    let scale = normalized
        .observations
        .iter()
        .map(|v| (v.camera.center_m - origin).norm())
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(unavailable(
            "no finite metric baseline for uncertainty propagation",
        ));
    }
    for view in &mut normalized.observations {
        view.camera.center_m = (view.camera.center_m - origin) / scale;
    }
    let mut covariance = [[0.0; 3]; 3];
    let mut sensitivities = Vec::with_capacity(noise.len());
    for (index, frame_noise) in noise.iter().enumerate() {
        let mut columns = [Vec3::ZERO; 2];
        for (axis, column) in columns.iter_mut().enumerate() {
            // Two step sizes reject unstable differentiation instead of publishing a precise
            // number from cancellation, a lens boundary or an ill-conditioned local solve.
            let coarse = derivative(&normalized, index, axis, 0.01)?;
            let fine = derivative(&normalized, index, axis, 0.005)?;
            let change = (coarse - fine).norm();
            let magnitude = fine.norm().max(1e-12);
            if !change.is_finite() || !magnitude.is_finite() || change > 1e-3 * magnitude {
                return Err(unavailable(
                    "click sensitivity did not converge across differential steps",
                ));
            }
            *column = fine * scale;
        }
        let variance = frame_noise.sigma_px * frame_noise.sigma_px;
        for (i, row) in covariance.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                *value +=
                    variance * (columns[0][i] * columns[0][j] + columns[1][i] * columns[1][j]);
            }
        }
        sensitivities.push(FrameSensitivity {
            frame: frame_noise.frame.clone(),
            sigma_px: frame_noise.sigma_px,
            jacobian_m_per_px: std::array::from_fn(|i| [columns[0][i], columns[1][i]]),
        });
    }
    if covariance.iter().flatten().any(|x| !x.is_finite())
        || (0..3).any(|i| covariance[i][i] <= 0.0)
    {
        return Err(unavailable(
            "click covariance overflowed or underflowed; no covariance was published",
        ));
    }
    let sigma = Vec3::new(
        covariance[0][0].sqrt(),
        covariance[1][1].sqrt(),
        covariance[2][2].sqrt(),
    );
    Ok((Mat3::from_rows(covariance), sigma, sensitivities))
}

/// Preserve the parent's estimate and all its gates, then propagate declared fitting-click noise.
///
/// The first-order covariance is Σ sigma_i² J_i J_iᵀ for the *actual triangulation estimator*.
/// J is evaluated with checked central differences at 0.01 and 0.005 pixels, rerunning the
/// parent's full fit-only pipeline, including lens inversion, pixel refinement and gates.
/// Camera centers are normalized before those solves. Held-out data never enters derivatives.
/// Invalid noise evidence is a refusal. Rejected geometry or unstable numerical differentiation
/// retains the diagnostic geometry report but returns UNAVAILABLE with null covariance/sigma.
/// This neither changes the point nor estimates calibration, scale, motion or systematic error.
pub fn triangulate_with_click_noise(
    input: &TriangulationInput,
    noise: &ClickNoiseInput,
) -> Result<UncertainTriangulationReport, Refusal> {
    let triangulation = super::triangulate(input)?;
    if noise.schema != "ftx-click-noise/1"
        || !super::text_present(&noise.evidence)
        || noise.observations.len() > 128
    {
        return Err(invalid(
            "click noise requires schema /1, an evidence source and at most 128 frames",
        ));
    }
    let fit: Vec<_> = triangulation
        .input
        .observations
        .iter()
        .filter(|v| v.partition == Partition::Fit)
        .collect();
    let mut sorted = noise.clone();
    sorted.observations.sort_by(|a, b| a.frame.cmp(&b.frame));
    let mut by_frame = BTreeMap::new();
    for n in &sorted.observations {
        if !n.sigma_px.is_finite()
            || n.sigma_px <= 0.0
            || n.sigma_px > 1000.0
            || by_frame.insert(n.frame.clone(), n.sigma_px).is_some()
        {
            return Err(invalid(
                "click sigmas must be finite in (0, 1000], with unique fitting frames",
            ));
        }
    }
    if by_frame.len() != fit.len() || fit.iter().any(|v| !by_frame.contains_key(&v.frame)) {
        return Err(invalid(
            "click noise needs exactly one entry per fit frame, with no held-out or unknown frames",
        ));
    }
    let mut report = ClickUncertaintyReport {
        schema: "ftx-triangulation-click-uncertainty/1",
        status: "UNAVAILABLE",
        input: sorted,
        covariance_m2: None,
        sigma_m: None,
        sensitivities: Vec::new(),
        flags: Vec::new(),
        no_claim: "First-order conditional click-noise precision only, assuming independent isotropic fitting-click errors and fixed cameras, metric scale and capture. Excludes calibration, scale, synchronization, motion, correspondence and systematic errors; not total uncertainty or a body-fit accuracy claim.",
    };
    if triangulation.point_m.is_none() {
        report
            .flags
            .push("Triangulation did not publish an accepted point".into());
    } else {
        match propagate(&triangulation.input, &report.input.observations) {
            Ok((covariance, sigma, sensitivities)) => {
                report.status = "AVAILABLE";
                report.covariance_m2 = Some(covariance);
                report.sigma_m = Some(sigma);
                report.sensitivities = sensitivities;
            }
            Err(error) => report.flags.push(error.to_string()),
        }
    }
    Ok(UncertainTriangulationReport {
        triangulation,
        conditional_click_uncertainty: report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, Distortion, Intrinsics};
    use crate::triangulate::{Capture, Observation};
    use ftx_core::Vec2;

    fn fixture() -> (TriangulationInput, ClickNoiseInput) {
        let point = Vec3::new(0.0, 0.0, 1.0);
        let observations: Vec<_> = [-0.5, 0.5, 0.0]
            .into_iter()
            .enumerate()
            .map(|(i, x)| {
                let camera = Camera {
                    intrinsics: Intrinsics {
                        focal_px: Vec2::new(800.0, 800.0),
                        principal_px: Vec2::new(0.0, 0.0),
                        distortion: Distortion::default(),
                    },
                    world_to_camera: Mat3::from_rows([
                        [1.0, 0.0, 0.0],
                        [0.0, 0.0, -1.0],
                        [0.0, 1.0, 0.0],
                    ]),
                    center_m: Vec3::new(x, -2.0, 1.0),
                };
                Observation {
                    frame: FrameId {
                        stream: format!("synthetic-{i}"),
                        timestamp_us: 0,
                    },
                    pixel_px: camera.project(point).unwrap(),
                    camera,
                    partition: if i == 2 {
                        Partition::HeldOut
                    } else {
                        Partition::Fit
                    },
                }
            })
            .collect();
        let noise = ClickNoiseInput {
            schema: "ftx-click-noise/1".into(),
            evidence: "synthetic one-pixel click noise".into(),
            observations: observations[..2]
                .iter()
                .map(|v| FrameNoise {
                    frame: v.frame.clone(),
                    sigma_px: 1.0,
                })
                .collect(),
        };
        (
            TriangulationInput {
                schema: "ftx-triangulation-input/1".into(),
                metric_scale_source: "synthetic one-metre baseline".into(),
                capture: Capture::Synchronized {
                    max_skew_us: 0,
                    evidence: "synthetic simultaneous views".into(),
                },
                observations,
            },
            noise,
        )
    }

    fn covariance(input: &TriangulationInput, noise: &ClickNoiseInput) -> Mat3 {
        let r = triangulate_with_click_noise(input, noise).unwrap();
        assert_eq!(r.conditional_click_uncertainty.status, "AVAILABLE");
        r.conditional_click_uncertainty.covariance_m2.unwrap()
    }

    #[test]
    fn symmetric_stereo_matches_analytic_click_covariance() {
        let (input, noise) = fixture();
        let r = triangulate_with_click_noise(&input, &noise).unwrap();
        let c = r.conditional_click_uncertainty.covariance_m2.unwrap();
        // f=800 px, baseline=1 m, depth=2 m: x/z sensitivities ±depth/(2f),
        // depth sensitivity ±depth²/(f*baseline), summed across two independent clicks.
        let expected = [3.125e-6, 5.0e-5, 3.125e-6];
        for (i, row) in c.rows.iter().enumerate() {
            assert!((row[i] - expected[i]).abs() < 1e-5 * expected[i]);
            for (j, value) in row.iter().enumerate() {
                if i != j {
                    assert!(value.abs() < 1e-11);
                }
            }
        }
        assert_eq!(r.conditional_click_uncertainty.sensitivities.len(), 2);
        assert!(r.conditional_click_uncertainty.sigma_m.unwrap().is_finite());
        let base = super::super::triangulate(&input).unwrap();
        assert_eq!(
            serde_json::to_value(base).unwrap(),
            serde_json::to_value(r.triangulation).unwrap()
        );
    }

    #[test]
    fn unequal_click_noise_preserves_cross_covariance_without_refitting_point() {
        let (input, mut noise) = fixture();
        noise.observations[1].sigma_px = 2.0;
        let c = covariance(&input, &noise);
        assert!((c.rows[0][0] - 7.8125e-6).abs() < 1e-10);
        assert!((c.rows[1][1] - 1.25e-4).abs() < 1e-9);
        assert!((c.rows[2][2] - 7.8125e-6).abs() < 1e-10);
        assert!((c.rows[0][1] - 1.875e-5).abs() < 1e-10);
        assert_eq!(c.rows[0][1], c.rows[1][0]);
    }

    #[test]
    fn noise_and_metric_scale_follow_squared_covariance_scaling() {
        let (mut input, mut noise) = fixture();
        let baseline = covariance(&input, &noise);
        for n in &mut noise.observations {
            n.sigma_px *= 2.0;
        }
        let doubled = covariance(&input, &noise);
        for (a, b) in doubled
            .rows
            .iter()
            .flatten()
            .zip(baseline.rows.iter().flatten())
        {
            assert!((a - b * 4.0).abs() < 1e-12);
        }
        for n in &mut noise.observations {
            n.sigma_px /= 2.0;
        }
        for v in &mut input.observations {
            v.camera.center_m = v.camera.center_m * 7.0;
        }
        let scaled = covariance(&input, &noise);
        for (a, b) in scaled
            .rows
            .iter()
            .flatten()
            .zip(baseline.rows.iter().flatten())
        {
            assert!((a - b * 49.0).abs() < 1e-10);
        }
    }

    #[test]
    fn large_world_origin_does_not_destroy_the_differential() {
        let (mut input, noise) = fixture();
        let before = covariance(&input, &noise);
        for v in &mut input.observations {
            v.camera.center_m += Vec3::new(1e9, -2e9, 3e9);
        }
        assert_eq!(before, covariance(&input, &noise));
    }

    #[test]
    fn partitions_remain_independent_and_input_order_does_not_matter() {
        let (mut input, mut noise) = fixture();
        let before = triangulate_with_click_noise(&input, &noise).unwrap();
        input.observations.reverse();
        noise.observations.reverse();
        let after = triangulate_with_click_noise(&input, &noise).unwrap();
        assert_eq!(
            serde_json::to_vec(&before).unwrap(),
            serde_json::to_vec(&after).unwrap()
        );
        input
            .observations
            .iter_mut()
            .find(|v| v.partition == Partition::HeldOut)
            .unwrap()
            .pixel_px
            .x += 0.2;
        let changed = triangulate_with_click_noise(&input, &noise).unwrap();
        assert_eq!(
            serde_json::to_vec(&before.conditional_click_uncertainty).unwrap(),
            serde_json::to_vec(&changed.conditional_click_uncertainty).unwrap()
        );
    }

    #[test]
    fn rejected_and_unobservable_geometry_never_get_covariance() {
        let (input, noise) = fixture();
        let mut bad = input.clone();
        bad.observations[2].pixel_px.y += 100.0;
        let rejected = triangulate_with_click_noise(&bad, &noise).unwrap();
        assert!(rejected.triangulation.point_m.is_none());
        assert!(
            rejected
                .conditional_click_uncertainty
                .covariance_m2
                .is_none()
        );
        let mut weak = input;
        weak.observations[1].camera = weak.observations[0].camera;
        weak.observations[1].pixel_px = weak.observations[0].pixel_px;
        let r = triangulate_with_click_noise(&weak, &noise).unwrap();
        assert_eq!(r.conditional_click_uncertainty.status, "UNAVAILABLE");
        assert!(r.conditional_click_uncertainty.sigma_m.is_none());
        assert!(r.conditional_click_uncertainty.sensitivities.is_empty());
    }

    #[test]
    fn lens_boundary_keeps_geometry_but_refuses_unsupported_differentiation() {
        let (mut input, noise) = fixture();
        for v in &mut input.observations {
            v.camera.intrinsics.distortion.max_radius = 0.250001;
        }
        let r = triangulate_with_click_noise(&input, &noise).unwrap();
        assert!(r.triangulation.point_m.is_some());
        assert_eq!(r.conditional_click_uncertainty.status, "UNAVAILABLE");
        assert!(r.conditional_click_uncertainty.covariance_m2.is_none());
        assert!(!r.conditional_click_uncertainty.flags.is_empty());
    }

    #[test]
    fn absent_duplicate_unknown_held_out_and_invalid_noise_are_refusals() {
        let (input, noise) = fixture();
        for sigma in [0.0, -1.0, f64::NAN, f64::INFINITY, 1000.01] {
            let mut bad = noise.clone();
            bad.observations[0].sigma_px = sigma;
            assert!(triangulate_with_click_noise(&input, &bad).is_err());
        }
        let mut bad = noise.clone();
        bad.observations.pop();
        assert!(triangulate_with_click_noise(&input, &bad).is_err());
        let mut bad = noise.clone();
        bad.observations[1].frame = bad.observations[0].frame.clone();
        assert!(triangulate_with_click_noise(&input, &bad).is_err());
        let mut bad = noise.clone();
        bad.observations[0].frame = input.observations[2].frame.clone();
        assert!(triangulate_with_click_noise(&input, &bad).is_err());
        let mut bad = noise.clone();
        bad.observations[0].frame.stream = "unknown".into();
        assert!(triangulate_with_click_noise(&input, &bad).is_err());
        let mut bad = noise.clone();
        bad.evidence.clear();
        assert!(triangulate_with_click_noise(&input, &bad).is_err());
        let mut bad = noise;
        bad.schema = "other".into();
        assert!(triangulate_with_click_noise(&input, &bad).is_err());
    }

    #[test]
    fn brown_distortion_and_overdetermined_fit_produce_positive_symmetric_covariance() {
        let (mut input, mut noise) = fixture();
        let point = Vec3::new(0.08, 0.2, 1.35);
        input.observations[2].partition = Partition::Fit;
        noise.observations.push(FrameNoise {
            frame: input.observations[2].frame.clone(),
            sigma_px: 0.4,
        });
        for v in &mut input.observations {
            v.camera.intrinsics.distortion = Distortion {
                radial: [-0.01, 0.002, 0.0001],
                tangential: [0.0005, -0.0004],
                max_radius: 2.0,
            };
            v.pixel_px = v.camera.project(point).unwrap();
        }
        let c = covariance(&input, &noise);
        for i in 0..3 {
            assert!(c.rows[i][i] > 0.0);
            for j in 0..3 {
                assert_eq!(c.rows[i][j], c.rows[j][i]);
            }
        }
        assert!(c.det() > 0.0);
    }

    #[test]
    fn underflow_is_unavailable_not_perfect_precision() {
        let (input, mut noise) = fixture();
        for entry in &mut noise.observations {
            entry.sigma_px = 1e-200;
        }
        let report = triangulate_with_click_noise(&input, &noise).unwrap();
        assert!(report.triangulation.point_m.is_some());
        assert_eq!(report.conditional_click_uncertainty.status, "UNAVAILABLE");
        assert!(report.conditional_click_uncertainty.covariance_m2.is_none());
        assert!(report.conditional_click_uncertainty.sigma_m.is_none());
    }
}
