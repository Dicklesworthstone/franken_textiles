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
    let fit = |p: &Value| {
        json!({"name":p["name"],"choice":{"mode":"fit","lower":p["lower"],"upper":p["upper"]}})
    };
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
    assert_eq!(prepared["choices_sha256"], sha256_hex(choices_text.as_bytes()));
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
        assert_eq!(image.report()["source_image"], draft["capture_images"][index]);
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
    capture.add_image("capture-frame-0.png", &images[0]).unwrap();
    assert!(capture.add_image("capture-frame-0.png", &images[0]).is_err());
    let decoded = ftx_trace::raster::image::load_png(&images[0]).unwrap();
    let reencoded = ftx_trace::raster::image::encode_png_bgr(&decoded).unwrap();
    assert_ne!(sha256_hex(&reencoded), sha256_hex(&images[0]));
    assert!(capture.add_image("capture-frame-1.png", &reencoded).is_err());
    assert!(capture.finish(&model, &choices.to_string()).is_err());
    capture.add_image("capture-frame-1.png", &images[1]).unwrap();
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
    assert_eq!(capture.finish(&model, &choices.to_string()).unwrap(), original);

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
    assert_eq!(fresh.finish(&model, &choices.to_string()).unwrap(), original);
}
