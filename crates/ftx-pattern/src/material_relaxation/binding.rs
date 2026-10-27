//! Bind relaxation to the same source-locked normalized material as all existing consumers.

use super::CompiledRelaxation;
use crate::material_catalog::ResolvedMaterial;
use crate::materials::{Form, Mechanics};
use ftx_core::{Refusal, RefusalCode};

impl Mechanics {
    /// Equilibrium membrane tangent at zero strain, N/m, using the solver's orthotropic convention.
    #[must_use]
    pub fn origin_tangent(&self) -> [[f64; 3]; 3] {
        let nu21 = self.poisson * self.weft_n_m / self.warp_n_m;
        let d = 1.0 - self.poisson * nu21;
        [
            [self.warp_n_m / d, self.poisson * self.weft_n_m / d, 0.0],
            [self.poisson * self.weft_n_m / d, self.weft_n_m / d, 0.0],
            [0.0, 0.0, self.shear_n_m],
        ]
    }
}

impl ResolvedMaterial {
    /// Compile optional time-dependent membrane response under this selection's evidence policy.
    /// The existing scalar/nonlinear law is EQUILIBRIUM stiffness; no instantaneous reinterpretation.
    ///
    /// # Errors
    /// Missing membrane characterization, invalid relaxation data or unapproved estimates.
    /// Mass, bending and contact observations are required only by consumers that use them.
    pub fn membrane_relaxation(&self) -> Result<Option<CompiledRelaxation>, Refusal> {
        self.material()
            .membrane_relaxation
            .as_ref()
            .map(|r| {
                let material = self.material();
                if matches!(material.form, Form::Fiber | Form::Yarn | Form::Fill) {
                    return Err(Refusal::new(
                        RefusalCode::InvalidDocument,
                        "membrane relaxation requires a characterized sheet",
                    ));
                }
                let plastic = material.compiled_membrane_plasticity(self.allows_estimates())?;
                if r.parallel_plasticity.is_some() {
                    return super::parallel::compile(
                        material,
                        self.allows_estimates(),
                        plastic.as_ref().ok_or_else(|| {
                            Refusal::new(
                                RefusalCode::InvalidDocument,
                                "parallel network lacks its plastic branch",
                            )
                        })?,
                        self.sha256().to_owned(),
                    );
                }
                r.validate_equilibrium(material)?;
                // Match the ordinary mechanics compiler's reference conversion exactly, without
                // making a membrane-only coupon invent thickness, bending or interface friction.
                let [warp, weft, poisson, shear] = if let Some(law) = self.membrane_response()? {
                    law.reference_moduli()?
                } else {
                    [
                        material.require("warp_stiffness", "N/m", self.allows_estimates())?,
                        material.require("weft_stiffness", "N/m", self.allows_estimates())?,
                        material.require("poisson_warp_weft", "1", self.allows_estimates())?,
                        material.require("shear_stiffness", "N/m", self.allows_estimates())?,
                    ]
                };
                let d = 1.0 - poisson * (poisson * weft / warp);
                let tangent = [
                    [warp / d, poisson * weft / d, 0.0],
                    [poisson * weft / d, weft / d, 0.0],
                    [0.0, 0.0, shear],
                ];
                r.compile(&material.sources, self.allows_estimates(), tangent)
            })
            .transpose()
    }
}
