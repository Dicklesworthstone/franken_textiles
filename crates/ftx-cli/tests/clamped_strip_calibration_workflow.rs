//! Synthetic finite-strip observations through both native commands and frozen material binding.
//! Identified membrane coefficients do not establish empirical textile or garment validation.

use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use ftx_core::receipt::sha256_hex;
use ftx_drape::strip::fit::Input;
use ftx_pattern::{material_catalog::Selection, textile_import};
use serde_json::{Value, json};

const INPUT: &str = include_str!("../../../tests/fixtures/materials/synthetic-clamped-strip.json");

#[test]
fn both_commands_preserve_explicit_extensometers_through_identification() {
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../examples/strip-identification/synthetic-calibration.json"
    ))
    .unwrap();
    for experiment in input["experiments"].as_array_mut().unwrap() {
        for observation in experiment["observations"].as_array_mut().unwrap() {
            let width = observation
                .as_object_mut()
                .unwrap()
                .remove("mid_width")
                .unwrap();
            observation["extensometers"] = json!([{
                "id":"midpoint", "operator":{"kind":"edge_chord","station_fraction":0.5},
                "value_m":width["value_m"], "scale_m":width["scale_m"]
            }]);
        }
    }
    let workspace = Workspace::new();
    fs::write(workspace.0.join("acquisition.json"), input.to_string()).unwrap();
    let primary = workspace.identify(true, true);
    let companion = workspace.identify(false, true);
    assert_eq!(primary["calibration"], companion["calibration"]);
    assert_eq!(primary["selection_json"], companion["selection_json"]);
    let report = &primary["calibration"];
    assert_eq!(report["accepted"], true, "{report}");
    assert_eq!(report["domain_replay_verified"], true);
    let typed: Input = serde_json::from_value(input).unwrap();
    assert_eq!(
        report["input"]["experiments"],
        serde_json::to_value(&typed.experiments).unwrap()
    );
    for prediction in report["predictions"].as_array().unwrap() {
        assert_eq!(prediction["extensometers"][0]["id"], "midpoint");
        assert_eq!(
            prediction["extensometers"][0]["operator"],
            json!({"kind":"edge_chord","station_fraction":0.5})
        );
        assert!(
            prediction["extensometers"][0]["predicted_m"]
                .as_f64()
                .unwrap()
                > 0.0
        );
        assert!(prediction["extensometers"][0]["residual_m"].is_number());
    }
    assert_eq!(primary["simulation_ready"], false);
    assert_eq!(primary["empirical_validation_claim"], false);
}

#[test]
fn both_commands_identify_the_joint_area_example_without_claiming_complete_garment_physics() {
    let workspace = Workspace::new();
    fs::write(
        workspace.0.join("acquisition.json"),
        include_str!("../../../examples/area-response/identify.json"),
    )
    .unwrap();
    let primary = workspace.identify(true, true);
    let companion = workspace.identify(false, true);
    assert_eq!(primary["calibration"], companion["calibration"]);
    let report = &primary["calibration"];
    assert_eq!(report["accepted"], true, "{report}");
    assert_eq!(report["domain_replay_verified"], true);
    let parameters = report["parameters"].as_array().unwrap();
    assert_eq!(parameters.len(), 6);
    for (actual, known) in parameters.iter().zip([1.1, 0.9, 1.2, -80.0, 31.0, 19.0]) {
        assert!((actual.as_f64().unwrap() - known).abs() < 1e-3);
    }
    assert_eq!(primary["selection_json"], companion["selection_json"]);
    assert_eq!(primary["selection"], report["selection"]);
    assert_eq!(primary["simulation_ready"], false);
    assert_eq!(primary["fit_claim"], false);
    assert_eq!(primary["empirical_validation_claim"], false);
}

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ftx-clamped-strip-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("acquisition.json"), INPUT).unwrap();
        Self(path)
    }

    fn identify(&self, primary: bool, admit: bool) -> Value {
        let mut command = Command::new(if primary {
            env!("CARGO_BIN_EXE_ftx")
        } else {
            env!("CARGO_BIN_EXE_ftx-materials")
        });
        command.current_dir(&self.0);
        if primary {
            command.args(["--json", "materials"]);
        }
        command.args(["calibrate-strip", "acquisition.json"]);
        if admit {
            command.arg("--allow-estimates");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        result.get("result").unwrap_or(&result).clone()
    }

    fn workbench(&self, arguments: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
            .current_dir(&self.0)
            .args(arguments)
            .output()
            .unwrap()
    }
}

#[test]
fn both_commands_keep_identification_separate_from_estimate_consent_and_missing_physics() {
    let workspace = Workspace::new();
    // Two inverse runs total: both executable adapters and both consumption policies.
    let denied = workspace.identify(false, false);
    let admitted = workspace.identify(true, true);
    for field in ["parameters", "fit", "check", "fitted_green_strain_range"] {
        assert_eq!(denied["calibration"][field], admitted["calibration"][field]);
    }
    let catalog = textile_import::catalog().unwrap();
    for (result, consent) in [(&denied, false), (&admitted, true)] {
        let calibration = &result["calibration"];
        assert_eq!(calibration["accepted"], true, "{calibration}");
        assert_eq!(calibration["reasons"], json!([]));
        assert_eq!(calibration["domain_replay_verified"], true);
        assert!(calibration["check"].is_object());
        let mut input: Input = serde_json::from_str(INPUT).unwrap();
        input.allow_estimates = consent;
        let input_hash = sha256_hex(&serde_json::to_vec(&input).unwrap());
        assert_eq!(calibration["input_sha256"], input_hash);
        assert_eq!(calibration["input"], serde_json::to_value(&input).unwrap());
        let source = format!("strip-{input_hash}");
        let material = &result["selection"]["record"];
        assert_eq!(material, &calibration["material"]);
        assert_eq!(result["selection"], calibration["selection"]);
        assert_eq!(result["selection_json"], calibration["selection_json"]);
        assert_eq!(
            material["sources"][&source]["reference"],
            format!("local:ftx-strip-calibration/{input_hash}")
        );
        for (key, original) in &input.material.sources {
            assert_eq!(
                material["sources"][key],
                serde_json::to_value(original).unwrap()
            );
        }
        for key in [
            "warp_stiffness",
            "weft_stiffness",
            "poisson_warp_weft",
            "shear_stiffness",
        ] {
            let coefficient = &material["properties"][key];
            assert_eq!(coefficient["evidence"], "estimate");
            assert_eq!(coefficient["source"], source);
            let domain: Value = serde_json::from_str(
                coefficient["conditions"]["fitted_green_strain_range"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(domain, calibration["fitted_green_strain_range"]);
        }
        assert_eq!(result["selection"]["allow_estimates"], consent);
        assert_eq!(result["fit_claim"], false);
        assert_eq!(result["empirical_validation_claim"], false);
        // The fixture has no independent bending, thickness or contact observations.
        assert_eq!(result["simulation_ready"], false);
        assert!(result["mechanics_refusal"].is_object());
        let selection: Selection =
            serde_json::from_str(result["selection_json"].as_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&selection).unwrap(),
            result["selection"]
        );
        let resolved = catalog.resolve(&selection).unwrap();
        assert_eq!(
            selection.expected_sha256.as_deref(),
            Some(resolved.sha256())
        );
        assert_eq!(
            resolved
                .material()
                .require("warp_stiffness", "N/m", resolved.allows_estimates())
                .is_ok(),
            consent
        );
        assert!(resolved.mechanics().is_err());
    }
    // Explicit adapter consent changes the receipt and snapshot identity, never fitted physics.
    assert_ne!(
        denied["calibration"]["input_sha256"],
        admitted["calibration"]["input_sha256"]
    );
    assert_ne!(
        denied["selection"]["expected_sha256"],
        admitted["selection"]["expected_sha256"]
    );
    let original = json!({"schema":"fabric-pattern/1","units":"mm","seams":[],
        "fabric":{"note":"Keep authored metadata"},
        "pieces":[{"id":"swatch","material":"shell","cut":{"qty":1},
            "vertices":{"a":[0,0],"b":[100,0],"c":[0,100]},
            "edges":[{"id":"ab","from":"a","to":"b"},
                {"id":"bc","from":"b","to":"c"},{"id":"ca","from":"c","to":"a"}],
            "grain":[[0,0],[0,100]]}]});
    fs::write(
        workspace.0.join("pattern.json"),
        serde_json::to_vec(&original).unwrap(),
    )
    .unwrap();
    fs::write(
        workspace.0.join("selection.json"),
        admitted["selection_json"].as_str().unwrap(),
    )
    .unwrap();
    let bound = workspace.workbench(&["bind", "pattern.json", "selection.json", "bound.json"]);
    assert!(
        bound.status.success(),
        "{}",
        String::from_utf8_lossy(&bound.stderr)
    );
    let bound: Value = serde_json::from_slice(&bound.stdout).unwrap();
    assert_eq!(bound["geometry_changed"], false);
    assert_eq!(bound["simulation_ready"], false);
    assert_eq!(
        bound["material_sha256"],
        admitted["selection"]["expected_sha256"]
    );
    for missing in [
        "bending_rigidity",
        "thickness",
        "friction_skin",
        "friction_self",
    ] {
        assert!(
            bound["missing_mechanics"]
                .as_array()
                .unwrap()
                .contains(&json!(missing))
        );
    }
    let saved: Value =
        serde_json::from_slice(&fs::read(workspace.0.join("bound.json")).unwrap()).unwrap();
    assert_eq!(saved["pieces"], original["pieces"]);
    assert_eq!(saved["seams"], original["seams"]);
    assert_eq!(saved["fabric"]["note"], original["fabric"]["note"]);
    assert_eq!(saved["fabric"]["material"], admitted["selection"]);
    let mut changed: Selection = serde_json::from_value(admitted["selection"].clone()).unwrap();
    changed
        .record
        .as_mut()
        .unwrap()
        .properties
        .get_mut("warp_stiffness")
        .unwrap()
        .value += 1.0;
    assert!(catalog.resolve(&changed).is_err());
    let mut changed: Selection = serde_json::from_value(admitted["selection"].clone()).unwrap();
    changed.allow_estimates = false;
    assert!(catalog.resolve(&changed).is_err());
}

#[test]
fn strip_commands_do_not_treat_unknown_flags_as_permission() {
    let workspace = Workspace::new();
    for (binary, refused) in [
        (env!("CARGO_BIN_EXE_ftx"), 3),
        (env!("CARGO_BIN_EXE_ftx-materials"), 2),
    ] {
        let mut command = Command::new(binary);
        command.current_dir(&workspace.0);
        if binary == env!("CARGO_BIN_EXE_ftx") {
            command.arg("materials");
        }
        let output = command
            .args(["calibrate-strip", "acquisition.json", "--allow-anything"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(refused));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}
