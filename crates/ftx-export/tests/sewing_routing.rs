//! Synthetic specialist cells exercise actual operation routing, not physical factory qualification.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_export::sewing::{
    SewOptions, SewPlan, SewingMachine, SewingQualification,
    production::{parallel::operator, workholding},
    program::SewProgram,
};
use ftx_pattern::Pattern;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::materials::Catalog;
use serde_json::{Value, json};

fn source(id: &str) -> Value {
    json!({"reference":format!("test:routing-{id}"),"accessed_on":"2026-10-27",
        "note":"Invented software inputs; no measured process, factory timing or seam-quality claim"})
}

fn refresh_locks(options: &mut Value) {
    for cell in options["production"]["routing"]["cells"]
        .as_array_mut()
        .unwrap()
    {
        let machine: SewingMachine =
            serde_json::from_value(cell["configuration"]["machine"].clone()).unwrap();
        let hash = workholding::machine_sha256(&machine).unwrap();
        let qualifications: Vec<SewingQualification> =
            serde_json::from_value(cell["configuration"]["qualifications"].clone()).unwrap();
        let refs: BTreeMap<_, _> = cell["configuration"]["recipes"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, recipe)| {
                let qualification = qualifications
                    .iter()
                    .find(|q| recipe["needle"] == q.needle && recipe["thread"] == q.thread)
                    .unwrap();
                (
                    id.clone(),
                    json!([{"id":qualification.id,
                        "sha256":workholding::qualification_sha256(qualification).unwrap()}]),
                )
            })
            .collect();
        cell["configuration"]["machine_sha256"] = json!(hash);
        cell["configuration"]["qualification_refs"] = json!(refs);
        cell["handling"]["machine_sha256"] = json!(hash);
        cell["handling"]["qualifications"] = json!(refs);
        if let Some(holding) = cell.get_mut("workholding") {
            holding["machine_sha256"] = json!(hash);
            holding["qualifications"] = json!(refs);
        }
    }
    refresh_cell_locks(options);
}

fn refresh_cell_locks(options: &mut Value) {
    let typed: SewOptions = serde_json::from_value(options.clone()).unwrap();
    let locks: BTreeMap<_, _> = typed
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap()
        .cells
        .iter()
        .map(|cell| {
            (
                cell.id.clone(),
                operator::cell_sha256(cell, &typed).unwrap(),
            )
        })
        .collect();
    options["production"]["routing"]["cell_locks"] = json!(locks);
}

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
    let mut cells = Vec::new();
    for (index, operation) in ["join", "reinforce"].into_iter().enumerate() {
        let id = ["cell-a", "cell-b"][index];
        let machine_id = format!("synthetic-specialist-{id}");
        let needle = format!("needle-{id}");
        let thread = format!("thread-{id}");
        let pitch = [2.5, 5.0][index];
        let mut machine = options["machine"].clone();
        machine["id"] = json!(machine_id);
        machine["stitch_mm"] = json!([[2.0, 3.0], [4.0, 6.0]][index]);
        let mut qualification = options["qualifications"][index].clone();
        qualification["id"] = json!(format!("qualified-{operation}"));
        qualification["machine"] = json!(machine_id);
        qualification["needle"] = json!(needle);
        qualification["thread"] = json!(thread);
        qualification["stitch_mm"] = machine["stitch_mm"].clone();
        cells.push(json!({
            "id":id,"available_at_milliseconds":0,
            "configuration":{"machine":machine,"machine_sha256":"",
                "qualifications":[qualification],"qualification_refs":{},
                "recipes":{operation:{"id":format!("recipe-{operation}"),"needle":needle,
                    "thread":thread,"max_stitch_mm":pitch,"stitches_per_minute":600.0}}},
            "ordering":{"objective":"minimize_production_time","max_search_nodes":100,
                "initial_setup":{"needle":needle,"thread":"idle"},
                "setup_times":{"machine":machine_id,"source":source(id),
                    "method":"Synthetic setup changes, separate from loading, unloading and feed",
                    "conditions":{},"evidence":"estimate","allow_estimates":true,
                    "transitions":[{"from":{"needle":needle,"thread":"idle"},
                        "to":{"needle":needle,"thread":thread},"milliseconds":([1000,500][index])}]}},
            "handling":{"model":"serial_operation_transitions/1","machine_sha256":"",
                "qualifications":{},"initial_milliseconds":{operation:100},
                "final_milliseconds":{operation:200},"transitions":[],"source":source(id),
                "method":"Full loading and unloading around each separate routed operation",
                "conditions":{},"evidence":"estimate","allow_estimates":true}
        }));
    }
    // This header is a source template, not an all-capable qualified production machine.
    options["qualifications"] = json!([]);
    options["production"] = json!({"production_copies":copies,
        "ordering":{"objective":"minimize_setup_changes","max_search_nodes":1},
        "routing":{"model":"fixed_operation_route/1","objective":"minimize_makespan",
            "time_basis":"ceil_operation_feed_milliseconds/1","protocol":"unloaded_between_operations/1",
            "independence":"independent_attended_cells_and_transfer_resource/1",
            "max_search_nodes":10000,"staging_capacity":copies,"cells":cells,"cell_locks":{},
            "route":[{"operation":"join","cell":"cell-a"},{"operation":"reinforce","cell":"cell-b"}],
            "transfer":{"id":"handoff-worker","model":"serialized_unloaded_handoffs/1",
                "available_at_milliseconds":0,
                "transfers":[{"from":"cell-a","to":"cell-b","milliseconds":700}],
                "source":source("handoff"),"method":"One exclusive handoff worker; every row includes positioning, pickup, movement and deposit, outside cell loading and unloading",
                "conditions":{},"evidence":"estimate","allow_estimates":true},
            "source":source("complete-route"),
            "method":"Independent machine and dedicated attending worker per cell, complete unloaded staging slots and one separate shared handoff worker",
            "conditions":{},"evidence":"estimate","allow_estimates":true}
    });
    refresh_locks(&mut options);
    (pattern, options)
}

fn build(pattern: &Pattern, options: Value) -> Result<SewPlan, Refusal> {
    SewPlan::build(pattern, &serde_json::from_value(options).unwrap())
}

fn attending_labor(mut options: Value, groups: &[&[&str]], travel: u32) -> Value {
    let workers: Vec<_> = groups
        .iter()
        .enumerate()
        .map(|(index, cells)| {
            let initial: BTreeMap<_, _> = cells.iter().map(|&cell| (cell, 0)).collect();
            let transfers: Vec<_> = cells
                .iter()
                .flat_map(|&from| {
                    cells
                        .iter()
                        .filter(move |&&to| to != from)
                        .map(move |&to| json!({"from":from,"to":to,"milliseconds":travel}))
                })
                .collect();
            json!({"id":format!("worker-{index}"),"cells":cells,
                "available_at_milliseconds":0,"initial_arrival_milliseconds":initial,
                "transfers":transfers})
        })
        .collect();
    let route = &mut options["production"]["routing"];
    route["independence"] = json!(if route.get("shared_workholding").is_some() {
        "shared_attending_workers_workholding_and_transfer_resource/1"
    } else {
        "shared_attending_workers_and_transfer_resource/1"
    });
    route["attending_labor"] = json!({
        "model":"exclusive_full_operation_attendance/1","cells":route["cell_locks"],
        "workers":workers,"source":source("attending-workers"),
        "method":"Invented workers attend all loading, setup, feed and unloading; separate initial and directed travel; the unloaded garment handoff worker remains independent",
        "conditions":{},"evidence":"estimate","allow_estimates":true
    });
    options
}

fn shared_fixtures(mut options: Value, capacity: u32) -> Value {
    for cell in options["production"]["routing"]["cells"]
        .as_array_mut()
        .unwrap()
    {
        let holds: Vec<_> = cell["configuration"]["recipes"]
            .as_object()
            .unwrap()
            .keys()
            .map(|operation| {
                json!({"id":format!("jig-{operation}"),"resource":"jig","units":1,
                "acquire":operation,"release":operation})
            })
            .collect();
        cell["workholding"] = json!({
            "machine_sha256":"","qualifications":{},
            "resources":[{"id":"jig","capacity":1}],"holds":holds,
            "source":source("fixture-local"),
            "method":"Synthetic local fixture applies through complete loading, setup, feed and unloading",
            "conditions":{},"evidence":"estimate","allow_estimates":true
        });
    }
    refresh_locks(&mut options);
    let route = &mut options["production"]["routing"];
    let bindings: Vec<_> = route["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| json!({"cell":cell["id"],"resource":"jig","pool":"common-jigs"}))
        .collect();
    route["independence"] =
        json!("independent_attended_cells_shared_workholding_and_transfer_resource/1");
    route["shared_workholding"] = json!({
        "model":"shared_workholding_pools/1",
        "release_protocol":"available_after_unload_with_access_in_cell_handling/1",
        "cells":route["cell_locks"],"resources":[{"id":"common-jigs","capacity":capacity}],
        "bindings":bindings,"source":source("fixture-pool"),
        "method":"Synthetic interchangeable pool: acquisition/access is included in qualified loading and complete return/release in unloading, independent of unloaded garment handoffs",
        "conditions":{},"evidence":"estimate","allow_estimates":true
    });
    options
}

fn refresh_pool_locks(options: &mut Value) {
    refresh_locks(options);
    let route = &mut options["production"]["routing"];
    route["shared_workholding"]["cells"] = route["cell_locks"].clone();
}

fn capture_routed_fixture(name: &str, value: &impl serde::Serialize) {
    let Some(directory) = std::env::var_os("FTX_ROUTED_FIXTURE_REPLAY") else {
        return;
    };
    use std::io::Write;
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(name))
        .unwrap();
    file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
}

#[test]
fn specialist_cells_jointly_sew_copies_with_exact_geometry_and_independent_timing() {
    let (pattern, raw) = fixture(2);
    let options: SewOptions = serde_json::from_value(raw).unwrap();
    assert!(options.qualifications.is_empty() && options.require_qualification);
    for cell in &options
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap()
        .cells
    {
        let whole_garment = cell.configured_options(&options).unwrap().unwrap();
        assert!(SewPlan::build(&pattern, &whole_garment).is_err());
    }
    let program = SewProgram::build(&pattern, &options).unwrap();
    let plan = &program.plan;
    let routed = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert!(routed.workholding.is_none());
    assert!(routed.attending_labor.is_none());
    assert!(
        serde_json::to_value(routed)
            .unwrap()
            .get("attending_labor")
            .is_none()
    );
    assert!(
        serde_json::to_value(routed)
            .unwrap()
            .get("workholding")
            .is_none()
    );
    assert_eq!(routed.cell_plans.len(), 2);
    assert_eq!(
        plan.pattern_sha256,
        sha256_hex(&serde_json::to_vec(pattern.raw()).unwrap())
    );
    assert_eq!(plan.total_stitch_intervals(), 120);
    assert_eq!(program.stitches.len(), 120);
    assert!((plan.total_feed_seconds() - 12.0).abs() < 1e-12);
    assert_eq!(routed.selected.makespan_milliseconds, 12600);
    assert_eq!(routed.selected.scheduled_feed_milliseconds, 12000);
    assert_eq!(routed.selected.setup_milliseconds, 1500);
    assert_eq!(routed.selected.handling_milliseconds, 1200);
    assert_eq!(routed.selected.transfer_milliseconds, 1400);
    assert!(!plan.hardware_execution_authorized && !plan.physical_seam_quality_validated);
    let mut counts = BTreeMap::new();
    for step in &program.stitches {
        let cell = step.cell.as_deref().unwrap();
        let source = &routed
            .cell_plans
            .iter()
            .find(|p| p.cell == cell)
            .unwrap()
            .plan;
        assert_eq!(source.pattern_sha256, plan.pattern_sha256);
        assert_eq!(source.seams.len(), 1);
        assert!(source.production.is_none() && source.consumables.is_none());
        let seam = &source.seams[step.operation];
        assert_eq!(step.a_mm, seam.stations.a[step.to_station]);
        assert_eq!(step.b_mm, seam.stations.b[step.to_station]);
        assert_eq!(step.feed_seconds, 0.1);
        *counts.entry((cell, step.copy.unwrap())).or_insert(0usize) += 1;
    }
    assert_eq!(
        counts,
        BTreeMap::from([
            (("cell-a", 0), 40),
            (("cell-a", 1), 40),
            (("cell-b", 0), 20),
            (("cell-b", 1), 20),
        ])
    );
    let rows: BTreeMap<_, _> = routed
        .selected
        .operations
        .iter()
        .map(|row| ((row.copy, row.operation_id.as_str()), row))
        .collect();
    for (copy, join_start, join_finish, finish_start, finish_end) in
        [(0, 0, 5300, 6000, 8800), (1, 5300, 9600, 10300, 12600)]
    {
        let join = rows[&(copy, "join")];
        let finish = rows[&(copy, "reinforce")];
        assert_eq!(join.loading_start_milliseconds, join_start);
        assert_eq!(join.finish_milliseconds, join_finish);
        assert_eq!(finish.loading_start_milliseconds, finish_start);
        assert_eq!(finish.finish_milliseconds, finish_end);
        assert_eq!(finish.cell, "cell-b");
        let transfer = &routed.selected.transfers[copy];
        assert_eq!(transfer.copy, copy);
        assert_eq!(transfer.start_milliseconds, join_finish);
        assert_eq!(transfer.finish_milliseconds, finish_start);
    }
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &plan.options).unwrap()).unwrap()
    );
    if let Some(directory) = std::env::var_os("FTX_ROUTING_REPLAY") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        for (name, value) in [
            ("pattern.json", pattern.raw().clone()),
            ("options.json", serde_json::to_value(&plan.options).unwrap()),
            ("program.json", serde_json::to_value(&program).unwrap()),
        ] {
            let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
            bytes.push(b'\n');
            std::fs::write(directory.join(name), bytes).unwrap();
        }
    }
}

#[test]
fn shared_attending_worker_serializes_full_feed_and_changes_actual_dispatch() {
    let (pattern, raw) = fixture(2);
    let options: SewOptions =
        serde_json::from_value(attending_labor(raw, &[&["cell-a", "cell-b"]], 500)).unwrap();
    let program = SewProgram::build(&pattern, &options).unwrap();
    let production = program.plan.production.as_ref().unwrap();
    let route = production.routing.as_ref().unwrap();
    let labor = route.attending_labor.as_ref().unwrap();
    assert_eq!(program.plan.schema, "ftx-sewing-plan/21");
    assert_eq!(production.schema, "ftx-sewing-production/9");
    assert_eq!(route.schema, "ftx-sewing-routing/4");
    assert_eq!(route.baseline.makespan_milliseconds, 16600);
    assert_eq!(route.selected.makespan_milliseconds, 15200);
    assert_eq!(labor.selected.travel_milliseconds, 500);
    assert_eq!(labor.selected.waiting_milliseconds, 0);
    assert_eq!(labor.selected.attended_milliseconds, 14700);
    assert_eq!(labor.baseline.travel_milliseconds, 1500);
    assert_eq!(labor.baseline.waiting_milliseconds, 400);
    assert_eq!(program.stitches.len(), 120);
    assert!((program.plan.total_feed_seconds() - 12.0).abs() < 1e-12);
    assert!(!labor.physical_labor_validated);
    assert_eq!(
        route
            .selected
            .operations
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["0/join", "1/join", "0/reinforce", "1/reinforce"]
    );
    assert!(route.lower_bound_makespan_milliseconds <= 15200);
    for (operation, step) in route.selected.operations.iter().zip(&labor.selected.steps) {
        assert_eq!(step.operation, operation.id);
        assert_eq!(step.copy, operation.copy);
        assert_eq!(step.cell, operation.cell);
        assert_eq!(step.worker, "worker-0");
        assert!(step.arrival_milliseconds <= step.loading_start_milliseconds);
        assert_eq!(
            step.feed_finish_milliseconds,
            operation.feed_finish_milliseconds
        );
        assert_eq!(step.finish_milliseconds, operation.finish_milliseconds);
    }
    for pair in labor.selected.steps.windows(2) {
        assert_eq!(pair[1].departure_milliseconds, pair[0].finish_milliseconds);
        assert!(pair[1].loading_start_milliseconds >= pair[0].finish_milliseconds);
    }
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap()).unwrap()
    );
}

#[test]
fn separate_attending_workers_reproduce_independent_cell_timing() {
    let (pattern, raw) = fixture(2);
    let old = build(&pattern, raw.clone()).unwrap();
    let new = build(
        &pattern,
        attending_labor(raw, &[&["cell-a"], &["cell-b"]], 0),
    )
    .unwrap();
    let old = old.production.as_ref().unwrap().routing.as_ref().unwrap();
    let new = new.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert_eq!(
        serde_json::to_value(&old.baseline).unwrap(),
        serde_json::to_value(&new.baseline).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&old.selected).unwrap(),
        serde_json::to_value(&new.selected).unwrap()
    );
    assert_eq!(old.search_nodes, new.search_nodes);
    assert_eq!(new.selected.makespan_milliseconds, 12600);
    let steps = &new.attending_labor.as_ref().unwrap().selected.steps;
    let a_second = steps
        .iter()
        .find(|step| step.operation == "1/join")
        .unwrap();
    let b_first = steps
        .iter()
        .find(|step| step.operation == "0/reinforce")
        .unwrap();
    assert!(a_second.loading_start_milliseconds < b_first.finish_milliseconds);
    assert!(b_first.loading_start_milliseconds < a_second.finish_milliseconds);
    assert_ne!(a_second.worker, b_first.worker);
}

#[test]
fn worker_shift_is_inclusive_and_search_replaces_an_unavailable_copy_major_seed() {
    let (pattern, raw) = fixture(2);
    let mut options = attending_labor(raw, &[&["cell-a", "cell-b"]], 500);
    options["production"]["routing"]["attending_labor"]["workers"][0]["available_until_milliseconds"] =
        json!(15200);
    let plan = build(&pattern, options.clone()).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert!(route.baseline_replaced);
    assert_eq!(route.selected.makespan_milliseconds, 15200);
    assert_eq!(
        serde_json::to_value(&route.baseline).unwrap(),
        serde_json::to_value(&route.selected).unwrap()
    );
    let mut short = options.clone();
    short["production"]["routing"]["attending_labor"]["workers"][0]["available_until_milliseconds"] =
        json!(15199);
    assert_eq!(
        build(&pattern, short).unwrap_err().code,
        RefusalCode::NotConverged
    );
    options["production"]["routing"]["max_search_nodes"] = json!(4);
    assert_eq!(
        build(&pattern, options).unwrap_err().code,
        RefusalCode::NotConverged
    );
}

#[test]
fn bounded_labor_search_retains_a_complete_feasible_execution() {
    let (pattern, raw) = fixture(2);
    let mut options = attending_labor(raw, &[&["cell-a", "cell-b"]], 500);
    options["production"]["routing"]["max_search_nodes"] = json!(4);
    let plan = build(&pattern, options).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert!(route.budget_exhausted && !route.search_complete && !route.optimality_proven);
    assert_eq!(route.selected.makespan_milliseconds, 16600);
    assert_eq!(route.selected.operations.len(), 4);
    assert_eq!(
        route.attending_labor.as_ref().unwrap().selected.steps.len(),
        4
    );
    assert_eq!((route.search_nodes, route.work_used), (4, 12));
}

#[test]
fn eligible_assignment_uses_actual_workers_and_relaxes_unused_late_labor() {
    let (pattern, raw) = eligible_fixture(2);
    let options: SewOptions = serde_json::from_value(attending_labor(
        raw,
        &[&["cell-a", "cell-b"], &["cell-c"]],
        500,
    ))
    .unwrap();
    let program = SewProgram::build(&pattern, &options).unwrap();
    let route = program
        .plan
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    assert_eq!(route.selected.makespan_milliseconds, 10900);
    assert_eq!(program.stitches.len(), 100);
    assert!((program.plan.total_feed_seconds() - 10.0).abs() < 1e-12);
    assert!(route.lower_bound_makespan_milliseconds <= 10900);
    let labor = &route.attending_labor.as_ref().unwrap().selected;
    assert_eq!(
        labor
            .steps
            .iter()
            .filter(|step| step.worker == "worker-0")
            .count(),
        3
    );
    assert_eq!(
        labor
            .steps
            .iter()
            .filter(|step| step.worker == "worker-1")
            .count(),
        1
    );
    let (pattern, mut raw) = eligible_fixture(1);
    raw["production"]["routing"]["cells"][0]["available_at_milliseconds"] = json!(1_000_000);
    raw["production"]["routing"]["cells"][0]["available_until_milliseconds"] = json!(1_005_300);
    refresh_locks(&mut raw);
    let mut raw = attending_labor(raw, &[&["cell-a"], &["cell-b", "cell-c"]], 500);
    raw["production"]["routing"]["attending_labor"]["workers"][0]["available_at_milliseconds"] =
        json!(1_000_000);
    let plan = build(&pattern, raw).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert_eq!(route.selected.makespan_milliseconds, 7000);
    assert!(route.lower_bound_makespan_milliseconds <= 7000);
    assert!(
        route
            .attending_labor
            .as_ref()
            .unwrap()
            .selected
            .steps
            .iter()
            .all(|step| step.worker == "worker-1")
    );
}

#[test]
fn worker_arrival_travel_waiting_and_full_unloading_compose_with_fixtures() {
    let (pattern, raw) = fixture(2);
    let mut options = attending_labor(shared_fixtures(raw, 1), &[&["cell-a"], &["cell-b"]], 0);
    let b = &mut options["production"]["routing"]["attending_labor"]["workers"][1];
    b["available_at_milliseconds"] = json!(2000);
    b["initial_arrival_milliseconds"]["cell-b"] = json!(1000);
    let plan = build(&pattern, options).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    let labor = route.attending_labor.as_ref().unwrap();
    let holding = route.workholding.as_ref().unwrap();
    assert_eq!(route.selected.makespan_milliseconds, 14700);
    assert_eq!(holding.selected.shared_peak_occupancy["common-jigs"], 1);
    let first_b = labor
        .selected
        .steps
        .iter()
        .find(|row| row.cell == "cell-b")
        .unwrap();
    assert_eq!(first_b.departure_milliseconds, 2000);
    assert_eq!(first_b.arrival_milliseconds, 3000);
    assert!(first_b.loading_start_milliseconds > first_b.arrival_milliseconds);
    assert_eq!(labor.selected.travel_milliseconds, 1000);
    for hold in &holding.selected.holds {
        let step = labor
            .selected
            .steps
            .iter()
            .find(|step| step.operation == hold.operation)
            .unwrap();
        assert_eq!(
            hold.acquired_at_milliseconds,
            step.loading_start_milliseconds
        );
        assert_eq!(hold.released_at_milliseconds, step.finish_milliseconds);
    }
}

#[test]
fn routed_labor_requires_complete_unique_qualified_ownership_and_exact_sources() {
    let (pattern, raw) = fixture(2);
    let raw = attending_labor(raw, &[&["cell-a", "cell-b"]], 500);
    let mut mutations: Vec<Value> = Vec::new();
    for (path, value) in [
        (
            "/production/routing/attending_labor/model",
            json!("autonomous_feed"),
        ),
        (
            "/production/routing/attending_labor/allow_estimates",
            json!(false),
        ),
        ("/production/routing/attending_labor/method", json!("")),
        (
            "/production/routing/attending_labor/cells/cell-a",
            json!("0".repeat(64)),
        ),
        (
            "/production/routing/attending_labor/workers/0/id",
            json!("handoff-worker"),
        ),
        (
            "/production/routing/attending_labor/workers/0/cells",
            json!(["cell-a"]),
        ),
        (
            "/production/routing/attending_labor/workers/0/cells",
            json!(["cell-a", "cell-a"]),
        ),
        (
            "/production/routing/attending_labor/workers/0/initial_arrival_milliseconds",
            json!({"cell-a":0}),
        ),
        (
            "/production/routing/attending_labor/workers/0/transfers",
            json!([]),
        ),
        (
            "/production/routing/attending_labor/workers/0/transfers/0/to",
            json!("cell-a"),
        ),
        (
            "/production/routing/attending_labor/workers/0/transfers/0/milliseconds",
            json!(86_400_001),
        ),
        (
            "/production/routing/attending_labor/workers/0/available_at_milliseconds",
            json!(86_400_001),
        ),
        (
            "/production/routing/independence",
            json!("independent_attended_cells_and_transfer_resource/1"),
        ),
    ] {
        let mut changed = raw.clone();
        *changed.pointer_mut(path).unwrap() = value;
        mutations.push(changed);
    }
    let mut duplicate = attending_labor(raw.clone(), &[&["cell-a"], &["cell-b"]], 0);
    duplicate["production"]["routing"]["attending_labor"]["workers"][1]["id"] = json!("worker-0");
    mutations.push(duplicate);
    let mut expired = raw.clone();
    expired["production"]["routing"]["attending_labor"]["workers"][0]["available_at_milliseconds"] =
        json!(20);
    expired["production"]["routing"]["attending_labor"]["workers"][0]["available_until_milliseconds"] =
        json!(19);
    mutations.push(expired);
    let mut stale = raw;
    stale["production"]["routing"]["cells"][0]["handling"]["initial_milliseconds"]["join"] =
        json!(101);
    refresh_cell_locks(&mut stale);
    mutations.push(stale);
    for (index, invalid) in mutations.into_iter().enumerate() {
        assert!(
            build(&pattern, invalid).is_err(),
            "mutation {index} must refuse"
        );
    }
}

#[test]
fn one_transfer_resource_serializes_loaded_handoffs_without_blocking_finished_cells() {
    let (pattern, mut raw) = fixture(3);
    raw["production"]["routing"]["transfer"]["transfers"][0]["milliseconds"] = json!(10000);
    let plan = build(&pattern, raw).unwrap();
    let routed = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert_eq!(routed.selected.makespan_milliseconds, 37600);
    assert_eq!(routed.selected.transfer_milliseconds, 30000);
    assert_eq!(routed.selected.transfers.len(), 3);
    for (index, transfer) in routed.selected.transfers.iter().enumerate() {
        assert_eq!(transfer.start_milliseconds, 5300 + index as u64 * 10000);
        assert_eq!(transfer.finish_milliseconds, 15300 + index as u64 * 10000);
    }
    let a_last = routed
        .selected
        .operations
        .iter()
        .filter(|row| row.cell == "cell-a")
        .map(|row| row.finish_milliseconds)
        .max()
        .unwrap();
    assert_eq!(a_last, 13900);
    // The two other completed garments wait in their independently admitted staging slots.
    assert!(a_last < routed.selected.transfers[0].finish_milliseconds);
}

#[test]
fn routed_preview_uses_the_requested_specialist_and_refuses_wrong_cell_operations() {
    let (pattern, raw) = fixture(2);
    let options: SewOptions = serde_json::from_value(raw).unwrap();
    let a = ftx_export::sewing::preview::svg_cell(&pattern, &options, "cell-a", "join").unwrap();
    let b =
        ftx_export::sewing::preview::svg_cell(&pattern, &options, "cell-b", "reinforce").unwrap();
    assert_eq!(a.matches("<circle class=\"needle\"").count(), 82);
    assert_eq!(b.matches("<circle class=\"needle\"").count(), 42);
    assert!(ftx_export::sewing::preview::svg_cell(&pattern, &options, "cell-b", "join").is_err());
    assert!(ftx_export::sewing::preview::svg(&pattern, &options, "join").is_err());
}

fn reentrant() -> (Pattern, Value) {
    let (pattern, mut raw) = fixture(2);
    let mut pass = raw["passes"][0].clone();
    pass["id"] = json!("finish");
    pass["thread"] = json!("finish-thread");
    pass["after"] = json!(["reinforce"]);
    raw["passes"].as_array_mut().unwrap().push(pass);
    let a = &mut raw["production"]["routing"]["cells"][0];
    let mut q = a["configuration"]["qualifications"][0].clone();
    q["id"] = json!("qualified-finish");
    q["thread"] = json!("finish-thread");
    a["configuration"]["qualifications"]
        .as_array_mut()
        .unwrap()
        .push(q);
    let mut recipe = a["configuration"]["recipes"]["join"].clone();
    recipe["id"] = json!("recipe-finish");
    recipe["thread"] = json!("finish-thread");
    a["configuration"]["recipes"]["finish"] = recipe;
    a["handling"]["initial_milliseconds"]["finish"] = json!(100);
    a["handling"]["final_milliseconds"]["finish"] = json!(200);
    let times = a["ordering"]["setup_times"]["transitions"]
        .as_array_mut()
        .unwrap();
    for (from, to, milliseconds) in [
        ("idle", "finish-thread", 1000),
        ("thread-cell-a", "finish-thread", 5000),
        ("finish-thread", "thread-cell-a", 5000),
    ] {
        times.push(json!({"from":{"needle":"needle-cell-a","thread":from},
            "to":{"needle":"needle-cell-a","thread":to},"milliseconds":milliseconds}));
    }
    raw["production"]["routing"]["route"]
        .as_array_mut()
        .unwrap()
        .push(json!({"operation":"finish","cell":"cell-a"}));
    raw["production"]["routing"]["transfer"]["transfers"]
        .as_array_mut()
        .unwrap()
        .push(json!({"from":"cell-b","to":"cell-a","milliseconds":700}));
    refresh_locks(&mut raw);
    (pattern, raw)
}

#[test]
fn reentrant_route_batches_machine_setups_and_retains_a_complete_bounded_incumbent() {
    let (pattern, raw) = reentrant();
    let plan = build(&pattern, raw.clone()).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    // Cell A alone needs 16 s feed, 1.2 s loading/unloading, its 1 s initial setup and one 5 s change.
    // A feasible pipeline reaches that independent machine-work lower bound exactly.
    assert_eq!(route.selected.makespan_milliseconds, 23200);
    assert_eq!(route.baseline.makespan_milliseconds, 41100);
    assert_eq!(route.selected.operations.len(), 6);
    assert_eq!(route.selected.transfers.len(), 4);
    assert_eq!(plan.total_stitch_intervals(), 200);
    let mut capped = raw;
    capped["production"]["routing"]["max_search_nodes"] = json!(6);
    let capped = build(&pattern, capped).unwrap();
    let retained = capped
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    assert_eq!(retained.selected.operations.len(), 6);
    assert_eq!(retained.selected.makespan_milliseconds, 41100);
    assert!(retained.search_nodes <= 6);
    assert!(retained.budget_exhausted && !retained.search_complete && !retained.optimality_proven);
}

#[test]
fn split_seam_coverage_is_admitted_across_specialists_and_checked_on_the_unchanged_pattern() {
    let (pattern, mut raw) = fixture(2);
    raw["passes"][0]["source_range"] = json!([0.0, 0.5]);
    raw["passes"][1]["source_range"] = json!([0.5, 1.0]);
    refresh_locks(&mut raw);
    let plan = build(&pattern, raw.clone()).unwrap();
    assert_eq!(plan.total_stitch_intervals(), 60);
    assert!((plan.total_feed_seconds() - 6.0).abs() < 1e-12);
    raw["passes"][1]["source_range"] = json!([0.6, 1.0]);
    refresh_locks(&mut raw);
    assert!(build(&pattern, raw).is_err());
}

#[test]
fn search_replaces_an_out_of_window_baseline_with_a_complete_feasible_route() {
    let (pattern, mut raw) = reentrant();
    raw["production"]["routing"]["cells"][0]["available_until_milliseconds"] = json!(23200);
    refresh_locks(&mut raw);
    let plan = build(&pattern, raw).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert!(route.baseline_replaced && route.optimality_proven);
    assert_eq!(route.selected.makespan_milliseconds, 23200);
    assert_eq!(route.selected.operations.len(), 6);
    assert_eq!(route.selected.transfers.len(), 4);
    assert_eq!(
        serde_json::to_value(&route.baseline).unwrap(),
        serde_json::to_value(&route.selected).unwrap()
    );
}

#[test]
fn inclusive_cell_and_transfer_windows_cover_unloading_and_complete_handoffs() {
    let (pattern, mut raw) = fixture(2);
    raw["production"]["routing"]["cells"][0]["available_until_milliseconds"] = json!(9600);
    raw["production"]["routing"]["cells"][1]["available_until_milliseconds"] = json!(12600);
    raw["production"]["routing"]["transfer"]["available_until_milliseconds"] = json!(10300);
    refresh_locks(&mut raw);
    assert!(build(&pattern, raw.clone()).is_ok());
    for (kind, index, end) in [
        ("cell", 0, 9599),
        ("cell", 1, 12599),
        ("transfer", 0, 10299),
    ] {
        let mut refused = raw.clone();
        if kind == "cell" {
            refused["production"]["routing"]["cells"][index]["available_until_milliseconds"] =
                json!(end);
            refresh_locks(&mut refused);
        } else {
            refused["production"]["routing"]["transfer"]["available_until_milliseconds"] =
                json!(end);
        }
        let error = build(&pattern, refused).unwrap_err();
        assert_eq!(
            error.code,
            RefusalCode::NotConverged,
            "{kind} {index}: {error:?}"
        );
    }
}

#[test]
fn routing_refuses_stale_sources_missing_semantics_and_unqualified_machine_substitution() {
    let (pattern, raw) = fixture(2);
    let mut cases = Vec::new();
    let mut bad = raw.clone();
    bad["production"]["routing"]["cell_locks"]["cell-a"] = json!("00".repeat(32));
    cases.push(bad);
    let mut bad = raw.clone();
    bad["production"]["routing"]["cells"][1]["configuration"]["qualifications"][0]["source"]["note"] =
        json!("Changed source under the same ID");
    cases.push(bad);
    let mut bad = raw.clone();
    bad["production"]["routing"]["allow_estimates"] = json!(false);
    cases.push(bad);
    let mut bad = raw.clone();
    bad["production"]["routing"]["transfer"]["allow_estimates"] = json!(false);
    cases.push(bad);
    let mut bad = raw.clone();
    bad["production"]["routing"]["staging_capacity"] = json!(1);
    cases.push(bad);
    let mut bad = raw.clone();
    bad["production"]["routing"]["route"]
        .as_array_mut()
        .unwrap()
        .pop();
    cases.push(bad);
    let mut bad = raw.clone();
    bad["production"]["routing"]["route"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    cases.push(bad);
    let mut bad = raw.clone();
    bad["production"]["routing"]["transfer"]["transfers"] = json!([]);
    cases.push(bad);
    let mut bad = raw.clone();
    bad["production"]["routing"]["cells"][1]["configuration"]["recipes"]["reinforce"]["max_stitch_mm"] =
        json!(2.5);
    refresh_locks(&mut bad);
    cases.push(bad);
    let mut bad = raw;
    bad["passes"][0]["after"] = json!(["reinforce"]);
    refresh_locks(&mut bad);
    cases.push(bad);
    for (index, bad) in cases.into_iter().enumerate() {
        assert!(
            build(&pattern, bad).is_err(),
            "invalid route case {index} admitted"
        );
    }
}

#[test]
fn routed_operations_contribute_only_actual_specialist_yarn_before_a_shared_purchase() {
    let (pattern, mut raw) = fixture(2);
    let yarn = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: "coats/epic-2994120-2026".into(),
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection();
    for (index, cell) in raw["production"]["routing"]["cells"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        cell["configuration"]["qualifications"][0]["thread_consumption"] = json!({
            "channels_complete":true,"evidence":"estimate","allow_estimates":true,
            "source":source("thread"),"method":"Synthetic complete channel consumption with per-operation allowances",
            "channels":[{"id":"all-required-thread","material":yarn,
                "metres_per_traversal":([0.01,0.02][index]),"start_allowance_m":([0.1,0.05][index]),
                "end_allowance_m":([0.2,0.05][index])}]
        });
    }
    raw["consumables"] = json!({"purchasing":{
        "quote_book":{"schema":"ftx-textile-quotes/1","quotes":[{
            "offer_id":"routed-yarn-offer","material_id":yarn.id,"material_sha256":yarn.expected_sha256,
            "supplier":"synthetic-supplier","sku":"shared-yarn","price":10.0,"quantity":2.0,"unit":"linear_meter",
            "minimum_quantity":2.0,"order_increment":2.0,"price_breaks":[{"minimum_quantity":4.0,"price":7.0}],
            "currency":"USD","region":"test","quality":"new","terms":"synthetic merchandise only",
            "observed_on":"2026-10-27","available":true,"available_quantity":4.0,"maximum_continuous_quantity":2.0,
            "source":"test:shared-routed-yarn-offer","source_sha256":"ab".repeat(32)
        }]},
        "market":{"currency":"USD","region":"test","quality":"new","terms":"synthetic merchandise only",
            "as_of":"2026-10-27","max_age_days":2,"require_observed_quantity":true,"require_observed_continuous_length":true}
    }});
    refresh_locks(&mut raw);
    let plan = build(&pattern, raw).unwrap();
    let report = plan.consumables.as_ref().unwrap();
    assert!(report.complete_consumption_coverage && report.complete_mass_coverage);
    assert_eq!(report.operation_channels.len(), 2);
    assert_eq!(report.material_requirements.len(), 1);
    assert!((report.total_required_length_m.unwrap() - 2.4).abs() < 1e-12);
    let contribution: BTreeMap<_, _> = report
        .operation_channels
        .iter()
        .map(|row| (row.cell.as_deref().unwrap(), row))
        .collect();
    assert_eq!(contribution["cell-a"].stitch_intervals, 80);
    assert_eq!(contribution["cell-b"].stitch_intervals, 40);
    assert!((contribution["cell-a"].required_length_m - 1.4).abs() < 1e-12);
    assert!((contribution["cell-b"].required_length_m - 1.0).abs() < 1e-12);
    let orders = report.purchasing.as_ref().unwrap();
    assert_eq!(orders.material_orders.len(), 1);
    assert_eq!(orders.least_observed_independent_order_total, Some(14.0));
    assert_eq!(
        orders.material_orders[0]
            .least_observed_order
            .as_ref()
            .unwrap()
            .order_quantity,
        4.0
    );
}

#[test]
fn routed_recipe_selection_uses_only_each_specialists_setup_domain() {
    use ftx_export::sewing_selection::Request;

    let (pattern, options) = fixture(2);
    let authored = options["passes"][1].clone();
    // The listed choice retains the authored pass. The specialist's fixed override remains the
    // actual process, so its setup must never be demanded from the other cell.
    let mut request = json!({"schema":"ftx-sewing-selection/5","options":options,
        "objective":"minimize_parallel_makespan","max_plan_evaluations":2,
        "alternatives":[{"operation":"reinforce","recipes":[{"id":"same-authored-pass",
            "needle":authored["needle"],"thread":authored["thread"],
            "max_stitch_mm":authored["max_stitch_mm"],
            "stitches_per_minute":authored["stitches_per_minute"]}]}]});
    let result = serde_json::from_value::<Request>(request.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(result.trials.len(), 2);
    assert!(result.trials.iter().all(|trial| trial.refusal.is_none()));
    assert_eq!(result.selected_objective_seconds, Some(12.6));
    assert_eq!(
        result.selected_choices.as_ref().unwrap()["reinforce"],
        "template"
    );
    let plan = result.selected_plan.as_ref().unwrap();
    assert_eq!(plan.total_stitch_intervals(), 120);
    assert!(plan.ordering_report().is_none());
    assert_eq!(
        serde_json::to_value(plan).unwrap(),
        serde_json::to_value(SewPlan::build(&pattern, &plan.options).unwrap()).unwrap()
    );
    request["schema"] = json!("ftx-sewing-selection/1");
    request["objective"] = json!("minimize_feed_seconds");
    let feed = serde_json::from_value::<Request>(request.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(feed.trials[0].feed_seconds, Some(12.0));
    assert_eq!(feed.selected_plan.unwrap().total_stitch_intervals(), 120);
    for (schema, objective) in [
        ("ftx-sewing-selection/2", "minimize_feed_and_setup_seconds"),
        (
            "ftx-sewing-selection/4",
            "minimize_modeled_production_seconds",
        ),
    ] {
        request["schema"] = json!(schema);
        request["objective"] = json!(objective);
        let refusal = serde_json::from_value::<Request>(request.clone())
            .unwrap()
            .select(&pattern)
            .unwrap_err();
        assert!(
            refusal.message.contains("serial time objectives"),
            "{refusal}"
        );
    }
}

fn route_selection() -> (Pattern, Value) {
    let (pattern, mut original) = fixture(2);
    original["production"]["routing"]["cells"][1]["available_at_milliseconds"] = json!(18000);
    refresh_locks(&mut original);
    let mut alternative = original.clone();
    let route = &mut alternative["production"]["routing"];
    route["cells"][1]["id"] = json!("cell-c");
    route["cells"][1]["available_at_milliseconds"] = json!(0);
    route["cells"][1]["configuration"]["recipes"]["reinforce"]["max_stitch_mm"] = json!(4.0);
    route["route"][1]["cell"] = json!("cell-c");
    route["transfer"]["transfers"][0]["to"] = json!("cell-c");
    route["source"] = source("available-specialists");
    refresh_locks(&mut alternative);
    let request = json!({"schema":"ftx-sewing-selection/6","options":original,
        "objective":"minimize_parallel_makespan","max_plan_evaluations":2,
        "route_candidates":[{"id":"available-specialists","routing":alternative["production"]["routing"]}]});
    (pattern, request)
}

#[test]
fn route_selection_compares_actual_complete_schedules_and_replays_winning_geometry() {
    use ftx_export::sewing_selection::Request;

    let (pattern, raw) = route_selection();
    let request: Request = serde_json::from_value(raw).unwrap();
    let original = serde_json::to_value(&request).unwrap();
    let result = request.select(&pattern).unwrap();
    assert_eq!(result.schema, "ftx-sewing-selection-report/6");
    assert_eq!(result.candidate_combinations, 2);
    assert_eq!(result.trials[0].route_choice.as_deref(), Some("template"));
    assert_eq!(result.trials[0].objective_seconds, Some(23.1));
    assert_eq!(result.trials[0].feed_seconds, Some(12.0));
    assert_eq!(result.trials[1].objective_seconds, Some(13.1));
    assert_eq!(result.trials[1].feed_seconds, Some(13.0));
    assert_eq!(
        result.selected_route.as_deref(),
        Some("available-specialists")
    );
    assert!(result.search_complete && !result.budget_exhausted);
    assert_eq!(result.optimality_proven, result.unresolved_trials == 0);
    let plan = result.selected_plan.as_ref().unwrap();
    assert_eq!(plan.total_stitch_intervals(), 130);
    assert_eq!(plan.options.passes.len(), 2);
    assert_eq!(
        plan.options.production.as_ref().unwrap().production_copies,
        Some(2)
    );
    assert_eq!(
        serde_json::to_value(SewPlan::build(&pattern, &plan.options).unwrap()).unwrap(),
        serde_json::to_value(plan).unwrap()
    );
    let stitches = SewProgram::build(&pattern, &plan.options).unwrap();
    assert_eq!(stitches.stitches.len(), 130);
    assert!(
        stitches
            .stitches
            .iter()
            .all(|stitch| { matches!(stitch.cell.as_deref(), Some("cell-a" | "cell-c")) })
    );
    assert!(!plan.hardware_execution_authorized && !plan.physical_seam_quality_validated);
    assert_eq!(serde_json::to_value(&request).unwrap(), original);
}

#[test]
fn route_and_recipe_search_share_one_cap_and_preserve_complete_incumbents() {
    use ftx_export::sewing_selection::Request;

    let (pattern, mut raw) = route_selection();
    raw["max_plan_evaluations"] = json!(1);
    let bounded = serde_json::from_value::<Request>(raw.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(bounded.selected_route.as_deref(), Some("template"));
    assert_eq!(bounded.selected_objective_seconds, Some(23.1));
    assert!(bounded.budget_exhausted && !bounded.optimality_proven);
    let pass = raw["options"]["passes"][1].clone();
    raw["alternatives"] = json!([{"operation":"reinforce","recipes":[{
        "id":"authored-again","needle":pass["needle"],"thread":pass["thread"],
        "max_stitch_mm":pass["max_stitch_mm"],"stitches_per_minute":pass["stitches_per_minute"]}]}]);
    raw["max_plan_evaluations"] = json!(3);
    let bounded = serde_json::from_value::<Request>(raw.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(bounded.candidate_combinations, 4);
    assert_eq!(bounded.trials.len(), 3);
    assert_eq!(bounded.trials[0].route_choice.as_deref(), Some("template"));
    assert_eq!(bounded.trials[1].route_choice.as_deref(), Some("template"));
    assert_eq!(bounded.trials[1].choices["reinforce"], "authored-again");
    assert_eq!(
        bounded.trials[2].route_choice.as_deref(),
        Some("available-specialists")
    );
    assert_eq!(
        bounded.selected_route.as_deref(),
        Some("available-specialists")
    );
    assert_eq!(
        bounded.selected_choices.as_ref().unwrap()["reinforce"],
        "template"
    );
    assert!(bounded.budget_exhausted && !bounded.search_complete && !bounded.optimality_proven);
    raw["max_plan_evaluations"] = json!(4);
    let complete = serde_json::from_value::<Request>(raw)
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert!(complete.search_complete && !complete.budget_exhausted);
    assert_eq!(complete.selected_route, bounded.selected_route);
    assert_eq!(
        serde_json::to_value(complete.selected_plan).unwrap(),
        serde_json::to_value(bounded.selected_plan).unwrap()
    );
}

#[test]
fn route_selection_retains_stale_unresolved_and_equal_candidates_in_canonical_order() {
    use ftx_export::sewing_selection::Request;

    let (pattern, mut raw) = route_selection();
    let mut equal = raw["route_candidates"][0].clone();
    equal["id"] = json!("z-equal");
    let mut stale = equal.clone();
    stale["id"] = json!("stale-lock");
    stale["routing"]["cell_locks"]["cell-c"] = json!("00".repeat(32));
    let mut unresolved = equal.clone();
    unresolved["id"] = json!("unresolved-window");
    unresolved["routing"]["transfer"]["available_until_milliseconds"] = json!(0);
    raw["route_candidates"]
        .as_array_mut()
        .unwrap()
        .extend([equal, unresolved, stale]);
    raw["max_plan_evaluations"] = json!(5);
    let result = serde_json::from_value::<Request>(raw.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    let ids: Vec<_> = result
        .trials
        .iter()
        .map(|trial| trial.route_choice.as_deref().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "template",
            "available-specialists",
            "stale-lock",
            "unresolved-window",
            "z-equal"
        ]
    );
    assert!(
        result.trials[2]
            .refusal
            .as_ref()
            .unwrap()
            .message
            .contains("lock")
    );
    assert_eq!(
        result.trials[3].refusal.as_ref().unwrap().code,
        RefusalCode::NotConverged
    );
    assert_eq!(
        result.selected_route.as_deref(),
        Some("available-specialists")
    );
    assert!(result.unresolved_trials > 0 && !result.optimality_proven && result.search_complete);
    raw["route_candidates"].as_array_mut().unwrap().reverse();
    let reordered = serde_json::from_value::<Request>(raw)
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(
        serde_json::to_value(result.trials).unwrap(),
        serde_json::to_value(reordered.trials).unwrap()
    );
    assert_eq!(result.selected_route, reordered.selected_route);
}

#[test]
fn unavailable_route_evidence_retains_other_routes_without_claiming_an_optimum() {
    use ftx_export::sewing_selection::Request;

    let (pattern, mut raw) = route_selection();
    raw["route_candidates"][0]["routing"]["cells"][1]["ordering"]["setup_times"]["transitions"] =
        json!([]);
    let result = serde_json::from_value::<Request>(raw.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(result.selected_route.as_deref(), Some("template"));
    assert_eq!(result.trials.len(), 2);
    assert!(result.trials[1].refusal.is_some());
    assert!(result.trials[1].objective_unavailable.is_some());
    assert!(result.trials[1].objective_seconds.is_none());
    assert!(result.search_complete && !result.optimality_proven && result.unresolved_trials > 0);

    let (pattern, mut raw) = route_selection();
    raw["options"]["production"]["routing"]["cells"][1]["ordering"]["setup_times"] = Value::Null;
    let result = serde_json::from_value::<Request>(raw)
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(
        result.selected_route.as_deref(),
        Some("available-specialists")
    );
    assert!(result.trials[0].objective_unavailable.is_some());
    assert!(!result.optimality_proven);

    let (pattern, mut raw) = route_selection();
    let mut unqualified = raw["options"].clone();
    unqualified["production"]["routing"] = raw["route_candidates"][0]["routing"].clone();
    unqualified["production"]["routing"]["cells"][1]["configuration"]["qualifications"][0]["materials"]
        [0] = json!("00".repeat(32));
    refresh_locks(&mut unqualified);
    raw["route_candidates"][0]["routing"] = unqualified["production"]["routing"].clone();
    let result = serde_json::from_value::<Request>(raw)
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(result.selected_route.as_deref(), Some("template"));
    assert!(
        result.trials[1]
            .refusal
            .as_ref()
            .unwrap()
            .message
            .contains("exact stack")
    );
}

#[test]
fn route_selection_requires_opt_in_source_identity_and_reserves_the_largest_candidate() {
    use ftx_export::sewing_selection::Request;

    let (pattern, raw) = route_selection();
    let mut wrong_schema = raw.clone();
    wrong_schema["schema"] = json!("ftx-sewing-selection/5");
    assert!(
        serde_json::from_value::<Request>(wrong_schema)
            .unwrap()
            .select(&pattern)
            .is_err()
    );
    for id in ["", "template"] {
        let mut invalid = raw.clone();
        invalid["route_candidates"][0]["id"] = json!(id);
        assert!(
            serde_json::from_value::<Request>(invalid)
                .unwrap()
                .select(&pattern)
                .is_err()
        );
    }
    let mut duplicate = raw.clone();
    let candidate = duplicate["route_candidates"][0].clone();
    duplicate["route_candidates"]
        .as_array_mut()
        .unwrap()
        .push(candidate);
    assert!(
        serde_json::from_value::<Request>(duplicate)
            .unwrap()
            .select(&pattern)
            .is_err()
    );
    let mut expensive = raw.clone();
    expensive["route_candidates"][0]["routing"]["max_search_nodes"] = json!(100000);
    let request: Request = serde_json::from_value(expensive).unwrap();
    assert_eq!(
        request.select(&pattern).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    let mut study_candidate = raw.clone();
    study_candidate["route_candidates"][0]["routing"]["max_search_nodes"] = json!(80000);
    assert!(
        serde_json::from_value::<Request>(study_candidate.clone())
            .unwrap()
            .scheduling_work_bound()
            .is_ok()
    );
    let binding =
        json!({"schema":"ftx-material-bindings/1","default":pattern.raw()["fabric"]["material"]});
    let study: ftx_export::material_study::Request = serde_json::from_value(json!({
        "schema":"ftx-material-study/1",
        "variants":[{"key":"first","changes":binding},{"key":"second","changes":binding}],
        "sewing_selection":study_candidate
    }))
    .unwrap();
    assert_eq!(
        study.evaluate(&pattern).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    let request: Request = serde_json::from_value(raw).unwrap();
    let mut changed = pattern.raw().clone();
    changed["fabric"]["material"]["record"]["name"] = json!("Different exact specimen");
    let result = request.select(&Pattern::new(changed)).unwrap();
    assert!(result.selected_plan.is_none() && result.selected_route.is_none());
    assert!(result.trials.iter().all(|trial| trial.refusal.is_some()));
    assert!(!result.optimality_proven);
}

#[test]
fn route_selection_study_reserves_complete_replays_for_inherited_cutting_quantity() {
    use ftx_export::sewing_selection::Request;

    let (pattern, mut selection) = route_selection();
    selection["options"]["production"]["production_copies"] = Value::Null;
    selection["options"]["production"]["routing"]["staging_capacity"] = json!(4);
    selection["route_candidates"][0]["routing"]["staging_capacity"] = json!(4);
    selection["route_candidates"][0]["routing"]["max_search_nodes"] = json!(49_990);
    // At one copy the two-route bound fits twice; four copies add replay work to every plan.
    assert_eq!(
        serde_json::from_value::<Request>(selection.clone())
            .unwrap()
            .scheduling_work_bound()
            .unwrap(),
        99_988
    );
    let binding =
        json!({"schema":"ftx-material-bindings/1","default":pattern.raw()["fabric"]["material"]});
    for production_copies in [Value::Null, json!(4)] {
        selection["options"]["production"]["production_copies"] = production_copies;
        let study: ftx_export::material_study::Request = serde_json::from_value(json!({
            "schema":"ftx-material-study/1",
            "variants":[{"key":"first","changes":binding},{"key":"second","changes":binding}],
            "cutting":{"copies":4,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0},
            "sewing_selection":selection
        }))
        .unwrap();
        let refusal = study.evaluate(&pattern).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::BudgetExceeded);
        assert!(refusal.message.contains("aggregate recipe/ordering"));
    }
}

#[test]
fn routed_study_compares_actual_specialist_sources_and_preserves_independent_refusals() {
    use ftx_export::material_study::Request;

    let (pattern, mut options) = fixture(2);
    let yarn = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: "coats/epic-2994120-2026".into(),
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection();
    for cell in options["production"]["routing"]["cells"]
        .as_array_mut()
        .unwrap()
    {
        cell["configuration"]["qualifications"][0]["thread_consumption"] = json!({
            "channels_complete":true,"evidence":"estimate","allow_estimates":true,
            "source":source("study-thread"),"method":"Synthetic total thread per actual specialist stitch",
            "channels":[{"id":"complete-thread","material":yarn,"metres_per_traversal":0.01,
                "start_allowance_m":0.0,"end_allowance_m":0.0}]
        });
    }
    options["consumables"] = json!({});
    refresh_locks(&mut options);
    let mut input = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"routed","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern.raw()["fabric"]["material"]}}],"sewing":options,
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[]},
        "comparison":{"basis":"nominal","required_consumers":["sewing","sewing_consumables"],
            "objectives":[{"metric":"sewing_parallel_makespan_seconds","direction":"minimize"},
                {"metric":"thread_required_length_m","direction":"minimize"}]}});
    let result = serde_json::to_value(
        serde_json::from_value::<Request>(input.clone())
            .unwrap()
            .evaluate(&pattern)
            .unwrap(),
    )
    .unwrap();
    let row = &result["alternatives"][0];
    assert_eq!(row["sewing"]["status"], "available");
    assert_eq!(row["metrics"]["stitch_intervals"], 120);
    assert_eq!(row["metrics"]["sewing_parallel_makespan_seconds"], 12.6);
    assert!((row["metrics"]["thread_required_length_m"].as_f64().unwrap() - 1.2).abs() < 1e-12);
    assert!(row["metrics"]["sewing_setup_seconds"].is_null());
    assert!(row["metrics"]["sewing_modeled_production_seconds"].is_null());
    assert_eq!(result["comparison"]["frontier"], json!(["routed"]));
    let bound =
        Pattern::from_json(row["binding"]["result"]["pattern_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        row["sewing"]["result"],
        serde_json::to_value(build(&bound, input["sewing"].clone()).unwrap()).unwrap()
    );
    input["sewing"]["production"]["routing"]["transfer"]["available_until_milliseconds"] = json!(0);
    let refused = serde_json::to_value(
        serde_json::from_value::<Request>(input)
            .unwrap()
            .evaluate(&pattern)
            .unwrap(),
    )
    .unwrap();
    let failed = &refused["alternatives"][0];
    assert_eq!(failed["sewing"]["status"], "refused");
    assert_eq!(failed["assembly_mass"]["status"], "available");
    assert!(failed["metrics"]["sewing_parallel_makespan_seconds"].is_null());
    assert_eq!(refused["comparison"]["frontier"], json!([]));
}

#[test]
fn shared_fixture_capacity_controls_routed_completion_and_exact_stitches() {
    let (pattern, raw) = fixture(2);
    for (capacity, finish, peak) in [(1, 14700, 1), (2, 12600, 2)] {
        let options: SewOptions =
            serde_json::from_value(shared_fixtures(raw.clone(), capacity)).unwrap();
        let program = SewProgram::build(&pattern, &options).unwrap();
        let production = program.plan.production.as_ref().unwrap();
        let route = production.routing.as_ref().unwrap();
        let holding = route.workholding.as_ref().unwrap();
        assert_eq!(program.plan.schema, "ftx-sewing-plan/20");
        assert_eq!(production.schema, "ftx-sewing-production/8");
        assert_eq!(route.schema, "ftx-sewing-routing/3");
        assert_eq!(route.selected.makespan_milliseconds, finish);
        assert_eq!(program.stitches.len(), 120);
        assert_eq!(program.plan.total_feed_seconds(), 12.0);
        assert_eq!(holding.selected.holds.len(), 4);
        assert_eq!(holding.selected.shared_peak_occupancy["common-jigs"], peak);
        assert_eq!(holding.selected.shared_final_occupancy["common-jigs"], 0);
        assert!(
            !holding.physical_pool_access_validated
                && !holding.fixture_geometry_and_access_validated
        );
        for cell in ["cell-a", "cell-b"] {
            assert_eq!(holding.selected.local_peak_occupancy[cell]["jig"], 1);
            assert_eq!(holding.selected.local_final_occupancy[cell]["jig"], 0);
        }
        for hold in &holding.selected.holds {
            let operation = route
                .selected
                .operations
                .iter()
                .find(|operation| operation.id == hold.operation)
                .unwrap();
            assert_eq!(
                hold.acquired_at_milliseconds,
                operation.loading_start_milliseconds
            );
            assert_eq!(hold.released_at_milliseconds, operation.finish_milliseconds);
            assert_eq!(hold.copy, operation.copy);
            assert_eq!(hold.cell, operation.cell);
        }
        if capacity == 1 {
            // Independent capacity-one verification, not the scheduler's reservation sweep.
            let mut intervals: Vec<_> = holding
                .selected
                .holds
                .iter()
                .map(|hold| (hold.acquired_at_milliseconds, hold.released_at_milliseconds))
                .collect();
            intervals.sort_unstable();
            assert!(intervals.windows(2).all(|pair| pair[0].1 <= pair[1].0));
            assert_eq!(route.baseline.makespan_milliseconds, 16100);
            assert!(route.search_complete && !route.optimality_proven);
        }
        assert_eq!(
            serde_json::to_value(&program).unwrap(),
            serde_json::to_value(SewProgram::build(&pattern, &program.plan.options).unwrap())
                .unwrap()
        );
        if capacity == 1 {
            capture_routed_fixture("sewing-pattern.json", pattern.raw());
            capture_routed_fixture("sewing-options.json", &options);
            capture_routed_fixture("sewing-program.json", &program);
        }
    }
}

#[test]
fn routed_fixtures_cover_future_reservations_and_complete_unloading() {
    let (pattern, raw) = fixture(2);
    let mut options = shared_fixtures(raw, 1);
    options["production"]["routing"]["max_search_nodes"] = json!(4);
    let plan = build(&pattern, options).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    let rows: BTreeMap<_, _> = route
        .selected
        .operations
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect();
    // Dispatch constructs B0 before A1 even though A1's unpooled earliest start is only 5300ms.
    // The future B0 reservation [6000,8800) excludes that full 4300ms A1 interval.
    assert_eq!(rows["0/reinforce"].loading_start_milliseconds, 6000);
    assert_eq!(rows["0/reinforce"].feed_finish_milliseconds, 8600);
    assert_eq!(rows["0/reinforce"].finish_milliseconds, 8800);
    assert_eq!(rows["1/join"].loading_start_milliseconds, 8800);
    assert_eq!(rows["1/join"].finish_milliseconds, 13100);
    assert_eq!(route.selected.makespan_milliseconds, 16100);
    assert!(route.budget_exhausted && !route.optimality_proven);
    assert_eq!(route.search_nodes, 4);
    assert_eq!(route.work_used, 12);
    assert_eq!(route.selected.operations.len(), 4);
    assert_eq!(route.workholding.as_ref().unwrap().selected.holds.len(), 4);
}

#[test]
fn routed_local_resource_names_do_not_create_shared_inventory() {
    let (pattern, raw) = fixture(2);
    let mut options = shared_fixtures(raw, 1);
    let route = &mut options["production"]["routing"];
    route.as_object_mut().unwrap().remove("shared_workholding");
    route["independence"] = json!("independent_attended_cells_and_transfer_resource/1");
    let plan = build(&pattern, options.clone()).unwrap();
    let routed = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    let holding = routed.workholding.as_ref().unwrap();
    assert_eq!(routed.selected.makespan_milliseconds, 12600);
    assert!(holding.shared_workholding.is_none());
    assert!(holding.selected.shared_peak_occupancy.is_empty());
    assert!(
        holding
            .selected
            .holds
            .iter()
            .all(|hold| hold.pool.is_none())
    );
    options["production"]["routing"]["cells"][0]["workholding"]["holds"][0]["units"] = json!(2);
    refresh_locks(&mut options);
    let error = build(&pattern, options).unwrap_err();
    assert_eq!(error.code, RefusalCode::InvalidDocument);
    assert!(error.message.contains("exceeds local resource"));
}

#[test]
fn routed_pool_demands_sum_all_bound_resources_and_require_every_pool() {
    let (pattern, raw) = fixture(2);
    let mut options = shared_fixtures(raw, 2);
    for cell in options["production"]["routing"]["cells"]
        .as_array_mut()
        .unwrap()
    {
        cell["workholding"]["resources"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"guide","capacity":1}));
        let mut guide = cell["workholding"]["holds"][0].clone();
        guide["id"] = json!("guide-hold");
        guide["resource"] = json!("guide");
        cell["workholding"]["holds"]
            .as_array_mut()
            .unwrap()
            .push(guide);
    }
    for cell in ["cell-a", "cell-b"] {
        options["production"]["routing"]["shared_workholding"]["bindings"]
            .as_array_mut()
            .unwrap()
            .push(json!({"cell":cell,"resource":"guide","pool":"common-jigs"}));
    }
    refresh_pool_locks(&mut options);
    let plan = build(&pattern, options.clone()).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert_eq!(route.selected.makespan_milliseconds, 14700);
    assert_eq!(
        route
            .workholding
            .as_ref()
            .unwrap()
            .selected
            .shared_peak_occupancy["common-jigs"],
        2
    );
    options["production"]["routing"]["shared_workholding"]["resources"][0]["capacity"] = json!(1);
    let error = build(&pattern, options.clone()).unwrap_err();
    assert_eq!(error.code, RefusalCode::InvalidDocument);
    assert!(error.message.contains("exceeds shared pool"));
    // Two separately qualified pools must be available together, even when one has spare units.
    options["production"]["routing"]["shared_workholding"]["resources"] = json!([
        {"id":"common-jigs","capacity":2},{"id":"common-guides","capacity":1}]);
    for binding in options["production"]["routing"]["shared_workholding"]["bindings"]
        .as_array_mut()
        .unwrap()
    {
        if binding["resource"] == "guide" {
            binding["pool"] = json!("common-guides");
        }
    }
    let plan = build(&pattern, options).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert_eq!(route.selected.makespan_milliseconds, 14700);
    let peaks = &route
        .workholding
        .as_ref()
        .unwrap()
        .selected
        .shared_peak_occupancy;
    assert_eq!(peaks["common-jigs"], 1);
    assert_eq!(peaks["common-guides"], 1);
}

#[test]
fn routed_fixture_protocol_sources_and_retained_lifecycles_are_independently_admitted() {
    let (pattern, raw) = fixture(2);
    let options = shared_fixtures(raw, 1);
    let mut cases = Vec::new();
    let mut bad = options.clone();
    bad["production"]["routing"]["shared_workholding"]["allow_estimates"] = json!(false);
    cases.push(bad);
    let mut bad = options.clone();
    bad["production"]["routing"]["shared_workholding"]["release_protocol"] =
        json!("available_at_feed_completion_with_access_in_cell_handling/1");
    cases.push(bad);
    let mut bad = options.clone();
    bad["production"]["routing"]["independence"] =
        json!("independent_attended_cells_and_transfer_resource/1");
    cases.push(bad);
    let mut bad = options.clone();
    bad["production"]["routing"]["shared_workholding"]["bindings"][0]["resource"] =
        json!("undeclared");
    cases.push(bad);
    let mut bad = options.clone();
    bad["production"]["routing"]["cells"][0]["handling"]["initial_milliseconds"]["join"] =
        json!(101);
    refresh_locks(&mut bad); // Re-admitting the cell does not silently re-admit old shared-pool access.
    cases.push(bad);
    let mut bad = options.clone();
    bad["production"]["routing"]["cells"][0]["workholding"]["allow_estimates"] = json!(false);
    refresh_pool_locks(&mut bad);
    cases.push(bad);
    let mut bad = options;
    bad["production"]["routing"]["cells"][0]["workholding"]["qualifications"]["join"][0]["sha256"] =
        json!("00".repeat(32));
    refresh_cell_locks(&mut bad);
    bad["production"]["routing"]["shared_workholding"]["cells"] =
        bad["production"]["routing"]["cell_locks"].clone();
    cases.push(bad);
    for (index, options) in cases.into_iter().enumerate() {
        assert!(
            build(&pattern, options).is_err(),
            "invalid routed fixture source case {index}"
        );
    }
    let (pattern, options) = reentrant();
    let mut options = shared_fixtures(options, 1);
    options["production"]["routing"]["cells"][0]["workholding"]["holds"][0]["acquire"] =
        json!("join");
    options["production"]["routing"]["cells"][0]["workholding"]["holds"][0]["release"] =
        json!("finish");
    refresh_pool_locks(&mut options);
    let error = build(&pattern, options).unwrap_err();
    assert!(
        error
            .message
            .contains("retained or transported occupied fixtures")
    );
}

#[test]
fn routed_fixture_windows_recover_complete_seeds_and_keep_unknown_search_unresolved() {
    let (pattern, raw) = fixture(2);
    let mut options = shared_fixtures(raw, 1);
    options["production"]["routing"]["cells"][1]["available_until_milliseconds"] = json!(14700);
    refresh_pool_locks(&mut options);
    let mut capped = options.clone();
    capped["production"]["routing"]["max_search_nodes"] = json!(4);
    assert_eq!(
        build(&pattern, capped).unwrap_err().code,
        RefusalCode::NotConverged
    );
    let plan = build(&pattern, options.clone()).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert!(route.baseline_replaced && route.search_complete);
    assert!(!route.optimality_proven);
    assert_eq!(route.selected.makespan_milliseconds, 14700);
    assert_eq!(route.selected.operations.len(), 4);
    assert_eq!(route.workholding.as_ref().unwrap().baseline.holds.len(), 4);
    assert_eq!(
        serde_json::to_value(&route.baseline).unwrap(),
        serde_json::to_value(&route.selected).unwrap()
    );
    options["production"]["routing"]["cells"][1]["available_until_milliseconds"] = json!(14699);
    refresh_pool_locks(&mut options);
    let error = build(&pattern, options).unwrap_err();
    assert_eq!(error.code, RefusalCode::NotConverged);
    assert!(
        error
            .message
            .contains("does not establish unrestricted production infeasibility")
    );
}

#[test]
fn routed_fixture_completion_reaches_material_studies_and_changed_materials_refuse() {
    use ftx_export::material_study::Request;

    let (pattern, raw) = fixture(2);
    let options = shared_fixtures(raw, 1);
    let material = pattern.raw()["fabric"]["material"].clone();
    let mut changed = material.clone();
    changed["record"]["properties"]["thickness"]["value"] = json!(0.45);
    let request: Request = serde_json::from_value(json!({
        "schema":"ftx-material-study/1",
        "variants":[
            {"key":"qualified","changes":{"schema":"ftx-material-bindings/1","default":material}},
            {"key":"changed-stack","changes":{"schema":"ftx-material-bindings/1","default":changed}}],
        "sewing":options,
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[]},
        "comparison":{"basis":"nominal","required_consumers":["sewing"],
            "objectives":[{"metric":"sewing_parallel_makespan_seconds","direction":"minimize"}]}
    })).unwrap();
    let report = request.evaluate(&pattern).unwrap();
    capture_routed_fixture("study-pattern.json", pattern.raw());
    capture_routed_fixture("study-request.json", &request);
    capture_routed_fixture("study-report.json", &report);
    let result = serde_json::to_value(report).unwrap();
    let alternatives = result["alternatives"].as_array().unwrap();
    let qualified = alternatives
        .iter()
        .find(|row| row["key"] == "qualified")
        .unwrap();
    assert_eq!(qualified["sewing"]["status"], "available");
    assert_eq!(
        qualified["metrics"]["sewing_parallel_makespan_seconds"],
        14.7
    );
    assert_eq!(qualified["metrics"]["stitch_intervals"], 120);
    assert_eq!(
        qualified["sewing"]["result"]["production"]["routing"]["workholding"]["selected"]["shared_peak_occupancy"]
            ["common-jigs"],
        1
    );
    let refused = alternatives
        .iter()
        .find(|row| row["key"] == "changed-stack")
        .unwrap();
    assert_eq!(refused["sewing"]["status"], "refused");
    assert!(refused["metrics"]["sewing_parallel_makespan_seconds"].is_null());
    assert_eq!(refused["assembly_mass"]["status"], "available");
    assert_eq!(result["comparison"]["frontier"], json!(["qualified"]));
}

fn eligible_fixture(copies: usize) -> (Pattern, Value) {
    let (pattern, mut raw) = fixture(copies);
    let mut alternate = raw["production"]["routing"]["cells"][0].clone();
    alternate["id"] = json!("cell-c");
    alternate["available_until_milliseconds"] = json!(3300);
    alternate["configuration"]["machine"]["id"] = json!("synthetic-alternate-c");
    alternate["configuration"]["machine"]["stitch_mm"] = json!([4, 6]);
    let q = &mut alternate["configuration"]["qualifications"][0];
    q["id"] = json!("qualified-alternate-join");
    q["machine"] = json!("synthetic-alternate-c");
    q["needle"] = json!("needle-c");
    q["thread"] = json!("thread-c");
    q["stitch_mm"] = json!([4, 6]);
    let recipe = &mut alternate["configuration"]["recipes"]["join"];
    recipe["id"] = json!("alternate-join");
    recipe["needle"] = json!("needle-c");
    recipe["thread"] = json!("thread-c");
    recipe["max_stitch_mm"] = json!(5);
    alternate["ordering"]["initial_setup"]["needle"] = json!("needle-c");
    alternate["ordering"]["setup_times"]["machine"] = json!("synthetic-alternate-c");
    let change = &mut alternate["ordering"]["setup_times"]["transitions"][0];
    change["from"]["needle"] = json!("needle-c");
    change["to"] = json!({"needle":"needle-c","thread":"thread-c"});
    let routing = &mut raw["production"]["routing"];
    routing["model"] = json!("eligible_operation_route/1");
    routing["cells"][0]["available_until_milliseconds"] = json!(5300);
    routing["cells"].as_array_mut().unwrap().push(alternate);
    routing["route"][0]["eligible_cells"] = json!(["cell-a", "cell-c"]);
    routing["route"][1]["eligible_cells"] = json!(["cell-b"]);
    routing["transfer"]["transfers"]
        .as_array_mut()
        .unwrap()
        .push(json!({"from":"cell-c","to":"cell-b","milliseconds":900}));
    refresh_locks(&mut raw);
    (pattern, raw)
}

/// Exhaust all four first-stage assignments and all six precedence-respecting dispatch orders.
/// This small integer oracle uses the independent synthetic phase inputs, no planner or receipt.
fn independently_enumerated_finish() -> u64 {
    let orders = [
        [0, 0, 1, 1],
        [0, 1, 0, 1],
        [0, 1, 1, 0],
        [1, 0, 0, 1],
        [1, 0, 1, 0],
        [1, 1, 0, 0],
    ];
    let mut best = u64::MAX;
    for assignment in [[0, 0], [0, 2], [2, 0], [2, 2]] {
        for order in orders {
            let mut stage = [0; 2];
            let mut garment = [0u64; 2];
            let mut machines = [0u64; 3];
            let mut set_up = [false; 3];
            let mut mover = 0;
            let mut feasible = true;
            for copy in order {
                let cell = if stage[copy] == 0 {
                    assignment[copy]
                } else {
                    1
                };
                let arrived = if stage[copy] == 0 {
                    0
                } else {
                    mover =
                        mover.max(garment[copy]) + if assignment[copy] == 0 { 700 } else { 900 };
                    mover
                };
                let setup = if set_up[cell] {
                    0
                } else {
                    [1000, 500, 1000][cell]
                };
                let finish =
                    machines[cell].max(arrived) + 100 + setup + [4000, 2000, 2000][cell] + 200;
                if finish > [5300, u64::MAX, 3300][cell] {
                    feasible = false;
                    break;
                }
                machines[cell] = finish;
                garment[copy] = finish;
                set_up[cell] = true;
                stage[copy] += 1;
            }
            if feasible {
                best = best.min(*garment.iter().max().unwrap());
            }
        }
    }
    best
}

#[test]
fn eligible_fixture_demand_follows_selected_cells_and_only_executed_holds() {
    let (pattern, raw) = eligible_fixture(2);
    let refusal = build(&pattern, shared_fixtures(raw.clone(), 1)).unwrap_err();
    // Both first-stage windows require the two machines to run concurrently; one common jig
    // cannot support that assignment, even though every cell has its own declared capacity.
    assert_eq!(refusal.code, RefusalCode::NotConverged);

    let options: SewOptions = serde_json::from_value(shared_fixtures(raw, 2)).unwrap();
    let program = SewProgram::build(&pattern, &options).unwrap();
    let route = program
        .plan
        .production
        .as_ref()
        .unwrap()
        .routing
        .as_ref()
        .unwrap();
    assert_eq!(
        route.selected.makespan_milliseconds,
        independently_enumerated_finish()
    );
    assert_eq!(program.stitches.len(), 100);
    assert_eq!(program.plan.total_feed_seconds(), 10.0);
    assert_eq!(route.schema, "ftx-sewing-routing/3");
    assert_eq!(route.reference_scope, Some("seed_route_one_garment/1"));
    let fixtures = route.workholding.as_ref().unwrap();
    assert_eq!(fixtures.cells.len(), 3); // All eligible source qualifications are retained.
    for timeline in [&fixtures.baseline, &fixtures.selected] {
        assert_eq!(timeline.holds.len(), 4); // No duplicate hold from the unchosen join cell.
        assert_eq!(timeline.shared_peak_occupancy["common-jigs"], 2);
        assert_eq!(timeline.shared_final_occupancy["common-jigs"], 0);
        let mut counts = BTreeMap::new();
        for hold in &timeline.holds {
            *counts.entry(hold.cell.as_str()).or_insert(0) += 1;
        }
        assert_eq!(
            counts,
            BTreeMap::from([("cell-a", 1), ("cell-b", 2), ("cell-c", 1)])
        );
    }
    for operation in &route.selected.operations {
        let hold = fixtures
            .selected
            .holds
            .iter()
            .find(|hold| hold.operation == operation.id)
            .unwrap();
        assert_eq!(hold.cell, operation.cell);
        assert_eq!(
            hold.acquired_at_milliseconds,
            operation.loading_start_milliseconds
        );
        assert_eq!(hold.released_at_milliseconds, operation.finish_milliseconds);
    }
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &options).unwrap()).unwrap()
    );
}

#[test]
fn eligible_unpooled_local_fixture_remains_independent_and_stale_source_refuses() {
    let (pattern, raw) = eligible_fixture(2);
    let mut raw = shared_fixtures(raw, 1);
    raw["production"]["routing"]["shared_workholding"]["bindings"]
        .as_array_mut()
        .unwrap()
        .retain(|binding| binding["cell"] != "cell-c");
    let plan = build(&pattern, raw.clone()).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    // The unpooled alternate finishes at3300. Shared cell-a holds until5300, so cell-b's
    // 2800ms first operation starts at5300 and its2300ms second finishes at10400.
    assert_eq!(route.selected.makespan_milliseconds, 10400);
    let fixtures = route.workholding.as_ref().unwrap();
    assert_eq!(fixtures.selected.holds.len(), 4);
    assert_eq!(fixtures.selected.shared_peak_occupancy["common-jigs"], 1);
    assert_eq!(fixtures.selected.local_peak_occupancy["cell-c"]["jig"], 1);
    assert!(
        fixtures
            .selected
            .holds
            .iter()
            .filter(|hold| hold.cell == "cell-c")
            .all(|hold| hold.pool.is_none())
    );

    // Exact complete cell/pool locks cannot turn a stale local material/process qualification
    // into source evidence, including for an alternate eligible machine.
    raw["production"]["routing"]["cells"][2]["workholding"]["qualifications"]["join"][0]["sha256"] =
        json!("00".repeat(32));
    refresh_cell_locks(&mut raw);
    raw["production"]["routing"]["shared_workholding"]["cells"] =
        raw["production"]["routing"]["cell_locks"].clone();
    let refusal = build(&pattern, raw).unwrap_err();
    assert!(refusal.message.contains("qualification"), "{refusal}");
}

#[test]
fn eligible_cells_find_a_mixed_complete_batch_and_replay_actual_recipe_geometry() {
    let (pattern, raw) = eligible_fixture(2);
    let options: SewOptions = serde_json::from_value(raw).unwrap();
    let program = SewProgram::build(&pattern, &options).unwrap();
    let plan = &program.plan;
    let production = plan.production.as_ref().unwrap();
    let route = production.routing.as_ref().unwrap();
    assert_eq!(independently_enumerated_finish(), 9300);
    assert_eq!(
        route.selected.makespan_milliseconds,
        independently_enumerated_finish()
    );
    assert!(route.baseline_replaced && route.search_complete && !route.budget_exhausted);
    assert!(route.lower_bound_makespan_milliseconds <= route.selected.makespan_milliseconds);
    assert!(!route.optimality_proven); // Exhaustive append dispatch is not unrestricted timing proof.
    assert_eq!(route.schema, "ftx-sewing-routing/2");
    assert_eq!(route.reference_scope, Some("seed_route_one_garment/1"));
    assert_eq!(plan.schema, "ftx-sewing-plan/19");
    assert_eq!(production.schema, "ftx-sewing-production/7");
    assert_eq!((plan.stitch_intervals, plan.feed_seconds), (60, 6.0));
    assert_eq!(
        (plan.total_stitch_intervals(), plan.total_feed_seconds()),
        (100, 10.0)
    );
    assert_eq!(program.stitches.len(), 100);
    assert_eq!(route.selected.transfer_milliseconds, 1600);
    let mut counts = BTreeMap::new();
    for operation in &production.operations {
        let cell = operation.cell.as_deref().unwrap();
        let source = plan.parallel_cell_plan(cell).unwrap();
        let seam = &source.seams[operation.seam_index];
        assert_eq!(seam.pass.operation_id(), operation.operation_id);
        assert_eq!(seam.qualification_id, operation.qualification_id);
        assert_eq!(seam.stitch_intervals, operation.stitch_intervals);
        assert_eq!(seam.feed_seconds, operation.feed_seconds);
        *counts.entry(cell).or_insert(0) += operation.stitch_intervals;
    }
    assert_eq!(
        counts,
        BTreeMap::from([("cell-a", 40), ("cell-b", 40), ("cell-c", 20)])
    );
    for stitch in &program.stitches {
        let cell = stitch.cell.as_deref().unwrap();
        let seam = &plan.parallel_cell_plan(cell).unwrap().seams[stitch.operation];
        assert_eq!(stitch.a_mm, seam.stations.a[stitch.to_station]);
        assert_eq!(stitch.b_mm, seam.stations.b[stitch.to_station]);
    }
    for transfer in &route.selected.transfers {
        let before = route
            .selected
            .operations
            .iter()
            .find(|row| row.copy == transfer.copy && row.operation_id == transfer.from_operation)
            .unwrap();
        let after = route
            .selected
            .operations
            .iter()
            .find(|row| row.copy == transfer.copy && row.operation_id == transfer.to_operation)
            .unwrap();
        assert_eq!(transfer.from, before.cell);
        assert_eq!(transfer.to, after.cell);
        assert!(transfer.start_milliseconds >= before.finish_milliseconds);
        assert!(after.loading_start_milliseconds >= transfer.finish_milliseconds);
    }
    assert_eq!(
        serde_json::to_value(&program).unwrap(),
        serde_json::to_value(SewProgram::build(&pattern, &plan.options).unwrap()).unwrap()
    );
}

#[test]
fn eligible_search_bounds_keep_only_complete_incumbents_and_ignore_unused_late_cells() {
    let (pattern, mut raw) = eligible_fixture(2);
    raw["production"]["routing"]["max_search_nodes"] = json!(4);
    assert_eq!(
        build(&pattern, raw.clone()).unwrap_err().code,
        RefusalCode::NotConverged
    );
    raw["production"]["routing"]["cells"][0]
        .as_object_mut()
        .unwrap()
        .remove("available_until_milliseconds");
    refresh_locks(&mut raw);
    let plan = build(&pattern, raw).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert!(!route.baseline_replaced && route.budget_exhausted && !route.optimality_proven);
    assert_eq!((route.search_nodes, route.work_used), (4, 12));
    assert_eq!(route.selected.operations.len(), 4);
    assert_eq!(plan.total_stitch_intervals(), 120);
    let (pattern, mut raw) = eligible_fixture(1);
    raw["production"]["routing"]["cells"][0]["available_at_milliseconds"] = json!(1_000_000);
    raw["production"]["routing"]["cells"][0]["available_until_milliseconds"] = json!(1_005_300);
    refresh_locks(&mut raw);
    let plan = build(&pattern, raw).unwrap();
    let route = plan.production.as_ref().unwrap().routing.as_ref().unwrap();
    assert_eq!(route.selected.makespan_milliseconds, 7000);
    assert!(
        route
            .selected
            .operations
            .iter()
            .all(|row| row.cell != "cell-a")
    );
    assert!(route.lower_bound_makespan_milliseconds <= 7000);
}

#[test]
fn eligible_domains_require_every_declared_alternative_and_directed_handoff() {
    let (pattern, raw) = eligible_fixture(2);
    let mut cases = Vec::new();
    for (field, value) in [
        ("model", json!("fixed_operation_route/1")),
        ("max_search_nodes", json!(3)),
    ] {
        let mut bad = raw.clone();
        bad["production"]["routing"][field] = value;
        cases.push(bad);
    }
    for cells in [
        json!([]),
        json!(["cell-a", "cell-a"]),
        json!(["cell-c"]),
        json!(["cell-a", "absent"]),
    ] {
        let mut bad = raw.clone();
        bad["production"]["routing"]["route"][0]["eligible_cells"] = cells;
        cases.push(bad);
    }
    let mut bad = raw.clone();
    bad["production"]["routing"]["transfer"]["transfers"]
        .as_array_mut()
        .unwrap()
        .pop();
    cases.push(bad);
    let mut bad = raw;
    bad["production"]["routing"]["cells"][2]["configuration"]["recipes"]["join"]["max_stitch_mm"] =
        json!(2.5);
    refresh_locks(&mut bad);
    cases.push(bad);
    for (index, bad) in cases.into_iter().enumerate() {
        assert!(
            build(&pattern, bad).is_err(),
            "invalid eligible domain {index} admitted"
        );
    }
}

#[test]
fn eligible_yarn_and_study_protocols_include_only_selected_operations_even_on_used_cells() {
    use ftx_export::material_study::Request;

    let (pattern, mut options) = eligible_fixture(2);
    let yarn = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: "coats/epic-2994120-2026".into(),
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection();
    for (index, cell) in options["production"]["routing"]["cells"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        cell["configuration"]["qualifications"][0]["thread_consumption"] = json!({
            "channels_complete":true,"evidence":"estimate","allow_estimates":true,
            "source":source("eligible-thread"),"method":"Synthetic complete consumption per actual operation",
            "channels":[{"id":"complete","material":yarn,
                "metres_per_traversal":([0.01,0.02,0.03][index]),
                "start_allowance_m":([0.1,0.05,0.2][index]),
                "end_allowance_m":([0.2,0.05,0.1][index])}]});
    }
    // C is used for one join. It also qualifies reinforcement, but its closed window makes that
    // alternative unused. That unexecuted operation deliberately has no consumption evidence.
    let c = &mut options["production"]["routing"]["cells"][2];
    let mut q = c["configuration"]["qualifications"][0].clone();
    q["id"] = json!("unused-c-reinforcement");
    q["thread"] = json!("unused-thread");
    q.as_object_mut().unwrap().remove("thread_consumption");
    c["configuration"]["qualifications"]
        .as_array_mut()
        .unwrap()
        .push(q);
    let mut recipe = c["configuration"]["recipes"]["join"].clone();
    recipe["id"] = json!("unused-reinforcement");
    recipe["thread"] = json!("unused-thread");
    c["configuration"]["recipes"]["reinforce"] = recipe;
    c["handling"]["initial_milliseconds"]["reinforce"] = json!(100);
    c["handling"]["final_milliseconds"]["reinforce"] = json!(200);
    for (from, to) in [
        ("idle", "unused-thread"),
        ("thread-c", "unused-thread"),
        ("unused-thread", "thread-c"),
    ] {
        c["ordering"]["setup_times"]["transitions"]
            .as_array_mut()
            .unwrap()
            .push(json!({
            "from":{"needle":"needle-c","thread":from},
            "to":{"needle":"needle-c","thread":to},"milliseconds":1000}));
    }
    options["production"]["routing"]["route"][1]["eligible_cells"] = json!(["cell-b", "cell-c"]);
    options["production"]["routing"]["transfer"]["transfers"]
        .as_array_mut()
        .unwrap()
        .push(json!({"from":"cell-a","to":"cell-c","milliseconds":600}));
    options["consumables"] = json!({});
    refresh_locks(&mut options);
    let plan = build(&pattern, options.clone()).unwrap();
    assert_eq!(plan.parallel_cell_plan("cell-c").unwrap().seams.len(), 2);
    let consumption = plan.consumables.as_ref().unwrap();
    assert!(consumption.complete_consumption_coverage && consumption.complete_mass_coverage);
    assert!(consumption.missing_operations.is_empty());
    assert_eq!(consumption.operation_channels.len(), 3);
    assert!((consumption.total_required_length_m.unwrap() - 2.6).abs() < 1e-12);
    let counts: BTreeMap<_, _> = consumption
        .operation_channels
        .iter()
        .map(|row| {
            (
                (row.cell.as_deref().unwrap(), row.operation_id.as_str()),
                row.stitch_intervals,
            )
        })
        .collect();
    assert_eq!(
        counts,
        BTreeMap::from([
            (("cell-a", "join"), 40),
            (("cell-b", "reinforce"), 40),
            (("cell-c", "join"), 20)
        ])
    );
    let request: Request = serde_json::from_value(json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"eligible","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern.raw()["fabric"]["material"]}}],"sewing":options,
        "comparison":{"basis":"nominal","required_consumers":["sewing","sewing_consumables"],
            "objectives":[{"metric":"sewing_parallel_makespan_seconds","direction":"minimize"},
                {"metric":"thread_required_length_m","direction":"minimize"}]}}))
    .unwrap();
    let report = serde_json::to_value(request.evaluate(&pattern).unwrap()).unwrap();
    assert_eq!(report["comparison"]["frontier"], json!(["eligible"]));
    assert_eq!(
        report["alternatives"][0]["metrics"]["stitch_intervals"],
        100
    );
    assert!(
        (report["alternatives"][0]["metrics"]["thread_required_length_m"]
            .as_f64()
            .unwrap()
            - 2.6)
            .abs()
            < 1e-12
    );
}

#[test]
fn eligible_recipe_selection_scores_makespan_and_refuses_unoptimized_feed_or_cost_claims() {
    use ftx_export::sewing_selection::Request;

    let (pattern, options) = eligible_fixture(2);
    let authored = &options["passes"][1];
    let mut request = json!({"schema":"ftx-sewing-selection/5","options":options,
        "objective":"minimize_parallel_makespan","max_plan_evaluations":2,
        "alternatives":[{"operation":"reinforce","recipes":[{"id":"same-authored-pass",
            "needle":authored["needle"],"thread":authored["thread"],
            "max_stitch_mm":authored["max_stitch_mm"],
            "stitches_per_minute":authored["stitches_per_minute"]}]}]});
    let selected = serde_json::from_value::<Request>(request.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(selected.selected_objective_seconds, Some(9.3));
    assert_eq!(
        selected
            .selected_plan
            .as_ref()
            .unwrap()
            .total_stitch_intervals(),
        100
    );
    assert!(!selected.optimality_proven);
    for (schema, objective) in [
        ("ftx-sewing-selection/1", "minimize_feed_seconds"),
        (
            "ftx-sewing-selection/3",
            "minimize_thread_observed_order_cost",
        ),
    ] {
        request["schema"] = json!(schema);
        request["objective"] = json!(objective);
        let error = serde_json::from_value::<Request>(request.clone())
            .unwrap()
            .select(&pattern)
            .unwrap_err();
        assert!(
            error.message.contains("cannot claim their own minimum"),
            "{error}"
        );
    }
}

#[test]
fn feed_and_thread_only_comparisons_retain_the_entire_eligible_routing_protocol() {
    use ftx_export::material_study::Request;

    let (pattern, mut options) = eligible_fixture(1);
    options["production"]["routing"]["cells"][0]["available_at_milliseconds"] = json!(1_000_000);
    options["production"]["routing"]["cells"][0]["available_until_milliseconds"] = json!(1_005_300);
    let yarn = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: "coats/epic-2994120-2026".into(),
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection();
    for cell in options["production"]["routing"]["cells"]
        .as_array_mut()
        .unwrap()
    {
        cell["configuration"]["qualifications"][0]["thread_consumption"] = json!({
            "channels_complete":true,"evidence":"estimate","allow_estimates":true,
            "source":source("protocol-thread"),"method":"Synthetic complete thread per selected interval",
            "channels":[{"id":"complete","material":yarn,"metres_per_traversal":0.01,
                "start_allowance_m":0.0,"end_allowance_m":0.0}]});
    }
    options["consumables"] = json!({});
    refresh_locks(&mut options);
    let study = |options: Value, metric: &str| {
        let request: Request = serde_json::from_value(json!({"schema":"ftx-material-study/1",
            "variants":[{"key":"eligible","changes":{"schema":"ftx-material-bindings/1",
                "default":pattern.raw()["fabric"]["material"]}}],"sewing":options,
            "comparison":{"basis":"nominal","objectives":[{"metric":metric,"direction":"minimize"}]}})).unwrap();
        serde_json::to_value(request.evaluate(&pattern).unwrap()).unwrap()
    };
    for metric in ["feed_seconds", "thread_required_length_m"] {
        let original = study(options.clone(), metric);
        assert_eq!(original, study(options.clone(), metric));
        assert_eq!(original["comparison"]["frontier"], json!(["eligible"]));
        let selected =
            &original["alternatives"][0]["sewing"]["result"]["production"]["routing"]["selected"];
        assert!(
            selected["operations"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["cell"] != "cell-a")
        );
        let mut changed = options.clone();
        changed["production"]["routing"]["cells"][0]["available_until_milliseconds"] =
            json!(1_005_301);
        // An idle alternative still needs its exact content lock before the job can be admitted.
        let stale = study(changed.clone(), metric);
        assert_eq!(stale["alternatives"][0]["sewing"]["status"], "refused");
        assert_eq!(stale["comparison"]["frontier"], json!([]));
        refresh_locks(&mut changed);
        let changed = study(changed, metric);
        assert_eq!(
            *selected,
            changed["alternatives"][0]["sewing"]["result"]["production"]["routing"]["selected"]
        );
        assert_eq!(
            original["comparison"]["alternatives"][0]["readings"],
            changed["comparison"]["alternatives"][0]["readings"]
        );
        let identity = &original["comparison"]["alternatives"][0]["input_protocol_sha256"];
        assert!(identity.is_string());
        assert_ne!(
            *identity,
            changed["comparison"]["alternatives"][0]["input_protocol_sha256"]
        );
    }
}

#[test]
fn authored_routes_and_eligible_assignments_share_exact_winners_and_full_search_identity() {
    use ftx_export::{material_study, sewing_selection};

    let (pattern, mut fixed) = fixture(2);
    let (_, mut eligible) = eligible_fixture(2);
    let yarn = Catalog::builtin()
        .unwrap()
        .resolve(&Selection {
            id: "coats/epic-2994120-2026".into(),
            ..Selection::default()
        })
        .unwrap()
        .frozen_selection();
    for options in [&mut fixed, &mut eligible] {
        for cell in options["production"]["routing"]["cells"]
            .as_array_mut()
            .unwrap()
        {
            cell["configuration"]["qualifications"][0]["thread_consumption"] = json!({
                "channels_complete":true,"evidence":"estimate","allow_estimates":true,
                "source":source("joint-route-thread"),"method":"Synthetic complete thread per actual interval",
                "channels":[{"id":"complete","material":yarn,"metres_per_traversal":0.01,
                    "start_allowance_m":0.0,"end_allowance_m":0.0}]});
        }
        options["consumables"] = json!({});
        refresh_locks(options);
    }
    let pass = fixed["passes"][1].clone();
    let mut input = json!({"schema":"ftx-sewing-selection/6","options":fixed,
        "objective":"minimize_parallel_makespan","max_plan_evaluations":4,
        "route_candidates":[{"id":"eligible-specialists","routing":eligible["production"]["routing"]}],
        "alternatives":[{"operation":"reinforce","recipes":[{"id":"same-authored-pass",
            "needle":pass["needle"],"thread":pass["thread"],"max_stitch_mm":pass["max_stitch_mm"],
            "stitches_per_minute":pass["stitches_per_minute"]}]}]});
    let result = serde_json::from_value::<sewing_selection::Request>(input.clone())
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(result.candidate_combinations, 4);
    assert_eq!(
        result
            .trials
            .iter()
            .map(|row| row.route_choice.as_deref().unwrap())
            .collect::<Vec<_>>(),
        [
            "template",
            "template",
            "eligible-specialists",
            "eligible-specialists"
        ]
    );
    assert_eq!(
        result.selected_route.as_deref(),
        Some("eligible-specialists")
    );
    assert_eq!(result.selected_objective_seconds, Some(9.3));
    assert_eq!(
        result.selected_choices.as_ref().unwrap()["reinforce"],
        "template"
    );
    assert!(result.search_complete && !result.optimality_proven);
    let winner = result.selected_plan.as_ref().unwrap();
    assert_eq!(winner.total_stitch_intervals(), 100);
    assert_eq!(
        winner
            .options
            .production
            .as_ref()
            .unwrap()
            .routing
            .as_ref()
            .unwrap()
            .model,
        "eligible_operation_route/1"
    );
    let program = SewProgram::build(&pattern, &winner.options).unwrap();
    assert_eq!(program.stitches.len(), 100);
    assert_eq!(
        serde_json::to_value(&program.plan).unwrap(),
        serde_json::to_value(winner).unwrap()
    );
    let mut bounded = input.clone();
    bounded["max_plan_evaluations"] = json!(1);
    let bounded = serde_json::from_value::<sewing_selection::Request>(bounded)
        .unwrap()
        .select(&pattern)
        .unwrap();
    assert_eq!(bounded.selected_route.as_deref(), Some("template"));
    assert_eq!(bounded.selected_plan.unwrap().total_stitch_intervals(), 120);
    assert!(bounded.budget_exhausted && !bounded.optimality_proven);
    for (schema, objective) in [
        ("ftx-sewing-selection/1", "minimize_feed_seconds"),
        (
            "ftx-sewing-selection/3",
            "minimize_thread_observed_order_cost",
        ),
    ] {
        let mut invalid = input.clone();
        invalid["schema"] = json!(schema);
        invalid["objective"] = json!(objective);
        assert!(
            serde_json::from_value::<sewing_selection::Request>(invalid)
                .unwrap()
                .select(&pattern)
                .is_err()
        );
    }
    let mut excessive = input.clone();
    excessive["route_candidates"][0]["routing"]["max_search_nodes"] = json!(50_000);
    assert_eq!(
        serde_json::from_value::<sewing_selection::Request>(excessive)
            .unwrap()
            .scheduling_work_bound()
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );

    // With a late transfer resource the fixed template wins. An unselected eligible candidate
    // and the outer work allowance still define which other production routes were searched.
    input["route_candidates"][0]["routing"]["transfer"]["available_at_milliseconds"] =
        json!(20_000);
    let study = |selection: Value, metric: &str| {
        let request: material_study::Request = serde_json::from_value(json!({
            "schema":"ftx-material-study/1","sewing_selection":selection,
            "variants":[{"key":"joint","changes":{"schema":"ftx-material-bindings/1",
                "default":pattern.raw()["fabric"]["material"]}}],
            "comparison":{"basis":"nominal","objectives":[{"metric":metric,"direction":"minimize"}]}})).unwrap();
        serde_json::to_value(request.evaluate(&pattern).unwrap()).unwrap()
    };
    for metric in ["feed_seconds", "thread_required_length_m"] {
        let original = study(input.clone(), metric);
        assert_eq!(original, study(input.clone(), metric));
        assert_eq!(original["comparison"]["frontier"], json!(["joint"]));
        assert_eq!(
            original["alternatives"][0]["sewing_selection"]["result"]["selected_route"],
            "template"
        );
        let plan = &original["alternatives"][0]["sewing"]["result"];
        assert_eq!(
            plan["production"]["routing"]["schema"],
            "ftx-sewing-routing/1"
        );
        let mut other = input.clone();
        let mut offered = other["options"].clone();
        offered["production"]["routing"] = other["route_candidates"][0]["routing"].clone();
        offered["production"]["routing"]["cells"][2]["available_until_milliseconds"] = json!(3301);
        refresh_locks(&mut offered);
        other["route_candidates"][0]["routing"] = offered["production"]["routing"].clone();
        let mut capped = input.clone();
        capped["max_plan_evaluations"] = json!(3);
        for changed in [other, capped] {
            let changed = study(changed, metric);
            assert_eq!(*plan, changed["alternatives"][0]["sewing"]["result"]);
            assert_eq!(
                original["comparison"]["alternatives"][0]["readings"],
                changed["comparison"]["alternatives"][0]["readings"]
            );
            let protocol = &original["comparison"]["alternatives"][0]["input_protocol_sha256"];
            assert!(protocol.is_string());
            assert_ne!(
                *protocol,
                changed["comparison"]["alternatives"][0]["input_protocol_sha256"]
            );
        }
    }
}
