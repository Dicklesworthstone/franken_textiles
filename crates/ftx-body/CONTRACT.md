# ftx-body contract

**Purpose.** The Anny parametric body model (anny 0.6.1, the `create_fullbody_model` default
configuration of `docs/spec/05` §0) evaluated in Rust: phenotype and local-change coefficients, rest shape
and skeleton, local-bone forward kinematics, linear blend skinning, anthropometry, and the WHO
years-to-age-parameter map; and the body as the drape needs it (`drape.rs`): poses, pose ramps, a nappy
and the checked skin mesh. Fitting and tape functionals come next (FTX-09).

**Drape poses** (definition in the `drape.rs` module notes). A joint turns its bone about its head by φ
about an axis in the rest pose's world frame; its local-bone delta is `φ·R_restᵀ·a`. Arms
(`upperarm01.L/R`): abduction θ from vertical in the plane of the rest arm and the vertical (rest ≈ 43°;
A-pose 35°, also 50° and arms up 150°). Legs about +X: seated = hips −90°, knees +90°; squat = hips −100°,
knees +110°, feet −10° (trunk upright). A ramp scales every φ by t. Nappy of size s:
`buttocks-volume-incr` += s, `hip-scale-depth-incr` += s/2 (s = 1 adds about 113 mm to a 2.5-year-old's
torso seat girth). The skin is the closed shell enclosing the most volume (the mesh also holds two eyes
and a tongue), refused unless every edge has exactly two oppositely wound triangles, no triangle is
degenerate and the volume is positive. `examples/posed_skins.rs` writes the poses as OBJ for a look.

**Data.** `tools/body-model/convert_anny.py` builds the model from the pinned package into a fresh cache,
checks the installed files against the wheel's RECORD and every array against spec 05 §8.1, and writes a
`.ftxbody` file (~152 MB, outside the repository; `FTX_ANNY_MODEL` or
`~/.cache/franken_textiles/anny-0.6.1-default.ftxbody`). The reader refuses a wrong magic, version,
dtype, shape, index range, bone order or (with `verify`) SHA-256. Blend shapes are CSR-sparse f64,
reconstructing the package's dense array exactly.

**Units and frames.** Metres, +Z up, the body faces −Y, +X is the body's left. Rest bodies sit in the
template frame (soles near z = −0.82 m); posed bodies are root-relative. Nothing is grounded here.

**Parameters.** gender 0 = male, 1 = female. Age is morphological (−1/3 newborn anchor … 1), not years:
use `age_parameter_for_years`. Out-of-range phenotype values clamp; local changes extrapolate linearly
beyond ±1 and are absolute offsets authored on the adult mesh (spec 05 §3). Batch size 1.

**Oracle and fixtures.** `tests/fixtures/oracle/body/anny-0.6.1-reference.json` (16 cases from
`tools/body-model/capture_anny_reference.py`: newborn anchor, birth, 1 y, toddler, 6 y, 12 y, adults,
extremes, out-of-range, local changes beyond ±1, two poses). `tests/anny_parity.rs` compares selected
vertices, bounding boxes, checksums, bone frames and poses, coefficients and anthropometry (1e-9 m and
1e-10 relative; measured 1.05e-14 m and 1.1e-13), and re-hashes every loaded array to the package's bytes.
`tests/fixtures/oracle/body/anny-0.6.1-poses.json` (9 cases from `tools/body-model/capture_anny_poses.py`,
which implements the pose definition again in NumPy and runs the package with it): `tests/drape_poses.rs`
compares joint axes, angles and local rotation vectors (worst 3.8e-15), posed vertices (8.8e-16 m),
checksums, and the skin shell's counts, vertex order, volume and area (5.7e-14 relative).
These tests are `#[ignore]`d without the converted model; run them with `--include-ignored`.

**Determinism class.** Fixed-order f64 arithmetic with `ftx-core::det` trig: bit-identical across targets.
The stored bone-roll matrices are the package's float32 values (not exactly orthonormal); the rigid
inverse uses their transpose, as the package does.

**Error model.** `Refusal`: `io` (file), `invalid_document` (malformed or inconsistent data, a missing
bone or local change), `non_finite` (a parameter, delta or vertex), `out_of_range` (wrong vector lengths,
an abduction outside 0–180°, a ramp outside 0–1, a negative nappy), `degenerate_geometry` (an open,
non-manifold or inconsistently wound skin, a degenerate triangle), `inward_wound` (negative volume).

**No claims.** Fit quality against real people (that is FTX-09 and gate G1); the shape distribution's
weight and muscle priors (spec 05 §9.11); anything about the SMPL family or the `rig="anny"` preset.
Poses are plain linear blend skinning without correctives: seated and squat bodies show webbing at the
crotch and lose 4–6 % of their volume (2.5-year-old: −4.4 % seated, −5.4 % squat), and the skin is not checked for self-intersection there. The
nappy is a shape proxy, not a garment.
