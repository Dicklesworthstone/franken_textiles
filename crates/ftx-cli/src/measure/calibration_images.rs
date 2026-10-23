//! Native image-file adapter for the shared incremental camera acquisition authority.

use std::path::Path;

use ftx_core::Refusal;
use ftx_measure::photo::{
    MAX_PHOTO_PIXELS, calibration_images::Acquisition, intrinsics::CalibrationInput,
};
use serde_json::{Value, json};

use super::{charuco_image, load_image};
use crate::{bad_input, measure::intrinsics::read_document};

pub(in crate::measure) fn load(
    manifest_path: &str,
    sheet_path: &str,
    python: &str,
) -> Result<(CalibrationInput, Value), Refusal> {
    let (_, manifest_bytes) = read_document(manifest_path)?;
    let (_, sheet_bytes) = read_document(sheet_path)?;
    let mut acquisition = Acquisition::new(&manifest_bytes, &sheet_bytes)?;
    let base = Path::new(manifest_path).parent().unwrap_or(Path::new("."));
    for id in acquisition.image_ids() {
        let path = base.join(&id);
        let path = path
            .to_str()
            .ok_or_else(|| bad_input("image path must be UTF-8"))?;
        let (image, receipt) = load_image(path, MAX_PHOTO_PIXELS)?;
        acquisition.add_image(&id, &image.data, [image.w, image.h], receipt, |sheet| {
            charuco_image::calibration_corners(&image, sheet, python)
        })?;
    }
    let (input, mut receipt) = acquisition.finish()?;
    receipt["manifest_path"] = json!(manifest_path);
    receipt["sheet_path"] = json!(sheet_path);
    Ok((input, receipt))
}
