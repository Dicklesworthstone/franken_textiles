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
    apply(&dir, "authored", 0);
    let authored = read(&dir, "authored/input.json");
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
    review(&dir, "single-review", 0);
    assert_eq!(
        read(&dir, "single-review/body-starts.json")["starts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
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
