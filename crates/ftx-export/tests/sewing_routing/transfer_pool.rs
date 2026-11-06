//! Invented independent movers exercise resource scheduling, not measured factory capacity.

use super::{
    attending_labor, availability_calendars, build, capture_routed_fixture, eligible_fixture,
    fixture, retained_fixtures, source, worker_owned_transfer,
};
use ftx_core::RefusalCode;
use ftx_export::sewing::program::SewProgram;
use serde_json::{Value, json};

fn pooled(mut options: Value, duration: u32) -> Value {
    let route = &mut options["production"]["routing"];
    let mut extra = route["transfer"].clone();
    extra["id"] = json!("second-mover");
    extra["source"] = source("second-mover");
    for edge in extra["transfers"].as_array_mut().unwrap() {
        edge["milliseconds"] = json!(duration);
    }
    route["transfer_pool"] = json!({
        "model":"independent_unloaded_transfer_resources/1",
        "additional_resources":[extra], "source":source("independent-movers"),
        "method":"Synthetic separately staffed movers can execute their complete ready-state-to-ready-state handoffs simultaneously, independently of attending workers and each other",
        "conditions":{}, "evidence":"estimate", "allow_estimates":true
    });
    options
}

fn bounded(mut options: Value, capacity: usize) -> Value {
    options["production"]["routing"]["staging_capacity"] = json!(capacity);
    options["production"]["routing"]["bounded_staging"] = json!({
        "model":"finite_unloaded_cell_buffers/1", "source":source("pooled-transfer-slots"),
        "method":"Synthetic finite cell slots; each independent handoff occupies both its source and destination slots through its complete qualified duration",
        "conditions":{}, "evidence":"estimate", "allow_estimates":true
    });
    options
}

#[test]
fn independent_movers_overlap_complete_handoffs_and_replay_actual_stitches() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["transfer"]["transfers"][0]["milliseconds"] = json!(10000);
    let raw = pooled(raw, 10000);
    let program =
        SewProgram::build(&pattern, &serde_json::from_value(raw.clone()).unwrap()).unwrap();
    let production = program.plan.production.as_ref().unwrap();
    let route = production.routing.as_ref().unwrap();
    assert_eq!(program.plan.schema, "ftx-sewing-plan/27");
    assert_eq!(production.schema, "ftx-sewing-production/15");
    assert_eq!(route.schema, "ftx-sewing-routing/10");
    assert_eq!(route.baseline.makespan_milliseconds, 27600);
    assert_eq!(route.selected.makespan_milliseconds, 21900);
    assert_eq!(route.selected.operations.len(), 4);
    assert_eq!(route.selected.scheduled_feed_milliseconds, 12000);
    assert_eq!(route.selected.setup_milliseconds, 1500);
    assert_eq!(route.selected.handling_milliseconds, 1200);
    assert_eq!(route.selected.transfer_milliseconds, 20000);
    assert_eq!(program.stitches.len(), 120);
    assert!(route.lower_bound_makespan_milliseconds <= 21900);
    assert!(route.search_complete && !route.budget_exhausted && !route.baseline_replaced);
    assert!(!route.physical_routing_validated);
    let mut handoffs: Vec<_> = route
        .selected
        .transfers
        .iter()
        .map(|row| {
            (
                row.start_milliseconds,
                row.finish_milliseconds,
                row.resource.as_deref().unwrap(),
            )
        })
        .collect();
    handoffs.sort_unstable();
    // The first A operation ends at 5300; its next copy ends at 9600. Both movers can work
    // together while B completes its first 2800 ms and later 2300 ms operation.
    assert_eq!(
        handoffs.iter().map(|&(a, b, _)| (a, b)).collect::<Vec<_>>(),
        [(5300, 15300), (9600, 19600)]
    );
    assert_ne!(handoffs[0].2, handoffs[1].2);
    let report = serde_json::to_value(route).unwrap();
    let pool = &report["transfer_pool"];
    assert_eq!(pool["schema"], "ftx-sewing-transfer-pool/1");
    assert_eq!(
        pool["policy"],
        raw["production"]["routing"]["transfer_pool"]
    );
    assert_eq!(pool["primary"], raw["production"]["routing"]["transfer"]);
    assert_eq!(pool["physical_pool_validated"], false);
    for use_row in pool["selected"].as_array().unwrap() {
        assert_eq!(use_row["handoffs"], 1);
        assert_eq!(use_row["occupied_milliseconds"], 10000);
    }
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
    capture_routed_fixture("transfer-pool-pattern.json", pattern.raw());
    capture_routed_fixture("transfer-pool-options.json", &program.plan.options);
    capture_routed_fixture("transfer-pool-program.json", &program);
}

#[test]
fn fast_carrier_controls_actual_duration_and_late_unused_carriers_do_not_raise_the_bound() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["transfer"]["transfers"][0]["milliseconds"] = json!(10000);
    let mut raw = pooled(raw, 100);
    let extras = raw["production"]["routing"]["transfer_pool"]["additional_resources"]
        .as_array_mut()
        .unwrap();
    let mut late = extras[0].clone();
    late["id"] = json!("late-unused-mover");
    late["source"] = source("late-unused-mover");
    late["available_at_milliseconds"] = json!(86400000);
    extras.push(late);
    // This also exercises independent finite-buffer reconstruction with selected durations that
    // differ from the primary transfer table. Neither handoff lasts the primary's 10000 ms.
    let plan = build(&pattern, bounded(raw, 2)).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert_eq!(route.selected.makespan_milliseconds, 12000);
    assert!(route.lower_bound_makespan_milliseconds <= 12000);
    assert_eq!(route.selected.transfer_milliseconds, 200);
    assert_eq!(route.selected.transfers.len(), 2);
    for row in &route.selected.transfers {
        assert_eq!(row.resource.as_deref(), Some("second-mover"));
        assert_eq!(row.finish_milliseconds - row.start_milliseconds, 100);
    }
    let report = serde_json::to_value(route).unwrap();
    let uses = report["transfer_pool"]["selected"].as_array().unwrap();
    assert_eq!(uses.len(), 3);
    for id in ["handoff-worker", "late-unused-mover"] {
        let unused = uses.iter().find(|row| row["resource"] == id).unwrap();
        assert_eq!(unused["handoffs"], 0);
        assert_eq!(unused["occupied_milliseconds"], 0);
        assert!(unused["first_start_milliseconds"].is_null());
        assert!(unused["last_finish_milliseconds"].is_null());
    }
    let slots = route.bounded_staging.as_ref().unwrap();
    assert!(
        slots
            .selected
            .reservations
            .iter()
            .all(|row| { row.finish_milliseconds - row.start_milliseconds == 100 })
    );
    for cell in ["cell-a", "cell-b"] {
        assert_eq!(slots.selected.final_reserved_slots[cell], 0);
    }
}

#[test]
fn eligible_routes_use_only_each_carriers_qualified_directed_edges() {
    let (pattern, mut raw) = eligible_fixture(2);
    for edge in raw["production"]["routing"]["transfer"]["transfers"]
        .as_array_mut()
        .unwrap()
    {
        edge["milliseconds"] = json!(10000);
    }
    let mut raw = pooled(raw, 100);
    raw["production"]["routing"]["transfer_pool"]["additional_resources"][0]["transfers"] =
        json!([{"from":"cell-c", "to":"cell-b", "milliseconds":100}]);
    let program = SewProgram::build(&pattern, &serde_json::from_value(raw).unwrap()).unwrap();
    let route = program
        .plan
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    // C must unload by 3300 and A by 5300, forcing one copy per eligible first cell. C reaches
    // B at 3400 and finishes at 6200; A arrives at 15300 and finishes B's next operation at 17600.
    assert_eq!(route.selected.makespan_milliseconds, 17600);
    assert_eq!(route.selected.transfer_milliseconds, 10100);
    assert_eq!(program.stitches.len(), 100);
    assert!(route.baseline_replaced && route.search_complete);
    assert!(route.lower_bound_makespan_milliseconds <= 17600);
    assert_eq!(route.selected.transfers.len(), 2);
    for row in &route.selected.transfers {
        let (carrier, duration) = match row.from.as_str() {
            "cell-a" => ("handoff-worker", 10000),
            "cell-c" => ("second-mover", 100),
            other => panic!("unexpected actual source cell {other}"),
        };
        assert_eq!(row.to, "cell-b");
        assert_eq!(row.resource.as_deref(), Some(carrier));
        assert_eq!(row.finish_milliseconds - row.start_milliseconds, duration);
    }
}

#[test]
fn carrier_calendars_admit_whole_handoffs_and_replace_an_unavailable_primary_seed() {
    let (pattern, mut raw) = fixture(1);
    raw["production"]["routing"]["transfer"]["transfers"][0]["milliseconds"] = json!(10000);
    let mut raw = pooled(raw, 10000);
    raw["production"]["routing"]["transfer"]["available_until_milliseconds"] = json!(15299);
    raw["production"]["routing"]["transfer_pool"]["additional_resources"][0]["available_until_milliseconds"] =
        json!(27000);
    let raw = availability_calendars(
        raw,
        json!({
            "handoff-worker":[{"start_milliseconds":0,"end_milliseconds":15299}],
            "second-mover":[{"start_milliseconds":0,"end_milliseconds":15299},
                {"start_milliseconds":17000,"end_milliseconds":27000}]
        }),
    );
    let plan = build(&pattern, raw.clone()).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert!(route.baseline_replaced && route.search_complete);
    assert_eq!(route.selected.makespan_milliseconds, 29800);
    let handoff = &route.selected.transfers[0];
    assert_eq!(handoff.resource.as_deref(), Some("second-mover"));
    assert_eq!(
        (handoff.start_milliseconds, handoff.finish_milliseconds),
        (17000, 27000)
    );
    assert_eq!(
        serde_json::to_value(route.availability_calendars.as_ref().unwrap()).unwrap(),
        raw["production"]["routing"]["availability_calendars"]
    );
    let mut short = raw;
    short["production"]["routing"]["availability_calendars"]["resources"]["second-mover"][1]["end_milliseconds"] =
        json!(26999);
    assert_eq!(
        build(&pattern, short).unwrap_err().code,
        RefusalCode::NotConverged
    );
}

#[test]
fn multiple_movers_still_respect_shared_finite_cell_buffer_capacity() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["transfer"]["transfers"][0]["milliseconds"] = json!(10000);
    let raw = pooled(raw, 10000);
    for (capacity, finish) in [(1, 27600), (2, 21900)] {
        let program = SewProgram::build(
            &pattern,
            &serde_json::from_value(bounded(raw.clone(), capacity)).unwrap(),
        )
        .unwrap();
        let route = program
            .plan
            .production
            .as_ref()
            .unwrap()
            .routing
            .as_ref()
            .unwrap();
        assert_eq!(route.selected.makespan_milliseconds, finish);
        assert_eq!(program.stitches.len(), 120);
        let slots = route.bounded_staging.as_ref().unwrap();
        for cell in ["cell-a", "cell-b"] {
            assert_eq!(slots.selected.peak_reserved_slots[cell], capacity);
            assert_eq!(slots.selected.final_reserved_slots[cell], 0);
        }
        let mut transfers: Vec<_> = route.selected.transfers.iter().collect();
        transfers.sort_unstable_by_key(|row| row.start_milliseconds);
        assert_eq!(transfers.len(), 2);
        assert_eq!(
            transfers[0].finish_milliseconds <= transfers[1].start_milliseconds,
            capacity == 1
        );
        assert_eq!(
            serde_json::to_value(&program).unwrap(),
            serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap())
                .unwrap()
        );
    }
}

#[test]
fn independent_movers_do_not_duplicate_or_release_a_shared_machine_attendant() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["transfer"]["transfers"][0]["milliseconds"] = json!(10000);
    let raw = attending_labor(pooled(raw, 10000), &[&["cell-a", "cell-b"]], 0);
    let plan = build(&pattern, raw).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert_eq!(route.selected.makespan_milliseconds, 21900);
    let labor = route.attending_labor.as_ref().unwrap();
    assert_eq!(labor.selected.steps.len(), 4);
    assert_eq!(labor.selected.attended_milliseconds, 14700);
    assert_eq!(labor.selected.travel_milliseconds, 0);
    assert_eq!(labor.selected.waiting_milliseconds, 7200);
    assert!(labor.transfer.is_none() && labor.selected.transfers.is_none());
    assert!(
        labor
            .selected
            .steps
            .iter()
            .all(|row| row.worker == "worker-0")
    );
    assert!(
        labor
            .selected
            .steps
            .windows(2)
            .all(|pair| { pair[0].finish_milliseconds <= pair[1].departure_milliseconds })
    );
    assert_eq!(
        route
            .selected
            .operations
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        labor
            .selected
            .steps
            .iter()
            .map(|row| row.operation.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn pooled_handoffs_preserve_retained_fixture_lifetimes_shared_labor_and_finite_buffers() {
    let (pattern, raw) = retained_fixtures(1, Some(2));
    let raw = bounded(
        attending_labor(pooled(raw, 100), &[&["cell-a", "cell-b"]], 0),
        1,
    );
    let program = SewProgram::build(&pattern, &serde_json::from_value(raw).unwrap()).unwrap();
    let route = program
        .plan
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    // A retains its sole jig through both complete handoffs and the B operation. Faster movers
    // cannot manufacture another jig, release the attending worker during feed, or skip unloading.
    assert_eq!(route.baseline.makespan_milliseconds, 41100);
    assert_eq!(route.selected.makespan_milliseconds, 38700);
    assert_eq!(route.selected.operations.len(), 6);
    assert_eq!(route.selected.transfers.len(), 4);
    assert_eq!(route.selected.transfer_milliseconds, 400);
    assert_eq!(program.stitches.len(), 200);
    let fixtures = route.workholding.as_ref().unwrap();
    for (cell, expected) in [
        ("cell-a", [(0, 17600), (17600, 38700)]),
        ("cell-b", [(5400, 8200), (27000, 29300)]),
    ] {
        let mut intervals: Vec<_> = fixtures
            .selected
            .holds
            .iter()
            .filter(|hold| hold.cell == cell)
            .map(|hold| (hold.acquired_at_milliseconds, hold.released_at_milliseconds))
            .collect();
        intervals.sort_unstable();
        assert_eq!(intervals, expected);
        assert_eq!(fixtures.selected.local_peak_occupancy[cell]["jig"], 1);
        assert_eq!(fixtures.selected.local_final_occupancy[cell]["jig"], 0);
        let slots = &route.bounded_staging.as_ref().unwrap().selected;
        assert_eq!(slots.peak_reserved_slots[cell], 1);
        assert_eq!(slots.final_reserved_slots[cell], 0);
    }
    assert_eq!(fixtures.selected.shared_peak_occupancy["common-jigs"], 2);
    assert_eq!(fixtures.selected.shared_final_occupancy["common-jigs"], 0);
    let labor = &route.attending_labor.as_ref().unwrap().selected;
    assert_eq!(labor.steps.len(), 6);
    assert!(
        labor
            .steps
            .windows(2)
            .all(|pair| { pair[0].finish_milliseconds <= pair[1].departure_milliseconds })
    );
    assert!(route.selected.transfers.iter().all(|row| {
        row.resource.as_deref() == Some("second-mover")
            && row.finish_milliseconds - row.start_milliseconds == 100
    }));
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
}

#[test]
fn a_transfer_search_limit_retains_the_complete_primary_seed() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["transfer"]["transfers"][0]["milliseconds"] = json!(10000);
    raw["production"]["routing"]["max_search_nodes"] = json!(4);
    let plan = build(&pattern, pooled(raw, 10000)).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert!(route.budget_exhausted && !route.search_complete && !route.optimality_proven);
    assert_eq!((route.search_nodes, route.work_used), (4, 12));
    assert_eq!(route.selected.makespan_milliseconds, 27600);
    assert_eq!(route.selected.operations.len(), 4);
    assert_eq!(route.selected.transfers.len(), 2);
    assert!(
        route
            .selected
            .transfers
            .iter()
            .all(|row| { row.resource.as_deref() == Some("handoff-worker") })
    );
    assert_eq!(
        serde_json::to_value(&route.baseline).unwrap(),
        serde_json::to_value(&route.selected).unwrap()
    );
}

#[test]
fn transfer_pools_require_own_sources_distinct_resources_and_supported_edges() {
    let (pattern, raw) = fixture(1);
    let valid = pooled(raw, 700);
    assert!(build(&pattern, valid.clone()).is_ok());
    let prefix = "/production/routing/transfer_pool";
    for (field, value) in [
        ("model", json!("unqualified_parallel_transfers/1")),
        ("method", json!("")),
        ("source/reference", json!("")),
        ("allow_estimates", json!(false)),
        ("additional_resources/0/id", json!("handoff-worker")),
        ("additional_resources/0/id", json!("cell-a")),
        ("additional_resources/0/id", json!("")),
        (
            "additional_resources/0/model",
            json!("attended_source_to_destination_handoffs/1"),
        ),
        ("additional_resources/0/source/note", json!("")),
        ("additional_resources/0/allow_estimates", json!(false)),
        (
            "additional_resources/0/available_at_milliseconds",
            json!(86400001),
        ),
        ("additional_resources/0/transfers", json!([])),
        ("additional_resources/0/transfers/0/to", json!("cell-a")),
        (
            "additional_resources/0/transfers/0/from",
            json!("unknown-cell"),
        ),
        (
            "additional_resources/0/transfers/0/milliseconds",
            json!(86400001),
        ),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(&format!("{prefix}/{field}")).unwrap() = value;
        assert_eq!(
            build(&pattern, invalid).unwrap_err().code,
            RefusalCode::InvalidDocument,
            "{field}"
        );
    }
    for count in [0, 8] {
        let mut invalid = valid.clone();
        let extras = (0..count)
            .map(|index| {
                let mut extra =
                    valid["production"]["routing"]["transfer_pool"]["additional_resources"][0]
                        .clone();
                extra["id"] = json!(format!("extra-{index}"));
                extra
            })
            .collect::<Vec<_>>();
        invalid["production"]["routing"]["transfer_pool"]["additional_resources"] = json!(extras);
        assert!(
            build(&pattern, invalid).is_err(),
            "{count} additional carriers"
        );
    }
    let mut duplicate = valid.clone();
    let edge =
        duplicate["production"]["routing"]["transfer_pool"]["additional_resources"][0]["transfers"]
            [0]
        .clone();
    duplicate["production"]["routing"]["transfer_pool"]["additional_resources"][0]["transfers"] =
        json!([edge.clone(), edge]);
    assert!(build(&pattern, duplicate).is_err());
    let mut unqualified_primary = valid.clone();
    unqualified_primary["production"]["routing"]["transfer"]["transfers"] = json!([]);
    assert!(build(&pattern, unqualified_primary).is_err());
    let mut missing_consent = valid.clone();
    missing_consent["production"]["routing"]["transfer_pool"]
        .as_object_mut()
        .unwrap()
        .remove("allow_estimates");
    assert!(build(&pattern, missing_consent).is_err());

    let labor = attending_labor(valid, &[&["cell-a"], &["cell-b"]], 0);
    assert!(build(&pattern, labor.clone()).is_ok());
    let mut collision = labor.clone();
    collision["production"]["routing"]["transfer_pool"]["additional_resources"][0]["id"] =
        json!("worker-0");
    assert!(build(&pattern, collision).is_err());
    let mut owner = labor.clone();
    owner["production"]["routing"]["transfer_pool"]["additional_resources"][0]["attending_worker"] =
        json!("worker-0");
    assert!(build(&pattern, owner).is_err());
    assert!(build(&pattern, worker_owned_transfer(labor, 0, 0)).is_err());
}

#[test]
fn absent_transfer_pool_keeps_the_single_resource_wire_contract() {
    let (pattern, raw) = fixture(2);
    let legacy = serde_json::to_value(build(&pattern, raw.clone()).unwrap()).unwrap();
    let route = &legacy["production"]["routing"];
    assert_eq!(route["schema"], "ftx-sewing-routing/1");
    assert!(route.get("transfer_pool").is_none());
    for timeline in ["baseline", "selected"] {
        assert!(
            route[timeline]["transfers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| { row.get("resource").is_none() })
        );
    }
    let mut null = raw;
    null["production"]["routing"]["transfer_pool"] = Value::Null;
    assert_eq!(
        legacy,
        serde_json::to_value(build(&pattern, null).unwrap()).unwrap()
    );
}
