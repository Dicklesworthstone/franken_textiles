//! Saved-round visual reviews exercise the real CLI, not a synthetic report-writing harness.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ftx_pattern::alter;
use serde_json::{Value, json};

fn setup(kind: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-review-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let cases: Value = serde_json::from_slice(
        &std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/oracle/draft/programs.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let c = cases["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == format!("{kind}-920"))
        .unwrap();
    save(&dir, "pattern.json", &c["output"]["pattern"]);
    save(&dir, "fit.json", &c["output"]["fit_spec"]);
    save(&dir, "measurements.json", &c["input"]["measurements"]);
    dir
}

fn save(dir: &Path, file: &str, value: &Value) {
    std::fs::write(dir.join(file), serde_json::to_vec(value).unwrap()).unwrap();
}

fn read(dir: &Path, file: &str) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(file)).unwrap()).unwrap()
}

fn text(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap()
}

fn run(dir: &Path, options: &[&str], code: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args([
            "round",
            "ws",
            "--pattern",
            "pattern.json",
            "--fit",
            "fit.json",
            "--measurements",
            "measurements.json",
            "--as-of",
            "2026-10-13",
            "--json",
        ])
        .args(options)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn completed_export_round_has_portable_previews_and_downloadable_files() {
    let dir = setup("fez");
    let r = run(
        &dir,
        &["--export", "--note", "<script>alert(1)</script>"],
        0,
    );
    let html = text(&dir, "ws/report.html");
    assert!(html.contains("Construction review"));
    for basis in [
        "Flat construction",
        "Drape prediction",
        "Manufacturing checks",
        "Measurement evidence",
    ] {
        assert!(html.contains(basis));
    }
    assert!(html.contains("READY_REFERENCE"));
    assert!(html.contains("Reference chart only"));
    assert!(html.contains("not proof of fit"));
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    for name in ["preview.svg", "preview_dark.svg"] {
        assert!(html.contains(&format!("src=\"rounds/000/{name}\"")));
        assert!(text(&dir, &format!("ws/rounds/000/{name}")).contains("not for cutting"));
    }
    assert!(html.contains("_LETTER.pdf"));
    assert!(html.contains("_master_mm.svg"));
    assert_eq!(r["result"]["preview"], "rounds/000/preview.svg");
    assert_eq!(r["result"], read(&dir, "ws/rounds/000/round.json"));
}

#[test]
fn successive_reviews_keep_old_failures_visible_after_a_corrected_round() {
    let dir = setup("zouave_bolero");
    let original = read(&dir, "pattern.json");
    save(
        &dir,
        "pattern.json",
        &alter::move_vertex(&original, "front", "side_hem", 0.0, -20.0).unwrap(),
    );
    run(&dir, &["--note", "damaged"], 2);
    let first = text(&dir, "ws/rounds/000/preview.svg");
    let record = text(&dir, "ws/rounds/000/round.json");
    assert!(first.contains("data-seam=\"side_R\" data-side=\"a\" data-failing=\"true\""));
    save(&dir, "pattern.json", &original);
    run(&dir, &["--note", "repaired"], 0);
    assert_eq!(text(&dir, "ws/rounds/000/preview.svg"), first);
    assert_eq!(text(&dir, "ws/rounds/000/round.json"), record);
    assert!(!text(&dir, "ws/rounds/001/preview.svg").contains("data-failing=\"true\""));
    let html = text(&dir, "ws/report.html");
    assert!(html.contains("Round 0 — NOT_CONVERGED"));
    assert!(html.contains("Round 1 — CONVERGED"));
    assert!(html.contains("rounds/000/preview.svg"));
    assert!(html.contains("rounds/001/preview.svg"));
}

#[test]
fn unready_and_blocked_rounds_publish_explanations_not_cutting_approval() {
    let dir = setup("zouave_bolero");
    let mut m = read(&dir, "measurements.json");
    m["wearer"]["reference"] = json!(false);
    save(&dir, "measurements.json", &m);
    let r = run(&dir, &[], 2);
    assert_eq!(r["result"]["status"], "MEASUREMENTS_NOT_READY");
    let html = text(&dir, "ws/report.html");
    assert!(html.contains("Measurement readiness: NOT_READY"));
    assert!(html.contains("hand_pass"));
    let mut p = read(&dir, "pattern.json");
    p["meta"]["scale_status"] = json!("UNSCALED");
    p["pieces"][0]["edges"][0]["from"] = json!("<unsafe-id>");
    save(&dir, "pattern.json", &p);
    let r = run(&dir, &["--export"], 2);
    assert_eq!(r["result"]["status"], "BLOCKED");
    assert!(!dir.join("ws/rounds/001/exports").exists());
    let svg = text(&dir, "ws/rounds/001/preview.svg");
    assert!(svg.contains("BLOCKED — UNSCALED"));
    assert!(!svg.contains("data-instance="));
    assert!(text(&dir, "ws/report.html").contains("&lt;unsafe-id&gt;"));
}

#[test]
fn failed_physics_is_distinct_from_flat_success_in_the_review() {
    let dir = setup("zouave_bolero");
    let r = run(&dir, &["--sim", "--model", "missing.ftxbody"], 2);
    assert_eq!(r["result"]["basis"]["flat"], "CONVERGED");
    assert_eq!(r["result"]["status"], "SIM_GATING_FLAGS");
    let html = text(&dir, "ws/report.html");
    assert!(html.contains("simulation failed:"));
    assert!(html.contains("gating flags (simulation failed)"));
    assert!(!html.contains("Open drape viewer"));
    assert!(!dir.join("ws/rounds/000/sim/sim_report.json").exists());
}
