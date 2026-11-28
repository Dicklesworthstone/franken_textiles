#![forbid(unsafe_code)]

//! Pure-Kernel Leaf lifted from `/dp/frankensim/crates/fs-contact/src/kernel/` (`FS-K04` / `FTX-K09`).
//!
//! Provenance:
//! - Source files: `/dp/frankensim/crates/fs-contact/src/kernel/{mod.rs,ccd.rs,codim_ipc.rs,friction.rs,golden_vectors.rs,periodic_bvh.rs}`
//! - Upstream bead: `FS-K04` (`/dp/frankensim` workspace snapshot `2026-11-27`; note `/dp/frankensim`
//!   has no `.git` directory, post-`frankensim-source-manifest.json` `source_root`
//!   `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`)
//! - Pinned upstream golden digests:
//!   - `CONTACT_KERNEL_GOLDEN_FNV64 = 0x3f46_0900_947c_535f`
//!   - `CONTACT_KERNEL_GOLDEN_SHA256 = c3d777c322d58852c3a290fedadeadf19e50485e90dc3fd86b92dae97f6fd688`
//! - Local adaptations:
//!   - Added [`FtxDetMath`] binding [`DetMath`] to `ftx_core::det::{sin, cos, atan2, sqrt, ln, exp, acos}`.
//!   - Submodule files copied verbatim from `/dp/frankensim/crates/fs-contact/src/kernel/`.
//!
//! Invariants of this leaf module:
//! - `#![forbid(unsafe_code)]`
//! - Zero cross-crate FrankenSim imports
//! - All transcendentals routed via [`DetMath`] path syntax (`M::ln`, etc.)
//! - Zero `mul_add` in hot loops
//! - Zero `HashMap` / `HashSet` iteration ordering
//! - Strict SI units (`m`, `kg`, `s`, `N`, `Pa`, `rad`)

pub mod ccd;
pub mod codim_ipc;
pub mod friction;
pub mod golden_vectors;
pub mod periodic_bvh;

pub use periodic_bvh::PeriodicDomain;

use core::fmt;

/// Deterministic elementary-function interface decoupling pure kernel math
/// from platform `libm`.
///
/// Methods are associated functions (no `&self`) so call sites use `M::ln(x)`
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

/// Typed fail-closed errors returned by the `fs-contact` Pure-Kernel Leaf.
#[derive(Clone, Debug, PartialEq)]
pub enum CodimContactError {
    /// A required input scalar or vector contained `NaN` or `±Inf`.
    NonFiniteInput {
        /// Location or parameter context of the non-finite value.
        context: &'static str,
    },
    /// A numeric parameter violated its positivity or domain constraint.
    InvalidParameter {
        /// Explanation of the violated parameter domain.
        detail: &'static str,
    },
    /// Initial centerline separation is already non-positive (`d0 <= 0`).
    InitialPenetration {
        /// Observed initial clearance in metres.
        min_clearance_m: f64,
    },
    /// A rest edge length is zero or non-finite, making mollification ill-defined.
    DegenerateRestEdge {
        /// Explanation of the degenerate rest configuration.
        detail: &'static str,
    },
    /// Instantaneous primitive geometry is degenerate (e.g., coincident vertices).
    DegenerateGeometry {
        /// Explanation of the degenerate geometry.
        detail: &'static str,
    },
    /// Periodic domain or shift specification is invalid.
    InvalidPeriodicity {
        /// Explanation of the periodicity error.
        detail: &'static str,
    },
    /// Outer lagged friction fixed-point iteration failed to reach `tol_f`.
    OuterLagDidNotConverge {
        /// Final infinity-norm tangential force residual `||Delta lambda_f||_inf`.
        residual: f64,
        /// Number of outer rounds executed.
        rounds: u32,
    },
    /// Constitutive input exceeded the law's declared physical envelope.
    ApplicabilityExceeded {
        /// Explanation of the exceeded applicability bound.
        detail: &'static str,
    },
}

impl fmt::Display for CodimContactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteInput { context } => {
                write!(f, "non-finite input in {context}")
            }
            Self::InvalidParameter { detail } => {
                write!(f, "invalid codimensional contact parameter: {detail}")
            }
            Self::InitialPenetration { min_clearance_m } => {
                write!(
                    f,
                    "initial state is already penetrating or tangent (min_clearance_m={min_clearance_m})"
                )
            }
            Self::DegenerateRestEdge { detail } => {
                write!(f, "degenerate rest edge for mollification: {detail}")
            }
            Self::DegenerateGeometry { detail } => {
                write!(f, "degenerate primitive geometry: {detail}")
            }
            Self::InvalidPeriodicity { detail } => {
                write!(f, "invalid periodic domain specification: {detail}")
            }
            Self::OuterLagDidNotConverge { residual, rounds } => {
                write!(
                    f,
                    "outer lagged friction loop did not converge after {rounds} rounds (residual={residual})"
                )
            }
            Self::ApplicabilityExceeded { detail } => {
                write!(f, "constitutive applicability exceeded: {detail}")
            }
        }
    }
}

impl std::error::Error for CodimContactError {}

// ---------------------------------------------------------------------------
// Deterministic 3D vector & matrix helpers (zero libm, zero mul_add).
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
pub(crate) fn rotate_z3<M: DetMath>(v: [f64; 3], angle_rad: f64) -> [f64; 3] {
    if angle_rad == 0.0 {
        return v;
    }
    let c = M::cos(angle_rad);
    let s = M::sin(angle_rad);
    [c * v[0] - s * v[1], s * v[0] + c * v[1], v[2]]
}

#[inline]
pub(crate) fn mat3_vec(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [dot3(m[0], v), dot3(m[1], v), dot3(m[2], v)]
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
