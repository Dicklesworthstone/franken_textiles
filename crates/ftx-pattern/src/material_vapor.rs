//! Conditional steady vapor transport through a common-area series stack.
//!
//! ISO 11092:2014 §2.2 defines Ret as vapor-pressure difference divided by evaporative heat
//! flux per area. This consumer applies that definition to a fixed-resistance, isothermal
//! network. Adding separately measured layers is an effective-model assumption, not a measured
//! assembly. Source conditions remain attached to every observation; no temperature/humidity
//! extrapolation, liquid transport, condensation or actual sweat supply is inferred.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use crate::{
    material_catalog::Selection,
    materials::{Catalog, Form, Quantity, Source},
};

pub mod wet_film;

/// Portable steady vapor-stack request.
pub const SCHEMA: &str = "ftx-vapor-stack/1";
/// Scope of the assumed network, independent of the evidence of its input observations.
pub const MODEL: &str = "constant_source_resistance_isothermal_series/1";
/// Explicit unequal conducting areas and interlayer resistances, referred to the boundary area.
pub const AREA_MODEL: &str = "constant_source_resistance_area_aware_isothermal_series/1";

/// One occurrence of a material in the ordered inside-to-outside stack.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    /// Unique occurrence key; repeated materials must still have distinct layer keys.
    pub key: String,
    /// Canonical source-bearing material selection, including its own estimate policy.
    pub selection: Selection,
    /// Optional conducting area in m². Absent uses the common boundary area; an explicit area
    /// contributes Ret × boundary area / conducting area without altering the source observation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area_m2: Option<f64>,
}

/// Explicit boundaries for a common-area, fixed-resistance potential transport calculation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must equal [`SCHEMA`].
    pub schema: String,
    /// One to 128 layers, ordered from the inside reservoir to the outside reservoir.
    pub layers: Vec<Layer>,
    /// Positive reference area shared by every layer and both boundary films, in m².
    pub area_m2: f64,
    /// Nonnegative prescribed inner vapor partial pressure, in Pa. No saturation is inferred.
    pub inside_vapor_pressure_pa: f64,
    /// Nonnegative prescribed outer vapor partial pressure, no greater than the inner pressure.
    pub outside_vapor_pressure_pa: f64,
    /// Explicit positive inner boundary-film resistance in m²·Pa/W; never inferred from wind.
    pub inside_film_resistance_m2_pa_w: f64,
    /// Explicit positive outer boundary-film resistance in m²·Pa/W; excludes the material layers.
    pub outside_film_resistance_m2_pa_w: f64,
    /// Positive caller-supplied latent heat in J/kg, used only to convert potential heat to mass.
    pub latent_heat_j_kg: f64,
    /// Exact subset required of every observation's conditions; no equivalence is inferred.
    #[serde(default)]
    pub required_conditions: BTreeMap<String, String>,
    /// Optional exact observation method required of every selected layer.
    #[serde(default)]
    pub required_method: Option<String>,
    /// Admit differences in complete reported methods/conditions. This does not establish
    /// comparability or override required filters, source locks or estimate consent.
    #[serde(default)]
    pub allow_mixed_protocols: bool,
    /// Optional inside-to-outside interface resistances per boundary area, in m²·Pa/W.
    /// When supplied, exactly one finite nonnegative value per adjacent layer pair is required.
    /// Empty retains the original no-interface model; no air-gap coefficient is inferred.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interface_resistance_m2_pa_w: Vec<f64>,
}

/// The qualified observation and potential pressure drop for one layer occurrence.
#[derive(Clone, Debug, Serialize)]
pub struct LayerReading {
    /// Request occurrence key.
    pub key: String,
    /// Full frozen selection, including authored data, source qualifications and content lock.
    pub selection: Selection,
    /// Normalized material identity; not an empirical validation certificate.
    pub material_sha256: String,
    /// Canonical m²·Pa/W observation with its original evidence, method, conditions and bounds.
    pub observation: Quantity,
    /// Source resolved from the observation's own source key.
    pub source: Source,
    /// Potential vapor pressure at this layer's inner face, in Pa.
    pub inside_pressure_pa: f64,
    /// Potential vapor pressure at this layer's outer face, in Pa.
    pub outside_pressure_pa: f64,
    /// Potential pressure drop across this layer, in Pa.
    pub pressure_drop_pa: f64,
    /// Explicit conducting area, only when that layer declares one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_m2: Option<f64>,
    /// Area-adjusted resistance per request boundary area; never a replacement observation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_resistance_m2_pa_w: Option<f64>,
}

/// Explicit non-material interface on the request boundary area; no inferred gas-gap physics.
#[derive(Clone, Debug, Serialize)]
pub struct InterfaceReading {
    /// Inner and outer material occurrence keys.
    pub layers: [String; 2],
    /// Declared resistance per boundary area in m²·Pa/W, including an explicitly supplied zero.
    pub resistance_m2_pa_w: f64,
    /// Potential vapor pressures at the interface's inner and outer boundaries, in Pa.
    pub inside_pressure_pa: f64,
    /// Potential vapor pressure at the outer boundary, in Pa.
    pub outside_pressure_pa: f64,
    /// Potential pressure drop, in Pa.
    pub pressure_drop_pa: f64,
}

/// Conditional endpoint envelopes from complete positive reported layer Ret bounds. Geometry,
/// pressure difference, films, interfaces and latent heat remain exactly the authored inputs.
/// Every output is monotone in the layer resistances; shared sources need no independence model.
/// These are not confidence intervals or uncertainty bounds on an actual garment.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ReportedBounds {
    /// Sum of area-adjusted material resistance endpoints, excluding films and interfaces.
    pub layer_resistance_m2_pa_w: [f64; 2],
    /// Total resistance endpoints, including the fixed films and interfaces.
    pub total_resistance_m2_pa_w: [f64; 2],
    /// Potential heat-flux endpoints, W/m². Resistance endpoints reverse under division.
    pub potential_heat_flux_w_m2: [f64; 2],
    /// Potential heat-flow endpoints on the fixed boundary area, W.
    pub potential_heat_flow_w: [f64; 2],
    /// Potential mass-flux endpoints using the fixed latent heat, kg/(m²·s).
    pub potential_mass_flux_kg_m2_s: [f64; 2],
    /// Potential mass-flow endpoints on the fixed boundary area, kg/s.
    pub potential_mass_flow_kg_s: [f64; 2],
}

/// Conditional transport potential, never an actual sweating or comfort prediction.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// ftx-vapor-stack-report/1.
    pub schema: &'static str,
    /// Complete typed request, including boundary/model inputs and source acceptance filters.
    pub request: Request,
    /// Identity of the typed request; not source or physical validation.
    pub request_sha256: String,
    /// Equivalent request with every material frozen for offline replay.
    pub replay_request: Request,
    /// Exact Rust serialization for replay without a JavaScript numeric round trip.
    pub replay_request_json: String,
    /// Ordered observations and their potential pressure profile.
    pub layers: Vec<LayerReading>,
    /// Explicit interfaces, absent from legacy reports with no requested interface terms.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<InterfaceReading>,
    /// Sum of layer resistances referred to the boundary area, excluding films and interfaces.
    pub layer_resistance_m2_pa_w: f64,
    /// Sum of all layer and film resistances, in m²·Pa/W.
    pub total_resistance_m2_pa_w: f64,
    /// Prescribed inside minus outside vapor partial pressure, in Pa.
    pub pressure_difference_pa: f64,
    /// Potential pressure drop across the inner film, in Pa.
    pub inside_film_pressure_drop_pa: f64,
    /// Potential pressure drop across the outer film, in Pa.
    pub outside_film_pressure_drop_pa: f64,
    /// Potential latent heat flux, Δp / total Ret, in W/m²; water availability is unknown.
    pub potential_heat_flux_w_m2: f64,
    /// Potential latent heat flow over the prescribed reference area, in W.
    pub potential_heat_flow_w: f64,
    /// Potential water-vapor mass flux using the caller's latent heat, in kg/(m²·s).
    pub potential_mass_flux_kg_m2_s: f64,
    /// Potential water-vapor mass flow over the prescribed reference area, in kg/s.
    pub potential_mass_flow_kg_s: f64,
    /// Present only when every consumed layer has positive reported bounds and all propagated
    /// endpoints are representable. Missing endpoints never become the nominal observation.
    pub reported_bounds: Option<ReportedBounds>,
    /// Missing layer bounds or endpoint arithmetic failures, independent of the nominal result.
    pub bounds_unavailable: Vec<String>,
    /// At least one reported method or complete conditions map differs from the first layer.
    pub mixed_protocols: bool,
    /// Effective network assumption, kept separate from each observation's evidence.
    pub effective_model: &'static str,
    /// Always false: the numeric network does not qualify a real assembly or human garment.
    pub physical_validation_claim: bool,
    /// Calculation and interpretation boundaries that accompany all consumers.
    pub notes: Vec<String>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn text_ok(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn add_resistance(sum: f64, value: f64) -> Result<f64, Refusal> {
    let next = sum + value;
    if !next.is_finite() || next <= sum {
        return Err(invalid(
            "vapor resistance sum overflows or loses a positive segment",
        ));
    }
    Ok(next)
}

impl Request {
    fn validate(&self) -> Result<(), Refusal> {
        if self.layers.len() > 128 || self.required_conditions.len() > 64 {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "vapor stack permits at most 128 layers and 64 condition filters",
            ));
        }
        if (!self.interface_resistance_m2_pa_w.is_empty()
            && self.interface_resistance_m2_pa_w.len() + 1 != self.layers.len())
            || self
                .interface_resistance_m2_pa_w
                .iter()
                .any(|r| !r.is_finite() || *r < 0.0)
            || self
                .layers
                .iter()
                .any(|l| l.area_m2.is_some_and(|a| !positive(a)))
        {
            return Err(invalid(
                "vapor network needs positive layer areas and one explicit nonnegative interface per adjacent pair",
            ));
        }
        let mut keys = BTreeSet::new();
        if self.schema != SCHEMA
            || self.layers.is_empty()
            || self
                .layers
                .iter()
                .any(|l| !text_ok(&l.key) || !keys.insert(&l.key))
            || self
                .required_conditions
                .iter()
                .any(|(k, v)| !text_ok(k) || !text_ok(v))
            || self.required_method.as_deref().is_some_and(|m| !text_ok(m))
        {
            return Err(invalid(
                "invalid vapor-stack schema, layer keys or source filters",
            ));
        }
        if !positive(self.area_m2)
            || !positive(self.latent_heat_j_kg)
            || !positive(self.inside_film_resistance_m2_pa_w)
            || !positive(self.outside_film_resistance_m2_pa_w)
            || !self.inside_vapor_pressure_pa.is_finite()
            || !self.outside_vapor_pressure_pa.is_finite()
            || self.outside_vapor_pressure_pa < 0.0
            || self.inside_vapor_pressure_pa < self.outside_vapor_pressure_pa
        {
            return Err(invalid(
                "vapor stack requires positive finite area, latent heat and films, and finite inside >= outside >= 0 Pa; reversed transport/condensation is outside this model",
            ));
        }
        Ok(())
    }

    /// Resolve only the requested vapor observations, then evaluate a constant-resistance network.
    ///
    /// A source measured at 35°C/40% RH/1 m/s remains qualified at precisely those conditions.
    /// Using its nominal Ret in the network does not interpolate/extrapolate it to an unstated
    /// temperature, humidity, wetness or pressure. The caller supplies the boundary assumptions.
    ///
    /// # Errors
    /// Invalid boundaries/budgets, unresolved or nonpositive Ret, source-filter mismatch,
    /// unadmitted estimates/protocol differences, digest mismatch or unrepresentable arithmetic.
    pub fn evaluate(&self, catalog: &Catalog) -> Result<Report, Refusal> {
        self.validate()?;
        let mut layers: Vec<LayerReading> = Vec::with_capacity(self.layers.len());
        let mut replay_request = self.clone();
        let mut layer_resistance = 0.0;
        let mut total_resistance = self.inside_film_resistance_m2_pa_w;
        let mut mixed_protocols = false;
        let mut interfaces: Vec<InterfaceReading> = self
            .interface_resistance_m2_pa_w
            .iter()
            .enumerate()
            .map(|(i, &r)| InterfaceReading {
                layers: [self.layers[i].key.clone(), self.layers[i + 1].key.clone()],
                resistance_m2_pa_w: r,
                inside_pressure_pa: 0.0,
                outside_pressure_pa: 0.0,
                pressure_drop_pa: 0.0,
            })
            .collect();
        for (index, layer) in self.layers.iter().enumerate() {
            let resolved = catalog.resolve(&layer.selection)?;
            let material = resolved.material();
            if matches!(material.form, Form::Fiber | Form::Yarn) {
                return Err(invalid(format!(
                    "{}: fiber/yarn observations are not a characterized vapor-transport sheet or fill layer",
                    layer.key
                )));
            }
            let resistance =
                material.require("vapor_resistance", "m2*Pa/W", resolved.allows_estimates())?;
            let observation = &material.properties["vapor_resistance"];
            if !positive(resistance) || observation.bounds.is_some_and(|b| b[0] <= 0.0) {
                return Err(invalid(format!(
                    "{}: vapor resistance and any reported bounds must be positive",
                    layer.key
                )));
            }
            if observation.conditions.len() > 64
                || observation
                    .conditions
                    .iter()
                    .any(|(k, v)| !text_ok(k) || !text_ok(v))
                || self
                    .required_conditions
                    .iter()
                    .any(|(k, v)| observation.conditions.get(k) != Some(v))
                || self
                    .required_method
                    .as_ref()
                    .is_some_and(|m| m != &observation.method)
            {
                return Err(invalid(format!(
                    "{}: vapor observation does not meet the requested method/conditions",
                    layer.key
                )));
            }
            if let Some(first) = layers.first() {
                mixed_protocols |= first.observation.method != observation.method
                    || first.observation.conditions != observation.conditions;
            }
            if mixed_protocols && !self.allow_mixed_protocols {
                return Err(invalid(
                    "vapor stack mixes reported methods/conditions; explicit allow_mixed_protocols is required",
                ));
            }
            let effective = match layer.area_m2 {
                Some(area) => resistance * (self.area_m2 / area),
                None => resistance,
            };
            if !positive(effective) {
                return Err(invalid(
                    "area-adjusted vapor resistance overflows or underflows",
                ));
            }
            layer_resistance = add_resistance(layer_resistance, effective)?;
            total_resistance = add_resistance(total_resistance, effective)?;
            if let Some(interface) = interfaces.get(index)
                && interface.resistance_m2_pa_w > 0.0
            {
                total_resistance = add_resistance(total_resistance, interface.resistance_m2_pa_w)?;
            }
            let selection = resolved.frozen_selection();
            replay_request.layers[index].selection = selection.clone();
            layers.push(LayerReading {
                key: layer.key.clone(),
                selection,
                material_sha256: resolved.sha256().into(),
                observation: observation.clone(),
                source: material.sources[&observation.source].clone(),
                inside_pressure_pa: 0.0,
                outside_pressure_pa: 0.0,
                pressure_drop_pa: 0.0,
                area_m2: layer.area_m2,
                effective_resistance_m2_pa_w: layer.area_m2.map(|_| effective),
            });
        }

        let total_resistance =
            add_resistance(total_resistance, self.outside_film_resistance_m2_pa_w)?;
        let pressure_difference = self.inside_vapor_pressure_pa - self.outside_vapor_pressure_pa;
        let heat_flux = pressure_difference / total_resistance;
        let heat_flow = heat_flux * self.area_m2;
        let mass_flux = heat_flux / self.latent_heat_j_kg;
        let mass_flow = mass_flux * self.area_m2;
        let inner_drop = heat_flux * self.inside_film_resistance_m2_pa_w;
        let outer_drop = heat_flux * self.outside_film_resistance_m2_pa_w;
        if [
            heat_flux, heat_flow, mass_flux, mass_flow, inner_drop, outer_drop,
        ]
        .iter()
        .any(|v| !v.is_finite() || (pressure_difference > 0.0 && *v <= 0.0))
        {
            return Err(invalid(
                "vapor stack flux arithmetic overflows or underflows",
            ));
        }

        // Accumulate from the outer reservoir so a small downstream pressure is not calculated
        // by subtracting a fraction near one. No saturation/condensation check is inferred.
        let mut downstream_resistance = self.outside_film_resistance_m2_pa_w;
        for (index, layer) in layers.iter_mut().enumerate().rev() {
            layer.outside_pressure_pa =
                self.outside_vapor_pressure_pa + heat_flux * downstream_resistance;
            let resistance = layer
                .effective_resistance_m2_pa_w
                .unwrap_or(layer.observation.value);
            downstream_resistance += resistance;
            layer.inside_pressure_pa =
                self.outside_vapor_pressure_pa + heat_flux * downstream_resistance;
            layer.pressure_drop_pa = heat_flux * resistance;
            if [
                layer.inside_pressure_pa,
                layer.outside_pressure_pa,
                layer.pressure_drop_pa,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
                || (pressure_difference > 0.0 && layer.pressure_drop_pa <= 0.0)
            {
                return Err(invalid("vapor stack pressure profile is not representable"));
            }
            if index > 0
                && let Some(interface) = interfaces.get_mut(index - 1)
            {
                interface.outside_pressure_pa = layer.inside_pressure_pa;
                downstream_resistance += interface.resistance_m2_pa_w;
                interface.inside_pressure_pa =
                    self.outside_vapor_pressure_pa + heat_flux * downstream_resistance;
                interface.pressure_drop_pa = heat_flux * interface.resistance_m2_pa_w;
                if [
                    interface.inside_pressure_pa,
                    interface.outside_pressure_pa,
                    interface.pressure_drop_pa,
                ]
                .iter()
                .any(|v| !v.is_finite() || *v < 0.0)
                    || (pressure_difference > 0.0
                        && interface.resistance_m2_pa_w > 0.0
                        && interface.pressure_drop_pa <= 0.0)
                {
                    return Err(invalid(
                        "vapor interface pressure profile is not representable",
                    ));
                }
            }
        }
        let area_aware = self.layers.iter().any(|l| l.area_m2.is_some()) || !interfaces.is_empty();
        let (reported_bounds, bounds_unavailable) = self.reported_bounds(&layers);
        let mut report = Report {
            schema: "ftx-vapor-stack-report/1",
            request: self.clone(),
            request_sha256: sha256_hex(&serde_json::to_vec(self).map_err(|e| invalid(e.to_string()))?),
            replay_request_json: serde_json::to_string(&replay_request).map_err(|e| invalid(e.to_string()))?,
            replay_request,
            layers,
            interfaces,
            layer_resistance_m2_pa_w: layer_resistance,
            total_resistance_m2_pa_w: total_resistance,
            pressure_difference_pa: pressure_difference,
            inside_film_pressure_drop_pa: inner_drop,
            outside_film_pressure_drop_pa: outer_drop,
            potential_heat_flux_w_m2: heat_flux,
            potential_heat_flow_w: heat_flow,
            potential_mass_flux_kg_m2_s: mass_flux,
            potential_mass_flow_kg_s: mass_flow,
            reported_bounds,
            bounds_unavailable,
            mixed_protocols,
            effective_model: if area_aware { AREA_MODEL } else { MODEL },
            physical_validation_claim: false,
            notes: vec![
                "This common-area, isothermal series network assumes each selected nominal Ret remains constant and adds only the two explicitly supplied films. A computed assembly is not a measured assembly; layer contact, gaps, compression, swelling, advection changes and temperature/humidity extrapolation are not modeled.".into(),
                "Heat and mass values are conditional transport potentials. Latent heat is a caller-supplied conversion assumption, not a source-certified or temperature-derived value. Water availability and sweating are unknown; these are not actual latent heat loss, wearer warmth/comfort or safety predictions. There is no dry-heat coupling, moisture storage, liquid flow or condensation calculation.".into(),
                "Original specimen, method, conditions, evidence and reported variability remain visible. Complete method/condition equality is a conservative filter, not empirical comparability proof; admitting mixed protocols does not make their conditions equivalent. Complete positive reported Ret endpoints propagate monotonically at fixed geometry, films, interfaces, pressure and latent heat. Shared sources require no independence assumption. These conditional ranges are not confidence intervals, measured assembly accuracy or general uncertainty; SD/CV are never converted into bounds.".into(),
            ],
        };
        if area_aware {
            report.notes[0] = "This isothermal series network retains each nominal source Ret and explicitly scales its resistance by boundary area / layer conducting area. Films and declared interfaces use boundary area. Changed conducting area does not characterize strain-dependent Ret. No air-gap, compression, swelling, advection or temperature/humidity law is inferred.".into();
        }
        Ok(report)
    }

    fn reported_bounds(&self, layers: &[LayerReading]) -> (Option<ReportedBounds>, Vec<String>) {
        let missing: Vec<_> = layers
            .iter()
            .filter(|layer| layer.observation.bounds.is_none())
            .map(|layer| format!("{}: vapor resistance has no reported bounds", layer.key))
            .collect();
        if !missing.is_empty() {
            return (None, missing);
        }
        match self.bound_endpoints(layers) {
            Ok(bounds) => (Some(bounds), Vec::new()),
            Err(error) => (
                None,
                vec![format!("reported vapor bounds: {}", error.message)],
            ),
        }
    }

    fn bound_endpoints(&self, layers: &[LayerReading]) -> Result<ReportedBounds, Refusal> {
        let mut layer_resistance = [0.; 2];
        let mut total_resistance = [self.inside_film_resistance_m2_pa_w; 2];
        for (index, layer) in layers.iter().enumerate() {
            let observed = layer.observation.bounds.expect("complete source bounds");
            for endpoint in 0..2 {
                let effective = match layer.area_m2 {
                    Some(area) => observed[endpoint] * (self.area_m2 / area),
                    None => observed[endpoint],
                };
                if !positive(effective) {
                    return Err(invalid(
                        "area-adjusted resistance endpoint overflows or underflows",
                    ));
                }
                layer_resistance[endpoint] = add_resistance(layer_resistance[endpoint], effective)?;
                total_resistance[endpoint] = add_resistance(total_resistance[endpoint], effective)?;
                if let Some(&interface) = self.interface_resistance_m2_pa_w.get(index)
                    && interface > 0.
                {
                    total_resistance[endpoint] =
                        add_resistance(total_resistance[endpoint], interface)?;
                }
            }
        }
        for resistance in &mut total_resistance {
            *resistance = add_resistance(*resistance, self.outside_film_resistance_m2_pa_w)?;
        }
        let pressure_difference = self.inside_vapor_pressure_pa - self.outside_vapor_pressure_pa;
        let heat_flux = [
            pressure_difference / total_resistance[1],
            pressure_difference / total_resistance[0],
        ];
        let heat_flow = heat_flux.map(|v| v * self.area_m2);
        let mass_flux = heat_flux.map(|v| v / self.latent_heat_j_kg);
        let mass_flow = mass_flux.map(|v| v * self.area_m2);
        if [heat_flux, heat_flow, mass_flux, mass_flow]
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || (pressure_difference > 0. && *v <= 0.))
        {
            return Err(invalid("flux endpoint overflows or underflows"));
        }
        Ok(ReportedBounds {
            layer_resistance_m2_pa_w: layer_resistance,
            total_resistance_m2_pa_w: total_resistance,
            potential_heat_flux_w_m2: heat_flux,
            potential_heat_flow_w: heat_flow,
            potential_mass_flux_kg_m2_s: mass_flux,
            potential_mass_flow_kg_s: mass_flow,
        })
    }
}
