//! Bounded file boundary for the shared strip experiment, inverse identification and acquisition APIs.
use ftx_core::{Refusal, RefusalCode};
use ftx_drape::{cylinder, strip};
use ftx_pattern::textile_import::catalog;
use serde_json::{Value, json};

fn invalid(error: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, error.to_string())
}

fn read(path: &str) -> Result<String, Refusal> {
    const MAX: u64 = 64 * 1024 * 1024;
    let bytes = super::calibration::read_bounded_bytes(
        path,
        MAX,
        "strip input",
        RefusalCode::BudgetExceeded,
        "strip input exceeds 64 MiB",
    )?;
    String::from_utf8(bytes).map_err(|e| invalid(format!("{path}: {e}")))
}

pub(super) fn experiment(path: &str) -> Result<Value, Refusal> {
    let input = serde_json::from_str(&read(path)?).map_err(invalid)?;
    serde_json::to_value(strip::evaluate(&input)?).map_err(invalid)
}

pub(super) fn history(path: &str) -> Result<Value, Refusal> {
    let input = serde_json::from_str(&read(path)?).map_err(invalid)?;
    serde_json::to_value(strip::history::evaluate(&input)?).map_err(invalid)
}

pub(super) fn cylinder(path: &str) -> Result<Value, Refusal> {
    let input = serde_json::from_str(&read(path)?).map_err(invalid)?;
    serde_json::to_value(cylinder::evaluate(&input)?).map_err(invalid)
}

pub(super) fn run(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    let mut input: strip::fit::Input = serde_json::from_str(&read(path)?).map_err(invalid)?;
    // Output estimate admission is explicit command policy. Observation evidence has its own
    // separate input permission and is never admitted by this flag.
    input.allow_estimates = allow_estimates;
    let report = strip::fit::calibrate(&input)?;
    if !report.accepted {
        return Ok(
            json!({"calibration":report,"selection":null,"selection_json":null,
            "simulation_ready":false,"fit_claim":false,"empirical_validation_claim":false}),
        );
    }
    let selection = report
        .selection
        .as_ref()
        .ok_or_else(|| invalid("accepted strip fit lacks its frozen selection"))?;
    let snapshot = catalog()?.resolve(selection)?;
    let readiness = super::calibration::simulation_readiness(&snapshot);
    Ok(
        json!({"calibration":report,"selection":selection,"selection_json":report.selection_json,
        "simulation_ready":readiness.is_ok(),"mechanics_refusal":readiness.err(),
        "fit_claim":false,"empirical_validation_claim":false}),
    )
}

pub(super) fn calibrate_cylinder(path: &str, allow_estimates: bool) -> Result<Value, Refusal> {
    let mut input: cylinder::fit::Input = serde_json::from_str(&read(path)?).map_err(invalid)?;
    // Output estimate admission is explicit command policy; observation evidence consent remains
    // separately owned by input.allow_estimated_observations.
    input.allow_estimates = allow_estimates;
    let report = cylinder::fit::calibrate(&input)?;
    if !report.accepted {
        return Ok(
            json!({"calibration":report,"selection":null,"selection_json":null,
            "simulation_ready":false,"fit_claim":false,"empirical_validation_claim":false}),
        );
    }
    let selection = report
        .selection
        .as_ref()
        .ok_or_else(|| invalid("accepted cylinder fit lacks its frozen selection"))?;
    let snapshot = catalog()?.resolve(selection)?;
    let readiness = super::calibration::simulation_readiness(&snapshot);
    Ok(
        json!({"calibration":report,"selection":selection,"selection_json":report.selection_json,
        "simulation_ready":readiness.is_ok(),"mechanics_refusal":readiness.err(),
        "fit_claim":false,"empirical_validation_claim":false}),
    )
}

pub(super) fn acquisition(path: &str) -> Result<Value, Refusal> {
    // Exact source text reaches the parser so its acquisition identity is not a JSON round trip.
    let input = read(path)?;
    serde_json::to_value(ftx_pattern::material_catalog::knit_strips::parse_acquisition(&input)?)
        .map_err(invalid)
}
