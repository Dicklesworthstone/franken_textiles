//! File-free, clock-free thermal workbench using exactly the native solver and source selections.

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::thermal::steady::request::{Request, SurfaceConditions};

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
