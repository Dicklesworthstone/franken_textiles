# Feature parity

One row per reference feature. Status is one of `present | partial | missing | excluded`; **partial never
rounds up**. A row becomes `present` only when the fixture IDs in its last column pass in
`ftx-conformance`. Spec references point into `docs/spec/`. Reference: the `fabric-pattern-creation-and-alteration`
skill's Python implementation at revision `88e03965`; body model: `anny==0.6.1`.

Last updated: 2026-10-12 (plan stage, no Rust code).

## Patterns (`ftx-pattern`, spec 01)

| Feature | Spec | Status | Fixtures |
|---|---|---|---|
| `fabric-pattern/1` document read/write round trip | 01 §2 | missing | |
| `fabric-fit/1` document | 01 §3 | missing | |
| Curve geometry (chord-relative cubic/quadratic, sampling counts) | 01 §4 | missing | |
| Numeric helpers (lengths, fractions, tangents, areas, widths) | 01 §5 | missing | |
| Instances (mirror pairs, on-fold unfolding, edge suffixes) | 01 §6 | missing | |
| Structure checks with the reference's messages | 01 §7 | missing | |
| Seam sides, notch fractions, seam pair sampling | 01 §8 | missing | |
| Validation: seam walk, truing, points of measure, status, table | 01 §9 | missing | |
| Default fit spec by role | 01 §9 | missing | |
| Alteration: slash-spread, move, true-seam, cap-ease, transfer-curve | 01 §10 | missing | |
| Drafting: Zouave bolero (incl. soutache layout) | 02 Part C | missing | |
| Drafting: Zouave trousers | 02 Part C | missing | |
| Drafting: fez | 02 Part C | missing | |
| Style parameter validation (unknown keys refused) | 02 Part C | missing | |

## Bodies (`ftx-body`, plan §9)

| Feature | Spec | Status | Fixtures |
|---|---|---|---|
| Analytic landmarks and sections (compatibility with drafted values) | 02 Part B | missing | |
| Anny data conversion at the pin | 05 §1, §8.1 | missing | |
| Anny forward model (phenotypes, local changes, skeleton, skinning) | 05 §2–5 | missing | |
| Anny anthropometry (height, waist, volume, mass, BMI) | 05 §6 | missing | |
| Anny reference cases (16 phenotype, local-change and pose cases) | 05 §8 | missing | |
| Tape functionals (girths as convex hulls of slices, lengths) | plan §9.3 | missing | |
| Fitting from age/sex/height/weight/BMI | plan §9.4 | missing | |
| Fitting from tape measurements | plan §9.4 | missing | |
| Poses (arm angles, seated, squat, nappy volume) | plan §9.5 | missing | |

## Measurement (`ftx-measure`, spec 02 Part A)

| Feature | Spec | Status | Fixtures |
|---|---|---|---|
| Catalogue (35 measurements), aliases, garment kinds | 02 Part A | missing | |
| Size-chart priors and the reference wearer | 02 Part A | missing | |
| Fusion and flags | 02 Part A | missing | |
| Readiness gate and re-measure requests | 02 Part A | missing | |
| ChArUco sheet generation and sidecar | 02 Part A | missing | |
| Photo scale from the ChArUco sheet | 02 Part A | missing | |
| Plain Letter/A4 sheet detection and photo scale | 02 Part A | missing | |
| measure-photo with bootstrap σ | 02 Part A | missing | |
| Photogrammetry of a moving child (frames, cameras, still windows, triangulation, factor graph) | plan §10.2 | missing | |
| Multi-frame body fit | plan §10.2 | missing | |

## Draping (`ftx-drape`, spec 04 §1–9, plan §11)

| Feature | Spec | Status | Fixtures |
|---|---|---|---|
| Garment mesh (spacing, seam pairs, instance ranges) | 04 §2 | missing | |
| Placement and start shape (wrap, pre-sew) | 04 §3–4 | missing | |
| Dynamics contract | 04 §5 | missing | |
| Metrics and flags with the reference's text and gating split | 04 §6 | missing | |
| Qualification bands and tightness gating | 04 §7 | missing | |
| Known-answer tests (plan §11.9) | 04 §8 | missing | |
| Output formats (fold.pc2, mesh_map.json, report, viewer data) | 04 §9 | missing | |

## Exports (`ftx-export`, spec 03)

| Feature | Spec | Status | Fixtures |
|---|---|---|---|
| Text and number helpers (ascii_text, safe_name, formatting) | 03 §2 | missing | |
| Seam-allowance cut outline | 03 §4 | missing | |
| Piece geometry (notches, grain, fold, internal, drills, decor, labels) | 03 §5 | missing | |
| Layouts | 03 §6 | missing | |
| Master SVG | 03 §7 | missing | |
| Tiled Letter and A4 PDFs | 03 §7 | missing | |
| Copy-shop A0 and 36 × 48 in | 03 §7 | missing | |
| Projector light and dark | 03 §7 | missing | |
| DXF-ASTM and DXF-AAMA (R12) | 03 §7 | missing | |
| Flat DXF (R2000, stroke font) | 03 §7 | missing | |
| HPGL | 03 §7 | missing | |
| Cutter SVGs and report | 03 §7 | missing | |
| GarmentCode JSON | 03 §7 | missing | |
| Tajima DST embroidery files | 03 §7 | missing | |
| Export manifest | 03 §7 | missing | |
| Acceptance checks (51) | 03 §8 | missing | |

## Tracing (`ftx-trace`, spec 04 §10)

| Feature | Spec | Status | Fixtures |
|---|---|---|---|
| Trace pipeline | 04 §10 | missing | |
| names.json / labels.json, annotate | 04 §10 | missing | |
| Round-0 audit | 04 §10 | missing | |
| Scale-fit | 04 §10 | missing | |

## CLI and browser (`ftx-cli`, `ftx-wasm`, spec 04 §11–12)

| Feature | Spec | Status | Fixtures |
|---|---|---|---|
| Round logic, statuses, basis, workspace layout, rounds.jsonl | 04 §11 | missing | |
| Measurement gate, qualification, tightness gating in the round | 04 §11 | missing | |
| Report HTML | 04 §11 | missing | |
| Browser page (replaces the Pyodide page) | 04 §12 | missing | |
| 3D fold-up viewer data | 04 §9 | missing | |

## Excluded by decision (plan §23)

| Feature | Reason |
|---|---|
| Blender cloth mechanics (pre-sew/relax hacks, collider inset, pass-through modifier, `.blend` output) | Replaced by the native solver |
| The analytic dress-form mesh | Replaced by Anny (its landmark formulas are ported) |
| The Pyodide browser page | Replaced by wasm |
| Image generation through codex `$imagegen` | Stays in the skill |
| SMPL / SMPL-X / SMIL as defaults | Non-commercial licences |
| House and drone reconstruction specifics | Out of scope |

## Parity exceptions (reference behaviour changed on purpose)

| Behaviour | Reference | Port | Reason |
|---|---|---|---|
| Bisection with no root | returns the upper bound silently | typed refusal | Fail closed |
| Malformed inputs that crash the reference | exception | structure error with a message | Fail closed |
| DXF extents | ±1e20 placeholders | real extents | Correct values; no check reads them |
| PDF/DXF creation dates | wall clock | the pattern's `created` | Reproducible files |
