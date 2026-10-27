//! Exact dependency blocks: shape affects every frame; a pose affects ONLY its decoded frame.
//! Retain dense column/reduction order, but do not re-skin unrelated poses for central differences.

use std::ops::Range;

use super::*;

pub(super) struct Residual {
    pub(super) values: Vec<f64>,
    // Measured during assembly, never guessed from the silhouette residual's padding convention.
    pub(super) frame_rows: Vec<Range<usize>>,
}

pub(super) fn frame_residual(
    frame: &FitFrame,
    model: &impl BodyModel,
    shape: &[f64],
    pose: &[f64],
    points: &[Vec3],
    budget: &mut EvaluationBudget,
) -> Result<Vec<f64>, Refusal> {
    let l = model.layout();
    if points.len() != l.keypoints.len() || points.iter().any(|p| !p.is_finite()) {
        return Err(invalid("model returned invalid keypoint vector"));
    }
    let mut r = Vec::new();
    for obs in frame
        .keypoints
        .iter()
        .filter(|o| o.partition == Partition::Fit)
    {
        let k = l.keypoints.iter().position(|n| n == &obs.name).unwrap();
        let d = (frame.camera.project(points[k])? - obs.pixel_px) / obs.sigma_px;
        r.extend([d.x, d.y]);
    }
    if let Some(s) = &frame.silhouette
        && s.lines.iter().any(|line| line.partition == Partition::Fit)
    {
        let mesh = budget.call(|| model.surface(shape, pose))?;
        let comparisons = silhouette::compare(
            &mesh,
            &frame.camera,
            frame.image_size_px,
            s,
            Some(Partition::Fit),
        )?;
        for (line, comparison) in s
            .lines
            .iter()
            .filter(|line| line.partition == Partition::Fit)
            .zip(comparisons)
        {
            let residuals = comparison.residuals_px.ok_or_else(|| {
                invalid(
                    "model does not intersect an observed silhouette slice; improve initialization",
                )
            })?;
            r.extend(residuals.iter().map(|v| v / line.sigma_px));
        }
    }
    if r.iter().any(|v| !v.is_finite()) || !cost(&r).is_finite() {
        return Err(invalid("non-finite body-fit objective"));
    }
    Ok(r)
}

pub(super) fn column(
    input: &BodyFitInput,
    model: &impl BodyModel,
    x: &[f64],
    k: usize,
    h: f64,
    base: &Residual,
    budget: &mut EvaluationBudget,
) -> Result<Vec<f64>, Refusal> {
    let l = model.layout();
    let ns = l.shape.len();
    let (range, plus, minus) = if k < ns {
        let mut plus = x.to_vec();
        let mut minus = x.to_vec();
        plus[k] += h;
        minus[k] -= h;
        let plus = residual(input, model, &plus, budget)?;
        let minus = residual(input, model, &minus, budget)?;
        if plus.frame_rows != base.frame_rows || minus.frame_rows != base.frame_rows {
            return Err(invalid(
                "body-fit residual dimensions changed during differentiation",
            ));
        }
        (0..base.values.len(), plus.values, minus.values)
    } else {
        let f = (k - ns) / l.pose.len();
        let begin = ns + f * l.pose.len();
        let shape = values(&x[..ns], &l.shape);
        let frame = &input.frames[f];
        let range = base.frame_rows[f].clone();
        if range.is_empty() {
            // Held-out evidence cannot supply a derivative for an unobserved pose.
            return Ok(vec![0.0; base.values.len()]);
        }
        let mut local = x[begin..begin + l.pose.len()].to_vec();
        local[k - begin] += h;
        let plus_pose = values(&local, &l.pose);
        local[k - begin] = x[k] - h;
        let minus_pose = values(&local, &l.pose);
        let plus_points = budget.call(|| model.points(&shape, &plus_pose))?;
        let plus = frame_residual(frame, model, &shape, &plus_pose, &plus_points, budget)?;
        let minus_points = budget.call(|| model.points(&shape, &minus_pose))?;
        let minus = frame_residual(frame, model, &shape, &minus_pose, &minus_points, budget)?;
        (range, plus, minus)
    };
    if plus.len() != range.len() || minus.len() != range.len() {
        return Err(invalid(
            "body-fit residual dimensions changed during differentiation",
        ));
    }
    // Structural zeros stay in the original dense positions: normal assembly and its rank gate
    // use the identical summation order. This is not sparse approximation or row subsampling.
    let mut col = vec![0.0; base.values.len()];
    for (v, (p, m)) in col[range].iter_mut().zip(plus.iter().zip(minus)) {
        *v = (p - m) / (2.0 * h);
        if !v.is_finite() {
            return Err(invalid("non-finite body-fit derivative"));
        }
    }
    Ok(col)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use ftx_core::Mat3;

    use super::*;
    use crate::{
        camera::{Distortion, Intrinsics},
        silhouette::{Axis, Scanline, Silhouette, SurfaceMesh},
    };

    // Callback trace independent of EvaluationBudget; the model has no cross-frame dependency.
    #[derive(Clone, Debug, PartialEq)]
    enum Call {
        Tape,
        Points(Vec<f64>),
        Surface(Vec<f64>),
    }
    struct Panel {
        layout: ModelLayout,
        calls: RefCell<Vec<Call>>,
        invalid_points: Cell<bool>,
        refuse_surface: Cell<bool>,
    }
    impl BodyModel for Panel {
        fn layout(&self) -> &ModelLayout {
            &self.layout
        }
        fn measurements(&self, s: &[f64]) -> Result<Vec<f64>, Refusal> {
            self.calls.borrow_mut().push(Call::Tape);
            Ok(vec![s[0]])
        }
        fn points(&self, s: &[f64], p: &[f64]) -> Result<Vec<Vec3>, Refusal> {
            self.calls.borrow_mut().push(Call::Points(p.to_vec()));
            if self.invalid_points.get() {
                return Ok(vec![Vec3::new(f64::NAN, 0., 0.)]);
            }
            Ok(vec![
                Vec3::new(p[0], 0., 1. + p[1]),
                Vec3::new(p[0] + s[0] / 4., 0., 1. + p[1]),
            ])
        }
        fn surface(&self, s: &[f64], p: &[f64]) -> Result<SurfaceMesh, Refusal> {
            self.calls.borrow_mut().push(Call::Surface(p.to_vec()));
            if self.refuse_surface.get() {
                return Err(invalid("synthetic unavailable surface"));
            }
            Ok(SurfaceMesh {
                vertices_m: vec![
                    Vec3::new(p[0] - s[0] / 2., 0., 0.5 + p[1]),
                    Vec3::new(p[0] + s[0] / 2., 0., 0.5 + p[1]),
                    Vec3::new(p[0] + s[0] / 2., 0., 1.5 + p[1]),
                    Vec3::new(p[0] - s[0] / 2., 0., 1.5 + p[1]),
                ],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
            })
        }
    }
    fn parameter(name: &str, lower: f64, upper: f64) -> Parameter {
        Parameter {
            name: name.into(),
            lower,
            upper,
        }
    }
    fn scene(n: usize, surfaces: bool) -> (Panel, BodyFitInput, Vec<f64>) {
        let model = Panel {
            layout: ModelLayout {
                model_id: "synthetic-difference-panel".into(),
                shape: vec![parameter("width", 0.2, 0.9)],
                pose: vec![parameter("x", -0.5, 0.5), parameter("z", -0.5, 0.5)],
                keypoints: vec!["center".into(), "quarter".into()],
                measurements: vec!["width".into()],
            },
            calls: RefCell::new(Vec::new()),
            invalid_points: Cell::new(false),
            refuse_surface: Cell::new(false),
        };
        let frames: Vec<_> = (0..n)
            .map(|f| FitFrame {
                frame: FrameId {
                    stream: "synthetic".into(),
                    timestamp_us: f as u64 * 100_000,
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
                keypoints: vec![
                    Keypoint {
                        name: "center".into(),
                        pixel_px: Vec2::new(100., 100.),
                        sigma_px: 1.3,
                        partition: Partition::Fit,
                    },
                    Keypoint {
                        name: "quarter".into(),
                        pixel_px: Vec2::new(115., 100.),
                        sigma_px: 2.1,
                        partition: Partition::HeldOut,
                    },
                ],
                silhouette: surfaces.then(|| Silhouette {
                    evidence: "analytical rectified panel".into(),
                    lines: [80., 100., 120.]
                        .into_iter()
                        .enumerate()
                        .map(|(i, y)| Scanline {
                            axis: Axis::Horizontal,
                            at_px: y,
                            intervals_px: vec![[70., 130.]],
                            sigma_px: 0.7,
                            // Deliberately mix frame-specific evidence blocks and padding lengths.
                            partition: if i == 1 || f % 3 == 1 {
                                Partition::HeldOut
                            } else {
                                Partition::Fit
                            },
                        })
                        .collect(),
                }),
            })
            .collect();
        let starts = [0., 0.01]
            .map(|offset| FitStart {
                shape: vec![0.6 + offset],
                poses: frames
                    .iter()
                    .enumerate()
                    .map(|(i, f)| FramePose {
                        frame: f.frame.clone(),
                        values: vec![0.01 * i as f64 + offset, offset],
                    })
                    .collect(),
            })
            .to_vec();
        let input = BodyFitInput {
            schema: "ftx-body-fit-input/1".into(),
            metric_scale_source: "synthetic".into(),
            observation_evidence: "synthetic".into(),
            frames,
            starts,
            measurements: vec![MetricObservation {
                name: "width".into(),
                value_m: 0.61,
                sigma_m: 0.002,
                evidence: "independent synthetic tape".into(),
                partition: Partition::Fit,
            }],
        };
        let input = prepare_input(&input, &model.layout).unwrap();
        let x = encode(&input.starts[0], &model.layout).unwrap();
        (model, input, x)
    }
    fn budget() -> EvaluationBudget {
        EvaluationBudget {
            used: 0,
            limit: 100_000,
        }
    }

    #[test]
    fn block_columns_match_whole_problem_differences_bit_for_bit() {
        for n in [2, 8] {
            for surfaces in [false, true] {
                let (model, input, x) = scene(n, surfaces);
                let base = residual(&input, &model, &x, &mut budget()).unwrap();
                let full_calls = 1
                    + n
                    + input
                        .frames
                        .iter()
                        .filter(|f| {
                            f.silhouette.as_ref().is_some_and(|s| {
                                s.lines.iter().any(|l| l.partition == Partition::Fit)
                            })
                        })
                        .count();
                for k in 0..x.len() {
                    let h = 1e-5;
                    let mut plus = x.clone();
                    plus[k] += h;
                    let mut minus = x.clone();
                    minus[k] -= h;
                    let mut dense_budget = budget();
                    model.calls.borrow_mut().clear();
                    // Deliberately evaluate the WHOLE problem, as the original solver did.
                    let rp = residual(&input, &model, &plus, &mut dense_budget).unwrap();
                    let rm = residual(&input, &model, &minus, &mut dense_budget).unwrap();
                    let expected: Vec<_> = rp
                        .values
                        .iter()
                        .zip(rm.values)
                        .map(|(p, m)| ((p - m) / (2. * h)).to_bits())
                        .collect();
                    assert_eq!(dense_budget.used, 2 * full_calls);
                    assert_eq!(model.calls.borrow().len(), dense_budget.used);
                    model.calls.borrow_mut().clear();
                    let mut local_budget = budget();
                    let actual =
                        column(&input, &model, &x, k, h, &base, &mut local_budget).unwrap();
                    assert_eq!(
                        actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                        expected,
                        "frames={n}, surfaces={surfaces}, column={k}"
                    );
                    assert_eq!(local_budget.used, model.calls.borrow().len());
                    if k == 0 {
                        assert_eq!(local_budget.used, dense_budget.used);
                    } else {
                        let f = (k - 1) / 2;
                        let has_surface = surfaces && f % 3 != 1;
                        assert_eq!(local_budget.used, if has_surface { 4 } else { 2 });
                        assert!(
                            model
                                .calls
                                .borrow()
                                .iter()
                                .all(|c| !matches!(c, Call::Tape))
                        );
                        assert!(
                            actual
                                .iter()
                                .enumerate()
                                .all(|(row, v)| base.frame_rows[f].contains(&row) || *v == 0.)
                        );
                        // An unrelated pose coordinate must never reach either callback.
                        for call in model.calls.borrow().iter() {
                            let (Call::Points(p) | Call::Surface(p)) = call else {
                                unreachable!()
                            };
                            let other = 1 - (k - 1) % 2;
                            let expected_pose =
                                values(&x[1 + 2 * f..3 + 2 * f], &model.layout.pose);
                            assert_eq!(p[other], expected_pose[other]);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn local_derivatives_charge_each_callback_and_refuse_invalid_evaluations() {
        let (model, input, x) = scene(2, true);
        let base = residual(&input, &model, &x, &mut budget()).unwrap();
        for limit in 0..4 {
            model.calls.borrow_mut().clear();
            let mut b = EvaluationBudget { used: 0, limit };
            let err = column(&input, &model, &x, 1, 1e-5, &base, &mut b).unwrap_err();
            assert_eq!(err.code, RefusalCode::BudgetExceeded);
            assert_eq!(b.used, limit);
            assert_eq!(model.calls.borrow().len(), limit);
        }
        model.invalid_points.set(true);
        model.calls.borrow_mut().clear();
        assert!(column(&input, &model, &x, 1, 1e-5, &base, &mut budget()).is_err());
        assert_eq!(model.calls.borrow().len(), 1);
        model.invalid_points.set(false);
        model.refuse_surface.set(true);
        model.calls.borrow_mut().clear();
        assert!(column(&input, &model, &x, 1, 1e-5, &base, &mut budget()).is_err());
        assert_eq!(model.calls.borrow().len(), 2);
    }

    #[test]
    fn held_out_only_pose_has_zero_column_without_model_calls_but_cannot_be_accepted() {
        let (model, mut input, x) = scene(2, true);
        for p in &mut input.frames[0].keypoints {
            p.partition = Partition::HeldOut;
        }
        for line in &mut input.frames[0].silhouette.as_mut().unwrap().lines {
            line.partition = Partition::HeldOut;
        }
        let base = residual(&input, &model, &x, &mut budget()).unwrap();
        assert!(base.frame_rows[0].is_empty());
        model.calls.borrow_mut().clear();
        let mut b = EvaluationBudget { used: 0, limit: 0 };
        assert!(
            column(&input, &model, &x, 1, 1e-5, &base, &mut b)
                .unwrap()
                .iter()
                .all(|v| *v == 0.)
        );
        assert!(model.calls.borrow().is_empty());
        let report = fit_body(&input, &model, &FitOptions::default()).unwrap();
        assert_eq!(report.status, "UNOBSERVABLE");
        assert!(report.solution.is_none());
    }
}
