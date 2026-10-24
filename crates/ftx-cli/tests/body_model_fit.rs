//! `ftx body` reports what it was not given as `MODEL_FIT` (plan §9.4): every tape reading the measurement
//! file lacks comes back as a `fabric-measurements/1` record read off the fitted body, with the population
//! spread as σ, and fusing it keeps it a model value, never a measurement.
//!
//! Needs the converted Anny model, so ignored by default:
//! `cargo test -p ftx-cli --release --test body_model_fit -- --include-ignored`.
//! The image-fit case uses synthetic projections of the full pinned model, not real photos.

#[path = "support/capture.rs"]
mod capture;

use std::path::{Path, PathBuf};
use std::process::Command;

use ftx_body::{Affine, AnnyModel, Phenotype, Pose, age_parameter_for_years};
use ftx_core::{Mat3, Vec2, Vec3, receipt::sha256_hex};
use ftx_measure::camera::{Camera, Distortion, Intrinsics};
use serde_json::{Value, json};

#[test]
#[ignore = "needs the converted Anny model"]
fn unmeasured_readings_come_back_as_model_fit() {
    let dir = std::env::temp_dir().join(format!("ftx-body-model-fit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/oracle/draft/programs.json");
    let fx: Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let mut meas = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap()["input"]["measurements"]
        .clone();
    // a wearer measured only round the trunk
    let kept: serde_json::Map<String, Value> = meas["fused"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| ["stature", "chest", "waist", "hip"].contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    meas["fused"] = Value::Object(kept);
    meas["wearer"]["reference"] = json!(false);
    std::fs::write(dir.join("m.json"), serde_json::to_vec(&meas).unwrap()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args([
            "body",
            "--measurements",
            "m.json",
            "--out",
            "body",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let r: Value = serde_json::from_slice(&out.stdout).unwrap();
    let body = &r["result"]["body"];
    let fitted: Vec<&str> = body["fit"]["tape"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let records = body["model_fit"].as_array().unwrap();
    let names: Vec<&str> = records
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for n in ftx_body::TAPE_FIELDS {
        assert!(
            fitted.contains(&n) != names.contains(&n),
            "{n}: fitted {fitted:?}, model_fit {names:?}"
        );
    }
    for m in records {
        assert_eq!(m["provenance"], "MODEL_FIT");
        assert!(
            m["sigma_mm"].as_f64().unwrap() > 0.0 && m["value_mm"].as_f64().unwrap() > 0.0,
            "{m}"
        );
        assert_eq!(
            body["readings_mm"][m["name"].as_str().unwrap()],
            m["value_mm"]
        );
    }
    // fused, a model value stays a model value: never a direct (tape) source
    let mut doc = meas.clone();
    doc["records"] = Value::Array(records.clone());
    let fused = ftx_measure::fuse(&doc, false).unwrap();
    assert_eq!(fused["fused"]["head"]["primary_provenance"], "MODEL_FIT");
}

fn production_command(dir: &Path, args: &[&str], expected: i32, receipt: &str) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    std::fs::write(dir.join(format!("{receipt}.stdout.json")), &out.stdout).unwrap();
    std::fs::write(dir.join(format!("{receipt}.stderr.txt")), &out.stderr).unwrap();
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
#[ignore = "needs the admitted production Anny model; synthetic numerical qualification only"]
fn production_image_fit_recovers_withheld_tapes_and_reuses_the_accepted_shape() {
    let model_path = ftx_body::default_model_path().expect("model path");
    let model_bytes = std::fs::read(&model_path).unwrap();
    let model_sha = sha256_hex(&model_bytes);
    assert_eq!(
        model_sha, "8f0bb151ba19975437147d239b7ff237642a7fa63659a6fac49fa23c299a0915",
        "this acceptance is bound to the admitted Anny 0.6.1 conversion"
    );
    drop(model_bytes);
    let model = AnnyModel::load(&model_path, true).unwrap();
    assert_eq!(model.data().header.counts.vertices, 13_718);
    let parameters = model.params(Phenotype {
        age: age_parameter_for_years(2.5),
        gender: 1.0,
        height: 0.62,
        weight: 0.43,
        ..Phenotype::default()
    });
    let rest = model.rest(&parameters).unwrap();
    let height = rest
        .vertices
        .iter()
        .map(|p| p.z)
        .fold(f64::NEG_INFINITY, f64::max)
        - rest
            .vertices
            .iter()
            .map(|p| p.z)
            .fold(f64::INFINITY, f64::min);
    let tape_pose = model
        .pose(
            &rest,
            &ftx_body::pose_deltas(&model, &rest, &Pose::a_pose(35.0), 1.0).unwrap(),
        )
        .unwrap();
    let body_frames = ftx_body::BodyFrames::new(&model, &tape_pose).unwrap();
    let faces: Vec<_> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();
    let expected_tapes = ftx_body::tape_readings(&faces, height, &tape_pose, &body_frames).unwrap();
    let arm = model.bone_index("upperarm01.L").unwrap();
    // Authored synthetic correspondences, not inferred anatomical/visible image landmarks.
    // Full skinning generates observations; the CLI uses its separate selected-vertex adapter.
    let vertices: Vec<_> = (0..48).map(|i| i * rest.vertices.len() / 48).collect();
    let targets: serde_json::Map<_, _> = vertices
        .iter()
        .map(|i| (format!("v{i:05}"), json!({"kind":"vertex","index":i})))
        .collect();
    // Each row is independent depth, vertical translation, root yaw and local arm rotation.
    let poses = [
        [0.04, 0.02, -0.12, -0.25],
        [-0.03, -0.01, 0.16, 0.30],
        [0.02, 0.01, 0.04, -0.10],
    ];
    let mut frames = Vec::new();
    for (i, p) in poses.iter().enumerate() {
        let mut deltas = vec![Affine::IDENTITY; model.data().header.counts.bones];
        deltas[0] = Affine {
            m: Mat3::rotation(Vec3::new(0.0, 0.0, 1.0), p[2]),
            t: Vec3::new(0.0, p[0], p[1]),
        };
        deltas[arm].m = Mat3::rotation(Vec3::new(1.0, 0.0, 0.0), p[3]);
        let posed = model.pose(&rest, &deltas).unwrap();
        let mid_z = 0.5
            * (posed
                .vertices
                .iter()
                .map(|v| v.z)
                .fold(f64::INFINITY, f64::min)
                + posed
                    .vertices
                    .iter()
                    .map(|v| v.z)
                    .fold(f64::NEG_INFINITY, f64::max));
        let yaw = [-0.3, 0.0, 0.35][i];
        let turn = Mat3::rotation(Vec3::new(0.0, 0.0, 1.0), yaw);
        let camera = Camera {
            intrinsics: Intrinsics {
                focal_px: Vec2::new(1100.0, 1120.0),
                principal_px: Vec2::new(640.0, 480.0),
                distortion: Distortion {
                    radial: [-0.01, 0.001, 0.0],
                    ..Distortion::default()
                },
            },
            world_to_camera: Mat3::from_rows([[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]])
                * turn.transpose(),
            center_m: turn.mul_vec(Vec3::new(0.0, -3.0, mid_z)),
        };
        let keypoints: Vec<_> = vertices
            .iter()
            .enumerate()
            .map(|(k, &v)| {
                let pixel = camera.project(posed.vertices[v]).unwrap();
                assert!(
                    (0.0..1280.0).contains(&pixel.x) && (0.0..960.0).contains(&pixel.y),
                    "frame {i}, vertex {v}: {pixel:?}"
                );
                json!({"name":format!("v{v:05}"),"pixel_px":pixel,"sigma_px":1.0,
                "partition":if k % 6 == 0 { "held_out" } else { "fit" }})
            })
            .collect();
        frames.push(
            json!({"frame":{"stream":"synthetic-production-anny","timestamp_us":i*200_000},
            "camera":camera,"image_size_px":[1280,960],"rotation_cw_deg":0,"keypoints":keypoints}),
        );
    }
    let starts: Vec<_> = [-0.03, 0.03]
        .into_iter()
        .map(|shift| {
            json!({
                "shape":[0.62+shift,0.43-shift],
                "poses":poses.iter().enumerate().map(|(i,p)| json!({"frame":frames[i]["frame"],
                    "values":p.iter().map(|v| v+shift).collect::<Vec<_>>()})).collect::<Vec<_>>()
            })
        })
        .collect();
    let request = json!({"schema":"ftx-anny-body-fit-input/1",
        "model":{"model_sha256":model_sha,"age_years":2.5,"gender":1.0,
            "shape_parameters":[{"name":"height","lower":0.2,"upper":0.8},
                {"name":"weight","lower":0.2,"upper":0.8}],
            "pose_parameters":[{"name":"root_y_m","lower":-0.2,"upper":0.2},
                {"name":"root_z_m","lower":-0.1,"upper":0.1},
                {"name":"root_rz_rad","lower":-0.4,"upper":0.4},
                {"name":"joint:upperarm01.L","lower":-0.8,"upper":0.8}],
            "joints":{"upperarm01.L":{"axis":[1,0,0]}},"keypoints":targets,
            "measurements":ftx_body::TAPE_FIELDS},
        "fit":{"schema":"ftx-body-fit-input/1","metric_scale_source":"synthetic metre cameras and independent model stature",
            "observation_evidence":"full-skin Anny projections; known vertex identities; no detector or visibility claim",
            "frames":frames,"starts":starts,"measurements":[{"name":"stature","value_m":height,
                "sigma_m":0.002,"evidence":"synthetic known rest height","partition":"fit"}]}});
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    // Optional durable directory keeps the actual synthetic CLI outputs for offline browser review.
    // Exclusive creation below refuses an existing path; no alternate renderer or result copying.
    let dir = std::env::var_os("FTX_CAPTURE_ROUND_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "ftx-production-image-fit-{}-{stamp}",
                std::process::id()
            ))
        });
    std::fs::create_dir(&dir).unwrap();
    println!("production image-fit fixture: {}", dir.display());
    let source = serde_json::to_vec_pretty(&request).unwrap();
    std::fs::write(dir.join("observations.json"), &source).unwrap();
    let model_arg = model_path.to_str().unwrap();
    let accepted = production_command(
        &dir,
        &[
            "measure",
            "fit-body",
            "observations.json",
            "--model",
            model_arg,
            "--out",
            "accepted.json",
        ],
        0,
        "accepted",
    );
    let report = &accepted["result"];
    assert_eq!(report["status"], "ACCEPTED");
    assert_eq!(report["attempts"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["solution"]["anny_parameters"]["provenance"],
        "MODEL_FIT"
    );
    for (k, truth) in [0.62, 0.43].into_iter().enumerate() {
        assert!((report["solution"]["shape"][k].as_f64().unwrap() - truth).abs() < 1e-5);
    }
    for (i, pose) in poses.iter().enumerate() {
        for (k, truth) in pose.iter().enumerate() {
            assert!(
                (report["solution"]["poses"][i]["values"][k]
                    .as_f64()
                    .unwrap()
                    - truth)
                    .abs()
                    < 1e-5
            );
        }
    }
    for attempt in report["attempts"].as_array().unwrap() {
        let checks = attempt["checks"].as_array().unwrap();
        assert_eq!(checks.iter().filter(|c| c["unit"] == "px").count(), 144);
        assert_eq!(
            checks
                .iter()
                .filter(|c| c["partition"] == "held_out")
                .count(),
            24
        );
        assert!(checks.iter().all(|c| c["passed"] == true));
    }
    for (i, name) in ftx_body::TAPE_FIELDS.iter().enumerate() {
        let residual = (report["solution"]["predicted_measurements_m"][i]
            .as_f64()
            .unwrap()
            * 1000.0
            - expected_tapes[i])
            .abs();
        let limit = if ftx_body::is_girth(name) { 3.0 } else { 2.0 };
        let partition = if *name == "stature" {
            "fit"
        } else {
            "withheld"
        };
        assert!(
            residual <= limit,
            "{partition} {name}: {residual} mm > {limit} mm"
        );
        println!("production image-fit {partition} {name}: {residual:.9} mm (limit {limit} mm)");
    }
    let body = production_command(
        &dir,
        &[
            "body",
            "--body-fit",
            "accepted.json",
            "--model",
            model_arg,
            "--pose",
            "rest",
            "--out",
            "reused",
        ],
        0,
        "reused",
    );
    let description = &body["result"]["body"];
    assert_eq!(description["source_fit"], *report);
    assert_eq!(description["shape_source"]["refitted"], false);
    assert_eq!(description["shape_source"]["image_poses_reused"], false);
    assert!(description["model_fit"].as_array().unwrap().is_empty());
    for name in ["height", "weight"] {
        assert_eq!(
            description["phenotype"][name],
            report["solution"]["anny_parameters"]["phenotype"][name]
        );
    }
    assert_eq!(
        std::fs::read(dir.join("reused/body_fit.json")).unwrap(),
        std::fs::read(dir.join("accepted.json")).unwrap()
    );
    let exported =
        ftx_drape::render::read_obj(&std::fs::read_to_string(dir.join("reused/body.obj")).unwrap())
            .unwrap();
    let truth = model
        .pose(
            &rest,
            &ftx_body::pose_deltas(&model, &rest, &Pose::rest(), 1.0).unwrap(),
        )
        .unwrap();
    let truth = ftx_body::skin_mesh(&truth.vertices, &model.data().faces).unwrap();
    assert_eq!(exported.triangles, truth.triangles);
    assert_eq!(exported.vertices.len(), truth.vertices.len());
    let max_error = exported
        .vertices
        .iter()
        .zip(&truth.vertices)
        .map(|(&a, &b)| (a - b).norm())
        .fold(0.0_f64, f64::max);
    assert!(
        max_error <= 0.002,
        "exported rest skin differs by {max_error} m"
    );
    println!(
        "production image-fit reused skin: {} vertices; max error {max_error:.12} m",
        exported.vertices.len()
    );
    let mut rejected = request.clone();
    rejected["fit"]["frames"][0]["keypoints"][0]["pixel_px"][0] = json!(
        request["fit"]["frames"][0]["keypoints"][0]["pixel_px"][0]
            .as_f64()
            .unwrap()
            + 20.0
    );
    std::fs::write(
        dir.join("held-out.json"),
        serde_json::to_vec(&rejected).unwrap(),
    )
    .unwrap();
    let rejected = production_command(
        &dir,
        &[
            "measure",
            "fit-body",
            "held-out.json",
            "--model",
            model_arg,
            "--out",
            "rejected.json",
        ],
        2,
        "rejected",
    );
    assert_eq!(rejected["result"]["status"], "REJECTED");
    assert!(rejected["result"]["solution"].is_null());
    for (a, b) in report["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .zip(rejected["result"]["attempts"].as_array().unwrap())
    {
        assert_eq!(
            a["cost_history"], b["cost_history"],
            "held-out evidence entered the fit"
        );
    }
    let budget = production_command(
        &dir,
        &[
            "measure",
            "fit-body",
            "observations.json",
            "--model",
            model_arg,
            "--max-evaluations",
            "1",
            "--out",
            "budget.json",
        ],
        4,
        "budget",
    );
    assert_eq!(budget["result"]["status"], "BUDGET_STOP");
    assert!(budget["result"]["solution"].is_null());
    for refused in ["rejected", "budget"] {
        let fit = format!("{refused}.json");
        let output = format!("{refused}-body");
        production_command(
            &dir,
            &[
                "body",
                "--body-fit",
                &fit,
                "--model",
                model_arg,
                "--pose",
                "rest",
                "--out",
                &output,
            ],
            3,
            &output,
        );
        assert!(!dir.join(output).exists(), "a refused fit published a body");
    }
    assert_eq!(
        std::fs::read(dir.join("observations.json")).unwrap(),
        source
    );
    let capture = capture::wrap(&dir, request);
    std::fs::write(
        dir.join("capture.json"),
        serde_json::to_vec(&capture).unwrap(),
    )
    .unwrap();
    let captured = production_command(
        &dir,
        &[
            "measure",
            "fit-body",
            "capture.json",
            "--model",
            model_arg,
            "--mesh-out",
            "capture-review",
        ],
        0,
        "capture-fit",
    );
    assert_eq!(captured["result"]["status"], "ACCEPTED");
    assert_eq!(captured["result"]["capture_input"]["input"], capture);
    for (i, name) in ftx_body::TAPE_FIELDS.iter().enumerate() {
        let residual = (captured["result"]["solution"]["predicted_measurements_m"][i]
            .as_f64()
            .unwrap()
            * 1000.0
            - expected_tapes[i])
            .abs();
        let limit = if ftx_body::is_girth(name) { 3.0 } else { 2.0 };
        assert!(
            residual <= limit,
            "capture {name}: {residual} mm > {limit} mm"
        );
        println!("production capture {name}: {residual:.9} mm (limit {limit} mm)");
    }
    for (i, truth) in [0.62, 0.43].iter().enumerate() {
        assert!(
            (captured["result"]["solution"]["shape"][i].as_f64().unwrap() - truth).abs() < 1e-5
        );
    }
    let reused = production_command(
        &dir,
        &[
            "body",
            "--body-fit",
            "capture-review/fit.json",
            "--model",
            model_arg,
            "--pose",
            "rest",
            "--out",
            "capture-body",
        ],
        0,
        "capture-body",
    );
    assert_eq!(reused["result"]["body"]["shape_source"]["refitted"], false);

    // A real garment and immutable rounds, with the existing public-prior target document.
    // Its chart evidence deliberately remains READY_REFERENCE; an accepted image fit adds no tape.
    let fixtures: Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/oracle/draft/programs.json"
    ))
    .unwrap();
    let garment = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "zouave_bolero-920")
        .unwrap();
    for (file, value) in [
        ("garment-pattern.json", &garment["output"]["pattern"]),
        ("garment-fit.json", &garment["output"]["fit_spec"]),
        (
            "garment-measurements.json",
            &garment["input"]["measurements"],
        ),
    ] {
        std::fs::write(dir.join(file), serde_json::to_vec(value).unwrap()).unwrap();
    }
    let mut round_args = vec![
        "round",
        "garment-workspace",
        "--pattern",
        "garment-pattern.json",
        "--fit",
        "garment-fit.json",
        "--measurements",
        "garment-measurements.json",
        "--as-of",
        "2026-10-24",
        "--export",
    ];
    let flat = production_command(&dir, &round_args, 0, "garment-flat");
    assert_eq!(flat["result"]["readiness"]["status"], "READY_REFERENCE");
    round_args.extend_from_slice(&[
        "--sim",
        "--body-fit",
        "capture-review/fit.json",
        "--model",
        model_arg,
        "--self-contact-mm",
        "2",
        "--sew",
        "12",
        "--drape",
        "72",
    ]);
    let round = production_command(&dir, &round_args, 0, "garment-captured");
    let record = &round["result"];
    assert_eq!(record["round"], 1);
    assert_eq!(record["readiness"], flat["result"]["readiness"]);
    assert_eq!(record["basis"]["measurements"], "READY_REFERENCE");
    assert_eq!(record["basis"]["flat"], "CONVERGED");
    assert_eq!(record["exports"]["pass"], true);
    assert_eq!(record["sim"]["gating"], json!([]));
    assert_eq!(record["sim"]["numerical"]["settled"], true);
    assert_eq!(record["sim"]["self_contact"]["enabled"], true);
    for (phase, steps) in [("sew", 12), ("drape", 72)] {
        let samples = record["sim"]["numerical"][phase]["samples"]
            .as_array()
            .unwrap();
        assert_eq!(samples.len(), steps);
        assert!(
            samples
                .iter()
                .all(|sample| sample["solve"]["converged"] == true)
        );
    }
    let body = &record["sim"]["body"];
    assert_eq!(body["source_fit"], captured["result"]);
    assert_eq!(body["shape_source"]["refitted"], false);
    assert_eq!(body["shape_source"]["image_poses_reused"], false);
    assert_eq!(body["phenotype"], reused["result"]["body"]["phenotype"]);
    assert_eq!(body["provenance"], "MODEL_FIT");
    let saved = dir.join("garment-workspace/rounds/001");
    let source = std::fs::read(dir.join("capture-review/fit.json")).unwrap();
    assert_eq!(
        std::fs::read(saved.join("sim/body_fit.json")).unwrap(),
        source
    );
    assert_eq!(body["shape_source"]["report_sha256"], sha256_hex(&source));
    for row in body["measurement_comparison"].as_array().unwrap() {
        assert_eq!(row["used_to_refit"], false);
        assert!(
            (row["residual_mm"].as_f64().unwrap()
                - (row["body_mm"].as_f64().unwrap() - row["target_mm"].as_f64().unwrap()))
            .abs()
                < 1e-9
        );
    }
    let before = std::fs::read(saved.join("round.json")).unwrap();
    let report_path = dir.join("garment-workspace/report.html");
    let html = std::fs::read_to_string(&report_path).unwrap();
    assert!(html.contains("Body used for draping — MODEL_FIT"));
    assert!(html.contains("rounds/001/sim/body_fit.json"));
    assert!(html.contains("Measurement readiness: READY_REFERENCE"));
    assert!(!html.contains("Measurement readiness: READY</h3>"));
    println!("production capture garment basis: {}", record["basis"]);

    // Changing the external fit cannot revise the completed round or fall back to a fresh tape fit.
    let mut rejected = captured["result"].clone();
    rejected["status"] = json!("REJECTED");
    std::fs::write(
        dir.join("capture-review/fit.json"),
        serde_json::to_vec(&rejected).unwrap(),
    )
    .unwrap();
    let failed = production_command(&dir, &round_args, 2, "garment-rejected");
    assert_eq!(failed["result"]["round"], 2);
    assert_eq!(failed["result"]["status"], "SIM_GATING_FLAGS");
    assert!(failed["result"]["sim"]["refusal"].is_object());
    assert!(failed["result"]["viewer"].is_null());
    assert_eq!(
        std::fs::read(saved.join("sim/body_fit.json")).unwrap(),
        source
    );
    assert_eq!(std::fs::read(saved.join("round.json")).unwrap(), before);
    assert!(
        !dir.join("garment-workspace/rounds/002/sim/body_fit.json")
            .exists()
    );
    let html = std::fs::read_to_string(report_path).unwrap();
    let failed_html = html.split("Round 2 —").nth(1).unwrap();
    assert!(!failed_html.contains("Original accepted body fit"));
    assert!(!failed_html.contains("Open drape viewer"));
    assert!(html.contains("rounds/001/sim/body_fit.json"));
}
