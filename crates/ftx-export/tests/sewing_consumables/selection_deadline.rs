//! The completion constraint filters complete canonical schedules while preserving real costs.

use super::{Request, close, deadline_request, request};
use ftx_core::RefusalCode;
use ftx_export::{
    sewing::{SewPlan, program::SewProgram},
    sewing_selection::deadline::{Basis, Status},
};
use serde_json::{Value, json};

#[test]
fn deadline_selects_the_cheapest_complete_recipe_that_finishes_in_time() {
    let (pattern, raw) = deadline_request();
    let request: Request = serde_json::from_value(raw).unwrap();
    let result = request.select(&pattern).unwrap();
    assert_eq!(result.schema, "ftx-sewing-selection-report/7");
    assert_eq!(result.request.completion_deadline_milliseconds, Some(7000));
    assert!(result.search_complete && result.optimality_proven);
    assert_eq!(result.unresolved_trials, 0);
    assert_eq!(result.selected_thread_order_cost, Some(21.0));
    assert_eq!(
        serde_json::to_value(&result.selected_choices).unwrap(),
        json!({"join":"faster","reinforce":"template"})
    );
    for (trial, seconds, cost, status) in [
        (&result.trials[0], 10.0, 14.0, Status::Excluded),
        (&result.trials[1], 7.2, 21.0, Status::Excluded),
        (&result.trials[2], 6.8, 21.0, Status::Feasible),
        (&result.trials[3], 4.0, 28.0, Status::Feasible),
    ] {
        assert_eq!(trial.thread_order_cost, Some(cost));
        assert!(trial.refusal.is_none() && trial.objective_unavailable.is_none());
        let timing = trial.completion_deadline.as_ref().unwrap();
        assert_eq!(timing.schema, "ftx-sewing-completion-deadline/1");
        assert_eq!(timing.status, status);
        assert_eq!(timing.basis, Some(Basis::SerialFeedSetupHandling));
        close(timing.selected_completion_seconds.unwrap(), seconds);
        close(timing.lower_bound_completion_seconds.unwrap(), seconds);
        assert_eq!(timing.timing_optimality_proven, Some(true));
        assert!(timing.selected_makespan_milliseconds.is_none());
    }
    let plan = result.selected_plan.unwrap();
    assert_eq!(plan.total_stitch_intervals(), 70);
    close(plan.modeled_production_seconds().unwrap(), 6.8);
    assert_eq!(
        serde_json::to_value(&plan).unwrap(),
        serde_json::to_value(SewPlan::build(&pattern, &plan.options).unwrap()).unwrap()
    );
    assert_eq!(
        SewProgram::build(&pattern, &plan.options)
            .unwrap()
            .stitches
            .len(),
        70
    );
}

#[test]
fn deadline_is_inclusive_and_outer_budget_never_proves_unexamined_choices_fail() {
    let (pattern, raw) = deadline_request();
    let mut request: Request = serde_json::from_value(raw).unwrap();
    request.completion_deadline_milliseconds = Some(4000);
    let exact = request.select(&pattern).unwrap();
    assert_eq!(exact.selected_thread_order_cost, Some(28.0));
    assert_eq!(
        exact.trials[3].completion_deadline.as_ref().unwrap().status,
        Status::Feasible
    );
    request.completion_deadline_milliseconds = Some(3999);
    let missed = request.select(&pattern).unwrap();
    assert!(missed.selected_plan.is_none() && missed.no_feasible_listed_recipe);
    assert!(missed.search_complete && !missed.optimality_proven);
    assert_eq!(missed.unresolved_trials, 0);
    assert_eq!(missed.trials[3].thread_order_cost, Some(28.0));

    request.completion_deadline_milliseconds = Some(7000);
    request.max_plan_evaluations = 1;
    let bounded = request.select(&pattern).unwrap();
    assert!(bounded.selected_plan.is_none() && bounded.budget_exhausted);
    assert!(!bounded.search_complete && !bounded.no_feasible_listed_recipe);
    assert_eq!(bounded.trials[0].thread_order_cost, Some(14.0));
}

#[test]
fn deadline_requires_complete_handling_and_preserves_available_original_scores() {
    let (pattern, mut raw) = request();
    raw["completion_deadline_milliseconds"] = json!(100000);
    let result = serde_json::from_value::<Request>(raw)
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert!(result.selected_plan.is_none() && !result.no_feasible_listed_recipe);
    assert!(result.search_complete && !result.optimality_proven);
    assert_eq!(result.unresolved_trials, 4);
    for (trial, cost) in result.trials.iter().zip([14.0, 21.0, 21.0, 28.0]) {
        assert_eq!(trial.thread_order_cost, Some(cost));
        assert!(trial.refusal.is_none() && trial.objective_unavailable.is_none());
        let timing = trial.completion_deadline.as_ref().unwrap();
        assert_eq!(timing.status, Status::Unresolved);
        assert!(timing.basis.is_none() && timing.selected_completion_seconds.is_none());
        assert!(timing.reason.contains("handling"));
    }
}

#[test]
fn deadline_distinguishes_unproved_misses_feasible_incumbents_and_proved_lower_bounds() {
    let (pattern, mut raw) = deadline_request();
    let pass = raw["options"]["passes"][0].clone();
    raw["alternatives"] = json!([{"operation":"join","recipes":[{
        "id":"same-source-recipe","needle":pass["needle"],"thread":pass["thread"],
        "max_stitch_mm":pass["max_stitch_mm"],"stitches_per_minute":pass["stitches_per_minute"]}]}]);
    raw["options"]["passes"][1]["after"] = json!([]);
    raw["options"]["production"]["ordering"]["max_search_nodes"] = json!(1);
    raw["options"]["production"]["handling"]["transitions"][0]["milliseconds"] = json!(3000);
    raw["completion_deadline_milliseconds"] = json!(11000);
    let mut request: Request = serde_json::from_value(raw).unwrap();
    let missed = request.select(&pattern).unwrap();
    assert!(missed.search_complete && missed.selected_plan.is_none());
    assert!(!missed.no_feasible_listed_recipe && !missed.optimality_proven);
    assert_eq!(missed.unresolved_trials, 2);
    for trial in &missed.trials {
        assert_eq!(trial.thread_order_cost, Some(14.0));
        let timing = trial.completion_deadline.as_ref().unwrap();
        assert_eq!(timing.status, Status::Unresolved);
        close(timing.selected_completion_seconds.unwrap(), 12.7);
        assert!(timing.lower_bound_completion_seconds.unwrap() <= 11.0);
        assert_eq!(timing.timing_optimality_proven, Some(false));
    }

    request.completion_deadline_milliseconds = Some(12700);
    let feasible = request.select(&pattern).unwrap();
    assert_eq!(feasible.selected_thread_order_cost, Some(14.0));
    assert!(feasible.optimality_proven && feasible.unresolved_trials == 0);
    assert_eq!(
        feasible.trials[0]
            .completion_deadline
            .as_ref()
            .unwrap()
            .timing_optimality_proven,
        Some(false)
    );

    request.schema = "ftx-sewing-selection/1".into();
    request.objective = super::Objective::MinimizeFeedSeconds;
    let feed = request.select(&pattern).unwrap();
    close(
        feed.selected_plan.as_ref().unwrap().total_feed_seconds(),
        9.0,
    );
    assert!(feed.optimality_proven && feed.unresolved_trials == 0);

    request.schema = "ftx-sewing-selection/4".into();
    request.objective = super::Objective::MinimizeModeledProductionSeconds;
    let timed = request.select(&pattern).unwrap();
    close(timed.selected_objective_seconds.unwrap(), 12.7);
    assert!(!timed.optimality_proven && timed.unresolved_trials == 2);
    request.completion_deadline_milliseconds = Some(0);
    let excluded = request.select(&pattern).unwrap();
    assert!(excluded.no_feasible_listed_recipe && excluded.unresolved_trials == 0);
    assert!(excluded.trials.iter().all(|trial| {
        trial.objective_seconds.is_some()
            && trial.completion_deadline.as_ref().unwrap().status == Status::Excluded
    }));

    request.completion_deadline_milliseconds = Some(11000);
    request
        .options
        .production
        .as_mut()
        .unwrap()
        .ordering
        .max_search_nodes = 100;
    let complete = request.select(&pattern).unwrap();
    close(complete.selected_objective_seconds.unwrap(), 10.0);
    assert!(complete.optimality_proven && complete.unresolved_trials == 0);
}

#[test]
fn omitted_deadline_preserves_wire_and_authored_clock_must_be_exact() {
    let (pattern, mut raw) = deadline_request();
    raw.as_object_mut()
        .unwrap()
        .remove("completion_deadline_milliseconds");
    let omitted = serde_json::from_value::<Request>(raw.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    let wire = serde_json::to_value(omitted).unwrap();
    assert_eq!(wire["schema"], "ftx-sewing-selection-report/3");
    assert!(
        wire["request"]
            .get("completion_deadline_milliseconds")
            .is_none()
    );
    assert!(wire["trials"][0].get("completion_deadline").is_none());
    raw["completion_deadline_milliseconds"] = Value::Null;
    assert_eq!(
        serde_json::to_value(
            serde_json::from_value::<Request>(raw.clone())
                .unwrap()
                .select(&pattern)
                .unwrap()
        )
        .unwrap(),
        wire
    );
    for invalid in [json!(-1), json!(1.5), json!("7000")] {
        raw["completion_deadline_milliseconds"] = invalid;
        assert!(serde_json::from_value::<Request>(raw.clone()).is_err());
    }
    raw["completion_deadline_milliseconds"] = json!(9_007_199_254_740_992_u64);
    assert_eq!(
        serde_json::from_value::<Request>(raw)
            .unwrap()
            .select(&pattern)
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
}
