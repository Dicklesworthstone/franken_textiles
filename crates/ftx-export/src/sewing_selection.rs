//! Bounded recipe selection using the production sewing planner, not a second feasibility model.
//! Only explicitly supplied needle/thread/pitch/rate choices vary. Geometry, assembly, pressure,
//! material evidence, backtacks and prerequisites stay in the original job.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_pattern::Pattern;
use serde::{Deserialize, Serialize};

use crate::sewing::{SewOptions, SewPlan, SewingPass};

const MAX_TRIALS: usize = 64;
const MAX_OPERATIONS: usize = 64;
const MAX_GROUPS: usize = 12;
const MAX_ALTERNATIVES: usize = 15;
const MAX_ORDERING_NODES: usize = 200_000;
const TEMPLATE: &str = "template";

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
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    /// Minimize actual generated continuous-feed seconds, counting locks and repeated travel.
    MinimizeFeedSeconds,
}

/// Search a finite Cartesian product of recipes, retaining the original complete job as a candidate.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Must equal `ftx-sewing-selection/1`.
    pub schema: String,
    /// Complete template job. `require_qualification` must be true; it is never silently enabled.
    pub options: SewOptions,
    /// One to twelve operations with explicit alternatives; unlisted operations remain unchanged.
    pub alternatives: Vec<Alternatives>,
    /// Exactly the declared continuous-feed objective, not an estimate of handling or setup time.
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
}

/// Complete incumbent plus explicit finite-search status. A refused template does not stop repair.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// `ftx-sewing-selection-report/1`.
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
    /// Plan trials interrupted by resource/convergence limits. They are not infeasible recipes.
    pub unresolved_trials: usize,
    /// Every supplied combination was examined; no assertion about unlisted process recipes.
    pub search_complete: bool,
    /// Work stopped before exhausting the supplied combinations, with or without an incumbent.
    pub budget_exhausted: bool,
    /// True only with a feasible plan, exhaustive recipe search and no unresolved plan trials.
    /// The objective is continuous feed alone.
    /// Any separate ordering optimizer's proof remains in `selected_plan.ordering`.
    pub optimality_proven: bool,
    /// All listed combinations were definitively refused, without resource/convergence exhaustion;
    /// does not establish physical or global infeasibility.
    pub no_feasible_listed_recipe: bool,
    /// Equal feed times retain the first admitted combination, preferring the original job.
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
    fn groups(&self) -> Result<Vec<Group<'_>>, Refusal> {
        if self.schema != "ftx-sewing-selection/1" || !self.options.require_qualification {
            return Err(invalid(
                "sewing selection requires ftx-sewing-selection/1 and explicit require_qualification=true",
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
        if self.options.ordering.as_ref().is_some_and(|policy| {
            self.max_plan_evaluations
                .checked_mul(policy.max_search_nodes)
                .is_none_or(|n| n > MAX_ORDERING_NODES)
        }) {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "combined recipe/ordering search exceeds 200000 configured ordering nodes",
            ));
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
            };
            match SewPlan::build(pattern, &options) {
                Ok(plan) => {
                    trial.feed_seconds = Some(plan.feed_seconds);
                    trial.stitch_intervals = Some(plan.stitch_intervals);
                    if selected_plan
                        .as_ref()
                        .is_none_or(|old| plan.feed_seconds < old.feed_seconds)
                    {
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
                })
            })
            .count();
        Ok(Report {
            schema: "ftx-sewing-selection-report/1",
            request: self.clone(),
            pattern_sha256,
            candidate_combinations: combinations,
            trials,
            selected_choices,
            selected_plan,
            unresolved_trials,
            search_complete,
            budget_exhausted: !search_complete,
            optimality_proven: search_complete && has_plan && unresolved_trials == 0,
            no_feasible_listed_recipe: search_complete && !has_plan && unresolved_trials == 0,
            tie_break: "template first, then operation ID / recipe ID lexicographic; retain first equal feed time",
        })
    }
}
