//! Real command preflights and paired-round failure handling; synthetic/public-prior data only.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn dir() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-two-start-cli-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    dir
}

fn command(dir: &Path, args: &[&str], code: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn round_checks_pair_policy_before_creating_any_workspace() {
    let d = dir();
    for options in [
        vec!["--two-start-sew", "18"],
        vec!["--sim", "--two-start-sew", "12"],
        vec!["--sim", "--two-start-sew", "18", "--spread-hem-mm", "NaN"],
        vec!["--sim", "--spread-seam-pp", "1.5"],
    ] {
        let mut args = vec!["round", "absent"];
        args.extend(options);
        let report = command(&d, &args, 3);
        assert!(
            !report["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("pattern")
        );
        assert!(!d.join("absent").exists());
    }
}

#[test]
fn direct_pair_cannot_overwrite_an_old_report_even_when_model_loading_will_fail() {
    let d = dir();
    std::fs::write(
        d.join("p.json"),
        include_str!("../../../tests/fixtures/materials/synthetic-scenario-pattern.json")
            .as_bytes(),
    )
    .unwrap();
    std::fs::write(d.join("m.json"), b"{}").unwrap();
    std::fs::create_dir(d.join("out")).unwrap();
    std::fs::write(d.join("out/sim_report.json"), b"old evidence").unwrap();
    let r = command(
        &d,
        &[
            "drape",
            "p.json",
            "--measurements",
            "m.json",
            "--out",
            "out",
            "--model",
            "missing.ftxbody",
            "--two-start-sew",
            "18",
        ],
        3,
    );
    assert!(
        r["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("already exists")
    );
    assert_eq!(
        std::fs::read(d.join("out/sim_report.json")).unwrap(),
        b"old evidence"
    );
}

#[test]
fn failed_paired_round_is_recorded_as_failure_without_a_successful_clip() {
    let d = dir();
    let fixtures: Value = serde_json::from_slice(
        &std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/oracle/draft/programs.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap();
    for (name, value) in [
        ("p.json", &case["output"]["pattern"]),
        ("m.json", &case["input"]["measurements"]),
        ("fit.json", &case["output"]["fit_spec"]),
    ] {
        std::fs::write(d.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    let r = command(
        &d,
        &[
            "round",
            "ws",
            "--pattern",
            "p.json",
            "--measurements",
            "m.json",
            "--fit",
            "fit.json",
            "--as-of",
            "2026-10-13",
            "--sim",
            "--two-start-sew",
            "18",
            "--model",
            "missing.ftxbody",
        ],
        2,
    );
    let result = &r["result"];
    assert_eq!(result["status"], "SIM_GATING_FLAGS");
    assert_eq!(result["sim"]["verdict"], "FAILED");
    assert_eq!(result["viewer"], Value::Null);
    assert!(
        result["sim"]["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("missing.ftxbody")
    );
    assert!(d.join("ws/rounds/000/sim_failure.json").is_file());
    assert!(!d.join("ws/rounds/000/sim/sim_report.json").exists());
    assert!(!d.join("ws/rounds/000/sim/fold_viewer.html").exists());
    assert_eq!(result["images"], json!([]));
}
