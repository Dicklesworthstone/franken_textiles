//! Sparse assembly and the symmetric direct solver.
//!
//! Provenance: `Coo` and `Csr` are lifted from frankensim `crates/fs-sparse/src/lib.rs` at commit
//! `93fc9b26f618bea6ebac6534f1d33c715d8a1c92` (MIT + rider), without the BSR/SELL formats, the x86 FMA
//! dispatch (the portable loop is bit-identical) or the interop features; [`direct`] is that crate's
//! LDLᵀ. Assembly orders triplets as a stable sort by (row, col) would, duplicates summed in insertion
//! order, so the matrix is a pure function of the logical insertion sequence. (Changed from the lift:
//! the global stable sort is a row-bucket pass plus a stable sort of each row by column, the same order
//! at a fraction of the cost; a garment's assembly spent a quarter of its time sorting.)

pub mod direct;
pub mod precond;

pub use direct::{
    DirectOrdering, FactorStats, Inertia, LdltError, LdltFactor, LdltOptions, SymbolicLdlt,
    amd_order,
};

/// Triplet staging buffer; duplicate (row, col) entries accumulate.
#[derive(Debug, Clone, Default)]
pub struct Coo {
    nrows: usize,
    ncols: usize,
    rows: Vec<usize>,
    cols: Vec<usize>,
    vals: Vec<f64>,
}

impl Coo {
    /// Empty staging buffer with fixed dimensions.
    #[must_use]
    pub fn new(nrows: usize, ncols: usize) -> Coo {
        Coo {
            nrows,
            ncols,
            rows: Vec::new(),
            cols: Vec::new(),
            vals: Vec::new(),
        }
    }

    /// Stages one contribution.
    ///
    /// # Panics
    /// On an out-of-range index (a mis-indexed element is a programmer error).
    pub fn push(&mut self, row: usize, col: usize, val: f64) {
        assert!(
            row < self.nrows && col < self.ncols,
            "coo push ({row},{col}) outside {}x{}",
            self.nrows,
            self.ncols
        );
        self.rows.push(row);
        self.cols.push(col);
        self.vals.push(val);
    }

    /// Number of staged triplets.
    #[must_use]
    pub fn staged(&self) -> usize {
        self.vals.len()
    }

    /// Canonical deterministic assembly into CSR.
    #[must_use]
    pub fn assemble(&self) -> Csr {
        let nnz_staged = self.vals.len();
        // Bucket by row in insertion order, then sort each row stably by column: the order of a
        // stable sort by (row, col).
        let mut start = vec![0usize; self.nrows + 1];
        for &r in &self.rows {
            start[r + 1] += 1;
        }
        for r in 0..self.nrows {
            start[r + 1] += start[r];
        }
        let mut fill = start.clone();
        let mut order = vec![0usize; nnz_staged];
        for (i, &r) in self.rows.iter().enumerate() {
            order[fill[r]] = i;
            fill[r] += 1;
        }
        for r in 0..self.nrows {
            order[start[r]..start[r + 1]].sort_by_key(|&i| self.cols[i]);
        }
        let mut row_ptr = vec![0usize; self.nrows + 1];
        let mut col_idx = Vec::new();
        let mut vals = Vec::new();
        let mut i = 0;
        while i < nnz_staged {
            let (r, c) = (self.rows[order[i]], self.cols[order[i]]);
            let mut acc = self.vals[order[i]];
            i += 1;
            while i < nnz_staged && self.rows[order[i]] == r && self.cols[order[i]] == c {
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
        Csr {
            nrows: self.nrows,
            ncols: self.ncols,
            row_ptr,
            col_idx,
            vals,
        }
    }
}

/// A fixed CSR pattern filled in place. Each value goes straight to its slot; a slot starts at −0.0
/// (the identity of IEEE addition: −0.0 + v = v for every v) and sums its contributions in insertion
/// order, so the result equals [`Coo::assemble`] of the same insertion sequence bit for bit, without
/// staging or sorting triplets. Every slot of the pattern must receive at least one contribution for
/// that equality (an untouched slot stays −0.0 where `Coo` would have no entry).
#[derive(Debug, Clone)]
pub struct CsrFill {
    csr: Csr,
}

impl CsrFill {
    /// The pattern of `pattern`, every value −0.0.
    #[must_use]
    pub fn new(pattern: &Csr) -> CsrFill {
        let mut csr = pattern.clone();
        csr.vals.fill(-0.0);
        CsrFill { csr }
    }

    /// Adds `val` at (row, col).
    ///
    /// # Panics
    /// When (row, col) is not in the pattern (a mis-indexed element is a programmer error).
    pub fn add(&mut self, row: usize, col: usize, val: f64) {
        let (lo, hi) = (self.csr.row_ptr[row], self.csr.row_ptr[row + 1]);
        let k = self.csr.col_idx[lo..hi]
            .binary_search(&col)
            .unwrap_or_else(|_| panic!("csr fill: ({row},{col}) is not in the pattern"));
        self.csr.vals[lo + k] += val;
    }

    /// The filled matrix.
    #[must_use]
    pub fn finish(self) -> Csr {
        self.csr
    }
}

/// Compressed sparse row; within each row, column indices are strictly ascending.
#[derive(Debug, Clone, PartialEq)]
pub struct Csr {
    nrows: usize,
    ncols: usize,
    row_ptr: Vec<usize>,
    col_idx: Vec<usize>,
    vals: Vec<f64>,
}

impl Csr {
    /// Builds from raw parts, validating the canonical invariant.
    ///
    /// # Panics
    /// On a malformed or non-canonical structure.
    #[must_use]
    pub fn from_parts(
        nrows: usize,
        ncols: usize,
        row_ptr: Vec<usize>,
        col_idx: Vec<usize>,
        vals: Vec<f64>,
    ) -> Csr {
        assert_eq!(
            row_ptr.len(),
            nrows + 1,
            "row_ptr must have nrows+1 entries"
        );
        assert_eq!(row_ptr[0], 0, "row_ptr must start at 0");
        assert_eq!(row_ptr[nrows], col_idx.len(), "row_ptr end must equal nnz");
        assert_eq!(col_idx.len(), vals.len(), "col_idx/vals length mismatch");
        for r in 0..nrows {
            assert!(
                row_ptr[r] <= row_ptr[r + 1],
                "row_ptr must be monotone (row {r})"
            );
            let cols = &col_idx[row_ptr[r]..row_ptr[r + 1]];
            for w in cols.windows(2) {
                assert!(w[0] < w[1], "row {r}: columns must be strictly ascending");
            }
            if let Some(&last) = cols.last() {
                assert!(last < ncols, "row {r}: column {last} out of range {ncols}");
            }
        }
        Csr {
            nrows,
            ncols,
            row_ptr,
            col_idx,
            vals,
        }
    }

    /// The n × n identity.
    #[must_use]
    pub fn identity(n: usize) -> Csr {
        Csr {
            nrows: n,
            ncols: n,
            row_ptr: (0..=n).collect(),
            col_idx: (0..n).collect(),
            vals: vec![1.0; n],
        }
    }

    /// Rows.
    #[must_use]
    pub fn nrows(&self) -> usize {
        self.nrows
    }

    /// Columns.
    #[must_use]
    pub fn ncols(&self) -> usize {
        self.ncols
    }

    /// Stored nonzeros.
    #[must_use]
    pub fn nnz(&self) -> usize {
        self.vals.len()
    }

    /// One row's (columns, values).
    #[must_use]
    pub fn row(&self, r: usize) -> (&[usize], &[f64]) {
        let span = self.row_ptr[r]..self.row_ptr[r + 1];
        (&self.col_idx[span.clone()], &self.vals[span])
    }

    /// Value access; zero if not stored.
    #[must_use]
    pub fn get(&self, r: usize, c: usize) -> f64 {
        let (cols, vals) = self.row(r);
        cols.binary_search(&c).map_or(0.0, |k| vals[k])
    }

    /// `y = A·x`, each row accumulated in ascending-column order with `mul_add`.
    ///
    /// # Panics
    /// On mismatched lengths.
    pub fn spmv(&self, x: &[f64], y: &mut [f64]) {
        assert_eq!(x.len(), self.ncols, "spmv: x length must equal ncols");
        assert_eq!(y.len(), self.nrows, "spmv: y length must equal nrows");
        for (r, out) in y.iter_mut().enumerate() {
            let (cols, vals) = self.row(r);
            let mut acc = 0.0f64;
            for (&c, &v) in cols.iter().zip(vals) {
                acc = v.mul_add(x[c], acc);
            }
            *out = acc;
        }
    }

    /// Dense expansion (tests).
    #[must_use]
    pub fn to_dense(&self) -> Vec<f64> {
        let mut d = vec![0.0; self.nrows * self.ncols];
        for r in 0..self.nrows {
            let (cols, vals) = self.row(r);
            for (&c, &v) in cols.iter().zip(vals) {
                d[r * self.ncols + c] = v;
            }
        }
        d
    }

    /// The stored values, in row-major CSR order (the pattern is unchanged).
    pub fn values_mut(&mut self) -> &mut [f64] {
        &mut self.vals
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The 5-point Laplacian on an n × n grid (the lifted solver tests use it).
    pub(crate) fn laplacian_2d(n: usize) -> Csr {
        let dim = n * n;
        let mut coo = Coo::new(dim, dim);
        for i in 0..n {
            for j in 0..n {
                let u = i * n + j;
                coo.push(u, u, 4.0);
                if i > 0 {
                    coo.push(u, u - n, -1.0);
                }
                if i + 1 < n {
                    coo.push(u, u + n, -1.0);
                }
                if j > 0 {
                    coo.push(u, u - 1, -1.0);
                }
                if j + 1 < n {
                    coo.push(u, u + 1, -1.0);
                }
            }
        }
        coo.assemble()
    }

    #[test]
    fn fill_equals_coo_bit_for_bit() {
        // duplicates in an awkward order, signed zeros and values whose sum depends on the order
        let pushes: [(usize, usize, f64); 9] = [
            (1, 0, 1e16),
            (0, 0, -0.0),
            (1, 0, 1.0),
            (0, 1, 0.1),
            (1, 0, -1e16),
            (0, 1, 0.2),
            (1, 1, -0.0),
            (1, 1, -0.0),
            (0, 1, 0.3),
        ];
        let mut coo = Coo::new(2, 2);
        for &(r, c, v) in &pushes {
            coo.push(r, c, v);
        }
        let want = coo.assemble();
        let mut fill = CsrFill::new(&want);
        for &(r, c, v) in &pushes {
            fill.add(r, c, v);
        }
        let got = fill.finish();
        assert_eq!(got.row_ptr, want.row_ptr);
        assert_eq!(got.col_idx, want.col_idx);
        let bits = |c: &Csr| c.vals.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&got), bits(&want), "{:?} vs {:?}", got.vals, want.vals);
    }

    #[test]
    fn assembly_accumulates_duplicates_in_order() {
        let mut coo = Coo::new(2, 2);
        coo.push(1, 0, 1.0);
        coo.push(0, 1, 2.0);
        coo.push(1, 0, 3.0);
        let a = coo.assemble();
        assert_eq!(a.get(1, 0), 4.0);
        assert_eq!(a.get(0, 1), 2.0);
        assert_eq!(a.nnz(), 2);
        let mut y = [0.0; 2];
        a.spmv(&[1.0, 1.0], &mut y);
        assert_eq!(y, [2.0, 4.0]);
    }

    #[test]
    fn given_permutation_solves_like_amd() {
        let a = laplacian_2d(6);
        let b: Vec<f64> = (0..36).map(|i| f64::from(i) * 0.1 - 1.0).collect();
        let s1 = SymbolicLdlt::analyze(&a, DirectOrdering::Amd).unwrap();
        let x1 = s1.factor(&a, &LdltOptions::default()).unwrap().solve(&b);
        let perm: Vec<usize> = (0..36).rev().collect();
        let s2 = SymbolicLdlt::analyze_with_permutation(&a, perm).unwrap();
        let x2 = s2.factor(&a, &LdltOptions::default()).unwrap().solve(&b);
        for (p, q) in x1.iter().zip(&x2) {
            assert!((p - q).abs() < 1e-12);
        }
    }
}
