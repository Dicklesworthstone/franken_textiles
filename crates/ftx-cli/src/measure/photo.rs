//! Photo commands from explicit coordinates or an oriented PNG. EXIF conversion remains external.

#[path = "calibration_images.rs"]
pub(super) mod calibration_images;
#[path = "charuco_image.rs"]
mod charuco_image;

use std::io::Read;
use std::path::Path;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_measure::camera::{Camera, Intrinsics};
use ftx_measure::photo::{
    MAX_PHOTO_PIXELS, PhotoOptions, analyze_plain_sheet, analyze_reference, measure_photo,
};
use ftx_trace::raster::{Bgr, image::load_png_with_limit};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Args, bad_input, known_options, read_json, required, workspace};

use super::{date, no_files, numeric, option};

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

impl LensInput {
    fn check_image(&self, size: &Value) -> Result<(), Refusal> {
        if *size != json!(self.image_size_px) {
            return Err(bad_input(
                "lens image_size_px must match the original oriented image exactly; no crop or rescaling is inferred",
            ));
        }
        Ok(())
    }
}

fn read_lens(args: &Args) -> Result<Option<(LensInput, Value)>, Refusal> {
    let Some(path) = option(args, "lens") else {
        if option(args, "lens-id").is_some() || option(args, "rotation-cw-deg").is_some() {
            return Err(bad_input("--lens-id and --rotation-cw-deg require --lens"));
        }
        return Ok(None);
    };
    if option(args, "paper").is_some()
        || (option(args, "image").is_some() && option(args, "sheet").is_none())
    {
        return Err(bad_input(
            "--lens requires ChArUco (--image with --sheet, or a ChArUco --reference); distorted plain-sheet edges are not supported",
        ));
    }
    let id = required(args, "lens-id")?;
    let rotation: u16 = required(args, "rotation-cw-deg")?
        .parse()
        .map_err(|_| bad_input("--rotation-cw-deg must be 0, 90, 180 or 270"))?;
    if ![0, 90, 180, 270].contains(&rotation) {
        return Err(bad_input("--rotation-cw-deg must be 0, 90, 180 or 270"));
    }
    let (value, bytes) = super::intrinsics::read_document(path)?;
    let recomputed = value["schema"] == "ftx-camera-calibration/1";
    let lens: LensInput = if recomputed {
        let checked = super::intrinsics::checked_calibration(&value)?;
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
            .map_err(|e| bad_input(format!("invalid photo lens: {e}")))?
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
        return Err(bad_input(
            "lens requires ftx-photo-lens/1, a lens_id, calibrated image geometry and nonempty calibration_evidence",
        ));
    }
    if lens.lens_id != id || lens.rotation_cw_deg != rotation {
        return Err(bad_input(
            "lens identifier and already-applied rotation must match the lens calibration exactly",
        ));
    }
    lens.intrinsics.validate()?;
    let receipt = json!({"path":path,"sha256":sha256_hex(&bytes),"calibration":value,
        "capture_lens_id":id,"rotation_applied_degrees":rotation,"recomputed":recomputed,
        "no_claim":"Metadata matching does not prove unchanged physical lens settings; intrinsic uncertainty is not estimated."});
    Ok(Some((lens, receipt)))
}

/// Bound both encoded input and decoded storage. Opaque 8-bit pixels preserve photo brightness;
/// composited alpha, clipped 16-bit gray and silently ignored EXIF are not measurement evidence.
pub(super) fn load_image(path: &str, max_pixels: usize) -> Result<(Bgr, Value), Refusal> {
    const MAX_BYTES: u64 = 64 * 1024 * 1024;
    let io = |e| Refusal::new(RefusalCode::Io, format!("{path}: {e}"));
    let file = std::fs::File::open(path).map_err(io)?;
    let metadata = file.metadata().map_err(io)?;
    if !metadata.is_file() {
        return Err(bad_input("photo input must be a regular file"));
    }
    let budget = || {
        Refusal::new(
            RefusalCode::BudgetExceeded,
            "photo PNG exceeds 64 MiB encoded input",
        )
    };
    if metadata.len() > MAX_BYTES {
        return Err(budget());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(budget());
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(bad_input(
            "photo input must be an oriented PNG; convert JPEG, HEIC or WebP first",
        ));
    }
    let mut pos = 8usize;
    while bytes.len() - pos >= 12 {
        let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        let end = pos
            .checked_add(12)
            .and_then(|n| n.checked_add(len))
            .filter(|&n| n <= bytes.len())
            .ok_or_else(|| bad_input("truncated PNG chunk"))?;
        match &bytes[pos + 4..pos + 8] {
            b"IHDR" if len == 13 => {
                let width =
                    u32::from_be_bytes(bytes[pos + 8..pos + 12].try_into().unwrap()) as usize;
                let height =
                    u32::from_be_bytes(bytes[pos + 12..pos + 16].try_into().unwrap()) as usize;
                if width.checked_mul(height).is_none_or(|n| n > max_pixels) {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        format!(
                            "photo exceeds {max_pixels} pixels; resize and click on the same oriented image"
                        ),
                    ));
                }
                if bytes[pos + 16] != 8 || !matches!(bytes[pos + 17], 0 | 2) {
                    return Err(bad_input("photo PNG must be opaque 8-bit grayscale or RGB"));
                }
            }
            b"eXIf" => {
                return Err(bad_input(
                    "apply EXIF orientation to pixels and export a PNG without EXIF before measuring",
                ));
            }
            b"tRNS" => {
                return Err(bad_input(
                    "transparent PNG pixels cannot supply photo evidence; export opaque RGB",
                ));
            }
            b"acTL" => {
                return Err(bad_input(
                    "animated PNG is not one identified photo; export the selected frame",
                ));
            }
            b"IEND" => break,
            _ => {}
        }
        pos = end;
    }
    let image = load_png_with_limit(&bytes, max_pixels).map_err(bad_input)?;
    let receipt = json!({"path":path, "sha256":sha256_hex(&bytes),
        "image_size_px":[image.w,image.h], "orientation":"stored_pixels_without_exif",
        "encoding":"opaque_8_bit_png"});
    Ok((image, receipt))
}

fn integer<T: std::str::FromStr>(args: &Args, key: &str, default: T) -> Result<T, Refusal> {
    match option(args, key) {
        None => Ok(default),
        Some(s) => s
            .parse()
            .map_err(|_| bad_input(format!("--{key} must be an unsigned integer in range"))),
    }
}

pub(super) fn run(args: &Args, command: &str) -> Result<(Value, i32), Refusal> {
    if command == "sheet-pose" && option(args, "calibration").is_some() {
        return super::intrinsics::sheet_pose(args);
    }
    let names: &[&str] = if command == "sheet-pose" {
        &["reference", "out"]
    } else if command == "photo-scale" {
        &[
            "reference",
            "image",
            "paper",
            "sheet",
            "detector-python",
            "lens",
            "lens-id",
            "rotation-cw-deg",
            "out",
        ]
    } else {
        &[
            "reference",
            "image",
            "paper",
            "sheet",
            "detector-python",
            "lens",
            "lens-id",
            "rotation-cw-deg",
            "points",
            "depth-offset-mm",
            "camera-distance-mm",
            "click-px",
            "view",
            "points-from-width",
            "sheet-level-sigma-deg",
            "bootstrap",
            "seed",
            "taken-on",
            "out",
        ]
    };
    known_options(args, command, names)?;
    no_files(args, command)?;
    if command == "sheet-pose" {
        let reference = read_json(required(args, "reference")?)?.0;
        let report = Camera::sheet_pose_report(&reference)?;
        let code = if report["ok"] == true { 0 } else { 3 };
        return Ok((report, code));
    }
    if option(args, "sheet").is_some()
        && (option(args, "image").is_none()
            || option(args, "reference").is_some()
            || option(args, "paper").is_some())
    {
        return Err(bad_input(
            "--sheet requires --image and cannot be combined with --reference or --paper",
        ));
    }
    if option(args, "detector-python").is_some() && option(args, "sheet").is_none() {
        return Err(bad_input("--detector-python requires --image and --sheet"));
    }
    if option(args, "reference").is_some() == option(args, "image").is_some() {
        return Err(bad_input(
            "give exactly one of --reference FILE or --image PNG",
        ));
    }
    let lens = read_lens(args)?;
    let (scale, source_key, source) = match (option(args, "reference"), option(args, "image")) {
        (Some(path), None) => {
            if option(args, "paper").is_some() {
                return Err(bad_input(
                    "--paper is for --image; coordinate references declare their own paper",
                ));
            }
            let reference = read_json(path)?.0;
            let scale = if let Some((lens, _)) = &lens {
                if reference["reference"] != "charuco" || reference.get("intrinsics").is_some() {
                    return Err(bad_input(
                        "--lens requires a ChArUco reference without inline intrinsics; no calibration is silently overridden",
                    ));
                }
                lens.check_image(&reference["image_size_px"])?;
                let mut calibrated = reference.clone();
                calibrated["intrinsics"] = json!(lens.intrinsics);
                analyze_reference(&calibrated)?
            } else {
                analyze_reference(&reference)?
            };
            (scale, "reference_input", reference)
        }
        (None, Some(path)) => {
            let paper = option(args, "paper").unwrap_or("auto");
            if !["letter", "a4", "auto"].contains(&paper) {
                return Err(bad_input("paper must be letter, a4 or auto"));
            }
            let (image, mut receipt) = load_image(path, MAX_PHOTO_PIXELS)?;
            if let Some((lens, _)) = &lens {
                lens.check_image(&json!([image.w, image.h]))?;
            }
            let scale = if let Some(sheet) = option(args, "sheet") {
                let (scale, detection) = charuco_image::analyze(
                    &image,
                    sheet,
                    option(args, "detector-python").unwrap_or("python3"),
                    lens.as_ref().map(|(lens, _)| lens.intrinsics),
                )?;
                receipt["charuco_detection"] = detection;
                scale
            } else {
                analyze_plain_sheet(&image.data, image.w, image.h, paper)?
            };
            (scale, "image_input", receipt)
        }
        _ => {
            return Err(bad_input(
                "give exactly one of --reference FILE or --image PNG",
            ));
        }
    };
    if command == "photo-scale" {
        let mut report = scale.report().clone();
        report[source_key] = source;
        if let Some((_, receipt)) = lens {
            report["lens_input"] = receipt;
        }
        charuco_image::annotate(&mut report);
        let code = if report["ok"] == true { 0 } else { 3 };
        return Ok((report, code));
    }
    let points = read_json(required(args, "points")?)?.0;
    let options = PhotoOptions {
        depth_offset_mm: numeric(args, "depth-offset-mm")?.unwrap_or(0.0),
        camera_distance_mm: numeric(args, "camera-distance-mm")?.unwrap_or(2500.0),
        click_px: numeric(args, "click-px")?.unwrap_or(3.0),
        view: option(args, "view").map(str::to_owned),
        points_from_width: numeric(args, "points-from-width")?,
        sheet_level_sigma_deg: numeric(args, "sheet-level-sigma-deg")?.unwrap_or(1.5),
        bootstrap: integer(args, "bootstrap", 200)?,
        seed: integer(args, "seed", 0)?,
    };
    // No capture date can be inferred from a coordinates file or its source-image label.
    let taken_on = option(args, "taken-on")
        .map(|_| date(args, "taken-on"))
        .transpose()?;
    let mut result = measure_photo(&scale, &points, &options)?;
    result["taken_on"] = json!(taken_on);
    result["photo"][source_key] = source;
    if let Some((_, receipt)) = lens {
        result["photo"]["lens_input"] = receipt;
    }
    charuco_image::annotate(&mut result["photo"]);
    result["photo"]["measurement_options"] = json!({
        "depth_offset_mm":options.depth_offset_mm,
        "camera_distance_mm":options.camera_distance_mm,
        "click_px":options.click_px,
        "view":options.view,
        "points_from_width":options.points_from_width,
        "sheet_level_sigma_deg":options.sheet_level_sigma_deg,
        "bootstrap":options.bootstrap,"seed":options.seed
    });
    Ok((result, 0))
}

pub(super) fn save(path: &str, value: &Value) -> Result<(), Refusal> {
    // Photo inputs are evidence, not editable caches. A typo in --out must not replace reference
    // coordinates, landmark clicks or an earlier measurement, including a symlink/hard-link alias.
    let bytes = workspace::json_bytes(value)?;
    let path = Path::new(path);
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent)
            .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", parent.display())))?;
    }
    workspace::write_new(path, &bytes)
}
