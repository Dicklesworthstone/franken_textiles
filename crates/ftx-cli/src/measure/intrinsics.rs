//! Local multi-view calibration and an explicit same-lens handoff to sheet-pose.

use std::{fs::File, io::Read};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_measure::{
    camera::Camera,
    photo::intrinsics::{CalibrationInput, calibrate_camera},
};
use serde_json::{Value, json};

use super::{no_files, option, photo, single};
use crate::{Args, bad_input, known_options, required};

fn read_document(path: &str) -> Result<(Value, Vec<u8>), Refusal> {
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
    known_options(args, "measure calibrate-camera", &["out"])?;
    let file = single(args, "calibrate-camera")?;
    let (value, bytes) = read_document(file)?;
    let input: CalibrationInput = serde_json::from_value(value)
        .map_err(|e| bad_input(format!("invalid camera calibration: {e}")))?;
    let report = calibrate_camera(&input)?;
    let code = if report.solution.is_some() { 0 } else { 2 };
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
    text.push_str(&format!("\n{}", report.no_claim));
    let mut value = serde_json::to_value(&report).map_err(|e| bad_input(e.to_string()))?;
    value["input_sha256"] = json!(sha256_hex(&bytes));
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
    if saved["schema"] != "ftx-camera-calibration/1"
        || saved["status"] != "ACCEPTED"
        || !saved["flags"].as_array().is_some_and(Vec::is_empty)
    {
        return Err(bad_input(
            "sheet-pose requires a saved ACCEPTED camera calibration, not a failed report or CLI envelope",
        ));
    }
    let input: CalibrationInput = serde_json::from_value(saved["input"].clone())
        .map_err(|e| bad_input(format!("invalid saved calibration input: {e}")))?;
    if input.lens_id != lens
        || reference["image_size_px"] != json!(input.image_size_px)
        || reference["rotation_applied_degrees"] != json!(input.rotation_cw_deg)
    {
        return Err(bad_input(
            "lens identifier, image size and applied rotation must match the calibration exactly; no rescaling or reorientation is inferred",
        ));
    }
    // Reuse the real calibration algorithm instead of trusting a stored success flag. This
    // checks internal consistency, not authenticity or whether the actual lens settings changed.
    let checked = calibrate_camera(&input)?;
    let solution = checked
        .solution
        .ok_or_else(|| bad_input("saved calibration no longer passes its input checks"))?;
    let k = serde_json::to_value(solution.intrinsics).map_err(|e| bad_input(e.to_string()))?;
    if saved["solution"]["intrinsics"] != k {
        return Err(bad_input(
            "stored intrinsics differ from calibration inputs; regenerate the calibration report",
        ));
    }
    reference["intrinsics"] = k;
    let mut report = Camera::sheet_pose_report(&reference)?;
    report["intrinsics_source"] = json!({"schema":"ftx-camera-calibration/1", "report_sha256":sha256_hex(&bytes),
        "reference_input_sha256":sha256_hex(&reference_bytes), "lens_id":lens, "recomputed":true,
        "input":input, "no_claim":checked.no_claim});
    let code = if report["ok"] == true { 0 } else { 3 };
    Ok((report, code))
}
