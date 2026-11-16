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


use ftx_measure::triangulate::distance::{self, Endpoint, Input as DistanceInput};

fn distance_fixture(fit_views: usize) -> DistanceInput {
    let (left, scale) = reference_scale_fixture(fit_views, Vec3::new(-0.1, -0.2, 0.15));
    let (right, other_scale) = reference_scale_fixture(fit_views, Vec3::new(0.1, -0.2, 0.15));
    assert_eq!(json!(scale), json!(other_scale));
    DistanceInput {
        schema: "ftx-triangulation-distance-input/1".into(),
        endpoints: [
            Endpoint {
                name: "left marker".into(),
                input: left,
                click_noise: None,
            },
            Endpoint {
                name: "right marker".into(),
                input: right,
                click_noise: None,
            },
        ],
        click_independence_evidence: None,
        lens_noise: None,
        sheet_noise: None,
        reference_scale_noise: Some(scale),
    }
}

#[test]
fn straight_distance_preserves_one_shared_scale_instead_of_independent_endpoint_errors() {
    for fit_views in [2, 3] {
        let input = distance_fixture(fit_views);
        let result = distance::run(&input).unwrap();
        assert_eq!(result.status, "ACCEPTED", "{:?}", result.flags);
        assert!((result.distance_m.unwrap() - 0.2).abs() < 1e-10);
        let precision = result.conditional_reference_scale_uncertainty.unwrap();
        assert_eq!(precision.status, "AVAILABLE", "{:?}", precision.flags);
        // One common scale error stretches the whole 0.2 m segment: 1% is 0.002 m.
        // Treating the two opposite endpoint errors as independent would halve this variance.
        assert!((precision.variance_m2.unwrap() - 4e-6).abs() < 1e-14);
        assert!((precision.sigma_m.unwrap() - 0.002).abs() < 1e-12);
        for (endpoint, original) in result.endpoints.iter().zip(&input.endpoints) {
            assert_eq!(
                json!(endpoint.triangulation),
                json!(triangulate(&original.input).unwrap()),
            );
        }
        assert!(result.conditional_click_uncertainty.is_none());
    }
}

#[test]
fn straight_distance_scale_is_invariant_to_a_translated_source_coordinate_origin() {
    let mut input = distance_fixture(2);
    let original = distance::run(&input).unwrap();
    let shift = Vec3::new(3., 0., 2.);
    for endpoint in &mut input.endpoints {
        for view in &mut endpoint.input.observations {
            view.camera.center_m += shift;
        }
    }
    for pose in &mut input.reference_scale_noise.as_mut().unwrap().sheet_poses {
        let mut source = pose.report["input"].clone();
        for corner in source["points"].as_array_mut().unwrap() {
            corner["sheet_xz_m"] = json!([
                corner["sheet_xz_m"][0].as_f64().unwrap() + shift.x,
                corner["sheet_xz_m"][1].as_f64().unwrap() + shift.z,
            ]);
        }
        pose.report = Camera::sheet_pose_report(&source).unwrap();
        assert_eq!(pose.report["ok"], true);
        let camera: Camera = serde_json::from_value(pose.report["camera"].clone()).unwrap();
        for endpoint in &mut input.endpoints {
            endpoint
                .input
                .observations
                .iter_mut()
                .find(|view| view.frame == pose.frame)
                .unwrap()
                .camera = camera;
        }
    }
    let translated = distance::run(&input).unwrap();
    assert_eq!(translated.status, "ACCEPTED");
    assert!((translated.distance_m.unwrap() - original.distance_m.unwrap()).abs() < 1e-9);
    assert!(
        (translated
            .conditional_reference_scale_uncertainty
            .unwrap()
            .sigma_m
            .unwrap()
            - original
                .conditional_reference_scale_uncertainty
                .unwrap()
                .sigma_m
                .unwrap())
        .abs()
            < 1e-11,
    );
    assert_ne!(translated.input_sha256, original.input_sha256);
}

#[test]
fn distance_replays_complete_scale_sources_and_preserves_unavailable_precision() {
    let input = distance_fixture(2);
    for variant in 0..4 {
        let mut changed = input.clone();
        let scale = changed.reference_scale_noise.as_mut().unwrap();
        match variant {
            0 => scale.reference_id.push_str("-different"),
            1 => scale.evidence.clear(),
            2 => {
                scale.sheet_poses.pop();
            }
            _ => scale.sheet_poses[0].report["ok"] = json!(false),
        }
        assert!(distance::run(&changed).is_err(), "source variant {variant}");
    }
    let mut small = input;
    small.reference_scale_noise.as_mut().unwrap().sigma_fraction = 1e-200;
    let report = distance::run(&small).unwrap();
    assert_eq!(report.status, "ACCEPTED");
    assert!(report.distance_m.is_some());
    let precision = report.conditional_reference_scale_uncertainty.unwrap();
    assert_eq!(precision.status, "UNAVAILABLE");
    assert!(precision.variance_m2.is_none());
    assert!(precision.sigma_m.is_none());
}

#[test]
fn distance_shared_sheet_precision_matches_joint_source_corner_refits() {
    use ftx_measure::triangulate::uncertainty::{FrameSheetCornerNoise, SheetCornerNoiseInput};
    let mut input = distance_fixture(2);
    let scale = input.reference_scale_noise.take().unwrap();
    input.sheet_noise = Some(SheetCornerNoiseInput {
        schema: "ftx-triangulation-sheet-noise/1".into(),
        independence_evidence:
            "Synthetic independent sheet corners; shared acquisitions for both endpoints".into(),
        observations: scale
            .sheet_poses
            .iter()
            .map(|pose| FrameSheetCornerNoise {
                frame: pose.frame.clone(),
                report: pose.report.clone(),
                sigma_px: 0.25,
            })
            .collect(),
    });
    let report = distance::run(&input).unwrap();
    let precision = report.conditional_sheet_uncertainty.unwrap();
    assert_eq!(precision.status, "AVAILABLE", "{:?}", precision.flags);

    // Independently perturb the original camera acquisition, re-solve both full landmarks
    // with that same camera and difference the resulting scalar length, not endpoint covariance.
    let mut expected = 0.;
    for original in &input.sheet_noise.as_ref().unwrap().observations {
        for (index, corner) in original.report["input"]["points"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            if corner["held_out"] == true {
                continue;
            }
            for axis in 0..2 {
                let sample = |step: f64| {
                    let mut source = original.report["input"].clone();
                    let pixel = source["points"][index]["pixel_px"][axis]
                        .as_f64()
                        .unwrap();
                    source["points"][index]["pixel_px"][axis] = json!(pixel + step);
                    let pose = Camera::sheet_pose_report(&source).unwrap();
                    assert_eq!(pose["ok"], true);
                    let camera: Camera = serde_json::from_value(pose["camera"].clone()).unwrap();
                    let points = input.endpoints.each_ref().map(|endpoint| {
                        let mut changed = endpoint.input.clone();
                        changed
                            .observations
                            .iter_mut()
                            .find(|view| view.frame == original.frame)
                            .unwrap()
                            .camera = camera;
                        triangulate(&changed).unwrap().point_m.unwrap()
                    });
                    (points[1] - points[0]).norm()
                };
                let derivative = (sample(0.001) - sample(-0.001)) / 0.002;
                expected += (derivative * original.sigma_px).powi(2);
            }
        }
    }
    assert!(expected > 0.);
    assert!((precision.variance_m2.unwrap() / expected - 1.).abs() < 1e-4);

    let mut mismatched = input;
    mismatched.reference_scale_noise = Some(scale);
    mismatched.sheet_noise.as_mut().unwrap().observations[0].report["unbound_annotation"] =
        json!("different original acquisition");
    assert!(distance::run(&mismatched).is_err());
}
