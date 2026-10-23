//! Synthetic sheet-plane answers and the actual shared photo engines; no real-camera claim.
use ftx_core::{Mat3, Vec2, Vec3, receipt::sha256_hex};
use ftx_measure::photo::{
    PhotoOptions,
    acquisition::analyze_charuco_image,
    analyze_plain_sheet,
    intrinsics::{CalibrationInput, calibrate_camera},
    measure_photo,
};
use ftx_trace::raster::{
    Gray,
    image::{encode_png_gray, load_photo_with_limit, load_png},
};
use ftx_wasm::photo::{CameraCalibration, Photo, camera_calibration};
use serde_json::{Value, json};

const SHEET: &str = include_str!("../../../web/calibration/calib-sheet-letter.json");
const SHEET_PNG: &[u8] = include_bytes!("../../../web/calibration/calib-sheet-letter.png");

fn fixture() -> Vec<u8> {
    let mut image = Gray::filled(720, 960, 90);
    for y in 0..image.h {
        for x in 0..image.w {
            let dx =
                ((x as f64 + 0.5).min(150.2 + 215.9 * 1.5) - (x as f64 - 0.5).max(150.2)).max(0.0);
            let dy =
                ((y as f64 + 0.5).min(170.4 + 279.4 * 1.5) - (y as f64 - 0.5).max(170.4)).max(0.0);
            image.data[y * image.w + x] = (90.0 + 152.0 * dx * dy).round() as u8;
        }
    }
    encode_png_gray(&image)
}

#[test]
fn photo_uses_native_unrounded_scale_records_and_exact_preview_pixels() {
    let bytes = fixture();
    let photo = Photo::new(&bytes, "letter").unwrap();
    let (image, receipt) = load_photo_with_limit(&bytes, 16_000_000).unwrap();
    let native = analyze_plain_sheet(&image.data, image.w, image.h, "letter").unwrap();
    let mut report = photo.report();
    assert_eq!(report["image_input"], receipt);
    assert_eq!(receipt["sha256"], sha256_hex(&bytes));
    report.as_object_mut().unwrap().remove("image_input");
    assert_eq!(&report, native.report());
    assert!(report["ok"].as_bool().unwrap());
    assert!((report["mm_per_px"].as_f64().unwrap() - 2.0 / 3.0).abs() < 0.002);
    assert_eq!(load_png(&photo.preview().unwrap()).unwrap(), image);
    let points = json!({"across_back":[[30,400],[630,400]]});
    let actual = photo.measure(&points.to_string(), "{}").unwrap();
    let expected = measure_photo(&native, &points, &PhotoOptions::default()).unwrap();
    assert_eq!(actual["records"], expected["records"]);
    assert!((actual["records"][0]["value_mm"].as_f64().unwrap() - 400.0).abs() < 1.0);
    assert!(actual["records"][0]["sigma_mm"].as_f64().unwrap() > 0.0);
    assert_eq!(actual["records"][0]["provenance"], "PHOTO_ESTIMATED");
    assert_eq!(actual["schema"], "fabric-measurements/1");
    assert_eq!(actual["taken_on"], serde_json::Value::Null);
    assert_eq!(actual, photo.measure(&points.to_string(), "{}").unwrap());
    assert!(
        photo
            .measure(r#"{"across_back":[[30,400],[730,400]]}"#, "{}")
            .is_err()
    );
    assert!(
        photo
            .measure(&points.to_string(), r#"{"bootstrap":19}"#)
            .is_err()
    );
    assert!(
        photo
            .measure(&points.to_string(), r#"{"unknown":1}"#)
            .is_err()
    );
}

#[test]
fn jpeg_orientation_and_preview_share_the_native_evidence_frame() {
    let bytes = include_bytes!("../../../tests/fixtures/photo/plain-letter-exif6.jpg");
    let (image, receipt) = load_photo_with_limit(bytes, 16_000_000).unwrap();
    let photo = Photo::new(bytes, "letter").unwrap();
    assert_eq!(receipt["exif_orientation"], 6);
    assert_eq!(photo.report()["image_input"], receipt);
    assert_eq!(load_png(&photo.preview().unwrap()).unwrap(), image);
    let native = analyze_plain_sheet(&image.data, image.w, image.h, "letter").unwrap();
    let points = json!({"across_back":[[30,400],[530,400]]});
    assert_eq!(
        photo.measure(&points.to_string(), "{}").unwrap()["records"],
        measure_photo(&native, &points, &PhotoOptions::default()).unwrap()["records"]
    );
}

#[test]
fn rejected_calibration_and_images_publish_no_measurements() {
    let photo = Photo::new(&fixture(), "a4").unwrap();
    assert_eq!(photo.report()["ok"], false);
    assert!(
        photo
            .measure(r#"{"across_back":[[30,400],[630,400]]}"#, "{}")
            .is_err()
    );
    assert!(Photo::new(b"not an image", "auto").is_err());
    assert!(Photo::new(&fixture(), "unknown").is_err());
    assert!(load_photo_with_limit(&fixture(), 10).is_err());
    let blank = Photo::new(&encode_png_gray(&Gray::filled(100, 100, 90)), "auto").unwrap();
    assert_eq!(blank.report()["ok"], false);
    assert!(
        blank
            .measure(r#"{"test":[[20,20],[50,50]]}"#, "{}")
            .is_err()
    );
}

#[test]
fn charuco_photo_uses_native_detection_diagnostics_and_original_sheet_identity() {
    let photo = Photo::with_reference(SHEET_PNG, SHEET, "", "{}").unwrap();
    let (image, _) = load_photo_with_limit(SHEET_PNG, 16_000_000).unwrap();
    let sheet: Value = serde_json::from_str(SHEET).unwrap();
    let (native, detection) =
        analyze_charuco_image(&image.data, image.w, image.h, &sheet, None).unwrap();
    let report = photo.report();
    assert_eq!(report["ok"], true);
    assert_eq!(report["coordinate_source"], "detected_charuco");
    assert_eq!(report["inliers"], 35);
    assert_eq!(
        report["blur_sigma_px_estimate"],
        native.report()["blur_sigma_px_estimate"]
    );
    assert!(report.get("lens_input").is_none());
    let actual = &report["image_input"]["charuco_detection"];
    assert_eq!(actual["implementation"]["backend"], "rust_charuco");
    assert_eq!(actual["reference_input"], detection["reference_input"]);
    assert_eq!(actual["sheet"], sheet);
    assert_eq!(actual["sheet_sha256"], sha256_hex(SHEET.as_bytes()));
    assert_eq!(load_png(&photo.preview().unwrap()).unwrap(), image);
    let pixel = |x: f64, y: f64| {
        [
            135.0 - 0.5 + x * 2079.0 / 176.0,
            94.0 - 0.5 + y * 1559.0 / 132.0,
        ]
    };
    let points = json!({"across_back":[pixel(44., 44.), pixel(132., 44.)]});
    let measured = photo.measure(&points.to_string(), "{}").unwrap();
    assert_eq!(
        measured["records"],
        measure_photo(&native, &points, &PhotoOptions::default()).unwrap()["records"]
    );
    assert!((measured["records"][0]["value_mm"].as_f64().unwrap() - 88.).abs() < 0.2);
    assert_eq!(measured["records"][0]["provenance"], "PHOTO_ESTIMATED");
    assert_eq!(measured["photo"]["image_input"], report["image_input"]);
    assert_eq!(measured["photo"]["coordinate_source"], "detected_charuco");
    assert!(Photo::with_reference(SHEET_PNG, "{}", "", "{}").is_err());
    assert!(Photo::with_reference(SHEET_PNG, SHEET, "", r#"{"rotation_cw_deg":0}"#).is_err());
    assert!(Photo::with_reference(SHEET_PNG, SHEET, "", r#"{"paper":"a4"}"#).is_err());
    assert!(Photo::with_reference(SHEET_PNG, &" ".repeat(256 * 1024 + 1), "", "{}").is_err());
}

// Same independent analytic Brown lens used by the calibration authority's known-answer tests.
// The measurement image below is a separate raster acquisition, not a fitted calibration view.
fn retained_brown() -> &'static Value {
    static REPORT: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    REPORT.get_or_init(|| {
        let controls: Vec<_> = (0..48).map(|i| json!({"id":format!("p{i:02}"),
            "sheet_xz_m":[(i % 8) as f64 * 0.06 - 0.21, (i / 8) as f64 * 0.06 - 0.15]})).collect();
        let base = Mat3::from_rows([[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]]);
        let poses = [(0.35, 0.12, -0.18, -0.10, 0.80), (-0.45, 0.3, 0.16, 0.13, 0.75),
            (0.2, -0.55, -0.12, 0.18, 0.85), (-0.3, -0.25, 0.23, -0.14, 0.8),
            (0.5, -0.4, 0.03, -0.04, 0.66), (-0.2, 0.5, -0.24, 0.06, 0.9)];
        let views: Vec<_> = poses.iter().enumerate().map(|(i, &(a, b, tx, ty, tz))| {
            let r = Mat3::rotation(Vec3::new(1., 0., 0.), a) * Mat3::rotation(Vec3::new(0., 1., 0.), b) * base;
            let corners: Vec<_> = controls.iter().enumerate().map(|(j, p)| {
                let q = r.mul_vec(Vec3::new(p["sheet_xz_m"][0].as_f64().unwrap(), 0., p["sheet_xz_m"][1].as_f64().unwrap())) + Vec3::new(tx, ty, tz);
                json!({"id":p["id"],"pixel_px":brown_project(Vec2::new(q.x / q.z, q.y / q.z)),
                    "partition":if [0,7,40,47].contains(&j) {"held_out"} else {"fit"}})
            }).collect();
            json!({"frame":{"stream":"synthetic-brown","timestamp_us":i * 100000},
                "partition":if i == 5 {"held_out"} else {"fit"},"corners":corners})
        }).collect();
        let input: CalibrationInput = serde_json::from_value(json!({
            "schema":"ftx-camera-calibration-input/1","image_model":"brown5_zero_skew",
            "lens_id":"synthetic-brown","pinhole_evidence":"",
            "distortion_fit":{"model_evidence":"Independent analytic synthetic lens; not a real camera",
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

fn brown_project(p: Vec2) -> [f64; 2] {
    let (x, y) = (p.x, p.y);
    let r2 = x * x + y * y;
    let radial = 1.0 + 0.12 * r2 - 0.035 * r2 * r2 + 0.009 * r2 * r2 * r2;
    [
        1050. * (x * radial + 0.004 * x * y - 0.0015 * (r2 + 2. * x * x)) + 790.,
        1000. * (y * radial + 0.002 * (r2 + 2. * y * y) - 0.003 * x * y) + 610.,
    ]
}

fn brown_photo() -> Vec<u8> {
    let board = load_png(SHEET_PNG).unwrap();
    let mut photo = Gray::filled(1600, 1200, 80);
    for y in 0..photo.h {
        for x in 0..photo.w {
            let (xd, yd) = ((x as f64 - 790.) / 1050., (y as f64 - 610.) / 1000.);
            let (mut u, mut v) = (xd, yd);
            // Independent fixed-point inverse, not the candidate camera's Newton solver.
            for _ in 0..35 {
                let r2 = u * u + v * v;
                let radial = 1. + 0.12 * r2 - 0.035 * r2 * r2 + 0.009 * r2 * r2 * r2;
                u = (xd - 0.004 * u * v + 0.0015 * (r2 + 2. * u * u)) / radial;
                v = (yd - 0.002 * (r2 + 2. * v * v) + 0.003 * u * v) / radial;
            }
            let (bx, by) = (
                (1050. * u + 790. - 400.) / 3.,
                (1000. * v + 610. - 300.) / 3.,
            );
            if !(0. ..176.).contains(&bx) || !(0. ..132.).contains(&by) {
                continue;
            }
            let (px, py) = (134.5 + bx * 2079. / 176., 93.5 + by * 1559. / 132.);
            let (ix, iy) = (px.floor() as usize, py.floor() as usize);
            let (dx, dy) = (px - ix as f64, py - iy as f64);
            let sample = |xx, yy| f64::from(board.at(xx, yy)[0]);
            let value = (1. - dy) * ((1. - dx) * sample(ix, iy) + dx * sample(ix + 1, iy))
                + dy * ((1. - dx) * sample(ix, iy + 1) + dx * sample(ix + 1, iy + 1));
            photo.data[y * photo.w + x] = value.round() as u8;
        }
    }
    encode_png_gray(&photo)
}

#[test]
fn retained_camera_uncertainty_reaches_browser_photo_measurements_without_resampling() {
    let bytes = brown_photo();
    let source = serde_json::to_string_pretty(retained_brown()).unwrap();
    let capture = json!({"capture_lens_id":"synthetic-brown","rotation_cw_deg":0,
        "independent_calibration_evidence":"Separate synthetic raster; calibration uses independent analytic observations"}).to_string();
    let photo = Photo::with_reference(&bytes, SHEET, &source, &capture).unwrap();
    let report = photo.report();
    assert_eq!(report["ok"], true);
    assert_eq!(report["lens_input"]["recomputed"], true);
    assert_eq!(
        report["lens_input"]["sha256"],
        sha256_hex(source.as_bytes())
    );
    assert_eq!(&report["lens_input"]["calibration"], retained_brown());
    assert!(report["intrinsic_uncertainty"].is_object());
    assert_eq!(
        report["image_input"]["charuco_detection"]["implementation"]["uses_lens_intrinsics"],
        true
    );
    let (image, _) = load_photo_with_limit(&bytes, 16_000_000).unwrap();
    assert_eq!(
        report["image_input"]["pixels_bgr_sha256"],
        sha256_hex(&image.data)
    );
    assert_eq!(load_png(&photo.preview().unwrap()).unwrap(), image);
    let point = |x: f64, y: f64| {
        brown_project(Vec2::new(
            (400. + 3. * x - 790.) / 1050.,
            (300. + 3. * y - 610.) / 1000.,
        ))
    };
    let points = json!({"across_back":[point(30., 20.),point(230., 20.)]}).to_string();
    let measured = photo.measure(&points, "{}").unwrap();
    assert!((measured["records"][0]["value_mm"].as_f64().unwrap() - 200.).abs() < 0.5);
    assert!(
        measured["records"][0]["sigma_terms_mm"]["intrinsics"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    assert_eq!(
        measured["photo"]["intrinsic_uncertainty"],
        report["intrinsic_uncertainty"]
    );
    assert_eq!(measured["photo"]["lens_input"], report["lens_input"]);
    assert_eq!(measured["records"][0]["provenance"], "PHOTO_ESTIMATED");
    assert_eq!(measured["taken_on"], Value::Null);
    // Optional exact native replay inputs for the existing compiled-WASM/browser test seam.
    // The caller supplies a fresh directory; retained artifacts are never overwritten.
    if let Ok(directory) = std::env::var("FTX_PHOTO_WEB_FIXTURES") {
        use std::io::Write;
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        for (name, data) in [
            ("calibrated-photo.png", bytes.clone()),
            ("calibrated-sheet.json", SHEET.as_bytes().to_vec()),
            ("calibrated-lens.json", source.as_bytes().to_vec()),
            ("calibrated-capture.json", capture.as_bytes().to_vec()),
            ("calibrated-points.json", points.as_bytes().to_vec()),
            (
                "calibrated-scale.json",
                serde_json::to_vec(&report).unwrap(),
            ),
            (
                "calibrated-measurements.json",
                serde_json::to_vec(&measured).unwrap(),
            ),
        ] {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(&data)
                .unwrap();
        }
    }
    // A deliberately tampered retained report or a missing independence assertion is refused.
    let mut tampered = retained_brown().clone();
    tampered["uncertainty"]["covariance"][0][0] = json!(123.0);
    assert!(Photo::with_reference(&bytes, SHEET, &tampered.to_string(), &capture).is_err());
    assert!(
        Photo::with_reference(
            &bytes,
            SHEET,
            &source,
            r#"{"capture_lens_id":"synthetic-brown","rotation_cw_deg":0}"#
        )
        .is_err()
    );
    // Encoded and decoded identities independently prohibit reusing a calibration photograph.
    for view in [
        json!({"sha256":sha256_hex(&bytes)}),
        json!({"pixels_bgr_sha256":sha256_hex(&image.data),"sha256":"other-container"}),
    ] {
        let mut reused = retained_brown().clone();
        reused["image_acquisition"] = json!({"views":[view]});
        let error = Photo::with_reference(&bytes, SHEET, &reused.to_string(), &capture)
            .err()
            .unwrap();
        assert!(error.to_string().contains("reuses a calibration image"));
    }
}

#[test]
fn calibrated_photo_requires_explicit_matching_capture_without_silent_fallback() {
    let source = json!({"schema":"ftx-photo-lens/1","lens_id":"synthetic-identity",
        "image_size_px":[2550,3300],"rotation_cw_deg":0,"calibration_evidence":"Synthetic software fixture only",
        "intrinsics":{"focal_px":[3000.,3000.],"principal_px":[1275.,1650.]}});
    let capture = json!({"capture_lens_id":"synthetic-identity","rotation_cw_deg":0});
    let photo =
        Photo::with_reference(SHEET_PNG, SHEET, &source.to_string(), &capture.to_string()).unwrap();
    assert_eq!(photo.report()["ok"], true);
    assert_eq!(photo.report()["lens_input"]["recomputed"], false);
    assert!(photo.report().get("intrinsic_uncertainty").is_none());
    assert!(photo.report().get("homography_px_to_board_mm").is_none());
    for bad in [
        json!({}),
        json!({"capture_lens_id":"other","rotation_cw_deg":0}),
        json!({"capture_lens_id":"synthetic-identity","rotation_cw_deg":90}),
        json!({"capture_lens_id":"synthetic-identity","rotation_cw_deg":45}),
        json!({"capture_lens_id":"synthetic-identity","rotation_cw_deg":0,"independent_calibration_evidence":"Cannot invent covariance"}),
    ] {
        assert!(
            Photo::with_reference(SHEET_PNG, SHEET, &source.to_string(), &bad.to_string()).is_err()
        );
    }
    let mut mismatch = source;
    mismatch["image_size_px"] = json!([1600, 1200]);
    assert!(
        Photo::with_reference(
            SHEET_PNG,
            SHEET,
            &mismatch.to_string(),
            &capture.to_string()
        )
        .is_err()
    );
}

#[test]
fn portable_coordinate_calibration_preserves_native_answers_source_and_rejection() {
    let source = include_str!("../../../docs/examples/camera-calibration.json");
    let report = camera_calibration(source, "").unwrap();
    let input: CalibrationInput = serde_json::from_str(source).unwrap();
    let native = serde_json::to_value(calibrate_camera(&input).unwrap()).unwrap();
    assert_eq!(report["status"], "ACCEPTED");
    assert_eq!(report["solution"], native["solution"]);
    assert_eq!(report["checks"], native["checks"]);
    assert_eq!(report["input_sha256"], sha256_hex(source.as_bytes()));
    let focal = &report["solution"]["intrinsics"]["focal_px"];
    assert!((focal[0].as_f64().unwrap() - 960.).abs() < 1e-6);
    assert!((focal[1].as_f64().unwrap() - 1000.).abs() < 1e-6);
    let mut bad: Value = serde_json::from_str(source).unwrap();
    bad["views"][3]["corners"][1]["pixel_px"] = json!([20., 20.]);
    let rejected = camera_calibration(&bad.to_string(), "").unwrap();
    assert_eq!(rejected["status"], "REJECTED");
    assert!(rejected["solution"].is_null());
    assert!(!rejected["flags"].as_array().unwrap().is_empty());
    assert!(camera_calibration("{}", "").is_err());
    assert!(camera_calibration(source, "{}").is_err());
    assert!(camera_calibration(&" ".repeat(16 * 1024 * 1024 + 1), "").is_err());
}

fn corner_noise() -> String {
    serde_json::to_string_pretty(&json!({"schema":"ftx-calibration-corner-noise/1",
        "sigma_px":0.2,"evidence":"Declared synthetic independent per-coordinate noise",
        "independence_evidence":"Separate synthetic observations; not empirical camera precision"}))
    .unwrap()
}

#[test]
fn portable_brown_calibration_retains_declared_noise_and_hands_off_to_photos() {
    let source = serde_json::to_string_pretty(&retained_brown()["input"]).unwrap();
    let inline = camera_calibration(&source, "").unwrap();
    assert_eq!(inline["solution"], retained_brown()["solution"]);
    assert_eq!(inline["uncertainty"], retained_brown()["uncertainty"]);
    let noise = corner_noise();
    assert!(camera_calibration(&source, &noise).is_err());
    let mut coordinates = retained_brown()["input"].clone();
    coordinates["distortion_fit"]
        .as_object_mut()
        .unwrap()
        .remove("corner_noise");
    let original = serde_json::to_string_pretty(&coordinates).unwrap();
    let separate = camera_calibration(&original, &noise).unwrap();
    assert_eq!(separate["input_sha256"], sha256_hex(original.as_bytes()));
    assert_eq!(
        separate["corner_noise_input"]["sha256"],
        sha256_hex(noise.as_bytes())
    );
    assert_eq!(
        separate["corner_noise_input"]["input"],
        serde_json::from_str::<Value>(&noise).unwrap()
    );
    assert_eq!(separate["conditional_uncertainty"]["status"], "AVAILABLE");
    assert_eq!(
        separate["conditional_uncertainty"]["estimate"]["covariance"],
        inline["uncertainty"]["covariance"]
    );
    let bytes = brown_photo();
    let capture = json!({"capture_lens_id":"synthetic-brown","rotation_cw_deg":0,
        "independent_calibration_evidence":"Separate synthetic raster and analytic calibration observations"}).to_string();
    let photo = Photo::with_reference(&bytes, SHEET, &separate.to_string(), &capture).unwrap();
    assert_eq!(photo.report()["ok"], true);
    let point = |x: f64, y: f64| {
        brown_project(Vec2::new(
            (400. + 3. * x - 790.) / 1050.,
            (300. + 3. * y - 610.) / 1000.,
        ))
    };
    let points = json!({"across_back":[point(30.,20.),point(230.,20.)]}).to_string();
    let measured = photo.measure(&points, "{}").unwrap();
    assert!((measured["records"][0]["value_mm"].as_f64().unwrap() - 200.).abs() < 0.5);
    assert!(
        measured["records"][0]["sigma_terms_mm"]["intrinsics"]
            .as_f64()
            .unwrap()
            > 0.
    );
    assert_eq!(measured["photo"]["lens_input"]["recomputed"], true);
}

fn camera_manifest() -> Value {
    json!({"schema":"ftx-camera-calibration-images/1","image_model":"pinhole_zero_skew",
        "lens_id":"synthetic-pinhole","pinhole_evidence":"Independent pinhole image renderer; no lens distortion",
        "reference_evidence":"Synthetic sheet with exact 22 mm squares; no real photograph",
        "image_size_px":[1600,1200],"rotation_cw_deg":0,"held_out_corner_ids":[0,6,28,34],
        "views":(0..4).map(|i|json!({"image":format!("view{i}.png"),
            "frame":{"stream":"synthetic-burst","timestamp_us":i*100_000},
            "partition":if i==3 {"held_out"} else {"fit"}})).collect::<Vec<_>>()})
}

// Independent ray/plane renderer. The page-to-board pixel convention is fixed by the public
// sheet sidecar; neither the candidate camera projection nor a fitted homography renders pixels.
fn pinhole_photo(index: usize) -> (Vec<u8>, [[f64; 2]; 2]) {
    let angles: [(f64, f64, f64); 5] = [
        (25., -17., 4.),
        (-23., 24., -7.),
        (16., 28., 12.),
        (-27., -18., 6.),
        (18., -26., 9.),
    ];
    let (x, y, z) = angles[index];
    let r = Mat3::rotation(Vec3::new(0., 0., 1.), z.to_radians())
        * Mat3::rotation(Vec3::new(0., 1., 0.), y.to_radians())
        * Mat3::rotation(Vec3::new(1., 0., 0.), x.to_radians());
    let t = Vec3::new(-0.085, -0.063, 0.40 + index as f64 * 0.02);
    let inverse = r.transpose();
    let origin = inverse.mul_vec(t);
    let board = load_png(SHEET_PNG).unwrap();
    let mut photo = Gray::filled(1600, 1200, 85);
    for y in 0..photo.h {
        for x in 0..photo.w {
            let ray = inverse.mul_vec(Vec3::new(
                (x as f64 - 800.) / 1400.,
                (y as f64 - 600.) / 1380.,
                1.,
            ));
            let p = ray * (origin.z / ray.z) - origin;
            let px = 134.5 + p.x * 2079. / 0.176;
            let py = 93.5 + p.y * 1559. / 0.132;
            if px < 0. || py < 0. || px >= (board.w - 1) as f64 || py >= (board.h - 1) as f64 {
                continue;
            }
            let (ix, iy) = (px.floor() as usize, py.floor() as usize);
            let (dx, dy) = (px - ix as f64, py - iy as f64);
            let sample = |xx, yy| f64::from(board.at(xx, yy)[0]);
            let value = (1. - dy) * ((1. - dx) * sample(ix, iy) + dx * sample(ix + 1, iy))
                + dy * ((1. - dx) * sample(ix, iy + 1) + dx * sample(ix + 1, iy + 1));
            photo.data[y * photo.w + x] = value.round() as u8;
        }
    }
    let point = |x, y| {
        let q = r.mul_vec(Vec3::new(x, y, 0.)) + t;
        [1400. * q.x / q.z + 800., 1380. * q.y / q.z + 600.]
    };
    (
        encode_png_gray(&photo),
        [point(0.044, 0.044), point(0.132, 0.044)],
    )
}

#[test]
fn local_images_calibrate_through_shared_acquisition_and_replay_into_photo_measurement() {
    use ftx_measure::photo::{
        calibration_images::Acquisition, detect_charuco, intrinsics::calibrate_document,
    };
    let manifest = serde_json::to_string_pretty(&camera_manifest()).unwrap();
    let mut portable = CameraCalibration::new(&manifest, SHEET, "").unwrap();
    let mut native = Acquisition::new(manifest.as_bytes(), SHEET.as_bytes()).unwrap();
    assert_eq!(
        portable.image_ids(),
        ["view0.png", "view1.png", "view2.png", "view3.png"]
    );
    assert!(portable.finish().is_err());
    assert!(
        portable
            .add_image("view1.png", b"do not decode out-of-order input")
            .is_err()
    );
    let mut images = Vec::new();
    for (i, id) in portable.image_ids().iter().enumerate() {
        let (bytes, _) = pinhole_photo(i);
        let receipt = portable.add_image(id, &bytes).unwrap();
        let (image, decoded) = load_photo_with_limit(&bytes, 16_000_000).unwrap();
        let shared = native
            .add_image(id, &image.data, [image.w, image.h], decoded, |sheet| {
                detect_charuco(&image.data, image.w, image.h, sheet, None)
            })
            .unwrap();
        assert_eq!(receipt, shared);
        assert_eq!(receipt["sha256"], sha256_hex(&bytes));
        assert_eq!(receipt["pixels_bgr_sha256"], sha256_hex(&image.data));
        assert_eq!(receipt["image"], *id);
        assert_eq!(receipt["held_out_corners"], 4);
        assert_eq!(receipt["detector"]["uses_lens_intrinsics"], false);
        if i == 0 {
            let duplicate = portable.add_image("view1.png", &bytes).unwrap_err();
            assert!(
                duplicate
                    .message
                    .contains("duplicate decoded calibration image")
            );
        }
        images.push(bytes);
    }
    let report = portable.finish().unwrap();
    let (input, acquisition) = native.finish().unwrap();
    let (_, mut expected) = calibrate_document(&serde_json::to_vec(&input).unwrap(), None).unwrap();
    expected["image_acquisition"] = acquisition;
    assert_eq!(report, expected);
    assert_eq!(report["status"], "ACCEPTED");
    assert_eq!(
        report["image_acquisition"]["manifest_sha256"],
        sha256_hex(manifest.as_bytes())
    );
    assert_eq!(
        report["image_acquisition"]["sheet_sha256"],
        sha256_hex(SHEET.as_bytes())
    );
    assert_eq!(report["input"]["views"][3]["partition"], "held_out");
    for view in report["input"]["views"].as_array().unwrap() {
        assert_eq!(
            view["corners"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["partition"] == "held_out")
                .count(),
            4
        );
    }
    let k = &report["solution"]["intrinsics"];
    for (field, want, tolerance) in [
        ("focal_px", [1400., 1380.], 10.),
        ("principal_px", [800., 600.], 5.),
    ] {
        for j in 0..2 {
            assert!(
                (k[field][j].as_f64().unwrap() - want[j]).abs() < tolerance,
                "{k}"
            );
        }
    }
    assert_eq!(portable.finish().unwrap(), report);
    assert!(portable.add_image("view0.png", &images[0]).is_err());
    let (independent, points) = pinhole_photo(4);
    let photo = Photo::with_reference(
        &independent,
        SHEET,
        &report.to_string(),
        r#"{"capture_lens_id":"synthetic-pinhole","rotation_cw_deg":0}"#,
    )
    .unwrap();
    assert_eq!(photo.report()["ok"], true);
    let measured = photo
        .measure(&json!({"across_back":points}).to_string(), "{}")
        .unwrap();
    assert!((measured["records"][0]["value_mm"].as_f64().unwrap() - 88.).abs() < 0.4);
    assert_eq!(measured["photo"]["lens_input"]["recomputed"], true);
    if let Ok(directory) = std::env::var("FTX_CAMERA_WEB_FIXTURES") {
        use std::io::Write;
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        let mut exports = vec![
            ("manifest.json".to_string(), manifest.as_bytes().to_vec()),
            ("sheet.json".to_string(), SHEET.as_bytes().to_vec()),
            (
                "report.json".to_string(),
                serde_json::to_vec(&report).unwrap(),
            ),
        ];
        exports.extend(
            images
                .into_iter()
                .enumerate()
                .map(|(i, bytes)| (format!("view{i}.png"), bytes)),
        );
        for (name, bytes) in exports {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(&bytes)
                .unwrap();
        }
    }
}

#[test]
fn image_calibration_refuses_changed_capture_incomplete_evidence_and_budget_overruns() {
    let manifest = camera_manifest().to_string();
    let mut batch = CameraCalibration::new(&manifest, SHEET, "").unwrap();
    assert!(batch.add_image("view0.png", SHEET_PNG).is_err());
    assert!(
        batch
            .add_image(
                "view0.png",
                &encode_png_gray(&Gray::filled(1600, 1200, 127))
            )
            .is_err()
    );
    assert!(batch.finish().is_err());
    for case in 0..4 {
        let mut bad = camera_manifest();
        match case {
            0 => bad["views"][3]["partition"] = json!("fit"),
            1 => bad["held_out_corner_ids"] = json!([0, 6, 28]),
            2 => bad["image_size_px"] = json!([5000, 4000]),
            _ => bad["views"][1]["frame"] = bad["views"][0]["frame"].clone(),
        }
        assert!(CameraCalibration::new(&bad.to_string(), SHEET, "").is_err());
    }
    assert!(CameraCalibration::new(&manifest, SHEET, "{}").is_err());
    assert!(CameraCalibration::new(&" ".repeat(16 * 1024 * 1024 + 1), SHEET, "").is_err());
}
