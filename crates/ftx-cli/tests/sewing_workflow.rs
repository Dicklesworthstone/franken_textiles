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

#[test]
fn multipass_cli_expands_every_directed_stitch_and_matches_library() {
    let p = fixture("synthetic-sewing-pattern.json");
    let o = fixture("synthetic-sewing-multipass-options.json");
    let before = fs::read_to_string(&o).unwrap();
    let plan = run(&["sew-plan", &p, &o]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let plan: Value = serde_json::from_slice(&plan.stdout).unwrap();
    let out = run(&["sew-stitches", &p, &o]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: Value = serde_json::from_slice(&out.stdout).unwrap();
    let program = ftx_export::sewing::program::SewProgram::build(
        &Pattern::from_json(PATTERN).unwrap(),
        &serde_json::from_str(&before).unwrap(),
    )
    .unwrap();
    assert_eq!(parsed, serde_json::to_value(&program).unwrap());
    assert_eq!(parsed["plan"], plan);
    assert_eq!(program.plan.stitch_intervals, 90);
    assert_eq!(program.plan.feed_seconds, 13.0);
    assert_eq!(program.stitches.len(), 90);
    assert_eq!(program.stitches.iter().filter(|s| s.begins_run).count(), 6);
    let mut feed_seconds = 0.0;
    for step in &program.stitches {
        let operation = &program.plan.seams[step.operation];
        assert_eq!(step.a_mm, operation.stations.a[step.to_station]);
        assert_eq!(step.b_mm, operation.stations.b[step.to_station]);
        assert_eq!(step.to_station.abs_diff(step.from_station), 1);
        assert_eq!(
            step.advance_mm[0].is_sign_positive(),
            step.to_station > step.from_station
        );
        assert_eq!(
            step.advance_mm[1].is_sign_positive(),
            step.to_station > step.from_station
        );
        assert!((step.feed_b_over_a - 1.0).abs() < 1e-12);
        feed_seconds += step.feed_seconds;
    }
    assert!((feed_seconds - 13.0).abs() < 1e-12);
    assert_eq!(program.stitches[49].to_station, 40);
    assert_eq!(program.stitches[50].operation, 1);
    assert_eq!(program.stitches[50].from_station, 40);
    assert_eq!(program.stitches[89].to_station, 0);
    assert_eq!(fs::read_to_string(o).unwrap(), before);
    assert_eq!(fs::read_to_string(p).unwrap(), PATTERN);
}

#[test]
fn expanded_program_admits_legacy_jobs_but_never_emits_partial_refused_travel() {
    let p = fixture("synthetic-sewing-pattern.json");
    let out = run(&[
        "sew-stitches",
        &p,
        &fixture("synthetic-sewing-options.json"),
    ]);
    assert!(out.status.success());
    let legacy: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(legacy["schema"], "ftx-sewing-program/1");
    assert_eq!(legacy["plan"]["schema"], "ftx-sewing-plan/2");
    assert_eq!(legacy["stitches"].as_array().unwrap().len(), 40);
    let mut o: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-sewing-multipass-options.json"
    ))
    .unwrap();
    o["machine"]["reverse"]["max_stitches_per_minute"] = json!(500.0);
    let out = run(&["sew-stitches", &p, &changed_pattern(&o)]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let refusal: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert!(
        refusal["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("Reverse stitch rate")
    );
    o["machine"]["reverse"]["max_stitches_per_minute"] = json!(800.0);
    o["passes"][0]["after"] = json!(["reinforce"]);
    let out = run(&["sew-stitches", &p, &changed_pattern(&o)]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn offset_cli_program_retains_paths_and_materials_without_mutating_inputs() {
    let p = fixture("synthetic-sewing-pattern.json");
    let o = fixture("synthetic-sewing-topstitch-options.json");
    let before = fs::read(&o).unwrap();
    let out = run(&["sew-stitches", &p, &o]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], "ftx-sewing-program/2");
    assert_eq!(v["plan"]["schema"], "ftx-sewing-plan/3");
    assert_eq!(v["stitches"].as_array().unwrap().len(), 90);
    let seam = &v["plan"]["seams"][1];
    assert_eq!(
        seam["path"]["polylines_mm"][0],
        json!([[17.0, 0.0], [17.0, 100.0]])
    );
    assert_eq!(
        seam["path"]["polylines_mm"][1],
        json!([[4.0, 0.0], [4.0, 100.0]])
    );
    assert_eq!(v["stitches"][50]["a_mm"][0], 17.0);
    assert!((v["stitches"][50]["a_mm"][1].as_f64().unwrap() - 97.5).abs() < 1e-10);
    let core = ftx_export::sewing::program::SewProgram::build(
        &Pattern::from_json(PATTERN).unwrap(),
        &serde_json::from_slice(&before).unwrap(),
    )
    .unwrap();
    assert_eq!(v, serde_json::to_value(core).unwrap());
    assert_eq!(fs::read(&o).unwrap(), before);
    assert_eq!(fs::read_to_string(&p).unwrap(), PATTERN);
    let mut o: Value = serde_json::from_slice(&before).unwrap();
    o["passes"][1]["stitch_path"]["offset_mm"] = json!([-11.0, 0.0]);
    let out = run(&["sew-plan", &p, &changed_pattern(&o)]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn sewing_svg_is_create_only_and_never_publishes_refused_geometry() {
    let p = fixture("synthetic-sewing-pattern.json");
    let o = fixture("synthetic-sewing-topstitch-options.json");
    let target = changed_pattern(&json!({"sentinel":"unchanged"}));
    let out = run(&["sew-svg", &p, &o, "topstitch", &target]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "{\"sentinel\":\"unchanged\"}"
    );
    let target = format!("{target}.svg");
    let out = run(&["sew-svg", &p, &o, "unknown", &target]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!std::path::Path::new(&target).exists());
    let out = run(&["sew-svg", &p, &o, "topstitch", &target]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = fs::read_to_string(&target).unwrap();
    let expected = ftx_export::sewing::preview::svg(
        &Pattern::from_json(PATTERN).unwrap(),
        &serde_json::from_str(&fs::read_to_string(&o).unwrap()).unwrap(),
        "topstitch",
    )
    .unwrap();
    assert_eq!(got, expected);
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        receipt["svg_sha256"],
        ftx_core::receipt::sha256_hex(got.as_bytes())
    );
    assert_eq!(receipt["hardware_execution_authorized"], false);
    assert_eq!(
        run(&["sew-svg", &p, &o, "topstitch", &target])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), got);
}
