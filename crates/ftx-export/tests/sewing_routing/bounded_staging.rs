//! Synthetic finite slot reservations, distinct from measured factory storage or pickup timing.

use super::{
    attending_labor, availability_calendars, build, capture_routed_fixture, eligible_fixture,
    fixture, reentrant, refresh_locks, source, worker_owned_transfer,
};
use ftx_core::RefusalCode;
use ftx_export::{
    sewing::{SewPlan, production::routing::staging, program::SewProgram},
    sewing_selection::Request,
};
use serde_json::{Value, json};

fn bounded(mut options: Value, capacity: usize) -> Value {
    let route = &mut options["production"]["routing"];
    route["staging_capacity"] = json!(capacity);
    route["bounded_staging"] = json!({
        "model":"finite_unloaded_cell_buffers/1",
        "source":source("finite-cell-slots"),
        "method":"Synthetic cell slots with entry at first loading and exit at final unloading; independent handoffs reserve both cells throughout the complete qualified handoff",
        "conditions":{},"evidence":"estimate","allow_estimates":true
    });
    options
}

fn intervals(timeline: &staging::Timeline, cell: &str, kind: &str) -> Vec<(usize, u64, u64)> {
    let mut rows: Vec<_> = timeline
        .reservations
        .iter()
        .filter(|row| row.cell == cell && row.kind == kind)
        .map(|row| (row.copy, row.start_milliseconds, row.finish_milliseconds))
        .collect();
    rows.sort_unstable();
    rows
}

#[test]
fn finite_slots_require_their_own_source_and_consent_and_preserve_legacy_omission() {
    let (pattern, raw) = fixture(2);
    let legacy = serde_json::to_value(build(&pattern, raw.clone()).unwrap()).unwrap();
    assert!(
        legacy["production"]["routing"]
            .get("bounded_staging")
            .is_none()
    );
    let mut null = raw.clone();
    null["production"]["routing"]["bounded_staging"] = Value::Null;
    assert_eq!(
        legacy,
        serde_json::to_value(build(&pattern, null).unwrap()).unwrap()
    );
    let mut undersized_legacy = raw.clone();
    undersized_legacy["production"]["routing"]["staging_capacity"] = json!(1);
    assert!(build(&pattern, undersized_legacy).is_err());

    let raw = bounded(raw, 1);
    assert!(build(&pattern, raw.clone()).is_ok());
    for (field, value) in [
        ("model", json!("implicit_unbounded_storage/1")),
        ("method", json!("")),
        ("allow_estimates", json!(false)),
    ] {
        let mut invalid = raw.clone();
        invalid["production"]["routing"]["bounded_staging"][field] = value;
        assert!(
            build(&pattern, invalid).is_err(),
            "admitted invalid {field}"
        );
    }
    let mut missing_consent = raw.clone();
    missing_consent["production"]["routing"]["bounded_staging"]
        .as_object_mut()
        .unwrap()
        .remove("allow_estimates");
    assert!(build(&pattern, missing_consent).is_err());
    let mut missing_source = raw.clone();
    missing_source["production"]["routing"]["bounded_staging"]["source"]["reference"] = json!("");
    assert!(build(&pattern, missing_source).is_err());
    for capacity in [0, 4097] {
        let mut invalid = raw.clone();
        invalid["production"]["routing"]["staging_capacity"] = json!(capacity);
        assert!(build(&pattern, invalid).is_err());
    }
}

#[test]
fn independent_handoffs_reserve_both_cells_and_retain_a_complete_capped_replay() {
    let (pattern, mut raw) = fixture(3);
    raw["production"]["routing"]["transfer"]["transfers"][0]["milliseconds"] = json!(10000);
    raw["production"]["routing"]["max_search_nodes"] = json!(6);
    let raw = bounded(raw, 1);
    let program =
        SewProgram::build(&pattern, &serde_json::from_value(raw.clone()).unwrap()).unwrap();
    let production = program.plan.production.as_ref().unwrap();
    let route = production.routing.as_ref().unwrap();
    let slots = route.bounded_staging.as_ref().unwrap();
    assert_eq!(program.plan.schema, "ftx-sewing-plan/26");
    assert_eq!(production.schema, "ftx-sewing-production/14");
    assert_eq!(route.schema, "ftx-sewing-routing/9");
    assert_eq!(slots.schema, "ftx-sewing-routing-staging/1");
    assert_eq!(slots.capacity, 1);
    assert_eq!(
        slots.entry_exit_protocol,
        "first_loading_entry_final_unloading_exit/1"
    );
    assert_eq!(
        slots.handoff_reservations,
        "source_until_handoff_finish_destination_from_handoff_start/1"
    );
    assert_eq!(
        serde_json::to_value(&slots.policy).unwrap(),
        raw["production"]["routing"]["bounded_staging"]
    );
    assert!(!slots.physical_staging_validated && !route.physical_routing_validated);
    assert_eq!((route.search_nodes, route.work_used), (6, 18));
    assert!(route.budget_exhausted && !route.search_complete && !route.optimality_proven);
    assert_eq!(route.selected.makespan_milliseconds, 37600);
    assert_eq!(route.selected.operations.len(), 6);
    assert_eq!(route.selected.transfers.len(), 3);
    assert_eq!(program.stitches.len(), 180);
    assert_eq!(route.selected.scheduled_feed_milliseconds, 18000);
    assert_eq!(route.selected.setup_milliseconds, 1500);
    assert_eq!(route.selected.handling_milliseconds, 1800);
    assert_eq!(route.selected.transfer_milliseconds, 30000);
    let a: Vec<_> = route
        .selected
        .operations
        .iter()
        .filter(|row| row.cell == "cell-a")
        .map(|row| {
            (
                row.copy,
                row.loading_start_milliseconds,
                row.finish_milliseconds,
            )
        })
        .collect();
    // A complete handoff includes unlocated positioning/return. Its source slot cannot be
    // released at the start; later full operations finish exactly at prior handoff finishes.
    assert_eq!(a, [(0, 0, 5300), (1, 11000, 15300), (2, 21000, 25300)]);
    let expected = [(0, 5300, 15300), (1, 15300, 25300), (2, 25300, 35300)];
    assert_eq!(
        intervals(&slots.selected, "cell-a", "after_unloading"),
        expected
    );
    assert_eq!(
        intervals(&slots.selected, "cell-b", "before_loading"),
        expected
    );
    assert_eq!(slots.selected.reservations.len(), 6);
    for cell in ["cell-a", "cell-b"] {
        assert_eq!(slots.selected.peak_reserved_slots[cell], 1);
        assert_eq!(slots.selected.final_reserved_slots[cell], 0);
    }
    assert_eq!(
        serde_json::to_value(&slots.baseline).unwrap(),
        serde_json::to_value(&slots.selected).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
    capture_routed_fixture("bounded-staging-pattern.json", pattern.raw());
    capture_routed_fixture("bounded-staging-options.json", &program.plan.options);
    capture_routed_fixture("bounded-staging-program.json", &program);
}

#[test]
fn owned_handoffs_release_at_pickup_and_acquire_only_after_deposit() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["cells"][1]["available_at_milliseconds"] = json!(20000);
    raw["production"]["routing"]["transfer"]["transfers"][0]["milliseconds"] = json!(10000);
    raw["production"]["routing"]["max_search_nodes"] = json!(4);
    refresh_locks(&mut raw);
    let mut raw = bounded(
        worker_owned_transfer(attending_labor(raw, &[&["cell-a"], &["cell-b"]], 0), 1, 0),
        1,
    );
    raw["production"]["routing"]["bounded_staging"]["method"] = json!(
        "Synthetic first-load entry and final-unload exit; actual source pickup releases its slot and destination deposit acquires its slot"
    );
    let plan = build(&pattern, raw).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    let slots = route.bounded_staging.as_ref().unwrap();
    assert_eq!(
        slots.handoff_reservations,
        "source_until_pickup_destination_from_deposit/1"
    );
    assert_eq!(route.selected.makespan_milliseconds, 35100);
    let transfers: Vec<_> = route
        .selected
        .transfers
        .iter()
        .map(|row| (row.start_milliseconds, row.finish_milliseconds))
        .collect();
    assert_eq!(transfers, [(5300, 15300), (22800, 32800)]);
    assert_eq!(
        intervals(&slots.selected, "cell-a", "after_unloading"),
        [(1, 9600, 22800)]
    );
    assert_eq!(
        intervals(&slots.selected, "cell-b", "before_loading"),
        [(0, 15300, 20000)]
    );
    // Immediate pickup or immediate loading has zero slot duration and is omitted.
    assert_eq!(slots.selected.reservations.len(), 2);
    assert_eq!(
        route
            .attending_labor
            .as_ref()
            .unwrap()
            .selected
            .transfer_milliseconds,
        Some(20000)
    );
    for cell in ["cell-a", "cell-b"] {
        assert_eq!(slots.selected.peak_reserved_slots[cell], 1);
        assert_eq!(slots.selected.final_reserved_slots[cell], 0);
    }
}

#[test]
fn destination_backpressure_refits_complete_operations_to_cell_and_worker_calendars() {
    let (pattern, mut raw) = fixture(3);
    raw["production"]["routing"]["cells"][1]["available_at_milliseconds"] = json!(20000);
    raw["production"]["routing"]["max_search_nodes"] = json!(6);
    refresh_locks(&mut raw);
    let raw = bounded(attending_labor(raw, &[&["cell-a"], &["cell-b"]], 0), 1);
    let raw = availability_calendars(
        raw,
        json!({
            "cell-a":[{"start_milliseconds":0,"end_milliseconds":13900},
                      {"start_milliseconds":19000,"end_milliseconds":24300}],
            "worker-0":[{"start_milliseconds":0,"end_milliseconds":13900},
                        {"start_milliseconds":20000,"end_milliseconds":24300}]
        }),
    );
    let plan = build(&pattern, raw).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    let slots = route.bounded_staging.as_ref().unwrap();
    assert_eq!(route.selected.makespan_milliseconds, 27400);
    let transfers: Vec<_> = route
        .selected
        .transfers
        .iter()
        .map(|row| (row.start_milliseconds, row.finish_milliseconds))
        .collect();
    assert_eq!(transfers, [(5300, 6000), (20000, 20700), (24300, 25000)]);
    assert_eq!(
        intervals(&slots.selected, "cell-b", "before_loading"),
        [(0, 5300, 20000), (1, 20000, 22800), (2, 24300, 25100)]
    );
    let last_a = route
        .selected
        .operations
        .iter()
        .find(|row| row.id == "2/join")
        .unwrap();
    // Its nominal [9600,13900] operation fits the first windows, but output would overflow.
    // Buffer delay must recheck the whole operation and worker interval in the next windows.
    assert_eq!(
        (
            last_a.loading_start_milliseconds,
            last_a.finish_milliseconds
        ),
        (20000, 24300)
    );
    assert_eq!(
        (
            last_a.setup_start_milliseconds,
            last_a.feed_start_milliseconds,
            last_a.feed_finish_milliseconds
        ),
        (20100, 20100, 24100)
    );
    let attended = route
        .attending_labor
        .as_ref()
        .unwrap()
        .selected
        .steps
        .iter()
        .find(|row| row.operation == "2/join")
        .unwrap();
    assert_eq!(
        (
            attended.departure_milliseconds,
            attended.loading_start_milliseconds,
            attended.finish_milliseconds
        ),
        (20000, 20000, 24300)
    );
    assert_eq!(slots.selected.peak_reserved_slots["cell-a"], 1);
    assert_eq!(slots.selected.peak_reserved_slots["cell-b"], 1);
    assert!(
        slots
            .selected
            .final_reserved_slots
            .values()
            .all(|&count| count == 0)
    );
}

#[test]
fn a_full_source_buffer_cannot_escape_its_inclusive_complete_operation_window() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["transfer"]["available_at_milliseconds"] = json!(20000);
    for (capacity, end, feasible) in [
        (1, 9600, false),
        (2, 9600, true),
        (1, 20700, true),
        (1, 20699, false),
    ] {
        let mut options = bounded(raw.clone(), capacity);
        options["production"]["routing"]["cells"][0]["available_until_milliseconds"] = json!(end);
        refresh_locks(&mut options);
        let result = build(&pattern, options);
        if !feasible {
            assert_eq!(result.unwrap_err().code, RefusalCode::NotConverged);
            continue;
        }
        let plan = result.unwrap();
        let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
        assert_eq!(route.selected.makespan_milliseconds, 25800);
        assert_eq!(route.selected.operations.len(), 4);
        assert_eq!(plan.total_stitch_intervals(), 120);
        let slots = route.bounded_staging.as_ref().unwrap();
        assert_eq!(slots.selected.peak_reserved_slots["cell-a"], capacity);
        assert_eq!(slots.selected.final_reserved_slots["cell-a"], 0);
        assert!(
            route
                .selected
                .operations
                .iter()
                .filter(|row| row.cell == "cell-a")
                .all(|row| row.finish_milliseconds <= end)
        );
    }
}

#[test]
fn reentrant_one_slot_routes_close_actual_handoffs_without_false_deadlock() {
    let (pattern, raw) = reentrant();
    let program =
        SewProgram::build(&pattern, &serde_json::from_value(bounded(raw, 1)).unwrap()).unwrap();
    let route = program
        .plan
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    let slots = route.bounded_staging.as_ref().unwrap();
    assert_eq!(route.baseline.makespan_milliseconds, 41100);
    assert_eq!(route.selected.makespan_milliseconds, 23200);
    assert!(route.optimality_proven);
    assert_eq!(route.selected.operations.len(), 6);
    assert_eq!(route.selected.transfers.len(), 4);
    assert_eq!(program.stitches.len(), 200);
    // A return can be reserved before an already dispatched output, provided it loads exactly
    // when that output unloads. Open output tails must close only on that garment's handoff.
    assert!(
        slots
            .selected
            .reservations
            .iter()
            .any(|row| row.cell == "cell-a"
                && row.kind == "before_loading"
                && row.start_milliseconds == 8800
                && row.finish_milliseconds == 9600)
    );
    for cell in ["cell-a", "cell-b"] {
        assert_eq!(slots.selected.peak_reserved_slots[cell], 1);
        assert_eq!(slots.selected.final_reserved_slots[cell], 0);
    }
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
}

#[test]
fn eligible_slot_reservations_follow_actual_cells_and_do_not_complete_a_partial_seed() {
    let (pattern, raw) = eligible_fixture(2);
    let mut capped = bounded(raw.clone(), 1);
    capped["production"]["routing"]["max_search_nodes"] = json!(4);
    assert_eq!(
        build(&pattern, capped).unwrap_err().code,
        RefusalCode::NotConverged
    );
    let program =
        SewProgram::build(&pattern, &serde_json::from_value(bounded(raw, 1)).unwrap()).unwrap();
    let route = program
        .plan
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    let slots = route.bounded_staging.as_ref().unwrap();
    // One C join (3300), its 900 handoff and B's first 2800 operation finish at 7000.
    // A's independent 5300 join and 700 handoff stage the other garment for B's final 2300.
    assert_eq!(route.selected.makespan_milliseconds, 9300);
    assert!(route.baseline_replaced && route.search_complete && !route.budget_exhausted);
    assert!(!route.optimality_proven);
    assert_eq!(program.stitches.len(), 100);
    for reservation in &slots.selected.reservations {
        let operation = route
            .selected
            .operations
            .iter()
            .find(|row| row.id == reservation.operation)
            .unwrap();
        assert_eq!(reservation.cell, operation.cell);
        assert_eq!(reservation.copy, operation.copy);
    }
    for cell in ["cell-a", "cell-b", "cell-c"] {
        assert_eq!(slots.selected.peak_reserved_slots[cell], 1);
        assert_eq!(slots.selected.final_reserved_slots[cell], 0);
    }
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
}

#[test]
fn route_selection_preserves_a_complete_buffer_feasible_winner_and_unresolved_alternative() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["cells"][0]["available_until_milliseconds"] = json!(9600);
    raw["production"]["routing"]["transfer"]["available_at_milliseconds"] = json!(20000);
    refresh_locks(&mut raw);
    let template = bounded(raw.clone(), 1);
    let mut alternative = bounded(raw, 2);
    alternative["production"]["routing"]["bounded_staging"]["source"] =
        source("two-qualified-slots");
    let policy = alternative["production"]["routing"]["bounded_staging"].clone();
    let request: Request = serde_json::from_value(json!({
        "schema":"ftx-sewing-selection/6","options":template,
        "objective":"minimize_parallel_makespan","max_plan_evaluations":2,
        "route_candidates":[{"id":"two-qualified-slots","routing":alternative["production"]["routing"]}]
    })).unwrap();
    let result = request.select(&pattern).unwrap();
    assert_eq!(
        result.selected_route.as_deref(),
        Some("two-qualified-slots")
    );
    assert_eq!(result.trials.len(), 2);
    assert_eq!(
        result.trials[0].refusal.as_ref().unwrap().code,
        RefusalCode::NotConverged
    );
    assert!(result.trials[0].objective_seconds.is_none());
    assert!(result.search_complete && !result.optimality_proven && result.unresolved_trials > 0);
    assert_eq!(result.selected_objective_seconds, Some(25.8));
    let plan = result.selected_plan.as_ref().unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert_eq!(route.bounded_staging.as_ref().unwrap().capacity, 2);
    assert_eq!(
        serde_json::to_value(&route.bounded_staging.as_ref().unwrap().policy).unwrap(),
        policy
    );
    assert_eq!(plan.total_stitch_intervals(), 120);
    assert_eq!(
        serde_json::to_value(plan).unwrap(),
        serde_json::to_value(SewPlan::build(&pattern, &plan.options).unwrap()).unwrap()
    );
}
