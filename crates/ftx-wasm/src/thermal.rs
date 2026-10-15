//! File-free, clock-free thermal workbench using exactly the native solver and source selections.

use ftx_core::{Refusal, RefusalCode};
use ftx_drape::thermal::steady::request::Request;

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
