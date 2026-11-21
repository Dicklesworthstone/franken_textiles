//! Advanced sparse formats, preconditioners, Krylov solvers, and nonlinear globalization
//! primitives lifted from FrankenSim for cloth mechanics.
//!
//! Provenance: adapted from FrankenSim (`frankensim-source-manifest.json` `source_root`
//! `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`; MIT + OpenAI/Anthropic
//! rider):
//! - [`Bsr`] — `crates/fs-sparse/src/bsr.rs` (3×3 and general block CSR, bit-identical SpMV to
//!   [`Csr::spmv`]);
//! - [`Precond`], [`IdentityPrecond`], [`Jacobi`], [`Chebyshev`], [`Ilu0`], [`ilu0`],
//!   [`ilu0_shifted`], [`ilu0_spd_shifted`] — `crates/fs-sparse/src/precond.rs`;
//! - [`CsrF32`], [`MixedReport`], [`mixed_cg_refine`] — `crates/fs-solver/src/mixed.rs`;
//! - [`ResidualClaim`], [`PminresReport`], [`PminresError`], [`pminres`] —
//!   `crates/fs-solver/src/krylov.rs`;
//! - [`rigal_gaches_backward_error`], [`BACKWARD_ERROR_ULPS`] —
//!   `crates/fs-conduction/src/solve.rs`;
//! - [`eisenstat_walker`] — `crates/fs-solver/src/nonlinear.rs`;
//! - [`SteihaugConfig`], [`SteihaugOutcome`], [`steihaug_cg`] —
//!   `crates/fs-ascent/src/trust.rs`.
//!
//! Adaptations for FrankenTextiles:
//! - `#![forbid(unsafe_code)]`, zero platform `libm` (transcendentals via [`ftx_core::det`]);
//! - zero `f64::mul_add` / `f32::mul_add` in hot loops to avoid browser-WASM software-FMA
//!   slowdown and preserve bit-identical accumulation with [`Csr::spmv`];
//! - structured [`Refusal`] / typed error returns instead of panics on runtime solver failures.

use ftx_core::{Refusal, RefusalCode, det};

use super::Csr;

/// Multiple of `f64::EPSILON` at or below which the Rigal–Gaches normwise backward error
/// certifies a linear solve (`η ≤ 64 ε`).
pub const BACKWARD_ERROR_ULPS: f64 = 64.0;

// ---------------------------------------------------------------------------
// Deterministic vector reductions
// ---------------------------------------------------------------------------

#[inline]
fn dot64(a: &[f64], b: &[f64]) -> f64 {
    let mut acc = 0.0f64;
    for (&x, &y) in a.iter().zip(b) {
        acc += x * y;
    }
    acc
}

#[inline]
fn norm2(v: &[f64]) -> f64 {
    dot64(v, v).sqrt()
}

#[inline]
fn inf_norm(v: &[f64]) -> f64 {
    v.iter().fold(
        0.0f64,
        |m, &x| {
            if x.is_nan() { f64::NAN } else { m.max(x.abs()) }
        },
    )
}

/// Normwise backward error (Rigal–Gaches):
/// `η = ‖b − Ax‖∞ / (‖A‖∞ ‖x‖∞ + ‖b‖∞)`.
///
/// Propagates `NaN` rather than absorbing it via `f64::max`.
#[must_use]
pub fn rigal_gaches_backward_error(a: &Csr, b: &[f64], x: &[f64]) -> f64 {
    if a.nrows() != b.len() || a.ncols() != x.len() {
        return f64::NAN;
    }
    let mut ax = vec![0.0f64; b.len()];
    a.spmv(x, &mut ax);
    let r_inf = b
        .iter()
        .zip(&ax)
        .map(|(&bi, &ai)| bi - ai)
        .fold(
            0.0f64,
            |m, v| {
                if v.is_nan() { f64::NAN } else { m.max(v.abs()) }
            },
        );
    let a_inf = (0..a.nrows())
        .map(|r| a.row(r).1.iter().fold(0.0f64, |s, &v| s + v.abs()))
        .fold(0.0f64, |m, v| if v.is_nan() { f64::NAN } else { m.max(v) });
    let x_inf = inf_norm(x);
    let b_inf = inf_norm(b);
    let denom = a_inf * x_inf + b_inf;
    if !r_inf.is_finite() || !denom.is_finite() {
        return f64::NAN;
    }
    if denom > 0.0 { r_inf / denom } else { r_inf }
}

/// Recomputed true Euclidean relative residual `‖b − Ax‖₂ / max(‖b‖₂, f64::MIN_POSITIVE)`.
#[must_use]
pub fn true_relative_residual(a: &Csr, b: &[f64], x: &[f64]) -> f64 {
    if a.nrows() != b.len() || a.ncols() != x.len() {
        return f64::NAN;
    }
    let mut ax = vec![0.0f64; b.len()];
    a.spmv(x, &mut ax);
    let r_norm = b
        .iter()
        .zip(&ax)
        .map(|(&bi, &ai)| {
            let d = bi - ai;
            d * d
        })
        .sum::<f64>()
        .sqrt();
    let b_norm = norm2(b);
    if b_norm == 0.0 {
        r_norm
    } else {
        r_norm / b_norm
    }
}

// ---------------------------------------------------------------------------
// Block CSR (BSR)
// ---------------------------------------------------------------------------

/// Block compressed sparse row with fixed `br × bc` dense micro-blocks (row-major within each
/// block). Block columns are strictly ascending within each block row, so [`Bsr::spmv`] visits
/// scalar entries in exact ascending global column order and matches [`Csr::spmv`] bit-for-bit
/// for finite vectors.
#[derive(Debug, Clone, PartialEq)]
pub struct Bsr {
    br: usize,
    bc: usize,
    nrows: usize,
    ncols: usize,
    brow_ptr: Vec<usize>,
    bcol_idx: Vec<usize>,
    blocks: Vec<f64>,
}

impl Bsr {
    /// Convert from canonical CSR.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] if `br == 0`, `bc == 0`, or matrix dimensions are not
    /// divisible by `(br, bc)`.
    pub fn from_csr(a: &Csr, br: usize, bc: usize) -> Result<Self, Refusal> {
        if br == 0 || bc == 0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "BSR block dimensions must be at least 1x1",
            ));
        }
        if !a.nrows().is_multiple_of(br) || !a.ncols().is_multiple_of(bc) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "matrix {}x{} is not divisible by block {}x{}",
                    a.nrows(),
                    a.ncols(),
                    br,
                    bc
                ),
            ));
        }
        let nbrows = a.nrows() / br;
        let mut brow_ptr = vec![0usize; nbrows + 1];
        let mut bcol_idx = Vec::new();
        let mut blocks = Vec::new();
        for rb in 0..nbrows {
            let mut present: Vec<usize> = Vec::new();
            for i in 0..br {
                let (cols, _) = a.row(rb * br + i);
                for &c in cols {
                    let cb = c / bc;
                    if let Err(pos) = present.binary_search(&cb) {
                        present.insert(pos, cb);
                    }
                }
            }
            let base = blocks.len();
            blocks.resize(base + present.len() * br * bc, 0.0);
            for i in 0..br {
                let (cols, vals) = a.row(rb * br + i);
                for (&c, &v) in cols.iter().zip(vals) {
                    let cb = c / bc;
                    let slot = present.binary_search(&cb).expect("block column inserted");
                    blocks[base + slot * br * bc + i * bc + (c % bc)] = v;
                }
            }
            brow_ptr[rb + 1] = brow_ptr[rb] + present.len();
            bcol_idx.extend_from_slice(&present);
        }
        Ok(Self {
            br,
            bc,
            nrows: a.nrows(),
            ncols: a.ncols(),
            brow_ptr,
            bcol_idx,
            blocks,
        })
    }

    /// Convenience constructor for 3×3 vertex-DOF cloth Hessians.
    ///
    /// # Errors
    /// As [`Bsr::from_csr`].
    pub fn from_csr_3x3(a: &Csr) -> Result<Self, Refusal> {
        Self::from_csr(a, 3, 3)
    }

    /// Expand back to CSR, dropping exact `0.0` fill zeros introduced by blocking.
    #[must_use]
    pub fn to_csr(&self) -> Csr {
        let mut row_ptr = vec![0usize; self.nrows + 1];
        let mut col_idx = Vec::new();
        let mut vals = Vec::new();
        let bsize = self.br * self.bc;
        for rb in 0..self.brow_ptr.len() - 1 {
            for i in 0..self.br {
                let r = rb * self.br + i;
                for slot in self.brow_ptr[rb]..self.brow_ptr[rb + 1] {
                    let cb = self.bcol_idx[slot];
                    for j in 0..self.bc {
                        let v = self.blocks[slot * bsize + i * self.bc + j];
                        if v != 0.0 {
                            col_idx.push(cb * self.bc + j);
                            vals.push(v);
                        }
                    }
                }
                row_ptr[r + 1] = col_idx.len();
            }
        }
        Csr::from_parts(self.nrows, self.ncols, row_ptr, col_idx, vals)
    }

    /// Scalar row count.
    #[must_use]
    pub const fn nrows(&self) -> usize {
        self.nrows
    }

    /// Scalar column count.
    #[must_use]
    pub const fn ncols(&self) -> usize {
        self.ncols
    }

    /// Number of stored dense micro-blocks.
    #[must_use]
    pub fn nblocks(&self) -> usize {
        self.bcol_idx.len()
    }

    /// Block shape `(br, bc)`.
    #[must_use]
    pub const fn block_shape(&self) -> (usize, usize) {
        (self.br, self.bc)
    }

    /// `y = A·x` in strict ascending global column order (bit-identical to [`Csr::spmv`] for
    /// finite inputs; uses multiply-then-add rather than `mul_add` for WASM performance parity).
    ///
    /// # Panics
    /// On mismatched `x` or `y` lengths.
    pub fn spmv(&self, x: &[f64], y: &mut [f64]) {
        assert_eq!(x.len(), self.ncols, "bsr spmv: x length must equal ncols");
        assert_eq!(y.len(), self.nrows, "bsr spmv: y length must equal nrows");
        if self.br == 3 && self.bc == 3 {
            self.spmv_3x3(x, y);
            return;
        }
        let bsize = self.br * self.bc;
        for rb in 0..self.brow_ptr.len() - 1 {
            for i in 0..self.br {
                let mut acc = 0.0f64;
                for slot in self.brow_ptr[rb]..self.brow_ptr[rb + 1] {
                    let cb = self.bcol_idx[slot] * self.bc;
                    let row = &self.blocks[slot * bsize + i * self.bc..][..self.bc];
                    for (j, &v) in row.iter().enumerate() {
                        acc += v * x[cb + j];
                    }
                }
                y[rb * self.br + i] = acc;
            }
        }
    }

    fn spmv_3x3(&self, x: &[f64], y: &mut [f64]) {
        for rb in 0..self.brow_ptr.len() - 1 {
            let mut a0 = 0.0f64;
            let mut a1 = 0.0f64;
            let mut a2 = 0.0f64;
            for slot in self.brow_ptr[rb]..self.brow_ptr[rb + 1] {
                let c = self.bcol_idx[slot] * 3;
                let b = &self.blocks[slot * 9..slot * 9 + 9];
                let (x0, x1, x2) = (x[c], x[c + 1], x[c + 2]);
                a0 += b[0] * x0;
                a0 += b[1] * x1;
                a0 += b[2] * x2;
                a1 += b[3] * x0;
                a1 += b[4] * x1;
                a1 += b[5] * x2;
                a2 += b[6] * x0;
                a2 += b[7] * x1;
                a2 += b[8] * x2;
            }
            let r = rb * 3;
            y[r] = a0;
            y[r + 1] = a1;
            y[r + 2] = a2;
        }
    }
}

// ---------------------------------------------------------------------------
// Preconditioners: Identity, Jacobi, Chebyshev, ILU(0), shifted ILU(0)
// ---------------------------------------------------------------------------

/// Deterministic preconditioner operator: `z ← M⁻¹·r`.
pub trait Precond {
    /// Apply the preconditioner to `r`, writing `z` (same length).
    fn apply(&self, r: &[f64], z: &mut [f64]);
}

/// Identity preconditioner (`z = r`).
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityPrecond;

impl Precond for IdentityPrecond {
    fn apply(&self, r: &[f64], z: &mut [f64]) {
        z.copy_from_slice(r);
    }
}

/// Diagonal (Jacobi) preconditioner; zero or non-finite diagonals fall back to `1.0` so no
/// division by zero can occur.
#[derive(Debug, Clone)]
pub struct Jacobi {
    inv_diag: Vec<f64>,
}

impl Jacobi {
    /// Extract diagonal reciprocals from `a`.
    #[must_use]
    pub fn new(a: &Csr) -> Self {
        let inv_diag = (0..a.nrows())
            .map(|i| {
                let d = a.get(i, i);
                if d.is_finite() && d != 0.0 {
                    1.0 / d
                } else {
                    1.0
                }
            })
            .collect();
        Self { inv_diag }
    }
}

impl Precond for Jacobi {
    fn apply(&self, r: &[f64], z: &mut [f64]) {
        for ((zi, &ri), &di) in z.iter_mut().zip(r).zip(&self.inv_diag) {
            *zi = ri * di;
        }
    }
}

/// Deterministic power iteration estimate of `λ_max(A)` from a fixed non-random seed, multiplied
/// by `safety`. Returns `0.0` for empty or zero matrices.
#[must_use]
pub fn lambda_max_estimate(a: &Csr, iters: usize, safety: f64) -> f64 {
    let n = a.nrows();
    if n == 0 || iters == 0 {
        return 0.0;
    }
    let mut v: Vec<f64> = (0..n)
        .map(|i| 1.0 + 0.3 * (((i % 97) * (2_654_435_761 % 97) % 97) as f64) / 97.0)
        .collect();
    let mut w = vec![0.0f64; n];
    let mut lam = 0.0f64;
    for _ in 0..iters {
        let nrm = norm2(&v);
        if !(nrm.is_finite() && nrm > 0.0) {
            return 0.0;
        }
        let inv = 1.0 / nrm;
        for x in &mut v {
            *x *= inv;
        }
        a.spmv(&v, &mut w);
        lam = dot64(&v, &w);
        std::mem::swap(&mut v, &mut w);
    }
    lam.max(0.0) * safety
}

/// Conservative spectral ceiling: max of power estimate and outward-rounded infinity norm.
/// Propagates non-finite entries as `NaN` rather than absorbing them via `f64::max`.
fn lambda_max_ceiling(a: &Csr, iters: usize, safety: f64) -> f64 {
    let mut row_bound = 0.0f64;
    for i in 0..a.nrows() {
        let mut sum = 0.0f64;
        for &value in a.row(i).1 {
            if !value.is_finite() {
                return f64::NAN;
            }
            sum = (sum + value.abs()).next_up();
        }
        if !sum.is_finite() {
            return f64::NAN;
        }
        if sum > row_bound {
            row_bound = sum;
        }
    }
    let est = lambda_max_estimate(a, iters, safety);
    if !est.is_finite() {
        return f64::NAN;
    }
    est.max(row_bound)
}

/// Degree-`k` Chebyshev polynomial smoother targeting `[λ_max / α, λ_max]` via pure SpMV chains.
#[derive(Debug, Clone)]
pub struct Chebyshev {
    a: Csr,
    degree: usize,
    lo: f64,
    hi: f64,
}

impl Chebyshev {
    /// Construct a Chebyshev smoother with a conservative spectral ceiling.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] on non-square matrix, `degree == 0`, `alpha <= 1.0`, or
    /// zero/non-finite spectral ceiling.
    pub fn new(a: &Csr, degree: usize, alpha: f64) -> Result<Self, Refusal> {
        if a.nrows() != a.ncols() || degree == 0 || !(alpha.is_finite() && alpha > 1.0) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "Chebyshev smoother requires a square matrix, degree >= 1, and finite alpha > 1",
            ));
        }
        let hi = lambda_max_ceiling(a, 30, 1.1);
        if !(hi.is_finite() && hi > 0.0) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "Chebyshev smoother requires a matrix with positive finite spectral ceiling",
            ));
        }
        Ok(Self {
            a: a.clone(),
            degree,
            lo: hi / alpha,
            hi,
        })
    }

    /// Targeted spectral band `(lo, hi)`.
    #[must_use]
    pub const fn band(&self) -> (f64, f64) {
        (self.lo, self.hi)
    }
}

impl Precond for Chebyshev {
    fn apply(&self, r: &[f64], z: &mut [f64]) {
        let n = r.len();
        let theta = 0.5 * (self.hi + self.lo);
        let delta = 0.5 * (self.hi - self.lo);
        let mut zk: Vec<f64> = r.iter().map(|&v| v / theta).collect();
        let mut zk_prev = vec![0.0f64; n];
        let mut rho_prev = delta / theta;
        let mut az = vec![0.0f64; n];
        for _ in 1..self.degree {
            self.a.spmv(&zk, &mut az);
            let rho = 1.0 / (2.0 * theta / delta - rho_prev);
            let c1 = 2.0 * rho / delta;
            let c2 = rho * rho_prev;
            for i in 0..n {
                let rk = r[i] - az[i];
                let znew = zk[i] + c1 * rk + c2 * (zk[i] - zk_prev[i]);
                zk_prev[i] = zk[i];
                zk[i] = znew;
            }
            rho_prev = rho;
        }
        z.copy_from_slice(&zk);
    }
}

/// Typed incomplete-factorization breakdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IluBreakdown {
    /// Row where a missing, zero, or non-finite pivot occurred.
    pub row: usize,
}

/// Zero-fill incomplete LU factorization `ILU(0)` on `A`'s exact CSR sparsity pattern.
#[derive(Debug, Clone)]
pub struct Ilu0 {
    n: usize,
    factored: Csr,
    min_pivot: f64,
}

impl Ilu0 {
    /// Dimension of the factored square matrix.
    #[must_use]
    pub fn dim(&self) -> usize {
        self.n
    }

    /// Smallest diagonal pivot `min_i U_{ii}` produced by the factorization (`+∞` for `n == 0`).
    /// Strictly positive pivots on a symmetric CSR matrix guarantee a positive-definite
    /// `L D Lᵀ` preconditioner suitable for PCG.
    #[must_use]
    pub fn min_pivot(&self) -> f64 {
        self.min_pivot
    }
}

/// Compute zero-fill `ILU(0)` of square matrix `a`.
///
/// # Errors
/// [`IluBreakdown`] naming the first row with a missing, zero, or non-finite pivot.
pub fn ilu0(a: &Csr) -> Result<Ilu0, IluBreakdown> {
    let n = a.nrows();
    if n != a.ncols() {
        return Err(IluBreakdown { row: 0 });
    }
    let mut vals: Vec<Vec<f64>> = Vec::with_capacity(n);
    let mut cols: Vec<Vec<usize>> = Vec::with_capacity(n);
    let mut diag_pos = Vec::with_capacity(n);
    for i in 0..n {
        let (c, v) = a.row(i);
        let Ok(dpos) = c.binary_search(&i) else {
            return Err(IluBreakdown { row: i });
        };
        diag_pos.push(dpos);
        cols.push(c.to_vec());
        vals.push(v.to_vec());
    }
    let mut min_pivot = f64::INFINITY;
    for i in 0..n {
        let row_len = cols[i].len();
        for kk in 0..row_len {
            let k = cols[i][kk];
            if k >= i {
                break;
            }
            let piv = vals[k][diag_pos[k]];
            if !piv.is_finite() || piv == 0.0 {
                return Err(IluBreakdown { row: k });
            }
            let lik = vals[i][kk] / piv;
            if !lik.is_finite() {
                return Err(IluBreakdown { row: i });
            }
            vals[i][kk] = lik;
            // Merge upper part of row k (j > k) against row i's columns using two-pointer scan.
            let mut pi = kk + 1;
            let k_start = diag_pos[k] + 1;
            let k_len = cols[k].len();
            for pk in k_start..k_len {
                let j = cols[k][pk];
                while pi < row_len && cols[i][pi] < j {
                    pi += 1;
                }
                if pi >= row_len {
                    break;
                }
                if cols[i][pi] == j {
                    let ukj = vals[k][pk];
                    vals[i][pi] -= lik * ukj;
                }
            }
        }
        let di = vals[i][diag_pos[i]];
        if !di.is_finite() || di == 0.0 {
            return Err(IluBreakdown { row: i });
        }
        if di < min_pivot {
            min_pivot = di;
        }
    }
    let mut row_ptr = vec![0usize; n + 1];
    let mut col_idx = Vec::with_capacity(a.nnz());
    let mut v_all = Vec::with_capacity(a.nnz());
    for i in 0..n {
        col_idx.extend_from_slice(&cols[i]);
        v_all.extend_from_slice(&vals[i]);
        row_ptr[i + 1] = col_idx.len();
    }
    Ok(Ilu0 {
        n,
        factored: Csr::from_parts(n, n, row_ptr, col_idx, v_all),
        min_pivot,
    })
}

/// Zero-fill `ILU(0)` with geometric diagonal-shift fallback: tries `ilu0(a)` first (`shift = 0`),
/// then retries on `A + σ·scale·I` with `σ = 10^(-8 + 2k)` for `k = 0..max_shifts`.
/// Returns `(preconditioner, applied_shift)`.
///
/// # Errors
/// [`IluBreakdown`] if every attempt (including shifts) breaks down.
pub fn ilu0_shifted(a: &Csr, max_shifts: usize) -> Result<(Ilu0, f64), IluBreakdown> {
    ilu0_shifted_impl(a, max_shifts, false)
}

/// Symmetric-SPD variant of [`ilu0_shifted`]: requires every diagonal pivot `U_{ii} > 0`, shifting
/// `A + σ·scale·I` geometrically until all pivots are strictly positive so the resulting
/// `L U = L D Lᵀ` preconditioner is positive definite for PCG.
///
/// # Errors
/// [`IluBreakdown`] if no attempt within `max_shifts` yields strictly positive pivots.
pub fn ilu0_spd_shifted(a: &Csr, max_shifts: usize) -> Result<(Ilu0, f64), IluBreakdown> {
    ilu0_shifted_impl(a, max_shifts, true)
}

fn ilu0_shifted_impl(
    a: &Csr,
    max_shifts: usize,
    require_positive_pivots: bool,
) -> Result<(Ilu0, f64), IluBreakdown> {
    if let Ok(ilu) = ilu0(a)
        && (!require_positive_pivots || ilu.min_pivot() > 0.0)
    {
        return Ok((ilu, 0.0));
    }
    let n = a.nrows();
    if n != a.ncols() {
        return Err(IluBreakdown { row: 0 });
    }
    let mut diag_slots = Vec::with_capacity(n);
    let mut offset = 0usize;
    let mut max_diag = 0.0f64;
    for i in 0..n {
        let (cols, vals) = a.row(i);
        let Ok(pos) = cols.binary_search(&i) else {
            return Err(IluBreakdown { row: i });
        };
        let d = vals[pos].abs();
        if !d.is_finite() {
            return Err(IluBreakdown { row: i });
        }
        max_diag = max_diag.max(d);
        diag_slots.push(offset + pos);
        offset += cols.len();
    }
    let scale = max_diag.max(1.0);
    let mut last_err = IluBreakdown { row: 0 };
    let mut factor = 1e-8f64;
    for _ in 0..max_shifts {
        let shift = factor * scale;
        let mut shifted = a.clone();
        let vals = shifted.values_mut();
        for &slot in &diag_slots {
            vals[slot] += shift;
        }
        match ilu0(&shifted) {
            Ok(ilu) if !require_positive_pivots || ilu.min_pivot() > 0.0 => {
                return Ok((ilu, shift));
            }
            Ok(_) => {
                last_err = IluBreakdown { row: 0 };
            }
            Err(e) => last_err = e,
        }
        factor *= 100.0;
    }
    Err(last_err)
}

impl Precond for Ilu0 {
    fn apply(&self, r: &[f64], z: &mut [f64]) {
        let n = self.n;
        z.copy_from_slice(r);
        for i in 0..n {
            let (cols, vals) = self.factored.row(i);
            let mut v = z[i];
            for (&c, &lv) in cols.iter().zip(vals) {
                if c >= i {
                    break;
                }
                v -= lv * z[c];
            }
            z[i] = v;
        }
        for i in (0..n).rev() {
            let (cols, vals) = self.factored.row(i);
            let mut v = z[i];
            let mut diag = 1.0;
            for (&c, &uv) in cols.iter().zip(vals) {
                if c < i {
                    continue;
                }
                if c == i {
                    diag = uv;
                } else {
                    v -= uv * z[c];
                }
            }
            z[i] = v / diag;
        }
    }
}

// ---------------------------------------------------------------------------
// Mixed-precision f32 inner CG + f64 iterative refinement
// ---------------------------------------------------------------------------

/// `f32` truncated copy of a [`Csr`] matrix for bandwidth-efficient inner Krylov iterations.
#[derive(Debug, Clone)]
pub struct CsrF32 {
    nrows: usize,
    ncols: usize,
    row_ptr: Vec<usize>,
    cols: Vec<usize>,
    vals: Vec<f32>,
}

impl CsrF32 {
    /// Truncate an `f64` CSR matrix to `f32` storage.
    #[must_use]
    pub fn from_csr(a: &Csr) -> Self {
        let mut row_ptr = Vec::with_capacity(a.nrows() + 1);
        let mut cols = Vec::with_capacity(a.nnz());
        let mut vals = Vec::with_capacity(a.nnz());
        row_ptr.push(0);
        for r in 0..a.nrows() {
            let (rc, rv) = a.row(r);
            for (&c, &v) in rc.iter().zip(rv) {
                cols.push(c);
                #[allow(clippy::cast_possible_truncation)]
                vals.push(v as f32);
            }
            row_ptr.push(cols.len());
        }
        Self {
            nrows: a.nrows(),
            ncols: a.ncols(),
            row_ptr,
            cols,
            vals,
        }
    }

    /// `y = A·x` in `f32` arithmetic (fixed ascending column order, no `mul_add`).
    ///
    /// # Panics
    /// On mismatched lengths.
    pub fn spmv(&self, x: &[f32], y: &mut [f32]) {
        assert_eq!(x.len(), self.ncols, "CsrF32 spmv: x length mismatch");
        assert_eq!(y.len(), self.nrows, "CsrF32 spmv: y length mismatch");
        for (r, yr) in y.iter_mut().enumerate() {
            let mut acc = 0.0f32;
            for i in self.row_ptr[r]..self.row_ptr[r + 1] {
                acc += self.vals[i] * x[self.cols[i]];
            }
            *yr = acc;
        }
    }
}

/// Evidence report from [`mixed_cg_refine`].
#[derive(Debug, Clone)]
pub struct MixedReport {
    /// Outer `f64` refinement steps taken.
    pub refine_steps: usize,
    /// Total inner `f32` CG iterations across all refinement rounds.
    pub inner_iters: usize,
    /// Final recomputed `f64` relative residual `‖b − Ax‖₂ / ‖b‖₂`.
    pub rel_residual: f64,
    /// Final Rigal–Gaches normwise backward error.
    pub backward_error: f64,
    /// Whether the target tolerance was achieved in `f64`.
    pub converged: bool,
    /// True `f64` relative residual at the start of each refinement round.
    pub history: Vec<f64>,
    /// True when the `f32` inner solve stalled or overflowed and the caller must escalate to `f64`.
    pub escalate: bool,
}

fn dot32(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = 0.0f32;
    for (&x, &y) in a.iter().zip(b) {
        acc += x * y;
    }
    acc
}

fn cg_f32(a: &CsrF32, b: &[f32], tol: f32, max_iters: usize) -> (Vec<f32>, usize, bool) {
    let n = b.len();
    let mut x = vec![0.0f32; n];
    let mut r = b.to_vec();
    let mut p = b.to_vec();
    let mut ap = vec![0.0f32; n];
    let bnorm = dot32(b, b).sqrt().max(f32::MIN_POSITIVE);
    let mut rz = dot32(&r, &r);
    if !rz.is_finite() {
        return (x, 0, true);
    }
    let mut iters = 0usize;
    for _ in 0..max_iters {
        if rz.sqrt() / bnorm < tol {
            break;
        }
        a.spmv(&p, &mut ap);
        let pap = dot32(&p, &ap);
        if !(pap.is_finite() && pap > 0.0) {
            return (x, iters, true);
        }
        let alpha = rz / pap;
        if !alpha.is_finite() {
            return (x, iters, true);
        }
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        let rz_new = dot32(&r, &r);
        if !rz_new.is_finite() {
            return (x, iters + 1, true);
        }
        let beta = rz_new / rz;
        rz = rz_new;
        for i in 0..n {
            p[i] = r[i] + beta * p[i];
        }
        iters += 1;
    }
    let broken = x.iter().any(|v| !v.is_finite());
    (x, iters, broken)
}

/// Solve SPD `a64 · x = b` to `f64` tolerance using `f32` inner CG under `f64` true-residual
/// iterative refinement. Sets [`MixedReport::escalate`] whenever `f32` stalls or loses precision
/// so callers escalate to full `f64` explicitly rather than silently accepting a poor solve.
pub fn mixed_cg_refine(
    a64: &Csr,
    a32: &CsrF32,
    b: &[f64],
    x: &mut [f64],
    tol: f64,
    max_refines: usize,
    max_inner: usize,
) -> MixedReport {
    let n = b.len();
    assert_eq!(a64.nrows(), n, "mixed_cg_refine: matrix/rhs size mismatch");
    assert_eq!(x.len(), n, "mixed_cg_refine: solution length mismatch");
    if !(tol.is_finite() && tol > 0.0) {
        return MixedReport {
            refine_steps: 0,
            inner_iters: 0,
            rel_residual: f64::NAN,
            backward_error: f64::NAN,
            converged: false,
            history: Vec::new(),
            escalate: true,
        };
    }
    let bnorm = norm2(b).max(f64::MIN_POSITIVE);
    let mut history = Vec::new();
    let mut inner_total = 0usize;
    let mut ax = vec![0.0f64; n];
    let mut escalate = false;
    for step in 0..=max_refines {
        a64.spmv(x, &mut ax);
        let r64: Vec<f64> = b.iter().zip(&ax).map(|(&bi, &ai)| bi - ai).collect();
        let rnorm = norm2(&r64);
        let rel = rnorm / bnorm;
        history.push(rel);
        if rel.is_finite() && rel < tol {
            let eta = rigal_gaches_backward_error(a64, b, x);
            return MixedReport {
                refine_steps: step,
                inner_iters: inner_total,
                rel_residual: rel,
                backward_error: eta,
                converged: true,
                history,
                escalate: false,
            };
        }
        if !rel.is_finite() || step == max_refines {
            escalate |= !rel.is_finite();
            break;
        }
        if history.len() >= 2 && rel > history[history.len() - 2] * 0.5 {
            escalate = true;
            break;
        }
        let scale = rnorm.max(f64::MIN_POSITIVE);
        #[allow(clippy::cast_possible_truncation)]
        let r32: Vec<f32> = r64.iter().map(|&v| (v / scale) as f32).collect();
        let (c32, it, broken) = cg_f32(a32, &r32, 1e-4, max_inner);
        inner_total += it;
        if broken {
            escalate = true;
            break;
        }
        for i in 0..n {
            x[i] += scale * f64::from(c32[i]);
        }
    }
    let rel = *history.last().unwrap_or(&f64::NAN);
    let eta = rigal_gaches_backward_error(a64, b, x);
    MixedReport {
        refine_steps: history.len().saturating_sub(1),
        inner_iters: inner_total,
        rel_residual: rel,
        backward_error: eta,
        converged: false,
        history,
        escalate,
    }
}

// ---------------------------------------------------------------------------
// Preconditioned MINRES with typed ResidualClaim (symmetric indefinite)
// ---------------------------------------------------------------------------

/// What a reported Krylov relative residual measures, and in which norm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResidualClaim {
    /// Recomputed `‖b − Ax‖₂ / ‖b‖₂`.
    TrueEuclidean(f64),
    /// Recursively propagated Euclidean estimate (can drift from true residual).
    RecursiveEstimate(f64),
    /// Recurrence estimate in the preconditioner's M-norm `‖r‖_M / ‖b‖_M`.
    MNormEstimate(f64),
}

impl ResidualClaim {
    /// Reported scalar magnitude.
    #[must_use]
    pub const fn value(self) -> f64 {
        match self {
            Self::TrueEuclidean(v) | Self::RecursiveEstimate(v) | Self::MNormEstimate(v) => v,
        }
    }

    /// Returns `Some(v)` ONLY for a recomputed Euclidean relative residual.
    #[must_use]
    pub const fn euclidean(self) -> Option<f64> {
        match self {
            Self::TrueEuclidean(v) => Some(v),
            Self::RecursiveEstimate(_) | Self::MNormEstimate(_) => None,
        }
    }
}

/// Failure modes of [`pminres`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PminresError {
    /// Dimension mismatch or invalid tolerance.
    InvalidInput,
    /// Preconditioner failed positivity (`⟨v, Mv⟩` failed to clear rounding floor).
    PreconditionerNotSpd {
        /// Iteration at which positivity failed.
        iteration: usize,
        /// Observed `⟨v, Mv⟩`.
        inner_product: f64,
    },
    /// Degenerate Givens rotation or non-finite state.
    Breakdown {
        /// Iteration at which breakdown occurred.
        iteration: usize,
    },
}

/// Evidence report from [`pminres`], carrying BOTH the recurrence M-norm claim AND recomputed
/// true Euclidean residual + Rigal–Gaches backward error so they are never conflated.
#[derive(Debug, Clone, PartialEq)]
pub struct PminresReport {
    /// Lanczos iterations performed.
    pub iters: usize,
    /// Recurrence M-norm residual claim.
    pub m_norm_claim: ResidualClaim,
    /// Recomputed true Euclidean relative residual `‖b − Ax‖₂ / ‖b‖₂`.
    pub true_euclidean_residual: f64,
    /// Recomputed Rigal–Gaches normwise backward error.
    pub backward_error: f64,
    /// Whether the recomputed Euclidean residual met `tol` (or backward error `≤ 64 ε`).
    pub converged_true: bool,
}

/// Preconditioned MINRES for symmetric (possibly indefinite) `a · x = b` with SPD preconditioner
/// `m`. Refuses non-SPD preconditioners fail-closed rather than fabricating convergence, and
/// independently verifies the true Euclidean residual and Rigal–Gaches backward error on exit.
///
/// # Errors
/// [`PminresError`] on invalid inputs, loss of preconditioner positivity, or numerical breakdown.
pub fn pminres<P: Precond>(
    a: &Csr,
    m: &P,
    b: &[f64],
    tol: f64,
    max_iters: usize,
) -> Result<(Vec<f64>, PminresReport), PminresError> {
    let n = a.nrows();
    if a.ncols() != n || b.len() != n || !(tol.is_finite() && tol > 0.0) {
        return Err(PminresError::InvalidInput);
    }
    if n == 0 || b.iter().all(|&v| v == 0.0) {
        return Ok((
            vec![0.0; n],
            PminresReport {
                iters: 0,
                m_norm_claim: ResidualClaim::MNormEstimate(0.0),
                true_euclidean_residual: 0.0,
                backward_error: 0.0,
                converged_true: true,
            },
        ));
    }
    if b.iter().any(|v| !v.is_finite()) {
        return Err(PminresError::InvalidInput);
    }
    let mut z = vec![0.0f64; n];
    m.apply(b, &mut z);
    let bz = dot64(b, &z);
    let b_norm = norm2(b);
    let z_norm = norm2(&z);
    let init_floor = 4.0 * f64::EPSILON * b_norm * z_norm;
    if !(bz.is_finite() && bz > init_floor) {
        return Err(PminresError::PreconditionerNotSpd {
            iteration: 0,
            inner_product: bz,
        });
    }
    let mut beta = bz.sqrt();
    let bnorm_m = beta;
    let mut v_prev = vec![0.0f64; n];
    let mut v: Vec<f64> = b.iter().map(|&bi| bi / beta).collect();
    for zi in &mut z {
        *zi /= beta;
    }
    let mut x = vec![0.0f64; n];
    let mut w_prev2 = vec![0.0f64; n];
    let mut w_prev1 = vec![0.0f64; n];
    let mut c_km1 = 1.0f64;
    let mut c_km2 = 1.0f64;
    let mut s_km1 = 0.0f64;
    let mut s_km2 = 0.0f64;
    let mut eta = beta;
    let mut p = vec![0.0f64; n];
    let mut z_next = vec![0.0f64; n];
    let mut iters = 0usize;

    for it in 0..max_iters {
        if eta.abs() / bnorm_m < tol {
            break;
        }
        a.spmv(&z, &mut p);
        let alpha = dot64(&z, &p);
        for i in 0..n {
            p[i] = p[i] - alpha * v[i] - beta * v_prev[i];
        }
        m.apply(&p, &mut z_next);
        let vz = dot64(&p, &z_next);
        let p_norm = norm2(&p);
        let zn_norm = norm2(&z_next);
        // Exact invariant-subspace happy breakdown only when p == 0; otherwise
        // require ⟨p, Mp⟩ > 4 ε ‖p‖ ‖Mp‖ so a non-SPD preconditioner can never
        // masquerade as convergence regardless of RHS or operator scaling.
        let happy_breakdown = p_norm == 0.0;
        let beta_next = if happy_breakdown {
            0.0
        } else {
            let floor = 4.0 * f64::EPSILON * p_norm * zn_norm;
            if !(vz.is_finite() && vz > floor) {
                return Err(PminresError::PreconditionerNotSpd {
                    iteration: it + 1,
                    inner_product: vz,
                });
            }
            vz.sqrt()
        };
        let delta = c_km1 * alpha - c_km2 * s_km1 * beta;
        let rho1 = det::hypot(delta, beta_next);
        if !(rho1.is_finite() && rho1 > 0.0) {
            return Err(PminresError::Breakdown { iteration: it + 1 });
        }
        let rho2 = s_km1 * alpha + c_km2 * c_km1 * beta;
        let rho3 = s_km2 * beta;
        let c_k = delta / rho1;
        let s_k = beta_next / rho1;
        let step_scale = c_k * eta;
        for i in 0..n {
            let w_k = (z[i] - rho3 * w_prev2[i] - rho2 * w_prev1[i]) / rho1;
            x[i] += step_scale * w_k;
            w_prev2[i] = w_prev1[i];
            w_prev1[i] = w_k;
        }
        eta *= -s_k;
        beta = beta_next;
        c_km2 = c_km1;
        c_km1 = c_k;
        s_km2 = s_km1;
        s_km1 = s_k;
        iters = it + 1;
        if happy_breakdown {
            break;
        }
        for i in 0..n {
            v_prev[i] = v[i];
            v[i] = p[i] / beta_next;
            z[i] = z_next[i] / beta_next;
        }
    }
    if x.iter().any(|v| !v.is_finite()) {
        return Err(PminresError::Breakdown { iteration: iters });
    }
    let true_rel = true_relative_residual(a, b, &x);
    let back_err = rigal_gaches_backward_error(a, b, &x);
    let converged_true = (true_rel.is_finite() && true_rel <= tol)
        || (back_err.is_finite() && back_err <= BACKWARD_ERROR_ULPS * f64::EPSILON);
    Ok((
        x,
        PminresReport {
            iters,
            m_norm_claim: ResidualClaim::MNormEstimate(eta.abs() / bnorm_m),
            true_euclidean_residual: true_rel,
            backward_error: back_err,
            converged_true,
        },
    ))
}

// ---------------------------------------------------------------------------
// Eisenstat–Walker adaptive forcing & Steihaug–Toint trust-region CG
// ---------------------------------------------------------------------------

/// Eisenstat–Walker Choice 2 safeguarded forcing term for inexact Newton:
/// `η_k = clamp(max(γ (‖g_k‖/‖g_{k−1}‖)^α, safeguard), η_min, η_max)`.
///
/// Uses exact multiplication for `α = 2.0` and `ftx_core::det::{exp, ln}` otherwise (no
/// platform `libm`).
#[must_use]
pub fn eisenstat_walker(
    prev_grad_norm: f64,
    curr_grad_norm: f64,
    prev_eta: f64,
    eta_min: f64,
    eta_max: f64,
    gamma: f64,
    alpha: f64,
) -> f64 {
    let fallback_max = if eta_max.is_finite() && eta_max > 0.0 {
        eta_max
    } else {
        0.5
    };
    if !(eta_min.is_finite()
        && eta_min >= 0.0
        && eta_max.is_finite()
        && eta_max > 0.0
        && eta_min <= eta_max
        && gamma.is_finite()
        && gamma > 0.0
        && alpha.is_finite()
        && alpha > 0.0)
    {
        return fallback_max;
    }
    if !(prev_grad_norm.is_finite()
        && curr_grad_norm.is_finite()
        && prev_grad_norm > 0.0
        && curr_grad_norm >= 0.0)
    {
        return eta_max;
    }
    if curr_grad_norm == 0.0 {
        return eta_min;
    }
    let ratio = curr_grad_norm / prev_grad_norm;
    let pow_fn = |x: f64, p: f64| -> f64 {
        if (p - 2.0).abs() == 0.0 {
            x * x
        } else {
            det::exp(p * det::ln(x))
        }
    };
    let mut eta = gamma * pow_fn(ratio, alpha);
    if prev_eta.is_finite() && prev_eta > 0.0 {
        let safeguard = gamma * pow_fn(prev_eta, alpha);
        if safeguard > 0.1 {
            eta = eta.max(safeguard);
        }
    }
    if !eta.is_finite() {
        return eta_max;
    }
    eta.clamp(eta_min, eta_max)
}

/// Configuration for [`steihaug_cg`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteihaugConfig {
    /// Trust-region radius `Δ > 0` in the Euclidean norm `‖p‖₂ ≤ Δ`.
    pub radius: f64,
    /// Relative residual tolerance `‖Hp + g‖₂ ≤ tol ‖g‖₂`.
    pub rel_tol: f64,
    /// Maximum CG iterations.
    pub max_iters: usize,
}

/// Result of [`steihaug_cg`] on `min_{‖p‖₂ ≤ Δ} gᵀp + ½ pᵀ H p`.
#[derive(Debug, Clone, PartialEq)]
pub struct SteihaugOutcome {
    /// Trial step `p` with `‖p‖₂ ≤ Δ`.
    pub step: Vec<f64>,
    /// Whether `p` landed on the trust-region boundary `‖p‖₂ = Δ`.
    pub hit_boundary: bool,
    /// Whether negative curvature `dᵀ H d ≤ 0` was encountered (escaping a saddle / buckle).
    pub negative_curvature: bool,
    /// Whether the unconstrained Newton equation converged inside the ball (`!hit_boundary`).
    pub interior_converged: bool,
    /// Hessian-vector products evaluated.
    pub matvecs: usize,
    /// Predicted quadratic model decrease `m(0) − m(p) = −gᵀp − ½ pᵀHp`.
    pub model_decrease: f64,
}

/// Numerically stable positive root `τ ≥ 0` of `‖p + τ d‖₂² = Δ²`.
fn boundary_tau(p: &[f64], d: &[f64], delta: f64) -> f64 {
    let pd = dot64(p, d);
    let dd = dot64(d, d);
    let pp = dot64(p, p);
    if !(dd.is_finite() && dd > 0.0) {
        return 0.0;
    }
    let gap = (delta * delta - pp).max(0.0);
    let disc = (pd * pd + dd * gap).max(0.0);
    let root = disc.sqrt();
    if pd >= 0.0 {
        let denom = pd + root;
        if denom > 0.0 { gap / denom } else { 0.0 }
    } else {
        (-pd + root) / dd
    }
}

/// Steihaug–Toint truncated conjugate gradients on the quadratic model
/// `m(p) = gᵀp + ½ pᵀ H p` subject to `‖p‖₂ ≤ Δ`, using an unprojected symmetric Hessian `a`.
///
/// Escapes negative curvature (`dᵀ H d ≤ 0`) by stepping along `d` to the trust-region boundary,
/// giving second-order descent across buckling / wrinkling saddles without PSD projection.
pub fn steihaug_cg(a: &Csr, g: &[f64], cfg: SteihaugConfig) -> SteihaugOutcome {
    let n = g.len();
    let mut p = vec![0.0f64; n];
    if n == 0
        || !(cfg.radius.is_finite() && cfg.radius > 0.0)
        || !(cfg.rel_tol.is_finite() && cfg.rel_tol >= 0.0)
    {
        return SteihaugOutcome {
            step: p,
            hit_boundary: false,
            negative_curvature: false,
            interior_converged: n == 0,
            matvecs: 0,
            model_decrease: 0.0,
        };
    }
    let mut r: Vec<f64> = g.iter().map(|&v| -v).collect();
    let mut d = r.clone();
    let mut rr = dot64(&r, &r);
    let g_norm = rr.sqrt();
    if !(g_norm.is_finite() && g_norm > 0.0) {
        return SteihaugOutcome {
            step: p,
            hit_boundary: false,
            negative_curvature: false,
            interior_converged: g_norm == 0.0,
            matvecs: 0,
            model_decrease: 0.0,
        };
    }
    let target = cfg.rel_tol * g_norm;
    let mut hd = vec![0.0f64; n];
    let mut matvecs = 0usize;
    let mut hit_boundary = false;
    let mut negative_curvature = false;
    let mut interior_converged = false;
    let cap = cfg.max_iters.max(1);

    for _ in 0..cap {
        if rr.sqrt() <= target {
            interior_converged = true;
            break;
        }
        a.spmv(&d, &mut hd);
        matvecs += 1;
        let dhd = dot64(&d, &hd);
        if !dhd.is_finite() {
            break;
        }
        if dhd <= 0.0 {
            let tau = boundary_tau(&p, &d, cfg.radius);
            for i in 0..n {
                p[i] += tau * d[i];
            }
            hit_boundary = true;
            negative_curvature = true;
            break;
        }
        let alpha = rr / dhd;
        if !alpha.is_finite() {
            let tau = boundary_tau(&p, &d, cfg.radius);
            for i in 0..n {
                p[i] += tau * d[i];
            }
            hit_boundary = true;
            break;
        }
        let mut p_next = p.clone();
        for i in 0..n {
            p_next[i] += alpha * d[i];
        }
        let p_next_norm = norm2(&p_next);
        if !p_next_norm.is_finite() || p_next_norm >= cfg.radius {
            let tau = boundary_tau(&p, &d, cfg.radius);
            for i in 0..n {
                p[i] += tau * d[i];
            }
            hit_boundary = true;
            break;
        }
        p = p_next;
        for i in 0..n {
            r[i] -= alpha * hd[i];
        }
        let rr_new = dot64(&r, &r);
        if !rr_new.is_finite() {
            break;
        }
        if rr_new.sqrt() <= target {
            interior_converged = true;
            break;
        }
        let beta = rr_new / rr;
        rr = rr_new;
        for i in 0..n {
            d[i] = r[i] + beta * d[i];
        }
    }

    // Recompute exact model decrease m(0) - m(p) = -g^T p - 1/2 p^T H p and verify the true
    // residual ‖H p + g‖₂ before certifying interior convergence (checked-CG gate, zero extra SpMV).
    a.spmv(&p, &mut hd);
    matvecs += 1;
    let gp = dot64(g, &p);
    let php = dot64(&p, &hd);
    let model_decrease = -gp - 0.5 * php;
    if interior_converged {
        let mut true_rr = 0.0f64;
        for i in 0..n {
            let ri = hd[i] + g[i];
            true_rr += ri * ri;
        }
        let true_r_norm = true_rr.sqrt();
        interior_converged = true_r_norm.is_finite() && true_r_norm <= target;
    }

    SteihaugOutcome {
        step: p,
        hit_boundary,
        negative_curvature,
        interior_converged,
        matvecs,
        model_decrease,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse::Coo;
    use crate::sparse::precond::pcg;
    use crate::sparse::tests::laplacian_2d;

    fn lcg(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((*seed >> 11) as f64) / ((1u64 << 53) as f64) - 0.5
    }

    #[test]
    fn bsr_spmv_matches_csr_bit_for_bit_across_block_shapes() {
        let mut seed = 42u64;
        for (n, br, bc) in [
            (36usize, 3usize, 3usize),
            (64, 2, 2),
            (64, 4, 4),
            (30, 3, 5),
        ] {
            let a = if n == 64 && br == bc {
                laplacian_2d(8)
            } else {
                let mut coo = Coo::new(n, n);
                for i in 0..n {
                    coo.push(i, i, 5.0 + lcg(&mut seed));
                    if i + 1 < n {
                        let v = lcg(&mut seed);
                        coo.push(i, i + 1, v);
                        coo.push(i + 1, i, v);
                    }
                    if i + 3 < n {
                        let v = 0.5 * lcg(&mut seed);
                        coo.push(i, i + 3, v);
                        coo.push(i + 3, i, v);
                    }
                }
                coo.assemble()
            };
            let bsr = Bsr::from_csr(&a, br, bc).unwrap();
            let rt = bsr.to_csr();
            assert_eq!(a.to_dense(), rt.to_dense());

            let x: Vec<f64> = (0..n).map(|_| lcg(&mut seed)).collect();
            let (mut y_csr, mut y_bsr) = (vec![0.0; n], vec![0.0; n]);
            a.spmv(&x, &mut y_csr);
            bsr.spmv(&x, &mut y_bsr);
            for r in 0..n {
                assert_eq!(
                    y_csr[r].to_bits(),
                    y_bsr[r].to_bits(),
                    "BSR({br}x{bc}) diverged from CSR at row {r}"
                );
            }
        }
        assert!(Bsr::from_csr(&laplacian_2d(3), 2, 2).is_err());
    }

    #[test]
    fn ilu0_and_shifted_ilu0_precondition_pcg_and_chebyshev_smooths() {
        let a = laplacian_2d(10);
        let n = a.nrows();
        let b: Vec<f64> = (0..n).map(|i| ((i % 11) as f64 - 5.0) * 0.25).collect();

        let (ilu, shift) = ilu0_shifted(&a, 4).unwrap();
        assert_eq!(shift, 0.0);
        let (x_ilu, rep_ilu) = pcg(
            &a,
            &b,
            |r| {
                let mut z = vec![0.0; n];
                ilu.apply(r, &mut z);
                z
            },
            1e-10,
            200,
        )
        .unwrap();
        let (x_jac, rep_jac) = pcg(
            &a,
            &b,
            |r| r.iter().map(|&v| v * 0.25).collect(),
            1e-10,
            200,
        )
        .unwrap();
        assert!(
            rep_ilu.iters < rep_jac.iters,
            "ILU(0) ({}) should take fewer PCG iters than Jacobi ({})",
            rep_ilu.iters,
            rep_jac.iters
        );
        assert!(rigal_gaches_backward_error(&a, &b, &x_ilu) < 1e-10);
        for (p, q) in x_ilu.iter().zip(&x_jac) {
            assert!((p - q).abs() < 1e-8);
        }

        // Exact zero-pivot matrix (`u_11 = 0.25 - (-1/4)*(-1) = 0`) triggers breakdown on plain
        // ilu0, recovered by ilu0_shifted.
        let mut coo = Coo::new(4, 4);
        for i in 0..4 {
            coo.push(i, i, if i == 1 { 0.25 } else { 4.0 });
            if i + 1 < 4 {
                coo.push(i, i + 1, -1.0);
                coo.push(i + 1, i, -1.0);
            }
        }
        let singular_piv = coo.assemble();
        assert_eq!(ilu0(&singular_piv).unwrap_err(), IluBreakdown { row: 1 });
        let (_shifted, applied_shift) = ilu0_shifted(&singular_piv, 6).unwrap();
        assert!(applied_shift > 0.0);

        // Negative-pivot matrix (`u_11 = 0 - 0.25 = -0.25`) is rejected at shift=0 by
        // `ilu0_spd_shifted` and shifted until all pivots are strictly positive.
        let mut coo_neg = Coo::new(4, 4);
        for i in 0..4 {
            coo_neg.push(i, i, if i == 1 { 0.0 } else { 4.0 });
            if i + 1 < 4 {
                coo_neg.push(i, i + 1, -1.0);
                coo_neg.push(i + 1, i, -1.0);
            }
        }
        let neg_piv = coo_neg.assemble();
        assert!(ilu0(&neg_piv).unwrap().min_pivot() < 0.0);
        let (spd_ilu, spd_shift) = ilu0_spd_shifted(&neg_piv, 6).unwrap();
        assert!(spd_shift > 0.0);
        assert!(spd_ilu.min_pivot() > 0.0);

        // Chebyshev smoother damps high-frequency error and never panics.
        let cheb = Chebyshev::new(&a, 4, 20.0).unwrap();
        let (lo, hi) = cheb.band();
        assert!(hi > lo && lo > 0.0);
        let mut z = vec![0.0; n];
        cheb.apply(&b, &mut z);
        assert!(z.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn mixed_precision_cg_refines_to_f64_and_escalates_on_indefinite() {
        let a = laplacian_2d(8);
        let a32 = CsrF32::from_csr(&a);
        let n = a.nrows();
        let b: Vec<f64> = (0..n).map(|i| (i as f64 + 1.0) * 0.1).collect();
        let mut x = vec![0.0; n];
        let rep = mixed_cg_refine(&a, &a32, &b, &mut x, 1e-12, 8, 200);
        assert!(rep.converged, "{rep:?}");
        assert!(!rep.escalate);
        assert!(rep.rel_residual < 1e-12);
        assert!(rep.backward_error < 1e-12);
        assert!(true_relative_residual(&a, &b, &x) < 1e-12);

        // Indefinite matrix must escalate cleanly instead of silently claiming convergence.
        let mut coo = Coo::new(3, 3);
        coo.push(0, 0, 2.0);
        coo.push(1, 1, -3.0);
        coo.push(2, 2, 2.0);
        let indef = coo.assemble();
        let indef32 = CsrF32::from_csr(&indef);
        let mut xi = vec![0.0; 3];
        let rep_i = mixed_cg_refine(&indef, &indef32, &[1.0, 1.0, 1.0], &mut xi, 1e-10, 4, 20);
        assert!(!rep_i.converged);
        assert!(rep_i.escalate);
    }

    #[test]
    fn pminres_solves_symmetric_indefinite_and_refuses_non_spd_preconditioner() {
        let mut coo = Coo::new(6, 6);
        let diag = [4.0, -3.0, 5.0, -2.5, 6.0, -4.0];
        for (i, &d) in diag.iter().enumerate() {
            coo.push(i, i, d);
            if i + 1 < 6 {
                coo.push(i, i + 1, 0.5);
                coo.push(i + 1, i, 0.5);
            }
        }
        let a = coo.assemble();
        let b = [1.0, -2.0, 0.5, 3.0, -1.5, 2.0];
        // SPD diagonal preconditioner |diag|^{-1}.
        struct AbsDiag(Vec<f64>);
        impl Precond for AbsDiag {
            fn apply(&self, r: &[f64], z: &mut [f64]) {
                for ((zi, &ri), &d) in z.iter_mut().zip(r).zip(&self.0) {
                    *zi = ri / d;
                }
            }
        }
        let m = AbsDiag(diag.iter().map(|d| d.abs()).collect());
        let (x, rep) = pminres(&a, &m, &b, 1e-12, 50).unwrap();
        assert!(rep.converged_true, "{rep:?}");
        assert!(rep.true_euclidean_residual < 1e-11);
        assert!(true_relative_residual(&a, &b, &x) < 1e-11);
        assert!(rep.m_norm_claim.euclidean().is_none());

        // Indefinite preconditioner must be refused fail-closed.
        let bad_m = Jacobi::new(&a);
        let err = pminres(&a, &bad_m, &b, 1e-12, 50).unwrap_err();
        assert!(matches!(err, PminresError::PreconditionerNotSpd { .. }));
    }

    #[test]
    fn steihaug_cg_escapes_negative_curvature_to_boundary_and_matches_newton_on_spd() {
        // Saddle Hessian diag(2, -5) with g = [1, 1]: d = [-1, -1], d^T H d = 2 - 5 = -3 < 0.
        let mut coo = Coo::new(2, 2);
        coo.push(0, 0, 2.0);
        coo.push(1, 1, -5.0);
        let h_saddle = coo.assemble();
        let g = [1.0, 1.0];
        let out = steihaug_cg(
            &h_saddle,
            &g,
            SteihaugConfig {
                radius: 0.5,
                rel_tol: 1e-10,
                max_iters: 10,
            },
        );
        assert!(out.negative_curvature);
        assert!(out.hit_boundary);
        assert!(!out.interior_converged);
        assert!((norm2(&out.step) - 0.5).abs() < 1e-12);
        assert!(out.model_decrease > 0.0);

        // SPD system well inside the ball converges to exact Newton step.
        let a = laplacian_2d(4);
        let g_spd: Vec<f64> = (0..16).map(|i| (i as f64 - 7.5) * 0.1).collect();
        let out_spd = steihaug_cg(
            &a,
            &g_spd,
            SteihaugConfig {
                radius: 100.0,
                rel_tol: 1e-12,
                max_iters: 64,
            },
        );
        assert!(out_spd.interior_converged);
        assert!(!out_spd.hit_boundary);
        assert!(!out_spd.negative_curvature);
        let b: Vec<f64> = g_spd.iter().map(|v| -v).collect();
        assert!(true_relative_residual(&a, &b, &out_spd.step) < 1e-11);

        // Eisenstat–Walker tightens monotonically as gradient drops.
        let eta1 = eisenstat_walker(1.0, 0.1, 0.5, 1e-8, 0.5, 0.9, 2.0);
        let eta2 = eisenstat_walker(0.1, 1e-3, eta1, 1e-8, 0.5, 0.9, 2.0);
        assert!(eta1 < 0.5 && eta2 < eta1);

        // Invalid trust-region radius or tolerance must never report interior convergence for n > 0.
        for bad_cfg in [
            SteihaugConfig {
                radius: 0.0,
                rel_tol: 1e-6,
                max_iters: 10,
            },
            SteihaugConfig {
                radius: f64::NAN,
                rel_tol: 1e-6,
                max_iters: 10,
            },
            SteihaugConfig {
                radius: 0.5,
                rel_tol: f64::NAN,
                max_iters: 10,
            },
        ] {
            let bad = steihaug_cg(&h_saddle, &g, bad_cfg);
            assert!(!bad.interior_converged);
        }

        // Tiny positive curvature where alpha = rr / dhd overflows to +inf must step cleanly to
        // the trust-region boundary without producing inf * 0.0 = NaN.
        let mut tiny_coo = Coo::new(2, 2);
        tiny_coo.push(0, 0, 1e-320);
        tiny_coo.push(1, 1, 1e-320);
        let h_tiny = tiny_coo.assemble();
        let out_tiny = steihaug_cg(
            &h_tiny,
            &[1.0, 0.0],
            SteihaugConfig {
                radius: 0.25,
                rel_tol: 1e-6,
                max_iters: 10,
            },
        );
        assert!(out_tiny.hit_boundary);
        assert!(out_tiny.step.iter().all(|v| v.is_finite()));
        assert!((norm2(&out_tiny.step) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn chebyshev_refuses_nan_pminres_handles_large_givens_and_ew_guards_negative_bounds() {
        // NaN in one row must not be swallowed by f64::max against another finite positive row.
        let mut coo_nan = Coo::new(2, 2);
        coo_nan.push(0, 0, f64::NAN);
        coo_nan.push(1, 1, 2.0);
        let a_nan = coo_nan.assemble();
        let err = Chebyshev::new(&a_nan, 3, 10.0).unwrap_err();
        assert_eq!(err.code, RefusalCode::OutOfRange);

        // Large-scale symmetric indefinite system where delta^2 would overflow f64 without hypot.
        let mut coo_big = Coo::new(2, 2);
        coo_big.push(0, 0, 2.0e160);
        coo_big.push(1, 1, -2.0e160);
        let a_big = coo_big.assemble();
        let (x_big, rep_big) = pminres(&a_big, &IdentityPrecond, &[1.0, 0.0], 1e-12, 10).unwrap();
        assert!(rep_big.converged_true, "{rep_big:?}");
        assert!((x_big[0] - 0.5e-160).abs() <= 1e-172);
        assert_eq!(x_big[1], 0.0);

        // Negative eta_min or non-positive/non-finite eta_max must never produce negative or NaN.
        let eta_neg_min = eisenstat_walker(1.0, 0.0, 0.1, -1.0, 0.5, 0.9, 2.0);
        assert_eq!(eta_neg_min, 0.5);
        for bad_max in [-0.25, 0.0, f64::NAN, f64::INFINITY] {
            let eta = eisenstat_walker(1.0, 0.1, 0.1, 1e-8, bad_max, 0.9, 2.0);
            assert!(eta.is_finite() && eta > 0.0, "bad_max={bad_max} -> {eta}");
        }
    }
}
