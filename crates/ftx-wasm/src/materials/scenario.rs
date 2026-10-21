//! File-free composition of real drape/heat with the existing material manufacturing study.
use ftx_body::AnnyModel;
use ftx_core::Refusal;
use ftx_drape::{
    run::DrapeBody,
    study::{
        Scenario, Session,
        fitted::{BodyRequest, FittedBody},
    },
    study_input::PreparedInput,
};
use ftx_export::material_study::Request;
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
    let source: Value = serde_json::from_str(pattern).map_err(invalid)?;
    let request: Request = serde_json::from_str(request).map_err(invalid)?;
    if let Some(policy) = &request.comparison {
        policy.validate(&request, true)?;
    }
    let scenario: Scenario = serde_json::from_str(scenario).map_err(invalid)?;
    scenario.validate()?;
    let prepared = PreparedInput::new(source, &scenario.options)?;
    let mut session = Session::new(&scenario, body)?;
    let mut descriptor = session.descriptor();
    prepared.annotate(&mut descriptor)?;
    let report = request.evaluate_with(prepared.pattern(), &descriptor, |key, pattern| {
        session.evaluate(pattern, |line| progress(&format!("{key}: {line}")))
    })?;
    serde_json::to_string(&report).map_err(invalid)
}

/// Select one shape once (tape or accepted report) and pose it independently. The last JSON input
/// accepts legacy measurements or ftx-scenario-body/1. Preserve manufacturing when setup refuses.
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
    let source: Value = serde_json::from_str(pattern).map_err(invalid)?;
    let request: Request = serde_json::from_str(request).map_err(invalid)?;
    if let Some(policy) = &request.comparison {
        policy.validate(&request, true)?;
    }
    let scenario: Scenario = serde_json::from_str(scenario).map_err(invalid)?;
    let body_input: Value = serde_json::from_str(measurements).map_err(invalid)?;
    scenario.validate()?;
    let prepared = PreparedInput::new(source, &scenario.options)?;
    let body_request = BodyRequest::read(&body_input);
    let fitted = body_request
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|request| FittedBody::prepare(model, model_sha256, request));
    let body = fitted.as_ref().map(FittedBody::body).map_err(Clone::clone);
    let mut session = body
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|body| Session::new(&scenario, body));
    if let (Ok(session), Ok(fitted), Some(motion)) = (&mut session, &fitted, &scenario.motion) {
        session.set_motion_path(fitted.motion_path(model, model_sha256, motion))?;
    }
    let mut descriptor = match &session {
        Ok(session) => session.descriptor(),
        Err(refusal) => json!({"scenario":scenario,"body_setup_refusal":refusal}),
    };
    descriptor["model_sha256"] = json!(model_sha256);
    descriptor["measurements"] = body_request
        .as_ref()
        .map_or(Value::Null, |request| request.measurements.clone());
    descriptor["body_input"] = body_input;
    descriptor["body_fit"] = fitted
        .as_ref()
        .map_or(Value::Null, |body| body.report.clone());
    prepared.annotate(&mut descriptor)?;
    let report = request.evaluate_with(prepared.pattern(), &descriptor, |key, pattern| {
        session
            .as_mut()
            .map_err(|e| e.clone())?
            .evaluate(pattern, |line| {
                progress(&format!("{key}: {line}"));
            })
    })?;
    serde_json::to_string(&report).map_err(invalid)
}
