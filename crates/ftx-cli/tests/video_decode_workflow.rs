//! Actual external decoding into the ordinary native selection workflow. Synthetic local media only.

use std::{path::Path, process::Command};

use ftx_trace::raster::image::{Gray, encode_png_gray};
use serde_json::{Value, json};

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn cli(dir: &Path, command: &str, input: &str, extra: &[&str], exit: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(["measure", command, input])
        .args(extra)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
#[ignore = "requires locally installed FFmpeg and ffprobe; run with --include-ignored"]
fn real_variable_rate_decode_preserves_pixels_clocks_and_selection() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("ftx-video-decode-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    for (case, epoch, rotation) in [("vfr%03d", 0, 90), ("large-clock", 3_600_000, 270)] {
        let dir = root.join(case);
        std::fs::create_dir(&dir).unwrap();
        let filter = format!("setpts={epoch}+if(eq(N\\,0)\\,0\\,if(eq(N\\,1)\\,17\\,70))");
        let encoded = Command::new("ffmpeg")
            .current_dir(&dir)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-n",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=64x48:rate=1000",
                "-frames:v",
                "3",
                "-vf",
                &filter,
                "-fps_mode",
                "passthrough",
                "-enc_time_base",
                "1:1000",
                "-c:v",
                "ffv1",
                "-threads",
                "1",
                "clip with spaces.mkv",
            ])
            .output()
            .unwrap();
        assert!(
            encoded.status.success(),
            "{}",
            String::from_utf8_lossy(&encoded.stderr)
        );
        let source = std::fs::read(dir.join("clip with spaces.mkv")).unwrap();
        let request = json!({"schema":"ftx-video-decode/1","video":"clip with spaces.mkv",
            "stream":"synthetic-camera","camera_epoch":"fixed-lens","rotation_cw_deg":rotation,
            "focal_px":[800,800],"options":{"window_us":150000,"min_interior_pixels":64,
                "min_person_variance":1,"min_sheet_variance":1,"max_focal_change_ratio":0.01}});
        save(&dir.join("decode-input.json"), &request);
        let result = cli(
            &dir,
            "video-decode",
            "decode-input.json",
            &["--out", "decoded"],
            0,
        )["result"]
            .clone();
        assert_eq!(result["status"], "DECODED_REQUIRES_MASKS");
        assert_eq!(result["frames"].as_array().unwrap().len(), 3);
        assert_eq!(
            result["frames"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["timestamp_us"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [
                epoch as u64 * 1000,
                (epoch as u64 + 17) * 1000,
                (epoch as u64 + 70) * 1000
            ]
        );
        assert_eq!(
            std::fs::read(dir.join("decoded/source.media")).unwrap(),
            source
        );
        assert_eq!(
            std::fs::read(dir.join("clip with spaces.mkv")).unwrap(),
            source
        );
        assert_eq!(
            std::fs::read_dir(dir.join("decoded/masks"))
                .unwrap()
                .count(),
            0
        );
        // Decoding alone cannot silently create usable body/sheet evidence.
        cli(&dir, "video-select", "decoded/frames.json", &[], 3);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(dir.join("decoded/frames.json")).unwrap())
                .unwrap();
        for frame in manifest["frames"].as_array().unwrap() {
            assert_eq!(frame["metadata"]["rotation_cw_deg"], rotation);
            for (field, left) in [("person_mask", true), ("sheet_mask", false)] {
                let mask = Gray {
                    w: 48,
                    h: 64,
                    data: (0..48 * 64)
                        .map(|i| if (i % 48 < 24) == left { 255 } else { 0 })
                        .collect(),
                };
                std::fs::write(
                    dir.join("decoded").join(frame[field].as_str().unwrap()),
                    encode_png_gray(&mask),
                )
                .unwrap();
            }
        }
        let selection = cli(&dir, "video-select", "decoded/frames.json", &[], 0)["result"].clone();
        assert_eq!(selection["status"], "SELECTED");
        let selected = &selection["selected_sources"][0];
        assert_eq!(selected["image_size_px"], json!([48, 64]));
        let image = selected["image"].as_str().unwrap();
        let decoded = result["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["image"] == image)
            .unwrap();
        assert_eq!(selected["image_sha256"], decoded["image_sha256"]);
        assert_eq!(
            selected["decoded_gray_sha256"],
            decoded["decoded_gray_sha256"]
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.join("decoded"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            for file in ["source.media", "decode.json", "frames.json", image] {
                assert_eq!(
                    std::fs::metadata(dir.join("decoded").join(file))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
        }
        let before = std::fs::read(dir.join("decoded/decode.json")).unwrap();
        cli(
            &dir,
            "video-decode",
            "decode-input.json",
            &["--out", "decoded"],
            3,
        );
        assert_eq!(
            std::fs::read(dir.join("decoded/decode.json")).unwrap(),
            before
        );
        let mut limited = request.clone();
        limited["max_frames"] = json!(2);
        save(&dir.join("limited.json"), &limited);
        cli(
            &dir,
            "video-decode",
            "limited.json",
            &["--out", "limited"],
            4,
        );
        assert!(!dir.join("limited/frames.json").exists());
        assert!(!dir.join("limited/decode.json").exists());
        assert_eq!(
            std::fs::read_dir(dir.join("limited/frames"))
                .unwrap()
                .count(),
            0
        );
        let mut invalid = request.clone();
        invalid["rotation_cw_deg"] = json!(45);
        save(&dir.join("invalid.json"), &invalid);
        cli(
            &dir,
            "video-decode",
            "invalid.json",
            &["--out", "invalid"],
            3,
        );
        assert!(!dir.join("invalid").exists());
    }
}
