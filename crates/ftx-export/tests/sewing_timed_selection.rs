//! Independent setup/feed tradeoffs using synthetic source windows, never machine recommendations.
use ftx_export::{
    sewing::SewPlan,
    sewing_selection::{Objective, Request},
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const INPUT: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-timed-selection.json");

fn pattern() -> Pattern {
    Pattern::new(serde_json::from_str(PATTERN).unwrap())
}
fn request() -> Request {
    serde_json::from_str(INPUT).unwrap()
}

#[test]
fn source_timing_alone_changes_selected_recipes_not_geometry_or_qualification() {
    let mut r = request();
    let slow = r.select(&pattern()).unwrap();
    assert_eq!(slow.schema, "ftx-sewing-selection-report/2");
    assert_eq!(slow.candidate_combinations, 4);
    // Independent AA, BA, AB, BB arithmetic; finish sorts before join in outer enumeration.
    assert_eq!(
        slow.trials
            .iter()
            .map(|t| t.objective_seconds.unwrap())
            .collect::<Vec<_>>(),
        [8.0, 23.0, 16.0, 14.0]
    );
    assert_eq!(slow.selected_objective_seconds, Some(8.0));
    assert!(
        slow.selected_choices
            .as_ref()
            .unwrap()
            .values()
            .all(|s| s == "template")
    );
    assert!(slow.optimality_proven && slow.unresolved_trials == 0);
    let material = serde_json::to_value(&slow.selected_plan.as_ref().unwrap().materials).unwrap();

    r.options
        .ordering
        .as_mut()
        .unwrap()
        .setup_times
        .as_mut()
        .unwrap()
        .transitions[0]
        .milliseconds = 3000;
    let fast = r.select(&pattern()).unwrap();
    assert_eq!(
        fast.trials
            .iter()
            .map(|t| t.objective_seconds.unwrap())
            .collect::<Vec<_>>(),
        [8.0, 16.0, 9.0, 7.0]
    );
    assert_eq!(fast.selected_objective_seconds, Some(7.0));
    assert!(
        fast.selected_choices
            .as_ref()
            .unwrap()
            .values()
            .all(|s| s == "fast")
    );
    let plan = fast.selected_plan.as_ref().unwrap();
    assert_eq!(plan.schema, "ftx-sewing-plan/9");
    assert_eq!(plan.feed_seconds, 4.0);
    assert_eq!(
        plan.ordering
            .as_ref()
            .unwrap()
            .timing
            .as_ref()
            .unwrap()
            .selected_setup_milliseconds,
        3000
    );
    assert_eq!(material, serde_json::to_value(&plan.materials).unwrap());
    assert_eq!(plan.pattern_sha256, slow.pattern_sha256);
    assert!(
        plan.seams
            .iter()
            .all(|s| s.qualification_id.as_deref() == Some("synthetic-alternative"))
    );
    assert!(!plan.hardware_execution_authorized && !plan.physical_seam_quality_validated);
    assert_eq!(
        serde_json::to_value(plan).unwrap(),
        serde_json::to_value(SewPlan::build(&pattern(), &plan.options).unwrap()).unwrap()
    );
    r.alternatives.reverse();
    r.options.passes.reverse();
    r.options
        .ordering
        .as_mut()
        .unwrap()
        .setup_times
        .as_mut()
        .unwrap()
        .transitions
        .reverse();
    let reordered = r.select(&pattern()).unwrap();
    assert_eq!(reordered.selected_choices, fast.selected_choices);
    assert_eq!(
        serde_json::to_value(reordered.trials).unwrap(),
        serde_json::to_value(fast.trials).unwrap()
    );
}

#[test]
fn feed_only_keeps_its_objective_and_wire_shape_and_ties_preserve_template() {
    let mut r = request();
    r.schema = "ftx-sewing-selection/1".into();
    r.objective = Objective::MinimizeFeedSeconds;
    let feed = r.select(&pattern()).unwrap();
    assert_eq!(feed.selected_plan.as_ref().unwrap().feed_seconds, 4.0);
    assert!(feed.selected_choices.unwrap().values().all(|s| s == "fast"));
    let wire = serde_json::to_value(&feed.trials[0]).unwrap();
    assert!(wire.get("objective_seconds").is_none());
    assert!(wire.get("ordering_optimality_proven").is_none());
    assert!(feed.selected_objective_seconds.is_none());
    let mut r = request();
    r.options
        .ordering
        .as_mut()
        .unwrap()
        .setup_times
        .as_mut()
        .unwrap()
        .transitions[0]
        .milliseconds = 4000;
    let tie = r.select(&pattern()).unwrap();
    assert_eq!(tie.selected_objective_seconds, Some(8.0));
    assert!(
        tie.selected_choices
            .unwrap()
            .values()
            .all(|s| s == "template")
    );
}

#[test]
fn incomplete_nested_schedules_are_not_a_joint_optimum_even_after_all_recipes() {
    let mut r = request();
    let a = r.options.passes[0].clone();
    let b = &r.alternatives[0].recipes[0];
    let mut passes = Vec::new();
    for i in 0..4 {
        let mut p = a.clone();
        p.id = Some(format!("p{i}"));
        p.after.clear();
        if i % 2 == 1 {
            p.needle = b.needle.clone();
            p.thread = b.thread.clone();
            p.max_stitch_mm = b.max_stitch_mm;
            p.stitches_per_minute = b.stitches_per_minute;
        }
        passes.push(p);
    }
    r.options.passes = passes;
    r.alternatives.truncate(1);
    r.alternatives[0].operation = "p0".into();
    // A second recipe with unchanged physical settings lets both full jobs reach the inner budget.
    let recipe = &mut r.alternatives[0].recipes[0];
    recipe.needle = a.needle;
    recipe.thread = a.thread;
    recipe.max_stitch_mm = a.max_stitch_mm;
    recipe.stitches_per_minute = a.stitches_per_minute;
    r.options.ordering.as_mut().unwrap().max_search_nodes = 1;
    let joint = r.select(&pattern()).unwrap();
    assert!(joint.search_complete && !joint.budget_exhausted);
    assert!(joint.selected_plan.is_some() && !joint.optimality_proven);
    assert_eq!(joint.unresolved_trials, 2);
    assert!(
        joint
            .trials
            .iter()
            .all(|t| t.ordering_optimality_proven == Some(false) && t.refusal.is_none())
    );
    assert_eq!(joint.selected_plan.unwrap().seams.len(), 4);
    r.schema = "ftx-sewing-selection/1".into();
    r.objective = Objective::MinimizeFeedSeconds;
    let feed = r.select(&pattern()).unwrap();
    assert!(feed.optimality_proven && feed.unresolved_trials == 0); // ordering cannot affect feed
}

#[test]
fn unmeasured_candidate_times_and_invalid_policies_refuse_before_search() {
    for case in 0..8 {
        let mut raw: Value = serde_json::from_str(INPUT).unwrap();
        match case {
            0 => raw["options"]["ordering"]["setup_times"]["transitions"] = json!([]),
            1 => raw["options"]["ordering"]["setup_times"]["allow_estimates"] = json!(false),
            2 => raw["options"]["ordering"]["setup_times"]["machine"] = json!("wrong-machine"),
            3 => raw["options"]["ordering"]["initial_setup"] = Value::Null,
            4 => raw["options"]["ordering"]["objective"] = json!("minimize_setup_changes"),
            5 => raw["schema"] = json!("ftx-sewing-selection/1"),
            6 => raw["options"]["ordering"] = Value::Null,
            _ => raw["options"]["ordering"]["setup_times"]["source"]["note"] = json!(""),
        }
        let r: Request = serde_json::from_value(raw).unwrap();
        assert!(r.select(&pattern()).is_err(), "case {case}");
    }
    let mut r = request();
    // Even an alternative that fails physical qualification cannot conceal missing timing evidence.
    r.alternatives[0].recipes[0].needle = "not-in-the-table".into();
    assert!(
        r.select(&pattern())
            .unwrap_err()
            .message
            .contains("duration")
    );
}

#[test]
fn outer_interruptions_and_material_changes_never_publish_a_partial_or_relabelled_job() {
    let mut r = request();
    r.max_plan_evaluations = 1;
    let stopped = r.select(&pattern()).unwrap();
    assert_eq!(stopped.selected_plan.unwrap().seams.len(), 2);
    assert_eq!(stopped.selected_objective_seconds, Some(8.0));
    assert!(stopped.budget_exhausted && !stopped.optimality_proven);
    r.max_plan_evaluations = 8;
    let mut p: Value = serde_json::from_str(PATTERN).unwrap();
    p["fabric"]["material"]["record"]["name"] = json!("different specimen identity");
    let failed = r.select(&Pattern::new(p)).unwrap();
    assert!(failed.no_feasible_listed_recipe && failed.selected_plan.is_none());
    assert!(failed.selected_objective_seconds.is_none());
    assert!(failed.trials.iter().all(|t| t.refusal.is_some()));
}
