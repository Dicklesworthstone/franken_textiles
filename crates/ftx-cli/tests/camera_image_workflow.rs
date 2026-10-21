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
# A lens/geometry switch only in the independent view: markers remain readable, but this
# anisotropic stretch cannot be explained by the common pinhole camera and a rigid board.
held=cv2.imread('capture/view3.png',cv2.IMREAD_GRAYSCALE)
A=np.array([[1.18,0,-144.],[0,1.,0],[0,0,1.]])
cv2.imwrite('capture/inconsistent.png',cv2.warpPerspective(held,A,(1600,1200),borderValue=85))
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

#[test]
#[ignore = "requires optional OpenCV Python detector"]
fn inconsistent_held_out_pixels_reject_calibration_without_changing_fit_or_overwriting_evidence() {
    let d = setup();
    render(&d);
    let good = run(&d, CALIBRATE, 0)["result"].clone();
    let mut m = manifest();
    m["views"][3]["image"] = json!("inconsistent.png");
    write(&d, "capture/views.json", &m);
    let mut args = CALIBRATE.to_vec();
    args.extend(["--out", "rejected.json"]);
    let rejected = run(&d, &args, 2)["result"].clone();
    assert_eq!(rejected["status"], "REJECTED");
    assert!(rejected["solution"].is_null());
    assert_eq!(rejected, read(&d, "rejected.json"));
    assert_eq!(rejected["input"]["views"].as_array().unwrap().len(), 4);
    assert_eq!(
        rejected["image_acquisition"]["views"][3]["corners_detected"],
        35
    );
    for field in ["constraint_pivot_ratio", "constraint_squared_residual"] {
        assert_eq!(
            good[field], rejected[field],
            "held-out image changed {field}"
        );
    }
    for i in 0..3 {
        assert_eq!(good["input"]["views"][i], rejected["input"]["views"][i]);
        assert_eq!(good["checks"][i], rejected["checks"][i]);
    }
    assert!(!rejected["flags"].as_array().unwrap().is_empty());
    // A diagnostic remains rejected when reused; neither photos nor poses may treat it as K.
    let refused = run(
        &d,
        &[
            "measure",
            "photo-scale",
            "--image",
            "capture/view0.png",
            "--sheet",
            "sheet/calib-sheet-letter.json",
            "--lens",
            "rejected.json",
            "--lens-id",
            "synthetic-pinhole",
            "--rotation-cw-deg",
            "0",
            "--out",
            "no-scale.json",
        ],
        3,
    );
    assert!(
        refused["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("ACCEPTED")
    );
    assert!(!d.join("no-scale.json").exists());
    // Atomic create-only output applies to valid and rejected acquisitions alike. Input
    // photos, sidecars, manifests, prior diagnostics and hard-link aliases are all evidence.
    std::fs::hard_link(d.join("capture/view0.png"), d.join("photo-alias")).unwrap();
    for path in [
        "capture/view0.png",
        "sheet/calib-sheet-letter.json",
        "capture/views.json",
        "rejected.json",
        "photo-alias",
    ] {
        let before = std::fs::read(d.join(path)).unwrap();
        let mut args = CALIBRATE.to_vec();
        args.extend(["--out", path]);
        run(&d, &args, 3);
        assert_eq!(
            std::fs::read(d.join(path)).unwrap(),
            before,
            "changed {path}"
        );
    }
}

// Direct inverse warping of raw Brown pixels. Neither the Rust fit nor OpenCV calibration/
// projection routines supply the synthetic observations or the independent measurement targets.
const BROWN_RENDER: &str = r#"
import cv2, json, numpy as np
from pathlib import Path
s=json.loads(Path('sheet/calib-sheet-letter.json').read_text())
page=cv2.imread('sheet/calib-sheet-letter.png', cv2.IMREAD_GRAYSCALE)
b=s['board']; ox,oy=b['origin_px']; rw,rh=b['size_px']
sx=b['size_mm'][0]/1000/rw; sy=b['size_mm'][1]/1000/rh
S=np.array([[sx,0,(.5-ox)*sx],[0,sy,(.5-oy)*sy],[0,0,1.]])
fx,fy,cx,cy=1050.,1000.,790.,610.
def distort(x,y):
    r2=x*x+y*y; radial=1+.12*r2-.035*r2*r2+.009*r2*r2*r2
    return (x*radial+.004*x*y-.0015*(r2+2*x*x),
            y*radial+.002*(r2+2*y*y)-.003*x*y)
yy,xx=np.mgrid[:1200,:1600].astype(np.float64)
dx,dy=(xx-cx)/fx,(yy-cy)/fy
ux,uy=dx.copy(),dy.copy()
for _ in range(30):
    x,y=distort(ux,uy); ux+=dx-x; uy+=dy-y
assert max(np.max(np.abs(distort(ux,uy)[0]-dx)),np.max(np.abs(distort(ux,uy)[1]-dy))) < 1e-10
rays=np.stack((ux,uy,np.ones_like(ux))).reshape(3,-1)
poses=[(20,-17,4,-.055,-.035,.25),(-23,24,-7,.06,.045,.27),
       (16,28,12,-.055,.055,.28),(-27,-18,6,.06,-.055,.28),
       (33,-22,-10,0.,-.015,.225),(-20,32,8,-.065,.02,.29),
       (12,-15,3,-.03,-.01,.30)]
for i,(a,b,c,tx,ty,tz) in enumerate(poses):
    x,y,z=np.deg2rad([a,b,c])
    Rx=np.array([[1,0,0],[0,np.cos(x),-np.sin(x)],[0,np.sin(x),np.cos(x)]])
    Ry=np.array([[np.cos(y),0,np.sin(y)],[0,1,0],[-np.sin(y),0,np.cos(y)]])
    Rz=np.array([[np.cos(z),-np.sin(z),0],[np.sin(z),np.cos(z),0],[0,0,1]])
    R=Rz@Ry@Rx; t=np.array([tx,ty,tz])-R@np.array([.088,.066,0.])
    G=np.column_stack((R[:,0],R[:,1],t))
    sample=np.linalg.inv(G@S)@rays
    mapx=(sample[0]/sample[2]).reshape(1200,1600).astype(np.float32)
    mapy=(sample[1]/sample[2]).reshape(1200,1600).astype(np.float32)
    im=cv2.remap(page,mapx,mapy,cv2.INTER_LINEAR,borderMode=cv2.BORDER_CONSTANT,borderValue=85)
    cv2.imwrite(f'capture/view{i}.png',im)
    if i in (5,6):
        def project(p):
            v=G@np.array([*p,1.]); x,y=distort(v[0]/v[2],v[1]/v[2])
            return [fx*x+cx,fy*y+cy]
        # The second span extends below the board, not just between fitted chessboard corners.
        points={'across_back':[project((.022,.044)),project((.154,.044))],
                'test_span':[project((.022,.16)),project((.172,.16))]}
        assert all(0<=p[0]<1600 and 0<=p[1]<1200 for pair in points.values() for p in pair)
        name='brown-points.json' if i==5 else 'independent-points.json'
        Path('capture/'+name).write_text(json.dumps(points))
"#;

#[test]
#[ignore = "requires optional OpenCV Python detector"]
fn raw_brown_images_calibrate_and_measure_through_the_saved_lens_without_transcription() {
    let d = setup();
    let mut m = manifest();
    m["image_model"] = json!("brown5_zero_skew");
    m.as_object_mut().unwrap().remove("pinhole_evidence");
    m["distortion_fit"] = json!({"model_evidence":"Independent synthetic raw Brown renderer; not real camera evidence",
        "max_radius":0.9,"max_iterations":80});
    m["views"] = json!(
        (0..6)
            .map(|i| json!({"image":format!("view{i}.png"),
        "frame":{"stream":"synthetic-burst","timestamp_us":i*100_000},
        "partition":if i==5 {"held_out"} else {"fit"}}))
            .collect::<Vec<_>>()
    );
    write(&d, "capture/views.json", &m);
    let o = Command::new("python3")
        .current_dir(&d)
        .args(["-I", "-c", BROWN_RENDER])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let mut args = CALIBRATE.to_vec();
    args.extend(["--out", "brown-camera.json"]);
    let r = run(&d, &args, 0)["result"].clone();
    let k = &r["solution"]["intrinsics"];
    assert_eq!(r["status"], "ACCEPTED");
    assert_eq!(r["input"]["image_model"], "brown5_zero_skew");
    assert!(r["input"].get("pinhole_evidence").is_none());
    for (field, want, tolerance) in [
        ("focal_px", [1050., 1000.], 20.),
        ("principal_px", [790., 610.], 10.),
    ] {
        for i in 0..2 {
            assert!(
                (k[field][i].as_f64().unwrap() - want[i]).abs() < tolerance,
                "{k}"
            );
        }
    }
    assert!(
        (k["distortion"]["radial"][0].as_f64().unwrap() - 0.12).abs() < 0.05,
        "{k}"
    );
    assert_eq!(r["image_acquisition"]["views"].as_array().unwrap().len(), 6);
    assert!(
        r["image_acquisition"]["views"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["detector"]["uses_lens_intrinsics"] == false)
    );
    assert!(
        r["checks"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|v| v["corners"].as_array().unwrap())
            .all(|p| p["error_px"].as_f64().unwrap() < 1.0)
    );
    let measured = run(
        &d,
        &[
            "measure",
            "measure-photo",
            "--image",
            "capture/view5.png",
            "--sheet",
            "sheet/calib-sheet-letter.json",
            "--lens",
            "brown-camera.json",
            "--lens-id",
            "synthetic-pinhole",
            "--rotation-cw-deg",
            "0",
            "--points",
            "capture/brown-points.json",
            "--out",
            "measured-brown.json",
        ],
        0,
    )["result"]
        .clone();
    for (name, want) in [("across_back", 132.), ("test_span", 150.)] {
        let record = measured["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap();
        assert!(
            (record["value_mm"].as_f64().unwrap() - want).abs() < 1.,
            "{record}"
        );
        assert_eq!(record["provenance"], "PHOTO_ESTIMATED");
        assert!(record["sigma_mm"].as_f64().unwrap() > 0.);
    }
    assert_eq!(measured["photo"]["lens_input"]["recomputed"], true);
    assert_eq!(measured["photo"]["intrinsics"], *k);
    assert!(measured["photo"].get("homography_px_to_board_mm").is_none());
    run(&d, &["measure", "validate", "measured-brown.json"], 0);
    // Refitting the retained pixels is Python-free and does not need the source images.
    write(&d, "brown-coordinates.json", &r["input"]);
    let replay = run(
        &d,
        &["measure", "calibrate-camera", "brown-coordinates.json"],
        0,
    );
    assert_eq!(replay["result"]["solution"], r["solution"]);
    assert_eq!(replay["result"]["refinement"], r["refinement"]);
    let mut changed = r.clone();
    changed["solution"]["intrinsics"]["distortion"]["radial"][0] = json!(0.);
    write(&d, "altered-camera.json", &changed);
    let refused = run(
        &d,
        &[
            "measure",
            "measure-photo",
            "--image",
            "capture/view5.png",
            "--sheet",
            "sheet/calib-sheet-letter.json",
            "--lens",
            "altered-camera.json",
            "--lens-id",
            "synthetic-pinhole",
            "--rotation-cw-deg",
            "0",
            "--points",
            "capture/brown-points.json",
            "--out",
            "not-created.json",
        ],
        3,
    );
    assert!(
        refused["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("stored intrinsics differ")
    );
    assert!(!d.join("not-created.json").exists());
}

#[test]
#[ignore = "requires optional OpenCV Python detector"]
fn fitted_intrinsic_covariance_replays_into_independent_photo_uncertainty_and_refuses_reuse() {
    let d = setup();
    let mut m = manifest();
    m["image_model"] = json!("brown5_zero_skew");
    m.as_object_mut().unwrap().remove("pinhole_evidence");
    m["distortion_fit"] = json!({"model_evidence":"Independent synthetic raw Brown renderer",
        "max_radius":0.9,"max_iterations":80,
        "corner_noise":{"model":"independent_isotropic","sigma_px":0.2,
            "evidence":"Assumed independent 0.2 px noise for synthetic test only; not measured detector uncertainty"}});
    m["views"] = json!(
        (0..6)
            .map(|i| json!({"image":format!("view{i}.png"),
        "frame":{"stream":"synthetic-burst","timestamp_us":i*100_000},
        "partition":if i==5 {"held_out"} else {"fit"}}))
            .collect::<Vec<_>>()
    );
    write(&d, "capture/views.json", &m);
    let render = Command::new("python3")
        .current_dir(&d)
        .args(["-I", "-c", BROWN_RENDER])
        .output()
        .unwrap();
    assert!(
        render.status.success(),
        "{}",
        String::from_utf8_lossy(&render.stderr)
    );
    let mut args = CALIBRATE.to_vec();
    args.extend(["--out", "uncertain-camera.json"]);
    let calibration = run(&d, &args, 0)["result"].clone();
    let u = &calibration["uncertainty"];
    assert_eq!(u["schema"], "ftx-intrinsic-uncertainty/1");
    assert_eq!(
        u["parameters"],
        json!(["fx", "fy", "cx", "cy", "k1", "k2", "k3", "p1", "p2"])
    );
    assert_eq!(u["covariance"].as_array().unwrap().len(), 9);
    write(&d, "coordinate-input.json", &calibration["input"]);
    let replay = run(
        &d,
        &["measure", "calibrate-camera", "coordinate-input.json"],
        0,
    );
    assert_eq!(replay["result"]["uncertainty"], *u);
    assert_eq!(replay["result"]["solution"], calibration["solution"]);
    let photo = |image, lens, out, evidence| {
        let mut args = vec![
            "measure",
            "measure-photo",
            "--image",
            image,
            "--sheet",
            "sheet/calib-sheet-letter.json",
            "--lens",
            lens,
            "--lens-id",
            "synthetic-pinhole",
            "--rotation-cw-deg",
            "0",
            "--points",
            "capture/independent-points.json",
            "--out",
            out,
        ];
        if evidence {
            args.extend([
                "--independent-calibration-evidence",
                "Seventh synthetic acquisition, not used in fitting or held-out selection",
            ]);
        }
        args
    };
    let no_evidence = run(
        &d,
        &photo(
            "capture/view6.png",
            "uncertain-camera.json",
            "no-evidence.json",
            false,
        ),
        3,
    );
    assert!(
        no_evidence["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("independent-calibration-evidence")
    );
    assert!(!d.join("no-evidence.json").exists());
    let measured = run(
        &d,
        &photo(
            "capture/view6.png",
            "uncertain-camera.json",
            "with-uncertainty.json",
            true,
        ),
        0,
    )["result"]
        .clone();
    for (name, want) in [("across_back", 132.), ("test_span", 150.)] {
        let r = measured["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap();
        assert!((r["value_mm"].as_f64().unwrap() - want).abs() < 1., "{r}");
        assert!(
            r["sigma_terms_mm"]["intrinsics"].as_f64().unwrap() > 0.,
            "{r}"
        );
        assert_eq!(r["provenance"], "PHOTO_ESTIMATED");
    }
    assert_eq!(
        measured["photo"]["intrinsic_uncertainty"]["calibration"],
        *u
    );
    run(&d, &["measure", "validate", "with-uncertainty.json"], 0);
    // The concurrently published shared API and separate-noise CLI input use the same
    // numerical covariance. Transport that report through the actual image measurement too.
    let mut shared_input = calibration["input"].clone();
    shared_input["distortion_fit"]
        .as_object_mut()
        .unwrap()
        .remove("corner_noise");
    write(&d, "shared-input.json", &shared_input);
    write(
        &d,
        "noise.json",
        &json!({
            "schema":"ftx-calibration-corner-noise/1", "sigma_px":0.2,
            "evidence":"Assumed synthetic 0.2 px error, not empirical detector precision",
            "independence_evidence":"Independent synthetic fitting coordinates and acquisitions"
        }),
    );
    let shared = run(
        &d,
        &[
            "measure",
            "calibrate-camera",
            "shared-input.json",
            "--corner-noise",
            "noise.json",
            "--out",
            "shared-camera.json",
        ],
        0,
    )["result"]
        .clone();
    assert_eq!(shared["conditional_uncertainty"]["status"], "AVAILABLE");
    assert_eq!(
        shared["conditional_uncertainty"]["estimate"]["covariance"],
        u["covariance"]
    );
    let shared_measurement = run(
        &d,
        &photo(
            "capture/view6.png",
            "shared-camera.json",
            "shared-measurements.json",
            true,
        ),
        0,
    )["result"]
        .clone();
    assert_eq!(shared_measurement["records"], measured["records"]);
    assert_eq!(
        shared_measurement["photo"]["intrinsic_uncertainty"]["calibration"]["calibration_independence_evidence"],
        "Independent synthetic fitting coordinates and acquisitions"
    );
    let mut stripped = shared;
    stripped
        .as_object_mut()
        .unwrap()
        .remove("conditional_uncertainty");
    write(&d, "stripped-shared.json", &stripped);
    let bad = run(
        &d,
        &photo(
            "capture/view6.png",
            "stripped-shared.json",
            "no-shared-output.json",
            true,
        ),
        3,
    );
    assert!(
        bad["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("stored conditional uncertainty")
    );
    assert!(!d.join("no-shared-output.json").exists());
    let mut changed = calibration.clone();
    changed["uncertainty"]["covariance"][0][0] = json!(123.);
    write(&d, "changed-camera.json", &changed);
    let bad = run(
        &d,
        &photo(
            "capture/view6.png",
            "changed-camera.json",
            "bad-covariance.json",
            true,
        ),
        3,
    );
    assert!(
        bad["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("stored intrinsic uncertainty")
    );
    assert!(!d.join("bad-covariance.json").exists());
    // Omitting a requested covariance must not silently make the lens "exact"; a caller
    // cannot inject covariance either when the original input requested no noise model.
    for injected in [false, true] {
        let mut changed = calibration.clone();
        if injected {
            changed["input"]["distortion_fit"]
                .as_object_mut()
                .unwrap()
                .remove("corner_noise");
        } else {
            changed.as_object_mut().unwrap().remove("uncertainty");
        }
        write(&d, "changed-camera.json", &changed);
        let bad = run(
            &d,
            &photo(
                "capture/view6.png",
                "changed-camera.json",
                "bad-covariance.json",
                true,
            ),
            3,
        );
        assert!(
            bad["refusal"]["message"]
                .as_str()
                .unwrap()
                .contains("stored intrinsic uncertainty")
        );
        assert!(!d.join("bad-covariance.json").exists());
    }
    let reused = run(
        &d,
        &photo(
            "capture/view0.png",
            "uncertain-camera.json",
            "reused.json",
            true,
        ),
        3,
    );
    assert!(
        reused["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("reuses a calibration image")
    );
    assert!(!d.join("reused.json").exists());
    let copy=Command::new("python3").current_dir(&d).args(["-I","-c",
        "import cv2; im=cv2.imread('capture/view0.png'); assert cv2.imwrite('capture/reencoded.png',im,[cv2.IMWRITE_PNG_COMPRESSION,9])"]).output().unwrap();
    assert!(copy.status.success());
    assert_ne!(
        std::fs::read(d.join("capture/view0.png")).unwrap(),
        std::fs::read(d.join("capture/reencoded.png")).unwrap()
    );
    let reused = run(
        &d,
        &photo(
            "capture/reencoded.png",
            "uncertain-camera.json",
            "reencoded-output.json",
            true,
        ),
        3,
    );
    assert!(
        reused["refusal"]["message"]
            .as_str()
            .unwrap()
            .contains("reuses a calibration image")
    );
    assert!(!d.join("reencoded-output.json").exists());
}
