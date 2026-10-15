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
