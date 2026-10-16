//! Clock-free, file-free manufacturing entry points for the browser, sharing the native planners.

use ftx_core::{Refusal, RefusalCode};
use ftx_export::sewing::{SewOptions, SewPlan, program::SewProgram};
use ftx_pattern::Pattern;

fn budget(inputs: &[&str]) -> Result<(), Refusal> {
    if inputs
        .iter()
        .try_fold(0usize, |n, s| n.checked_add(s.len()))
        .is_none_or(|n| n > 64 * 1024 * 1024)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "combined manufacturing input exceeds 64 MiB",
        ));
    }
    Ok(())
}

fn cut_inputs(pattern: &str, options: &str) -> Result<ftx_export::cutting::CutPlan, Refusal> {
    budget(&[pattern, options])?;
    let pattern = Pattern::new(serde_json::from_str(pattern).map_err(|e| invalid(e.to_string()))?);
    let options = serde_json::from_str(options).map_err(|e| invalid(e.to_string()))?;
    ftx_export::cutting::CutPlan::build(&pattern, &options)
}

fn stock_inputs(pattern: &str, request: &str) -> Result<ftx_export::cutting::stock::Plan, Refusal> {
    budget(&[pattern, request])?;
    let pattern = Pattern::new(serde_json::from_str(pattern).map_err(|e| invalid(e.to_string()))?);
    let request: ftx_export::cutting::stock::Request =
        serde_json::from_str(request).map_err(|e| invalid(e.to_string()))?;
    request.build(&pattern)
}

/// Complete finite hide/remnant allocation from the same source-bearing pattern as draping/sewing.
///
/// # Errors
/// Invalid/oversized inputs, stock/material mismatch, geometry or bounded search refusal.
pub fn stock_plan(pattern: &str, request: &str) -> Result<String, Refusal> {
    serde_json::to_string(&stock_inputs(pattern, request)?).map_err(|e| invalid(e.to_string()))
}

/// Finite-stock SVG in its measured cutting-axis frame; never a machine-coordinate instruction.
///
/// # Errors
/// Same complete-plan refusals as [`stock_plan`], invalid stock index or preview output budget.
pub fn stock_svg(pattern: &str, request: &str, index: usize) -> Result<String, Refusal> {
    ftx_export::cutting::preview::stock_svg(&stock_inputs(pattern, request)?, index)
}

/// Browser export of [`stock_plan`].
///
/// # Errors
/// Same errors as [`stock_plan`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = stock_plan)]
pub fn web_stock_plan(pattern: &str, request: &str) -> Result<String, wasm_bindgen::JsError> {
    stock_plan(pattern, request).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Browser export of [`stock_svg`].
///
/// # Errors
/// Same errors as [`stock_svg`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = stock_svg)]
pub fn web_stock_svg(
    pattern: &str,
    request: &str,
    index: usize,
) -> Result<String, wasm_bindgen::JsError> {
    stock_svg(pattern, request, index).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Material-specific roll requirements from original cut instances; not a device command.
///
/// # Errors
/// Oversized/invalid JSON or the native cutter's geometry, source, width or layout refusals.
pub fn cut_plan(pattern: &str, options: &str) -> Result<String, Refusal> {
    serde_json::to_string(&cut_inputs(pattern, options)?).map_err(|e| invalid(e.to_string()))
}

/// Dated offer comparison for actual marker stock, including waste/minima; never a checkout.
///
/// # Errors
/// Combined inputs over 64 MiB, invalid JSON, refused marker or incompatible quote/market scope.
pub fn cut_price(
    pattern: &str,
    options: &str,
    quotes: &str,
    market: &str,
) -> Result<String, Refusal> {
    budget(&[pattern, options, quotes, market])?;
    let plan = cut_inputs(pattern, options)?;
    let quotes = serde_json::from_str(quotes).map_err(|e| invalid(e.to_string()))?;
    let market = serde_json::from_str(market).map_err(|e| invalid(e.to_string()))?;
    serde_json::to_string(&ftx_export::cutting::procurement::evaluate(
        &plan, &quotes, &market,
    )?)
    .map_err(|e| invalid(e.to_string()))
}

/// Source-locked roll SVG preview in millimetres, not machine coordinates or hardware permission.
///
/// # Errors
/// Same input/planning refusals as [`cut_plan`], or an invalid roll index.
pub fn cut_svg(pattern: &str, options: &str, roll: usize) -> Result<String, Refusal> {
    ftx_export::cutting::preview::svg(&cut_inputs(pattern, options)?, roll)
}

/// Browser export of [`cut_plan`].
///
/// # Errors
/// Same input, source and geometry refusals as [`cut_plan`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = cut_plan)]
pub fn web_cut_plan(pattern: &str, options: &str) -> Result<String, wasm_bindgen::JsError> {
    cut_plan(pattern, options).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Browser export of [`cut_price`].
///
/// # Errors
/// Same marker, quote and market refusals as [`cut_price`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = cut_price)]
pub fn web_cut_price(
    pattern: &str,
    options: &str,
    quotes: &str,
    market: &str,
) -> Result<String, wasm_bindgen::JsError> {
    cut_price(pattern, options, quotes, market)
        .map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

/// Browser export of [`cut_svg`].
///
/// # Errors
/// Same marker/preview refusals as [`cut_svg`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = cut_svg)]
pub fn web_cut_svg(
    pattern: &str,
    options: &str,
    roll: usize,
) -> Result<String, wasm_bindgen::JsError> {
    cut_svg(pattern, options, roll).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

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

/// Millimetre SVG review of one operation in two independent pattern frames, not machine space.
/// The complete job is validated first; the preview neither reads files nor uses external resources.
///
/// # Errors
/// Input/planning refusals, unknown operation or preview geometry/label budgets.
pub fn sew_svg(pattern: &str, options: &str, operation: &str) -> Result<String, Refusal> {
    let (pattern, options) = inputs(pattern, options)?;
    ftx_export::sewing::preview::svg(&pattern, &options, operation)
}
