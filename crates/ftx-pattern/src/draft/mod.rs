//! Measured parametric drafting (`docs/spec/02` Parts B–C): the reference's analytic body and its three
//! programs, each `program(measurements, style) → (pattern, fit spec)`. The body owns size; the style owns
//! character.

pub mod bolero;
pub mod builder;
pub mod fez;
pub mod form;
pub mod geos;
pub mod trousers;

use ftx_core::{Refusal, RefusalCode};
use serde_json::Value;

/// The programs, in the reference's registry order (`PROGRAMS`).
pub const PROGRAMS: [&str; 3] = ["zouave_bolero", "zouave_trousers", "fez"];

/// Runs a drafting program on a measurement document (`fabric-measurements/1` or flat) and style overrides.
///
/// # Errors
/// `unknown_parameter` for an unknown program or style key; `out_of_range` for a fez style out of its
/// ranges; `invalid_document` for a missing measurement; `degenerate_geometry` when the draft cannot be built.
pub fn draft(
    program: &str,
    meas: &Value,
    style: Option<&Value>,
) -> Result<(Value, Value), Refusal> {
    match program {
        "zouave_bolero" => bolero::zouave_bolero(meas, style),
        "zouave_trousers" => trousers::zouave_trousers(meas, style),
        "fez" => fez::fez(meas, style),
        _ => Err(Refusal::new(
            RefusalCode::UnknownParameter,
            format!("unknown program {program:?}"),
        )
        .repair(format!("programs: {}", PROGRAMS.join(", ")))),
    }
}
