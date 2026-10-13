//! Real-binary metric sheet calibration; synthetic coordinates only.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-sheet-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let input = json!({"schema":"ftx-sheet-pose-input/1", "reference_id":"synthetic-fixed-a4",
    "image":"synthetic-camera", "rotation_applied_degrees":0,"image_size_px":[1280,1200],
    "intrinsics":{"focal_px":[1000,1000],"principal_px":[640,480]},
    "points":[
        {"sheet_xz_m":[0,0],"pixel_px":[640,980]},
        {"sheet_xz_m":[0.21,0],"pixel_px":[745,980]},
        {"sheet_xz_m":[0.21,0.297],"pixel_px":[745,831.5]},
        {"sheet_xz_m":[0,0.297],"pixel_px":[640,831.5]},
        {"sheet_xz_m":[0.105,0.1485],"pixel_px":[692.5,905.75],"held_out":true}
    ]});
    std::fs::write(
        dir.join("sheet.json"),
        serde_json::to_vec_pretty(&input).unwrap(),
    )
    .unwrap();
    dir
}

fn run(dir: &Path, extra: &[&str], code: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args([
            "measure",
            "sheet-pose",
            "--reference",
            "sheet.json",
            "--json",
        ])
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn writes_a_usable_camera_without_overwriting_source_evidence() {
    let dir = setup();
    let original = std::fs::read(dir.join("sheet.json")).unwrap();
    let report = run(&dir, &["--out", "reports/pose.json"], 0);
    assert_eq!(report["result"]["schema"], "ftx-sheet-pose/1");
    assert_eq!(report["result"]["ok"], true);
    let saved: Value =
        serde_json::from_slice(&std::fs::read(dir.join("reports/pose.json")).unwrap()).unwrap();
    assert_eq!(saved, report["result"]);
    let center = &saved["camera"]["center_m"];
    for (i, expected) in [0.0, -2.0, 1.0].into_iter().enumerate() {
        assert!((center[i].as_f64().unwrap() - expected).abs() < 1e-9);
    }
    run(&dir, &["--out", "sheet.json"], 3);
    run(&dir, &["--out", "reports/pose.json"], 3);
    assert_eq!(std::fs::read(dir.join("sheet.json")).unwrap(), original);
    run(&dir, &["--refernece", "sheet.json"], 3);
}

#[test]
fn rejected_held_out_geometry_exits_three_with_diagnostics_not_a_camera() {
    let dir = setup();
    let mut input: Value =
        serde_json::from_slice(&std::fs::read(dir.join("sheet.json")).unwrap()).unwrap();
    input["points"][4]["pixel_px"][1] = json!(915.75);
    std::fs::write(
        dir.join("sheet.json"),
        serde_json::to_vec_pretty(&input).unwrap(),
    )
    .unwrap();
    let report = run(&dir, &["--out", "rejected.json"], 3);
    assert_eq!(report["result"]["ok"], false);
    assert!(report["result"]["camera"].is_null());
    assert_eq!(report["result"]["reprojection"][4]["held_out"], true);
    assert!(!report["result"]["flags"].as_array().unwrap().is_empty());
    assert!(dir.join("rejected.json").is_file());
}
