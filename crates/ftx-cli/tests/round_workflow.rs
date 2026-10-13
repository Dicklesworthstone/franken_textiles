//! Real-binary rounds over synthetic/public-prior fixtures; no private wearer data.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

fn setup() -> (PathBuf, Value, Value) {
    setup_program("fez-920")
}

fn setup_program(id: &str) -> (PathBuf, Value, Value) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-round-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/oracle/draft/programs.json");
    let fixtures: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap();
    let (m, p) = (
        case["input"]["measurements"].clone(),
        case["output"]["pattern"].clone(),
    );
    save(&dir.join("m.json"), &m);
    save(&dir.join("p.json"), &p);
    save(&dir.join("fit_spec.json"), &case["output"]["fit_spec"]);
    (dir, m, p)
}

fn save(path: &Path, v: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn run(dir: &Path, args: &[&str], code: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "{args:?}\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(envelope["exit"], code);
    envelope
}

const FIRST: &[&str] = &[
    "round",
    "ws",
    "--pattern",
    "p.json",
    "--measurements",
    "m.json",
    "--as-of",
    "2026-10-13",
];

#[test]
fn a_round_exports_checked_files_and_preserves_its_inputs() {
    let (dir, m, p) = setup();
    let mut args = FIRST.to_vec();
    args.extend([
        "--export",
        "--created",
        "2026-10-13T00:00",
        "--note",
        "first fitting",
    ]);
    let r = run(&dir, &args, 0)["result"].clone();
    assert_eq!(r["status"], "CONVERGED");
    assert_eq!(r["basis"]["measurements"], "READY_REFERENCE");
    assert_eq!(r["basis"]["sim"], "not run");
    assert_eq!(r["exports"]["pass"], true);
    assert_eq!(r["note"], "first fitting");
    let rd = dir.join("ws/rounds/000");
    assert_eq!(read(&rd.join("pattern.json")), p);
    assert_eq!(read(&rd.join("measurements.source.json")), m);
    assert_eq!(read(&rd.join("round.json")), r);
    assert_eq!(read(&rd.join("exports/checks.json"))["status"], "PASS");
    let log = std::fs::read_to_string(dir.join("ws/rounds.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 1);
    assert_eq!(serde_json::from_str::<Value>(&log).unwrap(), r);
}

#[test]
fn repeats_allocate_after_the_highest_round_and_skip_incomplete_drafts() {
    let (dir, _, _) = setup();
    run(&dir, FIRST, 0);
    let prior = std::fs::read(dir.join("ws/rounds/000/round.json")).unwrap();
    std::fs::create_dir(dir.join("ws/rounds/002")).unwrap();
    let r = run(&dir, &["round", "ws", "--as-of", "2026-10-13"], 0);
    assert_eq!(r["result"]["round"], 3);
    assert_eq!(
        std::fs::read(dir.join("ws/rounds/000/round.json")).unwrap(),
        prior
    );
    assert_eq!(
        read(&dir.join("ws/rounds/003/pattern.json")),
        read(&dir.join("p.json"))
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("ws/rounds.jsonl"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn body_changes_require_explicit_adoption_and_keep_collision_safe_backups() {
    let (dir, mut m, _) = setup();
    run(&dir, FIRST, 0);
    let old = std::fs::read(dir.join("ws/measurements.json")).unwrap();
    m["wearer"]["label"] = json!("another synthetic reference");
    save(&dir.join("new.json"), &m);
    let args = [
        "round",
        "ws",
        "--measurements",
        "new.json",
        "--as-of",
        "2026-10-13",
    ];
    let r = run(&dir, &args, 3);
    assert!(
        r["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("one body")
    );
    assert!(!dir.join("ws/rounds/001").exists());
    std::fs::write(
        dir.join("ws/measurements.before-round-001.json"),
        b"previous backup",
    )
    .unwrap();
    let mut update = args.to_vec();
    update.push("--update-measurements");
    run(&dir, &update, 0);
    assert_eq!(
        std::fs::read(dir.join("ws/measurements.before-round-001.001.json")).unwrap(),
        old
    );
    assert_eq!(
        std::fs::read(dir.join("ws/measurements.before-round-001.json")).unwrap(),
        b"previous backup"
    );
    assert_eq!(read(&dir.join("ws/measurements.json")), m);
    assert_eq!(
        std::fs::read(dir.join("ws/rounds/000/measurements.source.json")).unwrap(),
        old
    );
}

#[test]
fn priors_cannot_pass_as_measured_evidence_and_stale_fused_values_are_ignored() {
    let (dir, mut m, _) = setup();
    m["wearer"]["reference"] = json!(false);
    m["fused"]["head"]["value_mm"] = json!(9999);
    save(&dir.join("m.json"), &m);
    let r = run(&dir, FIRST, 2);
    assert_eq!(r["result"]["status"], "MEASUREMENTS_NOT_READY");
    assert_eq!(r["result"]["basis"]["flat"], "CONVERGED");
    assert!(
        r["result"]["readiness"]["blocking"]
            .as_array()
            .unwrap()
            .contains(&json!("head"))
    );
    assert!(
        read(&dir.join("ws/rounds/000/measurements.json"))["fused"]["head"]["value_mm"]
            .as_f64()
            .unwrap()
            < 1000.0
    );
}

#[test]
fn unscaled_or_blocked_patterns_never_get_true_size_exports() {
    for blocked in [false, true] {
        let (dir, _, mut p) = setup();
        p["meta"]["scale_status"] = json!("UNSCALED");
        if blocked {
            p["schema"] = json!("broken");
        }
        save(&dir.join("p.json"), &p);
        let mut args = FIRST.to_vec();
        args.push("--export");
        let r = run(&dir, &args, 2);
        assert_eq!(
            r["result"]["status"],
            if blocked { "BLOCKED" } else { "UNSCALED" }
        );
        assert!(r["result"]["exports"].is_null());
        assert!(!dir.join("ws/rounds/000/exports").exists());
    }
}

#[test]
fn preflight_refuses_bad_inputs_without_changing_the_workspace() {
    let (dir, mut m, _) = setup();
    run(&dir, &["round", "ws"], 3);
    assert!(!dir.join("ws").exists());
    m["records"] = json!("bad evidence");
    save(&dir.join("m.json"), &m);
    run(&dir, FIRST, 3);
    assert!(!dir.join("ws").exists());
}

#[test]
fn non_simulated_hat_and_invalid_patterns_do_not_load_a_body() {
    let (dir, _, _) = setup();
    let mut args = FIRST.to_vec();
    args.extend(["--sim", "--model", "missing.ftxbody"]);
    let r = run(&dir, &args, 0);
    assert!(
        r["result"]["basis"]["sim"]
            .as_str()
            .unwrap()
            .starts_with("not applicable")
    );
    assert!(r["result"]["sim"].is_null());
    assert!(!dir.join("ws/rounds/000/sim").exists());
    for blocked in [false, true] {
        let (dir, _, mut p) = setup_program("zouave_bolero-920");
        p["meta"]["scale_status"] = json!("UNSCALED");
        if blocked {
            p["schema"] = json!("broken");
        }
        save(&dir.join("p.json"), &p);
        let r = run(&dir, &args, 2);
        assert!(
            r["result"]["basis"]["sim"]
                .as_str()
                .unwrap()
                .starts_with("skipped")
        );
        assert!(r["result"]["sim"].is_null());
        assert!(!dir.join("ws/rounds/000/sim").exists());
    }
}

#[test]
fn failed_simulation_cannot_reuse_an_earlier_success() {
    let (dir, _, _) = setup_program("zouave_bolero-920");
    run(&dir, FIRST, 0);
    let old = dir.join("ws/rounds/000/sim");
    std::fs::create_dir(&old).unwrap();
    save(
        &old.join("sim_report.json"),
        &json!({"gating":[], "settled":true}),
    );
    std::fs::write(old.join("fold_viewer.html"), "old successful viewer").unwrap();
    let r = run(
        &dir,
        &[
            "round",
            "ws",
            "--sim",
            "--model",
            "missing.ftxbody",
            "--as-of",
            "2026-10-13",
        ],
        2,
    );
    let r = &r["result"];
    assert_eq!(r["status"], "SIM_GATING_FLAGS");
    assert_eq!(r["sim"]["settled"], false);
    assert!(r["sim"]["refusal"].is_object());
    assert!(r["viewer"].is_null());
    assert!(
        r["sim"]["gating"][0]
            .as_str()
            .unwrap()
            .contains("missing.ftxbody")
    );
    assert_eq!(read(&old.join("sim_report.json"))["settled"], true);
    assert_eq!(read(&dir.join("ws/rounds/001/sim_failure.json")), r["sim"]);
    assert!(!dir.join("ws/rounds/001/sim/fold_viewer.html").exists());
}

#[test]
fn simulation_options_are_checked_before_workspace_mutation() {
    for options in [
        vec!["--self-mu", "0.3"],
        vec!["--model", "model.ftxbody"],
        vec!["--sim", "--self-contact-mm", "NaN"],
        vec!["--sim", "--self-mu", "0.3"],
        vec!["--sim", "--sew", "0", "--drape", "0"],
    ] {
        let (dir, _, _) = setup();
        let mut args = FIRST.to_vec();
        args.extend(options);
        run(&dir, &args, 3);
        assert!(!dir.join("ws").exists());
    }
}
