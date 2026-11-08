//! Both native binding commands use actual source-qualified material/contact authority.
//! Geometry/sliding forces are synthetic; the MIT source rows do not establish measured garment fit.

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

fn inspect(dir: &Path, unified: bool, path: &str) -> Output {
    let mut command = Command::new(if unified {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(dir);
    if unified {
        command.args(["--json", "materials"]);
    }
    command.args(["inspect", path]).output().unwrap()
}

#[test]
fn both_native_inspect_commands_preserve_custom_sources_and_match_portable_capabilities() {
    use ftx_pattern::material_catalog::Selection;

    let catalog = textile_import::catalog().unwrap();
    let original = catalog
        .resolve(&Selection {
            id: "fsri/hfm/cotton-sheet/dry/220330-r1".into(),
            ..Selection::default()
        })
        .unwrap();
    let mut record = original.authored().clone();
    record.id = "custom-retained-hfm-run".into();
    let dir = workspace();
    for consent in [false, true] {
        let selection = Selection {
            id: record.id.clone(),
            record: Some(record.clone()),
            allow_estimates: consent,
            ..Selection::default()
        };
        let input = serde_json::to_string(&selection).unwrap();
        let name = format!("custom {consent}.json");
        write(&dir, &name, input.as_bytes());
        let portable: Value =
            serde_json::from_str(&ftx_wasm::materials::inspect(&input).unwrap()).unwrap();
        assert_eq!(portable["selection"]["record"], json!(record));
        assert_eq!(portable["selection"]["allow_estimates"], consent);
        assert_eq!(
            portable["capabilities"]["steady_conduction"]["ready"],
            consent
        );
        for capability in [
            "mechanical_coefficients",
            "transient_thermal",
            "sewing_thickness_m",
        ] {
            assert_eq!(portable["capabilities"][capability]["ready"], false);
        }
        assert_eq!(portable["missing_mechanics"].as_array().unwrap().len(), 8);
        let frozen: Selection =
            serde_json::from_str(portable["selection_json"].as_str().unwrap()).unwrap();
        assert_eq!(
            catalog.resolve(&frozen).unwrap().sha256(),
            portable["snapshot"]["sha256"].as_str().unwrap()
        );
        for unified in [false, true] {
            assert_eq!(success(&inspect(&dir, unified, &name), unified), portable);
        }
        assert_eq!(fs::read(dir.join(&name)).unwrap(), input.as_bytes());
    }

    let input = r#"{"id":"mit.raw-2013.08a"}"#;
    write(&dir, "catalog-id.json", input.as_bytes());
    let portable: Value =
        serde_json::from_str(&ftx_wasm::materials::inspect(input).unwrap()).unwrap();
    for unified in [false, true] {
        assert_eq!(
            success(&inspect(&dir, unified, "catalog-id.json"), unified),
            portable
        );
    }
}

#[test]
fn native_inspection_refuses_stale_locks_and_invalid_records_without_rewriting_inputs() {
    let (pattern, _) = inputs();
    let selection = pattern["fabric"]["material"].clone();
    let mut stale = selection.clone();
    stale["expected_sha256"] = json!("0".repeat(64));
    let mut bad_source = selection;
    bad_source["record"]["properties"]["areal_density"]["source"] = json!("absent-source");
    let dir = workspace();
    for (i, input) in [stale.to_string(), bad_source.to_string(), "{".into()]
        .iter()
        .enumerate()
    {
        let name = format!("invalid-{i}.json");
        write(&dir, &name, input.as_bytes());
        let expected = ftx_wasm::materials::inspect(input).unwrap_err();
        for unified in [false, true] {
            let actual = refused(&inspect(&dir, unified, &name), unified, "invalid_document");
            if i < 2 {
                assert_eq!(actual, json!(expected));
            }
        }
        assert_eq!(fs::read(dir.join(name)).unwrap(), input.as_bytes());
    }
}

#[test]
fn inspection_stock_width_agrees_with_actual_cutting_without_rewriting_observations() {
    let source: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let options = serde_json::from_value(json!({"copies":1,"gap_mm":2.0,
        "edge_margin_mm":0.0,"end_margin_mm":0.0,"seam_allowance_mm":0.0}))
    .unwrap();
    let dir = workspace();
    for (i, (width, unit, consent, ready)) in [
        (-1.0, "m", true, false),
        (0.0, "m", true, false),
        (-0.0, "m", true, false),
        (1600.0, "mm", true, true),
        (1.6, "m", false, false),
    ]
    .into_iter()
    .enumerate()
    {
        let mut pattern = source.clone();
        let selected = &mut pattern["fabric"]["material"];
        selected["allow_estimates"] = json!(consent);
        selected["record"]["properties"]["width"] = json!({"value":width,"unit":unit,
            "source":"test","evidence":"estimate","method":"Synthetic stock-width admission test"});
        let authored: ftx_pattern::materials::Material =
            serde_json::from_value(selected["record"].clone()).unwrap();
        let input = selected.to_string();
        let name = format!("width-{i}.json");
        write(&dir, &name, input.as_bytes());
        let portable: Value =
            serde_json::from_str(&ftx_wasm::materials::inspect(&input).unwrap()).unwrap();
        assert_eq!(portable["selection"]["record"], json!(authored));
        assert_eq!(portable["selection"]["allow_estimates"], consent);
        assert_eq!(portable["capabilities"]["stock_width_m"]["ready"], ready);
        assert_eq!(
            portable["capabilities"]["sewing_thickness_m"]["ready"],
            consent
        );
        if ready {
            assert_eq!(portable["capabilities"]["stock_width_m"]["value"], 1.6);
        } else {
            assert!(
                portable["capabilities"]["stock_width_m"]
                    .get("value")
                    .is_none()
            );
            assert_eq!(
                portable["capabilities"]["stock_width_m"]["refusal"]["code"],
                "invalid_document"
            );
        }
        let cut = ftx_export::cutting::CutPlan::build(&Pattern::new(pattern), &options);
        assert_eq!(cut.is_ok(), ready);
        if consent && !ready {
            assert!(cut.unwrap_err().message.contains("stock width"));
        }
        for unified in [false, true] {
            assert_eq!(success(&inspect(&dir, unified, &name), unified), portable);
        }
        assert_eq!(fs::read(dir.join(name)).unwrap(), input.as_bytes());
    }
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
fn inactive_partial_sheets_and_unused_defaults_keep_active_drape_and_cut_inventory_independent() {
    let (mut pattern, _) = inputs();
    // Both simulated panels have explicit, characterized selections. The same authored garment
    // also contains a real cut piece deliberately omitted from cloth simulation.
    pattern["pieces"][0]["fabric"] = json!({"material":pattern["fabric"]["material"]});
    let mut label = pattern["pieces"][0].clone();
    label["id"] = json!("label");
    label["sim"] = json!(false);
    pattern["pieces"].as_array_mut().unwrap().push(label);
    let changes = json!({"schema":"ftx-material-bindings/1",
        "default":{"id":"mit.raw-2013.08a"},
        "pieces":{"label":{"id":"mit.raw-2013.08a"}}});
    let pattern_bytes = serde_json::to_vec_pretty(&pattern).unwrap();
    let changes_bytes = serde_json::to_vec_pretty(&changes).unwrap();
    let portable: Value = serde_json::from_str(
        &ftx_wasm::materials::bind(
            std::str::from_utf8(&pattern_bytes).unwrap(),
            std::str::from_utf8(&changes_bytes).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    for field in [
        "drape_materials_without_self_contact",
        "drape_materials_with_self_contact",
    ] {
        assert_eq!(portable[field]["ready"], true, "{portable}");
    }
    let bound = Pattern::new(portable["pattern"].clone());
    assert_eq!(bound.raw()["seams"], pattern["seams"]);
    for i in 0..3 {
        assert_eq!(
            bound.raw()["pieces"][i]["vertices"],
            pattern["pieces"][i]["vertices"]
        );
        assert_eq!(
            bound.raw()["pieces"][i]["edges"],
            pattern["pieces"][i]["edges"]
        );
    }
    let frozen: ftx_pattern::material_catalog::Selection =
        serde_json::from_value(bound.raw()["pieces"][2]["fabric"]["material"].clone()).unwrap();
    let linen = textile_import::catalog().unwrap().resolve(&frozen).unwrap();
    assert!(!linen.material().properties.contains_key("warp_stiffness"));
    assert!(!linen.material().properties.contains_key("friction_self"));
    // sim:false is a drape exclusion, never permission to omit a piece from the physical BOM.
    let inventory = ftx_export::assembly_mass::Request {
        schema: "ftx-assembly-mass/1".into(),
        inventory_complete: true,
        extras: Vec::new(),
        seam_allowance_mm: Some(0.0),
    }
    .evaluate(&bound)
    .unwrap();
    assert_eq!(inventory.pieces.len(), 3);
    let label = inventory
        .pieces
        .iter()
        .find(|p| p.piece == "label")
        .unwrap();
    assert!(
        (label.mass.kg.unwrap() - 0.002 * linen.material().properties["areal_density"].value).abs()
            < 1e-15
    );
    assert!(inventory.assembly_mass.kg.is_some());

    let dir = workspace();
    write(&dir, "pattern.json", &pattern_bytes);
    write(&dir, "changes.json", &changes_bytes);
    for unified in [false, true] {
        let out = format!("partial-inactive-{unified}.json");
        let result = success(&run(&dir, unified, &out), unified);
        for (field, expected) in portable.as_object().unwrap() {
            assert_eq!(&result[field], expected, "native/portable field {field}");
        }
        assert_eq!(
            fs::read(dir.join(&out)).unwrap(),
            portable["pattern_json"].as_str().unwrap().as_bytes()
        );
    }
    assert_eq!(fs::read(dir.join("pattern.json")).unwrap(), pattern_bytes);
    assert_eq!(fs::read(dir.join("changes.json")).unwrap(), changes_bytes);

    // The same incomplete observation becomes a real blocker when the piece is simulated.
    pattern["pieces"][2]["sim"] = json!(true);
    let active: Value = serde_json::from_str(
        &ftx_wasm::materials::bind(&pattern.to_string(), &changes.to_string()).unwrap(),
    )
    .unwrap();
    for field in [
        "drape_materials_without_self_contact",
        "drape_materials_with_self_contact",
    ] {
        assert_eq!(active[field]["ready"], false);
        assert!(active[field].get("value").is_none());
    }
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

#[test]
fn raw_linen_rows_bind_with_mass_and_thickness_without_inventing_drape_readiness() {
    let pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
    ))
    .unwrap();
    let dir = workspace();
    write(&dir, "pattern.json", &serde_json::to_vec(&pattern).unwrap());
    let changes = json!({"schema":"ftx-material-bindings/1","default":{"id":"mit.raw-2013.08a"}});
    write(&dir, "changes.json", &serde_json::to_vec(&changes).unwrap());
    let expected = serde_json::from_value::<Changes>(changes)
        .unwrap()
        .apply(&Pattern::new(pattern.clone()))
        .unwrap();
    let catalog = textile_import::catalog().unwrap();
    let mut hashes = Vec::new();
    for unified in [false, true] {
        let result = success(
            &run(&dir, unified, &format!("linen-{unified}.json")),
            unified,
        );
        for (field, value) in expected.as_object().unwrap() {
            assert_eq!(&result[field], value);
        }
        let frozen =
            serde_json::from_value(result["pattern"]["fabric"]["material"].clone()).unwrap();
        let material = catalog.resolve(&frozen).unwrap();
        hashes.push(material.sha256().to_string());
        assert!(
            (material
                .material()
                .require("reported_thickness", "m", false)
                .unwrap()
                - 0.000_215_9)
                .abs()
                < 1e-18
        );
        assert!(
            material.authored().sources["workbook"]
                .note
                .contains("94f71fbd0dbd67b108f0472f73a5b1c048b2917d3403f7863c981faaa84575c1")
        );
        let bound = Pattern::new(result["pattern"].clone());
        assert_eq!(bound.raw()["seams"], pattern["seams"]);
        // Real existing assembly accounting consumes mass without demanding nonexistent mechanics.
        let inventory: ftx_export::assembly_mass::Request = serde_json::from_value(json!({
            "schema":"ftx-assembly-mass/1", "inventory_complete":true, "extras":[], "seam_allowance_mm":0.0
        })).unwrap();
        let mass = inventory.evaluate(&bound).unwrap();
        assert!(
            (mass.assembly_mass.kg.unwrap()
                - 0.004 * material.material().properties["areal_density"].value)
                .abs()
                < 1e-15
        );
        assert!(mass.assembly_mass.reported_bounds_kg.is_none());
        assert!(!mass.physical_validation_claim);
        let sewing: ftx_export::sewing::SewOptions = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-sewing-options.json"
        ))
        .unwrap();
        assert!(ftx_export::sewing::SewPlan::build(&bound, &sewing).is_err());
        for field in [
            "drape_materials_without_self_contact",
            "drape_materials_with_self_contact",
        ] {
            assert_eq!(result[field]["ready"], false);
            assert!(result[field].get("value").is_none());
        }
    }
    assert_eq!(hashes[0], hashes[1]);
}
