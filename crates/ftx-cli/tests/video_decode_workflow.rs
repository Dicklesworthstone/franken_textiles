//! Actual external decoding into the ordinary native selection workflow. Synthetic local media only.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use ftx_body::{Affine, AnnyData, AnnyModel, Phenotype};
use ftx_core::{Mat3, Vec2, Vec3, receipt::sha256_hex};
use ftx_measure::camera::{Camera, Intrinsics};
use ftx_trace::raster::image::{Gray, encode_png_gray, load_png};
use serde_json::{Value, json};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn scratch_dir(prefix: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "{prefix}-{}-{stamp}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn ftx(dir: &Path, args: &[&str], exit: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{args:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn cli(dir: &Path, command: &str, input: &str, extra: &[&str], exit: i32) -> Value {
    let mut args = vec!["measure", command, input];
    args.extend_from_slice(extra);
    ftx(dir, &args, exit)
}

#[test]
#[ignore = "requires locally installed FFmpeg and ffprobe; run with --include-ignored"]
fn real_variable_rate_decode_preserves_pixels_clocks_and_selection() {
    let root = scratch_dir("ftx-video-decode");
    for (case, epoch, rotation) in [("vfr%03d", 0, 90), ("large-clock", 3_600_000, 270)] {
        let dir = root.join(case);
        std::fs::create_dir(&dir).unwrap();
        let filter = format!("setpts={epoch}+if(eq(N\\,0)\\,0\\,if(eq(N\\,1)\\,17\\,70))");
        let encoded = Command::new("ffmpeg")
            .current_dir(&dir)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-n",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=64x48:rate=1000",
                "-frames:v",
                "3",
                "-vf",
                &filter,
                "-fps_mode",
                "passthrough",
                "-enc_time_base",
                "1:1000",
                "-c:v",
                "ffv1",
                "-threads",
                "1",
                "clip with spaces.mkv",
            ])
            .output()
            .unwrap();
        assert!(
            encoded.status.success(),
            "{}",
            String::from_utf8_lossy(&encoded.stderr)
        );
        let source = std::fs::read(dir.join("clip with spaces.mkv")).unwrap();
        let request = json!({"schema":"ftx-video-decode/1","video":"clip with spaces.mkv",
            "stream":"synthetic-camera","camera_epoch":"fixed-lens","rotation_cw_deg":rotation,
            "focal_px":[800,800],"options":{"window_us":150000,"min_interior_pixels":64,
                "min_person_variance":1,"min_sheet_variance":1,"max_focal_change_ratio":0.01}});
        save(&dir.join("decode-input.json"), &request);
        let result = cli(
            &dir,
            "video-decode",
            "decode-input.json",
            &["--out", "decoded"],
            0,
        )["result"]
            .clone();
        assert_eq!(result["status"], "DECODED_REQUIRES_MASKS");
        assert_eq!(result["frames"].as_array().unwrap().len(), 3);
        assert_eq!(
            result["frames"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["timestamp_us"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [
                epoch as u64 * 1000,
                (epoch as u64 + 17) * 1000,
                (epoch as u64 + 70) * 1000
            ]
        );
        assert_eq!(
            std::fs::read(dir.join("decoded/source.media")).unwrap(),
            source
        );
        assert_eq!(
            std::fs::read(dir.join("clip with spaces.mkv")).unwrap(),
            source
        );
        assert_eq!(
            std::fs::read_dir(dir.join("decoded/masks"))
                .unwrap()
                .count(),
            0
        );
        // Decoding alone cannot silently create usable body/sheet evidence.
        cli(&dir, "video-select", "decoded/frames.json", &[], 3);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(dir.join("decoded/frames.json")).unwrap())
                .unwrap();
        for frame in manifest["frames"].as_array().unwrap() {
            assert_eq!(frame["metadata"]["rotation_cw_deg"], rotation);
            for (field, left) in [("person_mask", true), ("sheet_mask", false)] {
                let mask = Gray {
                    w: 48,
                    h: 64,
                    data: (0..48 * 64)
                        .map(|i| if (i % 48 < 24) == left { 255 } else { 0 })
                        .collect(),
                };
                std::fs::write(
                    dir.join("decoded").join(frame[field].as_str().unwrap()),
                    encode_png_gray(&mask),
                )
                .unwrap();
            }
        }
        let selection = cli(&dir, "video-select", "decoded/frames.json", &[], 0)["result"].clone();
        assert_eq!(selection["status"], "SELECTED");
        let selected = &selection["selected_sources"][0];
        assert_eq!(selected["image_size_px"], json!([48, 64]));
        let image = selected["image"].as_str().unwrap();
        let decoded = result["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["image"] == image)
            .unwrap();
        assert_eq!(selected["image_sha256"], decoded["image_sha256"]);
        assert_eq!(
            selected["decoded_gray_sha256"],
            decoded["decoded_gray_sha256"]
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.join("decoded"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            for file in ["source.media", "decode.json", "frames.json", image] {
                assert_eq!(
                    std::fs::metadata(dir.join("decoded").join(file))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
        }
        let before = std::fs::read(dir.join("decoded/decode.json")).unwrap();
        cli(
            &dir,
            "video-decode",
            "decode-input.json",
            &["--out", "decoded"],
            3,
        );
        assert_eq!(
            std::fs::read(dir.join("decoded/decode.json")).unwrap(),
            before
        );
        let mut limited = request.clone();
        limited["max_frames"] = json!(2);
        save(&dir.join("limited.json"), &limited);
        cli(
            &dir,
            "video-decode",
            "limited.json",
            &["--out", "limited"],
            4,
        );
        assert!(!dir.join("limited/frames.json").exists());
        assert!(!dir.join("limited/decode.json").exists());
        assert_eq!(
            std::fs::read_dir(dir.join("limited/frames"))
                .unwrap()
                .count(),
            0
        );
        let mut invalid = request.clone();
        invalid["rotation_cw_deg"] = json!(45);
        save(&dir.join("invalid.json"), &invalid);
        cli(
            &dir,
            "video-decode",
            "invalid.json",
            &["--out", "invalid"],
            3,
        );
        assert!(!dir.join("invalid").exists());
    }
}

fn operator_model_bytes(scale: f64, symmetric_weight_fold: bool) -> Vec<u8> {
    // Closed 16-gon articulated prism scaled into toddler stature range (845 mm at height=0.60).
    // Synthetic analytical geometry only; never a substitute for real-wearer anatomical qualification.
    let bones = vec![
        "root",
        "upperarm01.R",
        "upperarm02.R",
        "lowerarm01.R",
        "wrist.R",
        "upperarm01.L",
        "upperarm02.L",
        "lowerarm01.L",
        "wrist.L",
        "upperleg01.R",
        "lowerleg01.R",
        "foot.R",
        "upperleg01.L",
        "lowerleg01.L",
        "foot.L",
        "neck01",
        "head",
    ];
    let parents = vec![-1, 0, 1, 2, 3, 0, 5, 6, 7, 0, 9, 10, 0, 12, 13, 0, 15];
    let mut heads = vec![0.0, 0.0, 0.0];
    let mut tails = vec![0.0, 0.0, 0.1 * scale];
    for sign in [1.0, -1.0] {
        for j in 0..4 {
            let x = sign * (0.12 + 0.05 * f64::from(j)) * scale;
            let z = (0.9 - 0.06 * f64::from(j)) * scale;
            heads.extend([x, 0.0, z]);
            tails.extend([x + sign * 0.05 * scale, 0.0, z - 0.06 * scale]);
        }
    }
    for sign in [1.0, -1.0] {
        for z in [0.6, 0.3, 0.05] {
            heads.extend([sign * 0.06 * scale, 0.0, z * scale]);
            tails.extend([sign * 0.06 * scale, 0.0, (z - 0.05) * scale]);
        }
    }
    heads.extend([0.0, 0.0, 0.9 * scale, 0.0, 0.0, 1.0 * scale]);
    tails.extend([0.0, 0.0, 1.0 * scale, 0.0, 0.0, 1.1 * scale]);
    let bone_count = bones.len();

    let mut vertices = Vec::new();
    let mut influences = Vec::new();
    for (r, z) in [0.0, 0.4, 0.8, 1.0].into_iter().enumerate() {
        for i in 0..16 {
            let (s, c) = ftx_core::det::sin_cos(f64::from(i) * std::f64::consts::TAU / 16.0);
            vertices.extend([0.1 * scale * c, 0.1 * scale * s, z * scale]);
            influences.push(if r == 3 && [15, 0, 1].contains(&i) {
                1
            } else if r == 3 && [7, 8, 9].contains(&i) {
                5
            } else if r == 0 && c > 0.5 {
                9
            } else if r == 0 && c < -0.5 {
                12
            } else {
                0
            });
        }
    }
    vertices.extend([0.0, 0.0, 0.0, 0.0, 0.0, 1.0 * scale]);
    influences.extend([0, 0]);
    let mut faces = Vec::new();
    for i in 0..16_u32 {
        let j = (i + 1) % 16;
        faces.extend([64, j, i, 65, 48 + i, 48 + j]);
        for r in 0..3 {
            let a = 16 * r + i;
            let b = 16 * r + j;
            faces.extend([a, b, b + 16, a, b + 16, a + 16]);
        }
    }
    let n = vertices.len() / 3;
    let nfaces = faces.len() / 3;
    let height_delta: Vec<f64> = vertices
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|v| [0.0, 0.0, 0.5 * v[2]])
        .collect();
    let weight_delta: Vec<f64> = vertices
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|v| [0.4 * v[0], 0.4 * v[1], 0.0])
        .collect();
    let bone_head_deltas: Vec<f64> = heads
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|b| [0.0, 0.0, 0.5 * b[2]])
        .chain(
            heads
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|b| [0.4 * b[0], 0.4 * b[1], 0.0]),
        )
        .collect();
    let bone_tail_deltas: Vec<f64> = tails
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|b| [0.0, 0.0, 0.5 * b[2]])
        .chain(
            tails
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|b| [0.4 * b[0], 0.4 * b[1], 0.0]),
        )
        .collect();

    let f64s = |a: Vec<f64>| a.into_iter().flat_map(f64::to_le_bytes).collect::<Vec<_>>();
    let u32s = |a: Vec<u32>| a.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();
    let mut arrays = BTreeMap::new();
    let mut data = Vec::new();
    let mut add = |name: &str, dtype: &str, shape: Vec<usize>, bytes: Vec<u8>| {
        arrays.insert(
            name.to_string(),
            json!({
                "dtype": dtype,
                "shape": shape,
                "offset": data.len(),
                "nbytes": bytes.len(),
                "sha256": sha256_hex(&bytes),
            }),
        );
        data.extend(bytes);
    };
    add("template_vertices", "f64", vec![n, 3], f64s(vertices));
    add("faces", "u32", vec![nfaces, 3], u32s(faces));
    let (nblend, components, weight_anchors, blend_labels) = if symmetric_weight_fold {
        // Two separate blendshape rows (weight-low and weight-high) apply the identical radial
        // delta while the midpoint anchor is zero, so weight=0.275 and weight=0.725 produce
        // identical 3D geometry (blend sum 0.45) with full local rank (+-2.0).
        add(
            "blendshape_row_ptr",
            "u32",
            vec![4],
            u32s(vec![0, n as u32, (2 * n) as u32, (3 * n) as u32]),
        );
        add(
            "blendshape_vertex",
            "u32",
            vec![3 * n],
            u32s(
                (0..n as u32)
                    .chain(0..n as u32)
                    .chain(0..n as u32)
                    .collect(),
            ),
        );
        add(
            "blendshape_delta",
            "f64",
            vec![3 * n, 3],
            f64s([height_delta, weight_delta.clone(), weight_delta].concat()),
        );
        let mut mask = vec![0_u8; 78];
        mask[1] = 1;
        mask[26 + 2] = 1;
        mask[52 + 4] = 1;
        add("phenotype_mask", "u8", vec![3, 26], mask);
        let mut comps = vec![
            json!(["height", "low"]),
            json!(["height", "high"]),
            json!(["weight", "low"]),
            json!(["weight", "mid"]),
            json!(["weight", "high"]),
        ];
        comps.extend((0..21).map(|i| json!(["race", format!("r{i}")])));
        (
            3_usize,
            comps,
            json!([0.0, 0.5, 1.0]),
            json!(["height-high", "weight-low", "weight-high"]),
        )
    } else {
        add(
            "blendshape_row_ptr",
            "u32",
            vec![3],
            u32s(vec![0, n as u32, (2 * n) as u32]),
        );
        add(
            "blendshape_vertex",
            "u32",
            vec![2 * n],
            u32s((0..n as u32).chain(0..n as u32).collect()),
        );
        add(
            "blendshape_delta",
            "f64",
            vec![2 * n, 3],
            f64s([height_delta, weight_delta].concat()),
        );
        let mut mask = vec![0_u8; 52];
        mask[1] = 1;
        mask[26 + 3] = 1;
        add("phenotype_mask", "u8", vec![2, 26], mask);
        let mut comps = vec![
            json!(["height", "low"]),
            json!(["height", "high"]),
            json!(["weight", "low"]),
            json!(["weight", "high"]),
        ];
        comps.extend((0..22).map(|i| json!(["race", format!("r{i}")])));
        (
            2_usize,
            comps,
            json!([0, 1]),
            json!(["height-high", "weight-high"]),
        )
    };
    let mut bone_head_all = bone_head_deltas;
    let mut bone_tail_all = bone_tail_deltas;
    if symmetric_weight_fold {
        bone_head_all.extend_from_within(bone_count * 3..);
        bone_tail_all.extend_from_within(bone_count * 3..);
    }
    add(
        "template_bone_heads",
        "f64",
        vec![bone_count, 3],
        f64s(heads),
    );
    add(
        "template_bone_tails",
        "f64",
        vec![bone_count, 3],
        f64s(tails),
    );
    add(
        "bone_heads_blendshapes",
        "f64",
        vec![nblend, bone_count, 3],
        f64s(bone_head_all),
    );
    add(
        "bone_tails_blendshapes",
        "f64",
        vec![nblend, bone_count, 3],
        f64s(bone_tail_all),
    );
    add(
        "bone_rolls",
        "f64",
        vec![bone_count, 3, 3],
        f64s([1., 0., 0., 0., 1., 0., 0., 0., 1.].repeat(bone_count)),
    );
    add("vertex_bone_indices", "u32", vec![n, 1], u32s(influences));
    add("vertex_bone_weights", "f64", vec![n, 1], f64s(vec![1.0; n]));
    add(
        "base_mesh_vertex_indices",
        "u32",
        vec![n],
        u32s((0..n as u32).collect()),
    );
    let waist_loop: Vec<usize> = (16..32).collect();
    let header = json!({
        "schema": "ftxbody/1",
        "model": "anny",
        "anny_version": "0.6.1",
        "configuration": "synthetic-operator-capture-test",
        "licence": "synthetic numerical test",
        "package_sha256": {},
        "counts": {
            "vertices": n,
            "faces": nfaces,
            "bones": bone_count,
            "blendshapes": nblend,
            "phenotype_blendshapes": nblend,
            "local_changes": 0,
            "influences": 1,
        },
        "phenotype": {
            "features": ["height", "weight", "race"],
            "components": components,
            "anchors": {"height": [0, 1], "weight": weight_anchors},
            "exposed": ["height", "weight"],
            "fixed_value": 0.5,
        },
        "y_axis": [0, 1, 0],
        "degenerate_rotation": [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
        "labels": {
            "blendshapes": blend_labels,
            "bones": bones,
            "bone_parents": parents,
            "local_changes": [],
        },
        "waist_loop_final": waist_loop,
        "arrays": arrays,
    });
    let mut h = serde_json::to_vec(&header).unwrap();
    while !h.len().is_multiple_of(8) {
        h.push(b' ');
    }
    let mut out = b"FTXBODY\0".to_vec();
    out.extend(1_u32.to_le_bytes());
    out.extend(0_u32.to_le_bytes());
    out.extend((h.len() as u64).to_le_bytes());
    out.extend(h);
    out.extend(data);
    out
}

struct OperatorFixture {
    camera: Camera,
    focal_px: [f64; 2],
    frame_pngs: Vec<(u64, Vec<u8>)>,
    sharp_timestamps: [u64; 2],
    true_poses: [[f64; 7]; 2],
    observed_indices: [usize; 16],
    keypoints_by_window: [Value; 2],
    true_stature_m: f64,
}

fn prepare_operator_sources(dir: &Path) -> OperatorFixture {
    let scale = 0.62;
    let model_bytes = operator_model_bytes(scale, false);
    std::fs::write(dir.join("model.ftxbody"), &model_bytes).unwrap();
    std::fs::write(
        dir.join("ambiguous.ftxbody"),
        operator_model_bytes(scale, true),
    )
    .unwrap();

    // Step 1: Printable ChArUco reference sheet via the real CLI.
    ftx(
        dir,
        &["measure", "calib-sheet", "--paper", "a4", "--out", "sheet"],
        0,
    );
    let page = load_png(&std::fs::read(dir.join("sheet/calib-sheet-a4.png")).unwrap()).unwrap();
    let sidecar = read_json(&dir.join("sheet/calib-sheet-a4.json"));

    // Step 2: Camera intrinsics calibration via the real CLI.
    let cal_input: Value = serde_json::from_slice(include_bytes!(
        "../../../docs/examples/camera-calibration.json"
    ))
    .unwrap();
    save(&dir.join("calibration-input.json"), &cal_input);
    let calibration = cli(
        dir,
        "calibrate-camera",
        "calibration-input.json",
        &["--out", "accepted-calibration.json"],
        0,
    )["result"]
        .clone();
    assert_eq!(calibration["status"], "ACCEPTED");
    let intrinsics: Intrinsics =
        serde_json::from_value(calibration["solution"]["intrinsics"].clone()).unwrap();
    assert_eq!(intrinsics.distortion.radial, [0.0; 3]);
    assert_eq!(intrinsics.distortion.tangential, [0.0; 2]);
    let focal_px = [intrinsics.focal_px.x, intrinsics.focal_px.y];
    let camera = Camera {
        intrinsics,
        world_to_camera: Mat3::from_rows([[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]),
        center_m: Vec3::new(0.0, -1.0, 0.4375),
    };

    save(
        &dir.join("stationary-board.json"),
        &json!({
            "schema": "ftx-charuco-sheet-reference/1",
            "sheet": "sheet/calib-sheet-a4.json",
            "held_out_corner_ids": [0, 6, 35, 41],
            "board_bottom_left_xz_m": [0.15, 0.35],
            "stationary_upright_evidence": "Synthetic upright A4 ChArUco board fixed at y=0 in authored pinhole frame."
        }),
    );

    // Render 4 1280x960 frames across 2 selector windows ([0, 33_000] and [200_000, 240_000] us):
    // each window has a low-variance blurred decoy (rejected by video-select) and a sharp frame
    // with independent ChArUco board projection on the right half.
    let w = 1280_usize;
    let h = 960_usize;
    let origin = Vec2::new(0.15, 0.35);
    let b = &sidecar["board"];
    let width_m = b["size_mm"][0].as_f64().unwrap() / 1000.0;
    let height_m = b["size_mm"][1].as_f64().unwrap() / 1000.0;
    let mut board_half = vec![255_u8; (w / 2) * h];
    for y in 0..h {
        for x in w / 2..w {
            let z = camera.center_m.z
                - (y as f64 - camera.intrinsics.principal_px.y) * -camera.center_m.y
                    / camera.intrinsics.focal_px.y;
            let world_x = camera.center_m.x
                + (x as f64 - camera.intrinsics.principal_px.x) * -camera.center_m.y
                    / camera.intrinsics.focal_px.x;
            let lx = world_x - origin.x;
            let ly = origin.y + height_m - z;
            if (0.0..width_m).contains(&lx) && (0.0..height_m).contains(&ly) {
                let px = (b["origin_px"][0].as_f64().unwrap()
                    + lx / width_m * b["size_px"][0].as_f64().unwrap())
                .floor() as usize;
                let py = (b["origin_px"][1].as_f64().unwrap()
                    + ly / height_m * b["size_px"][1].as_f64().unwrap())
                .floor() as usize;
                board_half[y * (w / 2) + (x - w / 2)] = page.data[(py * page.w + px) * 3];
            }
        }
    }
    let sharp_timestamps = [33_000_u64, 240_000_u64];
    let specs = [
        (0_u64, false, 0_u8),
        (sharp_timestamps[0], true, 1_u8),
        (200_000_u64, false, 2_u8),
        (sharp_timestamps[1], true, 3_u8),
    ];
    let mut frame_pngs = Vec::with_capacity(specs.len());
    for (ts, sharp_person, tag) in specs {
        let mut data = vec![128_u8; w * h];
        for y in 0..h {
            for x in 0..w / 2 {
                data[y * w + x] = if sharp_person {
                    if (x + y).is_multiple_of(2) { 65 } else { 195 }
                } else {
                    128
                };
            }
            for x in w / 2..w {
                data[y * w + x] = board_half[y * (w / 2) + (x - w / 2)];
            }
        }
        data[0] = tag;
        frame_pngs.push((ts, encode_png_gray(&Gray { w, h, data })));
    }

    // Independent analytical 2D keypoints for the two moving poses on the synthetic toddler model.
    let model = AnnyModel::from_data(AnnyData::from_bytes(&model_bytes, true).unwrap()).unwrap();
    let rest = model
        .rest(&model.params(Phenotype {
            gender: 1.0,
            age: ftx_body::age_parameter_for_years(2.5),
            height: 0.60,
            weight: 0.45,
            ..Phenotype::default()
        }))
        .unwrap();
    let true_poses = [
        [-0.34, 0.04, 0.02, 0.03, -0.04, 0.08, -0.35],
        [-0.24, -0.05, 0.05, -0.03, 0.05, -0.09, 0.40],
    ];
    let observed_indices = [
        0_usize, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 49, 56, 63,
    ];
    let keypoints_by_window = std::array::from_fn(|f| {
        let p = &true_poses[f];
        let rv = Vec3::new(p[3], p[4], p[5]);
        let mut deltas = vec![Affine::IDENTITY; model.data().header.counts.bones];
        deltas[0] = Affine {
            m: Mat3::rotation(rv.normalized(), rv.norm()),
            t: Vec3::new(p[0], p[1], p[2]),
        };
        deltas[1] = Affine {
            m: Mat3::rotation(Vec3::new(1.0, 0.0, 0.0), p[6]),
            t: Vec3::ZERO,
        };
        let posed = model.pose(&rest, &deltas).unwrap();
        json!(
            observed_indices
                .iter()
                .map(|&idx| {
                    let px = camera.project(posed.vertices[idx]).unwrap();
                    assert!(
                        px.x > 40.0 && px.x < 620.0 && px.y > 20.0 && px.y < 940.0,
                        "keypoint p{idx} outside left half: {px:?}"
                    );
                    json!({
                        "name": format!("p{idx}"),
                        "pixel_px": px,
                        "sigma_px": 1.0,
                        "partition": if idx == 24 || idx == 49 { "held_out" } else { "fit" },
                    })
                })
                .collect::<Vec<_>>()
        )
    });

    OperatorFixture {
        camera,
        focal_px,
        frame_pngs,
        sharp_timestamps,
        true_poses,
        observed_indices,
        keypoints_by_window,
        true_stature_m: scale * 1.30,
    }
}

fn run_operator_pipeline(dir: &Path, raw_manifest: &str, fx: &OperatorFixture) {
    // Raw unmasked manifest must never be selectable directly.
    cli(dir, "video-select", raw_manifest, &[], 3);

    // Step 4-5: video-mask-review -> author polygons -> video-masks.
    cli(
        dir,
        "video-mask-review",
        raw_manifest,
        &["--out", "mask-review"],
        0,
    );
    let mut annotations = read_json(&dir.join("mask-review/annotations.json"));
    assert_eq!(annotations["schema"], "ftx-video-mask-polygons/1");
    assert_eq!(annotations["frames"].as_array().unwrap().len(), 4);
    for frame in annotations["frames"].as_array_mut().unwrap() {
        frame["person_regions"] =
            json!([{"outer": [[0, 0], [639, 0], [639, 959], [0, 959]], "holes": []}]);
        frame["sheet_regions"] =
            json!([{"outer": [[640, 0], [1279, 0], [1279, 959], [640, 959]], "holes": []}]);
    }
    save(&dir.join("mask-authored.json"), &annotations);
    cli(
        dir,
        "video-masks",
        "mask-authored.json",
        &["--out", "masked"],
        0,
    );

    // Step 6: video-select rejects the two blurred decoys and selects the two sharp frames.
    let selected = cli(
        dir,
        "video-select",
        "masked/frames.json",
        &["--out", "selection.json"],
        0,
    )["result"]
        .clone();
    assert_eq!(selected["status"], "SELECTED");
    assert_eq!(
        selected["selected_frames"],
        json!([
            {"stream": "operator-camera", "timestamp_us": fx.sharp_timestamps[0]},
            {"stream": "operator-camera", "timestamp_us": fx.sharp_timestamps[1]},
        ])
    );

    // Step 7-8: capture-init-review -> author capture choices -> capture-init (native ChArUco).
    cli(
        dir,
        "capture-init-review",
        "masked/frames.json",
        &[
            "--model",
            "model.ftxbody",
            "--calibration",
            "accepted-calibration.json",
            "--reference",
            "stationary-board.json",
            "--reference-id",
            "studio-board",
            "--out",
            "capture-setup",
        ],
        0,
    );
    let setup_doc = read_json(&dir.join("capture-setup/setup.json"));
    let eligible_indices: Vec<u64> = setup_doc["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            assert_eq!(f["eligible"], true, "candidate ineligible: {f}");
            f["frame_index"].as_u64().unwrap()
        })
        .collect();
    assert_eq!(eligible_indices, vec![1, 3]);

    let mut choices = read_json(&dir.join("capture-setup/capture-choices.json"));
    choices["frame_indices"] = json!(eligible_indices);
    choices["age_years"] = json!(2.5);
    choices["gender"] = json!(1.0);
    choices["shapes"] = json!([
        {"name": "height", "choice": {"mode": "fit", "lower": 0.0, "upper": 1.0}},
        {"name": "weight", "choice": {"mode": "fit", "lower": 0.0, "upper": 1.0}},
    ]);
    for root in choices["roots"].as_array_mut().unwrap() {
        assert!(!root["name"].as_str().unwrap().is_empty());
        root["choice"] = json!({"mode": "fit", "lower": -1.0, "upper": 1.0});
    }
    choices["joints"] = json!([{
        "bone": "upperarm01.R",
        "axis": [1.0, 0.0, 0.0],
        "choice": {"mode": "fit", "lower": -1.5, "upper": 1.5},
    }]);
    choices["metric_scale_source"] =
        json!("stationary A4 ChArUco board calibrated via ftx measure calibrate-camera");
    choices["observation_evidence"] =
        json!("reviewed synthetic 2D vertex correspondences with held-out CHECK partition");
    choices["measurements"] = json!([{
        "name": "stature",
        "value_m": fx.true_stature_m,
        "sigma_m": 0.002,
        "evidence": "independent stature tape observation",
        "partition": "fit",
    }]);
    save(&dir.join("capture-choices.json"), &choices);
    cli(
        dir,
        "capture-init",
        "capture-choices.json",
        &["--model", "model.ftxbody", "--out", "capture-input"],
        0,
    );

    // Step 9-10: body-target-review -> body-targets.
    cli(
        dir,
        "body-target-review",
        "capture-input/input.json",
        &["--model", "model.ftxbody", "--out", "target-review"],
        0,
    );
    let mut targets = read_json(&dir.join("target-review/body-targets.json"));
    let target_map: BTreeMap<_, _> = fx
        .observed_indices
        .iter()
        .map(|&idx| (format!("p{idx}"), json!({"kind": "vertex", "index": idx})))
        .collect();
    targets["keypoints"] = json!(target_map);
    save(&dir.join("targets-authored.json"), &targets);
    cli(
        dir,
        "body-targets",
        "targets-authored.json",
        &["--model", "model.ftxbody", "--out", "target-authored"],
        0,
    );

    // Step 11: capture-review acquires ChArUco camera poses and emits prepared-input.json.
    let reviewed = cli(
        dir,
        "capture-review",
        "target-authored/input.json",
        &["--out", "capture-review"],
        0,
    )["result"]
        .clone();
    for frame in reviewed["capture_input"]["frames"].as_array().unwrap() {
        assert_eq!(
            frame["sheet_acquisition"]["detection"]["detector"]["backend"],
            "rust_charuco"
        );
        assert_eq!(frame["sheet_pose"]["ok"], true);
        assert_eq!(frame["sheet_pose"]["held_out_points"], 4);
    }
    assert!(dir.join("capture-review/prepared-input.json").is_file());

    let mut observed = read_json(&dir.join("capture-review/capture.json"));
    for (w_idx, frame) in observed["fit"]["frames"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        assert_eq!(
            frame["source"]["metadata"]["frame"]["timestamp_us"].as_u64(),
            Some(fx.sharp_timestamps[w_idx])
        );
        frame["keypoints"] = fx.keypoints_by_window[w_idx].clone();
    }
    save(&dir.join("observations.json"), &observed);

    // Step 12-13: fit-start-review -> fit-starts.
    cli(
        dir,
        "fit-start-review",
        "observations.json",
        &["--model", "model.ftxbody", "--out", "start-review"],
        0,
    );
    let mut starts = read_json(&dir.join("start-review/body-starts.json"));
    starts["starts"] = json!(
        [0.03_f64, -0.025_f64]
            .into_iter()
            .map(|shift| {
                json!({
                    "shape": [0.60 + shift, 0.45 - shift],
                    "poses": fx.true_poses.iter().enumerate().map(|(frame_index, p)| {
                        json!({
                            "frame_index": frame_index,
                            "values": p.iter().map(|v| v + shift).collect::<Vec<_>>(),
                        })
                    }).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>()
    );
    save(&dir.join("starts-authored.json"), &starts);
    cli(
        dir,
        "fit-starts",
        "starts-authored.json",
        &["--model", "model.ftxbody", "--out", "start-authored"],
        0,
    );

    // Negative control 1 (stale lens): mutating accepted-calibration.json after fit-starts must
    // refuse before optimization and publish nothing.
    let cal_path = dir.join("accepted-calibration.json");
    let cal_original = std::fs::read(&cal_path).unwrap();
    let mut cal_stale = cal_original.clone();
    cal_stale.push(b'\n');
    std::fs::write(&cal_path, cal_stale).unwrap();
    cli(
        dir,
        "fit-body",
        "start-authored/input.json",
        &["--model", "model.ftxbody", "--mesh-out", "stale-lens-fit"],
        3,
    );
    assert!(!dir.join("stale-lens-fit").exists());
    std::fs::write(&cal_path, &cal_original).unwrap();

    // Negative control 2 (weak parallax / missing metric scale): frontal-plane y=0 keypoints only,
    // pure translational poses, and no tape scale produce an exact projective depth-scale nullspace
    // -> status == UNOBSERVABLE (exit 2, solution: null).
    let model_unamb = AnnyModel::from_data(
        AnnyData::from_bytes(&std::fs::read(dir.join("model.ftxbody")).unwrap(), true).unwrap(),
    )
    .unwrap();
    let rest_unamb = model_unamb
        .rest(&model_unamb.params(Phenotype {
            gender: 1.0,
            age: ftx_body::age_parameter_for_years(2.5),
            height: 0.60,
            weight: 0.45,
            ..Phenotype::default()
        }))
        .unwrap();
    let planar_indices = [0_usize, 8, 16, 24, 32, 40, 48, 56];
    let planar_poses = [[-0.34_f64, 0.0, 0.02], [-0.24_f64, 0.0, 0.05]];
    let planar_frames: Vec<_> = planar_poses
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut deltas = vec![Affine::IDENTITY; model_unamb.data().header.counts.bones];
            deltas[0].t = Vec3::new(p[0], p[1], p[2]);
            let posed = model_unamb.pose(&rest_unamb, &deltas).unwrap();
            let kps: Vec<_> = planar_indices
                .iter()
                .map(|&idx| {
                    json!({
                        "name": format!("p{idx}"),
                        "pixel_px": fx.camera.project(posed.vertices[idx]).unwrap(),
                        "sigma_px": 1.0,
                        "partition": if idx == 56 { "held_out" } else { "fit" },
                    })
                })
                .collect();
            json!({
                "frame": {"stream": "operator-camera", "timestamp_us": fx.sharp_timestamps[i]},
                "camera": fx.camera,
                "image_size_px": [1280, 960],
                "rotation_cw_deg": 0,
                "keypoints": kps,
            })
        })
        .collect();
    let planar_targets: BTreeMap<_, _> = planar_indices
        .iter()
        .map(|&idx| (format!("p{idx}"), json!({"kind": "vertex", "index": idx})))
        .collect();
    let unobservable_req = json!({
        "schema": "ftx-anny-body-fit-input/1",
        "model": {
            "model_sha256": sha256_hex(&std::fs::read(dir.join("model.ftxbody")).unwrap()),
            "age_years": 2.5,
            "gender": 1.0,
            "shape_parameters": [
                {"name": "height", "lower": 0.0, "upper": 1.0},
                {"name": "weight", "lower": 0.0, "upper": 1.0},
            ],
            "pose_parameters": [
                {"name": "root_x_m", "lower": -1.0, "upper": 1.0},
                {"name": "root_y_m", "lower": -0.5, "upper": 0.5},
                {"name": "root_z_m", "lower": -1.0, "upper": 1.0},
            ],
            "keypoints": planar_targets,
            "measurements": [],
        },
        "fit": {
            "schema": "ftx-body-fit-input/1",
            "metric_scale_source": "deliberate weak-parallax projective scale nullspace test",
            "observation_evidence": "coplanar y=0 points without depth parallax or tape scale",
            "frames": planar_frames,
            "measurements": [],
            "starts": ([0.02_f64, -0.02_f64].into_iter().map(|s| json!({
                "shape": [0.60 + s, 0.45 - s],
                "poses": [
                    {"frame": {"stream": "operator-camera", "timestamp_us": fx.sharp_timestamps[0]}, "values": [-0.34 + s, s, 0.02 + s]},
                    {"frame": {"stream": "operator-camera", "timestamp_us": fx.sharp_timestamps[1]}, "values": [-0.24 + s, -s, 0.05 + s]},
                ],
            })).collect::<Vec<_>>()),
        },
    });
    save(&dir.join("weak-parallax.json"), &unobservable_req);
    let weak_res = cli(
        dir,
        "fit-body",
        "weak-parallax.json",
        &["--model", "model.ftxbody", "--out", "weak-report.json"],
        2,
    )["result"]
        .clone();
    assert_eq!(weak_res["status"], "UNOBSERVABLE", "{weak_res}");
    assert!(weak_res["solution"].is_null());
    ftx(
        dir,
        &[
            "body",
            "--body-fit",
            "weak-report.json",
            "--model",
            "model.ftxbody",
            "--out",
            "weak-body",
        ],
        3,
    );
    assert!(!dir.join("weak-body").exists());

    // Negative control 3 (ambiguous shape): symmetric 3-anchor weight fold gives two converged
    // distinct minima (0.275 and 0.725) -> status == AMBIGUOUS (exit 2, solution: null).
    let mut ambig_req: Value = serde_json::from_slice(
        &std::fs::read(dir.join("capture-review/prepared-input.json")).unwrap(),
    )
    .unwrap();
    let ambig_hash = sha256_hex(&std::fs::read(dir.join("ambiguous.ftxbody")).unwrap());
    ambig_req["model"]["model_sha256"] = json!(ambig_hash);
    for (w_idx, frame) in ambig_req["fit"]["frames"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        frame["keypoints"] = fx.keypoints_by_window[w_idx].clone();
    }
    ambig_req["fit"]["starts"] = json!([
        {
            "shape": [0.58, 0.20],
            "poses": [
                {"frame": {"stream": "operator-camera", "timestamp_us": fx.sharp_timestamps[0]}, "values": fx.true_poses[0]},
                {"frame": {"stream": "operator-camera", "timestamp_us": fx.sharp_timestamps[1]}, "values": fx.true_poses[1]},
            ],
        },
        {
            "shape": [0.62, 0.80],
            "poses": [
                {"frame": {"stream": "operator-camera", "timestamp_us": fx.sharp_timestamps[0]}, "values": fx.true_poses[0]},
                {"frame": {"stream": "operator-camera", "timestamp_us": fx.sharp_timestamps[1]}, "values": fx.true_poses[1]},
            ],
        },
    ]);
    save(&dir.join("ambiguous-input.json"), &ambig_req);
    let ambig_res = cli(
        dir,
        "fit-body",
        "ambiguous-input.json",
        &[
            "--model",
            "ambiguous.ftxbody",
            "--out",
            "ambiguous-report.json",
        ],
        2,
    )["result"]
        .clone();
    assert_eq!(ambig_res["status"], "AMBIGUOUS", "{ambig_res}");
    assert!(ambig_res["solution"].is_null());
    for attempt in ambig_res["attempts"].as_array().unwrap() {
        assert_eq!(attempt["status"], "CONVERGED");
    }
    assert!(ambig_res["shape_spread_fraction"][1].as_f64().unwrap() > 0.40);
    ftx(
        dir,
        &[
            "body",
            "--body-fit",
            "ambiguous-report.json",
            "--model",
            "ambiguous.ftxbody",
            "--out",
            "ambiguous-body",
        ],
        3,
    );
    assert!(!dir.join("ambiguous-body").exists());

    // Step 14: Accepted multi-frame body fit on the complete path-bound operator input.
    let accepted = cli(
        dir,
        "fit-body",
        "start-authored/input.json",
        &["--model", "model.ftxbody", "--mesh-out", "accepted-fit"],
        0,
    )["result"]
        .clone();
    assert_eq!(accepted["status"], "ACCEPTED", "{accepted}");
    assert_eq!(
        accepted["solution"]["anny_parameters"]["provenance"],
        "MODEL_FIT"
    );
    assert!((accepted["solution"]["shape"][0].as_f64().unwrap() - 0.60).abs() < 1e-3);
    assert!((accepted["solution"]["shape"][1].as_f64().unwrap() - 0.45).abs() < 2e-3);
    assert!(
        (accepted["solution"]["predicted_measurements_m"][0]
            .as_f64()
            .unwrap()
            - fx.true_stature_m)
            .abs()
            < 0.002
    );

    // Step 15: Inspect reused accepted body in drape pose (verifying Wave 8 pose_geometry).
    let body_doc = ftx(
        dir,
        &[
            "body",
            "--body-fit",
            "accepted-fit/fit.json",
            "--model",
            "model.ftxbody",
            "--pose",
            "drape",
            "--out",
            "body-check",
        ],
        0,
    )["result"]["body"]
        .clone();
    assert_eq!(body_doc["shape_source"]["refitted"], false);
    assert_eq!(
        body_doc["pose_geometry"]["clothing_collider"]["admissible_for_drape"],
        true
    );
    assert_eq!(
        body_doc["pose_geometry"]["clothing_collider"]["self_intersections"]["intersecting_pairs"],
        0
    );
    let waist_mm = body_doc["readings_mm"]["waist"].as_f64().unwrap();

    // Step 16: Execute garment round + exports + report on the accepted MODEL_FIT body, verifying
    // that readiness stays NOT_READY (MEASUREMENTS_NOT_READY) and never auto-promotes to READY.
    let mut pattern: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let band_width = (waist_mm * 0.96).round();
    pattern["pieces"][0]["role"] = json!("bodice_front");
    pattern["pieces"][0]["vertices"]["b"][0] = json!(band_width);
    pattern["pieces"][0]["vertices"]["c"][0] = json!(band_width);
    pattern["pieces"][0]["placement"]["radius_mm"] = json!(waist_mm / std::f64::consts::TAU + 5.0);
    pattern["pieces"][0]["placement"]["x0"] = json!(band_width / 2.0);
    save(&dir.join("pattern.json"), &pattern);
    save(
        &dir.join("fit.json"),
        &json!({"schema": "fabric-fit/1", "points": []}),
    );
    let stature_mm = fx.true_stature_m * 1000.0;
    save(
        &dir.join("model-fit-measurements.json"),
        &json!({
            "schema": "fabric-measurements/1",
            "wearer": {
                "label": "synthetic operator capture check",
                "age_months": 30,
                "sex": "F",
                "stature_mm": stature_mm,
                "stature_provenance": "MODEL_FIT",
                "reference": false,
            },
            "taken_on": "2026-10-20",
            "records": [
                {
                    "name": "stature",
                    "value_mm": stature_mm,
                    "sigma_mm": 3.0,
                    "provenance": "MODEL_FIT",
                    "method": "accepted multi-frame synthetic Anny fit",
                    "source": "accepted-fit/fit.json",
                },
                {
                    "name": "waist",
                    "value_mm": waist_mm,
                    "sigma_mm": 5.0,
                    "provenance": "MODEL_FIT",
                    "method": "accepted multi-frame synthetic Anny fit",
                    "source": "accepted-fit/fit.json",
                },
            ],
            "fused": {},
        }),
    );
    let round = ftx(
        dir,
        &[
            "round",
            "garment-ws",
            "--pattern",
            "pattern.json",
            "--fit",
            "fit.json",
            "--measurements",
            "model-fit-measurements.json",
            "--sim",
            "--body-fit",
            "accepted-fit/fit.json",
            "--model",
            "model.ftxbody",
            "--as-of",
            "2026-10-24",
            "--export",
        ],
        2,
    )["result"]
        .clone();
    assert_eq!(round["status"], "MEASUREMENTS_NOT_READY", "{round}");
    assert_eq!(round["readiness"]["status"], "NOT_READY");
    assert_eq!(round["basis"]["measurements"], "NOT_READY");
    assert_eq!(round["basis"]["flat"], "CONVERGED");
    assert_eq!(round["exports"]["pass"], true);
    assert_eq!(round["sim"]["gating"], json!([]));
    assert_eq!(round["sim"]["body"]["provenance"], "MODEL_FIT");
    assert_eq!(round["sim"]["body"]["shape_source"]["refitted"], false);
    assert_eq!(
        round["sim"]["body_contact_proxy"]["lbs_volume_loss_advisory"],
        false
    );
    assert!(
        round["sim"]["body_contact_proxy"]["posed_volume_m3"]
            .as_f64()
            .unwrap()
            > 0.0
    );

    let report_res = ftx(dir, &["report", "garment-ws"], 0)["result"].clone();
    assert!(
        report_res["report"]
            .as_str()
            .unwrap()
            .ends_with("report.html")
    );
    let html = std::fs::read_to_string(dir.join("garment-ws/report.html")).unwrap();
    assert!(html.contains("Body used for draping — MODEL_FIT"));
    assert!(html.contains("Measurement readiness: NOT_READY"));
    assert!(!html.contains("Measurement readiness: READY</h3>"));
}

#[test]
fn operator_capture_protocol_runs_masks_selection_init_targets_starts_negatives_and_round() {
    let dir = scratch_dir("ftx-operator-capture");
    let fx = prepare_operator_sources(&dir);
    std::fs::create_dir(dir.join("raw")).unwrap();
    let mut frames = Vec::new();
    for (i, (ts, png_bytes)) in fx.frame_pngs.iter().enumerate() {
        let name = format!("frame-{i:04}.png");
        std::fs::write(dir.join("raw").join(&name), png_bytes).unwrap();
        frames.push(json!({
            "metadata": {
                "frame": {"stream": "operator-camera", "timestamp_us": ts},
                "rotation_cw_deg": 0,
                "camera_epoch": "fixed-studio-lens",
                "focal_px": fx.focal_px,
            },
            "image": name,
            "person_mask": format!("unauthored-person-{i:04}.png"),
            "sheet_mask": format!("unauthored-sheet-{i:04}.png"),
        }));
    }
    save(
        &dir.join("raw/frames.json"),
        &json!({
            "schema": "ftx-video-frames/1",
            "decode_receipt": "Synthetic unmasked 4-frame manifest (2 windows x {blurred decoy, sharp ChArUco+person}).",
            "options": {
                "window_us": 150_000,
                "min_interior_pixels": 64,
                "min_person_variance": 5.0,
                "min_sheet_variance": 5.0,
                "max_focal_change_ratio": 0.01,
            },
            "frames": frames,
        }),
    );
    run_operator_pipeline(&dir, "raw/frames.json", &fx);
}

#[test]
#[ignore = "requires locally installed FFmpeg and ffprobe; run with --include-ignored"]
fn real_ffmpeg_clip_drives_full_operator_capture_to_round_pipeline() {
    let dir = scratch_dir("ftx-operator-ffmpeg");
    let fx = prepare_operator_sources(&dir);
    for (i, (_, png_bytes)) in fx.frame_pngs.iter().enumerate() {
        std::fs::write(dir.join(format!("encode-{i:03}.png")), png_bytes).unwrap();
    }
    let filter = format!(
        "setpts=if(eq(N\\,0)\\,0\\,if(eq(N\\,1)\\,{}\\,if(eq(N\\,2)\\,200\\,{})))",
        fx.sharp_timestamps[0] / 1000,
        fx.sharp_timestamps[1] / 1000
    );
    let encoded = Command::new("ffmpeg")
        .current_dir(&dir)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-n",
            "-framerate",
            "1000",
            "-i",
            "encode-%03d.png",
            "-frames:v",
            "4",
            "-vf",
            &filter,
            "-fps_mode",
            "passthrough",
            "-enc_time_base",
            "1:1000",
            "-pix_fmt",
            "gray",
            "-c:v",
            "ffv1",
            "-threads",
            "1",
            "operator-clip.mkv",
        ])
        .output()
        .unwrap();
    assert!(
        encoded.status.success(),
        "{}",
        String::from_utf8_lossy(&encoded.stderr)
    );
    save(
        &dir.join("decode-request.json"),
        &json!({
            "schema": "ftx-video-decode/1",
            "video": "operator-clip.mkv",
            "stream": "operator-camera",
            "camera_epoch": "fixed-studio-lens",
            "rotation_cw_deg": 0,
            "focal_px": fx.focal_px,
            "options": {
                "window_us": 150_000,
                "min_interior_pixels": 64,
                "min_person_variance": 5.0,
                "min_sheet_variance": 5.0,
                "max_focal_change_ratio": 0.01,
            },
        }),
    );
    let decoded = cli(
        &dir,
        "video-decode",
        "decode-request.json",
        &["--out", "decoded"],
        0,
    )["result"]
        .clone();
    assert_eq!(decoded["status"], "DECODED_REQUIRES_MASKS");
    assert_eq!(decoded["frames"].as_array().unwrap().len(), 4);
    run_operator_pipeline(&dir, "decoded/frames.json", &fx);
}
