//! Real target-authoring commands, sharing the parent test's synthetic articulated model.

use super::*;

fn read(dir: &Path, name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(name)).unwrap()).unwrap()
}

fn write(dir: &Path, name: &str, value: &Value) {
    std::fs::write(dir.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn command(dir: &Path, arguments: &[&str], code: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(arguments)
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

fn review(dir: &Path, out: &str, code: i32) -> Value {
    command(
        dir,
        &[
            "measure",
            "body-target-review",
            "input.json",
            "--model",
            "model.ftxbody",
            "--out",
            out,
        ],
        code,
    )
}

fn apply(dir: &Path, out: &str, code: i32) -> Value {
    command(
        dir,
        &[
            "measure",
            "body-targets",
            "annotations.json",
            "--model",
            "model.ftxbody",
            "--out",
            out,
        ],
        code,
    )
}

#[test]
fn targets_bootstrap_empty_map_without_rounding_source_timestamps() {
    use ftx_wasm::body_fit::body_fit_targets;
    let dir = setup();
    let mut input = read(&dir, "input.json");
    let original_targets = input["model"]["keypoints"].clone();
    for i in 0..2 {
        let timestamp = 9_007_199_254_740_993_u64 + i as u64 * 200_000;
        input["fit"]["frames"][i]["frame"]["timestamp_us"] = json!(timestamp);
        for start in input["fit"]["starts"].as_array_mut().unwrap() {
            start["poses"][i]["frame"]["timestamp_us"] = json!(timestamp);
        }
    }
    input["model"]["keypoints"] = json!({});
    save(&dir, &input);
    let source_bytes = std::fs::read(dir.join("input.json")).unwrap();
    let model = std::fs::read(dir.join("model.ftxbody")).unwrap();
    let source = std::str::from_utf8(&source_bytes).unwrap();
    let preview = body_fit_targets(&model, source, "").unwrap();
    assert_eq!(preview["target_count"], 0);
    assert_eq!(preview["input"], source);
    let html = preview["editor_html"].as_str().unwrap();
    let data: Value = serde_json::from_str(
        html.split("<script type=\"application/json\" id=\"targetData\">")
            .nth(1)
            .unwrap()
            .split("</script>")
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(data["observed_names"].as_array().unwrap().len(), 10);
    assert_eq!(data["annotations"]["keypoints"], json!({}));
    let mut edits = json!({"input_sha256":preview["input_sha256"],
        "model_sha256":preview["model_sha256"],"keypoints":original_targets});
    edits["keypoints"].as_object_mut().unwrap().remove("p0");
    assert!(body_fit_targets(&model, source, &edits.to_string()).is_err());
    edits["keypoints"] = original_targets.clone();
    let repaired = body_fit_targets(&model, source, &edits.to_string()).unwrap();
    let mut expected = input.clone();
    expected["model"]["keypoints"] = original_targets.clone();
    assert_eq!(
        serde_json::from_str::<Value>(repaired["input"].as_str().unwrap()).unwrap(),
        expected
    );
    for field in ["camera", "sigma", "start"] {
        let mut invalid = input.clone();
        match field {
            "camera" => {
                invalid["fit"]["frames"][0]["camera"]["intrinsics"]["focal_px"] = json!([0, 0])
            }
            "sigma" => invalid["fit"]["frames"][0]["keypoints"][0]["sigma_px"] = json!(0),
            _ => invalid["fit"]["starts"][0]["shape"][0] = json!(0),
        }
        assert!(
            body_fit_targets(&model, &invalid.to_string(), "").is_err(),
            "{field}"
        );
    }
    review(&dir, "review", 0);
    assert_eq!(
        std::fs::read(dir.join("review/source-input.json")).unwrap(),
        source_bytes
    );
    let mut annotations = read(&dir, "review/body-targets.json");
    assert_eq!(annotations["schema"], "ftx-body-targets/1");
    assert_eq!(annotations["source_input"], json!(dir.join("input.json")));
    assert_eq!(
        annotations["source_input_sha256"],
        sha256_hex(&source_bytes)
    );
    assert_eq!(annotations["model_sha256"], input["model"]["model_sha256"]);
    assert_eq!(annotations["keypoints"], json!({}));
    annotations["keypoints"] = original_targets.clone();
    write(&dir, "annotations.json", &annotations);
    apply(&dir, "authored", 0);
    input["model"]["keypoints"] = original_targets;
    let reconstructed = read(&dir, "authored/input.json");
    assert_eq!(reconstructed, input);
    assert_eq!(
        reconstructed["fit"]["frames"][0]["frame"]["timestamp_us"].as_u64(),
        Some(9_007_199_254_740_993)
    );
    assert_eq!(
        std::fs::read(dir.join("authored/source-input.json")).unwrap(),
        source_bytes
    );
    assert_eq!(
        std::fs::read(dir.join("authored/body-targets.json")).unwrap(),
        std::fs::read(dir.join("annotations.json")).unwrap()
    );
    assert_eq!(std::fs::read(dir.join("input.json")).unwrap(), source_bytes);
}

#[test]
fn targets_editor_fixture_replays_a_corrected_capture_through_the_ordinary_fitter() {
    // Retain this exact tiny case for the actual browser download -> ordinary native replay.
    let dir = if let Some(path) = std::env::var_os("FTX_BODY_TARGET_DIR") {
        let path = PathBuf::from(path);
        std::fs::create_dir(&path).unwrap();
        let (model, input) = fixture();
        std::fs::write(path.join("model.ftxbody"), model).unwrap();
        save(&path, &input);
        path
    } else {
        setup()
    };
    let mut original = read(&dir, "input.json");
    for i in 0..2 {
        let timestamp = 9_007_199_254_740_993_u64 + i as u64 * 200_000;
        original["fit"]["frames"][i]["frame"]["timestamp_us"] = json!(timestamp);
        for start in original["fit"]["starts"].as_array_mut().unwrap() {
            start["poses"][i]["frame"]["timestamp_us"] = json!(timestamp);
        }
    }
    let mut good = capture::wrap(&dir, original);
    let hostile = "</script><script>globalThis.targetInjection=1</script>&\"";
    let target = good["model"]["keypoints"]
        .as_object_mut()
        .unwrap()
        .remove("p9")
        .unwrap();
    good["model"]["keypoints"][hostile] = target;
    for frame in good["fit"]["frames"].as_array_mut().unwrap() {
        frame["keypoints"][9]["name"] = json!(hostile);
    }
    write(&dir, "good.json", &good);
    let mut incomplete = good.clone();
    incomplete["model"]["keypoints"]
        .as_object_mut()
        .unwrap()
        .remove("p0");
    save(&dir, &incomplete);
    review(&dir, "review", 0);
    let page = std::fs::read_to_string(dir.join("review/index.html")).unwrap();
    assert!(!page.contains("<script>globalThis.targetInjection"));
    for control in ["targetDraftFile", "targetDraftDownload", "targetDownload"] {
        assert!(page.contains(&format!("id=\"{control}\"")));
    }
    let mut annotations = read(&dir, "review/body-targets.json");
    write(&dir, "annotations.json", &annotations);
    apply(&dir, "incomplete", 3);
    assert!(!dir.join("incomplete").exists());
    annotations["keypoints"] = good["model"]["keypoints"].clone();
    write(&dir, "annotations.json", &annotations);
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
    assert_eq!(
        authored["fit"]["frames"][0]["source"]["metadata"]["frame"]["timestamp_us"].as_u64(),
        Some(9_007_199_254_740_993)
    );
    // Resolve all rebased paths from the output directory, not the original source cwd.
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
    for (value, expected) in fit["solution"]["shape"]
        .as_array()
        .unwrap()
        .iter()
        .zip([0.6, 0.45])
    {
        assert!((value.as_f64().unwrap() - expected).abs() < 1e-5);
    }
    assert_eq!(fit["capture_input"]["input"], authored);
    assert_eq!(
        fit["anny_configuration"]["keypoints"],
        expected["model"]["keypoints"]
    );
    assert_eq!(read(&dir, "input.json"), incomplete);
}

#[test]
fn targets_annotations_refuse_stale_sources_and_invalid_model_locations_before_output() {
    use ftx_wasm::body_fit::body_fit_targets;
    let dir = setup();
    review(&dir, "review", 0);
    let good = read(&dir, "review/body-targets.json");
    let model_bytes = std::fs::read(dir.join("model.ftxbody")).unwrap();
    let source = std::fs::read_to_string(dir.join("input.json")).unwrap();
    let cases = [
        ("missing-observation", Value::Null),
        ("outside-mesh", json!({"kind":"vertex","index":10})),
        ("duplicate-vertex", json!({"kind":"vertex","index":1})),
        (
            "surface-alias",
            json!({"kind":"surface","face":0,"barycentric":[0,1,0]}),
        ),
        (
            "negative-weight",
            json!({"kind":"surface","face":0,"barycentric":[-0.1,0.6,0.5]}),
        ),
        (
            "unnormalized-weight",
            json!({"kind":"surface","face":0,"barycentric":[0.1,0.2,0.3]}),
        ),
        (
            "outside-face",
            json!({"kind":"surface","face":13,"barycentric":[0.2,0.3,0.5]}),
        ),
        ("unknown-bone", json!({"kind":"bone_head","bone":"absent"})),
    ];
    for (name, target) in cases {
        let mut invalid = good.clone();
        if target.is_null() {
            invalid["keypoints"].as_object_mut().unwrap().remove("p0");
        } else {
            invalid["keypoints"]["p0"] = target;
        }
        write(&dir, "annotations.json", &invalid);
        apply(&dir, name, 3);
        assert!(!dir.join(name).exists(), "{name} published an output");
        let edits = json!({"input_sha256":good["source_input_sha256"],
            "model_sha256":good["model_sha256"],"keypoints":invalid["keypoints"]});
        assert!(
            body_fit_targets(&model_bytes, &source, &edits.to_string()).is_err(),
            "{name}"
        );
    }
    let mut invalid = good.clone();
    invalid["model_sha256"] = json!("0".repeat(64));
    write(&dir, "annotations.json", &invalid);
    apply(&dir, "wrong-model", 3);
    assert!(!dir.join("wrong-model").exists());
    write(&dir, "annotations.json", &good);
    let bytes = std::fs::read(dir.join("input.json")).unwrap();
    let mut changed = bytes.clone();
    changed.push(b'\n');
    std::fs::write(dir.join("input.json"), changed).unwrap();
    apply(&dir, "changed-source", 3);
    assert!(!dir.join("changed-source").exists());
    std::fs::write(dir.join("input.json"), &bytes).unwrap();
    for (i, reference) in [json!(17), json!([]), Value::Null].into_iter().enumerate() {
        let mut malformed: Value = serde_json::from_slice(&bytes).unwrap();
        malformed["schema"] = json!("ftx-anny-capture-input/1");
        malformed["video_manifest"] = json!("frames.json");
        for frame in malformed["fit"]["frames"].as_array_mut().unwrap() {
            frame["calibration"] = json!("calibration.json");
            frame["reference"] = reference.clone();
        }
        save(&dir, &malformed);
        let review_dir = format!("malformed-reference-{i}-review");
        review(&dir, &review_dir, 0);
        let annotations = read(&dir, &format!("{review_dir}/body-targets.json"));
        write(&dir, "annotations.json", &annotations);
        let out = format!("malformed-reference-{i}");
        apply(&dir, &out, 3);
        assert!(!dir.join(out).exists());
    }
    std::fs::write(dir.join("input.json"), bytes).unwrap();
    write(&dir, "annotations.json", &good);
    let mut model = std::fs::read(dir.join("model.ftxbody")).unwrap();
    model.push(0);
    std::fs::write(dir.join("model.ftxbody"), model).unwrap();
    apply(&dir, "changed-model", 3);
    assert!(!dir.join("changed-model").exists());
}

#[test]
fn targets_outputs_are_private_create_only_and_accept_all_native_target_kinds() {
    use ftx_wasm::body_fit::body_fit_targets;
    let dir = setup();
    let original = std::fs::read(dir.join("input.json")).unwrap();
    review(&dir, "review", 0);
    let mut annotations = read(&dir, "review/body-targets.json");
    annotations["keypoints"]["surface"] =
        json!({"kind":"surface","face":2,"barycentric":[0.2,0.3,0.5]});
    annotations["keypoints"]["head"] = json!({"kind":"bone_head","bone":"arm"});
    annotations["keypoints"]["tail"] = json!({"kind":"bone_tail","bone":"arm"});
    write(&dir, "annotations.json", &annotations);
    apply(&dir, "authored", 0);
    assert_eq!(
        read(&dir, "authored/input.json")["model"]["keypoints"],
        annotations["keypoints"]
    );
    let model = std::fs::read(dir.join("model.ftxbody")).unwrap();
    let source = std::str::from_utf8(&original).unwrap();
    let portable = body_fit_targets(&model, source, "").unwrap();
    let html = portable["editor_html"].as_str().unwrap();
    let payload = html
        .split("<script type=\"application/json\" id=\"targetData\">")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    let editor: Value = serde_json::from_str(payload).unwrap();
    let native = read(&dir, "review/model.json");
    for key in ["mesh", "bones", "preview"] {
        assert_eq!(editor[key], native[key], "{key}");
    }
    assert_eq!(editor["annotations"]["source_input"], "");
    assert!(
        !html.contains("src=\"editor.js\""),
        "portable review has no external script"
    );
    let edits = json!({"input_sha256":portable["input_sha256"],
        "model_sha256":portable["model_sha256"],"keypoints":annotations["keypoints"]});
    let edited = body_fit_targets(&model, source, &edits.to_string()).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(edited["input"].as_str().unwrap()).unwrap(),
        read(&dir, "authored/input.json")
    );
    assert!(body_fit_targets(&model, &(source.to_owned() + "\n"), &edits.to_string()).is_err());
    let mut stale = edits.clone();
    stale["model_sha256"] = json!("0".repeat(64));
    assert!(body_fit_targets(&model, source, &stale.to_string()).is_err());
    stale = edits.clone();
    stale["keypoints"] = json!({});
    assert!(body_fit_targets(&model, source, &stale.to_string()).is_err());
    stale = edits;
    stale["fit"] = json!({"starts":[]});
    assert!(body_fit_targets(&model, source, &stale.to_string()).is_err());
    let hostile = "__FTX_TARGET_DATA____FTX_TARGET_SCRIPT____FTX_TARGET_CSP__</script><script>globalThis.targetInjection=1</script>";
    let mut labelled: Value = serde_json::from_str(source).unwrap();
    let target = labelled["model"]["keypoints"]
        .as_object_mut()
        .unwrap()
        .remove("p9")
        .unwrap();
    labelled["model"]["keypoints"][hostile] = target.clone();
    for frame in labelled["fit"]["frames"].as_array_mut().unwrap() {
        frame["keypoints"][9]["name"] = json!(hostile);
    }
    let preview = body_fit_targets(&model, &labelled.to_string(), "").unwrap();
    let html = preview["editor_html"].as_str().unwrap();
    assert!(!html.contains("<script>globalThis.targetInjection"));
    let data: Value = serde_json::from_str(
        html.split("<script type=\"application/json\" id=\"targetData\">")
            .nth(1)
            .unwrap()
            .split("</script>")
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(data["annotations"]["keypoints"][hostile], target);
    review(&dir, "review", 3);
    review(&dir, "input.json", 3);
    apply(&dir, "authored", 3);
    assert_eq!(std::fs::read(dir.join("input.json")).unwrap(), original);
    #[cfg(unix)]
    {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};
        symlink(dir.join("input.json"), dir.join("source-link")).unwrap();
        review(&dir, "source-link", 3);
        for name in ["review", "authored"] {
            let path = dir.join(name);
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700
            );
            for entry in std::fs::read_dir(path).unwrap() {
                assert_eq!(
                    entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
    }
}
