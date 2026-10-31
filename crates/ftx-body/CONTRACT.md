# ftx-body contract

**Purpose.** The Anny parametric body model (anny 0.6.1, the `create_fullbody_model` default
configuration of `docs/spec/05` §0) evaluated in Rust: phenotype and local-change coefficients, rest shape
and skeleton, local-bone forward kinematics, linear blend skinning, anthropometry, and the WHO
years-to-age-parameter map; and the body as the drape needs it: poses, pose ramps, a nappy, the checked
skin mesh and a trimmed drape body (`drape.rs`), and part frames, landmarks and section radii for
placing pattern pieces (`landmarks.rs`); tape girths (`tape.rs`) and fitting the body to a wearer
(`fit.rs`).

**Tape and fitting** (FTX-09). A girth is the perimeter of the convex hull of a section loop (a tape
bridges hollows); a vertex exactly at the cutting height counts as above it, so sections of a closed
mesh are closed loops. Torso girths are horizontal, around the torso axis; limb girths are perpendicular
to the right leg's or arm's part-frame axis (the vertices taken into that frame), around the axis or,
where the axis passes beside a narrow limb, the loop with the nearest section edge (`girth_near`).
Containing loops retain priority, with the outermost chosen if several contain the axis; external
distance ties retain section order. The fallback uses segment distance, not the mean of cut vertices:
subdividing a straight edge must not change which limb is measured. Nonfinite axis coordinates and
nonfinite or out-of-range slicing heights yield no reading. This is a native geometric rule, not an
independent qualification of the chosen anatomical axis. `Slicer` buckets triangles
by height (5 mm slabs) so a search over levels visits only nearby triangles (same loops, same order).
`tape_readings` reads 21 measurements on the posed body (`TAPE_FIELDS`, definitions in the `fit.rs`
table and the `tape.rs` functions): stature, chest, waist, hip, waist and crotch heights, thigh, knee,
calf, ankle, upper arm, wrist, arm length, knee height, back neck to waist (along the back's median line
on the outermost section containing the declared torso axis). Disconnected posterior limbs and nested
sections cannot replace that torso surface; a missing axis-containing section yields no reading.
The direct and height-bucketed back-point functions take the full (x, y) torso axis, in mesh units.
An independent slanted-prism control checks both the back points and the resulting straight spine path
under detached/nested sections, translation and triangle order; this qualifies section geometry only.
The remaining tape definitions are
neck base (the section across the neck axis at the `neck01` head: below it the section turns into the
trunk), head (the fullest horizontal section round the head bone's axis, 15–50 % of the way from crown to
C7), shoulder length (along the top of the shoulder in the coronal section through the shoulder joint,
from the side neck point to the shoulder point, taken as the top of the shoulder above the joint).
Shoulder crossings belong to the outermost coronal section containing that joint; detached upper
sections and nested shells cannot replace the shoulder surface. A missing joint-containing section
yields no reading. Analytic flat-shoulder controls check this geometric rule under translation and
triangle order; they do not qualify the anatomical joint or side-neck placement. Across
back (the back of the outermost horizontal section containing the torso axis 15 mm below the shoulder
points, between them, as a hull; disconnected arm sections are excluded and a missing torso section
yields no reading). The opposite shoulder x coordinate is mirrored about the declared torso axis,
not world zero, so a lateral translation preserves the tape path. This still assumes shoulder
symmetry about that axis; it does not independently qualify asymmetric anatomy. The existing
flat/ridged-back control includes positive and negative lateral offsets. Total
crotch length (the lower hull of the outermost median sagittal loops containing the declared waist
axis, within ±15 mm and clipped below the waist: a snug waist-to-waist tape bridges the gluteal cleft).
Sections belonging only to a lower foot or an offset arm do not belong to that path; absent
waist-containing loops yield no reading. The independent rectangular-path regression requires
180 mm (50 down, 80 across, 50 up), unchanged by detached or nested sections, translation and triangle
order; the previous unfiltered hull returned 236.619 mm with an unrelated lower section. This
qualifies geometric section selection, not the anatomical waist axis. Outseam runs from the waist's
side point over the hip, as a hull of the
right-side torso and leg seen from the front, to the hip's widest point, then straight down to the
floor. A straight or two-vertex projected outseam is a valid open tape path. Fewer than two profile
points remains unavailable, and the existing waist/landmark checks still apply. Independent box/frustum controls
check 100 mm vertical paths and a known diagonal plus its floor drop under translation and triangle
reordering. `fit_stature` bisects the height parameter to a stature. `fit_tape` runs Levenberg–Marquardt
(central differences, step 1e-3; stops when a step gains less than 1e-5 of the cost) over four phenotype
parameters (height, weight, muscle, proportions; prior 1σ = 0.5 about 0.5) and nineteen local changes
(fifteen `measure-*`, `measure-neck-circ-incr`, `measure-shoulder-dist-incr`, and both head scales, so the
prior spreads a head change evenly; prior 1σ = 0.35 about 0: the scanned shape space explains the body
before the authored blend shapes adjust it), with 1σ = 3 mm per girth and 2 mm per length, except 5 mm
for shoulder length, across back and total crotch length and 8 mm for the outseam (`fit_sigma`: readings
that rest on where a measurer puts the tape, whose catalogue tape σ is 10–15 mm); the phenotype is
clamped to [0, 1], local changes to [−1, 1]; age and gender are set, not fitted. Residuals are returned,
never hidden. Gate G1b (`tests/fit_tape.rs`): all 21 measurements drawn from the model itself (1.5, 2.5,
4 and 30 years, local changes up to ±0.3) are recovered within 1.5 mm (the gate asks 3 mm girths, 2 mm
lengths); the 92 cm reference toddler's 21 chart values in 9 iterations, the gate's within 2.6 mm and the
four gentler ones within 2σ (shoulder length 9 mm short and outseam 9 mm short: the chart's shoulder is
wider than Anny's toddler and its outseam 10 mm over its waist height; crotch length 7 mm long), where
the stature-only body read 75 mm less at the chest, 66 mm at the waist and 62 mm at the hip. With weak priors (1σ = 1)
the same fit had driven `weight` to 0 and compensated with large local changes. A known body weight
(`Wearer::weight_kg`) adds a row on Anny's mass (980 kg/m³ × volume) with 1σ = 1 % of it, at least
0.1 kg: with the stature alone it is the plan's "fit from little" (`tests/fit_tape.rs`: 2.5-, 6- and
35-year-olds reached within 2 mm and 1 %; a 15.5 kg toddler of 920 mm gets a 616 mm waist where a 12 kg
one gets 491 mm), with the tape it is one more measurement (synthetic masses within 0.3 %).
`tape_targets` and `Wearer::from_measurements` read a `fabric-measurements/1` file (fused values
whatever their provenance: the drape tests a pattern against the measurements it was drafted to; the
weight from `wearer.weight_kg`, else `wearer.bmi` × stature²).

**Drape poses** (definition in the `drape.rs` module notes). A joint turns its bone about its head by φ
about an axis in the rest pose's world frame; its local-bone delta is `φ·R_restᵀ·a`. Arms
(`upperarm01.L/R`): abduction θ from vertical in the plane of the rest arm and the vertical (rest ≈ 43°;
A-pose 35°, also 50° and arms up 150°). Legs about +X: seated = hips −90°, knees +90°; squat = hips −100°,
knees +110°, feet −10° (trunk upright). Straight arms (`Pose::drape`, A-pose 35° with them): Anny's rest
elbow is bent about 46° with the forearm forward, so `upperarm02` turns onto `upperarm01`'s line and
`lowerarm01` onto `upperarm02`'s, and a straight sleeve can contain the arm (a bent-elbow start crossed the
forearm). A ramp scales every φ by t. Nappy of size s:
`buttocks-volume-incr` += s, `hip-scale-depth-incr` += s/2 (s = 1 adds about 113 mm to a 2.5-year-old's
torso seat girth); `calibrate_nappy` bisects `s` (`0..=2.0`) so `seat_girth` on the posed body matches a
taped `seat_over_nappy` target within 0.01 mm, refusing targets outside the reachable range. The skin is
the closed shell enclosing the most volume (the mesh also holds two eyes
and a tongue), refused unless every edge has exactly two oppositely wound triangles, no triangle is
degenerate and the volume is positive. `trimmed_skin` leaves out the regions skinned mostly to given
bones and their descendants (the drape uses the wrists and feet: no garment in scope covers them, and a
12 mm garment mesh cannot clear a thumb) and closes each opening with a fan, checked the same way.
`examples/posed_skins.rs` writes the poses as OBJ for a look.

**Frames and landmarks** (`BodyFrames`, definitions in the `landmarks.rs` table). On the posed body, in
mm: floor, waist (Anny's waist loop), crotch, chest (the armpit), across back, hip, knee, calf, ankle,
shoulder tip, side neck point, C7 and head top, with the reference's formulas where it derives one from
others (hip = crotch + 0.45 × (waist − crotch), calf = ankle + 0.6 × (knee − ankle), across back midway
between chest and shoulder); torso and independent right/left arm and leg frames in the reference's
convention (spec 04 §3.2); section radii from the vertices of each region (by dominant skinning bone).
Explicit left panels use the actual left upper-arm bone, shoulder/wrist distance and upper-arm
radius. Left leg placement uses its own hip-to-ankle axis, ankle/calf/knee levels and left-leg/pelvis
vertices; shared torso levels retain their original definitions. Generated mirrored pattern
instances keep the reference's right-then-reflect convention. Independent synthetic asymmetric
joints and regions are exercised through actual `BodyFrames::new`, placement, wrap and roll in
the drape scene tests; these establish side selection and geometric arithmetic, not anatomical
or empirical fit accuracy. On the 92 cm
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

**Accepted shared shapes.** `accepted_shape::prepare` is the common file-free
native/browser importer. It checks an ACCEPTED `ftx-body-fit/1` report against the
caller-supplied digest of the same array-verified pinned model, reconstructs the
named parameters, checks declared wearer age/sex, and recomputes MODEL_FIT tape
predictions at 35 degrees. It retains source residuals, exact report/request
identities and signed comparisons without refitting or inventing uncertainty.
Measurements must already be validated and fused by the caller. Missing anatomy
remains unavailable. Report consistency is not authenticity or real-person fit.

`partial_tape_readings` retains each independently available positive finite reading in
`TAPE_FIELDS` order. A missing neck, shoulder or limb section cannot erase valid torso
circumferences, heights or stature. A failed dependency still withdraws every reading that
needs it; no inferred value replaces the missing section. Accepted-shape diagnostics identify
partial coverage and its exact unavailable fields, and material-study flat validation consumes
the same surviving MODEL_FIT values and signed residuals. The complete-only `tape_readings`
API and tape optimizer retain their requirement for all 21 readings. The square-frustum
known-answer check and accepted-prism handoff regression cover this bounded propagation;
they do not establish new anatomical validity, uncertainty or measurement readiness.

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
bone or local change), `non_finite` (a parameter, delta, vertex or derived skin area/volume), `out_of_range` (wrong vector lengths,
an abduction outside 0–180°, a ramp outside 0–1, a negative nappy), `degenerate_geometry` (an open,
non-manifold or inconsistently wound skin, a degenerate triangle), `inward_wound` (negative volume).
Finite vertices can still overflow geometry arithmetic; skin admission refuses non-finite derived
metrics rather than publishing an unusable volume or area. This adds no coordinate clamp.

**No claims.** Fit quality against real people (gate G1 on held-out measurements; the fit reaches the
targets, which says nothing about whether the shape between them is the wearer's); the WHO-calibrated
age prior of plan §9.4 (the priors here are the population middle); the shape distribution's
weight and muscle priors (spec 05 §9.11); anything about the SMPL family or the `rig="anny"` preset.
Poses are plain linear blend skinning without correctives: seated and squat bodies show webbing at the
crotch and lose 4–6 % of their volume (2.5-year-old: −4.4 % seated, −5.4 % squat), and the skin is not
checked for self-intersection there. The nappy is a shape proxy, not a garment. Landmarks are heuristics
on the body (see above), not measurements.
