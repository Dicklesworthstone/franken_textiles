//! Synthetic stationary targets and exact calibrated camera poses; no private measurements.
use ftx_core::{Mat3, Vec2, Vec3};
use ftx_measure::{
    bundle::{BundleInput, Observation, Point, View, adjust},
    camera::{Camera, Distortion, Intrinsics},
    triangulate::{Capture, FrameId, Partition},
};

fn scene() -> (BundleInput, Vec<View>, Vec<Point>) {
    let views: Vec<_> = (0..3)
        .map(|i| View {
            frame: FrameId {
                stream: "synthetic-still".into(),
                timestamp_us: 100_000 + i * 50_000,
            },
            camera: Camera {
                intrinsics: Intrinsics {
                    focal_px: Vec2::new(1100.0, 1080.0),
                    principal_px: Vec2::new(640.0, 480.0),
                    distortion: Distortion::default(),
                },
                world_to_camera: Mat3::from_rows([
                    [1.0, 0.0, 0.0],
                    [0.0, 0.0, -1.0],
                    [0.0, 1.0, 0.0],
                ]),
                center_m: Vec3::new(
                    -0.5 + 0.5 * i as f64,
                    -2.0 - 0.1 * i as f64,
                    1.0 + 0.1 * i as f64,
                ),
            },
        })
        .collect();
    let controls: Vec<_> = (0..9)
        .map(|i| Point {
            id: format!("c{i}"),
            position_m: Vec3::new(-0.3 + 0.3 * (i % 3) as f64, 0.0, 0.6 + 0.3 * (i / 3) as f64),
        })
        .collect();
    let landmarks = vec![
        Point {
            id: "a".into(),
            position_m: Vec3::new(-0.1, -0.3, 1.25),
        },
        Point {
            id: "b".into(),
            position_m: Vec3::new(0.2, -0.2, 0.8),
        },
        Point {
            id: "c".into(),
            position_m: Vec3::new(0.05, 0.15, 1.1),
        },
    ];
    let mut observations = Vec::new();
    for (i, v) in views.iter().enumerate() {
        for (p, partition) in
            controls
                .iter()
                .map(|p| (p, Partition::Fit))
                .chain(landmarks.iter().map(|p| {
                    (
                        p,
                        if i == 2 {
                            Partition::HeldOut
                        } else {
                            Partition::Fit
                        },
                    )
                }))
        {
            // Direct pinhole answer in the explicitly chosen z-up world frame, not generated
            // by the optimizer, its Jacobian or a candidate-based oracle capture.
            let d = p.position_m - v.camera.center_m;
            let pixel = Vec2::new(640.0 + 1100.0 * d.x / d.y, 480.0 - 1080.0 * d.z / d.y);
            observations.push(Observation {
                frame: v.frame.clone(),
                point: p.id.clone(),
                pixel_px: pixel,
                partition,
                sigma_px: 1.0,
            });
        }
    }
    let mut input = BundleInput {
        schema: "ftx-bundle-input/1".into(),
        metric_scale_source: "synthetic stationary metric grid".into(),
        capture: Capture::StillWindow {
            start_us: 100_000,
            end_us: 200_000,
            evidence: "analytically stationary synthetic scene".into(),
        },
        views: views.clone(),
        controls,
        landmarks: landmarks.clone(),
        observations,
    };
    for (i, v) in input.views.iter_mut().enumerate() {
        v.camera.center_m += Vec3::new(0.012, -0.009, 0.006) * (i + 1) as f64;
        v.camera.world_to_camera = v.camera.world_to_camera
            * Mat3::rotation(
                Vec3::new(1.0, -0.3, 0.2).normalized(),
                0.01 * (i + 1) as f64,
            );
    }
    for p in &mut input.landmarks {
        p.position_m += Vec3::new(0.02, -0.03, 0.01);
    }
    (input, views, landmarks)
}

#[test]
fn jointly_recovers_all_camera_poses_and_off_sheet_landmarks() {
    let (input, views, points) = scene();
    let before = serde_json::to_value(&input).unwrap();
    let report = adjust(&input, 50).unwrap();
    assert_eq!(
        report.status, "ACCEPTED",
        "{:?} / {:?}",
        report.flags, report.objective_history
    );
    assert_eq!(serde_json::to_value(&input).unwrap(), before);
    assert!(report.iterations > 1);
    assert!(report.objective_history.windows(2).all(|v| v[1] < v[0]));
    assert!(*report.objective_history.last().unwrap() < 1e-12);
    assert!(
        report.checks.iter().all(|c| c.error_px.unwrap() < 1e-7),
        "max residual {} / {:?}",
        report
            .checks
            .iter()
            .map(|c| c.error_px.unwrap())
            .fold(0.0, f64::max),
        report.objective_history
    );
    let solution = report.solution.unwrap();
    for (actual, expected) in solution.views.iter().zip(views) {
        assert!((actual.camera.center_m - expected.camera.center_m).norm() < 1e-8);
        assert!(
            actual
                .camera
                .world_to_camera
                .rows
                .iter()
                .flatten()
                .zip(expected.camera.world_to_camera.rows.iter().flatten())
                .all(|(a, b)| (a - b).abs() < 1e-8)
        );
        assert_eq!(
            serde_json::to_value(actual.camera.intrinsics).unwrap(),
            serde_json::to_value(expected.camera.intrinsics).unwrap()
        );
    }
    for (actual, expected) in solution.landmarks.iter().zip(points) {
        assert!((actual.position_m - expected.position_m).norm() < 1e-8);
    }
}

#[test]
fn independent_observations_cannot_steer_geometry_and_can_reject_it() {
    let (mut input, _, _) = scene();
    let original = adjust(&input, 50).unwrap();
    input
        .observations
        .iter_mut()
        .filter(|o| o.partition == Partition::HeldOut)
        .for_each(|o| o.pixel_px.x += 0.5);
    let small = adjust(&input, 50).unwrap();
    assert_eq!(small.status, "ACCEPTED", "{:?}", small.flags);
    assert_eq!(
        serde_json::to_value(&small.solution).unwrap(),
        serde_json::to_value(&original.solution).unwrap()
    );
    assert_eq!(small.objective_history, original.objective_history);
    input
        .observations
        .iter_mut()
        .find(|o| o.partition == Partition::HeldOut)
        .unwrap()
        .pixel_px
        .y += 4.0;
    let failed = adjust(&input, 50).unwrap();
    assert_eq!(failed.status, "REJECTED");
    assert!(failed.solution.is_none());
    assert_eq!(failed.objective_history, original.objective_history);
    assert_eq!(failed.checks.len(), input.observations.len());
}

#[test]
fn fit_weights_matter_but_cannot_bypass_the_unweighted_pixel_gate() {
    let (mut input, _, _) = scene();
    // Many small pixel errors leave an overdetermined, genuinely noisy bundle.
    for (i, o) in input
        .observations
        .iter_mut()
        .enumerate()
        .filter(|(_, o)| o.partition == Partition::Fit)
    {
        o.pixel_px += Vec2::new(0.12 * ((i % 3) as f64 - 1.0), 0.08 * ((i % 5) as f64 - 2.0));
    }
    let report = adjust(&input, 50).unwrap();
    assert_eq!(
        report.status, "ACCEPTED",
        "{:?} / {:?}",
        report.flags, report.objective_history
    );
    assert!(*report.objective_history.last().unwrap() > 0.01);
    assert!(report.objective_history.windows(2).all(|v| v[1] < v[0]));
    let held = input
        .observations
        .iter_mut()
        .find(|o| o.partition == Partition::HeldOut)
        .unwrap();
    held.pixel_px.x += 10.0;
    held.sigma_px = 1e6;
    let bad = adjust(&input, 50).unwrap();
    assert_eq!(bad.status, "REJECTED");
    assert!(bad.solution.is_none());
}

#[test]
fn unobservable_controls_and_landmarks_cannot_be_rescued_by_damping_or_holdouts() {
    let (input, _, _) = scene();
    let mut too_few = input.clone();
    for o in &mut too_few.observations {
        if o.point.starts_with('c') && o.point.len() == 2 && o.point != "c0" {
            o.partition = Partition::HeldOut;
        }
    }
    assert_eq!(adjust(&too_few, 50).unwrap().status, "UNOBSERVABLE");
    let mut collinear = input.clone();
    for (i, p) in collinear.controls.iter_mut().enumerate() {
        p.position_m = Vec3::new(-0.3 + i as f64 * 0.08, 0.0, 1.0);
    }
    // Four-plus observations are not a rank certificate: points along one line leave a pose DOF.
    for o in &mut collinear.observations {
        if let Some(p) = collinear.controls.iter().find(|p| p.id == o.point) {
            let c = &collinear
                .views
                .iter()
                .find(|v| v.frame == o.frame)
                .unwrap()
                .camera;
            o.pixel_px = c.project(p.position_m).unwrap();
        }
    }
    let report = adjust(&collinear, 50).unwrap();
    assert_eq!(report.status, "UNOBSERVABLE", "{:?}", report.flags);
    assert!(report.solution.is_none());
    let mut single = input;
    for o in &mut single.observations {
        if o.point == "a" && o.frame.timestamp_us > 100_000 {
            o.partition = Partition::HeldOut;
        }
    }
    assert_eq!(adjust(&single, 50).unwrap().status, "UNOBSERVABLE");
}

#[test]
fn budget_stops_never_publish_a_partly_refined_scene() {
    let (input, _, _) = scene();
    let report = adjust(&input, 1).unwrap();
    assert_eq!(report.status, "BUDGET_STOP");
    assert!(report.solution.is_none());
    assert_eq!(report.iterations, 1);
    assert_eq!(report.objective_history.len(), 2);
    assert!(adjust(&input, 0).is_err());
    assert!(adjust(&input, 101).is_err());
}

#[test]
fn canonical_order_and_metric_gauge_are_preserved() {
    let (mut input, _, points) = scene();
    let report = adjust(&input, 50).unwrap();
    input.views.reverse();
    input.controls.reverse();
    input.landmarks.reverse();
    input.observations.reverse();
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::to_value(adjust(&input, 50).unwrap()).unwrap()
    );
    let r = Mat3::rotation(Vec3::new(0.0, 0.0, 1.0), 0.2);
    let shift = Vec3::new(120.0, -80.0, 1.0);
    for scale in [0.02, 2.5, 100.0] {
        let mut transformed = input.clone();
        for p in transformed
            .controls
            .iter_mut()
            .chain(&mut transformed.landmarks)
        {
            p.position_m = r.mul_vec(p.position_m) * scale + shift;
        }
        for v in &mut transformed.views {
            v.camera.center_m = r.mul_vec(v.camera.center_m) * scale + shift;
            v.camera.world_to_camera = v.camera.world_to_camera * r.transpose();
        }
        let report = adjust(&transformed, 50).unwrap();
        assert_eq!(report.status, "ACCEPTED", "{:?}", report.flags);
        for (p, target) in report.solution.unwrap().landmarks.iter().zip(&points) {
            assert!(
                (p.position_m - (r.mul_vec(target.position_m) * scale + shift)).norm() < 1e-6,
                "scale {scale}: error {}",
                (p.position_m - (r.mul_vec(target.position_m) * scale + shift)).norm()
            );
        }
    }
}

#[test]
fn malformed_calibration_identity_capture_and_schema_are_refused() {
    let (input, _, _) = scene();
    for mutate in [
        |v: &mut BundleInput| v.observations.push(v.observations[0].clone()),
        |v: &mut BundleInput| v.views.push(v.views[0].clone()),
        |v: &mut BundleInput| v.landmarks[0].id = v.controls[0].id.clone(),
        |v: &mut BundleInput| v.controls[1].position_m = v.controls[0].position_m,
        |v: &mut BundleInput| v.observations[0].point = "missing".into(),
        |v: &mut BundleInput| v.observations[0].sigma_px = 0.0,
        |v: &mut BundleInput| v.landmarks[0].position_m.x = f64::NAN,
        |v: &mut BundleInput| v.views[0].camera.world_to_camera.rows[0][0] = -1.0,
        |v: &mut BundleInput| v.views[0].frame.timestamp_us = 1,
        |v: &mut BundleInput| v.metric_scale_source = " ".into(),
    ] {
        let mut v = input.clone();
        mutate(&mut v);
        assert!(adjust(&v, 50).is_err());
    }
    let mut json = serde_json::to_value(&input).unwrap();
    json["observations"][0]["sigam_px"] = serde_json::json!(1.0);
    assert!(serde_json::from_value::<BundleInput>(json).is_err());
}
