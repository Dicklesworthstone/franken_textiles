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
