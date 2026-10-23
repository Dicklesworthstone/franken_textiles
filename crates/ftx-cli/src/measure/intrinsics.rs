//! Local multi-view calibration and checked same-lens handoffs to sheet poses and photos.

use std::{fs::File, io::Read};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_measure::{camera::Camera, photo::intrinsics::calibrate_document};
use serde_json::{Value, json};

use super::{no_files, option, photo, single};
use crate::{Args, bad_input, known_options, required};

pub(super) use ftx_measure::photo::intrinsics::checked_calibration;

pub(super) fn read_document(path: &str) -> Result<(Value, Vec<u8>), Refusal> {
    const LIMIT: u64 = 16 * 1024 * 1024;
    let io = |e| Refusal::new(RefusalCode::Io, format!("{path}: {e}"));
    let file = File::open(path).map_err(io)?;
    let metadata = file.metadata().map_err(io)?;
    if !metadata.is_file() {
        return Err(bad_input("camera calibration input must be a regular file"));
    }
    let budget = || {
        Refusal::new(
            RefusalCode::BudgetExceeded,
            "camera calibration JSON exceeds 16 MiB",
        )
    };
    if metadata.len() > LIMIT {
        return Err(budget());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 > LIMIT {
        return Err(budget());
    }
    let value = serde_json::from_slice(&bytes).map_err(|e| bad_input(format!("{path}: {e}")))?;
    Ok((value, bytes))
}

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "measure calibrate-camera",
        &["images", "sheet", "detector-python", "corner-noise", "out"],
    )?;
    let (bytes, acquisition) = if let Some(path) = option(args, "images") {
        no_files(args, "calibrate-camera --images")?;
        let (input, receipt) = photo::calibration_images::load(
            path,
            required(args, "sheet")?,
            option(args, "detector-python").unwrap_or("python3"),
        )?;
        let bytes = serde_json::to_vec(&input).map_err(|e| bad_input(e.to_string()))?;
        (bytes, Some(receipt))
    } else {
        if option(args, "sheet").is_some() || option(args, "detector-python").is_some() {
            return Err(bad_input("--sheet and --detector-python require --images"));
        }
        let file = single(args, "calibrate-camera")?;
        let (_, bytes) = read_document(file)?;
        (bytes, None)
    };
    let noise = option(args, "corner-noise")
        .map(read_document)
        .transpose()?;
    let (report, mut value) =
        calibrate_document(&bytes, noise.as_ref().map(|(_, bytes)| bytes.as_slice()))?;
    let code = if report.solution.is_some()
        && report
            .conditional_uncertainty
            .as_ref()
            .is_none_or(|u| u.estimate.is_some())
    {
        0
    } else {
        2
    };
    let mut text = format!(
        "CAMERA CALIBRATION: {} ({} views, {})",
        report.status,
        report.input.views.len(),
        report.input.lens_id
    );
    if let Some(solution) = &report.solution {
        let k = solution.intrinsics;
        text.push_str(&format!(
            "\nFocal lengths: {:.6}, {:.6} px; principal point: {:.6}, {:.6} px",
            k.focal_px.x, k.focal_px.y, k.principal_px.x, k.principal_px.y
        ));
    } else {
        text.push_str("\nNo usable intrinsics or sheet poses were published.");
    }
    for flag in report.flags.iter().chain(&report.advisories) {
        text.push_str(&format!("\n  {flag}"));
    }
    if let Some(u) = &report.conditional_uncertainty {
        text.push_str(&format!(
            "\nConditional intrinsic uncertainty: {}",
            u.status
        ));
        for flag in &u.flags {
            text.push_str(&format!("\n  {flag}"));
        }
    }
    text.push_str(&format!("\n{}", report.no_claim));
    if let Some(receipt) = acquisition {
        value["image_acquisition"] = receipt;
    }
    if let Some(path) = option(args, "out") {
        photo::save(path, &value)?;
        eprintln!("wrote {path}");
    }
    Ok((value, code, text))
}

pub(super) fn sheet_pose(args: &Args) -> Result<(Value, i32), Refusal> {
    known_options(
        args,
        "measure sheet-pose",
        &["reference", "calibration", "lens-id", "out"],
    )?;
    no_files(args, "sheet-pose")?;
    let lens = required(args, "lens-id")?;
    let (mut reference, reference_bytes) = read_document(required(args, "reference")?)?;
    if !reference.is_object() || reference.get("intrinsics").is_some() {
        return Err(bad_input(
            "--calibration requires a sheet-pose reference object without inline intrinsics; no calibration is silently overridden",
        ));
    }
    let (saved, bytes) = read_document(required(args, "calibration")?)?;
    let checked = checked_calibration(&saved)?;
    let input = &checked.input;
    if input.lens_id != lens
        || reference["image_size_px"] != json!(input.image_size_px)
        || reference["rotation_applied_degrees"] != json!(input.rotation_cw_deg)
    {
        return Err(bad_input(
            "lens identifier, image size and applied rotation must match the calibration exactly; no rescaling or reorientation is inferred",
        ));
    }
    let k = serde_json::to_value(checked.solution.as_ref().unwrap().intrinsics)
        .map_err(|e| bad_input(e.to_string()))?;
    reference["intrinsics"] = k;
    let mut report = Camera::sheet_pose_report(&reference)?;
    report["intrinsics_source"] = json!({"schema":"ftx-camera-calibration/1", "report_sha256":sha256_hex(&bytes),
        "reference_input_sha256":sha256_hex(&reference_bytes), "lens_id":lens, "recomputed":true,
        "input":input, "no_claim":checked.no_claim});
    let code = if report["ok"] == true { 0 } else { 3 };
    Ok((report, code))
}
