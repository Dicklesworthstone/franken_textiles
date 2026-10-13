# Feature parity

One row per reference feature. Status is one of `present | partial | missing | excluded`; **partial never
rounds up**. A row becomes `present` only when the fixture IDs in its last column pass in
`ftx-conformance`. Spec references point into `docs/spec/`. Reference: the `fabric-pattern-creation-and-alteration`
skill's Python implementation at revision `88e03965`; body model: `anny==0.6.1`.

Last updated: 2026-10-12. Pattern geometry: `tests/fixtures/oracle/pattern/geometry.json` (404 cases captured
from the reference by `tools/oracle/capture_pattern_geometry.py`; 98.8 % of 15,897 numbers bit-exact, worst
3.9e-13 relative) passes in `ftx-conformance`. `ftx-body`: the Anny rows pass against the captured oracle
`tests/fixtures/oracle/body/anny-0.6.1-reference.json` (`crates/ftx-body/tests/anny_parity.rs`, run with
`--include-ignored` after `tools/body-model/convert_anny.py`).

## Patterns (`ftx-pattern`, spec 01)

| Feature | Spec | Status | Fixtures |
|---|---|---|---|
| `fabric-pattern/1` document read/write round trip | 01 §2 | present | oracle `pattern/drafts` (9 drafted garments, semantic equality) |
| `fabric-fit/1` document | 01 §3 | present | read with the reference's dictionary semantics by validation; oracle `validate/validation` |
| Curve geometry (chord-relative cubic/quadratic, sampling counts) | 01 §4 | present | oracle `pattern/geometry` (abs_to_rel, rel_to_abs, curve_points, mirror/reverse_curve); F1–F3 |
| Numeric helpers (lengths, fractions, tangents, areas, widths) | 01 §5 | present | oracle `pattern/geometry` (polyline_length, cumulative, at_fractions, tangent_at, signed_area, horizontal_width, point_in_polygon); F4–F6 |
| Instances (mirror pairs, on-fold unfolding, edge suffixes) | 01 §6 | present | oracle `pattern/instances` (F7–F10, curved pairs), `pattern/drafts` |
| Structure checks with the reference's messages | 01 §7 | present | oracle `pattern/structure` (24 F11 cases) |
| Seam sides, notch fractions, seam pair sampling | 01 §8 | present | oracle `pattern/seams` (F12–F21, F26, F27), `pattern/drafts` |
| Validation: seam walk, truing, points of measure, status, table | 01 §9 | present | oracle `validate/validation`: 9 drafted garments, M1–M12, F12–F32 (48 reports bit-exact, 48 tables identical) |
| Default fit spec by role | 01 §9 | present | oracle `validate/default_fit_spec` (9 garments) |
| Alteration: slash-spread, move, true-seam, cap-ease, transfer-curve | 01 §10 | missing | |
| Drafting: Zouave bolero (incl. soutache layout) | 02 Part C | missing | |
| Drafting: Zouave trousers | 02 Part C | missing | |
| Drafting: fez | 02 Part C | missing | |
| Style parameter validation (unknown keys refused) | 02 Part C | missing | |

## Bodies (`ftx-body`, plan §9)

| Feature | Spec | Status | Fixtures |
|---|---|---|---|
| Analytic landmarks and sections (compatibility with drafted values) | 02 Part B | partial | landmark heights (torso, arm, leg frames) in `ftx_pattern::Measurements`, exercised by every girth and width point of `validate/validation`; dress-form sections not ported |
| Anny data conversion at the pin | 05 §1, §8.1 | present | package array and label hashes (converter); `loaded_arrays_hash_to_the_package_values` |
| Anny forward model (phenotypes, local changes, skeleton, skinning) | 05 §2–5 | present | anny-0.6.1-reference cases 0–15 (worst 1.05e-14 m) |
| Anny anthropometry (height, waist, volume, mass, BMI) | 05 §6 | present | anny-0.6.1-reference cases 0–15 (worst 1.1e-13 relative) |
| Anny reference cases (16 phenotype, local-change and pose cases) | 05 §8 | present | anny-0.6.1-reference cases 0–15 |
| Tape functionals (girths as convex hulls of slices, lengths) | plan §9.3 | missing | |
| Fitting from age/sex/height/weight/BMI | plan §9.4 | missing | |
| Fitting from tape measurements | plan §9.4 | missing | |
| Poses (arm angles, seated, squat, nappy volume) | plan §9.5 | present | anny-0.6.1-poses (9 cases: A-pose 35/50°, arms up 150°, seated, squat, a ramp at t = 0.5, a nappy; poses re-implemented in NumPy by `capture_anny_poses.py`; worst 8.8e-16 m, 5.7e-14 relative) in `crates/ftx-body/tests/drape_poses.rs`; nappy size not yet calibrated to a taped `seat_over_nappy` (needs tape functionals) |
| Skin mesh for contact (closed, outward, checked) | plan §9.5 | present | the 9 pose cases (skin shell counts, volume, area, vertex order); refusals in `drape.rs` unit tests |

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
| Garment mesh (spacing, seam pairs, instance ranges) | 04 §2 | present | drape/garment_mesh (7 garments: bolero and trousers at 80/92/116 cm, bolero at 8 mm; structure exact, vertices 99.0 % bit-exact, worst 3.8e-15; all 42,003 triangles identical to Blender's CDT) |
| Placement and start shape (wrap, pre-sew) | 04 §3–4 | partial | drape/start_state (92 cm bolero and trousers): wrap with the reference's recorded frames, gather weights, sewing groups exact; pre-sew and relaxation to 1e-12 m against the reference's code with exact geometry; Blender's float32 BVH differs at crease ties (documented). Missing: part frames and landmarks on Anny (the reference's dress-form frames are not ported), inferred placement (§3.5), roll (§3.6) |
| Dynamics contract | 04 §5 | partial | implicit Euler on the incremental potential with body contact (IPC barrier on the ξ-offset gap, additive CCD, lagged Coulomb friction, start-intersection audit; `ftx-drape` CONTRACT); pre-sew, seams and flags pending; no self-contact |
| Metrics and flags with the reference's text and gating split | 04 §6 | missing | |
| Qualification bands and tightness gating | 04 §7 | missing | |
| Known-answer tests (plan §11.9) | 04 §8 | partial | KAT 2 (anisotropy, compression law), KAT 3 (Peirce cantilever vs exact elastica), KAT 9 (drape digest equal native/wasm) in `crates/ftx-drape/tests/kat.rs`; KAT 1 (barrier calculus vs AD and differences, unit test), KAT 4 (ring on a cylinder, 2πμT), KAT 5 (ring on a cone, tan α vs μ), KAT 6 (ease sweep), KAT 9 (contact digest equal native/wasm) in `tests/kat_contact.rs`; KAT 7 (elastic ladder), 8 (armpit wedge) and 10 (two-start spread) need elastics, seams, the garment scene and fitted bodies (FTX-09, FTX-13) |
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
| Malformed inputs that crash the reference | exception | structure error with a message | Fail closed (piece without id, `cut: null`, wrong Bézier control count, seam ref without `/`, zero-length side with notches; `ftx-pattern` CONTRACT) |
| `cut.qty` 2.7 or `"2"` | truncated or parsed to 2 | structure error | Only the integers 1 and 2 mean anything |
| Rewritten documents | insertion order, `"curve": null` kept | canonical key order, null curve dropped | Semantic equality is the bar (spec 01 §14.6) |
| DXF extents | ±1e20 placeholders | real extents | Correct values; no check reads them |
| Cubic Bézier samples | x³ via the platform `pow` (not correctly rounded, varies by platform) | correctly rounded x³ | Deterministic; ≤ 1 ULP from numpy, which disagrees with itself across platforms |
| Anny batch size | any batch, broadcast | batch 1 | Loop over bodies instead; simpler and as fast here (0.3–0.4 ms per body) |
| PDF/DXF creation dates | wall clock | the pattern's `created` | Reproducible files |
