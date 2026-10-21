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
