//! Original synthetic photographs reach portable calibration and the ordinary body-fit editors.
//! Rendered ChArUco pixels test camera/source admission. Authored body points are not anatomy proof.

use super::*;
use ftx_wasm::body_fit::{
    BodyCapture, BodyFitImage, body_capture_catalogue, body_fit_observations, body_fit_starts,
    body_fit_targets, fit_body,
};

fn sources() -> (PathBuf, Vec<u8>, Value) {
    let dir = setup();
    let (model, mut truth) = fixture_scaled(0.25);
    std::fs::write(dir.join("model.ftxbody"), &model).unwrap();
    capture::wrap_images(&dir, &mut truth, true);
    (dir, model, truth)
}

fn manifest() -> Value {
    json!({
        "schema": "ftx-body-photo-capture/1",
        "images": ["capture-frame-0.png", "capture-frame-1.png"],
        "lens_id": "synthetic-pinhole",
        "rotation_cw_deg": 0,
        "reference_id": "synthetic-fixed-sheet",
        "held_out_corner_ids": [0, 6, 35, 41],
        "board_bottom_left_xz_m": [0.15, 0.10],
        "stationary_upright_evidence": "Synthetic stationary upright board rendered from independent print geometry."
    })
}

fn choices(truth: &Value) -> Value {
    let fit = |p: &Value| json!({"name":p["name"],"choice":{"mode":"fit","lower":p["lower"],"upper":p["upper"]}});
    let pose = truth["model"]["pose_parameters"].as_array().unwrap();
    json!({
        "schema": "ftx-body-photo-choices/1",
        "model_sha256": truth["model"]["model_sha256"],
        "model": {
            "age_years": truth["model"]["age_years"],
            "gender": truth["model"]["gender"],
            "shapes": truth["model"]["shape_parameters"].as_array().unwrap().iter().map(fit).collect::<Vec<_>>(),
            "roots": pose[..6].iter().map(fit).collect::<Vec<_>>(),
            "joints": [{
                "bone": "arm", "axis": [1.0, 0.0, 0.0],
                "choice": {"mode":"fit","lower":pose[6]["lower"],"upper":pose[6]["upper"]}
            }]
        },
        "metric_scale_source": truth["fit"]["metric_scale_source"],
        "observation_evidence": truth["fit"]["observation_evidence"],
        "measurements": truth["fit"]["measurements"]
    })
}

fn input(report: &Value) -> Value {
    let text = report["input"].as_str().unwrap();
    assert_eq!(report["input_sha256"], sha256_hex(text.as_bytes()));
    serde_json::from_str(text).unwrap()
}

fn brown_pixel(x: f64, y: f64) -> [f64; 2] {
    let r2 = x * x + y * y;
    let radial = 1.0 + 0.12 * r2 - 0.035 * r2 * r2 + 0.009 * r2 * r2 * r2;
    [
        1050.0 * (x * radial + 0.004 * x * y - 0.0015 * (r2 + 2.0 * x * x)) + 790.0,
        1000.0 * (y * radial + 0.002 * (r2 + 2.0 * y * y) - 0.003 * x * y) + 610.0,
    ]
}

fn brown_calibration() -> Value {
    use ftx_measure::photo::intrinsics::{CalibrationInput, calibrate_camera};

    // Independent polynomial observations, as in the existing Brown calibration known answer.
    // The capture images below are separate raster acquisitions, never calibration views.
    let controls: Vec<_> = (0..48)
        .map(|i| {
            json!({"id":format!("p{i:02}"),
                "sheet_xz_m":[(i % 8) as f64 * 0.06 - 0.21, (i / 8) as f64 * 0.06 - 0.15]})
        })
        .collect();
    let base = Mat3::from_rows([[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]);
    let poses = [
        (0.35, 0.12, -0.18, -0.10, 0.80),
        (-0.45, 0.3, 0.16, 0.13, 0.75),
        (0.2, -0.55, -0.12, 0.18, 0.85),
        (-0.3, -0.25, 0.23, -0.14, 0.8),
        (0.5, -0.4, 0.03, -0.04, 0.66),
        (-0.2, 0.5, -0.24, 0.06, 0.9),
    ];
    let views: Vec<_> = poses
        .iter()
        .enumerate()
        .map(|(i, &(a, b, tx, ty, tz))| {
            let rotation = Mat3::rotation(Vec3::new(1.0, 0.0, 0.0), a)
                * Mat3::rotation(Vec3::new(0.0, 1.0, 0.0), b)
                * base;
            let corners: Vec<_> = controls
                .iter()
                .enumerate()
                .map(|(j, point)| {
                    let q = rotation.mul_vec(Vec3::new(
                        point["sheet_xz_m"][0].as_f64().unwrap(),
                        0.0,
                        point["sheet_xz_m"][1].as_f64().unwrap(),
                    )) + Vec3::new(tx, ty, tz);
                    json!({"id":point["id"],"pixel_px":brown_pixel(q.x / q.z, q.y / q.z),
                        "partition":if [0,7,40,47].contains(&j) {"held_out"} else {"fit"}})
                })
                .collect();
            json!({"frame":{"stream":"independent-brown-calibration","timestamp_us":i * 100000},
                "partition":if i == 5 {"held_out"} else {"fit"},"corners":corners})
        })
        .collect();
    let input: CalibrationInput = serde_json::from_value(json!({
        "schema":"ftx-camera-calibration-input/1","image_model":"brown5_zero_skew",
        "lens_id":"synthetic-brown","pinhole_evidence":"",
        "distortion_fit":{"model_evidence":"Independent polynomial lens; synthetic test only",
            "max_radius":0.9,"max_iterations":80},
        "reference_evidence":"Synthetic 60 mm planar grid","image_size_px":[1600,1200],
        "rotation_cw_deg":0,"controls":controls,"views":views,
    }))
    .unwrap();
    let report = calibrate_camera(&input).unwrap();
    assert_eq!(report.status, "ACCEPTED");
    json!(report)
}

fn brown_capture_images(sheet: &Value, sheet_png: &[u8]) -> Vec<Vec<u8>> {
    let page = ftx_trace::raster::image::load_png(sheet_png).unwrap();
    let board = &sheet["board"];
    let width_m = board["size_mm"][0].as_f64().unwrap() / 1000.0;
    let height_m = board["size_mm"][1].as_f64().unwrap() / 1000.0;
    let mut image = Gray::filled(1600, 1200, 80);
    // The known camera is at (0,-1,0.1875), facing the stationary y=0 board. A fixed-point
    // inverse of the authored polynomial is independent of the candidate Newton inverse.
    // This generous raster region includes the entire known board and its white surround.
    for y in 250..900 {
        for x in 850..1450 {
            let (xd, yd) = ((x as f64 - 790.0) / 1050.0, (y as f64 - 610.0) / 1000.0);
            let (mut u, mut v) = (xd, yd);
            for _ in 0..35 {
                let r2 = u * u + v * v;
                let radial = 1.0 + 0.12 * r2 - 0.035 * r2 * r2 + 0.009 * r2 * r2 * r2;
                u = (xd - 0.004 * u * v + 0.0015 * (r2 + 2.0 * u * u)) / radial;
                v = (yd - 0.002 * (r2 + 2.0 * v * v) + 0.003 * u * v) / radial;
            }
            let lx = u - 0.15;
            let ly = 0.10 + height_m - (0.1875 - v);
            image.data[y * image.w + x] =
                if (0.0..width_m).contains(&lx) && (0.0..height_m).contains(&ly) {
                    let px = (board["origin_px"][0].as_f64().unwrap()
                        + lx / width_m * board["size_px"][0].as_f64().unwrap())
                    .floor() as usize;
                    let py = (board["origin_px"][1].as_f64().unwrap()
                        + ly / height_m * board["size_px"][1].as_f64().unwrap())
                    .floor() as usize;
                    page.data[(py * page.w + px) * 3]
                } else {
                    255
                };
        }
    }
    (0..2)
        .map(|index| {
            // Distinct original exposures; no camera or body information is inferred here.
            for y in 590..610 {
                for x in 290..310 {
                    image.data[y * image.w + x] = if index == 0 { 0 } else { 200 };
                }
            }
            encode_png_gray(&image)
        })
        .collect()
}

#[test]
fn portable_photos_construct_source_bound_drafts_and_continue_through_fit_and_check() {
    let (dir, model, truth) = sources();
    let calibration = std::fs::read_to_string(dir.join("capture-calibration.json")).unwrap();
    let sheet = std::fs::read_to_string(dir.join("capture-board.json")).unwrap();
    let manifest = manifest();
    let manifest_text = manifest.to_string();
    let choices = choices(&truth);
    let choices_text = choices.to_string();
    let catalogue = body_capture_catalogue(&model).unwrap();
    assert_eq!(catalogue["schema"], "ftx-body-capture-catalogue/1");
    assert_eq!(catalogue["model_sha256"], sha256_hex(&model));
    assert_eq!(catalogue["catalogue"]["roots"].as_array().unwrap().len(), 6);

    let mut capture = BodyCapture::new(&manifest_text, &calibration, &sheet).unwrap();
    assert_eq!(
        capture.image_ids(),
        vec!["capture-frame-0.png", "capture-frame-1.png"]
    );
    let images: Vec<_> = capture
        .image_ids()
        .iter()
        .map(|name| std::fs::read(dir.join(name)).unwrap())
        .collect();
    for (index, name) in capture.image_ids().iter().enumerate() {
        capture.add_image(name, &images[index]).unwrap();
    }
    let prepared = capture.finish(&model, &choices_text).unwrap();
    assert_eq!(prepared, capture.finish(&model, &choices_text).unwrap());
    assert_eq!(prepared["body_fitted"], false);
    assert_eq!(prepared["status"], "CAPTURE_DRAFT");
    assert_eq!(prepared["frame_count"], 2);
    assert_eq!(prepared["start_count"], 0);
    assert_eq!(prepared["model_sha256"], sha256_hex(&model));
    assert_eq!(prepared["choices"], choices);
    assert_eq!(
        prepared["choices_sha256"],
        sha256_hex(choices_text.as_bytes())
    );
    let acquisition = &prepared["acquisition"];
    assert_eq!(acquisition["manifest"], manifest);
    assert_eq!(
        acquisition["manifest_sha256"],
        sha256_hex(manifest_text.as_bytes())
    );
    assert_eq!(
        acquisition["calibration_sha256"],
        sha256_hex(calibration.as_bytes())
    );
    assert_eq!(acquisition["sheet_sha256"], sha256_hex(sheet.as_bytes()));
    let draft = input(&prepared);
    assert_eq!(draft["schema"], "ftx-anny-body-fit-input/1");
    assert_eq!(draft["model"]["keypoints"], json!({}));
    assert_eq!(draft["fit"]["starts"], json!([]));
    assert_eq!(draft["fit"]["measurements"], truth["fit"]["measurements"]);
    assert_eq!(draft["capture_images"].as_array().unwrap().len(), 2);
    for (index, bytes) in images.iter().enumerate() {
        let image_hash = sha256_hex(bytes);
        let frame = &draft["fit"]["frames"][index];
        let source = &draft["capture_images"][index];
        assert_eq!(
            frame["frame"],
            json!({"stream":format!("still-image:{image_hash}"),"timestamp_us":0})
        );
        assert_eq!(frame["keypoints"], json!([]));
        assert_eq!(source["frame"], frame["frame"]);
        assert_eq!(source["image_name"], manifest["images"][index]);
        assert_eq!(source["image_sha256"], image_hash);
        assert_eq!(source["image_size_px"], json!([1280, 960]));
        assert_eq!(source["rotation_cw_deg"], 0);
        let camera: Camera = serde_json::from_value(frame["camera"].clone()).unwrap();
        assert!((camera.center_m - Vec3::new(0.0, -1.0, 0.1875)).norm() < 0.04);
        let acquired = &acquisition["frames"][index];
        assert_eq!(acquired["image_sha256"], source["image_sha256"]);
        assert_eq!(acquired["sheet_pose"]["ok"], true);
        assert_eq!(acquired["sheet_pose"]["held_out_points"], 4);
        assert_eq!(acquired["detection"]["detector"]["backend"], "rust_charuco");
    }

    let draft_text = prepared["input"].as_str().unwrap();
    let picker = body_fit_targets(&model, draft_text, "").unwrap();
    assert_eq!(picker["target_count"], 0);
    assert_eq!(picker["input"], draft_text);
    let targets = body_fit_targets(
        &model,
        draft_text,
        &json!({
            "input_sha256": picker["input_sha256"],
            "model_sha256": picker["model_sha256"],
            "keypoints": truth["model"]["keypoints"]
        })
        .to_string(),
    )
    .unwrap();
    let target_text = targets["input"].as_str().unwrap();
    let review = body_fit_observations(&model, target_text, "").unwrap();
    for (index, frame) in review["frames"].as_array().unwrap().iter().enumerate() {
        let receipt = frame["image_review"].as_str().unwrap();
        let image = BodyFitImage::new(&images[index], receipt).unwrap();
        assert_eq!(
            image.report()["source_image"],
            draft["capture_images"][index]
        );
        assert!(BodyFitImage::new(&images[1 - index], receipt).is_err());
    }
    let observations: Vec<_> = truth["fit"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(frame_index, frame)| {
            json!({"frame_index":frame_index,"keypoints":frame["keypoints"]})
        })
        .collect();
    let observed = body_fit_observations(
        &model,
        target_text,
        &json!({
            "input_sha256": targets["input_sha256"],
            "model_sha256": targets["model_sha256"],
            "frames": observations
        })
        .to_string(),
    )
    .unwrap();
    let starts: Vec<_> = truth["fit"]["starts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|start| {
            let poses: Vec<_> = start["poses"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(frame_index, pose)| {
                    json!({"frame_index":frame_index,"values":pose["values"]})
                })
                .collect();
            json!({"shape":start["shape"],"poses":poses})
        })
        .collect();
    let initialized = body_fit_starts(
        &model,
        observed["input"].as_str().unwrap(),
        &json!({
            "input_sha256": observed["input_sha256"],
            "model_sha256": observed["model_sha256"],
            "starts": starts
        })
        .to_string(),
    )
    .unwrap();
    let ready = input(&initialized);
    assert_eq!(ready["capture_images"], draft["capture_images"]);
    assert_eq!(ready["fit"]["frames"], input(&observed)["fit"]["frames"]);
    assert_eq!(ready["model"]["keypoints"], truth["model"]["keypoints"]);
    save(&dir, &ready);
    let native = run(
        &dir,
        &["--iterations", "50", "--max-evaluations", "100000"],
        0,
    );
    let fitted = fit_body(
        &model,
        initialized["input"].as_str().unwrap(),
        "50",
        "100000",
    )
    .unwrap();
    assert_eq!(fitted, native["result"]);
    assert_eq!(fitted["status"], "ACCEPTED");

    // A deliberately wrong independent CHECK point must reject acceptance without affecting
    // the optimization trajectory; its role and uncertainty remain those authored above.
    let mut bad_observations = observations;
    let held_out = &mut bad_observations[0]["keypoints"][3];
    assert_eq!(held_out["partition"], "held_out");
    held_out["pixel_px"][0] = json!(held_out["pixel_px"][0].as_f64().unwrap() + 10.0);
    let rejected_input = body_fit_observations(
        &model,
        initialized["input"].as_str().unwrap(),
        &json!({
            "input_sha256": initialized["input_sha256"],
            "model_sha256": initialized["model_sha256"],
            "frames": bad_observations
        })
        .to_string(),
    )
    .unwrap();
    let rejected = fit_body(
        &model,
        rejected_input["input"].as_str().unwrap(),
        "50",
        "100000",
    )
    .unwrap();
    assert_eq!(rejected["status"], "REJECTED");
    assert!(rejected["solution"].is_null());
    for (accepted, refused) in fitted["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .zip(rejected["attempts"].as_array().unwrap())
    {
        assert_eq!(accepted["cost_history"], refused["cost_history"]);
    }
}

#[test]
fn portable_photos_refuse_duplicate_pixels_stale_sources_bad_controls_and_missing_check() {
    let (dir, model, truth) = sources();
    let calibration = std::fs::read_to_string(dir.join("capture-calibration.json")).unwrap();
    let sheet = std::fs::read_to_string(dir.join("capture-board.json")).unwrap();
    let manifest = manifest();
    let choices = choices(&truth);
    let images: Vec<_> = (0..2)
        .map(|index| std::fs::read(dir.join(format!("capture-frame-{index}.png"))).unwrap())
        .collect();
    let mut capture = BodyCapture::new(&manifest.to_string(), &calibration, &sheet).unwrap();
    assert!(capture.finish(&model, &choices.to_string()).is_err());
    assert!(capture.add_image("unlisted.png", &images[0]).is_err());
    capture
        .add_image("capture-frame-0.png", &images[0])
        .unwrap();
    assert!(
        capture
            .add_image("capture-frame-0.png", &images[0])
            .is_err()
    );
    let decoded = ftx_trace::raster::image::load_png(&images[0]).unwrap();
    let reencoded = ftx_trace::raster::image::encode_png_bgr(&decoded).unwrap();
    assert_ne!(sha256_hex(&reencoded), sha256_hex(&images[0]));
    assert!(
        capture
            .add_image("capture-frame-1.png", &reencoded)
            .is_err()
    );
    assert!(capture.finish(&model, &choices.to_string()).is_err());
    capture
        .add_image("capture-frame-1.png", &images[1])
        .unwrap();
    let original = capture.finish(&model, &choices.to_string()).unwrap();

    let mut stale_model = model.clone();
    *stale_model.last_mut().unwrap() ^= 1;
    assert!(capture.finish(&stale_model, &choices.to_string()).is_err());
    let mut bad = choices.clone();
    bad["model_sha256"] = json!("0".repeat(64));
    assert!(capture.finish(&model, &bad.to_string()).is_err());
    let mut bad = choices.clone();
    bad["model"]["shapes"][0]["choice"]["lower"] = json!(1.0);
    assert!(capture.finish(&model, &bad.to_string()).is_err());
    let mut bad = choices.clone();
    bad["model"]["roots"].as_array_mut().unwrap().pop();
    assert!(capture.finish(&model, &bad.to_string()).is_err());
    let mut bad = choices.clone();
    bad["model"]["joints"][0]["axis"] = json!([0.0, 0.0, 0.0]);
    assert!(capture.finish(&model, &bad.to_string()).is_err());
    assert_eq!(
        capture.finish(&model, &choices.to_string()).unwrap(),
        original
    );

    let mut bad = manifest.clone();
    bad["images"] = json!(["capture-frame-0.png", "capture-frame-0.png"]);
    assert!(BodyCapture::new(&bad.to_string(), &calibration, &sheet).is_err());
    let mut bad = manifest.clone();
    bad["lens_id"] = json!("unmatched-lens");
    assert!(BodyCapture::new(&bad.to_string(), &calibration, &sheet).is_err());
    let mut bad = manifest.clone();
    bad["rotation_cw_deg"] = json!(45);
    assert!(BodyCapture::new(&bad.to_string(), &calibration, &sheet).is_err());
    let mut bad = manifest.clone();
    bad["held_out_corner_ids"] = json!([0, 6, 35]);
    assert!(BodyCapture::new(&bad.to_string(), &calibration, &sheet).is_err());

    // This existing pixel erasure removes both supports of predeclared CHECK corner 0.
    // A fresh capture must refuse the missing observation, not merely stale encoded bytes.
    let mut occluded = ftx_trace::raster::image::load_png(&images[0]).unwrap();
    for y in 412..459 {
        for x in 782..829 {
            let offset = (y * occluded.w + x) * 3;
            occluded.data[offset..offset + 3].fill(255);
        }
    }
    let missing_check = ftx_trace::raster::image::encode_png_bgr(&occluded).unwrap();
    let mut fresh = BodyCapture::new(&manifest.to_string(), &calibration, &sheet).unwrap();
    let error = fresh
        .add_image("capture-frame-0.png", &missing_check)
        .unwrap_err();
    assert!(error.message.contains("every predeclared CHECK"), "{error}");
    assert!(fresh.finish(&model, &choices.to_string()).is_err());
    fresh.add_image("capture-frame-0.png", &images[0]).unwrap();
    fresh.add_image("capture-frame-1.png", &images[1]).unwrap();
    assert_eq!(
        fresh.finish(&model, &choices.to_string()).unwrap(),
        original
    );
}

fn known_silhouette(points: &[Vec2], at: f64) -> Vec<[f64; 2]> {
    // Direct intersections of the synthetic fixture's authored triangles with one horizontal
    // line. This test does not call the production silhouette projection or coverage merger.
    let mut segments = Vec::new();
    for triangle in [
        [0, 1, 2],
        [1, 3, 2],
        [1, 8, 9],
        [4, 6, 7],
        [4, 7, 5],
        [0, 4, 5],
        [0, 5, 1],
        [2, 3, 7],
        [2, 7, 6],
        [0, 2, 6],
        [0, 6, 4],
        [1, 5, 7],
        [1, 7, 3],
    ] {
        let mut crossings = Vec::new();
        for edge in 0..3 {
            let a = points[triangle[edge]];
            let b = points[triangle[(edge + 1) % 3]];
            assert_ne!(a.y, at);
            if (a.y < at) != (b.y < at) {
                crossings.push((b.x * (at - a.y) + a.x * (b.y - at)) / (b.y - a.y));
            }
        }
        if crossings.len() == 2 {
            crossings.sort_by(f64::total_cmp);
            segments.push([crossings[0], crossings[1]]);
        }
    }
    segments.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut intervals: Vec<[f64; 2]> = Vec::new();
    for [lo, hi] in segments {
        if let Some(last) = intervals.last_mut()
            && lo <= last[1] + 1e-9
        {
            last[1] = last[1].max(hi);
        } else {
            intervals.push([lo, hi]);
        }
    }
    assert!(!intervals.is_empty());
    intervals
}

#[test]
fn brown_photo_rectification_preserves_source_poses_and_enables_silhouette_body_fitting() {
    let dir = setup();
    let (model, truth) = fixture_scaled(0.25);
    std::fs::write(dir.join("model.ftxbody"), &model).unwrap();
    let calibration = brown_calibration().to_string();
    let sheet = ftx_trace::calibration_sheet::render(
        "a4",
        22.0,
        16.0,
        ftx_trace::calibration_sheet::DEFAULT_DICTIONARY,
    )
    .unwrap();
    let images = brown_capture_images(&sheet.sidecar, &sheet.png);
    let sheet_text = sheet.sidecar_json();
    let sheet_text = std::str::from_utf8(&sheet_text).unwrap();
    let mut original_manifest = manifest();
    original_manifest["images"] = json!(["raw-0.png", "raw-1.png"]);
    original_manifest["lens_id"] = json!("synthetic-brown");
    let mut manifest = original_manifest.clone();
    manifest["rectify"] = json!(true);
    let choices = choices(&truth);
    let choices_text = choices.to_string();
    let mut raw =
        BodyCapture::new(&original_manifest.to_string(), &calibration, sheet_text).unwrap();
    let mut rectified = BodyCapture::new(&manifest.to_string(), &calibration, sheet_text).unwrap();
    assert!(rectified.take_image().is_empty());
    let mut derived = Vec::new();
    for (index, name) in rectified.image_ids().iter().enumerate() {
        let original = raw.add_image(name, &images[index]).unwrap();
        let result = rectified.add_image(name, &images[index]).unwrap();
        assert_eq!(result["sheet_pose"], original["sheet_pose"]);
        assert_eq!(result["detection"], original["detection"]);
        assert_eq!(result["image_sha256"], sha256_hex(&images[index]));
        assert!(rectified.finish(&model, &choices_text).is_err());
        assert!(rectified.add_image(name, &images[index]).is_err());
        let png = rectified.take_image();
        assert!(!png.is_empty());
        assert!(rectified.take_image().is_empty());
        let transform = &result["rectification"];
        let source = &transform["image"];
        assert_eq!(source["image_sha256"], sha256_hex(&png));
        assert_ne!(source["image_sha256"], result["image_sha256"]);
        assert_eq!(source["frame"], result["frame"]);
        assert_eq!(transform["source_camera"], result["sheet_pose"]["camera"]);
        for field in ["world_to_camera", "center_m"] {
            assert_eq!(
                transform["camera"][field],
                transform["source_camera"][field]
            );
        }
        let pixels = ftx_trace::raster::image::load_png(&png).unwrap();
        assert_eq!(source["image_size_px"], json!([pixels.w, pixels.h]));
        assert_eq!(transform["pixels_bgr_sha256"], sha256_hex(&pixels.data));
        let camera: Camera = serde_json::from_value(transform["camera"].clone()).unwrap();
        ftx_measure::silhouette::require_rectified(&camera).unwrap();
        assert_eq!(
            transform["camera"]["intrinsics"]["distortion"]["max_radius"],
            transform["source_camera"]["intrinsics"]["distortion"]["max_radius"]
        );
        assert!(pixels.w < 1600 || pixels.h < 1200);
        derived.push((source["image_name"].as_str().unwrap().to_owned(), png));
    }
    let prepared = rectified.finish(&model, &choices_text).unwrap();
    assert_eq!(prepared["schema"], "ftx-body-photo-capture-result/2");
    assert_eq!(prepared["status"], "CAPTURE_DRAFT");
    let draft = input(&prepared);
    let raw_prepared = raw.finish(&model, &choices_text).unwrap();
    assert_eq!(raw_prepared["schema"], "ftx-body-photo-capture-result/1");
    let target_edits = |source: &Value| {
        json!({"input_sha256":source["input_sha256"],"model_sha256":source["model_sha256"],
            "keypoints":truth["model"]["keypoints"]})
    };
    let raw_targets = body_fit_targets(
        &model,
        raw_prepared["input"].as_str().unwrap(),
        &target_edits(&raw_prepared).to_string(),
    )
    .unwrap();
    let targets = body_fit_targets(
        &model,
        prepared["input"].as_str().unwrap(),
        &target_edits(&prepared).to_string(),
    )
    .unwrap();
    let raw_review =
        body_fit_observations(&model, raw_targets["input"].as_str().unwrap(), "").unwrap();
    let raw_draft = input(&raw_prepared);
    for (index, frame) in raw_review["frames"].as_array().unwrap().iter().enumerate() {
        assert_eq!(frame["silhouette_editable"], true);
        assert_eq!(
            raw_draft["fit"]["frames"][index]["camera"],
            prepared["acquisition"]["frames"][index]["rectification"]["source_camera"]
        );
        let receipt = frame["image_review"].as_str().unwrap();
        let image = BodyFitImage::new(&images[index], receipt).unwrap();
        assert_eq!(
            image.report()["source_image"],
            raw_draft["capture_images"][index]
        );
        assert!(BodyFitImage::new(&derived[index].1, receipt).is_err());
    }
    let review = body_fit_observations(&model, targets["input"].as_str().unwrap(), "").unwrap();
    for (index, frame) in review["frames"].as_array().unwrap().iter().enumerate() {
        assert_eq!(frame["silhouette_editable"], true);
        assert_eq!(frame["keypoints"], json!([]));
        let receipt = frame["image_review"].as_str().unwrap();
        let image = BodyFitImage::new(&derived[index].1, receipt).unwrap();
        assert_eq!(
            image.report()["source_image"],
            draft["capture_images"][index]
        );
        assert!(BodyFitImage::new(&images[index], receipt).is_err());
        assert!(BodyFitImage::new(&derived[1 - index].1, receipt).is_err());
    }
    let observations: Vec<_> = truth["fit"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            let crop = &prepared["acquisition"]["frames"][index]["rectification"]["crop_origin_px"];
            let mut points = frame["keypoints"].clone();
            for point in points.as_array_mut().unwrap() {
                // Author new observations in the derived pixels using the independently known
                // pinhole projection. No existing measurements or weights are transformed.
                point["pixel_px"] = json!([
                    1050.0 * (point["pixel_px"][0].as_f64().unwrap() - 640.0) / 1000.0
                        + 790.0 - crop[0].as_f64().unwrap(),
                    point["pixel_px"][1].as_f64().unwrap() - 480.0
                        + 610.0 - crop[1].as_f64().unwrap(),
                ]);
            }
            let projected: Vec<Vec2> = points
                .as_array()
                .unwrap()
                .iter()
                .map(|point| serde_json::from_value(point["pixel_px"].clone()).unwrap())
                .collect();
            let at = (projected[0].y + projected[4].y) / 2.0 + 0.123;
            let silhouette = json!({"evidence":"Independent synthetic projected triangle intersections; no clothing or anatomical qualification",
                "lines":[{"axis":"horizontal","at_px":at,"intervals_px":known_silhouette(&projected,at),
                    "sigma_px":1.0,"partition":if index == 0 {"fit"} else {"held_out"}}]});
            json!({"frame_index":index,"keypoints":points,
                "silhouette_edit":{"action":"replace","silhouette":silhouette}})
        })
        .collect();
    let observed = body_fit_observations(
        &model,
        targets["input"].as_str().unwrap(),
        &json!({"input_sha256":targets["input_sha256"],"model_sha256":targets["model_sha256"],
            "frames":observations})
        .to_string(),
    )
    .unwrap();
    let starts: Vec<_> = truth["fit"]["starts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|start| {
            json!({"shape":start["shape"],"poses":start["poses"].as_array().unwrap().iter()
                .enumerate().map(|(i,pose)|json!({"frame_index":i,"values":pose["values"]})).collect::<Vec<_>>()})
        })
        .collect();
    let initialized = body_fit_starts(
        &model,
        observed["input"].as_str().unwrap(),
        &json!({"input_sha256":observed["input_sha256"],"model_sha256":observed["model_sha256"],"starts":starts})
            .to_string(),
    )
    .unwrap();
    save(&dir, &input(&initialized));
    let native = run(
        &dir,
        &["--iterations", "50", "--max-evaluations", "100000"],
        0,
    );
    let portable = fit_body(
        &model,
        initialized["input"].as_str().unwrap(),
        "50",
        "100000",
    )
    .unwrap();
    assert_eq!(native["result"], portable);
    assert_eq!(portable["status"], "ACCEPTED");
    assert!(
        portable["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|attempt| {
                attempt["checks"].as_array().unwrap().iter().any(|check| {
                    check["name"].as_str().unwrap().contains("silhouette")
                        && check["passed"] == true
                })
            })
    );

    // Different raw pixels only outside the certified crop do not constitute another view.
    let mut duplicate = BodyCapture::new(&manifest.to_string(), &calibration, sheet_text).unwrap();
    duplicate.add_image("raw-0.png", &images[0]).unwrap();
    let _ = duplicate.take_image();
    let mut outside = ftx_trace::raster::image::load_png(&images[0]).unwrap();
    outside.data[..3].fill(220);
    let outside = ftx_trace::raster::image::encode_png_bgr(&outside).unwrap();
    let error = duplicate.add_image("raw-1.png", &outside).unwrap_err();
    assert!(error.message.contains("duplicate rectified"), "{error}");
    assert!(duplicate.take_image().is_empty());
    duplicate.add_image("raw-1.png", &images[1]).unwrap();
    let _ = duplicate.take_image();
    assert_eq!(duplicate.finish(&model, &choices_text).unwrap(), prepared);

    if let Ok(directory) = std::env::var("FTX_BODY_RECTIFICATION_FIXTURES") {
        use std::io::Write;
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        let write = |name: &str, bytes: &[u8]| {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(bytes)
                .unwrap();
        };
        write("manifest.json", manifest.to_string().as_bytes());
        write("calibration.json", calibration.as_bytes());
        write("sheet.json", sheet_text.as_bytes());
        write("choices.json", choices_text.as_bytes());
        write("model.ftxbody", &model);
        write("raw-0.png", &images[0]);
        write("raw-1.png", &images[1]);
        write(
            "prepared.json",
            prepared["input"].as_str().unwrap().as_bytes(),
        );
        write("capture-result.json", prepared.to_string().as_bytes());
        write(
            "targets.json",
            target_edits(&prepared).to_string().as_bytes(),
        );
        write(
            "targeted.json",
            targets["input"].as_str().unwrap().as_bytes(),
        );
        for (name, png) in &derived {
            write(name, png);
        }
    }
}
