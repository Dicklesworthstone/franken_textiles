//! The `wasm-bindgen` boundary (wasm32 only): JSON text in and out, files as bytes. The page's worker calls
//! `program_info()`, `form_fields(args)` and `run(req)` as it called `webapp.py` under Pyodide, then
//! `WebBody(bytes)` and `fold_up(body, …)` for the fold-up on a fitted body.

use wasm_bindgen::prelude::*;

use crate::{fold, page};

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

#[wasm_bindgen]
extern "C" {
    /// The worker's `self.ftxProgress(line)`: the drape's progress lines while it runs.
    #[wasm_bindgen(js_name = ftxProgress)]
    fn ftx_progress(line: &str);
}

/// A converted body model (`.ftxbody`), parsed once and kept for every fold-up.
#[wasm_bindgen]
pub struct WebBody {
    inner: ftx_body::AnnyModel,
}

#[wasm_bindgen]
impl WebBody {
    /// Parses the model file's bytes.
    ///
    /// # Errors
    /// Not a converted model, or a malformed one.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<WebBody, JsError> {
        fold::load_body(bytes)
            .map(|inner| WebBody { inner })
            .map_err(|e| JsError::new(&e))
    }
}

/// A finished fold-up: the fit report and the viewer page.
#[wasm_bindgen]
pub struct WebFold {
    inner: fold::Fold,
}

#[wasm_bindgen]
impl WebFold {
    /// `sim_report.json`'s content as JSON text.
    #[must_use]
    pub fn report(&self) -> String {
        self.inner.report.to_string()
    }

    /// The fold-up viewer page (HTML).
    #[must_use]
    pub fn viewer(&self) -> String {
        self.inner.viewer.clone()
    }
}

/// Drapes the round's pattern on `body` fitted to its measurements (all JSON text, as the round wrote
/// them); `self_contact_mm` > 0 turns on cloth self-contact at that thickness.
///
/// # Errors
/// Bad JSON, a tape the fit cannot use, a pattern the drape refuses.
#[wasm_bindgen]
pub fn fold_up(
    body: &WebBody,
    pattern: &str,
    measurements: &str,
    fit_spec: &str,
    self_contact_mm: f64,
) -> Result<WebFold, JsError> {
    let parse = |s: &str| -> Result<serde_json::Value, JsError> {
        serde_json::from_str(s).map_err(|e| JsError::new(&e.to_string()))
    };
    fold::fold_up(
        &body.inner,
        &parse(pattern)?,
        &parse(measurements)?,
        &parse(fit_spec)?,
        self_contact_mm,
        ftx_progress,
    )
    .map(|inner| WebFold { inner })
    .map_err(|e| JsError::new(&e))
}

/// Local source-locked sewing plan, including explicit multi-pass, reverse-feed and backtack recipes.
///
/// # Errors
/// Invalid inputs, missing material data or a refused machine/geometry/operation constraint.
#[wasm_bindgen]
pub fn sew_plan(pattern: &str, options: &str) -> Result<String, JsError> {
    crate::manufacturing::sew_plan(pattern, options).map_err(|e| JsError::new(&e.message))
}

/// Expand every directed stitch interval in paired source-pattern mm frames, not robot coordinates.
///
/// # Errors
/// Any sewing input/planning refusal; no partial output or hardware execution permission is returned.
#[wasm_bindgen]
pub fn sew_stitches(pattern: &str, options: &str) -> Result<String, JsError> {
    crate::manufacturing::sew_stitches(pattern, options).map_err(|e| JsError::new(&e.message))
}

/// SVG review of one operation in independent pattern frames; no machine registration is implied.
///
/// # Errors
/// Any sewing input/planning refusal, unknown operation or bounded preview failure.
#[wasm_bindgen]
pub fn sew_svg(pattern: &str, options: &str, operation: &str) -> Result<String, JsError> {
    crate::manufacturing::sew_svg(pattern, options, operation).map_err(|e| JsError::new(&e.message))
}

/// Source-qualified steady textile conduction, with the native workbench's exact request schema.
/// No files, network access, missing heat storage or comfort rating are introduced.
///
/// # Errors
/// Oversized/invalid JSON, incomplete transport, denied evidence or a failed bounded thermal solve.
#[wasm_bindgen]
pub fn thermal_steady(request: &str) -> Result<String, JsError> {
    crate::thermal::steady(request).map_err(|e| JsError::new(&e.message))
}

/// Steady heat fields on retained drape geometry, without inventing absent heat storage.
///
/// # Errors
/// Invalid, oversized, changed or unsupported inputs, or a bounded steady solver refusal.
#[wasm_bindgen]
pub fn thermal_steady_surface(report: &str, conditions: &str) -> Result<String, JsError> {
    crate::thermal::steady_surface(report, conditions).map_err(|e| JsError::new(&e.message))
}
