//! Decoded synthetic PNG/JPEG frames through the real CLI. No private media or inferred stillness.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use ftx_trace::raster::image::{Gray, encode_png_gray};
use serde_json::{Value, json};

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-video-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(dir.join("video")).unwrap();
    for (name, person, sheet) in [
        ("blurred", 0, 80),
        ("uneven", 10, 40),
        ("balanced", 25, 25),
        ("blank", 0, 0),
    ] {
        let data = (0..32 * 16)
            .map(|i| {
                let amplitude = if i % 32 < 16 { person } else { sheet };
                if (i / 32 + i % 32) % 2 == 0 {
                    100 + amplitude
                } else {
                    100 - amplitude
                }
            })
            .collect();
        std::fs::write(
            dir.join(format!("video/{name}.png")),
            encode_png_gray(&Gray { w: 32, h: 16, data }),
        )
        .unwrap();
    }
    for (name, left) in [("person", true), ("sheet", false)] {
        let data = (0..32 * 16)
            .map(|i| if (i % 32 < 16) == left { 255 } else { 0 })
            .collect();
        std::fs::write(
            dir.join(format!("video/{name}.png")),
            encode_png_gray(&Gray { w: 32, h: 16, data }),
        )
        .unwrap();
    }
    let frames: Vec<_> = [(0, "blurred"), (33_333, "uneven"), (66_666, "balanced"), (150_000, "blank")].into_iter().map(|(timestamp, image)| json!({
        "metadata":{"frame":{"stream":"synthetic","timestamp_us":timestamp},"rotation_cw_deg":0,"camera_epoch":"synthetic-lens","focal_px":[800,800]},
        "image":format!("{image}.png"), "person_mask":"person.png", "sheet_mask":"sheet.png"
    })).collect();
    save(
        &dir.join("video/frames.json"),
        &json!({
            "schema":"ftx-video-frames/1","decode_receipt":"Synthetic rendered pixels and explicit microsecond presentation timestamps; no real capture.","frames":frames
        }),
    );
    dir
}

fn invoke(dir: &Path, extra: &[&str], json: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ftx"));
    command
        .current_dir(dir)
        .args(["measure", "video-select", "video/frames.json"])
        .args(extra);
    if json {
        command.arg("--json");
    }
    command.output().unwrap()
}

fn run(dir: &Path, extra: &[&str], code: i32) -> Value {
    let output = invoke(dir, extra, true);
    assert_eq!(
        output.status.code(),
        Some(code),
        "{} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["exit"], code);
    value
}

fn frame_jpeg(orientation: u8) -> Vec<u8> {
    // The existing synthetic paper photograph: stored 800 x 600 with a big-endian EXIF entry.
    // Only change its orientation declaration, never its compressed pixel data or source fixture.
    let mut bytes = include_bytes!("../../../tests/fixtures/photo/plain-letter-exif6.jpg").to_vec();
    let tag = [0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6];
    let at = bytes.windows(tag.len()).position(|p| p == tag).unwrap();
    bytes[at + 9] = orientation;
    bytes
}

fn jpeg_setup() -> PathBuf {
    let dir = setup();
    std::fs::write(dir.join("video/frame.jpg"), frame_jpeg(1)).unwrap();
    for (name, left) in [("person-wide", true), ("sheet-wide", false)] {
        let data = (0..800 * 600)
            .map(|i| if (i % 800 < 400) == left { 255 } else { 0 })
            .collect();
        std::fs::write(
            dir.join(format!("video/{name}.png")),
            encode_png_gray(&Gray {
                w: 800,
                h: 600,
                data,
            }),
        )
        .unwrap();
    }
    let mut manifest = read(&dir.join("video/frames.json"));
    manifest["frames"].as_array_mut().unwrap().truncate(1);
    manifest["frames"][0]["image"] = json!("frame.jpg");
    manifest["frames"][0]["person_mask"] = json!("person-wide.png");
    manifest["frames"][0]["sheet_mask"] = json!("sheet-wide.png");
    manifest["frames"][0]["metadata"]["rotation_cw_deg"] = json!(90);
    save(&dir.join("video/frames.json"), &manifest);
    dir
}

#[test]
fn jpeg_frames_reuse_strict_pixels_and_preserve_selection_and_source_files() {
    let dir = jpeg_setup();
    let paths = [
        "frames.json",
        "frame.jpg",
        "person-wide.png",
        "sheet-wide.png",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|p| std::fs::read(dir.join("video").join(p)).unwrap())
        .collect();
    let result = run(&dir, &["--out", "jpeg-selection.json"], 0)["result"].clone();
    assert_eq!(result, read(&dir.join("jpeg-selection.json")));
    assert_eq!(result["status"], "SELECTED");
    assert_eq!(
        result["selected_frames"],
        json!([{"stream":"synthetic","timestamp_us":0}])
    );
    let quality = &result["windows"][0]["candidates"][0]["quality"];
    assert_eq!(quality["image_size_px"], json!([800, 600]));
    assert_eq!(quality["metadata"]["rotation_cw_deg"], 90);
    for region in ["person", "sheet"] {
        assert!(quality[region]["laplacian_variance"].as_f64().unwrap() > 10.0);
    }
    for (path, bytes) in paths.iter().zip(before) {
        assert_eq!(std::fs::read(dir.join("video").join(path)).unwrap(), bytes);
    }
    let pixels = ftx_trace::raster::jpeg::decode_photo(&frame_jpeg(1), 40_000_000).unwrap();
    let gray = ftx_trace::raster::color::bgr_to_gray(&pixels.image);
    std::fs::write(dir.join("video/decoded.png"), encode_png_gray(&gray)).unwrap();
    let mut manifest = read(&dir.join("video/frames.json"));
    manifest["frames"][0]["image"] = json!("decoded.png");
    save(&dir.join("video/frames.json"), &manifest);
    assert_eq!(run(&dir, &[], 0)["result"]["windows"], result["windows"]);
    // Remove only the fixture's APP1 bytes in memory: an untagged JPEG has the same stored pixels.
    let bytes = frame_jpeg(1);
    assert_eq!(
        &bytes[20..30],
        &[0xff, 0xe1, 0, 34, b'E', b'x', b'i', b'f', 0, 0]
    );
    std::fs::write(
        dir.join("video/no-exif.jpg"),
        [&bytes[..20], &bytes[56..]].concat(),
    )
    .unwrap();
    manifest["frames"][0]["image"] = json!("no-exif.jpg");
    save(&dir.join("video/frames.json"), &manifest);
    assert_eq!(run(&dir, &[], 0)["result"]["windows"], result["windows"]);
    for path in [
        "video/frame.jpg",
        "video/person-wide.png",
        "jpeg-selection.json",
    ] {
        let before = std::fs::read(dir.join(path)).unwrap();
        run(&dir, &["--out", path], 3);
        assert_eq!(std::fs::read(dir.join(path)).unwrap(), before);
    }
}

#[test]
fn jpeg_frames_refuse_orientation_changes_corruption_and_non_png_masks() {
    let dir = jpeg_setup();
    let original = read(&dir.join("video/frames.json"));
    for orientation in [0, 2, 3, 4, 5, 6, 7, 8] {
        std::fs::write(dir.join("video/rotated.jpg"), frame_jpeg(orientation)).unwrap();
        let mut manifest = original.clone();
        manifest["frames"][0]["image"] = json!("rotated.jpg");
        save(&dir.join("video/frames.json"), &manifest);
        run(&dir, &["--out", "refused.json"], 3);
        assert!(!dir.join("refused.json").exists());
    }
    let bytes = frame_jpeg(1);
    for (name, bytes) in [
        ("cut.jpg", bytes[..bytes.len() / 2].to_vec()),
        ("trailing.jpg", [bytes.as_slice(), b"trailing"].concat()),
    ] {
        std::fs::write(dir.join("video").join(name), bytes).unwrap();
        let mut manifest = original.clone();
        manifest["frames"][0]["image"] = json!(name);
        save(&dir.join("video/frames.json"), &manifest);
        run(&dir, &["--out", "refused.json"], 3);
        assert!(!dir.join("refused.json").exists());
    }
    for (field, name) in [
        ("person_mask", "frame.jpg"),
        ("sheet_mask", "frame.jpg"),
        ("person_mask", "person.png"),
        ("sheet_mask", "sheet.png"),
    ] {
        let mut manifest = original.clone();
        manifest["frames"][0][field] = json!(name);
        save(&dir.join("video/frames.json"), &manifest);
        run(&dir, &["--out", "refused.json"], 3);
        assert!(!dir.join("refused.json").exists());
    }
}

#[test]
fn decoded_pngs_select_weaker_region_and_keep_unusable_windows() {
    let dir = setup();
    let result = run(&dir, &["--out", "reports/selection.json"], 2)["result"].clone();
    assert_eq!(result, read(&dir.join("reports/selection.json")));
    assert_eq!(result["schema"], "ftx-frame-selection/1");
    assert_eq!(result["status"], "PARTIAL");
    assert_eq!(
        result["selected_frames"],
        json!([{"stream":"synthetic","timestamp_us":66_666}])
    );
    assert_eq!(
        result["windows"][0]["candidates"][2]["quality"]["person"]["laplacian_variance"],
        40_000.0
    );
    assert_eq!(
        result["windows"][0]["candidates"][0]["rejected"],
        json!(["person: sharpness below threshold"])
    );
    assert!(result["windows"][1]["selected"].is_null());
    assert_eq!(result["input"]["options"]["window_us"], 150_000);
    assert!(
        result["no_claim"]
            .as_str()
            .unwrap()
            .contains("not still-window")
    );
    assert!(result.get("capture").is_none());
    let mut manifest = read(&dir.join("video/frames.json"));
    manifest["frames"].as_array_mut().unwrap().reverse();
    save(&dir.join("video/frames.json"), &manifest);
    assert_eq!(run(&dir, &[], 2)["result"], result);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.join("reports/selection.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn usable_capture_has_human_summary_without_claiming_motion_evidence() {
    let dir = setup();
    let mut manifest = read(&dir.join("video/frames.json"));
    manifest["frames"].as_array_mut().unwrap().pop();
    save(&dir.join("video/frames.json"), &manifest);
    assert_eq!(run(&dir, &[], 0)["result"]["status"], "SELECTED");
    let output = invoke(&dir, &[], false);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("selected 1 of 3 decoded frames"));
    assert!(text.contains("not still-window or synchronization evidence"));
}

#[test]
fn rectangular_regions_match_binary_masks_without_creating_mask_files() {
    let dir = setup();
    let original = read(&dir.join("video/frames.json"));
    let files = run(&dir, &[], 2)["result"].clone();
    let mut rectangles = original.clone();
    for frame in rectangles["frames"].as_array_mut().unwrap() {
        frame["person_mask"] = json!({"rect_px":[0,0,16,16]});
        frame["sheet_mask"] = json!({"rect_px":[16,0,16,16]});
    }
    save(&dir.join("video/frames.json"), &rectangles);
    let regions = run(&dir, &[], 2)["result"].clone();
    // The existing typed reader serializes focal lengths as f64 (800 -> 800.0).
    // Preserve that whole legacy representation, changing only the declared mask sources.
    let mut expected_frames = files["input"]["frames"].clone();
    for frame in expected_frames.as_array_mut().unwrap() {
        frame["person_mask"] = json!({"rect_px":[0,0,16,16]});
        frame["sheet_mask"] = json!({"rect_px":[16,0,16,16]});
    }
    assert_eq!(regions["input"]["frames"], expected_frames);
    let a = &files["selected_sources"][0];
    let b = &regions["selected_sources"][0];
    for key in [
        "image_sha256",
        "decoded_gray_sha256",
        "person_mask_sha256",
        "sheet_mask_sha256",
    ] {
        assert_eq!(a[key], b[key]);
    }
    assert!(a["person_mask"].is_string());
    assert!(a["person_mask_file_sha256"].is_string());
    assert!(b.get("person_mask_file_sha256").is_none());
    assert!(b.get("sheet_mask_file_sha256").is_none());
    assert_eq!(b["person_mask"], json!({"rect_px":[0,0,16,16]}));
    // Every score, rejection, candidate, selected identity and advisory must remain equal.
    let comparable = |mut report: Value| {
        for key in ["input", "selected_sources"] {
            report.as_object_mut().unwrap().remove(key);
        }
        report
    };
    assert_eq!(comparable(regions.clone()), comparable(files));
    rectangles["frames"][2]["sheet_mask"] = json!("sheet.png");
    save(&dir.join("video/frames.json"), &rectangles);
    let mixed = run(&dir, &[], 2)["result"].clone();
    assert_eq!(mixed["windows"], regions["windows"]);
    assert!(mixed["selected_sources"][0]["sheet_mask_file_sha256"].is_string());
    assert!(
        mixed["selected_sources"][0]
            .get("person_mask_file_sha256")
            .is_none()
    );
    assert_eq!(std::fs::read_dir(dir.join("video")).unwrap().count(), 7);
}

#[test]
fn rectangular_regions_refuse_invalid_bounds_and_keep_thin_regions_unusable() {
    let dir = setup();
    let original = read(&dir.join("video/frames.json"));
    for field in ["person_mask", "sheet_mask"] {
        for invalid in [
            json!({"rect_px":[0,0,0,16]}),
            json!({"rect_px":[0,0,16,0]}),
            json!({"rect_px":[17,0,16,16]}),
            json!({"rect_px":[0,1,16,16]}),
            json!({"rect_px":[u32::MAX,0,16,16]}),
            json!({"rect_px":[0,0,u32::MAX,16]}),
            json!({"rect_px":[-1,0,16,16]}),
            json!({"rect_px":[0.5,0,16,16]}),
            json!({"rect_px":[0,0,16]}),
            json!({"rect_px":[0,0,16,16],"path":"person.png"}),
            json!({}),
            Value::Null,
        ] {
            let mut manifest = original.clone();
            manifest["frames"][0][field] = invalid;
            save(&dir.join("video/frames.json"), &manifest);
            run(&dir, &["--out", "refused-region.json"], 3);
            assert!(!dir.join("refused-region.json").exists());
        }
    }
    let mut thin = original;
    for frame in thin["frames"].as_array_mut().unwrap() {
        frame["person_mask"] = json!({"rect_px":[0,0,2,16]});
    }
    save(&dir.join("video/frames.json"), &thin);
    let result = run(&dir, &[], 2)["result"].clone();
    assert_eq!(result["status"], "NO_USABLE_FRAMES");
    assert_eq!(result["selected_frames"], json!([]));
    for window in result["windows"].as_array().unwrap() {
        assert!(window["selected"].is_null());
        for candidate in window["candidates"].as_array().unwrap() {
            assert!(candidate["quality"]["person"]["laplacian_variance"].is_null());
        }
    }
}

#[test]
fn malformed_manifest_and_masks_publish_no_output() {
    let dir = setup();
    let original = read(&dir.join("video/frames.json"));
    std::fs::write(
        dir.join("video/wrong-size.png"),
        encode_png_gray(&Gray::filled(16, 32, 255)),
    )
    .unwrap();
    std::fs::write(
        dir.join("video/non-binary.png"),
        encode_png_gray(&Gray::filled(32, 16, 127)),
    )
    .unwrap();
    std::fs::write(dir.join("video/broken.png"), b"not a PNG").unwrap();
    let mut truncated = encode_png_gray(&Gray::filled(32, 16, 255));
    truncated.truncate(truncated.len() - 12);
    std::fs::write(dir.join("video/no-end.png"), truncated).unwrap();
    let mut trailing = encode_png_gray(&Gray::filled(32, 16, 255));
    trailing.push(0);
    std::fs::write(dir.join("video/trailing.png"), trailing).unwrap();
    let mut variants = Vec::new();
    for field in ["person_mask", "sheet_mask"] {
        for name in [
            "wrong-size.png",
            "non-binary.png",
            "broken.png",
            "missing.png",
            "no-end.png",
            "trailing.png",
        ] {
            let mut value = original.clone();
            value["frames"][0][field] = json!(name);
            variants.push(value);
        }
    }
    let mut duplicate = original.clone();
    duplicate["frames"][1]["metadata"]["frame"]["timestamp_us"] = json!(0);
    variants.push(duplicate);
    let mut no_receipt = original.clone();
    no_receipt["decode_receipt"] = json!("");
    variants.push(no_receipt);
    let mut wrong_units = original.clone();
    wrong_units["options"] = json!({"window_ms":150});
    variants.push(wrong_units);
    let mut bad_rotation = original;
    bad_rotation["frames"][0]["metadata"]["rotation_cw_deg"] = json!(45);
    variants.push(bad_rotation);
    for value in variants {
        save(&dir.join("video/frames.json"), &value);
        run(&dir, &["--out", "bad-output.json"], 3);
        assert!(!dir.join("bad-output.json").exists());
    }
}

#[test]
fn output_cannot_overwrite_manifest_images_or_previous_reports() {
    let dir = setup();
    for path in [
        "video/frames.json",
        "video/balanced.png",
        "video/person.png",
    ] {
        let before = std::fs::read(dir.join(path)).unwrap();
        run(&dir, &["--out", path], 3);
        assert_eq!(before, std::fs::read(dir.join(path)).unwrap());
    }
    run(&dir, &["--out", "selection.json"], 2);
    let before = std::fs::read(dir.join("selection.json")).unwrap();
    run(&dir, &["--out", "selection.json"], 3);
    assert_eq!(before, std::fs::read(dir.join("selection.json")).unwrap());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("video/person.png", dir.join("alias.json")).unwrap();
        let before = std::fs::read(dir.join("video/person.png")).unwrap();
        run(&dir, &["--out", "alias.json"], 3);
        assert_eq!(before, std::fs::read(dir.join("video/person.png")).unwrap());
    }
}
