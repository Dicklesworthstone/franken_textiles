//! Finite clamped-strip acquisition through the shared physical inverse solver.
use std::{fs::File, io::Read};

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::strip::fit::{Input, calibrate};
use ftx_pattern::textile_import::catalog;
use serde_json::{Value, json};

fn read(path: &str) -> Result<Vec<u8>, Refusal> {
    const MAX: u64 = 64 * 1024 * 1024;
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX + 1).read_to_end(&mut bytes))
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
    if bytes.len() as u64 > MAX {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "clamped-strip acquisition exceeds 64 MiB",
        ));
    }
    Ok(bytes)
}

pub(super) fn experiment(path: &str) -> Result<Value, Refusal> {
    let input = serde_json::from_slice(&read(path)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("{path}: {e}")))?;
    serde_json::to_value(ftx_drape::strip::evaluate(&input)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

pub(super) fn run(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    let mut input: Input = serde_json::from_slice(&read(path)?)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("{path}: {e}")))?;
    // Output estimate admission is explicit command policy. Observation evidence has its own
    // separate input permission and is never admitted by this flag.
    input.allow_estimates = allow_estimates;
    let report = calibrate(&input)?;
    if !report.accepted {
        return Ok(
            json!({"calibration":report,"selection":null,"selection_json":null,
            "simulation_ready":false,"fit_claim":false,"empirical_validation_claim":false}),
        );
    }
    let selection = report.selection.as_ref().ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "accepted strip fit lacks its frozen selection",
        )
    })?;
    let snapshot = catalog()?.resolve(selection)?;
    let readiness = (|| {
        snapshot.mechanics()?;
        snapshot.membrane_relaxation()?;
        snapshot.membrane_plasticity()?;
        snapshot.bending_response()?;
        snapshot.bending_plasticity()?;
        Ok::<(), Refusal>(())
    })();
    Ok(
        json!({"calibration":report,"selection":selection,"selection_json":report.selection_json,
        "simulation_ready":readiness.is_ok(),"mechanics_refusal":readiness.err(),
        "fit_claim":false,"empirical_validation_claim":false}),
    )
}
