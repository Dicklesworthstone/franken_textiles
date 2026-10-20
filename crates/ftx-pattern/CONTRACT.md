# ftx-pattern contract

**Purpose.** The `fabric-pattern/1` document and what every later stage reads from it: instances (mirror
pairs, on-fold unfolding), the structure checks with the reference's messages, seam sides, notch fractions
and notch-piecewise seam sampling (`docs/spec/01` §2, §6–8). Validation (FTX-04, `validate.rs`) and the
alteration ops (FTX-05, `alter.rs`) and the drafting programs (FTX-07, `draft/`) build on it.

**Units and frames.** Millimetres, 2D, x right, y up, loops counter-clockwise; every piece is the
wearer's right side seen from its public side. `.L` instances are mirrored about x = 0.

**Invariants.**
- `Pattern::new` never panics: problems land in `build_errors`, and when there are any no instances are
  built (no partial geometry of an ambiguous pattern).
- Structure checks run on the raw JSON, so malformed input gets the reference's messages; the typed
  `PatternDoc` exists only for documents that type cleanly.
- Reading and writing a document is lossless up to key order and number spelling (semantic equality,
  spec 01 §14.6). `Pattern::from_json` keeps vertex order; a `serde_json::Value` input has sorted keys.
- Seams stay JSON objects read with Python semantics (`reverse: null` is false; an absent `ratio_band`
  differs from a `null` one).

**Oracle and fixtures** (`tests/fixtures/oracle/pattern/`, compared in `ftx-conformance`):
`instances` (F7–F10, curved pairs), `structure` (24 F11 cases, exact messages; check-1 messages as a
multiset because the reference iterates a set), `seams` (F12–F21, F26, F27: sides, notch fractions, seam
pairs at 5 and 7 mm, tangents, corner angles), `drafts` (bolero, trousers and fez for public reference
wearers of 80, 92 and 116 cm: every instance, side and seam pair, and the round trip). 96–99 % of the
numbers are bit-exact, the worst 5.7e-14 relative.

**Deviations from the reference** (each on input the reference crashes on or silently accepts; spec 01
§15): a piece without an id is `piece {i}: missing id`; `cut: null` reads as absent; `cut.qty` must be the
integer 1 or 2 (the reference truncates 2.7 to 2); a Bézier with the wrong number of control points is a
definition error; a seam ref without `/` has its own message; a zero-length seam side with notches reports
the implausible length instead of dividing by zero; an edge's `"curve": null` is written back as absent.

**Error model.** Structure checks return messages. Lookups return `PyError` (`Key` prints quoted, like
Python's `str(KeyError)`; `Value` prints as is).

**Determinism class.** Fixed-order f64 arithmetic over `ftx-core`; bit-identical across targets.

**Permanent membrane identification** (`material_membrane_plasticity::calibration`). Whole acquired
histories replay the existing raw-Green/second-Piola incremental return map from explicitly pristine
reference states. Four source-qualified elastic observations and six independently supplied strain/
accumulation domains stay fixed; only six yield/hardening values are identified inside declared bounds.
FIT alone controls bounded multistart optimization, equivalent-candidate checks and local sensitivity;
CHECK replays the fixed choice afterwards. Missing force channels remain absent while their strain
nodes advance memory. Numerical bound projection never substitutes for stationarity, and every forward
probe/refusal/replay consumes the shared work budget. Ambiguous, incomplete or rejected reports carry
no material. Accepted source-bearing fitted quantities remain estimates with no inferred confidence,
continuous-path, measured-textile, damage, fatigue or empirical garment claim. The independent synthetic
cycle fixture and `tests/membrane_plasticity_calibration.rs` exercise signed history, coupled elasticity,
unobserved peaks, held-out isolation and constrained identification.

**Validation** (`validate`, spec 01 §9): seam walk, truing, points of measure, report status, text table
and the role-based default fit spec. The body comes through the `BodyMeasure` trait (values, σ, landmark
heights); `Measurements` implements it for a measurement file (flat or fused) with the reference's
analytic landmark formulas, and an Anny-fitted body can implement it later. Fit specs stay JSON read with
the reference's dictionary semantics. Oracle: `tests/fixtures/oracle/validate/` (48 reports bit-exact and
48 tables identical, 9 default fit specs exact). Deviations, all on inputs the reference crashes on: a
`ratio_band` that is not `[lo, hi]` is an ERROR row; a `null` ratio without a band reads as 1.0; a
measurement entry without a value counts as missing; a point without `body` is UNMEASURABLE and not
listed as missing.

**Alteration** (`alter`, spec 01 §10): slash-spread, vertex movement, true-seam, cap-ease and
transfer-curve return a new raw JSON document, preserve unedited fields and append `meta.history` with
`origin = VECTOR_EDIT`. Coordinate changes use the reference's ties-to-even rounding. Grain/internal/
decor/trace marks follow slash-spread and cap-ease; drills, notches and placement do not. true-seam and
cap-ease use the reference's 60-step bisection (±60 mm; 0.3–3 × cap height),
a band's mean by numpy's pairwise sum, and its sentences (`:+.1f`, `:.3f`). Direct regressions use the
spec's published F33–F36 and F39–F40 values. Oracle: `tests/fixtures/oracle/alter/alterations.json`
(`tools/oracle/capture_alterations.py`): 21 cases (the drafted bolero and trousers at 80, 92 and 116 cm,
a mutated bolero, error paths); `crates/ftx-conformance/tests/alterations.rs` matches all 27,437 numbers
bit for bit, every history sentence and every structure error, and each reference error is a refusal
naming the same thing. Malformed input and non-finite results are typed refusals, not panics or JSON
nulls; invalid axes or ends, zero-length source chords and a side b on a mirrored (`.L`) copy are
refused, as are the unknown seam, missing cap seam and missing destination edge the reference crashes on.
These operations alone establish neither structural validity nor garment fit.

Plan safety deviations, covered by `tests/alter_safety.rs`: unbracketed targets return `NoRoot`, not a
silently saturated bound; non-finite residuals, zero tangents/armholes, non-positive ratios,
malformed/reversed bands and duplicate/unknown seam ids are refused. Cap refs must actually join the
requested piece to another piece and use a single cap instance; unfolded mirrored cap edges are
refused rather than applied in the wrong frame. All 21 captured oracle cases still match bit for bit,
unchanged.

**Drafting** (`draft`, spec 02 Parts B–C): `draft(program, measurements, style) → (pattern, fit spec)` for
`zouave_bolero` (with the soutache layout), `zouave_trousers` and `fez`, on a `fabric-measurements/1` document
or a flat name → value object. `draft::form` is the reference's analytic body (`body.py`: landmarks, the
girth-matched torso sections with the crotch ring solved to the total crotch length, part frames, section
radii, landmark heights; the stable default form), which the programs read for levels and placements. The
programs keep the reference's solvers step for step (60-step bisections for the neck scale, armhole scoop,
cap height, cap balance and back crotch extension; the damped 2×2 Newton that trues the trouser legs; 200-
and 400-sample lengths) and its rounding of stored geometry. The soutache border goes through
`draft::geos`, GEOS 3.14.1's own offset-curve construction ported line for line (the buffer-input
simplifier with its middle-vertex sampling, the segment generator's mitres, inside-turn crossings and
closing segments, the vertex snap), with the loops at inside corners cut where the curve crosses itself,
then clipped to the piece. Oracle: `tests/fixtures/oracle/draft/` (`tools/oracle/capture_drafting.py`):
the body for three reference wearers and a flat dict (804 numbers within 1e-9), 21 drafts (three programs
at 80, 92 and 116 cm, belly room, no soutache, no motifs, a roomy bolero, nappy, knee and ankle cuffs, a
tall fez, a four-arc fez, flat dicts with a custom size label): of about 15,000 numbers all but 40 are
bit-identical (those within 1.2e-13), every vertex, curve control and braid point included; shapely's
offset curves and clipped borders reproduced point for point (1.7e-12 mm). Refusals: an unknown style
key (`unknown_parameter`, the reference's message), a fez style out of range (`out_of_range`), a missing
measurement (`invalid_document` naming it, the reference's `KeyError`). No claims: the dress-form mesh
and the simulator's placement helpers (spec 02 B.6–B.7; the drape places pieces on Anny); the fez's
taper refusal, which the style ranges make unreachable (the widest allowed sector is 314°); a braid line
that comes back within twice its inset of itself (the curve would then need the buffer's other side).

**Bounded thermal fits** (`material_thermal`, `tests/thermal_polynomial.rs`). A thermal series
contains either reported temperature observations or an explicitly classified polynomial fit of
maximum degree three. Coefficients are in ascending Celsius powers and canonical output units;
fits preserve their source/method/domain and never acquire measured-knot status. Admission checks
positive values at both endpoints and all interior stationary points and refuses numerically
cancellation-dominated positivity. All fitted/interpolated laws require explicit estimate consent.
Local Simpson integration is algebraically exact through cubic degree and avoids subtracting large
absolute enthalpies near equilibrium. Existing tabulated arithmetic and serialization are unchanged.
The same law reaches dry thermal stepping and source-locked surface replay; evidence qualification
is independent from physical specimen validation, uncertainty propagation and garment safety.

**Purchasing instants** (`material_cost`). `ftx-textile-quotes/1` retains its calendar-only observation,
scope and conflict semantics. Explicit `/2` books require UTC source timestamps and an explicit
matching-date evaluation instant. Bounded Gregorian/clock parsing and integer nanoseconds establish
ordering without platform timezones or floating-point truncation; unsupported clocks refuse. Choose
the latest observation at or before evaluation before checking inclusive source expiry, availability,
exact material lock and complete minimum/increment-rounded stock. A failed latest observation never
revives an older price. Distinct records at the same instant conflict. Quote fields retain original
source identity and estimates are not made into purchases or reserved inventory. The same evaluator
feeds ordinary price commands, marker procurement and native/WASM studies; see
`docs/textile-price-refresh.md` and the `material_cost` unit tests.
