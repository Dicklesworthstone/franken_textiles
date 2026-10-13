//! Shape-reuse CLI against a tiny closed synthetic Anny-format model.
//! Includes the real image-fit producer; no production-body or real-wearer accuracy claim.

use ftx_core::receipt::sha256_hex;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

fn model_bytes() -> Vec<u8> {
    let mut arrays = BTreeMap::new();
    let mut data = Vec::new();
    let mut add = |name: &str, dtype: &str, shape: Vec<usize>, bytes: Vec<u8>| {
        arrays.insert(name.to_string(),json!({"dtype":dtype,"shape":shape,"offset":data.len(),"nbytes":bytes.len(),"sha256":sha256_hex(&bytes)}));
        data.extend(bytes);
    };
    let f64s = |a: Vec<f64>| a.into_iter().flat_map(f64::to_le_bytes).collect::<Vec<_>>();
    let u32s = |a: Vec<u32>| a.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
    let mut vertices = Vec::new();
    let mut delta = Vec::new();
    for z in [0.0, 1.0] {
        for y in [-0.1, 0.1] {
            for x in [-0.1, 0.1] {
                vertices.extend([x, y, z]);
                delta.extend([0.0, 0.0, 0.5 * z]);
            }
        }
    }
    add("template_vertices", "f64", vec![8, 3], f64s(vertices));
    add(
        "faces",
        "u32",
        vec![12, 3],
        u32s(vec![
            0, 2, 3, 0, 3, 1, 4, 5, 7, 4, 7, 6, 0, 1, 5, 0, 5, 4, 2, 6, 7, 2, 7, 3, 0, 4, 6, 0, 6,
            2, 1, 3, 7, 1, 7, 5,
        ]),
    );
    add("blendshape_row_ptr", "u32", vec![2], u32s(vec![0, 8]));
    add("blendshape_vertex", "u32", vec![8], u32s((0..8).collect()));
    add("blendshape_delta", "f64", vec![8, 3], f64s(delta));
    let mut mask = vec![0; 26];
    mask[1] = 1;
    add("phenotype_mask", "u8", vec![1, 26], mask);
    add("template_bone_heads", "f64", vec![1, 3], f64s(vec![0.0; 3]));
    add(
        "template_bone_tails",
        "f64",
        vec![1, 3],
        f64s(vec![0.0, 0.0, 0.1]),
    );
    add(
        "bone_heads_blendshapes",
        "f64",
        vec![1, 1, 3],
        f64s(vec![0.0; 3]),
    );
    add(
        "bone_tails_blendshapes",
        "f64",
        vec![1, 1, 3],
        f64s(vec![0.0; 3]),
    );
    add(
        "bone_rolls",
        "f64",
        vec![1, 3, 3],
        f64s(vec![1., 0., 0., 0., 1., 0., 0., 0., 1.]),
    );
    add("vertex_bone_indices", "u32", vec![8, 1], u32s(vec![0; 8]));
    add("vertex_bone_weights", "f64", vec![8, 1], f64s(vec![1.0; 8]));
    add(
        "base_mesh_vertex_indices",
        "u32",
        vec![8],
        u32s((0..8).collect()),
    );
    let mut components = vec![json!(["height", "low"]), json!(["height", "high"])];
    components.extend((0..24).map(|i| json!(["race", format!("r{i}")])));
    let header = json!({"schema":"ftxbody/1","model":"anny","anny_version":"0.6.1","configuration":"synthetic interchange test",
        "licence":"synthetic","package_sha256":{},"counts":{"vertices":8,"faces":12,"bones":1,"blendshapes":1,"phenotype_blendshapes":1,"local_changes":0,"influences":1},
        "phenotype":{"features":["height","race"],"components":components,"anchors":{"height":[0,1]},"exposed":["height"],"fixed_value":0.5},
        "y_axis":[0,1,0],"degenerate_rotation":[[1,0,0],[0,1,0],[0,0,1]],"labels":{"blendshapes":["height-high"],"bones":["root"],"bone_parents":[-1],"local_changes":[]},"waist_loop_final":[],"arrays":arrays});
    let mut h = serde_json::to_vec(&header).unwrap();
    while !h.len().is_multiple_of(8) {
        h.push(b' ');
    }
    let mut out = b"FTXBODY\0".to_vec();
    out.extend(1u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend((h.len() as u64).to_le_bytes());
    out.extend(h);
    out.extend(data);
    out
}

fn receipt(hash: &str) -> Value {
    let parameter = json!([{"name":"height","lower":0.0,"upper":1.0}]);
    let attempt = json!({"status":"CONVERGED","flags":[],"checks":[{"name":"p0","unit":"px","residual":0.25,"limit":3.0,"passed":true}]});
    json!({"schema":"ftx-body-fit/1","status":"ACCEPTED","input_sha256":"b".repeat(64),
        "input":{"starts":[{},{}],"frames":[{},{}]},"attempts":[attempt,attempt],
        "options":{"max_shape_spread_fraction":0.01},"shape_spread_fraction":[0.0],
        "model":{"model_id":format!("anny/0.6.1/sha256:{hash}"),"shape":parameter},
        "anny_configuration":{"model_sha256":hash,"age_years":2.5,"gender":1.0,"fixed_shape":{},"shape_parameters":parameter},
        "solution":{"shape":[0.6],"poses":[{"values":[5000,1000,1000]}],"anny_parameters":{"provenance":"MODEL_FIT",
            "phenotype":{"gender":1.0,"age":ftx_body::age_parameter_for_years(2.5),"height":0.6,"weight":0.5,"muscle":0.5,"proportions":0.5},"local_changes":[]}},
        "advisories":["Synthetic interchange receipt; not a physical fit"],"no_claim":"Synthetic test"})
}

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-shape-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let bytes = model_bytes();
    save(&dir.join("fit.json"), &receipt(&sha256_hex(&bytes)));
    std::fs::write(dir.join("model.ftxbody"), bytes).unwrap();
    dir
}
fn invoke(dir: &Path, args: &[&str], expected: i32) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{args:?}: {} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn body(dir: &Path, extra: &[&str], expected: i32) -> Value {
    let mut args = vec![
        "body",
        "--body-fit",
        "fit.json",
        "--model",
        "model.ftxbody",
        "--pose",
        "rest",
    ];
    args.extend(extra);
    serde_json::from_slice(&invoke(dir, &args, expected).stdout).unwrap()
}

#[test]
fn body_reuses_exact_shape_not_image_pose_or_new_stature() {
    let dir = setup();
    save(
        &dir.join("measurements.json"),
        &json!({"wearer":{"age_months":30,"sex":"F","stature_mm":1111}}),
    );
    let before = std::fs::read(dir.join("fit.json")).unwrap();
    let value = body(
        &dir,
        &["--out", "first", "--measurements", "measurements.json"],
        0,
    );
    let doc = &value["result"]["body"];
    assert_eq!(doc["schema"], "ftx-body/1");
    assert_eq!(doc["readings_mm"]["stature"], 1300.0);
    assert_eq!(doc["shape_source"]["refitted"], false);
    assert_eq!(doc["shape_source"]["image_poses_reused"], false);
    assert_eq!(doc["phenotype"]["height"], 0.6);
    assert_eq!(doc["model_fit"], json!([]));
    assert_eq!(doc["tape_evaluation"]["status"], "UNAVAILABLE");
    assert_eq!(doc["source_fit"], read(&dir.join("fit.json")));
    assert_eq!(doc["measurement_comparison"][0]["residual_mm"], 189.0);
    assert_eq!(
        read(&dir.join("first/body_fit.json")),
        read(&dir.join("fit.json"))
    );
    assert_eq!(
        std::fs::read(dir.join("first/body_fit.json")).unwrap(),
        before
    );
    let mesh =
        ftx_drape::render::read_obj(&std::fs::read_to_string(dir.join("first/body.obj")).unwrap())
            .unwrap();
    assert_eq!(mesh.vertices.len(), 8);
    assert_eq!(mesh.triangles.len(), 12);
    assert_eq!(mesh.vertices.iter().map(|p| p.z).fold(0.0, f64::max), 1.3);
    body(&dir, &["--out", "second"], 0);
    assert_eq!(
        std::fs::read(dir.join("first/body.obj")).unwrap(),
        std::fs::read(dir.join("second/body.obj")).unwrap()
    );
    assert_eq!(std::fs::read(dir.join("fit.json")).unwrap(), before);
}

#[test]
fn actual_image_fit_report_round_trips_to_the_same_closed_body() {
    let dir = setup();
    let frame = |i| json!({"stream":"synthetic","timestamp_us":i * 100000});
    let mut frames = Vec::new();
    for (i, offset) in [0.04, 0.11].into_iter().enumerate() {
        // Independent analytic pinhole observations of the known 1.3 m cuboid, translated
        // separately in each image. No candidate fitter output supplies these pixels.
        let mut points = Vec::new();
        for z in [0.0, 1.3] {
            for y in [-0.1, 0.1] {
                for x in [-0.1, 0.1] {
                    points.push(json!({"name":format!("p{}",points.len()),
                        "pixel_px":[640.0+1000.0*x/(y+4.0),480.0-1000.0*(z+offset-0.7)/(y+4.0)],
                        "sigma_px":0.5,"partition":if points.len()==7 {"held_out"}else{"fit"}}));
                }
            }
        }
        frames.push(
            json!({"frame":frame(i),"image_size_px":[1280,960],"rotation_cw_deg":0,
            "camera":{"intrinsics":{"focal_px":[1000,1000],"principal_px":[640,480]},
            "world_to_camera":{"rows":[[1,0,0],[0,0,-1],[0,1,0]]},"center_m":[0,-4,0.7]},
            "keypoints":points}),
        );
    }
    let targets: BTreeMap<_, _> = (0..8)
        .map(|i| (format!("p{i}"), json!({"kind":"vertex","index":i})))
        .collect();
    let starts: Vec<_> = [0.4, 0.8]
        .into_iter()
        .map(|height| {
            json!({"shape":[height],"poses":[{"frame":frame(0),"values":[0.0]},
            {"frame":frame(1),"values":[0.2]}]})
        })
        .collect();
    let request = json!({"schema":"ftx-anny-body-fit-input/1",
        "model":{"model_sha256":sha256_hex(&model_bytes()),"age_years":2.5,"gender":1.0,
            "shape_parameters":[{"name":"height","lower":0.0,"upper":1.0}],
            "pose_parameters":[{"name":"root_z_m","lower":-1.0,"upper":1.0}],
            "keypoints":targets,"measurements":["stature"]},
        "fit":{"schema":"ftx-body-fit-input/1","metric_scale_source":"analytic metre camera",
            "observation_evidence":"synthetic cuboid corners","frames":frames,"measurements":[],"starts":starts}});
    save(&dir.join("observations.json"), &request);
    invoke(
        &dir,
        &[
            "measure",
            "fit-body",
            "observations.json",
            "--model",
            "model.ftxbody",
            "--out",
            "producer.json",
        ],
        0,
    );
    let source = read(&dir.join("producer.json"));
    assert_eq!(source["status"], "ACCEPTED");
    assert!((source["solution"]["shape"][0].as_f64().unwrap() - 0.6).abs() < 1e-7);
    let output = invoke(
        &dir,
        &[
            "body",
            "--body-fit",
            "producer.json",
            "--model",
            "model.ftxbody",
            "--pose",
            "rest",
            "--out",
            "reused",
        ],
        0,
    );
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    let doc = &output["result"]["body"];
    assert_eq!(doc["source_fit"], source);
    assert_eq!(doc["phenotype"]["height"], source["solution"]["shape"][0]);
    assert!((doc["readings_mm"]["stature"].as_f64().unwrap() - 1300.0).abs() < 1e-4);
    let mesh =
        ftx_drape::render::read_obj(&std::fs::read_to_string(dir.join("reused/body.obj")).unwrap())
            .unwrap();
    assert!(
        mesh.vertices
            .iter()
            .map(|p| p.z)
            .fold(f64::INFINITY, f64::min)
            .abs()
            < 1e-12
    );
    assert_eq!(
        std::fs::read(dir.join("reused/body_fit.json")).unwrap(),
        std::fs::read(dir.join("producer.json")).unwrap()
    );
}

#[test]
fn malformed_rejected_and_wrong_model_reports_publish_nothing() {
    for mutation in [
        "status",
        "parameters",
        "model",
        "age",
        "expanded",
        "damaged_model",
    ] {
        let dir = setup();
        let mut v = read(&dir.join("fit.json"));
        match mutation {
            "status" => v["status"] = json!("AMBIGUOUS"),
            "parameters" => v["solution"]["shape"][0] = json!(0.7),
            "model" => v["anny_configuration"]["model_sha256"] = json!("c".repeat(64)),
            "age" => v["anny_configuration"]["age_years"] = json!(5.0),
            "expanded" => v["solution"]["anny_parameters"]["local_changes"] = json!([0.0]),
            _ => std::fs::write(dir.join("model.ftxbody"), b"broken").unwrap(),
        }
        save(&dir.join("fit.json"), &v);
        body(&dir, &["--out", "nested/body"], 3);
        assert!(!dir.join("nested").exists(), "{mutation}");
    }
}

#[test]
fn demographic_conflicts_and_unknown_options_refuse_before_publication() {
    for wearer in [
        json!({"age_months":31,"sex":"F"}),
        json!({"age_months":30,"sex":"M"}),
    ] {
        let dir = setup();
        save(&dir.join("m.json"), &json!({"wearer":wearer}));
        body(&dir, &["--out", "body", "--measurements", "m.json"], 3);
        assert!(!dir.join("body").exists());
    }
    let dir = setup();
    body(&dir, &["--out", "body", "--typo", "x"], 3);
    assert!(!dir.join("body").exists());
}

#[test]
fn reused_shapes_never_overwrite_evidence_or_completed_outputs() {
    let dir = setup();
    let before = std::fs::read(dir.join("fit.json")).unwrap();
    body(&dir, &["--out", "fit.json"], 3);
    assert_eq!(std::fs::read(dir.join("fit.json")).unwrap(), before);
    body(&dir, &["--out", "body"], 0);
    let output = std::fs::read(dir.join("body/body.json")).unwrap();
    body(&dir, &["--out", "body"], 3);
    assert_eq!(std::fs::read(dir.join("body/body.json")).unwrap(), output);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        symlink(dir.join("body"), dir.join("alias")).unwrap();
        body(&dir, &["--out", "alias"], 3);
        for f in ["body.obj", "body.json", "body_fit.json"] {
            assert_eq!(
                std::fs::metadata(dir.join("body").join(f))
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
fn drape_and_round_reach_shape_reuse_without_falling_back_to_tape() {
    let dir = setup();
    save(&dir.join("pattern.json"), &json!({}));
    save(&dir.join("m.json"), &json!({}));
    let mut report = read(&dir.join("fit.json"));
    report["status"] = json!("REJECTED");
    save(&dir.join("fit.json"), &report);
    let out = invoke(
        &dir,
        &[
            "drape",
            "pattern.json",
            "--measurements",
            "m.json",
            "--body-fit",
            "fit.json",
            "--model",
            "model.ftxbody",
            "--out",
            "sim",
        ],
        3,
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        result["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("ACCEPTED")
    );
    assert!(!dir.join("sim/sim_report.json").exists());
    invoke(&dir, &["round", "ws", "--body-fit", "fit.json"], 3);
    assert!(!dir.join("ws").exists());
}

#[test]
fn report_budget_is_enforced_before_model_or_output_access() {
    let dir = setup();
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("fit.json"))
        .unwrap();
    f.set_len(16 * 1024 * 1024 + 1).unwrap();
    body(&dir, &["--out", "body"], 4);
    assert!(!dir.join("body").exists());
}

#[test]
fn simulated_round_preserves_the_import_refusal_in_its_own_verdict() {
    let dir = setup();
    let fixtures: Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/oracle/draft/programs.json"
    ))
    .unwrap();
    let case = fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap();
    save(&dir.join("pattern.json"), &case["output"]["pattern"]);
    save(&dir.join("m.json"), &case["input"]["measurements"]);
    save(&dir.join("fit_spec.json"), &case["output"]["fit_spec"]);
    let mut fit = read(&dir.join("fit.json"));
    fit["status"] = json!("AMBIGUOUS");
    save(&dir.join("fit.json"), &fit);
    let original = std::fs::read(dir.join("fit.json")).unwrap();
    let output = invoke(
        &dir,
        &[
            "round",
            "ws",
            "--pattern",
            "pattern.json",
            "--measurements",
            "m.json",
            "--fit",
            "fit_spec.json",
            "--body-fit",
            "fit.json",
            "--model",
            "model.ftxbody",
            "--sim",
            "--as-of",
            "2026-10-13",
        ],
        2,
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let result = &value["result"];
    assert_eq!(result["status"], "SIM_GATING_FLAGS");
    assert_eq!(result["sim"]["settled"], false);
    assert!(
        result["sim"]["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("ACCEPTED")
    );
    assert_eq!(
        read(&dir.join("ws/rounds/000/sim_failure.json")),
        result["sim"]
    );
    assert!(result["viewer"].is_null());
    assert!(!dir.join("ws/rounds/000/sim/sim_report.json").exists());
    assert_eq!(std::fs::read(dir.join("fit.json")).unwrap(), original);
}
