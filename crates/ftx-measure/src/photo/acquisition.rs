//! File-free admission of retained lenses and detected ChArUco photographs.
//! Adapters supply original bytes and already oriented full-resolution pixels.

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    MAX_PHOTO_PIXELS, PhotoScale, analyze_reference, detect_charuco,
    intrinsics::{IntrinsicUncertainty, checked_calibration},
};
use crate::{camera::Intrinsics, invalid};

/// Maximum retained camera calibration JSON, shared by local acquisition adapters.
pub const MAX_CALIBRATION_JSON_BYTES: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LensInput {
    schema: String,
    lens_id: String,
    image_size_px: [u32; 2],
    rotation_cw_deg: u16,
    calibration_evidence: String,
    intrinsics: Intrinsics,
}

/// A checked retained lens and any independently conditioned covariance.
pub struct PhotoLens {
    intrinsics: Intrinsics,
    uncertainty: Option<IntrinsicUncertainty>,
    receipt: Value,
    image_size_px: [u32; 2],
}

impl PhotoLens {
    /// Admit a raw lens or recompute a retained calibration using its original observations.
    /// Capture declarations describe this photo; they must not be inferred from the calibration.
    pub fn from_bytes(
        bytes: &[u8],
        capture_lens_id: &str,
        rotation_cw_deg: u16,
        independence: Option<&str>,
    ) -> Result<Self, Refusal> {
        if bytes.len() > MAX_CALIBRATION_JSON_BYTES {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "camera calibration JSON exceeds 16 MiB",
            ));
        }
        if ![0, 90, 180, 270].contains(&rotation_cw_deg) {
            return Err(invalid("rotation_cw_deg must be 0, 90, 180 or 270"));
        }
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|e| invalid(format!("invalid photo lens: {e}")))?;
        let recomputed = value["schema"] == "ftx-camera-calibration/1";
        let mut uncertainty = None;
        let lens: LensInput = if recomputed {
            let checked = checked_calibration(&value)?;
            uncertainty = checked.uncertainty.clone().map_or_else(
                || {
                    checked
                        .conditional_uncertainty
                        .as_ref()
                        .map(IntrinsicUncertainty::from_calibration)
                        .transpose()
                },
                |u| Ok(Some(u)),
            )?;
            LensInput {
                schema: "ftx-photo-lens/1".into(),
                lens_id: checked.input.lens_id,
                image_size_px: checked.input.image_size_px,
                rotation_cw_deg: checked.input.rotation_cw_deg,
                calibration_evidence: checked
                    .input
                    .distortion_fit
                    .map_or(checked.input.pinhole_evidence, |fit| fit.model_evidence),
                intrinsics: checked.solution.unwrap().intrinsics,
            }
        } else {
            serde_json::from_value(value.clone())
                .map_err(|e| invalid(format!("invalid photo lens: {e}")))?
        };
        if lens.schema != "ftx-photo-lens/1"
            || lens.lens_id.trim().is_empty()
            || lens.lens_id.len() > 256
            || lens.calibration_evidence.trim().is_empty()
            || lens.calibration_evidence.len() > 4096
            || lens
                .image_size_px
                .iter()
                .any(|n| !(2..=1_000_000).contains(n))
        {
            return Err(invalid(
                "lens requires ftx-photo-lens/1, a lens_id, calibrated image geometry and nonempty calibration_evidence",
            ));
        }
        if lens.lens_id != capture_lens_id || lens.rotation_cw_deg != rotation_cw_deg {
            return Err(invalid(
                "lens identifier and already-applied rotation must match the lens calibration exactly",
            ));
        }
        lens.intrinsics.validate()?;
        if uncertainty.is_some() {
            if independence.is_none_or(|s| s.trim().is_empty() || s.len() > 4096) {
                return Err(invalid(
                    "calibration carries intrinsic uncertainty: supply independent-calibration-evidence explaining independent acquisition of this photo; correlated calibration/photo noise is not supported",
                ));
            }
        } else if independence.is_some() {
            return Err(invalid(
                "independent-calibration-evidence requires a verified calibration report with declared corner noise and intrinsic uncertainty",
            ));
        }
        let mut receipt = json!({"sha256":sha256_hex(bytes),"calibration":value,
            "capture_lens_id":capture_lens_id,"rotation_applied_degrees":rotation_cw_deg,"recomputed":recomputed,
            "no_claim":"Metadata matching does not prove unchanged physical lens settings; intrinsic uncertainty is not estimated."});
        if uncertainty.is_some() {
            receipt["independence_evidence"] = json!(independence.unwrap());
            receipt["no_claim"] = json!(
                "Conditional first-order intrinsic covariance under declared corner-noise and independent-photo assumptions. No print/flatness error, lens drift or real-camera accuracy claim."
            );
        }
        Ok(Self {
            intrinsics: lens.intrinsics,
            uncertainty,
            receipt,
            image_size_px: lens.image_size_px,
        })
    }

    /// Admitted lens in the original oriented image geometry.
    #[must_use]
    pub fn intrinsics(&self) -> Intrinsics {
        self.intrinsics
    }

    /// Original source document, exact byte digest and explicit capture declarations.
    #[must_use]
    pub fn receipt(&self) -> &Value {
        &self.receipt
    }

    /// Require the exact full-resolution oriented geometry; never infer a crop or resize.
    pub fn check_image(&self, size: &Value) -> Result<(), Refusal> {
        if *size != json!(self.image_size_px) {
            return Err(invalid(
                "lens image_size_px must match the original oriented image exactly; no crop or rescaling is inferred",
            ));
        }
        Ok(())
    }

    /// Reject encoded or decoded calibration-image reuse when independence is required.
    /// `receipt` is the decoder's receipt for these original BGR pixels.
    pub fn check_acquisition(
        &self,
        bgr: &[u8],
        size: [usize; 2],
        receipt: &mut Value,
    ) -> Result<(), Refusal> {
        self.check_image(&json!(size))?;
        if self.uncertainty.is_some() {
            let digest = sha256_hex(bgr);
            let reused = self.receipt["calibration"]["image_acquisition"]["views"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|v| v["pixels_bgr_sha256"] == digest || v["sha256"] == receipt["sha256"]);
            if reused {
                return Err(invalid(
                    "photo reuses a calibration image: independent lens/photo uncertainty is not supported for this acquisition",
                ));
            }
            receipt["pixels_bgr_sha256"] = json!(digest);
        }
        Ok(())
    }

    /// Carry the admitted covariance alongside click and geometric uncertainty.
    pub fn apply_uncertainty(&self, scale: PhotoScale) -> Result<PhotoScale, Refusal> {
        if let Some(uncertainty) = &self.uncertainty {
            scale.with_lens_uncertainty(
                uncertainty,
                self.receipt["independence_evidence"].as_str().unwrap(),
            )
        } else {
            Ok(scale)
        }
    }
}

fn percentile(histogram: &[u32], fraction: f64, value: impl Fn(usize) -> f64) -> f64 {
    let count: u32 = histogram.iter().sum();
    let rank = fraction * f64::from(count - 1);
    let lo = rank.floor() as u32;
    let hi = rank.ceil() as u32;
    let mut seen = 0;
    let mut lower = None;
    for (i, &n) in histogram.iter().enumerate() {
        seen += n;
        if seen > lo && lower.is_none() {
            lower = Some(value(i));
        }
        if seen > hi {
            let a = lower.unwrap();
            return a + (rank - f64::from(lo)) * (value(i) - a);
        }
    }
    unreachable!("nonempty bounded pixel histogram")
}

// Native spec A.10 diagnostic, extracted unchanged. Fixed-point BGR luma and Sobel/percentile
// diagnostics use the fitted scale's ROI. Missing texture/contrast stays unknown, never zero.
fn blur(bgr: &[u8], width: usize, height: usize, reference: &Value, square_px: f64) -> Option<f64> {
    let corners = reference["corners"].as_array()?;
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for corner in corners {
        for k in 0..2 {
            let v = corner["point_px"][k].as_f64()?;
            low[k] = low[k].min(v);
            high[k] = high[k].max(v);
        }
    }
    let x0 = (low[0] - square_px).floor().max(0.0) as usize;
    let y0 = (low[1] - square_px).floor().max(0.0) as usize;
    let x1 = (high[0] + square_px).ceil().min(width as f64) as usize;
    let y1 = (high[1] + square_px).ceil().min(height as f64) as usize;
    let w = x1.checked_sub(x0)?;
    let h = y1.checked_sub(y0)?;
    if w < 3 || h < 3 || w * h < 100 {
        return None;
    }
    let gray: Vec<u8> = bgr
        .chunks_exact(3)
        .map(|p| {
            ((u32::from(p[0]) * 1868 + u32::from(p[1]) * 9617 + u32::from(p[2]) * 4899 + (1 << 13))
                >> 14) as u8
        })
        .collect();
    let mut values = [0_u32; 256];
    let mut gradients = vec![0_u32; 2 * 1020 * 1020 + 1];
    let reflect = |i: isize, n: usize| -> usize {
        if i < 0 {
            1
        } else if i >= n as isize {
            n - 2
        } else {
            i as usize
        }
    };
    let sample =
        |x: isize, y: isize| i32::from(gray[(y0 + reflect(y, h)) * width + x0 + reflect(x, w)]);
    for y in 0..h as isize {
        for x in 0..w as isize {
            values[sample(x, y) as usize] += 1;
            let gx = sample(x + 1, y - 1) + 2 * sample(x + 1, y) + sample(x + 1, y + 1)
                - sample(x - 1, y - 1)
                - 2 * sample(x - 1, y)
                - sample(x - 1, y + 1);
            let gy = sample(x - 1, y + 1) + 2 * sample(x, y + 1) + sample(x + 1, y + 1)
                - sample(x - 1, y - 1)
                - 2 * sample(x, y - 1)
                - sample(x + 1, y - 1);
            gradients[(gx * gx + gy * gy) as usize] += 1;
        }
    }
    let contrast =
        percentile(&values, 0.98, |i| i as f64) - percentile(&values, 0.02, |i| i as f64);
    let gradient = percentile(&gradients, 0.995, |i| (i as f64).sqrt() / 8.0);
    if contrast <= 0.0 || gradient <= 0.0 {
        return None;
    }
    let raw = contrast / ((2.0 * std::f64::consts::PI).sqrt() * gradient);
    Some((raw * raw - 0.64).max(0.0).sqrt())
}

/// Finalize detected coordinates using the source sheet's physical geometry and admitted lens.
/// The native optional external detector and the portable Rust detector share this authority.
pub fn analyze_charuco_detection(
    bgr: &[u8],
    width: usize,
    height: usize,
    sheet: &Value,
    response: &Value,
    intrinsics: Option<Intrinsics>,
) -> Result<(PhotoScale, Value), Refusal> {
    if width
        .checked_mul(height)
        .is_none_or(|n| n > MAX_PHOTO_PIXELS || n < 4)
        || width < 2
        || height < 2
        || bgr.len() != width * height * 3
    {
        return Err(invalid("invalid full-resolution ChArUco BGR image"));
    }
    if sheet["schema"] != "fabric-calib-sheet/1" || sheet["board"]["type"] != "charuco" {
        return Err(invalid(
            "sheet must be a fabric-calib-sheet/1 ChArUco sidecar",
        ));
    }
    let mut reference = response["reference"].clone();
    if reference["reference"] != "charuco" || reference["image_size_px"] != json!([width, height]) {
        return Err(invalid(
            "ChArUco detector returned the wrong reference/image frame",
        ));
    }
    reference["board"] = json!({
        "squares_x":sheet["board"]["squares_x"],"squares_y":sheet["board"]["squares_y"],
        "square_mm":sheet["board"]["square_mm"],
        "origin_mm":sheet["board"].get("origin_mm").cloned().unwrap_or(json!([0,0]))
    });
    reference["paper"] = sheet["paper"].clone();
    if reference.get("intrinsics").is_some() {
        return Err(invalid(
            "ChArUco detector cannot supply or override physical lens calibration",
        ));
    }
    if let Some(k) = intrinsics {
        reference["intrinsics"] = json!(k);
    }
    let mut scale = analyze_reference(&reference)?;
    if let Some(square_px) = scale.report()["square_px"].as_f64() {
        reference["blur_sigma_px_estimate"] =
            json!(blur(bgr, width, height, &reference, square_px));
        scale = analyze_reference(&reference)?;
    }
    let receipt = json!({"sheet":sheet,"implementation":response["detector"],
        "reference_input":reference,"blur_method":"Sobel3/8 ROI percentiles (spec A.10)"});
    Ok((scale, receipt))
}

/// Detect and analyze one ChArUco board in the original full-resolution pixel frame.
pub fn analyze_charuco_image(
    bgr: &[u8],
    width: usize,
    height: usize,
    sheet: &Value,
    intrinsics: Option<Intrinsics>,
) -> Result<(PhotoScale, Value), Refusal> {
    let response = detect_charuco(bgr, width, height, sheet, intrinsics)?;
    analyze_charuco_detection(bgr, width, height, sheet, &response, intrinsics)
}

/// Identify image-detected coordinates in both scale and measurement reports.
pub fn annotate_charuco(report: &mut Value) {
    if report["image_input"].get("charuco_detection").is_some() {
        report["coordinate_source"] = json!("detected_charuco");
        report["notes"][0] = json!(
            "ChArUco IDs and subpixel corners detected locally; homography, inliers and uncertainty computed in Rust. Pixels are as displayed: a PNG as stored, a JPEG with its EXIF orientation applied."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_percentiles_interpolate_magnitudes_not_squared_values() {
        let mut histogram = vec![0; 101];
        histogram[0] = 1;
        histogram[4] = 1;
        histogram[100] = 2;
        assert_eq!(percentile(&histogram, 0.5, |i| (i as f64).sqrt()), 6.0);
        assert_eq!(percentile(&histogram, 0.0, |i| i as f64), 0.0);
        assert_eq!(percentile(&histogram, 1.0, |i| i as f64), 100.0);
    }

    #[test]
    fn blur_requires_texture_and_recognizes_an_unblurred_step() {
        let mut bgr = vec![80; 100 * 100 * 3];
        let reference = json!({"corners":[{"point_px":[10,10]},{"point_px":[90,90]}]});
        assert_eq!(blur(&bgr, 100, 100, &reference, 10.0), None);
        for y in 0..100 {
            for x in 50..100 {
                bgr[3 * (y * 100 + x)..3 * (y * 100 + x) + 3].fill(240);
            }
        }
        assert_eq!(blur(&bgr, 100, 100, &reference, 10.0), Some(0.0));
        assert_eq!(blur(&bgr, 100, 100, &json!({"corners":[]}), 10.0), None);
    }
}
