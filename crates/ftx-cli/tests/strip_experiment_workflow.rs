//! Both native binaries execute the actual source-qualified finite strip, without garment data.
//! Coefficients and conditions are synthetic software inputs, not textile measurements.
use ftx_core::receipt::sha256_hex;
use ftx_drape::strip::{Request, evaluate};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn request() -> Value {
    let mut properties = serde_json::Map::new();
    for (key, value, unit) in [
        ("warp_stiffness", 0.8, "kN/m"),
        ("weft_stiffness", 0.4, "kN/m"),
        ("poisson_warp_weft", 0.0, "1"),
        ("shear_stiffness", 0.12, "kN/m"),
    ] {
        properties.insert(
            key.into(),
            json!({"value":value,"unit":unit,"source":"test",
            "evidence":"estimate","method":"Synthetic independent force check",
            "conditions":{"conditioning":"synthetic dry reference"}}),
        );
    }
    json!({"schema":"ftx-strip-experiment/1",
        "material":{"id":"synthetic-native-strip","allow_estimates":true,"record":{
            "id":"synthetic-native-strip","name":"Synthetic membrane only","form":"knit",
            "properties":properties,"sources":{"test":{"reference":"test:strip-consumers",
                "accessed_on":"2026-10-21","note":"Synthetic numerical fixture, not measured fabric"}}}},
        "fixture":{"width_m":0.06,"gauge_length_m":0.1,"warp_axis":[0.0,1.0],"cells":[2,4]},
        "extensions":[0.0,0.1],"controls":{"tolerance_m":1e-10,"force_tolerance_n":1e-7,"max_newton":100}})
}

fn call(dir: &Path, input: &Value, main: bool) -> Result<Value, Value> {
    let path = dir.join("input.json");
    let original = input.to_string();
    fs::write(&path, &original).unwrap();
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    if main {
        command.args(["--json", "materials"]);
    }
    let operation = if input["schema"] == "ftx-strip-history/1" {
        "strip-history"
    } else {
        "strip-experiment"
    };
    let output = command.arg(operation).arg(&path).output().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), original);
    if output.status.success() {
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        if main {
            assert_eq!(report["ok"], true);
            Ok(report["result"].clone())
        } else {
            Ok(report)
        }
    } else {
        assert_eq!(output.status.code(), Some(if main { 3 } else { 2 }));
        let report: Value =
            serde_json::from_slice(if main { &output.stdout } else { &output.stderr })
                .unwrap_or_else(|error| {
                    panic!(
                        "{error}: stdout {:?}; stderr {:?}",
                        output.stdout, output.stderr
                    )
                });
        assert!(report.get("result").is_none());
        Err(report["refusal"].clone())
    }
}

fn directory() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ftx-strip-consumers-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn both_native_entry_points_preserve_source_conditions_and_exact_frozen_forward_replay() {
    let input = request();
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let direct = evaluate(&typed).unwrap();
    let expected = serde_json::to_value(&direct).unwrap();
    assert_eq!(
        direct.input_sha256,
        sha256_hex(&serde_json::to_vec(&direct.input).unwrap())
    );
    assert!(direct.input.material.expected_sha256.is_some());
    // For nu=0, the affine solution is exact: F = width * Y * lambda * (lambda²-1)/2.
    assert!((direct.states[1].force_n - 0.06 * 800.0 * 1.1 * 0.105).abs() < 1e-10);
    assert_eq!(
        expected["input"]["material"]["record"],
        serde_json::to_value(&typed.material.record).unwrap()
    );
    assert_eq!(
        direct
            .input
            .material
            .record
            .as_ref()
            .unwrap()
            .properties
            .len(),
        4
    );
    let dir = directory();
    for main in [false, true] {
        assert_eq!(call(&dir, &input, main).unwrap(), expected);
        assert_eq!(call(&dir, &expected["input"], main).unwrap(), expected);
    }
}

#[test]
fn native_consumers_preserve_real_source_domain_and_numerical_work_refusals() {
    let original = request();
    let mut denied = original.clone();
    denied["material"]["allow_estimates"] = json!(false);
    let mut outside = original.clone();
    outside["material"]["record"]["sources"]["test"]["reference"] =
        json!("local:ftx-strip-calibration/synthetic-consumer-domain");
    for quantity in outside["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        quantity["conditions"]["fitted_green_strain_range"] =
            json!("[[-0.01,0.03],[-0.01,0.03],[-0.01,0.01]]");
    }
    let mut exhausted = original.clone();
    exhausted["material"]["record"]["properties"]["poisson_warp_weft"]["value"] = json!(0.25);
    exhausted["controls"]["max_newton"] = json!(1);
    let mut excessive = original;
    excessive["extensions"] = json!(vec![0.0; 65]);
    let dir = directory();
    for input in [denied, outside, exhausted, excessive] {
        let typed: Request = serde_json::from_value(input.clone()).unwrap();
        let expected = serde_json::to_value(evaluate(&typed).unwrap_err()).unwrap();
        for main in [false, true] {
            assert_eq!(call(&dir, &input, main).unwrap_err(), expected);
        }
    }
}

#[test]
fn both_native_history_entry_points_replay_the_complete_source_locked_loading_program() {
    let text = include_str!("../../../tests/fixtures/materials/synthetic-strip-history.json");
    let input: Value = serde_json::from_str(text).unwrap();
    let request = serde_json::from_str(text).unwrap();
    let direct = ftx_drape::strip::history::evaluate(&request).unwrap();
    assert_eq!(
        direct.input_sha256,
        sha256_hex(direct.replay_request_json.as_bytes())
    );
    let replay: Value = serde_json::from_str(&direct.replay_request_json).unwrap();
    let expected = serde_json::to_value(direct).unwrap();
    let dir = directory();
    for main in [false, true] {
        assert_eq!(call(&dir, &input, main).unwrap(), expected);
        assert_eq!(call(&dir, &replay, main).unwrap(), expected);
        let mut denied = input.clone();
        denied["material"]["allow_estimates"] = json!(false);
        assert!(call(&dir, &denied, main).is_err());
    }
}

#[test]
fn joint_memory_replays_through_both_native_commands_and_the_portable_boundary() {
    let material: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-parallel-membrane.json"
    ))
    .unwrap();
    let input = json!({
        "schema": "ftx-strip-history/1",
        "material": material,
        "fixture": {
            "gauge_length_m": 0.1, "width_m": 0.06,
            "warp_axis": [0.0, 1.0], "cells": [2, 2]
        },
        "loading": [
            {"time_s": 0.0, "extension_fraction": 0.0},
            {"time_s": 0.05, "extension_fraction": 0.08},
            {"time_s": 0.15, "extension_fraction": 0.08},
            {"time_s": 0.2, "extension_fraction": 0.0},
            {"time_s": 0.25, "extension_fraction": 0.0}
        ],
        "controls": {
            "max_step_s": 0.01, "max_steps": 32, "max_newton": 100,
            "velocity_tolerance_m_s": 1e-9, "force_tolerance_n": 1e-6
        }
    });
    let request = serde_json::from_value(input.clone()).unwrap();
    let direct = ftx_drape::strip::history::evaluate(&request).unwrap();
    assert!(direct.model_scope.contains("parallel"));
    let last = direct.states.last().unwrap();
    assert!(last.maxwell_dissipation_j > 0.0);
    assert!(last.plastic_dissipation_j > 0.0);
    assert!(last.relaxation.is_some() && last.plasticity.is_some());
    assert!(
        direct
            .states
            .iter()
            .any(|state| { state.solve.is_some_and(|solve| solve.roundoff_steps > 0) })
    );
    for state in &direct.states[1..] {
        let solve = state.solve.unwrap();
        assert!(solve.converged);
        assert!(solve.last_step / state.time_step_s < 1e-9);
        assert!(state.free_dynamic_residual_n <= 1e-6);
    }
    let replay: Value = serde_json::from_str(&direct.replay_request_json).unwrap();
    let expected = serde_json::to_value(&direct).unwrap();
    for source in [&input, &replay] {
        let portable: Value =
            serde_json::from_str(&ftx_wasm::materials::strip_history(&source.to_string()).unwrap())
                .unwrap();
        assert_eq!(portable, expected);
    }
    let mut unqualified = input.clone();
    unqualified["material"]["record"]["membrane_relaxation"]
        .as_object_mut()
        .unwrap()
        .remove("parallel_plasticity");
    let denied = serde_json::from_value(unqualified.clone()).unwrap();
    let refusal = ftx_drape::strip::history::evaluate(&denied).unwrap_err();
    assert_eq!(
        serde_json::to_value(
            ftx_wasm::materials::strip_history(&unqualified.to_string()).unwrap_err()
        )
        .unwrap(),
        serde_json::to_value(&refusal).unwrap()
    );
    let dir = directory();
    for main in [false, true] {
        assert_eq!(call(&dir, &input, main).unwrap(), expected);
        assert_eq!(call(&dir, &replay, main).unwrap(), expected);
        assert_eq!(
            call(&dir, &unqualified, main).unwrap_err(),
            serde_json::to_value(&refusal).unwrap()
        );
    }
}
