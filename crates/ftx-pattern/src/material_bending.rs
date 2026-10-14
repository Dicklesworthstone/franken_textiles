//! Explicit orthotropic panel bending, independent of membrane stiffness and scalar seam hinges.
//! No solid-plate thickness-cubed or fiber-mixture rule is inferred for textile constructions.

use ftx_core::{Refusal, RefusalCode};
use serde::Serialize;

use crate::material_catalog::ResolvedMaterial;
use crate::materials::Form;

/// Explicit opt-in in `material.structure.bending_model`.
pub const MODEL: &str = "orthotropic_quadratic_patch/1";

/// Positive-definite bending resultant matrix in [k_uu, k_vv, 2 k_uv] order, in N m.
/// u follows warp/wale/grain; v follows weft/course. This is not membrane Poisson coupling.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BendingTensor {
    matrix_nm: [[f64; 3]; 3],
}

impl BendingTensor {
    /// Construct an orthotropic bending law from independent D11, D22, D12 and D66 coefficients.
    ///
    /// # Errors
    /// Non-finite coefficients, nonpositive direct/twist stiffness or unstable normal coupling.
    pub fn new(warp: f64, weft: f64, coupling: f64, twist: f64) -> Result<Self, Refusal> {
        let scale = warp.max(weft);
        let a = warp / scale;
        let b = weft / scale;
        let c = coupling / scale;
        let schur = b - c * (c / a);
        if [warp, weft, coupling, twist].iter().any(|v| !v.is_finite())
            || warp <= 0.0
            || weft <= 0.0
            || twist <= 0.0
            || !schur.is_finite()
            || schur <= 0.0
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "panel bending needs finite positive D11/D22/D66 and positive-definite D12 coupling",
            ));
        }
        Ok(Self {
            matrix_nm: [
                [warp, coupling, 0.0],
                [coupling, weft, 0.0],
                [0.0, 0.0, twist],
            ],
        })
    }

    /// Bending matrix, in N m, conjugate to engineering curvature [k_uu, k_vv, 2 k_uv].
    #[must_use]
    pub fn matrix(&self) -> [[f64; 3]; 3] {
        self.matrix_nm
    }
}

impl ResolvedMaterial {
    /// Resolve an explicitly selected panel law under this snapshot's evidence policy.
    /// Merely storing a measured directional property does not activate a model or complete its data.
    ///
    /// # Errors
    /// Unknown model, unsupported physical form, missing/wrong-unit/unapproved/unstable coefficients.
    pub fn bending_response(&self) -> Result<Option<BendingTensor>, Refusal> {
        let material = self.material();
        let Some(model) = material.structure.get("bending_model") else {
            return Ok(None);
        };
        if model != MODEL || matches!(material.form, Form::Fiber | Form::Yarn | Form::Fill) {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "unsupported panel bending model or material form",
            ));
        }
        let value = |key| material.require(key, "N*m", self.allows_estimates());
        BendingTensor::new(
            value("bending_warp")?,
            value("bending_weft")?,
            value("bending_coupling")?,
            value("bending_twist")?,
        )
        .map(Some)
    }
}
