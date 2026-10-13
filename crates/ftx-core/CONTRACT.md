# ftx-core contract

**Purpose.** Foundations shared by every crate: deterministic elementary functions, the exact 3D
orientation predicate, vectors and 3×3 matrices, minimum-norm least squares, the numpy/Python semantics the reference's numbers and messages
depend on, chord-relative edge curves, polyline and polygon helpers, refusals and run receipts. No domain
logic (no pieces, seams, bodies or garments).

**Units and frames.** Pattern geometry: millimetres, 2D, x right, y up, loops counter-clockwise.
Body and drape geometry: metres, 3D, z up. Nothing here converts between them.

**Invariants.**
- Curves: `[t, h]` control points are relative to the chord (`docs/spec/01` §4.1). Sample counts are part
  of the contract: `EDGE_DENSE` 200, `OUTLINE_DENSE` 60, `MEASURE_DENSE` 120, `TRANSFER_DENSE` 60.
- Helpers follow `docs/spec/01` §5 exactly, including clamping, repeated-abscissa and half-open rules.
- Library code does not print or read the clock. It panics only on documented caller-contract violations
  (`# Panics`: an empty polyline, mismatched slice lengths, a non-finite point given to `orient3d`),
  never on the values in a document.
- `exact::orient3d` returns the exact sign of the orientation determinant (Shewchuk's stage-A filter,
  then expansion arithmetic), lifted from frankensim `e76c9421` (`fs-ivl` predicates and expansions,
  `fs-math` error-free transformations). Exact while coordinate differences and their cubes stay in the
  normal f64 range; checked against an i128 oracle on 4,000 lattice quadruples (many exactly coplanar).

**Determinism class.** Bit-identical across targets. The elementary functions use only correctly rounded
IEEE operations (lifted from frankensim `fs-math/src/det.rs` at `f58c1acb`, unchanged bodies; trig refuses
|x| > 2²⁰ with NaN instead of Payne–Hanek reduction). Sums run in fixed orders (numpy's pairwise order for
totals). Checked: frankensim's 25,000-sample golden hash and the fixtures below pass on aarch64-apple,
x86-64-linux and wasm32-wasip1 (Node WASI).

**Parity with the reference.** Bit-exact with numpy 2.5.4 for `linspace`, `cumsum`, `sum`, `interp`,
segment norms and quadratic/linear Bézier samples. Cubic Bézier samples use a correctly rounded x³ where
numpy calls the platform `pow` (≤ 1 ULP apart; numpy itself differs between platforms there). Areas and
least-squares tangents agree to ~1e-15 relative (numpy uses BLAS/LAPACK orders we cannot reproduce).

**Fixtures.** `docs/spec/01` §14.2 F1–F6 (values from the Python reference), as unit tests in `curve.rs`
and `polyline.rs`; oracle capture into `ftx-conformance` follows with FTX-02.

**Error model.** `CurveError` carries the reference's message text (`zero-length chord`, `quad curve
needs 1 control points`, `unknown curve type 'spline'`). `Refusal` = stable `RefusalCode` + message +
ranked repairs, serialisable. Functions that cannot fail on valid input return plain values; NaN in, NaN
out, and callers refuse non-finite values with `Refusal::finite`.

**No claims.** `repr_str` escapes control, separator, format and private-use characters but not
unassigned code points (needs Unicode tables). `ulp_distance` is meaningful for finite values of the same
sign. Receipts identify bytes, not meaning: two semantically equal JSON files with different formatting
have different digests.
