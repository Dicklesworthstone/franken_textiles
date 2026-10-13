//! The `wasm-bindgen` boundary (wasm32 only): JSON text in and out, files as bytes. The page's worker calls
//! `program_info()`, `form_fields(args)` and `run(req)` as it called `webapp.py` under Pyodide.

use wasm_bindgen::prelude::*;

use crate::page;

/// `{program: {label, style}}` as JSON text.
#[wasm_bindgen]
#[must_use]
pub fn program_info() -> String {
    page::program_info().dumps(None)
}

/// The tape form's rows as JSON text. `args`: `{program, stature_cm, age_months, nappies}` (program null for
/// someone else's pattern); `today`: `YYYY-MM-DD`.
///
/// # Errors
/// Bad JSON, or a stature or age the charts do not cover.
#[wasm_bindgen]
pub fn form_fields(args: &str, today: &str) -> Result<String, JsError> {
    let a: serde_json::Value =
        serde_json::from_str(args).map_err(|e| JsError::new(&e.to_string()))?;
    let rows = page::form_fields(
        a["program"].as_str(),
        a["stature_cm"].as_f64().unwrap_or(92.0),
        a["age_months"].as_f64(),
        a["nappies"].as_bool().unwrap_or(false),
        today,
    )
    .map_err(|e| JsError::new(&e))?;
    Ok(serde_json::Value::Array(rows).to_string())
}

/// One round's result: the page's JSON, its files and the zip.
#[wasm_bindgen]
pub struct WebRound {
    inner: page::Round,
}

#[wasm_bindgen]
impl WebRound {
    /// What the page shows, as JSON text.
    #[must_use]
    pub fn page(&self) -> String {
        self.inner.page.dumps(None)
    }

    /// The zip's file name.
    #[must_use]
    pub fn zip_name(&self) -> String {
        self.inner.zip_name.clone()
    }

    /// Every file in one zip.
    #[must_use]
    pub fn zip(&self) -> Vec<u8> {
        self.inner.zip.clone()
    }

    /// An export's bytes by file name (`exports/` implied), or a document's (`pattern.json`, …).
    #[must_use]
    pub fn file(&self, name: &str) -> Option<Vec<u8>> {
        let exported = format!("exports/{name}");
        self.inner
            .files
            .iter()
            .find(|(n, _)| *n == exported || n == name)
            .map(|(_, b)| b.clone())
    }
}

/// `webapp.run(req)`: `req` as JSON text, `today` `YYYY-MM-DD`, `created` `YYYY-MM-DDTHH:MM`.
///
/// # Errors
/// The reference's message for a body it refuses or an unknown program; a draft or export refused.
#[wasm_bindgen]
pub fn run(req: &str, today: &str, created: &str) -> Result<WebRound, JsError> {
    page::run(req, today, created)
        .map(|inner| WebRound { inner })
        .map_err(|e| JsError::new(&e))
}
