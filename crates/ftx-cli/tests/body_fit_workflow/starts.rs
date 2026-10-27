use super::*;

fn review(dir: &Path, output: &str, extra: &[&str], code: i32) -> Value {
    let result = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args([
            "measure",
            "fit-start-review",
            "input.json",
            "--model",
            "model.ftxbody",
            "--out",
            output,
            "--json",
        ])
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(code),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn start_review_projects_actual_declared_model_and_keeps_frame_identity() {
    let dir = setup();
    let (bytes, mut input) = fixture();
    input["fit"]["frames"].as_array_mut().unwrap().reverse();
    input["fit"]["starts"][0]["poses"]
        .as_array_mut()
        .unwrap()
        .reverse();
    save(&dir, &input);
    let original = std::fs::read(dir.join("input.json")).unwrap();
    let report = review(&dir, "starts", &[], 0)["result"].clone();
    assert_eq!(report["status"], "INITIALIZATION_REVIEW");
    assert_eq!(report["body_fitted"], false);
    assert!(report.get("solution").is_none());
    assert_eq!(
        std::fs::read(dir.join("starts/original-input.json")).unwrap(),
        original
    );
    assert_eq!(report["input_sha256"], sha256_hex(&original));
    assert_eq!(report["model_sha256"], sha256_hex(&bytes));
    let model = AnnyModel::from_data(AnnyData::from_bytes(&bytes, true).unwrap()).unwrap();
    for (s, start) in input["fit"]["starts"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let rest = model
            .rest(&model.params(Phenotype {
                height: start["shape"][0].as_f64().unwrap(),
                weight: start["shape"][1].as_f64().unwrap(),
                ..Phenotype::default()
            }))
            .unwrap();
        for record in report["starts"][s]["frames"].as_array().unwrap() {
            let source = input["fit"]["frames"]
                .as_array()
                .unwrap()
                .iter()
                .find(|frame| frame["frame"] == record["frame"])
                .unwrap();
            let pose = start["poses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|pose| pose["frame"] == record["frame"])
                .unwrap();
            assert_eq!(record["pose_values"], pose["values"]);
            let p: Vec<f64> = serde_json::from_value(pose["values"].clone()).unwrap();
            let rotation = Vec3::new(p[3], p[4], p[5]);
            let body = model
                .pose(
                    &rest,
                    &[
                        Affine {
                            m: Mat3::rotation(rotation.normalized(), rotation.norm()),
                            t: Vec3::new(p[0], p[1], p[2]),
                        },
                        Affine {
                            m: Mat3::rotation(Vec3::new(1.0, 0.0, 0.0), p[6]),
                            t: Vec3::ZERO,
                        },
                    ],
                )
                .unwrap();
            let camera: Camera = serde_json::from_value(source["camera"].clone()).unwrap();
            for point in record["keypoints"].as_array().unwrap() {
                let index = point["name"]
                    .as_str()
                    .unwrap()
                    .strip_prefix('p')
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                let expected = camera.project(body.vertices[index]).unwrap();
                let got: Vec2 = serde_json::from_value(point["predicted_px"].clone()).unwrap();
                assert!((expected - got).norm() < 1e-11);
            }
            let mesh = std::fs::read_to_string(
                dir.join("starts")
                    .join(record["mesh"]["file"].as_str().unwrap()),
            )
            .unwrap();
            assert!(mesh.contains("declared INITIALIZATION"));
            let vertices: Vec<Vec3> = mesh
                .lines()
                .filter_map(|line| line.strip_prefix("v "))
                .map(|line| {
                    let values: Vec<f64> = line
                        .split_whitespace()
                        .map(|v| v.parse().unwrap())
                        .collect();
                    Vec3::new(values[0], values[1], values[2])
                })
                .collect();
            assert_eq!(vertices, body.vertices);
            assert_eq!(
                mesh.lines().filter(|line| line.starts_with("f ")).count(),
                13
            );
        }
    }
    let before = std::fs::read(dir.join("starts/review.json")).unwrap();
    review(&dir, "starts", &[], 3);
    assert_eq!(
        std::fs::read(dir.join("starts/review.json")).unwrap(),
        before
    );
    mask_manifest(&dir);
    let masked = review(
        &dir,
        "masked-starts",
        &["--silhouettes", "masks/manifest.json"],
        0,
    )["result"]
        .clone();
    assert_eq!(
        masked["silhouette_input"]["manifest_sha256"],
        sha256_hex(&std::fs::read(dir.join("masks/manifest.json")).unwrap())
    );
    assert!(masked["starts"].as_array().unwrap().iter().all(|start| {
        start["frames"]
            .as_array()
            .unwrap()
            .iter()
            .all(|frame| frame["silhouette_comparisons"].as_array().unwrap().len() == 3)
    }));
    let mut malformed = input.clone();
    malformed["fit"]["starts"][0]["shape"][0] = json!(0.0);
    save(&dir, &malformed);
    review(&dir, "boundary", &[], 3);
    assert!(!dir.join("boundary").exists());
    malformed = input.clone();
    malformed["fit"]["starts"][1] = malformed["fit"]["starts"][0].clone();
    save(&dir, &malformed);
    review(&dir, "duplicate", &[], 3);
    assert!(!dir.join("duplicate").exists());
    malformed = input;
    malformed["model"]["model_sha256"] = json!("0".repeat(64));
    save(&dir, &malformed);
    review(&dir, "stale-model", &[], 3);
    assert!(!dir.join("stale-model").exists());
}

#[test]
fn start_review_retains_bad_projections_and_missing_contours_without_acceptance() {
    let dir = setup();
    let (_, mut input) = fixture();
    input["model"]["pose_parameters"][1]["lower"] = json!(-8.0);
    input["fit"]["starts"][0]["poses"][0]["values"][1] = json!(-6.0);
    for frame in input["fit"]["frames"].as_array_mut().unwrap() {
        frame["silhouette"] = json!({"evidence":"Synthetic deliberately misplaced initial foreground", "lines":[
            {"axis":"horizontal","at_px":10,"intervals_px":[[600,700]],"sigma_px":1,"partition":"fit"}
        ]});
    }
    save(&dir, &input);
    let report = review(&dir, "bad-starts", &[], 0)["result"].clone();
    let behind = &report["starts"][0]["frames"][0];
    assert_eq!(behind["unprojectable_vertices"], 10);
    assert!(behind["omitted_faces"].as_u64().unwrap() > 0);
    assert!(
        behind["keypoints"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p.get("projection_refusal").is_some() && p.get("predicted_px").is_none())
    );
    assert!(
        behind["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["stage"] == "silhouette_projection")
    );
    assert!(
        dir.join("bad-starts")
            .join(behind["mesh"]["file"].as_str().unwrap())
            .exists()
    );
    let other = &report["starts"][0]["frames"][1];
    assert_eq!(other["unprojectable_vertices"], 0);
    assert!(
        other["keypoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["residual_px"].as_f64().unwrap() > 3.0)
    );
    let contour = &other["silhouette_comparisons"][0];
    assert_eq!(contour["passed"], false);
    assert_eq!(contour["predicted_intervals_px"], json!([]));
    assert!(contour["residuals_px"].is_null());
    assert_eq!(contour["symmetric_difference_px"], 100.0);
    assert_eq!(report["starts"].as_array().unwrap().len(), 2);
    assert_eq!(report["status"], "INITIALIZATION_REVIEW");
}

#[test]
fn start_review_binds_capture_pixels_and_refuses_changed_sources() {
    let dir = setup();
    let (_, input) = fixture();
    let capture = capture::wrap(&dir, input);
    save(&dir, &capture);
    let report = review(&dir, "capture-starts", &[], 0)["result"].clone();
    eprintln!("START_REVIEW_DIR={}", dir.join("capture-starts").display());
    let page = std::fs::read_to_string(dir.join("capture-starts/index.html")).unwrap();
    assert_eq!(page.matches("<svg ").count(), 4);
    assert_eq!(page.matches("<image href=\"source-").count(), 4);
    assert!(!page.contains("<img src=\"start-"));
    for record in report["starts"][0]["frames"].as_array().unwrap() {
        let source = capture["fit"]["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|frame| frame["source"]["metadata"]["frame"] == record["frame"])
            .unwrap();
        let expected =
            std::fs::read(dir.join(source["source"]["image"].as_str().unwrap())).unwrap();
        let image = &record["source_image"];
        assert_eq!(image["sha256"], sha256_hex(&expected));
        assert_eq!(
            std::fs::read(
                dir.join("capture-starts")
                    .join(image["file"].as_str().unwrap())
            )
            .unwrap(),
            expected
        );
        let svg = std::fs::read_to_string(
            dir.join("capture-starts")
                .join(record["overlay"]["file"].as_str().unwrap()),
        )
        .unwrap();
        assert!(svg.contains(&format!(
            "<image href=\"{}\"",
            image["file"].as_str().unwrap()
        )));
    }
    let path = dir.join(
        capture["fit"]["frames"][0]["source"]["image"]
            .as_str()
            .unwrap(),
    );
    let image = Gray {
        w: 1280,
        h: 960,
        data: vec![255; 1280 * 960],
    };
    std::fs::write(path, encode_png_gray(&image)).unwrap();
    review(&dir, "stale-capture", &[], 3);
    assert!(!dir.join("stale-capture").exists());
}
