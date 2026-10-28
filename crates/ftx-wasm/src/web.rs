//! The `wasm-bindgen` boundary (wasm32 only): JSON text in and out, files as bytes. The page's worker calls
//! `program_info()`, `form_fields(args)` and `run(req)` as it called `webapp.py` under Pyodide, then
//! `WebBody(bytes)` and `fold_up(body, …)` for the fold-up on a fitted body.

use wasm_bindgen::prelude::*;

use crate::{fold, page};

/// Exact predefined dictionary names accepted by the shared printable-reference renderer.
#[wasm_bindgen]
#[must_use]
pub fn calibration_dictionaries() -> String {
    serde_json::to_string(ftx_trace::calibration_sheet::dictionaries())
        .expect("dictionary names JSON")
}

/// Matching PNG, lossless PDF and sidecar for one locally rendered print reference.
#[wasm_bindgen]
pub struct WebCalibrationSheet {
    inner: ftx_trace::calibration_sheet::Sheet,
}

#[wasm_bindgen]
impl WebCalibrationSheet {
    /// Render the native metric reference without writing files.
    ///
    /// # Errors
    /// Unsupported names, invalid dimensions or layouts that would clip marks or instructions.
    #[wasm_bindgen(constructor)]
    pub fn new(
        paper: &str,
        square_mm: f64,
        marker_mm: f64,
        dictionary: &str,
    ) -> Result<Self, JsError> {
        ftx_trace::calibration_sheet::render(paper, square_mm, marker_mm, dictionary)
            .map(|inner| Self { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Lossless grayscale PNG with explicit 300-DPI density.
    #[must_use]
    pub fn png(&self) -> Vec<u8> {
        self.inner.png.clone()
    }

    /// Lossless one-bit PDF; check the physical print with a ruler.
    #[must_use]
    pub fn pdf(&self) -> Vec<u8> {
        self.inner.pdf.clone()
    }

    /// Native-identical path-free sidecar text, including its final newline.
    #[must_use]
    pub fn sidecar(&self) -> String {
        String::from_utf8(self.inner.sidecar_json()).expect("calibration JSON is UTF-8")
    }
}

/// Fit local coordinate evidence with the canonical camera and conditional uncertainty engines.
///
/// # Errors
/// The shared input, evidence, domain or budget guards refuse.
#[wasm_bindgen]
pub fn camera_calibration(input: &str, corner_noise: &str) -> Result<String, JsError> {
    crate::photo::camera_calibration(input, corner_noise)
        .map(|value| value.to_string())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Incremental local camera acquisition. This handle retains no decoded image pixels.
#[wasm_bindgen]
pub struct WebCameraCalibration {
    inner: crate::photo::CameraCalibration,
}

#[wasm_bindgen]
impl WebCameraCalibration {
    /// Freeze a native image manifest, original sheet and optional declared corner-noise request.
    ///
    /// # Errors
    /// The portable acquisition's source, schema, evidence or budget checks refuse.
    #[wasm_bindgen(constructor)]
    pub fn new(
        manifest: &str,
        sheet: &str,
        corner_noise: &str,
    ) -> Result<WebCameraCalibration, JsError> {
        crate::photo::CameraCalibration::new(manifest, sheet, corner_noise)
            .map(|inner| Self { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// JSON array of exact file labels in canonical frame order.
    #[must_use]
    pub fn image_ids(&self) -> String {
        serde_json::json!(self.inner.image_ids()).to_string()
    }

    /// Admit one original image; release pixels before returning its source/partition diagnostics.
    ///
    /// # Errors
    /// Image decoding, identity, geometry, duplicate, detection or observation admission refuses.
    pub fn add_image(&mut self, image: &str, bytes: &[u8]) -> Result<String, JsError> {
        self.inner
            .add_image(image, bytes)
            .map(|value| value.to_string())
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Ordinary retained camera-calibration JSON, including rejected diagnostics.
    ///
    /// # Errors
    /// The batch is incomplete or canonical calibration/noise input guards refuse.
    pub fn finish(&self) -> Result<String, JsError> {
        self.inner
            .finish()
            .map(|value| value.to_string())
            .map_err(|e| JsError::new(&e.to_string()))
    }
}

/// Local photo evidence; the same strict pixels and unrounded calibration as the native commands.
#[wasm_bindgen]
pub struct WebPhoto {
    inner: crate::photo::Photo,
}

#[wasm_bindgen]
impl WebPhoto {
    /// Decode one opaque oriented PNG or supported EXIF JPEG and detect plain paper.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8], paper: &str) -> Result<WebPhoto, JsError> {
        crate::photo::Photo::new(bytes, paper)
            .map(|inner| Self { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Local ChArUco detection with an original sheet and optional checked lens calibration.
    pub fn from_reference(
        bytes: &[u8],
        sheet: &str,
        calibration: &str,
        capture: &str,
    ) -> Result<WebPhoto, JsError> {
        crate::photo::Photo::with_reference(bytes, sheet, calibration, capture)
            .map(|inner| Self { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Scale diagnostics and source identity; an `ok: false` result cannot measure points.
    #[must_use]
    pub fn report(&self) -> String {
        self.inner.report().to_string()
    }

    /// Lossless full-resolution oriented preview for pixel-coordinate clicks.
    pub fn preview(&self) -> Result<Vec<u8>, JsError> {
        self.inner
            .preview()
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Native PHOTO_ESTIMATED records, never a fit or readiness verdict.
    pub fn measure(&self, points: &str, options: &str) -> Result<String, JsError> {
        self.inner
            .measure(points, options)
            .map(|v| v.to_string())
            .map_err(|e| JsError::new(&e.to_string()))
    }
}

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
    source_sha256: String,
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
            .map(|inner| WebBody {
                inner,
                source_sha256: ftx_core::receipt::sha256_hex(bytes),
            })
            .map_err(|e| JsError::new(&e))
    }
}

/// Check a body against the current round's targets, without running cloth or changing readiness.
///
/// # Errors
/// Invalid/bounded evidence, a failed tape fit or an inconsistent accepted shape; no fallback.
#[wasm_bindgen]
pub fn inspect_body(
    body: &WebBody,
    measurements: &str,
    accepted_report: Option<String>,
) -> Result<String, JsError> {
    fold::inspect_body(
        &body.inner,
        &body.source_sha256,
        measurements,
        accepted_report.as_deref(),
    )
    .and_then(|value| serde_json::to_string(&value).map_err(|e| e.to_string()))
    .map_err(|e| JsError::new(&e))
}

/// Recompute manufacturing, fresh drapes and requested heat consumers for each material alternative.
/// Fits this source-identified body only once; no earlier material's state is reused.
///
/// # Errors
/// Malformed/excessive input; per-consumer refusals are retained in a successful study envelope.
#[wasm_bindgen]
pub fn material_scenario_study(
    body: &WebBody,
    pattern: &str,
    request: &str,
    scenario: &str,
    measurements: &str,
) -> Result<String, JsError> {
    crate::materials::scenario::on_model(
        &body.inner,
        &body.source_sha256,
        pattern,
        request,
        scenario,
        measurements,
        ftx_progress,
    )
    .map_err(|e| JsError::new(&e.to_string()))
}

/// Fit-constrained manufacturing on the explicitly selected body, with no drape/heat session.
///
/// # Errors
/// Invalid/oversized input, missing design_fit, unavailable body or scenario-dependent policy.
#[wasm_bindgen]
pub fn material_study_on_body(
    body: &WebBody,
    pattern: &str,
    request: &str,
    body_input: &str,
) -> Result<String, JsError> {
    crate::materials::scenario::manufacturing_on_model(
        &body.inner,
        &body.source_sha256,
        pattern,
        request,
        body_input,
    )
    .map_err(|e| JsError::new(&e.to_string()))
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
    two_start: bool,
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
        two_start,
        ftx_progress,
    )
    .map(|mut inner| {
        inner.report["body"]["model_sha256"] = serde_json::json!(body.source_sha256);
        inner.report["measurements_sha256"] =
            serde_json::json!(ftx_core::receipt::sha256_hex(measurements.as_bytes()));
        WebFold { inner }
    })
    .map_err(|e| JsError::new(&e))
}

/// Drape an explicitly associated accepted image-fit shape, with no fallback or tape refitting.
///
/// # Errors
/// Rejected/inconsistent fit, wrong model/wearer, bad pose or a refused cloth solve.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn fold_up_with_body_fit(
    body: &WebBody,
    pattern: &str,
    measurements: &str,
    fit_spec: &str,
    self_contact_mm: f64,
    two_start: bool,
    body_fit: &str,
    arm_deg: f64,
) -> Result<WebFold, JsError> {
    let parse = |s: &str| -> Result<serde_json::Value, JsError> {
        serde_json::from_str(s).map_err(|e| JsError::new(&e.to_string()))
    };
    fold::fold_up_with_body_fit(
        &body.inner,
        &parse(pattern)?,
        &parse(measurements)?,
        &parse(fit_spec)?,
        self_contact_mm,
        two_start,
        &fold::BodyFitSource {
            model_sha256: &body.source_sha256,
            report: body_fit,
            arm_deg,
        },
        ftx_progress,
    )
    .map(|mut inner| {
        inner.report["measurements_sha256"] =
            serde_json::json!(ftx_core::receipt::sha256_hex(measurements.as_bytes()));
        WebFold { inner }
    })
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

/// SVG review of one explicitly named parallel cell's actual admitted machine and stitch recipe.
///
/// # Errors
/// Any complete-job planning refusal, unknown cell/operation or bounded preview failure.
#[wasm_bindgen]
pub fn sew_cell_svg(
    pattern: &str,
    options: &str,
    cell: &str,
    operation: &str,
) -> Result<String, JsError> {
    crate::manufacturing::sew_cell_svg(pattern, options, cell, operation)
        .map_err(|e| JsError::new(&e.message))
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
