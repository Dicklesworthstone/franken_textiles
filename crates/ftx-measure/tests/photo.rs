//! Synthetic cameras and analytic distances, not candidate-generated oracle fixtures.
use ftx_core::det;
use ftx_measure::photo::{PhotoOptions, analyze_reference, measure_photo};
use serde_json::{Value, json};

fn reference(paper: &str) -> Value {
    json!({"schema":"ftx-photo-reference/1", "reference":"plain_sheet", "paper":paper,
        "image_size_px":[1500,2000], "corners_px":[[234.1,820.6],[665.9,820.6],[665.9,1379.4],[234.1,1379.4]]})
}

// Independent pinhole projection, as described by spec 02 D.7 (0.5 mm/px at the sheet centre).
fn pinhole(tilt_deg: f64, axis_deg: f64, centre: [f64; 2]) -> [[f64; 2]; 4] {
    let (s, c) = det::sin_cos(tilt_deg.to_radians());
    let (ay, ax) = det::sin_cos(axis_deg.to_radians());
    let (f, z, cx, cy) = (2875.0, 1437.5, 749.5, 999.5);
    [[0.0, 0.0], [215.9, 0.0], [215.9, 279.4], [0.0, 279.4]].map(|[x, y]| {
        let (x, y) = (x - 215.9 / 2.0, y - 279.4 / 2.0);
        let dot = ax * x + ay * y;
        let rx = c * x + (1.0 - c) * dot * ax;
        let ry = c * y + (1.0 - c) * dot * ay;
        let rz = s * (ax * y - ay * x);
        [
            cx + f * (rx + (centre[0] - cx) * z / f) / (z + rz),
            cy + f * (ry + (centre[1] - cy) * z / f) / (z + rz),
        ]
    })
}

#[test]
fn square_on_letter_a4_and_landscape_are_identified() {
    for (paper, size) in [("letter", [215.9, 279.4]), ("a4", [210.0, 297.0])] {
        for landscape in [false, true] {
            let [w, h] = if landscape { [size[1], size[0]] } else { size };
            let mut r = reference("auto");
            r["corners_px"] = json!([
                [200.0, 300.0],
                [200.0 + 2.0 * w, 300.0],
                [200.0 + 2.0 * w, 300.0 + 2.0 * h],
                [200.0, 300.0 + 2.0 * h]
            ]);
            let out = analyze_reference(&r).unwrap();
            assert_eq!(out.report()["ok"], true, "{}", out.report());
            assert_eq!(out.report()["paper"], paper);
            assert_eq!(
                out.report()["orientation"],
                if landscape { "landscape" } else { "portrait" }
            );
            assert_eq!(out.report()["mm_per_px"], 0.5);
            assert_eq!(out.report()["focal_px_estimate"], Value::Null);
            assert!(out.report()["warnings"].as_array().unwrap().is_empty());
        }
    }
    let out = analyze_reference(&reference("a4")).unwrap();
    assert_eq!(out.report()["ok"], false);
    assert!(
        out.report()["warnings"][0]
            .as_str()
            .unwrap()
            .starts_with("ASPECT_MISMATCH")
    );
    assert!(
        measure_photo(
            &out,
            &json!({"length":[[100,900],[900,900]]}),
            &PhotoOptions::default()
        )
        .is_err()
    );
}

#[test]
fn exact_camera_geometry_matches_d7_and_preserves_ambiguity() {
    let mut r = reference("auto");
    r["corners_px"] = json!(pinhole(25.0, 40.0, [700.0, 900.0]));
    let scale = analyze_reference(&r).unwrap();
    assert_eq!(scale.report()["ok"], true, "{}", scale.report());
    assert_eq!(scale.report()["paper"], "letter");
    assert!((scale.report()["rect_residual"].as_f64().unwrap() - 0.000218).abs() < 0.00001);
    assert!((scale.report()["focal_px_estimate"].as_f64().unwrap() - 2871.48).abs() < 0.1);
    assert!(
        (scale.report()["rect_residual_by_paper"]["a4"]
            .as_f64()
            .unwrap()
            - 0.0874)
            .abs()
            < 0.0001
    );
    assert!(scale.report()["warnings"].as_array().unwrap().len() >= 2);
    r["corners_px"] = json!(pinhole(20.0, 0.0, [450.0, 1100.0]));
    let scale = analyze_reference(&r).unwrap();
    assert_eq!(scale.report()["ok"], false, "{}", scale.report());
    assert_eq!(scale.report()["paper"], Value::Null);
    assert!(
        scale.report()["warnings"][0]
            .as_str()
            .unwrap()
            .starts_with("PAPER_AMBIGUOUS")
    );
    r["paper"] = json!("letter");
    assert_eq!(analyze_reference(&r).unwrap().report()["ok"], true);
}

#[test]
fn malformed_degenerate_and_nonpaper_references_cannot_calibrate() {
    for (key, value) in [
        ("paper", json!("Letter")),
        ("image_size_px", json!([0, 2000])),
        ("corner_sigma_px", json!(0)),
        ("corners_px", json!([[0, 0], [1, 1], [2, 2], [3, 3]])),
        (
            "corners_px",
            json!([[0, 0], [500, 500], [0, 500], [500, 0]]),
        ),
        (
            "corners_px",
            json!([[-1, 100], [500, 100], [500, 600], [0, 600]]),
        ),
        ("corner_sgima_px", json!(2)),
    ] {
        let mut r = reference("letter");
        r[key] = value;
        assert!(analyze_reference(&r).is_err(), "{r}");
    }
    let mut r = reference("auto");
    r["corners_px"] = json!([[100, 100], [200, 100], [200, 1000], [100, 1000]]);
    assert_eq!(analyze_reference(&r).unwrap().report()["ok"], false);
}

#[test]
fn distances_angles_depth_terms_and_resized_clicks() {
    let r = reference("letter");
    let original = r.clone();
    let scale = analyze_reference(&r).unwrap();
    let (s, c) = det::sin_cos(20.0_f64.to_radians());
    let points = json!({"arm_length@right":{"points":[[50.0,450.0],[450.0,450.0]],"depth_offset_mm":75.0,"view":"P1"},
        "shoulder_slope_deg@left":{"points":[[50.0,450.0],[(100.0+800.0*c)/2.0,(900.0+800.0*s)/2.0]],"view":"P3"}});
    let options = PhotoOptions {
        points_from_width: Some(750.0),
        ..Default::default()
    };
    let out = measure_photo(&scale, &points, &options).unwrap();
    assert!(ftx_measure::validate(&out).is_empty(), "{out}");
    assert_eq!(r, original);
    let length = &out["records"][0];
    let angle = &out["records"][1];
    assert_eq!(length["value_mm"], 400.0);
    assert_eq!(length["sigma_terms_mm"]["depth_offset"], 12.0);
    assert!((length["sigma_terms_mm"]["click"].as_f64().unwrap() - 2.121).abs() < 0.001);
    assert!(length["sigma_mm"].as_f64().unwrap() > 12.0);
    assert_eq!(length["points_px"], json!([[100.0, 900.0], [900.0, 900.0]]));
    assert_eq!(angle["value_deg"], 20.0);
    assert_eq!(angle["sigma_terms_deg"]["sheet_level"], 1.5);
    assert!(angle["sigma_deg"].as_f64().unwrap() >= 1.5);
    assert!(
        angle["method"]
            .as_str()
            .unwrap()
            .contains("photo P3 plain Letter sheet-plane homography [left]")
    );
    for rec in out["records"].as_array().unwrap() {
        assert_eq!(rec["provenance"], "PHOTO_ESTIMATED");
    }
    assert_eq!(
        out["photo"]["measure_warnings"].as_array().unwrap().len(),
        1
    ); // default depth quirk
    assert_eq!(out, measure_photo(&scale, &points, &options).unwrap());
}

#[test]
fn uncertain_corners_increase_sigma_and_photo_never_becomes_tape() {
    let mut r = reference("letter");
    r["corner_sigma_px"] = json!(0.01);
    let points = json!({"head":[[100,900],[1100,900]]});
    let options = PhotoOptions::default();
    let small = measure_photo(&analyze_reference(&r).unwrap(), &points, &options).unwrap();
    r["corner_sigma_px"] = json!(6.0);
    let large = measure_photo(&analyze_reference(&r).unwrap(), &points, &options).unwrap();
    assert!(
        large["records"][0]["sigma_mm"].as_f64().unwrap()
            > small["records"][0]["sigma_mm"].as_f64().unwrap()
    );
    let fused = ftx_measure::fuse(&large, false).unwrap();
    assert_eq!(
        fused["fused"]["head"]["primary_provenance"],
        "PHOTO_ESTIMATED"
    );
    let gate = ftx_measure::readiness(
        &fused,
        "hat_fez",
        &ftx_measure::ReadinessOptions {
            use_priors: false,
            ..Default::default()
        },
        "2026-10-13",
    )
    .unwrap();
    assert_eq!(gate["status"], "NOT_READY");
}

#[test]
fn invalid_measurements_and_unstable_calibration_are_atomic_refusals() {
    let scale = analyze_reference(&reference("letter")).unwrap();
    for points in [
        json!({}),
        json!({"x":[[100,900],[100,900]]}),
        json!({"x":[[100,900]]}),
        json!({"x":[[-1,900],[900,900]]}),
        json!({"x":{"points":[[100,900],[900,900]],"depth_offset_mm":2500}}),
        json!({"x":{"points":[[100,900],[900,900]],"veiw":"P1"}}),
    ] {
        assert!(
            measure_photo(&scale, &points, &PhotoOptions::default()).is_err(),
            "{points}"
        );
    }
    let points = json!({"x":[[100,900],[900,900]]});
    for o in [
        PhotoOptions {
            click_px: f64::NAN,
            ..Default::default()
        },
        PhotoOptions {
            bootstrap: 0,
            ..Default::default()
        },
        PhotoOptions {
            bootstrap: 2001,
            ..Default::default()
        },
        PhotoOptions {
            camera_distance_mm: 0.0,
            ..Default::default()
        },
        PhotoOptions {
            points_from_width: Some(-1.0),
            ..Default::default()
        },
        PhotoOptions {
            sheet_level_sigma_deg: -1.0,
            ..Default::default()
        },
    ] {
        assert!(measure_photo(&scale, &points, &o).is_err());
    }
    let mut r = reference("letter");
    r["corner_sigma_px"] = json!(1_000_000.0);
    assert!(
        measure_photo(
            &analyze_reference(&r).unwrap(),
            &points,
            &PhotoOptions::default()
        )
        .is_err()
    );
}

fn charuco_reference(noise: bool) -> Value {
    let corners: Vec<Value> = (0..35)
        .map(|id| {
            let x = (id % 5 + 1) as f64 * 22.0;
            let y = (id / 5 + 1) as f64 * 22.0;
            let jitter = if noise {
                (id % 7) as f64 * 0.11 - 0.33
            } else {
                0.0
            };
            json!({"id":id,"point_px":[200.0 + 2.0 * x + jitter, 300.0 + 2.0 * y - jitter]})
        })
        .collect();
    json!({"schema":"ftx-photo-reference/1", "reference":"charuco", "image_size_px":[1500,2000],
        "board":{"squares_x":6,"squares_y":8,"square_mm":22.0,"origin_mm":[12.0,20.0]},
        "corners":corners,"markers_detected":24})
}

#[test]
fn imported_charuco_rejects_outliers_and_measures_with_inlier_bootstrap() {
    let mut reference = charuco_reference(true);
    for id in [3, 15, 22, 29] {
        reference["corners"][id]["point_px"] = json!([1200.0 + id as f64, 1800.0]);
    }
    reference["blur_sigma_px_estimate"] = json!(2.8);
    let scale = analyze_reference(&reference).unwrap();
    assert_eq!(scale.report()["ok"], true, "{}", scale.report());
    assert_eq!(scale.report()["inliers"], 31);
    assert_eq!(scale.report()["outlier_ids"], json!([3, 15, 22, 29]));
    assert!(scale.report()["max_residual_mm"].as_f64().unwrap() < 0.3);
    assert!(scale.report()["reprojection_rms_px"].as_f64().unwrap() < 0.5);
    assert_eq!(
        scale.report()["board_origin_on_paper_mm"],
        json!([12.0, 20.0])
    );
    assert!(
        scale.report()["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().starts_with("BLURRY"))
    );
    let points = json!({"length":[[250.0,450.0],[750.0,450.0]]});
    let measured = measure_photo(&scale, &points, &PhotoOptions::default()).unwrap();
    let r = &measured["records"][0];
    assert!(
        (r["value_mm"].as_f64().unwrap() - 250.0).abs() <= 0.2,
        "{r}"
    );
    assert!(
        r["method"]
            .as_str()
            .unwrap()
            .starts_with("photo ? ChArUco sheet-plane homography")
    );
    assert_eq!(r["provenance"], "PHOTO_ESTIMATED");
    assert!(r["sigma_terms_mm"]["homography"].as_f64().unwrap() > 0.0);
    assert!(r["bootstrap_accepted"].as_u64().unwrap() >= 160);
    // External detector ordering cannot alter RANSAC, inlier order or the random stream.
    reference["corners"].as_array_mut().unwrap().reverse();
    let reversed = analyze_reference(&reference).unwrap();
    assert_eq!(scale.report(), reversed.report());
    assert_eq!(
        measured,
        measure_photo(&reversed, &points, &PhotoOptions::default()).unwrap()
    );
}

#[test]
fn exact_charuco_projection_and_invalid_identifiers() {
    let reference = charuco_reference(false);
    let scale = analyze_reference(&reference).unwrap();
    assert_eq!(scale.report()["mm_per_px"], 0.5);
    assert_eq!(scale.report()["inliers"], 35);
    assert_eq!(scale.report()["reprojection_rms_mm"], 0.0);
    let m = measure_photo(
        &scale,
        &json!({"length":[[100,900],[900,900]]}),
        &PhotoOptions::default(),
    )
    .unwrap();
    assert_eq!(m["records"][0]["value_mm"], 400.0);
    assert_eq!(m["records"][0]["sigma_terms_mm"]["homography"], 0.0);
    for (key, value) in [
        ("id", json!(35)),
        ("id", json!(-1)),
        ("id", json!(1)),
        ("id", json!(2.5)),
        ("point_px", json!([0, -1])),
        ("unexpected", json!(1)),
    ] {
        let mut bad = reference.clone();
        bad["corners"][0][key] = value;
        assert!(analyze_reference(&bad).is_err(), "{bad}");
    }
    let mut bad = reference.clone();
    bad["board"]["squares_x"] = json!(2);
    assert!(analyze_reference(&bad).is_err());
    bad = reference;
    bad["blur_sigma_px_estimate"] = json!(-1);
    assert!(analyze_reference(&bad).is_err());
}

#[test]
fn inadequate_charuco_consensus_is_not_a_usable_scale() {
    let mut reference = charuco_reference(false);
    reference["corners"].as_array_mut().unwrap().truncate(5);
    let scale = analyze_reference(&reference).unwrap();
    assert_eq!(scale.report()["ok"], false);
    assert!(
        scale.report()["warnings"][0]
            .as_str()
            .unwrap()
            .starts_with("BOARD_NOT_FOUND")
    );
    assert!(
        measure_photo(
            &scale,
            &json!({"length":[[100,900],[900,900]]}),
            &PhotoOptions::default()
        )
        .is_err()
    );
    let mut reference = charuco_reference(false);
    for (i, c) in reference["corners"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        c["point_px"] = json!([100.0 + i as f64, 300.0]);
    }
    let scale = analyze_reference(&reference).unwrap();
    assert_eq!(scale.report()["ok"], false);
    assert!(scale.report().get("homography_px_to_board_mm").is_none());
    // Six well-spread corners may calibrate, but too few diverse bootstrap draws must refuse
    // measurement rather than silently substituting zero homography uncertainty.
    let mut reference = charuco_reference(false);
    let all = reference["corners"].as_array().unwrap().clone();
    reference["corners"] = json!([all[0], all[4], all[10], all[14], all[30], all[34]]);
    let scale = analyze_reference(&reference).unwrap();
    assert_eq!(scale.report()["ok"], true);
    assert!(
        measure_photo(
            &scale,
            &json!({"length":[[100,900],[900,900]]}),
            &PhotoOptions::default()
        )
        .is_err()
    );
}
