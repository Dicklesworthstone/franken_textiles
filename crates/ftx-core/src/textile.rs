//! Source-backed textile records shared by simulation, manufacturing and purchasing.
//!
//! A fibre name is not a constitutive law. Records describe a particular construction and finish;
//! missing observations stay missing. Solver bundles are resolved atomically, with explicit units,
//! rather than filling unknown mechanical properties from the chemistry or a product name.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Refusal, RefusalCode};

/// Version of a portable, offline material collection.
pub const CATALOG_SCHEMA: &str = "ftx-textiles/1";
/// The checked-in observations, not a live network request.
pub const BUILTIN_JSON: &str = include_str!("../data/textiles.json");

/// Physical form; yarn/fill data must never silently become sheet mechanics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// Unspun fibre.
    Fiber,
    /// Spun or filament yarn/thread.
    Yarn,
    /// Interlaced warp/weft sheet.
    Woven,
    /// Loop-formed sheet, including stretch jerseys.
    Knit,
    /// Felt, bonded or other nonwoven sheet.
    Nonwoven,
    /// A processed hide; not a weave.
    Leather,
    /// Loose or continuous-filament insulation.
    Fill,
    /// A bonded multilayer sheet.
    Laminate,
}

/// Evidence is carried into the result, not promoted by successfully running a solver.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    /// Supplier or manufacturer specification; not independent validation.
    Manufacturer,
    /// Measurement with a cited test method/conditions.
    Measured,
    /// An explicitly documented approximation.
    Estimated,
    /// A user declaration or synthetic test input.
    Assumed,
}

/// Origin of one or more observations; a URL is a locator, not a redistribution licence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Public URL or local evidence locator.
    pub locator: String,
    /// Human-readable document/swatch identity.
    pub title: String,
    /// ISO date when the source was inspected, not its publication date.
    pub accessed: String,
}

/// A scalar observation in its original units. Absence, not zero, represents unknown data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Nominal reported value.
    pub value: f64,
    /// Unit, including the denominator (for example `g/m2`, never just `oz`).
    pub unit: String,
    /// Key into the owning record's sources.
    pub source: String,
    /// Nature of the evidence.
    pub evidence: Evidence,
    /// Test method, direction, humidity, preload, lot or other applicability limits.
    pub conditions: String,
    /// Optional reported interval; not automatically a confidence interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[f64; 2]>,
}

/// A specific fibre/yarn/fabric/fill, including construction and surface treatment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Textile {
    /// Stable product/construction identifier, not just a fibre name.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Physical form.
    pub form: Form,
    /// Fibre mass fractions. Empty means composition unknown, not a pure fibre.
    pub composition: BTreeMap<String, f64>,
    /// Source of identity, construction and composition.
    pub identity_source: String,
    /// Weave/knit, yarn, finish, coating and other qualitative descriptors.
    pub construction: BTreeMap<String, String>,
    /// Extensible named observations; solver consumers require recognized units.
    pub properties: BTreeMap<String, Observation>,
    /// Evidence locators, including local characterization files when appropriate.
    pub sources: BTreeMap<String, Source>,
    /// Explicit limitations and unsupported properties.
    #[serde(default)]
    pub notes: Vec<String>,
}

/// A portable collection. Its index is derived; records remain the authority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    /// Must be [`CATALOG_SCHEMA`].
    pub schema: String,
    /// Records in stable identifier order when exported.
    pub materials: Vec<Textile>,
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn positive(name: &str, value: f64) -> Result<f64, Refusal> {
    Refusal::finite(name, value)?;
    if value <= 0.0 {
        return Err(bad(format!("{name} must be positive")));
    }
    Ok(value)
}

impl Catalog {
    /// Loads and validates the offline built-in collection.
    ///
    /// # Errors
    /// Malformed records, unsupported schema or duplicate identities.
    pub fn builtin() -> Result<Self, Refusal> {
        Self::from_json(BUILTIN_JSON)
    }

    /// Loads a collection without mutating it or fetching any referenced URL.
    ///
    /// # Errors
    /// Invalid JSON, records or duplicate identifiers.
    pub fn from_json(json: &str) -> Result<Self, Refusal> {
        let mut catalog: Self = serde_json::from_str(json).map_err(|e| bad(e.to_string()))?;
        if catalog.schema != CATALOG_SCHEMA {
            return Err(bad("unsupported textile catalog schema"));
        }
        let mut ids = BTreeSet::new();
        for record in &catalog.materials {
            record.validate()?;
            if !ids.insert(record.id.as_str()) {
                return Err(bad(format!("duplicate textile id {}", record.id)));
            }
        }
        catalog.materials.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(catalog)
    }

    /// Exact identity lookup: no fuzzy match can change a simulation's material.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Textile> {
        self.materials.iter().find(|m| m.id == id)
    }

    /// Case-insensitive discovery over identity, composition and construction, in catalog order.
    #[must_use]
    pub fn search(&self, text: &str) -> Vec<&Textile> {
        let needle = text.to_lowercase();
        self.materials
            .iter()
            .filter(|m| {
                [&m.id, &m.name]
                    .into_iter()
                    .chain(m.composition.keys())
                    .chain(m.construction.values())
                    .any(|s| s.to_lowercase().contains(&needle))
            })
            .collect()
    }
}

impl Textile {
    /// Validates evidence links, composition, numerical observations and reported intervals.
    ///
    /// # Errors
    /// Missing identities/sources, invalid fractions, non-finite values or inconsistent bounds.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err(bad("textile id and name must not be empty"));
        }
        if !self.sources.contains_key(&self.identity_source) {
            return Err(bad(format!("{}: missing identity source", self.id)));
        }
        for (id, s) in &self.sources {
            if id.is_empty()
                || s.locator.trim().is_empty()
                || s.title.trim().is_empty()
                || s.accessed.trim().is_empty()
            {
                return Err(bad(format!("{}: incomplete source {id}", self.id)));
            }
        }
        let mut sum = 0.0;
        for (fiber, fraction) in &self.composition {
            if fiber.trim().is_empty() || !fraction.is_finite() || !(0.0..=1.0).contains(fraction) {
                return Err(bad(format!("{}: invalid fibre fraction", self.id)));
            }
            sum += fraction;
        }
        if !self.composition.is_empty() && (sum - 1.0).abs() > 1e-8 {
            return Err(bad(format!("{}: fibre fractions must sum to one", self.id)));
        }
        for (key, p) in &self.properties {
            Refusal::finite(key, p.value)?;
            if key.trim().is_empty()
                || p.unit.trim().is_empty()
                || !self.sources.contains_key(&p.source)
            {
                return Err(bad(format!("{}: {key} has no unit or source", self.id)));
            }
            if let Some([lo, hi]) = p.range
                && (!lo.is_finite() || !hi.is_finite() || lo > p.value || p.value > hi)
            {
                return Err(bad(format!("{}: {key} has invalid bounds", self.id)));
            }
        }
        Ok(())
    }

    /// Resolves an optional scalar into the requested SI unit. Unknown remains `None`.
    ///
    /// # Errors
    /// A present value has incompatible units, a missing evidence source or a non-finite conversion.
    pub fn scalar(&self, key: &str, unit: &str) -> Result<Option<f64>, Refusal> {
        let Some(p) = self.properties.get(key) else {
            return Ok(None);
        };
        if !self.sources.contains_key(&p.source) {
            return Err(bad(format!("{}: {key} has no evidence source", self.id)));
        }
        let scale = match (p.unit.as_str(), unit) {
            (a, b) if a == b => 1.0,
            ("g/m2", "kg/m2") => 0.001,
            ("oz/yd2", "kg/m2") => 0.028_349_523_125 / (0.9144 * 0.9144),
            ("lb/yd2", "kg/m2") => 0.453_592_37 / (0.9144 * 0.9144),
            ("mm", "m") => 0.001,
            ("cm", "m") => 0.01,
            ("in", "m") => 0.0254,
            ("clo", "m2*K/W") => 0.155,
            ("tex", "kg/m") => 1e-6,
            ("denier", "kg/m") => 1e-3 / 9000.0,
            _ => {
                return Err(bad(format!(
                    "{}: {key} needs {unit}, got {}",
                    self.id, p.unit
                )));
            }
        };
        let value = p.value * scale;
        Refusal::finite(key, value)?;
        Ok(Some(value))
    }

    /// Resolves one required property; missing evidence is an actionable refusal, not a default.
    ///
    /// # Errors
    /// Missing or incompatible observations.
    pub fn require(&self, key: &str, unit: &str) -> Result<f64, Refusal> {
        self.scalar(key, unit)?.ok_or_else(|| {
            bad(format!("{}: missing {key} ({unit})", self.id))
                .repair("characterize this fabric/finish and attach a sourced observation")
        })
    }

    /// Resolves all inputs of the currently supported orthotropic sheet model together.
    ///
    /// # Errors
    /// Fibre, yarn or fill used as a sheet; missing/invalid mechanical inputs; unstable elasticity.
    pub fn mechanics(&self) -> Result<Mechanics, Refusal> {
        self.validate()?;
        if matches!(self.form, Form::Fiber | Form::Yarn | Form::Fill) {
            return Err(bad(
                "fibre/yarn/fill requires a construction model before sheet simulation",
            ));
        }
        let m = Mechanics {
            areal_kg_m2: self.require("areal_mass", "kg/m2")?,
            thickness_m: self.require("thickness", "m")?,
            warp_n_m: self.require("warp_stiffness", "N/m")?,
            weft_n_m: self.require("weft_stiffness", "N/m")?,
            shear_n_m: self.require("shear_stiffness", "N/m")?,
            poisson: self.require("poisson_warp_weft", "1")?,
            bending_nm: self.require("bending_rigidity", "N*m")?,
            skin_mu: self.require("friction_skin", "1")?,
            self_mu: self.require("friction_self", "1")?,
        };
        m.validate()?;
        Ok(m)
    }

    /// Dry through-thickness material resistance, not a garment comfort or temperature rating.
    /// A directly measured resistance takes precedence over the slab approximation `thickness/k`.
    ///
    /// # Errors
    /// Invalid present data. Missing conductivity or thickness produces `None`.
    pub fn thermal_resistance(&self) -> Result<Option<f64>, Refusal> {
        if let Some(r) = self.scalar("thermal_resistance", "m2*K/W")? {
            return positive("thermal resistance", r).map(Some);
        }
        match (
            self.scalar("thickness", "m")?,
            self.scalar("thermal_conductivity", "W/(m*K)")?,
        ) {
            (Some(t), Some(k)) => {
                let r = positive("thickness", t)? / positive("thermal conductivity", k)?;
                positive("thermal resistance", r).map(Some)
            }
            _ => Ok(None),
        }
    }
}

/// Validated inputs of the orthotropic membrane / scalar-bending sheet approximation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mechanics {
    /// Sheet mass per reference area (kg/m²), not fibre solid density.
    pub areal_kg_m2: f64,
    /// Total physical thickness (m); contact with a body uses half of it.
    pub thickness_m: f64,
    /// Warp membrane stiffness (N/m).
    pub warp_n_m: f64,
    /// Weft membrane stiffness (N/m).
    pub weft_n_m: f64,
    /// Engineering shear stiffness (N/m).
    pub shear_n_m: f64,
    /// Major Poisson ratio ν12; ν21 is derived by reciprocity.
    pub poisson: f64,
    /// Scalar flexural rigidity per unit width (N·m).
    pub bending_nm: f64,
    /// Cloth–skin friction under the observation's stated conditions.
    pub skin_mu: f64,
    /// Cloth–same-cloth friction under the observation's stated conditions.
    pub self_mu: f64,
}

impl Mechanics {
    /// Rejects nonphysical or numerically unrepresentable stiffnesses before assembling a solver.
    ///
    /// # Errors
    /// Invalid moduli, density, thickness, friction or non-positive-definite elastic coupling.
    pub fn validate(&self) -> Result<(), Refusal> {
        for (key, v) in [
            ("areal mass", self.areal_kg_m2),
            ("thickness", self.thickness_m),
            ("warp stiffness", self.warp_n_m),
            ("weft stiffness", self.weft_n_m),
            ("shear stiffness", self.shear_n_m),
            ("bending rigidity", self.bending_nm),
        ] {
            positive(key, v)?;
        }
        for (key, v) in [
            ("skin friction", self.skin_mu),
            ("self friction", self.self_mu),
        ] {
            Refusal::finite(key, v)?;
            if v < 0.0 {
                return Err(bad(format!("{key} must be nonnegative")));
            }
        }
        Refusal::finite("Poisson ratio", self.poisson)?;
        let denominator = 1.0 - self.poisson * (self.poisson * self.weft_n_m / self.warp_n_m);
        positive("orthotropic stability denominator", denominator)?;
        for v in [
            self.warp_n_m / denominator,
            self.weft_n_m / denominator,
            self.poisson * self.weft_n_m / denominator,
        ] {
            Refusal::finite("orthotropic stiffness", v)?;
        }
        Ok(())
    }
}

/// Selects `fabric.material_id` from the built-ins, or a self-contained `fabric.material` record.
/// Absence means the caller may use its explicitly labelled legacy contract. Mixing selectors or
/// legacy mechanical values with a material record is refused, so there is one authority per number.
///
/// # Errors
/// Unknown id, conflicting selectors, malformed record or mixed legacy/material inputs.
pub fn from_fabric(fabric: &Value) -> Result<Option<Textile>, Refusal> {
    let id = fabric.get("material_id");
    let inline = fabric.get("material");
    if id.is_none() && inline.is_none() {
        return Ok(None);
    }
    if id.is_some() && inline.is_some() {
        return Err(bad("choose material_id OR material, not both"));
    }
    for key in ["areal_g_m2", "tensile_N_m", "bending_Nm"] {
        if fabric.get(key).is_some() {
            return Err(bad(format!(
                "{key} conflicts with the material record; characterize the record instead"
            )));
        }
    }
    let record = if let Some(id) = id {
        let id = id
            .as_str()
            .ok_or_else(|| bad("material_id must be a string"))?;
        Catalog::builtin()?
            .get(id)
            .cloned()
            .ok_or_else(|| bad(format!("unknown textile {id}")))?
    } else {
        serde_json::from_value(inline.expect("one selector exists").clone())
            .map_err(|e| bad(format!("invalid inline material: {e}")))?
    };
    record.validate()?;
    Ok(Some(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn specimen() -> Textile {
        let mut m = Catalog::builtin().unwrap().materials[0].clone();
        m.id = "synthetic_characterization".into();
        m.form = Form::Woven;
        for (key, value, unit) in [
            ("areal_mass", 200.0, "g/m2"),
            ("thickness", 0.8, "mm"),
            ("warp_stiffness", 4000.0, "N/m"),
            ("weft_stiffness", 1000.0, "N/m"),
            ("shear_stiffness", 200.0, "N/m"),
            ("poisson_warp_weft", 0.2, "1"),
            ("bending_rigidity", 3e-5, "N*m"),
            ("friction_skin", 0.3, "1"),
            ("friction_self", 0.5, "1"),
            ("thermal_conductivity", 0.04, "W/(m*K)"),
        ] {
            m.properties.insert(
                key.into(),
                Observation {
                    value,
                    unit: unit.into(),
                    source: "synthetic".into(),
                    evidence: Evidence::Assumed,
                    conditions: "Synthetic arithmetic test, not a fabric measurement".into(),
                    range: None,
                },
            );
        }
        m.sources.insert(
            "synthetic".into(),
            Source {
                locator: "test:synthetic".into(),
                title: "Synthetic characterization".into(),
                accessed: "2026-10-13".into(),
            },
        );
        m
    }

    #[test]
    fn builtin_is_sorted_source_backed_and_roundtrips() {
        let c = Catalog::builtin().unwrap();
        assert_eq!(c.materials.len(), 11);
        assert!(c.materials.windows(2).all(|w| w[0].id < w[1].id));
        assert_eq!(c.search("BAMBOO").len(), 1);
        assert_eq!(
            c,
            Catalog::from_json(&serde_json::to_string(&c).unwrap()).unwrap()
        );
        // Supplier identities/specs are not silently promoted into complete constitutive laws.
        assert!(c.materials.iter().all(|m| m.mechanics().is_err()));
        assert_eq!(
            c.get("vuori_dreamknit_ponto_185")
                .unwrap()
                .require("areal_mass", "kg/m2")
                .unwrap(),
            0.185
        );
    }

    #[test]
    fn bundle_and_thermal_resolution_preserve_units() {
        let m = specimen();
        let p = m.mechanics().unwrap();
        assert_eq!(p.areal_kg_m2, 0.2);
        assert_eq!(p.thickness_m, 0.0008);
        assert_eq!(p.warp_n_m, 4000.0);
        assert!((m.thermal_resistance().unwrap().unwrap() - 0.02).abs() < 1e-15);
        let mut direct = m.clone();
        let mut obs = direct.properties["thickness"].clone();
        obs.value = 2.0;
        obs.unit = "clo".into();
        direct.properties.insert("thermal_resistance".into(), obs);
        assert_eq!(direct.thermal_resistance().unwrap(), Some(0.31));
    }

    #[test]
    fn missing_wrong_units_and_evidence_refuse() {
        let m = specimen();
        for key in [
            "areal_mass",
            "thickness",
            "warp_stiffness",
            "weft_stiffness",
            "shear_stiffness",
            "poisson_warp_weft",
            "bending_rigidity",
            "friction_skin",
            "friction_self",
        ] {
            let mut missing = m.clone();
            missing.properties.remove(key);
            assert!(missing.mechanics().is_err(), "missing {key}");
            let mut wrong = m.clone();
            wrong.properties.get_mut(key).unwrap().unit = "unknown".into();
            assert!(wrong.mechanics().is_err(), "wrong unit {key}");
        }
        let mut broken = m.clone();
        broken.sources.remove("synthetic");
        assert!(broken.mechanics().is_err());
        let mut broken = m;
        broken.properties.get_mut("thickness").unwrap().range = Some([1.0, 2.0]);
        assert!(broken.validate().is_err());
    }

    #[test]
    fn invalid_physics_and_construction_never_fall_back() {
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut m = specimen();
            m.properties.get_mut("warp_stiffness").unwrap().value = value;
            assert!(m.mechanics().is_err());
        }
        let mut m = specimen();
        m.properties.get_mut("poisson_warp_weft").unwrap().value = 3.0;
        assert!(m.mechanics().is_err());
        for form in [Form::Fiber, Form::Yarn, Form::Fill] {
            let mut m = specimen();
            m.form = form;
            assert!(m.mechanics().is_err());
        }
    }

    #[test]
    fn selectors_are_exact_and_unambiguous() {
        assert!(from_fabric(&json!({"material_id":"typo"})).is_err());
        assert!(from_fabric(&json!({"material_id":null})).is_err());
        assert!(
            from_fabric(&json!({"material_id":"fs_il019_natural", "tensile_N_m":8000})).is_err()
        );
        assert!(from_fabric(&json!({"material_id":"fs_il019_natural", "material":{}})).is_err());
        assert!(from_fabric(&json!({})).unwrap().is_none());
        let m = specimen();
        assert_eq!(from_fabric(&json!({"material":m})).unwrap(), Some(m));
    }

    #[test]
    fn mass_units_never_confuse_momme_or_leather_ounces() {
        let c = Catalog::builtin().unwrap();
        let linen = c.get("fs_il019_natural").unwrap();
        assert!((linen.require("areal_mass", "kg/m2").unwrap() - 0.179_700_461_6).abs() < 1e-9);
        let silk = c.get("dharma_habotai_8_momme").unwrap();
        assert!(silk.scalar("thickness", "m").unwrap().is_none());
        assert!(silk.scalar("silk_weight_grade", "m").is_err());
        assert!(
            c.get("tandy_cow_garment_9114_71")
                .unwrap()
                .scalar("areal_mass", "kg/m2")
                .unwrap()
                .is_none()
        );
        assert!(
            c.get("rbtr_apex_25")
                .unwrap()
                .thermal_resistance()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn duplicate_identity_and_incomplete_composition_refuse() {
        let mut c = Catalog::builtin().unwrap();
        c.materials.push(c.materials[0].clone());
        assert!(Catalog::from_json(&serde_json::to_string(&c).unwrap()).is_err());
        let mut m = specimen();
        m.composition = BTreeMap::from([("wool".into(), 0.95)]);
        assert!(m.validate().is_err());
    }
}
