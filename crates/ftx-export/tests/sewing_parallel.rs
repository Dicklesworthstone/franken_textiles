//! Synthetic independent cells qualify scheduling arithmetic, never physical factory throughput.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode};
use ftx_export::sewing::{
    SewOptions, SewPlan, SewingMachine, SewingQualification,
    production::{parallel::operator, workholding},
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

fn refresh_operator_locks(options: &mut Value) {
    let typed: SewOptions = serde_json::from_value(options.clone()).unwrap();
    let parallel = typed
        .production
        .as_ref()
        .unwrap()
        .parallel
        .as_ref()
        .unwrap();
    let locks: BTreeMap<_, _> = parallel
        .cells
        .iter()
        .map(|cell| {
            (
                cell.id.clone(),
                operator::cell_sha256(cell, &typed).unwrap(),
            )
        })
        .collect();
    options["production"]["parallel"]["shared_operator"]["cells"] = json!(locks);
}

fn shared_operator(options: &mut Value, work: usize) {
    let parallel = &mut options["production"]["parallel"];
    parallel["time_basis"] = json!("ceil_operation_feed_milliseconds/1");
    parallel["independence"] = json!("independent_machines_and_fixtures_shared_operator/1");
    parallel["max_search_nodes"] = json!(60000);
    parallel["shared_operator"] = json!({
        "model":"exclusive_setup_handling_autonomous_feed/1", "operator":"synthetic-person",
        "available_at_milliseconds":0, "cells":{},
        "initial_arrival_milliseconds":{"cell-a":0,"cell-b":0},
        "transfers":[{"from":"cell-a","to":"cell-b","milliseconds":0},
            {"from":"cell-b","to":"cell-a","milliseconds":0}],
        "max_search_nodes":work,
        "source":{"reference":"test:one-shared-operator","accessed_on":"2026-10-27",
            "note":"Invented one-person protocol; unattended feed, safe waiting and travel are explicitly synthetic"},
        "method":"One person travels, handles and sets up a cell, then releases it to unattended feed; travel excludes handling",
        "conditions":{"scope":"Independent physical machines and fixtures; one exclusive operator; no garment transfers"},
        "evidence":"estimate","allow_estimates":true
    });
    refresh_operator_locks(options);
}

fn shared_fixtures(options: &mut Value, capacity: u32) {
    let release = options["passes"].as_array().unwrap().last().unwrap()["id"].clone();
    for cell in options["production"]["parallel"]["cells"]
        .as_array_mut()
        .unwrap()
    {
        if cell["workholding"].is_null() {
            cell["workholding"] = json!({
                "machine_sha256":cell["handling"]["machine_sha256"],
                "qualifications":cell["handling"]["qualifications"],
                "resources":[{"id":"holding-jig","capacity":2}],
                "holds":[{"id":"garment-in-jig","resource":"holding-jig","units":1,
                    "acquire":"join","release":release}],
                "source":{"reference":"test:local-pooled-fixture-use","accessed_on":"2026-10-27",
                    "note":"Synthetic local fixture applicability, not physical workholding evidence"},
                "method":"Declared local acquisition and completion boundaries for pool admission",
                "evidence":"estimate","allow_estimates":true
            });
        }
    }
    shared_operator(options, 40000);
    let parallel = &mut options["production"]["parallel"];
    parallel["independence"] =
        json!("independent_machines_shared_operator_and_workholding_pools/1");
    parallel["shared_workholding"] = json!({
        "model":"shared_workholding_pools/1",
        "release_protocol":"available_at_feed_completion_with_access_in_cell_handling/1",
        "cells":parallel["shared_operator"]["cells"],
        "resources":[{"id":"common-jigs","capacity":capacity}],
        "bindings":[{"cell":"cell-a","resource":"holding-jig","pool":"common-jigs"},
            {"cell":"cell-b","resource":"holding-jig","pool":"common-jigs"}],
        "source":{"reference":"test:finite-shared-fixtures","accessed_on":"2026-10-27",
            "note":"Synthetic shared inventory and access timing, not measured production or fixture fit"},
        "method":"The declared pool is available after release-operation feed; all access is covered by qualified cell handling",
        "conditions":{"scope":"Whole garments stay in their physical cells; only explicitly bound fixture inventory is shared"},
        "evidence":"estimate","allow_estimates":true
    });
}

// One 40-interval, four-second operation per garment. Its attended phases are independently known:
// first loading/setup 1100 ms, each later garment handling 5000 ms, final unloading 200 ms.
fn one_operation(copies: usize) -> (Pattern, Value) {
    let (pattern, mut options) = fixture(copies);
    options["passes"].as_array_mut().unwrap().truncate(1);
    let production = &mut options["production"];
    production["workholding"] = Value::Null;
    for key in [
        "qualifications",
        "initial_milliseconds",
        "final_milliseconds",
    ] {
        production["handling"][key]
            .as_object_mut()
            .unwrap()
            .remove("reinforce");
    }
    production["handling"]["transitions"]
        .as_array_mut()
        .unwrap()
        .retain(|row| row["from"] == "join" && row["to"] == "join");
    let handling = production["handling"].clone();
    production["parallel"]["qualifications"] = handling["qualifications"].clone();
    for cell in production["parallel"]["cells"].as_array_mut().unwrap() {
        cell["handling"] = handling.clone();
        cell["workholding"] = Value::Null;
    }
    (pattern, options)
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
fn completion_deadline_uses_complete_parallel_allocation_for_both_source_models() {
    use ftx_export::sewing_selection::{
        Request,
        deadline::{Basis, Status},
    };

    for ((pattern, options), finish) in [(fixture(3), 25500), (heterogeneous(), 9900)] {
        let pass = &options["passes"][0];
        let mut request: Request = serde_json::from_value(json!({
            "schema":"ftx-sewing-selection/5","options":options,
            "objective":"minimize_parallel_makespan","max_plan_evaluations":2,
            "completion_deadline_milliseconds":finish,
            "alternatives":[{"operation":"join","recipes":[{"id":"same-authored-pass",
                "needle":pass["needle"],"thread":pass["thread"],
                "max_stitch_mm":pass["max_stitch_mm"],
                "stitches_per_minute":pass["stitches_per_minute"]}]}]}))
        .unwrap();
        let exact = request.select(&pattern).unwrap();
        assert!(exact.optimality_proven && exact.unresolved_trials == 0);
        assert_eq!(
            exact.selected_objective_seconds,
            Some(f64::from(finish) / 1000.0)
        );
        let timing = exact.trials[0].completion_deadline.as_ref().unwrap();
        assert_eq!(timing.status, Status::Feasible);
        assert_eq!(timing.basis, Some(Basis::ParallelMakespan));
        assert_eq!(timing.selected_makespan_milliseconds, Some(finish as u64));
        assert_eq!(
            timing.lower_bound_makespan_milliseconds,
            Some(finish as u64)
        );
        assert_eq!(timing.timing_optimality_proven, Some(true));
        request.completion_deadline_milliseconds = Some(finish as u64 - 1);
        let excluded = request.select(&pattern).unwrap();
        assert!(excluded.selected_plan.is_none() && excluded.no_feasible_listed_recipe);
        assert_eq!(excluded.unresolved_trials, 0);
        assert_eq!(
            excluded.trials[0].objective_seconds,
            Some(f64::from(finish) / 1000.0)
        );
        assert_eq!(
            excluded.trials[0]
                .completion_deadline
                .as_ref()
                .unwrap()
                .status,
            Status::Excluded
        );
    }
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
fn finite_cell_windows_reassign_actual_recipes_stitches_and_thread_inventory() {
    let (pattern, mut options) = heterogeneous();
    thread_inventory(&mut options);
    // A slow-cell garment needs 9900 ms including unloading. Its qualified window now ends
    // one millisecond too soon, so every complete garment must move to the faster configuration.
    options["production"]["parallel"]["cells"][0]["available_until_milliseconds"] = json!(9899);
    options["production"]["parallel"]["cells"][1]["available_until_milliseconds"] = json!(11100);
    let typed: SewOptions = serde_json::from_value(options).unwrap();
    let program = SewProgram::build(&pattern, &typed).unwrap();
    let parallel = program
        .plan
        .production
        .as_ref()
        .unwrap()
        .parallel
        .as_ref()
        .unwrap();
    assert_eq!(parallel.schema, "ftx-sewing-parallel/3");
    assert_eq!(parallel.selected.makespan_milliseconds, 11100);
    assert_eq!(parallel.lower_bound_makespan_milliseconds, 11100);
    assert!(parallel.optimality_proven);
    assert!(parallel.selected.cells[0].copies.is_empty());
    assert_eq!(parallel.selected.cells[1].copies, vec![0, 1, 2]);
    assert_eq!(
        parallel.selected.cells[1].available_until_milliseconds,
        Some(11100)
    );
    let admission = parallel.availability.as_ref().unwrap();
    assert!(admission.baseline_replaced);
    assert!(
        admission.cells[0]
            .counts
            .iter()
            .all(|count| !count.feasible && count.infeasibility_proven)
    );
    assert!(admission.cells[1].counts[2].feasible);
    assert_eq!(
        serde_json::to_value(&parallel.baseline).unwrap(),
        serde_json::to_value(&parallel.selected).unwrap()
    );
    assert_eq!(program.stitches.len(), 120);
    assert!(
        program
            .stitches
            .iter()
            .all(|stitch| stitch.cell.as_deref() == Some("cell-b"))
    );
    let inventory = program.plan.consumables.as_ref().unwrap();
    assert!((inventory.total_required_length_m.unwrap() - 2.4).abs() < 1e-12);
    assert!((inventory.total_required_mass_kg.unwrap() - 57.6e-6).abs() < 1e-15);
    assert_eq!(
        inventory
            .purchasing
            .as_ref()
            .unwrap()
            .least_observed_independent_order_total,
        Some(14.0)
    );
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
    assert!(
        parallel.search_nodes
            <= typed
                .production
                .as_ref()
                .unwrap()
                .parallel
                .as_ref()
                .unwrap()
                .max_search_nodes
    );
}

#[test]
fn cell_window_finish_is_inclusive_and_counts_availability_and_final_unloading() {
    let (pattern, mut options) = fixture(1);
    options["production"]["parallel"]["cells"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    options["production"]["parallel"]["cells"][0]["available_at_milliseconds"] = json!(1000);
    options["production"]["parallel"]["cells"][0]["available_until_milliseconds"] = json!(10900);
    let plan = build(&pattern, options.clone()).unwrap();
    let parallel = plan.production.as_ref().unwrap().parallel.as_ref().unwrap();
    assert_eq!(parallel.selected.cells[0].finish_milliseconds, Some(10900));
    assert!(!parallel.availability.as_ref().unwrap().baseline_replaced);
    for end in [10899, 10700] {
        // The second limit accommodates all sewing and setup but not the final 200 ms unload.
        options["production"]["parallel"]["cells"][0]["available_until_milliseconds"] = json!(end);
        let refusal = build(&pattern, options.clone()).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::InvalidDocument);
        assert!(refusal.message.contains(
            "even the complete allocation over valid serial finish lower bounds is infeasible"
        ));
    }
}

#[test]
fn empty_cell_window_is_idle_but_malformed_or_inexact_windows_refuse() {
    let (pattern, mut options) = fixture(1);
    options["production"]["parallel"]["cells"][0]["available_until_milliseconds"] = json!(0);
    let plan = build(&pattern, options.clone()).unwrap();
    let parallel = plan.production.unwrap().parallel.unwrap();
    assert!(parallel.selected.cells[0].copies.is_empty());
    assert_eq!(parallel.selected.cells[0].finish_milliseconds, None);
    assert_eq!(parallel.selected.cells[1].copies, vec![0]);
    assert!(parallel.availability.unwrap().baseline_replaced);
    for (start, end) in [(1u32, 0u64), (0, 9_007_199_254_740_992)] {
        options["production"]["parallel"]["cells"][0]["available_at_milliseconds"] = json!(start);
        options["production"]["parallel"]["cells"][0]["available_until_milliseconds"] = json!(end);
        assert_eq!(
            build(&pattern, options.clone()).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
    }
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
    let legacy_parallel = serde_json::to_value(parallel).unwrap();
    assert!(legacy_parallel.get("availability").is_none());
    assert!(
        legacy_parallel["selected"]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .all(|cell| cell.get("available_until_milliseconds").is_none())
    );
    assert!(
        serde_json::to_value(&plan.options).unwrap()["production"]["parallel"]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .all(|cell| cell.get("available_until_milliseconds").is_none())
    );
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
fn finite_windows_match_independent_exhaustive_feasible_and_infeasible_assignments() {
    for (end_a, end_b, available_b) in [
        (30_000, 30_000, 0),
        (9_899, 50_000, 1_000),
        (50_000, 0, 0),
        (25_500, 9_900, 0),
        (9_899, 25_499, 0),
        (40_000, 30_000, 10_000),
    ] {
        let (pattern, mut options) = fixture(3);
        for (cell, end) in [(0, end_a), (1, end_b)] {
            options["production"]["parallel"]["cells"][cell]["available_until_milliseconds"] =
                json!(end);
        }
        options["production"]["parallel"]["cells"][1]["available_at_milliseconds"] =
            json!(available_b);
        let exact = (0..=3)
            .filter_map(|a| {
                let b = 3 - a;
                let finish_a = serial_cost(a, 2, 5000);
                let finish_b = if b == 0 {
                    0
                } else {
                    serial_cost(b, 2, 5000) + available_b
                };
                ((a == 0 || finish_a <= end_a) && (b == 0 || finish_b <= end_b))
                    .then_some(finish_a.max(finish_b))
            })
            .min();
        match (exact, build(&pattern, options)) {
            (Some(exact), Ok(plan)) => {
                let parallel = plan.production.unwrap().parallel.unwrap();
                assert_eq!(parallel.selected.makespan_milliseconds, exact);
                assert_eq!(parallel.lower_bound_makespan_milliseconds, exact);
                assert!(parallel.optimality_proven);
                for assignment in [&parallel.baseline, &parallel.selected] {
                    for cell in &assignment.cells {
                        assert!(cell.finish_milliseconds.is_none_or(
                            |finish| finish <= cell.available_until_milliseconds.unwrap()
                        ));
                    }
                }
            }
            (None, Err(refusal)) => assert_eq!(refusal.code, RefusalCode::InvalidDocument),
            (expected, actual) => panic!("expected {expected:?}, got {actual:?}"),
        }
    }
}

#[test]
fn bounded_search_does_not_claim_cell_window_infeasibility_from_a_slow_incumbent() {
    let (pattern, mut options) = fixture(4);
    // Two trials with two garments would fit a 17800 ms window after complete optimization.
    // One node per inner search retains the slower serial-copy seed; lower bounds still permit it.
    options["production"]["parallel"]["max_search_nodes"] = json!(71);
    for cell in options["production"]["parallel"]["cells"]
        .as_array_mut()
        .unwrap()
    {
        cell["available_until_milliseconds"] = json!(17800);
        for transition in cell["handling"]["transitions"].as_array_mut().unwrap() {
            if transition["relation"] == "different_copy" {
                transition["milliseconds"] = json!(0);
            }
        }
    }
    let refusal = build(&pattern, options.clone()).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::NotConverged);
    assert!(refusal.message.contains("infeasibility is not proved"));
    options["production"]["parallel"]["max_search_nodes"] = json!(10000);
    let parallel = build(&pattern, options)
        .unwrap()
        .production
        .unwrap()
        .parallel
        .unwrap();
    assert_eq!(parallel.selected.makespan_milliseconds, 17800);
    assert_eq!(
        parallel.selected.makespan_milliseconds,
        serial_cost(2, 2, 0)
    );
    assert!(parallel.optimality_proven);
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

#[test]
fn shared_operator_overlaps_only_autonomous_feed_and_replays_every_attended_phase() {
    let (pattern, mut options) = one_operation(2);
    let independent = build(&pattern, options.clone()).unwrap();
    assert_eq!(independent.parallel_makespan_seconds(), Some(5.3));
    shared_operator(&mut options, 40000);
    let typed: SewOptions = serde_json::from_value(options).unwrap();
    let program = SewProgram::build(&pattern, &typed).unwrap();
    let report = program
        .plan
        .production
        .as_ref()
        .unwrap()
        .parallel
        .as_ref()
        .unwrap();
    assert_eq!(report.schema, "ftx-sewing-parallel/4");
    assert_eq!(report.time_basis, "ceil_operation_feed_milliseconds/1");
    assert_eq!(report.baseline.makespan_milliseconds, 14300);
    assert_eq!(report.selected.makespan_milliseconds, 6400);
    assert_eq!(report.lower_bound_makespan_milliseconds, 5300);
    assert!(!report.optimality_proven);
    let labor = report.shared_operator.as_ref().unwrap();
    assert!(labor.search_complete);
    assert!(!labor.budget_exhausted);
    assert_eq!(labor.serial_order_scope, "retained_serial_incumbents/1");
    assert_eq!(labor.selected.busy_milliseconds, 2600);
    assert_eq!(labor.selected.waiting_milliseconds, 3800);
    assert_eq!(labor.selected.steps.len(), 4);
    assert!(
        labor.selected.steps[1].operator_release_milliseconds
            < labor.selected.steps[0].feed_finish_milliseconds
    );
    let mut operator_free = 0;
    let mut cell_ready = BTreeMap::<String, u64>::new();
    let mut operation_copies = BTreeSet::new();
    for step in &labor.selected.steps {
        assert_eq!(step.departure_milliseconds, operator_free);
        assert!(step.arrival_milliseconds >= step.departure_milliseconds);
        assert!(step.handling_start_milliseconds >= step.arrival_milliseconds);
        assert!(step.handling_start_milliseconds >= *cell_ready.get(&step.cell).unwrap_or(&0));
        assert!(step.setup_start_milliseconds >= step.handling_start_milliseconds);
        assert!(step.operator_release_milliseconds >= step.setup_start_milliseconds);
        assert!(step.feed_finish_milliseconds >= step.operator_release_milliseconds);
        if let Some(copy) = step.copy {
            operation_copies.insert(copy);
        }
        operator_free = step.operator_release_milliseconds;
        cell_ready.insert(step.cell.clone(), step.feed_finish_milliseconds);
    }
    assert_eq!(operation_copies, BTreeSet::from([0, 1]));
    assert_eq!(operator_free, 6400);
    for cell in &report.selected.cells {
        assert_eq!(cell.copies.len(), 1);
        assert_eq!(
            cell.finish_milliseconds,
            cell_ready.get(&cell.cell).copied()
        );
    }
    assert_eq!(program.stitches.len(), 80);
    for step in &program.stitches {
        let cell = report
            .selected
            .cells
            .iter()
            .find(|cell| Some(&cell.cell) == step.cell.as_ref())
            .unwrap();
        assert!(cell.copies.contains(&step.copy.unwrap()));
    }
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
    assert!(
        report.search_nodes
            <= typed
                .production
                .as_ref()
                .unwrap()
                .parallel
                .as_ref()
                .unwrap()
                .max_search_nodes
    );
}

#[test]
fn shared_fixture_capacity_controls_actual_feed_overlap_and_replays_complete_stitches() {
    for (capacity, finish) in [(1, 10400), (2, 6400)] {
        let (pattern, mut options) = one_operation(2);
        shared_fixtures(&mut options, capacity);
        let typed: SewOptions = serde_json::from_value(options).unwrap();
        let program = SewProgram::build(&pattern, &typed).unwrap();
        let report = program
            .plan
            .production
            .as_ref()
            .unwrap()
            .parallel
            .as_ref()
            .unwrap();
        assert_eq!(report.schema, "ftx-sewing-parallel/5");
        assert_eq!(report.selected.makespan_milliseconds, finish);
        assert!(report.lower_bound_makespan_milliseconds <= finish);
        let resources = report.shared_workholding.as_ref().unwrap();
        assert_eq!(resources.schema, "ftx-sewing-shared-workholding/1");
        assert!(!resources.physical_pool_access_validated);
        assert_eq!(resources.selected.holds.len(), 2);
        assert_eq!(resources.selected.peak_occupancy["common-jigs"], capacity);
        assert_eq!(resources.selected.final_occupancy["common-jigs"], 0);
        assert_eq!(resources.baseline.final_occupancy["common-jigs"], 0);
        let labor = report.shared_operator.as_ref().unwrap();
        let mut holds: Vec<_> = resources.selected.holds.iter().collect();
        holds.sort_by_key(|hold| hold.acquired_at_milliseconds);
        for hold in &holds {
            let step = labor
                .selected
                .steps
                .iter()
                .find(|step| {
                    step.cell == hold.cell && step.operation.as_ref() == Some(&hold.acquire)
                })
                .unwrap();
            assert_eq!(step.copy, Some(hold.copy));
            assert_eq!(
                step.handling_start_milliseconds,
                hold.acquired_at_milliseconds
            );
            assert_eq!(step.feed_finish_milliseconds, hold.released_at_milliseconds);
            assert_eq!(hold.acquire, hold.release);
            assert_eq!(hold.units, 1);
            assert!(hold.released_at_milliseconds > step.operator_release_milliseconds);
        }
        if capacity == 1 {
            // Reuse is legal exactly at feed completion, before the first cell's final unload.
            assert_eq!(holds[0].released_at_milliseconds, 5100);
            assert_eq!(
                holds[1].acquired_at_milliseconds,
                holds[0].released_at_milliseconds
            );
        } else {
            assert!(holds[1].acquired_at_milliseconds < holds[0].released_at_milliseconds);
        }
        assert_eq!(program.stitches.len(), 80);
        assert_eq!(
            program
                .stitches
                .iter()
                .map(|stitch| stitch.copy.unwrap())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([0, 1])
        );
        assert_eq!(
            serde_json::to_value(&program).unwrap(),
            serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap())
                .unwrap()
        );
    }
}

#[test]
fn shared_fixtures_stay_with_a_garment_until_its_declared_release_operation_finishes() {
    for (capacity, finish) in [(1, 19600), (2, 11000)] {
        let (pattern, mut options) = fixture(2);
        shared_fixtures(&mut options, capacity);
        let plan = build(&pattern, options.clone()).unwrap();
        let report = plan.production.as_ref().unwrap().parallel.as_ref().unwrap();
        assert_eq!(report.selected.makespan_milliseconds, finish);
        let resources = report.shared_workholding.as_ref().unwrap();
        let labor = report.shared_operator.as_ref().unwrap();
        for hold in &resources.selected.holds {
            let acquisition = labor
                .selected
                .steps
                .iter()
                .find(|step| {
                    step.cell == hold.cell && step.operation.as_ref() == Some(&hold.acquire)
                })
                .unwrap();
            let release = labor
                .selected
                .steps
                .iter()
                .find(|step| {
                    step.cell == hold.cell && step.operation.as_ref() == Some(&hold.release)
                })
                .unwrap();
            assert!(hold.acquire.ends_with("/join"));
            assert!(hold.release.ends_with("/reinforce"));
            assert_eq!(
                hold.acquired_at_milliseconds,
                acquisition.handling_start_milliseconds
            );
            assert_eq!(
                hold.released_at_milliseconds,
                release.feed_finish_milliseconds
            );
            assert!(hold.released_at_milliseconds > acquisition.feed_finish_milliseconds);
        }
        if capacity == 1 {
            let mut holds: Vec<_> = resources.selected.holds.iter().collect();
            holds.sort_by_key(|hold| hold.acquired_at_milliseconds);
            assert_eq!(holds[0].released_at_milliseconds, 9700);
            assert_eq!(holds[1].acquired_at_milliseconds, 9700);
            // An identical local resource name does not share inventory without an explicit binding.
            options["production"]["parallel"]["shared_workholding"]["bindings"]
                .as_array_mut()
                .unwrap()
                .truncate(1);
            let independent_b = build(&pattern, options).unwrap();
            let report = independent_b.production.unwrap().parallel.unwrap();
            assert_eq!(report.selected.makespan_milliseconds, 11000);
            assert_eq!(report.shared_workholding.unwrap().selected.holds.len(), 1);
        }
    }
}

#[test]
fn shared_pool_counts_units_across_different_local_resource_names() {
    for (capacity, finish) in [(2, 19600), (3, 11000)] {
        let (pattern, mut options) = fixture(2);
        let cells = options["production"]["parallel"]["cells"]
            .as_array_mut()
            .unwrap();
        cells[0]["workholding"]["holds"][0]["units"] = json!(2);
        cells[1]["workholding"]["resources"][0] = json!({"id":"other-local-jig","capacity":1});
        cells[1]["workholding"]["holds"][0]["resource"] = json!("other-local-jig");
        shared_fixtures(&mut options, capacity);
        options["production"]["parallel"]["shared_workholding"]["bindings"][1]["resource"] =
            json!("other-local-jig");
        let plan = build(&pattern, options).unwrap();
        let report = plan.production.unwrap().parallel.unwrap();
        assert_eq!(report.selected.makespan_milliseconds, finish);
        let resources = report.shared_workholding.unwrap();
        assert_eq!(resources.selected.peak_occupancy["common-jigs"], capacity);
        assert_eq!(resources.selected.final_occupancy["common-jigs"], 0);
        assert_eq!(
            resources
                .selected
                .holds
                .iter()
                .map(|hold| (hold.cell.as_str(), hold.units))
                .collect::<BTreeMap<_, _>>(),
            BTreeMap::from([("cell-a", 2), ("cell-b", 1)])
        );
    }
}

#[test]
fn shared_fixture_waiting_obeys_inclusive_cell_windows_and_final_unloading() {
    for (end, accepted) in [(10400, true), (10399, false)] {
        let (pattern, mut options) = one_operation(2);
        for cell in options["production"]["parallel"]["cells"]
            .as_array_mut()
            .unwrap()
        {
            cell["available_until_milliseconds"] = json!(end);
        }
        shared_fixtures(&mut options, 1);
        match build(&pattern, options) {
            Ok(plan) => {
                assert!(accepted);
                let report = plan.production.unwrap().parallel.unwrap();
                assert_eq!(report.selected.makespan_milliseconds, 10400);
                assert!(report.shared_operator.unwrap().baseline_replaced);
                assert!(
                    report
                        .selected
                        .cells
                        .iter()
                        .all(|cell| cell.finish_milliseconds.is_some_and(|finish| finish <= end))
                );
            }
            Err(refusal) => {
                assert!(!accepted);
                assert_eq!(refusal.code, RefusalCode::NotConverged);
                assert!(refusal.message.contains("infeasibility is not proved"));
            }
        }
    }
}

#[test]
fn shared_fixture_bounded_search_keeps_a_complete_resource_feasible_seed() {
    let (pattern, mut options) = one_operation(2);
    shared_fixtures(&mut options, 1);
    options["production"]["parallel"]["shared_operator"]["max_search_nodes"] = json!(3);
    let plan = build(&pattern, options).unwrap();
    let report = plan.production.unwrap().parallel.unwrap();
    assert_eq!(report.selected.makespan_milliseconds, 14300);
    assert!(report.budget_exhausted);
    assert!(!report.optimality_proven);
    let labor = report.shared_operator.unwrap();
    assert_eq!(labor.search_nodes, 3);
    assert!(!labor.search_complete);
    let resources = report.shared_workholding.unwrap();
    assert_eq!(resources.selected.holds.len(), 2);
    assert_eq!(resources.selected.peak_occupancy["common-jigs"], 1);
    assert_eq!(resources.selected.final_occupancy["common-jigs"], 0);
    assert!(
        resources
            .selected
            .holds
            .windows(2)
            .all(|holds| holds[0].released_at_milliseconds <= holds[1].acquired_at_milliseconds)
    );
}

#[test]
fn shared_pool_blocked_serial_seed_is_replaced_by_a_complete_feasible_allocation() {
    let (pattern, mut options) = fixture(2);
    for cell in options["production"]["parallel"]["cells"]
        .as_array_mut()
        .unwrap()
    {
        for row in cell["handling"]["transitions"].as_array_mut().unwrap() {
            if row["relation"] == "different_copy" {
                row["milliseconds"] = json!(0);
            }
        }
    }
    // Local inventory admits a cheaper grouped-join incumbent. That order needs two pool units
    // simultaneously, so the one-unit pool must find and retain another complete allocation.
    shared_fixtures(&mut options, 1);
    let plan = build(&pattern, options).unwrap();
    let report = plan.production.unwrap().parallel.unwrap();
    assert_eq!(report.selected.makespan_milliseconds, 19600);
    assert!(
        report
            .selected
            .cells
            .iter()
            .all(|cell| cell.copies.len() == 1)
    );
    assert!(report.shared_operator.unwrap().baseline_replaced);
    let resources = report.shared_workholding.unwrap();
    assert_eq!(resources.baseline.holds.len(), 2);
    assert_eq!(resources.baseline.peak_occupancy["common-jigs"], 1);
    assert_eq!(resources.baseline.final_occupancy["common-jigs"], 0);
}

#[test]
fn shared_fixture_source_policy_refuses_unlocked_or_unqualified_inventory() {
    let (pattern, mut original) = one_operation(2);
    shared_fixtures(&mut original, 1);
    for (field, value) in [
        ("allow_estimates", json!(false)),
        ("model", json!("automatic-inventory")),
        ("release_protocol", json!("release-at-operator-departure")),
        ("cells", json!({})),
        ("resources", json!([{ "id":"common-jigs", "capacity":0 }])),
        (
            "resources",
            json!([{ "id":"common-jigs", "capacity":1 }, { "id":"common-jigs", "capacity":1 }]),
        ),
        (
            "bindings",
            json!([{ "cell":"cell-a", "resource":"unknown", "pool":"common-jigs" }]),
        ),
        (
            "bindings",
            json!([{ "cell":"unknown", "resource":"holding-jig", "pool":"common-jigs" }]),
        ),
        (
            "bindings",
            json!([{ "cell":"cell-a", "resource":"holding-jig", "pool":"unknown" }]),
        ),
        (
            "bindings",
            json!([{ "cell":"cell-a", "resource":"holding-jig", "pool":"common-jigs" },
            { "cell":"cell-a", "resource":"holding-jig", "pool":"common-jigs" }]),
        ),
    ] {
        let mut wrong = original.clone();
        wrong["production"]["parallel"]["shared_workholding"][field] = value;
        assert!(build(&pattern, wrong).is_err(), "{field}");
    }
    let mut wrong = original.clone();
    wrong["production"]["parallel"]["shared_operator"] = Value::Null;
    assert!(build(&pattern, wrong).is_err());
    let mut wrong = original.clone();
    wrong["production"]["parallel"]["independence"] =
        json!("independent_machines_and_fixtures_shared_operator/1");
    assert!(build(&pattern, wrong).is_err());
    // Refreshing labor evidence cannot silently refresh independent fixture source qualification.
    let mut wrong = original;
    wrong["production"]["parallel"]["cells"][1]["available_at_milliseconds"] = json!(1);
    refresh_operator_locks(&mut wrong);
    assert!(
        build(&pattern, wrong)
            .unwrap_err()
            .message
            .contains("content lock mismatch")
    );
}

#[derive(Clone, Copy)]
struct OperatorCase {
    starts: [u64; 2],
    ends: [Option<u64>; 2],
    available: u64,
    until: Option<u64>,
    arrivals: [u64; 2],
    transfers: [[u64; 2]; 2],
}

// Exhaust all dispatch words for two fixed one-operation garment chains. This uses only independently
// specified durations, and neither production receipts nor the scheduler's bounds/pruning/heuristics.
fn operator_oracle(copies: usize, case: OperatorCase) -> Option<u64> {
    fn enumerate(
        case: OperatorCase,
        phases: &[Vec<(u64, u64)>; 2],
        positions: [usize; 2],
        ready: [u64; 2],
        previous: Option<usize>,
        free: u64,
    ) -> Option<u64> {
        if (0..2).all(|cell| positions[cell] == phases[cell].len()) {
            return Some(
                (0..2)
                    .filter(|&cell| !phases[cell].is_empty())
                    .map(|cell| ready[cell])
                    .max()
                    .unwrap(),
            );
        }
        (0..2)
            .filter_map(|cell| {
                let &(attended, feed) = phases[cell].get(positions[cell])?;
                let travel =
                    previous.map_or(case.arrivals[cell], |from| case.transfers[from][cell]);
                let release = (free + travel).max(ready[cell]) + attended;
                let finish = release + feed;
                if case.until.is_some_and(|end| release > end)
                    || case.ends[cell].is_some_and(|end| finish > end)
                {
                    return None;
                }
                let mut next = positions;
                next[cell] += 1;
                let mut completion = ready;
                completion[cell] = finish;
                enumerate(case, phases, next, completion, Some(cell), release)
            })
            .min()
    }
    (0..=copies)
        .filter_map(|a| {
            let phases = [a, copies - a].map(|count| {
                if count == 0 {
                    return Vec::new();
                }
                let mut phases: Vec<_> = (0..count)
                    .map(|index| (if index == 0 { 1100 } else { 5000 }, 4000))
                    .collect();
                phases.push((200, 0));
                phases
            });
            enumerate(case, &phases, [0, 0], case.starts, None, case.available)
        })
        .min()
}

#[test]
fn shared_operator_matches_exhaustive_allocations_travel_waits_and_inclusive_windows() {
    let base = OperatorCase {
        starts: [0, 0],
        ends: [None, None],
        available: 0,
        until: None,
        arrivals: [0, 0],
        transfers: [[0, 0], [0, 0]],
    };
    for (copies, case) in [
        (2, base),
        (3, base),
        (
            2,
            OperatorCase {
                arrivals: [700, 100],
                transfers: [[0, 900], [200, 0]],
                ..base
            },
        ),
        (
            3,
            OperatorCase {
                starts: [3000, 0],
                available: 500,
                transfers: [[0, 250], [600, 0]],
                ..base
            },
        ),
        (
            2,
            OperatorCase {
                ends: [Some(6400), Some(6400)],
                until: Some(6400),
                ..base
            },
        ),
        (
            2,
            OperatorCase {
                ends: [Some(6399), Some(6399)],
                ..base
            },
        ),
        (
            1,
            OperatorCase {
                ends: [Some(0), Some(5360)],
                arrivals: [0, 60],
                until: Some(5360),
                ..base
            },
        ),
    ] {
        let (pattern, mut options) = one_operation(copies);
        for cell in 0..2 {
            options["production"]["parallel"]["cells"][cell]["available_at_milliseconds"] =
                json!(case.starts[cell]);
            options["production"]["parallel"]["cells"][cell]["available_until_milliseconds"] =
                json!(case.ends[cell]);
        }
        shared_operator(&mut options, 40000);
        let operator = &mut options["production"]["parallel"]["shared_operator"];
        operator["available_at_milliseconds"] = json!(case.available);
        operator["available_until_milliseconds"] = json!(case.until);
        operator["initial_arrival_milliseconds"] =
            json!({"cell-a":case.arrivals[0],"cell-b":case.arrivals[1]});
        operator["transfers"][0]["milliseconds"] = json!(case.transfers[0][1]);
        operator["transfers"][1]["milliseconds"] = json!(case.transfers[1][0]);
        match (operator_oracle(copies, case), build(&pattern, options)) {
            (Some(exact), Ok(plan)) => {
                let report = plan.production.unwrap().parallel.unwrap();
                assert_eq!(report.selected.makespan_milliseconds, exact);
                assert!(report.lower_bound_makespan_milliseconds <= exact);
                assert_eq!(
                    report.optimality_proven,
                    report.lower_bound_makespan_milliseconds == exact
                );
                let labor = report.shared_operator.unwrap();
                assert!(labor.search_complete);
                if case.ends[0].is_some() {
                    assert!(labor.baseline_replaced);
                }
                for cell in report.selected.cells {
                    if let (Some(finish), Some(end)) =
                        (cell.finish_milliseconds, cell.available_until_milliseconds)
                    {
                        assert!(finish <= end);
                    }
                }
            }
            (None, Err(refusal)) => {
                // Completing the restricted serial-incumbent search cannot prove that all other
                // admitted serial orders fail, even when this deliberately simple fixture would.
                assert_eq!(refusal.code, RefusalCode::NotConverged);
                assert!(refusal.message.contains("infeasibility is not proved"));
            }
            (expected, actual) => panic!("expected {expected:?}, got {actual:?}"),
        }
    }
}

#[test]
fn shared_operator_retains_a_bounded_complete_seed_and_refuses_only_valid_shift_bounds() {
    let (pattern, mut options) = one_operation(2);
    shared_operator(&mut options, 3);
    let plan = build(&pattern, options.clone()).unwrap();
    let report = plan.production.unwrap().parallel.unwrap();
    assert_eq!(report.selected.makespan_milliseconds, 14300);
    assert!(!report.optimality_proven);
    assert!(report.budget_exhausted);
    let labor = report.shared_operator.unwrap();
    assert_eq!(labor.search_nodes, 3);
    assert_eq!(labor.selected.steps.len(), 3);
    assert!(!labor.search_complete);
    options["production"]["parallel"]["shared_operator"]["max_search_nodes"] = json!(2);
    let refused = build(&pattern, options.clone()).unwrap_err();
    assert_eq!(refused.code, RefusalCode::BudgetExceeded);
    assert!(refused.message.contains("complete first-cell seed"));
    options["production"]["parallel"]["shared_operator"]["max_search_nodes"] = json!(40000);
    options["production"]["parallel"]["shared_operator"]["available_until_milliseconds"] =
        json!(2599);
    let refused = build(&pattern, options.clone()).unwrap_err();
    assert_eq!(refused.code, RefusalCode::InvalidDocument);
    assert!(refused.message.contains("attended-work lower bounds"));
    options["production"]["parallel"]["shared_operator"]["available_until_milliseconds"] =
        json!(6399);
    assert_eq!(
        build(&pattern, options).unwrap_err().code,
        RefusalCode::NotConverged
    );
}

#[test]
fn shared_operator_source_domain_and_independent_consent_cannot_survive_changed_inputs() {
    let (pattern, mut original) = one_operation(2);
    shared_operator(&mut original, 40000);
    for field in [
        "allow_estimates",
        "model",
        "cells",
        "initial_arrival_milliseconds",
        "transfers",
    ] {
        let mut wrong = original.clone();
        wrong["production"]["parallel"]["shared_operator"][field] = match field {
            "allow_estimates" => json!(false),
            "model" => json!("fully-autonomous-with-no-source"),
            "transfers" => json!([]),
            _ => json!({}),
        };
        assert!(build(&pattern, wrong).is_err(), "{field}");
    }
    for (field, value) in [
        ("available_at_milliseconds", json!(1)),
        ("available_until_milliseconds", json!(50000)),
    ] {
        let mut wrong = original.clone();
        wrong["production"]["parallel"]["cells"][1][field] = value;
        let refused = build(&pattern, wrong).unwrap_err();
        assert!(refused.message.contains("content lock mismatch"));
    }
    let mut wrong = original.clone();
    wrong["passes"][0]["stitches_per_minute"] = json!(599.0);
    assert!(
        build(&pattern, wrong)
            .unwrap_err()
            .message
            .contains("content lock mismatch")
    );
    let mut wrong = original;
    wrong["production"]["parallel"]["time_basis"] = json!("ceil_cell_feed_milliseconds/1");
    assert!(build(&pattern, wrong).is_err());
}

#[test]
fn every_positive_autonomous_feed_keeps_its_own_ceiling_after_intervening_attended_work() {
    let (pattern, mut options) = heterogeneous();
    options["production"]["production_copies"] = json!(1);
    options["production"]["parallel"]["cells"][0]["available_until_milliseconds"] = json!(0);
    let fast = &mut options["production"]["parallel"]["cells"][1];
    fast["configuration"]["machine"]["max_stitches_per_minute"] = json!(6_000_000.0);
    fast["configuration"]["qualifications"][0]["stitches_per_minute"] = json!([1.0, 6_000_000.0]);
    for operation in ["join", "reinforce"] {
        fast["configuration"]["recipes"][operation]["stitches_per_minute"] = json!(6_000_000.0);
    }
    refresh_cell_locks(fast);
    shared_operator(&mut options, 40000);
    let plan = build(&pattern, options).unwrap();
    let report = plan.production.unwrap().parallel.unwrap();
    let fast = &report.selected.cells[1];
    assert_eq!(fast.scheduled_feed_milliseconds, 2);
    assert!((fast.feed_padding_seconds - 0.0016).abs() < 1e-12);
    assert_eq!(fast.finish_milliseconds, Some(902));
    assert_eq!(report.lower_bound_makespan_milliseconds, 902);
    assert!(report.optimality_proven);
    let steps = &report.shared_operator.unwrap().selected.steps;
    assert_eq!(steps.len(), 3);
    for step in &steps[..2] {
        assert_eq!(
            step.feed_finish_milliseconds - step.operator_release_milliseconds,
            1
        );
    }
    assert_eq!(steps[1].handling_start_milliseconds, 601);
    assert_eq!(steps[2].handling_start_milliseconds, 702);
}
