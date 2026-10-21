//! Both native binding commands use actual source-qualified material/contact authority.
//! These specimens and sliding forces are synthetic; no measured garment claim is made.

use ftx_drape::{material, run::DrapeOptions};
use ftx_pattern::{
    Pattern,
    material_binding::Changes,
    material_friction::{Binding, Input, Target, calibrate},
    textile_import,
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn inputs() -> (Value, Value) {
    let mut acquisition: Input = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-friction-sliding.json"
    ))
    .unwrap();
    let Target::Pair { materials } = acquisition.target.clone() else {
        panic!("fixture must identify a material pair")
    };
    let db = textile_import::catalog().unwrap();
    let pair = |input: &Input| {
        let Binding::Interface { interface, .. } =
            calibrate(input, &db).unwrap().binding(&db, true).unwrap()
        else {
            panic!("pair calibration must retain interface evidence")
        };
        serde_json::to_value(interface).unwrap()
    };
    let original_pair = pair(&acquisition);
    let mut replacement = materials[0].clone();
    replacement.id = "synthetic-replacement-shell".into();
    let record = replacement.record.as_mut().unwrap();
    record.id.clone_from(&replacement.id);
    record.name = "Synthetic replacement shell, not an empirical textile".into();
    record.properties.get_mut("areal_density").unwrap().value = 0.3;
    acquisition.target = Target::Pair {
        materials: Box::new([replacement.clone(), materials[1].clone()]),
    };
    let replacement_pair = pair(&acquisition);
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    pattern["fabric"]["material"] = json!(materials[0]);
    pattern["fabric"]["interfaces"] = json!([original_pair]);
    pattern["pieces"][1]["fabric"] = json!({"material":materials[1],"role":"lining"});
    let changes = json!({"schema":"ftx-material-bindings/1","default":replacement,
        "interfaces":[replacement_pair]});
    (pattern, changes)
}

fn workspace() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ftx-material-binding-cli-{}-{}",
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

fn run(dir: &Path, unified: bool, out: &str) -> Output {
    let mut command = Command::new(if unified {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(dir);
    if unified {
        command.arg("materials");
    }
    command.args(["bind-changes", "pattern.json", "changes.json", out]);
    if unified {
        command.arg("--json");
    }
    command.output().unwrap()
}

fn success(out: &Output, unified: bool) -> Value {
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
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

fn refused(out: &Output, unified: bool, code: &str) -> Value {
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
    assert!(value.get("result").is_none());
    value["refusal"].clone()
}

#[test]
fn both_binaries_replace_heterogeneous_materials_and_pairs_with_exact_core_and_file_replay() {
    let (pattern, changes) = inputs();
    let source = Pattern::new(pattern.clone());
    let changes: Changes = serde_json::from_value(changes).unwrap();
    let core = changes.apply(&source).unwrap();
    let bound = Pattern::new(core["pattern"].clone());
    let dir = workspace();
    let pattern_bytes = serde_json::to_vec_pretty(&pattern).unwrap();
    let changes_bytes = serde_json::to_vec_pretty(&changes).unwrap();
    write(&dir, "pattern.json", &pattern_bytes);
    write(&dir, "changes.json", &changes_bytes);
    for unified in [false, true] {
        let out = format!("bound {unified}.json");
        let result = success(&run(&dir, unified, &out), unified);
        for (field, expected) in core.as_object().unwrap() {
            assert_eq!(&result[field], expected, "core field {field}");
        }
        assert_eq!(result["out"], out);
        let bytes = fs::read(dir.join(&out)).unwrap();
        assert_eq!(bytes, core["pattern_json"].as_str().unwrap().as_bytes());
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            core["pattern"]
        );
        for contact in [false, true] {
            let base = DrapeOptions::default();
            let options = DrapeOptions {
                self_contact: contact.then_some(base.contact),
                ..base
            };
            let actual = material::resolve(&bound, &options).unwrap();
            let field = if contact {
                "drape_materials_with_self_contact"
            } else {
                "drape_materials_without_self_contact"
            };
            assert_eq!(result[field]["ready"], true);
            assert_eq!(result[field]["value"], actual.report);
        }
        assert_eq!(
            result["pieces"][0]["material_id"],
            "synthetic-replacement-shell"
        );
        assert_eq!(
            result["pieces"][1]["material_id"],
            pattern["pieces"][1]["fabric"]["material"]["id"]
        );
        assert_eq!(result["pattern"]["seams"], pattern["seams"]);
        assert_eq!(
            result["pattern"]["fabric"]["interfaces"],
            json!(changes.interfaces)
        );
        assert_ne!(
            result["pattern"]["fabric"]["interfaces"],
            pattern["fabric"]["interfaces"]
        );
        let mut frozen = changes.clone();
        frozen.default =
            Some(serde_json::from_value(result["pattern"]["fabric"]["material"].clone()).unwrap());
        frozen.pieces.insert(
            "back".into(),
            serde_json::from_value(result["pattern"]["pieces"][1]["fabric"]["material"].clone())
                .unwrap(),
        );
        assert_eq!(frozen.apply(&source).unwrap(), core);
    }
    assert_eq!(fs::read(dir.join("pattern.json")).unwrap(), pattern_bytes);
    assert_eq!(fs::read(dir.join("changes.json")).unwrap(), changes_bytes);
}

#[test]
fn stale_explicit_pairs_refuse_before_creating_output_in_either_binary() {
    let (pattern, mut changes) = inputs();
    changes["interfaces"] = pattern["fabric"]["interfaces"].clone();
    let dir = workspace();
    let pattern_bytes = serde_json::to_vec(&pattern).unwrap();
    let changes_bytes = serde_json::to_vec(&changes).unwrap();
    write(&dir, "pattern.json", &pattern_bytes);
    write(&dir, "changes.json", &changes_bytes);
    for unified in [false, true] {
        let out = format!("stale-{unified}.json");
        let refusal = refused(&run(&dir, unified, &out), unified, "invalid_document");
        assert!(
            refusal["message"]
                .as_str()
                .unwrap()
                .contains("unassigned or stale")
        );
        assert!(!dir.join(out).exists());
    }
    assert_eq!(fs::read(dir.join("pattern.json")).unwrap(), pattern_bytes);
    assert_eq!(fs::read(dir.join("changes.json")).unwrap(), changes_bytes);
}

#[test]
fn existing_outputs_and_source_paths_are_preserved_by_both_commands() {
    let (pattern, changes) = inputs();
    let dir = workspace();
    let pattern_bytes = serde_json::to_vec(&pattern).unwrap();
    let changes_bytes = serde_json::to_vec(&changes).unwrap();
    write(&dir, "pattern.json", &pattern_bytes);
    write(&dir, "changes.json", &changes_bytes);
    let previous = b"Previous user output must remain intact.\n";
    write(&dir, "existing.json", previous);
    for unified in [false, true] {
        for out in ["existing.json", "pattern.json", "changes.json"] {
            refused(&run(&dir, unified, out), unified, "io");
        }
    }
    assert_eq!(fs::read(dir.join("existing.json")).unwrap(), previous);
    assert_eq!(fs::read(dir.join("pattern.json")).unwrap(), pattern_bytes);
    assert_eq!(fs::read(dir.join("changes.json")).unwrap(), changes_bytes);
}

#[test]
fn unadmitted_pair_estimates_keep_binding_available_and_mechanical_readiness_false() {
    let (pattern, mut changes) = inputs();
    changes["interfaces"][0]["allow_estimates"] = json!(false);
    let core = serde_json::from_value::<Changes>(changes.clone())
        .unwrap()
        .apply(&Pattern::new(pattern.clone()))
        .unwrap();
    let dir = workspace();
    let pattern_bytes = serde_json::to_vec(&pattern).unwrap();
    let changes_bytes = serde_json::to_vec(&changes).unwrap();
    write(&dir, "pattern.json", &pattern_bytes);
    write(&dir, "changes.json", &changes_bytes);
    for unified in [false, true] {
        let out = format!("unadmitted-{unified}.json");
        let result = success(&run(&dir, unified, &out), unified);
        assert_eq!(
            fs::read(dir.join(&out)).unwrap(),
            core["pattern_json"].as_str().unwrap().as_bytes()
        );
        assert_eq!(
            result["pattern"]["fabric"]["interfaces"],
            changes["interfaces"]
        );
        for field in [
            "drape_materials_without_self_contact",
            "drape_materials_with_self_contact",
        ] {
            assert_eq!(result[field]["ready"], false);
            assert!(
                result[field]["refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("own explicit consent")
            );
            assert!(result[field].get("value").is_none());
        }
    }
    assert_eq!(fs::read(dir.join("pattern.json")).unwrap(), pattern_bytes);
    assert_eq!(fs::read(dir.join("changes.json")).unwrap(), changes_bytes);
}
