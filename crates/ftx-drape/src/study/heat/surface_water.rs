//! Free liquid owned by stable textile triangles, with explicit temperature feedback.
//! This is an inner-cell equilibrium film, not sorption, sweat, capillary transport or wet cloth
//! mechanics. A separate source must qualify fixed dry thermal/Ret properties over the full domain.

use std::collections::BTreeMap;

use ftx_core::{Refusal, Vec3};
use ftx_pattern::{
    material_catalog::ResolvedMaterial,
    material_vapor,
    materials::{CATALOG_SCHEMA, Catalog, Evidence, Source},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::invalid;
use crate::{
    run::DrapeRun,
    thermal::surface::{Surface, SurfaceState, SurfaceStep, io::retained_materials},
};

mod mechanics;

/// Source-locked initial loading on every retained instance, including explicitly dry instances.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// Exact normalized source material identity; a replacement requires new qualification.
    pub material_sha256: String,
    /// Initial free liquid per REFERENCE material area, kg/m². It adds no mechanical mass.
    pub initial_liquid_kg_m2: f64,
}

/// Prescribed liquid delivery to one instance, constant over each mechanical keyframe interval.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyBinding {
    /// Nonnegative kg/(reference m²·s), with an explicit value for every interval.
    pub rate_kg_m2_s: Vec<f64>,
    /// Incoming liquid temperature in °C, with an explicit value for every interval.
    pub temperature_c: Vec<f64>,
}

/// Independently qualified liquid supply; no body reservoir or unreported transfer is implied.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Supply {
    /// Must be `ftx-textile-liquid-supply/1`.
    pub schema: String,
    /// Every water-bound instance, including explicitly zero rates.
    pub bindings: BTreeMap<String, SupplyBinding>,
    /// Provenance for delivery rates and temperatures of the same declared liquid.
    pub source: Source,
    /// Independent evidence class for the supplied boundary values.
    pub evidence: Evidence,
    /// Explicit consent for estimated supply inputs.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Source method for the imposed delivery histories.
    pub method: String,
}

/// Explicit reverse transport and phase-change qualification for the same locked textile.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condensation {
    /// Must be `ftx-textile-condensation/1`.
    pub schema: String,
    /// Incoming vapor temperature in °C for every mechanical keyframe interval.
    /// Its effective sensible enthalpy is cp_liquid * (temperature - reference), as for
    /// outgoing vapor in this constant-latent model; it is not an inferred gas heat capacity.
    pub vapor_temperature_c: Vec<f64>,
    /// Independent evidence for the incoming reservoir and reverse phase/transport closure.
    pub source: Source,
    /// Evidence for this closure, independent of liquid supply and forward Ret observations.
    pub evidence: Evidence,
    /// Explicit consent for estimated reverse-transfer inputs.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Source method for the incoming vapor temperatures and boundary conditions.
    pub method: String,
    /// Source basis for reverse use of the exact locked Ret networks, condensation onto wet
    /// and dry patches, and the constant-latent/effective-sensible enthalpy convention over
    /// the complete declared pressure, temperature and liquid-loading domain.
    pub qualification: String,
}

/// Explicit free-film closure for the material-following thermal history.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Version 1 is initial-only; version 2 requires supply; version 3 additionally admits
    /// independently qualified condensation and subsequent re-evaporation.
    pub schema: String,
    /// Must be `inner_cell_equilibrium_free_film_frozen_properties/1`.
    pub model: String,
    /// Every retained cloth instance exactly once, including zero initial loadings.
    pub bindings: BTreeMap<String, Binding>,
    /// Source-qualified range is [0, maximum] kg/m² of reference area.
    pub maximum_liquid_loading_kg_m2: f64,
    /// Explicit constant liquid sensible capacity in J/(kg·K).
    pub liquid_specific_heat_j_kg_k: f64,
    /// Explicit constant latent energy in J/kg, relative to liquid at the sampled temperature.
    pub latent_heat_j_kg: f64,
    /// Common reference for liquid storage and exported vapor sensible enthalpy, °C.
    pub enthalpy_reference_c: f64,
    /// Source-qualified wet pressure: [°C, Pa], strictly increasing temperature, nondecreasing
    /// pressure. Piecewise linear, 2..=128 knots, no extrapolation or inferred saturation law.
    pub wet_vapor_pressure_pa: Vec<[f64; 2]>,
    /// Explicit ambient vapor pressure per mechanical keyframe interval, Pa.
    pub outside_vapor_pressure_pa: Vec<f64>,
    /// Positive declared film resistance before the textile Ret, m²·Pa/W.
    pub inside_film_resistance_m2_pa_w: f64,
    /// Positive declared film resistance after the textile Ret, m²·Pa/W.
    pub outside_film_resistance_m2_pa_w: f64,
    /// Version 1 requires exactly zero; versions 2/3 omit this scalar in favor of `supply`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replenishment_kg_m2_s: Option<f64>,
    /// Required by versions 2/3 and forbidden by version 1. Zero delivery remains explicit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supply: Option<Supply>,
    /// Required by version 3 and forbidden by earlier versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condensation: Option<Condensation>,
    /// Provenance for all fluid/boundary values and this exact material-domain qualification.
    pub source: Source,
    /// Independent of the evidence and estimate policy of each selected material.
    pub evidence: Evidence,
    /// Explicit consent for estimated free-film inputs; does not relax material consent.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Test/model method for these explicit liquid and pressure inputs.
    pub method: String,
    /// Source's basis for retaining the locked dry thermal and Ret observations throughout
    /// the pressure table's temperature domain and [0, maximum] liquid-loading domain.
    pub qualification: String,
}

impl Request {
    pub(super) fn validate(&self, intervals: usize) -> Result<(), Refusal> {
        let text = |s: &str| !s.trim().is_empty() && s.len() <= 4096;
        let version_valid = match self.schema.as_str() {
            "ftx-textile-surface-water/1" => {
                self.replenishment_kg_m2_s == Some(0.)
                    && self.supply.is_none()
                    && self.condensation.is_none()
            }
            "ftx-textile-surface-water/2" => {
                self.replenishment_kg_m2_s.is_none()
                    && self.supply.is_some()
                    && self.condensation.is_none()
            }
            "ftx-textile-surface-water/3" => {
                self.replenishment_kg_m2_s.is_none()
                    && self.supply.is_some()
                    && self.condensation.is_some()
            }
            _ => false,
        };
        if !version_valid
            || self.model != "inner_cell_equilibrium_free_film_frozen_properties/1"
            || self.bindings.is_empty()
            || self.bindings.len() > 65_536
            || !self.enthalpy_reference_c.is_finite()
            || self.enthalpy_reference_c < -273.15
            || [
                self.maximum_liquid_loading_kg_m2,
                self.liquid_specific_heat_j_kg_k,
                self.latent_heat_j_kg,
                self.inside_film_resistance_m2_pa_w,
                self.outside_film_resistance_m2_pa_w,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.)
            || self.outside_vapor_pressure_pa.len() != intervals
            || self
                .outside_vapor_pressure_pa
                .iter()
                .any(|v| !v.is_finite() || *v < 0.)
            || !(2..=128).contains(&self.wet_vapor_pressure_pa.len())
            || self
                .wet_vapor_pressure_pa
                .iter()
                .any(|p| !p[0].is_finite() || p[0] < -273.15 || !p[1].is_finite() || p[1] < 0.)
            || self.wet_vapor_pressure_pa.windows(2).any(|p| {
                p[1][0] <= p[0][0] || !(p[1][0] - p[0][0]).is_finite() || p[1][1] < p[0][1]
            })
            || !text(&self.source.reference)
            || !text(&self.source.note)
            || self.source.accessed_on.len() != 10
            || !text(&self.method)
            || !text(&self.qualification)
            || (self.evidence == Evidence::Estimate && !self.allow_estimates)
            || self.bindings.iter().any(|(id, b)| {
                !text(id)
                    || b.material_sha256.len() != 64
                    || !b
                        .material_sha256
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                    || !b.initial_liquid_kg_m2.is_finite()
                    || b.initial_liquid_kg_m2 < 0.
                    || b.initial_liquid_kg_m2 > self.maximum_liquid_loading_kg_m2
            })
        {
            return Err(invalid(
                "surface water needs its explicit versioned liquid model, locked bindings, positive SI inputs, monotone pressure table, source-qualified domain and independent estimate consent",
            ));
        }
        if let Some(supply) = &self.supply
            && (supply.schema != "ftx-textile-liquid-supply/1"
                || supply.bindings.keys().ne(self.bindings.keys())
                || !text(&supply.source.reference)
                || !text(&supply.source.note)
                || supply.source.accessed_on.len() != 10
                || !text(&supply.method)
                || (supply.evidence == Evidence::Estimate && !supply.allow_estimates)
                || supply.bindings.values().any(|binding| {
                    binding.rate_kg_m2_s.len() != intervals
                        || binding.temperature_c.len() != intervals
                        || binding
                            .rate_kg_m2_s
                            .iter()
                            .any(|v| !v.is_finite() || *v < 0.)
                        || binding
                            .temperature_c
                            .iter()
                            .any(|&t| self.pressure(t).is_err())
                }))
        {
            return Err(invalid(
                "textile liquid supply needs complete instance/interval rates and qualified incoming temperatures, source metadata and its own estimate consent",
            ));
        }
        if let Some(condensation) = &self.condensation
            && (condensation.schema != "ftx-textile-condensation/1"
                || condensation.vapor_temperature_c.len() != intervals
                || condensation
                    .vapor_temperature_c
                    .iter()
                    .any(|&t| self.pressure(t).is_err())
                || !text(&condensation.source.reference)
                || !text(&condensation.source.note)
                || condensation.source.accessed_on.len() != 10
                || !text(&condensation.method)
                || !text(&condensation.qualification)
                || (condensation.evidence == Evidence::Estimate && !condensation.allow_estimates))
        {
            return Err(invalid(
                "condensation needs qualified interval vapor temperatures, independently admitted reverse transport, wet/dry phase-change and effective enthalpy evidence",
            ));
        }
        Ok(())
    }

    fn pressure(&self, temperature: f64) -> Result<f64, Refusal> {
        let table = &self.wet_vapor_pressure_pa;
        if !temperature.is_finite()
            || temperature < table[0][0]
            || temperature > table[table.len() - 1][0]
        {
            return Err(invalid(
                "surface-water temperature leaves the raw qualified pressure/material domain; reduce thermal max_step_s or supply wider source evidence",
            ));
        }
        for pair in table.windows(2) {
            if temperature <= pair[1][0] {
                let t = (temperature - pair[0][0]) / (pair[1][0] - pair[0][0]);
                return Ok(pair[0][1] + t * (pair[1][1] - pair[0][1]));
            }
        }
        unreachable!("validated closed temperature domain")
    }
}

#[derive(Clone)]
pub(super) struct Water {
    request: Request,
    networks: Vec<material_vapor::Report>,
    instances: Vec<String>,
    initial: Vec<f64>,
    remaining: Vec<f64>,
    evaporated: Vec<f64>,
    supplied: Vec<f64>,
    supplied_sensible_j: Vec<f64>,
    condensed: Vec<f64>,
    incoming_vapor_sensible_j: Vec<f64>,
    initial_temperature_c: f64,
    latent_j: f64,
    condensation_latent_j: f64,
    exported_sensible_j: f64,
    liquid_stored_j: f64,
    dryout_events: usize,
    mechanical_flow: Option<mechanics::IntervalFlow>,
}

pub(super) struct Plan {
    pub seconds: f64,
    remaining: Vec<f64>,
    evaporated: Vec<f64>,
    supplied: Vec<f64>,
    supplied_sensible_j: Vec<f64>,
    supplied_old_sensible_j: f64,
    condensed: Vec<f64>,
    incoming_vapor_sensible_j: Vec<f64>,
    condensed_old_sensible_j: f64,
    condensation_latent_j: Vec<f64>,
    pub capacity_j_k: Vec<f64>,
    pub latent_j: Vec<f64>,
    pub liquid_heat_j: Vec<f64>,
    exported_sensible_j: f64,
    dryout_events: usize,
}

fn add_positive(total: &mut f64, increment: f64) -> Result<(), Refusal> {
    let sum = *total + increment;
    if !increment.is_finite()
        || increment < 0.
        || !sum.is_finite()
        || (increment > 0. && sum <= *total)
    {
        return Err(invalid(
            "surface-water accumulation loses a positive mass or energy term",
        ));
    }
    *total = sum;
    Ok(())
}

fn positive_sum(values: &[f64]) -> Result<f64, Refusal> {
    let mut total = 0.;
    for &value in values {
        add_positive(&mut total, value)?;
    }
    Ok(total)
}

impl Water {
    pub(super) fn remaining_liquid_kg(&self) -> &[f64] {
        &self.remaining
    }

    pub(super) fn new(
        request: Request,
        model: &Surface,
        run: &DrapeRun,
        initial_c: f64,
    ) -> Result<Self, Refusal> {
        let (records, _) = retained_materials(
            &run.report["material"],
            &run.scene.mesh.instances,
            &run.scene.triangle_owner,
        )?;
        Self::from_materials(
            request,
            model,
            &records,
            &run.scene.mesh.instances,
            &run.scene.triangle_owner,
            initial_c,
        )
    }

    fn from_materials(
        request: Request,
        model: &Surface,
        records: &[ResolvedMaterial],
        instances: &[String],
        owners: &[usize],
        initial_c: f64,
    ) -> Result<Self, Refusal> {
        request.validate(request.outside_vapor_pressure_pa.len())?;
        request.pressure(initial_c)?;
        if instances.len() != request.bindings.len()
            || instances
                .iter()
                .any(|id| !request.bindings.contains_key(id))
            || owners.len() != model.patches().len()
            || records.len() != model.column_count()
        {
            return Err(invalid(
                "surface water must bind every retained instance and source triangle exactly once",
            ));
        }
        let catalog = Catalog {
            schema: CATALOG_SCHEMA.into(),
            materials: vec![],
        };
        let networks = records
            .iter()
            .map(|record| {
                material_vapor::Request {
                    schema: material_vapor::SCHEMA.into(),
                    layers: vec![material_vapor::Layer {
                        key: record.material().id.clone(),
                        selection: record.frozen_selection(),
                        area_m2: None,
                    }],
                    area_m2: 1.,
                    inside_vapor_pressure_pa: 1.,
                    outside_vapor_pressure_pa: 0.,
                    inside_film_resistance_m2_pa_w: request.inside_film_resistance_m2_pa_w,
                    outside_film_resistance_m2_pa_w: request.outside_film_resistance_m2_pa_w,
                    latent_heat_j_kg: request.latent_heat_j_kg,
                    required_conditions: BTreeMap::new(),
                    required_method: None,
                    allow_mixed_protocols: false,
                    interface_resistance_m2_pa_w: vec![],
                }
                .evaluate(&catalog)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut initial = Vec::with_capacity(owners.len());
        let mut patch_instances = Vec::with_capacity(owners.len());
        for (patch, &owner) in model.patches().iter().zip(owners) {
            let id = instances
                .get(owner)
                .ok_or_else(|| invalid("invalid surface-water triangle owner"))?;
            let binding = &request.bindings[id];
            if binding.material_sha256 != records[patch.column].sha256() {
                return Err(invalid(format!(
                    "surface-water qualification does not match locked material for {id}"
                )));
            }
            let mass = binding.initial_liquid_kg_m2 * patch.reference_area_m2;
            if !mass.is_finite() || (binding.initial_liquid_kg_m2 > 0. && mass <= 0.) {
                return Err(invalid(
                    "initial surface liquid mass overflows or underflows",
                ));
            }
            initial.push(mass);
            patch_instances.push(id.clone());
        }
        Ok(Self {
            request,
            networks,
            instances: patch_instances,
            remaining: initial.clone(),
            evaporated: vec![0.; initial.len()],
            supplied: vec![0.; initial.len()],
            supplied_sensible_j: vec![0.; initial.len()],
            condensed: vec![0.; initial.len()],
            incoming_vapor_sensible_j: vec![0.; initial.len()],
            initial,
            initial_temperature_c: initial_c,
            latent_j: 0.,
            condensation_latent_j: 0.,
            exported_sensible_j: 0.,
            liquid_stored_j: 0.,
            dryout_events: 0,
            mechanical_flow: None,
        })
    }

    pub(super) fn plan(
        &self,
        model: &Surface,
        old: &SurfaceState,
        positions: &[Vec3],
        interval: usize,
        maximum_seconds: f64,
    ) -> Result<Plan, Refusal> {
        let outside = *self
            .request
            .outside_vapor_pressure_pa
            .get(interval)
            .ok_or_else(|| invalid("missing surface-water ambient interval"))?;
        let mut seconds = maximum_seconds;
        let mut rates = Vec::with_capacity(self.remaining.len());
        let mut dry_times = Vec::with_capacity(self.remaining.len());
        for (i, patch) in model.patches().iter().enumerate() {
            for &t in &old.temperature_c()[i] {
                self.request.pressure(t)?;
            }
            let (supply_rate, supply_temperature) = match &self.request.supply {
                Some(supply) => {
                    let binding = &supply.bindings[&self.instances[i]];
                    let rate = binding.rate_kg_m2_s[interval] * patch.reference_area_m2;
                    if !rate.is_finite() || (binding.rate_kg_m2_s[interval] > 0. && rate <= 0.) {
                        return Err(invalid(
                            "reference-area liquid supply rate is unrepresentable",
                        ));
                    }
                    (rate, binding.temperature_c[interval])
                }
                None => (0., old.temperature_c()[i][0]),
            };
            let potential_rate = if self.remaining[i] == 0.
                && supply_rate == 0.
                && self.request.condensation.is_none()
            {
                0.
            } else {
                let inside = self.request.pressure(old.temperature_c()[i][0])?;
                let flux = if self.request.condensation.is_some() {
                    self.networks[patch.column].signed_flux_at_pressures(inside, outside)?
                } else {
                    self.networks[patch.column].flux_at_pressures(inside, outside)
                        .map_err(|_| invalid("surface-water wet pressure is below ambient (condensation unsupported), or vapor flux is unrepresentable"))?
                };
                let p = patch.vertices.map(|v| positions.get(v).copied());
                let [Some(a), Some(b), Some(c)] = p else {
                    return Err(invalid("missing liquid surface geometry"));
                };
                let area = 0.5 * (b - a).cross(c - a).norm();
                let rate = flux.mass_kg_m2_s * area;
                if !area.is_finite()
                    || area <= 0.
                    || !rate.is_finite()
                    || (flux.mass_kg_m2_s != 0. && rate == 0.)
                {
                    return Err(invalid(
                        "surface-water current area or vapor rate is unrepresentable",
                    ));
                }
                rate
            };
            // At zero inventory, freshly delivered liquid can evaporate continuously but no
            // unmet potential may create negative storage. A positive net delivery rewets it.
            let rate = if self.remaining[i] == 0. {
                potential_rate.max(0.).min(supply_rate)
            } else {
                potential_rate.max(0.)
            };
            let condensation_rate = (-potential_rate).max(0.);
            let vapor_temperature = self
                .request
                .condensation
                .as_ref()
                .map_or(old.temperature_c()[i][0], |c| {
                    c.vapor_temperature_c[interval]
                });
            let dry_time = if self.remaining[i] > 0. && rate > supply_rate {
                self.remaining[i] / (rate - supply_rate)
            } else {
                f64::INFINITY
            };
            seconds = seconds.min(dry_time);
            rates.push((
                rate,
                supply_rate,
                supply_temperature,
                condensation_rate,
                vapor_temperature,
            ));
            dry_times.push(dry_time);
        }
        if !seconds.is_finite() || seconds <= 0. {
            return Err(invalid("surface-water dryout time is not representable"));
        }
        let mut plan = Plan {
            seconds,
            remaining: Vec::new(),
            evaporated: Vec::new(),
            supplied: Vec::new(),
            supplied_sensible_j: Vec::new(),
            supplied_old_sensible_j: 0.,
            condensed: Vec::new(),
            incoming_vapor_sensible_j: Vec::new(),
            condensed_old_sensible_j: 0.,
            condensation_latent_j: Vec::new(),
            capacity_j_k: Vec::new(),
            latent_j: Vec::new(),
            liquid_heat_j: Vec::new(),
            exported_sensible_j: 0.,
            dryout_events: 0,
        };
        for (i, (rate, supply_rate, supply_temperature, condensation_rate, vapor_temperature)) in
            rates.into_iter().enumerate()
        {
            let supplied = supply_rate * seconds;
            let condensed = condensation_rate * seconds;
            let evaporated = if dry_times[i] == seconds {
                self.remaining[i] + supplied
            } else if self.remaining[i] == 0. && rate == supply_rate {
                supplied
            } else {
                rate * seconds
            };
            let remaining = if dry_times[i] == seconds {
                0.
            } else if supply_rate == 0. {
                if condensed == 0. {
                    self.remaining[i] - evaporated
                } else {
                    self.remaining[i] + (condensed - evaporated)
                }
            } else {
                self.remaining[i] + (supplied + condensed - evaporated)
            };
            let latent = evaporated * self.request.latent_heat_j_kg;
            let condensation_latent = condensed * self.request.latent_heat_j_kg;
            let capacity = remaining * self.request.liquid_specific_heat_j_kg_k;
            let maximum =
                self.request.maximum_liquid_loading_kg_m2 * model.patches()[i].reference_area_m2;
            if self.request.supply.is_some() && (!maximum.is_finite() || remaining > maximum) {
                return Err(invalid(
                    "liquid supply or condensation exceeds the qualified reference-area loading domain; no overflow or drainage law is declared",
                ));
            }
            if !remaining.is_finite()
                || remaining < 0.
                || !supplied.is_finite()
                || !condensed.is_finite()
                || !evaporated.is_finite()
                || (supply_rate > 0. && supplied <= 0.)
                || (condensation_rate > 0. && condensed <= 0.)
                || !latent.is_finite()
                || !condensation_latent.is_finite()
                || !capacity.is_finite()
                || (rate > 0. && (evaporated <= 0. || latent <= 0.))
                || (condensed > 0. && condensation_latent <= 0.)
                || (supplied + condensed > evaporated && remaining <= self.remaining[i])
                || (supplied + condensed < evaporated && remaining >= self.remaining[i])
                || (dry_times[i] == seconds && evaporated <= supplied)
                || (remaining > 0. && capacity <= 0.)
            {
                return Err(invalid(
                    "surface-water mass, latent energy or capacity loses a positive term",
                ));
            }
            plan.dryout_events += usize::from(self.remaining[i] > 0. && remaining == 0.);
            plan.remaining.push(remaining);
            plan.evaporated.push(evaporated);
            plan.supplied.push(supplied);
            plan.condensed.push(condensed);
            plan.capacity_j_k.push(capacity);
            plan.latent_j.push(latent);
            plan.condensation_latent_j.push(condensation_latent);
            let supplied_capacity = supplied * self.request.liquid_specific_heat_j_kg_k;
            let supplied_sensible =
                supplied_capacity * (supply_temperature - self.request.enthalpy_reference_c);
            let mixing = supplied_capacity * (supply_temperature - old.temperature_c()[i][0]);
            plan.supplied_sensible_j.push(supplied_sensible);
            plan.supplied_old_sensible_j +=
                supplied_capacity * (old.temperature_c()[i][0] - self.request.enthalpy_reference_c);
            let condensed_capacity = condensed * self.request.liquid_specific_heat_j_kg_k;
            let incoming_sensible =
                condensed_capacity * (vapor_temperature - self.request.enthalpy_reference_c);
            plan.incoming_vapor_sensible_j.push(incoming_sensible);
            plan.condensed_old_sensible_j += condensed_capacity
                * (old.temperature_c()[i][0] - self.request.enthalpy_reference_c);
            let condensation_mixing =
                condensed_capacity * (vapor_temperature - old.temperature_c()[i][0]);
            plan.liquid_heat_j
                .push(mixing - latent + condensation_mixing + condensation_latent);
            // Outflow samples the OLD temperature, exactly as the explicit pressure/rate law.
            plan.exported_sensible_j += evaporated
                * self.request.liquid_specific_heat_j_kg_k
                * (old.temperature_c()[i][0] - self.request.enthalpy_reference_c);
        }
        if !plan.exported_sensible_j.is_finite()
            || !plan.supplied_old_sensible_j.is_finite()
            || !plan.condensed_old_sensible_j.is_finite()
            || plan.supplied_sensible_j.iter().any(|v| !v.is_finite())
            || plan
                .incoming_vapor_sensible_j
                .iter()
                .any(|v| !v.is_finite())
            || plan.liquid_heat_j.iter().any(|v| !v.is_finite())
        {
            return Err(invalid(
                "supplied liquid or exported vapor sensible enthalpy overflows",
            ));
        }
        Ok(plan)
    }

    pub(super) fn accept(
        &mut self,
        old: &SurfaceState,
        next: &mut SurfaceStep,
        plan: Plan,
        interval: usize,
    ) -> Result<(), Refusal> {
        let cp = self.request.liquid_specific_heat_j_kg_k;
        let reference = self.request.enthalpy_reference_c;
        let mut liquid_step = 0.;
        let latent_step = positive_sum(&plan.latent_j)?;
        let condensation_latent_step = positive_sum(&plan.condensation_latent_j)?;
        for i in 0..self.remaining.len() {
            for &t in &next.state.temperature_c()[i] {
                self.request.pressure(t)?;
            }
            if self.request.condensation.is_none()
                && (plan.remaining[i] > 0. || plan.supplied[i] > 0.)
                && self.request.pressure(next.state.temperature_c()[i][0])?
                    < self.request.outside_vapor_pressure_pa[interval]
            {
                return Err(invalid(
                    "surface-water cooling crosses into unsupported condensation; reduce thermal max_step_s",
                ));
            }
            liquid_step += cp
                * (plan.remaining[i] * (next.state.temperature_c()[i][0] - reference)
                    - self.remaining[i] * (old.temperature_c()[i][0] - reference));
        }
        // The solver includes signed latent heat and incoming liquid/vapor mixing at T_old.
        // Subtract the old-temperature sensible part of both incoming masses to reconcile their
        // complete reference enthalpy, including the liquid capacity that now belongs to them.
        let error = next.stored_change_j + liquid_step + plan.exported_sensible_j
            - plan.seconds * (next.inside_heat_w - next.outside_heat_w)
            - next.supplied_heat_j
            - plan.supplied_old_sensible_j
            - plan.condensed_old_sensible_j;
        let scale = 1.
            + next.stored_change_j.abs()
            + liquid_step.abs()
            + plan.exported_sensible_j.abs()
            + next.supplied_heat_j.abs()
            + plan.supplied_old_sensible_j.abs()
            + plan.condensed_old_sensible_j.abs();
        if !error.is_finite() || !scale.is_finite() || error.abs() > 1e-8 * scale {
            return Err(invalid(
                "surface-water sensible/latent energy balance failed",
            ));
        }
        if let Some(flow) = &mut self.mechanical_flow {
            flow.accept(&plan)?;
        }
        add_positive(&mut self.latent_j, latent_step)?;
        add_positive(&mut self.condensation_latent_j, condensation_latent_step)?;
        self.exported_sensible_j += plan.exported_sensible_j;
        self.liquid_stored_j += liquid_step;
        self.dryout_events += plan.dryout_events;
        for (total, increment) in self.evaporated.iter_mut().zip(plan.evaporated) {
            add_positive(total, increment)?;
        }
        for (total, increment) in self.supplied.iter_mut().zip(plan.supplied) {
            add_positive(total, increment)?;
        }
        for (total, increment) in self.condensed.iter_mut().zip(plan.condensed) {
            add_positive(total, increment)?;
        }
        for (total, increment) in self
            .supplied_sensible_j
            .iter_mut()
            .zip(plan.supplied_sensible_j)
        {
            *total += increment;
            if !total.is_finite() {
                return Err(invalid(
                    "cumulative supplied liquid sensible enthalpy overflows",
                ));
            }
        }
        for (total, increment) in self
            .incoming_vapor_sensible_j
            .iter_mut()
            .zip(plan.incoming_vapor_sensible_j)
        {
            *total += increment;
            if !total.is_finite() {
                return Err(invalid(
                    "cumulative incoming vapor sensible enthalpy overflows",
                ));
            }
        }
        self.remaining = plan.remaining;
        next.balance_error_j = error;
        self.snapshot(next.state.temperature_c(), false, 0.)?;
        Ok(())
    }

    pub(super) fn energy_correction_j(&self) -> f64 {
        self.liquid_stored_j + self.exported_sensible_j + self.latent_j
            - self.supplied_sensible_j.iter().sum::<f64>()
            - self.condensation_latent_j
            - self.incoming_vapor_sensible_j.iter().sum::<f64>()
    }

    pub(super) fn snapshot(
        &self,
        temperatures: &[Vec<f64>],
        full: bool,
        balance_error_j: f64,
    ) -> Result<Value, Refusal> {
        let initial = positive_sum(&self.initial)?;
        let remaining = positive_sum(&self.remaining)?;
        let evaporated = positive_sum(&self.evaporated)?;
        let supplied = positive_sum(&self.supplied)?;
        let supplied_sensible: f64 = self.supplied_sensible_j.iter().sum();
        let condensed = positive_sum(&self.condensed)?;
        let incoming_sensible: f64 = self.incoming_vapor_sensible_j.iter().sum();
        let error = remaining + evaporated - initial - supplied - condensed;
        let reconstructed: f64 = self
            .initial
            .iter()
            .zip(&self.remaining)
            .zip(temperatures)
            .map(|((&initial, &remaining), t)| {
                self.request.liquid_specific_heat_j_kg_k
                    * (remaining * (t[0] - self.request.enthalpy_reference_c)
                        - initial
                            * (self.initial_temperature_c - self.request.enthalpy_reference_c))
            })
            .sum();
        if [
            initial,
            remaining,
            evaporated,
            supplied,
            supplied_sensible,
            condensed,
            incoming_sensible,
            error,
            reconstructed,
            self.latent_j,
            self.condensation_latent_j,
            self.exported_sensible_j,
            self.liquid_stored_j,
            balance_error_j,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || error.abs() > 1024. * f64::EPSILON * (initial + supplied + condensed)
            || (reconstructed - self.liquid_stored_j).abs()
                > 1e-8 * (1. + reconstructed.abs() + self.liquid_stored_j.abs())
            || (self.latent_j - evaporated * self.request.latent_heat_j_kg).abs()
                > 1e-8 * (1. + self.latent_j)
            || (self.condensation_latent_j - condensed * self.request.latent_heat_j_kg).abs()
                > 1e-8 * (1. + self.condensation_latent_j)
        {
            return Err(invalid(
                "surface-water cumulative mass or enthalpy ledger failed",
            ));
        }
        let mut result = json!({"schema":"ftx-textile-surface-water-result/1",
            "initial_liquid_kg":initial,"remaining_liquid_kg":remaining,"evaporated_liquid_kg":evaporated,
            "latent_energy_j":self.latent_j,"exported_vapor_sensible_energy_j":self.exported_sensible_j,
            "liquid_stored_change_j":self.liquid_stored_j,"mass_balance_error_kg":error,
            "dryout_events":self.dryout_events,"balance_error_j":balance_error_j});
        if self.request.supply.is_some() {
            result["schema"] = json!("ftx-textile-surface-water-result/2");
            result["supplied_liquid_kg"] = json!(supplied);
            result["supplied_liquid_sensible_energy_j"] = json!(supplied_sensible);
        }
        if self.request.condensation.is_some() {
            result["schema"] = json!("ftx-textile-surface-water-result/3");
            result["condensed_liquid_kg"] = json!(condensed);
            result["condensation_latent_energy_j"] = json!(self.condensation_latent_j);
            result["incoming_vapor_sensible_energy_j"] = json!(incoming_sensible);
        }
        if full {
            result["request"] = json!(self.request);
            result["unit_pressure_networks"] = json!(self.networks);
            result["patches"] = json!((0..self.initial.len()).map(|i| json!({
                "source_triangle":i,"instance":self.instances[i],"initial_liquid_kg":self.initial[i],
                "remaining_liquid_kg":self.remaining[i],"evaporated_liquid_kg":self.evaporated[i]
            })).collect::<Vec<_>>());
            if self.request.supply.is_some() {
                for (i, patch) in result["patches"]
                    .as_array_mut()
                    .expect("patch array")
                    .iter_mut()
                    .enumerate()
                {
                    patch["supplied_liquid_kg"] = json!(self.supplied[i]);
                    patch["supplied_liquid_sensible_energy_j"] = json!(self.supplied_sensible_j[i]);
                }
            }
            if self.request.condensation.is_some() {
                for (i, patch) in result["patches"]
                    .as_array_mut()
                    .expect("patch array")
                    .iter_mut()
                    .enumerate()
                {
                    patch["condensed_liquid_kg"] = json!(self.condensed[i]);
                    patch["incoming_vapor_sensible_energy_j"] =
                        json!(self.incoming_vapor_sensible_j[i]);
                }
            }
            result["scope"] = json!(
                "Source-qualified initial free liquid shares the inner textile cell temperature. Frozen dry thermal laws and effective Ret are explicitly qualified over the authored temperature/loading domain. Dry storage integrates the selected constant or temperature-dependent specific heat against reference mass; remaining liquid adds its own sensible capacity. Pressure and vapor sensible export sample each substep's initial temperature; dry conduction is implicit. Earliest dryout splits the step and the dry remainder uses the same heat solver. Reference area owns water; current area conducts vapor. No replenishment, condensation, sorption, wicking, absorbed moisture, wet-property inference, added mechanical mass, body-water transfer or empirical/comfort claim. Time/space refinement remains required."
            );
            if self.request.supply.is_some() {
                result["scope"] = json!(
                    "Source-qualified free liquid shares the inner textile cell temperature. An independently sourced piecewise-constant delivery specifies reference-area mass rates and incoming liquid temperatures for every instance and interval. Supplied sensible enthalpy enters the same conservative thermal balance. Pressure and vapor sensible export sample old inner-cell temperature; implicit heat uses remaining liquid capacity. Net depletion splits at exact dryout; dry liquid supply can sustain limited evaporation or rewet the patch. Reference area owns liquid and delivery; current area conducts vapor. Qualified loading-domain overflow refuses without drainage or clamping. No condensation, sorption, wicking, absorbed moisture, wet-property inference, added mechanical mass, body-water transfer or empirical/comfort claim. Time/space refinement remains required."
                );
            }
            if self.request.condensation.is_some() {
                result["scope"] = json!(
                    "Source-qualified bidirectional free-film exchange uses the exact locked Ret network, with independent reverse-transport, wet/dry nucleation and effective enthalpy qualification. Negative pressure difference condenses vapor onto reference-owned liquid and releases latent heat; positive difference evaporates available liquid, including previously condensed water. Incoming vapor uses the explicit interval temperature and the same cp_liquid*(T-reference) effective sensible convention as outgoing vapor; constant latent heat is additional. It is not an inferred vapor heat capacity. Current area controls transfer; reference area owns water and delivery. The existing thermal solver conserves dry/liquid storage, signed boundary and mechanical heat, incoming liquid/vapor enthalpy and outgoing vapor enthalpy. Qualified loading/temperature overflow refuses transactionally without drainage or clamping. No sorption, wicking, absorbed moisture, wet-property inference, added mechanical mass, body-water transfer, whole-layer/wearer condensation or empirical/comfort claim. Time/space refinement remains required."
                );
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
