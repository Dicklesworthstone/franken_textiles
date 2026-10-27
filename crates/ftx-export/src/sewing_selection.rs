//! Bounded recipe selection using the production sewing planner, not a second feasibility model.
//! Only explicitly supplied needle/thread/pitch/rate choices vary. Geometry, assembly, pressure,
//! material evidence, backtacks and prerequisites stay in the original job.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::Pattern;
use serde::{Deserialize, Serialize};

use crate::sewing::{SewOptions, SewPlan, SewingPass, consumables, ordering};

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
    fn apply(&self, pass: &mut SewingPass) {
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
    /// Minimize elapsed whole-garment parallel-cell completion under the explicitly qualified
    /// availability, independence and conservative millisecond planning policy.
    MinimizeParallelMakespan,
    /// Minimize complete source-locked thread orders from the shared consumables price result.
    /// Unknown consumption or incomplete eligible offer coverage cannot supply a score.
    MinimizeThreadObservedOrderCost,
}

/// Search a finite Cartesian product of recipes, retaining the original complete job as a candidate.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// `/1` feed; `/2` feed plus setup; `/3` thread cost; `/4` serial time; `/5` parallel makespan.
    pub schema: String,
    /// Complete template job. `require_qualification` must be true; it is never silently enabled.
    pub options: SewOptions,
    /// One to twelve operations with explicit alternatives; unlisted operations remain unchanged.
    pub alternatives: Vec<Alternatives>,
    /// Explicit feed, feed-plus-setup or complete observed thread-order objective.
    pub objective: Objective,
    /// Maximum complete-plan evaluations, including refused trials and the original template, 1..64.
    pub max_plan_evaluations: usize,
}

/// Compact outcome of one complete job trial; refused jobs are never omitted from the count.
#[derive(Clone, Debug, Serialize)]
pub struct Trial {
    /// Operation ID to candidate ID, with `template` denoting an unchanged recipe.
    pub choices: BTreeMap<String, String>,
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
    /// Production succeeded but complete consumption or eligible source prices are unavailable.
    /// This is unresolved objective evidence, never a production infeasibility or a zero score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective_unavailable: Option<Refusal>,
}

/// Complete incumbent plus explicit finite-search status. A refused template does not stop repair.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `/1` for feed only, `/2` for joint time, `/3` for complete thread order cost.
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
    /// Resource/convergence limits, unproved timed schedules, or unavailable complete cost evidence.
    /// They are not infeasible recipes; a complete admitted incumbent may still be retained.
    pub unresolved_trials: usize,
    /// Every supplied combination was examined; no assertion about unlisted process recipes.
    pub search_complete: bool,
    /// Work stopped before exhausting the supplied combinations, with or without an incumbent.
    pub budget_exhausted: bool,
    /// True only with a feasible plan, exhaustive recipe search and no unresolved plan trials.
    /// Timed mode additionally requires every admitted trial's inner schedule to be proved optimal.
    /// Feed-only mode remains independent of the ordering optimizer's separate proof.
    pub optimality_proven: bool,
    /// All listed combinations were definitively refused, without resource/convergence exhaustion;
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

impl Request {
    /// Reserve the complete configured recipe search's thread-purchasing work before evaluation.
    /// Cost mode additionally validates the common canonical market once, visiting every quote.
    /// Studies must multiply this reservation by all expanded material/design rows.
    ///
    /// # Errors
    /// Unsupported thread inputs, arithmetic overflow or more than 200000 total work units.
    pub fn purchasing_work_bound(&self) -> Result<usize, Refusal> {
        let per_plan = consumables::purchasing_work_bound(&self.options)?;
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
        let schema = match self.objective {
            Objective::MinimizeFeedSeconds => "ftx-sewing-selection/1",
            Objective::MinimizeFeedAndSetupSeconds => "ftx-sewing-selection/2",
            Objective::MinimizeModeledProductionSeconds => "ftx-sewing-selection/4",
            Objective::MinimizeParallelMakespan => "ftx-sewing-selection/5",
            Objective::MinimizeThreadObservedOrderCost => "ftx-sewing-selection/3",
        };
        if self.schema != schema || !self.options.require_qualification {
            return Err(invalid(
                "sewing selection needs schema /1 for feed, /2 for feed plus setup, /3 for thread cost, /4 for serial production time or /5 for parallel makespan, and explicit require_qualification=true",
            ));
        }
        if !(1..=MAX_TRIALS).contains(&self.max_plan_evaluations)
            || self.options.passes.is_empty()
            || self.options.passes.len() > MAX_OPERATIONS
            || self.alternatives.is_empty()
            || self.alternatives.len() > MAX_GROUPS
            || self
                .alternatives
                .iter()
                .any(|g| g.recipes.is_empty() || g.recipes.len() > MAX_ALTERNATIVES)
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "sewing selection supports 1..64 plans, at most 64 operations, 1..12 groups and 1..15 alternatives per group",
            ));
        }
        // Ordering is separate from recipe selection. Bound its aggregate configured work without
        // reducing the caller's per-job budget or confusing ordering exhaustion with recipe proof.
        if self
            .max_plan_evaluations
            .checked_mul(self.options.scheduling_work_bound()?)
            .is_none_or(|n| n > MAX_ORDERING_NODES)
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "combined recipe/ordering search exceeds 200000 configured ordering nodes",
            ));
        }
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
        if (timed || handled) && parallel.is_some() {
            return Err(invalid(
                "serial time objectives cannot score a parallel-cell plan; use minimize_parallel_makespan",
            ));
        }
        if self.objective == Objective::MinimizeParallelMakespan && parallel.is_none() {
            return Err(invalid(
                "parallel makespan selection requires a qualified production.parallel policy",
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
        let policies = self.options.ordering_policy().into_iter().chain(
            parallel
                .into_iter()
                .flat_map(|policy| policy.cells.iter().map(|cell| &cell.ordering)),
        );
        for policy in policies {
            if let Some(times) = &policy.setup_times {
                let initial = policy.initial_setup.as_ref().ok_or_else(|| {
                    invalid("timed recipe ordering requires an explicit initial setup")
                })?;
                if !matches!(
                    policy.objective,
                    ordering::Objective::MinimizeSetupTime
                        | ordering::Objective::MinimizeProductionTime
                ) {
                    return Err(invalid("setup times require the timed ordering objective"));
                }
                let choices: BTreeSet<_> = self
                    .options
                    .passes
                    .iter()
                    .map(|p| ordering::Setup {
                        needle: p.needle.clone(),
                        thread: p.thread.clone(),
                    })
                    .chain(groups.iter().flat_map(|g| {
                        g.recipes.iter().map(|r| ordering::Setup {
                            needle: r.needle.clone(),
                            thread: r.thread.clone(),
                        })
                    }))
                    .collect();
                times.check_choices(&self.options.machine.id, &choices, initial)?;
            }
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
        let groups = self.groups()?;
        let combinations = groups
            .iter()
            .try_fold(1_u64, |n, g| n.checked_mul((g.recipes.len() + 1) as u64))
            .ok_or_else(|| invalid("sewing candidate count overflow"))?;
        let pattern_sha256 =
            sha256_hex(&serde_json::to_vec(pattern.raw()).map_err(|e| invalid(e.to_string()))?);
        let mut trials = Vec::new();
        let mut selected_plan: Option<SewPlan> = None;
        let mut selected_choices = None;
        let handled = self.objective == Objective::MinimizeModeledProductionSeconds;
        let parallel = self.objective == Objective::MinimizeParallelMakespan;
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
        loop {
            let mut options = self.options.clone();
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
                feed_seconds: None,
                stitch_intervals: None,
                refusal: None,
                objective_seconds: None,
                ordering_optimality_proven: None,
                thread_order_cost: None,
                objective_unavailable: None,
            };
            match SewPlan::build(pattern, &options) {
                Ok(plan) => {
                    trial.feed_seconds = Some(plan.total_feed_seconds());
                    trial.stitch_intervals = Some(plan.total_stitch_intervals());
                    let score = if let Some(currency) = objective_currency.as_deref() {
                        match thread_cost::score(&plan, currency) {
                            Ok(score) => {
                                trial.thread_order_cost = Some(score);
                                Some(score)
                            }
                            Err(refusal) => {
                                trial.objective_unavailable = Some(refusal);
                                None
                            }
                        }
                    } else if parallel {
                        let seconds = plan.parallel_makespan_seconds().ok_or_else(|| {
                            invalid("admitted parallel recipe lost its complete cell allocation")
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
                    if let Some(score) = score
                        && selected_score.is_none_or(|old| score < old)
                    {
                        selected_score = Some(score);
                        selected_choices = Some(trial.choices.clone());
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
            for (index, group) in indices.iter_mut().zip(&groups).rev() {
                if *index < group.recipes.len() {
                    *index += 1;
                    break;
                }
                *index = 0;
            }
        }
        let search_complete = trials.len() as u64 == combinations;
        let has_plan = selected_plan.is_some();
        let unresolved_trials = trials
            .iter()
            .filter(|trial| {
                trial.refusal.as_ref().is_some_and(|r| {
                    matches!(
                        r.code,
                        RefusalCode::BudgetExceeded | RefusalCode::NotConverged | RefusalCode::Io
                    )
                }) || trial.ordering_optimality_proven == Some(false)
                    || trial.objective_unavailable.is_some()
            })
            .count();
        Ok(Report {
            schema: if cost {
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
            selected_plan,
            selected_objective_seconds: if timed { selected_score } else { None },
            selected_thread_order_cost: if cost { selected_score } else { None },
            objective_currency,
            unresolved_trials,
            search_complete,
            budget_exhausted: !search_complete,
            optimality_proven: search_complete && has_plan && unresolved_trials == 0,
            no_feasible_listed_recipe: search_complete && !has_plan && unresolved_trials == 0,
            tie_break: if cost {
                "template first, then operation ID / recipe ID lexicographic; retain first equal complete thread order cost"
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
