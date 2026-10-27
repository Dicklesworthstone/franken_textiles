//! Qualified parallel elastoplastic and Maxwell branches in TOTAL physical Green strain.
//!
//! The permanent-strain return map sees only its own equilibrium-branch stress. Every Maxwell
//! branch sees total Green strain minus its own viscous strain, not the plastic strain. This is
//! an explicit parallel rheological network, not same-branch viscoplasticity or a textile default.

use std::collections::BTreeMap;

use ftx_core::{Refusal, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use super::{CompiledRelaxation, bad, source_ok};
use crate::{
    material_membrane_plasticity::{
        AXES, MODEL as PLASTIC_MODEL, PROPERTY_FAMILIES, PlasticMembrane,
    },
    materials::{Evidence, Material, Source},
};

/// Explicit parallel network; the original four elastic constants belong to its plastic branch.
pub const MODEL: &str = "parallel_green_maxwell_plastic/1";

/// Qualification of the NETWORK, independent of the evidence for its individual coefficients.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParallelPlasticity {
    /// Exactly [`MODEL`]. Both branches use raw [E11,E22,2E12], without gather softening.
    pub model: String,
    /// Evidence for this branch interpretation and combined applicability. Estimates require consent.
    pub evidence: Evidence,
    /// Source in the containing material's source map; no new physical numbers are inferred.
    pub source: String,
    /// Characterization or explicit modeling method establishing the parallel split.
    pub method: String,
    /// Shared specimen_id, conditioning and other declared qualifications, matched by every
    /// elastic, plastic, Maxwell and domain observation. Acquisition-specific extras may differ.
    pub conditions: BTreeMap<String, String>,
}

impl ParallelPlasticity {
    pub(super) fn validate(
        &self,
        sources: &BTreeMap<String, Source>,
        allow: bool,
    ) -> Result<(), Refusal> {
        if self.model != MODEL
            || self.method.trim().is_empty()
            || !source_ok(&self.source, sources)
            || (self.evidence == Evidence::Estimate && !allow)
            || self.conditions.len() > 64
            || self
                .conditions
                .iter()
                .any(|(k, v)| k.trim().is_empty() || v.trim().is_empty())
            || ["specimen_id", "conditioning"]
                .iter()
                .any(|key| !self.conditions.contains_key(*key))
        {
            return Err(bad(
                "parallel membrane network needs its exact model, source, method, specimen/conditioning and independent estimate consent",
            ));
        }
        Ok(())
    }

    fn matches(&self, conditions: &BTreeMap<String, String>) -> Result<(), Refusal> {
        if self
            .conditions
            .iter()
            .any(|(key, value)| conditions.get(key) != Some(value))
        {
            return Err(bad(
                "every parallel membrane observation must match the declared specimen and conditions",
            ));
        }
        Ok(())
    }
}

/// Only the complete material compiler creates this token; neither public construction nor
/// deserialization can turn two independently compiled laws into an admitted joint network.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Lock {
    pub(crate) material_sha256: String,
    pub(crate) strain_range: [[f64; 2]; 3],
}

/// Compile both branch interpretations against the same complete source-bearing material. The
/// supplied plastic law is already validated; this function never calls back into its compiler.
pub(crate) fn compile(
    material: &Material,
    allow: bool,
    plastic: &PlasticMembrane,
    material_sha256: String,
) -> Result<CompiledRelaxation, Refusal> {
    compile_inner(material, allow, plastic, material_sha256, false)
}

// Identification already validates the independent elastic/domain observations. Its six unknown
// yield/hardening coefficients are trial parameters, not fabricated source observations. This
// compiler only supplies a fixed transient response to that private calibration path; it cannot
// create a paired plastic law or publish a material. Publication uses the full compiler above.
pub(crate) fn compile_for_calibration(
    material: &Material,
    allow: bool,
    plastic: &PlasticMembrane,
) -> Result<CompiledRelaxation, Refusal> {
    let identity = sha256_hex(&serde_json::to_vec(material).map_err(|e| bad(e.to_string()))?);
    let mut compiled = compile_inner(material, allow, plastic, identity, true)?;
    compiled.parallel = None;
    Ok(compiled)
}

fn compile_inner(
    material: &Material,
    allow: bool,
    plastic: &PlasticMembrane,
    material_sha256: String,
    calibration: bool,
) -> Result<CompiledRelaxation, Refusal> {
    let relaxation = material
        .membrane_relaxation
        .as_ref()
        .ok_or_else(|| bad("parallel network requires Maxwell branches"))?;
    let declaration = relaxation
        .parallel_plasticity
        .as_ref()
        .ok_or_else(|| bad("parallel network requires an explicit joint qualification"))?;
    declaration.validate(&material.sources, allow)?;
    if material.membrane_response.is_some()
        || match material.structure.get("membrane_plasticity_model") {
            Some(model) => model != PLASTIC_MODEL,
            None => !calibration,
        }
        || relaxation.conditions != declaration.conditions
    {
        return Err(bad(
            "parallel network requires the scalar elastoplastic equilibrium model and exactly matching shared Maxwell conditions",
        ));
    }
    relaxation.validate_equilibrium(material)?;
    for key in [
        "warp_stiffness",
        "weft_stiffness",
        "poisson_warp_weft",
        "shear_stiffness",
    ] {
        declaration.matches(
            &material
                .properties
                .get(key)
                .ok_or_else(|| bad("parallel network lacks an equilibrium observation"))?
                .conditions,
        )?;
    }
    for (family, _) in PROPERTY_FAMILIES {
        if calibration && matches!(family, "membrane_yield" | "membrane_hardening") {
            continue;
        }
        for axis in AXES {
            declaration.matches(
                &material
                    .properties
                    .get(&format!("{family}_{axis}"))
                    .ok_or_else(|| bad("parallel network lacks a permanent-strain observation"))?
                    .conditions,
            )?;
        }
    }
    for branch in &relaxation.branches {
        declaration.matches(&branch.stiffness_ratio.conditions)?;
        declaration.matches(&branch.relaxation_time.conditions)?;
    }
    for branch in &relaxation.orthotropic_branches {
        for quantity in [
            &branch.stiffness.warp,
            &branch.stiffness.weft,
            &branch.stiffness.coupling,
            &branch.stiffness.shear,
            &branch.relaxation_time,
        ] {
            declaration.matches(&quantity.conditions)?;
        }
    }
    let mut compiled = relaxation.compile_inner(&material.sources, allow, plastic.tensor())?;
    let limits = plastic.strain_limits();
    let fitted =
        crate::material_calibration::fitted_domain(material)?.map(|domain| domain.ranges());
    for (i, limit) in limits.into_iter().enumerate() {
        compiled.range[i][0] = compiled.range[i][0].max(-limit);
        compiled.range[i][1] = compiled.range[i][1].min(limit);
        if let Some(fitted) = fitted {
            compiled.range[i][0] = compiled.range[i][0].max(fitted[i][0]);
            compiled.range[i][1] = compiled.range[i][1].min(fitted[i][1]);
        }
        let [lo, hi] = compiled.range[i];
        if lo > 0.0 || hi < 0.0 || lo >= hi {
            return Err(bad(
                "parallel network needs a nonempty common physical Green-strain domain containing zero",
            ));
        }
    }
    let lock = Lock {
        material_sha256,
        strain_range: compiled.range,
    };
    compiled.fingerprint = sha256_hex(
        &serde_json::to_vec(&(MODEL, &compiled.fingerprint, &lock))
            .map_err(|error| bad(error.to_string()))?,
    );
    compiled.parallel = Some(lock);
    Ok(compiled)
}
