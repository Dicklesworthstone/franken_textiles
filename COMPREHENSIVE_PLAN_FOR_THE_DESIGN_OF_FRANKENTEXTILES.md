# Comprehensive Plan for the Design of FrankenTextiles

Version 1.0 · 2026-10-12 · plan-first: nothing below is implemented unless a later commit says so.
Every number in this document is a **target, a budget or a measurement taken on the reference
implementations**, never a FrankenTextiles result.

---

## Contents

0. How to read this plan
1. Product contract
2. Where this comes from: the reference implementation and what it taught
3. The four claims, never conflated
4. Oracles and pins
5. Architecture overview
6. Units, coordinates, determinism and refusals
7. `ftx-core`
8. Patterns: model, validation, alteration, drafting (`ftx-pattern`)
9. Bodies: a real parametric body model (`ftx-body`)
10. Measurement and photogrammetry (`ftx-measure`)
11. Draping: a contact-correct cloth solver (`ftx-drape`)
12. Exports and manufacturing files (`ftx-export`)
13. Tracing pattern sheets (`ftx-trace`)
14. The `ftx` command line and the skill boundary (`ftx-cli`)
15. The browser (`ftx-wasm`)
16. Workspace crates, budgets, dependency direction and dependency policy
17. Verification program: oracle fixtures, known answers, determinism, the gauntlet
18. Privacy
19. Kill gates
20. Phases
21. Work breakdown (FTX-01 … FTX-24)
22. Risks and open questions
23. Explicit exclusions
24. Glossary
Appendix A. Sources

---

## 0. How to read this plan

FrankenTextiles turns a garment idea, a pattern sheet or an existing pattern into a sewing pattern that
**demonstrably sews together and fits a specific body**, and into the files a home sewist, a copy shop, a
cutting service or an embroidery machine needs. It is the mechanical core of the
`fabric-pattern-creation-and-alteration` agent skill, rebuilt in Rust so it is fast, deterministic, testable
against known answers, and runnable in a browser.

The plan has three tiers, kept distinguishable in code, docs and status reports:

| Tier | Meaning |
|---|---|
| **Solid** | Ports of behaviour the Python reference already has and tests (patterns, validation, drafting, exports, measurement fusion, photo scale). Success is measured against captured oracle fixtures. |
| **Frontier** | New capability the reference lacks and the project exists to provide: the parametric body model fitted to real measurements, the contact-correct drape solver, multi-frame photogrammetry of a moving child. Success is measured by known-answer tests and stated acceptance criteria. |
| **Optional research** | Ideas that may never ship: learned fabric parameters from drape videos, differentiable pattern optimisation through the solver, garment capture from a flat-lay photo. These never consume budget before the kill gates pass. |

Read §3 (the four claims) and §19 (kill gates) before anything else. The detailed behaviour specifications
extracted from the reference live in `docs/spec/01…04-*.md` (and the Anny forward model in `05`); this plan states architecture and decisions,
the specs state exact behaviour.

---

## 1. Product contract

### 1.1 Inputs

- **A garment source**, one of:
  - a parametric drafting program and its style parameters (a measured draft);
  - a `fabric-pattern/1` pattern JSON (from a previous round, an alteration, or another tool);
  - a pattern sheet image (an AI-generated or scanned sheet), traced into a pattern;
  - later: a flat-lay photo of a garment that fits (Optional research).
- **Body evidence**, any combination of:
  - tape measurements with provenance and uncertainty (`fabric-measurements/1`);
  - photos and short videos with a size reference (a printed ChArUco sheet, or a plain US Letter or A4 page);
  - age, sex, stature, weight or BMI only (a size-chart or population body, clearly labelled as such).
- **A fit contract** (`fabric-fit/1`): the points of measure and the ease bands the garment must meet.
- **Fabric**: areal density, warp/weft/bias stiffness, bending stiffness, thickness, friction against skin
  and against itself. Defaults are labelled UNCERTAIN until sourced.

### 1.2 Outputs

- The pattern (`fabric-pattern/1`), the fit contract, the fused measurements and the fitted body, per round.
- A verdict with its basis: flat validation, drape flags (gating and advisory), export acceptance checks and
  the measurement gate, each reported separately (§3).
- The drape: per-frame positions (PC2 and a compact viewer format), a 3D viewer page, strain and contact maps.
- Files: master SVG; tiled Letter and A4 PDFs with test squares and layers; A0 and 36 × 48 in copy-shop PDFs;
  projector PDFs; DXF-ASTM/AAMA (R12) and a flat DXF for laser front ends; HPGL; per-piece cutter SVGs;
  GarmentCode JSON; Tajima DST embroidery files for braid and trim; an export manifest and acceptance-check
  report.

### 1.3 Who uses it

- A parent or home sewist making clothes for a particular child or adult, through the skill or the browser
  page, with no install.
- An agent running the skill's round loop (draft → validate → drape → alter → repeat → export).
- A pattern maker or small producer who needs industry files and a believable fit check before cutting cloth.

### 1.4 What FrankenTextiles is not

- Not a CAD application with an interactive pattern editor (the browser page edits parameters, not curves).
- Not a fashion renderer: the drape exists to answer fit and construction questions, not to look pretty.
- Not a body-scanning product: it estimates the measurements a pattern needs, with stated uncertainty.
- Not a replacement for a muslin. The muslin and a photo of it on the wearer remain the truth (§3).
- Not a general cloth-simulation library. It simulates sewn garments on bodies, and is tested for that.

---

## 2. Where this comes from: the reference implementation and what it taught

The reference implementation is the Python code of the `fabric-pattern-creation-and-alteration` skill
(about 13,000 lines including tests, revision `88e03965`, 2026-10-12). It was built around a real case: an
AI image model turned a photo of a toddler's Zouave costume into a pattern sheet that looked convincing and
could not be sewn. The skill's round-0 audit of that sheet found side seams 8 % apart, a sleeve cap shorter
than its armhole, and printed dimensions contradicting each other by up to 57 %. It then drafted measured
patterns that pass every flat check, exported them, and folded them up in Blender.

What the reference established and FrankenTextiles keeps:
- **The pattern JSON is the editable explanation.** Lengths, ease and seam matching are decided on the flat
  pattern, where they are exact. One authority per number.
- **The round loop** with an explicit verdict basis, a measurement gate (a pattern fitted to size-chart
  guesses fits the chart, not the wearer) and export acceptance checks inside the round.
- **Fail closed**: an unscaled trace gets no true-size files; a crashed simulation never reports a previous
  run's result; a misspelt style parameter is an error.

What the reference showed does not work, and FrankenTextiles replaces:
- **The dress form.** The reference builds the body from stacked ellipse rings. Its armpit sits about 45 mm
  above the measured armscye level and its crotch path was 87 mm too long before a special solve; both made
  correct patterns look wrong in 3D. FrankenTextiles uses a real parametric body model (§9).
- **The cloth solver.** Blender's classic cloth needed a collider inset (else every girth gains 4.4 cm),
  pre-sewing and empirical relaxation to avoid launching the jacket, and still produced results that
  depended on the start state, jittered 0.2–0.5 mm per frame, and could not say whether an elastic waistband
  holds. FrankenTextiles uses an incremental-potential solver with barrier contact and Coulomb friction (§11).
- **The browser page** ran the Python under Pyodide (19 MB first load). FrankenTextiles compiles to wasm (§15).

---

## 3. The four claims, never conflated

| Claim | Meaning | Evidence that establishes it |
|---|---|---|
| **Parity** | A ported subsystem reproduces the reference (the Python skill, or upstream Anny) on the fixture corpus | Oracle fixtures captured from the pinned reference; differential tests pass |
| **Physics** | The drape solver behaves like cloth on a body where the answer is known | Known-answer tests with closed-form or measured thresholds (§11.9) |
| **Fit** | A garment fits a real person | A muslin or a dressed photo, compared against the round's predictions. Nothing computed establishes it |
| **Performance** | A stated workload runs in a stated time on stated hardware | A benchmark with machine fingerprint, inputs and spread |

Rules that follow:
1. Passing a parity test is not evidence the reference was right. Where the reference has a known quirk,
   the spec records it and the port either reproduces it under a named flag or fixes it with a regression
   test and a parity exception, never silently.
2. A drape with no gating flags is a prediction about construction and contact, not a fit claim.
3. A body model fitted to measurements fills everything unmeasured from its age prior. Such values are
   `MODEL_FIT` with their own uncertainty; they never masquerade as measured.
4. No speed claim without a benchmark. No "faster than Blender" before one exists.
5. A failure is a failed cell, never an excluded cell. Partial coverage never rounds up.

---

## 4. Oracles and pins

| Reference | Pin | Used for |
|---|---|---|
| Python reference implementation (the skill's `scripts/`) | revision `88e03965` (2026-10-12) | Parity fixtures for patterns, validation, alteration, drafting, measurements, photo scale, exports, tracing, the CLI's round logic |
| Anny parametric body model (NAVER LABS Europe), PyPI `anny==0.6.1` (2026-09-28), code Apache-2.0, MakeHuman/MPFB2-derived assets CC0 1.0 | wheel SHA-256 recorded in `tools/body-model/PIN.md` | Parity fixtures for the body forward model and its anthropometry; the converted model data |
| frankensim (same author, MIT + rider) | commit recorded per lifted file | Source of lifted numerics (sparse LDLᵀ, orthotropic membrane, barrier function, deterministic elementary functions, BVH); provenance header on every lifted file |
| ipc-toolkit (MIT) | version recorded in the drape crate's notes | Formula cross-check for contact distances and their derivatives (not vendored) |
| FFmpeg, OpenCV (ChArUco detection) | versions recorded in fixtures | External tools until a Rust replacement passes the same fixtures |

Rules:
- Fixtures are captured from the reference, stored under `tests/fixtures/oracle/`, and **never regenerated
  from the candidate**. A mismatch is a defect until proven otherwise.
- Moving a pin is a deliberate event: new fixtures, a diff of what changed, and the old results kept.
- The reference stays runnable alongside the port until the gauntlet (§17.6) certifies the subsystem.

---

## 5. Architecture overview

```text
 body evidence ──► ftx-measure ──► fused measurements (value, σ, provenance)
 (tape, photos,        │                    │
  video, age/BMI)      │                    ▼
                       └────────────► ftx-body ──► fitted body mesh + landmarks + MODEL_FIT values
                                              │
 garment source ──► ftx-trace / ftx-pattern ──┤──► pattern (fabric-pattern/1)
 (sheet image,       (draft, alter)           │          │
  program, JSON)                              │          ▼
                                              │     flat validation (ftx-pattern) ──► verdict basis
                                              │          │
                                              └──► ftx-drape (garment scene on the body) ──► drape report,
                                                         │                                    frames, viewer
                                                         ▼
                                                    ftx-export ──► SVG, PDFs, DXF, HPGL, cutter, DST, checks
```

The CLI (`ftx`) runs a **round**: measurement gate → flat validation → drape → exports, and writes one round
directory with a verdict that names its basis (§14). The browser page runs the same crates compiled to wasm.

Data contracts are files: `fabric-pattern/1`, `fabric-fit/1`, `fabric-measurements/1` (existing schemas,
specified in `docs/spec/01` and `02`), plus new `ftx-body/1` (a fitted body: model pin, parameters, pose,
landmarks, residuals), `ftx-drape-scene/1` and `ftx-drape-report/1` (§11.8).

---

## 6. Units, coordinates, determinism and refusals

- **Pattern files: millimetres**, 2D, y up, x to the wearer's right seen from the public side, closed loops
  counter-clockwise (exact conventions: `docs/spec/01`). **Bodies and drapes: metres, kilograms, seconds**,
  3D, z up, wearer facing −y (the reference's world frame). Conversions happen at one boundary each.
- **Determinism**: single-threaded fixed-order assembly in solvers; candidate lists sorted with
  `total_cmp` and index tie-breaks; no iteration over hash maps in anything that affects output;
  deterministic elementary functions (`ln`, `exp`, `atan2`, `acos`) lifted from frankensim instead of the
  platform maths library where results must match across native and wasm; no fused multiply-add in hot loops
  that must be bit-identical on wasm. Target: identical final-state digests native debug, native release and
  wasm (§17.5).
- **Refusals, never silent clamps**: typed refusal records with a stable code, a message and a ranked list of
  repairs, for non-finite input, degenerate geometry, an inward-wound body, an intersecting drape start, a
  candidate-budget overflow (refuse, never truncate), a Newton stall, a budget stop (publishes nothing
  partial), an unscaled pattern asked for true-size files, a misspelt parameter.
- **Rounding** where outputs must match the reference byte for byte (HPGL, DST, number formatting) follows
  the reference's ties-to-even behaviour, specified in `docs/spec/03`.

---

## 7. `ftx-core`

Shared foundations, no domain logic:
- 2D/3D vector and small-matrix types; deterministic elementary functions.
- Curves and polylines: chord-relative cubic and quadratic curves, dense sampling at the reference's sample
  counts (lengths depend on them; `docs/spec/01` §2), arclength, fractions, tangents, mirror and reverse.
- Polygons: signed area, point-in-polygon, horizontal width at a level, offsetting with mitre limits (the
  seam-allowance outline lives in `ftx-export` but uses these kernels).
- The shared JSON types (the edge `curve` object, refusals, receipts) with serde. The pattern and fit
  documents are typed in `ftx-pattern`, because their structure checks must read malformed input and
  report it with the reference's messages instead of failing to parse.
- Refusal and report types; a run receipt (input hashes, versions, parameters, output hash).

---

## 8. Patterns: model, validation, alteration, drafting (`ftx-pattern`)

Specified in `docs/spec/01-pattern-model-validation-alteration.md` and the drafting half of
`docs/spec/02-drafting-body-measurements.md`.

- **Model**: pieces, vertices, edges with curves, cut quantities, on-fold unfolding, mirror pairs into
  instances (`front.R`, `front.L`, `back` with `.R/.L` edge suffixes), seams with references
  (`INST/edgeA+edgeB`), ratios and ratio bands, notches, grain, internal lines, decor, drills, per-edge seam
  allowance, placement hints, roles, `sim_scale`, notes, metadata; every structure check with the reference's
  messages.
- **Validation**: the seam walk (notch-to-notch lengths against declared ratios within 3 mm), truing (angle
  sums where a garment line crosses a seam), points of measure (girth, width, vertical, path) against the
  body and target bands, the status logic, the default fit spec by role, and the text table.
- **Alteration**: slash-spread, move, true-seam, cap-ease, transfer-curve, with the reference's history
  records.
- **Drafting programs**: the Zouave bolero (with its soutache layout), the full gathered trousers, the fez,
  every style parameter and solve. Drafting reads body values through one interface, so a program drafts
  the same way from tape measurements, from a fitted body, or from the reference's analytic landmarks.
- **Decisions on reference quirks** (from the spec): the role-based default fit spec's bands differ from the
  drafted specs (keep, documented); cap-ease balances the total, not each half (keep, flag); true-seam trues
  to the band midpoint when a band exists (keep); bisection silently returns the upper bound with no root
  (**fix**: refuse with a typed error; parity exception recorded); several malformed inputs crash the
  reference (**fix**: structure errors). Each decision is a line in `FEATURE_PARITY.md`.

---

## 9. Bodies: a real parametric body model (`ftx-body`)

### 9.1 Why Anny

The body must be anatomically right where fit is judged (armpit, crotch, shoulder slope, belly, seat) for
babies, toddlers, children, teenagers and adults of either sex, parameterisable from very little (age, sex,
height, weight or BMI) up to many tape measurements, and fittable to photos. The research behind the skill
(its research file 03) compared SMPL/SMPL-X/SMIL (non-commercial licences), MakeHuman/MPFB2 (CC0 assets,
ages 1–10 interpolated rather than calibrated) and **Anny** (NAVER LABS Europe, arXiv 2511.03589): a
differentiable model built from MakeHuman/MPFB2 assets, covering newborn to elderly, with height-for-age
calibrated to WHO data, scan-fit error 2.0 mm on child scans versus 3.2 mm for SMPL-X (paper §5), code
Apache-2.0 and assets CC0. Anny is the default; its optional SMPL-X topology download is non-commercial and
excluded.

Checked on 2026-10-12 with `anny==0.6.1` on CPU: six phenotype parameters (gender, age, muscle, weight,
height, proportions), 256 local shape changes including twenty `measure-*` targets (bust, underbust, waist,
hips, neck, nape-to-waist, waist-to-hip, shoulder width, front chest, upper arm, lower arm, wrist, thigh,
knee, calf, ankle, upper and lower leg heights), a 13,718-vertex quad mesh, a 163-bone default rig, about
7 ms per forward pass after loading. Gender 0 is male and 1 female; the age parameter is morphological
(WHO-calibrated map: 0 y → 0.0, 1 y → 0.05, 4 y → 0.215, 11 y → 0.415, 16 y → 0.67, 18 y → 0.77,
30 y → 0.786, 64 y → 0.83). At average build a girl of 2.5 years is 94.4 cm tall with a 45.9 cm waist and
11.4 kg, a boy of 6 is 131.6 cm, a man of 30 is 189.8 cm and a woman of 30 is 176.4 cm
(`docs/spec/05-anny-forward-model.md` §8). Its local changes are fixed-size offsets (+1 on the waist adds
about 10 cm at any age), so fitting priors must scale with body size; its weight and muscle age priors look
unreliable and only the height prior is used.

### 9.2 Port: the forward model, not the training code

Essence extraction of Anny's forward model:
1. **Data conversion** (`tools/body-model/convert_anny.py`, Python, run once per pin): build the model data
   from the pinned package's bundled MakeHuman/MPFB2 assets into a fresh cache directory (the package's own
   cache key ignores its version; `data/cached/anny.pth` is not what this configuration uses), check the
   hashes listed in `docs/spec/05` §8.1, and export the template mesh, the phenotype blend shapes, the
   local-change targets, the rig (bone heads, tails and the float32 bone-roll matrices exactly as stored) and
   the skinning weights into a versioned little-endian file (`*.ftxbody`, with a JSON header naming the pin
   and every array's shape and hash). The blend shapes are 374 MB dense in float64; stored sparse as 16-bit
   integers × 0.1 mm (exact for all but the derived newborn rows) they are about 80 MB, and 216 of 1,136 rows
   (breast shapes, always zero in this configuration) are dropped. Converted data is never committed (§18;
   `.gitignore`); the pin and the converter are.
2. **Forward model in Rust**: phenotype interpolation exactly as Anny computes it, local changes, rest
   skeleton, pose (linear blend skinning first; dual-quaternion only if a fixture needs it), triangulation.
3. **Parity gate G1a**: vertices within 0.05 mm of PyTorch Anny for a fixture set of phenotypes, local
   changes and poses (random and edge cases: newborn, toddler, adult, extremes), and identical anthropometry
   (height, waist circumference, volume, mass, BMI) within float tolerance.

### 9.3 Measuring a body the way a tape does

Fitting needs the measurements of the model's surface, defined like the tape measurements they are compared
with (`docs/spec/02`, catalogue): a girth is the length of a convex hull of a plane section at a landmark
level (a tape bridges hollows), a length runs along the surface or straight as the definition says, and
landmark levels come from anatomical vertex sets on the template (chest at the armpit, natural waist,
seat, crotch, side neck point, shoulder tip, C7). These functionals are differentiable almost everywhere
and are themselves tested against the reference's slicing on fixture meshes.

### 9.4 Fitting

- **From little** (age, sex, stature, weight or BMI): solve the phenotype parameters for height and mass
  with the WHO-calibrated age prior; report every other measurement as `MODEL_FIT` with the population
  spread as its σ.
- **From tape measurements**: Gauss–Newton / Levenberg–Marquardt over phenotype parameters and the
  `measure-*` local changes, minimising σ-weighted residuals between the measured values and the model's
  tape functionals, with priors on parameters. Gate G1b: on synthetic targets drawn from the model itself,
  recovered girths within 3 mm and lengths within 2 mm; on held-out measurements, residuals reported, never
  hidden.
- **From photos and video** (§10): add 2D keypoint and silhouette terms per frame with a camera from the
  reference sheet; shape shared across frames, pose per frame.
- **Output** `ftx-body/1`: pin, parameters, pose, landmark heights and positions, the triangle mesh for the
  drape (outward-wound, checked), residuals per measurement and `MODEL_FIT` values with σ for everything the
  pattern needs that was not measured.

### 9.5 Poses

A-pose with a stated arm abduction (the drape's default 35°, plus 50° and arms-up for ride-up checks), a
seated pose and a squat (rise and seat ease are judged there), and a "nappy" seat-volume local change for
toddlers. Poses are kinematic in the drape: the body can move along a pose ramp while the garment follows.

---

## 10. Measurement and photogrammetry (`ftx-measure`)

### 10.1 Port of the reference (Solid)

Specified in `docs/spec/02`: the 35-entry measurement catalogue (definitions, how-to text, tape and target
σ), size-chart priors by stature and age, fusion (repeats, z-score thresholds 2 and 3, REMEASURE, Z_WARN,
PRIOR_ONLY, VERIFY_OUT_OF_BOUNDS), the readiness gate per garment kind with its re-measure requests, the
reference wearer, the ChArUco calibration sheet and its sidecar, photo scale from the sheet (homography,
residuals, warnings), the plain Letter/A4 sheet detector (masks, hull quad, sub-pixel edge lines, rectangle
residual with the focal search, `RECT_OK = 0.025`, every warning), and `measure-photo` with bootstrap σ.

### 10.2 Photogrammetry of a moving child (Frontier)

The two video-reconstruction skills surveyed for porting (research file 08) assume a static scene seen by a
moving camera. A toddler moves, so body measurement is **not** structure-from-motion. The design:
1. **Frame identity and selection**: frames identified by decoded timestamp (never a frame number), with the
   rotation record kept; sharpness scored separately inside the person mask and on the sheet, over
   0.1–0.25 s windows.
2. **A camera per frame from the sheet**: a planar pose solver for the sheet (its own solver and checks; the
   video skills' held-out pose check rejects planar point sets on purpose), lens distortion with an inverse,
   and detection of a zoom or lens switch mid-clip.
3. **Still windows**: detect stretches where the child barely moves; triangulate landmarks only inside them,
   with gates (ray angle ≥ 3°, reprojection error ≤ 3 px), or across two synchronized phones.
4. **A small factor-graph solver** with an explicit gauge and a metric-scale parameter, fit/held-out
   partitions and "unobservable" states, lifted in spirit from the interior skill: the engine for sheet
   calibration, landmark triangulation and the body fit.
5. **Body fit across frames**: shape shared, pose per frame, keypoint and silhouette terms. 2D keypoint and
   mask models (Apache-2.0 families such as RTMPose/MediaPipe and SAM-class masks) run as external ONNX
   models the user installs; never bundled.
6. **Reports what the data cannot separate**: a single front photo cannot separate body width from focal
   length (the drone skill's fixture shows a 5 mm lens change and a 10 % width change moving landmarks
   identically); the multistart pattern reports such ambiguity instead of picking one.

Order of the port (research file 08): conventions and receipts → camera core (distortion, projection, plane
homography, similarity fits, gated triangulation, RANSAC plane, homography scale and focal search) → frame
selection → factor graph and bundle adjustment → ambiguity tools → mesh checks, plane slices and silhouette
comparison. Their synthetic tests with exact answers become conformance tests.

External tools stay external: FFmpeg for decoding, OpenCV for ChArUco detection until a Rust detector passes
the reference's synthetic photo tests, COLMAP/GLOMAP only for an optional static-background reconstruction.

---

## 11. Draping: a contact-correct cloth solver (`ftx-drape`)

The design follows research file 07 (a survey of frankensim for this purpose).

### 11.1 What it must fix

| Blender failure (recorded in the reference) | Mechanism in `ftx-drape` |
|---|---|
| Start-state sensitivity; a strained start launches the jacket | Each step minimises an incremental potential with a backtracking line search: the potential never rises; a high-strain start relaxes |
| Collision jitter 0.2–0.5 mm/frame; "settled" was a drift statistic | Newton converges each step to a tolerance; settled is a residual and velocity criterion |
| Collider inset (else every girth gains 4.4 cm) | Barrier on the gap minus the real cloth half-thickness; clearance reported exactly |
| Cloth squeezed into the arm–torso wedge unstable | One smooth barrier potential; an equilibrium exists whenever a feasible gap exists |
| Cannot judge whether an elastic waistband holds | Coulomb friction with lagged normal forces plus a tension-only prestressed elastic ring; closed-form known answers (§11.9) |
| Isotropic springs | Orthotropic membrane (warp, weft, bias shear) rotated to each piece's grainline |
| Gathers via vertex-group stiffness | Per-material-direction tension/compression split, a material field |
| No intersection check | Exact triangle–triangle audit of the start and end states |
| Needs a Blender install; not in a browser | Std-only Rust core: a CLI and a wasm build |

What it does not fix: cloth energy is non-convex, so different starts can settle in different local minima.
Every drape reports a **two-start spread** as its error bar.

### 11.2 Energy and step

Per step of size h: predict `x̃ = xₙ + h vₙ + h² g`; minimise
`E(x) = ½h⁻²(x − x̃)ᵀM(x − x̃) + Ψ_membrane + Ψ_bend + Ψ_elastic + Ψ_pins + Σ κ b(d_k − ξ_k) + Σ D_k(x)`
with lumped mass, the IPC barrier on thickness-offset gaps active below d̂ (0.5–1 mm), and lagged friction
`D_k`. Projected Newton: assemble gradient and Hessian (element blocks projected to PSD, or project only when
the LDLᵀ inertia reports a negative eigenvalue), solve with sparse LDLᵀ reusing the symbolic analysis while
the sparsity pattern is unchanged, cap the step at 0.8 × the CCD time of impact, Armijo backtracking, stop on
‖p‖∞/h < tol. `vₙ₊₁ = (x − xₙ)/h`.

### 11.3 Schedule

Sew (seams welded, zero gravity, small h, until start strain relaxes) → drape (gravity) → settle (h grows
from 1/30 s to 0.25 s until the maximum speed stays below a threshold for K steps) → optional static solve
reporting the equilibrium itself.

### 11.4 The garment scene

- **Meshing**: piece outlines to triangles (boundary samples at the reference's spacing, hex interior,
  constrained Delaunay via `spade`), 2D rest coordinates kept per vertex.
- **Seams**: matched vertex pairs welded into single vertices (exact constraints, no stiff springs); gathered
  seams welded too, with compression softening near them as a material field (the reference's
  ratio > 1.15 rule).
- **Start state**: the reference's isometric wrap of each piece around its body part plus pre-sewing (sewing
  groups moved to centroids outside the body, displacement spread harmonically), pushed outside the body,
  then the exact audit. An intersecting start is refused with ranked repairs; a fallback IPC-native sewing
  phase (zero-rest-length stitches ramped in at zero gravity, then welded) exists for hard cases.
- **Elastic**: a tension-only ring of casing vertices with rest length shorter than the casing.
- **Body**: the fitted `ftx-body` mesh, static or on a pose ramp, BVH built once; cloth–body pairs never
  change the Hessian pattern. Self-contact (sleeve against bodice, overlapping fronts, cuff folds) through a
  refitted feature BVH.

### 11.5 Lifted from frankensim (provenance header on each file)

| Source | Use |
|---|---|
| `fs-sparse/src/direct.rs` (fill-reducing ordering, sparse LDLᵀ with inertia) and `lib.rs` assembly | Linear solves; ordering on the vertex graph cuts setup 5–9× at garment scale in the survey's scratch benchmark |
| `fs-plate/src/shell/preload.rs` + `shell.rs` (orthotropic large-rotation triangle membrane, exact stiffness) | Membrane energy |
| `fs-plate/src/lib.rs` grain-angle rotation; `shell/preload/tethers.rs` tension-only spring | Grainlines; elastic |
| `fs-solid/src/contact.rs` `Barrier` and line-search/friction structure | Barrier function; template for 3D |
| `fs-la/src/eigen.rs` `jacobi_eigh` | PSD projection |
| `fs-math/src/det.rs` | Deterministic elementary functions |
| `fs-query/src/features.rs`, `fs-rep-mesh` BVH and point-triangle distance, `winding.rs` | Broad phase, body queries, inside tests |
| `fs-ad` dual numbers and gradcheck | Derivative oracle in tests |
| `fs-ivl` predicates + `fs-topo` intersect | Exact intersection audit |

New code: point–triangle and edge–edge distances with gradients and Hessians (the main correctness risk;
ipc-toolkit is the formula reference, dual numbers the test oracle), additive CCD, quadratic bending, lagged
friction in 3D.

### 11.6 Metrics and flags (preserved from the reference)

Every metric and flag the reference reports is defined in solver-independent terms in `docs/spec/04` and
kept, with the same gating/advisory split and flag text (tightness demotion matches flags by text): seam
closure, per-seam strain, landmark ring strain and contact, penetration, corner slip, hem heights, rise and
balance, band positions (casing, cuffs), settledness, form crotch excess. New quantities are added, not
substituted: exact minimum gap ever, contact pressure maps, the elastic's slip over the hold window, the
two-start spread.

### 11.7 Performance targets (not measurements)

A toddler jacket (~2,200 vertices) and gathered trousers (~3,700) settled on a fitted body in under one
minute native on an M4-class laptop, and in under five minutes in a browser, for the default schedule. The
survey measured one LDLᵀ factorization of a 9,408-unknown system at 85–260 ms on a heavily loaded machine;
these targets are budgets until benchmarked (§3).

### 11.8 Files

`ftx-drape-scene/1` (scene JSON + little-endian sidecar), `ftx-drape-report/1` (per-step energy, Newton
iterations, contacts, minimum gap, friction state, maximum speed; final metrics and flags; a `no_claim`
line), `fold.pc2` and `mesh_map.json` in the reference's formats (so existing tools keep working), and the
viewer format `fabric-fold-viewer/1`.

### 11.9 Known-answer tests (gate G2)

1. Barrier and contact calculus: every element energy's gradient and Hessian match dual numbers to 1e-8
   relative, per pair type and degenerate case.
2. Membrane anisotropy: uniaxial stretch along warp, weft and bias reproduces the input moduli; rigid motion
   stores no energy.
3. Peirce cantilever (ASTM D1388 style): the 41.5° droop overhang returns the input bending stiffness within a
   few percent.
4. Ring on a cylinder with a hanging load: holds iff `W < 2πμT`, threshold within ±10 %.
5. Ring on a cone narrowing downward: slides iff `tan α > μ`, independent of T.
6. Ease sweep on a torso tube: mean clearance `(C − G)/2π − ξ`; a negative-ease tube stays on with hoop
   strain ≈ −e/G; no inset needed.
7. Elastic ladder on the fitted toddler: 0.85, 0.95, 1.0 and 1.2 × waist read hold, hold, hold, fall at two
   friction values (μ UNCERTAIN until sourced).
8. Armpit wedge: the bolero at three sizes on fitted bodies at 35°, no gating flags, hem rise within ±15 mm
   of the drafted level.
9. Determinism: identical final digests across native debug, native release and wasm.
10. Two-start spread stated and bounded.

---

## 12. Exports and manufacturing files (`ftx-export`)

Specified in `docs/spec/03-exports.md` (all 51 acceptance checks, every target's structure, the PDF writer,
the Helvetica width table and the single-stroke font as data). Decisions from the spec:
- Write the real DXF extents (the reference's DXF library overwrote them with ±1e20 placeholders; no check
  reads them).
- PDF and DXF creation dates come from the pattern's `created` field, so every file is reproducible
  (the reference used the wall clock); parity is then structural for those files, byte-exact elsewhere.
- The PDF tiles and the master SVG share one vertex list (the re-stitch check compares them directly).
- A missing listed file fails its check instead of crashing the checker; colliding cutter/DST file names get
  a deterministic suffix.
- Tajima DST for braid couching per piece: placement run, stop, couching run, needle-down after jumps, 1 mm
  tie-offs, hoop table, README; verified against an independent decoder.

---

## 13. Tracing pattern sheets (`ftx-trace`)

Specified in `docs/spec/04` (vectorize): thresholds, contour extraction, dashed-line bridging, piece/edge/
corner labelling, `names.json`/`labels.json`, annotate, the round-0 audit metrics, scale-fit, overlays.
Image decoding and processing in Rust (PNG/JPEG decode, thresholds, connected components, contour tracing,
polygon simplification); the vision-language step that names pieces stays an agent/API step outside the
crate. Traced patterns stay `UNSCALED` until scale-fit; exports refuse them (§6).

---

## 14. The `ftx` command line and the skill boundary (`ftx-cli`)

`ftx` mirrors the reference's `fab` commands so the skill's workflow does not change:
`draft`, `round`, `validate`, `drape` (replacing `sim`; `qualify` becomes known-answer bands on the fitted
body), `body fit|show|measure`, `measure …` (catalogue, prior, add, fuse, ready, photo-scale, measure-photo,
calib-sheet, video), `export`, `trace …`, `alter …`, `outfit`, `viewer`, `report`. Every command prints
human text and, with `--json`, a stable JSON envelope; exit codes are documented (0 converged / pass,
2 not converged or flags, 3 refused input, 4 budget stop).

The round keeps the reference's semantics (`docs/spec/04`): a round directory per round, `rounds.jsonl`, the
status precedence (BLOCKED → UNSCALED → flat status → drape gating → export checks → measurement gate →
CONVERGED), the basis line, the measurement gate and qualification rules.

**The skill boundary**: the skill keeps the workflow, judgement and documentation (how to measure a child,
when to regenerate an image, how to read flags, the case study). Its Python scripts become thin wrappers that
call `ftx` subsystem by subsystem as each reaches parity, and are retired only after the gauntlet certifies
that subsystem. Image generation (codex `$imagegen`) and Blender stay in the skill.

---

## 15. The browser (`ftx-wasm`)

The same crates compiled to `wasm32-unknown-unknown` with a thin `wasm-bindgen` boundary (numerical crates
never see browser types). Replaces the Pyodide page: draft, validate, measurement gate and exports in
milliseconds with a small download; the fitted body from tape measurements; a drape of a small garment in
the page with a step/progress interface and positions handed to a Three.js view as a `Float32Array`. The
converted body-model data is fetched once from the user's own installation or a CC0-clean hosted copy, never
bundled with a person's data. Gate G4.

---

## 16. Workspace crates, budgets, dependency direction and dependency policy

Ten crates, not a tower of tiny layers. A crate joins the workspace when it gets real code. No crate outside
this table without explicit approval.

| Crate | Responsibility | Planned Rust lines incl. tests |
|---|---|---:|
| `ftx-core` | Units, deterministic maths, curves/polylines/polygons, schemas, refusals, receipts | 8,000 |
| `ftx-pattern` | Pattern model, instances, validation, alteration, drafting programs | 16,000 |
| `ftx-body` | Anny forward model, tape functionals, fitting, poses, landmarks | 9,000 |
| `ftx-measure` | Catalogue, priors, fusion, readiness, photo scale, photogrammetry | 16,000 |
| `ftx-drape` | Garment meshing and scene, contact cloth solver, metrics and flags | 14,000 |
| `ftx-export` | SVG, PDF, DXF, HPGL, cutter, GarmentCode, DST, acceptance checks | 14,000 |
| `ftx-trace` | Tracing pattern sheets | 7,000 |
| `ftx-cli` | The `ftx` binary, rounds, reports | 4,000 |
| `ftx-wasm` | Browser bindings | 3,000 |
| `ftx-conformance` | Oracle fixtures, differential, known-answer and determinism tests | 9,000 |
| **Planned** | | **100,000** |
| Contingency | | 20,000 |
| **Ceiling** | | **120,000** |

Dependency direction:

```text
core <- pattern
core <- body
core <- measure            (measure uses body for model fits through a trait, not a cycle)
core/pattern/body <- drape
core/pattern <- export
core/pattern <- trace
everything <- cli, wasm, conformance
```

Dependency policy: a small pinned allowlist. `serde`/`serde_json` (files only, never inside a solver loop),
`spade` (constrained Delaunay), PNG/JPEG decoding crates for tracing and photos, `wasm-bindgen`/`js-sys`
in `ftx-wasm` only. Forbidden in the library crates: Python at runtime, PyTorch, BLAS/LAPACK, C/C++ FFI,
Tokio (nothing here is async), a mandatory Rayon pool (parallelism, if any, behind a feature with a
deterministic reduction). Each candidate crate records its version, used API, wasm build result, transitive
closure and demonstrated benefit. Sibling Franken projects are resources, not assumptions: frankensim code
is lifted file by file with provenance, not depended on as a workspace.

---

## 17. Verification program

### 17.1 Oracle fixtures (Parity)

`tools/oracle/` holds Python capture scripts run against the pinned references; they write
`tests/fixtures/oracle/<subsystem>/<case>.json` (inputs, outputs, reference pin, capture date). The specs list
the cases (`docs/spec/01` §conformance: 42 standalone fixtures, golden tables for the three drafted garments,
12 mutation cases; `02`: the reference wearer and its drafts, fusion and readiness outputs, photo-scale
results with image generators; `03`: the 26 export tests, golden values, byte/structural/tolerance classes;
`04`: CLI, round logic and the recorded simulator readings). Comparison classes: byte-exact,
canonical-structural (parsed and normalised), tolerance (stated per quantity).

### 17.2 Known answers (Physics)

§11.9 for the drape; for the body, synthetic recover-the-parameters tests; for photogrammetry, the synthetic
scenes with exact answers from the reference and the video skills.

### 17.3 Properties and metamorphic relations

Mirror symmetry of left/right instances, invariance of validation under rigid motion of a piece, scaling a
pattern and a body together leaves ease ratios unchanged, re-ordering seams leaves the verdict unchanged,
exports of a pattern and its re-import round-trip.

### 17.4 Feature parity tracking

`FEATURE_PARITY.md`: one row per reference feature with `present | partial | missing | excluded` and the
fixture IDs that prove it. Partial never rounds up.

### 17.5 Determinism lanes

Two native runs, native debug versus release, and wasm in Node versus native must produce identical digests
for the drape, the body fit and every export.

### 17.6 The gauntlet (release gate G5)

The `running-the-gauntlet-on-your-rust-port` methodology, in `gauntlet-full` mode for ported subsystems and
`gauntlet-greenfield` mode for the frontier subsystems (body fit, drape, photogrammetry): a sibling
`franken_textiles__gauntlet_workspace/`, three negative ledgers (performance, conformance, surface), at least
ten rounds with two consecutive clean rounds and every open hypothesis resolved, and the final report,
parity runbook and release certification. It runs when there is a port to certify (after Phase 2), not
before.

---

## 18. Privacy

The usual wearer is a young child. Therefore:
- No real person's measurements, photos, videos, scans or name are ever committed: fixtures are synthetic or
  the public-prior reference wearer; `.gitignore` excludes the obvious media and `private/` paths.
- Everything runs locally; the browser page uploads nothing.
- Fitted bodies of real people are user data in user workspaces, never in caches inside the repository.
- External models for keypoints and masks are opt-in installs with their licences shown.

---

## 19. Kill gates

1. **G1 Body**: (a) the Rust Anny forward model matches PyTorch Anny within 0.05 mm on the fixture set;
   (b) fitting recovers synthetic tape targets within 3 mm (girths) and 2 mm (lengths). If (a) fails after a
   reasonable effort, ship the body through a converted mesh cache rather than a Rust forward model and
   reopen the browser plan.
2. **G2 Drape**: §11.9 tests 1–7 pass, and the converged bolero and trousers settle on fitted toddler bodies
   with no gating flags within the §11.7 budget. If contact or performance cannot be made to work, keep the
   Blender path for draping and say so; do not ship a solver that is merely different.
3. **G3 Port slice**: patterns, validation, drafting and exports pass their oracle fixtures, and the skill's
   round runs end to end through `ftx`.
4. **G4 Browser**: the wasm page drafts, validates and exports with parity, and drapes a small garment.
5. **G5 Gauntlet**: convergence per §17.6 before a 1.0 release.

---

## 20. Phases

| Phase | Content | Exit |
|---|---|---|
| **0** | Workspace, `ftx-core`, oracle capture harness; Anny conversion and Rust forward model; drape core without contact (membrane, bending, sparse LDLᵀ, Newton) with tests 2–3 | G1a; drape tests 2–3 |
| **1** | Pattern model, validation, alteration, drafting, measurements (fusion, readiness); body tape functionals and fitting; contact (distances, barrier, CCD, BVH, friction), garment scene, metrics and flags | G1b; drape tests 1, 4–6; parity fixtures for spec 01–02 |
| **2** | Exports (SVG, PDF, DXF, HPGL, cutter, GarmentCode, DST, checks); the `ftx` CLI and round; the skill calls `ftx`; drape on fitted bodies | G2; G3 |
| **3** | Photo scale (homography, plain sheet, ChArUco through an external detector), photogrammetry core, multi-frame body fit | Synthetic-scene fixtures; a real-photo check with a ruler in frame |
| **4** | Tracing; wasm and the browser page; outfit and viewer from `ftx` | G4 |
| **5** | The gauntlet; retirement of the reference's ported scripts; release | G5 |

Build Phase 0 first. Frontier work never borrows from the parity budget, and optional research waits for
G2.

---

## 21. Work breakdown (bead titles)

| ID | Unit |
|---|---|
| FTX-01 | Workspace bootstrap and `ftx-core` (vectors, deterministic maths, curves, polylines, polygons, refusals, receipts) |
| FTX-02 | Oracle capture harness (`tools/oracle/`) and `ftx-conformance` skeleton with the first fixtures |
| FTX-03 | Pattern model, instances and structure checks (spec 01) |
| FTX-04 | Validation: seam walk, truing, points of measure, status, table |
| FTX-05 | Alteration operations |
| FTX-06 | Measurement catalogue, priors, fusion, readiness, reference wearer (spec 02) |
| FTX-07 | Drafting programs (bolero with soutache, trousers, fez) and the reference's analytic landmarks for compatibility |
| FTX-08 | Anny conversion tool and the Rust forward model (G1a) |
| FTX-09 | Tape functionals on the body and fitting from tape and from age/sex/height/weight/BMI (G1b) |
| FTX-10 | Poses (arm angles, seated, squat, nappy volume) and the body export for the drape |
| FTX-11 | Drape core: inertia, orthotropic membrane, bending, sparse LDLᵀ, projected Newton; tests 2–3 |
| FTX-12 | Contact: distances with derivatives, barrier, CCD, BVH, lagged friction; tests 1, 4–6 |
| FTX-13 | Garment scene: meshing, wrap and pre-sew start, welded seams, gathers, elastic; outputs and metrics/flags per spec 04 |
| FTX-14 | Drape on fitted bodies: elastic ladder, armpit wedge, two-start spread (G2) |
| FTX-15 | Export geometry and SVG: seam-allowance outlines, notches, grain, labels, layouts |
| FTX-16 | PDF writer: tiles, copy shop, projector, layers, test squares |
| FTX-17 | DXF (ASTM/AAMA, flat), HPGL, cutter, GarmentCode, DST and the acceptance checks (G3 with FTX-18) |
| FTX-18 | The `ftx` CLI and round; the skill's scripts call `ftx` |
| FTX-19 | Photo scale: homography, plain sheet detector, ChArUco via external detector |
| FTX-20 | Photogrammetry core: frame identity and selection, cameras, planar sheet pose, still windows, gated triangulation, factor graph |
| FTX-21 | Multi-frame body fit to keypoints and silhouettes (external 2D models, opt-in) |
| FTX-22 | Tracing pattern sheets (spec 04 vectorize) |
| FTX-23 | `ftx-wasm` and the browser page; drape in the page (G4) |
| FTX-24 | The gauntlet and release (G5) |

---

## 22. Risks and open questions

- **Fabric data.** No sourced warp/weft/bias/bending parameters or fabric–skin friction yet; all UNCERTAIN.
  A research pass (Kawabata-style data, published drape tests) is required before fit-sensitive gates.
- **Contact derivatives** are the main correctness risk (edge–edge mollifier); mitigated by the dual-number
  oracle and formula cross-checks.
- **Self-contact cost**: factorization rose 3–6× in a pessimistic test; PCG with incomplete factorization as a
  fallback inner solver, coarser spacing, symbolic reuse.
- **Friction creep floor**: a held band still creeps at about the friction smoothing velocity; "holds" is
  slip below a threshold over a fixed simulated time, calibrated by the ring-on-cylinder test.
- **Local minima** remain; the two-start spread and the muslin stay.
- **Anny for children**: calibrated for height-for-age; how well its shape space covers individual toddlers'
  bellies, seats and nappies is unproven; local changes and photo terms are the remedy, residuals the report.
- **A moving toddler**: still windows may be short or absent; the protocol (tape first, photos second) stays.
- **wasm performance**: an estimated 1.5–2× slower than native without fused multiply-add; to be measured.
- **Body-model size in the browser**: about 80 MB sparse even quantized. Options: an age-band subset of the
  blend shapes, a coarser fitting mesh with the full mesh only for the drape, or fetching the converted data
  from the user's own installation. Decided at G4, measured, not guessed.
- **The reference keeps moving** while the port happens; pins and fixture refreshes are deliberate events.

---

## 23. Explicit exclusions

Not ported, by decision:
- Blender's cloth mechanics (pre-sew/relax hacks, collider inset, GN pass-through modifier, `.blend` files);
  the drape is native. Blender remains available to the skill for renders.
- The analytic stacked-ellipse dress form mesh. Its landmark and section formulas are ported (drafted
  patterns depend on their values), its mesh is replaced by Anny.
- The Pyodide browser page (replaced by wasm).
- Image generation through codex `$imagegen` and any chat-app workflow (they stay in the skill).
- SMPL/SMPL-X/SMIL and any non-commercial model or checkpoint as a default; they may be opt-in user installs
  with a licence notice, never bundled.
- House- and drone-specific reconstruction from the video skills (map assembly, rooms, walls, roofs).

---

## 24. Glossary

- **Ease**: finished garment measurement minus body measurement at the same point.
- **Seam walk**: comparing the lengths of the two edges sewn together, notch to notch.
- **Truing**: making garment lines run smoothly across a seam (corner angles summing to 180°).
- **Incremental potential**: the energy minimised once per implicit time step (inertia plus elastic plus
  contact plus friction).
- **IPC**: Incremental Potential Contact, barrier-based contact that guarantees no interpenetration.
- **Muslin**: a test garment in cheap cloth; the ground truth for fit.
- **MODEL_FIT**: a measurement read off a fitted body model rather than measured.

---

## Appendix A. Sources

- Brégier et al., "Human Mesh Modeling for Anny Body", arXiv 2511.03589 (2025); github.com/naver/anny.
- Li et al., "Incremental Potential Contact", SIGGRAPH 2020; Li et al., "Codimensional Incremental Potential
  Contact" (C-IPC), 2021; ipc-toolkit (MIT).
- The reference implementation's behaviour specifications: `docs/spec/01…04`; the Anny forward model: `docs/spec/05`.
- Research behind the reference (body measurement and body models; formats and manufacturing; Blender
  sewing; the frankensim cloth survey; the photogrammetry port survey), summarised where used above.
