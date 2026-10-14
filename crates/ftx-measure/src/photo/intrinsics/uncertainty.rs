//! First-order Brown-lens precision under declared independent, equal-variance corner noise.
//! Use the undamped joint lens/pose Jacobian; taking only the lens Hessian would incorrectly
//! treat the estimated board poses as exact. No noise level is inferred from fit residuals.

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::{CalibrationInput, CalibrationReport, calibrate_camera, refine};
use crate::{camera::Intrinsics, invalid};

/// Order of both axes of every intrinsic covariance matrix. Focal/principal coordinates are pixels;
/// Brown coefficients are dimensionless. This is not OpenCV's coefficient storage order.
pub const LENS_PARAMETERS: [&str; 9] = ["fx", "fy", "cx", "cy", "k1", "k2", "k3", "p1", "p2"];

/// Explicit homoscedastic, independent x/y error model for fitting calibration corners.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationNoise {
    /// Must be `ftx-calibration-corner-noise/1`.
    pub schema: String,
    /// One-sigma error per coordinate in original oriented image pixels, in (0, 1000].
    pub sigma_px: f64,
    /// Source or measurement supporting the stated corner uncertainty.
    pub evidence: String,
    /// Justification for independent errors across coordinates, corners and fitting images.
    /// Correlated detector or reference errors cannot be represented by this model.
    pub independence_evidence: String,
}

impl CalibrationNoise {
    /// Reject missing assumptions or invalid noise without silently deriving sigma from residuals.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-calibration-corner-noise/1"
            || !self.sigma_px.is_finite()
            || !(0.0 < self.sigma_px && self.sigma_px <= 1000.0)
            || !super::evidence(&self.evidence)
            || !super::evidence(&self.independence_evidence)
        {
            return Err(invalid(
                "corner noise requires schema /1, 0 < sigma_px <= 1000, evidence and independence_evidence",
            ));
        }
        Ok(())
    }
}

/// Marginal lens covariance after accounting for every fitted board pose. Constructed only by
/// the checked calibration pipeline; serialized matrices are not independently trusted inputs.
#[derive(Clone, Debug, Serialize)]
pub struct IntrinsicUncertainty {
    pub(crate) intrinsics: Intrinsics,
    pub(crate) covariance: [[f64; 9]; 9],
    pub(crate) sigma: [f64; 9],
    pub(crate) parameters: [&'static str; 9],
    pub(crate) fitting_views: usize,
    pub(crate) fitting_scalar_observations: usize,
}

impl IntrinsicUncertainty {
    /// The exact, unrounded lens to which this covariance belongs.
    #[must_use]
    pub fn intrinsics(&self) -> Intrinsics {
        self.intrinsics
    }

    /// Symmetric covariance in physical parameter units, in [`LENS_PARAMETERS`] order.
    #[must_use]
    pub fn covariance(&self) -> &[[f64; 9]; 9] {
        &self.covariance
    }

    /// Marginal one-sigma values, not simultaneous confidence limits or accuracy guarantees.
    #[must_use]
    pub fn sigma(&self) -> &[f64; 9] {
        &self.sigma
    }
}

/// Optional precision diagnostic; unavailable estimates never publish zero covariance.
#[derive(Clone, Debug, Serialize)]
pub struct CalibrationUncertainty {
    /// `ftx-camera-calibration-uncertainty/1`.
    pub schema: &'static str,
    /// AVAILABLE or UNAVAILABLE, separate from the parent calibration's acceptance.
    pub status: &'static str,
    /// All declared noise assumptions.
    pub input: CalibrationNoise,
    /// Lens estimate and full marginal covariance, absent on any failed gate.
    pub estimate: Option<IntrinsicUncertainty>,
    /// Numerical or parent-calibration failures.
    pub flags: Vec<String>,
    /// Method and limits of the conditional approximation.
    pub no_claim: &'static str,
}

/// Run the unchanged Brown estimator, then compute sigma² times the lens block of (JᵀJ)⁻¹.
/// J includes only fit corners in fit views, with all fitted poses marginalized, no damping,
/// priors, residual-based noise estimate or held-out observations. This Gauss–Newton covariance
/// is a local small-noise approximation, not exact nonlinear estimator covariance.
///
/// Pinhole's algebraic estimator is intentionally not assigned this different estimator's
/// covariance. An unavailable precision estimate preserves geometry but has no usable matrix.
pub fn calibrate_camera_with_uncertainty(
    input: &CalibrationInput,
    noise: &CalibrationNoise,
) -> Result<CalibrationReport, Refusal> {
    noise.validate()?;
    if input.image_model != "brown5_zero_skew" {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "corner-noise covariance requires the nonlinear Brown estimator, not algebraic pinhole calibration",
        ));
    }
    let mut report = calibrate_camera(input)?;
    let result = if report.solution.is_some() {
        refine::covariance(&report, noise.sigma_px)
    } else {
        Err(invalid(
            "no accepted calibration for corner-noise propagation",
        ))
    };
    let (estimate, flags) = match result {
        Ok(value) => (Some(value), Vec::new()),
        Err(error) => (None, vec![error.message]),
    };
    report.conditional_uncertainty = Some(CalibrationUncertainty {
        schema: "ftx-camera-calibration-uncertainty/1",
        status: if estimate.is_some() {
            "AVAILABLE"
        } else {
            "UNAVAILABLE"
        },
        input: noise.clone(),
        estimate,
        flags,
        no_claim: "First-order Gauss-Newton covariance under stated independent equal-variance pixel noise, with fitted poses marginalized and metric controls exact. Does not cover correlated corner errors, print/flatness error, model bias, changed lens settings, unsampled-domain accuracy or real-wearer fit. Unchanged geometry is not evidence that this approximation is accurate.",
    });
    report.no_claim = "Local nonlinear Brown calibration; conditional corner-noise precision is reported separately and does not establish total calibration accuracy, global optimality or real-wearer fit. Sheet-local poses are not a common world frame when the sheet moves.";
    Ok(report)
}
