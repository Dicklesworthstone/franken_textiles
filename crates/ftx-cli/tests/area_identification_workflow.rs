//! Synthetic joint area identification reaches the executable adapters and production forward solver.
//! Frozen numerical coefficients do not constitute measured textile or garment validation.
use ftx_core::Refusal;
use ftx_drape::strip::{self, Controls, Fixture, Request};
use ftx_pattern::{material_calibration::nonlinear, material_catalog::Selection, textile_import};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const SWATCH: &str = include_str!("../../../examples/area-identification/swatch.json");

fn workspace() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ftx-area-identification-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
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
        "{} / {}",
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

fn refused(output: Output, primary: bool, expected: &Refusal) {
    assert_eq!(output.status.code(), Some(if primary { 3 } else { 2 }));
    let value: Value = if primary {
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        value
    } else {
        assert!(output.stdout.is_empty());
        serde_json::from_slice(&output.stderr).unwrap()
    };
    assert_eq!(value["refusal"], serde_json::to_value(expected).unwrap());
    assert!(value.get("result").is_none());
}

#[test]
fn joint_swatch_identification_publishes_one_exact_selection_and_replays_both_commands() {
    let dir = workspace();
    let input: nonlinear::Input = serde_json::from_str(SWATCH).unwrap();
    write(&dir, "swatch.json", &serde_json::from_str(SWATCH).unwrap());
    let expected = nonlinear::calibrate(&input).unwrap();
    let identified = success(
        run(
            &dir,
            false,
            &["calibrate-nonlinear", "swatch.json", "--allow-estimates"],
        ),
        false,
    );
    assert_eq!(
        identified,
        success(
            run(
                &dir,
                true,
                &["calibrate-nonlinear", "swatch.json", "--allow-estimates"],
            ),
            true,
        )
    );
    assert_eq!(
        identified["calibration"],
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(identified["calibration"]["parameter_count"], 9);
    assert_eq!(identified["calibration"]["fit"]["components"], 24);
    assert_eq!(identified["calibration"]["check"]["components"], 4);
    assert_eq!(identified["fit_claim"], false);
    assert_eq!(identified["simulation_ready"], false);
    assert_eq!(
        identified["calibration_observations_are_not_independent_garment_validation"],
        true
    );
    let selection: Selection =
        serde_json::from_str(identified["selection_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&selection).unwrap(),
        identified["selection"]
    );
    let snapshot = textile_import::catalog()
        .unwrap()
        .resolve(&selection)
        .unwrap();
    assert_eq!(
        selection.expected_sha256.as_deref(),
        Some(snapshot.sha256())
    );
    assert!(snapshot.authored().properties.is_empty());
    assert!(snapshot.mechanics().is_err());
    let response = snapshot.authored().membrane_response.as_ref().unwrap();
    let area = response.area_response.as_ref().unwrap();
    assert!((area.quadratic.value - 40.0).abs() < 1e-7);
    assert!((area.log_squared.value - 15.0).abs() < 1e-7);
    assert_eq!(
        json!(area.area_ratio_range),
        identified["calibration"]["fitted_area_ratio_range"]
    );
    assert!(area.area_ratio_range[0] > 0.7 && area.area_ratio_range[1] < 1.5);
    for coefficient in [&area.quadratic, &area.log_squared] {
        assert_eq!(
            serde_json::to_value(coefficient.evidence).unwrap(),
            "estimate"
        );
        assert_eq!(coefficient.conditions["specimen"], "synthetic-area-swatch");
        assert!(
            snapshot
                .authored()
                .sources
                .contains_key(&coefficient.source)
        );
    }

    let request = Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selection,
        fixture: Fixture {
            gauge_length_m: 0.1,
            width_m: 0.06,
            warp_axis: [0.0, 1.0],
            cells: [2, 4],
        },
        extensions: vec![0.0, 0.02],
        controls: Controls::default(),
    };
    let forward = strip::evaluate(&request).unwrap();
    assert!(forward.states[1].force_n > 0.0);
    assert!(forward.states[1].mid_width_m < request.fixture.width_m);
    let replay = serde_json::to_value(&forward.input).unwrap();
    write(&dir, "forward.json", &replay);
    for primary in [false, true] {
        assert_eq!(
            success(
                run(&dir, primary, &["strip-experiment", "forward.json"]),
                primary
            ),
            serde_json::to_value(&forward).unwrap()
        );
    }
    assert_eq!(
        serde_json::to_string(&strip::evaluate(&forward.input).unwrap()).unwrap(),
        serde_json::to_string(&forward).unwrap()
    );

    let denied = success(
        run(&dir, false, &["calibrate-nonlinear", "swatch.json"]),
        false,
    );
    assert_eq!(denied["calibration"], identified["calibration"]);
    assert_eq!(denied["selection"]["allow_estimates"], false);
    let mut no_consent = request;
    no_consent.material = serde_json::from_value(denied["selection"].clone()).unwrap();
    assert!(strip::evaluate(&no_consent).is_err());
}

#[test]
fn native_area_identification_preserves_acquisition_admission_refusals() {
    let dir = workspace();
    let original: Value = serde_json::from_str(SWATCH).unwrap();
    let mut mismatch = original.clone();
    mismatch["conditions"]["specimen"] = json!("another specimen");
    let mut denied = original;
    denied["allow_estimated_observations"] = json!(false);
    for (name, input) in [("mismatch.json", mismatch), ("denied.json", denied)] {
        let typed: nonlinear::Input = serde_json::from_value(input.clone()).unwrap();
        let expected = nonlinear::calibrate(&typed).unwrap_err();
        write(&dir, name, &input);
        for primary in [false, true] {
            refused(
                run(
                    &dir,
                    primary,
                    &["calibrate-nonlinear", name, "--allow-estimates"],
                ),
                primary,
                &expected,
            );
        }
    }
}
