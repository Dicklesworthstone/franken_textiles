//! Both native boundaries preserve source acquisitions and call the production strip solver.
//! Forward material values are explicit synthetic software fixtures, never textile measurements.
use ftx_drape::strip::{self, Controls, Fixture, Request};
use ftx_pattern::{
    material_catalog::{Selection, knit_strips},
    materials::{CATALOG_SCHEMA, Catalog, Material},
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const ACQUISITION: &str =
    include_str!("../../ftx-pattern/src/material_catalog/knit-strip-measurements.json");
const INVERSE: &str =
    include_str!("../../../examples/strip-identification/synthetic-calibration.json");
const NONLINEAR: &str =
    include_str!("../../../examples/strip-identification/synthetic-nonlinear-calibration.json");
const INDEPENDENT_INTERVALS: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-nonlinear-clamped-strip.json");

fn workspace() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ftx-strip-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn write(dir: &Path, name: &str, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(name))
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn forward() -> Request {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-compression.json"
    ))
    .unwrap();
    let mut material: Material = serde_json::from_value(fixture["material"].clone()).unwrap();
    material.properties.retain(|key, _| {
        [
            "warp_stiffness",
            "weft_stiffness",
            "poisson_warp_weft",
            "shear_stiffness",
        ]
        .contains(&key.as_str())
    });
    Request {
        schema: "ftx-strip-experiment/1".into(),
        material: Selection {
            id: material.id.clone(),
            record: Some(material),
            allow_estimates: true,
            ..Selection::default()
        },
        fixture: Fixture {
            gauge_length_m: 0.1,
            width_m: 0.06,
            warp_axis: [0.0, 1.0],
            cells: [2, 4],
        },
        extensions: vec![0.0, 0.04],
        controls: Controls::default(),
    }
}

fn run(dir: &Path, unified: bool, args: &[&str]) -> Output {
    let mut command = Command::new(if unified {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(dir);
    if unified {
        command.arg("materials");
    }
    command.args(args);
    if unified {
        command.arg("--json");
    }
    command.output().unwrap()
}

fn success(output: &Output, unified: bool) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    if unified {
        assert_eq!(value["schema"], "ftx-cli/1");
        assert_eq!(value["ok"], true);
        assert_eq!(value["exit"], 0);
        value["result"].clone()
    } else {
        value
    }
}

fn refused(output: &Output, unified: bool, code: &str) {
    let exit = if unified {
        if code == "budget_exceeded" { 4 } else { 3 }
    } else {
        2
    };
    assert_eq!(output.status.code(), Some(exit));
    let value: Value = if unified {
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["exit"], exit);
        value
    } else {
        assert!(output.stdout.is_empty());
        serde_json::from_slice(&output.stderr).unwrap()
    };
    assert_eq!(value["refusal"]["code"], code);
    assert!(value.get("result").is_none());
}

#[test]
fn both_commands_preserve_grip_equilibria_and_replay_frozen_material_without_unrelated_properties()
{
    let dir = workspace();
    let input = forward();
    let original = serde_json::to_vec_pretty(&input).unwrap();
    write(&dir, "forward.json", &original);
    let core = strip::evaluate(&input).unwrap();
    assert!(core.states[1].force_n > 0.0);
    assert!(core.states[1].mid_width_m < input.fixture.width_m);
    let empty = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    let material = empty.resolve(&core.input.material).unwrap();
    assert!(!material.material().properties.contains_key("areal_density"));
    assert!(!material.material().missing_mechanics().is_empty());
    let replay = serde_json::to_vec(&core.input).unwrap();
    write(&dir, "replay.json", &replay);
    for unified in [false, true] {
        let report = success(
            &run(&dir, unified, &["strip-experiment", "forward.json"]),
            unified,
        );
        assert_eq!(report, serde_json::to_value(&core).unwrap());
        assert_eq!(
            report,
            success(
                &run(&dir, unified, &["strip-experiment", "replay.json"]),
                unified
            )
        );
        assert_eq!(report["input"]["material"]["allow_estimates"], true);
        assert_eq!(report["states"][1]["extension_fraction"], 0.04);
    }
    assert_eq!(fs::read(dir.join("forward.json")).unwrap(), original);
    assert_eq!(fs::read(dir.join("replay.json")).unwrap(), replay);
}

#[test]
fn both_acquisition_commands_keep_original_text_identity_and_all_loading_and_unloading_rows() {
    let dir = workspace();
    // Whitespace is deliberately distinct: the returned acquisition hash must identify this file.
    let raw = format!("\n{ACQUISITION}\n");
    write(&dir, "acquisition.json", raw.as_bytes());
    let expected = knit_strips::parse_acquisition(&raw).unwrap();
    for unified in [false, true] {
        let report = success(
            &run(&dir, unified, &["strip-acquisition", "acquisition.json"]),
            unified,
        );
        assert_eq!(report, serde_json::to_value(&expected).unwrap());
        assert_eq!(
            report["acquisition_sha256"],
            ftx_core::receipt::sha256_hex(raw.as_bytes())
        );
        assert_eq!(
            report["curves"][0]["observations"]
                .as_array()
                .unwrap()
                .len(),
            916
        );
        assert_eq!(
            report["curves"][0]["observations"][915]["branch"],
            "unloading"
        );
        for key in ["material", "selection", "selection_json"] {
            assert!(report.get(key).is_none());
        }
    }
    assert_eq!(
        fs::read(dir.join("acquisition.json")).unwrap(),
        raw.as_bytes()
    );
}

#[test]
fn both_inverse_commands_keep_candidate_decisions_source_permissions_and_exact_selections() {
    let dir = workspace();
    let mut input: strip::fit::Input = serde_json::from_str(INVERSE).unwrap();
    let empty = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: vec![],
    };
    for allow in [false, true] {
        let name = format!("inverse-{allow}.json");
        let original = serde_json::to_vec_pretty(&input).unwrap();
        write(&dir, &name, &original);
        let mut effective = input.clone();
        effective.allow_estimates = allow;
        let core = strip::fit::calibrate(&effective).unwrap();
        assert!(core.accepted, "{:?}", core.reasons);
        for unified in [false, true] {
            let mut args = vec!["calibrate-strip", &name];
            if allow {
                args.push("--allow-estimates");
            }
            let report = success(&run(&dir, unified, &args), unified);
            assert_eq!(report["calibration"], serde_json::to_value(&core).unwrap());
            assert_eq!(report["selection"]["allow_estimates"], allow);
            let raw = report["selection_json"].as_str().unwrap();
            assert_eq!(Some(raw), core.selection_json.as_deref());
            let selection = serde_json::from_str(raw).unwrap();
            let material = empty.resolve(&selection).unwrap();
            assert_eq!(material.allows_estimates(), allow);
            assert_eq!(material.material().properties.len(), 4);
            assert!(material.mechanics().is_err());
        }
        assert_eq!(fs::read(dir.join(name)).unwrap(), original);
    }
    let accepted = strip::fit::calibrate(&input).unwrap();
    let check = input
        .experiments
        .iter_mut()
        .find(|e| e.partition == ftx_pattern::material_calibration::Partition::Check)
        .unwrap();
    check.observations[0].force_n += 10.0;
    let original = serde_json::to_vec(&input).unwrap();
    write(&dir, "rejected.json", &original);
    let core = strip::fit::calibrate(&input).unwrap();
    assert!(!core.accepted);
    assert_eq!(core.parameters, accepted.parameters);
    for unified in [false, true] {
        // A rejected candidate is a reviewable completed computation, not a published material.
        let report = success(
            &run(
                &dir,
                unified,
                &["calibrate-strip", "rejected.json", "--allow-estimates"],
            ),
            unified,
        );
        assert_eq!(report["calibration"], serde_json::to_value(&core).unwrap());
        assert_eq!(report["calibration"]["accepted"], false);
        for field in ["material", "selection", "selection_json"] {
            assert!(report[field].is_null());
        }
        assert!(
            !report["calibration"]["reasons"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            report["calibration"]["check"]["peak_scaled"]
                .as_f64()
                .unwrap()
                > 90.0
        );
    }
    assert_eq!(fs::read(dir.join("rejected.json")).unwrap(), original);
    input.allow_estimated_observations = false;
    write(
        &dir,
        "unapproved-observations.json",
        &serde_json::to_vec(&input).unwrap(),
    );
    for unified in [false, true] {
        refused(
            &run(
                &dir,
                unified,
                &["calibrate-strip", "unapproved-observations.json"],
            ),
            unified,
            "invalid_document",
        );
    }
}

#[test]
fn nonlinear_strip_commands_keep_shape_observation_and_output_policies_independent() {
    let dir = workspace();
    write(&dir, "nonlinear.json", NONLINEAR.as_bytes());
    let input: Value = serde_json::from_str(NONLINEAR).unwrap();
    let mut fitted = Value::Null;
    for unified in [false, true] {
        // Exercise both output policies without changing the supplied shape/observation consent.
        let mut args = vec!["calibrate-strip", "nonlinear.json"];
        if unified {
            args.push("--allow-estimates");
        }
        let result = success(&run(&dir, unified, &args), unified);
        let calibration = &result["calibration"];
        assert_eq!(calibration["accepted"], true, "{calibration}");
        assert_eq!(calibration["domain_replay_verified"], true);
        assert_eq!(
            calibration["input"]["response_family"],
            input["response_family"]
        );
        assert_eq!(calibration["input"]["allow_estimated_observations"], true);
        assert_eq!(calibration["input"]["allow_estimates"], unified);
        if !fitted.is_null() {
            assert_eq!(calibration["parameters"], fitted);
        }
        fitted = calibration["parameters"].clone();
        assert_eq!(calibration["parameter_descriptors"][2]["id"], "shear_scale");
        assert_eq!(calibration["parameter_descriptors"][2]["unit"], "1");
        assert_eq!(
            calibration["parameter_descriptors"][3]["id"],
            "normal_coupling"
        );
        assert_eq!(calibration["parameter_descriptors"][3]["unit"], "N/m");
        let raw = result["selection_json"].as_str().unwrap();
        assert_eq!(result["selection_json"], calibration["selection_json"]);
        let selection: Selection = serde_json::from_str(raw).unwrap();
        let resolved = ftx_pattern::textile_import::catalog()
            .unwrap()
            .resolve(&selection)
            .unwrap();
        assert_eq!(
            serde_json::to_string(&resolved.frozen_selection()).unwrap(),
            raw
        );
        assert_eq!(resolved.membrane_response().is_ok(), unified);
        assert_eq!(result["simulation_ready"], false);
        let record = selection.record.as_ref().unwrap();
        assert!(record.properties.is_empty());
        let response = record.membrane_response.as_ref().unwrap();
        assert_eq!(response.model, "tabulated_green_strain/1");
        for curve in [&response.warp, &response.weft, &response.shear] {
            assert_eq!(curve.evidence, ftx_pattern::materials::Evidence::Estimate);
        }
        for (key, source) in input["material"]["sources"].as_object().unwrap() {
            assert_eq!(serde_json::to_value(&record.sources[key]).unwrap(), *source);
        }
    }
    for (name, shape) in [
        ("denied-shapes.json", true),
        ("denied-observations.json", false),
    ] {
        let mut denied = input.clone();
        if shape {
            denied["response_family"]["allow_estimated_shapes"] = json!(false);
        } else {
            denied["allow_estimated_observations"] = json!(false);
        }
        write(&dir, name, &serde_json::to_vec(&denied).unwrap());
        for unified in [false, true] {
            refused(
                &run(
                    &dir,
                    unified,
                    &["calibrate-strip", name, "--allow-estimates"],
                ),
                unified,
                "invalid_document",
            );
        }
    }
    let mut rejected = input;
    let check = rejected["experiments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["partition"] == "check")
        .unwrap();
    let force = check["observations"][0]["force_n"].as_f64().unwrap();
    check["observations"][0]["force_n"] = json!(force + 10.0);
    write(
        &dir,
        "rejected-nonlinear.json",
        &serde_json::to_vec(&rejected).unwrap(),
    );
    for unified in [false, true] {
        let result = success(
            &run(
                &dir,
                unified,
                &[
                    "calibrate-strip",
                    "rejected-nonlinear.json",
                    "--allow-estimates",
                ],
            ),
            unified,
        );
        assert_eq!(result["calibration"]["accepted"], false);
        assert_eq!(result["calibration"]["parameters"], fitted);
        assert!(
            result["calibration"]["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason.as_str().unwrap().contains("CHECK"))
        );
        assert!(result["selection"].is_null() && result["selection_json"].is_null());
        assert!(result["calibration"]["material"].is_null());
    }
    assert_eq!(
        fs::read(dir.join("nonlinear.json")).unwrap(),
        NONLINEAR.as_bytes()
    );
}

#[test]
fn independent_interval_strip_commands_match_core_and_replay_exact_selection() {
    let dir = workspace();
    write(&dir, "intervals.json", INDEPENDENT_INTERVALS.as_bytes());
    let input: strip::fit::Input = serde_json::from_str(INDEPENDENT_INTERVALS).unwrap();
    let core = strip::fit::calibrate(&input).unwrap();
    assert!(core.accepted, "{:?}", core.reasons);
    assert!(core.domain_replay_verified);
    assert_eq!(core.parameters.len(), 9);
    assert!(core.input.response_family.is_none());
    assert!(core.input.parameter_bounds.is_empty());
    let model = core.input.nonlinear.as_ref().unwrap();
    let bounds: Vec<_> = model
        .tangent_bounds
        .iter()
        .flatten()
        .copied()
        .chain([model.coupling_bounds])
        .collect();
    assert_eq!(core.parameter_bounds, bounds);
    let descriptors = core.parameter_descriptors.as_ref().unwrap();
    assert_eq!(descriptors.len(), 9);
    assert_eq!(descriptors[0].id, "warp_tangent_0");
    assert_eq!(descriptors[8].id, "normal_coupling");
    assert!(descriptors.iter().all(|parameter| parameter.unit == "N/m"));

    let raw_selection = core.selection_json.as_deref().unwrap();
    let selection: Selection = serde_json::from_str(raw_selection).unwrap();
    let resolved = ftx_pattern::textile_import::catalog()
        .unwrap()
        .resolve(&selection)
        .unwrap();
    assert_eq!(
        serde_json::to_string(&resolved.frozen_selection()).unwrap(),
        raw_selection
    );
    assert!(resolved.membrane_response().unwrap().is_some());
    assert!(resolved.material().properties.is_empty());
    assert!(resolved.mechanics().is_err());

    let acquired = input
        .experiments
        .iter()
        .find(|experiment| {
            experiment.partition == ftx_pattern::material_calibration::Partition::Check
        })
        .unwrap();
    let mut fixture = acquired.fixture.clone();
    fixture.cells.iter_mut().for_each(|cells| *cells *= 2);
    let forward = Request {
        schema: "ftx-strip-experiment/1".into(),
        material: selection,
        fixture,
        extensions: acquired
            .observations
            .iter()
            .map(|observation| observation.extension_fraction)
            .collect(),
        controls: input.controls.clone(),
    };
    write(
        &dir,
        "interval-replay.json",
        &serde_json::to_vec(&forward).unwrap(),
    );
    let forward_core = strip::evaluate(&forward).unwrap();
    for (observation, state) in acquired.observations.iter().zip(&forward_core.states) {
        let prediction = core
            .predictions
            .iter()
            .find(|prediction| {
                prediction.experiment == acquired.id && prediction.observation == observation.id
            })
            .unwrap();
        assert!((state.force_n - prediction.state.force_n).abs() < 1e-7);
        assert!((state.mid_width_m - prediction.state.mid_width_m).abs() < 1e-9);
    }
    for unified in [false, true] {
        let report = success(
            &run(
                &dir,
                unified,
                &["calibrate-strip", "intervals.json", "--allow-estimates"],
            ),
            unified,
        );
        assert_eq!(report["calibration"], serde_json::to_value(&core).unwrap());
        assert_eq!(report["selection_json"], raw_selection);
        assert_eq!(report["simulation_ready"], false);
        assert_eq!(report["fit_claim"], false);
        let replay = success(
            &run(&dir, unified, &["strip-experiment", "interval-replay.json"]),
            unified,
        );
        assert_eq!(replay, serde_json::to_value(&forward_core).unwrap());
    }
    assert_eq!(
        fs::read(dir.join("intervals.json")).unwrap(),
        INDEPENDENT_INTERVALS.as_bytes()
    );
}

#[test]
fn both_strip_boundaries_refuse_unknown_fields_flags_unapproved_estimates_and_oversized_files() {
    let dir = workspace();
    let mut input = forward();
    input.material.allow_estimates = false;
    write(&dir, "denied.json", &serde_json::to_vec(&input).unwrap());
    let mut unknown = serde_json::to_value(forward()).unwrap();
    unknown["assume_homogeneous_strain"] = json!(true);
    write(&dir, "unknown.json", &serde_json::to_vec(&unknown).unwrap());
    write(&dir, "invalid.json", b"{}");
    write(&dir, "invalid-utf8.json", &[0xff]);
    let huge = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join("huge.json"))
        .unwrap();
    huge.set_len(64 * 1024 * 1024 + 1).unwrap();
    for unified in [false, true] {
        for name in ["denied.json", "unknown.json"] {
            refused(
                &run(&dir, unified, &["strip-experiment", name]),
                unified,
                "invalid_document",
            );
        }
        for action in ["strip-acquisition", "strip-experiment", "calibrate-strip"] {
            refused(
                &run(&dir, unified, &[action, "invalid.json"]),
                unified,
                "invalid_document",
            );
            refused(
                &run(&dir, unified, &[action, "invalid-utf8.json"]),
                unified,
                "invalid_document",
            );
            refused(
                &run(&dir, unified, &[action, "invalid.json", "--allow-anything"]),
                unified,
                "invalid_document",
            );
            refused(
                &run(&dir, unified, &[action, "invalid.json", "extra.json"]),
                unified,
                "invalid_document",
            );
            refused(
                &run(&dir, unified, &[action, "huge.json"]),
                unified,
                "budget_exceeded",
            );
        }
    }
    for binary in [
        env!("CARGO_BIN_EXE_ftx"),
        env!("CARGO_BIN_EXE_ftx-materials"),
    ] {
        let help = Command::new(binary).arg("--help").output().unwrap();
        assert!(help.status.success());
        let help = String::from_utf8(help.stdout).unwrap();
        for action in ["strip-acquisition", "strip-experiment", "calibrate-strip"] {
            assert!(help.contains(action));
        }
    }
}
