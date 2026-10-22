//! Shared bounded file boundary for the two thermal command-line entry points.
use std::{fs::File, io::Read};

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::thermal::steady::request::{Request, SurfaceConditions};
use serde_json::Value;

const MAX_INPUT: usize = 64 * 1024 * 1024;

fn read(path: &str, remaining: usize) -> Result<String, Refusal> {
    let io = |e| Refusal::new(RefusalCode::Io, format!("{path}: {e}"));
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io)?
        .take(remaining as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > remaining {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "combined thermal input exceeds 64 MiB",
        ));
    }
    String::from_utf8(bytes).map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

fn invalid(e: serde_json::Error) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, e.to_string())
}

pub(crate) fn compression_history(path: &str) -> Result<Value, Refusal> {
    let report = ftx_drape::thermal::compression::evaluate_json(&read(path, MAX_INPUT)?)?;
    serde_json::from_str(&report).map_err(invalid)
}

pub(crate) fn vapor(path: &str) -> Result<Value, Refusal> {
    let request: ftx_pattern::material_vapor::Request =
        serde_json::from_str(&read(path, MAX_INPUT)?).map_err(invalid)?;
    serde_json::to_value(request.evaluate(&ftx_pattern::textile_import::catalog()?)?)
        .map_err(invalid)
}

pub(crate) fn vapor_surface(report: &str, conditions: &str) -> Result<Value, Refusal> {
    let report = read(report, MAX_INPUT)?;
    let conditions: ftx_drape::thermal::vapor::Conditions =
        serde_json::from_str(&read(conditions, MAX_INPUT - report.len())?).map_err(invalid)?;
    conditions.evaluate(&serde_json::from_str(&report).map_err(invalid)?)
}

pub(crate) fn stack(path: &str) -> Result<Value, Refusal> {
    let request: Request = serde_json::from_str(&read(path, MAX_INPUT)?).map_err(invalid)?;
    request.evaluate(&ftx_pattern::textile_import::catalog()?)
}

pub(crate) fn surface(report: &str, conditions: &str) -> Result<Value, Refusal> {
    let report = read(report, MAX_INPUT)?;
    let conditions: SurfaceConditions =
        serde_json::from_str(&read(conditions, MAX_INPUT - report.len())?).map_err(invalid)?;
    conditions.evaluate(&serde_json::from_str(&report).map_err(invalid)?)
}

pub(crate) fn coverage_request(path: &str) -> Result<Value, Refusal> {
    let request: ftx_drape::thermal::steady::coverage::request::Request =
        serde_json::from_str(&read(path, MAX_INPUT)?).map_err(invalid)?;
    request.evaluate()
}

pub(crate) fn coverage(report: &str, map: &str, conditions: &str) -> Result<Value, Refusal> {
    let report = read(report, MAX_INPUT)?;
    let map = read(map, MAX_INPUT - report.len())?;
    let conditions = read(conditions, MAX_INPUT - report.len() - map.len())?;
    ftx_drape::thermal::steady::coverage::request::Request {
        schema: "ftx-steady-coverage-request/1".into(),
        report: serde_json::from_str(&report).map_err(invalid)?,
        map: serde_json::from_str(&map).map_err(invalid)?,
        conditions: serde_json::from_str(&conditions).map_err(invalid)?,
    }
    .evaluate()
}
