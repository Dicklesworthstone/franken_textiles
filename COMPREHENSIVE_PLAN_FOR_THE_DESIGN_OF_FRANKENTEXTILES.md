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
| Start-state sensitivity; a strained start launches the jacket | Each step minimises an incremental potential with backtracking; resolvable increases refuse, while roundoff-limited full trials require derivative progress (§25.93) |
| Collision jitter 0.2–0.5 mm/frame; "settled" was a drift statistic | Newton converges each step to a tolerance; settled is a residual and velocity criterion |
| Collider inset (else every girth gains 4.4 cm) | Barrier on the gap minus the real cloth half-thickness; clearance reported exactly |
| Cloth squeezed into the arm–torso wedge unstable | One smooth barrier potential; an equilibrium exists whenever a feasible gap exists |
| Cannot judge whether an elastic waistband holds | Coulomb friction with lagged normal forces plus a tension-only prestressed elastic ring; closed-form known answers (§11.9) |
| Isotropic springs | Orthotropic membrane (warp, weft, bias shear) rotated to each piece's grainline |
| Gathers via vertex-group stiffness | Per-material-direction tension/compression split, a material field |
| No intersection check | Exact triangle–triangle audit of the start and end states |
| Needs a Blender install; not in a browser | Std-only Rust core: a CLI and a wasm build |

What it does not fix: cloth energy is non-convex, so different starts can settle in different local minima.
Production routes therefore expose an explicit **two-path sewing-duration sensitivity** check: native
`--two-start-sew` and the browser fold-up checkbox run the same bounded 12-vs-18 policy. It executes a
second full solve, so ordinary runs remain one-path and must say that path sensitivity was not checked.
The resulting spread is review evidence, not an error bar, confidence interval or uniqueness proof.

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
two-start spread when that explicit sensitivity check is requested.

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
preserves legacy serialization and arithmetic. Inverse fitters refuse to discard an area law even when
replacement is otherwise allowed. The bounded strip fitter (`ftx-ai1`) explicitly preserves qualified
fixed coefficients or identifies both alongside declared directional shapes/signed intervals, through
the same production compiler, complete work budget, FIT-only support and independent CHECK gates.

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

The material workbench accepts a standalone vapor-stack request, shows potential heat/water exchange
and source-qualified layer pressure profiles, and downloads exact input/report/frozen replay bytes.
It also displays partial-seam correspondence from §25.46, including full source and traveled lengths.
Compiled-worker outputs for both features match complete native reports; replacing or refusing input
withdraws obsolete results. No browser-side physics or replacement material observation is introduced.

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

### 25.46 Partial-seam sewing operations for changing construction

**Delivered software (`ftx-sp3`).** A sewing pass can select an interval of original side-A
arclength, with side B derived through the existing oriented notch map. Exact selected endpoints,
enclosed junctions/notches and both-side stitch pitch survive ordinary and offset paths. Separate
sections of one source seam can now use distinct explicit fold/reinforcement stacks, material
thickness, feed direction and lock recipes. This closes the previous inability to act on the
assembly planner's instruction to split a seam into constant-layer operations.

The union of passes must cover both complete original seam sides; overlap is allowed and gaps
refuse. Different offsets retain their source mapping and receive clearance checks on traveled
polylines and stitch chords. Reverse/backtack expansion stays within the selected station table.
Full-source seam acceptance, material/qualification limits, ordering and hardware refusal remain
authoritative. Explicit spans use plan schema `/10`, retaining original ranges/full lengths and
one-way traveled lengths. Omitted spans preserve existing whole-seam serialization and arithmetic.

Focused core, real CLI and portable checks cover local reinforced/folded sections, material-only
thickness substitution, curved/notched/reversed endpoints, clearance outside a selected span,
complete A/B coverage and expanded interval/time accounting. The synthetic 100 mm split seam
travels 60 mm forward and 40 mm reverse in 40 intervals at four seconds of continuous feed.
These checks do not establish seam quality, physical fold transitions or robot execution;
source-qualified seam coupons and calibrated machine handoff remain open under `ftx-sp2`.
See [material-sewing-plans.md](docs/material-sewing-plans.md#partial-seam-operations-and-local-construction-changes).

---

### 25.47 Joint area and directional identification from homogeneous swatches

**Delivered software (`ftx-ai2`).** The existing nonlinear swatch workflow can jointly identify
directional knot resultants, normal coupling and both physical-area coefficients from complete
deformation gradients and observed nominal-force channels. Its two additional design columns come
from the production constitutive evaluator. The existing deterministic least-squares and normalized
excitation gate apply to the complete parameter vector; unsupported branches, negative coefficients,
unstable base laws and excessive FIT or independent CHECK residuals refuse without projection.

An explicit area-model request declares admissible J bounds and matching specimen/conditioning.
Only FIT observations determine the published strain and physical-area domains; J includes the
undeformed reference, and neither CHECK observations nor replacement permission widen inherited
support. Existing area applicability conditions, including additional temperature or treatment
qualifiers, survive replacement. The three curves, coupling and two area quantities share one fitted
source and exact domain metadata. Removing the area object or mixing their sources/domains refuses.

The ordinary material, frozen selection, native commands and file-free WASM adapter carry this law
directly into the production finite-strip experiment. A seven-state independent analytic example
identifies nine unknowns, recovering synthetic area coefficients 40 and 15 N/m with six FIT states
and one independent CHECK state. Focused tests cover known answers, held-out isolation, physical
support, replacement, estimate admission, source retention and actual forward replay. Other mechanics
remain absent unless separately characterized; this result supplies no bending, mass, contact or
thermal properties and does not establish an empirical textile or garment fit. The measured-material
gate remains under `ftx-ki4`; bounded clamped-strip identification continues under `ftx-ai1`.
See [material-area-response.md](docs/material-area-response.md) and the executable
[swatch request](examples/area-identification/swatch.json).

---

### 25.48 Physical-area support from the actual inverse strip experiment

**Delivered software (`ftx-ai3`, extending `ftx-ai1`).** Joint area identification now observes
physical J at every production triangle in the accepted coarse and refined FIT states. The published
interval contains the explicitly simulated reference and intersects inherited source qualification.
It is independent of the separate directional strain extrema; box corners are not acquisitions.
CHECK states cannot widen either domain. Publication and ordinary frozen-law replay use this exact
support, and all six identified components retain one source and matching fitted-area metadata.

The existing Preserve/Identify API and both declared-shape and independent-interval modes remain.
Preserve mode keeps independently qualified area observations and their original interval unchanged.
Identification never uses preserved CHECK acquisitions as a fixed prior. Bounds retain a positive
representable area tangent throughout the complete search box; mesh, stationarity, excitation,
aggregate work and independent residual gates remain authoritative.

The canonical synthetic strip request recovers the six coordinates `[1.1, 0.9, 1.2, -80, 31, 19]`,
passes refinement and replays the exact published selection. Three focused support tests and the
twelve restored upstream nonlinear tests cover recovery, CHECK isolation, source/domain erasure,
unchanged preserved evidence and full solve accounting. These are numerical software checks;
independent acquired textiles, transverse operators, bending/contact and garment fit remain open.

---

### 25.49 Material-bound vapor transport in fixed-pattern studies

**Delivered software (`ftx-vs1`, supporting `ftx-ms2`/`ftx-tm5`).** An optional explicit common-area
vapor stack now follows each study alternative's final source-piece or default material assignments.
The existing canonical vapor evaluator supplies the full source-bearing report and frozen offline
replay. Its result or refusal remains independent of cutting, sewing, purchasing, mass and mechanical
scenarios; missing Ret is unknown, while missing mechanics do not block the vapor consumer.

Declared potential latent-power and mass-flow objectives participate in the existing bounded Pareto
comparison. Only consumed layer occurrences supply the vapor protocol fingerprint, and differing
reported protocols require explicit admission. Stack area, ordering, pressure boundaries, films and
latent heat remain caller inputs, shared across alternatives; pattern membership does not establish
overlap, body coverage, air-gap transport or actual evaporation. Source intervals do not become output
confidence bounds. The native, portable and browser study paths reuse the same evaluator.

Known series-network answers, final assignment changes, independent consumer refusals, exact frozen
replay and protocol isolation verify this integration. The runnable example uses existing measured
leather Ret records with explicitly illustrative boundary assumptions. Coupled moisture/heat, matched
measured layers and garment acceptance remain open under `ftx-tmc`, `ftx-tm2` and `ftx-tm8`; a potential
transfer comparison does not establish sweating or comfort. See [vapor-resistance.md](docs/vapor-resistance.md).

---

### 25.50 Source-declared virtual extensometers in strip identification

**Delivered software (`ftx-vo1`, supporting `ftx-ki4`/`ftx-tm4`).** Strip observations can now declare
up to eight transverse channels at explicit original-gauge stations. Each measures a Euclidean
side-material-point chord or its signed projection on an explicit unit axis in the strip frame.
The production displacement field supplies the observations; refinement preserves their material
stations. The existing midpoint channel and omitted-field behavior remain unchanged.

Every explicit channel participates in the existing FIT objective, sensitivities, mesh checks,
independent CHECK and final published-law replay. IDs, operators, predictions and residuals travel
through the ordinary native/portable reports and browser display. Bounded channels, finite coordinates,
valid axes and positive scales are required; neither missing source operators nor forces are inferred.

Independent affine/interpolation answers and synthetic inverse/forward regressions establish the
software behavior, including signed projections away from the midpoint and held-out isolation.
The retained real-knit transverse curves still need their actual acquisition definition and branch
qualification, so `ftx-ki4` remains open. This extension introduces no measured coefficients, new
constitutive model, garment-fit claim or source-data substitution. See
[clamped-strip-identification.md](docs/clamped-strip-identification.md).

---

### 25.51 Measured yarn-to-knit construction matrix and the next coupled consumers

The delivered materials expansion adds the 2022 Salopek Čubrić et al. sportswear matrix
(DOI `10.3390/ma15093306`) to the existing catalog: five separately characterized yarns
and 25 knit constructions, taking the built-in corpus from 221 to 251 records. Cotton,
wool, PAN bouclé, wool/PAN blend and PA filament each retain single plain, single miss,
single tuck/piqué, double plain and double Milano rib variants. Tables 2–7 supply
measured yarn and construction observations; original units, specimen identities,
protocols and unresolved source discrepancies remain attached to every selection.

This closes a concrete coverage gap in `ftx-tm2`, without turning constituent yarn
tenacity into a fabric modulus. Shared mass, screening, dry steady-resistance and
vapor-resistance consumers use the new records immediately. The executable
`screen-sportswear.json` comparison shows the measured mass/insulation/vapor tradeoff
between wool plain and piqué constructions. Missing mechanics and heat storage stay
missing. Loaded gauge thickness cannot become sewing/contact clearance, and volume
airflow measured through different areas cannot become unqualified permeability.
See `docs/sportswear-knit-materials.md`; the delivered slice is tracked as `ftx-kn2`.

Two delivered product slices use the same physical authority and existing solvers:

- **Timed production strip experiments (`ftx-sh1`).** Prescribed grip loading,
  holding and unloading must preserve accepted Maxwell or permanent-strain history,
  report dynamic residuals and separate stored energy, free-vertex inertia and
  dissipated work, and reject an entire failed request. Source-qualified areal mass
  is required; unrelated contact, bending and thermal properties are not. Native and
  browser consumers must replay the same frozen material and complete time program.
  Independent analytic responses and time refinement establish software behavior;
  measured history identification and garment adequacy remain separate gates.
- **Material-following vapor potential (`ftx-vc1`).** Reuse canonical measured `Ret`
  observations and explicit vapor pressures/films on retained drape triangles.
  Fixed-pattern and moving scenario studies must recompute the current surface area
  after each material substitution/pose, without requiring dry conductivity or heat
  capacity. Report independent-sheet potential heat and mass flow, source conditions
  and exact replay inputs. Do not silently interpret summed sheet potentials as
  whole-wearer evaporation or inject latent cooling into the separate dry solver.

The same scenario comparison now accepts the independent current-sheet vapor heat and
mass flows as explicit nominal objectives or constraints, alongside cost, mass and the
other existing consumers. Fresh source/body/scenario identities, settled geometry and
clear simulation gates remain required. Comparison checks the set of consumed vapor
observation methods, conditions, evidence and units; canonical-column deduplication or
different measured Ret values alone do not cause false protocol mismatches. Missing
results and incompatible source qualifications remain visible exclusions.

The next coupling gate (`ftx-vc2`, under `ftx-tmc`) builds on the fixed-frame covered-body
and interlayer vapor topology now delivered by `ftx-vc3` (§25.54). Remaining work includes
moisture availability/storage, moving layered coverage and conservative latent-energy feedback.
It needs independently qualified wet/dry transport and boundary conditions, conservative
moving-area/mass accounting and held-out layer validation. Existing geometry-derived
thermal coverage is the shared geometry authority; a second body mapping or material
database is not warranted. The broader linen/leather/performance-knit garment gate,
finite-stock supplier qualification and physical sewing/robot qualification remain open.

---

### 25.52 Fresh strip identification without prior material coefficients

**Delivered software (`ftx-ai4`, supporting `ftx-ki4`/`ftx-tm4`).** The bounded clamped-strip fitter
can identify physical-area response directly from acquisitions and explicit trial domains without
requiring a prior membrane or fabricated area observations. The same area policy now supports a
scalar orthotropic base as well as declared shapes and independent nonlinear intervals. Four scalar
base coordinates convert exactly to the ordinary canonical directional law; the two area coordinates
remain separate and every trial uses the existing production compiler and equilibrium solver.

Explicit J support intersects inherited qualification. Scalar bases require signed Green-strain
trial support; specimen and conditioning remain explicit. All coordinates retain the existing work,
excitation, stationarity, independent CHECK and mesh gates. Only actual FIT states determine published
Green-strain and physical-area support, and ordinary frozen-law replay precedes selection. Preserve
mode still retains independent original area observations unchanged. The complete published response
replaces the base scalar properties so downstream consumers cannot silently lose the area term.

An executable synthetic request retains all thirteen observations from the prior area example while
removing prefilled properties and response shapes. Recovery, independent analytic energy/stress,
source/domain refusals and frozen forward replay cover the new path. Existing native and portable
entry points expose the resulting material and its explicit base-coefficient descriptors. This
software prerequisite does not close matched empirical knit characterization, bending/contact,
thermal qualification or real-garment fit. See [material-area-response.md](docs/material-area-response.md).

---

### 25.53 Complete cutting jobs across rolls, hides and remnants

**Delivered software (`ftx-cj1`, supporting material substitution and manufacturing).** One explicit
job now routes every original source piece to roll stock or finite stock, then checks the complete
physical inventory of pieces, instances and copies. Both existing planners retain the same frozen
pattern, source-piece identities, material snapshots, grain restrictions, holes, drills and cut paths.
Copies and effective seam allowance must agree. A missing, duplicate or failed route refuses the
complete job; a valid leather shell no longer prevents its roll-fabric lining from being planned.

Optional purchasing evaluates both routes against one dated market and quote book. A finite item's
bound offer cannot also supply roll stock. The total is available only when every engaged route has
complete observed quantity and offer coverage in the same currency; unknown prices never become a
partial cheap total. The result retains independent route reports, full replay inputs, finite-stock
identities, allocated area and source-qualified stock mass. Fixed-pattern studies and their existing
bounded comparisons recompute this complete job after each material assignment.

Native commands, portable bindings and the browser workbench share this core. The executable
`examples/mixed-cutting/` case allocates mirrored shell parts and on-fold lining across two copies.
Its declared synthetic stock and offers demonstrate exact allocation and whole-job cost accounting,
not supplier availability, optimal nesting, physical manufacturability or machine execution.
See [mixed-stock-cutting.md](docs/mixed-stock-cutting.md).

---

### 25.54 Layered vapor paths on actual covered body regions

**Delivered software (`ftx-vc3`, the geometry/network slice of `ftx-vc2`).** The existing thermal
projection kernel now also supplies disjoint covered-body regions and ordered cloth layers without
requiring dry conductivity or heat capacity. The canonical vapor evaluator retains each measured Ret
and scales resistance by explicit boundary area divided by current conducting layer area. Boundary
films and caller-declared interlayer resistances use covered body area; a geometric gap supplies no
coefficient. Every encountered adjacency needs an explicit vapor interface, including declared zero.

An optional `vapor_coverage` scenario consumer rebuilds this source-bound topology from each newly
draped material alternative. It reports covered and uncovered body area, unallocated cloth, each
layer's source protocol and area, pressure profiles and covered potential heat/mass subtotals. The
complete request freezes materials and geometry for offline replay. Dry-heat or storage refusals do
not erase an available vapor result. Declared comparisons admit these objectives only for complete
coverage of the same explicit body domain and matching scenario/material/geometry identities;
partial coverage cannot rank as reduced wearer evaporation.

Analytical pressure/area balances, overlapping-layer geometry, nonuniform boundary identities,
source/refusal checks and real native/portable consumers verify this software path. Browser outputs
retain exact replay requests and distinguish independent sheets from the layered coverage result.
This fixed-frame, isothermal potential does not infer sweat supply, bare-skin transfer, moisture
storage, condensation, deformation-dependent Ret, comfort or empirical garment fit. Conservative
moisture/latent feedback, motion coverage and qualified wet/dry multilayer validation remain open
under `ftx-vc2`/`ftx-tmc`. See [vapor-coverage.md](docs/vapor-coverage.md).

---

### 25.55 Bind vapor optimization to the actual material assignments and sewn geometry

**Delivered software (`ftx-vc4`, supporting `ftx-ms2`/`ftx-vc1`).** Building on the independent-sheet
vapor objective names in §25.51, scenario comparison now admits those readings only from a complete
source- and geometry-matched result for the newly computed initial drape. Explicit constraints and
the required `vapor_surface` consumer share these checks alongside the ordinary missing-data and
settled/gating-clear admission. No surrogate area, second transport model or dry-heat dependency is
introduced.

Every reading stays tied to the bound pattern, body, scenario, material, current triangle geometry
and ownership, frozen boundary conditions and canonical Ret result. Only source protocols consumed
by each stable cloth-instance identity govern cross-alternative compatibility; column reordering,
deduplication and unused observations do not change that meaning. Explicit mixed-protocol consent
does not override stale results or unavailable inputs. Independent manufacturing and thermal results
remain available after another consumer refuses.

This completes the result-admission path for nominal initial-frame sheet potential. It does not infer
overlap, covered-body evaporation, moisture supply, latent cooling, motion-integrated transfer or
propagated uncertainty. The distinct §25.54 coverage topology and remaining `ftx-vc2` coupling and
empirical transport gates retain their own scope.
Source/freshness refusals, protocol ownership, exact frozen replay and actual production scene
substitution are exercised at the existing core and portable seams. See
[material-scenario-studies.md](docs/material-scenario-studies.md#vapor-potential-on-the-current-sewn-geometry).

---

### 25.56 Source-qualified sewing duration in material tradeoffs

The existing fixed-pattern study now exposes `sewing_setup_seconds` and
`sewing_feed_and_setup_seconds` as explicit nominal objectives and inclusive constraints.
Both come from the selected production sewing plan's qualified directed timing table,
including its declared initial setup. The existing `feed_seconds` retains its feed-only
meaning. Recipe selection and material comparison therefore evaluate the same declared
feed-plus-setup work; a faster feed rate no longer has to stand in for the complete
available timing model.

Missing timing remains unknown, including plans that count setup changes without timed
observations. A refused material-specific recipe cannot supply a cheap zero. Bounded
search retains a complete feasible incumbent and its unproved optimality status. The
browser displays feed, setup and joint duration separately with their retained evidence.
Production-planner tests verify a material-frontier change when setup time is included,
inclusive limits, absent timing and independent consumer refusals. These are software
checks; handling, acceleration, final resets and other undeclared work remain outside
the durations, and physical sewing qualification remains open under `ftx-sp2`/`ftx-tm7`.

---

### 25.57 Layered vapor potential follows the complete accepted motion

The existing `scenario.vapor_coverage` consumer now rebuilds its layer paths on every motion
keyframe. An explicit `motion.vapor_coverage_history` policy additionally evaluates every accepted
mechanical sample, preserving the same frozen material selections, physical layer names, source
filters, body-triangle identities and fixed vapor boundaries. Nonuniform pressure fields verify
their initial body identity before following those triangles through the new poses. No dry
conductivity or heat capacity is required.

The declared right-endpoint rule integrates each new sample's covered potential power and mass
flow over its preceding interval. An optional composite-trapezoid rule averages the previous and
current rates using the same projections, including the initial state. Analytical linear and quadratic
area histories verify exactness and second-order refinement without additional geometry solves.
Initial geometry, every accepted sample, projected coverage,
per-interval values and exact frozen replay remain in the report. Bounded projection work and
retention are charged before further work; failed samples refuse the complete vapor history while
retaining independent mechanics and other available consumers. A new native command and portable
WASM entry point replay the same history without a body-model installation.

Material studies can compare `motion_covered_vapor_potential_energy_j` and
`motion_covered_vapor_potential_mass_kg`, or require `motion_vapor_coverage`. Admission requires a
complete mechanical path, the current material and geometry identities, and full coverage of the
explicit body domain at every retained state. Partial covered subtotals stay inspectable without
ranking as whole-domain transfer. The browser displays initial and sampled coverage, potential
integrals, independent refusals and the exact replay download.

Analytic area/time cases, time refinement, source substitution, nonuniform boundary rebinding,
late failures and actual production-motion replay provide numerical verification. These potential
integrals depend on the declared reservoirs and motion sampling. Moisture availability/storage,
actual evaporation, conservative latent feedback and matched wet/dry measurements remain open
under `ftx-vc2`/`ftx-tmc`. See [vapor-coverage.md](docs/vapor-coverage.md).

---

### 25.58 Material substitutions choose their actual stock routes

**Delivered software (`ftx-cj2`, supporting `ftx-tm7`/`ftx-ms2`).** The fixed-pattern jacket scenario
allows the same source piece to move from linen or knit roll stock to an individually identified leather
hide without changing its geometry or rewriting the study. An explicit `ftx-cutting-job/2` request maps
each source piece's exact permitted material snapshots to roll or finite stock. Final canonical
material binding selects the route; unlisted or ambiguous snapshots refuse. Fixed `/1` routes retain
their original behavior. No fiber label, material name or proximity to an available hide chooses a route.

Retain the full immutable route, stock and source-qualified grain-permission pools for replay, but run
only the planners engaged by the current alternative. Both route options must agree on copies and
effective seam allowance even when one is dormant. Preserve complete physical inventory checks,
finite-item identity, defect/zone constraints and independent purchasing. Dormant hide offers cannot
become roll supply; missing offers cannot produce a partial cheap total. Native, portable and browser
consumers report the selected source-piece/material/stock mapping and replay the same job. Retained
dormant geometry and source bindings pass the same canonical bounds as engaged planners.

Acceptance covers one unchanged pattern switching roll → hide → roll, mixed shell/lining inventory,
same-ID changed-snapshot refusal, scoped grain permissions and unchanged complete purchasing. This
delivers automatic manufacturing routing through existing planners; measured cutter/seam recipes,
physical sewing feasibility and robot execution remain separate gates. See
[mixed-stock-cutting.md](docs/mixed-stock-cutting.md).

---

### 25.59 Source-observed quantity pricing follows the complete order

**Delivered software (`ftx-qt1`, supporting `ftx-tm6`/`ftx-pq1`).** Dated supplier observations may retain
an explicit all-unit quantity-price schedule in the quote's original physical sale units. Every break
belongs to the same source, material, currency and selling terms, with any tier-specific SKU retained;
it is not a separate competing supplier or a marginal graduated rate. Apply minimum quantity and selling increment to the entire required order
first, then choose the highest reached break and price that complete order. Do not automatically buy
extra stock to chase a discount. A quoted price still covers the quote's stated physical quantity.

The canonical quote evaluator remains the only pricing authority for area, mass and linear requirements,
combined marker orders, finite stock and material-study objectives. Reports retain the actual selected
threshold and effective unit price. Latest-observation, exact-material, expiry, observed-stock and
continuous-cut gates remain in force. Missing prices, rates outside a declared schedule or unsupported
source semantics cannot become bargains. Explicit CSV source mappings and immutable source replay
preserve observed schedules; adapters without qualified tier support continue to refuse those inputs.

Acceptance uses original-unit conversion, MOQ/increment threshold crossings, aggregate markers,
malformed schedules, stale/unavailable latest offers and source revision review. A dated public
5,000-metre thread-cone observation exercises regular and five-cone pricing with exact material and
tier-specific SKU identity. The browser displays the selected threshold and source price without
recalculating the Rust result. Other live quotes still need actual source retrieval and exact product/lot qualification; observed-offer percentiles do not
become a market survey, checkout or reserved inventory. See
[textile-price-refresh.md](docs/textile-price-refresh.md).

---

### 25.60 Source-qualified endpoint replay and physical-consumer independence

**Delivered checks and workbench integration (`ftx-vc5`, supporting `ftx-vc2`).** Extend the shared
moving-vapor path in §25.57 with production-motion regressions against actual retained cloth/body
samples, independent area/resistance arithmetic, Ret-only material substitution with unchanged
mechanics, source tampering and exact frozen replay. Valid initially qualified boundary fields
follow the same body triangles; stale initial geometry locks remain refusals. Missing Ret or dry
heat properties preserve each independent consumer's result. Retained replay geometry remains
charged in the aggregate motion budget.

The browser exposes each endpoint's source evidence and lossless standalone replay download as well
as the optional complete-history replay. Endpoint rates keep their instantaneous meaning; only the
explicit accepted-sample history supplies the potential integral. Neither asserts actual water
availability, evaporation or latent cooling. The broader moisture/storage and matched empirical
transport gates stay open. See [vapor-coverage.md](docs/vapor-coverage.md).

---

### 25.61 Bounded purchasing across multiple suppliers

The optional `market.split_orders` policy assigns complete, indivisible markers to source-locked
offers through the existing quote evaluator. Each supplier order aggregates its marker lengths
before applying minimum quantities, increments, observed stock, continuous-cut limits and sourced
quantity prices. A material can therefore have a complete purchasing plan even when no single
supplier can fulfill all its markers. Latest-observation, currency and source qualification retain
their existing meaning, and aliases cannot create duplicate stock.

The bounded search reports a complete feasible selected order separately from a proved minimum.
Budget exhaustion preserves a feasible incumbent without claiming optimality; an incomplete order
never supplies a cheap subtotal. Mixed roll/finite-stock jobs and fixed-pattern studies retain this
distinction through explicit selected-cost objectives. Study requests reserve the declared work
across every alternative against one aggregate cap. Native, portable and browser consumers show the
same assignments, quote evidence, costs and search status. This software path does not reserve stock
or establish live supplier fulfillment; qualification remains open under `ftx-tm6`/`ftx-pq1`.
See [mixed-stock-cutting.md](docs/mixed-stock-cutting.md).

---

### 25.62 Source-reported Ret ranges through layered geometry and motion

The canonical vapor evaluator now propagates complete positive source-reported Ret endpoints
through fixed layer areas, declared boundary/interface resistances and vapor-pressure conditions.
Resistance sums retain endpoint order; potential heat and mass transfer invert the resistance
endpoints. Independent sheets, disjoint covered-body regions and accepted motion intervals carry
the same conditional ranges through their actual area and time accounting. Missing endpoints or
unrepresentable interval arithmetic leave nominal results inspectable and report why ranges are
unavailable. They never become zero-width uncertainty estimates.

The existing `reported_bounds` comparison policy accepts these eight vapor objectives alongside
assembly mass. Inclusive constraints require the complete range; strict guaranteed dominance
requires separated ranges, leaving overlaps and touching endpoints unresolved. Admission checks
the current sources, geometry, canonical networks and aggregate endpoints before ranking. Browser
tables and frozen replay retain both nominal values and ranges. These are conditional source
endpoints, not confidence intervals or uncertainty in geometry, boundary conditions, moisture supply
or latent feedback. Those model and empirical gates remain open under `ftx-ms2`/`ftx-vc2`/`ftx-tmc`.
See [material-screening-and-studies.md](docs/material-screening-and-studies.md) and
[vapor-coverage.md](docs/vapor-coverage.md).

---

### 25.63 Finite body water through moving material-bound vapor paths

**Delivered software (`ftx-wf1`, a bounded part of `ftx-vc2`).** The existing moving vapor history
accepts sourced initial liquid and constant supply on each selected body triangle, bound to the exact
initial body geometry. It reuses canonical Ret transport and accepted cloth/body projections; a
material change automatically changes potential demand and resulting depletion. Regional paths sum
before consuming one reservoir per body triangle, so layers and moving overlap cannot duplicate
water. The declared interval-average demand and supply approximation yields finite modeled covered
evaporation, remaining water and unmet demand through either supported quadrature.

Per-step and cumulative balances retain signed conservation residuals and latent energy supplied by
the prescribed body reservoir. Full source-bearing state survives native/portable replay and browser
inspection. Invalid identities, nonfinite arithmetic or exhausted retention budgets refuse the
complete history without erasing independent mechanics, dry heat or instantaneous vapor results.
Independent analytical and real sewn-motion tests check depletion, source substitution, changing
coverage, nonuniform boundaries, carryover and transactional rejection.

This does not couple latent energy into textile temperature or infer textile moisture laws,
physiological sweating, bare-body transfer, water redistribution or whole-wearer heat loss. Producer
water histories remain nominal; conditional final cumulative Ret comparison ranges (§25.69) do not
establish per-interval film bounds or general water-state uncertainty. Source-qualified
textile storage/sorption, conservative temperature feedback and held-out wet/dry garment experiments
remain open under `ftx-vc2`/`ftx-tmc`. See [vapor-coverage.md](docs/vapor-coverage.md).

---

### 25.64 One material decision across named operating cases

**Delivered software (`ftx-ms2`).** The existing scenario-study commands and browser API accept a
bounded `ftx-drape-scenario-matrix/1` request. The common body shape is selected once and the same
frozen material alternatives share one binding/manufacturing evaluation. Every named case starts
fresh production drapes and its own existing heat, vapor and optional motion consumers. Case-specific
comparison policies may override the ordinary study policy without changing physical input meaning.

The combined frontier requires every case's constraints and required consumers. Dominance uses the
existing nominal/source-range relation on all case/objective coordinates: no worse everywhere,
strictly better somewhere. Opposing improvements, missing data and incompatible source protocols
remain explicit. Source comparability is checked within each case; different environments are not
silently averaged, assigned probabilities or treated as one experimental protocol. Case outputs and
exact replay identities remain available independently of the combined decision.

One to eight cases reserve at most 64 case/alternative evaluations before planning or callbacks.
Mechanical allocations, retained motion geometry and heat/vapor work retain aggregate limits across
the matrix. Native and browser consumers share the implementation; browser input changes cancel and
invalidate the whole matrix. See `docs/material-scenario-studies.md` and the complete numerical
example `examples/scenario-matrix.json`. This advances the material decision workflow without closing
the measured textile, moisture coupling, sewn-garment or machine qualification gates.

---

### 25.65 Compare material choices under finite body-water supply

**Delivered software (`ftx-wf2`).** Single-case and named-case studies can require the existing
`motion_wet_film` consumer and use modeled evaporation, remaining liquid, unmet demand, prescribed
supply and body-reservoir latent energy as objectives or inclusive constraints, with nominal values
or conditional final cumulative Ret ranges (§25.69). Admission
requires complete covered motion and the exact authored source, initial liquid and supply. Every
body-triangle step and cumulative receipt must conserve liquid and agree with canonical regional
demand, the declared quadrature and latent conversion. Altered, incomplete or refused histories
cannot enter the frontier; independent manufacturing, mechanics and dry-heat results remain visible.

The production motion adapter and decision admission use one geometry-independent canonical water
reducer. Every retained step and cumulative receipt is reconstructed from current qualified demand
and the prescribed liquid policy, including residuals and retention charges. Finite supply can make
different transport potentials tie in modeled evaporation; missing film evidence never becomes an
assumed dry film. Native and portable model-backed studies retain the same exact replay and decisions.

The two quadratures retain their own declared sampled-time meaning. Conditional `reported_bounds`
comparison fixes the declared water inventory, supply and sampling; it does not propagate general
water-state uncertainty. Source Ret protocols still govern comparability. This advances decisions
about the conditional body-film model without claiming
physiological sweating, textile-temperature feedback or empirical wearer cooling. See
[material-scenario-studies.md](docs/material-scenario-studies.md).

---

### 25.66 Use measured membrane, bending and contact evidence in every client

**Delivered software (`ftx-mw1`, part of `ftx-tm2`).** The primary native CLI and portable/browser
workbench expose the existing canonical linear-swatch, directional-bending and kinetic-friction
calibrators. Body and same-material friction produce ordinary frozen selections. Two-material
friction preserves both sheet identities and produces separate exact interface evidence, with
interface readiness distinct from sheet or whole-garment readiness. Input evidence admission and
permission to consume fitted estimates remain independent.

The workbench displays coefficients, source-qualified domains, FIT/CHECK residuals and missing
capabilities. Pair fitting preserves the inspected selection; staging checks the final assigned
snapshots, preserves unrelated pairs and requires explicit replacement of existing pair evidence.
Original requests and exact selections, bindings and interfaces remain downloadable for replay.
These connected workflows do not supply missing observations or establish empirical garment fit.
See [browser-material-workbench.md](docs/browser-material-workbench.md).

---

### 25.67 Plan manufacturing against the reported lower roll width

**Delivered software (`ftx-ms2`, supporting `ftx-tm7`).** Existing roll options accept
`width_basis: "reported_lower_bound"` to place and nest complete cut pieces inside the admitted
source width's lower endpoint, with the same declared margins and marker-length limit. Missing or
invalid bounds refuse that cutting consumer. A narrower layout can require longer or additional
markers, changing complete supplier orders and the material-study frontier through existing planners.
Mixed jobs preserve their independent finite-stock geometry and exact whole-job replay.

Reports and SVG retain both the used layout width and source range. Nominal source width remains the
basis for gross stock area, mass and invoice conversion at the newly planned length; no area, mass
or price interval is inferred. Default nominal requests retain their previous output. Native cutting,
purchasing and inventory regressions verify changed feasibility, order quantities, source preservation
and a production study decision while independent assembly mass survives missing width bounds.
These are conditional source endpoints with fixed cut geometry, not guaranteed stock tolerances,
confidence intervals, empirical garment accuracy or machine qualification. Those acceptance gates
remain open. See [cutting-plans.md](docs/cutting-plans.md).


---

### 25.68 Compare explicit garment alterations and material choices together

**Implemented and software acceptance verified (`ftx-ds1`, 2026-10-24).** The opt-in
`ftx-design-study/1` request crosses independently authored canonical alteration sequences with
explicit, screened or grouped material alternatives. It always retains the original-geometry
baseline. Width/ease, length, vertex, seam-truing and cap-ease changes use existing operators; each
sequence starts from the unchanged original and retains its exact operations and altered document.

Every candidate passes canonical structure, seam walk, free-edge truing, cut/grain completeness and
sampled outline simplicity before binding or downstream consumers. Invalid designs remain visible
with their actual validation/refusal and cannot win a manufacturing or scenario comparison. Each
admitted altered-and-bound pattern reruns the real cutting, finite-stock/mixed-job, purchasing, mass,
sewing/recipe and requested simulation consumers. Single and named-case studies share the same
production path; no second browser solver or implicit seam repair is introduced.

The complete design/material product is bounded to 32 rows including baseline. Operation counts,
root-search document passes, repeated source-bearing selections, outline checks and existing
manufacturing/scenario reservations are charged before or across their actual work. Reports retain
collision-free pair identities, geometry-change truth, exact altered JSON, final bound JSON and
frozen material changes. The workbench displays designs, failed gates and exact replay downloads.
The existing fixed-pattern study schema retains its previous behavior.

This is a finite authored-design search, not continuous optimization, inferred wearer fit, automatic
safe ease or empirical garment performance. `ftx-ds2` remains open for bounded synthesis of fit-driven
alterations from accepted body evidence and explicit design variables, with real sewn feasibility
constraints and held-out checks. See [material-screening-and-studies.md](docs/material-screening-and-studies.md)
and the synthetic runnable request [design-study.json](examples/design-study.json).

---

### 25.69 Compare final finite-water outcomes across source Ret endpoints

**Delivered software (`ftx-ms2`, supporting `ftx-wf2`).** All five finite-water decision quantities
support `basis: "reported_bounds"` through the existing single-case and named-matrix comparators.
After complete nominal history admission, the comparator replays canonical low- and high-demand Ret
endpoints through the same per-triangle finite-water authority. Final cumulative evaporation, unmet
demand and body-reservoir latent energy use low-to-high demand totals; remaining liquid reverses
that endpoint order. Liquid depletion remains part of the calculation, rather than treating
potential vapor demand as available water.

Declared supplied water is a fixed singleton range and needs no reported Ret bounds, while retaining
complete nominal source and history admission. Missing Ret endpoints exclude the other bounded water
metrics without erasing their nominal readings or independent consumers. Inclusive constraints must
contain the complete final range; overlapping or touching ranges remain unresolved under the ordinary
strict range-dominance rules. The browser displays native readings and decisions without reranking.

These final comparison ranges hold geometry, boundaries, coverage, latent heat, initial liquid,
supply and temporal integration fixed. Producer cumulative and accepted-sample water ledgers, and
their exact frozen replays, remain nominal; no per-interval water bounds are introduced. The ranges
are conditional source-endpoint consequences, not confidence intervals or general water-supply or
film uncertainty. They do not close measured moisture coupling, empirical wearer cooling, garment
fit or process qualification gates. See [material-scenario-studies.md](docs/material-scenario-studies.md).

---

### 25.70 Generate fit-constrained garment alterations on the selected body

**Implemented software (`ftx-ds2`), with focused runtime verification in §25.71.** The existing design study now
generates bounded linked width/ease and length alterations from explicit finite variable domains,
crosses them with material choices, and evaluates the actual altered garment through the same
manufacturing and optional scenario consumers. Original geometry remains a visible baseline.
Each generated row retains its scalar coordinates, canonical operations, validation, exact altered
document and final material binding.

Explicit points-of-measure/ease targets are evaluated against one immutable selected model shape.
The body adapter uses canonical tape sections and actual reference-pose frame landmarks, retains
MODEL_FIT provenance and exact model/shape/source/geometry/value identities, and never substitutes
conflicting supplied tapes or analytic dress-form landmarks. Missing sections remain unavailable.
Canonical flat validation and sampled outline checks gate binding and all downstream callbacks;
out-of-band raw values cannot pass merely because display rounding produces a zero residual.
Single and named-case native/portable studies share the same body and manufacturing admission.

A declared allowance can visit only a deterministic prefix of the finite grid. Reports retain
total/generated/omitted counts and explicit search completeness; no unvisited point is certified
infeasible and no conditional frontier is called a continuous or global optimum. Expanded rows,
measurement sampling, source documents and existing manufacturing/scenario budgets are reserved.
The workbench exposes variable choices, full target residuals, source identity and incomplete search.
See [material-screening-and-studies.md](docs/material-screening-and-studies.md).

Focused numerical, synthetic selected-body and native/portable regressions are included; the
executable checks in §25.71 now cover the finite-grid path. Adaptive/continuous inverse design, qualified local strain/pressure
constraints and held-out measured garment acceptance remain substantive next gates. A numerical
ease band is neither a physical garment-fit claim nor a safe-fit or comfort threshold.

---

### 25.71 Screen fit-qualified designs before spending downstream evaluation slots

**Delivered bounded software (`ftx-ds2`).** Optional `design_grid.fit_screening` inspects an
explicit prefix of up to 256 authored grid points using the existing canonical alteration,
flat-validation, outline and selected-body fit authorities. It retains the original baseline and
sends the first `max_designs` qualifying candidates through the ordinary material, manufacturing
and scenario consumers. A useful candidate beyond the old short prefix can therefore reach real
consumers without consuming a downstream slot for every failed flat design. Omitting this option
preserves the original prefix behavior.

Every inspected point retains exact geometry, operations, body/fit evidence and any refusal.
Reports distinguish rejected, selected, qualified-but-omitted and wholly unvisited points; a
complete flat screening pass is not a complete downstream comparison when passing points were
omitted. Canonical flat results are reused across materials and named cases. Source/document,
root-search, retained identity, measurement and outline work are bounded before screening.
See [material-screening-and-studies.md](docs/material-screening-and-studies.md).

All 18 export design regressions pass, including actual manufacturing replay and incomplete-search
behavior. The native selected-body regression reaches the seventh grid point (+30 mm) with one
downstream design slot after rejecting the first six; both `ftx` and `ftx-materials` exactly match
portable single-case and named-matrix reports. These checks establish software behavior on
synthetic evidence. `ftx-ds2` remains in progress for further verification, adaptive/continuous
design, qualified local strain/pressure constraints and independent measured garment validation.
No safe-fit threshold, empirical wearer fit or global optimum is inferred.

---

### 25.72 Choose qualified sewing recipes by complete thread order cost

`ftx-sewing-selection/3` adds an explicit `minimize_thread_observed_order_cost` objective to the
existing bounded production recipe search (`ftx-tm7` / `ftx-ms2`). Every candidate still runs the
same sewing planner; its exact yarn demands aggregate before canonical whole-order purchasing
applies supplier minimums, increments, price tiers and the declared dated market. A slower recipe
can therefore win on its complete order cost, while quantity thresholds can also make a larger
order cheaper. No price or thread-use model is inferred.

Missing consumption or eligible offers leave a built trial unscored and unresolved, prevent a
false optimum or infeasibility proof, and never become a zero cost. A bounded search may retain a
complete incumbent without proving it cheapest. The selected plan and exact replay flow through
material/design studies and native/browser clients. The objective excludes cutting costs, setup
time, undeclared supplier bundles and empirical seam quality. See [sewing-selection.md](docs/sewing-selection.md).

---

### 25.73 Discover fit-qualified alterations between authored grid values

Optional `design_adaptive` extends the selected-body design study (`ftx-ds2`) with deterministic
coordinate polling inside explicit variable bounds. Initial/minimum steps and a finite evaluation
allowance govern discovery; linked slash sites and subsequent operations still use the canonical
alteration authority. The search shares unrounded flat-fit readings with ordinary validation and
minimizes maximum band violation, then total violation. Actual altered geometry governs admission,
including the existing operator's 0.001 mm coordinate rounding.

Every trial retains exact commands, geometry, selected-body identity, fit readings and refusals.
Qualified designs plus the unchanged baseline flow through the existing material/manufacturing
and scenario consumers, with one search reused across materials and named cases. Budget and
minimum-step stops preserve discovered candidates but never prove the unvisited continuous domain
infeasible or optimal. Local pressure/strain qualification and empirical wearer fit remain separate
open gates. See [material-screening-and-studies.md](docs/material-screening-and-studies.md).

---

### 25.74 Cool moving textiles with explicitly supplied initial free liquid

Optional `motion.transient_surface.surface_water` delivers a bounded conservative software slice
of `ftx-vc2`: initial textile-owned liquid follows reference material triangles while current area
conducts vapor through the canonical Ret authority. Exact material locks and independent source
evidence qualify constant dry thermal/Ret properties, liquid capacity, latent heat and an authored
pressure/temperature table over explicit temperature and loading domains. Dry instances retain
explicit zero loading; missing qualification refuses only this heat consumer.

Pressure and exported vapor sensible enthalpy sample the initial inner-cell temperature of each
thermal substep. The existing implicit heat/exterior solver uses remaining-liquid sensible capacity
and signed latent energy, together with separately requested qualified mechanical heat. Exact
dryout splits its containing substep; anchored regular endpoints prevent phantom rounding steps.
Dry-textile storage, liquid storage, vapor sensible export and latent energy balance the signed
boundary and mechanical input. Temperature, water, source counters and trace commit together only
after the complete accepted interval succeeds. Reports retain per-triangle water, actual cooling,
source networks and signed mass/energy residuals; native and portable scenarios share this path.

Replenishment must be zero. Condensation and raw temperature-domain departure refuse. Sorption,
wicking, absorbed moisture, wet-dependent properties, added mechanical water mass, body-water
transfer and empirical garment validation remain open. Time/space refinement is still required,
and existing dry-heat optimization admission excludes this different energy-ownership model until
it has a matching canonical decision consumer. Analytic conservation/refinement/rollback and real
moving-garment/portable-study regressions cover this slice. See
[textile-surface-water.md](docs/textile-surface-water.md).

---

### 25.75 Compare coupled textile-water histories through actual scenario decisions

The existing transient-surface decision consumer (`ftx-ms2` / `ftx-vc2`) admits qualified
initial-liquid histories after matching source materials, reference triangles, canonical Ret
networks, accepted motion and complete water/enthalpy accounting. Final and sampled temperature,
boundary energy and dry sensible storage keep their existing meanings. Nominal textile-water
evaporated mass, remaining liquid and latent energy are explicit additional objectives and
inclusive constraints, separate from prescribed-body reservoir quantities.

Missing qualification and inconsistent or incomplete histories exclude the affected alternative
without erasing manufacturing or independently available simulation outputs. The same admitted
readings reach native, portable and browser single/named-scenario comparisons. No water-state
uncertainty, empirical cooling, physiological comfort or global optimum is inferred. Explicit
material locks remain in force; source policy for one material cannot authorize a replacement.

---

### 25.76 Supply textile liquid with independent mass and sensible heat

The initial-film cooling model now has an explicit version 2 continuation for supplied free liquid.
Each retained instance declares independently sourced rates and incoming temperatures per mechanical
interval. Delivery uses reference material area; evaporation still uses the current vapor path.
Incoming sensible enthalpy joins the same conservative balance as dry textile storage, liquid storage,
latent cooling, vapor export, boundary exchange and qualified material work. Dry patches can consume
their incoming supply or rewet, while qualified maximum loading, temperature domains and condensation
rules refuse unsupported intervals transactionally.

Actual scenario comparisons can constrain supplied mass alongside temperature, evaporation and final
inventory after reconstructing the exact schedule and its energy. Native and portable consumers retain
the independent supply evidence and per-patch accounting. This remains a thermal free-film calculation:
it adds no mechanical water mass, sorption, wicking, body-water transfer, wet material law or wearer-fit
claim. Source-reported water or temperature uncertainty is not propagated.

### 25.77 Constrain actual material-direction strain over declared garment regions

Scenario studies can request raw material-frame Green strain on explicitly selected retained instances
or the full retained garment. Production membrane gradients provide warp, weft and engineering shear;
signed extrema retain exact source triangles, frozen materials and deterministic sampled witnesses.
Initial and complete accepted-motion reports share an explicit aggregate work budget and preserve
independent mechanical/manufacturing results when the optional consumer is unavailable.

Static and motion comparison metrics support user-declared inclusive requirements on each direction.
Decision admission reconciles source ownership, scope, canonical material domains, geometry identities,
all accepted samples and consistent extrema. The browser exposes both the spatial scope and responsible
samples. These are nominal design constraints: characterized strain domains are not failure limits,
and sampled compliance establishes neither continuous-time safety nor comfort or physical garment fit.

### 25.78 Extend actual insulation choices through the shared material authority

The built-in catalog adds ten source-qualified manufacturer grades: six Type G polyester insulation
sheets and four pre-compressed Type B footwear sheets. Their original mass, thickness, thermal
resistance and nominal width observations retain document revision, table page, source digest and
test method. ASTM D5736 thickness pressure is not assigned to the separate ASTM F1868 thermal result;
rounded clo columns do not replace the published SI observations. Type B composition stays unknown
where the source omits it. See [insulation-data.md](docs/insulation-data.md).

Existing resolution, freezing, mass/width screening and steady thermal consumers use these records
without a second database or solver edits. A material-only G100/B100 substitution changes conditional
steady heat flow through the existing series model. This is useful commercial coverage, not a measured
garment, a temperature rating, a wet-state law or complete transient/mechanical characterization.
Matched lot-specific measurements and the unchanged-pattern linen/leather/performance-knit acceptance
in `ftx-tm2` and `ftx-tm8` remain the principal physical-data gaps. The source-import slice is `ftx-ti1`.

### 25.79 One production quantity for cutting and sewing-thread orders

The sewing consumables request accepts an explicit `production_copies` count. Material and design
studies inherit the actual roll, finite-stock or mixed cutting-job count when it is omitted; all
requested routes must agree and explicit conflicts refuse sewing. Qualified operation/channel
traversals and start/end allowances repeat per garment, then exact yarn requirements aggregate before
the existing price engine evaluates minimums, increments, tiers and observed inventory. Continuity
remains one complete operation/channel rather than one unbroken thread across all garments.

Both fixed sewing and qualified recipe search use this quantity. Version-2 consumables reports retain
per-copy contributions and complete production totals; selected options replay independently. Omitted
standalone counts preserve the original one-garment output. Sewing geometry, feed/setup durations and
assembly mass keep their per-garment scope. Cross-garment scheduling and combined fabric/thread supplier
orders remain separate capabilities. Track this functional integration in `ftx-pb1`; see
[material-sewing-plans.md](docs/material-sewing-plans.md#production-quantities-share-cutting-and-thread-purchasing).

### 25.80 Compare source-bound body contact loads through actual scenarios

Optional `contact_loads` evaluates the production contact model on the selected union of welded
garment vertices, retaining material thickness, surface fields and seam envelopes. Initial and
complete accepted-motion reports bind the exact retained material assignments, cloth/body geometry,
active contact policy and selected source instances. Moving bodies keep those same material fields.
Explicit vertex and candidate-pair allowances bound the additional evaluation; optional refusal
preserves independently available motion, thermal and manufacturing outputs.

Initial reaction receipts bind the completed material report, including retained per-piece instance
and triangle ownership. Heterogeneous-garment regressions compare the complete receipt against the
ordinary production contact calculation; native and portable comparisons exercise this per-piece path.

Four nominal comparison metrics cover the initial resultant, initial maximum nodal force and their
peaks across accepted motion. Admission reconciles complete sample histories, source ownership,
independent initial collider identity, force-vector reductions, extrema witnesses and work totals.
Browser tables expose region, vector components and sample/time/vertex witnesses; source replacement
invalidates stale outputs. Force cancellation and mesh dependence remain explicit. These body barrier
loads exclude tangential friction and are not contact pressure, empirical comfort, safe fit or
continuous-time bounds. Track the slice in `ftx-cl1`; see
[material-scenario-studies.md](docs/material-scenario-studies.md#body-contact-loads-in-material-studies).

### 25.81 Connect sourced sewing yarn choices to the shared production inventory

Four exact Coats Epic articles add published tex, put-up length and source-defined average strength
to the existing yarn catalog, with original units, table page, edition, retrieval date and document
hash. The resolver preserves both authored observations and normalized linear density. Exact frozen
yarns can feed existing qualified sewing-consumption channels and separately observed length offers,
including shared cutting/sewing production quantities. Package lengths do not invent purchasing
increments, source averages do not establish seam-strength limits, and reported elongation ranges
do not supply a constitutive law. No consumption coefficient, stock or price is fabricated.
The catalog now contains 265 records; `ftx-yb1` tracks this source-qualified slice. See
[supplier-apparel-catalog.md](docs/supplier-apparel-catalog.md#coats-epic-sewing-thread-observations-august-2026).

### 25.82 Remaining functional priorities after source, load and production integration

The principal physical acceptance gap remains `ftx-tm2` / `ftx-tm8`: exact specimen/lot and
construction-dependent characterization, followed by held-out unchanged-pattern/body tests spanning
linen, leather and performance knit. Published composition, mass, marketing stretch or thread counts
cannot replace directional loading/recovery, bending, friction, compression and transport data.
Preserve missing capabilities rather than making the demonstration pass through guessed laws.

Add source-qualified treatment and laundering transitions for an exact fabric variant, process,
cycle count and conditioning (`ftx-care1`). Connect admitted directional shrinkage to cloth rest
geometry and optional pattern compensation, then recompute cutting yield and seam compatibility.
Change mass, mechanical and thermal properties only where supported post-treatment evidence exists;
retain unknown effects as unknown. Freeze before/after states and provenance for replay. Existing
`shrinkage_warp` and `shrinkage_weft` observations currently have no such consumer and must not imply
automatic treatment propagation.

Complete actual multi-garment production scheduling (`ftx-pb2`) on top of shared quantities.
Expand per-copy operation dependencies and qualified machine/fixture setup transitions, retain finite
material and tool constraints, and expose a bounded incumbent when scheduling is incomplete. Batch
thread demand is already aggregated; per-garment feed/setup durations must not masquerade as a
validated cross-garment schedule. Keep hardware execution behind explicit machine/process qualification.

### 25.83 Source-qualified treatment reaches ordinary pattern geometry

`material_treatment` implements the first functional slice of `ftx-care1`: an exact specimen,
process, cycle count and before/after conditioning bind two embedded, content-locked ordinary
material selections to directional dimensional-change observations. Both material states must
qualify the same specimen and their respective conditioning, in addition to valid content locks.
Post-treatment properties and
response laws require their own qualified evidence; no original coefficient or mass conservation
is inferred. Negative dimensional change can express expansion. Five-cycle observations are one
complete transition, not an invented per-cycle law.

The shared geometry operation transforms seam lines, absolute curve controls, physical notch
locations and material marks in each source piece's grain frame. Treat mode selects the AFTER
record; compensation uses the inverse dimensions while retaining the BEFORE material for cutting.
The output is an ordinary frozen pattern, so the existing fit, drape, thermal, cutting and sewing
authorities consume its changed geometry and material without special-case physics. Fresh seam
checks expose differential shrinkage. Mirrored pieces and principal-axis folds are supported;
ambiguous oblique folds and noncommuting simulation scales refuse. Seam-allowance/tool widths
remain authored manufacturing choices. Constrained washing of an assembled garment is still an
independent physical problem, not established by free coupon dimensional changes.

The built-in catalog adds ten measured before/after denim states from Kanat and Ozdil (2026),
DOI `10.3390/ma19153264`, taking the corpus to 275 records. Five fabric-code groups preserve their
reported construction, five-cycle shrinkage, areal mass and thermal resistance, original units,
source conditions and separate reported deviations. Unqualified gauge thickness remains a reported
attribute, not contact or sewing clearance. The exact source adapter produces ordinary frozen
treatment requests; no new materials database or law is introduced. See
[laundered-denim-materials.md](docs/laundered-denim-materials.md). This sourced tranche and numerical
round-trip checks do not close `ftx-tm2`, `ftx-tm8` or measured sewn-garment treatment acceptance.

### 25.84 Production sewing is a batch optimization and replay problem

The first functional slice of `ftx-pb2` expands source operations into physical copy/operation pairs,
scoping every dependency to its garment. One shared-machine schedule can retain a compatible
needle/thread setup across copies. The existing bounded solver optimizes identity changes or
explicitly qualified directed setup durations, charging the declared initial setup only once.
It retains complete baseline and selected schedules, lower bounds and honest incumbent/budget
status. The expanded graph is bounded at 63 operations; complete stitch output remains bounded
before allocation. No source station geometry is duplicated merely to schedule another copy.

Cutting, thread purchasing and sewing scheduling share a reconciled production count. The count
also propagates when no thread inventory is requested. Material-study metrics, qualified recipe
selection and comparisons use actual batch feed and setup totals; a three-copy regression changes
the selected recipe because shared setup changes its objective. Conflicting quantities refuse the
affected sewing consumer while independent cutting remains available. Thread-only requests retain
their existing per-garment timing meaning. Actual stitch programs expand the chosen copy-scoped
schedule, preserving partial seams, backtacks, reverse travel and qualification identities.

Both native commands and the materials browser consume these authorities. The browser distinguishes
setup-duration optimality from setup-count optimality and shows physical copy IDs. The same native
and portable interfaces expose treatment/compensation, exact frozen replay and ordinary downstream
mass/thermal consumption; the browser can adopt a derived pattern and withdraw stale results when
its source changes. See [material-treatments.md](docs/material-treatments.md) and
[sewing-production.md](docs/sewing-production.md) for runnable examples and limits.

The remaining high-impact work is empirical and mechanical: matched real linen/leather/performance
material laws and held-out sewn treatment behavior; then qualified multi-machine, fixture,
workholding, seam-access and handling constraints. Synthetic timings and complete software schedules
do not establish physical execution readiness. `ftx-care1`, `ftx-pb2`, `ftx-tm2`, `ftx-tm8` and
`ftx-sp2` retain those unsatisfied physical requirements; no hardware authorization is created.

### 25.85 Released fabric observations expand real material discovery

The Fabric-101 source adapter adds 96 individually identified released specimens, taking the
canonical catalog from 275 to 371 records. Its 192 measured observations preserve original areal
mass and reported thickness, with fiber composition and construction only where the exact source
identities agree. Coverage includes linen/ramie, cotton, silk, wool, polyester and nylon, blends,
jerseys, rib, fleece and coated sheets. Exact release revision, CSV hashes, specimen IDs, instrument
header paths/blob hashes, conditioning and source discrepancies accompany each record. Catalog
search, ordinary bindings, assembly mass and material screening consume these records directly.

The authors' 103-row catalog has 98 mapped released specimens; two disagree with their instrument
mass metadata and are excluded, leaving 96 concordant entries. Stale article labels and values are
retained as explicit discrepancies rather than silently joined across specimen IDs. Unqualified
gauge thickness remains `reported_thickness`, separate from contact/sewing thickness. No curve fit,
stiffness, friction, thermal property or uncertainty interval is manufactured from these metadata.
The cylinder force/torque experiments need a separately justified forward model, raw timing/tare
and geometry qualification, and independent CHECK evidence before they can supply solver laws
(`ftx-tm2.1`).
See [fabric101-materials.md](docs/fabric101-materials.md) for exact sources, exclusions and usage.
This useful data expansion leaves the principal measured substitution gates `ftx-tm2` and `ftx-tm8`
open; source-backed mass is not complete garment simulation readiness.

### 25.86 Fit and manufacturing are evaluated after treatment

The ordinary design operation `treatment` now runs the existing source-qualified geometry
authority inside authored designs and grid/adaptive `after` sequences. Final seam, outline and
selected-body fit gates evaluate the transformed pattern. This closes the software gap where a
pre-wash pattern could pass fit while its post-wash dimensions were never tested. Search variables
can describe pre-treatment alterations, with admission against explicit post-treatment targets.

Each completed stage retains the exact canonical request, before pattern and resulting pattern.
The final design still replays through `altered_pattern_json` and `frozen_changes`. After design
operations, material variants cannot replace a treated piece's last promised material identity;
unrelated lining/fill substitutions remain possible. Refused final seams, fit or bindings withhold
downstream consumers while preserving completed stages for inspection. A material-only transition
does not claim changed geometry. Work reservations include all attempted grid/adaptive operations
and replicated per-piece material/history records, not only selected candidates.

Both native commands and the existing portable study interfaces use this same authority. Browser
studies expose exact intermediate before/after/request downloads for alternatives and screened or
adaptive trials, retaining final admission/refusal status and withdrawing obsolete links on edits.
The laundered-denim example can emit a complete comparison of untreated, treated, compensated and
compensated-then-treated panels. This implements the software scope of `ftx-care1.3`; measured sewn
restraint, process-dependent contact and held-out garment behavior remain under `ftx-care1`.

### 25.87 Production batching respects finite reusable fixtures

An optional source-qualified workholding policy now declares finite resource capacities and
per-copy acquisition/release operations inside the shared production optimizer. Full machine and
admitted qualification hashes bind applicability to the exact configuration, material stack,
process and source evidence. Distinct acquisition/release anchors add actual dependencies. Units
are acquired before an operation starts and released only after it completes; single-operation
holds still occupy resources during execution. The search retains complete feasible incumbents
and applies the existing count or directed-time objective under these extra constraints.

This changes real batch order. In the synthetic three-copy join/reinforce example, one, two and
three fixtures yield 6.5, 4.0 and 1.5 seconds of declared setup respectively, with the same 240 stitch
intervals and 24 seconds of feed. These are software examples, not measured factory throughput.
The stitch program follows the selected constrained order, and native/portable reports retain all
resource capacities, expanded holds and complete before/during/after occupancy timelines.

The existing serial-copy baseline is checked before optimization or lower-bound shortcuts. If that
particular seed is unsupported, `NotConverged` keeps recipe selection unresolved; it does not prove
that no other feasible schedule exists. Invalid added cycles remain definitive request refusals.
The resource graph, integer demands, qualification references and expanded holds are bounded.
Omitted policy preserves prior output. See [sewing-production.md](docs/sewing-production.md) and
the complete `examples/sewing-workholding-options.json`.

Direct sewing and manufacturing/scenario browser views show capacities, baseline/selected peaks
and actual before/during/after occupancy. Source evidence and both complete timelines remain
inspectable; the page does not rerun the optimizer or grant physical qualification.

This implements finite reusable inventory under `ftx-pb2.2.1`. Multi-machine assignment/transfers,
fixture geometry and grip mechanics, seam access, measured handling and real execution qualification
remain open. Counting available jigs does not establish those physical capabilities.

### 25.88 Sewing checks the complete tool sweep against declared fixtures

The shared sewing planner now admits optional source-qualified static planar access policies for
explicit registered/folded assemblies. A circular tool envelope sweeps every complete canonical
needle chord against filled simple fixture polygons. Checking complete segments catches collisions
between otherwise-clear stitch stations; reverse feed and backtacks revisit the same checked sweep.
Tool contact refuses, and requested positive separation is inclusive. Full machine and admitted
material/process qualification hashes bind the policy's applicability. Missing geometry evidence or
an unqualified operation cannot become an access pass.

Ordinary stitch programs, production, recipe search and material studies use this same admission.
Reports retain minimum clearance, exact limiting points, source policy and frame; browser views show
them without recalculating geometry. Omission preserves earlier behavior. A complete synthetic example
combines three copies, finite fixture inventory and per-operation clearance. See
[sewing-access.md](docs/sewing-access.md) and `examples/sewing-access-options.json`.

This advances `ftx-pb2.2` and `ftx-sp2` with a bounded geometric constraint. Grip force, cloth support,
three-dimensional approach/retraction, calibrated machine registration, handling and multi-machine
assignment remain separate unsatisfied requirements. Static planar clearance and a source-qualified
schedule do not authorize hardware or establish measured seam quality.

### 25.89 Production timing includes qualified garment handling

The shared batch scheduler accepts an optional `production.handling` policy for explicit
initial loading, directed operation transitions and final unloading. Same-copy and different-copy
transitions have separate source-qualified durations; keeping a needle/thread setup never implies
zero garment handling. Full machine and admitted process/material qualification hashes bind the
timing domain. Missing observations remain unresolved in recipe selection, while malformed or
unknown operation identities refuse.

The `minimize_production_time` objective adds serial, nonoverlapping handling to qualified setup
durations inside the existing bounded scheduler. Final unloading participates in search and its
lower bound. Generated feed time, dependencies, finite fixture occupancy, planar access and exact
stitch replay retain their existing authorities. Reports preserve baseline and selected feed,
setup and handling separately; a combined optimum does not imply a setup-only optimum.
`minimize_modeled_production_seconds` lets recipe choice follow this complete declared objective.

Both native commands, portable adapters and the browser expose this behavior. Studies compare
`sewing_handling_seconds` and `sewing_modeled_production_seconds`; missing evidence cannot become
a winning zero or a proof that all recipes are infeasible. The complete synthetic
`examples/sewing-handling-options.json` demonstrates a transfer penalty changing batch order.
See [sewing-production.md](docs/sewing-production.md). This delivers the software slice
`ftx-pb2.2.3`; concurrent machines, measured phase overlap, physical handling, grip and machine
execution remain separate qualification requirements.

### 25.90 Exact yarn density connects consumption to mass and purchasing

The existing yarn material authority now supplies admitted positive linear density in kg/m to
thread consumption and the canonical quote engine. Complete consumed length can therefore produce
a source-bound mass receipt and satisfy a kilogram-priced offer. Supplier minimums, increments,
quantity tiers and finite stock are still applied in original kilograms before conversion to
acquired metres and comparable rates. Missing or unapproved density excludes only the mass route;
ordinary length requirements and length offers remain available.

Per-yarn and complete consumed-mass totals retain their exact frozen material identities, including
the observations actually selected through overrides. The nominal `thread_required_mass_kg`
study objective uses this complete inventory and checks its source protocols. Kilogram inventory
does not establish uninterrupted length, so strict continuity still needs independent qualifying
length evidence. Neither nominal cone size nor tex invents stock availability.

Consumed thread includes declared operation allowances and is not automatically retained garment
mass, an attachment or a new mechanical load. The built-in Coats yarn observations exercise this
bridge without adding fabricated material values or prices. Synthetic offers verify original-unit
rounding, finite stock, exact source locks and both native/portable workflows.
See [material-sewing-plans.md](docs/material-sewing-plans.md). This completes the bounded yarn
integration `ftx-yb2`, while live supplier coverage and retained-trim measurements remain open.

### 25.91 Source-qualified condensation and re-evaporation share the thermal state

Version 3 of the existing textile free-film request extends independently sourced liquid delivery
with explicitly qualified reverse Ret transport, deposition on dry or wet textile and incoming
vapor temperature for every motion interval. The same frozen material network determines transfer
over current cloth area, while reference material area owns liquid and its qualified loading cap.
Deposited water remains available for later evaporation when the boundary reverses.

The existing implicit thermal solver receives condensation latent release and incoming sensible
mixing. Its declared effective enthalpy convention uses the existing liquid heat capacity and
constant latent heat; this is an explicit qualified model, not an inferred vapor heat capacity.
Evaporative removal, condensation release, supplied-liquid enthalpy and incoming-vapor enthalpy stay
separate. Initial plus supplied plus condensed water equals remaining plus evaporated water.
A late temperature, loading or numerical failure rolls back the complete accepted interval.

Native, portable, single-case and named-case studies admit the complete source/geometry/mass/energy
history before comparing `textile_water_condensed_mass_kg` or
`textile_water_condensation_latent_energy_j`. Browser views show the separate ledgers and withdraw
refused histories; compiled-worker acceptance replays native-generated complete reports. Analytic,
time-refinement, enthalpy-reference, source-substitution and actual sewn-motion reversal tests
establish the bounded software behavior.

This implements `ftx-vc2.1`, not the full empirical moisture program. Sorption, capillary transfer,
drainage, mechanical water mass, general garment dew-location prediction and measured wet-property
qualification remain open. See [textile-surface-water.md](docs/textile-surface-water.md).

### 25.92 A qualified textile can retain permanent set and delayed recovery together

An explicit `parallel_green_maxwell_plastic/1` declaration now qualifies an elastoplastic equilibrium
membrane in parallel with the existing Maxwell branches. Every branch uses the same physical
material-frame Green strain. Plastic flow responds to equilibrium-branch stress; Maxwell arms retain
their own viscous strains. Their stored energies and stresses add, while plastic and viscous losses
remain separate. This particular network does not describe plastic flow inside a Maxwell arm.

Joint admission requires matching specimen and conditioning evidence, compatible equilibrium
stiffness interpretation, unit compression factors and the intersection of the original characterized
domains. Private paired identities bind both compiled laws to the complete resolved material. An
atomic solver binder admits the complete network; either joint half alone refuses. Ordinary records
without the declaration keep the previous exclusivity and serialization.

The canonical sewn-scene and timed-strip consumers use this binder. Each material triangle keeps its
own two histories after seam welding, and failed steps commit neither trial history. Reports retain
the network interpretation and source identity. The synthetic shared selection and direct known-answer
regressions exercise loading, holds, unloading, derivatives, ledgers, initialization and rollback.
See [material-parallel-memory.md](docs/material-parallel-memory.md).

Both native strip-history commands and the portable/browser boundary retain the complete source-locked
network in frozen replay. The workbench explains the branch interpretation and keeps the two loss
ledgers separate; input changes withdraw the prior result.

This advances the constitutive capability under `ftx-tm4`. It supplies no guessed fabric parameters,
joint inverse calibration or measured linen/leather/performance-knit acceptance. Matched complete
loading/recovery observations and independent garment validation remain under `ftx-tm2` and
`ftx-tm8`.

### 25.93 Resolve cloth equilibrium when energy differences reach floating-point resolution

The shared implicit cloth solver now supports a tightly bounded full-step approximate-Wolfe
acceptance when absolute-energy Armijo loses the tiny predicted decrease to rounding. The trial
must still pass existing contact and characterized-material domains, satisfy endpoint directional
derivative bounds and strictly reduce the free-gradient residual. Only roundoff-scale predicted
decreases and energy discrepancies qualify. This is a line-search decision; the original requested
Newton-correction convergence test remains separate and unchanged. Reports retain the number of
such accepted steps, including friction-lag rounds.

This removes a reproduced blocker in ordinary nominal-density textile load/hold/unload histories.
The synthetic joint permanent-set/Maxwell specimen now completes 4% and 8% cycles, an 8% half-time-step
case and an 8% refined-mesh case under the original correction and independent force tolerances.
Both history ledgers and exact frozen replay remain intact. Contact, failed-step rollback and
material-domain checks retain their original authority. The algorithm uses the approximate-Wolfe
conditions of Hager and Zhang (2005, section 4), with additional conservative eligibility and
gradient-progress checks; this is no unconditional convergence or physical textile-validation claim.
See [material-parallel-memory.md](docs/material-parallel-memory.md). The bounded numerical repair
is `ftx-tm4.3`; measured characterization and garment acceptance remain open.

### 25.94 Allocate complete garments across independently qualified sewing cells

Optional `production.parallel` adds a bounded whole-garment assignment over explicitly identified
physical cells sharing one fully admitted machine configuration. Each cell retains its own setup,
availability, handling and optional finite fixtures. Source qualification must establish independent
machines, fixture units and handling labor; a shared operator is not silently multiplied. Garments
stay in their assigned cell, and each cell reuses the existing serial production authority.

An integer allocation solve minimizes the maximum used-cell completion, with a second solve over
valid inner lower bounds. Incomplete inner searches retain complete feasible schedules and honest
proof status. The explicit millisecond convention ceilings total feed once per used cell, preserving
raw feed and padding. Availability, initial setup/loading and final unloading are charged to every
used cell. Aggregate machine work remains distinct from elapsed completion. One shared parallel
work cap covers all nested cell/count schedules, returned-schedule reconstruction and allocation
work; recipe and study cross products reserve the full amount before evaluation. The count cache
retains fixed-size timing/search receipts and streamed full-schedule digests. Only the returned
baseline and selected schedules retain full operation, handling and fixture histories; their exact
reconstruction is checked and charged to the same cap. Parallel plans also admit the ordinary
250000 expanded stitch-interval bound before count searches. These bounded retention and output
repairs are tracked in `ftx-pb2.2.5`.

Every output stitch identifies its cell and global garment copy; nested fixture and handling histories
retain local IDs with exact global mappings. The old outer serial report remains a reference and
cannot supply the selected parallel time. Both native commands, portable/browser consumers, recipe
selection and material/design/scenario comparisons use this same result through the dedicated
`sewing_parallel_makespan_seconds` objective. The complete synthetic two-cell example reduces
three-garment completion from 41.1 to 25.5 declared seconds while preserving all 240 stitch intervals.
See [sewing-production.md](docs/sewing-production.md) and `examples/sewing-parallel-options.json`.
This implements `ftx-pb2.2.4`; heterogeneous machines, shared-resource overlap, inter-cell transfers
and measured physical execution remain open under the parent production qualification work.

### 25.95 Identify Maxwell recovery alongside a known permanent-set branch

Optional `plastic_equilibrium` extends the existing `ftx-relaxation-calibration/1` acquisition path
with an independently characterized elastoplastic equilibrium branch. Elastic, yield, hardening and
domain observations remain fixed. Every complete acquisition starts from an explicitly pristine
zero-Green-strain reference, and every recorded node advances the canonical return map, including
nodes with no observed force. The existing proportional, fixed-time directional, bounded directional-time
and model-comparison engines identify only the Maxwell response remaining after that known branch.

The network interpretation requires its own source, matching specimen and conditioning, and separate
estimate consent. Original strain and accumulated-history limits remain active. Maxwell arms use total
physical Green strain; their stress does not enter the equilibrium branch's yield test. FIT alone
selects coefficients, times and competing models. CHECK can reject the retained choice without
selecting another candidate. Accepted output is an ordinary frozen `parallel_green_maxwell_plastic/1`
material, with per-node permanent strain, accumulated strain, known equilibrium force and separate
step/cumulative plastic-work receipts. Omitted input preserves the previous serialization and
incompatible-law refusals.

The complete synthetic [relaxation-plastic-equilibrium.json](examples/relaxation-plastic-equilibrium.json)
example retains nine FIT nodes and seven CHECK nodes, including an unobserved-force yielded peak.
Its independently constructed recovery ratio 2 and time 0.05 seconds are identification targets,
not measured fabric properties. Both native commands expose the same calibration and ordinary
binding/strip-history path. The browser workbench displays the known branch at every acquisition node,
retains the fixed-property and network-source evidence, and withdraws stale fitted selections after
input changes; see [relaxation-calibration.md](docs/relaxation-calibration.md).

Verification comprises 42 passing focused pattern tests, both fitting-derivative unit regressions,
all six native/portable calibration workflow tests and 155 browser lifecycle tests. The actual compiled-WASM worker identifies the
complete example, binds its frozen material, runs a 27-state strip history and reproduces its frozen
replay exactly. Corrupted CHECK forces and denied network-estimate consent refuse in both native
commands and the real browser worker. The parallel-cell worker also matches complete native
program, recipe-selection and material-study reports. Workspace formatting and strict Clippy for
all production libraries/binaries and changed integration tests pass. The full all-target Clippy
process was killed by the execution environment while checking the export library test target;
that gate and full-workspace test completion remain for CI. These focused results do not claim a
complete-suite pass.

This closes the bounded software feature `ftx-rc3`. The plastic experiment uses recorded-node
return-map updates; exact Maxwell convolution does not establish continuous-time exactness or
refinement of the plastic path. Joint identification of plastic and viscous coefficients, missing
temperature/moisture-dependent constitutive laws, measured specimen agreement and garment acceptance
remain open under `ftx-rc1`, `ftx-tm4`, `ftx-tm2` and `ftx-tm8`.

### 25.96 Identify permanent stretch alongside an independently known recovery spectrum

The existing whole-history permanent membrane calibration now admits an explicitly qualified
parallel Maxwell spectrum. Only the six directional yield and hardening coefficients vary;
the equilibrium tensor, recovery tensors/times, initialization, source records and operating
domains remain independently supplied. Every recorded signed strain/time node advances the
fixed recovery state, including unobserved force nodes and irregular dwells. Fits and held-out
residuals compare the complete parallel-network second-Piola load.

The fixed spectrum is replayed once per FIT/CHECK partition through the canonical forward law,
with its work charged to the existing whole-operation budgets. Algebraic starts remove its
known stress before estimating the permanent branch; final predictions add both stresses once.
CHECK is prepared only after FIT selection and excitation analysis and cannot choose a different
candidate. Accepted publication runs the complete ordinary material compiler, retaining the
source-locked spectrum and independent mass, bending, contact and thermal requirements.

Reports keep equilibrium storage and plastic loss unchanged and separately expose Maxwell
stress, spring storage, viscous strains, dashpot work and total storage. The original pure-plastic
wire representation and arithmetic remain unchanged. The complete artificial example
`examples/material-parallel-calibration.json` supplies 27 FIT and five CHECK nodes with two
fixed branches; independent recurrence, irregular-dwell, qualification, domain and budget tests
pass through the existing native calibration suite. See
[material-membrane-plasticity.md](docs/material-membrane-plasticity.md).

This implements the software scope of `ftx-tm4.4`. Together with section 25.95, either branch
can now be identified conditional on independently characterized coefficients for the other;
neither operation simultaneously identifies both unknown branches or resolves that inverse
problem's possible nonuniqueness. Synthetic recovery does not close measured cyclic textile,
damage/rupture or unchanged-pattern garment acceptance.

### 25.97 Plan production with different qualified machine and recipe configurations

Version 2 of the independent whole-garment cell model gives each physical cell a complete
source-locked machine, process qualifications and explicit recipe overrides. Omitted operations
inherit the current parent candidate; fixed overrides remain fixed. Every cell regenerates its
ordinary stitch stations, traversal count and feed on the unchanged pattern, including a cell
that ultimately remains idle. Material-stack, reverse-feed, pressure and requested access checks
retain their existing authority. Allocation and exact schedule reconstruction use these actual
source plans while preserving the bounded receipt cache and replay accounting of section 25.94.

Selected production, stitch output and thread inventory use the assigned cell's actual recipe.
Demand for the same exact yarn is combined across used cells before one supplier minimum,
increment, tier and stock evaluation. Missing used-cell coverage withholds complete totals.
Material/design/scenario studies compare the same actual feed, thread mass, order cost and
elapsed completion. Recipe search uses the explicit makespan objective; its resulting allocation
does not also establish a minimum-feed or minimum-cost assignment. The outer serial plan stays
a reference. Source-plan operation and retained-station budgets include every configured cell.

Both native commands and the portable/WASM interface expose an explicit-cell SVG preview,
retaining actual pitch/rate, machine and qualification locks and assigned global garment IDs.
An ambiguous configured-cell preview refuses until the cell is named. Browser sewing and study
views show actual machines, recipes and selected thread contributions. The complete artificial
`examples/sewing-heterogeneous-options.json` specifies the independently computed three-garment
acceptance case: 160 intervals, 9.9 modeled seconds, 3 m of the same yarn and one rounded 4 m
synthetic USD 14 order. These are declared numerical targets, not measured production performance.
See [sewing-production.md](docs/sewing-production.md).

This delivers the implementation of `ftx-pb2.2.6`; fresh Rust typechecking and native/portable
and compiled-WASM acceptance remain tracked there until those gates complete. The merged browser lifecycle
suite passes 157 tests; compiled-WASM cases were explicitly skipped in the storage-exhausted
local environment. Every garment still stays in one cell with independently qualified machines,
fixture units and handling labor. Shared operators/resources, inter-cell transfers, coupled time/cost
allocation, hardware execution and measured seam/garment acceptance remain open under `ftx-pb2.2`.

### 25.98 Released bovine tensile observations enter ordinary material selection

Ten individually identified bovine specimens from Schneider's 2026 CC BY 4.0 tensile release add
40 direct observations to the shared catalog: reported caliper thickness, maximum force, force at
break and instrument displacement at break. Original workbook units, cells, source directions,
version and verified asset hash remain attached. Canonical kgf-to-N normalization makes the force
observations usable in existing condition-qualified screening and frozen material selections.

The source's 33-versus-65 mm gauge discrepancy and inconsistent calculated stress columns remain
explicit. No stress, strain, modulus, warp/weft direction, contact thickness or simulation law is
derived from them. Missing specimen conditioning, hide location, supplier and finish stay missing.
This supplies useful cowhide selection data without promoting incomplete geometry into mechanics.
See [leather-tensile-materials.md](docs/leather-tensile-materials.md).

Three focused source/screening/replay tests and the existing catalog and screening suites pass.
The bounded data integration is `ftx-tm2.2`; resolving source geometry and matching a complete
mechanical/transport characterization remain under `ftx-tm2` and the unchanged-garment `ftx-tm8`
acceptance gate.

### 25.99 Identify permanent stretch and delayed recovery together

Optional `joint_relaxation` extends the existing complete-history permanent-membrane calibration
to identify six directional yield/hardening coefficients and one proportional Maxwell stiffness
ratio and recovery time in one bounded FIT problem. Fresh inputs require no provisional unknown
coefficients. Elasticity, strain and accumulated-history limits, network topology, initialization
and exact specimen/conditioning remain independently qualified inputs.

Every trial replays both canonical histories at every recorded node, including unobserved loads,
irregular dwells and reversals. The eight-column FIT excitation gate detects local confounding;
CHECK can reject the retained FIT choice but cannot select another. Both histories participate in
the existing shared work limits. Joint optimization solves the bounded local least-squares problem
with active-set free-coordinate updates and endpoint release; clipping an unconstrained coupled
step no longer stalls an otherwise identifiable example. Existing six-parameter arithmetic stays
unchanged.

Accepted output is one ordinary frozen material, with all eight fitted quantities retained as
estimates under the same receipt. The workbench distinguishes unknown recovery identified jointly
from an independently supplied fixed spectrum, displays both coefficients and their diagnostics,
and withdraws unresolved selections. The complete artificial
`examples/material-joint-calibration.json` supplies 27 FIT and five CHECK nodes with independently
constructed targets, and reaches normal pattern binding and timed cloth histories.
See [material-membrane-plasticity.md](docs/material-membrane-plasticity.md).

This bounded software feature is `ftx-tm4.5`. Multiple/directional unknown recovery arms,
temperature/moisture feedback into mechanical laws and physical specimen/garment acceptance remain
open. The recorded-node plastic update and backward-Euler recovery do not establish continuous-time
exactness, coefficient uncertainty, global uniqueness or measured textile agreement.

### 25.100 Finite operating windows constrain actual sewing production

The independent-cell production model now admits a source-qualified inclusive end to each
cell's availability. A complete garment assignment must finish within that window after
initial setup, all actual recipe feed, handling, fixture transitions and final unloading.
The same bounded allocation updates emitted stitches, used machines and combined yarn orders;
a shorter shift cannot retain an earlier infeasible allocation or its cheaper consumption.

Optional windows retain the established millisecond time basis and exact integer bounds.
The unbounded input keeps its previous report representation. Windowed plans report the
declared intervals and whether the old first-cell reference assignment had to be replaced
with a complete feasible baseline. Lower-bound feasibility is separate from incumbent
feasibility: exhausted inner search must not call a possibly feasible job impossible, and
an incomplete allocation never masquerades as an executable schedule. Existing whole-job
work caps still cover count solves, both allocations and exact schedule reconstruction.

This is the bounded software feature `ftx-pb2.2.7`, reaching both native commands and the
portable material-study path through the existing sewing authority. Synthetic exhaustive
small assignments, inclusive boundaries and final-unloading cases are the acceptance
checks. Shared operators, inter-cell transfers and measured factory execution remain open
under `ftx-pb2.2`; a window is an explicitly qualified input, not inferred live availability.
See [sewing-production.md](docs/sewing-production.md).

### 25.101 Exact supplier sale units reach live finite-stock purchasing

The source refresh path now admits a reviewed Fabric Mart decimal-yard storefront without
changing the canonical quote or purchasing schemas. Exact product, variant and SKU identities,
published textile specification rows, currency and quantity controls must agree across a
product-HTML/public-JSON/product-HTML observation bracket. A quarter-yard billing price is
cross-checked against the displayed yard price; Shopify's billing-unit minimum cannot replace
the storefront's explicit one-yard fabric minimum and quarter-yard increment.

One real nylon/Lycra activewear-knit purchasing identity enters the shared catalog with its
source-reported nominal width and missing composition fractions/physics retained as unknown.
The explicit displayed-yard inventory policy preserves both its selected capacity and the
separate underlying quarter-unit inventory, including any declared display difference. It
does not infer uninterrupted roll length, usable width, checkout reservation or fulfillment.
Changed identities, units, material specifications or inconsistent stock refuse publication.

The adapter participates in existing immutable evidence and supplier-revision replay. The
checked-in dated quote comes from an actual live refresh, and ordinary native purchasing
has exercised the one-yard order, excess-capacity and missing-continuity outcomes. This
implements `ftx-pq1.1`; current availability always requires another source observation.
Broader supplier coverage, representative market percentiles and empirical specimen data
remain under `ftx-tm6`, `ftx-pq1` and `ftx-tm2`.
See [fabricmart-stock-refresh.md](docs/fabricmart-stock-refresh.md).

### 25.102 Retained liquid shares one mechanical and assembly mass authority

An explicit `dry_textile_plus_fixed_retained_liquid/1` selection adds independently qualified
liquid mass once to the dry substrate on the same reference textile area. Specimen,
conditioning, dry/additional mass bases and a closed co-moving no-exchange regime are required.
Mechanical observations must qualify that exact liquid loading; a heavier wet selection cannot
silently retain unqualified dry membrane, bending, thickness or friction observations.

The shared authority supplies ordinary per-piece/sewn cloth, gravity and inertia, body motion,
timed strip experiments, retained assembly mass and roll/remnant gross-stock mass. Reports keep
dry, liquid and total components, and propagate reported endpoints only when both observations
provide usable bounds. Attached accessories remain independent. Material studies compare the
actual combined results and include liquid acquisition/retention protocols in their admission.
Static membrane experiments and sewing thickness also enforce the selected wet state without
forcing otherwise independent dry consumers to acquire unrelated properties.
The browser material inspector uses that same wet-thickness qualification, so a stale dry
measurement cannot advertise sewing readiness. Actual generated WebAssembly and native strip
histories agree completely on dry/wet mass, dynamics, frozen-source replay and source refusals.

This completes the bounded software scope of `ftx-vc2.2`, with source, conservation, motion,
stock-accounting and consumer-refusal regressions. It supplies no new empirical wet-textile
coefficients. Existing dry thermal/vapor and pressure-conditioning consumers explicitly refuse
this fixed-liquid selection until the corresponding liquid storage, transport and retention
effects are independently supported. The unchanged-pattern linen/leather/performance-knit
acceptance remains dependent on matched real specimens under `ftx-tm2` and `ftx-tm8`.

The next coupling task, `ftx-vc2.3`, must advance changing textile liquid and cloth together:
evaporation, supply, condensation and drainage change local mass and require explicit incoming/
outgoing momentum, wet constitutive domains and conservative energy accounting. Accepted-step
rollback must include every coupled state. A thermal history computed after a dry mechanical
run cannot establish this two-way behavior. See [attached-mass.md](docs/attached-mass.md).

### 25.103 Temperature-dependent textile laws participate in free-water heat exchange

The existing textile free-film consumer now accepts the canonical temperature-dependent dry
conductivity and specific-heat laws in all three water versions: evaporation, prescribed supply,
and qualified condensation/re-evaporation. The remaining liquid augments only the inner cell's
capacity. Dry storage follows the source enthalpy integral, while liquid storage, latent exchange
and incoming/outgoing sensible energy remain separate conserved quantities.

Signed enthalpy inversion handles cooling and heating within the characterized temperature domain.
It refuses an unsupported preload rather than extrapolating the source law. Exact dryout resumes
the same frozen dry authority; the reference textile mass and material identity do not change.
Bounded inversion work is charged to the existing cell-step cap, and late failures roll back the
whole thermal/water interval. Constant-property histories retain their existing arithmetic and
work allowance.

Material/scenario decisions reconstruct dry storage with the same source integral and constrain
sampled thermal ranges to each material's domain. A substituted temperature profile therefore
reaches motion, evaporation/condensation and decision metrics through the ordinary material
selection. Independent analytic quadratics, source-domain refusals, dryout, multicell transport
and actual selected-body workflows verify the composition. Both native commands and the portable
adapter agree on single and named-case reports; missing Ret still refuses heat without erasing
manufacturing results. See [textile-surface-water.md](docs/textile-surface-water.md).

This delivers `ftx-vc2.4`. The coupling still requires independent qualification for retaining the
exact dry profiles over the declared liquid-loading domain. It does not infer sorption, wet-property
changes, liquid transport between layers, added water mass in mechanical dynamics, thermoregulation
or empirical garment warmth. Those coupled models and matched measurements remain open.

### 25.104 A separately published Cotton Duck heat-capacity model enables transient use

The NISTIR 7282 catalog now offers an explicit linear-specific-heat alternative for the same
historical conditioned Cotton Duck material group. Appendix A Table 1 and Figure 7A independently
state this source-authored model; it does not repair or choose between the conflicting cubic
representations. The original ten NIST records and their identities remain unchanged.

The ordinary material authority combines the published Celsius heat-capacity fit with the group's
existing conductivity and explicit estimated areal mass. The common admitted range is 20..70 C.
Point evaluation, integrated sensible storage, transient layer histories and frozen source replay
use that selection without solver changes. Estimate consent, source identity and temperature-domain
refusals remain mandatory. Original coefficients, fit diagnostics, conditioning and source PDF hash
are retained; reported residuals are not converted into prediction uncertainty.

This delivers the bounded source-model capability `ftx-tma.1`. The record represents a conditioned
material group rather than an identical shared coupon or exact lot, and its mass remains derived
from reported group means. Missing mechanics, cubic discrepancies, independent coupon/garment
validation and cold-weather extrapolation remain unresolved. See
[nist-thermal-materials.md](docs/nist-thermal-materials.md).

### 25.105 One qualified operator can tend multiple autonomous-feed sewing cells

Parallel production now admits one explicitly shared operator while retaining independent machines,
fixtures and whole-garment cell ownership. Source-qualified initial arrival, directed travel,
waiting, handling, setup and final unloading occupy that person exclusively; only independently
qualified autonomous feed releases the operator. Both cell and operator availability ends remain
inclusive. The actual assignment still drives stitch geometry, yarn consumption and purchasing.

The bounded search considers complete garment allocations and operator interleavings of retained
serial schedules. It distinguishes that restricted search from the unrestricted joint optimum;
optimality requires a complete feasible result meeting an independently valid lower bound. Every
positive operation feed receives its own millisecond ceiling, so earlier rounding cannot release
a later machine operation prematurely. Shared work admission reserves a complete baseline seed.

This delivers the bounded scheduling capability `ftx-pb2.2.8` through ordinary native, portable,
material-study and browser consumers. The browser retains the timed operator phases and withdraws
them after source changes. Existing independent-labor inputs retain their old representation.
Measured labor/autonomy applicability, heterogeneous shared fixtures, garment transfers and hardware
execution remain separate work. See [sewing-production.md](docs/sewing-production.md).

### 25.106 Qualified seam and closure heat paths reach garment coverage decisions

The existing steady coverage authority now accepts explicit retained mesh-edge centerlines with
source-qualified excess linear thermal conductance. Their contribution is additional to the exact
unbridged reference stack under the same reservoir films, rather than a second full seam footprint.
Actual current centerline length and projected coverage ownership are recomputed; shared mesh/body
boundaries count each physical portion once, and inconsistent neighboring stacks refuse.

Every consumed qualification binds the existing source/topology identity, ordered materials,
interfaces, films, area-ratio ranges and temperature/length domains. Material alternatives select
their own matching observations. Baseline, signed excess and total power remain explicit, and the
ordinary body-coverage objective consumes the total. Comparison admission binds the requested and
frozen policy and the observations actually consumed; unqualified or stale coverage cannot supply
a favorable objective while independent supported manufacturing remains available.

This delivers the bounded steady capability `ftx-tm5.1` through native commands, portable/browser
APIs, frozen replay and material studies. No-bridge behavior is retained. The first model excludes
transient bridge storage, exterior radiation coupling, inferred seam microgeometry, generic
heterogeneous seam laws and empirical warmth claims. Matched physical seam/closure measurements
and garment validation remain under `ftx-tm5` and `ftx-tmf`. See
[steady-thermal-coverage.md](docs/steady-thermal-coverage.md).

### 25.107 Published material laws reach standalone browser heating and cooling

Standalone point and transient thermal calculations now share one source-resolving authority
between the native workbench and WebAssembly. Browser clients can evaluate a selected material at
a stated temperature, or integrate a layered heating/cooling history with the existing prescribed
films and qualified exterior exchange. Temperature-dependent source laws, material enthalpy,
estimate consent, applicability domains and finite work/trace limits retain the native behavior.

The browser accepts original catalog selections or exact frozen selections, and the existing
native report schemas remain stable. This makes the published Cotton Duck linear-Cp alternative
usable in browser transient calculations without adding another solver or duplicating the workflow.
The implementation is tracked by `ftx-tma.2`; measured garment validation, absent material
observations, comfort physiology and graphical history-authoring controls remain separate work.

### 25.108 Regional seam heat and closed garment paths reach the workbench

Qualified seam/closure paths now support distinct prescribed body-region temperatures and films.
Each projected centerline interval selects its own exact source qualification and uses its local
reservoir difference, so one path can contain both heat loss and heat gain. Signed interval powers
remain visible and add to the original coverage baseline once. Shared-boundary ambiguity still
refuses when neighboring qualifications or physical boundary conditions disagree.

Simple closed paths represent cuffs, necklines and hems by repeating the first retained vertex at
the end. The actual closing mesh edge contributes once, while backtracks, internal revisits,
overlapping centerlines and repeated edges remain invalid. Open paths retain their authored length.

The materials workbench accepts an optional path file alongside ordinary coverage inputs and can
clear it to restore baseline-only coverage. Standalone reports, initial material alternatives and
steady motion endpoints display baseline, seam excess and combined signed power, current lengths
and consumed qualifications. Source edits withdraw stale results and downloads; native and portable
replay retain the same source locks. These extensions remain conditional software capabilities under
`ftx-tm5`; physical seam measurements, transient bridge dynamics and empirical garment acceptance
remain open. See [steady-thermal-coverage.md](docs/steady-thermal-coverage.md).

### 25.109 Source-qualified heating and cooling become interactive and exactly replayable

The materials workbench now exposes the shared material-point and dry transient thermal APIs.
An explicit temperature evaluates the current source selection's resistance, areal heat capacity
and admitted domain. A complete layered-history request runs independently of garment/body inputs
and retains signed boundary energy, stored heat, numerical balance, steady targets and qualified
exterior contributions. A bounded sample browser exposes every retained cell and time without
creating an unbounded page. Source changes cancel or withdraw obsolete results and downloads.

The shared Rust authority emits exact `replay_request_json` for each successful transient, preserving
the original controls and independently qualified frozen layers. That text replays with an empty
catalog and the same portable report; no browser reconstruction of hash-bearing material numbers
is required. Expanded requests must fit the existing 64 MiB consumer limit before integration.
Original inputs, complete reports and frozen replay remain separate downloadable products.

This extends `ftx-tma.2` into ordinary user controls and exact replay under `ftx-tma.3`. It adds no
material coefficients or new thermal solver, and does not close measured coupon/garment validation,
moisture/mechanical coupling, thermoregulation or warmth claims. See
[material-thermal.md](docs/material-thermal.md).

### 25.110 Shared fixture inventory constrains actual multi-cell production

Qualified sewing cells can now bind their existing fixture lifecycles to finite common pools while
sharing one operator. Source locks cover every cell's effective machine, recipe, material, timing,
availability and local fixture policy. Pool access and return require their own explicit protocol
within qualified handling; neither a shared name nor a matching needle implies free transfer.

Allocation and dispatch reserve pool units before handling and retain them through autonomous feed,
waits and any intervening operations until the declared release operation completes. A release can
supply another acquisition at the same instant. Pending releases can delay dispatch; persistent holds
can rule out an interleaving. Existing cell/operator windows and bounded complete-incumbent behavior
remain authoritative. Actual selected cells still determine stitch geometry, yarn demand and cost.

Native and portable reports retain exact baseline/selected reservations, global garment identities,
capacities, peaks and final released inventory. The workbench exposes those same histories and source
locks, with exact downloads and source-change invalidation. The bounded search continues to use
retained serial orders; its exhaustion does not establish unrestricted physical infeasibility.
This extends `ftx-pb2.2`; inter-cell garment routing, measured factory access/timing, seam quality and
hardware execution remain separate gates. See [sewing-production.md](docs/sewing-production.md).

### 25.111 Evaluated vector patterns enter the ordinary browser workbench

The existing evaluated GarmentCode importer now has local file controls in the materials workbench.
Users select source geometry and optional construction annotations, review the canonical conversion
report, and explicitly adopt the native pattern for material binding, manufacturing and supported
studies. Review retains physical panel/source mapping, missing semantics and `REVIEW_REQUIRED`;
conversion alone does not replace the active pattern or establish measured readiness or garment fit.

The portable adapter returns exact Rust-generated native pattern, report and optional fit-contract
texts alongside its existing structured values. Browser downloads and downstream input use those
strings directly, matching native artifact bytes and preserving signed zero and precise numeric
round-trips. Existing input and aggregate output limits remain enforced. Source changes, cancellation,
late reads and obsolete worker replies cannot publish an old conversion over current work. Adoption
uses the existing original-pattern reset for assignments, interfaces, outfit ownership and results.

This extends `ftx-a9dm` into a user-facing browser workflow without adding another geometry converter.
Unsupported source semantics and garment-completeness/fit qualification
remain with the canonical import and downstream consumers. See the [README](README.md#import-an-evaluated-garmentcode-pattern).

### 25.112 Physical limb panels retain their own geometry through placement

Already expanded arm and leg panels can declare an explicit physical side through the ordinary
pattern placement record. The same selection drives fit landmarks, body-frame placement and wrap/
roll without generating another cut piece or reflecting its flat geometry. Contradictory side,
quantity and fold declarations refuse; unplaced imported limb roles cannot infer a side. Existing
generated mirror pairs retain their reference behavior. The current Anny adapter supplies a
properly handed symmetric left start frame with right-derived heights and radii. Direct synthetic
tests cover sewn imported panels and independent side-specific frame providers; no asymmetric
anatomical, drape-convergence or physical-fit claim follows. See spec 01 §2.8 and spec 04 §3.4.

### 25.113 Route complete garments through independently qualified specialist cells

An explicit fixed operation route now lets one garment use several machines, including returning
to an earlier cell for finishing. Every original operation appears once per copy in dependency order.
Each specialist admits only its assigned passes against the unchanged pattern, while the complete
union still covers both seam sides. Actual assigned recipes determine stitch geometry, feed and
combined yarn purchasing through the existing sewing, preview and material-study consumers.

The first routing model uses dedicated attending labor at each cell, complete loading and unloading
around every operation, sufficient unloaded staging at every cell, and one independently qualified
serialized handoff resource. Complete source-bearing transfer intervals include their return to a
qualified ready state. Cell and transfer availability windows remain inclusive. No fictitious
all-capable serial reference machine, occupied-fixture transfer or autonomous feed is introduced.

Bounded dispatch retains a complete feasible batch, including a searched replacement when the
copy-order seed misses a window. Restricted search completion stays distinct from a proved minimum;
the latter requires matching an independent mandatory-work lower bound. Baseline and selected
timelines preserve exact garment/operation/cell identities and actual phase boundaries for replay.
This extends `ftx-pb2.2`; flexible machine routes, retained/shared fixture composition, measured
factory access/timing, seam quality and hardware execution remain separate work. See
[sewing-production.md](docs/sewing-production.md#route-one-garment-through-specialist-cells).

### 25.114 Circular source panels enter the ordinary garment workflow

Evaluated GarmentCode circular edges can now enter the shared native importer under explicit
author-chosen distance and whole-edge length limits. Source panel-unit radii and direction/major-arc
flags produce bounded polylines while preserving one original edge identity, physical panel,
stitch correspondence and source-direction notch fractions. The native pattern and import report
retain the policy, geometric and numerical allowances, source circle and actual stored lengths.
Unresolved precision, invalid geometry and excessive subdivision refuse without relaxing limits.

The materials workbench can author this policy when no annotations file is supplied, review each
admitted bound against its requested limit and adopt the exact native result. An annotations file
retains sole authority when supplied. Input or tolerance changes withdraw obsolete reviews and
dependent adopted results. Bounds describe frozen imported sewing-edge polylines; later alterations,
allowance offsets, stitch sampling and cloth meshing retain their existing numerical rules. This
extends `ftx-a9dm`; conversion remains `REVIEW_REQUIRED`, with no exact-circle or physical-fit claim.
See the [pattern contract](crates/ftx-pattern/CONTRACT.md) for source semantics and limits.

### 25.115 Find feasible production orders before optimizing scarce fixture use

Finite workholding now participates in finding the first complete sewing schedule, rather than
requiring the original copy-order seed to fit every fixture and acquisition/release prerequisite.
The same bounded optimizer backtracks out of fixture deadlocks, retains the first complete feasible
replacement, then improves its declared setup or setup-plus-handling objective. Source-qualified
capacities, machine/material/process locks and actual operation dependencies remain unchanged.

Feasibility and improvement share one work cap. A budget limit before any complete schedule remains
unresolved; exhaustive failure is scoped to the declared finite scheduling model. Once found, a
complete incumbent survives later budget exhaustion. Replacement baselines are explicitly marked,
and their fixture, handling and stitch histories replay the actual retained order. Existing feasible
source orders preserve their prior representation. Native serial, parallel and specialist-route
regressions cover this integration under `ftx-pb2.2.9`; measured fixture adequacy, factory timing and
hardware execution remain separate acceptance gates under `ftx-pb2.2` and `ftx-sp2`.

### 25.116 Published insulation specimens drive ordinary material comparisons

Nine source-identified nonwovens from Lupescu et al. (2025), DOI `10.3390/textiles5010005`,
extend the canonical catalog to 392 records. Seven milkweed/LM 2.2/PLA air-laid feed constructions
and the study's Thinsulate/Primaloft comparison specimens retain 108 measured or source-derived
scalar values from Tables 2–6. Original units, methods, specimen groups, humidity conditions,
reported variability, version-of-record PDF identity and CC BY attribution remain attached.

The existing material selection, unchanged-pattern mass accounting, steady conduction, condition-
qualified screening and frozen offline replay consume this cohort. Conductivity in mW/(m K)
normalizes at the shared boundary; the separately rounded source resistance remains authoritative
for steady comparisons. Gauge thickness at 100 Pa does not become unloaded contact thickness.
Input recipes do not become final composition after preferential fiber loss, and the branded
comparison specimens do not become arbitrary current supplier grades or purchase offers.

Humidity observations, compression percentages and recovery endpoints remain separately scoped
facts. They do not supply missing wet geometry, heat capacity, compression branches, membrane,
bending or interface friction. Five source/consumer regressions and eleven canonical catalog tests
verify the bounded software integration (`ftx-tm2.3`), without closing `ftx-tm2` or `ftx-tm8`.
Follow-up `ftx-tm2.4` acquires matched pressure-area/thickness/transport, humidity and heat-storage
observations for fills so the same material authority can drive measured load-to-loft-to-heat
comparisons. See [airlaid-insulation-materials.md](docs/airlaid-insulation-materials.md).

### 25.117 Prescribed pressure controls the same compression and heat history

The existing stack authority now accepts a declared normal pressure and computes each layer's
source-qualified loft and transport before the thermal hold. Version
`ftx-compressed-thermal-history/3` admits either pressure or clearance on every hold and keeps
the same material cells, temperatures, reference mass, branch work and heat ledgers. It uses the
existing thermal integrator, including an optional request-wide adaptive work budget; material
changes therefore propagate without another physics implementation.

Forward pressure control admits characterized constant-thickness intervals that cannot be inverted
uniquely from clearance. Qualified loading/unloading transitions join at the independently known
current pressure and thickness, including when the next hold changes to clearance control.
Source/specimen/conditioning/storage identity, pressure ranges and loading direction remain
mandatory; unsupported reversals and over-domain loads refuse atomically. Attempted adaptive
work remains charged after a failed step, while physical state rolls back. Mechanical work stays
separate from sensible heat.

Both native material commands and the portable browser engine expose the same request and exact
frozen final layer selections. The existing workbench labels the prescribed boundary separately
from the computed pressure and loft and retains the original input, complete report and every
final layer selection for ordinary pattern assignment. Version-1/2 requests retain their existing
semantics. Known-answer pressure/loft/work/heat, mixed controls, source-qualified plateau joins,
native replay, downstream material binding and refusal regressions cover this bounded software
delivery under `ftx-cp2.2`; see [compressed-thermal-history.md](docs/compressed-thermal-history.md).

This is prescribed-boundary coupling. Solving pressure from a moving garment/body contact field,
identifying real loading/unloading/recovery behavior, and validating measured layered insulation
remain open under `ftx-cp2`, `ftx-tm2.4` and the empirical garment gates. A fiber name, a branded
garment or one compression percentage cannot supply those missing constitutive observations.

### 25.118 Search local pattern shape through linked vertex alterations

Finite-grid and adaptive fit discovery now generate canonical vertex moves alongside existing linked
slash sites. One authored scalar can move several named source vertices horizontally, vertically or
diagonally, including paired front/back changes. This enables bounded local shoulder, neckline,
armhole-endpoint and taper searches without adding a second geometry or fit authority. Every trial
retains exact scalar coordinates and operations, canonical rounded geometry and the selected body
identity; ordinary seam, free-edge truing, outline and fit gates still precede all real consumers.

Complete domain admission rejects malformed sites, zero multiplier pairs and nonfinite or underflowed
displacements, including points outside a visited grid prefix. Actual adaptive coordinates are checked
again. Original slash inputs retain their representation and behavior; mixed site order, document-work
limits, failed-candidate reporting and exact frozen replay remain shared across native and portable
paths. Independent synthetic regressions cover local dimensions, matching seam lengths, cutting area,
assembly mass, relative curves, unchanged marks and refusal of unbalanced seams. This extends
`ftx-ds2`; authored geometric targets and bounded search do not establish empirical fit, safe ease,
continuous-domain completeness or a global optimum. See [material-screening-and-studies.md](docs/material-screening-and-studies.md).

### 25.119 Distribute each sewing operation across independently eligible cells

An explicit eligible-cell routing model now lets different physical copies choose different qualified
machines at each authored operation. Each declared operation/cell pair passes the ordinary source,
geometry, recipe and access authorities; every possible adjacent directed handoff needs evidence.
Bounded dispatch uses actual previous cells, dedicated attending workers, full load/setup/feed/unload
phases, staging and inclusive resource windows. It retains complete feasible incumbents and can replace
an infeasible seed assignment. Actual choices determine stitches, feed, thread allowances, combined
orders, previews and material-study results; unused alternatives contribute no invented consumption.

The outer seed garment is explicitly marked as reference geometry. Complete production rows identify
the actual cell and its source operation, and the workbench displays every eligible recipe with its
selected copy count. Full eligible domains remain part of comparison protocol identity even when only
feed or yarn is compared. Authored-route and recipe selection can use the same makespan authority;
cost or feed optimum claims require an objective that actually searches those quantities. Independent
relaxed work bounds remain separate from completion of the restricted append-dispatch search.

This extends `ftx-pb2.2` through the existing native and portable consumers. Synthetic exhaustive small
schedules, actual recipe geometry, quantity accounting and source/window/budget refusals verify the
bounded software model. Retained/shared fixture composition, measured factory timing and seam quality,
and hardware execution remain independent requirements; the finite single-operation fixture
composition is delivered in section 25.120 below. See [sewing-production.md](docs/sewing-production.md).

### 25.120 Share finite qualified fixtures across routed specialist cells

Fixed and eligible-cell sewing routes now reserve finite cell-local fixtures and shared pools for each
complete loading, setup, feed and unloading operation. The existing bounded dispatch search accounts
for simultaneous demand across machines, including reservations inserted out of physical-time order.
Independent full-timeline replay checks local and shared capacities, releases before equal-time
acquisitions, and zero final held inventory. Equal resource names do not imply shared inventory.
Eligible choices retain every source qualification, while occupied holds follow the selected cell
for each actual operation. Unused eligible plans do not consume fixtures.

The original machine and material/process qualifications remain authoritative. Common-pool access
and return require independent source evidence and exact cell locks; their time must be covered by
the qualified handling observations. A material substitution must still satisfy those qualifications.
The selected constrained completion and exact stitches flow through ordinary material studies and
the existing browser workbench, including complete fixture reservations and downloadable replay.

The synthetic two-copy route completes in 14.7 seconds with one shared fixture, against a complete
16.1-second copy-order baseline; two fixtures recover the unconstrained 12.6-second result without
changing stitch count or feed. Source/domain refusals, future reservations, multiple pools, finite
windows, recovered seeds and capped incumbents have direct regressions. This bounded software
delivery is tracked by `ftx-pb2.2.10`; see [sewing-production.md](docs/sewing-production.md).

Every hold must begin and end on one operation. Occupied-fixture transport, cross-operation retention
and shared attending labor need distinct qualifications and remain outside this protocol. Exhausting
the restricted dispatch search proves neither an unrestricted optimum nor physical infeasibility;
only equality with the independent lower bound establishes the reported optimum. Physical grip,
access, seam quality and hardware authorization remain separate requirements under `ftx-pb2.2`.

### 25.121 Supplied and condensed liquid changes the actual moving garment

The existing accepted-step liquid authority now admits independently qualified incoming liquid and
condensed vapor alongside evaporation. One source-owned reference-triangle inventory supplies both
the thermal ledger and the welded cloth's inertia and gravitational load. Dry fabric and attached
accessory observations remain separate. Material substitutions retain the full material/interface
report lock and per-instance loading/temperature domains; no fiber label or dry measurement implies
wet mechanical qualification.

Every accepted thermal interval records incoming and outgoing mass separately, including equal-net
throughflow. The same mechanical authority applies co-moving endpoint exchange and records momentum,
kinetic energy and signed gravitational energy independently of latent and sensible heat. It preserves
source-law, contact, topology, gravity, history and constraint checks and invalidates the mass-dependent
solver cache. Any later thermal/source failure restores the whole pre-admission motion state.

The ordinary native, portable and browser scenario consumers carry the exact same qualification and
transfer ledgers into nominal thermal/water comparison admission. Zero or unchanged incoming mass
cannot acquire additional cumulative incoming momentum or bulk energy. The workbench exposes initial,
incoming and outgoing transfers, exact source reports and refusal withdrawal. Existing evaporation-only
requests keep their original restrictions; no second mass or scoring authority is introduced.

This bounded software delivery is tracked by `ftx-vc2.3.1`. Known-answer conservation and independent
variable-mass oscillator refinement, sewn heterogeneous/accessory ownership, whole-motion rollback,
comparison tampering and both native commands cover supplied and condensing single/named-case studies.
See [textile-surface-water.md](docs/textile-surface-water.md). Initial water is explicitly admitted
co-moving after a dry drape. The first-order endpoint model requires mechanical time-step refinement;
relative flow impact, drainage, inferred wet constitutive changes, pinned-coordinate exchange impulses
and empirical wet-garment qualification remain outside this model and under the open parent work.

### 25.122 Guide adaptive alterations with actual material-bound consumer results

Adaptive design studies can now nominate one authored material alternative and one existing nominal
comparison objective to guide bounded alteration search. Canonical geometry and selected-body flat-fit
gates run before actual manufacturing and optional single-scenario probes. The ordinary comparison
authority supplies readings, source admission, required consumers, constraints and protocol compatibility;
no surrogate fit score or browser ranking replaces those quantities. The original fit-only search remains
available with its original behavior.

Consumer-guided polling continues after the first flat-passing designs and replaces inferior retained
geometries. Each selected geometry still crosses every authored material alternative. A bounded cache
reuses the nominated material's exact successful manufacturing and scenario outputs, so final assembly
does not rerun that incumbent against an exhausted scenario allowance. Every trial retains raw fit
results, exact altered geometry, consumer readings and refusals. Probe and final manufacturing work
share aggregate reservations and counters; scenario callbacks use their existing cumulative session.

Evaluation/step stops and individual callback refusals preserve admitted cached candidates. Structural
or global manufacturing hard-budget failures still refuse the whole request. The guidance material does
not optimize the other material alternatives, and named-case matrices refuse this mode until an explicit
case aggregation rule exists. Unvisited values, global optimality and empirical garment fit remain
unqualified. This bounded extension is tracked by `ftx-ds2.1`; the broader `ftx-ds2` remains open.
See [material-screening-and-studies.md](docs/material-screening-and-studies.md).

## Appendix A. Sources

- Brégier et al., "Human Mesh Modeling for Anny Body", arXiv 2511.03589 (2025); github.com/naver/anny.
- Li et al., "Incremental Potential Contact", SIGGRAPH 2020; Li et al., "Codimensional Incremental Potential
  Contact" (C-IPC), 2021; ipc-toolkit (MIT).
- The reference implementation's behaviour specifications: `docs/spec/01…04`; the Anny forward model: `docs/spec/05`.
- Research behind the reference (body measurement and body models; formats and manufacturing; Blender
  sewing; the frankensim cloth survey; the photogrammetry port survey), summarised where used above.
