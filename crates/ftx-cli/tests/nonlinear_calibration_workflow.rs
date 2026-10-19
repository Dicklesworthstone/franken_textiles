//! Actual native commands produce a frozen nonlinear selection consumed by the real drape resolver.
use ftx_drape::{material, run::DrapeOptions};
use ftx_pattern::Pattern;
use serde_json::{Value, json};
use std::process::Command;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/materials/synthetic-nonlinear-swatch.json"
);

fn invoke(primary: bool, allow: bool) -> Value {
    let mut command = Command::new(if primary {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    if primary {
        command.arg("materials");
    }
    command.args(["calibrate-nonlinear", FIXTURE]);
    if allow {
        command.arg("--allow-estimates");
    }
    if primary {
        command.arg("--json");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    if primary {
        output["result"].clone()
    } else {
        output
    }
}

#[test]
fn both_native_commands_replay_the_same_source_complete_nonlinear_material() {
    let denied = invoke(false, false);
    assert_eq!(denied["simulation_ready"], false);
    assert_eq!(denied["selection"]["allow_estimates"], false);
    let fitted = invoke(false, true);
    assert_eq!(fitted, invoke(true, true));
    assert_eq!(fitted, invoke(false, true));
    assert_eq!(fitted["simulation_ready"], true);
    assert_eq!(fitted["fit_claim"], false);
    assert_eq!(fitted["calibration"]["parameter_count"], 11);
    assert_eq!(fitted["calibration"]["check"]["components"], 4);
    assert_eq!(denied["calibration"], fitted["calibration"]);
    let pattern = Pattern::new(json!({"fabric":{"material":fitted["selection"]}}));
    let physical = material::resolve(&pattern, &DrapeOptions::default()).unwrap();
    let selection = serde_json::from_value(fitted["selection"].clone()).unwrap();
    let selected = ftx_pattern::textile_import::catalog()
        .unwrap()
        .resolve(&selection)
        .unwrap();
    let law = selected.membrane_response().unwrap().unwrap();
    assert_eq!(
        physical.report["nonlinear_strain_range"],
        json!(law.strain_range())
    );
    assert_eq!(physical.report["record_sha256"], selected.sha256());
    let stress = law.try_evaluate([0.17, -0.08, -0.6]).unwrap().stress;
    for (actual, expected) in stress.into_iter().zip([63.2, -4.7, -30.]) {
        assert!((actual - expected).abs() < 1e-9);
    }
    assert_eq!(physical.fabric.areal_kg_m2, 0.2);
    assert_eq!(physical.options.contact.thickness, 0.00035);
    let denied_pattern = Pattern::new(json!({"fabric":{"material":denied["selection"]}}));
    assert!(material::resolve(&denied_pattern, &DrapeOptions::default()).is_err());
}

#[test]
fn nonlinear_command_does_not_interpret_unknown_flags_as_permission() {
    // The primary command uses the shared refusal exit code; the companion uses 2.
    for (binary, refused) in [
        (env!("CARGO_BIN_EXE_ftx"), 3),
        (env!("CARGO_BIN_EXE_ftx-materials"), 2),
    ] {
        let mut command = Command::new(binary);
        if binary == env!("CARGO_BIN_EXE_ftx") {
            command.arg("materials");
        }
        let output = command
            .args(["calibrate-nonlinear", FIXTURE, "--allow-anything", "true"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(refused));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}
