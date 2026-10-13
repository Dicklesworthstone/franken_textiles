//! Real-binary wearer adoption and immutable workspace drafting, spec 04 §11.2–4.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-ws-draft-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    let m = ftx_measure::reference_wearer(920.0, Some(30.0), None, None).unwrap();
    fs::write(dir.join("m.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    dir
}

fn run(dir: &Path, args: &[&str], exit: i32) -> Value {
    let o = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        o.status.code(),
        Some(exit),
        "{args:?}\n{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_slice(&o.stdout).unwrap()
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn every_program_drafts_into_a_new_round_and_preserves_the_first_contract() {
    let dir = setup();
    let mut first_contract = Vec::new();
    for (i, program) in ["zouave_bolero", "zouave_trousers", "fez"]
        .iter()
        .enumerate()
    {
        let r = run(
            &dir,
            &[
                "draft",
                program,
                "--measurements",
                "m.json",
                "--ws",
                "w",
                "--as-of",
                "2026-10-13",
            ],
            0,
        );
        assert_eq!(r["result"]["round"], i);
        assert_eq!(r["result"]["readiness"]["status"], "READY_REFERENCE");
        let round = dir.join(format!("w/rounds/{i:03}"));
        for name in [
            "pattern.json",
            "fit_spec.json",
            "readiness.json",
            "fused_measurements.json",
        ] {
            assert!(read(&round.join(name)).is_object(), "{name}");
        }
        assert_eq!(
            fs::read(round.join("measurements.json")).unwrap(),
            fs::read(dir.join("m.json")).unwrap()
        );
        let contract = fs::read(dir.join("w/fit_spec.json")).unwrap();
        if i == 0 {
            first_contract = contract.clone();
        }
        assert_eq!(contract, first_contract);
    }
    assert!(
        !dir.join("w/rounds.jsonl").exists(),
        "draft must not claim to have run a round"
    );
    assert_eq!(
        fs::read(dir.join("w/rounds/000/fit_spec.json")).unwrap(),
        first_contract
    );
}

#[test]
fn numbering_skips_gaps_and_archives_never_clobber_earlier_bodies() {
    let dir = setup();
    run(
        &dir,
        &["draft", "fez", "--measurements", "m.json", "--ws", "w"],
        0,
    );
    fs::create_dir(dir.join("w/rounds/002")).unwrap();
    let old = fs::read(dir.join("m.json")).unwrap();
    let first_pattern = fs::read(dir.join("w/rounds/000/pattern.json")).unwrap();
    let mut changed = read(&dir.join("m.json"));
    changed["wearer"]["label"] = json!("another fictional wearer");
    fs::write(
        dir.join("changed.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    let refusal = run(
        &dir,
        &[
            "draft",
            "fez",
            "--measurements",
            "changed.json",
            "--ws",
            "w",
        ],
        3,
    );
    assert!(
        refusal["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("one body")
    );
    assert_eq!(fs::read(dir.join("w/measurements.json")).unwrap(), old);
    assert!(!dir.join("w/rounds/003").exists());
    fs::write(
        dir.join("w/measurements.before-round-003.json"),
        "prior archive",
    )
    .unwrap();
    fs::write(
        dir.join("w/measurements.before-round-003.001.json"),
        "older archive",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            dir.join("w/measurements.json"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }
    let r = run(
        &dir,
        &[
            "draft",
            "fez",
            "--measurements",
            "changed.json",
            "--ws",
            "w",
            "--update-measurements",
        ],
        0,
    );
    assert_eq!(r["result"]["round"], 3);
    assert_eq!(
        fs::read(dir.join("w/measurements.before-round-003.002.json")).unwrap(),
        old
    );
    assert_eq!(
        fs::read_to_string(dir.join("w/measurements.before-round-003.json")).unwrap(),
        "prior archive"
    );
    assert_eq!(
        fs::read_to_string(dir.join("w/measurements.before-round-003.001.json")).unwrap(),
        "older archive"
    );
    assert_eq!(
        fs::read(dir.join("w/rounds/000/pattern.json")).unwrap(),
        first_pattern
    );
    assert_eq!(read(&dir.join("w/measurements.json")), changed);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(dir.join("w/measurements.before-round-003.002.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
        assert_eq!(
            fs::metadata(dir.join("w/rounds/003/measurements.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn rejected_inputs_do_not_create_or_modify_a_workspace() {
    let dir = setup();
    for args in [
        vec!["draft", "unknown", "--measurements", "m.json", "--ws", "w"],
        vec!["draft", "fez", "--measurements", "missing", "--ws", "w"],
        vec![
            "draft",
            "fez",
            "--measurements",
            "m.json",
            "--ws",
            "w",
            "--style",
            "missing",
        ],
        vec![
            "draft",
            "fez",
            "--measurements",
            "m.json",
            "--ws",
            "w",
            "--out",
            "d",
        ],
        vec![
            "draft",
            "fez",
            "--measurements",
            "m.json",
            "--ws",
            "w",
            "--as-of",
            "2026-02-30",
        ],
    ] {
        run(&dir, &args, 3);
        assert!(!dir.join("w").exists());
    }
    fs::write(
        dir.join("bad.json"),
        r#"{"schema":"fabric-measurements/1","records":"broken"}"#,
    )
    .unwrap();
    run(
        &dir,
        &["draft", "fez", "--measurements", "bad.json", "--ws", "w"],
        3,
    );
    assert!(!dir.join("w").exists());
}

#[test]
fn fused_cache_is_recomputed_and_a_busy_workspace_refuses_writes() {
    let dir = setup();
    let mut m = read(&dir.join("m.json"));
    m["fused"]["head"]["value_mm"] = json!(1.0);
    fs::write(dir.join("m.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    run(
        &dir,
        &["draft", "fez", "--measurements", "m.json", "--ws", "w"],
        0,
    );
    assert!(
        read(&dir.join("w/rounds/000/fused_measurements.json"))["fused"]["head"]["value_mm"]
            .as_f64()
            .unwrap()
            > 400.0
    );
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("w/.ftx-lock"))
        .unwrap();
    lock.lock().unwrap();
    run(
        &dir,
        &["draft", "fez", "--measurements", "m.json", "--ws", "w"],
        3,
    );
    assert!(!dir.join("w/rounds/001").exists());
    drop(lock);
    run(
        &dir,
        &["draft", "fez", "--measurements", "m.json", "--ws", "w"],
        0,
    );
}
