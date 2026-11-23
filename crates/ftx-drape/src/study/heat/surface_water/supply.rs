//! One finite liquid owner shared by every prescribed textile-delivery stream.
//! Delivery retains its original rate until the exact depletion event, then stops. Proposals
//! are immutable so dryout, vapor-pressure and thermal trials cannot consume source stock.

use super::{Evidence, Refusal, Source, add_positive, invalid, positive_sum};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

/// Independently qualified homogeneous liquid stock, withdrawn at one fixed temperature.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must be `ftx-textile-liquid-reservoir/1`.
    pub schema: String,
    /// Must be `well_mixed_constant_temperature_liquid_inventory/1`.
    pub model: String,
    /// Actual available stock, kg; zero explicitly supplies no liquid.
    pub initial_liquid_mass_kg: f64,
    /// Temperature of this homogeneous inventory and every supplied stream, degrees C.
    pub temperature_c: f64,
    /// Independent provenance for this stock and its thermal state.
    pub source: Source,
    /// Evidence independent of delivery rates and textile qualifications.
    pub evidence: Evidence,
    /// Explicit consent for estimated reservoir data only.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Source method defining available stock and constant-temperature withdrawal.
    pub method: String,
    /// Qualifies homogeneous withdrawal of the same liquid with no refill or unreported heat.
    pub qualification: String,
}

pub(super) fn non_null<'de, D>(deserializer: D) -> Result<Option<Request>, D::Error>
where
    D: Deserializer<'de>,
{
    Request::deserialize(deserializer).map(Some)
}

impl Request {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        let text = |s: &str| !s.trim().is_empty() && s.len() <= 4096;
        if self.schema != "ftx-textile-liquid-reservoir/1"
            || self.model != "well_mixed_constant_temperature_liquid_inventory/1"
            || !self.initial_liquid_mass_kg.is_finite()
            || self.initial_liquid_mass_kg < 0.
            || !self.temperature_c.is_finite()
            || self.temperature_c < -273.15
            || !text(&self.source.reference)
            || !text(&self.source.note)
            || self.source.accessed_on.len() != 10
            || !text(&self.method)
            || !text(&self.qualification)
            || (self.evidence == Evidence::Estimate && !self.allow_estimates)
        {
            return Err(invalid(
                "finite liquid supply needs independently admitted source evidence, nonnegative available stock and qualified constant-temperature withdrawal of the same liquid",
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct State {
    mass_kg: f64,
    requested_mass_kg: f64,
    unmet_mass_kg: f64,
    elapsed_seconds: f64,
    exhaustion_events: usize,
}

pub(super) struct Step {
    before: State,
    rates: Vec<f64>,
    requested_rate_kg_s: f64,
    maximum_seconds: f64,
    depletion_seconds: Option<f64>,
}

impl State {
    pub(super) fn new(request: &Request) -> Self {
        Self {
            mass_kg: request.initial_liquid_mass_kg,
            requested_mass_kg: 0.,
            unmet_mass_kg: 0.,
            elapsed_seconds: 0.,
            exhaustion_events: 0,
        }
    }

    pub(super) fn plan(&self, mut rates: Vec<f64>, maximum_seconds: f64) -> Result<Step, Refusal> {
        // The authored demand continues after stock exhaustion. Retain it before switching
        // actual delivery off; a dry donor must not make an unsatisfied operating case disappear.
        let requested_rate_kg_s = positive_sum(&rates)?;
        if self.mass_kg == 0. {
            rates.fill(0.);
        }
        let total_rate = if self.mass_kg == 0. {
            0.
        } else {
            requested_rate_kg_s
        };
        let depletion_seconds = if self.mass_kg > 0. && total_rate > 0. {
            let seconds = self.mass_kg / total_rate;
            if seconds <= 0. {
                return Err(invalid(
                    "finite liquid-source depletion time is unrepresentable",
                ));
            }
            // Division and prior accepted withdrawals can place the same physical event a
            // few ulps after a regular endpoint. Reconcile only that rounding neighborhood;
            // a genuinely later depletion retains its unused inventory.
            if seconds <= maximum_seconds {
                Some(seconds)
            } else if seconds.is_finite()
                && seconds - maximum_seconds <= 64. * f64::EPSILON * seconds.max(maximum_seconds)
            {
                Some(maximum_seconds)
            } else {
                None
            }
        } else {
            None
        };
        let maximum_seconds = depletion_seconds.unwrap_or(maximum_seconds);
        if !maximum_seconds.is_finite() || maximum_seconds <= 0. {
            return Err(invalid(
                "finite liquid delivery needs positive representable time",
            ));
        }
        Ok(Step {
            before: self.clone(),
            rates,
            requested_rate_kg_s,
            maximum_seconds,
            depletion_seconds,
        })
    }

    pub(super) fn snapshot(
        &self,
        request: &Request,
        supplied_mass_kg: f64,
        exported_enthalpy_j: f64,
        liquid_specific_heat: f64,
        enthalpy_reference_c: f64,
    ) -> Result<Value, Refusal> {
        let mass_error = (self.mass_kg - request.initial_liquid_mass_kg) + supplied_mass_kg;
        let mass_scale = self.mass_kg + request.initial_liquid_mass_kg + supplied_mass_kg;
        let demand_error = (supplied_mass_kg - self.requested_mass_kg) + self.unmet_mass_kg;
        let demand_scale = supplied_mass_kg + self.requested_mass_kg + self.unmet_mass_kg;
        let specific_enthalpy =
            liquid_specific_heat * (request.temperature_c - enthalpy_reference_c);
        let enthalpy_change = specific_enthalpy * (self.mass_kg - request.initial_liquid_mass_kg);
        let enthalpy_error = enthalpy_change + exported_enthalpy_j;
        let expected_export = specific_enthalpy * supplied_mass_kg;
        let energy_scale = 1. + enthalpy_change.abs() + exported_enthalpy_j.abs();
        if [
            mass_error,
            mass_scale,
            demand_error,
            demand_scale,
            specific_enthalpy,
            enthalpy_change,
            enthalpy_error,
            expected_export,
            energy_scale,
        ]
        .iter()
        .any(|value| !value.is_finite())
            || mass_error.abs() > 1024. * f64::EPSILON * mass_scale
            || demand_error.abs() > 1024. * f64::EPSILON * demand_scale
            || enthalpy_error.abs() > 1e-8 * energy_scale
            || (expected_export - exported_enthalpy_j).abs() > 1e-8 * energy_scale
        {
            return Err(invalid(
                "finite liquid-source mass, delivery demand or sensible-enthalpy ledger failed",
            ));
        }
        Ok(json!({
            "schema":"ftx-textile-liquid-reservoir-result/1",
            "initial_liquid_mass_kg":request.initial_liquid_mass_kg,
            "remaining_liquid_mass_kg":self.mass_kg,
            "supplied_liquid_mass_kg":supplied_mass_kg,
            "requested_liquid_mass_kg":self.requested_mass_kg,
            "unmet_liquid_mass_kg":self.unmet_mass_kg,
            "temperature_c":request.temperature_c,
            "elapsed_seconds":self.elapsed_seconds,
            "exhaustion_events":self.exhaustion_events,
            "liquid_enthalpy_change_j":enthalpy_change,
            "exported_liquid_enthalpy_j":exported_enthalpy_j,
            "mass_balance_error_kg":mass_error,
            "demand_balance_error_kg":demand_error,
            "enthalpy_balance_error_j":enthalpy_error
        }))
    }
}

impl Step {
    pub(super) fn maximum_seconds(&self) -> f64 {
        self.maximum_seconds
    }

    pub(super) fn rate(&self, patch: usize) -> f64 {
        self.rates[patch]
    }

    pub(super) fn amounts(&self, seconds: f64) -> Result<(Vec<f64>, State), Refusal> {
        if !seconds.is_finite() || seconds <= 0. || seconds > self.maximum_seconds {
            return Err(invalid(
                "finite liquid-source proposal uses an invalid duration",
            ));
        }
        let exhausted = self.depletion_seconds == Some(seconds);
        let mut amounts: Vec<_> = self.rates.iter().map(|rate| rate * seconds).collect();
        if exhausted {
            // Reconcile only event-rounding error, on the largest stream so a small legitimate
            // delivery cannot be erased. This is not proportional throttling over a whole step.
            let largest = self
                .rates
                .iter()
                .enumerate()
                .max_by(|(a_index, a), (b_index, b)| {
                    a.total_cmp(b).then_with(|| b_index.cmp(a_index))
                })
                .map(|(index, _)| index)
                .ok_or_else(|| invalid("finite liquid-source depletion has no delivery stream"))?;
            let others: Vec<_> = amounts
                .iter()
                .enumerate()
                .filter_map(|(index, &mass)| (index != largest).then_some(mass))
                .collect();
            let remainder = self.before.mass_kg - positive_sum(&others)?;
            let scale = self.before.mass_kg + amounts[largest];
            if !remainder.is_finite()
                || remainder <= 0.
                || !scale.is_finite()
                || (remainder - amounts[largest]).abs() > 1024. * f64::EPSILON * scale
            {
                return Err(invalid(
                    "finite liquid-source exhaustion loses a delivery stream",
                ));
            }
            amounts[largest] = remainder;
        }
        if self
            .rates
            .iter()
            .zip(&amounts)
            .any(|(&rate, &mass)| rate > 0. && mass <= 0.)
        {
            return Err(invalid("finite liquid delivery loses positive mass"));
        }
        let supplied = positive_sum(&amounts)?;
        let remaining = if exhausted {
            let scale = self.before.mass_kg + supplied;
            if !scale.is_finite()
                || (supplied - self.before.mass_kg).abs() > 1024. * f64::EPSILON * scale
            {
                return Err(invalid(
                    "finite liquid-source exhaustion does not conserve stock",
                ));
            }
            0.
        } else {
            self.before.mass_kg - supplied
        };
        let elapsed = self.before.elapsed_seconds + seconds;
        if !remaining.is_finite()
            || remaining < 0.
            || (supplied > 0. && remaining >= self.before.mass_kg)
            || !elapsed.is_finite()
            || elapsed <= self.before.elapsed_seconds
        {
            return Err(invalid(
                "finite liquid-source withdrawal overdraws or loses mass or time",
            ));
        }
        let requested = self.requested_rate_kg_s * seconds;
        if self.requested_rate_kg_s > 0. && requested <= 0. {
            return Err(invalid("finite liquid delivery loses positive demand"));
        }
        let mut requested_mass_kg = self.before.requested_mass_kg;
        add_positive(&mut requested_mass_kg, requested)?;
        let mut unmet_mass_kg = self.before.unmet_mass_kg;
        if self.before.mass_kg == 0. {
            // Exact exhaustion already splits the physical interval. Only the subsequent
            // empty-source portion is unmet; no subtraction of large near-equal totals is used.
            add_positive(&mut unmet_mass_kg, requested)?;
        }
        Ok((
            amounts,
            State {
                mass_kg: remaining,
                requested_mass_kg,
                unmet_mass_kg,
                elapsed_seconds: elapsed,
                exhaustion_events: self.before.exhaustion_events
                    + usize::from(self.before.mass_kg > 0. && remaining == 0.),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_supply_regular_endpoint_keeps_one_exhaustion_without_a_phantom_substep() {
        for mass in [0.1 * 0.1, (0.1_f64 * 0.1).next_up()] {
            let state = State {
                mass_kg: mass,
                requested_mass_kg: 0.,
                unmet_mass_kg: 0.,
                elapsed_seconds: 0.,
                exhaustion_events: 0,
            };
            let proposal = state.plan(vec![0.1], 0.1).unwrap();
            let (amounts, after) = proposal.amounts(proposal.maximum_seconds()).unwrap();
            assert_eq!(amounts, vec![mass]);
            assert_eq!(after.mass_kg, 0.);
            assert_eq!(after.requested_mass_kg, 0.1 * 0.1);
            assert_eq!(after.unmet_mass_kg, 0.);
            assert_eq!(after.exhaustion_events, 1);
            let empty = after.plan(vec![0.1], 0.1).unwrap();
            assert_eq!(empty.maximum_seconds(), 0.1);
            let (amounts, after) = empty.amounts(0.1).unwrap();
            assert_eq!(amounts, vec![0.]);
            assert_eq!(after.exhaustion_events, 1);
            assert_eq!(after.requested_mass_kg, 2. * (0.1 * 0.1));
            assert_eq!(after.unmet_mass_kg, 0.1 * 0.1);
        }
        let later = State {
            mass_kg: 0.01000001,
            requested_mass_kg: 0.,
            unmet_mass_kg: 0.,
            elapsed_seconds: 0.,
            exhaustion_events: 0,
        };
        let (amounts, after) = later.plan(vec![0.1], 0.1).unwrap().amounts(0.1).unwrap();
        assert_eq!(amounts, vec![0.1 * 0.1]);
        assert!(after.mass_kg > 0.);
        assert_eq!(after.exhaustion_events, 0);
        assert_eq!(after.requested_mass_kg, 0.1 * 0.1);
        assert_eq!(after.unmet_mass_kg, 0.);
    }

    #[test]
    fn finite_supply_demand_uses_only_accepted_time_and_retains_scheduled_shortfall() {
        let original = State {
            mass_kg: 0.125,
            requested_mass_kg: 0.,
            unmet_mass_kg: 0.,
            elapsed_seconds: 0.,
            exhaustion_events: 0,
        };
        let proposal = original.plan(vec![0.125, 0.375], 1.).unwrap();
        assert_eq!(proposal.maximum_seconds(), 0.25);
        // A discarded full proposal and a shorter accepted thermal interval use the same
        // unchanged donor. Neither pressure probes nor dryout trials spend future demand.
        let (_, discarded) = proposal.amounts(0.25).unwrap();
        assert_eq!(discarded.requested_mass_kg, 0.125);
        let (delivered, half) = proposal.amounts(0.125).unwrap();
        assert_eq!(delivered, vec![0.015625, 0.046875]);
        assert_eq!(half.requested_mass_kg, 0.0625);
        assert_eq!(half.unmet_mass_kg, 0.);
        assert_eq!(original.requested_mass_kg, 0.);
        let (_, depleted) = half
            .plan(vec![0.125, 0.375], 1.)
            .unwrap()
            .amounts(0.125)
            .unwrap();
        assert_eq!(depleted.mass_kg, 0.);
        assert_eq!(depleted.requested_mass_kg, 0.125);
        assert_eq!(depleted.unmet_mass_kg, 0.);
        let (_, idle) = depleted
            .plan(vec![0., 0.], 1.)
            .unwrap()
            .amounts(1.)
            .unwrap();
        assert_eq!(idle.requested_mass_kg, 0.125);
        assert_eq!(idle.unmet_mass_kg, 0.);
        let (delivered, final_state) = idle.plan(vec![0., 0.25], 2.).unwrap().amounts(2.).unwrap();
        assert_eq!(delivered, vec![0., 0.]);
        assert_eq!(final_state.requested_mass_kg, 0.625);
        assert_eq!(final_state.unmet_mass_kg, 0.5);
        assert_eq!(final_state.elapsed_seconds, 3.25);
        assert_eq!(final_state.exhaustion_events, 1);
    }

    #[test]
    fn empty_donor_preserves_tiny_shortfalls_and_refuses_unrepresentable_demand() {
        let original = State {
            mass_kg: 0.,
            requested_mass_kg: 1.,
            unmet_mass_kg: 0.,
            elapsed_seconds: 1.,
            exhaustion_events: 1,
        };
        let rate = 2.0_f64.powi(-40);
        let (_, after) = original.plan(vec![rate], 1.).unwrap().amounts(1.).unwrap();
        assert_eq!(after.requested_mass_kg, 1. + rate);
        assert_eq!(after.unmet_mass_kg, rate);
        assert!(
            original
                .plan(vec![f64::MAX], 2.)
                .unwrap()
                .amounts(2.)
                .is_err()
        );
        assert!(
            original
                .plan(vec![f64::from_bits(1)], 0.5)
                .unwrap()
                .amounts(0.5)
                .is_err()
        );
        assert_eq!(original.requested_mass_kg, 1.);
        assert_eq!(original.unmet_mass_kg, 0.);
    }
}
