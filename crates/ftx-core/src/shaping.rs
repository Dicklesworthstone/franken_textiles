//! Seam 1 (`ShapingSurface`): pure 3D shell contract shared across `ftx-body` and `ftx-pattern`
//! via `ftx-core` so `ftx-body` and `ftx-pattern` stay strictly disjoint sibling crates.

use serde::{Deserialize, Serialize};

use crate::Refusal;

/// Target anatomical region clipped cleanly via `BodyFrames` planes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnitShellRegion {
    /// Waist-to-neck torso cylinder (armholes open).
    Torso,
    /// Shoulder-to-wrist left sleeve cylinder.
    LeftSleeve,
    /// Shoulder-to-wrist right sleeve cylinder.
    RightSleeve,
    /// Full upper-body sweater shell (waist hem, both wrists, neck open).
    FullUpperBody,
    /// Generic open topological tube (beanie/sock/cuff).
    Tube,
}

/// Regional wearing ease specification (negative ease = stretched fit; positive = relaxed).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EaseProfile {
    /// Radial ease at chest/bust/torso, in mm (may be negative for ribbed/fitted knits).
    pub torso_radial_mm: f64,
    /// Radial ease at sleeves/limbs, in mm.
    pub limb_radial_mm: f64,
    /// Minimum physical clearance enforced during positive-ease offset clamping, in mm.
    pub min_clearance_mm: f64,
    /// Explicit consent required for negative-ease (stretched-on-body) garments.
    #[serde(default)]
    pub allow_negative_ease: bool,
}

impl EaseProfile {
    /// Positive relaxed pullover ease (`+12 mm` torso radial, `+8 mm` limb radial).
    #[must_use]
    pub const fn relaxed_pullover() -> Self {
        Self {
            torso_radial_mm: 12.0,
            limb_radial_mm: 8.0,
            min_clearance_mm: 2.0,
            allow_negative_ease: false,
        }
    }
}

/// Ordered boundary loop (hem, cuff, or neckline) on an [`EaseShellMesh`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryRing {
    /// Semantic ring label (e.g. `"waist_hem"`, `"neck"`, `"left_wrist"`, `"right_wrist"`).
    pub name: String,
    /// True for `t = 0` start rings (cast-on hem/cuffs); false for `t = 1` end rings (neck/bind-off).
    pub is_start: bool,
    /// Ordered CCW vertex indices around the boundary loop.
    pub vertices: Vec<u32>,
    /// Measured boundary perimeter in metres.
    pub perimeter_m: f64,
}

/// Open manifold 3D target garment shell (metres, z-up, wearer facing `-y`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EaseShellMesh {
    /// Vertex positions in metres (`z` up).
    pub vertices_m: Vec<[f64; 3]>,
    /// Outward-wound CCW triangle indices.
    pub triangles: Vec<[u32; 3]>,
    /// Named open boundary rings (`>= 2` for harmonic time-field boundary conditions).
    pub boundary_rings: Vec<BoundaryRing>,
    /// Canonical SHA-256 of the underlying fitted body mesh/report.
    pub source_body_sha256: String,
}

/// Trait boundary implemented by `ftx-body` and consumed by `ftx-pattern` without creating a cycle.
pub trait ShapingSurface {
    /// Builds an open manifold [`EaseShellMesh`] for the requested region and ease profile.
    ///
    /// # Errors
    /// [`crate::RefusalCode::DegenerateGeometry`] if the offset self-intersects or leaves `< 2`
    /// boundary rings, or [`crate::RefusalCode::OutOfRange`] if negative ease lacks explicit opt-in.
    fn build_ease_shell(
        &self,
        region: KnitShellRegion,
        ease: &EaseProfile,
    ) -> Result<EaseShellMesh, Refusal>;
}
