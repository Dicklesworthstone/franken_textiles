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
        Ok(vec![Vec3::new(p[0], 0., 1.)])
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
    input.frames[0].camera.intrinsics.distortion.radial[0] = 0.01;
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
