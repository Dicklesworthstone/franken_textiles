//! Coordinate-based photo commands; image detection and EXIF processing remain external.

use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_measure::camera::Camera;
use ftx_measure::photo::{PhotoOptions, analyze_reference, measure_photo};
use serde_json::{Value, json};

use crate::{Args, bad_input, known_options, read_json, required, workspace};

use super::{date, no_files, numeric, option};

fn integer<T: std::str::FromStr>(args: &Args, key: &str, default: T) -> Result<T, Refusal> {
    match option(args, key) {
        None => Ok(default),
        Some(s) => s
            .parse()
            .map_err(|_| bad_input(format!("--{key} must be an unsigned integer in range"))),
    }
}

pub(super) fn run(args: &Args, command: &str) -> Result<(Value, i32), Refusal> {
    let names: &[&str] = if command == "photo-scale" || command == "sheet-pose" {
        &["reference", "out"]
    } else {
        &[
            "reference",
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
    let reference = read_json(required(args, "reference")?)?.0;
    if command == "sheet-pose" {
        let report = Camera::sheet_pose_report(&reference)?;
        let code = if report["ok"] == true { 0 } else { 3 };
        return Ok((report, code));
    }
    let scale = analyze_reference(&reference)?;
    if command == "photo-scale" {
        let mut report = scale.report().clone();
        report["reference_input"] = reference;
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
    result["photo"]["reference_input"] = reference;
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
