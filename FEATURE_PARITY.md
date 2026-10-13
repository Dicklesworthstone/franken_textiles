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
| Alteration: slash-spread, move, true-seam, cap-ease, transfer-curve | 01 §10 | present | alter/alterations (21 cases: drafted bolero and trousers at 80/92/116 cm, a mutated bolero, error paths; 27,437 numbers bit-exact, history sentences and structure errors identical) in `crates/ftx-conformance/tests/alterations.rs`; CLI `ftx alter` (exit 2 where the reference exits 1) |
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
| Tape functionals (girths as convex hulls of slices, lengths) | plan §9.3 | partial | `ftx_body::tape` (sections, hull girth around an axis or the nearest loop, sections in a part frame, the back's median line, a height-bucketed `Slicer`; unit tests on prisms, a star, two bodies, a tilted prism) and `tape_readings` (15 measurements: stature, chest, waist, hip, waist/crotch/knee heights, thigh, knee, calf, ankle, upper arm, wrist, arm length, back neck to waist); neck, across back, shoulder length, total crotch length, outseam and head missing; not yet compared with the reference's slicing on fixture meshes |
| Fitting from age/sex/height/weight/BMI | plan §9.4 | partial | `fit_stature` (age, sex, stature; height parameter bisected) and `fit_tape` with the stature and `Wearer::weight_kg` (from `wearer.weight_kg` or `wearer.bmi`): stature within 2 mm and mass within 1 % at 2.5, 6 and 35 years (`tests/fit_tape.rs`); the WHO age prior and `MODEL_FIT` σ for the unmeasured missing |
| Fitting from tape measurements | plan §9.4 | partial | `fit_tape` (Levenberg–Marquardt over four phenotype parameters and fifteen `measure-*` changes with priors favouring the phenotype, residuals returned); G1b in `crates/ftx-body/tests/fit_tape.rs`: all 15 synthetic targets at 1.5–30 years recovered within 0.43 mm, the 92 cm reference toddler's 15 chart values within 2.4 mm; `MODEL_FIT` σ and `ftx-body/1` missing |
| Poses (arm angles, seated, squat, nappy volume) | plan §9.5 | present | anny-0.6.1-poses (10 cases: A-pose 35/50°, arms up 150°, seated, squat, a ramp at t = 0.5, a nappy, the straight-armed drape pose; poses re-implemented in NumPy by `capture_anny_poses.py`; worst 8.8e-16 m, 5.7e-14 relative) in `crates/ftx-body/tests/drape_poses.rs`; nappy size not yet calibrated to a taped `seat_over_nappy` (needs tape functionals) |
| Skin mesh for contact (closed, outward, checked) | plan §9.5 | present | the 10 pose cases (skin shell counts, volume, area, vertex order); refusals in `drape.rs` unit tests; `trimmed_skin` (hands and feet left out, openings capped) checked the same way |
| Part frames, landmarks, section radii on the body (placement) | 04 §3.2 on Anny | partial | `ftx_body::BodyFrames` (heuristics on the posed skeleton and skin, documented in `landmarks.rs`); on the tape-fitted 92 cm toddler waist and crotch heights equal the taped 530 and 372.5 mm; no oracle (the reference uses its dress form) |

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
| Placement and start shape (wrap, pre-sew) | 04 §3–4 | partial | drape/start_state (92 cm bolero and trousers): wrap with the reference's recorded frames, gather weights, sewing groups exact; pre-sew and relaxation to 1e-12 m against the reference's code with exact geometry; Blender's float32 BVH differs at crease ties (documented). On Anny: placements from `BodyFrames`, start untangling and the offset ramp (`scene.rs`). Missing: the reference's dress-form frames (not ported), inferred placement (§3.5), roll (§3.6) |
| Dynamics contract | 04 §5 | partial | implicit Euler on the incremental potential with body contact (IPC barrier on the ξ-offset gap, additive CCD, lagged Coulomb friction, start-intersection audit; `ftx-drape` CONTRACT); welded seams, grain-aligned rest, fabric from the pattern, sew and drape phases (`scene.rs`, `examples/drape_drafted.rs`); compression as stiff as tension with gather zones softening it per element to 3 % (spec 04 §5.3–5.4, `ClothModel::set_compression`); settledness and frames in the metrics; a drafted casing simulated at its cut length and drawn up by a tension-only elastic (two rings, fold and seam line; the reference simulates it at its finished size, `sim_scale`), the report saying whether its friction 2πμT carries the garment; no self-contact; a toddler fitted to 15 tape measurements takes about 30 s for the bolero and 2.6 min for the trousers (no friction lag round while sewing) |
| Metrics and flags with the reference's text and gating split | 04 §6 | present | drape/metrics (the reference's `metrics()` on four synthetic sequences over the exact start states of the 92 cm bolero and trousers: settled, slipping with open seams and penetration, stretched rings, trousers with sliding cuffs; 443 numbers bit-exact, all 51 flag texts identical) in `crates/ftx-conformance/tests/metrics.rs`; `sim_report.json` from `examples/drape_drafted.rs` lacks the run metadata fields (§6.11) |
| Qualification bands and tightness gating | 04 §7 | missing | |
| Known-answer tests (plan §11.9) | 04 §8 | partial | KAT 2 (anisotropy, compression law), KAT 3 (Peirce cantilever vs exact elastica), KAT 9 (drape digest equal native/wasm) in `crates/ftx-drape/tests/kat.rs`; KAT 1 (barrier calculus vs AD and differences, unit test), KAT 4 (ring on a cylinder, 2πμT), KAT 5 (ring on a cone, tan α vs μ), KAT 6 (ease sweep), KAT 9 (contact digest equal native/wasm) in `tests/kat_contact.rs`; KAT 7 (elastic ladder on the 92 cm trousers: 0.85, 0.95 and 1.0 × the waist grip, 1.2 does not and is flagged; release, needs the Anny model) in `tests/kat_elastic.rs`; KAT 8 (armpit wedge) and 10 (two-start spread) need fitted bodies (FTX-09, FTX-14) |
| Output formats (fold.pc2, mesh_map.json, report, viewer data) | 04 §9 | partial | `animation.rs`: board layout, fold kinematics (rigid flight with Horn rotations instead of SVD Kabsch, isometric roll-up, §3.6 roll), zip, PC2, mesh map, seam and strain colours, the `fabric-fold-viewer/1` payload and page (`assets/fold_viewer_template.html`, the skill's template byte for byte); unit tests (roll isometry within 1 %, last fold frame = wrap). No oracle yet; `sim_report.json` waits for the metrics |

## Exports (`ftx-export`, spec 03)

| Feature | Spec | Status | Fixtures |
|---|---|---|---|
| Text and number helpers (ascii_text, safe_name, formatting) | 03 §2 | present | `ftx-export` `text.rs`: fnum, ascii_text (NFKD from a table generated with the reference's Python), safe_name, text_width, wrap, stroke font, column letters, page lists; 524 oracle values byte-exact (`tests/export_helpers.rs`) |
| Seam-allowance cut outline | 03 §4 | present | `outline.rs`, `union.rs` (own overlay in place of GEOS): 102 outlines of the spec's fixtures and the drafted garments, methods equal, mitre within 1.2e-13 mm, union within 4.9e-6 mm Hausdorff (`tests/export_outlines.rs`) |
| Piece geometry (notches, grain, fold, internal, drills, decor, labels) | 03 §5 | present | `geom.rs`, `pole.rs`: 376 geometries, 38,422 numbers within 1e-9 mm incl. default grains (GEOS's inscribed circle: exact for convex triangles and quadrilaterals, else its branch and bound); main labels where the largest block fits (all fit; never > 5 % smaller unless the reference's overlaps) (`tests/export_geoms.rs`) |
| Layouts | 03 §6 | present | `layout.rs` `shelf_layout` (rotate to grain, y down/up, top offset); the master layout in `ctx.rs`; equal placements in every master SVG fixture |
| Master SVG | 03 §7 | present | `svg.rs`: 25 SVGs (fixtures, `--unfold`, `--layout-width 300`), 2,080 of 2,479 lines byte-identical, the rest union cut lines within 0.05 mm and main-label lines (`tests/export_svg.rs`); no `ftx export` command yet (FTX-18) |
| Tiled Letter and A4 PDFs | 03 §7 | present | `pdf.rs` (hand-written writer, FlateDecode via `miniz_oxide`, dates from `created`): same objects and content as the reference's files but for union cut lines (0.05 mm) and main-label lines (`tests/export_pdfs.rs`) |
| Copy-shop A0 and 36 × 48 in | 03 §7 | present | `pdf.rs`: orientation with fewer sheets, band with squares and scale bars; as above |
| Projector light and dark | 03 §7 | present | `pdf.rs`: grain vertical, 12 mm text, calibration grid layer off, the 200 in limit refused; as above |
| DXF-ASTM and DXF-AAMA (R12) | 03 §7 | present | `dxf.rs`, own writer (ezdxf-clean): same blocks and entities as the reference's (union cut lines 0.05 mm); `check_dxf` reproduces the reference checker (`tests/export_dxf.rs`) |
| Flat DXF (R2000, stroke font) | 03 §7 | present | `dxf.rs`: handles, tables, block records, root dictionary, mm; entity for entity as the reference's |
| HPGL | 03 §7 | present | `hpgl.rs`: commands, pens and polylines as the reference's (main-label strokes and union cut lines aside); `check_hpgl` reproduces the reference checker (`tests/export_targets.rs`) |
| Cutter SVGs and report | 03 §7 | present | `cutter.rs`: outlines within 0.05 mm as regions, the same report and manifest entry |
| GarmentCode JSON | 03 §7 | missing | |
| Tajima DST embroidery files | 03 §7 | missing | |
| Export manifest | 03 §7 | missing | |
| Acceptance checks (51) | 03 §8 | partial | the SVG and PDF checks (`checks.rs`): pass on ftx's files; on the reference's PDFs the PDF checker gives exactly its 294 items; mutations fail exactly their check; DXF, HPGL, cutter, GarmentCode and DST checks to come (FTX-17) |

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
| `ftx validate` (the reference's `fab validate`: table, exit codes, `--json` envelope) | 04 §11, plan §14 | present | the validation fixtures (via `ftx-pattern`); CLI parsing and wearer unit tests |
| `ftx drape` (replaces `fab sim`: Anny of the wearer's age and sex fitted to the tape, the drape, `sim_report.json`, `fold.pc2`, `mesh_map.json`, `fold_viewer.html`, the last frame and body as OBJ) | 04 §5–9, plan §14 | partial | run end to end on the drafted bolero and trousers and through the skill (`fab sim`/`fab round --sim`/`fab qualify` pick it when `ftx` is installed; Blender renders the stills from the OBJs); videos and the round's other commands missing |
| `ftx body` (the fitted body as `body.obj` + `ftx-body/1` JSON, any drape pose) | plan §9.4, §14 | partial | run on the reference toddler with a weight; `MODEL_FIT` σ for unmeasured values missing |
| Round logic, statuses, basis, workspace layout, rounds.jsonl | 04 §11 | missing | |
| Measurement gate, qualification, tightness gating in the round | 04 §11 | missing | |
| Report HTML | 04 §11 | missing | |
| Browser page (replaces the Pyodide page) | 04 §12 | missing | |
| 3D fold-up viewer data | 04 §9 | partial | written by `ftx drape` and `examples/drape_drafted.rs` (`fold_viewer.html`); the report line carries the run's own gating and advisory flags |

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
