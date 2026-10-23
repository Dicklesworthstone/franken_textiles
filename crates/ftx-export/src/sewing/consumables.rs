//! Source-qualified thread consumption on a freshly admitted sewing plan. The price engine remains
//! QuoteBook; stitch geometry, sewing qualification and material resolution retain their authorities.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_catalog::{ResolvedMaterial, Selection};
use ftx_pattern::material_cost::{Basis, Offer, PriceSample, Quote, QuoteBook, Scope};
use ftx_pattern::materials::{CATALOG_SCHEMA, Catalog, Evidence, Form, Source};
use serde::{Deserialize, Serialize};

use crate::cutting::procurement::Market;

use super::{SewOptions, SewPlan, bad, positive, qualified_text};

const MAX_CHANNELS: usize = 8;
const MAX_MATERIALS: usize = 32;
const MAX_QUOTES: usize = 4096;
const MAX_WORK: usize = 200_000;

/// One declared thread channel; its coefficient is applicable only inside the parent qualification.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    /// Exact channel name, such as needle or bobbin. No channel is inferred from a stitch name.
    pub id: String,
    /// Embedded, digest-locked yarn selection; a live catalog ID alone is insufficient.
    pub material: Selection,
    /// Metres consumed for one generated station interval, including every repeated traversal.
    pub metres_per_traversal: f64,
    /// Additional metres at the start of ONE operation/channel, never per stitch or directed run.
    pub start_allowance_m: f64,
    /// Additional metres at the end of ONE operation/channel, never per stitch or directed run.
    pub end_allowance_m: f64,
}

/// Independently sourced effective consumption model, attached to one exact sewing qualification.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consumption {
    /// Explicit source assertion that every required thread channel is represented.
    /// False preserves partial diagnostics but prevents a complete requirement or purchase total.
    pub channels_complete: bool,
    /// One to eight separately identified channels, including any required lower/looper threads.
    pub channels: Vec<Channel>,
    /// Consumption evidence, independent of the parent process-envelope evidence.
    pub evidence: Evidence,
    /// Source of the per-traversal observations and separately declared start/end allowances.
    pub source: Source,
    /// Measurement and reduction method, including applicability of the effective coefficient.
    pub method: String,
    /// Conditioning and consumption-specific acquisition details.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
    /// Independent permission for estimated consumption; process/material consent cannot imply it.
    #[serde(default)]
    pub allow_estimates: bool,
}

/// Optional dated purchasing of the complete thread inventory, through the existing price schema.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Purchasing {
    /// Original immutable observations; no prices are synthesized by the sewing consumer.
    pub quote_book: QuoteBook,
    /// Existing comparable market, quantity and continuity policies. Split orders are unsupported.
    pub market: Market,
}

/// Opt-in consumer; an empty object requests quantities without purchasing.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Absence leaves prices unrequested rather than supplying a zero merchandise cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purchasing: Option<Purchasing>,
}

/// One actual operation/channel contribution, retaining the separately applied allowances.
#[derive(Clone, Debug, Serialize)]
pub struct Contribution {
    /// Exact operation identity, independent of the selected schedule's position.
    pub operation_id: String,
    /// Admitted parent qualification; its retained consumption source governs this contribution.
    pub qualification_id: String,
    /// Exact channel from that qualification's complete or explicitly partial list.
    pub channel_id: String,
    /// Resolved yarn snapshot, indexed by the report's material map.
    pub material_sha256: String,
    /// Actual intervals, counting reverse, backtack and reinforcement travel every time.
    pub stitch_intervals: usize,
    /// Qualified coefficient in metres per traversed interval.
    pub metres_per_traversal: f64,
    /// Interval count multiplied by the qualified coefficient, in metres.
    pub traversal_length_m: f64,
    /// Start allowance applied once to this complete operation/channel.
    pub start_allowance_m: f64,
    /// End allowance applied once to this complete operation/channel.
    pub end_allowance_m: f64,
    /// Traversal length plus both declared allowances, in metres.
    pub required_length_m: f64,
}

/// A complete whole-job requirement for one exact yarn snapshot.
#[derive(Clone, Debug, Serialize)]
pub struct Requirement {
    /// Index into the source-complete frozen material map.
    pub material_sha256: String,
    /// Sum of all operations/channels before any supplier minimum or increment is applied.
    pub required_length_m: f64,
    /// Largest complete operation/channel demand; no mid-operation thread splice is inferred.
    pub required_continuous_length_m: f64,
}

/// Why an operation cannot contribute a complete thread inventory.
#[derive(Clone, Debug, Serialize)]
pub struct MissingOperation {
    /// Exact operation whose consumption is unknown or only partial.
    pub operation_id: String,
    /// Missing process qualification, missing consumption or incomplete channel coverage.
    pub reason: String,
}

/// Canonical pricing for one whole exact-yarn requirement.
#[derive(Clone, Debug, Serialize)]
pub struct MaterialOrder {
    /// Complete aggregate requirement which was passed to the price engine.
    pub requirement: Requirement,
    /// Unmodified canonical sample, including dated eligibility and stock exclusions.
    pub sample: PriceSample,
    /// Current canonical offers without an exact lock to the consumed yarn; never selected here.
    pub excluded_unlocked_offer_ids: Vec<String>,
    /// Current offers whose supplier/base/tier SKU component spans distinct consumed snapshots.
    pub excluded_shared_stock_offer_ids: Vec<String>,
    /// Cheapest eligible, exact-locked, unambiguous whole order; ties use the exact offer ID.
    pub least_observed_order: Option<Offer>,
}

/// Whole-order observations, not checkout, reserved inventory or a supplier-bundle optimization.
#[derive(Clone, Debug, Serialize)]
pub struct PurchasingReport {
    /// Exact comparable currency, without an exchange-rate assumption.
    pub currency: String,
    /// Every complete requirement has an admitted whole order.
    pub complete_offer_coverage: bool,
    /// Every selected rounded order has sufficient source-observed capacity.
    pub complete_observed_quantity_coverage: bool,
    /// Every largest operation/channel demand fits a source-observed continuous-length limit.
    pub complete_continuous_length_limit_coverage: bool,
    /// Empty when consumption is incomplete; a known prefix is never priced as the whole job.
    pub material_orders: Vec<MaterialOrder>,
    /// Sum of complete independent observed minima, absent if any contribution/order is unknown.
    pub least_observed_independent_order_total: Option<f64>,
    /// Explicit reason for an unknown complete total.
    pub unavailable_reason: Option<String>,
    /// Quote observations visited by canonical market selection and per-material evaluations.
    pub quote_observations_evaluated: usize,
    /// Base/tier SKU declarations visited once while resolving transitive inventory aliases.
    pub stock_aliases_examined: usize,
}

/// Source-qualified inventory derived from the actual selected production sewing plan.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-consumables/1`.
    pub schema: &'static str,
    /// Every operation has an admitted source model declaring all required channels.
    pub complete_consumption_coverage: bool,
    /// Missing quantities are not zero and cannot be repaired by a later price lookup.
    pub missing_operations: Vec<MissingOperation>,
    /// Canonically ordered by operation/channel ID, independent of schedule optimization.
    pub operation_channels: Vec<Contribution>,
    /// Exact frozen yarn selections; no sheet properties are required or inferred.
    pub materials: BTreeMap<String, Selection>,
    /// Complete aggregate requirements only; empty when any operation is unknown or partial.
    pub material_requirements: Vec<Requirement>,
    /// Sum of required metres across all channels/materials, absent on incomplete consumption.
    pub total_required_length_m: Option<f64>,
    /// Absent unless requested; no partial purchase total is ever supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purchasing: Option<PurchasingReport>,
    /// Conservative preflight work reservation, including unused qualified alternatives.
    pub purchasing_work_bound: usize,
}

fn budget(message: &str) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}

fn nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn add(sum: f64, value: f64) -> Result<f64, Refusal> {
    let next = sum + value;
    if !nonnegative(value) || !next.is_finite() || (value > 0.0 && next <= sum) {
        return Err(bad(
            "thread quantity/cost sum overflows or loses a positive contribution",
        ));
    }
    Ok(next)
}

fn resolve_thread(selection: &Selection) -> Result<ResolvedMaterial, Refusal> {
    if selection.record.is_none() || selection.expected_sha256.is_none() {
        return Err(bad(
            "thread consumption needs an embedded, exact digest-locked yarn selection",
        ));
    }
    let selected = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    }
    .resolve(selection)?;
    if selected.material().form != Form::Yarn {
        return Err(bad(
            "a sewing thread channel must select a yarn record, not a sheet or fiber",
        ));
    }
    Ok(selected)
}

impl Consumption {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        if self.channels.is_empty() || self.channels.len() > MAX_CHANNELS {
            return Err(budget(
                "thread consumption supports one to eight declared channels",
            ));
        }
        if !qualified_text(&self.source.reference)
            || !qualified_text(&self.source.accessed_on)
            || !qualified_text(&self.source.note)
            || !qualified_text(&self.method)
            || self.conditions.len() > 64
            || self
                .conditions
                .iter()
                .any(|(k, v)| !qualified_text(k) || !qualified_text(v))
        {
            return Err(bad(
                "thread consumption needs its own source, method and conditioning metadata",
            ));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(bad(
                "estimated thread consumption requires independent explicit consent",
            ));
        }
        let mut ids = BTreeSet::new();
        for channel in &self.channels {
            if !qualified_text(&channel.id)
                || !ids.insert(&channel.id)
                || !positive(channel.metres_per_traversal)
                || !nonnegative(channel.start_allowance_m)
                || !nonnegative(channel.end_allowance_m)
            {
                return Err(bad(
                    "thread channels need unique IDs, positive finite consumption and explicit nonnegative allowances",
                ));
            }
            resolve_thread(&channel.material)?;
        }
        Ok(())
    }
}

/// Reserve bounded canonical quote observations and base/tier alias processing for one plan.
/// Studies should multiply this conservative reservation by their possible recipe/variant trials.
///
/// # Errors
/// More than 8 channels per qualification, 32 yarn snapshots, 4096 observations or 200000 work
/// units; an unlocked thread selection or unsupported supplier splitting. No source is consulted.
pub fn purchasing_work_bound(options: &SewOptions) -> Result<usize, Refusal> {
    let Some(request) = &options.consumables else {
        return Ok(0);
    };
    let mut hashes = BTreeSet::new();
    for model in options
        .qualifications
        .iter()
        .filter_map(|q| q.thread_consumption.as_ref())
    {
        if model.channels.is_empty() || model.channels.len() > MAX_CHANNELS {
            return Err(budget(
                "thread consumption supports one to eight declared channels",
            ));
        }
        for channel in &model.channels {
            hashes.insert(
                channel
                    .material
                    .expected_sha256
                    .as_ref()
                    .ok_or_else(|| bad("thread consumption needs an exact yarn snapshot lock"))?,
            );
            if hashes.len() > MAX_MATERIALS {
                return Err(budget(
                    "thread consumption supports at most 32 exact yarn snapshots",
                ));
            }
        }
    }
    let Some(purchasing) = &request.purchasing else {
        return Ok(0);
    };
    if purchasing.market.split_orders.is_some() {
        return Err(bad(
            "sewing consumables use one complete order per yarn; split orders are unsupported",
        ));
    }
    let quotes = &purchasing.quote_book.quotes;
    if quotes.len() > MAX_QUOTES || quotes.iter().any(|q| q.price_breaks.len() > 64) {
        return Err(budget(
            "thread purchasing supports 4096 observations and 64 price thresholds per quote",
        ));
    }
    let aliases: usize = quotes
        .iter()
        .map(|q| 1 + q.price_breaks.iter().filter(|p| p.sku.is_some()).count())
        .sum();
    let work = quotes.len() * (hashes.len() + 1) + aliases;
    if work > MAX_WORK {
        return Err(budget(
            "thread purchasing exceeds 200000 canonical quote/stock-alias work units",
        ));
    }
    Ok(work)
}

pub(super) fn evaluate(plan: &SewPlan, request: &Request) -> Result<Report, Refusal> {
    let mut report = Report {
        schema: "ftx-sewing-consumables/1",
        complete_consumption_coverage: true,
        missing_operations: Vec::new(),
        operation_channels: Vec::new(),
        materials: BTreeMap::new(),
        material_requirements: Vec::new(),
        total_required_length_m: None,
        purchasing: None,
        purchasing_work_bound: purchasing_work_bound(&plan.options)?,
    };
    let qualifications: BTreeMap<_, _> = plan
        .options
        .qualifications
        .iter()
        .map(|q| (q.id.as_str(), q))
        .collect();
    let mut resolved = BTreeMap::new();
    let mut requirements: BTreeMap<String, Requirement> = BTreeMap::new();
    let mut operations: Vec<_> = plan.seams.iter().collect();
    operations.sort_by_key(|seam| seam.pass.operation_id());
    for seam in operations {
        let id = seam.pass.operation_id();
        let qualification = seam
            .qualification_id
            .as_deref()
            .and_then(|id| qualifications.get(id).copied());
        let missing = match qualification {
            None => Some("no admitted process qualification for this operation"),
            Some(q) if q.thread_consumption.is_none() => {
                Some("admitted qualification has no thread consumption evidence")
            }
            Some(q) if !q.thread_consumption.as_ref().unwrap().channels_complete => {
                Some("source does not establish all required thread channels")
            }
            _ => None,
        };
        if let Some(reason) = missing {
            report.missing_operations.push(MissingOperation {
                operation_id: id.into(),
                reason: reason.into(),
            });
        }
        let Some((q, model)) =
            qualification.and_then(|q| q.thread_consumption.as_ref().map(|m| (q, m)))
        else {
            continue;
        };
        let mut channels: Vec<_> = model.channels.iter().collect();
        channels.sort_by_key(|c| &c.id);
        for channel in channels {
            let hash = channel
                .material
                .expected_sha256
                .as_ref()
                .ok_or_else(|| bad("thread snapshot lock is absent"))?;
            if !resolved.contains_key(hash) {
                let selected = resolve_thread(&channel.material)?;
                report
                    .materials
                    .insert(hash.clone(), selected.frozen_selection());
                resolved.insert(hash.clone(), selected);
            }
            let traversal = seam.stitch_intervals as f64 * channel.metres_per_traversal;
            if !positive(traversal) {
                return Err(bad(
                    "thread traversal requirement must be representable and positive",
                ));
            }
            let length = add(
                add(traversal, channel.start_allowance_m)?,
                channel.end_allowance_m,
            )?;
            let requirement = requirements
                .entry(hash.clone())
                .or_insert_with(|| Requirement {
                    material_sha256: hash.clone(),
                    required_length_m: 0.0,
                    required_continuous_length_m: 0.0,
                });
            requirement.required_length_m = add(requirement.required_length_m, length)?;
            requirement.required_continuous_length_m =
                requirement.required_continuous_length_m.max(length);
            report.operation_channels.push(Contribution {
                operation_id: id.into(),
                qualification_id: q.id.clone(),
                channel_id: channel.id.clone(),
                material_sha256: hash.clone(),
                stitch_intervals: seam.stitch_intervals,
                metres_per_traversal: channel.metres_per_traversal,
                traversal_length_m: traversal,
                start_allowance_m: channel.start_allowance_m,
                end_allowance_m: channel.end_allowance_m,
                required_length_m: length,
            });
        }
    }
    report.complete_consumption_coverage = report.missing_operations.is_empty();
    if report.complete_consumption_coverage {
        let mut total = 0.0;
        for requirement in requirements.into_values() {
            total = add(total, requirement.required_length_m)?;
            report.material_requirements.push(requirement);
        }
        report.total_required_length_m = Some(total);
    }
    if let Some(purchasing) = &request.purchasing {
        report.purchasing = Some(price(&report, &resolved, purchasing)?);
    }
    Ok(report)
}

// Every current base/tier SKU of one supplier is an inventory alias. Canonical market/as-of
// selection includes unused bridging offers, without importing future or superseded identities.
// At most 4096 observations / the preflight alias budget.
fn stock_components(quotes: &[&Quote]) -> (BTreeMap<String, usize>, usize) {
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

fn purchase_scope(requirement: &Requirement, market: &Market) -> Scope {
    Scope {
        currency: market.currency.clone(),
        region: market.region.clone(),
        quality: market.quality.clone(),
        terms: market.terms.clone(),
        as_of: market.as_of.clone(),
        as_of_at: market.as_of_at.clone(),
        max_age_days: market.max_age_days,
        requirement: requirement.required_length_m,
        basis: Basis::Length,
        require_observed_quantity: market.require_observed_quantity,
        continuous_length_m: Some(requirement.required_continuous_length_m),
        require_observed_continuous_length: market.require_observed_continuous_length,
    }
}

fn price(
    report: &Report,
    materials: &BTreeMap<String, ResolvedMaterial>,
    request: &Purchasing,
) -> Result<PurchasingReport, Refusal> {
    request.quote_book.validate()?;
    let market = &request.market;
    let mut priced = PurchasingReport {
        currency: market.currency.clone(),
        complete_offer_coverage: false,
        complete_observed_quantity_coverage: false,
        complete_continuous_length_limit_coverage: false,
        material_orders: Vec::new(),
        least_observed_independent_order_total: None,
        unavailable_reason: None,
        quote_observations_evaluated: 0,
        stock_aliases_examined: 0,
    };
    if !report.complete_consumption_coverage {
        priced.unavailable_reason =
            Some("thread consumption is incomplete; partial requirements are not priced".into());
        return Ok(priced);
    }
    let first = report.material_requirements.first().ok_or_else(|| {
        bad("complete thread consumption must contain at least one yarn requirement")
    })?;
    let current = request
        .quote_book
        .latest_observations(&purchase_scope(first, market))?;
    priced.quote_observations_evaluated = request.quote_book.quotes.len();
    let (components, aliases) = stock_components(&current);
    priced.stock_aliases_examined = aliases;
    for requirement in &report.material_requirements {
        let scope = purchase_scope(requirement, market);
        let sample = request
            .quote_book
            .evaluate(&materials[&requirement.material_sha256], &scope)?;
        priced.quote_observations_evaluated += request.quote_book.quotes.len();
        priced.material_orders.push(MaterialOrder {
            requirement: requirement.clone(),
            sample,
            excluded_unlocked_offer_ids: Vec::new(),
            excluded_shared_stock_offer_ids: Vec::new(),
            least_observed_order: None,
        });
    }
    let locked = |offer: &Offer, hash: &str| {
        offer
            .quote
            .material_sha256
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case(hash))
    };
    let mut owners: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
    for order in &priced.material_orders {
        for offer in &order.sample.offers {
            if locked(offer, &order.requirement.material_sha256) {
                owners
                    .entry(components[&offer.quote.offer_id])
                    .or_default()
                    .insert(&order.requirement.material_sha256);
            }
        }
    }
    let shared: BTreeSet<_> = owners
        .iter()
        .filter_map(|(&id, hashes)| (hashes.len() > 1).then_some(id))
        .collect();
    drop(owners);
    let mut total = 0.0;
    let mut complete = true;
    let mut quantity = true;
    let mut continuity = true;
    for order in &mut priced.material_orders {
        let mut candidates = Vec::new();
        for offer in &order.sample.offers {
            if !locked(offer, &order.requirement.material_sha256) {
                order
                    .excluded_unlocked_offer_ids
                    .push(offer.quote.offer_id.clone());
            } else if shared.contains(&components[&offer.quote.offer_id]) {
                order
                    .excluded_shared_stock_offer_ids
                    .push(offer.quote.offer_id.clone());
            } else {
                candidates.push(offer);
            }
        }
        order.least_observed_order = candidates
            .into_iter()
            .min_by(|a, b| {
                a.order_cost
                    .total_cmp(&b.order_cost)
                    .then_with(|| a.quote.offer_id.cmp(&b.quote.offer_id))
            })
            .cloned();
        if let Some(offer) = &order.least_observed_order {
            total = add(total, offer.order_cost)?;
            quantity &= offer.quantity_sufficiency_observed;
            continuity &= offer.within_observed_continuous_limit == Some(true);
        } else {
            complete = false;
        }
    }
    priced.complete_offer_coverage = complete;
    priced.complete_observed_quantity_coverage = complete && quantity;
    priced.complete_continuous_length_limit_coverage = complete && continuity;
    priced.least_observed_independent_order_total = complete.then_some(total);
    if !complete {
        priced.unavailable_reason = Some("at least one exact yarn requirement has no eligible unambiguous source-locked whole order".into());
    }
    Ok(priced)
}
