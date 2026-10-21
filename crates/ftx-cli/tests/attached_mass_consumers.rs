//! Both native study entry points preserve sourced attachments through real material substitution
//! and assembly accounting. All pattern/material/mass observations here are explicitly synthetic.
use ftx_export::{assembly_mass, material_study::Request};
use ftx_pattern::Pattern;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn extra() -> Value {
    json!({"id":"closure-set","mass":{"value":0.03,"unit":"kg","bounds":[0.028,0.032],
        "evidence":"estimate","source":"test","method":"Synthetic retained total group",
        "conditions":{"inventory":"one assembly total"}},
        "sources":{"test":{"reference":"test:attachment-consumers","accessed_on":"2026-10-20",
            "note":"Synthetic software input, not measured hardware"}},"allow_estimates":true})
}

fn inputs() -> (Value, Value) {
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    pattern["attached_mass"] = json!({"schema":"ftx-attached-mass/1","groups":[{
        "extra":extra(),"targets":[{"instance":"band","vertex":"a","fraction":0.25},
            {"instance":"band","vertex":"b","fraction":0.75}]}]});
    let light = pattern["fabric"]["material"].clone();
    let mut heavy = light.clone();
    heavy["record"]["properties"]["areal_density"]["value"] = json!(0.4);
    let mut thread = extra();
    thread["id"] = json!("sewing-thread");
    thread["mass"]["value"] = json!(0.002);
    thread["mass"]["bounds"] = json!([0.0018, 0.0022]);
    let request = json!({"schema":"ftx-material-study/1","variants":[
        {"key":"a-light","changes":{"schema":"ftx-material-bindings/1","default":light}},
        {"key":"b-heavy","changes":{"schema":"ftx-material-bindings/1","default":heavy}}],
        "assembly_mass":{"schema":"ftx-assembly-mass/1","inventory_complete":true,
            "seam_allowance_mm":0.0,"extras":[extra(),thread]},
        "comparison":{"basis":"nominal","required_consumers":["assembly_mass"],
            "objectives":[{"metric":"assembly_mass_kg","direction":"minimize"}]}});
    (pattern, request)
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
    let output = command
        .args(["study", "pattern.json", "study.json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    report.get("result").unwrap_or(&report).clone()
}

fn both_match(pattern: &Value, request: &Value) -> Value {
    let typed: Request = serde_json::from_value(request.clone()).unwrap();
    let expected =
        serde_json::to_value(typed.evaluate(&Pattern::new(pattern.clone())).unwrap()).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ftx-attached-study-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let original_pattern = pattern.to_string();
    let original_request = request.to_string();
    fs::write(dir.join("pattern.json"), &original_pattern).unwrap();
    fs::write(dir.join("study.json"), &original_request).unwrap();
    for main in [false, true] {
        assert_eq!(run(main, &dir), expected);
    }
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        original_pattern
    );
    assert_eq!(
        fs::read_to_string(dir.join("study.json")).unwrap(),
        original_request
    );
    expected
}

#[test]
fn both_native_binaries_use_one_sourced_inventory_after_sheet_substitution() {
    let (pattern, request) = inputs();
    let result = both_match(&pattern, &request);
    let inventory: assembly_mass::Request =
        serde_json::from_value(request["assembly_mass"].clone()).unwrap();
    let rows = result["alternatives"].as_array().unwrap();
    let mut sheet_masses = Vec::new();
    for row in rows {
        assert_eq!(row["assembly_mass"]["status"], "available");
        let bound =
            Pattern::from_json(row["binding"]["result"]["pattern_json"].as_str().unwrap()).unwrap();
        assert_eq!(bound.raw()["attached_mass"], pattern["attached_mass"]);
        let direct = inventory.evaluate(&bound).unwrap();
        assert_eq!(
            row["assembly_mass"]["result"],
            serde_json::to_value(&direct).unwrap()
        );
        assert_eq!(direct.extras.len(), 2); // closure repeats in request and pattern, but counts once.
        let total = direct.assembly_mass.kg.unwrap();
        let sheet = direct.sheet_mass.kg.unwrap();
        assert!((total - sheet - 0.032).abs() < 1e-15);
        assert_eq!(row["metrics"]["assembly_mass_kg"], total);
        assert_eq!(
            row["assembly_mass"]["result"]["attached_inventory"],
            pattern["attached_mass"]
        );
        assert_eq!(
            row["assembly_mass"]["result"]["request"]["extras"][0],
            extra()
        );
        sheet_masses.push(sheet);
    }
    assert_eq!(sheet_masses[1], 2. * sheet_masses[0]);
    assert_ne!(
        rows[0]["assembly_mass"]["result"]["pattern_sha256"],
        rows[1]["assembly_mass"]["result"]["pattern_sha256"]
    );
    assert_eq!(result["comparison"]["frontier"], json!(["a-light"]));

    let mut without_duplicate = request;
    without_duplicate["assembly_mass"]["extras"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    let deduplicated = both_match(&pattern, &without_duplicate);
    for (a, b) in rows
        .iter()
        .zip(deduplicated["alternatives"].as_array().unwrap())
    {
        assert_eq!(
            a["assembly_mass"]["result"]["assembly_mass"],
            b["assembly_mass"]["result"]["assembly_mass"]
        );
        assert_eq!(a["metrics"], b["metrics"]);
    }
}

#[test]
fn native_source_conflicts_refuse_the_consumer_and_unknown_mass_never_wins() {
    let (mut pattern, mut request) = inputs();
    request["assembly_mass"]["extras"][0]["sources"]["test"]["note"] =
        json!("Different observation identity");
    let conflict = both_match(&pattern, &request);
    for row in conflict["alternatives"].as_array().unwrap() {
        assert_eq!(row["binding"]["status"], "available");
        assert_eq!(row["assembly_mass"]["status"], "refused");
        assert!(
            row["assembly_mass"]["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("conflicts")
        );
    }
    assert_eq!(conflict["comparison"]["frontier"], json!([]));
    request["assembly_mass"]["extras"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    pattern["attached_mass"]["groups"][0]["extra"]["mass"] = Value::Null;
    let unknown = both_match(&pattern, &request);
    for row in unknown["alternatives"].as_array().unwrap() {
        assert_eq!(row["assembly_mass"]["status"], "available");
        assert!(
            row["assembly_mass"]["result"]["sheet_mass"]["kg"]
                .as_f64()
                .is_some()
        );
        assert!(row["assembly_mass"]["result"]["assembly_mass"]["kg"].is_null());
        assert!(row["metrics"]["assembly_mass_kg"].is_null());
    }
    assert_eq!(unknown["comparison"]["frontier"], json!([]));
}
