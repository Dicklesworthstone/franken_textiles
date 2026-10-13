//! Command-level measurement workflow tests; all inputs are fictional/specification examples.

use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

fn file(name: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ftx-measure-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_string_lossy().into_owned()
}
fn run(args: &[&str], code: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .arg("measure")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "{:?}\nstdout {}\nstderr {}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(envelope["schema"], "ftx-cli/1");
    assert_eq!(envelope["command"], "measure");
    assert_eq!(envelope["exit"], code);
    assert_eq!(envelope["ok"], code == 0);
    envelope
}
fn init(path: &str) {
    run(
        &[
            "init",
            "--out",
            path,
            "--label",
            "fictional example",
            "--stature",
            "92cm",
            "--age-months",
            "30",
            "--taken-on",
            "2026-10-01",
        ],
        0,
    );
}

#[test]
fn init_add_repeat_fuse_and_ready_round_trip() {
    let path = file("measurements.json");
    init(&path);
    let d = run(
        &[
            "add",
            &path,
            "--name",
            "head-girth",
            "--value",
            "49cm",
            "--value",
            "49.2cm",
            "--value",
            "48.8cm",
        ],
        0,
    );
    assert_eq!(d["result"]["records"].as_array().unwrap().len(), 4);
    assert_eq!(d["result"]["records"][1]["name"], "head");
    let fused = file("fused.json");
    let f = run(&["fuse", &path, "--no-priors", "--out", &fused], 0);
    assert_eq!(f["result"]["fused"]["head"]["value_mm"], 490.0);
    assert_eq!(f["result"]["fused"]["head"]["sigma_mm"], 5.0);
    assert_eq!(f["result"]["fused"]["head"]["sources"][0]["n_repeats"], 3);
    let written: Value = serde_json::from_slice(&std::fs::read(&fused).unwrap()).unwrap();
    assert_eq!(written, f["result"]); // File is not a CLI envelope.
    let r = run(
        &["ready", &fused, "--garment", "hat", "--as-of", "2026-10-12"],
        0,
    );
    assert_eq!(r["result"]["status"], "READY");
    let r = run(
        &[
            "ready",
            &fused,
            "--garment",
            "bodice",
            "--as-of",
            "2026-10-12",
        ],
        3,
    );
    assert_eq!(r["result"]["status"], "NOT_READY");
    assert_eq!(run(&["validate", &path], 0)["result"]["valid"], true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn reference_priors_and_garment_guidance_are_exposed() {
    let path = file("reference.json");
    let r = run(
        &[
            "reference",
            "--stature-mm",
            "920",
            "--age-months",
            "30",
            "--out",
            &path,
        ],
        0,
    );
    assert_eq!(r["result"]["records"].as_array().unwrap().len(), 35);
    let r = run(
        &[
            "ready",
            &path,
            "--garment",
            "zouave",
            "--cuff-level",
            "knee",
            "--nappies",
            "--elastic-waist",
            "--as-of",
            "2026-10-12",
        ],
        0,
    );
    assert_eq!(r["result"]["status"], "READY_REFERENCE");
    assert_eq!(r["result"]["for_real_child"].as_array().unwrap().len(), 9);
    assert_eq!(r["result"]["blocking"], json!([]));
    assert_eq!(
        run(&["catalogue"], 0)["result"].as_object().unwrap().len(),
        35
    );
    let c = run(&["catalogue", "--garment", "sirwal"], 0);
    assert_eq!(c["result"]["garment"], "full_trousers_gathered");
    assert_eq!(c["result"]["must"].as_array().unwrap().len(), 5);
    assert_eq!(c["result"]["targets_override"]["hip"], 20.0);
    assert_eq!(
        run(&["prior", "--stature-mm", "920", "--age-months", "30"], 0)["result"]["chest"]["value_mm"],
        540.0
    );
}

#[test]
fn invalid_edits_preserve_original_bytes_and_refuse_ambiguous_units() {
    let path = file("original.json");
    init(&path);
    let before = std::fs::read(&path).unwrap();
    for values in [vec!["49"], vec!["49cm", "NaNcm"]] {
        let mut args = vec!["add", path.as_str(), "--name", "head"];
        for v in &values {
            args.extend(["--value", v]);
        }
        assert!(run(&args, 3).get("refusal").is_some());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    run(
        &[
            "add",
            &path,
            "--name",
            "shoulder_slope",
            "--value",
            "18",
            "--provenance",
            "PHOTO_ESTIMATED",
        ],
        3,
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let d = run(
        &[
            "add",
            &path,
            "--name",
            "shoulder_slope",
            "--value",
            "18deg",
            "--sigma",
            "2deg",
            "--provenance",
            "PHOTO_ESTIMATED",
        ],
        0,
    );
    assert_eq!(d["result"]["records"][1]["value_deg"], 18.0);
    assert_eq!(d["result"]["records"][1]["sigma_deg"], 2.0);
    let bad = file("must-not-be-created.json");
    run(&["reference", "--stature-mm", "1800", "--out", &bad], 3);
    assert!(!PathBuf::from(bad).exists());
}

#[test]
fn merging_files_and_clearing_stale_cache_keeps_original_evidence() {
    let first = file("first.json");
    let second = file("second.json");
    std::fs::write(&first,serde_json::to_vec(&json!({"schema":"fabric-measurements/1","wearer":{"label":null},"taken_on":null,"records":[],"fused":{},"photo":{"obsolete":true}})).unwrap()).unwrap();
    init(&second);
    run(&["add", &second, "--name", "head", "--value", "49cm"], 0);
    let original = std::fs::read(&second).unwrap();
    let r = run(&["fuse", &first, &second, "--no-priors"], 0);
    assert_eq!(r["result"]["wearer"]["stature_mm"], 920.0);
    assert_eq!(r["result"]["taken_on"], "2026-10-01");
    assert!(r["result"].get("photo").is_none());
    assert_eq!(std::fs::read(&second).unwrap(), original);
    run(&["fuse", &second, "--out", &second], 0);
    let r = run(&["add", &second, "--name", "head", "--value", "49.1cm"], 0);
    assert_eq!(r["result"]["fused"], json!({}));
    assert!(r["result"].get("fusion").is_none());
    assert_eq!(r["result"]["records"].as_array().unwrap().len(), 3);
}

#[test]
fn malformed_arguments_and_documents_have_non_success_json_envelopes() {
    for args in [
        vec!["prior", "--stature-mm"],
        vec!["prior", "--stature-mm", "NaN"],
        vec!["catalogue", "--garmnet", "bodice"],
        vec!["fuse"],
        vec!["init", "--label", "example", "--out", "--nappies"],
    ] {
        assert!(run(&args, 3).get("refusal").is_some());
    }
    let path = file("invalid.json");
    std::fs::write(&path, b"{}").unwrap();
    let r = run(&["validate", &path], 3);
    assert_eq!(r["result"]["valid"], false);
    assert!(!r["result"]["errors"].as_array().unwrap().is_empty());
}
