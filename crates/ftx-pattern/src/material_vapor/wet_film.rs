//! Finite prescribed body liquid under independently qualified covered vapor demand.
//! Geometry consumers bind the ordered body identities and supply canonical per-body rates;
//! simulation and decision admission share this one water and latent-energy reducer.

use ftx_core::Refusal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::invalid;

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
    /// Validate prescribed inputs before any geometry work. Identity binding belongs to [State::new].
    ///
    /// # Errors
    /// Wrong schema, identity spelling, source text, array lengths or nonfinite/negative quantities.
    pub fn validate(&self, count: usize) -> Result<(), Refusal> {
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
        Ok(())
    }
}

fn add(total: &mut f64, value: f64) -> Result<(), Refusal> {
    let sum = *total + value;
    if !value.is_finite() || value < 0. || !sum.is_finite() || (value > 0. && sum <= *total) {
        return Err(invalid(
            "vapor coverage accumulation overflows or loses a positive term",
        ));
    }
    *total = sum;
    Ok(())
}

fn integral(rate: f64, duration: f64) -> Result<f64, Refusal> {
    let value = rate * duration;
    if !value.is_finite() || (rate > 0. && value <= 0.) {
        return Err(invalid(
            "vapor history interval integral overflows or loses positive transfer",
        ));
    }
    Ok(value)
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

/// Conservative retained-state accounting charge, separate from true geometry vertex counts.
#[must_use]
pub fn scalar_equivalents(body_count: usize) -> usize {
    body_count.saturating_mul(16)
}

/// Immutable-by-advance finite liquid state, owned by stable body-triangle identities.
#[derive(Clone)]
pub struct State {
    policy: WetFilm,
    latent_heat_j_kg: Vec<f64>,
    cumulative: Vec<Balance>,
}

impl State {
    /// Bind the reservoir to an independently verified initial body and explicit per-body latent heat.
    /// Geometry, material observations and boundary qualifications remain the caller's authority.
    ///
    /// # Errors
    /// Invalid prescribed inputs, mismatched body identity, invalid latent heat or aggregate overflow.
    pub fn new(
        policy: &WetFilm,
        body_mesh_sha256: &str,
        latent_heat_j_kg: &[f64],
    ) -> Result<Self, Refusal> {
        policy.validate(latent_heat_j_kg.len())?;
        if body_mesh_sha256 != policy.body_mesh_sha256 {
            return Err(invalid(
                "wet film body_mesh_sha256 does not match the exact initial body geometry and triangle identities",
            ));
        }
        if latent_heat_j_kg.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return Err(invalid(
                "wet film requires positive finite declared latent heat",
            ));
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
        receipt(&cumulative)?;
        Ok(Self {
            policy: policy.clone(),
            latent_heat_j_kg: latent_heat_j_kg.to_vec(),
            cumulative,
        })
    }

    /// Number of stable triangle reservoirs, independent of changing covered-region counts.
    #[must_use]
    pub fn body_triangle_count(&self) -> usize {
        self.cumulative.len()
    }

    /// Evolve canonical covered mass rates in kg/s for one interval, without mutating this state.
    /// `previous` selects composite trapezoid; its absence selects the current right endpoint.
    /// Quadrature-average demand and prescribed supply act constantly throughout the interval.
    ///
    /// # Errors
    /// Invalid counts, time or rates; overflow or lost positive transfers; failed conservation.
    pub fn advance(
        &self,
        current: &[f64],
        previous: Option<&[f64]>,
        seconds: f64,
    ) -> Result<(Self, Value), Refusal> {
        let count = self.cumulative.len();
        if current.len() != count
            || previous.is_some_and(|rates| rates.len() != count)
            || !seconds.is_finite()
            || seconds <= 0.
            || current
                .iter()
                .chain(previous.into_iter().flatten())
                .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err(invalid(
                "wet-film step needs positive finite time and one finite nonnegative covered rate per body triangle",
            ));
        }
        let mut next = self.clone();
        let mut steps = Vec::with_capacity(count);
        for (i, &rate) in current.iter().enumerate() {
            let demand = match previous {
                Some(previous) => {
                    let mut value = integral(previous[i], 0.5 * seconds)?;
                    add(&mut value, integral(rate, 0.5 * seconds)?)?;
                    value
                }
                None => integral(rate, seconds)?,
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

    /// Cumulative per-body and total balances, using time-zero liquid as the starting mass.
    ///
    /// # Errors
    /// Unrepresentable whole-domain sums or failed aggregate conservation.
    pub fn report(&self) -> Result<Value, Refusal> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> WetFilm {
        WetFilm {
            schema: "ftx-body-wet-film/1".into(),
            body_mesh_sha256: "1".repeat(64),
            source: "local:independent-two-reservoir-case".into(),
            basis: "Synthetic masses, rates and latent heats for exact conservation".into(),
            initial_liquid_mass_kg: vec![3., 0.],
            supply_rate_kg_s: vec![0.5, 1.],
        }
    }

    #[test]
    fn two_reservoirs_preserve_water_supply_and_distinct_latent_heat_through_quadrature() {
        let policy = policy();
        let initial = State::new(&policy, &policy.body_mesh_sha256, &[2., 5.]).unwrap();
        let initial_report = initial.report().unwrap();
        let (first, step) = initial.advance(&[4., 0.], None, 1.).unwrap();
        assert_eq!(initial.report().unwrap(), initial_report);
        assert_eq!(step["body"][0]["evaporated_mass_kg"], 3.5);
        assert_eq!(step["body"][1]["remaining_liquid_mass_kg"], 1.);
        let (second, step) = first.advance(&[0., 4.], Some(&[4., 0.]), 0.5).unwrap();
        assert_eq!(step["body"][0]["evaporated_mass_kg"], 0.25);
        assert_eq!(step["body"][1]["evaporated_mass_kg"], 1.);
        // Independent exact binary arithmetic: supplied 1.5 + 0.75 kg, evaporation
        // 3.5 + 0.25 + 1 kg, remaining 0.5 kg; energy 3.75*2 + 1*5 J.
        assert_eq!(
            second.report().unwrap()["totals"],
            json!({"starting_liquid_mass_kg":3.,"supplied_mass_kg":2.25,
                "potential_evaporation_mass_kg":6.,"evaporated_mass_kg":4.75,
                "unmet_demand_mass_kg":1.25,"remaining_liquid_mass_kg":0.5,
                "body_reservoir_latent_energy_j":12.5,
                "mass_balance_error_kg":0.,"demand_balance_error_kg":0.})
        );
    }

    #[test]
    fn shared_reducer_refuses_invalid_rates_and_binding_without_mutating_liquid() {
        let policy = policy();
        assert!(State::new(&policy, &"2".repeat(64), &[2., 5.]).is_err());
        assert!(State::new(&policy, &policy.body_mesh_sha256, &[2.]).is_err());
        let state = State::new(&policy, &policy.body_mesh_sha256, &[2., 5.]).unwrap();
        let before = state.report().unwrap();
        for (current, previous, duration) in [
            (vec![1.], None, 1.),
            (vec![-1., 0.], None, 1.),
            (vec![f64::NAN, 0.], None, 1.),
            (vec![1., 0.], Some(vec![0.]), 1.),
            (vec![1., 0.], Some(vec![f64::INFINITY, 0.]), 1.),
            (vec![1., 0.], None, 0.),
            (vec![1., 0.], None, -1.),
            (vec![f64::MAX, 0.], None, 2.),
        ] {
            assert!(
                state
                    .advance(&current, previous.as_deref(), duration)
                    .is_err()
            );
            assert_eq!(state.report().unwrap(), before);
        }
    }
}
