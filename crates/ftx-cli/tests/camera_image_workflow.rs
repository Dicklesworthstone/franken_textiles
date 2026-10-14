//! Printed sheet -> independent camera renders -> detected IDs -> native calibration -> checked reuse.
//! All photographs and measurements are synthetic. Optional image tests require OpenCV 4.13.
use ftx_core::receipt::sha256_hex;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

fn scratch() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "ftx-camera-images-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&p).unwrap();
    p
}
fn write(dir: &Path, name: &str, v: &Value) {
    std::fs::write(dir.join(name), serde_json::to_vec_pretty(v).unwrap()).unwrap();
}
fn read(dir: &Path, name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(name)).unwrap()).unwrap()
}
fn run(dir: &Path, args: &[&str], code: i32) -> Value {
    let o = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        o.status.code(),
        Some(code),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_slice(&o.stdout).unwrap()
}
fn manifest() -> Value {
    json!({"schema":"ftx-camera-calibration-images/1","image_model":"pinhole_zero_skew",
        "lens_id":"synthetic-pinhole","pinhole_evidence":"Independent pinhole image renderer; no lens distortion",
        "reference_evidence":"Synthetic sheet with exact 22 mm squares; no real photograph",
        "image_size_px":[1600,1200],"rotation_cw_deg":0,"held_out_corner_ids":[0,6,28,34],
        "views":(0..4).map(|i|json!({"image":format!("view{i}.png"),
            "frame":{"stream":"synthetic-burst","timestamp_us":i*100_000},
            "partition":if i==3 {"held_out"} else {"fit"}})).collect::<Vec<_>>()})
}
const CALIBRATE: &[&str] = &[
    "measure",
    "calibrate-camera",
    "--images",
    "capture/views.json",
    "--sheet",
    "sheet/calib-sheet-letter.json",
];

fn setup() -> PathBuf {
    let d = scratch();
    run(
        &d,
        &[
            "measure",
            "calib-sheet",
            "--paper",
            "letter",
            "--out",
            "sheet",
        ],
        0,
    );
    std::fs::create_dir(d.join("capture")).unwrap();
    write(&d, "capture/views.json", &manifest());
    d
}

const RENDER: &str = r#"
import cv2, json, numpy as np
from pathlib import Path
s=json.loads(Path('sheet/calib-sheet-letter.json').read_text())
page=cv2.imread('sheet/calib-sheet-letter.png', cv2.IMREAD_GRAYSCALE)
b=s['board']; ox,oy=b['origin_px']; rw,rh=b['size_px']
sx=b['size_mm'][0]/1000/rw; sy=b['size_mm'][1]/1000/rh
S=np.array([[sx,0,(.5-ox)*sx],[0,sy,(.5-oy)*sy],[0,0,1.]])
K=np.array([[1400.,0,800.],[0,1380.,600.],[0,0,1.]])
angles=[(25,-17,4),(-23,24,-7),(16,28,12),(-27,-18,6)]
projected=[]
for i,abc in enumerate(angles):
    x,y,z=np.deg2rad(abc)
    Rx=np.array([[1,0,0],[0,np.cos(x),-np.sin(x)],[0,np.sin(x),np.cos(x)]])
    Ry=np.array([[np.cos(y),0,np.sin(y)],[0,1,0],[-np.sin(y),0,np.cos(y)]])
    Rz=np.array([[np.cos(z),-np.sin(z),0],[np.sin(z),np.cos(z),0],[0,0,1]])
    R=Rz@Ry@Rx; t=np.array([-.085,-.063,.40+i*.02])
    G=K@np.column_stack((R[:,0],R[:,1],t))
    im=cv2.warpPerspective(page,G@S,(1600,1200),flags=cv2.INTER_LINEAR,borderValue=85)
    cv2.imwrite(f'capture/view{i}.png',im)
    def project(p):
        v=G@np.array([*p,1.]);return (v[:2]/v[2]).tolist()
    points=[project((.044,.044)),project((.132,.044))]
    projected.append(points)
Path('capture/points.json').write_text(json.dumps({'across_back':projected[3]}))
# Same decoded pixels with a different encoding; it must not become a held-out view.
im=cv2.imread('capture/view0.png')
cv2.imwrite('capture/copy.png',im,[cv2.IMWRITE_PNG_COMPRESSION,0])
cv2.imwrite('capture/blank.png',np.full((1200,1600),127,np.uint8))
"#;

fn render(d: &Path) {
    let o = Command::new("python3")
        .current_dir(d)
        .args(["-I", "-c", RENDER])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn image_calibration_rejects_ambiguous_options_metadata_and_budgets_before_detection() {
    let d = setup();
    for extra in [
        vec!["--sheet", "x.json"],
        vec!["--detector-python", "missing"],
        vec![
            "coordinates.json",
            "--images",
            "capture/views.json",
            "--sheet",
            "sheet/calib-sheet-letter.json",
        ],
    ] {
        let mut args = vec!["measure", "calibrate-camera"];
        args.extend(extra);
        assert_eq!(run(&d, &args, 3)["refusal"]["code"], "invalid_document");
    }
    for kind in 0..6 {
        let mut m = manifest();
        match kind {
            0 => m["views"][3]["partition"] = json!("fit"),
            1 => m["held_out_corner_ids"] = json!([0, 0, 6, 28, 34]),
            2 => m["views"][1]["frame"] = m["views"][0]["frame"].clone(),
            3 => m["intrinsics"] = json!({}),
            4 => m["image_size_px"] = json!([u32::MAX, u32::MAX]),
            _ => m["reference_evidence"] = json!(""),
        }
        write(&d, "capture/views.json", &m);
        let mut args = CALIBRATE.to_vec();
        args.extend(["--detector-python", "missing", "--out", "no/result.json"]);
        let code = if kind == 4 { 4 } else { 3 };
        run(&d, &args, code);
        assert!(!d.join("no").exists());
    }
}

#[test]
#[ignore = "requires optional OpenCV Python detector"]
fn images_calibrate_and_replay_through_the_existing_checked_camera_handoff() {
    let d = setup();
    render(&d);
    let mut args = CALIBRATE.to_vec();
    args.extend(["--out", "camera.json"]);
    let r = run(&d, &args, 0)["result"].clone();
    assert_eq!(r, read(&d, "camera.json"));
    assert_eq!(r["status"], "ACCEPTED");
    let k = &r["solution"]["intrinsics"];
    for (field, expected, tol) in [
        ("focal_px", [1400., 1380.], 10.),
        ("principal_px", [800., 600.], 5.),
    ] {
        for j in 0..2 {
            assert!(
                (k[field][j].as_f64().unwrap() - expected[j]).abs() < tol,
                "{k}"
            );
        }
    }
    let acquisition = &r["image_acquisition"];
    assert_eq!(
        acquisition["manifest_sha256"],
        sha256_hex(&std::fs::read(d.join("capture/views.json")).unwrap())
    );
    assert_eq!(
        acquisition["sheet_sha256"],
        sha256_hex(&std::fs::read(d.join("sheet/calib-sheet-letter.json")).unwrap())
    );
    assert_eq!(acquisition["views"].as_array().unwrap().len(), 4);
    assert_eq!(r["input"]["views"][3]["partition"], "held_out");
    for v in r["input"]["views"].as_array().unwrap() {
        assert_eq!(
            v["corners"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["partition"] == "held_out")
                .count(),
            4
        );
    }
    // Replaying canonical coordinates neither redetects pixels nor needs Python/files.
    write(&d, "coordinates.json", &r["input"]);
    let replay = run(&d, &["measure", "calibrate-camera", "coordinates.json"], 0);
    assert_eq!(replay["result"]["solution"], r["solution"]);
    assert_eq!(replay["result"]["checks"], r["checks"]);
    let v = &r["input"]["views"][3];
    let controls = r["input"]["controls"].as_array().unwrap();
    let mut points = Vec::new();
    for p in v["corners"].as_array().unwrap() {
        let c = controls.iter().find(|c| c["id"] == p["id"]).unwrap();
        points.push(
            json!({"sheet_xz_m":c["sheet_xz_m"],"pixel_px":p["pixel_px"],
            "held_out":p["partition"] == "held_out"}),
        );
    }
    // Use the same documented sheet-pose schema, not a separate camera representation.
    let template: Value = serde_json::from_str(include_str!(
        "../../../docs/examples/calibrated-sheet-pose.json"
    ))
    .unwrap();
    let mut pose = template;
    pose["image_size_px"] = json!([1600, 1200]);
    pose["points"] = json!(points);
    write(&d, "pose.json", &pose);
    let reused = run(
        &d,
        &[
            "measure",
            "sheet-pose",
            "--reference",
            "pose.json",
            "--calibration",
            "camera.json",
            "--lens-id",
            "synthetic-pinhole",
        ],
        0,
    );
    assert_eq!(reused["result"]["camera"]["intrinsics"], *k);
    assert_eq!(reused["result"]["intrinsics_source"]["recomputed"], true);
}

#[test]
#[ignore = "requires optional OpenCV Python detector"]
fn image_calibration_never_drops_a_failed_view_or_counts_a_reencoded_copy_as_independent() {
    let d = setup();
    render(&d);
    for replacement in ["copy.png", "blank.png"] {
        let mut m = manifest();
        m["views"][3]["image"] = json!(replacement);
        write(&d, "capture/views.json", &m);
        let mut args = CALIBRATE.to_vec();
        args.extend(["--out", "not-created.json"]);
        let r = run(&d, &args, 3);
        assert_eq!(
            r["refusal"]["code"],
            if replacement == "copy.png" {
                "invalid_document"
            } else {
                "unscaled"
            }
        );
        assert!(!d.join("not-created.json").exists());
    }
}
