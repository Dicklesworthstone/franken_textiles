//! Discrete known answers using the real seam planner; no qualified machine or measured seam claim.
use ftx_core::RefusalCode;
use ftx_export::sewing::{SewOptions, SewPlan};
use ftx_export::sewing_selection::{Recipe, Request};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn pattern() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap()
}

fn request(raw: &Value) -> Request {
    let options: SewOptions = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-options.json"
    ))
    .unwrap();
    let plan = SewPlan::build(&Pattern::new(raw.clone()), &options).unwrap();
    let mut options = serde_json::to_value(options).unwrap();
    options["require_qualification"] = json!(true);
    let q = json!({"id":"synthetic-process-window", "materials":[
            plan.seams[0].sides[0].material_sha256,plan.seams[0].sides[1].material_sha256],
        "machine":options["machine"]["id"],"needle":options["passes"][0]["needle"],
        "thread":options["passes"][0]["thread"], "stitch_mm":[0.5,4.0],
        "feed_b_over_a":[0.9,1.1],"stack_mm":[0.7,0.9],"stitches_per_minute":[300,900],
        "allow_reverse":false,"evidence":"estimate","allow_estimates":true,
        "source":{"reference":"test:sewing-selection","accessed_on":"2026-10-20",
            "note":"Synthetic process window, NOT physical qualification"},
        "method":"Synthetic software regression only"});
    options["qualifications"] = json!([q]);
    serde_json::from_value(json!({"schema":"ftx-sewing-selection/1","options":options,
        "objective":"minimize_feed_seconds","max_plan_evaluations":64,
        "alternatives":[{"operation":"side","recipes":[{
            "id":"faster","needle":"operator-selected-needle","thread":"operator-selected-thread",
            "max_stitch_mm":4.0,"stitches_per_minute":750.0}]}]}))
    .unwrap()
}

fn choice(id: &str, pitch: f64, speed: f64) -> Recipe {
    serde_json::from_value(json!({"id":id,"needle":"operator-selected-needle",
        "thread":"operator-selected-thread","max_stitch_mm":pitch,"stitches_per_minute":speed}))
    .unwrap()
}

#[test]
fn selects_actual_minimum_and_keeps_the_complete_source_qualified_plan() {
    let raw = pattern();
    let original = raw.clone();
    let input = request(&raw);
    let result = input.select(&Pattern::new(raw.clone())).unwrap();
    assert_eq!(result.candidate_combinations, 2);
    assert_eq!(result.trials.len(), 2);
    assert_eq!(result.trials[0].feed_seconds, Some(4.0)); // 40 at 600 / minute
    assert_eq!(result.trials[1].feed_seconds, Some(2.0)); // 25 at 750 / minute
    assert!(result.search_complete && result.optimality_proven);
    assert!(!result.budget_exhausted && !result.no_feasible_listed_recipe);
    let plan = result.selected_plan.as_ref().unwrap();
    assert_eq!(
        plan.seams[0].qualification_id.as_deref(),
        Some("synthetic-process-window")
    );
    assert_eq!(plan.seams[0].pass.max_stitch_mm, 4.0);
    assert_eq!(plan.options.machine.id, input.options.machine.id);
    assert!(plan.options.require_qualification);
    assert!(!plan.hardware_execution_authorized && !plan.physical_seam_quality_validated);
    assert_eq!(raw, original);
    assert_eq!(input.options.passes[0].max_stitch_mm, 2.5);
    let replay = SewPlan::build(&Pattern::new(raw), &plan.options).unwrap();
    assert_eq!(
        serde_json::to_value(plan).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
}

#[test]
fn qualification_and_real_machine_refusals_are_counted_not_silently_repaired() {
    let raw = pattern();
    let mut input = request(&raw);
    input.alternatives[0].recipes = vec![
        choice("outside-process", 5.0, 600.0),
        choice("outside-machine", 4.0, 1200.0),
    ];
    let result = input.select(&Pattern::new(raw)).unwrap();
    assert_eq!(result.trials.len(), 3);
    assert!(
        result.trials[1..]
            .iter()
            .all(|trial| trial.refusal.is_some())
    );
    assert_eq!(result.selected_choices.unwrap()["side"], "template");
    assert!(result.optimality_proven);
}

#[test]
fn source_material_swap_invalidates_every_previously_qualified_recipe() {
    let mut raw = pattern();
    let input = request(&raw);
    raw["fabric"]["material"]["record"]["name"] = json!("another lot with same thickness");
    let result = input.select(&Pattern::new(raw)).unwrap();
    assert!(result.selected_plan.is_none() && result.no_feasible_listed_recipe);
    assert!(result.search_complete && !result.optimality_proven);
    assert!(result.trials.iter().all(|trial| {
        trial
            .refusal
            .as_ref()
            .unwrap()
            .message
            .contains("exact stack")
    }));
}

#[test]
fn incomplete_search_returns_only_a_complete_incumbent_and_never_an_optimality_claim() {
    let raw = pattern();
    let mut input = request(&raw);
    input.max_plan_evaluations = 1;
    let result = input.select(&Pattern::new(raw.clone())).unwrap();
    assert_eq!(result.trials.len(), 1);
    assert_eq!(result.selected_plan.unwrap().feed_seconds, 4.0);
    assert!(result.budget_exhausted && !result.optimality_proven && !result.search_complete);
    input.options.passes[0].needle = "unqualified-needle".into();
    let result = input.select(&Pattern::new(raw.clone())).unwrap();
    assert!(result.selected_plan.is_none());
    assert!(!result.no_feasible_listed_recipe);
    input.max_plan_evaluations = 2;
    let result = input.select(&Pattern::new(raw)).unwrap();
    assert!(result.optimality_proven && result.selected_plan.is_some());
}

#[test]
fn multi_operation_choice_can_repair_an_entirely_unqualified_template() {
    let raw = pattern();
    let mut input = request(&raw);
    input.options.passes[0].id = Some("join".into());
    let mut second = input.options.passes[0].clone();
    second.id = Some("reinforce".into());
    second.after = vec!["join".into()];
    input.options.passes.push(second);
    input.alternatives[0].operation = "join".into();
    let mut second = input.alternatives[0].clone();
    second.operation = "reinforce".into();
    second.recipes[0].stitches_per_minute = 600.0;
    input.alternatives.push(second);
    for pass in &mut input.options.passes {
        pass.needle = "unqualified".into();
    }
    let result = input.select(&Pattern::new(raw.clone())).unwrap();
    assert_eq!(result.trials.len(), 4);
    assert_eq!(
        result
            .trials
            .iter()
            .filter(|trial| trial.refusal.is_some())
            .count(),
        3
    );
    let plan = result.selected_plan.unwrap();
    assert_eq!(plan.seams[0].pass.operation_id(), "join");
    assert_eq!(plan.seams[1].pass.operation_id(), "reinforce");
    assert_eq!(plan.feed_seconds, 4.5);
    assert!(result.optimality_proven);
    input.alternatives.reverse();
    let reordered = input.select(&Pattern::new(raw)).unwrap();
    assert_eq!(result.selected_choices, reordered.selected_choices);
    assert_eq!(
        serde_json::to_value(plan).unwrap(),
        serde_json::to_value(reordered.selected_plan).unwrap()
    );
}

#[test]
fn actual_notch_intervals_and_locks_remain_part_of_the_objective_and_qualification() {
    let mut raw = pattern();
    // 1 mm to a matched notch is below the declared process minimum, even at nominal 4 mm pitch.
    raw["pieces"][0]["notches"] = json!([{"edge":"right","s":0.01}]);
    raw["pieces"][1]["notches"] = json!([{"edge":"left","s":0.99}]);
    let mut input = request(&raw);
    input.options.qualifications[0].stitch_mm = [2.0, 4.0];
    let result = input.select(&Pattern::new(raw)).unwrap();
    assert!(result.no_feasible_listed_recipe && result.selected_plan.is_none());

    let raw = pattern();
    let mut input = request(&raw);
    input.options.machine.reverse = Some(
        serde_json::from_value(json!({
            "stitch_mm":[0.5,5.0],"feed_b_over_a":[0.5,2.0],"max_stitches_per_minute":1000
        }))
        .unwrap(),
    );
    input.options.passes[0].backtack.start_stitches = 2;
    input.options.qualifications[0].allow_reverse = true;
    let result = input.select(&Pattern::new(raw)).unwrap();
    assert_eq!(result.trials[0].stitch_intervals, Some(44));
    assert_eq!(result.trials[1].stitch_intervals, Some(29));
    assert!((result.selected_plan.unwrap().feed_seconds - 2.32).abs() < 1e-12);
}

#[test]
fn estimates_stay_unapproved_and_equal_time_prefers_unchanged_template() {
    let raw = pattern();
    let mut input = request(&raw);
    input.alternatives[0].recipes = vec![choice("same-time", 2.0, 750.0)];
    let result = input.select(&Pattern::new(raw.clone())).unwrap();
    assert_eq!(result.trials[0].feed_seconds, result.trials[1].feed_seconds);
    assert_eq!(result.selected_choices.unwrap()["side"], "template");
    input.options.qualifications[0].allow_estimates = false;
    let result = input.select(&Pattern::new(raw)).unwrap();
    assert!(result.selected_plan.is_none());
    assert!(result.trials.iter().all(|trial| {
        trial
            .refusal
            .as_ref()
            .unwrap()
            .message
            .contains("explicit consent")
    }));
}

#[test]
fn candidate_refusal_from_inner_budget_cannot_become_infeasibility_or_optimality() {
    let mut raw = pattern();
    let mut input = request(&raw);
    // Two long sides at small pitch exceed the existing whole-job station budget before allocation.
    for piece in raw["pieces"].as_array_mut().unwrap() {
        piece["vertices"]["c"][1] = json!(90000.0);
        piece["vertices"]["d"][1] = json!(90000.0);
    }
    input.options.passes[0].max_stitch_mm = 0.01;
    input.options.machine.stitch_mm = [0.001, 5.0];
    let result = input.select(&Pattern::new(raw)).unwrap();
    assert_eq!(
        result.trials[0].refusal.as_ref().unwrap().code,
        RefusalCode::BudgetExceeded
    );
    assert_eq!(result.unresolved_trials, 1);
    assert!(result.search_complete && result.selected_plan.is_some());
    assert!(!result.optimality_proven && !result.no_feasible_listed_recipe);
}

#[test]
fn invalid_request_and_aggregate_ordering_work_refuse_before_any_selection() {
    let raw = pattern();
    for change in [
        (|r: &mut Request| r.options.require_qualification = false) as fn(&mut Request),
        |r| r.max_plan_evaluations = 0,
        |r| r.max_plan_evaluations = 65,
        |r| r.alternatives[0].operation = "absent".into(),
        |r| r.alternatives[0].recipes[0].id = "template".into(),
        |r| r.alternatives[0].recipes[0].max_stitch_mm = f64::NAN,
        |r| r.alternatives[0].recipes[0].stitches_per_minute = 0.0,
        |r| r.alternatives.push(r.alternatives[0].clone()),
        |r| {
            let duplicate = r.alternatives[0].recipes[0].clone();
            r.alternatives[0].recipes.push(duplicate);
        },
        |r| {
            r.options.ordering = Some(
                serde_json::from_value(json!({
                    "objective":"minimize_setup_changes","max_search_nodes":200000
                }))
                .unwrap(),
            )
        },
    ] {
        let mut input = request(&raw);
        change(&mut input);
        assert!(input.select(&Pattern::new(raw.clone())).is_err());
    }
}
