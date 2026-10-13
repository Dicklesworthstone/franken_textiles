//! Exact independent projective observations; no photographs, fitted fixtures or Anny assets.
use ftx_core::{Mat3, Vec2, Vec3};
use ftx_measure::{
    photo::intrinsics::{CalibrationInput, Control, Corner, View, calibrate_camera},
    triangulate::{FrameId, Partition},
};

fn fixture() -> (CalibrationInput, Vec<(Mat3, Vec3)>) {
    let controls: Vec<_> = (0..20)
        .map(|i| Control {
            id: format!("p{i:02}"),
            sheet_xz_m: Vec2::new((i % 5) as f64 * 0.12 - 0.24, (i / 5) as f64 * 0.12 - 0.18),
        })
        .collect();
    let base = Mat3::from_rows([[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]]);
    let poses: Vec<_> = [(0.35, 0.1), (-0.45, 0.3), (0.2, -0.55), (-0.3, -0.25)]
        .iter()
        .enumerate()
        .map(|(i, &(a, b))| {
            (
                Mat3::rotation(Vec3::new(1., 0., 0.), a)
                    * Mat3::rotation(Vec3::new(0., 1., 0.), b)
                    * base,
                Vec3::new(
                    (i as f64 - 1.5) * 0.05,
                    0.015 * i as f64,
                    1.2 + 0.08 * i as f64,
                ),
            )
        })
        .collect();
    // Pixels computed directly from known rotation/translation and a non-square pinhole K,
    // independently of Camera::project, Camera::from_sheet and the calibration implementation.
    let views = poses
        .iter()
        .enumerate()
        .map(|(i, (r, t))| View {
            frame: FrameId {
                stream: "synthetic-lens".into(),
                timestamp_us: (i as u64 + 1) * 200000,
            },
            partition: if i == 3 {
                Partition::HeldOut
            } else {
                Partition::Fit
            },
            corners: controls
                .iter()
                .enumerate()
                .map(|(k, p)| {
                    let q = r.mul_vec(Vec3::new(p.sheet_xz_m.x, 0., p.sheet_xz_m.y)) + *t;
                    Corner {
                        id: p.id.clone(),
                        pixel_px: Vec2::new(900. * q.x / q.z + 641., 1100. * q.y / q.z + 463.),
                        partition: if k == 6 || k == 13 {
                            Partition::HeldOut
                        } else {
                            Partition::Fit
                        },
                    }
                })
                .collect(),
        })
        .collect();
    (
        CalibrationInput {
            schema: "ftx-camera-calibration-input/1".into(),
            image_model: "pinhole_zero_skew".into(),
            lens_id: "synthetic-pinhole".into(),
            pinhole_evidence: "analytic zero-skew projection, no distortion".into(),
            reference_evidence: "synthetic 120 mm planar grid".into(),
            image_size_px: [1280, 960],
            rotation_cw_deg: 0,
            controls,
            views,
        },
        poses,
    )
}

#[test]
fn recovers_focals_principal_point_and_all_sheet_local_poses() {
    let (input, poses) = fixture();
    let report = calibrate_camera(&input).unwrap();
    assert_eq!(report.status, "ACCEPTED", "{report:?}");
    let solution = report.solution.unwrap();
    assert!((solution.intrinsics.focal_px - Vec2::new(900., 1100.)).norm() < 1e-8);
    assert!((solution.intrinsics.principal_px - Vec2::new(641., 463.)).norm() < 1e-8);
    assert!(report.constraint_pivot_ratio.unwrap() > report.min_constraint_pivot_ratio);
    assert!(report.constraint_squared_residual.unwrap() < 1e-25);
    for (got, (r, t)) in solution.sheet_poses.iter().zip(poses) {
        let c = got.camera_in_sheet_frame;
        assert!((c.center_m + r.transpose().mul_vec(t)).norm() < 1e-10);
        for (actual, expected) in c
            .world_to_camera
            .rows
            .iter()
            .flatten()
            .zip(r.rows.iter().flatten())
        {
            assert!((actual - expected).abs() < 1e-10);
        }
    }
    assert!(
        report
            .checks
            .iter()
            .flat_map(|c| &c.corners)
            .all(|c| c.error_px.unwrap() < 1e-8)
    );
    assert!(report.advisories.is_empty());
}

#[test]
fn ordering_is_canonical_and_input_is_not_modified() {
    let (mut input, _) = fixture();
    let before = serde_json::to_value(&input).unwrap();
    let expected = serde_json::to_value(calibrate_camera(&input).unwrap()).unwrap();
    assert_eq!(before, serde_json::to_value(&input).unwrap());
    input.controls.reverse();
    input.views.reverse();
    for view in &mut input.views {
        view.corners.reverse();
    }
    assert_eq!(
        expected,
        serde_json::to_value(calibrate_camera(&input).unwrap()).unwrap()
    );
}

#[test]
fn modest_pixel_noise_still_recovers_a_useful_camera_without_claiming_exactness() {
    let (mut input, _) = fixture();
    for (i, view) in input.views.iter_mut().enumerate() {
        for (k, p) in view.corners.iter_mut().enumerate() {
            p.pixel_px.x += ((k * 7 + i * 11) % 19) as f64 * 0.001 - 0.009;
            p.pixel_px.y += ((k * 13 + i * 3) % 17) as f64 * 0.001 - 0.008;
        }
    }
    let report = calibrate_camera(&input).unwrap();
    assert_eq!(report.status, "ACCEPTED", "{report:?}");
    let k = report.solution.unwrap().intrinsics;
    assert!((k.focal_px - Vec2::new(900., 1100.)).norm() < 1.0, "{k:?}");
    assert!(
        (k.principal_px - Vec2::new(641., 463.)).norm() < 1.0,
        "{k:?}"
    );
}

#[test]
fn translations_and_in_plane_rotations_do_not_manufacture_observability() {
    let (mut input, _) = fixture();
    for (i, view) in input.views.iter_mut().enumerate() {
        let (s, c) = ftx_core::det::sin_cos(i as f64 * 0.25);
        for (p, control) in view.corners.iter_mut().zip(&input.controls) {
            let q = control.sheet_xz_m;
            p.pixel_px = Vec2::new(
                900. * (q.x * c - q.y * s) / (1.2 + i as f64 * 0.1) + 600. + i as f64 * 3.,
                1100. * (q.x * s + q.y * c) / (1.2 + i as f64 * 0.1) + 470.,
            );
        }
    }
    let report = calibrate_camera(&input).unwrap();
    assert_eq!(report.status, "UNOBSERVABLE", "{report:?}");
    assert!(report.solution.is_none());
    assert!(report.constraint_pivot_ratio.unwrap() < report.min_constraint_pivot_ratio);
    let (mut input, _) = fixture();
    input.views[0].partition = Partition::HeldOut;
    assert_eq!(calibrate_camera(&input).unwrap().status, "UNOBSERVABLE");
}

#[test]
fn held_out_corners_never_change_intrinsics_or_pose_fits_but_can_reject_them() {
    let (mut input, _) = fixture();
    let good = calibrate_camera(&input).unwrap();
    let point = input.views[0]
        .corners
        .iter_mut()
        .find(|p| p.partition == Partition::HeldOut)
        .unwrap();
    point.pixel_px.x += 20.;
    let bad = calibrate_camera(&input).unwrap();
    assert_eq!(bad.status, "REJECTED");
    assert!(bad.solution.is_none());
    assert_eq!(bad.constraint_pivot_ratio, good.constraint_pivot_ratio);
    assert_eq!(
        bad.constraint_squared_residual,
        good.constraint_squared_residual
    );
    for (a, b) in bad
        .checks
        .iter()
        .flat_map(|c| &c.corners)
        .zip(good.checks.iter().flat_map(|c| &c.corners))
    {
        if a.partition == Partition::Fit {
            assert_eq!(a.error_px, b.error_px);
        }
    }
}

#[test]
fn held_out_views_cannot_steer_shared_intrinsics() {
    let (mut input, _) = fixture();
    let good = calibrate_camera(&input).unwrap();
    for p in &mut input.views[3].corners {
        p.pixel_px.x = 641. + (p.pixel_px.x - 641.) * 1.25;
    }
    let bad = calibrate_camera(&input).unwrap();
    assert_eq!(bad.status, "REJECTED", "{bad:?}");
    assert!(bad.solution.is_none());
    assert_eq!(
        bad.constraint_squared_residual,
        good.constraint_squared_residual
    );
    for (a, b) in bad.checks[..3].iter().zip(&good.checks[..3]) {
        assert_eq!(
            serde_json::to_value(a).unwrap(),
            serde_json::to_value(b).unwrap()
        );
    }
}

#[test]
fn reference_unit_changes_scale_poses_but_not_intrinsics() {
    let (mut input, _) = fixture();
    let good = calibrate_camera(&input).unwrap().solution.unwrap();
    for p in &mut input.controls {
        p.sheet_xz_m = p.sheet_xz_m * 2.5;
    }
    let scaled = calibrate_camera(&input).unwrap().solution.unwrap();
    assert!((good.intrinsics.focal_px - scaled.intrinsics.focal_px).norm() < 1e-8);
    for (a, b) in good.sheet_poses.iter().zip(scaled.sheet_poses) {
        assert!(
            (a.camera_in_sheet_frame.center_m * 2.5 - b.camera_in_sheet_frame.center_m).norm()
                < 1e-10
        );
    }
}

#[test]
fn malformed_identities_coordinates_models_and_budgets_refuse() {
    for mutation in 0..9 {
        let (mut input, _) = fixture();
        match mutation {
            0 => input.views[1].frame = input.views[0].frame.clone(),
            1 => input.views[1].frame.stream = "different-phone".into(),
            2 => input.controls[1].sheet_xz_m = input.controls[0].sheet_xz_m,
            3 => input.views[0].corners[0].id = "unknown".into(),
            4 => input.views[0].corners[0].pixel_px.x = f64::NAN,
            5 => input.views[0].corners[0].pixel_px.x = 1280.,
            6 => input.image_model = "brown_distorted".into(),
            7 => input.pinhole_evidence.clear(),
            _ => input.views.resize(33, input.views[0].clone()),
        }
        let err = calibrate_camera(&input).unwrap_err();
        if mutation == 8 {
            assert_eq!(err.code, ftx_core::RefusalCode::BudgetExceeded);
        }
    }
    let (input, _) = fixture();
    let mut json = serde_json::to_value(input).unwrap();
    json["typo"] = serde_json::json!(1);
    assert!(serde_json::from_value::<CalibrationInput>(json).is_err());
}

#[test]
fn collinear_or_insufficient_fit_controls_cannot_be_repaired_by_held_out_points() {
    let (mut input, _) = fixture();
    for p in &mut input.views[0].corners[3..] {
        p.partition = Partition::HeldOut;
    }
    assert_eq!(calibrate_camera(&input).unwrap().status, "UNOBSERVABLE");
    let (mut input, _) = fixture();
    for (i, p) in input.controls.iter_mut().enumerate() {
        p.sheet_xz_m = Vec2::new(i as f64 * 0.01, 0.);
    }
    assert_eq!(calibrate_camera(&input).unwrap().status, "UNOBSERVABLE");
}

#[test]
fn large_nonprojective_warp_is_rejected_not_labelled_lens_calibration() {
    let (mut input, _) = fixture();
    for view in &mut input.views {
        for (i, p) in view.corners.iter_mut().enumerate() {
            if i % 3 == 0 {
                p.pixel_px.x += 25.;
            }
        }
    }
    let report = calibrate_camera(&input).unwrap();
    assert_ne!(report.status, "ACCEPTED");
    assert!(report.solution.is_none());
}
