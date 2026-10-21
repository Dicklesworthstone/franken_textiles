//! Inline-noise/photo representation of the shared Brown covariance estimator.
//! Numerical covariance has one authority: `refine::covariance` and the public uncertainty API.

use ftx_core::Refusal;
use serde::{Deserialize, Serialize};

use super::super::uncertainty::{CalibrationUncertainty, IntrinsicUncertainty as SharedEstimate};
use crate::{camera::Intrinsics, invalid};

/// Explicit per-coordinate noise assumption, separate from the fit's residuals.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CornerNoise {
    /// Must be `independent_isotropic`: same independent variance for every x/y observation.
    pub model: String,
    /// Known/assumed one-sigma noise in original, oriented image pixels (0 < sigma <= 1000).
    pub sigma_px: f64,
    /// Basis for the noise magnitude and independence, not merely the fitted residual RMS.
    pub evidence: String,
}

impl CornerNoise {
    /// Reject unstated, unsupported, zero or nonfinite noise rather than report false precision.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.model != "independent_isotropic"
            || !self.sigma_px.is_finite()
            || self.sigma_px <= 0.0
            || self.sigma_px > 1000.0
            || !super::super::evidence(&self.evidence)
        {
            return Err(invalid(
                "corner_noise requires independent_isotropic, 0 < sigma_px <= 1000 and evidence",
            ));
        }
        Ok(())
    }
}

/// First-order covariance of the fitted lens, with all fitting poses marginalized out.
/// Source noise and physical reference assumptions remain the caller's responsibility.
#[derive(Clone, Debug, Serialize)]
pub struct IntrinsicUncertainty {
    /// `ftx-intrinsic-uncertainty/1`.
    pub schema: &'static str,
    /// Matrix order; first four entries in pixels, remaining coefficients dimensionless.
    pub parameters: [&'static str; 9],
    /// Exact fitted lens to which this covariance belongs (including its certified domain).
    pub intrinsics: Intrinsics,
    /// Full covariance in the declared physical parameter order, including cross terms.
    pub covariance: [[f64; 9]; 9],
    /// Square roots of the covariance diagonal in the same order (one sigma).
    pub standard_deviations: [f64; 9],
    /// Explicit input noise assumptions; never inferred from a small reprojection error.
    pub corner_noise: CornerNoise,
    /// Separate independence justification retained by the shared explicit-noise API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calibration_independence_evidence: Option<String>,
    /// Number of fitted two-coordinate corners; excludes held-out views and corners.
    pub fit_corners: usize,
    /// Six unknown pose coordinates per fitting view, marginalized rather than fixed.
    pub nuisance_parameters: usize,
    /// 2 * fit_corners - (9 + nuisance_parameters); not used to rescale covariance.
    pub residual_degrees_of_freedom: usize,
    /// What the covariance does not establish.
    pub no_claim: &'static str,
}

pub(super) fn from_estimate(value: SharedEstimate, noise: CornerNoise) -> IntrinsicUncertainty {
    IntrinsicUncertainty {
        schema: "ftx-intrinsic-uncertainty/1",
        parameters: value.parameters,
        intrinsics: value.intrinsics,
        covariance: value.covariance,
        standard_deviations: value.sigma,
        corner_noise: noise,
        calibration_independence_evidence: None,
        fit_corners: value.fitting_scalar_observations / 2,
        nuisance_parameters: 6 * value.fitting_views,
        residual_degrees_of_freedom: value.fitting_scalar_observations
            - 9
            - 6 * value.fitting_views,
        no_claim: "Local linearized covariance conditional on independent isotropic corner noise and exact reference geometry. No correlated detector noise, print/flatness error, lens drift, empirical coverage or real-camera accuracy claim.",
    }
}

impl IntrinsicUncertainty {
    /// Carry a checked shared uncertainty report into photo propagation without a second
    /// numerical estimator or loss of its noise/independence assumptions. Unavailable is refused.
    pub fn from_calibration(report: &CalibrationUncertainty) -> Result<Self, Refusal> {
        report.input.validate()?;
        if report.schema != "ftx-camera-calibration-uncertainty/1"
            || report.status != "AVAILABLE"
            || !report.flags.is_empty()
        {
            return Err(invalid("shared calibration uncertainty is unavailable"));
        }
        let estimate = report
            .estimate
            .clone()
            .ok_or_else(|| invalid("missing intrinsic covariance"))?;
        let mut result = from_estimate(
            estimate,
            CornerNoise {
                model: "independent_isotropic".into(),
                sigma_px: report.input.sigma_px,
                evidence: report.input.evidence.clone(),
            },
        );
        result.calibration_independence_evidence = Some(report.input.independence_evidence.clone());
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::uncertainty::{CalibrationNoise, LENS_PARAMETERS};
    use super::*;
    use crate::camera::Distortion;
    use ftx_core::Vec2;

    #[test]
    fn shared_covariance_is_transported_exactly_with_its_assumptions() {
        let k = Intrinsics {
            focal_px: Vec2::new(100., 80.),
            principal_px: Vec2::new(50., 50.),
            distortion: Distortion::default(),
        };
        let covariance =
            std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 4. } else { 0.5 }));
        let mut report = CalibrationUncertainty {
            schema: "ftx-camera-calibration-uncertainty/1",
            status: "AVAILABLE",
            input: CalibrationNoise {
                schema: "ftx-calibration-corner-noise/1".into(),
                sigma_px: 0.5,
                evidence: "Synthetic noise".into(),
                independence_evidence: "Independent synthetic draws".into(),
            },
            estimate: Some(SharedEstimate {
                intrinsics: k,
                covariance,
                sigma: [2.; 9],
                parameters: LENS_PARAMETERS,
                fitting_views: 4,
                fitting_scalar_observations: 128,
            }),
            flags: vec![],
            no_claim: "Synthetic only",
        };
        let result = IntrinsicUncertainty::from_calibration(&report).unwrap();
        assert_eq!(result.covariance, covariance);
        assert_eq!(result.standard_deviations, [2.; 9]);
        assert_eq!(
            result.calibration_independence_evidence.as_deref(),
            Some("Independent synthetic draws")
        );
        assert_eq!(result.residual_degrees_of_freedom, 95);
        report.status = "UNAVAILABLE";
        assert!(IntrinsicUncertainty::from_calibration(&report).is_err());
    }
}
