//! Execute both native entry points against the canonical source-bearing catalog.
use std::path::Path;
use std::process::Command;

use ftx_pattern::{material_screen::Request, textile_import};
use serde_json::Value;

const FIXTURE: &str = include_str!("../../../tests/fixtures/materials/screen-insulation.json");

#[test]
fn both_binaries_emit_the_same_screening_as_the_library() {
    let request: Request = serde_json::from_str(FIXTURE).unwrap();
    let expected = serde_json::to_value(
        request
            .evaluate(&textile_import::catalog().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(!expected["frontier"].as_array().unwrap().is_empty());
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/materials/screen-insulation.json");
    let companion = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("screen")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        companion.status.success(),
        "{}",
        String::from_utf8_lossy(&companion.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&companion.stdout).unwrap(),
        expected
    );
    let main = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .args(["materials", "screen"])
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        main.status.success(),
        "{}",
        String::from_utf8_lossy(&main.stderr)
    );
    let result: Value = serde_json::from_slice(&main.stdout).unwrap();
    assert_eq!(result["result"], expected);
}

#[test]
fn native_screening_refuses_missing_inputs_and_unrecognized_options() {
    for args in [
        vec!["materials", "screen"],
        vec!["materials", "screen", "missing.json"],
        vec!["materials", "screen", "missing.json", "--allow-estimates"],
    ] {
        assert!(
            !Command::new(env!("CARGO_BIN_EXE_ftx"))
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}

#[test]
fn both_material_study_commands_reach_real_markers_and_explicit_missing_widths() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/materials");
    let pattern = root.join("synthetic-sewing-pattern.json");
    let request = root.join("study-insulation.json");
    let expected: ftx_export::material_study::Request =
        serde_json::from_slice(&std::fs::read(&request).unwrap()).unwrap();
    let source =
        ftx_pattern::Pattern::from_json(&std::fs::read_to_string(&pattern).unwrap()).unwrap();
    let expected = serde_json::to_value(expected.evaluate(&source).unwrap()).unwrap();
    assert!(
        expected["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["cutting"]["status"] == "available")
    );
    assert!(
        expected["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["cutting"]["status"] == "refused")
    );
    for (binary, prefix) in [
        (env!("CARGO_BIN_EXE_ftx-materials"), vec!["study"]),
        (
            env!("CARGO_BIN_EXE_ftx"),
            vec!["--json", "materials", "study"],
        ),
    ] {
        let out = Command::new(binary)
            .args(prefix)
            .arg(&pattern)
            .arg(&request)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let result: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            if result.get("result").is_some() {
                &result["result"]
            } else {
                &result
            },
            &expected
        );
    }
}

#[test]
fn both_material_study_commands_compute_declared_objectives_using_the_same_library() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/materials");
    let pattern = root.join("synthetic-sewing-pattern.json");
    let source =
        ftx_pattern::Pattern::from_json(&std::fs::read_to_string(&pattern).unwrap()).unwrap();
    let mut request: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/study-insulation.json"
    ))
    .unwrap();
    request["comparison"] = serde_json::json!({"basis":"nominal", "objectives":[
        {"metric":"stock_mass_kg","direction":"minimize"}], "allow_mixed_protocols":true});
    let typed: ftx_export::material_study::Request =
        serde_json::from_value(request.clone()).unwrap();
    let expected = serde_json::to_value(typed.evaluate(&source).unwrap()).unwrap();
    assert!(
        !expected["comparison"]["frontier"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        expected["comparison"]["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["eligible"] == false)
    );
    let path =
        std::env::temp_dir().join(format!("ftx-objective-study-{}.json", std::process::id()));
    std::fs::write(&path, request.to_string()).unwrap();
    for (binary, prefix) in [
        (env!("CARGO_BIN_EXE_ftx-materials"), vec!["study"]),
        (
            env!("CARGO_BIN_EXE_ftx"),
            vec!["--json", "materials", "study"],
        ),
    ] {
        let out = Command::new(binary)
            .args(prefix)
            .arg(&pattern)
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let result: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(result.get("result").unwrap_or(&result), &expected);
    }
}
