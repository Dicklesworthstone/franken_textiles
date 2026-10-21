//! Real executable compression/heat history with source-locked downstream material reuse.
//! All acquisitions and boundary conditions are synthetic numerical verification inputs.
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

fn input() -> Value {
    let mut preparation: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    preparation["pressure_pa"] = json!(0.0);
    let hold = |gap| {
        json!({"seconds":10.0,"gap_m":gap,
        "interface_resistance_m2_k_w":[],"inside_c":40.0,"outside_c":0.0,
        "inside_resistance_m2_k_w":0.0,"outside_resistance_m2_k_w":0.0})
    };
    json!({"schema":"ftx-compressed-thermal-history/1", "setup":{
        "stack":{"schema":"ftx-compression-stack/1","gap_m":0.020,"reference_area_m2":0.25,
            "direction":"loading","allow_estimates":true,"layers":[{"id":"synthetic-layer","preparation":preparation}]},
        "cells_per_layer":1,"initial_temperature_c":[0.0],
        "thermal_evidence":"Synthetic constant storage, explicit interfaces/films and reservoirs; no mechanical heat conversion"},
        "holds":[hold(0.015),hold(0.010)]})
}

fn setup(input: &Value) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-compressed-heat-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(
        dir.join("request.json"),
        serde_json::to_vec_pretty(input).unwrap(),
    )
    .unwrap();
    dir
}

fn run(dir: &std::path::Path, unified: bool, args: &[&str], code: i32) -> std::process::Output {
    let mut cmd = Command::new(if unified {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    cmd.current_dir(dir);
    if unified {
        cmd.args(["--json", "materials"]);
    }
    let out = cmd.args(args).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

#[test]
fn both_commands_preserve_heat_history_and_replay_the_final_material_without_clobbering() {
    let original = input();
    let dir = setup(&original);
    let source = std::fs::read(dir.join("request.json")).unwrap();
    let out = run(&dir, false, &["thermal-compression", "request.json"], 0);
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    let same = run(&dir, true, &["thermal-compression", "request.json"], 0);
    let envelope: Value = serde_json::from_slice(&same.stdout).unwrap();
    assert_eq!(envelope["result"], report);
    assert_eq!(report["schema"], "ftx-compressed-thermal-history-report/1");
    let typed: ftx_drape::thermal::compression::Input = serde_json::from_value(original).unwrap();
    assert_eq!(report["input"], serde_json::to_value(typed).unwrap());
    let first = &report["steps"][0];
    let last = &report["steps"][1];
    assert!((last["pressure_pa"].as_f64().unwrap() - 500.0).abs() < 1e-9);
    assert!(
        last["temperature_c"][0].as_f64().unwrap() > first["temperature_c"][0].as_f64().unwrap()
    );
    assert!((last["cumulative_external_work_j"].as_f64().unwrap() - 0.625).abs() < 1e-10);
    assert!(
        last["cumulative_heat_balance_error_j"]
            .as_f64()
            .unwrap()
            .abs()
            < 1e-9
    );
    let selection = report["final_compression"]["layers"][0]["conditioned"]["selection_json"]
        .as_str()
        .unwrap();
    std::fs::write(dir.join("selection.json"), selection).unwrap();
    std::fs::write(
        dir.join("pattern.json"),
        include_bytes!("../../../tests/fixtures/materials/synthetic-stock-pattern.json"),
    )
    .unwrap();
    let pattern: Value =
        serde_json::from_slice(&std::fs::read(dir.join("pattern.json")).unwrap()).unwrap();
    run(
        &dir,
        false,
        &["bind", "pattern.json", "selection.json", "bound.json"],
        0,
    );
    let bound_bytes = std::fs::read(dir.join("bound.json")).unwrap();
    let bound: Value = serde_json::from_slice(&bound_bytes).unwrap();
    assert_eq!(pattern["pieces"], bound["pieces"]);
    let physical = run(&dir, false, &["resolve", "bound.json"], 0);
    let physical: Value = serde_json::from_slice(&physical.stdout).unwrap();
    assert_eq!(physical["body_offset_m"], json!(0.005));
    assert_eq!(physical["mechanics"]["areal_kg_m2"], json!(0.1));
    run(
        &dir,
        false,
        &["bind", "pattern.json", "selection.json", "bound.json"],
        2,
    );
    assert_eq!(std::fs::read(dir.join("bound.json")).unwrap(), bound_bytes);
    assert_eq!(std::fs::read(dir.join("request.json")).unwrap(), source);
    let again = run(&dir, false, &["thermal-compression", "request.json"], 0);
    assert_eq!(out.stdout, again.stdout);
}

#[test]
fn late_failure_or_unknown_options_cannot_publish_a_partial_success() {
    let mut value = input();
    value["holds"][1]["inside_c"] = json!(-300.0);
    let dir = setup(&value);
    let original = std::fs::read(dir.join("request.json")).unwrap();
    let bad = run(&dir, false, &["thermal-compression", "request.json"], 2);
    assert!(bad.stdout.is_empty());
    let refusal: Value = serde_json::from_slice(&bad.stderr).unwrap();
    assert!(refusal["refusal"].is_object());
    let bad = run(&dir, true, &["thermal-compression", "request.json"], 3);
    let response: Value = serde_json::from_slice(&bad.stdout).unwrap();
    assert!(response.get("result").is_none_or(Value::is_null));
    assert_eq!(std::fs::read(dir.join("request.json")).unwrap(), original);
    run(
        &dir,
        true,
        &["thermal-compression", "request.json", "--allow-estimates"],
        3,
    );
    run(
        &dir,
        false,
        &["thermal-compression", "request.json", "extra"],
        2,
    );
    for change in [
        |v: &mut Value| v["schema"] = json!("unknown"),
        |v: &mut Value| v["holds"][0]["duration"] = json!(1),
        |v: &mut Value| v["setup"]["stack"]["allow_estimates"] = json!(false),
        |v: &mut Value| v["setup"]["initial_temperature_c"] = json!([]),
    ] {
        let mut v = input();
        change(&mut v);
        let d = setup(&v);
        assert!(
            run(&d, false, &["thermal-compression", "request.json"], 2)
                .stdout
                .is_empty()
        );
        run(&d, true, &["thermal-compression", "request.json"], 3);
    }
}

fn cyclic_input() -> Value {
    let mut value = input();
    value["schema"] = json!("ftx-compressed-thermal-history/2");
    // Explicit synthetic, branch-independent reference observations. Never remove a real source's
    // branch restriction to obtain acceptance: the engine retains and checks all supplied conditions.
    let p = &mut value["setup"]["stack"]["layers"][0]["preparation"];
    for q in p["material"]["properties"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        q["conditions"]
            .as_object_mut()
            .unwrap()
            .remove("loading_branch");
    }
    let mut entry = value["setup"]["stack"].clone();
    entry["gap_m"] = json!(0.010);
    entry["direction"] = json!("unloading");
    let p = &mut entry["layers"][0]["preparation"];
    p["pressure_pa"] = json!(500.0);
    p["loading_branch"] = json!("unloading-after-500");
    let mut mid = p["samples"][0].clone();
    mid["pressure_pa"] = json!(250.0);
    mid["properties"]["thickness"]["value"] = json!(12.0);
    mid["properties"]["thickness"]
        .as_object_mut()
        .unwrap()
        .remove("bounds");
    mid["properties"]["thermal_resistance"]["value"] = json!(0.3);
    for q in mid["properties"].as_object_mut().unwrap().values_mut() {
        q["conditions"]["pressure_pa"] = json!("250");
    }
    p["samples"].as_array_mut().unwrap().insert(1, mid);
    for row in p["samples"].as_array_mut().unwrap() {
        for q in row["properties"].as_object_mut().unwrap().values_mut() {
            q["conditions"]["loading_branch"] = json!("unloading-after-500");
        }
    }
    for q in p["invariants"].as_object_mut().unwrap().values_mut() {
        q["conditions"]["loading_branch"] = json!("unloading-after-500");
    }
    let link = json!({"source":"synthetic","evidence":"estimate","method":"Invented full cycle after a 500 Pa peak, for software verification only",
        "conditions":{"specimen":p["specimen"],"conditioning":p["conditioning"],
            "from_loading_branch":"loading","to_loading_branch":"unloading-after-500","pressure_pa":"500"}});
    let mut first = value["holds"][0].clone();
    first["gap_m"] = json!(0.010);
    let mut middle = first.clone();
    middle["gap_m"] = json!(0.012);
    let mut last = first.clone();
    last["gap_m"] = json!(0.020);
    value.as_object_mut().unwrap().remove("holds");
    value["legs"] = json!([
        {"holds":[first]},
        {"branch_change":{"entry":entry,"transitions":{"synthetic-layer":link}},"holds":[middle,last]}
    ]);
    value
}

#[test]
fn both_native_commands_run_a_cyclic_path_and_reuse_its_final_selection() {
    let original = cyclic_input();
    let dir = setup(&original);
    let source = std::fs::read(dir.join("request.json")).unwrap();
    let output = run(&dir, false, &["thermal-compression", "request.json"], 0);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let output = run(&dir, true, &["thermal-compression", "request.json"], 0);
    let wrapped: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(wrapped["result"], report);
    assert_eq!(report["schema"], "ftx-compressed-thermal-history-report/2");
    let typed: ftx_drape::thermal::compression::path::Input =
        serde_json::from_value(original).unwrap();
    assert_eq!(
        report,
        serde_json::to_value(ftx_drape::thermal::compression::path::evaluate(&typed).unwrap())
            .unwrap()
    );
    let last = &report["steps"][2];
    assert!((last["cumulative_external_work_j"].as_f64().unwrap() - 0.1875).abs() < 1e-12);
    assert_eq!(last["elapsed_s"], 30.0);
    assert!(
        last["temperature_c"][0].as_f64().unwrap()
            > report["steps"][1]["temperature_c"][0].as_f64().unwrap()
    );
    assert_eq!(report["transitions"][0]["before_hold"], 1);
    assert_eq!(
        report["transitions"][0]["result"]["entry_compression"]["external_work_j"],
        0.0
    );
    let selection = report["final_compression"]["layers"][0]["conditioned"]["selection_json"]
        .as_str()
        .unwrap();
    std::fs::write(dir.join("cyclic-selection.json"), selection).unwrap();
    std::fs::write(
        dir.join("pattern.json"),
        include_bytes!("../../../tests/fixtures/materials/synthetic-stock-pattern.json"),
    )
    .unwrap();
    run(
        &dir,
        false,
        &[
            "bind",
            "pattern.json",
            "cyclic-selection.json",
            "bound.json",
        ],
        0,
    );
    let resolved = run(&dir, false, &["resolve", "bound.json"], 0);
    let resolved: Value = serde_json::from_slice(&resolved.stdout).unwrap();
    assert!(resolved.to_string().contains("synthetic/compression-sheet"));
    run(
        &dir,
        false,
        &[
            "bind",
            "pattern.json",
            "cyclic-selection.json",
            "bound.json",
        ],
        2,
    );
    assert_eq!(source, std::fs::read(dir.join("request.json")).unwrap());
    assert_eq!(
        selection,
        std::fs::read_to_string(dir.join("cyclic-selection.json")).unwrap()
    );
}

#[test]
fn missing_reversal_evidence_or_late_heat_failure_never_emits_a_partial_native_path() {
    for mutate in [
        |v: &mut Value| v["legs"][1]["branch_change"]["transitions"] = json!({}),
        |v: &mut Value| {
            v["legs"][1]["branch_change"]["entry"]["layers"][0]["preparation"]["material"]["id"] =
                json!("substituted")
        },
        |v: &mut Value| v["legs"][1]["holds"][1]["outside_c"] = json!(-300.0),
        |v: &mut Value| v["holds"] = json!([]),
    ] {
        let mut value = cyclic_input();
        mutate(&mut value);
        let dir = setup(&value);
        let before = std::fs::read(dir.join("request.json")).unwrap();
        assert!(
            run(&dir, false, &["thermal-compression", "request.json"], 2)
                .stdout
                .is_empty()
        );
        let refused = run(&dir, true, &["thermal-compression", "request.json"], 3);
        let envelope: Value = serde_json::from_slice(&refused.stdout).unwrap();
        assert!(envelope["result"].is_null());
        assert_eq!(before, std::fs::read(dir.join("request.json")).unwrap());
    }
}

#[test]
fn both_commands_run_adaptive_monotone_and_cyclic_histories_and_keep_one_work_budget() {
    for (mut value, resistances) in [
        (input(), [0.375_f64, 0.25]),
        (cyclic_input(), [0.25_f64, 0.3]),
    ] {
        // Use two holds in the cyclic case, preserving its source-qualified reversal.
        if value["schema"] == "ftx-compressed-thermal-history/2" {
            value["legs"][1]["holds"]
                .as_array_mut()
                .unwrap()
                .truncate(1);
        }
        value["adaptive"] = json!({"absolute_temperature_tolerance_k":1e-6,
            "absolute_heat_tolerance_j_m2":1e-4,"relative_tolerance":0.0,
            "min_step_s":0.00001,"max_step_s":10.0,"max_trials":10000,"max_cell_solves":262144});
        let dir = setup(&value);
        let original = std::fs::read(dir.join("request.json")).unwrap();
        let standalone = run(&dir, false, &["thermal-compression", "request.json"], 0);
        let wrapped = run(&dir, true, &["thermal-compression", "request.json"], 0);
        let report: Value = serde_json::from_slice(&standalone.stdout).unwrap();
        let envelope: Value = serde_json::from_slice(&wrapped.stdout).unwrap();
        assert_eq!(report, envelope["result"]);
        let typed = ftx_drape::thermal::compression::evaluate_json(&value.to_string()).unwrap();
        assert_eq!(report, serde_json::from_str::<Value>(&typed).unwrap());
        let mut temperature = 0.0;
        for (step, resistance) in report["steps"].as_array().unwrap().iter().zip(resistances) {
            temperature = 20.0 + (temperature - 20.0) * (-40.0 / (100.0 * resistance)).exp();
            assert!((step["temperature_c"][0].as_f64().unwrap() - temperature).abs() < 0.01);
            assert!(
                step["integration"]["max_accepted_error_ratio"]
                    .as_f64()
                    .unwrap()
                    <= 1.0
            );
            assert!(
                step["cumulative_heat_balance_error_j"]
                    .as_f64()
                    .unwrap()
                    .abs()
                    < 1e-8
            );
        }
        assert_eq!(std::fs::read(dir.join("request.json")).unwrap(), original);
        value["adaptive"]["max_cell_solves"] =
            report["steps"][0]["integration"]["cell_solves"].clone();
        let rejected = setup(&value);
        assert!(
            run(
                &rejected,
                false,
                &["thermal-compression", "request.json"],
                2
            )
            .stdout
            .is_empty()
        );
        let out = run(&rejected, true, &["thermal-compression", "request.json"], 4);
        let failure: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(failure["result"].is_null());
        assert_eq!(failure["refusal"]["code"], "budget_exceeded");
    }
}
