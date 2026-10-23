//! Native calibration adapters share bounded input and complete mechanical admission.
use std::fs::File;
use std::io::Read;

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

pub(super) fn simulation_readiness(material: &ResolvedMaterial) -> Result<(), Refusal> {
    material.mechanics()?;
    material.membrane_relaxation()?;
    material.membrane_plasticity()?;
    material.bending_response()?;
    material.bending_plasticity()?;
    Ok(())
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
