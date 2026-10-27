//! Reviewed synthetic polygons through the real CLI; no inferred segmentation or private media.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use ftx_core::receipt::sha256_hex;
use ftx_trace::raster::image::{Gray, encode_png_gray};
use serde_json::{Value, json};

const EPOCH: u64 = 9_007_199_254_740_993;

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn cli(dir: &Path, command: &str, input: &str, out: Option<&str>, exit: i32) -> Value {
    let mut command_line = Command::new(env!("CARGO_BIN_EXE_ftx"));
    command_line
        .current_dir(dir)
        .args(["measure", command, input, "--json"]);
    if let Some(out) = out {
        command_line.args(["--out", out]);
    }
    let output = command_line.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["exit"], exit);
    envelope["result"].clone()
}

fn png(data: Vec<u8>) -> Vec<u8> {
    encode_png_gray(&Gray { w: 32, h: 32, data })
}

fn setup() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("ftx-video-masks-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(dir.join("video")).unwrap();
    for (name, person, sheet) in [("blurred", 0, 80), ("sharp", 25, 25)] {
        let data = (0usize..32 * 32)
            .map(|i| {
                let amplitude = if i % 32 < 16 { person } else { sheet };
                if (i / 32 + i % 32).is_multiple_of(2) {
                    100 + amplitude
                } else {
                    100 - amplitude
                }
            })
            .collect();
        std::fs::write(dir.join(format!("video/{name}.png")), png(data)).unwrap();
    }
    let frames: Vec<_> = [(EPOCH + 33_333, "sharp"), (EPOCH, "blurred")].into_iter()
        .map(|(timestamp_us, name)| json!({
            "metadata":{"frame":{"stream":"synthetic","timestamp_us":timestamp_us},
                "rotation_cw_deg":90,"camera_epoch":"declared-lens","focal_px":[801.25,800.5]},
            "image":format!("{name}.png"),"person_mask":"absent-person.png","sheet_mask":"absent-sheet.png"
        })).collect();
    save(
        &dir.join("video/frames.json"),
        &json!({"schema":"ftx-video-frames/1",
        "decode_receipt":"Synthetic stored pixels, explicit source clock and already-applied rotation.",
        "options":{"window_us":150000,"min_interior_pixels":64,"min_person_variance":7.5,
            "min_sheet_variance":5,"max_focal_change_ratio":0.02},"frames":frames}),
    );
    dir
}

fn rectangle(x0: u32, y0: u32, x1: u32, y1: u32) -> Value {
    json!({"outer":[[x0,y0],[x1,y0],[x1,y1],[x0,y1]],"holes":[]})
}

fn review(dir: &Path) -> Value {
    cli(
        dir,
        "video-mask-review",
        "video/frames.json",
        Some("review"),
        0,
    );
    read(&dir.join("review/annotations.json"))
}

fn author(annotations: &mut Value) {
    for frame in annotations["frames"].as_array_mut().unwrap() {
        frame["person_regions"] = json!([rectangle(0, 0, 15, 31)]);
        frame["sheet_regions"] = json!([rectangle(16, 0, 31, 31)]);
    }
}

fn materialize(dir: &Path, annotations: &Value, out: &str, exit: i32) {
    save(&dir.join("edited.json"), annotations);
    cli(dir, "video-masks", "edited.json", Some(out), exit);
    if exit != 0 {
        assert!(!dir.join(out).join("masks.json").exists());
        assert!(!dir.join(out).join("frames.json").exists());
    }
}

#[test]
fn reviewed_masks_preserve_exact_sources_clocks_and_ordinary_selection() {
    let dir = setup();
    cli(&dir, "video-select", "video/frames.json", None, 3);
    let original = read(&dir.join("video/frames.json"));
    let mut annotations = review(&dir);
    assert_eq!(annotations["schema"], "ftx-video-mask-polygons/1");
    assert_eq!(
        annotations["source_manifest"],
        json!(std::fs::canonicalize(dir.join("video/frames.json")).unwrap())
    );
    assert_eq!(
        annotations["source_manifest_sha256"],
        sha256_hex(&std::fs::read(dir.join("video/frames.json")).unwrap())
    );
    for file in ["index.html", "editor.js", "review.json"] {
        assert!(dir.join("review").join(file).is_file());
    }
    for (i, name) in ["blurred", "sharp"].into_iter().enumerate() {
        let frame = &annotations["frames"][i];
        assert_eq!(frame["index"], i);
        assert_eq!(frame["image_size_px"], json!([32, 32]));
        assert!(frame["person_regions"].is_null() && frame["sheet_regions"].is_null());
        assert!(frame.get("metadata").is_none() && frame.get("timestamp_us").is_none());
        let bytes = std::fs::read(dir.join(format!("video/{name}.png"))).unwrap();
        assert_eq!(frame["image_sha256"], sha256_hex(&bytes));
        assert_eq!(
            std::fs::read(dir.join(format!("review/source-{i:04}.png"))).unwrap(),
            bytes
        );
    }
    cli(
        &dir,
        "video-mask-review",
        "video/frames.json",
        Some("review"),
        3,
    );
    author(&mut annotations);
    materialize(&dir, &annotations, "masked", 0);
    let manifest = read(&dir.join("masked/frames.json"));
    assert_eq!(manifest["frames"].as_array().unwrap().len(), 2);
    for field in ["options", "decode_receipt"] {
        assert_eq!(manifest[field], original[field]);
    }
    for (i, name) in ["blurred", "sharp"].into_iter().enumerate() {
        assert_eq!(
            manifest["frames"][i]["metadata"],
            original["frames"][1 - i]["metadata"]
        );
        assert_eq!(
            std::fs::read(dir.join(format!("masked/source-{i:04}.png"))).unwrap(),
            std::fs::read(dir.join(format!("video/{name}.png"))).unwrap()
        );
    }
    // Independent masks built directly from pixel membership, not the polygon rasterizer.
    for (region, left) in [("person", true), ("sheet", false)] {
        let expected = png((0..32 * 32)
            .map(|i| if (i % 32 < 16) == left { 255 } else { 0 })
            .collect());
        std::fs::write(dir.join(format!("video/absent-{region}.png")), &expected).unwrap();
        for i in 0..2 {
            assert_eq!(
                std::fs::read(dir.join(format!("masked/{region}-{i:04}.png"))).unwrap(),
                expected
            );
        }
    }
    let selected = cli(&dir, "video-select", "masked/frames.json", None, 0);
    let direct = cli(&dir, "video-select", "video/frames.json", None, 0);
    assert_eq!(selected["windows"], direct["windows"]);
    assert_eq!(
        selected["selected_frames"],
        json!([{"stream":"synthetic","timestamp_us":EPOCH + 33_333}])
    );
    for field in [
        "metadata",
        "image_size_px",
        "image_sha256",
        "decoded_gray_sha256",
        "person_mask_file_sha256",
        "sheet_mask_file_sha256",
        "person_mask_sha256",
        "sheet_mask_sha256",
    ] {
        assert_eq!(
            selected["selected_sources"][0][field],
            direct["selected_sources"][0][field]
        );
    }
    let before = std::fs::read(dir.join("masked/masks.json")).unwrap();
    cli(&dir, "video-masks", "edited.json", Some("masked"), 3);
    assert_eq!(
        std::fs::read(dir.join("masked/masks.json")).unwrap(),
        before
    );
    assert_eq!(read(&dir.join("video/frames.json")), original);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["review", "masked"] {
            let output = dir.join(name);
            assert_eq!(
                std::fs::metadata(&output).unwrap().permissions().mode() & 0o777,
                0o700
            );
            for entry in std::fs::read_dir(output).unwrap() {
                assert_eq!(
                    entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
    }
}

#[test]
fn rectangle_sources_materialize_equivalent_pixels_with_new_file_evidence() {
    let dir = setup();
    let path = dir.join("video/frames.json");
    let mut original = read(&path);
    for frame in original["frames"].as_array_mut().unwrap() {
        frame["person_mask"] = json!({"rect_px":[0,0,16,32]});
        frame["sheet_mask"] = json!({"rect_px":[16,0,16,32]});
    }
    save(&path, &original);
    let direct = cli(&dir, "video-select", "video/frames.json", None, 0);
    let mut annotations = review(&dir);
    // Existing rectangles do not silently count as a newly reviewed polygon document.
    assert!(annotations["frames"][0]["person_regions"].is_null());
    author(&mut annotations);
    materialize(&dir, &annotations, "masked", 0);
    assert_eq!(
        std::fs::read(dir.join("masked/source-manifest.json")).unwrap(),
        std::fs::read(&path).unwrap()
    );
    let selected = cli(&dir, "video-select", "masked/frames.json", None, 0);
    assert_eq!(selected["windows"], direct["windows"]);
    assert_eq!(selected["selected_frames"], direct["selected_frames"]);
    let before = &direct["selected_sources"][0];
    let after = &selected["selected_sources"][0];
    for field in [
        "metadata",
        "image_size_px",
        "image_sha256",
        "decoded_gray_sha256",
        "person_mask_sha256",
        "sheet_mask_sha256",
    ] {
        assert_eq!(after[field], before[field]);
    }
    for field in ["person_mask", "sheet_mask"] {
        assert!(before[field].is_object());
        assert!(after[field].is_string());
        let hash = format!("{field}_file_sha256");
        assert!(before.get(&hash).is_none());
        assert!(after[&hash].is_string());
    }
    // The existing capture replay sees different source evidence even for equal mask pixels.
    assert_ne!(before, after);
    original["frames"][0]["person_mask"]["rect_px"][2] = json!(15);
    save(&path, &original);
    materialize(&dir, &annotations, "changed-region", 3);
}

#[test]
fn unreviewed_stale_or_invalid_annotations_cannot_publish_masks() {
    let dir = setup();
    let mut annotations = review(&dir);
    materialize(&dir, &annotations, "unreviewed", 3);
    author(&mut annotations);
    for (name, pointer, value) in [
        ("duplicate", "/frames/0/index", json!(1)),
        (
            "image-hash",
            "/frames/0/image_sha256",
            json!("0".repeat(64)),
        ),
        (
            "gray-hash",
            "/frames/0/decoded_gray_sha256",
            json!("0".repeat(64)),
        ),
        ("size", "/frames/0/image_size_px", json!([31, 32])),
        ("unreviewed-person", "/frames/0/person_regions", Value::Null),
        (
            "fractional",
            "/frames/0/person_regions/0/outer/0/0",
            json!(0.5),
        ),
        ("outside", "/frames/0/person_regions/0/outer/0/0", json!(32)),
        (
            "repeated-point",
            "/frames/0/person_regions/0/outer",
            json!([[0, 0], [15, 0], [15, 31], [0, 0]]),
        ),
        (
            "crossed",
            "/frames/0/person_regions/0/outer",
            json!([[0, 0], [15, 31], [15, 0], [0, 20]]),
        ),
    ] {
        let mut invalid = annotations.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        materialize(&dir, &invalid, name, 3);
    }
    let mut absent = annotations.clone();
    for frame in absent["frames"].as_array_mut().unwrap() {
        frame["person_regions"] = json!([]);
    }
    materialize(&dir, &absent, "not-visible", 0);
    assert_eq!(
        std::fs::read(dir.join("not-visible/person-0000.png")).unwrap(),
        png(vec![0; 32 * 32])
    );
    let unavailable = cli(&dir, "video-select", "not-visible/frames.json", None, 2);
    assert_eq!(unavailable["status"], "NO_USABLE_FRAMES");
    assert_eq!(unavailable["selected_frames"], json!([]));
    let image_path = dir.join("video/sharp.png");
    let original_bytes = std::fs::read(&image_path).unwrap();
    std::fs::write(&image_path, png(vec![127; 32 * 32])).unwrap();
    materialize(&dir, &annotations, "stale-image", 3);
    std::fs::write(&image_path, original_bytes).unwrap();
    let mut manifest = read(&dir.join("video/frames.json"));
    manifest["decode_receipt"] = json!("Changed acquisition declaration after annotation.");
    save(&dir.join("video/frames.json"), &manifest);
    materialize(&dir, &annotations, "stale-manifest", 3);
}

#[test]
fn component_union_and_holes_are_exact_and_order_independent() {
    let dir = setup();
    let mut annotations = review(&dir);
    author(&mut annotations);
    let mut component = rectangle(2, 2, 26, 26);
    component["holes"] = json!([[[8, 8], [20, 8], [20, 20], [8, 20]]]);
    for frame in annotations["frames"].as_array_mut().unwrap() {
        frame["person_regions"] = json!([component, rectangle(12, 12, 24, 24)]);
    }
    materialize(&dir, &annotations, "union", 0);
    let expected = png((0..32 * 32)
        .map(|i| {
            let (x, y) = (i % 32, i / 32);
            let square = |lo, hi| (lo..=hi).contains(&x) && (lo..=hi).contains(&y);
            if (square(2, 26) && !square(8, 20)) || square(12, 24) {
                255
            } else {
                0
            }
        })
        .collect());
    for frame in annotations["frames"].as_array_mut().unwrap() {
        frame["person_regions"].as_array_mut().unwrap().reverse();
    }
    materialize(&dir, &annotations, "reversed", 0);
    for name in ["union", "reversed"] {
        for i in 0..2 {
            assert_eq!(
                std::fs::read(dir.join(format!("{name}/person-{i:04}.png"))).unwrap(),
                expected
            );
        }
    }
}
