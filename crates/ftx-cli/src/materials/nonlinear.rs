//! Bounded file boundary for the shared nonlinear swatch identification engine.
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::{
    material_calibration::nonlinear::{Input, calibrate},
    material_catalog::Selection,
    textile_import,
};
use serde_json::{Value, json};

pub(super) fn run(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    const MAX: u64 = 64 * 1024 * 1024;
    let bytes = super::calibration::read_bounded_bytes(
        path,
        MAX,
        "nonlinear swatch input",
        RefusalCode::BudgetExceeded,
        "nonlinear swatch input exceeds 64 MiB",
    )?;
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("{path}: {e}")))?;
    let report = calibrate(&input)?;
    let snapshot = textile_import::catalog()?.resolve(&Selection {
        id: report.material.id.clone(),
        record: Some(report.material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    let mechanics = super::calibration::simulation_readiness(&snapshot);
    let selection = snapshot.frozen_selection();
    let selection_json = serde_json::to_string(&selection)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    Ok(
        json!({"calibration":report,"selection":selection,"selection_json":selection_json,
        "simulation_ready":mechanics.is_ok(),"mechanics_refusal":mechanics.err(),
        "fit_claim":false,"calibration_observations_are_not_independent_garment_validation":true}),
    )
}
