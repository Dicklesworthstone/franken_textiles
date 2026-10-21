# Comprehensive Plan for the Design of FrankenTextiles

Version 1.1 · 2026-10-13 · plan-first: nothing below is implemented unless a later commit says so.
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
25. Source-bearing textile platform, material substitution and manufacturing
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
| **3** | Photo scale (homography, plain sheet detector, ChArUco through an external detector), photogrammetry core, multi-frame body fit | Synthetic-scene fixtures; a real-photo check with a ruler in frame |
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
| FTX-12 | Contact: distances with derivatives, barrier, CCD, BVH, friction; tests 1, 4–6 |
| FTX-13 | Garment scene: meshing, wrap and pre-sew start, welded seams, gathers, elastic; outputs and metrics/flags per spec 04 |
| FTX-14 | Drape on fitted bodies: elastic ladder, armpit wedge, two-start spread (G2) |
| FTX-15 | Export geometry and SVG: seam-allowance outlines, notches, grain, labels, layouts |
| FTX-16 | PDF writer: tiles, copy shop, projector, layers, test squares |
| FTX-17 | DXF (ASTM/AAMA, flat), HPGL, cutter, GarmentCode, DST and the acceptance checks (G3 with FTX-18) |
| FTX-18 | The `ftx` CLI and round; the skill's scripts call `ftx` |
| FTX-19 | Photo scale: homography, plain sheet detector, ChArUco via external detector |
| FTX-20 | Photogrammetry core: frame identity and selection, cameras, planar sheet pose, still windows, gated triangulation, factor graph |
| FTX-21 | Multi-frame body fit to keypoints and silhouettes (external 2D models, opt-in) |
| FTX-22 | Tracing pattern sheets (`ftx-trace`) |
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

## 25. Source-bearing textile platform, material substitution and manufacturing

### 25.1 Expanded product direction and one physical authority

The next product horizon is a general textile design, alteration, simulation and optimization system,
with complex costumes on human bodies as the first demanding application. It must also admit other
woven, knitted, bonded, felted, hide, laminate and filled assemblies. This deliberately expands the
garment-only scope stated in §1.4; it does not weaken the parity, physics, fit or performance claims in §3.
Robotic cutting and sewing are future consumers of a verified assembly/process model, not a claim that
exporting a cutter outline is already a manufacturing robot.

The central acceptance scenario is a fixed jacket pattern on a fixed fitted body. Replace characterized
linen with characterized cow leather, then with a characterized performance knit/blend. Without editing
solver code, all applicable consumers must use the selected material: inertia, warp/weft/wale/course and
bias response, grain rotation, bending, thickness, contact/friction, sewing/gathers, strain/fit reports,
layer insulation, purchasing and eventually manufacturing constraints. Pattern geometry remains fixed
unless an explicit subsequent design optimization changes it. Every run records the resolved material
snapshot, source evidence, estimate policy, model limits and digest; no stale cached defaults survive a
substitution. A missing property blocks only consumers that require it and is reported as unknown to
the others; an incomplete commercial record is not made simulation-ready with invented values.

The frankensim materials-card/consumer boundary is an architectural reference. Shared numerics can be
lifted only with pins, provenance and tests, not by adding sibling workspace dependencies. Cross-domain
catalog and discovery ideas from music_theory_data_tool are inspiration, not empirical textile evidence.
Keep one canonical material-resolution boundary within the existing crates and front ends; external
data collectors publish immutable inputs, never participate in the solver's time stepping.

### 25.2 Database coverage and hierarchy

Represent distinct levels rather than conflating a fiber with a finished fabric: constituent fiber;
spun/filament yarn and sewing thread; woven/knitted/nonwoven sheet; hide and tanning/finish; laminate;
loose fill or batt; and a garment's ordered, mechanically distinct layer assembly. Stable identifiers
distinguish supplier, product, variant, batch, construction, finish and conditioning. Cotton, polyester,
nylon, elastane, flax/linen, wool species, silk, hemp, cashmere, alpaca and blends belong in the family
index, as do regenerated cellulose types (viscose/modal/lyocell) and true bamboo bast. A bamboo-derived
viscose yarn is not silently relabelled bamboo bast. Down/feather mixtures, polyester fill, cotton/wool
batts and technical insulation need fill-specific records, not shell parameters borrowed from woven cloth.
Performance brands such as Vuori are recorded only at their published product/composition scope; a
brand name or four-way-stretch claim is not a measured constitutive law or a raw-fabric sales quote.

The database is sparse and extensible, not a table padded with zeroes. Each quantitative observation
needs its unit, method, direction, specimen/conditioning context, measured/published/estimated evidence
class, source locator and retrieval/measurement date, plus bounds or uncertainty when reported. Preserve
original units and ranges alongside normalized values. Do not choose an unreported range midpoint or
turn a confidence interval into a tolerance. Track source rights and permitted retention; prefer primary
supplier specifications, open research datasets and reproducible specimen measurements.

Required property families include:
- Identity/composition and construction: fiber mass fractions; yarn tex/denier/count system, twist,
  ply/filament, spinning; warp/weft counts, weave, crimp, knit gauge/loop structure; bonding, pile,
  coating, tanning, wash/softener and surface treatment; sheet width, thickness under stated pressure,
  dry/conditioned areal mass and bulk/fiber density kept distinct.
- Mechanics/durability: directional stress-strain and shear curves, recovery and hysteresis, creep,
  anisotropic bending, compression/loft recovery, tensile/tear/puncture/cut resistance, abrasion/pilling,
  seam slippage/strength, dimensional change, wet response and interface-specific friction. Test-standard
  results carry their specimen geometry and conditioning; they are not interchangeable scalar strengths.
- Transport/safety: dry thermal conductivity/resistance, heat capacity, vapor resistance, permeability,
  sorption/drying, wind and moisture effects, down fill power/loft, ignition/burn observations, melt/drip,
  UV and chemical/process restrictions. Warmth marketing labels, fill power, fiber composition and areal
  mass do not become a wearer temperature rating or a flammability/cut-protection certification.
- Commercial/process observations: dated supplier SKU quotes, quality/region/currency/terms, actual sale
  quantity, width, minimum and increment, availability and evidence hashes; sourced cutting, needle/thread,
  stitch/feed/tension, pressing/adhesive and finishing constraints where documented.

### 25.3 Constitutive and heterogeneous assembly work

The initial effective law resolves areal mass, E-warp/E-weft, Poisson coupling, engineering shear,
independent scalar bending, full thickness and skin/self friction. It respects material axes already
rotated by the pattern grainline. Positive-definiteness, finite values, dimensions and explicit estimate
admission are gates, not suggestions. Bulk fiber modulus in Pa is not sheet stiffness in N/m; solid-plate
thickness-cubed bending is not automatically valid for a yarn/loop network.

Next, add per-piece and per-layer assignments, per-element constitutive data and mass, adjacent-material
bending rules, and interface-pair contact properties. Body offset is one half-thickness; two different
sheets use the sum of their half-thicknesses. One global self-friction coefficient cannot characterize
leather against satin and knit against knit. Preserve seam associations, grain/mirror transformations,
gather compression and material hashes through remeshing and motion continuation. A lining, shell and
fill have distinct roles; loose fill must not impersonate a membrane. Until implemented, heterogeneous
assignments must refuse rather than silently use the global material.

Then introduce independently calibrated nonlinear woven/knit response, tension/compression asymmetry,
shear locking, history-dependent recovery, damage and directional bending, with conditioning and strain
range validity. The constitutive interface must let solver assembly remain generic. Derivative checks,
energy/objectivity tests, known-answer patch tests and measured drape/stretch validation precede claims
that a construction descriptor alone predicts fabric behavior.

### 25.4 Thermal coupling, purchasing and machine-aware design

The current dry one-dimensional thermal stack is a comparison calculator, not body thermoregulation.
The next thermal model must use actual layer thickness/compression, air gaps from the draped geometry,
wind-dependent boundary films, moisture/evaporation, thermal bridges at seams and closures, transient
storage and explicit wearer/environment boundary conditions. Validate dry layers first, then coupled
transport, against stated measurements. Report local heat flow and uncertainty before any comfort index;
do not claim a safe outdoor temperature from a static cloth drape or an unverified branded textile.

Purchasing remains a source-observation problem separate from material physics. Compare only matching
currency, region, quality, sale basis and tax/shipping terms; convert linear yard/metre prices through
sourced usable width; retain order minima/increments and sample counts. Distinguish observed-offer
p10/median/p90 from supplier-balanced statistics and from a representative market survey. Never infer
exchange rates, hide yield or unknown shipping costs. Refresh must preserve raw evidence, timestamps,
source policies and old snapshots on failure. Broaden beyond the initial Shopify adapter with validated
JSON-LD, CSV and supplier-specific importers, actual primary-source manifests and scheduled refresh only
when configured. Intraday quote-history semantics require a deliberate schema extension, not timestamp
rewriting. Material choice should eventually drive nesting/BOM requirements and actual procurement cost.

Manufacturing planning must connect material, construction and machine capability. Add seam topology,
thread/needle/stitch/feed models, seam allowances and bulk, reinforcement, stretch differential, pucker,
cutting direction/nap, defects and usable yield. Optimize cut layouts and sewing order against explicit
quality/time/cost objectives and uncertainty. A robot execution adapter needs machine-specific capability
limits, calibration, workholding/sensing, collision checks, interlocks and an explicit execution boundary.
An unknown coating, finish or ignition characteristic cannot authorize thermal cutting or processing.
Keep geometric export acceptance separate from process feasibility and from permission to operate hardware.

### 25.5 Delivery state and next acceptance gates

Implemented foundation: `ftx-pattern::materials` source-bearing records, units/evidence validation,
effective orthotropic mechanics and dry series-layer heat-flow checks (commit `0d2f5e8`); the subsequent
shared catalog/selection/import/drape resolver and `ftx-materials` workbench are the consumer path.
`scripts/refresh_textile_prices.py` emits that workbench's existing `ftx-textile-quotes/1` schema with
exact-variant/currency/basis checks and immutable evidence (commit `b82536e`). Its 15 offline tests passed;
live supplier endpoints were inaccessible from the implementation environment, so this is not a claim
of a successfully populated live market feed. See `docs/spec/06-textile-materials.md` and
`docs/textile-price-refresh.md`. Existing family and supplier records are partial observations, not a
comprehensive characterized library. No nonlinear construction, heterogeneous thermal comfort or robot
execution capability is asserted by these foundational commits.

The `ftx materials` CLI now exposes the unified, built-in source-bearing catalog through
`list`, `show ID`, `compare FIRST_ID SECOND_ID` and `freeze ID`. Comparison reports only
common, unit-matched properties for which evidence is admitted, along with independent
mechanics readiness and missing-coefficient lists; it makes no false garment-performance
claim. Freeze emits a content-locked, portable selection compatible with the existing
`fabric.material` resolver. This is the operator-facing entry point for the material
substitution workflow, **not** evidence that commercially sourced mechanical properties
are complete or that a linen-to-leather jacket drape has been validated.

Next practical product gate: take two fully characterized sheet records, freeze both,
substitute the `fabric.material` value on an identical pattern, and assert identical
pattern geometry and different predicted mass/contact/orthotropic energies and material
digest. Only after that extend to construction-specific nonlinear mechanics, local
layer-stacks and machine-process constraints. The critical data dependency is obtaining
real specimen-level stiffness, bending, interface-friction and thickness measurements;
supplier fiber labels alone cannot unblock the solver.

Track the expansion in beads `ftx-tm0` through `ftx-tm8`. Prioritize (1) measured, construction-specific
material coverage, (2) heterogeneous solver/contact assembly, and (3) the fixed-pattern substitution gate.
That gate must verify unchanged geometry, changed mass and directional energy, contact offsets/friction,
different reproducible dynamics, correct source hashes and no stale material reuse. Add thermal-stack
and price-width known answers, native/wasm determinism, explicit incomplete-data refusals and separately
reported real-specimen validation. Measured linen/leather/knit jacket comparisons are the eventual
end-to-end acceptance, not a synthetic test dressed up as real-world fit evidence.

### 25.6 Material-following thermal surfaces: delivered path and remaining physical gates

The dry thermal foundation now reaches actual garment geometry, rather than remaining an isolated
catalogue calculator. Commits `fb055935`, `faca56e7` and `bd07cad0` add conservative surface columns,
automatic characterized-drape exchange and `ftx-materials thermal-surface`. Each retained solver
triangle carries its reference area, welded vertex indices, instance owner and the same frozen
material observations/evidence policy as mechanics. Heat exchange uses current area; sensible heat
storage uses reference mass. A change of pose cannot create material. Thermal-only material changes
change heat flow/capacity while leaving mechanical trajectories unchanged; mechanics changes can
also alter the deformed exchange geometry. Portable reports bind material, topology and positions,
and refuse modified receipts rather than resolving a newer catalogue entry by name.

This delivery is an explicitly constant-resistance, independent dry-sheet model, not a whole-wearer
heat-loss or comfort calculation. The library admits explicitly coextensive series layers, but the
drape adapter does not infer a shell/lining stack merely because triangles are nearby. Missing heat
capacity or resistance leaves mechanics available and records thermal unavailability. The command
produces per-triangle steady flux/temperature fields and bounded transient energy histories on the
final drape geometry. See `docs/thermal-surface.md` for equations, units, source replay, budgets and
limitations. Ten new synthetic numerical, actual sewn-drape propagation and CLI tests cover this
slice; they do not establish agreement with a measured textile or a real jacket. Bead `ftx-tm9`
records this delivered slice; the wider `ftx-tm5` remains open.

The next material/transport work is separated into independently falsifiable gates:

1. **Condition-qualified measured data (`ftx-tma`, P0).** Extend the shared materials system with
   temperature/conditioning/pressure-dependent observations, not a second unrelated catalogue.
   Preserve specimen identity, test geometry, original mean and separately labelled standard
   deviation, exact source/version and permitted retention. Distinguish sample variability from
   uncertainty of the mean and confidence intervals. Interpolation is an explicit fitted/estimated
   operation with source provenance and validity domain; never extrapolate silently, mix dried and
   unconditioned data, or equate fibrous bulk density with solid-fiber density. Qualify scalar and
   temperature-dependent consumers separately, including missing heat-capacity refusals.
2. **Garment/body and interlayer topology (`ftx-tmb`).** Introduce explicit layer order and attachment
   identities, body coverage, shared overlap areas, exposed faces and material-following air-gap
   geometry. Preserve energy across many-to-many overlaps and changing poses; count one covered
   body patch once, not once per sheet. Ambiguous or intersecting layering must refuse. Establish
   analytic flat multilayer and partial-coverage cases before claiming total wearer heat loss, and
   retain the independent-sheet mode as a distinct, reproducible calculation.
3. **Measured compression and coupled transport (`ftx-tmc`).** Add pressure/strain-dependent loft,
   thickness, conductivity and moisture storage/transport only from characterized constructions.
   Couple film/radiative/evaporative terms to declared environment conditions; preserve reference
   mass and sensible/latent energy and explicitly account for mechanical heating when supported.
   Validate compressed batts, wet/dry layers and transient response against held-out measurements,
   with discretization convergence and uncertainty. A physical validation failure is not repaired
   by silently fitting the held-out case or replacing unknown data with a generic fiber constant.

Primary data leads inspected on 2026-10-14 include FSRI's Materials and Products Database:
`https://materials.fsri.org/materialdetail/cotton-sheet`,
`https://materials.fsri.org/materialdetail/cotton-batting`, and
`https://materials.fsri.org/materialdetail/overstuffed-chair-polyester-batting`, with source data in
`https://github.com/ulfsri/fsri_materials_database`. These retain materially different temperature
and conditioning coverage; the polyester-batting page, for example, must not be assigned cotton's
heat capacity. NISTIR 7282 is another primary lead for measured clothing-material thermophysical
properties (`https://www.nist.gov/publications/estimates-thermal-properties-fire-fighters-protective-clothing-materials-nistir-7282`).
These are acquisition leads, not numbers already ingested or validated by the thermal-surface work.
Neither a fire-test observation nor a dry heat map grants a safety rating or machine-process approval.

### 25.7 Temperature-qualified transport: implemented path and empirical handoff

Commits `02dd3b2` and `f451e8d` extend the section 25.6 delivery beyond constant properties.
The shared `Material` now retains specimen- and conditioning-qualified temperature observations,
reported means and separately labelled standard deviations. The catalog adds five FSRI cotton-sheet,
cotton-batting and chair-polyester-batting records with 25 temperature-specific observations, bringing
the canonical seed to 41 records; legacy compatibility imports remain additional discovery records.
Dried and unconditioned specimens remain distinct. These numbers are now ingested, superseding the
acquisition-only status above, but matching conditioned HFM geometry and areal mass are still missing.
Two records also lack specific heat. None of these five is represented as a complete drape or thermal
simulation input, and none supplies a generic cotton/polyester or branded-performance-fabric law.

The implementation compiles explicitly admitted piecewise-linear conductivity and specific heat,
integrates sensible heat against reference mass, and enforces the intersection of the characterized
temperature intervals. Interpolation is an estimated modeling choice even when its knots were
measured. Companion mass/geometry observations must match the specimen and conditioning; scalar
conflicts, absent data, nonconvergence and extrapolation refuse. Reported variability is not silently
converted to confidence intervals or propagated uncertainty. The solver uses a bounded conservative
enthalpy backward-Euler iteration with checks of the original local equations, global energy balance
and maximum principle; constant-only models keep their original arithmetic and snapshot format.

The same selected record now drives the dry transient CLI, actual sewn-drape thermal exchange,
material-following surface columns and source-locked replay without solver edits. A thermal-only
record change leaves mechanical trajectories/pattern geometry unchanged, changes heat transport and
invalidates the previous thermal state. Surface heat storage follows undeformed mass even when
current exchange area changes. See `docs/temperature-dependent-textiles.md` and the executable
`tests/fixtures/materials/synthetic-variable-thermal.json`. Analytic enthalpy/Fourier answers,
spatial-refinement checks, mixed-material boundary reversal, moving-surface conservation and actual
sewn-drape propagation are software acceptance, not measured garment validation or a warmth rating.
Native tests and wasm32 compilation are not a claim of cross-platform execution parity.

Record this completed software slice in `ftx-tmd`; keep the broader material/transport, real-specimen
and fixed-pattern garment gates open. The immediate empirical handoff is:

1. **Matched raw characterization (`ftx-tme`, P0, under `ftx-tma`).** Acquire specimen/lot identity,
   ply count, actual HFM thickness, mass/area, direction, contact pressure, conditioning protocol,
   replicate information, temperature range and source revision/retention terms together. Import and
   replay the raw observations against independently transcribed tables. Derive areal mass only
   from matching geometry and mass, not a different test's nominal density. Dried cotton-batting and
   polyester-batting heat capacity remain absent until characterized; extend cold-weather validity
   with measurements rather than extrapolating room-temperature curves.
2. **Independent physical acceptance (`ftx-tmf`, P0).** Validate held-out coupon and sewn-layer
   heat-flux and transient-temperature data with declared geometry/interfaces and conditioning.
   Separate discretization error, sample variability and measurement uncertainty; retain replicate
   counts/covariance when estimating uncertainty. Do not tune against the held-out acceptance case.
   Compare measured prediction residuals and coverage, not merely a small numerical energy residual.

In parallel, the highest-value mechanical dependency remains `ftx-tm2`: fully characterize linen,
treated cowhide and performance-knit specimens, then execute the unchanged-pattern mechanical and
thermal acceptance in `ftx-tm8`. Explicit body/layer overlap topology (`ftx-tmb`) and characterized
compression/moisture laws (`ftx-tmc`) are still required before whole-wearer heat loss. Supplier
distribution coverage/refresh (`ftx-tm6`) and material-qualified cutting/sewing constraints (`ftx-tm7`)
remain separate product gates. Neither this temperature-law delivery nor its partial catalog authorizes
robot execution, establishes fire/cut resistance, or supplies an outdoor comfort/safety temperature.

---

### 25.8 Measured catalog breadth and material-aware sewing: delivered integration and next gates

The 2026-10-14 continuation delivers two additional parts of the requested common materials
platform, without introducing another material database or a parallel physics authority:

- **Measured specimen breadth (`629694b`).** The canonical built-in catalog now has 81 records,
  including 30 distinct specimens and 60 published observations from the MIT ICCV2013 fabric
  dataset. The primary table covers linen, cotton, wool, silk, Lycra-labelled specimens, denim,
  corduroy, fleece, faux fur, velvet, canvas, nylon rip stop, terry knit, laminated cotton, pleather,
  backed vinyl and upholstery. Original units, specimen IDs, aggregation qualifiers and source
  citations survive normalized selection and portable snapshots. These are numerical summary-table
  facts, not an ingested raw-replicate archive or permission to redistribute the source media.
  Test-specimen flexural rigidity is `N*m2`, not sheet bending rigidity `N*m`; absent coupon geometry
  prevents automatic conversion. Labels do not establish blend fractions. See
  `docs/mit-fabric-measurements.md`. The 30 records remain partially characterized, not new fully
  drape-ready or thermal-ready presets.
- **Material-following sewing (`7444388`, `e6a1567`; `ftx-sp1`).** `ftx-materials sew-plan` and
  `ftx_export::sewing::SewPlan` resolve the same per-piece selections used by draping and cutting.
  Actual seam curves, reverse/mirror conventions, edge junctions and matched notches determine
  paired stations and local differential feed. Explicit complete seam passes retain a deterministic
  precedence order, needle/thread identifiers, declared ply counts, stitch limits and machine
  envelope. Sourced thickness and its reported upper bound, when present, determine the uncompressed
  stack; edge/piece/export allowances and both-side advances are checked without changing geometry.
  A material-only change can therefore invalidate machine feasibility without solver or pattern
  edits. Simulation-only omission/scaling cannot alter manufacturing pieces. Unknown thickness,
  implicit layer stacks, unmatched notches, cyclic/partial jobs and excessive allocations refuse.
  `docs/material-sewing-plans.md` provides the executable fixture and exact command.

These deliveries extend the end-to-end material-swap architecture requested in §25.1–25.4. They do
not turn a fiber name, marketing warmth claim or partially measured specimen into an effective
fabric law. Existing purchasing history/percentiles, roll cutting, nonlinear mechanics and thermal
transport continue to use their existing source-bearing material paths. Any new property must
retain specimen/construction, dimensional meaning, conditions and evidence policy before a relevant
consumer can use it; otherwise that consumer remains unavailable rather than guessing.

**Next high-impact acceptance gates:**

1. **Raw specimen upgrade (`ftx-md1`, under `ftx-tm2`).** Obtain retained source data and matched
   coupon geometry, direction, conditioning and replicate metadata. Reproduce the published table
   independently, and derive effective sheet parameters only with the required geometry and
   uncertainty. Complete matched thickness, membrane response, directional bending and pair friction
   for linen, treated cowhide and performance-knit specimens; do not combine similarly named lots.
2. **Qualified sewing and machine handoff (`ftx-sp2`, under `ftx-tm7`).** Add measured seam coupons
   tied to exact lots/finishes, layer order, needle, thread, stitch class, feed/tension/speed,
   workholding and presser-foot compression. Extend one forward pass to explicit multi-pass,
   locking/backtack and fold recipes with physically accessible assembly topology. Compare held-out
   pucker, seam slippage/strength and penetration measurements; material substitution must rerun
   these qualification checks. Calibrated machine coordinates, kinematics, collision checks,
   sensing, interlocks and operator-approved envelopes precede any hardware execution.
3. **Unchanged-pattern empirical comparison (`ftx-tm8`).** Use those matched records to test the
   same jacket on the same body across linen, cowhide and performance knit, including mechanical
   drape, thermal exchange and manufacturability. Retain the independent thermal gates in §25.7,
   and distinguish software propagation/conservation from agreement with measured garments.

The sewing output always sets `hardware_execution_authorized` and
`physical_seam_quality_validated` to false. Eight core and three real-binary regressions establish
software behavior, including material-swap refusal and identical CLI/library output. The measured
catalog tranche has separate identity/unit/provenance tests. Neither these tests nor passing native
or wasm builds closes the physical-validation, whole-wearer warmth, fire/cut-resistance or qualified
robotics gates. `ftx-tm0`, `ftx-tm2`, `ftx-tm7` and `ftx-tm8` remain open.

---

### 25.9 Capability-specific garment exchange and the next material-swap delivery

The section 25.1 rule that missing data blocks only the consumers that need it now reaches geometric
garment/body exchange. `SteadyCoverage` (`bae3525`) accepts the same frozen sheet selections and
shared fractional coverage layout without requiring heat capacity or inventing transient storage.
Each series layer uses its allocated current cloth area; films and interfaces use the covered body
area. Covered body area is counted once, while uncovered fractions remain explicit bare parallel
paths. Temperature-qualified conductivity uses the existing integrated Fourier solver, per-material
domains and original-equation residuals. Frozen observations are never rewritten to account for a
pose or an area change. The output exposes unassigned cloth rather than silently counting it as body
coverage. This closes a software dependency, not a material-characterization gap.

The shared geometric bridge (`cbeafb4`) reuses actual retained cloth triangles, local-normal polygon
clipping, inverse cloth allocations, physical layer order and finite-thickness gap evaluation for
both storage-bearing and conduction-only surfaces. `7750faf` connects it to the native and browser
workbench through one file-free request evaluator. Both `ftx materials thermal-steady-coverage` and
`ftx-materials thermal-steady-coverage` accept a saved drape, explicit body/layer map and boundary
conditions, or a source-complete frozen replay request. The wasm-bindgen export is
`thermal_steady_coverage`. Every evaluation recomputes coverage from its retained geometry and
selected material records; it does not reuse an older pose's polygons or substitute a newer catalog
entry behind a stable name. Spatially varying boundary conditions are bound to the exact body mesh.
Unrelated saved-report metadata is omitted from the portable request. See
`docs/steady-thermal-coverage.md` and its executable fixtures.

Material substitution therefore has a further end-to-end consumer: change the existing pattern
selection, rerun draping, and the same new report updates geometric overlap, source-thickness-driven
admitted gas gaps, series transport and signed body heat fields without changing solver code. A
transport-only substitution on an actual sewn-drape fixture preserves mechanical trajectories and
clipped footprints while changing power and source identities. This test uses synthetic coefficients
and a local prescribed-temperature body patch. It is not an empirical linen/leather/knit jacket
comparison. A conductivity-only specimen may also be evaluated on explicitly supplied geometry,
but it does not acquire missing mechanical parameters or become drape-ready.

**Data and capability policy remains unchanged.** The database coverage in section 25.2 applies to
fiber, yarn/thread, woven and knitted constructions, nonwovens, hides/finishes, laminates and fills.
Cotton, polyester/blends, wool, linen, regenerated-cellulose and bamboo distinctions, silk, leather,
performance-branded products, down and synthetic/cotton/wool fill must remain construction- and
specimen-specific. Source-bearing property families and dated comparable-offer p10/median/p90 are
still required; a populated identity record is not a complete effective fabric law. This delivery
adds no invented material measurements, new worldwide price survey or generic brand-based physics.
One canonical material authority continues to feed mechanical, transport, procurement and process
consumers, each with its own readiness and missing-data report.

The following focused beads supplement, rather than close, the larger material-platform gates:

1. **Delivered conductivity-only coverage (`ftx-cv1`).** Shared area allocation, exact steady series
   transport, retained-geometry projection, native/browser workbench and frozen replay. Numerical
   checks include unequal areas, partial exposure, nonlinear conductivity, source-qualified HFM
   transport without heat capacity, material substitution and explicit malformed/stale-input
   refusals. Physical validation and whole-wearer claims remain outside this completed slice.
2. **Qualified full-garment body coverage (`ftx-cv2`, P0; supports `ftx-tmb` and `ftx-tm8`).** Extend
   the local synthetic propagation proof to independently checked full-jacket topology and measured
   boundary exchange using matched material lots. Test body winding, self-intersections, open
   boundaries, folds, sewn layers, exposed areas and pose changes. Report cloth/body allocation,
   spatial convergence, horizon/normal-closure sensitivity and all unresolved regions. Compare
   local heat fields and total exchange against held-out measurements with declared uncertainty;
   do not fit the held-out garment. Couple to the matched data gates `ftx-tm2` and `ftx-tmf`.
3. **Qualified exterior convection and radiation (`ftx-cv3`, P1; a concrete `ftx-tmc` slice).** Add
   environment-dependent boundary exchange driven by declared air state/wind and source-qualified
   surface properties, including finish-dependent emissivity. Solve boundary face temperatures
   consistently with the existing nonlinear material columns. Separate convection, radiation and
   conduction terms, enforce their validity domains, and retain signed energy residuals. Establish
   limiting-case, reversed-flow and held-out boundary experiments before replacing prescribed films
   or publishing warmth indices. Moisture, compression and human thermoregulation remain separate
   gates; a dry gas approximation is not silently promoted to a ventilated garment model.
4. **Integrated browser material-swap workbench (`ftx-cv4`, P1).** Use the existing shared resolution
   and wasm APIs to select source-locked shell/lining/fill records, show capability-specific gaps,
   rerun drape and coverage, and inspect changed mechanics, heat, cut requirements, comparable cost
   and sewing feasibility for the same authored pattern. Invalidate outputs when their material,
   pose, environment or process inputs change; never patch missing coefficients from a family label.
   Build the operator workflow rather than a second database or browser-only physics implementation.
   Keep machine planning distinct from hardware execution authorization.

The higher-priority matched specimen characterization, uncertainty-qualified experimental results
and qualified manufacturing work in `ftx-tm2`, `ftx-tme`, `ftx-tmf`, `ftx-md1` and `ftx-sp2` remain
open. All current coverage results explicitly decline comfort, empirical validation and propagated
measurement-uncertainty claims. A complete material-driven product requires these data and physical
acceptance gates, not merely additional property names or a more convincing visualization.

---

### 25.10 Source-qualified optimization and fixed-pattern material studies

The material platform must make the same authored jacket, costume or other textile assembly usable
as a controlled design experiment: change source-locked shell/lining/fill selections and recompute
the relevant consequences, rather than creating a separate physics implementation or pretending a
fiber label determines fabric behavior. Sections 25.1–25.9 remain the governing requirements for
all fiber/yarn, woven/knit, nonwoven, hide, laminate and fill families, physical attributes, comparable
live offers, specimen identity, conditioning, uncertainty and empirical qualification. The hierarchy
must retain yarn construction, thread density, weave/knit, finish, thickness, layer role and effective
measured response. Marketing names, blend percentages and cheap missing-value defaults are not
substitutes for these data.

**Delivered software path (`ftx-ms1`).** `ftx_pattern::material_screen` applies canonical-unit scalar
constraints, exact method/condition restrictions and explicit estimate consent to the existing
catalog or embedded selections. Nominal comparison and conservative reported-interval comparison
are explicit alternatives. Missing bounds never become zero-width intervals. Pareto dominance
requires all objectives to be no worse, at least one strictly better, and matching full reported
protocols unless cross-protocol comparison is explicitly admitted. Ties, overlapping intervals and
incomparable protocols remain visible; there is no invented aggregate textile score. Compiler
readiness invokes the real mechanical, steady-conduction or transient-thermal implementations,
without requiring unrelated measurements. Every admitted selection is frozen for existing bindings.

`ftx_export::material_study` evaluates up to 32 explicit or screened alternatives against ONE
original pattern and shared process options. Default edits preserve explicit lining/cuff assignments;
named-piece edits target source IDs. Geometry, seams, grainlines and unrelated process fields are
unchanged. Every variant starts from the original, never the preceding variant. Actual `CutPlan`,
`SewPlan` and marker-derived procurement rerun independently: a thickness-based sewing refusal
does not erase valid cutting, missing width does not prevent sewing, and absent prices or incomplete
mass coverage remain unknown. Gross marker stock includes waste and sale increments; feed-only
time is not robot cycle time. Scalar-frontier pruning before manufacturing requires explicit consent,
since a scalar-dominated textile can still have a better marker or whole-order cost. The report retains
all screening exclusions, individual consumer outcomes, source-complete bound `pattern_json` and
`frozen_changes` that replay against the original pattern.

Both `ftx materials screen/study` and `ftx-materials screen/study`, plus the browser-callable
`material_screen` / `material_study` exports, use these same file-free engines. See
`docs/material-screening-and-studies.md` and its executable insulation examples. This slice adds
browser APIs, not selection-study controls to the existing browser workbench. It performs real
manufacturing evaluations but does not yet execute a batch drape/body/environment study. Its
source-locked patterns can enter the existing drape and thermal paths without solver changes.
The first three implementation commits are `38f7f75`, `7e333a5` and `d3c221a`; their complete
workspace run passed 1,127 Rust tests with no failures and 28 pre-existing ignores, plus strict
Clippy, wasm32 compilation, 49 Python and 18 Node tests. These are software checks, not measured
material, garment, price-market or machine-performance validation.

**Source expansion with explicit limits.** The built-in catalog now also includes the M1
cotton/polyester/elastane twill specimen from Mahnić Naglić, Petrak and Tomljenović (2025),
DOI `10.3390/polym17152013`, retaining construction, measured mass/thickness/thread density,
conditioning and original KES observations. See `docs/measured-woven-m1.md`. The thickness is
pressure-qualified. Original KES extensions are not silently converted to sheet laws, tensile
elongation at a reported load is not breaking strain, and AI-derived CAD coefficients are not
relabeled measured. Missing stock width, friction, thermal properties and comparable offers remain
missing. One specimen measured by three characterization methods remains one specimen, not three
independent fabrics. This adds usable source-qualified discovery/screening data, not a fully
characterized linen/leather/performance-knit substitution benchmark.

**Next highest-impact implementation and evidence gates:**

1. **Scenario-qualified multiphysics optimization (`ftx-ms2`, P0; supports `ftx-tm0`/`ftx-tm8`).**
   Extend the existing study composition, not the solver models: retain exact body/pose, geometry,
   environment and process policies; rerun actual drape, source-thickness-aware coverage, qualified
   boundary transport and manufacturing for each alternative. Refuse stale cached poses/heat fields
   after material edits. Carry independent consumer failures and capability gaps into the results.
   Optimize declared fit/strain, signed heat exchange, mass, manufacturing and dated cost objectives
   only where each result has admitted source data and a valid operating domain. Keep garment-level
   comparisons separate from scalar screening. Preserve Pareto alternatives and explicit uncertainty;
   no warmth index, safety rating or global optimum claim from incomplete input. Integrate a browser
   study workflow with source/condition inspection and output invalidation, reusing these wasm APIs.
2. **Resolve raw woven characterization and conversions (`ftx-ms3`, P1; supports `ftx-tm2`).**
   Obtain primary raw M1 observations or author clarification for KES load normalization, effective
   bending dimensions and the CAD conversion discrepancy, and units for the adapted Fabric Kit
   force table. Retain coupon geometry, direction, conditioning, replicates and uncertainty. Reproduce
   the published observations before deriving material-law parameters, never copying a CAD tuning
   coefficient into a physical tensor by name alone. Use existing calibration APIs, source locks and
   explicit estimate admission for fitted laws, then test held-out coupons. Extend the same matched-
   specimen acquisition route to linen, treated cowhide and performance knit under `ftx-tm2`; do not
   close that larger gate with one partially characterized blend or synthetic numerical tests.
3. **Measured garment and process acceptance remains blocking.** Preserve `ftx-tm8`, `ftx-tme`,
   `ftx-tmf`, `ftx-md1`, `ftx-sp2` and the relevant `ftx-tm6` supplier gates. Refresh live offers through
   retained, rights-aware adapters, match exact SKU/lot/width and currency/region/quality/order terms,
   and compare dated observed p10/median/p90 and marker-derived whole-order costs without claiming
   population percentiles. Calibrate sewing recipes against measured stacks, workholding, needle,
   thread, feed and compression; hardware needs its own kinematics, sensing, collisions, interlocks
   and approval. Material changes must rerun these checks rather than inheriting an earlier pass.

`geometry_changed`, `physical_validation_claim` and `hardware_execution_authorized` remain false
for these studies. Software propagation and deterministic source replay are necessary foundations;
only matched measurements and held-out experiments can establish the empirical jacket/costume
fit, thermal and manufacturing accuracy sought by the overall platform.

---

### 25.11 Purchasable textile records and complete-history design objectives

The source-qualified catalog and the fixed-pattern workbench are the same material platform.
Supplier records must enter the existing `Material` authority, retain original units and evidence,
and reach each consumer through its normal frozen selection. A new product must not require a
brand-specific branch in mechanics, heat, cutting, sewing or purchasing. Construction and specimen
data remain essential: fiber percentages, marketing stretch and a product name do not determine an
effective sheet law.

**Purchasable source expansion (`ftx-sr1`).** The built-in catalog contains 96 records, including
twelve additional exact supplier products with 21 numeric source observations. This tranche covers
linen, cotton poplin/interlock, bamboo-derived rayon/cotton/elastane jersey, silk charmeuse, merino
knit/felt, cotton sewing thread and batting, two separately identified Polartec mesh constructions,
and treated goose down. It preserves supplier identity, SKU or explicit variant, composition,
construction, original quantities, source URL, access date and source-page digest. Yarn linear density
remains yarn data; fill-power and marketing stretch labels do not become shell mechanics or thermal
ratings. Nominal fabric width does not assert measured defect-free cutting width. These are published
supplier observations with capability-specific gaps, not twelve fully measured simulation materials.
See `docs/supplier-apparel-catalog.md`.

The reviewed four-offer manifest in `docs/examples/textile-supplier-refresh.json` uses the existing
Product/Offer adapter and quote schema. Exact SKU, currency, stock, sale quantity and reviewed
description checks remain mandatory. An optional exact HTML-text check also protects selling-unit
evidence that the supplier places outside its structured product description. Source changes refuse
publication; they never silently reinterpret a half-metre price as a full-metre price. Requests obey
the applicable supplier robots rules, including redirect destinations and longest-path precedence.
New runs retain immutable dated quotes and raw evidence separately; no previous observation is
overwritten or made current by changing its date. CAD and USD, different textile variants and different
market/order terms remain separate samples. One offer's p10/median/p90 is a one-observation summary,
not a world-market distribution. An out-of-stock observation remains unavailable.

**Final-state objectives on complete histories (`ftx-mo1`, supports `ftx-ms2`).** The existing
scenario comparison admits complete motion and transient sheet/body-domain histories. It now also
exposes each thermal domain's signed net boundary energy and final minimum/maximum cloth temperature,
alongside inward energy, retained-history temperature extrema and peak panel-edge strain. A final
temperature constraint is distinct from an extreme reached earlier in the motion. All readings come
from the admitted production history; the browser displays the Rust frontier and `motion_required`
scope rather than computing another score. Initial static objectives retain their initial-frame basis.

Admission requires a settled, gating-clear initial drape and the complete, converged requested path.
Transient objectives additionally require matching policy, duration, source geometry and every
accepted mechanical sample. A requested failed heat history cannot qualify through successful static
heat, and the final moving state need not be at rest. Sheet and body-coverage energies remain separate
domains; stored heat, bare-body exchange, remap residuals and numerical balance error remain explicit.
Negative temperatures and reversed energy are valid within their declared domains. The same native
and wasm engines recompute material substitutions without editing solver code. Synthetic model-backed
replay verifies software propagation; it does not establish measured jacket fit or warmth.

**Remaining highest-impact delivery gates:**

1. Complete matched linen, treated cowhide and performance-knit characterization under `ftx-tm2`,
   `ftx-md1`, `ftx-tme` and `ftx-ms3`: exact lot/finish, conditioned thickness/areal mass, directional
   load-extension/shear/bending, pair friction and compatible thermal observations. Use primary raw
   data and held-out coupons; unrelated lots or proprietary brand descriptions cannot fill gaps.
2. Add reviewable supplier specification revisions (`ftx-sr2`). Compare newly observed composition,
   construction, sale basis and nominal dimensions against the exact retained product revision before
   accepting updates. Preserve frozen studies and old prices, distinguish product/SKU from measured
   specimen/lot, and require explicit review for changed identities or specifications. Extend actual
   supplier coverage only with unambiguous variants, order tiers, currencies and permitted sources.
3. Use those matched records for the unchanged jacket/body experiment in `ftx-tm8` and independent
   garment/layer thermal acceptance in `ftx-tmf`/`ftx-cv2`. Keep the highest-priority evidence gaps open.
   Qualify sewing recipes on measured material stacks under `ftx-sp2`; machine execution still needs
   calibrated workholding, kinematics, sensing, collisions, interlocks and its own authorization.

---

### 25.12 Measured leather and material-following exterior cooling

**Delivered measured transport (`ftx-ml1`).** Commit `956fd391` adds ten bovine leather study
sample groups and 40 numeric observations from Kopitar et al. (2022), DOI
[`10.1177/15280837221077048`](https://doi.org/10.1177/15280837221077048), Tables 4–5.
Together with the supplier tranche in §25.11, the current built-in catalog has **106 records**.
Each leather selection preserves tanning, finish and neck/back sampling, measured thickness and
dry/vapor resistance, published effective conductivity, original units and separate reported SD/CV.
Existing steady transport and thickness-based manufacturing consumers use these records without
new physics branches. The [commit's CI run](https://github.com/Dicklesworthstone/franken_textiles/actions/runs/39159137113)
passed formatting, strict Clippy, workspace tests, Python checks, wasm compilation and JavaScript
checks. See `docs/measured-leather.md` for the executable comparison and source limitations.

This is a measured single-protocol contribution, not complete cowhide characterization. Thermal
and vapor tests have different air temperature/humidity conditions. Thickness pressure, thickness
sample count and preconditioning are not numerically supplied by the article. SD/CV remain sample
variability rather than tolerance bounds. Areal mass, heat capacity, directional mechanics, friction,
temperature dependence, prices and qualified machine recipes remain absent. A missing number never
becomes zero or a default from a fiber name; missing data blocks only the consumers that require it.

**Delivered transient exterior exchange (`ftx-ex1`).** Core commit `c0735cc0` and adapter commit
`383db5da` extend the existing conservative thermal step and material-following surface/motion paths
with prescribed convection and source-qualified exterior radiation. The exposed face temperature
is solved consistently; reference mass is preserved while current area changes, and signed
convective/radiative energy is reported separately from numerical residuals. Each accepted motion
interval uses the same frozen finish/emissivity selection. Prescribed coefficients do not infer wind
or ventilation. The [complete adapter CI run](https://github.com/Dicklesworthstone/franken_textiles/actions/runs/39160779479)
passed formatting, strict Clippy, workspace tests, Python checks, wasm compilation and JavaScript
checks. Native core, CLI and motion analytic/replay checks establish this software path. The shared
WASM motion entry point compiles; this delivery does not claim browser-runtime numerical parity,
measured garment agreement or whole-wearer comfort.

**Remaining end-to-end priorities:**

1. **Matched material lots (`ftx-tm2`, `ftx-tme`, `ftx-md1`).** Characterize linen,
   treated cowhide, performance knits and fills together with yarn/construction/finish, conditioned
   mass and thickness, directional membrane/shear/bending, pair friction and compatible transport.
   Preserve fill compression/loft separately from shell mechanics. Use held-out coupons and then
   the unchanged jacket/body comparisons in `ftx-tm8`/`ftx-tmf`; these empirical gates stay open.
2. **Exterior coverage during motion (`ftx-ex2`, `ftx-tmb`).** Couple exterior exchange to explicit
   layer order, exposed faces and changing body/cloth overlap. Covered body area must be counted
   once; hidden lining must not radiate as an independently exposed shell. Preserve stored heat
   across conservative remapping and report uncovered or ambiguous regions. Test moving partial
   coverage and layer rearrangement before claiming full-garment heat loss.
3. **Source and process qualification (`ftx-sr2`, `ftx-tm6`, `ftx-tmc`, `ftx-sp2`).** Extend reliable
   live quotes and reviewed specification revisions while binding measurements to exact lots.
   Compare dated offers only across compatible currency, region, quality and sale/order terms.
   Qualify compression, moisture, air gaps and finish effects, then measured seam stacks, thread,
   needle, feed and machine envelopes. Material substitution must rerun these consumers and
   invalidate prior results; hardware execution remains a separate authorized operation.

The MIT ground-truth archive was inspected read-only under `ftx-md1`: its workbook contains 52
lettered specimen rows with thickness, area weight, bulk density and beam bending. It also exposes
raw/summary discrepancies for silk #4, Lycra #29 and taffeta #7 aggregation. These findings are
research leads, not newly ingested records. Retain raw identities and resolve the discrepancies;
missing coupon width, direction and conditioning still prevent a justified sheet-bending conversion.

---

### 25.13 Raw swatches, directional recovery and finite purchasing

The priority remains the unchanged jacket/body experiment in §25.1: material construction, lot,
finish and conditioning must determine the existing simulation and manufacturing inputs. The
complete attribute scope in §25.2–25.5 remains in force. A family name, blend percentage or supplier
description is insufficient to supply missing mechanical or thermal measurements.

**Nonlinear swatch identification (`ftx-tm4`, supports `ftx-tm2`).** Both native material
commands now expose `calibrate-nonlinear`, and the file-free browser API exposes the same Rust
engine. Full deformation gradients and observed nominal force components identify the existing
separable tabulated Green-strain membrane, including tension/compression asymmetry, toe regions,
stiffening, nonlinear shear and constant normal coupling. Missing force channels remain absent.
Only FIT observations determine the knot endpoints, parameters and strain domain; independently
held-out observations retain separate residual budgets and cannot extend or tune the fit.
Unexcited knots, unstable coupled tangents and extrapolation refuse. Estimates require explicit
admission, old equilibrium evidence requires explicit replacement, and a previous relaxation law
cannot silently attach to a newly fitted equilibrium. The output is the ordinary source-complete
frozen selection, preserving unrelated mass, bending, friction, thickness and thermal observations.
Native, portable and actual sewn-band tests exercise propagation without changing geometry or mass.
See `docs/nonlinear-material-calibration.md`.

**Directional recovery (`ftx-tm4`).** Commit `25a13d3` extends the current Maxwell history model
with independently sourced orthotropic branch tensors. Warp/wale, weft/course and bias may recover
at different rates. Explicit inactive directions, stable signed normal coupling and matched
specimen/conditioning are required; no material-name branch or separate solver is introduced.
The existing proportional history and serialization remain compatible. Actual per-piece sewn
triangles, clamped reactions, free dynamics and motion continuation use the same history path.
At this increment the timed fitter identified proportional spectra. Section 25.16 adds
independent directional identification at supplied times; measured cyclic acceptance remains open.

**Quantity-aware procurement (`ftx-tm6`).** Commit `00a773b` binds optional quotes to exact
resolved material hashes and checks the complete minimum/increment-rounded order against observed
supplier capacity in the quote's sale unit. An explicit policy can require observed capacity.
Unavailable, stale, mismatched, unknown-capacity and insufficient-capacity offers retain exclusion
reasons; a later unusable observation cannot revive an earlier quote. Marker cost remains unknown
when any material lacks an eligible whole-order offer. Source adapters must obtain stock quantities
from retained observations of the exact variant, never a static guessed manifest value. A dated
observation does not reserve inventory. Offer percentiles remain sample summaries, not worldwide
quality/location distributions.

**Source-backed mechanical catalog (`ftx-tm2`, `ftx-fj1`).** Three Fujii et al. (2026)
cotton sheeting, Oxford and twill specimens bring the built-in catalog to **109 records**.
Their 99 source-qualified numeric properties include published construction, measured thickness
and low-angle shear, and the original fitted tensile, Poisson and shear coefficients. Converted
shear and its source modulus represent the same observation; fitted coefficients remain estimates
and unconsumed extensions. Missing weight units and inconsistent bending units remain explicit
gaps, so these records cannot masquerade as complete garment materials. See
`docs/fujii-mechanical-measurements.md` for methods, conditioning and the primary DOI.

**Supplier refresh (`ftx-tm6`).** Commit `a95792d` carries reviewed material hashes through
CSV, Shopify and JSON-LD receipts. Numeric stock is obtained from the exact selected CSV row;
boolean availability from the other adapters does not imply a known order capacity. Existing
selling-basis page guards and representation-specific retrieval remain enforced.

**Next product and evidence gaps:**

1. **Matched practical materials (`ftx-tm2`, `ftx-tm8`).** Acquire complete compatible
   conditioned linen, treated cowhide and performance-knit swatches, with independently measured
   mass/thickness, directional membrane/shear/bending, counterface friction and transport. Fit the
   declared law only inside its characterized range, then validate held-out coupons and the same
   sewn jacket/body. Extend fill loft/compression and moisture separately. Numerical fixtures
   cannot close these highest-priority empirical gates.
2. **Independent recovery identification (`ftx-rc1`).** Identify orthotropic tensors and times
   from complete matched warp/weft/shear hold, unload and cyclic histories, including lateral
   reactions and independently characterized equilibrium. Report deficient excitation, relaxation
   outside measured durations and held-out disagreement rather than borrowing another axis's time.
3. **Permanent in-plane set and damage (`ftx-ps1`).** Add a measured, dissipative membrane state
   model for residual stretch and stiffness loss, preserving material-frame objectivity, grain,
   exact welded seams, failed-step rollback and motion continuation. Bending plasticity and
   recoverable Maxwell memory do not establish these mechanisms.
4. **Primary-source closure (`ftx-fj1`) and live stock qualification (`ftx-pq1`).** Resolve
   missing units and ambiguous bending normalization in the Fujii et al. (2026) cotton study before
   those observations become canonical mass or bending. Obtain permitted raw directional data.
   Qualify real supplier quantity observations and material/lot identity, unit/width/order tiers,
   zero/depleted stock and source changes. Preserve old frozen designs and unavailable results.

These increments establish common material propagation and numerical identification. They do not
establish measured garment fit/warmth, complete textile failure behavior or robot execution.

---

### 25.14 Material-conditioned compression and less wasteful cutting

**Grain-qualified stock use (`ftx-gr1`, supports `ftx-tm7`).** Commit `367677b` adds explicit
180-degree reversal permission to both roll polygon nesting and finite hide/remnant allocation.
Permission names the exact material SHA-256 being cut; a replacement textile, nap or finish cannot
inherit it silently. One-way grain remains the default. Outer cuts, holes, drills, directed grain
and affine transforms move together, without mirroring or crossgrain substitution. Defects,
clearances and occupied residual stock remain checked. The completed feasible marker survives a
bounded roll-search limit. A synthetic pair of triangular pieces now uses a 120 mm marker instead
of over 200 mm, and the existing increment-rounded procurement consumer reduces its order from
$2.50 to $1.50. A second test makes an otherwise unusable remnant usable. These are reproducible
geometry/accounting results, not global nesting optimality or physical cutter qualification.

**Prepared pressure states (`ftx-cp1`, supports `ftx-tmc`).**
`ftx-material-compression/1` prepares the existing frozen material selection at a declared normal
pressure from matched specimen, conditioning and loading-branch observations. Exact observed knots
retain source quantities and units. Interpolation needs explicit estimate consent and stays inside
the characterized interval. Retaining another scalar requires explicit source-bearing invariance
evidence over the interval; its original fixed-pressure observation must also be covered. Reference
areal mass keeps its original quantity. Unqualified scalars and old nonlinear, relaxation and
temperature-dependent response blocks/selectors are removed and reported, preventing a new loft
from retaining an unrelated resistance, density, stiffness or history law.

Both native commands and the browser workbench expose this operation. The output carries a normal
`Selection` and exact Rust-serialized `selection_json`, so assignment reaches existing drape contact
thickness, sheet mass, mechanics, steady/transient heat, cutting and study consumers without a new
material solver. Sewing refuses a selected compressed thickness where unloaded stack clearance is
required. The synthetic fixture gives 15 mm thickness and 0.375 m² K/W at 250 Pa, with invariant
100 J/(m² K) heat capacity; actual sewn-band tests also exercise pressure-dependent contact and
thermal transport with unchanged reference mass. This is an operator-prescribed state, not solved
normal contact pressure, reversible fill mechanics, a pressure trajectory or validated seam bulk.
See `docs/pressure-conditioned-materials.md` and `tests/fixtures/materials/synthetic-compression.json`.

**Measured loaded insulation (`ftx-li1`, supports `ftx-tm2` and `ftx-tmc`).** Six Rogale et al.
(2023) records represent polyester fleece, polyester spacer and a quilted polyester/polypropylene
assembly at 0.95 N and 9.5 N, bringing the catalog to **115 records** with the preceding cotton
tranche. Published group mass and directly measured thickness/displacement/resistance retain their
own conditions. Loaded thickness derived by subtraction is an explicit estimate; the directly
measured resistance remains usable without admitting that estimate. The existing steady heat
consumers change automatically when the selected force state changes. Numerical loaded area is
absent, so force cannot be converted into pressure knots for the new preparation API. Missing heat
capacity and mechanical coefficients remain unavailable. See `docs/loaded-insulation-measurements.md`.

**Next critical coupling (`ftx-cp2`).** Obtain matched pressure-area/force, thickness, thermal
transport and loading/unloading/recovery observations for real fills and layered fabrics. Then
couple local contact pressure and compression state to geometry and heat while conserving
reference mass, tracking mechanical work/dissipation and rolling back failed motion steps. Preserve
separate branches and relaxation durations; measurements after unloading cannot become sustained
pressure observations. Validate held-out compressed coupons before fixed-pattern garment comparisons.
The broader measured-material, moisture, garment-substitution and machine gates remain open.

---


### 25.15 Exterior heat across layered coverage and motion

The delivered exterior solver now reaches the existing body/cloth coverage topology. Covered
regions form series-connected material columns; only the region's outermost selected textile
exchanges convection and radiation with its prescribed environment. Body contact films and
interfaces retain covered-body-area units, exterior exchange uses allocated current outer-cloth
area, and sensible heat follows allocated reference mass. Moving or stretching the geometry does
not create fabric mass or count hidden lining as a second exterior shell. The core increments are
`bc1380b9` (area-aware coverage), `2258a267` (moving coverage and exposed-cloth continuation)
and `1c7359d8` (static, saved-map and shared scenario consumers).

`Coverage::{steady_exterior,advance_exterior}` and
`Frame::advance_with_exchange` share the existing constant/temperature-dependent material
transport and storage solvers. A covered environment is indexed by body triangle and qualifies
each active region's outermost material observation. Bare-body fractions require separately
declared films with the same prescribed body temperature; textile emissivity never becomes
skin emissivity. Uncovered cloth retains its canonical through-thickness state and is adiabatic
unless explicit face conditions are supplied. Qualified exterior exchange may be selected
independently for covered and uncovered cloth.

The existing material-coordinate polygon/depth remapper and frozen checkpoints preserve
covered-to-exposed-to-covered continuation. Changing the next interval's air/radiant temperature
or prescribed convection does not reset material temperatures. Source/material or topology edits
still invalidate incompatible state; edits to previously reported conditions change the result
receipt. Physical boundary energy, numerical remap residuals and exterior face-equation residuals
remain separate. Signed convection and radiation are reported by active region/source triangle
and accumulated over the requested intervals.

The native static coverage and saved-map motion consumers, plus the shared material-scenario
motion evaluator used by native and WASM entry points, accept optional exterior schedules and
explicit bare/exposed conditions. Omitted fields retain the film-only behavior. The actual
material observations, boundary policies, geometry and final face fields remain in the frozen
inputs/results. See `docs/thermal-motion.md` for the interval contract and
`docs/thermal-coverage.md` for static coverage. Independent tests exercise unequal body/cloth
areas and reference masses, two-cell conduction networks, manufactured signed radiation,
partial-body coverage, material changes, moving exposure and checkpoint continuation.

**Measured optical observations (`ftx-op1`).** The current 148-record built-in catalog has no
optical observations qualified for this radiation model. Acquire matched optical and thermal
measurements for an ordinary woven, a performance knit and finished cowhide, retaining specimen,
actual face, finish, ply count, conditioning, temperature validity, spectral/angular basis and
uncertainty. Keep partial observations useful in the database without promoting them into a
solver coefficient. Belliveau et al. (DOI [10.1177/0040517519888825](https://doi.org/10.1177/0040517519888825))
is a useful ordinary-apparel source lead, but its camera-band, two-ply measurements cannot be
relabelled as total hemispherical single-sheet emissivity. Missing spectral/angular evidence
and matched transport/storage remain explicit acquisition work.

**Remaining physical and automation gates stay open.** This completes a software propagation
slice of `ftx-ex2`; its independent measured garment experiments and uncertainty acceptance
remain required. Named optical observations can now be bound to source-sheet orientation and
selected automatically as described in §25.18; without that declaration, explicit-face mode
retains the caller's physical assignment. `ftx-ex3` remains open for independent asymmetric-coupon
validation. Measured ventilation, non-unit view factors, pressure-dependent
loft, moisture/evaporation, full wearer thermoregulation and the fixed-jacket empirical gates in
`ftx-tm2`/`ftx-tm8`/`ftx-tmf` are not established by these numerical results.

---

### 25.16 Measured-history identification reaches the material workbench

**Calibration as a usable design workflow (`ftx-tm2`, `ftx-tm4`, `ftx-rc1`).** Commit
`f4f3a28` adds local nonlinear-swatch and timed-recovery identification to the browser material
workbench, with the same shared Rust engines used by both native commands. Imported observations
produce separate fit/held-out residuals, applicable strain domains, source/evidence diagnostics and
ordinary material capability reports. The generated frozen selection can be assigned to the pattern
default or individual pieces and consumed by the existing drape and manufacturing paths. Exact
selection, original acquisition and report text are retained for downloads and binding; evidence
consent, failed replacement fits and newer edits cannot silently reuse a prior pending selection.

**Independent directional recovery (`ftx-rc1`).** The timed fitter now accepts one to eight
explicit candidate times and identifies independent warp/wale, weft/course, signed normal coupling
and shear stiffnesses at each time. It uses complete deformation histories, nominal forces in N/m,
independently characterized equilibrium and the existing exact acquisition-time integration.
A convex positive-semidefinite fit supports inactive axes and an explicit caller-selected zero
threshold. Every candidate retains its unconstrained, constrained and published coefficients,
including inactive candidates. Deficient excitation, nonconvergence and failed residual budgets
refuse; held-out histories never select coefficients or candidates. The original proportional
ratio/time optimizer and serialized reports remain compatible when this mode is omitted.

The result is the existing orthotropic Maxwell material law, not a new cloth solver. Final residuals
are replayed through the compiled physical response. Independent synthetic checks cover mixed-mode
forces, unequal acquisition scales, signed coupling, incomplete sensors, nonlinear equilibrium,
loading/unloading, actual clamped reactions, changed free motion and per-piece sewn history.
The browser displays the retained directional tensors and runs the same fixture through WASM and
native identification, exact binding and capability checks. See `docs/relaxation-calibration.md`
and `docs/browser-material-workbench.md`.

**Next consequential identification work (`ftx-rc2`).** The fixed-candidate mode above takes
supplied times; bounded automatic directional time identification is now implemented in §25.17.
Explicit model-order comparison and competing-spectrum ambiguity remain open. Preserve inactive
directions, physical tensor constraints, reproducible initialization and honest sensitivity/domain
limits without using held-out histories to select the model. Numerical convergence is not
coefficient uncertainty.

**Empirical and material-state priorities remain open.** Matched real linen, treated cowhide and
performance-knit acquisitions and independent sewn-garment checks still gate `ftx-tm2`,
`ftx-tm8` and `ftx-rc1`. Residual in-plane stretch and damage (`ftx-ps1`), contact-coupled
compression/heat (`ftx-cp2`), moisture and qualified machine recipes remain separate work.
A successful fit does not supply missing mass, bending, friction, transport or process evidence.

### 25.17 Bounded automatic directional recovery-time identification

**Recover times as well as tensors (`ftx-rc2`).** An optional directional request now supplies
one to three separate positive `orthotropic.time_bounds_seconds` windows instead of exact
candidate times. A deterministic bounded multistart search identifies one recovery time in each
window while the shared convex fit identifies its independent warp/wale, weft/course, signed
coupling and shear tensor. Finite-ramp acquisition integration, independent equilibrium, source
conditions and material compilation remain the existing mechanisms. The resulting selection
drives the existing cloth solver and per-piece sewn histories without simulation-code changes.

Only FIT observations determine the times and tensors. Exact log-time derivatives drive the
profile objective; each trial solves the positive-semidefinite tensor subproblem. Stationarity,
joint amplitude/time excitation and distance from search boundaries must pass independently of
the caller's residual budgets. Every searched time requires a retained observable branch; inactive
directions remain supported. Fixed-time mode continues to preserve inactive candidates and
equilibrium-only outcomes, with unchanged input hashes and method text. Automatic search refuses
after 1,024 total objective evaluations or 262,144 projected tensor iterations rather than
publishing a partial best result.

The report and browser expose attempted/converged starts, selected iterations, total objective and
tensor work, scaled objective gradient, time sensitivities, joint excitation and normalized boundary
distances. Search windows are limits, not confidence intervals. A converged local minimum is not a
global-optimum proof or a unique material spectrum. Final thresholded tensors still replay through
the ordinary compiled law against separate FIT and held-out CHECK residual budgets.

Independent synthetic acquisitions cover one, two and three active times, inactive directions,
signed coupling, finite ramps and held-out unloading. A constrained noisy acquisition with 100-fold
unequal force scales checks the independently known one-second optimum. An objective-gradient
difference check exercises reoptimized PSD tensors. Actual triangle reactions, changed free motion,
sewn per-piece assignment and the native/WASM/browser binding path use the identified selections.
The new `synthetic-orthotropic-relaxation-search.json` fixture preserves all independently authored
observations from the fixed-time fixture and changes only the declared time-search options.
See `docs/relaxation-calibration.md` for request examples and numerical/physical limits.

**Remaining consequential work.** Branch count and admissible time windows are still caller choices.
Explicit model-order comparison, competing-spectrum ambiguity and meaningful parameter uncertainty
remain part of `ftx-rc2`. Matched physical coupons and garment validation remain `ftx-rc1`,
`ftx-tm2` and `ftx-tm8`; synthetic numerical success does not close those empirical gates.
Missing mass, bending, friction, heat/moisture or machine-recipe evidence stays missing.

---

### 25.18 Oriented material optics and measured neoprene assemblies

**Delivered material-to-exterior integration (`ftx-ex3`).** Optional source-bearing
`Material.optical_faces` binds positive/negative source-sheet sides to exact optical
observations. Automatic radiation selects the current outer covered side using the same
cloth/body-normal authority as through-thickness temperature remapping. Independent columns
and explicitly exposed cloth use the canonical positive outside face; the other face keeps
its declared inside film. Mirrored piece names introduce no extra flip. No nearest-body,
second ambient face, skin emissivity or arbitrary finish label is inferred.

The mapping preserves source, evidence, specimen and conditioning and participates in ordinary
material hashes. Missing assignments, uncharacterized sides, unadmitted estimates and unsupported
optical spectral/temperature qualifications refuse. Existing explicit-face requests remain valid.
Results retain the selected side, observation and material identity. Stored coverage temperatures
bind layer orientation; direct reversal requires motion remapping, which preserves reference
enthalpy through exposure changes and checkpoints. Pressure-conditioned preparation discards and
reports stale optical assignments. See [material-optical-faces.md](docs/material-optical-faces.md).

Validation includes 111 passing focused Rust tests: 62 thermal core, 16 material
optics/conditioning, 11 catalog and 22 actual CLI tests, plus pinned formatting,
strict workspace Clippy and the `wasm32-unknown-unknown` build.
Native acceptance covers sewn mirrored pieces, opposite body sides, automatic face reports,
missing-map refusal, measured material substitution and offline replay. Compiled WASM and
the actual worker match native results, including automatic asymmetric optical selection
and missing-map refusal. A complete Chromium pass is not established by this session.
Independent asymmetric-coupon and garment
validation remain open under `ftx-ex3`, `ftx-op1` and `ftx-tm8`.

**Measured catalog expansion (`ftx-tm2`).** The catalog now contains 148 records, including
33 unlined, one-sided-lined and both-sided-lined neoprene specimen groups from
[Potočić Matković et al. (2025), DOI 10.3390/polym17212820](https://doi.org/10.3390/polym17212820).
All 231 imported quantities were independently matched to the source XML tables: reference
thickness, areal mass, dry thermal resistance and four pressure-specific compression displacements
per group. Original units, separate instrument protocols, specimen-group identities and reported
limits are retained. Measured dry resistance feeds the existing steady and layered conduction
consumers through ordinary material selection; estimates are not needed for those observations.

The displacement zero/preload and compatibility with the separate 1 kPa thickness reference
remain unspecified. These observations do not become loaded thickness, pressure-invariant mass,
measured loaded thermal resistance or a recovery law. `ftx-np1` tracks the focused source-reference
and loaded-thermal acquisition supporting `ftx-tm2`/`ftx-cp2`, without blocking already qualified
dry conduction. See [measured-neoprene.md](docs/measured-neoprene.md). Specific heat, directional
mechanics, bending/contact, moisture and qualified optical observations remain separate missing
inputs; the new records do not close the linen/leather/knit or independent garment gates.

---

### 25.19 Atomic material and interface substitution

**Delivered contact-data propagation (`ftx-if1`, supports `ftx-tm3`/`ftx-tm8`).** The existing
material binding request accepts an optional complete interface replacement alongside default and
named-piece material selections. A shell change can therefore supply its new shell-to-lining
observations in one operation. Final assigned snapshot hashes, source metadata, units and unique
unordered pairs are validated together; no old coefficient or hash is relabelled. Omitting the field
preserves the original table, while `[]` explicitly clears it. Observation validity remains separate
from the mechanical consumer's estimate admission and pair-coverage requirements.

Both native commands expose `bind-changes PATTERN.json CHANGES.json NEW-PATTERN.json`, writing a new
exact pattern and reporting real drape-material readiness with and without self-contact. The browser
assignment section accepts the same pair list and retains source, request, exact bound pattern and
resolver report for download. Changed or invalid pending evidence withdraws dependent outputs.
Manufacturing and scenario alternatives use the same operation and retain explicit replacements in
their frozen replay changes. Existing requests that omit the new field preserve their semantics.

Independent synthetic sewn tests keep geometry, mesh, rest areas, welds and lining identity fixed
while the changed shell updates reference mass and the actual symmetric friction matrix. Stale or
unadmitted pair evidence prevents simulation before its callback. Native commands, shared Rust WASM
entry points and the compiled WASM worker exercise exact replay and independent consumer refusals.
This closes a software substitution blocker; matched real material pairs and linen/leather/knit
garment acceptance remain open. See [piece-materials.md](docs/piece-materials.md).

---

### 25.20 Measured moisture states through the shared thermal material authority

**Delivered wet-denim observations (`ftx-wd1`, supports `ftx-tm2`/`ftx-tmc`).** Six separately
identified cotton-warp denim states cover cotton, spun-polypropylene and PET wefts at the first and
last reported moisture observations in Mangat's 2012 Technical University of Liberec dissertation,
Table 4-9. The combined built-in catalog now contains **154 records**. Each record retains the
reported 1,000 Pa Alambeta setting, wet-basis water mass fraction, measured thickness and thermal
resistance, plus separately rounded conductivity and absorptivity observations. The primary table
values and units were independently reviewed; full dissertation text and images are not bundled.

Selecting a state changes existing `ConductionLaw` and `ThermalStack` results without estimates or
solver edits. Reported resistance remains the single transport authority. Cotton-weft observations,
for example, retain measured resistance 0.0128 and 0.0032 m² K/W at water fractions 0.012 and 0.447.
Moisture is water mass divided by total wet fabric mass, not dry-mass regain or relative humidity.
Measured loaded thickness retains the existing pressure-state restriction on sewing clearance.

These are discrete measurement states. Missing row-specific washing/lot IDs, uncertainty, plate
temperatures, mass and heat capacity stay missing. No interpolation, evaporation, free-air drying,
pressure recovery, transient storage or wet mechanical law follows from the scalar table. The
next coupling still requires matched state-dependent transport, storage, geometry and loading
observations with conservative continuation. See [wet-denim-measurements.md](docs/wet-denim-measurements.md).

---

### 25.21 Source-qualified permanent in-plane stretch

**Delivered software slice (`ftx-ps1`, supports `ftx-tm4`).** A material can now opt in to
`orthotropic_green_return_map/1` through `structure.membrane_plasticity_model`. Twelve explicit
warp/weft/shear yield, hardening, total-strain and accumulation quantities must match the specimen
and conditioning of all four scalar elastic observations. The same normalized material snapshot
supplies the elastic tensor and binds history identity. Missing or incompatible evidence refuses;
the catalog gains no guessed textile yields, damage thresholds or fiber-name rules.

The bounded associated return map uses raw reference-frame Green strain `[E11,E22,2E12]` and
work-conjugate second-Piola sheet resultants in N/m. Consistent condensed derivatives replace each
selected triangle's elastic contribution once. Physical elastic-plus-hardening storage and
nonnegative plastic dissipation are reported separately in joules using retained reference area.
Current and permanent metrics must remain positive definite, and total/accumulated strain must
stay inside their authored domains. This additive phenomenological law captures permanent set,
elastic unloading and directional isotropic hardening; it does not implement damage or rupture.

Existing default/per-piece material binding, unchanged-pattern sewing and prescribed-body motion
consume the law after grain mapping and seam welding. Selected triangles require the exact tensor,
unit compression factors and no simultaneous Maxwell/nonlinear membrane law. Other triangles keep
their own models. History starts pristine relative to the authored reference metric; kinematic
placement is not an inferred forming process. Physical step acceptance commits geometry, velocity,
contact and material memories together. Trials and failed steps cannot advance permanent strain;
cloning and motion continuation preserve it. Static/speed-only settling refuses an unspecified
irreversible loading path.

**Qualified irreversible-work heat.** Optional `motion.transient_surface.plastic_dissipation_heat`
now transfers accepted per-triangle membrane dissipation after motion time zero into the existing
reference-mass thermal columns. Each active plastic material must qualify its own conversion
fraction, specimen/conditioning, irreversible-work basis and temperature interval. Uniform
reference-thickness/interval deposition preserves separate boundary, supplied and stored energy;
initial drape work and failed trials are excluded. Source/law identity, bounded capture and enthalpy
inversion, thermal-domain refusals and rollback remain enforced. This is independent-sheet,
one-way coupling; it supplies no measured conversion data, friction/coverage heating or temperature
feedback. See the material and scenario documentation for requests, budgets and scope.

The ordinary synthetic selection and executable commands are documented in
[material-membrane-plasticity.md](docs/material-membrane-plasticity.md). Drape and motion reports
retain `membrane_plasticity_history`, law hashes, accepted strain, storage and dissipation.
Checked library reactions expose the frozen accepted state and refuse stale or invalid history.
Known-answer material/solver checks cover return-map work, derivatives, unloading and accepted-state
rollback. Native sewn/motion workflows exercise piece-local history, heat conservation and late
thermal refusal. A model-backed synthetic replay compares complete native and compiled-WASM worker
results, including unchanged mechanics and changed temperatures under conversion-only substitution.
These numerical and adapter checks do not establish empirical textile or garment agreement.

**Remaining consequential work.** `ftx-ps1` and `ftx-tm4` remain open for measured matched cyclic
warp/weft/shear data, independent loading/unloading/reloading acceptance, stiffness degradation,
damage and rupture. A same-triangle Maxwell/nonlinear plastic law, gather softening, conditioning,
remeshing/restart transfer and two-way thermomechanical response require their own qualified models.
No numerical success supplies missing linen/leather/knit parameters or closes `ftx-tm2`/`ftx-tm8`.

---

### 25.22 Measured apparel specimens extend the shared material authority

**Eight matched thermal fabric groups (`ftx-tm2`, `ftx-tma`).** The bundled catalog now
includes De Mey et al. (2025), DOI `10.3390/textiles5040066`, Table 1: angora, polyester/Outlast,
cotton/elastane and cotton single jerseys, two linen/cotton complex wovens, a wool/linen/cotton
plain weave and a cotton/linen chenille. All eight retain the reported composition/construction,
measured mass, gauge thickness and effective guarded-hotplate resistance, with immutable source
identity and original units. Ordinary material discovery, freezing, screening and steady thermal
comparison consume these same records. The total catalog grows from 154 to 162 records, preserving
the recently added neoprene constructions and discrete wet-denim observations.

The source's 0.5 kPa gauge thickness and 35/25 °C plate-to-plate thermal protocol are distinct.
No conductivity is derived from unmatched pressure/contact conditions. Raw replicate values,
uncertainty bounds, heat capacity, contact corrections, temperature validity intervals and Outlast
phase-change properties are not supplied. Existing scalar resistance semantics support a declared
constant-property steady calculation; this is not an experimentally validated field-temperature
extrapolation or arbitrary multilayer contact law. Transient heat and complete mechanics remain
unavailable for these records. Independent table transcription and actual canonical flux/screening
tests check software propagation and exact-source replay. See `docs/measured-apparel-thermal.md`.

**Remaining physical characterization.** Matched directional mechanics, conditioned thermal
storage, contact-separated transport and held-out sewn-garment measurements remain open. These
source-specific observations must not become universal values for a fiber, brand or construction.

---

### 25.23 Supplier revision review and explicit recovery-model comparison

**Supplier specification revisions (`ftx-sr2`).** Existing source adapters can retain a
specification snapshot and compare a later exact product/variant observation with it. Replay of
source bytes and receipts establishes which declarations changed; composition, construction,
finish, dimensions, variant and selling basis remain original source facts with dates and hashes.
Price/stock-only updates preserve the existing material binding. Specification or material-binding
changes produce a review report and withhold new quotes while preserving previous snapshots.
The source review does not infer a measured specimen/lot link or change a frozen material.
See [textile-price-refresh.md](docs/textile-price-refresh.md). Broader primary supplier acquisition
and matched conditioned lot evidence remain open.

**Competing directional recovery models (`ftx-rc2`).** The existing calibration commands and
browser entry accept two to four named fixed-time or bounded-time orthotropic alternatives on one
acquisition. All candidates share the 1,024-objective and 262,144-projected-iteration limits,
including unsuccessful work. Reports retain each spectrum, residuals, prediction differences,
declared parameter count, failure and actual work. FIT equivalence, parsimony and candidate choice
are fixed before CHECK replay. Ambiguous or incomplete automatic comparisons cannot publish a
selection; an explicit reviewed choice still must pass its supplied acceptance gates. A failed
CHECK rejects the same chosen candidate and never promotes another model. Unresolved browser
results clear the previous assignable material while keeping the report and request downloadable.
See [relaxation-calibration.md](docs/relaxation-calibration.md).

These comparisons apply only to the declared alternatives, acquisition and tolerance. They do not
discover unlimited model orders, establish global spectrum uniqueness, infer a noise model or
produce parameter confidence intervals. Fitted quantities remain estimates; matched measured
coupon and garment acceptance stay open under `ftx-rc1`, `ftx-tm2` and `ftx-tm8`.

---

### 25.24 Assembly-mass limits from complete reported source bounds

**Conditional mass comparison (`ftx-ms2`).** Existing manufacturing and scenario studies can
request `basis: "reported_bounds"` for `assembly_mass_kg` objectives and constraints. Complete
source endpoints propagate through the unchanged authored areas and explicit additive inventory.
An inclusive mass limit must contain the entire reported range; a nominally light garment whose
upper endpoint exceeds the limit is excluded. Missing bounds retain visible nominal mass with an
explicit exclusion, and overlapping or touching ranges remain unresolved on the Pareto frontier.
The browser displays these ranges alongside each nominal value through the existing study flow.
Ordinary nominal comparisons preserve their previous policy and output semantics.

This policy currently supports mass only. Other consumers can still be required through their
own completion gates, but unsupported drape, thermal, cost or process-result intervals refuse.
Reported mass endpoints are conditional on the authored geometry, complete selected inventory
and admitted source protocols. They are not confidence intervals, uncertain-geometry propagation,
finishing allowances or an empirical garment guarantee. See
[material-screening-and-studies.md](docs/material-screening-and-studies.md) and
[material-scenario-studies.md](docs/material-scenario-studies.md).

---

### 25.25 Intraday supplier history in the existing purchasing consumers

**Explicit observation instants (`ftx-tm6`).** The existing quote book accepts a version-2
extension with source `observed_at` instants and an explicit evaluation `as_of_at`. UTC instants
use integer nanosecond ordering, retain matching calendar dates and never use an implicit clock.
Version-1 date-only histories keep their original behavior. The latest observation at or before
the requested instant controls admission; expiry, unavailability, material mismatch or insufficient
stock cannot revive an older offer. Inclusive source expiry and existing calendar-age policies
both apply, with exact source observations retained in exclusions.

The reviewed Shopify, CSV and JSON-LD adapters can opt into version 2 with `--intraday`, preserving
the actual retrieval instant and source bytes. JSON-LD timed validity is checked and normalized
without rounding away supported source precision. Supplier-specification review replays these
receipts without turning expiry changes into material revisions. The existing native price,
marker procurement, manufacturing/scenario studies and WASM consumers share this same evaluator;
the browser displays observation times, whole-order costs and exclusions from its results.
See [textile-price-refresh.md](docs/textile-price-refresh.md). These are dated supplier observations,
not reserved stock, executable purchases, worldwide market coverage or inferred landed costs.
Primary supplier qualification and broader source coverage remain open.

---

### 25.26 Sewing order with fewer declared setup changes

**Bounded assembly-order planning (`ftx-tm7`).** Existing `SewOptions` can request
`ordering.objective: "minimize_setup_changes"` with an explicit search-node limit. Every complete
operation first passes the ordinary geometry, material, layer-stack, machine and optional source
qualification checks. The planner then minimizes the sum of needle and thread identity changes
while preserving declared operation dependencies. An optional exact initial setup contributes its
first transition; omission does not assume an installed needle or thread. No final reset is added.

The result retains the baseline and selected schedules, separate component counts, actual search
work and a proved lower bound. Bounded search preserves a complete feasible incumbent. Optimality
is stated only after all improvements are excluded or the incumbent attains the lower bound.
The selected operations keep their exact stations, directions, backtacks, folds, sources and material
identities through normal stitch expansion, native/WASM manufacturing and material studies. Browser
results expose the same schedule and proof status, with stale outputs withdrawn after recipe edits.
Omitting the policy preserves legacy planning behavior and serialization.

This objective counts declared setup changes. It does not infer handling/rethreading durations,
physical assembly accessibility, seam quality or machine execution permission. Up to 20 operations
and 200,000 search nodes are supported in this opt-in mode; larger ordinary plans remain available
without it. Measured recipes, broader quality/time objectives and hardware qualification remain open.
See [material-sewing-plans.md](docs/material-sewing-plans.md).

---

### 25.27 Permanent stretch identified from complete cyclic acquisitions

**Source-qualified yield and hardening identification (`ftx-ps1`, `ftx-tm4`).** The existing
permanent membrane law now accepts bounded identification of its six directional yield/hardening
coefficients from signed raw-Green strain and work-conjugate second-Piola sheet-load histories.
Four independently supplied elastic observations and six characterized strain/accumulation limits
remain fixed. Every recorded strain node advances the same return-map memory used by the simulator,
including unloading, reversal and nodes with missing load channels. No rate law or unobserved
loading path is inferred.

Whole histories are partitioned into FIT and CHECK. Four bounded deterministic starts, local
excitation and residual-equivalent candidate checks use FIT only; CHECK replays the fixed choice
afterwards. All forward probes, refused trials and final replays share explicit work limits.
Unexcited, ambiguous, incomplete or rejected results retain diagnostics and publish no material.
Accepted fitted quantities remain source-bearing estimates, with search bounds separate from
independently characterized domains and no invented confidence intervals.

Both native commands, the shared WASM API and the browser expose the same calibration-to-frozen-
selection path. Acquisition admission and downstream use of estimates remain separate decisions.
The browser retains exact input, residuals, permanent state and work diagnostics, and clears a
previous assignable result when a new identification is unresolved. Ordinary material binding
then carries an admitted selection into existing sewn and moving-body consumers. Independent
synthetic cyclic answers check software behavior; measured coupon, damage/rupture and garment
acceptance remain open. See [material-membrane-plasticity.md](docs/material-membrane-plasticity.md).

---

### 25.28 Enforced acquisition domains and the next material-coupling gaps

**Scalar calibration reaches physical-domain admission (`ftx-sd1`, supports `ftx-tm4`).**
The four identified orthotropic scalar coefficients must retain one source and the same FIT-only
raw-Green strain box. Normal material resolution carries that box through each piece's grain frame
into the retained sewn triangles. Scene placement, untangling, accepted states and Newton trials
cannot extrapolate outside it, including when numerical compression softening is enabled. Material
replacement clears old domains; Maxwell and permanent-set histories refuse changed domain identity.
Held-out samples never widen the acquisition range. The independent synthetic tests demonstrate
software propagation and refusal, not experimental accuracy throughout the box or measured garment fit.

The same retained equilibrium box now constrains cyclic permanent-set and proportional/directional
Maxwell identification at every recorded strain node, including nodes without a measured force.
An invalid FIT history refuses before identification. A held-out CHECK-domain failure retains the
same FIT candidate and diagnostics, publishes no material, and cannot change model selection or
consume its FIT-only work allowance. Fixed and searched directional spectra use this same rule.

The following integration increments use the same authorities: qualify actual finite supplier
capacity against complete rounded cut orders (§25.29, `ftx-pq1`/`ftx-tm6`), and carry admitted plastic-work heat
through changing covered/exposed cloth partitions (`ftx-ps1`/`ftx-tmb`) without counting source energy
twice or turning remap residual into heat. Neither requires a brand-specific physics branch.
Matched conditioned linen, treated cowhide, performance-knit and fill characterization, measured
conversion laws, and independent sewn-garment/process acceptance remain the larger open gates.

---

### 25.29 Public supplier observations bound to actual textile revisions

**Reviewed offers and finite capacity (`ftx-sr3`, supports `ftx-pq1`/`ftx-tm6`).** The existing
refresh and specification-review path now accepts an explicitly reviewed public Shopify product
adapter. Public JSON observations bracket the Ajax response, checking exact product/variant/SKU,
currency, price, description and ordering rules. Numeric inventory is admitted only when all three
observations agree on Shopify-tracked stock with overselling disabled. Unreported capacity stays
unknown; a newly reported quantity requires review. Separately reviewed source expiry is never
extended by refreshing a product. All three source digests and actual retrieval times are retained.

The October 20 tranche contains six exact offers: two Ripstop by the Roll nylon constructions,
Climashield APEX 2.5 and three Discovery Fabrics Power Stretch variants. Four new material cards
and 14 numerical observations bring the built-in catalog to 166 records; existing APEX and Olive
Mesh evidence is preserved. Canonical material hashes bind every offer through the normal pricing
and whole-order capacity evaluator. USD and CAD stay separate. Nominal width does not establish
usable cutting width, and total inventory does not establish one continuous cut (APEX's reviewed
standard maximum is five yards). Missing mechanics and heat properties remain unavailable.

The dated snapshot, refresh manifest and replay instructions are in
[textile-price-refresh.md](docs/textile-price-refresh.md). Native integration checks exercise all
six identities, rounded-order capacity, unknown stock, expiry and changed-material refusals;
offline adapter tests additionally cover source drift and malformed observations. Broader supplier
coverage, observed stock-depletion/replenishment histories, matched material characterization and
empirical garment/process acceptance remain open. Quotes neither reserve inventory nor establish
delivery, landed cost or worldwide price distributions.

---

### 25.30 Material-qualified deformation heat through moving coverage

**Covered layers and exposed cloth (`ftx-ch1`, supports `ftx-ps1`/`ftx-tmb`).** An optional
`motion.transient_coverage.plastic_dissipation_heat: "uniform_reference_thickness"` consumes the
same source-qualified material conversion fractions and accepted permanent-work increments as
the independent-sheet history. Each retained triangle's nonnegative joules are partitioned over
its covered and exposed reference area, then through its own layer depth. This preserves distinct
layer capacities, heats adiabatic exposed cloth, and supplies no heat to bare body. The existing
bounded enthalpy kernel handles temperature-dependent capacity and refuses unsupported domains.

Initial drape work and rejected solver trials supply no heat. Mechanical work is captured once
when either or both histories request it; the two histories remain separate model domains, with
separate energy totals. Conversion evidence, exact material/law identities and temperature limits
are retained. Explicit zero conversion is supported; missing evidence refuses only the affected
thermal history. No material family, brand, empirical conversion value or thermal feedback is
inferred.

Reference enthalpy survives cover/expose/re-cover remapping. Physical storage change equals net
boundary energy plus supplied source heat, with allocation and inversion residuals reported;
remap residual is separate. A fully covered source no longer requests nonexistent exposed history
when actual overlap falls short only within the existing allocation tolerance. Covered donor
weights and reported area/enthalpy residuals remain unchanged; larger gaps and empty stencils refuse.
Native/WASM scenario reports and browser comparison expose the same
source receipts, per-step energy, fields and refusals. The comparator admits coverage heat only
when its accepted-work history and exact frozen source evidence reconcile. Heating work budgets
include every layer cell and body face; unsupported work leaves no partial thermal result.

Independent synthetic enthalpy tests cover variable heat capacity, layer/reference-area allocation,
moving exposure, exterior exchange and invalid-state rollback. The actual accepted-body native
workflow checks unchanged mechanical motion with conversion-only temperature changes, coverage-only
capture and preserved mechanics on missing conversion evidence. These validate software coupling;
matched measured conversion laws, held-out thermal histories, moisture physiology and physical
garment acceptance remain open. See [material-scenario-studies.md](docs/material-scenario-studies.md).

---

### 25.31 Complete material combinations through garment consumers

**Independent shell, lining and trim choices (`ftx-mc1`, supports `ftx-ms2`).** The existing
`ftx-material-study/1` request accepts `combinations` as an alternative to explicit variants or
scalar screening. One to eight named groups each select a material for the pattern default or a
disjoint set of source pieces. The complete Cartesian product is limited to 32 alternatives and
refuses before source resolution when over budget; no surrogate score or hidden pruning removes
a candidate. Default replacement preserves explicit piece assignments, and named assignments
retain their ordinary precedence.

Every alternative goes through the same source binding, mass, cutting, finite-stock, sewing,
purchasing and optional fresh drape/thermal scenario consumers as a manually authored variant.
Each result retains its group choices, complete source-locked assignments and independent consumer
refusals. The existing comparison policy ranks only supported actual outputs. A supplied pool of
qualified pair-friction observations is validated completely, then selects only pairs used by
that alternative's exact material hashes. Missing unlike-material observations remain unavailable;
no averaging or material-family friction rule is introduced.

The browser workbench can capture inspected selections into these named groups, display the
combination count, load an optional interface pool and run ordinary manufacturing or scenario
studies. It retains original JSON numbers and source locks, clears stale results after choice,
policy or pattern changes, and downloads the complete generated request for replay. Native CLI
and file-free WASM use the same request and production evaluators.

This delivers finite heterogeneous design search, not unbounded global optimization or empirical
garment accuracy. Large design spaces, alternative geometry/process variables and independently
measured matched material/garment acceptance remain under `ftx-ms2` and `ftx-tm2`/`ftx-tm8`.
See [material-screening-and-studies.md](docs/material-screening-and-studies.md) for a complete
catalog-backed two-by-two example and the interface/assignment contract.

---

### 25.32 Measured performance-knit strip observations

**Matched production specimens (`ftx-kn1`, supports `ftx-tm2`).** Three records from the
MIT-licensed Sperl et al. SIGGRAPH 2022 dataset add polyester/spandex single jerseys SJ1/SJ3 and
the polyester/Celliant-polyester/spandex interlock DKIN8. They retain reported yarn mass fractions,
yarn identities, counts and finishes, three areal masses, three specimen-gauge observations and
18 directional grip-force values. The built-in catalog now contains **169 records**.

For each specimen and warp/weft/bias cut, the force attributes retain exact author-reported loading
rows at 10% and 30% grip extension on a 100 mm free-gauge by 60 mm-wide clamped strip. The source's
upstream processing, conditioning, rate and uncertainty remain unspecified where not supplied.
No interpolation, smoothing or conversion into a homogeneous deformation gradient is performed.
The original nine CSV byte strings, full metadata, archive paths, source hashes and complete MIT
notice are retained for test-time reconstruction; normal native/WASM catalog construction uses
compact transcriptions without bundling the source fixture into the application binary.

The ordinary screener can now choose minimum measured areal mass subject to a qualified directional
grip-force constraint. For example, weft force at 30% of at least 2 N selects SJ3 among these three
when the caller explicitly permits comparison across specimen identities. Missing source bounds
remain unavailable. Unspecified gauge pressure prevents promoting specimen thickness to canonical
unloaded clearance. These observations do not supply thermal, homogeneous membrane, shear or bending
laws, and do not make a knit solver-ready by inference.

**Next critical acquisition consumer (`ftx-ki1`).** Implement an inverse characterization path that
models the actual clamped strip geometry, orientations, grip motion and transverse freedom before
fitting constitutive parameters. Retain loading/unloading history and upstream processing scope;
separate fitting from held-out tests and quantify identifiability before publishing a frozen law.
Matched independent bending/contact and thermal observations remain separate requirements. This is
the path from the retained force observations to defensible automatic garment substitution, rather
than treating grip extension as uniform textile strain. See
[measured-knit-strips.md](docs/measured-knit-strips.md) for sources, all values and a screening request.

---

### 25.33 Source-qualified accessory mass in cloth dynamics

**Retained trims, closures and ornaments (`ftx-am1`).** Optional `pattern.attached_mass` supplies
the total source-qualified mass of each extra group and its explicit fractions at named physical
instance/boundary vertices. The existing mass-evidence authority now lives in `ftx-pattern` and is
shared by assembly accounting and drape. Estimate consent, units, sources, conditions and reported
bounds retain their ordinary meaning. Unknown mass stays unknown in accounting and cannot drive
a physical point attachment.

The source corners resolve through the actual simulation mesh and seam weld map. Mirrored instances
are explicit; welded aliases accumulate their declared shares without multiplying the group total.
After sheet materials and seam welds are established, the shares augment the existing lumped mass
vector, so sewing, draping, static solves, elastic-grip weight and prescribed motion all consume the
same gravity and inertia. Material substitution rebuilds sheet properties and reapplies the original
extra groups. Stale edits to mass or retained reference topology refuse before accepted evolution.

Complete assembly accounting merges pattern groups and request extras by exact typed source
identity. Identical repeated observations count once; conflicting observations refuse independently.
Aggregate studies charge the merged distinct inventory, with no duplicate charge or source-only
budget bypass. `inventory_complete` remains an explicit caller declaration. Simulated cloth mass,
attached group mass, total inertial mass and full cut-assembly mass remain separately reported.

The drape receipt retains original attachment inputs and hashes, source masses, allocation residuals
and original/welded vertex mappings. Native commands, file-free WASM and the browser workbench expose
the same results and independent refusals. Cloth thermal storage continues to use characterized sheet
materials and reference triangle areas; point masses do not imply accessory heat capacity, geometry,
contact shape, stiffness or fastener strength.

Independent synthetic checks cover static force balance, implicit inertia, mirrored/welded targets,
material substitution, unchanged sheet heat storage, stale-state refusal and transactional motion.
Actual native/WASM scenario studies recompute changed sheet response while retaining attached source
identity. Physical costume/fastener validation, rigid accessory dynamics and separately characterized
accessory thermal/contact properties remain future work. See
[attached-mass.md](docs/attached-mass.md) for the complete input and receipt contract.

---

### 25.34 Material substitution chooses qualified sewing recipes

The material study now accepts the existing finite `ftx-sewing-selection/1` or timed `/2` request as
`sewing_selection`, mutually exclusive with fixed `sewing` options. Every frozen material
alternative reruns the production selector against its own exact material identities. The
selected needle/thread/stitch/feed recipe supplies the ordinary sewing plan and comparison
metrics; the complete search receipt retains all tested choices, qualification refusals,
budget exhaustion and whether the listed optimum was proved. An unfinished search may
provide a valid incumbent, but a search without an incumbent cannot satisfy required sewing
or become a zero-cost winner. Cutting, mass, purchasing and physical consumers stay independent.

Aggregate recipe-plan, operation and ordering limits bound a whole material study, including
Cartesian material combinations and scenario studies. Native and WASM entry points use this
same evaluator. This closes the software gap between material substitution and automatic
process selection (`ftx-mr1`); it does not supply missing measured seam qualification, complete
robot cycle times, sensing, kinematics or hardware execution under `ftx-sp2`/`ftx-tm7`.

### 25.35 Viscoelastic work heats moving covered and exposed textile layers

`motion.transient_coverage.relaxation_dissipation_heat` now admits the same explicit
`uniform_reference_thickness` policy as independent sheets. Source-qualified Maxwell dashpot
work follows its retained material triangle through body coverage, layer order and exposed
regions. Each channel needs its own conversion observations and temperature domain. Maxwell
and permanent-stretch heating retain separate source and energy ledgers, then add heat once
before material-depth allocation. Initial drape work, rejected trials, numerical spring loss
and reversible stored energy do not become heat. Independent-sheet and coverage calculations
share captured work arrays while remaining separately reported thermal domains.

Missing evidence, out-of-domain temperature and work-budget failures preserve successful
mechanics and independent consumers. Scenario comparison checks source identities and the
full work/heat balance before admitting these outputs. Synthetic sewn-garment regressions
independently integrate covered/exposed reference-mass enthalpy and verify shared capture,
mixed-source conservation and refusal isolation (`ftx-vh1`). Matched empirical conversion
laws, contact-driven local compression/recovery, moisture coupling and garment experiments
remain open under `ftx-cp2`, `ftx-tmc` and the specimen/garment acceptance gates.

---

### 25.36 Finite-strip characterization through native material consumers

`ftx materials strip-experiment` and `calibrate-strip`, plus their `ftx-materials` counterparts,
now expose the shared finite-strip forward experiment and inverse material authority. Actual
clamped geometry and free interior deformation determine force and midpoint chord width. The
fitter requires independent held-out acquisitions, stationary bounded searches, local sensitivity,
mesh refinement and production replay inside its FIT-only strain support before publishing an
ordinary source-locked estimated material. Existing characterized domains constrain every refit
candidate and cannot be widened by replacement, probes or CHECK data. Output estimate admission
changes the recorded consumption policy, never the fitted physics.

The native consumer increment (`ftx-ki2`) preserves complete receipts, exact selection JSON,
independent missing-property readiness and ordinary binding. A rejected model retains diagnostics
without a selection; work exhaustion and physical solve failures refuse. The executable synthetic
fixture exercises actual forward/inverse behavior but leaves absent bending, contact, thickness
and thermal evidence unavailable. See
[clamped-strip-calibration.md](docs/clamped-strip-calibration.md) for requests and replay commands.

The retained Sperl 2022 acquisitions now include all nine original transverse-contraction
CSVs for SJ1, SJ3 and DKIN8: **66 observations and 1,736 original bytes**, with archive paths,
SHA-256 identities and the existing source metadata and MIT notice. The paper defines the
reported quantity as one minus current width divided by the 60 mm reference width. The
fixture preserves its dimensionless values, exact nodes and original line endings.

The source does not identify an exact width station or edge statistic, nor transverse
loading/unloading labels or timestamps. Those gaps remain explicit. A finite-strip inverse
experiment must declare and retain its width observation operator and mechanical assumptions;
these measurements are neither local strain nor a directly measured Poisson ratio. The
existing catalog scalars, material identities and supported capabilities remain unchanged.

The matched acquisition data supports the finite-strip inverse-characterization
gate `ftx-ki1`. Directional force histories alone do not guarantee that four constitutive
coefficients can be distinguished. The shared inverse consumer checks parameter sensitivity,
freezes fitting decisions before held-out evaluation and rejects an inadequate law. Source
hysteresis, unknown timing, independent coupons and matched bending/contact/thermal evidence
retain their separate acceptance requirements. See
[measured-knit-strips.md](docs/measured-knit-strips.md) for the retained observations and scope.

---

### 25.37 Browser finite-strip identification and source-preserving assignment

The file-free WASM boundary and material workbench now expose the same finite-strip inverse
authority as the native commands (`ftx-ki3`). The user loads source-bearing acquisitions,
declares consumption of output estimates, reviews independent FIT/CHECK errors and actual
grip-force/width predictions, then assigns only an accepted frozen material selection.
Geometry, warp axes, search bounds, numerical work, doubled-mesh change and enforced-domain
replay remain visible. Source text is displayed literally and original JSON downloads retain
their exact bytes; numbers are not reconstructed from the display tables.

Completed unaccepted fits retain diagnostics without a material selection. Physical refusals,
work exhaustion and changed inputs withdraw stale output. A separate forward WASM export permits
independent strip experiments using the ordinary source/material authority. Real compiled-WASM
worker checks compare the complete synthetic fit and exact selection with native results and
exercise forward replay, ordinary pattern binding, missing mechanics and refused replacement.
This delivers the interactive characterization-to-assignment path; it does not supply a missing
source width-measurement operator or establish measured-knit and garment accuracy under `ftx-ki1`.

---

### 25.38 Source acquisitions drive actual clamped-strip identification

**Delivered experiment and identification software (`ftx-kf1`, continuing `ftx-ki1`).** The
existing production membrane and Newton solver now evaluate finite strips with the actual free
gauge, width, material direction, fully fixed grip edges and free in-plane interior contraction.
Forces are sums of grip reactions in newtons; prescribed grip extension is a boundary condition,
never an assertion of homogeneous fabric strain. Source-frozen scalar or tabulated forward laws
retain their estimate, form and strain-domain authority. The concurrently delivered continuation
path API remains available; inverse experiments initialize each declared elastic state independently.
The explicit model is planar, quasistatic and memoryless. It does not identify slip, out-of-plane
wrinkling, rate, hysteresis, bending or contact from force/extension observations.

Bounded identification fits warp/weft stiffness, major Poisson ratio and shear stiffness through
these equilibria. Three deterministic bounded starts use FIT acquisitions only. Publication requires
stationarity, interior coefficients, independent local sensitivity, no discovered equivalent separated
minima, independent CHECK acquisitions, declared force/width residual limits, doubled-mesh agreement
and actual forward replay with the final FIT-only strain bounds enforced. Renamed source keys,
changed meshes and antipodal material axes cannot turn the same acquisition into independent CHECK
evidence. CHECK observations cannot select coefficients or enlarge their eventual domain. Candidate
evaluations, finite differences, refinement and replay share one work budget. Failed completed fits
retain diagnostic reports with no material or selection; invalid or exhausted operations refuse.
Coefficients remain model estimates and unrelated properties remain unknown.
When replacing a previously characterized law, its inherited strain domain constrains every trial,
probe and replay, and the final domain is their intersection. An exploratory probe outside prior
support can refuse replacement even when the existing frozen selection itself replays.

**Retained source extension (`ftx-ka1`).** The Sperl et al. dataset adapter now exposes the complete
nine force histories for SJ1, SJ3 and DKIN8 and nine newly acquired original transverse-contraction
CSVs. Original source bytes, archive entries, independent SHA-256 pins, signed observations,
loading/unloading branches, full metadata and the MIT notice/README are retained. The adapter verifies
the source's 100 mm free gauge and 60 mm width without inventing rate, conditioning, replicate or
uncertainty data. Transverse compression remains the reported dimensionless observation. Its spatial
sampling and branch correspondence are not established, so it is not automatically converted into a
solver's material-midpoint width or a Poisson ratio.

`ftx materials` and `ftx-materials` expose `strip-acquisition`, `strip-experiment` and
`calibrate-strip`; shared WASM exports drive the same browser workbench. Exact input text and reports
are downloadable. Only accepted results expose an assignable frozen `selection_json`, preserving the
Rust serialization and caller's explicit estimate policy. The workbench displays candidate bounds,
FIT/CHECK residuals, excitation, refinement, source rows and rejection reasons; stale requests cannot
publish a selection. Reusable synthetic and real-source preparation examples are described in
[clamped-strip-identification.md](docs/clamped-strip-identification.md).

**Observed result and highest-priority continuation (`ftx-ki4`).** The synthetic example recovers
its four declared coefficients and passes frozen replay. The independently held-out numerical cases
test the software; they are not physical specimens. A reproducible SJ1 attempt using exact 10/20/30%
warp/weft loading observations as FIT and the bias cut as CHECK completes with no published law:
the candidate reaches parameter bounds, the starts do not reach stationarity, force residuals exceed
their budgets and CHECK strains escape the FIT-only domain. This failed bounded attempt is not a
global impossibility proof. Extend identification through the shared nonlinear material authority,
qualify an area-preserving response where justified and resolve the transverse measurement operator;
do not weaken acceptance gates or invent local strains to make a card pass. Matched independent
bending/contact characterization, specimen uncertainty and the fixed-pattern linen/leather/knit
garment gate remain open under `ftx-tm2` and `ftx-tm8`.

### 25.39 Declared nonlinear shapes enter the clamped-strip inverse solver

**Delivered capability (`ftx-ki5`, continuing `ftx-ki4`).** Scalar strip identification cannot express a
knit's changing tangent stiffness. The existing finite-strip authority now accepts an explicitly
declared `scaled_piecewise_linear/1` response family: three source-bearing signed Green-strain
curves, three positive dimensionless amplitude scales and one signed normal coupling in N/m.
Knot locations and relative branch shapes remain caller-supplied assumptions. Every candidate uses
the existing `TabulatedMembrane` compiler and production equilibrium solver; no parallel material
engine, inferred yarn law, symmetry assumption or hidden area penalty is introduced.

Keep the full parameter box stable, enforce authored and inherited domains during all trials,
and preserve three-start stationarity, excitation, FIT/CHECK separation and doubled-mesh gates.
Accepted publication restricts each piecewise-linear curve to FIT-only support while retaining
interior knots, then solves and checks the actual published law again. The final report must describe
that replayed law's force/width residuals and numerical refinement. Source and domain metadata remain
validated when the result enters ordinary material consumers. Estimated shape nodes, estimated
acquisition observations and estimated output consumption require independent explicit permissions.
Missing unrelated material properties stay unknown.

The same public native/WASM requests and browser workbench expose this optional family, with
report-owned parameter names/units and exact frozen selection downloads. Existing scalar requests
retain their serialized identities when the family is absent. A reproducible synthetic nonlinear
example recovers dimensionless scales `[1.1, 0.9, 1.2]` and coupling `40 N/m`, passes actual
publication-domain replay and freezes a directly reusable selection. Native/WASM/worker comparisons
cover accepted and rejected nonlinear requests, independent permissions and exact output identities.
These numerical checks do not qualify an industrial knit.

**Empirical continuation remains open.** The Sperl source fits an area-preserving shell model using
force and contraction in all three cut directions. Its fitted coefficients cannot supply an
independent shape prior for a later bias CHECK, and its area terms are not equivalent to this
separable curve family. Obtain independently justified shapes, qualify a shared area-response
extension where supported, and establish the transverse measurement operator under `ftx-ki4`.
Preserve the independent bending/contact and garment gates in §25.38. The detailed schema,
permissions and executable examples belong in
[clamped-strip-identification.md](docs/clamped-strip-identification.md).

---

### 25.40 Continuous-cut limits qualify actual marker purchases

The existing quote authority now distinguishes total inventory from a source's maximum uninterrupted
linear cut. Roll procurement passes each computed marker's actual length into the shared price
evaluator. A known shorter limit excludes the offer even when total inventory is ample. Unknown
limits remain explicitly unknown unless the caller requires an observed limit; a maximum is never
reported as guaranteed continuous inventory. Latest-observation, exact material-lock and whole-order
quantity rules remain authoritative. No physical marker is split by dividing its area arithmetically.

Shopify, JSON-LD and CSV refresh paths retain reviewed limits with checked source text or columns,
and specification review detects changed or removed limits. A fresh 2026-10-21 APEX observation keeps
its five-yard maximum and observed inventory separately; the prior snapshot is preserved. That
catalog record remains fill, so it is exercised through the quote evaluator rather than treated as
a cuttable characterized sheet. Actual marker, native CLI, shared WASM and material-study tests use
explicit synthetic sheet selections (`ftx-pc1`). The primary `ftx materials` command now exposes the
same cut-plan, cut-price and cut-svg implementations as the companion workbench. See
[cutting-plans.md](docs/cutting-plans.md) and [textile-price-refresh.md](docs/textile-price-refresh.md).

---

### 25.41 Independent nonlinear interval tangents from finite-strip acquisitions

**Delivered software (`ftx-ki6`, extending `ftx-ki5`).** The existing bounded strip inverse now also
identifies 7–12 independent signed tangent intervals and constant normal coupling through the same
production membrane authority. Warp, weft and engineering-shear knot positions are declared fixed
model coordinates; relative interval stiffnesses are fitted. Compression/tension and opposite shear
branches remain independent, with no inferred symmetry or missing branch. This optional mode is
mutually exclusive with the preserved four-parameter declared-shape family. Existing scalar and
fixed-shape request identities and parameter meanings remain intact.

The complete search box must be stable. Whole-acquisition FIT/CHECK separation, stationary bounded
starts, local excitation, equivalent-minimum checks, numerical refinement and aggregate work limits
govern publication. Unexcited intervals cannot become accepted material properties. Prior fitted
metadata, compiled curve support and declared knot support constrain all trials. Accepted curves
are restricted to FIT-only support; the actual cropped material is compiled and replayed, and its
force/width residuals and mesh changes must pass again before an exact frozen selection is returned.

Five focused interval tests and the preserved scalar/fixed-shape regressions pass. Native commands
and the shared portable material API retain the same descriptors, source identities and ordinary
selection replay. The executable synthetic acquisition recovers nine known coefficients, including
positive-strain stiffening and unequal signed shear tangents. It is numerical software evidence;
`ftx-ki4` remains open for measured-knit model adequacy, qualified transverse observations, independent
specimens and matched bending/contact and garment acceptance. Detailed inputs, bounds, permissions
and the executable example are in
[clamped-strip-identification.md](docs/clamped-strip-identification.md#identify-independent-signed-tangent-intervals).

---

### 25.42 Joint-outfit material workbench and nonlinear characterization display

**Delivered software (`ftx-oi1`).** The browser material workbench imports an ordinary
`ftx-outfit-drape/1` request through the production WASM composer, retaining exact source bytes,
the initial composition receipt and the exact composed pattern. Material assignments can target
the whole outfit, one garment or one source piece. Existing binding, drape, cutting, purchasing,
sewing and material studies consume that composition; outfit imports require self-contact in both
the page and worker. Source-qualified attached mass and independent elastic casings retain their
existing composed-pattern behavior. Replaced requests invalidate dependent results.

The original composition receipt remains scoped before substitutions. Current bound patterns and
downstream reports retain their own source identities. The browser uses ordinary composed-pattern
drape and does not append the native outfit wrapper's runtime garment-ownership annotation. It
does not infer a dressing sequence, physical fit, missing material interfaces or new thermal boundaries.

Independent nonlinear interval results display every coefficient's descriptor, unit and search bounds
alongside the declared fixed strain knots. Purchasing displays the actual marker length and each
supplier's observed continuous-cut limit independently of total stock, retaining unknowns and exclusions.
All 71 frontend/compiled-worker checks pass with zero skips: the actual WASM interval report and frozen
selection equal native output, a held-out refined strip replays through the ordinary material authority,
and an outfit material substitution doubles computed assembly mass without changing geometry. Two
native outfit-adapter tests additionally check actual joint-solver propagation and source refusals.
These are software integration results; the measured-material and garment acceptance gates remain open.

---

### 25.43 Whole-piece roll partitions and combined material orders

**Delivered core (`ftx-rp1`, continuing `ftx-pc1`).** A caller may declare
`maximum_marker_length_m` for the cutting table or requested continuous segments. The existing
grain-constrained planner assigns complete physical instances to deterministic bounded shelves across
multiple markers of the same exact material snapshot. Both end margins count on every marker; an
oversized piece refuses. Mirroring, on-fold expansion, internal cuts, drills, notches, grain and affine
transforms remain unchanged. Existing polygon compaction runs within each assigned marker. Counts and
total length are heuristic; a feasible partition does not prove globally minimal waste or marker count.

Purchasing groups all markers of one exact material into one whole order. Summed gross area drives
minimum quantities, increments, prices and total observed stock; the longest actual marker drives the
continuous-cut upper-limit check. Every marker retains its order membership and length. Costs live once
on the material order, never once per marker, and a missing material offer keeps the total unknown.
Different material revisions remain separate. Quoted increments describe the whole order; requested
segmentation does not establish a supplier's per-cut charging rules, continuous inventory or fulfillment.

Omitted limits preserve legacy options and `/1` plans; bounded plans use `/2`. Repeated-material
procurement uses `/2` with explicit `material_orders`; ordinary single-marker purchasing preserves
its existing result shape. Focused geometry, source-identity, compaction, finite-stock and preview tests
cover the core. Native, portable and browser consumers share these same planners. Details and commands:
[cutting-plans.md](docs/cutting-plans.md).

---

### 25.44 Source-qualified area response in the production membrane

**Delivered software (`ftx-ar1`).** An optional `area_green_strain/1` response extends the existing
source-bearing directional membrane with `quadratic * (J - 1)^2 + log_squared * (ln J)^2`, where
`J = sqrt(det(C))` is the positive physical sheet-area ratio. This follows the area-energy form in
Sperl et al. (2022), §5.1 equation (1), with no extra half factor. Its extension to authored nonlinear
directional curves is explicit. The same material compiler, triangle assembly and production solver
evaluate exact energy, stress and tangent; there is no separate strip-only material implementation.

Both coefficients require ordinary source-bearing quantities in N/m, matching specimen and conditioning
across the base and area observations, explicit estimate admission and a declared positive J interval.
That interval intersects all existing strain and identification domains. The area energy acts on raw
physical strain even when the separable contribution uses gather softening. Its tangent can be indefinite;
the production element uses the existing full-Hessian projection path. The positive metric and unsigned
area ratio do not establish orientation preservation, global convexity or collision freedom.

Reference stiffness includes the area contribution. Material reports retain the J domain only when the
optional law is enabled, and frozen selections bind all coefficients, sources and conditions. Omission
preserves legacy serialization and arithmetic. Existing inverse fitters refuse to discard an area law
even when replacement is otherwise allowed; identifying its extra coefficients requires an area-capable
extension of the existing bounded inverse solver (`ftx-ai1`).

The committed paired coupon requests differ only in the area law. At 10% grip extension, the synthetic
demonstration changes reaction from 1.090489 N to 1.304122 N and midpoint width from 49.318030 mm to
52.395037 mm through the ordinary production solve. Analytical derivative checks, physical-strain
assembly, exact frozen replay and native/portable adapters cover the implementation. These numerical
results do not qualify an empirical textile or a complete garment material. Source transverse operators,
independent acquisitions and matched bending/contact remain open under `ftx-ki4`, `ftx-tm2` and `ftx-tm8`.
The model, commands, source citation and executable examples are documented in
[material-area-response.md](docs/material-area-response.md).

---

### 25.45 Potential vapor transfer from canonical source observations

**Delivered software (`ftx-vp1`).** The canonical material authority now resolves measured
`vapor_resistance` into a bounded, common-area series network. Explicit inside/outside vapor
pressures, boundary-film resistances, area and caller-supplied latent heat determine conditional
potential heat and mass transfer. The report retains every material's source-qualified Ret, method,
conditions, evidence and content lock, plus layer/film pressure drops and exact frozen replay JSON.
The native primary/companion commands and file-free WASM boundary share this one implementation.

The existing ten measured bovine-leather groups can now drive this consumer without fabricated
mechanics or heat storage. Their 35°C air, 40% RH and 1 m/s source conditions remain visible;
reported SD/CV is not an uncertainty interval. Exact required method/condition filters, independent
estimate admission and explicit mixed-protocol consent remain enforced. Source observations are
separate from the assumed fixed-resistance, isothermal network; adding measured layers does not
make the assembled network a measured specimen.

Analytical layer/film balances, equal-pressure and material-substitution cases, finite arithmetic,
source refusals and exact native/portable offline replay verify this software path. Water supply,
actual sweat evaporation, condensation, moisture storage, temperature/humidity dependence, dry/wet
energy coupling and garment comfort remain open under `ftx-tm5` and `ftx-tmc`. See
[vapor-resistance.md](docs/vapor-resistance.md) for the request, equations and source references.

---

## Appendix A. Sources

- Brégier et al., "Human Mesh Modeling for Anny Body", arXiv 2511.03589 (2025); github.com/naver/anny.
- Li et al., "Incremental Potential Contact", SIGGRAPH 2020; Li et al., "Codimensional Incremental Potential
  Contact" (C-IPC), 2021; ipc-toolkit (MIT).
- The reference implementation's behaviour specifications: `docs/spec/01…04`; the Anny forward model: `docs/spec/05`.
- Research behind the reference (body measurement and body models; formats and manufacturing; Blender
  sewing; the frankensim cloth survey; the photogrammetry port survey), summarised where used above.
