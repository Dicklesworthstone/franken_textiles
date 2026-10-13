//! Calibrated sparse-landmark still-window candidates. A passing candidate still requires review.

use std::io::Read;
use std::path::Path;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_measure::still::{StillWindowInput, assess_still_window};
use serde_json::{Value, json};

use crate::{Args, bad_input, known_options};

const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;

fn read_bounded(path: &Path) -> Result<Vec<u8>, Refusal> {
    let io = |e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display()));
    let file = std::fs::File::open(path).map_err(io)?;
    let metadata = file.metadata().map_err(io)?;
    if !metadata.is_file() {
        return Err(bad_input("still-window input must be a regular file"));
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "still-window input exceeds 16 MiB",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "still-window input exceeds 16 MiB",
        ));
    }
    Ok(bytes)
}

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "measure still-window", &["out"])?;
    let path = Path::new(super::single(args, "still-window")?);
    let bytes = read_bounded(path)?;
    let input: StillWindowInput = serde_json::from_slice(&bytes)
        .map_err(|e| bad_input(format!("{}: {e}", path.display())))?;
    let report = assess_still_window(&input)?;
    let code = if report.status == "STILL_CANDIDATE" {
        0
    } else {
        2
    };
    let mut text = format!(
        "STILL WINDOW: {} ({}..{} us, {} fit + {} held-out landmarks)",
        report.status,
        report.start_us,
        report.end_us,
        report.fit_landmarks,
        report.held_out_landmarks
    );
    for check in &report.checks {
        text.push_str(&format!(
            "\n  {} ({:?}): motion {}, max all-frame reprojection {}",
            check.name,
            check.partition,
            check
                .motion_mm
                .map_or_else(|| "unavailable".into(), |v| format!("{v:.3} mm")),
            check
                .all
                .max_reprojection_px
                .map_or_else(|| "unavailable".into(), |v| format!("{v:.3} px"))
        ));
        for flag in &check.flags {
            text.push_str(&format!("\n    {flag}"));
        }
    }
    for flag in report.flags.iter().chain(&report.advisories) {
        text.push_str(&format!("\n  {flag}"));
    }
    text.push_str(&format!("\n{}", report.no_claim));
    let mut value = serde_json::to_value(&report).map_err(|e| bad_input(e.to_string()))?;
    value["input_sha256"] = json!(sha256_hex(&bytes));
    value["source"] = json!(path.to_string_lossy());
    if let Some(output) = super::option(args, "out") {
        super::photo::save(output, &value)?;
        eprintln!("wrote {output}");
    }
    Ok((value, code, text))
}
