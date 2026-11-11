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
Bucket and face-membership storage are bounded linearly by the mesh face count. Unsuitable slab widths
or excessive face replication use the original-order full mesh cut; tape definitions and values do not change.
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
yields no reading. A missing across-back section does not erase an independently available shoulder
path; both still require the declared side-neck evidence and shoulder height. Analytic flat-shoulder
controls check this geometric rule under translation and
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
before the authored blend shapes adjust it). Each measurement residual and its Jacobian row use the
larger of its supplied `sigma_mm` and the existing functional fitting floor: 3 mm per girth and 2 mm
per length, except 5 mm for shoulder length, across back and total crotch length and 8 mm for the
outseam (`fit_sigma`: readings that rest on where a measurer puts the tape, whose catalogue tape σ is
10–15 mm). Missing sigma retains that floor exactly; invalid supplied sigmas refuse. This explicit
residual-scaling policy does not calibrate a model-error distribution or certify posterior uncertainty.
The phenotype is bounded to [0, 1], local changes to [−1, 1]; age and gender are set, not fitted.
Each damped step solves within those bounds, redistributing motion from saturated controls and
releasing a bound when the remaining gradient points inward. Interior steps retain the original
least-squares arithmetic. The full tape fit may start at the nearest height-only endpoint when
proportions or local lengths are needed to reach the requested stature; the standalone
`fit_stature` retains its strict one-parameter range refusal. Requested measurements are never
clipped, and unreachable targets retain their residuals. Independent analytic length and bounded
least-squares controls cover these numerical behaviors without claiming anatomical accuracy.
Previously recorded G1b results (`tests/fit_tape.rs`, before the bounded-step correction):
all 21 measurements drawn from the model itself (1.5, 2.5,
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
weight from `wearer.weight_kg`, else `wearer.bmi` × stature²). `tape_targets` retains each selected
fused value's optional `sigma_mm`. The independent `wearer.stature_mm` override never borrows a fused
stature's uncertainty, even when their values coincide; without the override, fused stature retains
its own sigma. A missing sigma remains supported for legacy fused-only inputs, while a supplied null,
non-number, zero, negative or nonfinite sigma is refused. Numerical residuals retain the original
reported sigma and the effective scale separately from signed millimetre discrepancies. The weighted
LM objective has an analytic quadratic control: changing which of two incompatible section
targets has 3 versus 30 mm uncertainty changes the fitted section toward the more precise target,
while absent and below-floor sigmas retain exactly the old fit. This is numerical fitting evidence,
not anatomical or empirical fit qualification.

**Drape poses** (definition in the `drape.rs` module notes). A joint turns its bone about its head by φ
about an axis in the rest pose's world frame; its local-bone delta is `φ·R_restᵀ·a`. Arms
(`upperarm01.L/R`): abduction θ from vertical in the plane of the rest arm and the vertical (rest ≈ 43°;
A-pose 35°, also 50° and arms up 150°). Legs about +X: seated = hips −90°, knees +90°; squat = hips −100°,
knees +110°, feet −10° (trunk upright). Straight arms (`Pose::drape`, A-pose 35° with them): Anny's rest
elbow is bent about 46° with the forearm forward, so `upperarm02` turns onto `upperarm01`'s line and
`lowerarm01` onto `upperarm02`'s, and a straight sleeve can contain the arm (a bent-elbow start crossed the
forearm). A ramp scales every φ by t.

The independent `Pose.leg_fraction` (finite, inclusive 0..1; default 1) scales only the selected
seated/squat hip, knee and ankle rotations. Arms retain their specified abduction and straightening,
so small explicit flexion endpoints do not inadvertently move the arms back toward rest. Zero is
the standing leg geometry; one is the existing complete posture. This is a kinematic control,
not a measured movement, anatomical correction or guarantee that a cloth trajectory is admissible.

Nappy of size s:
`buttocks-volume-incr` += s, `hip-scale-depth-incr` += s/2 (s = 1 adds about 113 mm to a 2.5-year-old's
torso seat girth); `calibrate_nappy` bisects `s` (`0..=2.0`) so `seat_girth` on the posed body matches a
taped `seat_over_nappy` target within 0.01 mm, refusing targets outside the reachable range. The skin is
the closed shell enclosing the most volume (the mesh also holds two eyes
and a tongue), refused unless every edge has exactly two oppositely wound triangles, no triangle is
degenerate and the volume is positive. `trimmed_skin` leaves out the regions skinned mostly to given
bones and their descendants and closes each opening with a fan, checked the same way.

**Declared nappy occupancy.** `nappy_occupancy` activates only for explicit `wearer.nappies: true`
and requires a positive fused `seat_over_nappy` target; absent, malformed or unreachable targets
refuse without a default thickness or bare-body fallback. The existing size calibration runs once
in the straight-arm 35-degree measurement pose, and the derived parameters are reused for all
requested poses and motion. The bare fitted/accepted parameters, source report, mass and tape
residuals remain unchanged. The separate `nappy_occupancy` description retains the complete fused
target evidence, any valid supplied sigma, achieved seat, size/local additions and occupied reference
geometry digest. Its `seat_over_nappy` is a MODEL_FIT prediction, including when the target is a
chart prior; calibration never creates measurement readiness. This is a geometric occupancy proxy,
not a characterized nappy garment, added body mass or an inferred thermal/friction/material law.

Production `clothing_skin` selects wrists, feet and, where present, paired `levator05` facial-detail
regions. The pinned detailed face has intersections even with correct upstream coordinates. This
clothing proxy preserves retained source vertices exactly and identifies synthetic cap centres;
consumers still require the strict full-surface intersection audit. `SkinMesh::audit_self_intersections`
evaluates exact `ftx_core::intersection::indexed_triangle_pair` crossings/contacts over a deterministic
median-split AABB BVH in lexicographic `(a, b)` face order within an explicit candidate-pair budget.
`SkinMesh::pose_geometry_against_rest` and `body_pose_geometry_json` report `rest_volume_m3`,
`posed_volume_m3`, `volume_change_fraction`, `lbs_volume_loss_advisory` (`|ΔV / V_rest| > 3 %`),
`posed_area_m2` and `SkinIntersectionAudit` for both the full `skin_mesh` and `clothing_skin` when limb
cap roots are present. `body_contact_proxy` (`SkinMesh::contact_proxy_basis_with_rest`) in drape,
qualification, browser fold and fitted-study reports records selected roots, synthetic cap count and
rest-to-posed volume change. Full measurement/display skin is retained. No hand,
foot, facial-contact, anatomical or real-fit qualification follows from these caps. The positive
and detailed-source negative are in `ftx-drape/tests/kat_fitted.rs`.
`examples/posed_skins.rs` writes the poses as OBJ for a look. On the admitted Anny 0.6.1 mesh
(`tests/independent_tape_qualification.rs`), `clothing_skin` is strictly intersection-free (`0` crossing
pairs, `|ΔV/V_rest| ≤ 1.59 %`) across `rest`, `drape` (`35°`), `standing` (`50°`), `arms_up` (`150°`),
`seated` up to `leg_fraction = 0.50`, and `squat` up to `0.50` (`2.5 y` toddler) or `0.25` (`30 y`
adult). Deeper LBS flexion (`adult` `squat 0.50`: `12` pairs, `−2.03 %`; `toddler`/`adult` `seated` and
`squat` `0.75–1.00`: `82–286` hip/knee crease pairs and `−2.64 %` to `−7.03 %` enclosed-volume loss)
triggers `lbs_volume_loss_advisory` beyond `3 %` and refuses direct contact-collider start admission
(`IntersectingStart`).

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
the drape scene tests and `tests/independent_tape_qualification.rs` (zero-shared-code
`IndependentBodyFrames` + Dan Sunday winding + Jarvis-march 2D hull + polyline path oracle: all 21
analytic closed-form functionals within `1e-9 mm`, `BodyFrames` within `1.14e-13 mm`, and end-to-end
6-cohort × 21-functional readings within `2.27e-13 mm` on admitted Anny 0.6.1); these establish side
selection and geometric/numerical fidelity, not real-person anatomical or empirical fit accuracy. On
the 92 cm reference toddler fitted to stature only: waist 525 mm (taped 530), crotch 394 (372.5), knee
248 (231), chest 617 (the reference's formula on the tape gives 635); fitted to the tape, waist and
crotch land on the taped heights. Across `0.5–35 y` on the `35°` measurement pose, the `shoulder`
heuristic (`0.8 × r_arm` along `arm.up`) sits `+14.8` to `+42.7 mm` above `upperarm01.R`, while the
`chest` armpit heuristic (lowest `upperarm01/02` vertex within `|shoulder.x| + r_arm`) sits `66.2` to
`194.4 mm` below `upperarm01.R` (`85.0 mm` at `2.5 y`) and rises `+27.0 mm` when abducted from `35°`
to `50°` (`±5 mm` landmark shifts move `2.5 y` toddler `chest` `−4.09/+3.85 mm`, `waist` `+5.82/−5.31 mm`,
`knee` `−1.86/+1.39 mm`). Raising arms to `150°` inverts `shoulder` below `chest`, and `seated`/`squat`
drop torso levels `235–290 mm`: direct flat-to-3D `BodyFrames` wrap is therefore bounded to standing
A-poses (`35°`/`50°`), whereas `150°`/`seated`/`squat` must be reached by pose ramp or motion
continuation, and `tape_readings`/`fit_tape` is pinned to `Pose::drape()` (`35°`). Held-out `fit_tape`
cross-validation (`tests/independent_tape_qualification.rs`) reports synthetic 21-target recovery
separately from unmeasured `MODEL_FIT` extrapolation: fitting girths+stature holds out 9 lengths at
`4.23–6.87 mm` RMSE (`max 17.19 mm`); fitting lengths+stature holds out 11 girths at `7.35–10.20 mm`
RMSE (`3.98–5.21 mm` with `weight_kg` when global `weight` dominates, `8.85 mm` under localized
bust/hip/arm blendshapes); and fitting core trunk (`stature`/`chest`/`waist`/`hip`) against the external
92 cm public-prior chart leaves `24.24 mm` held-out RMSE (`max 69.90 mm`), confirming why `MODEL_FIT`
never satisfies measurement readiness.

**Data.** `tools/body-model/convert_anny.py` builds the model from the pinned package into a fresh cache,
checks the installed files against the wheel's RECORD and every array against spec 05 §8.1, and writes a
`.ftxbody` file (~152 MB, outside the repository; `FTX_ANNY_MODEL` or
`~/.cache/franken_textiles/anny-0.6.1-default.ftxbody`). The reader refuses a wrong magic, version,
dtype, shape, index range, bone order or (with `verify`) SHA-256. Blend shapes are CSR-sparse f64,
reconstructing the package's dense array exactly.
The loaded model caches which CSR rows contain exactly the identity vertex sequence. Those rows
traverse sums and deltas directly, preserving the same shape order and multiply/add operations;
all sparse, reordered and repeated-index rows retain the indexed path. The file format is unchanged.

**Accepted shared shapes.** `accepted_shape::prepare` is the common file-free
native/browser importer. It checks an ACCEPTED `ftx-body-fit/1` report against the
caller-supplied digest of the same array-verified pinned model, reconstructs the
named parameters, checks declared wearer age/sex, and recomputes MODEL_FIT tape
predictions at 35 degrees. It retains source residuals, exact report/request
identities and signed comparisons with the supplied target sigma without refitting or inventing
uncertainty. A reused accepted shape has no new fitting weights or normalized fitting residuals.
Measurements must already be validated and fused by the caller. Missing anatomy
remains unavailable. Report consistency is not authenticity or real-person fit.

**Calibrated image correspondences.** `correspondence::CorrespondenceModel` is the shared
Anny evaluator behind native and portable multi-frame fitting. It checks the existing authored
configuration against the exact array-verified model, resolves explicit vertex/surface/bone targets,
and evaluates shared shape, independent root/joint poses and requested rest-body tape functionals.
Surface points interpolate the individually skinned triangle vertices; skeleton endpoints use the
same posed rig. The sparse target path and complete-mesh export checks retain identical arithmetic.
`posed_with_points` evaluates one full requested pose and derives all ordered correspondences
from those same vertices and rig transforms. Initialization inspection shares the cached rest
shape but does not evaluate independent tape functionals; unavailable tape anatomy cannot hide
otherwise usable initial geometry. The fitter's measurement, sparse-point and ordinary posed
paths keep their existing requested-tape admission.
`target_preview` supplies the native and portable anatomical pickers with the same unfitted
rest geometry at the declared free-shape bound midpoints. It retains original mesh topology,
named rest-bone endpoints, fixed shape and explicit preview metadata. Nonempty finite geometry
is required, bounded to 100,000 vertices, 200,000 triangles and 1,024 bones. This inspection
does not evaluate tape functionals or change the authored configuration; target application
still uses ordinary correspondence validation.
The module has no camera, fitting solver, filesystem or measurement-crate dependency. Native and
portable adapters implement the existing `ftx-measure::body_fit::BodyModel` trait; that engine retains
all observability, multistart, held-out and work-budget gates. Configurations do not establish
anatomical correctness, calibrated scale, uncertainty, measured dimensions or physical fit.

`partial_tape_readings` retains each independently available positive finite reading in
`TAPE_FIELDS` order. A missing neck, shoulder or limb section cannot erase valid torso
circumferences, heights or stature. A failed dependency still withdraws every reading that
needs it; no inferred value replaces the missing section. Accepted-shape diagnostics identify
partial coverage and its exact unavailable fields, and material-study flat validation consumes
the same surviving MODEL_FIT values and signed residuals. The complete-only `tape_readings`
API still requires all 21 readings. The tape optimizer requires only the supplied targets,
the always-fitted stature and known weight. A missing requested reading refuses the start;
finite-difference probes and trial steps that lose a requested reading are inadmissible.
Unrequested sections never block a fit or become invented values. `TapeFit.readings` retains
each independently available prediction as an option; native, portable and study reports
identify partial coverage and the exact unavailable fields. Native MODEL_FIT records contain
only available predictions. Square-frustum, analytic-optimizer and articulated-prism workflow
regressions cover these boundaries; they do not establish new anatomical validity, uncertainty
or measurement readiness.

Stature/weight-only solves retain model, pose and frame admission on every evaluation, while
deferring unused posed tape functionals. All final independently available tape diagnostics are
evaluated once at the fitted parameters; other tape-target solves retain their complete reading path.

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
crotch and lose 4–6 % of their volume (2.5-year-old: −4.4 % seated, −5.4 % squat); `pose_geometry`
and `body_contact_proxy` report that volume change and exact surface self-intersection counts, and
drape consumers still refuse self-intersecting posed colliders (`IntersectingStart`), without claiming
soft-tissue volume conservation or anatomical crease fidelity. Independent plane-mesh slicing,
signed-angle winding and Jarvis-march convex-hull cross-checks in `tests/independent_tape_qualification.rs`
verify all 21 `TAPE_FIELDS` functionals geometrically without establishing real-person anatomical accuracy.
The nappy is a shape proxy, not a garment. Landmarks are heuristics
on the body (see above), not measurements.
