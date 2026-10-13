//! Photo commands from explicit coordinates or an oriented PNG. EXIF conversion remains external.

use std::io::Read;
use std::path::Path;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_measure::camera::Camera;
use ftx_measure::photo::{
    MAX_PHOTO_PIXELS, PhotoOptions, analyze_plain_sheet, analyze_reference, measure_photo,
};
use ftx_trace::raster::{Bgr, image::load_png_with_limit};
use serde_json::{Value, json};

use crate::{Args, bad_input, known_options, read_json, required, workspace};

use super::{date, no_files, numeric, option};

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
        &["reference", "image", "paper", "out"]
    } else {
        &[
            "reference",
            "image",
            "paper",
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
    let (scale, source_key, source) = match (option(args, "reference"), option(args, "image")) {
        (Some(path), None) => {
            if option(args, "paper").is_some() {
                return Err(bad_input(
                    "--paper is for --image; coordinate references declare their own paper",
                ));
            }
            let reference = read_json(path)?.0;
            (analyze_reference(&reference)?, "reference_input", reference)
        }
        (None, Some(path)) => {
            let paper = option(args, "paper").unwrap_or("auto");
            if !["letter", "a4", "auto"].contains(&paper) {
                return Err(bad_input("paper must be letter, a4 or auto"));
            }
            let (image, receipt) = load_image(path, MAX_PHOTO_PIXELS)?;
            (
                analyze_plain_sheet(&image.data, image.w, image.h, paper)?,
                "image_input",
                receipt,
            )
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
