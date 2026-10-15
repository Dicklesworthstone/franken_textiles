//! Actual command execution, source-locked replay and atomic JSON refusal behavior.
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/materials")
        .join(name)
}
#[test]
fn command_projects_frozen_drape_inputs_and_replays_the_same_request() {
    let paths = [
        fixture("synthetic-steady-surface-report.json"),
        fixture("steady-coverage-map.json"),
        fixture("steady-coverage-conditions.json"),
    ];
    let originals: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    let out = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-steady-coverage")
        .args(&paths)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["schema"], "ftx-steady-coverage-result/1");
    assert!((result["steady"]["power_w"].as_f64().unwrap() - 15.0 / 0.7 - 15.0 / 1.1).abs() < 1e-7);
    let primary = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .args(["materials", "thermal-steady-coverage", "--json"])
        .args(&paths)
        .output()
        .unwrap();
    assert!(
        primary.status.success(),
        "{}",
        String::from_utf8_lossy(&primary.stderr)
    );
    let envelope: Value = serde_json::from_slice(&primary.stdout).unwrap();
    assert_eq!(envelope["schema"], "ftx-cli/1");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["result"], result);
    for (p, bytes) in paths.iter().zip(originals) {
        assert_eq!(fs::read(p).unwrap(), bytes);
    }
    let temp =
        std::env::temp_dir().join(format!("ftx-steady-coverage-{}.json", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &result["request"]).unwrap();
    drop(file);
    let replay = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-steady-coverage")
        .arg(&temp)
        .output()
        .unwrap();
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&replay.stdout).unwrap(),
        result
    );
    let primary_replay = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .args(["materials", "thermal-steady-coverage", "--json"])
        .arg(&temp)
        .output()
        .unwrap();
    assert!(primary_replay.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&primary_replay.stdout).unwrap()["result"],
        result
    );
    // Global flags cannot weaken the per-material evidence policy or silently discard extra args.
    for extra in ["--allow-estimates", "--unknown", "extra.json"] {
        let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .args(["materials", "thermal-steady-coverage", "--json"])
            .arg(&temp)
            .arg(extra)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&out.stdout).unwrap()["ok"],
            false
        );
    }
    let mut bad = result["request"].clone();
    bad["map"]["body_triangles"] = json!([[0, 0, 2]]);
    fs::write(&temp, serde_json::to_vec(&bad).unwrap()).unwrap();
    let refused = Command::new(env!("CARGO_BIN_EXE_ftx-materials"))
        .arg("thermal-steady-coverage")
        .arg(&temp)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(refused.stdout.is_empty());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("refusal"));
    // Keep the generated test file; this repository does not permit implicit file deletion.
}
