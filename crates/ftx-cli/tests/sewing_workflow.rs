//! Real binary coverage of synthetic sewing plans and the source-bearing MIT catalog.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

use ftx_export::sewing::{SewOptions, SewPlan};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const OPTIONS: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-options.json");

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .args(args)
        .output()
        .unwrap()
}
fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/materials")
        .join(name)
        .to_str()
        .unwrap()
        .to_string()
}
fn changed_pattern(value: &Value) -> String {
    let path = std::env::temp_dir().join(format!(
        "ftx-sewing-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn real_cli_matches_library_and_does_not_mutate_evidence() {
    let p = fixture("synthetic-sewing-pattern.json");
    let o = fixture("synthetic-sewing-options.json");
    let out = run(&["sew-plan", &p, &o]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: Value = serde_json::from_slice(&out.stdout).unwrap();
    let pattern = Pattern::from_json(PATTERN).unwrap();
    let options: SewOptions = serde_json::from_str(OPTIONS).unwrap();
    assert_eq!(
        parsed,
        serde_json::to_value(SewPlan::build(&pattern, &options).unwrap()).unwrap()
    );
    assert_eq!(parsed["stitch_intervals"], 40);
    assert_eq!(parsed["feed_seconds"], 4.0);
    assert_eq!(fs::read_to_string(p).unwrap(), PATTERN);
    assert_eq!(fs::read_to_string(o).unwrap(), OPTIONS);
    assert_eq!(parsed["hardware_execution_authorized"], false);
    let guide = include_str!("../../../docs/material-sewing-plans.md");
    let example = guide
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(example).unwrap(),
        serde_json::from_str::<Value>(OPTIONS).unwrap()
    );
}

#[test]
fn material_swap_affects_machine_feasibility_and_failure_has_no_partial_stdout() {
    let mut p: Value = serde_json::from_str(PATTERN).unwrap();
    p["fabric"]["material"]["record"]["properties"]["thickness"]["value"] = json!(2.0);
    let path = changed_pattern(&p);
    let out = run(&["sew-plan", &path, &fixture("synthetic-sewing-options.json")]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let refusal: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert!(
        refusal["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("stack thickness")
    );
    p["fabric"]["material"]["allow_estimates"] = json!(false);
    let denied = run(&[
        "sew-plan",
        &changed_pattern(&p),
        &fixture("synthetic-sewing-options.json"),
    ]);
    assert_eq!(denied.status.code(), Some(2));
    assert!(denied.stdout.is_empty());
}

#[test]
fn measured_specimens_are_discoverable_without_a_false_drape_readiness_claim() {
    let out = run(&["catalog", "mit.fabric-2013."]);
    assert!(out.status.success());
    let rows: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rows["materials"].as_array().unwrap().len(), 30);
    let out = run(&["show", "mit.fabric-2013.08"]);
    assert!(out.status.success());
    let shown: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(shown["simulation_ready"], false);
    let props = &shown["selection"]["record"]["properties"];
    assert_eq!(props["areal_density"]["value"], 4.475);
    assert_eq!(props["areal_density"]["unit"], "oz/yd2");
    assert_eq!(props["specimen_flexural_rigidity"]["unit"], "lbf*in2");
    assert!(props.get("bending_rigidity").is_none());
}
