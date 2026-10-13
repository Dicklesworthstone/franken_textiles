//! Exercise the real binary, saved pattern JSON and validation, not just the alteration functions.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

fn workspace() -> (PathBuf, Value) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-alter-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/oracle/pattern/drafts.json");
    let fixture: Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let input = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap()["input"]
        .clone();
    for (name, value) in [
        ("source.json", &input["pattern"]),
        ("measurements.json", &input["measurements"]),
        ("fit.json", &input["fit_spec"]),
    ] {
        std::fs::write(dir.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    (dir, input)
}

fn run(dir: &Path, args: &[&str], exit: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{args:?}\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["schema"], "ftx-cli/1");
    assert_eq!(envelope["exit"], exit);
    assert_eq!(envelope["ok"], exit == 0);
    envelope
}

fn saved(dir: &Path, name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(name)).unwrap()).unwrap()
}

#[test]
fn all_five_operations_save_history_without_touching_the_source() {
    let (dir, input) = workspace();
    let original = std::fs::read(dir.join("source.json")).unwrap();
    let cases: &[(&str, &[&str])] = &[
        (
            "slash-spread",
            &[
                "--piece", "front", "--axis", "x", "--at", "50", "--add", "2",
            ],
        ),
        (
            "move",
            &["--piece", "front", "--vertex", "side_hem", "--dy", "-10"],
        ),
        ("true-seam", &["--seam", "side_R", "--end", "start"]),
        (
            "cap-ease",
            &[
                "--piece",
                "sleeve",
                "--seams",
                "armhole_f_R,armhole_b_R",
                "--target",
                "1.06",
            ],
        ),
        (
            "transfer-curve",
            &[
                "--from",
                "source.json:sleeve.R/cap_f",
                "--to",
                "front/armhole",
            ],
        ),
    ];
    for (op, options) in cases {
        let name = format!("{op}.json");
        let mut args = vec!["alter", op, "source.json", "--out", &name];
        args.extend_from_slice(options);
        let result = run(&dir, &args, 0);
        assert_eq!(result["result"]["structure_errors"], json!([]));
        let changed = saved(&dir, &name);
        assert_eq!(changed["meta"]["origin"], "VECTOR_EDIT");
        let history = changed["meta"]["history"].as_array().unwrap();
        assert_eq!(history.last().unwrap()["op"], *op);
        assert_eq!(
            changed["meta"]["source"],
            input["pattern"]["meta"]["source"]
        );
    }
    assert_eq!(std::fs::read(dir.join("source.json")).unwrap(), original);
}

#[test]
fn saved_damage_can_be_validated_repaired_and_validated_again() {
    let (dir, _) = workspace();
    run(
        &dir,
        &[
            "alter",
            "move",
            "source.json",
            "--piece",
            "front",
            "--vertex",
            "side_hem",
            "--dy",
            "-10",
            "--out",
            "damaged.json",
        ],
        0,
    );
    let validate = |path: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&dir)
            .args([
                "validate",
                path,
                "--measurements",
                "measurements.json",
                "--fit",
                "fit.json",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(matches!(output.status.code(), Some(0 | 2)));
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"].clone()
    };
    let before = validate("damaged.json");
    run(
        &dir,
        &[
            "alter",
            "true-seam",
            "damaged.json",
            "--seam",
            "side_R",
            "--end",
            "start",
            "--out",
            "repaired.json",
        ],
        0,
    );
    let after = validate("repaired.json");
    for seam in ["side_R", "side_L"] {
        let status = |v: &Value| {
            v["seams"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["seam"] == seam)
                .unwrap()["status"]
                .clone()
        };
        assert_eq!(status(&before), "FAIL");
        assert_eq!(status(&after), "PASS");
    }
}

#[test]
fn impossible_or_malformed_requests_never_publish_a_file() {
    let (dir, _) = workspace();
    for (target, code) in [("10", "no_root"), ("NaN", "non_finite")] {
        let reply = run(
            &dir,
            &[
                "alter",
                "cap-ease",
                "source.json",
                "--piece",
                "sleeve",
                "--seams",
                "armhole_f_R,armhole_b_R",
                "--target",
                target,
                "--out",
                "absent.json",
            ],
            3,
        );
        assert_eq!(reply["refusal"]["code"], code);
        assert!(!dir.join("absent.json").exists());
    }
    for options in [
        vec!["--dx", "1", "--dx", "2"],
        vec!["--dx", "--dy", "2"],
        vec!["--delta-x", "1"],
        vec!["extra.json"],
    ] {
        let mut args = vec![
            "alter",
            "move",
            "source.json",
            "--piece",
            "front",
            "--vertex",
            "side_hem",
            "--out",
            "absent.json",
        ];
        args.extend_from_slice(&options);
        assert_eq!(run(&dir, &args, 3)["refusal"]["code"], "invalid_document");
        assert!(!dir.join("absent.json").exists());
    }
}

#[test]
fn output_collision_including_hard_links_never_clobbers_a_pattern() {
    let (dir, _) = workspace();
    let original = std::fs::read(dir.join("source.json")).unwrap();
    std::fs::write(dir.join("existing.json"), "previous result").unwrap();
    std::fs::hard_link(dir.join("source.json"), dir.join("alias.json")).unwrap();
    for name in ["source.json", "existing.json", "alias.json"] {
        let reply = run(
            &dir,
            &[
                "alter",
                "move",
                "source.json",
                "--piece",
                "front",
                "--vertex",
                "side_hem",
                "--dy",
                "-10",
                "--out",
                name,
            ],
            3,
        );
        assert_eq!(reply["refusal"]["code"], "io");
    }
    assert_eq!(std::fs::read(dir.join("source.json")).unwrap(), original);
    assert_eq!(std::fs::read(dir.join("alias.json")).unwrap(), original);
    assert_eq!(
        std::fs::read_to_string(dir.join("existing.json")).unwrap(),
        "previous result"
    );
}

#[test]
fn structure_failures_remain_explicit_results_not_fit_claims() {
    let (dir, _) = workspace();
    let reply = run(
        &dir,
        &[
            "alter",
            "slash-spread",
            "source.json",
            "--piece",
            "front",
            "--axis",
            "x",
            "--at",
            "0",
            "--add",
            "-1000",
            "--out",
            "invalid.json",
        ],
        2,
    );
    assert!(
        !reply["result"]["structure_errors"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(dir.join("invalid.json").exists());
}
