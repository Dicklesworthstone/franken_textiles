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

    // Custom --out path also works.
    let out_custom = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args(["viewer", "sim", "--out", "custom/viewer.html", "--json"])
        .output()
        .unwrap();
    assert_eq!(out_custom.status.code(), Some(0));
    assert!(dir.join("custom/viewer.html").is_file());
}
