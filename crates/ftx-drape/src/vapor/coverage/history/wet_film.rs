//! Explicit body-side liquid availability under the existing prescribed-reservoir vapor demand.
//! Water is owned by stable body triangles, never by changing projected regions or cloth layers.

use ftx_core::Refusal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{CoverageRequest, Integration, add, integral, invalid, number, trapezoid};

/// Authored liquid film and constant supply on the initial body triangle identities.
/// These are prescribed experimental inputs, not inferred sweat, textile moisture or physiology.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WetFilm {
    /// Must be `ftx-body-wet-film/1`.
    pub schema: String,
    /// Exact initial coverage body identity, including ordered positions and triangles.
    pub body_mesh_sha256: String,
    /// Source locator or experimental provenance for the declared liquid and supply.
    pub source: String,
    /// Physical basis and assumptions of these prescribed inputs, not a material-law claim.
    pub basis: String,
    /// Initial kilograms of liquid on each ordered body triangle; zero is explicit dry film.
    pub initial_liquid_mass_kg: Vec<f64>,
    /// Constant kilograms per second supplied to each ordered body triangle throughout motion.
    /// Supply to an uncovered triangle remains stored; no bare-body evaporation is inferred.
    pub supply_rate_kg_s: Vec<f64>,
}

impl WetFilm {
    fn validate(&self, initial: &CoverageRequest) -> Result<(), Refusal> {
        let count = initial.map.body_triangles.len();
        let text = |s: &str| (8..=4096).contains(&s.trim().len());
        if self.schema != "ftx-body-wet-film/1"
            || self.body_mesh_sha256.len() != 64
            || !self
                .body_mesh_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !text(&self.source)
            || !text(&self.basis)
            || !(1..=65_536).contains(&count)
            || self.initial_liquid_mass_kg.len() != count
            || self.supply_rate_kg_s.len() != count
            || self
                .initial_liquid_mass_kg
                .iter()
                .chain(&self.supply_rate_kg_s)
                .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err(invalid(
                "wet film needs its schema, a lowercase body SHA-256, source and basis of 8..4096 bytes, and one finite nonnegative initial mass and supply rate per body triangle",
            ));
        }
        if initial.validate_body_binding()? != self.body_mesh_sha256 {
            return Err(invalid(
                "wet film body_mesh_sha256 does not match the exact initial body geometry and triangle identities",
            ));
        }
        Ok(())
    }
}

/// Per-triangle and aggregate balances share arithmetic and field meaning. In a step, starting
/// means the previous accepted state; in the complete history it means the time-zero liquid.
#[derive(Clone, Debug, Default, Serialize)]
struct Balance {
    starting_liquid_mass_kg: f64,
    supplied_mass_kg: f64,
    potential_evaporation_mass_kg: f64,
    evaporated_mass_kg: f64,
    unmet_demand_mass_kg: f64,
    remaining_liquid_mass_kg: f64,
    body_reservoir_latent_energy_j: f64,
    mass_balance_error_kg: f64,
    demand_balance_error_kg: f64,
}

impl Balance {
    fn reconcile(&mut self) -> Result<(), Refusal> {
        self.mass_balance_error_kg = self.remaining_liquid_mass_kg - self.starting_liquid_mass_kg
            + self.evaporated_mass_kg
            - self.supplied_mass_kg;
        self.demand_balance_error_kg = self.evaporated_mass_kg + self.unmet_demand_mass_kg
            - self.potential_evaporation_mass_kg;
        let scale = self
            .starting_liquid_mass_kg
            .max(self.supplied_mass_kg)
            .max(self.potential_evaporation_mass_kg)
            .max(self.evaporated_mass_kg)
            .max(self.remaining_liquid_mass_kg);
        let tolerance = 256. * f64::EPSILON * scale;
        if [self.mass_balance_error_kg, self.demand_balance_error_kg]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > tolerance)
        {
            return Err(invalid("wet-film mass or demand conservation failed"));
        }
        Ok(())
    }

    fn accumulate(&mut self, step: &Self) -> Result<(), Refusal> {
        add(&mut self.supplied_mass_kg, step.supplied_mass_kg)?;
        add(
            &mut self.potential_evaporation_mass_kg,
            step.potential_evaporation_mass_kg,
        )?;
        add(&mut self.evaporated_mass_kg, step.evaporated_mass_kg)?;
        add(&mut self.unmet_demand_mass_kg, step.unmet_demand_mass_kg)?;
        add(
            &mut self.body_reservoir_latent_energy_j,
            step.body_reservoir_latent_energy_j,
        )?;
        self.remaining_liquid_mass_kg = step.remaining_liquid_mass_kg;
        self.reconcile()
    }
}

fn receipt(body: &[Balance]) -> Result<Value, Refusal> {
    let mut totals = Balance::default();
    let mut rows = Vec::with_capacity(body.len());
    for (index, balance) in body.iter().enumerate() {
        add(
            &mut totals.starting_liquid_mass_kg,
            balance.starting_liquid_mass_kg,
        )?;
        add(&mut totals.supplied_mass_kg, balance.supplied_mass_kg)?;
        add(
            &mut totals.potential_evaporation_mass_kg,
            balance.potential_evaporation_mass_kg,
        )?;
        add(&mut totals.evaporated_mass_kg, balance.evaporated_mass_kg)?;
        add(
            &mut totals.unmet_demand_mass_kg,
            balance.unmet_demand_mass_kg,
        )?;
        add(
            &mut totals.remaining_liquid_mass_kg,
            balance.remaining_liquid_mass_kg,
        )?;
        add(
            &mut totals.body_reservoir_latent_energy_j,
            balance.body_reservoir_latent_energy_j,
        )?;
        let mut row = serde_json::to_value(balance).map_err(|e| invalid(e.to_string()))?;
        row["body_triangle"] = json!(index);
        rows.push(row);
    }
    totals.reconcile()?;
    Ok(json!({"body":rows,"totals":totals}))
}

fn rates(report: &Value, count: usize) -> Result<Vec<f64>, Refusal> {
    let regions = report["coverage"]["regions"]
        .as_array()
        .ok_or_else(|| invalid("wet-film demand needs canonical covered regions"))?;
    let mut values = vec![0.; count];
    for region in regions {
        let body = region["body_triangle"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n < count)
            .ok_or_else(|| invalid("wet-film demand has an unknown body triangle"))?;
        add(
            &mut values[body],
            number(
                &region["vapor"]["potential_mass_flow_kg_s"],
                "covered-region mass flow",
            )?,
        )?;
    }
    Ok(values)
}

/// A conservative upper accounting charge per triangle and retained state. This keeps the
/// existing geometry limit meaningful without misreporting water scalars as geometry vertices.
pub(super) fn scalar_equivalents(body_count: usize) -> usize {
    body_count.saturating_mul(16)
}

#[derive(Clone)]
pub(super) struct State {
    policy: WetFilm,
    latent_heat_j_kg: Vec<f64>,
    cumulative: Vec<Balance>,
}

impl State {
    /// Validate the independent reservoir before doing any clipping. Boundary/body validation
    /// reuses the canonical request; any refusal remains local to the vapor history consumer.
    pub(super) fn new(policy: &WetFilm, initial: &CoverageRequest) -> Result<Self, Refusal> {
        policy.validate(initial)?;
        let mut latent_heat = Vec::with_capacity(initial.map.body_triangles.len());
        for i in 0..initial.map.body_triangles.len() {
            let index = if initial.conditions.boundaries.len() == 1 {
                0
            } else {
                i
            };
            let value = initial.conditions.boundaries[index].latent_heat_j_kg;
            if !value.is_finite() || value <= 0. {
                return Err(invalid(
                    "wet film requires positive finite declared latent heat",
                ));
            }
            latent_heat.push(value);
        }
        let cumulative: Vec<Balance> = policy
            .initial_liquid_mass_kg
            .iter()
            .map(|&mass| Balance {
                starting_liquid_mass_kg: mass,
                remaining_liquid_mass_kg: mass,
                ..Balance::default()
            })
            .collect();
        // Check whole-domain arithmetic even if no eventual covered demand is present.
        receipt(&cumulative)?;
        Ok(Self {
            policy: policy.clone(),
            latent_heat_j_kg: latent_heat,
            cumulative,
        })
    }

    /// The declared quadrature-average demand and constant supply act throughout this interval.
    /// This admits exact finite-reservoir depletion for that piecewise-constant approximation;
    /// it does not infer a hidden trajectory of coverage or pressure between samples.
    pub(super) fn advance(
        &self,
        previous: &Value,
        current: &Value,
        integration: Integration,
        seconds: f64,
    ) -> Result<(Self, Value), Refusal> {
        let count = self.cumulative.len();
        let current = rates(current, count)?;
        let previous = match integration {
            Integration::CompositeTrapezoid => Some(rates(previous, count)?),
            Integration::RightEndpoint => None,
        };
        let mut next = self.clone();
        let mut steps = Vec::with_capacity(count);
        for i in 0..count {
            let demand = match &previous {
                Some(previous) => trapezoid(previous[i], current[i], seconds)?,
                None => integral(current[i], seconds)?,
            };
            let supplied = integral(self.policy.supply_rate_kg_s[i], seconds)?;
            let starting = self.cumulative[i].remaining_liquid_mass_kg;
            let mut available = starting;
            add(&mut available, supplied)?;
            if starting > 0. && supplied > 0. && available <= supplied {
                return Err(invalid("wet-film supply loses positive starting liquid"));
            }
            let evaporated = demand.min(available);
            let remaining = available - evaporated;
            let unmet = demand - evaporated;
            if (evaporated > 0. && remaining >= available) || (evaporated > 0. && unmet >= demand) {
                return Err(invalid("wet-film subtraction loses positive evaporation"));
            }
            let mut step = Balance {
                starting_liquid_mass_kg: starting,
                supplied_mass_kg: supplied,
                potential_evaporation_mass_kg: demand,
                evaporated_mass_kg: evaporated,
                unmet_demand_mass_kg: unmet,
                remaining_liquid_mass_kg: remaining,
                body_reservoir_latent_energy_j: integral(evaporated, self.latent_heat_j_kg[i])?,
                mass_balance_error_kg: 0.,
                demand_balance_error_kg: 0.,
            };
            step.reconcile()?;
            next.cumulative[i].accumulate(&step)?;
            steps.push(step);
        }
        let mut result = receipt(&steps)?;
        result["schema"] = json!("ftx-body-wet-film-step/1");
        Ok((next, result))
    }

    pub(super) fn report(&self) -> Result<Value, Refusal> {
        let mut result = receipt(&self.cumulative)?;
        result["schema"] = json!("ftx-body-wet-film-report/1");
        result["body_mesh_sha256"] = json!(self.policy.body_mesh_sha256);
        result["source"] = json!(self.policy.source);
        result["basis"] = json!(self.policy.basis);
        result["textile_temperature_coupled"] = json!(false);
        result["uncertainty_propagated"] = json!(false);
        result["notes"] = json!([
            "Each body triangle owns its prescribed liquid and constant supply. Disjoint covered-region demands are summed before the declared interval quadrature; projected regions never own separate copies of the liquid.",
            "Quadrature-average demand and prescribed supply are constant over each interval. Evaporation is limited by starting liquid plus supplied liquid, with unmet covered demand retained. Uncovered-body transfer remains unknown and is excluded; this is not complete wearer water loss.",
            "Latent energy is supplied by the prescribed body reservoir using that triangle's fixed declared latent heat. It is not extracted from textile sensible storage and does not change cloth temperatures, mass, source Ret, sorption, geometry, thermoregulation or condensation.",
            "These conditional balances do not propagate Ret ranges into water-state uncertainty and do not establish physical garment performance."
        ]);
        Ok(result)
    }
}
