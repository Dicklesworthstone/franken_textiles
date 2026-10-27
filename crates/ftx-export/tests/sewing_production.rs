//! Synthetic operation graphs and source tables verify scheduling, not measured production times.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::Refusal;
use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn build(pattern: Value, options: Value) -> Result<SewPlan, Refusal> {
    SewPlan::build(
        &Pattern::new(pattern),
        &serde_json::from_value::<SewOptions>(options).unwrap(),
    )
}

fn fixture() -> (Value, Value) {
    let pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let mut options: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-options.json"
    ))
    .unwrap();
    options["passes"][0]["id"] = json!("a");
    options["passes"][0]["thread"] = json!("thread-a");
    let mut finish = options["passes"][0].clone();
    finish["id"] = json!("b");
    finish["thread"] = json!("thread-b");
    finish["after"] = json!(["a"]);
    options["passes"].as_array_mut().unwrap().push(finish);
    let baseline = build(pattern.clone(), options.clone()).unwrap();
    options["qualifications"] = json!(baseline.seams.iter().map(|seam| json!({
        "id":format!("qualification-{}", seam.pass.operation_id()),
        "materials":seam.sides.iter().map(|side| &side.material_sha256).collect::<Vec<_>>(),
        "machine":options["machine"]["id"],"needle":seam.pass.needle,"thread":seam.pass.thread,
        "stitch_mm":[0.01,5.0],"feed_b_over_a":[0.5,2.0],"stack_mm":[0.01,3.0],
        "stitches_per_minute":[1.0,1000.0],"allow_reverse":true,
        "evidence":"estimate","allow_estimates":true,
        "source":{"reference":"test:production-seam","accessed_on":"2026-10-26",
            "note":"Synthetic software fixture, not an empirical seam qualification"},
        "method":"Invented complete process envelope for scheduling checks"
    })).collect::<Vec<_>>());
    options["require_qualification"] = json!(true);
    (pattern, options)
}

fn batch(options: &mut Value, copies: usize) {
    options["production"] = json!({
        "production_copies":copies,
        "ordering":{"objective":"minimize_setup_changes","max_search_nodes":2000}
    });
}

fn timed(options: &mut Value) {
    let setup = |thread: &str| json!({"needle":options["passes"][0]["needle"],"thread":thread});
    let transitions = json!([
        {"from":setup("idle"),"to":setup("thread-a"),"milliseconds":1000},
        {"from":setup("idle"),"to":setup("thread-b"),"milliseconds":3000},
        {"from":setup("thread-a"),"to":setup("thread-b"),"milliseconds":500},
        {"from":setup("thread-b"),"to":setup("thread-a"),"milliseconds":2000}
    ]);
    options["production"]["ordering"] = json!({
        "objective":"minimize_setup_time","max_search_nodes":2000,
        "initial_setup":setup("idle"),
        "setup_times":{"machine":options["machine"]["id"],
            "source":{"reference":"test:production-time","accessed_on":"2026-10-26",
                "note":"Synthetic directed times, not a measured production rate"},
            "method":"Invented whole needle/thread transition durations",
            "evidence":"estimate","allow_estimates":true,"transitions":transitions}
    });
}

fn complete(plan: &SewPlan) {
    let production = plan.production.as_ref().unwrap();
    let operations: BTreeMap<_, _> = production
        .operations
        .iter()
        .map(|op| (op.id.as_str(), op))
        .collect();
    assert_eq!(
        operations.len(),
        production.production_copies * plan.seams.len()
    );
    for order in [
        &production.ordering.baseline_order,
        &production.ordering.selected_order,
    ] {
        let mut done = BTreeSet::new();
        for id in order {
            let operation = operations[id.as_str()];
            assert!(operation.after.iter().all(|before| done.contains(before)));
            assert!(done.insert(id.clone()), "operation repeated");
            let source = &plan.seams[operation.seam_index];
            assert_eq!(operation.operation_id, source.pass.operation_id());
            assert_eq!(operation.qualification_id, source.qualification_id);
            assert_eq!(operation.stitch_intervals, source.stitch_intervals);
            assert!(
                operation
                    .after
                    .iter()
                    .all(|before| operations[before.as_str()].copy == operation.copy)
            );
        }
        assert_eq!(done.len(), operations.len(), "complete incumbent required");
    }
}

#[test]
fn repeated_garments_share_setups_without_losing_copy_dependencies_or_qualification() {
    let (pattern, mut options) = fixture();
    let single = build(pattern.clone(), options.clone()).unwrap();
    batch(&mut options, 3);
    let plan = build(pattern, options).unwrap();
    complete(&plan);
    let production = plan.production.as_ref().unwrap();
    assert_eq!(plan.schema, "ftx-sewing-plan/12");
    assert_eq!(production.ordering.baseline_changes.thread_changes, 5);
    assert_eq!(production.ordering.selected_changes.thread_changes, 1);
    assert_eq!(production.ordering.selected_changes.needle_changes, 0);
    assert_eq!(
        production.ordering.selected_order,
        ["0/a", "1/a", "2/a", "0/b", "1/b", "2/b"]
    );
    assert!(production.ordering.optimality_proven);
    assert_eq!(plan.stitch_intervals, single.stitch_intervals);
    assert_eq!(plan.feed_seconds, single.feed_seconds);
    assert_eq!(plan.total_stitch_intervals(), 3 * single.stitch_intervals);
    assert_eq!(plan.total_feed_seconds(), 3.0 * single.feed_seconds);
    for (actual, original) in plan.seams.iter().zip(&single.seams) {
        assert_eq!(actual.stations, original.stations);
    }
    assert!(!plan.hardware_execution_authorized && !plan.physical_seam_quality_validated);
}

#[test]
fn initial_setup_is_charged_once_and_directed_source_times_follow_the_entire_batch() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    timed(&mut options);
    let plan = build(pattern.clone(), options.clone()).unwrap();
    complete(&plan);
    let timing = plan.ordering_report().unwrap().timing.as_ref().unwrap();
    assert_eq!(timing.baseline_setup_milliseconds, 6500);
    assert_eq!(timing.selected_setup_milliseconds, 1500);
    assert_eq!(timing.lower_bound_setup_milliseconds, 1500);
    assert_eq!(
        timing.feed_and_setup_seconds,
        plan.total_feed_seconds() + 1.5
    );
    let replay = build(
        pattern.clone(),
        serde_json::to_value(&plan.options).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(plan).unwrap(),
        serde_json::to_value(replay).unwrap()
    );

    options["production"]["ordering"]["setup_times"]["transitions"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(
        build(pattern.clone(), options.clone())
            .unwrap_err()
            .message
            .contains("missing directed")
    );
    timed(&mut options);
    options["production"]["ordering"]["setup_times"]["allow_estimates"] = json!(false);
    assert!(
        build(pattern.clone(), options.clone())
            .unwrap_err()
            .message
            .contains("consent")
    );
    timed(&mut options);
    options["production"]["ordering"]["setup_times"]["machine"] = json!("another-machine");
    assert!(
        build(pattern, options)
            .unwrap_err()
            .message
            .contains("machine mismatch")
    );
}

#[test]
fn a_bounded_search_keeps_every_operation_and_an_honest_unproved_incumbent() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    options["production"]["ordering"]["max_search_nodes"] = json!(1);
    let plan = build(pattern, options).unwrap();
    complete(&plan);
    let order = plan.ordering_report().unwrap();
    assert_eq!(order.search_nodes, 1);
    assert!(order.budget_exhausted && !order.search_complete && !order.optimality_proven);
    assert_eq!(order.selected_order, order.baseline_order);
    assert_eq!(order.lower_bound_total_changes, 1);
    assert_eq!(order.selected_changes.total_changes, 5);
}

#[test]
fn counts_are_shared_with_thread_inventory_and_conflicts_never_change_legacy_scope() {
    let (pattern, options) = fixture();
    let legacy = build(pattern.clone(), options.clone()).unwrap();
    let legacy_json = serde_json::to_value(&legacy).unwrap();
    assert!(legacy_json.get("production").is_none());
    assert!(legacy_json["options"].get("production").is_none());
    assert_eq!(legacy.total_feed_seconds(), legacy.feed_seconds);
    assert_eq!(legacy.total_stitch_intervals(), legacy.stitch_intervals);

    let mut scheduled = options.clone();
    batch(&mut scheduled, 3);
    scheduled["consumables"] = json!({});
    let plan = build(pattern.clone(), scheduled.clone()).unwrap();
    assert_eq!(
        plan.options.consumables.as_ref().unwrap().production_copies,
        Some(3)
    );
    assert_eq!(
        plan.consumables.as_ref().unwrap().production_copies,
        Some(3)
    );
    assert!(
        !plan
            .consumables
            .as_ref()
            .unwrap()
            .complete_consumption_coverage
    );
    scheduled["production"]
        .as_object_mut()
        .unwrap()
        .remove("production_copies");
    scheduled["consumables"]["production_copies"] = json!(3);
    let inherited = build(pattern.clone(), scheduled.clone()).unwrap();
    assert_eq!(
        inherited
            .options
            .production
            .as_ref()
            .unwrap()
            .production_copies,
        Some(3)
    );
    scheduled["production"]["production_copies"] = json!(2);
    assert!(
        build(pattern.clone(), scheduled)
            .unwrap_err()
            .message
            .contains("must match")
    );

    let mut inventory_only = options;
    inventory_only["consumables"] = json!({"production_copies":3});
    let plan = build(pattern, inventory_only).unwrap();
    assert!(plan.production.is_none());
    assert_eq!(plan.total_feed_seconds(), legacy.feed_seconds);
}

#[test]
fn invalid_quantity_graph_budget_and_material_revision_refuse_before_scheduling() {
    let (pattern, options) = fixture();
    for copies in [0, 4097, 32] {
        let mut bad = options.clone();
        batch(&mut bad, copies);
        assert!(build(pattern.clone(), bad).is_err());
    }
    let mut both = options.clone();
    batch(&mut both, 3);
    both["ordering"] = both["production"]["ordering"].clone();
    assert!(
        build(pattern.clone(), both)
            .unwrap_err()
            .message
            .contains("not both")
    );
    let mut changed = pattern;
    changed["fabric"]["material"]["record"]["properties"]["thickness"]["value"] = json!(0.7);
    let mut scheduled = options;
    batch(&mut scheduled, 3);
    assert!(
        build(changed, scheduled)
            .unwrap_err()
            .message
            .contains("qualification")
    );
}

#[test]
fn expanded_stitches_follow_selected_copies_with_locks_and_reverse_traversal() {
    let (pattern, mut options) = fixture();
    options["machine"]["reverse"] = json!({"stitch_mm":[0.5,5.0],
        "feed_b_over_a":[0.5,2.0],"max_stitches_per_minute":1000.0});
    options["passes"][0]["backtack"] = json!({"start_stitches":2,"end_stitches":3});
    options["passes"][1]["direction"] = json!("reverse");
    batch(&mut options, 3);
    let program = SewProgram::build(
        &Pattern::new(pattern),
        &serde_json::from_value(options).unwrap(),
    )
    .unwrap();
    assert_eq!(program.schema, "ftx-sewing-program/4");
    assert_eq!(
        program.stitches.len(),
        program.plan.total_stitch_intervals()
    );
    let mut sequence = Vec::new();
    let mut last = None;
    for step in &program.stitches {
        let key = (step.copy.unwrap(), step.operation);
        if last != Some(key) {
            sequence.push(format!(
                "{}/{}",
                key.0,
                program.plan.seams[key.1].pass.operation_id()
            ));
            last = Some(key);
        }
        let source = &program.plan.seams[step.operation];
        assert_eq!(step.a_mm, source.stations.a[step.to_station]);
        assert_eq!(step.b_mm, source.stations.b[step.to_station]);
    }
    assert_eq!(
        sequence,
        program.plan.ordering_report().unwrap().selected_order
    );
    assert_eq!(program.plan.total_stitch_intervals(), 3 * (50 + 40));
    assert!(program.stitches.iter().any(|step| step.advance_mm[0] < 0.0));
}

#[test]
fn batch_stitch_expansion_charges_every_copy_before_allocating_output() {
    let (mut pattern, mut options) = fixture();
    for piece in pattern["pieces"].as_array_mut().unwrap() {
        piece["vertices"]["c"][1] = json!(1000.0);
        piece["vertices"]["d"][1] = json!(1000.0);
    }
    options["machine"]["stitch_mm"][0] = json!(0.01);
    for pass in options["passes"].as_array_mut().unwrap() {
        pass["max_stitch_mm"] = json!(0.1);
    }
    batch(&mut options, 31);
    options["production"]["ordering"]["max_search_nodes"] = json!(1);
    let pattern = Pattern::new(pattern);
    let options: SewOptions = serde_json::from_value(options).unwrap();
    let plan = SewPlan::build(&pattern, &options).unwrap();
    assert!(plan.stitch_intervals <= 250_000);
    assert!(plan.total_stitch_intervals() > 250_000);
    let error = SewProgram::build(&pattern, &options).unwrap_err();
    assert_eq!(error.code, ftx_core::RefusalCode::BudgetExceeded);
    assert!(error.message.contains("expanded sewing program"));
}

#[test]
fn masks_and_time_totals_cover_sixty_three_operations_without_wasm_sized_overflow() {
    let (pattern, mut options) = fixture();
    let template = options["passes"][0].clone();
    let mut passes = Vec::new();
    for index in 0..21 {
        let mut pass = template.clone();
        pass["id"] = json!(format!("pass-{index:02}"));
        pass["thread"] = json!(format!("thread-{index:02}"));
        pass["after"] = if index == 0 {
            json!([])
        } else {
            json!([format!("pass-{:02}", index - 1)])
        };
        passes.push(pass);
    }
    options["passes"] = json!(passes);
    // This numerical overflow test uses declared legacy recipes, without fabricating new qualifications.
    options["require_qualification"] = json!(false);
    options["qualifications"] = json!([]);
    batch(&mut options, 3);
    let setup = |thread: String| json!({"needle":template["needle"],"thread":thread});
    let mut transitions = Vec::new();
    for from in 0..=21 {
        for to in 0..21 {
            if from != to {
                transitions.push(json!({"from":setup(format!("thread-{from:02}")),
                    "to":setup(format!("thread-{to:02}")),"milliseconds":86_400_000}));
            }
        }
    }
    options["production"]["ordering"] = json!({"objective":"minimize_setup_time", "max_search_nodes":1,
        "initial_setup":setup("thread-21".into()),
        "setup_times":{"machine":options["machine"]["id"],
            "source":{"reference":"test:long-production-times","accessed_on":"2026-10-26",
                "note":"Extreme synthetic table for integer arithmetic only"},
            "method":"Synthetic one-day transitions", "evidence":"estimate", "allow_estimates":true,
            "transitions":transitions}});
    let plan = build(pattern, options).unwrap();
    complete(&plan);
    assert_eq!(plan.production.as_ref().unwrap().operations.len(), 63);
    assert_eq!(
        plan.ordering_report()
            .unwrap()
            .timing
            .as_ref()
            .unwrap()
            .baseline_setup_milliseconds,
        63_u64 * 86_400_000
    );
}

#[test]
fn source_input_order_and_explicit_single_copy_preserve_deterministic_ordering() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    let first = build(pattern.clone(), options.clone()).unwrap();
    options["passes"].as_array_mut().unwrap().reverse();
    let reversed = build(pattern.clone(), options.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&first.production).unwrap(),
        serde_json::to_value(&reversed.production).unwrap()
    );
    options["production"]["production_copies"] = json!(1);
    let one = build(pattern.clone(), options.clone()).unwrap();
    options["ordering"] = options["production"]["ordering"].clone();
    options.as_object_mut().unwrap().remove("production");
    let original = build(pattern, options).unwrap();
    assert_eq!(one.total_feed_seconds(), original.total_feed_seconds());
    assert_eq!(
        one.total_stitch_intervals(),
        original.total_stitch_intervals()
    );
    assert_eq!(
        one.ordering_report().unwrap().selected_changes,
        original.ordering_report().unwrap().selected_changes
    );
    assert_eq!(
        one.ordering_report()
            .unwrap()
            .selected_order
            .iter()
            .map(|id| id.strip_prefix("0/").unwrap())
            .collect::<Vec<_>>(),
        original
            .ordering_report()
            .unwrap()
            .selected_order
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
}
