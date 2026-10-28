//! Transfer the accepted thermal authority's exact liquid flows to the shared mechanical binding.
//! This adapter owns no mass allocator, constitutive law, momentum model or mechanical ledger.

use super::*;

/// Exact accepted thermal increments within the current mechanical interval. Subtracting large
/// cumulative counters can lose a short interval's flow or invent an excess at exact dryout.
#[derive(Clone)]
pub(super) struct IntervalFlow {
    pub(super) incoming: Vec<f64>,
    pub(super) outgoing: Vec<f64>,
}

impl IntervalFlow {
    pub(super) fn accept(&mut self, plan: &Plan) -> Result<(), Refusal> {
        if self.incoming.len() != plan.remaining.len()
            || self.outgoing.len() != plan.remaining.len()
        {
            return Err(invalid(
                "thermal and mechanical liquid-flow ownership differs",
            ));
        }
        for i in 0..self.incoming.len() {
            add_positive(&mut self.incoming[i], plan.supplied[i])?;
            add_positive(&mut self.incoming[i], plan.condensed[i])?;
            add_positive(&mut self.outgoing[i], plan.evaporated[i])?;
        }
        Ok(())
    }
}

impl Water {
    pub(in crate::study::heat) fn begin_mechanical_interval(&mut self) {
        self.mechanical_flow = Some(IntervalFlow {
            incoming: vec![0.; self.remaining.len()],
            outgoing: vec![0.; self.remaining.len()],
        });
    }

    pub(in crate::study::heat) fn accept_mechanical_exchange(
        &self,
        temperatures: &[Vec<f64>],
        solver: &mut crate::Solver,
    ) -> Result<crate::liquid_mass::Report, Refusal> {
        let flow = self
            .mechanical_flow
            .as_ref()
            .ok_or_else(|| invalid("missing accepted thermal interval flows"))?;
        solver.exchange_liquid(
            &self.remaining,
            &flow.incoming,
            &flow.outgoing,
            temperatures,
        )
    }
}
