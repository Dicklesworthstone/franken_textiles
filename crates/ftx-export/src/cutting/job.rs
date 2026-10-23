//! Complete garment cutting across roll stock and individually identified hides/remnants.
//! Routes are explicit source-piece or exact-material assignments. Both planners keep the whole source pattern;
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

/// One explicitly admitted supply route for an exact material revision.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialChoice {
    /// Exact lowercase SHA-256 from the shared material resolver, never a material/fiber label.
    pub material_sha256: String,
    /// Declared physical supply route for this snapshot.
    pub stock: StockKind,
}

/// Route a source piece after resolving its actual material assignment.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialRoute {
    /// Source-piece ID, disjoint from every fixed route and other material route.
    pub piece: String,
    /// Complete allowed alternatives. An unlisted revision refuses instead of borrowing a route.
    pub choices: Vec<MaterialChoice>,
}

/// Selected route and material identity for one original source piece.
#[derive(Clone, Debug, Serialize)]
pub struct ResolvedRoute {
    /// Original source-piece ID.
    pub piece: String,
    /// Planner selected by the declared fixed or exact-material rule.
    pub stock: StockKind,
    /// Actual frozen material revision used by that source piece.
    pub material_sha256: String,
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
    /// `ftx-cutting-job/1` for fixed routes, or `/2` for exact-material route alternatives.
    pub schema: String,
    /// Fixed routes; together with `material_routes`, exactly one rule per original source piece.
    #[serde(default)]
    pub routes: Vec<Route>,
    /// Exact-material alternatives in schema `/2`; no material name or stock-form inference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub material_routes: Vec<MaterialRoute>,
    /// Required iff at least one declared route or alternative uses roll stock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll: Option<CutOptions>,
    /// Required iff at least one declared route or alternative uses physical finite stock.
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
    /// For split roll orders, this requires a proved minimum over the admitted assignment search.
    pub least_observed_independent_order_total: Option<f64>,
    /// Complete feasible selected order cost, including a bounded split-order incumbent.
    pub selected_observed_order_total: Option<f64>,
    /// Every engaged roll allocation completed its search; finite-item quotes are evaluated fully.
    pub order_allocation_optimality_proven: bool,
    /// Failure to form a complete finite total; independent route reports remain inspectable.
    pub aggregation_refusal: Option<Refusal>,
    /// Quotes do not authorize checkout or reserve inventory.
    pub execution_authorized: bool,
}

/// Atomic complete geometric job with independent commercial evidence.
#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    /// `ftx-cutting-job-plan/1`, or `/2` when material-following routes are requested.
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
    /// Actual selected source-piece routes in `/2`; full alternatives remain in `request`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resolved_routes: Vec<ResolvedRoute>,
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

struct Routing {
    routes: BTreeMap<String, StockKind>,
    materials: BTreeMap<String, String>,
    permitted: BTreeMap<StockKind, BTreeSet<String>>,
    finite_pieces: BTreeSet<String>,
}

impl Routing {
    fn compile(request: &Request, pattern: &Pattern) -> Result<Self, Refusal> {
        let pieces = pattern.raw()["pieces"]
            .as_array()
            .ok_or_else(|| invalid("cutting job needs source pieces"))?;
        let mut result = Self {
            routes: BTreeMap::new(),
            materials: BTreeMap::new(),
            permitted: BTreeMap::new(),
            finite_pieces: BTreeSet::new(),
        };
        for (i, piece) in pieces.iter().enumerate() {
            let id = piece["id"]
                .as_str()
                .ok_or_else(|| invalid("cutting job needs source-piece IDs"))?;
            let material =
                textile_import::resolve_fabric(textile_import::piece_fabric(pattern, i)?)?
                    .ok_or_else(|| invalid("cutting job needs an explicit material selection"))?;
            if id.trim().is_empty()
                || result
                    .materials
                    .insert(id.into(), material.sha256().into())
                    .is_some()
            {
                return Err(invalid(
                    "cutting job needs unique nonempty source-piece IDs",
                ));
            }
        }
        let mut assigned = BTreeSet::new();
        for route in &request.routes {
            let material = result
                .materials
                .get(&route.piece)
                .ok_or_else(|| invalid("cutting routes must name original source pieces"))?;
            if !assigned.insert(route.piece.clone()) {
                return Err(invalid(
                    "each source piece needs exactly one cutting route rule",
                ));
            }
            result.routes.insert(route.piece.clone(), route.stock);
            result
                .permitted
                .entry(route.stock)
                .or_default()
                .insert(material.clone());
            if route.stock == StockKind::FiniteStock {
                result.finite_pieces.insert(route.piece.clone());
            }
        }
        let mut choice_count = 0usize;
        for route in &request.material_routes {
            let material = result
                .materials
                .get(&route.piece)
                .ok_or_else(|| invalid("material routes must name original source pieces"))?;
            if !assigned.insert(route.piece.clone()) || route.choices.is_empty() {
                return Err(invalid(
                    "material routes need unique pieces and nonempty exact choices",
                ));
            }
            choice_count = choice_count.saturating_add(route.choices.len());
            if choice_count > 4096 {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "cutting route choices exceed 4096 exact material alternatives",
                ));
            }
            let mut choices = BTreeSet::new();
            for choice in &route.choices {
                let hash = &choice.material_sha256;
                if hash.len() != 64
                    || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                    || !choices.insert(hash)
                {
                    return Err(invalid(
                        "material route choices need unique exact lowercase SHA-256 identities",
                    ));
                }
                result
                    .permitted
                    .entry(choice.stock)
                    .or_default()
                    .insert(hash.clone());
                if choice.stock == StockKind::FiniteStock {
                    result.finite_pieces.insert(route.piece.clone());
                }
                if hash == material {
                    result.routes.insert(route.piece.clone(), choice.stock);
                }
            }
            if !result.routes.contains_key(&route.piece) {
                return Err(invalid(format!(
                    "{}: no cutting route declares material snapshot {material}; review the new material revision",
                    route.piece
                )));
            }
        }
        if assigned.len() != pieces.len() {
            return Err(invalid(
                "cutting routes must cover every original source piece exactly once",
            ));
        }
        if request.roll.is_some() != result.permitted.contains_key(&StockKind::Roll)
            || request.finite_stock.is_some()
                != result.permitted.contains_key(&StockKind::FiniteStock)
        {
            return Err(invalid(
                "supply exactly the planner options used by the declared cutting routes",
            ));
        }
        Ok(result)
    }

    fn scope_permissions(
        &self,
        permissions: &mut Vec<String>,
        kind: StockKind,
    ) -> Result<(), Refusal> {
        let allowed = self.permitted.get(&kind);
        let mut seen = BTreeSet::new();
        if permissions.len() > 4096
            || permissions.iter().any(|hash| {
                !seen.insert(hash.clone()) || allowed.is_none_or(|pool| !pool.contains(hash))
            })
        {
            return Err(invalid(
                "grain-reversal pools need unique exact materials declared for that stock route",
            ));
        }
        permissions.retain(|hash| {
            self.routes
                .iter()
                .any(|(piece, route)| *route == kind && self.materials[piece] == *hash)
        });
        Ok(())
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
        let material_following = self.schema == "ftx-cutting-job/2";
        if (!material_following && self.schema != "ftx-cutting-job/1")
            || (!material_following && !self.material_routes.is_empty())
            || self.routes.len().saturating_add(self.material_routes.len()) > 4096
        {
            return Err(invalid("invalid cutting-job schema or route budget"));
        }
        if material_following && let Some(inventory) = &self.finite_stock {
            inventory.validate_geometry_budget()?;
        }
        let errors = pattern.structure_errors();
        if !errors.is_empty() {
            return Err(invalid(errors.join("; ")));
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
        let pieces = pattern.raw()["pieces"]
            .as_array()
            .ok_or_else(|| invalid("cutting job needs source pieces"))?;
        let routing = Routing::compile(self, &pattern)?;
        let routes = &routing.routes;
        let indices = |kind| {
            pieces
                .iter()
                .enumerate()
                .filter_map(|(i, p)| (routes[p["id"].as_str().unwrap()] == kind).then_some(i))
                .collect::<BTreeSet<_>>()
        };
        let roll_indices = indices(StockKind::Roll);
        let finite_indices = indices(StockKind::FiniteStock);
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
                    &pattern,
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
        if material_following && let Some(request) = &mut frozen.finite_stock {
            if request.schema != "ftx-finite-stock-request/1"
                || request.stocks.is_empty()
                || request.stocks.len() > 128
                || !(1..=20_000_000).contains(&request.options.work_limit)
            {
                return Err(invalid(
                    "material-following inventory needs the finite-stock schema, 1..128 physical items and bounded work inputs",
                ));
            }
            let db = textile_import::catalog()?;
            let mut ids = BTreeSet::new();
            for stock in &mut request.stocks {
                if stock.id.trim().is_empty() || !ids.insert(stock.id.clone()) {
                    return Err(invalid("physical stock IDs must be nonempty and unique"));
                }
                stock.material = db.resolve(&stock.material)?.frozen_selection();
            }
        }
        let mut scoped_roll = frozen.roll.clone();
        let mut scoped_finite = frozen.finite_stock.clone();
        if material_following {
            if let Some(options) = &mut scoped_roll {
                options.validate()?;
                if let Some(nesting) = &mut options.nesting {
                    routing.scope_permissions(
                        &mut nesting.grain_reversal_materials,
                        StockKind::Roll,
                    )?;
                }
            }
            if let Some(request) = &mut scoped_finite {
                routing.scope_permissions(
                    &mut request.options.grain_reversal_materials,
                    StockKind::FiniteStock,
                )?;
                if !request.options.zone_margin_mm.is_finite()
                    || request.options.zone_margin_mm < 0.0
                    || (request.options.zone_margin_mm > 0.0
                        && request.options.piece_zones.is_empty())
                    || request.options.piece_zones.iter().any(|(piece, zone)| {
                        !routing.finite_pieces.contains(piece) || zone.trim().is_empty()
                    })
                {
                    return Err(invalid(
                        "finite-stock zone pools need declared finite-route pieces, nonempty zones and a finite nonnegative margin",
                    ));
                }
                request
                    .options
                    .piece_zones
                    .retain(|piece, _| routes[piece] == StockKind::FiniteStock);
                if request.options.piece_zones.is_empty() {
                    request.options.zone_margin_mm = 0.0;
                }
            }
        }
        let roll = scoped_roll
            .as_ref()
            .filter(|_| !roll_indices.is_empty())
            .map(|o| CutPlan::build_scoped(&pattern, o, Some(&roll_indices)))
            .transpose()?;
        let finite_stock = scoped_finite
            .as_ref()
            .filter(|_| !finite_indices.is_empty())
            .map(|r| r.build_scoped(&pattern, Some(&finite_indices)))
            .transpose()?;
        if let Some(plan) = &finite_stock {
            if material_following {
                // Freeze inventory without losing permissions for the other declared alternatives.
                frozen.finite_stock.as_mut().unwrap().stocks = plan.request().stocks.clone();
            } else {
                frozen.finite_stock = Some(plan.request().clone());
            }
        }
        let finite_stock_identities = finite_stock
            .as_ref()
            .map(procurement::finite::identities)
            .transpose()?
            .unwrap_or_default();
        let mut expected = BTreeSet::new();
        for instance in &pattern.instances {
            let piece = pattern.raw()["pieces"][instance.piece_index]["id"]
                .as_str()
                .unwrap();
            for copy in 0..copies {
                if !expected.insert(Item {
                    piece: piece.into(),
                    instance: instance.id.clone(),
                    copy,
                    stock: routes[piece],
                    material_sha256: routing.materials[piece].clone(),
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
        let purchasing = self.purchasing.as_ref().map(|p| {
            price(
                roll.as_ref(),
                finite_stock.as_ref(),
                p,
                frozen.finite_stock.as_ref().filter(|_| material_following),
            )
        });
        let replay_request_json = encode(&frozen)?;
        let resolved_routes = if material_following {
            routes
                .iter()
                .map(|(piece, stock)| ResolvedRoute {
                    piece: piece.clone(),
                    stock: *stock,
                    material_sha256: routing.materials[piece].clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(Plan {
            schema: if material_following {
                "ftx-cutting-job-plan/2"
            } else {
                "ftx-cutting-job-plan/1"
            },
            pattern_sha256,
            request_sha256: sha256_hex(replay_request_json.as_bytes()),
            pattern_json,
            replay_request_json,
            request: frozen,
            resolved_routes,
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
    dormant_inventory: Option<&stock::Request>,
) -> PriceReport {
    if finite.is_none()
        && let Some(inventory) = dormant_inventory
    {
        let ids = inventory.stocks.iter().map(|s| s.id.as_str()).collect();
        if let Err(refusal) =
            procurement::finite::validate_bindings(&request.quotes, &request.finite_bindings, &ids)
        {
            return PriceReport {
                schema: "ftx-cutting-job-procurement/1",
                request: request.clone(),
                roll: Outcome::Refused {
                    refusal: refusal.clone(),
                },
                finite_stock: Outcome::Refused {
                    refusal: refusal.clone(),
                },
                currency: request.market.currency.clone(),
                complete_offer_coverage: false,
                complete_observed_quantity_coverage: false,
                least_observed_independent_order_total: None,
                selected_observed_order_total: None,
                order_allocation_optimality_proven: false,
                aggregation_refusal: Some(refusal),
                execution_authorized: false,
            };
        }
    }
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
        None if dormant_inventory.is_none() && !request.finite_bindings.is_empty() => {
            Outcome::Refused {
                refusal: invalid("finite offer bindings require finite-stock routes"),
            }
        }
        None => Outcome::NotRequested,
    };
    let mut total = Some(0.0);
    let mut selected = Some(0.0);
    let mut observed = true;
    match &roll {
        Outcome::Available { result } => {
            total = result["least_observed_independent_order_total"].as_f64();
            selected = if result["schema"] == "ftx-cut-procurement/3" {
                result["selected_observed_order_total"].as_f64()
            } else {
                total
            };
            observed &= result["complete_observed_quantity_coverage"] == true;
        }
        Outcome::Refused { .. } => {
            total = None;
            selected = None;
            observed = false;
        }
        Outcome::NotRequested => {}
    }
    match &finite_stock {
        Outcome::Available { result } => {
            total = total
                .zip(result.least_observed_independent_order_total)
                .map(|(a, b)| a + b);
            selected = selected
                .zip(result.least_observed_independent_order_total)
                .map(|(a, b)| a + b);
            observed &= result.complete_observed_quantity_coverage;
        }
        Outcome::Refused { .. } => {
            total = None;
            selected = None;
            observed = false;
        }
        Outcome::NotRequested => {}
    }
    let aggregation_refusal = total
        .into_iter()
        .chain(selected)
        .any(|x| !x.is_finite())
        .then(|| invalid("complete cutting job order total overflows"));
    if aggregation_refusal.is_some() {
        total = None;
        selected = None;
        observed = false;
    }
    PriceReport {
        schema: "ftx-cutting-job-procurement/1",
        request: request.clone(),
        roll,
        finite_stock,
        currency: request.market.currency.clone(),
        complete_offer_coverage: selected.is_some(),
        complete_observed_quantity_coverage: observed && selected.is_some(),
        least_observed_independent_order_total: total,
        selected_observed_order_total: selected,
        order_allocation_optimality_proven: total.is_some(),
        aggregation_refusal,
        execution_authorized: false,
    }
}
