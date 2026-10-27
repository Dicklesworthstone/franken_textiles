//! Synthetic independent cells qualify scheduling arithmetic, never physical factory throughput.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_export::sewing::{
    SewOptions, SewPlan, SewingMachine, SewingQualification, production::workholding,
    program::SewProgram,
};
use ftx_pattern::Pattern;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::materials::Catalog;
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

fn refresh_cell_locks(cell: &mut Value) {
    let machine: SewingMachine =
        serde_json::from_value(cell["configuration"]["machine"].clone()).unwrap();
    let machine_hash = workholding::machine_sha256(&machine).unwrap();
    let qualifications: Vec<SewingQualification> =
        serde_json::from_value(cell["configuration"]["qualifications"].clone()).unwrap();
    let refs: BTreeMap<_, _> = ["join", "reinforce"]
        .into_iter()
        .enumerate()
        .map(|(index, operation)| {
            let q = &qualifications[index.min(qualifications.len() - 1)];
            (
                operation,
                json!([{"id":q.id,"sha256":workholding::qualification_sha256(q).unwrap()}]),
            )
        })
        .collect();
    cell["configuration"]["machine_sha256"] = json!(machine_hash);
    cell["configuration"]["qualification_refs"] = json!(refs);
    cell["handling"]["machine_sha256"] = json!(machine_hash);
    cell["handling"]["qualifications"] = json!(refs);
    if !cell["workholding"].is_null() {
        cell["workholding"]["machine_sha256"] = json!(machine_hash);
        cell["workholding"]["qualifications"] = json!(refs);
    }
}

fn heterogeneous() -> (Pattern, Value) {
    let (pattern, mut options) = fixture(3);
    let machine = options["machine"].clone();
    let qualifications = options["qualifications"].clone();
    options["production"]["parallel"]["model"] = json!("independent_whole_garment_cells/2");
    options["production"]["parallel"]["method"] = json!(
        "Synthetic independent qualification of each exact machine configuration and its declared process recipes"
    );
    let cells = options["production"]["parallel"]["cells"]
        .as_array_mut()
        .unwrap();
    for cell in cells.iter_mut() {
        cell["configuration"] =
            json!({"machine":machine,"qualifications":qualifications,"recipes":{}});
    }
    let fast = &mut cells[1];
    fast["configuration"]["machine"]["id"] = json!("synthetic-higher-rate-configuration");
    fast["configuration"]["machine"]["max_stitches_per_minute"] = json!(1200.0);
    let mut q = qualifications[0].clone();
    q["id"] = json!("synthetic-fast-cell-qualification");
    q["machine"] = fast["configuration"]["machine"]["id"].clone();
    q["needle"] = json!("fast-needle");
    q["thread"] = json!("fast-thread");
    q["stitches_per_minute"] = json!([1.0, 1200.0]);
    fast["configuration"]["qualifications"] = json!([q]);
    for operation in ["join", "reinforce"] {
        fast["configuration"]["recipes"][operation] = json!({"id":format!("fast-{operation}"),
            "needle":"fast-needle","thread":"fast-thread","max_stitch_mm":5.0,"stitches_per_minute":900.0});
    }
    fast["ordering"]["initial_setup"] = json!({"needle":"fast-needle","thread":"idle"});
    fast["ordering"]["setup_times"]["machine"] = fast["configuration"]["machine"]["id"].clone();
    fast["ordering"]["setup_times"]["transitions"] = json!([{"from":{"needle":"fast-needle","thread":"idle"},
        "to":{"needle":"fast-needle","thread":"fast-thread"},"milliseconds":500}]);
    for row in fast["handling"]["transitions"].as_array_mut().unwrap() {
        if row["relation"] == "different_copy" {
            row["milliseconds"] = json!(1000);
        }
    }
    for cell in cells {
        refresh_cell_locks(cell);
    }
    (pattern, options)
}

fn thread_inventory(options: &mut Value) -> Selection {
    let yarn = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: "coats/epic-2994120-2026".into(),
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection();
    for (index, cell) in options["production"]["parallel"]["cells"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        for q in cell["configuration"]["qualifications"]
            .as_array_mut()
            .unwrap()
        {
            q["thread_consumption"] = json!({"channels_complete":true,"evidence":"estimate","allow_estimates":true,
                "source":{"reference":format!("test:cell-{index}-consumption"),"accessed_on":"2026-10-27",
                    "note":"Synthetic complete effective channel model, not physical thread-use evidence"},
                "method":"Independently declared per-traversal and per-operation allowances",
                "channels":[{"id":"all-required-thread","material":yarn,
                    "metres_per_traversal":if index==0 {0.01}else{0.015},
                    "start_allowance_m":if index==0 {0.1}else{0.05},
                    "end_allowance_m":if index==0 {0.2}else{0.05}}]});
        }
        refresh_cell_locks(cell);
    }
    options["consumables"] = json!({"purchasing":{
        "quote_book":{"schema":"ftx-textile-quotes/1","quotes":[{
            "offer_id":"synthetic-yarn-offer","material_id":yarn.id,"material_sha256":yarn.expected_sha256,
            "supplier":"synthetic-supplier","sku":"shared-yarn","price":10.0,"quantity":2.0,"unit":"linear_meter",
            "minimum_quantity":2.0,"order_increment":2.0,"price_breaks":[{"minimum_quantity":4.0,"price":7.0}],
            "currency":"USD","region":"test","quality":"new","terms":"synthetic merchandise only",
            "observed_on":"2026-10-27","available":true,"available_quantity":4.0,"maximum_continuous_quantity":2.0,
            "source":"test:shared-yarn-offer","source_sha256":"ab".repeat(32)
        }]},
        "market":{"currency":"USD","region":"test","quality":"new","terms":"synthetic merchandise only",
            "as_of":"2026-10-27","max_age_days":2,"require_observed_quantity":true,"require_observed_continuous_length":true}
    }});
    yarn
}

#[test]
fn heterogeneous_cells_use_actual_machine_recipe_geometry_feed_and_global_copy_stitches() {
    let (pattern, options) = heterogeneous();
    let options: SewOptions = serde_json::from_value(options).unwrap();
    assert_eq!(options.source_plan_count(), 3);
    let program = SewProgram::build(&pattern, &options).unwrap();
    assert_eq!(program.plan.retained_source_station_count(), 206);
    assert_eq!(program.schema, "ftx-sewing-program/6");
    assert_eq!(program.plan.schema, "ftx-sewing-plan/17");
    assert_eq!(program.plan.parallel_makespan_seconds(), Some(9.9));
    assert_eq!(program.plan.parallel_optimality_proven(), Some(true));
    assert_eq!(program.plan.total_stitch_intervals(), 160);
    assert_eq!(program.stitches.len(), 160);
    assert!((program.plan.total_feed_seconds() - 40.0 / 3.0).abs() < 1e-12);
    let production = program.plan.production.as_ref().unwrap();
    assert_eq!(production.schema, "ftx-sewing-production/5");
    let parallel = production.parallel.as_ref().unwrap();
    assert_eq!(parallel.schema, "ftx-sewing-parallel/2");
    assert_eq!(parallel.cell_plans.len(), 2);
    assert_eq!(parallel.selected.cells[0].copies, vec![0]);
    assert_eq!(parallel.selected.cells[1].copies, vec![1, 2]);
    assert_eq!(parallel.selected.cells[1].finish_milliseconds, Some(7334));
    let mut counts = BTreeMap::new();
    let mut feed = 0.0;
    for stitch in &program.stitches {
        let cell = stitch.cell.as_ref().unwrap();
        let source = program.plan.parallel_cell_plan(cell).unwrap();
        let seam = &source.seams[stitch.operation];
        assert_eq!(stitch.a_mm, seam.stations.a[stitch.to_station]);
        assert_eq!(stitch.b_mm, seam.stations.b[stitch.to_station]);
        assert_eq!(stitch.feed_seconds, 60.0 / seam.pass.stitches_per_minute);
        *counts
            .entry((cell.clone(), stitch.copy.unwrap()))
            .or_insert(0usize) += 1;
        feed += stitch.feed_seconds;
    }
    assert_eq!(
        counts,
        BTreeMap::from([
            (("cell-a".into(), 0), 80),
            (("cell-b".into(), 1), 40),
            (("cell-b".into(), 2), 40)
        ])
    );
    assert!((feed - program.plan.total_feed_seconds()).abs() < 1e-12);
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
    assert!(
        parallel
            .cell_plans
            .iter()
            .all(|source| source.plan.production.is_none()
                && source.plan.options.production.is_none()
                && source.plan.consumables.is_none())
    );
}

#[test]
fn explicit_cell_preview_uses_actual_spacing_feed_and_full_job_source_admission() {
    let (pattern, raw) = heterogeneous();
    let options: SewOptions = serde_json::from_value(raw.clone()).unwrap();
    let preview = ftx_export::sewing::preview::svg_cell;
    let a = preview(&pattern, &options, "cell-a", "join").unwrap();
    let b = preview(&pattern, &options, "cell-b", "join").unwrap();
    // Both paired source-side frames are drawn: 40+1 versus 20+1 stations on each.
    assert_eq!(a.matches("<circle class=\"needle\"").count(), 82);
    assert_eq!(b.matches("<circle class=\"needle\"").count(), 42);
    assert!(a.contains("data-station=\"1\" cx=\"20\" cy=\"2.5\""));
    assert!(b.contains("data-station=\"1\" cx=\"20\" cy=\"5\""));
    assert!(b.contains("20 intervals | stack 0.8 mm | continuous feed 1.333333 s"));
    assert!(b.contains("feed rate 900 stitches/minute"));
    assert!(b.contains("ftx-sewing-preview/2"));
    assert!(b.contains("&quot;cell&quot;:&quot;cell-b&quot;"));
    assert!(b.contains("&quot;production_copy_ids&quot;:[1,2]"));
    assert!(b.contains("synthetic-higher-rate-configuration"));
    let configuration = &raw["production"]["parallel"]["cells"][1]["configuration"];
    assert!(b.contains(configuration["machine_sha256"].as_str().unwrap()));
    assert!(
        b.contains(
            configuration["qualification_refs"]["join"][0]["sha256"]
                .as_str()
                .unwrap()
        )
    );
    assert_eq!(b, preview(&pattern, &options, "cell-b", "join").unwrap());
    assert!(ftx_export::sewing::preview::svg(&pattern, &options, "join").is_err());
    assert!(preview(&pattern, &options, "unknown", "join").is_err());
    assert!(preview(&pattern, &options, "cell-b", "unknown").is_err());

    let mut invalid = raw.clone();
    invalid["production"]["parallel"]["cells"][0]["handling"]["machine_sha256"] =
        json!("00".repeat(32));
    assert!(
        preview(
            &pattern,
            &serde_json::from_value(invalid).unwrap(),
            "cell-b",
            "join"
        )
        .is_err()
    );

    let mut idle = raw.clone();
    idle["production"]["parallel"]["cells"][1]["available_at_milliseconds"] = json!(60000);
    let idle = preview(
        &pattern,
        &serde_json::from_value(idle).unwrap(),
        "cell-b",
        "join",
    )
    .unwrap();
    assert!(idle.contains("&quot;production_copy_ids&quot;:[]"));
    assert!(idle.contains("selected garment copies: 0"));
    assert_eq!(idle.matches("<circle class=\"needle\"").count(), 42);

    let mut renamed = raw;
    let label = "cell<&\"";
    renamed["production"]["parallel"]["cells"][1]["id"] = json!(label);
    let escaped = preview(
        &pattern,
        &serde_json::from_value(renamed.clone()).unwrap(),
        label,
        "join",
    )
    .unwrap();
    assert!(escaped.contains("Cell: cell&lt;&amp;&quot;"));
    renamed["production"]["parallel"]["cells"][1]["id"] = json!("cell\u{1}");
    assert!(
        preview(
            &pattern,
            &serde_json::from_value(renamed).unwrap(),
            "cell\u{1}",
            "join"
        )
        .is_err()
    );
}

#[test]
fn selected_cells_combine_actual_yarn_demands_before_one_complete_priced_order() {
    let (pattern, mut options) = heterogeneous();
    let yarn = thread_inventory(&mut options);
    let plan = build(&pattern, options).unwrap();
    let report = plan.consumables.as_ref().unwrap();
    assert_eq!(report.schema, "ftx-sewing-consumables/3");
    assert!(report.complete_consumption_coverage && report.complete_mass_coverage);
    assert_eq!(report.operation_channels.len(), 4);
    assert_eq!(report.material_requirements.len(), 1);
    assert!((report.total_required_length_m.unwrap() - 3.0).abs() < 1e-12);
    assert!((report.total_required_mass_kg.unwrap() - 72e-6).abs() < 1e-15);
    let requirement = &report.material_requirements[0];
    assert_eq!(requirement.material_sha256, yarn.expected_sha256.unwrap());
    assert!((requirement.required_continuous_length_m - 0.7).abs() < 1e-12);
    let purchasing = report.purchasing.as_ref().unwrap();
    assert_eq!(purchasing.material_orders.len(), 1);
    assert_eq!(
        purchasing.least_observed_independent_order_total,
        Some(14.0)
    );
    assert_eq!(
        purchasing.material_orders[0]
            .least_observed_order
            .as_ref()
            .unwrap()
            .order_quantity,
        4.0
    );
    let by_cell: BTreeMap<_, f64> =
        report
            .operation_channels
            .iter()
            .fold(BTreeMap::new(), |mut totals, row| {
                *totals.entry(row.cell.as_deref().unwrap()).or_default() += row.required_length_m;
                totals
            });
    assert!((by_cell["cell-a"] - 1.4).abs() < 1e-12);
    assert!((by_cell["cell-b"] - 1.6).abs() < 1e-12);
    assert!(purchasing.quote_observations_evaluated <= report.purchasing_work_bound);
}

#[test]
fn unused_cell_thread_evidence_cannot_enter_the_selected_order_and_missing_used_coverage_stays_partial()
 {
    let (pattern, mut options) = heterogeneous();
    thread_inventory(&mut options);
    for recipe in options["production"]["parallel"]["cells"][1]["configuration"]["recipes"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        recipe["stitches_per_minute"] = json!(1200.0);
    }
    let plan = build(&pattern, options.clone()).unwrap();
    assert_eq!(plan.parallel_makespan_seconds(), Some(9.1));
    assert_eq!(plan.total_stitch_intervals(), 120);
    assert_eq!(plan.total_feed_seconds(), 6.0);
    let consumed = plan.consumables.unwrap();
    assert!(
        consumed
            .operation_channels
            .iter()
            .all(|row| row.cell.as_deref() == Some("cell-b"))
    );
    assert!((consumed.total_required_length_m.unwrap() - 2.4).abs() < 1e-12);
    options["production"]["parallel"]["cells"][1]["configuration"]["qualifications"][0]["thread_consumption"]
        ["channels_complete"] = json!(false);
    refresh_cell_locks(&mut options["production"]["parallel"]["cells"][1]);
    let partial = build(&pattern, options).unwrap().consumables.unwrap();
    assert!(!partial.complete_consumption_coverage);
    assert!(partial.total_required_length_m.is_none() && partial.material_requirements.is_empty());
    assert!(
        partial
            .purchasing
            .unwrap()
            .least_observed_independent_order_total
            .is_none()
    );
    assert!(
        partial
            .missing_operations
            .iter()
            .all(|row| row.cell.as_deref() == Some("cell-b"))
    );
}

#[test]
fn heterogeneous_configuration_and_every_admitted_machine_qualification_are_revalidated() {
    let (pattern, original) = heterogeneous();
    for mutation in 0..5 {
        let mut changed = original.clone();
        let fast = &mut changed["production"]["parallel"]["cells"][1];
        match mutation {
            0 => fast["configuration"]["machine"]["max_stack_mm"] = json!(4.0),
            1 => {
                fast["configuration"]["qualifications"][0]["source"]["note"] =
                    json!("Revised source must invalidate locks")
            }
            2 => {
                fast["configuration"]["recipes"]["join"]["stitches_per_minute"] = json!(1300.0);
            }
            3 => {
                fast["configuration"]["recipes"]["unknown-operation"] =
                    fast["configuration"]["recipes"]["join"].clone();
            }
            _ => {
                fast["configuration"]["machine"]["max_stack_mm"] = json!(0.05);
                refresh_cell_locks(fast);
            }
        }
        assert!(build(&pattern, changed).is_err(), "mutation {mutation}");
    }
    let mut wrong_mode = original;
    wrong_mode["production"]["parallel"]["model"] = json!("independent_whole_garment_cells/1");
    assert!(build(&pattern, wrong_mode).is_err());
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
    assert_eq!(plan.options.source_plan_count(), 1);
    assert_eq!(plan.retained_source_station_count(), 82);
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
    // Two complete DPs need 60 transitions; eight trials and three reserved replays need one node each.
    options["production"]["parallel"]["max_search_nodes"] = json!(70);
    assert_eq!(
        build(&pattern, options.clone()).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    options["production"]["parallel"]["max_search_nodes"] = json!(71);
    let typed: SewOptions = serde_json::from_value(options.clone()).unwrap();
    assert_eq!(typed.scheduling_work_bound().unwrap(), 2071);
    let plan = build(&pattern, options).unwrap();
    let parallel = plan.production.unwrap().parallel.unwrap();
    assert!(parallel.budget_exhausted);
    assert!(!parallel.optimality_proven);
    assert!(!parallel.search_complete);
    assert!(parallel.search_nodes <= 71);
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
fn compact_count_receipts_replay_exact_schedules_and_charge_every_reconstruction() {
    let (pattern, options) = fixture(3);
    let plan = build(&pattern, options.clone()).unwrap();
    let parallel = plan.production.as_ref().unwrap().parallel.as_ref().unwrap();
    let cells = options["production"]["parallel"]["cells"]
        .as_array()
        .unwrap();
    let copies = 3;
    let cap = options["production"]["parallel"]["max_search_nodes"]
        .as_u64()
        .unwrap() as usize;
    let allocation_work = cells.len() * (copies + 1) * (copies + 2);
    let replay_slots = 1 + cells.len().min(copies);
    let per_trial = (cap - allocation_work) / (cells.len() * copies + replay_slots);
    let serial = |cell: &Value, count: usize, node_limit: usize| {
        let mut input = options.clone();
        input["production"] = json!({
            "production_copies": count,
            "ordering": cell["ordering"],
            "handling": cell["handling"],
            "workholding": cell["workholding"]
        });
        input["production"]["ordering"]["max_search_nodes"] = json!(node_limit);
        build(&pattern, input).unwrap().production.unwrap()
    };
    let mut actual_work = allocation_work;
    for cell in cells {
        let declared = cell["ordering"]["max_search_nodes"].as_u64().unwrap() as usize;
        let node_limit = per_trial.min(declared);
        for count in 1..=copies {
            actual_work += serial(cell, count, node_limit).ordering.search_nodes;
        }
    }
    assert!(parallel.selected.makespan_milliseconds < parallel.baseline.makespan_milliseconds);
    for assignment in [&parallel.baseline, &parallel.selected] {
        for cell in &assignment.cells {
            if let Some(schedule) = &cell.schedule {
                let input = cells
                    .iter()
                    .find(|input| input["id"].as_str() == Some(cell.cell.as_str()))
                    .unwrap();
                let replay = serial(input, cell.copies.len(), cell.schedule_node_limit);
                actual_work += replay.ordering.search_nodes;
                assert_eq!(
                    serde_json::to_value(schedule).unwrap(),
                    serde_json::to_value(replay).unwrap()
                );
                assert!(cell.schedule_node_limit <= per_trial);
            }
        }
    }
    assert_eq!(parallel.search_nodes, actual_work);
    assert!(parallel.search_nodes <= cap);
    assert_eq!(parallel.selected.makespan_milliseconds, 25500);
    assert_eq!(parallel.lower_bound_makespan_milliseconds, 25500);
    assert!(parallel.optimality_proven);
    for cell in &plan
        .options
        .production
        .as_ref()
        .unwrap()
        .parallel
        .as_ref()
        .unwrap()
        .cells
    {
        let original = cells
            .iter()
            .find(|input| input["id"].as_str() == Some(cell.id.as_str()))
            .unwrap();
        assert_eq!(
            cell.ordering.max_search_nodes,
            original["ordering"]["max_search_nodes"].as_u64().unwrap() as usize
        );
    }
}

#[test]
fn parallel_plan_refuses_a_batch_that_the_ordinary_stitch_program_cannot_emit() {
    let (_, mut options) = fixture(31);
    let mut source: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    for piece in source["pieces"].as_array_mut().unwrap() {
        piece["vertices"]["c"][1] = json!(10100.0);
        piece["vertices"]["d"][1] = json!(10100.0);
    }
    let pattern = Pattern::new(source);
    options["production"]["ordering"]["max_search_nodes"] = json!(1);
    let mut serial = options.clone();
    serial["production"]
        .as_object_mut()
        .unwrap()
        .remove("parallel");
    let serial: SewOptions = serde_json::from_value(serial).unwrap();
    let ordinary = SewPlan::build(&pattern, &serial).unwrap();
    assert_eq!(ordinary.total_stitch_intervals(), 250480);
    assert_eq!(
        SewProgram::build(&pattern, &serial).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    let refusal = build(&pattern, options).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::BudgetExceeded);
    assert!(refusal.message.contains("stitch-program"));
}

#[test]
fn configured_cell_stitch_bound_uses_its_actual_recipe_before_count_search() {
    let (_, mut options) = heterogeneous();
    let mut source: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    for piece in source["pieces"].as_array_mut().unwrap() {
        piece["vertices"]["c"][1] = json!(4100.0);
        piece["vertices"]["d"][1] = json!(4100.0);
    }
    let pattern = Pattern::new(source);
    options["production"]["production_copies"] = json!(31);
    options["production"]["ordering"]["max_search_nodes"] = json!(1);
    for recipe in options["production"]["parallel"]["cells"][1]["configuration"]["recipes"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        recipe["max_stitch_mm"] = json!(1.0);
    }
    let mut serial = options.clone();
    serial["production"]
        .as_object_mut()
        .unwrap()
        .remove("parallel");
    let reference = build(&pattern, serial).unwrap();
    assert_eq!(reference.total_stitch_intervals(), 101680);
    // The actual 1 mm cell recipe needs 2 * 4100 * 31 = 254200 intervals, although the
    // outer 2.5 mm reference fits. The bounded search can assign all copies to either cell.
    let refusal = build(&pattern, options).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::BudgetExceeded);
    assert!(refusal.message.contains("configured parallel cell"));
    assert!(refusal.message.contains("stitch-program"));
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
