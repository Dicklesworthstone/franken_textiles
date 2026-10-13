//! FrankenTextiles drape (plan §11): an implicit cloth solver for sewing patterns onto bodies. This
//! stage has the cloth without contact: an orthotropic Green-strain membrane rotated to the grain,
//! quadratic bending, lumped inertia, and projected Newton on a sparse LDLᵀ with a line search, checked
//! by known-answer tests (membrane anisotropy, the Peirce cantilever). Contact, friction, seams and the
//! garment scene come next (FTX-12, FTX-13).
//!
//! Units: metres, kilograms, seconds; z up.
#![forbid(unsafe_code)]

pub mod cloth;
pub mod eigen;
pub mod elements;
pub mod sparse;

pub use cloth::{ClothModel, SettleReport, SolveReport, Solver, grid, hex_grid};
pub use elements::{Hinge, Membrane, MembraneElement};
