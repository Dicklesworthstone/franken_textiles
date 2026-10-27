//! Real production CLI over published material fits; environment/assembly inputs are illustrative.
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

fn command(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .args(args)
        .output()
        .unwrap()
}
fn success(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn refused(out: &Output) {
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert!(error["refusal"].is_object());
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-7 * (1.0 + b.abs()), "{a} != {b}");
}
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/nist-7282-transient.json"
    ))
    .unwrap()
}
fn transient(input: &Value) -> (Output, PathBuf) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "ftx-nist-{}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let bytes = serde_json::to_vec(input).unwrap();
    fs::write(&path, &bytes).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-transient")
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    (out, path)
}

#[test]
fn actual_catalog_and_point_evaluation_expose_complete_source_locked_estimates() {
    let db = success(&command(&["catalog", "nist/7282/"]));
    assert_eq!(db["materials"].as_array().unwrap().len(), 11);
    let args = ["thermal-at", "nist/7282/aralite", "50", "--allow-estimates"];
    let first = command(&args);
    let point = success(&first);
    assert_eq!(first.stdout, command(&args).stdout);
    assert_eq!(point["temperature_domain_c"], json!([20.0, 70.0]));
    near(
        point["resistance_m2_k_w"].as_f64().unwrap(),
        0.00359 / 0.0454,
    );
    near(
        point["areal_heat_capacity_j_m2_k"].as_f64().unwrap(),
        0.266378 * 1693.0,
    );
    assert_eq!(
        point["material"]["authored"]["properties"]["areal_density"]["evidence"],
        "estimate"
    );
    assert_eq!(
        point["selection"]["expected_sha256"],
        point["material"]["sha256"]
    );
    assert_eq!(point["uncertainty_propagated"], false);
    assert_eq!(point["empirical_validation_claim"], false);
}

#[test]
fn source_fits_drive_layer_transients_and_exact_reference_mass_enthalpy() {
    let input = fixture();
    let (out, _) = transient(&input);
    let report = success(&out);
    assert_eq!(report["model"]["schema"], "ftx-dry-thermal-model/2");
    let trace = report["trace"].as_array().unwrap();
    assert_eq!(trace.len(), 5);
    let final_t = trace.last().unwrap()["temperature_c"].as_array().unwrap();
    assert_eq!(final_t.len(), 8);
    // Independent quartic primitives of the source Table 4 SI cubics, not the candidate integrator.
    let h = |t: f64, layer: usize| {
        let (a, b) = if layer == 0 {
            (1798.0, -22.3)
        } else {
            (1832.2, -24.5)
        };
        a * t + b * t * t + 0.4 * t * t * t - 0.00175 * t * t * t * t
    };
    let expected: f64 = final_t
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mass = if i < 4 { 0.266378 } else { 0.259858 };
            mass / 4.0 * (h(t.as_f64().unwrap(), i / 4) - h(23.0, i / 4))
        })
        .sum();
    let stored = report["stored_energy_change_j"].as_f64().unwrap();
    near(stored, expected);
    near(
        stored,
        report["incoming_energy_j"].as_f64().unwrap()
            - report["outgoing_energy_j"].as_f64().unwrap(),
    );
    assert!(
        report["largest_step_energy_balance_error_j"]
            .as_f64()
            .unwrap()
            < 1e-7
    );
    assert_eq!(report["comfort_claim"], false);
    let (again, _) = transient(&input);
    assert_eq!(again.stdout, out.stdout);
}

#[test]
fn changing_only_the_builtin_material_changes_heat_flow_and_frozen_replay_is_locked() {
    let mut input = fixture();
    let original = success(&transient(&input).0);
    input["layers"][1]["id"] = json!("nist/7282/neo-guard");
    let changed = success(&transient(&input).0);
    assert!(
        changed["steady_heat_w"].as_f64().unwrap() > original["steady_heat_w"].as_f64().unwrap()
    );
    assert_ne!(changed["trace"], original["trace"]);
    assert_ne!(
        changed["model"]["layers"][1]["sha256"],
        original["model"]["layers"][1]["sha256"]
    );
    let point = success(&command(&[
        "thermal-at",
        "nist/7282/aralite",
        "50",
        "--allow-estimates",
    ]));
    input["layers"][0] = point["selection"].clone();
    let replay = success(&transient(&input).0);
    assert_eq!(replay, changed);
    input["layers"][0]["record"]["thermal_response"]["specific_heat"]["polynomial"]["coefficients"]
        [0] = json!(1799.0);
    refused(&transient(&input).0);
}

#[test]
fn published_cotton_linear_fit_drives_transient_enthalpy_and_portable_source_replay() {
    let mut input = fixture();
    let original = success(&transient(&input).0);
    input["layers"][0]["id"] = json!("nist/7282/cotton-duck-linear-cp");
    let (out, _) = transient(&input);
    let report = success(&out);
    assert_ne!(report["trace"], original["trace"]);
    assert!(
        report["steady_heat_w"].as_f64().unwrap() > original["steady_heat_w"].as_f64().unwrap()
    );
    let final_t = report["trace"].as_array().unwrap().last().unwrap()["temperature_c"]
        .as_array()
        .unwrap();
    assert_eq!(final_t.len(), 8);
    // Independently integrate Appendix A's linear Cp and Table 4's unchanged Nomex cubic.
    // Geometry/mass are the source-group effective values; the assembly is illustrative.
    let primitive = |t: f64, cotton: bool| {
        if cotton {
            1422.9 * t + 1.95 * t * t
        } else {
            1832.2 * t - 24.5 * t * t + 0.4 * t * t * t - 0.00175 * t * t * t * t
        }
    };
    let expected: f64 = final_t
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let cotton = i < 4;
            let mass = if cotton { 0.684948 } else { 0.259858 };
            mass / 4.0 * (primitive(t.as_f64().unwrap(), cotton) - primitive(23.0, cotton))
        })
        .sum();
    near(report["stored_energy_change_j"].as_f64().unwrap(), expected);
    near(
        expected,
        report["incoming_energy_j"].as_f64().unwrap()
            - report["outgoing_energy_j"].as_f64().unwrap(),
    );
    assert!(
        report["largest_step_energy_balance_error_j"]
            .as_f64()
            .unwrap()
            < 1e-7
    );
    assert_eq!(report["comfort_claim"], false);
    let point = success(&command(&[
        "thermal-at",
        "nist/7282/cotton-duck-linear-cp",
        "50",
        "--allow-estimates",
    ]));
    assert_eq!(point["temperature_domain_c"], json!([20.0, 70.0]));
    near(
        point["areal_heat_capacity_j_m2_k"].as_f64().unwrap(),
        0.684948 * 1617.9,
    );
    assert_eq!(point["empirical_validation_claim"], false);
    input["layers"][0] = point["selection"].clone();
    assert_eq!(transient(&input).0.stdout, out.stdout);
    input["layers"][0]["record"]["sources"]["nist7282"]["reference"] =
        json!("https://example.invalid/unrelated-report");
    refused(&transient(&input).0);
    input["layers"][0] = point["selection"].clone();
    input["layers"][0]["allow_estimates"] = json!(false);
    refused(&transient(&input).0);
    input["layers"][0] = point["selection"].clone();
    input["outside_c"] = json!(19.0);
    refused(&transient(&input).0);
    for temperature in ["19", "71"] {
        refused(&command(&[
            "thermal-at",
            "nist/7282/cotton-duck-linear-cp",
            temperature,
            "--allow-estimates",
        ]));
    }
    refused(&command(&[
        "thermal-at",
        "nist/7282/cotton-duck-linear-cp",
        "50",
    ]));
}

#[test]
fn unapproved_extrapolated_incomplete_and_malformed_requests_publish_no_result() {
    refused(&command(&["thermal-at", "nist/7282/aralite", "50"]));
    for t in ["19", "71", "NaN", "inf", "50C"] {
        refused(&command(&[
            "thermal-at",
            "nist/7282/aralite",
            t,
            "--allow-estimates",
        ]));
    }
    for id in ["cotton-duck", "nomex-e89-crosstech", "scotchlite"] {
        refused(&command(&[
            "thermal-at",
            &format!("nist/7282/{id}"),
            "50",
            "--allow-estimates",
        ]));
    }
    refused(&command(&[
        "thermal-at",
        "nist/7282/aralite",
        "50",
        "--allow-anything",
    ]));
    let mut input = fixture();
    input["outside_c"] = json!(-10.0);
    refused(&transient(&input).0);
    input = fixture();
    input["layers"][0]["allow_estimates"] = json!(false);
    refused(&transient(&input).0);
}

#[test]
fn native_point_and_signed_transients_match_the_portable_request_authority_completely() {
    use ftx_drape::thermal::request::{self, Request};
    use ftx_pattern::{material_catalog::Selection, textile_import::catalog};

    let db = catalog().unwrap();
    let selection = Selection {
        id: "nist/7282/cotton-duck-linear-cp".into(),
        allow_estimates: true,
        ..Selection::default()
    };
    assert_eq!(
        success(&command(&[
            "thermal-at",
            &selection.id,
            "50",
            "--allow-estimates"
        ])),
        request::point(&selection, 50.0, &db).unwrap()
    );
    for cooling in [false, true] {
        let mut input = fixture();
        input["layers"][0]["id"] = json!(selection.id);
        if cooling {
            input["initial_temperature_c"] = json!(50.0);
            input["inside_c"] = json!(20.0);
            input["outside_c"] = json!(35.0);
        }
        let request: Request = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(
            success(&transient(&input).0),
            request.evaluate(&db).unwrap()
        );
    }
    let malformed = command(&["thermal-at", &selection.id, "50C", "--allow-estimates"]);
    refused(&malformed);
    let error: Value = serde_json::from_slice(&malformed.stderr).unwrap();
    assert_eq!(
        error["refusal"]["message"],
        "temperature must be in degrees Celsius"
    );
}
