//! Cutting/BOM/purchasing share one computed marker and its resolved material snapshots.
//! The requirements are gross stock areas, never user-guessed net garment area or drape mesh area.

use std::fs::OpenOptions;
use std::io::Write;

use ftx_core::{Refusal, receipt::sha256_hex};
use ftx_export::cutting::{CutOptions, CutPlan, procurement};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

use super::{invalid, io, read};

fn build(pattern: &str, options: &str) -> Result<CutPlan, Refusal> {
    CutPlan::build(&Pattern::new(read(pattern)?), &read::<CutOptions>(options)?)
}

pub(super) fn plan(pattern: &str, options: &str) -> Result<Value, Refusal> {
    serde_json::to_value(build(pattern, options)?).map_err(|e| invalid(e.to_string()))
}

pub(super) fn price(
    pattern: &str,
    options: &str,
    quotes: &str,
    market: &str,
) -> Result<Value, Refusal> {
    let plan = build(pattern, options)?;
    procurement::evaluate(&plan, &read(quotes)?, &read(market)?)
}

pub(super) fn preview(
    pattern: &str,
    options: &str,
    index: &str,
    out: &str,
) -> Result<Value, Refusal> {
    let index: usize = index
        .parse()
        .map_err(|_| invalid("roll index must be a nonnegative integer"))?;
    let plan = build(pattern, options)?;
    let svg = ftx_export::cutting::preview::svg(&plan, index)?;
    // Produce the complete preview before creating a file; never replace any existing path.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|e| io(out, e))?;
    file.write_all(svg.as_bytes()).map_err(|e| io(out, e))?;
    Ok(json!({"out":out,"svg_sha256":sha256_hex(svg.as_bytes()),
        "pattern_sha256":plan.pattern_sha256,"material_sha256":plan.rolls[index].material.sha256(),
        "roll_index":index,"machine_execution":false}))
}
