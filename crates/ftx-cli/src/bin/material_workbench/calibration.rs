//! Native calibration adapters share bounded input and complete mechanical admission.
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_calibration::{Input, calibrate};
use ftx_pattern::material_catalog::{ResolvedMaterial, Selection};
use ftx_pattern::textile_import::catalog;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub(super) fn read_bounded_bytes(
    path: impl AsRef<Path>,
    max_bytes: u64,
    label: &str,
    budget_code: RefusalCode,
    budget_message: &str,
) -> Result<Vec<u8>, Refusal> {
    let path = path.as_ref();
    let io = |error| Refusal::new(RefusalCode::Io, format!("{}: {error}", path.display()));
    let not_regular = || {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("{}: {label} must be a regular file", path.display()),
        )
    };
    let budget = || Refusal::new(budget_code, budget_message);
    let meta = std::fs::metadata(path).map_err(io)?;
    if !meta.is_file() {
        return Err(not_regular());
    }
    if meta.len() > max_bytes {
        return Err(budget());
    }
    let file = File::open(path).map_err(io)?;
    let fd_meta = file.metadata().map_err(io)?;
    if !fd_meta.is_file() {
        return Err(not_regular());
    }
    if fd_meta.len() > max_bytes {
        return Err(budget());
    }
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > max_bytes {
        return Err(budget());
    }
    Ok(bytes)
}

pub(super) fn read<T: DeserializeOwned>(path: &str) -> Result<T, Refusal> {
    const MAX: u64 = 64 * 1024 * 1024;
    let bytes = read_bounded_bytes(
        path,
        MAX,
        "material input",
        RefusalCode::BudgetExceeded,
        "material input exceeds 64 MiB",
    )?;
    serde_json::from_slice(&bytes)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("{path}: {e}")))
}

/// Publish a brand-new output artifact after validation succeeds: creates missing parent
/// directories, refuses existing paths or symlinks via `create_new`, and syncs to disk.
pub(super) fn write_new_file(path: &str, bytes: &[u8]) -> Result<(), Refusal> {
    let io = |error| Refusal::new(RefusalCode::Io, format!("{path}: {error}"));
    let out = Path::new(path);
    let has_trailing_dir_syntax = path.ends_with('/')
        || path.ends_with("/.")
        || (cfg!(windows) && (path.ends_with('\\') || path.ends_with("\\.")));
    if !path.is_empty()
        && !has_trailing_dir_syntax
        && out.file_name().is_some()
        && let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(io)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(io)
}

pub(super) fn simulation_readiness(material: &ResolvedMaterial) -> Result<(), Refusal> {
    material.mechanics()?;
    material.membrane_relaxation()?;
    material.membrane_plasticity()?;
    material.bending_response()?;
    material.bending_plasticity()?;
    Ok(())
}

pub(super) fn linearize_origin(path: &str) -> Result<Value, Refusal> {
    let input: ftx_pattern::material_linearization::Input = read(path)?;
    serde_json::to_value(ftx_pattern::material_linearization::linearize(&input)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

pub(super) fn run(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    let input: Input = read(path)?;
    let report = calibrate(&input)?;
    let snapshot = catalog()?.resolve(&Selection {
        id: report.material.id.clone(),
        record: Some(report.material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    let mechanics = simulation_readiness(&snapshot);
    let selection = snapshot.frozen_selection();
    let selection_json = serde_json::to_string_pretty(&selection)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    Ok(
        json!({"calibration":report,"selection":selection,"selection_json":selection_json,
        "simulation_ready":mechanics.is_ok(),"mechanics_refusal":mechanics.err(),
        "fit_claim":false,"calibration_observations_are_not_independent_garment_validation":true}),
    )
}
