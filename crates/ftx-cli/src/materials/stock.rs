//! Bounded file adapter shared by both native finite-stock commands. No inventory/device writes.
use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_export::cutting::{
    preview::stock_svg,
    stock::{Plan, Request},
};
use ftx_pattern::Pattern;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
};

fn read<T: DeserializeOwned>(path: &str, left: &mut u64) -> Result<T, Refusal> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(*left + 1).read_to_end(&mut bytes))
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
    if bytes.len() as u64 > *left {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "combined finite-stock inputs exceed 64 MiB",
        ));
    }
    *left -= bytes.len() as u64;
    serde_json::from_slice(&bytes)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("{path}: {e}")))
}
fn build(pattern: &str, request: &str) -> Result<Plan, Refusal> {
    let mut left = 64 * 1024 * 1024;
    let pattern = Pattern::new(read(pattern, &mut left)?);
    let request: Request = read(request, &mut left)?;
    request.build(&pattern)
}
pub(super) fn plan(pattern: &str, request: &str) -> Result<Value, Refusal> {
    serde_json::to_value(build(pattern, request)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}
pub(super) fn residual(pattern: &str, request: &str) -> Result<Value, Refusal> {
    serde_json::to_value(build(pattern, request)?.residual_request()?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}
pub(super) fn preview(
    pattern: &str,
    request: &str,
    index: &str,
    out: &str,
) -> Result<Value, Refusal> {
    let index: usize = index.parse().map_err(|_| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "finite-stock index must be a nonnegative integer",
        )
    })?;
    let plan = build(pattern, request)?;
    let svg = stock_svg(&plan, index)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{out}: {e}")))?;
    file.write_all(svg.as_bytes())
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{out}: {e}")))?;
    Ok(
        json!({"out":out,"stock_index":index,"stock_id":plan.markers()[index].id,
        "svg_sha256":sha256_hex(svg.as_bytes()),"hardware_execution_authorized":false}),
    )
}
