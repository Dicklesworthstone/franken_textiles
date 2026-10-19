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
