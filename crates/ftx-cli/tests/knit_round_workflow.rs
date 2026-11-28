//! End-to-end Plan §5.1 knitting & yarn-dyeing CLI integration tests (`FTX-K17`).
//!
//! Uses a tiny hermetic synthetic `ftxbody/1` rig (zero real-person PII or external model files)
//! and exercises every §5.1 command, `--json` `ftx-cli/1` envelope, `0`/`2`/`3`/`4` exit codes,
//! `reserve_output` fresh-directory enforcement, `--created` deterministic PDF hashes, and
//! woven `ftx drape` regression parity.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use ftx_body::knit_shell::{
    AnatomicalCut, AnatomicalSection, AnatomicalShellRequest, AnatomicalShellSpec, body_mesh_sha256,
};
use ftx_core::Vec3;
use ftx_core::receipt::sha256_hex;
use ftx_pattern::dye::{DyeMachineProfile, OpticalMeasurementBasis, YarnDyeingQualification};
use ftx_pattern::knit::{KnitSwatchQualification, MachineProfile};
use ftx_pattern::materials::{Evidence, Source};
use serde_json::{Value, json};

fn scratch_dir(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "ftx-knit-round-{label}-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_ftx(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(cwd)
        .args(args)
        .arg("--json")
        .output()
        .unwrap()
}

fn run_json_expect(cwd: &Path, args: &[&str], expected_exit: i32) -> Value {
    let out = run_ftx(cwd, args);
    assert_eq!(
        out.status.code(),
        Some(expected_exit),
        "args={args:?}\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], "ftx-cli/1");
    assert_eq!(v["exit"], expected_exit);
    assert_eq!(v["ok"], expected_exit == 0);
    v
}

/// Minimal hermetic synthetic articulated `ftxbody/1` rig (zero real-person data).
fn synthetic_ftxbody_bytes() -> Vec<u8> {
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
    let mut tails = vec![0.0, 0.0, 0.1];
    for sign in [1.0, -1.0] {
        for j in 0..4 {
            let x = sign * (0.12 + 0.05 * f64::from(j));
            let z = 0.9 - 0.06 * f64::from(j);
            heads.extend([x, 0.0, z]);
            tails.extend([x + sign * 0.05, 0.0, z - 0.06]);
        }
    }
    for sign in [1.0, -1.0] {
        for z in [0.6, 0.3, 0.05] {
            heads.extend([sign * 0.06, 0.0, z]);
            tails.extend([sign * 0.06, 0.0, z - 0.05]);
        }
    }
    heads.extend([0.0, 0.0, 0.9, 0.0, 0.0, 1.0]);
    tails.extend([0.0, 0.0, 1.0, 0.0, 0.0, 1.1]);

    let bone_count = bones.len();
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
                "sha256": sha256_hex(&bytes)
            }),
        );
        data.extend(bytes);
    };
    let f64s = |a: Vec<f64>| a.into_iter().flat_map(f64::to_le_bytes).collect::<Vec<_>>();
    let u32s = |a: Vec<u32>| a.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>();

    let mut vertices = Vec::new();
    let mut influences = Vec::new();
    let mut faces = Vec::new();
    for (r, z) in [0.0, 0.4, 0.8, 1.0].into_iter().enumerate() {
        for i in 0..16 {
            let (s, c) = ftx_core::det::sin_cos(f64::from(i) * std::f64::consts::TAU / 16.0);
            vertices.extend([0.1 * c, 0.1 * s, z]);
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
    vertices.extend([0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    influences.extend([0, 0]);
    for i in 0..16 {
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
    let delta: Vec<f64> = vertices
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|v| [0.0, 0.0, 0.5 * v[2]])
        .collect();
    let blend_vertices: Vec<u32> = (0..n as u32).collect();
    let blend_rows = vec![0, n as u32];

    add("template_vertices", "f64", vec![n, 3], f64s(vertices));
    add("faces", "u32", vec![nfaces, 3], u32s(faces));
    add("blendshape_row_ptr", "u32", vec![2], u32s(blend_rows));
    add("blendshape_vertex", "u32", vec![n], u32s(blend_vertices));
    add("blendshape_delta", "f64", vec![n, 3], f64s(delta));
    let mut mask = vec![0; 26];
    mask[1] = 1;
    add("phenotype_mask", "u8", vec![1, 26], mask);
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
        vec![1, bone_count, 3],
        f64s(vec![0.0; 3 * bone_count]),
    );
    add(
        "bone_tails_blendshapes",
        "f64",
        vec![1, bone_count, 3],
        f64s(vec![0.0; 3 * bone_count]),
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
    let mut components = vec![json!(["height", "low"]), json!(["height", "high"])];
    components.extend((0..24).map(|i| json!(["race", format!("r{i}")])));
    let waist_loop: Vec<usize> = (16..32).collect();
    let header = json!({
        "schema": "ftxbody/1",
        "model": "anny",
        "anny_version": "0.6.1",
        "configuration": "synthetic knit round test",
        "licence": "synthetic",
        "package_sha256": {},
        "counts": {
            "vertices": n,
            "faces": nfaces,
            "bones": bone_count,
            "blendshapes": 1,
            "phenotype_blendshapes": 1,
            "local_changes": 0,
            "influences": 1
        },
        "phenotype": {
            "features": ["height", "race"],
            "components": components,
            "anchors": {"height": [0, 1]},
            "exposed": ["height"],
            "fixed_value": 0.5
        },
        "y_axis": [0, 1, 0],
        "degenerate_rotation": [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
        "labels": {
            "blendshapes": ["height-high"],
            "bones": bones,
            "bone_parents": parents,
            "local_changes": []
        },
        "waist_loop_final": waist_loop,
        "arrays": arrays
    });
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

fn synthetic_body_fit(model_hash: &str) -> Value {
    let parameter = json!([{"name": "height", "lower": 0.0, "upper": 1.0}]);
    let attempt = json!({
        "status": "CONVERGED",
        "flags": [],
        "checks": [{"name": "p0", "unit": "px", "residual": 0.25, "limit": 3.0, "passed": true}]
    });
    json!({
        "schema": "ftx-body-fit/1",
        "status": "ACCEPTED",
        "input_sha256": "b".repeat(64),
        "input": {"starts": [{}, {}], "frames": [{}, {}]},
        "attempts": [attempt, attempt],
        "options": {"max_shape_spread_fraction": 0.01},
        "shape_spread_fraction": [0.0],
        "model": {"model_id": format!("anny/0.6.1/sha256:{model_hash}"), "shape": parameter},
        "anny_configuration": {
            "model_sha256": model_hash,
            "age_years": 2.5,
            "gender": 1.0,
            "fixed_shape": {},
            "shape_parameters": parameter
        },
        "solution": {
            "shape": [0.6],
            "poses": [{"values": [5000, 1000, 1000]}],
            "anny_parameters": {
                "provenance": "MODEL_FIT",
                "phenotype": {
                    "gender": 1.0,
                    "age": ftx_body::age_parameter_for_years(2.5),
                    "height": 0.6,
                    "weight": 0.5,
                    "muscle": 0.5,
                    "proportions": 0.5
                },
                "local_changes": []
            }
        },
        "advisories": ["Synthetic interchange receipt; not a physical fit"],
        "no_claim": "Synthetic test"
    })
}

fn synthetic_anatomy_request() -> AnatomicalShellRequest {
    let mut vertices_m = Vec::new();
    for z in [0.0, 1.0] {
        for (x, y) in [(-0.1, -0.1), (0.1, -0.1), (0.1, 0.1), (-0.1, 0.1)] {
            vertices_m.push([x, y, z]);
        }
    }
    let mut triangles = Vec::new();
    for a in 0..4 {
        let b = (a + 1) % 4;
        triangles.extend([[a, b, b + 4], [a, b + 4, a + 4]]);
    }
    triangles.extend([[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7]]);
    let vertices: Vec<_> = vertices_m.iter().copied().map(Vec3::from).collect();
    let indices: Vec<_> = triangles.iter().map(|t| t.map(|v| v as usize)).collect();
    let expected_body_mesh_sha256 = Some(body_mesh_sha256(&vertices, &indices).unwrap());
    AnatomicalShellRequest {
        vertices_m,
        triangles,
        spec: AnatomicalShellSpec {
            cuts: [("hem", 0.35, true), ("neck", 0.65, false)]
                .map(|(name, z, start)| AnatomicalCut {
                    name: name.into(),
                    is_start: start,
                    keep_positive: start,
                    section: AnatomicalSection::Plane {
                        origin_m: [0.0, 0.0, z],
                        normal: [0.0, 0.0, 1.0],
                    },
                })
                .to_vec(),
            radial_offsets_m: vec![0.0; 8],
            min_clearance_m: 0.0,
            allow_negative_ease: false,
        },
        expected_body_mesh_sha256,
    }
}

fn synthetic_knit_qualification(material_sha256: &str) -> KnitSwatchQualification {
    KnitSwatchQualification {
        id: "synthetic-round-swatch".to_string(),
        material_sha256: material_sha256.to_string(),
        machine_sha256: MachineProfile::kniterate_7e().canonical_sha256().unwrap(),
        yarn_lot_id: "lot-round".to_string(),
        was_washed_and_blocked: true,
        // A coarse authored software-fixture gauge keeps the complete one-stitch
        // surface bounded; the draper must never decimate a finer source document.
        wales_per_100mm: 12.0,
        courses_per_100mm: 8.0,
        thickness_mm: 0.6,
        cam_stitch_setting: 7.0,
        wick_dispersion_mm: None,
        shrinkage_axial_pct: None,
        yarn_intake_drift_eps: Some(0.001),
        yarn_intake_drift_sigma_eta: Some(0.01),
        evidence: Evidence::Estimate,
        source: Source {
            reference: "synthetic:knit_round_workflow".to_string(),
            accessed_on: "2026-11-27".to_string(),
            note: "synthetic test qualification; no physical measurements".to_string(),
        },
        method: "synthetic".to_string(),
        conditioning: BTreeMap::new(),
        allow_estimates: true,
    }
}

#[test]
fn plan_5_1_full_knit_and_dye_round_cli_and_woven_parity() {
    let root = scratch_dir("full-round");
    let knit_selection: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-coupled-knit.json"
    ))
    .unwrap();
    let knit_fabric = json!({"material": knit_selection});
    let material = ftx_pattern::textile_import::resolve_fabric(&knit_fabric)
        .unwrap()
        .unwrap();
    let mat_sha = material.sha256().to_string();
    let fabric_bytes = serde_json::to_vec_pretty(&knit_fabric).unwrap();
    fs::write(root.join("knit_fabric.json"), &fabric_bytes).unwrap();
    let knit_qual = synthetic_knit_qualification(&mat_sha);
    fs::write(
        root.join("knit_qual.json"),
        serde_json::to_vec_pretty(&knit_qual).unwrap(),
    )
    .unwrap();
    let model_bytes = synthetic_ftxbody_bytes();
    let model_sha = sha256_hex(&model_bytes);
    fs::write(root.join("model.ftxbody"), &model_bytes).unwrap();
    fs::write(
        root.join("body_fit.json"),
        serde_json::to_vec_pretty(&synthetic_body_fit(&model_sha)).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("measurements.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": "fabric-measurements/1",
            "wearer": {"age_months": 30, "sex": "F", "stature_mm": 960.0}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("anatomy.json"),
        serde_json::to_vec_pretty(&synthetic_anatomy_request()).unwrap(),
    )
    .unwrap();

    // 1. `ftx knit swatch` + reserve_output check
    let swatch = run_json_expect(
        &root,
        &[
            "knit",
            "swatch",
            "--machine",
            "kniterate-7e",
            "--needles",
            "20",
            "--courses-per-zone",
            "6",
            "--created",
            "2026-11-27T12:00",
            "--out",
            "swatch_out",
        ],
        0,
    );
    assert_eq!(swatch["command"], "knit");
    assert_eq!(swatch["result"]["hardware_execution_authorized"], false);
    assert_eq!(swatch["result"]["physical_knitting_validated"], false);
    // Reusing --out must refuse (exit 3) without overwriting.
    let reused = run_json_expect(
        &root,
        &[
            "knit",
            "swatch",
            "--machine",
            "kniterate-7e",
            "--needles",
            "20",
            "--courses-per-zone",
            "6",
            "--created",
            "2026-11-27T12:00",
            "--out",
            "swatch_out",
        ],
        3,
    );
    assert_eq!(reused["refusal"]["code"], "invalid_document");

    // 2. `ftx knit shape` from accepted Anny body (`--body-fit` + `--region tube`) and from `--input`
    let shape_body = run_json_expect(
        &root,
        &[
            "knit",
            "shape",
            "--body-fit",
            "body_fit.json",
            "--model",
            "model.ftxbody",
            "--region",
            "tube",
            "--age-class",
            "child",
            "--machine",
            "kniterate-7e",
            "--wales",
            "12",
            "--courses",
            "8",
            "--qual",
            "knit_qual.json",
            "--material-sha256",
            &mat_sha,
            "--out",
            "shape_body",
        ],
        0,
    );
    assert!(shape_body["result"]["course_count"].as_u64().unwrap() >= 4);
    assert!(shape_body["result"]["max_abs_girth_residual_mm"].is_number());
    assert!(shape_body["result"]["height_residual_mm"].is_number());
    assert_eq!(
        shape_body["result"]["geometry_method"],
        "intrinsic_metric_harmonic_coarea_mean_normal_span"
    );
    assert_eq!(shape_body["result"]["course_growth_direction"], "upward");
    assert_eq!(shape_body["result"]["surface_topology"], "annular_unplaced");
    assert_eq!(shape_body["result"]["hardware_execution_authorized"], false);
    assert!(root.join("shape_body/pattern.knit.json").is_file());
    assert!(root.join("shape_body/autoknit_report.json").is_file());

    let shape_input = run_json_expect(
        &root,
        &[
            "knit",
            "shape",
            "--input",
            "anatomy.json",
            "--age-class",
            "adult",
            "--machine",
            "kniterate-7e",
            "--qual",
            "knit_qual.json",
            "--material-sha256",
            &mat_sha,
            "--out",
            "shape_input",
        ],
        0,
    );
    assert!(shape_input["result"]["stitches"].as_u64().unwrap() > 100);
    assert_eq!(shape_input["result"]["surface_topology"], "torso_tube");

    // 3. `ftx knit draft` with `--style rib` and `--measurements` / `--body-fit`
    let draft_rib = run_json_expect(
        &root,
        &[
            "knit",
            "draft",
            "--age-class",
            "child",
            "--style",
            "rib",
            "--body-fit",
            "body_fit.json",
            "--model",
            "model.ftxbody",
            "--machine",
            "hand-knit",
            "--chest-mm",
            "320",
            "--neck-mm",
            "200",
            "--yoke-mm",
            "60",
            "--length-mm",
            "80",
            "--created",
            "2026-11-27T12:00",
            "--out",
            "draft_rib",
        ],
        0,
    );
    assert_eq!(draft_rib["result"]["style"], "rib");
    assert!(draft_rib["result"]["body"].is_object());

    // 4. `ftx knit verify --strict-knitout` + invalid knit refusal (exit 3)
    let verify_strict = run_json_expect(
        &root,
        &[
            "knit",
            "verify",
            "shape_body/pattern.knit.json",
            "--machine",
            "kniterate-7e",
            "--qual",
            "knit_qual.json",
            "--material-sha256",
            &mat_sha,
            "--strict-knitout",
        ],
        0,
    );
    assert_eq!(verify_strict["result"]["verified"], true);
    assert_eq!(verify_strict["result"]["strict_knitout"], true);

    fs::write(root.join("bad_knit.json"), b"{not valid json").unwrap();
    let bad_verify = run_json_expect(&root, &["knit", "verify", "bad_knit.json"], 3);
    assert_eq!(bad_verify["refusal"]["code"], "invalid_document");

    // 5. `ftx knit export` with `--target` (knitout, hand-pdf deterministic --created, shima-handoff, and Tier-B direct binary refusal)
    let export_hand_a = run_json_expect(
        &root,
        &[
            "knit",
            "export",
            "shape_body/pattern.knit.json",
            "--target",
            "hand-pdf",
            "--material-sha256",
            &mat_sha,
            "--created",
            "2026-11-27T12:00",
            "--out",
            "export_hand_a",
        ],
        0,
    );
    let _export_hand_b = run_json_expect(
        &root,
        &[
            "knit",
            "export",
            "shape_body/pattern.knit.json",
            "--target",
            "hand-pdf",
            "--material-sha256",
            &mat_sha,
            "--created",
            "2026-11-27T12:00",
            "--out",
            "export_hand_b",
        ],
        0,
    );
    assert_eq!(export_hand_a["result"]["gauge_assumption_disclosed"], true);
    assert_eq!(
        sha256_hex(&fs::read(root.join("export_hand_a/chart.pdf")).unwrap()),
        sha256_hex(&fs::read(root.join("export_hand_b/chart.pdf")).unwrap()),
        "--created must yield bit-identical PDF digests"
    );

    let prop_refusal = run_json_expect(
        &root,
        &[
            "knit",
            "export",
            "shape_body/pattern.knit.json",
            "--target",
            "shima-dat",
            "--material-sha256",
            &mat_sha,
            "--out",
            "never_shima_dat",
        ],
        3,
    );
    assert_eq!(prop_refusal["refusal"]["code"], "opaque_proprietary_target");
    assert!(!root.join("never_shima_dat").exists());

    let sintral_refusal = run_json_expect(
        &root,
        &[
            "knit",
            "export",
            "shape_body/pattern.knit.json",
            "--target",
            "stoll-sintral",
            "--material-sha256",
            &mat_sha,
            "--out",
            "never_stoll_sintral",
        ],
        3,
    );
    assert_eq!(
        sintral_refusal["refusal"]["code"],
        "opaque_proprietary_target"
    );

    // 6. Knit drapes admit an explicit source material before writing output.
    fs::write(
        root.join("legacy_fabric.json"),
        serde_json::to_vec(&json!({
            "areal_g_m2": 200.0, "tensile_N_m": 900.0, "bending_Nm": 0.00003
        }))
        .unwrap(),
    )
    .unwrap();
    let mut no_consent = knit_fabric.clone();
    no_consent["material"]["allow_estimates"] = json!(false);
    fs::write(
        root.join("no_consent_fabric.json"),
        serde_json::to_vec(&no_consent).unwrap(),
    )
    .unwrap();
    let mut no_contact = knit_fabric.clone();
    no_contact["material"]["record"]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("friction_skin");
    fs::write(
        root.join("no_contact_fabric.json"),
        serde_json::to_vec(&no_contact).unwrap(),
    )
    .unwrap();
    for (name, extra, expected_code) in [
        ("no_fabric", Vec::<&str>::new(), "unqualified_process"),
        (
            "legacy_fabric",
            vec!["--fabric", "legacy_fabric.json"],
            "unqualified_process",
        ),
        (
            "wrong_material",
            vec!["--fabric", "knit_fabric.json", "--material-sha256", "wrong"],
            "verification_mismatch",
        ),
        (
            "no_estimate_consent",
            vec!["--fabric", "no_consent_fabric.json"],
            "invalid_document",
        ),
        (
            "no_contact_source",
            vec!["--fabric", "no_contact_fabric.json"],
            "verification_mismatch",
        ),
    ] {
        let mut args = vec![
            "drape",
            "shape_body/pattern.knit.json",
            "--measurements",
            "measurements.json",
            "--body-fit",
            "body_fit.json",
            "--model",
            "model.ftxbody",
            "--out",
            name,
        ];
        args.extend(extra);
        let refused = run_json_expect(&root, &args, 3);
        assert_eq!(refused["refusal"]["code"], expected_code, "{name}");
        assert!(
            !root.join(name).exists(),
            "{name} reserved output before admission"
        );
    }
    // Missing body contact characterization also refuses on an otherwise unbound KnitDoc.
    let mut unbound_doc: Value =
        serde_json::from_slice(&fs::read(root.join("shape_body/pattern.knit.json")).unwrap())
            .unwrap();
    unbound_doc["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("material_sha256");
    fs::write(
        root.join("unbound.knit.json"),
        serde_json::to_vec(&unbound_doc).unwrap(),
    )
    .unwrap();
    let contact_refusal = run_json_expect(
        &root,
        &[
            "drape",
            "unbound.knit.json",
            "--fabric",
            "no_contact_fabric.json",
            "--measurements",
            "measurements.json",
            "--out",
            "uncharacterized_contact",
        ],
        3,
    );
    assert_eq!(contact_refusal["refusal"]["code"], "invalid_document");
    assert!(!root.join("uncharacterized_contact").exists());

    for (name, field, changed, expected_code) in [
        (
            "wrong_q_material",
            "material_sha256",
            json!("wrong"),
            "verification_mismatch",
        ),
        (
            "wrong_q_machine",
            "machine_sha256",
            json!("wrong"),
            "verification_mismatch",
        ),
        (
            "wrong_q_gauge",
            "wales_per_100mm",
            json!(25.0),
            "verification_mismatch",
        ),
        (
            "wrong_q_cam",
            "cam_stitch_setting",
            json!(8.0),
            "verification_mismatch",
        ),
        (
            "wrong_q_lot",
            "yarn_lot_id",
            json!("other-lot"),
            "verification_mismatch",
        ),
        (
            "unwashed_q",
            "was_washed_and_blocked",
            json!(false),
            "unqualified_process",
        ),
    ] {
        let mut changed_qual = serde_json::to_value(&knit_qual).unwrap();
        changed_qual[field] = changed;
        let path = format!("{name}.json");
        fs::write(root.join(&path), serde_json::to_vec(&changed_qual).unwrap()).unwrap();
        let refused = run_json_expect(
            &root,
            &[
                "drape",
                "shape_body/pattern.knit.json",
                "--fabric",
                "knit_fabric.json",
                "--qual",
                &path,
                "--measurements",
                "measurements.json",
                "--out",
                name,
            ],
            3,
        );
        assert_eq!(refused["refusal"]["code"], expected_code, "{name}");
        assert!(
            !root.join(name).exists(),
            "{name} reserved output before binding checks"
        );
    }

    let unplaced = run_json_expect(
        &root,
        &[
            "drape",
            "shape_body/pattern.knit.json",
            "--fabric",
            "knit_fabric.json",
            "--qual",
            "knit_qual.json",
            "--measurements",
            "measurements.json",
            "--out",
            "unplaced_generic_tube",
        ],
        3,
    );
    assert_eq!(unplaced["refusal"]["code"], "unqualified_process");
    assert!(
        unplaced["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("torso placement")
    );
    assert!(!root.join("unplaced_generic_tube").exists());

    // The explicit source hem/neck shell supplies torso semantics. A generic body tube
    // remains exportable but does not silently acquire an anatomical placement.
    let knit_drape = run_ftx(
        &root,
        &[
            "drape",
            "shape_input/pattern.knit.json",
            "--measurements",
            "measurements.json",
            "--body-fit",
            "body_fit.json",
            "--model",
            "model.ftxbody",
            "--fabric",
            "knit_fabric.json",
            "--qual",
            "knit_qual.json",
            "--material-sha256",
            &mat_sha,
            "--sew",
            "2",
            "--drape",
            "6",
            "--audit-patch",
            "selvedge",
            "--out",
            "drape_knit_out",
        ],
    );
    let knit_drape_code = knit_drape.status.code().unwrap();
    assert!(
        knit_drape_code == 0 || knit_drape_code == 2,
        "unexpected knit drape exit {knit_drape_code}: stdout={} stderr={}",
        String::from_utf8_lossy(&knit_drape.stdout),
        String::from_utf8_lossy(&knit_drape.stderr)
    );
    let knit_drape_val: Value = serde_json::from_slice(&knit_drape.stdout).unwrap();
    assert_eq!(knit_drape_val["schema"], "ftx-cli/1");
    assert_eq!(knit_drape_val["result"]["schema"], "ftx-knit/1");
    assert_eq!(
        knit_drape_val["result"]["hardware_execution_authorized"],
        false
    );
    assert_eq!(knit_drape_val["result"]["fit_claimed"], false);
    assert_eq!(knit_drape_val["result"]["material_sha256"], mat_sha);
    assert_eq!(knit_drape_val["result"]["sew_steps_run"], 2);
    assert_eq!(knit_drape_val["result"]["drape_steps_run"], 6);
    assert_eq!(knit_drape_val["result"]["steps_run"], 8);
    assert_eq!(
        knit_drape_val["result"]["material"]["mechanics"]["areal_kg_m2"],
        0.2
    );
    assert_eq!(
        knit_drape_val["result"]["material"]["mechanics"]["thickness_m"],
        0.0006
    );
    assert_eq!(
        knit_drape_val["result"]["material"]["mechanics"]["friction_skin"],
        0.2
    );
    assert_eq!(knit_drape_val["result"]["surface"]["topology"], "tube");
    assert_eq!(
        knit_drape_val["result"]["surface"]["vertices"],
        shape_input["result"]["stitches"]
    );
    assert_eq!(knit_drape_val["result"]["surface"]["boundary_loops"], 2);
    assert_eq!(
        knit_drape_val["result"]["surface"]["course_growth_upward"],
        true
    );
    assert_eq!(
        knit_drape_val["result"]["patch_audit"]["material_sha256"],
        mat_sha
    );
    assert_eq!(
        knit_drape_val["result"]["drape_uncertainty_receipt"]["evidence"],
        "estimate"
    );
    assert_eq!(
        knit_drape_val["result"]["patch_audit"]["pore_open_fraction"],
        Value::Null
    );
    assert!(knit_drape_val["result"]["drape_uncertainty_receipt"]["payload_sha256"].is_string());
    assert!(knit_drape_val["result"]["patch_audit"]["energy_density_j_m2"].is_number());
    assert!(
        root.join("drape_knit_out/drape_uncertainty_receipt.json")
            .is_file()
    );
    assert!(root.join("drape_knit_out/patch_audit.json").is_file());
    assert!(root.join("drape_knit_out/garment.obj").is_file());
    assert!(root.join("drape_knit_out/sim_report.json").is_file());
    let source_surface_bytes = fs::read(root.join("drape_knit_out/knit_surface.json")).unwrap();
    assert_eq!(
        knit_drape_val["result"]["surface"]["sha256"],
        sha256_hex(&source_surface_bytes)
    );
    let source_surface: Value = serde_json::from_slice(&source_surface_bytes).unwrap();
    assert_eq!(
        source_surface["pattern_sha256"],
        shape_input["result"]["pattern_sha256"]
    );
    let source_vertices = source_surface["vertices"].as_array().unwrap();
    for (triangle, source_ids) in source_surface["triangles"]
        .as_array()
        .unwrap()
        .iter()
        .zip(source_surface["triangle_stitch_ids"].as_array().unwrap())
    {
        for corner in 0..3 {
            assert_eq!(
                source_vertices[triangle[corner].as_u64().unwrap() as usize]["stitch_id"],
                source_ids[corner]
            );
        }
    }
    assert_eq!(
        fs::read(root.join("drape_knit_out/fabric.source.json")).unwrap(),
        fabric_bytes
    );
    let frozen_fabric: Value = serde_json::from_slice(
        &fs::read(root.join("drape_knit_out/fabric_override.json")).unwrap(),
    )
    .unwrap();
    let replay_material =
        ftx_pattern::textile_import::resolve_fabric(&frozen_fabric["effective_fabric"])
            .unwrap()
            .unwrap();
    assert_eq!(replay_material.sha256(), mat_sha);
    assert!(
        root.join("drape_knit_out/swatch_qualification.json")
            .is_file()
    );

    // 7. Export the same synthetic gauge/process-bound garment with Kniterate.
    let export_kniterate = run_json_expect(
        &root,
        &[
            "knit",
            "export",
            "shape_body/pattern.knit.json",
            "--target",
            "kniterate",
            "--qual",
            "knit_qual.json",
            "--material-sha256",
            &mat_sha,
            "--created",
            "2026-11-27T12:00",
            "--out",
            "export_kniterate",
        ],
        0,
    );
    assert!(root.join("export_kniterate/program.kc").is_file());
    assert_eq!(
        export_kniterate["result"]["hardware_execution_authorized"],
        false
    );

    // 8. `ftx dye swatch --applicator piezo` and `dip`
    let dye_sw = run_json_expect(
        &root,
        &[
            "dye",
            "swatch",
            "--applicator",
            "piezo",
            "--yarn",
            "cotton-120tex",
            "--out",
            "dye_swatch_piezo",
        ],
        0,
    );
    assert_eq!(dye_sw["result"]["not_for_garment_wear"], true);
    assert_eq!(dye_sw["result"]["machine"], "benchtop-piezo-cmyk-v1");

    // 9. `ftx dye plan` using `<knit.json>`, `--drape`, `--image`, and `--qual`
    let rgb: Vec<u8> = (0..(8 * 8))
        .flat_map(|i| {
            if i < 32 {
                [30u8, 120, 190]
            } else {
                [210u8, 160, 40]
            }
        })
        .collect();
    let png_bytes = ftx_drape::render::png(8, 8, &rgb);
    fs::write(root.join("art.png"), png_bytes).unwrap();

    let piezo = DyeMachineProfile::benchtop_piezo_cmyk();
    let dye_qual = YarnDyeingQualification {
        id: "synthetic-dye-qual".to_string(),
        material_sha256: mat_sha.clone(),
        dye_machine_sha256: piezo.canonical_sha256().unwrap(),
        yarn_lot_id: "lot-round".to_string(),
        dye_lot_id: "dye-round".to_string(),
        wick_dispersion_mm: 1.2,
        shrinkage_axial_pct: 2.0,
        wet_tenacity_retention_ratio: 0.94,
        yarn_breaking_force_n: 9.0,
        yarn_tg_c: None,
        optical_measurement_basis: OpticalMeasurementBasis::CalibratedChartScanner,
        washing_off_validated: true,
        iso_105_c06_grade: Some(4.5),
        iso_105_x12_wet_rub_grade: Some(4.5),
        saliva_fastness_validated: true,
        evidence: Evidence::Measured,
        source: Source {
            reference: "synthetic:dye_round_workflow".to_string(),
            accessed_on: "2026-11-27".to_string(),
            note: "synthetic dye qualification".to_string(),
        },
        method: "steam fix + wash".to_string(),
        conditioning: BTreeMap::new(),
        allow_estimates: false,
    };
    fs::write(
        root.join("dye_qual.json"),
        serde_json::to_vec_pretty(&dye_qual).unwrap(),
    )
    .unwrap();

    let dye_plan = run_json_expect(
        &root,
        &[
            "dye",
            "plan",
            "shape_input/pattern.knit.json",
            "--drape",
            "drape_knit_out/drape_uncertainty_receipt.json",
            "--image",
            "art.png",
            "--qual",
            "dye_qual.json",
            "--applicator",
            "piezo",
            "--max-iters",
            "6",
            "--out",
            "dye_plan_out",
        ],
        0,
    );
    assert_eq!(dye_plan["result"]["hardware_execution_authorized"], false);
    assert_eq!(dye_plan["result"]["physical_dyeing_validated"], false);
    assert!(
        dye_plan["result"]["aspect_ratio_stretch_bound"]
            .as_f64()
            .unwrap()
            >= 1.0
    );
    assert!(root.join("dye_plan_out/dye_program.json").is_file());
    assert!(
        root.join("dye_plan_out/dye_uncertainty_receipt.json")
            .is_file()
    );
    assert!(root.join("dye_plan_out/pareto_regimes.json").is_file());
    assert!(root.join("dye_plan_out/simulated_preview.png").is_file());

    // 10. `ftx dye export` (succeeds with --qual + --receipt, refuses unqualified adult without --qual)
    let dye_export = run_json_expect(
        &root,
        &[
            "dye",
            "export",
            "dye_plan_out/dye_program.json",
            "--qual",
            "dye_qual.json",
            "--receipt",
            "dye_plan_out/dye_uncertainty_receipt.json",
            "--material-sha256",
            &mat_sha,
            "--out",
            "dye_export_out",
        ],
        0,
    );
    assert_eq!(dye_export["result"]["verified"], true);
    assert_eq!(dye_export["result"]["hardware_execution_authorized"], false);
    assert_eq!(dye_export["result"]["physical_dyeing_validated"], false);
    assert_eq!(dye_export["result"]["fit_claimed"], false);
    assert!(root.join("dye_export_out/dye_schedule.csv").is_file());
    assert!(root.join("dye_export_out/dye_controller.gcode").is_file());
    assert!(root.join("dye_export_out/dye_cert.json").is_file());

    let unqual_export = run_json_expect(
        &root,
        &[
            "dye",
            "export",
            "dye_plan_out/dye_program.json",
            "--out",
            "never_dye_export",
        ],
        3,
    );
    assert_eq!(unqual_export["refusal"]["code"], "unqualified_process");
    assert!(!root.join("never_dye_export").exists());

    // 11. Exit code 4 (`BudgetExceeded`) check
    let budget = run_json_expect(
        &root,
        &[
            "dye",
            "optimize",
            "--courses",
            "100000",
            "--wales",
            "100000",
            "--out",
            "never_budget",
        ],
        4,
    );
    assert_eq!(budget["refusal"]["code"], "budget_exceeded");

    // 12. Woven `ftx drape` regression parity on the same synthetic `ftxbody/1`
    let woven: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/materials/synthetic-scenario-pattern.json"
    ))
    .unwrap();
    let mut woven_pat = woven;
    let width = std::f64::consts::TAU * 180.0;
    woven_pat["pieces"][0]["vertices"]["b"][0] = json!(width);
    woven_pat["pieces"][0]["vertices"]["c"][0] = json!(width);
    woven_pat["pieces"][0]["placement"]["radius_mm"] = json!(180.0);
    woven_pat["pieces"][0]["placement"]["x0"] = json!(width / 2.0);
    fs::write(
        root.join("woven.json"),
        serde_json::to_vec_pretty(&woven_pat).unwrap(),
    )
    .unwrap();
    let woven_out = run_ftx(
        &root,
        &[
            "drape",
            "woven.json",
            "--measurements",
            "measurements.json",
            "--body-fit",
            "body_fit.json",
            "--model",
            "model.ftxbody",
            "--spacing-mm",
            "100",
            "--sew",
            "3",
            "--drape",
            "3",
            "--out",
            "woven_drape_out",
        ],
    );
    let woven_code = woven_out.status.code().unwrap();
    assert!(
        woven_code == 0 || woven_code == 2,
        "woven drape regression failed: code={woven_code} stdout={} stderr={}",
        String::from_utf8_lossy(&woven_out.stdout),
        String::from_utf8_lossy(&woven_out.stderr)
    );
    assert!(root.join("woven_drape_out/sim_report.json").is_file());
    assert!(
        !root
            .join("woven_drape_out/drape_uncertainty_receipt.json")
            .exists()
    );
}
