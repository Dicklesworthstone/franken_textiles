//! Bind relaxation to the same source-locked normalized material as all existing consumers.

use super::CompiledRelaxation;
use crate::material_catalog::ResolvedMaterial;
use crate::materials::Mechanics;
use ftx_core::Refusal;

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
    /// Missing mechanics, invalid relaxation data or unapproved estimates.
    pub fn membrane_relaxation(&self) -> Result<Option<CompiledRelaxation>, Refusal> {
        self.material()
            .membrane_relaxation
            .as_ref()
            .map(|r| {
                r.compile(
                    &self.material().sources,
                    self.allows_estimates(),
                    self.mechanics()?.origin_tangent(),
                )
            })
            .transpose()
    }
}
