//! Pure-Rust `#![forbid(unsafe_code)]`, zero-`mul_add` sparse symmetric
//! direct multifrontal LDLT solver (Amestoy-Davis-Duff AMD + Duff-Reid 1x1/2x2
//! threshold pivoting + delayed-pivot migration), deterministic Jacobi and
//! block-Jacobi PCG fallback, and small dense partial-pivoting LU solver.

use core::cmp::Reverse;
use std::collections::BinaryHeap;

use super::{CodimError, DetMath};

/// Canonical sorted full structurally symmetric Compressed Sparse Row matrix.
///
/// Both upper and lower triangles must be stored with strictly ascending column
/// indices per row so [`SymbolicLdlt::analyze`] can enforce full structural
/// symmetry via binary search.
#[derive(Clone, Debug, PartialEq)]
pub struct SymCsr {
    /// Number of rows.
    pub nrows: usize,
    /// Number of columns.
    pub ncols: usize,
    /// Row pointer array of length `nrows + 1`.
    pub row_ptr: Vec<usize>,
    /// Strictly ascending column indices within each row.
    pub col_idx: Vec<usize>,
    /// Numeric entries parallel to `col_idx`.
    pub vals: Vec<f64>,
}

impl SymCsr {
    /// Construct a [`SymCsr`] from raw CSR arrays, validating dimensions and
    /// strictly ascending in-bounds column indices per row.
    pub fn from_raw(
        nrows: usize,
        ncols: usize,
        row_ptr: Vec<usize>,
        col_idx: Vec<usize>,
        vals: Vec<f64>,
    ) -> Result<Self, CodimError> {
        if row_ptr.len() != nrows + 1 || col_idx.len() != vals.len() {
            return Err(CodimError::InvalidTopology {
                detail: "CSR array lengths inconsistent with nrows/nnz",
            });
        }
        if row_ptr[0] != 0 || row_ptr[nrows] != col_idx.len() {
            return Err(CodimError::InvalidTopology {
                detail: "CSR row_ptr endpoints invalid",
            });
        }
        for r in 0..nrows {
            let start = row_ptr[r];
            let end = row_ptr[r + 1];
            if start > end || end > col_idx.len() {
                return Err(CodimError::InvalidTopology {
                    detail: "CSR row_ptr not monotonic",
                });
            }
            let mut prev = None;
            for &c in &col_idx[start..end] {
                if c >= ncols {
                    return Err(CodimError::InvalidTopology {
                        detail: "CSR column index out of bounds",
                    });
                }
                if let Some(p) = prev
                    && c <= p
                {
                    return Err(CodimError::InvalidTopology {
                        detail: "CSR row column indices must be strictly ascending without duplicates",
                    });
                }
                prev = Some(c);
            }
        }
        Ok(Self {
            nrows,
            ncols,
            row_ptr,
            col_idx,
            vals,
        })
    }

    /// Assemble a square `n x n` matrix from `(row, col, val)` triplets in
    /// deterministic sorted `(row, col)` order, summing duplicates.
    ///
    /// Note: callers supplying only one triangle should mirror off-diagonal
    /// triplets (or call [`Self::from_lower_or_symmetric_triplets`]).
    pub fn from_triplets(n: usize, triplets: &[(usize, usize, f64)]) -> Result<Self, CodimError> {
        let mut sorted: Vec<(usize, usize, f64)> = Vec::with_capacity(triplets.len());
        for &(r, c, v) in triplets {
            if r >= n || c >= n {
                return Err(CodimError::InvalidTopology {
                    detail: "triplet index out of matrix bounds",
                });
            }
            if !v.is_finite() {
                return Err(CodimError::NonFiniteEntry { row: r, col: c });
            }
            sorted.push((r, c, v));
        }
        sorted.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        let mut row_ptr = vec![0usize; n + 1];
        let mut col_idx = Vec::with_capacity(sorted.len());
        let mut vals = Vec::with_capacity(sorted.len());
        let mut idx = 0usize;
        for r in 0..n {
            row_ptr[r] = col_idx.len();
            while idx < sorted.len() && sorted[idx].0 == r {
                let c = sorted[idx].1;
                let mut sum = sorted[idx].2;
                idx += 1;
                while idx < sorted.len() && sorted[idx].0 == r && sorted[idx].1 == c {
                    sum += sorted[idx].2;
                    idx += 1;
                }
                col_idx.push(c);
                vals.push(sum);
            }
        }
        row_ptr[n] = col_idx.len();
        Ok(Self {
            nrows: n,
            ncols: n,
            row_ptr,
            col_idx,
            vals,
        })
    }

    /// Number of stored nonzeros.
    #[must_use]
    pub fn nnz(&self) -> usize {
        self.vals.len()
    }

    /// Column indices and values of row `r`.
    #[must_use]
    pub fn row(&self, r: usize) -> (&[usize], &[f64]) {
        let s = self.row_ptr[r];
        let e = self.row_ptr[r + 1];
        (&self.col_idx[s..e], &self.vals[s..e])
    }

    /// Deterministic sparse matrix-vector product `y = A * x`.
    #[must_use]
    pub fn spmv(&self, x: &[f64]) -> Vec<f64> {
        let mut y = vec![0.0; self.nrows];
        for (r, yr) in y.iter_mut().enumerate() {
            let (cols, vals) = self.row(r);
            let mut acc = 0.0;
            for (&c, &v) in cols.iter().zip(vals.iter()) {
                acc += v * x[c];
            }
            *yr = acc;
        }
        y
    }
}

/// Fill-reducing ordering strategy for [`SymbolicLdlt::analyze`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectOrdering {
    /// Identity permutation.
    Natural,
    /// Amestoy-Davis-Duff approximate minimum degree ordering ([`amd_order`]).
    Amd,
}

/// Options for numeric multifrontal LDLT factorization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LdltOptions {
    /// Duff-Reid relative pivot threshold `u` in `(0, 0.5]`. Default `0.01`.
    pub pivot_threshold: f64,
    /// Permit symmetric 2x2 pivots for indefinite matrices. Default `true`.
    pub allow_2x2: bool,
    /// Relative zero-pivot floor `zero_pivot_rel * max|A|`. Default `256 * eps`.
    pub zero_pivot_rel: f64,
}

impl Default for LdltOptions {
    fn default() -> Self {
        Self {
            pivot_threshold: 0.01,
            allow_2x2: true,
            zero_pivot_rel: 256.0 * f64::EPSILON,
        }
    }
}

/// Sylvester inertia of the factored matrix: sign counts of `D`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inertia {
    /// Number of positive eigenvalues.
    pub positive: usize,
    /// Number of negative eigenvalues.
    pub negative: usize,
}

/// Accounting statistics for one numeric factorization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FactorStats {
    /// Matrix dimension.
    pub n: usize,
    /// Stored nonzeros in input matrix.
    pub nnz_a: usize,
    /// Nonzeros in unit-lower `L` including diagonal.
    pub nnz_l: usize,
    /// `nnz_l / nnz_a`.
    pub fill_ratio: f64,
    /// Approximate floating-point operation count.
    pub flops: u64,
    /// Peak bytes of live frontal + update storage.
    pub peak_front_bytes: usize,
    /// Number of fundamental supernodes.
    pub supernodes: usize,
    /// Count of 1x1 pivots taken.
    pub pivots_1x1: usize,
    /// Count of 2x2 pivots taken.
    pub pivots_2x2: usize,
    /// Count of delayed column hops across supernodes.
    pub pivots_delayed: usize,
    /// Maximum frontal matrix dimension encountered.
    pub max_front_dim: usize,
}

const AMD_HEAP_SLACK: usize = 4;
const AMD_HEAP_SLACK_FLOOR: usize = 1024;
const NONE: usize = usize::MAX;

fn a_contains(a: &SymCsr, r: usize, c: usize) -> bool {
    let (cols, _) = a.row(r);
    cols.binary_search(&c).is_ok()
}

/// Deterministic Amestoy-Davis-Duff approximate minimum degree ordering on the
/// symmetric pattern of `a` with smallest-index tie-breaking.
#[must_use]
#[allow(clippy::needless_range_loop)]
#[allow(clippy::too_many_lines)]
pub fn amd_order(a: &SymCsr) -> Vec<usize> {
    assert_eq!(a.nrows, a.ncols, "amd_order: matrix must be square");
    let n = a.nrows;
    if n == 0 {
        return Vec::new();
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for r in 0..n {
        let (cols, _) = a.row(r);
        for &c in cols {
            if c != r {
                adj[r].push(c);
            }
        }
    }
    for r in 0..n {
        let (cols, _) = a.row(r);
        for &c in cols {
            if c != r && !a_contains(a, c, r) {
                adj[c].push(r);
            }
        }
    }
    for l in &mut adj {
        l.sort_unstable();
        l.dedup();
    }

    let mut elems_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut elem_vars: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut elem_alive = vec![false; n];
    let mut var_alive = vec![true; n];
    let mut degree: Vec<usize> = adj.iter().map(Vec::len).collect();

    let mut heap: BinaryHeap<Reverse<(usize, usize)>> =
        (0..n).map(|i| Reverse((degree[i], i))).collect();

    let mut perm = Vec::with_capacity(n);
    let mut marked = vec![usize::MAX; n];
    let mut w_stamp = vec![usize::MAX; n];
    let mut w_val = vec![0usize; n];

    for step in 0..n {
        if heap.len()
            > AMD_HEAP_SLACK
                .saturating_mul(n - step)
                .saturating_add(AMD_HEAP_SLACK_FLOOR)
        {
            heap = (0..n)
                .filter(|&i| var_alive[i])
                .map(|i| Reverse((degree[i], i)))
                .collect();
        }
        let p = loop {
            let Reverse((d, i)) = heap.pop().expect("amd: heap exhausted early");
            if var_alive[i] && degree[i] == d {
                break i;
            }
        };
        var_alive[p] = false;
        perm.push(p);

        let mut lp: Vec<usize> = adj[p].iter().copied().filter(|&v| var_alive[v]).collect();
        for &e in &elems_of[p] {
            if elem_alive[e] {
                lp.extend(
                    elem_vars[e]
                        .iter()
                        .copied()
                        .filter(|&v| var_alive[v] && v != p),
                );
            }
        }
        lp.sort_unstable();
        lp.dedup();
        for &e in &elems_of[p] {
            elem_alive[e] = false;
            elem_vars[e] = Vec::new();
        }
        for &v in &lp {
            marked[v] = step;
        }

        for &i in &lp {
            for &e in &elems_of[i] {
                if elem_alive[e] {
                    if w_stamp[e] != step {
                        w_stamp[e] = step;
                        w_val[e] = elem_vars[e].iter().filter(|&&v| var_alive[v]).count();
                    }
                    w_val[e] -= 1;
                }
            }
        }
        for &i in &lp {
            for &e in &elems_of[i] {
                if elem_alive[e] && w_stamp[e] == step && w_val[e] == 0 {
                    elem_alive[e] = false;
                    elem_vars[e] = Vec::new();
                }
            }
        }

        let remaining = n - step - 1;
        for &i in &lp {
            adj[i].retain(|&v| var_alive[v] && marked[v] != step);
            elems_of[i].retain(|&e| elem_alive[e]);
            elems_of[i].push(p);
            let mut d = adj[i].len() + (lp.len() - 1);
            for &e in &elems_of[i] {
                if e != p && w_stamp[e] == step {
                    d += w_val[e];
                }
            }
            let d = d.min(remaining);
            degree[i] = d;
            heap.push(Reverse((d, i)));
        }
        elem_vars[p] = lp;
        elem_alive[p] = true;
        elems_of[p].clear();
    }
    perm
}

#[derive(Debug, Clone)]
struct SnodeSym {
    col_start: usize,
    width: usize,
    panel: Vec<usize>,
    parent: usize,
    children: Vec<usize>,
}

/// Reusable symbolic LDLT analysis for a fixed full structurally symmetric
/// sparsity pattern.
#[derive(Debug, Clone)]
pub struct SymbolicLdlt {
    n: usize,
    perm: Vec<usize>,
    iperm: Vec<usize>,
    snodes: Vec<SnodeSym>,
    nnz_l: usize,
    pat_row_ptr: Vec<usize>,
    pat_col_idx: Vec<usize>,
}

impl SymbolicLdlt {
    /// Analyze the symmetric pattern of `a`. Validates squareness and full
    /// structural symmetry (`StructurallyAsymmetric { row, col }`) prior to
    /// ordering.
    pub fn analyze(a: &SymCsr, ordering: DirectOrdering) -> Result<Self, CodimError> {
        if a.nrows != a.ncols {
            return Err(CodimError::NotSquare {
                nrows: a.nrows,
                ncols: a.ncols,
            });
        }
        let n = a.nrows;
        for r in 0..n {
            let (cols, _) = a.row(r);
            for &c in cols {
                if !a_contains(a, c, r) {
                    return Err(CodimError::StructurallyAsymmetric { row: r, col: c });
                }
            }
        }
        let base_perm = match ordering {
            DirectOrdering::Natural => (0..n).collect::<Vec<usize>>(),
            DirectOrdering::Amd => amd_order(a),
        };
        Self::analyze_with_perm(a, &base_perm)
    }

    /// Analyze using an explicit caller permutation (still validates symmetry).
    pub fn analyze_with_perm(a: &SymCsr, base_perm: &[usize]) -> Result<Self, CodimError> {
        if a.nrows != a.ncols {
            return Err(CodimError::NotSquare {
                nrows: a.nrows,
                ncols: a.ncols,
            });
        }
        let n = a.nrows;
        if base_perm.len() != n {
            return Err(CodimError::InvalidTopology {
                detail: "permutation length does not match matrix dimension",
            });
        }
        for r in 0..n {
            let (cols, _) = a.row(r);
            for &c in cols {
                if !a_contains(a, c, r) {
                    return Err(CodimError::StructurallyAsymmetric { row: r, col: c });
                }
            }
        }
        let mut seen = vec![false; n];
        let mut base_iperm = vec![0usize; n];
        for (k, &o) in base_perm.iter().enumerate() {
            if o >= n || seen[o] {
                return Err(CodimError::InvalidTopology {
                    detail: "invalid permutation passed to SymbolicLdlt",
                });
            }
            seen[o] = true;
            base_iperm[o] = k;
        }
        let ladj = build_lower_adj(a, &base_iperm);
        let parent0 = etree(&ladj);
        let post = postorder(&parent0);
        let perm: Vec<usize> = post.iter().map(|&k| base_perm[k]).collect();
        let mut iperm = vec![0usize; n];
        for (k, &o) in perm.iter().enumerate() {
            iperm[o] = k;
        }
        let ladj = build_lower_adj(a, &iperm);
        let parent = etree(&ladj);
        let colstruct = col_structs(&ladj, &parent);

        let mut snodes: Vec<SnodeSym> = Vec::new();
        let mut snode_of = vec![NONE; n.max(1)];
        let mut j = 0usize;
        while j < n {
            let start = j;
            while j + 1 < n
                && parent[j] == j + 1
                && colstruct[j].len() == colstruct[j + 1].len() + 1
                && colstruct[j].first() == Some(&(j + 1))
                && colstruct[j][1..] == colstruct[j + 1][..]
            {
                j += 1;
            }
            let width = j - start + 1;
            let panel: Vec<usize> = colstruct[start][width - 1..].to_vec();
            let id = snodes.len();
            for slot in &mut snode_of[start..=j] {
                *slot = id;
            }
            snodes.push(SnodeSym {
                col_start: start,
                width,
                panel,
                parent: NONE,
                children: Vec::new(),
            });
            j += 1;
        }
        for id in 0..snodes.len() {
            let last = snodes[id].col_start + snodes[id].width - 1;
            let p = parent[last];
            if p != NONE {
                let ps = snode_of[p];
                snodes[id].parent = ps;
                snodes[ps].children.push(id);
            }
        }
        let nnz_l = n + colstruct.iter().map(Vec::len).sum::<usize>();
        let (pat_row_ptr, pat_col_idx) = pattern_of(a);
        Ok(Self {
            n,
            perm,
            iperm,
            snodes,
            nnz_l,
            pat_row_ptr,
            pat_col_idx,
        })
    }

    /// Matrix dimension.
    #[must_use]
    pub fn n(&self) -> usize {
        self.n
    }

    /// Predicted nonzeros of `L` including unit diagonal.
    #[must_use]
    pub fn nnz_l(&self) -> usize {
        self.nnz_l
    }

    /// Number of fundamental supernodes.
    #[must_use]
    pub fn supernode_count(&self) -> usize {
        self.snodes.len()
    }

    /// Elimination permutation (`perm[k]` = original index eliminated `k`-th).
    #[must_use]
    pub fn permutation(&self) -> &[usize] {
        &self.perm
    }

    /// Numeric multifrontal factorization `P * A * P^T = L * D * L^T` with
    /// zero `.mul_add` and fail-closed `PivotBreakdown`.
    #[allow(clippy::too_many_lines)]
    pub fn factor(&self, a: &SymCsr, opts: &LdltOptions) -> Result<LdltFactor, CodimError> {
        if !opts.pivot_threshold.is_finite()
            || opts.pivot_threshold <= 0.0
            || opts.pivot_threshold > 0.5
        {
            return Err(CodimError::InvalidSolverOptions {
                field: "pivot_threshold must be in (0, 0.5]",
            });
        }
        if !opts.zero_pivot_rel.is_finite() || opts.zero_pivot_rel <= 0.0 {
            return Err(CodimError::InvalidSolverOptions {
                field: "zero_pivot_rel must be positive",
            });
        }
        if a.nrows != self.n || a.ncols != self.n {
            return Err(CodimError::PatternMismatch);
        }
        if a.row_ptr != self.pat_row_ptr || a.col_idx != self.pat_col_idx {
            return Err(CodimError::PatternMismatch);
        }

        let n = self.n;
        let mut max_abs = 0.0_f64;
        for r in 0..n {
            let (cols, vals) = a.row(r);
            for (&c, &v) in cols.iter().zip(vals) {
                if !v.is_finite() {
                    return Err(CodimError::NonFiniteEntry { row: r, col: c });
                }
                max_abs = max_abs.max(v.abs());
            }
        }
        let zero_tol = opts.zero_pivot_rel * max_abs.max(f64::MIN_POSITIVE);

        let mut stats = FactorStats {
            n,
            nnz_a: a.nnz(),
            nnz_l: self.nnz_l,
            fill_ratio: if a.nnz() == 0 {
                1.0
            } else {
                self.nnz_l as f64 / a.nnz() as f64
            },
            flops: 0,
            peak_front_bytes: 0,
            supernodes: self.snodes.len(),
            pivots_1x1: 0,
            pivots_2x2: 0,
            pivots_delayed: 0,
            max_front_dim: 0,
        };
        let mut nnz_l_actual = 0usize;
        let mut inertia = Inertia {
            positive: 0,
            negative: 0,
        };
        let mut snodes_num: Vec<SnodeNum> = Vec::with_capacity(self.snodes.len());
        let mut updates: Vec<Option<Update>> = vec![None; self.snodes.len()];
        let mut live_update_bytes = 0usize;
        let mut pos = vec![usize::MAX; n];

        for (sid, sym) in self.snodes.iter().enumerate() {
            let w = sym.width;
            let mut delayed: Vec<usize> = Vec::new();
            for &ch in &sym.children {
                if let Some(up) = updates[ch].as_ref() {
                    delayed.extend(up.vars.iter().copied().filter(|&v| v < sym.col_start));
                }
            }
            delayed.sort_unstable();
            let mut fs: Vec<usize> = delayed;
            fs.extend(sym.col_start..sym.col_start + w);
            let wf = fs.len();
            let m = wf + sym.panel.len();
            stats.max_front_dim = stats.max_front_dim.max(m);

            let mut front_local: Vec<usize> = fs;
            front_local.extend(sym.panel.iter().copied());
            for (idx, &v) in front_local.iter().enumerate() {
                pos[v] = idx;
            }
            let mut f = vec![0.0_f64; m * m];
            let front_bytes = m * m * 8;
            stats.peak_front_bytes = stats.peak_front_bytes.max(front_bytes + live_update_bytes);

            for j in sym.col_start..sym.col_start + w {
                let c = pos[j];
                let orig = self.perm[j];
                let (cols, vals) = a.row(orig);
                for (&oc, &v) in cols.iter().zip(vals) {
                    let pi = self.iperm[oc];
                    if pi >= j {
                        let li = pos[pi];
                        f[li + c * m] += v;
                        if li != c {
                            f[c + li * m] += v;
                        }
                    }
                }
            }
            for &ch in &sym.children {
                let up = updates[ch].take().expect("child update consumed twice");
                live_update_bytes -= up.data.len() * 8;
                let map: Vec<usize> = up.vars.iter().map(|&v| pos[v]).collect();
                let q = up.vars.len();
                for cj in 0..q {
                    let dst_c = map[cj];
                    for ri in 0..q {
                        f[map[ri] + dst_c * m] += up.data[ri + cj * q];
                    }
                }
            }
            for &v in &front_local {
                pos[v] = usize::MAX;
            }

            let mut local2var = front_local;
            let mut dblocks: Vec<DBlock> = Vec::new();
            let mut k = 0usize;
            while k < wf {
                let choice = choose_pivot(&f, m, wf, k, opts, zero_tol);
                match choice {
                    PivotChoice::One(q) => {
                        if q != k {
                            swap_sym(&mut f, m, k, q);
                            local2var.swap(k, q);
                        }
                        let d = f[k + k * m];
                        if d > 0.0 {
                            inertia.positive += 1;
                        } else {
                            inertia.negative += 1;
                        }
                        stats.pivots_1x1 += 1;
                        dblocks.push(DBlock::Single(d));
                        for i in k + 1..m {
                            f[i + k * m] /= d;
                        }
                        for jc in k + 1..m {
                            let cj = d * f[jc + k * m];
                            for i in jc..m {
                                let t = f[i + k * m] * cj;
                                f[i + jc * m] -= t;
                                if i != jc {
                                    f[jc + i * m] = f[i + jc * m];
                                }
                            }
                        }
                        stats.flops += ((m - k) as u64) * ((m - k) as u64);
                        k += 1;
                    }
                    PivotChoice::Two(pq, qq) => {
                        if pq != k {
                            swap_sym(&mut f, m, k, pq);
                            local2var.swap(k, pq);
                        }
                        let qq = if qq == k { pq } else { qq };
                        if qq != k + 1 {
                            swap_sym(&mut f, m, k + 1, qq);
                            local2var.swap(k + 1, qq);
                        }
                        let e11 = f[k + k * m];
                        let e21 = f[(k + 1) + k * m];
                        let e22 = f[(k + 1) + (k + 1) * m];
                        let det = e11 * e22 - e21 * e21;
                        if det < 0.0 {
                            inertia.positive += 1;
                            inertia.negative += 1;
                        } else if e11 + e22 > 0.0 {
                            inertia.positive += 2;
                        } else {
                            inertia.negative += 2;
                        }
                        stats.pivots_2x2 += 1;
                        dblocks.push(DBlock::Pair(e11, e21, e22));
                        let inv11 = e22 / det;
                        let inv21 = -e21 / det;
                        let inv22 = e11 / det;
                        let acol: Vec<f64> = (k + 2..m).map(|i| f[i + k * m]).collect();
                        let bcol: Vec<f64> = (k + 2..m).map(|i| f[i + (k + 1) * m]).collect();
                        for (idx, i) in (k + 2..m).enumerate() {
                            let l1 = acol[idx] * inv11 + bcol[idx] * inv21;
                            let l2 = acol[idx] * inv21 + bcol[idx] * inv22;
                            f[i + k * m] = l1;
                            f[i + (k + 1) * m] = l2;
                        }
                        for (jdx, jc) in (k + 2..m).enumerate() {
                            let aj = acol[jdx];
                            let bj = bcol[jdx];
                            for i in jc..m {
                                let t = f[i + k * m] * aj + f[i + (k + 1) * m] * bj;
                                f[i + jc * m] -= t;
                                if i != jc {
                                    f[jc + i * m] = f[i + jc * m];
                                }
                            }
                        }
                        stats.flops += 2 * ((m - k) as u64) * ((m - k) as u64);
                        f[(k + 1) + k * m] = 0.0;
                        f[k + (k + 1) * m] = 0.0;
                        k += 2;
                    }
                    PivotChoice::Fail { best_abs, required } => {
                        if sym.parent == NONE {
                            return Err(CodimError::PivotBreakdown {
                                orig_col: self.perm[local2var[k]],
                                permuted_col: local2var[k],
                                best_abs,
                                required,
                                two_by_two_enabled: opts.allow_2x2,
                            });
                        }
                        break;
                    }
                }
            }
            let k_elim = k;
            stats.pivots_delayed += wf - k_elim;

            let mut l = vec![0.0_f64; m * k_elim];
            for c in 0..k_elim {
                l[c + c * m] = 1.0;
                for i in c + 1..m {
                    l[i + c * m] = f[i + c * m];
                }
                nnz_l_actual += m - c;
            }
            let remaining = m - k_elim;
            if sym.parent != NONE && remaining > 0 {
                let mut data = vec![0.0_f64; remaining * remaining];
                for cj in 0..remaining {
                    for ri in 0..remaining {
                        data[ri + cj * remaining] = f[(k_elim + ri) + (k_elim + cj) * m];
                    }
                }
                live_update_bytes += data.len() * 8;
                stats.peak_front_bytes = stats.peak_front_bytes.max(live_update_bytes);
                updates[sid] = Some(Update {
                    vars: local2var[k_elim..].to_vec(),
                    data,
                });
            }
            snodes_num.push(SnodeNum {
                block_vars: local2var[..k_elim].to_vec(),
                panel_vars: local2var[k_elim..].to_vec(),
                l,
                d: dblocks,
            });
        }

        stats.nnz_l = nnz_l_actual;
        stats.fill_ratio = if a.nnz() == 0 {
            1.0
        } else {
            stats.nnz_l as f64 / a.nnz() as f64
        };
        Ok(LdltFactor {
            n,
            perm: self.perm.clone(),
            snodes: snodes_num,
            inertia,
            stats,
        })
    }
}

fn pattern_of(a: &SymCsr) -> (Vec<usize>, Vec<usize>) {
    (a.row_ptr.clone(), a.col_idx.clone())
}

fn build_lower_adj(a: &SymCsr, iperm: &[usize]) -> Vec<Vec<usize>> {
    let n = a.nrows;
    let mut ladj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for r in 0..n {
        let (cols, _) = a.row(r);
        let pi = iperm[r];
        for &c in cols {
            let pj = iperm[c];
            if pj < pi {
                ladj[pi].push(pj);
            }
        }
    }
    for l in &mut ladj {
        l.sort_unstable();
    }
    ladj
}

fn etree(ladj: &[Vec<usize>]) -> Vec<usize> {
    let n = ladj.len();
    let mut parent = vec![NONE; n];
    let mut ancestor = vec![NONE; n];
    for (i, lower) in ladj.iter().enumerate() {
        for &j in lower {
            let mut t = j;
            while ancestor[t] != NONE && ancestor[t] != i {
                let next = ancestor[t];
                ancestor[t] = i;
                t = next;
            }
            if ancestor[t] == NONE {
                ancestor[t] = i;
                parent[t] = i;
            }
        }
    }
    parent
}

fn postorder(parent: &[usize]) -> Vec<usize> {
    let n = parent.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut roots = Vec::new();
    for j in 0..n {
        if parent[j] == NONE {
            roots.push(j);
        } else {
            children[parent[j]].push(j);
        }
    }
    let mut post = Vec::with_capacity(n);
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for &r in &roots {
        stack.push((r, 0));
        while let Some(&(node, next)) = stack.last() {
            if next < children[node].len() {
                stack.last_mut().expect("nonempty stack").1 += 1;
                stack.push((children[node][next], 0));
            } else {
                post.push(node);
                stack.pop();
            }
        }
    }
    post
}

fn col_structs(ladj: &[Vec<usize>], parent: &[usize]) -> Vec<Vec<usize>> {
    let n = ladj.len();
    let mut cs: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut stamp = vec![NONE; n];
    for i in 0..n {
        stamp[i] = i;
        for &j in &ladj[i] {
            let mut t = j;
            while t != i && stamp[t] != i {
                cs[t].push(i);
                stamp[t] = i;
                let p = parent[t];
                if p == NONE {
                    break;
                }
                t = p;
            }
        }
    }
    cs
}

enum PivotChoice {
    One(usize),
    Two(usize, usize),
    Fail { best_abs: f64, required: f64 },
}

fn gamma_of(f: &[f64], m: usize, k: usize, c: usize) -> f64 {
    let mut g = 0.0_f64;
    for i in k..m {
        if i != c {
            g = g.max(f[i + c * m].abs());
        }
    }
    g
}

fn gamma_pair(f: &[f64], m: usize, k: usize, c: usize, other: usize) -> f64 {
    let mut g = 0.0_f64;
    for i in k..m {
        if i != c && i != other {
            g = g.max(f[i + c * m].abs());
        }
    }
    g
}

fn choose_pivot(
    f: &[f64],
    m: usize,
    w: usize,
    k: usize,
    opts: &LdltOptions,
    zero_tol: f64,
) -> PivotChoice {
    let u = opts.pivot_threshold;
    let mut best_abs = 0.0_f64;
    let mut required = zero_tol;
    for q in k..w {
        let dq = f[q + q * m].abs();
        let gq = gamma_of(f, m, k, q);
        best_abs = best_abs.max(dq);
        required = required.max(u * gq).max(zero_tol);
        if dq > zero_tol && dq >= u * gq {
            return PivotChoice::One(q);
        }
    }
    if opts.allow_2x2 && w - k >= 2 {
        let det_tol = zero_tol * zero_tol;
        for p in k..w {
            for q in p + 1..w {
                let e11 = f[p + p * m];
                let e21 = f[q + p * m];
                let e22 = f[q + q * m];
                let det = e11 * e22 - e21 * e21;
                if det.abs() <= det_tol {
                    continue;
                }
                let gp = gamma_pair(f, m, k, p, q);
                let gq = gamma_pair(f, m, k, q, p);
                let g1 = (e22.abs() * gp + e21.abs() * gq) / det.abs();
                let g2 = (e21.abs() * gp + e11.abs() * gq) / det.abs();
                if g1.max(g2) <= 1.0 / u {
                    return PivotChoice::Two(p, q);
                }
            }
        }
    }
    PivotChoice::Fail { best_abs, required }
}

fn swap_sym(f: &mut [f64], m: usize, a: usize, b: usize) {
    if a == b {
        return;
    }
    for r in 0..m {
        f.swap(r + a * m, r + b * m);
    }
    for c in 0..m {
        f.swap(a + c * m, b + c * m);
    }
}

#[derive(Debug, Clone)]
struct Update {
    vars: Vec<usize>,
    data: Vec<f64>,
}

#[derive(Debug, Clone, Copy)]
enum DBlock {
    Single(f64),
    Pair(f64, f64, f64),
}

#[derive(Debug, Clone)]
struct SnodeNum {
    block_vars: Vec<usize>,
    panel_vars: Vec<usize>,
    l: Vec<f64>,
    d: Vec<DBlock>,
}

/// Numeric multifrontal LDLT factorization `P * A * P^T = L * D * L^T`.
#[derive(Debug, Clone)]
pub struct LdltFactor {
    n: usize,
    perm: Vec<usize>,
    snodes: Vec<SnodeNum>,
    inertia: Inertia,
    stats: FactorStats,
}

impl LdltFactor {
    /// Sylvester inertia of `A` (sign counts of `D`).
    #[must_use]
    pub fn inertia(&self) -> Inertia {
        self.inertia
    }

    /// Factorization statistics.
    #[must_use]
    pub fn stats(&self) -> &FactorStats {
        &self.stats
    }

    /// Dimension of the factored matrix.
    #[must_use]
    pub fn n(&self) -> usize {
        self.n
    }

    /// Solve `A * x = b` deterministically using zero `.mul_add`.
    #[must_use]
    pub fn solve(&self, b: &[f64]) -> Vec<f64> {
        assert_eq!(b.len(), self.n, "solve: rhs length must equal n");
        let mut y = vec![0.0_f64; self.n];
        for (k, &o) in self.perm.iter().enumerate() {
            y[k] = b[o];
        }
        for s in &self.snodes {
            let w = s.block_vars.len();
            let m = w + s.panel_vars.len();
            let mut v: Vec<f64> = Vec::with_capacity(m);
            v.extend(s.block_vars.iter().map(|&i| y[i]));
            v.extend(s.panel_vars.iter().map(|&i| y[i]));
            for k in 0..w {
                let vk = v[k];
                for (i, vi) in v.iter_mut().enumerate().skip(k + 1) {
                    *vi -= s.l[i + k * m] * vk;
                }
            }
            for (k, &i) in s.block_vars.iter().enumerate() {
                y[i] = v[k];
            }
            for (k, &i) in s.panel_vars.iter().enumerate() {
                y[i] = v[w + k];
            }
        }
        for s in &self.snodes {
            let mut k = 0usize;
            for blk in &s.d {
                match *blk {
                    DBlock::Single(d) => {
                        y[s.block_vars[k]] /= d;
                        k += 1;
                    }
                    DBlock::Pair(d11, d21, d22) => {
                        let det = d11 * d22 - d21 * d21;
                        let z1 = y[s.block_vars[k]];
                        let z2 = y[s.block_vars[k + 1]];
                        y[s.block_vars[k]] = (d22 * z1 - d21 * z2) / det;
                        y[s.block_vars[k + 1]] = (d11 * z2 - d21 * z1) / det;
                        k += 2;
                    }
                }
            }
        }
        for s in self.snodes.iter().rev() {
            let w = s.block_vars.len();
            let m = w + s.panel_vars.len();
            let mut v: Vec<f64> = Vec::with_capacity(m);
            v.extend(s.block_vars.iter().map(|&i| y[i]));
            v.extend(s.panel_vars.iter().map(|&i| y[i]));
            for k in (0..w).rev() {
                let mut acc = v[k];
                for (i, &vi) in v.iter().enumerate().skip(k + 1) {
                    acc -= s.l[i + k * m] * vi;
                }
                v[k] = acc;
            }
            for (k, &i) in s.block_vars.iter().enumerate() {
                y[i] = v[k];
            }
        }
        let mut x = vec![0.0_f64; self.n];
        for (k, &o) in self.perm.iter().enumerate() {
            x[o] = y[k];
        }
        x
    }
}

/// Output of deterministic Preconditioned Conjugate Gradient solve.
#[derive(Clone, Debug, PartialEq)]
pub struct PcgSolution {
    /// Solution vector `x`.
    pub x: Vec<f64>,
    /// Number of CG iterations executed.
    pub iters: usize,
    /// Achieved relative residual norm `||b - A x||_2 / max(||b||_2, 1)`.
    pub rel_residual: f64,
}

fn dot_slice(a: &[f64], b: &[f64]) -> f64 {
    let mut acc = 0.0;
    for (&x, &y) in a.iter().zip(b.iter()) {
        acc += x * y;
    }
    acc
}

/// Deterministic Jacobi-preconditioned Conjugate Gradient solver for symmetric
/// positive definite [`SymCsr`] systems.
pub fn solve_pcg_jacobi<M: DetMath>(
    a: &SymCsr,
    b: &[f64],
    tol_rel: f64,
    max_iters: usize,
) -> Result<PcgSolution, CodimError> {
    solve_pcg_block_jacobi::<M>(a, b, 1, tol_rel, max_iters)
}

/// Deterministic block-Jacobi-preconditioned Conjugate Gradient solver for
/// symmetric positive definite [`SymCsr`] systems with nodal blocks of size
/// `block_size` (e.g. `1` for scalar Jacobi, `3` or `6` for nodal blocks).
pub fn solve_pcg_block_jacobi<M: DetMath>(
    a: &SymCsr,
    b: &[f64],
    block_size: usize,
    tol_rel: f64,
    max_iters: usize,
) -> Result<PcgSolution, CodimError> {
    if a.nrows != a.ncols {
        return Err(CodimError::NotSquare {
            nrows: a.nrows,
            ncols: a.ncols,
        });
    }
    let n = a.nrows;
    if b.len() != n {
        return Err(CodimError::InvalidTopology {
            detail: "PCG rhs length mismatch",
        });
    }
    if block_size == 0 || !tol_rel.is_finite() || tol_rel <= 0.0 {
        return Err(CodimError::InvalidSolverOptions {
            field: "PCG block_size must be >= 1 and tol_rel > 0",
        });
    }
    if !b.iter().all(|v| v.is_finite()) {
        return Err(CodimError::NonFiniteInput { context: "PCG rhs" });
    }
    if n == 0 {
        return Ok(PcgSolution {
            x: Vec::new(),
            iters: 0,
            rel_residual: 0.0,
        });
    }

    // Build contiguous diagonal blocks (falling back to smaller tail block).
    let mut blocks: Vec<(usize, usize, Vec<f64>)> = Vec::new();
    let mut start = 0usize;
    while start < n {
        let bs = block_size.min(n - start);
        let mut mat = vec![0.0; bs * bs];
        for r_off in 0..bs {
            let r = start + r_off;
            let (cols, vals) = a.row(r);
            for (&c, &v) in cols.iter().zip(vals.iter()) {
                if !v.is_finite() {
                    return Err(CodimError::NonFiniteEntry { row: r, col: c });
                }
                if c >= start && c < start + bs {
                    mat[r_off * bs + (c - start)] = v;
                }
            }
        }
        blocks.push((start, bs, mat));
        start += bs;
    }

    let apply_precond = |r: &[f64]| -> Result<Vec<f64>, CodimError> {
        let mut z = vec![0.0; n];
        for (b_start, bs, mat) in &blocks {
            if *bs == 1 {
                let d = mat[0];
                if d.abs() <= 1e-30 {
                    return Err(CodimError::SingularLinearSystem {
                        pivot_idx: *b_start,
                    });
                }
                z[*b_start] = r[*b_start] / d;
            } else {
                let sol = solve_dense_lu(mat, &r[*b_start..*b_start + *bs], *bs)?;
                z[*b_start..*b_start + *bs].copy_from_slice(&sol);
            }
        }
        Ok(z)
    };

    let b_norm = M::sqrt(dot_slice(b, b)).max(1.0);
    let mut x = vec![0.0; n];
    let mut r = b.to_vec();
    let r_norm0 = M::sqrt(dot_slice(&r, &r));
    if r_norm0 / b_norm <= tol_rel {
        return Ok(PcgSolution {
            x,
            iters: 0,
            rel_residual: r_norm0 / b_norm,
        });
    }

    let mut z = apply_precond(&r)?;
    let mut p = z.clone();
    let mut rz = dot_slice(&r, &z);
    if !rz.is_finite() || rz <= 0.0 {
        return Err(CodimError::PcgDidNotConverge {
            iters: 0,
            rel_residual: r_norm0 / b_norm,
        });
    }

    let mut last_rel = r_norm0 / b_norm;
    for iter in 1..=max_iters {
        let ap = a.spmv(&p);
        let p_ap = dot_slice(&p, &ap);
        if !p_ap.is_finite() || p_ap <= 0.0 {
            return Err(CodimError::PcgDidNotConverge {
                iters: iter,
                rel_residual: last_rel,
            });
        }
        let alpha = rz / p_ap;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        last_rel = M::sqrt(dot_slice(&r, &r)) / b_norm;
        if last_rel <= tol_rel {
            return Ok(PcgSolution {
                x,
                iters: iter,
                rel_residual: last_rel,
            });
        }
        z = apply_precond(&r)?;
        let rz_new = dot_slice(&r, &z);
        if !rz_new.is_finite() || rz_new <= 0.0 {
            return Err(CodimError::PcgDidNotConverge {
                iters: iter,
                rel_residual: last_rel,
            });
        }
        let beta = rz_new / rz;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
        rz = rz_new;
    }

    Err(CodimError::PcgDidNotConverge {
        iters: max_iters,
        rel_residual: last_rel,
    })
}

/// Deterministic small dense row-major `n x n` partial-pivoting LU solver
/// (`A * x = b`, zero `.mul_add`) for asymmetric follower-moment compatibility.
pub fn solve_dense_lu(a: &[f64], b: &[f64], n: usize) -> Result<Vec<f64>, CodimError> {
    if a.len() != n * n || b.len() != n {
        return Err(CodimError::InvalidTopology {
            detail: "dense LU dimensions mismatch",
        });
    }
    if !a.iter().all(|v| v.is_finite()) || !b.iter().all(|v| v.is_finite()) {
        return Err(CodimError::NonFiniteInput {
            context: "solve_dense_lu",
        });
    }
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut lu = a.to_vec();
    let mut rhs = b.to_vec();

    for k in 0..n {
        let mut piv_row = k;
        let mut piv_abs = lu[k * n + k].abs();
        for r in k + 1..n {
            let v = lu[r * n + k].abs();
            if v > piv_abs {
                piv_abs = v;
                piv_row = r;
            }
        }
        if !piv_abs.is_finite() || piv_abs <= 1e-30 {
            return Err(CodimError::SingularLinearSystem { pivot_idx: k });
        }
        if piv_row != k {
            for c in 0..n {
                lu.swap(k * n + c, piv_row * n + c);
            }
            rhs.swap(k, piv_row);
        }
        let diag = lu[k * n + k];
        for r in k + 1..n {
            let factor = lu[r * n + k] / diag;
            lu[r * n + k] = factor;
            for c in k + 1..n {
                lu[r * n + c] -= factor * lu[k * n + c];
            }
            rhs[r] -= factor * rhs[k];
        }
    }

    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut acc = rhs[r];
        for c in r + 1..n {
            acc -= lu[r * n + c] * x[c];
        }
        x[r] = acc / lu[r * n + r];
    }
    Ok(x)
}
