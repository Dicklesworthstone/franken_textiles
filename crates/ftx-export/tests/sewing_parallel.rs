//! Synthetic independent cells qualify scheduling arithmetic, never physical factory throughput.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

fn fixture(copies: usize) -> (Pattern, Value) {
    let pattern = Pattern::new(
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
        ))
        .unwrap(),
    );
    let mut options: Value = serde_json::from_str(include_str!(
        "../../../examples/sewing-handling-options.json"
    ))
    .unwrap();
    options["production"]["production_copies"] = json!(copies);
    let production = options["production"].clone();
    let cells: Vec<_> = ["cell-a", "cell-b"]
        .into_iter()
        .map(|id| {
            json!({
                "id":id, "available_at_milliseconds":0,
                "ordering":production["ordering"],
                "handling":production["handling"],
                "workholding":production["workholding"]
            })
        })
        .collect();
    options["production"]["parallel"] = json!({
        "model":"independent_whole_garment_cells/1", "objective":"minimize_makespan",
        "time_basis":"ceil_cell_feed_milliseconds/1",
        "independence":"independent_machines_fixtures_and_handling_labor/1",
        "max_search_nodes":10000,
        "machine_sha256":production["handling"]["machine_sha256"],
        "qualifications":production["handling"]["qualifications"],
        "cells":cells,
        "source":{"reference":"test:independent-sewing-cells","accessed_on":"2026-10-27",
            "note":"Invented software cells, each with separate machine, fixture inventory and worker"},
        "method":"Synthetic qualification of identical configuration on explicitly independent physical instances",
        "conditions":{"resources":"No machine, fixture unit or handling labor is shared between cells"},
        "evidence":"estimate","allow_estimates":true
    });
    (pattern, options)
}

fn build(pattern: &Pattern, options: Value) -> Result<SewPlan, Refusal> {
    SewPlan::build(pattern, &serde_json::from_value(options).unwrap())
}

// Independent exhaustive enumeration over each garment's join/reinforce state. This never uses
// optimizer transitions, lower bounds, scoped-ID expansion or its count-allocation algorithm.
fn serial_cost(copies: usize, capacity: usize, transfer: u64) -> u64 {
    if copies == 0 {
        return 0;
    }
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
    copies as u64 * 8000 + best
}

#[test]
fn independent_cells_reduce_elapsed_span_without_losing_aggregate_work() {
    let (pattern, options) = fixture(3);
    let plan = build(&pattern, options.clone()).unwrap();
    assert_eq!(plan.schema, "ftx-sewing-plan/16");
    assert!(plan.ordering_report().is_none());
    assert!(plan.modeled_production_seconds().is_none());
    assert_eq!(plan.parallel_makespan_seconds(), Some(25.5));
    assert_eq!(plan.parallel_optimality_proven(), Some(true));
    let production = plan.production.as_ref().unwrap();
    assert_eq!(production.schema, "ftx-sewing-production/4");
    let parallel = production.parallel.as_ref().unwrap();
    assert_eq!(parallel.baseline.makespan_milliseconds, 41100);
    assert_eq!(parallel.selected.makespan_milliseconds, 25500);
    assert_eq!(parallel.lower_bound_makespan_milliseconds, 25500);
    assert_eq!(parallel.selected.feed_seconds, 24.0);
    assert_eq!(parallel.selected.scheduled_feed_milliseconds, 24000);
    assert_eq!(parallel.selected.setup_milliseconds, 5500);
    assert_eq!(parallel.selected.handling_milliseconds, 5900);
    assert!(parallel.search_complete);
    assert!(!parallel.budget_exhausted);
    assert!(!parallel.hardware_execution_authorized);
    assert_eq!(parallel.intercell_transfers, 0);
    assert_eq!(production.stitch_intervals, 240);
    assert_eq!(
        serde_json::to_value(SewPlan::build(&pattern, &plan.options).unwrap()).unwrap(),
        serde_json::to_value(&plan).unwrap()
    );
    let mut legacy = options;
    legacy["production"]
        .as_object_mut()
        .unwrap()
        .remove("parallel");
    let legacy = build(&pattern, legacy).unwrap();
    assert_eq!(
        serde_json::to_value(&legacy.production.as_ref().unwrap().ordering).unwrap(),
        serde_json::to_value(&production.ordering).unwrap()
    );
    assert!(
        serde_json::to_value(legacy).unwrap()["production"]
            .get("parallel")
            .is_none()
    );
}

#[test]
fn complete_published_example_replays_its_independent_inventory_and_stitch_streams() {
    let (pattern, _) = fixture(3);
    let options: SewOptions = serde_json::from_str(include_str!(
        "../../../examples/sewing-parallel-options.json"
    ))
    .unwrap();
    let program = SewProgram::build(&pattern, &options).unwrap();
    assert_eq!(program.plan.parallel_makespan_seconds(), Some(25.5));
    assert_eq!(program.plan.parallel_optimality_proven(), Some(true));
    assert_eq!(program.stitches.len(), 240);
    let selected = &program
        .plan
        .production
        .as_ref()
        .unwrap()
        .parallel
        .as_ref()
        .unwrap()
        .selected;
    let inventories: BTreeSet<_> = selected
        .cells
        .iter()
        .filter_map(|cell| cell.schedule.as_ref())
        .flat_map(|schedule| {
            schedule
                .workholding
                .as_ref()
                .unwrap()
                .resources
                .iter()
                .map(|resource| resource.id.as_str())
        })
        .collect();
    assert_eq!(inventories, BTreeSet::from(["jigs-a", "jigs-b"]));
}

#[test]
fn allocation_matches_independent_enumeration_with_availability_and_separate_fixture_capacity() {
    for (capacity_a, capacity_b, transfer_a, transfer_b, available_b) in [
        (1, 2, 2000, 5000, 0),
        (2, 1, 5000, 0, 4000),
        (1, 1, 5000, 5000, 50000),
        (3, 2, 0, 2000, 11000),
    ] {
        let (pattern, mut options) = fixture(3);
        for (cell, capacity, transfer) in [(0, capacity_a, transfer_a), (1, capacity_b, transfer_b)]
        {
            let input = &mut options["production"]["parallel"]["cells"][cell];
            input["workholding"]["resources"][0]["capacity"] = json!(capacity);
            for row in input["handling"]["transitions"].as_array_mut().unwrap() {
                if row["relation"] == "different_copy" {
                    row["milliseconds"] = json!(transfer);
                }
            }
        }
        options["production"]["parallel"]["cells"][1]["available_at_milliseconds"] =
            json!(available_b);
        let plan = build(&pattern, options).unwrap();
        let parallel = plan.production.unwrap().parallel.unwrap();
        let exact = (0..=3)
            .map(|first| {
                let second = 3 - first;
                let a = serial_cost(first, capacity_a, transfer_a);
                let b = if second == 0 {
                    0
                } else {
                    serial_cost(second, capacity_b, transfer_b) + available_b
                };
                a.max(b)
            })
            .min()
            .unwrap();
        assert_eq!(parallel.selected.makespan_milliseconds, exact);
        assert_eq!(parallel.lower_bound_makespan_milliseconds, exact);
        assert!(parallel.optimality_proven);
        for cell in &parallel.selected.cells {
            if let Some(schedule) = &cell.schedule {
                let resource = schedule.workholding.as_ref().unwrap();
                let capacity = resource.resources[0].capacity;
                assert!(
                    resource
                        .selected
                        .peak_occupancy
                        .values()
                        .all(|&used| used <= capacity)
                );
            } else {
                assert!(cell.copies.is_empty());
                assert_eq!(cell.finish_milliseconds, None);
                assert_eq!(cell.scheduled_feed_milliseconds, 0);
            }
        }
    }
}

#[test]
fn cell_stitch_streams_replay_every_physical_copy_with_local_history_mapping() {
    let (pattern, options) = fixture(3);
    let options: SewOptions = serde_json::from_value(options).unwrap();
    let program = SewProgram::build(&pattern, &options).unwrap();
    assert_eq!(program.schema, "ftx-sewing-program/5");
    assert_eq!(program.stitches.len(), 240);
    let parallel = program
        .plan
        .production
        .as_ref()
        .unwrap()
        .parallel
        .as_ref()
        .unwrap();
    let mut physical_copies = BTreeSet::new();
    let mut counts = BTreeMap::new();
    for cell in &parallel.selected.cells {
        for &copy in &cell.copies {
            assert!(physical_copies.insert(copy));
        }
        if let Some(schedule) = &cell.schedule {
            let ids: BTreeSet<_> = schedule
                .operations
                .iter()
                .map(|operation| operation.id.as_str())
                .collect();
            assert_eq!(schedule.production_copies, cell.copies.len());
            for operation in &schedule.operations {
                assert!(operation.copy < cell.copies.len());
                assert!(operation.after.iter().all(|id| ids.contains(id.as_str())));
                let global = cell.copies[operation.copy];
                counts.insert(
                    (cell.cell.clone(), global, operation.seam_index),
                    operation.stitch_intervals,
                );
            }
            let holding = schedule.workholding.as_ref().unwrap();
            assert!(
                holding
                    .holds
                    .iter()
                    .all(|hold| ids.contains(hold.acquire.as_str())
                        && ids.contains(hold.release.as_str())
                        && hold.copy < cell.copies.len())
            );
            assert!(
                schedule
                    .handling
                    .as_ref()
                    .unwrap()
                    .selected
                    .steps
                    .iter()
                    .all(|step| step
                        .from
                        .iter()
                        .chain(&step.to)
                        .all(|id| ids.contains(id.as_str())))
            );
        }
    }
    assert_eq!(physical_copies, BTreeSet::from([0, 1, 2]));
    for stitch in &program.stitches {
        let key = (
            stitch.cell.clone().unwrap(),
            stitch.copy.unwrap(),
            stitch.operation,
        );
        *counts.get_mut(&key).unwrap() -= 1;
        assert_eq!(
            stitch.a_mm,
            program.plan.seams[stitch.operation].stations.a[stitch.to_station]
        );
        assert_eq!(
            stitch.b_mm,
            program.plan.seams[stitch.operation].stations.b[stitch.to_station]
        );
    }
    assert!(counts.values().all(|&count| count == 0));
}

#[test]
fn one_global_work_budget_preserves_complete_incumbents_and_a_sound_lower_bound() {
    let (pattern, mut options) = fixture(4);
    options["production"]["parallel"]["max_search_nodes"] = json!(68);
    let typed: SewOptions = serde_json::from_value(options.clone()).unwrap();
    assert_eq!(typed.scheduling_work_bound().unwrap(), 2068);
    let plan = build(&pattern, options).unwrap();
    let parallel = plan.production.unwrap().parallel.unwrap();
    assert!(parallel.budget_exhausted);
    assert!(!parallel.optimality_proven);
    assert!(!parallel.search_complete);
    assert!(parallel.search_nodes <= 68);
    let exact = (0..=4)
        .map(|a| serial_cost(a, 2, 5000).max(serial_cost(4 - a, 2, 5000)))
        .min()
        .unwrap();
    assert!(parallel.lower_bound_makespan_milliseconds <= exact);
    assert!(parallel.selected.makespan_milliseconds >= exact);
    assert_eq!(
        parallel
            .selected
            .cells
            .iter()
            .map(|cell| cell.copies.len())
            .sum::<usize>(),
        4
    );
    assert!(
        parallel
            .selected
            .cells
            .iter()
            .filter_map(|cell| cell.schedule.as_ref())
            .all(|schedule| schedule.ordering.selected_order.len() == schedule.operations.len())
    );
}

#[test]
fn explicit_millisecond_feed_ceiling_is_once_per_used_cell_and_retains_raw_feed() {
    let (pattern, mut options) = fixture(3);
    for pass in options["passes"].as_array_mut().unwrap() {
        pass["stitches_per_minute"] = json!(613.0);
    }
    let plan = build(&pattern, options).unwrap();
    let parallel = plan.production.unwrap().parallel.unwrap();
    assert_eq!(parallel.time_basis, "ceil_cell_feed_milliseconds/1");
    for cell in &parallel.selected.cells {
        if let Some(schedule) = &cell.schedule {
            let actual = cell.copies.len() as f64 * 80.0 * 60.0 / 613.0;
            assert!((schedule.feed_seconds - actual).abs() < 1e-12);
            assert_eq!(
                cell.scheduled_feed_milliseconds,
                (schedule.feed_seconds * 1000.0).ceil() as u64
            );
            assert!((0.0..0.001).contains(&cell.feed_padding_seconds));
        }
    }
}

#[test]
fn source_independence_identity_missing_time_and_bounds_fail_closed_even_for_unused_cells() {
    let (pattern, original) = fixture(3);
    let mut cases = Vec::new();
    let mut wrong = original.clone();
    wrong["production"]["parallel"]["allow_estimates"] = json!(false);
    cases.push(wrong);
    let mut wrong = original.clone();
    wrong["production"]["parallel"]["independence"] = json!("one-shared-worker");
    cases.push(wrong);
    let mut wrong = original.clone();
    wrong["production"]["parallel"]["cells"][1]["id"] = json!("cell-a");
    cases.push(wrong);
    let mut wrong = original.clone();
    wrong["production"]["parallel"]["machine_sha256"] = json!("0".repeat(64));
    cases.push(wrong);
    let mut wrong = original.clone();
    wrong["production"]["parallel"]["qualifications"]["join"][0]["sha256"] = json!("0".repeat(64));
    cases.push(wrong);
    let mut wrong = original.clone();
    wrong["production"]["parallel"]["time_basis"] = json!("silent-rounded-feed");
    cases.push(wrong);
    let mut wrong = original.clone();
    wrong["production"]["parallel"]["max_search_nodes"] = json!(200000);
    cases.push(wrong);
    let mut wrong = original.clone();
    wrong["production"]["parallel"]["max_search_nodes"] = json!(1);
    cases.push(wrong);
    for wrong in cases {
        assert!(build(&pattern, wrong).is_err());
    }
    let mut wrong = original;
    wrong["production"]["parallel"]["cells"][1]["available_at_milliseconds"] = json!(86400000);
    wrong["production"]["parallel"]["cells"][1]["handling"]["transitions"]
        .as_array_mut()
        .unwrap()
        .retain(|row| {
            !(row["from"] == "join"
                && row["to"] == "reinforce"
                && row["relation"] == "different_copy")
        });
    let refusal = build(&pattern, wrong).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::NotConverged);
}
