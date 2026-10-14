//! Synthetic PNG and JPEG pixels through detection, measurement and evidence-file protection.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ftx_core::receipt::sha256_hex;
use ftx_trace::raster::{
    Gray,
    image::{encode_png_bgr, encode_png_gray},
};
use serde_json::{Value, json};

fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ftx-photo-pixels-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    let mut image = Gray::filled(720, 960, 90);
    // An independently rasterized, nominal Letter rectangle at 1.5 pixels/mm.
    for y in 0..image.h {
        for x in 0..image.w {
            let dx =
                ((x as f64 + 0.5).min(150.2 + 215.9 * 1.5) - (x as f64 - 0.5).max(150.2)).max(0.0);
            let dy =
                ((y as f64 + 0.5).min(170.4 + 279.4 * 1.5) - (y as f64 - 0.5).max(170.4)).max(0.0);
            image.data[y * image.w + x] = (90.0 + 152.0 * dx * dy).round() as u8;
        }
    }
    std::fs::write(path.join("photo.png"), encode_png_gray(&image)).unwrap();
    std::fs::write(
        path.join("points.json"),
        br#"{"across_back":[[30,400],[630,400]]}"#,
    )
    .unwrap();
    path
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

#[test]
fn png_calibration_reaches_measurement_and_fusion_with_edge_uncertainty() {
    let dir = setup();
    let args = [
        "measure",
        "photo-scale",
        "--image",
        "photo.png",
        "--paper",
        "auto",
    ];
    let scale = run(&dir, &args, 0)["result"].clone();
    assert_eq!(scale["coordinate_source"], "detected_edges");
    assert_eq!(scale["paper"], "letter");
    assert_eq!(
        scale["image_input"]["sha256"],
        sha256_hex(&std::fs::read(dir.join("photo.png")).unwrap())
    );
    assert_eq!(scale["image_input"]["image_size_px"], json!([720, 960]));
    assert!(scale.get("reference_input").is_none());
    assert!((scale["mm_per_px"].as_f64().unwrap() - 2.0 / 3.0).abs() < 0.002);
    assert_eq!(scale, run(&dir, &args, 0)["result"]);
    let measured = run(
        &dir,
        &[
            "measure",
            "measure-photo",
            "--image",
            "photo.png",
            "--paper",
            "letter",
            "--points",
            "points.json",
            "--out",
            "measured.json",
        ],
        0,
    )["result"]
        .clone();
    let record = &measured["records"][0];
    assert!((record["value_mm"].as_f64().unwrap() - 400.0).abs() < 1.0);
    assert_eq!(record["provenance"], "PHOTO_ESTIMATED");
    assert!(record["sigma_mm"].as_f64().unwrap() > 0.0);
    assert!(
        measured["photo"]["bootstrap"]["method"]
            .as_str()
            .unwrap()
            .contains("edge points")
    );
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
}

#[test]
fn failed_detection_never_publishes_measurements() {
    let dir = setup();
    std::fs::write(
        dir.join("blank.png"),
        encode_png_gray(&Gray::filled(100, 100, 90)),
    )
    .unwrap();
    for image in ["blank.png", "photo.png"] {
        let scale = run(
            &dir,
            &["measure", "photo-scale", "--image", image, "--paper", "a4"],
            3,
        );
        assert_eq!(scale["result"]["ok"], false);
        assert!(scale["result"].get("homography_px_to_board_mm").is_none());
        run(
            &dir,
            &[
                "measure",
                "measure-photo",
                "--image",
                image,
                "--paper",
                "a4",
                "--points",
                "points.json",
                "--out",
                "no-result.json",
            ],
            3,
        );
        assert!(!dir.join("no-result.json").exists());
    }
}

#[test]
fn conflicting_sources_and_options_refuse_before_opening_files() {
    let dir = setup();
    for args in [
        vec!["--reference", "absent.json", "--image", "absent.png"],
        vec!["--reference", "absent.json", "--paper", "letter"],
        vec!["--image", "absent.png", "--paper", "legal"],
        vec![],
    ] {
        let mut command = vec!["measure", "photo-scale"];
        command.extend(args);
        assert_eq!(
            run(&dir, &command, 3)["refusal"]["code"],
            "invalid_document"
        );
    }
}

#[test]
fn output_cannot_overwrite_photo_or_prior_scale() {
    let dir = setup();
    let before = std::fs::read(dir.join("photo.png")).unwrap();
    run(
        &dir,
        &[
            "measure",
            "photo-scale",
            "--image",
            "photo.png",
            "--out",
            "photo.png",
        ],
        3,
    );
    assert_eq!(before, std::fs::read(dir.join("photo.png")).unwrap());
    std::fs::write(dir.join("scale.json"), b"prior evidence").unwrap();
    run(
        &dir,
        &[
            "measure",
            "photo-scale",
            "--image",
            "photo.png",
            "--out",
            "scale.json",
        ],
        3,
    );
    assert_eq!(
        std::fs::read(dir.join("scale.json")).unwrap(),
        b"prior evidence"
    );
}

#[test]
fn encoded_and_pixel_budgets_fail_before_decoding() {
    let dir = setup();
    let file = std::fs::File::create(dir.join("large.png")).unwrap();
    file.set_len(64 * 1024 * 1024 + 1).unwrap();
    let out = run(&dir, &["measure", "photo-scale", "--image", "large.png"], 4);
    assert_eq!(out["refusal"]["code"], "budget_exceeded");
    let mut bytes = std::fs::read(dir.join("photo.png")).unwrap();
    bytes[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
    std::fs::write(dir.join("huge.png"), bytes).unwrap();
    let out = run(&dir, &["measure", "photo-scale", "--image", "huge.png"], 4);
    assert_eq!(out["refusal"]["code"], "budget_exceeded");
}

#[test]
fn exif_alpha_and_animation_are_not_silently_treated_as_photo_pixels() {
    let dir = setup();
    let original = std::fs::read(dir.join("photo.png")).unwrap();
    for kind in [b"eXIf", b"tRNS", b"acTL"] {
        let mut bytes = original[..33].to_vec();
        bytes.extend([0; 4]);
        bytes.extend(kind);
        bytes.extend([0; 4]);
        bytes.extend(&original[33..]);
        std::fs::write(dir.join("metadata.png"), bytes).unwrap();
        let out = run(
            &dir,
            &["measure", "photo-scale", "--image", "metadata.png"],
            3,
        );
        assert_eq!(out["refusal"]["code"], "invalid_document");
    }
    let mut bytes = original;
    bytes[25] = 6;
    std::fs::write(dir.join("alpha.png"), bytes).unwrap();
    let out = run(&dir, &["measure", "photo-scale", "--image", "alpha.png"], 3);
    assert!(
        out["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("opaque 8-bit")
    );
}

#[test]
fn a_jpeg_is_measured_in_its_exif_orientation_as_its_pixels_are_as_png() {
    // A synthetic Letter sheet at 2 px/mm, displayed 600 x 800 and stored 800 x 600 with EXIF
    // Orientation 6, as a phone held upright writes it (Pillow 12.3, grayscale, quality 90).
    let jpeg = include_bytes!("../../../tests/fixtures/photo/plain-letter-exif6.jpg");
    let dir = setup();
    std::fs::write(dir.join("photo.jpg"), jpeg).unwrap();
    let photo = ftx_trace::raster::jpeg::decode_photo(jpeg, 16_000_000).unwrap();
    std::fs::write(
        dir.join("oriented.png"),
        encode_png_bgr(&photo.image).unwrap(),
    )
    .unwrap();
    let scale = |image: &str| {
        let mut result = run(
            &dir,
            &[
                "measure",
                "photo-scale",
                "--image",
                image,
                "--paper",
                "letter",
            ],
            0,
        )["result"]
            .clone();
        let receipt = result
            .as_object_mut()
            .unwrap()
            .remove("image_input")
            .unwrap();
        (result, receipt)
    };
    let (from_jpeg, receipt) = scale("photo.jpg");
    let (from_png, _) = scale("oriented.png");
    assert_eq!(from_jpeg, from_png);
    assert!((from_jpeg["mm_per_px"].as_f64().unwrap() - 0.5).abs() < 0.002);
    assert_eq!(receipt["encoding"], "jpeg");
    assert_eq!(receipt["exif_orientation"], 6);
    assert_eq!(receipt["image_size_px"], json!([600, 800]));
    assert_eq!(receipt["stored_size_px"], json!([800, 600]));
    assert_eq!(receipt["sha256"], sha256_hex(jpeg));
    // A cut file is refused, not decoded with libjpeg's repair.
    std::fs::write(dir.join("cut.jpg"), &jpeg[..jpeg.len() / 2]).unwrap();
    let out = run(&dir, &["measure", "photo-scale", "--image", "cut.jpg"], 3);
    assert_eq!(out["refusal"]["code"], "invalid_document");
}
