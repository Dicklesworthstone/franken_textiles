//! Real command chain on a completely specified analytical camera and planar grid.
use ftx_core::receipt::sha256_hex;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let p = std::env::temp_dir().join(format!(
        "ftx-camera-calibration-{}-{time}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&p).unwrap();
    std::fs::write(
        p.join("input.json"),
        include_bytes!("../../../docs/examples/camera-calibration.json"),
    )
    .unwrap();
    std::fs::write(
        p.join("pose.json"),
        include_bytes!("../../../docs/examples/calibrated-sheet-pose.json"),
    )
    .unwrap();
    p
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn save(path: &Path, v: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}
fn invoke(dir: &Path, args: &[&str], code: i32) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "{args:?}: {} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn run(dir: &Path, args: &[&str], code: i32) -> Value {
    let mut args = args.to_vec();
    args.push("--json");
    let value: Value = serde_json::from_slice(&invoke(dir, &args, code).stdout).unwrap();
    assert_eq!(value["exit"], code);
    value
}
fn calibrate(dir: &Path) -> Value {
    run(
        dir,
        &[
            "measure",
            "calibrate-camera",
            "input.json",
            "--out",
            "calibration.json",
        ],
        0,
    )["result"]
        .clone()
}
const SHEET: &[&str] = &[
    "measure",
    "sheet-pose",
    "--reference",
    "pose.json",
    "--calibration",
    "calibration.json",
    "--lens-id",
    "synthetic-pinhole",
];

#[test]
fn documented_calibration_flows_into_sheet_pose_with_exact_source_receipts() {
    let dir = setup();
    let original = std::fs::read(dir.join("input.json")).unwrap();
    let report = calibrate(&dir);
    assert_eq!(report, read(&dir.join("calibration.json")));
    assert_eq!(report["input_sha256"], sha256_hex(&original));
    let k = &report["solution"]["intrinsics"];
    for (key, expected) in [("focal_px", [960., 1000.]), ("principal_px", [640., 480.])] {
        for (i, x) in expected.iter().enumerate() {
            assert!((k[key][i].as_f64().unwrap() - x).abs() < 1e-7);
        }
    }
    let mut args = SHEET.to_vec();
    args.extend(["--out", "results/camera.json"]);
    let result = run(&dir, &args, 0)["result"].clone();
    assert_eq!(result, read(&dir.join("results/camera.json")));
    assert_eq!(result["ok"], true);
    assert_eq!(result["camera"]["intrinsics"], *k);
    let source = &result["intrinsics_source"];
    assert_eq!(source["recomputed"], true);
    assert_eq!(source["input"], report["input"]);
    assert_eq!(
        source["report_sha256"],
        sha256_hex(&std::fs::read(dir.join("calibration.json")).unwrap())
    );
    assert_eq!(
        source["reference_input_sha256"],
        sha256_hex(&std::fs::read(dir.join("pose.json")).unwrap())
    );
    assert_eq!(original, std::fs::read(dir.join("input.json")).unwrap());
    assert_eq!(
        report,
        run(&dir, &["measure", "calibrate-camera", "input.json"], 0)["result"]
    );
    let human = invoke(&dir, &["measure", "calibrate-camera", "input.json"], 0);
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(
        text.contains("CAMERA CALIBRATION: ACCEPTED") && text.contains("not a shared world frame")
    );
}

#[test]
fn rejected_and_unobservable_calibrations_save_only_diagnostics() {
    for weak in [false, true] {
        let dir = setup();
        let mut input = read(&dir.join("input.json"));
        if weak {
            input["views"][0]["partition"] = json!("held_out");
        } else {
            input["views"][0]["corners"][1]["pixel_px"][0] = json!(900);
        }
        save(&dir.join("input.json"), &input);
        let r = run(
            &dir,
            &[
                "measure",
                "calibrate-camera",
                "input.json",
                "--out",
                "calibration.json",
            ],
            2,
        );
        assert!(r["result"]["solution"].is_null());
        assert_eq!(
            r["result"]["status"],
            if weak { "UNOBSERVABLE" } else { "REJECTED" }
        );
        assert_eq!(read(&dir.join("calibration.json")), r["result"]);
        let mut args = SHEET.to_vec();
        args.extend(["--out", "no-output/pose.json"]);
        run(&dir, &args, 3);
        assert!(!dir.join("no-output").exists());
    }
}

#[test]
fn handoff_rechecks_input_and_refuses_lens_geometry_and_inline_conflicts() {
    for kind in ["lens", "size", "rotation", "inline", "parameters", "source"] {
        let dir = setup();
        calibrate(&dir);
        let mut pose = read(&dir.join("pose.json"));
        let mut calibration = read(&dir.join("calibration.json"));
        match kind {
            "lens" => calibration["input"]["lens_id"] = json!("other-lens"),
            "size" => pose["image_size_px"] = json!([640, 480]),
            "rotation" => pose["rotation_applied_degrees"] = json!(90),
            "inline" => pose["intrinsics"] = calibration["solution"]["intrinsics"].clone(),
            "parameters" => calibration["solution"]["intrinsics"]["focal_px"][0] = json!(999),
            _ => calibration["input"]["views"][0]["corners"][1]["pixel_px"][0] = json!(900),
        }
        save(&dir.join("pose.json"), &pose);
        save(&dir.join("calibration.json"), &calibration);
        let mut args = SHEET.to_vec();
        args.extend(["--out", "no-output/result.json"]);
        run(&dir, &args, 3);
        assert!(!dir.join("no-output").exists(), "{kind}");
    }
}

#[test]
fn existing_evidence_and_its_aliases_are_never_overwritten() {
    let dir = setup();
    let original = std::fs::read(dir.join("input.json")).unwrap();
    run(
        &dir,
        &[
            "measure",
            "calibrate-camera",
            "input.json",
            "--out",
            "input.json",
        ],
        3,
    );
    assert_eq!(original, std::fs::read(dir.join("input.json")).unwrap());
    calibrate(&dir);
    let saved = std::fs::read(dir.join("calibration.json")).unwrap();
    run(
        &dir,
        &[
            "measure",
            "calibrate-camera",
            "input.json",
            "--out",
            "calibration.json",
        ],
        3,
    );
    let mut args = SHEET.to_vec();
    args.extend(["--out", "calibration.json"]);
    run(&dir, &args, 3);
    assert_eq!(saved, std::fs::read(dir.join("calibration.json")).unwrap());
    std::fs::hard_link(dir.join("input.json"), dir.join("alias.json")).unwrap();
    run(
        &dir,
        &[
            "measure",
            "calibrate-camera",
            "input.json",
            "--out",
            "alias.json",
        ],
        3,
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("input.json", dir.join("link.json")).unwrap();
        run(
            &dir,
            &[
                "measure",
                "calibrate-camera",
                "input.json",
                "--out",
                "link.json",
            ],
            3,
        );
    }
    assert_eq!(original, std::fs::read(dir.join("input.json")).unwrap());
}

#[test]
fn budgets_and_bad_schema_options_have_distinct_exit_codes_and_no_output() {
    let dir = setup();
    run(
        &dir,
        &[
            "measure",
            "calibrate-camera",
            "input.json",
            "--iterations",
            "50",
        ],
        3,
    );
    let mut input = read(&dir.join("input.json"));
    input["typo"] = json!(true);
    save(&dir.join("bad.json"), &input);
    run(
        &dir,
        &[
            "measure",
            "calibrate-camera",
            "bad.json",
            "--out",
            "absent/result.json",
        ],
        3,
    );
    assert!(!dir.join("absent").exists());
    let mut input = read(&dir.join("input.json"));
    input["views"] = json!(vec![input["views"][0].clone(); 33]);
    save(&dir.join("large.json"), &input);
    run(
        &dir,
        &[
            "measure",
            "calibrate-camera",
            "large.json",
            "--out",
            "absent/result.json",
        ],
        4,
    );
    let file = std::fs::File::create(dir.join("oversized.json")).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    run(
        &dir,
        &[
            "measure",
            "calibrate-camera",
            "oversized.json",
            "--out",
            "absent/result.json",
        ],
        4,
    );
    assert!(!dir.join("absent").exists());
}
