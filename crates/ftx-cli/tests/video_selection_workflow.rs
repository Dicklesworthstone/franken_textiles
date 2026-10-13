//! Decoded synthetic PNG frames through the real CLI. No private media or inferred stillness.

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
