//! Both real CLI entry points consume measured leather Ret under illustrative boundary inputs.
use ftx_pattern::{
    material_vapor::Request,
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

const INPUT: &str = include_str!("../../../tests/fixtures/materials/leather-vapor-stack.json");

fn invoke(main: bool, input: &str, extra: &[&str]) -> Output {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "ftx-vapor-{}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, input).unwrap();
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    if main {
        command.args(["materials", "--json"]);
    }
    let output = command
        .arg("thermal-vapor")
        .arg(&path)
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), input);
    output
}

fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    if value["schema"] == "ftx-cli/1" {
        assert_eq!(value["command"], "materials");
        assert_eq!(value["ok"], true);
        assert_eq!(value["exit"], 0);
        value["result"].clone()
    } else {
        value
    }
}

fn refusal(main: bool, output: &Output) {
    assert!(!output.status.success());
    let document = if main { &output.stdout } else { &output.stderr };
    let value: Value = serde_json::from_slice(document).unwrap();
    assert!(value["refusal"].is_object());
    assert!(value.get("result").is_none());
    if !main {
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn both_binaries_match_core_preserve_source_conditions_and_replay_offline() {
    let request: Request = serde_json::from_str(INPUT).unwrap();
    let expected = serde_json::to_value(
        request
            .evaluate(&ftx_pattern::textile_import::catalog().unwrap())
            .unwrap(),
    )
    .unwrap();
    let replay = expected["replay_request_json"].as_str().unwrap();
    let frozen: Request = serde_json::from_str(replay).unwrap();
    let empty = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    let replayed = serde_json::to_value(frozen.evaluate(&empty).unwrap()).unwrap();
    for main in [false, true] {
        assert_eq!(success(&invoke(main, INPUT, &[])), expected);
        assert_eq!(success(&invoke(main, replay, &[])), replayed);
    }
    // The Ret mean is transcribed independently from Kopitar2022 Table 5; boundary data are
    // illustrative inputs, not a measured wearer condition or a validated stack experiment.
    let expected_power = 0.5 * 2000.0 / (10.0 + 11.27 + 20.0);
    assert!((expected["potential_heat_flow_w"].as_f64().unwrap() - expected_power).abs() < 1e-12);
    assert_eq!(expected["layers"][0]["observation"]["evidence"], "measured");
    assert_eq!(
        expected["layers"][0]["observation"]["conditions"]["air_temperature_c"],
        "35"
    );
    assert_eq!(expected["physical_validation_claim"], false);
    assert_eq!(replayed["layers"], expected["layers"]);
    assert_eq!(
        replayed["potential_heat_flow_w"],
        expected["potential_heat_flow_w"]
    );
}

#[test]
fn changing_only_canonical_material_changes_both_entry_points() {
    let original = success(&invoke(true, INPUT, &[]));
    let mut changed: Value = serde_json::from_str(INPUT).unwrap();
    changed["layers"][0]["selection"]["id"] = json!("zagreb/leather-2022/syn-d-h-neck");
    let second = success(&invoke(true, &changed.to_string(), &[]));
    assert_eq!(second, success(&invoke(false, &changed.to_string(), &[])));
    assert_ne!(second["request_sha256"], original["request_sha256"]);
    assert_ne!(
        second["layers"][0]["material_sha256"],
        original["layers"][0]["material_sha256"]
    );
    assert!((second["potential_heat_flow_w"].as_f64().unwrap() - 1000.0 / 60.38).abs() < 1e-12);
    assert!(
        second["potential_heat_flow_w"].as_f64().unwrap()
            < original["potential_heat_flow_w"].as_f64().unwrap()
    );
}

#[test]
fn malformed_unqualified_and_changed_frozen_sources_refuse_without_partial_reports() {
    let report = success(&invoke(false, INPUT, &[]));
    let mut tampered = report["replay_request"].clone();
    tampered["layers"][0]["selection"]["record"]["properties"]["vapor_resistance"]["value"] =
        json!(1.0);
    let mut unsupported: Value = serde_json::from_str(INPUT).unwrap();
    unsupported["required_conditions"]["air_temperature_c"] = json!("20");
    let mut reverse: Value = serde_json::from_str(INPUT).unwrap();
    reverse["outside_vapor_pressure_pa"] = json!(4000.0);
    let mut unknown: Value = serde_json::from_str(INPUT).unwrap();
    unknown["infer_sweat_supply"] = json!(true);
    let mut missing: Value = serde_json::from_str(INPUT).unwrap();
    missing["layers"][0]["selection"]["id"] = json!("fsri/hfm/cotton-sheet/dry/220330-r1");
    for main in [false, true] {
        for input in [
            "{oops".into(),
            tampered.to_string(),
            unsupported.to_string(),
            reverse.to_string(),
            unknown.to_string(),
            missing.to_string(),
        ] {
            refusal(main, &invoke(main, &input, &[]));
        }
        for extra in [
            vec!["--allow-estimates"],
            vec!["extra.json"],
            vec!["--infer-sweat"],
        ] {
            refusal(main, &invoke(main, INPUT, &extra));
        }
    }
}
