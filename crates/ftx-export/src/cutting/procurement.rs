//! A computed marker's source-locked stock requirements feed the existing dated quote evaluator.
//! Pure library boundary shared by CLI and potential browser consumers; no I/O or checkout action.

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_cost::{Basis, QuoteBook, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::CutPlan;

/// Comparable market scope; purchase quantities are derived from the marker rather than guessed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Market {
    /// Currency, three uppercase letters. No exchange rate is inferred.
    pub currency: String,
    /// Exact regional market label.
    pub region: String,
    /// Exact user-defined quality/condition stratum.
    pub quality: String,
    /// Exact tax/shipping/promotion cost basis.
    pub terms: String,
    /// Reproducible as-of date, YYYY-MM-DD, not the wall clock.
    pub as_of: String,
    /// Maximum permissible observation age in calendar days.
    pub max_age_days: u32,
}

/// Price each marker's gross stock area, then sum the least observed independent whole-order costs.
/// An empty offer sample for ANY material makes the total unknown, never zero or a partial total.
/// This composes the existing `QuoteBook` policies; it does not reimplement quote/currency logic.
///
/// # Errors
/// Invalid quote book/market, empty marker, incompatible width evidence or numerical overflow.
pub fn evaluate(plan: &CutPlan, book: &QuoteBook, market: &Market) -> Result<Value, Refusal> {
    if plan.rolls.is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "cannot price an empty cut plan",
        ));
    }
    let mut priced = Vec::new();
    let mut complete = true;
    let mut total = 0.0;
    for (index, roll) in plan.rolls.iter().enumerate() {
        let scope = Scope {
            currency: market.currency.clone(),
            region: market.region.clone(),
            quality: market.quality.clone(),
            terms: market.terms.clone(),
            as_of: market.as_of.clone(),
            max_age_days: market.max_age_days,
            requirement: roll.stock_area_m2,
            basis: Basis::Area,
        };
        let sample = book.evaluate(&roll.material, &scope)?;
        let cheapest = sample.offers.iter().min_by(|a, b| {
            a.order_cost
                .total_cmp(&b.order_cost)
                .then_with(|| a.quote.offer_id.cmp(&b.quote.offer_id))
        });
        if let Some(offer) = cheapest {
            total += offer.order_cost;
            if !total.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    "independent order cost sum overflows",
                ));
            }
        } else {
            complete = false;
        }
        priced.push(
            json!({"roll_index":index,"material_sha256":roll.material.sha256(),
            "required_length_m":roll.length_m,"required_stock_area_m2":roll.stock_area_m2,
            "least_observed_order":cheapest,"sample":sample}),
        );
    }
    Ok(
        json!({"schema":"ftx-cut-procurement/1","cut_plan":plan,"roll_prices":priced,
        "complete_offer_coverage":complete,"currency":market.currency,
        "least_observed_independent_order_total":if complete {Some(total)} else {None},
        "limits":["Each material snapshot is ordered independently; no supplier bundle/shipping optimization.",
            "Offers are dated observations, not reserved stock or checkout guarantees.",
            "The total is the sum of observed minimum whole-order costs, not a market percentile or an optimal nesting claim.",
            "Different material snapshots are not pooled even when their product IDs coincide."]}),
    )
}
