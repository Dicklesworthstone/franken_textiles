//! A body width invisible to centerline keypoints becomes observable through surface silhouettes.
use ftx_core::{Mat3, Refusal, Vec2, Vec3};
use ftx_measure::{
    body_fit::*,
    camera::{Camera, Distortion, Intrinsics},
    silhouette::{Axis, Scanline, Silhouette, SurfaceMesh},
    triangulate::{FrameId, Partition},
};

struct Panel {
    layout: ModelLayout,
}
impl BodyModel for Panel {
    fn layout(&self) -> &ModelLayout {
        &self.layout
    }
    fn measurements(&self, _: &[f64]) -> Result<Vec<f64>, Refusal> {
        Ok(vec![])
    }
    fn points(&self, _: &[f64], p: &[f64]) -> Result<Vec<Vec3>, Refusal> {
        if self.layout.keypoints.is_empty() {
            Ok(vec![])
        } else {
            Ok(vec![Vec3::new(p[0], 0., 1.)])
        }
    }
    fn surface(&self, s: &[f64], p: &[f64]) -> Result<SurfaceMesh, Refusal> {
        Ok(SurfaceMesh {
            vertices_m: vec![
                Vec3::new(p[0] - s[0] / 2., 0., 0.5),
                Vec3::new(p[0] + s[0] / 2., 0., 0.5),
                Vec3::new(p[0] + s[0] / 2., 0., 1.5),
                Vec3::new(p[0] - s[0] / 2., 0., 1.5),
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        })
    }
}
fn scene() -> (Panel, BodyFitInput) {
    let model = Panel {
        layout: ModelLayout {
            model_id: "analytic-panel".into(),
            shape: vec![Parameter {
                name: "width".into(),
                lower: 0.2,
                upper: 0.9,
            }],
            pose: vec![Parameter {
                name: "root_x".into(),
                lower: -0.5,
                upper: 0.5,
            }],
            keypoints: vec!["center".into()],
            measurements: vec![],
        },
    };
    let frames: Vec<_> = [-0.1, 0.1]
        .into_iter()
        .enumerate()
        .map(|(i, x)| FitFrame {
            frame: FrameId {
                stream: "synthetic".into(),
                timestamp_us: i as u64 * 100_000,
            },
            camera: Camera {
                intrinsics: Intrinsics {
                    focal_px: Vec2::new(300., 300.),
                    principal_px: Vec2::new(100., 100.),
                    distortion: Distortion::default(),
                },
                center_m: Vec3::new(0., -3., 1.),
                world_to_camera: Mat3::from_rows([[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]]),
            },
            image_size_px: [200, 200],
            rotation_cw_deg: 0,
            keypoints: vec![Keypoint {
                name: "center".into(),
                pixel_px: Vec2::new(100. + 100. * x, 100.),
                sigma_px: 1.,
                partition: Partition::Fit,
            }],
            silhouette: Some(Silhouette {
                evidence: "analytic rectified moving panel".into(),
                lines: [80., 100., 120.]
                    .into_iter()
                    .enumerate()
                    .map(|(j, y)| Scanline {
                        axis: Axis::Horizontal,
                        at_px: y,
                        intervals_px: vec![[100. + 100. * (x - 0.3), 100. + 100. * (x + 0.3)]],
                        sigma_px: 1.,
                        partition: if j == 1 {
                            Partition::HeldOut
                        } else {
                            Partition::Fit
                        },
                    })
                    .collect(),
            }),
        })
        .collect();
    let starts = [(0.5, 0.03), (0.7, -0.04)]
        .into_iter()
        .map(|(width, shift)| FitStart {
            shape: vec![width],
            poses: frames
                .iter()
                .zip([-0.1, 0.1])
                .map(|(f, x)| FramePose {
                    frame: f.frame.clone(),
                    values: vec![x + shift],
                })
                .collect(),
        })
        .collect();
    (
        model,
        BodyFitInput {
            schema: "ftx-body-fit-input/1".into(),
            metric_scale_source: "synthetic exact cameras".into(),
            observation_evidence: "synthetic panel".into(),
            frames,
            starts,
            measurements: vec![],
        },
    )
}

fn brown_scene() -> (Panel, BodyFitInput) {
    let (model, mut input) = scene();
    for (frame, root_x) in input.frames.iter_mut().zip([-0.1, 0.1]) {
        frame.camera.intrinsics.distortion.radial[0] = 0.5;
        let normalized_x = root_x / 3.;
        frame.keypoints[0].pixel_px.x =
            100. + 300. * normalized_x * (1. + 0.5 * normalized_x * normalized_x);
        let silhouette = frame.silhouette.as_mut().unwrap();
        silhouette.evidence = "independent analytic radial projection of a moving panel".into();
        for line in &mut silhouette.lines {
            // On each vertical panel edge x is constant. Solve the monotone scalar cubic
            // y*(1 + (x*x+y*y)/2) = observed_y independently of the camera/contour kernel.
            line.intervals_px[0] = [-0.3, 0.3].map(|offset| {
                let x = (root_x + offset) / 3.;
                let target = (line.at_px - 100.) / 300.;
                let (mut lo, mut hi) = (-1. / 6., 1. / 6.);
                for _ in 0..64 {
                    let y = (lo + hi) * 0.5;
                    if y * (1. + 0.5 * (x * x + y * y)) < target {
                        lo = y;
                    } else {
                        hi = y;
                    }
                }
                let y = (lo + hi) * 0.5;
                100. + 300. * x * (1. + 0.5 * (x * x + y * y))
            });
        }
    }
    (model, input)
}

#[test]
fn silhouette_only_views_recover_shape_and_lateral_pose_without_dummy_targets() {
    for (mut model, mut input) in [scene(), brown_scene()] {
        model.layout.keypoints.clear();
        for frame in &mut input.frames {
            frame.keypoints.clear();
        }
        let original = serde_json::to_value(&input).unwrap();
        let prepared = prepare_start_authoring(&input, &model.layout).unwrap();
        assert_eq!(serde_json::to_value(prepared).unwrap(), original);
        let baseline = fit_body(&input, &model, &FitOptions::default()).unwrap();
        assert_eq!(baseline.status, "ACCEPTED", "{baseline:#?}");
        assert!(baseline.model.keypoints.is_empty());
        assert!(baseline.input.measurements.is_empty());
        assert_eq!(serde_json::to_value(&input).unwrap(), original);
        let solution = baseline.solution.as_ref().unwrap();
        assert!((solution.shape[0] - 0.6).abs() < 1e-7);
        for (pose, truth) in solution.poses.iter().zip([-0.1, 0.1]) {
            assert!((pose.values[0] - truth).abs() < 1e-7);
        }
        assert!(
            baseline
                .attempts
                .iter()
                .flat_map(|attempt| &attempt.checks)
                .all(|check| check.name.starts_with("silhouette:") && check.passed)
        );
        let mut bad_check = input.clone();
        bad_check.frames[0].silhouette.as_mut().unwrap().lines[1].intervals_px[0][1] += 5.;
        let rejected = fit_body(&bad_check, &model, &FitOptions::default()).unwrap();
        assert_eq!(rejected.status, "REJECTED", "{rejected:#?}");
        assert!(rejected.solution.is_none());
        for (changed, accepted) in rejected.attempts.iter().zip(&baseline.attempts) {
            assert_eq!(changed.cost_history, accepted.cost_history);
        }

        // A CHECK-only view cannot identify its free lateral pose, even though the other
        // frame still supplies FIT silhouettes. Held-out evidence never repairs deficient rank.
        let mut unobserved_pose = input.clone();
        for line in &mut unobserved_pose.frames[0].silhouette.as_mut().unwrap().lines {
            line.partition = Partition::HeldOut;
        }
        let report = fit_body(&unobserved_pose, &model, &FitOptions::default()).unwrap();
        assert_eq!(report.status, "UNOBSERVABLE");
        assert!(report.solution.is_none());

        let report = fit_body(
            &input,
            &model,
            &FitOptions {
                max_model_evaluations: 1,
                ..FitOptions::default()
            },
        )
        .unwrap();
        assert_eq!(report.status, "BUDGET_STOP");
        assert!(report.solution.is_none());
        input.frames.reverse();
        for frame in &mut input.frames {
            frame.silhouette.as_mut().unwrap().lines.reverse();
        }
        assert_eq!(
            serde_json::to_value(&baseline).unwrap(),
            serde_json::to_value(fit_body(&input, &model, &FitOptions::default()).unwrap())
                .unwrap()
        );
    }
}

#[test]
fn silhouette_only_admission_requires_real_contours_and_preserves_draft_and_start_rules() {
    let (mut model, mut input) = scene();
    let unknown = input.frames[0].keypoints[0].clone();
    model.layout.keypoints.clear();
    for frame in &mut input.frames {
        frame.keypoints.clear();
    }
    let mut draft = input.clone();
    draft.starts.clear();
    assert!(prepare_start_authoring(&draft, &model.layout).is_ok());
    assert!(prepare_input(&draft, &model.layout).is_err());
    for frame in &mut draft.frames {
        frame.silhouette = None;
    }
    assert!(prepare_capture_authoring(&draft, &model.layout).is_ok());
    assert!(prepare_start_authoring(&draft, &model.layout).is_err());
    draft.starts = input.starts.clone();
    assert!(fit_body(&draft, &model, &FitOptions::default()).is_err());

    // Tape-only fitting is a different boundary; an empty target map cannot make this
    // calibrated-image workflow silently accept frames with no image evidence at all.
    model.layout.measurements.push("width".into());
    draft.measurements.push(MetricObservation {
        name: "width".into(),
        value_m: 0.6,
        sigma_m: 0.001,
        evidence: "synthetic independent tape".into(),
        partition: Partition::Fit,
    });
    assert!(prepare_start_authoring(&draft, &model.layout).is_err());
    assert!(prepare_input(&draft, &model.layout).is_err());
    model.layout.measurements.clear();

    let mut invalid = input.clone();
    invalid.frames[0].silhouette.as_mut().unwrap().lines.clear();
    assert!(prepare_start_authoring(&invalid, &model.layout).is_err());
    invalid = input.clone();
    invalid.frames[0].keypoints.push(unknown);
    assert!(prepare_input(&invalid, &model.layout).is_err());
    invalid = input.clone();
    invalid.starts[1] = invalid.starts[0].clone();
    assert!(prepare_input(&invalid, &model.layout).is_err());

    for frame in &mut input.frames {
        for line in &mut frame.silhouette.as_mut().unwrap().lines {
            line.partition = Partition::HeldOut;
        }
    }
    assert!(prepare_start_authoring(&input, &model.layout).is_ok());
    let report = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(report.status, "UNOBSERVABLE");
    assert!(report.solution.is_none());
}

#[test]
fn original_brown_silhouettes_recover_shared_width_without_rectifying_observations() {
    let (model, mut input) = brown_scene();
    let original = serde_json::to_value(&input).unwrap();
    let baseline = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(baseline.status, "ACCEPTED", "{baseline:#?}");
    assert_eq!(serde_json::to_value(&input).unwrap(), original);
    let solution = baseline.solution.as_ref().unwrap();
    assert!((solution.shape[0] - 0.6).abs() < 1e-7);
    for check in baseline.attempts.iter().flat_map(|attempt| &attempt.checks) {
        if check.name.starts_with("silhouette:") {
            assert!(
                check
                    .prediction_error_bound_px
                    .is_some_and(|bound| (0. ..=1e-9).contains(&bound))
            );
        } else {
            assert!(check.prediction_error_bound_px.is_none());
        }
    }
    for (pose, truth) in solution.poses.iter().zip([-0.1, 0.1]) {
        assert!((pose.values[0] - truth).abs() < 1e-7);
    }
    input.frames.reverse();
    for frame in &mut input.frames {
        frame.silhouette.as_mut().unwrap().lines.reverse();
    }
    assert_eq!(
        serde_json::to_value(&baseline).unwrap(),
        serde_json::to_value(fit_body(&input, &model, &FitOptions::default()).unwrap()).unwrap()
    );
    input.frames[0].silhouette.as_mut().unwrap().lines[1].intervals_px[0][1] += 5.;
    let rejected = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(rejected.status, "REJECTED", "{rejected:#?}");
    assert!(rejected.solution.is_none());
    for (changed, original) in rejected.attempts.iter().zip(baseline.attempts) {
        assert_eq!(changed.cost_history, original.cost_history);
    }
}
#[test]
fn silhouette_resolves_a_shape_variable_not_constrained_by_keypoints() {
    let (model, input) = scene();
    let mut no_mask = input.clone();
    for f in &mut no_mask.frames {
        f.silhouette = None;
    }
    assert_eq!(
        fit_body(&no_mask, &model, &FitOptions::default())
            .unwrap()
            .status,
        "UNOBSERVABLE"
    );
    let report = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(report.status, "ACCEPTED", "{report:#?}");
    let solution = report.solution.unwrap();
    assert!((solution.shape[0] - 0.6).abs() < 1e-8);
    for (p, x) in solution.poses.iter().zip([-0.1, 0.1]) {
        assert!((p.values[0] - x).abs() < 1e-8);
    }
    assert!(
        report
            .attempts
            .iter()
            .flat_map(|a| &a.checks)
            .all(|c| c.passed)
    );
}
#[test]
fn held_out_lines_never_change_the_fit_but_can_reject_a_body() {
    let (model, mut input) = scene();
    let baseline = fit_body(&input, &model, &FitOptions::default()).unwrap();
    input.frames[0].silhouette.as_mut().unwrap().lines[1].intervals_px[0][1] += 2.;
    let shifted = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(shifted.status, "ACCEPTED");
    assert_eq!(
        shifted.solution.unwrap().shape,
        baseline.solution.unwrap().shape
    );
    input.frames[0].silhouette.as_mut().unwrap().lines[1].intervals_px[0][1] += 3.;
    let rejected = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(rejected.status, "REJECTED");
    assert!(rejected.solution.is_none());
    for (a, b) in rejected.attempts.iter().zip(baseline.attempts) {
        assert_eq!(a.cost_history, b.cost_history);
    }
}
#[test]
fn line_order_and_frame_order_do_not_change_results() {
    let (model, mut input) = scene();
    let first = fit_body(&input, &model, &FitOptions::default()).unwrap();
    input.frames.reverse();
    for f in &mut input.frames {
        f.silhouette.as_mut().unwrap().lines.reverse();
    }
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(fit_body(&input, &model, &FitOptions::default()).unwrap()).unwrap()
    );
}
#[test]
fn silhouette_budget_stops_and_malformed_evidence_publish_no_body() {
    let (model, mut input) = scene();
    let budget = fit_body(
        &input,
        &model,
        &FitOptions {
            iterations: 1,
            ..FitOptions::default()
        },
    )
    .unwrap();
    assert_eq!(budget.status, "BUDGET_STOP");
    assert!(budget.solution.is_none());
    input.frames[0]
        .silhouette
        .as_mut()
        .unwrap()
        .evidence
        .clear();
    assert!(fit_body(&input, &model, &FitOptions::default()).is_err());
    let (model, mut input) = scene();
    input.frames[0].camera.intrinsics.distortion.radial[0] = -0.5;
    assert!(fit_body(&input, &model, &FitOptions::default()).is_err());
}

struct CountedSurface {
    panel: Panel,
    calls: std::cell::Cell<usize>,
    surfaces: std::cell::Cell<usize>,
}
impl BodyModel for CountedSurface {
    fn layout(&self) -> &ModelLayout {
        self.panel.layout()
    }
    fn points(&self, s: &[f64], p: &[f64]) -> Result<Vec<Vec3>, Refusal> {
        self.calls.set(self.calls.get() + 1);
        self.panel.points(s, p)
    }
    fn measurements(&self, s: &[f64]) -> Result<Vec<f64>, Refusal> {
        self.calls.set(self.calls.get() + 1);
        self.panel.measurements(s)
    }
    fn surface(&self, s: &[f64], p: &[f64]) -> Result<SurfaceMesh, Refusal> {
        self.calls.set(self.calls.get() + 1);
        self.surfaces.set(self.surfaces.get() + 1);
        self.panel.surface(s, p)
    }
}
#[test]
fn surface_evaluations_share_the_global_budget_including_held_out_checks() {
    let (panel, input) = scene();
    let model = CountedSurface {
        panel,
        calls: std::cell::Cell::new(0),
        surfaces: std::cell::Cell::new(0),
    };
    let full = fit_body(&input, &model, &FitOptions::default()).unwrap();
    assert_eq!(full.status, "ACCEPTED");
    assert_eq!(full.model_evaluations, model.calls.get());
    assert!(model.surfaces.get() > 10);
    for limit in 1..full.model_evaluations {
        model.calls.set(0);
        model.surfaces.set(0);
        let report = fit_body(
            &input,
            &model,
            &FitOptions {
                max_model_evaluations: limit,
                ..FitOptions::default()
            },
        )
        .unwrap();
        assert_eq!(report.status, "BUDGET_STOP", "limit {limit}");
        assert_eq!(report.model_evaluations, limit);
        assert_eq!(model.calls.get(), limit);
        assert!(report.solution.is_none());
    }
}
