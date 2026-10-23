//! Initial free liquid owned by stable textile triangles, with explicit temperature feedback.
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

/// Source-locked initial loading on every retained instance, including explicitly dry instances.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// Exact normalized source material identity; a replacement requires new qualification.
    pub material_sha256: String,
    /// Initial free liquid per REFERENCE material area, kg/m². It adds no mechanical mass.
    pub initial_liquid_kg_m2: f64,
}

/// Explicit initial-only free-film closure for the material-following thermal history.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-textile-surface-water/1`.
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
    /// Must be exactly zero. No supplied mass or supply enthalpy is omitted from this model.
    pub replenishment_kg_m2_s: f64,
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
        if self.schema != "ftx-textile-surface-water/1"
            || self.model != "inner_cell_equilibrium_free_film_frozen_properties/1"
            || self.bindings.is_empty()
            || self.bindings.len() > 65_536
            || self.replenishment_kg_m2_s != 0.
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
                "surface water needs its explicit initial-only model, locked bindings, positive SI inputs, monotone pressure table, source-qualified domain and independent estimate consent",
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
    initial_temperature_c: f64,
    latent_j: f64,
    exported_sensible_j: f64,
    liquid_stored_j: f64,
    dryout_events: usize,
}

pub(super) struct Plan {
    pub seconds: f64,
    remaining: Vec<f64>,
    evaporated: Vec<f64>,
    pub capacity_j_k: Vec<f64>,
    pub latent_j: Vec<f64>,
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
        model.require_constant_liquid_model()?;
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
            initial,
            initial_temperature_c: initial_c,
            latent_j: 0.,
            exported_sensible_j: 0.,
            liquid_stored_j: 0.,
            dryout_events: 0,
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
            let rate = if self.remaining[i] == 0. {
                0.
            } else {
                let inside = self.request.pressure(old.temperature_c()[i][0])?;
                let flux = self.networks[patch.column].flux_at_pressures(inside, outside)
                    .map_err(|_| invalid("surface-water wet pressure is below ambient (condensation unsupported), or vapor flux is unrepresentable"))?;
                let p = patch.vertices.map(|v| positions.get(v).copied());
                let [Some(a), Some(b), Some(c)] = p else {
                    return Err(invalid("missing liquid surface geometry"));
                };
                let area = 0.5 * (b - a).cross(c - a).norm();
                let rate = flux.mass_kg_m2_s * area;
                if !area.is_finite()
                    || area <= 0.
                    || !rate.is_finite()
                    || (flux.mass_kg_m2_s > 0. && rate <= 0.)
                {
                    return Err(invalid(
                        "surface-water current area or vapor rate is unrepresentable",
                    ));
                }
                rate
            };
            let dry_time = if rate == 0. {
                f64::INFINITY
            } else {
                self.remaining[i] / rate
            };
            seconds = seconds.min(dry_time);
            rates.push(rate);
            dry_times.push(dry_time);
        }
        if !seconds.is_finite() || seconds <= 0. {
            return Err(invalid("surface-water dryout time is not representable"));
        }
        let mut plan = Plan {
            seconds,
            remaining: Vec::new(),
            evaporated: Vec::new(),
            capacity_j_k: Vec::new(),
            latent_j: Vec::new(),
            exported_sensible_j: 0.,
            dryout_events: 0,
        };
        for (i, rate) in rates.into_iter().enumerate() {
            let evaporated = if dry_times[i] == seconds {
                self.remaining[i]
            } else {
                rate * seconds
            };
            let remaining = self.remaining[i] - evaporated;
            let latent = evaporated * self.request.latent_heat_j_kg;
            let capacity = remaining * self.request.liquid_specific_heat_j_kg_k;
            if !remaining.is_finite()
                || remaining < 0.
                || !latent.is_finite()
                || !capacity.is_finite()
                || (rate > 0.
                    && (evaporated <= 0. || remaining >= self.remaining[i] || latent <= 0.))
                || (remaining > 0. && capacity <= 0.)
            {
                return Err(invalid(
                    "surface-water mass, latent energy or capacity loses a positive term",
                ));
            }
            plan.dryout_events += usize::from(self.remaining[i] > 0. && remaining == 0.);
            plan.remaining.push(remaining);
            plan.evaporated.push(evaporated);
            plan.capacity_j_k.push(capacity);
            plan.latent_j.push(latent);
            // Outflow samples the OLD temperature, exactly as the explicit pressure/rate law.
            plan.exported_sensible_j += evaporated
                * self.request.liquid_specific_heat_j_kg_k
                * (old.temperature_c()[i][0] - self.request.enthalpy_reference_c);
        }
        if !plan.exported_sensible_j.is_finite() {
            return Err(invalid("exported vapor sensible enthalpy overflows"));
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
        for i in 0..self.remaining.len() {
            for &t in &next.state.temperature_c()[i] {
                self.request.pressure(t)?;
            }
            if plan.remaining[i] > 0.
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
        // supplied_heat is mechanical input MINUS latent removal. The vapor sensible export and
        // liquid storage give the remaining-capacity equation used by the shared thermal solver.
        let error = next.stored_change_j + liquid_step + plan.exported_sensible_j
            - plan.seconds * (next.inside_heat_w - next.outside_heat_w)
            - next.supplied_heat_j;
        let scale = 1.
            + next.stored_change_j.abs()
            + liquid_step.abs()
            + plan.exported_sensible_j.abs()
            + next.supplied_heat_j.abs();
        if !error.is_finite() || !scale.is_finite() || error.abs() > 1e-8 * scale {
            return Err(invalid(
                "surface-water sensible/latent energy balance failed",
            ));
        }
        add_positive(&mut self.latent_j, latent_step)?;
        self.exported_sensible_j += plan.exported_sensible_j;
        self.liquid_stored_j += liquid_step;
        self.dryout_events += plan.dryout_events;
        for (total, increment) in self.evaporated.iter_mut().zip(plan.evaporated) {
            add_positive(total, increment)?;
        }
        self.remaining = plan.remaining;
        next.balance_error_j = error;
        self.snapshot(next.state.temperature_c(), false, 0.)?;
        Ok(())
    }

    pub(super) fn energy_correction_j(&self) -> f64 {
        self.liquid_stored_j + self.exported_sensible_j + self.latent_j
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
        let error = remaining + evaporated - initial;
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
            error,
            reconstructed,
            self.latent_j,
            self.exported_sensible_j,
            self.liquid_stored_j,
            balance_error_j,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || error.abs() > 1024. * f64::EPSILON * initial
            || (reconstructed - self.liquid_stored_j).abs()
                > 1e-8 * (1. + reconstructed.abs() + self.liquid_stored_j.abs())
            || (self.latent_j - evaporated * self.request.latent_heat_j_kg).abs()
                > 1e-8 * (1. + self.latent_j)
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
        if full {
            result["request"] = json!(self.request);
            result["unit_pressure_networks"] = json!(self.networks);
            result["patches"] = json!((0..self.initial.len()).map(|i| json!({
                "source_triangle":i,"instance":self.instances[i],"initial_liquid_kg":self.initial[i],
                "remaining_liquid_kg":self.remaining[i],"evaporated_liquid_kg":self.evaporated[i]
            })).collect::<Vec<_>>());
            result["scope"] = json!(
                "Source-qualified initial free liquid shares the inner textile cell temperature. Constant dry thermal and effective Ret properties are explicitly qualified over the authored temperature/loading domain. Pressure and vapor sensible export sample each substep's initial temperature; dry conduction is implicit. Earliest dryout splits the step and the dry remainder uses the same heat solver. Reference area owns water; current area conducts vapor. No replenishment, condensation, sorption, wicking, absorbed moisture, wet-property inference, added mechanical mass, body-water transfer or empirical/comfort claim. Time/space refinement remains required."
            );
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
