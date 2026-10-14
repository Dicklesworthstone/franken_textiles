//! Actual PNG -> optional OpenCV -> Rust scale/uncertainty. No real photographs.
use ftx_core::receipt::sha256_hex;
use ftx_trace::raster::{
    Gray,
    image::{encode_png_gray, load_png},
};
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
        "ftx-charuco-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    dir
}
fn run(dir: &Path, args: &[&str], code: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
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
fn python() -> String {
    std::env::var("FTX_DETECTOR_PYTHON").unwrap_or_else(|_| "python3".into())
}
fn sheet(dir: &Path, paper: &str) -> Value {
    run(
        dir,
        &["measure", "calib-sheet", "--paper", paper, "--out", "sheet"],
        0,
    );
    serde_json::from_slice(
        &std::fs::read(dir.join(format!("sheet/calib-sheet-{paper}.json"))).unwrap(),
    )
    .unwrap()
}
fn photo_args<'a>(
    command: &'a str,
    image: &'a str,
    sidecar: &'a str,
    python: &'a str,
) -> Vec<&'a str> {
    vec![
        "measure",
        command,
        "--image",
        image,
        "--sheet",
        sidecar,
        "--detector-python",
        python,
    ]
}
fn raster_point(p: [f64; 2], board: &Value) -> [f64; 2] {
    std::array::from_fn(|k| {
        board["origin_px"][k].as_f64().unwrap() - 0.5
            + p[k] * board["size_px"][k].as_f64().unwrap() / board["size_mm"][k].as_f64().unwrap()
    })
}

#[test]
fn charuco_option_conflicts_refuse_before_opening_files() {
    let dir = setup();
    for extra in [
        vec!["--sheet", "absent.json"],
        vec!["--sheet", "absent.json", "--reference", "absent.json"],
        vec![
            "--image",
            "absent.png",
            "--sheet",
            "absent.json",
            "--paper",
            "letter",
        ],
        vec![
            "--image",
            "absent.png",
            "--detector-python",
            "absent-python",
        ],
    ] {
        let mut args = vec!["measure", "photo-scale"];
        args.extend(extra);
        assert_eq!(run(&dir, &args, 3)["refusal"]["code"], "invalid_document");
    }
}

#[test]
fn unavailable_backend_refuses_without_publishing_or_falling_back() {
    let dir = setup();
    std::fs::write(
        dir.join("photo.png"),
        encode_png_gray(&Gray::filled(100, 100, 255)),
    )
    .unwrap();
    std::fs::write(
        dir.join("sheet.json"),
        br#"{"schema":"fabric-calib-sheet/1","board":{"type":"charuco"}}"#,
    )
    .unwrap();
    let mut args = photo_args(
        "measure-photo",
        "photo.png",
        "sheet.json",
        "ftx-deliberately-absent-python-19",
    );
    args.extend(["--points", "absent.json", "--out", "not-created.json"]);
    let report = run(&dir, &args, 3);
    assert_eq!(report["refusal"]["code"], "unscaled");
    assert!(
        report["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("cannot start ChArUco detector")
    );
    assert!(!dir.join("not-created.json").exists());
}

#[test]
#[ignore = "requires optional OpenCV; CI runs this with --include-ignored"]
fn generated_letter_and_a4_reach_measurement_and_fusion_with_receipts() {
    let py = python();
    for (paper, total) in [("letter", 35), ("a4", 42)] {
        let dir = setup();
        let sidecar = sheet(&dir, paper);
        let image = format!("sheet/calib-sheet-{paper}.png");
        let metadata = format!("sheet/calib-sheet-{paper}.json");
        let args = photo_args("photo-scale", &image, &metadata, &py);
        let scale = run(&dir, &args, 0)["result"].clone();
        assert_eq!(scale["coordinate_source"], "detected_charuco");
        assert_eq!(scale["reference"], "charuco");
        assert_eq!(scale["inliers"], total);
        assert!((scale["mm_per_px"].as_f64().unwrap() - 25.4 / 300.0).abs() < 0.0001);
        assert!(scale["blur_sigma_px_estimate"].as_f64().unwrap() < 1.0);
        assert_eq!(
            scale["image_input"]["sha256"],
            sha256_hex(&std::fs::read(dir.join(&image)).unwrap())
        );
        let detection = &scale["image_input"]["charuco_detection"];
        assert_eq!(detection["sheet"], sidecar);
        assert_eq!(detection["implementation"]["backend"], "opencv_charuco");
        assert_eq!(
            detection["reference_input"]["corners"]
                .as_array()
                .unwrap()
                .len(),
            total as usize
        );
        assert_eq!(scale, run(&dir, &args, 0)["result"]);
        // Independent printer pixel-edge geometry, not detector-derived clicks.
        let points = json!({"across_back":[raster_point([44.0,44.0], &sidecar["board"]), raster_point([132.0,44.0], &sidecar["board"])]});
        std::fs::write(
            dir.join("points.json"),
            serde_json::to_vec(&points).unwrap(),
        )
        .unwrap();
        let mut args = photo_args("measure-photo", &image, &metadata, &py);
        args.extend([
            "--points",
            "points.json",
            "--depth-offset-mm",
            "50",
            "--camera-distance-mm",
            "2500",
            "--out",
            "measured.json",
        ]);
        let measured = run(&dir, &args, 0)["result"].clone();
        let record = &measured["records"][0];
        assert!(
            (record["value_mm"].as_f64().unwrap() - 88.0).abs() <= 0.2,
            "{record}"
        );
        assert_eq!(record["provenance"], "PHOTO_ESTIMATED");
        assert!((record["sigma_terms_mm"]["depth_offset"].as_f64().unwrap() - 1.76).abs() < 0.01);
        assert!(record["sigma_terms_mm"]["click"].as_f64().unwrap() > 0.0);
        assert!(record["bootstrap_accepted"].as_u64().unwrap() >= 160);
        assert_eq!(measured["photo"]["coordinate_source"], "detected_charuco");
        assert_eq!(measured["photo"]["image_input"], scale["image_input"]);
        assert!(measured["taken_on"].is_null());
        run(&dir, &["measure", "validate", "measured.json"], 0);
        let fused = run(
            &dir,
            &["measure", "fuse", "measured.json", "--no-priors"],
            0,
        );
        assert_eq!(
            fused["result"]["fused"]["across_back"]["primary_provenance"],
            "PHOTO_ESTIMATED"
        );
        let before = std::fs::read(dir.join(&metadata)).unwrap();
        let mut args = photo_args("photo-scale", &image, &metadata, &py);
        args.extend(["--out", &metadata]);
        run(&dir, &args, 3);
        assert_eq!(before, std::fs::read(dir.join(&metadata)).unwrap());
    }
}

// Independent analytic projective camera (board mm -> photo pixels).
fn project([x, y]: [f64; 2]) -> [f64; 2] {
    let d = 1.0 + 0.0006 * x - 0.0003 * y;
    [
        (3.8 * x + 0.25 * y + 100.0) / d,
        (0.18 * x + 3.9 * y + 80.0) / d,
    ]
}

#[test]
#[ignore = "requires optional OpenCV; CI runs this with --include-ignored"]
fn perspective_blur_and_occlusion_preserve_ids_and_metric_lengths() {
    let dir = setup();
    let py = python();
    let sidecar = sheet(&dir, "a4");
    let src = load_png(&std::fs::read(dir.join("sheet/calib-sheet-a4.png")).unwrap()).unwrap();
    let mut image = Gray::filled(900, 800, 80);
    for y in 0..image.h {
        for x in 0..image.w {
            let (u, v) = (x as f64, y as f64);
            let (a, b, c, d) = (
                3.8 - 0.0006 * u,
                0.25 + 0.0003 * u,
                0.18 - 0.0006 * v,
                3.9 + 0.0003 * v,
            );
            let det = a * d - b * c;
            let p = [
                ((u - 100.0) * d - b * (v - 80.0)) / det,
                (a * (v - 80.0) - c * (u - 100.0)) / det,
            ];
            if p[0] < 50.0 || p[0] >= 176.0 || p[1] < 0.0 || p[1] >= 154.0 {
                continue;
            }
            let p = raster_point(p, &sidecar["board"]);
            let (ix, iy) = (p[0].floor() as usize, p[1].floor() as usize);
            let (dx, dy) = (p[0] - ix as f64, p[1] - iy as f64);
            let value = |x, y| f64::from(src.at(x, y)[0]);
            let v = (1.0 - dy) * ((1.0 - dx) * value(ix, iy) + dx * value(ix + 1, iy))
                + dy * ((1.0 - dx) * value(ix, iy + 1) + dx * value(ix + 1, iy + 1));
            image.data[y * image.w + x] = (v * 0.75 + 25.0).round() as u8;
        }
    }
    // Independent separable binomial blur, sigma=1 pixel.
    for horizontal in [true, false] {
        let previous = image.clone();
        for y in 2..image.h - 2 {
            for x in 2..image.w - 2 {
                let sum: u32 = [1, 4, 6, 4, 1]
                    .iter()
                    .enumerate()
                    .map(|(i, k)| {
                        let p = if horizontal {
                            previous.at(x + i - 2, y)
                        } else {
                            previous.at(x, y + i - 2)
                        };
                        k * u32::from(p)
                    })
                    .sum();
                image.data[y * image.w + x] = ((sum + 8) / 16) as u8;
            }
        }
    }
    std::fs::write(dir.join("warped.png"), encode_png_gray(&image)).unwrap();
    let points = json!({"across_back":[project([66.0, 66.0]), project([154.0, 66.0])]});
    std::fs::write(
        dir.join("points.json"),
        serde_json::to_vec(&points).unwrap(),
    )
    .unwrap();
    let mut args = photo_args(
        "measure-photo",
        "warped.png",
        "sheet/calib-sheet-a4.json",
        &py,
    );
    args.extend(["--points", "points.json"]);
    let measured = run(&dir, &args, 0)["result"].clone();
    assert!(
        (measured["records"][0]["value_mm"].as_f64().unwrap() - 88.0).abs() <= 0.3,
        "{measured}"
    );
    assert!(measured["photo"]["inliers"].as_u64().unwrap() >= 18);
    assert!(measured["photo"]["corners_detected"].as_u64().unwrap() < 42);
    assert!(measured["photo"]["reprojection_rms_px"].as_f64().unwrap() < 0.8);
    assert!(
        measured["photo"]["blur_sigma_px_estimate"]
            .as_f64()
            .unwrap()
            > 0.4
    );
    assert_eq!(measured, run(&dir, &args, 0)["result"]);
}

#[test]
#[ignore = "requires optional OpenCV; CI runs this with --include-ignored"]
fn missing_markers_and_bad_sidecars_never_create_measurements() {
    let dir = setup();
    let py = python();
    let sidecar = sheet(&dir, "letter");
    let mut blank = Gray::filled(720, 960, 80);
    for y in 170..589 {
        blank.data[y * 720 + 150..y * 720 + 474].fill(255);
    }
    std::fs::write(dir.join("plain.png"), encode_png_gray(&blank)).unwrap();
    assert_eq!(
        run(
            &dir,
            &[
                "measure",
                "photo-scale",
                "--image",
                "plain.png",
                "--paper",
                "letter"
            ],
            0
        )["result"]["reference"],
        "plain_sheet"
    );
    let scale = run(
        &dir,
        &photo_args(
            "photo-scale",
            "plain.png",
            "sheet/calib-sheet-letter.json",
            &py,
        ),
        3,
    );
    assert_eq!(scale["result"]["ok"], false);
    assert!(scale["result"].get("homography_px_to_board_mm").is_none());
    std::fs::write(
        dir.join("points.json"),
        br#"{"length":[[200,300],[400,300]]}"#,
    )
    .unwrap();
    let mut args = photo_args(
        "measure-photo",
        "plain.png",
        "sheet/calib-sheet-letter.json",
        &py,
    );
    args.extend(["--points", "points.json", "--out", "not-created.json"]);
    assert_eq!(run(&dir, &args, 3)["refusal"]["code"], "unscaled");
    assert!(!dir.join("not-created.json").exists());
    let mut wrong = sidecar;
    wrong["board"]["dictionary"] = json!("DICT_5X5_50");
    std::fs::write(dir.join("wrong.json"), serde_json::to_vec(&wrong).unwrap()).unwrap();
    let report = run(
        &dir,
        &photo_args("photo-scale", "plain.png", "wrong.json", &py),
        3,
    );
    assert!(
        report["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("unsupported dictionary")
    );
}
