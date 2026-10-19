//! Acquisition-to-selection adapter. Observation permission never implies permission to simulate.
use std::{fs::File, io::Read};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_relaxation::calibration::{Input, calibrate};
use ftx_pattern::textile_import::catalog;
use serde_json::{Value, json};

pub(super) fn run(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    const MAX: u64 = 64 * 1024 * 1024;
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX + 1).read_to_end(&mut bytes))
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
    if bytes.len() as u64 > MAX {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "relaxation history input exceeds 64 MiB",
        ));
    }
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("{path}: {e}")))?;
    let report = calibrate(&input)?;
    let material = catalog()?.resolve(&Selection {
        id: report.material.id.clone(),
        record: Some(report.material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    // This checks complete mechanics as well as the fitted law under the SAME consuming policy.
    let readiness = (|| {
        material.mechanics()?;
        material.membrane_relaxation()?;
        material.bending_response()?;
        material.bending_plasticity()?;
        Ok::<(), Refusal>(())
    })();
    let selection = material.frozen_selection();
    let selection_json = serde_json::to_string(&selection)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    Ok(
        json!({"calibration":report,"selection":selection,"selection_json":selection_json,
        "simulation_ready":readiness.is_ok(),"mechanics_refusal":readiness.err(),
        "empirical_validation_claim":false,"fit_claim":false,
        "time_integration":"Calibration analytically integrates piecewise-linear Green strain; the drape solver uses backward Euler and requires time-step refinement.",
        "uncertainty_note":"Fixed equilibrium and strain history; sensitivity and search bounds are not parameter confidence intervals."}),
    )
}
