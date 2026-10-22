//! Complete garment cutting across roll stock and individually identified hides/remnants.
//! Routes are explicit source-piece assignments. Both planners keep the whole source pattern;
//! a final inventory equality check prevents omitted, duplicated or silently rerouted cut parts.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::{Pattern, material_binding::Changes, material_cost::QuoteBook, textile_import};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{CutOptions, CutPlan, procurement, stock};
use crate::{Ctx, ExportOptions, material_study::Outcome};

/// Explicit physical supply route, not inferred from a textile name or inventory proximity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockKind {
    /// Source-qualified rectangular roll stock.
    Roll,
    /// Individually identified finite hides, remnants or sheets.
    FiniteStock,
}

/// Route every physical instance and copy of one original source piece together.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    /// Source-piece ID; instance names and material labels do not identify this scope.
    pub piece: String,
    /// Planner responsible for this piece's complete physical cut inventory.
    pub stock: StockKind,
}

/// One dated comparable market, using the existing quote and exact-stock binding authorities.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Purchasing {
    /// Immutable source observations for both roll materials and whole finite items.
    pub quotes: QuoteBook,
    /// One currency, region, quality, terms and evaluation time for the complete order.
    pub market: procurement::Market,
    /// Exact physical-stock links; empty when no finite items are priced.
    #[serde(default)]
    pub finite_bindings: Vec<procurement::finite::Binding>,
}

impl Purchasing {
    fn finite_request(&self) -> procurement::finite::Request {
        procurement::finite::Request {
            schema: "ftx-finite-stock-pricing/1".into(),
            quotes: self.quotes.clone(),
            market: self.market.clone(),
            bindings: self.finite_bindings.clone(),
        }
    }
}

/// Complete mixed-stock manufacturing request, evaluated without changing pattern geometry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// `ftx-cutting-job/1`.
    pub schema: String,
    /// Exactly one route per source piece, including unsimulated lining and reinforcement.
    pub routes: Vec<Route>,
    /// Required iff at least one piece uses roll stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll: Option<CutOptions>,
    /// Required iff at least one piece uses physical finite stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finite_stock: Option<stock::Request>,
    /// Missing or invalid prices do not erase an otherwise complete geometric allocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purchasing: Option<Purchasing>,
}

/// A physical cut part, retained exactly once in the complete job inventory.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Item {
    /// Original source-piece ID.
    pub piece: String,
    /// Original expanded physical instance ID, including mirrors and unfolded pieces.
    pub instance: String,
    /// Zero-based complete-garment copy index.
    pub copy: usize,
    /// Explicit planner route.
    pub stock: StockKind,
    /// Exact source-qualified material snapshot placed on stock.
    pub material_sha256: String,
}

/// Independent purchasing outcomes, with no partial sum presented as a complete job cost.
#[derive(Clone, Debug, Serialize)]
pub struct PriceReport {
    /// `ftx-cutting-job-procurement/1`.
    pub schema: &'static str,
    /// Exact common source and market inputs.
    pub request: Purchasing,
    /// Ordinary whole-material roll orders or their independent refusal.
    pub roll: Outcome<Value>,
    /// Ordinary exact-item procurement or its independent refusal.
    pub finite_stock: Outcome<procurement::finite::Report>,
    /// Common market currency; no conversion is performed.
    pub currency: String,
    /// Every used material and physical item has a valid complete offer.
    pub complete_offer_coverage: bool,
    /// Every selected offer additionally has enough observed whole-order inventory.
    pub complete_observed_quantity_coverage: bool,
    /// Roll orders plus whole used finite items, counted once each, or unknown.
    pub least_observed_independent_order_total: Option<f64>,
    /// Failure to form a complete finite total; independent route reports remain inspectable.
    pub aggregation_refusal: Option<Refusal>,
    /// Quotes do not authorize checkout or reserve inventory.
    pub execution_authorized: bool,
}

/// Atomic complete geometric job with independent commercial evidence.
#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    /// `ftx-cutting-job-plan/1`.
    pub schema: &'static str,
    /// Whole frozen pattern identity, shared by both scoped subplans.
    pub pattern_sha256: String,
    /// Exact frozen request identity.
    pub request_sha256: String,
    /// Full original geometry with every effective material selection frozen for replay.
    pub pattern_json: String,
    /// Exact JSON representation of `request`, without a browser numeric round trip.
    pub replay_request_json: String,
    /// Routes, allowances, dated prices and frozen physical inventory.
    pub request: Request,
    /// Complete deterministic original-piece/instance/copy/material inventory.
    pub inventory: Vec<Item>,
    /// Canonical roll subplan, explicitly scoped to its routed source pieces.
    pub roll: Option<CutPlan>,
    /// Canonical finite-stock subplan, explicitly scoped to its routed source pieces.
    pub finite_stock: Option<stock::Plan>,
    /// Canonical identities for authoring exact physical-item offer bindings from this allocation.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub finite_stock_identities: Vec<procurement::finite::Identity>,
    /// Independent source-qualified purchasing, if requested.
    pub purchasing: Option<PriceReport>,
    /// True only after exact expanded-inventory equality across both planners.
    pub complete_inventory: bool,
    /// Gross roll area plus whole used finite-item area; includes waste, defects and prior cuts.
    pub stock_area_m2: f64,
    /// Sum of newly allocated external cutter areas on both routes; holes are not recovered stock.
    pub external_cut_area_m2: f64,
    /// Complete engaged-stock mass, including waste and prior occupied finite stock, or unknown.
    pub stock_mass_kg: Option<f64>,
    /// This plan is not a cutter command or permission to operate hardware.
    pub hardware_execution_authorized: bool,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn encode(value: &impl Serialize) -> Result<String, Refusal> {
    serde_json::to_string(value).map_err(|e| invalid(e.to_string()))
}
fn result<T>(value: Result<T, Refusal>) -> Outcome<T> {
    match value {
        Ok(result) => Outcome::Available { result },
        Err(refusal) => Outcome::Refused { refusal },
    }
}

impl Request {
    /// The complete garment count, checked by [`Self::build`] for equality between routes.
    #[must_use]
    pub fn copies(&self) -> usize {
        self.roll
            .as_ref()
            .map_or(0, |o| o.copies)
            .max(self.finite_stock.as_ref().map_or(0, |r| r.options.copies))
    }

    /// Allocate a complete unchanged pattern through the existing roll and finite-stock planners.
    /// Every physical instance/copy must occur exactly once; one refused route publishes no job.
    /// Missing quotes remain an independent pricing outcome after geometric allocation.
    ///
    /// # Errors
    /// Invalid or incomplete routes, mismatched copies/effective allowances, unsupported material
    /// forms, exhausted canonical planning budgets, or failed exact inventory coverage.
    pub fn build(&self, pattern: &Pattern) -> Result<Plan, Refusal> {
        if self.schema != "ftx-cutting-job/1" || self.routes.len() > 4096 {
            return Err(invalid("invalid cutting-job schema or route budget"));
        }
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(invalid(errors.join("; ")));
        }
        let pieces = pattern.raw()["pieces"]
            .as_array()
            .ok_or_else(|| invalid("cutting job needs source pieces"))?;
        let mut routes = BTreeMap::new();
        for route in &self.routes {
            if route.piece.trim().is_empty()
                || routes.insert(route.piece.as_str(), route.stock).is_some()
            {
                return Err(invalid(
                    "cutting routes need unique nonempty source-piece IDs",
                ));
            }
        }
        if pieces.len() != routes.len()
            || pieces
                .iter()
                .any(|p| p["id"].as_str().is_none_or(|id| !routes.contains_key(id)))
        {
            return Err(invalid(
                "cutting routes must cover every original source piece exactly once",
            ));
        }
        let indices = |kind| {
            pieces
                .iter()
                .enumerate()
                .filter_map(|(i, p)| (routes[p["id"].as_str().unwrap()] == kind).then_some(i))
                .collect::<BTreeSet<_>>()
        };
        let roll_indices = indices(StockKind::Roll);
        let finite_indices = indices(StockKind::FiniteStock);
        if self.roll.is_some() != !roll_indices.is_empty()
            || self.finite_stock.is_some() != !finite_indices.is_empty()
        {
            return Err(invalid(
                "supply exactly the planner options used by the explicit cutting routes",
            ));
        }
        let copies = self.copies();
        if !(1..=4096).contains(&copies)
            || pattern
                .instances
                .len()
                .checked_mul(copies)
                .is_none_or(|n| n == 0 || n > 4096)
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "cutting job needs 1..4096 physical instances across all routes",
            ));
        }
        if let (Some(roll), Some(finite)) = (&self.roll, &self.finite_stock) {
            let allowance = |sa| {
                Ctx::new(
                    pattern,
                    ExportOptions {
                        sa,
                        ..ExportOptions::default()
                    },
                )
                .map(|c| c.sa)
            };
            if roll.copies != finite.options.copies
                || allowance(roll.seam_allowance_mm)?
                    != allowance(finite.options.seam_allowance_mm)?
            {
                return Err(invalid(
                    "cutting routes need the same garment copies and effective seam allowance",
                ));
            }
        }
        let bound = Changes {
            schema: "ftx-material-bindings/1".into(),
            ..Changes::default()
        }
        .apply(pattern)?;
        let pattern = Pattern::new(bound["pattern"].clone());
        let pattern_json = bound["pattern_json"]
            .as_str()
            .ok_or_else(|| invalid("frozen cutting pattern lost its JSON"))?
            .to_owned();
        let pattern_sha256 = bound["pattern_sha256"]
            .as_str()
            .ok_or_else(|| invalid("frozen cutting pattern lost its identity"))?
            .to_owned();
        let roll = self
            .roll
            .as_ref()
            .map(|o| CutPlan::build_scoped(&pattern, o, Some(&roll_indices)))
            .transpose()?;
        let mut frozen = self.clone();
        if let Some(request) = &mut frozen.finite_stock
            && request.options.optimize == Some(stock::search::Objective::MinimizePurchaseCost)
            && let Some(pricing) = &self.purchasing
        {
            let common = pricing.finite_request();
            if let Some(existing) = &request.pricing
                && encode(existing)? != encode(&common)?
            {
                return Err(invalid(
                    "finite-stock cost search must use the complete job's same dated purchasing inputs",
                ));
            }
            request.pricing = Some(common);
        }
        let finite_stock = frozen
            .finite_stock
            .as_ref()
            .map(|r| r.build_scoped(&pattern, Some(&finite_indices)))
            .transpose()?;
        if let Some(plan) = &finite_stock {
            frozen.finite_stock = Some(plan.request().clone());
        }
        let finite_stock_identities = finite_stock
            .as_ref()
            .map(procurement::finite::identities)
            .transpose()?
            .unwrap_or_default();
        let mut expected = BTreeSet::new();
        for instance in &pattern.instances {
            let material = textile_import::resolve_fabric(textile_import::piece_fabric(
                &pattern,
                instance.piece_index,
            )?)?
            .ok_or_else(|| invalid("cutting job lost a material selection"))?;
            let piece = pattern.raw()["pieces"][instance.piece_index]["id"]
                .as_str()
                .unwrap();
            for copy in 0..copies {
                if !expected.insert(Item {
                    piece: piece.into(),
                    instance: instance.id.clone(),
                    copy,
                    stock: routes[piece],
                    material_sha256: material.sha256().into(),
                }) {
                    return Err(invalid("ambiguous duplicate physical cut instance"));
                }
            }
        }
        let mut actual = BTreeSet::new();
        let mut stock_area = 0.0;
        let mut cut_area = 0.0;
        let mut stock_mass = Some(0.0);
        let mut collect = |placement: &super::CutPlacement, kind, sha: &str| {
            actual.insert(Item {
                piece: placement.piece.clone(),
                instance: placement.instance.clone(),
                copy: placement.copy,
                stock: kind,
                material_sha256: sha.into(),
            })
        };
        if let Some(plan) = &roll {
            for marker in &plan.rolls {
                stock_area += marker.stock_area_m2;
                cut_area += marker.external_cut_area_m2;
                stock_mass = stock_mass.zip(marker.stock_mass_kg).map(|(a, b)| a + b);
                for p in &marker.placements {
                    if !collect(p, StockKind::Roll, marker.material.sha256()) {
                        return Err(invalid("duplicate roll cut instance"));
                    }
                }
            }
        }
        if let Some(plan) = &finite_stock {
            for marker in plan.markers().iter().filter(|m| !m.placements.is_empty()) {
                stock_area += marker.gross_area_m2;
                cut_area += marker.external_cut_area_m2;
                stock_mass = stock_mass.zip(marker.stock_mass_kg).map(|(a, b)| a + b);
                for p in &marker.placements {
                    if !collect(p, StockKind::FiniteStock, marker.material.sha256()) {
                        return Err(invalid("duplicate finite-stock cut instance"));
                    }
                }
            }
        }
        if actual != expected {
            return Err(invalid(
                "cutting job does not allocate every physical instance exactly once",
            ));
        }
        if !stock_area.is_finite()
            || !cut_area.is_finite()
            || stock_mass.is_some_and(|x| !x.is_finite())
        {
            return Err(invalid("complete cutting job stock metrics overflow"));
        }
        let purchasing = self
            .purchasing
            .as_ref()
            .map(|p| price(roll.as_ref(), finite_stock.as_ref(), p));
        let replay_request_json = encode(&frozen)?;
        Ok(Plan {
            schema: "ftx-cutting-job-plan/1",
            pattern_sha256,
            request_sha256: sha256_hex(replay_request_json.as_bytes()),
            pattern_json,
            replay_request_json,
            request: frozen,
            inventory: actual.into_iter().collect(),
            roll,
            finite_stock,
            finite_stock_identities,
            purchasing,
            complete_inventory: true,
            stock_area_m2: stock_area,
            external_cut_area_m2: cut_area,
            stock_mass_kg: stock_mass,
            hardware_execution_authorized: false,
        })
    }
}

fn price(
    roll: Option<&CutPlan>,
    finite: Option<&stock::Plan>,
    request: &Purchasing,
) -> PriceReport {
    // A quote explicitly bound to a physical remnant cannot also supply a fungible roll order.
    // Without this separation, two routes using the same sheet snapshot could buy one item twice.
    let physical_offers: BTreeSet<_> = request
        .finite_bindings
        .iter()
        .flat_map(|binding| binding.offer_ids.iter().map(String::as_str))
        .collect();
    let roll_quotes = QuoteBook {
        schema: request.quotes.schema.clone(),
        quotes: request
            .quotes
            .quotes
            .iter()
            .filter(|quote| !physical_offers.contains(quote.offer_id.as_str()))
            .cloned()
            .collect(),
    };
    let roll = match roll {
        Some(plan) => result(procurement::evaluate(plan, &roll_quotes, &request.market)),
        None => Outcome::NotRequested,
    };
    let finite_stock = match finite {
        Some(plan) => result(procurement::finite::evaluate(
            plan,
            &request.finite_request(),
        )),
        None if !request.finite_bindings.is_empty() => Outcome::Refused {
            refusal: invalid("finite offer bindings require finite-stock routes"),
        },
        None => Outcome::NotRequested,
    };
    let mut total = Some(0.0);
    let mut observed = true;
    match &roll {
        Outcome::Available { result } => {
            total = result["least_observed_independent_order_total"].as_f64();
            observed &= result["complete_observed_quantity_coverage"] == true;
        }
        Outcome::Refused { .. } => {
            total = None;
            observed = false;
        }
        Outcome::NotRequested => {}
    }
    match &finite_stock {
        Outcome::Available { result } => {
            total = total
                .zip(result.least_observed_independent_order_total)
                .map(|(a, b)| a + b);
            observed &= result.complete_observed_quantity_coverage;
        }
        Outcome::Refused { .. } => {
            total = None;
            observed = false;
        }
        Outcome::NotRequested => {}
    }
    let aggregation_refusal = total
        .filter(|x| !x.is_finite())
        .map(|_| invalid("complete cutting job order total overflows"));
    if aggregation_refusal.is_some() {
        total = None;
        observed = false;
    }
    PriceReport {
        schema: "ftx-cutting-job-procurement/1",
        request: request.clone(),
        roll,
        finite_stock,
        currency: request.market.currency.clone(),
        complete_offer_coverage: total.is_some(),
        complete_observed_quantity_coverage: observed && total.is_some(),
        least_observed_independent_order_total: total,
        aggregation_refusal,
        execution_authorized: false,
    }
}
