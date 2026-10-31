//! Consume one complete canonical purchasing score; no independent quantity or price arithmetic.

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_cost::{Basis, Scope};

use crate::sewing::{SewOptions, SewPlan, consumables::Purchasing};

pub(super) fn purchasing(options: &SewOptions) -> Result<&Purchasing, Refusal> {
    options
        .consumables
        .as_ref()
        .and_then(|request| request.purchasing.as_ref())
        .ok_or_else(|| {
            super::invalid(
                "thread-cost selection requires explicit consumables.purchasing with its quote book and comparable market",
            )
        })
}

pub(super) fn validate(options: &SewOptions) -> Result<(), Refusal> {
    let purchasing = purchasing(options)?;
    let market = &purchasing.market;
    // The canonical market selector validates the book, source instants and comparable market.
    // It does not consume requirement/continuity quantities or calculate a price. The positive
    // placeholder is solely a well-formed Scope; each actual plan supplies its own real demand.
    purchasing.quote_book.latest_observations(&Scope {
        currency: market.currency.clone(),
        region: market.region.clone(),
        quality: market.quality.clone(),
        terms: market.terms.clone(),
        as_of: market.as_of.clone(),
        as_of_at: market.as_of_at.clone(),
        max_age_days: market.max_age_days,
        requirement: 1.0,
        basis: Basis::Length,
        require_observed_quantity: market.require_observed_quantity,
        continuous_length_m: None,
        require_observed_continuous_length: market.require_observed_continuous_length,
    })?;
    Ok(())
}

fn unavailable(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::NotConverged, message)
        .repair("supply complete independently qualified thread consumption and eligible exact-yarn orders in the declared market")
}

pub(super) fn score(plan: &SewPlan, currency: &str) -> Result<(f64, Option<bool>), Refusal> {
    let consumption = plan
        .consumables
        .as_ref()
        .ok_or_else(|| unavailable("admitted sewing plan has no thread consumption report"))?;
    if !consumption.complete_consumption_coverage || consumption.total_required_length_m.is_none() {
        return Err(unavailable(
            "thread order cost is unknown because an admitted operation has missing consumption evidence or incomplete channel coverage",
        ));
    }
    let purchasing = consumption
        .purchasing
        .as_ref()
        .ok_or_else(|| unavailable("admitted sewing plan has no thread purchasing report"))?;
    if purchasing.currency != currency {
        return Err(unavailable(
            "thread purchasing currency differs from the declared common recipe market",
        ));
    }
    if !purchasing.complete_offer_coverage {
        return Err(unavailable(
            purchasing
                .unavailable_reason
                .as_deref()
                .unwrap_or("complete eligible thread order coverage is unavailable"),
        ));
    }
    purchasing
        .selected_total()
        .filter(|cost| cost.is_finite() && *cost >= 0.0)
        .map(|cost| (cost, purchasing.order_allocation_optimality_proven))
        .ok_or_else(|| unavailable("complete canonical thread order total is unavailable"))
}
