//! Shared bounded file boundary for the two thermal command-line entry points.
use std::{fs::File, io::Read};

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::thermal::steady::request::{Request, SurfaceConditions};
use serde_json::Value;

const MAX_INPUT: usize = 64 * 1024 * 1024;
const MAX_LOCAL_MODEL_INPUT: usize = 16 * 1024 * 1024;

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

fn read_bounded_regular(path: &str, max_bytes: usize, label: &str) -> Result<String, Refusal> {
    let io = |e| Refusal::new(RefusalCode::Io, format!("{path}: {e}"));
    let file = File::open(path).map_err(io)?;
    if !file.metadata().map_err(io)?.is_file() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            format!("{path}: {label} input must be a regular file"),
        ));
    }
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > max_bytes {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("{label} input exceeds {max_bytes} bytes"),
        ));
    }
    String::from_utf8(bytes).map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

fn invalid(e: serde_json::Error) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, e.to_string())
}

pub(crate) fn microclimate(path: &str) -> Result<Value, Refusal> {
    let raw = read_bounded_regular(path, MAX_LOCAL_MODEL_INPUT, "microclimate")?;
    let report = ftx_drape::thermal::microclimate::evaluate_json(&raw)?;
    serde_json::from_str(&report).map_err(invalid)
}

pub(crate) fn lower_visco(path: &str) -> Result<Value, Refusal> {
    let raw = read_bounded_regular(path, MAX_LOCAL_MODEL_INPUT, "fractional-Zener lowering")?;
    let report = ftx_pattern::material_relaxation::exact_exp::evaluate_lowering_json(&raw)?;
    serde_json::from_str(&report).map_err(invalid)
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

pub(crate) fn vapor_coverage(path: &str) -> Result<Value, Refusal> {
    let request: ftx_drape::vapor::coverage::Request =
        serde_json::from_str(&read(path, MAX_INPUT)?).map_err(invalid)?;
    serde_json::to_value(request.evaluate()?).map_err(invalid)
}

pub(crate) fn vapor_coverage_history(path: &str) -> Result<Value, Refusal> {
    let request: ftx_drape::vapor::coverage::history::Request =
        serde_json::from_str(&read(path, MAX_INPUT)?).map_err(invalid)?;
    serde_json::to_value(request.evaluate()?).map_err(invalid)
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
        thermal_bridges: None,
    }
    .evaluate()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_regular_reader_refuses_directories_oversized_files_and_bad_utf8() {
        let dir = std::env::temp_dir().join(format!(
            "ftx-bounded-local-model-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let dir_err =
            read_bounded_regular(dir.to_str().unwrap(), 1024, "microclimate").unwrap_err();
        assert_eq!(dir_err.code, RefusalCode::InvalidDocument);
        assert!(dir_err.message.contains("regular file"));

        let oversized = dir.join("oversized.json");
        std::fs::write(&oversized, b"0123456789abcdef0").unwrap();
        let budget_err =
            read_bounded_regular(oversized.to_str().unwrap(), 16, "microclimate").unwrap_err();
        assert_eq!(budget_err.code, RefusalCode::BudgetExceeded);

        let exact = dir.join("exact.json");
        std::fs::write(&exact, b"0123456789abcdef").unwrap();
        assert_eq!(
            read_bounded_regular(exact.to_str().unwrap(), 16, "microclimate").unwrap(),
            "0123456789abcdef"
        );

        let invalid_utf8 = dir.join("bad.bin");
        std::fs::write(&invalid_utf8, [0xffu8, 0xfe]).unwrap();
        let utf8_err =
            read_bounded_regular(invalid_utf8.to_str().unwrap(), 16, "microclimate").unwrap_err();
        assert_eq!(utf8_err.code, RefusalCode::InvalidDocument);
    }
}
