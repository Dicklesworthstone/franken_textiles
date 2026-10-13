//! Real-binary photo evidence through calibration, measurement, fusion and readiness.
//! Synthetic coordinates only; no wearer photographs or private measurements.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-photo-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    save(
        &dir.join("reference.json"),
        &json!({
            "schema":"ftx-photo-reference/1", "reference":"plain_sheet", "paper":"letter",
            "image_size_px":[1500,2000], "image":"synthetic-camera",
            "corners_px":[[234.1,820.6],[665.9,820.6],[665.9,1379.4],[234.1,1379.4]]
        }),
    );
    save(
        &dir.join("points.json"),
        &json!({
            "arm_length@right":{"points":[[100,900],[900,900]],"view":"P1"},
            "head":{"points":[[100,900],[1100,900]],"depth_offset_mm":200}
        }),
    );
    dir
}

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn invoke(dir: &Path, args: &[&str], json: bool) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ftx"));
    cmd.current_dir(dir).args(args);
    if json {
        cmd.arg("--json");
    }
    cmd.output().unwrap()
}

fn run(dir: &Path, args: &[&str], code: i32) -> Value {
    let out = invoke(dir, args, true);
    assert_eq!(
        out.status.code(),
        Some(code),
        "{args:?}: {} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["exit"], code);
    value
}

const MEASURE: &[&str] = &[
    "measure",
    "measure-photo",
    "--reference",
    "reference.json",
    "--points",
    "points.json",
];

#[test]
fn photo_records_survive_saved_file_fusion_without_becoming_tape() {
    let dir = setup();
    let scale = run(
        &dir,
        &[
            "measure",
            "photo-scale",
            "--reference",
            "reference.json",
            "--out",
            "scale.json",
        ],
        0,
    );
    assert_eq!(read(&dir.join("scale.json")), scale["result"]);
    assert_eq!(scale["result"]["mm_per_px"], 0.5);
    let mut args = MEASURE.to_vec();
    args.extend([
        "--taken-on",
        "2026-10-13",
        "--out",
        "measurements/photo.json",
    ]);
    let result = run(&dir, &args, 0)["result"].clone();
    assert_eq!(result, read(&dir.join("measurements/photo.json")));
    assert_eq!(result["taken_on"], "2026-10-13");
    assert_eq!(result["records"][0]["value_mm"], 400.0);
    assert_eq!(result["records"][1]["value_mm"], 500.0);
    assert_eq!(result["records"][1]["sigma_terms_mm"]["depth_offset"], 40.0);
    assert_eq!(
        result["photo"]["reference_input"],
        read(&dir.join("reference.json"))
    );
    run(&dir, &["measure", "validate", "measurements/photo.json"], 0);
    let fused = run(
        &dir,
        &[
            "measure",
            "fuse",
            "measurements/photo.json",
            "--no-priors",
            "--out",
            "fused.json",
        ],
        0,
    );
    for name in ["head", "arm_length"] {
        assert_eq!(
            fused["result"]["fused"][name]["primary_provenance"],
            "PHOTO_ESTIMATED"
        );
    }
    let ready = run(
        &dir,
        &[
            "measure",
            "ready",
            "fused.json",
            "--garment",
            "hat_fez",
            "--no-priors",
            "--as-of",
            "2026-10-13",
        ],
        3,
    );
    assert_eq!(ready["result"]["status"], "NOT_READY");
    assert_eq!(ready["result"]["blocking"], json!(["head"]));
}

#[test]
fn thumbnail_coordinates_seed_and_angle_uncertainty_are_cli_controls() {
    let dir = setup();
    save(
        &dir.join("points.json"),
        &json!({"shoulder_slope_deg@L":[[50,450],[250,650]]}),
    );
    let mut args = MEASURE.to_vec();
    args.extend([
        "--points-from-width",
        "750",
        "--view",
        "P3",
        "--sheet-level-sigma-deg",
        "2.5",
        "--click-px",
        "2",
        "--bootstrap",
        "300",
        "--seed",
        "18446744073709551615",
    ]);
    let r = run(&dir, &args, 0)["result"].clone();
    assert_eq!(r, run(&dir, &args, 0)["result"]);
    let rec = &r["records"][0];
    assert_eq!(rec["points_px"], json!([[100.0, 900.0], [500.0, 1300.0]]));
    assert_eq!(rec["value_deg"], 45.0);
    assert_eq!(rec["sigma_terms_deg"]["sheet_level"], 2.5);
    assert!(rec["sigma_deg"].as_f64().unwrap() >= 2.5);
    assert!(
        rec["method"]
            .as_str()
            .unwrap()
            .contains("photo P3 plain Letter")
    );
    assert!(r["taken_on"].is_null()); // No capture date invented from the current clock.
    assert_eq!(r["photo"]["measurement_options"]["seed"], u64::MAX);
}

#[test]
fn wrong_paper_produces_a_diagnostic_but_no_measurement() {
    let dir = setup();
    let mut reference = read(&dir.join("reference.json"));
    reference["paper"] = json!("a4");
    save(&dir.join("reference.json"), &reference);
    let result = run(
        &dir,
        &[
            "measure",
            "photo-scale",
            "--reference",
            "reference.json",
            "--out",
            "rejected.json",
        ],
        3,
    );
    assert_eq!(result["result"]["ok"], false);
    assert!(result["result"].get("homography_px_to_board_mm").is_none());
    assert!(
        result["result"]["warnings"][0]
            .as_str()
            .unwrap()
            .contains("ASPECT_MISMATCH")
    );
    let mut args = MEASURE.to_vec();
    args.extend(["--out", "absent/photo.json"]);
    let failed = run(&dir, &args, 3);
    assert_eq!(failed["refusal"]["code"], "unscaled");
    assert!(!dir.join("absent").exists());
    assert_eq!(read(&dir.join("rejected.json")), result["result"]);
}

#[test]
fn invalid_controls_refuse_without_touching_output() {
    for options in [
        vec!["--image", "photo.png"],
        vec!["--click-px", "NaN"],
        vec!["--bootstrap", "2.5"],
        vec!["--bootstrap", "19"],
        vec!["--seed", "18446744073709551616"],
        vec!["--seed", "-1"],
        vec!["--camera-distance-mm", "0"],
        vec!["--depth-offset-mm", "2500"],
        vec!["--taken-on", "2026-02-30"],
        vec!["unexpected.json"],
    ] {
        let dir = setup();
        let mut args = MEASURE.to_vec();
        args.extend(options);
        args.extend(["--out", "out/photo.json"]);
        run(&dir, &args, 3);
        assert!(!dir.join("out").exists());
    }
}

#[test]
fn existing_outputs_and_input_aliases_are_not_clobbered() {
    let dir = setup();
    let original = std::fs::read(dir.join("reference.json")).unwrap();
    std::fs::hard_link(dir.join("reference.json"), dir.join("alias.json")).unwrap();
    for out in ["reference.json", "points.json", "alias.json"] {
        let before = std::fs::read(dir.join(out)).unwrap();
        let mut args = MEASURE.to_vec();
        args.extend(["--out", out]);
        run(&dir, &args, 3);
        assert_eq!(std::fs::read(dir.join(out)).unwrap(), before);
    }
    assert_eq!(std::fs::read(dir.join("reference.json")).unwrap(), original);
    let mut args = MEASURE.to_vec();
    args.extend(["--out", "photo.json"]);
    let out = invoke(&dir, &args, false);
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("wrote photo.json"));
    let before = std::fs::read(dir.join("photo.json")).unwrap();
    run(&dir, &args, 3);
    assert_eq!(std::fs::read(dir.join("photo.json")).unwrap(), before);
}

#[test]
fn imported_charuco_detections_are_accepted_without_an_image_runtime() {
    let dir = setup();
    let corners: Vec<Value> = (0..35)
        .map(|id| {
            json!({"id":id,
        "point_px":[200 + (id % 5 + 1)*44, 300 + (id / 5 + 1)*44]})
        })
        .collect();
    save(
        &dir.join("reference.json"),
        &json!({"schema":"ftx-photo-reference/1",
        "reference":"charuco", "image":"not-read-or-uploaded.png", "image_size_px":[1500,2000],
        "board":{"squares_x":6,"squares_y":8,"square_mm":22},"corners":corners}),
    );
    let report = run(
        &dir,
        &["measure", "photo-scale", "--reference", "reference.json"],
        0,
    );
    assert_eq!(report["result"]["inliers"], 35);
    let value = run(&dir, MEASURE, 0);
    assert_eq!(value["result"]["records"][0]["value_mm"], 400.0);
    assert!(
        value["result"]["records"][0]["method"]
            .as_str()
            .unwrap()
            .contains("ChArUco")
    );
    assert!(
        value["result"]["photo"]["bootstrap"]["method"]
            .as_str()
            .unwrap()
            .starts_with("resample identified inlier")
    );
}
