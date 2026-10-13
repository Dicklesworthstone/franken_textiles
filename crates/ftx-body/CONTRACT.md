# ftx-body contract

**Purpose.** The Anny parametric body model (anny 0.6.1, the `create_fullbody_model` default
configuration of `docs/spec/05` §0) evaluated in Rust: phenotype and local-change coefficients, rest shape
and skeleton, local-bone forward kinematics, linear blend skinning, anthropometry, and the WHO
years-to-age-parameter map; and the body as the drape needs it: poses, pose ramps, a nappy, the checked
skin mesh and a trimmed drape body (`drape.rs`), and part frames, landmarks and section radii for
placing pattern pieces (`landmarks.rs`); tape girths (`tape.rs`) and fitting the body to a wearer
(`fit.rs`).

**Tape and fitting** (FTX-09, first slice). A girth is the perimeter of the convex hull of the
horizontal section loop around the torso axis (a tape bridges hollows); a vertex exactly at the cutting
height counts as above it, so sections of a closed mesh are closed loops. `tape_readings` reads, on the
posed body, stature (the rest height), chest (10 mm below the armpit), waist (at the waist loop's height),
hip (the largest girth from 20 % of the way up from the crotch to the waist, 2 mm steps), waist height
and crotch height. `fit_stature` bisects the height parameter to a stature. `fit_tape` runs
Levenberg–Marquardt (central differences, step 1e-3) over the height and weight parameters and five
`measure-*` local changes (bust, waist and hip girths, upper-leg height, waist-to-hip distance), with
1σ = 3 mm per girth and 2 mm per length and a weak prior (1σ = 1) toward the population middle; the
phenotype is clamped to [0, 1], local changes to [−1, 1]; age and gender are set, not fitted. Residuals
are returned, never hidden. Gate G1b (`tests/fit_tape.rs`): measurements drawn from the model itself
(1.5, 2.5, 4 and 30 years) are recovered within 0.03 mm (the gate asks 3 mm girths, 2 mm lengths);
the 92 cm reference toddler's six chart values are reached within 0.1 mm in 5 iterations (0.5 s), where
the stature-only body read 75 mm less at the chest, 66 mm at the waist and 62 mm at the hip.
`tape_targets` and `Wearer::from_measurements` read a `fabric-measurements/1` file (fused values
whatever their provenance: the drape tests a pattern against the measurements it was drafted to).

**Drape poses** (definition in the `drape.rs` module notes). A joint turns its bone about its head by φ
about an axis in the rest pose's world frame; its local-bone delta is `φ·R_restᵀ·a`. Arms
(`upperarm01.L/R`): abduction θ from vertical in the plane of the rest arm and the vertical (rest ≈ 43°;
A-pose 35°, also 50° and arms up 150°). Legs about +X: seated = hips −90°, knees +90°; squat = hips −100°,
knees +110°, feet −10° (trunk upright). Straight arms (`Pose::drape`, A-pose 35° with them): Anny's rest
elbow is bent about 46° with the forearm forward, so `upperarm02` turns onto `upperarm01`'s line and
`lowerarm01` onto `upperarm02`'s, and a straight sleeve can contain the arm (a bent-elbow start crossed the
forearm). A ramp scales every φ by t. Nappy of size s:
`buttocks-volume-incr` += s, `hip-scale-depth-incr` += s/2 (s = 1 adds about 113 mm to a 2.5-year-old's
torso seat girth). The skin is the closed shell enclosing the most volume (the mesh also holds two eyes
and a tongue), refused unless every edge has exactly two oppositely wound triangles, no triangle is
degenerate and the volume is positive. `trimmed_skin` leaves out the regions skinned mostly to given
bones and their descendants (the drape uses the wrists and feet: no garment in scope covers them, and a
12 mm garment mesh cannot clear a thumb) and closes each opening with a fan, checked the same way.
`examples/posed_skins.rs` writes the poses as OBJ for a look.

**Frames and landmarks** (`BodyFrames`, definitions in the `landmarks.rs` table). On the posed body, in
mm: floor, waist (Anny's waist loop), crotch, chest (the armpit), across back, hip, knee, calf, ankle,
shoulder tip, side neck point, C7 and head top, with the reference's formulas where it derives one from
others (hip = crotch + 0.45 × (waist − crotch), calf = ankle + 0.6 × (knee − ankle), across back midway
between chest and shoulder); torso, right-arm and right-leg frames in the reference's convention (spec 04
§3.2); section radii from the vertices of each region (by dominant skinning bone). On the 92 cm
reference toddler fitted to stature only: waist 525 mm (taped 530), crotch 394 (372.5), knee 248 (231),
chest 617 (the reference's formula on the tape gives 635); fitted to the tape, waist and crotch land on
the taped heights. The armpit (lowest upper-arm vertex by the torso) and the shoulder tip (0.8 ×
upper-arm radius along the arm) stay heuristics.

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
`tests/fixtures/oracle/body/anny-0.6.1-poses.json` (10 cases from `tools/body-model/capture_anny_poses.py`,
which implements the pose definition again in NumPy and runs the package with it): `tests/drape_poses.rs`
compares joint axes, angles and local rotation vectors (worst 6.0e-15), posed vertices (8.8e-16 m),
checksums, and the skin shell's counts, vertex order, volume and area (5.7e-14 relative).
These tests are `#[ignore]`d without the converted model; run them with `--include-ignored`.

**Determinism class.** Fixed-order f64 arithmetic with `ftx-core::det` trig: bit-identical across targets.
The stored bone-roll matrices are the package's float32 values (not exactly orthonormal); the rigid
inverse uses their transpose, as the package does.

**Error model.** `Refusal`: `io` (file), `invalid_document` (malformed or inconsistent data, a missing
bone or local change), `non_finite` (a parameter, delta or vertex), `out_of_range` (wrong vector lengths,
an abduction outside 0–180°, a ramp outside 0–1, a negative nappy), `degenerate_geometry` (an open,
non-manifold or inconsistently wound skin, a degenerate triangle), `inward_wound` (negative volume).

**No claims.** Fit quality against real people (gate G1 on held-out measurements; the fit reaches the
targets, which says nothing about whether the shape between them is the wearer's); fitting from weight
or BMI, from other tape measurements (thigh, neck, lengths along the body) and from photos; `MODEL_FIT`
values with σ and `ftx-body/1` output; the shape distribution's
weight and muscle priors (spec 05 §9.11); anything about the SMPL family or the `rig="anny"` preset.
Poses are plain linear blend skinning without correctives: seated and squat bodies show webbing at the
crotch and lose 4–6 % of their volume (2.5-year-old: −4.4 % seated, −5.4 % squat), and the skin is not
checked for self-intersection there. The nappy is a shape proxy, not a garment. Landmarks are heuristics
on the body (see above), not measurements.
