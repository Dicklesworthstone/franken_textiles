//! FTX-21: one shared shape and an independent articulated pose per decoded frame.
//! Cameras stay calibrated and fixed. Metric tape constraints can break the shape/depth gauge;
//! damping and a good-looking image residual cannot substitute for observable fit data.

use std::collections::BTreeSet;

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::{
    camera::Camera,
    invalid,
    silhouette::{self, Silhouette, SurfaceMesh},
    triangulate::{FrameId, Partition},
};

mod derivatives;

/// One scalar model parameter. Fitting stays strictly inside its declared physical bounds.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    /// Stable model-specific name.
    pub name: String,
    /// Exclusive lower bound; fix an endpoint in the adapter instead of fitting it.
    pub lower: f64,
    /// Exclusive upper bound.
    pub upper: f64,
}

/// The model adapter's ordered parameter and correspondence contract.
#[derive(Clone, Debug, Serialize)]
pub struct ModelLayout {
    /// Model/version/content identity supplied by the adapter, retained in the report.
    pub model_id: String,
    /// Shared shape parameters.
    pub shape: Vec<Parameter>,
    /// Independent parameters in each frame (root placement and selected joint angles).
    pub pose: Vec<Parameter>,
    /// Correspondence labels in the exact order returned by `points`.
    pub keypoints: Vec<String>,
    /// Metric scalar labels in the exact order returned by `measurements`.
    pub measurements: Vec<String>,
}

/// Boundary to a parametric body without a measure -> body crate dependency.
/// Implementations must be deterministic and return finite, correctly ordered metre values.
/// A pose may move the body between frames; it must never change the shared shape parameters.
pub trait BodyModel {
    /// Bounds, correspondence names and model identity.
    fn layout(&self) -> &ModelLayout;
    /// World-space keypoints in metres, z up, under the supplied shape and pose.
    fn points(&self, shape: &[f64], pose: &[f64]) -> Result<Vec<Vec3>, Refusal>;
    /// Rest-body tape functionals in metres, independent of pose.
    fn measurements(&self, shape: &[f64]) -> Result<Vec<f64>, Refusal>;
    /// Posed world-space triangle surface for optional silhouette evidence.
    /// Models without a surface retain keypoint/tape fitting but refuse silhouette requests.
    fn surface(&self, _shape: &[f64], _pose: &[f64]) -> Result<SurfaceMesh, Refusal> {
        Err(invalid(
            "this body model does not expose a silhouette surface",
        ))
    }
}

/// A 2D observation, not a model-inferred or triangulated point.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keypoint {
    /// Explicit model correspondence; no detector-to-model mapping is guessed.
    pub name: String,
    /// Oriented, full-resolution image coordinates.
    pub pixel_px: Vec2,
    /// Positive per-coordinate observation sigma, in pixels.
    pub sigma_px: f64,
    /// Held-out points do not enter the objective, derivatives or rank tests.
    pub partition: Partition,
}

/// One decoded image, with an independently variable body pose and a fixed metric camera.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitFrame {
    /// Decoded stream/timestamp identity; duplicated identities are refused.
    pub frame: FrameId,
    /// Camera calibrated in the common metric frame.
    pub camera: Camera,
    /// Dimensions after orientation has already been applied.
    pub image_size_px: [u32; 2],
    /// Recorded clockwise rotation already applied to pixels and calibration.
    pub rotation_cw_deg: u16,
    /// Fit and independently held-out evidence, with unique correspondence names.
    pub keypoints: Vec<Keypoint>,
    /// Optional fixed scanlines from an oriented, rectified body mask. Absence preserves /1 JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silhouette: Option<Silhouette>,
}

/// Independent tape evidence that constrains shared shape, never a population prior.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricObservation {
    /// Adapter measurement name.
    pub name: String,
    /// Positive measured length in metres.
    pub value_m: f64,
    /// Positive uncertainty in metres. Not an accuracy certificate for the fitted body.
    pub sigma_m: f64,
    /// Source/measurement-definition receipt, retained verbatim.
    pub evidence: String,
    /// Held-out tape targets check the result without steering it.
    pub partition: Partition,
}

/// Named pose vector so canonical frame sorting never misattaches a starting pose.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FramePose {
    /// Decoded frame identity.
    pub frame: FrameId,
    /// Values in `ModelLayout::pose` order.
    pub values: Vec<f64>,
}

/// One independent initialization; starts are not evidence and supply no regularization.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitStart {
    /// Values in `ModelLayout::shape` order.
    pub shape: Vec<f64>,
    /// One initial pose for each frame.
    pub poses: Vec<FramePose>,
}

/// A calibrated multi-frame articulated fit with at least two distinct initializations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyFitInput {
    /// `ftx-body-fit-input/1`.
    pub schema: String,
    /// Receipt establishing the metric camera frame and scale.
    pub metric_scale_source: String,
    /// Source and correspondence-definition receipt for the image observations.
    pub observation_evidence: String,
    /// Between two and eight distinct decoded frames; the subject may move between them.
    pub frames: Vec<FitFrame>,
    /// Up to 32 independent rest-body tape constraints, not guessed chart measurements.
    pub measurements: Vec<MetricObservation>,
    /// Two to four starts; a failed start is reported and prevents accepted geometry.
    pub starts: Vec<FitStart>,
}

/// Bounded numerical policy. Residual gates are fixed, not relaxed through large sigmas.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct FitOptions {
    /// Maximum accepted steps per start, 1..=100.
    pub iterations: usize,
    /// Largest permitted shared-shape spread as a fraction of each parameter's range.
    pub max_shape_spread_fraction: f64,
    /// Largest per-frame pose spread, normalized by each declared parameter range.
    /// This is a conservative parameter-space gate, not an angular equivalence test.
    pub max_pose_spread_fraction: f64,
    /// Total points/measurements/surface calls across ALL starts, derivatives, trials and output.
    /// Failed calls consume budget too; layout queries do not. Not a wall-clock limit.
    pub max_model_evaluations: usize,
}

impl Default for FitOptions {
    fn default() -> Self {
        Self {
            iterations: 50,
            max_shape_spread_fraction: 0.01,
            max_pose_spread_fraction: 0.01,
            max_model_evaluations: 100_000,
        }
    }
}

/// A residual with its unit and partition preserved.
#[derive(Clone, Debug, Serialize)]
pub struct FitCheck {
    /// Frame identity for pixels, absent for a tape target.
    pub frame: Option<FrameId>,
    /// Keypoint or tape name.
    pub name: String,
    /// Fit or held-out evidence.
    pub partition: Partition,
    /// `px` or `m`.
    pub unit: &'static str,
    /// Absolute residual; absent when projection failed.
    pub residual: Option<f64>,
    /// Fixed pixel gate (3 px), or three times the declared tape sigma.
    pub limit: f64,
    /// Whether this observation passes its independent acceptance gate.
    pub passed: bool,
}

/// Every initialization is retained; no unsuccessful run is silently excluded.
#[derive(Clone, Debug, Serialize)]
pub struct FitAttempt {
    /// CONVERGED, REJECTED, UNOBSERVABLE, NOT_CONVERGED or BUDGET_STOP.
    pub status: &'static str,
    /// Initial and strictly decreasing accepted sigma-weighted costs.
    pub cost_history: Vec<f64>,
    /// Residual checks for a converged solve, including held-out evidence.
    pub checks: Vec<FitCheck>,
    /// Concrete numerical or evidence failures.
    pub flags: Vec<String>,
    /// Model calls consumed by this start, including failed evaluations and validation.
    pub model_evaluations: usize,
}

/// Parameter disagreement for one decoded frame; never confused with physical pose values.
#[derive(Clone, Debug, Serialize)]
pub struct PoseSpread {
    /// Canonical decoded-frame identity.
    pub frame: FrameId,
    /// High minus low over converged starts, in normalized `ModelLayout::pose` order.
    pub fractions: Vec<f64>,
}

/// Accepted local model estimate, not a measured body or an uncertainty estimate.
#[derive(Clone, Debug, Serialize)]
pub struct BodyFitSolution {
    /// One shared shape vector for all frames.
    pub shape: Vec<f64>,
    /// Independent per-frame root/joint parameters.
    pub poses: Vec<FramePose>,
    /// Model predictions in layout measurement order, not records eligible for evidence fusion.
    pub predicted_measurements_m: Vec<f64>,
}

/// Complete fit receipt with fail-closed publication of optimized geometry.
#[derive(Clone, Debug, Serialize)]
pub struct BodyFitReport {
    /// `ftx-body-fit/1`.
    pub schema: &'static str,
    /// ACCEPTED, REJECTED, AMBIGUOUS, UNOBSERVABLE, NOT_CONVERGED or BUDGET_STOP.
    pub status: &'static str,
    /// Canonical input with every original observation and initialization.
    pub input: BodyFitInput,
    /// Adapter identity and the ordered parameter/correspondence contract.
    pub model: ModelLayout,
    /// Numerical and multistart policy used for every attempt.
    pub options: FitOptions,
    /// One diagnostic cell per start, including failures.
    pub attempts: Vec<FitAttempt>,
    /// Shared-shape range over all converged starts, normalized by parameter bounds.
    pub shape_spread_fraction: Option<Vec<f64>>,
    /// Per-frame pose disagreement, populated only when all attempts converged and passed.
    pub pose_spread_fraction: Option<Vec<PoseSpread>>,
    /// Total model calls, including any final publication check outside the attempts.
    pub model_evaluations: usize,
    /// Hard failures after individual attempts, such as ambiguity or a final budget stop.
    pub flags: Vec<String>,
    /// Present only when every start, residual check and shape/pose spread gate passed.
    pub solution: Option<BodyFitSolution>,
    /// Explicit scope limitations and missing independent checks.
    pub advisories: Vec<String>,
    /// Boundary of this inference.
    pub no_claim: &'static str,
}

struct EvaluationBudget {
    used: usize,
    limit: usize,
}

impl EvaluationBudget {
    fn call<T>(&mut self, evaluate: impl FnOnce() -> Result<T, Refusal>) -> Result<T, Refusal> {
        if self.used >= self.limit {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "global body-model evaluation budget exhausted",
            ));
        }
        self.used += 1;
        evaluate()
    }
}

impl FitAttempt {
    fn failed_evaluation(&mut self, error: &Refusal) {
        if error.code == RefusalCode::BudgetExceeded {
            self.status = "BUDGET_STOP";
        }
        self.flags.push(error.to_string());
    }
}

fn text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 4096
}
fn unique<'a>(names: impl Iterator<Item = &'a str>) -> bool {
    let mut set = BTreeSet::new();
    names.into_iter().all(|s| text(s) && set.insert(s))
}

/// Validate and canonically order observations and declared initializations without fitting.
/// This performs the same identity, parameter-bound and distinct-start checks as [`fit_body`].
/// It evaluates no body model and establishes neither observability nor acceptance.
pub fn prepare_input(input: &BodyFitInput, layout: &ModelLayout) -> Result<BodyFitInput, Refusal> {
    canonical(input, layout, 2)
}

/// Prepare an initialization editor, allowing zero or one declared start while it is incomplete.
/// All existing starts and observations retain the ordinary validation rules. Fitting and
/// materializing edited starts must still use [`prepare_input`], which requires 2..4 starts.
pub fn prepare_start_authoring(
    input: &BodyFitInput,
    layout: &ModelLayout,
) -> Result<BodyFitInput, Refusal> {
    canonical(input, layout, 0)
}

fn canonical(
    input: &BodyFitInput,
    layout: &ModelLayout,
    minimum_starts: usize,
) -> Result<BodyFitInput, Refusal> {
    let ns = layout.shape.len();
    let np = layout.pose.len();
    if input.schema != "ftx-body-fit-input/1"
        || !text(&input.metric_scale_source)
        || !text(&input.observation_evidence)
        || !text(&layout.model_id)
        || !(2..=8).contains(&input.frames.len())
        || !(minimum_starts..=4).contains(&input.starts.len())
        || !(1..=12).contains(&ns)
        || !(1..=18).contains(&np)
        || ns + np * input.frames.len() > 128
        || input.measurements.len() > 32
        || layout.keypoints.is_empty()
        || layout.keypoints.len() > 128
        || layout.measurements.len() > 32
    {
        return Err(invalid(
            "invalid body-fit schema, evidence or model/input budget",
        ));
    }
    for params in [&layout.shape, &layout.pose] {
        if !unique(params.iter().map(|p| p.name.as_str()))
            || params.iter().any(|p| {
                !p.lower.is_finite()
                    || !p.upper.is_finite()
                    || p.lower >= p.upper
                    || !(p.upper - p.lower).is_finite()
            })
        {
            return Err(invalid(
                "model parameters need unique names and finite ordered bounds",
            ));
        }
    }
    if !unique(layout.keypoints.iter().map(String::as_str))
        || !unique(layout.measurements.iter().map(String::as_str))
    {
        return Err(invalid("model correspondence names must be unique"));
    }
    let mut input = input.clone();
    input.frames.sort_by(|a, b| a.frame.cmp(&b.frame));
    let mut ids = BTreeSet::new();
    for frame in &mut input.frames {
        frame.camera.validate()?;
        if let Some(silhouette) = &mut frame.silhouette {
            silhouette.validate(frame.image_size_px)?;
            silhouette::require_rectified(&frame.camera)?;
            silhouette.canonicalize();
        }
        if !text(&frame.frame.stream)
            || !ids.insert(frame.frame.clone())
            || frame
                .image_size_px
                .iter()
                .any(|n| !(2..=1_000_000).contains(n))
            || ![0, 90, 180, 270].contains(&frame.rotation_cw_deg)
            || frame.keypoints.len() > 128
            || !unique(frame.keypoints.iter().map(|p| p.name.as_str()))
        {
            return Err(invalid(
                "invalid or duplicate frame, image metadata or keypoint identity",
            ));
        }
        frame.keypoints.sort_by(|a, b| a.name.cmp(&b.name));
        for point in &frame.keypoints {
            if !layout.keypoints.contains(&point.name)
                || !point.pixel_px.is_finite()
                || point.pixel_px.x < 0.0
                || point.pixel_px.y < 0.0
                || point.pixel_px.x > f64::from(frame.image_size_px[0] - 1)
                || point.pixel_px.y > f64::from(frame.image_size_px[1] - 1)
                || !point.sigma_px.is_finite()
                || !(1e-6..=1e6).contains(&point.sigma_px)
            {
                return Err(invalid("unknown correspondence or invalid pixel/sigma"));
            }
            frame.camera.ray(point.pixel_px)?;
        }
    }
    if !unique(input.measurements.iter().map(|p| p.name.as_str())) {
        return Err(invalid(
            "duplicate tape targets are not independent evidence",
        ));
    }
    input.measurements.sort_by(|a, b| a.name.cmp(&b.name));
    for m in &input.measurements {
        if !layout.measurements.contains(&m.name)
            || !text(&m.evidence)
            || !m.value_m.is_finite()
            || m.value_m <= 0.0
            || !m.sigma_m.is_finite()
            || !(1e-6..=1.0).contains(&m.sigma_m)
        {
            return Err(invalid(
                "unknown or invalid metric target/uncertainty/evidence",
            ));
        }
    }
    for start in &mut input.starts {
        start.poses.sort_by(|a, b| a.frame.cmp(&b.frame));
        if start.poses.len() != input.frames.len()
            || start
                .poses
                .iter()
                .zip(&input.frames)
                .any(|(pose, frame)| pose.frame != frame.frame)
        {
            return Err(invalid(
                "each start needs exactly one correctly identified pose per frame",
            ));
        }
        encode(start, layout)?;
    }
    let encoded: Vec<_> = input
        .starts
        .iter()
        .map(|s| encode(s, layout))
        .collect::<Result<_, _>>()?;
    for (i, x) in encoded.iter().enumerate() {
        if encoded[..i]
            .iter()
            .any(|y| x.iter().zip(y).all(|(a, b)| (a - b).abs() < 1e-6))
        {
            return Err(invalid(
                "multistart requires distinct initializations, not duplicate copies",
            ));
        }
    }
    Ok(input)
}

fn encode(start: &FitStart, layout: &ModelLayout) -> Result<Vec<f64>, Refusal> {
    let mut x = Vec::new();
    for (values, params) in std::iter::once((&start.shape, &layout.shape))
        .chain(start.poses.iter().map(|p| (&p.values, &layout.pose)))
    {
        if values.len() != params.len() {
            return Err(invalid("starting parameter vector has wrong length"));
        }
        for (&v, p) in values.iter().zip(params) {
            let u = (v - p.lower) / (p.upper - p.lower);
            if !v.is_finite() || !(0.0..1.0).contains(&u) || u == 0.0 {
                return Err(invalid(format!(
                    "{} must start strictly inside its bounds",
                    p.name
                )));
            }
            x.push(u);
        }
    }
    Ok(x)
}

fn values(x: &[f64], params: &[Parameter]) -> Vec<f64> {
    x.iter()
        .zip(params)
        .map(|(&v, p)| p.lower + v * (p.upper - p.lower))
        .collect()
}

struct Prediction {
    points: Vec<Vec<Vec3>>,
    metrics: Vec<f64>,
}
fn predict(
    model: &impl BodyModel,
    x: &[f64],
    frames: usize,
    budget: &mut EvaluationBudget,
) -> Result<Prediction, Refusal> {
    let l = model.layout();
    let shape = values(&x[..l.shape.len()], &l.shape);
    let metrics = budget.call(|| model.measurements(&shape))?;
    if metrics.len() != l.measurements.len() || metrics.iter().any(|v| !v.is_finite() || *v <= 0.0)
    {
        return Err(invalid("model returned invalid measurement vector"));
    }
    let mut points = Vec::with_capacity(frames);
    for f in 0..frames {
        let begin = l.shape.len() + f * l.pose.len();
        let pose = values(&x[begin..begin + l.pose.len()], &l.pose);
        let p = budget.call(|| model.points(&shape, &pose))?;
        if p.len() != l.keypoints.len() || p.iter().any(|v| !v.is_finite()) {
            return Err(invalid("model returned invalid keypoint vector"));
        }
        points.push(p);
    }
    Ok(Prediction { points, metrics })
}

fn residual(
    input: &BodyFitInput,
    model: &impl BodyModel,
    x: &[f64],
    budget: &mut EvaluationBudget,
) -> Result<derivatives::Residual, Refusal> {
    let p = predict(model, x, input.frames.len(), budget)?;
    let l = model.layout();
    let mut r = Vec::new();
    let mut frame_rows = Vec::with_capacity(input.frames.len());
    let ns = l.shape.len();
    let shape = values(&x[..ns], &l.shape);
    for (f, frame) in input.frames.iter().enumerate() {
        let begin = ns + f * l.pose.len();
        let first = r.len();
        r.extend(derivatives::frame_residual(
            frame,
            model,
            &shape,
            &values(&x[begin..begin + l.pose.len()], &l.pose),
            &p.points[f],
            budget,
        )?);
        frame_rows.push(first..r.len());
    }
    for obs in input
        .measurements
        .iter()
        .filter(|o| o.partition == Partition::Fit)
    {
        let k = l.measurements.iter().position(|n| n == &obs.name).unwrap();
        r.push((p.metrics[k] - obs.value_m) / obs.sigma_m);
    }
    if r.iter().any(|v| !v.is_finite()) || !cost(&r).is_finite() {
        return Err(invalid("non-finite body-fit objective"));
    }
    Ok(derivatives::Residual {
        values: r,
        frame_rows,
    })
}
fn cost(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum()
}

// Cholesky tests the UNDAMPED, column-normalized Gram matrix before any LM prior is added.
// This conservative local rank test is not a global observability or covariance certificate.
fn factor(a: &[Vec<f64>], floor: f64) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let v = a[i][j] - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
            if !v.is_finite() {
                return None;
            }
            if i == j {
                if v <= floor {
                    return None;
                }
                l[i][j] = v.sqrt();
            } else {
                l[i][j] = v / l[j][j];
            }
        }
    }
    Some(l)
}
fn solve(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = b.len();
    let mut x = b.to_vec();
    for i in 0..n {
        x[i] = (x[i] - (0..i).map(|j| l[i][j] * x[j]).sum::<f64>()) / l[i][i];
    }
    for i in (0..n).rev() {
        x[i] = (x[i] - (i + 1..n).map(|j| l[j][i] * x[j]).sum::<f64>()) / l[i][i];
    }
    x
}

fn checks(
    input: &BodyFitInput,
    model: &impl BodyModel,
    x: &[f64],
    budget: &mut EvaluationBudget,
) -> Result<Vec<FitCheck>, Refusal> {
    let p = predict(model, x, input.frames.len(), budget)?;
    let l = model.layout();
    let mut out = Vec::new();
    for (f, frame) in input.frames.iter().enumerate() {
        for obs in &frame.keypoints {
            let k = l.keypoints.iter().position(|n| n == &obs.name).unwrap();
            let error = frame
                .camera
                .project(p.points[f][k])
                .ok()
                .map(|q| q.dist(obs.pixel_px))
                .filter(|e| e.is_finite());
            out.push(FitCheck {
                frame: Some(frame.frame.clone()),
                name: obs.name.clone(),
                partition: obs.partition,
                unit: "px",
                residual: error,
                limit: 3.0,
                passed: error.is_some_and(|e| e <= 3.0),
            });
        }
        if let Some(s) = &frame.silhouette {
            let ns = l.shape.len();
            let begin = ns + f * l.pose.len();
            let mesh = budget.call(|| {
                model.surface(
                    &values(&x[..ns], &l.shape),
                    &values(&x[begin..begin + l.pose.len()], &l.pose),
                )
            })?;
            for line in silhouette::compare(&mesh, &frame.camera, frame.image_size_px, s, None)? {
                out.push(FitCheck {
                    frame: Some(frame.frame.clone()),
                    name: format!("silhouette:{:?}:{}", line.axis, line.at_px),
                    partition: line.partition,
                    unit: "px",
                    residual: line.boundary_error_px,
                    limit: 3.0,
                    passed: line.passed,
                });
            }
        }
    }
    for obs in &input.measurements {
        let k = l.measurements.iter().position(|n| n == &obs.name).unwrap();
        let error = (p.metrics[k] - obs.value_m).abs();
        out.push(FitCheck {
            frame: None,
            name: obs.name.clone(),
            partition: obs.partition,
            unit: "m",
            residual: error.is_finite().then_some(error),
            limit: 3.0 * obs.sigma_m,
            passed: error <= 3.0 * obs.sigma_m,
        });
    }
    Ok(out)
}

fn attempt(
    input: &BodyFitInput,
    model: &impl BodyModel,
    start: &FitStart,
    options: &FitOptions,
    budget: &mut EvaluationBudget,
) -> (FitAttempt, Option<Vec<f64>>) {
    let mut result = FitAttempt {
        status: "NOT_CONVERGED",
        cost_history: Vec::new(),
        checks: Vec::new(),
        flags: Vec::new(),
        model_evaluations: 0,
    };
    let mut x = encode(start, model.layout()).expect("validated start");
    let mut r = match residual(input, model, &x, budget) {
        Ok(r) => r,
        Err(e) => {
            result.failed_evaluation(&e);
            return (result, None);
        }
    };
    result.cost_history.push(cost(&r.values));
    let n = x.len();
    let mut damping = 1e-3_f64;
    if r.values.len() < n {
        result.status = "UNOBSERVABLE";
        result
            .flags
            .push("fewer fit residuals than free parameters".into());
        return (result, None);
    }
    for iteration in 0..=options.iterations {
        let mut cols = Vec::with_capacity(n);
        for k in 0..n {
            let h = 1e-5_f64.min(0.2 * x[k]).min(0.2 * (1.0 - x[k]));
            if h < 1e-9 {
                result.flags.push(
                    "fit reached a parameter boundary; fix it explicitly or revise the model"
                        .into(),
                );
                return (result, None);
            }
            match derivatives::column(input, model, &x, k, h, &r, budget) {
                Ok(col) => cols.push(col),
                Err(e) => {
                    result.flags.push("derivative unavailable".into());
                    result.failed_evaluation(&e);
                    return (result, None);
                }
            }
        }
        let norms: Vec<f64> = cols.iter().map(|c| cost(c).sqrt()).collect();
        if norms.iter().any(|v| !v.is_finite() || *v <= 1e-12) {
            result.status = "UNOBSERVABLE";
            result
                .flags
                .push("fit data do not constrain every shape/pose parameter".into());
            return (result, None);
        }
        for (col, &norm) in cols.iter_mut().zip(&norms) {
            for v in col {
                *v /= norm;
            }
        }
        let a: Vec<Vec<f64>> = cols
            .iter()
            .map(|ci| {
                cols.iter()
                    .map(|cj| ci.iter().zip(cj).map(|(i, j)| i * j).sum())
                    .collect()
            })
            .collect();
        if factor(&a, 1e-9).is_none() {
            result.status = "UNOBSERVABLE";
            result.flags.push("rank-deficient or ill-conditioned fit data; tape evidence or better views are needed".into());
            return (result, None);
        }
        let g: Vec<f64> = cols
            .iter()
            .map(|c| c.iter().zip(&r.values).map(|(j, v)| j * v).sum())
            .collect();
        if g.iter()
            .all(|v| v.abs() <= 1e-6 * cost(&r.values).sqrt().max(1.0))
        {
            match checks(input, model, &x, budget) {
                Ok(c) => result.checks = c,
                Err(e) => {
                    result.failed_evaluation(&e);
                    return (result, None);
                }
            }
            result.status = if result.checks.iter().all(|c| c.passed) {
                "CONVERGED"
            } else {
                "REJECTED"
            };
            return (result, Some(x));
        }
        if iteration == options.iterations {
            result.status = "BUDGET_STOP";
            result.flags.push("accepted-step budget exhausted".into());
            return (result, None);
        }
        let mut accepted = None;
        for _ in 0..16 {
            let mut damped = a.clone();
            for (i, row) in damped.iter_mut().enumerate() {
                row[i] += damping;
            }
            let Some(l) = factor(&damped, 0.0) else {
                damping *= 10.0;
                continue;
            };
            let step = solve(&l, &g.iter().map(|v| -v).collect::<Vec<_>>());
            let trial: Vec<f64> = x
                .iter()
                .zip(step.iter().zip(&norms))
                .map(|(v, (s, n))| v + s / n)
                .collect();
            if trial.iter().all(|v| v.is_finite() && *v > 0.0 && *v < 1.0) {
                match residual(input, model, &trial, budget) {
                    Ok(next) if cost(&next.values) < cost(&r.values) => {
                        accepted = Some((trial, next));
                        damping = (damping * 0.3).max(1e-12);
                        break;
                    }
                    Err(e) if e.code == RefusalCode::BudgetExceeded => {
                        result.failed_evaluation(&e);
                        return (result, None);
                    }
                    _ => {}
                }
            }
            damping *= 10.0;
        }
        let Some((next, next_r)) = accepted else {
            result
                .flags
                .push("no valid objective-decreasing step".into());
            return (result, None);
        };
        x = next;
        r = next_r;
        result.cost_history.push(cost(&r.values));
    }
    unreachable!("bounded loop returns on its final iteration")
}

/// Fit articulated observations with one shared shape, independent poses, optional tape constraints
/// and explicit multistart checks. No learned detector, population prior or body-model dependency.
/// Every starting cell must converge and pass all evidence gates; failure publishes no solution.
pub fn fit_body(
    input: &BodyFitInput,
    model: &impl BodyModel,
    options: &FitOptions,
) -> Result<BodyFitReport, Refusal> {
    if !(1..=100).contains(&options.iterations)
        || !options.max_shape_spread_fraction.is_finite()
        || !(1e-6..=0.1).contains(&options.max_shape_spread_fraction)
        || !options.max_pose_spread_fraction.is_finite()
        || !(1e-6..=0.1).contains(&options.max_pose_spread_fraction)
        || !(1..=1_000_000).contains(&options.max_model_evaluations)
    {
        return Err(invalid(
            "invalid body-fit iteration, model-evaluation or multistart-spread budget",
        ));
    }
    let input = prepare_input(input, model.layout())?;
    let mut report = BodyFitReport {
        schema: "ftx-body-fit/1",
        status: "REJECTED",
        input: input.clone(),
        model: model.layout().clone(),
        options: *options,
        attempts: Vec::new(),
        shape_spread_fraction: None,
        pose_spread_fraction: None,
        model_evaluations: 0,
        flags: Vec::new(),
        solution: None,
        advisories: Vec::new(),
        no_claim: "Local model estimate conditional on camera calibration, correspondences and tape evidence. Not measured anatomy, a global uniqueness proof, a covariance estimate or a real-wearer fit claim.",
    };
    if !input.frames.iter().any(|f| {
        f.keypoints
            .iter()
            .any(|p| p.partition == Partition::HeldOut)
            || f.silhouette.as_ref().is_some_and(|s| {
                s.lines
                    .iter()
                    .any(|line| line.partition == Partition::HeldOut)
            })
    }) {
        report
            .advisories
            .push("No held-out image observations were supplied.".into());
    }
    report.advisories.push("Cameras, lens parameters and tape definitions are fixed; their systematic uncertainty is not estimated.".into());
    if input.frames.iter().any(|f| f.silhouette.is_some()) {
        report.advisories.push("Silhouettes are checked only on supplied rectified scanlines, not over the entire mask. Segmentation, clothing, occlusion and contour correlation are caller assumptions; contour weights are not a calibrated independent-noise model.".into());
    } else {
        report
            .advisories
            .push("No silhouette observations were supplied.".into());
    }
    let mut states = Vec::new();
    let mut budget = EvaluationBudget {
        used: 0,
        limit: options.max_model_evaluations,
    };
    for start in &input.starts {
        let before = budget.used;
        let (mut a, x) = attempt(&input, model, start, options, &mut budget);
        a.model_evaluations = budget.used - before;
        report.attempts.push(a);
        states.push(x);
    }
    report.model_evaluations = budget.used;
    for status in ["BUDGET_STOP", "UNOBSERVABLE", "NOT_CONVERGED", "REJECTED"] {
        if report.attempts.iter().any(|a| a.status == status) {
            report.status = status;
            return Ok(report);
        }
    }
    let ns = model.layout().shape.len();
    let parameter_spread = |k: usize| {
        let lo = states
            .iter()
            .map(|s| s.as_ref().unwrap()[k])
            .fold(f64::INFINITY, f64::min);
        let hi = states
            .iter()
            .map(|s| s.as_ref().unwrap()[k])
            .fold(f64::NEG_INFINITY, f64::max);
        hi - lo
    };
    let spread: Vec<f64> = (0..ns).map(parameter_spread).collect();
    let shape_ambiguous = spread
        .iter()
        .any(|s| *s > options.max_shape_spread_fraction);
    report.shape_spread_fraction = Some(spread);
    let np = model.layout().pose.len();
    let poses: Vec<PoseSpread> = input
        .frames
        .iter()
        .enumerate()
        .map(|(i, f)| PoseSpread {
            frame: f.frame.clone(),
            fractions: (ns + i * np..ns + (i + 1) * np)
                .map(parameter_spread)
                .collect(),
        })
        .collect();
    let pose_ambiguous = poses.iter().any(|p| {
        p.fractions
            .iter()
            .any(|s| *s > options.max_pose_spread_fraction)
    });
    report.pose_spread_fraction = Some(poses);
    if shape_ambiguous || pose_ambiguous {
        report.status = "AMBIGUOUS";
        if shape_ambiguous {
            report
                .flags
                .push("converged starts disagree on shared shape".into());
        }
        if pose_ambiguous {
            report
                .flags
                .push("converged starts disagree on a frame's pose parameters".into());
        }
        return Ok(report);
    }
    let best = (0..states.len())
        .min_by(|&i, &j| {
            report.attempts[i]
                .cost_history
                .last()
                .unwrap()
                .total_cmp(report.attempts[j].cost_history.last().unwrap())
                .then(i.cmp(&j))
        })
        .unwrap();
    let x = states[best].as_ref().unwrap();
    let l = model.layout();
    let shape = values(&x[..ns], &l.shape);
    let poses = input
        .frames
        .iter()
        .enumerate()
        .map(|(i, f)| FramePose {
            frame: f.frame.clone(),
            values: values(
                &x[ns + i * l.pose.len()..ns + (i + 1) * l.pose.len()],
                &l.pose,
            ),
        })
        .collect();
    let final_metrics = budget.call(|| model.measurements(&shape));
    report.model_evaluations = budget.used;
    let predicted_measurements_m = match final_metrics {
        Ok(m) if m.len() == l.measurements.len() && m.iter().all(|v| v.is_finite() && *v > 0.0) => {
            m
        }
        Ok(_) => {
            report
                .flags
                .push("model returned invalid final measurements".into());
            return Ok(report);
        }
        Err(e) => {
            if e.code == RefusalCode::BudgetExceeded {
                report.status = "BUDGET_STOP";
            }
            report.flags.push(e.to_string());
            return Ok(report);
        }
    };
    report.solution = Some(BodyFitSolution {
        predicted_measurements_m,
        shape,
        poses,
    });
    report.status = "ACCEPTED";
    Ok(report)
}
