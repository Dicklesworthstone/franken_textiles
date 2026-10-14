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

fn lens_document() -> Value {
    json!({"schema":"ftx-photo-lens/1", "lens_id":"synthetic-brown",
        "image_size_px":[1000,800], "rotation_cw_deg":0,
        "calibration_evidence":"Synthetic analytic Brown lens; not a production camera calibration.",
        "intrinsics":{"focal_px":[700.0,650.0],"principal_px":[500.0,360.0],
            "distortion":{"radial":[0.25,0.015,0.0],"tangential":[0.004,-0.003],"max_radius":1.2}}})
}

fn lens_project([x, y]: [f64; 2]) -> [f64; 2] {
    // A physically rigid tilted plane under the declared K, not an arbitrary projective
    // warp that OpenCV's known-intrinsics pose interpolation must (correctly) reject.
    let (sx, cx) = (-0.15_f64).sin_cos();
    let (sy, cy) = 0.20_f64.sin_cos();
    let (x, y) = (x - 110.0, y - 90.0);
    let d = 250.0 - cx * sy * x + sx * y;
    let u = cy * x / d;
    let v = (sx * sy * x + cx * y) / d;
    let r2 = u * u + v * v;
    let radial = 1.0 + 0.25 * r2 + 0.015 * r2 * r2;
    [
        700.0 * (u * radial + 0.008 * u * v - 0.003 * (r2 + 2.0 * u * u)) + 500.0,
        650.0 * (v * radial + 0.004 * (r2 + 2.0 * v * v) - 0.006 * u * v) + 360.0,
    ]
}

fn write_json(dir: &Path, name: &str, value: &Value) {
    std::fs::write(dir.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}

fn lens_inputs(dir: &Path) {
    write_json(dir, "lens.json", &lens_document());
    let corners: Vec<_> = (0..35)
        .map(|id| {
            json!({"id":id,"point_px":lens_project([
        (id % 7 + 1) as f64 * 22.0,(id / 7 + 1) as f64 * 22.0])})
        })
        .collect();
    write_json(
        dir,
        "reference.json",
        &json!({"schema":"ftx-photo-reference/1","reference":"charuco",
        "image_size_px":[1000,800],"board":{"squares_x":8,"squares_y":6,"square_mm":22},"corners":corners}),
    );
    write_json(
        dir,
        "points.json",
        &json!({"across_back":[lens_project([22.0,66.0]),lens_project([154.0,66.0])],
        "off_board_length":[lens_project([10.0,150.0]),lens_project([210.0,150.0])]}),
    );
}

const LENS_FLAGS: [&str; 6] = [
    "--lens",
    "lens.json",
    "--lens-id",
    "synthetic-brown",
    "--rotation-cw-deg",
    "0",
];

#[test]
fn imported_lens_calibration_reaches_measurement_without_an_image_runtime() {
    let dir = setup();
    lens_inputs(&dir);
    let mut args = vec!["measure", "photo-scale", "--reference", "reference.json"];
    args.extend(LENS_FLAGS);
    let scale = run(&dir, &args, 0)["result"].clone();
    assert_eq!(scale["inliers"], 35);
    assert!(scale.get("homography_px_to_board_mm").is_none());
    assert!(scale["homography_undistorted_px_to_board_mm"].is_array());
    assert_eq!(
        scale["lens_input"]["sha256"],
        sha256_hex(&std::fs::read(dir.join("lens.json")).unwrap())
    );
    args[1] = "measure-photo";
    args.extend(["--points", "points.json", "--out", "measured.json"]);
    let measured = run(&dir, &args, 0)["result"].clone();
    assert_eq!(measured["records"][0]["value_mm"], 132.0);
    assert_eq!(measured["records"][1]["value_mm"], 200.0);
    assert_eq!(measured["photo"]["lens_input"], scale["lens_input"]);
    assert_eq!(
        measured["photo"]["intrinsics"],
        lens_document()["intrinsics"]
    );
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
    let original = std::fs::read(dir.join("lens.json")).unwrap();
    *args.last_mut().unwrap() = "lens.json";
    run(&dir, &args, 3);
    assert_eq!(original, std::fs::read(dir.join("lens.json")).unwrap());
}

#[test]
fn lens_identity_geometry_and_model_mismatches_are_atomic_refusals() {
    let dir = setup();
    lens_inputs(&dir);
    let mut args = vec!["measure", "measure-photo", "--reference", "reference.json"];
    args.extend(LENS_FLAGS);
    args.extend(["--points", "points.json", "--out", "not-created.json"]);
    for (field, value) in [
        ("schema", json!("ftx-camera-calibration/1")),
        ("lens_id", json!("another-lens")),
        ("rotation_cw_deg", json!(90)),
        ("image_size_px", json!([500, 400])),
        ("calibration_evidence", json!("")),
        (
            "intrinsics",
            json!({"focal_px":[700,650],"principal_px":[500,360],"distortion":{"radial":[-1,0,0]}}),
        ),
        (
            "intrinsics",
            json!({"focal_px":[700,650],"principal_px":[500,360],"distortion":{"max_radius":0.1}}),
        ),
        ("extra_field", json!(1)),
    ] {
        let mut lens = lens_document();
        lens[field] = value;
        write_json(&dir, "lens.json", &lens);
        run(&dir, &args, 3);
        assert!(!dir.join("not-created.json").exists());
    }
    write_json(&dir, "lens.json", &lens_document());
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.join("reference.json")).unwrap()).unwrap();
    for reference in [
        {
            let mut reference = original.clone();
            reference["intrinsics"] = lens_document()["intrinsics"].clone();
            reference
        },
        {
            let mut reference = original.clone();
            reference["reference"] = json!("plain_sheet");
            reference
        },
    ] {
        write_json(&dir, "reference.json", &reference);
        let refused = run(&dir, &args, 3);
        assert!(
            refused["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("without inline intrinsics")
        );
        assert!(!dir.join("not-created.json").exists());
    }
    write_json(&dir, "reference.json", &original);
    for extra in [
        vec!["--reference", "absent.json", "--lens-id", "synthetic-brown"],
        vec!["--reference", "absent.json", "--rotation-cw-deg", "0"],
        vec!["--reference", "absent.json", "--lens", "lens.json"],
        vec!["--image", "absent.png", "--lens", "lens.json"],
        vec![
            "--reference",
            "absent.json",
            "--lens",
            "lens.json",
            "--lens-id",
            "synthetic-brown",
            "--rotation-cw-deg",
            "45",
        ],
    ] {
        let mut command = vec!["measure", "photo-scale"];
        command.extend(extra);
        assert_eq!(
            run(&dir, &command, 3)["refusal"]["code"],
            "invalid_document"
        );
    }
}

#[test]
#[ignore = "requires optional OpenCV; CI runs this with --include-ignored"]
fn actual_brown_distorted_png_uses_one_lens_for_detection_scale_and_uncertainty() {
    let dir = setup();
    let py = python();
    lens_inputs(&dir);
    let metadata = sheet(&dir, "letter");
    let source =
        load_png(&std::fs::read(dir.join("sheet/calib-sheet-letter.png")).unwrap()).unwrap();
    let mut image = Gray::filled(1000, 800, 80);
    let (sx, cx) = (-0.15_f64).sin_cos();
    let (sy, cy) = 0.20_f64.sin_cos();
    for y in 0..image.h {
        for x in 0..image.w {
            let (xd, yd) = ((x as f64 - 500.0) / 700.0, (y as f64 - 360.0) / 650.0);
            let (mut u, mut v) = (xd, yd);
            // Independent fixed-point inverse for this synthetic lens, not the Rust
            // camera's Newton inverse and not an image produced by the detector.
            for _ in 0..45 {
                let r2 = u * u + v * v;
                let radial = 1.0 + 0.25 * r2 + 0.015 * r2 * r2;
                let tx = 0.008 * u * v - 0.003 * (r2 + 2.0 * u * u);
                let ty = 0.004 * (r2 + 2.0 * v * v) - 0.006 * u * v;
                u = (xd - tx) / radial;
                v = (yd - ty) / radial;
            }
            let (a, b, c, d) = (
                cy + u * cx * sy,
                -u * sx,
                sx * sy + v * cx * sy,
                cx - v * sx,
            );
            let determinant = a * d - b * c;
            let point = [
                110.0 + 250.0 * (u * d - b * v) / determinant,
                90.0 + 250.0 * (a * v - c * u) / determinant,
            ];
            if point[0] < 0.0 || point[0] >= 176.0 || point[1] < 0.0 || point[1] >= 132.0 {
                continue;
            }
            let p = raster_point(point, &metadata["board"]);
            let (ix, iy) = (p[0].floor() as usize, p[1].floor() as usize);
            let (dx, dy) = (p[0] - ix as f64, p[1] - iy as f64);
            let sample = |x, y| f64::from(source.at(x, y)[0]);
            let value = (1.0 - dy) * ((1.0 - dx) * sample(ix, iy) + dx * sample(ix + 1, iy))
                + dy * ((1.0 - dx) * sample(ix, iy + 1) + dx * sample(ix + 1, iy + 1));
            image.data[y * image.w + x] = value.round() as u8;
        }
    }
    std::fs::write(dir.join("distorted.png"), encode_png_gray(&image)).unwrap();
    let mut args = photo_args(
        "measure-photo",
        "distorted.png",
        "sheet/calib-sheet-letter.json",
        &py,
    );
    args.extend(LENS_FLAGS);
    args.extend(["--points", "points.json", "--out", "measured.json"]);
    let measured = run(&dir, &args, 0)["result"].clone();
    assert!(
        (measured["records"][0]["value_mm"].as_f64().unwrap() - 132.0).abs() < 0.4,
        "{measured}"
    );
    assert!(
        (measured["records"][1]["value_mm"].as_f64().unwrap() - 200.0).abs() < 0.5,
        "{measured}"
    );
    assert!(measured["photo"]["inliers"].as_u64().unwrap() >= 30);
    assert!(measured["photo"]["reprojection_rms_px"].as_f64().unwrap() < 0.6);
    assert_eq!(measured["photo"]["coordinate_source"], "detected_charuco");
    assert_eq!(
        measured["photo"]["image_input"]["charuco_detection"]["implementation"]["uses_lens_intrinsics"],
        true
    );
    assert_eq!(
        measured["photo"]["lens_input"]["sha256"],
        sha256_hex(&std::fs::read(dir.join("lens.json")).unwrap())
    );
    assert!(measured["photo"].get("homography_px_to_board_mm").is_none());
    run(&dir, &["measure", "validate", "measured.json"], 0);
    // A size-mismatched lens is refused before invoking even an unavailable detector.
    let mut wrong = lens_document();
    wrong["image_size_px"] = json!([500, 400]);
    write_json(&dir, "lens.json", &wrong);
    let mut args = photo_args(
        "measure-photo",
        "distorted.png",
        "sheet/calib-sheet-letter.json",
        "not-installed-python",
    );
    args.extend(LENS_FLAGS);
    args.extend(["--points", "points.json", "--out", "not-created.json"]);
    let refused = run(&dir, &args, 3);
    assert!(
        refused["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("image_size_px")
    );
    assert!(!dir.join("not-created.json").exists());
}
