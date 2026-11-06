//! Prepared multi-frame observations fitted locally with the canonical Anny adapter and solver.
//! File-backed capture acquisition remains native; this boundary never follows paths or loads images.

use ftx_body::{
    AnnyData, AnnyModel,
    correspondence::{Configuration, CorrespondenceModel},
};
use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_measure::{
    body_fit::{self, BodyFitInput, BodyModel, FitOptions, ModelLayout, Parameter},
    silhouette::SurfaceMesh,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    model: Configuration,
    fit: BodyFitInput,
}

struct Adapter {
    inner: CorrespondenceModel,
    layout: ModelLayout,
}

impl Adapter {
    fn new(inner: CorrespondenceModel) -> Self {
        let config = inner.configuration();
        let parameters = |source: &[ftx_body::correspondence::Parameter]| {
            source
                .iter()
                .map(|p| Parameter {
                    name: p.name.clone(),
                    lower: p.lower,
                    upper: p.upper,
                })
                .collect()
        };
        let layout = ModelLayout {
            model_id: format!("anny/0.6.1/sha256:{}", config.model_sha256),
            shape: parameters(&config.shape_parameters),
            pose: parameters(&config.pose_parameters),
            keypoints: config.keypoints.keys().cloned().collect(),
            measurements: config.measurements.clone(),
        };
        Self { inner, layout }
    }
}

impl BodyModel for Adapter {
    fn layout(&self) -> &ModelLayout {
        &self.layout
    }
    fn measurements(&self, shape: &[f64]) -> Result<Vec<f64>, Refusal> {
        self.inner.measurements(shape)
    }
    fn points(&self, shape: &[f64], pose: &[f64]) -> Result<Vec<Vec3>, Refusal> {
        self.inner.points(shape, pose)
    }
    fn surface(&self, shape: &[f64], pose: &[f64]) -> Result<SurfaceMesh, Refusal> {
        let posed = self.inner.posed(shape, pose)?;
        Ok(SurfaceMesh {
            vertices_m: posed.vertices,
            triangles: self
                .inner
                .model()
                .data()
                .faces
                .iter()
                .map(|t| t.map(|k| k as usize))
                .collect(),
        })
    }
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// Fit one shared shape and independent frame poses from original prepared JSON and model bytes.
/// The result is the ordinary `ftx-body-fit/1` report, including every failed/held-out/ambiguous
/// attempt and the exact input hash. Only accepted solutions contain reusable Anny parameters.
/// Textual work limits retain native integer parsing without JavaScript numeric truncation.
///
/// # Errors
/// More than 16 MiB input or 512 MiB model data, invalid prepared schema/controls, wrong model
/// identity/arrays, or the canonical solver's evidence and budget admission refusals. A solver
/// `BUDGET_STOP` is a diagnostic report, not an accepted partial shape.
pub fn fit_body(
    model_bytes: &[u8],
    input: &str,
    iterations: &str,
    max_evaluations: &str,
) -> Result<Value, Refusal> {
    if input.len() > 16 * 1024 * 1024 || model_bytes.len() > 512 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "body fit supports at most 16 MiB prepared input and 512 MiB converted model",
        ));
    }
    let request: Request = serde_json::from_str(input)
        .map_err(|e| invalid(format!("invalid Anny body-fit request: {e}")))?;
    if request.schema != "ftx-anny-body-fit-input/1" {
        return Err(invalid(
            "expected prepared ftx-anny-body-fit-input/1; file-backed capture acquisition requires native preparation",
        ));
    }
    let iterations = iterations
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=100).contains(n))
        .ok_or_else(|| invalid("--iterations must be 1..100"))?;
    let max_model_evaluations = max_evaluations
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=1_000_000).contains(n))
        .ok_or_else(|| invalid("--max-evaluations must be 1..1000000"))?;
    let hash = sha256_hex(model_bytes);
    if hash != request.model.model_sha256 {
        return Err(invalid("model SHA-256 mismatch; no fit was run"));
    }
    let model = AnnyModel::from_data(AnnyData::from_bytes(model_bytes, true)?)?;
    let adapter = Adapter::new(CorrespondenceModel::new(model, request.model, &hash)?);
    let report = body_fit::fit_body(
        &request.fit,
        &adapter,
        &FitOptions {
            iterations,
            max_model_evaluations,
            ..FitOptions::default()
        },
    )?;
    let mut value = serde_json::to_value(&report).map_err(|e| invalid(e.to_string()))?;
    value["anny_configuration"] =
        serde_json::to_value(adapter.inner.configuration()).map_err(|e| invalid(e.to_string()))?;
    value["input_sha256"] = json!(sha256_hex(input.as_bytes()));
    if let Some(solution) = &report.solution {
        let p = adapter.inner.parameters(&solution.shape)?;
        let h = p.phenotype;
        value["solution"]["anny_parameters"] = json!({"provenance":"MODEL_FIT",
            "phenotype":{"gender":h.gender,"age":h.age,"muscle":h.muscle,"weight":h.weight,"height":h.height,"proportions":h.proportions},
            "local_changes":p.local_changes});
    }
    Ok(value)
}
