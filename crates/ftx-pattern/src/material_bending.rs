//! Explicit orthotropic panel bending, independent of membrane stiffness and scalar seam hinges.
//! No solid-plate thickness-cubed or fiber-mixture rule is inferred for textile constructions.

use ftx_core::{Refusal, RefusalCode};
use serde::Serialize;

use crate::material_catalog::ResolvedMaterial;
use crate::materials::Form;

/// Explicit opt-in in `material.structure.bending_model`.
pub const MODEL: &str = "orthotropic_quadratic_patch/1";

/// Normal-projected second fundamental form, in reference material coordinates.
pub const NORMAL_MODEL: &str = "orthotropic_normal_patch/1";

/// Signed preferred covariant curvature [b_uu, b_vv, 2 b_uv], in inverse reference metres.
pub const PREFERRED_CURVATURE_PROPERTIES: [&str; 3] = [
    "preferred_curvature_warp",
    "preferred_curvature_weft",
    "preferred_curvature_twist",
];

/// Positive-definite bending resultant matrix in [k_uu, k_vv, 2 k_uv] order, in N m.
/// u follows warp/wale/grain; v follows weft/course. This is not membrane Poisson coupling.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BendingTensor {
    matrix_nm: [[f64; 3]; 3],
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    normal_projection: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    preferred_curvature_m_inv: Option<[f64; 3]>,
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
            normal_projection: false,
            preferred_curvature_m_inv: None,
        })
    }

    /// Bending matrix, in N m, conjugate to engineering curvature [k_uu, k_vv, 2 k_uv].
    #[must_use]
    pub fn matrix(&self) -> [[f64; 3]; 3] {
        self.matrix_nm
    }

    /// Use normal-projected curvature instead of the quasi-isometric vector Hessian.
    /// The independent constitutive matrix is unchanged; this does not infer stretch-dependent D.
    #[must_use]
    pub fn normal_projected(mut self) -> Self {
        self.normal_projection = true;
        self
    }

    /// Whether to project the reconstructed second derivatives onto the current surface normal.
    #[must_use]
    pub fn uses_normal(&self) -> bool {
        self.normal_projection
    }

    /// Specify preferred curvature without changing the reference membrane metric or the tensor.
    /// Sign is relative to x_u cross x_v, with u along grain and v its positive in-plane normal.
    /// An incompatible target (e.g. spherical curvature with a flat metric) may retain residual stress.
    ///
    /// # Errors
    /// Vector-Hessian kinematics, non-finite curvature, or overflowing stored energy at flatness.
    pub fn with_preferred_curvature(mut self, curvature: [f64; 3]) -> Result<Self, Refusal> {
        let stored: f64 = (0..3)
            .flat_map(|i| (0..3).map(move |j| self.matrix_nm[i][j] * curvature[i] * curvature[j]))
            .sum();
        if !self.uses_normal() || curvature.iter().any(|v| !v.is_finite()) || !stored.is_finite() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "preferred curvature requires normal-projected bending, finite inverse metres and finite flat energy",
            ));
        }
        self.preferred_curvature_m_inv = Some(curvature);
        Ok(self)
    }

    /// Preferred covariant curvature in inverse reference metres. Absence retains flat-reference
    /// bending exactly; this is not a radius, current principal curvature, or a stress-free metric.
    #[must_use]
    pub fn preferred_curvature(&self) -> [f64; 3] {
        self.preferred_curvature_m_inv.unwrap_or([0.0; 3])
    }

    /// Exact selected kinematic model, independent of the material's display name.
    #[must_use]
    pub fn model(&self) -> &'static str {
        if self.normal_projection {
            NORMAL_MODEL
        } else {
            MODEL
        }
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
        if (model != MODEL && model != NORMAL_MODEL)
            || matches!(material.form, Form::Fiber | Form::Yarn | Form::Fill)
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "unsupported panel bending model or material form",
            ));
        }
        let value = |key| material.require(key, "N*m", self.allows_estimates());
        let mut law = BendingTensor::new(
            value("bending_warp")?,
            value("bending_weft")?,
            value("bending_coupling")?,
            value("bending_twist")?,
        )?;
        if model == NORMAL_MODEL {
            law = law.normal_projected();
        }
        if PREFERRED_CURVATURE_PROPERTIES
            .iter()
            .any(|key| material.properties.contains_key(*key))
        {
            let mut curvature = [0.0; 3];
            for (out, key) in curvature.iter_mut().zip(PREFERRED_CURVATURE_PROPERTIES) {
                *out = material.require(key, "1/m", self.allows_estimates())?;
            }
            law = law.with_preferred_curvature(curvature)?;
        }
        Ok(Some(law))
    }
}
