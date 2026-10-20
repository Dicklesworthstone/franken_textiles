//! File boundary shared by both native material commands; evaluators themselves perform no I/O.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
};

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

pub(super) fn pressure_condition(path: &str) -> Result<Value, Refusal> {
    let request: ftx_pattern::material_compression::Input = read(path, &mut (64 * 1024 * 1024))?;
    serde_json::to_value(ftx_pattern::material_compression::condition(&request)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

pub(super) fn compression_stack(path: &str) -> Result<Value, Refusal> {
    let request: ftx_pattern::material_compression::stack::Input =
        read(path, &mut (64 * 1024 * 1024))?;
    serde_json::to_value(ftx_pattern::material_compression::stack::close(&request)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

pub(super) fn bind_changes(pattern: &str, changes: &str, out: &str) -> Result<Value, Refusal> {
    let mut remaining = 64 * 1024 * 1024;
    let pattern = Pattern::new(read(pattern, &mut remaining)?);
    let changes: ftx_pattern::material_binding::Changes = read(changes, &mut remaining)?;
    let mut result = changes.apply(&pattern)?;
    let bound = Pattern::new(result["pattern"].clone());
    let mut options = ftx_drape::run::DrapeOptions {
        self_contact: None,
        ..ftx_drape::run::DrapeOptions::default()
    };
    let readiness = |result: Result<Value, Refusal>| match result {
        Ok(value) => serde_json::json!({"ready":true,"value":value}),
        Err(refusal) => serde_json::json!({"ready":false,"refusal":refusal}),
    };
    result["drape_materials_without_self_contact"] =
        readiness(ftx_drape::material::resolve(&bound, &options).map(|m| m.report));
    options.self_contact = Some(options.contact);
    result["drape_materials_with_self_contact"] =
        readiness(ftx_drape::material::resolve(&bound, &options).map(|m| m.report));
    // Validate first, then write a new exact pattern. A refused mechanical consumer still leaves
    // the source-qualified binding usable for independent cutting/thermal/manufacturing consumers.
    let bytes = result["pattern_json"].as_str().ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "binding omitted its exact pattern",
        )
    })?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .and_then(|mut file| file.write_all(bytes.as_bytes()))
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{out}: {e}")))?;
    result["out"] = Value::String(out.to_owned());
    Ok(result)
}

pub(super) fn study(pattern: &str, request: &str) -> Result<Value, Refusal> {
    let mut remaining = 64 * 1024 * 1024;
    let pattern = Pattern::new(read(pattern, &mut remaining)?);
    let request: ftx_export::material_study::Request = read(request, &mut remaining)?;
    serde_json::to_value(request.evaluate(&pattern)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}
