//! The `wasm-bindgen` boundary (wasm32 only): JSON text in and out, files as bytes. The page's worker calls
//! `program_info()`, `form_fields(args)` and `run(req)` as it called `webapp.py` under Pyodide, then
//! `WebBody(bytes)` and `fold_up(body, …)` for the fold-up on a fitted body.

use wasm_bindgen::prelude::*;
/// The threaded build's pool: `await initThreadPool(n)` once, after `init()` and before any drape, starts
/// `n` workers for the solver's parallel evaluation (`threads` feature).
#[cfg(feature = "threads")]
pub use wasm_bindgen_rayon::init_thread_pool;

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

/// Triangulate a calibrated landmark with optional declared click-only uncertainty.
///
/// # Errors
/// The portable input-size, camera, capture, geometry and noise guards refuse.
#[wasm_bindgen]
pub fn triangulate(input: &str, click_noise: &str) -> Result<String, JsError> {
    crate::photo::triangulate(input, click_noise)
        .map(|value| value.to_string())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Triangulate a landmark with optional click and source-replayed shared-lens uncertainty.
/// Lens `/1` fixes camera poses; `/2` re-solves their original sheet observations. Metric scale
/// remains fixed, and the reported components are separate.
///
/// # Errors
/// The shared input budgets, calibration replay, lens grouping and geometry guards refuse.
#[wasm_bindgen]
pub fn triangulate_with_lens_noise(
    input: &str,
    click_noise: &str,
    lens_noise: &str,
) -> Result<String, JsError> {
    crate::photo::triangulate_with_lens_noise(input, click_noise, lens_noise)
        .map(|value| value.to_string())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Triangulate a landmark with separate click, shared-lens and sheet-corner precision.
/// Empty sheet evidence preserves the original lens wrapper. No total covariance is reported.
///
/// # Errors
/// The shared input budgets, exact source replay, calibration and geometry guards refuse.
#[wasm_bindgen]
pub fn triangulate_with_camera_noise(
    input: &str,
    click_noise: &str,
    lens_noise: &str,
    sheet_noise: &str,
) -> Result<String, JsError> {
    crate::photo::triangulate_with_camera_noise(input, click_noise, lens_noise, sheet_noise)
        .map(|value| value.to_string())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Triangulate with a separately source-bound common metric-reference scale contribution.
/// Empty scale evidence preserves the existing camera-noise route; no total covariance is inferred.
///
/// # Errors
/// The shared source budgets, exact sheet replay and geometry guards refuse.
#[wasm_bindgen]
pub fn triangulate_with_reference_scale_noise(
    input: &str,
    click_noise: &str,
    lens_noise: &str,
    sheet_noise: &str,
    reference_scale_noise: &str,
) -> Result<String, JsError> {
    crate::photo::triangulate_with_reference_scale_noise(
        input,
        click_noise,
        lens_noise,
        sheet_noise,
        reference_scale_noise,
    )
    .map(|value| value.to_string())
    .map_err(|e| JsError::new(&e.to_string()))
}

/// Reconstruct a straight distance with separately retained endpoint and shared-source evidence.
///
/// # Errors
/// The portable source budget, exact camera cohort and conditional precision guards refuse.
#[wasm_bindgen]
pub fn triangulate_distance(input: &str) -> Result<String, JsError> {
    crate::photo::triangulate_distance(input)
        .map(|value| value.to_string())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Refine anchored camera/landmark geometry with a text-parsed native iteration limit.
///
/// # Errors
/// The portable source, budget, gauge and camera guards refuse.
#[wasm_bindgen]
pub fn bundle_adjust(input: &str, iterations: &str) -> Result<String, JsError> {
    crate::photo::bundle_adjust(input, iterations)
        .map(|value| value.to_string())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Open the shared unfitted anatomical target picker or apply source-bound model locations.
///
/// # Errors
/// Invalid sources, stale hashes, invalid model targets or removal of an observed name.
#[wasm_bindgen]
pub fn body_fit_targets(model_bytes: &[u8], input: &str, edits: &str) -> Result<String, JsError> {
    crate::body_fit::body_fit_targets(model_bytes, input, edits)
        .map(|value| value.to_string())
        .map_err(|error| JsError::new(&error.to_string()))
}

/// Read explicit capture controls from the exact local model before acquiring photographs.
///
/// # Errors
/// Oversized or invalid converted model bytes.
#[wasm_bindgen]
pub fn body_capture_catalogue(model_bytes: &[u8]) -> Result<String, JsError> {
    crate::body_fit::body_capture_catalogue(model_bytes)
        .map(|value| value.to_string())
        .map_err(|error| JsError::new(&error.to_string()))
}

/// Incremental source-bound still-photo acquisition using the native camera authorities.
/// This handle retains camera/source records and at most one pending derived PNG, never
/// decoded image pixels between calls.
#[wasm_bindgen]
pub struct WebBodyCapture {
    inner: crate::body_fit::BodyCapture,
}

#[wasm_bindgen]
impl WebBodyCapture {
    /// Freeze the complete photo selection, checked calibration and stationary printed board.
    ///
    /// # Errors
    /// Invalid sources, declarations, held-out partitions or input budgets.
    #[wasm_bindgen(constructor)]
    pub fn new(manifest: &str, calibration: &str, sheet: &str) -> Result<Self, JsError> {
        crate::body_fit::BodyCapture::new(manifest, calibration, sheet)
            .map(|inner| Self { inner })
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Freeze explicit per-image calibrations for a `/2` capture manifest. Every override
    /// names an exact declared image and retains its original ACCEPTED calibration JSON.
    ///
    /// # Errors
    /// Invalid/duplicate bindings, mismatched lens or rotation, unqualified calibration or budgets.
    pub fn with_calibrations(
        manifest: &str,
        calibration: &str,
        sheet: &str,
        overrides: &str,
    ) -> Result<Self, JsError> {
        crate::body_fit::BodyCapture::with_calibrations(manifest, calibration, sheet, overrides)
            .map(|inner| Self { inner })
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Exact local image labels in the frozen acquisition order.
    #[must_use]
    pub fn image_ids(&self) -> String {
        serde_json::json!(self.inner.image_ids()).to_string()
    }

    /// Decode and check one original photograph, releasing its pixels before returning.
    ///
    /// # Errors
    /// Wrong source order, duplicate images, calibration mismatch, detection or camera refusal.
    pub fn add_image(&mut self, image: &str, bytes: &[u8]) -> Result<String, JsError> {
        self.inner
            .add_image(image, bytes)
            .map(|value| value.to_string())
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Take the most recent rectified PNG and release its retained bytes.
    /// Its name, hash and dimensions are in the addition receipt's rectification image record.
    /// Empty means rectification was disabled or the PNG was already taken.
    #[must_use]
    pub fn take_image(&mut self) -> Vec<u8> {
        self.inner.take_image()
    }

    /// Construct the ordinary unfinished body-fit input from explicit model choices.
    /// Targets, observations and starting values still require the existing authoring workflow.
    ///
    /// # Errors
    /// Incomplete acquisition, stale model choices, invalid bounds or capture-authoring refusal.
    pub fn finish(&self, model_bytes: &[u8], choices: &str) -> Result<String, JsError> {
        self.inner
            .finish(model_bytes, choices)
            .map(|value| value.to_string())
            .map_err(|error| JsError::new(&error.to_string()))
    }
}

/// Prepare named body-fit initializations or apply bounded edits while keeping evidence fixed.
/// An empty edit string returns the exact original prepared input.
///
/// # Errors
/// Invalid prepared sources/model, excessive inputs, incomplete poses or invalid start values.
#[wasm_bindgen]
pub fn body_fit_starts(model_bytes: &[u8], input: &str, edits: &str) -> Result<String, JsError> {
    crate::body_fit::body_fit_starts(model_bytes, input, edits)
        .map(|value| value.to_string())
        .map_err(|error| JsError::new(&error.to_string()))
}

/// Inspect or apply source-bound manual keypoints and explicitly edited rectified silhouette slices.
/// Imported mask samples and every other prepared input field remain fixed.
///
/// # Errors
/// Invalid sources, frame ordinals, target names, coordinates, uncertainties, fixed evidence roles,
/// unrectified cameras or source-bound mask edits.
#[wasm_bindgen]
pub fn body_fit_observations(
    model_bytes: &[u8],
    input: &str,
    edits: &str,
) -> Result<String, JsError> {
    crate::body_fit::body_fit_observations(model_bytes, input, edits)
        .map(|value| value.to_string())
        .map_err(|error| JsError::new(&error.to_string()))
}

/// Import source-bound binary PNG silhouette samples without changing camera or pixel geometry.
///
/// # Errors
/// Stale sources, invalid mask pixels, existing silhouettes, invalid slices or lens-domain refusal.
#[wasm_bindgen]
pub fn body_fit_mask(
    model_bytes: &[u8],
    input: &str,
    mask_bytes: &[u8],
    selection: &str,
) -> Result<String, JsError> {
    crate::body_fit::body_fit_mask(model_bytes, input, mask_bytes, selection)
        .map(|value| value.to_string())
        .map_err(|error| JsError::new(&error.to_string()))
}

/// Fit prepared multi-frame observations and local model bytes with native acceptance gates.
/// Work limits are strings so the boundary cannot truncate a JavaScript number into a valid limit.
///
/// # Errors
/// Invalid prepared input, model identity/arrays, work controls or canonical fitting admission.
#[wasm_bindgen]
pub fn fit_body(
    model_bytes: &[u8],
    input: &str,
    iterations: &str,
    max_evaluations: &str,
) -> Result<String, JsError> {
    crate::body_fit::fit_body(model_bytes, input, iterations, max_evaluations)
        .map(|value| value.to_string())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Review one accepted image-frame mesh and its original evidence without another fit.
///
/// # Errors
/// Invalid accepted report/model/ordinal, excessive mesh, or failed full-mesh evidence checks.
#[wasm_bindgen]
pub fn review_body_fit_frame(
    model_bytes: &[u8],
    report: &str,
    frame_index: &str,
) -> Result<String, JsError> {
    crate::body_fit::review_body_fit_frame(model_bytes, report, frame_index)
        .map(|value| value.to_string())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Preview one declared start/frame using the actual model, without running or accepting a fit.
///
/// # Errors
/// Invalid prepared sources, ordinals, model identity, initialization bounds or mesh budgets.
#[wasm_bindgen]
pub fn review_body_fit_start(
    model_bytes: &[u8],
    input: &str,
    start_index: &str,
    frame_index: &str,
) -> Result<String, JsError> {
    crate::body_fit::review_body_fit_start(model_bytes, input, start_index, frame_index)
        .map(|value| value.to_string())
        .map_err(|error| JsError::new(&error.to_string()))
}

/// A local photograph checked against the capture identity of an accepted or initialization review.
#[wasm_bindgen]
pub struct WebBodyFitImage {
    inner: crate::body_fit::BodyFitImage,
}

#[wasm_bindgen]
impl WebBodyFitImage {
    /// Verify original encoded bytes and decode only already-oriented capture pixels.
    ///
    /// # Errors
    /// Missing/stale source identity, unsupported encoding/orientation or bounded input refusal.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8], review: &str) -> Result<Self, JsError> {
        crate::body_fit::BodyFitImage::new(bytes, review)
            .map(|inner| Self { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Full-resolution lossless PNG, preserving the original stored pixel coordinates.
    #[must_use]
    pub fn preview(&self) -> Vec<u8> {
        self.inner.preview().to_vec()
    }

    /// Source/review/preview hashes and the already-applied orientation, without image bytes.
    #[must_use]
    pub fn report(&self) -> String {
        self.inner.report().to_string()
    }
}

/// Assess a sparse still-window candidate without creating reviewed capture evidence.
///
/// # Errors
/// The portable source-size, camera and track guards refuse.
#[wasm_bindgen]
pub fn still_window(input: &str, source: &str) -> Result<String, JsError> {
    crate::photo::still_window(input, source)
        .map(|value| value.to_string())
        .map_err(|e| JsError::new(&e.to_string()))
}

/// Derive PHOTO_ESTIMATED girths or calibrate a section coefficient from original local evidence.
///
/// # Errors
/// The shared source, wearer, provenance, option and numerical-domain refusals.
#[wasm_bindgen]
pub fn photo_section(
    width_source: &str,
    depth_source: &str,
    options: &str,
) -> Result<String, JsError> {
    crate::photo::photo_section(width_source, depth_source, options)
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

/// Guided alteration targets from the actual source pattern and expanded physical instances.
///
/// # Errors
/// Invalid pattern structure or the direct alteration input budget.
#[wasm_bindgen]
pub fn alteration_targets(input: &str) -> Result<String, JsError> {
    crate::alteration::targets(input).map_err(|e| JsError::new(&e.to_string()))
}

/// Apply one native alteration without replacing its source; return lossless pattern text.
///
/// # Errors
/// Invalid operations, refused geometry or the native operation and input budgets.
#[wasm_bindgen]
pub fn alter_pattern(
    input: &str,
    operation: &str,
    source: Option<String>,
) -> Result<String, JsError> {
    crate::alteration::apply(input, operation, source.as_deref())
        .map_err(|e| JsError::new(&e.to_string()))
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

/// Resolve measurement evidence through the ordinary round's validation and fusion.
/// Original supplemental document strings remain in the resulting measurement file.
///
/// # Errors
/// Excessive JSON, invalid evidence, missing wearer association or ordinary body refusals.
#[wasm_bindgen]
pub fn measurement_file(body: &str, today: &str) -> Result<String, JsError> {
    if body.len() > 64 * 1024 * 1024 {
        return Err(JsError::new("measurement body JSON exceeds 64 MiB"));
    }
    let body = ftx_export::PyJson::parse(body).map_err(|e| JsError::new(&e))?;
    page::body_file(&body, today)
        .and_then(|data| serde_json::to_string(&data).map_err(|e| e.to_string()))
        .map_err(|e| JsError::new(&e))
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

/// Draft or check a pattern on an explicitly selected accepted shape.
/// Original measurement evidence remains the separate readiness authority.
///
/// # Errors
/// Invalid sources/model, unavailable body dimensions, or ordinary drafting/export refusals.
#[wasm_bindgen]
pub fn run_on_body(
    body: &WebBody,
    req: &str,
    today: &str,
    created: &str,
    accepted_report: &str,
) -> Result<WebRound, JsError> {
    page::run_on_body(
        &body.inner,
        &body.source_sha256,
        req,
        today,
        created,
        accepted_report,
    )
    .map(|inner| WebRound { inner })
    .map_err(|e| JsError::new(&e))
}

/// Draft or check a pattern on the selected body, fitted to tape when no accepted report is supplied.
/// Existing patterns retain their geometry and original drafting identity.
///
/// # Errors
/// Failed fitting/admission, unavailable required geometry/evidence, or pattern/export refusals.
#[wasm_bindgen]
pub fn run_on_selected_body(
    body: &WebBody,
    req: &str,
    today: &str,
    created: &str,
    accepted_report: Option<String>,
) -> Result<WebRound, JsError> {
    page::run_on_selected_body(
        &body.inner,
        &body.source_sha256,
        req,
        today,
        created,
        accepted_report.as_deref(),
    )
    .map(|inner| WebRound { inner })
    .map_err(|e| JsError::new(&e))
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
        fold::FoldOptions {
            self_contact_mm,
            two_start,
        },
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

/// Drape either body source with explicit garment pose and bounded settling settings.
///
/// # Errors
/// Unknown or invalid settings, inconsistent accepted source, or a refused cloth solve.
#[wasm_bindgen]
pub fn fold_up_configured(
    body: &WebBody,
    pattern: &str,
    measurements: &str,
    fit_spec: &str,
    settings_json: &str,
    body_fit: Option<String>,
) -> Result<WebFold, JsError> {
    let settings: fold::FoldSettings = serde_json::from_str(settings_json)
        .map_err(|e| JsError::new(&format!("fold settings: {e}")))?;
    let source = body_fit.as_deref().map(|report| fold::BodyFitSource {
        model_sha256: &body.source_sha256,
        report,
        arm_deg: settings.arm_abduction_deg,
    });
    let parse = |s: &str| -> Result<serde_json::Value, JsError> {
        serde_json::from_str(s).map_err(|e| JsError::new(&e.to_string()))
    };
    fold::fold_up_configured(
        &body.inner,
        &parse(pattern)?,
        &parse(measurements)?,
        &parse(fit_spec)?,
        settings,
        source.as_ref(),
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

/// A garment sewn and draped on a baked synthetic body and kept in memory, re-draped by the production
/// moving-body step as the page moves the body's arms (see [`crate::session`]). Arrays cross the boundary
/// as typed arrays: positions are xyz in metres (z up, wearer facing −y), indices are `u32`.
#[wasm_bindgen]
pub struct WebDrapeSession {
    inner: crate::session::Session,
}

#[wasm_bindgen]
impl WebDrapeSession {
    /// Sews and drapes `pattern` on the baked body (`ftx-posable-body/1` header text and its array data).
    /// `settings_json` is a [`crate::session::SessionSettings`] object; `{}` takes the defaults. The
    /// drape's progress lines go to the worker's `ftxProgress`.
    ///
    /// # Errors
    /// A malformed body, pattern, fit specification or settings, or a refused drape.
    #[wasm_bindgen(constructor)]
    pub fn new(
        body_header: &str,
        body_data: &[u8],
        pattern: &str,
        fit_spec: &str,
        settings_json: &str,
    ) -> Result<WebDrapeSession, JsError> {
        let parse = |name: &str, text: &str| -> Result<serde_json::Value, JsError> {
            serde_json::from_str(text).map_err(|e| JsError::new(&format!("{name}: {e}")))
        };
        let settings = serde_json::from_str(settings_json)
            .map_err(|e| JsError::new(&format!("session settings: {e}")))?;
        let body = crate::session::PosableBody::parse(body_header, body_data)
            .map_err(|e| JsError::new(&e))?;
        crate::session::Session::start(
            body,
            &parse("pattern", pattern)?,
            &parse("fit specification", fit_spec)?,
            settings,
            ftx_progress,
        )
        .map(|inner| WebDrapeSession { inner })
        .map_err(|e| JsError::new(&e))
    }

    /// As the constructor, with the left and right arm at sampled angles (degrees from vertical): the
    /// pieces are placed by the arms in that pose and sewn and hung there.
    ///
    /// # Errors
    /// As the constructor, and an angle that is not one of the body's samples.
    pub fn posed(
        body_header: &str,
        body_data: &[u8],
        pattern: &str,
        fit_spec: &str,
        settings_json: &str,
        left_deg: f64,
        right_deg: f64,
    ) -> Result<WebDrapeSession, JsError> {
        let parse = |name: &str, text: &str| -> Result<serde_json::Value, JsError> {
            serde_json::from_str(text).map_err(|e| JsError::new(&format!("{name}: {e}")))
        };
        let settings = serde_json::from_str(settings_json)
            .map_err(|e| JsError::new(&format!("session settings: {e}")))?;
        let body = crate::session::PosableBody::parse(body_header, body_data)
            .map_err(|e| JsError::new(&e))?;
        crate::session::Session::start_posed(
            body,
            &parse("pattern", pattern)?,
            &parse("fit specification", fit_spec)?,
            settings,
            left_deg,
            right_deg,
            ftx_progress,
        )
        .map(|inner| WebDrapeSession { inner })
        .map_err(|e| JsError::new(&e))
    }

    /// Counts, pieces, the opening drape's phases and its report (`ftx-drape-session/1`) as JSON text.
    #[must_use]
    pub fn summary(&self) -> String {
        self.inner.summary().to_string()
    }

    /// Every frame of the opening fold-up, frame-major.
    #[must_use]
    pub fn replay(&self) -> Vec<f32> {
        self.inner.replay().to_vec()
    }

    /// The cloth as it is now.
    #[must_use]
    pub fn cloth(&self) -> Vec<f32> {
        self.inner.cloth()
    }

    /// The cloth's triangles.
    #[must_use]
    pub fn cloth_triangles(&self) -> Vec<u32> {
        self.inner.cloth_triangles()
    }

    /// Each cloth vertex's place on its flat pattern piece (xy, mm).
    #[must_use]
    pub fn cloth_pattern_mm(&self) -> Vec<f32> {
        self.inner.cloth_pattern_mm()
    }

    /// The displayed body with the arms at these angles (degrees from vertical).
    #[must_use]
    pub fn display(&self, left_deg: f64, right_deg: f64) -> Vec<f32> {
        self.inner.display(left_deg, right_deg)
    }

    /// The displayed body's triangles.
    #[must_use]
    pub fn display_triangles(&self) -> Vec<u32> {
        self.inner.display_triangles()
    }

    /// Moves the arms to these angles over `seconds` of cloth time. Returns each accepted substep as its
    /// time (s) followed by the cloth's positions; the same angles as now let the cloth settle.
    ///
    /// # Errors
    /// An invalid request or a move the solver refuses. The session is then exactly where it was.
    pub fn advance(
        &mut self,
        left_deg: f64,
        right_deg: f64,
        seconds: f64,
    ) -> Result<Vec<f32>, JsError> {
        self.inner
            .advance(left_deg, right_deg, seconds)
            .map_err(|e| JsError::new(&e))
    }

    /// The last accepted move's work as JSON: `trials` (rejected ones included), `substeps` and each
    /// substep's `newton_iterations`; `null` before the first move.
    #[must_use]
    pub fn last_move(&self) -> String {
        self.inner.last_move().to_string()
    }
}
