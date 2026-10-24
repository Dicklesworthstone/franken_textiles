//! Synthetic source-join exercise. Texture pixels test selection/identity, not body detection.
//! Body correspondences are authored projections; no real photograph or anatomical accuracy claim.

use ftx_core::{Vec2, Vec3, receipt::sha256_hex};
use ftx_measure::camera::Camera;
use ftx_trace::raster::image::{Gray, encode_png_gray};
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn command(dir: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["result"].clone()
}

pub(super) fn wrap(dir: &Path, mut request: Value) -> Value {
    // Reuse the independent analytical calibration input, with its original fit/held-out partitions.
    let calibration_input: Value = serde_json::from_slice(include_bytes!(
        "../../../../docs/examples/camera-calibration.json"
    ))
    .unwrap();
    write(
        &dir.join("capture-calibration-input.json"),
        &calibration_input,
    );
    let calibration = command(
        dir,
        &[
            "measure",
            "calibrate-camera",
            "capture-calibration-input.json",
            "--out",
            "capture-calibration.json",
        ],
    );
    let k = &calibration["solution"]["intrinsics"];
    let calibration_sha = sha256_hex(&std::fs::read(dir.join("capture-calibration.json")).unwrap());
    let mut video_frames = Vec::new();
    let mut fit_frames = Vec::new();
    for (i, frame) in request["fit"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let original: Camera = serde_json::from_value(frame["camera"].clone()).unwrap();
        let mut camera = original;
        camera.intrinsics = serde_json::from_value(k.clone()).unwrap();
        assert_eq!(camera.intrinsics.distortion.radial, [0.0; 3]);
        assert_eq!(camera.intrinsics.distortion.tangential, [0.0; 2]);
        let mut keypoints = frame["keypoints"].clone();
        // Change only the synthetic sensor model. Preserve each original calibrated ray,
        // body pose, correspondence identity and fit/held-out assignment.
        for point in keypoints.as_array_mut().unwrap() {
            let pixel: Vec2 = serde_json::from_value(point["pixel_px"].clone()).unwrap();
            let ray = original.intrinsics.bearing(pixel).unwrap();
            point["pixel_px"] = json!([
                camera.intrinsics.principal_px.x + camera.intrinsics.focal_px.x * ray.x / ray.z,
                camera.intrinsics.principal_px.y + camera.intrinsics.focal_px.y * ray.y / ray.z
            ]);
        }
        let w = frame["image_size_px"][0].as_u64().unwrap() as usize;
        let h = frame["image_size_px"][1].as_u64().unwrap() as usize;
        let mut image = Gray {
            w,
            h,
            data: (0..w * h)
                .map(|p| {
                    if (p / w + p % w).is_multiple_of(2) {
                        70
                    } else {
                        190
                    }
                })
                .collect(),
        };
        image.data[0] = i as u8;
        let image_name = format!("capture-frame-{i}.png");
        std::fs::write(dir.join(&image_name), encode_png_gray(&image)).unwrap();
        for (name, left) in [("person", true), ("sheet", false)] {
            let mask = Gray {
                w,
                h,
                data: (0..w * h)
                    .map(|p| if (p % w < w / 2) == left { 255 } else { 0 })
                    .collect(),
            };
            std::fs::write(
                dir.join(format!("capture-{name}-{i}.png")),
                encode_png_gray(&mask),
            )
            .unwrap();
        }
        video_frames.push(json!({"metadata":{"frame":frame["frame"],"rotation_cw_deg":0,
            "camera_epoch":"synthetic-checked-lens","focal_px":k["focal_px"]},"image":image_name,
            "person_mask":format!("capture-person-{i}.png"),"sheet_mask":format!("capture-sheet-{i}.png")}));
        let mut points = Vec::new();
        for z in [0.4, 0.7, 1.0] {
            for x in [-0.2, 0.0, 0.2] {
                points.push(json!({"sheet_xz_m":[x,z],"pixel_px":camera.project(Vec3::new(x,0.0,z)).unwrap(),"held_out":x==0.0}));
            }
        }
        let reference = format!("capture-reference-{i}.json");
        write(
            &dir.join(&reference),
            &json!({"schema":"ftx-sheet-pose-input/1","reference_id":"synthetic-fixed-sheet",
            "image_size_px":[w,h],"rotation_applied_degrees":0,"image":image_name,"points":points}),
        );
        fit_frames.push(json!({"source":null,"lens_id":"synthetic-pinhole","calibration":"capture-calibration.json",
            "calibration_sha256":calibration_sha,"reference":reference,"keypoints":keypoints}));
    }
    write(
        &dir.join("capture-frames.json"),
        &json!({"schema":"ftx-video-frames/1",
        "decode_receipt":"Synthetic texture buffers at authored timestamps; no decoder, detector or real-image proof.","frames":video_frames}),
    );
    let selection = command(
        dir,
        &[
            "measure",
            "video-select",
            "capture-frames.json",
            "--out",
            "capture-selection.json",
        ],
    );
    for (i, frame) in fit_frames.iter_mut().enumerate() {
        frame["source"] = selection["selected_sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["metadata"]["frame"] == request["fit"]["frames"][i]["frame"])
            .unwrap()
            .clone();
    }
    request["schema"] = json!("ftx-anny-capture-input/1");
    request["video_manifest"] = json!("capture-frames.json");
    request["reference_id"] = json!("synthetic-fixed-sheet");
    request["fit"]["frames"] = json!(fit_frames);
    request
}
