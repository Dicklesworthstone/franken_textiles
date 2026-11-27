//! Seam 1 (`ShapingSurface`): re-exported from [`ftx_core::shaping`] so `ftx-body` and
//! `ftx-pattern` remain disjoint sibling crates above `ftx-core`.

pub use ftx_core::shaping::{
    BoundaryRing, EaseProfile, EaseShellMesh, KnitShellRegion, ShapingSurface,
};
