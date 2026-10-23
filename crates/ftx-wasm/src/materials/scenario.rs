//! File-free composition of real drape/heat with the existing material manufacturing study.
use ftx_body::AnnyModel;
use ftx_core::Refusal;
use ftx_drape::{
    run::DrapeBody,
    study::{
        Scenario, Session,
        fitted::{BodyRequest, FittedBody},
        matrix::{Budget as MatrixBudget, Request as MatrixRequest},
    },
    study_input::PreparedInput,
};
use ftx_export::material_study::{
    Request, comparison::Policy, design::fit::Context, matrix::Case as MatrixCase,
};
use serde_json::{Value, json};

use super::{budget, invalid};

/// Evaluate source-locked alternatives on a caller's fixed body using production consumers.
/// This also gives native conformance tests a file-free seam without a converted Anny asset.
/// The scenario input also accepts `ftx-drape-scenario-matrix/1`, sharing one manufacturing study
/// and bounded work accounting across named cases while starting each numerical evaluation fresh.
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
    let document: Value = serde_json::from_str(scenario).map_err(invalid)?;
    if document["schema"] == "ftx-drape-scenario-matrix/1" {
        return on_body_matrix(
            body,
            source,
            &request,
            serde_json::from_value(document).map_err(invalid)?,
            progress,
        );
    }
    if let Some(policy) = &request.comparison {
        policy.validate(&request, true)?;
    }
    let scenario: Scenario = serde_json::from_value(document).map_err(invalid)?;
    scenario.validate()?;
    let prepared = PreparedInput::new(source, &scenario.options)?;
    let mut session = Session::new(&scenario, body)?;
    let mut descriptor = session.descriptor();
    annotate_raw_fit(&request, &mut descriptor);
    prepared.annotate(&mut descriptor)?;
    let report =
        request.evaluate_with_fit(prepared.pattern(), &descriptor, None, |key, pattern| {
            session.evaluate(pattern, |line| progress(&format!("{key}: {line}")))
        })?;
    serde_json::to_string(&report).map_err(invalid)
}

/// Select one shape once (tape or accepted report) and pose it independently. The last JSON input
/// accepts legacy measurements or ftx-scenario-body/1. Preserve manufacturing when setup refuses.
/// Model bytes are already parsed by the browser's WebBody; its exact source hash is retained.
/// Named scenario matrices fit the common shape once and prepare each case's own motion path.
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
    let document: Value = serde_json::from_str(scenario).map_err(invalid)?;
    if document["schema"] == "ftx-drape-scenario-matrix/1" {
        return on_model_matrix(
            model,
            model_sha256,
            source,
            &request,
            serde_json::from_value(document).map_err(invalid)?,
            measurements,
            progress,
        );
    }
    if let Some(policy) = &request.comparison {
        policy.validate(&request, true)?;
    }
    let scenario: Scenario = serde_json::from_value(document).map_err(invalid)?;
    let body_input: Value = serde_json::from_str(measurements).map_err(invalid)?;
    scenario.validate()?;
    let prepared = PreparedInput::new(source, &scenario.options)?;
    let body_request = BodyRequest::read(&body_input);
    let fitted = body_request
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|request| FittedBody::prepare(model, model_sha256, request));
    let fit_body = request.design_fit.as_ref().map(|_| {
        fitted
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|fitted| fitted.flat_fit_body(model, model_sha256))
    });
    let fit = fit_body
        .as_ref()
        .and_then(|body| body.as_ref().ok())
        .map(|body| Context {
            body,
            identity: body.identity(),
        });
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
    match &fit_body {
        Some(Ok(body)) => descriptor["design_fit_body"] = body.identity().clone(),
        Some(Err(refusal)) => descriptor["design_fit_setup_refusal"] = json!(refusal),
        None => {}
    }
    prepared.annotate(&mut descriptor)?;
    let report = request.evaluate_with_fit(
        prepared.pattern(),
        &descriptor,
        fit.as_ref(),
        |key, pattern| {
            session
                .as_mut()
                .map_err(|e| e.clone())?
                .evaluate(pattern, |line| {
                    progress(&format!("{key}: {line}"));
                })
        },
    )?;
    serde_json::to_string(&report).map_err(invalid)
}

fn annotate_raw_fit(request: &Request, descriptor: &mut Value) {
    if request.design_fit.is_some() {
        descriptor["design_fit_setup_refusal"] = json!(invalid(
            "design fit requires a selected fitted body; a raw drape body has no canonical flat-fit readings",
        ));
    }
}

// Resolve every effective policy before preparing a body or starting a numerical consumer.
// An omitted override stays omitted in the frozen case; an unused fallback is not a requirement.
fn matrix_policies(
    request: &Request,
    matrix: &MatrixRequest,
) -> Result<Vec<Option<Policy>>, Refusal> {
    matrix.validate()?;
    matrix
        .cases
        .iter()
        .map(|case| {
            let policy: Option<Policy> = case
                .comparison
                .clone()
                .map(serde_json::from_value)
                .transpose()
                .map_err(invalid)?;
            if let Some(effective) = policy.as_ref().or(request.comparison.as_ref()) {
                effective.validate(request, true)?;
            }
            Ok(policy)
        })
        .collect()
}

fn on_body_matrix(
    body: &DrapeBody<'_>,
    source: Value,
    request: &Request,
    matrix: MatrixRequest,
    progress: impl FnMut(&str),
) -> Result<String, Refusal> {
    let policies = matrix_policies(request, &matrix)?;
    let prepared = matrix.prepare(source)?;
    let mut sessions: Vec<_> = matrix
        .cases
        .iter()
        .map(|case| Session::new(&case.scenario, body))
        .collect();
    let mut cases = Vec::with_capacity(matrix.cases.len());
    for ((case, session), comparison) in matrix.cases.iter().zip(&sessions).zip(policies) {
        let mut descriptor = match session {
            Ok(session) => session.descriptor(),
            Err(refusal) => json!({"scenario":case.scenario,"body_setup_refusal":refusal}),
        };
        annotate_raw_fit(request, &mut descriptor);
        prepared.annotate(&mut descriptor)?;
        cases.push(MatrixCase {
            key: case.key.clone(),
            scenario: descriptor,
            comparison,
        });
    }
    evaluate_matrix(
        request,
        &prepared,
        &cases,
        matrix.max_evaluations,
        None,
        &mut sessions,
        progress,
    )
}

fn on_model_matrix(
    model: &AnnyModel,
    model_sha256: &str,
    source: Value,
    request: &Request,
    matrix: MatrixRequest,
    measurements: &str,
    progress: impl FnMut(&str),
) -> Result<String, Refusal> {
    let policies = matrix_policies(request, &matrix)?;
    let prepared = matrix.prepare(source)?;
    let body_input: Value = serde_json::from_str(measurements).map_err(invalid)?;
    let body_request = BodyRequest::read(&body_input);
    // Fit or select the common shape exactly once. Every case then owns a fresh session and
    // motion path; setup refusals remain case outcomes alongside the shared manufacturing study.
    let fitted = body_request
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|request| FittedBody::prepare(model, model_sha256, request));
    // Every case uses this one fixed reference-pose body, independently of its motion path.
    let fit_body = request.design_fit.as_ref().map(|_| {
        fitted
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|fitted| fitted.flat_fit_body(model, model_sha256))
    });
    let fit = fit_body
        .as_ref()
        .and_then(|body| body.as_ref().ok())
        .map(|body| Context {
            body,
            identity: body.identity(),
        });
    let body = fitted.as_ref().map(FittedBody::body).map_err(Clone::clone);
    let mut sessions = Vec::with_capacity(matrix.cases.len());
    let mut cases = Vec::with_capacity(matrix.cases.len());
    for (case, comparison) in matrix.cases.iter().zip(policies) {
        let session = body.as_ref().map_err(Clone::clone).and_then(|body| {
            let mut session = Session::new(&case.scenario, body)?;
            if let (Ok(fitted), Some(motion)) = (&fitted, &case.scenario.motion) {
                session.set_motion_path(fitted.motion_path(model, model_sha256, motion))?;
            }
            Ok(session)
        });
        let mut descriptor = match &session {
            Ok(session) => session.descriptor(),
            Err(refusal) => json!({"scenario":case.scenario,"body_setup_refusal":refusal}),
        };
        descriptor["model_sha256"] = json!(model_sha256);
        descriptor["measurements"] = body_request
            .as_ref()
            .map_or(Value::Null, |request| request.measurements.clone());
        descriptor["body_input"] = body_input.clone();
        descriptor["body_fit"] = fitted
            .as_ref()
            .map_or(Value::Null, |body| body.report.clone());
        match &fit_body {
            Some(Ok(body)) => descriptor["design_fit_body"] = body.identity().clone(),
            Some(Err(refusal)) => descriptor["design_fit_setup_refusal"] = json!(refusal),
            None => {}
        }
        prepared.annotate(&mut descriptor)?;
        sessions.push(session);
        cases.push(MatrixCase {
            key: case.key.clone(),
            scenario: descriptor,
            comparison,
        });
    }
    evaluate_matrix(
        request,
        &prepared,
        &cases,
        matrix.max_evaluations,
        fit.as_ref(),
        &mut sessions,
        progress,
    )
}

fn evaluate_matrix(
    request: &Request,
    prepared: &PreparedInput,
    cases: &[MatrixCase],
    max_evaluations: usize,
    fit: Option<&Context<'_>>,
    sessions: &mut [Result<Session<'_, '_>, Refusal>],
    mut progress: impl FnMut(&str),
) -> Result<String, Refusal> {
    let mut work = MatrixBudget::default();
    let report = request.evaluate_matrix_with_fit(
        prepared.pattern(),
        cases,
        max_evaluations,
        fit,
        |index, key, pattern| {
            let session = sessions[index].as_mut().map_err(|e| e.clone())?;
            work.evaluate(session, pattern, |line| {
                progress(&format!("{} / {key}: {line}", cases[index].key));
            })
        },
    )?;
    serde_json::to_string(&report).map_err(invalid)
}
