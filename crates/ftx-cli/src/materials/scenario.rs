//! Bounded file adapter for the fresh-state material scenario shared by both native binaries.
use std::{fs::File, io::Read};

use ftx_body::{AnnyData, AnnyModel};
use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_drape::study::{Scenario, Session, fitted::FittedBody};
use ftx_export::material_study::Request;
use ftx_pattern::Pattern;
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
    let pattern = Pattern::new(read(pattern, &mut remaining)?);
    let request: Request = read(request, &mut remaining)?;
    let scenario: Scenario = read(scenario, &mut remaining)?;
    let measurements: Value = read(measurements, &mut remaining)?;
    scenario.validate()?;
    let bytes = bytes(model, &mut (512 * 1024 * 1024))?;
    let model_sha256 = sha256_hex(&bytes);
    let model = AnnyData::from_bytes(&bytes, false).and_then(AnnyModel::from_data);
    let fitted = model
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|model| FittedBody::fit(model, &measurements));
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
                eprintln!("{key}: {line}");
            })
    })?;
    serde_json::to_value(report).map_err(invalid)
}
