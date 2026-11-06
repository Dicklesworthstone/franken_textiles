//! Invented worker qualifications exercise bounded scheduling, not measured factory staffing.

use super::{
    attending_labor, availability_calendars, build, capture_routed_fixture, eligible_fixture,
    fixture, retained_fixtures, source, worker_owned_transfer,
};
use ftx_core::RefusalCode;
use ftx_export::sewing::program::SewProgram;
use serde_json::{Value, json};

fn eligible(options: Value, groups: &[&[&str]], travel: u32) -> Value {
    let mut options = attending_labor(options, groups, travel);
    let policy = &mut options["production"]["routing"]["attending_labor"];
    policy["model"] = json!("eligible_full_operation_attendance/1");
    policy["source"] = source("eligible-workers");
    policy["method"] = json!(
        "Synthetic workers independently qualify complete loading, setup, feed and unloading at every listed cell; explicit initial and directed travel, no cooperative or autonomous attendance"
    );
    options
}

fn additional_carrier(mut options: Value, duration: u32) -> Value {
    let route = &mut options["production"]["routing"];
    let mut carrier = route["transfer"].clone();
    carrier["id"] = json!("second-mover");
    carrier["source"] = source("eligible-labor-second-mover");
    for edge in carrier["transfers"].as_array_mut().unwrap() {
        edge["milliseconds"] = json!(duration);
    }
    route["transfer_pool"] = json!({
        "model":"independent_unloaded_transfer_resources/1", "additional_resources":[carrier],
        "source":source("eligible-labor-independent-movers"),
        "method":"Synthetic independently staffed carriers qualify simultaneous complete handoffs outside machine attendance",
        "conditions":{}, "evidence":"estimate", "allow_estimates":true
    });
    options
}

fn bounded(mut options: Value, capacity: usize) -> Value {
    options["production"]["routing"]["staging_capacity"] = json!(capacity);
    options["production"]["routing"]["bounded_staging"] = json!({
        "model":"finite_unloaded_cell_buffers/1", "source":source("eligible-labor-slots"),
        "method":"Synthetic cell staging occupies source and destination slots through each complete independent handoff",
        "conditions":{}, "evidence":"estimate", "allow_estimates":true
    });
    options
}

#[test]
fn interchangeable_workers_attend_overlapping_operations_and_replay_actual_stitches() {
    let (pattern, raw) = fixture(2);
    let single = build(
        &pattern,
        attending_labor(raw.clone(), &[&["cell-a", "cell-b"]], 0),
    )
    .unwrap();
    assert_eq!(
        single
            .production
            .as_ref()
            .unwrap()
            .routing
            .as_ref()
            .unwrap()
            .selected
            .makespan_milliseconds,
        14700
    );
    let raw = eligible(raw, &[&["cell-a", "cell-b"], &["cell-a", "cell-b"]], 0);
    let program =
        SewProgram::build(&pattern, &serde_json::from_value(raw.clone()).unwrap()).unwrap();
    let production = program.plan.production.as_ref().unwrap();
    let route = production.routing.as_ref().unwrap();
    let labor = route.attending_labor.as_ref().unwrap();
    assert_eq!(program.plan.schema, "ftx-sewing-plan/28");
    assert_eq!(production.schema, "ftx-sewing-production/16");
    assert_eq!(route.schema, "ftx-sewing-routing/11");
    assert_eq!(labor.schema, "ftx-sewing-attending-labor/3");
    assert_eq!(route.baseline.makespan_milliseconds, 16100);
    assert_eq!(route.selected.makespan_milliseconds, 12600);
    assert_eq!(route.selected.operations.len(), 4);
    assert_eq!(labor.selected.attended_milliseconds, 14700);
    assert_eq!(program.stitches.len(), 120);
    assert!(route.search_complete && !route.baseline_replaced);
    assert!(route.lower_bound_makespan_milliseconds <= 12600);
    assert!(!labor.physical_labor_validated && !program.plan.hardware_execution_authorized);
    assert_eq!(
        serde_json::to_value(&labor.policy).unwrap(),
        raw["production"]["routing"]["attending_labor"]
    );
    for (operation, step) in route.selected.operations.iter().zip(&labor.selected.steps) {
        assert_eq!(
            operation.attending_worker.as_deref(),
            Some(step.worker.as_str())
        );
        assert_eq!(step.operation, operation.id);
        assert_eq!(step.copy, operation.copy);
        assert_eq!(step.cell, operation.cell);
        assert_eq!(
            step.loading_start_milliseconds,
            operation.loading_start_milliseconds
        );
        assert_eq!(step.finish_milliseconds, operation.finish_milliseconds);
    }
    let a = labor
        .selected
        .steps
        .iter()
        .find(|row| row.operation == "1/join")
        .unwrap();
    let b = labor
        .selected
        .steps
        .iter()
        .find(|row| row.operation == "0/reinforce")
        .unwrap();
    assert_eq!(
        (a.loading_start_milliseconds, a.finish_milliseconds),
        (5300, 9600)
    );
    assert_eq!(
        (b.loading_start_milliseconds, b.finish_milliseconds),
        (6000, 8800)
    );
    assert_ne!(a.worker, b.worker);
    let usage = labor.selected.worker_usage.as_ref().unwrap();
    assert_eq!(
        usage
            .iter()
            .map(|row| row.worker.as_str())
            .collect::<Vec<_>>(),
        ["worker-0", "worker-1"]
    );
    assert_eq!(usage.iter().map(|row| row.operations).sum::<usize>(), 4);
    assert_eq!(
        usage
            .iter()
            .map(|row| row.attended_milliseconds)
            .sum::<u64>(),
        14700
    );
    assert_eq!(
        usage
            .iter()
            .map(|row| row.waiting_milliseconds)
            .sum::<u64>(),
        labor.selected.waiting_milliseconds
    );
    for worker in usage {
        let steps: Vec<_> = labor
            .selected
            .steps
            .iter()
            .filter(|row| row.worker == worker.worker)
            .collect();
        assert!(
            steps
                .windows(2)
                .all(|pair| pair[0].finish_milliseconds <= pair[1].departure_milliseconds)
        );
        assert_eq!((worker.handoffs, worker.transfer_milliseconds), (0, 0));
    }
    let idle = &labor.baseline.worker_usage.as_ref().unwrap()[1];
    assert_eq!(
        (idle.operations, idle.handoffs, idle.attended_milliseconds),
        (0, 0, 0)
    );
    assert!(idle.first_departure_milliseconds.is_none() && idle.last_finish_milliseconds.is_none());
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
    capture_routed_fixture("eligible-labor-pattern.json", pattern.raw());
    capture_routed_fixture("eligible-labor-options.json", &program.plan.options);
    capture_routed_fixture("eligible-labor-program.json", &program);
}

#[test]
fn more_workers_than_cells_leave_late_unused_qualifications_out_of_the_bound() {
    let (pattern, raw) = fixture(2);
    let mut raw = eligible(
        raw,
        &[
            &["cell-a", "cell-b"],
            &["cell-a", "cell-b"],
            &["cell-a", "cell-b"],
        ],
        0,
    );
    raw["production"]["routing"]["attending_labor"]["workers"][2]["available_at_milliseconds"] =
        json!(86400000);
    let plan = build(&pattern, raw.clone()).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert_eq!(route.selected.makespan_milliseconds, 12600);
    assert!(route.lower_bound_makespan_milliseconds <= 12600);
    let labor = route.attending_labor.as_ref().unwrap();
    assert_eq!(labor.policy.workers.len(), 3);
    let unused = &labor.selected.worker_usage.as_ref().unwrap()[2];
    assert_eq!(unused.worker, "worker-2");
    assert_eq!(
        (
            unused.operations,
            unused.handoffs,
            unused.travel_milliseconds,
            unused.waiting_milliseconds,
            unused.attended_milliseconds,
            unused.transfer_milliseconds
        ),
        (0, 0, 0, 0, 0, 0)
    );
    assert!(
        unused.first_departure_milliseconds.is_none() && unused.last_finish_milliseconds.is_none()
    );
    raw["production"]["routing"]["attending_labor"]["model"] =
        json!("exclusive_full_operation_attendance/1");
    assert_eq!(
        build(&pattern, raw).unwrap_err().code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn worker_calendars_cover_initial_travel_and_complete_attendance_after_seed_replacement() {
    let (pattern, raw) = fixture(1);
    let mut raw = eligible(raw, &[&["cell-a", "cell-b"], &["cell-a", "cell-b"]], 500);
    let workers = &mut raw["production"]["routing"]["attending_labor"]["workers"];
    workers[0]["available_until_milliseconds"] = json!(0);
    workers[1]["available_at_milliseconds"] = json!(1000);
    workers[1]["available_until_milliseconds"] = json!(17100);
    workers[1]["initial_arrival_milliseconds"]["cell-a"] = json!(300);
    let mut raw = availability_calendars(
        raw,
        json!({"worker-1":[
            {"start_milliseconds":1000,"end_milliseconds":6599},
            {"start_milliseconds":8000,"end_milliseconds":17100}
        ]}),
    );
    let plan = build(&pattern, raw.clone()).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    let labor = &route.attending_labor.as_ref().unwrap().selected;
    // The first window is one millisecond short of initial travel plus the complete A operation.
    assert!(route.baseline_replaced && route.search_complete);
    assert_eq!(route.selected.makespan_milliseconds, 17100);
    assert_eq!(
        (
            labor.travel_milliseconds,
            labor.waiting_milliseconds,
            labor.attended_milliseconds
        ),
        (800, 200, 8100)
    );
    let first = &labor.steps[0];
    assert_eq!(first.worker, "worker-1");
    assert_eq!(
        (
            first.departure_milliseconds,
            first.arrival_milliseconds,
            first.finish_milliseconds
        ),
        (8000, 8300, 13600)
    );
    let second = &labor.steps[1];
    assert_eq!(second.worker, "worker-1");
    assert_eq!(second.from_cell.as_deref(), Some("cell-a"));
    assert_eq!(
        (
            second.departure_milliseconds,
            second.arrival_milliseconds,
            second.loading_start_milliseconds,
            second.finish_milliseconds
        ),
        (13600, 14100, 14300, 17100)
    );
    assert_eq!(
        labor.worker_usage.as_ref().unwrap()[1].first_departure_milliseconds,
        Some(8000)
    );
    assert_eq!(
        labor.worker_usage.as_ref().unwrap()[1].last_finish_milliseconds,
        Some(17100)
    );
    raw["production"]["routing"]["availability_calendars"]["resources"]["worker-1"][1]["end_milliseconds"] =
        json!(17099);
    assert_eq!(
        build(&pattern, raw).unwrap_err().code,
        RefusalCode::NotConverged
    );
}

#[test]
fn eligible_cells_and_attendants_preserve_actual_skills_and_source_geometry() {
    let (pattern, raw) = eligible_fixture(2);
    let raw = eligible(raw, &[&["cell-a", "cell-b"], &["cell-b", "cell-c"]], 0);
    let program = SewProgram::build(&pattern, &serde_json::from_value(raw).unwrap()).unwrap();
    let route = program
        .plan
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    assert!(route.baseline_replaced && route.search_complete);
    assert_eq!(route.selected.makespan_milliseconds, 9300);
    assert!(route.lower_bound_makespan_milliseconds <= 9300);
    assert_eq!(program.stitches.len(), 100);
    let mut b = Vec::new();
    for operation in &route.selected.operations {
        match operation.cell.as_str() {
            "cell-a" => {
                assert_eq!(operation.attending_worker.as_deref(), Some("worker-0"));
                assert_eq!(
                    (
                        operation.loading_start_milliseconds,
                        operation.finish_milliseconds
                    ),
                    (0, 5300)
                );
            }
            "cell-c" => {
                assert_eq!(operation.attending_worker.as_deref(), Some("worker-1"));
                assert_eq!(
                    (
                        operation.loading_start_milliseconds,
                        operation.finish_milliseconds
                    ),
                    (0, 3300)
                );
            }
            "cell-b" => b.push((
                operation.loading_start_milliseconds,
                operation.finish_milliseconds,
            )),
            cell => panic!("unexpected specialist {cell}"),
        }
    }
    b.sort_unstable();
    assert_eq!(b, [(4200, 7000), (7000, 9300)]);
    for stitch in &program.stitches {
        let cell = stitch.cell.as_deref().unwrap();
        let seam = &program.plan.parallel_cell_plan(cell).unwrap().seams[stitch.operation];
        assert_eq!(stitch.a_mm, seam.stations.a[stitch.to_station]);
        assert_eq!(stitch.b_mm, seam.stations.b[stitch.to_station]);
    }
}

#[test]
fn selected_attendant_can_share_the_carrier_clock_or_leave_its_owner_handoff_only() {
    let (pattern, raw) = fixture(1);
    for short_owner_shift in [false, true] {
        let raw = eligible(raw.clone(), &[&["cell-b"], &["cell-a", "cell-b"]], 500);
        let mut raw = worker_owned_transfer(raw, 0, 500);
        if short_owner_shift {
            raw["production"]["routing"]["attending_labor"]["workers"][0]["available_until_milliseconds"] =
                json!(6000);
        }
        let plan = build(&pattern, raw).unwrap();
        let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
        let labor = route.attending_labor.as_ref().unwrap();
        assert_eq!(route.selected.makespan_milliseconds, 8800);
        assert_eq!(route.baseline_replaced, short_owner_shift);
        assert_eq!(
            labor.transfer.as_ref().unwrap().attending_worker.as_deref(),
            Some("worker-0")
        );
        let transfer = &labor.selected.transfers.as_ref().unwrap()[0];
        assert_eq!(transfer.worker, "worker-0");
        assert_eq!(
            (
                transfer.departure_milliseconds,
                transfer.arrival_milliseconds,
                transfer.start_milliseconds,
                transfer.finish_milliseconds
            ),
            (0, 0, 5300, 6000)
        );
        let owner = &labor.selected.worker_usage.as_ref().unwrap()[0];
        assert_eq!(
            (
                owner.handoffs,
                owner.transfer_milliseconds,
                owner.travel_milliseconds,
                owner.waiting_milliseconds
            ),
            (1, 700, 0, 5300)
        );
        assert_eq!(owner.first_departure_milliseconds, Some(0));
        let b = &labor.selected.steps[1];
        if short_owner_shift {
            assert_eq!((owner.operations, owner.attended_milliseconds), (0, 0));
            assert_eq!(owner.last_finish_milliseconds, Some(6000));
            assert_eq!(b.worker, "worker-1");
            assert_eq!(
                (
                    b.departure_milliseconds,
                    b.arrival_milliseconds,
                    b.loading_start_milliseconds
                ),
                (5300, 5800, 6000)
            );
        } else {
            assert_eq!((owner.operations, owner.attended_milliseconds), (1, 2800));
            assert_eq!(owner.last_finish_milliseconds, Some(8800));
            assert_eq!(b.worker, "worker-0");
            assert_eq!(b.from_cell.as_deref(), Some("cell-b"));
            assert_eq!(
                (
                    b.departure_milliseconds,
                    b.arrival_milliseconds,
                    b.loading_start_milliseconds
                ),
                (6000, 6000, 6000)
            );
        }
    }
}

#[test]
fn worker_and_carrier_choices_share_finite_staging_capacity() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["transfer"]["transfers"][0]["milliseconds"] = json!(10000);
    let raw = eligible(
        additional_carrier(raw, 10000),
        &[&["cell-a", "cell-b"], &["cell-a", "cell-b"]],
        0,
    );
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
        assert_eq!(
            route
                .attending_labor
                .as_ref()
                .unwrap()
                .selected
                .attended_milliseconds,
            14700
        );
        let slots = &route.bounded_staging.as_ref().unwrap().selected;
        for cell in ["cell-a", "cell-b"] {
            assert_eq!(slots.peak_reserved_slots[cell], capacity);
            assert_eq!(slots.final_reserved_slots[cell], 0);
        }
        let mut handoffs: Vec<_> = route.selected.transfers.iter().collect();
        handoffs.sort_unstable_by_key(|row| row.start_milliseconds);
        assert_eq!(
            handoffs[0].finish_milliseconds <= handoffs[1].start_milliseconds,
            capacity == 1
        );
        assert!(
            route
                .selected
                .operations
                .iter()
                .all(|operation| operation.attending_worker.is_some())
        );
    }
}

#[test]
fn extra_attendants_cannot_release_retained_fixtures_before_their_qualified_operation() {
    let (pattern, raw) = retained_fixtures(1, Some(2));
    let raw = bounded(
        eligible(
            additional_carrier(raw, 100),
            &[&["cell-a", "cell-b"], &["cell-a", "cell-b"]],
            0,
        ),
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
    assert_eq!(route.baseline.makespan_milliseconds, 41100);
    assert_eq!(route.selected.makespan_milliseconds, 38700);
    assert_eq!(route.selected.transfers.len(), 4);
    assert_eq!(route.selected.transfer_milliseconds, 400);
    assert_eq!(program.stitches.len(), 200);
    let fixtures = &route.workholding.as_ref().unwrap().selected;
    let mut held: Vec<_> = fixtures
        .holds
        .iter()
        .filter(|row| row.cell == "cell-a")
        .map(|row| (row.acquired_at_milliseconds, row.released_at_milliseconds))
        .collect();
    held.sort_unstable();
    assert_eq!(held, [(0, 17600), (17600, 38700)]);
    assert_eq!(fixtures.shared_peak_occupancy["common-jigs"], 2);
    assert_eq!(fixtures.shared_final_occupancy["common-jigs"], 0);
    for cell in ["cell-a", "cell-b"] {
        assert_eq!(fixtures.local_peak_occupancy[cell]["jig"], 1);
        assert_eq!(fixtures.local_final_occupancy[cell]["jig"], 0);
        assert_eq!(
            route
                .bounded_staging
                .as_ref()
                .unwrap()
                .selected
                .final_reserved_slots[cell],
            0
        );
    }
    assert_eq!(
        route.attending_labor.as_ref().unwrap().selected.steps.len(),
        6
    );
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
}

#[test]
fn worker_choice_budget_retains_the_complete_first_worker_seed() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["max_search_nodes"] = json!(4);
    let plan = build(
        &pattern,
        eligible(raw, &[&["cell-a", "cell-b"], &["cell-a", "cell-b"]], 0),
    )
    .unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert!(route.budget_exhausted && !route.search_complete && !route.optimality_proven);
    assert_eq!((route.search_nodes, route.work_used), (4, 12));
    assert_eq!(route.selected.makespan_milliseconds, 16100);
    assert_eq!(route.selected.operations.len(), 4);
    assert!(
        route
            .selected
            .operations
            .iter()
            .all(|row| row.attending_worker.as_deref() == Some("worker-0"))
    );
    assert_eq!(
        serde_json::to_value(&route.baseline).unwrap(),
        serde_json::to_value(&route.selected).unwrap()
    );
}

#[test]
fn worker_domains_require_complete_coverage_unique_identity_and_their_own_source() {
    let (pattern, raw) = fixture(1);
    let valid = eligible(
        raw.clone(),
        &[&["cell-a", "cell-b"], &["cell-a", "cell-b"]],
        500,
    );
    assert!(build(&pattern, valid.clone()).is_ok());
    let prefix = "/production/routing/attending_labor";
    for (field, value) in [
        ("model", json!("inferred_interchangeable_workers/1")),
        ("source/reference", json!("")),
        ("method", json!("")),
        ("allow_estimates", json!(false)),
        ("cells/cell-a", json!("0".repeat(64))),
        ("workers/0/id", json!("worker-1")),
        ("workers/0/id", json!("cell-a")),
        ("workers/0/id", json!("handoff-worker")),
        ("workers/0/cells", json!([])),
        ("workers/0/cells", json!(["cell-a", "cell-a"])),
        ("workers/0/cells", json!(["unknown-cell"])),
        (
            "workers/0/initial_arrival_milliseconds",
            json!({"cell-a":0}),
        ),
        ("workers/0/transfers", json!([])),
        ("workers/0/transfers/0/milliseconds", json!(86400001)),
        ("workers/0/available_at_milliseconds", json!(86400001)),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(&format!("{prefix}/{field}")).unwrap() = value;
        assert_eq!(
            build(&pattern, invalid).unwrap_err().code,
            RefusalCode::InvalidDocument,
            "{field}"
        );
    }
    for count in [0, 9] {
        let mut invalid = valid.clone();
        invalid["production"]["routing"]["attending_labor"]["workers"] = json!(
            (0..count)
                .map(|index| {
                    let mut worker =
                        valid["production"]["routing"]["attending_labor"]["workers"][0].clone();
                    worker["id"] = json!(format!("worker-{index}"));
                    worker
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(
            build(&pattern, invalid).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
    }
    assert_eq!(
        build(&pattern, eligible(raw, &[&["cell-a"], &["cell-a"]], 0))
            .unwrap_err()
            .code,
        RefusalCode::InvalidDocument
    );
    let mut fixed = valid;
    fixed["production"]["routing"]["attending_labor"]["model"] =
        json!("exclusive_full_operation_attendance/1");
    assert_eq!(
        build(&pattern, fixed).unwrap_err().code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn fixed_attendance_keeps_legacy_versions_and_omits_worker_choice_receipts() {
    let (pattern, raw) = fixture(2);
    for (options, schema) in [
        (raw.clone(), "ftx-sewing-routing/1"),
        (
            attending_labor(raw, &[&["cell-a", "cell-b"]], 500),
            "ftx-sewing-routing/4",
        ),
    ] {
        let plan = build(&pattern, options).unwrap();
        let route =
            serde_json::to_value(plan.production.as_ref().unwrap().routing.as_ref().unwrap())
                .unwrap();
        assert_eq!(route["schema"], schema);
        for name in ["baseline", "selected"] {
            assert!(
                route[name]["operations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|row| row.get("attending_worker").is_none())
            );
            if let Some(labor) = route.get("attending_labor") {
                assert_eq!(labor["schema"], "ftx-sewing-attending-labor/1");
                assert!(labor[name].get("worker_usage").is_none());
            }
        }
    }
}
