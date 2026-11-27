//! Portable anatomical target extraction using the same source-bound body path as the CLI.

use ftx_body::knit_shell::{
    AcceptedAnatomicalShellRequest, MAX_ANATOMICAL_INPUT_BYTES, anatomical_shell_json,
    build_accepted_anatomical_shell, serialize_anatomical_shell,
};
use ftx_body::{AnnyData, AnnyModel};
use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};

/// Extract explicit source anatomy as canonical `EaseShellMesh` JSON.
///
/// # Errors
/// The shared bounded source admission and anatomical geometry refusals.
pub fn knit_shell_json(request_json: &str) -> Result<String, Refusal> {
    anatomical_shell_json(request_json)
}

/// Extract a named region from an accepted, source-matched Anny shape without refitting.
/// Model and report remain their exact bytes; the request contains region/ease/pose choices.
///
/// # Errors
/// Budgets, malformed or changed model/report inputs and the shared anatomical refusals.
pub fn knit_anny_shell_json(
    model_bytes: &[u8],
    accepted_report: &str,
    request_json: &str,
) -> Result<String, Refusal> {
    if model_bytes.len() > 512 * 1024 * 1024
        || accepted_report.len() > MAX_ANATOMICAL_INPUT_BYTES
        || request_json.len() > MAX_ANATOMICAL_INPUT_BYTES
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "anatomical model/request exceeds its input budget",
        ));
    }
    let request: AcceptedAnatomicalShellRequest =
        serde_json::from_str(request_json).map_err(|e| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                format!("anatomical shell request: {e}"),
            )
        })?;
    let model = AnnyModel::from_data(AnnyData::from_bytes(model_bytes, true)?)?;
    let shell = build_accepted_anatomical_shell(
        &model,
        &sha256_hex(model_bytes),
        accepted_report.as_bytes(),
        &request,
    )?;
    serialize_anatomical_shell(&shell)
}

#[cfg(target_arch = "wasm32")]
fn envelope(result: Result<String, Refusal>) -> String {
    match result {
        Ok(result) => format!(r#"{{"ok":true,"result":{result}}}"#),
        Err(refusal) => serde_json::json!({"ok":false,"refusal":refusal}).to_string(),
    }
}

/// Browser entry for explicit anatomical geometry, with structured refusals.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = knit_shell_json)]
pub fn web_knit_shell_json(request_json: &str) -> String {
    envelope(knit_shell_json(request_json))
}

/// Browser entry for an accepted Anny shape and its actual model bytes.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = knit_anny_shell_json)]
pub fn web_knit_anny_shell_json(
    model_bytes: &[u8],
    accepted_report: &str,
    request_json: &str,
) -> String {
    envelope(knit_anny_shell_json(
        model_bytes,
        accepted_report,
        request_json,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_anatomical_inputs_keep_structured_refusals() {
        assert_eq!(
            knit_shell_json("{}").unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        assert_eq!(
            knit_anny_shell_json(&[], "{}", "{}").unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        assert_eq!(
            knit_shell_json(&" ".repeat(MAX_ANATOMICAL_INPUT_BYTES + 1))
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
    }
}
