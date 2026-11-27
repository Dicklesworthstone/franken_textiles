//! File boundary shared by both native material commands; evaluators themselves perform no I/O.
use std::io::Write;

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::{Pattern, material_screen::Request, textile_import};
use serde::de::DeserializeOwned;
use serde_json::Value;

fn read<T: DeserializeOwned>(path: &str, remaining: &mut u64) -> Result<T, Refusal> {
    let bytes = super::calibration::read_bounded_bytes(
        path,
        *remaining,
        "material study/screening input",
        RefusalCode::BudgetExceeded,
        "combined material study/screening input exceeds 64 MiB",
    )?;
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

pub(super) fn treatment(path: &str) -> Result<Value, Refusal> {
    let request: ftx_pattern::material_treatment::Input = read(path, &mut (64 * 1024 * 1024))?;
    serde_json::to_value(ftx_pattern::material_treatment::evaluate(&request)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

pub(super) fn treatment_pattern(
    pattern: &str,
    request: &str,
    out: Option<&str>,
) -> Result<Value, Refusal> {
    let mut remaining = 64 * 1024 * 1024;
    let pattern = Pattern::new(read(pattern, &mut remaining)?);
    let request: ftx_pattern::material_treatment::geometry::Input = read(request, &mut remaining)?;
    let mut result = request.apply(&pattern)?;
    if let Some(path) = out {
        let bytes = result["pattern_json"].as_str().ok_or_else(|| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                "treatment omitted its exact pattern",
            )
        })?;
        super::calibration::write_new_file(path, bytes.as_bytes())?;
        result["out"] = Value::String(path.to_owned());
    }
    Ok(result)
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
    super::calibration::write_new_file(out, bytes.as_bytes())?;
    result["out"] = Value::String(out.to_owned());
    Ok(result)
}

fn study(pattern: &str, request: &str) -> Result<Value, Refusal> {
    let mut remaining = 64 * 1024 * 1024;
    let pattern = Pattern::new(read(pattern, &mut remaining)?);
    let request: ftx_export::material_study::Request = read(request, &mut remaining)?;
    serde_json::to_value(request.evaluate(&pattern)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

fn alternative<'a>(report: &'a Value, key: &str) -> Result<&'a Value, Refusal> {
    report["alternatives"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["key"].as_str() == Some(key)))
        .ok_or_else(|| super::invalid(format!("study has no evaluated alternative {key:?}")))
}

fn check_comparison(report: &Value, key: &str) -> Result<(), Refusal> {
    if let Some(comparison) = report.get("comparison")
        && alternative(comparison, key)?["eligible"] != true
    {
        return Err(super::invalid(format!(
            "alternative {key:?} is excluded by the study comparison"
        )));
    }
    Ok(())
}

fn check_destination(path: &str) -> Result<(), Refusal> {
    let out = std::path::Path::new(path);
    if out.file_name().is_none()
        || path.ends_with('/')
        || path.ends_with("/.")
        || (cfg!(windows) && (path.ends_with('\\') || path.ends_with("\\.")))
    {
        return Err(super::invalid("adoption needs a new pattern file path"));
    }
    match std::fs::symlink_metadata(out) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Refusal::new(RefusalCode::Io, format!("{path}: {e}"))),
        Ok(_) => Err(super::invalid(format!(
            "adopted pattern output already exists: {path}"
        ))),
    }
}

fn publish_pattern(
    path: &str,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<(), Refusal> {
    use std::fs::{self, OpenOptions};
    use std::io::ErrorKind;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let out = Path::new(path);
    let io = |e| Refusal::new(RefusalCode::Io, format!("{path}: {e}"));
    let parent = out.parent().unwrap_or(Path::new(""));
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    // link() publishes complete bytes and atomically refuses concurrent or existing outputs.
    // The temporary file is owned by this call only after create_new succeeds.
    let suffix = if out
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
    {
        "stage"
    } else {
        "tmp"
    };
    for _ in 0..64 {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let stage = parent.join(format!(".ftx-pattern-{}-{id}.{suffix}", std::process::id()));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&stage) {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io(e)),
        };
        let ready = write(&mut file).and_then(|()| file.sync_all());
        drop(file);
        let published = ready.and_then(|()| fs::hard_link(&stage, out));
        let _ = fs::remove_file(stage);
        return published.map_err(io);
    }
    Err(super::invalid(
        "could not reserve a staging file for the adopted pattern",
    ))
}

// Accept only a fresh result from one of the evaluators below. The stored JSON string
// preserves floating-point geometry and the exact source-locked material selections.
fn adopt(report: &mut Value, key: &str, path: &str) -> Result<(), Refusal> {
    let manufacturing = report.get("manufacturing").unwrap_or(report);
    let row = alternative(manufacturing, key)?;
    if row["binding"]["status"] != "available" {
        return Err(super::invalid(format!(
            "alternative {key:?} has no available pattern binding"
        )));
    }
    check_comparison(report, key)?;
    if let Some(cases) = report["cases"].as_array() {
        for case in cases {
            check_comparison(case, key)?;
        }
    }
    let binding = &row["binding"]["result"];
    let pattern = binding["pattern_json"]
        .as_str()
        .ok_or_else(|| super::invalid("binding omitted its exact pattern"))?;
    let digest = ftx_core::receipt::sha256_hex(pattern.as_bytes());
    if binding["pattern_sha256"].as_str() != Some(digest.as_str()) {
        return Err(super::invalid(
            "bound pattern does not match its study digest",
        ));
    }
    publish_pattern(path, |file| file.write_all(pattern.as_bytes()))?;
    report["adopted_pattern"] = serde_json::json!({
        "key": key, "out": path, "pattern_sha256": digest
    });
    Ok(())
}

pub(super) fn study_command(
    words: &[&str],
    key: Option<&str>,
    out: Option<&str>,
) -> Result<Value, Refusal> {
    let destination = match (key, out) {
        (None, None) => None,
        (Some(key), Some(path)) if !key.trim().is_empty() && !path.is_empty() => Some((key, path)),
        _ => {
            return Err(super::invalid(
                "use --adopt KEY together with --out NEW-PATTERN.json",
            ));
        }
    };
    if let Some((_, path)) = destination {
        check_destination(path)?;
    }
    let mut report = match words {
        ["study", pattern, request] => study(pattern, request),
        ["study-on-body", pattern, request, body, model] => {
            super::scenario::manufacturing(pattern, request, body, model)
        }
        [
            "scenario-study",
            pattern,
            request,
            scenario,
            measurements,
            model,
        ] => super::scenario::run(pattern, request, scenario, measurements, model),
        _ => {
            return Err(super::invalid(
                "expected study PATTERN REQUEST, study-on-body PATTERN REQUEST BODY MODEL, or scenario-study PATTERN REQUEST SCENARIO MEASUREMENTS MODEL",
            ));
        }
    }?;
    if let Some((key, path)) = destination {
        adopt(&mut report, key, path)?;
    }
    Ok(report)
}

#[cfg(test)]
mod adoption_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_named_case_with_a_comparison_must_admit_the_selected_binding() {
        let binding = json!({"status":"available", "result":{
            "pattern_json":"{}", "pattern_sha256":ftx_core::receipt::sha256_hex(b"{}")
        }});
        let mut report = json!({
            "manufacturing": {"alternatives":[{"key":"cotton","binding":binding}]},
            "cases": [
                {"key":"unranked"},
                {"key":"winter", "comparison":{"alternatives":[{"key":"cotton","eligible":false}]}}
            ]
        });
        let err = adopt(&mut report, "cotton", "").unwrap_err();
        assert!(err.message.contains("excluded by the study comparison"));
        assert!(report.get("adopted_pattern").is_none());
    }

    #[test]
    fn partial_writes_and_failed_publication_preserve_the_destination() {
        let dir = std::env::temp_dir().join(format!(
            "ftx-adopt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("pattern.json");
        let out = path.to_str().unwrap();
        let partial = |file: &mut std::fs::File| {
            file.write_all(b"partial")?;
            Err(std::io::Error::other("injected write failure"))
        };
        assert!(publish_pattern(out, partial).is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::write(&path, b"original").unwrap();
        assert!(publish_pattern(out, |file| file.write_all(b"new")).is_err());
        assert_eq!(std::fs::read(&path).unwrap().as_slice(), b"original");
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1);
    }
}
