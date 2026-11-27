//! Deterministic sparse CSR assembly and SPD linear solver (`ftx-core::sparse`).
//!
//! Zero `unsafe`, zero platform libm, zero hot-loop `mul_add`, and deterministic column-then-row
//! counting-pass assembly so both `ftx-pattern` (3D-to-knit harmonic time fields) and `ftx-drape`
//! (Rung-1 DER rod networks) share a bit-identical native/WASM sparse solver without any crate cycle.

use ftx_core_err::{Refusal, RefusalCode};

use crate as ftx_core_err;

/// Triplet staging buffer; duplicate `(row, col)` entries sum in deterministic insertion order.
#[derive(Clone, Debug, Default)]
pub struct SparseCoo {
    nrows: usize,
    ncols: usize,
    rows: Vec<usize>,
    cols: Vec<usize>,
    vals: Vec<f64>,
}

impl SparseCoo {
    /// Creates an empty triplet buffer of dimensions `nrows x ncols`.
    #[must_use]
    pub const fn new(nrows: usize, ncols: usize) -> Self {
        Self {
            nrows,
            ncols,
            rows: Vec::new(),
            cols: Vec::new(),
            vals: Vec::new(),
        }
    }

    /// Stages one entry `(row, col, val)`.
    ///
    /// # Panics
    /// If `row >= nrows` or `col >= ncols`.
    pub fn push(&mut self, row: usize, col: usize, val: f64) {
        assert!(
            row < self.nrows && col < self.ncols,
            "SparseCoo::push ({row}, {col}) out of bounds {}x{}",
            self.nrows,
            self.ncols
        );
        self.rows.push(row);
        self.cols.push(col);
        self.vals.push(val);
    }

    /// Deterministic linear-time counting-sort assembly into canonical CSR.
    #[must_use]
    pub fn assemble(&self) -> SparseCsr {
        let nnz = self.vals.len();
        let by_col = counting_pass(&self.cols, self.ncols, 0..nnz);
        let order = counting_pass(&self.rows, self.nrows, by_col.iter().copied());
        let mut row_ptr = vec![0usize; self.nrows + 1];
        let mut col_idx = Vec::new();
        let mut vals = Vec::new();
        let mut i = 0;
        while i < nnz {
            let r = self.rows[order[i]];
            let c = self.cols[order[i]];
            let mut acc = self.vals[order[i]];
            i += 1;
            while i < nnz && self.rows[order[i]] == r && self.cols[order[i]] == c {
                acc += self.vals[order[i]];
                i += 1;
            }
            row_ptr[r + 1] += 1;
            col_idx.push(c);
            vals.push(acc);
        }
        for r in 0..self.nrows {
            row_ptr[r + 1] += row_ptr[r];
        }
        SparseCsr {
            nrows: self.nrows,
            ncols: self.ncols,
            row_ptr,
            col_idx,
            vals,
        }
    }
}

fn counting_pass(
    keys: &[usize],
    nkeys: usize,
    input: impl ExactSizeIterator<Item = usize> + Clone,
) -> Vec<usize> {
    let mut next = vec![0usize; nkeys + 1];
    for i in input.clone() {
        next[keys[i] + 1] += 1;
    }
    for k in 0..nkeys {
        next[k + 1] += next[k];
    }
    let mut out = vec![0usize; input.len()];
    for i in input {
        let k = keys[i];
        out[next[k]] = i;
        next[k] += 1;
    }
    out
}

/// Canonical Compressed Sparse Row matrix with sorted column indices per row.
#[derive(Clone, Debug, PartialEq)]
pub struct SparseCsr {
    /// Row count.
    pub nrows: usize,
    /// Column count.
    pub ncols: usize,
    /// Row pointer array of length `nrows + 1`.
    pub row_ptr: Vec<usize>,
    /// Sorted column indices of length `nnz`.
    pub col_idx: Vec<usize>,
    /// Nonzero entries of length `nnz`.
    pub vals: Vec<f64>,
}

impl SparseCsr {
    /// Matrix-vector product `y = A * x` in deterministic row-then-column order (no `mul_add`).
    ///
    /// # Panics
    /// If `x.len() != self.ncols`.
    #[must_use]
    pub fn spmv(&self, x: &[f64]) -> Vec<f64> {
        assert_eq!(x.len(), self.ncols, "spmv dimension mismatch");
        let mut y = vec![0.0; self.nrows];
        for r in 0..self.nrows {
            let mut sum = 0.0;
            for k in self.row_ptr[r]..self.row_ptr[r + 1] {
                sum += self.vals[k] * x[self.col_idx[k]];
            }
            y[r] = sum;
        }
        y
    }

    /// Extracts diagonal entries (`0.0` where structurally absent).
    #[must_use]
    pub fn diagonal(&self) -> Vec<f64> {
        let n = self.nrows.min(self.ncols);
        let mut diag = vec![0.0; n];
        for r in 0..n {
            for k in self.row_ptr[r]..self.row_ptr[r + 1] {
                if self.col_idx[k] == r {
                    diag[r] = self.vals[k];
                    break;
                }
            }
        }
        diag
    }

    /// Solves a symmetric positive-definite linear system `A x = b` deterministically.
    ///
    /// Uses exact dense/banded LDLᵀ for small systems (`n <= 256`) and Jacobi-preconditioned
    /// Conjugate Gradient (PCG) for larger sparse SPD meshes (such as harmonic AutoKnit Laplacians).
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`], [`RefusalCode::DegenerateGeometry`] (dimension/singularity),
    /// or [`RefusalCode::NotConverged`] if `max_iters` is exhausted before reaching `rel_tol`.
    pub fn solve_spd(
        &self,
        b: &[f64],
        rel_tol: f64,
        max_iters: usize,
    ) -> Result<Vec<f64>, Refusal> {
        if self.nrows != self.ncols || b.len() != self.nrows {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "SPD solve requires square matrix matching RHS length",
            ));
        }
        let n = self.nrows;
        if n == 0 {
            return Ok(Vec::new());
        }
        if b.iter().any(|v| !v.is_finite()) || self.vals.iter().any(|v| !v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "SPD linear system contains non-finite entries",
            ));
        }
        if n <= 256 {
            return self.solve_dense_ldlt(b);
        }
        self.solve_pcg(b, rel_tol, max_iters)
    }

    fn solve_dense_ldlt(&self, b: &[f64]) -> Result<Vec<f64>, Refusal> {
        let n = self.nrows;
        let mut a = vec![0.0; n * n];
        for r in 0..n {
            for k in self.row_ptr[r]..self.row_ptr[r + 1] {
                a[r * n + self.col_idx[k]] = self.vals[k];
            }
        }
        let mut l = vec![0.0; n * n];
        let mut d = vec![0.0; n];
        for i in 0..n {
            l[i * n + i] = 1.0;
            for j in 0..i {
                let mut sum = a[i * n + j];
                for k in 0..j {
                    sum -= l[i * n + k] * d[k] * l[j * n + k];
                }
                l[i * n + j] = sum / d[j];
            }
            let mut diag = a[i * n + i];
            for k in 0..i {
                diag -= l[i * n + k] * l[i * n + k] * d[k];
            }
            if !diag.is_finite() || diag.abs() < 1e-18 {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    format!("singular or indefinite pivot at row {i} (d={diag})"),
                ));
            }
            d[i] = diag;
        }
        // Forward substitution L y = b
        let mut y = vec![0.0; n];
        for i in 0..n {
            let mut sum = b[i];
            for j in 0..i {
                sum -= l[i * n + j] * y[j];
            }
            y[i] = sum;
        }
        // Diagonal scaling D z = y
        let mut z = vec![0.0; n];
        for i in 0..n {
            z[i] = y[i] / d[i];
        }
        // Back substitution L^T x = z
        let mut x = vec![0.0; n];
        for i in (0..n).rev() {
            let mut sum = z[i];
            for j in (i + 1)..n {
                sum -= l[j * n + i] * x[j];
            }
            if !sum.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "LDLT back-substitution produced non-finite value",
                ));
            }
            x[i] = sum;
        }
        Ok(x)
    }

    fn solve_pcg(&self, b: &[f64], rel_tol: f64, max_iters: usize) -> Result<Vec<f64>, Refusal> {
        let n = self.nrows;
        let diag = self.diagonal();
        let mut inv_d = vec![0.0; n];
        for i in 0..n {
            if !diag[i].is_finite() || diag[i] <= 1e-18 {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    format!("SPD Jacobi preconditioner non-positive diagonal at row {i}"),
                ));
            }
            inv_d[i] = 1.0 / diag[i];
        }
        let b_norm = dot(b, b).sqrt();
        if b_norm == 0.0 {
            return Ok(vec![0.0; n]);
        }
        let tol_abs = rel_tol.max(1e-14) * b_norm;
        let mut x = vec![0.0; n];
        let mut r = b.to_vec();
        let mut z: Vec<f64> = (0..n).map(|i| inv_d[i] * r[i]).collect();
        let mut p = z.clone();
        let mut rz = dot(&r, &z);

        for _ in 0..max_iters.max(1) {
            let ap = self.spmv(&p);
            let pap = dot(&p, &ap);
            if !pap.is_finite() || pap <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    "PCG encountered non-positive curvature (matrix not SPD)",
                ));
            }
            let alpha = rz / pap;
            for i in 0..n {
                x[i] += alpha * p[i];
                r[i] -= alpha * ap[i];
            }
            let r_norm = dot(&r, &r).sqrt();
            if !r_norm.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "PCG residual became non-finite",
                ));
            }
            if r_norm <= tol_abs {
                return Ok(x);
            }
            for i in 0..n {
                z[i] = inv_d[i] * r[i];
            }
            let rz_new = dot(&r, &z);
            let beta = rz_new / rz;
            for i in 0..n {
                p[i] = z[i] + beta * p[i];
            }
            rz = rz_new;
        }
        Err(Refusal::new(
            RefusalCode::NotConverged,
            format!("PCG did not reach rel_tol={rel_tol} within {max_iters} iterations"),
        ))
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut sum = 0.0;
    for i in 0..a.len() {
        sum += a[i] * b[i];
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ldlt_and_pcg_solve_1d_laplacian_accurately() {
        for n in [16usize, 300usize] {
            let mut coo = SparseCoo::new(n, n);
            let mut b = vec![0.0; n];
            for i in 0..n {
                coo.push(i, i, 2.0);
                if i > 0 {
                    coo.push(i, i - 1, -1.0);
                }
                if i + 1 < n {
                    coo.push(i, i + 1, -1.0);
                }
                b[i] = 1.0 / ((n + 1) as f64);
            }
            let csr = coo.assemble();
            let x = csr.solve_spd(&b, 1e-11, 4000).unwrap();
            let ax = csr.spmv(&x);
            let max_err = ax
                .iter()
                .zip(&b)
                .map(|(u, v)| (u - v).abs())
                .fold(0.0, f64::max);
            assert!(max_err < 1e-8, "n={n} max_err={max_err}");
        }
    }
}
