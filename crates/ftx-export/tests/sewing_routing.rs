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
    }
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
