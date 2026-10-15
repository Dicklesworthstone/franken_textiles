//! File boundary shared by both native material commands; evaluators themselves perform no I/O.
use std::{fs::File, io::Read};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::{Pattern, material_screen::Request, textile_import};
use serde::de::DeserializeOwned;
use serde_json::Value;

fn read<T: DeserializeOwned>(path: &str, remaining: &mut u64) -> Result<T, Refusal> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(*remaining + 1).read_to_end(&mut bytes))
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
    if bytes.len() as u64 > *remaining {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "combined material study/screening input exceeds 64 MiB",
        ));
    }
    *remaining -= bytes.len() as u64;
    serde_json::from_slice(&bytes)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("{path}: {e}")))
}

pub(super) fn run(path: &str) -> Result<Value, Refusal> {
    let request: Request = read(path, &mut (64 * 1024 * 1024))?;
    serde_json::to_value(request.evaluate(&textile_import::catalog()?)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

pub(super) fn study(pattern: &str, request: &str) -> Result<Value, Refusal> {
    let mut remaining = 64 * 1024 * 1024;
    let pattern = Pattern::new(read(pattern, &mut remaining)?);
    let request: ftx_export::material_study::Request = read(request, &mut remaining)?;
    serde_json::to_value(request.evaluate(&pattern)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}
