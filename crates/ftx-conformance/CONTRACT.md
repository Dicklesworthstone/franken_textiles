# ftx-conformance contract

**Purpose.** Differential, property, and determinism verification of the workspace crates against the
pinned reference implementations (plan §17.1–17.3):
- `ftx-oracle/1` fixtures under `tests/fixtures/oracle/<subsystem>/<name>.json`, captured exclusively
  from the pinned Python `fabric-pattern-creation-and-alteration` reference (`tools/oracle/`).
- Determinism audits (`detaudit`) enforcing cross-target floating-point hygiene (`no_platform_libm`,
  fixed reduction orders, explicit `mul_add` rules).
- Deterministic property and metamorphic checks (`propcheck`) across geometry, alteration, annotation,
  and polygon buffering.

Body-model parity against pinned upstream `anny==0.6.1` (`tools/body-model/PIN.md`) lives next to the
body crate in `crates/ftx-body/tests/{anny_parity.rs,drape_poses.rs,fit_tape.rs}` because those suites
require the external converted `.ftxbody` asset outside Git.

**Units and frames.** Each suite compares values in the owning crate's documented coordinate frame
(2D pattern/export millimetres or crop pixels; 3D garment/start/metric metres and millimetres).
`ftx-conformance` performs no unit conversion of its own.

**Oracle pins and non-regeneration law.**
- Python skill reference: revision `88e03965` (`tools/oracle/`).
- Body model reference: `anny==0.6.1` (`tools/body-model/PIN.md`).
- **Never regenerate an oracle fixture from the Rust candidate.** Every mismatch is a candidate defect
  or an explicitly documented parity exception in `FEATURE_PARITY.md` with a dedicated regression test.

**Comparison classes (`Compare`).**
- **Byte-exact (`tol = 0.0`)**: SVG, tiled/copy-shop/projector PDFs, DXF-ASTM/AAMA, HPGL, cutter,
  GarmentCode, Tajima DST, `PyJson` formatting, `export_helpers`, `tracing` (`post.json`), and
  `trace_image` (`raster_trace.json`).
- **Bounded relative tolerance (`|got − want| ≤ tol · max(1, |want|)`)**: stated per fixture where
  upstream numpy/LAPACK/GEOS/OpenCV HAL reductions differ in the last bits (`Compare::finish` prints
  exact count, bit-exact percentage, and worst relative deviation).

**Determinism class.** Deterministic across `aarch64`, `x86_64`, and `wasm32`: seeded generators use
explicit deterministic PRNGs (`ftx_core::philox`), never `HashMap` iteration order or wall-clock state.

**Error model.** `fixture(subsystem, name)` panics immediately if a tracked `ftx-oracle/1` file is
missing or malformed (broken checkout). `Compare::finish` aggregates up to 40 mismatch lines before
failing the test so regressions report complete context in one pass.

**No claims.** Passing `ftx-conformance` establishes **Parity** on the captured reference corpus and
internal numerical/property invariants only. It never establishes **Physics** beyond the reference's
ported equations, **Fit** on a real wearer, physical machine/cutting/thermal accuracy, or **Performance**.
