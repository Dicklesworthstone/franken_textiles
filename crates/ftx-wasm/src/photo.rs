//! File-free photo evidence through the native decoder, detector and uncertainty engine.

use ftx_core::{Refusal, RefusalCode};
use ftx_measure::photo::{
    MAX_PHOTO_PIXELS, PhotoOptions, PhotoScale, analyze_plain_sheet, measure_photo,
};
use ftx_trace::raster::image::{Bgr, encode_png_bgr, load_photo_with_limit};
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_JSON: usize = 256 * 1024;

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
        })
    }

    /// Native scale report and exact source identity, without any filesystem path or photo pixels.
    #[must_use]
    pub fn report(&self) -> Value {
        let mut report = self.scale.report().clone();
        report["image_input"] = self.receipt.clone();
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
        result["photo"]["measurement_options"] = json!({
            "depth_offset_mm":options.depth_offset_mm,"camera_distance_mm":options.camera_distance_mm,
            "click_px":options.click_px,"view":options.view,"points_from_width":options.points_from_width,
            "sheet_level_sigma_deg":options.sheet_level_sigma_deg,"bootstrap":options.bootstrap,"seed":options.seed
        });
        Ok(result)
    }
}
