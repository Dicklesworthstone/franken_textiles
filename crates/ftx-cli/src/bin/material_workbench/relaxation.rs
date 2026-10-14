//! Acquisition-to-selection adapter. Observation permission never implies permission to simulate.
use ftx_core::Refusal;
use ftx_pattern::material_catalog::Selection;
use ftx_pattern::material_relaxation::calibration::{Input, calibrate};
use ftx_pattern::textile_import::catalog;
use serde_json::{Value, json};

use super::{read, simulation_readiness};

pub(super) fn run(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    let input: Input = read(path)?;
    let report = calibrate(&input)?;
    let material = catalog()?.resolve(&Selection {
        id: report.material.id.clone(),
        record: Some(report.material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    // This checks complete mechanics as well as the fitted law under the SAME consuming policy.
    let readiness = simulation_readiness(&material);
    Ok(
        json!({"calibration":report,"selection":material.frozen_selection(),
        "simulation_ready":readiness.is_ok(),"mechanics_refusal":readiness.err(),
        "empirical_validation_claim":false,"fit_claim":false,
        "time_integration":"Calibration analytically integrates piecewise-linear Green strain; the drape solver uses backward Euler and requires time-step refinement.",
        "uncertainty_note":"Fixed equilibrium and strain history; sensitivity and search bounds are not parameter confidence intervals."}),
    )
}
