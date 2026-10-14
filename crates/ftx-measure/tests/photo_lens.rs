//! Independent Brown projections: lens correction must reach scale AND click uncertainty.
use ftx_core::{RefusalCode, Vec2};
use ftx_measure::{
    camera::{Distortion, Intrinsics},
    photo::{PhotoOptions, analyze_reference, measure_photo},
};
use serde_json::{Value, json};

fn lens() -> Intrinsics {
    Intrinsics {
        focal_px: Vec2::new(700.0, 830.0),
        principal_px: Vec2::new(640.0, 480.0),
        distortion: Distortion {
            radial: [0.22, 0.02, 0.0],
            tangential: [0.006, -0.004],
            max_radius: 1.2,
        },
    }
}

// Direct polynomial, not candidate-generated truth or the camera implementation.
fn observed(k: &Intrinsics, p: [f64; 2]) -> [f64; 2] {
    let x = (p[0] - k.principal_px.x) / k.focal_px.x;
    let y = (p[1] - k.principal_px.y) / k.focal_px.y;
    let r2 = x * x + y * y;
    let [k1, k2, k3] = k.distortion.radial;
    let [p1, p2] = k.distortion.tangential;
    let r = 1.0 + k1 * r2 + k2 * r2 * r2 + k3 * r2 * r2 * r2;
    [
        k.principal_px.x + k.focal_px.x * (x * r + 2.0 * p1 * x * y + p2 * (r2 + 2.0 * x * x)),
        k.principal_px.y + k.focal_px.y * (y * r + p1 * (r2 + 2.0 * y * y) + 2.0 * p2 * x * y),
    ]
}

fn pixel(k: &Intrinsics, board: [f64; 2]) -> [f64; 2] {
    observed(k, [200.0 + 2.0 * board[0], 200.0 + 2.0 * board[1]])
}

fn reference(k: &Intrinsics) -> Value {
    let corners: Vec<_> = (0..35)
        .map(|id| {
            json!({"id":id,
        "point_px":pixel(k, [22.0*(id % 7 + 1) as f64, 22.0*(id / 7 + 1) as f64])})
        })
        .collect();
    json!({"schema":"ftx-photo-reference/1", "reference":"charuco",
        "image_size_px":[1280,960], "board":{"squares_x":8,"squares_y":6,"square_mm":22},
        "corners":corners, "intrinsics":k})
}

fn points(k: &Intrinsics) -> Value {
    json!({"across_back":[pixel(k,[30.0,20.0]),pixel(k,[230.0,20.0])],
        "shoulder_slope_deg":[pixel(k,[30.0,20.0]),pixel(k,[190.0,95.0])]})
}

#[test]
fn distortion_correction_recovers_metric_lengths_and_angles_not_just_corners() {
    let k = lens();
    let mut r = reference(&k);
    let scale = analyze_reference(&r).unwrap();
    let report = scale.report();
    assert_eq!(report["ok"], true);
    assert_eq!(report["inliers"], 35);
    assert_eq!(report["reprojection_rms_mm"], 0.0);
    assert_eq!(report["reprojection_rms_px"], 0.0);
    assert!(report.get("homography_px_to_board_mm").is_none());
    assert!(report["homography_undistorted_px_to_board_mm"].is_array());
    assert_eq!(
        report["homography_coordinate_frame"],
        "undistorted_pixels_same_intrinsics"
    );
    let result = measure_photo(&scale, &points(&k), &PhotoOptions::default()).unwrap();
    assert_eq!(result["records"][0]["value_mm"], 200.0);
    assert_eq!(result["records"][1]["value_deg"], 25.11);
    assert_eq!(result["records"][0]["provenance"], "PHOTO_ESTIMATED");
    assert_eq!(
        result["records"][0]["points_px"][0][0],
        ftx_core::py::round(pixel(&k, [30.0, 20.0])[0], 2)
    );
    assert_eq!(
        result,
        measure_photo(&scale, &points(&k), &PhotoOptions::default()).unwrap()
    );
    // Detecting the board and fitting raw pixels alone leaves a measurable off-board bias.
    r.as_object_mut().unwrap().remove("intrinsics");
    let raw = analyze_reference(&r).unwrap();
    let uncorrected = measure_photo(&raw, &points(&k), &PhotoOptions::default()).unwrap();
    let bias = (uncorrected["records"][0]["value_mm"].as_f64().unwrap() - 200.0).abs();
    assert!(bias > 1.0, "uncorrected: {uncorrected}");
}

// Independent forward differences d observed_pixel / d board_mm, then inverse.
fn inverse_jacobian(k: &Intrinsics, q: [f64; 2]) -> [[f64; 2]; 2] {
    let eps = 1e-3;
    let mut j = [[0.0; 2]; 2];
    for c in 0..2 {
        let (mut a, mut b) = (q, q);
        a[c] += eps;
        b[c] -= eps;
        let (a, b) = (pixel(k, a), pixel(k, b));
        for r in 0..2 {
            j[r][c] = (a[r] - b[r]) / (2.0 * eps);
        }
    }
    let d = j[0][0] * j[1][1] - j[0][1] * j[1][0];
    [[j[1][1] / d, -j[0][1] / d], [-j[1][0] / d, j[0][0] / d]]
}

#[test]
fn click_sigma_and_reported_local_scale_use_the_composed_raw_pixel_jacobian() {
    let k = lens();
    let scale = analyze_reference(&reference(&k)).unwrap();
    let j = inverse_jacobian(&k, [88.0, 66.0]);
    let mmpp = (j[0][0] * j[1][1] - j[0][1] * j[1][0]).sqrt();
    assert!((scale.report()["mm_per_px"].as_f64().unwrap() - mmpp).abs() < 1e-6);
    let centre = pixel(&k, [88.0, 66.0]);
    for (i, p) in centre.iter().enumerate() {
        assert!((scale.report()["board_centre_px"][i].as_f64().unwrap() - p).abs() < 0.006);
    }
    let result = measure_photo(&scale, &points(&k), &PhotoOptions::default()).unwrap();
    for (index, q, direction, factor, key) in [
        (0, [230.0, 20.0], [1.0, 0.0], 1.0, "sigma_terms_mm"),
        (
            1,
            [190.0, 95.0],
            [-75.0 / 176.70597047072292, 160.0 / 176.70597047072292],
            180.0 / std::f64::consts::PI / 176.70597047072292,
            "sigma_terms_deg",
        ),
    ] {
        let norm = |j: [[f64; 2]; 2]| {
            (direction[0] * j[0][0] + direction[1] * j[1][0]).powi(2)
                + (direction[0] * j[0][1] + direction[1] * j[1][1]).powi(2)
        };
        let sigma = 3.0
            * factor
            * (norm(inverse_jacobian(&k, [30.0, 20.0])) + norm(inverse_jacobian(&k, q))).sqrt();
        assert!(
            (result["records"][index][key]["click"].as_f64().unwrap() - sigma).abs() < 0.001,
            "{result}"
        );
    }
}

#[test]
fn calibrated_domain_is_enforced_for_corners_board_and_landmarks() {
    let k = lens();
    let mut r = reference(&k);
    r["intrinsics"]["distortion"]["radial"] = json!([-2.0, 0, 0]);
    assert!(analyze_reference(&r).is_err());
    r = reference(&k);
    r["intrinsics"]["distortion"]["max_radius"] = json!(0.05);
    assert!(analyze_reference(&r).is_err());
    let scale = analyze_reference(&reference(&k)).unwrap();
    // A point inside the image but outside a tighter calibrated disk must not be measured.
    let mut narrow = k;
    narrow.distortion.max_radius = 0.75;
    let scale_narrow = analyze_reference(&reference(&narrow)).unwrap();
    let outside = json!({"length":[[1279.0,959.0],pixel(&narrow,[40.0,40.0])]});
    assert!(measure_photo(&scale_narrow, &outside, &PhotoOptions::default()).is_err());
    assert!(measure_photo(&scale, &outside, &PhotoOptions::default()).is_ok());
    r = reference(&k);
    r["intrinsics"] = Value::Null;
    assert_eq!(
        analyze_reference(&r).unwrap_err().code,
        RefusalCode::InvalidDocument
    );
}

#[test]
fn analytic_pixel_inverse_jacobian_matches_finite_differences_with_unequal_focals() {
    let k = lens();
    for p in [[200.0, 150.0], [930.0, 600.0], [500.0, 790.0]] {
        let raw = observed(&k, p);
        let (u, j) = k
            .undistort_pixel_with_jacobian(Vec2::new(raw[0], raw[1]))
            .unwrap();
        assert!((u.x - p[0]).abs() < 1e-8 && (u.y - p[1]).abs() < 1e-8);
        assert!((k.distort_pixel(u).unwrap() - Vec2::new(raw[0], raw[1])).norm() < 1e-8);
        for c in 0..2 {
            let (mut a, mut b) = (raw, raw);
            a[c] += 1e-3;
            b[c] -= 1e-3;
            let a = k
                .undistort_pixel_with_jacobian(Vec2::new(a[0], a[1]))
                .unwrap()
                .0;
            let b = k
                .undistort_pixel_with_jacobian(Vec2::new(b[0], b[1]))
                .unwrap()
                .0;
            for (r, d) in [(0, (a.x - b.x) / 2e-3), (1, (a.y - b.y) / 2e-3)] {
                assert!((j[r][c] - d).abs() < 1e-7);
            }
        }
    }
}

#[test]
fn zero_distortion_preserves_records_and_existing_coordinate_workflows() {
    let mut k = lens();
    k.distortion = Distortion::default();
    let mut r = reference(&k);
    let corrected = measure_photo(
        &analyze_reference(&r).unwrap(),
        &points(&k),
        &PhotoOptions::default(),
    )
    .unwrap();
    r.as_object_mut().unwrap().remove("intrinsics");
    let original = measure_photo(
        &analyze_reference(&r).unwrap(),
        &points(&k),
        &PhotoOptions::default(),
    )
    .unwrap();
    assert_eq!(corrected["records"], original["records"]);
    assert_eq!(
        corrected["photo"]["mm_per_px"],
        original["photo"]["mm_per_px"]
    );
    let p = Vec2::new(123.456, 789.012);
    assert_eq!(
        k.undistort_pixel_with_jacobian(p).unwrap(),
        (p, [[1.0, 0.0], [0.0, 1.0]])
    );
}

#[test]
fn resized_clicks_and_outlier_rejection_use_one_consistent_lens_frame() {
    let k = lens();
    let mut r = reference(&k);
    for id in [0, 17, 34] {
        r["corners"][id]["point_px"] = json!([1000.0, 850.0]);
    }
    let scale = analyze_reference(&r).unwrap();
    assert_eq!(scale.report()["outlier_ids"], json!([0, 17, 34]));
    assert_eq!(scale.report()["inliers"], 32);
    let mut clicks = points(&k);
    let full = measure_photo(&scale, &clicks, &PhotoOptions::default()).unwrap();
    for points in clicks.as_object_mut().unwrap().values_mut() {
        for p in points.as_array_mut().unwrap() {
            for c in p.as_array_mut().unwrap() {
                *c = json!(c.as_f64().unwrap() / 2.0);
            }
        }
    }
    let options = PhotoOptions {
        points_from_width: Some(640.0),
        ..PhotoOptions::default()
    };
    assert_eq!(measure_photo(&scale, &clicks, &options).unwrap(), full);
    r["corners"].as_array_mut().unwrap().reverse();
    assert_eq!(analyze_reference(&r).unwrap().report(), scale.report());
}
