//! Exact synthetic fold-up samples; no Anny asset or wearer data is needed.
use ftx_core::Vec3;
use ftx_drape::animation::pc2;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn setup() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ftx-sim-points-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    std::fs::create_dir(dir.join("sim")).unwrap();
    let measurements = ftx_measure::reference_wearer(920.0, Some(30.0), Some("F"), None).unwrap();
    let waist = measurements["fused"]["waist_height"]["value_mm"]
        .as_f64()
        .unwrap();
    save(&dir.join("m.json"), &measurements);
    let piece = |id: &str, part: &str, qty| json!({"id":id,"vertices":{"A":[0,0],"B":[100,0],"C":[100,100],"D":[0,100]},"edges":[{"id":"bottom","from":"A","to":"B"},{"id":"right","from":"B","to":"C"},{"id":"top","from":"C","to":"D"},{"id":"left","from":"D","to":"A"}],"cut":{"qty":qty},"placement":{"part":part,"y0_landmark":"waist","y0":0},"sim_scale":[1,0.5]});
    save(
        &dir.join("p.json"),
        &json!({"schema":"fabric-pattern/1","units":"mm","style":"SYNTHETIC","pieces":[piece("front","torso",2),piece("sleeve","arm",1)],"seams":[]}),
    );
    save(
        &dir.join("sim/mesh_map.json"),
        &json!({"vertices":12,"units":"m","inst_range":{"front.R":[0,4],"front.L":[4,8],"sleeve":[8,12]},"corners":{"front.R:C":2,"front.L:C":6,"front.R:A":0,"sleeve:A":8},"frames":{"fold":3,"zip":2,"sew":0,"drape":15}}),
    );
    let frames: Vec<_> = (0..20)
        .map(|frame| {
            (0..12)
                .map(|i| {
                    let y = if i % 4 >= 2 { 100.0 } else { 0.0 };
                    let rise = if frame < 5 {
                        900.0
                    } else {
                        30.0 + (frame - 5) as f64
                    };
                    Vec3::new(i as f64 * 0.01, -0.1, -0.5 + (waist + y + rise) / 1000.0)
                })
                .collect()
        })
        .collect();
    std::fs::write(dir.join("sim/fold.pc2"), pc2(&frames)).unwrap();
    // Root-relative body; an unused lower vertex is NOT its displayed floor.
    std::fs::write(
        dir.join("sim/body.obj"),
        "v 0 0 -0.5\nv 0.1 0 -0.5\nv 0 0 0.42\nv 0 0 -99\nf 1 2 3\n",
    )
    .unwrap();
    dir
}

fn run(dir: &Path, extra: &[&str], code: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args([
            "sim-points",
            "sim",
            "--pattern",
            "p.json",
            "--measurements",
            "m.json",
            "--json",
        ])
        .args(extra)
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
fn tail_mean_mirror_sorting_floor_and_unscaled_drafted_levels() {
    let dir = setup();
    let r = run(&dir, &[], 0)["result"].clone();
    assert_eq!(r["frames_averaged"], 15);
    assert_eq!(r["floor_z_m"], -0.5);
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3); // exclude sleeves
    assert_eq!(rows[0]["point"], "front.L:C");
    assert_eq!(rows[1]["point"], "front.R:A");
    assert_eq!(rows[2]["point"], "front.R:C");
    for row in rows {
        assert_eq!(row["rise"], 37.0, "{r}");
    }
    assert_eq!(run(&dir, &[], 0)["result"], r);
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args([
            "sim-points",
            "sim",
            "--pattern",
            "p.json",
            "--measurements",
            "m.json",
        ])
        .output()
        .unwrap();
    let table = String::from_utf8(out.stdout).unwrap();
    assert!(table.contains("drafted z") && table.contains("37.0"));
}

#[test]
fn mismatched_dimensions_and_cross_instance_corners_are_refused() {
    let dir = setup();
    let path = dir.join("sim/mesh_map.json");
    let original: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for edit in 0..5 {
        let mut map = original.clone();
        match edit {
            0 => map["vertices"] = json!(11),
            1 => map["units"] = json!("mm"),
            2 => map["corners"]["front.R:A"] = json!(8),
            3 => map["frames"]["drape"] = json!(16),
            _ => map["corners"]["front.R:A"] = json!(4294967296_u64),
        }
        save(&path, &map);
        assert_eq!(run(&dir, &[], 3)["refusal"]["code"], "invalid_document");
    }
}

#[test]
fn malformed_caches_and_bad_options_have_structured_refusals() {
    let dir = setup();
    run(&dir, &["--typo", "value"], 3);
    run(&dir, &["unexpected"], 3);
    let path = dir.join("sim/fold.pc2");
    let original = std::fs::read(&path).unwrap();
    let mut bytes = original.clone();
    bytes[32..36].copy_from_slice(&f32::NAN.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(run(&dir, &[], 3)["refusal"]["code"], "non_finite");
    std::fs::write(&path, &original[..original.len() - 1]).unwrap();
    run(&dir, &[], 3);
}

#[test]
fn short_caches_use_every_available_frame_not_zero_padding() {
    let dir = setup();
    let mut map: Value =
        serde_json::from_slice(&std::fs::read(dir.join("sim/mesh_map.json")).unwrap()).unwrap();
    map["frames"] = json!({"fold":0,"zip":0,"sew":0,"drape":2});
    save(&dir.join("sim/mesh_map.json"), &map);
    std::fs::write(
        dir.join("sim/fold.pc2"),
        pc2(&[vec![Vec3::ZERO; 12], vec![Vec3::new(0.0, 0.0, 0.1); 12]]),
    )
    .unwrap();
    let r = run(&dir, &[], 0)["result"].clone();
    assert_eq!(r["frames_averaged"], 2);
    assert_eq!(r["rows"][0]["final_z"], 550.0);
}

#[test]
fn viewer_rebuilds_fold_viewer_html_from_saved_drape_directory() {
    let dir = setup();
    save(
        &dir.join("sim/sim_report.json"),
        &json!({
            "settled": true,
            "gating": [],
            "advisory": ["minor shift"],
            "arm_deg": 35.0
        }),
    );
    let mut garment_obj = String::new();
    for i in 0..12 {
        garment_obj.push_str(&format!("v {} -0.1 0.1 0.55 0.06 0.06\n", i as f64 * 0.01));
    }
    garment_obj.push_str("f 1 2 3\nf 5 6 7\nf 9 10 11\n");
    std::fs::write(dir.join("sim/garment.obj"), &garment_obj).unwrap();
    std::fs::write(dir.join("sim/garment_strain.obj"), &garment_obj).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args(["viewer", "sim", "--pattern", "p.json", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(env["result"]["vertices"], 12);
    assert_eq!(env["result"]["source_frames"], 20);
    let html = std::fs::read_to_string(dir.join("sim/fold_viewer.html")).unwrap();
    assert!(html.contains("\"schema\":\"fabric-fold-viewer/1\""));
    assert!(html.contains("<title>SYNTHETIC Fold-up</title>"));
    assert!(html.contains("data:text/javascript;base64,"));
    assert!(html.contains("Copyright © 2010-2023 three.js authors"));
    assert!(!html.contains("https://cdn.jsdelivr.net/"));
    println!(
        "viewer fixture: {}",
        dir.join("sim/fold_viewer.html").display()
    );

    // A contact-protected historical cache has no panel-station marker. Its first
    // four display frames must stay on the board. The retained solver suffix is f32.
    save(
        &dir.join("sim/sim_report.json"),
        &json!({"self_contact":{"enabled":true}}),
    );
    let out_custom = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args(["viewer", "sim", "--out", "custom/viewer.html", "--json"])
        .output()
        .unwrap();
    assert_eq!(out_custom.status.code(), Some(0));
    let html = std::fs::read_to_string(dir.join("custom/viewer.html")).unwrap();
    let (_, data) = html.split_once("\nconst D = ").unwrap();
    let (data, _) = data.split_once(";\nconst $ = ").unwrap();
    let view: Value = serde_json::from_str(data).unwrap();
    assert_eq!(view["contact_replay"]["first_physical"], 4);
    assert_eq!(view["phase"][3], "Flat pattern board");
    assert_eq!(view["phase"][4], "Sewn starting position");
    let suffix = view["contact_replay"]["positions_f32"].as_str().unwrap();
    assert_eq!(suffix.len(), 16 * 12 * 3 * 4 / 3 * 4);
    let map_path = dir.join("sim/mesh_map.json");
    let mut map: Value = serde_json::from_slice(&std::fs::read(&map_path).unwrap()).unwrap();
    let refuse = |message: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&dir)
            .args(["viewer", "sim", "--out", "unchecked.html", "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3));
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            result["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains(message)
        );
        assert!(!dir.join("unchecked.html").exists());
    };
    save(
        &dir.join("sim/sim_report.json"),
        &json!({"self_contact":{"enabled":true}, "animation":{"first_physical_frame":17}}),
    );
    refuse("first physical frame");
    map.as_object_mut().unwrap().remove("frames");
    save(&map_path, &map);
    refuse("frame counts");
}

#[test]
fn viewer_checks_cloth_crossings_even_in_frames_it_would_drop() {
    let dir = setup().join("sim");
    save(
        &dir.join("mesh_map.json"),
        &json!({"units":"m","vertices":6,"frames":{"fold":3,"zip":2,"sew":0,"drape":125}}),
    );
    save(
        &dir.join("sim_report.json"),
        &json!({"self_contact":{"enabled":true},"animation":{"first_physical_frame":4}}),
    );
    std::fs::write(dir.join("body.obj"), "v 0 0 5\nv 1 0 5\nv 0 1 5\nf 1 2 3\n").unwrap();
    std::fs::write(
        dir.join("garment.obj"),
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 0 1 1\nf 1 2 3\nf 4 5 6\n",
    )
    .unwrap();
    let clear = vec![
        Vec3::ZERO,
        Vec3::new(1., 0., 0.),
        Vec3::new(0., 1., 0.),
        Vec3::new(0.2, 0.2, 1.),
        Vec3::new(0.4, 0.2, 1.),
        Vec3::new(0.2, 0.4, 1.),
    ];
    let mut frames = vec![clear; 130];
    let viewer = |name: &str| {
        Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&dir)
            .args(["viewer", ".", "--out", name, "--json"])
            .output()
            .unwrap()
    };
    std::fs::write(dir.join("fold.pc2"), pc2(&frames)).unwrap();
    let contact_only = || {
        Command::new(env!("CARGO_BIN_EXE_ftx"))
            .current_dir(&dir)
            .args(["viewer", ".", "--check-contact", "--json"])
            .output()
            .unwrap()
    };
    let checked = contact_only();
    assert!(checked.status.success());
    let result: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(result["result"]["checked_frames"], 126);
    assert!(!dir.join("fold_viewer.html").exists());
    assert!(viewer("clear.html").status.success());
    // At 60 display samples from 130 source frames, frame 30 lies between 28 and 31.
    frames[30][3].z = -1.;
    std::fs::write(dir.join("fold.pc2"), pc2(&frames)).unwrap();
    assert!(!contact_only().status.success());
    let output = viewer("crossing.html");
    assert!(!output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["refusal"]["code"], "intersecting_trajectory");
    assert_eq!(
        result["refusal"]["message"],
        "physical frame 30: cloth triangle 0 crosses cloth triangle 1"
    );
    assert!(!dir.join("crossing.html").exists());
    save(&dir.join("sim_report.json"), &json!({}));
    let unprotected: Value = serde_json::from_slice(&contact_only().stdout).unwrap();
    assert_eq!(unprotected["refusal"]["code"], "invalid_document");
}

#[test]
fn sim_points_recomputes_record_bearing_measurements_ignoring_stale_or_missing_fused_cache() {
    let dir = setup();
    let expected = run(&dir, &[], 0)["result"].clone();
    let fresh: Value = serde_json::from_slice(&std::fs::read(dir.join("m.json")).unwrap()).unwrap();

    let mut poisoned = fresh.clone();
    poisoned["fused"]["waist_height"]["value_mm"] = json!(123.0);
    save(&dir.join("m.json"), &poisoned);
    let from_poisoned = run(&dir, &[], 0)["result"].clone();
    assert_eq!(from_poisoned["rows"], expected["rows"]);
    assert_ne!(
        from_poisoned["measurements_sha256"],
        expected["measurements_sha256"]
    );

    let mut unfused = fresh;
    unfused.as_object_mut().unwrap().remove("fused");
    save(&dir.join("m.json"), &unfused);
    let from_unfused = run(&dir, &[], 0)["result"].clone();
    assert_eq!(from_unfused["rows"], expected["rows"]);
}
