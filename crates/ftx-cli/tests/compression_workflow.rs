//! Actual native conditioning, frozen-selection binding and drape resolution.
//! All pressures and material observations are explicitly synthetic software fixtures.

use ftx_pattern::{
    material_catalog::Selection,
    material_compression::{Input, condition},
    materials::{CATALOG_SCHEMA, Catalog},
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const FIXTURE: &str = include_str!("../../../tests/fixtures/materials/synthetic-compression.json");

fn workspace() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ftx-compression-cli-{}-{}",
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

fn success(out: &Output, unified: bool) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    if unified {
        assert_eq!(value["schema"], "ftx-cli/1");
        assert_eq!(value["ok"], true);
        assert_eq!(value["exit"], 0);
        value["result"].clone()
    } else {
        value
    }
}

fn refused(out: &Output, unified: bool, code: &str) {
    assert_eq!(out.status.code(), Some(if unified { 3 } else { 2 }));
    let value: Value = if unified {
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["schema"], "ftx-cli/1");
        assert_eq!(value["ok"], false);
        assert_eq!(value["exit"], 3);
        value
    } else {
        assert!(out.stdout.is_empty());
        serde_json::from_slice(&out.stderr).unwrap()
    };
    assert_eq!(value["refusal"]["code"], code);
    for absent in ["result", "selection", "selection_json", "material"] {
        assert!(value.get(absent).is_none());
    }
}

#[test]
fn both_pressure_commands_match_core_and_replay_raw_locked_selections_offline() {
    let dir = workspace();
    let empty = Catalog {
        schema: CATALOG_SCHEMA.into(),
        materials: Vec::new(),
    };
    for (index, pressure_pa, thickness_m) in [(0, 250.0, 0.015), (1, 500.0, 0.010)] {
        let mut input: Input = serde_json::from_str(FIXTURE).unwrap();
        input.pressure_pa = pressure_pa;
        let report = condition(&input).unwrap();
        let expected = serde_json::to_value(&report).unwrap();
        let name = format!("pressure-{index}.json");
        let bytes = serde_json::to_vec(&input).unwrap();
        write(&dir, &name, &bytes);
        for unified in [false, true] {
            let out = run(&dir, unified, &["pressure-condition", &name]);
            let result = success(&out, unified);
            assert_eq!(result, expected);
            assert_eq!(result["interpolated"], pressure_pa == 250.0);
            let raw = result["selection_json"].as_str().unwrap();
            assert_eq!(raw, report.selection_json);
            let selection: Selection = serde_json::from_str(raw).unwrap();
            let resolved = empty.resolve(&selection).unwrap();
            assert_eq!(
                Some(resolved.sha256()),
                report.selection.expected_sha256.as_deref()
            );
            assert!((resolved.mechanics().unwrap().thickness_m - thickness_m).abs() < 1e-15);
            assert!((resolved.mechanics().unwrap().areal_kg_m2 - 0.1).abs() < 1e-15);
        }
        assert_eq!(fs::read(dir.join(&name)).unwrap(), bytes);
    }
}

#[test]
fn raw_conditioned_selection_binds_without_geometry_edits_and_updates_real_contact_offsets() {
    let dir = workspace();
    let pattern = json!({"schema":"fabric-pattern/1","units":"mm","seams":[],"fabric":{},
        "pieces":[{"id":"coupon","cut":{"qty":1},"vertices":{"a":[0,0],"b":[100,0],"c":[0,100]},
            "edges":[{"id":"ab","from":"a","to":"b"},{"id":"bc","from":"b","to":"c"},{"id":"ca","from":"c","to":"a"}],"grain":[[0,0],[0,100]]}]});
    write(&dir, "pattern.json", &serde_json::to_vec(&pattern).unwrap());
    let mut material_hashes = Vec::new();
    for (index, pressure_pa, thickness_m) in [(0, 0.0, 0.020), (1, 500.0, 0.010)] {
        let mut input: Input = serde_json::from_str(FIXTURE).unwrap();
        input.pressure_pa = pressure_pa;
        let request = format!("request-{index}.json");
        let selection = format!("selection-{index}.json");
        let bound = format!("bound-{index}.json");
        write(&dir, &request, &serde_json::to_vec(&input).unwrap());
        let report = success(&run(&dir, false, &["pressure-condition", &request]), false);
        write(
            &dir,
            &selection,
            report["selection_json"].as_str().unwrap().as_bytes(),
        );
        let binding = success(
            &run(&dir, false, &["bind", "pattern.json", &selection, &bound]),
            false,
        );
        assert_eq!(binding["geometry_changed"], false);
        assert_eq!(binding["simulation_ready"], true);
        assert_eq!(
            binding["material_sha256"],
            report["selection"]["expected_sha256"]
        );
        let saved: Value = serde_json::from_slice(&fs::read(dir.join(&bound)).unwrap()).unwrap();
        assert_eq!(saved["pieces"], pattern["pieces"]);
        assert_eq!(saved["seams"], pattern["seams"]);
        let resolved = success(&run(&dir, false, &["resolve", &bound]), false);
        assert_eq!(resolved["record_sha256"], binding["material_sha256"]);
        assert!(
            (resolved["mechanics"]["thickness_m"].as_f64().unwrap() - thickness_m).abs() < 1e-15
        );
        assert!((resolved["body_offset_m"].as_f64().unwrap() - thickness_m / 2.0).abs() < 1e-15);
        assert_eq!(resolved["mechanics"]["warp_n_m"], 8000.0);
        assert_eq!(resolved["mechanics"]["weft_n_m"], 4000.0);
        assert!((resolved["mechanics"]["areal_kg_m2"].as_f64().unwrap() - 0.1).abs() < 1e-15);
        material_hashes.push(binding["material_sha256"].clone());
    }
    assert_ne!(material_hashes[0], material_hashes[1]);
}

#[test]
fn both_commands_refuse_unknown_out_of_range_and_unapproved_requests_without_success() {
    let dir = workspace();
    for (index, key, value, code) in [
        (0, "force_n", json!(9.5), "invalid_document"),
        (1, "pressure_pa", json!(501.0), "out_of_range"),
        (2, "allow_estimates", json!(false), "invalid_document"),
    ] {
        let mut input: Value = serde_json::from_str(FIXTURE).unwrap();
        input[key] = value;
        let name = format!("invalid-{index}.json");
        let bytes = serde_json::to_vec(&input).unwrap();
        write(&dir, &name, &bytes);
        for unified in [false, true] {
            refused(
                &run(&dir, unified, &["pressure-condition", &name]),
                unified,
                code,
            );
        }
        assert_eq!(fs::read(dir.join(&name)).unwrap(), bytes);
    }
    write(&dir, "valid.json", FIXTURE.as_bytes());
    for unified in [false, true] {
        refused(
            &run(
                &dir,
                unified,
                &["pressure-condition", "valid.json", "--allow-anything"],
            ),
            unified,
            "invalid_document",
        );
    }
}

fn stack_request() -> ftx_pattern::material_compression::stack::Input {
    use ftx_pattern::material_compression::stack::{Direction, Input as Stack, Layer, SCHEMA};
    let mut preparation: Input = serde_json::from_str(FIXTURE).unwrap();
    preparation.pressure_pa = 0.0;
    Stack {
        schema: SCHEMA.into(),
        gap_m: 0.030,
        reference_area_m2: 0.25,
        direction: Direction::Loading,
        layers: ["outer", "inner"]
            .map(|id| Layer {
                id: id.into(),
                preparation: preparation.clone(),
            })
            .to_vec(),
        allow_estimates: true,
    }
}

#[test]
fn both_stack_commands_match_core_and_each_selection_binds_to_actual_contact() {
    let dir = workspace();
    let input = stack_request();
    let original = serde_json::to_vec(&input).unwrap();
    write(&dir, "stack.json", &original);
    let core = ftx_pattern::material_compression::stack::close(&input).unwrap();
    for unified in [false, true] {
        let report = success(
            &run(&dir, unified, &["compression-stack", "stack.json"]),
            unified,
        );
        assert_eq!(report, serde_json::to_value(&core).unwrap());
        assert!((report["pressure_pa"].as_f64().unwrap() - 250.0).abs() < 1e-10);
    }
    let pattern = json!({"schema":"fabric-pattern/1","units":"mm","seams":[],"fabric":{},
        "pieces":[{"id":"coupon","cut":{"qty":1},"vertices":{"a":[0,0],"b":[100,0],"c":[0,100]},
            "edges":[{"id":"ab","from":"a","to":"b"},{"id":"bc","from":"b","to":"c"},{"id":"ca","from":"c","to":"a"}],"grain":[[0,0],[0,100]]}]});
    write(&dir, "pattern.json", &serde_json::to_vec(&pattern).unwrap());
    for (index, layer) in core.layers.iter().enumerate() {
        let selection = format!("layer-{index}.json");
        let bound = format!("bound-{index}.json");
        write(
            &dir,
            &selection,
            layer.conditioned.selection_json.as_bytes(),
        );
        let binding = success(
            &run(&dir, false, &["bind", "pattern.json", &selection, &bound]),
            false,
        );
        assert_eq!(binding["geometry_changed"], false);
        let actual = success(&run(&dir, false, &["resolve", &bound]), false);
        assert!((actual["body_offset_m"].as_f64().unwrap() - 0.0075).abs() < 1e-15);
        assert!((actual["mechanics"]["areal_kg_m2"].as_f64().unwrap() - 0.1).abs() < 1e-15);
        assert_eq!(
            actual["record_sha256"],
            layer.conditioned.selection.expected_sha256.clone().unwrap()
        );
        assert!(
            !run(
                &dir,
                false,
                &["bind", "pattern.json", &selection, "stack.json"]
            )
            .status
            .success()
        );
        assert!(
            !run(&dir, false, &["bind", "pattern.json", &selection, &bound])
                .status
                .success()
        );
    }
    assert_eq!(fs::read(dir.join("stack.json")).unwrap(), original);
}

#[test]
fn stack_command_refusals_never_publish_partial_layer_selections() {
    let dir = workspace();
    for (index, key, value, code) in [
        (0, "gap_m", json!(0.050), "out_of_range"),
        (1, "allow_estimates", json!(false), "invalid_document"),
        (2, "initial_force_n", json!(5), "invalid_document"),
    ] {
        let mut request = serde_json::to_value(stack_request()).unwrap();
        request[key] = value;
        let name = format!("refused-{index}.json");
        write(&dir, &name, &serde_json::to_vec(&request).unwrap());
        for unified in [false, true] {
            refused(
                &run(&dir, unified, &["compression-stack", &name]),
                unified,
                code,
            );
        }
    }
    write(
        &dir,
        "stack.json",
        &serde_json::to_vec(&stack_request()).unwrap(),
    );
    for unified in [false, true] {
        refused(
            &run(
                &dir,
                unified,
                &["compression-stack", "stack.json", "--force"],
            ),
            unified,
            "invalid_document",
        );
    }
}
