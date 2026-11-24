//! Analytical articulated geometry; not a human-data fixture or an Anny parity claim.
use ftx_core::{Mat3, Refusal, Vec2, Vec3};
use ftx_measure::{
    body_fit::*,
    camera::{Camera, Distortion, Intrinsics},
    triangulate::{FrameId, Partition},
};

struct HingedBody {
    layout: ModelLayout,
}
fn parameter(name: &str, lower: f64, upper: f64) -> Parameter {
    Parameter {
        name: name.into(),
        lower,
        upper,
    }
}
impl HingedBody {
    fn new() -> Self {
        Self {
            layout: ModelLayout {
                model_id: "synthetic-hinged-body/1".into(),
                shape: vec![parameter("width", 0.2, 0.9), parameter("height", 0.6, 2.0)],
                pose: vec![
                    parameter("x", -2.0, 2.0),
                    parameter("y", -2.0, 2.0),
                    parameter("z", -1.0, 1.0),
                    parameter("arm", -1.5, 1.5),
                ],
                keypoints: (0..10).map(|i| format!("p{i}")).collect(),
                measurements: vec!["stature".into()],
            },
        }
    }
}
impl BodyModel for HingedBody {
    fn layout(&self) -> &ModelLayout {
        &self.layout
    }
    fn measurements(&self, s: &[f64]) -> Result<Vec<f64>, Refusal> {
        Ok(vec![s[1]])
    }
    fn points(&self, s: &[f64], p: &[f64]) -> Result<Vec<Vec3>, Refusal> {
        let root = Vec3::new(p[0], p[1], p[2]);
        let (w, h) = (s[0], s[1]);
        let mut points = Vec::new();
        for z in [0.0, h] {
            for y in [-0.15 * w, 0.15 * w] {
                for x in [-0.5 * w, 0.5 * w] {
                    points.push(root + Vec3::new(x, y, z));
                }
            }
        }
        let shoulder = Vec3::new(0.5 * w, 0.0, 0.75 * h);
        let r = Mat3::rotation(Vec3::new(0.0, 1.0, 0.0), p[3]);
        points.push(root + shoulder + r.mul_vec(Vec3::new(0.35 * h, 0.0, 0.0)));
        points.push(root + shoulder + r.mul_vec(Vec3::new(0.2 * h, 0.02 * w, -0.03 * h)));
        Ok(points)
    }
}

fn scene() -> (HingedBody, BodyFitInput, Vec<Vec<f64>>) {
    let model = HingedBody::new();
    let shape = vec![0.48, 1.25];
    let poses = vec![vec![-0.2, 0.1, 0.1, -0.4], vec![0.3, -0.2, 0.05, 0.65]];
    let frames: Vec<_> = poses
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let camera = Camera {
                intrinsics: Intrinsics {
                    focal_px: Vec2::new(1000.0, 1050.0),
                    principal_px: Vec2::new(640.0, 480.0),
                    distortion: Distortion {
                        radial: [0.05, 0.001, 0.0],
                        tangential: [0.001, -0.002],
                        max_radius: 2.0,
                    },
                },
                world_to_camera: Mat3::from_rows([
                    [1.0, 0.0, 0.0],
                    [0.0, 0.0, -1.0],
                    [0.0, 1.0, 0.0],
                ]),
                center_m: Vec3::new(0.1 * i as f64, -4.0, 0.75),
            };
            FitFrame {
                frame: FrameId {
                    stream: "synthetic-moving-sequence".into(),
                    timestamp_us: i as u64 * 200000,
                },
                camera,
                image_size_px: [1280, 960],
                rotation_cw_deg: 0,
                silhouette: None,
                fixed_pose: None,
                keypoints: model
                    .points(&shape, p)
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(k, &v)| Keypoint {
                        name: format!("p{k}"),
                        pixel_px: camera.project(v).unwrap(),
                        sigma_px: 1.0,
                        partition: if k == 3 {
                            Partition::HeldOut
                        } else {
                            Partition::Fit
                        },
                    })
                    .collect(),
            }
        })
        .collect();
    let starts = [(-0.04, 0.08, 0.10), (0.03, -0.06, -0.08)]
        .into_iter()
        .map(|(dw, dh, dp)| FitStart {
            shape: vec![shape[0] + dw, shape[1] + dh],
            poses: frames
                .iter()
                .zip(&poses)
                .map(|(f, p)| FramePose {
                    frame: f.frame.clone(),
                    values: p.iter().map(|v| v + dp).collect(),
                })
                .collect(),
        })
        .collect();
    let input = BodyFitInput {
        schema: "ftx-body-fit-input/1".into(),
        metric_scale_source: "synthetic calibrated metric cameras".into(),
        observation_evidence: "exact synthetic articulated correspondences".into(),
        frames,
        starts,
        measurements: vec![MetricObservation {
            name: "stature".into(),
            value_m: shape[1],
            sigma_m: 0.002,
            evidence: "synthetic exact rest height".into(),
            partition: Partition::Fit,
        }],
    };
    (model, input, poses)
}

#[test]
fn full_mesh_review_preserves_observation_roles_and_refuses_failed_projections() {
    let (model, input, poses) = scene();
    let points = model.points(&[0.48, 1.25], &poses[0]).unwrap();
    let mut frame = input.frames[0].clone();
    frame.keypoints.reverse();
    let review = review_keypoints(&frame, &model.layout.keypoints, &points).unwrap();
    for (observed, projected) in frame.keypoints.iter().zip(&review) {
        assert_eq!(projected.name, observed.name);
        assert_eq!(projected.partition, observed.partition);
        assert_eq!(projected.sigma_px, observed.sigma_px);
        assert_eq!(projected.observed_px, observed.pixel_px);
        assert_eq!(projected.predicted_px, observed.pixel_px);
        assert_eq!(projected.residual_px, 0.0);
        let index = model
            .layout
            .keypoints
            .iter()
            .position(|n| n == &observed.name)
            .unwrap();
        assert_eq!(projected.world_m, points[index]);
    }
    for role in [Partition::Fit, Partition::HeldOut] {
        let mut changed = frame.clone();
        let observation = changed
            .keypoints
            .iter_mut()
            .find(|p| p.partition == role)
            .unwrap();
        observation.pixel_px.x += 3.0;
        assert!(review_keypoints(&changed, &model.layout.keypoints, &points).is_ok());
        changed
            .keypoints
            .iter_mut()
            .find(|p| p.partition == role)
            .unwrap()
            .pixel_px
            .x += 0.01;
        assert!(review_keypoints(&changed, &model.layout.keypoints, &points).is_err());
    }
    assert!(review_keypoints(&frame, &model.layout.keypoints, &points[..9]).is_err());
    let mut behind_camera = points;
    behind_camera[0] = frame.camera.center_m;
    assert!(review_keypoints(&frame, &model.layout.keypoints, &behind_camera).is_err());
    frame.keypoints[0].name = "unknown".into();
    assert!(review_keypoints(&frame, &model.layout.keypoints, &behind_camera).is_err());
}

#[test]
fn first_capture_drafts_retain_validation_without_becoming_fittable() {
    let (mut model, mut input, _) = scene();
    let point = input.frames[0].keypoints[0].clone();
    model.layout.keypoints.clear();
    input.starts.clear();
    for frame in &mut input.frames {
        frame.keypoints.clear();
    }
    let prepared = prepare_capture_authoring(&input, &model.layout).unwrap();
    assert_eq!(
        serde_json::to_value(&prepared).unwrap(),
        serde_json::to_value(&input).unwrap()
    );
    assert!(prepare_start_authoring(&input, &model.layout).is_err());
    assert!(prepare_input(&input, &model.layout).is_err());
    assert!(fit_body(&input, &model, &FitOptions::default()).is_err());

    let mut invalid = input.clone();
    invalid.frames[0].keypoints.push(point);
    assert!(prepare_capture_authoring(&invalid, &model.layout).is_err());
    invalid = input.clone();
    invalid.measurements[0].sigma_m = 0.0;
    assert!(prepare_capture_authoring(&invalid, &model.layout).is_err());
    invalid = input.clone();
    invalid.metric_scale_source.clear();
    assert!(prepare_capture_authoring(&invalid, &model.layout).is_err());
    invalid = input.clone();
    invalid.frames[0].camera.intrinsics.focal_px.x = 0.0;
    assert!(prepare_capture_authoring(&invalid, &model.layout).is_err());

    model.layout.pose = (0..18)
        .map(|i| parameter(&format!("p{i}"), -1.0, 1.0))
        .collect();
    input.frames = (0..8)
        .map(|i| {
            let mut frame = prepared.frames[0].clone();
            frame.frame.timestamp_us = i * 200_000;
            frame
        })
        .collect();
    assert!(prepare_capture_authoring(&input, &model.layout).is_err());
}

#[test]
fn fixed_pose_views_recover_only_shape_and_retain_every_evidence_gate() {
    struct FixedPoseBody {
        body: HingedBody,
        pose: [f64; 4],
    }
    impl BodyModel for FixedPoseBody {
        fn layout(&self) -> &ModelLayout {
            &self.body.layout
        }
        fn measurements(&self, shape: &[f64]) -> Result<Vec<f64>, Refusal> {
            self.body.measurements(shape)
        }
        fn points(&self, shape: &[f64], pose: &[f64]) -> Result<Vec<Vec3>, Refusal> {
            assert!(pose.is_empty(), "no synthetic free pose control");
            self.body.points(shape, &self.pose)
        }
    }

    let (mut body, mut input, _) = scene();
    body.layout.pose.clear();
    let mut model = FixedPoseBody {
        body,
        pose: [-0.2, 0.1, 0.1, -0.4],
    };
    let truth = [0.48, 1.25];
    let points = model.points(&truth, &[]).unwrap();
    for frame in &mut input.frames {
        for (observation, point) in frame.keypoints.iter_mut().zip(&points) {
            observation.pixel_px = frame.camera.project(*point).unwrap();
        }
    }
    for start in &mut input.starts {
        for pose in &mut start.poses {
            pose.values.clear();
        }
    }
    // Calibrated views of a fixed metric pose constrain these two dimensions without a tape prior.
    input.measurements.clear();
    let report = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(report.status, "ACCEPTED", "{report:#?}");
    let solution = report.solution.as_ref().unwrap();
    for (actual, expected) in solution.shape.iter().zip(truth) {
        assert!((actual - expected).abs() < 1e-7, "{solution:?}");
    }
    assert!(report.model.pose.is_empty());
    for (pose, frame) in solution.poses.iter().zip(&input.frames) {
        assert_eq!(pose.frame, frame.frame);
        assert!(pose.values.is_empty());
    }
    assert!(
        report
            .pose_spread_fraction
            .as_ref()
            .unwrap()
            .iter()
            .all(|pose| pose.fractions.is_empty())
    );
    assert!(
        report
            .advisories
            .iter()
            .any(|message| message.contains("No pose parameters were fitted"))
    );
    assert!(report.attempts.iter().all(|attempt| {
        attempt.status == "CONVERGED"
            && attempt.checks.iter().all(|check| check.passed)
            && attempt
                .cost_history
                .windows(2)
                .all(|pair| pair[1] < pair[0])
    }));

    let mut invalid = input.clone();
    invalid.starts[1] = invalid.starts[0].clone();
    assert!(prepare_input(&invalid, model.layout()).is_err());
    invalid = input.clone();
    invalid.starts[0].poses.pop();
    assert!(prepare_input(&invalid, model.layout()).is_err());
    invalid = input.clone();
    invalid.starts[0].poses[0].values.push(0.0);
    assert!(prepare_input(&invalid, model.layout()).is_err());

    let mut held_out = input.clone();
    held_out.frames[0].keypoints[3].pixel_px.x += 4.0;
    let rejected = fit_body(&held_out, &model, &FitOptions::default()).unwrap();
    assert_eq!(rejected.status, "REJECTED");
    assert!(rejected.solution.is_none());
    for (base, changed) in report.attempts.iter().zip(&rejected.attempts) {
        assert_eq!(base.cost_history, changed.cost_history);
    }

    let mut unobserved = input.clone();
    for frame in &mut unobserved.frames {
        for point in &mut frame.keypoints {
            point.partition = Partition::HeldOut;
        }
    }
    let unobservable = fit_body(&unobserved, &model, &FitOptions::default()).unwrap();
    assert_eq!(unobservable.status, "UNOBSERVABLE");
    assert!(unobservable.solution.is_none());

    let stopped = fit_body(
        &input,
        &model,
        &FitOptions {
            max_model_evaluations: 1,
            ..FitOptions::default()
        },
    )
    .unwrap();
    assert_eq!(stopped.status, "BUDGET_STOP");
    assert!(stopped.solution.is_none());

    // Fixed does not mean known-correct: incompatible articulated placement must still fail.
    model.pose[3] = 0.65;
    let wrong_pose = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_ne!(wrong_pose.status, "ACCEPTED");
    assert!(wrong_pose.solution.is_none());
}

#[test]
fn shared_shape_and_distinct_articulated_poses_are_recovered() {
    let (model, input, poses) = scene();
    let r = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(r.status, "ACCEPTED", "{r:#?}");
    let s = r.solution.unwrap();
    for (a, b) in s.shape.iter().zip([0.48, 1.25]) {
        assert!((a - b).abs() < 1e-7, "{s:?}");
    }
    for (p, target) in s.poses.iter().zip(poses) {
        for (a, b) in p.values.iter().zip(target) {
            assert!((a - b).abs() < 1e-6, "{s:?}");
        }
    }
    assert!(r.shape_spread_fraction.unwrap().iter().all(|v| *v < 1e-6));
    for a in r.attempts {
        assert!(a.cost_history.windows(2).all(|w| w[1] < w[0]));
        assert!(a.checks.iter().all(|c| c.passed));
    }
}

#[test]
fn scale_depth_ambiguity_cannot_be_hidden_by_damping_or_held_out_tape() {
    let (model, mut input, _) = scene();
    input.measurements[0].partition = Partition::HeldOut;
    let r = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(r.status, "UNOBSERVABLE", "{r:#?}");
    assert!(r.solution.is_none());
    assert!(r.attempts.iter().all(|a| a.status == "UNOBSERVABLE"));
}

#[test]
fn held_out_pixels_never_steer_the_solution_and_still_gate_it() {
    let (model, mut input, _) = scene();
    let base = fit_body(&input, &model, &FitOptions::default()).unwrap();
    input.frames[0].keypoints[3].pixel_px.x += 2.0;
    let good = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(good.status, "ACCEPTED");
    assert_eq!(
        serde_json::to_value(&base.solution).unwrap(),
        serde_json::to_value(&good.solution).unwrap()
    );
    input.frames[0].keypoints[3].pixel_px.x += 2.0;
    let bad = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(bad.status, "REJECTED");
    assert!(bad.solution.is_none());
    for (a, b) in base.attempts.iter().zip(bad.attempts) {
        assert_eq!(a.cost_history, b.cost_history);
        assert!(b.checks.iter().any(|c| !c.passed));
    }
}

#[test]
fn noisy_data_reduces_the_actual_fit_objective() {
    let (model, mut input, _) = scene();
    for (f, frame) in input.frames.iter_mut().enumerate() {
        for (k, p) in frame.keypoints.iter_mut().enumerate() {
            p.pixel_px += Vec2::new(
                ((k + f) % 3) as f64 * 0.15 - 0.15,
                if k % 2 == 0 { 0.12 } else { -0.12 },
            );
        }
    }
    let r = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(r.status, "ACCEPTED", "{r:#?}");
    assert!((r.solution.unwrap().shape[0] - 0.48).abs() < 0.002);
    for a in r.attempts {
        assert!(a.cost_history.last().unwrap() < &(a.cost_history[0] * 0.01));
    }
}

#[test]
fn canonical_ordering_preserves_pose_identity_and_every_result() {
    let (model, mut input, _) = scene();
    let base = fit_body(&input, &model, &FitOptions::default()).unwrap();
    input.frames.reverse();
    for frame in &mut input.frames {
        frame.keypoints.reverse();
    }
    for start in &mut input.starts {
        start.poses.reverse();
    }
    assert_eq!(
        serde_json::to_value(base).unwrap(),
        serde_json::to_value(fit_body(&input, &model, &FitOptions::default()).unwrap()).unwrap()
    );
}

#[test]
fn failed_starts_and_budget_stops_do_not_publish_geometry() {
    let (model, mut input, _) = scene();
    let r = fit_body(
        &input,
        &model,
        &FitOptions {
            iterations: 1,
            ..FitOptions::default()
        },
    )
    .unwrap();
    assert_eq!(r.status, "BUDGET_STOP");
    assert!(r.solution.is_none());
    assert_eq!(r.attempts.len(), 2);
    input.starts[1].poses[0].values[1] = -1.99;
    input.starts[1].poses[0].values[2] = -0.99;
    let r = fit_body(
        &input,
        &model,
        &FitOptions {
            iterations: 1,
            ..FitOptions::default()
        },
    )
    .unwrap();
    assert!(r.solution.is_none());
    assert_eq!(r.attempts.len(), 2);
}

#[test]
fn malformed_evidence_unknown_correspondences_duplicate_starts_and_bounds_refuse() {
    let (model, input, _) = scene();
    for modify in [
        |x: &mut BodyFitInput| x.schema = "unknown".into(),
        |x: &mut BodyFitInput| x.frames[1].frame = x.frames[0].frame.clone(),
        |x: &mut BodyFitInput| x.frames[0].keypoints[0].name = "guessed-nose".into(),
        |x: &mut BodyFitInput| x.frames[0].keypoints[0].sigma_px = f64::NAN,
        |x: &mut BodyFitInput| x.frames[0].rotation_cw_deg = 45,
        |x: &mut BodyFitInput| x.measurements[0].evidence = "".into(),
        |x: &mut BodyFitInput| x.starts[1] = x.starts[0].clone(),
        |x: &mut BodyFitInput| x.starts[0].shape[0] = 0.2,
        |x: &mut BodyFitInput| x.starts[0].poses[0].frame.timestamp_us = 42,
    ] {
        let mut x = input.clone();
        modify(&mut x);
        assert!(fit_body(&x, &model, &FitOptions::default()).is_err());
    }
    let mut value = serde_json::to_value(input).unwrap();
    value["frames"][0]["keypoints"][0]["sigam_px"] = serde_json::json!(1);
    assert!(serde_json::from_value::<BodyFitInput>(value).is_err());
}

struct TwoShapes(HingedBody);
impl BodyModel for TwoShapes {
    fn layout(&self) -> &ModelLayout {
        self.0.layout()
    }
    fn measurements(&self, s: &[f64]) -> Result<Vec<f64>, Refusal> {
        self.0.measurements(s)
    }
    fn points(&self, s: &[f64], p: &[f64]) -> Result<Vec<Vec3>, Refusal> {
        // Distinct shape parameters give identical geometry, with full LOCAL rank at either root.
        let width = 0.4 + (s[0] - 0.55) * (s[0] - 0.55);
        self.0.points(&[width, s[1]], p)
    }
}

#[test]
fn distinct_shape_minima_are_ambiguous_even_with_full_local_rank() {
    let (base, mut input, poses) = scene();
    let model = TwoShapes(base);
    for (frame, pose) in input.frames.iter_mut().zip(&poses) {
        let points = model.points(&[0.35, 1.25], pose).unwrap();
        for (k, obs) in frame.keypoints.iter_mut().enumerate() {
            obs.pixel_px = frame.camera.project(points[k]).unwrap();
        }
    }
    input.starts[0].shape[0] = 0.32;
    input.starts[1].shape[0] = 0.78;
    let r = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(r.status, "AMBIGUOUS", "{r:#?}");
    assert!(r.solution.is_none());
    assert!(r.attempts.iter().all(|a| a.status == "CONVERGED"));
    assert!(r.shape_spread_fraction.unwrap()[0] > 0.5);
}

struct CountedBody {
    body: HingedBody,
    calls: std::cell::Cell<usize>,
}
impl BodyModel for CountedBody {
    fn layout(&self) -> &ModelLayout {
        self.body.layout()
    }
    fn measurements(&self, s: &[f64]) -> Result<Vec<f64>, Refusal> {
        self.calls.set(self.calls.get() + 1);
        self.body.measurements(s)
    }
    fn points(&self, s: &[f64], p: &[f64]) -> Result<Vec<Vec3>, Refusal> {
        self.calls.set(self.calls.get() + 1);
        self.body.points(s, p)
    }
}

#[test]
fn global_evaluation_budget_covers_derivatives_trials_checks_and_final_publication() {
    let (body, input, _) = scene();
    let model = CountedBody {
        body,
        calls: std::cell::Cell::new(0),
    };
    let options = FitOptions::default();
    let complete = fit_body(&input, &model, &options).unwrap();
    assert_eq!(complete.status, "ACCEPTED");
    let total = complete.model_evaluations;
    assert_eq!(total, model.calls.get());
    assert_eq!(
        total,
        complete
            .attempts
            .iter()
            .map(|a| a.model_evaluations)
            .sum::<usize>()
            + 1
    );
    assert!(total > 100);
    let first = complete.attempts[0].model_evaluations;
    // Exhaust every callback in the first two Jacobian/line-search rounds and at the
    // first-start boundary and final publication. No hidden derivative or rejected trial work.
    let limits: Vec<usize> = (1..=140)
        .chain([first - 1, first, first + 1, total - 1])
        .collect();
    for limit in limits {
        model.calls.set(0);
        let r = fit_body(
            &input,
            &model,
            &FitOptions {
                max_model_evaluations: limit,
                ..options
            },
        )
        .unwrap();
        assert_eq!(r.status, "BUDGET_STOP", "limit {limit}: {r:#?}");
        assert_eq!(model.calls.get(), limit);
        assert_eq!(r.model_evaluations, limit);
        assert!(r.solution.is_none());
        assert_eq!(r.attempts.len(), input.starts.len());
        assert_eq!(
            r.attempts
                .iter()
                .map(|a| a.model_evaluations)
                .sum::<usize>(),
            limit
        );
        if limit == first {
            assert_eq!(r.attempts[0].status, "CONVERGED");
            assert_eq!(r.attempts[1].status, "BUDGET_STOP");
            assert_eq!(r.attempts[1].model_evaluations, 0);
        }
        if limit == total - 1 {
            assert!(r.attempts.iter().all(|a| a.status == "CONVERGED"));
            assert!(!r.flags.is_empty());
        }
    }
    model.calls.set(0);
    let exact = fit_body(
        &input,
        &model,
        &FitOptions {
            max_model_evaluations: total,
            ..options
        },
    )
    .unwrap();
    assert_eq!(exact.status, "ACCEPTED");
    assert_eq!(exact.model_evaluations, total);
    assert_eq!(
        serde_json::to_value(exact.solution).unwrap(),
        serde_json::to_value(complete.solution).unwrap()
    );
}

#[test]
fn invalid_evaluation_or_pose_policy_refuses_without_model_work() {
    let (body, input, _) = scene();
    let model = CountedBody {
        body,
        calls: std::cell::Cell::new(0),
    };
    for options in [
        FitOptions {
            max_model_evaluations: 0,
            ..FitOptions::default()
        },
        FitOptions {
            max_model_evaluations: 1_000_001,
            ..FitOptions::default()
        },
        FitOptions {
            max_pose_spread_fraction: f64::NAN,
            ..FitOptions::default()
        },
        FitOptions {
            max_pose_spread_fraction: 0.0,
            ..FitOptions::default()
        },
        FitOptions {
            max_pose_spread_fraction: 1.0,
            ..FitOptions::default()
        },
    ] {
        assert!(fit_body(&input, &model, &options).is_err());
        assert_eq!(model.calls.get(), 0);
    }
}

struct TwoPoses(HingedBody);
impl BodyModel for TwoPoses {
    fn layout(&self) -> &ModelLayout {
        self.0.layout()
    }
    fn measurements(&self, s: &[f64]) -> Result<Vec<f64>, Refusal> {
        self.0.measurements(s)
    }
    fn points(&self, s: &[f64], p: &[f64]) -> Result<Vec<Vec3>, Refusal> {
        // Synthetic ambiguous correspondence mapping: distinct poses with identical observed points.
        // Each root has full local rank; no shape-only comparison can detect the ambiguity.
        let mut pose = p.to_vec();
        pose[3] *= pose[3];
        self.0.points(s, &pose)
    }
}

#[test]
fn identical_shape_minima_do_not_hide_conflicting_per_frame_poses() {
    let (body, mut input, mut poses) = scene();
    let model = TwoPoses(body);
    poses[0][3] = 0.6;
    poses[1][3] = 0.7;
    for (frame, pose) in input.frames.iter_mut().zip(&poses) {
        let points = model.points(&[0.48, 1.25], pose).unwrap();
        for (k, obs) in frame.keypoints.iter_mut().enumerate() {
            obs.pixel_px = frame.camera.project(points[k]).unwrap();
        }
    }
    for (start, sign) in input.starts.iter_mut().zip([1.0, -1.0]) {
        for p in &mut start.poses {
            p.values[3] = sign * 0.65;
        }
    }
    let r = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(r.status, "AMBIGUOUS", "{r:#?}");
    assert!(r.solution.is_none());
    assert!(r.attempts.iter().all(|a| a.status == "CONVERGED"));
    assert!(
        r.shape_spread_fraction
            .as_ref()
            .unwrap()
            .iter()
            .all(|&v| v < 1e-6)
    );
    let spread = r.pose_spread_fraction.as_ref().unwrap();
    for (p, f) in spread.iter().zip(&r.input.frames) {
        assert_eq!(p.frame, f.frame);
        assert!(p.fractions[3] > 0.39);
        assert!(p.fractions[..3].iter().all(|&v| v < 1e-6));
    }
    assert!(r.flags.iter().any(|s| s.contains("pose")));
    // Canonical frame identities, not input ordinal, bind the pose disagreement to its evidence.
    input.frames.reverse();
    for start in &mut input.starts {
        start.poses.reverse();
    }
    let reversed = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(
        serde_json::to_value(r).unwrap(),
        serde_json::to_value(reversed).unwrap()
    );
}

fn fix_frame_pose(
    input: &mut BodyFitInput,
    layout: &ModelLayout,
    frame_index: usize,
    coordinates: &[(usize, f64)],
) {
    let frame = &mut input.frames[frame_index];
    frame.fixed_pose = Some(FixedPose {
        values: coordinates
            .iter()
            .map(|&(index, value)| (layout.pose[index].name.clone(), value))
            .collect(),
        evidence: format!("synthetic independently declared pose for frame {frame_index}"),
    });
    for start in &mut input.starts {
        let pose = start
            .poses
            .iter_mut()
            .find(|pose| pose.frame == frame.frame)
            .unwrap();
        for &(index, value) in coordinates {
            pose.values[index] = value;
        }
    }
}

#[test]
fn distinct_fixed_frame_poses_admit_a_check_only_view_without_fitting_its_pose() {
    let (model, mut input, truth) = scene();
    input.measurements.clear();
    for (frame, pose) in truth.iter().enumerate() {
        fix_frame_pose(
            &mut input,
            model.layout(),
            frame,
            &pose.iter().copied().enumerate().collect::<Vec<_>>(),
        );
    }
    for observation in &mut input.frames[1].keypoints {
        observation.partition = Partition::HeldOut;
    }
    let baseline = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(baseline.status, "ACCEPTED", "{baseline:#?}");
    let solution = baseline.solution.as_ref().unwrap();
    for (actual, expected) in solution.shape.iter().zip([0.48, 1.25]) {
        assert!((actual - expected).abs() < 1e-7);
    }
    for (pose, expected) in solution.poses.iter().zip(&truth) {
        assert_eq!(pose.values, *expected);
    }
    for spread in baseline.pose_spread_fraction.as_ref().unwrap() {
        assert_eq!(spread.fractions, vec![0.0; 4]);
    }
    assert!(
        baseline.advisories.iter().any(|text| {
            text.contains("fixed frame-pose") && text.contains("not a pose estimate")
        })
    );
    assert!(baseline.attempts.iter().all(|attempt| {
        attempt.checks.iter().all(|check| check.passed)
            && attempt
                .cost_history
                .windows(2)
                .all(|pair| pair[1] < pair[0])
    }));

    // The fully held-out frame cannot steer shared shape, but still rejects inaccurate placement.
    let mut shifted = input.clone();
    shifted.frames[1].keypoints[0].pixel_px.x += 4.0;
    let rejected = fit_body(&shifted, &model, &FitOptions::default()).unwrap();
    assert_eq!(rejected.status, "REJECTED", "{rejected:#?}");
    assert!(rejected.solution.is_none());
    for (original, changed) in baseline.attempts.iter().zip(&rejected.attempts) {
        assert_eq!(original.cost_history, changed.cost_history);
    }

    let mut wrong_pose = input.clone();
    let shifted_x = truth[1][0] + 0.03;
    wrong_pose.frames[1]
        .fixed_pose
        .as_mut()
        .unwrap()
        .values
        .insert("x".into(), shifted_x);
    for start in &mut wrong_pose.starts {
        start.poses[1].values[0] = shifted_x;
    }
    let rejected = fit_body(&wrong_pose, &model, &FitOptions::default()).unwrap();
    assert_eq!(rejected.status, "REJECTED", "{rejected:#?}");
    assert!(rejected.solution.is_none());
    for (original, changed) in baseline.attempts.iter().zip(&rejected.attempts) {
        assert_eq!(original.cost_history, changed.cost_history);
    }

    // A remaining free pose coordinate in a CHECK-only frame still has no fitting evidence.
    let mut unobserved = input.clone();
    unobserved.frames[1]
        .fixed_pose
        .as_mut()
        .unwrap()
        .values
        .remove("arm");
    let report = fit_body(&unobserved, &model, &FitOptions::default()).unwrap();
    assert_eq!(report.status, "UNOBSERVABLE", "{report:#?}");
    assert!(report.solution.is_none());

    let stopped = fit_body(
        &input,
        &model,
        &FitOptions {
            max_model_evaluations: baseline.model_evaluations - 1,
            ..FitOptions::default()
        },
    )
    .unwrap();
    assert_eq!(stopped.status, "BUDGET_STOP");
    assert!(stopped.solution.is_none());

    input.frames.reverse();
    for frame in &mut input.frames {
        frame.keypoints.reverse();
    }
    for start in &mut input.starts {
        start.poses.reverse();
    }
    assert_eq!(
        serde_json::to_value(&baseline).unwrap(),
        serde_json::to_value(fit_body(&input, &model, &FitOptions::default()).unwrap()).unwrap()
    );
}

#[test]
fn mixed_fixed_subsets_recover_only_the_independent_free_coordinates() {
    let (model, mut input, truth) = scene();
    input.measurements.clear();
    fix_frame_pose(
        &mut input,
        model.layout(),
        0,
        &[(0, truth[0][0]), (1, truth[0][1]), (2, truth[0][2])],
    );
    fix_frame_pose(&mut input, model.layout(), 1, &[(1, truth[1][1])]);
    let report = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(report.status, "ACCEPTED", "{report:#?}");
    let solution = report.solution.as_ref().unwrap();
    for (actual, expected) in solution.shape.iter().zip([0.48, 1.25]) {
        assert!((actual - expected).abs() < 1e-7);
    }
    for ((pose, expected), frame) in solution.poses.iter().zip(&truth).zip(&input.frames) {
        for (index, (actual, expected)) in pose.values.iter().zip(expected).enumerate() {
            if frame
                .fixed_pose
                .as_ref()
                .unwrap()
                .values
                .contains_key(&model.layout.pose[index].name)
            {
                assert_eq!(actual.to_bits(), expected.to_bits());
            } else {
                assert!((actual - expected).abs() < 1e-6);
            }
        }
    }
    let spread = report.pose_spread_fraction.as_ref().unwrap();
    assert_eq!(spread[0].fractions[..3], [0.0; 3]);
    assert_eq!(spread[1].fractions[1], 0.0);
    assert!(
        spread
            .iter()
            .flat_map(|pose| &pose.fractions)
            .all(|v| *v < 1e-6)
    );

    // Removing the fixed-depth evidence restores the existing scale/depth gauge failure.
    for frame in &mut input.frames {
        frame.fixed_pose = None;
    }
    let unobservable = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(unobservable.status, "UNOBSERVABLE", "{unobservable:#?}");
    assert!(unobservable.solution.is_none());
}

#[test]
fn fixed_pose_admission_refuses_conflicts_and_retains_legacy_serialization() {
    let (body, mut input, truth) = scene();
    let legacy = serde_json::to_value(&input).unwrap();
    assert!(
        legacy["frames"]
            .as_array()
            .unwrap()
            .iter()
            .all(|frame| { frame.get("fixed_pose").is_none() })
    );
    let roundtrip: BodyFitInput = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(serde_json::to_value(&roundtrip).unwrap(), legacy);
    fix_frame_pose(&mut input, body.layout(), 1, &[(1, truth[1][1])]);
    let model = CountedBody {
        body,
        calls: std::cell::Cell::new(0),
    };
    let prepared = prepare_input(&input, model.layout()).unwrap();
    assert_eq!(
        prepared.frames[1].fixed_pose.as_ref().unwrap().values["y"],
        truth[1][1]
    );
    let mut draft = input.clone();
    draft.starts.clear();
    assert!(prepare_start_authoring(&draft, model.layout()).is_ok());

    let mut invalids = Vec::new();
    for (key, value) in [
        ("unknown", 0.0),
        ("width", 0.48),
        ("y", f64::NAN),
        ("y", f64::INFINITY),
        ("y", -2.0),
        ("y", 2.0),
    ] {
        let mut changed = input.clone();
        changed.frames[1].fixed_pose.as_mut().unwrap().values =
            [(key.to_owned(), value)].into_iter().collect();
        invalids.push(changed);
    }
    let mut changed = input.clone();
    changed.frames[1]
        .fixed_pose
        .as_mut()
        .unwrap()
        .evidence
        .clear();
    invalids.push(changed);
    let mut changed = input.clone();
    changed.frames[1]
        .fixed_pose
        .as_mut()
        .unwrap()
        .values
        .clear();
    invalids.push(changed);
    let mut changed = input.clone();
    changed.starts[0].poses[1].values[1] += 0.01;
    invalids.push(changed);
    let mut changed = input.clone();
    changed.starts[1] = changed.starts[0].clone();
    invalids.push(changed);
    for invalid in invalids {
        assert!(fit_body(&invalid, &model, &FitOptions::default()).is_err());
        assert_eq!(model.calls.get(), 0);
    }

    // A fixed source declaration never consumes a free-variable slot, but all source bounds
    // and retained vectors remain limited by the existing per-model/frame budgets.
    let mut layout = model.layout().clone();
    layout.pose = (0..18)
        .map(|index| parameter(&format!("p{index}"), -1.0, 1.0))
        .collect();
    draft.frames = (0..8)
        .map(|index| {
            let mut frame = draft.frames[0].clone();
            frame.frame.timestamp_us = index * 200_000;
            frame.fixed_pose = None;
            frame
        })
        .collect();
    assert!(prepare_capture_authoring(&draft, &layout).is_err());
    for frame in &mut draft.frames {
        frame.fixed_pose = Some(FixedPose {
            values: [("p0".into(), 0.0), ("p1".into(), 0.1), ("p2".into(), -0.1)]
                .into_iter()
                .collect(),
            evidence: "synthetic fixed subset".into(),
        });
    }
    assert!(prepare_capture_authoring(&draft, &layout).is_ok());
}
