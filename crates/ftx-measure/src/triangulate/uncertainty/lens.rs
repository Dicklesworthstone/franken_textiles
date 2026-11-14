//! Shared-lens precision with explicitly fixed camera poses. A retained calibration is replayed
//! before its covariance may perturb all of its fitting views together. Different lens groups
//! contribute independently only under the caller's explicit acquisition-independence assertion.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Mat3, Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::super::{FrameId, Partition, TriangulationInput, TriangulationReport};
use crate::{
    camera::Intrinsics,
    invalid,
    photo::intrinsics::{IntrinsicUncertainty, checked_calibration},
};

/// One calibration shared by all of the listed fitting cameras.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LensNoiseGroup {
    /// Unique, nonempty report key; the physical lens identity comes from the calibration.
    pub key: String,
    /// Complete retained ACCEPTED `ftx-camera-calibration/1` report with declared corner noise.
    /// Original inputs are recomputed; saved success flags and covariance are not trusted.
    pub calibration: Value,
    /// Exactly the fitting frames sharing this lens; held-out frames do not receive derivatives.
    pub frames: Vec<FrameId>,
}

/// Declared independent calibration acquisitions, with correlated errors within each lens group.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LensNoiseInput {
    /// Must be `ftx-triangulation-lens-noise/1`.
    pub schema: String,
    /// Basis for independence between calibration groups and measurement clicks. Supplied camera
    /// poses are treated as exact; their uncertainty and dependence on the lens are excluded.
    pub independence_evidence: String,
    /// 1–8 groups covering every FIT frame exactly once, with distinct calibrated lens identities.
    pub groups: Vec<LensNoiseGroup>,
}

/// First-order response to a complete correlated lens, not independent per-camera errors.
#[derive(Clone, Debug, Serialize)]
pub struct LensGroupSensitivity {
    /// Caller-selected group key.
    pub key: String,
    /// Physical lens/settings identity replayed from the original calibration input.
    pub lens_id: String,
    /// Canonical fitting-frame order within this shared lens group.
    pub frames: Vec<FrameId>,
    /// SHA-256 of canonical parsed calibration JSON, not a claim about original file bytes.
    pub calibration_sha256: String,
    /// Each vector is d(world point)/d(standard-normal covariance mode), in metres. All group
    /// cameras vary jointly along one Cholesky column of the full nine-parameter covariance.
    pub mode_sensitivities_m: [Vec3; 9],
    /// This group's sum of mode outer products in square metres.
    pub covariance_m2: Mat3,
}

/// Conditional first-order lens contribution. Camera poses and metric scale remain fixed.
#[derive(Clone, Debug, Serialize)]
pub struct LensUncertaintyReport {
    /// `ftx-triangulation-lens-uncertainty/1`.
    pub schema: &'static str,
    /// AVAILABLE or UNAVAILABLE; numerical failure never publishes a zero covariance.
    pub status: &'static str,
    /// Complete evidence in canonical group/frame order, including original calibrations.
    pub input: LensNoiseInput,
    /// SHA-256 of canonical parsed `input`, distinct from the caller's original byte identity.
    pub input_sha256: String,
    /// Full world covariance in square metres; absent unless every requested group is available.
    pub covariance_m2: Option<Mat3>,
    /// Marginal one-sigma x/y/z lens contributions, not simultaneous confidence limits.
    pub sigma_m: Option<Vec3>,
    /// Group contributions; empty when the complete result is unavailable.
    pub sensitivities: Vec<LensGroupSensitivity>,
    /// The numerical or parent-geometry reason for unavailable uncertainty.
    pub flags: Vec<String>,
    /// Conditional scope, including the explicit exclusion of lens/pose dependence.
    pub no_claim: &'static str,
}

/// Unchanged triangulation geometry plus a separately requested conditional lens contribution.
#[derive(Clone, Debug, Serialize)]
pub struct LensUncertainTriangulationReport {
    /// Original estimate, residuals, gates and input receipt; no lens-noise refitting is performed.
    #[serde(flatten)]
    pub triangulation: TriangulationReport,
    /// Independent of the optional click-noise report; this is not total measurement uncertainty.
    pub conditional_lens_uncertainty: LensUncertaintyReport,
}

struct PreparedGroup {
    key: String,
    lens_id: String,
    frames: Vec<FrameId>,
    calibration_sha256: String,
    uncertainty: IntrinsicUncertainty,
    factor: Vec<Vec<f64>>,
    image_size_px: [u32; 2],
}

fn canonical_bytes(value: &impl Serialize) -> Result<Vec<u8>, Refusal> {
    serde_json::to_vec(value).map_err(|error| invalid(error.to_string()))
}

fn prepare(
    input: &TriangulationInput,
    noise: &LensNoiseInput,
) -> Result<(LensNoiseInput, Vec<PreparedGroup>), Refusal> {
    if noise.schema != "ftx-triangulation-lens-noise/1"
        || !super::super::text_present(&noise.independence_evidence)
        || !(1..=8).contains(&noise.groups.len())
    {
        return Err(invalid(
            "lens noise requires schema /1, explicit calibration/measurement independence evidence and 1–8 lens groups",
        ));
    }
    if canonical_bytes(noise)?.len() > 16 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "retained lens-noise evidence exceeds 16 MiB",
        ));
    }
    let fit: BTreeMap<_, _> = input
        .observations
        .iter()
        .filter(|view| view.partition == Partition::Fit)
        .map(|view| (view.frame.clone(), view.camera.intrinsics))
        .collect();
    let mut sorted = noise.clone();
    sorted.groups.sort_by(|a, b| a.key.cmp(&b.key));
    let mut keys = BTreeSet::new();
    let mut assigned = BTreeSet::new();
    for group in &mut sorted.groups {
        if !super::super::text_present(&group.key)
            || !keys.insert(group.key.clone())
            || group.frames.is_empty()
            || group.frames.len() > 128
        {
            return Err(invalid(
                "lens groups need unique nonempty keys and 1–128 fitting frames",
            ));
        }
        group.frames.sort();
        for frame in &group.frames {
            if !fit.contains_key(frame) || !assigned.insert(frame.clone()) {
                return Err(invalid(
                    "each lens-noise frame must name a unique FIT observation, never a held-out or unknown frame",
                ));
            }
        }
    }
    if assigned.len() != fit.len() {
        return Err(invalid(
            "lens noise must assign every fitting frame to exactly one calibration group",
        ));
    }

    let mut lens_ids = BTreeSet::new();
    let mut calibration_frames = BTreeSet::new();
    let mut prepared = Vec::with_capacity(sorted.groups.len());
    for group in &sorted.groups {
        let checked = checked_calibration(&group.calibration)?;
        if !lens_ids.insert(checked.input.lens_id.clone()) {
            return Err(invalid(
                "one calibrated lens cannot be split into independent noise groups; share all of its fitting frames",
            ));
        }
        for view in &checked.input.views {
            if view.partition == Partition::Fit
                && (fit.contains_key(&view.frame) || !calibration_frames.insert(view.frame.clone()))
            {
                return Err(invalid(
                    "independent lens calibration groups must not reuse fitting calibration frames or triangulation FIT images",
                ));
            }
        }
        let uncertainty = checked
            .uncertainty
            .map_or_else(
                || {
                    checked
                        .conditional_uncertainty
                        .as_ref()
                        .map(IntrinsicUncertainty::from_calibration)
                        .transpose()
                },
                |value| Ok(Some(value)),
            )?
            .ok_or_else(|| {
                invalid("lens-noise calibration needs available declared-noise covariance")
            })?;
        let factor = uncertainty.covariance_factor()?;
        let expected = json!(uncertainty.intrinsics);
        for frame in &group.frames {
            if json!(fit[frame]) != expected {
                return Err(invalid(format!(
                    "lens group {:?}: every assigned camera must use the exact replayed intrinsics and certified lens domain",
                    group.key,
                )));
            }
        }
        prepared.push(PreparedGroup {
            key: group.key.clone(),
            lens_id: checked.input.lens_id,
            frames: group.frames.clone(),
            calibration_sha256: sha256_hex(&canonical_bytes(&group.calibration)?),
            uncertainty,
            factor,
            image_size_px: checked.input.image_size_px,
        });
    }
    Ok((sorted, prepared))
}

fn shift(mut k: Intrinsics, direction: [f64; 9], step: f64) -> Result<Intrinsics, Refusal> {
    k.focal_px.x += step * direction[0];
    k.focal_px.y += step * direction[1];
    k.principal_px.x += step * direction[2];
    k.principal_px.y += step * direction[3];
    for (value, delta) in k.distortion.radial.iter_mut().zip(&direction[4..7]) {
        *value += step * delta;
    }
    for (value, delta) in k.distortion.tangential.iter_mut().zip(&direction[7..9]) {
        *value += step * delta;
    }
    k.validate()?;
    Ok(k)
}

fn derivative(
    input: &TriangulationInput,
    indices: &[usize],
    group: &PreparedGroup,
    direction: [f64; 9],
    step: f64,
) -> Result<Vec3, Refusal> {
    let high = shift(group.uncertainty.intrinsics, direction, step)?;
    let low = shift(group.uncertainty.intrinsics, direction, -step)?;
    let width = 2.0 * step;
    if !width.is_finite() || width <= 0.0 || json!(high) == json!(low) {
        return Err(super::unavailable(
            "lens parameters cannot resolve the differential step",
        ));
    }
    let sample = |k| {
        let mut trial = input.clone();
        for &index in indices {
            trial.observations[index].camera.intrinsics = k;
        }
        let result = super::super::triangulate(&trial)?;
        result.point_m.ok_or_else(|| {
            super::unavailable(format!(
                "lens differential triangulation did not pass its gates: {}",
                result.flags.join("; "),
            ))
        })
    };
    let result = (sample(high)? - sample(low)?) / width;
    if !result.is_finite() {
        return Err(super::unavailable("non-finite lens sensitivity"));
    }
    Ok(result)
}

fn propagate(
    input: &TriangulationInput,
    groups: &[PreparedGroup],
) -> Result<(Mat3, Vec3, Vec<LensGroupSensitivity>), Refusal> {
    let mut normalized = input.clone();
    normalized
        .observations
        .retain(|view| view.partition == Partition::Fit);
    let origin = normalized.observations[0].camera.center_m;
    let scale = normalized
        .observations
        .iter()
        .map(|view| (view.camera.center_m - origin).norm())
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(super::unavailable(
            "no finite metric baseline for lens-noise propagation",
        ));
    }
    for view in &mut normalized.observations {
        view.camera.center_m = (view.camera.center_m - origin) / scale;
    }
    let mut covariance = [[0.0; 3]; 3];
    let mut sensitivities = Vec::with_capacity(groups.len());
    for group in groups {
        let indices: Vec<_> = normalized
            .observations
            .iter()
            .enumerate()
            .filter_map(|(index, view)| group.frames.contains(&view.frame).then_some(index))
            .collect();
        let k = group.uncertainty.intrinsics;
        let scales = [
            k.focal_px.x,
            k.focal_px.y,
            f64::from(group.image_size_px[0]),
            f64::from(group.image_size_px[1]),
            1.0,
            1.0,
            1.0,
            1.0,
            1.0,
        ];
        let mut modes = [Vec3::ZERO; 9];
        for (j, mode) in modes.iter_mut().enumerate() {
            let direction = std::array::from_fn(|i| group.factor[i][j]);
            let magnitude = direction
                .iter()
                .zip(scales)
                .map(|(delta, scale)| delta.abs() / scale)
                .fold(0.0, f64::max);
            let step = 1e-5 / magnitude;
            if !step.is_finite() || step <= 0.0 || step * 0.5 == 0.0 {
                return Err(super::unavailable(
                    "lens covariance perturbation is not representable",
                ));
            }
            let coarse = derivative(&normalized, &indices, group, direction, step)?;
            let fine = derivative(&normalized, &indices, group, direction, step * 0.5)?;
            let change = (coarse - fine).norm();
            let size = fine.norm().max(1e-12);
            if !change.is_finite() || !size.is_finite() || change > 1e-3 * size {
                return Err(super::unavailable(
                    "lens sensitivity did not converge across differential steps",
                ));
            }
            *mode = fine * scale;
            if !mode.is_finite() {
                return Err(super::unavailable("world lens sensitivity overflowed"));
            }
        }
        let mut component = [[0.0; 3]; 3];
        for mode in &modes {
            for (i, row) in component.iter_mut().enumerate() {
                for (j, value) in row.iter_mut().enumerate() {
                    *value += mode[i] * mode[j];
                }
            }
        }
        for (row, component) in covariance.iter_mut().zip(component) {
            for (value, contribution) in row.iter_mut().zip(component) {
                *value += contribution;
            }
        }
        sensitivities.push(LensGroupSensitivity {
            key: group.key.clone(),
            lens_id: group.lens_id.clone(),
            frames: group.frames.clone(),
            calibration_sha256: group.calibration_sha256.clone(),
            mode_sensitivities_m: modes,
            covariance_m2: Mat3::from_rows(component),
        });
    }
    if covariance.iter().flatten().any(|value| !value.is_finite())
        || (0..3).any(|i| covariance[i][i] < 0.0)
        || (0..3).all(|i| covariance[i][i] == 0.0)
    {
        return Err(super::unavailable(
            "lens covariance overflowed or underflowed; no covariance was published",
        ));
    }
    let sigma = Vec3::new(
        covariance[0][0].sqrt(),
        covariance[1][1].sqrt(),
        covariance[2][2].sqrt(),
    );
    Ok((Mat3::from_rows(covariance), sigma, sensitivities))
}

/// Replay complete calibration evidence and propagate its full covariance through the actual
/// triangulation estimator, perturbing all cameras of a shared lens jointly. The point and its
/// acceptance gates remain unchanged. Extrinsics, metric scale and capture conditions stay fixed.
///
/// # Errors
/// Malformed/oversized evidence, unavailable calibration covariance, reused calibration FIT images,
/// incomplete/duplicate FIT assignments or any mismatch with the exact retained lens are refused.
/// Rejected point geometry or unstable/invalid differential solves instead retain the geometry
/// report and return UNAVAILABLE, with null covariance and no partial group contributions.
pub fn triangulate_with_lens_noise(
    input: &TriangulationInput,
    noise: &LensNoiseInput,
) -> Result<LensUncertainTriangulationReport, Refusal> {
    let triangulation = super::super::triangulate(input)?;
    let (input, groups) = prepare(&triangulation.input, noise)?;
    let mut report = LensUncertaintyReport {
        schema: "ftx-triangulation-lens-uncertainty/1",
        status: "UNAVAILABLE",
        input_sha256: sha256_hex(&canonical_bytes(&input)?),
        input,
        covariance_m2: None,
        sigma_m: None,
        sensitivities: Vec::new(),
        flags: Vec::new(),
        no_claim: "First-order conditional lens contribution with fixed supplied camera poses and metric scale. Shared-lens errors remain correlated across fitting frames; distinct calibration groups and measurement clicks are assumed independent as declared. Camera-pose uncertainty and lens-pose correlations are excluded, including for poses previously estimated using this lens. Also excludes click noise, scale, synchronization, motion, correspondence, print/flatness, lens drift and other systematic errors. Not total measurement uncertainty, real-camera accuracy or physical body/garment fit.",
    };
    if triangulation.point_m.is_none() {
        report
            .flags
            .push("Triangulation did not publish an accepted point".into());
    } else {
        match propagate(&triangulation.input, &groups) {
            Ok((covariance, sigma, sensitivities)) => {
                report.status = "AVAILABLE";
                report.covariance_m2 = Some(covariance);
                report.sigma_m = Some(sigma);
                report.sensitivities = sensitivities;
            }
            Err(error) => report.flags.push(error.to_string()),
        }
    }
    Ok(LensUncertainTriangulationReport {
        triangulation,
        conditional_lens_uncertainty: report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photo::intrinsics::CornerNoise;

    fn group(input: &TriangulationInput, covariance: [[f64; 9]; 9]) -> PreparedGroup {
        let uncertainty = IntrinsicUncertainty {
            schema: "ftx-intrinsic-uncertainty/1",
            parameters: ["fx", "fy", "cx", "cy", "k1", "k2", "k3", "p1", "p2"],
            intrinsics: input.observations[0].camera.intrinsics,
            covariance,
            standard_deviations: std::array::from_fn(|i| covariance[i][i].sqrt()),
            corner_noise: CornerNoise {
                model: "independent_isotropic".into(),
                sigma_px: 0.25,
                evidence: "Synthetic covariance for an independent analytic stereo check".into(),
            },
            calibration_independence_evidence: None,
            fit_corners: 220,
            nuisance_parameters: 30,
            residual_degrees_of_freedom: 401,
            no_claim: "Synthetic covariance; no real camera",
        };
        PreparedGroup {
            key: "synthetic".into(),
            lens_id: "synthetic".into(),
            frames: input
                .observations
                .iter()
                .filter(|view| view.partition == Partition::Fit)
                .map(|view| view.frame.clone())
                .collect(),
            calibration_sha256: "synthetic analytic test".into(),
            factor: uncertainty.covariance_factor().unwrap(),
            uncertainty,
            image_size_px: [1600, 1200],
        }
    }

    fn covariance() -> [[f64; 9]; 9] {
        let sigma: [f64; 9] = [1.2, 1.1, 0.7, 0.8, 0.003, 0.002, 0.001, 0.0007, 0.0005];
        let mut covariance = std::array::from_fn(|i| {
            std::array::from_fn(|j| if i == j { sigma[i].powi(2) } else { 0.0 })
        });
        covariance[0][2] = 0.5 * sigma[0] * sigma[2];
        covariance[2][0] = covariance[0][2];
        covariance
    }

    fn assert_matrix(actual: Mat3, expected: Mat3, tolerance: f64) {
        for (i, row) in actual.rows.iter().enumerate() {
            for (j, value) in row.iter().enumerate() {
                assert!(
                    (value - expected.rows[i][j]).abs() <= tolerance,
                    "({i}, {j}): {actual:?} != {expected:?}",
                );
            }
        }
    }

    #[test]
    fn shared_lens_matches_closed_form_all_brown_modes_and_covariance_cross_terms() {
        let (input, _) = super::super::tests::fixture();
        // Parallel stereo with f=800 px, baseline=1 m and depth=2 m. Rays are x=±0.25,
        // y=0. Differentiating the pinhole/disparity equations and Brown polynomial gives
        // this complete physical-parameter Jacobian, independently of the production solver.
        let jacobian = [
            [0.0, 0.0, -0.0025, 0.0, 0.0, 0.0, 0.0, 0.0, -0.375],
            [
                0.0025,
                0.0,
                0.0,
                0.0,
                0.125,
                0.0078125,
                0.00048828125,
                0.0,
                0.0,
            ],
            [0.0, 0.0, 0.0, 0.0025, 0.0, 0.0, 0.0, 0.125, 0.0],
        ];
        for sign in [1.0, -1.0] {
            let mut covariance = covariance();
            covariance[0][2] *= sign;
            covariance[2][0] *= sign;
            let expected = Mat3::from_rows(std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    let mut value = 0.0;
                    for (a, row) in covariance.iter().enumerate() {
                        for (b, entry) in row.iter().enumerate() {
                            value += jacobian[i][a] * entry * jacobian[j][b];
                        }
                    }
                    value
                })
            }));
            let (actual, _, groups) = propagate(&input, &[group(&input, covariance)]).unwrap();
            assert_matrix(actual, expected, 2e-10);
            assert_eq!(actual.rows[0][1].is_sign_negative(), sign > 0.0);
            assert_eq!(actual.rows[0][1], actual.rows[1][0]);
            assert_eq!(groups.len(), 1);
            assert_matrix(groups[0].covariance_m2, actual, 0.0);
        }
    }

    #[test]
    fn a_shared_principal_point_does_not_invent_independent_depth_noise() {
        let (input, _) = super::super::tests::fixture();
        let mut covariance =
            std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1e-12 } else { 0.0 }));
        covariance[2][2] = 1.0;
        let shared = propagate(&input, &[group(&input, covariance)]).unwrap().0;
        let mut left = group(&input, covariance);
        let mut right = group(&input, covariance);
        left.frames.truncate(1);
        right.frames.remove(0);
        let independent = propagate(&input, &[left, right]).unwrap().0;
        // Shared cx shifts X by -depth/f and cancels disparity. Two independent cx errors
        // shift X by -depth/(2f) each and depth by ±depth²/(f*baseline).
        assert!((shared.rows[0][0] - 6.25e-6).abs() < 1e-10);
        assert!(shared.rows[1][1] < 1e-10);
        assert!((independent.rows[0][0] - 3.125e-6).abs() < 1e-10);
        assert!((independent.rows[1][1] - 5e-5).abs() < 1e-10);
    }

    #[test]
    fn covariance_follows_world_rotation_translation_and_metric_scale() {
        let (input, _) = super::super::tests::fixture();
        let original = propagate(&input, &[group(&input, covariance())]).unwrap().0;
        let rotation = Mat3::rotation(Vec3::new(1.0, 2.0, -0.5).normalized(), 0.31);
        let expected = rotation * original * rotation.transpose();
        for scale in [0.001, 2.5, 1000.0] {
            let mut moved = input.clone();
            for view in &mut moved.observations {
                view.camera.center_m = rotation.mul_vec(view.camera.center_m) * scale
                    + Vec3::new(1200.0, -900.0, 40.0);
                view.camera.world_to_camera = view.camera.world_to_camera * rotation.transpose();
            }
            let actual = propagate(&moved, &[group(&moved, covariance())]).unwrap().0;
            assert_matrix(
                actual,
                Mat3::from_rows(
                    expected
                        .rows
                        .map(|row| row.map(|value| value * scale * scale)),
                ),
                3e-10 * scale * scale,
            );
        }
    }

    #[test]
    fn held_out_values_do_not_enter_lens_derivatives_and_domain_failure_is_unavailable() {
        let (mut input, _) = super::super::tests::fixture();
        let original = propagate(&input, &[group(&input, covariance())]).unwrap().0;
        input.observations[2].pixel_px.y += 0.5;
        assert!(
            super::super::super::triangulate(&input)
                .unwrap()
                .point_m
                .is_some()
        );
        let changed = propagate(&input, &[group(&input, covariance())]).unwrap().0;
        assert_matrix(changed, original, 0.0);
        for view in &mut input.observations {
            view.camera.intrinsics.distortion.max_radius = 0.25 + 1e-8;
        }
        assert!(
            super::super::super::triangulate(&input)
                .unwrap()
                .point_m
                .is_some()
        );
        let error = propagate(&input, &[group(&input, covariance())]).unwrap_err();
        assert!(error.message.contains("lens"), "{error}");
    }
}
