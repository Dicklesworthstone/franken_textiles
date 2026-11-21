//! One finite, isothermal vapor inventory shared by the existing textile-water plans.
//!
//! Adapted from FrankenSim `crates/fs-couple/src/moisture/atmosphere.rs`,
//! `FiniteMoistureAtmosphere::advance_fields`, and the compensated inventory sum in
//! `crates/fs-couple/src/moisture/reservoir.rs`, commit
//! d1aea97c310fa4b6767e975af8c126f450e9f71f. Copyright (c) 2026 Jeffrey Emanuel;
//! MIT License (with OpenAI/Anthropic Rider), retained in the repository LICENSE.
//!
//! The lifted mechanism retries every bounded scalar probe from the same accepted physical
//! owners, balances actual transfer increments, and publishes all owners together. Unlike the
//! source's fixed-duration diffusion solve, a textile probe may end at its earliest dryout.
//! Every patch and the reservoir use that ONE probe duration. Cloth temperatures remain the
//! accepted-start samples of the original film scheme; only ambient pressure is implicit.

use super::{Evidence, Plan, Refusal, Source, Water, add_positive, invalid, positive_sum};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

/// Independently sourced fixed-temperature vapor storage; no gas or textile law is inferred.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// `ftx-textile-vapor-reservoir/1` keeps an optional constant ambient boundary;
    /// version 2 requires one independently qualified boundary per mechanical interval.
    pub schema: String,
    /// Must be `isothermal_constant_vapor_capacitance/1`.
    pub model: String,
    /// Constant source-qualified dm_vapor/dp, kg/Pa, so m_vapor = C_vapor * p.
    pub vapor_capacitance_kg_pa: f64,
    /// Actual initial vapor inventory; zero is an explicitly empty atmosphere.
    pub initial_vapor_mass_kg: f64,
    /// Fixed temperature of incoming vapor and this reservoir's effective enthalpy, degrees C.
    pub temperature_c: f64,
    /// Closed source-qualified pressure interval in Pa, including the initial inventory state.
    pub pressure_domain_pa: [f64; 2],
    /// Optional independently qualified exchange with one constant ambient vapor reservoir.
    /// Its pressure law participates in the same implicit finite-inventory/dryout solve.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "non_null"
    )]
    pub ambient_exchange: Option<AmbientExchange>,
    /// Version 2 only: complete interval-aligned boundaries, including explicit zero conductance.
    /// Changes preserve the shared vapor, textile liquid and thermal state; no interpolation occurs.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "non_null"
    )]
    pub ambient_exchange_by_interval: Option<Vec<AmbientExchange>>,
    /// Bisection budget in 1..=128, in addition to initial and bracket-endpoint probes.
    pub max_iterations: usize,
    /// Absolute per-step and cumulative water-balance ceiling in kg. Inventory-scaled roundoff
    /// must also pass; a loose tolerance cannot authorize unsupported vapor or liquid mass.
    pub mass_tolerance_kg: f64,
    /// Provenance for the vapor storage response and isothermal boundary reduction.
    pub source: Source,
    /// Evidence independent of the textile and liquid-phase qualifications.
    pub evidence: Evidence,
    /// Explicit consent for estimated reservoir data only.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Source method defining capacitance, inventory and the fixed-temperature convention.
    pub method: String,
    /// Qualifies well-mixed isothermal storage, the pressure range and the existing constant-
    /// latent/cp_liquid effective vapor enthalpy convention. No fog or gas heat capacity is added.
    pub qualification: String,
}

/// A source-qualified pressure-driven vapor boundary, not an inferred airflow or gas law.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbientExchange {
    /// Must be `ftx-textile-vapor-exchange/1`.
    pub schema: String,
    /// Must be `linear_pressure_vapor_exchange/1`.
    pub model: String,
    /// Nonnegative vapor mass conductance in kg/(Pa·s); zero explicitly closes this boundary.
    pub conductance_kg_pa_s: f64,
    /// Constant independently prescribed ambient vapor pressure, Pa.
    pub ambient_vapor_pressure_pa: f64,
    /// Incoming vapor temperature, °C, under the same constant-latent/cp_liquid convention.
    /// Outgoing vapor carries the finite reservoir's independently declared temperature.
    pub ambient_temperature_c: f64,
    /// Qualified reservoir pressure interval, covering the complete finite reservoir domain.
    pub reservoir_pressure_domain_pa: [f64; 2],
    /// Independent source for the conductance and ambient boundary values.
    pub source: Source,
    /// Evidence for this exchange closure, independent of storage and textile transport.
    pub evidence: Evidence,
    /// Explicit consent for estimated exchange inputs only.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Method defining the constant ambient boundary and pressure-driven mass exchange.
    pub method: String,
    /// Qualifies the full pressure domain and incoming/outgoing effective vapor enthalpy.
    pub qualification: String,
}

// A supplied null cannot masquerade as an absent policy under an older schema.
fn non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl AmbientExchange {
    fn validate(&self, domain: [f64; 2]) -> Result<(), Refusal> {
        let text = |s: &str| !s.trim().is_empty() && s.len() <= 4096;
        let [lo, hi] = self.reservoir_pressure_domain_pa;
        if self.schema != "ftx-textile-vapor-exchange/1"
            || self.model != "linear_pressure_vapor_exchange/1"
            || !self.conductance_kg_pa_s.is_finite()
            || self.conductance_kg_pa_s < 0.
            || !self.ambient_vapor_pressure_pa.is_finite()
            || self.ambient_vapor_pressure_pa < 0.
            || !self.ambient_temperature_c.is_finite()
            || self.ambient_temperature_c <= -273.15
            || !lo.is_finite()
            || !hi.is_finite()
            || lo < 0.
            || lo >= hi
            || lo > domain[0]
            || hi < domain[1]
            || !text(&self.source.reference)
            || !text(&self.source.note)
            || self.source.accessed_on.len() != 10
            || !text(&self.method)
            || !text(&self.qualification)
            || (self.evidence == Evidence::Estimate && !self.allow_estimates)
        {
            return Err(invalid(
                "ambient vapor exchange needs independent source evidence, a nonnegative conductance, physical constant ambient conditions and qualification over the complete reservoir pressure domain",
            ));
        }
        Ok(())
    }

    fn increments(&self, pressure: f64, seconds: f64) -> Result<[f64; 5], Refusal> {
        let difference = self.ambient_vapor_pressure_pa - pressure;
        let inward = difference.max(0.) * seconds;
        let outward = (-difference).max(0.) * seconds;
        let pressure_time = pressure * seconds;
        let incoming = self.conductance_kg_pa_s * inward;
        let outgoing = self.conductance_kg_pa_s * outward;
        let values = [incoming, outgoing, pressure_time, inward, outward];
        if values.iter().any(|v| !v.is_finite() || *v < 0.)
            || (pressure > 0. && pressure_time <= 0.)
            || (difference != 0. && inward == 0. && outward == 0.)
            || (self.conductance_kg_pa_s > 0.
                && ((inward > 0. && incoming <= 0.) || (outward > 0. && outgoing <= 0.)))
        {
            return Err(invalid("ambient vapor exchange is unrepresentable"));
        }
        Ok(values)
    }
}

impl Request {
    pub(super) fn validate(&self, intervals: usize) -> Result<(), Refusal> {
        let text = |s: &str| !s.trim().is_empty() && s.len() <= 4096;
        let [lo, hi] = self.pressure_domain_pa;
        let pressure = self.initial_vapor_mass_kg / self.vapor_capacitance_kg_pa;
        let capacity_mass = hi * self.vapor_capacitance_kg_pa;
        let version_valid = match self.schema.as_str() {
            "ftx-textile-vapor-reservoir/1" => self.ambient_exchange_by_interval.is_none(),
            "ftx-textile-vapor-reservoir/2" => {
                self.ambient_exchange.is_none()
                    && self
                        .ambient_exchange_by_interval
                        .as_ref()
                        .is_some_and(|schedule| !schedule.is_empty() && schedule.len() == intervals)
            }
            _ => false,
        };
        if !version_valid
            || intervals == 0
            || self.model != "isothermal_constant_vapor_capacitance/1"
            || !self.vapor_capacitance_kg_pa.is_finite()
            || self.vapor_capacitance_kg_pa <= 0.
            || !self.initial_vapor_mass_kg.is_finite()
            || self.initial_vapor_mass_kg < 0.
            || !self.temperature_c.is_finite()
            || self.temperature_c <= -273.15
            || !lo.is_finite()
            || !hi.is_finite()
            || lo < 0.
            || lo >= hi
            || !pressure.is_finite()
            || pressure < lo
            || pressure > hi
            || (self.initial_vapor_mass_kg > 0. && pressure <= 0.)
            || !capacity_mass.is_finite()
            || capacity_mass <= 0.
            || !(1..=128).contains(&self.max_iterations)
            || !self.mass_tolerance_kg.is_finite()
            || self.mass_tolerance_kg <= 0.
            || !text(&self.source.reference)
            || !text(&self.source.note)
            || self.source.accessed_on.len() != 10
            || !text(&self.method)
            || !text(&self.qualification)
            || (self.evidence == Evidence::Estimate && !self.allow_estimates)
        {
            return Err(invalid(
                "finite vapor reservoir needs independently admitted source evidence, positive capacitance, actual nonnegative inventory, a supported pressure domain and bounded solve controls",
            ));
        }
        if let Some(exchange) = &self.ambient_exchange {
            exchange.validate(self.pressure_domain_pa)?;
        }
        for exchange in self.ambient_exchange_by_interval.iter().flatten() {
            exchange.validate(self.pressure_domain_pa)?;
        }
        Ok(())
    }

    fn exchange(&self, interval: usize) -> Result<Option<&AmbientExchange>, Refusal> {
        if let Some(schedule) = &self.ambient_exchange_by_interval {
            schedule.get(interval).map(Some).ok_or_else(|| {
                invalid("finite vapor ambient schedule has no boundary for this interval")
            })
        } else {
            Ok(self.ambient_exchange.as_ref())
        }
    }
}

/// Compensated signed increments retain opposing exchanges through one common inventory.
fn water_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let (mut sum, mut correction) = (0.0_f64, 0.0_f64);
    for value in values {
        let next = sum + value;
        correction += if sum.abs() >= value.abs() {
            (sum - next) + value
        } else {
            (value - next) + sum
        };
        sum = next;
    }
    sum + correction
}

fn exchange_snapshot(
    integrals: [f64; 5],
    incoming_enthalpy_j: f64,
    outgoing_enthalpy_j: f64,
) -> Result<Value, Refusal> {
    let [incoming, outgoing, pressure_time, inward, outward] = integrals;
    let net = water_sum([incoming, -outgoing]);
    let net_enthalpy_j = water_sum([incoming_enthalpy_j, -outgoing_enthalpy_j]);
    if integrals
        .iter()
        .any(|value| !value.is_finite() || *value < 0.)
        || [
            net,
            incoming_enthalpy_j,
            outgoing_enthalpy_j,
            net_enthalpy_j,
        ]
        .iter()
        .any(|value| !value.is_finite())
    {
        return Err(invalid("ambient vapor exchange receipt is unrepresentable"));
    }
    Ok(json!({
        "schema":"ftx-textile-vapor-exchange-result/1",
        "incoming_vapor_mass_kg":incoming,
        "outgoing_vapor_mass_kg":outgoing,
        "net_vapor_received_kg":net,
        "incoming_vapor_enthalpy_j":incoming_enthalpy_j,
        "outgoing_vapor_enthalpy_j":outgoing_enthalpy_j,
        "net_vapor_enthalpy_received_j":net_enthalpy_j,
        "reservoir_pressure_time_pa_s":pressure_time,
        "inward_pressure_time_pa_s":inward,
        "outward_pressure_time_pa_s":outward,
    }))
}

#[derive(Clone)]
pub(super) struct State {
    mass_kg: f64,
    elapsed_seconds: f64,
    pressure_trials: usize,
    maximum_step_mass_residual_kg: f64,
    // Actual accepted ambient mass and implicit endpoint-pressure integrals. Every failed
    // pressure probe is discarded; these counters share the accepted water/heat transaction.
    ambient_integrals: [f64; 5],
    // Version 2 retains each independently qualified temperature and pressure interval's share,
    // including zero future entries. Global accumulation keeps the constant-path arithmetic.
    ambient_by_interval: Vec<AmbientInterval>,
}

#[derive(Clone, Default)]
struct AmbientInterval {
    elapsed_seconds: f64,
    integrals: [f64; 5],
}

struct Candidate {
    plan: Plan,
    mass_kg: f64,
    residual_kg: f64,
    stored_pressure_residual_kg: f64,
    net_rate_kg_s: f64,
    accepted: bool,
    ambient_integrals: [f64; 5],
}

#[derive(Clone, Copy)]
struct Probe {
    pressure: f64,
    seconds: f64,
    residual: f64,
    net_rate: f64,
    dryout_slope_factor: f64,
    accepted: bool,
}

impl Probe {
    fn from_candidate(pressure: f64, candidate: &Candidate) -> Self {
        Self {
            pressure,
            seconds: candidate.plan.seconds,
            residual: candidate.residual_kg,
            net_rate: candidate.net_rate_kg_s,
            dryout_slope_factor: candidate.plan.dryout_time_slope_factor,
            accepted: candidate.accepted,
        }
    }
}

fn exhausted() -> Refusal {
    invalid(
        "finite vapor balance exhausted its pressure iteration or representable-event budget; no partial water or heat state published",
    )
}

// Increasing ambient pressure reduces every initial-wet depletion rate h_i with exact slope
// -k_i. Thus t(p)=min(max_dt,M_i/h_i(p)) is nondecreasing, with t' <= t_hi² max(k_i/M_i).
// Total signed vapor rate g(p) is nonincreasing, including dry supply/drain sharing and optional
// ambient conductance k*(p_ambient-p). That boundary adds -k to g' without changing dryout t'. Thus
// F'=C-g't-gt' is positive when C > max(g_lo,0)*t_hi²*max(k_i/M_i). Constant-duration
// intervals are also monotone. Search uncertified intervals from the accepted pressure outward;
// ordinary whole-domain bisection could otherwise skip an early dryout and choose a later root
// where the same cold patch has rewetted. Unresolved near-tangent events refuse within the budget.
fn first_evaporation_root(
    request: &Request,
    initial_pressure: f64,
    lower: Probe,
    upper: Probe,
    mut evaluate: impl FnMut(f64) -> Result<Probe, Refusal>,
) -> Result<f64, Refusal> {
    let mut intervals = vec![(lower, upper)];
    let mut iterations = 0usize;
    while let Some((mut lo, mut hi)) = intervals.pop() {
        if lo.accepted {
            return Ok(lo.pressure);
        }
        let adverse_slope = lo.net_rate.max(0.) * hi.seconds * hi.seconds * lo.dryout_slope_factor;
        let monotone = lo.seconds == hi.seconds
            || request.vapor_capacitance_kg_pa > adverse_slope * (1. + 2048. * f64::EPSILON);
        if monotone {
            if hi.accepted {
                return Ok(hi.pressure);
            }
            if hi.residual < 0. {
                continue;
            }
            if lo.residual > 0. {
                return Err(exhausted());
            }
            loop {
                let middle = lo.pressure + 0.5 * (hi.pressure - lo.pressure);
                if iterations == request.max_iterations
                    || middle == lo.pressure
                    || middle == hi.pressure
                {
                    return Err(exhausted());
                }
                let trial = evaluate(middle)?;
                iterations += 1;
                if trial.accepted {
                    return Ok(middle);
                }
                if trial.residual < 0. {
                    lo = trial;
                } else {
                    hi = trial;
                }
            }
        }
        let storage = request.vapor_capacitance_kg_pa * (hi.pressure - initial_pressure);
        let transfer = hi.net_rate
            * if hi.net_rate >= 0. {
                lo.seconds
            } else {
                hi.seconds
            };
        let upper_residual = storage - transfer;
        let rounding = 2048.
            * f64::EPSILON
            * (storage.abs() + transfer.abs() + request.vapor_capacitance_kg_pa * initial_pressure);
        if !upper_residual.is_finite() || !rounding.is_finite() {
            return Err(invalid("finite vapor event interval bound overflows"));
        }
        if upper_residual < -rounding {
            continue;
        }
        let middle = lo.pressure + 0.5 * (hi.pressure - lo.pressure);
        if iterations == request.max_iterations || middle == lo.pressure || middle == hi.pressure {
            return Err(exhausted());
        }
        let probe = evaluate(middle)?;
        iterations += 1;
        // Retain scalar certificates only, never a stack of full tentative material fields.
        intervals.push((probe, hi));
        intervals.push((lo, probe));
    }
    Err(invalid(
        "finite vapor balance is not bracketed inside the source-qualified pressure domain",
    ))
}

impl State {
    pub(super) fn new(request: &Request, intervals: usize) -> Result<Self, Refusal> {
        request.validate(intervals)?;
        Ok(Self {
            mass_kg: request.initial_vapor_mass_kg,
            elapsed_seconds: 0.,
            pressure_trials: 0,
            maximum_step_mass_residual_kg: 0.,
            ambient_integrals: [0.; 5],
            ambient_by_interval: request
                .ambient_exchange_by_interval
                .as_ref()
                .map_or_else(Vec::new, |schedule| {
                    vec![AmbientInterval::default(); schedule.len()]
                }),
        })
    }

    pub(super) fn retain_failed_trials(&mut self, trials: usize) -> Result<(), Refusal> {
        self.pressure_trials = self
            .pressure_trials
            .checked_add(trials)
            .ok_or_else(|| invalid("finite vapor pressure work overflows"))?;
        Ok(())
    }

    /// Every probe receives a pressure and returns the complete original water plan, including
    /// its common earliest-dryout duration. Neither the accepted water nor its heat state moves.
    pub(super) fn propose(
        &self,
        request: &Request,
        interval: usize,
        mut plan_at_pressure: impl FnMut(f64) -> Result<Plan, Refusal>,
        charge_trial: &mut impl FnMut() -> Result<(), Refusal>,
    ) -> Result<(Plan, Self), Refusal> {
        let initial_pressure = self.mass_kg / request.vapor_capacitance_kg_pa;
        let [mut lower, mut upper] = request.pressure_domain_pa;
        let exchange = request.exchange(interval)?;
        let mut trials = 0usize;
        let mut evaluate = |pressure: f64| -> Result<Candidate, Refusal> {
            charge_trial()?;
            trials += 1;
            let plan = plan_at_pressure(pressure)?;
            let evaporated = positive_sum(&plan.evaporated)?;
            let condensed = positive_sum(&plan.condensed)?;
            let textile_net = water_sum(
                plan.evaporated
                    .iter()
                    .zip(&plan.condensed)
                    .flat_map(|(&e, &c)| [e, -c]),
            );
            let ambient_integrals = exchange
                .map(|exchange| exchange.increments(pressure, plan.seconds))
                .transpose()?
                .unwrap_or([0.; 5]);
            let [incoming, outgoing, ..] = ambient_integrals;
            let net = water_sum([textile_net, incoming, -outgoing]);
            // Preserve a balanced accepted start exactly instead of rounding it through C*p.
            let trial_mass = if pressure.to_bits() == initial_pressure.to_bits() {
                self.mass_kg
            } else {
                request.vapor_capacitance_kg_pa * pressure
            };
            let residual = water_sum([trial_mass, -self.mass_kg, -net]);
            // The scalar root is an approximate pressure closure, not a source or sink of
            // physical water. Commit the ACTUAL phase transfers; otherwise a same-sign root
            // defect accumulates artificial loss and makes thermostat heat reference-dependent.
            let mass = water_sum([self.mass_kg, net]);
            let stored_pressure_residual = trial_mass - mass;
            let net_rate = net / plan.seconds;
            let available = self.mass_kg + evaporated + incoming;
            let removed = condensed + outgoing;
            let scale = self.mass_kg + trial_mass + evaporated + condensed + incoming + outgoing;
            // Drive the scalar closure to arithmetic roundoff. The wider independent report
            // admission ceiling is not a stopping target: a small vapor capacitance amplifies
            // an otherwise admissible absolute mass defect into a visible pressure error.
            let tolerance = request.mass_tolerance_kg.min(32. * f64::EPSILON * scale);
            if [
                mass,
                trial_mass,
                net,
                net_rate,
                residual,
                stored_pressure_residual,
                available,
                removed,
                scale,
                tolerance,
            ]
            .iter()
            .any(|v| !v.is_finite())
                || trial_mass < 0.
                || (pressure > 0. && trial_mass <= 0.)
            {
                return Err(invalid(
                    "finite vapor inventory or balance is unrepresentable",
                ));
            }
            // A numerical mass tolerance never purchases vapor for condensation. Nonzero net
            // transport must also survive in the stored reservoir, even beside a large stock.
            let represented =
                net == 0. || (net > 0. && mass > self.mass_kg) || (net < 0. && mass < self.mass_kg);
            Ok(Candidate {
                plan,
                mass_kg: mass,
                residual_kg: residual,
                stored_pressure_residual_kg: stored_pressure_residual,
                net_rate_kg_s: net_rate,
                accepted: residual.abs().max(stored_pressure_residual.abs()) <= tolerance
                    && mass >= 0.
                    && (pressure == 0. || mass > 0.)
                    && removed <= available
                    && mass <= available
                    && represented,
                ambient_integrals,
            })
        };
        let initial = evaluate(initial_pressure)?;
        let accepted = if initial.accepted {
            initial
        } else if initial.residual_kg < 0. {
            let start = Probe::from_candidate(initial_pressure, &initial);
            drop(initial);
            let high = evaluate(upper)?;
            let end = Probe::from_candidate(upper, &high);
            drop(high);
            let pressure = first_evaporation_root(request, initial_pressure, start, end, |p| {
                let candidate = evaluate(p)?;
                Ok(Probe::from_candidate(p, &candidate))
            })?;
            // Repeat only the selected pure probe to retain its one complete material plan.
            evaluate(pressure)?
        } else {
            drop(initial);
            // Net condensation's admissible root lies below the accepted pressure. Unrelated
            // high-pressure states cannot improve this monotone bracket.
            upper = initial_pressure;
            let low = evaluate(lower)?;
            let high = evaluate(upper)?;
            if low.accepted {
                low
            } else if high.accepted {
                high
            } else {
                if low.residual_kg.is_sign_positive() == high.residual_kg.is_sign_positive() {
                    return Err(invalid(
                        "finite vapor balance is not bracketed inside the source-qualified pressure domain",
                    ));
                }
                let mut low_residual = low.residual_kg;
                drop(low);
                drop(high);
                let mut iterations = 0usize;
                loop {
                    let middle = lower + 0.5 * (upper - lower);
                    let trial = evaluate(middle)?;
                    iterations += 1;
                    if trial.accepted {
                        break trial;
                    }
                    if iterations == request.max_iterations || middle == lower || middle == upper {
                        return Err(invalid(
                            "finite vapor balance exhausted its pressure iteration or representable-inventory budget; no partial water or heat state published",
                        ));
                    }
                    if trial.residual_kg.is_sign_positive() == low_residual.is_sign_positive() {
                        lower = middle;
                        low_residual = trial.residual_kg;
                    } else {
                        upper = middle;
                    }
                }
            }
        };
        let elapsed_seconds = self.elapsed_seconds + accepted.plan.seconds;
        let pressure = accepted.mass_kg / request.vapor_capacitance_kg_pa;
        if !elapsed_seconds.is_finite()
            || elapsed_seconds <= self.elapsed_seconds
            || !pressure.is_finite()
            || (accepted.mass_kg > 0. && pressure <= 0.)
            || pressure < request.pressure_domain_pa[0]
            || pressure > request.pressure_domain_pa[1]
        {
            return Err(invalid(
                "finite vapor accepted time or pressure leaves its domain",
            ));
        }
        let pressure_trials = self
            .pressure_trials
            .checked_add(trials)
            .ok_or_else(|| invalid("finite vapor pressure work overflows"))?;
        let mut ambient_integrals = self.ambient_integrals;
        for (total, increment) in ambient_integrals.iter_mut().zip(accepted.ambient_integrals) {
            add_positive(total, increment)?;
        }
        let mut ambient_by_interval = self.ambient_by_interval.clone();
        if request.ambient_exchange_by_interval.is_some() {
            let active = ambient_by_interval
                .get_mut(interval)
                .ok_or_else(|| invalid("finite vapor ambient state has no matching interval"))?;
            add_positive(&mut active.elapsed_seconds, accepted.plan.seconds)?;
            for (total, increment) in active.integrals.iter_mut().zip(accepted.ambient_integrals) {
                add_positive(total, increment)?;
            }
        }
        Ok((
            accepted.plan,
            Self {
                mass_kg: accepted.mass_kg,
                elapsed_seconds,
                pressure_trials,
                maximum_step_mass_residual_kg: self
                    .maximum_step_mass_residual_kg
                    .max(accepted.residual_kg.abs())
                    .max(accepted.stored_pressure_residual_kg.abs()),
                ambient_integrals,
                ambient_by_interval,
            },
        ))
    }

    pub(super) fn snapshot(&self, request: &Request, water: &Water) -> Result<Value, Refusal> {
        let evaporated = positive_sum(&water.evaporated)?;
        let condensed = positive_sum(&water.condensed)?;
        let received = water_sum(
            water
                .evaporated
                .iter()
                .zip(&water.condensed)
                .flat_map(|(&e, &c)| [e, -c]),
        );
        let [ambient_in, ambient_out, ..] = self.ambient_integrals;
        let ambient_net = water_sum([ambient_in, -ambient_out]);
        let mass_error = water_sum([
            self.mass_kg,
            -request.initial_vapor_mass_kg,
            -received,
            -ambient_net,
        ]);
        let mass_scale = self.mass_kg
            + request.initial_vapor_mass_kg
            + evaporated
            + condensed
            + ambient_in
            + ambient_out;
        let mass_tolerance = request
            .mass_tolerance_kg
            .min(4096. * f64::EPSILON * mass_scale);
        let effective_enthalpy = water.request.latent_heat_j_kg
            + water.request.liquid_specific_heat_j_kg_k
                * (request.temperature_c - water.request.enthalpy_reference_c);
        let enthalpy_change = effective_enthalpy * (self.mass_kg - request.initial_vapor_mass_kg);
        let incoming_sensible = water_sum(water.incoming_vapor_sensible_j.iter().copied());
        let enthalpy_received = water_sum([
            water.latent_j,
            water.exported_sensible_j,
            -water.condensation_latent_j,
            -incoming_sensible,
        ]);
        let incoming_enthalpy = |mass: f64, exchange: &AmbientExchange| {
            mass * (water.request.latent_heat_j_kg
                + water.request.liquid_specific_heat_j_kg_k
                    * (exchange.ambient_temperature_c - water.request.enthalpy_reference_c))
        };
        let mut interval_receipts = Vec::new();
        let ambient_in_j = if let Some(schedule) = &request.ambient_exchange_by_interval {
            if self.ambient_by_interval.len() != schedule.len() {
                return Err(invalid(
                    "finite vapor ambient state lost its source interval mapping",
                ));
            }
            let mut incoming_energies = Vec::with_capacity(schedule.len());
            for (interval, (exchange, accepted)) in
                schedule.iter().zip(&self.ambient_by_interval).enumerate()
            {
                let incoming_j = incoming_enthalpy(accepted.integrals[0], exchange);
                let outgoing_j = accepted.integrals[1] * effective_enthalpy;
                let mut receipt = exchange_snapshot(accepted.integrals, incoming_j, outgoing_j)?;
                receipt["interval"] = json!(interval);
                receipt["elapsed_seconds"] = json!(accepted.elapsed_seconds);
                interval_receipts.push(receipt);
                incoming_energies.push(incoming_j);
            }
            water_sum(incoming_energies)
        } else {
            request
                .ambient_exchange
                .as_ref()
                .map_or(0., |exchange| incoming_enthalpy(ambient_in, exchange))
        };
        let ambient_out_j = ambient_out * effective_enthalpy;
        let ambient_received_j = water_sum([ambient_in_j, -ambient_out_j]);
        // Positive heat is removed by the explicitly isothermal reservoir's thermostat. This
        // closes the declared effective-enthalpy model; it is not an inferred gas heat capacity.
        let isothermal_heat_removed = enthalpy_received + ambient_received_j - enthalpy_change;
        let enthalpy_error = water_sum([
            enthalpy_change,
            isothermal_heat_removed,
            -enthalpy_received,
            -ambient_received_j,
        ]);
        let energy_scale = enthalpy_change.abs()
            + isothermal_heat_removed.abs()
            + enthalpy_received.abs()
            + ambient_in_j.abs()
            + ambient_out_j.abs();
        let pressure = self.mass_kg / request.vapor_capacitance_kg_pa;
        if [
            mass_error,
            mass_scale,
            mass_tolerance,
            effective_enthalpy,
            enthalpy_change,
            enthalpy_received,
            isothermal_heat_removed,
            enthalpy_error,
            ambient_in_j,
            ambient_out_j,
            ambient_received_j,
            energy_scale,
            pressure,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || mass_error.abs() > mass_tolerance
            || enthalpy_error.abs() > 1024. * f64::EPSILON * energy_scale
            || (self.mass_kg > 0. && pressure <= 0.)
            || pressure < request.pressure_domain_pa[0]
            || pressure > request.pressure_domain_pa[1]
        {
            return Err(invalid(
                "finite vapor cumulative mass or enthalpy balance failed",
            ));
        }
        let mut result = json!({
            "schema":if request.ambient_exchange_by_interval.is_some() {
                "ftx-textile-vapor-reservoir-result/2"
            } else {
                "ftx-textile-vapor-reservoir-result/1"
            },
            "initial_vapor_mass_kg":request.initial_vapor_mass_kg,
            "remaining_vapor_mass_kg":self.mass_kg,
            "initial_vapor_pressure_pa":request.initial_vapor_mass_kg/request.vapor_capacitance_kg_pa,
            "vapor_pressure_pa":pressure,
            "net_vapor_received_kg":received,
            "mass_balance_error_kg":mass_error,
            "vapor_enthalpy_change_j":enthalpy_change,
            "net_vapor_enthalpy_received_j":enthalpy_received,
            "isothermal_heat_removed_j":isothermal_heat_removed,
            "enthalpy_balance_error_j":enthalpy_error,
            "elapsed_seconds":self.elapsed_seconds,
            "pressure_trials":self.pressure_trials,
            "maximum_step_mass_residual_kg":self.maximum_step_mass_residual_kg
        });
        if request.ambient_exchange.is_some() || request.ambient_exchange_by_interval.is_some() {
            let mut exchange =
                exchange_snapshot(self.ambient_integrals, ambient_in_j, ambient_out_j)?;
            if request.ambient_exchange_by_interval.is_some() {
                exchange["schema"] = json!("ftx-textile-vapor-exchange-result/2");
                exchange["intervals"] = json!(interval_receipts);
            }
            result["ambient_exchange"] = exchange;
        }
        Ok(result)
    }
}
