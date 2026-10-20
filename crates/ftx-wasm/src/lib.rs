//! FrankenTextiles in the browser (plan §15): the page's round (body, measured draft or someone else's
//! pattern, flat checks, measurement gate, every export with its acceptance checks, one zip) in [`page`],
//! testable natively against the reference's `webapp.py`, and the fold-up on a fitted body in [`fold`]
//! (`ftx drape` without files), behind a thin `wasm-bindgen` boundary (`web`, wasm32 only) so the numerical
//! crates never see browser types.
#![forbid(unsafe_code)]

pub mod fold;
pub mod manufacturing;
pub mod materials;
pub mod page;
pub mod stock_pricing;
pub mod thermal;
pub mod tracing;
#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub mod web_tracing;
pub mod zip;

/// Select a complete source-qualified sewing recipe using the same bounded native planner.
/// Returns the report and lossless selected options text; never authorizes machine execution.
///
/// # Errors
/// Invalid search inputs, JSON or the shared input/output budgets.
pub fn sew_select(pattern: &str, request: &str) -> Result<String, ftx_core::Refusal> {
    ftx_export::sewing_selection::select_json(pattern, request)
}

/// Browser export of [`sew_select`].
///
/// # Errors
/// The same refusals as the native, file-free entry point.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = sew_select)]
pub fn web_sew_select(pattern: &str, request: &str) -> Result<String, wasm_bindgen::JsError> {
    sew_select(pattern, request).map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}
