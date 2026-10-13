//! Anchored stationary-scene reconstruction; never relabels optimized geometry as tape evidence.

use ftx_core::Refusal;
use ftx_measure::bundle::{BundleInput, adjust};
use serde_json::Value;

use super::{option, photo, single};
use crate::{Args, bad_input, known_options, read_json};

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "measure bundle-adjust", &["out", "iterations"])?;
    let path = single(args, "bundle-adjust")?;
    let input: BundleInput = serde_json::from_value(read_json(path)?.0)
        .map_err(|e| bad_input(format!("invalid bundle input: {e}")))?;
    let iterations = option(args, "iterations").map_or(Ok(50), |value| {
        value
            .parse::<usize>()
            .map_err(|_| bad_input("--iterations must be an integer in 1..=100"))
    })?;
    let report = adjust(&input, iterations)?;
    let code = match report.status {
        "ACCEPTED" => 0,
        "BUDGET_STOP" => 4,
        _ => 2,
    };
    let mut text = format!(
        "BUNDLE: {} ({} cameras, {} landmarks, {} accepted steps)",
        report.status,
        input.views.len(),
        input.landmarks.len(),
        report.iterations
    );
    if let (Some(first), Some(last)) = (
        report.objective_history.first(),
        report.objective_history.last(),
    ) {
        text.push_str(&format!(
            "\nFit sum of squared normalized pixel residuals: {first:.9} -> {last:.9}"
        ));
    }
    if report.solution.is_none() {
        text.push_str("\nNo optimized geometry was published.");
    }
    for flag in report.flags.iter().chain(&report.advisories) {
        text.push_str(&format!("\n  {flag}"));
    }
    text.push_str(&format!("\n{}", report.no_claim));
    let value = serde_json::to_value(&report).map_err(|e| bad_input(e.to_string()))?;
    if let Some(path) = option(args, "out") {
        // Includes diagnostic-only failures. The same create-only evidence boundary as photos
        // refuses overwriting inputs, existing reports, and symlink/hard-link aliases.
        photo::save(path, &value)?;
        eprintln!("wrote {path}");
    }
    Ok((value, code, text))
}
