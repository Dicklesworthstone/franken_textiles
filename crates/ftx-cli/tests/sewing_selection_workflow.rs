//! Real-command selection and replay through the existing stitch generator; entirely synthetic.
use ftx_export::sewing::{SewOptions, program::SewProgram};
use ftx_pattern::Pattern;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

const PATTERN: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-pattern.json");
const REQUEST: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-selection.json");
static NEXT: AtomicU64 = AtomicU64::new(0);

fn workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ftx-sewing-selection-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("pattern.json"), PATTERN).unwrap();
    fs::write(dir.join("request.json"), REQUEST).unwrap();
    dir
}
fn invoke(dir: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args([
            "materials",
            "sew-select",
            "pattern.json",
            "request.json",
            "--json",
        ])
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn actual_selector_and_stitch_binary_use_the_same_complete_qualified_plan() {
    let dir = workspace();
    let out = invoke(&dir, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope: Value = serde_json::from_slice(&out.stdout).unwrap();
    let job = &envelope["result"];
    let expected: Value =
        serde_json::from_str(&ftx_export::sewing_selection::select_json(PATTERN, REQUEST).unwrap())
            .unwrap();
    assert_eq!(job, &expected);
    assert_eq!(job["report"]["trials"].as_array().unwrap().len(), 3);
    assert!(job["report"]["trials"][2]["refusal"].is_object());
    assert_eq!(job["report"]["selected_plan"]["feed_seconds"], 2.0);
    assert_eq!(
        job["report"]["selected_choices"]["side"],
        "faster-qualified"
    );
    let text = job["selected_options_json"].as_str().unwrap();
    fs::write(dir.join("chosen.json"), text).unwrap();
    let program = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .current_dir(&dir)
        .args(["sew-stitches", "pattern.json", "chosen.json"])
        .output()
        .unwrap();
    assert!(
        program.status.success(),
        "{}",
        String::from_utf8_lossy(&program.stderr)
    );
    let program: Value = serde_json::from_slice(&program.stdout).unwrap();
    let options: SewOptions = serde_json::from_str(text).unwrap();
    let expected = SewProgram::build(
        &Pattern::new(serde_json::from_str(PATTERN).unwrap()),
        &options,
    )
    .unwrap();
    assert_eq!(program, serde_json::to_value(expected).unwrap());
    assert_eq!(
        program["plan"]["options"],
        job["report"]["selected_plan"]["options"]
    );
    assert_eq!(
        program["plan"]["seams"][0]["qualification_id"],
        "synthetic-alternative"
    );
    assert_eq!(program["plan"]["hardware_execution_authorized"], false);
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        PATTERN
    );
    assert_eq!(
        fs::read_to_string(dir.join("request.json")).unwrap(),
        REQUEST
    );
}

#[test]
fn finite_budget_keeps_baseline_and_material_edit_withdraws_the_selection() {
    let dir = workspace();
    let mut request: Value = serde_json::from_str(REQUEST).unwrap();
    request["max_plan_evaluations"] = json!(1);
    fs::write(
        dir.join("request.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    let out = invoke(&dir, &[]);
    assert!(out.status.success());
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["result"]["report"]["budget_exhausted"], true);
    assert_eq!(value["result"]["report"]["optimality_proven"], false);
    assert_eq!(
        value["result"]["report"]["selected_plan"]["feed_seconds"],
        4.0
    );
    let mut pattern: Value = serde_json::from_str(PATTERN).unwrap();
    pattern["fabric"]["material"]["record"]["name"] = json!("different source lot");
    fs::write(
        dir.join("pattern.json"),
        serde_json::to_vec(&pattern).unwrap(),
    )
    .unwrap();
    let out = invoke(&dir, &[]);
    assert_eq!(out.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(value["result"]["selected_options_json"].is_null());
    assert_eq!(
        value["result"]["report"]["no_feasible_listed_recipe"],
        false
    );
    assert!(value["result"]["report"]["trials"][0]["refusal"].is_object());
}

#[test]
fn malformed_input_and_unknown_estimate_flag_cannot_bypass_source_policy() {
    let dir = workspace();
    assert!(!invoke(&dir, &["--allow-estimates"]).status.success());
    let mut request: Value = serde_json::from_str(REQUEST).unwrap();
    request["options"]["require_qualification"] = json!(false);
    fs::write(
        dir.join("request.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    let out = invoke(&dir, &[]);
    assert!(!out.status.success());
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(value.get("result").is_none());
    assert!(value["refusal"].is_object());
}

const TIMED: &str =
    include_str!("../../../tests/fixtures/materials/synthetic-sewing-timed-selection.json");

#[test]
fn actual_timed_selection_and_replay_keep_source_durations_and_joint_objective() {
    let dir = workspace();
    fs::write(dir.join("request.json"), TIMED).unwrap();
    let out = invoke(&dir, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope: Value = serde_json::from_slice(&out.stdout).unwrap();
    let job = &envelope["result"];
    let expected: Value =
        serde_json::from_str(&ftx_export::sewing_selection::select_json(PATTERN, TIMED).unwrap())
            .unwrap();
    assert_eq!(job, &expected);
    assert_eq!(job["report"]["selected_objective_seconds"], 8.0);
    assert_eq!(job["report"]["selected_choices"]["join"], "template");
    assert_eq!(job["report"]["selected_choices"]["finish"], "template");
    assert_eq!(job["report"]["trials"][3]["feed_seconds"], 4.0);
    assert_eq!(job["report"]["trials"][3]["objective_seconds"], 14.0);
    assert_eq!(job["report"]["optimality_proven"], true);
    fs::write(
        dir.join("chosen.json"),
        job["selected_options_json"].as_str().unwrap(),
    )
    .unwrap();
    let replay = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .current_dir(&dir)
        .args(["sew-stitches", "pattern.json", "chosen.json"])
        .output()
        .unwrap();
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["plan"], job["report"]["selected_plan"]);
    assert_eq!(replay["plan"]["schema"], "ftx-sewing-plan/9");
    assert_eq!(replay["stitches"].as_array().unwrap().len(), 80);
    assert_eq!(
        fs::read_to_string(dir.join("pattern.json")).unwrap(),
        PATTERN
    );
    assert_eq!(fs::read_to_string(dir.join("request.json")).unwrap(), TIMED);
}

#[test]
fn real_command_rejects_unmeasured_timing_before_publishing_any_recipe() {
    let dir = workspace();
    let mut request: Value = serde_json::from_str(TIMED).unwrap();
    request["options"]["ordering"]["setup_times"]["transitions"] = json!([]);
    fs::write(dir.join("request.json"), request.to_string()).unwrap();
    let out = invoke(&dir, &[]);
    assert!(!out.status.success());
    let envelope: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(envelope.get("result").is_none());
    assert!(
        envelope["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("duration")
    );
    assert!(!invoke(&dir, &["--allow-estimates"]).status.success());
}
