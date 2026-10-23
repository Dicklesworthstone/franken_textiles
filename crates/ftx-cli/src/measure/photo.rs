//! Photo commands from explicit coordinates, an oriented PNG or a JPEG (its EXIF orientation applied).

#[path = "calibration_images.rs"]
pub(super) mod calibration_images;
#[path = "charuco_image.rs"]
mod charuco_image;

use std::io::Read;
use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_measure::camera::Camera;
use ftx_measure::photo::{
    MAX_PHOTO_PIXELS, PhotoOptions, acquisition::PhotoLens, analyze_plain_sheet, analyze_reference,
    measure_photo,
};
use ftx_trace::raster::{Bgr, image::load_photo_with_limit};
use serde_json::{Value, json};

use crate::{Args, bad_input, known_options, read_json, required, workspace};

use super::{date, no_files, numeric, option};

struct LensSource {
    lens: PhotoLens,
    receipt: Value,
}

fn read_lens(args: &Args) -> Result<Option<LensSource>, Refusal> {
    let Some(path) = option(args, "lens") else {
        if option(args, "lens-id").is_some()
            || option(args, "rotation-cw-deg").is_some()
            || option(args, "independent-calibration-evidence").is_some()
        {
            return Err(bad_input(
                "lens identity, rotation and independent-calibration-evidence require --lens",
            ));
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
    let (_, bytes) = super::intrinsics::read_document(path)?;
    let lens = PhotoLens::from_bytes(
        &bytes,
        id,
        rotation,
        option(args, "independent-calibration-evidence"),
    )?;
    let mut receipt = lens.receipt().clone();
    receipt["path"] = json!(path);
    Ok(Some(LensSource { lens, receipt }))
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
            "photo exceeds 64 MiB encoded input",
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
    let (image, mut receipt) = load_photo_with_limit(&bytes, max_pixels)?;
    receipt["path"] = json!(path);
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
            "independent-calibration-evidence",
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
            "independent-calibration-evidence",
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
            let scale = if let Some(source) = &lens {
                let lens = &source.lens;
                if reference["reference"] != "charuco" || reference.get("intrinsics").is_some() {
                    return Err(bad_input(
                        "--lens requires a ChArUco reference without inline intrinsics; no calibration is silently overridden",
                    ));
                }
                lens.check_image(&reference["image_size_px"])?;
                let mut calibrated = reference.clone();
                calibrated["intrinsics"] = json!(lens.intrinsics());
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
            if let Some(source) = &lens {
                source
                    .lens
                    .check_acquisition(&image.data, [image.w, image.h], &mut receipt)?;
            }
            let scale = if let Some(sheet) = option(args, "sheet") {
                let (scale, detection) = charuco_image::analyze(
                    &image,
                    sheet,
                    option(args, "detector-python"),
                    lens.as_ref().map(|s| s.lens.intrinsics()),
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
    let scale = if let Some(source) = &lens {
        source.lens.apply_uncertainty(scale)?
    } else {
        scale
    };
    if command == "photo-scale" {
        let mut report = scale.report().clone();
        report[source_key] = source;
        if let Some(source) = lens {
            report["lens_input"] = source.receipt;
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
    if let Some(source) = lens {
        result["photo"]["lens_input"] = source.receipt;
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
