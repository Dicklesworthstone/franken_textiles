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
pub mod photo;
pub mod stock_pricing;
pub mod thermal;
pub mod tracing;
#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub mod web_tracing;
pub mod zip;

/// Import evaluated GarmentCode JSON without files or network access. Returns the native pattern
/// and review report, including hashes of the exact input texts. Annotations are optional native
/// manufacturing semantics; conversion never certifies garment completeness or physical fit.
///
/// # Errors
/// Unsupported or invalid geometry, annotations, units, or the shared input/work budgets.
pub fn import_garmentcode(
    input: &str,
    annotations: Option<&str>,
) -> Result<String, ftx_core::Refusal> {
    let imported = ftx_pattern::garmentcode_import::import_garmentcode_json(input, annotations)?;
    let text = serde_json::to_string(&imported).map_err(|e| {
        ftx_core::Refusal::new(ftx_core::RefusalCode::InvalidDocument, e.to_string())
    })?;
    if text.len() > 16 * 1024 * 1024 {
        return Err(ftx_core::Refusal::new(
            ftx_core::RefusalCode::BudgetExceeded,
            "GarmentCode import output exceeds 16 MiB",
        ));
    }
    Ok(text)
}

/// Browser binding for the same bounded GarmentCode conversion used by the CLI.
///
/// # Errors
/// The same refusals as [`import_garmentcode`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = import_garmentcode)]
pub fn web_import_garmentcode(
    input: &str,
    annotations: Option<String>,
) -> Result<String, wasm_bindgen::JsError> {
    import_garmentcode(input, annotations.as_deref())
        .map_err(|e| wasm_bindgen::JsError::new(&e.to_string()))
}

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
