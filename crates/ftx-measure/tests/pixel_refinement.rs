//! Closed-form pixel optimum for parallel stereo with unequal focal lengths.
//! Stereo inputs/expected results are analytical; the distorted case checks stationarity, not parity.
use ftx_core::{Mat3, Vec2, Vec3};
use ftx_measure::camera::{Camera, Distortion, Intrinsics};
use ftx_measure::triangulate::{
    Capture, FrameId, Observation, Partition, TriangulationInput, triangulate,
};

fn request() -> TriangulationInput {
    let observations = [(-0.4, 400.0, 2.0), (0.4, 2000.0, -1.0)]
        .into_iter()
        .enumerate()
        .map(|(i, (x, focal, noise))| Observation {
            frame: FrameId {
                stream: "synthetic-stereo".into(),
                timestamp_us: i as u64,
            },
            camera: Camera {
                intrinsics: Intrinsics {
                    focal_px: Vec2::new(focal, focal),
                    principal_px: Vec2::new(500.0, 400.0),
                    distortion: Distortion::default(),
                },
                world_to_camera: Mat3::from_rows([
                    [1.0, 0.0, 0.0],
                    [0.0, 0.0, -1.0],
                    [0.0, 1.0, 0.0],
                ]),
                center_m: Vec3::new(x, -3.0, 1.0),
            },
            pixel_px: Vec2::new(500.0 - focal * x / 3.0, 400.0 - 0.1 * focal + noise),
            partition: Partition::Fit,
        })
        .collect();
    TriangulationInput {
        schema: "ftx-triangulation-input/1".into(),
        metric_scale_source: "Synthetic 0.8 m baseline".into(),
        capture: Capture::StillWindow {
            start_us: 0,
            end_us: 2,
            evidence: "Synthetic stationary point".into(),
        },
        observations,
    }
}

fn expected() -> Vec3 {
    // x observations fix X=0 and optical depth=3. Minimizing vertical pixel residuals gives
    // q_y = sum(f_i*(observed_y_i-c_y)) / sum(f_i^2); world z = 1 - 3*q_y.
    let qy = (400.0 * (-38.0) + 2000.0 * (-201.0)) / (400.0_f64.powi(2) + 2000.0_f64.powi(2));
    Vec3::new(0.0, 0.0, 1.0 - 3.0 * qy)
}

#[test]
fn unequal_focal_stereo_reaches_the_closed_form_pixel_optimum() {
    let input = request();
    let report = triangulate(&input).unwrap();
    assert_eq!(report.status, "ACCEPTED", "{:?}", report.flags);
    assert!((report.point_m.unwrap() - expected()).norm() < 1e-9);
    let refinement = report.refinement.unwrap();
    assert!(refinement.converged);
    assert!(refinement.iterations > 0 && refinement.iterations <= 30);
    assert!(refinement.final_cost_px2 < 0.2 * refinement.initial_cost_px2);
    let optimum = (400.0_f64 * ((1.0 - expected().z) / 3.0) + 38.0).powi(2)
        + (2000.0_f64 * ((1.0 - expected().z) / 3.0) + 201.0).powi(2);
    assert!((refinement.final_cost_px2 - optimum).abs() < 1e-10);
    assert!(
        report
            .reprojection
            .iter()
            .all(|r| r.error_px.unwrap() <= 3.0)
    );
}

#[test]
fn held_out_errors_never_change_optimization_and_still_gate_the_result() {
    let mut input = request();
    let original = triangulate(&input).unwrap();
    let mut view = input.observations[0].clone();
    view.frame.timestamp_us = 2;
    view.partition = Partition::HeldOut;
    view.pixel_px = Vec2::new(
        500.0 + 400.0 * 0.4 / 3.0,
        400.0 + 400.0 * (1.0 - expected().z) / 3.0,
    );
    input.observations.push(view);
    let checked = triangulate(&input).unwrap();
    assert_eq!(checked.status, "ACCEPTED", "{:?}", checked.flags);
    assert_eq!(checked.point_m, original.point_m);
    let cost = checked.refinement.unwrap().final_cost_px2;
    input.observations[2].pixel_px.y += 4.0;
    let rejected = triangulate(&input).unwrap();
    assert_eq!(rejected.status, "REJECTED");
    assert!(rejected.point_m.is_none());
    assert_eq!(rejected.refinement.unwrap().final_cost_px2, cost);
    assert!(rejected.reprojection[2].error_px.unwrap() > 3.9);
}

#[test]
fn noisy_refinement_is_metric_frame_invariant_and_deterministic() {
    let mut input = request();
    let original = triangulate(&input).unwrap();
    input.observations.reverse();
    assert_eq!(
        serde_json::to_value(&original).unwrap(),
        serde_json::to_value(triangulate(&input).unwrap()).unwrap()
    );
    let rotation = Mat3::rotation(Vec3::new(1.0, 2.0, -0.5).normalized(), 0.31);
    let shift = Vec3::new(1200.0, -900.0, 40.0);
    for scale in [0.001, 2.5, 1000.0] {
        let mut transformed = input.clone();
        for view in &mut transformed.observations {
            view.camera.center_m = rotation.mul_vec(view.camera.center_m) * scale + shift;
            view.camera.world_to_camera = view.camera.world_to_camera * rotation.transpose();
        }
        let report = triangulate(&transformed).unwrap();
        assert_eq!(report.status, "ACCEPTED", "{:?}", report.flags);
        let target = rotation.mul_vec(expected()) * scale + shift;
        assert!((report.point_m.unwrap() - target).norm() < 1e-7);
        assert!(
            (report.refinement.unwrap().final_cost_px2
                - original.refinement.as_ref().unwrap().final_cost_px2)
                .abs()
                < 1e-6
        );
    }
}

#[test]
fn distorted_noisy_fit_is_stationary_under_independent_pixel_differences() {
    let mut input = request();
    let truth = Vec3::new(0.2, 0.1, 1.5);
    for (i, view) in input.observations.iter_mut().enumerate() {
        view.camera.intrinsics.distortion = Distortion {
            radial: [0.15, 0.004, 0.001],
            tangential: [0.002, -0.003],
            max_radius: 2.0,
        };
        view.pixel_px =
            view.camera.project(truth).unwrap() + Vec2::new(0.3, if i == 0 { 2.0 } else { -1.0 });
    }
    let report = triangulate(&input).unwrap();
    assert_eq!(report.status, "ACCEPTED", "{:?}", report.flags);
    let point = report.point_m.unwrap();
    let cost = |p| {
        input
            .observations
            .iter()
            .map(|view| (view.camera.project(p).unwrap() - view.pixel_px).norm2())
            .sum::<f64>()
    };
    assert!(report.refinement.as_ref().unwrap().iterations > 0);
    assert!((cost(point) - report.refinement.unwrap().final_cost_px2).abs() < 1e-9);
    for k in 0..3 {
        let delta = Vec3::from(std::array::from_fn(|i| if i == k { 1e-6 } else { 0.0 }));
        let derivative = (cost(point + delta) - cost(point - delta)) / 2e-6;
        assert!(
            derivative.abs() < 1e-3,
            "nonstationary axis {k}: {derivative}"
        );
    }
}
