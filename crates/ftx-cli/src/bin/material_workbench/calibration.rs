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

pub(super) fn read<T: DeserializeOwned>(path: &str) -> Result<T, Refusal> {
    const MAX: u64 = 64 * 1024 * 1024;
    let io = |error| Refusal::new(RefusalCode::Io, format!("{path}: {error}"));
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io)?
        .take(MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "material input exceeds 64 MiB",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("{path}: {e}")))
}

/// Publish a brand-new output artifact after validation succeeds: creates missing parent
/// directories, refuses existing paths or symlinks via `create_new`, and syncs to disk.
pub(super) fn write_new_file(path: &str, bytes: &[u8]) -> Result<(), Refusal> {
    let io = |error| Refusal::new(RefusalCode::Io, format!("{path}: {error}"));
    let out = Path::new(path);
    let has_trailing_sep =
        path.ends_with(std::path::MAIN_SEPARATOR) || (cfg!(windows) && path.ends_with('/'));
    if !path.is_empty()
        && !has_trailing_sep
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
