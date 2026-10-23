//! File-free photo evidence through the native decoder, detector and uncertainty engine.

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_measure::photo::{
    MAX_PHOTO_PIXELS, PhotoOptions, PhotoScale,
    acquisition::{PhotoLens, analyze_charuco_image, annotate_charuco},
    analyze_plain_sheet,
    calibration_images::Acquisition,
    detect_charuco,
    intrinsics::{calibrate_document, uncertainty::CalibrationNoise},
    measure_photo,
};
use ftx_trace::raster::image::{Bgr, encode_png_bgr, load_photo_with_limit};
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_JSON: usize = 256 * 1024;

/// Fit original coordinate observations through the shared camera engine. Empty noise means no
/// separate noise document; explicit requests retain source bytes and never imply zero covariance.
///
/// # Errors
/// The shared input-size, schema, calibration-domain or explicit-noise guards refuse.
pub fn camera_calibration(input: &str, corner_noise: &str) -> Result<Value, Refusal> {
    calibrate_document(
        input.as_bytes(),
        (!corner_noise.is_empty()).then_some(corner_noise.as_bytes()),
    )
    .map(|(_, value)| value)
}

/// One bounded image batch. Decoded photos are released after each call; only corner observations
/// and source receipts survive until the canonical calibration is solved.
pub struct CameraCalibration {
    acquisition: Acquisition,
    corner_noise: String,
}

impl CameraCalibration {
    /// Freeze the original native image manifest, sheet and optional noise request before pixels.
    ///
    /// # Errors
    /// Invalid/oversized source documents, unsupported acquisition or noise, missing independent
    /// observations, or a declared image batch exceeding the shared bounds.
    pub fn new(manifest: &str, sheet: &str, corner_noise: &str) -> Result<Self, Refusal> {
        if corner_noise.len() > ftx_measure::photo::acquisition::MAX_CALIBRATION_JSON_BYTES {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "camera calibration JSON exceeds 16 MiB",
            ));
        }
        if !corner_noise.is_empty() {
            let noise: CalibrationNoise = serde_json::from_str(corner_noise)
                .map_err(|e| invalid(format!("invalid calibration corner noise: {e}")))?;
            noise.validate()?;
        }
        Ok(Self {
            acquisition: Acquisition::new(manifest.as_bytes(), sheet.as_bytes())?,
            corner_noise: corner_noise.into(),
        })
    }

    /// Exact manifest names, sorted by the native frame identity; every name must be supplied once.
    #[must_use]
    pub fn image_ids(&self) -> Vec<String> {
        self.acquisition.image_ids()
    }

    /// Decode and detect one full-resolution PNG/JPEG using the same Rust authorities as photos.
    /// Neither the lens fit nor photo-scale RANSAC participates in acquisition.
    ///
    /// # Errors
    /// Out-of-order, invalid, oversized, wrong-sized or duplicate image; detector refusal;
    /// or insufficient observations for the original FIT/held-out partitions.
    pub fn add_image(&mut self, image: &str, bytes: &[u8]) -> Result<Value, Refusal> {
        self.acquisition.check_next_image(image)?;
        let (pixels, receipt) = load_photo_with_limit(bytes, MAX_PHOTO_PIXELS)?;
        self.acquisition.add_image(
            image,
            &pixels.data,
            [pixels.w, pixels.h],
            receipt,
            |sheet| detect_charuco(&pixels.data, pixels.w, pixels.h, sheet, None),
        )
    }

    /// Fit only the complete original batch. Failed calibration gates remain ordinary inspectable
    /// reports; missing images or malformed evidence refuse without fitting a partial batch.
    ///
    /// # Errors
    /// Incomplete acquisition or canonical calibration/noise input refusal.
    pub fn finish(&self) -> Result<Value, Refusal> {
        let (input, acquisition) = self.acquisition.finish()?;
        let bytes = serde_json::to_vec(&input).map_err(|e| invalid(e.to_string()))?;
        let (_, mut report) = calibrate_document(
            &bytes,
            (!self.corner_noise.is_empty()).then_some(self.corner_noise.as_bytes()),
        )?;
        report["image_acquisition"] = acquisition;
        Ok(report)
    }
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, Refusal> {
    if text.len() > MAX_JSON {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "photo JSON exceeds 256 KiB",
        ));
    }
    serde_json::from_str(text).map_err(|e| invalid(e.to_string()))
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Options {
    depth_offset_mm: f64,
    camera_distance_mm: f64,
    click_px: f64,
    sheet_level_sigma_deg: f64,
    bootstrap: usize,
    seed: u64,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    capture_lens_id: Option<String>,
    rotation_cw_deg: Option<u16>,
    independent_calibration_evidence: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        let o = PhotoOptions::default();
        Self {
            depth_offset_mm: o.depth_offset_mm,
            camera_distance_mm: o.camera_distance_mm,
            click_px: o.click_px,
            sheet_level_sigma_deg: o.sheet_level_sigma_deg,
            bootstrap: o.bootstrap,
            seed: o.seed,
        }
    }
}

/// One immutable image/calibration; points always refer to the full-resolution oriented pixels.
pub struct Photo {
    image: Bgr,
    scale: PhotoScale,
    receipt: Value,
    lens_input: Option<Value>,
}

impl Photo {
    /// Decode one supported photo and run the existing plain Letter/A4/auto detector.
    /// Rejected scales retain diagnostics but cannot produce measurements.
    ///
    /// # Errors
    /// The shared image-format, byte/pixel and detector work limits or unknown paper.
    pub fn new(bytes: &[u8], paper: &str) -> Result<Self, Refusal> {
        if !["letter", "a4", "auto"].contains(&paper) {
            return Err(invalid("paper must be letter, a4 or auto"));
        }
        let (image, receipt) = load_photo_with_limit(bytes, MAX_PHOTO_PIXELS)?;
        let scale = analyze_plain_sheet(&image.data, image.w, image.h, paper)?;
        Ok(Self {
            image,
            scale,
            receipt,
            lens_input: None,
        })
    }

    /// Detect a ChArUco sheet using its original sidecar and an optional retained calibration.
    /// `calibration` is empty for uncalibrated detection. Otherwise `capture` explicitly declares
    /// this photo's lens ID and already-applied rotation, plus independence evidence when needed.
    /// Retained calibration is recomputed; no camera model is newly identified from this photo.
    ///
    /// # Errors
    /// Invalid/budget-exceeding JSON or image, mismatched lens geometry, failed source verification,
    /// correlated acquisition, unsupported board, or the canonical detector/uncertainty refusals.
    pub fn with_reference(
        bytes: &[u8],
        sheet: &str,
        calibration: &str,
        capture: &str,
    ) -> Result<Self, Refusal> {
        let sheet_value: Value = parse(sheet)?;
        let capture: Capture = parse(capture)?;
        let lens = if calibration.is_empty() {
            if capture.capture_lens_id.is_some()
                || capture.rotation_cw_deg.is_some()
                || capture.independent_calibration_evidence.is_some()
            {
                return Err(invalid(
                    "capture lens identity, rotation and independence evidence require a calibration",
                ));
            }
            None
        } else {
            Some(PhotoLens::from_bytes(
                calibration.as_bytes(),
                capture
                    .capture_lens_id
                    .as_deref()
                    .ok_or_else(|| invalid("calibration requires explicit capture_lens_id"))?,
                capture
                    .rotation_cw_deg
                    .ok_or_else(|| invalid("calibration requires explicit rotation_cw_deg"))?,
                capture.independent_calibration_evidence.as_deref(),
            )?)
        };
        let (image, mut receipt) = load_photo_with_limit(bytes, MAX_PHOTO_PIXELS)?;
        if let Some(lens) = &lens {
            lens.check_acquisition(&image.data, [image.w, image.h], &mut receipt)?;
        }
        let (scale, mut detection) = analyze_charuco_image(
            &image.data,
            image.w,
            image.h,
            &sheet_value,
            lens.as_ref().map(PhotoLens::intrinsics),
        )?;
        detection["sheet_sha256"] = json!(sha256_hex(sheet.as_bytes()));
        receipt["charuco_detection"] = detection;
        let scale = if let Some(lens) = &lens {
            lens.apply_uncertainty(scale)?
        } else {
            scale
        };
        Ok(Self {
            image,
            scale,
            receipt,
            lens_input: lens.map(|l| l.receipt().clone()),
        })
    }

    /// Native scale report and exact source identity, without any filesystem path or photo pixels.
    #[must_use]
    pub fn report(&self) -> Value {
        let mut report = self.scale.report().clone();
        report["image_input"] = self.receipt.clone();
        if let Some(lens) = &self.lens_input {
            report["lens_input"] = lens.clone();
        }
        annotate_charuco(&mut report);
        report
    }

    /// Lossless preview of the actual decoded/oriented measurement frame, without resampling.
    ///
    /// # Errors
    /// Invalid raster geometry or the shared 16-million-pixel encoding limit.
    pub fn preview(&self) -> Result<Vec<u8>, Refusal> {
        encode_png_bgr(&self.image).map_err(invalid)
    }

    /// Ordinary `fabric-measurements/1`, retaining decomposed PHOTO_ESTIMATED uncertainty.
    /// Capture time is unknown. The options expose native defaults, not inferred camera geometry.
    ///
    /// # Errors
    /// Invalid/oversized points or options, rejected scale or unstable uncertainty bootstrap.
    pub fn measure(&self, points: &str, options: &str) -> Result<Value, Refusal> {
        let points: Value = parse(points)?;
        let o: Options = parse(options)?;
        let options = PhotoOptions {
            depth_offset_mm: o.depth_offset_mm,
            camera_distance_mm: o.camera_distance_mm,
            click_px: o.click_px,
            sheet_level_sigma_deg: o.sheet_level_sigma_deg,
            bootstrap: o.bootstrap,
            seed: o.seed,
            ..PhotoOptions::default()
        };
        let mut result = measure_photo(&self.scale, &points, &options)?;
        result["taken_on"] = Value::Null;
        result["photo"]["image_input"] = self.receipt.clone();
        if let Some(lens) = &self.lens_input {
            result["photo"]["lens_input"] = lens.clone();
        }
        annotate_charuco(&mut result["photo"]);
        result["photo"]["measurement_options"] = json!({
            "depth_offset_mm":options.depth_offset_mm,"camera_distance_mm":options.camera_distance_mm,
            "click_px":options.click_px,"view":options.view,"points_from_width":options.points_from_width,
            "sheet_level_sigma_deg":options.sheet_level_sigma_deg,"bootstrap":options.bootstrap,"seed":options.seed
        });
        Ok(result)
    }
}
