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
