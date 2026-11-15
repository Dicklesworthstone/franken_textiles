//! The common metric sheet gauge must survive camera recovery and off-plane triangulation.

use ftx_core::{Mat3, Vec2, Vec3};
use ftx_measure::camera::{Camera, Distortion, Intrinsics};
use ftx_measure::triangulate::{
    Capture, FrameId, Observation, Partition, TriangulationInput, triangulate,
};

#[test]
fn stationary_sheet_cameras_recover_an_off_sheet_landmark() {
    let intrinsics = Intrinsics {
        focal_px: Vec2::new(900.0, 900.0),
        principal_px: Vec2::new(500.0, 400.0),
        distortion: Distortion::default(),
    };
    let sheet = [
        Vec2::new(0.0, 0.0),
        Vec2::new(0.21, 0.0),
        Vec2::new(0.21, 0.297),
        Vec2::new(0.0, 0.297),
        Vec2::new(0.105, 0.1485),
    ];
    let point = Vec3::new(0.02, -0.2, 1.3);
    let mut observations = Vec::new();
    for (i, x) in [-0.4, 0.4, 0.0].into_iter().enumerate() {
        let known = Camera {
            intrinsics,
            world_to_camera: Mat3::from_rows([[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]),
            center_m: Vec3::new(x, -3.0, 1.0),
        };
        let pixels: Vec<_> = sheet
            .iter()
            .map(|p| known.project(Vec3::new(p.x, 0.0, p.y)).unwrap())
            .collect();
        let camera = Camera::from_sheet(intrinsics, &sheet, &pixels).unwrap();
        observations.push(Observation {
            frame: FrameId {
                stream: format!("synthetic-camera-{i}"),
                timestamp_us: 1_000_000,
            },
            camera,
            pixel_px: known.project(point).unwrap(),
            partition: if i == 2 {
                Partition::HeldOut
            } else {
                Partition::Fit
            },
        });
    }
    let input = TriangulationInput {
        schema: "ftx-triangulation-input/1".into(),
        metric_scale_source: "Synthetic common stationary 210x297 mm sheet".into(),
        capture: Capture::Synchronized {
            max_skew_us: 0,
            evidence: "Synthetic simultaneous stationary acquisition".into(),
        },
        observations,
    };
    let report = triangulate(&input).unwrap();
    assert_eq!(report.status, "ACCEPTED", "{:?}", report.flags);
    assert!((report.point_m.unwrap() - point).norm() < 1e-8);
    assert_eq!(report.held_out_views, 1);
}

use ftx_measure::triangulate::uncertainty::{
    FrameSheetPose, ReferenceScaleNoiseInput, triangulate_with_reference_scale_noise,
};
use serde_json::json;

fn reference_scale_fixture(
    fit_views: usize,
    point: Vec3,
) -> (TriangulationInput, ReferenceScaleNoiseInput) {
    let intrinsics = Intrinsics {
        focal_px: Vec2::new(800., 800.),
        principal_px: Vec2::new(800., 600.),
        distortion: Distortion::default(),
    };
    let mut observations = Vec::new();
    let mut sheet_poses = Vec::new();
    for (i, x) in [-0.5, 0.5, 0., 0.75]
        .into_iter()
        .take(fit_views + 1)
        .enumerate()
    {
        let known = Camera {
            intrinsics,
            world_to_camera: Mat3::from_rows([[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]]),
            center_m: Vec3::new(x, -2., 0.3),
        };
        let points: Vec<_> = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5), (0.1, 0.2)]
            .into_iter()
            .enumerate()
            .map(|(index, (x, z))| {
                json!({"sheet_xz_m":[x,z],
                    "pixel_px":known.project(Vec3::new(x,0.,z)).unwrap(),
                    "held_out":index == 4})
            })
            .collect();
        let source = json!({"schema":"ftx-sheet-pose-input/1","reference_id":"synthetic-scale-square",
            "intrinsics":intrinsics,"image_size_px":[1600,1200],"rotation_applied_degrees":0,
            "points":points});
        let report = Camera::sheet_pose_report(&source).unwrap();
        assert_eq!(report["ok"], true);
        let camera: Camera = serde_json::from_value(report["camera"].clone()).unwrap();
        let frame = FrameId {
            stream: format!("scale-camera-{i}"),
            timestamp_us: 1_000_000,
        };
        observations.push(Observation {
            frame: frame.clone(),
            camera,
            pixel_px: known.project(point).unwrap(),
            partition: if i < fit_views {
                Partition::Fit
            } else {
                Partition::HeldOut
            },
        });
        if i < fit_views {
            sheet_poses.push(FrameSheetPose { frame, report });
        }
    }
    (
        TriangulationInput {
            schema: "ftx-triangulation-input/1".into(),
            metric_scale_source: "Synthetic stationary square; one shared dimensional scale".into(),
            capture: Capture::Synchronized {
                max_skew_us: 0,
                evidence: "Synthetic simultaneous views".into(),
            },
            observations,
        },
        ReferenceScaleNoiseInput {
            schema: "ftx-triangulation-reference-scale-noise/1".into(),
            reference_id: "synthetic-scale-square".into(),
            sigma_fraction: 0.01,
            evidence: "Synthetic one-percent uniform scale about the fixed sheet origin".into(),
            sheet_poses,
        },
    )
}

#[test]
fn shared_reference_scale_matches_analytic_stereo_cross_covariance() {
    let point = Vec3::new(0.08, -0.2, 0.15);
    let (input, noise) = reference_scale_fixture(2, point);
    let result = triangulate_with_reference_scale_noise(&input, &noise).unwrap();
    assert_eq!(
        json!(result.triangulation),
        json!(triangulate(&input).unwrap())
    );
    let report = result.conditional_reference_scale_uncertainty;
    assert_eq!(report.status, "AVAILABLE", "{:?}", report.flags);
    assert_eq!(report.reference_origin_m, Vec3::ZERO);
    assert!((report.jacobian_m_per_fraction.unwrap() - point).norm() < 1e-10);
    // An independent 1-percent common scale mode: [0.0008, -0.002, 0.0015] metres.
    let expected = [
        [6.4e-7, -1.6e-6, 1.2e-6],
        [-1.6e-6, 4e-6, -3e-6],
        [1.2e-6, -3e-6, 2.25e-6],
    ];
    for (actual, expected) in report
        .covariance_m2
        .unwrap()
        .rows
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
    {
        assert!((actual - expected).abs() < 1e-14);
    }
}

#[test]
fn shared_reference_scale_agrees_with_rescaling_the_original_sheet_estimator() {
    let (input, noise) = reference_scale_fixture(2, Vec3::new(0.08, -0.2, 0.15));
    let estimate = triangulate_with_reference_scale_noise(&input, &noise)
        .unwrap()
        .conditional_reference_scale_uncertainty
        .jacobian_m_per_fraction
        .unwrap();
    let rescaled = |factor: f64| {
        let mut changed = input.clone();
        for view in &mut changed.observations {
            view.camera.center_m = view.camera.center_m * factor;
        }
        for pose in &noise.sheet_poses {
            let mut source = pose.report["input"].clone();
            for corner in source["points"].as_array_mut().unwrap() {
                corner["sheet_xz_m"] = json!([
                    corner["sheet_xz_m"][0].as_f64().unwrap() * factor,
                    corner["sheet_xz_m"][1].as_f64().unwrap() * factor
                ]);
            }
            let replay = Camera::sheet_pose_report(&source).unwrap();
            assert_eq!(replay["ok"], true);
            changed
                .observations
                .iter_mut()
                .find(|view| view.frame == pose.frame)
                .unwrap()
                .camera = serde_json::from_value(replay["camera"].clone()).unwrap();
        }
        triangulate(&changed).unwrap().point_m.unwrap()
    };
    for factor in [0.5, 7.] {
        assert!((rescaled(factor) - estimate * factor).norm() < 1e-9);
    }
    let h = 1e-4;
    let differential = (rescaled(1. + h) - rescaled(1. - h)) / (2. * h);
    assert!((differential - estimate).norm() < 1e-7);
}

#[test]
fn common_reference_scale_does_not_average_down_with_extra_views() {
    let point = Vec3::new(0.08, -0.2, 0.15);
    let (input, noise) = reference_scale_fixture(2, point);
    let original = triangulate_with_reference_scale_noise(&input, &noise).unwrap();
    let (mut more, mut extra) = reference_scale_fixture(3, point);
    let result = triangulate_with_reference_scale_noise(&more, &extra).unwrap();
    for (actual, expected) in result
        .conditional_reference_scale_uncertainty
        .covariance_m2
        .unwrap()
        .rows
        .iter()
        .flatten()
        .zip(
            original
                .conditional_reference_scale_uncertainty
                .covariance_m2
                .unwrap()
                .rows
                .iter()
                .flatten(),
        )
    {
        assert!((actual - expected).abs() < 1e-14);
    }
    more.observations.reverse();
    extra.sheet_poses.reverse();
    assert_eq!(
        json!(result),
        json!(triangulate_with_reference_scale_noise(&more, &extra).unwrap())
    );
    more.observations[0].pixel_px.x += 0.1;
    assert_eq!(
        json!(result.conditional_reference_scale_uncertainty),
        json!(
            triangulate_with_reference_scale_noise(&more, &extra)
                .unwrap()
                .conditional_reference_scale_uncertainty
        )
    );
}

#[test]
fn reference_scale_requires_original_complete_camera_sources_and_a_fixed_origin() {
    let (input, noise) = reference_scale_fixture(2, Vec3::new(0.08, -0.2, 0.15));
    for sigma in [0., -1., f64::NAN, f64::INFINITY] {
        let mut changed = noise.clone();
        changed.sigma_fraction = sigma;
        assert!(triangulate_with_reference_scale_noise(&input, &changed).is_err());
    }
    for variant in 0..6 {
        let mut changed = noise.clone();
        match variant {
            0 => changed.reference_id = "different-reference".into(),
            1 => changed.evidence.clear(),
            2 => changed.schema.push_str("-unknown"),
            3 => {
                changed.sheet_poses.pop();
            }
            4 => changed.sheet_poses[1].frame = changed.sheet_poses[0].frame.clone(),
            _ => changed.sheet_poses[0].frame = input.observations[2].frame.clone(),
        }
        assert!(triangulate_with_reference_scale_noise(&input, &changed).is_err());
    }
    let mut shifted = input.clone();
    for view in &mut shifted.observations {
        view.camera.center_m += Vec3::new(3., 0., 2.);
    }
    assert_eq!(triangulate(&shifted).unwrap().status, "ACCEPTED");
    assert!(triangulate_with_reference_scale_noise(&shifted, &noise).is_err());
    let mut changed = json!(noise);
    changed["guessed_origin_m"] = json!([3., 0., 2.]);
    assert!(serde_json::from_value::<ReferenceScaleNoiseInput>(changed).is_err());
}

#[test]
fn unavailable_reference_scale_preserves_geometry_without_partial_precision() {
    let (input, mut noise) = reference_scale_fixture(2, Vec3::new(0.08, -0.2, 0.15));
    noise.sigma_fraction = 1e-200;
    let result = triangulate_with_reference_scale_noise(&input, &noise).unwrap();
    assert!(result.triangulation.point_m.is_some());
    let precision = result.conditional_reference_scale_uncertainty;
    assert_eq!(precision.status, "UNAVAILABLE");
    assert!(precision.covariance_m2.is_none());
    assert!(precision.sigma_m.is_none());
    assert!(precision.jacobian_m_per_fraction.is_none());

    noise.sigma_fraction = 0.01;
    let mut rejected = input;
    rejected.observations[2].pixel_px.x += 50.;
    let result = triangulate_with_reference_scale_noise(&rejected, &noise).unwrap();
    assert!(result.triangulation.point_m.is_none());
    assert_eq!(
        result.conditional_reference_scale_uncertainty.status,
        "UNAVAILABLE"
    );
    assert!(
        result
            .conditional_reference_scale_uncertainty
            .covariance_m2
            .is_none()
    );
}
