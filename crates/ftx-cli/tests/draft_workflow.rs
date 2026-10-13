//! `ftx draft` on the real binary, then the rest of a round's flat half: each program at 92 cm drafts the
//! oracle's pattern and fit spec, validates CONVERGED, and exports files that pass every acceptance check.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

fn workspace() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-draft-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    dir
}

fn programs() -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/oracle/draft/programs.json");
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
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
    assert_eq!(envelope["exit"], exit);
    envelope
}

/// Equal, numbers within 1e-9.
fn close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() <= 1e-9
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| close(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| close(p, q)))
        }
        _ => a == b,
    }
}

#[test]
fn drafts_validate_and_export_like_the_reference() {
    let fx = programs();
    for program in ["zouave_bolero", "zouave_trousers", "fez"] {
        let case = fx["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == format!("{program}-920"))
            .unwrap();
        let dir = workspace();
        std::fs::write(
            dir.join("m.json"),
            serde_json::to_vec(&case["input"]["measurements"]).unwrap(),
        )
        .unwrap();
        let r = run(
            &dir,
            &["draft", program, "--measurements", "m.json", "--out", "d"],
            0,
        );
        assert_eq!(r["result"]["structure_errors"], serde_json::json!([]));
        for (file, key) in [
            ("d/pattern.json", "pattern"),
            ("d/fit_spec.json", "fit_spec"),
        ] {
            let got: Value =
                serde_json::from_slice(&std::fs::read(dir.join(file)).unwrap()).unwrap();
            assert!(
                close(&got, &case["output"][key]),
                "{program}: {file} differs from the oracle's"
            );
        }
        let v = run(
            &dir,
            &[
                "validate",
                "d/pattern.json",
                "--measurements",
                "m.json",
                "--fit",
                "d/fit_spec.json",
            ],
            0,
        );
        assert_eq!(v["result"]["status"], "CONVERGED", "{program}");
        let e = run(
            &dir,
            &[
                "export",
                "all",
                "d/pattern.json",
                "--out",
                "d/exports",
                "--created",
                "2026-10-11T12:00",
            ],
            0,
        );
        assert_eq!(e["result"]["checks"]["status"], "PASS", "{program}");
    }
}

#[test]
fn unknown_programs_and_style_keys_are_refused() {
    let dir = workspace();
    let fx = programs();
    std::fs::write(
        dir.join("m.json"),
        serde_json::to_vec(&fx["cases"][0]["input"]["measurements"]).unwrap(),
    )
    .unwrap();
    let r = run(
        &dir,
        &["draft", "fedora", "--measurements", "m.json", "--out", "d"],
        3,
    );
    assert_eq!(r["refusal"]["code"], "unknown_parameter");
    std::fs::write(dir.join("s.json"), r#"{"chest_eas": 100}"#).unwrap();
    let r = run(
        &dir,
        &[
            "draft",
            "zouave_bolero",
            "--measurements",
            "m.json",
            "--style",
            "s.json",
            "--out",
            "d",
        ],
        3,
    );
    assert!(
        r["refusal"]["message"]
            .as_str()
            .unwrap()
            .starts_with("unknown style key(s) ['chest_eas']")
    );
    assert!(!dir.join("d").exists(), "a refused draft writes nothing");
}
