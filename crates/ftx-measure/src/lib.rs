//! Measurement evidence and uncertainty, per `docs/spec/02` Part A.
//!
//! Lengths are millimetres, angles degrees, and uncertainty is one sigma. A chart prior and a
//! model prediction never become measured evidence merely because their uncertainty is small.
#![forbid(unsafe_code)]

pub mod camera;
mod catalogue;
mod document;
mod fusion;
pub mod photo;
mod priors;
mod readiness;
mod records;
mod round;
mod section;

pub use catalogue::{NAMES, PROVENANCES, canonical_name, catalogue, definition, unit};
pub use document::{fuse, fused_value, reference_wearer};
pub use fusion::{fuse_one, normalize_record};
pub use priors::{age_from_stature, cdc_stature, priors_for};
pub use readiness::{ReadinessOptions, garment_catalogue, garment_kind, readiness};
pub use records::{date_day, make_record, parse_value, validate};
pub use round::{format_round_readiness, garment_checks, round_readiness};
pub use section::{SectionOptions, calibrate_k, girth_from_section};

use ftx_core::{Refusal, RefusalCode};

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
