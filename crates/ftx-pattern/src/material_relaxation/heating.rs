//! Explicit conversion of qualified Maxwell dashpot work, never spring storage or numerical loss.

use ftx_core::Refusal;
use serde::Serialize;

use super::bad;
use crate::material_catalog::ResolvedMaterial;

/// Optional scalar observation consumed only by the relaxation-to-heat path.
pub const HEAT_FRACTION_PROPERTY: &str = "membrane_relaxation_heat_fraction";
/// The observation qualifies physical viscous work in the existing effective-Green Maxwell law.
pub const HEAT_WORK_BASIS: &str = "membrane_maxwell_viscous_dissipation";

/// Source-qualified fraction of physical Maxwell dissipation supplied as sensible sheet heat.
/// It does not qualify material temperature feedback, contact heating or numerical dissipation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RelaxationHeatFraction {
    /// Explicit dimensionless fraction in [0,1]; no default conversion is inferred.
    pub fraction: f64,
    /// Inclusive characterized material-temperature interval in degrees Celsius.
    pub temperature_range_c: [f64; 2],
}

impl ResolvedMaterial {
    /// Resolve conversion only for an explicitly requested thermal consumer. An inactive law
    /// returns None; every active law requires an observation, including an explicit zero. All
    /// proportional/directional branches and the consumed equilibrium response must refer to the
    /// law's declared specimen and conditions. Mechanical-only consumers retain their behavior.
    ///
    /// # Errors
    /// Missing/incompatible relaxation, unqualified conversion, mismatched observations, wrong
    /// work basis, unapproved estimates or invalid fraction/temperature bounds.
    pub fn membrane_relaxation_heat_fraction(
        &self,
    ) -> Result<Option<RelaxationHeatFraction>, Refusal> {
        if self.membrane_relaxation()?.is_none() {
            return Ok(None);
        }
        let material = self.material();
        let law = material
            .membrane_relaxation
            .as_ref()
            .expect("compiled active law");
        for identity in ["specimen_id", "conditioning"] {
            if law
                .conditions
                .get(identity)
                .is_none_or(|s| s.trim().is_empty())
            {
                return Err(bad(format!(
                    "Maxwell heat needs the law's explicit {identity}"
                )));
            }
        }
        // Proportional-only spectra historically permitted unspecified specimen conditions.
        // Heat must not turn that compatibility allowance into source-qualified dissipation.
        for branch in &law.branches {
            law.matching_conditions(&branch.stiffness_ratio.conditions)?;
            law.matching_conditions(&branch.relaxation_time.conditions)?;
        }
        // Directional branch conditions were checked by the ordinary compiler. Check the actual
        // equilibrium authority for ALL spectra, including the proportional-only legacy form.
        if let Some(response) = &material.membrane_response {
            for conditions in [
                &response.warp.conditions,
                &response.weft.conditions,
                &response.shear.conditions,
                &response.coupling.conditions,
            ] {
                law.matching_conditions(conditions)?;
            }
        } else {
            for key in [
                "warp_stiffness",
                "weft_stiffness",
                "poisson_warp_weft",
                "shear_stiffness",
            ] {
                let q = material
                    .properties
                    .get(key)
                    .ok_or_else(|| bad("Maxwell heat needs a characterized equilibrium tensor"))?;
                law.matching_conditions(&q.conditions)?;
            }
        }
        let fraction = material.require(HEAT_FRACTION_PROPERTY, "1", self.allows_estimates())?;
        let observation = &material.properties[HEAT_FRACTION_PROPERTY];
        law.matching_conditions(&observation.conditions)?;
        if !(0.0..=1.0).contains(&fraction)
            || observation
                .bounds
                .is_some_and(|[lo, hi]| lo < 0. || hi > 1.)
        {
            return Err(bad("Maxwell heat fraction and bounds must lie in [0,1]"));
        }
        if observation.conditions.get("work_basis").map(String::as_str) != Some(HEAT_WORK_BASIS) {
            return Err(bad(
                "Maxwell heat needs membrane_maxwell_viscous_dissipation work basis",
            ));
        }
        let mut temperature_range_c = [0.; 2];
        for (i, name) in ["temperature_min_c", "temperature_max_c"]
            .iter()
            .enumerate()
        {
            temperature_range_c[i] = observation
                .conditions
                .get(*name)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= -273.15)
                .ok_or_else(|| bad(format!("Maxwell heat needs finite {name} >= -273.15")))?;
        }
        if temperature_range_c[0] > temperature_range_c[1] {
            return Err(bad("Maxwell heat temperature range must be ordered"));
        }
        Ok(Some(RelaxationHeatFraction {
            fraction,
            temperature_range_c,
        }))
    }
}
