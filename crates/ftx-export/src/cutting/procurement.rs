//! A computed marker's source-locked stock requirements feed the existing dated quote evaluator.
//! Pure library boundary shared by CLI and potential browser consumers; no I/O or checkout action.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_cost::{Basis, Quote, QuoteBook, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::CutPlan;

/// Bounded assignment of whole markers to separately observed supplier orders.
pub mod allocation;
/// Source-locked purchasing of individually identified hides and remnants, not fungible roll area.
pub mod finite;

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
    /// Required for version-2 quotes: UTC evaluation instant, with the same date as `as_of`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of_at: Option<String>,
    /// Maximum permissible observation age in calendar days.
    pub max_age_days: u32,
    /// Require source-observed capacity for each complete minimum/increment-rounded order.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub require_observed_quantity: bool,
    /// Require a source-stated maximum uninterrupted cut for every actual roll marker.
    /// This is independent of total stock and does not establish guaranteed continuous inventory.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub require_observed_continuous_length: bool,
    /// Optional indivisible-demand allocation across exact-source-locked supplier offers. Cutting
    /// uses the marker schema; thread purchasing uses its operation/channel/copy segment schema.
    /// Omission keeps the original one-offer-per-material calculation and its wire format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_orders: Option<allocation::Policy>,
}

// Every current base/tier SKU of one supplier is an inventory alias. Canonical market/as-of
// selection includes unused bridging offers, without importing future or superseded identities.
pub(crate) fn stock_components(quotes: &[&Quote]) -> (BTreeMap<String, usize>, usize) {
    fn root(parents: &mut [usize], index: usize) -> usize {
        let mut representative = index;
        while parents[representative] != representative {
            representative = parents[representative];
        }
        let mut next = index;
        while parents[next] != next {
            let previous = parents[next];
            parents[next] = representative;
            next = previous;
        }
        representative
    }
    let mut parents: Vec<_> = (0..quotes.len()).collect();
    let mut owner = BTreeMap::new();
    let mut count = 0;
    for (index, quote) in quotes.iter().enumerate() {
        for sku in std::iter::once(&quote.sku)
            .chain(quote.price_breaks.iter().filter_map(|p| p.sku.as_ref()))
        {
            count += 1;
            if let Some(previous) = owner.insert((&quote.supplier, sku), index) {
                let left = root(&mut parents, index);
                let right = root(&mut parents, previous);
                parents[left] = right;
            }
        }
    }
    (
        quotes
            .iter()
            .enumerate()
            .map(|(i, q)| (q.offer_id.clone(), root(&mut parents, i)))
            .collect(),
        count,
    )
}

/// Price each exact material's gross stock area in one whole order, including all of its markers.
/// An empty offer sample for ANY material makes the total unknown, never zero or a partial total.
/// This composes the existing `QuoteBook` policies; it does not reimplement quote/currency logic.
///
/// # Errors
/// Invalid quote book/market, empty marker, incompatible width evidence or numerical overflow.
pub fn evaluate(plan: &CutPlan, book: &QuoteBook, market: &Market) -> Result<Value, Refusal> {
    if let Some(policy) = &market.split_orders {
        return allocation::evaluate(plan, book, market, policy);
    }
    if plan.rolls.is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "cannot price an empty cut plan",
        ));
    }
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, roll) in plan.rolls.iter().enumerate() {
        if !roll.length_m.is_finite()
            || roll.length_m <= 0.0
            || !roll.stock_area_m2.is_finite()
            || roll.stock_area_m2 <= 0.0
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "cannot price nonpositive or nonfinite marker requirements",
            ));
        }
        groups
            .entry(roll.material.sha256())
            .or_default()
            .push(index);
    }
    let multiple = groups.values().any(|indices| indices.len() > 1);
    let mut prepared = Vec::with_capacity(groups.len());
    for indices in groups.values() {
        let roll = &plan.rolls[indices[0]];
        let mut area = 0.0;
        let mut length = 0.0;
        let mut longest = 0.0_f64;
        for &index in indices {
            let marker = &plan.rolls[index];
            if marker.width_m != roll.width_m {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "markers of one material snapshot must retain the same stock width",
                ));
            }
            area += marker.stock_area_m2;
            length += marker.length_m;
            longest = longest.max(marker.length_m);
        }
        if !area.is_finite() || !length.is_finite() {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "whole-material marker requirements overflow",
            ));
        }
        let scope = Scope {
            currency: market.currency.clone(),
            region: market.region.clone(),
            quality: market.quality.clone(),
            terms: market.terms.clone(),
            as_of: market.as_of.clone(),
            as_of_at: market.as_of_at.clone(),
            max_age_days: market.max_age_days,
            requirement: area,
            basis: Basis::Area,
            require_observed_quantity: market.require_observed_quantity,
            continuous_length_m: Some(longest),
            require_observed_continuous_length: market.require_observed_continuous_length,
        };
        let sample = book.evaluate(&roll.material, &scope)?;
        prepared.push((indices, area, length, longest, sample));
    }
    let current = book.latest_observations(&prepared[0].4.scope)?;
    let (components, _) = stock_components(&current);
    let mut owners: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
    for (_, _, _, _, sample) in &prepared {
        for offer in &sample.offers {
            owners
                .entry(components[&offer.quote.offer_id])
                .or_default()
                .insert(&sample.material_sha256);
        }
    }
    let shared: BTreeSet<_> = owners
        .iter()
        .filter_map(|(&id, hashes)| (hashes.len() > 1).then_some(id))
        .collect();
    drop(owners);
    let mut priced = Vec::new();
    let mut orders = Vec::new();
    let mut complete = true;
    let mut observed_quantity = true;
    let mut continuous_limit = true;
    let mut total = 0.0;
    for (indices, area, length, longest, sample) in prepared {
        let roll = &plan.rolls[indices[0]];
        // Separate snapshot orders cannot each consume the same physical stock. Keep canonical
        // samples intact, but exclude ambiguous components before selecting independent minima.
        let excluded: Vec<_> = sample
            .offers
            .iter()
            .filter(|offer| shared.contains(&components[&offer.quote.offer_id]))
            .map(|offer| &offer.quote.offer_id)
            .collect();
        let cheapest = sample
            .offers
            .iter()
            .filter(|offer| !shared.contains(&components[&offer.quote.offer_id]))
            .min_by(|a, b| {
                a.order_cost
                    .total_cmp(&b.order_cost)
                    .then_with(|| a.quote.offer_id.cmp(&b.quote.offer_id))
            });
        if let Some(offer) = cheapest {
            observed_quantity &= offer.quantity_sufficiency_observed;
            continuous_limit &= offer.within_observed_continuous_limit == Some(true);
            total += offer.order_cost;
            if !total.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    "independent order cost sum overflows",
                ));
            }
        } else {
            complete = false;
            observed_quantity = false;
            continuous_limit = false;
        }
        let continuity = if cheapest.is_none() {
            "no_eligible_offer"
        } else if cheapest.is_some_and(|offer| offer.within_observed_continuous_limit == Some(true))
        {
            "within_observed_limit"
        } else {
            "source_limit_unknown"
        };
        if multiple {
            let order_index = orders.len();
            for &index in indices {
                let marker = &plan.rolls[index];
                // Costs belong to the whole order. Repeating them in marker rows would silently
                // double-count minimum quantities, increments and the same finite source stock.
                priced.push(json!({"roll_index":index,"material_sha256":roll.material.sha256(),
                    "required_length_m":marker.length_m,"required_stock_area_m2":marker.stock_area_m2,
                    "continuity":continuity,"material_order_index":order_index}));
            }
            let mut order = json!({"material_order_index":order_index,"material_sha256":roll.material.sha256(),
                "roll_indices":indices,"required_length_m":length,"required_stock_area_m2":area,
                "required_continuous_length_m":longest,
                "marker_lengths_m":indices.iter().map(|&i| plan.rolls[i].length_m).collect::<Vec<_>>(),
                "continuity":continuity,"least_observed_order":cheapest,"sample":sample});
            if !excluded.is_empty() {
                order["excluded_shared_stock_offer_ids"] = json!(excluded);
            }
            orders.push(order);
        } else {
            let mut order = json!({"roll_index":indices[0],"material_sha256":roll.material.sha256(),
            "required_length_m":roll.length_m,"required_stock_area_m2":roll.stock_area_m2,
            "continuity":continuity,"least_observed_order":cheapest,"sample":sample});
            if !excluded.is_empty() {
                order["excluded_shared_stock_offer_ids"] = json!(excluded);
            }
            priced.push(order);
        }
    }
    priced.sort_by_key(|row| row["roll_index"].as_u64());
    let mut result = json!({"schema":if multiple {"ftx-cut-procurement/2"} else {"ftx-cut-procurement/1"},"cut_plan":plan,"roll_prices":priced,
        "complete_offer_coverage":complete,"currency":market.currency,
        "complete_observed_quantity_coverage":observed_quantity,
        "complete_continuous_length_limit_coverage":continuous_limit,
        "least_observed_independent_order_total":if complete {Some(total)} else {None},
        "limits":["Each material snapshot is ordered independently; no supplier bundle/shipping optimization.",
            "Offers are dated observations, not reserved stock or checkout guarantees.",
            "An offer without available_quantity has unknown capacity; require_observed_quantity excludes it when quantified stock is required.",
            "Each actual marker needs one uninterrupted full-width segment; no marker or cutter is split to fit a supplier limit.",
            "A source maximum continuous cut is an upper limit, not guaranteed continuous inventory; unknown limits stay explicit unless require_observed_continuous_length excludes them.",
            "The total is the sum of observed minimum unambiguous whole-order costs, not a market percentile or an optimal nesting claim.",
            "Current supplier/base/tier SKU aliases share one physical stock identity. Components offered to distinct required material snapshots are excluded from independent orders, including links through unused current offers; unresolved stock never supplies a complete total.",
            "Different material snapshots are not pooled even when their product IDs coincide."]});
    if multiple {
        result["material_orders"] = json!(orders);
        result["limits"].as_array_mut().unwrap().push(json!(
            "Markers of the same exact snapshot share one whole order: minimum, increment and observed total stock are checked once against their summed gross area. The longest actual marker determines the continuous-cut upper-limit check; individual pieces are never divided."
        ));
        result["limits"].as_array_mut().unwrap().push(json!(
            "The quote's minimum and increment describe the whole order, not a per-cut charging rule. Marker lengths are a requested segmentation, not evidence that a supplier can or will deliver those cuts."
        ));
    }
    Ok(result)
}
