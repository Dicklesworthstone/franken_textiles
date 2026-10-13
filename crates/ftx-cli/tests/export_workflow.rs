//! `ftx export` and `ftx check` on the real binary: every target written and checked, the manifest kept across
//! separate runs, failures reported with exit 2, and the reference oracle's file list reproduced.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

fn workspace(case: &str) -> (PathBuf, Value) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-export-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/oracle/export/all.json");
    let fixture: Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == case)
        .unwrap()
        .clone();
    std::fs::write(
        dir.join("pattern.json"),
        serde_json::to_vec(&case["input"]["pattern"]).unwrap(),
    )
    .unwrap();
    (dir, case)
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
    envelope
}

fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if p.is_dir() {
            for s in std::fs::read_dir(&p).unwrap() {
                out.push(format!(
                    "{name}/{}",
                    s.unwrap().file_name().to_string_lossy()
                ));
            }
        } else {
            out.push(name);
        }
    }
    out.sort();
    out
}

#[test]
fn export_all_writes_the_reference_files_and_passes_its_checks() {
    let (dir, case) = workspace("testbodice");
    let r = run(
        &dir,
        &[
            "export",
            "all",
            "pattern.json",
            "--out",
            "out",
            "--created",
            "2026-10-11T12:00",
        ],
        0,
    );
    assert_eq!(r["result"]["checks"]["status"], "PASS");
    assert_eq!(r["result"]["checks"]["total"], 150);
    let want: Vec<&str> = case["output"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files_under(&dir.join("out")), want);
    let manifest =
        std::fs::read_to_string(dir.join("out/TESTBODICE_export_manifest.json")).unwrap();
    assert_eq!(
        manifest.lines().count(),
        case["output"]["manifest"].as_str().unwrap().lines().count()
    );
    let r = run(&dir, &["check", "out", "--pattern-ref", "pattern.json"], 0);
    assert_eq!(
        (
            r["result"]["passed"].as_u64(),
            r["result"]["total"].as_u64()
        ),
        (Some(150), Some(150))
    );
    // a damaged file fails its checks with exit 2
    std::fs::write(dir.join("out/TESTBODICE_plot.plt"), "IN;").unwrap();
    let r = run(&dir, &["check", "out"], 2);
    assert_eq!(r["result"]["status"], "FAIL");
    assert!(
        r["result"]["failures"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["file"] == "TESTBODICE_plot.plt")
    );
}

#[test]
fn separate_targets_share_one_manifest() {
    let (dir, _) = workspace("bolero920");
    for t in ["svg", "hpgl", "embroidery"] {
        run(
            &dir,
            &[
                "export",
                t,
                "pattern.json",
                "--out",
                "out",
                "--created",
                "2026-10-11T12:00",
            ],
            0,
        );
    }
    let text = std::fs::read_to_string(dir.join("out/ZOUAVE_BOLERO_export_manifest.json")).unwrap();
    let Some(ftx_export::PyJson::Dict(files)) = ftx_export::PyJson::parse(&text)
        .unwrap()
        .get("files")
        .cloned()
    else {
        panic!("no files in the manifest")
    };
    let keys: Vec<&str> = files.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        [
            "svg_master",
            "hpgl",
            "dst_back",
            "dst_front.R",
            "dst_front.L",
            "dst_sleeve.R",
            "dst_sleeve.L"
        ]
    );
    let r = run(&dir, &["check", "out", "--pattern-ref", "pattern.json"], 0);
    assert_eq!(r["result"]["status"], "PASS");
}

#[test]
fn bad_targets_options_and_directories_are_refused_or_failed() {
    let (dir, _) = workspace("testbodice");
    let r = run(&dir, &["export", "nope", "pattern.json", "--out", "out"], 3);
    assert_eq!(r["refusal"]["code"], "invalid_document");
    run(
        &dir,
        &[
            "export",
            "svg",
            "pattern.json",
            "--out",
            "out",
            "--bogus",
            "1",
        ],
        3,
    );
    run(
        &dir,
        &[
            "export",
            "svg",
            "pattern.json",
            "--out",
            "out",
            "--sa",
            "-2",
        ],
        3,
    );
    assert!(!dir.join("out").exists(), "a refused export writes nothing");
    std::fs::create_dir(dir.join("empty")).unwrap();
    let r = run(&dir, &["check", "empty"], 2);
    assert_eq!(r["result"]["failures"][0]["check"], "manifest");
}
