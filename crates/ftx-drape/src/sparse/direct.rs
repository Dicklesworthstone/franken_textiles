//! Provenance: lifted from frankensim `crates/fs-sparse/src/direct.rs` at commit
//! `93fc9b26f618bea6ebac6534f1d33c715d8a1c92` (same author, MIT + OpenAI/Anthropic rider). Changes:
//! import paths, and [`SymbolicLdlt::analyze_with_permutation`] so the cloth solver can order the
//! vertex graph and expand it to three DOFs per vertex (5–9× faster analysis, same fill); the panel
//! block's Schur updates deferred until the fully-summed columns are done; slice kernels the compiler
//! vectorizes and an upper triangle mirrored only when the full pivot search needs it (factorization
//! 2.9×, solve 1.3× faster on a 10,800-DOF cloth system; every result bit-identical, positive definite
//! or shifted indefinite with 2×2 and delayed pivots); and a backward solve that sums each column's
//! panel rows first, by four partial sums ([`dot4`]), instead of one serial chain from the diagonal
//! (solve 2.1× faster again; different rounding, the same on every target); and, with the crate's
//! `parallel` feature, independent assembly subtrees factored as tasks and a large front's panel
//! columns updated at once (each front's arithmetic unchanged, so bit-identical to the serial pass,
//! refusals and statistics included). The original module documentation follows.
//!
//! Sparse symmetric direct factorization: supernodal multifrontal LDLᵀ with
//! AMD fill-reducing ordering, restricted (within-supernode) 1×1/2×2
//! threshold pivoting, and Sylvester inertia (bead
//! frankensim-fsim-sparse-direct-4a38j, musical-acoustics program).
//!
//! WHY THIS EXISTS — shift-invert modal analysis factors (K − σM), which is
//! symmetric INDEFINITE for interior shifts, and the inertia of D is exactly
//! the eigenvalue count below σ that spectrum-slicing certification needs
//! (Sylvester's law). Cholesky alone cannot serve that consumer; iterative
//! solvers cannot produce inertia at all.
//!
//! DESIGN — three explicit stages with separate cost models:
//! 1. [`amd_order`]: approximate-minimum-degree ordering (Amestoy–Davis–Duff
//!    quotient-graph algorithm; v1 without supervariable merging — an
//!    ordering-QUALITY boundary, never a correctness one).
//! 2. [`SymbolicLdlt::analyze`]: elimination tree, postorder, per-column L
//!    structure, and fundamental supernodes (exact structure-match test, so
//!    within-supernode column exchanges provably preserve the symbolic
//!    factorization). Reusable across every matrix with the SAME pattern —
//!    each shift σ in (K − σM) refactors numerically but never re-analyzes.
//! 3. [`SymbolicLdlt::factor`]: multifrontal numeric factorization. Each
//!    supernode assembles a dense frontal matrix (original entries plus
//!    children's Schur updates), factors its fully-summed block with
//!    threshold pivoting restricted to that block (Duff–Reid growth test,
//!    1×1 and symmetric 2×2 pivots), and passes the Schur complement to its
//!    assembly-tree parent.
//!
//! DELAYED PIVOTS make restricted pivoting complete: a fully-summed column
//! with no acceptable pivot migrates inside the update matrix to its parent
//! front, where more of the matrix has accumulated and 2×2 partners exist
//! (the MA57/MUMPS mechanism). Width-1 supernodes with exactly zero
//! diagonals — e.g. (K − σM) at a shift that zeroes the diagonal — factor
//! correctly this way. Only a column that remains unpivotable at a ROOT
//! front refuses with a named error instead of perturbing: a perturbed
//! factorization would silently corrupt the inertia count that downstream
//! certification treats as authority (refusal-not-clamp, workspace law).
//!
//! Determinism: every stage is sequential with index-ordered tie-breaking —
//! repeat factorization of the same matrix is bitwise identical (tested). The
//! `parallel` feature only changes which thread computes a front, never its
//! operations or their order.

use super::Csr;

/// Fill-reducing ordering strategy for [`SymbolicLdlt::analyze`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectOrdering {
    /// Identity permutation — the fill-comparison baseline and the debug aid.
    Natural,
    /// Approximate minimum degree ([`amd_order`]). The default.
    Amd,
}

/// Options for the numeric factorization stage.
#[derive(Debug, Clone, Copy)]
pub struct LdltOptions {
    /// Relative pivot threshold `u` in (0, 0.5] (Duff–Reid). A 1×1 pivot
    /// `a_kk` is accepted only when `|a_kk| ≥ u·γ_k` (γ_k = largest
    /// off-diagonal magnitude in the candidate column within the front); a
    /// 2×2 pivot must bound the multiplier growth by `1/u`. Larger `u` is
    /// more stable, smaller `u` accepts more pivots. Default `0.01`.
    pub pivot_threshold: f64,
    /// Permit symmetric 2×2 pivots (required for saddle-point/zero-diagonal
    /// indefinite matrices). Default `true`; disabling exists so tests can
    /// prove the 2×2 path is load-bearing.
    pub allow_2x2: bool,
    /// Relative zero-pivot floor: a pivot (or 2×2 block determinant, squared
    /// scale) below `zero_pivot_rel · max|A|` (resp. its square) refuses as
    /// numerically singular. Default `256·ε ≈ 5.7e-14`.
    pub zero_pivot_rel: f64,
}

impl Default for LdltOptions {
    fn default() -> LdltOptions {
        LdltOptions {
            pivot_threshold: 0.01,
            allow_2x2: true,
            zero_pivot_rel: 256.0 * f64::EPSILON,
        }
    }
}

/// Sylvester inertia of the factored matrix: eigenvalue sign counts of D
/// (equal to those of A by Sylvester's law of inertia). `zero` is always 0
/// here — a numerically singular pivot refuses instead of recording a zero
/// eigenvalue claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inertia {
    /// Number of positive eigenvalues.
    pub positive: usize,
    /// Number of negative eigenvalues.
    pub negative: usize,
}

/// Work/memory/pivot accounting for one numeric factorization.
#[derive(Debug, Clone, Copy)]
pub struct FactorStats {
    /// Matrix dimension.
    pub n: usize,
    /// Stored nonzeros of the input matrix (both triangles as given).
    pub nnz_a: usize,
    /// Nonzeros of L including the unit diagonal.
    pub nnz_l: usize,
    /// `nnz_l / nnz_a` (1.0 when `nnz_a` is 0).
    pub fill_ratio: f64,
    /// Approximate floating-point operation count of the numeric phase.
    pub flops: u64,
    /// Peak bytes of live frontal + update-matrix storage.
    pub peak_front_bytes: usize,
    /// Number of supernodes.
    pub supernodes: usize,
    /// Count of 1×1 pivots taken.
    pub pivots_1x1: usize,
    /// Count of 2×2 pivots taken.
    pub pivots_2x2: usize,
    /// Column-delay events: a fully-summed column with no acceptable pivot
    /// migrated to its parent front and was eliminated there (a column
    /// delayed through several ancestors counts once per hop).
    pub pivots_delayed: usize,
    /// Largest frontal matrix dimension encountered (delays widen ancestor
    /// fronts, so this can exceed the symbolic prediction).
    pub max_front_dim: usize,
}

/// Typed refusals of the direct-factorization stages. Display strings carry
/// stable `FS-SPARSE-DIRECT-*` codes for log matching.
#[derive(Debug, Clone, PartialEq)]
pub enum LdltError {
    /// The matrix is not square.
    NotSquare {
        /// Rows of the offending matrix.
        nrows: usize,
        /// Columns of the offending matrix.
        ncols: usize,
    },
    /// The sparsity pattern is not structurally symmetric; `(row, col)` is a
    /// witness entry whose mirror is absent.
    StructurallyAsymmetric {
        /// Witness row.
        row: usize,
        /// Witness column.
        col: usize,
    },
    /// A stored value is NaN or infinite. Non-finite input would contaminate
    /// every downstream claim, so it fails closed here.
    NonFiniteEntry {
        /// Row of the non-finite entry.
        row: usize,
        /// Column of the non-finite entry.
        col: usize,
    },
    /// `factor` was called with a matrix whose pattern differs from the one
    /// this symbolic analysis was computed for.
    PatternMismatch,
    /// Invalid [`LdltOptions`] field (threshold outside (0, 0.5], or a
    /// non-positive zero-pivot floor).
    InvalidOptions {
        /// Name of the offending option field.
        field: &'static str,
    },
    /// No acceptable 1×1 or 2×2 pivot exists in a ROOT front's fully-summed
    /// block after delayed-pivot migration: the matrix is numerically
    /// singular (or beyond the threshold's reach). The factorization
    /// REFUSES rather than perturbs.
    PivotBreakdown {
        /// Column in the ORIGINAL index space where elimination stopped.
        orig_col: usize,
        /// The same column in elimination (permuted) order.
        permuted_col: usize,
        /// Best candidate pivot magnitude seen in the block.
        best_abs: f64,
        /// The magnitude the threshold test demanded.
        required: f64,
        /// Whether 2×2 pivots were permitted during the search.
        two_by_two_enabled: bool,
    },
}

impl core::fmt::Display for LdltError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LdltError::NotSquare { nrows, ncols } => {
                write!(f, "FS-SPARSE-DIRECT-NOT-SQUARE: {nrows}x{ncols} matrix")
            }
            LdltError::StructurallyAsymmetric { row, col } => write!(
                f,
                "FS-SPARSE-DIRECT-ASYMMETRIC-PATTERN: entry ({row},{col}) has no stored mirror"
            ),
            LdltError::NonFiniteEntry { row, col } => write!(
                f,
                "FS-SPARSE-DIRECT-NON-FINITE: entry ({row},{col}) is NaN or infinite"
            ),
            LdltError::PatternMismatch => write!(
                f,
                "FS-SPARSE-DIRECT-PATTERN-MISMATCH: matrix pattern differs from the analyzed one"
            ),
            LdltError::InvalidOptions { field } => {
                write!(f, "FS-SPARSE-DIRECT-INVALID-OPTIONS: {field}")
            }
            LdltError::PivotBreakdown {
                orig_col,
                permuted_col,
                best_abs,
                required,
                two_by_two_enabled,
            } => write!(
                f,
                "FS-SPARSE-DIRECT-PIVOT-BREAKDOWN: no acceptable pivot at original column \
                 {orig_col} (elimination position {permuted_col}); best |candidate| {best_abs:.3e} \
                 < required {required:.3e}; 2x2 pivots {}",
                if *two_by_two_enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
        }
    }
}

impl std::error::Error for LdltError {}

// ---------------------------------------------------------------------------
// AMD ordering
// ---------------------------------------------------------------------------

/// Approximate minimum degree ordering (Amestoy–Davis–Duff) on the symmetric
/// pattern of `a` (the pattern is symmetrized internally; values are
/// ignored). Returns `perm` with `perm[k]` = the original index eliminated
/// k-th. Deterministic: minimum approximate degree with smallest-index
/// tie-breaking.
///
/// v1 boundary: no supervariable (indistinguishable-node) merging and no
/// dense-row special case — both affect ordering quality and speed on some
/// graphs, never correctness. Panics on a non-square matrix (programmer
/// error; [`SymbolicLdlt::analyze`] refuses with a typed error first).
#[must_use]
#[allow(clippy::needless_range_loop)] // index-parallel arrays throughout
#[allow(clippy::too_many_lines)] // one quotient-graph elimination loop; splitting hides invariants
pub fn amd_order(a: &Csr) -> Vec<usize> {
    assert_eq!(a.nrows(), a.ncols(), "amd_order: matrix must be square");
    let n = a.nrows();
    if n == 0 {
        return Vec::new();
    }
    // Symmetrized adjacency without the diagonal.
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

    // Quotient graph state. Element ids reuse the pivot variable's id.
    let mut elems_of: Vec<Vec<usize>> = vec![Vec::new(); n]; // elements adjacent to variable
    let mut elem_vars: Vec<Vec<usize>> = vec![Vec::new(); n]; // element -> boundary variables
    let mut elem_alive = vec![false; n];
    let mut var_alive = vec![true; n];
    let mut degree: Vec<usize> = adj.iter().map(Vec::len).collect();

    // Lazy min-heap of (degree, index).
    let mut heap: std::collections::BinaryHeap<core::cmp::Reverse<(usize, usize)>> =
        (0..n).map(|i| core::cmp::Reverse((degree[i], i))).collect();

    let mut perm = Vec::with_capacity(n);
    let mut marked = vec![usize::MAX; n]; // stamp array for Lp membership
    let mut w_stamp = vec![usize::MAX; n]; // stamp array for |Le \ Lp| counters
    let mut w_val = vec![0usize; n];

    for step in 0..n {
        // Pop until a live entry whose recorded degree is current.
        let p = loop {
            let core::cmp::Reverse((d, i)) = heap.pop().expect("amd: heap exhausted early");
            if var_alive[i] && degree[i] == d {
                break i;
            }
        };
        var_alive[p] = false;
        perm.push(p);

        // Lp = (adj[p] ∪ ⋃_{e ∈ E_p} Le) \ {p}, live variables only.
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
            elem_alive[e] = false; // absorbed into the new element p
        }
        for &v in &lp {
            marked[v] = step;
        }

        // |Le \ Lp| for every live element touching Lp, via the classic
        // decrement pass (i ∈ E_e ∩ Lp ⇔ e ∈ E_i for i ∈ Lp).
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
        // Absorb elements entirely contained in Lp.
        for &i in &lp {
            for &e in &elems_of[i] {
                if elem_alive[e] && w_stamp[e] == step && w_val[e] == 0 {
                    elem_alive[e] = false;
                }
            }
        }

        let remaining = n - step - 1;
        for &i in &lp {
            // Prune variable adjacency: members of Lp now reach i through
            // element p; dead variables drop.
            adj[i].retain(|&v| var_alive[v] && marked[v] != step);
            elems_of[i].retain(|&e| elem_alive[e]);
            elems_of[i].push(p);
            // Approximate external degree (ADD bound).
            let mut d = adj[i].len() + (lp.len() - 1);
            for &e in &elems_of[i] {
                if e != p && w_stamp[e] == step {
                    d += w_val[e];
                }
            }
            let d = d.min(remaining);
            degree[i] = d;
            heap.push(core::cmp::Reverse((d, i)));
        }
        elem_vars[p] = lp;
        elem_alive[p] = true;
        elems_of[p].clear();
    }
    perm
}

fn a_contains(a: &Csr, r: usize, c: usize) -> bool {
    let (cols, _) = a.row(r);
    cols.binary_search(&c).is_ok()
}

// ---------------------------------------------------------------------------
// Symbolic analysis
// ---------------------------------------------------------------------------

const NONE: usize = usize::MAX;

#[derive(Debug, Clone)]
struct SnodeSym {
    /// First column (elimination order) of this supernode.
    col_start: usize,
    /// Number of columns.
    width: usize,
    /// Below-block row structure (elimination-order ids, ascending) — shared
    /// by every column of the supernode (exact-match certified).
    panel: Vec<usize>,
    /// Assembly-tree parent supernode, or `NONE`.
    parent: usize,
    /// Children supernodes (ascending).
    children: Vec<usize>,
}

/// Reusable symbolic factorization of a symmetric sparsity pattern:
/// permutation, elimination tree, supernode partition, and L structure.
/// One `analyze` serves every numeric [`SymbolicLdlt::factor`] with the
/// same pattern — the shift ladder (K − σᵢM) analyzes once and factors per
/// shift.
#[derive(Debug, Clone)]
pub struct SymbolicLdlt {
    n: usize,
    /// `perm[k]` = original index eliminated k-th (AMD ∘ etree postorder).
    perm: Vec<usize>,
    /// Inverse of `perm`.
    iperm: Vec<usize>,
    snodes: Vec<SnodeSym>,
    /// nnz(L) including the unit diagonal.
    nnz_l: usize,
    /// Exact pattern the analysis was computed for (mismatch guard).
    pat_row_ptr: Vec<usize>,
    pat_col_idx: Vec<usize>,
}

impl SymbolicLdlt {
    /// Analyze the symmetric pattern of `a`: validate squareness and
    /// structural symmetry, order (AMD or natural), build the elimination
    /// tree and its postorder, compute per-column L structures, and certify
    /// fundamental supernodes by EXACT structure comparison.
    pub fn analyze(a: &Csr, ordering: DirectOrdering) -> Result<SymbolicLdlt, LdltError> {
        if a.nrows() != a.ncols() {
            return Err(LdltError::NotSquare {
                nrows: a.nrows(),
                ncols: a.ncols(),
            });
        }
        let base_perm = match ordering {
            DirectOrdering::Natural => (0..a.nrows()).collect::<Vec<usize>>(),
            DirectOrdering::Amd => amd_order(a),
        };
        Self::analyze_with_permutation(a, base_perm)
    }

    /// [`SymbolicLdlt::analyze`] with a caller-supplied fill-reducing permutation (`perm[k]` = the
    /// original index eliminated k-th). The elimination-tree postorder is still applied on top.
    ///
    /// # Errors
    /// As [`SymbolicLdlt::analyze`].
    ///
    /// # Panics
    /// If `perm` is not a permutation of `0..n`.
    pub fn analyze_with_permutation(
        a: &Csr,
        base_perm: Vec<usize>,
    ) -> Result<SymbolicLdlt, LdltError> {
        if a.nrows() != a.ncols() {
            return Err(LdltError::NotSquare {
                nrows: a.nrows(),
                ncols: a.ncols(),
            });
        }
        let n = a.nrows();
        // Structural symmetry: every (r,c) needs its mirror.
        for r in 0..n {
            let (cols, _) = a.row(r);
            for &c in cols {
                if !a_contains(a, c, r) {
                    return Err(LdltError::StructurallyAsymmetric { row: r, col: c });
                }
            }
        }
        assert_eq!(
            base_perm.len(),
            n,
            "permutation length must equal the matrix dimension"
        );
        let mut base_iperm = vec![usize::MAX; n];
        for (k, &o) in base_perm.iter().enumerate() {
            assert!(
                o < n && base_iperm[o] == usize::MAX,
                "not a permutation: index {o}"
            );
            base_iperm[o] = k;
        }
        // Lower-triangular adjacency in base elimination order.
        let ladj = build_lower_adj(a, &base_iperm);
        let parent0 = etree(&ladj);
        let post = postorder(&parent0);
        // Compose: final elimination order.
        let perm: Vec<usize> = post.iter().map(|&k| base_perm[k]).collect();
        let mut iperm = vec![0usize; n];
        for (k, &o) in perm.iter().enumerate() {
            iperm[o] = k;
        }
        let ladj = build_lower_adj(a, &iperm);
        let parent = etree(&ladj); // relabeled tree, recomputed for simplicity
        let colstruct = col_structs(&ladj, &parent);

        // Fundamental supernodes, certified by exact structure match:
        // col j may join col j+1's supernode iff struct(j) = {j+1} ∪ struct(j+1).
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
        Ok(SymbolicLdlt {
            n,
            perm,
            iperm,
            snodes,
            nnz_l,
            pat_row_ptr,
            pat_col_idx,
        })
    }

    /// Dimension of the analyzed pattern.
    #[must_use]
    pub fn n(&self) -> usize {
        self.n
    }

    /// nnz(L) including the unit diagonal (a pure function of the pattern
    /// and ordering — available before any numeric work, so callers can
    /// budget memory).
    #[must_use]
    pub fn nnz_l(&self) -> usize {
        self.nnz_l
    }

    /// Number of supernodes.
    #[must_use]
    pub fn supernode_count(&self) -> usize {
        self.snodes.len()
    }

    /// The elimination permutation (`perm[k]` = original index eliminated
    /// k-th).
    #[must_use]
    pub fn permutation(&self) -> &[usize] {
        &self.perm
    }

    /// Numeric multifrontal factorization of `a` (which must carry exactly
    /// the analyzed pattern) as P·A·Pᵀ = L·D·Lᵀ with unit-lower L and D of
    /// 1×1/2×2 blocks. Refuses (never perturbs) on pivot breakdown.
    #[allow(clippy::too_many_lines)] // one coherent numeric pipeline; splitting obscures the data flow
    pub fn factor(&self, a: &Csr, opts: &LdltOptions) -> Result<LdltFactor, LdltError> {
        // NaN thresholds fail the finiteness test, so both gates fail closed.
        if !opts.pivot_threshold.is_finite()
            || opts.pivot_threshold <= 0.0
            || opts.pivot_threshold > 0.5
        {
            return Err(LdltError::InvalidOptions {
                field: "pivot_threshold must be in (0, 0.5]",
            });
        }
        if !opts.zero_pivot_rel.is_finite() || opts.zero_pivot_rel <= 0.0 {
            return Err(LdltError::InvalidOptions {
                field: "zero_pivot_rel must be positive",
            });
        }
        let (rp, ci) = pattern_of(a);
        if rp != self.pat_row_ptr || ci != self.pat_col_idx {
            return Err(LdltError::PatternMismatch);
        }
        let n = self.n;
        let mut max_abs = 0.0f64;
        for r in 0..n {
            let (cols, vals) = a.row(r);
            for (&c, &v) in cols.iter().zip(vals) {
                if !v.is_finite() {
                    return Err(LdltError::NonFiniteEntry { row: r, col: c });
                }
                max_abs = max_abs.max(v.abs());
            }
        }
        let zero_tol = opts.zero_pivot_rel * max_abs.max(f64::MIN_POSITIVE);
        let fronts = self.factor_fronts(a, opts, zero_tol)?;

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
        // The live frontal and update storage of one pass in supernode order, whatever order the
        // fronts were factored in: each front is allocated while its children's updates are alive.
        let mut update_bytes = vec![0usize; self.snodes.len()];
        let mut live_update_bytes = 0usize;
        for (sid, (num, work)) in fronts.into_iter().enumerate() {
            stats.max_front_dim = stats.max_front_dim.max(work.m);
            stats.peak_front_bytes = stats
                .peak_front_bytes
                .max(work.m * work.m * 8 + live_update_bytes);
            for &ch in &self.snodes[sid].children {
                live_update_bytes -= update_bytes[ch];
            }
            inertia.positive += work.positive;
            inertia.negative += work.negative;
            stats.pivots_1x1 += work.pivots_1x1;
            stats.pivots_2x2 += work.pivots_2x2;
            stats.pivots_delayed += work.delayed;
            stats.flops += work.flops;
            nnz_l_actual += work.nnz_l;
            if work.update_len > 0 {
                update_bytes[sid] = work.update_len * 8;
                live_update_bytes += update_bytes[sid];
                stats.peak_front_bytes = stats.peak_front_bytes.max(live_update_bytes);
            }
            snodes_num.push(num);
        }

        stats.nnz_l = nnz_l_actual;
        stats.fill_ratio = if a.nnz() == 0 {
            1.0
        } else {
            stats.nnz_l as f64 / a.nnz() as f64
        };
        #[cfg(feature = "parallel")]
        let tasks = {
            // A solve touches each entry of L about twice, and runs far more often than the
            // factorization, so its subtrees are coarser: about one per thread, by entries of L. A
            // subtree with fewer than SOLVE_TASK_ENTRIES finishes sooner than a thread wakes for it
            // (WebAssembly workers especially), so it stays in the serial part of the solves, which is
            // always correct: only ancestors and earlier supernodes are read, whichever thread runs a
            // subtree.
            let threads = rayon::current_num_threads();
            let ranges = if threads < 2 {
                Vec::new()
            } else {
                self.subtree_ranges(|sid| snodes_num[sid].l.len() as f64, threads)
            };
            let tasks: Vec<SolveTask> = ranges
                .into_iter()
                .filter(|&(_, _, entries)| entries >= SOLVE_TASK_ENTRIES as f64)
                .map(|(first, last, _)| SolveTask {
                    sids: (first, last),
                    vars: (
                        self.snodes[first].col_start,
                        self.snodes[last].col_start + self.snodes[last].width,
                    ),
                })
                .collect();
            if tasks.len() < 2 { Vec::new() } else { tasks }
        };
        #[cfg(not(feature = "parallel"))]
        let tasks = Vec::new();
        Ok(LdltFactor {
            n,
            perm: self.perm.clone(),
            snodes: snodes_num,
            inertia,
            stats,
            tasks,
        })
    }

    /// Every supernode's numeric front, in supernode order. Each front depends only on the matrix
    /// and its children's updates, added in child order, so it is the same whichever thread factors
    /// it and whenever: with the `parallel` feature, independent subtrees run as tasks (see
    /// [`SymbolicLdlt::subtree_tasks`]) and the supernodes above them follow in order. A refusal is
    /// the one a single pass in supernode order meets first.
    fn factor_fronts(
        &self,
        a: &Csr,
        opts: &LdltOptions,
        zero_tol: f64,
    ) -> Result<Vec<(SnodeNum, FrontWork)>, LdltError> {
        let ns = self.snodes.len();
        let mut updates: Vec<Option<Update>> = vec![None; ns];
        let mut out: Vec<Option<(SnodeNum, FrontWork)>> = (0..ns).map(|_| None).collect();
        #[cfg(feature = "parallel")]
        let (failed, done) = self.factor_subtrees(a, opts, zero_tol, &mut updates, &mut out);
        #[cfg(not(feature = "parallel"))]
        let (failed, done): (Option<(usize, LdltError)>, Vec<bool>) = (None, Vec::new());
        // Scratch var→local-position map, stamped and reset per supernode so
        // the factorization stays O(front work), not O(n · supernodes).
        let mut pos = vec![usize::MAX; self.n];
        for sid in (0..ns).filter(|&sid| !done.get(sid).copied().unwrap_or(false)) {
            // A task failed below this supernode: a serial pass would have stopped there.
            if failed.as_ref().is_some_and(|(first, _)| *first < sid) {
                break;
            }
            let children = self.snodes[sid]
                .children
                .iter()
                .map(|&ch| updates[ch].take().expect("child update consumed twice"))
                .collect();
            let (num, update, work) =
                self.factor_front(sid, a, opts, zero_tol, &mut pos, children)?;
            updates[sid] = update;
            out[sid] = Some((num, work));
        }
        if let Some((_, error)) = failed {
            return Err(error);
        }
        Ok(out
            .into_iter()
            .map(|front| front.expect("every supernode is factored"))
            .collect())
    }

    /// Factors the [`SymbolicLdlt::subtree_tasks`] on rayon's pool, storing their fronts in `out` and
    /// the updates they pass up in `updates`. Returns the first (lowest-numbered) refusing supernode
    /// with its refusal, and which supernodes were factored.
    #[cfg(feature = "parallel")]
    #[allow(clippy::type_complexity)]
    fn factor_subtrees(
        &self,
        a: &Csr,
        opts: &LdltOptions,
        zero_tol: f64,
        updates: &mut [Option<Update>],
        out: &mut [Option<(SnodeNum, FrontWork)>],
    ) -> (Option<(usize, LdltError)>, Vec<bool>) {
        let tasks = self.subtree_tasks();
        let mut done = vec![false; self.snodes.len()];
        let mut failed: Option<(usize, LdltError)> = None;
        if tasks.is_empty() {
            return (failed, done);
        }
        let results = crate::par::map_tasks(tasks.len(), |t| {
            let (start, end) = tasks[t];
            let mut pos = vec![usize::MAX; self.n];
            let mut local: Vec<Option<Update>> = vec![None; end + 1 - start];
            let mut fronts = Vec::with_capacity(end + 1 - start);
            for sid in start..=end {
                let children = self.snodes[sid]
                    .children
                    .iter()
                    .map(|&ch| {
                        local[ch - start]
                            .take()
                            .expect("child update consumed twice")
                    })
                    .collect();
                let (num, update, work) = self
                    .factor_front(sid, a, opts, zero_tol, &mut pos, children)
                    .map_err(|e| (sid, e))?;
                local[sid - start] = update;
                fronts.push((num, work));
            }
            // What is left is what the roots pass up to supernodes outside the task.
            let passed: Vec<(usize, Update)> = local
                .into_iter()
                .enumerate()
                .filter_map(|(k, update)| update.map(|u| (start + k, u)))
                .collect();
            Ok((fronts, passed))
        });
        for (&(start, end), result) in tasks.iter().zip(results) {
            done[start..=end].fill(true);
            match result {
                Ok((fronts, passed)) => {
                    for (k, front) in fronts.into_iter().enumerate() {
                        out[start + k] = Some(front);
                    }
                    for (sid, update) in passed {
                        updates[sid] = Some(update);
                    }
                }
                Err((sid, error)) => {
                    if failed.as_ref().is_none_or(|(first, _)| sid < *first) {
                        failed = Some((sid, error));
                    }
                }
            }
        }
        (failed, done)
    }

    /// Consecutive supernode ranges `(first, last)` that are unions of whole subtrees of the assembly
    /// tree, each about a quarter of the factorization's work per thread or less, in order; every
    /// supernode outside them is an ancestor of one. Empty on a single thread. Work is each front's
    /// elimination count `Σ (m − k)²` from the symbolic structure (delays not foreseen).
    #[cfg(feature = "parallel")]
    fn subtree_tasks(&self) -> Vec<(usize, usize)> {
        let threads = rayon::current_num_threads();
        if threads < 2 {
            return Vec::new();
        }
        self.subtree_ranges(
            |sid| {
                let s = &self.snodes[sid];
                let (w, m) = (s.width as f64, (s.width + s.panel.len()) as f64);
                w * m * m - m * w * (w - 1.0) + (w - 1.0) * w * (2.0 * w - 1.0) / 6.0
            },
            4 * threads,
        )
        .into_iter()
        .map(|(start, end, _)| (start, end))
        .collect()
    }

    /// Consecutive supernode ranges `(first, last, work)` that are unions of whole subtrees of the
    /// assembly tree, each with at most `1/parts` of the total of `own(sid)` (a supernode's own work)
    /// where the tree allows, in order; every supernode outside them is an ancestor of one. Empty when
    /// fewer than two ranges result.
    #[cfg(feature = "parallel")]
    fn subtree_ranges(&self, own: impl Fn(usize) -> f64, parts: usize) -> Vec<(usize, usize, f64)> {
        let ns = self.snodes.len();
        if parts < 2 || ns < 2 {
            return Vec::new();
        }
        let mut first: Vec<usize> = (0..ns).collect();
        let mut subtree = vec![0.0f64; ns];
        for (sid, s) in self.snodes.iter().enumerate() {
            subtree[sid] += own(sid);
            if s.parent != NONE {
                subtree[s.parent] += subtree[sid];
                first[s.parent] = first[s.parent].min(first[sid]);
            }
        }
        let roots: Vec<usize> = (0..ns).filter(|&s| self.snodes[s].parent == NONE).collect();
        let total: f64 = roots.iter().map(|&r| subtree[r]).sum();
        let target = total / parts as f64;
        let mut whole = Vec::new();
        let mut stack = roots;
        while let Some(s) = stack.pop() {
            if subtree[s] <= target {
                whole.push(s);
            } else {
                stack.extend(&self.snodes[s].children);
            }
        }
        whole.sort_unstable();
        // Adjacent subtrees with nothing between them merge while the task stays within the target.
        let mut tasks: Vec<(usize, usize, f64)> = Vec::new();
        for root in whole {
            match tasks.last_mut() {
                Some((_, end, work))
                    if *end + 1 == first[root] && *work + subtree[root] <= target =>
                {
                    *end = root;
                    *work += subtree[root];
                }
                _ => tasks.push((first[root], root, subtree[root])),
            }
        }
        if tasks.len() < 2 {
            return Vec::new();
        }
        tasks
    }

    /// One supernode's front: assembly of its original entries and its children's updates (in
    /// `children`, child order), restricted-pivot elimination, the deferred panel update, L, and the
    /// update it passes to its parent.
    #[allow(clippy::too_many_lines)] // one coherent numeric pipeline; splitting obscures the data flow
    fn factor_front(
        &self,
        sid: usize,
        a: &Csr,
        opts: &LdltOptions,
        zero_tol: f64,
        pos: &mut [usize],
        children: Vec<Update>,
    ) -> Result<(SnodeNum, Option<Update>, FrontWork), LdltError> {
        let sym = &self.snodes[sid];
        let mut work = FrontWork::default();
        {
            let w = sym.width;
            // DELAYED PIVOTS: fully-summed columns of this front are the
            // supernode's own block columns PLUS any columns a descendant
            // failed to pivot and passed upward inside its update matrix
            // (recognizable as update vars below `col_start` — every
            // non-delayed update row is a struct row, which is ≥ col_start).
            // A delayed column is fully summed here because its remaining
            // structure was contained in the child front, which the assembly
            // tree embeds in this one.
            let mut delayed: Vec<usize> = Vec::new();
            for up in &children {
                delayed.extend(up.vars.iter().copied().filter(|&v| v < sym.col_start));
            }
            delayed.sort_unstable();
            let mut fs: Vec<usize> = delayed;
            fs.extend(sym.col_start..sym.col_start + w);
            let wf = fs.len();
            let m = wf + sym.panel.len();
            work.m = m;
            // Local layout: fully-summed columns first, then panel rows.
            let mut front_local: Vec<usize> = fs;
            front_local.extend(sym.panel.iter().copied());
            for (idx, &v) in front_local.iter().enumerate() {
                pos[v] = idx;
            }
            let mut f = vec![0.0f64; m * m];

            // Scatter original entries of the OWN block columns (values read
            // from the row of the eliminated column; the input contract is a
            // numerically symmetric matrix). Delayed columns are NOT
            // rescattered: their original entries already entered the
            // descendant front and travel here inside its update matrix.
            for j in sym.col_start..sym.col_start + w {
                let c = pos[j];
                let orig = self.perm[j];
                let (cols, vals) = a.row(orig);
                for (&oc, &v) in cols.iter().zip(vals) {
                    let pi = self.iperm[oc];
                    if pi >= j {
                        let li = pos[pi];
                        assert!(
                            li != usize::MAX,
                            "symbolic invariant: neighbor outside front"
                        );
                        f[li + c * m] += v;
                        if li != c {
                            f[c + li * m] += v;
                        }
                    }
                }
            }
            // Extend-add the children's Schur updates (delayed rows/columns
            // included — their labels are in this front by construction).
            for up in children {
                let map: Vec<usize> = up
                    .vars
                    .iter()
                    .map(|&v| {
                        let li = pos[v];
                        assert!(
                            li != usize::MAX,
                            "assembly invariant: child var outside parent front"
                        );
                        li
                    })
                    .collect();
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

            // Factor the fully-summed block with restricted pivoting.
            let mut local2var = front_local;
            let mut dblocks: Vec<DBlock> = Vec::new();
            let mut deferred: Vec<Deferred> = Vec::new();
            let mut k = 0usize;
            while k < wf {
                // The eliminations keep only the lower triangle current. The usual pivot, the current
                // column, reads only that; the full search and its swaps also read the upper one, so it
                // is copied over first: the values the eager mirror held, bit for bit.
                let choice = match diagonal_pivot(&f, m, k, opts, zero_tol) {
                    Some(choice) => choice,
                    None => {
                        mirror_block(&mut f, m, k, wf);
                        choose_pivot(&f, m, wf, k, opts, zero_tol)
                    }
                };
                match choice {
                    PivotChoice::One(q) => {
                        if q != k {
                            swap_sym(&mut f, m, k, q);
                            local2var.swap(k, q);
                        }
                        let d = f[k + k * m];
                        if d > 0.0 {
                            work.positive += 1;
                        } else {
                            work.negative += 1;
                        }
                        work.pivots_1x1 += 1;
                        dblocks.push(DBlock::Single(d));
                        for i in k + 1..m {
                            f[i + k * m] /= d;
                        }
                        // Fully-summed columns now (their lower triangle: see `mirror_block`); the
                        // panel block after the loop.
                        for jc in k + 1..wf {
                            let (left, right) = f.split_at_mut(jc * m);
                            let src = &left[k * m + jc..(k + 1) * m];
                            let cj = d * src[0];
                            for (x, &s) in right[jc..m].iter_mut().zip(src) {
                                *x -= s * cj;
                            }
                        }
                        deferred.push(Deferred::One { col: k, d });
                        work.flops += ((m - k) as u64) * ((m - k) as u64);
                        k += 1;
                    }
                    PivotChoice::Two(pq, qq) => {
                        if pq != k {
                            swap_sym(&mut f, m, k, pq);
                            local2var.swap(k, pq);
                        }
                        // After the first swap, the partner may have moved.
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
                            work.positive += 1;
                            work.negative += 1;
                        } else if e11 + e22 > 0.0 {
                            work.positive += 2;
                        } else {
                            work.negative += 2;
                        }
                        work.pivots_2x2 += 1;
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
                        for (jdx, jc) in (k + 2..wf).enumerate() {
                            let aj = acol[jdx];
                            let bj = bcol[jdx];
                            let (left, right) = f.split_at_mut(jc * m);
                            let sa = &left[k * m + jc..(k + 1) * m];
                            let sb = &left[(k + 1) * m + jc..(k + 2) * m];
                            for ((x, &u), &w) in right[jc..m].iter_mut().zip(sa).zip(sb) {
                                *x -= u * aj + w * bj;
                            }
                        }
                        // the panel rows' pre-scaling column values, for the deferred update
                        deferred.push(Deferred::Two {
                            col: k,
                            a: acol[wf - (k + 2)..].to_vec(),
                            b: bcol[wf - (k + 2)..].to_vec(),
                        });
                        work.flops += 2 * ((m - k) as u64) * ((m - k) as u64);
                        // The intra-pair coupling lives in D, not L: the
                        // 2×2 block is eliminated jointly, so L's strict
                        // lower entry inside the pair is exactly zero.
                        f[(k + 1) + k * m] = 0.0;
                        f[k + (k + 1) * m] = 0.0;
                        k += 2;
                    }
                    PivotChoice::Fail { best_abs, required } => {
                        if sym.parent == NONE {
                            // No parent front exists to absorb the columns:
                            // the matrix is numerically singular (or beyond
                            // the threshold's reach). Refuse, never perturb.
                            return Err(LdltError::PivotBreakdown {
                                orig_col: self.perm[local2var[k]],
                                permuted_col: local2var[k],
                                best_abs,
                                required,
                                two_by_two_enabled: opts.allow_2x2,
                            });
                        }
                        // Delay every remaining fully-summed column to the
                        // parent front (no further mass can arrive in THIS
                        // front, so re-scanning here cannot succeed).
                        break;
                    }
                }
            }
            let k_elim = k;
            work.delayed = wf - k_elim;

            // The panel block's Schur updates, deferred from the pivots above. Nothing read the
            // block during the elimination (the pivot search and the swaps stay in the fully-summed
            // columns) and each entry still receives its updates in pivot order with the same
            // operands, so the result is bit-identical to updating it pivot by pivot with its mirror,
            // without the strided upper-triangle writes and one column at a time. Only its lower
            // triangle is kept; the update below reads that.
            // (Pivot columns lie left of every panel column, so splitting the front after the pivot
            // columns gives disjoint slices the compiler vectorizes; the arithmetic is unchanged.)
            // Each panel column reads only the pivot columns, so the columns are independent: with
            // the `parallel` feature a large front updates runs of them at once, the same arithmetic
            // per entry.
            let (left, right) = f.split_at_mut(wf * m);
            let left = &*left;
            let update_column = |jc: usize, dst: &mut [f64]| {
                for p in &deferred {
                    match p {
                        Deferred::One { col, d } => {
                            let k = *col;
                            let src = &left[k * m + jc..(k + 1) * m];
                            let cj = d * src[0];
                            for (x, &s) in dst.iter_mut().zip(src) {
                                *x -= s * cj;
                            }
                        }
                        Deferred::Two { col, a, b } => {
                            let k = *col;
                            let (aj, bj) = (a[jc - wf], b[jc - wf]);
                            let sa = &left[k * m + jc..(k + 1) * m];
                            let sb = &left[(k + 1) * m + jc..(k + 2) * m];
                            for ((x, &u), &w) in dst.iter_mut().zip(sa).zip(sb) {
                                *x -= u * aj + w * bj;
                            }
                        }
                    }
                }
            };
            let panel = m - wf;
            // About 2^16 multiply-adds per task; a small front stays one task (serial).
            let columns =
                if panel.saturating_mul(panel).saturating_mul(k_elim) >= PANEL_PARALLEL_WORK {
                    (PANEL_TASK_WORK / panel.saturating_mul(k_elim).max(1)).clamp(1, panel)
                } else {
                    panel.max(1)
                };
            crate::par::for_each_chunk_mut(right, m * columns, |c, run| {
                for (idx, column) in run.chunks_mut(m).enumerate() {
                    let jc = wf + c * columns + idx;
                    update_column(jc, &mut column[jc..m]);
                }
            });

            // Harvest L: unit-diagonal dense trapezoid, col-major m×k_elim.
            let mut l = vec![0.0f64; m * k_elim];
            for c in 0..k_elim {
                l[c + c * m] = 1.0;
                for i in c + 1..m {
                    let val = f[i + c * m];
                    if !val.is_finite() {
                        return Err(LdltError::PivotBreakdown {
                            orig_col: self.perm[local2var[c]],
                            permuted_col: local2var[c],
                            best_abs: f64::INFINITY,
                            required: zero_tol,
                            two_by_two_enabled: opts.allow_2x2,
                        });
                    }
                    l[i + c * m] = val;
                }
                work.nnz_l += m - c;
            }
            // Remaining rows: delayed columns (if any) then panel.
            let remaining = m - k_elim;
            // Schur complement (plus any delayed rows/columns) → parent
            // update. A root supernode provably has an empty panel, and its
            // delay path returned above, so remaining == 0 there.
            debug_assert!(
                sym.parent != NONE || remaining == 0,
                "root left residual rows"
            );
            let update = if sym.parent != NONE && remaining > 0 {
                let mut data = vec![0.0f64; remaining * remaining];
                for cj in 0..remaining {
                    for ri in 0..remaining {
                        // from the lower triangle: above it the panel columns are not kept
                        let (r, c) = (k_elim + ri.max(cj), k_elim + ri.min(cj));
                        data[ri + cj * remaining] = f[r + c * m];
                    }
                }
                work.update_len = data.len();
                Some(Update {
                    vars: local2var[k_elim..].to_vec(),
                    data,
                })
            } else {
                None
            };
            Ok((
                SnodeNum {
                    block_vars: local2var[..k_elim].to_vec(),
                    panel_vars: local2var[k_elim..].to_vec(),
                    l,
                    d: dblocks,
                },
                update,
                work,
            ))
        }
    }
}

/// Below this many panel multiply-adds (`panel² · pivots`) a front's panel update is one task.
const PANEL_PARALLEL_WORK: usize = 1 << 18;
/// Multiply-adds per task of a large front's panel update.
const PANEL_TASK_WORK: usize = 1 << 16;
/// Entries of L below which a subtree's share of a solve is not worth a task of its own.
#[cfg(feature = "parallel")]
const SOLVE_TASK_ENTRIES: usize = 1 << 16;

/// One front's share of [`FactorStats`] and [`Inertia`], and the size of the update it passes up.
#[derive(Debug, Clone, Copy, Default)]
struct FrontWork {
    m: usize,
    flops: u64,
    pivots_1x1: usize,
    pivots_2x2: usize,
    delayed: usize,
    positive: usize,
    negative: usize,
    nnz_l: usize,
    update_len: usize,
}

fn pattern_of(a: &Csr) -> (Vec<usize>, Vec<usize>) {
    let n = a.nrows();
    let mut row_ptr = Vec::with_capacity(n + 1);
    let mut col_idx = Vec::with_capacity(a.nnz());
    row_ptr.push(0);
    for r in 0..n {
        let (cols, _) = a.row(r);
        col_idx.extend_from_slice(cols);
        row_ptr.push(col_idx.len());
    }
    (row_ptr, col_idx)
}

/// Lower-triangular adjacency in elimination order: for permuted row `i`,
/// the sorted permuted neighbors `j < i`.
fn build_lower_adj(a: &Csr, iperm: &[usize]) -> Vec<Vec<usize>> {
    let n = a.nrows();
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

/// Elimination tree (Liu's algorithm with path compression).
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

/// Iterative etree postorder with ascending-child determinism.
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

/// Per-column below-diagonal L structure via the row-wise etree traversal
/// (each row i is added to the structure of every column on the path from
/// its lower neighbors up to i). Rows arrive in ascending order, so each
/// structure vector is sorted.
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

/// Largest off-diagonal magnitude of local column `c` among rows `k..m`.
/// `Σ aᵢbᵢ` by four interleaved partial sums combined as `(s₀ + s₁) + (s₂ + s₃)`, then the tail: one
/// fixed order on every target, without a single serial chain of multiply-adds.
fn dot4(a: &[f64], b: &[f64]) -> f64 {
    let mut s = [0.0f64; 4];
    let ((ca, ta), (cb, tb)) = (a.as_chunks::<4>(), b.as_chunks::<4>());
    for (x, y) in ca.iter().zip(cb) {
        for j in 0..4 {
            s[j] += x[j] * y[j];
        }
    }
    let mut t = (s[0] + s[1]) + (s[2] + s[3]);
    for (x, y) in ta.iter().zip(tb) {
        t += x * y;
    }
    t
}

/// [`choose_pivot`]'s first candidate, the current column `k`, which reads only the lower triangle.
fn diagonal_pivot(
    f: &[f64],
    m: usize,
    k: usize,
    opts: &LdltOptions,
    zero_tol: f64,
) -> Option<PivotChoice> {
    let dk = f[k + k * m].abs();
    let gk = gamma_of(f, m, k, k);
    (dk.is_finite() && gk.is_finite() && dk > zero_tol && dk >= opts.pivot_threshold * gk)
        .then_some(PivotChoice::One(k))
}

/// Copies the lower triangle of the trailing fully-summed block `k..w` onto its upper triangle, for the
/// full pivot search and the symmetric swaps (the eliminations update only the lower triangle).
fn mirror_block(f: &mut [f64], m: usize, k: usize, w: usize) {
    for c in k..w {
        for r in c + 1..w {
            f[c + r * m] = f[r + c * m];
        }
    }
}

fn gamma_of(f: &[f64], m: usize, k: usize, c: usize) -> f64 {
    let mut g = 0.0f64;
    for i in k..m {
        if i != c {
            let v = f[i + c * m].abs();
            if !v.is_finite() {
                return f64::NAN;
            }
            g = g.max(v);
        }
    }
    g
}

/// Deterministic restricted pivot search inside the fully-summed block
/// `k..w` of an m×m front: first acceptable 1×1 (current column, then
/// ascending scan), else the lexicographically first acceptable 2×2 under
/// the Duff–Reid growth bound, else a typed failure report.
fn choose_pivot(
    f: &[f64],
    m: usize,
    w: usize,
    k: usize,
    opts: &LdltOptions,
    zero_tol: f64,
) -> PivotChoice {
    let u = opts.pivot_threshold;
    let mut best_abs = 0.0f64;
    let mut required = zero_tol;
    for q in k..w {
        let dq = f[q + q * m].abs();
        let gq = gamma_of(f, m, k, q);
        if dq.is_finite() {
            best_abs = best_abs.max(dq);
        }
        if gq.is_finite() {
            required = required.max(u * gq).max(zero_tol);
        }
        if dq.is_finite() && gq.is_finite() && dq > zero_tol && dq >= u * gq {
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
                if !(e11.is_finite() && e21.is_finite() && e22.is_finite()) {
                    continue;
                }
                let det = e11 * e22 - e21 * e21;
                if !det.is_finite() || det.abs() <= det_tol {
                    continue;
                }
                let inv11 = e22 / det;
                let inv21 = -e21 / det;
                let inv22 = e11 / det;
                if !(inv11.is_finite() && inv21.is_finite() && inv22.is_finite()) {
                    continue;
                }
                let gp = gamma_pair(f, m, k, p, q);
                let gq = gamma_pair(f, m, k, q, p);
                if !(gp.is_finite() && gq.is_finite()) {
                    continue;
                }
                let g1 = (e22.abs() * gp + e21.abs() * gq) / det.abs();
                let g2 = (e21.abs() * gp + e11.abs() * gq) / det.abs();
                if g1.is_finite() && g2.is_finite() && g1.max(g2) <= 1.0 / u {
                    return PivotChoice::Two(p, q);
                }
            }
        }
    }
    PivotChoice::Fail { best_abs, required }
}

/// Largest magnitude in local column `c` among rows `k..m` excluding both
/// pivot-pair rows.
fn gamma_pair(f: &[f64], m: usize, k: usize, c: usize, other: usize) -> f64 {
    let mut g = 0.0f64;
    for i in k..m {
        if i != c && i != other {
            let v = f[i + c * m].abs();
            if !v.is_finite() {
                return f64::NAN;
            }
            g = g.max(v);
        }
    }
    g
}

/// Symmetric row+column swap of a full col-major m×m matrix.
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

/// A pivot's update of the panel block, applied after the fully-summed columns are eliminated.
enum Deferred {
    /// 1×1 pivot at local column `col` with value `d` (L's column holds the scaled entries).
    One { col: usize, d: f64 },
    /// 2×2 pivot at `col`, `col + 1`: the panel rows' entries of both columns before scaling.
    Two {
        col: usize,
        a: Vec<f64>,
        b: Vec<f64>,
    },
}

#[derive(Debug, Clone, Copy)]
enum DBlock {
    Single(f64),
    Pair(f64, f64, f64), // (d11, d21, d22)
}

#[derive(Debug, Clone)]
struct SnodeNum {
    /// Block columns in PIVOT order (elimination-order ids).
    block_vars: Vec<usize>,
    /// Panel rows (elimination-order ids, ascending).
    panel_vars: Vec<usize>,
    /// Dense unit-diagonal trapezoid, col-major (block+panel) × width.
    l: Vec<f64>,
    /// D blocks in pivot order.
    d: Vec<DBlock>,
}

/// Numeric LDLᵀ factorization: P·A·Pᵀ = L·D·Lᵀ. Produced by
/// [`SymbolicLdlt::factor`]; serves any number of right-hand sides via
/// [`LdltFactor::solve`] and exposes the Sylvester [`Inertia`] that
/// spectrum-slicing certification consumes.
#[derive(Debug, Clone)]
pub struct LdltFactor {
    n: usize,
    perm: Vec<usize>,
    snodes: Vec<SnodeNum>,
    inertia: Inertia,
    stats: FactorStats,
    /// Subtrees the solves run on separate threads (`parallel` feature; empty otherwise).
    tasks: Vec<SolveTask>,
}

impl LdltFactor {
    /// Sylvester inertia of A (sign counts of D). `zero` is structurally
    /// absent: singular pivots refuse during factorization.
    #[must_use]
    pub fn inertia(&self) -> Inertia {
        self.inertia
    }

    /// Work/memory/pivot accounting of the factorization.
    #[must_use]
    pub fn stats(&self) -> &FactorStats {
        &self.stats
    }

    /// Solve A·x = b. Panics on length mismatch (programmer error).
    #[must_use]
    pub fn solve(&self, b: &[f64]) -> Vec<f64> {
        assert_eq!(b.len(), self.n, "solve: rhs length must equal n");
        // y indexed by elimination-order id.
        let mut y = vec![0.0f64; self.n];
        for (k, &o) in self.perm.iter().enumerate() {
            y[k] = b[o];
        }
        // Forward: L z = P b. (One gather buffer for every supernode; slice loops, same arithmetic.)
        let mut v: Vec<f64> = Vec::new();
        if self.tasks.is_empty() {
            for s in &self.snodes {
                forward_step(s, &mut y, &mut v);
            }
        } else {
            self.forward_in_tasks(&mut y, &mut v);
        }
        // Diagonal: D w = z.
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
        // Backward: Lᵀ x = w.
        if self.tasks.is_empty() {
            for s in self.snodes.iter().rev() {
                backward_step(s, &mut y, &mut v);
            }
        } else {
            self.backward_in_tasks(&mut y, &mut v);
        }
        // Un-permute.
        let mut x = vec![0.0f64; self.n];
        for (k, &o) in self.perm.iter().enumerate() {
            x[o] = y[k];
        }
        x
    }

    /// The tasks' own variables as disjoint pieces of `y`, in task order.
    fn task_pieces<'y>(&self, y: &'y mut [f64]) -> Vec<(&SolveTask, &'y mut [f64])> {
        let mut pieces = Vec::with_capacity(self.tasks.len());
        let mut rest = y;
        let mut at = 0;
        for task in &self.tasks {
            let (_, tail) = std::mem::take(&mut rest).split_at_mut(task.vars.0 - at);
            let (own, tail) = tail.split_at_mut(task.vars.1 - task.vars.0);
            pieces.push((task, own));
            rest = tail;
            at = task.vars.1;
        }
        pieces
    }

    /// The forward substitution with each task's subtree on its own thread. A subtree's variables
    /// are updated only by its own supernodes, so each task finishes its own; the products it would
    /// subtract from variables above it are recorded in order instead, and subtracted in supernode
    /// order between the remaining supernodes: every variable receives the serial pass's
    /// subtractions in the serial order.
    fn forward_in_tasks(&self, y: &mut [f64], v: &mut Vec<f64>) {
        let records = crate::par::map_owned(self.task_pieces(y), |_, (task, own)| {
            let (mut v, mut outside, mut out) = (Vec::new(), Vec::new(), Vec::new());
            for s in &self.snodes[task.sids.0..=task.sids.1] {
                forward_step_within(s, own, task.vars.0, &mut v, &mut outside, &mut out);
            }
            out
        });
        let (mut sid, mut t) = (0, 0);
        while sid < self.snodes.len() {
            if self.tasks.get(t).is_some_and(|task| task.sids.0 == sid) {
                for &(i, product) in &records[t] {
                    y[i] -= product;
                }
                sid = self.tasks[t].sids.1 + 1;
                t += 1;
            } else {
                forward_step(&self.snodes[sid], y, v);
                sid += 1;
            }
        }
    }

    /// The backward substitution: the supernodes outside the tasks first, last to first (each reads
    /// only its ancestors, none of them in a task), then each task's subtree on its own thread,
    /// reading the variables above it from a copy of the finished ones.
    fn backward_in_tasks(&self, y: &mut [f64], v: &mut Vec<f64>) {
        let (mut sid, mut t) = (self.snodes.len(), self.tasks.len());
        while sid > 0 {
            if t > 0 && self.tasks[t - 1].sids.1 + 1 == sid {
                sid = self.tasks[t - 1].sids.0;
                t -= 1;
            } else {
                sid -= 1;
                backward_step(&self.snodes[sid], y, v);
            }
        }
        let finished = y.to_vec();
        crate::par::map_owned(self.task_pieces(y), |_, (task, own)| {
            let mut v = Vec::new();
            for s in self.snodes[task.sids.0..=task.sids.1].iter().rev() {
                backward_step_within(s, own, task.vars.0, &finished, &mut v);
            }
        });
    }
}

/// A subtree task of the solves: supernodes `sids.0..=sids.1` and the variables (elimination-order
/// ids) `vars.0..vars.1` their symbolic columns span, which only they and their ancestors touch.
#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "parallel"), allow(dead_code))] // only the `parallel` feature makes tasks
struct SolveTask {
    sids: (usize, usize),
    vars: (usize, usize),
}

/// One supernode of the forward substitution on the whole of `y`.
fn forward_step(s: &SnodeNum, y: &mut [f64], v: &mut Vec<f64>) {
    let w = s.block_vars.len();
    let m = w + s.panel_vars.len();
    v.clear();
    v.extend(s.block_vars.iter().map(|&i| y[i]));
    v.extend(s.panel_vars.iter().map(|&i| y[i]));
    for k in 0..w {
        let vk = v[k];
        let col = &s.l[k * m + k + 1..(k + 1) * m];
        for (vi, &l) in v[k + 1..].iter_mut().zip(col) {
            *vi -= l * vk;
        }
    }
    for (k, &i) in s.block_vars.iter().enumerate() {
        y[i] = v[k];
    }
    for (k, &i) in s.panel_vars.iter().enumerate() {
        y[i] = v[w + k];
    }
}

/// [`forward_step`] on a task's own variables `own` (ids from `r0`): a panel variable outside them
/// gets no value here; each product the serial step would subtract from it is pushed to `out`, in
/// the serial step's order. (`outside` is scratch.)
fn forward_step_within(
    s: &SnodeNum,
    own: &mut [f64],
    r0: usize,
    v: &mut Vec<f64>,
    outside: &mut Vec<usize>,
    out: &mut Vec<(usize, f64)>,
) {
    let w = s.block_vars.len();
    let m = w + s.panel_vars.len();
    let end = r0 + own.len();
    let inside = |i: usize| (r0..end).contains(&i);
    outside.clear();
    outside.extend((0..s.panel_vars.len()).filter(|&p| !inside(s.panel_vars[p])));
    v.clear();
    v.extend(s.block_vars.iter().map(|&i| own[i - r0]));
    v.extend(
        s.panel_vars
            .iter()
            .map(|&i| if inside(i) { own[i - r0] } else { 0.0 }),
    );
    for k in 0..w {
        let vk = v[k];
        let col = &s.l[k * m + k + 1..(k + 1) * m];
        for (vi, &l) in v[k + 1..].iter_mut().zip(col) {
            *vi -= l * vk;
        }
        for &p in outside.iter() {
            out.push((s.panel_vars[p], s.l[k * m + w + p] * vk));
        }
    }
    for (k, &i) in s.block_vars.iter().enumerate() {
        own[i - r0] = v[k];
    }
    for (k, &i) in s.panel_vars.iter().enumerate() {
        if inside(i) {
            own[i - r0] = v[w + k];
        }
    }
}

/// One supernode of the backward substitution on the whole of `y`.
fn backward_step(s: &SnodeNum, y: &mut [f64], v: &mut Vec<f64>) {
    let w = s.block_vars.len();
    let m = w + s.panel_vars.len();
    v.clear();
    v.extend(s.block_vars.iter().map(|&i| y[i]));
    v.extend(s.panel_vars.iter().map(|&i| y[i]));
    // Each column's panel rows first (they do not depend on this block's solve, and four
    // partial sums keep the multiply-adds from forming one serial chain), then its block rows.
    for k in (0..w).rev() {
        let col = &s.l[k * m..(k + 1) * m];
        let mut acc = v[k] - dot4(&col[w..], &v[w..]);
        for (&l, &vi) in col[k + 1..w].iter().zip(&v[k + 1..w]) {
            acc -= l * vi;
        }
        v[k] = acc;
    }
    for (k, &i) in s.block_vars.iter().enumerate() {
        y[i] = v[k];
    }
}

/// [`backward_step`] on a task's own variables `own` (ids from `r0`), reading every other variable
/// from `finished`.
fn backward_step_within(
    s: &SnodeNum,
    own: &mut [f64],
    r0: usize,
    finished: &[f64],
    v: &mut Vec<f64>,
) {
    let w = s.block_vars.len();
    let m = w + s.panel_vars.len();
    let end = r0 + own.len();
    let value = |own: &[f64], i: usize| {
        if (r0..end).contains(&i) {
            own[i - r0]
        } else {
            finished[i]
        }
    };
    v.clear();
    v.extend(s.block_vars.iter().map(|&i| own[i - r0]));
    v.extend(s.panel_vars.iter().map(|&i| value(own, i)));
    for k in (0..w).rev() {
        let col = &s.l[k * m..(k + 1) * m];
        let mut acc = v[k] - dot4(&col[w..], &v[w..]);
        for (&l, &vi) in col[k + 1..w].iter().zip(&v[k + 1..w]) {
            acc -= l * vi;
        }
        v[k] = acc;
    }
    for (k, &i) in s.block_vars.iter().enumerate() {
        own[i - r0] = v[k];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse::Coo;

    fn lcg(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*seed >> 11) as f64) / (1u64 << 53) as f64 - 0.5
    }

    /// tridiag(-1, 2, -1): eigenvalues 2 − 2cos(kπ/(n+1)), k = 1..n.
    fn tridiag(n: usize) -> Csr {
        let mut coo = Coo::new(n, n);
        for i in 0..n {
            coo.push(i, i, 2.0);
            if i > 0 {
                coo.push(i, i - 1, -1.0);
            }
            if i + 1 < n {
                coo.push(i, i + 1, -1.0);
            }
        }
        coo.assemble()
    }

    fn shifted(a: &Csr, sigma: f64) -> Csr {
        let n = a.nrows();
        let mut coo = Coo::new(n, n);
        for r in 0..n {
            let (cols, vals) = a.row(r);
            for (&c, &v) in cols.iter().zip(vals) {
                coo.push(r, c, v);
            }
        }
        for i in 0..n {
            coo.push(i, i, -sigma);
        }
        coo.assemble()
    }

    /// Random sparse symmetric matrix with a stored diagonal.
    fn random_sym(n: usize, extra_per_row: usize, seed: u64) -> Csr {
        let mut s = seed;
        let mut coo = Coo::new(n, n);
        for i in 0..n {
            coo.push(i, i, 4.0 + lcg(&mut s));
        }
        for i in 0..n {
            for _ in 0..extra_per_row {
                let j = ((lcg(&mut s) + 0.5) * n as f64) as usize % n;
                if i != j {
                    let v = lcg(&mut s);
                    coo.push(i, j, v);
                    coo.push(j, i, v);
                }
            }
        }
        coo.assemble()
    }

    fn factor(a: &Csr, ordering: DirectOrdering, opts: &LdltOptions) -> LdltFactor {
        let sym = SymbolicLdlt::analyze(a, ordering).expect("analyze");
        sym.factor(a, opts).expect("factor")
    }

    fn max_abs(a: &Csr) -> f64 {
        let mut m = 0.0f64;
        for r in 0..a.nrows() {
            let (_, vals) = a.row(r);
            for &v in vals {
                m = m.max(v.abs());
            }
        }
        m
    }

    /// Residual gate ‖A·x − b‖∞ ≤ tol·‖A‖·(‖x‖∞ + ‖b‖∞-ish).
    fn check_solve(a: &Csr, f: &LdltFactor, seed: u64, tol_scale: f64) {
        let n = a.nrows();
        let mut s = seed;
        let x_true: Vec<f64> = (0..n).map(|_| lcg(&mut s)).collect();
        let mut b = vec![0.0; n];
        a.spmv(&x_true, &mut b);
        let x = f.solve(&b);
        let mut ax = vec![0.0; n];
        a.spmv(&x, &mut ax);
        let xin: f64 = x.iter().fold(0.0f64, |m, &v| m.max(v.abs()));
        let tol = tol_scale * (n as f64) * f64::EPSILON * max_abs(a) * xin.max(1.0);
        for i in 0..n {
            assert!(
                (ax[i] - b[i]).abs() <= tol,
                "solve residual row {i}: {} vs tol {tol}",
                (ax[i] - b[i]).abs()
            );
        }
    }

    #[test]
    fn etree_matches_hand_example() {
        // Davis, Direct Methods, ch. 4 style: pattern chosen so the tree is
        // hand-checkable. Lower entries: (1,0),(2,1),(3,0),(3,2).
        let mut coo = Coo::new(4, 4);
        for i in 0..4 {
            coo.push(i, i, 4.0);
        }
        for (r, c) in [(1usize, 0usize), (2, 1), (3, 0), (3, 2)] {
            coo.push(r, c, -1.0);
            coo.push(c, r, -1.0);
        }
        let a = coo.assemble();
        let iperm: Vec<usize> = (0..4).collect();
        let ladj = build_lower_adj(&a, &iperm);
        let parent = etree(&ladj);
        // col 0: first lower neighbor is row 1 → parent 1; col 1 → 2; col 2 → 3.
        assert_eq!(parent, vec![1, 2, 3, NONE]);
    }

    #[test]
    fn amd_is_a_permutation_on_assorted_graphs() {
        for a in [
            tridiag(17),
            random_sym(40, 3, 7),
            crate::sparse::tests::laplacian_2d(8),
        ] {
            let p = amd_order(&a);
            let n = a.nrows();
            let mut seen = vec![false; n];
            for &v in &p {
                assert!(v < n && !seen[v], "amd emitted a non-permutation");
                seen[v] = true;
            }
        }
    }

    #[test]
    fn amd_beats_natural_ordering_on_the_arrow_matrix() {
        // Arrow pointing the WRONG way: dense first row/column. Natural
        // ordering eliminates column 0 first and fills completely; any
        // minimum-degree ordering eliminates the pendant nodes first and
        // keeps L linear in n.
        let n = 200;
        let mut coo = Coo::new(n, n);
        for i in 0..n {
            coo.push(i, i, 4.0);
        }
        for j in 1..n {
            coo.push(0, j, -1.0);
            coo.push(j, 0, -1.0);
        }
        let a = coo.assemble();
        let nat = SymbolicLdlt::analyze(&a, DirectOrdering::Natural).unwrap();
        let amd = SymbolicLdlt::analyze(&a, DirectOrdering::Amd).unwrap();
        assert!(
            nat.nnz_l() > n * (n - 1) / 4,
            "natural ordering should fill catastrophically (got {})",
            nat.nnz_l()
        );
        assert!(
            amd.nnz_l() < 4 * n,
            "amd must keep the arrow sparse (got {})",
            amd.nnz_l()
        );
        println!(
            "{{\"suite\":\"fs-sparse-direct\",\"case\":\"amd-vs-natural-arrow\",\"nnz_l_natural\":{},\"nnz_l_amd\":{},\"verdict\":\"pass\"}}",
            nat.nnz_l(),
            amd.nnz_l()
        );
    }

    #[test]
    fn spd_tridiagonal_solve_and_inertia() {
        let n = 50;
        let a = tridiag(n);
        let f = factor(&a, DirectOrdering::Amd, &LdltOptions::default());
        assert_eq!(
            f.inertia(),
            Inertia {
                positive: n,
                negative: 0
            }
        );
        assert_eq!(f.stats().pivots_2x2, 0, "SPD must factor with 1x1 pivots");
        check_solve(&a, &f, 11, 8.0);
    }

    #[test]
    fn zero_diagonal_indefinite_needs_the_2x2_path() {
        // [[0,1],[1,0]] — eigenvalues ±1; no 1×1 pivot exists.
        let mut coo = Coo::new(2, 2);
        coo.push(0, 1, 1.0);
        coo.push(1, 0, 1.0);
        let a = coo.assemble();
        let f = factor(&a, DirectOrdering::Natural, &LdltOptions::default());
        assert_eq!(
            f.inertia(),
            Inertia {
                positive: 1,
                negative: 1
            }
        );
        assert_eq!(f.stats().pivots_2x2, 1);
        check_solve(&a, &f, 3, 8.0);

        // MUTATION CONTROL: with 2×2 pivots disabled, the same matrix must
        // REFUSE with the named breakdown — proving the 2×2 path is
        // load-bearing, not decorative.
        let sym = SymbolicLdlt::analyze(&a, DirectOrdering::Natural).unwrap();
        let opts = LdltOptions {
            allow_2x2: false,
            ..LdltOptions::default()
        };
        let err = sym.factor(&a, &opts).unwrap_err();
        assert!(
            matches!(err, LdltError::PivotBreakdown { .. }),
            "expected pivot breakdown, got {err}"
        );
        assert!(err.to_string().contains("FS-SPARSE-DIRECT-PIVOT-BREAKDOWN"));
    }

    #[test]
    fn saddle_point_kkt_inertia() {
        // [[A, Bᵀ],[B, 0]] with A = 2·I₃ SPD, B = [[1,0,0],[0,1,0]] full
        // row rank → inertia (3, 2) by the standard KKT result.
        let mut coo = Coo::new(5, 5);
        for i in 0..3 {
            coo.push(i, i, 2.0);
        }
        for (bi, &c) in [0usize, 1usize].iter().enumerate() {
            coo.push(3 + bi, c, 1.0);
            coo.push(c, 3 + bi, 1.0);
        }
        let a = coo.assemble();
        let f = factor(&a, DirectOrdering::Amd, &LdltOptions::default());
        assert_eq!(
            f.inertia(),
            Inertia {
                positive: 3,
                negative: 2
            }
        );
        check_solve(&a, &f, 5, 8.0);
    }

    #[test]
    fn inertia_certifies_analytic_eigenvalue_counts_across_shifts() {
        // THE spectrum-slicing seed property: for K = tridiag(-1,2,-1) the
        // eigenvalues are λ_k = 2 − 2cos(kπ/(n+1)), so the inertia of
        // K − σI must equal the analytic counts on both sides of σ.
        let n = 60;
        let k_mat = tridiag(n);
        let sym = SymbolicLdlt::analyze(&k_mat, DirectOrdering::Amd).unwrap();
        for sigma in [0.1, 0.5, 1.0, 2.0, 3.7] {
            let a = shifted(&k_mat, sigma);
            let f = sym.factor(&a, &LdltOptions::default()).expect("factor");
            let below = (1..=n)
                .filter(|&k| {
                    2.0 - 2.0 * (k as f64 * std::f64::consts::PI / (n as f64 + 1.0)).cos() < sigma
                })
                .count();
            assert_eq!(
                f.inertia(),
                Inertia {
                    positive: n - below,
                    negative: below
                },
                "inertia mismatch at sigma={sigma}"
            );
        }
        println!(
            "{{\"suite\":\"fs-sparse-direct\",\"case\":\"inertia-vs-analytic\",\"shifts\":5,\"verdict\":\"pass\"}}"
        );
    }

    #[test]
    fn reconstruction_and_solve_on_random_symmetric_matrices() {
        for (n, extra, seed) in [(12usize, 2usize, 1u64), (40, 3, 2), (80, 5, 3)] {
            let a = random_sym(n, extra, seed);
            let f = factor(&a, DirectOrdering::Amd, &LdltOptions::default());
            assert_eq!(f.inertia().positive + f.inertia().negative, n);
            check_solve(&a, &f, seed ^ 0xABCD, 64.0);
        }
    }

    #[test]
    fn indefinite_random_matrices_solve_within_gate() {
        // Indefinite: flip diagonal signs on alternating rows.
        for (n, seed) in [(30usize, 21u64), (70, 22)] {
            let base = random_sym(n, 3, seed);
            let mut coo = Coo::new(n, n);
            for r in 0..n {
                let (cols, vals) = base.row(r);
                for (&c, &v) in cols.iter().zip(vals) {
                    if r == c && r % 2 == 1 {
                        coo.push(r, c, -v);
                    } else {
                        coo.push(r, c, v);
                    }
                }
            }
            let a = coo.assemble();
            let f = factor(&a, DirectOrdering::Amd, &LdltOptions::default());
            assert!(f.inertia().negative >= n / 4, "should be indefinite");
            check_solve(&a, &f, seed ^ 0x77, 64.0);
        }
    }

    #[test]
    fn factorization_is_bitwise_deterministic() {
        let a = random_sym(60, 4, 0xDE7);
        let sym = SymbolicLdlt::analyze(&a, DirectOrdering::Amd).unwrap();
        let f1 = sym.factor(&a, &LdltOptions::default()).unwrap();
        let f2 = sym.factor(&a, &LdltOptions::default()).unwrap();
        assert_eq!(f1.perm, f2.perm);
        for (s1, s2) in f1.snodes.iter().zip(&f2.snodes) {
            assert_eq!(s1.block_vars, s2.block_vars);
            assert!(
                s1.l.iter()
                    .zip(&s2.l)
                    .all(|(x, y)| x.to_bits() == y.to_bits()),
                "L values must be bitwise identical"
            );
        }
        let b: Vec<f64> = (0..60).map(|i| f64::from(i) - 30.0).collect();
        let x1 = f1.solve(&b);
        let x2 = f2.solve(&b);
        assert!(x1.iter().zip(&x2).all(|(a, b)| a.to_bits() == b.to_bits()));
        println!(
            "{{\"suite\":\"fs-sparse-direct\",\"case\":\"bitwise-determinism\",\"verdict\":\"pass\"}}"
        );
    }

    #[test]
    fn symbolic_reuse_rejects_pattern_mismatch() {
        let a = tridiag(10);
        let sym = SymbolicLdlt::analyze(&a, DirectOrdering::Amd).unwrap();
        // Same pattern, different values (a shift) → accepted.
        let shifted_a = shifted(&a, 0.25);
        assert!(sym.factor(&shifted_a, &LdltOptions::default()).is_ok());
        // Different pattern → typed refusal.
        let b = tridiag(11);
        assert_eq!(
            sym.factor(&b, &LdltOptions::default()).unwrap_err(),
            LdltError::PatternMismatch
        );
    }

    #[test]
    fn named_refusals_fire() {
        // Not square.
        let mut coo = Coo::new(2, 3);
        coo.push(0, 0, 1.0);
        let rect = coo.assemble();
        assert!(matches!(
            SymbolicLdlt::analyze(&rect, DirectOrdering::Amd),
            Err(LdltError::NotSquare { nrows: 2, ncols: 3 })
        ));
        // Structurally asymmetric.
        let mut coo = Coo::new(2, 2);
        coo.push(0, 0, 1.0);
        coo.push(1, 1, 1.0);
        coo.push(0, 1, 1.0);
        let asym = coo.assemble();
        assert!(matches!(
            SymbolicLdlt::analyze(&asym, DirectOrdering::Amd),
            Err(LdltError::StructurallyAsymmetric { row: 0, col: 1 })
        ));
        // Non-finite entry.
        let mut coo = Coo::new(2, 2);
        coo.push(0, 0, 1.0);
        coo.push(1, 1, f64::NAN);
        let nan = coo.assemble();
        let sym = SymbolicLdlt::analyze(&nan, DirectOrdering::Amd).unwrap();
        assert!(matches!(
            sym.factor(&nan, &LdltOptions::default()),
            Err(LdltError::NonFiniteEntry { row: 1, col: 1 })
        ));
        // Numerically singular (an exactly zero matrix with stored pattern).
        let mut coo = Coo::new(3, 3);
        for i in 0..3 {
            coo.push(i, i, 0.0);
        }
        let zero = coo.assemble();
        let sym = SymbolicLdlt::analyze(&zero, DirectOrdering::Amd).unwrap();
        assert!(matches!(
            sym.factor(&zero, &LdltOptions::default()),
            Err(LdltError::PivotBreakdown { .. })
        ));
        // Invalid options.
        let a = tridiag(3);
        let sym = SymbolicLdlt::analyze(&a, DirectOrdering::Amd).unwrap();
        let bad = LdltOptions {
            pivot_threshold: 0.9,
            ..LdltOptions::default()
        };
        assert!(matches!(
            sym.factor(&a, &bad),
            Err(LdltError::InvalidOptions { .. })
        ));
        // Intermediate Schur complement overflow must refuse instead of accepting ±inf as a pivot.
        let mut coo = Coo::new(2, 2);
        coo.push(0, 0, 1e305);
        coo.push(0, 1, 1e307);
        coo.push(1, 0, 1e307);
        coo.push(1, 1, 1e305);
        let schur_overflow = coo.assemble();
        let sym_so = SymbolicLdlt::analyze(&schur_overflow, DirectOrdering::Natural).unwrap();
        assert!(matches!(
            sym_so.factor(&schur_overflow, &LdltOptions::default()),
            Err(LdltError::PivotBreakdown { .. })
        ));
        // Overflowing 2x2 determinant must refuse instead of dividing by ±inf into zeroed L/D⁻¹.
        let mut coo = Coo::new(2, 2);
        coo.push(0, 1, 1e200);
        coo.push(1, 0, 1e200);
        let det_overflow = coo.assemble();
        let sym_do = SymbolicLdlt::analyze(&det_overflow, DirectOrdering::Natural).unwrap();
        assert!(matches!(
            sym_do.factor(&det_overflow, &LdltOptions::default()),
            Err(LdltError::PivotBreakdown { .. })
        ));
    }

    #[test]
    fn empty_and_singleton_matrices() {
        let empty = Coo::new(0, 0).assemble();
        let f = factor(&empty, DirectOrdering::Amd, &LdltOptions::default());
        assert_eq!(f.solve(&[]), Vec::<f64>::new());
        assert_eq!(
            f.inertia(),
            Inertia {
                positive: 0,
                negative: 0
            }
        );

        let mut coo = Coo::new(1, 1);
        coo.push(0, 0, -3.0);
        let one = coo.assemble();
        let f = factor(&one, DirectOrdering::Natural, &LdltOptions::default());
        assert_eq!(
            f.inertia(),
            Inertia {
                positive: 0,
                negative: 1
            }
        );
        let x = f.solve(&[6.0]);
        assert!((x[0] + 2.0).abs() < 1e-15);
    }

    /// With the `parallel` feature, independent subtrees are factored as tasks and large panel
    /// updates are split; on one thread the pass is serial. Every thread count gives the same bits:
    /// L, D, inertia, statistics, solves, and the refusal a serial pass meets first.
    #[cfg(feature = "parallel")]
    #[test]
    fn thread_count_changes_no_bit_of_a_factorization_or_its_refusal() {
        let on = |threads: usize, run: &(dyn Fn() -> String + Sync)| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(run)
        };
        // Two singular 2×2 blocks in different subtrees: the serial pass refuses at the first.
        let blocks = |parts: &[Csr]| {
            let n: usize = parts.iter().map(Csr::nrows).sum();
            let mut coo = Coo::new(n, n);
            let mut base = 0;
            for p in parts {
                for r in 0..p.nrows() {
                    let (cols, vals) = p.row(r);
                    for (&c, &v) in cols.iter().zip(vals) {
                        coo.push(base + r, base + c, v);
                    }
                }
                base += p.nrows();
            }
            coo.assemble()
        };
        let singular = |v: f64| {
            let mut coo = Coo::new(2, 2);
            for (r, c) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                coo.push(r, c, v);
            }
            coo.assemble()
        };
        let grid = crate::sparse::tests::laplacian_2d(20);
        let cases = [
            crate::sparse::tests::laplacian_2d(60),
            shifted(&crate::sparse::tests::laplacian_2d(40), 0.37),
            random_sym(2000, 3, 0xBEEF),
            blocks(&[grid.clone(), singular(1.0), grid, singular(2.0)]),
            // Large enough for subtree tasks in the solves too (each of at least SOLVE_TASK_ENTRIES).
            crate::sparse::tests::laplacian_2d(160),
        ];
        let solve_tasks = on(4, &|| {
            let a = &cases[4];
            let sym = SymbolicLdlt::analyze(a, DirectOrdering::Amd).unwrap();
            format!(
                "{}",
                sym.factor(a, &LdltOptions::default()).unwrap().tasks.len()
            )
        });
        assert_ne!(
            solve_tasks, "0",
            "the large grid's solves run subtree tasks"
        );
        let mut split = 0;
        for (case, a) in cases.iter().enumerate() {
            let sym = SymbolicLdlt::analyze(a, DirectOrdering::Amd).unwrap();
            let b: Vec<f64> = (0..a.nrows()).map(|i| (i % 17) as f64 - 8.0).collect();
            let run = || match sym.factor(a, &LdltOptions::default()) {
                Ok(f) => {
                    let x: Vec<u64> = f.solve(&b).iter().map(|v| v.to_bits()).collect();
                    format!("{:?} {:?} {:?} {x:?}", f.snodes, f.inertia, f.stats)
                }
                Err(e) => format!("refused: {e:?}"),
            };
            let serial = on(1, &run);
            assert_eq!(serial, on(4, &run), "case {case}");
            assert_eq!(serial, on(3, &run), "case {case}");
            assert_eq!(serial.starts_with("refused"), case == 3, "case {case}");
            split += usize::from(on(4, &|| format!("{}", sym.subtree_tasks().len())) != "0");
        }
        assert!(split >= 3, "subtree tasks ran in {split} cases");
    }

    #[test]
    fn grid_laplacian_factors_under_both_orderings_with_fill_recorded() {
        let a = crate::sparse::tests::laplacian_2d(16); // 256 unknowns
        let nat = SymbolicLdlt::analyze(&a, DirectOrdering::Natural).unwrap();
        let amd = SymbolicLdlt::analyze(&a, DirectOrdering::Amd).unwrap();
        let f = amd.factor(&a, &LdltOptions::default()).unwrap();
        assert_eq!(f.inertia().negative, 0, "grid Laplacian is SPD");
        check_solve(&a, &f, 31, 64.0);
        println!(
            "{{\"suite\":\"fs-sparse-direct\",\"case\":\"grid-fill\",\"n\":256,\"nnz_l_natural\":{},\"nnz_l_amd\":{},\"supernodes\":{},\"max_front\":{},\"verdict\":\"pass\"}}",
            nat.nnz_l(),
            amd.nnz_l(),
            f.stats().supernodes,
            f.stats().max_front_dim
        );
    }
}
