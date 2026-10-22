//! Both native study commands reach independent real roll and finite-inventory consumers.
use ftx_export::{
    cutting::{CutPlan, stock},
    material_study::Request,
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-stock-pattern.json");
const STOCK: &str = include_str!("../../../tests/fixtures/materials/synthetic-stock-request.json");

fn request() -> Value {
    let pattern: Value = serde_json::from_str(PATTERN).unwrap();
    let hide = pattern["fabric"]["material"].clone();
    let mut roll = hide.clone();
    roll["record"]["form"] = json!("woven");
    roll["record"]["properties"]["width"] = json!({"value":0.1,"unit":"m",
        "evidence":"estimate","source":"test","method":"Synthetic roll width"});
    json!({"schema":"ftx-material-study/1","variants":[
        {"key":"hide","changes":{"schema":"ftx-material-bindings/1","default":hide}},
        {"key":"roll","changes":{"schema":"ftx-material-bindings/1","default":roll}}],
        "finite_stock":serde_json::from_str::<Value>(STOCK).unwrap(),
        "cutting":{"copies":1,"gap_mm":2.0,"edge_margin_mm":2.0,"end_margin_mm":2.0},
        "comparison":{"basis":"nominal","required_consumers":["finite_stock"],
            "objectives":[{"metric":"used_stock_mass_kg","direction":"minimize"}]}})
}

fn run(main: bool, dir: &Path) -> Value {
    let mut command = Command::new(if main {
        env!("CARGO_BIN_EXE_ftx")
    } else {
        env!("CARGO_BIN_EXE_ftx-materials")
    });
    command.current_dir(dir);
    if main {
        command.args(["--json", "materials"]);
    }
    let out = command
        .args(["study", "pattern.json", "study.json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    report.get("result").unwrap_or(&report).clone()
}

#[test]
fn both_study_commands_and_cut_job_replay_allocate_the_complete_mixed_inventory() {
    let pattern = include_str!("../../../examples/mixed-cutting/pattern.json");
    let input = include_str!("../../../examples/mixed-cutting/study.json");
    let request: Request = serde_json::from_str(input).unwrap();
    let expected = serde_json::to_value(
        request
            .evaluate(&Pattern::from_json(pattern).unwrap())
            .unwrap(),
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ftx-mixed-cutting-consumer-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("study.json"), input).unwrap();
    let job = &expected["alternatives"][0]["cutting_job"]["result"];
    assert_eq!(job["complete_inventory"], true);
    assert_eq!(job["inventory"].as_array().unwrap().len(), 6);
    assert_eq!(job["roll"]["source_piece_ids"], json!(["lining"]));
    assert_eq!(job["finite_stock"]["source_piece_ids"], json!(["shell"]));
    let replay_pattern = job["pattern_json"].as_str().unwrap();
    let replay_request = job["replay_request_json"].as_str().unwrap();
    fs::write(dir.join("job-pattern.json"), replay_pattern).unwrap();
    fs::write(dir.join("job-request.json"), replay_request).unwrap();
    for main in [false, true] {
        assert_eq!(run(main, &dir), expected);
        let mut command = Command::new(if main {
            env!("CARGO_BIN_EXE_ftx")
        } else {
            env!("CARGO_BIN_EXE_ftx-materials")
        });
        command.current_dir(&dir);
        if main {
            command.args(["--json", "materials"]);
        }
        let output = command
            .args(["cut-job", "job-pattern.json", "job-request.json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let replayed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(replayed.get("result").unwrap_or(&replayed), job);
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        pattern
    );
    assert_eq!(fs::read_to_string(dir.join("study.json")).unwrap(), input);
    assert_eq!(
        fs::read_to_string(dir.join("job-pattern.json")).unwrap(),
        replay_pattern
    );
    assert_eq!(
        fs::read_to_string(dir.join("job-request.json")).unwrap(),
        replay_request
    );
}

#[test]
fn both_study_commands_recompute_measured_vapor_transport_from_piece_substitutions() {
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let input = include_str!("../../../examples/vapor-study/leather.json");
    let request: Request = serde_json::from_str(input).unwrap();
    let expected = serde_json::to_value(
        request
            .evaluate(&Pattern::from_json(pattern).unwrap())
            .unwrap(),
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ftx-vapor-study-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("study.json"), input).unwrap();
    for main in [false, true] {
        let report = run(main, &dir);
        assert_eq!(report, expected);
        let rows = report["alternatives"].as_array().unwrap();
        assert_eq!(rows[0]["vapor"]["status"], "available");
        assert_eq!(rows[1]["vapor"]["status"], "available");
        assert_eq!(rows[2]["vapor"]["status"], "refused");
        assert_eq!(rows[2]["binding"]["status"], "available");
        assert_eq!(report["comparison"]["frontier"], json!(["chrome-neck"]));
        assert_eq!(
            rows[0]["vapor"]["result"]["layers"][0]["observation"]["evidence"],
            "measured"
        );
        assert_eq!(report["geometry_changed"], false);
        assert_eq!(report["physical_validation_claim"], false);
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        pattern
    );
    assert_eq!(fs::read_to_string(dir.join("study.json")).unwrap(), input);
}

#[test]
fn both_study_binaries_select_qualified_recipes_per_material_without_stale_reuse() {
    let pattern = include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
    let source: Value = serde_json::from_str(pattern).unwrap();
    let original = source["fabric"]["material"].clone();
    let mut changed = original.clone();
    changed["record"]["name"] = json!("Different lot with the same nominal thickness");
    let input = json!({"schema":"ftx-material-study/1",
        "variants":[
            {"key":"original","changes":{"schema":"ftx-material-bindings/1","default":original}},
            {"key":"changed","changes":{"schema":"ftx-material-bindings/1","default":changed}}],
        "sewing_selection":serde_json::from_str::<Value>(include_str!(
            "../../../tests/fixtures/materials/synthetic-sewing-selection.json")).unwrap(),
        "comparison":{"basis":"nominal","required_consumers":["sewing"],
            "objectives":[{"metric":"feed_seconds","direction":"minimize"}]}});
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let expected = serde_json::to_value(typed.evaluate(&Pattern::new(source)).unwrap()).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ftx-study-recipes-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), pattern).unwrap();
    fs::write(dir.join("study.json"), input.to_string()).unwrap();
    for main in [false, true] {
        let report = run(main, &dir);
        assert_eq!(report, expected);
        let changed = &report["alternatives"][0];
        let original = &report["alternatives"][1];
        assert_eq!(changed["sewing"]["status"], "refused");
        assert_eq!(
            changed["sewing_selection"]["result"]["no_feasible_listed_recipe"],
            true
        );
        assert!(changed["metrics"]["feed_seconds"].is_null());
        assert_eq!(original["sewing"]["status"], "available");
        assert_eq!(
            original["sewing_selection"]["result"]["selected_choices"]["side"],
            "faster-qualified"
        );
        assert_eq!(original["metrics"]["feed_seconds"], 2.0);
        assert_eq!(report["comparison"]["frontier"], json!(["original"]));
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        pattern
    );
}

#[test]
fn both_study_commands_expand_mixed_material_choices_through_actual_planners() {
    let mut input = request();
    let choices: Vec<Value> = input["variants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| json!({"key":v["key"],"selection":v["changes"]["default"]}))
        .collect();
    input.as_object_mut().unwrap().remove("variants");
    input["combinations"] = json!({"groups":[
        {"key":"main","target":{"scope":"default"},"choices":choices},
        {"key":"pocket","target":{"scope":"pieces","ids":["pocket"]},"choices":choices}
    ]});
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let expected = serde_json::to_value(
        typed
            .evaluate(&Pattern::from_json(PATTERN).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(expected["alternatives"].as_array().unwrap().len(), 4);
    assert!(
        expected["alternatives"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| {
                row["choices"].as_object().unwrap().len() == 2
                    && row["binding"]["status"] == "available"
            })
    );
    let dir = std::env::temp_dir().join(format!(
        "ftx-combination-study-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), PATTERN).unwrap();
    let original = input.to_string();
    fs::write(dir.join("study.json"), &original).unwrap();
    for main in [false, true] {
        assert_eq!(run(main, &dir), expected);
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        PATTERN
    );
    assert_eq!(
        fs::read_to_string(dir.join("study.json")).unwrap(),
        original
    );
}

#[test]
fn both_study_binaries_compare_real_stock_without_losing_independent_roll_results() {
    let dir = std::env::temp_dir().join(format!(
        "ftx-stock-study-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), PATTERN).unwrap();
    let input = request();
    fs::write(dir.join("study.json"), input.to_string()).unwrap();
    let typed: Request = serde_json::from_value(input.clone()).unwrap();
    let expected = serde_json::to_value(
        typed
            .evaluate(&Pattern::from_json(PATTERN).unwrap())
            .unwrap(),
    )
    .unwrap();
    let hide = &expected["alternatives"][0];
    let roll = &expected["alternatives"][1];
    let hide_pattern =
        Pattern::from_json(hide["binding"]["result"]["pattern_json"].as_str().unwrap()).unwrap();
    let roll_pattern =
        Pattern::from_json(roll["binding"]["result"]["pattern_json"].as_str().unwrap()).unwrap();
    let inventory: stock::Request = serde_json::from_str(STOCK).unwrap();
    assert_eq!(hide["finite_stock"]["status"], "available", "{hide}");
    assert_eq!(
        hide["finite_stock"]["result"],
        serde_json::to_value(inventory.build(&hide_pattern).unwrap()).unwrap()
    );
    assert_eq!(hide["cutting"]["status"], "refused");
    assert_eq!(roll["finite_stock"]["status"], "refused");
    assert_eq!(roll["cutting"]["status"], "available", "{roll}");
    assert_eq!(
        roll["cutting"]["result"],
        serde_json::to_value(
            CutPlan::build(&roll_pattern, typed.cutting.as_ref().unwrap()).unwrap()
        )
        .unwrap()
    );
    assert_eq!(expected["comparison"]["frontier"], json!(["hide"]));
    assert_eq!(expected["comparison"]["alternatives"][1]["eligible"], false);
    assert_eq!(
        hide["metrics"]["used_stock_mass_kg"],
        hide["finite_stock"]["result"]["markers"][0]["stock_mass_kg"]
    );
    assert_eq!(expected["hardware_execution_authorized"], false);
    for main in [false, true] {
        assert_eq!(run(main, &dir), expected);
    }
    // Replay source locks while retaining the same physical inventory, without consuming it.
    let mut replay = input;
    replay["finite_stock"] = hide["finite_stock"]["result"]["request"].clone();
    for (variant, row) in replay["variants"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(expected["alternatives"].as_array().unwrap())
    {
        variant["changes"] = row["frozen_changes"].clone();
    }
    fs::write(dir.join("study.json"), replay.to_string()).unwrap();
    for main in [false, true] {
        let actual = run(main, &dir);
        assert_eq!(actual["alternatives"], expected["alternatives"]);
        assert_eq!(actual["comparison"]["frontier"], json!(["hide"]));
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        PATTERN
    );
}
