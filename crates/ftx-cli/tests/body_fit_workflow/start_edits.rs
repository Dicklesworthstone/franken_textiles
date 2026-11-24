//! Actual initialization authoring and native replay, using the shared synthetic body fixture.

use super::*;

fn read(dir: &Path, file: &str) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(file)).unwrap()).unwrap()
}

fn write(dir: &Path, file: &str, value: &Value) {
    std::fs::write(dir.join(file), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn command(dir: &Path, args: &[&str], code: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn review(dir: &Path, output: &str, code: i32) -> Value {
    command(
        dir,
        &[
            "measure",
            "fit-start-review",
            "input.json",
            "--model",
            "model.ftxbody",
            "--out",
            output,
        ],
        code,
    )
}

fn apply(dir: &Path, output: &str, code: i32) -> Value {
    command(
        dir,
        &[
            "measure",
            "fit-starts",
            "annotations.json",
            "--model",
            "model.ftxbody",
            "--out",
            output,
        ],
        code,
    )
}

fn frame_id(frame: &Value) -> &Value {
    frame
        .get("frame")
        .unwrap_or(&frame["source"]["metadata"]["frame"])
}

fn annotations_for(input: &Value) -> Value {
    json!(
        input["fit"]["starts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|start| {
                let poses: Vec<_> = input["fit"]["frames"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(index, frame)| {
                        let source = start["poses"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|p| &p["frame"] == frame_id(frame))
                            .unwrap();
                        json!({"frame_index":index,"values":source["values"]})
                    })
                    .collect();
                json!({"shape":start["shape"],"poses":poses})
            })
            .collect::<Vec<_>>()
    )
}

fn long_frame_ids(input: &mut Value, first: u64) {
    let stream = "synthetic </script><script>globalThis.startInjection=1</script>";
    for i in 0..2 {
        let frame = json!({"stream":stream,"timestamp_us":first + i as u64 * 200_000});
        input["fit"]["frames"][i]["frame"] = frame.clone();
        for start in input["fit"]["starts"].as_array_mut().unwrap() {
            start["poses"][i]["frame"] = frame.clone();
        }
    }
}

#[test]
fn portable_observation_authoring_reaches_start_editing_preview_and_actual_fitting() {
    use ftx_wasm::body_fit::{
        BodyFitImage, body_fit_observations, body_fit_starts, body_fit_targets, fit_body,
        review_body_fit_start,
    };
    let (model, mut expected) = fixture();
    long_frame_ids(&mut expected, u64::MAX - 200_000);
    expected["model"]["root_rotation_rad"] = json!([-0.0, 0.0, 0.0]);
    let pixels = Gray {
        w: 1280,
        h: 960,
        data: (0..1280 * 960)
            .map(|i| ((i / 1280 * 7 + i % 1280 * 13) % 256) as u8)
            .collect(),
    };
    let image = encode_png_gray(&pixels);
    expected["capture_images"] = json!(expected["fit"]["frames"].as_array_mut().unwrap().iter_mut().map(|frame| {
        frame["rotation_cw_deg"] = json!(90);
        json!({"frame":frame["frame"],"image_name":"synthetic-rotated.png","image_sha256":sha256_hex(&image),
            "image_size_px":frame["image_size_px"],"rotation_cw_deg":90})
    }).collect::<Vec<_>>());
    let mut draft = expected.clone();
    draft["model"]["keypoints"] = json!({});
    draft["fit"]["starts"] = json!([]);
    for frame in draft["fit"]["frames"].as_array_mut().unwrap() {
        frame["keypoints"] = json!([]);
    }
    let source = format!(" \r\n{draft}\r\n");
    let targets = body_fit_targets(&model, &source, "").unwrap();
    assert_eq!(targets["input"], source);
    assert_eq!(targets["target_count"], 0);
    let targeted = body_fit_targets(
        &model,
        &source,
        &json!({
        "input_sha256":targets["input_sha256"],"model_sha256":targets["model_sha256"],
        "keypoints":expected["model"]["keypoints"]})
        .to_string(),
    )
    .unwrap();
    draft["model"]["keypoints"] = expected["model"]["keypoints"].clone();
    let source = targeted["input"].as_str().unwrap().to_owned();
    assert_eq!(serde_json::from_str::<Value>(&source).unwrap(), draft);
    let opened = body_fit_observations(&model, &source, "").unwrap();
    assert_eq!(opened["input"], source);
    assert_eq!(opened["body_fitted"], false);
    assert_eq!(opened["start_count"], 0);
    assert!(
        opened["frames"][1]["label"]
            .as_str()
            .unwrap()
            .contains(&u64::MAX.to_string())
    );
    let receipt = opened["frames"][1]["image_review"].as_str().unwrap();
    let decoded = BodyFitImage::new(&image, receipt).unwrap();
    assert_eq!(decoded.report()["review_kind"], "OBSERVATION_REVIEW");
    assert_eq!(
        decoded.report()["source_image"]["frame"]["timestamp_us"].as_u64(),
        Some(u64::MAX)
    );
    assert_eq!(
        ftx_trace::raster::color::bgr_to_gray(
            &ftx_trace::raster::image::load_png(decoded.preview()).unwrap()
        ),
        pixels,
        "90-degree source metadata describes already-oriented pixels, never another rotation"
    );
    let mut wrong = image.clone();
    wrong[0] ^= 1;
    assert!(BodyFitImage::new(&wrong, receipt).is_err());
    for (field, value) in [
        ("body_fitted", json!(true)),
        ("status", json!("ACCEPTED")),
        ("rotation_cw_deg", json!(0)),
    ] {
        let mut wrong: Value = serde_json::from_str(receipt).unwrap();
        wrong[field] = value;
        assert!(BodyFitImage::new(&image, &wrong.to_string()).is_err());
    }
    let frames: Vec<_> = expected["fit"]["frames"].as_array().unwrap().iter().enumerate()
        .map(|(frame_index, frame)| json!({"frame_index":frame_index,"keypoints":frame["keypoints"]})).rev().collect();
    let edits = json!({"input_sha256":opened["input_sha256"],"model_sha256":opened["model_sha256"],"frames":frames});
    let annotated = body_fit_observations(&model, &source, &edits.to_string()).unwrap();
    let text = annotated["input"].as_str().unwrap();
    let value: Value = serde_json::from_str(text).unwrap();
    let mut no_starts = expected.clone();
    no_starts["fit"]["starts"] = json!([]);
    assert_eq!(
        value, no_starts,
        "only the intended frame observations may change"
    );
    assert!(
        value["model"]["root_rotation_rad"][0]
            .as_f64()
            .unwrap()
            .is_sign_negative()
    );
    assert_eq!(annotated["frames"][1]["image_review"], receipt);
    let starts = body_fit_starts(&model, text, "").unwrap();
    let with_starts = body_fit_starts(
        &model,
        text,
        &json!({"input_sha256":starts["input_sha256"],
        "model_sha256":starts["model_sha256"],"starts":annotations_for(&expected)})
        .to_string(),
    )
    .unwrap();
    let ready = with_starts["input"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<Value>(ready).unwrap(), expected);
    let preview = review_body_fit_start(&model, ready, "0", "1").unwrap();
    assert_eq!(preview["frame"], expected["fit"]["frames"][1]["frame"]);
    let mut actual = fit_body(&model, ready, "60", "100000").unwrap();
    let mut manual = fit_body(&model, &expected.to_string(), "60", "100000").unwrap();
    assert_eq!(actual["status"], "ACCEPTED");
    assert_eq!(actual["input_sha256"], sha256_hex(ready.as_bytes()));
    actual.as_object_mut().unwrap().remove("input_sha256");
    manual.as_object_mut().unwrap().remove("input_sha256");
    assert_eq!(actual, manual);
    let reopened = body_fit_observations(&model, ready, "").unwrap();
    let mut corrections = json!({"input_sha256":reopened["input_sha256"],"model_sha256":reopened["model_sha256"],"frames":frames});
    corrections["frames"][0]["keypoints"][0]["pixel_px"][0] = json!(600.25);
    let corrected = body_fit_observations(&model, ready, &corrections.to_string()).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(corrected["input"].as_str().unwrap()).unwrap()["fit"]["starts"],
        expected["fit"]["starts"]
    );
    assert!(
        body_fit_observations(&model, ready, &edits.to_string()).is_err(),
        "edits bind the current, not an earlier, snapshot"
    );
}

#[test]
fn portable_observation_authoring_refuses_stale_or_invalid_changes() {
    use ftx_wasm::body_fit::body_fit_observations;
    let (model, input) = fixture();
    let text = input.to_string();
    let opened = body_fit_observations(&model, &text, "").unwrap();
    assert_eq!(opened["frames"][0]["image_review"], Value::Null);
    let frames: Vec<_> = input["fit"]["frames"].as_array().unwrap().iter().enumerate()
        .map(|(frame_index, frame)| json!({"frame_index":frame_index,"keypoints":frame["keypoints"]})).collect();
    let edits = json!({"input_sha256":opened["input_sha256"],"model_sha256":opened["model_sha256"],"frames":frames});
    for kind in [
        "source",
        "model",
        "missing_frame",
        "duplicate_frame",
        "unknown_frame",
        "unknown_target",
        "duplicate_target",
        "bounds",
        "sigma",
        "role",
        "extra",
    ] {
        let mut bad = edits.clone();
        match kind {
            "source" => bad["input_sha256"] = json!("0".repeat(64)),
            "model" => bad["model_sha256"] = json!("0".repeat(64)),
            "missing_frame" => {
                bad["frames"].as_array_mut().unwrap().pop();
            }
            "duplicate_frame" => bad["frames"][1]["frame_index"] = json!(0),
            "unknown_frame" => bad["frames"][0]["frame_index"] = json!(8),
            "unknown_target" => {
                bad["frames"][0]["keypoints"][0]["name"] = json!("invented anatomy")
            }
            "duplicate_target" => {
                bad["frames"][0]["keypoints"][1] = bad["frames"][0]["keypoints"][0].clone()
            }
            "bounds" => bad["frames"][0]["keypoints"][0]["pixel_px"] = json!([-1.0, 10.0]),
            "sigma" => bad["frames"][0]["keypoints"][0]["sigma_px"] = json!(2.0),
            "role" => bad["frames"][0]["keypoints"][0]["partition"] = json!("held_out"),
            "extra" => bad["cameras"] = json!([]),
            _ => unreachable!(),
        }
        assert!(
            body_fit_observations(&model, &text, &bad.to_string()).is_err(),
            "{kind}"
        );
    }
    assert!(body_fit_observations(&model, &(text.clone() + "\n"), &edits.to_string()).is_err());
    assert!(body_fit_observations(&model, &text, &" ".repeat(256 * 1024 + 1)).is_err());
    let mut cleared = edits;
    for frame in cleared["frames"].as_array_mut().unwrap() {
        frame["keypoints"] = json!([]);
    }
    let empty = body_fit_observations(&model, &text, &cleared.to_string()).unwrap();
    assert!(
        empty["frames"][0]["keypoints"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let unfitted =
        ftx_wasm::body_fit::fit_body(&model, empty["input"].as_str().unwrap(), "50", "100000")
            .unwrap();
    assert_eq!(unfitted["status"], "UNOBSERVABLE");
    assert_eq!(unfitted["solution"], Value::Null);
}

fn observation_edits(view: &Value) -> Value {
    json!({"input_sha256":view["input_sha256"],"model_sha256":view["model_sha256"],
        "frames":view["frames"].as_array().unwrap().iter().map(|frame|
            json!({"frame_index":frame["frame_index"],"keypoints":frame["keypoints"]})).collect::<Vec<_>>()})
}

fn manual_contour() -> Value {
    json!({"evidence":"Synthetic manually reviewed contours, no clothing or occluders; not physical evidence.",
        "lines":[
            {"axis":"horizontal","at_px":400.0,"intervals_px":[[100.0,200.0],[300.0,450.0]],"sigma_px":1.0,"partition":"fit"},
            {"axis":"vertical","at_px":600.0,"intervals_px":[[200.0,700.0]],"sigma_px":2.0,"partition":"held_out"},
            {"axis":"horizontal","at_px":50.0,"intervals_px":[],"sigma_px":1.5,"partition":"held_out"}]})
}

#[test]
fn portable_silhouette_authoring_reaches_actual_native_and_portable_fit() {
    use ftx_wasm::body_fit::{body_fit_observations, fit_body, review_body_fit_start};
    let dir = setup();
    let (model, mut original) = fixture();
    long_frame_ids(&mut original, u64::MAX - 200_000);
    original["model"]["root_rotation_rad"] = json!([-0.0, 0.0, 0.0]);
    original["fit"]["frames"].as_array_mut().unwrap().reverse();
    let masks = mask_manifest_for(&dir, &original);
    original["capture_images"] = json!(original["fit"]["frames"].as_array().unwrap().iter().zip(&masks)
        .enumerate().map(|(index, (frame, mask))| json!({"frame":frame["frame"],
            "image_name":format!("synthetic-contour-{index}.png"),"image_sha256":sha256_hex(&encode_png_gray(mask)),
            "image_size_px":frame["image_size_px"],"rotation_cw_deg":frame["rotation_cw_deg"]})).collect::<Vec<_>>());
    let source = format!(" \n{original}\n");
    let opened = body_fit_observations(&model, &source, "").unwrap();
    let mut edits = observation_edits(&opened);
    for (index, mask) in masks.iter().enumerate() {
        // Independent integer rasters provide boundary pairs; neither the fitting silhouette
        // projection nor its mask sampler generates this observed contour.
        let lines: Vec<_> = [100, 420, 480, 540]
            .into_iter()
            .map(|row| {
                let pixels = &mask.data[row * mask.w..(row + 1) * mask.w];
                assert_eq!((pixels[0], pixels[mask.w - 1]), (0, 0));
                let edges: Vec<_> = pixels
                    .windows(2)
                    .enumerate()
                    .filter(|(_, pair)| pair[0] != pair[1])
                    .map(|(column, _)| column as f64 + 0.5)
                    .collect();
                assert_eq!(edges.len() % 2, 0);
                let intervals = edges.as_chunks::<2>().0;
                json!({"axis":"horizontal","at_px":row,"intervals_px":intervals,"sigma_px":1.0,
                "partition":if row == 100 || row == 480 {"held_out"}else{"fit"}})
            })
            .collect();
        edits["frames"][index]["silhouette_edit"] = json!({"action":"replace","silhouette":{
            "evidence":"Independent synthetic raster boundaries manually declared as scanlines; no person or camera evidence.","lines":lines}});
    }
    edits["frames"].as_array_mut().unwrap().reverse();
    let applied = body_fit_observations(&model, &source, &edits.to_string()).unwrap();
    assert_eq!(
        applied["frames"][0]["image_review"],
        opened["frames"][0]["image_review"]
    );
    let prepared = applied["input"].as_str().unwrap();
    let mut value: Value = serde_json::from_str(prepared).unwrap();
    assert_eq!(
        applied["original_input_sha256"],
        sha256_hex(source.as_bytes())
    );
    assert_eq!(applied["input_sha256"], sha256_hex(prepared.as_bytes()));
    assert!(
        applied["frames"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["silhouette_editable"] == true)
    );
    assert_eq!(
        value["fit"]["frames"][0]["frame"]["timestamp_us"].as_u64(),
        Some(u64::MAX)
    );
    assert!(
        value["model"]["root_rotation_rad"][0]
            .as_f64()
            .unwrap()
            .is_sign_negative()
    );
    for frame in value["fit"]["frames"].as_array_mut().unwrap() {
        frame.as_object_mut().unwrap().remove("silhouette");
    }
    assert_eq!(
        value, original,
        "only explicitly edited silhouette fields may change"
    );
    let preview = review_body_fit_start(&model, prepared, "0", "0").unwrap();
    assert!(!preview["silhouette"].is_null());
    let portable = fit_body(&model, prepared, "50", "100000").unwrap();
    assert_eq!(portable["status"], "ACCEPTED");
    std::fs::write(dir.join("input.json"), prepared).unwrap();
    let native = run(&dir, &[], 0);
    assert_eq!(portable, native["result"]);
    // A CHECK contour never steers the fit and cannot be discarded by this handoff.
    let mut bad: Value = serde_json::from_str(prepared).unwrap();
    bad["fit"]["frames"][0]["silhouette"]["lines"][2]["intervals_px"][0][0] = json!(100.0);
    let rejected = fit_body(&model, &bad.to_string(), "50", "100000").unwrap();
    assert_eq!(rejected["status"], "REJECTED");
    assert!(rejected["solution"].is_null());
    for (baseline, changed) in portable["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .zip(rejected["attempts"].as_array().unwrap())
    {
        assert_eq!(
            baseline["cost_history"], changed["cost_history"],
            "held-out contours never steer optimization"
        );
    }
}

#[test]
fn portable_binary_masks_preserve_sources_and_replay_native_sampling_and_fitting() {
    use ftx_wasm::body_fit::{body_fit_mask, body_fit_observations, fit_body};

    let dir = setup();
    let (model, mut original) = fixture();
    long_frame_ids(&mut original, u64::MAX - 200_000);
    original["model"]["root_rotation_rad"] = json!([-0.0, 0.0, 0.0]);
    original["fit"]["frames"].as_array_mut().unwrap().reverse();
    let masks = mask_manifest_for(&dir, &original);
    let manifest = read(&dir, "masks/manifest.json");
    write(&dir, "input.json", &original);
    let native_masks = run(&dir, &["--silhouettes", "masks/manifest.json"], 0);
    let mut source = format!(" \n{original}\n");
    for (index, mask) in masks.iter().enumerate() {
        let png = encode_png_gray(mask);
        let selection = json!({
            "input_sha256":sha256_hex(source.as_bytes()),"model_sha256":sha256_hex(&model),
            "frame_index":index,"mask_name":format!("mask-{index}.png"),
            "evidence":manifest["evidence"],"samples":manifest["frames"][index]["samples"]
        });
        let before: Value = serde_json::from_str(&source).unwrap();
        let imported = body_fit_mask(&model, &source, &png, &selection.to_string()).unwrap();
        assert_eq!(
            imported["original_input_sha256"],
            sha256_hex(source.as_bytes())
        );
        assert_eq!(
            imported["mask_import"]["encoded_mask_sha256"],
            sha256_hex(&png)
        );
        assert_eq!(
            imported["mask_import"]["decoded_mask_sha256"],
            sha256_hex(&mask.data)
        );
        assert_eq!(
            imported["mask_import"]["frame"],
            before["fit"]["frames"][index]["frame"]
        );
        assert_eq!(imported["mask_import"]["samples"], selection["samples"]);
        assert_eq!(imported["frames"][index]["silhouette_editable"], false);
        source = imported["input"].as_str().unwrap().to_owned();
        let mut without_import: Value = serde_json::from_str(&source).unwrap();
        without_import["fit"]["frames"][index]
            .as_object_mut()
            .unwrap()
            .remove("silhouette");
        assert_eq!(
            without_import, before,
            "only the selected silhouette may change"
        );
        let opened = body_fit_observations(&model, &source, "").unwrap();
        let mut edits = observation_edits(&opened);
        edits["frames"][index]["silhouette_edit"] = json!({"action":"remove"});
        assert!(body_fit_observations(&model, &source, &edits.to_string()).is_err());
        let mut repeat = selection.clone();
        repeat["input_sha256"] = json!(sha256_hex(source.as_bytes()));
        assert!(body_fit_mask(&model, &source, &png, &repeat.to_string()).is_err());
    }
    let portable = fit_body(&model, &source, "50", "100000").unwrap();
    assert_eq!(portable["status"], "ACCEPTED");
    assert_eq!(
        portable["input"]["frames"][1]["frame"]["timestamp_us"].as_u64(),
        Some(u64::MAX)
    );
    for (frame, native) in portable["input"]["frames"].as_array().unwrap().iter().zip(
        native_masks["result"]["input"]["frames"]
            .as_array()
            .unwrap(),
    ) {
        assert_eq!(frame["silhouette"]["lines"], native["silhouette"]["lines"]);
    }
    for (attempt, native) in portable["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .zip(native_masks["result"]["attempts"].as_array().unwrap())
    {
        assert_eq!(attempt["cost_history"], native["cost_history"]);
    }
    std::fs::write(dir.join("input.json"), &source).unwrap();
    assert_eq!(portable, run(&dir, &[], 0)["result"]);
}

#[test]
fn portable_binary_masks_refuse_stale_or_transformed_sources_and_invalid_slices() {
    use ftx_wasm::body_fit::body_fit_mask;

    let (model, original) = fixture();
    let source = original.to_string();
    let mut mask = Gray::filled(1280, 960, 0);
    mask.data[500 * 1280 + 100..500 * 1280 + 201].fill(255);
    let png = encode_png_gray(&mask);
    let selection = json!({
        "input_sha256":sha256_hex(source.as_bytes()),"model_sha256":sha256_hex(&model),
        "frame_index":0,"mask_name":"synthetic.png","evidence":"Synthetic binary contour; no wearer data",
        "samples":[{"axis":"horizontal","index":500,"sigma_px":1.0,"partition":"held_out"}]
    });
    let imported = body_fit_mask(&model, &source, &png, &selection.to_string()).unwrap();
    assert_eq!(
        imported["frames"][0]["silhouette"]["lines"][0]["intervals_px"],
        json!([[99.5, 200.5]])
    );
    assert_eq!(
        imported["frames"][0]["silhouette"]["lines"][0]["partition"],
        "held_out"
    );

    for (field, value) in [
        ("input_sha256", json!("0".repeat(64))),
        ("model_sha256", json!("0".repeat(64))),
        ("frame_index", json!(8)),
        ("mask_name", json!("../mask.png")),
        ("evidence", json!("")),
        ("evidence", json!("decoded_mask_sha256:invented")),
    ] {
        let mut changed = selection.clone();
        changed[field] = value;
        assert!(
            body_fit_mask(&model, &source, &png, &changed.to_string()).is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("index", json!(960)),
        ("index", json!(500.5)),
        ("sigma_px", json!(0)),
        ("partition", json!("inferred")),
    ] {
        let mut changed = selection.clone();
        changed["samples"][0][field] = value;
        assert!(
            body_fit_mask(&model, &source, &png, &changed.to_string()).is_err(),
            "{field}"
        );
    }
    let mut duplicate = selection.clone();
    duplicate["samples"]
        .as_array_mut()
        .unwrap()
        .push(selection["samples"][0].clone());
    assert!(body_fit_mask(&model, &source, &png, &duplicate.to_string()).is_err());
    assert!(
        body_fit_mask(
            &model,
            &(source.clone() + "\n"),
            &png,
            &selection.to_string()
        )
        .is_err()
    );
    assert!(body_fit_mask(&model, &source, &png, &" ".repeat(64 * 1024 + 1)).is_err());

    let mut soft = mask.clone();
    soft.data[500 * 1280 + 100] = 128;
    let mut cropped = mask.clone();
    cropped.data[0] = 255;
    let mut rgb_header = png.clone();
    rgb_header[25] = 2;
    let mut depth_header = png.clone();
    depth_header[24] = 16;
    for invalid in [
        encode_png_gray(&soft),
        encode_png_gray(&cropped),
        encode_png_gray(&Gray::filled(1280, 960, 0)),
        encode_png_gray(&Gray::filled(16, 16, 0)),
        rgb_header,
        depth_header,
        vec![0xff, 0xd8, 0xff, 0xd9],
    ] {
        assert!(body_fit_mask(&model, &source, &invalid, &selection.to_string()).is_err());
    }
    let mut occupied = original.clone();
    occupied["fit"]["frames"][0]["silhouette"] = manual_contour();
    let occupied = occupied.to_string();
    let mut rebound = selection.clone();
    rebound["input_sha256"] = json!(sha256_hex(occupied.as_bytes()));
    assert!(body_fit_mask(&model, &occupied, &png, &rebound.to_string()).is_err());
}

#[test]
fn portable_silhouette_edits_preserve_existing_roles_and_allow_explicit_removal() {
    use ftx_wasm::body_fit::body_fit_observations;
    let (model, mut source) = fixture();
    source["fit"]["frames"][0]["silhouette"] = manual_contour();
    source["fit"]["frames"][1]["silhouette"] = Value::Null;
    let text = source.to_string();
    let opened = body_fit_observations(&model, &text, "").unwrap();
    assert_eq!(opened["input"], text);
    assert_eq!(opened["frames"][0]["silhouette"], manual_contour());
    let mut edits = observation_edits(&opened);
    let unchanged = body_fit_observations(&model, &text, &edits.to_string()).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(unchanged["input"].as_str().unwrap()).unwrap(),
        source
    );
    let mut replacement = manual_contour();
    replacement["lines"][0]["intervals_px"] = json!([[110.0, 190.0], [310.0, 440.0]]);
    replacement["lines"]
        .as_array_mut()
        .unwrap()
        .push(json!({"axis":"vertical","at_px":610.0,
        "intervals_px":[[200.0,700.0]],"sigma_px":0.5,"partition":"fit"}));
    edits["frames"][0]["silhouette_edit"] = json!({"action":"replace","silhouette":replacement});
    let corrected = body_fit_observations(&model, &text, &edits.to_string()).unwrap();
    assert_eq!(corrected["frames"][0]["silhouette"], replacement);
    for (field, value) in [("partition", json!("held_out")), ("sigma_px", json!(1.5))] {
        let mut bad = edits.clone();
        bad["frames"][0]["silhouette_edit"]["silhouette"]["lines"][0][field] = value;
        assert!(
            body_fit_observations(&model, &text, &bad.to_string()).is_err(),
            "{field}"
        );
    }
    edits["frames"][0]["silhouette_edit"] = json!({"action":"remove"});
    let removed = body_fit_observations(&model, &text, &edits.to_string()).unwrap();
    let mut value: Value = serde_json::from_str(removed["input"].as_str().unwrap()).unwrap();
    assert!(value["fit"]["frames"][0].get("silhouette").is_none());
    assert_eq!(
        value["fit"]["frames"][1].get("silhouette"),
        Some(&Value::Null)
    );
    value["fit"]["frames"][0]["silhouette"] = source["fit"]["frames"][0]["silhouette"].clone();
    assert_eq!(value, source);
}

#[test]
fn portable_silhouette_authoring_preserves_imported_masks_with_brown_cameras() {
    use ftx_wasm::body_fit::body_fit_observations;
    let (model, mut source) = fixture();
    let mut mask_contour = manual_contour();
    mask_contour["evidence"] = json!(format!(
        "Native source; decoded_mask_sha256:{}",
        "a".repeat(64)
    ));
    source["fit"]["frames"][0]["silhouette"] = mask_contour.clone();
    for frame in source["fit"]["frames"].as_array_mut().unwrap() {
        frame["camera"]["intrinsics"]["distortion"]["radial"] = json!([0.001, 0.0, 0.0]);
    }
    let text = source.to_string();
    let opened = body_fit_observations(&model, &text, "").unwrap();
    assert_eq!(opened["frames"][0]["silhouette_editable"], false);
    assert!(
        opened["frames"][0]["silhouette_reason"]
            .as_str()
            .unwrap()
            .contains("mask")
    );
    assert_eq!(opened["frames"][1]["silhouette_editable"], true);
    let mut edits = observation_edits(&opened);
    edits["frames"][0]["keypoints"][0]["pixel_px"][0] = json!(600.0);
    let points = body_fit_observations(&model, &text, &edits.to_string()).unwrap();
    assert_eq!(points["frames"][0]["silhouette"], mask_contour);
    for action in [
        json!({"action":"replace","silhouette":manual_contour()}),
        json!({"action":"remove"}),
    ] {
        let mut bad = edits.clone();
        bad["frames"][0]["silhouette_edit"] = action;
        assert!(body_fit_observations(&model, &text, &bad.to_string()).is_err());
    }
    source["fit"]["frames"][0]
        .as_object_mut()
        .unwrap()
        .remove("silhouette");
    let text = source.to_string();
    let opened = body_fit_observations(&model, &text, "").unwrap();
    let mut edits = observation_edits(&opened);
    edits["frames"][0]["silhouette_edit"] = json!({"action":"replace","silhouette":mask_contour});
    assert!(
        body_fit_observations(&model, &text, &edits.to_string()).is_err(),
        "manual edits cannot invent mask receipts"
    );
}

#[test]
fn portable_brown_silhouette_authoring_preserves_raw_pixels_cameras_and_roles() {
    use ftx_wasm::body_fit::body_fit_observations;
    let (model, mut source) = fixture();
    long_frame_ids(&mut source, u64::MAX - 200_000);
    source["fit"]["frames"][0]["camera"]["intrinsics"]["distortion"] = json!({
        "radial":[0.08,-0.01,0.002],"tangential":[0.003,-0.002],"max_radius":1.0
    });
    let text = format!(" \r\n{source}\r\n");
    let opened = body_fit_observations(&model, &text, "").unwrap();
    assert_eq!(opened["input"], text);
    assert_eq!(opened["frames"][0]["silhouette_editable"], true);
    let mut contour = manual_contour();
    contour["lines"][0]["intervals_px"] = json!([[100.25, 200.5], [300.75, 450.125]]);
    let mut edits = observation_edits(&opened);
    edits["frames"][0]["silhouette_edit"] = json!({"action":"replace","silhouette":contour});
    let authored = body_fit_observations(&model, &text, &edits.to_string()).unwrap();
    let authored_text = authored["input"].as_str().unwrap();
    let value: Value = serde_json::from_str(authored_text).unwrap();
    source["fit"]["frames"][0]["silhouette"] = contour.clone();
    assert_eq!(
        value, source,
        "raw Brown contours must not rectify pixels, alter cameras or rewrite source identities"
    );
    assert_eq!(authored["frames"][0]["silhouette"], contour);
    assert_eq!(authored["body_fitted"], false);
    let mut correction = observation_edits(&authored);
    let mut moved = contour.clone();
    moved["lines"][0]["intervals_px"][0] = json!([105.25, 195.5]);
    correction["frames"][0]["silhouette_edit"] = json!({"action":"replace","silhouette":moved});
    let corrected = body_fit_observations(&model, authored_text, &correction.to_string()).unwrap();
    source["fit"]["frames"][0]["silhouette"] = moved;
    assert_eq!(
        serde_json::from_str::<Value>(corrected["input"].as_str().unwrap()).unwrap(),
        source
    );
    for (line, field, value) in [
        (0, "partition", json!("held_out")),
        (1, "partition", json!("fit")),
        (0, "sigma_px", json!(1.5)),
    ] {
        let mut invalid = correction.clone();
        invalid["frames"][0]["silhouette_edit"]["silhouette"]["lines"][line][field] = value;
        assert!(
            body_fit_observations(&model, authored_text, &invalid.to_string()).is_err(),
            "Brown admission must preserve existing slice {line} {field}"
        );
    }
}

#[test]
fn portable_brown_silhouette_authoring_refuses_uncertified_foreground_and_folding_lenses() {
    use ftx_wasm::body_fit::body_fit_observations;
    let (model, mut source) = fixture();
    // Isolate contour admission from existing keypoint bearings. Empty background slices do not
    // invent a point outside the small calibrated disk; actual foreground endpoints must invert.
    source["fit"]["frames"][0]["keypoints"] = json!([]);
    source["fit"]["frames"][0]["camera"]["intrinsics"]["distortion"] = json!({
        "radial":[0.08,0.0,0.0],"tangential":[0.0,0.0],"max_radius":0.25
    });
    let text = source.to_string();
    let opened = body_fit_observations(&model, &text, "").unwrap();
    let contour = json!({"evidence":"Synthetic raw-pixel domain control; not physical evidence.",
        "lines":[
            {"axis":"horizontal","at_px":480.0,"intervals_px":[[600.0,680.0]],"sigma_px":1.0,"partition":"fit"},
            {"axis":"horizontal","at_px":490.0,"intervals_px":[[610.0,690.0]],"sigma_px":2.0,"partition":"held_out"},
            {"axis":"vertical","at_px":0.0,"intervals_px":[],"sigma_px":1.5,"partition":"held_out"}]});
    let mut edits = observation_edits(&opened);
    edits["frames"][0]["silhouette_edit"] = json!({"action":"replace","silhouette":contour});
    let accepted = body_fit_observations(&model, &text, &edits.to_string()).unwrap();
    assert_eq!(accepted["frames"][0]["silhouette"], contour);
    for line in 0..2 {
        let mut invalid = edits.clone();
        invalid["frames"][0]["silhouette_edit"]["silhouette"]["lines"][line]["intervals_px"][0]
            [1] = json!(1000.0);
        assert_eq!(
            body_fit_observations(&model, &text, &invalid.to_string())
                .unwrap_err()
                .code,
            ftx_core::RefusalCode::InvalidDocument,
            "both FIT and CHECK foreground endpoints require the calibrated lens domain"
        );
        let mut invalid_source = source.clone();
        invalid_source["fit"]["frames"][0]["silhouette"] =
            invalid["frames"][0]["silhouette_edit"]["silhouette"].clone();
        assert!(body_fit_observations(&model, &invalid_source.to_string(), "").is_err());
    }
    source["fit"]["frames"][0]["camera"]["intrinsics"]["distortion"] = json!({
        "radial":[-1.0,0.0,0.0],"tangential":[0.0,0.0],"max_radius":1.0
    });
    assert_eq!(
        body_fit_observations(&model, &source.to_string(), "")
            .unwrap_err()
            .code,
        ftx_core::RefusalCode::InvalidDocument
    );
}

#[test]
fn portable_silhouette_authoring_uses_the_canonical_contour_limits() {
    use ftx_wasm::body_fit::body_fit_observations;
    let (model, source) = fixture();
    let text = source.to_string();
    let opened = body_fit_observations(&model, &text, "").unwrap();
    let mut edits = observation_edits(&opened);
    edits["frames"][0]["silhouette_edit"] =
        json!({"action":"replace","silhouette":manual_contour()});
    for kind in [
        "null",
        "unknown_action",
        "extra_action_field",
        "missing_contour",
        "evidence",
        "long_evidence",
        "no_lines",
        "background_only",
        "duplicate",
        "axis",
        "coordinate",
        "out_of_bounds",
        "sigma",
        "missing_partition",
        "many_lines",
        "many_intervals",
        "overlap",
        "touching",
        "outside",
        "backwards",
        "extra_line_field",
    ] {
        let mut bad = edits.clone();
        let edit = &mut bad["frames"][0]["silhouette_edit"];
        match kind {
            "null" => *edit = Value::Null,
            "unknown_action" => edit["action"] = json!("unchanged"),
            "extra_action_field" => edit["ignored"] = json!(true),
            "missing_contour" => {
                edit.as_object_mut().unwrap().remove("silhouette");
            }
            "evidence" => edit["silhouette"]["evidence"] = json!(" "),
            "long_evidence" => edit["silhouette"]["evidence"] = json!("a".repeat(4097)),
            "no_lines" => edit["silhouette"]["lines"] = json!([]),
            "background_only" => {
                for line in edit["silhouette"]["lines"].as_array_mut().unwrap() {
                    line["intervals_px"] = json!([]);
                }
            }
            "duplicate" => {
                let line = edit["silhouette"]["lines"][0].clone();
                edit["silhouette"]["lines"]
                    .as_array_mut()
                    .unwrap()
                    .push(line);
            }
            "axis" => edit["silhouette"]["lines"][0]["axis"] = json!("diagonal"),
            "coordinate" => edit["silhouette"]["lines"][0]["at_px"] = json!(-1.0),
            "out_of_bounds" => edit["silhouette"]["lines"][0]["at_px"] = json!(960.0),
            "sigma" => edit["silhouette"]["lines"][0]["sigma_px"] = json!(0.0),
            "missing_partition" => {
                edit["silhouette"]["lines"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("partition");
            }
            "many_lines" => {
                edit["silhouette"]["lines"] = json!(
                    (0..65)
                        .map(|i| {
                            let mut line = manual_contour()["lines"][0].clone();
                            line["at_px"] = json!(i);
                            line
                        })
                        .collect::<Vec<_>>()
                )
            }
            "many_intervals" => {
                edit["silhouette"]["lines"][0]["intervals_px"] =
                    json!((0..9).map(|i| [i * 3, i * 3 + 1]).collect::<Vec<_>>())
            }
            "overlap" => {
                edit["silhouette"]["lines"][0]["intervals_px"] = json!([[100, 300], [200, 400]])
            }
            "touching" => {
                edit["silhouette"]["lines"][0]["intervals_px"] = json!([[100, 200], [200, 400]])
            }
            "outside" => edit["silhouette"]["lines"][0]["intervals_px"] = json!([[100, 1280]]),
            "backwards" => edit["silhouette"]["lines"][0]["intervals_px"] = json!([[200, 100]]),
            "extra_line_field" => edit["silhouette"]["lines"][0]["ignored"] = json!(true),
            _ => unreachable!(),
        }
        assert!(
            body_fit_observations(&model, &text, &bad.to_string()).is_err(),
            "{kind}"
        );
    }
}

#[test]
fn portable_start_authoring_preserves_evidence_and_replays_native_fit() {
    let (model, mut original) = fixture();
    long_frame_ids(&mut original, u64::MAX - 200_000);
    original["fit"]["frames"].as_array_mut().unwrap().reverse();
    original["fit"]["starts"][0]["poses"]
        .as_array_mut()
        .unwrap()
        .reverse();
    original["capture_images"] = json!(
        original["fit"]["frames"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, frame)| json!({"frame":frame["frame"],
            "image_name":format!("synthetic-{i}.png"),"image_sha256":"a".repeat(64),
            "image_size_px":frame["image_size_px"],"rotation_cw_deg":frame["rotation_cw_deg"]}))
            .collect::<Vec<_>>()
    );
    let desired = annotations_for(&original);
    let mut draft = original.clone();
    draft["fit"]["starts"] = json!([]);
    let source = serde_json::to_string_pretty(&draft).unwrap();
    let prepared = ftx_wasm::body_fit::body_fit_starts(&model, &source, "").unwrap();
    assert_eq!(prepared["input"], source);
    assert_eq!(prepared["input_sha256"], sha256_hex(source.as_bytes()));
    assert_eq!(prepared["original_input_sha256"], prepared["input_sha256"]);
    assert_eq!(prepared["model_sha256"], sha256_hex(&model));
    assert_eq!(prepared["starts"], json!([]));
    assert_eq!(prepared["body_fitted"], false);
    assert_eq!(
        prepared["frames"][0]["identity"]["timestamp_us"],
        u64::MAX.to_string()
    );
    assert_eq!(prepared["frames"][0]["frame_index"], 0);
    assert_eq!(
        prepared["shape"][0]["name"],
        original["model"]["shape_parameters"][0]["name"]
    );
    assert_eq!(
        prepared["shape"][0]["lower"],
        original["model"]["shape_parameters"][0]["lower"]
    );
    assert_eq!(prepared["pose"][0]["unit"], "m");
    assert_eq!(prepared["pose"][6]["unit"], "rad");

    // Annotations deliberately arrive in a different pose order. The edited input retains
    // the original frame order and joins each pose by ordinal, not by its array position.
    let mut edits = json!({"starts":desired});
    edits["starts"][0]["poses"]
        .as_array_mut()
        .unwrap()
        .reverse();
    edits["starts"][0]["shape"][0] = json!(0.64);
    let applied = ftx_wasm::body_fit::body_fit_starts(&model, &source, &edits.to_string()).unwrap();
    let text = applied["input"].as_str().unwrap();
    let mut value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        applied["original_input_sha256"],
        sha256_hex(source.as_bytes())
    );
    assert_eq!(applied["input_sha256"], sha256_hex(text.as_bytes()));
    assert_ne!(applied["input_sha256"], applied["original_input_sha256"]);
    assert_eq!(
        value["fit"]["frames"][0]["frame"]["timestamp_us"].as_u64(),
        Some(u64::MAX)
    );
    for start in value["fit"]["starts"].as_array().unwrap() {
        for (pose, frame) in start["poses"]
            .as_array()
            .unwrap()
            .iter()
            .zip(original["fit"]["frames"].as_array().unwrap())
        {
            assert_eq!(pose["frame"], frame["frame"]);
        }
    }
    value["fit"]["starts"] = json!([]);
    assert_eq!(
        value, draft,
        "editing starts changed observation or source evidence"
    );
    let reopened = ftx_wasm::body_fit::body_fit_starts(&model, text, "").unwrap();
    assert_eq!(reopened["input"], text);
    assert_eq!(reopened["starts"], applied["starts"]);
    let preview = ftx_wasm::body_fit::review_body_fit_start(&model, text, "0", "0").unwrap();
    assert_eq!(preview["shape_values"][0], 0.64);
    assert_eq!(preview["frame"]["timestamp_us"].as_u64(), Some(u64::MAX));
    assert_eq!(preview["source_image"], original["capture_images"][0]);

    let portable = ftx_wasm::body_fit::fit_body(&model, text, "60", "20000").unwrap();
    assert_eq!(portable["status"], "ACCEPTED");
    let dir = setup();
    std::fs::write(dir.join("input.json"), text).unwrap();
    let native = run(
        &dir,
        &["--iterations", "60", "--max-evaluations", "20000"],
        0,
    );
    assert_eq!(portable, native["result"]);
}

#[test]
fn portable_start_authoring_supports_one_start_without_weakening_fit_admission() {
    let (model, input) = fixture();
    let original = input.to_string();
    let starts = annotations_for(&input);
    let applied = ftx_wasm::body_fit::body_fit_starts(
        &model,
        &original,
        &json!({"starts":[starts[0]]}).to_string(),
    )
    .unwrap();
    let text = applied["input"].as_str().unwrap();
    assert_eq!(applied["starts"].as_array().unwrap().len(), 1);
    assert_eq!(
        ftx_wasm::body_fit::review_body_fit_start(&model, text, "0", "1").unwrap()["body_fitted"],
        false
    );
    let refusal = ftx_wasm::body_fit::fit_body(&model, text, "50", "10000").unwrap_err();
    assert_eq!(refusal.code, ftx_core::RefusalCode::InvalidDocument);
    assert_eq!(
        refusal.message,
        "body-fit input requires 2..=4 starts; found 1"
    );
    assert_eq!(
        ftx_wasm::body_fit::body_fit_starts(&model, &original, "").unwrap()["input"],
        original
    );
}

#[test]
fn portable_start_authoring_binds_edits_to_exact_source_and_model() {
    use ftx_wasm::body_fit::body_fit_starts;

    let (model, input) = fixture();
    let source = format!(" \r\n{input}\r\n");
    let opened = body_fit_starts(&model, &source, "").unwrap();
    let mut edits = json!({"input_sha256":opened["input_sha256"],
        "model_sha256":opened["model_sha256"],"starts":opened["starts"]});
    edits["starts"][0]["shape"][0] = json!(0.64);
    let bound = body_fit_starts(&model, &source, &edits.to_string()).unwrap();
    let legacy = json!({"starts":edits["starts"]});
    assert_eq!(
        bound,
        body_fit_starts(&model, &source, &legacy.to_string()).unwrap(),
        "paired source hashes must retain the existing authoring behavior"
    );
    for field in ["input_sha256", "model_sha256"] {
        let mut missing = edits.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            body_fit_starts(&model, &source, &missing.to_string())
                .unwrap_err()
                .to_string()
                .contains("both input_sha256 and model_sha256")
        );
        for value in ["0".repeat(64), "bad".to_owned()] {
            let mut wrong = edits.clone();
            wrong[field] = json!(value);
            assert!(
                body_fit_starts(&model, &source, &wrong.to_string())
                    .unwrap_err()
                    .to_string()
                    .contains("different input or model bytes")
            );
        }
    }
    let mut changed = input.clone();
    changed["fit"]["frames"][0]["keypoints"][0]["sigma_px"] = json!(1.25);
    for stale in [source.clone() + "\n", changed.to_string()] {
        assert!(body_fit_starts(&model, &stale, "").is_ok());
        assert!(
            body_fit_starts(&model, &stale, &edits.to_string())
                .unwrap_err()
                .to_string()
                .contains("different input or model bytes"),
            "edits cannot cross even valid source revisions"
        );
    }
}

#[test]
fn portable_start_authoring_refuses_invalid_edits_and_changed_sources() {
    let (model, input) = fixture();
    let text = input.to_string();
    let valid = json!({"starts":annotations_for(&input)});
    for kind in [
        "empty",
        "too_many",
        "duplicate_start",
        "boundary",
        "dimension",
        "missing_pose",
        "duplicate_frame",
        "unknown_frame",
        "fractional_frame",
        "edit_evidence",
        "extra_pose_field",
    ] {
        let mut bad = valid.clone();
        match kind {
            "empty" => bad["starts"] = json!([]),
            "too_many" => bad["starts"] = json!(vec![bad["starts"][0].clone(); 5]),
            "duplicate_start" => bad["starts"][1] = bad["starts"][0].clone(),
            "boundary" => {
                bad["starts"][0]["shape"][0] =
                    input["model"]["shape_parameters"][0]["lower"].clone()
            }
            "dimension" => bad["starts"][0]["shape"] = json!([]),
            "missing_pose" => {
                bad["starts"][0]["poses"].as_array_mut().unwrap().pop();
            }
            "duplicate_frame" => bad["starts"][0]["poses"][1]["frame_index"] = json!(0),
            "unknown_frame" => bad["starts"][0]["poses"][0]["frame_index"] = json!(8),
            "fractional_frame" => bad["starts"][0]["poses"][0]["frame_index"] = json!(0.5),
            "edit_evidence" => bad["measurements"] = json!([]),
            "extra_pose_field" => {
                bad["starts"][0]["poses"][0]["frame"] = input["fit"]["frames"][0]["frame"].clone()
            }
            _ => unreachable!(),
        }
        assert!(
            ftx_wasm::body_fit::body_fit_starts(&model, &text, &bad.to_string()).is_err(),
            "{kind}"
        );
    }
    let infinite = valid.to_string().replacen("0.63", "1e999", 1);
    assert_ne!(infinite, valid.to_string());
    assert!(ftx_wasm::body_fit::body_fit_starts(&model, &text, &infinite).is_err());
    for kind in ["schema", "model", "capture", "evidence"] {
        let mut bad = input.clone();
        match kind {
            "schema" => bad["schema"] = json!("ftx-anny-capture-input/1"),
            "model" => bad["model"]["model_sha256"] = json!("a".repeat(64)),
            "capture" => bad["capture_images"] = json!([]),
            "evidence" => bad["fit"]["frames"][0]["keypoints"][0]["sigma_px"] = json!(0),
            _ => unreachable!(),
        }
        assert!(
            ftx_wasm::body_fit::body_fit_starts(&model, &bad.to_string(), &valid.to_string())
                .is_err(),
            "{kind}"
        );
    }
    assert!(
        ftx_wasm::body_fit::body_fit_starts(&model, &text, &" ".repeat(64 * 1024 + 1)).is_err()
    );
}

#[test]
fn start_edits_bootstrap_and_ordinals_preserve_exact_native_frame_identities() {
    let dir = setup();
    let mut good = read(&dir, "input.json");
    long_frame_ids(&mut good, u64::MAX - 200_000);
    good["fit"]["frames"].as_array_mut().unwrap().reverse();
    good["fit"]["starts"][0]["poses"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let desired = annotations_for(&good);
    let mut empty = good.clone();
    empty["fit"]["starts"] = json!([]);
    save(&dir, &empty);
    let original = std::fs::read(dir.join("input.json")).unwrap();
    let report = review(&dir, "empty-review", 0)["result"].clone();
    assert_eq!(report["body_fitted"], false);
    assert_eq!(report["starts"], json!([]));
    let page = std::fs::read_to_string(dir.join("empty-review/index.html")).unwrap();
    assert!(!page.contains("<script>globalThis.startInjection"));
    let review_prepared_bytes =
        std::fs::read(dir.join("empty-review/prepared-input.json")).unwrap();
    let review_prepared: Value = serde_json::from_slice(&review_prepared_bytes).unwrap();
    assert_eq!(review_prepared["schema"], "ftx-anny-body-fit-input/1");
    let original_fit: ftx_measure::body_fit::BodyFitInput =
        serde_json::from_value(empty["fit"].clone()).unwrap();
    assert_eq!(
        review_prepared["fit"],
        serde_json::to_value(original_fit).unwrap()
    );
    let entry = &report["start_editor"]["prepared_input"];
    assert_eq!(entry["file"], "prepared-input.json");
    assert_eq!(entry["bytes"], review_prepared_bytes.len());
    assert_eq!(entry["sha256"], sha256_hex(&review_prepared_bytes));
    let embedded = page
        .split_once("id=\"startPreparedInput\">")
        .unwrap()
        .1
        .split_once("</script>")
        .unwrap()
        .0;
    assert_eq!(
        serde_json::from_str::<Value>(embedded).unwrap(),
        review_prepared
    );
    for (index, frame) in empty["fit"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let descriptor = &report["start_editor"]["frames"][index];
        assert_eq!(descriptor["frame_index"], index);
        assert_eq!(descriptor["identity"]["stream"], frame["frame"]["stream"]);
        assert_eq!(
            descriptor["identity"]["timestamp_us"],
            frame["frame"]["timestamp_us"].as_u64().unwrap().to_string()
        );
    }
    assert_eq!(
        review_prepared["fit"]["frames"][0]["frame"]["timestamp_us"].as_u64(),
        Some(u64::MAX)
    );
    let mut annotation = read(&dir, "empty-review/body-starts.json");
    assert_eq!(annotation["schema"], "ftx-body-starts/1");
    assert_eq!(annotation["source_input"], json!(dir.join("input.json")));
    assert_eq!(annotation["source_input_sha256"], sha256_hex(&original));
    assert_eq!(annotation["model_sha256"], good["model"]["model_sha256"]);
    assert!(annotation["capture_sources_sha256"].is_null());
    assert!(annotation["silhouettes"].is_null());
    assert_eq!(annotation["starts"], json!([]));
    write(&dir, "annotations.json", &annotation);
    apply(&dir, "empty-applied", 3);
    assert!(!dir.join("empty-applied").exists());
    // A draft-only review must not weaken the ordinary fitter's multistart gate.
    run(&dir, &[], 3);

    annotation["starts"] = desired;
    write(&dir, "annotations.json", &annotation);
    let applied = apply(&dir, "authored", 0)["result"].clone();
    let authored = read(&dir, "authored/input.json");
    let prepared_bytes = std::fs::read(dir.join("authored/prepared-input.json")).unwrap();
    let prepared: Value = serde_json::from_slice(&prepared_bytes).unwrap();
    assert_eq!(prepared["schema"], "ftx-anny-body-fit-input/1");
    assert_eq!(prepared["fit"]["starts"], authored["fit"]["starts"]);
    assert_eq!(prepared["fit"]["frames"], authored["fit"]["frames"]);
    assert_eq!(prepared["model"], review_prepared["model"]);
    assert_eq!(
        prepared["model"]["model_sha256"],
        authored["model"]["model_sha256"]
    );
    assert_eq!(applied["prepared_input"]["file"], "prepared-input.json");
    assert_eq!(applied["prepared_input"]["bytes"], prepared_bytes.len());
    assert_eq!(
        applied["prepared_input"]["sha256"],
        sha256_hex(&prepared_bytes)
    );
    let mut remaining = authored.clone();
    remaining["fit"]["starts"] = json!([]);
    assert_eq!(remaining, empty);
    for (index, start) in authored["fit"]["starts"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(start["shape"], good["fit"]["starts"][index]["shape"]);
        for frame in good["fit"]["frames"].as_array().unwrap() {
            let pose = start["poses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| &p["frame"] == frame_id(frame))
                .unwrap();
            let expected = good["fit"]["starts"][index]["poses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["frame"] == pose["frame"])
                .unwrap();
            assert_eq!(pose, expected);
        }
    }
    assert_eq!(
        authored["fit"]["frames"][1]["frame"]["timestamp_us"].as_u64(),
        Some(u64::MAX - 200_000)
    );
    assert_eq!(
        std::fs::read(dir.join("authored/original-input.json")).unwrap(),
        original
    );
    assert_eq!(std::fs::read(dir.join("input.json")).unwrap(), original);

    let mut single = good;
    single["fit"]["starts"].as_array_mut().unwrap().truncate(1);
    save(&dir, &single);
    let single_review = review(&dir, "single-review", 0)["result"].clone();
    assert_eq!(
        read(&dir, "single-review/body-starts.json")["starts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let model = std::fs::read(dir.join("model.ftxbody")).unwrap();
    let text = std::fs::read_to_string(dir.join("single-review/prepared-input.json")).unwrap();
    let prepared: Value = serde_json::from_str(&text).unwrap();
    for (index, frame) in prepared["fit"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let preview =
            ftx_wasm::body_fit::review_body_fit_start(&model, &text, "0", &index.to_string())
                .unwrap();
        let native = single_review["starts"][0]["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["frame"] == frame["frame"])
            .unwrap();
        assert_eq!(preview["schema"], "ftx-body-start-frame-review/1");
        assert_eq!(preview["status"], "INITIALIZATION_REVIEW");
        assert_eq!(preview["body_fitted"], false);
        assert_eq!(preview["pose_evaluations"], 1);
        assert_eq!(preview["start_count"], 1);
        assert_eq!(preview["frame_count"], 2);
        assert_eq!(preview["start_index"], 0);
        assert_eq!(preview["frame_index"], index);
        assert_eq!(preview["frame"], frame["frame"]);
        assert_eq!(
            preview["frame_label"],
            format!(
                "{} @ {} us",
                frame["frame"]["stream"].as_str().unwrap(),
                frame["frame"]["timestamp_us"].as_u64().unwrap()
            )
        );
        assert_eq!(preview["input_sha256"], sha256_hex(text.as_bytes()));
        assert_eq!(preview["model_sha256"], sha256_hex(&model));
        assert_eq!(preview["shape_parameters"], single_review["model"]["shape"]);
        assert_eq!(preview["pose_parameters"], single_review["model"]["pose"]);
        assert_eq!(preview["shape_values"], single["fit"]["starts"][0]["shape"]);
        for field in [
            "pose_values",
            "camera",
            "keypoints",
            "image_size_px",
            "rotation_cw_deg",
            "unprojectable_vertices",
        ] {
            assert_eq!(preview[field], native[field], "{field}");
        }
        let native_mesh = std::fs::read_to_string(
            dir.join("single-review")
                .join(native["mesh"]["file"].as_str().unwrap()),
        )
        .unwrap();
        assert_eq!(
            preview["mesh_obj"]
                .as_str()
                .unwrap()
                .lines()
                .filter(|line| line.starts_with("v ") || line.starts_with("f "))
                .collect::<Vec<_>>(),
            native_mesh
                .lines()
                .filter(|line| line.starts_with("v ") || line.starts_with("f "))
                .collect::<Vec<_>>()
        );
    }
    run(&dir, &[], 3);
}

#[test]
fn start_edits_editor_fixture_replays_the_complete_capture_through_the_fitter() {
    let dir = if let Some(path) = std::env::var_os("FTX_BODY_START_DIR") {
        let path = PathBuf::from(path);
        std::fs::create_dir(&path).unwrap();
        let (model, input) = fixture();
        std::fs::write(path.join("model.ftxbody"), model).unwrap();
        save(&path, &input);
        path
    } else {
        setup()
    };
    let mut input = read(&dir, "input.json");
    long_frame_ids(&mut input, 9_007_199_254_740_993);
    let good = capture::wrap(&dir, input);
    write(&dir, "good.json", &good);
    command(
        &dir,
        &[
            "measure",
            "fit-start-review",
            "good.json",
            "--model",
            "model.ftxbody",
            "--out",
            "seeded-review",
        ],
        0,
    );
    let mut empty = good.clone();
    empty["fit"]["starts"] = json!([]);
    save(&dir, &empty);
    review(&dir, "review", 0);
    let mut annotation = read(&dir, "review/body-starts.json");
    assert_eq!(
        annotation["capture_sources_sha256"].as_str().unwrap().len(),
        64
    );
    annotation["starts"] = annotations_for(&good);
    write(&dir, "annotations.json", &annotation);
    write(&dir, "expected-annotations.json", &annotation);
    apply(&dir, "authored", 0);
    let authored = read(&dir, "authored/input.json");
    let mut expected = good;
    expected["video_manifest"] = json!(dir.join("capture-frames.json"));
    for (i, frame) in expected["fit"]["frames"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        frame["calibration"] = json!(dir.join("capture-calibration.json"));
        frame["reference"] = json!(dir.join(format!("capture-reference-{i}.json")));
    }
    assert_eq!(authored, expected);
    command(
        &dir.join("authored"),
        &[
            "measure",
            "capture-review",
            "input.json",
            "--out",
            "capture-review",
        ],
        0,
    );
    let fit = command(
        &dir.join("authored"),
        &[
            "measure",
            "fit-body",
            "input.json",
            "--model",
            "../model.ftxbody",
            "--out",
            "expected-fit.json",
        ],
        0,
    )["result"]
        .clone();
    assert_eq!(fit["status"], "ACCEPTED");
    assert_eq!(fit["capture_input"]["input"], authored);
    let prepared_text = std::fs::read_to_string(dir.join("authored/prepared-input.json")).unwrap();
    let prepared: Value = serde_json::from_str(&prepared_text).unwrap();
    assert_eq!(prepared["schema"], "ftx-anny-body-fit-input/1");
    assert!(prepared.get("video_manifest").is_none());
    assert!(prepared.get("silhouettes").is_none());
    assert_eq!(prepared["fit"]["starts"], authored["fit"]["starts"]);
    assert_eq!(
        prepared["fit"]["frames"][0]["frame"]["timestamp_us"].as_u64(),
        Some(9_007_199_254_740_993)
    );
    let portable = ftx_wasm::body_fit::fit_body(
        &std::fs::read(dir.join("model.ftxbody")).unwrap(),
        &prepared_text,
        "50",
        "100000",
    )
    .unwrap();
    let mut expected_portable = fit.clone();
    expected_portable
        .as_object_mut()
        .unwrap()
        .remove("capture_input");
    expected_portable["input_sha256"] = json!(sha256_hex(prepared_text.as_bytes()));
    assert_eq!(portable, expected_portable);
    assert_eq!(prepared["capture_images"], fit["capture_images"]);
    assert_eq!(
        prepared["capture_images"],
        read(&dir, "review/prepared-input.json")["capture_images"]
    );
    let model = std::fs::read(dir.join("model.ftxbody")).unwrap();
    for (index, frame) in prepared["fit"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let preview = ftx_wasm::body_fit::review_body_fit_start(
            &model,
            &prepared_text,
            "0",
            &index.to_string(),
        )
        .unwrap();
        let source = prepared["capture_images"]
            .as_array()
            .unwrap()
            .iter()
            .find(|image| image["frame"] == frame["frame"])
            .unwrap();
        assert_eq!(preview["source_image"], *source);
        assert_eq!(preview["frame"], frame["frame"]);
        let bytes = std::fs::read(dir.join(source["image_name"].as_str().unwrap())).unwrap();
        let image = ftx_wasm::body_fit::BodyFitImage::new(&bytes, &preview.to_string()).unwrap();
        assert_eq!(image.report()["source_image"], *source);
        let other = prepared["capture_images"]
            .as_array()
            .unwrap()
            .iter()
            .find(|image| image["frame"] != frame["frame"])
            .unwrap();
        let wrong = std::fs::read(dir.join(other["image_name"].as_str().unwrap())).unwrap();
        assert!(ftx_wasm::body_fit::BodyFitImage::new(&wrong, &preview.to_string()).is_err());
    }
    let frame_review = ftx_wasm::body_fit::review_body_fit_frame(
        &std::fs::read(dir.join("model.ftxbody")).unwrap(),
        &portable.to_string(),
        "0",
    )
    .unwrap();
    assert_eq!(
        frame_review["source_image"]["frame"]["timestamp_us"].as_u64(),
        Some(9_007_199_254_740_993)
    );
    let image =
        std::fs::read(dir.join(frame_review["source_image"]["image_name"].as_str().unwrap()))
            .unwrap();
    assert!(ftx_wasm::body_fit::BodyFitImage::new(&image, &frame_review.to_string()).is_ok());
    for (value, expected) in fit["solution"]["shape"]
        .as_array()
        .unwrap()
        .iter()
        .zip([0.6, 0.45])
    {
        assert!((value.as_f64().unwrap() - expected).abs() < 1e-5);
    }
    for (name, binding) in [
        ("missing-capture-binding", Value::Null),
        ("stale-capture-binding", json!("0".repeat(64))),
    ] {
        let mut bad = annotation.clone();
        bad["capture_sources_sha256"] = binding;
        write(&dir, "annotations.json", &bad);
        apply(&dir, name, 3);
        assert!(!dir.join(name).exists());
    }
    write(&dir, "annotations.json", &annotation);
    let image_path = dir.join(
        empty["fit"]["frames"][0]["source"]["image"]
            .as_str()
            .unwrap(),
    );
    let original_image = std::fs::read(&image_path).unwrap();
    let changed_image = Gray::filled(1280, 960, 255);
    std::fs::write(&image_path, encode_png_gray(&changed_image)).unwrap();
    apply(&dir, "changed-capture-image", 3);
    assert!(!dir.join("changed-capture-image").exists());
    std::fs::write(&image_path, original_image).unwrap();
    let before = std::fs::read(dir.join("authored/input.json")).unwrap();
    apply(&dir, "authored", 3);
    assert_eq!(
        std::fs::read(dir.join("authored/input.json")).unwrap(),
        before
    );
    assert_eq!(read(&dir, "input.json"), empty);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.join("authored"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for name in [
            "input.json",
            "prepared-input.json",
            "original-input.json",
            "body-starts.json",
            "starts.json",
        ] {
            assert_eq!(
                std::fs::metadata(dir.join("authored").join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn start_edits_refuse_invalid_initializations_and_stale_bindings_before_output() {
    let dir = setup();
    review(&dir, "review", 0);
    let good = read(&dir, "review/body-starts.json");
    let mut cases = Vec::new();
    let mut add = |name: &str, value: Value| cases.push((name.to_owned(), value));
    let mut bad = good.clone();
    bad["starts"].as_array_mut().unwrap().truncate(1);
    add("single", bad);
    let mut bad = good.clone();
    bad["starts"] = json!(vec![bad["starts"][0].clone(); 5]);
    add("too-many", bad);
    let mut bad = good.clone();
    bad["starts"][1] = bad["starts"][0].clone();
    add("duplicate", bad);
    let mut bad = good.clone();
    bad["starts"][0]["shape"][0] = json!(0.0);
    add("shape-boundary", bad);
    let mut bad = good.clone();
    bad["starts"][0]["shape"][0] = Value::Null;
    add("missing-value", bad);
    let mut bad = good.clone();
    bad["starts"][0]["shape"].as_array_mut().unwrap().pop();
    add("shape-length", bad);
    let mut bad = good.clone();
    bad["starts"][0]["poses"][0]["values"][0] = json!(1.0);
    add("pose-boundary", bad);
    let mut bad = good.clone();
    bad["starts"][0]["poses"][0]["frame_index"] = json!(2);
    add("outside-frame", bad);
    let mut bad = good.clone();
    bad["starts"][0]["poses"][0]["frame_index"] = json!(1);
    add("duplicate-frame", bad);
    let mut bad = good.clone();
    bad["starts"][0]["poses"].as_array_mut().unwrap().pop();
    add("missing-frame", bad);
    let mut bad = good.clone();
    bad["starts"][0]["poses"][0]["unexpected"] = json!(true);
    add("unknown-field", bad);
    let mut bad = good.clone();
    bad["model_sha256"] = json!("0".repeat(64));
    add("annotation-model", bad);
    let mut bad = good.clone();
    bad["source_input_sha256"] = json!("0".repeat(64));
    add("annotation-source", bad);
    let mut bad = good.clone();
    bad["source_input"] = json!("input.json");
    add("relative-source", bad);
    let mut bad = good.clone();
    bad["capture_sources_sha256"] = json!("0".repeat(64));
    add("unexpected-capture-binding", bad);
    for (name, bad) in cases {
        write(&dir, "annotations.json", &bad);
        apply(&dir, &name, 3);
        assert!(!dir.join(name).exists());
    }
    write(&dir, "annotations.json", &good);
    let original = std::fs::read(dir.join("input.json")).unwrap();
    let mut changed = original.clone();
    changed.push(b'\n');
    std::fs::write(dir.join("input.json"), changed).unwrap();
    apply(&dir, "changed-source", 3);
    assert!(!dir.join("changed-source").exists());
    std::fs::write(dir.join("input.json"), original).unwrap();
    let mut model = std::fs::read(dir.join("model.ftxbody")).unwrap();
    model.push(0);
    std::fs::write(dir.join("model.ftxbody"), model).unwrap();
    apply(&dir, "changed-model", 3);
    assert!(!dir.join("changed-model").exists());
}

#[test]
fn start_edits_preserve_mixed_inline_and_imported_masks_and_refuse_stale_evidence() {
    let dir = setup();
    let mut masks = mask_manifest(&dir);
    let initial = command(
        &dir,
        &[
            "measure",
            "fit-start-review",
            "input.json",
            "--model",
            "model.ftxbody",
            "--silhouettes",
            "masks/manifest.json",
            "--out",
            "mask-source",
        ],
        0,
    )["result"]
        .clone();
    // Reuse the existing importer to establish an already-inline frame. This test checks
    // transport of its exact constraints; mask_manifest supplies the independent raster.
    let mut input = read(&dir, "input.json");
    input["fit"]["frames"][0]["silhouette"] =
        initial["canonical_input"]["frames"][0]["silhouette"].clone();
    save(&dir, &input);
    let mut manifest = read(&dir, "masks/manifest.json");
    manifest["frames"].as_array_mut().unwrap().remove(0);
    write(&dir, "masks/manifest.json", &manifest);
    let reviewed = command(
        &dir,
        &[
            "measure",
            "fit-start-review",
            "input.json",
            "--model",
            "model.ftxbody",
            "--silhouettes",
            "masks/manifest.json",
            "--out",
            "review",
        ],
        0,
    )["result"]
        .clone();
    let mut annotation = read(&dir, "review/body-starts.json");
    assert_eq!(
        annotation["silhouettes"]["path"],
        json!(dir.join("masks/manifest.json"))
    );
    assert_eq!(
        annotation["silhouettes"]["sha256"].as_str().unwrap().len(),
        64
    );
    let review_prepared = read(&dir, "review/prepared-input.json");
    for frame in review_prepared["fit"]["frames"].as_array().unwrap() {
        let expected = reviewed["canonical_input"]["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["frame"] == frame["frame"])
            .unwrap();
        assert_eq!(frame["silhouette"], expected["silhouette"]);
        assert!(!frame["silhouette"].is_null());
    }
    let preview_text = std::fs::read_to_string(dir.join("review/prepared-input.json")).unwrap();
    let model = std::fs::read(dir.join("model.ftxbody")).unwrap();
    for (index, frame) in review_prepared["fit"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let preview = ftx_wasm::body_fit::review_body_fit_start(
            &model,
            &preview_text,
            "0",
            &index.to_string(),
        )
        .unwrap();
        let native = reviewed["starts"][0]["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["frame"] == frame["frame"])
            .unwrap();
        assert_eq!(
            preview["silhouette"]["evidence"],
            frame["silhouette"]["evidence"]
        );
        let lines = preview["silhouette"]["lines"].as_array().unwrap();
        assert_eq!(
            lines.len(),
            frame["silhouette"]["lines"].as_array().unwrap().len()
        );
        for line in lines {
            assert!(
                frame["silhouette"]["lines"]
                    .as_array()
                    .unwrap()
                    .contains(&line["observation"])
            );
            let expected = native["silhouette_comparisons"]
                .as_array()
                .unwrap()
                .iter()
                .find(|candidate| {
                    candidate["axis"] == line["observation"]["axis"]
                        && candidate["at_px"] == line["observation"]["at_px"]
                })
                .unwrap();
            assert_eq!(line["comparison"], *expected);
        }
    }
    annotation["starts"][0]["shape"][0] = json!(0.62);
    write(&dir, "annotations.json", &annotation);
    apply(&dir, "authored", 0);
    let authored = read(&dir, "authored/input.json");
    let mut expected = input.clone();
    expected["fit"]["starts"][0]["shape"][0] = json!(0.62);
    write(&dir, "reference.json", &expected);
    expected["fit"]["frames"][1]["silhouette"] =
        reviewed["canonical_input"]["frames"][1]["silhouette"].clone();
    assert_eq!(authored, expected);
    assert_eq!(
        authored["fit"]["frames"][0]["silhouette"],
        input["fit"]["frames"][0]["silhouette"]
    );

    let mut reference_fit = command(
        &dir,
        &[
            "measure",
            "fit-body",
            "reference.json",
            "--model",
            "model.ftxbody",
            "--silhouettes",
            "masks/manifest.json",
        ],
        0,
    )["result"]
        .clone();
    let fitted = command(
        &dir.join("authored"),
        &[
            "measure",
            "fit-body",
            "input.json",
            "--model",
            "../model.ftxbody",
        ],
        0,
    )["result"]
        .clone();
    assert_eq!(fitted["status"], "ACCEPTED");
    assert_eq!(
        reference_fit["silhouette_input"],
        reviewed["silhouette_input"]
    );
    assert_eq!(
        reference_fit["input_sha256"],
        sha256_hex(&std::fs::read(dir.join("reference.json")).unwrap())
    );
    assert_eq!(
        fitted["input_sha256"],
        sha256_hex(&std::fs::read(dir.join("authored/input.json")).unwrap())
    );
    reference_fit
        .as_object_mut()
        .unwrap()
        .remove("silhouette_input");
    reference_fit["input_sha256"] = fitted["input_sha256"].clone();
    assert_eq!(fitted, reference_fit);
    let prepared_text = std::fs::read_to_string(dir.join("authored/prepared-input.json")).unwrap();
    let prepared: Value = serde_json::from_str(&prepared_text).unwrap();
    assert_eq!(prepared["fit"]["starts"], authored["fit"]["starts"]);
    let mut expected_prepared = review_prepared;
    expected_prepared["fit"]["starts"] = prepared["fit"]["starts"].clone();
    assert_eq!(prepared, expected_prepared);
    for index in 0..2 {
        assert_eq!(
            prepared["fit"]["frames"][index]["silhouette"],
            authored["fit"]["frames"][index]["silhouette"]
        );
    }
    let portable = ftx_wasm::body_fit::fit_body(
        &std::fs::read(dir.join("model.ftxbody")).unwrap(),
        &prepared_text,
        "50",
        "100000",
    )
    .unwrap();
    let mut expected_portable = fitted;
    expected_portable["input_sha256"] = json!(sha256_hex(prepared_text.as_bytes()));
    assert_eq!(portable, expected_portable);

    let original_manifest = std::fs::read(dir.join("masks/manifest.json")).unwrap();
    let mut changed_manifest = original_manifest.clone();
    changed_manifest.push(b'\n');
    std::fs::write(dir.join("masks/manifest.json"), changed_manifest).unwrap();
    apply(&dir, "stale-mask-manifest", 3);
    assert!(!dir.join("stale-mask-manifest").exists());
    std::fs::write(dir.join("masks/manifest.json"), original_manifest).unwrap();
    masks[1].data[0] = 255;
    std::fs::write(dir.join("masks/mask-1.png"), encode_png_gray(&masks[1])).unwrap();
    apply(&dir, "stale-mask-pixels", 3);
    assert!(!dir.join("stale-mask-pixels").exists());
    assert_eq!(read(&dir, "input.json"), input);
}

#[test]
fn portable_start_authoring_prescribes_and_releases_per_frame_fixed_poses_with_refusals() {
    use ftx_wasm::body_fit::{
        body_fit_starts, fit_body, review_body_fit_frame, review_body_fit_start,
    };

    let (model, mut original) = fixture();
    long_frame_ids(&mut original, u64::MAX - 200_000);
    original["fit"]["frames"].as_array_mut().unwrap().reverse();
    original["fit"]["starts"][0]["poses"]
        .as_array_mut()
        .unwrap()
        .reverse();
    // Retain the fixture's predeclared held-out CHECK keypoint (`p3`) to confirm fixed-pose
    // authoring never alters CHECK roles or full-width frame identities.
    assert_eq!(
        original["fit"]["frames"][1]["keypoints"][3]["partition"],
        json!("held_out")
    );
    let frame_count = original["fit"]["frames"].as_array().unwrap().len();
    let original_frames = original["fit"]["frames"].clone();
    let desired_starts = annotations_for(&original);

    // 1. Zero-start draft can prescribe per-frame fixed poses before any start exists.
    let mut zero_draft = original.clone();
    zero_draft["fit"]["starts"] = json!([]);
    let zero_text = serde_json::to_string_pretty(&zero_draft).unwrap();
    let opened_zero = body_fit_starts(&model, &zero_text, "").unwrap();
    let one_value = |name: &str, value: f64| {
        let mut map = serde_json::Map::new();
        map.insert(name.to_owned(), json!(value));
        Value::Object(map)
    };
    let pose_name = opened_zero["pose"][0]["name"].as_str().unwrap().to_owned();
    let pose_lower = opened_zero["pose"][0]["lower"].as_f64().unwrap();
    let pose_upper = opened_zero["pose"][0]["upper"].as_f64().unwrap();
    // Reversed frame 0 is fixture pose[1], whose true root_x_m is 0.2 m.
    let fixed_val = 0.2_f64;
    let expected_fixed = json!({
        "values": one_value(&pose_name, fixed_val),
        "evidence": "Caller floor tape mark for frame 1"
    });
    let mut fixed_entries: Vec<Value> = (0..frame_count)
        .map(|idx| {
            if idx == 0 {
                json!({
                    "frame_index": idx,
                    "fixed_pose": expected_fixed,
                })
            } else {
                json!({"frame_index": idx, "fixed_pose": Value::Null})
            }
        })
        .collect();
    // Reverse edit array order to prove joining happens strictly by original frame_index ordinal.
    fixed_entries.reverse();
    let zero_fixed_edits = json!({
        "input_sha256": opened_zero["input_sha256"],
        "model_sha256": opened_zero["model_sha256"],
        "starts": [],
        "fixed_poses": fixed_entries,
    });
    let applied_zero = body_fit_starts(&model, &zero_text, &zero_fixed_edits.to_string()).unwrap();
    assert_eq!(applied_zero["starts"], json!([]));
    assert_eq!(applied_zero["frames"][0]["fixed_pose"], expected_fixed);
    assert!(applied_zero["frames"][1].get("fixed_pose").is_none());
    let zero_edited_text = applied_zero["input"].as_str().unwrap();
    let zero_edited_val: Value = serde_json::from_str(zero_edited_text).unwrap();
    assert_eq!(
        zero_edited_val["fit"]["frames"][0]["frame"]["timestamp_us"].as_u64(),
        Some(u64::MAX)
    );
    assert_eq!(
        zero_edited_val["fit"]["frames"][1]["keypoints"][3]["partition"],
        json!("held_out")
    );

    // 2. Apply reconciled starts on top of the fixed pose, verify preview, fit, and frame review.
    let mut reconciled = desired_starts.clone();
    for start in reconciled.as_array_mut().unwrap() {
        for pose in start["poses"].as_array_mut().unwrap() {
            if pose["frame_index"] == 0 {
                pose["values"][0] = json!(fixed_val);
            }
        }
    }
    let with_starts_edits = json!({
        "input_sha256": applied_zero["input_sha256"],
        "model_sha256": applied_zero["model_sha256"],
        "starts": reconciled,
    });
    let with_starts =
        body_fit_starts(&model, zero_edited_text, &with_starts_edits.to_string()).unwrap();
    let fitted_input = with_starts["input"].as_str().unwrap();
    let start_preview = review_body_fit_start(&model, fitted_input, "0", "0").unwrap();
    assert_eq!(start_preview["fixed_pose"], expected_fixed);
    let fitted = fit_body(&model, fitted_input, "60", "20000").unwrap();
    assert_eq!(fitted["status"], "ACCEPTED");
    // Fixed coordinate retains zero spread by constraint, not inferred measurement precision.
    let frame0_spread = fitted["pose_spread_fraction"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["frame"]["timestamp_us"].as_u64() == Some(u64::MAX))
        .unwrap();
    assert_eq!(frame0_spread["fractions"][0], json!(0.0));
    // review_body_fit_frame uses the canonical sorted frame order (reversed frame 0 has u64::MAX -> index 1).
    let canonical_idx = fitted["input"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .position(|f| f["frame"]["timestamp_us"].as_u64() == Some(u64::MAX))
        .unwrap();
    let frame_review = review_body_fit_frame(
        &model,
        &serde_json::to_string(&fitted).unwrap(),
        &canonical_idx.to_string(),
    )
    .unwrap();
    assert_eq!(frame_review["fixed_pose"], expected_fixed);

    // 2b. A wrong caller-supplied fixed pose still passes authoring (it is a caller assumption)
    //     but is rejected by the real image checks during fitting.
    let wrong_val = 0.35_f64;
    let mut wrong_starts = desired_starts.clone();
    for start in wrong_starts.as_array_mut().unwrap() {
        for pose in start["poses"].as_array_mut().unwrap() {
            if pose["frame_index"] == 0 {
                pose["values"][0] = json!(wrong_val);
            }
        }
    }
    let wrong_fixed: Vec<Value> = (0..frame_count)
        .map(|idx| {
            if idx == 0 {
                json!({
                    "frame_index": idx,
                    "fixed_pose": {
                        "values": one_value(&pose_name, wrong_val),
                        "evidence": "Misplaced caller mark"
                    }
                })
            } else {
                json!({"frame_index": idx, "fixed_pose": Value::Null})
            }
        })
        .collect();
    let wrong_applied = body_fit_starts(
        &model,
        fitted_input,
        &json!({
            "input_sha256": with_starts["input_sha256"],
            "model_sha256": with_starts["model_sha256"],
            "starts": wrong_starts,
            "fixed_poses": wrong_fixed,
        })
        .to_string(),
    )
    .unwrap();
    let wrong_fit = fit_body(
        &model,
        wrong_applied["input"].as_str().unwrap(),
        "60",
        "20000",
    )
    .unwrap();
    assert_eq!(
        wrong_fit["status"], "REJECTED",
        "wrong caller-supplied fixed pose must not pass image residual acceptance"
    );

    // 3. Releasing fixed_pose removes the field from raw JSON and preserves every other field.
    let release_entries: Vec<Value> = (0..frame_count)
        .map(|idx| json!({"frame_index": idx, "fixed_pose": Value::Null}))
        .collect();
    let release_edits = json!({
        "input_sha256": with_starts["input_sha256"],
        "model_sha256": with_starts["model_sha256"],
        "starts": desired_starts,
        "fixed_poses": release_entries,
    });
    let released = body_fit_starts(&model, fitted_input, &release_edits.to_string()).unwrap();
    assert!(released["frames"][0].get("fixed_pose").is_none());
    let released_val: Value = serde_json::from_str(released["input"].as_str().unwrap()).unwrap();
    assert!(released_val["fit"]["frames"][0].get("fixed_pose").is_none());
    assert_eq!(released_val["fit"]["frames"], original_frames);

    // 4. Refusals: missing/stale hashes, blank/oversized evidence, empty/unknown/out-of-domain
    //    fixed values, bad frame ordinals, and unreconciled start disagreement.
    let base_text = serde_json::to_string_pretty(&original).unwrap();
    let opened = body_fit_starts(&model, &base_text, "").unwrap();
    let ok_fixed: Vec<Value> = (0..frame_count)
        .map(|idx| {
            if idx == 0 {
                json!({
                    "frame_index": idx,
                    "fixed_pose": {
                        "values": one_value(&pose_name, fixed_val),
                        "evidence": "Explicit mark"
                    }
                })
            } else {
                json!({"frame_index": idx, "fixed_pose": Value::Null})
            }
        })
        .collect();
    let valid_fixed_req = json!({
        "input_sha256": opened["input_sha256"],
        "model_sha256": opened["model_sha256"],
        "starts": reconciled,
        "fixed_poses": ok_fixed,
    });
    assert!(body_fit_starts(&model, &base_text, &valid_fixed_req.to_string()).is_ok());

    for (label, bad) in [
        ("missing_hashes", {
            let mut v = valid_fixed_req.clone();
            v.as_object_mut().unwrap().remove("input_sha256");
            v.as_object_mut().unwrap().remove("model_sha256");
            v
        }),
        ("stale_input_hash", {
            let mut v = valid_fixed_req.clone();
            v["input_sha256"] = json!("0".repeat(64));
            v
        }),
        ("stale_model_hash", {
            let mut v = valid_fixed_req.clone();
            v["model_sha256"] = json!("0".repeat(64));
            v
        }),
        ("blank_evidence", {
            let mut v = valid_fixed_req.clone();
            v["fixed_poses"][0]["fixed_pose"]["evidence"] = json!("   ");
            v
        }),
        ("oversized_evidence", {
            let mut v = valid_fixed_req.clone();
            v["fixed_poses"][0]["fixed_pose"]["evidence"] = json!("x".repeat(4097));
            v
        }),
        ("empty_values", {
            let mut v = valid_fixed_req.clone();
            v["fixed_poses"][0]["fixed_pose"]["values"] = json!({});
            v
        }),
        ("unknown_pose_param", {
            let mut v = valid_fixed_req.clone();
            v["fixed_poses"][0]["fixed_pose"]["values"] = json!({"not_a_pose_param": 0.0});
            v
        }),
        ("boundary_lower", {
            let mut v = valid_fixed_req.clone();
            v["fixed_poses"][0]["fixed_pose"]["values"] = one_value(&pose_name, pose_lower);
            v
        }),
        ("boundary_upper", {
            let mut v = valid_fixed_req.clone();
            v["fixed_poses"][0]["fixed_pose"]["values"] = one_value(&pose_name, pose_upper);
            v
        }),
        ("missing_frame_entry", {
            let mut v = valid_fixed_req.clone();
            v["fixed_poses"].as_array_mut().unwrap().pop();
            v
        }),
        ("duplicate_frame_index", {
            let mut v = valid_fixed_req.clone();
            v["fixed_poses"][1]["frame_index"] = json!(0);
            v
        }),
        ("oob_frame_index", {
            let mut v = valid_fixed_req.clone();
            v["fixed_poses"][0]["frame_index"] = json!(frame_count);
            v
        }),
        ("unreconciled_start_disagreement", {
            let mut v = valid_fixed_req.clone();
            let shifted = (fixed_val + 0.05).min(pose_upper - 0.01);
            v["starts"][1]["poses"][0]["values"][0] = json!(shifted);
            v
        }),
        ("extra_fixed_pose_field", {
            let mut v = valid_fixed_req.clone();
            v["fixed_poses"][0]["extra"] = json!(true);
            v
        }),
    ] {
        assert!(
            body_fit_starts(&model, &base_text, &bad.to_string()).is_err(),
            "fixed pose refusal case must fail closed: {label}"
        );
    }
}

#[test]
fn native_fit_starts_materializes_and_releases_per_frame_fixed_poses_with_refusals() {
    let dir = setup();
    let mut input = read(&dir, "input.json");
    long_frame_ids(&mut input, u64::MAX - 200_000);
    input["fit"]["frames"].as_array_mut().unwrap().reverse();
    input["fit"]["starts"][0]["poses"]
        .as_array_mut()
        .unwrap()
        .reverse();
    write(&dir, "input.json", &input);

    let rev = review(&dir, "rev", 0);
    assert_eq!(rev["ok"], true);
    assert_eq!(rev["result"]["body_fitted"], false);
    let mut ann = read(&dir, "rev/body-starts.json");
    // Reversed frame 0 is fixture pose[1] (true root_x_m = 0.2 m).
    let fixed_val = 0.2_f64;
    for start in ann["starts"].as_array_mut().unwrap() {
        for pose in start["poses"].as_array_mut().unwrap() {
            if pose["frame_index"] == 0 {
                pose["values"][0] = json!(fixed_val);
            }
        }
    }
    // Reverse order to prove joining uses original frame_index ordinals.
    ann["fixed_poses"] = json!([
        {"frame_index": 1, "fixed_pose": Value::Null},
        {
            "frame_index": 0,
            "fixed_pose": {
                "values": {"root_x_m": fixed_val},
                "evidence": "CLI floor tape mark for frame 1"
            }
        }
    ]);
    write(&dir, "annotations.json", &ann);
    let applied = apply(&dir, "applied", 0);
    assert_eq!(applied["result"]["status"], "STARTS_APPLIED");
    assert!(
        applied["result"]["no_claim"]
            .as_str()
            .unwrap()
            .contains("fixed pose")
    );
    let materialized = read(&dir, "applied/input.json");
    assert_eq!(
        materialized["fit"]["frames"][0]["fixed_pose"],
        json!({"values": {"root_x_m": fixed_val}, "evidence": "CLI floor tape mark for frame 1"})
    );
    assert!(materialized["fit"]["frames"][1].get("fixed_pose").is_none());
    assert_eq!(
        materialized["fit"]["frames"][0]["frame"]["timestamp_us"].as_u64(),
        Some(u64::MAX)
    );
    assert_eq!(
        materialized["fit"]["frames"][1]["keypoints"][3]["partition"],
        json!("held_out")
    );

    // Release fixed pose from the materialized input and confirm fixed_pose is removed.
    let rev2 = command(
        &dir,
        &[
            "measure",
            "fit-start-review",
            "applied/input.json",
            "--model",
            "model.ftxbody",
            "--out",
            "rev2",
        ],
        0,
    );
    assert_eq!(rev2["ok"], true);
    assert_eq!(rev2["result"]["body_fitted"], false);
    let mut release = read(&dir, "rev2/body-starts.json");
    release["fixed_poses"] = json!([
        {"frame_index": 0, "fixed_pose": Value::Null},
        {"frame_index": 1, "fixed_pose": Value::Null},
    ]);
    write(&dir, "annotations.json", &release);
    apply(&dir, "released", 0);
    let released_input = read(&dir, "released/input.json");
    assert!(
        released_input["fit"]["frames"][0]
            .get("fixed_pose")
            .is_none()
    );
    assert_eq!(
        released_input["fit"]["frames"], input["fit"]["frames"],
        "releasing fixed_pose must restore exact original frame records"
    );

    // Refusals: blank evidence, missing frame entry, duplicate ordinal, unreconciled start disagreement.
    for (idx, bad) in [
        {
            let mut v = ann.clone();
            v["fixed_poses"][1]["fixed_pose"]["evidence"] = json!("   ");
            v
        },
        {
            let mut v = ann.clone();
            v["fixed_poses"].as_array_mut().unwrap().pop();
            v
        },
        {
            let mut v = ann.clone();
            v["fixed_poses"][0]["frame_index"] = json!(0);
            v
        },
        {
            let mut v = ann.clone();
            v["starts"][1]["poses"][0]["values"][0] = json!(0.25);
            v
        },
    ]
    .into_iter()
    .enumerate()
    {
        write(&dir, "annotations.json", &bad);
        let out_name = format!("bad-{idx}");
        apply(&dir, &out_name, 3);
        assert!(!dir.join(&out_name).exists());
    }
}
