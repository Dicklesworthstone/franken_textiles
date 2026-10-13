//! Real-binary workflow using a tiny synthetic two-bone .ftxbody, never production model assets.
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

fn synthetic_model() -> Vec<u8> {
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
    let bytes = synthetic_model();
    let model = AnnyModel::from_data(AnnyData::from_bytes(&bytes, true).unwrap()).unwrap();
    let body = model.params(Phenotype {
        height: 0.6,
        weight: 0.45,
        ..Phenotype::default()
    });
    let rest = model.rest(&body).unwrap();
    let poses = [
        [0.0, 0.1, 0.05, 0.04, -0.05, 0.02, -0.4],
        [0.2, -0.1, 0.08, -0.03, 0.06, -0.02, 0.5],
    ];
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
            center_m: Vec3::new(0., -4., 0.75),
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
            "frames":frames,"starts":starts,"measurements":[{"name":"stature","value_m":1.3,"sigma_m":0.002,"evidence":"synthetic independent height","partition":"fit"}]}});
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
