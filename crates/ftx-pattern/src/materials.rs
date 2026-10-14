//! Source-bearing textile records and the common material-resolution boundary.
//!
//! Fiber identity is not a fabric constitutive law. Properties belong to a particular yarn,
//! construction, finish and test condition. Missing numbers remain missing; estimates require an
//! explicit opt-in. No network access, filesystem access or solver dependency in this module.
//! See `docs/spec/06-textile-materials.md` for the property vocabulary and no-claim boundaries.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

/// Version of a portable, offline textile catalog.
pub const CATALOG_SCHEMA: &str = "ftx-textile-catalog/1";

/// Physical level of a record. Yarn properties never silently become fabric properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// Constituent fiber, not a sheet.
    Fiber,
    /// Spun or filament yarn, including sewing thread.
    Yarn,
    /// Interlaced warp and weft.
    Woven,
    /// Looped fabric; material axes denote wale and course.
    Knit,
    /// Felt, bonded web or other nonwoven sheet.
    Nonwoven,
    /// Leather or another hide sheet.
    Hide,
    /// Loose or batt insulation; not a load-bearing shell by default.
    Fill,
    /// Bonded multilayer sheet with its own measured effective properties.
    Laminate,
}

/// How a number was obtained, independent of its precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    /// Measured on the stated specimen and under the stated conditions.
    Measured,
    /// Published by the named supplier or primary source; not independently verified.
    Published,
    /// Inferred or chosen. Never usable without explicit permission.
    Estimate,
}

/// A citation or local measurement record, shared by quantities within one material.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// HTTP(S) URL, DOI URL, or local measurement identifier.
    pub reference: String,
    /// Retrieval or measurement date; ISO 8601 calendar date.
    pub accessed_on: String,
    /// Bibliographic or specimen details, not a copy of the source page.
    pub note: String,
}

/// One dimensional observation. Absence is represented by an absent property, never zero.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quantity {
    /// Central value in the explicitly named unit.
    pub value: f64,
    /// Canonical unit, e.g. `kg/m2`, `N/m`, `N*m`, `m`, or `1`.
    pub unit: String,
    /// Origin of the value.
    pub evidence: Evidence,
    /// Key in the containing material's source map.
    pub source: String,
    /// Test method or explicit conversion/estimation method.
    pub method: String,
    /// Test conditions, specimen direction, humidity, pressure, strain interval, etc.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Optional lower and upper bounds in the same unit; not assumed to be a confidence interval.
    #[serde(default)]
    pub bounds: Option<[f64; 2]>,
}

/// A constituent mass fraction, not a rule for mixing moduli.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    /// Fiber identity, including distinctions such as viscose-from-bamboo vs bast bamboo.
    pub fiber: String,
    /// Dry mass fraction in (0, 1].
    pub fraction: f64,
    /// Source key for the composition claim.
    pub source: String,
}

/// A textile specimen, product variant or explicitly identified generic family.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    /// Stable, versionable catalog identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Material level; resolves only at a supported physical level.
    pub form: Form,
    /// Fiber composition when known.
    #[serde(default)]
    pub composition: Vec<Component>,
    /// True only when the listed mass fractions describe the entire blend.
    #[serde(default)]
    pub composition_complete: bool,
    /// Construction, spinning, weave/knit, tanning, finish and commercial variant descriptors.
    #[serde(default)]
    pub structure: BTreeMap<String, String>,
    /// Sparse extensible properties. Unknown properties may be stored, never inferred or consumed.
    #[serde(default)]
    pub properties: BTreeMap<String, Quantity>,
    /// Optional source-bearing nonlinear membrane law. When present it replaces the four scalar
    /// warp/weft/Poisson/shear properties; mass, bending, thickness and interfaces remain ordinary
    /// observations so consumers can use them independently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membrane_response: Option<crate::material_response::TabulatedMembrane>,
    /// Sources keyed by stable local identifiers.
    pub sources: BTreeMap<String, Source>,
}

/// Portable catalog; a failed import does not modify any existing catalog.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    /// Must equal [`CATALOG_SCHEMA`].
    pub schema: String,
    /// Deterministic records; duplicate identifiers are refused.
    pub materials: Vec<Material>,
}

/// Properties required by the current orthotropic shell, in canonical units.
pub const MECHANICAL_PROPERTIES: [(&str, &str); 9] = [
    ("areal_density", "kg/m2"),
    ("warp_stiffness", "N/m"),
    ("weft_stiffness", "N/m"),
    ("poisson_warp_weft", "1"),
    ("shear_stiffness", "N/m"),
    ("bending_rigidity", "N*m"),
    ("thickness", "m"),
    ("friction_skin", "1"),
    ("friction_self", "1"),
];

/// The validated physical inputs shared by scene construction, contact and reporting.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mechanics {
    /// Mass per undeformed sheet area (kg/m²), not constituent solid density.
    pub areal_kg_m2: f64,
    /// Warp/wale membrane modulus (N/m), already integrated through thickness.
    pub warp_n_m: f64,
    /// Weft/course membrane modulus (N/m).
    pub weft_n_m: f64,
    /// Major Poisson ratio; reciprocal ratio is nu12 * E2 / E1.
    pub poisson: f64,
    /// Engineering shear modulus (N/m).
    pub shear_n_m: f64,
    /// Measured effective bending rigidity (N·m); not inferred using solid-plate t³.
    pub bending_nm: f64,
    /// Full sheet thickness (m); body contact uses half, self-contact the full thickness.
    pub thickness_m: f64,
    /// Cloth against skin coefficient at the cited conditions.
    pub friction_skin: f64,
    /// Cloth against itself coefficient at the cited conditions.
    pub friction_self: f64,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

impl Catalog {
    /// Parses and validates a complete catalog without inventing missing fields.
    ///
    /// # Errors
    /// Invalid JSON, schema, source references, values, compositions or duplicate IDs.
    pub fn from_json(text: &str) -> Result<Self, Refusal> {
        let catalog: Self =
            serde_json::from_str(text).map_err(|e| invalid(format!("textile catalog: {e}")))?;
        catalog.validate()?;
        Ok(catalog)
    }

    /// Validates all records, including records not selected for simulation.
    ///
    /// # Errors
    /// Unsupported schema, duplicate IDs, or any invalid material record.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != CATALOG_SCHEMA {
            return Err(invalid(format!(
                "unsupported textile schema {}",
                self.schema
            )));
        }
        let mut ids = BTreeSet::new();
        for material in &self.materials {
            material.validate()?;
            if !ids.insert(&material.id) {
                return Err(invalid(format!("duplicate textile ID {}", material.id)));
            }
        }
        Ok(())
    }

    /// Finds one exact identifier; no fuzzy matching in physical selection.
    ///
    /// # Errors
    /// Unknown identifier, never a substitution with a default fabric.
    pub fn get(&self, id: &str) -> Result<&Material, Refusal> {
        self.materials
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| invalid(format!("unknown textile material {id}")))
    }
}

impl Material {
    /// Checks provenance, finite values, uncertainty bounds and blend mass balance.
    ///
    /// # Errors
    /// Empty identifiers/sources, unresolved source keys, non-finite values or invalid fractions.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err(invalid("a textile needs an ID and name"));
        }
        for (id, source) in &self.sources {
            if id.trim().is_empty()
                || source.reference.trim().is_empty()
                || source.accessed_on.trim().is_empty()
                || source.note.trim().is_empty()
            {
                return Err(invalid(format!("{}: incomplete source {id}", self.id)));
            }
        }
        let mut fibers = BTreeSet::new();
        let mut total = 0.0;
        for c in &self.composition {
            if c.fiber.trim().is_empty()
                || !fibers.insert(&c.fiber)
                || !c.fraction.is_finite()
                || c.fraction <= 0.0
                || c.fraction > 1.0
                || !self.sources.contains_key(&c.source)
            {
                return Err(invalid(format!("{}: invalid composition", self.id)));
            }
            total += c.fraction;
        }
        if total > 1.0 + 1e-9 || (self.composition_complete && (total - 1.0).abs() > 1e-9) {
            return Err(invalid(format!(
                "{}: composition mass balance is {total}",
                self.id
            )));
        }
        for (name, q) in &self.properties {
            if name.trim().is_empty()
                || !q.value.is_finite()
                || q.unit.trim().is_empty()
                || q.method.trim().is_empty()
                || !self.sources.contains_key(&q.source)
            {
                return Err(invalid(format!("{}: invalid or unsourced {name}", self.id)));
            }
            if let Some([lo, hi]) = q.bounds
                && (!lo.is_finite() || !hi.is_finite() || lo > q.value || hi < q.value)
            {
                return Err(invalid(format!("{}: invalid bounds for {name}", self.id)));
            }
        }
        if let Some(response) = &self.membrane_response {
            for key in [
                "warp_stiffness",
                "weft_stiffness",
                "poisson_warp_weft",
                "shear_stiffness",
            ] {
                if self.properties.contains_key(key) {
                    return Err(invalid(format!(
                        "{}: {key} conflicts with membrane_response",
                        self.id
                    )));
                }
            }
            // Structural validation is independent of whether a downstream simulation consents to
            // estimated/interpolated response data. The consumer applies that evidence policy again.
            response.compile(&self.sources, true)?;
        }
        Ok(())
    }

    /// Compile the optional nonlinear law under the caller's evidence policy.
    ///
    /// # Errors
    /// Invalid sources/curves, unapproved interpolation/estimates or unstable response data.
    pub fn compiled_membrane(
        &self,
        allow_estimates: bool,
    ) -> Result<Option<crate::material_response::CompiledMembrane>, Refusal> {
        self.validate()?;
        self.membrane_response
            .as_ref()
            .map(|r| r.compile(&self.sources, allow_estimates))
            .transpose()
    }

    /// Reads one property in a specific unit, enforcing the evidence policy.
    ///
    /// # Errors
    /// Missing, unsourced, non-finite, wrong-unit or unapproved estimated property.
    pub fn require(&self, name: &str, unit: &str, allow_estimates: bool) -> Result<f64, Refusal> {
        let q = self.properties.get(name).ok_or_else(|| {
            invalid(format!(
                "{}: missing textile property {name} ({unit})",
                self.id
            ))
            .repair("supply a sourced specimen measurement; do not substitute a fiber modulus")
        })?;
        if q.unit != unit || !q.value.is_finite() || !self.sources.contains_key(&q.source) {
            return Err(invalid(format!(
                "{}: {name} must be sourced and finite in {unit}",
                self.id
            )));
        }
        if q.evidence == Evidence::Estimate && !allow_estimates {
            return Err(invalid(format!(
                "{}: {name} is an estimate; explicit opt-in required",
                self.id
            )));
        }
        Ok(q.value)
    }

    /// Lists mechanical coverage gaps in a stable order without manufacturing fake defaults.
    #[must_use]
    pub fn missing_mechanics(&self) -> Vec<&'static str> {
        MECHANICAL_PROPERTIES
            .iter()
            .filter(|(name, _)| {
                let response_supplies = self.membrane_response.is_some()
                    && matches!(
                        *name,
                        "warp_stiffness"
                            | "weft_stiffness"
                            | "poisson_warp_weft"
                            | "shear_stiffness"
                    );
                !response_supplies && !self.properties.contains_key(*name)
            })
            .map(|(name, _)| *name)
            .collect()
    }

    /// Resolves a sheet's actual constitutive law and interface properties.
    ///
    /// # Errors
    /// Unsupported material level, incomplete data, unapproved estimates, nonpositive moduli or
    /// thickness, negative bending/friction, or a non-positive-definite orthotropic law.
    pub fn mechanics(&self, allow_estimates: bool) -> Result<Mechanics, Refusal> {
        self.validate()?;
        if matches!(self.form, Form::Fiber | Form::Yarn | Form::Fill) {
            return Err(invalid(format!(
                "{}: {:?} is not a shell constitutive law",
                self.id, self.form
            )));
        }
        let mut v = [0.0; 9];
        v[0] = self.require("areal_density", "kg/m2", allow_estimates)?;
        if let Some(law) = self.compiled_membrane(allow_estimates)? {
            let [q11, q22, q12, g12] = law.origin_stiffness();
            let nu12 = q12 / q22;
            let d = 1.0 - nu12 * (q12 / q11);
            v[1] = q11 * d;
            v[2] = q22 * d;
            v[3] = nu12;
            v[4] = g12;
            if [v[1], v[2], v[3], v[4], d].iter().any(|x| !x.is_finite())
                || v[1] <= 0.0
                || v[2] <= 0.0
                || v[4] <= 0.0
                || d <= 0.0
            {
                return Err(invalid(format!(
                    "{}: nonlinear origin tangent cannot form a stable orthotropic reference law",
                    self.id
                )));
            }
        } else {
            for i in 1..=4 {
                let (name, unit) = MECHANICAL_PROPERTIES[i];
                v[i] = self.require(name, unit, allow_estimates)?;
            }
        }
        for i in 5..9 {
            let (name, unit) = MECHANICAL_PROPERTIES[i];
            v[i] = self.require(name, unit, allow_estimates)?;
        }
        if [0, 1, 2, 4, 6].iter().any(|&i| v[i] <= 0.0) || [5, 7, 8].iter().any(|&i| v[i] < 0.0) {
            return Err(invalid(format!(
                "{}: nonphysical mechanical property",
                self.id
            )));
        }
        let coupling = v[3] * (v[3] * v[2] / v[1]);
        let denominator = 1.0 - coupling;
        let q = [
            v[1] / denominator,
            v[2] / denominator,
            v[3] * v[2] / denominator,
        ];
        if !coupling.is_finite() || denominator <= 0.0 || q.iter().any(|x| !x.is_finite()) {
            return Err(invalid(format!(
                "{}: unstable orthotropic stiffness",
                self.id
            )));
        }
        Ok(Mechanics {
            areal_kg_m2: v[0],
            warp_n_m: v[1],
            weft_n_m: v[2],
            poisson: v[3],
            shear_n_m: v[4],
            bending_nm: v[5],
            thickness_m: v[6],
            friction_skin: v[7],
            friction_self: v[8],
        })
    }

    /// Dry through-thickness thermal resistance (m²·K/W) for the stated test condition.
    /// Prefers a measured/published resistance to thickness/conductivity. Neither branch infers
    /// conductivity from composition, fill power, marketing warmth labels or areal mass.
    ///
    /// # Errors
    /// Incomplete, unapproved, nonpositive or non-finite thermal data.
    pub fn thermal_resistance(&self, allow_estimates: bool) -> Result<f64, Refusal> {
        self.validate()?;
        let r = if self.properties.contains_key("thermal_resistance") {
            self.require("thermal_resistance", "m2*K/W", allow_estimates)?
        } else {
            let t = self.require("thickness", "m", allow_estimates)?;
            let k = self.require("thermal_conductivity", "W/(m*K)", allow_estimates)?;
            if t <= 0.0 || k <= 0.0 {
                return Err(invalid(format!(
                    "{}: thickness and conductivity must be positive",
                    self.id
                )));
            }
            t / k
        };
        if !r.is_finite() || r <= 0.0 {
            return Err(invalid(format!(
                "{}: thermal resistance must be finite and positive",
                self.id
            )));
        }
        Ok(r)
    }
}

/// A dry, one-dimensional steady-state layer-stack comparison, not a comfort/survival rating.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThermalStack {
    /// Sum of layer resistances, excluding boundary air films (m²·K/W).
    pub resistance_m2_k_w: f64,
    /// Signed steady dry heat flow through the specified area (W).
    pub heat_flow_w: f64,
}

impl ThermalStack {
    /// Adds layers in series with an explicitly supplied combined boundary-film resistance.
    /// Positive heat flow means the inner surface loses heat to the outer environment. No implicit
    /// air gaps, wind, evaporation, compression or body thermoregulation are modeled.
    ///
    /// # Errors
    /// Empty stack, invalid boundary conditions or incomplete/invalid material thermal data.
    pub fn evaluate(
        layers: &[&Material],
        area_m2: f64,
        inside_c: f64,
        outside_c: f64,
        boundary_resistance_m2_k_w: f64,
        allow_estimates: bool,
    ) -> Result<Self, Refusal> {
        if layers.is_empty()
            || !area_m2.is_finite()
            || area_m2 <= 0.0
            || !inside_c.is_finite()
            || !outside_c.is_finite()
            || inside_c < -273.15
            || outside_c < -273.15
            || !boundary_resistance_m2_k_w.is_finite()
            || boundary_resistance_m2_k_w < 0.0
        {
            return Err(invalid("invalid thermal stack or boundary conditions"));
        }
        let mut r = 0.0;
        for layer in layers {
            r += layer.thermal_resistance(allow_estimates)?;
        }
        let total = r + boundary_resistance_m2_k_w;
        let heat_flow_w = area_m2 * ((inside_c - outside_c) / total);
        if !r.is_finite() || !total.is_finite() || !heat_flow_w.is_finite() {
            return Err(invalid("thermal stack arithmetic overflow"));
        }
        Ok(Self {
            resistance_m2_k_w: r,
            heat_flow_w,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specimen() -> Material {
        let sources = BTreeMap::from([(
            "test".into(),
            Source {
                reference: "test:synthetic-known-answer".into(),
                accessed_on: "2026-10-13".into(),
                note: "Synthetic test inputs, not measurements of a commercial fabric".into(),
            },
        )]);
        let values = [0.2, 8000.0, 2000.0, 0.2, 1000.0, 3e-5, 0.001, 0.3, 0.5];
        let properties = MECHANICAL_PROPERTIES
            .iter()
            .zip(values)
            .map(|((name, unit), value)| {
                (
                    (*name).into(),
                    Quantity {
                        value,
                        unit: (*unit).into(),
                        evidence: Evidence::Estimate,
                        source: "test".into(),
                        method: "synthetic known answer".into(),
                        conditions: BTreeMap::new(),
                        bounds: None,
                    },
                )
            })
            .collect();
        Material {
            id: "synthetic".into(),
            name: "Synthetic test sheet".into(),
            form: Form::Woven,
            composition: Vec::new(),
            composition_complete: false,
            structure: BTreeMap::new(),
            properties,
            membrane_response: None,
            sources,
        }
    }

    #[test]
    fn anisotropy_and_estimate_opt_in() {
        let m = specimen();
        assert!(m.mechanics(false).is_err());
        let p = m.mechanics(true).unwrap();
        assert_eq!(p.warp_n_m, 8000.0);
        assert_eq!(p.weft_n_m, 2000.0);
        assert_eq!(p.friction_self, 0.5);
    }

    #[test]
    fn missing_and_wrong_units_never_default() {
        let mut m = specimen();
        m.properties.remove("bending_rigidity");
        assert_eq!(m.missing_mechanics(), vec!["bending_rigidity"]);
        assert!(m.mechanics(true).is_err());
        let mut m = specimen();
        m.properties.get_mut("warp_stiffness").unwrap().unit = "Pa".into();
        assert!(m.mechanics(true).is_err());
    }

    #[test]
    fn fiber_yarn_and_fill_cannot_impersonate_sheets() {
        for form in [Form::Fiber, Form::Yarn, Form::Fill] {
            let mut m = specimen();
            m.form = form;
            assert!(m.mechanics(true).is_err());
        }
    }

    #[test]
    fn refuses_nonfinite_and_unstable_laws() {
        for value in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            let mut m = specimen();
            m.properties.get_mut("warp_stiffness").unwrap().value = value;
            assert!(m.mechanics(true).is_err());
        }
        let mut m = specimen();
        m.properties.get_mut("poisson_warp_weft").unwrap().value = 2.0;
        assert!(m.mechanics(true).is_err());
    }

    #[test]
    fn zero_friction_and_bending_are_explicitly_supported() {
        let mut m = specimen();
        for name in ["bending_rigidity", "friction_skin", "friction_self"] {
            m.properties.get_mut(name).unwrap().value = 0.0;
        }
        assert!(m.mechanics(true).is_ok());
    }

    #[test]
    fn incomplete_and_overfull_blends_are_distinct() {
        let mut m = specimen();
        m.composition = vec![Component {
            fiber: "cotton".into(),
            fraction: 0.8,
            source: "test".into(),
        }];
        assert!(m.validate().is_ok());
        m.composition_complete = true;
        assert!(m.validate().is_err());
        m.composition[0].fraction = 1.0;
        assert!(m.validate().is_ok());
        m.composition.push(Component {
            fiber: "polyester".into(),
            fraction: 0.2,
            source: "test".into(),
        });
        assert!(m.validate().is_err());
    }

    #[test]
    fn source_references_and_bounds_are_checked() {
        let mut m = specimen();
        m.properties.get_mut("thickness").unwrap().bounds = Some([0.002, 0.003]);
        assert!(m.validate().is_err());
        let mut m = specimen();
        m.sources.clear();
        assert!(m.mechanics(true).is_err());
    }

    #[test]
    fn catalog_roundtrip_and_duplicate_rejection() {
        let mut c = Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: vec![specimen()],
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(Catalog::from_json(&json).unwrap().get("synthetic").is_ok());
        assert!(c.get("typo").is_err());
        c.materials.push(specimen());
        assert!(c.validate().is_err());
    }

    #[test]
    fn thermal_series_known_answer_and_monotonicity() {
        let mut m = specimen();
        let mut q = m.properties["thickness"].clone();
        q.value = 0.1;
        q.unit = "m2*K/W".into();
        m.properties.insert("thermal_resistance".into(), q);
        let one = ThermalStack::evaluate(&[&m], 2.0, 30.0, 0.0, 0.1, true).unwrap();
        let two = ThermalStack::evaluate(&[&m, &m], 2.0, 30.0, 0.0, 0.1, true).unwrap();
        assert!((one.heat_flow_w - 300.0).abs() < 1e-10);
        assert!((two.heat_flow_w - 200.0).abs() < 1e-10);
        assert!(two.heat_flow_w < one.heat_flow_w);
        assert!(ThermalStack::evaluate(&[&m], 2.0, 30.0, 0.0, 0.1, false).is_err());
    }

    #[test]
    fn nonlinear_response_replaces_scalar_membrane_inputs_without_inventing_other_properties() {
        let m: Material = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-nonlinear.json"
        ))
        .unwrap();
        assert!(m.validate().is_ok());
        assert!(m.mechanics(false).is_err());
        let mechanics = m.mechanics(true).unwrap();
        assert!((mechanics.warp_n_m - 199.0).abs() < 1e-12);
        assert!((mechanics.weft_n_m - 99.5).abs() < 1e-12);
        assert!((mechanics.poisson - 0.1).abs() < 1e-12);
        assert!((mechanics.shear_n_m - 40.0).abs() < 1e-12);
        assert!(m.missing_mechanics().is_empty());
        assert!(m.compiled_membrane(true).unwrap().is_some());

        let mut conflicting = m;
        conflicting.properties.insert(
            "warp_stiffness".into(),
            Quantity {
                value: 999.0,
                unit: "N/m".into(),
                evidence: Evidence::Estimate,
                source: "test".into(),
                method: "Conflicting synthetic scalar".into(),
                conditions: BTreeMap::new(),
                bounds: None,
            },
        );
        assert!(conflicting.validate().is_err());
    }
}
