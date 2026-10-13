//! Exercise the actual CLI, output receipts and nonmutation guarantees with synthetic cameras.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ftx-triangulation-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("input.json"),
            include_str!("../../../docs/examples/triangulation.json"),
        )
        .unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn run(dir: &Path, extra: &[&str], expected: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(["measure", "triangulate", "input.json", "--json"])
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(envelope["exit"], expected);
    envelope
}

#[test]
fn accepted_receipt_and_human_summary() {
    let dir = Scratch::new();
    let out = run(&dir.0, &["--out", "point.json"], 0);
    let result = &out["result"];
    assert_eq!(read(&dir.0.join("point.json")), *result);
    assert_eq!(result["status"], "ACCEPTED");
    assert_eq!(result["fit_views"], 2);
    assert_eq!(result["held_out_views"], 1);
    assert!(result["point_m"][0].as_f64().unwrap().abs() < 1e-10);
    assert!(result["point_m"][1].as_f64().unwrap().abs() < 1e-10);
    assert!((result["point_m"][2].as_f64().unwrap() - 1.3).abs() < 1e-10);
    assert!(result["input"]["metric_scale_source"].is_string());
    assert!(result.get("records").is_none());
    let human = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir.0)
        .args(["measure", "triangulate", "input.json"])
        .output()
        .unwrap();
    assert!(human.status.success());
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("TRIANGULATION: ACCEPTED"));
    assert!(text.contains("1 held-out"));
    assert!(text.contains("not a body fit"));
}

#[test]
fn rejected_and_unobservable_reports_have_no_usable_point() {
    let dir = Scratch::new();
    let path = dir.0.join("input.json");
    let mut input = read(&path);
    input["observations"][2]["pixel_px"][0] = 540.into();
    save(&path, &input);
    let out = run(&dir.0, &["--out", "rejected.json"], 2);
    assert_eq!(out["result"]["status"], "REJECTED");
    assert!(out["result"]["point_m"].is_null());
    assert_eq!(read(&dir.0.join("rejected.json")), out["result"]);
    input["observations"][1]["partition"] = "held_out".into();
    save(&path, &input);
    let out = run(&dir.0, &[], 2);
    assert_eq!(out["result"]["status"], "UNOBSERVABLE");
    assert!(out["result"]["point_m"].is_null());
}

#[test]
fn invalid_input_or_options_do_not_create_output() {
    let dir = Scratch::new();
    let path = dir.0.join("input.json");
    let before = std::fs::read(&path).unwrap();
    run(&dir.0, &["--min-angle-deg", "0", "--out", "new/point.json"], 3);
    assert!(!dir.0.join("new").exists());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut input = read(&path);
    input["observations"][2]["frame"]["timestamp_us"] = 1_000_001.into();
    save(&path, &input);
    run(&dir.0, &["--out", "new/point.json"], 3);
    assert!(!dir.0.join("new").exists());
    input["observations"][2]["frame"]["timestamp_us"] = 1_000_000.into();
    input["observations"][0]["frame_number"] = 1.into();
    save(&path, &input);
    run(&dir.0, &["--out", "new/point.json"], 3);
    assert!(!dir.0.join("new").exists());
}

#[test]
fn inputs_and_existing_reports_cannot_be_overwritten_even_by_alias() {
    let dir = Scratch::new();
    let input = dir.0.join("input.json");
    let before = std::fs::read(&input).unwrap();
    run(&dir.0, &["--out", "input.json"], 3);
    assert_eq!(std::fs::read(&input).unwrap(), before);
    std::fs::hard_link(&input, dir.0.join("alias.json")).unwrap();
    run(&dir.0, &["--out", "alias.json"], 3);
    assert_eq!(std::fs::read(&input).unwrap(), before);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&input, dir.0.join("link.json")).unwrap();
        run(&dir.0, &["--out", "link.json"], 3);
        assert_eq!(std::fs::read(&input).unwrap(), before);
    }
    run(&dir.0, &["--out", "point.json"], 0);
    let report = std::fs::read(dir.0.join("point.json")).unwrap();
    run(&dir.0, &["--out", "point.json"], 3);
    assert_eq!(std::fs::read(dir.0.join("point.json")).unwrap(), report);
}
