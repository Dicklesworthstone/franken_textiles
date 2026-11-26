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
    let operation = match input["schema"].as_str() {
        Some("ftx-strip-history/1") => "strip-history",
        Some("ftx-cylinder-experiment/1") => "cylinder-experiment",
        _ => "strip-experiment",
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
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ftx-strip-consumers-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    dir
}

#[test]
fn history_cylinder_calibration_recovers_relaxation_and_replays_through_both_commands() {
    let source = include_str!("../../../examples/cylinder-maxwell-calibration.json");
    let original: Value = serde_json::from_str(source).unwrap();
    let input: ftx_drape::cylinder::fit::Input = serde_json::from_value(original.clone()).unwrap();
    let portable: Value =
        serde_json::from_str(&ftx_wasm::materials::calibrate_cylinder(source, true).unwrap())
            .unwrap();
    let report = &portable["calibration"];
    assert_eq!(report["schema"], "ftx-cylinder-calibration-report/2");
    assert_eq!(report["accepted"], true, "{}", report["reasons"]);
    assert_eq!(report["domain_replay_verified"], true);
    assert_eq!(report["temporal_refinement"]["subdivision_factor"], 2);
    assert_eq!(report["temporal_refinement"]["accepted"], true);
    assert_eq!(portable["simulation_ready"], false);
    assert_eq!(portable["fit_claim"], false);
    assert_eq!(portable["empirical_validation_claim"], false);

    let frozen = portable["selection_json"].as_str().unwrap();
    let selection: ftx_pattern::material_catalog::Selection = serde_json::from_str(frozen).unwrap();
    let material = selection.record.as_ref().unwrap();
    let branch = &material.membrane_relaxation.as_ref().unwrap().branches[0];
    // The literal acquisition forces come from an independent continuum Maxwell ramp/hold
    // calculation. Allow only the declared discretization/inertia difference in this 3D solve.
    assert!(
        (branch.stiffness_ratio.value - 0.6).abs() < 0.006,
        "{}",
        report["parameters"]
    );
    assert!(
        (branch.relaxation_time.value - 0.15).abs() < 0.0015,
        "{}",
        report["parameters"]
    );
    assert_eq!(
        serde_json::to_value(&material.properties).unwrap(),
        serde_json::to_value(&input.material.properties).unwrap()
    );
    assert_eq!(
        branch.stiffness_ratio.evidence,
        ftx_pattern::materials::Evidence::Estimate
    );
    assert_eq!(
        branch.relaxation_time.evidence,
        ftx_pattern::materials::Evidence::Estimate
    );
    assert!(branch.stiffness_ratio.bounds.is_none());
    assert!(branch.relaxation_time.bounds.is_none());
    assert_eq!(branch.id, "recovery");

    let dir = directory();
    let path = dir.join("history-calibration.json");
    fs::write(&path, source).unwrap();
    for main in [false, true] {
        let mut command = Command::new(if main {
            env!("CARGO_BIN_EXE_ftx")
        } else {
            env!("CARGO_BIN_EXE_ftx-materials")
        });
        if main {
            command.args(["--json", "materials"]);
        }
        let output = command
            .arg("calibrate-cylinder")
            .arg(&path)
            .arg("--allow-estimates")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let native: Value = serde_json::from_slice(&output.stdout).unwrap();
        let native = if main { &native["result"] } else { &native };
        assert_eq!(&native["calibration"], report);
        assert_eq!(native["selection_json"], frozen);
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), source);

    // Frozen predictions replay on the reported doubled mesh with the entire timed history.
    let check = input
        .experiments
        .iter()
        .find(|e| e.partition == ftx_pattern::material_calibration::Partition::Check)
        .unwrap();
    let mut fixture = check.fixture.clone();
    fixture.cells = fixture.cells.map(|n| n * 2);
    let frozen_law = ftx_pattern::textile_import::catalog()
        .unwrap()
        .resolve(&selection)
        .unwrap()
        .membrane_relaxation()
        .unwrap()
        .unwrap();
    let replay_request = ftx_drape::cylinder::Request {
        schema: ftx_drape::cylinder::REQUEST_SCHEMA.into(),
        material: selection,
        fixture,
        loading: check.loading.clone(),
        geometry_output: ftx_drape::cylinder::GeometryOutput::EveryStep,
        nominal_protocol: None,
        controls: input.controls,
    };
    let replay = ftx_drape::cylinder::evaluate(&replay_request).unwrap();
    for prediction in report["predictions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["experiment"] == check.id)
    {
        let row = check
            .observations
            .iter()
            .find(|row| prediction["observation"] == row.id)
            .unwrap();
        let state = replay
            .states
            .iter()
            .find(|state| state.time_s == check.loading[row.waypoint_index].time_s)
            .unwrap();
        let mut predicted = prediction["state"].clone();
        let mut replayed = serde_json::to_value(state).unwrap();
        let predicted_elements = predicted["relaxation"]["elements"].as_array_mut().unwrap();
        let replayed_elements = replayed["relaxation"]["elements"].as_array_mut().unwrap();
        assert_eq!(predicted_elements.len(), replayed_elements.len());
        for (candidate, frozen) in predicted_elements.iter_mut().zip(replayed_elements) {
            // Publication changes quantity provenance, hence the law identity, while preserving
            // all physical coefficients and accepted memory. Verify the new identity separately.
            assert_ne!(candidate["law_sha256"], frozen["law_sha256"]);
            assert_eq!(frozen["law_sha256"], frozen_law.fingerprint());
            candidate.as_object_mut().unwrap().remove("law_sha256");
            frozen.as_object_mut().unwrap().remove("law_sha256");
        }
        assert_eq!(
            sha256_hex(&serde_json::to_vec(&predicted).unwrap()),
            sha256_hex(&serde_json::to_vec(&replayed).unwrap()),
            "physical prediction and full history replay differ at {}",
            state.time_s
        );
    }

    // CHECK can reject the already selected coefficients, never improve or replace them.
    let mut corrupted = original;
    let held_out = corrupted["experiments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["partition"] == "check")
        .unwrap();
    let value = held_out["observations"][1]["force_n"]["value"]
        .as_f64()
        .unwrap();
    held_out["observations"][1]["force_n"]["value"] = json!(value + 0.5);
    let rejected: Value = serde_json::from_str(
        &ftx_wasm::materials::calibrate_cylinder(&corrupted.to_string(), true).unwrap(),
    )
    .unwrap();
    assert_eq!(rejected["calibration"]["accepted"], false);
    assert_eq!(rejected["calibration"]["parameters"], report["parameters"]);
    assert!(rejected["selection"].is_null());
    assert!(rejected["selection_json"].is_null());
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

#[test]
fn cylinder_experiment_replays_through_both_native_commands_and_wasm_and_refuses_uncalibrated_fabric101()
 {
    let text = include_str!("../../../tests/fixtures/materials/synthetic-cylinder-experiment.json");
    let input: Value = serde_json::from_str(text).unwrap();
    let request: ftx_drape::cylinder::Request = serde_json::from_str(text).unwrap();
    let direct = ftx_drape::cylinder::evaluate(&request).unwrap();
    assert_eq!(
        direct.input_sha256,
        sha256_hex(direct.replay_request_json.as_bytes())
    );
    assert!(!direct.mechanics_readiness_promoted);
    assert!(!direct.fit_claim);
    assert!(!direct.empirical_validation_claim);
    assert_eq!(direct.protocol_audit.discrepancies.len(), 4);
    let last = direct.states.last().unwrap();
    assert!(last.maxwell_dissipation_j > 0.0);
    assert!(direct.first_slip_time_s.is_some());

    let replay: Value = serde_json::from_str(&direct.replay_request_json).unwrap();
    let expected = serde_json::to_value(&direct).unwrap();
    for source in [&input, &replay] {
        let portable: Value = serde_json::from_str(
            &ftx_wasm::materials::cylinder_experiment(&source.to_string()).unwrap(),
        )
        .unwrap();
        assert_eq!(portable, expected);
    }

    let missing_pretension = json!({
        "schema": "ftx-cylinder-experiment/1",
        "material": {"id": "fabric101-2026/l01"},
        "fixture": {
            "holder_radius_m": 0.04,
            "specimen_width_m": 0.26,
            "specimen_height_m": 0.11,
            "nominal_free_height_m": 0.05,
            "warp_axis": [0.0, 1.0],
            "cells": [12, 4]
        },
        "loading": [{"time_s": 0.0, "axial_displacement_m": 0.0, "rotation_deg": 0.0}]
    });
    assert!(
        ftx_wasm::materials::cylinder_experiment(&missing_pretension.to_string())
            .unwrap_err()
            .message
            .contains("pretension_displacement_m")
    );

    let uncalibrated = json!({
        "schema": "ftx-cylinder-experiment/1",
        "material": {"id": "fabric101-2026/l01"},
        "fixture": {
            "holder_radius_m": 0.04,
            "specimen_width_m": 0.26,
            "specimen_height_m": 0.11,
            "nominal_free_height_m": 0.05,
            "pretension_displacement_m": 0.00114125,
            "warp_axis": [0.0, 1.0],
            "cells": [12, 4]
        },
        "loading": [{"time_s": 0.0, "axial_displacement_m": 0.0, "rotation_deg": 0.0}]
    });
    let l01_req: ftx_drape::cylinder::Request =
        serde_json::from_value(uncalibrated.clone()).unwrap();
    let refusal = ftx_drape::cylinder::evaluate(&l01_req).unwrap_err();
    assert!(refusal.message.contains("warp_stiffness"));
    assert_eq!(
        serde_json::to_value(
            ftx_wasm::materials::cylinder_experiment(&uncalibrated.to_string()).unwrap_err()
        )
        .unwrap(),
        serde_json::to_value(&refusal).unwrap()
    );

    let dir = directory();
    for main in [false, true] {
        assert_eq!(call(&dir, &input, main).unwrap(), expected);
        assert_eq!(call(&dir, &replay, main).unwrap(), expected);
        assert_eq!(
            call(&dir, &uncalibrated, main).unwrap_err(),
            serde_json::to_value(&refusal).unwrap()
        );
    }
}
