//! Measurement evidence and uncertainty, per `docs/spec/02` Part A.
//!
//! Lengths are millimetres, angles degrees, and uncertainty is one sigma. A chart prior and a
//! model prediction never become measured evidence merely because their uncertainty is small.
#![forbid(unsafe_code)]

mod catalogue;
mod fusion;
mod records;

pub use catalogue::{NAMES, PROVENANCES, canonical_name, catalogue, definition, unit};
pub use fusion::{fuse_one, normalize_record};
pub use records::{date_day, make_record, parse_value, validate};

use ftx_core::{Refusal, RefusalCode};

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
