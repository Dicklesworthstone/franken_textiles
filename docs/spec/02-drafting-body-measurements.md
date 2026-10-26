# 02: Drafting programs, body model and measurements (essence spec)

Status: extracted spec for the Rust port. Implementers work from this document alone; they do not read the legacy code.

Legacy oracle: the Python reference implementation (fabric-pattern-creation-and-alteration skill), specifically
`scripts/drafting.py`, `scripts/body.py`, `scripts/measurements.py` and their tests `scripts/tests/test_core.py` and
`scripts/tests/test_measurements.py`. All 92 tests in those two files pass on the oracle as of this extraction
(Python 3.12, numpy 2, OpenCV 5.0, shapely 2). Every number in the conformance section was produced by running the
oracle, not copied from its documentation.

Method: this is an essence extraction. It states what each function computes (inputs, constants, formulas, outputs,
messages), not how the Python is laid out. Do not translate line by line. Where the oracle has a quirk that changes
output numbers, it is called out as **[quirk]**: keep it for conformance, or change it deliberately and update the
fixture.

Related subsystems that are specified elsewhere and only referenced here: the pattern model (pieces, instances,
curves, seams; `pattern_model.py`), flat validation (`validate.py`) and alteration (see
`01-pattern-model-validation-alteration.md`); the fold-up simulator (`blender_sew.py`) and the command-line/round
driver (`fabric.py`) (see `04-simulation-vectorize-cli.md`); exports in their own spec.

---

## 0. Conventions shared by all three modules

### 0.1 Units and schemas

- Every length is millimetres (`value_mm`, `sigma_mm`). The one angle, `shoulder_slope_deg`, uses `value_deg` /
  `sigma_deg`. A name is an angle when the catalogue says `unit = "deg"`, or, for names not in the catalogue, when it
  ends in `_deg`.
- Schema strings: measurement file `fabric-measurements/1`; calibration-sheet sidecar `fabric-calib-sheet/1`; photo
  scale report `fabric-photo-scale/1`; readiness report `fabric-readiness/1`; fit spec `fabric-fit/1`; pattern
  `fabric-pattern/1`.
- Sigma (σ) is always 1σ.

### 0.2 Pattern-space conventions used by the drafting programs

- Every piece is drafted as the wearer's right half, seen from its public (outer) side, x to the viewer's right,
  y up, edge loops counter-clockwise (positive signed area).
- Torso and trouser pieces: y = 0 at the body waist line. Sleeves: y = 0 at the cap top (shoulder tip). Fez pieces:
  origin at the cone apex (crown) or the tip centre.
- Curved edges are stored relative to their chord: a control point `[t, h]` means `p0 + t·(p1 − p0) + h·perp(p1 − p0)`
  with `perp(d) = (−d.y, d.x)`.
- The piece builder (pattern model) has these behaviours that drafting outputs depend on:
  - vertex coordinates are rounded to 3 decimals when stored;
  - a cubic given by absolute control points is stored as relative `[t, h]` pairs rounded to 5 decimals;
  - notch positions `s` are rounded to 4 decimals;
  - `cut.qty = 2` yields instances `<id>.R` (as drafted) and `<id>.L` (mirrored, loop reversed so it stays CCW);
    `cut.on_fold = <edge id>` yields one unfolded instance `<id>` whose drafted-half edges are `<e>.R` and
    mirrored-half edges `<e>.L`;
  - **edge length** = length of the polyline through 200 evenly spaced Bézier parameter values (t = 0..1, endpoints
    included); straight edges are exact;
  - instance **outline(dense)** = concatenation of each edge sampled at `dense` parameter values, dropping each edge's
    last point (default dense 60; validation uses 120);
  - **horizontal_width(poly, y)** = total length of the horizontal line y inside the closed polygon (sort the edge
    crossings with `(ya > y) != (yb > y)`, sum alternate intervals).
- Seam side references are `instance/edge[+edge…]` in loop order. `reverse: true` pairs side a's start with side b's
  end. A seam's measured ratio is `length(a) / length(b)`; `ratio` is the nominal value, `ratio_band` an accepted band.

### 0.3 Numeric primitives

- **bisect(f, lo, hi, it = 60)** (drafting): evaluate `flo = f(lo)`; repeat 60 times: `mid = (lo + hi)/2`; if
  `(f(mid) > 0) == (flo > 0)` then `lo = mid, flo = f(mid)`, else `hi = mid`. Return `(lo + hi)/2`. No sign-change
  check: if the root is not bracketed it silently converges to an end. **[quirk]** zero counts as non-positive.
- **cubic length for solves** (drafting, sleeve cap only): polyline through 400 Bézier samples of the *unrounded*
  absolute control points. (Pattern edge lengths elsewhere use the stored, rounded, 200-sample form; the two differ in
  the 4th decimal, which is why validation reads cap ratios of 1.045 to 3 decimals, not exactly.)
- **Ramanujan II ellipse perimeter** for semi-axes a, b: `h = ((a − b)/(a + b))²`,
  `P = π(a + b)(1 + 3h / (10 + √(4 − 3h)))`. `measurements` raises if a ≤ 0 or b ≤ 0; `body` does not check.
- **Linear interpolation with linear extrapolation** (`interp_x`, measurements priors): below the first knot use the
  first segment's slope, above the last knot the last segment's slope, inside piecewise linear. Contrast with
  **clamped interpolation** (numpy-style, flat beyond the ends), used where stated.

---

## Part A: Measurements (`measurements.py`)

### A.1 Constants

| constant | value | meaning |
|---|---|---|
| PROVENANCES | `MEASURED_TAPE`, `PHOTO_ESTIMATED`, `CHART_PRIOR`, `MODEL_FIT`, `SCAN_MEASURED`, `GARMENT_DERIVED` | allowed `provenance` values, in this order |
| DIRECT_PROVENANCES | `MEASURED_TAPE`, `SCAN_MEASURED`, `GARMENT_DERIVED` | sources that may earn `OK_TAPE_LIMITED` |
| DEFAULT_SIGMA_BY_PROVENANCE | `GARMENT_DERIVED` 18, `MODEL_FIT` 12, `SCAN_MEASURED` 15 (mm) | `add` default σ when not tape |
| Z_FUSE_WARN | 2.0 | z above this: fuse but flag `Z_WARN` |
| Z_REFUSE | 3.0 | z above this: do not fuse, flag `REMEASURE` |
| Z_P3_P97 | 1.881 | prior P3–P97 half-width in σ |
| Z_P1_P99 | 2.326 | prior P1–P99 half-width in σ |
| STALE_DAYS | 42 | readiness advisory when a set is older |
| GIRTH_ELASTICITY | 0.5 | girths scale as (stature ratio)^0.5 |

### A.2 Measurement catalogue

Each definition (`MeasureDef`) has: name, unit (`mm`|`deg`), kind (`girth|length|height|breadth|depth|angle`),
definition_id, iso_clause (ISO 8559-1 clause or null), definition text, how text (one-line instruction, quoted
verbatim in re-measure requests), tape σ (default 1σ for a parent with a tape; null = not a tape item), target σ
(target 1σ of the fused value; null = none), view (`P1` front, `P2` side, `P3` back, `P4` seated, or null), notes,
equivalent ids (other definitions accepted unchanged), and normalization hooks (definition id → offset, extra σ,
reason). The catalogue order below is significant: it orders fused output, priors and the `catalogue` CLI output.

| # | name | unit | kind | definition_id | ISO 8559-1 clause | tape σ | target σ | view |
|---|---|---|---|---|---|---|---|---|
| 1 | `stature` | mm | height | `ISO8559-1:5.1.1` | 5.1.1 | 10 | 10 | P1 |
| 2 | `chest` | mm | girth | `EN13402:chest` | — | 10 | 10 | — |
| 3 | `waist` | mm | girth | `SKILL:waist-ribbon` | 5.3.10 (level differs) | 18 | 15 | — |
| 4 | `hip` | mm | girth | `ISO8559-1:5.3.13` | 5.3.13 | 10 | 10 | — |
| 5 | `seat_over_nappy` | mm | girth | `SKILL:seat-over-nappy` | — | 12 | 10 | — |
| 6 | `neck_base` | mm | girth | `ISO8559-1:5.3.3` | 5.3.3 | 10 | 5 | — |
| 7 | `across_back` | mm | length | `ISO8559-1:5.4.2` | 5.4.2 | 15 | 10 | P3 |
| 8 | `shoulder_length` | mm | length | `ISO8559-1:5.4.1` | 5.4.1 | 10 | 5 | P3 |
| 9 | `shoulder_slope_deg` | deg | angle | `SKILL:shoulder-slope-P3` | — | — | 3 | P3 |
| 10 | `back_neck_to_waist` | mm | length | `ISO8559-1:5.4.5` | 5.4.5 | 10 | 10 | P3 |
| 11 | `arm_length` | mm | length | `ISO8559-1:5.7.8` | 5.7.8 | 15 | 10 | P1 |
| 12 | `upper_arm` | mm | girth | `ISO8559-1:5.3.16` | 5.3.16 | 5 | 5 | — |
| 13 | `wrist` | mm | girth | `ISO8559-1:5.3.19` | 5.3.19 | 5 | 5 | — |
| 14 | `hand_pass` | mm | girth | `SKILL:hand-pass` | — | 5 | 5 | — |
| 15 | `body_rise` | mm | length | `SKILL:body-rise-seated` | — | 18 | 10 | P4 |
| 16 | `total_crotch_length` | mm | length | `ISO8559-1:5.4.18` | 5.4.18 | 15 | 10 | — |
| 17 | `inseam` | mm | length | `OTTOBRE:inside-leg` | 5.1.15 | 20 | 20 | P1 |
| 18 | `outseam` | mm | length | `ISO8559-1:5.4.22` | 5.4.22 | 15 | 20 | P1 |
| 19 | `thigh` | mm | girth | `ISO8559-1:5.3.20` | 5.3.20 | 10 | 10 | — |
| 20 | `knee` | mm | girth | `ISO8559-1:5.3.22` | 5.3.22 | 8 | 15 | — |
| 21 | `calf` | mm | girth | `ISO8559-1:5.3.24` | 5.3.24 | 7 | 15 | — |
| 22 | `ankle` | mm | girth | `CPSC77:83-ankle` | — | 5 | 15 | — |
| 23 | `foot_pass` | mm | girth | `SKILL:heel-instep` | — | 7 | 10 | — |
| 24 | `head` | mm | girth | `ISO8559-1:5.3.1` | 5.3.1 | 5 | 5 | — |
| 25 | `belly_depth` | mm | depth | `SKILL:belly-depth-P2` | — | — | 10 | P2 |
| 26 | `chest_depth` | mm | depth | `SKILL:chest-depth-P2` | — | — | — | P2 |
| 27 | `waist_depth` | mm | depth | `SKILL:waist-depth-P2` | — | — | — | P2 |
| 28 | `hip_depth` | mm | depth | `SKILL:hip-depth-P2` | — | — | — | P2 |
| 29 | `chest_width` | mm | breadth | `SKILL:chest-width-P1` | — | — | — | P1 |
| 30 | `waist_width` | mm | breadth | `SKILL:waist-width-P1` | — | — | — | P1 |
| 31 | `hip_width` | mm | breadth | `SKILL:hip-width-P1` | — | — | — | P1 |
| 32 | `armscye_depth` | mm | length | `SKILL:scye-depth` | — | 15 | — | — |
| 33 | `waist_height` | mm | height | `ISO8559-1:5.1.10` | 5.1.10 | 15 | 20 | P1 |
| 34 | `crotch_height` | mm | height | `ISO8559-1:5.1.15` | 5.1.15 | 20 | 20 | P1 |
| 35 | `knee_height` | mm | height | `SKILL:knee-height` | — | 10 | — | P1 |

Notes on the table: `waist`'s ISO clause string is literally `5.3.10 (level differs)`; `inseam`'s definition_id is
`OTTOBRE:inside-leg` with iso_clause `5.1.15`.

**How texts (verbatim):**

| name | how (verbatim; embedded in re-measure requests) |
|---|---|
| `stature` | Barefoot against a wall, book flat on the crown, mark the wall, measure mark to floor. |
| `chest` | Tape level under the arms and over the shoulder blades; read on a normal breath out. |
| `waist` | Tie the ribbon first, let the child wiggle, then tape at the ribbon without pulling the tummy in. |
| `hip` | Tape level round the fullest part of the seat; check it is level from the side. |
| `seat_over_nappy` | Same as seat, over the usual nappy. |
| `neck_base` | Tape round the base of the neck where a T-shirt neckband sits: over the C7 bump and the breastbone notch. |
| `across_back` | Find the bony shoulder tips (mark them); tape across the back from tip to tip, arms hanging. |
| `shoulder_length` | From where the neck meets the shoulder to the bony shoulder tip. |
| `shoulder_slope_deg` | Back photo P3, arms in A-pose; the agent clicks side-neck point and shoulder tip on both sides. |
| `back_neck_to_waist` | Head bent forward to find C7; tape straight down the spine to the ribbon's lower edge. |
| `arm_length` | Elbow slightly bent, hand on tummy; shoulder tip over the elbow to the wrist bone. |
| `upper_arm` | Arm hanging relaxed; tape round the thickest part halfway shoulder–elbow. |
| `wrist` | Tape round the wrist over the knobbly bone. |
| `hand_pass` | Thumb tucked to the palm; tape round the widest part of the hand (knuckles and thumb base). |
| `body_rise` | Child sits upright on a hard chair; ruler/tape vertical at the side from ribbon to seat. |
| `total_crotch_length` | Tape from the front ribbon, down and between the legs, up to the back ribbon; snug, child standing. |
| `inseam` | Legs slightly apart; from the crotch straight down the inside leg to the floor. |
| `outseam` | From the ribbon's lower edge at the side, over the hip, to the floor. |
| `thigh` | Tape level round the top of one leg, just under the seat crease. |
| `knee` | Leg straight; tape round the middle of the kneecap. |
| `calf` | Tape level round the fullest part of the calf. |
| `ankle` | Tape round the narrowest part just above the ankle bones. |
| `foot_pass` | Foot relaxed, toes pointed slightly; tape round the heel and over the top of the instep. |
| `head` | Tape above the eyebrows and ears, over the bump at the back; measure at the intended band line. |
| `belly_depth` | Side photo P2 (profile, arms hanging); the agent clicks back surface and belly front on one horizontal line. |
| `chest_depth` | Side photo P2; back to front at chest level. |
| `waist_depth` | Side photo P2; back to front at the ribbon. |
| `hip_depth` | Side photo P2; seat to front at hip level. |
| `chest_width` | Front photo P1; torso edge to torso edge just under the arms. |
| `waist_width` | Front photo P1; edge to edge along the ribbon. |
| `hip_width` | Front photo P1; edge to edge at the fullest seat level. |
| `armscye_depth` | Rarely measured: drafted from chest and stature (03 §1.2). |
| `waist_height` | Ruler vertical at the side, ribbon to floor. |
| `crotch_height` | Same as inseam but vertical; usually read from the front photo. |
| `knee_height` | Ruler vertical behind the knee to the floor. |

**Definitions and notes (verbatim; emitted by `catalogue`):**

| name | definition (verbatim) | notes (verbatim) |
|---|---|---|
| `stature` | Vertical distance from the crown (median line) to the floor, barefoot, heels and back against a wall. | σ 2.5 cm without the book (03 §2.3). Also the best whole-body photo scale bar (03 §3.1). |
| `chest` | Horizontal girth high under the arms and over the shoulder blades, normal breathing (EN 13402; = bust for children). | Breathing moves it about ±1 cm in small children (03 §2.3). |
| `waist` | Horizontal girth at the waist ribbon: narrow elastic tied on and settled by wiggling (Big-4/Ottobre natural waist). | Least reliable girth (belly, breath, meals; 03 §2.3). Target ±3 cm for an elastic/drawstring waist. |
| `hip` | Horizontal girth at the fullest part of the seat (greatest buttock projection seen from the side), over underwear. | Over underwear, not a nappy (see seat_over_nappy). ±2 cm is fine for full trousers (03 §6). |
| `seat_over_nappy` | As hip, but taken over the nappy the garment will be worn over. | 06 Recommended defaults item 11. No free prior: the chart value is a placeholder (UNCERTAIN). |
| `neck_base` | Girth over C7, the front neck point (top of the breastbone notch) and both side neck points: the bodice neckline seam. | Ticklish; tape rides up to mid-neck (smaller). |
| `across_back` | Shoulder point (acromion tip) to shoulder point across the back, along the surface. | Acromion is hard to find on a chubby, moving child (03 §2.3). Children need more back ease, not less. |
| `shoulder_length` | Side neck point to shoulder point along the top of the shoulder. | Only ~8 cm long, so 1 cm is >10% (03 §2.3, §6). |
| `shoulder_slope_deg` | Angle below horizontal of the line from side neck point to shoulder point, from the P3 back photo (sheet upright). | 06 Q3 addition; tolerance ±3° (06 E3.2). No free prior: 18° ±5° is a placeholder (UNCERTAIN). |
| `back_neck_to_waist` | From the C7 bump down the spine (along the surface) to the lower edge of the waist ribbon. | Ribbon level dominates the error (03 §2.3). |
| `arm_length` | Shoulder point over a slightly bent elbow to the wrist bone (ulnar styloid). | Elbow bend and shrugging (03 §2.3); growth goes into the hem. |
| `upper_arm` | Girth midway between shoulder point and elbow, perpendicular to the arm, arm relaxed. | Trained teams under-read it (WHO MGRS, 03 §2.1). |
| `wrist` | Girth at the wrist bone (ulnar styloid). | Matters only for a fitted cuff; a closed cuff must pass the hand (hand_pass). |
| `hand_pass` | Smallest loop that slides over the hand with the thumb tucked into the palm: minimum opening of a cuff with no placket. | No free prior in 03; chart value is a placeholder (UNCERTAIN). |
| `body_rise` | Seated on a hard flat chair: vertical distance from the side waist ribbon to the chair seat. | No exposed ISO clause (03 §1.1). Chair compression and slumping (03 §2.3). |
| `total_crotch_length` | From the centre-front waist ribbon down between the legs to the centre-back waist ribbon, along the body. | 06 Q3 addition: decides trouser rise fit. Tape σ is a guess (UNCERTAIN). Over the nappy if worn. |
| `inseam` | Crotch to floor, standing with legs slightly apart (toddler inside leg). | Least reliable length (03 §2.3). |
| `outseam` | From the lower edge of the waist ribbon down the side, over the hip, to the floor. | The hem absorbs it; grows ~4.5 cm/yr (03 §2.4). |
| `thigh` | Maximum horizontal girth at the top of the leg just below the buttock crease. | Fitted trousers only; a minimum for full trousers. |
| `knee` | Girth at mid-kneecap, leg straight. | Cuff level for knee-gathered trousers. No free prior in 03 (UNCERTAIN). |
| `calf` | Horizontal girth at the fullest part of the calf. | Cuff level for calf-gathered trousers. |
| `ankle` | Minimum girth just above the ankle bones. | Cuff level for ankle-gathered trousers. |
| `foot_pass` | Diagonal girth round the back of the heel and over the instep: the loop a closed cuff must pass over. | Pass-over inequality for gathered cuffs (06 Q4). No free prior in 03 (UNCERTAIN). |
| `head` | Maximum head girth above the brow ridges and over the back bump (occiput), 1–2 cm above the ears: the hat band line. | Lowest-error tape item (03 §2.3). A fez has no stretch: ±0.5 cm ≈ half a hat size (03 §6). |
| `belly_depth` | Side view P2: horizontal front-to-back body depth at the most prominent point of the belly. | 06 Q3 addition: drives front-hem lift. Prior derived from the section model (UNCERTAIN). |
| `chest_depth` | Side view P2: horizontal front-to-back depth at chest level (under the arms). | Input to girth-from-section. |
| `waist_depth` | Side view P2: horizontal front-to-back depth at the waist ribbon. | Input to girth-from-section. |
| `hip_depth` | Side view P2: horizontal depth at hip level (greatest buttock projection). | Input to girth-from-section. |
| `chest_width` | Front view P1: horizontal silhouette width of the torso at chest level, arms out (A-pose). | Input to girth-from-section. |
| `waist_width` | Front view P1: silhouette width at the waist ribbon. | Input to girth-from-section. |
| `hip_width` | Front view P1: silhouette width at hip level. | Input to girth-from-section. |
| `armscye_depth` | Vertical from the top of the shoulder at the shoulder point down to armpit (chest) level. | No free prior in 03; placeholder 0.13 × stature (UNCERTAIN). |
| `waist_height` | Vertical distance from the waist ribbon to the floor at the side. | — |
| `crotch_height` | Vertical distance from the crotch (highest point between the thighs) to the floor. | ≈ inseam (03 §1.1). |
| `knee_height` | Vertical distance from the back-of-knee crease to the floor. | No free prior in 03; placeholder 0.62 × crotch height (UNCERTAIN). |

**Equivalents and normalization hooks:**

| name | equivalent_ids (accepted unchanged) | normalization hooks: definition_id → (offset mm, extra σ mm, reason) |
|---|---|---|
| `chest` | `ISO8559-1:5.3.4`, `CPSC77:63-axilla`, `BIG4:chest`, `OTTOBRE:chest` | — |
| `waist` | `BIG4:natural-waist`, `OTTOBRE:waist`, `CPSC77:68-natural-waist` | `CPSC77:66-navel` → (-10, 5, "navel waist ≈ natural waist + 1 cm at 2–6 y (03 §0.2.1)")<br>`ISO8559-1:5.3.10` → (+0, 10, "ISO rib/iliac-midpoint level vs ribbon level: offset unknown (UNCERTAIN, 03 §1.1)") |
| `hip` | `CPSC77:71-buttocks`, `OTTOBRE:hip`, `BIG4:hip` | — |
| `neck_base` | — | `ISO8559-1:5.3.2` → (+15, 8, "mid-neck girth → neck base, +1.5 cm (UNCERTAIN)")<br>`CPSC77:32-mid-neck` → (+15, 8, "mid-neck girth → neck base, +1.5 cm (UNCERTAIN)") |
| `across_back` | — | `CPSC77:36-biacromial` → (+10, 8, "straight bony breadth → surface across back, +1 cm (UNCERTAIN)") |
| `shoulder_length` | `OTTOBRE:shoulder-length` | — |
| `back_neck_to_waist` | `BIG4:back-waist-length`, `OTTOBRE:back-waist-length` | — |
| `arm_length` | `OTTOBRE:arm-length` | — |
| `upper_arm` | `CPSC77:39-upper-arm` | — |
| `body_rise` | — | `SKILL:body-rise-standing` → (+15, 10, "standing waist height − crotch height runs 1–2 cm shorter than seated (03 §1.1, §4.4; UNCERTAIN)") |
| `inseam` | `ISO8559-1:5.1.15`, `CPSC75:73-crotch-height` | — |
| `outseam` | `OTTOBRE:outside-leg` | — |
| `thigh` | `CPSC77:77-upper-thigh` | — |
| `calf` | `CPSC77:81-calf` | — |
| `head` | `CPSC77:18-head`, `WHO:head-circumference`, `CDC:head-circumference` | — |
| `chest_width` | `CPSC77:64-chest-breadth` | — |
| `waist_width` | — | `CPSC77:67-navel-breadth` → (+0, 5, "navel-level breadth used for ribbon level (UNCERTAIN)") |
| `hip_width` | — | `CPSC77:72-bitrochanteric` → (+0, 10, "bony bitrochanteric breadth used as silhouette width (UNCERTAIN)") |
| `crotch_height` | `CPSC75:73-crotch-height`, `OTTOBRE:inside-leg` | — |

#### A.2.1 Name aliases

Canonicalisation: strip, lower-case, replace spaces and hyphens with underscores, then map through the alias table;
unknown names pass through unchanged.

| alias | canonical | alias | canonical |
|---|---|---|---|
| seat, hips, hip_girth | hip | crotch_length, total_rise | total_crotch_length |
| bust, chest_girth | chest | inside_leg | inseam |
| waist_girth | waist | outside_leg | outseam |
| neck_base_girth | neck_base | head_girth | head |
| nape_to_waist, back_waist_length | back_neck_to_waist | upper_arm_girth, biceps | upper_arm |
| crotch_depth, rise | body_rise | thigh_girth | thigh |
| shoulder_slope | shoulder_slope_deg | scye_depth | armscye_depth |
| height | stature | heel_instep | foot_pass |
| hand_girth | hand_pass | | |

#### A.2.2 Garment kinds

Kind lookup: strip, lower-case, `-` → `_`; match a kind key or any alias; otherwise error
`unknown garment kind '<kind>'; use one of bodice, set_in_sleeve, trousers, full_trousers_gathered, hat_fez`.

| kind | aliases | must | should | target overrides (mm) | basis (verbatim) |
|---|---|---|---|---|---|
| `bodice` | jacket, bolero, waistcoat | stature, chest, back_neck_to_waist, neck_base, across_back, shoulder_length | waist, shoulder_slope_deg, belly_depth | — | `03 §1.2 bodice row; should-list adds 06 Q3 (shoulder slope, belly depth for front-hem balance)` |
| `set_in_sleeve` | sleeve | arm_length, upper_arm | wrist | — | `03 §1.2 sleeve row; hand_pass becomes must with --closed-cuff` |
| `trousers` | straight_trousers, pants | waist, hip, body_rise, outseam, total_crotch_length | inseam, thigh, belly_depth | — | `03 §1.2 trousers row; total crotch length promoted to must (06 Q3, defaults item 11)` |
| `full_trousers_gathered` | zouave, harem, sirwal | waist, hip, outseam, body_rise, total_crotch_length | thigh, inseam, belly_depth | waist 30, hip 20, thigh 20 | `03 §1.2 Zouave row + 06 Q4 pass-over inequalities; rise and crotch length are minimums (06 Q3)` |
| `hat_fez` | hat, fez, cap | head | — | — | `03 §1.2 hat row` |

Option effects (applied in `readiness`, in this order, appending to the end of the must list):

- `bodice` + `passes_hip` → must += `hip`.
- `set_in_sleeve` + `closed_cuff` → must += `hand_pass` (and `hand_pass` removed from should, a no-op today).
- `trousers` or `full_trousers_gathered`: `nappies` → must += `seat_over_nappy`; `elastic_waist` → target(waist) = 30.
- `full_trousers_gathered`: `cuff_level` must be `knee|calf|ankle` (else error `cuff_level must be knee, calf or
  ankle`) → must += cuff_level; unless `cuff_opening`: must += `foot_pass`, and if cuff_level is `knee` also must +=
  `calf`.

(The section-tag strings such as `03 §1.2` or `06 Q3` are citations to the skill's research notes. They appear in
output strings and are reproduced verbatim; the port does not need the notes.)

### A.3 Measurement file, records, validation, value parsing

**File** (`fabric-measurements/1`): `{schema, wearer: {label, age_months, stature_mm, [sex], [nappies: true],
[reference: true], [stature_provenance]}, taken_on: "YYYY-MM-DD" | null, records: [...], fused: {...}, [fusion],
[notes], [photo]}`. A new file has `records: []`, `fused: {}`; `sex` and `nappies` keys are present only when set.

**Record**: `{name (canonical), definition_id, value_mm|value_deg, sigma_mm|sigma_deg, provenance, method, notes}`.
`make_record` canonicalises the name, rejects provenance not in PROVENANCES (`provenance must be one of (...)`),
defaults `definition_id` to the catalogue's id (null for unknown names), stores value and σ as floats.

**validate(data) → list of messages** (empty = valid). Checks in order:
1. not an object → `top level must be a JSON object` (only message).
2. `schema` ≠ `fabric-measurements/1` → `schema must be 'fabric-measurements/1'`.
3. `wearer` not an object → `wearer must be an object`; else `wearer.age_months` / `wearer.stature_mm`, when present
   and not null, must be finite numbers (not booleans) → `wearer.<key> must be a number or null`.
4. `taken_on` not null and not an ISO date → `taken_on must be YYYY-MM-DD or null`.
5. `records` not a list → `records must be a list`. Per record i: missing/blank name → `records[i]: needs a name`
   (skip rest). Unit from the canonical name. Missing the unit's value key → `records[i] <name>: missing value_mm`
   (or `value_deg`), and if it is an angle that carries `value_mm` the hint ` (<name> is an angle: use
   value_deg/sigma_deg)`; skip rest. Non-finite value → `...: <vk> must be a finite number`; σ not a finite number
   > 0 → `...: <sk> must be > 0`; provenance not allowed → `...: provenance must be one of MEASURED_TAPE|...`
   (pipe-joined).
6. `fused` present and not an object → `fused must be an object`.

**parse_value(text, unit)**: regex `^\s*([-+]?\d+(?:\.\d+)?)\s*(mm|cm|m|in|inch|inches|"|deg|°)?\s*$`,
case-insensitive. For unit `deg`: suffix must be empty, `deg` or `°`, else `'<text>': an angle, give degrees`.
For lengths the unit is mandatory: factors mm 1, cm 10, m 1000, in/inch/inches/`"` 25.4; missing → `'<text>': give a
unit (mm, cm, in) — tape numbers are ambiguous without one`. Unparseable → `cannot read '<text>'; write e.g. 55cm,
21.5in, 550mm`. Examples: `55cm` → 550, `21.5in` → 546.1, `18` (deg) → 18.

### A.4 Chart priors (public data only)

Priors are weak `CHART_PRIOR` records used as fusion inputs, sanity bounds and the reference wearer. Only free,
citable sources; ASTM tables are never used (policy: ASTM forbids AI use; the port must not add them).

#### A.4.1 Source strings (verbatim, emitted in each prior's `source`)

- SRC_OTTOBRE: `Ottobre Design kids size chart, height-indexed rows 86–122 cm (ottobredesign.com/pages/size-charts; 03 §1.4b, VERIFIED)`
- SRC_CPSC77: `CPSC/UMTRI 1977 child anthropometry UM-HSRI-77-17 via NIST AnthroKids, combined sexes, age-band mean/SD scaled by stature ratio (03 §2.5, VERIFIED; 1970s sample)`
- SRC_CPSC75: `CPSC/UMTRI 1975 crotch height, data1975/73.csv via NIST AnthroKids (03 §2.5, VERIFIED)`
- SRC_CDC: `CDC 2000 stature-for-age statage.csv (03 §1.4c, VERIFIED)`
- SRC_WHO: `WHO Child Growth Standards head-circumference-for-age medians, SD≈1.4 cm (03 §1.4d, VERIFIED); CDC 2000 cross-check`
- SRC_SECTION: `derived: CPSC 1977 section shape (girth vs breadth) via k·Ramanujan inversion, scaled to the Ottobre girth (03 §2.5, §3.3)`
- SRC_JUDGEMENT: `engineering judgement: no free source in 03`
- body_rise uses the composite `derived from <SRC_OTTOBRE> and <SRC_CPSC75>`.

#### A.4.2 Tables

Ottobre height-indexed rows (cm), indexed by stature H = 86, 92, 98, 104, 110, 116, 122 cm:

| key | 86 | 92 | 98 | 104 | 110 | 116 | 122 |
|---|---|---|---|---|---|---|---|
| chest | 52 | 54 | 56 | 58 | 60 | 62 | 64 |
| waist | 52 | 53 | 54 | 55 | 56 | 57 | 58 |
| hip | 57 | 58 | 60 | 62 | 64 | 66 | 68 |
| shoulder_length | 7.6 | 8.0 | 8.4 | 8.8 | 9.2 | 9.6 | 10.0 |
| arm_length | 30 | 32 | 34 | 36 | 38 | 40 | 42 |
| outseam | 50 | 54 | 58 | 62 | 66 | 70 | 74 |
| back_neck_to_waist | 21 | 22.5 | 24 | 25 | 26.5 | 28 | 29.5 |

Ottobre toddler inside leg (cross-check note only): 86 cm → 33.5, 92 cm → 37.0.

CPSC/UMTRI 1977 per age band, combined sexes, (mean, SD) cm; band centres 33, 48, 60, 72 months (bands 2.0–3.5,
3.5–4.5, 4.5–5.5, 5.5–6.5 y):

| key | 33 mo | 48 mo | 60 mo | 72 mo |
|---|---|---|---|---|
| stature | 93.4, 5.0 | 101.4, 4.5 | 108.3, 4.7 | 114.6, 5.1 |
| head | 49.5, 1.7 | 50.2, 1.4 | 50.7, 1.5 | 51.1, 1.6 |
| neck_mid (#32) | 23.8, 1.2 | 24.3, 1.4 | 25.0, 1.4 | 25.7, 1.4 |
| biacromial (#36) | 21.9, 1.4 | 23.0, 1.3 | 24.4, 1.2 | 25.8, 1.4 |
| upper_arm (#39) | 15.8, 1.3 | 16.2, 1.3 | 16.7, 1.4 | 17.2, 1.5 |
| chest_axilla | 50.7, 2.7 | 52.7, 2.7 | 55.0, 3.0 | 57.3, 3.4 |
| chest_breadth | 16.1, 1.2 | 16.7, 1.1 | 17.3, 1.0 | 17.9, 1.4 |
| waist_navel (#66) | 48.1, 3.2 | 49.3, 2.9 | 51.1, 3.6 | 52.0, 3.7 |
| waist_breadth_navel (#67) | 16.1, 1.0 | 17.0, 1.1 | 17.8, 1.2 | 18.3, 1.6 |
| waist_natural | 47.0, 3.1 | 48.4, 2.8 | 50.9, 2.8 | 51.6, 3.5 |
| hip_buttocks | 51.7, 3.2 | 53.5, 3.5 | 56.1, 3.5 | 58.3, 4.0 |
| hip_breadth_trochanter | 18.0, 1.0 | 18.7, 1.1 | 19.6, 1.0 | 20.2, 1.1 |
| upper_thigh | 29.1, 2.5 | 30.9, 2.6 | 32.5, 2.9 | 34.0, 3.1 |
| calf | 20.6, 1.5 | 21.2, 1.5 | 22.1, 1.5 | 23.0, 1.6 |
| ankle | 14.8, 1.0 | 14.9, 0.9 | 15.5, 0.9 | 16.0, 1.1 |

CPSC/UMTRI 1975 crotch height: (age 33.5 mo, mean 37.7 cm, SD 2.4, mean stature 93.1 cm) and (69.5 mo, 50.6 cm, 3.1,
113.3 cm).

CDC 2000 stature for age, (P5, P50, P95) cm at ages 24.5, 30.5, 36.5, 48.5, 60.5, 66.5, 72.5 months:

| sex | 24.5 | 30.5 | 36.5 | 48.5 | 60.5 | 66.5 | 72.5 |
|---|---|---|---|---|---|---|---|
| M | 81.1, 86.9, 92.6 | 85.3, 91.3, 97.7 | 89.2, 95.3, 101.9 | 95.6, 102.5, 109.5 | 101.5, 109.2, 116.8 | 104.4, 112.4, 120.3 | 107.3, 115.7, 123.9 |
| F | 79.6, 85.4, 91.1 | 84.2, 90.3, 96.6 | 87.8, 94.2, 100.8 | 94.0, 101.0, 108.4 | 100.4, 108.0, 116.1 | 103.6, 111.5, 120.0 | 106.9, 115.0, 123.9 |

WHO head-circumference medians (cm) at 24, 36, 48, 60 months: M 48.3, 49.5, 50.2, 50.7; F 47.2, 48.5, 49.3, 49.9.

Within-age population SDs (cm), POP_SD_CM: chest 3.0, waist 3.5, hip 3.5, neck_base 1.3, head 1.5, upper_arm 1.4,
arm_length 1.3, outseam 3.0, crotch_height 3.0, inseam 3.0, back_neck_to_waist 1.5.

Section shape factor k = girth / Ramanujan(width/2, depth/2), K_SECTION (k, σ_k, note verbatim):

| level | k | σ_k | note |
|---|---|---|---|
| waist | 1.06 | 0.022 | `ANSUR II adult fit k=1.054 M / 1.061 F (03 §3.3); UNCERTAIN for children` |
| hip | 1.09 | 0.020 | `ANSUR II adult fit k=1.089 M / 1.097 F (03 §3.3); UNCERTAIN for children` |
| chest | 1.07 | 0.030 | `UNCERTAIN: toddler chest k≈1.05–1.10 assumed; adult chest k 1.16–1.24 does not transfer (03 §3.3)` |

#### A.4.3 Sex and age helpers

- `sex_key(sex)`: first character upper-cased; `M` or `B` → M, `F` or `G` → F, anything else/empty → none.
- `cdc_stature(age, sex)` → (P50 mm, SD mm): for the sex key, or both sexes averaged when none: `P50 = mean(interp_x(age,
  CDC ages, P50s))`, `SD = mean(interp_x(age, CDC ages, (P95 − P5)/3.29))`, both × 10. Example: 30 mo, no sex →
  (904.125, 37.462).
- `age_from_stature(mm)`: clamped interpolation of the sex-averaged P50 (mm) curve onto CDC ages (so 24.5–72.5 mo).
  Example: 920 mm → 32.32 mo.

#### A.4.4 `priors_for(stature_mm, age_months = null, sex = null)`

Let S = stature_mm / 10 (cm). If S ∉ [80, 128] raise `chart priors cover stature 80–128 cm (toddlers and young
children); measure everything else`.

Common notes (base note, joined with `; `, appended to every prior's notes):
- age not given → age = age_from_stature, note `age inferred from CDC P50 stature: <age:.0f> mo`;
- age ∉ [18, 84] → `age outside 18–84 mo: priors extrapolated (UNCERTAIN)`;
- S ∉ [86, 122] → `stature outside Ottobre rows 86–122 cm: extrapolated (UNCERTAIN)`.
`note(parts…)` = `; `-join of the non-empty parts followed by the base note.

CPSC scaling: `age_c = clamp(age, 33, 72)`; age_note = empty if age_c == age, else `CPSC band centres span 33–72 mo;
nearest band used at <age:.0f> mo`. `s_age = interp_x(age_c, band centres, CPSC stature means)`, `ratio = S / s_age`.
`cpsc(key, elastic)`: mean and SD by interp_x at age_c, multiplied by `f = ratio^(0.5 if elastic else 1)`, × 10 → mm.
`cpsc_method = "CPSC 1977 at <age_c:.0f> mo × (stature ratio <ratio:.3f>)"`.
`ott(key) = interp_x(S, Ottobre H, row) × 10`. `elastic_note = "girth scaled by ratio^0.5 (Ottobre steps imply
≈0.55; UNCERTAIN)"`.

Each prior record (`_prior`): `{name, definition_id (catalogue), value, sigma, provenance: "CHART_PRIOR", method,
source, notes, p3_p97_<u>: [v − 1.881σ, v + 1.881σ], p1_p99_<u>: [v − 2.326σ, v + 2.326σ]}`, where value, σ and the
bounds are rounded to 1 decimal (2 for degrees) and the bounds are computed from the *unrounded* v and σ. Key suffix
`_mm` or `_deg`.

Items, in computation order (σ in mm unless stated; "rounded X" means the already-rounded value of prior X):

| name | value | σ | method string | extra note parts |
|---|---|---|---|---|
| stature (only when age_months was given) | CDC P50 at age (sex) | CDC SD | `CDC P50 at <age:.0f> mo` | `only used when stature is not measured` |
| chest, waist, hip, arm_length, outseam, back_neck_to_waist | ott(key) | POP_SD × 10 | `Ottobre row interpolated at <S:.1f> cm` | `σ = within-age SD per 03 §0.2` |
| shoulder_length | ott | 6.0 | same | `UNCERTAIN σ: Ottobre gives no SD; 6 mm by analogy with biacromial CV≈6%` |
| seat_over_nappy | rounded hip + 50 | hypot(35, 20) | `hip prior + 5 cm nappy bulk` | `UNCERTAIN placeholder: nappy bulk is not tabulated; always tape it over the nappy` (source SRC_JUDGEMENT) |
| crotch_height, inseam | crotch = ratio_c × S × 10 | 30 | `crotch/stature ratio <ratio_c:.3f> at <a_c:.0f> mo` | `σ = 3 cm per 03 §0.2`, and if S ≤ 95: `cross-check Ottobre toddler inside leg <interp_x(S,[86,92],[33.5,37.0]):.1f> cm` |
| waist_height | rounded outseam − 10 | 30 | `Ottobre outside leg − 1 cm (tape follows the hip)` | `UNCERTAIN offset (surface over hip vs vertical)` (source Ottobre) |
| body_rise | waist_height − crotch + 15 | 25 | `waist height − crotch height + 1.5 cm seated (03 §1.1, §4.4)` | `UNCERTAIN: ribbon-level (Ottobre) waist; the CPSC navel-level derivation gives ≈11–12 cm standing at 3 y (03 §2.5) — the waist level is the difference` |
| total_crotch_length | 2.3 × body_rise | 40 | `2.3 × body rise` | `UNCERTAIN placeholder (no free child source); tape it (06 Q3)` (judgement) |
| knee_height | 0.62 × crotch | 20 | `0.62 × crotch height` | `UNCERTAIN placeholder` (judgement) |
| head | mean over sex keys of interp_x(age, WHO ages, WHO medians) × 10 (no stature scaling) | 15 | `WHO median at <age:.0f> mo` + `, sexes averaged` when no sex | `σ = 1.5 cm per 03 §0.2`, and `extrapolated beyond 24–60 mo (UNCERTAIN)` when age ∉ [24, 60] |
| neck_base | cpsc(neck_mid, elastic) mean + 15 | hypot(13, 8) | `<cpsc_method>: mid-neck #32 + 1.5 cm` | `UNCERTAIN: neck base runs larger than CPSC mid-neck girth; +1.5 cm is judgement`, elastic_note, age_note |
| upper_arm | cpsc(upper_arm, elastic) mean (unrounded = ua) | 14 | `<cpsc_method>: #39` | elastic_note, age_note |
| thigh, calf, ankle | cpsc(upper_thigh / calf / ankle, elastic) | the scaled CPSC SD | `<cpsc_method>: <src key>` | elastic_note, age_note |
| across_back | cpsc(biacromial, not elastic) mean + 10 | hypot(scaled SD, 8) | `<cpsc_method>: biacromial #36 + 1 cm surface allowance` | `UNCERTAIN: surface across back vs bony breadth offset is judgement`, age_note |
| wrist | 0.70 × ua | 8 | `0.70 × upper-arm prior` | `UNCERTAIN placeholder` |
| hand_pass | 1.30 × 0.70 × ua | 10 | `1.30 × wrist prior` | `UNCERTAIN placeholder` |
| knee | 1.03 × rounded calf | 20 | `1.03 × calf prior` | `UNCERTAIN placeholder` |
| foot_pass | 1.40 × rounded ankle | 15 | `1.40 × ankle prior` | `UNCERTAIN placeholder` |
| shoulder_slope_deg | 18.0° | 5.0° | `placeholder` | `UNCERTAIN placeholder: measure from the P3 back photo (06 Q3)` |
| armscye_depth | 0.13 × S × 10 | 15 | `0.13 × stature` | `UNCERTAIN placeholder; drafted from chest and stature (03 §1.2)` |
| chest/waist/hip `_width` and `_depth` | see below | see below | see below | `UNCERTAIN: section shape from 1970s CPSC breadths and adult k (03 §3.3)` (+ `σ = 0.6 × girth σ` for depths), age_note |
| belly_depth | see below | 0.6 × waist σ | `navel-level depth inverted from CPSC #66/#67, scaled like waist` | section note, `measure it from the P2 side photo (06 Q3)`, age_note |

Crotch ratio: `r_lo = 37.7/93.1`, `r_hi = 50.6/113.3`, `a_c = clamp(age, 33.5, 69.5)`, `ratio_c = clamped-interp(a_c,
[33.5, 69.5], [r_lo, r_hi])`.

Sections (levels chest/waist/hip with CPSC girth key chest_axilla / waist_natural / hip_buttocks and breadth key
chest_breadth / waist_breadth_navel / hip_breadth_trochanter): `k = K_SECTION[level].k`; `cg` = interp_x CPSC girth
mean × 10; `cb, cb_sd` = interp_x CPSC breadth mean and SD × 10; `d = 2 · semi_axis_for_perimeter(cg / k, cb / 2)`;
`s = rounded prior(level) / cg`. Then `<level>_width = cb · s` with σ `cb_sd · s`, method `CPSC <breadth key> ×
<s:.3f> (Ottobre/CPSC girth ratio)`; `<level>_depth = d · s` with σ `0.6 × σ(level)`, method `inverted from CPSC
<girth key> and <breadth key>, k=<k>`. All with source SRC_SECTION.

Belly depth: `cn`, `cnb` = interp_x CPSC waist_navel and waist_breadth_navel means × 10; `belly = 2 ·
semi_axis_for_perimeter(cn / 1.06, cnb / 2) · rounded waist / (interp_x CPSC waist_natural mean × 10)`.

The returned map is ordered by the catalogue and contains every catalogue name except `stature` when no age was
given (34 names), or all 35 when an age was given.

`semi_axis_for_perimeter(P, a)`: requires `P > 4a` (else `perimeter too small for this semi-axis`); bisection on
b ∈ [1e−9, P/π], 200 iterations, `ramanujan(a, mid) < P` → raise lo; return the midpoint.

Reference values: see section D.2 (920 mm / 30 mo, every prior with bounds and method) and D.6 (other statures).

### A.5 Girth from a width and a depth

`girth_from_section(width, depth, level = "waist", σ_w = 0, σ_d = 0, k = null, k_sigma = null, k_ratio = 1,
k_ratio_sigma = 0, clothing_mm = 0)`:
- If level ∈ K_SECTION take (k0, σk0, note) from it; else if k is null raise `level '<level>' has no default k; pass
  k (and k_sigma)`; else (k0, σk0, note) = (k, 0.03, `user k`).
- `k_use = (k ?? k0) · k_ratio`; `σk = hypot((k_sigma ?? σk0) · k_ratio, (k ?? k0) · k_ratio_sigma)`.
- `w = width − 2·clothing`, `d = depth − 2·clothing`; `P = Ramanujan(w/2, d/2)`.
- Central differences with step `h = 1e−3 · max(w, d)` on the full width/depth:
  `dP/dw = (P((w+h)/2, d/2) − P((w−h)/2, d/2)) / 2h`, likewise dP/dd.
- σ terms: width `k_use · dP/dw · σ_w`, depth `k_use · dP/dd · σ_d`, k `P · σk`; σ = root-sum-square.
- Output `{level, girth_mm: round1(k_use·P), sigma_mm: round1, ellipse_perimeter_mm: round1(P), k: round4(k_use),
  k_sigma: round4(σk), sigma_terms_mm: {width, depth, k} (abs, round2), width_mm, depth_mm (as given),
  clothing_mm_per_side, notes}`; notes = the k note, plus `; clothing offset subtracted (2–5 mm per side for thin
  clothes, UNCERTAIN)` when clothing > 0.

`calibrate_k(tape_girth, width, depth, level, σ_tape = 10, σ_w = 0, σ_d = 0, clothing = 0)`: run girth_from_section
with k = 1, k_sigma = 0; `P` = its ellipse perimeter, `σP` = its σ; `k_child = tape / P`; `σk = k_child ·
hypot(σ_tape/tape, σP/P)`; `k_default` = K_SECTION k (1.0 for unknown levels). Output `{level, k_child (round4),
k_child_sigma (round4), k_default, k_ratio = round4(k_child/k_default), k_ratio_sigma = round4(σk/k_default), use:
"pass --k-ratio/--k-ratio-sigma to girth-from-section for this child's other levels"}`.

### A.6 Normalization and fusion

#### A.6.1 Normalizing one record

Canonical name; value/σ/unit from the record; `did` = record definition_id or null.
- Status `ok` (unchanged) when: the name is not in the catalogue, or did is null, or did equals the catalogue id, or
  did is one of its equivalent ids.
- Status `normalized` when did has a hook (offset, extra, reason): value += offset, σ = hypot(σ, extra), note
  `<did> → <catalogue id>: <offset:+.0f> mm, σ+<extra:.0f> (<reason>)`.
- Otherwise status `excluded`, note `definition <did> has no normalization to <catalogue id>; not fused`.

#### A.6.2 Collapsing repeats

Observations are grouped by (provenance, method), first-seen order. A single member passes through with n = 1. A
group of n ≥ 2 becomes one observation: value = median of values, stated = median of σs, spread = sample standard
deviation (n − 1 denominator), σ = max(stated, spread), note = existing note `; ` `<n> repeats: median, spread
<spread:.1f>`. Flags: `REPEATS_COLLAPSED` always for such a group; `REPEATS_SPREAD` too when spread > 1.5 × stated.
Repeats never shrink σ below the stated value (they are not treated as independent).

#### A.6.3 `fuse_one(name, observations, prior, use_prior)`

Rounding: values and σ to 1 decimal (2 for degrees); weights to 4 decimals.
1. Collapse repeats (flags carried).
2. Bounds: the prior's `p1_p99` if present, else prior value ± 2.326 × prior σ.
3. No observations and no prior → no entry.
4. No observations → `{unit, value, sigma (prior), primary_provenance: "CHART_PRIOR", sources: [prior, weight 1,
   used], flags: ["PRIOR_ONLY"], p1_p99 (if bounds)}`.
5. z over every pair of observations: `|x_i − x_j| / hypot(σ_i, σ_j)`; keep the first strictly-largest pair.
6. z_max > 3 (REFUSE): take the observation with the smallest σ (first on ties) as the value; flags + `REMEASURE`;
   note `not fused: <A prov> <A.value:.1f>±<A.σ:.1f> vs <B prov> <B.value:.1f>±<B.σ:.1f>, z=<z:.2f> > 3 — re-measure
   this one` (A, B = the max pair). Entry: `{unit, value, sigma, primary_provenance (best's), sources: every
   observation with weight 1.0/used true for the best and 0.0/false for the rest, flags (sorted, unique), z_max
   (round2), notes, p1_p99}`. The prior is not listed in this branch.
7. 2 < z_max ≤ 3: flag `Z_WARN`, note `sources disagree at z=<z:.2f> (2<z≤3): fused, but check`.
8. Evidence-only inverse-variance mean `x_e = Σ(x/σ²)/Σ(1/σ²)`.
9. If a prior exists and x_e is outside the bounds: flag `VERIFY_OUT_OF_BOUNDS`, note `measured <x_e:.1f> is outside
   the chart P1–P99 [<lo:.1f>, <hi:.1f>]: confirm once; chart prior left out of the fusion`. Else if a prior exists
   and use_prior: the prior joins the fused set.
10. Fused value `Σ(w·x)/Σw`, σ `(Σw)^−½`, w = 1/σ². Normalized weights `w/Σw`; `primary_provenance` = the
    provenance with the largest summed normalized weight (first inserted wins ties).
11. Sources: the used observations (and prior if used) with their normalized weights, `used: true`; a prior that was
    not used is appended with weight 0, `used: false`.
12. Entry `{unit, value, sigma, primary_provenance, sources, flags (sorted unique), z_max (only when ≥ 2
    observations), p1_p99 (when bounds), notes (when any)}`.

Source entry: `{provenance, method, definition_id, value, sigma, weight, used, [n_repeats if n > 1], [note], [auto_prior:
true when the prior came from the chart rather than a user record]}`.

#### A.6.4 `fuse(data, use_priors = true)` → copy of data with `fused` and `fusion`

1. Validate; problems raise `invalid measurement file: <messages joined by "; ">`.
2. Normalize every record; excluded ones are collected per name (notes), normalized names remembered; the rest grouped
   by canonical name.
3. Stature used to condition priors, first that applies: fused non-prior stature records (`fused stature records`);
   the first `CHART_PRIOR` stature record (`stature CHART_PRIOR record`); `wearer.stature_mm` (`wearer.stature_mm`);
   CDC P50 at `wearer.age_months`/sex (`CDC P50 for age (stature not measured)`); none (`none`).
4. Priors = priors_for(stature, wearer.age_months, wearer.sex). If it raises, the message becomes the fusion note; no
   stature → note `no stature or age: no chart priors or sanity bounds`.
5. Names fused: catalogue names (catalogue order) that have records, or that have a prior when use_priors; then
   non-catalogue names with records, sorted.
6. Per name: evidence = non-`CHART_PRIOR` observations; user priors = `CHART_PRIOR` records. The prior is the
   lowest-σ user prior (its own p1_p99 bounds) if any; else the chart prior when (use_priors or there is evidence);
   else none. So with use_priors false a chart prior still supplies sanity bounds for measured names (it is listed as
   unused, weight 0). Special case: `stature` with no evidence and no user prior but a numeric `wearer.stature_mm`
   gets one synthetic observation (σ 10, provenance `wearer.stature_provenance` or `MEASURED_TAPE`, method
   `wearer.stature_mm`, note `from the wearer block`). Call fuse_one with use_prior = use_priors or (user priors
   exist).
7. Add flags `NORMALIZED_DEFINITION` (some record was normalized), `UNNORMALIZED_EXCLUDED` (some record was excluded;
   its notes are appended to the entry's notes), `NOT_IN_CATALOGUE`.
8. `fusion = {stature_for_priors_mm (round1 or null), stature_source, priors_used: use_priors and priors non-empty,
   z_refuse: 3.0, z_warn: 2.0, notes: [prior note] or [], [excluded: {name: [notes]}]}`.

`fused_value(data, name)` returns (value, σ) of a fused entry after canonicalising the name.

### A.7 Readiness gate

`readiness(data, garment, passes_hip = false, closed_cuff = false, nappies = null, elastic_waist = false, cuff_level =
"calf", cuff_opening = false, as_of = null, use_priors = true)`:

1. Resolve the kind; fuse the data; `reference = wearer.reference`; nappies defaults to `wearer.nappies`.
2. Targets: catalogue target σ per name, then the kind's overrides, then option effects (A.2.2). Must/should lists
   with option effects.
3. For each must item, then each should item: `target = targets[name] or tape σ or 10` (falsy fallback). Item
   `{name, role, target_sigma_<u>, [value, sigma, primary_provenance, flags], status}` where status is:
   - `MISSING` (no fused entry);
   - else `REMEASURE` if flagged, else `PRIOR_ONLY` if flagged, else `OK` if σ ≤ target + 1e−9, else
     `OK_TAPE_LIMITED` if some *used* source is a direct provenance and σ ≤ 2·target + 1e−9, else `TOO_UNCERTAIN`.
   Note `PRIOR_ONLY` blocks even when the prior's σ meets the target.
4. Advisories, in item order, using `fmt(v)` = `<v/10:.1f> cm` (or `<v:.1f>°`; `fmt(v, 2)` uses 2 decimals):
   - `VERIFY_OUT_OF_BOUNDS` → `<name>: <fmt value> is outside the chart P1–P99 for this height — confirm once.`
   - `Z_WARN` → `<name>: sources disagree (z <z_max>); fused, worth a second look.`
   - `OK_TAPE_LIMITED` → `<name>: ±<fmt(σ,2)> is the tape's limit (target ±<fmt(target,2)>); check it on the muslin.`
   - status OK/OK_TAPE_LIMITED with primary provenance `CHART_PRIOR` → `<name>: the size-chart prior outweighs the
     measurement here; fine for drafting, re-check on the muslin.`
5. Blocking statuses (MISSING, PRIOR_ONLY, TOO_UNCERTAIN, REMEASURE) produce the line
   `<name>: <how text> Need ±<fmt target>; <have>.` with have = `not measured yet` | `only a size-chart guess
   (±<fmt σ>)` | `have <fmt value> ±<fmt σ> (<primary provenance>)` | `tape and photo disagree — re-measure`.
   Must items → `blocking` and `remeasure_request`; should items → advisory `optional — <line>`.
6. Staleness (not for reference wearers): if `taken_on` parses, days = (as_of or today) − taken_on; > 42 →
   advisory `STALE: measured <days> days ago; a toddler grows ~0.7 cm/month — re-take lengths.` (unparseable dates are
   ignored).
7. Status: `READY_REFERENCE` for a reference wearer; else `READY` when nothing blocks, `NOT_READY` otherwise.
8. Output `{schema: "fabric-readiness/1", garment (resolved kind), status, basis, options: {passes_hip, closed_cuff,
   nappies, elastic_waist, cuff_level (null unless full_trousers_gathered), cuff_opening}, items, blocking,
   remeasure_request, advisories}`. A reference wearer gets `blocking: []`, `remeasure_request: []`, `note:
   "Reference wearer: values are definitions built from public priors; σ is population spread. A real child would
   still need the list in for_real_child."` and `for_real_child` = the request lines.

Native readiness exception: before applying sigma thresholds, a fused item with primary provenance
`MODEL_FIT` or any *used* `MODEL_FIT` source receives item status `MODEL_FIT`. Required items block;
optional items produce an advisory requesting independent evidence. REMEASURE and PRIOR_ONLY keep
their existing precedence. Unused/excluded estimates cannot block independent measurements. Reference
wearers remain explicitly fictional READY_REFERENCE, never ordinary READY. Fusion values, source
weights and residuals are retained unchanged; an apparently small supplied sigma does not certify a
model prediction. This intentionally strengthens the reference's sigma-only branch. The capture-to-round
regression reproduced the defect with all required records labelled MODEL_FIT and 1 mm test weights:
the native gate returned READY. Direct and mixed-source regressions are in
`ftx-measure/tests/readiness.rs`; no reference fixture is regenerated.

CLI `ready` exits 0 for READY/READY_REFERENCE and 3 otherwise.

Integration (round driver, specified elsewhere, summarized because drafting roles feed it): piece roles select the
gates. `bodice_front|bodice_back` → `bodice`; `sleeve` → `set_in_sleeve` with `closed_cuff` true; `pant_front|
pant_back` → `full_trousers_gathered` (if a `cuff` role exists; options `elastic_waist` = a `waistband` role exists,
`cuff_level` from the pattern's style (default calf), `nappies` true if style.nappy) else `trousers` (elastic_waist as
above); `hat_crown|hat_tip` → `hat_fez`. A measurements object without `records` (plain name → value map) or a pattern
without roles yields `NOT_CHECKED`.

### A.8 Reference wearer

`reference_wearer(stature_mm, age_months = null, sex = null, label = null)`:
priors = priors_for(stature, age, sex); file = new file with label (default `reference toddler <stature/10:g> cm`),
age, sex, stature; `wearer.reference = true`; `notes = "Reference wearer built only from public priors (03 §1.4,
§2.5); not a real child."`; `taken_on` null. The stature prior is replaced by value = the given stature, σ = CDC SD at
age (or at age_from_stature when no age), source SRC_CDC, method `conditioning stature of the reference wearer (by
definition)`, notes `σ = CDC within-age SD; the value is the definition`. Records = all priors in catalogue order
(all `CHART_PRIOR`). Return `fuse(file)`. Every fused entry is therefore `PRIOR_ONLY`, `stature_source` is `stature
CHART_PRIOR record`, and readiness is `READY_REFERENCE` for every kind.

Hard invariant: demos, examples and tests use only this wearer; no real person's measurements ship with the port.

### A.9 ChArUco calibration sheet (`calib-sheet`)

`calib_sheet(paper, out_dir, square_mm = 22, marker_mm = 16, dictionary = "DICT_4X4_50")` writes
`calib-sheet-<paper>.png`, `.pdf` and `.json` and returns the sidecar (+ `paths`, not written to the JSON).

Constants: PAPERS letter 215.9 × 279.4 mm, a4 210 × 297 mm; DPI 300; PPM = 300/25.4 px per mm; `px(mm) =
round(mm·PPM)`; SHEET_SQUARES letter 8 × 6, a4 8 × 7. Errors: unknown paper → `paper must be letter or a4`;
non-integer sizes or not 0 < marker < square → `square_mm and marker_mm must be integers with marker < square`.

Layout (mm, page origin top-left, y down):
- board `bw = sx·square`, `bh = sy·square`; `ybar_gap 4.0`, `bar_t 3.5`, `bar_gap 6.0`; `block_w = bw + 4 + 3.5 + 6 +
  3.5`; `x0 = (W − block_w)/2`; `y0 = 8.0`; `band_y = y0 + bh + 6 + 9 + 3.5 + 1.5 + 6`. If `band_y + 100 > H − 6` or
  `block_w > W − 10` → `board does not fit on this paper; use a smaller square_mm`.
- Board image: an OpenCV-compatible ChArUco board (sx × sy squares, square and marker size in mm, the named
  predefined ArUco dictionary, non-legacy layout, top-left square black, markers in the white squares, ids 0.. in
  row-major order: on the 8 × 6 letter board, ids 0–3 sit in row 0 columns 1, 3, 5, 7, ids 4–7 in row 1 columns 0,
  2, 4, 6, and so on; marker border 1 bit, no margin). Rendered at 12 px/mm, then nearest-neighbour resized to
  `(px(bw), px(bh))` and pasted at `(px(x0), px(y0))`.
  Provider clarification (OpenCV 4.13.0, fixture `calibration_sheets_opencv413`): each marker's
  canonical row-major data bits plus its one-cell black border are nearest-neighbour enlarged to
  `12*marker` pixels. Destination pixel `x` reads source cell `floor(x*(bits+2)/(12*marker))` (likewise y);
  this matters when the requested physical marker size is not divisible by its cell count. The 27
  predefined names/aliases and 108 independent board rasters are frozen in
  `tests/fixtures/oracle/measure/calibration_sheets.json`. These supplement the unchanged Python skill
  pin, rather than replacing its fixtures. Fixed 8×6/7 sheets use only IDs 0–23/27; no arbitrary
  dictionary editor is implied. The built-in image detector still accepts only DICT_4X4_50; other
  printed dictionaries require matching external detection or reviewed corner-coordinate input.
- Bars (alternate segments filled, starting filled; thin outline `px(0.25)`; end ticks `px(2.0)` beyond each side):
  `y_100mm` vertical at x = x0 + bw + 4, y = y0 + 4, 100 mm, 10 mm segments, label `100 mm`; `y_4in` at x = that +
  3.5 + 6, 101.6 mm, 12.7 mm segments, label `4 in`; `x_100mm` horizontal at (x0, y0 + bh + 6), label `100 mm  (10 mm
  segments)`; `x_4in` at (x0, y0 + bh + 15), label `4 in  (1/2 in segments)`. Bar sidecar: `{name, axis, length_mm =
  round3(px(length)/PPM), nominal_mm, start_mm, end_mm (mid-thickness end points, round3), thickness_mm 3.5,
  segment_mm}`.
- Test squares (outline `px(0.5)`): `square_10cm` (100 mm) at (x0, band_y) and `square_1in` (25.4 mm) at (x0 + 106,
  band_y); sidecar `{name, size_mm (round3 of the rendered size), nominal_mm, origin_mm}`. Captions `10 cm square`,
  `measure outer edge to outer edge`, `1 in square`, `(outer edges)`.
- Instruction text at x0 + 106 from y = band_y + 31: `PRINT AT 100% / ACTUAL SIZE.` (title, advance 6.2 mm), then
  body lines (advance 5.0) `Turn OFF 'Fit to page' / 'Scale to fit'.`, `Check both bars with a ruler before use:`,
  `100 mm and 4 in, along X and along Y.`, `Tape the sheet flat to stiff card.`, `Photos: sheet beside the child,
  facing`, `the camera, centre over the floor line.`, then small lines (advance 4.0) `ChArUco <dict>, <sx>x<sy>
  squares, square <sq> mm, marker <mk> mm` and `<PAPER> <W> x <H> mm, 300 dpi  -  fabric-pattern calib sheet v1`.
  Overflow past H − 4 → `sheet text overflows the page`. Font sizes 2.4 / 3.2 / 4.2 mm (small / body / title); the
  typeface is not a conformance target.
- Page thresholded to pure black/white (≥ 128 → white), PNG with 300 dpi metadata, page size `px(W) × px(H)` (letter
  2550 × 3300, a4 2480 × 3508); PDF at exact paper size (MediaBox letter 612 × 792 pt, a4 595.2 × 841.92 pt), stored
  losslessly as 1-bit (CCITT G4 when available; `pdf_lossless` records whether it was).

Sidecar (`fabric-calib-sheet/1`): `{schema, paper, paper_mm, dpi, image_px, board: {type: "charuco", dictionary,
squares_x, squares_y, square_mm, marker_mm, legacy_pattern: false, origin_mm (round3 of px(x0)/PPM, px(y0)/PPM),
origin_px, size_mm [bw, bh], size_px, png_scale = round6(size_px.x / PPM / bw), frame: "board mm: x right, y down from
the board's top-left outer corner; chessboard corner id i at ((i % (sx-1) + 1)·square, (i // (sx-1) + 1)·square)"},
bars, test_squares, files: {png, pdf}, pdf_lossless, print_instructions: "Print at 100% / Actual size. Check both
bars with a ruler before use.", pixel_convention: "page mm = px / (dpi/25.4) at pixel edges; OpenCV pixel centres sit
at (px + 0.5)"}`. Interior chessboard corners: (sx − 1)(sy − 1) (35 letter, 42 a4).

### A.10 Photo scale with the ChArUco sheet

Common helpers:
- `apply_h(H, pts)`: homogeneous 3 × 3 mapping of (N, 2) points.
- `h_jacobian(H, p)`: with `w = H20 x + H21 y + H22`, `q = H·p` (dehomogenized), J = `[[H00 − qx H20, H01 − qx H21],
  [H10 − qy H20, H11 − qy H21]] / w`.
- `plane_scale(H, bw, bh)` (H maps image px → plane mm): centre_px = H⁻¹(bw/2, bh/2); J at centre_px; singular values
  sv (descending); `mm_per_px = √|det J|`; `tilt = degrees(acos(min(1, sv1/sv0)))`; scale at the four rectangle
  corners `√|det J|` (each evaluated at H⁻¹ of the corner); `variation = max/min − 1`.
- `scale_warnings(tilt, variation, blur, rms_mm, mmpp)`, in order:
  - tilt > 10 → `STRONG_PERSPECTIVE: sheet ≈<tilt:.0f>° off the image plane (10° ≈ 1.5% foreshortening, 03 §3.1); body widths via this sheet will be off — turn the sheet square to the camera`
  - variation > 0.05 → `STRONG_PERSPECTIVE: scale varies <100·variation:.1f>% across the sheet (too close / wide lens?); use the 2× lens from ~2.5 m`
  - blur known and > 2.0 → `BLURRY: edge blur ≈<blur:.1f> px; use burst/timer, more light`
  - rms_mm > 0.5 → `HIGH_RESIDUAL: <rms:.2f> mm RMS — sheet not flat (tape it to card) or strong lens distortion`
  - mmpp > 1.0 → `LOW_RESOLUTION: <mmpp:.2f> mm/px at the sheet; each landmark click is ±<3·mmpp:.0f> mm`

Images are read with EXIF orientation applied; unreadable → `cannot read image <path> (convert HEIC to JPEG/PNG
first)`. A sidecar whose schema is not `fabric-calib-sheet/1` → `<path>: not a fabric-calib-sheet/1 sidecar`. CLI
measurement files are validated on load (`<path>: <messages joined by "; ">`).

`analyze_photo(image, sheet)`:
1. Detect ChArUco corners on the grey image (marker detector with minimum marker perimeter rate 0.01, refine markers
   on). Corner object points are the board's chessboard corners in board mm (sidecar frame).
2. Report skeleton `{schema: "fabric-photo-scale/1", image_size_px [w, h], paper, corners_detected n, corners_total,
   markers_detected, warnings: [], notes: ["Print scale assumed 100%: confirm the 100 mm bar with a ruler (05 #9).",
   "Scale is exact only on the sheet's plane (03 §3.1)."]}`.
3. n < 6 → `ok: false`, warning `BOARD_NOT_FOUND: <n> ChArUco corners (need ≥6; ≥12 for a usable scale)`.
4. Homography image px → board mm by RANSAC (reprojection threshold 2.0 mm); if ≥ 4 inliers, refit by plain least
   squares on the inliers. Failure → `homography fit failed`.
5. Residuals on inliers in mm (`rms_mm`) and back-projected in px (`rms_px`); `dof = max(2·inliers − 8, 1)`;
   `sigma_point = √(Σres²/dof)`; `max_residual_mm`.
6. `plane_scale(H, sx·square, sy·square)`; `square_px = square / mmpp`.
7. Blur estimate: ROI = bounding box of the detected corners padded by square_px (clipped); fewer than 100 pixels →
   unknown; Sobel 3 × 3 gradients divided by 8; `contrast = P98 − P2` of the ROI; `gmax = P99.5` of gradient
   magnitude; non-positive → unknown; `raw = contrast/(√(2π)·gmax)`; `blur = √max(raw² − 0.64, 0)` (0.8 px is a perfect
   step through Sobel).
8. Report adds `ok: true, inliers, homography_px_to_board_mm (round 10), board_origin_on_paper_mm, board_centre_px
   (round2), mm_per_px (round6), px_per_mm (round4), mm_per_px_axes (round6), reprojection_rms_mm (round4),
   reprojection_rms_px, sigma_point_mm, max_residual_mm, tilt_deg_estimate (round2),
   scale_variation_across_board (round4), square_px (round2), blur_sigma_px_estimate (round2 or null)`.
9. Warnings: n < 12 → `TOO_FEW_CORNERS: <n> of <total>; need ≥12 for a reliable scale`; else n < 0.5·total →
   `PARTIAL_BOARD: <n> of <total> corners — fingers, glare or the frame edge cover the sheet`; then
   scale_warnings. `ok` stays true with warnings.
10. Context for measuring: H, inlier image points and object points, σ_point.

### A.11 Plain Letter / A4 sheet as the scale reference

Purpose: a blank sheet of known size replaces the printed board. Four corners fix the homography exactly, so the only
evidence that the quad is the named paper is a **rectangle check** (can this quad be a rectangle of that aspect seen
by a pinhole camera?).

Constants: SHEET_SEARCH_PX 1200 (long side of the search copy); SHEET_MIN_AREA 0.003 (of the search image);
SHEET_MAX_SATURATION 60 (HSV S of 255); SHEET_END_SKIP 0.12 (fraction of each side skipped at both ends);
RECT_OK 0.025; SHEET_SIZE_SIGMA_MM 0.5 (1σ per sheet side, bootstrap); PAPER_LABELS letter → `Letter`, a4 → `A4`.

Geometry helpers:
- `line_x((p, d), (q, e))`: intersection of two point-direction lines; null if `|d × e| < 1e−9·|d|·|e|`.
- `poly_area`: shoelace `½Σ(x_i y_{i+1} − x_{i+1} y_i)`, positive when the corners run clockwise on screen (y down).
- `convex_cw(quad)`: every cross product of consecutive edge vectors > 0.
- `sheet_obj(w, h)` = [(0,0), (w,0), (w,h), (0,h)] (sheet mm, corner order = quad order).

Step 1, candidate quads (`sheet_candidates`, on a copy scaled by `s = min(1, 1200/max(h, w))`, area interpolation):
1. Grey blurred with Gaussian σ 1.5; HSV saturation blurred σ 1.5.
2. Otsu threshold of the grey → `otsu`. Masks: (a) `otsu AND saturation ≤ 60`; (b) `otsu`; (c) if otsu has any
   pixel, a second Otsu threshold computed only over the bright class, mask `grey > t2` (finds a sheet on a light
   wall).
3. Each mask: morphological opening with a 5 × 5 elliptical kernel; contours with two-level hierarchy, simple chain
   approximation.
4. Reject contours that are holes (have a parent), have area < 0.003 × image area, or whose bounding box comes within
   1 px of the frame edge.
5. Convex hull → polygon approximation with tolerance 0.004 × hull perimeter → `hull_quad` (needs ≥ 4 vertices).
   Reject when no quad or when the contour area < 0.85 × quad area (the blob must fill its quad).
6. `hull_quad`: while more than 4 vertices, find the shortest edge (p_i → p_{i+1}), replace p_i by the intersection
   of the line through p_{i−1} along (p_i − p_{i−1}) and the line through p_{i+2} along (p_{i+1} − p_{i+2}), delete
   p_{i+1}; parallel → none. Orient clockwise on screen, rotate to start at the vertex with the smallest x + y, return
   only if convex. (A thumb over a corner cuts that corner off the blob; the two neighbouring sides still meet at the
   true corner.)
7. Rescale to full resolution: `(quad + 0.5)/[sw/w, sh/h] − 0.5`. Keep only if every already-kept quad differs by a
   max corner distance > 2/s px. Return up to 12, largest area first.

Step 2, edge refinement (`refine_quad`, on the full-resolution grey image blurred σ 1.0, run twice: first with
`half = max(8, round(0.05 · shortest side))`, then again from the refined corners with half = 8):
- For each side i (corner i → corner i+1), `edge_points`:
  - unit direction d, normal n = (d.y, −d.x) flipped to point away from the quad centre;
  - `count = int(clip((1 − 2·0.12)·length/3, 16, 200))` base points evenly from 12 % to 88 % of the side;
  - sample a profile at integer offsets −half..half along n with bilinear interpolation (edge replicate);
  - central difference `g = (prof[k−1] − prof[k+1])/2` (brightness falling outward is positive); peak index k = argmax
    clipped to [1, len − 2]; g0, g1, g2 at k−1, k, k+1; `den = g0 − 2g1 + g2`;
  - keep rows with `den < 0`, `g1 ≥ g0`, `g1 ≥ g2` and `g1 > 0.35 · max(P75(g1), 0)` (the upper quartile stands for the
    uncovered edge even when more than half of the side is covered);
  - sub-pixel offset `k + 0.5(g0 − g2)/den`; edge width estimate `step/(√(2π)·(g1 − (g0 − g2)²/(8·den)))` with step =
    mean of the first two profile samples − mean of the last two;
  - return points `base + offset·n`, the sampled count, widths.
- `need = max(8, count // 5)`. With ≥ need points, robust line: Huber line fit; residuals r; robust scatter =
  1.4826·median(r); reject the side if scatter > half/4; inliers r ≤ max(3·scatter, 0.5); refit by total least squares
  (centroid, principal direction). Reject the quad if a side has no fit, fewer than `need` inliers, or inliers whose
  projection span along the side is < 0.5·(1 − 2·0.12)·|side|² (i.e. less than half the sampled stretch).
- Corner i = intersection of side i−1 and side i lines. Reject if any is null, the corners are not convex clockwise, or
  `area(corners)/area(quad) ∉ (0.8, 1.25)`.
- Result: `{corners, edge_pts (inliers per side), rms_px (perpendicular residuals of all inliers), blur =
  √max(median(widths)² − 1.5, 0), area}`. (1.5 px² = 1 px pre-blur² + 1/3 central difference + 1/6 bilinear
  sampling; it reads 0.67, 1.05, 2.02, 2.94 px for true 0.67, 1.04, 2.02, 3.01 px.)
- Fits whose corners are within 2 px (max corner distance) of an earlier fit are dropped.

Step 3, rectangle residual (`rect_residual(corners, size_mm, image_wh)`):
- Centre the corners on the image centre `(wh − 1)/2`; G = exact homography sheet_obj(size) → centred corners;
  a = G column 0, b = G column 1.
- For focal f: `aa = (a0² + a1²)/f² + a2²`, `bb` likewise, `ab = (a0 b0 + a1 b1)/f² + a2 b2`; residual(f) =
  `√(ab²/(aa·bb) + (½ ln(aa/bb))²)` (cosine of the angle between K⁻¹h1 and K⁻¹h2, and ln of their length ratio).
- Search f over `diag · geomspace(0.3, 10, 71)` (diag = image diagonal), then refine over `geomspace(f_{i−1}, f_{i+1},
  41)` around the coarse minimum (indices clamped). Return (min residual, its f). With no perspective f drops out and
  the residual is `|ln(aspect seen / aspect assumed)|` (Letter vs A4 square-on: ln 1.0929 ≈ 0.0889).
- `paper_fit(corners, paper)`: evaluate portrait (w, h) and landscape (h, w) sizes, keep the lower residual (portrait
  means the first quad side, top edge, is the short one); H = exact homography corners → sheet_obj(size) (px → sheet
  mm); scale = plane_scale(H, size).

Step 4, selection and verdict (`analyze_plain_sheet(image, paper = "auto")`, paper ∈ letter | a4 | auto, else
`paper must be letter, a4 or auto`):
- Report skeleton `{schema, image_size_px, reference: "plain_sheet", paper: null if auto else paper, paper_requested,
  warnings, notes: ["Plain sheet: four corners give the homography with no redundancy, so the rectangle check is the
  only evidence. The sheet must be flat (taped to card or a clipboard), fully visible, and on a background darker than
  the paper.", "Sheet size assumed nominal. Scale is exact only on the sheet's plane (03 §3.1)."]}`; `candidates` = number
  of refined fits.
- No fits → `ok: false`, `SHEET_NOT_FOUND: no bright four-sided shape with straight edges lies wholly inside the frame
  — show the whole sheet, flat, on a background darker than the paper`.
- asked = both papers for auto, else the named one; score(fit) = min residual over asked papers. Passing = score ≤
  RECT_OK. If any pass: among passing fits with area ≥ ½ the largest passing area, the lowest score. Otherwise the fit
  with the lowest residual over both papers.
- best = asked paper with the lowest residual on that fit; other = the other paper. Report `paper, orientation,
  corners_px (round2), edge_fit_rms_px (round4), edge_points (total inliers), rect_residual (round5),
  rect_residual_by_paper (round5 each)`.
- None passing → `ok: false` and either `ASPECT_MISMATCH: asked for <Lb> but the sheet fits <Lo> (rect_residual
  <best:.3f> vs <other:.3f>) — pass --paper <other>` when a paper was named and other ≤ min(RECT_OK, best/2), or else
  `NOT_A_SHEET: the best rectangle misses <Lb> by <best:.3f> (limit 0.025) — probably a window, door or book; or the
  sheet is bent, has an edge covered (tape along it), or is tilted in a cropped photo`.
- `both` = other's residual ≤ RECT_OK. auto and both → `ok: false`, `paper: null`, `PAPER_AMBIGUOUS: fits <Lb> tilted
  ≈<tilt:.0f>° and <Lo> tilted ≈<tilt_other:.0f>° (rect_residual_by_paper) — four corners cannot tell a tilted sheet
  of one from the other; pass --paper letter or --paper a4`. (A sheet tilted about one axis fits the other paper
  exactly at another focal length.)
- Accepted: `ok: true, homography_px_to_board_mm, board_centre_px, mm_per_px, px_per_mm, mm_per_px_axes,
  tilt_deg_estimate, scale_variation_across_board, focal_px_estimate (round1, only when variation ≥ 0.02 else null),
  blur_sigma_px_estimate (round2)`. Warnings in order: more than one passing fit → `SEVERAL_SHEETS: <n> rectangles
  pass as <Lb>; used the one centred at (<x:.0f>, <y:.0f>) px — check the annotated image`; both and tilt −
  tilt_other > 10 → `PAPER_AMBIGUOUS: as <Lb> the sheet is ≈<tilt:.0f>° off square, as <Lo> only
  ≈<tilt_other:.0f>° — four corners cannot tell them apart; check which paper it is` (the named paper is still used);
  then scale_warnings with `rms_mm = rms_px · mmpp`.
- Context: H, edge points per side, size_mm, σ_point = rms_mm.

Known limits (documented behaviour, not bugs to fix silently): a white card ~10 mm larger behind the sheet passes as
the sheet (scale off ~7.6 %); dark tape over ~4 mm of a whole edge shifts scale ~0.7 %; a plain sheet needs ~25 grey
levels of contrast with its background.

### A.12 `measure_photo` and its σ model

`measure_photo(image, sheet_path | null, points, depth_offset_mm = 0, camera_distance_mm = 2500, click_px = 3, view =
null, points_from_width = null, sheet_level_sigma_deg = 1.5, bootstrap = 200, seed = 0, paper = null)`:
- Exactly one of sheet sidecar / paper, else `give either the calibration sheet sidecar (--sheet) or a plain sheet
  (--paper), not both`. Analyze; not ok → raise with the warnings joined by `; ` (CLI exit 1).
- Bootstrap homographies (B = 200, seeded RNG): ChArUco: resample the inlier corners with replacement, skip draws with
  fewer than 6 unique points, least-squares H. Plain sheet: per side resample its edge points with replacement, TLS
  line fit, intersect into corners (skip if any null), perturb the sheet size by N(0, 0.5 mm) on each axis, exact H.
- `points` JSON: key `name[@tag]` → either `[[x1, y1], [x2, y2]]` or `{points, view, depth_offset_mm,
  definition_id}`. Pixel coordinates are full-resolution, EXIF-rotated; when `points_from_width` is given every
  coordinate is multiplied by `image width / points_from_width`. Exactly two points, else `<key>: need exactly two [x,
  y] points`; coincident mapped points → `<key>: the two points coincide`.
- Map both points through H (mm on the sheet plane); J1, J2 = Jacobians at the two pixels; `dist`, unit u, normal
  `n = (−u.y, u.x)`. Method prefix `photo <view or ?> <ref> sheet-plane homography` + ` [<tag>]` when tagged, ref =
  `ChArUco` | `plain Letter` | `plain A4`.
- Angle names (catalogue unit deg or name ending `_deg`): value = `degrees(atan2(|Δy|, |Δx|))` in the sheet plane
  (0–90°, "below horizontal"); homography term = std of the bootstrap angles (population std, 0 if < 2 draws); click
  term = `degrees(click_px · √(nᵀJ1J1ᵀn + nᵀJ2J2ᵀn) / dist)`; σ = √(sa² + sc² + sheet_level²). Record value and σ
  rounded to 2 decimals, method + `; angle below horizontal in the sheet plane (sheet assumed upright)`, notes `σ terms
  deg: homography <sa:.2f>, click <sc:.2f> (<click_px:g> px), sheet level <lvl:g>`, `sigma_terms_deg: {homography
  (round3), click (round3), sheet_level}`.
- Distances: value = dist; homography term sa = std of bootstrap distances; depth term `sb = dist · |Δz| / Z0` (Δz =
  per-point depth_offset_mm or the default, Z0 = camera distance); click term `sc = click_px · √(uᵀJ1J1ᵀu +
  uᵀJ2J2ᵀu)`; σ = √(sa² + sb² + sc²). Value and σ rounded to 1 decimal; method + `; sheet-plane distance`; notes `σ
  terms mm: homography <sa:.2f>, depth <sb:.2f> (Δz <dz:g> at Z₀ <Z0:g>), click <sc:.2f> (<click_px:g> px)`;
  `sigma_terms_mm: {homography, depth_offset, click}` (round3).
- Every record is `PHOTO_ESTIMATED` (definition_id from the spec object or the catalogue default) with `points_px`
  (scaled, round2); non-catalogue names get notes += `; name not in the catalogue`.
- Output = a new empty measurement file plus `photo` (the scale report) and the records; `photo.measure_warnings` =
  [`depth offset 0 assumed: valid only for landmarks in the sheet's plane (sheet beside the child, centre over the same
  floor line; 03 §3.1). Otherwise pass --depth-offset-mm.`] when the *default* depth offset is 0 (per-point overrides do
  not clear it) **[quirk]**, else [].
- Photo widths/depths become girths only through `girth-from-section`; a photo never replaces a tape girth (its σ is
  far larger, so fusion and readiness keep it secondary).

### A.13 `annotate`

Visual check image (not a conformance target beyond colours): grey image converted to colour; dot radius `r =
max(3, round(min(h, w)/400))`. ChArUco: detected corners as filled green dots (BGR 0,200,0). Plain sheet: the quad as a
closed polyline, green when the report is ok, orange (BGR 0,140,255) when found but rejected, thickness max(1, r//2),
plus green corner dots. Measured records: red (BGR 0,0,255) segment between `points_px`, label `<name> <value>` in a
Hershey simplex font, scale r/7, thickness max(1, r//5).

### A.14 CLI (`measurements.py <subcommand>`)

All JSON output is pretty-printed (indent 2, non-ASCII kept) to `--out` (parents created, `wrote <path>` on stderr) or
stdout. Exit codes: 0 ok; 1 bad input (value/key/file/JSON errors, printed as `error: <message>` on stderr); 3 gate
not ready or photo not usable; argument errors use the parser's own exit (2).

| subcommand | arguments (defaults) | behaviour |
|---|---|---|
| `catalogue` | `--garment` (kind or alias), `--out` | all definitions keyed by name `{unit, kind, definition_id, iso_clause, definition, how, tape_sigma, target_sigma, view, notes, equivalent_ids, normalization_hooks: {id: [offset, extra, reason]}}`, or for a garment `{garment, must, should, basis, targets_override}` |
| `prior` | `--stature-mm` (required), `--age-months`, `--sex`, `--out` | priors_for output |
| `init` | `--out` (req), `--label` (req, a nickname), `--age-months`, `--sex`, `--stature` (with unit), `--taken-on` (default today), `--nappies` | new file; with stature also a `MEASURED_TAPE` stature record (σ = tape σ 10, method `wall + book (init)`) and `wearer.stature_mm` |
| `add FILE` | `--name` (req), `--value` (repeatable, with unit), `--sigma` (with unit), `--provenance` (default MEASURED_TAPE), `--method`, `--definition-id`, `--notes`, `--out` (default: update FILE) | one record per value; σ = `--sigma`, else catalogue tape σ for MEASURED_TAPE, else provenance default for mm units, else error `give --sigma for <name> (<provenance> has no default)` |
| `validate FILE` | | `{file, valid, errors}`; exit 1 when invalid |
| `fuse FILES…` | `--no-priors`, `--out` | merge files (first is the base; a base wearer with all-null fields takes the next file's wearer; records concatenated; taken_on filled from the first that has one; `photo` dropped), then fuse |

| `ready FILES…` | `--garment` (req), `--passes-hip`, `--closed-cuff`, `--nappies` (true or "use the wearer flag"), `--elastic-waist`, `--cuff-level` knee/calf/ankle (calf), `--cuff-opening`, `--as-of`, `--no-priors`, `--out` | readiness; exit 3 unless READY/READY_REFERENCE |
| `girth-from-section` | `--level` (waist), `--width-mm`, `--depth-mm`, `--sigma-width-mm` 0, `--sigma-depth-mm` 0, `--from FILE`, `--k`, `--k-sigma`, `--k-ratio` 1, `--k-ratio-sigma` 0, `--clothing-mm` 0, `--tape-girth-mm`, `--sigma-tape-mm` 10, `--name`, `--out` | `--from` reads `<level>_width` and `<level>_depth` records (`<file>: no <key> record` if absent); missing w or d → `give --width-mm and --depth-mm, or --from FILE`; with `--tape-girth-mm` output calibrate_k; else girth_from_section plus, when a name is known (`--name` or a K_SECTION level), a `PHOTO_ESTIMATED` `record` with method `girth-from-section k·Ramanujan(width <w:g>, depth <d:g>), k=<k>` |
| `calib-sheet` | `--paper` letter/a4 (letter), `--out` dir (req), `--square-mm` 22, `--marker-mm` 16, `--dictionary` DICT_4X4_50 | writes the three files; prints the sidecar |
| `photo-scale` | `--image` (req), exactly one of `--sheet` / `--paper` (letter, a4, auto), `--annotate`, `--out` | scale report (+ `image`, and `sheet` for ChArUco); exit 3 unless ok |
| `measure-photo` | `--image`, `--sheet`/`--paper`, `--points` (req), `--depth-offset-mm` 0, `--camera-distance-mm` 2500, `--click-px` 3, `--view`, `--points-from-width`, `--sheet-level-sigma-deg` 1.5, `--annotate`, `--out` | measurement file with PHOTO_ESTIMATED records |
| `reference` | `--stature-mm` (req), `--age-months`, `--sex`, `--label`, `--out` | reference wearer |

Browser extension: a tape/file body request may supply `supplemental_measurements`, an array of
at most eight original UTF-8 `fabric-measurements/1` document strings (16 MiB combined), and
`supplemental_same_wearer: true`. The explicit assertion associates all supplied records with the
primary wearer; source wearer metadata never silently replaces that wearer. Chart/demo bodies and
reference-wearer documents refuse this combination. Every source is validated before its records
are appended, with at most 4096 combined records; duplicate source strings refuse. Recompute ordinary
fusion from the combined original records, ignoring every cached `fused` value. Preserve the exact
supplemental text in the saved measurements' `measurement_imports` array, including photo source and
uncertainty reports. Existing imports are retained, not re-applied as new observations.
For this mixed-source extension, `taken_on` is the oldest input date only if every input has a known
date, otherwise null. The original primary date is retained in `measurement_primary_taken_on`.
This conservative aggregate cannot establish per-record freshness; source dates
remain inspectable. No provenance, sigma, definition, conflict or readiness rule changes. Source/body
edits withdraw outputs and wearer consent. Evidence import is local and never enters browser storage.

Browser body-check extension: after a round saves its current measurements, the same verified Anny
model can be checked without a pattern placement or cloth simulation. The ordinary path uses the
existing tape fitter at its standing drape pose and retains raw target/body/signed residual mm, supplied
weight residual kg, iterations and all predicted readings. An explicitly associated accepted image-fit
report instead uses the exact-shape importer, never a fallback tape fit; current targets are compared
without changing that shape. Record-bearing inputs are validated and re-fused. Measurement text and
accepted-report text are limited to 16 MiB each; the result binds exact input/model digests.
The `ftx-body-check/1` result contains `body`, `measurements_sha256` and `cloth_simulated: false`.
Predictions remain MODEL_FIT, unavailable anatomy stays unavailable, and no readiness or physical-fit
approval is added. This is separate from drape flags and exports. Round, model or evidence replacement
withdraws the check's display/download; superseded requests cannot publish. A flat-only garment can
use this check even though folding remains unavailable.

---

## Part B: Body frames, landmarks and the analytic form (`body.py`)

Coordinates: Blender convention, mm. +Z up; the wearer faces −Y; the wearer's right is −X. Around any part axis,
θ = 0 is the front; +90° is the frame's `lat` direction (+X for the torso = wearer's left). A viewer outside the
surface sees θ increase to their right, which is why pieces drafted public-side-up wrap with x increasing → θ
increasing.

Constants: `A_POSE_DEG = 35` (arm abduction from vertical); `ANATOMICAL = false` (experimental form, see B.5 and
Part E); `LEG_SPREAD_DEG = 4`; `THIGH_GAP_MM = 18`; `RING_SEGMENTS = 48`.

### B.1 `mval(meas, name)`

If the object has `fused`, return `fused[name].value_mm` (or `value_deg`); otherwise treat it as a flat name → value
map. A missing name is an error (KeyError); drafting relies on that to fail loudly.

### B.2 Landmarks (`landmarks(meas)`, heights in mm above the floor)

Inputs: waist_height, back_neck_to_waist, shoulder_slope_deg, neck_base, shoulder_length, armscye_depth,
crotch_height, head, stature, knee_height, hip_width, arm_length (all required).

| key | formula |
|---|---|
| floor | 0 |
| ankle | 55 (constant) |
| calf | 55 + 0.6 · (knee_height − 55) |
| knee | knee_height |
| crotch | crotch_height |
| hip | crotch + 0.45 · (waist − crotch) |
| waist | waist_height |
| chest | c7 − armscye_depth |
| across_back | ½ (chest + shoulder) |
| shoulder | snp − shoulder_length · sin(slope) |
| snp | c7 − 5 (default form); c7 + 12 when ANATOMICAL |
| c7 | waist + back_neck_to_waist |
| head_top | stature |
| head_centre | stature − head_r |
| neck_r | neck_base / 2π |
| head_r | head / 2π |
| shoulder_half_width | neck_r + shoulder_length · cos(slope) |
| hip_joint_x | hip_width / 4 |
| arm_length | arm_length |

(slope = radians(shoulder_slope_deg).)

### B.3 Torso sections and the crotch ring

`torso_sections(meas)` → list of `(z, a = half width, b = half depth, centre_y)`; every section's perimeter equals
the tape girth at that level (Ramanujan), and all sections share one back plane `back_y = chest_depth / 2`, so
`centre_y = back_y − b` (a deeper belly protrudes forward, toward −Y).

`sec(z, girth, width)`: `a = width/2`, `b = depth_for_perimeter(girth, a)`.
`depth_for_perimeter(P, a)` (body's own version): `lo = 1, hi = max(a, P/2)`; if Ramanujan(a, 1) > P return 1; else
80 bisection steps (`Ramanujan(a, mid) < P` → lo = mid); return the midpoint. Never raises.

Default (non-anatomical) order:
1. hip: `sec(L.hip, hip, hip_width)`;
2. waist: `sec(L.waist, waist, waist_width)`;
3. crotch ring inserted at index 0 (below);
4. chest: `sec(L.chest, chest, chest_width)`;
5. across-back: `sec(L.across_back, 0.97·chest, 0.98·chest_width)`;
6. shoulder: `sec(L.shoulder − 15, 0.80·chest, 2·shoulder_half_width − 30)`;
7. neck: `sec(L.snp + 5, 2π·neck_r·1.6, 2·neck_r·2.2)`;
8. the last two get `centre_y = chest centre_y + 15` (shoulders and neck over the chest centre, slightly back).

ANATOMICAL replaces 6–8 by a shoulder roof: `nb_r = neck_r + 8`; sections `(shoulder, shoulder_half_width − 4, 0.22 ·
chest_depth)`, `(½(shoulder + snp), ½(shoulder_half_width + nb_r), 0.28 · chest_depth)`, `(snp, nb_r, 0.95·nb_r)`, all
centred at chest centre_y + 15.

Crotch ring (solves the form's crotch path to the measured total crotch length):
- `z1 = crotch + min(25, ½(z_hip − crotch))`; ring(b) = `(z1, 0.92·a_hip, b, centre_y_hip)`.
- Sagittal path (x = 0 centre line, y–z plane) through: front waist `(cy_w − b_w, z_w)`, front hip `(cy_h − b_h, z_h)`,
  front ring `(cy_1 − b, z1)`, crotch point `(cy_1, crotch)`, back ring `(cy_1 + b, z1)`, back hip `(cy_h + b_h, z_h)`,
  back waist `(cy_w + b_w, z_w)`; length = sum of the 6 segment lengths.
- `lo = b_h · (z1 − crotch)/(z_h − crotch)` (straight hip → crotch, the shortest path). If total_crotch_length is
  absent, use b = 0.6·b_h. If path(lo) ≥ target use lo (the form cannot be shorter; the simulator reports the excess).
  Else bisection on [lo, b_h], 40 steps, path < target → raise lo; return the midpoint.
- `crotch_path_mm(meas)` = path through sections 0 (ring), 1 (hip), 2 (waist). On the reference wearer it equals
  total_crotch_length within 0.5 mm; on wearers where the straight-line path is already longer, the excess is ≤ 15 mm
  (800/15: +11.8 mm; 1160/72: +9.6 mm).

### B.4 Part frames (`frames(meas)`)

Each frame: `origin`, `up` (pointing proximally), `front`, `lat = up × front`; limbs also carry extra scalars.
- torso: origin (0, 0, 0), up (0, 0, 1), front (0, −1, 0) → lat (1, 0, 0).
- arms (sx = −1 for R, +1 for L; a = 35°): `up_arm = upper_arm/2π`, `r_root = 1.15·up_arm`, `xj = shoulder_half_width −
  0.6·up_arm`, `up = (−sx·sin a, 0, cos a)` (wrist → shoulder), front (0, −1, 0).
  - default: origin `(sx·xj, 0, shoulder − 0.8·up_arm)`, `tip_h = 0.8·up_arm` (joint just under the shoulder tip; the
    form's armpit sits ~45 mm above the armscye level, a documented bias that affects only advisory sim flags).
  - ANATOMICAL: `t = max(0, (chest_width/2 − xj + r_root·cos a)/max(sin a, 1e−3))`; origin `(sx·xj, 0, chest + t·cos a +
    r_root·sin a)`; `tip_h = ((sx·shoulder_half_width, 0, shoulder) − origin)·up`.
- legs (b = 4°): origin `(sx·hip_joint_x, 0, crotch + 25)`, `up = (−sx·sin b, 0, cos b)`, front (0, −1, 0),
  `length = (origin.z − ankle)/cos b`, `knee_d = (origin.z − knee)/cos b`.

### B.5 `landmark_h(meas, part, name)` and `section_radius(meas, part, h)`

landmark_h, the height coordinate along a part frame:
- torso: `L[name]` (z);
- arm: `shoulder_tip` → tip_h; `elbow` → tip_h − 0.55·arm_length; `wrist` → tip_h − arm_length (other names error);
- leg: `−(origin.z − L[name]) / up.z` (any landmark name);
- unknown part → error.

section_radius, the max distance from the part axis to the full-size form near h:
- torso: interpolate a, b, centre_y at z = h across the section z's (clamped interpolation); return the max over 96
  samples t ∈ [0, 2π] (inclusive linspace) of `hypot(a·sin t, cy − b·cos t)`.
- arm: `1.15 · upper_arm / 2π` (constant).
- leg: `r = 1.05 · thigh / 2π`; `z = origin.z + h·up.z`; if `z > origin.z − 20` (at or above the leg root): interpolate
  torso a, b, cy at z; `xj = |origin.x|`; `reach = |cy| + b·√max(0, 1 − (xj/a)²)` if a > xj else b; `r = max(r, reach,
  a − xj)`. Return r.
- other part → error.

### B.6 Analytic dress form mesh (`dress_form(meas, inset_mm = 0, slim = 1)`)

To be replaced by the Anny body model (Part E); specified here for compatibility and for the existing tests.
Returns parts `{torso, head, arm_R, arm_L, leg_R, leg_L}`, each `{verts (n, 3), faces}`; `inset_mm` shrinks every
section (collision compensation: the cloth solver keeps thickness_outer + distance_min outside the collider, which
would otherwise add 2π·offset to every girth); `slim` is a radial scale.
- Ring(cx, cy, z, a, b): 48 points, `t = 2πi/48`, `(cx + a·sin t, cy − b·cos t, z)` (t = 0 at the front).
- Tube(rings, cap_bottom, cap_top): quads `(a, b, b + n, a + n)` between consecutive rings; bottom cap triangles
  `((i+1)%n, i, c)`; top cap triangles `(base + i, base + (i+1)%n, c)`.
- Torso: one ring per section `(0, cy·slim, z, max(a·slim − inset, 3), max(b·slim − inset, 3))`, plus two neck rings at
  z = snp + 25 and c7 + 55 with radius `max(neck_r·slim − inset, 3)` centred at `neck_cy` = last section centre_y ·
  slim; bottom cap vertex `(0, cy_ring·slim, crotch + inset)`, top cap `(0, neck_cy, c7 + 60)`. Reference: 434 vertices,
  480 faces, z from 372.5 to 815.
- Head: UV sphere radius `max(head_r·slim − inset, 5)` at `(0, neck_cy, head_centre)`, 12 latitude bands × 48
  longitudes: bottom pole, 11 rings at φ = −π/2 + πi/12, top pole; bottom fan `(0, 1 + (j+1)%48, 1 + j)`, quads between
  rings, top fan. 530 vertices, 576 faces.
- Arms: stations (distance d along −up from the origin, girth): `(−12, 1.15·upper_arm)`, `(0.3·al − tip_h, upper_arm)`,
  `(0.55·al − tip_h, 1.05·½(upper_arm + wrist))`, `(al − tip_h, wrist)`, `(al + 45 − tip_h, 0.9·hand_pass)`; keep the
  first and those with d > 0; girth × slim when d > 0.25·al. Limb ring radius `max(girth/2π − inset, 2)`, centre
  `origin − up·d`, points `centre + r(sin t·lat + cos t·front)`. Caps at `origin + 20·up` and `origin − (al + 60 −
  tip_h)·up`. Faces reversed (rings run proximal → distal, opposite handedness to the torso) so normals point outward.
  242 vertices, 288 faces each.
- Legs: `root = min(thigh, 2π·(hip_joint_x − 9))` (keeps an 18 mm gap between the thigh roots); stations `(−10,
  1.02·root)`, `(0.15·ll, root)`, `(0.35·ll, ½(thigh + knee))`, `(knee_d, knee)`, `(0.75·ll, calf)`, `(ll, ankle)`,
  sorted by d, girths × slim; caps at `origin + 15·up` and `origin − (ll + 10)·up`; faces reversed. 290 vertices, 336
  faces each.
- Invariants (tests): every part has positive signed volume (outward faces); the torso's lowest vertex is exactly at
  the crotch height; the right leg's max x ≤ −7 mm.

### B.7 Placement helpers used by the simulator

A piece's `placement`: `{part: torso|arm|leg, theta_deg, x0, y0_landmark, y0 (default 0), clearance_mm (default 25),
radius_mm (optional), inferred (optional)}`. Limb parts always use the right-side frame (`arm_R`, `leg_R`); the
wearer's-left instance is placed by mirroring (below).

`ROLE_PARTS`: bodice_front, bodice_back, skirt_front, skirt_back → torso; sleeve → arm; pant_front, pant_back → leg.

`infer_placement(piece, meas, role = piece.role)` (for traced pieces without one; pieces are wearer's right, public
side up, mm); unknown or non-simulated roles (waistband, cuff, facing, …) → none. With xmin, xmax, ymax over the
piece's vertices:
- bodice_front: `{part torso, theta_deg −4.8, x0 xmax, y0_landmark waist, y0 round2(ymax − (snp + 10 − waist)),
  clearance_mm 20, inferred true}` (CF is the +x side; the highest point hangs at the neck point + 10 mm);
- bodice_back: same but `theta_deg 180, x0 xmin`;
- skirt_*/pant_*: part torso for skirts, leg for pants; `theta_deg` 0 for fronts, 180 for backs; `x0` = xmax (torso
  front), xmin (torso back), or the x mid-point (legs), round2; `y0_landmark waist, y0 round2(ymax), clearance_mm 25,
  inferred true`;
- sleeve: top = the highest vertex (first on ties); `{part arm, theta_deg −90, x0 round2(top.x), y0_landmark
  shoulder_tip, y0 round2(top.y), radius_mm round2(max((xmax − xmin)/2π, section_radius(arm_R) + 6)), inferred true}`.

`infer_placements(pattern, meas)` fills missing placements in place (skipping pieces that have one or have `sim:
false`) and returns one ledger note per decision: `<id>: placement inferred from role '<role>' (<part>, theta
<theta>)` or `<id>: no placement (role '<role>' is not simulated); excluded from the fold-up` (role printed in Python
repr style, `None` when absent).

`wrap(points2d, meas, placement, h_range = null)` → (points3d, {radius_mm, part}), an isometric wrap onto a cylinder
around the part axis:
- `h0 = landmark_h(part_key, y0_landmark) − y0`; per point `h = h0 + y`.
- R = `radius_mm` if given, else `max(section_radius(h) for 9 evenly spaced h over [min h, max h] or h_range) +
  clearance_mm (default 25)`.
- `θ = radians(theta_deg) + (x − x0)/R`; point = `origin + h·up + R(sin θ·lat + cos θ·front)`.

`roll(points2d, meas, placement, k)`: exact isometry between the flat piece (k = 0) and the wrap (k = 1) for
animation. k ≥ 1 returns the wrap. Otherwise `κ = max(k, 1e−6)/R`, `u = x − x0`, local `lx = sin(κu)/κ`, `ly = (1 −
cos(κu))/κ`; `flat` = wrap of the anchor column points `(x0, y)`; `θ0 = radians(theta_deg)`; `tangent = cos θ0·lat −
sin θ0·front`, `normal_in = −(sin θ0·lat + cos θ0·front)`; result `flat + lx·tangent + ly·normal_in`.

`mirror_x(p)`: negate the x component (last axis). The simulator places a mirrored (`.L`) instance by wrapping
`mirror_x` of its 2D points and mirroring the 3D result back; it also scales `x0` and `y0` by the piece's
`sim_scale` (specified in the simulator spec).

Tested invariants: wrap preserves arc length along the outline within 1 %; `roll(k = 1) == wrap`.

---

## Part C: Drafting programs (`drafting.py`)

### C.0 Common machinery

- Each program is `program(meas, style = null) → (pattern, fit_spec)`. Registry `PROGRAMS`: `zouave_bolero`,
  `zouave_trousers`, `fez`.
- Style merge: defaults overridden by the style object; any unknown key raises `unknown style key(s) [<sorted
  unknown>]; this program takes [<sorted defaults>]` (Python list repr). Types are not checked (except the fez ranges).
- `size_label` = `<stature:.0f> mm stature`, or `custom` when stature is missing.
- `mirror_seams(list)`: for each seam emit `{reverse: true, …seam, id: <id>_R}` (an explicit `reverse` in the seam
  wins); if side a or b contains `.R`, also emit the `_L` copy with every `.R` replaced by `.L` in a and b.
- `meta` common: `{title, size_label, origin: "DRAFT", round: 0, sa_mm, program: "drafting.<name>", style (the merged
  style), solved}`.
- CLI: `drafting.py PROGRAM --measurements M.json [--style S.json] --out DIR` writes `pattern.json` and
  `fit_spec.json` (JSON indent 1 + newline), prints `{pattern, fit_spec, structure_errors, solved}`, exits 1 if the
  pattern has structure errors, else 0.

Native CLI freshness exception: when `ftx draft --out`, tape-fitting `ftx body`, or `ftx drape` receives `records`,
it recomputes fusion through the same authority as workspace drafting and rounds before calling the
unchanged drafting/fitting kernels. Invalid records refuse before opening a body model or creating an
output directory; original input bytes still supply the reported measurements SHA-256.
The cached `fused` object cannot override changed records; invalid records refuse before output.
Flat and fused-only inputs retain their existing behavior. This deliberately strengthens the CLI
boundary rather than changing the reference kernel or its oracle fixtures. Evidence: at native
revision `7fa7a8e`, replacing the 920 mm source record with a synthetic 1021.655 mm target while retaining
the old cache still drafted a pattern labelled 920 mm; the next round re-fused the records and judged
a different body. `ftx-cli/tests/draft_workflow.rs` covers poisoned caches, workspace consistency,
unchanged source bytes and invalid-record refusal. At `1a3ebbd`, standalone body and ordinary drape
still pass raw cached values to fitting, so record edits can put the draft and collider on different
bodies. They now share this CLI boundary; `body_model_fit.rs` compares fresh and poisoned-cache body
and drape outputs on the pinned model. This freshness check changes no provenance or
measurement-readiness classification, and fused-only callers explicitly retain their supplied targets.

Fit-spec semantics (evaluated by validation, summarized): `girth` = sum over instances of horizontal_width of the
instance outline (120 samples/edge) at `landmark_h(landmark) − landmark_h(y0_landmark) + y0 + dy` in piece y;
`width` = horizontal_width of one instance at a vertex's y (or a landmark level) + dy; `vertical` = |y(from) −
y(to)|; `path` = sum of seam-side polyline lengths × `scale`. Body value = `mval(body) · body_scale + body_offset_mm`;
`ease = finished − body`; `target_mm = [lo, nominal, hi]`; status IN_BAND / LOW / HIGH.

Measurements read (all required unless noted):
- every program that calls landmarks: the 12 landmark inputs of B.2;
- bolero: + chest, neck_base, shoulder_length, back_neck_to_waist, across_back, upper_arm, arm_length, hand_pass,
  wrist;
- trousers: + hip (or seat_over_nappy when `nappy`), body_rise, the cuff-level girth (calf/knee/ankle), foot_pass,
  total_crotch_length, waist, thigh, chest_depth, hip_width, waist_width, chest, chest_width, upper_arm (frames and
  section_radius);
- fez: head only (stature optional, for the label).

### C.1 `zouave_bolero`: open front, curved cutaway, set-in sleeve, back cut on the fold

Style defaults (`BOLERO_DEFAULTS`):

| key | default | meaning |
|---|---|---|
| chest_ease | 100.0 | total chest ease (open jacket over a shirt; UNCERTAIN children's value) |
| hem_y | −10.0 | hem 10 mm below the waist line |
| armhole_drop | 12.0 | underarm point below the body chest (armscye) level |
| back_neck_rise | 12.0 | side neck point above centre-back neck |
| neck_ease | 5.0 | added to neck_base/6 for the neck width |
| front_neck_extra | 12.0 | added to neck_base/6 for the front neck depth |
| neckline_ease | 50.0 | finished neckline over neck base (solved by the neck scale) |
| back_slope_deg | 16.0 | back shoulder slope |
| front_slope_deg | 20.0 | front shoulder slope |
| back_shoulder_ease | 4.0 | back shoulder longer than the front (eased in) |
| across_back_ease | 25.0 | solved via the back armhole scoop |
| front_gap_mm | 10.0 | each front edge this far from CF at the neck |
| front_cutaway | 45.0 | how far the curved front hem swings back from CF |
| front_belly_mm | 0.0 | extra front length at the hem, tapering to 0 at the side seam |
| bicep_ease | 50.0 | sleeve width over upper arm |
| cap_ease | 0.045 | cap length / armhole length − 1 (UNCERTAIN: 0.03–0.06 toddler) |
| sleeve_extra | 15.0 | sleeve length past the wrist bone |
| cuff_ease | 25.0 | sleeve hem over hand pass (closed cuff) |
| soutache | true | draft the braid layout (decor) |
| soutache_inset | 12.0 | braid centre line inside finished edges |
| soutache_width | 3.0 | flat braid width |
| sleeve_band | 35.0 | braid band above the sleeve hem |
| motif | 0.035 | trefoil radius / chest girth (0 = no motifs; back pair is 1.2×) |

Derived quantities (L = landmarks):
- `q = (chest + chest_ease)/4`; back width `wb = q + 5`; front width `wf = q − 5`.
- `y_ua = (L.chest − L.waist) − armhole_drop`; `hem = hem_y`; `y_cbn = back_neck_to_waist`; `y_snp = y_cbn +
  back_neck_rise`.
- `neck_target = neck_base + neckline_ease`; `y_ab = L.across_back − L.waist`; `ab_target = across_back +
  across_back_ease`; `bsl = shoulder_length + back_shoulder_ease`.

Bodice for neck scale k and back-armhole scoop σs (`bodice(k, scoop)`); `neck_w = k·(neck_base/6 + neck_ease)`:

Back (id `back`, name `BACK`, material shell, cut `{qty 1, on_fold "cb"}`, role `bodice_back`, grain [[20, 0], [20,
150]]):
- vertices: `cb_hem (0, hem)`, `side_hem (wb − 5, hem)`, `ua (wb, y_ua)`, `tip = (neck_w + bsl·cos βb, y_snp − bsl·sin
  βb)` (βb = back slope), `snp (neck_w, y_snp)`, `cbn (0, y_cbn)`.
- edges: `hem` cb_hem→side_hem (line); `side` side_hem→ua (line); `armhole` ua→tip cubic; `shoulder` tip→snp (line);
  `neck` snp→cbn cubic; `cb` cbn→cb_hem (line, the fold).
- armhole cubic: `sh = unit(snp − tip)`; `arr = (sh.y, −sh.x)` flipped to have y ≥ 0; `depth = tip.y − y_ua`;
  `c1 = (wb − scoop·(wb − tip.x + 40), y_ua)`; `c2 = tip − 0.42·depth·arr`.
- neck cubic: `leave = (−sh.y, sh.x)` flipped to have y ≤ 0 (perpendicular to the shoulder at snp); `c1 = snp +
  0.35·neck_w·leave`; `c2 = (0.45·neck_w, y_cbn)` (leaves the shoulder at 90° and stays perpendicular for a while; a
  short arm hooks the curve).
- notch `n_back_arm` on armhole at s 0.45, type `double`.
- placement `{part torso, theta_deg 180, x0 0, y0_landmark waist, clearance_mm 20}`.

Front, wearer's right (id `front`, name `FRONT`, material shell, cut `{qty 2}`, role `bodice_front`, grain [[wf −
40, 0], [wf − 40, 150]]); side seam at x = 0, CF toward +x:
- `cfx = wf − front_gap_mm`; `snp_f = (wf − neck_w + 3, y_snp)`; `tip_f = snp_f + shoulder_length·(−cos βf, −sin βf)`;
  `y_cfn = y_snp − k·(neck_base/6 + front_neck_extra)`; `hem_f = hem − front_belly_mm`.
- vertices (insertion order): `side_hem (5, hem)`, `front_hem (cfx − front_cutaway, hem_f)`, `cfn (cfx, y_cfn)`, `snp
  snp_f`, `tip tip_f`, `ua (0, y_ua)`.
- edges: `hem` side_hem→front_hem: a line when front_belly_mm = 0; otherwise a cubic with `x0 = 5`, `x1 = cfx −
  cutaway`, `c1 = (x0 + 0.45(x1 − x0), hem)`, `c2 = (x1 − 0.35(x1 − x0), hem_f)` (leaves the side seam square). `front_edge`
  front_hem→cfn cubic `c1 = (cfx − 0.1·cutaway, hem_f + 8)`, `c2 = (cfx, y_cfn − 0.55(y_cfn − hem_f))`. `neck` cfn→snp
  cubic with `sh = unit(tip_f − snp_f)`, `into = (sh.y, −sh.x)` flipped to y ≥ 0, `c1 = (cfx − 0.55(cfx − snp_f.x),
  y_cfn)`, `c2 = snp_f − 0.45(y_snp − y_cfn)·into`. `shoulder` snp→tip (line). `armhole` tip→ua cubic with `down =
  (−sh.y, sh.x)` flipped to y ≤ 0, `depth = tip_f.y − y_ua`, `c1 = tip_f + 0.42·depth·down`, `c2 = (0.30·(tip_f.x + 45),
  y_ua)`. `side` ua→side_hem (line).
- notch `n_front_arm` on armhole at s 0.55, type `V`.
- placement `{part torso, theta_deg −degrees(front_gap_mm/120), x0 cfx, y0_landmark waist, clearance_mm 20}`
  (**[quirk]** the gap is read as an arc on a 120 mm radius: −4.7746° by default; `infer_placement` uses a fixed −4.8).

Solves (both by C.0 bisection):
- Neckline length N(k, scoop) = 2·(front.R `neck` + back `neck.R`) edge lengths on the built pattern (back unfolded).
- Across-back width A(k, scoop) = horizontal_width of the unfolded back instance outline (120 samples/edge) at y_ab.
- Start k = 1.0, scoop = 0.32; two sweeps of: `k = bisect(N(·, scoop) − neck_target, 0.6, 2.2)`; `scoop =
  bisect(A(k, ·) − ab_target, 0.05, 1.2)`. Build the final bodice with (k, scoop).
- Armhole lengths `arm_f` = front.R `armhole`, `arm_b` = back `armhole.R` (pattern edge lengths).

Sleeve (id `sleeve`, name `SLEEVE`, material shell, cut `{qty 2}`, role `sleeve`):
- `bic = upper_arm + bicep_ease`; `slen = arm_length + sleeve_extra`; `hem_w = max(hand_pass + cuff_ease, wrist +
  60)`; `target = (arm_f + arm_b)·(1 + cap_ease)`.
- caps(hc, xt): `uf = (bic/2, −hc)`, `ub = (−bic/2, −hc)`, `top = (xt, 0)`; `wf' = bic/2 − xt`, `wb' = bic/2 + xt`;
  `d_f = unit(uf − (hem_w/2, −slen))`, `d_b = unit(ub − (−hem_w/2, −slen))`; front cap `(uf, cf1 = uf + 0.30·wf'·(−d_f.y,
  d_f.x), cf2 = top + (0.42·wf', 0), top)`; back cap `(top, cb1 = top + (−0.48·wb', 0), cb2 = ub + 0.26·wb'·(d_b.y,
  −d_b.x), ub)` (each half leaves its underarm at 90° to the tapered underarm seam; both meet the top horizontally).
- `solve_hc(xt) = bisect(len(front cap) + len(back cap) − target, 5, 0.9·bic)` (400-sample cubic lengths).
- `imbalance(xt) = len(front cap)/arm_f − len(back cap)/arm_b` at hc = solve_hc(xt); `xt = bisect(imbalance, −0.3·bic,
  0.3·bic)`; `hc = solve_hc(xt)`. Result: both halves carry exactly cap_ease (nested bisection).
- vertices: `hem_b (−hem_w/2, −slen)`, `hem_f (hem_w/2, −slen)`, `ua_f uf`, `top`, `ua_b ub`.
- edges: `hem` hem_b→hem_f cubic with `d_f = unit(uf − hem_f)`, `d_b = unit(ub − hem_b)`, `out_f = (d_f.y, −d_f.x)`,
  `out_b = (d_b.y, −d_b.x)`, `c1 = hem_b + 0.33·hem_w·out_b`, `c2 = hem_f − 0.33·hem_w·out_f` (hem perpendicular to both
  underarm seams); `under_f` hem_f→ua_f (line); `cap_f` ua_f→top (cf1, cf2); `cap_b` top→ua_b (cb1, cb2); `under_b`
  ua_b→hem_b (line).
- notches `n_cap_f` on cap_f s 0.45 `V`; `n_cap_b` on cap_b s 0.55 `double`.
- grain [[0, −hc − 20], [0, −slen + 30]].
- placement `{part arm, theta_deg −90, x0 round3(xt), y0_landmark shoulder_tip, radius_mm max(bic/2π,
  section_radius(arm_R, 0) + 6)}`.

Seams (through mirror_seams): `side` a `front.R/side`, b `back/side.R`; `shoulder` a `back/shoulder.R`, b
`front.R/shoulder`, ratio round4(bsl/shoulder_length), ratio_band [1.0, 1.08], kind `ease`; `armhole_f` a
`sleeve.R/cap_f`, b `front.R/armhole`, ratio_band [1.0, 1.08], kind ease (no nominal ratio); `armhole_b` a
`sleeve.R/cap_b`, b `back/armhole.R`, same band; `underarm` a `sleeve.R/under_f`, b `sleeve.R/under_b`. Each yields `_R`
and `_L` (10 seams).

Pattern: style `ZOUAVE_BOLERO`, units mm, meta title `Zouave bolero (open front, set-in sleeve)`, sa_mm 10; fabric
`{id shell, name "wool flannel, lined", kind woven, areal_g_m2 380, width_mm 1500, tensile_N_m 10000.0, bending_Nm
1.2e−4, sim_note "lined-jacket composite (UNCERTAIN)"}` (one simulated layer standing for shell + lining + interfaced
fronts); pieces in order back, front, sleeve. `meta.solved = {cap_height_mm: round1(hc), armhole_mm: round1(arm_f +
arm_b), cap_target_mm: round1(target), cap_top_offset_mm: round1(xt), neck_scale: round4(k)}`.

Fit spec `{schema fabric-fit/1, garment zouave_bolero, seam_tol_mm 3.0, truing_tol_deg 8.0, truing: [side_R start,
side_L end, shoulder_R start, shoulder_L end, underarm_R end, underarm_L start], points}`:

| id | kind | where | body | target [lo, nom, hi] | label |
|---|---|---|---|---|---|
| chest | girth | front.R, front.L, back at landmark chest, dy −(armhole_drop + 12) | chest | 70, 100, 130 | Chest ease (fronts edge to edge) |
| across_back | girth | back at landmark across_back | across_back | 10, 25, 45 | Across-back ease |
| bicep | width | sleeve.R at vertex ua_f | upper_arm | 35, 50, 70 | Bicep ease |
| sleeve_length | vertical | sleeve.R top → hem_f | arm_length | 0, 15, 35 | Sleeve length over arm length |
| cuff_pass | path | sleeve.R/hem | hand_pass | 10, 25, 60 | Sleeve hem passes the hand |
| neckline | path | front.R/neck, front.L/neck, back/neck.R, back/neck.L | neck_base | 30, 50, 90 | Neckline ease (open front) |
| back_length | vertical | back cbn → cb_hem | back_neck_to_waist | −10, 10, 30 | Back length vs nape-to-waist |

### C.2 Soutache layout (bolero decor)

Runs last, only when `soutache` is true; adds `decor` lists to front, back and sleeve. Each decor item is `{kind:
"soutache", pts (round2), width_mm, closed}`.

- Piece outline for this purpose: each edge sampled with 120 Bézier samples (straight edges are their 2 endpoints),
  concatenated dropping each edge's last point, in the piece's own drafted coordinates (half pieces, not instances).
- **Border** (`braid_border(piece, chain, inset)`): concatenate the chain's edge polylines (dropping the duplicate
  joint points); extend the start by `3·inset` along `pts[0] − pts[1]` and the end by `3·inset` along `pts[−1] −
  pts[−2]`; take the planar offset curve at +inset (left side = inside for a CCW loop) with mitre joins, mitre limit
  3.0; intersect with the piece polygon; keep the longest resulting line string (none if empty). The braid ends
  therefore lie on the seam lines, so they are caught in the seams.
  - front: chain `hem, front_edge, neck`, inset = soutache_inset (from the side seam at the hem to the shoulder seam);
  - back: chain `hem`, inset = soutache_inset;
  - sleeve: chain `hem`, inset = sleeve_band.
- **Motifs** (when motif > 0): `r = max(motif·chest, 6·width)` (loop tips turn on r/3, at least twice the braid
  width); `gap = 8`; `clear = inset + width + gap`.
  - trefoil(centre, r): 241 points for t = 2πi/240, i = 0..240: `x = cx + (r/3)(sin t + 2 sin 2t)`, `y = cy − (r/3)(cos t
    − 2 cos 2t)` (one stroke, one loop up, closed).
  - front: centre `front_hem + (−(clear + r), clear + r)`, radius r;
  - back: centre `(½·side_hem.x, cb_hem.y + clear + 1.2r)`, radius 1.2r (one each side of CB once unfolded);
  - a motif is kept only if it lies inside the piece polygon shrunk by `clear` (else omitted).
- decor per piece = [border (closed false)] if a border exists, then [motif (closed true)] if kept.

Invariants (tests, at 800/18 and 1160/72): front border ends within 0.05 mm of the side seam (start) and the shoulder
seam (end); away from its ends the border keeps the inset from the finished edges within 0.3 mm; the motif's max
radius / 3 ≥ 2·width − 0.05.

### C.3 `zouave_trousers`: full legs gathered into cuffs, elastic casing

Style defaults (`TROUSER_DEFAULTS`):

| key | default | meaning |
|---|---|---|
| seat_ease | 80.0 | seat ease before fullness |
| fullness | 60.0 | extra width per panel (Zouave bloom) |
| rise_ease | 15.0 | crotch depth below body rise |
| nappy | false | worn over a nappy (uses seat_over_nappy, +20 mm rise and crotch) |
| front_ext_frac | 0.065 | front crotch extension / seat |
| crotch_ease | 30.0 | total crotch length over the body |
| back_rise_extra | 20.0 | CB waist raised |
| cb_slant | 15.0 | CB waist moved in |
| cuff_height | 30.0 | finished cuff height (cut double) |
| cuff_level | "calf" | landmark/girth the cuff sits at (knee, calf, ankle) |
| cuff_ease | 25.0 | cuff over the cuff-level girth |
| leg_gather | 1.8 | leg hem / cuff length |
| blouse_drop | 40.0 | fabric that blouses over the cuff |
| casing_height | 30.0 | finished casing height (cut double) |
| elastic_frac | 0.95 | relaxed elastic / waist girth |
| elastic_overlap | 20.0 | added to the elastic cut length |

Derived quantities:
- `seat = seat_over_nappy if nappy else hip`; `quarter = (seat + seat_ease)/4`; front width `wf = quarter − 10 +
  fullness`; back width `wb = quarter + 10 + fullness`.
- `y_hip = L.hip − L.waist`; `y_cr = −(body_rise + rise_ease + (20 if nappy else 0))`; `ext_f = front_ext_frac·seat`.
- `cuff_body = mval(cuff_level)`; `cuff_len = max(cuff_body + cuff_ease, foot_pass + 15)` (pass-through first);
  `hem_total = cuff_len·leg_gather`; `hem_f = 0.47·hem_total`, `hem_b = 0.53·hem_total`.
- Leg length ends at the top of the cuff where it is worn, plus the blouse: `z_cuff_top = L[cuff_level] +
  cuff_height/2`; `y_hem = (z_cuff_top − L.waist) − blouse_drop`. (The first draft ran to the floor, ~150 mm too long.)
- Wrap radius: `h_waist = landmark_h(leg_R, waist)`; `r_wrap = max(section_radius(leg_R, h_waist + y) for 12 y
  evenly in [y_hem, 20]) + 22`; `mid_deg = degrees(asin(min(1, |leg_R origin.x| / r_wrap)))` (the angle of the body
  midline seen from the leg axis).

Front panel (id `tr_front`, name `TROUSER FRONT`, material trouser, cut `{qty 2}`, role `pant_front`):
- `crease = (wf + ext_f)/2`; vertices `hem_side (crease − hem_f/2, y_hem)`, `hem_in (crease + hem_f/2, y_hem)`,
  `crotch_pt (wf + ext_f, y_cr)`, `cf_hip (wf, y_hip)`, `waist_cf (wf, 0)`, `waist_side (0, 0)`.
- edges: `hem` hem_side→hem_in; `inseam` hem_in→crotch_pt; `crotch` crotch_pt→cf_hip cubic `c1 = (wf + 0.45·ext_f,
  y_cr)`, `c2 = (wf, y_hip − 0.45(y_hip − y_cr))`; `cf` cf_hip→waist_cf; `waist` waist_cf→waist_side; `outseam`
  waist_side→hem_side (all others lines).
- grain [[crease, y_hem + 60], [crease, y_cr − 20]]; placement `{part leg, theta_deg round2(mid_deg), x0 wf,
  y0_landmark waist, radius_mm round1(r_wrap)}` (CF lands on the body midline).

Back panel builder `back(ext_b, hem_dy = 0, hem_dx = 0)` (id `tr_back`, name `TROUSER BACK`, material trouser, cut
`{qty 2}`, role `pant_back`):
- `crease = (wb − ext_b)/2 + hem_dx`; vertices `hem_in (crease − hem_b/2, y_hem + hem_dy)`, `hem_side (crease +
  hem_b/2, y_hem + hem_dy)`, `waist_side (wb, 0)`, `waist_cb (cb_slant, back_rise_extra)`, `cb_hip (0, y_hip)`,
  `crotch_pt (−ext_b, y_cr)`.
- edges: `hem` hem_in→hem_side; `outseam` hem_side→waist_side; `waist` waist_side→waist_cb; `cb` waist_cb→cb_hip;
  `crotch` cb_hip→crotch_pt cubic with `cb_dir = unit((0, y_hip) − (cb_slant, back_rise_extra))`, `c1 = (0, y_hip) +
  0.5(y_hip − y_cr)·cb_dir`, `c2 = (−ext_b/2, y_cr)`; `inseam` crotch_pt→hem_in.
- grain [[crease, y_hem + 60], [crease, y_cr − 20]]; placement `{part leg, theta_deg round2(180 − mid_deg), x0 0,
  y0_landmark waist, radius_mm round1(r_wrap)}` (CB on the midline at the back).

Solves:
- `target_crotch = total_crotch_length + crotch_ease + (20 if nappy)`. `ext_b = bisect(front(crotch + cf) +
  back(cb + crotch) − target_crotch, 0.04·seat, 0.20·seat)` (instance `.R` edge lengths; back built with hem_dy =
  hem_dx = 0).
- Leg truing, front is the reference: `f_in`, `f_out` = front inseam and outseam lengths. Residual(v) = (back inseam −
  f_in, back outseam − f_out) for back(ext_b, v.dy, v.dx). Damped Newton from v = (0, 0), at most 25 iterations: stop
  when max|r| < 0.05 mm; Jacobian by forward differences with step 0.5 mm in each component; step = least-squares
  solve of `J·s = −r`; clip each component of s to [−40, 40]; v += s. (A fixed-point version diverged.)

Waist casing (id `waistband`, name `WAIST CASING`, material trouser, cut `{qty 1}`, role `waistband`):
- `wl_f`, `wl_b` = front and back waist edge lengths; `band = max(seat + 30, 2(wl_f + wl_b)/1.6)` (passes over the
  seat); `seg_b = band·wl_b/(2(wl_f + wl_b))`, `seg_f = band·wl_f/(2(wl_f + wl_b))`; `h = 2·casing_height` (cut double).
- `elastic = elastic_frac·waist`; `sim_scale = [round4(waist/band), 0.5]` (the fold-up draws the casing to the waist
  girth at its finished height; the cut piece stays full size; whether the elastic grips is judged flat by
  `elastic_hold`).
- note `Elastic <casing_height − 5:.0f> mm wide: cut <elastic + overlap:.0f> mm (includes <overlap:.0f> mm overlap)`.
- vertices `b0..b4` at x = cumulative [0, seg_b, seg_f, seg_f, seg_b], y 0; `t4 (band, h)`; `t0 (0, h)`.
- edges `bot_bR` b0→b1, `bot_fR` b1→b2, `bot_fL` b2→b3, `bot_bL` b3→b4, `end_r` b4→t4, `top` t4→t0, `end_l` t0→b0.
- grain [[40, h/2 − 10], [band − 40, h/2 − 10]]; internal `[{kind "draw", label "FOLD (elastic casing)", pts [[0,
  h/2], [band, h/2]]}]`; placement `{part torso, theta_deg 180, x0 0, y0_landmark waist, y0 0, clearance_mm 15}`.

Cuff (id `cuff`, name `LEG CUFF`, material trouser, cut `{qty 2}`, role `cuff`, sim_scale [1.0, 0.5]):
- `h = 2·cuff_height` (**[history]** an early version used the casing height); `hf_share = hem_f/hem_total`.
- vertices `b0 (0, 0)`, `b1 (cuff_len, 0)`, `t1 (cuff_len, h)`, `tm (cuff_len·hf_share, h)`, `t0 (0, h)`.
- edges `bottom` b0→b1, `end_r` b1→t1, `top_back` t1→tm, `top_front` tm→t0, `end_l` t0→b0 (the seam joins the top
  (cut) edge to the leg hem).
- grain [[20, h/4], [cuff_len − 20, h/4]]; internal `[{kind draw, label "FOLD", pts [[0, h/2], [cuff_len, h/2]]}]`;
  placement `{part leg, theta_deg −90, x0 0, y0_landmark cuff_level, y0 h/2, radius_mm round1(max(cuff_len/2π,
  cuff_body/2π + 6))}`.

Seams: mirror_seams over `crotch_front` (a `tr_front.R/crotch+cf`, b `tr_front.L/cf+crotch`), `inseam` (front.R/back.R
inseams), `outseam`, `cuff_front` (a `tr_front.R/hem`, b `cuff.R/top_front`, ratio round3(leg_gather), band [1.4,
2.4], kind gather), `cuff_back` (a `tr_back.R/hem`, b `cuff.R/top_back`, same), `cuff_end` (a `cuff.R/end_r`, b
`cuff.R/end_l`); then remove `crotch_front_L` (it would duplicate the R/L seam); append `crotch_back` (a
`tr_back.R/cb+crotch`, b `tr_back.L/crotch+cb`, reverse true); four casing seams `casing_bot_bR|bot_fR|bot_fL|bot_bL`
joining `tr_back.R|tr_front.R|tr_front.L|tr_back.L` `/waist` to `waistband/<edge>` with reverse true, ratio
round3(2(wl_f + wl_b)/band), band [1.0, 2.0], kind gather; and `casing_cb` (a `waistband/end_r`, b
`waistband/end_l`, reverse true). Total 17 seams.

Pattern: style `ZOUAVE_TROUSER`, title `Zouave full trousers (elastic casing, gathered into cuffs)`, sa_mm 10; fabric
`{id trouser, name "wool flannel", kind woven, areal_g_m2 280, width_mm 1500}`; pieces front, back, waistband, cuff.
`meta.solved = {back_crotch_ext_mm: round1(ext_b), crotch_target_mm: round1, band_mm: round1(band), cuff_mm:
round1(cuff_len), elastic_cut_mm: round1(elastic + overlap), leg_to_cuff_top_mm: round1(−y_hem)}`.

Fit spec `{schema fabric-fit/1, garment zouave_trousers, seam_tol_mm 3.0, truing_tol_deg 8.0, points}` (no explicit
truing list):

| id | kind | where | body | target | label |
|---|---|---|---|---|---|
| seat | girth | tr_front.R, tr_front.L, tr_back.R, tr_back.L at landmark hip | hip (seat_over_nappy if nappy) | 200, 320, 480 | Seat fullness (design ease) |
| crotch | path | tr_front.R/crotch, tr_front.R/cf, tr_back.R/cb, tr_back.R/crotch | total_crotch_length | 15, 30, 50 | Total crotch length ease |
| casing_pass | path | waistband bot_bR, bot_fR, bot_fL, bot_bL | hip (or seat_over_nappy) | 20, 40, 120 | Elastic casing passes over the seat |
| elastic_hold | path, scale round5(elastic/band) | same four edges | waist | round1(−0.12·waist), round1(−0.05·waist), −5.0 | Relaxed elastic vs waist (must be shorter to hold) |
| cuff_pass | path | cuff.R/bottom | foot_pass | 10, 25, 60 | Cuff passes over the foot |
| cuff_fit | path | cuff.R/bottom | cuff_level | 10, 25, 45 | Cuff ease at the <cuff_level> |
| leg_blouse | vertical, body_offset_mm −round1(z_cuff_top) | tr_front.R waist_side → hem_side | waist_height | 15, 40, 80 | Leg length blousing over the <cuff_level> cuff |

### C.4 `fez`: one-piece crown with a back seam, round tip

Style defaults (`FEZ_DEFAULTS`): `head_ease 10.0` (base edge over head girth; sits on the crown of the head),
`height_frac 0.62` (height / head diameter; UNCERTAIN, read off a reference photo), `top_frac 0.78` (top diameter /
base diameter). Validation: `top_frac ∈ [0.3, 0.98)` else `top_frac must be in [0.3, 0.98): a cylinder or a sharp cone
is not a fez`; `height_frac ∈ [0.2, 2]` else `height_frac (height / head diameter) must be in [0.2, 2]`; `head_ease ∈
[−10, 40]` else `head_ease must be in [-10, 40] mm`.

Closed form (the frustum is developable, so the flat pattern is the hat):
- `d_head = head/π`; `r_base = (head + head_ease)/2π`; `r_top = top_frac·r_base`; `height = height_frac·d_head`;
  `slant = hypot(height, r_base − r_top)`.
- Unrolled sector: `ρb = slant·r_base/(r_base − r_top)`, `ρt = ρb − slant`, `φ = 2π·r_base/ρb`. If φ > 0.9·2π raise
  `taper too strong for a one-piece crown: raise top_frac or the height`. `n = 2` if φ ≤ π else 4 (arcs of at most
  90°, and a centre-front vertex).
- angles `a_i = −π/2 − φ/2 + φ·i/n`, i = 0..n (centre front at −π/2, i.e. the bottom of the drawing).
- Arc control points (`arc_ctrl(c, ρ, a0, a1)`): `κ = (4/3)·tan((a1 − a0)/4)·ρ`; `p0 = c + ρ(cos a0, sin a0)`, `p3 = c +
  ρ(cos a1, sin a1)`; `c1 = p0 + κ(−sin a0, cos a0)`, `c2 = p3 − κ(−sin a1, cos a1)` (length error < 0.03 % for ≤ 90°).

Crown (id `crown`, name `FEZ CROWN`, material felt, cut `{qty 1}`, role `hat_crown`, sim false, note `Stiffen with
buckram or heavy fusible interfacing`):
- vertices `b_i = ρb(cos a_i, sin a_i)`, `t_i = ρt(cos a_i, sin a_i)` (inserted b0, t0, b1, t1, …).
- edges: `base_i` b_i→b_{i+1} (arc about the apex, radius ρb), each with edge `sa: 0` (the felt edge is bound or turned
  by hand); `back_r` b_n→t_n (line); for i = n−1 down to 0: `top_i` t_{i+1}→t_i (arc, radius ρt), each with notches
  `n_top_<i>_<j>` of type V at s 0.25 and 0.75 (n = 2) or 0.5 (n = 4), i.e. at the eighths of the top circle; `back_l`
  t0→b0 (line).
- grain `[[0, −mid − 0.3·slant], [0, −mid + 0.3·slant]]` with `mid = (ρb + ρt)/2` (straight grain up the centre front).

Tip (id `tip`, name `FEZ TIP`, material felt, cut `{qty 1}`, role `hat_tip`, sim false, note `Tassel at the centre
mark`, grain [[0, −r_top/2], [0, r_top/2]]):
- quarter angles `−π/2 + (π/2)·i`, i = 0..4 (p0 at the back seam); vertices p0..p3 at radius r_top; edges `q_i`
  p_i→p_{(i+1)%4} as quarter arcs; notch `n_q<i>` V at s 0.5 on each; drills `[{p: [0, 0], d: 3}]`.

Seams: `back` (a `crown/back_r`, b `crown/back_l`, reverse true); `tip` (a `crown/top_<n−1>+…+top_0`, b
`tip/q0+q1+q2+q3`, reverse true).

Pattern: style `FEZ`, title `Fez (one-piece crown, round tip)`, sa_mm 7, fabric `{id felt, name "wool felt", kind
felt, areal_g_m2 350, width_mm 900}`. `meta.solved = {base_girth_mm: round1(2π·r_base), top_girth_mm: round1(2π·r_top),
height_mm: round1, slant_mm: round1, sector_deg: round1(degrees φ)}`.

Fit spec `{schema fabric-fit/1, garment fez, seam_tol_mm 2.0, truing_tol_deg 8.0, points}`:
- `head_fit`: path over `crown/base_0..base_{n−1}`, body head, target [0, 10, 20], label `Base edge over the head
  girth`.
- `height`: path `crown/back_r`, body head, `body_scale 1/π`, target `[round1(slant − 1.15·d_head), round1(slant −
  d_head), round1(slant − 0.85·d_head)]`, label `Slant height (back seam) against the head diameter (design
  proportion)`. This is proportion, not fit: the band (±15 % of the head diameter) follows the style, so a taller fez
  still converges against its own contract; it catches an edit or tracing that drifted.

The round driver treats a fez as flat-only ("sim: not applicable": a cone has nothing for a fold-up to find).

---

## D. Conformance oracle

A Rust port is conformant when it reproduces the fixtures below within the stated tolerances. Unless a row says
otherwise: values printed to 1 decimal must match exactly after the same rounding (allow ±0.1 where a value sits on a
rounding boundary); vertex coordinates within 1e−3 mm; relative curve parameters within 1e−5; solved scalars within
the printed precision; image-based results within the test tolerances (they depend on OpenCV-style image processing
and an RNG that need not be bit-identical).

### D.1 Existing tests and what they pin

`test_measurements.py` (43 tests, all relevant):

| test | asserts |
|---|---|
| inverse_variance_fusion_evidence_only | tape 550±10 + photo 570±20, no priors: value 554.0, σ 8.9 (= closed form), primary MEASURED_TAPE, no flags, used weights sum to 1 |
| prior_joins_fusion_with_small_weight | tape 550±10 at 920 mm/30 mo: prior 540±30 joins, value = (550/100 + 540/900)/(1/100 + 1/900) → 549.0 |
| z_gate_refuses_above_3 | 550±10 vs 640±20: REMEASURE, z_max 4.02, value 550 σ 10, one used source |
| z_gate_fuses_and_flags_between_2_and_3 | 550±10 vs 610±20: z 2.68, Z_WARN, fused 562.0 |
| out_of_bounds_flag_and_prior_excluded | tape 700: VERIFY_OUT_OF_BOUNDS, value 700, prior unused |
| repeats_collapse_to_median_without_shrinking_sigma | neck_base 250/255/252 ±10 → 252 ±10, REPEATS_COLLAPSED |
| definition_normalization_hook | waist 500±15 as `CPSC77:66-navel` → 490, σ hypot(15, 5) = 15.8, NORMALIZED_DEFINITION |
| unknown_definition_is_excluded_not_fused | inseam 330 as `ISO8559-1:5.7.6` excluded; value 370, UNNORMALIZED_EXCLUDED |
| aliases_and_angle_units | `seat` → hip; `shoulder_slope` → shoulder_slope_deg with value_deg; an angle stored as value_mm fails validation with the angle hint |
| parse_value_requires_units | 55cm → 550, 21.5in → 546.1, 18 (deg) → 18, bare 55 for a length raises |
| readiness_missing_lists_specific_items | jacket with stature + chest only: NOT_READY, blocking {back_neck_to_waist, neck_base, across_back, shoulder_length}, each with a request line starting `<name>:` |
| readiness_ready_with_tape_limited_items | full taped bodice: READY; chest OK; neck_base OK_TAPE_LIMITED with an advisory |
| readiness_photo_only_girth_too_uncertain | chest only from a photo (σ 20): NOT_READY, blocking [chest] |
| readiness_trousers_require_total_crotch_length_and_nappy_seat | nappies: blocking {total_crotch_length, seat_over_nappy} |
| readiness_gathered_cuff_and_remeasure | zouave, photo waist 640 vs tape 520: blocking [waist], status REMEASURE |
| readiness_stale_advisory | fez, taken 2026-10-01, as_of 2026-12-31: READY with a STALE advisory |
| reference_wearer_is_complete_and_ready_reference | 920/30: reference true, fused names = catalogue, all records CHART_PRIOR, chest 540, hip 580, READY_REFERENCE + for_real_child for every kind |
| priors_sources_bounds_and_no_astm | 920/30 covers the catalogue; P3 < value < P97; no ASTM; judgement sources say UNCERTAIN; σ chest 30, waist 35, head 15; 980/36 back_neck_to_waist 240; 1600 mm raises |
| section_priors_are_self_consistent | girth_from_section(width prior, depth prior) = girth prior ±1 mm for chest, waist, hip |
| ramanujan_exact_cases | circle 2π·50; (10, 5) → 48.44224110 (rel 1e−6); semi-axis inversion round trip 61 ±1e−6 |
| girth_from_section_value_and_sigma | (160, 130, waist, 4, 5): 1.06·P(80, 65) = 484.2; σ 14.6 by the formula; chest notes say UNCERTAIN |
| calibrate_k_roundtrip | tape 560 on (172, 150, chest): k_ratio reproduces 560 ±0.2 |
| calib_sheet_exact_size[letter/a4] | PNG 2550 × 3300 / 2480 × 3508 at 300 dpi; PDF MediaBox 612 × 792 / 595.2 × 841.92; CCITT 1-bit; sidecar fields; bars within 0.09 mm of nominal |
| clean_sheet_detects_full_board | the letter PNG itself: 35/35 corners, mm/px = 25.4/300/png_scale (rel 1e−3), rms < 0.05 mm |
| photo_scale_recovers_mm_per_px | synthetic warped photo (D.7): ≥ 30 corners, mm/px rel 0.5 %, board centre within 2 px, rms < 0.5 mm |
| measure_photo_known_distances | the 100 mm bar and a 400 mm in-plane pair within 1 %; depth term = value·75/2500; validates and fuses |
| measure_photo_angle_and_resized_points | a 20° line picked on a half-size preview (`points_from_width` 1500): 20 ±0.3°, σ ≥ 1.5°, `[left]` tag in the method |
| cli_roundtrip | reference → ready zouave exits 0; init + add chest → ready bodice exits 3 with neck_base blocking; add without a unit exits 1 |
| rect_residual_exact_geometry | exact pinhole corners: residual < 1e−3 and f within 1 %; wrong paper > 0.05; square-on Letter as A4 = ln 1.0929 ±1e−3; orientation portrait/landscape by corner rotation; one-axis lean fits both papers (< 1e−3) with f_A4 > 1.5 f_Letter |
| plain_sheet_square_on_scale_and_distance | no warnings; mm/px rel 0.5 %; corners within 0.5 px; edge rms < 0.2 px; rect_residual < 0.005; blur 1.04 ±0.3; focal null; 400 mm ±1 % and within 3σ; homography σ term 0.5–1.5 mm |
| plain_sheet_tilted_20_degrees[axis 90/40] | ok with STRONG_PERSPECTIVE; tilt 25 ±2°; focal within 5 %; mm/px rel 0.5 %; 400 mm ±1.5 % |
| plain_sheet_auto_tells_letter_from_a4 | auto picks the right paper; own residual < 0.005; other ≈ ln 1.0929 ±0.005 |
| plain_sheet_wrong_paper_is_refused | Letter read as A4: ok false, only ASPECT_MISMATCH, message names `--paper letter`, no mm_per_px; measure_photo raises; neither reference → `not both` error |
| plain_sheet_auto_refuses_to_guess_under_one_axis_tilt | Letter leaning 5° about x: auto → PAPER_AMBIGUOUS only, both residuals < 0.005; named letter ok without warnings; named a4 ok with PAPER_AMBIGUOUS + STRONG_PERSPECTIVE and tilt > letter tilt + 10 |
| plain_sheet_larger_window_is_not_chosen | a 250 × 500 mm bright window behind: 2 candidates, the sheet chosen; window alone → NOT_A_SHEET with residual > 0.3 |
| plain_sheet_on_white_wall_not_found_exit_3 | CLI exit 3, 0 candidates, SHEET_NOT_FOUND |
| plain_sheet_thumb_over_a_corner[0/2] | a skin-tone blob over a corner: scale rel 1 %, 400 mm ±1 %, the hidden corner within 0.5 px |
| plain_sheet_under_a_clipboard_clip | a 120 mm clip hiding 56 % of the top edge: ok, no warnings, corners within 0.5 px; a 160 mm clip (74 %) → SHEET_NOT_FOUND |
| plain_sheet_cli | auto → letter, `image` set, no `sheet` key; the annotated quad is green; measure-photo method string exact; a4 refused (exit 1); zero or both references → argument error |

`test_core.py` tests in this subsystem's scope (the rest cover the pattern model, validation, alteration, export and
the workspace driver and belong to those specs):

| test | asserts |
|---|---|
| dress_form_faces_point_outward | positive signed volume for every part |
| torso_sections_match_tape_girths | Ramanujan(a, b) at hip, waist and chest = tape girth ±0.5 |
| form_crotch_path_matches_measured_total_crotch_length | path = TCL ±0.5 on 920/30; lowest torso vertex = crotch ±0.01; at 800/15, 1040/48, 1160/72 the excess is in [−0.5, 15] |
| thigh_roots_leave_a_gap_for_the_crotch_seam | leg_R max x ≤ −7 |
| wrap_is_isometric_and_roll_ends_at_wrap | bolero front.R outline arc lengths preserved within 1 %; roll(1) == wrap |
| drafts_converge_on_reference_wearer | bolero and trousers CONVERGED |
| trousers_are_placed_for_the_fold_up | roles; casing sim_scale.x · band = waist ±0.1, sim_scale.y 0.5; cuff [1, 0.5]; CF and CB lines wrap within 12 mm of x = 0; wrap radius ≥ leg section radius at hip + 15; finished cuff centre at the calf ±4 mm |
| trouser_leg_length_is_measured | leg_blouse IN_BAND, ease 40.0 ±0.5; hems lowered 150 mm → HIGH |
| elastic_must_be_shorter_than_the_waist | elastic_hold IN_BAND and negative; elastic_cut = 0.95·waist + 20 ±0.1; elastic_frac 1.05 → HIGH, not CONVERGED |
| bolero_cap_ease_equal_on_both_halves | armhole_f_R ratio = armhole_b_R ratio ±0.002 = 1.045 ±0.002 |
| mutation_side_seam_mismatch_is_caught | front side_hem 10 mm lower → side_R FAIL, residual > 9 |
| mutation_chest_too_small_reads_low | chest_ease 10 → chest LOW |
| mutation_truing_kink_is_caught | sleeve hem made straight → truing FAIL on `hem / hem` |
| missing_vertex_is_unmeasurable_not_a_crash | unknown at_vertex → UNMEASURABLE |
| default_form_armpit_bias_is_what_the_docs_say | default form armpit 30–60 mm above the chest landmark |
| form_armpit_is_at_the_measured_armscye_level | ANATOMICAL only (Part E) |
| form_shoulder_roof_matches_measured_slope | ANATOMICAL only (Part E) |
| infer_placement_by_role | bodice_front x0 = xmax, top at snp + 10; sleeve → arm; waistband → none |
| fez_is_an_exact_cone_that_fits_the_head | CONVERGED; base = head + 10 ±0.2; top = tip perimeter ±0.3 = 0.78·base ±0.3; hypot(height, Δr) = back_r ±0.1; back_r = back_l; a fez for a head 30 mm smaller reads head_fit LOW; top_frac 1.0 and height_frac 0 raise; height_frac 0.95 converges; readiness checks = [hat_fez] |
| soutache_border_ends_in_the_seams_and_keeps_its_inset | C.2 invariants at 800/18 and 1160/72 |
| measurement_gate_follows_the_pattern_roles | bolero → [bodice, set_in_sleeve]; trousers → full_trousers_gathered (elastic, calf); reference → READY_REFERENCE; only chest taped → NOT_READY naming neck_base, across_back, arm_length; all eight bolero musts taped (chest, back_neck_to_waist, neck_base, across_back, shoulder_length, arm_length, upper_arm, hand_pass) + stature → READY; no roles or no records → NOT_CHECKED |

### D.2 Fixture: reference wearer 920 mm / 30 months (no sex)

`reference_wearer(920, 30)`: 35 records, all `CHART_PRIOR`; every fused entry has flags `["PRIOR_ONLY"]`, value and σ
equal to the record; `fusion = {stature_for_priors_mm: 920.0, stature_source: "stature CHART_PRIOR record",
priors_used: true, z_refuse: 3.0, z_warn: 2.0, notes: []}`; wearer `{label: "reference toddler 92 cm", age_months: 30,
stature_mm: 920.0, reference: true}`. Records (catalogue order; P-bounds in the record's unit):

| name | value | σ | P3–P97 | P1–P99 | method (verbatim) |
|---|---|---|---|---|---|
| `stature` | 920 | 37.5 | [849.5, 990.5] | [832.9, 1007.1] | conditioning stature of the reference wearer (by definition) |
| `chest` | 540 | 30 | [483.6, 596.4] | [470.2, 609.8] | Ottobre row interpolated at 92.0 cm |
| `waist` | 530 | 35 | [464.2, 595.8] | [448.6, 611.4] | Ottobre row interpolated at 92.0 cm |
| `hip` | 580 | 35 | [514.2, 645.8] | [498.6, 661.4] | Ottobre row interpolated at 92.0 cm |
| `seat_over_nappy` | 630 | 40.3 | [554.2, 705.8] | [536.2, 723.8] | hip prior + 5 cm nappy bulk |
| `neck_base` | 251.2 | 15.3 | [222.5, 279.9] | [215.7, 286.7] | CPSC 1977 at 33 mo × (stature ratio 0.985): mid-neck #32 + 1.5 cm |
| `across_back` | 225.7 | 15.9 | [195.7, 255.7] | [188.6, 262.8] | CPSC 1977 at 33 mo × (stature ratio 0.985): biacromial #36 + 1 cm surface allowance |
| `shoulder_length` | 80 | 6 | [68.7, 91.3] | [66.0, 94.0] | Ottobre row interpolated at 92.0 cm |
| `shoulder_slope_deg` | 18 | 5 | [8.6, 27.41] | [6.37, 29.63] | placeholder |
| `back_neck_to_waist` | 225 | 15 | [196.8, 253.2] | [190.1, 259.9] | Ottobre row interpolated at 92.0 cm |
| `arm_length` | 320 | 13 | [295.5, 344.5] | [289.8, 350.2] | Ottobre row interpolated at 92.0 cm |
| `upper_arm` | 156.8 | 14 | [130.5, 183.1] | [124.2, 189.4] | CPSC 1977 at 33 mo × (stature ratio 0.985): #39 |
| `wrist` | 109.8 | 8 | [94.7, 124.8] | [91.2, 128.4] | 0.70 × upper-arm prior |
| `hand_pass` | 142.7 | 10 | [123.9, 161.5] | [119.4, 166.0] | 1.30 × wrist prior |
| `body_rise` | 172.5 | 25 | [125.4, 219.5] | [114.3, 230.6] | waist height − crotch height + 1.5 cm seated (03 §1.1, §4.4) |
| `total_crotch_length` | 396.6 | 40 | [321.4, 471.9] | [303.6, 489.7] | 2.3 × body rise |
| `inseam` | 372.5 | 30 | [316.1, 429.0] | [302.8, 442.3] | crotch/stature ratio 0.405 at 34 mo |
| `outseam` | 540 | 30 | [483.6, 596.4] | [470.2, 609.8] | Ottobre row interpolated at 92.0 cm |
| `thigh` | 288.8 | 24.8 | [242.1, 335.5] | [231.1, 346.5] | CPSC 1977 at 33 mo × (stature ratio 0.985): upper_thigh |
| `knee` | 210.6 | 20 | [173.0, 248.3] | [164.1, 257.2] | 1.03 × calf prior |
| `calf` | 204.5 | 14.9 | [176.4, 232.5] | [169.8, 239.1] | CPSC 1977 at 33 mo × (stature ratio 0.985): calf |
| `ankle` | 146.9 | 9.9 | [128.2, 165.6] | [123.8, 170.0] | CPSC 1977 at 33 mo × (stature ratio 0.985): ankle |
| `foot_pass` | 205.7 | 15 | [177.4, 233.9] | [170.8, 240.6] | 1.40 × ankle prior |
| `head` | 483.8 | 15 | [455.5, 512.0] | [448.9, 518.6] | WHO median at 30 mo, sexes averaged |
| `belly_depth` | 143.1 | 21 | [103.6, 182.6] | [94.2, 191.9] | navel-level depth inverted from CPSC #66/#67, scaled like waist |
| `chest_depth` | 149.4 | 18 | [115.6, 183.3] | [107.6, 191.3] | inverted from CPSC chest_axilla and chest_breadth, k=1.07 |
| `waist_depth` | 135 | 21 | [95.5, 174.5] | [86.2, 183.9] | inverted from CPSC waist_natural and waist_breadth_navel, k=1.06 |
| `hip_depth` | 133.3 | 21 | [93.8, 172.8] | [84.4, 182.1] | inverted from CPSC hip_buttocks and hip_breadth_trochanter, k=1.09 |
| `chest_width` | 171.5 | 12.8 | [147.4, 195.5] | [141.8, 201.2] | CPSC chest_breadth × 1.065 (Ottobre/CPSC girth ratio) |
| `waist_width` | 181.6 | 11.3 | [160.3, 202.8] | [155.3, 207.8] | CPSC waist_breadth_navel × 1.128 (Ottobre/CPSC girth ratio) |
| `hip_width` | 201.9 | 11.2 | [180.8, 223.0] | [175.8, 228.0] | CPSC hip_breadth_trochanter × 1.122 (Ottobre/CPSC girth ratio) |
| `armscye_depth` | 119.6 | 15 | [91.4, 147.8] | [84.7, 154.5] | 0.13 × stature |
| `waist_height` | 530 | 30 | [473.6, 586.4] | [460.2, 599.8] | Ottobre outside leg − 1 cm (tape follows the hip) |
| `crotch_height` | 372.5 | 30 | [316.1, 429.0] | [302.8, 442.3] | crotch/stature ratio 0.405 at 34 mo |
| `knee_height` | 231 | 20 | [193.4, 268.6] | [184.5, 277.5] | 0.62 × crotch height |

`priors_for(920, 30)` is identical except `stature` = 904.1 ± 37.5 (`CDC P50 at 30 mo`, P1–P99 [817.0, 991.3]).
`priors_for(920)` (no age) has no `stature` entry, infers age 32 mo (note `age inferred from CDC P50 stature: 32 mo`)
and gives head 486.2 (all other values equal to the 30 mo column, because CPSC is clamped at 33 mo either way).
Notes examples (verbatim): chest `σ = within-age SD per 03 §0.2`; upper_arm `girth scaled by ratio^0.5 (Ottobre steps
imply ≈0.55; UNCERTAIN); CPSC band centres span 33–72 mo; nearest band used at 30 mo`; inseam `σ = 3 cm per 03 §0.2;
cross-check Ottobre toddler inside leg 37.0 cm`.

Landmarks (`landmarks(ref)`):

| landmark | value |
|---|---|
| `floor` | 0.0000 |
| `ankle` | 55.0000 |
| `calf` | 160.6000 |
| `knee` | 231.0000 |
| `crotch` | 372.5000 |
| `hip` | 443.3750 |
| `waist` | 530.0000 |
| `chest` | 635.4000 |
| `across_back` | 680.3393 |
| `shoulder` | 725.2786 |
| `snp` | 750.0000 |
| `c7` | 755.0000 |
| `head_top` | 920.0000 |
| `head_centre` | 843.0008 |
| `neck_r` | 39.9797 |
| `head_r` | 76.9992 |
| `shoulder_half_width` | 116.0642 |
| `hip_joint_x` | 50.4750 |
| `arm_length` | 320.0000 |

Torso sections (z, a, b, centre_y):

| level | z | half_width a | half_depth b | centre_y |
|---|---|---|---|---|
| crotch ring | 397.5000 | 92.8740 | 45.4697 | -8.5440 |
| hip | 443.3750 | 100.9500 | 83.2440 | -8.5440 |
| waist | 530.0000 | 90.8000 | 77.6474 | -2.9474 |
| chest | 635.4000 | 85.7500 | 86.1371 | -11.4371 |
| across_back | 680.3393 | 84.0350 | 82.6930 | -7.9930 |
| shoulder − 15 | 710.2786 | 101.0642 | 24.4537 | 3.5629 |
| snp + 5 | 755.0000 | 87.9554 | 33.9101 | 3.5629 |

`crotch_path_mm` = 396.600 (target 396.6). Frames: arm_R origin (−101.0909, 0, 705.3142), up (0.573576, 0, 0.819152),
lat (0.819152, 0, −0.573576), tip_h 19.9644; leg_R origin (−50.475, 0, 397.5), up (0.069756, 0, 0.997564), lat
(0.997564, 0, −0.069756), length 343.3364, knee_d 166.9066 (L mirrored in x). `section_radius`: arm 28.6988; torso at
the waist 90.9760; leg_R at the hip level 80.6354. `landmark_h`: leg_R waist 132.8236, leg_R calf −237.4785; arm_R
shoulder_tip 19.9644, elbow −156.0356, wrist −300.0356. Dress form: torso 434 v / 480 f (z 372.5–815.0, |x| ≤
101.0642); head 530 / 576 (z 766.0017–920.0); each arm 242 / 288 (arm_R x −315.7389 to −70.6993, z 410.3903–731.6050);
each leg 290 / 336 (leg_R x −100.9054 to −7.5760, z 45.0244–412.4635).

### D.3 Fixture: the three drafts on the reference wearer

All three validate `CONVERGED`. Geometry as stored in pattern.json (vertices rounded to 3 decimals, relative curve
parameters to 5). Grain points and placement numbers are stored unrounded; they are shown here to 3 and 4 decimals
(for example the front placement `theta_deg` is −4.77464829275686 and the sleeve `radius_mm` 34.698819338330566;
the trouser placements are rounded by the program itself: theta to 2 decimals, radius to 1).

#### `zouave_bolero` on the reference wearer

`meta.solved` = `{"cap_height_mm": 91.8, "armhole_mm": 273.8, "cap_target_mm": 286.1, "cap_top_offset_mm": 12.8, "neck_scale": 1.2503}`

**Piece `back`** (role `bodice_back`, cut `{"qty": 1, "on_fold": "cb"}`)

- vertices: `cb_hem` (0, -10), `side_hem` (160, -10), `ua` (165, 93.4), `tip` (139.342, 213.846), `snp` (58.596, 237), `cbn` (0, 225)
- edges (CCW): `hem` cb_hem→side_hem; `side` side_hem→ua; `armhole` ua→tip cubic c=[[0.09982, 0.46857], [0.63739, 0.19301]]; `shoulder` tip→snp; `neck` snp→cbn cubic c=[[0.15871, 0.30394], [0.56811, 0.08845]]; `cb` cbn→cb_hem
- notches: `n_back_arm` on armhole s=0.45 double
- placement: `{"part": "torso", "theta_deg": 180.0, "x0": 0.0, "y0_landmark": "waist", "clearance_mm": 20}`
- grain: `[[20, 0], [20, 150]]`
- decor: soutache open 2 pts from [0.0, 2.0] to [160.58, 2.0], width 3.0; soutache closed 241 pts from [80.0, 43.24] to [80.0, 43.24], width 3.0

**Piece `front`** (role `bodice_front`, cut `{"qty": 2}`)

- vertices: `side_hem` (5, -10), `front_hem` (100, -10), `cfn` (145, 169.652), `snp` (99.404, 237), `tip` (24.228, 209.638), `ua` (0, 93.4)
- edges (CCW): `hem` side_hem→front_hem; `front_edge` front_hem→cfn cubic c=[[0.09504, -0.20163], [0.48247, -0.12963]]; `neck` cfn→snp cubic c=[[0.17286, 0.25533], [0.63859, 0.09077]]; `shoulder` snp→tip; `armhole` tip→ua cubic c=[[0.34954, 0.21651], [0.96431, 0.17123]]; `side` ua→side_hem
- notches: `n_front_arm` on armhole s=0.55 V
- placement: `{"part": "torso", "theta_deg": -4.7746, "x0": 145.0, "y0_landmark": "waist", "clearance_mm": 20}`
- grain: `[[115.0, 0], [115.0, 150]]`
- decor: soutache open 222 pts from [4.42, 2.0] to [88.13, 232.9], width 3.0; soutache closed 241 pts from [58.1, 38.2] to [58.1, 38.2], width 3.0

**Piece `sleeve`** (role `sleeve`, cut `{"qty": 2}`)

- vertices: `hem_b` (-84.9, -335), `hem_f` (84.9, -335), `ua_f` (103.4, -91.777), `top` (12.832, 0), `ua_b` (-103.4, -91.777)
- edges (CCW): `hem` hem_b→hem_f cubic c=[[0.32905, 0.02503], [0.67095, 0.02503]]; `under_f` hem_f→ua_f; `cap_f` ua_f→top cubic c=[[0.15896, 0.13833], [0.79278, -0.20998]]; `cap_b` top→ua_b cubic c=[[0.29566, -0.23346], [0.83072, 0.11395]]; `under_b` ua_b→hem_b
- notches: `n_cap_f` on cap_f s=0.45 V, `n_cap_b` on cap_b s=0.55 double
- placement: `{"part": "arm", "theta_deg": -90.0, "x0": 12.832, "y0_landmark": "shoulder_tip", "radius_mm": 34.6988}`
- grain: `[[0, -111.777], [0, -305.0]]`
- decor: soutache open 122 pts from [-87.55, -300.1] to [87.55, -300.1], width 3.0

Validation (flat checks, for cross-reference): status **CONVERGED**

| point | finished | body | ease | target | status |
|---|---|---|---|---|---|
| `chest` | 612.4 | 540.0 | 72.4 | [70, 100, 130] | IN_BAND |
| `across_back` | 250.7 | 225.7 | 25.0 | [10, 25, 45] | IN_BAND |
| `bicep` | 206.8 | 156.8 | 50.0 | [35, 50, 70] | IN_BAND |
| `sleeve_length` | 335.0 | 320.0 | 15.0 | [0, 15, 35] | IN_BAND |
| `cuff_pass` | 170.0 | 142.7 | 27.3 | [10, 25, 60] | IN_BAND |
| `neckline` | 301.2 | 251.2 | 50.0 | [30, 50, 90] | IN_BAND |
| `back_length` | 235.0 | 225.0 | 10.0 | [-10, 10, 30] | IN_BAND |

| seam | len a | len b | ratio | status |
|---|---|---|---|---|
| `side_R` | 103.5 | 103.5 | 1.0 | PASS |
| `side_L` | 103.5 | 103.5 | 1.0 | PASS |
| `shoulder_R` | 84.0 | 80.0 | 1.05 | PASS |
| `shoulder_L` | 84.0 | 80.0 | 1.05 | PASS |
| `armhole_f_R` | 133.2 | 127.5 | 1.045 | PASS |
| `armhole_f_L` | 133.2 | 127.5 | 1.045 | PASS |
| `armhole_b_R` | 152.9 | 146.3 | 1.045 | PASS |
| `armhole_b_L` | 152.9 | 146.3 | 1.045 | PASS |
| `underarm_R` | 243.9 | 243.9 | 1.0 | PASS |
| `underarm_L` | 243.9 | 243.9 | 1.0 | PASS |

#### `zouave_trousers` on the reference wearer

`meta.solved` = `{"back_crotch_ext_mm": 49.5, "crotch_target_mm": 426.6, "band_mm": 610.0, "cuff_mm": 229.5, "elastic_cut_mm": 523.5, "leg_to_cuff_top_mm": 394.4}`

**Piece `tr_front`** (role `pant_front`, cut `{"qty": 2}`)

- vertices: `hem_side` (29.271, -394.4), `hem_in` (223.428, -394.4), `crotch_pt` (252.7, -187.5), `cf_hip` (215, -86.625), `waist_cf` (215, 0), `waist_side` (0, 0)
- edges (CCW): `hem` hem_side→hem_in; `inseam` hem_in→crotch_pt; `crotch` crotch_pt→cf_hip cubic c=[[0.06741, 0.18036], [0.60515, 0.14757]]; `cf` cf_hip→waist_cf; `waist` waist_cf→waist_side; `outseam` waist_side→hem_side
- placement: `{"part": "leg", "theta_deg": 31.0, "x0": 215.0, "y0_landmark": "waist", "radius_mm": 98.0}`
- grain: `[[126.35, -334.4], [126.35, -207.5]]`

**Piece `tr_back`** (role `pant_back`, cut `{"qty": 2}`)

- vertices: `hem_in` (-17.744, -394.04), `hem_side` (201.199, -394.04), `waist_side` (235, 0), `waist_cb` (15, 20), `cb_hip` (0, -86.625), `crotch_pt` (-49.481, -187.5)
- edges (CCW): `hem` hem_in→hem_side; `outseam` hem_side→waist_side; `waist` waist_side→waist_cb; `cb` waist_cb→cb_hip; `crotch` cb_hip→crotch_pt cubic c=[[0.42664, 0.13962], [0.90303, 0.19769]]; `inseam` crotch_pt→hem_in
- placement: `{"part": "leg", "theta_deg": 149.0, "x0": 0.0, "y0_landmark": "waist", "radius_mm": 98.0}`
- grain: `[[91.727, -334.4], [91.727, -207.5]]`

**Piece `waistband`** (role `waistband`, cut `{"qty": 1}`, sim_scale `[0.8689, 0.5]`)

- vertices: `b0` (0, 0), `b1` (154.567, 0), `b2` (305, 0), `b3` (455.433, 0), `b4` (610, 0), `t4` (610, 60), `t0` (0, 60)
- edges (CCW): `bot_bR` b0→b1; `bot_fR` b1→b2; `bot_fL` b2→b3; `bot_bL` b3→b4; `end_r` b4→t4; `top` t4→t0; `end_l` t0→b0
- placement: `{"part": "torso", "theta_deg": 180.0, "x0": 0.0, "y0_landmark": "waist", "y0": 0.0, "clearance_mm": 15}`
- grain: `[[40, 20.0], [570.0, 20.0]]`
- internal: `[{"kind": "draw", "label": "FOLD (elastic casing)", "pts": [[0, 30.0], [610.0, 30.0]]}]`
- note: "Elastic 25 mm wide: cut 524 mm (includes 20 mm overlap)"

**Piece `cuff`** (role `cuff`, cut `{"qty": 2}`, sim_scale `[1.0, 0.5]`)

- vertices: `b0` (0, 0), `b1` (229.5, 0), `t1` (229.5, 60), `tm` (107.865, 60), `t0` (0, 60)
- edges (CCW): `bottom` b0→b1; `end_r` b1→t1; `top_back` t1→tm; `top_front` tm→t0; `end_l` t0→b0
- placement: `{"part": "leg", "theta_deg": -90.0, "x0": 0.0, "y0_landmark": "calf", "y0": 30.0, "radius_mm": 38.5}`
- grain: `[[20, 15.0], [209.5, 15.0]]`
- internal: `[{"kind": "draw", "label": "FOLD", "pts": [[0, 30.0], [229.5, 30.0]]}]`

Validation (flat checks, for cross-reference): status **CONVERGED**

| point | finished | body | ease | target | status |
|---|---|---|---|---|---|
| `seat` | 872.3 | 580.0 | 292.3 | [200, 320, 480] | IN_BAND |
| `crotch` | 426.6 | 396.6 | 30.0 | [15, 30, 50] | IN_BAND |
| `casing_pass` | 610.0 | 580.0 | 30.0 | [20, 40, 120] | IN_BAND |
| `elastic_hold` | 503.5 | 530.0 | -26.5 | [-63.6, -26.5, -5.0] | IN_BAND |
| `cuff_pass` | 229.5 | 205.7 | 23.8 | [10, 25, 60] | IN_BAND |
| `cuff_fit` | 229.5 | 204.5 | 25.0 | [10, 25, 45] | IN_BAND |
| `leg_blouse` | 394.4 | 354.4 | 40.0 | [15, 40, 80] | IN_BAND |

| seam | len a | len b | ratio | status |
|---|---|---|---|---|
| `crotch_front_R` | 200.3 | 200.3 | 1.0 | PASS |
| `inseam_R` | 209.0 | 209.0 | 1.0 | PASS |
| `inseam_L` | 209.0 | 209.0 | 1.0 | PASS |
| `outseam_R` | 395.5 | 395.5 | 1.0 | PASS |
| `outseam_L` | 395.5 | 395.5 | 1.0 | PASS |
| `cuff_front_R` | 194.2 | 107.9 | 1.8 | PASS |
| `cuff_front_L` | 194.2 | 107.9 | 1.8 | PASS |
| `cuff_back_R` | 218.9 | 121.6 | 1.8 | PASS |
| `cuff_back_L` | 218.9 | 121.6 | 1.8 | PASS |
| `cuff_end_R` | 60.0 | 60.0 | 1.0 | PASS |
| `cuff_end_L` | 60.0 | 60.0 | 1.0 | PASS |
| `crotch_back` | 226.3 | 226.3 | 1.0 | PASS |
| `casing_bot_bR` | 220.9 | 154.6 | 1.4292 | PASS |
| `casing_bot_fR` | 215.0 | 150.4 | 1.4292 | PASS |
| `casing_bot_fL` | 215.0 | 150.4 | 1.4292 | PASS |
| `casing_bot_bL` | 220.9 | 154.6 | 1.4292 | PASS |
| `casing_cb` | 60.0 | 60.0 | 1.0 | PASS |

#### `fez` on the reference wearer

`meta.solved` = `{"base_girth_mm": 493.8, "top_girth_mm": 385.2, "height_mm": 95.5, "slant_mm": 97.0, "sector_deg": 64.1}`

**Piece `crown`** (role `hat_crown`, cut `{"qty": 1}`, sim false)

- vertices: `b0` (-234.205, -373.733), `t0` (-182.68, -291.512), `b1` (0, -441.054), `t1` (0, -344.022), `b2` (234.205, -373.733), `t2` (182.68, -291.512)
- edges (CCW): `base_0` b0→b1 cubic c=[[0.32672, -0.09391], [0.67328, -0.09391]] sa=0; `base_1` b1→b2 cubic c=[[0.32672, -0.09391], [0.67328, -0.09391]] sa=0; `back_r` b2→t2; `top_1` t2→t1 cubic c=[[0.32672, 0.09391], [0.67328, 0.09391]]; `top_0` t1→t0 cubic c=[[0.32672, 0.09391], [0.67328, 0.09391]]; `back_l` t0→b0
- notches: `n_top_1_0` on top_1 s=0.25 V, `n_top_1_1` on top_1 s=0.75 V, `n_top_0_0` on top_0 s=0.25 V, `n_top_0_1` on top_0 s=0.75 V
- grain: `[[0.0, -421.647], [0.0, -363.428]]`
- note: "Stiffen with buckram or heavy fusible interfacing"

**Piece `tip`** (role `hat_tip`, cut `{"qty": 1}`, sim false)

- vertices: `p0` (0, -61.301), `p1` (61.301, 0), `p2` (0, 61.301), `p3` (-61.301, 0)
- edges (CCW): `q0` p0→p1 cubic c=[[0.27614, -0.27614], [0.72386, -0.27614]]; `q1` p1→p2 cubic c=[[0.27614, -0.27614], [0.72386, -0.27614]]; `q2` p2→p3 cubic c=[[0.27614, -0.27614], [0.72386, -0.27614]]; `q3` p3→p0 cubic c=[[0.27614, -0.27614], [0.72386, -0.27614]]
- notches: `n_q0` on q0 s=0.5 V, `n_q1` on q1 s=0.5 V, `n_q2` on q2 s=0.5 V, `n_q3` on q3 s=0.5 V
- grain: `[[0, -30.65], [0, 30.65]]`
- drills: `[{"p": [0.0, 0.0], "d": 3}]`
- note: "Tassel at the centre mark"

Validation (flat checks, for cross-reference): status **CONVERGED**

| point | finished | body | ease | target | status |
|---|---|---|---|---|---|
| `head_fit` | 493.8 | 483.8 | 10.0 | [0, 10, 20] | IN_BAND |
| `height` | 97.0 | 154.0 | -57.0 | [-80.1, -57.0, -33.9] | IN_BAND |

| seam | len a | len b | ratio | status |
|---|---|---|---|---|
| `back` | 97.0 | 97.0 | 1.0 | PASS |
| `tip` | 385.2 | 385.2 | 0.9999 | PASS |


Fez truing row: `base_1 / base_0` 180.0° PASS. Fez fit-spec height target on this wearer: [−80.1, −57.0, −33.9].
Trouser `elastic_hold` scale 0.82541, target [−63.6, −26.5, −5.0]; `leg_blouse` body_offset_mm −175.6; casing seam
ratio 1.429 (measured 1.4292).

Intermediate values worth asserting (bolero): q 160, wb 165, wf 155, y_ua 93.4, y_snp 237, neck_target 301.2, y_ab
150.3393, ab_target 250.7, neck_w 58.596, cfx 145, y_cfn 169.652, bic 206.8, slen 335, hem_w 169.8 (wrist + 60 wins
over hand_pass + 25 = 167.7). Trousers: quarter 165, wf 215, wb 235, y_hip −86.625, y_cr −187.5, ext_f 37.7,
cuff_len 229.5 (calf + 25 wins over foot_pass + 15 = 220.7), hem_total 413.1, z_cuff_top 175.6, y_hem −394.4, r_wrap
98.0, mid_deg 31.0, wl_f 215.0, wl_b 220.907, band 610 (seat + 30 wins over 544.88), Newton result hem_dy ≈ +0.36,
hem_dx ≈ −1.03. Fez: d_head 154.0, r_base 78.59, r_top 61.301, ρb 441.054, ρt 344.022, φ 64.1°, n 2.

### D.4 Fixture: drafts at other statures (reference wearers)

| wearer | bolero solved | trousers solved | fez solved |
|---|---|---|---|
| 800 mm / 18 mo | cap_height 83.8, armhole 255.7, cap_target 267.3, cap_top_offset 17.3, neck_scale 1.2546 | back_crotch_ext 29.2, crotch_target 354.4, band 590.0, cuff 215.7, elastic_cut 504.5, leg_to_cuff_top 332.5 | base 481.2, top 375.3, height 93.0, slant 94.5, sector 64.2 |
| 1160 mm / 72 mo | cap_height 116.0, armhole 319.1, cap_target 333.4, cap_top_offset 9.2, neck_scale 1.2448 | back_crotch_ext 54.5, crotch_target 460.0, band 690.0, cuff 256.4, elastic_cut 561.5, leg_to_cuff_top 500.3 | base 518.5, top 404.4, height 100.4, slant 102.0, sector 64.1 |

Form crotch path vs total crotch length (mm): 920/30 396.600 vs 396.6; 800/15 and 800/18 336.163 vs 324.4; 1040/48
428.700 vs 428.7; 1160/72 439.593 vs 430.0.

### D.5 Fixture: fusion and readiness examples (wearer 920 mm / 30 mo, taken 2026-10-01)

Exact fused entries (tape = `MEASURED_TAPE`, method `tape, parent`; photo = `PHOTO_ESTIMATED`, method `photo P1`):

| case | value | σ | flags | other |
|---|---|---|---|---|
| chest tape 550±10 + photo 570±20, no priors | 554.0 | 8.9 | — | weights 0.8 / 0.2; z_max 0.89; chart prior listed unused (weight 0, auto_prior); p1_p99 [470.2, 609.8] |
| chest tape 550±10, priors on | 549.0 | 9.5 | — | weights tape 0.9, prior 0.1 |
| chest 550±10 vs 640±20 | 550.0 | 10.0 | REMEASURE | z_max 4.02; note `not fused: MEASURED_TAPE 550.0±10.0 vs PHOTO_ESTIMATED 640.0±20.0, z=4.02 > 3 — re-measure this one` |
| chest 550±10 vs 610±20 | 562.0 | 8.9 | Z_WARN | z_max 2.68; note `sources disagree at z=2.68 (2<z≤3): fused, but check` |
| chest tape 700±10 | 700.0 | 10.0 | VERIFY_OUT_OF_BOUNDS | note `measured 700.0 is outside the chart P1–P99 [470.2, 609.8]: confirm once; chart prior left out of the fusion` |
| neck_base 250, 255, 252 ±10 | 252.0 | 10.0 | REPEATS_COLLAPSED | source n_repeats 3, note `3 repeats: median, spread 2.5` |
| waist 500±15 as CPSC77:66-navel | 490.0 | 15.8 | NORMALIZED_DEFINITION | source note `CPSC77:66-navel → SKILL:waist-ribbon: -10 mm, σ+5 (navel waist ≈ natural waist + 1 cm at 2–6 y (03 §0.2.1))` |
| inseam 370±20 + 330±20 as ISO8559-1:5.7.6 | 370.0 | 20.0 | UNNORMALIZED_EXCLUDED | notes `definition ISO8559-1:5.7.6 has no normalization to OTTOBRE:inside-leg; not fused`; fusion.excluded lists it; stature_source `wearer.stature_mm`, priors_used false |

`girth_from_section(160, 130, waist, 4, 5)` → girth 484.2, σ 14.6, ellipse 456.8, k 1.06, k_sigma 0.022, terms
{width 6.99, depth 7.87, k 10.05}. `girth_from_section(170, 150, chest)` → 538.4 ± 15.1 (ellipse 503.1; k term 15.09).
`calibrate_k(560, 172, 150, chest, 10)` → k_child 1.1058, k_child_sigma 0.0197, k_default 1.07, k_ratio 1.0335,
k_ratio_sigma 0.0185.

Readiness, bodice with stature 921 and chest 548 taped (as_of 2026-10-12): NOT_READY; items stature OK, chest OK,
back_neck_to_waist/neck_base/across_back/shoulder_length PRIOR_ONLY, should items PRIOR_ONLY; request lines:
- `back_neck_to_waist: Head bent forward to find C7; tape straight down the spine to the ribbon's lower edge. Need ±1.0 cm; only a size-chart guess (±1.5 cm).`
- `neck_base: Tape round the base of the neck where a T-shirt neckband sits: over the C7 bump and the breastbone notch. Need ±0.5 cm; only a size-chart guess (±1.5 cm).`
- `across_back: Find the bony shoulder tips (mark them); tape across the back from tip to tip, arms hanging. Need ±1.0 cm; only a size-chart guess (±1.6 cm).`
- `shoulder_length: From where the neck meets the shoulder to the bony shoulder tip. Need ±0.5 cm; only a size-chart guess (±0.6 cm).`
and advisories `optional — waist: … Need ±1.5 cm; only a size-chart guess (±3.5 cm).`, `optional — shoulder_slope_deg:
… Need ±3.0°; only a size-chart guess (±5.0°).`, `optional — belly_depth: … Need ±1.0 cm; only a size-chart guess
(±2.1 cm).`

Full taped bodice (stature 921, chest 548, back_neck_to_waist 228, neck_base 252, across_back 226, shoulder_length 81,
tape σ from the catalogue): READY; neck_base, across_back, shoulder_length OK_TAPE_LIMITED; advisories begin
`neck_base: ±0.84 cm is the tape's limit (target ±0.50 cm); check it on the muslin.`, `across_back: ±1.09 cm is the
tape's limit (target ±1.00 cm); check it on the muslin.`, `shoulder_length: ±0.51 cm is the tape's limit (target
±0.50 cm); check it on the muslin.`, `shoulder_length: the size-chart prior outweighs the measurement here; fine for
drafting, re-check on the muslin.`

Reference wearer, `full_trousers_gathered` (cuff calf): READY_REFERENCE, items waist (target 30), hip (20), outseam,
body_rise, total_crotch_length, calf, foot_pass, then thigh, inseam, belly_depth; `for_real_child` has 7 lines (e.g.
`calf: Tape level round the fullest part of the calf. Need ±1.5 cm; only a size-chart guess (±1.5 cm).`, showing that
PRIOR_ONLY blocks even at target). Request counts for the other kinds: bodice 6, set_in_sleeve 2, trousers 5,
hat_fez 1.

### D.6 Fixture: priors across statures

Value ± σ (mm; degrees for the slope):

| name | 800 mm / 15 mo | 920 mm / 30 mo | 980 mm / 36 mo | 1040 mm / 48 mo | 1160 mm / 72 mo |
|---|---|---|---|---|---|
| `stature` | 787.9 ± 30.6 | 904.1 ± 37.5 | 944.2 ± 38.9 | 1014.6 ± 42.8 | 1150.7 ± 50.9 |
| `chest` | 500 ± 30 | 540 ± 30 | 560 ± 30 | 580 ± 30 | 620 ± 30 |
| `waist` | 510 ± 35 | 530 ± 35 | 540 ± 35 | 550 ± 35 | 570 ± 35 |
| `hip` | 560 ± 35 | 580 ± 35 | 600 ± 35 | 620 ± 35 | 660 ± 35 |
| `seat_over_nappy` | 610 ± 40.3 | 630 ± 40.3 | 650 ± 40.3 | 670 ± 40.3 | 710 ± 40.3 |
| `neck_base` | 235.3 ± 15.3 | 251.2 ± 15.3 | 257.7 ± 15.3 | 261.1 ± 15.3 | 273.6 ± 15.3 |
| `across_back` | 197.6 ± 14.4 | 225.7 ± 15.9 | 238.2 ± 16.3 | 245.9 ± 15.5 | 271.2 ± 16.3 |
| `shoulder_length` | 72 ± 6 | 80 ± 6 | 84 ± 6 | 88 ± 6 | 96 ± 6 |
| `shoulder_slope_deg` | 18 ± 5 | 18 ± 5 | 18 ± 5 | 18 ± 5 | 18 ± 5 |
| `back_neck_to_waist` | 195 ± 15 | 225 ± 15 | 240 ± 15 | 250 ± 15 | 280 ± 15 |
| `arm_length` | 280 ± 13 | 320 ± 13 | 340 ± 13 | 360 ± 13 | 400 ± 13 |
| `upper_arm` | 146.2 ± 14 | 156.8 ± 14 | 161.3 ± 14 | 164.1 ± 14 | 173 ± 14 |
| `wrist` | 102.4 ± 8 | 109.8 ± 8 | 112.9 ± 8 | 114.8 ± 8 | 121.1 ± 8 |
| `hand_pass` | 133.1 ± 10 | 142.7 ± 10 | 146.8 ± 10 | 149.3 ± 10 | 157.5 ± 10 |
| `body_rise` | 141 ± 25 | 172.5 ± 25 | 185.3 ± 25 | 186.4 ± 25 | 186.9 ± 25 |
| `total_crotch_length` | 324.4 ± 40 | 396.6 ± 40 | 426.2 ± 40 | 428.7 ± 40 | 430 ± 40 |
| `inseam` | 324 ± 30 | 372.5 ± 30 | 399.7 ± 30 | 438.6 ± 30 | 518.1 ± 30 |
| `outseam` | 460 ± 30 | 540 ± 30 | 580 ± 30 | 620 ± 30 | 700 ± 30 |
| `thigh` | 269.3 ± 23.1 | 288.8 ± 24.8 | 299.2 ± 25.6 | 312.9 ± 26.3 | 342.1 ± 31.2 |
| `knee` | 196.4 ± 20 | 210.6 ± 20 | 216.7 ± 20 | 221.1 ± 20 | 238.3 ± 20 |
| `calf` | 190.7 ± 13.9 | 204.5 ± 14.9 | 210.4 ± 15.2 | 214.7 ± 15.2 | 231.4 ± 16.1 |
| `ankle` | 137 ± 9.3 | 146.9 ± 9.9 | 150.5 ± 10 | 150.9 ± 9.1 | 161 ± 11.1 |
| `foot_pass` | 191.8 ± 15 | 205.7 ± 15 | 210.7 ± 15 | 211.3 ± 15 | 225.4 ± 15 |
| `head` | 468.1 ± 15 | 483.8 ± 15 | 490 ± 15 | 497.5 ± 15 | 508.5 ± 15 |
| `belly_depth` | 137.7 ± 21 | 143.1 ± 21 | 144.3 ± 21 | 141.3 ± 21 | 140 ± 21 |
| `chest_depth` | 138.4 ± 18 | 149.4 ± 18 | 155 ± 18 | 160.9 ± 18 | 175 ± 18 |
| `waist_depth` | 130 ± 21 | 135 ± 21 | 136.5 ± 21 | 134.5 ± 21 | 137.1 ± 21 |
| `hip_depth` | 128.7 ± 21 | 133.3 ± 21 | 137.7 ± 21 | 141.4 ± 21 | 153 ± 21 |
| `chest_width` | 158.8 ± 11.8 | 171.5 ± 12.8 | 177.8 ± 12.9 | 183.8 ± 12.1 | 193.7 ± 15.1 |
| `waist_width` | 174.7 ± 10.9 | 181.6 ± 11.3 | 185.9 ± 11.6 | 193.2 ± 12.5 | 202.2 ± 17.7 |
| `hip_width` | 195 ± 10.8 | 201.9 ± 11.2 | 209.1 ± 11.8 | 216.7 ± 12.7 | 228.7 ± 12.5 |
| `armscye_depth` | 104 ± 15 | 119.6 ± 15 | 127.4 ± 15 | 135.2 ± 15 | 150.8 ± 15 |
| `waist_height` | 450 ± 30 | 530 ± 30 | 570 ± 30 | 610 ± 30 | 690 ± 30 |
| `crotch_height` | 324 ± 30 | 372.5 ± 30 | 399.7 ± 30 | 438.6 ± 30 | 518.1 ± 30 |
| `knee_height` | 200.9 ± 20 | 231 ± 20 | 247.8 ± 20 | 271.9 ± 20 | 321.2 ± 20 |

### D.7 Fixture: photo scale

ChArUco sheet sidecars (`calib_sheet(paper, dir)` with defaults): letter board origin_mm [11.43, 7.959], origin_px [135,
94], size_px [2079, 1559], png_scale 1.000125; bars y_100mm start [193.167, 12.023] end [193.167, 112.014] length
99.991; y_4in x 202.65, length 101.6; x_100mm start [11.43, 147.701] end [111.421, 147.701]; x_4in y 156.76; squares
square_10cm origin [11.43, 166.031] size 99.991, square_1in origin [117.433, 166.031] size 25.4. A4: origin_mm [8.467,
7.959], origin_px [100, 94], size_px [2079, 1819]; y bars at x 190.204 and 199.771; x bars at y 169.714 and 178.774;
squares at y 187.96 (x 8.467 and 114.469).

ChArUco photo scale on the clean letter PNG: 35/35 corners, 24 markers, 35 inliers, mm/px 0.084697 (expected
25.4/300/1.000125 = 0.0846561, within 1e−3), rms 0.0279 mm, no warnings.

Synthetic ChArUco photo (test generator): the letter PNG blurred σ 1.2 px, warped by the perspective map taking the
sheet's pixel-edge corners (−0.5, −0.5), (w − 0.5, −0.5), (w − 0.5, h − 0.5), (−0.5, h − 0.5) to (1400, 1900), (1990,
1945), (2012, 2700), (1378, 2688) in a 3000 × 4000 image (background 105, bilinear), blurred σ 0.8, multiplied by a
vertical ramp 0.85 → 1.05, plus Gaussian noise σ 3 (seed 7), JPEG quality 92. Oracle result: 35 corners, mm/px
0.366473 (truth 0.366190), tilt 22.13°, variation 0.1118, two STRONG_PERSPECTIVE warnings, board centre (1678.08,
2115.05) vs truth (1678.00, 2115.06). measure_photo: the x_100mm bar 100.0 ± 1.5 (terms homography 0.042, depth 0,
click 1.468; notes end `; name not in the catalogue`); a 400 mm pair at sheet mm (−120, 150)–(280, 150) with depth
offset 75 → 400.3 ± 12.1 (depth term 12.008). Angle: a 20° line picked at half size → 20.01° ± 1.72 (terms 0.019,
0.835, 1.5), method `photo ? ChArUco sheet-plane homography [left]; angle below horizontal in the sheet plane (sheet
assumed upright)`.

Plain-sheet synthetic photos (test generator, 1500 × 2000 px at 0.5 mm/px): pinhole camera with f = 1.15 × image
diagonal (2875 px), principal point at the image centre, the sheet centre imaged at (450, 1100) px; tilt by a rotation
of `tilt_deg` about the in-plane axis at `axis_deg` from the sheet x axis. Wall: grey 110 with texture σ 14 (sum of two
unit-variance Gaussian-blurred noise fields, scales 6 and 40 px, seed 5) and a slight red tint (channel gains B 1.0,
G 0.96, R 0.92); sheet grey 240 drawn with anti-aliased coverage; optional window (grey 235, sheet-plane mm polygon
(300,−200), (550,−200), (550,300), (300,300)), dark clip (grey 55) or thumb (BGR 70, 95, 140 ellipse of radius 13 % of
the short side at a corner, y squashed 1.25); then blur σ 1 px, vertical shading 0.90 → 1.04, noise σ 3 (seed 3), JPEG
92. Oracle results (paper requested → outcome):

| case | letter | a4 | auto | measure 400 mm (paper as drawn) |
|---|---|---|---|---|
| Letter square-on | ok, portrait, rr 0.0002 (a4 0.08863), mm/px 0.500004, tilt 1.17, blur 1.04, rms 0.0385 px, 500 edge pts, no warnings | ASPECT_MISMATCH | ok letter | 400.0 ± 2.3 (homography 0.896, click 2.122), method `photo P1 plain Letter sheet-plane homography; sheet-plane distance` |
| A4 square-on | ASPECT_MISMATCH (rr 0.08877) | ok, rr 0.00017, mm/px 0.499988 | ok a4 | 400.0 ± 2.3 (homography 0.945) |
| Letter tilted 20° about axis 90° | ok, rr 5e−5, tilt 25.39, variation 0.0802, focal 2871.5, 2 × STRONG_PERSPECTIVE | ok landscape (rr 0.00069), PAPER_AMBIGUOUS + 2 × STRONG_PERSPECTIVE, tilt 60.4 | PAPER_AMBIGUOUS | 400.0 ± 2.5 |
| Letter tilted 20° about axis 40° | ok, rr 0.00044, tilt 25.09, focal 2864.3, 2 × STRONG_PERSPECTIVE | ASPECT_MISMATCH (0.08694) | ok letter | 400.0 ± 2.4 |
| Letter leaning 5° (axis 0°) | ok, rr 6e−5, tilt 8.61, no warnings | ok, rr 0.00132, tilt 24.71, PAPER_AMBIGUOUS + STRONG_PERSPECTIVE | PAPER_AMBIGUOUS | 400.0 ± 2.3 |
| Letter with window behind | ok (2 candidates) | ASPECT_MISMATCH | ok letter | 400.0 ± 2.3 |
| window only | NOT_A_SHEET (rr 0.43532) | NOT_A_SHEET (0.34652) | NOT_A_SHEET | refused |
| white wall (grey 236, σ 3, no tint) | SHEET_NOT_FOUND, 0 candidates | same | same | refused |
| thumb over corner 0 / 2 | ok, rr 0.0002, 497 / 496 edge pts | ASPECT_MISMATCH | ok | 400.0 ± 2.3 |
| clip 120 mm (56 % of top edge) | ok, 421 edge pts, no warnings | ASPECT_MISMATCH | ok | 400.0 ± 2.3 |
| clip 160 mm (74 %) | SHEET_NOT_FOUND | same | same | refused |

Exact-geometry rectangle residuals (no image): Letter tilted 25° about axis 40°, centre (700, 900): as Letter 0.000218
at f 2871.48 (true 2875); as A4 0.0874. Square-on Letter read as A4: 0.088796. Letter leaning 20° about axis 0°: as
Letter 0.000143 (f 2871.5), as A4 0.000074 (f 4691.5).

### D.8 Tolerance guidance for the port

- Pure arithmetic (priors, fusion, readiness, girth, landmarks, sections, frames, drafting): exact to printed
  precision; strings exact.
- Bisection results depend on iteration counts and edge-length sampling; keep 60 iterations, 200-sample edge lengths,
  400-sample cap lengths and the same rounding of stored geometry, or the 4th–5th decimals drift.
- Image pipeline: corners within 0.5 px of truth on the synthetic photos; mm/px within 0.5 % (1 % for occlusion
  cases); classification (warning codes, ok flags, candidate counts) exact; bootstrap σ terms within ±20 % (RNG streams
  differ).

---

## E. Exclusions / not to port

1. **Analytic dress-form mesh → replaced by the Anny body model.** `dress_form` (rings, tubes, head sphere, limb
   stations, inset/slim, thigh-gap slimming, face winding), the `ANATOMICAL` variant (shoulder roof, solved arm joint)
   and its two tests are not ported as the collision body; Anny (Apache-2.0 code, CC0 assets, child-capable) supplies
   the mesh. The ANATOMICAL form was unstable in cloth simulation and is not a default anywhere.
2. **Still needed for compatibility (port as a small "legacy frame" layer, independent of the mesh):**
   - `mval`, `landmarks` (every key): drafting uses chest, waist, hip, across_back, calf/knee/ankle levels; validation
     uses landmark heights for girth points.
   - `torso_sections` including the crotch-ring solve, `section_radius`, `frames`, `landmark_h`: the drafting
     programs write `radius_mm` and `theta_deg` from them into pattern.json (sleeve radius 34.6988, trouser wrap
     radius 98.0 and midline angle 31.0 on the reference wearer), so the conformance fixtures depend on them.
   - `infer_placement(s)`, `wrap`, `roll`, `mirror_x`: the placement contract shared with the simulator and the
     traced-sheet path.
   - `crotch_path_mm`: the simulator's `form_crotch_excess_mm` diagnostic (meaningless once Anny supplies the body;
     recompute it on the Anny mesh or drop it).
   Switching landmarks or radii to Anny-derived values later is a deliberate, versioned fixture change, not a port
   detail.
3. **Calibration-sheet rendering bytes:** fonts, PNG/PDF encoders, CCITT compression are not conformance targets;
   page size, DPI, board geometry, marker dictionary bits and the sidecar numbers are. Marker bit patterns must match
   OpenCV's predefined `DICT_4X4_50` so sheets printed by either implementation detect in both.
4. **OpenCV internals:** ChArUco detection, RANSAC, Huber line fit, contours, polygon approximation, Otsu, resize,
   remap, Gaussian blur. Port the algorithms as described (or bind a library); match outcomes within D.8, not bit for
   bit.
5. **RNG streams:** the bootstrap uses a seeded generator; reproduce the method (resampling, B = 200, size jitter
   0.5 mm), not the numbers.
6. **`annotate`:** visual aid only (colours and what is drawn are specified; pixels are not).
7. **Out of this spec:** the pattern model, flat validation, alteration operators, exports, the fold-up simulator
   (`sim_scale` handling, own-side clamp, pre-sew), the round/workspace driver and report, the browser (Pyodide) page.
   Their tests in `test_core.py` are listed in their own specs.
8. **Never port:** any real person's measurements or photos; ASTM size tables; non-commercial body models (SMPL family
   and similar) as defaults.

### E.1 Native capture review extension (not reference parity)

For a new, unbound review only, a frame may declare `frame: {stream, timestamp_us}` instead of
`source`; its `calibration_sha256` may be omitted. The review resolves that exact identity through
the current decoded-frame selector and hashes the declared calibration file, then performs all
ordinary lens/size/rotation/epoch/sheet checks. It never chooses another frame or infers a pose,
camera, model target or observation. The input must declare exactly one of `frame` and `source`.
Complete source records still require their declared calibration hash. Supplied hashes always
must match. Declarations are refused by `fit-body` and whenever `reviewed_sources_sha256` is present;
they cannot refresh a previously bound capture silently. The replay replaces declarations with
complete selected-source records and calibration hashes, retaining the original parsed input and
bytes separately. It then follows the existing source-bound review, annotation and fitting route.

`ftx measure capture-review CAPTURE.json --out NEW_DIR` runs the existing
`ftx-anny-capture-input/1` source-selection and checked-calibration/sheet-pose join without
loading a body model or fitting. Its private create-only directory contains original input
bytes, current preparation evidence, hash-checked copies of selected images, and overlays of
every supplied named keypoint with its original FIT/CHECK partition, sigma and model target.
Pixels and coordinates are neither rotated nor rectified again. Unknown/duplicate labels,
nonpositive sigmas and out-of-image points refuse rather than disappear from the overlay.
Each frame also exposes a separate read-only stationary-sheet overlay on the same stored image.
It shows every original metric/pixel correspondence and FIT/CHECK role, its projection through
the already checked camera, and the existing per-point residual. Reference/lens identity and
calibration/reference hashes remain visible. This display neither redetects nor edits sheet
points, changes camera admission, rectifies pixels, nor authenticates a physically stationary,
flat, correctly sized reference. Body annotation and replay remain unchanged.

`capture.json` retains the original capture document except its local manifest/calibration/
reference paths are rebased to absolute paths and `reviewed_sources_sha256` binds the current
selection and per-frame source/camera preparation evidence. The existing `fit-body` consumer
reruns those checks from any working directory and refuses a changed evidence digest. This
optional binding is absent from older inputs; it does not change their behavior or the fitter.
The digest excludes the capture request's own pathname/bytes to avoid a recursive identity.
An optional top-level `silhouettes` local manifest path reuses the existing strict
`ftx-body-silhouettes/1` reader during capture preparation, before loading a body model.
`capture-review --silhouettes MANIFEST` adds that path to the replay document; it refuses to
replace an existing manifest declaration. Replay rebases the path and ordinary fitting rereads
the same manifest and masks. The receipt's `input` remains the original file's parsed document;
the replay carries the added declaration and preparation retains the mask-import receipt.
When present, the source digest additionally binds the manifest bytes,
sample definitions and decoded mask hashes; changed evidence refuses. Inputs without a manifest
retain their original source-digest calculation. Inline silhouettes cannot be overwritten by masks.
The review validates supplied inline/imported silhouettes through the existing rectified-camera
and scanline checks, then displays every observed slice and foreground interval, including explicit
empty/background lines, original FIT/CHECK role, axis, location, sigma and evidence. These are
read-only observed constraints, not predicted contours. No segmentation, rectification, clothing
or occlusion inference, full-mask validation or body fitting occurs in this step.
The copied images are review artifacts;
fitting still requires the original current sources. `review.json` is the last publication
write and reports `PREPARED`, never accepted shape, observable fit or READY. This review does
not validate target anatomy against model topology, segmentation, decoding, stationary reference,
physical camera accuracy or wearer fit. Its original input hash and full preparation evidence
identify the source; a changed source must be reviewed and prepared again.

The local review optionally edits existing keypoint coordinates in memory: select a named point,
click its image in stored-pixel coordinates or apply finite numeric coordinates within the image.
No clamping, inferred anatomy, model-target definition change or existing-observation sigma or
FIT/CHECK reassignment is permitted. Other capture input values and source bindings stay fixed,
except for the explicitly declared observation additions below. Reset restores the original point
lists. Pending/invalid edits and reset withdraw/revoke corrected downloads.
Only a current changed input is downloadable as `capture-edited.json`; original on-disk input,
SVG and preparation evidence stay unchanged and are labelled original. A new immutable review can
be generated from the correction before fitting. No body computation or acceptance occurs in the
editor. Static original review remains usable with scripts disabled; editing uses only its local
external script, safely embedded inert JSON and in-memory Blob downloads, without network/storage.
Documents containing integers outside JavaScript's exact safe range refuse browser editing;
the native static review retains their original values. No timestamp is rounded to enable an edit.

Frames may initially have no keypoint observations. The local editor can append observations only
for names already declared in `model.keypoints`. Before placement the caller explicitly chooses a
frame, unused declared target, FIT/CHECK partition and finite sigma in 1e-6..1e6 pixels. An image
click or finite in-image numeric coordinates then commits the observation. No model target is added,
guessed or reassigned; existing observations still permit coordinate edits only. Duplicate names and
more than 128 observations per frame refuse. Pending/invalid placement or cancellation withdraws
downloads; reset restores the entire original keypoint lists. All other input values and source
bindings stay fixed. Partial drafts are saveable; source preparation and point count do not establish
body observability, fit acceptance or READY. The ordinary fitter retains all its original gates.

## F. Quirks to preserve (or change deliberately with a fixture update)

- bisection has no bracket check and treats f = 0 as negative (C.0, 0.3).
- Bolero front placement angle −degrees(front_gap/120) vs `infer_placement`'s fixed −4.8.
- The bolero hem width takes `max(hand_pass + cuff_ease, wrist + 60)`; on the reference wearer the wrist term wins.
- `seat_over_nappy`, `waist_height`, `knee` and `foot_pass` priors derive from *rounded* priors; `wrist` and
  `hand_pass` from the unrounded upper-arm value.
- `priors_for` without an age omits `stature`; the reference wearer always has it (value by definition, σ from CDC).
- Readiness `PRIOR_ONLY` blocks even when the prior σ meets the target; `target or tape σ or 10` is a falsy fallback.
- With `use_priors = false`, chart priors still bound measured values (listed unused).
- `measure_photo` warns about a zero depth offset based on the default only.
- The trouser casing seam ratio is stored to 3 decimals (1.429) while measured as 1.4292.
- `body.depth_for_perimeter` (never raises, floor b = 1) differs from `measurements.semi_axis_for_perimeter` (raises
  when P ≤ 4a).
