#![forbid(unsafe_code)]

//! Pure-Kernel Leaf lifted from `/dp/frankensim/crates/fs-codim/src/kernel/` (`FS-K05` / `FTX-K09`).
//!
//! Provenance:
//! - Source files: `/dp/frankensim/crates/fs-codim/src/kernel/{mod.rs,adjoint.rs,cosserat.rs,der.rs,golden_vectors.rs,junction.rs,section_laws.rs,solver.rs,sparse_ldlt.rs}`
//! - Upstream bead: `FS-K05` (`/dp/frankensim` workspace snapshot `2026-11-27`; note `/dp/frankensim`
//!   has no `.git` directory, post-`frankensim-source-manifest.json` `source_root`
//!   `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`)
//! - Pinned upstream golden digests:
//!   - `CODIM_KERNEL_GOLDEN_FNV64 = 0x14cb_fef4_d2b6_5e84`
//!   - `CODIM_KERNEL_GOLDEN_SHA256 = e0d3f1873c515c9acfb40b33ea4f1b05ae32adf7da72989cd1f62b308603dbe9`
//! - Local adaptations:
//!   - Added [`FtxDetMath`] binding [`DetMath`] to `ftx_core::det::{sin, cos, atan2, sqrt, ln, exp, acos}`.
//!   - Submodule files copied verbatim from `/dp/frankensim/crates/fs-codim/src/kernel/`.
//!
//! Invariants of this leaf module:
//! - `#![forbid(unsafe_code)]`
//! - Zero cross-crate FrankenSim imports (`fs_*`)
//! - All transcendentals routed via [`DetMath`] path syntax (`M::sin`, etc.)
//! - Zero `.mul_add` in hot loops
//! - Zero `HashMap` / `HashSet` iteration
//! - Strict SI units (`m`, `kg`, `s`, `N`, `N*m`, `rad`)

pub mod adjoint;
pub mod cosserat;
pub mod der;
pub mod golden_vectors;
pub mod junction;
pub mod section_laws;
pub mod solver;
pub mod sparse_ldlt;

use core::fmt;

/// Deterministic elementary-function interface decoupling pure kernel math
/// from platform `libm`.
///
/// Methods are associated functions (no `&self`) so call sites use `M::sin`
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

/// Typed fail-closed errors returned by the `fs-codim` Pure-Kernel Leaf.
#[derive(Clone, Debug, PartialEq)]
pub enum CodimError {
    /// A required input scalar, vector, or quaternion contained `NaN` or `±Inf`.
    NonFiniteInput {
        /// Context naming the offending field or operation.
        context: &'static str,
    },
    /// A numeric parameter violated its positivity or physical domain constraint.
    InvalidParameter {
        /// Explanation of the violated constraint.
        detail: &'static str,
    },
    /// Rod network topology, index, or degree-of-freedom count is invalid.
    InvalidTopology {
        /// Explanation of the topology error.
        detail: &'static str,
    },
    /// A rod segment has zero or near-zero length (`<= 1e-15`).
    DegenerateSegment {
        /// Index of the degenerate segment.
        segment_idx: usize,
    },
    /// Two consecutive tangents are nearly antiparallel (`1 + t0 . t1 <= 1e-12`),
    /// making parallel transport and curvature binormal ill-defined.
    AntiparallelTangents {
        /// Index of the offending joint.
        joint_idx: usize,
    },
    /// A [`section_laws::SectionHistory`] variant does not match the evaluated
    /// [`section_laws::RodSectionLaw`].
    HistoryLawMismatch,
    /// Matrix passed to [`sparse_ldlt::SymbolicLdlt`] is not square.
    NotSquare {
        /// Matrix row count.
        nrows: usize,
        /// Matrix column count.
        ncols: usize,
    },
    /// Matrix sparsity pattern is not structurally symmetric: `(row, col)` has
    /// no stored mirror `(col, row)`.
    StructurallyAsymmetric {
        /// Row index of witness entry.
        row: usize,
        /// Column index of witness entry.
        col: usize,
    },
    /// A stored sparse matrix value is `NaN` or infinite.
    NonFiniteEntry {
        /// Row index.
        row: usize,
        /// Column index.
        col: usize,
    },
    /// Matrix sparsity pattern differs from the analyzed [`sparse_ldlt::SymbolicLdlt`].
    PatternMismatch,
    /// Invalid [`sparse_ldlt::LdltOptions`] configuration.
    InvalidSolverOptions {
        /// Name or rule of offending option.
        field: &'static str,
    },
    /// Direct LDLT encountered an unpivotable singular root front; refuses rather
    /// than perturbing the diagonal.
    PivotBreakdown {
        /// Column in original index order.
        orig_col: usize,
        /// Column in elimination order.
        permuted_col: usize,
        /// Best candidate pivot magnitude seen.
        best_abs: f64,
        /// Required threshold magnitude.
        required: f64,
        /// Whether symmetric 2x2 pivoting was enabled.
        two_by_two_enabled: bool,
    },
    /// Dense LU solve encountered a singular pivot.
    SingularLinearSystem {
        /// Row/column index of zero pivot.
        pivot_idx: usize,
    },
    /// Preconditioned conjugate gradient solver failed to reach tolerance.
    PcgDidNotConverge {
        /// Iterations executed.
        iters: usize,
        /// Final relative residual norm.
        rel_residual: f64,
    },
    /// Contact bridge detected initial penetration (`d0 <= 0` or `g <= 0`).
    InitialPenetration {
        /// Observed clearance in metres.
        min_clearance_m: f64,
    },
    /// CCD or line search could not find an admissible step.
    NewtonStalled {
        /// Newton iterations executed.
        iters: u32,
        /// Residual norm at stall.
        residual: f64,
    },
    /// Outer lagged friction loop failed to converge within `max_outer_iters`.
    OuterLagDidNotConverge {
        /// Final infinity-norm friction force delta.
        residual: f64,
        /// Outer rounds executed.
        rounds: u32,
    },
    /// Single-solve IFT adjoint was requested on an equilibrium with
    /// irreversible constitutive history, lagged friction, or non-conservative
    /// follower moments.
    IrreversibleHistoryRequiresCheckpointAdjoint {
        /// Reason the single-solve IFT path cannot be used.
        reason: &'static str,
    },
    /// Contact bridge returned an error.
    ContactBridgeError {
        /// Description from the contact bridge.
        detail: &'static str,
    },
}

impl fmt::Display for CodimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteInput { context } => write!(f, "non-finite input in {context}"),
            Self::InvalidParameter { detail } => write!(f, "invalid parameter: {detail}"),
            Self::InvalidTopology { detail } => write!(f, "invalid topology: {detail}"),
            Self::DegenerateSegment { segment_idx } => {
                write!(f, "degenerate rod segment at index {segment_idx}")
            }
            Self::AntiparallelTangents { joint_idx } => {
                write!(f, "antiparallel consecutive tangents at joint {joint_idx}")
            }
            Self::HistoryLawMismatch => {
                write!(f, "section history variant does not match section law")
            }
            Self::NotSquare { nrows, ncols } => {
                write!(f, "sparse matrix is not square ({nrows}x{ncols})")
            }
            Self::StructurallyAsymmetric { row, col } => write!(
                f,
                "sparse matrix pattern is structurally asymmetric: entry ({row},{col}) has no stored mirror"
            ),
            Self::NonFiniteEntry { row, col } => {
                write!(f, "non-finite sparse matrix entry at ({row},{col})")
            }
            Self::PatternMismatch => {
                write!(f, "sparse matrix pattern differs from symbolic analysis")
            }
            Self::InvalidSolverOptions { field } => {
                write!(f, "invalid sparse solver options: {field}")
            }
            Self::PivotBreakdown {
                orig_col,
                permuted_col,
                best_abs,
                required,
                two_by_two_enabled,
            } => write!(
                f,
                "LDLT pivot breakdown at original col {orig_col} (permuted {permuted_col}); \
                 best |pivot| {best_abs:.3e} < required {required:.3e} (2x2={two_by_two_enabled})"
            ),
            Self::SingularLinearSystem { pivot_idx } => {
                write!(f, "dense linear system singular at pivot {pivot_idx}")
            }
            Self::PcgDidNotConverge {
                iters,
                rel_residual,
            } => write!(
                f,
                "PCG did not converge after {iters} iterations (rel_residual={rel_residual:.3e})"
            ),
            Self::InitialPenetration { min_clearance_m } => write!(
                f,
                "initial contact penetration or zero clearance (min_clearance_m={min_clearance_m})"
            ),
            Self::NewtonStalled { iters, residual } => write!(
                f,
                "Newton solver stalled after {iters} iterations (residual={residual:.3e})"
            ),
            Self::OuterLagDidNotConverge { residual, rounds } => write!(
                f,
                "outer lagged friction did not converge after {rounds} rounds (residual={residual:.3e})"
            ),
            Self::IrreversibleHistoryRequiresCheckpointAdjoint { reason } => write!(
                f,
                "single-solve elastic IFT adjoint refused ({reason}); checkpointed adjoint required"
            ),
            Self::ContactBridgeError { detail } => {
                write!(f, "contact bridge error: {detail}")
            }
        }
    }
}

impl std::error::Error for CodimError {}

// ---------------------------------------------------------------------------
// Deterministic 3D vector, 3x3 matrix, and [w, x, y, z] quaternion math
// (zero libm, zero mul_add).
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
pub(crate) fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
pub(crate) fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

#[inline]
pub(crate) fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
pub(crate) fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
pub(crate) fn norm2_3(a: [f64; 3]) -> f64 {
    dot3(a, a)
}

#[inline]
pub(crate) fn norm3<M: DetMath>(a: [f64; 3]) -> f64 {
    M::sqrt(norm2_3(a))
}

#[inline]
pub(crate) fn all_finite3(a: [f64; 3]) -> bool {
    a[0].is_finite() && a[1].is_finite() && a[2].is_finite()
}

#[inline]
pub(crate) fn all_finite4(q: [f64; 4]) -> bool {
    q[0].is_finite() && q[1].is_finite() && q[2].is_finite() && q[3].is_finite()
}

/// Skew-symmetric matrix `[v]_x` such that `[v]_x w = v x w`.
#[inline]
#[must_use]
pub fn skew3(v: [f64; 3]) -> [[f64; 3]; 3] {
    [[0.0, -v[2], v[1]], [v[2], 0.0, -v[0]], [-v[1], v[0], 0.0]]
}

#[inline]
pub(crate) fn eye3() -> [[f64; 3]; 3] {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}

#[inline]
pub(crate) fn mat3_vec(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [dot3(m[0], v), dot3(m[1], v), dot3(m[2], v)]
}

#[inline]
pub(crate) fn mat3_t_vec(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
    ]
}

#[inline]
pub(crate) fn mat3_mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    out
}

#[inline]
pub(crate) fn mat3_transpose(a: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    [
        [a[0][0], a[1][0], a[2][0]],
        [a[0][1], a[1][1], a[2][1]],
        [a[0][2], a[1][2], a[2][2]],
    ]
}

#[inline]
pub(crate) fn outer3(a: [f64; 3], b: [f64; 3]) -> [[f64; 3]; 3] {
    [
        [a[0] * b[0], a[0] * b[1], a[0] * b[2]],
        [a[1] * b[0], a[1] * b[1], a[1] * b[2]],
        [a[2] * b[0], a[2] * b[1], a[2] * b[2]],
    ]
}

/// Conjugate of a `[w, x, y, z]` quaternion.
#[inline]
#[must_use]
pub fn quat_conj(q: [f64; 4]) -> [f64; 4] {
    [q[0], -q[1], -q[2], -q[3]]
}

/// Hamilton product of two `[w, x, y, z]` quaternions.
#[inline]
#[must_use]
pub fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

/// Normalize a `[w, x, y, z]` quaternion, failing closed on zero/non-finite norm.
pub fn quat_normalize<M: DetMath>(q: [f64; 4]) -> Result<[f64; 4], CodimError> {
    if !all_finite4(q) {
        return Err(CodimError::NonFiniteInput {
            context: "quat_normalize",
        });
    }
    let n2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    let n = M::sqrt(n2);
    if !n.is_finite() || n <= 1e-15 {
        return Err(CodimError::InvalidParameter {
            detail: "quaternion has zero or degenerate norm",
        });
    }
    Ok([q[0] / n, q[1] / n, q[2] / n, q[3] / n])
}

/// Rotate 3D vector `v` by unit `[w, x, y, z]` quaternion `q`: `q * [0, v] * q^*`.
#[inline]
#[must_use]
pub fn quat_rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let qv = [q[1], q[2], q[3]];
    let t = scale3(cross3(qv, v), 2.0);
    add3(v, add3(scale3(t, q[0]), cross3(qv, t)))
}

/// Convert unit `[w, x, y, z]` quaternion `q` to its 3x3 rotation matrix `R(q)`.
#[inline]
#[must_use]
pub fn quat_to_mat3(q: [f64; 4]) -> [[f64; 3]; 3] {
    let [w, x, y, z] = q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// Exponential map `SO(3)`: maps rotation vector `phi` (radians) to unit
/// `[w, x, y, z]` quaternion `exp(phi) = [cos(|phi|/2), sin(|phi|/2) * phi/|phi|]`.
/// Uses a Taylor series branch for `|phi|/2 < 1e-6`.
#[must_use]
pub fn quat_exp<M: DetMath>(phi: [f64; 3]) -> [f64; 4] {
    let half = [0.5 * phi[0], 0.5 * phi[1], 0.5 * phi[2]];
    let theta2 = norm2_3(half);
    let theta = M::sqrt(theta2);
    let (c, sinc) = if theta < 1e-6 {
        let theta4 = theta2 * theta2;
        (
            1.0 - 0.5 * theta2 + theta4 / 24.0,
            1.0 - theta2 / 6.0 + theta4 / 120.0,
        )
    } else {
        (M::cos(theta), M::sin(theta) / theta)
    };
    [c, sinc * half[0], sinc * half[1], sinc * half[2]]
}

/// Right-multiplicative body-frame quaternion update: `q * exp(phi)`.
#[must_use]
pub fn quat_exp_step<M: DetMath>(q: [f64; 4], omega_body: [f64; 3], h: f64) -> [f64; 4] {
    quat_mul(q, quat_exp::<M>(scale3(omega_body, h)))
}

/// Principal rotation-vector logarithm of unit `[w, x, y, z]` quaternion `q`
/// in `[-pi, pi]` using [`DetMath::atan2`] (zero platform `libm`).
#[must_use]
pub fn quat_log<M: DetMath>(q: [f64; 4]) -> [f64; 3] {
    let sign = if q[0] < 0.0 { -1.0 } else { 1.0 };
    let w = (sign * q[0]).clamp(-1.0, 1.0);
    let qv = [sign * q[1], sign * q[2], sign * q[3]];
    let vn = M::sqrt(norm2_3(qv));
    if vn < 1e-14 {
        return [2.0 * qv[0], 2.0 * qv[1], 2.0 * qv[2]];
    }
    let angle = 2.0 * M::atan2(vn, w);
    let s = angle / vn;
    [qv[0] * s, qv[1] * s, qv[2] * s]
}

/// Hemisphere-aligned normalized quaternion mean of two unit `[w, x, y, z]`
/// quaternions `a` and `b`.
#[must_use]
pub fn quat_mid<M: DetMath>(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let s = if dot >= 0.0 { 1.0 } else { -1.0 };
    let m = [
        a[0] + s * b[0],
        a[1] + s * b[1],
        a[2] + s * b[2],
        a[3] + s * b[3],
    ];
    let n = M::sqrt(m[0] * m[0] + m[1] * m[1] + m[2] * m[2] + m[3] * m[3]);
    if n <= 1e-15 {
        return a;
    }
    [m[0] / n, m[1] / n, m[2] / n, m[3] / n]
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
