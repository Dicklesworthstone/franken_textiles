//! Clock-free, file-free sewing entry points for the browser. Both use the same source-bearing
//! material resolver, pattern seam geometry and manufacturing planner as the native CLI.

use ftx_core::{Refusal, RefusalCode};
use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
use ftx_pattern::Pattern;

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn inputs(pattern: &str, options: &str) -> Result<(Pattern, SewOptions), Refusal> {
    if pattern.len() > 64 * 1024 * 1024 || options.len() > 64 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "material input exceeds 64 MiB",
        ));
    }
    Ok((
        Pattern::new(serde_json::from_str(pattern).map_err(|e| invalid(e.to_string()))?),
        serde_json::from_str(options).map_err(|e| invalid(e.to_string()))?,
    ))
}

/// Source-locked seam/operation plan as JSON; input JSON is retained, not edited or uploaded.
///
/// # Errors
/// Invalid/oversized JSON, missing source properties or any material/geometry/machine planning refusal.
pub fn sew_plan(pattern: &str, options: &str) -> Result<String, Refusal> {
    let (pattern, options) = inputs(pattern, options)?;
    serde_json::to_string(&SewPlan::build(&pattern, &options)?).map_err(|e| invalid(e.to_string()))
}

/// Every directed stitch interval, including repeated lock travel, in original paired local mm frames.
/// Run/operation boundaries are explicit; neither repositioning nor device execution is inferred.
///
/// # Errors
/// The same input/planning refusals as [`sew_plan`], plus bounded stitch expansion failures.
pub fn sew_stitches(pattern: &str, options: &str) -> Result<String, Refusal> {
    let (pattern, options) = inputs(pattern, options)?;
    serde_json::to_string(&SewProgram::build(&pattern, &options)?)
        .map_err(|e| invalid(e.to_string()))
}
