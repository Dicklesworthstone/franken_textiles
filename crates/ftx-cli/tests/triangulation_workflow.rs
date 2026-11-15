//! Exercise the actual CLI, output receipts and nonmutation guarantees with synthetic cameras.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ftx_core::{Mat3, RefusalCode, Vec2, Vec3};
use ftx_measure::{
    camera::Camera,
    photo::intrinsics::{CalibrationInput, calibrate_camera},
};
use serde_json::{Value, json};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ftx-triangulation-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("input.json"),
            include_str!("../../../docs/examples/triangulation.json"),
        )
        .unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn save(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn run(dir: &Path, extra: &[&str], expected: i32) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(["measure", "triangulate", "input.json", "--json"])
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{} / {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let envelope: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(envelope["exit"], expected);
    envelope
}

#[test]
fn accepted_receipt_and_human_summary() {
    let dir = Scratch::new();
    let out = run(&dir.0, &["--out", "point.json"], 0);
    let result = &out["result"];
    assert_eq!(read(&dir.0.join("point.json")), *result);
    assert_eq!(result["status"], "ACCEPTED");
    assert_eq!(result["fit_views"], 2);
    assert_eq!(result["held_out_views"], 1);
    assert!(result["point_m"][0].as_f64().unwrap().abs() < 1e-10);
    assert!(result["point_m"][1].as_f64().unwrap().abs() < 1e-10);
    assert!((result["point_m"][2].as_f64().unwrap() - 1.3).abs() < 1e-10);
    assert!(result["input"]["metric_scale_source"].is_string());
    assert!(result.get("records").is_none());
    let human = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir.0)
        .args(["measure", "triangulate", "input.json"])
        .output()
        .unwrap();
    assert!(human.status.success());
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("TRIANGULATION: ACCEPTED"));
    assert!(text.contains("1 held-out"));
    assert!(text.contains("not a body fit"));
}

#[test]
fn rejected_and_unobservable_reports_have_no_usable_point() {
    let dir = Scratch::new();
    let path = dir.0.join("input.json");
    let mut input = read(&path);
    input["observations"][2]["pixel_px"][0] = 540.into();
    save(&path, &input);
    let out = run(&dir.0, &["--out", "rejected.json"], 2);
    assert_eq!(out["result"]["status"], "REJECTED");
    assert!(out["result"]["point_m"].is_null());
    assert_eq!(read(&dir.0.join("rejected.json")), out["result"]);
    input["observations"][1]["partition"] = "held_out".into();
    save(&path, &input);
    let out = run(&dir.0, &[], 2);
    assert_eq!(out["result"]["status"], "UNOBSERVABLE");
    assert!(out["result"]["point_m"].is_null());
}

#[test]
fn invalid_input_or_options_do_not_create_output() {
    let dir = Scratch::new();
    let path = dir.0.join("input.json");
    let before = std::fs::read(&path).unwrap();
    run(
        &dir.0,
        &["--min-angle-deg", "0", "--out", "new/point.json"],
        3,
    );
    assert!(!dir.0.join("new").exists());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut input = read(&path);
    input["observations"][2]["frame"]["timestamp_us"] = 1_000_001.into();
    save(&path, &input);
    run(&dir.0, &["--out", "new/point.json"], 3);
    assert!(!dir.0.join("new").exists());
    input["observations"][2]["frame"]["timestamp_us"] = 1_000_000.into();
    input["observations"][0]["frame_number"] = 1.into();
    save(&path, &input);
    run(&dir.0, &["--out", "new/point.json"], 3);
    assert!(!dir.0.join("new").exists());
}

#[test]
fn inputs_and_existing_reports_cannot_be_overwritten_even_by_alias() {
    let dir = Scratch::new();
    let input = dir.0.join("input.json");
    let before = std::fs::read(&input).unwrap();
    run(&dir.0, &["--out", "input.json"], 3);
    assert_eq!(std::fs::read(&input).unwrap(), before);
    std::fs::hard_link(&input, dir.0.join("alias.json")).unwrap();
    run(&dir.0, &["--out", "alias.json"], 3);
    assert_eq!(std::fs::read(&input).unwrap(), before);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&input, dir.0.join("link.json")).unwrap();
        run(&dir.0, &["--out", "link.json"], 3);
        assert_eq!(std::fs::read(&input).unwrap(), before);
    }
    run(&dir.0, &["--out", "point.json"], 0);
    let report = std::fs::read(dir.0.join("point.json")).unwrap();
    run(&dir.0, &["--out", "point.json"], 3);
    assert_eq!(std::fs::read(dir.0.join("point.json")).unwrap(), report);
}

// Independent polynomial observations from the existing Brown-calibration known-answer case.
// Calibration views and the triangulated landmark use separate synthetic acquisitions.
fn retained_brown_calibration() -> &'static Value {
    static REPORT: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    REPORT.get_or_init(|| {
        let controls: Vec<_> = (0..48)
            .map(|i| {
                json!({"id":format!("p{i:02}"),
                    "sheet_xz_m":[(i % 8) as f64 * 0.06 - 0.21, (i / 8) as f64 * 0.06 - 0.15]})
            })
            .collect();
        let base = Mat3::from_rows([[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]]);
        let poses = [
            (0.35, 0.12, -0.18, -0.10, 0.80),
            (-0.45, 0.3, 0.16, 0.13, 0.75),
            (0.2, -0.55, -0.12, 0.18, 0.85),
            (-0.3, -0.25, 0.23, -0.14, 0.8),
            (0.5, -0.4, 0.03, -0.04, 0.66),
            (-0.2, 0.5, -0.24, 0.06, 0.9),
        ];
        let views: Vec<_> = poses.iter().enumerate().map(|(i, &(a, b, tx, ty, tz))| {
            let r = Mat3::rotation(Vec3::new(1., 0., 0.), a)
                * Mat3::rotation(Vec3::new(0., 1., 0.), b) * base;
            let corners: Vec<_> = controls.iter().enumerate().map(|(j, p)| {
                let q = r.mul_vec(Vec3::new(p["sheet_xz_m"][0].as_f64().unwrap(), 0.,
                    p["sheet_xz_m"][1].as_f64().unwrap())) + Vec3::new(tx, ty, tz);
                let (x, y) = (q.x / q.z, q.y / q.z);
                let r2 = x * x + y * y;
                let radial = 1.0 + 0.12 * r2 - 0.035 * r2 * r2 + 0.009 * r2 * r2 * r2;
                let pixel = Vec2::new(
                    1050. * (x * radial + 0.004 * x * y - 0.0015 * (r2 + 2. * x * x)) + 790.,
                    1000. * (y * radial + 0.002 * (r2 + 2. * y * y) - 0.003 * x * y) + 610.);
                json!({"id":p["id"],"pixel_px":pixel,
                    "partition":if [0,7,40,47].contains(&j) {"held_out"} else {"fit"}})
            }).collect();
            json!({"frame":{"stream":"synthetic-calibration","timestamp_us":i * 100000},
                "partition":if i == 5 {"held_out"} else {"fit"},"corners":corners})
        }).collect();
        let input: CalibrationInput = serde_json::from_value(json!({
            "schema":"ftx-camera-calibration-input/1","image_model":"brown5_zero_skew",
            "lens_id":"synthetic-shared-brown",
            "distortion_fit":{"model_evidence":"Independent synthetic polynomial lens",
                "max_radius":0.9,"max_iterations":80,"corner_noise":{"model":"independent_isotropic",
                    "sigma_px":0.2,"evidence":"Declared synthetic independent per-coordinate noise"}},
            "reference_evidence":"Synthetic 60 mm planar grid","image_size_px":[1600,1200],
            "rotation_cw_deg":0,"controls":controls,"views":views})).unwrap();
        let report = calibrate_camera(&input).unwrap();
        assert_eq!(report.status, "ACCEPTED");
        assert!(report.uncertainty.is_some());
        json!(report)
    })
}

fn project_landmark(input: &mut Value) {
    for view in input["observations"].as_array_mut().unwrap() {
        let camera: Camera = serde_json::from_value(view["camera"].clone()).unwrap();
        view["pixel_px"] = json!(camera.project(Vec3::new(0.0, 0.0, 1.3)).unwrap());
    }
}

fn lens_case(dir: &Path) -> (Value, Value, Value) {
    let mut input = read(&dir.join("input.json"));
    input["capture"] = json!({"kind":"still_window","start_us":10_000_000,"end_us":10_200_000,
        "evidence":"Synthetic stationary landmark and moving camera; no real wearer"});
    for (i, view) in input["observations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        view["frame"] =
            json!({"stream":"synthetic-landmark","timestamp_us":10_000_000 + i * 100_000});
        view["camera"]["intrinsics"] =
            retained_brown_calibration()["solution"]["intrinsics"].clone();
    }
    project_landmark(&mut input);
    let frames: Vec<_> = input["observations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|view| view["partition"] == "fit")
        .map(|view| view["frame"].clone())
        .collect();
    let click = json!({"schema":"ftx-click-noise/1","evidence":"Synthetic independent half-pixel clicks",
        "observations":frames.iter().map(|frame| json!({"frame":frame,"sigma_px":0.5})).collect::<Vec<_>>()});
    let lens = json!({"schema":"ftx-triangulation-lens-noise/1",
        "independence_evidence":"Calibration grid and landmark clicks are independent synthetic acquisitions; camera poses and scale fixed",
        "groups":[{"key":"shared-lens","calibration":retained_brown_calibration(),"frames":frames}]});
    save(&dir.join("input.json"), &input);
    save(&dir.join("click.json"), &click);
    save(&dir.join("lens.json"), &lens);
    (input, click, lens)
}

#[test]
fn shared_lens_uncertainty_replays_native_and_portable_without_changing_clicks_or_geometry() {
    let dir = Scratch::new();
    let (input, click, lens) = lens_case(&dir.0);
    let original = ["input.json", "click.json", "lens.json"]
        .map(|name| (name, std::fs::read(dir.0.join(name)).unwrap()));
    let plain = run(&dir.0, &[], 0)["result"].clone();
    let click_only = run(&dir.0, &["--click-noise", "click.json"], 0)["result"].clone();
    let lens_only = run(&dir.0, &["--lens-noise", "lens.json"], 0)["result"].clone();
    let both = run(
        &dir.0,
        &[
            "--click-noise",
            "click.json",
            "--lens-noise",
            "lens.json",
            "--out",
            "point.json",
        ],
        0,
    )["result"]
        .clone();
    assert_eq!(read(&dir.0.join("point.json")), both);
    let portable = ftx_wasm::photo::triangulate_with_lens_noise(
        &input.to_string(),
        &click.to_string(),
        &lens.to_string(),
    )
    .unwrap();
    assert_eq!(portable, both);
    assert_eq!(
        ftx_wasm::photo::triangulate_with_lens_noise(&input.to_string(), "", &lens.to_string())
            .unwrap(),
        lens_only
    );
    assert_eq!(
        both["conditional_click_uncertainty"],
        click_only["conditional_click_uncertainty"]
    );
    assert_eq!(
        both["conditional_lens_uncertainty"],
        lens_only["conditional_lens_uncertainty"]
    );
    let uncertainty = &both["conditional_lens_uncertainty"];
    assert_eq!(uncertainty["status"], "AVAILABLE");
    assert_eq!(uncertainty["input"], lens);
    assert_eq!(uncertainty["sensitivities"].as_array().unwrap().len(), 1);
    assert_eq!(
        uncertainty["sensitivities"][0]["frames"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        uncertainty["sigma_m"]
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x.as_f64().unwrap() > 0.0)
    );
    assert!((plain["point_m"][2].as_f64().unwrap() - 1.3).abs() < 1e-9);
    let mut geometric = both.clone();
    geometric
        .as_object_mut()
        .unwrap()
        .remove("conditional_click_uncertainty");
    geometric
        .as_object_mut()
        .unwrap()
        .remove("conditional_lens_uncertainty");
    assert_eq!(geometric, plain);
    assert!(both.get("records").is_none());
    assert!(both.get("total_uncertainty").is_none());
    for (name, bytes) in original {
        assert_eq!(std::fs::read(dir.0.join(name)).unwrap(), bytes);
    }
    if let Ok(directory) = std::env::var("FTX_TRIANGULATION_LENS_FIXTURES") {
        use std::io::Write;
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        for (name, value) in [
            ("triangulation-lens-input.json", &input),
            ("triangulation-click-noise.json", &click),
            ("triangulation-lens-noise.json", &lens),
            ("triangulation-lens-result.json", &lens_only),
            ("triangulation-lens-click-result.json", &both),
        ] {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(&serde_json::to_vec_pretty(value).unwrap())
                .unwrap();
        }
    }
}

#[test]
fn lens_noise_replays_sources_and_refuses_incomplete_or_mismatched_evidence_before_writing() {
    let dir = Scratch::new();
    let (input, _, lens) = lens_case(&dir.0);
    for variant in [
        "covariance",
        "missing",
        "duplicate",
        "held-out",
        "evidence",
        "intrinsics",
    ] {
        let mut changed = lens.clone();
        let mut changed_input = input.clone();
        match variant {
            "covariance" => {
                changed["groups"][0]["calibration"]["uncertainty"]["covariance"][0][0] =
                    json!(123.0)
            }
            "missing" => {
                let _ = changed["groups"][0]["frames"].as_array_mut().unwrap().pop();
            }
            "duplicate" => {
                changed["groups"][0]["frames"][1] = changed["groups"][0]["frames"][0].clone()
            }
            "held-out" => {
                changed["groups"][0]["frames"][1] = input["observations"][2]["frame"].clone()
            }
            "evidence" => changed["independence_evidence"] = json!(" "),
            _ => {
                changed_input["observations"][0]["camera"]["intrinsics"]["focal_px"][0] =
                    json!(1051.0)
            }
        }
        save(&dir.0.join("input.json"), &changed_input);
        save(&dir.0.join("lens.json"), &changed);
        run(
            &dir.0,
            &["--lens-noise", "lens.json", "--out", "new/point.json"],
            3,
        );
        assert!(!dir.0.join("new").exists(), "{variant}");
        assert!(
            ftx_wasm::photo::triangulate_with_lens_noise(
                &changed_input.to_string(),
                "",
                &changed.to_string()
            )
            .is_err(),
            "{variant}"
        );
    }
}

#[test]
fn requested_lens_uncertainty_keeps_geometry_when_its_derivative_is_unavailable() {
    let dir = Scratch::new();
    let (mut input, _, lens) = lens_case(&dir.0);
    let radius = 0.9_f64 - 1e-8;
    let x = 0.71_f64;
    let y = -(radius * radius - x * x).sqrt();
    input["observations"][0]["camera"]["center_m"] = json!([-3.0 * x, -3.0, 1.3 + 3.0 * y]);
    project_landmark(&mut input);
    save(&dir.0.join("input.json"), &input);
    let plain = run(&dir.0, &[], 0)["result"].clone();
    let reported = run(
        &dir.0,
        &["--lens-noise", "lens.json", "--out", "edge.json"],
        2,
    )["result"]
        .clone();
    assert_eq!(reported["status"], "ACCEPTED");
    assert_eq!(reported["point_m"], plain["point_m"]);
    assert_eq!(
        reported["conditional_lens_uncertainty"]["status"],
        "UNAVAILABLE"
    );
    assert!(reported["conditional_lens_uncertainty"]["covariance_m2"].is_null());
    assert!(reported["conditional_lens_uncertainty"]["sigma_m"].is_null());
    assert_eq!(reported, read(&dir.0.join("edge.json")));
    assert_eq!(
        reported,
        ftx_wasm::photo::triangulate_with_lens_noise(&input.to_string(), "", &lens.to_string())
            .unwrap()
    );
    input["observations"][2]["pixel_px"][0] = json!(50.0);
    save(&dir.0.join("input.json"), &input);
    let rejected = run(&dir.0, &["--lens-noise", "lens.json"], 2);
    assert_eq!(rejected["result"]["status"], "REJECTED");
    assert!(rejected["result"]["point_m"].is_null());
    assert_eq!(
        rejected["result"]["conditional_lens_uncertainty"]["status"],
        "UNAVAILABLE"
    );
}

#[test]
fn lens_noise_preserves_no_clobber_and_strict_portable_optional_inputs() {
    let dir = Scratch::new();
    let (input, click, lens) = lens_case(&dir.0);
    let before = ["input.json", "click.json", "lens.json"]
        .map(|name| (name, std::fs::read(dir.0.join(name)).unwrap()));
    std::fs::hard_link(dir.0.join("lens.json"), dir.0.join("lens-alias.json")).unwrap();
    for output in ["input.json", "click.json", "lens.json", "lens-alias.json"] {
        run(
            &dir.0,
            &[
                "--click-noise",
                "click.json",
                "--lens-noise",
                "lens.json",
                "--out",
                output,
            ],
            3,
        );
    }
    for (name, bytes) in before {
        assert_eq!(std::fs::read(dir.0.join(name)).unwrap(), bytes);
    }
    let old = ftx_wasm::photo::triangulate(&input.to_string(), &click.to_string()).unwrap();
    assert_eq!(
        ftx_wasm::photo::triangulate_with_lens_noise(&input.to_string(), &click.to_string(), "")
            .unwrap(),
        old
    );
    for bad in [" ".to_owned(), "{}".to_owned(), format!("\u{feff}{lens}")] {
        assert!(
            ftx_wasm::photo::triangulate_with_lens_noise(&input.to_string(), "", &bad).is_err()
        );
    }
    assert!(
        ftx_wasm::photo::triangulate_with_lens_noise(&input.to_string(), " ", &lens.to_string())
            .is_err()
    );
    let oversized = " ".repeat(16 * 1024 * 1024 + 1);
    assert_eq!(
        ftx_wasm::photo::triangulate_with_lens_noise(&input.to_string(), "", &oversized)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
}

fn sheet_camera_case(dir: &Path) -> (Value, Value, Value, Value) {
    let (mut input, click, mut lens) = lens_case(dir);
    let target = Vec3::new(0.05, 0.4, 1.25);
    let mut poses = Vec::new();
    let mut observations = Vec::new();
    for view in input["observations"].as_array_mut().unwrap() {
        let known: Camera = serde_json::from_value(view["camera"].clone()).unwrap();
        let points: Vec<_> = [0.5, 1.0, 1.5]
            .into_iter()
            .flat_map(|z| [-0.6, 0.0, 0.6].into_iter().map(move |x| (x, z)))
            .enumerate()
            .map(|(i, (x, z))| {
                json!({
                    "sheet_xz_m":[x,z],
                    "pixel_px":known.project(Vec3::new(x,0.0,z)).unwrap(),
                    "held_out":i == 4,
                })
            })
            .collect();
        let report = Camera::sheet_pose_report(&json!({
            "schema":"ftx-sheet-pose-input/1",
            "reference_id":"synthetic-stationary-camera-noise-sheet",
            "intrinsics":known.intrinsics,
            "image_size_px":[1600,1200],
            "rotation_applied_degrees":0,
            "points":points,
        }))
        .unwrap();
        assert_eq!(report["ok"], true, "{report}");
        // The original known camera generates all landmark observations; the retained
        // report supplies the exactly replayable estimated camera for each view.
        view["camera"] = report["camera"].clone();
        view["pixel_px"] = json!(known.project(target).unwrap());
        if view["partition"] == "fit" {
            poses.push(json!({"frame":view["frame"],"report":report}));
            observations.push(json!({
                "frame":view["frame"],"report":report,"sigma_px":0.25,
            }));
        }
    }
    lens["schema"] = json!("ftx-triangulation-lens-noise/2");
    lens["independence_evidence"] = json!(
        "Independent synthetic calibration acquisition; common physical lens across sheet-pose and landmark images"
    );
    lens["sheet_poses"] = json!(poses);
    let sheet = json!({
        "schema":"ftx-triangulation-sheet-noise/1",
        "independence_evidence":"Synthetic independent sheet-corner axes, frames, calibration acquisition and landmark clicks; metric sheet fixed",
        "observations":observations,
    });
    save(&dir.join("input.json"), &input);
    save(&dir.join("lens.json"), &lens);
    save(&dir.join("sheet.json"), &sheet);
    (input, click, lens, sheet)
}

#[test]
fn sheet_camera_uncertainty_replays_native_and_portable_with_separate_components() {
    let dir = Scratch::new();
    let (input, click, lens, sheet) = sheet_camera_case(&dir.0);
    let original = ["input.json", "click.json", "lens.json", "sheet.json"]
        .map(|name| (name, std::fs::read(dir.0.join(name)).unwrap()));
    let plain = run(&dir.0, &[], 0)["result"].clone();
    let click_only = run(&dir.0, &["--click-noise", "click.json"], 0)["result"].clone();
    let lens_only = run(&dir.0, &["--lens-noise", "lens.json"], 0)["result"].clone();
    let sheet_only = run(&dir.0, &["--sheet-noise", "sheet.json"], 0)["result"].clone();
    let sheet_click = run(
        &dir.0,
        &["--sheet-noise", "sheet.json", "--click-noise", "click.json"],
        0,
    )["result"]
        .clone();
    let lens_sheet = run(
        &dir.0,
        &["--sheet-noise", "sheet.json", "--lens-noise", "lens.json"],
        0,
    )["result"]
        .clone();
    let all = run(
        &dir.0,
        &[
            "--sheet-noise",
            "sheet.json",
            "--click-noise",
            "click.json",
            "--lens-noise",
            "lens.json",
            "--out",
            "point.json",
        ],
        0,
    )["result"]
        .clone();
    assert_eq!(read(&dir.0.join("point.json")), all);
    let input_text = input.to_string();
    let click_text = click.to_string();
    let lens_text = lens.to_string();
    let sheet_text = sheet.to_string();
    for (native, clicks, lenses) in [
        (&sheet_only, "", ""),
        (&sheet_click, click_text.as_str(), ""),
        (&lens_sheet, "", lens_text.as_str()),
        (&all, click_text.as_str(), lens_text.as_str()),
    ] {
        let portable = ftx_wasm::photo::triangulate_with_camera_noise(
            &input_text,
            clicks,
            lenses,
            &sheet_text,
        )
        .unwrap();
        assert_eq!(portable, *native);
        assert_eq!(
            native["conditional_sheet_uncertainty"],
            sheet_only["conditional_sheet_uncertainty"]
        );
        let mut geometry = native.clone();
        for component in [
            "conditional_click_uncertainty",
            "conditional_lens_uncertainty",
            "conditional_sheet_uncertainty",
        ] {
            geometry.as_object_mut().unwrap().remove(component);
        }
        assert_eq!(geometry, plain);
        assert!(native.get("total_uncertainty").is_none());
        assert!(native.get("records").is_none());
    }
    assert_eq!(
        all["conditional_click_uncertainty"],
        click_only["conditional_click_uncertainty"]
    );
    assert_eq!(
        sheet_click["conditional_click_uncertainty"],
        click_only["conditional_click_uncertainty"]
    );
    assert_eq!(
        all["conditional_lens_uncertainty"],
        lens_only["conditional_lens_uncertainty"]
    );
    assert_eq!(
        lens_sheet["conditional_lens_uncertainty"],
        lens_only["conditional_lens_uncertainty"]
    );
    assert_eq!(
        all["conditional_lens_uncertainty"]["schema"],
        "ftx-triangulation-lens-uncertainty/2"
    );
    let uncertainty = &all["conditional_sheet_uncertainty"];
    assert_eq!(
        uncertainty["schema"],
        "ftx-triangulation-sheet-uncertainty/1"
    );
    assert_eq!(uncertainty["status"], "AVAILABLE");
    assert_eq!(uncertainty["input"], sheet);
    assert_eq!(uncertainty["sensitivities"].as_array().unwrap().len(), 2);
    for sensitivity in uncertainty["sensitivities"].as_array().unwrap() {
        assert_eq!(
            sensitivity["fit_point_indices"].as_array().unwrap().len(),
            8
        );
        assert_eq!(
            sensitivity["jacobians_m_per_px"].as_array().unwrap().len(),
            8
        );
    }
    for (actual, expected) in plain["point_m"]
        .as_array()
        .unwrap()
        .iter()
        .zip([0.05, 0.4, 1.25])
    {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-8);
    }
    let human = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir.0)
        .args([
            "measure",
            "triangulate",
            "input.json",
            "--lens-noise",
            "lens.json",
            "--sheet-noise",
            "sheet.json",
        ])
        .output()
        .unwrap();
    assert!(human.status.success());
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("source-replayed sheet camera poses"));
    assert!(text.contains("CONDITIONAL SHEET-CORNER UNCERTAINTY: AVAILABLE"));
    assert!(text.contains("fixed lens and metric sheet"));
    for (name, bytes) in original {
        assert_eq!(std::fs::read(dir.0.join(name)).unwrap(), bytes);
    }
    if let Ok(directory) = std::env::var("FTX_TRIANGULATION_CAMERA_FIXTURES") {
        use std::io::Write;
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        for (name, value) in [
            ("camera-input.json", &input),
            ("click-noise.json", &click),
            ("lens-noise.json", &lens),
            ("sheet-noise.json", &sheet),
            ("sheet-result.json", &sheet_only),
            ("sheet-click-result.json", &sheet_click),
            ("lens-sheet-result.json", &lens_sheet),
            ("all-result.json", &all),
        ] {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(&serde_json::to_vec_pretty(value).unwrap())
                .unwrap();
        }
    }
}

#[test]
fn sheet_camera_sources_refuse_stale_incomplete_or_tampered_evidence_before_writing() {
    let dir = Scratch::new();
    let (input, _, _, sheet) = sheet_camera_case(&dir.0);
    for variant in [
        "stale-camera",
        "tampered-report",
        "missing",
        "duplicate",
        "held-out-frame",
        "no-evidence",
        "no-pose-report",
    ] {
        let mut changed_input = input.clone();
        let mut changed = sheet.clone();
        match variant {
            "stale-camera" => {
                changed_input["observations"][0]["camera"]["center_m"][0] = json!(-0.39);
            }
            "tampered-report" => {
                changed["observations"][0]["report"]["camera"]["center_m"][0] = json!(-0.39);
            }
            "missing" => {
                let _ = changed["observations"].as_array_mut().unwrap().pop();
            }
            "duplicate" => {
                changed["observations"][1] = changed["observations"][0].clone();
            }
            "held-out-frame" => {
                changed["observations"][1]["frame"] = input["observations"][2]["frame"].clone();
            }
            "no-evidence" => changed["independence_evidence"] = json!(" "),
            _ => {
                let _ = changed["observations"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("report");
            }
        }
        save(&dir.0.join("input.json"), &changed_input);
        save(&dir.0.join("sheet.json"), &changed);
        run(
            &dir.0,
            &["--sheet-noise", "sheet.json", "--out", "new/point.json"],
            3,
        );
        assert!(!dir.0.join("new").exists(), "{variant}");
        assert!(
            ftx_wasm::photo::triangulate_with_camera_noise(
                &changed_input.to_string(),
                "",
                "",
                &changed.to_string(),
            )
            .is_err(),
            "{variant}"
        );
    }
}

#[test]
fn combined_camera_components_require_the_same_original_sheet_reports() {
    let dir = Scratch::new();
    let (input, _, lens, mut sheet) = sheet_camera_case(&dir.0);
    let original = sheet["observations"][0]["report"].clone();
    let mut source = original["input"].clone();
    let check = source["points"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|point| point["held_out"] == true)
        .unwrap();
    check["pixel_px"][0] = json!(check["pixel_px"][0].as_f64().unwrap() + 0.1);
    let changed = Camera::sheet_pose_report(&source).unwrap();
    assert_eq!(changed["ok"], true);
    // A CHECK corner never steers the camera. Equal numeric cameras therefore do
    // not establish that two uncertainty components refer to the same acquisition.
    assert_eq!(changed["camera"], original["camera"]);
    assert_ne!(changed, original);
    sheet["observations"][0]["report"] = changed;
    save(&dir.0.join("sheet.json"), &sheet);
    run(&dir.0, &["--sheet-noise", "sheet.json"], 0);
    run(
        &dir.0,
        &[
            "--lens-noise",
            "lens.json",
            "--sheet-noise",
            "sheet.json",
            "--out",
            "new/point.json",
        ],
        3,
    );
    assert!(!dir.0.join("new").exists());
    assert!(
        ftx_wasm::photo::triangulate_with_camera_noise(
            &input.to_string(),
            "",
            &lens.to_string(),
            &sheet.to_string(),
        )
        .is_err()
    );
}

#[test]
fn native_calibration_annotated_sheet_sources_replay_in_both_camera_components() {
    for separate_corner_noise in [false, true] {
        let dir = Scratch::new();
        let (input, click, mut lens, mut sheet) = sheet_camera_case(&dir.0);
        let calibration = if separate_corner_noise {
            let mut source = retained_brown_calibration()["input"].clone();
            let _ = source["distortion_fit"]
                .as_object_mut()
                .unwrap()
                .remove("corner_noise");
            save(&dir.0.join("calibration-input.json"), &source);
            save(
                &dir.0.join("corner-noise.json"),
                &json!({
                    "schema":"ftx-calibration-corner-noise/1",
                    "sigma_px":0.2,
                    "evidence":"Synthetic independent per-coordinate noise",
                    "independence_evidence":"Independent axes, corners and synthetic calibration frames",
                }),
            );
            let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
                .current_dir(&dir.0)
                .args([
                    "measure",
                    "calibrate-camera",
                    "calibration-input.json",
                    "--corner-noise",
                    "corner-noise.json",
                    "--out",
                    "calibration.json",
                    "--json",
                ])
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(0),
                "{} / {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let report = read(&dir.0.join("calibration.json"));
            assert_eq!(report["conditional_uncertainty"]["status"], "AVAILABLE");
            report
        } else {
            save(
                &dir.0.join("calibration.json"),
                retained_brown_calibration(),
            );
            retained_brown_calibration().clone()
        };
        assert_eq!(
            calibration["solution"]["intrinsics"],
            retained_brown_calibration()["solution"]["intrinsics"]
        );
        lens["groups"][0]["calibration"] = calibration.clone();
        let count = sheet["observations"].as_array().unwrap().len();
        for i in 0..count {
            let mut reference = sheet["observations"][i]["report"]["input"].clone();
            let _ = reference.as_object_mut().unwrap().remove("intrinsics");
            let reference_name = format!("reference-{i}.json");
            let report_name = format!("pose-{i}.json");
            save(&dir.0.join(&reference_name), &reference);
            let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
                .current_dir(&dir.0)
                .args([
                    "measure",
                    "sheet-pose",
                    "--reference",
                    &reference_name,
                    "--calibration",
                    "calibration.json",
                    "--lens-id",
                    "synthetic-shared-brown",
                    "--out",
                    &report_name,
                    "--json",
                ])
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(0),
                "{} / {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let report = read(&dir.0.join(report_name));
            assert_eq!(report["ok"], true);
            assert_eq!(
                report["intrinsics_source"]["lens_id"],
                "synthetic-shared-brown"
            );
            assert_eq!(report["intrinsics_source"]["input"], calibration["input"]);
            assert_eq!(
                report["camera"],
                sheet["observations"][i]["report"]["camera"]
            );
            lens["sheet_poses"][i]["report"] = report.clone();
            sheet["observations"][i]["report"] = report;
        }
        save(&dir.0.join("lens.json"), &lens);
        save(&dir.0.join("sheet.json"), &sheet);
        let native = run(
            &dir.0,
            &[
                "--click-noise",
                "click.json",
                "--lens-noise",
                "lens.json",
                "--sheet-noise",
                "sheet.json",
            ],
            0,
        )["result"]
            .clone();
        let portable = ftx_wasm::photo::triangulate_with_camera_noise(
            &input.to_string(),
            &click.to_string(),
            &lens.to_string(),
            &sheet.to_string(),
        )
        .unwrap();
        assert_eq!(portable, native);
        assert_eq!(
            native["conditional_lens_uncertainty"]["status"],
            "AVAILABLE"
        );
        assert_eq!(
            native["conditional_sheet_uncertainty"]["status"],
            "AVAILABLE"
        );
    }
}

#[test]
fn camera_noise_documents_are_bounded_before_parse_and_outputs_never_clobber_sources() {
    let dir = Scratch::new();
    sheet_camera_case(&dir.0);
    std::fs::hard_link(dir.0.join("sheet.json"), dir.0.join("sheet-alias.json")).unwrap();
    let original = ["input.json", "click.json", "lens.json", "sheet.json"]
        .map(|name| (name, std::fs::read(dir.0.join(name)).unwrap()));
    for name in [
        "input.json",
        "click.json",
        "lens.json",
        "sheet.json",
        "sheet-alias.json",
    ] {
        run(
            &dir.0,
            &[
                "--click-noise",
                "click.json",
                "--lens-noise",
                "lens.json",
                "--sheet-noise",
                "sheet.json",
                "--out",
                name,
            ],
            3,
        );
    }
    for (name, bytes) in original {
        assert_eq!(std::fs::read(dir.0.join(name)).unwrap(), bytes);
    }
    run(
        &dir.0,
        &["--sheet-noise", "sheet.json", "--out", "point.json"],
        0,
    );
    let point = std::fs::read(dir.0.join("point.json")).unwrap();
    run(
        &dir.0,
        &["--sheet-noise", "sheet.json", "--out", "point.json"],
        3,
    );
    assert_eq!(std::fs::read(dir.0.join("point.json")).unwrap(), point);

    // Sparse malformed JSON exercises the file-size guard before JSON parsing.
    let oversized = dir.0.join("oversized.json");
    std::fs::write(&oversized, b"{").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&oversized)
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    std::fs::write(dir.0.join("malformed.json"), b"{").unwrap();
    for flag in ["--lens-noise", "--sheet-noise"] {
        let out = run(
            &dir.0,
            &[flag, "oversized.json", "--out", "new/point.json"],
            4,
        );
        assert_eq!(out["refusal"]["code"], json!(RefusalCode::BudgetExceeded));
        assert!(!dir.0.join("new").exists());
        run(
            &dir.0,
            &[flag, "malformed.json", "--out", "new/point.json"],
            3,
        );
        assert!(!dir.0.join("new").exists());
    }
}

#[test]
fn requested_sheet_uncertainty_preserves_geometry_when_precision_is_unavailable() {
    let dir = Scratch::new();
    let (mut input, click, lens, mut sheet) = sheet_camera_case(&dir.0);
    assert_eq!(
        ftx_wasm::photo::triangulate_with_camera_noise(
            &input.to_string(),
            &click.to_string(),
            &lens.to_string(),
            "",
        )
        .unwrap(),
        ftx_wasm::photo::triangulate_with_lens_noise(
            &input.to_string(),
            &click.to_string(),
            &lens.to_string(),
        )
        .unwrap()
    );
    for frame in sheet["observations"].as_array_mut().unwrap() {
        frame["sigma_px"] = json!(1e-300);
    }
    save(&dir.0.join("sheet.json"), &sheet);
    let plain = run(&dir.0, &[], 0)["result"].clone();
    let reported = run(
        &dir.0,
        &["--sheet-noise", "sheet.json", "--out", "unavailable.json"],
        2,
    )["result"]
        .clone();
    assert_eq!(reported["status"], "ACCEPTED");
    assert_eq!(reported["point_m"], plain["point_m"]);
    let uncertainty = &reported["conditional_sheet_uncertainty"];
    assert_eq!(uncertainty["status"], "UNAVAILABLE");
    assert!(uncertainty["covariance_m2"].is_null());
    assert!(uncertainty["sigma_m"].is_null());
    assert!(uncertainty["sensitivities"].as_array().unwrap().is_empty());
    assert_eq!(reported, read(&dir.0.join("unavailable.json")));
    assert_eq!(
        reported,
        ftx_wasm::photo::triangulate_with_camera_noise(
            &input.to_string(),
            "",
            "",
            &sheet.to_string(),
        )
        .unwrap()
    );
    input["observations"][2]["pixel_px"][0] = json!(50.0);
    save(&dir.0.join("input.json"), &input);
    let rejected = run(&dir.0, &["--sheet-noise", "sheet.json"], 2);
    assert_eq!(rejected["result"]["status"], "REJECTED");
    assert!(rejected["result"]["point_m"].is_null());
    assert_eq!(
        rejected["result"]["conditional_sheet_uncertainty"]["status"],
        "UNAVAILABLE"
    );
    assert_eq!(
        rejected["result"],
        ftx_wasm::photo::triangulate_with_camera_noise(
            &input.to_string(),
            "",
            "",
            &sheet.to_string(),
        )
        .unwrap()
    );
    for bad in [" ".to_owned(), "{}".to_owned(), format!("\u{feff}{sheet}")] {
        assert!(
            ftx_wasm::photo::triangulate_with_camera_noise(&input.to_string(), "", "", &bad,)
                .is_err()
        );
    }
    let oversized = " ".repeat(16 * 1024 * 1024 + 1);
    assert_eq!(
        ftx_wasm::photo::triangulate_with_camera_noise(&input.to_string(), "", "", &oversized)
            .unwrap_err()
            .code,
        RefusalCode::BudgetExceeded
    );
}

fn reference_scale_case(dir: &Path) -> (Value, Value, Value, Value, Value) {
    let (input, click, lens, sheet) = sheet_camera_case(dir);
    let scale = json!({
        "schema":"ftx-triangulation-reference-scale-noise/1",
        "reference_id":"synthetic-stationary-camera-noise-sheet",
        "sigma_fraction":0.002,
        "evidence":"Synthetic 0.2-percent common dimensional error about the fixed source sheet origin",
        "sheet_poses":lens["sheet_poses"],
    });
    save(&dir.join("reference-scale.json"), &scale);
    (input, click, lens, sheet, scale)
}

#[test]
fn reference_scale_replays_native_and_portable_without_changing_other_components() {
    let dir = Scratch::new();
    let (input, click, lens, sheet, scale) = reference_scale_case(&dir.0);
    let plain = run(&dir.0, &[], 0)["result"].clone();
    let scale_only = run(
        &dir.0,
        &["--reference-scale-noise", "reference-scale.json"],
        0,
    )["result"]
        .clone();
    let previous = ftx_wasm::photo::triangulate_with_camera_noise(
        &input.to_string(),
        &click.to_string(),
        &lens.to_string(),
        &sheet.to_string(),
    )
    .unwrap();
    let all = run(
        &dir.0,
        &[
            "--click-noise", "click.json",
            "--lens-noise", "lens.json",
            "--sheet-noise", "sheet.json",
            "--reference-scale-noise", "reference-scale.json",
            "--out", "scale-result.json",
        ],
        0,
    )["result"]
        .clone();
    assert_eq!(read(&dir.0.join("scale-result.json")), all);
    assert_eq!(
        all,
        ftx_wasm::photo::triangulate_with_reference_scale_noise(
            &input.to_string(),
            &click.to_string(),
            &lens.to_string(),
            &sheet.to_string(),
            &scale.to_string(),
        )
        .unwrap()
    );
    assert_eq!(
        scale_only,
        ftx_wasm::photo::triangulate_with_reference_scale_noise(
            &input.to_string(), "", "", "", &scale.to_string(),
        )
        .unwrap()
    );
    assert_eq!(
        previous,
        ftx_wasm::photo::triangulate_with_reference_scale_noise(
            &input.to_string(),
            &click.to_string(),
            &lens.to_string(),
            &sheet.to_string(),
            "",
        )
        .unwrap()
    );
    for (mut augmented, original) in [(scale_only.clone(), plain), (all.clone(), previous)] {
        augmented.as_object_mut().unwrap().remove("conditional_reference_scale_uncertainty");
        assert_eq!(augmented, original);
    }
    let component = &all["conditional_reference_scale_uncertainty"];
    assert_eq!(component, &scale_only["conditional_reference_scale_uncertainty"]);
    assert_eq!(component["status"], "AVAILABLE");
    assert_eq!(component["input"], scale);
    assert_eq!(component["reference_origin_m"], json!([0.,0.,0.]));
    for (actual, expected) in component["sigma_m"].as_array().unwrap().iter().zip([0.0001, 0.0008, 0.0025]) {
        assert!((actual.as_f64().unwrap() - expected).abs() < 1e-10);
    }
    assert!(all.get("total_uncertainty").is_none());
    assert!(all.get("records").is_none());
    let human = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir.0)
        .args(["measure", "triangulate", "input.json", "--reference-scale-noise", "reference-scale.json"])
        .output()
        .unwrap();
    assert!(human.status.success());
    assert!(String::from_utf8(human.stdout).unwrap().contains(
        "CONDITIONAL REFERENCE-SCALE UNCERTAINTY: AVAILABLE"
    ));
    if let Ok(directory) = std::env::var("FTX_TRIANGULATION_CAMERA_FIXTURES") {
        use std::io::Write;
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        for (name, value) in [
            ("reference-scale-noise.json", &scale),
            ("reference-scale-result.json", &scale_only),
            ("reference-scale-all-result.json", &all),
        ] {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(&serde_json::to_vec_pretty(value).unwrap())
                .unwrap();
        }
    }
}

#[test]
fn reference_scale_refuses_different_original_acquisitions_in_combined_requests() {
    let dir = Scratch::new();
    let (input, _, lens, sheet, mut scale) = reference_scale_case(&dir.0);
    let original = scale["sheet_poses"][0]["report"].clone();
    let mut source = original["input"].clone();
    let check = source["points"].as_array_mut().unwrap().iter_mut()
        .find(|point| point["held_out"] == true).unwrap();
    check["pixel_px"][0] = json!(check["pixel_px"][0].as_f64().unwrap() + 0.1);
    let different = Camera::sheet_pose_report(&source).unwrap();
    assert_eq!(different["ok"], true);
    assert_eq!(different["camera"], original["camera"]);
    scale["sheet_poses"][0]["report"] = different;
    save(&dir.0.join("reference-scale.json"), &scale);
    run(&dir.0, &["--reference-scale-noise", "reference-scale.json"], 0);
    for (flag, name, lenses, sheets) in [
        ("--lens-noise", "lens.json", lens.to_string(), String::new()),
        ("--sheet-noise", "sheet.json", String::new(), sheet.to_string()),
    ] {
        run(
            &dir.0,
            &["--reference-scale-noise", "reference-scale.json", flag, name, "--out", "new/result.json"],
            3,
        );
        assert!(!dir.0.join("new").exists());
        assert!(ftx_wasm::photo::triangulate_with_reference_scale_noise(
            &input.to_string(), "", &lenses, &sheets, &scale.to_string(),
        ).is_err());
    }
}

#[test]
fn reference_scale_preserves_unavailable_precision_bounds_and_no_clobber() {
    let dir = Scratch::new();
    let (input, _, _, _, mut scale) = reference_scale_case(&dir.0);
    let original = std::fs::read(dir.0.join("reference-scale.json")).unwrap();
    run(
        &dir.0,
        &["--reference-scale-noise", "reference-scale.json", "--out", "reference-scale.json"],
        3,
    );
    assert_eq!(std::fs::read(dir.0.join("reference-scale.json")).unwrap(), original);
    scale["sigma_fraction"] = json!(1e-200);
    save(&dir.0.join("reference-scale.json"), &scale);
    let unavailable = run(
        &dir.0,
        &["--reference-scale-noise", "reference-scale.json"],
        2,
    )["result"].clone();
    assert_eq!(unavailable["status"], "ACCEPTED");
    assert!(unavailable["point_m"].is_array());
    let component = &unavailable["conditional_reference_scale_uncertainty"];
    assert_eq!(component["status"], "UNAVAILABLE");
    for field in ["covariance_m2", "sigma_m", "jacobian_m_per_fraction"] {
        assert!(component[field].is_null());
    }
    assert_eq!(
        unavailable,
        ftx_wasm::photo::triangulate_with_reference_scale_noise(
            &input.to_string(), "", "", "", &scale.to_string(),
        ).unwrap()
    );
    for bad in [" ", "{}"] {
        assert!(ftx_wasm::photo::triangulate_with_reference_scale_noise(
            &input.to_string(), "", "", "", bad,
        ).is_err());
    }
    let oversized = " ".repeat(16 * 1024 * 1024 + 1);
    assert_eq!(
        ftx_wasm::photo::triangulate_with_reference_scale_noise(
            &input.to_string(), "", "", "", &oversized,
        ).unwrap_err().code,
        RefusalCode::BudgetExceeded
    );
    std::fs::write(dir.0.join("oversized.json"), oversized).unwrap();
    run(
        &dir.0,
        &["--reference-scale-noise", "oversized.json", "--out", "new/result.json"],
        4,
    );
    assert!(!dir.0.join("new").exists());
}
