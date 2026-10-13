//! Exercise the actual documented synthetic bundle through the real ftx binary.
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-bundle-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(
        dir.join("bundle.json"),
        include_bytes!("../../../docs/examples/bundle.json"),
    )
    .unwrap();
    dir
}

fn run(dir: &Path, extra: &[&str], exit: i32) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(["measure", "bundle-adjust", "bundle.json"])
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(exit),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn json(dir: &Path, extra: &[&str], exit: i32) -> Value {
    let mut args = vec!["--json"];
    args.extend_from_slice(extra);
    serde_json::from_slice(&run(dir, &args, exit).stdout).unwrap()
}

#[test]
fn documented_example_recovers_cameras_and_landmark_and_writes_a_new_report() {
    let dir = setup();
    let out = json(&dir, &["--out", "reports/fit.json"], 0);
    let report = &out["result"];
    assert_eq!(report["schema"], "ftx-bundle/1");
    assert_eq!(report["status"], "ACCEPTED");
    assert!(report["iterations"].as_u64().unwrap() > 1);
    let saved: Value =
        serde_json::from_slice(&std::fs::read(dir.join("reports/fit.json")).unwrap()).unwrap();
    assert_eq!(&saved, report);
    let p = &report["solution"]["landmarks"][0]["position_m"];
    for (i, x) in [0.0, -0.4, 1.2].into_iter().enumerate() {
        assert!((p[i].as_f64().unwrap() - x).abs() < 1e-8);
    }
    for (i, x) in [-0.5, 0.0, 0.5].into_iter().enumerate() {
        let c = &report["solution"]["views"][i]["camera"]["center_m"];
        assert!((c[0].as_f64().unwrap() - x).abs() < 1e-8);
        assert!((c[1].as_f64().unwrap() + 2.0).abs() < 1e-8);
    }
    let human = run(&dir, &[], 0);
    assert!(String::from_utf8_lossy(&human.stdout).contains("BUNDLE: ACCEPTED"));
    assert_eq!(*report, json(&dir, &[], 0)["result"]);
}

#[test]
fn held_out_failure_and_budget_stop_write_diagnostics_without_geometry() {
    let dir = setup();
    let budget = json(&dir, &["--iterations", "1", "--out", "budget.json"], 4);
    assert_eq!(budget["result"]["status"], "BUDGET_STOP");
    assert!(budget["result"]["solution"].is_null());
    assert!(dir.join("budget.json").is_file());
    let mut input: Value =
        serde_json::from_slice(&std::fs::read(dir.join("bundle.json")).unwrap()).unwrap();
    input["observations"][17]["pixel_px"][1] = serde_json::json!(360);
    std::fs::write(dir.join("bundle.json"), serde_json::to_vec(&input).unwrap()).unwrap();
    let rejected = json(&dir, &["--out", "rejected.json"], 2);
    assert_eq!(rejected["result"]["status"], "REJECTED");
    assert!(rejected["result"]["solution"].is_null());
    assert_eq!(rejected["result"]["checks"].as_array().unwrap().len(), 18);
    assert!(String::from_utf8_lossy(&run(&dir, &[], 2).stdout).contains("No optimized geometry"));
}

#[test]
fn evidence_and_existing_reports_cannot_be_overwritten_even_through_aliases() {
    let dir = setup();
    let original = std::fs::read(dir.join("bundle.json")).unwrap();
    json(&dir, &["--out", "bundle.json"], 3);
    std::fs::hard_link(dir.join("bundle.json"), dir.join("alias.json")).unwrap();
    json(&dir, &["--out", "alias.json"], 3);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("bundle.json", dir.join("symlink.json")).unwrap();
        json(&dir, &["--out", "symlink.json"], 3);
    }
    json(&dir, &["--out", "report.json"], 0);
    let saved = std::fs::read(dir.join("report.json")).unwrap();
    json(&dir, &["--out", "report.json"], 3);
    assert_eq!(original, std::fs::read(dir.join("bundle.json")).unwrap());
    assert_eq!(saved, std::fs::read(dir.join("report.json")).unwrap());
}

#[test]
fn malformed_options_and_schemas_refuse_without_creating_an_output() {
    let dir = setup();
    for value in ["0", "101", "-1", "oops"] {
        json(&dir, &["--iterations", value, "--out", "never.json"], 3);
        assert!(!dir.join("never.json").exists());
    }
    json(&dir, &["--itterations", "2"], 3);
    let mut input: Value =
        serde_json::from_slice(&std::fs::read(dir.join("bundle.json")).unwrap()).unwrap();
    input["observations"][0]["sigma_px"] = serde_json::json!(0);
    std::fs::write(dir.join("bundle.json"), serde_json::to_vec(&input).unwrap()).unwrap();
    json(&dir, &["--out", "never.json"], 3);
    assert!(!dir.join("never.json").exists());
}
