//! FrankenTextiles tracing (plan §13, `docs/spec/04` §10): a raster pattern sheet becomes an UNSCALED
//! `fabric-pattern/1` draft (1 unit = 1 crop px), which is then named, scaled and audited.
//!
//! This crate holds the commands that work on a traced draft, ported from the reference's `vectorize.py`:
//! `names.json` ([`annotate()`]), the reference grammar ([`measure_ref`]), [`fit_scale`], the round-0
//! [`audit()`] with its label consensus, and [`parse_length`]. Every message is the reference's text.
//! The raster trace itself (thresholds, contours, corners, marks) comes next.
#![forbid(unsafe_code)]

pub mod annotate;
pub mod audit;
pub mod geos;
pub mod length;
pub mod ordered;
pub mod refs;
pub mod scale;

pub use annotate::{Names, annotate};
pub use audit::{AuditOptions, audit, label_consensus};
pub use length::{parse_length, parse_number};
pub use ordered::OValue;
pub use refs::{measure_ref, piece_instance, resolve_side, twin};
pub use scale::fit_scale;
