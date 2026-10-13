//! FrankenTextiles foundations (plan §6–7): no domain logic, only what every other crate shares.
//!
//! - [`det`]: deterministic elementary functions (bit-identical native and wasm).
//! - [`exact`]: the exact orientation predicate for intersection audits.
//! - [`intersection`]: exact triangle contact, with shared mesh features decided rather than skipped.
//! - [`vec`]: 2D/3D vectors and 3×3 matrices.
//! - [`linalg`]: minimum-norm least squares.
//! - [`np`] and [`py`]: the numpy and Python semantics the reference's numbers and messages depend
//!   on (`docs/spec/01` §13).
//! - [`curve`] and [`polyline`]: chord-relative edge curves at the reference's sample counts, and the
//!   polyline/polygon helpers (`docs/spec/01` §4–5).
//! - [`refusal`] and [`receipt`]: typed refusals and run receipts.
//!
//! Units: pattern geometry is in millimetres (2D, y up); bodies and drapes are in metres (3D, z up).
#![forbid(unsafe_code)]

pub mod curve;
pub mod det;
pub mod exact;
pub mod intersection;
pub mod linalg;
pub mod np;
pub mod polyline;
pub mod py;
pub mod receipt;
pub mod refusal;
pub mod vec;

pub use curve::{Curve, CurveError, CurveKind};
pub use polyline::End;
pub use refusal::{Refusal, RefusalCode};
pub use vec::{Mat3, Vec2, Vec3};
