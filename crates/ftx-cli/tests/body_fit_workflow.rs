//! Real-binary workflow using a tiny synthetic two-bone .ftxbody, never production model assets.
#[path = "support/capture.rs"]
mod capture;
#[path = "body_fit_workflow/setup.rs"]
mod setup;
#[path = "body_fit_workflow/start_edits.rs"]
mod start_edits;
#[path = "body_fit_workflow/starts.rs"]
mod starts;
#[path = "body_fit_workflow/targets.rs"]
mod targets;
use ftx_body::{Affine, AnnyData, AnnyModel, Phenotype};
use ftx_core::{Mat3, Vec2, Vec3, receipt::sha256_hex};
use ftx_measure::camera::{Camera, Distortion, Intrinsics};
use ftx_trace::raster::image::{Gray, encode_png_gray};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

fn f64_bytes(values: impl IntoIterator<Item = f64>) -> Vec<u8> {
    values.into_iter().flat_map(f64::to_le_bytes).collect()
}
fn u32_bytes(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_le_bytes).collect()
}
fn xyz(points: &[Vec3]) -> Vec<u8> {
    f64_bytes(points.iter().flat_map(|v| <[f64; 3]>::from(*v)))
}

fn synthetic_model_scaled(scale: f64) -> Vec<u8> {
    let xyz = |points: &[Vec3]| xyz(&points.iter().map(|p| *p * scale).collect::<Vec<_>>());
    let mut template = Vec::new();
    let mut height_delta = Vec::new();
    let mut width_delta = Vec::new();
    for z in [0.0, 1.0] {
        for y in [-0.045, 0.045] {
            for x in [-0.15, 0.15] {
                template.push(Vec3::new(x, y, z));
                height_delta.push(Vec3::new(0.0, 0.0, z * 0.5));
                width_delta.push(Vec3::new(x * 4.0 / 3.0, y * 4.0 / 3.0, 0.0));
            }
        }
    }
    template.extend([Vec3::new(0.5, 0.0, 0.75), Vec3::new(0.35, 0.02, 0.72)]);
    height_delta.extend([Vec3::new(0.175, 0.0, 0.375), Vec3::new(0.1, 0.0, 0.36)]);
    width_delta.extend([
        Vec3::new(0.2, 0.0, 0.0),
        Vec3::new(0.2, 0.02 * 4.0 / 3.0, 0.0),
    ]);
    let mut arrays = BTreeMap::new();
    let mut data = Vec::new();
    let mut add = |name: &str, dtype: &str, shape: Vec<usize>, bytes: Vec<u8>| {
        arrays.insert(name.to_string(),json!({"dtype":dtype,"shape":shape,"offset":data.len(),"nbytes":bytes.len(),"sha256":sha256_hex(&bytes)}));
        data.extend(bytes);
    };
    add("template_vertices", "f64", vec![10, 3], xyz(&template));
    add(
        "faces",
        "u32",
        vec![13, 3],
        u32_bytes([
            0, 1, 2, 1, 3, 2, 1, 8, 9, 4, 6, 7, 4, 7, 5, 0, 4, 5, 0, 5, 1, 2, 3, 7, 2, 7, 6, 0, 2,
            6, 0, 6, 4, 1, 5, 7, 1, 7, 3,
        ]),
    );
    add("blendshape_row_ptr", "u32", vec![3], u32_bytes([0, 10, 20]));
    add(
        "blendshape_vertex",
        "u32",
        vec![20],
        u32_bytes((0..10).chain(0..10)),
    );
    add(
        "blendshape_delta",
        "f64",
        vec![20, 3],
        xyz(&[height_delta, width_delta].concat()),
    );
    let mut mask = vec![0; 52];
    mask[1] = 1;
    mask[26 + 3] = 1;
    add("phenotype_mask", "u8", vec![2, 26], mask);
    let heads = [Vec3::ZERO, Vec3::new(0.15, 0.0, 0.75)];
    let tails = [Vec3::new(0.0, 0.0, 0.1), Vec3::new(0.5, 0.0, 0.75)];
    add("template_bone_heads", "f64", vec![2, 3], xyz(&heads));
    add("template_bone_tails", "f64", vec![2, 3], xyz(&tails));
    add(
        "bone_heads_blendshapes",
        "f64",
        vec![2, 2, 3],
        xyz(&[
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 0.375),
            Vec3::ZERO,
            Vec3::new(0.2, 0.0, 0.0),
        ]),
    );
    add(
        "bone_tails_blendshapes",
        "f64",
        vec![2, 2, 3],
        xyz(&[
            Vec3::ZERO,
            Vec3::new(0.175, 0.0, 0.375),
            Vec3::ZERO,
            Vec3::new(0.2, 0.0, 0.0),
        ]),
    );
    add(
        "bone_rolls",
        "f64",
        vec![2, 3, 3],
        f64_bytes([1., 0., 0., 0., 1., 0., 0., 0., 1.].repeat(2)),
    );
    add(
        "vertex_bone_indices",
        "u32",
        vec![10, 1],
        u32_bytes([0, 0, 0, 0, 0, 0, 0, 0, 1, 1]),
    );
    add(
        "vertex_bone_weights",
        "f64",
        vec![10, 1],
        f64_bytes([1.; 10]),
    );
    add(
        "base_mesh_vertex_indices",
        "u32",
        vec![10],
        u32_bytes(0..10),
    );
    let mut components = vec![
        json!(["height", "low"]),
        json!(["height", "high"]),
        json!(["weight", "low"]),
        json!(["weight", "high"]),
    ];
    components.extend((0..22).map(|i| json!(["race", format!("r{i}")])));
    let header = json!({"schema":"ftxbody/1","model":"anny","anny_version":"0.6.1","configuration":"synthetic-test-only",
        "licence":"synthetic numerical test","package_sha256":{},"counts":{"vertices":10,"faces":13,"bones":2,"blendshapes":2,"phenotype_blendshapes":2,"local_changes":0,"influences":1},
        "phenotype":{"features":["height","weight","race"],"components":components,"anchors":{"height":[0,1],"weight":[0,1]},"exposed":["height","weight"],"fixed_value":0.5},
        "y_axis":[0,1,0],"degenerate_rotation":[[1,0,0],[0,1,0],[0,0,1]],"labels":{"blendshapes":["height-high","weight-high"],"bones":["root","arm"],"bone_parents":[-1,0],"local_changes":[]},"waist_loop_final":[],"arrays":arrays});
    let mut h = serde_json::to_vec(&header).unwrap();
    while !h.len().is_multiple_of(8) {
        h.push(b' ');
    }
    let mut bytes = b"FTXBODY\0".to_vec();
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend((h.len() as u64).to_le_bytes());
    bytes.extend(h);
    bytes.extend(data);
    bytes
}

fn param(name: &str, lo: f64, hi: f64) -> Value {
    json!({"name":name,"lower":lo,"upper":hi})
}
fn fixture() -> (Vec<u8>, Value) {
    fixture_scaled(1.0)
}
fn fixture_scaled(scale: f64) -> (Vec<u8>, Value) {
    let bytes = synthetic_model_scaled(scale);
    let model = AnnyModel::from_data(AnnyData::from_bytes(&bytes, true).unwrap()).unwrap();
    let body = model.params(Phenotype {
        height: 0.6,
        weight: 0.45,
        ..Phenotype::default()
    });
    let rest = model.rest(&body).unwrap();
    let mut poses = [
        [0.0, 0.1, 0.05, 0.04, -0.05, 0.02, -0.4],
        [0.2, -0.1, 0.08, -0.03, 0.06, -0.02, 0.5],
    ];
    for pose in &mut poses {
        for value in &mut pose[..3] {
            *value *= scale;
        }
    }
    let mut frames = Vec::new();
    for (i, p) in poses.iter().enumerate() {
        let rv = Vec3::new(p[3], p[4], p[5]);
        let deltas = [
            Affine {
                m: Mat3::rotation(rv.normalized(), rv.norm()),
                t: Vec3::new(p[0], p[1], p[2]),
            },
            Affine {
                m: Mat3::rotation(Vec3::new(1.0, 0.0, 0.0), p[6]),
                t: Vec3::ZERO,
            },
        ];
        let posed = model.pose(&rest, &deltas).unwrap();
        let camera = Camera {
            intrinsics: Intrinsics {
                focal_px: Vec2::new(1000., 1000.),
                principal_px: Vec2::new(640., 480.),
                distortion: Distortion::default(),
            },
            world_to_camera: Mat3::from_rows([[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]]),
            center_m: Vec3::new(0., -4., 0.75) * scale,
        };
        let keypoints: Vec<_> = posed
            .vertices
            .iter()
            .enumerate()
            .map(|(k, &point)| {
                json!({"name":format!("p{k}"),"pixel_px":camera.project(point).unwrap(),
            "sigma_px":1.0,"partition":if k==3{"held_out"}else{"fit"}})
            })
            .collect();
        frames.push(json!({"frame":{"stream":"synthetic-moving-body","timestamp_us":i*200000},"camera":camera,"image_size_px":[1280,960],"rotation_cw_deg":0,"keypoints":keypoints}));
    }
    let starts:Vec<_>=[0.03,-0.025].into_iter().map(|shift|json!({"shape":[0.6+shift,0.45-shift],
        "poses":poses.iter().enumerate().map(|(i,p)|json!({"frame":frames[i]["frame"],"values":p.iter().map(|v|v+shift).collect::<Vec<_>>()})).collect::<Vec<_>>()})).collect();
    let targets: BTreeMap<_, _> = (0..10)
        .map(|i| (format!("p{i}"), json!({"kind":"vertex","index":i})))
        .collect();
    let input = json!({"schema":"ftx-anny-body-fit-input/1","model":{"model_sha256":sha256_hex(&bytes),"age_years":30,"gender":0.5,
        "shape_parameters":[param("height",0.,1.),param("weight",0.,1.)],
        "joints":{"arm":{"axis":[1,0,0]}},
        "pose_parameters":[param("root_x_m",-1.,1.),param("root_y_m",-1.,1.),param("root_z_m",-1.,1.),
            param("root_rx_rad",-1.,1.),param("root_ry_rad",-1.,1.),param("root_rz_rad",-1.,1.),param("joint:arm",-1.5,1.5)],
        "keypoints":targets,"measurements":["stature"]},
        "fit":{"schema":"ftx-body-fit-input/1","metric_scale_source":"synthetic metric camera frame","observation_evidence":"synthetic exact Anny-format vertices",
            "frames":frames,"starts":starts,"measurements":[{"name":"stature","value_m":1.3*scale,"sigma_m":0.002*scale,"evidence":"synthetic independent height","partition":"fit"}]}});
    (bytes, input)
}

fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-body-fit-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let (bytes, input) = fixture();
    std::fs::write(dir.join("model.ftxbody"), bytes).unwrap();
    save(&dir, &input);
    dir
}
fn save(dir: &Path, v: &Value) {
    std::fs::write(
        dir.join("input.json"),
        serde_json::to_vec_pretty(v).unwrap(),
    )
    .unwrap();
}
fn run(dir: &Path, extra: &[&str], code: i32) -> Value {
    let o = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args([
            "measure",
            "fit-body",
            "input.json",
            "--model",
            "model.ftxbody",
            "--json",
        ])
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(
        o.status.code(),
        Some(code),
        "{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn actual_forward_model_adapter_recovers_shape_root_and_joint_motion() {
    let dir = setup();
    let r = run(&dir, &["--out", "fit.json"], 0);
    let v = &r["result"];
    assert_eq!(v["status"], "ACCEPTED");
    assert!((v["solution"]["shape"][0].as_f64().unwrap() - 0.6).abs() < 1e-5);
    assert!((v["solution"]["shape"][1].as_f64().unwrap() - 0.45).abs() < 1e-5);
    assert!((v["solution"]["poses"][0]["values"][6].as_f64().unwrap() + 0.4).abs() < 1e-5);
    assert!((v["solution"]["poses"][1]["values"][6].as_f64().unwrap() - 0.5).abs() < 1e-5);
    assert_eq!(v["solution"]["anny_parameters"]["provenance"], "MODEL_FIT");
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(dir.join("fit.json")).unwrap()).unwrap(),
        *v
    );
    let original = std::fs::read(dir.join("input.json")).unwrap();
    run(&dir, &["--out", "input.json"], 3);
    run(&dir, &["--out", "fit.json"], 3);
    assert_eq!(std::fs::read(dir.join("input.json")).unwrap(), original);
    std::fs::hard_link(dir.join("input.json"), dir.join("alias.json")).unwrap();
    run(&dir, &["--out", "alias.json"], 3);
    assert_eq!(std::fs::read(dir.join("input.json")).unwrap(), original);
}

#[test]
fn reviewed_capture_joins_actual_selection_cameras_and_articulated_fit() {
    let dir = setup();
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    let input = capture::wrap(&dir, original);
    save(&dir, &input);
    let result = run(&dir, &["--mesh-out", "capture-review"], 0)["result"].clone();
    eprintln!("CAPTURE_REVIEW={}", dir.join("capture-review").display());
    assert_eq!(result["status"], "ACCEPTED");
    assert!((result["solution"]["shape"][0].as_f64().unwrap() - 0.6).abs() < 1e-5);
    assert!((result["solution"]["shape"][1].as_f64().unwrap() - 0.45).abs() < 1e-5);
    assert_eq!(
        result["solution"]["anny_parameters"]["provenance"],
        "MODEL_FIT"
    );
    assert_eq!(result["capture_input"]["input"], input);
    for (i, frame) in result["mesh_review"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let image = &frame["source_image"];
        assert_eq!(
            image["sha256"],
            input["fit"]["frames"][i]["source"]["image_sha256"]
        );
        assert_eq!(
            std::fs::read(
                dir.join("capture-review")
                    .join(image["file"].as_str().unwrap())
            )
            .unwrap(),
            std::fs::read(
                dir.join(
                    input["fit"]["frames"][i]["source"]["image"]
                        .as_str()
                        .unwrap()
                )
            )
            .unwrap()
        );
        let svg = std::fs::read_to_string(
            dir.join("capture-review")
                .join(frame["reprojection_svg"]["file"].as_str().unwrap()),
        )
        .unwrap();
        assert!(svg.contains(&format!(
            "<image href=\"{}\"",
            image["file"].as_str().unwrap()
        )));
    }
    assert_eq!(
        result["capture_input"]["frames"].as_array().unwrap().len(),
        2
    );
    assert!(
        result["capture_input"]["frames"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["sheet_pose"]["ok"] == true)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(
            &std::fs::read(dir.join("capture-review/fit.json")).unwrap()
        )
        .unwrap(),
        result
    );
    let before = std::fs::read(dir.join("capture-review/fit.json")).unwrap();
    // RCH retires its temporary source tree. Preserve this actual generated bundle
    // only when requested for the separate offline Chromium image/CSP check.
    if let Some(destination) = std::env::var_os("FTX_CAPTURE_REVIEW_DIR") {
        let destination = PathBuf::from(destination);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&destination).unwrap();
        for file in std::fs::read_dir(dir.join("capture-review")).unwrap() {
            let file = file.unwrap();
            assert!(file.file_type().unwrap().is_file());
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            std::io::copy(
                &mut std::fs::File::open(file.path()).unwrap(),
                &mut options.open(destination.join(file.file_name())).unwrap(),
            )
            .unwrap();
        }
        eprintln!("CAPTURE_REVIEW_ARTIFACTS={}", destination.display());
    }
    run(&dir, &["--mesh-out", "capture-review"], 3);
    assert_eq!(
        std::fs::read(dir.join("capture-review/fit.json")).unwrap(),
        before
    );
    let budget = run(&dir, &["--max-evaluations", "1"], 4);
    assert_eq!(budget["result"]["status"], "BUDGET_STOP");
    assert!(budget["result"]["solution"].is_null());
    assert_eq!(budget["result"]["capture_input"]["input"], input);
    let mut failed = input;
    failed["fit"]["frames"][0]["keypoints"][3]["pixel_px"][0] = json!(
        failed["fit"]["frames"][0]["keypoints"][3]["pixel_px"][0]
            .as_f64()
            .unwrap()
            + 20.0
    );
    save(&dir, &failed);
    let rejected = run(&dir, &[], 2);
    assert_eq!(rejected["result"]["status"], "REJECTED");
    assert!(rejected["result"]["solution"].is_null());
    for (a, b) in result["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .zip(rejected["result"]["attempts"].as_array().unwrap())
    {
        assert_eq!(
            a["cost_history"], b["cost_history"],
            "capture held-out observations entered optimization"
        );
    }
}

fn capture_review(dir: &Path, output: &str, code: i32) -> Value {
    capture_review_args(dir, &["input.json", "--out", output], code)
}

fn capture_review_args(dir: &Path, args: &[&str], code: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(["measure", "capture-review"])
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn capture_acquires_actual_charuco_pixels_and_replays_the_checked_cameras() {
    let dir = setup();
    // A small synthetic model lets the camera resolve a physical-sized board at 1280×960;
    // no production input is rescaled or granted a different image budget.
    let (model, original) = fixture_scaled(0.25);
    std::fs::write(dir.join("model.ftxbody"), model).unwrap();
    let mut input = original;
    // Actual marker pixels reach the detector; body correspondences remain synthetic.
    capture::wrap_images(&dir, &mut input, true);
    save(&dir, &input);
    let review = capture_review(&dir, "charuco-review", 0)["result"].clone();
    assert_eq!(review["body_model_loaded"], false);
    let frames = review["capture_input"]["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 2);
    for (i, frame) in frames.iter().enumerate() {
        let acquisition = &frame["sheet_acquisition"];
        assert_eq!(
            acquisition["detection"]["detector"]["backend"],
            "rust_charuco"
        );
        assert_eq!(
            acquisition["image_sha256"],
            input["fit"]["frames"][i]["source"]["image_sha256"]
        );
        assert_eq!(
            acquisition["sheet_sha256"],
            sha256_hex(&std::fs::read(dir.join("capture-board.json")).unwrap())
        );
        assert_eq!(frame["sheet_pose"]["ok"], true);
        assert_eq!(frame["sheet_pose"]["held_out_points"], 4);
        let corners = acquisition["detection"]["reference"]["corners"]
            .as_array()
            .unwrap();
        let points = frame["sheet_pose"]["input"]["points"].as_array().unwrap();
        assert_eq!(corners.len(), points.len());
        assert_eq!(corners.len(), 42);
        for (corner, point) in corners.iter().zip(points) {
            let id = corner["id"].as_u64().unwrap() as usize;
            assert_eq!(point["pixel_px"], corner["point_px"]);
            assert_eq!(point["held_out"], [0, 6, 35, 41].contains(&id));
            assert_eq!(
                point["sheet_xz_m"],
                json!([
                    0.15 + (id % 7 + 1) as f64 * 0.022,
                    0.10 + (6 - id / 7) as f64 * 0.022
                ])
            );
        }
        // The original camera is known independently of the rendered detections.
        let camera: Camera = serde_json::from_value(frame["sheet_pose"]["camera"].clone()).unwrap();
        assert!((camera.center_m - Vec3::new(0.0, -1.0, 0.1875)).norm() < 0.04);
        let overlay =
            std::fs::read_to_string(dir.join(format!("charuco-review/sheet-{i:04}.svg"))).unwrap();
        assert!(overlay.contains("CHECK") && overlay.contains("FIT"));
    }
    let replay: Value =
        serde_json::from_slice(&std::fs::read(dir.join("charuco-review/capture.json")).unwrap())
            .unwrap();
    assert!(
        Path::new(
            replay["fit"]["frames"][0]["reference"]["sheet"]
                .as_str()
                .unwrap()
        )
        .is_absolute()
    );
    save(&dir, &replay);
    let again = capture_review(&dir, "charuco-replay", 0)["result"].clone();
    assert_eq!(
        again["capture_input"]["sources_sha256"],
        review["capture_input"]["sources_sha256"]
    );
    assert_eq!(
        again["capture_input"]["frames"],
        review["capture_input"]["frames"]
    );

    // Reference-path input consumes these exact acquired correspondences through the same camera
    // authority. Only source metadata differs; the body fit must be numerically identical.
    let acquired = run(&dir, &[], 0)["result"].clone();
    let mut manual = input.clone();
    for (i, frame) in frames.iter().enumerate() {
        let mut reference = frame["sheet_pose"]["input"].clone();
        reference.as_object_mut().unwrap().remove("intrinsics");
        let path = format!("acquired-points-{i}.json");
        std::fs::write(
            dir.join(&path),
            serde_json::to_vec_pretty(&reference).unwrap(),
        )
        .unwrap();
        manual["fit"]["frames"][i]["reference"] = json!(path);
    }
    save(&dir, &manual);
    let authored = run(&dir, &[], 0)["result"].clone();
    for key in [
        "input",
        "solution",
        "attempts",
        "flags",
        "model_evaluations",
    ] {
        assert_eq!(acquired[key], authored[key], "{key}");
    }

    let mut changed = replay.clone();
    changed["fit"]["frames"][0]["reference"]["held_out_corner_ids"] = json!([1, 6, 35, 41]);
    save(&dir, &changed);
    let refused = capture_review(&dir, "changed-partitions", 3);
    assert!(
        refused["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("sources changed")
    );
    assert!(!dir.join("changed-partitions").exists());
    let mut shifted = input.clone();
    shifted["fit"]["frames"][1]["reference"]["board_bottom_left_xz_m"] = json!([0.151, 0.10]);
    save(&dir, &shifted);
    assert!(
        capture_review(&dir, "changed-origin", 3)["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("cannot change sidecar or board bottom-left")
    );
    assert!(!dir.join("changed-origin").exists());
    save(&dir, &replay);
    let mut sheet: Value =
        serde_json::from_slice(&std::fs::read(dir.join("capture-board.json")).unwrap()).unwrap();
    sheet["print_instructions"] =
        json!("Changed source sidecar must not silently refresh a reviewed capture");
    std::fs::write(
        dir.join("capture-board.json"),
        serde_json::to_vec(&sheet).unwrap(),
    )
    .unwrap();
    assert!(
        capture_review(&dir, "changed-sheet", 3)["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("sources changed")
    );
    assert!(!dir.join("changed-sheet").exists());

    // Remove both marker supports around predeclared CHECK corner 0 in the actual pixels.
    // Resolve a fresh unbound capture so this exercises missing observation admission, rather
    // than merely the already-tested stale-image hash check. Other corners remain available.
    let path = dir.join("capture-frame-0.png");
    let mut image = ftx_trace::raster::image::load_png(&std::fs::read(&path).unwrap()).unwrap();
    for y in 412..459 {
        for x in 782..829 {
            let offset = (y * image.w + x) * 3;
            image.data[offset..offset + 3].fill(255);
        }
    }
    std::fs::write(
        path,
        ftx_trace::raster::image::encode_png_bgr(&image).unwrap(),
    )
    .unwrap();
    let mut occluded = input;
    for frame in occluded["fit"]["frames"].as_array_mut().unwrap() {
        let id = frame["source"]["metadata"]["frame"].clone();
        frame.as_object_mut().unwrap().remove("source");
        frame["frame"] = id;
    }
    save(&dir, &occluded);
    assert!(
        capture_review(&dir, "occluded-check", 3)["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("every predeclared CHECK")
    );
    assert!(!dir.join("occluded-check").exists());
}

#[test]
fn capture_rectangular_sharpness_regions_preserve_numerical_fit_and_bind_source_edits() {
    let dir = setup();
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    let mut input = capture::wrap(&dir, original);
    save(&dir, &input);
    let mut file_fit = run(&dir, &[], 0)["result"].clone();
    let path = dir.join("capture-frames.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for (i, frame) in manifest["frames"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        frame["person_mask"] = json!({"rect_px":[0,0,640,960]});
        frame["sheet_mask"] = json!({"rect_px":[640,0,640,960]});
        // The new path must work without either binary mask file; preserve them under new names.
        for kind in ["person", "sheet"] {
            std::fs::rename(
                dir.join(format!("capture-{kind}-{i}.png")),
                dir.join(format!("capture-{kind}-{i}.unused")),
            )
            .unwrap();
        }
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    let selected = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args(["measure", "video-select", "capture-frames.json", "--json"])
        .output()
        .unwrap();
    assert!(
        selected.status.success(),
        "{}",
        String::from_utf8_lossy(&selected.stderr)
    );
    let selected: Value = serde_json::from_slice(&selected.stdout).unwrap();
    for frame in input["fit"]["frames"].as_array_mut().unwrap() {
        let id = frame["source"]["metadata"]["frame"].clone();
        frame["source"] = selected["result"]["selected_sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["metadata"]["frame"] == id)
            .unwrap()
            .clone();
        assert!(frame["source"].get("person_mask_file_sha256").is_none());
    }
    save(&dir, &input);
    let review = capture_review(&dir, "rect-review", 0)["result"].clone();
    let old_capture = file_fit["capture_input"].clone();
    let current = &review["capture_input"];
    assert_ne!(current["sources_sha256"], old_capture["sources_sha256"]);
    assert_eq!(
        current["selection"]["windows"],
        old_capture["selection"]["windows"]
    );
    for (a, b) in current["frames"]
        .as_array()
        .unwrap()
        .iter()
        .zip(old_capture["frames"].as_array().unwrap())
    {
        for key in [
            "frame",
            "lens_id",
            "calibration_sha256",
            "calibration",
            "sheet_pose",
            "reference_input_sha256",
        ] {
            assert_eq!(a[key], b[key]);
        }
        for key in [
            "image_sha256",
            "decoded_gray_sha256",
            "person_mask_sha256",
            "sheet_mask_sha256",
        ] {
            assert_eq!(a["source"][key], b["source"][key]);
        }
    }
    let replay: Value =
        serde_json::from_slice(&std::fs::read(dir.join("rect-review/capture.json")).unwrap())
            .unwrap();
    assert_eq!(replay["reviewed_sources_sha256"], current["sources_sha256"]);
    save(&dir, &replay);
    let mut region_fit = run(&dir, &[], 0)["result"].clone();
    assert_eq!(
        region_fit["input_sha256"],
        sha256_hex(&std::fs::read(dir.join("input.json")).unwrap())
    );
    assert_eq!(region_fit["capture_input"]["input"], replay);
    assert_eq!(
        region_fit["capture_input"]["sources_sha256"],
        current["sources_sha256"]
    );
    // Compare every numerical/model/fit field. Different source declarations and exact input
    // bytes were checked separately above; they must not be presented as the same evidence.
    for report in [&mut file_fit, &mut region_fit] {
        report.as_object_mut().unwrap().remove("input_sha256");
        report.as_object_mut().unwrap().remove("capture_input");
    }
    assert_eq!(region_fit, file_fit);
    manifest["frames"][0]["person_mask"]["rect_px"][2] = json!(639);
    std::fs::write(path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    std::fs::rename(dir.join("model.ftxbody"), dir.join("unused-model.ftxbody")).unwrap();
    let stale = run(&dir, &["--out", "stale-region.json"], 3);
    assert!(
        stale["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("capture source changed")
    );
    assert!(!dir.join("stale-region.json").exists());
}

#[test]
fn capture_review_runs_without_model_and_replays_the_original_fit() {
    let dir = setup();
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    let mut input = capture::wrap(&dir, original);
    input["reference_id"] = json!("sheet </script><script>globalThis.sheetInjection=1</script>&\"");
    for i in 0..2 {
        let path = dir.join(format!("capture-reference-{i}.json"));
        let mut reference: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        reference["reference_id"] = input["reference_id"].clone();
        // Retain a small nonzero CHECK residual; the display must not round it away.
        reference["points"][1]["pixel_px"][0] =
            json!(reference["points"][1]["pixel_px"][0].as_f64().unwrap() + 0.25);
        std::fs::write(path, serde_json::to_vec_pretty(&reference).unwrap()).unwrap();
    }
    save(&dir, &input);
    let bytes = std::fs::read(dir.join("input.json")).unwrap();
    // Move, never delete, the test model: the review must not depend on its availability.
    std::fs::rename(dir.join("model.ftxbody"), dir.join("unused-model.ftxbody")).unwrap();
    let report = capture_review(&dir, "before-fit", 0)["result"].clone();
    assert_eq!(report["status"], "PREPARED");
    assert_eq!(report["body_model_loaded"], false);
    assert_eq!(report["body_fitted"], false);
    assert!(report.get("solution").is_none());
    assert_eq!(report["input_sha256"], sha256_hex(&bytes));
    assert_eq!(
        std::fs::read(dir.join("before-fit/original-input.json")).unwrap(),
        bytes
    );
    assert_eq!(report["capture_input"]["input"], input);
    assert!(!dir.join("before-fit/fit.json").exists());
    let saved: Value =
        serde_json::from_slice(&std::fs::read(dir.join("before-fit/review.json")).unwrap())
            .unwrap();
    assert_eq!(saved, report);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.join("before-fit"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for entry in std::fs::read_dir(dir.join("before-fit")).unwrap() {
            assert_eq!(
                entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    for (i, frame) in report["frames"].as_array().unwrap().iter().enumerate() {
        assert_eq!(frame["keypoints"], input["fit"]["frames"][i]["keypoints"]);
        assert_eq!(
            frame["frame"],
            input["fit"]["frames"][i]["source"]["metadata"]["frame"]
        );
        for key in ["source_image", "keypoint_svg", "sheet_svg"] {
            let artifact = &frame[key];
            let data = std::fs::read(
                dir.join("before-fit")
                    .join(artifact["file"].as_str().unwrap()),
            )
            .unwrap();
            assert_eq!(sha256_hex(&data), artifact["sha256"]);
        }
        let svg = std::fs::read_to_string(
            dir.join("before-fit")
                .join(frame["keypoint_svg"]["file"].as_str().unwrap()),
        )
        .unwrap();
        assert_eq!(
            svg.matches("<circle ").count(),
            frame["keypoints"].as_array().unwrap().len()
        );
        assert!(svg.contains("data-partition=\"FIT\""));
        assert!(svg.contains("data-partition=\"CHECK\""));
        assert!(!svg.contains("predicted"));
        let sheet = &report["capture_input"]["frames"][i]["sheet_pose"];
        let sheet_svg = std::fs::read_to_string(
            dir.join("before-fit")
                .join(frame["sheet_svg"]["file"].as_str().unwrap()),
        )
        .unwrap();
        let camera: Camera = serde_json::from_value(sheet["camera"].clone()).unwrap();
        let points = sheet["input"]["points"].as_array().unwrap();
        assert_eq!(
            sheet_svg.matches("<g data-sheet-point=").count(),
            points.len()
        );
        assert_eq!(sheet_svg.matches("data-partition=\"CHECK\"").count(), 3);
        for (j, point) in points.iter().enumerate() {
            let p: Vec2 = serde_json::from_value(point["pixel_px"].clone()).unwrap();
            let xz: Vec2 = serde_json::from_value(point["sheet_xz_m"].clone()).unwrap();
            let projected = camera.project(Vec3::new(xz.x, 0.0, xz.y)).unwrap();
            let residual = sheet["reprojection"][j]["error_px"].as_f64().unwrap();
            assert!(
                sheet_svg.contains(&format!("class=\"observed\" cx=\"{}\" cy=\"{}\"", p.x, p.y))
            );
            assert!(sheet_svg.contains(&format!(
                "class=\"projected\" data-x=\"{}\" data-y=\"{}\"",
                projected.x, projected.y
            )));
            assert!(sheet_svg.contains(&format!("residual {residual} px")));
        }
        assert!(sheet["reprojection"][1]["error_px"].as_f64().unwrap() > 0.24);
        assert_eq!(
            frame["source_image"]["sha256"],
            input["fit"]["frames"][i]["source"]["image_sha256"]
        );
    }
    let html = std::fs::read_to_string(dir.join("before-fit/index.html")).unwrap();
    assert!(html.contains("sheet &lt;/script&gt;&lt;script&gt;"));
    assert!(!html.contains("<script>globalThis.sheetInjection"));
    let replay: Value =
        serde_json::from_slice(&std::fs::read(dir.join("before-fit/capture.json")).unwrap())
            .unwrap();
    let mut expected = input.clone();
    expected["video_manifest"] = json!(dir.join("capture-frames.json"));
    expected["reviewed_sources_sha256"] = report["capture_input"]["sources_sha256"].clone();
    for (i, f) in expected["fit"]["frames"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        f["calibration"] = json!(dir.join("capture-calibration.json"));
        f["reference"] = json!(dir.join(format!("capture-reference-{i}.json")));
    }
    assert_eq!(replay, expected);
    capture_review(&dir, "before-fit", 3);
    assert_eq!(
        std::fs::read(dir.join("before-fit/original-input.json")).unwrap(),
        bytes
    );
    std::fs::rename(dir.join("unused-model.ftxbody"), dir.join("model.ftxbody")).unwrap();
    let original_fit = run(&dir, &[], 0)["result"].clone();
    // A different cwd exposes accidental dependence on the review output's relative paths.
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir.join("before-fit"))
        .args([
            "measure",
            "fit-body",
            "capture.json",
            "--model",
            "../model.ftxbody",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let replayed: Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut replay_fit = replayed["result"].clone();
    assert_eq!(
        replay_fit["input_sha256"],
        sha256_hex(&std::fs::read(dir.join("before-fit/capture.json")).unwrap())
    );
    assert_eq!(replay_fit["capture_input"]["input"], replay);
    // Only the request's exact byte identity, location and explicitly rebased local paths differ.
    replay_fit["input_sha256"] = original_fit["input_sha256"].clone();
    replay_fit["capture_input"]["source"] = original_fit["capture_input"]["source"].clone();
    replay_fit["capture_input"]["input"] = original_fit["capture_input"]["input"].clone();
    assert_eq!(replay_fit, original_fit);
    let reference_path = dir.join("capture-reference-0.json");
    let mut changed_reference = std::fs::read(&reference_path).unwrap();
    changed_reference.push(b'\n');
    std::fs::write(&reference_path, changed_reference).unwrap();
    let stale = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir.join("before-fit"))
        .args([
            "measure",
            "fit-body",
            "capture.json",
            "--model",
            "absent.ftxbody",
            "--out",
            "stale-fit.json",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(stale.status.code(), Some(3));
    let stale: Value = serde_json::from_slice(&stale.stdout).unwrap();
    assert!(
        stale["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("reviewed capture sources changed")
    );
    assert!(!dir.join("before-fit/stale-fit.json").exists());
    // Optional durable copy for inspecting these actual generated bytes in Chromium after RCH retires its tree.
    if let Some(destination) = std::env::var_os("FTX_PREFIT_REVIEW_DIR") {
        let destination = PathBuf::from(destination);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&destination).unwrap();
        for entry in std::fs::read_dir(dir.join("before-fit")).unwrap() {
            let entry = entry.unwrap();
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = options.open(destination.join(entry.file_name())).unwrap();
            std::io::copy(&mut std::fs::File::open(entry.path()).unwrap(), &mut output).unwrap();
        }
        eprintln!("PREFIT_REVIEW={}", destination.display());
    }
}

#[test]
fn capture_review_refuses_stale_sources_bad_points_and_escaped_targets_are_literal() {
    let dir = setup();
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    let input = capture::wrap(&dir, original);
    let point = input["fit"]["frames"][0]["keypoints"][0].clone();
    let mut outside = point.clone();
    outside["pixel_px"] = json!([-1, 20]);
    let mut sigma = point.clone();
    sigma["sigma_px"] = json!(0);
    for (i, change) in [
        ("source", json!(null)),
        ("lens_id", json!("another-lens")),
        ("calibration_sha256", json!("wrong-hash")),
        (
            "keypoints",
            json!([{"name":"missing","pixel_px":[10,20],"sigma_px":1,"partition":"fit"}]),
        ),
        ("keypoints", json!([outside])),
        ("keypoints", json!([sigma])),
        ("keypoints", json!([point, point])),
    ]
    .into_iter()
    .enumerate()
    {
        let mut changed = input.clone();
        changed["fit"]["frames"][0][change.0] = change.1;
        save(&dir, &changed);
        let output = format!("refused-{i}");
        capture_review(&dir, &output, 3);
        assert!(!dir.join(output).exists());
    }
    let mut labelled = input;
    let name = labelled["fit"]["frames"][0]["keypoints"][0]["name"]
        .as_str()
        .unwrap()
        .to_string();
    let target = labelled["model"]["keypoints"]
        .as_object_mut()
        .unwrap()
        .remove(&name)
        .unwrap();
    let hostile = "<script>bad()</script>&\"\0";
    labelled["model"]["keypoints"][hostile] = target;
    labelled["fit"]["frames"][0]["keypoints"][0]["name"] = json!(hostile);
    // Other frames may still name the original correspondence; keep both explicit definitions.
    labelled["model"]["keypoints"][&name] = labelled["model"]["keypoints"][hostile].clone();
    save(&dir, &labelled);
    let result = capture_review(&dir, "escaped", 0);
    assert_eq!(
        result["result"]["frames"][0]["keypoints"][0]["name"],
        hostile
    );
    let svg = std::fs::read_to_string(dir.join("escaped/frame-0000.svg")).unwrap();
    assert!(svg.contains("&lt;script&gt;bad()&lt;/script&gt;&amp;&quot;"));
    assert!(!svg.contains("<script>"));
    let before = std::fs::read(dir.join("escaped/review.json")).unwrap();
    let path = dir.join("capture-frame-0.png");
    let changed = Gray {
        w: 1280,
        h: 960,
        data: vec![0; 1280 * 960],
    };
    std::fs::write(path, encode_png_gray(&changed)).unwrap();
    let refused = capture_review(&dir, "stale-image", 3);
    assert!(refused["refusal"].is_object());
    assert!(!dir.join("stale-image").exists());
    assert_eq!(
        std::fs::read(dir.join("escaped/review.json")).unwrap(),
        before
    );
}

#[test]
fn capture_editor_fixture_rejects_bad_check_and_keeps_a_positive_native_reference() {
    // The same assertions run normally; the optional path only retains this synthetic workspace
    // beyond RCH source retirement for the real browser -> ordinary CLI replay.
    let dir = if let Some(path) = std::env::var_os("FTX_CAPTURE_EDITOR_DIR") {
        let path = PathBuf::from(path);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path).unwrap();
        let (bytes, input) = fixture();
        std::fs::write(path.join("model.ftxbody"), bytes).unwrap();
        save(&path, &input);
        path
    } else {
        setup()
    };
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    let mut good = capture::wrap(&dir, original);
    let hostile = "</script><script>globalThis.captureInjection=1</script>&\"";
    let target = good["model"]["keypoints"]
        .as_object_mut()
        .unwrap()
        .remove("p9")
        .unwrap();
    good["model"]["keypoints"][hostile] = target;
    for frame in good["fit"]["frames"].as_array_mut().unwrap() {
        frame["keypoints"][9]["name"] = json!(hostile);
    }
    std::fs::write(
        dir.join("good.json"),
        serde_json::to_vec_pretty(&good).unwrap(),
    )
    .unwrap();
    let mut bad = good.clone();
    assert_eq!(
        bad["fit"]["frames"][0]["keypoints"][3]["partition"],
        "held_out"
    );
    bad["fit"]["frames"][0]["keypoints"][3]["pixel_px"][0] = json!(
        good["fit"]["frames"][0]["keypoints"][3]["pixel_px"][0]
            .as_f64()
            .unwrap()
            + 12.0
    );
    save(&dir, &bad);
    let review = capture_review(&dir, "edit-review", 0)["result"].clone();
    assert_eq!(review["status"], "PREPARED");
    assert_eq!(review["body_fitted"], false);
    let script = std::fs::read(dir.join("edit-review/review.js")).unwrap();
    assert_eq!(review["editor"]["sha256"], sha256_hex(&script));
    assert_eq!(
        script,
        include_bytes!("../src/measure/body_fit/capture_review.js")
    );
    let page = std::fs::read_to_string(dir.join("edit-review/index.html")).unwrap();
    assert!(page.contains("script-src 'self'"));
    assert!(page.contains("\\u003c/script>"));
    assert!(!page.contains(hostile));
    let rejected = run(&dir, &["--out", "bad-fit.json"], 2);
    assert!(rejected["result"]["solution"].is_null());
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args([
            "measure",
            "fit-body",
            "good.json",
            "--model",
            "model.ftxbody",
            "--out",
            "expected-fit.json",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let expected: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(expected["result"]["status"], "ACCEPTED");
    assert!((expected["result"]["solution"]["shape"][0].as_f64().unwrap() - 0.6).abs() < 1e-5);
    assert!((expected["result"]["solution"]["shape"][1].as_f64().unwrap() - 0.45).abs() < 1e-5);
    assert_eq!(review["capture_input"]["input"], bad);
    assert_eq!(
        std::fs::read(dir.join("edit-review/original-input.json")).unwrap(),
        std::fs::read(dir.join("input.json")).unwrap()
    );
    // Initial placement starts with no guessed or placeholder observations. Cameras, model
    // targets and all decoded-source identities are the same independently authored fixture.
    let mut empty = good.clone();
    for frame in empty["fit"]["frames"].as_array_mut().unwrap() {
        frame["keypoints"] = json!([]);
    }
    std::fs::write(
        dir.join("empty.json"),
        serde_json::to_vec_pretty(&empty).unwrap(),
    )
    .unwrap();
    let full_empty =
        capture_review_args(&dir, &["empty.json", "--out", "empty-full-review"], 0)["result"]
            .clone();
    let mut declarations = empty.clone();
    for frame in declarations["fit"]["frames"].as_array_mut().unwrap() {
        frame["frame"] = frame["source"]["metadata"]["frame"].clone();
        let object = frame.as_object_mut().unwrap();
        object.remove("source");
        object.remove("calibration_sha256");
    }
    std::fs::write(
        dir.join("initial.json"),
        serde_json::to_vec_pretty(&declarations).unwrap(),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args([
            "measure",
            "capture-review",
            "initial.json",
            "--out",
            "empty-review",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let prepared: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(prepared["result"]["status"], "PREPARED");
    assert_eq!(prepared["result"]["body_fitted"], false);
    assert_eq!(prepared["result"]["capture_input"]["input"], declarations);
    assert_eq!(full_empty["status"], "PREPARED");
    assert_eq!(full_empty["body_fitted"], false);
    assert_eq!(full_empty["capture_input"]["input"], empty);
    assert_eq!(prepared["result"]["frames"], full_empty["frames"]);
    let replay: Value =
        serde_json::from_slice(&std::fs::read(dir.join("empty-review/capture.json")).unwrap())
            .unwrap();
    let full_replay: Value =
        serde_json::from_slice(&std::fs::read(dir.join("empty-full-review/capture.json")).unwrap())
            .unwrap();
    assert_eq!(replay, full_replay);
    assert_eq!(
        std::fs::read(dir.join("empty-review/original-input.json")).unwrap(),
        std::fs::read(dir.join("initial.json")).unwrap()
    );
    for frame in replay["fit"]["frames"].as_array().unwrap() {
        assert!(frame.get("frame").is_none());
        assert!(frame["source"].is_object());
        assert_eq!(frame["calibration_sha256"].as_str().unwrap().len(), 64);
    }
    assert_eq!(
        prepared["result"]["capture_input"]["sources_sha256"],
        review["capture_input"]["sources_sha256"]
    );
    for frame in prepared["result"]["frames"].as_array().unwrap() {
        assert_eq!(frame["keypoints"], json!([]));
    }
    let refused = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args([
            "measure",
            "fit-body",
            "initial.json",
            "--model",
            "model.ftxbody",
            "--out",
            "unprepared-fit.json",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        refused.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&refused.stdout)
    );
    assert!(!dir.join("unprepared-fit.json").exists());
    let mut ambiguous = declarations.clone();
    ambiguous["fit"]["frames"][0]["source"] = empty["fit"]["frames"][0]["source"].clone();
    let mut missing = declarations.clone();
    missing["fit"]["frames"][0]["frame"]["timestamp_us"] = json!(1);
    let mut duplicate = declarations.clone();
    duplicate["fit"]["frames"][1]["frame"] = duplicate["fit"]["frames"][0]["frame"].clone();
    let mut wrong_hash = declarations.clone();
    wrong_hash["fit"]["frames"][0]["calibration_sha256"] = json!("0".repeat(64));
    let mut bound = declarations.clone();
    bound["reviewed_sources_sha256"] = review["capture_input"]["sources_sha256"].clone();
    let mut incomplete = empty.clone();
    incomplete["fit"]["frames"][0]
        .as_object_mut()
        .unwrap()
        .remove("calibration_sha256");
    for (i, invalid) in [ambiguous, missing, duplicate, wrong_hash, bound, incomplete]
        .iter()
        .enumerate()
    {
        save(&dir, invalid);
        let output = format!("invalid-declaration-{i}");
        capture_review(&dir, &output, 3);
        assert!(!dir.join(output).exists());
    }
    save(&dir, &bad);
    eprintln!("CAPTURE_EDITOR_CASE={}", dir.display());
    let unsafe_dir = dir.join("numeric-unsafe");
    std::fs::create_dir(&unsafe_dir).unwrap();
    let (_, mut unsafe_input) = fixture();
    for i in 0..2 {
        let timestamp = 9_007_199_254_740_993u64 + i as u64 * 200_000;
        unsafe_input["fit"]["frames"][i]["frame"]["timestamp_us"] = json!(timestamp);
        for start in unsafe_input["fit"]["starts"].as_array_mut().unwrap() {
            start["poses"][i]["frame"]["timestamp_us"] = json!(timestamp);
        }
    }
    let unsafe_input = capture::wrap(&unsafe_dir, unsafe_input);
    save(&unsafe_dir, &unsafe_input);
    let unsafe_review = capture_review(&unsafe_dir, "review", 0);
    assert_eq!(
        unsafe_review["result"]["frames"][0]["frame"]["timestamp_us"].as_u64(),
        Some(9_007_199_254_740_993)
    );
    // Same authored rays on the checked capture sensor, independently rasterized into masks.
    let mut mask_input = fixture().1;
    for i in 0..2 {
        mask_input["fit"]["frames"][i]["keypoints"] = good["fit"]["frames"][i]["keypoints"].clone();
    }
    let mut masks = mask_manifest_for(&dir, &mask_input);
    let manifest_path = dir.join("masks/manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    for frame in manifest["frames"].as_array_mut().unwrap() {
        frame["samples"].as_array_mut().unwrap().extend([
            json!({"axis":"vertical","index":640,"sigma_px":1.0,"partition":"held_out"}),
            json!({"axis":"horizontal","index":50,"sigma_px":2.0,"partition":"fit"}),
        ]);
    }
    // Retain the near-edge column as a failed CHECK case: small raster/fit differences amplify
    // along a steep contour there. The positive route samples an interior column instead.
    std::fs::write(
        dir.join("masks/edge-column.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    for frame in manifest["frames"].as_array_mut().unwrap() {
        frame["samples"][3]["index"] = json!(700);
    }
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let original_manifest = std::fs::read(&manifest_path).unwrap();
    let masked = capture_review_args(
        &dir,
        &[
            "input.json",
            "--out",
            "mask-review",
            "--silhouettes",
            "masks/manifest.json",
        ],
        0,
    )["result"]
        .clone();
    assert_eq!(masked["body_model_loaded"], false);
    assert_eq!(masked["body_fitted"], false);
    assert_eq!(masked["capture_input"]["input"], bad);
    assert_eq!(
        std::fs::read(dir.join("mask-review/original-input.json")).unwrap(),
        std::fs::read(dir.join("input.json")).unwrap()
    );
    let masks_receipt = &masked["capture_input"]["silhouette_input"];
    assert_eq!(
        masks_receipt["manifest_sha256"],
        sha256_hex(&original_manifest)
    );
    assert_eq!(
        masks_receipt["masks"][0]["decoded_mask_sha256"],
        sha256_hex(&masks[0].data)
    );
    assert_ne!(
        masked["capture_input"]["sources_sha256"],
        review["capture_input"]["sources_sha256"]
    );
    for frame in masked["frames"].as_array().unwrap() {
        let lines = frame["silhouette"]["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 5);
        assert!(lines.iter().any(|l| l["intervals_px"] == json!([])));
        let svg = std::fs::read_to_string(
            dir.join("mask-review")
                .join(frame["keypoint_svg"]["file"].as_str().unwrap()),
        )
        .unwrap();
        assert_eq!(svg.matches("data-silhouette=").count(), lines.len());
    }
    let mut masked_good = good.clone();
    masked_good["silhouettes"] = json!("masks/manifest.json");
    std::fs::write(
        dir.join("masked-good.json"),
        serde_json::to_vec_pretty(&masked_good).unwrap(),
    )
    .unwrap();
    let fit = |input: &str, output: &str, extra: &[&str], code| {
        let result = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&dir)
            .args([
                "measure",
                "fit-body",
                input,
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
            "{} / {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["result"].clone()
    };
    let reference = fit("masked-good.json", "expected-masked-fit.json", &[], 0);
    // Author the same positive synthetic constraints inline. The actual browser checker starts
    // without them and produces this ordinary fitter input through explicit operator controls.
    let mut inline_good = good.clone();
    for (i, frame) in inline_good["fit"]["frames"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        frame["silhouette"] = masked["frames"][i]["silhouette"].clone();
        frame["silhouette"]["evidence"] = json!(
            "Synthetic posed surface samples; no clothing, external occluders or real photographs. Explicitly reviewed foreground and background."
        );
    }
    std::fs::write(
        dir.join("inline-good.json"),
        serde_json::to_vec_pretty(&inline_good).unwrap(),
    )
    .unwrap();
    let inline_start =
        capture_review_args(&dir, &["good.json", "--out", "inline-review"], 0)["result"].clone();
    assert_eq!(
        inline_start["capture_input"]["sources_sha256"],
        review["capture_input"]["sources_sha256"]
    );
    let inline_reference = fit("inline-good.json", "expected-inline-fit.json", &[], 0);
    assert_eq!(inline_reference["solution"], reference["solution"]);
    assert_eq!(inline_reference["attempts"], reference["attempts"]);
    let mut failed_inline_check = inline_good.clone();
    let check = failed_inline_check["fit"]["frames"][0]["silhouette"]["lines"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|line| line["partition"] == "held_out" && !line["intervals_px"][0].is_null())
        .unwrap();
    for endpoint in check["intervals_px"][0].as_array_mut().unwrap() {
        *endpoint = json!(endpoint.as_f64().unwrap() + 12.0);
    }
    std::fs::write(
        dir.join("inline-failed-check.json"),
        serde_json::to_vec_pretty(&failed_inline_check).unwrap(),
    )
    .unwrap();
    let failed_inline = fit("inline-failed-check.json", "inline-check-fit.json", &[], 2);
    assert!(failed_inline["solution"].is_null());
    for (before, after) in inline_reference["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .zip(failed_inline["attempts"].as_array().unwrap())
    {
        assert_eq!(before["cost_history"], after["cost_history"]);
        assert!(after["checks"].as_array().unwrap().iter().any(|check| {
            check["partition"] == "held_out"
                && check["name"].as_str().unwrap().starts_with("silhouette:")
                && check["passed"] == false
        }));
    }
    // Preparation preserves disjoint foreground and explicit empty background; it does not turn
    // those authored observations into a fit claim or silently fill the gap between limbs.
    let mut disjoint = good.clone();
    disjoint["fit"]["frames"][0]["silhouette"] = json!({
        "evidence":"Synthetic disjoint contour; clothing absent, occlusion explicitly reviewed.",
        "lines":[
            {"axis":"horizontal","at_px":100.25,"intervals_px":[[100.5,120.25],[200.75,225.5]],"sigma_px":2.5,"partition":"held_out"},
            {"axis":"vertical","at_px":10.5,"intervals_px":[],"sigma_px":1.5,"partition":"fit"}
        ]
    });
    std::fs::write(
        dir.join("inline-disjoint.json"),
        serde_json::to_vec_pretty(&disjoint).unwrap(),
    )
    .unwrap();
    let disjoint_review = capture_review_args(
        &dir,
        &["inline-disjoint.json", "--out", "inline-disjoint-review"],
        0,
    )["result"]
        .clone();
    assert_eq!(disjoint_review["body_fitted"], false);
    assert_eq!(
        disjoint_review["frames"][0]["silhouette"],
        disjoint["fit"]["frames"][0]["silhouette"]
    );
    for (name, lines) in [
        ("overlap", json!([[[100, 210], [200, 225]], []])),
        ("touching", json!([[[100, 200], [200, 225]], []])),
        ("outside", json!([[[-1, 120], [200, 225]], []])),
        ("only-background", json!([[], []])),
    ] {
        let mut invalid = disjoint.clone();
        for i in 0..2 {
            invalid["fit"]["frames"][0]["silhouette"]["lines"][i]["intervals_px"] =
                lines[i].clone();
        }
        let input = format!("inline-{name}.json");
        let output = format!("inline-{name}-review");
        std::fs::write(
            dir.join(&input),
            serde_json::to_vec_pretty(&invalid).unwrap(),
        )
        .unwrap();
        capture_review_args(&dir, &[&input, "--out", &output], 3);
        assert!(!dir.join(output).exists());
    }
    let old_route = fit(
        "good.json",
        "old-mask-fit.json",
        &["--silhouettes", "masks/manifest.json"],
        0,
    );
    assert_eq!(reference["input"], old_route["input"]);
    assert_eq!(reference["solution"], old_route["solution"]);
    assert_eq!(reference["attempts"], old_route["attempts"]);
    let edge = fit(
        "good.json",
        "edge-column-fit.json",
        &["--silhouettes", "masks/edge-column.json"],
        2,
    );
    assert!(edge["attempts"].as_array().unwrap().iter().all(|attempt| {
        attempt["checks"].as_array().unwrap().iter().any(|check| {
            check["name"] == "silhouette:Vertical:640"
                && check["partition"] == "held_out"
                && check["passed"] == false
        })
    }));
    assert_eq!(
        reference["capture_input"]["silhouette_input"],
        old_route["silhouette_input"]
    );
    assert_eq!(
        reference["capture_input"]["sources_sha256"],
        masked["capture_input"]["sources_sha256"]
    );
    fit("mask-review/capture.json", "masked-bad-fit.json", &[], 2);
    // A valid JSON whitespace change and an unsampled interior pixel still change source identity.
    let mut changed = original_manifest.clone();
    changed.push(b'\n');
    std::fs::write(&manifest_path, changed).unwrap();
    fit(
        "mask-review/capture.json",
        "stale-manifest-fit.json",
        &[],
        3,
    );
    assert!(!dir.join("stale-manifest-fit.json").exists());
    std::fs::write(&manifest_path, &original_manifest).unwrap();
    let original_mask = std::fs::read(dir.join("masks/mask-0.png")).unwrap();
    masks[0].data[1281] = 255;
    std::fs::write(dir.join("masks/mask-0.png"), encode_png_gray(&masks[0])).unwrap();
    fit("mask-review/capture.json", "stale-mask-fit.json", &[], 3);
    assert!(!dir.join("stale-mask-fit.json").exists());
    masks[0].data[1281] = 127;
    std::fs::write(dir.join("masks/mask-0.png"), encode_png_gray(&masks[0])).unwrap();
    capture_review_args(
        &dir,
        &[
            "good.json",
            "--out",
            "soft-mask-review",
            "--silhouettes",
            "masks/manifest.json",
        ],
        3,
    );
    assert!(!dir.join("soft-mask-review").exists());
    std::fs::write(dir.join("masks/mask-0.png"), original_mask).unwrap();
    manifest["frames"][1]["frame"] = manifest["frames"][0]["frame"].clone();
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    capture_review_args(
        &dir,
        &[
            "good.json",
            "--out",
            "duplicate-mask-review",
            "--silhouettes",
            "masks/manifest.json",
        ],
        3,
    );
    assert!(!dir.join("duplicate-mask-review").exists());
    std::fs::write(&manifest_path, &original_manifest).unwrap();
    capture_review_args(
        &dir,
        &[
            "masked-good.json",
            "--out",
            "overwrite-mask-review",
            "--silhouettes",
            "masks/manifest.json",
        ],
        3,
    );
    assert!(!dir.join("overwrite-mask-review").exists());
    let mut inline = masked_good.clone();
    inline["fit"]["frames"][0]["silhouette"] = masked["frames"][0]["silhouette"].clone();
    save(&dir, &inline);
    capture_review(&dir, "inline-conflict-review", 3);
    assert!(!dir.join("inline-conflict-review").exists());
    inline.as_object_mut().unwrap().remove("silhouettes");
    inline["fit"]["frames"][0]["silhouette"]["lines"][0]["sigma_px"] = json!(0);
    save(&dir, &inline);
    capture_review(&dir, "invalid-inline-review", 3);
    assert!(!dir.join("invalid-inline-review").exists());
    save(&dir, &bad);
}

#[test]
fn capture_source_identity_lens_and_epoch_conflicts_never_publish_output() {
    let dir = setup();
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    let input = capture::wrap(&dir, original);
    let mut duplicate = input.clone();
    duplicate["fit"]["frames"][1]["source"] = duplicate["fit"]["frames"][0]["source"].clone();
    let mut unknown = input.clone();
    unknown["fit"]["frames"][0]["source"]["metadata"]["frame"]["timestamp_us"] = json!(1);
    let mut stale = input.clone();
    stale["fit"]["frames"][0]["calibration_sha256"] = json!("0".repeat(64));
    let mut lens = input.clone();
    lens["fit"]["frames"][0]["lens_id"] = json!("another-lens");
    let mut reference = input.clone();
    reference["reference_id"] = json!("another-stationary-sheet");
    let mut bypass = input.clone();
    bypass["fit"]["frames"][0]["camera"] = json!({});
    let mut other: Value =
        serde_json::from_slice(&std::fs::read(dir.join("capture-calibration.json")).unwrap())
            .unwrap();
    other["source_note"] = json!("separately retained synthetic calibration artifact");
    let bytes = serde_json::to_vec(&other).unwrap();
    std::fs::write(dir.join("other-calibration.json"), &bytes).unwrap();
    let mut epoch = input;
    epoch["fit"]["frames"][1]["calibration"] = json!("other-calibration.json");
    epoch["fit"]["frames"][1]["calibration_sha256"] = json!(sha256_hex(&bytes));
    for bad in [duplicate, unknown, stale, lens, reference, bypass, epoch] {
        save(&dir, &bad);
        run(&dir, &["--out", "capture-invalid.json"], 3);
        assert!(!dir.join("capture-invalid.json").exists());
    }
}

#[test]
fn capture_changed_pixels_masks_and_held_out_sheet_points_refuse_before_fit() {
    let dir = setup();
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    let input = capture::wrap(&dir, original);
    save(&dir, &input);
    for name in [
        "capture-frame-0.png",
        "capture-person-0.png",
        "capture-sheet-0.png",
    ] {
        let path = dir.join(name);
        let bytes = std::fs::read(&path).unwrap();
        let mut gray = ftx_trace::raster::color::bgr_to_gray(
            &ftx_trace::raster::image::load_png(&bytes).unwrap(),
        );
        gray.data[100] = 255 - gray.data[100];
        std::fs::write(&path, encode_png_gray(&gray)).unwrap();
        run(&dir, &["--out", "changed-capture.json"], 3);
        assert!(!dir.join("changed-capture.json").exists());
        std::fs::write(path, bytes).unwrap();
    }
    let path = dir.join("capture-reference-0.json");
    let mut reference: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    reference["points"][1]["pixel_px"][0] =
        json!(reference["points"][1]["pixel_px"][0].as_f64().unwrap() + 20.0);
    std::fs::write(path, serde_json::to_vec(&reference).unwrap()).unwrap();
    run(&dir, &["--out", "bad-camera.json"], 3);
    assert!(!dir.join("bad-camera.json").exists());
}

#[test]
fn capture_json_and_aggregate_evidence_budgets_publish_no_fit() {
    let dir = setup();
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    let mut input = capture::wrap(&dir, original);
    input["fit"]["observation_evidence"] = json!("x".repeat(4 * 1024 * 1024));
    save(&dir, &input);
    run(&dir, &["--out", "over-budget.json"], 4);
    assert!(!dir.join("over-budget.json").exists());
    std::fs::File::create(dir.join("input.json"))
        .unwrap()
        .set_len(17 * 1024 * 1024)
        .unwrap();
    run(&dir, &["--out", "oversize-input.json"], 4);
    assert!(!dir.join("oversize-input.json").exists());
}

#[test]
fn bone_tail_correspondence_uses_the_same_posed_skeleton_as_the_skin() {
    let dir = setup();
    let mut input = fixture().1;
    // The synthetic arm's final vertex is authored exactly at its bone tail.
    input["model"]["keypoints"]["p8"] = json!({"kind":"bone_tail","bone":"arm"});
    save(&dir, &input);
    let report = run(&dir, &[], 0);
    assert!((report["result"]["solution"]["shape"][0].as_f64().unwrap() - 0.6).abs() < 1e-5);
}

#[test]
fn held_out_failures_and_budget_stops_save_diagnostics_without_a_body() {
    let dir = setup();
    let budget = run(&dir, &["--iterations", "1", "--out", "budget.json"], 4);
    assert_eq!(budget["result"]["status"], "BUDGET_STOP");
    assert!(budget["result"]["solution"].is_null());
    let mut input = fixture().1;
    let x = input["fit"]["frames"][0]["keypoints"][3]["pixel_px"][0]
        .as_f64()
        .unwrap();
    input["fit"]["frames"][0]["keypoints"][3]["pixel_px"][0] = json!(x + 10.);
    save(&dir, &input);
    let bad = run(&dir, &["--out", "rejected.json"], 2);
    assert_eq!(bad["result"]["status"], "REJECTED");
    assert!(bad["result"]["solution"].is_null());
}

#[test]
fn model_identity_correspondence_and_parameter_typos_refuse_before_output() {
    let dir = setup();
    let base = fixture().1;
    for mutate in [
        |v: &mut Value| v["model"]["model_sha256"] = json!("0".repeat(64)),
        |v: &mut Value| v["model"]["shape_parameters"][0]["name"] = json!("hieght"),
        |v: &mut Value| v["model"]["pose_parameters"][0]["name"] = json!("root_x"),
        |v: &mut Value| v["model"]["keypoints"]["p0"]["index"] = json!(100),
        |v: &mut Value| v["model"]["joints"]["arm"]["axis"] = json!([2, 0, 0]),
        |v: &mut Value| v["model"]["fixed_shape"] = json!({"height":0.5}),
        |v: &mut Value| v["model"]["age_years"] = json!(200),
    ] {
        let mut input = base.clone();
        mutate(&mut input);
        save(&dir, &input);
        run(&dir, &["--out", "bad.json"], 3);
        assert!(!dir.join("bad.json").exists());
    }
    save(&dir, &base);
    run(&dir, &["--iteratoins", "5"], 3);
}

// An independent integer-pixel raster of the synthetic convex torso. This does not use the
// candidate's scanline projection, interval union, residuals or mask sampler to construct evidence.
fn mask_manifest(dir: &Path) -> Vec<Gray> {
    let input = fixture().1;
    mask_manifest_for(dir, &input)
}

fn mask_manifest_for(dir: &Path, input: &Value) -> Vec<Gray> {
    std::fs::create_dir(dir.join("masks")).unwrap();
    let mut frames = Vec::new();
    let mut masks = Vec::new();
    for (i, frame) in input["fit"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let pixels: Vec<Vec2> = frame["keypoints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| serde_json::from_value(p["pixel_px"].clone()).unwrap())
            .collect();
        let faces = [
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
        ];
        let mut mask = Gray::filled(1280, 960, 0);
        let xmin = pixels.iter().map(|p| p.x.floor() as usize).min().unwrap();
        let xmax = pixels.iter().map(|p| p.x.ceil() as usize).max().unwrap();
        let ymin = pixels.iter().map(|p| p.y.floor() as usize).min().unwrap();
        let ymax = pixels.iter().map(|p| p.y.ceil() as usize).max().unwrap();
        for y in ymin..=ymax {
            for x in xmin..=xmax {
                let p = Vec2::new(x as f64, y as f64);
                if faces.iter().any(|t| {
                    let [a, b, c] = t.map(|k| pixels[k]);
                    let area = (b - a).cross(c - a);
                    if area.abs() < 1e-10 {
                        return false;
                    }
                    let signs = [
                        (b - a).cross(p - a),
                        (c - b).cross(p - b),
                        (a - c).cross(p - c),
                    ];
                    signs.iter().all(|v| *v >= 0.) || signs.iter().all(|v| *v <= 0.)
                }) {
                    mask.data[y * mask.w + x] = 255;
                }
            }
        }
        std::fs::write(
            dir.join(format!("masks/mask-{i}.png")),
            encode_png_gray(&mask),
        )
        .unwrap();
        frames.push(
            json!({"frame":frame["frame"],"mask":format!("mask-{i}.png"),"samples":[
                {"axis":"horizontal","index":420,"sigma_px":1.0,"partition":"fit"},
                {"axis":"horizontal","index":480,"sigma_px":1.0,"partition":"held_out"},
                {"axis":"horizontal","index":540,"sigma_px":1.0,"partition":"fit"}
            ]}),
        );
        masks.push(mask);
    }
    let manifest = json!({"schema":"ftx-body-silhouettes/1","evidence":"synthetic posed torso raster; no photographs","frames":frames});
    std::fs::write(
        dir.join("masks/manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    masks
}

#[test]
fn png_silhouettes_fit_the_actual_anny_surface_and_preserve_evidence() {
    let dir = setup();
    let masks = mask_manifest(&dir);
    let r = run(
        &dir,
        &[
            "--silhouettes",
            "masks/manifest.json",
            "--out",
            "silhouette-fit.json",
        ],
        0,
    );
    let report = &r["result"];
    assert_eq!(
        report["input"]["frames"][0]["silhouette"]["lines"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        report["silhouette_input"]["masks"][0]["decoded_mask_sha256"],
        sha256_hex(&masks[0].data)
    );
    let shape = &report["solution"]["shape"];
    assert!((shape[0].as_f64().unwrap() - 0.6).abs() < 0.02);
    assert!((shape[1].as_f64().unwrap() - 0.45).abs() < 0.02);
    assert!(report["attempts"].as_array().unwrap().iter().all(|a| {
        a["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["name"].as_str().unwrap().starts_with("silhouette:"))
            .count()
            == 6
    }));
    let original = std::fs::read(dir.join("masks/mask-0.png")).unwrap();
    run(
        &dir,
        &[
            "--silhouettes",
            "masks/manifest.json",
            "--out",
            "masks/mask-0.png",
        ],
        3,
    );
    assert_eq!(
        std::fs::read(dir.join("masks/mask-0.png")).unwrap(),
        original
    );
    run(
        &dir,
        &[
            "--silhouettes",
            "masks/manifest.json",
            "--out",
            "masks/manifest.json",
        ],
        3,
    );
    run(
        &dir,
        &[
            "--silhouettes",
            "masks/manifest.json",
            "--out",
            "silhouette-fit.json",
        ],
        3,
    );
}

#[test]
fn a_bad_held_out_mask_row_rejects_the_fit_without_changing_its_objective() {
    let dir = setup();
    let mut masks = mask_manifest(&dir);
    let first = run(&dir, &["--silhouettes", "masks/manifest.json"], 0);
    let row = &mut masks[0].data[480 * 1280..481 * 1280];
    let last = row.iter().rposition(|&v| v == 255).unwrap();
    row[last + 1..=last + 10].fill(255);
    std::fs::write(dir.join("masks/mask-0.png"), encode_png_gray(&masks[0])).unwrap();
    let rejected = run(
        &dir,
        &[
            "--silhouettes",
            "masks/manifest.json",
            "--out",
            "silhouette-rejected.json",
        ],
        2,
    );
    assert_eq!(rejected["result"]["status"], "REJECTED");
    assert!(rejected["result"]["solution"].is_null());
    for (a, b) in first["result"]["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .zip(rejected["result"]["attempts"].as_array().unwrap())
    {
        assert_eq!(a["cost_history"], b["cost_history"]);
    }
}

#[test]
fn conflicting_mask_identity_dimensions_and_soft_pixels_are_refused() {
    let dir = setup();
    let mut masks = mask_manifest(&dir);
    let original: Value =
        serde_json::from_slice(&std::fs::read(dir.join("masks/manifest.json")).unwrap()).unwrap();
    for mutate in [
        |v: &mut Value| v["frames"][1]["frame"] = v["frames"][0]["frame"].clone(),
        |v: &mut Value| v["frames"][0]["frame"]["timestamp_us"] = json!(17),
        |v: &mut Value| v["frames"][0]["samples"][0]["partiton"] = json!("fit"),
        |v: &mut Value| v["frames"][0]["samples"][1] = v["frames"][0]["samples"][0].clone(),
    ] {
        let mut v = original.clone();
        mutate(&mut v);
        std::fs::write(
            dir.join("masks/manifest.json"),
            serde_json::to_vec(&v).unwrap(),
        )
        .unwrap();
        run(
            &dir,
            &[
                "--silhouettes",
                "masks/manifest.json",
                "--out",
                "invalid-mask.json",
            ],
            3,
        );
        assert!(!dir.join("invalid-mask.json").exists());
    }
    std::fs::write(
        dir.join("masks/manifest.json"),
        serde_json::to_vec(&original).unwrap(),
    )
    .unwrap();
    masks[0].data[1280 + 1] = 127;
    std::fs::write(dir.join("masks/mask-0.png"), encode_png_gray(&masks[0])).unwrap();
    run(&dir, &["--silhouettes", "masks/manifest.json"], 3);
    std::fs::write(
        dir.join("masks/mask-0.png"),
        encode_png_gray(&Gray::filled(100, 100, 0)),
    )
    .unwrap();
    run(&dir, &["--silhouettes", "masks/manifest.json"], 3);
}

#[test]
fn surface_landmarks_fit_the_posed_triangle_not_a_blended_rest_point() {
    let dir = setup();
    let (bytes, mut input) = fixture();
    let model = AnnyModel::from_data(AnnyData::from_bytes(&bytes, true).unwrap()).unwrap();
    let rest = model
        .rest(&model.params(Phenotype {
            height: 0.6,
            weight: 0.45,
            ..Phenotype::default()
        }))
        .unwrap();
    let poses = [
        [0.0, 0.1, 0.05, 0.04, -0.05, 0.02, -0.4],
        [0.2, -0.1, 0.08, -0.03, 0.06, -0.02, 0.5],
    ];
    input["model"]["keypoints"]["p8"] =
        json!({"kind":"surface","face":2,"barycentric":[0.2,0.3,0.5]});
    for (frame, p) in input["fit"]["frames"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(poses)
    {
        let rv = Vec3::new(p[3], p[4], p[5]);
        let deltas = [
            Affine {
                m: Mat3::rotation(rv.normalized(), rv.norm()),
                t: Vec3::new(p[0], p[1], p[2]),
            },
            Affine {
                m: Mat3::rotation(Vec3::new(1.0, 0.0, 0.0), p[6]),
                t: Vec3::ZERO,
            },
        ];
        let posed = model.pose(&rest, &deltas).unwrap();
        // Vertices carry different bone weights; interpolate AFTER skinning, BEFORE projection.
        let point = posed.vertices[1] * 0.2 + posed.vertices[8] * 0.3 + posed.vertices[9] * 0.5;
        let camera: Camera = serde_json::from_value(frame["camera"].clone()).unwrap();
        frame["keypoints"][8]["pixel_px"] = json!(camera.project(point).unwrap());
    }
    save(&dir, &input);
    let v = run(&dir, &[], 0);
    assert!((v["result"]["solution"]["shape"][0].as_f64().unwrap() - 0.6).abs() < 1e-5);
    assert!(
        (v["result"]["solution"]["poses"][1]["values"][6]
            .as_f64()
            .unwrap()
            - 0.5)
            .abs()
            < 1e-5
    );
    let again = run(&dir, &[], 0);
    assert_eq!(v, again);
    let review = run(&dir, &["--mesh-out", "surface-review"], 0);
    for frame in review["result"]["mesh_review"]["frames"]
        .as_array()
        .unwrap()
    {
        let folder = dir.join("surface-review");
        let obj =
            std::fs::read_to_string(folder.join(frame["mesh"]["file"].as_str().unwrap())).unwrap();
        let vertices: Vec<Vec3> = obj
            .lines()
            .filter_map(|l| l.strip_prefix("v "))
            .map(|l| {
                let p: Vec<f64> = l.split_whitespace().map(|x| x.parse().unwrap()).collect();
                Vec3::new(p[0], p[1], p[2])
            })
            .collect();
        let points: Value = serde_json::from_slice(
            &std::fs::read(folder.join(frame["reprojections"]["file"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let p8 = points["keypoints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "p8")
            .unwrap();
        let expected = vertices[1] * 0.2 + vertices[8] * 0.3 + vertices[9] * 0.5;
        assert_eq!(p8["world_m"], json!(expected));
        let camera: Camera = serde_json::from_value(points["camera"].clone()).unwrap();
        assert_eq!(p8["predicted_px"], json!(camera.project(expected).unwrap()));
    }
}

#[test]
fn surface_aliases_and_invalid_weights_are_not_extra_evidence() {
    let dir = setup();
    let base = fixture().1;
    for target in [
        json!({"kind":"surface","face":99,"barycentric":[0.2,0.3,0.5]}),
        json!({"kind":"surface","face":0,"barycentric":[-0.1,0.6,0.5]}),
        json!({"kind":"surface","face":0,"barycentric":[0.1,0.2,0.5]}),
        json!({"kind":"surface","face":0,"barycentric":[0,0,0]}),
        // p0 already names this corner as a vertex.
        json!({"kind":"surface","face":0,"barycentric":[1,0,0]}),
    ] {
        let mut input = base.clone();
        input["model"]["keypoints"]["p8"] = target;
        save(&dir, &input);
        run(&dir, &["--out", "invalid.json"], 3);
        assert!(!dir.join("invalid.json").exists());
    }
    let mut input = base;
    input["model"]["keypoints"]["p8"] =
        json!({"kind":"surface","face":0,"barycentric":[0,0.25,0.75]});
    input["model"]["keypoints"]["p9"] =
        json!({"kind":"surface","face":1,"barycentric":[0.25,0,0.75]});
    save(&dir, &input);
    run(&dir, &["--out", "alias.json"], 3);
    assert!(!dir.join("alias.json").exists());
}

#[test]
fn accepted_mesh_bundle_replays_each_pose_without_grounding_or_refitting() {
    let dir = setup();
    let result = run(&dir, &["--mesh-out", "meshes"], 0);
    let report = &result["result"];
    let plain = run(&dir, &[], 0);
    let mut without_mesh = report.clone();
    without_mesh.as_object_mut().unwrap().remove("mesh_review");
    assert_eq!(without_mesh, plain["result"]);
    let bundle = dir.join("meshes");
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(bundle.join("fit.json")).unwrap()).unwrap(),
        *report
    );
    let (bytes, _) = fixture();
    let model = AnnyModel::from_data(AnnyData::from_bytes(&bytes, true).unwrap()).unwrap();
    let shape = &report["solution"]["shape"];
    let rest = model
        .rest(&model.params(Phenotype {
            height: shape[0].as_f64().unwrap(),
            weight: shape[1].as_f64().unwrap(),
            ..Phenotype::default()
        }))
        .unwrap();
    let entries = report["mesh_review"]["frames"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for (i, entry) in entries.iter().enumerate() {
        assert_eq!(entry["frame"], report["solution"]["poses"][i]["frame"]);
        let p: Vec<f64> =
            serde_json::from_value(report["solution"]["poses"][i]["values"].clone()).unwrap();
        let rv = Vec3::new(p[3], p[4], p[5]);
        let body = model
            .pose(
                &rest,
                &[
                    Affine {
                        m: Mat3::rotation(rv.normalized(), rv.norm()),
                        t: Vec3::new(p[0], p[1], p[2]),
                    },
                    Affine {
                        m: Mat3::rotation(Vec3::new(1., 0., 0.), p[6]),
                        t: Vec3::ZERO,
                    },
                ],
            )
            .unwrap();
        let raw = std::fs::read(bundle.join(entry["mesh"]["file"].as_str().unwrap())).unwrap();
        assert_eq!(entry["mesh"]["sha256"], sha256_hex(&raw));
        assert_eq!(entry["mesh"]["bytes"], raw.len());
        let text = std::str::from_utf8(&raw).unwrap();
        let vertices: Vec<Vec3> = text
            .lines()
            .filter_map(|l| l.strip_prefix("v "))
            .map(|l| {
                let v: Vec<f64> = l.split_whitespace().map(|s| s.parse().unwrap()).collect();
                Vec3::new(v[0], v[1], v[2])
            })
            .collect();
        assert_eq!(vertices, body.vertices);
        let faces: Vec<[u32; 3]> = text
            .lines()
            .filter_map(|l| l.strip_prefix("f "))
            .map(|l| {
                let v: Vec<u32> = l
                    .split_whitespace()
                    .map(|s| s.parse::<u32>().unwrap() - 1)
                    .collect();
                [v[0], v[1], v[2]]
            })
            .collect();
        assert_eq!(faces, model.data().faces);
    }
    // Bundle paths are not used in numerical results or the deterministic files.
    let again = run(&dir, &["--mesh-out", "again"], 0);
    assert_eq!(again, result);
    for entry in entries {
        let name = entry["mesh"]["file"].as_str().unwrap();
        assert_eq!(
            std::fs::read(bundle.join(name)).unwrap(),
            std::fs::read(dir.join("again").join(name)).unwrap()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&bundle).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(bundle.join("fit.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn failed_body_fit_never_creates_a_mesh_bundle() {
    let dir = setup();
    let calls = run(&dir, &["--max-evaluations", "1", "--mesh-out", "calls"], 4);
    assert_eq!(calls["result"]["model_evaluations"], 1);
    assert!(calls["result"]["solution"].is_null());
    assert!(!dir.join("calls").exists());
    let complete = run(&dir, &[], 0);
    let total = complete["result"]["model_evaluations"].as_u64().unwrap();
    let last = run(
        &dir,
        &[
            "--max-evaluations",
            &(total - 1).to_string(),
            "--mesh-out",
            "last",
        ],
        4,
    );
    assert!(last["result"]["solution"].is_null());
    assert!(
        last["result"]["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["status"] == "CONVERGED")
    );
    assert!(!dir.join("last").exists());
    let exact = run(
        &dir,
        &[
            "--max-evaluations",
            &total.to_string(),
            "--mesh-out",
            "exact",
        ],
        0,
    );
    assert_eq!(exact["result"]["solution"], complete["result"]["solution"]);
    assert_eq!(exact["result"]["model_evaluations"], total);
    assert_eq!(exact["result"]["mesh_review"]["export_pose_evaluations"], 2);
    let budget = run(&dir, &["--iterations", "1", "--mesh-out", "budget"], 4);
    assert!(budget["result"]["solution"].is_null());
    assert!(!dir.join("budget").exists());
    let mut input = fixture().1;
    let x = input["fit"]["frames"][0]["keypoints"][3]["pixel_px"][0]
        .as_f64()
        .unwrap();
    input["fit"]["frames"][0]["keypoints"][3]["pixel_px"][0] = json!(x + 10.0);
    save(&dir, &input);
    let rejected = run(&dir, &["--mesh-out", "rejected"], 2);
    assert!(rejected["result"]["solution"].is_null());
    assert!(!dir.join("rejected").exists());
}

#[test]
fn mesh_output_preflight_preserves_files_directories_and_links() {
    let dir = setup();
    let original = std::fs::read(dir.join("input.json")).unwrap();
    for output in ["input.json", "model.ftxbody", ".", "missing/parent"] {
        run(&dir, &["--mesh-out", output], 3);
    }
    std::fs::create_dir(dir.join("existing")).unwrap();
    std::fs::write(dir.join("existing/sentinel"), b"previous evidence").unwrap();
    run(&dir, &["--mesh-out", "existing"], 3);
    run(&dir, &["--mesh-out", "new", "--out", "report.json"], 3);
    assert!(!dir.join("new").exists());
    assert!(!dir.join("report.json").exists());
    assert_eq!(std::fs::read(dir.join("input.json")).unwrap(), original);
    assert_eq!(
        std::fs::read(dir.join("existing/sentinel")).unwrap(),
        b"previous evidence"
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("existing", dir.join("alias")).unwrap();
        std::os::unix::fs::symlink("not-created", dir.join("dangling")).unwrap();
        for output in ["alias", "dangling"] {
            run(&dir, &["--mesh-out", output], 3);
            assert!(
                std::fs::symlink_metadata(dir.join(output))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        assert!(!dir.join("not-created").exists());
    }
}

#[test]
fn silhouette_fits_export_without_refitting_and_keep_held_out_failures_closed() {
    let dir = setup();
    let mut masks = mask_manifest(&dir);
    let plain = run(&dir, &["--silhouettes", "masks/manifest.json"], 0);
    let exported = run(
        &dir,
        &[
            "--silhouettes",
            "masks/manifest.json",
            "--mesh-out",
            "silhouette-review",
        ],
        0,
    );
    let mut result = exported["result"].clone();
    result.as_object_mut().unwrap().remove("mesh_review");
    assert_eq!(result, plain["result"]);
    let saved: Value =
        serde_json::from_slice(&std::fs::read(dir.join("silhouette-review/fit.json")).unwrap())
            .unwrap();
    assert_eq!(saved, exported["result"]);
    assert_eq!(
        saved["silhouette_input"]["masks"][0]["decoded_mask_sha256"],
        sha256_hex(&masks[0].data)
    );
    for (artifact, frame) in saved["mesh_review"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .zip(saved["input"]["frames"].as_array().unwrap())
    {
        let folder = dir.join("silhouette-review");
        let coordinates: Value = serde_json::from_slice(
            &std::fs::read(folder.join(artifact["reprojections"]["file"].as_str().unwrap()))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            coordinates["silhouette"]["evidence"],
            frame["silhouette"]["evidence"]
        );
        let lines = coordinates["silhouette"]["lines"].as_array().unwrap();
        assert_eq!(
            lines.len(),
            frame["silhouette"]["lines"].as_array().unwrap().len()
        );
        for (line, observation) in lines
            .iter()
            .zip(frame["silhouette"]["lines"].as_array().unwrap())
        {
            assert_eq!(line["observation"], *observation);
            assert_eq!(line["comparison"]["partition"], observation["partition"]);
            assert_eq!(line["comparison"]["passed"], true);
            assert!(line["comparison"]["boundary_error_px"].as_f64().unwrap() <= 3.);
        }
        let svg = std::fs::read_to_string(
            folder.join(artifact["reprojection_svg"]["file"].as_str().unwrap()),
        )
        .unwrap();
        assert_eq!(svg.matches("data-silhouette=").count(), lines.len());
        assert!(svg.contains("data-silhouette=\"held_out\""));
    }
    let row = &mut masks[0].data[480 * 1280..481 * 1280];
    let last = row.iter().rposition(|&v| v == 255).unwrap();
    row[last + 1..=last + 10].fill(255);
    std::fs::write(dir.join("masks/mask-0.png"), encode_png_gray(&masks[0])).unwrap();
    let rejected = run(
        &dir,
        &[
            "--silhouettes",
            "masks/manifest.json",
            "--mesh-out",
            "silhouette-rejected",
        ],
        2,
    );
    assert!(rejected["result"]["solution"].is_null());
    assert!(!dir.join("silhouette-rejected").exists());
    for (a, b) in plain["result"]["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .zip(rejected["result"]["attempts"].as_array().unwrap())
    {
        assert_eq!(a["cost_history"], b["cost_history"]);
    }
}

#[test]
fn mesh_review_retains_observations_and_escapes_untrusted_labels() {
    let dir = setup();
    let mut input = fixture().1;
    let name = "<script>alert('landmark')</script>&\"";
    let stream = "<script>alert('stream')</script>&\"";
    let target = input["model"]["keypoints"]
        .as_object_mut()
        .unwrap()
        .remove("p9")
        .unwrap();
    input["model"]["keypoints"][name] = target;
    for frame in input["fit"]["frames"].as_array_mut().unwrap() {
        frame["frame"]["stream"] = json!(stream);
        frame["keypoints"][9]["name"] = json!(name);
    }
    for start in input["fit"]["starts"].as_array_mut().unwrap() {
        for pose in start["poses"].as_array_mut().unwrap() {
            pose["frame"]["stream"] = json!(stream);
        }
    }
    save(&dir, &input);
    let r = run(&dir, &["--mesh-out", "review"], 0);
    let report = &r["result"];
    let folder = dir.join("review");
    for (entry, frame) in report["mesh_review"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .zip(report["input"]["frames"].as_array().unwrap())
    {
        assert_eq!(entry["frame"], frame["frame"]);
        let raw =
            std::fs::read(folder.join(entry["reprojections"]["file"].as_str().unwrap())).unwrap();
        assert_eq!(entry["reprojections"]["sha256"], sha256_hex(&raw));
        let points: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(points["camera"], frame["camera"]);
        assert_eq!(points["image_size_px"], frame["image_size_px"]);
        assert_eq!(points["rotation_cw_deg"], frame["rotation_cw_deg"]);
        for (p, observation) in points["keypoints"]
            .as_array()
            .unwrap()
            .iter()
            .zip(frame["keypoints"].as_array().unwrap())
        {
            assert_eq!(p["name"], observation["name"]);
            assert_eq!(p["partition"], observation["partition"]);
            assert_eq!(p["observed_px"], observation["pixel_px"]);
            assert_eq!(p["sigma_px"], observation["sigma_px"]);
            let world: Vec3 = serde_json::from_value(p["world_m"].clone()).unwrap();
            let camera: Camera = serde_json::from_value(points["camera"].clone()).unwrap();
            assert_eq!(p["predicted_px"], json!(camera.project(world).unwrap()));
            assert!(p["residual_px"].as_f64().unwrap() < 1e-3);
        }
        let svg = std::fs::read_to_string(
            folder.join(entry["reprojection_svg"]["file"].as_str().unwrap()),
        )
        .unwrap();
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&lt;script&gt;"));
        assert!(svg.contains("held_out"));
        assert_eq!(
            entry["reprojection_svg"]["sha256"],
            sha256_hex(svg.as_bytes())
        );
    }
    let page = std::fs::read_to_string(folder.join("index.html")).unwrap();
    assert!(!page.contains("<script>"));
    assert!(page.contains("&lt;script&gt;"));
    assert!(page.contains("default-src 'none'"));
    assert!(page.contains("No photograph is embedded or loaded"));
    assert_eq!(
        report["mesh_review"]["viewer"]["sha256"],
        sha256_hex(page.as_bytes())
    );
    let input_before = std::fs::read(dir.join("input.json")).unwrap();
    run(&dir, &["--mesh-out", "review"], 3);
    assert_eq!(
        std::fs::read_to_string(folder.join("index.html")).unwrap(),
        page
    );
    assert_eq!(std::fs::read(dir.join("input.json")).unwrap(), input_before);
}

#[test]
fn model_evaluation_limit_is_global_and_cannot_publish_a_partial_fit() {
    let dir = setup();
    let stopped = run(
        &dir,
        &["--max-evaluations", "1", "--out", "evaluation-stop.json"],
        4,
    );
    let v = &stopped["result"];
    assert_eq!(v["status"], "BUDGET_STOP");
    assert_eq!(v["model_evaluations"], 1);
    assert_eq!(v["options"]["max_model_evaluations"], 1);
    assert_eq!(v["attempts"][0]["model_evaluations"], 1);
    assert_eq!(v["attempts"][1]["model_evaluations"], 0);
    assert_eq!(v["attempts"][1]["status"], "BUDGET_STOP");
    assert!(v["solution"].is_null());
    for limit in ["0", "1000001", "-1", "bad"] {
        run(
            &dir,
            &["--max-evaluations", limit, "--out", "invalid-limit.json"],
            3,
        );
        assert!(!dir.join("invalid-limit.json").exists());
    }
    let complete = run(&dir, &[], 0);
    let total = complete["result"]["model_evaluations"].as_u64().unwrap();
    let final_stop = run(&dir, &["--max-evaluations", &(total - 1).to_string()], 4);
    assert!(final_stop["result"]["solution"].is_null());
    assert!(
        final_stop["result"]["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["status"] == "CONVERGED")
    );
    let exact = run(&dir, &["--max-evaluations", &total.to_string()], 0);
    assert_eq!(exact["result"]["solution"], complete["result"]["solution"]);
    assert_eq!(
        exact["result"]["pose_spread_fraction"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
