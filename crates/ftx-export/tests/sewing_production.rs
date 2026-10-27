//! Synthetic operation graphs and source tables verify scheduling, not measured production times.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_export::sewing::{SewOptions, SewPlan, production::workholding, program::SewProgram};
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

fn fixtures(pattern: &Value, options: &mut Value, capacity: u32, holds: Value) {
    let mut original = options.clone();
    original["production"]
        .as_object_mut()
        .unwrap()
        .remove("workholding");
    let plan = build(pattern.clone(), original).unwrap();
    let qualifications: BTreeMap<_, _> = plan
        .seams
        .iter()
        .map(|seam| {
            let id = seam.qualification_id.as_ref().unwrap();
            let qualification = plan
                .options
                .qualifications
                .iter()
                .find(|q| &q.id == id)
                .unwrap();
            (
                seam.pass.operation_id().to_string(),
                json!([{
                    "id":id,"sha256":workholding::qualification_sha256(qualification).unwrap()
                }]),
            )
        })
        .collect();
    options["production"]["workholding"] = json!({
        "machine_sha256":workholding::machine_sha256(&plan.options.machine).unwrap(),
        "qualifications":qualifications,
        "resources":[{"id":"jig","capacity":capacity}],"holds":holds,
        "source":{"reference":"test:finite-workholding","accessed_on":"2026-10-27",
            "note":"Synthetic reusable fixture observations; no physical fixture-fit evidence"},
        "method":"Declared finite fixture occupancy through each supported operation envelope",
        "conditions":{"configuration":"synthetic single-machine fixture bench"},
        "evidence":"estimate","allow_estimates":true
    });
}

fn one_hold() -> Value {
    json!([{"id":"held-garment","resource":"jig","units":1,"acquire":"a","release":"b"}])
}

fn handling(pattern: &Value, options: &mut Value, different_copy_ms: u32) {
    let mut original = options.clone();
    original["production"]
        .as_object_mut()
        .unwrap()
        .remove("handling");
    original["production"]["ordering"]["objective"] = json!("minimize_setup_time");
    let plan = build(pattern.clone(), original).unwrap();
    let qualifications: BTreeMap<_, _> = plan
        .seams
        .iter()
        .map(|seam| {
            let id = seam.qualification_id.as_ref().unwrap();
            let qualification = plan
                .options
                .qualifications
                .iter()
                .find(|q| &q.id == id)
                .unwrap();
            (
                seam.pass.operation_id().to_string(),
                json!([{
                    "id":id,"sha256":workholding::qualification_sha256(qualification).unwrap()
                }]),
            )
        })
        .collect();
    let mut transitions = Vec::new();
    for from in ["a", "b"] {
        for to in ["a", "b"] {
            transitions.push(json!({"from":from,"to":to,"relation":"different_copy","milliseconds":different_copy_ms}));
            if from != to {
                transitions
                    .push(json!({"from":from,"to":to,"relation":"same_copy","milliseconds":100}));
            }
        }
    }
    options["production"]["ordering"]["objective"] = json!("minimize_production_time");
    options["production"]["handling"] = json!({
        "model":"serial_operation_transitions/1",
        "machine_sha256":workholding::machine_sha256(&plan.options.machine).unwrap(),
        "qualifications":qualifications,
        "initial_milliseconds":{"a":100,"b":100},"final_milliseconds":{"a":200,"b":200},
        "transitions":transitions,
        "source":{"reference":"test:qualified-serial-handling","accessed_on":"2026-10-27",
            "note":"Invented software timing observations, not measured machine throughput"},
        "method":"Separate additive initial load, between-operation handling and final unload; exclude feed and setup",
        "evidence":"estimate","allow_estimates":true
    });
}

// Independent garment-state enumeration, including the final unload. No optimizer matrix/bounds.
fn exhaustive_handling_cost(copies: usize, capacity: usize, transfer: u64) -> u64 {
    fn visit(
        states: &mut [u8],
        capacity: usize,
        transfer: u64,
        previous: Option<(usize, u8)>,
        cost: u64,
        best: &mut u64,
    ) {
        if states.iter().all(|&state| state == 2) {
            *best = (*best).min(cost + 200);
            return;
        }
        for copy in 0..states.len() {
            let phase = states[copy];
            if phase == 2
                || (phase == 0 && states.iter().filter(|&&state| state == 1).count() == capacity)
            {
                continue;
            }
            let setup = match (previous.map(|(_, phase)| phase), phase) {
                (None, 0) => 1000,
                (None, 1) => 3000,
                (Some(0), 1) => 500,
                (Some(1), 0) => 2000,
                _ => 0,
            };
            let handling = match previous {
                None => 100,
                Some((last, _)) if last == copy => 100,
                Some(_) => transfer,
            };
            states[copy] += 1;
            visit(
                states,
                capacity,
                transfer,
                Some((copy, phase)),
                cost + setup + handling,
                best,
            );
            states[copy] -= 1;
        }
    }
    let mut best = u64::MAX;
    visit(&mut vec![0; copies], capacity, transfer, None, 0, &mut best);
    best
}

#[test]
fn handling_changes_the_actual_batch_order_and_matches_independent_exhaustive_optima() {
    for transfer in [0, 2000, 5000] {
        for capacity in [1, 2, 3] {
            let (pattern, mut options) = fixture();
            batch(&mut options, 3);
            timed(&mut options);
            fixtures(&pattern, &mut options, capacity, one_hold());
            handling(&pattern, &mut options, transfer);
            let plan = build(pattern.clone(), options).unwrap();
            complete(&plan);
            assert_eq!(plan.schema, "ftx-sewing-plan/15");
            let production = plan.production.as_ref().unwrap();
            assert_eq!(production.schema, "ftx-sewing-production/3");
            let order = &production.ordering;
            let times = order.handling_timing.as_ref().unwrap();
            let actual = order.timing.as_ref().unwrap().selected_setup_milliseconds
                + times.selected_handling_milliseconds;
            let expected = exhaustive_handling_cost(3, capacity as usize, u64::from(transfer));
            assert_eq!(actual, expected);
            assert!(order.optimality_proven);
            assert_eq!(times.lower_bound_setup_and_handling_milliseconds, expected);
            assert_eq!(
                times.selected_modeled_production_seconds,
                plan.total_feed_seconds() + expected as f64 / 1000.0
            );
            assert_eq!(
                order
                    .timing
                    .as_ref()
                    .unwrap()
                    .lower_bound_setup_milliseconds,
                0
            );
            let handling = production.handling.as_ref().unwrap();
            assert_eq!(
                handling.selected.total_milliseconds,
                times.selected_handling_milliseconds
            );
            assert_eq!(
                handling.selected.steps.len(),
                production.operations.len() + 1
            );
            assert!(handling.selected.steps.first().unwrap().from.is_none());
            assert!(handling.selected.steps.last().unwrap().to.is_none());
            assert!(!handling.physical_timing_validated);
            assert!(!plan.hardware_execution_authorized);
            if transfer == 2000 && capacity == 2 {
                let program =
                    SewProgram::build(&Pattern::new(pattern.clone()), &plan.options).unwrap();
                let mut sequence = Vec::new();
                for step in &program.stitches {
                    let operation = format!(
                        "{}/{}",
                        step.copy.unwrap(),
                        program.plan.seams[step.operation].pass.operation_id()
                    );
                    if sequence.last() != Some(&operation) {
                        sequence.push(operation);
                    }
                }
                assert_eq!(sequence, order.selected_order);
                assert_eq!(
                    program
                        .plan
                        .production
                        .as_ref()
                        .unwrap()
                        .handling
                        .as_ref()
                        .unwrap()
                        .selected
                        .total_milliseconds,
                    times.selected_handling_milliseconds
                );
            }
            let replay = build(
                pattern.clone(),
                serde_json::to_value(&plan.options).unwrap(),
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(&plan).unwrap(),
                serde_json::to_value(&replay).unwrap()
            );
            if transfer == 5000 {
                assert_eq!(order.selected_order, order.baseline_order);
                assert_eq!(actual, 17100);
                assert_eq!(
                    order.timing.as_ref().unwrap().selected_setup_milliseconds,
                    6500
                );
            }
        }
    }
}

#[test]
fn terminal_unload_changes_the_selected_last_operation_and_bounded_incumbents_stay_complete() {
    let (pattern, mut options) = fixture();
    options["passes"][1]["after"] = json!([]);
    batch(&mut options, 1);
    timed(&mut options);
    handling(&pattern, &mut options, 5000);
    options["production"]["handling"]["final_milliseconds"] = json!({"a":0,"b":10000});
    let plan = build(pattern.clone(), options.clone()).unwrap();
    let order = plan.ordering_report().unwrap();
    assert_eq!(order.selected_order, ["0/b", "0/a"]);
    let handled = order.handling_timing.as_ref().unwrap();
    assert_eq!(handled.selected_handling_milliseconds, 200);
    assert_eq!(handled.lower_bound_setup_and_handling_milliseconds, 5200);

    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    timed(&mut options);
    handling(&pattern, &mut options, 2000);
    options["production"]["ordering"]["max_search_nodes"] = json!(1);
    let plan = build(pattern, options).unwrap();
    complete(&plan);
    let order = plan.ordering_report().unwrap();
    assert!(order.budget_exhausted && !order.optimality_proven);
    assert_eq!(order.selected_order, order.baseline_order);
    let times = order.handling_timing.as_ref().unwrap();
    assert!(
        times.lower_bound_setup_and_handling_milliseconds <= exhaustive_handling_cost(3, 3, 2000)
    );
    assert_eq!(
        plan.production
            .as_ref()
            .unwrap()
            .handling
            .as_ref()
            .unwrap()
            .selected
            .steps
            .len(),
        7
    );
}

#[test]
fn handling_requires_exact_source_applicability_and_never_fills_missing_times() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    timed(&mut options);
    handling(&pattern, &mut options, 5000);
    for (pointer, value) in [
        ("/machine/max_stitches_per_minute", json!(1100)),
        (
            "/qualifications/0/source/note",
            json!("changed timing/material applicability"),
        ),
        ("/production/handling/allow_estimates", json!(false)),
        (
            "/production/handling/initial_milliseconds/a",
            json!(86400001),
        ),
        ("/production/handling/final_milliseconds/b", json!(86400001)),
        ("/production/handling/model", json!("overlapping-phases")),
        (
            "/production/ordering/objective",
            json!("minimize_setup_time"),
        ),
    ] {
        let mut changed = options.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(build(pattern.clone(), changed).is_err(), "{pointer}");
    }
    let mut missing = options.clone();
    missing["production"]["handling"]["transitions"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    assert_eq!(
        build(pattern.clone(), missing).unwrap_err().code,
        RefusalCode::NotConverged
    );
    let mut missing = options.clone();
    missing["production"]["handling"]["qualifications"]["a"] = json!([]);
    assert_eq!(
        build(pattern.clone(), missing).unwrap_err().code,
        RefusalCode::NotConverged
    );
    for field in [
        "initial_milliseconds",
        "final_milliseconds",
        "qualifications",
    ] {
        let mut missing = options.clone();
        missing["production"]["handling"][field]
            .as_object_mut()
            .unwrap()
            .remove("b");
        assert_eq!(
            build(pattern.clone(), missing).unwrap_err().code,
            RefusalCode::NotConverged,
            "{field}"
        );
        let mut unknown = options.clone();
        let value = unknown["production"]["handling"][field]["b"].clone();
        unknown["production"]["handling"][field]["absent-operation"] = value;
        assert_eq!(
            build(pattern.clone(), unknown).unwrap_err().code,
            RefusalCode::InvalidDocument,
            "{field}"
        );
    }
    let mut duplicate = options;
    let row = duplicate["production"]["handling"]["transitions"][0].clone();
    duplicate["production"]["handling"]["transitions"]
        .as_array_mut()
        .unwrap()
        .push(row);
    assert!(build(pattern, duplicate).is_err());
}

#[test]
fn handling_changes_recipe_selection_and_unknown_handling_never_proves_a_recipe_infeasible() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    timed(&mut options);
    let alternatives = json!([{"operation":"b","recipes":[{"id":"same-thread-slower",
        "needle":options["passes"][0]["needle"],"thread":"thread-a","max_stitch_mm":2.5,"stitches_per_minute":500.0}]}]);
    let request = |options: Value, handled| {
        serde_json::from_value::<ftx_export::sewing_selection::Request>(json!({
        "schema":if handled {"ftx-sewing-selection/4"} else {"ftx-sewing-selection/2"},
        "options":options,"alternatives":alternatives,
        "objective":if handled {"minimize_modeled_production_seconds"} else {"minimize_feed_and_setup_seconds"},
        "max_plan_evaluations":2
    })).unwrap()
    };
    let without = request(options.clone(), false)
        .select(&Pattern::new(pattern.clone()))
        .unwrap();
    assert_eq!(without.selected_choices.as_ref().unwrap()["b"], "template");
    handling(&pattern, &mut options, 5000);
    let first_lock = options["production"]["handling"]["qualifications"]["a"][0].clone();
    options["production"]["handling"]["qualifications"]["b"]
        .as_array_mut()
        .unwrap()
        .push(first_lock);
    let with = request(options.clone(), true)
        .select(&Pattern::new(pattern.clone()))
        .unwrap();
    assert_eq!(with.schema, "ftx-sewing-selection-report/4");
    assert_eq!(
        with.selected_choices.as_ref().unwrap()["b"],
        "same-thread-slower"
    );
    assert!(with.optimality_proven);
    assert_eq!(with.selected_objective_seconds, Some(38.0));
    for field in [
        "initial_milliseconds",
        "final_milliseconds",
        "qualifications",
    ] {
        let mut missing = options.clone();
        missing["production"]["handling"][field]
            .as_object_mut()
            .unwrap()
            .remove("b");
        let unresolved = request(missing, true)
            .select(&Pattern::new(pattern.clone()))
            .unwrap();
        assert!(unresolved.search_complete, "{field}");
        assert_eq!(unresolved.unresolved_trials, 2, "{field}");
        assert!(
            !unresolved.optimality_proven && !unresolved.no_feasible_listed_recipe,
            "{field}"
        );
        assert!(unresolved.selected_plan.is_none(), "{field}");
        assert!(
            unresolved.trials.iter().all(|trial| trial
                .refusal
                .as_ref()
                .is_some_and(|refusal| refusal.code == RefusalCode::NotConverged)),
            "{field}"
        );
    }
    options["production"]["handling"]["qualifications"]["b"]
        .as_array_mut()
        .unwrap()
        .pop();
    let unknown = request(options, true)
        .select(&Pattern::new(pattern))
        .unwrap();
    assert_eq!(unknown.unresolved_trials, 1);
    assert!(!unknown.optimality_proven && !unknown.no_feasible_listed_recipe);
    assert!(unknown.selected_plan.is_some());
}

#[test]
fn handling_totals_remain_exact_above_wasm_u32_with_sixty_three_operations() {
    let (pattern, mut options) = fixture();
    options["passes"].as_array_mut().unwrap().pop();
    batch(&mut options, 63);
    timed(&mut options);
    options["production"]["ordering"]["initial_setup"]["thread"] = json!("thread-a");
    handling(&pattern, &mut options, 86400000);
    options["production"]["handling"]["initial_milliseconds"] = json!({"a":86400000});
    options["production"]["handling"]["final_milliseconds"] = json!({"a":86400000});
    options["production"]["handling"]["transitions"]
        .as_array_mut()
        .unwrap()
        .retain(|row| row["from"] == "a" && row["to"] == "a");
    let plan = build(pattern, options).unwrap();
    let timing = plan
        .ordering_report()
        .unwrap()
        .handling_timing
        .as_ref()
        .unwrap();
    assert_eq!(timing.selected_handling_milliseconds, 64_u64 * 86400000);
    assert!(timing.selected_handling_milliseconds > u64::from(u32::MAX));
    assert_eq!(
        timing.lower_bound_setup_and_handling_milliseconds,
        timing.selected_handling_milliseconds
    );
    assert!(plan.ordering_report().unwrap().optimality_proven);
    assert_eq!(
        plan.production
            .as_ref()
            .unwrap()
            .handling
            .as_ref()
            .unwrap()
            .selected
            .steps
            .len(),
        64
    );
}

// Independent full enumeration of the small two-operation garment example. This uses explicit
// garment state and acquired/released inventory, not the optimizer's bit masks or lower bounds.
fn exhaustive_fixture_cost(copies: usize, capacity: usize, timed: bool) -> u64 {
    fn visit(
        states: &mut [u8],
        capacity: usize,
        last: Option<u8>,
        cost: u64,
        timed: bool,
        best: &mut u64,
    ) {
        if states.iter().all(|&state| state == 2) {
            *best = (*best).min(cost);
            return;
        }
        for copy in 0..states.len() {
            let phase = states[copy];
            if phase == 2
                || (phase == 0 && states.iter().filter(|&&state| state == 1).count() == capacity)
            {
                continue;
            }
            let change = if timed {
                match (last, phase) {
                    (None, 0) => 1000,
                    (None, 1) => 3000,
                    (Some(0), 1) => 500,
                    (Some(1), 0) => 2000,
                    _ => 0,
                }
            } else {
                u64::from(last.is_some_and(|previous| previous != phase))
            };
            states[copy] += 1;
            visit(states, capacity, Some(phase), cost + change, timed, best);
            states[copy] -= 1;
        }
    }
    let mut best = u64::MAX;
    visit(&mut vec![0; copies], capacity, None, 0, timed, &mut best);
    best
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
fn finite_fixtures_change_actual_batch_order_and_match_independent_exhaustive_optima() {
    for capacity in 1..=3 {
        for timed_objective in [false, true] {
            let (pattern, mut options) = fixture();
            batch(&mut options, 3);
            if timed_objective {
                timed(&mut options);
            }
            fixtures(&pattern, &mut options, capacity, one_hold());
            let plan = build(pattern, options).unwrap();
            complete(&plan);
            assert_eq!(plan.schema, "ftx-sewing-plan/13");
            let production = plan.production.as_ref().unwrap();
            assert_eq!(production.schema, "ftx-sewing-production/2");
            let report = production.workholding.as_ref().unwrap();
            assert_eq!(report.holds.len(), 3);
            assert_eq!(report.baseline.peak_occupancy["jig"], 1);
            assert_eq!(report.selected.peak_occupancy["jig"], capacity);
            assert!(!report.fixture_geometry_and_access_validated);
            assert!(production.ordering.optimality_proven);
            let actual = if timed_objective {
                let timing = production.ordering.timing.as_ref().unwrap();
                assert_eq!(timing.baseline_setup_milliseconds, 6500);
                assert_eq!(
                    timing.selected_setup_milliseconds,
                    [6500, 4000, 1500][capacity as usize - 1]
                );
                timing.selected_setup_milliseconds
            } else {
                production.ordering.selected_changes.total_changes as u64
            };
            assert_eq!(
                actual,
                exhaustive_fixture_cost(3, capacity as usize, timed_objective)
            );
            for timeline in [&report.baseline, &report.selected] {
                let mut previous = 0;
                for step in &timeline.operations {
                    assert_eq!(step.occupied_before["jig"], previous);
                    assert!(step.occupied_during["jig"] <= capacity);
                    previous = step.occupied_after["jig"];
                }
                assert_eq!(previous, 0);
            }
            assert!(!plan.hardware_execution_authorized && !plan.physical_seam_quality_validated);
        }
    }
}

#[test]
fn resource_search_limit_preserves_a_complete_admitted_incumbent() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    fixtures(&pattern, &mut options, 2, one_hold());
    options["production"]["ordering"]["max_search_nodes"] = json!(1);
    let plan = build(pattern, options).unwrap();
    complete(&plan);
    let production = plan.production.as_ref().unwrap();
    assert!(production.ordering.budget_exhausted);
    assert!(!production.ordering.optimality_proven);
    assert_eq!(
        production.ordering.selected_order,
        production.ordering.baseline_order
    );
    let resources = production.workholding.as_ref().unwrap();
    assert_eq!(resources.selected.peak_occupancy["jig"], 1);
    assert_eq!(resources.selected.operations.len(), 6);
}

#[test]
fn multiple_resources_enforce_integer_demands_instead_of_only_counting_open_copies() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    timed(&mut options);
    fixtures(
        &pattern,
        &mut options,
        3,
        json!([
            {"id":"two-jaws","resource":"jig","units":2,"acquire":"a","release":"b"},
            {"id":"support","resource":"support","units":1,"acquire":"a","release":"b"}
        ]),
    );
    options["production"]["workholding"]["resources"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"support","capacity":2}));
    for (capacity, expected_time, expected_peak) in [(3, 6500, 1), (4, 4000, 2)] {
        options["production"]["workholding"]["resources"][0]["capacity"] = json!(capacity);
        let plan = build(pattern.clone(), options.clone()).unwrap();
        let production = plan.production.as_ref().unwrap();
        assert_eq!(
            production
                .ordering
                .timing
                .as_ref()
                .unwrap()
                .selected_setup_milliseconds,
            expected_time
        );
        let peak = &production
            .workholding
            .as_ref()
            .unwrap()
            .selected
            .peak_occupancy;
        assert_eq!(peak["jig"], 2 * expected_peak);
        assert_eq!(peak["support"], expected_peak);
    }
}

#[test]
fn single_operation_holds_are_available_before_start_and_released_after_completion() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    fixtures(
        &pattern,
        &mut options,
        1,
        json!([
            {"id":"join-fixture","resource":"jig","units":1,"acquire":"a","release":"a"},
            {"id":"finish-fixture","resource":"jig","units":1,"acquire":"b","release":"b"}
        ]),
    );
    let plan = build(pattern.clone(), options.clone()).unwrap();
    let production = plan.production.as_ref().unwrap();
    assert_eq!(production.ordering.selected_changes.total_changes, 1);
    let resources = production.workholding.as_ref().unwrap();
    for step in &resources.selected.operations {
        assert_eq!(step.occupied_before["jig"], 0);
        assert_eq!(step.occupied_during["jig"], 1);
        assert_eq!(step.occupied_after["jig"], 0);
        assert_eq!(step.acquired, step.released);
    }

    // An existing hold cannot be borrowed by another hold merely because this operation releases it.
    options["production"]["workholding"]["holds"][0]["release"] = json!("b");
    let refusal = build(pattern, options).unwrap_err();
    assert!(refusal.message.contains("before start"));
    assert!(refusal.message.contains("baseline is unsupported"));
}

#[test]
fn lifecycles_add_real_precedence_and_refuse_cycles_or_unknown_anchors() {
    let (pattern, mut options) = fixture();
    options["passes"][1]["after"] = json!([]);
    batch(&mut options, 3);
    fixtures(&pattern, &mut options, 2, one_hold());
    let plan = build(pattern.clone(), options.clone()).unwrap();
    complete(&plan);
    for operation in &plan.production.as_ref().unwrap().operations {
        if operation.operation_id == "b" {
            assert_eq!(operation.after, [format!("{}/a", operation.copy)]);
        }
    }
    let mut wrong = options.clone();
    wrong["production"]["workholding"]["holds"][0]["release"] = json!("missing");
    assert!(
        build(pattern.clone(), wrong)
            .unwrap_err()
            .message
            .contains("anchor")
    );
    options["passes"][1]["after"] = json!(["a"]);
    options["production"]["workholding"]["holds"][0]["acquire"] = json!("b");
    options["production"]["workholding"]["holds"][0]["release"] = json!("a");
    assert!(
        build(pattern, options)
            .unwrap_err()
            .message
            .contains("dependency cycle")
    );
}

#[test]
fn infeasible_serial_seed_never_becomes_a_zero_cost_incumbent_or_global_infeasibility_claim() {
    let (pattern, mut options) = fixture();
    let pass = options["passes"][0].clone();
    options["passes"] = json!(["a", "b", "c", "d"].map(|id| {
        let mut pass = pass.clone();
        pass["id"] = json!(id);
        pass
    }));
    options["qualifications"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    batch(&mut options, 2);
    fixtures(
        &pattern,
        &mut options,
        1,
        json!([
            {"id":"first","resource":"jig","units":1,"acquire":"a","release":"c"},
            {"id":"second","resource":"jig","units":1,"acquire":"b","release":"d"}
        ]),
    );
    let refusal = build(pattern.clone(), options.clone()).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::NotConverged);
    assert!(refusal.message.contains("baseline is unsupported"));
    assert!(refusal.message.contains("no global infeasibility"));
    let selection: ftx_export::sewing_selection::Request = serde_json::from_value(json!({
        "schema":"ftx-sewing-selection/1","options":options,
        "alternatives":[{"operation":"a","recipes":[{"id":"slower",
            "needle":options["passes"][0]["needle"],"thread":options["passes"][0]["thread"],
            "max_stitch_mm":2.5,"stitches_per_minute":300.0}]}],
        "objective":"minimize_feed_seconds","max_plan_evaluations":2
    }))
    .unwrap();
    let unresolved = selection.select(&Pattern::new(pattern.clone())).unwrap();
    assert_eq!(unresolved.unresolved_trials, 2);
    assert!(unresolved.search_complete && unresolved.selected_plan.is_none());
    assert!(!unresolved.no_feasible_listed_recipe && !unresolved.optimality_proven);
    // A compatible explicit source order really is feasible; the earlier refusal did not prove otherwise.
    options["passes"][1]["after"] = json!(["c"]);
    options["passes"][2]["after"] = json!(["a"]);
    let plan = build(pattern, options).unwrap();
    complete(&plan);
    assert_eq!(
        plan.ordering_report()
            .unwrap()
            .selected_changes
            .total_changes,
        0
    );
    assert!(plan.ordering_report().unwrap().optimality_proven);
}

#[test]
fn workholding_requires_exact_admitted_machine_material_process_and_independent_evidence() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    fixtures(&pattern, &mut options, 2, one_hold());
    for defect in [
        "machine",
        "qualification",
        "consent",
        "source",
        "coverage",
        "selected",
        "material",
    ] {
        let mut changed = options.clone();
        let mut material = pattern.clone();
        match defect {
            "machine" => changed["machine"]["max_stitches_per_minute"] = json!(1500.0),
            "qualification" => {
                changed["qualifications"][0]["method"] =
                    json!("Another qualification under the same ID")
            }
            "consent" => changed["production"]["workholding"]["allow_estimates"] = json!(false),
            "source" => changed["production"]["workholding"]["source"]["note"] = json!(""),
            "coverage" => {
                changed["production"]["workholding"]["qualifications"]
                    .as_object_mut()
                    .unwrap()
                    .remove("b");
            }
            "selected" => {
                changed["production"]["workholding"]["qualifications"]["b"] =
                    changed["production"]["workholding"]["qualifications"]["a"].clone()
            }
            "material" => {
                material["fabric"]["material"]["record"]["properties"]["thickness"]["value"] =
                    json!(0.7);
                let mut unqualified = changed.clone();
                unqualified["production"]
                    .as_object_mut()
                    .unwrap()
                    .remove("workholding");
                unqualified["qualifications"] = json!([]);
                unqualified["require_qualification"] = json!(false);
                let fresh = build(material.clone(), unqualified).unwrap();
                for qualification in changed["qualifications"].as_array_mut().unwrap() {
                    qualification["materials"] = json!(
                        fresh.seams[0]
                            .sides
                            .iter()
                            .map(|side| &side.material_sha256)
                            .collect::<Vec<_>>()
                    );
                }
            }
            _ => unreachable!(),
        }
        assert!(build(material, changed).is_err(), "{defect}");
    }

    // Even a freshly valid resource reference cannot admit a sewing recipe outside its process envelope.
    options["require_qualification"] = json!(false);
    options["qualifications"][1]["stitches_per_minute"] = json!([1.0, 2.0]);
    let typed: SewOptions = serde_json::from_value(options.clone()).unwrap();
    options["production"]["workholding"]["qualifications"]["b"][0]["sha256"] =
        json!(workholding::qualification_sha256(&typed.qualifications[1]).unwrap());
    assert!(
        build(pattern, options)
            .unwrap_err()
            .message
            .contains("actual admitted sewing qualification")
    );
}

#[test]
fn resource_counts_and_expansion_are_bounded_before_search() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    fixtures(&pattern, &mut options, 2, one_hold());
    for (field, value) in [
        ("capacity", 0),
        ("capacity", 4097),
        ("units", 0),
        ("units", 4097),
    ] {
        let mut wrong = options.clone();
        let row = if field == "capacity" {
            "resources"
        } else {
            "holds"
        };
        wrong["production"]["workholding"][row][0][field] = json!(value);
        assert!(build(pattern.clone(), wrong).is_err());
    }
    let mut too_many = options.clone();
    too_many["production"]["workholding"]["holds"] = json!(
        (0..65)
            .map(|index| {
                let mut hold = one_hold()[0].clone();
                hold["id"] = json!(format!("hold-{index}"));
                hold
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(
        build(pattern.clone(), too_many).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );

    options["passes"].as_array_mut().unwrap().truncate(1);
    options["qualifications"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    batch(&mut options, 63);
    fixtures(
        &pattern,
        &mut options,
        64,
        json!((0..64).map(|index| json!({
        "id":format!("hold-{index}"),"resource":"jig","units":1,"acquire":"a","release":"a"
    })).collect::<Vec<_>>()),
    );
    assert_eq!(
        build(pattern, options).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
}

#[test]
fn qualified_resource_order_replays_through_the_real_stitch_program_and_omission_is_unchanged() {
    let (pattern, mut options) = fixture();
    batch(&mut options, 3);
    timed(&mut options);
    let legacy = build(pattern.clone(), options.clone()).unwrap();
    let legacy_json = serde_json::to_value(&legacy).unwrap();
    assert!(legacy_json["production"].get("workholding").is_none());
    assert!(
        legacy_json["options"]["production"]
            .get("workholding")
            .is_none()
    );
    fixtures(&pattern, &mut options, 2, one_hold());
    let plan = build(pattern.clone(), options.clone()).unwrap();
    let replay = SewPlan::build(&Pattern::new(pattern.clone()), &plan.options).unwrap();
    assert_eq!(
        serde_json::to_value(&plan).unwrap(),
        serde_json::to_value(replay).unwrap()
    );
    let program = SewProgram::build(&Pattern::new(pattern), &plan.options).unwrap();
    assert_eq!(
        serde_json::to_value(&program.plan).unwrap(),
        serde_json::to_value(&plan).unwrap()
    );
    let actual: Vec<_> = program
        .stitches
        .iter()
        .filter(|step| step.begins_run)
        .map(|step| {
            format!(
                "{}/{}",
                step.copy.unwrap(),
                plan.seams[step.operation].pass.operation_id()
            )
        })
        .collect();
    assert_eq!(actual, plan.ordering_report().unwrap().selected_order);
    assert_eq!(program.stitches.len(), legacy.total_stitch_intervals());
    assert_eq!(
        serde_json::to_value(&plan.options).unwrap()["production"]["workholding"],
        options["production"]["workholding"]
    );
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
