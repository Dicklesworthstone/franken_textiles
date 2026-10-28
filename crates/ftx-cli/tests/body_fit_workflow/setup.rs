//! First-capture construction from selected pixels and explicit controls, before a request exists.

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

fn sources(retain: bool, first_timestamp: u64) -> (PathBuf, Value) {
    let dir = if let Some(path) = retain
        .then(|| std::env::var_os("FTX_CAPTURE_SETUP_DIR"))
        .flatten()
    {
        PathBuf::from(path)
    } else {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ftx-capture-setup-{}-{stamp}", std::process::id()))
    };
    std::fs::create_dir(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    // This is the existing actual ChArUco acquisition fixture's physical scale and raster.
    // Do not use setup(): a pre-existing capture/model request is exactly what is absent.
    let (model, mut truth) = fixture_scaled(0.25);
    for i in 0..2 {
        let id = json!({"stream":"synthetic-moving-body","timestamp_us":first_timestamp + i as u64 * 200_000});
        truth["fit"]["frames"][i]["frame"] = id.clone();
        for start in truth["fit"]["starts"].as_array_mut().unwrap() {
            start["poses"][i]["frame"] = id.clone();
        }
    }
    std::fs::write(dir.join("model.ftxbody"), model).unwrap();
    capture::wrap_images(&dir, &mut truth, true);
    let reference = &truth["fit"]["frames"][0]["reference"];
    assert_eq!(reference, &truth["fit"]["frames"][1]["reference"]);
    write(&dir, "reference.json", reference);
    assert!(!dir.join("input.json").exists());
    assert!(!dir.join("model.json").exists());
    (dir, truth)
}

fn review(dir: &Path, output: &str, code: i32) -> Value {
    command(
        dir,
        &[
            "measure",
            "capture-init-review",
            "capture-frames.json",
            "--model",
            "model.ftxbody",
            "--calibration",
            "capture-calibration.json",
            "--reference",
            "reference.json",
            "--reference-id",
            "synthetic-fixed-sheet",
            "--out",
            output,
        ],
        code,
    )
}

fn construct(dir: &Path, output: &str, code: i32) -> Value {
    command(
        dir,
        &[
            "measure",
            "capture-init",
            "choices.json",
            "--model",
            "model.ftxbody",
            "--out",
            output,
        ],
        code,
    )
}

fn choices(dir: &Path, review_dir: &str, truth: &Value) -> Value {
    let mut choices = read(dir, &format!("{review_dir}/capture-choices.json"));
    assert_eq!(choices["schema"], "ftx-capture-choices/1");
    assert_eq!(
        choices["source_setup"],
        json!(dir.join(review_dir).join("setup.json"))
    );
    assert_eq!(
        choices["source_setup_sha256"],
        sha256_hex(&std::fs::read(dir.join(review_dir).join("setup.json")).unwrap())
    );
    choices["frame_indices"] = json!([0, 1]);
    choices["age_years"] = truth["model"]["age_years"].clone();
    choices["gender"] = truth["model"]["gender"].clone();
    let fit = |p: &Value| json!({"name":p["name"],"choice":{"mode":"fit","lower":p["lower"],"upper":p["upper"]}});
    choices["shapes"] = json!(
        truth["model"]["shape_parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(fit)
            .collect::<Vec<_>>()
    );
    let pose = truth["model"]["pose_parameters"].as_array().unwrap();
    choices["roots"] = json!(pose[..6].iter().map(fit).collect::<Vec<_>>());
    assert_eq!(pose[6]["name"], "joint:arm");
    choices["joints"] = json!([{"bone":"arm","axis":[1.0,0.0,0.0],
        "choice":{"mode":"fit","lower":pose[6]["lower"],"upper":pose[6]["upper"]}}]);
    choices["metric_scale_source"] = truth["fit"]["metric_scale_source"].clone();
    choices["observation_evidence"] = truth["fit"]["observation_evidence"].clone();
    choices["measurements"] = truth["fit"]["measurements"].clone();
    choices
}

fn expected_model(truth: &Value) -> Value {
    let mut model = truth["model"].clone();
    model["age_years"] = json!(30.0);
    model["fixed_shape"] = json!({});
    model["root_translation_m"] = json!([0.0, 0.0, 0.0]);
    model["root_rotation_rad"] = json!([0.0, 0.0, 0.0]);
    model["joints"]["arm"]["axis"] = json!([1.0, 0.0, 0.0]);
    model["joints"]["arm"]["angle_rad"] = json!(0.0);
    model["keypoints"] = json!({});
    model
}

#[test]
fn capture_setup_constructs_the_first_request_and_reaches_the_ordinary_fitter() {
    let (dir, truth) = sources(true, 0);
    review(&dir, "setup-review", 0);
    assert!(!dir.join("input.json").exists());
    let selections = choices(&dir, "setup-review", &truth);
    write(&dir, "choices.json", &selections);
    construct(&dir, "constructed", 0);
    let initial = read(&dir, "constructed/input.json");
    assert_eq!(initial["schema"], "ftx-anny-capture-input/1");
    assert_eq!(initial["model"], expected_model(&truth));
    assert_eq!(initial["fit"]["starts"], json!([]));
    assert_eq!(initial["fit"]["measurements"], truth["fit"]["measurements"]);
    assert_eq!(
        initial["fit"]["metric_scale_source"],
        truth["fit"]["metric_scale_source"]
    );
    assert_eq!(
        initial["fit"]["observation_evidence"],
        truth["fit"]["observation_evidence"]
    );
    assert_eq!(
        initial["video_manifest"],
        json!(dir.join("capture-frames.json"))
    );
    assert_eq!(initial["reference_id"], "synthetic-fixed-sheet");
    assert_eq!(initial["fit"]["frames"].as_array().unwrap().len(), 2);
    for (i, frame) in initial["fit"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(frame["source"], truth["fit"]["frames"][i]["source"]);
        assert_eq!(frame["keypoints"], json!([]));
        assert_eq!(
            frame["calibration"],
            json!(dir.join("capture-calibration.json"))
        );
        assert_eq!(
            frame["calibration_sha256"],
            truth["fit"]["frames"][i]["calibration_sha256"]
        );
        let mut reference = truth["fit"]["frames"][i]["reference"].clone();
        reference["sheet"] = json!(dir.join("capture-board.json"));
        assert_eq!(frame["reference"], reference);
    }
    assert_eq!(
        std::fs::read(dir.join("constructed/choices.json")).unwrap(),
        std::fs::read(dir.join("choices.json")).unwrap()
    );
    assert_eq!(
        std::fs::read(dir.join("constructed/source-setup.json")).unwrap(),
        std::fs::read(dir.join("setup-review/setup.json")).unwrap()
    );
    let original = std::fs::read(dir.join("constructed/input.json")).unwrap();
    construct(&dir, "constructed", 3);
    assert_eq!(
        std::fs::read(dir.join("constructed/input.json")).unwrap(),
        original
    );
    // Retained browser fixtures are saved only after genuine first construction succeeds.
    write(&dir, "truth.json", &truth);
    write(&dir, "expected-choices.json", &selections);

    command(
        &dir,
        &[
            "measure",
            "body-target-review",
            "constructed/input.json",
            "--model",
            "model.ftxbody",
            "--out",
            "target-review",
        ],
        0,
    );
    let mut targets = read(&dir, "target-review/body-targets.json");
    assert_eq!(targets["keypoints"], json!({}));
    targets["keypoints"] = truth["model"]["keypoints"].clone();
    write(&dir, "targets.json", &targets);
    command(
        &dir,
        &[
            "measure",
            "body-targets",
            "targets.json",
            "--model",
            "model.ftxbody",
            "--out",
            "target-authored",
        ],
        0,
    );
    let cameras = command(
        &dir,
        &[
            "measure",
            "capture-review",
            "target-authored/input.json",
            "--out",
            "capture-review",
        ],
        0,
    )["result"]
        .clone();
    assert_eq!(
        cameras["capture_input"]["sources_sha256"],
        initial["reviewed_sources_sha256"]
    );
    for frame in cameras["capture_input"]["frames"].as_array().unwrap() {
        assert_eq!(
            frame["sheet_acquisition"]["detection"]["detector"]["backend"],
            "rust_charuco"
        );
        assert_eq!(frame["sheet_pose"]["ok"], true);
        assert_eq!(frame["sheet_pose"]["held_out_points"], 4);
    }
    let mut observed = read(&dir, "capture-review/capture.json");
    for frame in observed["fit"]["frames"].as_array_mut().unwrap() {
        let original = truth["fit"]["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| {
                source["source"]["metadata"]["frame"] == frame["source"]["metadata"]["frame"]
            })
            .unwrap();
        frame["keypoints"] = original["keypoints"].clone();
    }
    write(&dir, "observations.json", &observed);
    command(
        &dir,
        &[
            "measure",
            "fit-start-review",
            "observations.json",
            "--model",
            "model.ftxbody",
            "--out",
            "start-review",
        ],
        0,
    );
    let mut starts = read(&dir, "start-review/body-starts.json");
    assert_eq!(starts["starts"], json!([]));
    starts["starts"] = json!(
        truth["fit"]["starts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|start| {
                let poses: Vec<_> = observed["fit"]["frames"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(frame_index, frame)| {
                        let pose = start["poses"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|pose| pose["frame"] == frame["source"]["metadata"]["frame"])
                            .unwrap();
                        json!({"frame_index":frame_index,"values":pose["values"]})
                    })
                    .collect();
                json!({"shape":start["shape"],"poses":poses})
            })
            .collect::<Vec<_>>()
    );
    write(&dir, "starts.json", &starts);
    command(
        &dir,
        &[
            "measure",
            "fit-starts",
            "starts.json",
            "--model",
            "model.ftxbody",
            "--out",
            "start-authored",
        ],
        0,
    );
    let final_input = read(&dir, "start-authored/input.json");
    let mut expected = initial;
    expected["model"]["keypoints"] = truth["model"]["keypoints"].clone();
    expected["fit"]["starts"] = truth["fit"]["starts"].clone();
    for i in 0..2 {
        expected["fit"]["frames"][i]["keypoints"] = truth["fit"]["frames"][i]["keypoints"].clone();
    }
    assert_eq!(final_input, expected);
    let fitted = command(
        &dir.join("start-authored"),
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
    let mut reference = command(
        &dir,
        &[
            "measure",
            "fit-body",
            "truth.json",
            "--model",
            "model.ftxbody",
        ],
        0,
    )["result"]
        .clone();
    assert_eq!(fitted["status"], "ACCEPTED");
    assert_eq!(reference["capture_input"]["input"], truth);
    assert_eq!(fitted["capture_input"]["input"], final_input);
    assert_eq!(
        reference["capture_input"]["source"],
        json!(dir.join("truth.json"))
    );
    assert_eq!(
        fitted["capture_input"]["source"],
        json!(dir.join("start-authored/input.json"))
    );
    assert_eq!(
        reference["input_sha256"],
        sha256_hex(&std::fs::read(dir.join("truth.json")).unwrap())
    );
    assert_eq!(
        fitted["input_sha256"],
        sha256_hex(&std::fs::read(dir.join("start-authored/input.json")).unwrap())
    );
    reference["capture_input"]["input"] = fitted["capture_input"]["input"].clone();
    reference["capture_input"]["source"] = fitted["capture_input"]["source"].clone();
    reference["input_sha256"] = fitted["input_sha256"].clone();
    assert_eq!(fitted, reference);
}

#[test]
fn capture_setup_refuses_invalid_choices_stale_sources_and_missing_check_corners() {
    let (dir, truth) = sources(false, 0);
    review(&dir, "setup-review", 0);
    let good = choices(&dir, "setup-review", &truth);
    let mut cases = Vec::new();
    let mut add = |name: &str, value: Value| cases.push((name.to_owned(), value));
    let mut bad = good.clone();
    bad["shapes"][0]["choice"]["lower"] = json!(1.0);
    add("empty-shape-bounds", bad);
    let mut bad = good.clone();
    bad["shapes"][0]["choice"]["lower"] = json!(-0.1);
    add("outside-shape-bounds", bad);
    let mut bad = good.clone();
    bad["shapes"][0]["name"] = json!("hieght");
    add("unknown-shape", bad);
    let mut bad = good.clone();
    bad["shapes"][1] = bad["shapes"][0].clone();
    add("duplicate-shape", bad);
    let mut bad = good.clone();
    bad["roots"][0]["name"] = json!("root_x");
    add("unknown-root", bad);
    let mut bad = good.clone();
    bad["roots"].as_array_mut().unwrap().pop();
    add("missing-root", bad);
    let mut bad = good.clone();
    bad["joints"][0]["bone"] = json!("missing-bone");
    add("unknown-joint", bad);
    let mut bad = good.clone();
    bad["joints"][0]["axis"] = json!([0.0, 0.0, 0.0]);
    add("zero-axis", bad);
    let mut bad = good.clone();
    bad["frame_indices"] = json!([0, 0]);
    add("duplicate-frame", bad);
    let mut bad = good.clone();
    bad["frame_indices"] = json!([0, 2]);
    add("outside-frame", bad);
    let mut bad = good.clone();
    bad["frame_indices"] = json!([0]);
    add("one-frame", bad);
    let mut bad = good.clone();
    bad["age_years"] = Value::Null;
    add("missing-age", bad);
    let mut bad = good.clone();
    bad["observation_evidence"] = json!("");
    add("missing-evidence", bad);
    let mut bad = good.clone();
    bad["measurements"][0]["sigma_m"] = json!(0.0);
    add("invalid-measurement", bad);
    let mut bad = good.clone();
    bad["source_setup_sha256"] = json!("0".repeat(64));
    add("stale-setup-binding", bad);
    let mut bad = good.clone();
    bad["unexpected"] = json!(true);
    add("unknown-field", bad);
    for (name, bad) in cases {
        write(&dir, "choices.json", &bad);
        construct(&dir, &name, 3);
        assert!(!dir.join(name).exists());
    }
    let mut fixed = good.clone();
    fixed["shapes"][0]["choice"] = json!({"mode":"fixed","value":0.6});
    fixed["roots"][0]["choice"] = json!({"mode":"fixed","value":0.125});
    fixed["joints"][0]["choice"] = json!({"mode":"fixed","value":-0.2});
    write(&dir, "choices.json", &fixed);
    construct(&dir, "fixed-controls", 0);
    let model = read(&dir, "fixed-controls/input.json")["model"].clone();
    assert_eq!(model["fixed_shape"], json!({"height":0.6}));
    assert_eq!(
        model["shape_parameters"],
        json!([truth["model"]["shape_parameters"][1].clone()])
    );
    assert_eq!(model["root_translation_m"], json!([0.125, 0.0, 0.0]));
    assert_eq!(model["joints"]["arm"]["angle_rad"], -0.2);
    assert_eq!(
        model["pose_parameters"],
        json!(truth["model"]["pose_parameters"].as_array().unwrap()[1..6])
    );
    write(&dir, "choices.json", &good);
    for (index, file) in [
        "setup-review/setup.json",
        "capture-frames.json",
        "capture-calibration.json",
        "reference.json",
        "capture-board.json",
        "capture-person-0.png",
        "model.ftxbody",
    ]
    .iter()
    .enumerate()
    {
        let path = dir.join(file);
        let original = std::fs::read(&path).unwrap();
        let mut changed = original.clone();
        changed.push(b'\n');
        std::fs::write(&path, changed).unwrap();
        let output = format!("stale-source-{index}");
        construct(&dir, &output, 3);
        assert!(!dir.join(output).exists());
        std::fs::write(&path, original).unwrap();
    }
    let path = dir.join("capture-frame-0.png");
    let original = std::fs::read(&path).unwrap();
    let mut pixels = ftx_trace::raster::image::load_png(&original).unwrap();
    pixels.data[..3].fill(255);
    std::fs::write(
        &path,
        ftx_trace::raster::image::encode_png_bgr(&pixels).unwrap(),
    )
    .unwrap();
    construct(&dir, "changed-pixels", 3);
    assert!(!dir.join("changed-pixels").exists());
    std::fs::write(&path, original).unwrap();

    // A fresh setup binds this occlusion, so refusal must come from the native detector's
    // missing predeclared CHECK corner, not a stale image hash or fabricated marker result.
    let mut pixels = ftx_trace::raster::image::load_png(&std::fs::read(&path).unwrap()).unwrap();
    for y in 412..459 {
        for x in 782..829 {
            let offset = (y * pixels.w + x) * 3;
            pixels.data[offset..offset + 3].fill(255);
        }
    }
    std::fs::write(
        &path,
        ftx_trace::raster::image::encode_png_bgr(&pixels).unwrap(),
    )
    .unwrap();
    review(&dir, "occluded-review", 0);
    write(
        &dir,
        "choices.json",
        &choices(&dir, "occluded-review", &truth),
    );
    let refusal = construct(&dir, "missing-check", 3);
    assert!(
        refusal["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("every predeclared CHECK")
    );
    assert!(!dir.join("missing-check").exists());
}

#[test]
fn capture_setup_frame_ordinals_preserve_sorted_full_width_native_identities() {
    // Leave room for the selector's checked window endpoint while retaining full-width IDs.
    let first = u64::MAX - 2_000_000;
    let (dir, truth) = sources(false, first);
    let mut manifest = read(&dir, "capture-frames.json");
    manifest["frames"].as_array_mut().unwrap().reverse();
    write(&dir, "capture-frames.json", &manifest);
    review(&dir, "setup-review", 0);
    write(&dir, "choices.json", &choices(&dir, "setup-review", &truth));
    construct(&dir, "constructed", 0);
    let input = read(&dir, "constructed/input.json");
    for (i, frame) in input["fit"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(
            frame["source"]["metadata"]["frame"],
            truth["fit"]["frames"][i]["source"]["metadata"]["frame"]
        );
        assert_eq!(
            frame["source"]["metadata"]["frame"]["timestamp_us"].as_u64(),
            Some(first + i as u64 * 200_000)
        );
    }
    assert_eq!(input["fit"]["starts"], json!([]));
    assert!(
        input["fit"]["frames"]
            .as_array()
            .unwrap()
            .iter()
            .all(|frame| frame["keypoints"] == json!([]))
    );
}
