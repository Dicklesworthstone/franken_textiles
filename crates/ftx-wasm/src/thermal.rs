//! File-free, clock-free thermal workbench using exactly the native solver and source selections.

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::thermal::steady::request::{Request, SurfaceConditions};

/// Evaluate qualified clearance changes followed by dry thermal holds, retaining material heat
/// state, mass and source-locked final selections. Mechanical work is not injected as heat.
/// This is the same local operation as `ftx materials thermal-compression`.
/// Accepts original single-branch version 1 and source-qualified cyclic version 2 histories.
///
/// # Errors
/// Input over 64 MiB, invalid evidence/schema, exceeded work limits or any refused hold. No partial
/// history or replacement selection is returned, and the caller's input is never edited.
pub fn compression_history(input: &str) -> Result<String, Refusal> {
    ftx_drape::thermal::compression::evaluate_json(input)
}

/// Browser binding for the same transactional compression/heat history as [`compression_history`].
///
/// # Errors
/// The same source, model, budget and arithmetic refusals as [`compression_history`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn thermal_compression_history(input: &str) -> Result<String, wasm_bindgen::JsError> {
    compression_history(input).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Evaluate `ftx-thermal-steady/1` JSON and return the shared source-locked result as JSON text.
/// Input is neither uploaded nor edited; missing heat capacity is not replaced with a default.
///
/// # Errors
/// Input over 64 MiB, invalid JSON/schema, missing evidence/transport or any bounded solver refusal.
pub fn steady(input: &str) -> Result<String, Refusal> {
    if input.len() > 64 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "material input exceeds 64 MiB",
        ));
    }
    let invalid =
        |error: serde_json::Error| Refusal::new(RefusalCode::InvalidDocument, error.to_string());
    let request: Request = serde_json::from_str(input).map_err(invalid)?;
    let result = request.evaluate(&ftx_pattern::textile_import::catalog()?)?;
    serde_json::to_string(&result).map_err(invalid)
}

/// Steady heat fields on source-locked drape triangles, independent of transient storage readiness.
/// Neither input is uploaded or modified. Boundaries follow the retained material column identity.
///
/// # Errors
/// Combined input over 64 MiB, invalid conditions, changed sources/geometry, unavailable transport
/// or any bounded steady solver refusal. No fallback to generic material properties occurs.
pub fn steady_surface(report: &str, conditions: &str) -> Result<String, Refusal> {
    if report
        .len()
        .checked_add(conditions.len())
        .is_none_or(|n| n > 64 * 1024 * 1024)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "combined thermal input exceeds 64 MiB",
        ));
    }
    let invalid = |e: serde_json::Error| Refusal::new(RefusalCode::InvalidDocument, e.to_string());
    let conditions: SurfaceConditions = serde_json::from_str(conditions).map_err(invalid)?;
    let report = serde_json::from_str(report).map_err(invalid)?;
    serde_json::to_string(&conditions.evaluate(&report)?).map_err(invalid)
}

/// Recompute material-following body coverage and exact steady heat from a portable request.
/// This calls the same implementation as `ftx-materials thermal-steady-coverage` without files.
///
/// # Errors
/// Input over 64 MiB, invalid JSON, source/geometry refusals, or unsupported transport conditions.
pub fn steady_coverage(input: &str) -> Result<String, Refusal> {
    if input.len() > 64 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "coverage input exceeds 64 MiB",
        ));
    }
    let invalid = |e: serde_json::Error| Refusal::new(RefusalCode::InvalidDocument, e.to_string());
    let request: ftx_drape::thermal::steady::coverage::request::Request =
        serde_json::from_str(input).map_err(invalid)?;
    serde_json::to_string(&request.evaluate()?).map_err(invalid)
}

/// Browser binding for source-locked, storage-independent garment/body coverage.
///
/// # Errors
/// The same input, geometric and physical capability refusals as [`steady_coverage`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn thermal_steady_coverage(input: &str) -> Result<String, wasm_bindgen::JsError> {
    steady_coverage(input).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}
