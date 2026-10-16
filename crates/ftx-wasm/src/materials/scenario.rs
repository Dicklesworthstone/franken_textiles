//! File-free composition of real drape/heat with the existing material manufacturing study.
use ftx_body::AnnyModel;
use ftx_core::Refusal;
use ftx_drape::{
    run::DrapeBody,
    study::{Scenario, Session, fitted::FittedBody},
};
use ftx_export::material_study::Request;
use ftx_pattern::Pattern;
use serde_json::{Value, json};

use super::{budget, invalid};

/// Evaluate source-locked alternatives on a caller's fixed body using production consumers.
/// This also gives native conformance tests a file-free seam without a converted Anny asset.
///
/// # Errors
/// Invalid or excessive input. Drape/thermal failures remain per-consumer outcomes.
pub fn on_body(
    body: &DrapeBody<'_>,
    pattern: &str,
    request: &str,
    scenario: &str,
    mut progress: impl FnMut(&str),
) -> Result<String, Refusal> {
    budget(&[pattern, request, scenario])?;
    let pattern = Pattern::from_json(pattern).map_err(invalid)?;
    let request: Request = serde_json::from_str(request).map_err(invalid)?;
    if let Some(policy) = &request.comparison {
        policy.validate(&request, true)?;
    }
    let scenario: Scenario = serde_json::from_str(scenario).map_err(invalid)?;
    let mut session = Session::new(&scenario, body)?;
    let descriptor = session.descriptor();
    let report = request.evaluate_with(&pattern, &descriptor, |key, pattern| {
        session.evaluate(pattern, |line| progress(&format!("{key}: {line}")))
    })?;
    serde_json::to_string(&report).map_err(invalid)
}

/// Fit one model once, preserving manufacturing outcomes even when body setup refuses.
/// Model bytes are already parsed by the browser's WebBody; its exact source hash is retained.
///
/// # Errors
/// Invalid/oversized JSON or scenario policy. Body/consumer failures remain independent outcomes.
pub fn on_model(
    model: &AnnyModel,
    model_sha256: &str,
    pattern: &str,
    request: &str,
    scenario: &str,
    measurements: &str,
    mut progress: impl FnMut(&str),
) -> Result<String, Refusal> {
    budget(&[pattern, request, scenario, measurements, model_sha256])?;
    let pattern = Pattern::from_json(pattern).map_err(invalid)?;
    let request: Request = serde_json::from_str(request).map_err(invalid)?;
    if let Some(policy) = &request.comparison {
        policy.validate(&request, true)?;
    }
    let scenario: Scenario = serde_json::from_str(scenario).map_err(invalid)?;
    let measurements: Value = serde_json::from_str(measurements).map_err(invalid)?;
    scenario.validate()?;
    let fitted = FittedBody::fit(model, &measurements);
    let body = fitted.as_ref().map(FittedBody::body).map_err(Clone::clone);
    let mut session = body
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|body| Session::new(&scenario, body));
    let mut descriptor = match &session {
        Ok(session) => session.descriptor(),
        Err(refusal) => json!({"scenario":scenario,"body_setup_refusal":refusal}),
    };
    descriptor["model_sha256"] = json!(model_sha256);
    descriptor["measurements"] = measurements;
    descriptor["body_fit"] = fitted
        .as_ref()
        .map_or(Value::Null, |body| body.report.clone());
    let report = request.evaluate_with(&pattern, &descriptor, |key, pattern| {
        session
            .as_mut()
            .map_err(|e| e.clone())?
            .evaluate(pattern, |line| {
                progress(&format!("{key}: {line}"));
            })
    })?;
    serde_json::to_string(&report).map_err(invalid)
}
