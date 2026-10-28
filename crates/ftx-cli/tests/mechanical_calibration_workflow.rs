//! Both native entry points preserve source-locked fits, evidence consent and actual solver admission.
//! All acquisitions are synthetic software fixtures, not measured textile validation.
use ftx_core::Refusal;
use ftx_drape::{material, run::DrapeOptions};
use ftx_pattern::{Pattern, material_catalog::Selection, textile_import::catalog};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

const CASES: [(&str, &str); 3] = [
    ("calibrate", "synthetic-swatch.json"),
    ("calibrate-bending", "synthetic-bending-coupons.json"),
    ("calibrate-friction", "synthetic-friction-sliding.json"),
];

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/materials")
        .join(name);
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn workspace() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "ftx-mechanical-calibration-cli-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn write(dir: &Path, name: &str, value: &Value) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(name))
        .unwrap()
        .write_all(&serde_json::to_vec(value).unwrap())
        .unwrap();
}

fn run(dir: &Path, primary: bool, args: &[&str]) -> Output {
    let mut command = Command::new(if primary {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(dir);
    if primary {
        command.args(["--json", "materials"]);
    }
    command.args(args).output().unwrap()
}

fn success(output: Output, primary: bool) -> Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    if primary {
        assert_eq!(value["ok"], true);
        value["result"].clone()
    } else {
        value
    }
}

fn both(dir: &Path, action: &str, name: &str, allow: bool) -> Value {
    let mut args = vec![action, name];
    if allow {
        args.push("--allow-estimates");
    }
    let result = success(run(dir, false, &args), false);
    assert_eq!(result, success(run(dir, true, &args), true));
    result
}

fn refusal(output: Output, primary: bool) -> Value {
    assert_eq!(output.status.code(), Some(if primary { 3 } else { 2 }));
    let value: Value = if primary {
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        value
    } else {
        assert!(output.stdout.is_empty());
        serde_json::from_slice(&output.stderr).unwrap()
    };
    assert!(value.get("result").is_none());
    assert!(value.get("selection").is_none());
    assert!(value.get("binding").is_none());
    value["refusal"].clone()
}

#[test]
fn swatch_and_bending_routes_replay_identical_materials_in_the_actual_drape_resolver() {
    let dir = workspace();
    for (action, name) in &CASES[..2] {
        let input = fixture(name);
        write(&dir, name, &input);
        let denied = both(&dir, action, name, false);
        let fitted = both(&dir, action, name, true);
        assert_eq!(denied["calibration"], fitted["calibration"]);
        assert_eq!(denied["simulation_ready"], false);
        assert_eq!(denied["selection"]["allow_estimates"], false);
        assert_eq!(fitted["simulation_ready"], true);
        assert_eq!(fitted["fit_claim"], false);

        let selection: Selection =
            serde_json::from_str(fitted["selection_json"].as_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&selection).unwrap(),
            fitted["selection"]
        );
        let frozen = catalog().unwrap().resolve(&selection).unwrap();
        assert_eq!(Some(frozen.sha256()), selection.expected_sha256.as_deref());
        let physical = material::resolve(
            &Pattern::new(json!({"fabric":{"material":selection}})),
            &DrapeOptions::default(),
        )
        .unwrap();
        assert_eq!(physical.report["record_sha256"], frozen.sha256());
        assert!(
            material::resolve(
                &Pattern::new(json!({"fabric":{"material":denied["selection"]}})),
                &DrapeOptions::default(),
            )
            .is_err()
        );
        if *action == "calibrate" {
            let q = physical.fabric.membrane(1.0).q;
            for (actual, expected) in [q[0][0], q[1][1], q[2][2]]
                .into_iter()
                .zip([9000.0, 4000.0, 700.0])
            {
                assert!((actual - expected).abs() < 1e-8);
            }
            assert_eq!(physical.options.contact.thickness, 0.00035);
        } else {
            let bending = frozen.bending_response().unwrap().unwrap();
            let matrix = bending.matrix();
            for (actual, expected) in [matrix[0][0], matrix[1][1], matrix[0][1], matrix[2][2]]
                .into_iter()
                .zip([4e-4, 1.5e-4, 5e-5, 8e-5])
            {
                assert!((actual - expected).abs() < 1e-12);
            }
            for (actual, expected) in bending
                .preferred_curvature()
                .into_iter()
                .zip([1.0, -0.5, 0.25])
            {
                assert!((actual - expected).abs() < 1e-10);
            }
        }
    }
}

#[test]
fn friction_routes_preserve_pair_consent_and_replay_body_and_self_contact() {
    let dir = workspace();
    let mut input = fixture(CASES[2].1);
    write(&dir, "pair.json", &input);
    let denied = both(&dir, "calibrate-friction", "pair.json", false);
    let fitted = both(&dir, "calibrate-friction", "pair.json", true);
    assert_eq!(denied["calibration"], fitted["calibration"]);
    assert_eq!(denied["readiness"]["ready"], false);
    assert_eq!(fitted["readiness"]["ready"], true);
    assert_eq!(fitted["readiness"]["whole_garment_checked"], false);
    let binding: Value = serde_json::from_str(fitted["binding_json"].as_str().unwrap()).unwrap();
    assert_eq!(binding, fitted["binding"]);
    let interface: ftx_pattern::material_friction::Interface =
        serde_json::from_str(fitted["interface_json"].as_str().unwrap()).unwrap();
    interface.validate().unwrap();
    assert!((interface.friction.value - 0.4).abs() < 1e-12);
    for selection in binding["materials"].as_array().unwrap() {
        let selection: Selection = serde_json::from_value(selection.clone()).unwrap();
        let material = catalog().unwrap().resolve(&selection).unwrap();
        assert!(
            interface
                .materials
                .iter()
                .any(|hash| hash == material.sha256())
        );
    }

    input["target"]["materials"][0]["allow_estimates"] = json!(false);
    write(&dir, "pair-sheet-denied.json", &input);
    let partial = both(&dir, "calibrate-friction", "pair-sheet-denied.json", true);
    assert_eq!(partial["readiness"]["ready"], true);
    assert_eq!(partial["readiness"]["sheets"][0]["ready"], false);
    assert_eq!(partial["binding"]["materials"][0]["allow_estimates"], false);

    for target in ["body", "self_contact"] {
        let mut input = fixture(CASES[2].1);
        input["target"] = json!({"kind":target,"material":input["target"]["materials"][0]});
        if target == "body" {
            input["target"]["counterface"] = json!({"reference":"test:body-surrogate",
                "accessed_on":"2026-10-18","note":"Synthetic counterface, not a measurement on skin"});
        }
        input["replace_existing"] = json!(true);
        let name = format!("{target}.json");
        write(&dir, &name, &input);
        let fitted = both(&dir, "calibrate-friction", &name, true);
        assert_eq!(fitted["readiness"]["scope"], "material_mechanics");
        assert_eq!(fitted["readiness"]["ready"], true);
        let selection: Selection =
            serde_json::from_str(fitted["selection_json"].as_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&selection).unwrap(),
            fitted["binding"]["selection"]
        );
        let options = DrapeOptions {
            self_contact: Some(DrapeOptions::default().contact),
            ..DrapeOptions::default()
        };
        let physical = material::resolve(
            &Pattern::new(json!({"fabric":{"material":selection}})),
            &options,
        )
        .unwrap();
        let (actual, unchanged) = if target == "body" {
            (
                physical.options.contact.mu,
                physical.options.self_contact.unwrap().mu,
            )
        } else {
            (
                physical.options.self_contact.unwrap().mu,
                physical.options.contact.mu,
            )
        };
        assert!((actual - 0.4).abs() < 1e-12);
        assert_eq!(unchanged, if target == "body" { 0.3 } else { 0.2 });
    }
}

#[test]
fn consumer_estimate_consent_never_admits_unpermitted_observations() {
    let dir = workspace();
    for (action, name) in CASES {
        let mut input = fixture(name);
        input["allow_estimated_observations"] = json!(false);
        write(&dir, name, &input);
        let expected: Refusal = match action {
            "calibrate" => ftx_pattern::material_calibration::calibrate(
                &serde_json::from_value(input).unwrap(),
            )
            .unwrap_err(),
            "calibrate-bending" => ftx_pattern::material_bending_calibration::calibrate(
                &serde_json::from_value(input).unwrap(),
            )
            .unwrap_err(),
            _ => ftx_pattern::material_friction::calibrate(
                &serde_json::from_value(input).unwrap(),
                &catalog().unwrap(),
            )
            .unwrap_err(),
        };
        for primary in [false, true] {
            let observed = refusal(
                run(&dir, primary, &[action, name, "--allow-estimates"]),
                primary,
            );
            assert_eq!(observed, serde_json::to_value(&expected).unwrap());
        }
    }
}

#[test]
fn successful_fits_leave_missing_mechanical_properties_unavailable() {
    let dir = workspace();
    for (action, name) in CASES {
        let mut input = fixture(name);
        let properties = if action == "calibrate-friction" {
            &mut input["target"]["materials"][0]["record"]["properties"]
        } else {
            &mut input["material"]["properties"]
        };
        properties.as_object_mut().unwrap().remove("thickness");
        write(&dir, name, &input);
        let fitted = both(&dir, action, name, true);
        assert!(fitted["calibration"].is_object());
        if action == "calibrate-friction" {
            assert_eq!(fitted["readiness"]["scope"], "interface_only");
            assert_eq!(fitted["readiness"]["ready"], true);
            assert_eq!(fitted["readiness"]["sheets"][0]["ready"], false);
            assert!(fitted["readiness"]["sheets"][0]["refusal"].is_object());
            assert!(
                fitted["binding"]["materials"][0]["record"]["properties"]
                    .get("thickness")
                    .is_none()
            );
        } else {
            assert_eq!(fitted["simulation_ready"], false);
            assert!(fitted["mechanics_refusal"].is_object());
            assert!(
                fitted["selection"]["record"]["properties"]
                    .get("thickness")
                    .is_none()
            );
        }
    }
}

#[test]
fn unknown_flags_and_extra_arguments_do_not_publish_partial_calibrations() {
    let dir = workspace();
    for (action, name) in CASES {
        write(&dir, name, &fixture(name));
        for primary in [false, true] {
            for extra in ["--allow-anything", "extra.json"] {
                let error = refusal(run(&dir, primary, &[action, name, extra]), primary);
                assert!(error.is_object());
            }
        }
    }
}
