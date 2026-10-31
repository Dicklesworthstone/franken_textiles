//! Bounded recipe selection using the production sewing planner, not a second feasibility model.
//! Explicit needle/thread/pitch/rate choices and opt-in complete specialist routes may vary.
//! Pattern geometry, assembly, pressure, material evidence, backtacks and prerequisites stay fixed.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::Pattern;
use serde::{Deserialize, Serialize};

use crate::sewing::{SewOptions, SewPlan, SewingPass, consumables, ordering, production::routing};

pub mod deadline;
mod thread_cost;

const MAX_TRIALS: usize = 64;
const MAX_OPERATIONS: usize = 64;
const MAX_GROUPS: usize = 12;
const MAX_ALTERNATIVES: usize = 15;
const MAX_ORDERING_NODES: usize = 200_000;
const MAX_PURCHASING_WORK: usize = 200_000;
const TEMPLATE: &str = "template";
const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;

/// An explicit process candidate, never inferred from a textile's brand or composition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// Stable choice identity within an operation; `template` is reserved for the original pass.
    pub id: String,
    /// Needle identifier, matched against existing source qualifications by the production planner.
    pub needle: String,
    /// Thread identifier, under the same exact-match policy as an ordinary sewing job.
    pub thread: String,
    /// Requested maximum stitch advance in millimetres; actual stations are regenerated and checked.
    pub max_stitch_mm: f64,
    /// Requested continuous stitch rate, not a total production-time estimate.
    pub stitches_per_minute: f64,
}

impl Recipe {
    pub(crate) fn apply(&self, pass: &mut SewingPass) {
        pass.needle.clone_from(&self.needle);
        pass.thread.clone_from(&self.thread);
        pass.max_stitch_mm = self.max_stitch_mm;
        pass.stitches_per_minute = self.stitches_per_minute;
    }
}

/// Alternatives to one existing operation. Its original recipe is always the first candidate.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alternatives {
    /// Exact existing operation ID; seam names cannot replace distinct pass IDs.
    pub operation: String,
    /// One to fifteen explicit choices, considered in ID order after the original recipe.
    pub recipes: Vec<Recipe>,
}

/// One independently qualified complete specialist route, never inferred from machine capabilities.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteCandidate {
    /// Stable candidate identity; `template` is reserved for the original route.
    pub id: String,
    /// Complete ordinary routing policy, including actual cell configurations and source locks.
    pub routing: routing::Policy,
}

/// Explicit objective with no hidden seam-quality or setup-duration weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    /// Minimize actual generated continuous-feed seconds, counting locks and repeated travel.
    MinimizeFeedSeconds,
    /// Minimize generated feed plus source-qualified setup seconds, including initial setup.
    /// Requires the timed ordering objective; neither term includes handling or final reset.
    MinimizeFeedAndSetupSeconds,
    /// Minimize serial feed, setup and source-qualified copy/operation handling, including endpoints.
    MinimizeModeledProductionSeconds,
    /// Minimize elapsed whole-garment allocation or fixed-route specialist-cell completion under
    /// the explicitly qualified availability, resource and millisecond planning policy.
    MinimizeParallelMakespan,
    /// Minimize complete source-locked thread orders from the shared consumables price result.
    /// Unknown consumption or incomplete eligible offer coverage cannot supply a score.
    MinimizeThreadObservedOrderCost,
}

/// Search a finite product of recipes and optional routes, retaining the original complete job.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// `/1` feed; `/2` feed plus setup; `/3` thread cost; `/4` serial time; `/5` parallel makespan;
    /// `/6` makespan or complete thread cost across authored specialist routes.
    pub schema: String,
    /// Complete template job. `require_qualification` must be true; it is never silently enabled.
    pub options: SewOptions,
    /// One to twelve operations with explicit alternatives; may be empty for route selection `/6`.
    #[serde(default)]
    pub alternatives: Vec<Alternatives>,
    /// One to fifteen complete alternative routes in `/6`; omission preserves existing selection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub route_candidates: Vec<RouteCandidate>,
    /// Explicit feed, feed-plus-setup or complete observed thread-order objective.
    pub objective: Objective,
    /// Maximum complete-plan evaluations, including refused trials and the original template, 1..64.
    pub max_plan_evaluations: usize,
    /// Optional inclusive completion limit for the complete declared production job, measured from
    /// its modeled time origin. Unknown handling or an unproved miss cannot establish infeasibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_deadline_milliseconds: Option<u64>,
}

/// Compact outcome of one complete job trial; refused jobs are never omitted from the count.
#[derive(Clone, Debug, Serialize)]
pub struct Trial {
    /// Operation ID to candidate ID, with `template` denoting an unchanged recipe.
    pub choices: BTreeMap<String, String>,
    /// Complete route identity in `/6`, including `template`; absent for existing recipe searches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_choice: Option<String>,
    /// Actual generated feed time when the entire job was admitted.
    pub feed_seconds: Option<f64>,
    /// Total traversed intervals, including backtacks, when admitted.
    pub stitch_intervals: Option<usize>,
    /// Original shared-planner refusal; no weakened retry or manufactured feasible result.
    pub refusal: Option<Refusal>,
    /// Actual joint score when admitted in timed mode; omitted in feed-only mode or on refusal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective_seconds: Option<f64>,
    /// Inner schedule proof for a timed trial. False retains a complete incumbent, not an optimum.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordering_optimality_proven: Option<bool>,
    /// Canonical complete thread-order score, in the request's one unchanged market currency.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_order_cost: Option<f64>,
    /// Inner supplier-allocation proof when split thread orders were requested. An unproved
    /// complete incumbent can win the observed comparison but cannot prove the recipe optimum.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_order_optimality_proven: Option<bool>,
    /// Complete source timing, consumption or eligible prices are unavailable. Timing admission may
    /// fail before production; this is unresolved objective evidence, never a zero or infeasibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective_unavailable: Option<Refusal>,
    /// Independent completion constraint using the shared planner's timing and proof authority.
    /// An excluded or unresolved trial retains its original objective values for inspection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_deadline: Option<deadline::Report>,
}

/// Complete incumbent plus explicit finite-search status. A refused template does not stop repair.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `/1` feed; `/2` feed/setup time; `/3` thread cost; `/4` serial time; `/5` parallel time;
    /// `/6` complete authored route and recipe selection; `/7` a completion-constrained search.
    pub schema: &'static str,
    /// Original request, including all candidate settings and independent qualification evidence.
    pub request: Request,
    /// Digest of the complete original pattern, including its material assignments.
    pub pattern_sha256: String,
    /// Number of complete combinations in the explicitly supplied discrete search space.
    pub candidate_combinations: u64,
    /// Every examined full job, starting with the template, in stable canonical order.
    pub trials: Vec<Trial>,
    /// Choices producing the complete incumbent; absent when nothing examined was admissible.
    pub selected_choices: Option<BTreeMap<String, String>>,
    /// Complete incumbent route identity in `/6`; absent without an incumbent or route search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_route: Option<String>,
    /// Exact production plan, retaining material/qualification/pressure/ordering receipts.
    pub selected_plan: Option<SewPlan>,
    /// Joint feed-and-setup score for the incumbent; omitted in feed-only mode or with no incumbent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_objective_seconds: Option<f64>,
    /// Canonical complete thread-order total for the incumbent, omitted for time objectives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_thread_order_cost: Option<f64>,
    /// Exact common thread market currency for cost mode, including when no incumbent exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective_currency: Option<String>,
    /// Resource/convergence limits, unresolved deadlines, unproved timed schedules, or unavailable
    /// complete cost evidence among trials not independently excluded by the completion constraint.
    /// They are not infeasible recipes; a complete admitted incumbent may still be retained.
    pub unresolved_trials: usize,
    /// Every supplied combination was examined; no assertion about unlisted process recipes.
    pub search_complete: bool,
    /// Work stopped before exhausting the supplied combinations, with or without an incumbent.
    pub budget_exhausted: bool,
    /// True only with a feasible plan, exhaustive recipe search and no unresolved plan trials.
    /// Timed mode additionally requires every deadline-eligible trial's inner schedule to be proved
    /// optimal; a complete feasible schedule alone suffices to meet a deadline in feed/cost mode.
    /// Feed-only mode remains independent of the ordering optimizer's separate proof.
    pub optimality_proven: bool,
    /// All listed combinations were definitively refused or excluded by the completion constraint,
    /// without resource/convergence exhaustion;
    /// does not establish physical or global infeasibility.
    pub no_feasible_listed_recipe: bool,
    /// Equal objective values retain the first admitted combination, preferring the original job.
    pub tie_break: &'static str,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 4096
}

struct Group<'a> {
    pass: usize,
    operation: &'a str,
    recipes: Vec<&'a Recipe>,
}

fn check_timing_choices(
    policy: &ordering::Policy,
    options: &SewOptions,
    groups: &[Group<'_>],
    fixed: Option<&BTreeMap<String, Recipe>>,
) -> Result<(), Refusal> {
    let Some(times) = &policy.setup_times else {
        return Ok(());
    };
    let initial = policy
        .initial_setup
        .as_ref()
        .ok_or_else(|| invalid("timed recipe ordering requires an explicit initial setup"))?;
    if !matches!(
        policy.objective,
        ordering::Objective::MinimizeSetupTime | ordering::Objective::MinimizeProductionTime
    ) {
        return Err(invalid("setup times require the timed ordering objective"));
    }
    let choices: BTreeSet<_> = options
        .passes
        .iter()
        .map(|pass| ordering::Setup {
            needle: pass.needle.clone(),
            thread: pass.thread.clone(),
        })
        .chain(
            groups
                .iter()
                .filter(|group| {
                    options
                        .passes
                        .iter()
                        .any(|pass| pass.operation_id() == group.operation)
                })
                .filter(|group| fixed.is_none_or(|recipes| !recipes.contains_key(group.operation)))
                .flat_map(|group| {
                    group.recipes.iter().map(|recipe| ordering::Setup {
                        needle: recipe.needle.clone(),
                        thread: recipe.thread.clone(),
                    })
                }),
        )
        .collect();
    times.check_choices(&options.machine.id, &choices, initial)
}

fn check_route_timing(options: &SewOptions, groups: &[Group<'_>]) -> Result<(), Refusal> {
    if let Some(route) = options
        .production
        .as_ref()
        .and_then(|production| production.routing.as_ref())
    {
        for cell in &route.cells {
            let configured = routing::source_options(cell, options, &route.route)?;
            check_timing_choices(
                &cell.ordering,
                &configured,
                groups,
                cell.configuration.as_ref().map(|source| &source.recipes),
            )?;
        }
    }
    Ok(())
}

fn check_candidate_timing(options: &SewOptions) -> Result<(), Refusal> {
    let route = options
        .production
        .as_ref()
        .and_then(|production| production.routing.as_ref())
        .ok_or_else(|| invalid("route candidate has no complete routing policy"))?;
    if route
        .cells
        .iter()
        .any(|cell| cell.ordering.initial_setup.is_none() || cell.ordering.setup_times.is_none())
    {
        return Err(invalid(
            "route candidate lacks explicit initial setup or source-qualified setup times",
        ));
    }
    check_route_timing(options, &[])
}

impl Request {
    fn options_for_route(&self, candidate: &RouteCandidate) -> Result<SewOptions, Refusal> {
        let mut options = self.options.clone();
        let production = options.production.as_mut().ok_or_else(|| {
            invalid("route selection requires an explicit production.routing template")
        })?;
        if production.routing.is_none() || production.parallel.is_some() {
            return Err(invalid(
                "route selection requires a specialist routing policy without a parallel policy",
            ));
        }
        production.routing = Some(candidate.routing.clone());
        Ok(options)
    }

    fn check_route_count(&self) -> Result<(), Refusal> {
        if self.route_candidates.len() > MAX_ALTERNATIVES {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "sewing selection supports at most fifteen alternative complete routes",
            ));
        }
        Ok(())
    }

    /// Largest retained source-plan count among the template and all complete route candidates.
    #[must_use]
    pub fn source_plan_count(&self) -> usize {
        self.route_candidates
            .iter()
            .map(|candidate| 1usize.saturating_add(candidate.routing.cells.len()))
            .fold(self.options.source_plan_count(), usize::max)
    }

    /// Reserve every configured complete-plan evaluation at the largest candidate scheduling cap.
    /// Studies multiply this entire bound by their expanded material/design alternatives.
    ///
    /// # Errors
    /// Invalid candidate bounds, conflicting quantities or more than 200000 total work units.
    pub fn scheduling_work_bound(&self) -> Result<usize, Refusal> {
        self.check_route_count()?;
        let mut per_plan = self.options.scheduling_work_bound()?;
        for candidate in &self.route_candidates {
            per_plan = per_plan.max(self.options_for_route(candidate)?.scheduling_work_bound()?);
        }
        self.max_plan_evaluations
            .checked_mul(per_plan)
            .filter(|work| *work <= MAX_ORDERING_NODES)
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "combined recipe/route/ordering search exceeds 200000 configured ordering nodes",
                )
            })
    }

    /// Reserve the complete configured recipe search's thread-purchasing work before evaluation.
    /// Cost mode additionally validates the common canonical market once, visiting every quote.
    /// Studies must multiply this reservation by all expanded material/design rows.
    ///
    /// # Errors
    /// Unsupported thread inputs, arithmetic overflow or more than 200000 total work units.
    pub fn purchasing_work_bound(&self) -> Result<usize, Refusal> {
        self.check_route_count()?;
        let mut per_plan = consumables::purchasing_work_bound(&self.options)?;
        for candidate in &self.route_candidates {
            per_plan = per_plan.max(consumables::purchasing_work_bound(
                &self.options_for_route(candidate)?,
            )?);
        }
        let preflight = if self.objective == Objective::MinimizeThreadObservedOrderCost {
            thread_cost::purchasing(&self.options)?
                .quote_book
                .quotes
                .len()
        } else {
            0
        };
        self.max_plan_evaluations
            .checked_mul(per_plan)
            .and_then(|work| work.checked_add(preflight))
            .filter(|work| *work <= MAX_PURCHASING_WORK)
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "combined recipe/thread purchasing exceeds 200000 configured quote/stock-alias work units",
                )
            })
    }

    fn groups(&self) -> Result<Vec<Group<'_>>, Refusal> {
        let route_search = self.schema == "ftx-sewing-selection/6";
        let schema = match self.objective {
            Objective::MinimizeFeedSeconds => "ftx-sewing-selection/1",
            Objective::MinimizeFeedAndSetupSeconds => "ftx-sewing-selection/2",
            Objective::MinimizeModeledProductionSeconds => "ftx-sewing-selection/4",
            Objective::MinimizeParallelMakespan if route_search => "ftx-sewing-selection/6",
            Objective::MinimizeParallelMakespan => "ftx-sewing-selection/5",
            Objective::MinimizeThreadObservedOrderCost if route_search => "ftx-sewing-selection/6",
            Objective::MinimizeThreadObservedOrderCost => "ftx-sewing-selection/3",
        };
        if self.schema != schema || !self.options.require_qualification {
            return Err(invalid(
                "sewing selection needs schema /1 for feed, /2 for feed plus setup, /3 for thread cost, /4 for serial production time, /5 for parallel makespan or /6 for authored-route makespan or fixed-route thread cost, and explicit require_qualification=true",
            ));
        }
        if route_search == self.route_candidates.is_empty() {
            return Err(invalid(
                "route_candidates are required exclusively with sewing selection /6",
            ));
        }
        self.check_route_count()?;
        let mut route_ids = BTreeSet::new();
        for candidate in &self.route_candidates {
            if !text(&candidate.id) || candidate.id == TEMPLATE || !route_ids.insert(&candidate.id)
            {
                return Err(invalid(
                    "route candidates need unique nonreserved identities",
                ));
            }
        }
        let has_eligible_route = self
            .options
            .production
            .as_ref()
            .and_then(|production| production.routing.as_ref())
            .is_some_and(routing::Policy::eligible)
            || self
                .route_candidates
                .iter()
                .any(|candidate| candidate.routing.eligible());
        if has_eligible_route
            && matches!(
                self.objective,
                Objective::MinimizeFeedSeconds | Objective::MinimizeThreadObservedOrderCost
            )
        {
            return Err(invalid(
                "eligible-route cells assign operations for minimum makespan; feed and thread cost depend on those choices and cannot claim their own minimum; use minimize_parallel_makespan and inspect actual feed and thread orders",
            ));
        }
        if !(1..=MAX_TRIALS).contains(&self.max_plan_evaluations)
            || self.options.passes.is_empty()
            || self.options.passes.len() > MAX_OPERATIONS
            || (!route_search && self.alternatives.is_empty())
            || self.alternatives.len() > MAX_GROUPS
            || self
                .alternatives
                .iter()
                .any(|g| g.recipes.is_empty() || g.recipes.len() > MAX_ALTERNATIVES)
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "sewing selection supports 1..64 plans, at most 64 operations, up to 12 recipe groups and 1..15 alternatives per group; recipe-only searches need at least one group",
            ));
        }
        // Ordering is separate from recipe selection. Bound its aggregate configured work without
        // reducing the caller's per-job budget or confusing ordering exhaustion with recipe proof.
        self.scheduling_work_bound()?;
        self.purchasing_work_bound()?;
        if self.objective == Objective::MinimizeThreadObservedOrderCost {
            thread_cost::validate(&self.options)?;
        }
        let mut ids = BTreeMap::new();
        for (index, pass) in self.options.passes.iter().enumerate() {
            if !text(pass.operation_id()) || ids.insert(pass.operation_id(), index).is_some() {
                return Err(invalid(
                    "sewing template needs unique nonempty operation IDs",
                ));
            }
        }
        let mut seen = BTreeSet::new();
        let mut groups = Vec::new();
        for alternatives in &self.alternatives {
            let Some(&pass) = ids.get(alternatives.operation.as_str()) else {
                return Err(invalid(format!(
                    "unknown sewing operation {} in recipe alternatives",
                    alternatives.operation
                )));
            };
            if !seen.insert(&alternatives.operation) {
                return Err(invalid("duplicate operation in sewing alternatives"));
            }
            let mut choices = BTreeSet::new();
            for recipe in &alternatives.recipes {
                if !text(&recipe.id)
                    || recipe.id == TEMPLATE
                    || !choices.insert(&recipe.id)
                    || !text(&recipe.needle)
                    || !text(&recipe.thread)
                    || !recipe.max_stitch_mm.is_finite()
                    || recipe.max_stitch_mm <= 0.0
                    || !recipe.stitches_per_minute.is_finite()
                    || recipe.stitches_per_minute <= 0.0
                {
                    return Err(invalid("invalid or duplicate sewing recipe candidate"));
                }
            }
            let mut recipes: Vec<_> = alternatives.recipes.iter().collect();
            recipes.sort_by(|a, b| a.id.cmp(&b.id));
            groups.push(Group {
                pass,
                operation: &alternatives.operation,
                recipes,
            });
        }
        groups.sort_by(|a, b| a.operation.cmp(b.operation));
        let timed = self.objective == Objective::MinimizeFeedAndSetupSeconds;
        let handled = self.objective == Objective::MinimizeModeledProductionSeconds;
        let parallel = self
            .options
            .production
            .as_ref()
            .and_then(|production| production.parallel.as_ref());
        let routing = self
            .options
            .production
            .as_ref()
            .and_then(|production| production.routing.as_ref());
        if parallel.is_some_and(|policy| policy.model == "independent_whole_garment_cells/2")
            && matches!(
                self.objective,
                Objective::MinimizeFeedSeconds | Objective::MinimizeThreadObservedOrderCost
            )
        {
            return Err(invalid(
                "heterogeneous cells allocate garments for minimum makespan; feed and thread cost depend on that allocation and cannot claim their own minimum; use minimize_parallel_makespan and inspect the actual feed and thread orders",
            ));
        }
        if (timed || handled) && (parallel.is_some() || routing.is_some()) {
            return Err(invalid(
                "serial time objectives cannot score a parallel-cell or routed plan; use minimize_parallel_makespan",
            ));
        }
        if self.objective == Objective::MinimizeParallelMakespan
            && parallel.is_none()
            && routing.is_none()
        {
            return Err(invalid(
                "parallel makespan selection requires a qualified production.parallel or production.routing policy",
            ));
        }
        if timed
            && self.options.ordering_policy().is_none_or(|policy| {
                policy.objective != ordering::Objective::MinimizeSetupTime
                    || policy.initial_setup.is_none()
                    || policy.setup_times.is_none()
            })
        {
            return Err(invalid(
                "joint timed selection requires minimize_setup_time ordering with initial_setup and source setup_times",
            ));
        }
        if handled
            && self.options.production.as_ref().is_none_or(|production| {
                production.handling.is_none()
                    || production.ordering.objective != ordering::Objective::MinimizeProductionTime
                    || production.ordering.initial_setup.is_none()
                    || production.ordering.setup_times.is_none()
            })
        {
            return Err(invalid(
                "modeled production selection requires qualified handling, minimize_production_time ordering, explicit initial setup and setup times",
            ));
        }
        if let Some(policy) = self.options.ordering_policy()
            && routing.is_none()
        {
            check_timing_choices(policy, &self.options, &groups, None)?;
        }
        if let Some(parallel) = parallel {
            for cell in &parallel.cells {
                let configured = cell.configured_options(&self.options)?;
                check_timing_choices(
                    &cell.ordering,
                    configured.as_ref().unwrap_or(&self.options),
                    &groups,
                    cell.configuration.as_ref().map(|source| &source.recipes),
                )?;
            }
        }
        if !route_search {
            check_route_timing(&self.options, &groups)?;
        }
        Ok(groups)
    }

    /// Select among explicitly declared recipes by evaluating complete qualified production plans.
    /// Every trial uses the original pattern and job constraints; no property, allowance, dependency,
    /// machine or evidence policy is relaxed. Selected options can enter ordinary sew-stitches/SVG.
    ///
    /// # Errors
    /// Malformed search request or exceeded configured search caps. Per-job geometry, qualification
    /// and resource refusals instead remain in `trials`, consuming one evaluation each. No incumbent
    /// after interrupted search is not a proof that all listed choices fail.
    pub fn select(&self, pattern: &Pattern) -> Result<Report, Refusal> {
        if self
            .completion_deadline_milliseconds
            .is_some_and(|limit| limit > deadline::MAX_MILLISECONDS)
        {
            return Err(invalid(
                "completion deadline exceeds the exact native/browser millisecond clock",
            ));
        }
        let groups = self.groups()?;
        let mut routes: Vec<_> = self.route_candidates.iter().collect();
        routes.sort_by(|a, b| a.id.cmp(&b.id));
        let route_search = !routes.is_empty();
        let combinations = groups
            .iter()
            .try_fold((routes.len() + 1) as u64, |n, g| {
                n.checked_mul((g.recipes.len() + 1) as u64)
            })
            .ok_or_else(|| invalid("sewing candidate count overflow"))?;
        let pattern_sha256 =
            sha256_hex(&serde_json::to_vec(pattern.raw()).map_err(|e| invalid(e.to_string()))?);
        let mut trials = Vec::new();
        let mut selected_plan: Option<SewPlan> = None;
        let mut selected_choices = None;
        let mut selected_route = None;
        let handled = self.objective == Objective::MinimizeModeledProductionSeconds;
        let parallel = self.objective == Objective::MinimizeParallelMakespan;
        let routed = self
            .options
            .production
            .as_ref()
            .is_some_and(|production| production.routing.is_some());
        let timed = self.objective == Objective::MinimizeFeedAndSetupSeconds || handled || parallel;
        let cost = self.objective == Objective::MinimizeThreadObservedOrderCost;
        let objective_currency = if cost {
            Some(
                thread_cost::purchasing(&self.options)?
                    .market
                    .currency
                    .clone(),
            )
        } else {
            None
        };
        let mut selected_score = None;
        let mut indices = vec![0; groups.len()];
        let mut route_index = 0;
        loop {
            let mut options = if route_index == 0 {
                self.options.clone()
            } else {
                self.options_for_route(routes[route_index - 1])?
            };
            let mut choices = BTreeMap::new();
            for (group, &index) in groups.iter().zip(&indices) {
                let id = if index == 0 {
                    TEMPLATE
                } else {
                    let recipe = group.recipes[index - 1];
                    recipe.apply(&mut options.passes[group.pass]);
                    &recipe.id
                };
                choices.insert(group.operation.to_owned(), id.to_owned());
            }
            let mut trial = Trial {
                choices,
                route_choice: route_search.then(|| {
                    if route_index == 0 {
                        TEMPLATE.to_owned()
                    } else {
                        routes[route_index - 1].id.clone()
                    }
                }),
                feed_seconds: None,
                stitch_intervals: None,
                refusal: None,
                objective_seconds: None,
                ordering_optimality_proven: None,
                thread_order_cost: None,
                thread_order_optimality_proven: None,
                objective_unavailable: None,
                completion_deadline: self
                    .completion_deadline_milliseconds
                    .map(deadline::Report::unavailable),
            };
            // A route owns its timing evidence. An unavailable alternative cannot suppress an
            // independently qualified incumbent or be treated as a proved inferior schedule.
            let timing = if route_search {
                check_candidate_timing(&options).inspect_err(|refusal| {
                    trial.objective_unavailable = Some(refusal.clone());
                })
            } else {
                Ok(())
            };
            match timing.and_then(|()| SewPlan::build(pattern, &options)) {
                Ok(plan) => {
                    trial.feed_seconds = Some(plan.total_feed_seconds());
                    trial.stitch_intervals = Some(plan.total_stitch_intervals());
                    let score = if let Some(currency) = objective_currency.as_deref() {
                        match thread_cost::score(&plan, currency) {
                            Ok((score, proved)) => {
                                trial.thread_order_cost = Some(score);
                                trial.thread_order_optimality_proven = proved;
                                Some(score)
                            }
                            Err(refusal) => {
                                trial.objective_unavailable = Some(refusal);
                                None
                            }
                        }
                    } else if parallel {
                        let seconds = plan.parallel_makespan_seconds().ok_or_else(|| {
                            invalid(
                                "admitted multi-cell recipe lost its complete production schedule",
                            )
                        })?;
                        trial.objective_seconds = Some(seconds);
                        trial.ordering_optimality_proven = plan.parallel_optimality_proven();
                        Some(seconds)
                    } else if timed {
                        let schedule = plan
                            .ordering_report()
                            .ok_or_else(|| invalid("admitted timed recipe lost its schedule"))?;
                        let time = schedule.timing.as_ref().ok_or_else(|| {
                            invalid("admitted timed recipe lost its duration accounting")
                        })?;
                        let seconds = if handled {
                            schedule.handling_timing.as_ref().ok_or_else(|| invalid("admitted modeled production recipe lost its handling accounting"))?.selected_modeled_production_seconds
                        } else {
                            time.feed_and_setup_seconds
                        };
                        trial.objective_seconds = Some(seconds);
                        trial.ordering_optimality_proven = Some(schedule.optimality_proven);
                        Some(seconds)
                    } else {
                        Some(plan.total_feed_seconds())
                    };
                    if score.is_some_and(|score| !score.is_finite()) {
                        return Err(invalid("nonrepresentable sewing selection objective"));
                    }
                    trial.completion_deadline = self
                        .completion_deadline_milliseconds
                        .map(|limit| deadline::Report::evaluate(&plan, limit));
                    let meets_deadline = trial
                        .completion_deadline
                        .as_ref()
                        .is_none_or(|receipt| receipt.status == deadline::Status::Feasible);
                    if let Some(score) = score
                        && meets_deadline
                        && selected_score.is_none_or(|old| score < old)
                    {
                        selected_score = Some(score);
                        selected_choices = Some(trial.choices.clone());
                        selected_route.clone_from(&trial.route_choice);
                        selected_plan = Some(plan);
                    }
                }
                Err(refusal) => trial.refusal = Some(refusal),
            }
            trials.push(trial);
            if trials.len() == self.max_plan_evaluations || trials.len() as u64 == combinations {
                break;
            }
            // Last operation ID varies fastest. Input array order cannot change traversal or ties.
            let mut next_route = true;
            for (index, group) in indices.iter_mut().zip(&groups).rev() {
                if *index < group.recipes.len() {
                    *index += 1;
                    next_route = false;
                    break;
                }
                *index = 0;
            }
            if next_route {
                route_index += 1;
            }
        }
        let search_complete = trials.len() as u64 == combinations;
        let has_plan = selected_plan.is_some();
        let unresolved_trials = trials
            .iter()
            .filter(|trial| {
                // A definitive shared-planner refusal needs no timing proof. Unavailable route
                // timing remains unresolved, just as it does without a completion constraint.
                if trial.refusal.is_none()
                    && let Some(receipt) = &trial.completion_deadline
                {
                    match receipt.status {
                        deadline::Status::Excluded => return false,
                        deadline::Status::Unresolved => return true,
                        deadline::Status::Feasible => {}
                    }
                }
                trial.refusal.as_ref().is_some_and(|r| {
                    matches!(
                        r.code,
                        RefusalCode::BudgetExceeded | RefusalCode::NotConverged | RefusalCode::Io
                    )
                }) || trial.ordering_optimality_proven == Some(false)
                    || trial.thread_order_optimality_proven == Some(false)
                    || trial.objective_unavailable.is_some()
            })
            .count();
        Ok(Report {
            schema: if self.completion_deadline_milliseconds.is_some() {
                "ftx-sewing-selection-report/7"
            } else if route_search {
                "ftx-sewing-selection-report/6"
            } else if cost {
                "ftx-sewing-selection-report/3"
            } else if parallel {
                "ftx-sewing-selection-report/5"
            } else if handled {
                "ftx-sewing-selection-report/4"
            } else if timed {
                "ftx-sewing-selection-report/2"
            } else {
                "ftx-sewing-selection-report/1"
            },
            request: self.clone(),
            pattern_sha256,
            candidate_combinations: combinations,
            trials,
            selected_choices,
            selected_route,
            selected_plan,
            selected_objective_seconds: if timed { selected_score } else { None },
            selected_thread_order_cost: if cost { selected_score } else { None },
            objective_currency,
            unresolved_trials,
            search_complete,
            budget_exhausted: !search_complete,
            optimality_proven: search_complete && has_plan && unresolved_trials == 0,
            no_feasible_listed_recipe: search_complete && !has_plan && unresolved_trials == 0,
            tie_break: if route_search && cost {
                "template route first, then route ID, operation ID and recipe ID lexicographic; retain first equal complete thread order cost"
            } else if route_search {
                "template route first, then route ID, operation ID and recipe ID lexicographic; retain first equal declared routed dispatch makespan"
            } else if cost {
                "template first, then operation ID / recipe ID lexicographic; retain first equal complete thread order cost"
            } else if parallel
                && self
                    .options
                    .production
                    .as_ref()
                    .and_then(|production| production.routing.as_ref())
                    .is_some_and(routing::Policy::eligible)
            {
                "template first, then operation ID / recipe ID lexicographic; retain first equal declared eligible-route dispatch makespan"
            } else if parallel && routed {
                "template first, then operation ID / recipe ID lexicographic; retain first equal declared fixed-route dispatch makespan"
            } else if parallel {
                "template first, then operation ID / recipe ID lexicographic; retain first equal declared parallel-cell makespan"
            } else if handled {
                "template first, then operation ID / recipe ID lexicographic; retain first equal modeled feed-plus-setup-plus-handling time"
            } else if timed {
                "template first, then operation ID / recipe ID lexicographic; retain first equal feed-plus-setup time"
            } else {
                "template first, then operation ID / recipe ID lexicographic; retain first equal feed time"
            },
        })
    }
}

struct JsonBytes {
    bytes: Vec<u8>,
    exceeded: bool,
}

impl Write for JsonBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > MAX_JSON_BYTES)
        {
            self.exceeded = true;
            return Err(std::io::Error::other("sewing selection output budget"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn encode(value: &impl Serialize) -> Result<String, Refusal> {
    let mut writer = JsonBytes {
        bytes: Vec::new(),
        exceeded: false,
    };
    if let Err(error) = serde_json::to_writer(&mut writer, value) {
        return Err(Refusal::new(
            if writer.exceeded {
                RefusalCode::BudgetExceeded
            } else {
                RefusalCode::InvalidDocument
            },
            error.to_string(),
        ));
    }
    String::from_utf8(writer.bytes).map_err(|e| invalid(e.to_string()))
}

/// Clock-free native/browser boundary: report plus an exact options document for ordinary sewing.
/// Keep `selected_options_json` as text when saving it; parsing/reformatting in JavaScript can change
/// floating-point source identities. Absent incumbents yield no assignable options, not stale output.
///
/// # Errors
/// Combined input over 64 MiB, malformed JSON, invalid search policy, or output over 64 MiB.
/// A completed search with no admitted recipe is a report with null selected options, not a parse error.
pub fn select_json(pattern: &str, request: &str) -> Result<String, Refusal> {
    if pattern
        .len()
        .checked_add(request.len())
        .is_none_or(|n| n > MAX_JSON_BYTES)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "combined sewing selection input exceeds 64 MiB",
        ));
    }
    let pattern = Pattern::new(serde_json::from_str(pattern).map_err(|e| invalid(e.to_string()))?);
    let request: Request = serde_json::from_str(request).map_err(|e| invalid(e.to_string()))?;
    let report = request.select(&pattern)?;
    let options = report
        .selected_plan
        .as_ref()
        .map(|plan| encode(&plan.options))
        .transpose()?;
    #[derive(Serialize)]
    struct Job<'a> {
        schema: &'static str,
        report: &'a Report,
        selected_options_json: Option<String>,
    }
    encode(&Job {
        schema: "ftx-sewing-selection-job/1",
        report: &report,
        selected_options_json: options,
    })
}
