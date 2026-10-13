//! FrankenTextiles drape (plan §11): an implicit cloth solver for sewing patterns onto bodies.
//!
//! - The cloth ([`cloth`], [`elements`]): an orthotropic Green-strain membrane rotated to the grain,
//!   quadratic bending, lumped inertia, projected Newton on a sparse LDLᵀ with a line search.
//! - Contact with a static body ([`contact`], [`distance`], [`ad`]): the IPC barrier on the
//!   thickness-offset gap, additive CCD, lagged Coulomb friction, an exact start audit.
//! - The garment ([`garment`], [`start`], [`nearest`], [`scene`]): the reference's mesh and start state,
//!   then welded seams, placement on a body and the sew and drape schedule.
//! - The fold-up animation and its files ([`animation`]): board, roll-up, zip, PC2, the viewer payload.
//! - Metrics and flags with the reference's texts ([`metrics`]).
//! - One run end to end, as `ftx drape` calls it ([`run`]).
//!
//! Units: metres, kilograms, seconds; z up.
#![forbid(unsafe_code)]

pub mod ad;
pub mod animation;
mod bvh;
pub mod cloth;
pub mod contact;
pub mod distance;
pub mod eigen;
pub mod elements;
pub mod garment;
pub mod metrics;
pub mod nearest;
pub mod render;
pub mod run;
pub mod scene;
pub mod sparse;
pub mod start;

pub use cloth::{ClothModel, SettleReport, SolveReport, Solver, Tube, grid, hex_grid, hex_tube};
pub use contact::{Body, Contact, ContactParams, Intersection};
pub use elements::{Hinge, Membrane, MembraneElement};
pub use garment::{GarmentMesh, SeamRecord, build_garment};
pub use nearest::{Nearest, NearestTree};
pub use start::{PartFrame, StartState, WrapPlacement, start_state, wrap_points};
