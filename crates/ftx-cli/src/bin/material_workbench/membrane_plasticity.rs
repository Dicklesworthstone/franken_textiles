//! Cyclic acquisition-to-selection adapter; fitting and simulation use separate estimate policies.
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_membrane_plasticity::calibration::{Input, calibrate};
use ftx_pattern::textile_import::catalog;
use serde_json::{Value, json};

pub(super) fn run(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    const MAX: u64 = 64 * 1024 * 1024;
    let bytes = super::calibration::read_bounded_bytes(
        path,
        MAX,
        "permanent-stretch history input",
        RefusalCode::BudgetExceeded,
        "permanent-stretch history input exceeds 64 MiB",
    )?;
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("{path}: {e}")))?;
    let report = calibrate(&input)?;
    let Some(material) = &report.material else {
        return Ok(
            json!({"calibration":report,"selection":null,"selection_json":null,
            "simulation_ready":false,"empirical_validation_claim":false,"fit_claim":false}),
        );
    };
    let resolved = catalog()?.resolve(&Selection {
        id: material.id.clone(),
        record: Some(material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    let readiness = super::calibration::simulation_readiness(&resolved);
    let selection = resolved.frozen_selection();
    let selection_json = serde_json::to_string(&selection)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    Ok(
        json!({"calibration":report,"selection":selection,"selection_json":selection_json,
        "simulation_ready":readiness.is_ok(),"mechanics_refusal":readiness.err(),
        "empirical_validation_claim":false,"fit_claim":false,
        "uncertainty_note":"The declared search ranges and local sensitivity are not parameter confidence intervals. Loading history and applicability domains remain supplied evidence."}),
    )
}
