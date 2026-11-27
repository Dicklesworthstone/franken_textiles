#![forbid(unsafe_code)]

//! Pure-Kernel Leaf lifted from `/dp/frankensim/crates/fs-tangle/src/kernel/` (`FS-K02`).
//!
//! Provenance:
//! - Source files: `/dp/frankensim/crates/fs-tangle/src/kernel/{mod.rs,bishop.rs,linking.rs,relax.rs,golden_vectors.rs}`
//! - Upstream bead: `FS-K02` (`/dp/frankensim` workspace snapshot `2026-11-27`; note `/dp/frankensim`
//!   has no `.git` directory, post-`frankensim-source-manifest.json` `source_root`
//!   `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`)
//! - Pinned golden digests:
//!   - `TANGLE_KERNEL_GOLDEN_FNV64 = 0xd91e_8c64_cb58_7c59`
//!   - `TANGLE_KERNEL_GOLDEN_SHA256 = 3124c95697888175cacff67d41861dafbd41e26834b468ef6738985db4d9b5f6`
//! - Local adaptations:
//!   - Packaged as a single leaf file with inline `pub mod` submodules (`bishop`, `linking`, `relax`,
//!     `golden_vectors`) to preserve upstream paths with zero new crate dependencies.
//!   - Added [`FtxDetMath`] binding [`DetMath`] to `ftx_core::det::{sin, cos, atan2, sqrt, ln, exp, acos}`.
//!   - Crate root `crates/ftx-pattern/src/lib.rs` enforces `#![forbid(unsafe_code)]` alongside this
//!     leaf module's `#![forbid(unsafe_code)]`.
//!
//! Invariants of this leaf module:
//! - `#![forbid(unsafe_code)]`
//! - All transcendentals routed via [`DetMath`] (zero platform `libm`)
//! - Zero `mul_add` in hot loops
//! - Zero `HashMap` / `HashSet` iteration ordering

use core::fmt;

pub use bishop::{BishopFrames, compute_bishop_frames};
pub use linking::{
    SegmentClosest, certify_topology, segment_pair_gauss_solid_angle, segment_segment_closest,
};
pub use relax::relax_collision_free_embedding;

/// Deterministic elementary-function interface decoupling pure kernel math
/// from platform `libm`.
///
/// Methods are associated functions (no `&self`) so call sites use `M::sin(x)`
/// path syntax, which never triggers method-call `libm` scanners and imposes
/// zero runtime overhead.
pub trait DetMath {
    /// Deterministic sine of `x` (radians).
    fn sin(x: f64) -> f64;
    /// Deterministic cosine of `x` (radians).
    fn cos(x: f64) -> f64;
    /// Deterministic four-quadrant arctangent `atan2(y, x)` (radians).
    fn atan2(y: f64, x: f64) -> f64;
    /// Deterministic IEEE-754 square root.
    fn sqrt(x: f64) -> f64;
    /// Deterministic natural logarithm.
    fn ln(x: f64) -> f64;
    /// Deterministic exponential `e^x`.
    fn exp(x: f64) -> f64;
    /// Deterministic arccosine (radians).
    fn acos(x: f64) -> f64;
}

/// Canonical [`DetMath`] binding backed by `ftx_core::det`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FtxDetMath;

impl DetMath for FtxDetMath {
    #[inline]
    fn sin(x: f64) -> f64 {
        ftx_core::det::sin(x)
    }

    #[inline]
    fn cos(x: f64) -> f64 {
        ftx_core::det::cos(x)
    }

    #[inline]
    fn atan2(y: f64, x: f64) -> f64 {
        ftx_core::det::atan2(y, x)
    }

    #[inline]
    fn sqrt(x: f64) -> f64 {
        ftx_core::det::sqrt(x)
    }

    #[inline]
    fn ln(x: f64) -> f64 {
        ftx_core::det::ln(x)
    }

    #[inline]
    fn exp(x: f64) -> f64 {
        ftx_core::det::exp(x)
    }

    #[inline]
    fn acos(x: f64) -> f64 {
        ftx_core::det::acos(x)
    }
}

/// Periodicity domain of an embedded 1D filament complex.
#[derive(Clone, Debug, PartialEq)]
pub enum Periodicity {
    /// Non-periodic open or closed curves in R^3.
    Bounded,
    /// 1D helical screw periodicity along `+z`: image `k` rotates by
    /// `k * twist_rad` around `+z` and translates by `(0, 0, k * pitch_m)`.
    Helical1D {
        /// Axial repeat pitch in metres (strictly positive).
        pitch_m: f64,
        /// Screw twist angle per axial period in radians.
        twist_rad: f64,
    },
    /// 2D planar Bravais lattice spanned by `(u_vec, v_vec)` with free out-of-plane coordinate.
    Planar2D {
        /// First lattice translation vector in metres.
        u_vec: [f64; 3],
        /// Second lattice translation vector in metres.
        v_vec: [f64; 3],
    },
    /// 2D tubular/cylindrical periodicity: axial translation by `pitch_m` along `+z`
    /// and discrete circumferential rotational symmetry of order `circumferential_count`.
    TubularHelical2D {
        /// Nominal cylinder radius in metres (strictly positive).
        radius_m: f64,
        /// Axial repeat pitch in metres (strictly positive).
        pitch_m: f64,
        /// Number of circumferential copies around `2*pi` (at least 1).
        circumferential_count: u32,
    },
    /// 3D Bravais lattice with three linearly independent basis vectors in metres.
    Lattice3D {
        /// Three 3D lattice basis vectors `[b0, b1, b2]` in metres.
        basis: [[f64; 3]; 3],
    },
}

/// Signed handedness of a 3D filament crossing.
///
/// Using the right-hand Gauss integral convention: for an over-segment with
/// tangent `t_over` and under-segment with tangent `t_under`, let
/// `d = p_over - p_under` connect closest points. A crossing is `Positive`
/// iff `(t_over x t_under) . d > 0` and `Negative` iff `< 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossingSign {
    /// Positive right-handed crossing (`(t_over x t_under) . (p_over - p_under) > 0`).
    Positive,
    /// Negative left-handed crossing (`(t_over x t_under) . (p_over - p_under) < 0`).
    Negative,
}

/// Prescribed local crossing invariant between two filament segments.
#[derive(Clone, Debug, PartialEq)]
pub struct CrossingSpec {
    /// Index into `FilamentComplex::curves` for the over-strand.
    pub over_curve: usize,
    /// Segment index along `over_curve`.
    pub over_segment: usize,
    /// Index into `FilamentComplex::curves` for the under-strand.
    pub under_curve: usize,
    /// Segment index along `under_curve`.
    pub under_segment: usize,
    /// Periodic image shift applied to `under_curve` relative to `over_curve`.
    pub under_image_shift: [i32; 3],
    /// Non-zero viewing direction along which `over` must lie strictly above `under`.
    pub view_dir: [f64; 3],
    /// Prescribed crossing handedness.
    pub expected_sign: CrossingSign,
}

/// A single directed polyline filament curve inside a [`FilamentComplex`].
#[derive(Clone, Debug, PartialEq)]
pub struct FilamentCurve {
    /// Stable caller-assigned filament identifier.
    pub id: u32,
    /// Ordered node indices into `FilamentComplex::positions_m` (length `>= 2`
    /// for open curves; `>= 3` for non-periodic closed curves; `>= 1` for
    /// periodic closed curves with non-zero `periodic_shift`).
    pub nodes: Vec<usize>,
    /// True when the curve closes from its last node back to `nodes[0]`
    /// (shifted by `periodic_shift`).
    pub closed: bool,
    /// Periodic image shift connecting the end of the curve to `nodes[0]` when
    /// `closed == true`; must be `[0, 0, 0]` for `Periodicity::Bounded` or open curves.
    pub periodic_shift: [i32; 3],
    /// Nominal cross-sectional filament radius in metres (`>= 0.0`).
    pub radius_m: f64,
    /// Optional per-segment material twist angles (radians) relative to the
    /// reference Bishop frame; empty means zero explicit twist.
    pub segment_twist_rad: Vec<f64>,
}

impl FilamentCurve {
    /// Number of directed segments in this curve.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        if self.closed {
            self.nodes.len()
        } else {
            self.nodes.len().saturating_sub(1)
        }
    }
}

/// Canonical 1D embedded filament complex with flat SI coordinates (`m`).
#[derive(Clone, Debug, PartialEq)]
pub struct FilamentComplex {
    /// Periodicity domain of the embedding.
    pub periodicity: Periodicity,
    /// Flat node coordinates `[x0, y0, z0, x1, y1, z1, ...]` in metres.
    pub positions_m: Vec<f64>,
    /// Ordered filament curves referencing nodes in `positions_m`.
    pub curves: Vec<FilamentCurve>,
    /// Optional explicit topological crossing certificates to enforce.
    pub crossings: Vec<CrossingSpec>,
}

/// Certified topological invariants produced by [`certify_topology`].
#[derive(Clone, Debug, PartialEq)]
pub struct TopologyCertificate {
    /// Pairwise integer Gauss linking numbers `(curve_id_a, curve_id_b, lk)`
    /// for all pairs of closed non-periodic curves (`a < b`).
    pub pairwise_linking: Vec<(u32, u32, i32)>,
    /// Raw floating-point Gauss linking integrals `(curve_id_a, curve_id_b, raw_lk)`
    /// matching `pairwise_linking` for residual auditing.
    pub pairwise_linking_raw: Vec<(u32, u32, f64)>,
    /// Maximum deviation `|raw_lk - round(raw_lk)|` across all certified closed pairs.
    pub max_linking_residual: f64,
    /// Writhe per curve (meaningful for closed curves; `0.0` for open curves).
    pub writhe: Vec<f64>,
    /// Minimum pairwise clearance `min(dist(seg_a, seg_b) - (r_a + r_b))` in metres
    /// over all non-excluded segment pairs (and periodic neighbor images).
    /// Returns `f64::INFINITY` if no non-excluded pair exists.
    pub min_clearance_m: f64,
    /// Minimum center-to-center distance across all non-excluded pairs (metres).
    pub min_centerline_dist_m: f64,
    /// Number of prescribed `CrossingSpec` invariants verified.
    pub crossings_checked: usize,
    /// Indices of any `CrossingSpec` entries whose over/under order or sign failed.
    pub failed_crossings: Vec<usize>,
    /// True iff no non-excluded segments intersect, all closed-curve linking
    /// numbers isolate within `±0.25` of an integer, and all prescribed crossings hold.
    pub isotopy_valid: bool,
}

/// Structured fail-closed error returned by `lifted_tangle` kernel routines.
#[derive(Clone, Debug, PartialEq)]
pub enum KernelErr {
    /// `positions_m.len()` is not a multiple of 3.
    InvalidPositionBuffer {
        /// Actual length of `positions_m`.
        len: usize,
    },
    /// A coordinate, radius, twist, or periodicity scalar is NaN or infinite.
    NonFiniteInput {
        /// Diagnostic context describing which input was non-finite.
        context: &'static str,
    },
    /// A curve references a node index outside `0..node_count`.
    NodeIndexOutOfRange {
        /// Curve index.
        curve_idx: usize,
        /// Out-of-range node index.
        node: usize,
        /// Available node count.
        node_count: usize,
    },
    /// Periodicity parameters or an image shift violate domain invariants.
    InvalidPeriodicity {
        /// Diagnostic detail.
        detail: &'static str,
    },
    /// A curve definition is structurally invalid (too few nodes, negative radius, etc.).
    InvalidCurve {
        /// Curve index.
        curve_idx: usize,
        /// Diagnostic detail.
        detail: &'static str,
    },
    /// A segment has near-zero length (`<= 1e-15 m`).
    DegenerateSegment {
        /// Curve index.
        curve_idx: usize,
        /// Segment index.
        segment_idx: usize,
    },
    /// Consecutive segments reverse by `~180°`, leaving parallel transport undefined.
    AntiparallelTangents {
        /// Curve index.
        curve_idx: usize,
        /// Joint index between segments.
        joint_idx: usize,
    },
    /// A `CrossingSpec` references invalid indices or a zero viewing vector.
    InvalidCrossingSpec {
        /// Index in `complex.crossings`.
        crossing_idx: usize,
        /// Diagnostic detail.
        detail: &'static str,
    },
    /// Two non-excluded segments intersect (zero centerline separation), preventing
    /// well-defined clearance normal extraction.
    IntersectingSegments {
        /// First curve index.
        curve_a: usize,
        /// First segment index.
        seg_a: usize,
        /// Second curve index.
        curve_b: usize,
        /// Second segment index.
        seg_b: usize,
    },
    /// Relaxation could not resolve overlap without violating topology invariants.
    TopologyViolationDuringRelaxation {
        /// Iteration index where refusal occurred.
        iteration: usize,
    },
}

impl fmt::Display for KernelErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for KernelErr {}

// ---------------------------------------------------------------------------
// Deterministic vector math (plain `*` and `+`, zero `mul_add`).
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
pub(crate) fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

#[inline]
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
pub(crate) fn norm<M: DetMath>(a: [f64; 3]) -> f64 {
    M::sqrt(dot(a, a))
}

#[inline]
pub(crate) fn rotate_z<M: DetMath>(v: [f64; 3], angle_rad: f64) -> [f64; 3] {
    if angle_rad == 0.0 {
        return v;
    }
    let c = M::cos(angle_rad);
    let s = M::sin(angle_rad);
    [c * v[0] - s * v[1], s * v[0] + c * v[1], v[2]]
}

#[inline]
pub(crate) fn all_finite3(v: [f64; 3]) -> bool {
    v[0].is_finite() && v[1].is_finite() && v[2].is_finite()
}

impl Periodicity {
    /// Validate periodicity domain parameters.
    pub fn validate(&self) -> Result<(), KernelErr> {
        match *self {
            Self::Bounded => Ok(()),
            Self::Helical1D { pitch_m, twist_rad } => {
                if !pitch_m.is_finite() || !twist_rad.is_finite() {
                    return Err(KernelErr::NonFiniteInput {
                        context: "Periodicity::Helical1D",
                    });
                }
                if pitch_m <= 0.0 {
                    return Err(KernelErr::InvalidPeriodicity {
                        detail: "Helical1D pitch_m must be strictly positive",
                    });
                }
                Ok(())
            }
            Self::Planar2D { u_vec, v_vec } => {
                if !all_finite3(u_vec) || !all_finite3(v_vec) {
                    return Err(KernelErr::NonFiniteInput {
                        context: "Periodicity::Planar2D",
                    });
                }
                let area_sq = dot(cross(u_vec, v_vec), cross(u_vec, v_vec));
                if area_sq <= 1e-24 {
                    return Err(KernelErr::InvalidPeriodicity {
                        detail: "Planar2D basis vectors must be linearly independent",
                    });
                }
                Ok(())
            }
            Self::TubularHelical2D {
                radius_m,
                pitch_m,
                circumferential_count,
            } => {
                if !radius_m.is_finite() || !pitch_m.is_finite() {
                    return Err(KernelErr::NonFiniteInput {
                        context: "Periodicity::TubularHelical2D",
                    });
                }
                if radius_m <= 0.0 || pitch_m <= 0.0 || circumferential_count == 0 {
                    return Err(KernelErr::InvalidPeriodicity {
                        detail: "TubularHelical2D requires positive radius_m, pitch_m, and count >= 1",
                    });
                }
                Ok(())
            }
            Self::Lattice3D { basis } => {
                if !all_finite3(basis[0]) || !all_finite3(basis[1]) || !all_finite3(basis[2]) {
                    return Err(KernelErr::NonFiniteInput {
                        context: "Periodicity::Lattice3D",
                    });
                }
                let det = dot(basis[0], cross(basis[1], basis[2])).abs();
                if det <= 1e-24 {
                    return Err(KernelErr::InvalidPeriodicity {
                        detail: "Lattice3D basis vectors must span nonzero volume",
                    });
                }
                Ok(())
            }
        }
    }

    /// Check whether a discrete integer shift `[i, j, k]` is valid for this periodicity.
    pub fn validate_shift(&self, shift: [i32; 3]) -> Result<(), KernelErr> {
        let ok = match *self {
            Self::Bounded => shift == [0, 0, 0],
            Self::Helical1D { .. } => shift[1] == 0 && shift[2] == 0,
            Self::Planar2D { .. } | Self::TubularHelical2D { .. } => shift[2] == 0,
            Self::Lattice3D { .. } => true,
        };
        if ok {
            Ok(())
        } else {
            Err(KernelErr::InvalidPeriodicity {
                detail: "periodic shift uses an inactive axis for this Periodicity",
            })
        }
    }

    /// Map a point `p` in the base cell to periodic image cell `shift`.
    #[must_use]
    pub fn apply_image_point<M: DetMath>(&self, p: [f64; 3], shift: [i32; 3]) -> [f64; 3] {
        if shift == [0, 0, 0] {
            return p;
        }
        match *self {
            Self::Bounded => p,
            Self::Helical1D { pitch_m, twist_rad } => {
                let k = f64::from(shift[0]);
                let rot = rotate_z::<M>(p, k * twist_rad);
                [rot[0], rot[1], rot[2] + k * pitch_m]
            }
            Self::Planar2D { u_vec, v_vec } => {
                let du = scale(u_vec, f64::from(shift[0]));
                let dv = scale(v_vec, f64::from(shift[1]));
                add(add(p, du), dv)
            }
            Self::TubularHelical2D {
                pitch_m,
                circumferential_count,
                ..
            } => {
                let kz = f64::from(shift[0]);
                let kc = f64::from(shift[1]);
                let dtheta = (2.0 * core::f64::consts::PI) / f64::from(circumferential_count);
                let rot = rotate_z::<M>(p, kc * dtheta);
                [rot[0], rot[1], rot[2] + kz * pitch_m]
            }
            Self::Lattice3D { basis } => {
                let d0 = scale(basis[0], f64::from(shift[0]));
                let d1 = scale(basis[1], f64::from(shift[1]));
                let d2 = scale(basis[2], f64::from(shift[2]));
                add(add(add(p, d0), d1), d2)
            }
        }
    }

    /// Push a tangent/frame vector `v` from the base cell to periodic image cell `shift`.
    #[must_use]
    pub fn apply_image_vector<M: DetMath>(&self, v: [f64; 3], shift: [i32; 3]) -> [f64; 3] {
        if shift == [0, 0, 0] {
            return v;
        }
        match *self {
            Self::Bounded | Self::Planar2D { .. } | Self::Lattice3D { .. } => v,
            Self::Helical1D { twist_rad, .. } => rotate_z::<M>(v, f64::from(shift[0]) * twist_rad),
            Self::TubularHelical2D {
                circumferential_count,
                ..
            } => {
                let dtheta = (2.0 * core::f64::consts::PI) / f64::from(circumferential_count);
                rotate_z::<M>(v, f64::from(shift[1]) * dtheta)
            }
        }
    }

    /// Pull a displacement vector `v` from periodic image cell `shift` back to the base cell.
    #[must_use]
    pub fn pull_back_vector<M: DetMath>(&self, v: [f64; 3], shift: [i32; 3]) -> [f64; 3] {
        self.apply_image_vector::<M>(v, [-shift[0], -shift[1], -shift[2]])
    }

    /// Deterministic sorted list of first-shell neighbor image shifts (including `[0, 0, 0]`).
    #[must_use]
    pub fn neighbor_shifts(&self) -> Vec<[i32; 3]> {
        let mut shifts = Vec::new();
        match *self {
            Self::Bounded => shifts.push([0, 0, 0]),
            Self::Helical1D { .. } => {
                for i in -1..=1 {
                    shifts.push([i, 0, 0]);
                }
            }
            Self::Planar2D { .. } => {
                for i in -1..=1 {
                    for j in -1..=1 {
                        shifts.push([i, j, 0]);
                    }
                }
            }
            Self::TubularHelical2D {
                circumferential_count,
                ..
            } => {
                let n = i32::try_from(circumferential_count).unwrap_or(1).max(1);
                let lo = -(n / 2);
                let hi = (n - 1) / 2;
                for kz in -1..=1 {
                    for kc in lo..=hi {
                        shifts.push([kz, kc, 0]);
                    }
                }
            }
            Self::Lattice3D { .. } => {
                for i in -1..=1 {
                    for j in -1..=1 {
                        for k in -1..=1 {
                            shifts.push([i, j, k]);
                        }
                    }
                }
            }
        }
        shifts.sort_unstable();
        shifts.dedup();
        shifts
    }
}

impl FilamentComplex {
    /// Number of 3D nodes stored in `positions_m`.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.positions_m.len() / 3
    }

    /// Total number of directed segments summed across all curves.
    #[must_use]
    pub fn total_segment_count(&self) -> usize {
        self.curves.iter().map(FilamentCurve::segment_count).sum()
    }

    /// Read node `idx` position `[x, y, z]` in metres.
    #[must_use]
    pub fn node_pos(&self, idx: usize) -> [f64; 3] {
        let base = 3 * idx;
        [
            self.positions_m[base],
            self.positions_m[base + 1],
            self.positions_m[base + 2],
        ]
    }

    /// Validate structural integrity of the complex prior to numerical evaluation.
    pub fn validate(&self) -> Result<(), KernelErr> {
        if !self.positions_m.len().is_multiple_of(3) {
            return Err(KernelErr::InvalidPositionBuffer {
                len: self.positions_m.len(),
            });
        }
        if !self.positions_m.iter().all(|v| v.is_finite()) {
            return Err(KernelErr::NonFiniteInput {
                context: "FilamentComplex::positions_m",
            });
        }
        self.periodicity.validate()?;
        let n_nodes = self.node_count();
        for (c_idx, curve) in self.curves.iter().enumerate() {
            if !curve.radius_m.is_finite() {
                return Err(KernelErr::NonFiniteInput {
                    context: "FilamentCurve::radius_m",
                });
            }
            if curve.radius_m < 0.0 {
                return Err(KernelErr::InvalidCurve {
                    curve_idx: c_idx,
                    detail: "radius_m must be non-negative",
                });
            }
            self.periodicity.validate_shift(curve.periodic_shift)?;
            if !curve.closed && curve.periodic_shift != [0, 0, 0] {
                return Err(KernelErr::InvalidCurve {
                    curve_idx: c_idx,
                    detail: "open curves must carry zero periodic_shift",
                });
            }
            let min_nodes = if !curve.closed {
                2
            } else if curve.periodic_shift == [0, 0, 0] {
                3
            } else {
                1
            };
            if curve.nodes.len() < min_nodes {
                return Err(KernelErr::InvalidCurve {
                    curve_idx: c_idx,
                    detail: "curve has fewer nodes than topologically required",
                });
            }
            for &node in &curve.nodes {
                if node >= n_nodes {
                    return Err(KernelErr::NodeIndexOutOfRange {
                        curve_idx: c_idx,
                        node,
                        node_count: n_nodes,
                    });
                }
            }
            let seg_count = curve.segment_count();
            if !curve.segment_twist_rad.is_empty() && curve.segment_twist_rad.len() != seg_count {
                return Err(KernelErr::InvalidCurve {
                    curve_idx: c_idx,
                    detail: "segment_twist_rad length must be 0 or match segment_count",
                });
            }
            if !curve.segment_twist_rad.iter().all(|t| t.is_finite()) {
                return Err(KernelErr::NonFiniteInput {
                    context: "FilamentCurve::segment_twist_rad",
                });
            }
        }
        for (x_idx, spec) in self.crossings.iter().enumerate() {
            if spec.over_curve >= self.curves.len() || spec.under_curve >= self.curves.len() {
                return Err(KernelErr::InvalidCrossingSpec {
                    crossing_idx: x_idx,
                    detail: "curve index out of range",
                });
            }
            if spec.over_segment >= self.curves[spec.over_curve].segment_count()
                || spec.under_segment >= self.curves[spec.under_curve].segment_count()
            {
                return Err(KernelErr::InvalidCrossingSpec {
                    crossing_idx: x_idx,
                    detail: "segment index out of range",
                });
            }
            self.periodicity.validate_shift(spec.under_image_shift)?;
            if !all_finite3(spec.view_dir) || dot(spec.view_dir, spec.view_dir) <= 1e-24 {
                return Err(KernelErr::InvalidCrossingSpec {
                    crossing_idx: x_idx,
                    detail: "view_dir must be finite and non-zero",
                });
            }
        }
        Ok(())
    }

    /// Return the start and end 3D points of segment `seg_idx` on `curve_idx`
    /// in the base cell (applying `curve.periodic_shift` on the closing segment).
    #[must_use]
    pub fn segment_endpoints<M: DetMath>(
        &self,
        curve_idx: usize,
        seg_idx: usize,
    ) -> ([f64; 3], [f64; 3]) {
        let curve = &self.curves[curve_idx];
        let p0 = self.node_pos(curve.nodes[seg_idx]);
        let next_idx = seg_idx + 1;
        let p1 = if next_idx < curve.nodes.len() {
            self.node_pos(curve.nodes[next_idx])
        } else {
            self.periodicity
                .apply_image_point::<M>(self.node_pos(curve.nodes[0]), curve.periodic_shift)
        };
        (p0, p1)
    }
}

/// Compute parallel-transport Bishop frames using [`FtxDetMath`].
pub fn compute_bishop_frames_det(complex: &FilamentComplex) -> Result<BishopFrames, KernelErr> {
    compute_bishop_frames::<FtxDetMath>(complex)
}

/// Certify topological invariants of `complex` using [`FtxDetMath`].
pub fn certify_topology_det(complex: &FilamentComplex) -> Result<TopologyCertificate, KernelErr> {
    certify_topology::<FtxDetMath>(complex)
}

/// Relax overlapping filament segments using [`FtxDetMath`].
pub fn relax_collision_free_embedding_det(
    complex: &mut FilamentComplex,
    target_clearance_m: f64,
    max_iters: usize,
) -> Result<TopologyCertificate, KernelErr> {
    relax_collision_free_embedding::<FtxDetMath>(complex, target_clearance_m, max_iters)
}

// ---------------------------------------------------------------------------
// Self-contained deterministic hashing (FNV-1a 64 + FIPS 180-4 SHA-256).
// Zero external crate dependencies inside src/kernel/.
// ---------------------------------------------------------------------------

/// Streaming FNV-1a 64-bit hasher for kernel golden vectors.
#[derive(Clone, Debug)]
pub(crate) struct Fnv1a64(u64);

impl Fnv1a64 {
    pub(crate) fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub(crate) fn feed_f64(&mut self, x: f64) {
        let normalized = if x == 0.0 { 0.0 } else { x };
        self.update(&normalized.to_bits().to_le_bytes());
    }

    pub(crate) fn finish(self) -> u64 {
        self.0
    }
}

/// Pure safe-Rust FIPS 180-4 SHA-256 implementation for self-contained kernel
/// golden digests.
#[derive(Clone, Debug)]
pub(crate) struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_bits: u64,
}

impl Sha256 {
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    pub(crate) fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_bits: 0,
        }
    }

    fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().enumerate().take(16) {
            let j = 4 * i;
            *word = u32::from_be_bytes([block[j], block[j + 1], block[j + 2], block[j + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for (&k_i, &w_i) in Self::K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k_i)
                .wrapping_add(w_i);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        self.total_bits = self.total_bits.wrapping_add((bytes.len() as u64) * 8);
        let mut idx = 0;
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(bytes.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&bytes[..take]);
            self.buf_len += take;
            idx += take;
            if self.buf_len == 64 {
                let block = self.buf;
                Self::compress(&mut self.state, &block);
                self.buf_len = 0;
            }
        }
        while idx + 64 <= bytes.len() {
            let mut block = [0u8; 64];
            block.copy_from_slice(&bytes[idx..idx + 64]);
            Self::compress(&mut self.state, &block);
            idx += 64;
        }
        let rem = bytes.len() - idx;
        if rem > 0 {
            self.buf[..rem].copy_from_slice(&bytes[idx..]);
            self.buf_len = rem;
        }
    }

    pub(crate) fn feed_f64(&mut self, x: f64) {
        let normalized = if x == 0.0 { 0.0 } else { x };
        self.update(&normalized.to_bits().to_le_bytes());
    }

    pub(crate) fn finalize(mut self) -> [u8; 32] {
        let total_bits = self.total_bits;
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            for b in &mut self.buf[self.buf_len..64] {
                *b = 0;
            }
            let block = self.buf;
            Self::compress(&mut self.state, &block);
            self.buf = [0u8; 64];
            self.buf_len = 0;
        } else {
            for b in &mut self.buf[self.buf_len..56] {
                *b = 0;
            }
        }
        self.buf[56..64].copy_from_slice(&total_bits.to_be_bytes());
        let block = self.buf;
        Self::compress(&mut self.state, &block);
        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

pub mod bishop {
    //! Parallel-transport Bishop frames, Bergou discrete curvature binormals, and
    //! exact periodic boundary twist closure.

    use super::{DetMath, FilamentComplex, KernelErr, add, cross, dot, norm, scale, sub};

    /// Parallel-transport Bishop frames and curvature binormals for a [`FilamentComplex`].
    #[derive(Clone, Debug, PartialEq)]
    pub struct BishopFrames {
        /// Unit segment tangents `t_e` concatenated across all curves.
        pub tangents: Vec<[f64; 3]>,
        /// Pure parallel-transport Bishop normals `u_e` (zero interior twist).
        pub normals_u: Vec<[f64; 3]>,
        /// Pure parallel-transport binormals `v_e = t_e x u_e`.
        pub binormals_v: Vec<[f64; 3]>,
        /// Periodic twist-continuous normals (distributing closing seam holonomy
        /// proportionally to segment arc length so closed/periodic curves close
        /// seamlessly across `periodic_shift`; identical to `normals_u` on open curves).
        pub periodic_normals_u: Vec<[f64; 3]>,
        /// Periodic twist-continuous binormals `t_e x periodic_normals_u[e]`.
        pub periodic_binormals_v: Vec<[f64; 3]>,
        /// Discrete curvature binormals `2 (t_prev x t_next) / (1 + t_prev . t_next)`
        /// at every joint (including the closing/periodic joint of closed curves).
        pub curvature_binormals: Vec<[f64; 3]>,
        /// Exact parallel-transport holonomy angle (radians in `(-pi, pi]`) across
        /// the closing/periodic boundary of each curve (`0.0` for open curves).
        pub curve_holonomy_rad: Vec<f64>,
        /// Starting segment offset into `tangents`/`normals_u` for each curve.
        pub curve_segment_offsets: Vec<usize>,
    }

    /// Choose a deterministic initial unit normal orthogonal to unit tangent `t`.
    fn initial_normal<M: DetMath>(t: [f64; 3]) -> [f64; 3] {
        let ax = t[0].abs();
        let ay = t[1].abs();
        let az = t[2].abs();
        let ref_axis = if ax <= ay && ax <= az {
            [1.0, 0.0, 0.0]
        } else if ay <= az {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        let proj = sub(ref_axis, scale(t, dot(ref_axis, t)));
        let len = norm::<M>(proj);
        scale(proj, 1.0 / len)
    }

    /// Exact singularity-free minimum-rotation parallel transport of unit normal `u`
    /// (orthogonal to `t0`) from `t0` to `t1`.
    fn parallel_transport<M: DetMath>(
        u: [f64; 3],
        t0: [f64; 3],
        t1: [f64; 3],
        curve_idx: usize,
        joint_idx: usize,
    ) -> Result<[f64; 3], KernelErr> {
        let c = dot(t0, t1);
        if c <= -1.0 + 1e-12 {
            return Err(KernelErr::AntiparallelTangents {
                curve_idx,
                joint_idx,
            });
        }
        let denom = 1.0 + c;
        let coeff = dot(u, t1) / denom;
        let raw = sub(u, scale(add(t0, t1), coeff));
        // Re-orthogonalize against t1 and re-normalize to eliminate cumulative drift.
        let ortho = sub(raw, scale(t1, dot(raw, t1)));
        let len = norm::<M>(ortho);
        if len <= 1e-15 {
            return Err(KernelErr::AntiparallelTangents {
                curve_idx,
                joint_idx,
            });
        }
        Ok(scale(ortho, 1.0 / len))
    }

    /// Rotate unit vector `u` (orthogonal to unit axis `t`) by `angle_rad` around `t`.
    fn rotate_about_tangent<M: DetMath>(u: [f64; 3], t: [f64; 3], angle_rad: f64) -> [f64; 3] {
        if angle_rad == 0.0 {
            return u;
        }
        let c = M::cos(angle_rad);
        let s = M::sin(angle_rad);
        let v = cross(t, u);
        let raw = add(scale(u, c), scale(v, s));
        let ortho = sub(raw, scale(t, dot(raw, t)));
        let len = norm::<M>(ortho);
        scale(ortho, 1.0 / len)
    }

    /// Compute discrete curvature binormal `2 (t0 x t1) / (1 + t0 . t1)`.
    fn curvature_binormal(
        t0: [f64; 3],
        t1: [f64; 3],
        curve_idx: usize,
        joint_idx: usize,
    ) -> Result<[f64; 3], KernelErr> {
        let c = dot(t0, t1);
        if c <= -1.0 + 1e-12 {
            return Err(KernelErr::AntiparallelTangents {
                curve_idx,
                joint_idx,
            });
        }
        Ok(scale(cross(t0, t1), 2.0 / (1.0 + c)))
    }

    /// Compute parallel-transport Bishop frames, discrete curvature binormals, and
    /// exact periodic boundary twist closure for every curve in `complex`.
    pub fn compute_bishop_frames<M: DetMath>(
        complex: &FilamentComplex,
    ) -> Result<BishopFrames, KernelErr> {
        complex.validate()?;

        let total_segs = complex.total_segment_count();
        let mut tangents = Vec::with_capacity(total_segs);
        let mut normals_u = Vec::with_capacity(total_segs);
        let mut binormals_v = Vec::with_capacity(total_segs);
        let mut periodic_normals_u = Vec::with_capacity(total_segs);
        let mut periodic_binormals_v = Vec::with_capacity(total_segs);
        let mut curvature_binormals = Vec::with_capacity(total_segs);
        let mut curve_holonomy_rad = Vec::with_capacity(complex.curves.len());
        let mut curve_segment_offsets = Vec::with_capacity(complex.curves.len());

        for (c_idx, curve) in complex.curves.iter().enumerate() {
            let seg_count = curve.segment_count();
            curve_segment_offsets.push(tangents.len());

            let mut local_t = Vec::with_capacity(seg_count);
            let mut local_len = Vec::with_capacity(seg_count);
            let mut total_arc = 0.0;

            for s_idx in 0..seg_count {
                let (p0, p1) = complex.segment_endpoints::<M>(c_idx, s_idx);
                let e = sub(p1, p0);
                let l = norm::<M>(e);
                if l <= 1e-15 {
                    return Err(KernelErr::DegenerateSegment {
                        curve_idx: c_idx,
                        segment_idx: s_idx,
                    });
                }
                local_t.push(scale(e, 1.0 / l));
                local_len.push(l);
                total_arc += l;
            }

            let mut local_u = Vec::with_capacity(seg_count);
            let u0 = initial_normal::<M>(local_t[0]);
            local_u.push(u0);

            for s_idx in 1..seg_count {
                let kb = curvature_binormal(local_t[s_idx - 1], local_t[s_idx], c_idx, s_idx)?;
                curvature_binormals.push(kb);
                let next_u = parallel_transport::<M>(
                    local_u[s_idx - 1],
                    local_t[s_idx - 1],
                    local_t[s_idx],
                    c_idx,
                    s_idx,
                )?;
                local_u.push(next_u);
            }

            let holonomy = if curve.closed {
                let t0_img = complex
                    .periodicity
                    .apply_image_vector::<M>(local_t[0], curve.periodic_shift);
                let u0_img = complex
                    .periodicity
                    .apply_image_vector::<M>(local_u[0], curve.periodic_shift);
                let kb_close =
                    curvature_binormal(local_t[seg_count - 1], t0_img, c_idx, seg_count)?;
                curvature_binormals.push(kb_close);
                let u_return = parallel_transport::<M>(
                    local_u[seg_count - 1],
                    local_t[seg_count - 1],
                    t0_img,
                    c_idx,
                    seg_count,
                )?;
                let sin_psi = dot(cross(u_return, u0_img), t0_img);
                let cos_psi = dot(u_return, u0_img);
                M::atan2(sin_psi, cos_psi)
            } else {
                0.0
            };
            curve_holonomy_rad.push(holonomy);

            // Build twist-continuous periodic frames by distributing `holonomy`
            // proportionally to cumulative arc length from the start of segment 0.
            let mut cum_arc = 0.0;
            for s_idx in 0..seg_count {
                let t = local_t[s_idx];
                let u = local_u[s_idx];
                let v = cross(t, u);
                let twist_closure = if curve.closed && total_arc > 0.0 {
                    holonomy * (cum_arc / total_arc)
                } else {
                    0.0
                };
                let pu = rotate_about_tangent::<M>(u, t, twist_closure);
                let pv = cross(t, pu);
                tangents.push(t);
                normals_u.push(u);
                binormals_v.push(v);
                periodic_normals_u.push(pu);
                periodic_binormals_v.push(pv);
                cum_arc += local_len[s_idx];
            }
        }

        Ok(BishopFrames {
            tangents,
            normals_u,
            binormals_v,
            periodic_normals_u,
            periodic_binormals_v,
            curvature_binormals,
            curve_holonomy_rad,
            curve_segment_offsets,
        })
    }
}

pub mod linking {
    //! Exact piecewise-linear discrete Gauss linking integral (Van Oosterom–Strackee
    //! solid-angle formula), writhe, exact segment-segment clearance across periodic
    //! images, and prescribed crossing-sign verification.

    use super::{
        CrossingSign, CrossingSpec, DetMath, FilamentComplex, KernelErr, TopologyCertificate, add,
        cross, dot, norm, scale, sub,
    };

    /// Result of an exact segment-segment closest-point query.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct SegmentClosest {
        /// Closest-point parameter on segment 1 (`s in [0, 1]`).
        pub s: f64,
        /// Closest-point parameter on segment 2 (`t in [0, 1]`).
        pub t: f64,
        /// Closest point on segment 1 in metres.
        pub point_a: [f64; 3],
        /// Closest point on segment 2 in metres.
        pub point_b: [f64; 3],
        /// Euclidean distance between `point_a` and `point_b` in metres.
        pub distance_m: f64,
    }

    /// Exact closest points and Euclidean distance between line segments `[a0, a1]`
    /// and `[b0, b1]` (Ericson §5.1.9, pure IEEE arithmetic, zero `mul_add`).
    #[must_use]
    pub fn segment_segment_closest<M: DetMath>(
        a0: [f64; 3],
        a1: [f64; 3],
        b0: [f64; 3],
        b1: [f64; 3],
    ) -> SegmentClosest {
        let d1 = sub(a1, a0);
        let d2 = sub(b1, b0);
        let r = sub(a0, b0);
        let a = dot(d1, d1);
        let e = dot(d2, d2);
        let f = dot(d2, r);

        let (s, t) = if a <= 1e-30 && e <= 1e-30 {
            (0.0, 0.0)
        } else if a <= 1e-30 {
            (0.0, (f / e).clamp(0.0, 1.0))
        } else {
            let c = dot(d1, r);
            if e <= 1e-30 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else {
                let b = dot(d1, d2);
                let denom = a * e - b * b;
                let mut s_val = if denom > 1e-30 * a * e {
                    ((b * f - c * e) / denom).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let t_nom = b * s_val + f;
                let t_val = if t_nom < 0.0 {
                    s_val = (-c / a).clamp(0.0, 1.0);
                    0.0
                } else if t_nom > e {
                    s_val = ((b - c) / a).clamp(0.0, 1.0);
                    1.0
                } else {
                    t_nom / e
                };
                (s_val, t_val)
            }
        };

        let point_a = add(a0, scale(d1, s));
        let point_b = add(b0, scale(d2, t));
        let diff = sub(point_a, point_b);
        let distance_m = norm::<M>(diff);
        SegmentClosest {
            s,
            t,
            point_a,
            point_b,
            distance_m,
        }
    }

    /// Exact signed solid angle of the spherical triangle `(u, v, w)` on `S^2`
    /// via the Van Oosterom–Strackee (1983) formula.
    #[inline]
    fn spherical_triangle_solid_angle<M: DetMath>(u: [f64; 3], v: [f64; 3], w: [f64; 3]) -> f64 {
        let lu = norm::<M>(u);
        let lv = norm::<M>(v);
        let lw = norm::<M>(w);
        if lu <= 1e-15 || lv <= 1e-15 || lw <= 1e-15 {
            return 0.0;
        }
        let num = dot(u, cross(v, w));
        let den = lu * lv * lw + dot(u, v) * lw + dot(v, w) * lu + dot(w, u) * lv;
        2.0 * M::atan2(num, den)
    }

    /// Exact signed Gauss solid angle `Omega(S_a, S_b)` contributed by directed
    /// segments `S_a = [a0, a1]` and `S_b = [b0, b1]` to the discrete Gauss linking
    /// integral `Lk = sum(Omega) / (4*pi)`.
    ///
    /// Analytically exact on piecewise-linear curves (zero quadrature error).
    #[must_use]
    pub fn segment_pair_gauss_solid_angle<M: DetMath>(
        a0: [f64; 3],
        a1: [f64; 3],
        b0: [f64; 3],
        b1: [f64; 3],
    ) -> f64 {
        let v00 = sub(a0, b0);
        let v01 = sub(a0, b1);
        let v11 = sub(a1, b1);
        let v10 = sub(a1, b0);
        let omega1 = spherical_triangle_solid_angle::<M>(v00, v01, v11);
        let omega2 = spherical_triangle_solid_angle::<M>(v00, v11, v10);
        omega1 + omega2
    }

    /// Cumulative arc-length table for one curve (length `seg_count + 1`).
    pub(crate) fn curve_arc_lengths<M: DetMath>(
        complex: &FilamentComplex,
        curve_idx: usize,
    ) -> Result<Vec<f64>, KernelErr> {
        let seg_count = complex.curves[curve_idx].segment_count();
        let mut arc = Vec::with_capacity(seg_count + 1);
        arc.push(0.0);
        let mut acc = 0.0;
        for s_idx in 0..seg_count {
            let (p0, p1) = complex.segment_endpoints::<M>(curve_idx, s_idx);
            let l = norm::<M>(sub(p1, p0));
            if l <= 1e-15 {
                return Err(KernelErr::DegenerateSegment {
                    curve_idx,
                    segment_idx: s_idx,
                });
            }
            acc += l;
            arc.push(acc);
        }
        Ok(arc)
    }

    /// Determine whether an intra-curve segment pair `(s1 in [0,0,0], s2 in shift)`
    /// lies inside the topological self-contact exclusion horizon (`shared vertex`
    /// or geodesic rest arc separation `<= pi * (r_a + r_b)` along the same chain).
    ///
    /// Distinct curves (`c1 != c2`) and distinct parallel periodic copies are NEVER
    /// excluded.
    pub(crate) fn is_intra_curve_excluded(
        complex: &FilamentComplex,
        curve_idx: usize,
        s1: usize,
        s2: usize,
        shift: [i32; 3],
        arc: &[f64],
    ) -> bool {
        let curve = &complex.curves[curve_idx];
        let seg_count = curve.segment_count();
        let total_len = arc[seg_count];
        let m1 = 0.5 * (arc[s1] + arc[s1 + 1]);
        let m2 = 0.5 * (arc[s2] + arc[s2 + 1]);
        let horizon = core::f64::consts::PI * (2.0 * curve.radius_m);

        if shift == [0, 0, 0] {
            if s1 == s2 || s1.abs_diff(s2) <= 1 {
                return true;
            }
            if curve.closed && curve.periodic_shift == [0, 0, 0] {
                if (s1 == 0 && s2 + 1 == seg_count) || (s2 == 0 && s1 + 1 == seg_count) {
                    return true;
                }
                let raw = (m1 - m2).abs();
                let geo = raw.min(total_len - raw);
                return geo <= horizon;
            }
            return (m1 - m2).abs() <= horizon;
        }

        if curve.closed && curve.periodic_shift != [0, 0, 0] {
            let ps = curve.periodic_shift;
            if shift == ps {
                if s1 + 1 == seg_count && s2 == 0 {
                    return true;
                }
                let geo = ((m2 + total_len) - m1).abs();
                return geo <= horizon;
            }
            if shift == [-ps[0], -ps[1], -ps[2]] {
                if s1 == 0 && s2 + 1 == seg_count {
                    return true;
                }
                let geo = (m1 - (m2 - total_len)).abs();
                return geo <= horizon;
            }
        }

        false
    }

    /// Evaluate whether a single prescribed [`CrossingSpec`] holds on `complex`.
    pub(crate) fn check_crossing_spec<M: DetMath>(
        complex: &FilamentComplex,
        spec: &CrossingSpec,
    ) -> bool {
        let (a0, a1) = complex.segment_endpoints::<M>(spec.over_curve, spec.over_segment);
        let (b0_raw, b1_raw) = complex.segment_endpoints::<M>(spec.under_curve, spec.under_segment);
        let b0 = complex
            .periodicity
            .apply_image_point::<M>(b0_raw, spec.under_image_shift);
        let b1 = complex
            .periodicity
            .apply_image_point::<M>(b1_raw, spec.under_image_shift);

        let closest = segment_segment_closest::<M>(a0, a1, b0, b1);
        if closest.distance_m <= 1e-12 {
            return false;
        }
        let sep = sub(closest.point_a, closest.point_b);
        if dot(sep, spec.view_dir) <= 0.0 {
            return false;
        }
        let t_over = sub(a1, a0);
        let t_under = sub(b1, b0);
        let signed_vol = dot(cross(t_over, t_under), sep);
        match spec.expected_sign {
            CrossingSign::Positive => signed_vol > 0.0,
            CrossingSign::Negative => signed_vol < 0.0,
        }
    }

    /// Certify the topological invariants of `complex`:
    /// - Pairwise exact discrete Gauss linking numbers for closed bounded curves,
    /// - Writhe for every closed curve,
    /// - Minimum segment-segment clearance across active periodic neighbor images,
    /// - Verification of every prescribed [`CrossingSpec`].
    #[allow(clippy::too_many_lines)]
    pub fn certify_topology<M: DetMath>(
        complex: &FilamentComplex,
    ) -> Result<TopologyCertificate, KernelErr> {
        complex.validate()?;

        let n_curves = complex.curves.len();
        let mut arcs = Vec::with_capacity(n_curves);
        for c_idx in 0..n_curves {
            arcs.push(curve_arc_lengths::<M>(complex, c_idx)?);
        }

        // 1. Pairwise clearance across active periodic neighbor shifts.
        let shifts = complex.periodicity.neighbor_shifts();
        let mut min_clearance_m = f64::INFINITY;
        let mut min_centerline_dist_m = f64::INFINITY;
        let mut any_intersection = false;

        for c1 in 0..n_curves {
            let segs1 = complex.curves[c1].segment_count();
            let r1 = complex.curves[c1].radius_m;
            for s1 in 0..segs1 {
                let (a0, a1) = complex.segment_endpoints::<M>(c1, s1);
                for c2 in c1..n_curves {
                    let segs2 = complex.curves[c2].segment_count();
                    let r2 = complex.curves[c2].radius_m;
                    for s2 in 0..segs2 {
                        if c1 == c2 && s2 < s1 {
                            continue;
                        }
                        let (b0_raw, b1_raw) = complex.segment_endpoints::<M>(c2, s2);
                        for &shift in &shifts {
                            if c1 == c2 && s1 == s2 && shift <= [0, 0, 0] {
                                continue;
                            }
                            if c1 == c2
                                && is_intra_curve_excluded(complex, c1, s1, s2, shift, &arcs[c1])
                            {
                                continue;
                            }
                            let b0 = complex.periodicity.apply_image_point::<M>(b0_raw, shift);
                            let b1 = complex.periodicity.apply_image_point::<M>(b1_raw, shift);
                            let cl = segment_segment_closest::<M>(a0, a1, b0, b1);
                            if cl.distance_m <= 1e-12 {
                                any_intersection = true;
                            }
                            if cl.distance_m < min_centerline_dist_m {
                                min_centerline_dist_m = cl.distance_m;
                            }
                            let clearance = cl.distance_m - (r1 + r2);
                            if clearance < min_clearance_m {
                                min_clearance_m = clearance;
                            }
                        }
                    }
                }
            }
        }

        // 2. Pairwise discrete Gauss linking numbers for pairs of closed loops.
        let mut pairwise_linking = Vec::new();
        let mut pairwise_linking_raw = Vec::new();
        let mut max_linking_residual = 0.0f64;
        let mut linking_isolated = true;
        let four_pi = 4.0 * core::f64::consts::PI;

        for c1 in 0..n_curves {
            let curve1 = &complex.curves[c1];
            if !curve1.closed || curve1.periodic_shift != [0, 0, 0] {
                continue;
            }
            let segs1 = curve1.segment_count();
            for c2 in (c1 + 1)..n_curves {
                let curve2 = &complex.curves[c2];
                if !curve2.closed || curve2.periodic_shift != [0, 0, 0] {
                    continue;
                }
                let segs2 = curve2.segment_count();
                let mut omega_sum = 0.0;
                for s1 in 0..segs1 {
                    let (a0, a1) = complex.segment_endpoints::<M>(c1, s1);
                    for s2 in 0..segs2 {
                        let (b0, b1) = complex.segment_endpoints::<M>(c2, s2);
                        omega_sum += segment_pair_gauss_solid_angle::<M>(a0, a1, b0, b1);
                    }
                }
                let raw_lk = omega_sum / four_pi;
                let rounded = raw_lk.round();
                let residual = (raw_lk - rounded).abs();
                if residual > max_linking_residual {
                    max_linking_residual = residual;
                }
                if residual > 0.25 {
                    linking_isolated = false;
                }
                pairwise_linking.push((curve1.id, curve2.id, rounded as i32));
                pairwise_linking_raw.push((curve1.id, curve2.id, raw_lk));
            }
        }

        // 3. Writhe per closed loop.
        let two_pi = 2.0 * core::f64::consts::PI;
        let mut writhe = Vec::with_capacity(n_curves);
        for c_idx in 0..n_curves {
            let curve = &complex.curves[c_idx];
            if !curve.closed || curve.periodic_shift != [0, 0, 0] {
                writhe.push(0.0);
                continue;
            }
            let segs = curve.segment_count();
            let mut omega_sum = 0.0;
            for s1 in 0..segs {
                let (a0, a1) = complex.segment_endpoints::<M>(c_idx, s1);
                for s2 in (s1 + 1)..segs {
                    // Skip adjacent segments sharing a vertex (coplanar, zero solid angle).
                    if s2 == s1 + 1 || (s1 == 0 && s2 + 1 == segs) {
                        continue;
                    }
                    let (b0, b1) = complex.segment_endpoints::<M>(c_idx, s2);
                    omega_sum += segment_pair_gauss_solid_angle::<M>(a0, a1, b0, b1);
                }
            }
            writhe.push(omega_sum / two_pi);
        }

        // 4. Prescribed CrossingSpec verification.
        let mut failed_crossings = Vec::new();
        for (x_idx, spec) in complex.crossings.iter().enumerate() {
            if !check_crossing_spec::<M>(complex, spec) {
                failed_crossings.push(x_idx);
            }
        }

        let isotopy_valid = !any_intersection && linking_isolated && failed_crossings.is_empty();

        Ok(TopologyCertificate {
            pairwise_linking,
            pairwise_linking_raw,
            max_linking_residual,
            writhe,
            min_clearance_m,
            min_centerline_dist_m,
            crossings_checked: complex.crossings.len(),
            failed_crossings,
            isotopy_valid,
        })
    }
}

pub mod relax {
    //! Deterministic projected collision-free embedding relaxation preserving
    //! periodicity, pairwise linking numbers, and prescribed crossing signs.

    use super::linking::{
        certify_topology, curve_arc_lengths, is_intra_curve_excluded, segment_segment_closest,
    };
    use super::{DetMath, FilamentComplex, KernelErr, TopologyCertificate, norm, scale, sub};

    /// Deterministically project overlapping filament segments apart until every
    /// non-excluded pair (including periodic neighbor images) achieves at least
    /// `target_clearance_m` surface clearance, while strictly preserving crossing
    /// signs and pairwise integer linking numbers.
    #[allow(clippy::too_many_lines)]
    pub fn relax_collision_free_embedding<M: DetMath>(
        complex: &mut FilamentComplex,
        target_clearance_m: f64,
        max_iters: usize,
    ) -> Result<TopologyCertificate, KernelErr> {
        if !target_clearance_m.is_finite() || target_clearance_m < 0.0 {
            return Err(KernelErr::NonFiniteInput {
                context: "target_clearance_m",
            });
        }
        let baseline = certify_topology::<M>(complex)?;
        if !baseline.isotopy_valid {
            return Err(KernelErr::TopologyViolationDuringRelaxation { iteration: 0 });
        }
        if baseline.min_clearance_m >= target_clearance_m - 1e-12 || max_iters == 0 {
            return Ok(baseline);
        }

        let n_curves = complex.curves.len();
        let n_nodes = complex.node_count();
        let shifts = complex.periodicity.neighbor_shifts();

        for iter in 0..max_iters {
            let mut arcs = Vec::with_capacity(n_curves);
            for c_idx in 0..n_curves {
                arcs.push(curve_arc_lengths::<M>(complex, c_idx)?);
            }

            let mut disp = vec![[0.0f64; 3]; n_nodes];
            let mut weight = vec![0.0f64; n_nodes];
            let mut min_centerline_dist = f64::INFINITY;
            let mut max_violation = 0.0f64;

            for c1 in 0..n_curves {
                let curve1 = &complex.curves[c1];
                let segs1 = curve1.segment_count();
                let r1 = curve1.radius_m;
                for s1 in 0..segs1 {
                    let (a0, a1) = complex.segment_endpoints::<M>(c1, s1);
                    let n1_a = curve1.nodes[s1];
                    let n1_b = curve1.nodes[(s1 + 1) % curve1.nodes.len()];
                    let n1_b_shift = if s1 + 1 < curve1.nodes.len() {
                        [0, 0, 0]
                    } else {
                        curve1.periodic_shift
                    };

                    for c2 in c1..n_curves {
                        let curve2 = &complex.curves[c2];
                        let segs2 = curve2.segment_count();
                        let r2 = curve2.radius_m;
                        let required_dist = r1 + r2 + target_clearance_m;
                        for s2 in 0..segs2 {
                            if c1 == c2 && s2 < s1 {
                                continue;
                            }
                            let (b0_raw, b1_raw) = complex.segment_endpoints::<M>(c2, s2);
                            let n2_a = curve2.nodes[s2];
                            let n2_b = curve2.nodes[(s2 + 1) % curve2.nodes.len()];
                            let n2_b_local_shift = if s2 + 1 < curve2.nodes.len() {
                                [0, 0, 0]
                            } else {
                                curve2.periodic_shift
                            };

                            for &shift in &shifts {
                                if c1 == c2 && s1 == s2 && shift <= [0, 0, 0] {
                                    continue;
                                }
                                if c1 == c2
                                    && is_intra_curve_excluded(
                                        complex, c1, s1, s2, shift, &arcs[c1],
                                    )
                                {
                                    continue;
                                }
                                let b0 = complex.periodicity.apply_image_point::<M>(b0_raw, shift);
                                let b1 = complex.periodicity.apply_image_point::<M>(b1_raw, shift);
                                let cl = segment_segment_closest::<M>(a0, a1, b0, b1);
                                if cl.distance_m <= 1e-12 {
                                    return Err(KernelErr::IntersectingSegments {
                                        curve_a: c1,
                                        seg_a: s1,
                                        curve_b: c2,
                                        seg_b: s2,
                                    });
                                }
                                if cl.distance_m < min_centerline_dist {
                                    min_centerline_dist = cl.distance_m;
                                }
                                if cl.distance_m < required_dist {
                                    let overlap = required_dist - cl.distance_m;
                                    if overlap > max_violation {
                                        max_violation = overlap;
                                    }
                                    let normal =
                                        scale(sub(cl.point_a, cl.point_b), 1.0 / cl.distance_m);
                                    // Split correction symmetrically between the two segments.
                                    let push = scale(normal, 0.5 * overlap);
                                    let wa0 = 1.0 - cl.s;
                                    let wa1 = cl.s;
                                    let wb0 = 1.0 - cl.t;
                                    let wb1 = cl.t;

                                    // Segment 1 endpoints (pull back closing endpoint if periodic).
                                    let da0 = scale(push, wa0);
                                    let da1 = complex
                                        .periodicity
                                        .pull_back_vector::<M>(scale(push, wa1), n1_b_shift);
                                    for k in 0..3 {
                                        disp[n1_a][k] += da0[k];
                                        disp[n1_b][k] += da1[k];
                                    }
                                    weight[n1_a] += wa0;
                                    weight[n1_b] += wa1;

                                    // Segment 2 endpoints (pull back through cell shift + closing shift).
                                    let neg_push = scale(push, -1.0);
                                    let db0 = complex
                                        .periodicity
                                        .pull_back_vector::<M>(scale(neg_push, wb0), shift);
                                    let combined_shift = [
                                        shift[0] + n2_b_local_shift[0],
                                        shift[1] + n2_b_local_shift[1],
                                        shift[2] + n2_b_local_shift[2],
                                    ];
                                    let db1 = complex.periodicity.pull_back_vector::<M>(
                                        scale(neg_push, wb1),
                                        combined_shift,
                                    );
                                    for k in 0..3 {
                                        disp[n2_a][k] += db0[k];
                                        disp[n2_b][k] += db1[k];
                                    }
                                    weight[n2_a] += wb0;
                                    weight[n2_b] += wb1;
                                }
                            }
                        }
                    }
                }
            }

            if max_violation <= 1e-12 {
                break;
            }

            // Normalize nodal updates and enforce conservative topological step cap
            // (never move any node by more than 0.25 * min_centerline_dist in one step).
            let step_cap = 0.25 * min_centerline_dist;
            for i in 0..n_nodes {
                if weight[i] > 0.0 {
                    let avg = scale(disp[i], 1.0 / weight[i]);
                    let mag = norm::<M>(avg);
                    disp[i] = if mag > step_cap && mag > 0.0 {
                        scale(avg, step_cap / mag)
                    } else {
                        avg
                    };
                }
            }

            // Backtracking trial step preserving isotopy and pairwise linking numbers.
            let prev_positions = complex.positions_m.clone();
            let mut accepted = false;
            let mut alpha = 1.0;
            for _ in 0..6 {
                for (i, d) in disp.iter().enumerate() {
                    let base = 3 * i;
                    complex.positions_m[base] = prev_positions[base] + alpha * d[0];
                    complex.positions_m[base + 1] = prev_positions[base + 1] + alpha * d[1];
                    complex.positions_m[base + 2] = prev_positions[base + 2] + alpha * d[2];
                }
                if let Ok(trial) = certify_topology::<M>(complex)
                    && trial.isotopy_valid
                    && trial.pairwise_linking == baseline.pairwise_linking
                {
                    accepted = true;
                    break;
                }
                alpha *= 0.5;
            }

            if !accepted {
                complex.positions_m = prev_positions;
                return Err(KernelErr::TopologyViolationDuringRelaxation { iteration: iter });
            }
        }

        certify_topology::<M>(complex)
    }
}

pub mod golden_vectors {
    //! Deterministic golden test vector for `lifted_tangle` (`fs-tangle::kernel`).
    //!
    //! Computes both a canonical SHA-256 digest and an FNV-1a 64-bit digest over a
    //! fixed multi-curve complex (Hopf link + periodic planar crossing relaxation)
    //! parameterized solely by `M: DetMath`.

    use super::{
        CrossingSign, CrossingSpec, DetMath, FilamentComplex, FilamentCurve, Fnv1a64, Periodicity,
        Sha256, certify_topology, compute_bishop_frames, relax_collision_free_embedding,
    };

    /// Pinned FNV-1a 64-bit golden digest for [`compute_golden_digests`].
    pub const TANGLE_KERNEL_GOLDEN_FNV64: u64 = 0xd91e_8c64_cb58_7c59;

    /// Pinned SHA-256 golden digest for [`compute_golden_digests`].
    pub const TANGLE_KERNEL_GOLDEN_SHA256: [u8; 32] = [
        0x31, 0x24, 0xc9, 0x56, 0x97, 0x88, 0x81, 0x75, 0xca, 0xcf, 0xf6, 0x7d, 0x41, 0x86, 0x1d,
        0xaf, 0xbd, 0x41, 0xe2, 0x68, 0x34, 0xb4, 0x68, 0xef, 0x67, 0x38, 0x98, 0x5d, 0xb4, 0xd9,
        0xb5, 0xf6,
    ];

    /// Build the canonical Hopf-link complex (2 interlocking circles of 16 segments).
    #[must_use]
    pub fn build_canonical_hopf_link<M: DetMath>() -> FilamentComplex {
        let n = 16usize;
        let r_major = 0.05;
        let mut positions = Vec::with_capacity(2 * n * 3);
        let mut nodes_a = Vec::with_capacity(n);
        let mut nodes_b = Vec::with_capacity(n);

        // Ring A in the xy-plane centered at (0, 0, 0).
        for i in 0..n {
            let theta = (2.0 * core::f64::consts::PI * (i as f64)) / (n as f64);
            nodes_a.push(i);
            positions.push(r_major * M::cos(theta));
            positions.push(r_major * M::sin(theta));
            positions.push(0.0);
        }
        // Ring B in the xz-plane centered at (r_major, 0, 0), passing through Ring A's interior.
        for i in 0..n {
            let theta = (2.0 * core::f64::consts::PI * (i as f64)) / (n as f64);
            nodes_b.push(n + i);
            positions.push(r_major + r_major * M::cos(theta));
            positions.push(0.0);
            positions.push(r_major * M::sin(theta));
        }

        FilamentComplex {
            periodicity: Periodicity::Bounded,
            positions_m: positions,
            curves: vec![
                FilamentCurve {
                    id: 1,
                    nodes: nodes_a,
                    closed: true,
                    periodic_shift: [0, 0, 0],
                    radius_m: 0.004,
                    segment_twist_rad: Vec::new(),
                },
                FilamentCurve {
                    id: 2,
                    nodes: nodes_b,
                    closed: true,
                    periodic_shift: [0, 0, 0],
                    radius_m: 0.004,
                    segment_twist_rad: Vec::new(),
                },
            ],
            crossings: Vec::new(),
        }
    }

    /// Build a periodic planar interlaced tangle (`Planar2D`) with an explicit
    /// crossing specification and tight initial overlap for relaxation testing.
    #[must_use]
    pub fn build_canonical_planar_tangle<M: DetMath>() -> FilamentComplex {
        let pitch_x = 0.02;
        let pitch_y = 0.02;
        // Curve 0 advances along +x (`periodic_shift = [1, 0, 0]`) undulating in z.
        // Curve 1 advances along +y (`periodic_shift = [0, 1, 0]`) undulating in z,
        // crossing under Curve 0 at (0.005, 0.005) and over at (0.015, 0.015).
        let positions = vec![
            // Curve 0 (4 nodes along x at y = 0.005)
            0.000, 0.005, 0.000, 0.005, 0.005, 0.0012, 0.010, 0.005, 0.000, 0.015, 0.005, -0.0012,
            // Curve 1 (4 nodes along y at x = 0.005)
            0.005, 0.000, 0.000, 0.005, 0.005, -0.0012, 0.005, 0.010, 0.000, 0.005, 0.015, 0.0012,
        ];
        let _ = M::sqrt(1.0); // keep generic parameter used cleanly
        FilamentComplex {
            periodicity: Periodicity::Planar2D {
                u_vec: [pitch_x, 0.0, 0.0],
                v_vec: [0.0, pitch_y, 0.0],
            },
            positions_m: positions,
            curves: vec![
                FilamentCurve {
                    id: 10,
                    nodes: vec![0, 1, 2, 3],
                    closed: true,
                    periodic_shift: [1, 0, 0],
                    radius_m: 0.0015,
                    segment_twist_rad: Vec::new(),
                },
                FilamentCurve {
                    id: 20,
                    nodes: vec![4, 5, 6, 7],
                    closed: true,
                    periodic_shift: [0, 1, 0],
                    radius_m: 0.0015,
                    segment_twist_rad: Vec::new(),
                },
            ],
            crossings: vec![CrossingSpec {
                over_curve: 0,
                over_segment: 0,
                under_curve: 1,
                under_segment: 0,
                under_image_shift: [0, 0, 0],
                view_dir: [0.0, 0.0, 1.0],
                expected_sign: CrossingSign::Positive,
            }],
        }
    }

    /// Evaluate the deterministic golden vector suite and return `(fnv64, sha256)`.
    #[must_use]
    pub fn compute_golden_digests<M: DetMath>() -> (u64, [u8; 32]) {
        let hopf = build_canonical_hopf_link::<M>();
        let frames = compute_bishop_frames::<M>(&hopf).expect("canonical hopf bishop frames");
        let cert = certify_topology::<M>(&hopf).expect("canonical hopf certify");

        let mut planar = build_canonical_planar_tangle::<M>();
        let relaxed_cert = relax_collision_free_embedding::<M>(&mut planar, 0.0005, 32)
            .expect("canonical planar relaxation");
        let planar_frames =
            compute_bishop_frames::<M>(&planar).expect("canonical planar bishop frames");

        let mut fnv = Fnv1a64::new();
        let mut sha = Sha256::new();

        let feed_f64 = |f: &mut Fnv1a64, s: &mut Sha256, v: f64| {
            f.feed_f64(v);
            s.feed_f64(v);
        };
        let feed_i32 = |f: &mut Fnv1a64, s: &mut Sha256, v: i32| {
            let bytes = v.to_le_bytes();
            f.update(&bytes);
            s.update(&bytes);
        };

        for v in &frames.normals_u {
            for &c in v {
                feed_f64(&mut fnv, &mut sha, c);
            }
        }
        for v in &frames.periodic_normals_u {
            for &c in v {
                feed_f64(&mut fnv, &mut sha, c);
            }
        }
        for v in &frames.curvature_binormals {
            for &c in v {
                feed_f64(&mut fnv, &mut sha, c);
            }
        }
        for &h in &frames.curve_holonomy_rad {
            feed_f64(&mut fnv, &mut sha, h);
        }
        for &(a, b, lk) in &cert.pairwise_linking {
            feed_i32(&mut fnv, &mut sha, a as i32);
            feed_i32(&mut fnv, &mut sha, b as i32);
            feed_i32(&mut fnv, &mut sha, lk);
        }
        for &w in &cert.writhe {
            feed_f64(&mut fnv, &mut sha, w);
        }
        feed_f64(&mut fnv, &mut sha, cert.min_clearance_m);

        for &p in &planar.positions_m {
            feed_f64(&mut fnv, &mut sha, p);
        }
        for v in &planar_frames.periodic_normals_u {
            for &c in v {
                feed_f64(&mut fnv, &mut sha, c);
            }
        }
        feed_f64(&mut fnv, &mut sha, relaxed_cert.min_clearance_m);
        feed_i32(&mut fnv, &mut sha, i32::from(relaxed_cert.isotopy_valid));

        (fnv.finish(), sha.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::FtxDetMath;
    use super::golden_vectors::{
        TANGLE_KERNEL_GOLDEN_FNV64, TANGLE_KERNEL_GOLDEN_SHA256, compute_golden_digests,
    };

    #[test]
    fn lifted_tangle_golden_digests_match_frankensim() {
        let (fnv64, sha256) = compute_golden_digests::<FtxDetMath>();
        assert_eq!(fnv64, TANGLE_KERNEL_GOLDEN_FNV64);
        assert_eq!(sha256, TANGLE_KERNEL_GOLDEN_SHA256);
    }
}
