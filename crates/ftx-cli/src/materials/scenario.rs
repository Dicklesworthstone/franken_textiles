//! Bounded file adapter for the fresh-state material scenario shared by both native binaries.
use std::{fs::File, io::Read};

use ftx_body::{AnnyData, AnnyModel};
use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_drape::study::{
    Scenario, Session,
    fitted::{BodyRequest, FittedBody},
    matrix,
};
use ftx_drape::study_input::PreparedInput;
use ftx_export::material_study::Request;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn invalid(e: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, e.to_string())
}
fn bytes(path: &str, remaining: &mut u64) -> Result<Vec<u8>, Refusal> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(*remaining + 1).read_to_end(&mut bytes))
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
    if bytes.len() as u64 > *remaining {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "scenario file budget exceeded",
        ));
    }
    *remaining -= bytes.len() as u64;
    Ok(bytes)
}
fn read<T: DeserializeOwned>(path: &str, remaining: &mut u64) -> Result<T, Refusal> {
    serde_json::from_slice(&bytes(path, remaining)?).map_err(invalid)
}

pub(super) fn run(
    pattern: &str,
    request: &str,
    scenario: &str,
    measurements: &str,
    model: &str,
) -> Result<Value, Refusal> {
    let mut remaining = 64 * 1024 * 1024;
    let source: Value = read(pattern, &mut remaining)?;
    let request: Request = read(request, &mut remaining)?;
    let scenario_input: Value = read(scenario, &mut remaining)?;
    if scenario_input["schema"] == "ftx-drape-scenario-matrix/1" {
        let matrix: matrix::Request = serde_json::from_value(scenario_input).map_err(invalid)?;
        matrix.validate()?;
        let policies = matrix
            .cases
            .iter()
            .map(|case| {
                let policy: Option<ftx_export::material_study::comparison::Policy> = case
                    .comparison
                    .as_ref()
                    .map(|value| serde_json::from_value(value.clone()).map_err(invalid))
                    .transpose()?;
                if let Some(effective) = policy.as_ref().or(request.comparison.as_ref()) {
                    effective.validate(&request, true)?;
                }
                Ok(policy)
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        let prepared = matrix.prepare(source)?;
        let body_input: Value = read(measurements, &mut remaining)?;
        let bytes = bytes(model, &mut (512 * 1024 * 1024))?;
        return run_matrix(request, matrix, policies, prepared, body_input, bytes);
    }
    if let Some(policy) = &request.comparison {
        policy.validate(&request, true)?;
    }
    let scenario: Scenario = serde_json::from_value(scenario_input).map_err(invalid)?;
    scenario.validate()?;
    let prepared = PreparedInput::new(source, &scenario.options)?;
    let body_input: Value = read(measurements, &mut remaining)?;
    let bytes = bytes(model, &mut (512 * 1024 * 1024))?;
    let model_sha256 = sha256_hex(&bytes);
    let model = AnnyData::from_bytes(&bytes, false).and_then(AnnyModel::from_data);
    let body_request = BodyRequest::read(&body_input);
    let fitted = body_request
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|request| {
            model
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|model| FittedBody::prepare(model, &model_sha256, request))
        });
    let body = fitted.as_ref().map(FittedBody::body).map_err(Clone::clone);
    let mut session = body
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|body| Session::new(&scenario, body));
    if let (Ok(session), Ok(fitted), Ok(model), Some(motion)) =
        (&mut session, &fitted, &model, &scenario.motion)
    {
        session.set_motion_path(fitted.motion_path(model, &model_sha256, motion))?;
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
                eprintln!("{key}: {line}");
            })
    })?;
    serde_json::to_value(report).map_err(invalid)
}

fn run_matrix(
    request: Request,
    matrix: matrix::Request,
    policies: Vec<Option<ftx_export::material_study::comparison::Policy>>,
    prepared: PreparedInput,
    body_input: Value,
    bytes: Vec<u8>,
) -> Result<Value, Refusal> {
    let model_sha256 = sha256_hex(&bytes);
    let model = AnnyData::from_bytes(&bytes, false).and_then(AnnyModel::from_data);
    let body_request = BodyRequest::read(&body_input);
    let fitted = body_request
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|request| {
            model
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|model| FittedBody::prepare(model, &model_sha256, request))
        });
    let body = fitted.as_ref().map(FittedBody::body).map_err(Clone::clone);
    let mut sessions = matrix
        .cases
        .iter()
        .map(|case| {
            body.as_ref()
                .map_err(Clone::clone)
                .and_then(|body| Session::new(&case.scenario, body))
        })
        .collect::<Vec<_>>();
    let mut cases = Vec::with_capacity(matrix.cases.len());
    for ((case, session), comparison) in matrix.cases.iter().zip(&mut sessions).zip(policies) {
        if let (Ok(session), Ok(fitted), Ok(model), Some(motion)) =
            (&mut *session, &fitted, &model, &case.scenario.motion)
        {
            session.set_motion_path(fitted.motion_path(model, &model_sha256, motion))?;
        }
        let mut descriptor = match session {
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
        prepared.annotate(&mut descriptor)?;
        cases.push(ftx_export::material_study::matrix::Case {
            key: case.key.clone(),
            scenario: descriptor,
            comparison,
        });
    }
    let mut work = matrix::Budget::default();
    let report = request.evaluate_matrix_with(
        prepared.pattern(),
        &cases,
        matrix.max_evaluations,
        |index, key, pattern| {
            work.evaluate(
                sessions[index].as_mut().map_err(|e| e.clone())?,
                pattern,
                |line| eprintln!("{} / {key}: {line}", cases[index].key),
            )
        },
    )?;
    serde_json::to_value(report).map_err(invalid)
}
