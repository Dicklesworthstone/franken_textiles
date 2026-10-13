//! Real-binary still-window workflow on analytically projected calibrated landmarks.

use ftx_core::{Mat3, Vec2, Vec3};
use ftx_measure::{
    camera::{Camera, Distortion, Intrinsics},
    triangulate::Partition,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

fn camera(x: f64) -> Camera {
    Camera {
        intrinsics: Intrinsics {
            focal_px: Vec2::new(900.0, 900.0),
            principal_px: Vec2::new(640.0, 480.0),
            distortion: Distortion::default(),
        },
        world_to_camera: Mat3::from_rows([[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]),
        center_m: Vec3::new(x, -2.0, 0.7),
    }
}

fn request(motion_m: f64, held_motion_m: f64) -> Value {
    let landmarks = [
        ("left", Vec3::new(-0.2, 0.0, 0.9), Partition::Fit),
        ("right", Vec3::new(0.2, 0.0, 0.9), Partition::Fit),
        ("waist", Vec3::new(0.0, 0.0, 0.6), Partition::Fit),
        ("held", Vec3::new(0.0, 0.0, 1.1), Partition::HeldOut),
    ];
    let frames: Vec<_> = [-0.45, -0.15, 0.15, 0.45]
        .into_iter()
        .enumerate()
        .map(|(i, x)| {
            let camera = camera(x);
            let late = i >= 2;
            let points: Vec<_> = landmarks
                .iter()
                .map(|(name, point, partition)| {
                    let motion = if late {
                        if *partition == Partition::HeldOut {
                            held_motion_m
                        } else {
                            motion_m
                        }
                    } else {
                        0.0
                    };
                    let pixel = camera
                        .project(*point + Vec3::new(motion, 0.0, 0.0))
                        .unwrap();
                    json!({"name":name,"pixel_px":pixel,"partition":match partition { Partition::Fit => "fit", Partition::HeldOut => "held_out" }})
                })
                .collect();
            json!({
                "frame":{"stream":"synthetic-clip","timestamp_us":i as u64 * 50_000},
                "camera":camera,
                "landmarks":points
            })
        })
        .collect();
    json!({
        "schema":"ftx-still-window-input/1",
        "metric_scale_source":"synthetic stationary reference",
        "observation_evidence":"analytically projected synthetic landmarks",
        "max_motion_mm":5.0,
        "frames":frames
    })
}

fn scratch() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ftx-still-window-{}-{now}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    path
}

fn run(dir: &Path, extra: &[&str], expected: i32) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(["measure", "still-window", "input.json", "--json"])
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn result(out: &Output) -> Value {
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["result"].clone()
}

#[test]
fn stationary_tracks_publish_a_review_candidate_without_certifying_stillness() {
    let dir = scratch();
    std::fs::write(
        dir.join("input.json"),
        serde_json::to_vec_pretty(&request(0.0, 0.0)).unwrap(),
    )
    .unwrap();
    let out = run(&dir, &["--out", "candidate.json"], 0);
    let value = result(&out);
    assert_eq!(value["status"], "STILL_CANDIDATE");
    assert_eq!(value["start_us"], 0);
    assert_eq!(value["end_us"], 150_000);
    assert_eq!(value["fit_landmarks"], 3);
    assert_eq!(value["held_out_landmarks"], 1);
    assert!(value["no_claim"].as_str().unwrap().contains("Human review"));
    let saved: Value =
        serde_json::from_slice(&std::fs::read(dir.join("candidate.json")).unwrap()).unwrap();
    assert_eq!(saved, value);
    assert!(String::from_utf8_lossy(&out.stdout).contains("STILL_CANDIDATE"));
}

#[test]
fn subtle_motion_and_independent_held_out_motion_are_rejected() {
    for (fit, held) in [(0.010, 0.0), (0.0, 0.010)] {
        let dir = scratch();
        std::fs::write(
            dir.join("input.json"),
            serde_json::to_vec(&request(fit, held)).unwrap(),
        )
        .unwrap();
        let value = result(&run(&dir, &[], 2));
        assert_eq!(value["status"], "REJECTED");
        assert!(
            value["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["passed"] == false)
        );
    }
}

#[test]
fn source_and_previous_reports_are_never_overwritten() {
    let dir = scratch();
    let bytes = serde_json::to_vec_pretty(&request(0.0, 0.0)).unwrap();
    std::fs::write(dir.join("input.json"), &bytes).unwrap();
    run(&dir, &["--out", "report.json"], 0);
    let report = std::fs::read(dir.join("report.json")).unwrap();
    run(&dir, &["--out", "report.json"], 3);
    assert_eq!(std::fs::read(dir.join("report.json")).unwrap(), report);
    run(&dir, &["--out", "input.json"], 3);
    assert_eq!(std::fs::read(dir.join("input.json")).unwrap(), bytes);
}

#[test]
fn malformed_and_over_budget_inputs_fail_without_output() {
    let dir = scratch();
    std::fs::write(dir.join("input.json"), b"{}").unwrap();
    run(&dir, &["--out", "nested/report.json"], 3);
    assert!(!dir.join("nested").exists());

    let file = std::fs::File::create(dir.join("input.json")).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    run(&dir, &["--out", "budget.json"], 4);
    assert!(!dir.join("budget.json").exists());
}
