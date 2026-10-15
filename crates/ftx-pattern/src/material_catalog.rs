//! Bundled catalog, dimensional normalization and immutable selections of [`crate::materials`].
//!
//! This is a data adapter, not another material model: all physics consumers use the existing
//! `Material`, `Mechanics` and `ThermalStack` types. The authored observations survive normalization.

use crate::materials::{Catalog, Material, Mechanics, Quantity, Source};
use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod mit;
mod nist;

/// A selection for `fabric.material`. A frozen record makes custom/edited patterns self-contained.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Exact catalog ID, also required to match an embedded record.
    pub id: String,
    /// Optional authored material snapshot; otherwise resolve the ID against the catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<Material>,
    /// Explicit, source-bearing property replacements. No mixture or thickness law is inferred.
    #[serde(default)]
    pub overrides: BTreeMap<String, Quantity>,
    /// Additional replacement sources. Existing source IDs cannot be rebound to different evidence.
    #[serde(default)]
    pub sources: BTreeMap<String, Source>,
    /// Permit estimated load-bearing properties; false unless explicitly selected.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Optional digest lock, checked after normalization and override resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_sha256: Option<String>,
}

/// An immutable, source-complete snapshot. A hash is an identity, not physical validation.
#[derive(Clone, Debug, Serialize)]
pub struct ResolvedMaterial {
    schema: &'static str,
    authored: Material,
    normalized: Material,
    allow_estimates: bool,
    sha256: String,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// Canonical units of consumable properties. Breaking force and stiffness are intentionally distinct.
/// Unknown extension properties remain data but are never automatically consumed.
#[must_use]
pub fn property_unit(key: &str) -> Option<&'static str> {
    Some(match key {
        "areal_density" => "kg/m2",
        "thickness" | "width" | "fiber_diameter" | "staple_length" => "m",
        "bulk_density" => "kg/m3",
        "warp_stiffness" | "weft_stiffness" | "shear_stiffness" => "N/m",
        "bending_rigidity"
        | "bending_warp"
        | "bending_weft"
        | "bending_coupling"
        | "bending_twist"
        | "bending_hardening_warp"
        | "bending_hardening_weft"
        | "bending_hardening_twist" => "N*m",
        "specimen_flexural_rigidity" => "N*m2",
        "preferred_curvature_warp" | "preferred_curvature_weft" | "preferred_curvature_twist" => {
            "1/m"
        }
        "bending_curvature_limit_warp"
        | "bending_curvature_limit_weft"
        | "bending_curvature_limit_twist"
        | "bending_accumulation_limit_warp"
        | "bending_accumulation_limit_weft"
        | "bending_accumulation_limit_twist" => "1/m",
        "poisson_warp_weft"
        | "friction_skin"
        | "friction_self"
        | "elongation_break_warp"
        | "elongation_break_weft"
        | "moisture_regain"
        | "shrinkage_warp"
        | "shrinkage_weft"
        | "porosity"
        | "emissivity"
        | "limiting_oxygen_index" => "1",
        "thermal_conductivity" => "W/(m*K)",
        "thermal_resistance" => "m2*K/W",
        "specific_heat" => "J/(kg*K)",
        "vapor_resistance" => "m2*Pa/W",
        "air_permeability" => "m/s",
        "breaking_force_warp"
        | "breaking_force_weft"
        | "tear_force"
        | "cut_force"
        | "needle_penetration_force"
        | "bending_yield_warp"
        | "bending_yield_weft"
        | "bending_yield_twist" => "N",
        "abrasion_cycles" => "cycles",
        "yarn_linear_density" => "kg/m",
        "thread_density_warp" | "thread_density_weft" | "twist" => "1/m",
        _ => return None,
    })
}

fn factor(unit: &str, expected: &str) -> Option<f64> {
    if unit == expected {
        return Some(1.0);
    }
    Some(match (unit, expected) {
        ("g/m2", "kg/m2") => 0.001,
        ("oz/yd2", "kg/m2") => 0.028_349_523_125 / (0.9144 * 0.9144),
        ("mm", "m") => 0.001,
        ("cm", "m") => 0.01,
        ("um", "m") => 1e-6,
        ("in", "m") => 0.0254,
        ("g/cm3", "kg/m3") => 1000.0,
        ("kN/m", "N/m") | ("kN", "N") => 1000.0,
        ("mN*m", "N*m") => 0.001,
        ("lbf*in2", "N*m2") => 4.448_221_615_260_5 * 0.0254 * 0.0254,
        ("%", "1") => 0.01,
        ("clo", "m2*K/W") => 0.155,
        ("tog", "m2*K/W") => 0.1,
        ("tex", "kg/m") => 1e-6,
        ("denier", "kg/m") => 1e-3 / 9000.0,
        ("1/in", "1/m") => 1.0 / 0.0254,
        ("1/cm", "1/m") => 100.0,
        ("1/mm", "1/m") => 1000.0,
        _ => return None,
    })
}

/// Copy a material into canonical units without mutating its source observations.
///
/// # Errors
/// Invalid provenance/values, dimensional mismatch or non-finite converted values/bounds.
pub fn normalize(material: &Material) -> Result<Material, Refusal> {
    material.validate()?;
    let mut normalized = material.clone();
    for (key, datum) in &mut normalized.properties {
        let Some(unit) = property_unit(key) else {
            continue;
        };
        let scale = factor(&datum.unit, unit).ok_or_else(|| {
            invalid(format!(
                "{}: {key} unit {} is not convertible to {unit}",
                material.id, datum.unit
            ))
        })?;
        datum.value *= scale;
        datum.bounds = datum.bounds.map(|b| b.map(|v| v * scale));
        if !datum.value.is_finite()
            || datum
                .bounds
                .is_some_and(|b| !b.iter().all(|v| v.is_finite()))
        {
            return Err(invalid(format!(
                "{}: {key} unit conversion overflows",
                material.id
            )));
        }
        datum.unit = unit.into();
    }
    normalized.validate()?;
    Ok(normalized)
}

impl Catalog {
    /// The offline factual seed, preserving authored units. Missing measurements remain absent.
    ///
    /// # Errors
    /// A malformed bundled catalog is a refusal, never a panic or silent default.
    pub fn builtin() -> Result<Self, Refusal> {
        let mut catalog = Self::from_json(include_str!("material_catalog/catalog.json"))?;
        let insulation = Self::from_json(include_str!("material_catalog/insulation.json"))?;
        catalog.materials.extend(insulation.materials);
        let measured = Self::from_json(include_str!("material_catalog/thermal-measurements.json"))?;
        catalog.materials.extend(measured.materials);
        let hfm = Self::from_json(include_str!("material_catalog/hfm-runs.json"))?;
        catalog.materials.extend(hfm.materials);
        catalog.materials.extend(nist::records());
        catalog.materials.extend(mit::records());
        catalog.validate()?;
        Ok(catalog)
    }

    /// Resolve an exact ID or frozen record, then source-preserving overrides, without mutating data.
    ///
    /// # Errors
    /// Invalid catalog, unknown ID, mismatched record ID, rebound source, wrong units or digest lock.
    pub fn resolve(&self, selection: &Selection) -> Result<ResolvedMaterial, Refusal> {
        self.validate()?;
        let mut material = match &selection.record {
            Some(record) if record.id == selection.id => record.clone(),
            Some(_) => return Err(invalid("embedded material ID differs from the selected ID")),
            None => self.get(&selection.id)?.clone(),
        };
        for (id, source) in &selection.sources {
            if material.sources.get(id).is_some_and(|old| {
                old.reference != source.reference
                    || old.accessed_on != source.accessed_on
                    || old.note != source.note
            }) {
                return Err(invalid(format!(
                    "source {id} cannot be rebound; use a new source ID"
                )));
            }
            material.sources.insert(id.clone(), source.clone());
        }
        material.properties.extend(selection.overrides.clone());
        let normalized = normalize(&material)?;
        let schema = "ftx-resolved-material/1";
        let bytes =
            serde_json::to_vec(&(schema, &material, &normalized, selection.allow_estimates))
                .map_err(|e| invalid(format!("material snapshot: {e}")))?;
        let sha256 = sha256_hex(&bytes);
        if selection
            .expected_sha256
            .as_ref()
            .is_some_and(|expected| expected != &sha256)
        {
            return Err(invalid(
                "resolved material does not match expected_sha256; no substituted material is allowed",
            ));
        }
        Ok(ResolvedMaterial {
            schema,
            authored: material,
            normalized,
            allow_estimates: selection.allow_estimates,
            sha256,
        })
    }
}

impl ResolvedMaterial {
    /// Authored observations, retaining original units, bounds, evidence, source and conditions.
    #[must_use]
    pub fn authored(&self) -> &Material {
        &self.authored
    }

    /// The same existing material type, normalized for all downstream consumers.
    #[must_use]
    pub fn material(&self) -> &Material {
        &self.normalized
    }

    /// The selected evidence policy; consumers must not silently relax it.
    #[must_use]
    pub fn allows_estimates(&self) -> bool {
        self.allow_estimates
    }

    /// Identity of the full source-bearing snapshot and normalization result.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Resolve the common orthotropic sheet law under the selected evidence policy.
    ///
    /// # Errors
    /// The existing material consumer's missing, unsupported, estimated or nonphysical-data refusals.
    pub fn mechanics(&self) -> Result<Mechanics, Refusal> {
        self.normalized.mechanics(self.allow_estimates)
    }

    /// Compile the optional nonlinear membrane response under this frozen selection's evidence policy.
    ///
    /// # Errors
    /// Invalid response/source data or missing consent for interpolated/estimated observations.
    pub fn membrane_response(
        &self,
    ) -> Result<Option<crate::material_response::CompiledMembrane>, Refusal> {
        self.normalized.compiled_membrane(self.allow_estimates)
    }

    /// Resolve dry thermal resistance under exactly the same evidence policy.
    ///
    /// # Errors
    /// The existing thermal consumer's missing, estimated, wrong-unit or nonphysical-data refusals.
    pub fn thermal_resistance(&self) -> Result<f64, Refusal> {
        self.normalized.thermal_resistance(self.allow_estimates)
    }

    /// Produce an embedded selection for a portable pattern, with a content lock and no live lookup.
    #[must_use]
    pub fn frozen_selection(&self) -> Selection {
        Selection {
            id: self.authored.id.clone(),
            record: Some(self.authored.clone()),
            allow_estimates: self.allow_estimates,
            expected_sha256: Some(self.sha256.clone()),
            ..Selection::default()
        }
    }
}
