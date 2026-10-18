//! Bounded file boundary for the shared nonlinear swatch identification engine.
use std::{fs::File, io::Read};

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::{
    material_calibration::nonlinear::{Input, calibrate},
    material_catalog::Selection,
    textile_import,
};
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
            "nonlinear swatch input exceeds 64 MiB",
        ));
    }
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("{path}: {e}")))?;
    let report = calibrate(&input)?;
    let snapshot = textile_import::catalog()?.resolve(&Selection {
        id: report.material.id.clone(),
        record: Some(report.material.clone()),
        allow_estimates,
        ..Selection::default()
    })?;
    let mechanics = (|| {
        snapshot.mechanics()?;
        snapshot.membrane_relaxation()?;
        snapshot.bending_response()?;
        snapshot.bending_plasticity()?;
        Ok::<(), Refusal>(())
    })();
    Ok(
        json!({"calibration":report,"selection":snapshot.frozen_selection(),
        "simulation_ready":mechanics.is_ok(),"mechanics_refusal":mechanics.err(),
        "fit_claim":false,"calibration_observations_are_not_independent_garment_validation":true}),
    )
}
