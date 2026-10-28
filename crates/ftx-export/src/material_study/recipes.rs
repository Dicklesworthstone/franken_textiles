//! Material alternatives reuse the qualified production recipe selector. The selected station
//! geometry lives once in the ordinary sewing outcome; this receipt preserves the entire search.
use std::{borrow::Cow, collections::BTreeMap};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::Pattern;
use serde::Serialize;

use super::{Outcome, budget, invalid};
use crate::{
    sewing::{SewOptions, SewPlan},
    sewing_selection,
};

pub(super) fn production_options(
    options: &SewOptions,
    cutting_copies: Option<usize>,
) -> Result<Cow<'_, SewOptions>, Refusal> {
    if options.consumables.is_none() && options.production.is_none() {
        return Ok(Cow::Borrowed(options));
    }
    let thread = options
        .consumables
        .as_ref()
        .and_then(|c| c.production_copies);
    let schedule = options
        .production
        .as_ref()
        .and_then(|p| p.production_copies);
    let counts: Vec<_> = [cutting_copies, thread, schedule]
        .into_iter()
        .flatten()
        .collect();
    if counts.windows(2).any(|pair| pair[0] != pair[1]) {
        return Err(invalid(
            "thread production_copies and sewing schedule production_copies must match every requested cutting quantity",
        ));
    }
    let Some(copies) = counts.first().copied() else {
        return Ok(Cow::Borrowed(options));
    };
    let needs_threads = options
        .consumables
        .as_ref()
        .is_some_and(|c| c.production_copies.is_none() && (copies != 1 || schedule.is_some()));
    let needs_schedule = options
        .production
        .as_ref()
        .is_some_and(|p| p.production_copies.is_none());
    if !needs_threads && !needs_schedule {
        return Ok(Cow::Borrowed(options));
    }
    let mut resolved = options.clone();
    if needs_threads {
        resolved.consumables.as_mut().unwrap().production_copies = Some(copies);
    }
    if needs_schedule {
        resolved.production.as_mut().unwrap().production_copies = Some(copies);
    }
    Ok(Cow::Owned(resolved))
}

/// Recipe-selection receipt for one material-study alternative. Its selected complete plan is
/// `Alternative.sewing`, not a duplicated set of source/material/needle station arrays.
#[derive(Clone, Debug, Serialize)]
pub struct Selection {
    /// `ftx-material-study-sewing-selection/1`.
    pub schema: &'static str,
    /// Original source-bearing candidates, machine limits and qualification policy.
    pub request: sewing_selection::Request,
    /// Exact substituted pattern which was passed to the production selector.
    pub pattern_sha256: String,
    /// Number of complete combinations in the explicitly supplied recipe choice space.
    pub candidate_combinations: u64,
    /// Every examined whole-job trial, including its original production-planner refusals.
    pub trials: Vec<sewing_selection::Trial>,
    /// Selected operation-to-recipe identities; absent when no qualified plan was found.
    pub selected_choices: Option<BTreeMap<String, String>>,
    /// Selected complete specialist route identity for opt-in route selection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_route: Option<String>,
    /// Selected objective, including qualified setup time when requested by the selector.
    pub selected_objective_seconds: Option<f64>,
    /// Complete canonical thread order cost for a cost-directed search; never a partial total.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_thread_order_cost: Option<f64>,
    /// Exact thread-market currency; no conversion to cutting or other costs is inferred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective_currency: Option<String>,
    /// Trials whose resource/convergence refusal prevents a proof of infeasibility or optimality.
    pub unresolved_trials: usize,
    /// All supplied combinations were examined; not a global manufacturing search.
    pub search_complete: bool,
    /// The requested plan-evaluation limit stopped the finite search.
    pub budget_exhausted: bool,
    /// Source selector's proof for the requested objective over the listed choices only.
    pub optimality_proven: bool,
    /// Every supplied combination was definitively refused, without unresolved trials.
    pub no_feasible_listed_recipe: bool,
    /// Source selector's deterministic rule for equal objective values.
    pub tie_break: &'static str,
}

pub(super) fn check_budget(
    request: &sewing_selection::Request,
    variants: usize,
    cutting_copies: Result<Option<usize>, Refusal>,
) -> Result<(), Refusal> {
    // Reject the whole configured search before binding any alternative. The selector still
    // enforces its per-job geometry, station, pass, candidate and source-qualification limits.
    let plans = request.max_plan_evaluations.saturating_mul(variants);
    let passes = plans
        .saturating_mul(request.options.passes.len())
        .saturating_mul(request.source_plan_count());
    if plans > 256 || passes > 4096 {
        return Err(budget(
            "material study exceeds 256 aggregate sewing recipe plan evaluations or 4096 operation evaluations; reduce alternatives or recipe search limits",
        ));
    }
    let scheduling_work = match (|| {
        // Reserve the same authored or inherited quantity that each row actually schedules.
        let options = production_options(&request.options, cutting_copies?)?;
        match options {
            Cow::Borrowed(_) => request.scheduling_work_bound(),
            Cow::Owned(options) => {
                let mut resolved = request.clone();
                resolved.options = options;
                resolved.scheduling_work_bound()
            }
        }
    })() {
        Ok(work) => work,
        Err(refusal) if refusal.code == RefusalCode::BudgetExceeded => return Err(refusal),
        // Malformed or contradictory sewing inputs remain independent per-row refusals.
        // They fail before scheduling and must not suppress a valid cutting or mass result.
        Err(_) => 0,
    };
    if variants.saturating_mul(scheduling_work) > 200_000 {
        return Err(budget(
            "material study exceeds 200000 aggregate recipe/ordering search nodes",
        ));
    }
    Ok(())
}

pub(super) fn evaluate(
    pattern: &Pattern,
    request: &sewing_selection::Request,
    cutting_copies: Result<Option<usize>, Refusal>,
) -> (Outcome<SewPlan>, Outcome<Selection>) {
    let report = match (|| {
        let options = production_options(&request.options, cutting_copies?)?;
        match options {
            Cow::Borrowed(_) => request.select(pattern),
            Cow::Owned(options) => {
                let mut resolved = request.clone();
                resolved.options = options;
                resolved.select(pattern)
            }
        }
    })() {
        Ok(report) => report,
        Err(refusal) => {
            return (
                Outcome::Refused {
                    refusal: refusal.clone(),
                },
                Outcome::Refused { refusal },
            );
        }
    };
    // Keep the selector's status and evidence unchanged while moving (not cloning) the geometry
    // into the existing consumer. An available search is not necessarily an available sew plan.
    let sewing_selection::Report {
        schema: _,
        request,
        pattern_sha256,
        candidate_combinations,
        trials,
        selected_choices,
        selected_route,
        selected_objective_seconds,
        selected_thread_order_cost,
        objective_currency,
        selected_plan,
        unresolved_trials,
        search_complete,
        budget_exhausted,
        optimality_proven,
        no_feasible_listed_recipe,
        tie_break,
    } = report;
    let sewing = match selected_plan {
        Some(result) => Outcome::Available { result },
        None => Outcome::Refused {
            refusal: Refusal::new(
                if budget_exhausted {
                    RefusalCode::BudgetExceeded
                } else if unresolved_trials > 0 {
                    RefusalCode::NotConverged
                } else {
                    RefusalCode::InvalidDocument
                },
                if no_feasible_listed_recipe {
                    "Every listed sewing recipe combination was refused by the production planner; inspect sewing_selection.trials for source qualification and machine limits"
                } else {
                    "No complete sewing plan with an available requested objective was selected; inspect production refusals and objective_unavailable in the recipe trials. This does not establish infeasibility"
                },
            ),
        },
    };
    (
        sewing,
        Outcome::Available {
            result: Selection {
                schema: "ftx-material-study-sewing-selection/1",
                request,
                pattern_sha256,
                candidate_combinations,
                trials,
                selected_choices,
                selected_route,
                selected_objective_seconds,
                selected_thread_order_cost,
                objective_currency,
                unresolved_trials,
                search_complete,
                budget_exhausted,
                optimality_proven,
                no_feasible_listed_recipe,
                tie_break,
            },
        },
    )
}
