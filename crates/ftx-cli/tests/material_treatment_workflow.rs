//! Both real native commands apply measured treatment observations to ordinary pattern files.
//! Panel dimensions are synthetic; no empirical garment or machine-qualification claim is made.

use ftx_core::receipt::sha256_hex;
use ftx_export::assembly_mass;
use ftx_pattern::{
    Pattern,
    material_treatment::{self, geometry},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

const DENIM: &str =
    include_str!("../../../tests/fixtures/materials/kanat-ozdil-2026-denim-1-treatment.json");
static NEXT: AtomicU64 = AtomicU64::new(0);

fn workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ftx-treatment-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let treatment: Value = serde_json::from_str(DENIM).unwrap();
    let pattern = json!({"schema":"fabric-pattern/1","units":"mm",
        "fabric":{"material":treatment["before"]},"pieces":[{"id":"panel","sa":0.,
            "vertices":{"a":[0.,0.],"b":[100.,0.],"c":[100.,200.],"d":[0.,200.]},
            "edges":[{"id":"bottom","from":"a","to":"b"},
                {"id":"right","from":"b","to":"c"},
                {"id":"top","from":"c","to":"d"},
                {"id":"left","from":"d","to":"a"}],
            "grain":[[0.,0.],[0.,100.]]}],"seams":[]});
    let request = json!({"schema":geometry::SCHEMA,"mode":"treat",
        "transitions":{"wash":treatment},"pieces":{"panel":"wash"}});
    fs::write(dir.join("treatment.json"), DENIM).unwrap();
    fs::write(dir.join("pattern.json"), pattern.to_string()).unwrap();
    fs::write(dir.join("pattern-treatment.json"), request.to_string()).unwrap();
    dir
}

fn invoke(main: bool, dir: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(dir);
    if main {
        command.args(["--json", "materials"]);
    }
    command.args(args).output().unwrap()
}

fn result(main: bool, output: Output) -> Value {
    assert!(
        output.status.success(),
        "{} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    if main { value["result"].clone() } else { value }
}

fn refusal(main: bool, output: Output) -> Value {
    // The primary JSON command uses exit 3 for these refusals; the companion uses exit 2.
    assert_eq!(output.status.code(), Some(if main { 3 } else { 2 }));
    let value: Value =
        serde_json::from_slice(if main { &output.stdout } else { &output.stderr }).unwrap();
    assert!(value["refusal"].is_object(), "{value}");
    value["refusal"].clone()
}

#[test]
fn both_native_commands_keep_the_exact_frozen_material_report() {
    let dir = workspace();
    let input: material_treatment::Input = serde_json::from_str(DENIM).unwrap();
    let expected = serde_json::to_value(material_treatment::evaluate(&input).unwrap()).unwrap();
    for main in [true, false] {
        let report = result(main, invoke(main, &dir, &["treatment", "treatment.json"]));
        assert_eq!(report, expected);
        assert_eq!(
            report["input_sha256"],
            sha256_hex(&serde_json::to_vec(&input).unwrap())
        );
        for state in ["before", "after"] {
            let selection: Value =
                serde_json::from_str(report[format!("{state}_selection_json")].as_str().unwrap())
                    .unwrap();
            assert_eq!(selection, report[state]);
            assert_eq!(
                selection["expected_sha256"],
                report[format!("{state}_sha256")]
            );
        }
    }
    assert_eq!(
        fs::read_to_string(dir.join("treatment.json")).unwrap(),
        DENIM
    );
}

#[test]
fn both_commands_write_exact_ordinary_patterns_and_refuse_to_replace_existing_files() {
    let dir = workspace();
    let source = fs::read_to_string(dir.join("pattern.json")).unwrap();
    let request = fs::read_to_string(dir.join("pattern-treatment.json")).unwrap();
    let input: geometry::Input = serde_json::from_str(&request).unwrap();
    let expected = input.apply(&Pattern::from_json(&source).unwrap()).unwrap();
    let mass: assembly_mass::Request = serde_json::from_value(json!({
        "schema":"ftx-assembly-mass/1","inventory_complete":true,"extras":[],
        "seam_allowance_mm":0.0
    }))
    .unwrap();
    for (main, name) in [
        (true, "main-treated.json"),
        (false, "companion-treated.json"),
    ] {
        let args = [
            "treatment-pattern",
            "pattern.json",
            "pattern-treatment.json",
            name,
        ];
        let report = result(main, invoke(main, &dir, &args));
        let mut expected_written = expected.clone();
        expected_written["out"] = json!(name);
        assert_eq!(report, expected_written);
        let bytes = fs::read(dir.join(name)).unwrap();
        assert_eq!(bytes, report["pattern_json"].as_str().unwrap().as_bytes());
        assert_eq!(report["pattern_sha256"], sha256_hex(&bytes));
        assert_eq!(
            report["source_pattern_sha256"],
            sha256_hex(source.as_bytes())
        );
        let pattern = Pattern::from_json(std::str::from_utf8(&bytes).unwrap()).unwrap();
        let inventory = mass.evaluate(&pattern).unwrap();
        let area = 0.02 * (1.0 - 0.0483) * (1.0 - 0.055);
        assert!((inventory.pieces[0].retained_area_m2 - area).abs() < 1e-12);
        assert!((inventory.assembly_mass.kg.unwrap() - area * 0.37010).abs() < 1e-12);
        assert_eq!(
            inventory.pieces[0].material.as_ref().unwrap().sha256(),
            report["transitions"]["wash"]["after_sha256"]
                .as_str()
                .unwrap()
        );
        assert_eq!(refusal(main, invoke(main, &dir, &args))["code"], "io");
        assert_eq!(fs::read(dir.join(name)).unwrap(), bytes);
        assert_eq!(
            refusal(
                main,
                invoke(
                    main,
                    &dir,
                    &[
                        "treatment-pattern",
                        "pattern.json",
                        "pattern-treatment.json",
                        "pattern.json"
                    ]
                )
            )["code"],
            "io"
        );
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        source
    );
    assert_eq!(
        fs::read_to_string(dir.join("pattern-treatment.json")).unwrap(),
        request
    );
}

#[test]
fn both_commands_evaluate_treated_designs_and_replay_exact_intermediate_patterns() {
    let dir = workspace();
    let mut pattern: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("pattern.json")).unwrap()).unwrap();
    pattern["pieces"][0]["cut"] = json!({"qty":1});
    let source = pattern.to_string();
    fs::write(dir.join("pattern.json"), &source).unwrap();
    let treatment: Value = serde_json::from_str(DENIM).unwrap();
    let wash: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("pattern-treatment.json")).unwrap())
            .unwrap();
    let mut compensation = wash.clone();
    compensation["mode"] = json!("compensate");
    let mut request = json!({"schema":"ftx-design-study/1",
        "designs":[
            {"key":"washed","operations":[{"op":"treatment","request":wash}]},
            {"key":"compensated","operations":[{"op":"treatment","request":compensation},
                {"op":"treatment","request":wash}]}],
        "variants":[{"key":"retained","changes":{"schema":"ftx-material-bindings/1"}}],
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,
            "extras":[],"seam_allowance_mm":0.0}});
    let evaluate = |request: &Value| -> Value {
        serde_json::from_str(&ftx_wasm::materials::study(&source, &request.to_string()).unwrap())
            .unwrap()
    };
    let report = evaluate(&request);
    assert_eq!(report["schema"], "ftx-design-study-report/1");
    assert_eq!(report["geometry_changed"], true);
    for row in report["alternatives"].as_array().unwrap() {
        assert_eq!(row["binding"]["status"], "available", "{row}");
        assert_eq!(row["assembly_mass"]["status"], "available", "{row}");
        let key = row["design"]["key"].as_str().unwrap();
        let (area, density, count) = match key {
            "baseline" => (0.02, 0.33993, 0),
            "washed" => (0.02 * 0.9517 * 0.945, 0.37010, 1),
            "compensated" => (0.02, 0.37010, 2),
            _ => panic!("unexpected design {key}"),
        };
        assert!(
            (row["metrics"]["assembly_mass_kg"].as_f64().unwrap() - area * density).abs() < 1e-12
        );
        let stages = row["design"]["treatments"].as_array();
        assert_eq!(stages.map_or(0, Vec::len), count);
        if let Some(stages) = stages {
            for (index, stage) in stages.iter().enumerate() {
                assert_eq!(stage["operation_index"], index);
                let receipt = &stage["report"];
                let replay: Value = serde_json::from_str(
                    &ftx_wasm::materials::treatment_pattern(
                        receipt["source_pattern_json"].as_str().unwrap(),
                        receipt["frozen_request_json"].as_str().unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap();
                assert_eq!(replay, *receipt);
            }
        }
        let rebound: Value = serde_json::from_str(
            &ftx_wasm::materials::bind(
                row["design"]["altered_pattern_json"].as_str().unwrap(),
                &row["frozen_changes"].to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            rebound["pattern_json"],
            row["binding"]["result"]["pattern_json"]
        );
    }
    fs::write(dir.join("study.json"), request.to_string()).unwrap();
    for main in [false, true] {
        assert_eq!(
            result(
                main,
                invoke(main, &dir, &["study", "pattern.json", "study.json"])
            ),
            report
        );
    }
    // A later material variant must not attach the untreated material to the treated geometry.
    request["variants"][0]["changes"]["pieces"] = json!({"panel":treatment["before"]});
    let overwritten = evaluate(&request);
    for row in overwritten["alternatives"].as_array().unwrap() {
        if row["design"]["key"] == "baseline" {
            assert_eq!(row["binding"]["status"], "available");
        } else {
            assert_eq!(row["binding"]["status"], "refused");
            assert!(
                row["binding"]["refusal"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("treatment state")
            );
            assert!(row["metrics"]["assembly_mass_kg"].is_null());
            assert!(!row["design"]["treatments"].as_array().unwrap().is_empty());
        }
    }
    fs::write(dir.join("overwritten.json"), request.to_string()).unwrap();
    for main in [false, true] {
        assert_eq!(
            result(
                main,
                invoke(main, &dir, &["study", "pattern.json", "overwritten.json"])
            ),
            overwritten
        );
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        source
    );
}

#[test]
fn native_refusals_never_publish_patterns_from_unknown_or_stale_requests() {
    for main in [true, false] {
        let dir = workspace();
        let mut wrong: Value = serde_json::from_str(DENIM).unwrap();
        wrong["schema"] = json!("ftx-material-treatment/99");
        fs::write(dir.join("wrong.json"), wrong.to_string()).unwrap();
        assert_eq!(
            refusal(main, invoke(main, &dir, &["treatment", "wrong.json"]))["code"],
            "invalid_document"
        );
        wrong = serde_json::from_str(DENIM).unwrap();
        wrong["after"]["record"]["name"] = json!("An edited state with the original lock");
        fs::write(dir.join("stale-state.json"), wrong.to_string()).unwrap();
        refusal(main, invoke(main, &dir, &["treatment", "stale-state.json"]));
        let mut wrong_pattern_request: Value =
            serde_json::from_str(&fs::read_to_string(dir.join("pattern-treatment.json")).unwrap())
                .unwrap();
        wrong_pattern_request["schema"] = json!("ftx-pattern-treatment/99");
        fs::write(
            dir.join("wrong-pattern.json"),
            wrong_pattern_request.to_string(),
        )
        .unwrap();
        assert_eq!(
            refusal(
                main,
                invoke(
                    main,
                    &dir,
                    &[
                        "treatment-pattern",
                        "pattern.json",
                        "wrong-pattern.json",
                        "refused.json"
                    ]
                )
            )["code"],
            "invalid_document"
        );
        assert!(!dir.join("refused.json").exists());
        let mut stale_pattern: Value =
            serde_json::from_str(&fs::read_to_string(dir.join("pattern.json")).unwrap()).unwrap();
        stale_pattern["fabric"]["material"]["record"]["name"] = json!("Another source lot");
        fs::write(dir.join("stale-pattern.json"), stale_pattern.to_string()).unwrap();
        refusal(
            main,
            invoke(
                main,
                &dir,
                &[
                    "treatment-pattern",
                    "stale-pattern.json",
                    "pattern-treatment.json",
                    "stale-output.json",
                ],
            ),
        );
        assert!(!dir.join("stale-output.json").exists());
    }
}
