//! Freeze a source outfit for the same material-binding, drape and study consumers as one pattern.

use ftx_core::{Refusal, receipt::sha256_hex};
use ftx_drape::{outfit, run::DrapeOptions};
use serde_json::json;

use super::{budget, invalid};

/// Preserve the exact request and compose qualified garments without changing placement or topology.
/// The resulting ordinary pattern can enter the existing material workbench. Its initial receipt
/// identifies the import, not later substituted materials or an empirical garment-fit result.
///
/// # Errors
/// Invalid/oversized requests or any shared outfit material, attachment or topology refusal.
pub fn prepare(input: &str) -> Result<String, Refusal> {
    budget(&[input])?;
    let request: outfit::Input = serde_json::from_str(input).map_err(invalid)?;
    let defaults = DrapeOptions::default();
    let options = DrapeOptions {
        self_contact: Some(defaults.contact),
        ..defaults
    };
    let prepared = outfit::prepare(&request, &options)?;
    let pattern_json = serde_json::to_string(prepared.pattern().raw()).map_err(invalid)?;
    let composition_json = serde_json::to_string(prepared.receipt()).map_err(invalid)?;
    serde_json::to_string(&json!({"schema":"ftx-outfit-workbench/1",
        "request_json":input,"request_sha256":sha256_hex(input.as_bytes()),
        "pattern_json":pattern_json,"composition":prepared.receipt(),
        "composition_json":composition_json,"self_contact_required":true,
        "scope":"Source composition before material substitutions. Placements and seam topology remain authored; material binding and every subsequent consumer revalidate their own inputs. No fit or dressing claim."}))
        .map_err(invalid)
}

/// Import an outfit through the production composer, returning exact source and pattern JSON.
///
/// # Errors
/// Same bounded source, topology, material and contact failures as [`prepare`].
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn material_outfit_prepare(input: &str) -> Result<String, wasm_bindgen::JsError> {
    prepare(input).map_err(|e| wasm_bindgen::JsError::new(&e.message))
}
