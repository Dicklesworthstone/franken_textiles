//! Bounded file boundary for the shared recipe selector; never writes or executes a machine file.
use ftx_core::{Refusal, RefusalCode};
use serde_json::Value;
use std::{fs::File, io::Read};

fn read(path: &str, remaining: &mut u64) -> Result<String, Refusal> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(*remaining + 1).read_to_end(&mut bytes))
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
    if bytes.len() as u64 > *remaining {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "combined sewing selection input exceeds 64 MiB",
        ));
    }
    *remaining -= bytes.len() as u64;
    String::from_utf8(bytes).map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

pub(super) fn run(pattern: &str, request: &str) -> Result<(Value, i32, String), Refusal> {
    let mut remaining = 64 * 1024 * 1024;
    let pattern = read(pattern, &mut remaining)?;
    let request = read(request, &mut remaining)?;
    let text = ftx_export::sewing_selection::select_json(&pattern, &request)?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let status = if value["selected_options_json"].is_string() {
        0
    } else {
        2
    };
    Ok((value, status, text))
}
