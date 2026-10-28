//! Actual native commands and portable adapters consume one complete production plan and quantity.
//! All source fixtures are synthetic; these checks establish software integration only.

use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const OPTIONS: &str = include_str!("../../../examples/sewing-production-options.json");
const WORKHOLDING: &str = include_str!("../../../examples/sewing-workholding-options.json");
const ACCESS: &str = include_str!("../../../examples/sewing-access-options.json");
const HANDLING: &str = include_str!("../../../examples/sewing-handling-options.json");
const PARALLEL: &str = include_str!("../../../examples/sewing-parallel-options.json");
const HETEROGENEOUS: &str = include_str!("../../../examples/sewing-heterogeneous-options.json");
const TIMED_SELECTION: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-timed-selection.json");
static NEXT: AtomicU64 = AtomicU64::new(0);

fn workspace() -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "ftx-sewing-production-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    directory
}

fn run(main: bool, directory: &Path, action: &str, pattern: &str, request: &str) -> Value {
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(directory);
    if main {
        command.args(["--json", "materials"]);
    }
    let output = command.args([action, pattern, request]).output().unwrap();
    assert!(
        output.status.success(),
        "{main}/{action}: {} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    report.get("result").unwrap_or(&report).clone()
}

fn portable(text: String) -> Value {
    serde_json::from_str(&text).unwrap()
}

fn refresh_shared_cell_locks(options: &mut Value) {
    let typed: ftx_export::sewing::SewOptions = serde_json::from_value(options.clone()).unwrap();
    let locks: std::collections::BTreeMap<_, _> = typed
        .production
        .as_ref()
        .unwrap()
        .parallel
        .as_ref()
        .unwrap()
        .cells
        .iter()
        .map(|cell| {
            (
                cell.id.clone(),
                ftx_export::sewing::production::parallel::operator::cell_sha256(cell, &typed)
                    .unwrap(),
            )
        })
        .collect();
    options["production"]["parallel"]["shared_operator"]["cells"] = json!(locks);
    if options["production"]["parallel"]["shared_workholding"].is_object() {
        options["production"]["parallel"]["shared_workholding"]["cells"] = json!(locks);
    }
}

fn shared_operator_options(windowed: bool) -> Value {
    let mut options: Value = serde_json::from_str(HETEROGENEOUS).unwrap();
    let parallel = &mut options["production"]["parallel"];
    parallel["time_basis"] = json!("ceil_operation_feed_milliseconds/1");
    parallel["independence"] = json!("independent_machines_and_fixtures_shared_operator/1");
    parallel["max_search_nodes"] = json!(60000);
    if windowed {
        parallel["cells"][0]["available_until_milliseconds"] = json!(9899);
        parallel["cells"][1]["available_until_milliseconds"] = json!(11104);
    }
    parallel["shared_operator"] = json!({
        "model":"exclusive_setup_handling_autonomous_feed/1","operator":"synthetic-worker",
        "available_at_milliseconds":0,"available_until_milliseconds":if windowed {11104} else {50000},
        "cells":{},"initial_arrival_milliseconds":{"cell-a":0,"cell-b":0},
        "transfers":[{"from":"cell-a","to":"cell-b","milliseconds":0},
            {"from":"cell-b","to":"cell-a","milliseconds":0}],"max_search_nodes":40000,
        "source":{"reference":"test:portable-shared-operator","accessed_on":"2026-10-27",
            "note":"Invented one-worker protocol, zero travel and unattended feed; no measured machine claim"},
        "method":"Nonpreemptive travel, handling and setup followed by qualified unattended feed; safe waiting explicitly assumed",
        "evidence":"estimate","allow_estimates":true
    });
    refresh_shared_cell_locks(&mut options);
    options
}

fn routed_specialist_options() -> Value {
    let mut options: Value = serde_json::from_str(HETEROGENEOUS).unwrap();
    let mut cells = options["production"]["parallel"]["cells"].clone();
    for (cell, operation) in cells
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(["join", "reinforce"])
    {
        for key in ["recipes", "qualification_refs"] {
            cell["configuration"][key]
                .as_object_mut()
                .unwrap()
                .retain(|id, _| id == operation);
        }
        let admitted: Vec<_> = cell["configuration"]["qualification_refs"][operation]
            .as_array()
            .unwrap()
            .iter()
            .map(|q| q["id"].clone())
            .collect();
        cell["configuration"]["qualifications"]
            .as_array_mut()
            .unwrap()
            .retain(|q| admitted.contains(&q["id"]));
        for key in [
            "qualifications",
            "initial_milliseconds",
            "final_milliseconds",
        ] {
            cell["handling"][key]
                .as_object_mut()
                .unwrap()
                .retain(|id, _| id == operation);
        }
        cell["handling"]["transitions"] = json!([]);
        cell["handling"]["method"] = json!(
            "Load and unload every separate routed operation; no retained garment or fixture"
        );
        cell.as_object_mut().unwrap().remove("workholding");
    }
    let source = json!({"reference":"test:native-specialist-route","accessed_on":"2026-10-27",
        "note":"Synthetic complete handoffs and independent attended cells; no physical process validation"});
    options["qualifications"] = json!([]);
    options["consumables"]["production_copies"] = json!(2);
    options["production"] = json!({"production_copies":2,
        "ordering":{"objective":"minimize_setup_changes","max_search_nodes":1},
        "routing":{"model":"fixed_operation_route/1","objective":"minimize_makespan",
            "time_basis":"ceil_operation_feed_milliseconds/1","protocol":"unloaded_between_operations/1",
            "independence":"independent_attended_cells_and_transfer_resource/1",
            "max_search_nodes":10000,"staging_capacity":2,"cells":cells,"cell_locks":{},
            "route":[{"operation":"join","cell":"cell-a"},{"operation":"reinforce","cell":"cell-b"}],
            "transfer":{"id":"transfer-worker","model":"serialized_unloaded_handoffs/1",
                "available_at_milliseconds":0,"transfers":[{"from":"cell-a","to":"cell-b","milliseconds":700}],
                "source":source,"method":"Position, pick up, move, deposit and return ready; outside cell loading and unloading",
                "evidence":"estimate","allow_estimates":true},
            "source":source,"method":"Dedicated attending worker per machine; sufficient staging at every cell; separate handoff worker",
            "evidence":"estimate","allow_estimates":true}});
    refresh_routed_locks(&mut options);
    options
}

fn refresh_routed_locks(options: &mut Value) {
    let typed: ftx_export::sewing::SewOptions = serde_json::from_value(options.clone()).unwrap();
    let locks: std::collections::BTreeMap<_, _> = typed
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
                ftx_export::sewing::production::parallel::operator::cell_sha256(cell, &typed)
                    .unwrap(),
            )
        })
        .collect();
    options["production"]["routing"]["cell_locks"] = json!(locks);
}

#[test]
fn routed_specialists_replay_actual_native_stitches_inventory_previews_and_material_studies() {
    let options = routed_specialist_options();
    let options_text = options.to_string();
    let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, &options_text).unwrap());
    let plan = &program["plan"];
    let route = &plan["production"]["routing"];
    assert_eq!(program["schema"], "ftx-sewing-program/7");
    assert_eq!(route["selected"]["makespan_milliseconds"], 11934);
    assert_eq!(route["selected"]["scheduled_feed_milliseconds"], 10668);
    assert_eq!(route["selected"]["transfers"].as_array().unwrap().len(), 2);
    assert_eq!(program["stitches"].as_array().unwrap().len(), 120);
    assert!(
        (plan["consumables"]["total_required_length_m"]
            .as_f64()
            .unwrap()
            - 2.2)
            .abs()
            < 1e-12
    );
    assert_eq!(
        plan["consumables"]["purchasing"]["material_orders"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        plan["consumables"]["purchasing"]["least_observed_independent_order_total"],
        14.0
    );
    for source in route["cell_plans"].as_array().unwrap() {
        assert_eq!(source["plan"]["pattern_sha256"], plan["pattern_sha256"]);
        assert_eq!(source["plan"]["seams"].as_array().unwrap().len(), 1);
    }
    let replay = portable(
        ftx_wasm::manufacturing::sew_stitches(PATTERN, &plan["options"].to_string()).unwrap(),
    );
    assert_eq!(program, replay);
    let pattern: Value = serde_json::from_str(PATTERN).unwrap();
    let study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"specialists","changes":{"schema":"ftx-material-bindings/1","default":pattern["fabric"]["material"]}}],
        "sewing":options,"comparison":{"basis":"nominal","required_consumers":["sewing","sewing_consumables","sewing_purchasing"],
            "objectives":[{"metric":"sewing_parallel_makespan_seconds","direction":"minimize"},
                {"metric":"thread_required_length_m","direction":"minimize"}]}});
    let report = portable(ftx_wasm::materials::study(PATTERN, &study.to_string()).unwrap());
    assert_eq!(report["comparison"]["frontier"], json!(["specialists"]));
    let bound_plan = portable(
        ftx_wasm::manufacturing::sew_plan(
            report["alternatives"][0]["binding"]["result"]["pattern_json"]
                .as_str()
                .unwrap(),
            &options.to_string(),
        )
        .unwrap(),
    );
    assert_eq!(
        report["alternatives"][0]["sewing"]["result"]["production"]["routing"],
        bound_plan["production"]["routing"]
    );
    assert_eq!(
        bound_plan["production"]["routing"]["selected"],
        route["selected"]
    );
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    fs::write(directory.join("options.json"), options_text).unwrap();
    fs::write(directory.join("study.json"), study.to_string()).unwrap();
    for main in [false, true] {
        assert_eq!(
            run(main, &directory, "sew-plan", "pattern.json", "options.json"),
            *plan
        );
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "options.json"
            ),
            program
        );
        assert_eq!(
            run(main, &directory, "study", "pattern.json", "study.json"),
            report
        );
    }
    for (cell, operation) in [("cell-a", "join"), ("cell-b", "reinforce")] {
        let svg =
            ftx_wasm::manufacturing::sew_cell_svg(PATTERN, &options.to_string(), cell, operation)
                .unwrap();
        assert!(svg.contains(cell) && svg.contains(operation));
    }
    assert!(
        ftx_wasm::manufacturing::sew_cell_svg(PATTERN, &options.to_string(), "cell-a", "reinforce")
            .is_err()
    );
    let mut stale = options;
    stale["production"]["routing"]["cells"][1]["configuration"]["machine"]["max_stitches_per_minute"] =
        json!(1.0);
    assert!(ftx_wasm::manufacturing::sew_plan(PATTERN, &stale.to_string()).is_err());
}

#[test]
fn authored_route_selection_replays_native_portable_stitches_yarn_orders_and_material_studies() {
    let mut original = routed_specialist_options();
    original["production"]["routing"]["cells"][1]["available_at_milliseconds"] = json!(18000);
    refresh_routed_locks(&mut original);
    let mut alternative = original.clone();
    let route = &mut alternative["production"]["routing"];
    route["cells"][1]["id"] = json!("cell-c");
    route["cells"][1]["available_at_milliseconds"] = json!(0);
    route["cells"][1]["configuration"]["recipes"]["reinforce"]["max_stitch_mm"] = json!(4.0);
    route["route"][1]["cell"] = json!("cell-c");
    route["transfer"]["transfers"][0]["to"] = json!("cell-c");
    refresh_routed_locks(&mut alternative);
    let request = json!({"schema":"ftx-sewing-selection/6","options":original,
        "objective":"minimize_parallel_makespan","max_plan_evaluations":2,
        "route_candidates":[{"id":"available-specialists","routing":alternative["production"]["routing"]}]});
    let selected_text = ftx_wasm::sew_select(PATTERN, &request.to_string()).unwrap();
    assert_eq!(
        selected_text,
        ftx_export::sewing_selection::select_json(PATTERN, &request.to_string()).unwrap()
    );
    let selected = portable(selected_text);
    assert_eq!(
        selected["report"]["selected_route"],
        "available-specialists"
    );
    assert_eq!(selected["report"]["candidate_combinations"], 2);
    let options = selected["selected_options_json"].as_str().unwrap();
    let plan = &selected["report"]["selected_plan"];
    assert_eq!(
        portable(ftx_wasm::manufacturing::sew_plan(PATTERN, options).unwrap()),
        *plan
    );
    let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, options).unwrap());
    assert_eq!(program["plan"], *plan);
    assert_eq!(program["stitches"].as_array().unwrap().len(), 130);
    assert!(
        program["stitches"]
            .as_array()
            .unwrap()
            .iter()
            .all(|stitch| { matches!(stitch["cell"].as_str(), Some("cell-a" | "cell-c")) })
    );
    assert_eq!(plan["consumables"]["complete_consumption_coverage"], true);
    assert!(
        plan["consumables"]["purchasing"]["least_observed_independent_order_total"]
            .as_f64()
            .is_some()
    );
    let svg =
        ftx_wasm::manufacturing::sew_cell_svg(PATTERN, options, "cell-c", "reinforce").unwrap();
    assert!(svg.contains("cell-c") && svg.contains("reinforce"));
    let pattern: Value = serde_json::from_str(PATTERN).unwrap();
    let study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"specialists","changes":{"schema":"ftx-material-bindings/1","default":pattern["fabric"]["material"]}}],
        "sewing_selection":request,
        "comparison":{"basis":"nominal","required_consumers":["sewing","sewing_consumables","sewing_purchasing"],
            "objectives":[{"metric":"sewing_parallel_makespan_seconds","direction":"minimize"}]}});
    let report = portable(ftx_wasm::materials::study(PATTERN, &study.to_string()).unwrap());
    let row = &report["alternatives"][0];
    assert_eq!(
        row["sewing_selection"]["result"]["selected_route"],
        "available-specialists"
    );
    assert_eq!(
        row["sewing_selection"]["result"]["trials"],
        selected["report"]["trials"]
    );
    assert_eq!(
        row["sewing"]["result"]["production"]["routing"]["selected"],
        plan["production"]["routing"]["selected"]
    );
    assert_eq!(row["sewing"]["result"]["consumables"], plan["consumables"]);
    assert_eq!(report["comparison"]["frontier"], json!(["specialists"]));
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    fs::write(directory.join("request.json"), request.to_string()).unwrap();
    fs::write(directory.join("selected-options.json"), options).unwrap();
    fs::write(directory.join("study.json"), study.to_string()).unwrap();
    for main in [false, true] {
        assert_eq!(
            run(
                main,
                &directory,
                "sew-select",
                "pattern.json",
                "request.json"
            ),
            selected
        );
        assert_eq!(
            run(
                main,
                &directory,
                "sew-plan",
                "pattern.json",
                "selected-options.json"
            ),
            *plan
        );
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "selected-options.json"
            ),
            program
        );
        assert_eq!(
            run(main, &directory, "study", "pattern.json", "study.json"),
            report
        );
    }
}

#[test]
fn shared_operator_replays_actual_heterogeneous_geometry_inventory_and_bound_material_study() {
    for windowed in [false, true] {
        let directory = workspace();
        let options = shared_operator_options(windowed);
        let text = options.to_string();
        let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, &text).unwrap());
        let plan = &program["plan"];
        let parallel = &plan["production"]["parallel"];
        assert_eq!(parallel["schema"], "ftx-sewing-parallel/4");
        assert_eq!(
            parallel["selected"]["makespan_milliseconds"],
            if windowed { 11104 } else { 9900 }
        );
        assert_eq!(
            parallel["shared_operator"]["serial_order_scope"],
            "retained_serial_incumbents/1"
        );
        assert_eq!(parallel["shared_operator"]["baseline_replaced"], windowed);
        assert_eq!(
            program["stitches"].as_array().unwrap().len(),
            if windowed { 120 } else { 160 }
        );
        let yarn = plan["consumables"]["total_required_length_m"]
            .as_f64()
            .unwrap();
        assert!((yarn - if windowed { 2.4 } else { 3.0 }).abs() < 1e-12);
        let mut free = 0;
        for phase in parallel["shared_operator"]["selected"]["steps"]
            .as_array()
            .unwrap()
        {
            assert_eq!(phase["departure_milliseconds"], free);
            free = phase["operator_release_milliseconds"].as_u64().unwrap();
            if let Some(copy) = phase["copy"].as_u64() {
                assert!(
                    program["stitches"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|step| step["cell"] == phase["cell"] && step["copy"] == copy)
                );
            }
        }
        assert_eq!(parallel["selected"]["makespan_milliseconds"], free);
        assert_eq!(
            portable(
                ftx_wasm::manufacturing::sew_plan(PATTERN, &plan["options"].to_string()).unwrap()
            ),
            *plan
        );
        let pattern: Value = serde_json::from_str(PATTERN).unwrap();
        let study = json!({"schema":"ftx-material-study/1",
            "variants":[{"key":"one-worker","changes":{"schema":"ftx-material-bindings/1",
                "default":pattern["fabric"]["material"]}}],"sewing":options,
            "comparison":{"basis":"nominal","required_consumers":["sewing","sewing_consumables","sewing_purchasing"],
                "objectives":[{"metric":"sewing_parallel_makespan_seconds","direction":"minimize"}]}});
        let report = portable(ftx_wasm::materials::study(PATTERN, &study.to_string()).unwrap());
        let row = &report["alternatives"][0];
        let bound = row["binding"]["result"]["pattern_json"].as_str().unwrap();
        let rebound = portable(ftx_wasm::manufacturing::sew_plan(bound, &text).unwrap());
        assert_eq!(row["sewing"]["result"], rebound);
        assert_eq!(
            rebound["production"]["parallel"]["shared_operator"],
            parallel["shared_operator"]
        );
        assert_eq!(
            row["metrics"]["sewing_parallel_makespan_seconds"],
            if windowed { 11.104 } else { 9.9 }
        );
        assert_eq!(report["comparison"]["frontier"], json!(["one-worker"]));
        fs::write(directory.join("pattern.json"), PATTERN).unwrap();
        fs::write(directory.join("options.json"), &text).unwrap();
        fs::write(directory.join("study.json"), study.to_string()).unwrap();
        fs::write(directory.join("bound.json"), bound).unwrap();
        for main in [false, true] {
            assert_eq!(
                run(main, &directory, "sew-plan", "pattern.json", "options.json"),
                *plan
            );
            assert_eq!(
                run(
                    main,
                    &directory,
                    "sew-stitches",
                    "pattern.json",
                    "options.json"
                ),
                program
            );
            assert_eq!(
                run(main, &directory, "study", "pattern.json", "study.json"),
                report
            );
            assert_eq!(
                run(main, &directory, "sew-plan", "bound.json", "options.json"),
                rebound
            );
        }
        let mut refused_study = study;
        refused_study["sewing"]["production"]["parallel"]["shared_operator"]["available_until_milliseconds"] =
            json!(0);
        assert!(
            ftx_wasm::manufacturing::sew_plan(PATTERN, &refused_study["sewing"].to_string())
                .is_err()
        );
        let refused =
            portable(ftx_wasm::materials::study(PATTERN, &refused_study.to_string()).unwrap());
        assert_eq!(refused["alternatives"][0]["sewing"]["status"], "refused");
        assert!(
            refused["comparison"]["frontier"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn shared_fixture_pools_change_actual_cell_geometry_and_replay_through_both_commands() {
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    for (capacity, milliseconds, intervals, yarn_m) in [(1, 11104, 120, 2.4), (2, 9900, 160, 3.0)] {
        let mut options = shared_operator_options(false);
        let parallel = &mut options["production"]["parallel"];
        parallel["independence"] =
            json!("independent_machines_shared_operator_and_workholding_pools/1");
        parallel["source"]["note"] = json!(
            "Synthetic independent machines with one shared worker and the explicitly mapped jig pool"
        );
        parallel["method"] = json!(
            "Synthetic whole-garment cells sharing the declared operator and pooled reusable fixtures"
        );
        parallel["conditions"] =
            json!({"sharing":"Only the declared jig units and operator are shared"});
        for cell in parallel["cells"].as_array_mut().unwrap() {
            cell["workholding"]["resources"][0]["capacity"] = json!(1);
            cell["workholding"]["source"]["note"] = json!(
                "Synthetic local concurrent-hold limit; units come from the explicitly shared pool"
            );
            cell["handling"]["method"] = json!(
                "Synthetic source includes local pool access and return in existing handling; a jig is available when the release feed completes"
            );
            cell["handling"]["source"]["note"] = json!(
                "Synthetic phase durations attended by the common operator, including declared pool movement"
            );
            cell["ordering"]["setup_times"]["method"] = json!(
                "Synthetic complete needle/thread changes attended by the common operator, excluding feed and handling"
            );
        }
        parallel["shared_workholding"] = json!({
            "model":"shared_workholding_pools/1",
            "release_protocol":"available_at_feed_completion_with_access_in_cell_handling/1",
            "cells":{},"resources":[{"id":"shared-jigs","capacity":capacity}],
            "bindings":[{"cell":"cell-a","resource":"jigs-a","pool":"shared-jigs"},
                {"cell":"cell-b","resource":"jigs-b","pool":"shared-jigs"}],
            "source":{"reference":"test:native-shared-fixture-pool","accessed_on":"2026-10-27",
                "note":"Invented common fixture inventory; no measured handling or hardware qualification"},
            "method":"Continuous shared occupancy from initial handling until reinforcement feed completion; pool access and return included in each cell handling source",
            "evidence":"estimate","allow_estimates":true
        });
        refresh_shared_cell_locks(&mut options);
        let text = options.to_string();
        let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, &text).unwrap());
        let plan = &program["plan"];
        let parallel = &plan["production"]["parallel"];
        // Cell B's six feeds each ceil to 1334 ms. One jig makes its three complete
        // garments preferable to the A/B split; two jigs admit A's 9900 ms completion.
        assert_eq!(parallel["selected"]["makespan_milliseconds"], milliseconds);
        assert_eq!(program["stitches"].as_array().unwrap().len(), intervals);
        assert!(
            (plan["consumables"]["total_required_length_m"]
                .as_f64()
                .unwrap()
                - yarn_m)
                .abs()
                < 1e-12
        );
        assert_eq!(
            parallel["selected"]["cells"][0]["copies"],
            if capacity == 1 { json!([]) } else { json!([0]) }
        );
        assert_eq!(
            parallel["selected"]["cells"][1]["copies"],
            if capacity == 1 {
                json!([0, 1, 2])
            } else {
                json!([1, 2])
            }
        );
        let pools = &parallel["shared_workholding"];
        assert_eq!(pools["schema"], "ftx-sewing-shared-workholding/1");
        assert_eq!(pools["resources"][0]["capacity"], capacity);
        assert_eq!(pools["selected"]["peak_occupancy"]["shared-jigs"], capacity);
        assert_eq!(pools["selected"]["final_occupancy"]["shared-jigs"], 0);
        assert_eq!(pools["physical_pool_access_validated"], false);
        let holds = pools["selected"]["holds"].as_array().unwrap();
        let phases = parallel["shared_operator"]["selected"]["steps"]
            .as_array()
            .unwrap();
        assert_eq!(holds.len(), 3);
        let mut events = Vec::new();
        let mut copies = std::collections::BTreeSet::new();
        for hold in holds {
            assert!(copies.insert(hold["copy"].as_u64().unwrap()));
            assert_eq!(hold["pool"], "shared-jigs");
            assert_eq!(hold["units"], 1);
            let acquire = phases
                .iter()
                .find(|phase| {
                    phase["cell"] == hold["cell"] && phase["operation"] == hold["acquire"]
                })
                .unwrap();
            let release = phases
                .iter()
                .find(|phase| {
                    phase["cell"] == hold["cell"] && phase["operation"] == hold["release"]
                })
                .unwrap();
            assert_eq!(acquire["copy"], hold["copy"]);
            assert_eq!(release["copy"], hold["copy"]);
            assert_eq!(
                hold["acquired_at_milliseconds"],
                acquire["handling_start_milliseconds"]
            );
            assert_eq!(
                hold["released_at_milliseconds"],
                release["feed_finish_milliseconds"]
            );
            events.push((hold["acquired_at_milliseconds"].as_u64().unwrap(), 1));
            events.push((hold["released_at_milliseconds"].as_u64().unwrap(), -1));
        }
        assert_eq!(copies, std::collections::BTreeSet::from([0, 1, 2]));
        // Independent interval sweep: completion releases precede equal-time acquisitions.
        events.sort_unstable();
        let mut occupied = 0;
        for (_, change) in events {
            occupied += change;
            assert!((0..=capacity).contains(&occupied));
        }
        assert_eq!(occupied, 0);
        assert_eq!(plan["hardware_execution_authorized"], false);
        let replay = plan["options"].to_string();
        assert_eq!(
            portable(ftx_wasm::manufacturing::sew_plan(PATTERN, &replay).unwrap()),
            *plan
        );
        assert_eq!(
            portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, &replay).unwrap()),
            program
        );

        let pattern: Value = serde_json::from_str(PATTERN).unwrap();
        let study = json!({"schema":"ftx-material-study/1",
            "variants":[{"key":"pooled-fixtures","changes":{"schema":"ftx-material-bindings/1",
                "default":pattern["fabric"]["material"]}}],"sewing":options,
            "comparison":{"basis":"nominal","required_consumers":["sewing","sewing_consumables","sewing_purchasing"],
                "objectives":[{"metric":"sewing_parallel_makespan_seconds","direction":"minimize"}]}});
        let report = portable(ftx_wasm::materials::study(PATTERN, &study.to_string()).unwrap());
        let row = &report["alternatives"][0];
        let bound = row["binding"]["result"]["pattern_json"].as_str().unwrap();
        let rebound = portable(ftx_wasm::manufacturing::sew_plan(bound, &text).unwrap());
        assert_eq!(row["sewing"]["result"], rebound);
        assert_eq!(
            rebound["production"]["parallel"]["shared_workholding"],
            *pools
        );
        assert_eq!(
            row["metrics"]["sewing_parallel_makespan_seconds"],
            f64::from(milliseconds) / 1000.0
        );
        assert_eq!(report["comparison"]["frontier"], json!(["pooled-fixtures"]));
        let filename = format!("shared-fixtures-{capacity}.json");
        let study_filename = format!("shared-fixture-study-{capacity}.json");
        let replay_filename = format!("shared-fixture-replay-{capacity}.json");
        fs::write(directory.join(&filename), &text).unwrap();
        fs::write(directory.join(&study_filename), study.to_string()).unwrap();
        fs::write(directory.join(&replay_filename), &replay).unwrap();
        for main in [false, true] {
            assert_eq!(
                run(main, &directory, "sew-plan", "pattern.json", &filename),
                *plan
            );
            assert_eq!(
                run(main, &directory, "sew-stitches", "pattern.json", &filename),
                program
            );
            assert_eq!(
                run(
                    main,
                    &directory,
                    "sew-stitches",
                    "pattern.json",
                    &replay_filename
                ),
                program
            );
            assert_eq!(
                run(main, &directory, "study", "pattern.json", &study_filename),
                report
            );
        }

        // Even a cell left idle by the scarce-pool allocation remains source-locked.
        // Updating the operator lock alone must not bless an old fixture qualification.
        let mut changed = options.clone();
        changed["production"]["parallel"]["cells"][0]["handling"]["initial_milliseconds"]["join"] =
            json!(101);
        let stale_pool_locks =
            changed["production"]["parallel"]["shared_workholding"]["cells"].clone();
        refresh_shared_cell_locks(&mut changed);
        changed["production"]["parallel"]["shared_workholding"]["cells"] = stale_pool_locks;
        assert!(ftx_wasm::manufacturing::sew_plan(PATTERN, &changed.to_string()).is_err());
        assert!(ftx_wasm::manufacturing::sew_stitches(PATTERN, &changed.to_string()).is_err());
        let refused_filename = format!("shared-fixture-stale-{capacity}.json");
        fs::write(directory.join(&refused_filename), changed.to_string()).unwrap();
        for main in [false, true] {
            for action in ["sew-plan", "sew-stitches"] {
                let mut command = Command::new(if main {
                    env!("CARGO_BIN_EXE_ftx")
                } else {
                    env!("CARGO_BIN_EXE_ftx-materials")
                });
                command.current_dir(&directory);
                if main {
                    command.args(["--json", "materials"]);
                }
                let refused = command
                    .args([action, "pattern.json", &refused_filename])
                    .output()
                    .unwrap();
                assert!(!refused.status.success());
                let message = format!(
                    "{}{}",
                    String::from_utf8_lossy(&refused.stdout),
                    String::from_utf8_lossy(&refused.stderr)
                );
                assert!(message.contains("content lock mismatch"), "{message}");
            }
        }
        let mut refused_study = study;
        refused_study["sewing"] = changed;
        let refused =
            portable(ftx_wasm::materials::study(PATTERN, &refused_study.to_string()).unwrap());
        assert_eq!(refused["alternatives"][0]["sewing"]["status"], "refused");
        assert_eq!(refused["comparison"]["frontier"], json!([]));
        assert_eq!(fs::read_to_string(directory.join(filename)).unwrap(), text);
    }
    assert_eq!(
        fs::read_to_string(directory.join("pattern.json")).unwrap(),
        PATTERN
    );
}

#[test]
fn finite_cell_windows_replay_actual_assignment_and_inventory_in_both_commands() {
    let directory = workspace();
    let mut options: Value = serde_json::from_str(HETEROGENEOUS).unwrap();
    options["production"]["parallel"]["cells"][0]["available_until_milliseconds"] = json!(9899);
    options["production"]["parallel"]["cells"][1]["available_until_milliseconds"] = json!(11100);
    let text = options.to_string();
    let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, &text).unwrap());
    let plan = &program["plan"];
    let parallel = &plan["production"]["parallel"];
    assert_eq!(parallel["schema"], "ftx-sewing-parallel/3");
    assert_eq!(parallel["selected"]["makespan_milliseconds"], 11100);
    assert_eq!(parallel["availability"]["baseline_replaced"], true);
    assert_eq!(parallel["selected"]["cells"][0]["copies"], json!([]));
    assert_eq!(parallel["selected"]["cells"][1]["copies"], json!([0, 1, 2]));
    assert_eq!(program["stitches"].as_array().unwrap().len(), 120);
    assert!(
        (plan["consumables"]["total_required_length_m"]
            .as_f64()
            .unwrap()
            - 2.4)
            .abs()
            < 1e-12
    );
    assert_eq!(
        portable(ftx_wasm::manufacturing::sew_plan(PATTERN, &plan["options"].to_string()).unwrap()),
        *plan
    );
    let pattern: Value = serde_json::from_str(PATTERN).unwrap();
    let study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"window-qualified","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern["fabric"]["material"]}}],"sewing":options,
        "comparison":{"basis":"nominal","required_consumers":["sewing","sewing_consumables","sewing_purchasing"],
            "objectives":[{"metric":"sewing_parallel_makespan_seconds","direction":"minimize"}]}});
    let report = portable(ftx_wasm::materials::study(PATTERN, &study.to_string()).unwrap());
    let row = &report["alternatives"][0];
    // Binding freezes the selected material into a new pattern identity. Replay that exact
    // document so configured cell plans retain the correct source hash.
    let bound_plan = portable(
        ftx_wasm::manufacturing::sew_plan(
            row["binding"]["result"]["pattern_json"].as_str().unwrap(),
            &options.to_string(),
        )
        .unwrap(),
    );
    assert_eq!(
        row["sewing"]["result"]["production"],
        bound_plan["production"]
    );
    assert_eq!(
        report["alternatives"][0]["sewing"]["result"]["consumables"],
        plan["consumables"]
    );
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    fs::write(directory.join("options.json"), text).unwrap();
    fs::write(directory.join("study.json"), study.to_string()).unwrap();
    for main in [false, true] {
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "options.json"
            ),
            program
        );
        assert_eq!(
            run(main, &directory, "study", "pattern.json", "study.json"),
            report
        );
    }
    // A limit 1 ms before the only remaining cell's completion invalidates the entire sewing
    // consumer; neither the native portable plan nor the study may reuse the earlier assignment.
    let mut too_short = options;
    too_short["production"]["parallel"]["cells"][1]["available_until_milliseconds"] = json!(11099);
    assert!(ftx_wasm::manufacturing::sew_plan(PATTERN, &too_short.to_string()).is_err());
    let mut refused_study = study;
    refused_study["sewing"] = too_short;
    let refused =
        portable(ftx_wasm::materials::study(PATTERN, &refused_study.to_string()).unwrap());
    assert_eq!(refused["alternatives"][0]["sewing"]["status"], "refused");
    assert!(
        refused["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn heterogeneous_cells_replay_actual_stitches_and_combined_thread_orders_through_studies() {
    let directory = workspace();
    let options: Value = serde_json::from_str(HETEROGENEOUS).unwrap();
    let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, HETEROGENEOUS).unwrap());
    let plan = &program["plan"];
    let parallel = &plan["production"]["parallel"];
    assert_eq!(parallel["schema"], "ftx-sewing-parallel/2");
    assert_eq!(parallel["optimality_proven"], true);
    assert_eq!(parallel["selected"]["makespan_milliseconds"], 9900);
    let steps = program["stitches"].as_array().unwrap();
    assert_eq!(steps.len(), 160);
    let feed: f64 = steps
        .iter()
        .map(|step| step["feed_seconds"].as_f64().unwrap())
        .sum();
    assert!((feed - plan["production"]["feed_seconds"].as_f64().unwrap()).abs() < 1e-10);
    for cell in parallel["selected"]["cells"].as_array().unwrap() {
        let copies = cell["copies"].as_array().unwrap();
        if copies.is_empty() {
            continue;
        }
        let source = parallel["cell_plans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| source["cell"] == cell["cell"])
            .unwrap();
        let policy = options["production"]["parallel"]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .find(|policy| policy["id"] == cell["cell"])
            .unwrap();
        let mut local = source["plan"]["options"].clone();
        local["production"] = json!({"production_copies":copies.len(),"ordering":policy["ordering"],
            "handling":policy["handling"],"workholding":policy["workholding"]});
        local["production"]["ordering"]["max_search_nodes"] = cell["schedule_node_limit"].clone();
        let replay =
            portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, &local.to_string()).unwrap());
        assert_eq!(replay["plan"]["production"], cell["schedule"]);
        let expected: Vec<_> = replay["stitches"]
            .as_array()
            .unwrap()
            .iter()
            .map(|step| {
                let mut global = step.clone();
                global["copy"] = copies[step["copy"].as_u64().unwrap() as usize].clone();
                global["cell"] = cell["cell"].clone();
                global
            })
            .collect();
        assert_eq!(
            steps
                .iter()
                .filter(|step| step["cell"] == cell["cell"])
                .cloned()
                .collect::<Vec<_>>(),
            expected
        );
    }
    let consumption = &plan["consumables"];
    assert_eq!(consumption["complete_consumption_coverage"], true);
    assert!((consumption["total_required_length_m"].as_f64().unwrap() - 3.0).abs() < 1e-12);
    assert_eq!(
        consumption["material_requirements"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        consumption["purchasing"]["material_orders"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        consumption["purchasing"]["least_observed_independent_order_total"],
        14.0
    );
    let pattern: Value = serde_json::from_str(PATTERN).unwrap();
    let mut study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"heterogeneous","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern["fabric"]["material"]}}],"sewing":options,
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[],"seam_allowance_mm":0},
        "comparison":{"basis":"nominal","required_consumers":["sewing","sewing_consumables","sewing_purchasing"],
            "objectives":[{"metric":"sewing_parallel_makespan_seconds","direction":"minimize"},
                {"metric":"thread_required_length_m","direction":"minimize"},
                {"metric":"thread_required_mass_kg","direction":"minimize"},
                {"metric":"thread_observed_order_cost","direction":"minimize"}]}});
    let report = portable(ftx_wasm::materials::study(PATTERN, &study.to_string()).unwrap());
    let row = &report["alternatives"][0];
    let bound_plan = portable(
        ftx_wasm::manufacturing::sew_plan(
            row["binding"]["result"]["pattern_json"].as_str().unwrap(),
            &options.to_string(),
        )
        .unwrap(),
    );
    assert_eq!(
        row["sewing"]["result"]["production"],
        bound_plan["production"]
    );
    assert_eq!(row["sewing"]["result"]["consumables"], *consumption);
    assert_eq!(
        row["metrics"]["feed_seconds"],
        plan["production"]["feed_seconds"]
    );
    assert_eq!(row["metrics"]["stitch_intervals"], 160);
    assert!((row["metrics"]["thread_required_length_m"].as_f64().unwrap() - 3.0).abs() < 1e-12);
    assert_eq!(row["metrics"]["thread_observed_order_cost"], 14.0);
    assert_eq!(report["comparison"]["frontier"], json!(["heterogeneous"]));
    for (name, text) in [
        ("pattern.json", PATTERN.to_owned()),
        ("options.json", HETEROGENEOUS.to_owned()),
        ("program.json", program.to_string()),
        ("study.json", study.to_string()),
        ("study-report.json", report.to_string()),
    ] {
        fs::write(directory.join(name), text).unwrap();
    }
    for main in [false, true] {
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "options.json"
            ),
            program
        );
        assert_eq!(
            run(main, &directory, "study", "pattern.json", "study.json"),
            report
        );
    }
    eprintln!("heterogeneous production replay: {}", directory.display());
    study["sewing"]["production"]["parallel"]["cells"][0]["configuration"]["machine"]["max_stitches_per_minute"] =
        json!(1.0);
    let refused = portable(ftx_wasm::materials::study(PATTERN, &study.to_string()).unwrap());
    assert_eq!(refused["alternatives"][0]["sewing"]["status"], "refused");
    assert_eq!(
        refused["alternatives"][0]["assembly_mass"]["status"],
        "available"
    );
}

#[test]
fn heterogeneous_recipe_selection_uses_cell_setup_domains_and_refuses_other_allocation_optima() {
    let options: Value = serde_json::from_str(HETEROGENEOUS).unwrap();
    let mut request = json!({"schema":"ftx-sewing-selection/5","options":options,
        "objective":"minimize_parallel_makespan","max_plan_evaluations":2,
        "alternatives":[{"operation":"join","recipes":[{"id":"faster-qualified",
            "needle":"example-needle","thread":"example-thread-a","max_stitch_mm":2.5,"stitches_per_minute":1000.0}]}]});
    let result = portable(ftx_wasm::sew_select(PATTERN, &request.to_string()).unwrap());
    assert!(result["report"]["selected_plan"].is_object());
    assert_eq!(result["report"]["search_complete"], true);
    let selected = result["selected_options_json"].as_str().unwrap();
    assert_eq!(
        portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, selected).unwrap())["plan"],
        result["report"]["selected_plan"]
    );
    for (schema, objective) in [
        ("ftx-sewing-selection/1", "minimize_feed_seconds"),
        (
            "ftx-sewing-selection/3",
            "minimize_thread_observed_order_cost",
        ),
    ] {
        request["schema"] = json!(schema);
        request["objective"] = json!(objective);
        let refusal = ftx_wasm::sew_select(PATTERN, &request.to_string()).unwrap_err();
        assert!(
            refusal.message.contains("depend on that allocation"),
            "{refusal}"
        );
    }
}

#[test]
fn parallel_cells_replay_through_native_commands_studies_and_each_independent_cell() {
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    fs::write(directory.join("parallel.json"), PARALLEL).unwrap();
    let plan = portable(ftx_wasm::manufacturing::sew_plan(PATTERN, PARALLEL).unwrap());
    let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, PARALLEL).unwrap());
    let parallel = &plan["production"]["parallel"];
    assert_eq!(plan["schema"], "ftx-sewing-plan/16");
    assert_eq!(program["schema"], "ftx-sewing-program/5");
    assert_eq!(program["plan"], plan);
    assert_eq!(parallel["selected"]["makespan_milliseconds"], 25500);
    assert_eq!(parallel["baseline"]["makespan_milliseconds"], 41100);
    assert_eq!(parallel["optimality_proven"], true);
    assert_eq!(program["stitches"].as_array().unwrap().len(), 240);
    for cell in parallel["selected"]["cells"].as_array().unwrap() {
        let copies = cell["copies"].as_array().unwrap();
        if copies.is_empty() {
            continue;
        }
        let source = plan["options"]["production"]["parallel"]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| source["id"] == cell["cell"])
            .unwrap();
        let mut options = plan["options"].clone();
        options["production"]
            .as_object_mut()
            .unwrap()
            .remove("parallel");
        options["production"]["production_copies"] = json!(copies.len());
        options["production"]["ordering"] = source["ordering"].clone();
        options["production"]["ordering"]["max_search_nodes"] = cell["schedule_node_limit"].clone();
        options["production"]["handling"] = source["handling"].clone();
        options["production"]["workholding"] = source["workholding"].clone();
        let replay =
            portable(ftx_wasm::manufacturing::sew_plan(PATTERN, &options.to_string()).unwrap());
        assert_eq!(replay["production"], cell["schedule"]);
        let stream: Vec<_> = program["stitches"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|step| step["cell"] == cell["cell"])
            .collect();
        assert_eq!(stream.len(), copies.len() * 80);
        assert!(stream.iter().all(|step| copies.contains(&step["copy"])));
    }
    let pattern: Value = serde_json::from_str(PATTERN).unwrap();
    let mut study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"cells","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern["fabric"]["material"]}}],
        "sewing":serde_json::from_str::<Value>(PARALLEL).unwrap(),
        "comparison":{"basis":"nominal","required_consumers":["sewing"],
            "objectives":[{"metric":"sewing_parallel_makespan_seconds","direction":"minimize"}],
            "constraints":[{"metric":"sewing_parallel_makespan_seconds","maximum":26.0}]}});
    let report = portable(ftx_wasm::materials::study(PATTERN, &study.to_string()).unwrap());
    let row = &report["alternatives"][0];
    assert_eq!(row["sewing"]["result"]["production"], plan["production"]);
    assert_eq!(row["metrics"]["sewing_parallel_makespan_seconds"], 25.5);
    assert_eq!(row["metrics"]["feed_seconds"], 24.0);
    assert!(row["metrics"]["sewing_modeled_production_seconds"].is_null());
    assert!(row["metrics"]["sewing_setup_seconds"].is_null());
    assert_eq!(report["comparison"]["frontier"], json!(["cells"]));
    fs::write(directory.join("study.json"), study.to_string()).unwrap();
    for main in [false, true] {
        assert_eq!(
            run(
                main,
                &directory,
                "sew-plan",
                "pattern.json",
                "parallel.json"
            ),
            plan
        );
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "parallel.json"
            ),
            program
        );
        assert_eq!(
            run(main, &directory, "study", "pattern.json", "study.json"),
            report
        );
    }
    study["sewing"]["production"]["parallel"]["allow_estimates"] = json!(false);
    let refused = portable(ftx_wasm::materials::study(PATTERN, &study.to_string()).unwrap());
    assert_eq!(refused["alternatives"][0]["binding"]["status"], "available");
    assert_eq!(refused["alternatives"][0]["sewing"]["status"], "refused");
    assert!(refused["alternatives"][0]["metrics"]["sewing_parallel_makespan_seconds"].is_null());
    assert_eq!(refused["comparison"]["frontier"], json!([]));
    assert_eq!(
        fs::read_to_string(directory.join("parallel.json")).unwrap(),
        PARALLEL
    );
}

#[test]
fn parallel_recipe_selection_uses_elapsed_completion_and_preserves_independent_consumer_failures() {
    let directory = workspace();
    let options: Value = serde_json::from_str(PARALLEL).unwrap();
    let mut request = json!({"schema":"ftx-sewing-selection/5","options":options,
        "objective":"minimize_parallel_makespan","max_plan_evaluations":2,
        "alternatives":[{"operation":"join","recipes":[{"id":"faster-qualified",
            "needle":"example-needle","thread":"example-thread-a",
            "max_stitch_mm":2.5,"stitches_per_minute":1000.0}]}]});
    let result = portable(ftx_wasm::sew_select(PATTERN, &request.to_string()).unwrap());
    let report = &result["report"];
    assert_eq!(report["schema"], "ftx-sewing-selection-report/5");
    assert_eq!(
        report["selected_choices"],
        json!({"join":"faster-qualified"})
    );
    assert_eq!(report["optimality_proven"], true);
    assert_eq!(report["trials"][0]["objective_seconds"], 25.5);
    assert_eq!(report["selected_objective_seconds"], 22.3);
    let selected = result["selected_options_json"].as_str().unwrap();
    let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, selected).unwrap());
    assert_eq!(program["plan"], report["selected_plan"]);
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    fs::write(directory.join("selection.json"), request.to_string()).unwrap();
    fs::write(directory.join("selected.json"), selected).unwrap();
    for main in [false, true] {
        assert_eq!(
            run(
                main,
                &directory,
                "sew-select",
                "pattern.json",
                "selection.json"
            ),
            result
        );
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "selected.json"
            ),
            program
        );
    }
    // Forty allocation transitions, six count trials and three reserved schedule replays.
    request["options"]["production"]["parallel"]["max_search_nodes"] = json!(49);
    let bounded = portable(ftx_wasm::sew_select(PATTERN, &request.to_string()).unwrap());
    assert_eq!(bounded["report"]["search_complete"], true);
    assert_eq!(bounded["report"]["optimality_proven"], false);
    assert!(bounded["report"]["unresolved_trials"].as_u64().unwrap() > 0);
    assert!(bounded["report"]["selected_plan"].is_object());
    request["options"] = options.clone();
    request["max_plan_evaluations"] = json!(64);
    assert_eq!(
        ftx_wasm::sew_select(PATTERN, &request.to_string())
            .unwrap_err()
            .code,
        ftx_core::RefusalCode::BudgetExceeded
    );
    request["max_plan_evaluations"] = json!(2);
    // Invalid scheduling is one independent consumer, not a reason to erase complete assembly mass.
    request["options"]["ordering"] = options["production"]["ordering"].clone();
    let pattern: Value = serde_json::from_str(PATTERN).unwrap();
    let study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"independent","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern["fabric"]["material"]}}],"sewing_selection":request,
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,
            "extras":[],"seam_allowance_mm":0}});
    let outcome = portable(ftx_wasm::materials::study(PATTERN, &study.to_string()).unwrap());
    assert_eq!(
        outcome["alternatives"][0]["assembly_mass"]["status"],
        "available"
    );
    assert_eq!(outcome["alternatives"][0]["sewing"]["status"], "refused");
    assert!(outcome["alternatives"][0]["metrics"]["sewing_parallel_makespan_seconds"].is_null());
}

#[test]
fn both_native_plan_and_stitch_commands_execute_the_documented_batch_and_match_portable() {
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    fs::write(directory.join("options.json"), OPTIONS).unwrap();
    let plan = portable(ftx_wasm::manufacturing::sew_plan(PATTERN, OPTIONS).unwrap());
    let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, OPTIONS).unwrap());
    assert_eq!(plan["schema"], "ftx-sewing-plan/12");
    assert_eq!(plan["production"]["production_copies"], 3);
    assert_eq!(plan["production"]["stitch_intervals"], 240);
    assert_eq!(plan["production"]["feed_seconds"], 24.0);
    assert_eq!(
        plan["production"]["ordering"]["timing"]["feed_and_setup_seconds"],
        25.5
    );
    assert_eq!(program["schema"], "ftx-sewing-program/4");
    assert_eq!(program["stitches"].as_array().unwrap().len(), 240);
    assert_eq!(program["plan"], plan);
    for main in [false, true] {
        assert_eq!(
            run(main, &directory, "sew-plan", "pattern.json", "options.json"),
            plan
        );
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "options.json"
            ),
            program
        );
    }
    assert_eq!(
        fs::read_to_string(directory.join("pattern.json")).unwrap(),
        PATTERN
    );
    assert_eq!(
        fs::read_to_string(directory.join("options.json")).unwrap(),
        OPTIONS
    );
}

#[test]
fn finite_fixture_inventory_changes_both_native_schedules_and_the_real_stitch_program() {
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    for (capacity, setup_seconds) in [(1, 6.5), (2, 4.0), (3, 1.5)] {
        let mut options: Value = serde_json::from_str(WORKHOLDING).unwrap();
        options["production"]["workholding"]["resources"][0]["capacity"] = json!(capacity);
        let text = options.to_string();
        let filename = format!("fixtures-{capacity}.json");
        fs::write(directory.join(&filename), &text).unwrap();
        let plan = portable(ftx_wasm::manufacturing::sew_plan(PATTERN, &text).unwrap());
        let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, &text).unwrap());
        assert_eq!(plan["schema"], "ftx-sewing-plan/13");
        assert_eq!(plan["production"]["schema"], "ftx-sewing-production/2");
        assert_eq!(plan["production"]["stitch_intervals"], 240);
        assert_eq!(plan["production"]["feed_seconds"], 24.0);
        let ordering = &plan["production"]["ordering"];
        assert_eq!(ordering["optimality_proven"], true);
        assert_eq!(
            ordering["timing"]["feed_and_setup_seconds"],
            24.0 + setup_seconds
        );
        let workholding = &plan["production"]["workholding"];
        assert_eq!(workholding["schema"], "ftx-sewing-workholding/1");
        assert_eq!(workholding["fixture_geometry_and_access_validated"], false);
        assert_eq!(workholding["holds"].as_array().unwrap().len(), 3);
        let resource = workholding["resources"][0]["id"].as_str().unwrap();
        assert_eq!(workholding["baseline"]["peak_occupancy"][resource], 1);
        assert_eq!(
            workholding["selected"]["peak_occupancy"][resource],
            capacity
        );
        let timeline = workholding["selected"]["operations"].as_array().unwrap();
        assert_eq!(timeline.len(), 6);
        assert_eq!(timeline.last().unwrap()["occupied_after"][resource], 0);
        for (step, operation) in timeline
            .iter()
            .zip(ordering["selected_order"].as_array().unwrap())
        {
            assert_eq!(step["operation"], *operation);
            assert!(step["occupied_during"][resource].as_u64().unwrap() <= capacity);
        }
        assert_eq!(program["plan"], plan);
        assert_eq!(program["stitches"].as_array().unwrap().len(), 240);
        assert_eq!(plan["hardware_execution_authorized"], false);
        for main in [false, true] {
            assert_eq!(
                run(main, &directory, "sew-plan", "pattern.json", &filename),
                plan
            );
            assert_eq!(
                run(main, &directory, "sew-stitches", "pattern.json", &filename),
                program
            );
        }
        assert_eq!(fs::read_to_string(directory.join(filename)).unwrap(), text);
    }
    assert_eq!(
        fs::read_to_string(directory.join("pattern.json")).unwrap(),
        PATTERN
    );
}

#[test]
fn both_native_commands_and_portable_exports_enforce_the_same_planar_tool_sweep() {
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    fs::write(directory.join("access.json"), ACCESS).unwrap();
    let plan = portable(ftx_wasm::manufacturing::sew_plan(PATTERN, ACCESS).unwrap());
    let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, ACCESS).unwrap());
    assert_eq!(plan["schema"], "ftx-sewing-plan/14");
    assert_eq!(program["plan"], plan);
    assert_eq!(program["stitches"].as_array().unwrap().len(), 240);
    for seam in plan["seams"].as_array().unwrap() {
        assert_eq!(seam["access"]["minimum_clearance_mm"], 3.0);
        assert_eq!(seam["access"]["checked_segments"], 40);
        assert_eq!(seam["access"]["physical_access_validated"], false);
    }
    let mut collision: Value = serde_json::from_str(ACCESS).unwrap();
    collision["passes"][0]["access"]["tool_radius_mm"] = json!(6.0);
    let collision = collision.to_string();
    fs::write(directory.join("collision.json"), &collision).unwrap();
    assert!(
        ftx_wasm::manufacturing::sew_plan(PATTERN, &collision)
            .unwrap_err()
            .message
            .contains("planar access")
    );
    assert!(
        ftx_wasm::manufacturing::sew_stitches(PATTERN, &collision)
            .unwrap_err()
            .message
            .contains("planar access")
    );
    for main in [false, true] {
        for (action, expected) in [("sew-plan", &plan), ("sew-stitches", &program)] {
            assert_eq!(
                run(main, &directory, action, "pattern.json", "access.json"),
                *expected
            );
            let mut command = Command::new(if main {
                env!("CARGO_BIN_EXE_ftx")
            } else {
                env!("CARGO_BIN_EXE_ftx-materials")
            });
            command.current_dir(&directory);
            if main {
                command.args(["--json", "materials"]);
            }
            let refused = command
                .args([action, "pattern.json", "collision.json"])
                .output()
                .unwrap();
            assert!(!refused.status.success());
            let message = format!(
                "{}{}",
                String::from_utf8_lossy(&refused.stdout),
                String::from_utf8_lossy(&refused.stderr)
            );
            assert!(message.contains("planar access"), "{message}");
        }
    }
    assert_eq!(
        fs::read_to_string(directory.join("pattern.json")).unwrap(),
        PATTERN
    );
    assert_eq!(
        fs::read_to_string(directory.join("access.json")).unwrap(),
        ACCESS
    );
    assert_eq!(
        fs::read_to_string(directory.join("collision.json")).unwrap(),
        collision
    );
}

#[test]
fn qualified_handling_changes_the_actual_batch_and_replays_through_study_and_commands() {
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    fs::write(directory.join("handling.json"), HANDLING).unwrap();
    let plan = portable(ftx_wasm::manufacturing::sew_plan(PATTERN, HANDLING).unwrap());
    let program = portable(ftx_wasm::manufacturing::sew_stitches(PATTERN, HANDLING).unwrap());
    assert_eq!(plan["schema"], "ftx-sewing-plan/15");
    assert_eq!(program["plan"], plan);
    assert_eq!(program["stitches"].as_array().unwrap().len(), 240);
    let production = &plan["production"];
    assert_eq!(
        production["ordering"]["selected_order"],
        json!([
            "0/join",
            "0/reinforce",
            "1/join",
            "1/reinforce",
            "2/join",
            "2/reinforce"
        ])
    );
    assert_eq!(production["ordering"]["optimality_proven"], true);
    assert_eq!(
        production["ordering"]["timing"]["selected_setup_milliseconds"],
        6500
    );
    let timing = &production["ordering"]["handling_timing"];
    assert_eq!(timing["selected_handling_milliseconds"], 10600);
    assert!(
        (timing["selected_modeled_production_seconds"]
            .as_f64()
            .unwrap()
            - 41.1)
            .abs()
            < 1e-12
    );
    assert_eq!(
        production["handling"]["selected"]["steps"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(
        production["workholding"]["selected"]["peak_occupancy"]["holding-jig"],
        1
    );
    assert_eq!(plan["hardware_execution_authorized"], false);

    let pattern: Value = serde_json::from_str(PATTERN).unwrap();
    let mut request = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"retained","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern["fabric"]["material"]}}],
        "sewing":serde_json::from_str::<Value>(HANDLING).unwrap(),
        "comparison":{"basis":"nominal","required_consumers":["sewing"],
            "objectives":[{"metric":"sewing_modeled_production_seconds","direction":"minimize"}],
            "constraints":[{"metric":"sewing_handling_seconds","maximum":11.0}]}});
    let report = portable(ftx_wasm::materials::study(PATTERN, &request.to_string()).unwrap());
    let row = &report["alternatives"][0];
    let bound_pattern = row["binding"]["result"]["pattern_json"].as_str().unwrap();
    let resolved_options = row["sewing"]["result"]["options"].to_string();
    assert_eq!(
        row["sewing"]["result"],
        portable(ftx_wasm::manufacturing::sew_plan(bound_pattern, &resolved_options).unwrap())
    );
    assert_eq!(row["sewing"]["result"]["production"], *production);
    assert_eq!(row["metrics"]["sewing_handling_seconds"], 10.6);
    assert_eq!(
        row["metrics"]["sewing_modeled_production_seconds"],
        timing["selected_modeled_production_seconds"]
    );
    assert_eq!(report["comparison"]["frontier"], json!(["retained"]));
    fs::write(directory.join("study.json"), request.to_string()).unwrap();
    fs::write(directory.join("bound.json"), bound_pattern).unwrap();
    fs::write(directory.join("resolved.json"), &resolved_options).unwrap();
    for main in [false, true] {
        assert_eq!(
            run(
                main,
                &directory,
                "sew-plan",
                "pattern.json",
                "handling.json"
            ),
            plan
        );
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "handling.json"
            ),
            program
        );
        assert_eq!(
            run(main, &directory, "study", "pattern.json", "study.json"),
            report
        );
        assert_eq!(
            run(main, &directory, "sew-plan", "bound.json", "resolved.json"),
            row["sewing"]["result"]
        );
    }

    // Unsupported source qualification withdraws the time objective without erasing binding.
    request["sewing"]["production"]["handling"]["machine_sha256"] = json!("0".repeat(64));
    let refused = portable(ftx_wasm::materials::study(PATTERN, &request.to_string()).unwrap());
    assert_eq!(refused["alternatives"][0]["binding"]["status"], "available");
    assert_eq!(refused["alternatives"][0]["sewing"]["status"], "refused");
    assert!(refused["alternatives"][0]["metrics"]["sewing_modeled_production_seconds"].is_null());
    assert_eq!(refused["comparison"]["frontier"], json!([]));
    assert_eq!(
        fs::read_to_string(directory.join("pattern.json")).unwrap(),
        PATTERN
    );
    assert_eq!(
        fs::read_to_string(directory.join("handling.json")).unwrap(),
        HANDLING
    );
}

#[test]
fn cutting_quantity_reaches_batch_studies_and_exact_standalone_native_replay() {
    let directory = workspace();
    let mut pattern: Value = serde_json::from_str(PATTERN).unwrap();
    pattern["fabric"]["material"]["record"]["properties"]["width"] = json!({
        "value":0.2,"unit":"m","evidence":"estimate","source":"test",
        "method":"Synthetic stock width for complete production integration"
    });
    let pattern_text = pattern.to_string();
    fs::write(directory.join("pattern.json"), &pattern_text).unwrap();
    let mut options: Value = serde_json::from_str(OPTIONS).unwrap();
    options["production"]
        .as_object_mut()
        .unwrap()
        .remove("production_copies");
    let mut study = json!({"schema":"ftx-material-study/1",
        "variants":[{"key":"original","changes":{"schema":"ftx-material-bindings/1",
            "default":pattern["fabric"]["material"]}}],
        "cutting":{"copies":3,"gap_mm":2.0,"edge_margin_mm":0.0,"end_margin_mm":0.0},
        "sewing":options,
        "comparison":{"basis":"nominal","required_consumers":["cutting","sewing"],
            "objectives":[{"metric":"sewing_feed_and_setup_seconds","direction":"minimize"}]}});
    let expected = portable(ftx_wasm::materials::study(&pattern_text, &study.to_string()).unwrap());
    let row = &expected["alternatives"][0];
    assert_eq!(row["cutting"]["status"], "available");
    assert_eq!(row["sewing"]["status"], "available");
    let plan = &row["sewing"]["result"];
    assert_eq!(plan["options"]["production"]["production_copies"], 3);
    assert_eq!(row["metrics"]["stitch_intervals"], 240);
    assert_eq!(row["metrics"]["feed_seconds"], 24.0);
    assert_eq!(row["metrics"]["sewing_setup_seconds"], 1.5);
    assert_eq!(row["metrics"]["sewing_feed_and_setup_seconds"], 25.5);
    assert_eq!(expected["comparison"]["frontier"], json!(["original"]));
    fs::write(directory.join("study.json"), study.to_string()).unwrap();
    fs::write(
        directory.join("bound.json"),
        row["binding"]["result"]["pattern_json"].as_str().unwrap(),
    )
    .unwrap();
    fs::write(directory.join("resolved.json"), plan["options"].to_string()).unwrap();
    for main in [false, true] {
        assert_eq!(
            run(main, &directory, "study", "pattern.json", "study.json"),
            expected
        );
        assert_eq!(
            run(main, &directory, "sew-plan", "bound.json", "resolved.json"),
            *plan
        );
    }

    // A contradictory authored schedule withdraws sewing while preserving independent stock planning.
    study["sewing"]["production"]["production_copies"] = json!(2);
    fs::write(directory.join("mismatch.json"), study.to_string()).unwrap();
    let refused = portable(ftx_wasm::materials::study(&pattern_text, &study.to_string()).unwrap());
    assert_eq!(refused["alternatives"][0]["cutting"], row["cutting"]);
    assert_eq!(refused["alternatives"][0]["sewing"]["status"], "refused");
    assert!(refused["alternatives"][0]["metrics"]["feed_seconds"].is_null());
    assert_eq!(refused["comparison"]["frontier"], json!([]));
    for main in [false, true] {
        assert_eq!(
            run(main, &directory, "study", "pattern.json", "mismatch.json"),
            refused
        );
    }
    assert_eq!(
        fs::read_to_string(directory.join("pattern.json")).unwrap(),
        pattern_text
    );
}

#[test]
fn complete_batch_timing_changes_recipe_choice_and_both_commands_replay_the_selected_plan() {
    let directory = workspace();
    fs::write(directory.join("pattern.json"), PATTERN).unwrap();
    let single = portable(ftx_wasm::sew_select(PATTERN, TIMED_SELECTION).unwrap());
    assert_eq!(single["report"]["selected_objective_seconds"], 8.0);
    assert_eq!(
        single["report"]["selected_choices"],
        json!({"join":"template","finish":"template"})
    );
    let mut request: Value = serde_json::from_str(TIMED_SELECTION).unwrap();
    let ordering = request["options"]
        .as_object_mut()
        .unwrap()
        .remove("ordering")
        .unwrap();
    request["options"]["production"] = json!({"production_copies":3,"ordering":ordering});
    let text = request.to_string();
    fs::write(directory.join("selection.json"), &text).unwrap();
    let selected = portable(ftx_wasm::sew_select(PATTERN, &text).unwrap());
    assert_eq!(selected["report"]["selected_objective_seconds"], 22.0);
    assert_eq!(
        selected["report"]["selected_choices"],
        json!({"join":"fast","finish":"fast"})
    );
    assert_eq!(selected["report"]["optimality_proven"], true);
    assert_eq!(
        selected["report"]["trials"]
            .as_array()
            .unwrap()
            .iter()
            .map(|trial| trial["objective_seconds"].as_f64().unwrap())
            .collect::<Vec<_>>(),
        [24.0, 35.0, 28.0, 22.0]
    );
    fs::write(
        directory.join("selected.json"),
        selected["selected_options_json"].as_str().unwrap(),
    )
    .unwrap();
    let program = portable(
        ftx_wasm::manufacturing::sew_stitches(
            PATTERN,
            selected["selected_options_json"].as_str().unwrap(),
        )
        .unwrap(),
    );
    assert_eq!(program["plan"], selected["report"]["selected_plan"]);
    assert_eq!(program["plan"]["feed_seconds"], 4.0);
    assert_eq!(program["plan"]["production"]["feed_seconds"], 12.0);
    assert_eq!(program["stitches"].as_array().unwrap().len(), 150);
    for main in [false, true] {
        assert_eq!(
            run(
                main,
                &directory,
                "sew-select",
                "pattern.json",
                "selection.json"
            ),
            selected
        );
        assert_eq!(
            run(
                main,
                &directory,
                "sew-stitches",
                "pattern.json",
                "selected.json"
            ),
            program
        );
    }
    assert_eq!(
        fs::read_to_string(directory.join("selection.json")).unwrap(),
        text
    );
}
