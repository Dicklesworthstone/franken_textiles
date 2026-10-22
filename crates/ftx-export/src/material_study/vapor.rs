//! Resolve an explicit common-area vapor stack from the alternative's final material assignments.
//! Geometry, body coverage, gaps and sweating are not inferred from source-piece membership.

use std::collections::BTreeMap;

use ftx_core::Refusal;
use ftx_pattern::{Pattern, material_catalog::Selection, material_vapor, textile_import};
use serde::{Deserialize, Serialize};

use super::{budget, invalid};

/// Which final source-bearing fabric supplies one physical stack occurrence.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    /// The pattern default, regardless of any explicit source-piece assignments.
    Default,
    /// The named original source piece's effective fabric, including default inheritance.
    Piece {
        /// Exact source-piece ID, never a generated instance or cutting label.
        id: String,
    },
}

/// An ordered physical occurrence. Repeated materials retain distinct occurrence keys.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    /// Unique key within this inside-to-outside stack.
    pub key: String,
    /// Assignment to resolve after the alternative's complete atomic material binding.
    pub target: Target,
}

/// Explicit common-area network, evaluated independently for every material alternative.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// One to 128 ordered physical layer occurrences. Source pieces do not imply layer order.
    pub layers: Vec<Layer>,
    /// Positive common area in m²; neither pattern area nor draped/body area is substituted.
    pub area_m2: f64,
    /// Prescribed inside vapor partial pressure in Pa; no saturation or sweat supply is inferred.
    pub inside_vapor_pressure_pa: f64,
    /// Prescribed outside vapor partial pressure in Pa, no greater than the inside pressure.
    pub outside_vapor_pressure_pa: f64,
    /// Explicit positive inner film resistance in m²·Pa/W.
    pub inside_film_resistance_m2_pa_w: f64,
    /// Explicit positive outer film resistance in m²·Pa/W.
    pub outside_film_resistance_m2_pa_w: f64,
    /// Positive supplied latent heat in J/kg, used only to convert potential heat to mass flow.
    pub latent_heat_j_kg: f64,
    /// Exact observation-condition filters, preserved through the canonical evaluator.
    #[serde(default)]
    pub required_conditions: BTreeMap<String, String>,
    /// Optional exact observation method; no cross-method equivalence is inferred.
    #[serde(default)]
    pub required_method: Option<String>,
    /// Explicit admission of different complete methods/conditions, independent of estimate policy.
    #[serde(default)]
    pub allow_mixed_protocols: bool,
}

impl Request {
    /// Only the study calls this after the binder has frozen every effective fabric selection.
    pub(super) fn evaluate(&self, pattern: &Pattern) -> Result<material_vapor::Report, Refusal> {
        if self.layers.len() > 128 {
            return Err(budget("material-study vapor stack exceeds 128 layers"));
        }
        let pieces = pattern.raw()["pieces"]
            .as_array()
            .ok_or_else(|| invalid("vapor stack needs source-piece assignments"))?;
        let layers = self
            .layers
            .iter()
            .map(|layer| {
                let fabric = match &layer.target {
                    Target::Default => &pattern.raw()["fabric"],
                    Target::Piece { id } => {
                        let index = pieces
                            .iter()
                            .position(|piece| piece["id"].as_str() == Some(id.as_str()))
                            .ok_or_else(|| invalid(format!("unknown vapor source piece {id}")))?;
                        textile_import::piece_fabric(pattern, index)?
                    }
                };
                let selection: Selection = serde_json::from_value(fabric["material"].clone())
                    .map_err(|e| invalid(format!("vapor stack needs a frozen material: {e}")))?;
                Ok(material_vapor::Layer {
                    key: layer.key.clone(),
                    selection,
                })
            })
            .collect::<Result<_, Refusal>>()?;
        material_vapor::Request {
            schema: material_vapor::SCHEMA.into(),
            layers,
            area_m2: self.area_m2,
            inside_vapor_pressure_pa: self.inside_vapor_pressure_pa,
            outside_vapor_pressure_pa: self.outside_vapor_pressure_pa,
            inside_film_resistance_m2_pa_w: self.inside_film_resistance_m2_pa_w,
            outside_film_resistance_m2_pa_w: self.outside_film_resistance_m2_pa_w,
            latent_heat_j_kg: self.latent_heat_j_kg,
            required_conditions: self.required_conditions.clone(),
            required_method: self.required_method.clone(),
            allow_mixed_protocols: self.allow_mixed_protocols,
        }
        .evaluate(&textile_import::catalog()?)
    }
}
