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
