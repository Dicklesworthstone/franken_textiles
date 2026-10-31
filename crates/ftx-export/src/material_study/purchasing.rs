//! Complete actual fabric and thread orders share one market/history and distinct physical stock.
//! Existing procurement consumers retain pricing, rounding, capacity and source admission authority.

use std::collections::BTreeSet;

use ftx_core::Refusal;
use ftx_pattern::material_cost::{Basis, QuoteBook, Scope};
use serde::Serialize;
use serde_json::Value;

use super::{Alternative, Request, hash, invalid};
use crate::{
    cutting::procurement::{Market, stock_components},
    sewing::SewOptions,
};

/// Complete selected cut-stock and thread merchandise orders for one declared production batch.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-cut-and-sew-purchasing/1`.
    pub schema: &'static str,
    /// Exact common pattern used by both the complete cutting inventory and selected sewing plan.
    pub pattern_sha256: String,
    /// Same physical garment quantity on both sides of this composition.
    pub production_copies: usize,
    /// Comparable scope, without the fabric-only supplier-allocation algorithm.
    pub market: Market,
    /// Exact canonical quote history consumed by both sides, including unused bridging offers.
    pub quote_book_sha256: String,
    /// `cutting` or the complete mixed-stock `cutting_job` consumer.
    pub fabric_consumer: &'static str,
    /// Complete feasible fabric order cost, including a bounded supplier-allocation incumbent.
    pub fabric_selected_order_cost: f64,
    /// Complete observed yarn orders for the actual selected recipe and cell assignment.
    pub thread_selected_order_cost: f64,
    /// Sum of both complete contributions in the exact common market currency.
    pub selected_order_cost: f64,
    /// Existing fabric allocation proof for these fixed markers/items, not optimal nesting.
    pub fabric_order_allocation_optimality_proven: bool,
    /// Both fixed-demand order allocations are proved under their existing independent scopes.
    /// This does not prove a global design, recipe, cell assignment or supplier-bundle optimum.
    pub order_allocation_optimality_proven: bool,
    /// Every selected order has source-observed capacity for its complete rounded requirement.
    pub complete_observed_quantity_coverage: bool,
    /// Exact selected fabric offer identities; ordinary component reports retain full orders.
    pub fabric_offer_ids: Vec<String>,
    /// Exact selected yarn offer identities for this selected sewing plan.
    pub thread_offer_ids: Vec<String>,
    /// Actual current base/tier SKU declarations examined for cross-consumer inventory aliases.
    pub stock_aliases_examined: usize,
    /// No purchase, inventory reservation or machine action is authorized.
    pub execution_authorized: bool,
}

struct Fabric<'a> {
    pattern: &'a str,
    copies: usize,
    market: &'a Market,
    quotes: &'a QuoteBook,
    cost: f64,
    proved: bool,
    observed: bool,
    offers: Vec<String>,
    consumer: &'static str,
}

fn amount(value: Option<f64>) -> Result<f64, Refusal> {
    value
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or_else(|| invalid("cut-and-sew purchasing requires both complete finite order totals"))
}

fn offer_id(value: &Value) -> Result<String, Refusal> {
    value["quote"]["offer_id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("complete fabric order lost its selected source offer"))
}

fn roll_offers(value: &Value) -> Result<Vec<String>, Refusal> {
    let schema = value["schema"].as_str().unwrap_or_default();
    let rows = value[if schema == "ftx-cut-procurement/1" {
        "roll_prices"
    } else {
        "material_orders"
    }]
    .as_array()
    .ok_or_else(|| invalid("complete fabric purchasing lost its order inventory"))?;
    let mut result = Vec::new();
    for row in rows {
        if schema == "ftx-cut-procurement/3" {
            for order in row["allocation"]["selected_orders"]
                .as_array()
                .ok_or_else(|| invalid("fabric allocation has no complete selected orders"))?
            {
                result.push(offer_id(&order["offer"])?);
            }
        } else if matches!(schema, "ftx-cut-procurement/1" | "ftx-cut-procurement/2") {
            result.push(offer_id(&row["least_observed_order"])?);
        } else {
            return Err(invalid("unsupported fabric purchasing receipt"));
        }
    }
    Ok(result)
}

fn fabric<'a>(request: &'a Request, row: &'a Alternative) -> Result<Fabric<'a>, Refusal> {
    if request.cutting_job.is_some() {
        let plan = row
            .cutting_job
            .value()
            .filter(|plan| plan.complete_inventory)
            .ok_or_else(|| invalid("cut-and-sew purchasing needs a complete cutting job"))?;
        let priced = plan
            .purchasing
            .as_ref()
            .filter(|priced| priced.complete_offer_coverage)
            .ok_or_else(|| invalid("cut-and-sew purchasing needs complete cutting-job prices"))?;
        let mut offers = Vec::new();
        if plan.roll.is_some() {
            offers.extend(roll_offers(priced.roll.value().ok_or_else(|| {
                invalid("cutting-job roll purchasing is unavailable")
            })?)?);
        }
        if plan.finite_stock.is_some() {
            let finite = priced
                .finite_stock
                .value()
                .filter(|finite| finite.complete_offer_coverage)
                .ok_or_else(|| invalid("cutting-job finite-stock purchasing is unavailable"))?;
            for stock in finite.stocks.iter().filter(|stock| stock.identity.used) {
                offers.push(
                    stock.least_observed_offer_id.clone().ok_or_else(|| {
                        invalid("used finite stock has no complete selected offer")
                    })?,
                );
            }
        }
        return Ok(Fabric {
            pattern: &plan.pattern_sha256,
            copies: plan
                .inventory
                .iter()
                .map(|item| item.copy)
                .max()
                .and_then(|copy| copy.checked_add(1))
                .ok_or_else(|| invalid("complete cutting job has no garment copies"))?,
            market: &priced.request.market,
            quotes: &priced.request.quotes,
            cost: amount(priced.selected_observed_order_total)?,
            proved: priced.order_allocation_optimality_proven,
            observed: priced.complete_observed_quantity_coverage,
            offers,
            consumer: "cutting_job",
        });
    }
    if request.finite_stock.is_some() {
        return Err(invalid(
            "combine roll and finite stock through one complete cutting_job before cut-and-sew purchasing",
        ));
    }
    let input = request
        .purchasing
        .as_ref()
        .ok_or_else(|| invalid("cut-and-sew purchasing requires fabric purchasing"))?;
    let plan = row
        .cutting
        .value()
        .filter(|plan| plan.source_piece_ids.is_none())
        .ok_or_else(|| invalid("cut-and-sew purchasing needs a complete all-piece cut plan"))?;
    let priced = row
        .purchasing
        .value()
        .filter(|priced| priced["complete_offer_coverage"] == true)
        .ok_or_else(|| invalid("cut-and-sew purchasing needs complete fabric offer coverage"))?;
    let split = priced["schema"] == "ftx-cut-procurement/3";
    Ok(Fabric {
        pattern: &plan.pattern_sha256,
        copies: plan.options.copies,
        market: &input.market,
        quotes: &input.quotes,
        cost: amount(
            priced[if split {
                "selected_observed_order_total"
            } else {
                "least_observed_independent_order_total"
            }]
            .as_f64(),
        )?,
        proved: !split || priced["order_allocation_optimality_proven"] == true,
        observed: priced["complete_observed_quantity_coverage"] == true,
        offers: roll_offers(priced)?,
        consumer: "cutting",
    })
}

/// Conservative additional quote-history and alias work for one selected sewing plan.
pub(super) fn work_bound(options: &SewOptions) -> usize {
    options
        .consumables
        .as_ref()
        .and_then(|c| c.purchasing.as_ref())
        .map_or(0, |p| {
            p.quote_book.quotes.iter().fold(0usize, |sum, quote| {
                sum.saturating_add(3)
                    .saturating_add(quote.price_breaks.len())
            })
        })
}

pub(super) fn evaluate(request: &Request, row: &Alternative) -> Result<Report, Refusal> {
    let fabric = fabric(request, row)?;
    let sewing = row
        .sewing
        .value()
        .ok_or_else(|| invalid("cut-and-sew purchasing needs an admitted selected sewing plan"))?;
    let consumed = sewing
        .consumables
        .as_ref()
        .filter(|c| c.complete_consumption_coverage)
        .ok_or_else(|| {
            invalid("cut-and-sew purchasing needs complete selected-plan thread consumption")
        })?;
    let priced = consumed
        .purchasing
        .as_ref()
        .filter(|priced| priced.complete_offer_coverage)
        .ok_or_else(|| {
            invalid("cut-and-sew purchasing needs complete selected-plan thread orders")
        })?;
    let input = sewing
        .options
        .consumables
        .as_ref()
        .and_then(|c| c.purchasing.as_ref())
        .ok_or_else(|| invalid("selected thread orders lost their purchasing inputs"))?;
    if sewing.pattern_sha256 != fabric.pattern
        || consumed.production_copies.unwrap_or(1) != fabric.copies
    {
        return Err(invalid(
            "cut-and-sew purchasing requires the same pattern and production count",
        ));
    }
    let mut market = fabric.market.clone();
    // Supplier-allocation strategies are independent of the comparable market/source scope.
    market.split_orders = None;
    let mut thread_market = input.market.clone();
    thread_market.split_orders = None;
    if hash(&market)? != hash(&thread_market)? {
        return Err(invalid(
            "cut-and-sew purchasing requires the same comparable market, clock and capacity policies",
        ));
    }
    let quote_book_sha256 = hash(fabric.quotes)?;
    if quote_book_sha256 != hash(&input.quote_book)? {
        return Err(invalid(
            "cut-and-sew purchasing requires the same canonical immutable quote history",
        ));
    }
    let first = consumed
        .material_requirements
        .first()
        .ok_or_else(|| invalid("complete thread consumption has no exact yarn requirement"))?;
    let current = input.quote_book.latest_observations(&Scope {
        currency: market.currency.clone(),
        region: market.region.clone(),
        quality: market.quality.clone(),
        terms: market.terms.clone(),
        as_of: market.as_of.clone(),
        as_of_at: market.as_of_at.clone(),
        max_age_days: market.max_age_days,
        requirement: first.required_length_m,
        basis: Basis::Length,
        require_observed_quantity: market.require_observed_quantity,
        continuous_length_m: Some(first.required_continuous_length_m),
        require_observed_continuous_length: market.require_observed_continuous_length,
    })?;
    let (components, stock_aliases_examined) = stock_components(&current);
    let component = |id: &str| {
        components.get(id).copied().ok_or_else(|| {
            invalid("selected purchase is absent from the common current source history")
        })
    };
    let fabric_components = fabric
        .offers
        .iter()
        .map(|id| component(id))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let thread_offer_ids = priced
        .selected_offers()
        .map(|offer| offer.quote.offer_id.clone())
        .collect::<Vec<_>>();
    if thread_offer_ids.is_empty() {
        return Err(invalid(
            "complete thread order lost its selected source offers",
        ));
    }
    for id in &thread_offer_ids {
        if fabric_components.contains(&component(id)?) {
            return Err(invalid(
                "cut-stock and thread orders share a current supplier stock alias; their independent capacities cannot be combined",
            ));
        }
    }
    let thread_selected_order_cost = amount(priced.selected_total())?;
    let selected_order_cost = fabric.cost + thread_selected_order_cost;
    if !selected_order_cost.is_finite()
        || (fabric.cost > 0.0
            && thread_selected_order_cost > 0.0
            && (selected_order_cost <= fabric.cost
                || selected_order_cost <= thread_selected_order_cost))
    {
        return Err(invalid(
            "complete cut-and-sew cost overflows or loses a positive contribution",
        ));
    }
    Ok(Report {
        schema: "ftx-cut-and-sew-purchasing/1",
        pattern_sha256: fabric.pattern.into(),
        production_copies: fabric.copies,
        market,
        quote_book_sha256,
        fabric_consumer: fabric.consumer,
        fabric_selected_order_cost: fabric.cost,
        thread_selected_order_cost,
        selected_order_cost,
        fabric_order_allocation_optimality_proven: fabric.proved,
        order_allocation_optimality_proven: fabric.proved
            && priced.order_allocation_optimality_proven.unwrap_or(true),
        complete_observed_quantity_coverage: fabric.observed
            && priced.complete_observed_quantity_coverage,
        fabric_offer_ids: fabric.offers,
        thread_offer_ids,
        stock_aliases_examined,
        execution_authorized: false,
    })
}
