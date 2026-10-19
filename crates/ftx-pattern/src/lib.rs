//! FrankenTextiles patterns (`docs/spec/01`): the `fabric-pattern/1` document, instances (mirror pairs
//! and on-fold unfolding), structure checks with the reference's messages, seam sides, notch fractions
//! and seam sampling. Validation, alteration and drafting build on this.
//!
//! Frame: millimetres, 2D, x right, y up, loops counter-clockwise; every piece is drafted as the
//! wearer's right side seen from its public side.
#![forbid(unsafe_code)]

pub mod alter;
pub mod doc;
pub mod draft;
pub mod instance;
pub mod material_bending;
pub mod material_bending_calibration;
pub mod material_binding;
pub mod material_calibration;
pub mod material_catalog;
pub mod material_compression;
pub mod material_cost;
pub mod material_friction;
pub mod material_optics;
pub mod material_plasticity;
pub mod material_relaxation;
pub mod material_response;
pub mod material_screen;
pub mod material_thermal;
pub mod materials;
pub mod pattern;
pub mod round;
pub mod textile_import;
pub mod validate;

pub use doc::{
    Cut, Edge, Grain, Mark, Notch, PatternDoc, Piece, Placement, SCHEMA, Seam, Vertices, truthy,
};
pub use instance::{InstEdge, Instance, PyError, Side, piece_instances};
pub use pattern::{Pattern, SeamPairs, piece_definition_errors, py_repr_value};
pub use validate::{BodyMeasure, Measurements, Report, default_fit_spec, format_table, validate};
