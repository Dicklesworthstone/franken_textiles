//! Indivisible-demand supplier allocation. QuoteBook remains the sole price, source-history,
//! capacity and sale-unit authority; roll markers and thread segments share this bounded search.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_catalog::ResolvedMaterial;
use ftx_pattern::material_cost::{
    Basis, ExclusionReason, Offer, PriceSample, Quote, QuoteBook, Scope,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{CutPlan, Market};

const MAX_MARKERS: usize = 32;
const MAX_MATERIAL_MARKERS: usize = 12;
pub(crate) const MAX_THREAD_SEGMENTS: usize = 256;
const MAX_YARN_SEGMENTS: usize = 63;
const MAX_OFFERS: usize = 8;
const MAX_QUOTES: usize = 4096;
const MAX_WORK: usize = 200_000;

/// Explicit finite search; no marker, continuous thread segment or supplier stock is divided.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// `ftx-cut-order-allocation/1` for markers or `ftx-thread-order-allocation/1` for thread.
    pub schema: String,
    /// Shared limit across all materials, 1..200000. Each attempted assignment and each uncached
    /// canonical quote evaluation consumes one unit; cached subsets do not re-evaluate quotes.
    pub max_search_nodes: usize,
}

/// One complete observed order and the indivisible markers assigned to it.
#[derive(Clone, Debug, Serialize)]
pub struct Order {
    /// Exact canonical quote result, including minimum/increment rounding and source capacity.
    pub offer: Offer,
    /// Original indices in the cut plan, sorted; each physical marker occurs in one order only.
    pub roll_indices: Vec<usize>,
    /// Sum of the assigned full marker lengths, in metres.
    pub required_length_m: f64,
    /// Sum of the assigned full-width gross marker areas, in m².
    pub required_stock_area_m2: f64,
    /// Longest assigned marker, checked against the offer's declared uninterrupted-cut limit.
    pub required_continuous_length_m: f64,
    /// Exact scope passed to the canonical quote evaluator for this subset.
    pub scope: Scope,
}

/// A complete feasible incumbent, never a subtotal from an unfinished assignment.
/// Roll orders retain cut-plan marker indices; thread orders retain consumables segment indices.
#[derive(Clone, Debug, Serialize)]
pub struct Report<O = Order> {
    /// `ftx-cut-order-allocation-report/1` or `ftx-thread-order-allocation-report/1`.
    pub schema: &'static str,
    /// Complete admitted bounded-search policy.
    pub policy: Policy,
    /// Current exact-locked offers which can improve through allocation of indivisible demands.
    pub candidate_offer_ids: Vec<String>,
    /// Otherwise current offers omitted because they do not lock the selected material snapshot.
    pub excluded_unlocked_offer_ids: Vec<String>,
    /// Whole-material canonical sample, including reasons a single offer cannot supply the job.
    pub whole_order_sample: Option<PriceSample>,
    /// Complete orders for every marker or thread segment, or unknown if none were found.
    pub selected_orders: Option<Vec<O>>,
    /// Sum of the complete incumbent's merchandise costs, never a partial price.
    pub selected_observed_order_total: Option<f64>,
    /// Attempted feasible-prefix extensions, including rejected assignments.
    pub search_nodes: usize,
    /// Actual uncached QuoteBook evaluations, also charged against the shared work limit.
    pub quote_evaluations: usize,
    /// Every potentially improving listed assignment was exhausted.
    pub search_complete: bool,
    /// The shared limit prevented completing this material's search.
    pub budget_exhausted: bool,
    /// A feasible incumbent exists and all possible listed improvements were excluded.
    pub optimality_proven: bool,
    /// Listed current offers cannot allocate the job; never asserted after budget exhaustion.
    pub no_feasible_listed_allocation: bool,
    /// Stable equal-cost behavior, conditional on the listed offers and indivisible demands.
    pub tie_break: &'static str,
}

impl<O> Report<O> {
    pub(crate) fn map_orders<T>(self, map: impl FnMut(O) -> T) -> Report<T> {
        Report {
            schema: self.schema,
            policy: self.policy,
            candidate_offer_ids: self.candidate_offer_ids,
            excluded_unlocked_offer_ids: self.excluded_unlocked_offer_ids,
            whole_order_sample: self.whole_order_sample,
            selected_orders: self
                .selected_orders
                .map(|orders| orders.into_iter().map(map).collect()),
            selected_observed_order_total: self.selected_observed_order_total,
            search_nodes: self.search_nodes,
            quote_evaluations: self.quote_evaluations,
            search_complete: self.search_complete,
            budget_exhausted: self.budget_exhausted,
            optimality_proven: self.optimality_proven,
            no_feasible_listed_allocation: self.no_feasible_listed_allocation,
            tie_break: self.tie_break,
        }
    }
}

pub(crate) struct Segment {
    pub index: usize,
    pub length_m: f64,
    pub quantity: f64,
}

pub(crate) struct Demand<'a> {
    pub material: &'a ResolvedMaterial,
    pub basis: Basis,
    pub segments: Vec<Segment>,
}

pub(crate) struct Assignment {
    pub offer: Offer,
    pub indices: Vec<usize>,
    pub length_m: f64,
    pub scope: Scope,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn budget(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}

fn add(sum: f64, value: f64) -> Result<f64, Refusal> {
    let next = sum + value;
    if !value.is_finite() || value <= 0.0 || !next.is_finite() || next <= sum {
        return Err(invalid(
            "supplier allocation sum overflows or loses a positive contribution",
        ));
    }
    Ok(next)
}

fn stock_keys(quote: &Quote) -> BTreeSet<(String, String)> {
    std::iter::once(&quote.sku)
        .chain(
            quote
                .price_breaks
                .iter()
                .filter_map(|tier| tier.sku.as_ref()),
        )
        .map(|sku| (quote.supplier.clone(), sku.clone()))
        .collect()
}

#[derive(Default)]
struct Work {
    limit: usize,
    nodes: usize,
    quotes: usize,
}

impl Work {
    fn charge(&mut self, quote: bool) -> bool {
        if self.nodes + self.quotes >= self.limit {
            return false;
        }
        if quote {
            self.quotes += 1;
        } else {
            self.nodes += 1;
        }
        true
    }
}

struct Incumbent {
    cost: f64,
    masks: Vec<u64>,
    offers: Vec<Option<Offer>>,
}

struct Prepared {
    sample: Option<PriceSample>,
    ids: Vec<String>,
    aliases: Vec<BTreeSet<(String, String)>>,
    components: Vec<usize>,
    unlocked: Vec<String>,
    costs_monotone: bool,
}

fn scope(
    demand: &Demand<'_>,
    market: &Market,
    mask: u64,
) -> Result<(Scope, f64, Vec<usize>), Refusal> {
    let mut quantity = 0.0;
    let mut length = 0.0;
    let mut longest = 0.0_f64;
    let mut indices = Vec::new();
    for (bit, segment) in demand.segments.iter().enumerate() {
        if mask & (1 << bit) != 0 {
            quantity = add(quantity, segment.quantity)?;
            length = add(length, segment.length_m)?;
            longest = longest.max(segment.length_m);
            indices.push(segment.index);
        }
    }
    Ok((
        Scope {
            currency: market.currency.clone(),
            region: market.region.clone(),
            quality: market.quality.clone(),
            terms: market.terms.clone(),
            as_of: market.as_of.clone(),
            as_of_at: market.as_of_at.clone(),
            max_age_days: market.max_age_days,
            requirement: quantity,
            basis: demand.basis,
            require_observed_quantity: market.require_observed_quantity,
            continuous_length_m: Some(longest),
            require_observed_continuous_length: market.require_observed_continuous_length,
        },
        length,
        indices,
    ))
}

fn prepare(
    demand: &Demand<'_>,
    book: &QuoteBook,
    market: &Market,
    work: &mut Work,
) -> Result<Prepared, Refusal> {
    let sample = if work.charge(true) {
        let (scope, _, _) = scope(demand, market, (1 << demand.segments.len()) - 1)?;
        Some(book.evaluate(demand.material, &scope)?)
    } else {
        None
    };
    let mut current = BTreeMap::new();
    let mut unlocked = BTreeSet::new();
    let mut costs_monotone = true;
    if let Some(sample) = &sample {
        let candidates = sample.offers.iter().map(|offer| &offer.quote).chain(
            sample
                .excluded_offers
                .iter()
                .filter(|excluded| {
                    matches!(
                        excluded.reason,
                        ExclusionReason::InsufficientQuantity
                            | ExclusionReason::InsufficientContinuousLength
                    )
                })
                .map(|excluded| &excluded.quote),
        );
        for quote in candidates {
            if quote
                .material_sha256
                .as_deref()
                .is_some_and(|sha| sha.eq_ignore_ascii_case(demand.material.sha256()))
            {
                costs_monotone &= quote.price_breaks.is_empty();
                current.insert(quote.offer_id.clone(), stock_keys(quote));
            } else {
                unlocked.insert(quote.offer_id.clone());
            }
        }
        if current.len() > MAX_OFFERS {
            return Err(budget(
                "split orders support at most eight current exact-locked offers per material; narrow the supplied quote book explicitly",
            ));
        }
    }
    let (ids, aliases) = current.into_iter().unzip();
    Ok(Prepared {
        sample,
        ids,
        aliases,
        components: Vec::new(),
        unlocked: unlocked.into_iter().collect(),
        costs_monotone,
    })
}

// Alias identity is transitive, including through unselected quotes for other material groups.
// At most 32 groups × 8 candidates enter this union; we retain component IDs, not duplicated
// transitive alias sets. Every identity comes from the already charged current canonical sample.
fn connect_stock(prepared: &mut [Prepared]) {
    fn root(parents: &[usize], mut index: usize) -> usize {
        while parents[index] != index {
            index = parents[index];
        }
        index
    }
    let count = prepared.iter().map(|p| p.ids.len()).sum();
    let mut parents: Vec<_> = (0..count).collect();
    let mut owner = BTreeMap::new();
    for (index, keys) in prepared.iter().flat_map(|p| &p.aliases).enumerate() {
        for key in keys {
            if let Some(previous) = owner.insert(key, index) {
                let left = root(&parents, index);
                let right = root(&parents, previous);
                parents[left] = right;
            }
        }
    }
    let components: Vec<_> = (0..count).map(|index| root(&parents, index)).collect();
    drop(owner);
    let mut offset = 0;
    for group in prepared {
        group.components = components[offset..offset + group.ids.len()].to_vec();
        offset += group.ids.len();
    }
}

struct Search<'a> {
    demand: &'a Demand<'a>,
    book: &'a QuoteBook,
    market: &'a Market,
    ids: Vec<String>,
    stock_components: Vec<usize>,
    costs_monotone: bool,
    cache: BTreeMap<u64, Vec<Option<Offer>>>,
    masks: Vec<u64>,
    active: Vec<Option<Offer>>,
    visit_order: Vec<usize>,
    incumbent: Option<Incumbent>,
    work: &'a mut Work,
    exhausted: bool,
}

impl Search<'_> {
    fn scope(&self, mask: u64) -> Result<(Scope, f64, Vec<usize>), Refusal> {
        scope(self.demand, self.market, mask)
    }

    fn sample(&mut self, mask: u64) -> Result<Option<PriceSample>, Refusal> {
        if !self.work.charge(true) {
            self.exhausted = true;
            return Ok(None);
        }
        let (scope, _, _) = self.scope(mask)?;
        self.book.evaluate(self.demand.material, &scope).map(Some)
    }

    fn priced(&mut self, mask: u64, candidate: usize) -> Result<Option<Offer>, Refusal> {
        if !self.cache.contains_key(&mask) {
            let Some(sample) = self.sample(mask)? else {
                return Ok(None);
            };
            let offers = self
                .ids
                .iter()
                .map(|id| {
                    sample
                        .offers
                        .iter()
                        .find(|offer| &offer.quote.offer_id == id)
                        .cloned()
                })
                .collect();
            self.cache.insert(mask, offers);
        }
        Ok(self.cache[&mask][candidate].clone())
    }

    fn visit(&mut self, depth: usize) -> Result<(), Refusal> {
        if depth == self.visit_order.len() {
            let cost = self
                .active
                .iter()
                .flatten()
                .try_fold(0.0, |sum, offer| add(sum, offer.order_cost))?;
            if self.incumbent.as_ref().is_none_or(|old| cost < old.cost) {
                self.incumbent = Some(Incumbent {
                    cost,
                    masks: self.masks.clone(),
                    offers: self.active.clone(),
                });
            }
            return Ok(());
        }
        let bit = 1 << self.visit_order[depth];
        for candidate in 0..self.ids.len() {
            if !self.work.charge(false) {
                self.exhausted = true;
                break;
            }
            // Multiple tiers for one physical supplier SKU are alternatives, not extra stock.
            if self.masks.iter().enumerate().any(|(other, &mask)| {
                other != candidate
                    && mask != 0
                    && self.stock_components[other] == self.stock_components[candidate]
            }) {
                continue;
            }
            let old_mask = self.masks[candidate];
            let new_mask = old_mask | bit;
            let Some(offer) = self.priced(new_mask, candidate)? else {
                if self.exhausted {
                    break;
                }
                continue;
            };
            let previous = self.active[candidate].replace(offer);
            self.masks[candidate] = new_mask;
            let cost = self
                .active
                .iter()
                .flatten()
                .try_fold(0.0, |sum, offer| add(sum, offer.order_cost))?;
            // An all-unit source price break can make a larger order cheaper than its prefix.
            // Partial-cost pruning is valid only when every candidate has a constant unit price.
            if !self.costs_monotone || self.incumbent.as_ref().is_none_or(|old| cost < old.cost) {
                self.visit(depth + 1)?;
            }
            self.active[candidate] = previous;
            self.masks[candidate] = old_mask;
            if self.exhausted {
                break;
            }
        }
        Ok(())
    }

    fn orders(&self) -> Result<Option<Vec<Assignment>>, Refusal> {
        let Some(incumbent) = &self.incumbent else {
            return Ok(None);
        };
        let mut orders = Vec::new();
        for (&mask, offer) in incumbent.masks.iter().zip(&incumbent.offers) {
            if let Some(offer) = offer {
                let (scope, length, indices) = self.scope(mask)?;
                orders.push(Assignment {
                    offer: offer.clone(),
                    indices,
                    length_m: length,
                    scope,
                });
            }
        }
        Ok(Some(orders))
    }
}

fn group(
    demand: &Demand<'_>,
    book: &QuoteBook,
    market: &Market,
    policy: &Policy,
    prepared: Prepared,
    work: &mut Work,
) -> Result<(Report<Assignment>, BTreeSet<usize>), Refusal> {
    let before = (work.nodes, work.quotes);
    let Prepared {
        sample,
        ids,
        components,
        unlocked,
        costs_monotone,
        ..
    } = prepared;
    let initial_quotes = usize::from(sample.is_some());
    let candidate_count = ids.len();
    let mut search = Search {
        demand,
        book,
        market,
        ids,
        stock_components: components,
        costs_monotone,
        cache: BTreeMap::new(),
        masks: vec![0; candidate_count],
        active: vec![None; candidate_count],
        visit_order: (0..demand.segments.len()).collect(),
        incumbent: None,
        work,
        exhausted: sample.is_none(),
    };
    search.visit_order.sort_by(|&a, &b| {
        demand.segments[b]
            .length_m
            .total_cmp(&demand.segments[a].length_m)
            .then(a.cmp(&b))
    });
    let full = (1 << demand.segments.len()) - 1;
    if let Some(sample) = &sample {
        let offers: Vec<_> = search
            .ids
            .iter()
            .map(|id| {
                sample
                    .offers
                    .iter()
                    .find(|offer| &offer.quote.offer_id == id)
                    .cloned()
            })
            .collect();
        if let Some((index, offer)) = offers
            .iter()
            .enumerate()
            .filter_map(|(i, offer)| offer.as_ref().map(|offer| (i, offer)))
            .min_by(|(_, a), (_, b)| {
                a.order_cost
                    .total_cmp(&b.order_cost)
                    .then(a.quote.offer_id.cmp(&b.quote.offer_id))
            })
        {
            let mut masks = search.masks.clone();
            let mut active = search.active.clone();
            masks[index] = full;
            active[index] = Some(offer.clone());
            search.incumbent = Some(Incumbent {
                cost: offer.order_cost,
                masks,
                offers: active,
            });
        }
        search.cache.insert(full, offers);
        // With at most one possible offer the whole-order sample already exhausts every option.
        if search.ids.len() > 1 {
            search.visit(0)?;
        }
    }
    let selected_orders = search.orders()?;
    let total = search.incumbent.as_ref().map(|incumbent| incumbent.cost);
    let selected_components = search
        .incumbent
        .as_ref()
        .map(|incumbent| {
            incumbent
                .masks
                .iter()
                .enumerate()
                .filter(|(_, mask)| **mask != 0)
                .map(|(index, _)| search.stock_components[index])
                .collect()
        })
        .unwrap_or_default();
    Ok((
        Report {
            schema: "ftx-cut-order-allocation-report/1",
            policy: policy.clone(),
            candidate_offer_ids: search.ids.clone(),
            excluded_unlocked_offer_ids: unlocked,
            whole_order_sample: sample,
            selected_orders,
            selected_observed_order_total: total,
            search_nodes: search.work.nodes - before.0,
            quote_evaluations: initial_quotes + search.work.quotes - before.1,
            search_complete: !search.exhausted,
            budget_exhausted: search.exhausted,
            optimality_proven: !search.exhausted && total.is_some(),
            no_feasible_listed_allocation: !search.exhausted && total.is_none(),
            tie_break: "retain the cheapest whole-order baseline on equal cost; otherwise retain the first complete allocation in descending-marker-length then offer-ID order",
        },
        selected_components,
    ))
}

pub(crate) fn validate_thread_policy(policy: &Policy) -> Result<(), Refusal> {
    if policy.schema != "ftx-thread-order-allocation/1"
        || !(1..=MAX_WORK).contains(&policy.max_search_nodes)
    {
        return Err(invalid(
            "thread split_orders requires ftx-thread-order-allocation/1 and 1..200000 shared work units",
        ));
    }
    Ok(())
}

// The sewing consumer supplies canonical complete per-operation/channel/copy lengths. The
// allocator never reconstructs stitch geometry, divides a continuous segment, or prices a prefix.
pub(crate) fn threads(
    demands: &[Demand<'_>],
    book: &QuoteBook,
    market: &Market,
    policy: &Policy,
    stock_components: &BTreeMap<String, usize>,
    excluded_components: &BTreeSet<usize>,
) -> Result<Vec<Report<Assignment>>, Refusal> {
    validate_thread_policy(policy)?;
    let count = demands.iter().map(|d| d.segments.len()).sum::<usize>();
    if demands.is_empty() || count > MAX_THREAD_SEGMENTS || book.quotes.len() > MAX_QUOTES {
        return Err(budget(
            "thread split orders support 1..256 continuous segments and at most 4096 quote observations",
        ));
    }
    let mut identities = BTreeSet::new();
    let mut indices = BTreeSet::new();
    for demand in demands {
        if demand.segments.is_empty() || demand.segments.len() > MAX_YARN_SEGMENTS {
            return Err(budget(
                "thread split orders support at most 63 continuous segments per exact yarn",
            ));
        }
        if demand.basis != Basis::Length || !identities.insert(demand.material.sha256()) {
            return Err(invalid(
                "thread allocation needs distinct exact-yarn length requirements",
            ));
        }
        for segment in &demand.segments {
            add(0.0, segment.length_m)?;
            if segment.quantity != segment.length_m || !indices.insert(segment.index) {
                return Err(invalid(
                    "thread allocation must preserve every complete segment exactly once",
                ));
            }
        }
    }
    let mut work = Work {
        limit: policy.max_search_nodes,
        ..Work::default()
    };
    let mut prepared = demands
        .iter()
        .map(|demand| prepare(demand, book, market, &mut work))
        .collect::<Result<Vec<_>, _>>()?;
    for input in &mut prepared {
        // Use all current observations, including unused transitive bridging SKUs. Ambiguous
        // stock shared by distinct exact yarn snapshots is independently excluded by the caller.
        input
            .ids
            .retain(|id| !excluded_components.contains(&stock_components[id]));
        input.components = input.ids.iter().map(|id| stock_components[id]).collect();
    }
    demands.iter().zip(prepared).map(|(demand, prepared)| {
        let (mut report, _) = group(demand, book, market, policy, prepared, &mut work)?;
        report.schema = "ftx-thread-order-allocation-report/1";
        report.tie_break = "retain the cheapest whole-order baseline on equal cost; otherwise retain the first complete allocation in descending-segment-length then offer-ID order";
        Ok(report)
    }).collect()
}

pub(super) fn evaluate(
    plan: &CutPlan,
    book: &QuoteBook,
    market: &Market,
    policy: &Policy,
) -> Result<Value, Refusal> {
    if policy.schema != "ftx-cut-order-allocation/1"
        || !(1..=MAX_WORK).contains(&policy.max_search_nodes)
    {
        return Err(invalid(
            "split_orders requires ftx-cut-order-allocation/1 and 1..200000 shared work units",
        ));
    }
    if plan.rolls.is_empty() || plan.rolls.len() > MAX_MARKERS || book.quotes.len() > MAX_QUOTES {
        return Err(budget(
            "split orders support 1..32 markers and at most 4096 quote observations",
        ));
    }
    book.validate()?;
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, marker) in plan.rolls.iter().enumerate() {
        add(0.0, marker.length_m)?;
        add(0.0, marker.stock_area_m2)?;
        let indices = groups.entry(marker.material.sha256()).or_default();
        if indices
            .first()
            .is_some_and(|&i| plan.rolls[i].width_m != marker.width_m)
        {
            return Err(invalid(
                "markers of one material snapshot must retain the same stock width",
            ));
        }
        indices.push(index);
        if indices.len() > MAX_MATERIAL_MARKERS {
            return Err(budget(
                "split orders support at most twelve markers per material",
            ));
        }
    }
    let mut work = Work {
        limit: policy.max_search_nodes,
        ..Work::default()
    };
    let demands = groups
        .values()
        .map(|indices| Demand {
            material: &plan.rolls[indices[0]].material,
            basis: Basis::Area,
            segments: indices
                .iter()
                .map(|&index| Segment {
                    index,
                    length_m: plan.rolls[index].length_m,
                    quantity: plan.rolls[index].stock_area_m2,
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let mut prepared = demands
        .iter()
        .map(|demand| prepare(demand, book, market, &mut work))
        .collect::<Result<Vec<_>, _>>()?;
    connect_stock(&mut prepared);
    let mut material_orders = Vec::new();
    let mut roll_prices = Vec::new();
    let mut complete = true;
    let mut observed = true;
    let mut continuous = true;
    let mut proved = true;
    let mut search_complete = true;
    let mut total = 0.0;
    let mut used_stock = BTreeSet::new();
    for (((hash, indices), demand), prepared) in groups.into_iter().zip(&demands).zip(prepared) {
        let (report, components) = group(demand, book, market, policy, prepared, &mut work)?;
        let report = report.map_orders(|order| Order {
            offer: order.offer,
            roll_indices: order.indices,
            required_length_m: order.length_m,
            required_stock_area_m2: order.scope.requirement,
            required_continuous_length_m: order.scope.continuous_length_m.expect("marker scope"),
            scope: order.scope,
        });
        // All groups' candidate identities were connected before selection. An unselected bridging
        // quote therefore cannot make one supplier's physical stock appear independent again.
        if !used_stock.is_disjoint(&components) {
            return Err(invalid(
                "split orders cannot reuse one supplier stock alias component across distinct material snapshots",
            ));
        }
        used_stock.extend(components);
        let mut assignments = BTreeMap::new();
        if let Some(orders) = &report.selected_orders {
            for order in orders {
                observed &= order.offer.quantity_sufficiency_observed;
                continuous &= order.offer.within_observed_continuous_limit == Some(true);
                total = add(total, order.offer.order_cost)?;
                for &index in &order.roll_indices {
                    assignments.insert(index, &order.offer);
                }
            }
        } else {
            complete = false;
            observed = false;
            continuous = false;
        }
        proved &= report.optimality_proven;
        search_complete &= report.search_complete;
        let mut area = 0.0;
        let mut length = 0.0;
        let mut longest = 0.0_f64;
        for &index in &indices {
            let marker = &plan.rolls[index];
            area = add(area, marker.stock_area_m2)?;
            length = add(length, marker.length_m)?;
            longest = longest.max(marker.length_m);
            let offer = assignments.get(&index);
            roll_prices.push(json!({"roll_index":index,"material_sha256":hash,
                "required_length_m":marker.length_m,"required_stock_area_m2":marker.stock_area_m2,
                "material_order_index":material_orders.len(),
                "allocated_offer_id":offer.map(|o| &o.quote.offer_id),
                "continuity":match offer {None => "no_complete_allocation", Some(o) if o.within_observed_continuous_limit == Some(true) => "within_observed_limit", _ => "source_limit_unknown"}}));
        }
        material_orders.push(
            json!({"material_order_index":material_orders.len(),"material_sha256":hash,
            "roll_indices":indices,"required_length_m":length,"required_stock_area_m2":area,
            "required_continuous_length_m":longest,
            "marker_lengths_m":indices.iter().map(|&i| plan.rolls[i].length_m).collect::<Vec<_>>(),
            "allocation":report}),
        );
    }
    roll_prices.sort_by_key(|row| row["roll_index"].as_u64());
    Ok(
        json!({"schema":"ftx-cut-procurement/3","cut_plan":plan,"market":market,"quotes":book,
        "roll_prices":roll_prices,"material_orders":material_orders,"currency":market.currency,
        "complete_offer_coverage":complete,"complete_observed_quantity_coverage":observed,
        "complete_continuous_length_limit_coverage":continuous,
        "selected_observed_order_total":if complete {Some(total)} else {None},
        "least_observed_independent_order_total":if complete && proved {Some(total)} else {None},
        "order_allocation_optimality_proven":complete && proved,"search_complete":search_complete,
        "budget_exhausted":!search_complete,"search_nodes":work.nodes,"quote_evaluations":work.quotes,
        "limits":["Only complete indivisible markers are assigned; marker and pattern geometry are unchanged.",
            "Only exact material-snapshot-locked offers are candidates. Current offers connected by supplier/base/tier SKU aliases share one stock identity, including transitive links through unselected candidates in other material groups; they cannot multiply observed stock.",
            "Canonical quote evaluation applies latest observations, market/date/source locks, minima, increments, aggregate order capacity and longest-marker continuous-cut limits independently to each selected order.",
            "Capacity and continuous-cut limits are dated observations, not reservations, guaranteed cut segmentation or checkout permission. Missing capacity stays explicit under the market's ordinary admission policy.",
            "The selected total is a complete feasible incumbent. The least-observed total is present only after every listed improvement is excluded. No supplier bundle, shipping, population-market or unlisted-offer optimum is inferred.",
            "The shared work limit charges attempted assignments and uncached canonical quote evaluations across all material groups."]}),
    )
}
