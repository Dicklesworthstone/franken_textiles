//! Explicit optical face identities on an oriented source sheet.
//!
//! Positive means the retained source triangle's vertex-order normal; negative is its opposite.
//! A mirrored pattern instance already preserves that convention and needs no additional flip.
//! This declaration binds an observation to geometry. It does not turn band-limited measurements
//! into total hemispherical emissivity or certify a temperature domain or optical approximation.

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use crate::materials::{Evidence, Material};

/// A persistent side of an oriented source sheet, independent of the nearest body surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// Direction of the retained source triangle's oriented normal.
    Positive,
    /// Direction opposite the retained source triangle's oriented normal.
    Negative,
}

/// One named optical observation; the numeric value and all its qualifications remain in Material.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpticalFace {
    /// Exact physical face label in the selected observation's `face` condition.
    pub face: String,
    /// Exact property key in this material, without a generic fiber or opposite-face fallback.
    pub emissivity_property: String,
}

/// Source-bearing assignment of named material observations to canonical source-sheet sides.
///
/// Either side may remain uncharacterized. Every assigned observation must describe the declared
/// specimen and conditioning, but the orientation and optical measurements may have distinct sources.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpticalFaces {
    /// Observation on the positive source-normal side, when identified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positive: Option<OpticalFace>,
    /// Observation on the opposite side, when identified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative: Option<OpticalFace>,
    /// Existing Material source key for the orientation assignment.
    pub source: String,
    /// Evidence for the assignment, separate from evidence for either optical measurement.
    pub evidence: Evidence,
    /// How the observed faces were related to the oriented source sheet.
    pub method: String,
    /// Exact optical specimen identity, not a generic fiber or trade name.
    pub specimen: String,
    /// Exact conditioning identity shared by the assigned optical observations.
    pub conditioning: String,
}

/// Qualified total-hemispherical optical observation for opaque-gray thermal exchange.
#[derive(Clone, Copy, Debug)]
pub struct ThermalEmissivity {
    /// Nominal dimensionless value in [0, 1].
    pub value: f64,
    /// Original source interval; absence never becomes a singleton nominal observation.
    pub bounds: Option<[f64; 2]>,
    /// Qualified surface and radiant-enclosure temperature domain, degrees Celsius.
    pub temperature_range_c: [f64; 2],
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

impl OpticalFaces {
    pub(crate) fn validate(&self, material: &Material) -> Result<(), Refusal> {
        if (self.positive.is_none() && self.negative.is_none())
            || self.source.trim().is_empty()
            || !material.sources.contains_key(&self.source)
            || self.method.trim().is_empty()
            || self.specimen.trim().is_empty()
            || self.conditioning.trim().is_empty()
        {
            return Err(invalid(format!(
                "{}: optical face mapping needs a side, source, method, specimen and conditioning",
                material.id
            )));
        }
        for face in [&self.positive, &self.negative].into_iter().flatten() {
            let q = material
                .properties
                .get(&face.emissivity_property)
                .ok_or_else(|| {
                    invalid(format!(
                        "{}: optical face {} refers to missing property {}",
                        material.id, face.face, face.emissivity_property
                    ))
                })?;
            if face.face.trim().is_empty()
                || face.emissivity_property.trim().is_empty()
                || !material.sources.contains_key(&q.source)
                || q.conditions.get("face") != Some(&face.face)
                || q.conditions.get("specimen") != Some(&self.specimen)
                || q.conditions.get("conditioning") != Some(&self.conditioning)
            {
                return Err(invalid(format!(
                    "{}: optical face {} must match its sourced observation, specimen and conditioning",
                    material.id, face.face
                )));
            }
        }
        Ok(())
    }
}

impl Material {
    /// Qualify one exact optical face against this material's transport specimen and conditioning.
    /// The caller separately admits the opaque-gray closure and the convection boundary.
    ///
    /// # Errors
    /// Missing/unadmitted observation, wrong face or spectral basis, mismatched specimen,
    /// nonphysical emissivity, or an unqualified radiant-enclosure temperature.
    pub fn thermal_emissivity(
        &self,
        property: &str,
        face: &str,
        ambient_c: f64,
        allow_estimates: bool,
    ) -> Result<ThermalEmissivity, Refusal> {
        self.validate()?;
        if property.trim().is_empty() || face.trim().is_empty() {
            return Err(invalid(
                "radiation needs an explicit face and emissivity property",
            ));
        }
        let value = self.require(property, "1", allow_estimates)?;
        let q = &self.properties[property];
        if !(0.0..=1.0).contains(&value)
            || q.bounds.is_some_and(|[lo, hi]| lo < 0.0 || hi > 1.0)
            || q.conditions.get("face").map(String::as_str) != Some(face)
            || q.conditions.get("spectral_basis").map(String::as_str) != Some("total_hemispherical")
        {
            return Err(invalid(
                "emissivity needs [0,1] bounds, the exact exposed face and total_hemispherical spectral basis",
            ));
        }
        let transport = self
            .properties
            .get("thermal_resistance")
            .or_else(|| self.properties.get("thermal_conductivity"))
            .or_else(|| self.properties.get("thickness"));
        for field in ["specimen", "conditioning"] {
            let actual = q
                .conditions
                .get(field)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| invalid(format!("emissivity requires {field}")))?;
            let expected = self
                .thermal_response
                .as_ref()
                .map(|t| {
                    if field == "specimen" {
                        &t.specimen
                    } else {
                        &t.conditioning
                    }
                })
                .or_else(|| transport.and_then(|t| t.conditions.get(field)));
            if expected.is_some_and(|v| v != actual) {
                return Err(invalid(format!("emissivity must match transport {field}")));
            }
        }
        let number = |key: &str| {
            q.conditions
                .get(key)
                .and_then(|s| s.parse::<f64>().ok())
                .ok_or_else(|| invalid(format!("emissivity requires numeric {key}")))
        };
        let [lo, hi] = [number("temperature_min_c")?, number("temperature_max_c")?];
        if !lo.is_finite() || !hi.is_finite() || lo < -273.15 || hi < lo || !(hi - lo).is_finite() {
            return Err(invalid("invalid exterior surface temperature range"));
        }
        if !ambient_c.is_finite() || ambient_c < lo || ambient_c > hi {
            return Err(invalid(
                "radiant enclosure lies outside the emissivity temperature qualification",
            ));
        }
        Ok(ThermalEmissivity {
            value,
            bounds: q.bounds,
            temperature_range_c: [lo, hi],
        })
    }

    /// Resolve the explicitly mapped observation for a canonical side under the material policy.
    /// Spectral/angular, temperature and numeric qualifications remain the consumer's responsibility.
    ///
    /// # Errors
    /// Invalid provenance/observation identity, absent mapping or side, or unadmitted estimates in
    /// either the orientation declaration or the selected optical observation.
    pub fn optical_face(&self, side: Side, allow_estimates: bool) -> Result<&OpticalFace, Refusal> {
        self.validate()?;
        let mapping = self
            .optical_faces
            .as_ref()
            .ok_or_else(|| invalid(format!("{}: no source-sheet optical face mapping", self.id)))?;
        let face = match side {
            Side::Positive => &mapping.positive,
            Side::Negative => &mapping.negative,
        }
        .as_ref()
        .ok_or_else(|| {
            invalid(format!(
                "{}: no optical observation mapped to the {side:?} source-sheet side",
                self.id
            ))
        })?;
        let q = &self.properties[&face.emissivity_property];
        if !allow_estimates
            && (mapping.evidence == Evidence::Estimate || q.evidence == Evidence::Estimate)
        {
            return Err(invalid(format!(
                "{}: estimated optical face mapping or observation requires material estimate consent",
                self.id
            )));
        }
        Ok(face)
    }
}
