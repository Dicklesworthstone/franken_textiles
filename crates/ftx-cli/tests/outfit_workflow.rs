//! The real outfit CLI on synthetic completed native drapes; no wearer data or Anny assets.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ftx_core::{Vec3, receipt::sha256_hex};
use ftx_drape::animation::pc2;
use serde_json::{Value, json};

const MEASUREMENTS: &[u8] = br#"{"stature":1000,"head":480}"#;
const BODY: &str = "v -0.1 0 0\nv 0.1 0 0\nv 0 0 1\nf 1 2 3\n";

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn cloth() -> Vec<Vec3> {
    vec![
        Vec3::new(-0.2, -0.03, 0.45),
        Vec3::new(0.2, -0.03, 0.45),
        Vec3::new(0.2, -0.03, 0.8),
        Vec3::new(-0.2, -0.03, 0.8),
    ]
}

fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ftx-outfit-{}-{now}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("measurements.json"), MEASUREMENTS).unwrap();
    for (i, style) in [("jacket", "BOLERO"), ("trousers", "TROUSERS")] {
        let dir = path.join(i);
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("body.obj"), BODY).unwrap();
        let mut points = cloth();
        if i == "trousers" {
            for p in &mut points {
                p.z -= 0.4;
            }
        }
        let mut obj = String::new();
        for p in &points {
            obj.push_str(&format!("v {} {} {} 0.1 0.7 0.1\n", p.x, p.y, p.z));
        }
        obj.push_str("f 1 2 3\nf 1 3 4\n");
        std::fs::write(dir.join("garment.obj"), obj).unwrap();
        // The first frame is deliberately different: outfits must use the final cache frame.
        std::fs::write(dir.join("fold.pc2"), pc2(&[vec![Vec3::ZERO; 4], points])).unwrap();
        save(
            &dir.join("sim_report.json"),
            &json!({"style":style,"arm_deg":35,"settled":true,
            "gating":[],"flags":[],"vertices":4,"faces":2,"animation":{"frames":2},
            "measurements_sha256":sha256_hex(MEASUREMENTS),"bands":{"synthetic":17},
            "self_contact":{"enabled":true,"audit":"passed"}}),
        );
    }
    path
}

fn run(dir: &Path, options: &[&str], expected: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args([
            "outfit",
            "--measurements",
            "measurements.json",
            "--sim",
            "jacket",
            "--sim",
            "trousers",
            "--res",
            "40x48",
            "--json",
        ])
        .args(options)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(expected),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn whole_outfit_is_portable_deterministic_and_uses_final_native_frames() {
    let dir = setup();
    let value = run(&dir, &["--out", "review"], 0);
    let report = &value["result"];
    assert_eq!(report, &read(&dir.join("review/outfit.json")));
    assert_eq!(report["status"], "VISUALIZATION_ONLY");
    assert_eq!(report["inter_garment_contact"], "not_simulated");
    assert_eq!(report["pose"]["arm_deg"], 35.0);
    assert_eq!(report["garments"].as_array().unwrap().len(), 2);
    for g in report["garments"].as_array().unwrap() {
        assert_eq!(g["same_body"], true);
        assert_eq!(g["source"]["cache"]["final_frame"], 1);
        assert_eq!(g["bands"]["synthetic"], 17);
    }
    for view in ["front", "back", "side", "three_quarter"] {
        let name = format!("outfit_{view}.png");
        assert_eq!(report["stills"][view], name);
        let image = ftx_trace::raster::image::load_png(
            &std::fs::read(dir.join("review").join(name)).unwrap(),
        )
        .unwrap();
        assert_eq!((image.w, image.h), (40, 48));
    }
    let html = std::fs::read_to_string(dir.join("review/report.html")).unwrap();
    assert!(html.contains("not resolved"));
    assert!(html.contains("Content-Security-Policy"));
    assert!(!html.contains("http://") && !html.contains("https://"));
    run(&dir, &["--out", "second"], 0);
    for entry in std::fs::read_dir(dir.join("review")).unwrap() {
        let e = entry.unwrap();
        assert_eq!(
            std::fs::read(e.path()).unwrap(),
            std::fs::read(dir.join("second").join(e.file_name())).unwrap()
        );
    }
}

#[test]
fn independent_arm_pose_is_preserved_and_compares_actual_angles_before_rendering() {
    let dir = setup();
    let arms = json!({"left_abduction_deg":20.0,"right_abduction_deg":80.0});
    for (garment, common) in [("jacket", 35.0), ("trousers", 50.0)] {
        let path = dir.join(garment).join("sim_report.json");
        let mut report = read(&path);
        report["arm_pose"] = arms.clone();
        // Explicit pairs override the otherwise different symmetric fallback.
        report["arm_deg"] = json!(common);
        save(&path, &report);
    }
    let report = run(&dir, &["--out", "paired"], 0);
    assert_eq!(report["result"]["pose"]["arm_pose"], arms);
    let html = std::fs::read_to_string(dir.join("paired/report.html")).unwrap();
    assert!(html.contains("Arms: left 20.0 degrees, right 80.0 degrees"));
    for invalid in [
        Value::Null,
        json!({"left_abduction_deg":20.0}),
        json!({"left_abduction_deg":20.0,"right_abduction_deg":181.0}),
        json!({"left_abduction_deg":20.0,"right_abduction_deg":70.0}),
    ] {
        let path = dir.join("trousers/sim_report.json");
        let mut report = read(&path);
        report["arm_pose"] = invalid;
        save(&path, &report);
        run(&dir, &["--out", "invalid-pose"], 3);
        assert!(!dir.join("invalid-pose").exists());
    }
    // An explicit equal pair and the same omitted pair describe the same physical pose.
    let dir = setup();
    let path = dir.join("trousers/sim_report.json");
    let mut report = read(&path);
    report["arm_pose"] = json!({"left_abduction_deg":35.0,"right_abduction_deg":35.0});
    save(&path, &report);
    run(&dir, &["--out", "equal-pair"], 0);
}

#[test]
fn flags_and_unsettled_runs_remain_visible_and_return_two() {
    let dir = setup();
    let file = dir.join("trousers/sim_report.json");
    let mut report = read(&file);
    report["style"] = json!("<script>evil()</script>");
    report["gating"] = json!(["<b>crossing</b>"]);
    report["flags"] = report["gating"].clone();
    report["settled"] = json!(false);
    save(&file, &report);
    let result = run(&dir, &["--out", "review"], 2);
    assert_eq!(result["result"]["status"], "VISUALIZATION_WITH_FLAGS");
    assert_eq!(result["result"]["garments"][1]["gating"], report["gating"]);
    let html = std::fs::read_to_string(dir.join("review/report.html")).unwrap();
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("&lt;b&gt;crossing&lt;/b&gt;"));
    assert!(!html.contains("<script>"));
}

#[test]
fn mismatched_measurements_pose_and_body_refuse_before_output_creation() {
    for mutation in ["hash", "pose", "body"] {
        let dir = setup();
        let file = dir.join("trousers/sim_report.json");
        let mut report = read(&file);
        match mutation {
            "hash" => report["measurements_sha256"] = json!("not the requested wearer"),
            "pose" => report["arm_deg"] = json!(50),
            _ => std::fs::write(dir.join("trousers/body.obj"), BODY.replace("0.1", "0.2")).unwrap(),
        }
        save(&file, &report);
        run(&dir, &["--out", "new/review"], 3);
        assert!(!dir.join("new").exists());
    }
}

#[test]
fn malformed_sim_report_and_non_utf8_mesh_refuse_as_invalid_document() {
    let dir = setup();
    std::fs::write(dir.join("trousers/sim_report.json"), b"{not valid json").unwrap();
    let bad_json = run(&dir, &["--out", "bad-json"], 3);
    assert_eq!(bad_json["refusal"]["code"], "invalid_document");
    assert!(!dir.join("bad-json").exists());

    let dir = setup();
    std::fs::write(dir.join("trousers/garment.obj"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    let bad_utf8 = run(&dir, &["--out", "bad-utf8"], 3);
    assert_eq!(bad_utf8["refusal"]["code"], "invalid_document");
    assert!(!dir.join("bad-utf8").exists());
}

#[test]
fn stale_truncated_and_malformed_point_caches_are_refused() {
    for mutation in ["truncated", "stale", "nan", "count", "extra", "magic"] {
        let dir = setup();
        let file = dir.join("jacket/fold.pc2");
        let mut bytes = std::fs::read(&file).unwrap();
        match mutation {
            "truncated" => {
                bytes.pop();
            }
            "stale" => bytes[80..84].copy_from_slice(&0.7f32.to_le_bytes()),
            "nan" => bytes[80..84].copy_from_slice(&f32::NAN.to_le_bytes()),
            "count" => bytes[28..32].copy_from_slice(&u32::MAX.to_le_bytes()),
            "extra" => bytes.push(0),
            _ => bytes[0] = 0,
        }
        std::fs::write(&file, bytes).unwrap();
        run(&dir, &["--out", "new/review"], 3);
        assert!(!dir.join("new").exists());
    }
}

#[test]
fn existing_output_inputs_and_duplicate_run_aliases_are_not_modified() {
    let dir = setup();
    let original = std::fs::read(dir.join("jacket/garment.obj")).unwrap();
    run(&dir, &["--out", "jacket"], 3);
    assert_eq!(
        std::fs::read(dir.join("jacket/garment.obj")).unwrap(),
        original
    );
    run(&dir, &["--sim", "./jacket", "--out", "nope"], 3);
    assert!(!dir.join("nope").exists());
    run(&dir, &["--out", "review"], 0);
    let before = std::fs::read(dir.join("review/outfit.json")).unwrap();
    run(&dir, &["--out", "review"], 3);
    assert_eq!(
        std::fs::read(dir.join("review/outfit.json")).unwrap(),
        before
    );
}

#[test]
fn invalid_cli_and_geometry_fail_closed_without_output() {
    let dir = setup();
    for options in [
        vec!["--turntable", "true"],
        vec!["--seams", "--seams"],
        vec!["--sim"],
        vec!["--typo", "x"],
    ] {
        let mut full = vec!["--out", "nope"];
        full.extend(options);
        run(&dir, &full, 3);
        assert!(!dir.join("nope").exists());
    }
    std::fs::write(
        dir.join("jacket/garment.obj"),
        "v NaN 0 0\nv 0 1 0\nv 0 0 1\nf 1 2 3\n",
    )
    .unwrap();
    run(&dir, &["--out", "nope"], 3);
    assert!(!dir.join("nope").exists());
}

#[test]
fn seam_mode_preserves_source_colours_and_missing_colours_refuse() {
    let dir = setup();
    run(&dir, &["--out", "plain"], 0);
    let result = run(&dir, &["--out", "seams", "--seams"], 0);
    assert_eq!(result["result"]["colour_mode"], "seams");
    assert_ne!(
        std::fs::read(dir.join("plain/outfit_front.png")).unwrap(),
        std::fs::read(dir.join("seams/outfit_front.png")).unwrap()
    );
    let path = dir.join("jacket/garment.obj");
    let plain = std::fs::read_to_string(&path)
        .unwrap()
        .replace(" 0.1 0.7 0.1", "");
    std::fs::write(path, plain).unwrap();
    run(&dir, &["--out", "missing", "--seams"], 3);
    assert!(!dir.join("missing").exists());
}

fn prepare_hat(dir: &Path) -> Value {
    let measurements = ftx_measure::reference_wearer(1000.0, Some(30.0), Some("F"), None).unwrap();
    save(&dir.join("measurements.json"), &measurements);
    let hash = sha256_hex(&std::fs::read(dir.join("measurements.json")).unwrap());
    for garment in ["jacket", "trousers"] {
        let path = dir.join(garment).join("sim_report.json");
        let mut report = read(&path);
        report["measurements_sha256"] = json!(hash);
        save(&path, &report);
    }
    let hat = ftx_pattern::draft::fez::fez(&measurements, None).unwrap().0;
    save(&dir.join("hat.json"), &hat);
    hat
}

#[test]
fn measured_fez_renders_with_dimensions_and_cannot_clear_source_flags() {
    let dir = setup();
    let hat = prepare_hat(&dir);
    run(&dir, &["--out", "plain"], 0);
    let report = run(&dir, &["--out", "hat-review", "--hat", "hat.json"], 0)["result"].clone();
    assert_eq!(
        report["hat"]["height_mm"],
        hat["meta"]["solved"]["height_mm"]
    );
    assert_eq!(
        report["hat"]["base_girth_mm"],
        hat["meta"]["solved"]["base_girth_mm"]
    );
    assert_eq!(report["hat"]["wall_mm"], 2.0);
    assert_eq!(report["hat"]["floor_z_m"], 0.0);
    assert_eq!(report["garments"].as_array().unwrap().len(), 2);
    let height = report["hat"]["top_mm_above_floor"].as_f64().unwrap()
        - report["hat"]["base_mm_above_floor"].as_f64().unwrap();
    assert!((height - report["hat"]["height_mm"].as_f64().unwrap()).abs() < 1e-10);
    assert_ne!(
        std::fs::read(dir.join("plain/outfit_front.png")).unwrap(),
        std::fs::read(dir.join("hat-review/outfit_front.png")).unwrap()
    );
    let html = std::fs::read_to_string(dir.join("hat-review/report.html")).unwrap();
    assert!(html.contains("Schematic fez") && html.contains("not a draped or fitted hat"));
    let path = dir.join("jacket/sim_report.json");
    let mut flagged = read(&path);
    flagged["gating"] = json!(["synthetic source flag"]);
    save(&path, &flagged);
    let report = run(&dir, &["--out", "flagged-hat", "--hat", "hat.json"], 2);
    assert_eq!(report["result"]["status"], "VISUALIZATION_WITH_FLAGS");
    assert_eq!(report["result"]["garments"][0]["gating"], flagged["gating"]);
}

#[test]
fn hat_placement_preserves_root_relative_source_coordinates() {
    let dir = setup();
    prepare_hat(&dir);
    for garment in ["jacket", "trousers"] {
        let path = dir.join(garment);
        for name in ["body.obj", "garment.obj"] {
            let file = path.join(name);
            let mut m =
                ftx_drape::render::read_obj(&std::fs::read_to_string(&file).unwrap()).unwrap();
            let mut text = String::new();
            for p in &mut m.vertices {
                p.z -= 0.5;
                text.push_str(&format!("v {} {} {}\n", p.x, p.y, p.z));
            }
            for [a, b, c] in m.triangles {
                text.push_str(&format!("f {} {} {}\n", a + 1, b + 1, c + 1));
            }
            std::fs::write(&file, text).unwrap();
            if name == "garment.obj" {
                // The final cache is made from the precise OBJ source, not an already rounded
                // cache: subtracting from f32 samples would introduce a different rounding.
                std::fs::write(
                    path.join("fold.pc2"),
                    pc2(&[vec![Vec3::ZERO; 4], m.vertices]),
                )
                .unwrap();
            }
        }
    }
    let before = std::fs::read(dir.join("jacket/garment.obj")).unwrap();
    let report = run(&dir, &["--out", "review", "--hat", "hat.json"], 0)["result"].clone();
    assert_eq!(report["hat"]["floor_z_m"], -0.5);
    assert_eq!(report["hat"]["head_top_mm"], 1000.0);
    assert!(report["hat"]["base_mm_above_floor"].as_f64().unwrap() > 800.0);
    assert_eq!(
        before,
        std::fs::read(dir.join("jacket/garment.obj")).unwrap()
    );
}

#[test]
fn invalid_hat_patterns_do_not_publish_partial_reviews() {
    let dir = setup();
    let original = prepare_hat(&dir);
    for mutation in ["program", "units", "height", "girth", "missing"] {
        let mut hat = original.clone();
        match mutation {
            "program" => hat["meta"]["program"] = json!("drafting.trousers"),
            "units" => hat["units"] = json!("px"),
            "height" => hat["meta"]["solved"]["height_mm"] = json!(-1),
            "girth" => hat["meta"]["solved"]["top_girth_mm"] = json!(999999),
            _ => hat["meta"]["solved"]["base_girth_mm"] = Value::Null,
        }
        save(&dir.join("hat.json"), &hat);
        run(&dir, &["--out", "nope/review", "--hat", "hat.json"], 3);
        assert!(!dir.join("nope").exists());
    }
}

#[test]
fn hat_recomputes_record_bearing_measurements_ignoring_stale_or_missing_fused_cache() {
    let dir = setup();
    let fresh = ftx_measure::reference_wearer(1000.0, Some(30.0), Some("F"), None).unwrap();
    let hat = ftx_pattern::draft::fez::fez(&fresh, None).unwrap().0;
    save(&dir.join("hat.json"), &hat);

    let rebind = |doc: &Value| {
        save(&dir.join("measurements.json"), doc);
        let hash = sha256_hex(&std::fs::read(dir.join("measurements.json")).unwrap());
        for garment in ["jacket", "trousers"] {
            let path = dir.join(garment).join("sim_report.json");
            let mut report = read(&path);
            report["measurements_sha256"] = json!(hash);
            save(&path, &report);
        }
    };

    rebind(&fresh);
    let expected = run(&dir, &["--out", "fresh", "--hat", "hat.json"], 0)["result"].clone();

    let mut poisoned = fresh.clone();
    poisoned["fused"]["stature"]["value_mm"] = json!(650.0);
    poisoned["fused"]["head"]["value_mm"] = json!(320.0);
    rebind(&poisoned);
    let from_poisoned = run(&dir, &["--out", "poisoned", "--hat", "hat.json"], 0)["result"].clone();
    assert_eq!(from_poisoned["hat"], expected["hat"]);
    assert_ne!(
        from_poisoned["measurements_sha256"],
        expected["measurements_sha256"]
    );
    assert_eq!(
        std::fs::read(dir.join("poisoned/outfit_front.png")).unwrap(),
        std::fs::read(dir.join("fresh/outfit_front.png")).unwrap()
    );

    let mut unfused = fresh;
    unfused.as_object_mut().unwrap().remove("fused");
    rebind(&unfused);
    let from_unfused = run(&dir, &["--out", "unfused", "--hat", "hat.json"], 0)["result"].clone();
    assert_eq!(from_unfused["hat"], expected["hat"]);
}

#[test]
fn outfit_legible_shading_renders_distinct_pngs_and_refuses_unknown_shading() {
    let dir = setup();
    let standard = run(&dir, &["--out", "std_out"], 0)["result"].clone();
    assert_eq!(standard["shading"], "standard");
    let legible = run(&dir, &["--out", "leg_out", "--shading", "legible"], 0)["result"].clone();
    assert_eq!(legible["shading"], "legible");
    assert_ne!(
        std::fs::read(dir.join("std_out/outfit_front.png")).unwrap(),
        std::fs::read(dir.join("leg_out/outfit_front.png")).unwrap(),
        "legible shading must differ from standard shading"
    );
    let html = std::fs::read_to_string(dir.join("leg_out/report.html")).unwrap();
    assert!(html.contains("Shading: legible"));
    run(&dir, &["--out", "bad_out", "--shading", "glossy"], 3);
    assert!(!dir.join("bad_out").exists());
}

#[cfg(unix)]
#[test]
fn outfit_refuses_symlinked_simdir_and_core_simulation_files() {
    use std::os::unix::fs::symlink;

    let dir = setup();
    let alias = dir.join("jacket_alias");
    symlink(dir.join("jacket"), &alias).unwrap();
    for sim_arg in ["jacket_alias", "jacket_alias/"] {
        let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&dir)
            .args([
                "outfit",
                "--measurements",
                "measurements.json",
                "--sim",
                sim_arg,
                "--out",
                "bad_alias",
                "--res",
                "40x48",
                "--json",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3), "{sim_arg}");
        let body: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(body["refusal"]["code"], "invalid_document", "{sim_arg}");
        assert!(!dir.join("bad_alias").exists());
    }

    for (idx, linked) in ["sim_report.json", "body.obj", "garment.obj", "fold.pc2"]
        .into_iter()
        .enumerate()
    {
        let case_dir = dir.join(format!("case_{idx}"));
        std::fs::create_dir(&case_dir).unwrap();
        for name in ["sim_report.json", "body.obj", "garment.obj", "fold.pc2"] {
            if name == linked {
                symlink(dir.join("jacket").join(name), case_dir.join(name)).unwrap();
            } else {
                std::fs::copy(dir.join("jacket").join(name), case_dir.join(name)).unwrap();
            }
        }
        let out_name = format!("bad_case_{idx}");
        let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&dir)
            .args([
                "outfit",
                "--measurements",
                "measurements.json",
                "--sim",
                case_dir.to_str().unwrap(),
                "--out",
                &out_name,
                "--res",
                "40x48",
                "--json",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3), "{linked}");
        let body: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(body["refusal"]["code"], "invalid_document", "{linked}");
        assert!(!dir.join(&out_name).exists());
    }
}
