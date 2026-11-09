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
    assert!(ftx_wasm::body_fit::fit_body(&model, text, "50", "10000").is_err());
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
