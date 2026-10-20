//! Price the exact physical items used by a checked finite-stock plan. Quote history, clocks,
//! currency and availability remain owned by material_cost, not a second purchasing engine.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::material_cost::{Basis, ExcludedOffer, Offer, Quote, QuoteBook, SaleUnit, Scope};
use serde::{Deserialize, Serialize};

use super::{super::stock::Plan, Market};

/// Explicit association of supplier offer histories with one physical stock snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// Physical stock ID, not the material ID or a generic supplier product category.
    pub stock_id: String,
    /// Hash of the frozen stock entry returned by [`identities`], including geometry and prior cuts.
    pub stock_sha256: String,
    /// Alternative offers for this same physical item. An offer cannot be bound to two items.
    pub offer_ids: Vec<String>,
}

/// Portable, clock-free purchasing inputs. Prices retain the existing quote-book schema.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// `ftx-finite-stock-pricing/1`.
    pub schema: String,
    /// Immutable observed prices with the existing calendar or intraday history policy.
    pub quotes: QuoteBook,
    /// Comparable currency, region, quality, terms and evaluation date/instant.
    pub market: Market,
    /// Explicit identity links. Missing links make used stock unpriced, never free.
    pub bindings: Vec<Binding>,
}

/// Identity to which an offer may be deliberately bound, derived from the actual complete plan.
#[derive(Clone, Debug, Serialize)]
pub struct Identity {
    /// Index in the plan's original inventory order.
    pub stock_index: usize,
    /// Unique physical inventory identity.
    pub stock_id: String,
    /// Frozen stock entry hash; includes material policy, geometry, source, zones and prior cuts.
    pub stock_sha256: String,
    /// Exact material snapshot, also required on each admitted source quote.
    pub material_sha256: String,
    /// Whole physical stock area including defects; not the sum of cut pieces or usable area.
    pub gross_area_m2: f64,
    /// Whether the complete plan places at least one piece on this stock item.
    pub used: bool,
}

/// Additional physical-item qualification failure for a latest source observation.
#[derive(Clone, Debug, Serialize)]
pub struct StockExclusion {
    /// The actual latest observation; older observations are never retried after this failure.
    pub quote: Quote,
    /// Why this observation cannot price the identified indivisible stock item.
    pub reason: String,
}

/// A source history whose canonical evaluation failed; independent alternatives remain available.
#[derive(Debug, Serialize)]
pub struct SourceRefusal {
    /// Offer history in the retained quote book.
    pub offer_id: String,
    /// Original quote-engine diagnostic, without replacing it with a guessed price.
    pub refusal: Refusal,
}

/// Purchasing result for a physical item, including independent missing-data diagnostics.
#[derive(Debug, Serialize)]
pub struct StockPrice {
    /// Identity and geometry basis from the checked allocation.
    pub identity: Identity,
    /// Why a used stock item could not be associated with its declared offer histories.
    pub binding_refusal: Option<Refusal>,
    /// Eligible whole-item observations in deterministic offer-ID order.
    pub offers: Vec<Offer>,
    /// Existing quote-engine availability, expiry, age, material and capacity exclusions.
    pub source_exclusions: Vec<ExcludedOffer>,
    /// Independent failed evaluations, for example a linear offer without a characterized width.
    pub source_refusals: Vec<SourceRefusal>,
    /// Additional indivisible-item and mandatory-lock failures.
    pub stock_exclusions: Vec<StockExclusion>,
    /// Chosen offer ID; equal costs retain the lexicographically first ID.
    pub least_observed_offer_id: Option<String>,
    /// Cheapest whole-item cost; unused and unpriced stock remain None, not zero-priced offers.
    pub least_observed_order_cost: Option<f64>,
}

/// Complete price coverage is independent of successful geometric allocation.
#[derive(Debug, Serialize)]
pub struct Report {
    /// `ftx-finite-stock-procurement/1`.
    pub schema: &'static str,
    /// Exact checked allocation that determined which items are required.
    pub plan_sha256: String,
    /// Hash of all dated pricing and binding inputs.
    pub pricing_sha256: String,
    /// Original pricing inputs for replay against the same complete plan.
    pub request: Request,
    /// Every stock item, including unused inventory, in the original order.
    pub stocks: Vec<StockPrice>,
    /// True only when every used stock item has an admitted whole-item offer.
    pub complete_offer_coverage: bool,
    /// True only when every selected offer also has sufficient observed capacity.
    pub complete_observed_quantity_coverage: bool,
    /// Common currency; no currency conversion is performed.
    pub currency: String,
    /// Sum over used physical items only; any unknown required price makes the sum unknown.
    pub least_observed_independent_order_total: Option<f64>,
    /// No inventory reservation, purchase or machine action is authorized by this report.
    pub execution_authorized: bool,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn digest(value: &impl Serialize) -> Result<String, Refusal> {
    Ok(sha256_hex(
        &serde_json::to_vec(value).map_err(|e| invalid(e.to_string()))?,
    ))
}

/// Produce offer-binding identities from the frozen inventory of a complete allocation.
///
/// # Errors
/// Serialization failure. No source inputs or inventory are modified.
pub fn identities(plan: &Plan) -> Result<Vec<Identity>, Refusal> {
    plan.request()
        .stocks
        .iter()
        .zip(plan.markers())
        .enumerate()
        .map(|(i, (stock, marker))| {
            Ok(Identity {
                stock_index: i,
                stock_id: stock.id.clone(),
                stock_sha256: digest(stock)?,
                material_sha256: marker.material.sha256().into(),
                gross_area_m2: marker.gross_area_m2,
                used: !marker.placements.is_empty(),
            })
        })
        .collect()
}

/// Evaluate exact whole-hide/remnant offers using the existing immutable quote authority.
/// Each admitted square-metre observation must represent precisely one whole item's area in its
/// quantity, minimum and increment. This is an explicit source normalization, never inferred yield.
///
/// # Errors
/// Invalid history/scope/bindings, overflow, or more than 128 bindings, 4096 links or 16384 quotes.
/// Ordinary missing prices and stale stock locks remain per-item incomplete outcomes.
pub fn evaluate(plan: &Plan, request: &Request) -> Result<Report, Refusal> {
    if request.schema != "ftx-finite-stock-pricing/1" {
        return Err(invalid("unsupported finite-stock pricing schema"));
    }
    if request.bindings.len() > 128
        || request.quotes.quotes.len() > 16384
        || request
            .bindings
            .iter()
            .try_fold(0usize, |n, b| n.checked_add(b.offer_ids.len()))
            .is_none_or(|n| n > 4096)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "finite-stock quote/link budget exceeded",
        ));
    }
    request.quotes.validate()?;
    let ids = identities(plan)?;
    let stock_ids: BTreeSet<_> = ids.iter().map(|i| i.stock_id.as_str()).collect();
    let quoted_ids: BTreeSet<_> = request
        .quotes
        .quotes
        .iter()
        .map(|q| q.offer_id.as_str())
        .collect();
    let mut bindings = BTreeMap::new();
    let mut used_offers = BTreeSet::new();
    for binding in &request.bindings {
        if !stock_ids.contains(binding.stock_id.as_str())
            || bindings
                .insert(binding.stock_id.as_str(), binding)
                .is_some()
            || binding.stock_sha256.len() != 64
            || !binding.stock_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || binding.offer_ids.is_empty()
        {
            return Err(invalid(
                "stock bindings need unique existing stock IDs, SHA-256 locks and offer IDs",
            ));
        }
        for id in &binding.offer_ids {
            if !quoted_ids.contains(id.as_str()) || !used_offers.insert(id.as_str()) {
                return Err(invalid(
                    "a bound offer must exist and may identify only one physical stock item",
                ));
            }
        }
    }
    let market = &request.market;
    let mut stocks = Vec::new();
    let (mut complete, mut observed, mut total) = (true, true, 0.0);
    for identity in ids {
        let marker = &plan.markers()[identity.stock_index];
        let scope = Scope {
            currency: market.currency.clone(),
            region: market.region.clone(),
            quality: market.quality.clone(),
            terms: market.terms.clone(),
            as_of: market.as_of.clone(),
            as_of_at: market.as_of_at.clone(),
            max_age_days: market.max_age_days,
            requirement: identity.gross_area_m2,
            basis: Basis::Area,
            require_observed_quantity: market.require_observed_quantity,
        };
        // Validate even an empty/unbound market through the same clock/scope authority.
        let empty = QuoteBook {
            schema: request.quotes.schema.clone(),
            quotes: Vec::new(),
        };
        empty.evaluate(&marker.material, &scope)?;
        let mut row = StockPrice {
            identity,
            binding_refusal: None,
            offers: Vec::new(),
            source_exclusions: Vec::new(),
            source_refusals: Vec::new(),
            stock_exclusions: Vec::new(),
            least_observed_offer_id: None,
            least_observed_order_cost: None,
        };
        if row.identity.used {
            match bindings.get(row.identity.stock_id.as_str()) {
                None => {
                    row.binding_refusal =
                        Some(invalid("no exact-stock offer binding; price is unknown"))
                }
                Some(binding)
                    if !binding
                        .stock_sha256
                        .eq_ignore_ascii_case(&row.identity.stock_sha256) =>
                {
                    row.binding_refusal = Some(invalid(
                        "stock snapshot changed; old offer binding cannot price this geometry/material/occupancy",
                    ));
                }
                Some(binding) => {
                    let offers: BTreeSet<_> =
                        binding.offer_ids.iter().map(String::as_str).collect();
                    for id in offers {
                        // Keep each history whole. The canonical evaluator picks its latest
                        // observation before admission, so a changed unit/lock never revives an
                        // older price. One failed history cannot erase an independent valid offer.
                        let book = QuoteBook {
                            schema: request.quotes.schema.clone(),
                            quotes: request
                                .quotes
                                .quotes
                                .iter()
                                .filter(|q| q.offer_id == id)
                                .cloned()
                                .collect(),
                        };
                        let sample = match book.evaluate(&marker.material, &scope) {
                            Ok(sample) => sample,
                            Err(refusal) => {
                                row.source_refusals.push(SourceRefusal {
                                    offer_id: id.into(),
                                    refusal,
                                });
                                continue;
                            }
                        };
                        row.source_exclusions.extend(sample.excluded_offers);
                        for offer in sample.offers {
                            let q = &offer.quote;
                            let area = row.identity.gross_area_m2;
                            let reason = if q.material_sha256.is_none() {
                                Some("whole-stock offers require an exact material SHA-256")
                            } else if q.unit != SaleUnit::SquareMeter
                                || q.quantity != area
                                || q.minimum_quantity != area
                                || q.order_increment != area
                                || q.available_quantity.is_some_and(|n| n > area)
                            {
                                Some(
                                    "quote must explicitly represent one indivisible stock item: square-metre quantity/minimum/increment equal its whole area and capacity no greater than one item",
                                )
                            } else {
                                None
                            };
                            if let Some(reason) = reason {
                                row.stock_exclusions.push(StockExclusion {
                                    quote: q.clone(),
                                    reason: reason.into(),
                                });
                                continue;
                            }
                            row.offers.push(offer);
                        }
                    }
                }
            }
            if let Some(offer) = row.offers.iter().min_by(|a, b| {
                a.order_cost
                    .total_cmp(&b.order_cost)
                    .then(a.quote.offer_id.cmp(&b.quote.offer_id))
            }) {
                row.least_observed_offer_id = Some(offer.quote.offer_id.clone());
                row.least_observed_order_cost = Some(offer.order_cost);
                observed &= offer.quantity_sufficiency_observed;
                total += offer.order_cost;
                if !total.is_finite() {
                    return Err(invalid("whole-stock order total overflows"));
                }
            } else {
                complete = false;
                observed = false;
            }
        }
        stocks.push(row);
    }
    Ok(Report {
        schema: "ftx-finite-stock-procurement/1",
        plan_sha256: digest(plan)?,
        pricing_sha256: digest(request)?,
        request: request.clone(),
        stocks,
        complete_offer_coverage: complete,
        complete_observed_quantity_coverage: observed,
        currency: market.currency.clone(),
        least_observed_independent_order_total: complete.then_some(total),
        execution_authorized: false,
    })
}
