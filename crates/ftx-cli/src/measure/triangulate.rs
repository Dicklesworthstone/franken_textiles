//! Calibrated multi-view coordinates; no decoder, image detector or guessed metric scale.

use ftx_core::Refusal;
use ftx_measure::triangulate::{TriangulationInput, triangulate};
use serde_json::Value;

use super::{option, photo, single};
use crate::{Args, bad_input, known_options, read_json};

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "measure triangulate", &["out"])?;
    let path = single(args, "triangulate")?;
    let input: TriangulationInput = serde_json::from_value(read_json(path)?.0)
        .map_err(|e| bad_input(format!("invalid triangulation input: {e}")))?;
    let report = triangulate(&input)?;
    let code = if report.point_m.is_some() { 0 } else { 2 };
    let mut text = format!(
        "TRIANGULATION: {} ({} fit, {} held-out views)",
        report.status, report.fit_views, report.held_out_views
    );
    if let Some(point) = report.point_m {
        text.push_str(&format!(
            "\nWorld point [m, z up]: [{:.9}, {:.9}, {:.9}]",
            point.x, point.y, point.z
        ));
    } else {
        text.push_str("\nNo usable point was published.");
    }
    for flag in report.flags.iter().chain(&report.advisories) {
        text.push_str(&format!("\n  {flag}"));
    }
    text.push_str(&format!("\n{}", report.no_claim));
    let value = serde_json::to_value(&report).map_err(|e| bad_input(e.to_string()))?;
    if let Some(path) = option(args, "out") {
        // Reuse exclusive evidence creation: --out may not overwrite an input or old report,
        // including through a symlink/hard link. Finish computation before creating any file.
        photo::save(path, &value)?;
        eprintln!("wrote {path}");
    }
    Ok((value, code, text))
}
