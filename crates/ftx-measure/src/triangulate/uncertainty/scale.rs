//! One shared metric-reference scale error in the stationary sheet coordinate frame.
//!
//! A uniform rescaling of the original sheet coordinates about their fixed zero origin scales
//! every recovered camera center and the triangulated point together, with rotations and pixels
//! unchanged. Its one common mode must not be divided by the number of views.

use std::collections::BTreeMap;

use ftx_core::{Mat3, Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use super::super::{TriangulationInput, TriangulationReport};
use super::sheet::{FrameSheetPose, prepare_sheet_poses};
use super::{LensNoiseInput, SheetCornerNoiseInput};
use crate::invalid;

/// Declared common dimensional uncertainty of the original stationary reference.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceScaleNoiseInput {
    /// Must be `ftx-triangulation-reference-scale-noise/1`.
    pub schema: String,
    /// Exact stationary reference identity retained in every original sheet report.
    pub reference_id: String,
    /// Positive finite one-sigma fractional error shared by every metric sheet coordinate.
    /// For example, 0.001 declares 0.1 percent; no sigma is inferred from pixel residuals.
    pub sigma_fraction: f64,
    /// Source/method supporting this uniform scale model about the fixed sheet origin.
    /// Anisotropic print stretch, a moving origin and sheet warp are not this model.
    pub evidence: String,
    /// Original accepted sheet reports for every FIT frame, with exact camera matching.
    pub sheet_poses: Vec<FrameSheetPose>,
}

/// Conditional precision from one common uniform scale mode, never independent frame scales.
#[derive(Clone, Debug, Serialize)]
pub struct ReferenceScaleUncertaintyReport {
    /// `ftx-triangulation-reference-scale-uncertainty/1`.
    pub schema: &'static str,
    /// AVAILABLE or UNAVAILABLE, independently of the retained geometry status.
    pub status: &'static str,
    /// Complete declared source evidence, in canonical frame order.
    pub input: ReferenceScaleNoiseInput,
    /// SHA-256 of the canonical parsed evidence; distinct from original file-byte identities.
    pub input_sha256: String,
    /// Fixed world origin of the source sheet's `(x, z)` coordinates; always zero.
    pub reference_origin_m: Vec3,
    /// World x/y/z sensitivity to the one shared fractional scale error, metres.
    /// Absent when geometry or the complete covariance cannot be published.
    pub jacobian_m_per_fraction: Option<Vec3>,
    /// Full rank-one world covariance in square metres, absent when unavailable.
    /// A coordinate at the fixed origin can have exactly zero contribution from this mode.
    pub covariance_m2: Option<Mat3>,
    /// Marginal one-sigma contributions in metres, not total positional uncertainty.
    pub sigma_m: Option<Vec3>,
    /// Original-geometry or numerical reasons the requested contribution is unavailable.
    pub flags: Vec<String>,
    /// Assumptions and errors not described by this one dimensional mode.
    pub no_claim: &'static str,
}

/// Original geometry and its independently requested conditional reference-scale contribution.
#[derive(Clone, Debug, Serialize)]
pub struct ReferenceScaleUncertainTriangulationReport {
    /// Unchanged point, observations, residuals and geometry gates.
    #[serde(flatten)]
    pub triangulation: TriangulationReport,
    /// One common source-scale contribution; never a sum with other noise components.
    pub conditional_reference_scale_uncertainty: ReferenceScaleUncertaintyReport,
}

/// Require the same original sheet acquisitions when source-scale and camera precision coexist.
///
/// Equal fitted cameras cannot identify which original observations established their frame.
/// Legacy lens `/1` has no sheet sources; its separately conditional fixed-pose meaning remains.
///
/// # Errors
/// Duplicate, incomplete or differing source reports in a combined request are refused.
pub fn validate_reference_scale_sources(
    scale: &ReferenceScaleNoiseInput,
    lens: Option<&LensNoiseInput>,
    sheet: Option<&SheetCornerNoiseInput>,
) -> Result<(), Refusal> {
    let sources: BTreeMap<_, _> = scale
        .sheet_poses
        .iter()
        .map(|pose| (&pose.frame, &pose.report))
        .collect();
    if sources.len() != scale.sheet_poses.len() {
        return Err(invalid("reference-scale sheet frames must be unique"));
    }
    if let Some(poses) = lens.and_then(|lens| lens.sheet_poses.as_ref()) {
        let other: BTreeMap<_, _> = poses
            .iter()
            .map(|pose| (&pose.frame, &pose.report))
            .collect();
        if other.len() != poses.len() || other != sources {
            return Err(invalid(
                "combined reference-scale and lens noise must retain the same complete sheet-pose reports",
            ));
        }
    }
    if let Some(sheet) = sheet {
        let other: BTreeMap<_, _> = sheet
            .observations
            .iter()
            .map(|frame| (&frame.frame, &frame.report))
            .collect();
        if other.len() != sheet.observations.len() || other != sources {
            return Err(invalid(
                "combined reference-scale and sheet noise must retain the same complete sheet-pose reports",
            ));
        }
    }
    Ok(())
}

fn covariance(point: Vec3, sigma_fraction: f64) -> Result<(Mat3, Vec3), Refusal> {
    // The source sheet defines world zero. Subtracting the first camera would replace the
    // declared metrology anchor and remove a real shared scale contribution.
    let weighted = point * sigma_fraction;
    if !weighted.is_finite() || (0..3).any(|i| point[i] != 0. && weighted[i] == 0.) {
        return Err(super::unavailable(
            "reference-scale sensitivity overflowed or underflowed",
        ));
    }
    let rows: [[f64; 3]; 3] =
        std::array::from_fn(|i| std::array::from_fn(|j| weighted[i] * weighted[j]));
    for (i, row) in rows.iter().enumerate() {
        for (j, value) in row.iter().enumerate() {
            if !value.is_finite() || (weighted[i] != 0. && weighted[j] != 0. && *value == 0.) {
                return Err(super::unavailable(
                    "reference-scale covariance overflowed or underflowed",
                ));
            }
        }
    }
    Ok((
        Mat3::from_rows(rows),
        Vec3::new(rows[0][0].sqrt(), rows[1][1].sqrt(), rows[2][2].sqrt()),
    ))
}

/// Preserve the ordinary landmark solve and propagate one source-bound uniform scale error.
///
/// For original sheet coordinates `q -> (1 + e) q`, the calibrated sheet pose has the same
/// rotation and center `c -> (1 + e) c`. The pixel-refined estimator consequently obeys
/// `p -> (1 + e) p`. Thus `J = p` and `C = sigma_fraction² J Jᵀ`, including cross terms.
/// Every frame shares this mode; extra views do not average it down. The fixed anchor is the
/// original sheet chart's zero, not the camera or reconstructed landmark. Changing that chart
/// defines a different scale-error model; translating cameras without their sources refuses.
///
/// # Errors
/// Invalid noise, unmatched reference identity, incomplete/changed sheet evidence, incompatible
/// cameras or sources above 16 MiB refuse. Rejected geometry or unrepresentable covariance
/// instead retains the ordinary report with UNAVAILABLE, null covariance and null sensitivity.
pub fn triangulate_with_reference_scale_noise(
    input: &TriangulationInput,
    noise: &ReferenceScaleNoiseInput,
) -> Result<ReferenceScaleUncertainTriangulationReport, Refusal> {
    if noise.schema != "ftx-triangulation-reference-scale-noise/1"
        || !super::super::text_present(&noise.reference_id)
        || !super::super::text_present(&noise.evidence)
        || !noise.sigma_fraction.is_finite()
        || noise.sigma_fraction <= 0.
        || noise.sheet_poses.is_empty()
        || noise.sheet_poses.len() > 128
    {
        return Err(invalid(
            "reference-scale noise needs schema /1, a reference and evidence, positive finite fractional sigma and 1–128 FIT sheet reports",
        ));
    }
    let mut canonical = noise.clone();
    canonical.sheet_poses.sort_by(|a, b| a.frame.cmp(&b.frame));
    let bytes = serde_json::to_vec(&canonical).map_err(|error| invalid(error.to_string()))?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "reference-scale evidence exceeds 16 MiB",
        ));
    }
    let triangulation = super::super::triangulate(input)?;
    prepare_sheet_poses(&triangulation.input, &canonical.sheet_poses)?;
    if canonical.sheet_poses.iter().any(|pose| {
        pose.report["input"]["reference_id"].as_str() != Some(canonical.reference_id.as_str())
    }) {
        return Err(invalid(
            "reference-scale evidence must name the original stationary sheet reference_id",
        ));
    }
    let mut report = ReferenceScaleUncertaintyReport {
        schema: "ftx-triangulation-reference-scale-uncertainty/1",
        status: "UNAVAILABLE",
        input_sha256: sha256_hex(&bytes),
        input: canonical,
        reference_origin_m: Vec3::ZERO,
        jacobian_m_per_fraction: None,
        covariance_m2: None,
        sigma_m: None,
        flags: Vec::new(),
        no_claim: "Conditional contribution of one declared uniform metric-reference scale error shared across all FIT sheet cameras, about the fixed source sheet coordinate origin. Pixels and camera rotations stay fixed. Excludes anisotropic print stretch, warp, uncertain origin, lens/corner/click error, drift, synchronization, motion and other systematic errors. The source model and sigma are declared, not physically established. Not total uncertainty, a fused measurement, real-camera accuracy or body/garment fit.",
    };
    if let Some(point) = triangulation.point_m {
        match covariance(point, noise.sigma_fraction) {
            Ok((covariance, sigma)) => {
                report.status = "AVAILABLE";
                report.jacobian_m_per_fraction = Some(point);
                report.covariance_m2 = Some(covariance);
                report.sigma_m = Some(sigma);
            }
            Err(error) => report.flags.push(error.to_string()),
        }
    } else {
        report
            .flags
            .push("Triangulation did not publish an accepted point".into());
    }
    Ok(ReferenceScaleUncertainTriangulationReport {
        triangulation,
        conditional_reference_scale_uncertainty: report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_origin_and_zero_coordinates_are_valid_shared_modes() {
        let (c, sigma) = covariance(Vec3::new(0., 0., 2.), 0.01).unwrap();
        assert_eq!(
            c,
            Mat3::from_rows([[0., 0., 0.], [0., 0., 0.], [0., 0., 0.0004]])
        );
        assert_eq!(sigma, Vec3::new(0., 0., 0.02));
        let (c, sigma) = covariance(Vec3::ZERO, 1e-300).unwrap();
        assert_eq!(c, Mat3::from_rows([[0.; 3]; 3]));
        assert_eq!(sigma, Vec3::ZERO);
    }

    #[test]
    fn nonzero_unrepresentable_covariance_is_never_zero_precision() {
        for sigma in [1e-200, f64::MAX] {
            assert!(covariance(Vec3::new(0.08, -0.2, 1.3), sigma).is_err());
        }
    }
}
