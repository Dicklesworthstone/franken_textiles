# Spec 01: pattern model, flat validation and alteration

Extracted 2026-10-12 from the Python reference implementation (fabric-pattern-creation-and-alteration
skill, `scripts/pattern_model.py`, `scripts/validate.py`, `scripts/alter.py`), its tests
(`scripts/tests/test_core.py`) and its reference documents (`references/PATTERN-JSON.md`,
`references/FIT-CONTRACT.md`). The method is spec-first: an implementer should be able to build a
conforming Rust port from this document alone, without reading the Python. Expected values in §14
were captured by running the reference (all 29 subsystem tests in `test_core.py` pass on it).

Normative words: **MUST** = conformance requirement; **SHOULD** = recommended; "reference" = the Python
implementation. Lengths are mm, angles degrees, unless stated otherwise.

<!-- TOC: 0 Scope | 1 Conventions | 2 fabric-pattern/1 | 3 fabric-fit/1 | 4 Curves | 5 Numeric helpers |
6 Instances | 7 Construction and structure_errors | 8 Seam sides and sampling | 9 Validation |
10 Alteration | 11 PieceBuilder | 12 External interfaces | 13 Python semantics | 14 Conformance oracle |
15 Reference crash paths | 16 Exclusions | 17 Surprises -->

---

## 0. Scope

| In scope (this spec) | Out of scope (other specs; only the interface is given here, §12) |
|---|---|
| The `fabric-pattern/1` and `fabric-fit/1` documents | Body model, landmarks, part frames, wrapping (`body.py`) |
| Curve geometry, numeric helpers, instance generation | Measurement files, fusion, priors, the reference wearer (`measurements.py`) |
| Seam refs, seam sampling, notch matching | Drafting programs (`drafting.py`), except the builder they use (§11) |
| Structure checks, seam walk, truing, points of measure, status | Exports (`export.py`), fold-up sim (`blender_sew.py`), tracer (`vectorize.py`) |
| Role-based default fit spec, text table, validate CLI | Round orchestration, workspaces, readiness gate (`fabric.py`) |
| Alteration ops and their CLI | Report HTML, browser page |

The validator is the **authority** for seam lengths, cap ease, crotch length, truing and finished
measurements. The fold-up sim never reports ease. A disagreeing cross-check is a bug to investigate,
never averaged in.

---

## 1. Conventions

| Rule | Detail | Enforced? |
|---|---|---|
| Units | mm everywhere. Top-level `units` must be `"mm"` (absent means mm). | yes (structure error) |
| Frame | 2D, x to the right, y up. | convention |
| Side | Every piece is drafted as the **wearer's right** side, seen from its **public (outer) side**. A front drawn with CF on the left is a wearer's-left front and must be mirrored before entering the model (the tracer does this and records `drawn_side: "L"`). A back drawn with CB on the left is the wearer's right half. | convention |
| Winding | Edge loops are **counter-clockwise** (signed area > 0). Adjacent outward-facing pieces then run opposite ways along a shared seam, which is why seam `reverse` defaults to `true`. | yes (structure error) |
| y origin | Torso and trouser pieces: y = 0 at the body waist line. Sleeves: y = 0 at the cap top. Only a convention; the real mapping is `placement.y0_landmark` + `placement.y0`. | convention |
| Curves | Control points are stored **relative to the edge chord** (§4.1), so moving a vertex keeps the curve's character. Same convention as GarmentCode, so control points copy unchanged. | by construction |
| Id grammar | Piece, vertex and edge ids MUST NOT contain `.` `/` `+` or a space. (The reference documentation also lists `:`; the code does not check it, §17.) Seam ids and notch ids are not checked. | yes (structure error) |
| Reserved suffixes | `.R` / `.L` mark instances (`front.R`) and the halves of unfolded pieces (`side.R`). `/` separates instance and path in seam refs; `+` joins edges. | by the id grammar |
| Instances | What is physically cut. Seams and points of measure address **instances**, never pieces (§6). | — |

---

## 2. `fabric-pattern/1` document

The canonical model. Every export, the sim mesh and the GarmentCode JSON are projections of it.

### 2.1 Top level

| Field | Type | Req. | Default | Meaning / consumer |
|---|---|---|---|---|
| `schema` | string | yes | — | MUST equal `"fabric-pattern/1"` (structure error otherwise). |
| `units` | string | no | `"mm"` | MUST equal `"mm"`. |
| `style` | string | no | — | Style tag, e.g. `"ZOUAVE_BOLERO"`. Export file stem and labels (export uses `"STYLE"` when absent). Not read by this subsystem. |
| `meta` | object | no | `{}` | §2.2. Alteration writes `meta.history` and `meta.origin`. |
| `fabric` | object | no | — | §2.3. Not read by this subsystem. |
| `pieces` | array of Piece | yes | — | §2.4. Absent → reference crashes (KeyError). |
| `seams` | array of Seam | no | `[]` | §2.9. |

All objects MAY carry extra keys; they MUST be preserved verbatim, in order, by every operation that
rewrites the document (§2.10).

### 2.2 `meta`

None of these is read by the model or the validator.

| Field | Type | Meaning |
|---|---|---|
| `title` | string | Human title. |
| `size_label` | string | e.g. `"920 mm stature"`; printed on export labels. |
| `origin` | enum `DRAFT` \| `AI_SHEET` \| `VECTOR_EDIT` | Provenance. Every alteration op sets `"VECTOR_EDIT"`. |
| `round` | integer | Round number; printed on export labels. |
| `source` | object `{image, sha256}` | Tracer provenance; alteration never touches it. |
| `scale_status` | enum `UNSCALED` \| `FITTED` | Tracer scale state. |
| `scale_fit` | object | Tracer scale-fit report. |
| `sa_mm` | number | Default seam allowance for exports (export falls back to 10.0). |
| `program` | string | Drafting program, e.g. `"drafting.zouave_bolero"`. |
| `style` | object | Style parameters used by the draft. |
| `solved` | object | Parameters the draft solved for. |
| `history` | array of `{op, args, why}` | Appended by every alteration op (§10.1). |

### 2.3 `fabric`

Not read by this subsystem; listed for completeness. `id` (string), `name` (string; export's default
material label), `kind` (`woven`, `felt`, …), `areal_g_m2` (sim default 200), `width_mm`,
`tensile_N_m` (sim default 8000), `bending_Nm` (sim default 3e-5), `sim_note`. A lined jacket is one
composite layer. The sim qualification fingerprint is `[areal_g_m2, tensile_N_m, bending_Nm]`.

### 2.4 Piece

| Field | Type | Req. | Default | Meaning |
|---|---|---|---|---|
| `id` | string | yes | — | Unique among pieces; id grammar. Absent → reference crashes. |
| `name` | string | no | — | Label text (export uses `id` if absent). |
| `material` | string | no | — | Informational (usually the fabric id). Not read here. |
| `cut` | object | no | `{"qty": 1}` (only when absent) | Cut instruction. The completeness check (§9.4) requires it to be present and non-empty. |
| `cut.qty` | integer 1 or 2 | no | 1 | 2 = mirror pair (`.R`/`.L`). Coerced like Python `int()` (truncation; numeric strings accepted, §13). Anything else is a structure error. qty > 2: make separate pieces. |
| `cut.on_fold` | string (edge id) | no | — | The piece is drafted as a half; the instance is unfolded across this edge (§6.3). The edge must exist and be straight with distinct finite endpoints. |
| `cut.material` | string | no | `fabric.name` | Export label only. |
| `vertices` | object: id → `[x, y]` | yes | — | Vertex coordinates. Duplicate JSON keys: last wins. |
| `edges` | array of Edge | yes | — | Closed CCW loop: edge *i* ends where edge *i+1* starts (cyclic). |
| `notches` | array of Notch | no | `[]` | §2.6. |
| `grain` | `[[x0,y0],[x1,y1]]` or legacy `{"p0":[..],"p1":[..]}` | no | — | Grainline. The completeness check requires it to be truthy. Readers MUST accept the legacy object; writers emit the pair (§10.1). |
| `internal` | array | no | `[]` | Drawn/cut internal lines (§2.7). |
| `decor` | array | no | `[]` | Braid/embroidery lines (§2.7). |
| `drills` | array | no | `[]` | Drill holes (§2.7). |
| `trace_marks` | array | no | `[]` | Tracer's unverified detections (same shape as `internal`); never exported; moved by alteration. |
| `sa` | number | no | — | Piece default seam allowance. Export precedence: `edge.sa` > `piece.sa` > `meta.sa_mm` > 10.0; `0` is a valid value. |
| `sim` | boolean | no | `true` | `false`: exported but not folded up (facings, pockets, hats). |
| `sim_scale` | `[sx, sy]` | no | — | The piece's **finished** size in the fold-up as x and y factors on its coordinates (a casing cut double and drawn in; a cuff folded in half). The cut piece stays full size. Sim only. |
| `note` | string | no | — | One making-up instruction, printed on the export label. |
| `role` | string | no | `""` | See role vocabulary below. |
| `placement` | object | no | — | §2.8. |
| tracer extras (`trace`, `grain_meta`, `drawn_side`, `symmetric`, `notes`, …) | any | no | — | Pass-through. |

Role vocabulary (by consumer): `default_fit_spec` (§9.5) reads `bodice_front`, `bodice_back`,
`sleeve`, `pant_front`, `pant_back`. Placement inference (body spec) maps `bodice_front`,
`bodice_back`, `skirt_front`, `skirt_back` → torso; `sleeve` → arm; `pant_front`, `pant_back` → leg;
every other role (`waistband`, `cuff`, facings, `hat_crown`, `hat_tip`) gets no placement.

### 2.5 Edge and curve

| Field | Type | Req. | Default | Meaning |
|---|---|---|---|---|
| `id` | string | yes | — | Unique within the piece; id grammar. |
| `from` | vertex id | yes | — | Start vertex. |
| `to` | vertex id | yes | — | End vertex. |
| `curve` | object or null | no | straight | Absent, null or `{"type": "line"}` = straight. |
| `sa` | number | no | — | Per-edge seam allowance (export). |
| other keys | any | no | — | Copied into every instance edge. |

| Curve field | Type | Meaning |
|---|---|---|
| `type` | `"line"` \| `"quad"` \| `"cubic"` \| `"poly"` | Default `"line"`. Any other value is a structure error. |
| `c` | array of `[t, h]` | Control points relative to the chord (§4.1). `quad`: exactly 1; `cubic`: exactly 2; `poly`: any number (0 allowed: a straight edge); `line`: ignored. |
| other keys | any | Preserved by mirroring. |

`poly` edges are the polyline through `p0`, the control points and `p1` (no densification). Traced
edges and transferred curves are `poly`.

### 2.6 Notch

| Field | Type | Req. | Default | Meaning |
|---|---|---|---|---|
| `id` | string | no | — | Name. |
| `edge` | string | yes | — | Edge id **as drafted** (no `.R`/`.L` suffix). On unfolded pieces it matches both halves. |
| `s` | number | yes | — | Arclength fraction 0..1 along the edge **as drafted** (from → to). Mirrored copies reverse it automatically (§8.3). |
| `type` | `V` \| `double` \| `slit` \| `T` \| `castle` \| `check` \| `U` | no | `"V"` | Rendering only; ignored by validation. |
| `depth`, `width` | number | no | export defaults | Rendering only. |

End notches (fraction along the seam side within 1e-4 of 0 or 1) are ignored for matching: the seam
ends already match.

### 2.7 Marks

| Key | Item shape | Notes |
|---|---|---|
| `grain` | `[[x0,y0],[x1,y1]]` (legacy `{"p0","p1"}`) | Moved by slash-spread and cap-ease (converted to the pair form). |
| `internal[]` | `{kind: "draw"\|"cut", label?, pts: [[x,y],…], closed?}` | Export: `kind == "cut"` is cut, anything else drawn; `closed` defaults to (more than 2 points and first ≈ last). Moved by alteration. |
| `decor[]` | `{kind: "soutache"\|"embroidery", pts, width_mm (3.0), closed (false)}` | Moved by alteration. |
| `drills[]` | `{p: [x,y], d: 3}` | **Not** moved by any alteration op (§17). |
| `trace_marks[]` | as `internal` (`kind` may be `line`, `parallel_line`, …) | Never exported; moved by alteration. |

### 2.8 Placement

Read here only by points of measure (§9.3) to locate landmark lines; the sim and the body spec own the
rest.

| Field | Type | Default here | Meaning |
|---|---|---|---|
| `part` | `"torso"` \| `"arm"` \| `"leg"` | `"torso"` | Body part. Arm and leg pieces use the right-side frame (`arm_R`, `leg_R`) for both `.R` and `.L` instances. |
| `y0_landmark` | landmark name | `"waist"` | The landmark that piece `y0` sits on. |
| `y0` | number | 0 | Piece y of that landmark. |
| `theta_deg`, `x0`, `clearance_mm` (25), `radius_mm`, `inferred` | — | — | Sim placement only (body spec). |

Piece y of landmark *L* = `h(part, L) − h(part, y0_landmark) + y0`, where `h` is the landmark height
in the part frame (§12).

### 2.9 Seam

| Field | Type | Req. | Default | Meaning |
|---|---|---|---|---|
| `id` | string | yes | — | Unique among seams. Missing ids are reported as duplicates of `None` and crash the reference elsewhere (§15). |
| `a`, `b` | seam ref | yes | — | The two sides (grammar below). |
| `reverse` | boolean | no | `true` | `true`: a's start pairs with b's end (adjacent CCW loops run opposite ways). |
| `ratio` | number | no | 1.0 when no band; none when a band is given | Target len(a)/len(b): 1 for plain seams, > 1 when a is eased or gathered onto b. |
| `ratio_band` | `[lo, hi]` | no | — | Accepted ratio range instead of a single ratio (cap ease, gathers). Legacy alias `band` is consulted **only when the `ratio_band` key is absent**. |
| `kind` | `plain` \| `ease` \| `gather` \| `dart` | no | — | Informational to validation; the sim and tracer audit use `gather`. |
| `tol_mm` | number | no | fit spec `seam_tol_mm` (3.0) | Per-seam residual tolerance. |

Seam ref grammar:

```
REF       := INSTANCE "/" PATH          (split at the FIRST "/")
INSTANCE  := piece-id | piece-id ".R" | piece-id ".L"
PATH      := EDGE ("+" EDGE)*
EDGE      := edge-id | edge-id ".R" | edge-id ".L"   (suffix only on halves of on_fold pieces)
```

- Edges in a PATH MUST be consecutive in the instance's loop: `edge[k].to == edge[k+1].from` (vertex
  equality, so a path may wrap from the last loop edge to the first, e.g. `tr_front.L/cf+crotch`).
- In a mirrored `.L` instance the loop order is reversed, so a multi-edge path is written in the
  opposite order to its `.R` twin (`tr_front.R/crotch+cf` ↔ `tr_front.L/cf+crotch`).
- One instance edge belongs to at most one seam side (validated, §7.3).
- Twin rule (documented, **not** validated): a seam touching a `.R` instance or a `.R` half needs an
  `.L` twin; drafting programs name them `<id>_R` / `<id>_L`.

### 2.10 Serialization rules

- Unknown keys at every level MUST round-trip unchanged and in order (the reference keeps insertion
  order). Alteration ops deep-copy the input document and change only the keys described in §10.
- JSON writers (alter CLI, validate `--out`) emit 1-space indentation plus a trailing newline. The
  reference escapes non-ASCII (`–` as `–`, `°` as `°`, `²` as `²`). A port MAY write
  UTF-8; conformance is **semantic** JSON equality (numbers compared numerically, §14.6).
- Integers in input stay integers (`target_mm: [70, 100, 130]`); numbers the code computes are floats.
- The model never mutates its input document.

---

## 3. `fabric-fit/1` document

### 3.1 Top level

| Field | Type | Default | Meaning |
|---|---|---|---|
| `schema` | string | — | `"fabric-fit/1"`. Not checked by the validator. |
| `garment` | string | — | Program or garment name. |
| `generated` | string | — | Present only on role-based specs: `"validate.default_fit_spec (role-based)"`. |
| `seam_tol_mm` | number | 3.0 | Default seam residual tolerance. |
| `truing_tol_deg` | number | 8.0 | Truing tolerance (|angle sum − 180°|). |
| `truing` | array of `{seam, end}` | `[]` | Extra truing checks at seam ends whose continuation edges are sewn. `end` ∈ `start`/`end` (in side a's direction). Entries naming unknown seams are ignored. |
| `points` | array of Point | `[]` | Points of measure. |

### 3.2 Point of measure

`target_mm` is `[min, nominal, max]` of **finished − body** (ease, or clearance for pass-through
points). The body side is `mval(body) × body_scale + body_offset_mm`.

| Field | Kinds | Req. | Default | Meaning |
|---|---|---|---|---|
| `id` | all | yes | — | Row id. |
| `kind` | all | yes | — | `girth` \| `width` \| `vertical` \| `path`. Other values → `UNMEASURABLE`. |
| `body` | all | yes | — | Measurement name (§12). Absent → reference crashes (§15). |
| `target_mm` | all | yes | — | Exactly 3 numbers, else `UNMEASURABLE`. |
| `label` | all | no | `id` | Display label. |
| `body_scale` | all | no | 1.0 | Multiplies the body value. |
| `body_offset_mm` | all | no | 0.0 | Added to the scaled body value. |
| `instances` | girth | yes | — | Instance ids whose widths are summed. |
| `landmark` | girth; width (if no `at_vertex`) | yes | — | Landmark name for the measuring line. |
| `dy` | girth, width | no | 0.0 | Shifts the measuring line (mm, +up). |
| `instance` | width, vertical | yes | — | Instance id. |
| `at_vertex` | width | no | — | Measure at this vertex's y instead of a landmark. |
| `from`, `to` | vertical | yes | — | Vertex ids; value = |y_from − y_to|. |
| `refs` | path | yes | — | Seam refs (§2.9 grammar) whose lengths are summed. |
| `scale` | path | no | 1.0 | Multiplies the summed path (e.g. relaxed elastic = casing × elastic/casing). |

---

## 4. Curve geometry

### 4.1 Chord-relative encoding

With `d = p1 − p0` and `perp(d) = (−d_y, d_x)` (d rotated +90°):

- `rel_to_abs(p0, p1, [t, h]) = p0 + t·d + h·perp(d)`
- `abs_to_rel(p0, p1, q)`: `L2 = d·d`; if `L2 == 0` exactly → error `"zero-length chord"`; with
  `w = q − p0`: `t = (w·d)/L2`, `h = (w·perp(d))/L2`.

`h` is in chord lengths (scale-free). `h > 0` is to the left of the chord direction, which for a CCW
loop is **into** the piece; `h < 0` bulges outward.

### 4.2 Sampling an edge: `curve_points(p0, p1, curve, dense = 200)`

| `curve.type` | Result |
|---|---|
| absent / null / `line` | `[p0, p1]` (2 points; `dense` ignored). |
| `quad`, `cubic` | Control points `c_i = rel_to_abs(p0, p1, c[i])`. Count MUST be 1 (quad) / 2 (cubic), else error `"quad curve needs 1 control points"` / `"cubic curve needs 2 control points"` (verbatim). Bezier of degree `k = count + 1` on the control polygon `P = [p0, c…, p1]`, sampled at `dense` parameters `t_j = j/(dense−1)`, `j = 0..dense−1`, Bernstein form `B(t) = Σ_{i=0..k} C(k,i)·t^i·(1−t)^(k−i)·P_i`. |
| `poly` | `[p0, rel_to_abs(c_0), …, rel_to_abs(c_{m−1}), p1]` (`dense` ignored). |
| other | error `"unknown curve type 'X'"` (Python repr of the value). |

Sample counts are part of the contract: lengths are polyline lengths of these samples, so a port MUST
use the same counts to reproduce reported values (a 200-sample cubic reads about 3 ppm short of its
true arclength; §14.2 F2).

| Context | `dense` |
|---|---|
| `edge_dense` default, `edge_length`, `perimeter`, seam sides, notch fractions, seam sampling, true-seam end direction | 200 |
| `Instance.outline` default (area, CCW check) | 60 |
| Points of measure (`girth`, `width`) | 120 |
| `transfer-curve` source edge | 60 |

### 4.3 Mirroring curves

- `mirror_curve(curve)` — parameters after mirroring the piece **and** reversing the edge (the two
  always happen together, which keeps loops CCW): null or `line` → unchanged copy; otherwise
  `c' = reverse([[1 − t, h] for [t, h] in c])`; all other keys kept.
- `reverse_curve(curve)` — same geometry traversed the other way: `c' = reverse([[1 − t, −h] …])`.
  Unused in the reference (§16).

---

## 5. Numeric helpers

| Helper | Exact semantics | Edge cases |
|---|---|---|
| `polyline_length(P)` | `Σ_i ‖P[i+1] − P[i]‖` | 1 point → 0. |
| `cumulative(P)` | `[0, s_1, …, s_{n−1}]`, `s_i = Σ_{j<i} ‖P[j+1] − P[j]‖` | Same length as P. |
| `at_fractions(P, fr)` | `s = cumulative(P)`, `T = s_last`; for each f: `u = clamp(f, 0, 1)·T`; x, y = piecewise-linear interpolation of `(s, P.x)` and `(s, P.y)` at u. | Values outside [0,1] clamp. With repeated `s` (zero-length segments) the value at a repeated abscissa is the **last** of the repeated samples (numpy `interp`); a zero-length polyline returns its last point. |
| `tangent_at(P, end)` | Unit tangent (direction of travel) at `start` or `end` from a least-squares quadratic fit, see below. | — |
| `signed_area(poly)` | Shoelace on the open ring (wraps last → first): `0.5·Σ (x_i·y_{i+1} − y_i·x_{i+1})`. | CCW > 0. |
| `point_in_polygon(pts, poly)` | Even-odd: for each edge (wrapping), crossing if `(y0 > y) ≠ (y1 > y)`; `xc = x0 + (y − y0)(x1 − x0)/(y1 − y0)`; count crossings with `x < xc` (strict); inside iff count is odd. | Used by the sim only. |
| `horizontal_width(poly, y)` | Total length of the line `Y = y` inside the closed polygon: collect `x` of every edge with `(ya > y) ≠ (yb > y)`, `x = xa + (y − ya)(xb − xa)/(yb − ya)`; sort ascending; sum `x[2i+1] − x[2i]` over complete pairs. | Half-open rule: a vertex exactly on the line counts as **below** it, horizontal edges never count, an odd trailing intersection is ignored. |

`tangent_at(P, end)`:

1. `p = P` for `start`, `p = reverse(P)` for `end`; `s = cumulative(p)`; `L = s_last`.
2. If `len(p) < 3`: `d = p[1] − p[0]`.
3. Else: `lo = min(2.0, 0.1·L)`, `hi = min(14.0, 0.5·L)`; `idx = {i : lo ≤ s_i ≤ hi}`. If
   `|idx| < 4`: `idx = 0 .. min(len(p), max(4, r)) − 1`, where `r` = number of samples with
   `s_i ≤ hi` (right-sided search). If still `|idx| < 3`: `d = p[1] − p[0]`. Otherwise fit
   `x(s) ≈ a0 + a1·s + a2·s²` and `y(s) ≈ b0 + b1·s + b2·s²` by least squares over `idx` and take
   `d = (a1, b1)` (the derivative at s = 0, extrapolated back to the vertex).
4. For `end`: `d = −d`. Return `d/‖d‖` (if `‖d‖ = 0`, return `d` unscaled).

The window starts ~2 mm out so a traced corner's rounding is not read as the edge direction. With
fewer than 4 samples the fit is the interpolating quadratic, which is not the first-segment direction
(§14.2 F5).

---

## 6. Instances

### 6.1 Instance record

| Field | Meaning |
|---|---|
| `id` | `pid`, `pid.R` or `pid.L`. |
| `piece` | Piece id. |
| `side` | `"R"`, `"L"`, or `"C"` (single, centre or unfolded). |
| `mirrored` | true only for the `.L` instance of a qty-2 piece. |
| `verts` | vertex id → point, in this instance's 2D frame. |
| `edges` | CCW loop of edge records (all original keys kept; `id`, `from`, `to`, `curve` possibly rewritten). |
| `piece_def` | The source piece object (for `role`, `placement`, `notches`, …). |

### 6.2 Instance methods

| Method | Semantics |
|---|---|
| `edge(eid)` | First edge with that id, else KeyError `"{inst.id}: no edge '{eid}' (have ['e1', 'e2', …])"` (Python list repr of the loop's edge ids). |
| `edge_dense(eid, dense=200)` | `curve_points(verts[from], verts[to], curve, dense)`. |
| `edge_length(eid)` | `polyline_length(edge_dense(eid, 200))`. |
| `outline(dense=60)` | Concatenate, over the loop, each `edge_dense(e, dense)` without its last point → open ring. |
| `area()` | `signed_area(outline(60))`. |
| `perimeter()` | `Σ edge_length(e)` over the loop. |
| `edge_tangent(eid, end)` | Unit direction of travel at the edge's `start`/`end`. `poly`: `tangent_at(edge_dense(eid), end)`. Otherwise exact from the control polygon `ctrl = [p0, controls (quad/cubic only), p1]`: at `start` the first `q ∈ ctrl[1..]` with `‖q − p0‖ > 1e-9` gives `unit(q − p0)`; at `end` the first `q` in `reverse(ctrl)[1..]` with `‖p1 − q‖ > 1e-9` gives `unit(p1 − q)`; none → `(1, 0)`. |
| `corner_angle(vid)` | Interior angle at a loop vertex. Take the **first** edge `e` (loop order) with `e.to == vid` and the next edge `n` (cyclic). `t_in = edge_tangent(e, end)`, `t_out = edge_tangent(n, start)`, `turn = atan2(t_in × t_out, t_in · t_out)` in degrees (`a × b = a_x·b_y − a_y·b_x`), result `180 − turn` ∈ [0, 360). No edge ends at `vid` → KeyError(vid). |

### 6.3 Generation: `piece_instances(piece)`

Step 1, the base instance:

- **No fold**: `Instance(pid, side "C", mirrored false, verts, edges as drafted)`.
- **`cut.on_fold = F`** (truthy): let `f` = edge F at loop index `i`, `A = f.from`, `B = f.to`.
  1. `rest = edges[i+1 ..] ++ edges[.. i]`: the loop without F, running B → … → A.
  2. Right half: each edge of `rest` with id `→ id + ".R"` (from, to, curve unchanged).
  3. Left half: `mirrored_loop(rest, mv)` with ids `→ id + ".L"`, where `mv(v) = v` if `v ∈ {A, B}`
     else `v + ".L"`, and `mirrored_loop(E, vmap)` = for each `e` in **reverse(E)**: a copy of `e` with
     `from = vmap(e.to)`, `to = vmap(e.from)`, `curve = mirror_curve(e.curve)`.
  4. Vertices: all original vertices, plus for **every** vertex `v ∉ {A, B}` (even ones no edge uses)
     a twin `v + ".L"` = reflection of `p_v` across the line A–B:
     `u = (p_B − p_A)/‖p_B − p_A‖`, `w = p − p_A`, `p' = p_A + 2(w·u)u − w`.
  5. Base = `Instance(pid, "C", false, verts', right ++ left)`. The fold edge disappears; the loop is
     B → … → A → (mirrored) → B, CCW, twice the drafted area.

Step 2, quantity (`qty = int(cut.qty)`, default 1):

- `qty == 2`: `[R, L]` with `R = Instance(base.id + ".R", side "R", mirrored false, base verts, base edges)`
  and `L = Instance(base.id + ".L", side "L", mirrored true, verts (x, y) → (−x, y), edges = mirrored_loop(base.edges, identity))`.
  The mirror is about the **y-axis (x = 0)**, not about the piece. Edge ids (including `.R`/`.L`
  half suffixes) are unchanged in the `.L` instance, so `side.R` exists inside `back.L` as the mirror
  image of `back.R`'s `side.R`.
- `qty == 1`: `[base]`.
- otherwise: error `"{pid}: cut.qty must be 1 or 2 (use separate pieces for more)"`.

| `cut` | Instances | Edge ids |
|---|---|---|
| `{"qty": 1}` | `front` | as drafted |
| `{"qty": 2}` | `front.R` (as drafted), `front.L` (mirrored, still CCW, loop reversed) | as drafted on both |
| `{"qty": 1, "on_fold": "cb"}` | `back` (unfolded) | `side.R`, `side.L`, …; `cb` is gone |
| `{"qty": 2, "on_fold": "cb"}` | `back.R`, `back.L`, each unfolded | `side.R` inside `back.L`, etc. |

Instance iteration order (everywhere: structure checks, role grouping) = piece order, and within a
piece `[R, L]` or `[base]`.

---

## 7. Pattern construction and `structure_errors()`

### 7.1 Construction: `Pattern(data)`

1. `seams = data.seams` or `[]`.
2. For each piece `p` in order (`pid = p.id`; absent → crash):
   a. `pid` already seen → build error `"duplicate piece id '{pid}'"` (the first piece keeps the id).
   b. Append the piece-definition errors (§7.2). Then the would-be instance ids
      (`[pid.R, pid.L]` if `int(cut.qty) == 2`, else `[pid]`): any id already seen →
      `"duplicate instance id '{iid}'"`. An exception while doing (b) (missing key, bad qty literal) →
      `"{pid}: cannot validate piece definition: {exc}"`.
3. If there is any build error, **stop: no instances are built** (no partial geometry of an ambiguous
   pattern; tests assert instance construction is never attempted).
4. Otherwise build each piece's instances (§6.3) in order; an id already present →
   `"duplicate instance id '{id}'"` (skipped); an exception → `"{pid}: cannot build instances: {exc}"`.

### 7.2 Piece-definition checks, in order

| # | Condition | Message (verbatim template) |
|---|---|---|
| 1 | Any vertex id, edge id or the piece id contains `.`, `/`, `+` or a space | `{pid}: id '{name}' must not contain '.', '/', '+' or spaces` |
| 2a | Per edge, `from` / `to` not a vertex id (checked from then to) | `{pid}/{edge_id}: unknown vertex '{v}'` |
| 2b | Per edge, curve type (default `line`) not in line/quad/cubic/poly | `{pid}/{edge_id}: curve type '{type}'` |
| 3 | Per consecutive pair (cyclic), `e.to ≠ next.from` | `{pid}: edge {e_id} ends at {e_to} but {next_id} starts at {next_from}` (no quotes) |
| 4 | Edge ids not unique | `{pid}: duplicate edge ids` |
| 5 | `int(cut.qty)` ∉ {1, 2} | `{pid}: cut.qty must be 1 or 2 (use separate pieces for more)` |
| 6 | `on_fold` truthy and not an edge id | `{pid}: on_fold edge '{fold}' missing` |
| 7a | Fold edge curve type ≠ `line` | `{pid}: fold edge '{fold}' must be straight` |
| 7b | Fold edge endpoints both known and (a coordinate is not finite, or `‖b − a‖` is not > 0) | `{pid}: fold edge '{fold}' must have finite, distinct endpoints` |

Checks 2a and 2b interleave per edge (2a from, 2a to, 2b; next edge). Check 1 iterates vertex ids in
the reference's **set** order (hash-dependent, so not deterministic across runs), then edge ids in loop
order, then the piece id: compare check-1 messages as a multiset. `'{x}'` is Python `repr` (§13).

### 7.3 `structure_errors()` algorithm

1. `schema ≠ "fabric-pattern/1"` → `schema must be 'fabric-pattern/1'`.
2. `units` (default `"mm"`) ≠ `"mm"` → `units must be mm`.
3. For each piece: the §7.2 errors (recomputed), or `"{pid}: cannot validate piece definition: {exc}"`.
4. Every seam-id value occurring more than once (missing ids count as `None`), in ascending order:
   `duplicate seam id '{sid}'` (`duplicate seam id None` for missing ids).
5. Append each build error (§7.1) not already in the list built so far (this is how
   `duplicate piece id`, `duplicate instance id` and `cannot build instances` surface).
6. **If the list is non-empty, return it.**
7. For each instance: `a = area()`; `a ≤ 0` →
   `{iid}: edge loop is clockwise or degenerate (area {a:.0f} mm²); loops must be CCW`.
8. For each seam `s`, for `k` in `a`, `b`: resolve `side(s[k])` (§8.1); on error →
   `seam {s.id}: {exc}` and continue. For each edge `e` of the side: if `(inst.id, e)` was already used
   → `seam {s.id}: edge {inst.id}/{e} already used by seam {other_id}`; then record it as used by `s`
   (the same edge on both sides of one seam is also caught).
9. Return the list (steps 7 and 8 both run; there is no early return between them).

Not checked (documented elsewhere but not enforced): the `.L` twin rule, notch edge existence or `s`
range, seam `ratio`/`ratio_band` shape, grain shape, placement, roles, the `:` character.

---

## 8. Seam sides, notches and seam sampling

### 8.1 `side(ref) → (instance, [edge ids])`

1. Split `ref` at the first `/` into `inst_id`, `path` (no `/` → error; the reference message is
   Python's `not enough values to unpack (expected 2, got 1)`).
2. Unknown instance → KeyError `seam ref '{ref}': unknown instance '{inst_id}' (have [sorted instance ids])`.
3. `eids = path.split("+")`; each must exist (`Instance.edge` KeyError, §6.2).
4. Each consecutive pair must satisfy `edge(a).to == edge(b).from`, else ValueError
   `seam ref '{ref}': edges {a} and {b} are not consecutive in loop order`.

### 8.2 `side_polyline(ref, dense=200)` and `side_length(ref)`

Concatenate `edge_dense(e, 200)` for each edge of the path, dropping the first point of every edge
after the first. Edge lengths `len_k` = polyline length of each edge's samples; junctions = the
internal edge boundaries as arclength fractions, `cumsum(len)[:-1] / total` (empty if total is 0).
`side_length(ref)` = polyline length of the concatenation (= Σ len_k).

### 8.3 `notch_fractions(ref)`

Notches come from the piece definition of the side's instance. With `len_k = edge_length(eids[k])` and
`T = Σ len_k`, for each notch `n` and each path edge `e_k` whose stem (`e_k` minus a trailing `.R`/`.L`)
equals `n.edge`:

- `s = n.s`; if (`e_k` ends with `.L`) **xor** `instance.mirrored` → `s = 1 − s` (a `.L` half or a
  mirrored instance runs reversed; a `.L` half inside a mirrored instance does not).
- `f = (Σ_{j<k} len_j + s·len_k) / T`; keep `f` only if `1e-4 < f < 1 − 1e-4`.

Return the kept fractions sorted ascending. `T = 0` with a matching notch divides by zero in the
reference (§15).

### 8.4 `seam_pairs(seam, spacing)`

Matched sample points on both sides, piecewise-proportional in arclength between matched notches.
`seam_walk` uses `spacing = 5.0`; the sim passes its own spacing.

1. `(A, a_j) = side_polyline(seam.a)`, `(B, b_j) = side_polyline(seam.b)`.
2. If `reverse` (default true): `B = reverse(B)`, `b_j = reverse([1 − j for j in b_j])`.
3. `na = notch_fractions(a)`, `nb = notch_fractions(b)`; if `reverse`: `nb = sorted(1 − f)`.
4. Knots: if `na` is non-empty and `|na| == |nb|`: `ka = [0, *na, 1]`, `kb = [0, *nb, 1]`,
   `notch_match = "piecewise"`. Else `ka = kb = [0, 1]` and `notch_match = "none"` if both are empty,
   otherwise `"count-mismatch {|na|}:{|nb|}"`.
5. `to_b(u) = interp(u, ka, kb)`, `to_a(u) = interp(u, kb, ka)` (piecewise linear).
6. Breakpoints `brk` = sorted unique of `round(x, 9)` over `ka ∪ a_j ∪ {to_a(j) : j ∈ b_j}`.
7. `la = len(A)`, `lb = len(B)`; unless `0 < la < 1e5` and `0 < lb < 1e5` → ValueError
   `seam {id}: implausible side lengths {la:.0f} / {lb:.0f} mm`.
8. `fa = [0]`, `fb = [0]`; for each consecutive `(u0, u1)` in `brk`:
   `n = max(1, ceil(max((u1 − u0)·la, (to_b(u1) − to_b(u0))·lb) / spacing))`;
   append `u0 + (u1 − u0)·m/n` for `m = 1..n` to `fa` and their `to_b` images to `fb`.
9. Return `{a: at_fractions(A, fa), b: at_fractions(B, fb), fa, fb, len_a: la, len_b: lb,
   a_junctions: a_j, b_junctions: b_j (after step 2), notches_a: na, notches_b: nb (after step 3),
   notch_match}`.

---

## 9. Validation (`validate.py`)

### 9.1 Seam walk: `seam_walk(pattern, tol_mm = 3.0)`

For each seam, in order:

1. `pr = seam_pairs(s, 5.0)`. A KeyError or ValueError → row
   `{seam: s.id (null if absent), status: "ERROR", message: str(exc)}` (KeyError text is wrapped in
   quotes, §13).
2. `la, lb`; `ratio = la/lb` (∞ if `lb = 0`, unreachable after step 7 of §8.4).
3. `band = s.ratio_band` if that key exists (even if null), else `s.band`.
   `nominal = s.ratio` if present, else `1.0` when `band` is falsy, else none.
4. Residual and target:
   - `band` truthy, `[lo, hi] = band`: `resid = 0` if `lo ≤ ratio ≤ hi`; else
     `resid = la − edge·lb` with `edge = lo` if `ratio < lo` else `hi`. `target = "{lo:.3f}–{hi:.3f}"`
     (en dash U+2013).
   - otherwise: `resid = la − nominal·lb`, `target = "{nominal:.3f}"`.
5. `tol = s.tol_mm` if present, else the walk's `tol_mm` (fit spec `seam_tol_mm`, default 3.0).
6. Row (key order): `seam`, `a`, `b`, `len_a_mm = round(la, 1)`, `len_b_mm = round(lb, 1)`,
   `ratio = round(ratio, 4)`, `target_ratio = target`, `residual_mm = round(resid, 1)`,
   `status = PASS if |resid| ≤ tol else FAIL` (unrounded resid), `notch_match`.
7. If `notch_match == "piecewise"`: `r = nominal` if truthy else the measured `ratio`;
   `segs[i] = round((ka[i+1] − ka[i])·la − r·(kb[i+1] − kb[i])·lb, 1)` for each notch interval;
   `notch_segments_mm = segs`. If any `|seg| > max(tol, 0.08·la/len(segs))` (rounded segs) →
   `status = FAIL`, `message = "ease is not where the notches say it is"`.
8. Else if `notch_match` starts with `count-mismatch`: `status = WARN` if it was PASS (FAIL stays);
   `message = "notches don't pair ({notch_match}); ease distribution is unchecked"` (set even when FAIL).
9. If `status == FAIL` and no message yet: with `longer = a` if `resid > 0` else `b`:
   - no band: `{s[longer]} is {|resid|:.1f} mm too long for the declared ratio {target}`
   - band: `ratio {ratio:.3f} is outside {target} by {|resid|:.1f} mm`

`WARN` does not count as failing anywhere.

### 9.2 Truing: `truing(pattern, spec)`

Garment lines that cross a seam (necklines, armholes, hems, caps) must run straight through: the
corner angles at matching seam ends sum to ~180°.

1. `tol = spec.truing_tol_deg` (8.0). `sewn` = every `(instance id, edge id)` on any seam side.
   `explicit` = set of `(t.seam, t.end)` from `spec.truing`.
2. For each seam `s` (sides resolved with §8.1; errors propagate, so call only after structure
   checks pass), `rev = s.reverse` (true), for `end` in `start`, `end`:
   - `va` = side a's first edge `from` (start) or last edge `to` (end).
   - `b_end` = the opposite end if `rev`, else the same end; `vb` = side b's vertex at `b_end`.
   - `ca = continuation(inst_a, va, path_a)`, `cb = continuation(inst_b, vb, path_b)`: the first edge in
     loop order that is not in the path and has `from == v` or `to == v`; none → skip this end.
   - `free` = neither `(inst_a, ca)` nor `(inst_b, cb)` is in `sewn`. If not `free` and
     `(s.id, end) ∉ explicit` → skip.
   - `ang = corner_angle(inst_a, va) + corner_angle(inst_b, vb)`, `dev = ang − 180`.
   - Row (key order): `seam`, `end`, `at = "{inst_a}:{va} + {inst_b}:{vb}"`, `line = "{ca} / {cb}"`,
     `angle_sum_deg = round(ang, 1)`, `deviation_deg = round(dev, 1)`,
     `status = PASS if |dev| ≤ tol else FAIL`, `message = ""` or
     `the {ca}/{cb} line {dips|peaks} by {|dev|:.0f}° at this seam; true the corner so the joined line runs straight through`
     (`dips` when `dev > 0`, `peaks` otherwise).

### 9.3 Points of measure: `measure_point(pattern, point, meas)`

Landmark line of an instance: `level_y(inst, L) = h(part_key, L) − h(part_key, y0_landmark) + y0`
with `part = placement.part` (`torso`), `part_key = "torso"` or `"{part}_R"`, `y0_landmark`
(`"waist"`), `y0` (0) from the instance's piece placement (missing placement = all defaults).

| Kind | Finished value `val` |
|---|---|
| `girth` | `Σ_{iid ∈ instances} horizontal_width(outline_iid(120), level_y(iid, landmark) + dy)` |
| `width` | `y = verts[at_vertex].y` if `at_vertex` is given, else `level_y(instance, landmark)`; `val = horizontal_width(outline(120), y + dy)` |
| `vertical` | `|verts[from].y − verts[to].y|` |
| `path` | `scale · Σ_{ref ∈ refs} side_length(ref)` |
| other | ValueError `unknown point kind '{kind}'` |

Then `bval = mval(meas, body)·body_scale + body_offset_mm`, `ease = val − bval`,
`[lo, nom, hi] = target_mm`:

- `status = IN_BAND` if `lo ≤ ease ≤ hi`, `LOW` if `ease < lo`, else `HIGH`.
- `residual = 0` (IN_BAND), `ease − lo` (LOW), `ease − hi` (HIGH).
- `sigma = round(meas.fused[body].sigma_mm (else .sigma_deg, else 0.0), 1)`; null if the file has no
  `fused` entry for that name (flat measurement dicts always give null).
- Row (key order): `id`, `label` (default id), `authority: "ANALYTIC"`, `finished_mm = round(val, 1)`,
  `body_mm = round(bval, 1)`, `body_sigma_mm = sigma`, `ease_mm = round(ease, 1)`,
  `target_mm` (as given), `residual_mm = round(residual, 1)`, `status`.
- If `sigma` is truthy and `sigma > (hi − lo)/2`: add `note` =
  `body {body} is only known to ±{sigma:.0f} mm, wider than half the target band; a tape measurement would decide this point`.

Any KeyError/ValueError while measuring (unknown instance, vertex, landmark, kind, missing body
measurement, wrong `target_mm` arity) becomes an `UNMEASURABLE` row, never a crash (key order): `id`,
`label`, `authority: "ANALYTIC"`, `finished_mm: null`, `body_mm: null`, `ease_mm: null`, `target_mm`,
`residual_mm: null`, `status: "UNMEASURABLE"`,
`message: "{label or id}: cannot measure on this pattern ({exc}); rename the vertex/edge or adapt the fit spec"`.
There is no `body_sigma_mm` key on these rows.

### 9.4 Report assembly and status: `validate(pattern, spec, meas)`

1. `structure_errors()` non-empty → return exactly `{status: "BLOCKED", structure_errors: [...]}`.
2. `seams = seam_walk(pattern, spec.seam_tol_mm (3.0))`; `truing = truing(pattern, spec)`;
   `points` = one row per `spec.points` entry (§9.3).
3. `missing_measurements` = for each distinct `body` name over all points, sorted ascending: names for
   which `mval` raises KeyError.
4. `completeness`, per piece in first-definition order: `"{pid}: no grainline"` if `grain` is falsy;
   `"{pid}: no cut instruction"` if `cut` is absent or empty.
5. `fails` = seam rows with status FAIL or ERROR (seam order), then truing FAIL rows, then point rows
   with status ≠ IN_BAND (LOW, HIGH, UNMEASURABLE).
6. Status:

| Condition (first match) | Status | CLI exit |
|---|---|---|
| structure errors | `BLOCKED` | 3 |
| `missing_measurements` non-empty (everything else is still computed) | `BLOCKED` | 3 |
| no `fails` and no `completeness` | `CONVERGED` | 0 |
| otherwise | `NOT_CONVERGED` | 2 |

7. `worst`: stable-sort `fails` by `−|key|` where `key = residual_mm` if the row has it, else
   `deviation_deg`, else 0, with null treated as 0; take the first 6; each becomes `row.message` if
   non-empty, else `"{label (or seam)}: {status} (residual {residual_mm or deviation_deg})"` with the
   number printed as Python `str(float)` (e.g. `Chest ease (fronts edge to edge): LOW (residual -87.6)`).
8. Return (key order): `status`, `missing_measurements`, `completeness`, `seams`, `truing`, `points`,
   `summary {seams_failing (FAIL + ERROR), truing_failing (FAIL), points_out_of_band (≠ IN_BAND)}`,
   `worst`.

The report is persisted per round as `validation.json` and read by the report page and the browser UI
(`points[].label/body_mm/finished_mm/ease_mm/target_mm/status`, `seams[].seam/len_a_mm/len_b_mm/ratio/target_ratio/residual_mm/status`,
`truing[].seam/end/angle_sum_deg/message/status`): these field names are an on-disk format.

### 9.5 Role-based default fit spec: `default_fit_spec(pattern, garment = "generic")`

For patterns without a drafting program (traced sheets, scans). Group instances by `piece.role`
(`""` when absent), in instance order. Emit a point only when its role/edges exist:

| Point | Condition | Definition |
|---|---|---|
| `chest` | any `bodice_front` or `bodice_back` instance | girth over [fronts…, backs…], landmark `chest`, `dy: -24.0`, body `chest`, `[70, 100, 130]`, label `Chest ease (pieces edge to edge)` |
| `across_back` | any `bodice_back` | girth over backs, landmark `across_back`, body `across_back`, `[10, 25, 45]`, `Across-back ease` |
| `neckline` | any bodice edge whose id before the first `.` is `neck` | path, refs `"{inst}/{edge}"` for each such edge (bodice instances in order, edges in loop order), body `neck_base`, `[20, 50, 90]`, `Neckline ease` |
| `cuff_pass` | first `sleeve` instance has an edge `hem` | path `["{sleeve}/hem"]`, body `hand_pass`, `[10, 25, 60]`, `Sleeve hem passes the hand` |
| `bicep` | first `sleeve` instance has an edge `cap_f` | width, instance = that sleeve, `at_vertex` = `cap_f.from`, body `upper_arm`, `[35, 50, 70]`, `Bicep ease` |
| `seat` | any `pant_front` or `pant_back` | girth over [pant fronts…, pant backs…], landmark `hip`, body `hip`, `[60, 160, 480]`, `Seat ease` |

Output order: chest, across_back, neckline, cuff_pass, bicep, seat. Returned document (key order):
`{schema: "fabric-fit/1", garment, generated: "validate.default_fit_spec (role-based)", seam_tol_mm: 3.0, truing_tol_deg: 8.0, points}`
(no `truing` key). Bands are outerwear starting values; see §17 for where they differ from the
drafted specs.

### 9.6 Text table: `format_table(report)`

Lines joined with `\n`, no trailing newline. Widths are in characters; `s:N` = left-aligned padded to N
(never truncated unless sliced), `>N` = right-aligned.

1. `STATUS: {status}`
2. If `structure_errors`: one line per error `  structure: {e}`, and stop.
3. `Finished measurements (authority: flat pattern)`
4. Header: `"  " + "point":28 + " " + "body">8 + " " + "finished">9 + " " + "ease">7 + "  target(min/nom/max)  status"`, i.e.
   `  point                            body  finished    ease  target(min/nom/max)  status`
5. Per point: `"  " + label[:28]:28 + " " + F(body_mm,8) + " " + F(finished_mm,9) + " " + F(ease_mm,7) + "  " + T:19 + "  " + status`,
   where `F(v, w)` = `v` with 1 decimal right-aligned in w, or `—` (U+2014) right-aligned in w when
   null, and `T` = the three targets truncated toward zero to integers and joined with `/`.
6. `Seam walk`
7. Per seam: ERROR rows → `"  " + seam:22 + " ERROR " + message`; others →
   `"  " + seam:22 + " a=" + len_a_mm:7.1f + " b=" + len_b_mm:7.1f + " ratio=" + ratio:.3f + " target=" + target_ratio:11 + " resid=" + residual_mm:6.1f + "  " + status`.
8. If any truing rows: `Truing (angle sums across seams)`, then per row
   `"  " + seam:22 + " " + end:5 + " " + line[:30]:30 + " " + angle_sum_deg:6.1f + "°  " + status`.
9. Per completeness item: `  completeness: {c}`.
10. If `worst` is non-empty: `Worst:` then `  - {w}` per item.

`missing_measurements` is **not** printed (the affected points show as UNMEASURABLE). Negative values
that round to zero print as `-0.0` (Python). Example in §14.4.

### 9.7 Validate CLI

`validate PATTERN.json --fit FIT.json --measurements MEAS.json [--out report.json] [--json]`. Both
`--fit` and `--measurements` are required. Writes the report JSON to `--out` (indent 1 + newline) when
given; prints the JSON (`--json`) or the table; exits 0 / 2 / 3 for CONVERGED / NOT_CONVERGED /
BLOCKED. A usage error also exits 2 in the reference (argparse), colliding with NOT_CONVERGED (§17).

---

## 10. Alteration ops (`alter.py`)

Ops edit vertices or relative curve parameters only, never resample geometry, and append one sewist
sentence to `meta.history`. Relative curves follow moved vertices, so curve character survives size
changes. Each op returns a **new** document (deep copy); the input is not modified. Ops do not run
structure checks themselves (the CLI does, after writing).

### 10.1 Shared helpers

- `history(doc, op, args, why)`: `meta` is created if absent; `meta.history` is created if absent and
  `{op, args, why}` appended; `meta.origin = "VECTOR_EDIT"` (inserted after `history` if new).
  `meta.source` is never touched.
- `piece(doc, pid)`: first piece with that id, else KeyError `no piece '{pid}'`.
- `shift_marks(piece, fn)`: if the piece has a grain (either form) it becomes
  `[fn(p0), fn(p1)]` (always the pair form afterwards); every item of `internal`, `decor` and
  `trace_marks` has each point of `pts` replaced by `fn(point)`. Notches (fractions), drills and
  placement are **not** changed.
- `bisect(f, lo, hi, it = 60)`: `flo = f(lo)`; 60 times: `mid = (lo + hi)/2`; if
  `(f(mid) > 0) == (flo > 0)` then `lo = mid, flo = f(mid)` else `hi = mid`; return `(lo + hi)/2`.
  No bracket check: without a sign change it converges to `hi` silently (§17).

### 10.2 `slash-spread(piece, axis, at, add)`

Cut the drafted piece along `x = at` (axis `x`: adds width) or `y = at` (any other axis value: adds
length) and spread by `add` mm.

- `k = 0` for `x`, else 1. `fn(q)`: if `q[k] > at` (strict) then `q[k] += add`; return
  `[round(q.x, 3), round(q.y, 3)]` (every point is rounded, moved or not; outputs are floats).
- Applied to all vertices and, via `shift_marks`, to grain, internal, decor, trace_marks.
- History: op `slash-spread`, args `{piece, axis, at, add}`, why
  `Slashed {piece} at {axis}={at:.0f} mm and spread {add:+.1f} mm to {add|remove} {width|length} there.`
  (`add` iff `add > 0`; `width` iff axis is `x`).
- An edge crossing the cut has its endpoints separated; its relative curve stretches with the chord
  (an affine stretch, not a true spread).

### 10.3 `move(piece, vertex, dx, dy)`

`vertices[vertex] = [round(x + dx, 3), round(y + dy, 3)]` (missing vertex → KeyError). Marks are not
moved. History: op `move`, args `{piece, vertex, dx, dy}`, why
`Moved {piece}:{vertex} by ({dx:+.1f}, {dy:+.1f}) mm.`

### 10.4 `true-seam(seam, end = "end")`

Make side b match side a under the declared ratio by sliding one vertex of side b along b's end
direction.

1. Build the pattern (no structure check). Find the seam by id (missing → reference crashes, §15).
2. `ratio` = **mean of the band** if `ratio_band` (or legacy `band`) is truthy, else `s.ratio` (1.0).
   A declared `ratio` is ignored when a band exists.
3. `(inst, eids) = side(s.b)`. If `inst.mirrored`, or `inst.side == "L"`, or any edge id ends with
   `.L` → ValueError `true the drafted (.R) side; the mirrored copy follows`.
4. `eid` = last edge (`end`) or first edge (`start`); `vid` = that edge's `to` (end) or `from` (start);
   `pid = inst.piece`; `base` = the vertex's **drafted** coordinates.
5. Direction `t`: from `edge_dense(eid, 200)`, `t = P[-1] − P[-2]` (end) or `P[0] − P[1]` (start),
   normalised; it points beyond the vertex, so moving by `+d` lengthens side b.
6. `target = side_length(s.a) / ratio`; `resid(d)` = side_length(s.b) of the pattern with the drafted
   vertex at `base + d·t` (unrounded) minus `target`; `d = bisect(resid, −60, 60)`.
7. Output vertex = `[round(base.x + d·t.x, 3), round(base.y + d·t.y, 3)]`. Every edge meeting the
   vertex, its mirrored/unfolded copies and any other seam using them move too.
8. History: op `true-seam`, args `{seam, end, moved_mm: round(d, 2)}`, why
   `Trued seam {seam}: moved {pid}:{vid} {d:+.1f} mm along the seam so side b is {target:.1f} mm (side a / {ratio:.3f}).`

### 10.5 `cap-ease(piece, seams, target)`

Scale the cap height (everything above the underarm line) so that **total** cap length / total
armhole length = `target`.

1. `ss` = the pattern's seams (pattern order) whose id is in `seams` (argument order irrelevant).
2. For each `s ∈ ss`: the cap ref is `s.a` if the part of `s.a` before `/`, cut at its first `.`,
   equals `piece`, else `s.b`; the armhole ref is the other side.
3. `inst` = the instance of the first cap ref (no match → reference crashes, §15). `y_ua` = minimum
   y over the `from` and `to` vertices of every edge of every cap ref (coordinates looked up in `inst`).
4. `arm = Σ side_length(armhole refs)` (fixed).
5. `scaled(k)`: copy of the document where each vertex of the drafted piece with `y > y_ua` (strict)
   becomes `[x, y_ua + k·(y − y_ua)]` (no rounding).
6. `k = bisect(k ↦ Σ side_length_{scaled(k)}(cap refs)/arm − target, 0.3, 3.0)`.
7. Output = `scaled(k)`, then `shift_marks` with the same map (`q.y > y_ua` → scaled, else unchanged).
8. `old = Σ side_length(cap refs)/arm − 1` on the input. History: op `cap-ease`, args
   `{piece, seams, target, height_scale: round(k, 4)}`, why
   `Scaled the {piece} cap height by {k:.3f} so cap ease goes from {old·100:.1f}% to {(target − 1)·100:.1f}% of the armhole.`

The two cap halves are not kept equal (§17).

### 10.6 `transfer-curve(src_doc, src_ref, dst_ref)`

Copy an edge's curve **character** (relative to its chord) from another pattern.

1. Build the source pattern; `src_ref = "INSTANCE/EDGE"` (exactly one `/`; an **instance** id).
2. `P = curve_points(src from, src to, src curve, 60)`; `p0 = P[0]`, `p1 = P[-1]`.
3. `rel = [abs_to_rel(p0, p1, P[i]) for i = 1, 4, 7, …, ≤ len(P) − 2]` (every 3rd interior sample:
   20 points for a quad/cubic source; none for a line; every 3rd control point of a poly source).
4. `dst_ref = "PIECE/EDGE"` (a **piece** id, not an instance id); the destination edge's `curve`
   becomes `{"type": "poly", "c": [[round(t, 5), round(h, 5)] …]}` (missing edge → reference crashes).
5. History: op `transfer-curve`, args `{from: src_ref, to: dst_ref}`, why
   `Gave {dst_ref} the curve character of {src_ref} (shape copied relative to the edge, size kept).`

Endpoints are kept; because `h` is relative to the chord, the curve's depth scales with the
destination chord.

### 10.7 Alter CLI

| Subcommand | Arguments |
|---|---|
| `slash-spread PATTERN` | `--piece` (req), `--axis x\|y` (req), `--at` float (req), `--add` float (req) |
| `move PATTERN` | `--piece` (req), `--vertex` (req), `--dx` float (0), `--dy` float (0) |
| `true-seam PATTERN` | `--seam` (req), `--end start\|end` (end) |
| `cap-ease PATTERN` | `--piece` (req), `--seams` comma-separated ids (req), `--target` float (req) |
| `transfer-curve PATTERN` | `--from SRC.json:instance/edge` (split at the **last** `:`), `--to piece/edge` |

Every subcommand requires `--out`. The CLI runs the op, computes `structure_errors()` on the result,
writes the result (indent 1 + newline) **even if there are errors**, prints the last history sentence,
then either prints `structure errors:` followed by each error on its own line indented 2 spaces and
exits 1, or exits 0.

---

## 11. `PieceBuilder` (used by drafting programs)

| Call | Effect |
|---|---|
| `PieceBuilder(pid, name, **meta)` | `{"id", "name", "vertices": {}, "edges": [], "notches": [], **meta}` (meta keys like `cut`, `role`, `grain`, `material`, `sim`, `sim_scale` follow, in call order). |
| `v(vid, x, y)` | `vertices[vid] = [round(x, 3), round(y, 3)]`; returns `vid`. |
| `e(eid, a, b, curve = None, **meta)` | Appends `{"id", "from", "to", **meta}` plus `"curve"` if curve is truthy. |
| `cubic_abs(eid, a, b, c1, c2, **meta)` | Edge with `{"type": "cubic", "c": [r(abs_to_rel(pa, pb, c1)), r(abs_to_rel(pa, pb, c2))]}` where `pa`, `pb` are the already-rounded stored vertices and `r` rounds each parameter to 5 decimals. |
| `quad_abs(eid, a, b, c1, **meta)` | Same with one control point and `"quad"`. |
| `notch(nid, edge, s, kind = "V")` | Appends `{"id", "edge", "s": round(s, 4), "type": kind}`. |
| `build()` | Returns the piece object. |

---

## 12. External interfaces

The validator needs a body model; a port SHOULD put it behind a trait so §14 fixtures can stub it.

| Function | Contract |
|---|---|
| `mval(meas, name) → f64` | If `meas` has `fused`: `fused[name].value_mm`, else `fused[name].value_deg`. Otherwise `meas[name]` (a flat name → value object). Missing name → "missing measurement" (KeyError in the reference); missing value inside a present entry crashes the reference (§15). |
| `sigma(meas, name) → f64?` | `fused[name].sigma_mm`, else `.sigma_deg`, else 0.0, rounded to 1 decimal; null when there is no `fused` or no entry. |
| `landmark_h(meas, part_key, name) → f64` | Landmark height in a part frame (body spec). `torso`: absolute height (mm above the floor) of any torso landmark (`floor`, `ankle`, `calf`, `knee`, `crotch`, `hip`, `waist`, `chest`, `across_back`, `shoulder`, `snp`, `c7`, `head_top`, …). `arm_R`: only `shoulder_tip`, `elbow`, `wrist` (anything else → unknown landmark → UNMEASURABLE; note the `waist` default `y0_landmark` is invalid for arms). `leg_R`: distance along the leg axis, `(z(name) − z_origin)/cos(leg spread)` for any torso landmark. |

Measurement names used by the specs here: `chest`, `across_back`, `upper_arm`, `arm_length`,
`hand_pass`, `neck_base`, `back_neck_to_waist`, `hip`, `seat_over_nappy`, `total_crotch_length`,
`waist`, `foot_pass`, `calf`, `waist_height`, `head` (catalogue owned by the measurements spec).

---

## 13. Python semantics a port must reproduce

| Topic | Reference behaviour | Port requirement / test vector |
|---|---|---|
| `round(x, n)` | Correctly rounded on the exact binary value, ties to even. | `round(0.125, 2) = 0.12`, `round(2.675, 2) = 2.67`, `round(1.0005, 3) = 1.0`, `round(-0.0004, 3) = -0.0`, `round(5e-05, 4) = 0.0001`. |
| Fixed-point formatting (`.1f`, `.3f`, `.0f`, `+.1f`) | Same rounding as `round`; keeps `-` on negative values that round to zero. | `{-0.04:6.1f}` = `  -0.0`; `{1.25:+.1f}` = `+1.2`; `{0.5:.0f}` = `0`; `{2.5:.0f}` = `2`; `{-0.4:.0f}` = `-0`. |
| `str(float)` in `worst` messages | Shortest round-trip repr, always with a decimal point for |x| < 1e16. | `-87.6`, `12.0`, `0.0`. |
| `'{x}'` in messages | Python `repr` of a string: single quotes, double quotes if the string contains a single quote and no double quote. Lists print as `['a', 'b']`. | — |
| KeyError text | `str(KeyError(m))` = `repr(m)`, so messages from KeyErrors are wrapped in quotes: `seam s: "seam ref 'missing/right': unknown instance 'missing' (have ['a', 'b'])"`; a missing vertex reads `('nope')` inside UNMEASURABLE messages. | Reproduce for golden-text tests, or compare such messages by substring (the reference tests do). |
| `int()` on `cut.qty` | Truncates floats (`2.7 → 2`), parses numeric strings. | SHOULD accept only integers 1 and 2 and treat other numbers as a structure error; record the deviation (§15). |
| Ordering | Dicts keep insertion order; `sorted` is stable. | Use ordered maps; stable sort for `worst`. |
| numpy `interp` | Clamps outside the abscissa range; repeated abscissae return the last sample. On the reference's arm64 build `slope·(x − xp[j]) + fp[j]` is one fused multiply-add (clang contracts it). | `interp(1.0, [0,1,1,2], [0,5,7,9]) = 7`; compute the interpolation with a correctly rounded FMA to match the reference bit for bit. |
| numpy `linspace(0, 1, n)` | Endpoints exact. | `t_j = j/(n−1)`, last forced to 1. |
| Least squares (`lstsq`, `rcond=None`) | Minimum-norm solution for rank-deficient systems. | Use a QR/SVD least-squares solve; only the linear coefficient is used. |
| JSON | `ensure_ascii`, `indent=1`; floats as shortest repr (`1e-05`, `1e+16`). | Semantic equality is the bar (§14.6). |

---

## 14. Conformance oracle

### 14.1 Which tests pin which behaviour

All in `scripts/tests/test_core.py` unless noted.

| Test | Pins |
|---|---|
| `test_rel_abs_roundtrip` | §4.1 round trip. |
| `test_qty2_mirror_instance_is_congruent_and_ccw` | §6.3 qty 2: `.L` mirrored, both CCW, equal edge lengths (cubic edge included). |
| `test_on_fold_unfold_doubles_area_and_names_halves` | §6.3 fold: area doubles; edge ids `{top,bottom,right}.{R,L}`, fold edge gone. |
| `test_structure_errors_catch_common_mistakes` | §7.2 #1, §7.3 step 7 (clockwise), #6, step 4 (duplicate seam id). |
| `test_single_edge_seam_checks_the_instance_and_edge` | §8.1 single-edge refs; unknown instance / unknown edge messages; side length; seam walk PASS. |
| `test_duplicate_piece_ids_are_reported_without_overwriting_input` | §7.1 2a, first piece kept, input untouched, no instances. |
| `test_generated_instance_identity_collision_is_reported_before_construction` | §7.1 2b: `panel` qty 2 + piece `panel.R` → `duplicate instance id 'panel.R'`, no construction attempted. |
| `test_piece_semantics_are_checked_before_geometry` (4 cases) | §7.2 #4, #2a, #7a, #7b, with no instance construction attempted and input unchanged. |
| `test_valid_mirrored_folded_pieces_keep_single_edge_seams` | qty 2 + fold instances, areas 4800, seams on `.R`/`.L` halves PASS. |
| `test_seam_pairs_proportional_and_notch_piecewise` | §8.4 piecewise notch mapping (a notch 0.25 ↔ b notch 0.5). |
| `test_exact_tangent_for_cubic_and_horizontal_width` | §6.2 corner angle 90° on a rectangle; §5 horizontal width 100. |
| `test_drafts_converge_on_reference_wearer` | §9.4 CONVERGED for bolero and trousers (integration). |
| `test_trouser_leg_length_is_measured` | `vertical` + `body_offset_mm`: ease 40.0 ± 0.5; hem 150 mm lower → HIGH. |
| `test_elastic_must_be_shorter_than_the_waist` | `path` + `scale`: IN_BAND with negative ease; elastic 1.05 × waist → HIGH and not CONVERGED. |
| `test_bolero_cap_ease_equal_on_both_halves` | Seam ratios of both cap halves equal to 1.045 ± 0.002. |
| `test_mutation_side_seam_mismatch_is_caught` | §9.1 FAIL with |residual| > 9. |
| `test_mutation_chest_too_small_reads_low` | §9.3 LOW. |
| `test_mutation_truing_kink_is_caught` | §9.2 FAIL on `hem / hem`. |
| `test_missing_vertex_is_unmeasurable_not_a_crash` | §9.3 UNMEASURABLE. |
| `test_slash_spread_adds_exact_width` | §10.2 width 112 and history op. |
| `test_true_seam_closes_mismatch` | §10.4 residual < 0.5 mm after truing. |
| `test_cap_ease_hits_target` | §10.5 total ratio 1.06 ± 0.002. |
| `test_transfer_curve_keeps_endpoints_and_copies_shape` | §10.6 endpoints kept, length > chord. |
| `test_default_fit_spec_measures_a_traced_bolero` | §9.5 emits chest, across_back, neckline, cuff_pass, bicep; none UNMEASURABLE. |
| `test_qualification_band_has_exact_girth` | `horizontal_width` sums to chest + ease (±0.01); seam walk PASS (needs the fabric module's band pattern). |
| `test_fez_is_an_exact_cone_that_fits_the_head` | Fez CONVERGED; multi-edge `path` points; `edge_length` of cubic arcs; LOW on a 30 mm larger head. |
| `test_traced_sheet_flows_through_annotate_scale_and_export` | Grain is a point pair after the tracer; structure accepted by export. |
| `test_wrap_is_isometric_and_roll_ends_at_wrap` | Uses `outline(40)`, `at_fractions`, `polyline_length` (body spec). |
| `test_export.py` (several) | `signed_area` of cut rings, `piece_instances` of a folded rect, `_bezier` with 400 samples, `rel_to_abs`, `perimeter`, `structure_errors() == []` on fixtures. |
| `test_vectorize.py` (several) | `at_fractions`, `structure_errors() == []` on traced patterns, `area()` of traced pieces. |

### 14.2 Standalone fixtures (no body, measurement or drafting port needed)

Notation: `R(id, w, h, cut)` = piece `{"id": id, "name": ID-uppercased, "vertices": {a:[0,0], b:[w,0], c:[w,h], d:[0,h]}, "edges": [bottom a→b, right b→c, top c→d, left d→a], "notches": [], "cut": cut (default {"qty":1}), "grain": [[10,10],[10,40]]}`.
`D(pieces, seams)` = `{"schema": "fabric-pattern/1", "units": "mm", "pieces": …, "seams": …}`.
Floats are given to full precision where the port should match to ~1e-12; others to the shown digits.

| # | Input | Expected |
|---|---|---|
| F1 | `abs_to_rel((3,4), (40,-10), (20,15))`; then `rel_to_abs` back; `rel_to_abs((0,0),(100,0),[0.5,0.2])`; `abs_to_rel((1,1),(1,1),(2,2))` | `[0.3035143769968051, 0.41214057507987223]`; `(20, 15)`; `(50, 20)`; error `zero-length chord` |
| F2 | `curve_points((0,0),(100,0), c)` for: none; poly `[[0.25,0.1],[0.75,-0.1]]`; quad `[[0.5,0.5]]`; cubic `[[0.25,0.3],[0.75,0.3]]`; quad with 2 points; type `spline` | `[(0,0),(100,0)]`; `[(0,0),(25,10),(75,-10),(100,0)]`; 200 points, `pt[1] = (0.5025125628140703, 0.49998737405621063)`, length `114.77905987262527`; 200 points, length `113.24051269018844` (2000 samples would give 113.24087703591515); `quad curve needs 1 control points`; `unknown curve type 'spline'` |
| F3 | `mirror_curve({"type":"cubic","c":[[0.2,0.1],[0.7,-0.3]],"x":1})`; `reverse_curve` of the same without `x`; `mirror_curve(null)`; `mirror_curve({"type":"line"})` | `c = [[0.3,-0.3],[0.8,0.1]]`, `x` kept; `c = [[0.3,0.3],[0.8,-0.1]]`; null; unchanged |
| F4 | `at_fractions([(0,0),(10,0),(10,0),(10,10)], [0,0.25,0.5,0.75,1,1.5,-1])`; `at_fractions([(3,4),(3,4)], [0,0.5,1])` | `[(0,0),(5,0),(10,0),(10,5),(10,10),(10,10),(0,0)]`; three times `(3,4)` |
| F5 | `tangent_at` start/end of: `[(0,0),(1,0)]`; the 200-sample quad of F2; `[(0,0),(5,0),(10,0),(10,10)]`; `[(0,0),(1,0),(2,1)]` | `(1,0)`/`(1,-0)`; `(0.70379761, 0.71040054)`/`(0.70379761, -0.71040054)`; `(0.95799406, -0.28678803)`/`(-0.27056336, 0.96270217)`; `(0.96753822, -0.25272473)`/`(0.43096441, 0.90236893)` |
| F6 | `signed_area` of square (0,0)(10,0)(10,10)(0,10), reversed, and `[(0,0),(1,0)]`. `horizontal_width(square, y)` for y = 5, 0, 10, −1. U-shape `[(0,0),(30,0),(30,30),(20,30),(20,10),(10,10),(10,30),(0,30)]` at y = 20, 10. Triangle `[(0,0),(10,0),(5,10)]` at y = 0, 5. `point_in_polygon` of (5,5),(15,5),(10,0),(0,5),(10,10) in the square | `100`, `-100`, `0`; `10, 10, 0, 0`; `20, 20`; `10, 5`; `T, F, F, T, F` |
| F7 | `D([R("back",60,40,{"qty":1,"on_fold":"left"})])` | one instance `back` (C, not mirrored), edges `bottom.R a→b, right.R b→c, top.R c→d, top.L d→c.L, right.L c.L→b.L, bottom.L b.L→a`; extra verts `b.L (-60,0)`, `c.L (-60,40)`; area 4800 |
| F8 | `D([R("p",60,40,{"qty":2})])` | `p.R` as drafted, area 2400; `p.L`: side L, mirrored, edges `left a→d, top d→c, right c→b, bottom b→a`, verts `a (-0,0), b (-60,0), c (-60,40), d (-0,40)`, area 2400 |
| F9 | `D([R("bk",60,40,{"qty":2,"on_fold":"left"})])` | `bk.R` = F7's loop; `bk.L` edges `bottom.L a→b.L, right.L b.L→c.L, top.L c.L→d, top.R d→c, right.R c→b, bottom.R b→a`, verts `a(-0,0) b(-60,0) c(-60,40) d(-0,40) b.L(60,0) c.L(60,40)` |
| F10 | Triangle piece `t`, qty 1, on_fold `f`: a(0,0), b(100,0), c(0,100); `e1` a→b cubic with absolute controls (30,−10),(70,−10) (→ `[[0.3,-0.1],[0.7,-0.1]]`); `f` b→c; `e3` c→a | edges `e3.R c→a`, `e1.R a→b` (curve kept), `e1.L b→a.L` (curve `[[0.3,-0.1],[0.7,-0.1]]`), `e3.L a.L→c`; `a.L = (100,100)`; area `11019.684` (outline 60) |
| F11 | Structure errors (each input alone) | see table F11 below |
| F12 | Seam walk on `D([R("a",100,50), R("b",100,60)], [{"id":"s","a":"a/right","b":"b/left"}])` with the seam extras shown | see table F12 below |
| F13 | `R("a",100,50)` notch `{edge: right, s: 0.25}`, `R("b",100,60)` notch `{edge: left, s: 0.5}`, seam `a/right` ↔ `b/left`, reverse | `seam_pairs(spacing 5)`: `notch_match = piecewise`, `notches_a = [0.25]`, `notches_b = [0.5]`, 15 samples, `fa = [0, 1/24, 1/12, 0.125, 1/6, 0.208333, 0.25, 0.34375, 0.4375, 0.53125, 0.625, 0.71875, 0.8125, 0.90625, 1]`, `fb = [0, 1/12, 1/6, 0.25, 1/3, 5/12, 0.5, 0.5625, 0.625, 0.6875, 0.75, 0.8125, 0.875, 0.9375, 1]`; seam walk row FAIL, `residual -10.0`, `notch_segments_mm [-17.5, 7.5]`, message `ease is not where the notches say it is` |
| F14 | F13 with b's notch at `s 0.75` and `ratio 50/60` | PASS, `target_ratio "0.833"`, `notch_segments_mm [0.0, 0.0]` |
| F15 | b notches at 0.5 and 0.8, `ratio 50/60`; same without ratio | `WARN`, `notch_match "count-mismatch 1:2"`, message `notches don't pair (count-mismatch 1:2); ease distribution is unchecked`; without ratio: `FAIL`, residual −10.0, same message |
| F16 | b notches at s 0.0 and 1.0 | `notch_fractions("b/left") = []` (end notches dropped) |
| F17 | `R("m",60,40,{"qty":2})` notch `{edge: top, s: 0.25}` | `notch_fractions("m.R/right+top") = [0.55]`, `("m.L/top+right") = [0.45]`; junctions of `m.R/right+top` = `[0.4]`; side length 100 |
| F18 | `R("f",60,40,{"qty":1,"on_fold":"left"})` notch top 0.25; and the same with qty 2 | `f/top.R → [0.25]`, `f/top.L → [0.75]`, `f/top.R+top.L → [0.125, 0.875]`; qty 2: `f.L/top.L → [0.25]`, `f.L/top.R → [0.75]` |
| F19 | Seam `a/bottom+right` ↔ `b/left+bottom` on F12's rects | `a_junctions [0.6666666666666666]`, `b_junctions [0.625]`, 34 samples at spacing 5; row FAIL, `len 150.0 / 160.0`, ratio 0.9375, residual −10.0, message `b/left+bottom is 10.0 mm too long for the declared ratio 1.000` |
| F20 | F12's seam, spacing 5 / 7, no notches | 13 / 10 samples; `fa[1] = 0.08333333333333333` |
| F21 | F12 with `R("a")` collapsed so `a/right` has zero length (c = (100,0), d = (0,50)) | `structure_errors() = []` (area 2500 > 0); seam walk row `{seam: s, status: ERROR, message: "seam s: implausible side lengths 0 / 50 mm"}` |
| F22 | Truing on `D([R("a"), R("b")], [s: a/right ↔ b/left])`, spec `{}` | rows `s start at "a:b + b:a" line "bottom / bottom" 180.0 PASS` and `s end at "a:c + b:d" line "top / top" 180.0 PASS` |
| F23 | F22 with b's vertex a moved to (0,−5) | start row `angle_sum_deg 177.1`, `deviation_deg -2.9`, PASS |
| F24 | F22 plus seam `t: a/top ↔ b/top`; then with spec truing `[{seam: s, end: end}]` | rows: `s start` and `t end (at "a:d + b:c", line "left / right")`; with the explicit entry, `s end` is added (3 rows) |
| F25 | F22 with `reverse: false` | rows `at "a:b + b:d" line "bottom / top"` and `at "a:c + b:a" line "top / bottom"`, both 180.0 |
| F26 | L-shaped piece a(0,0) b(30,0) c(30,10) d(10,10) e(10,30) f(0,30), straight edges; `corner_angle` of every vertex; `corner_angle("zz")` | `a 90, b 90, c 90, d 270, e 90, f 90`; KeyError |
| F27 | Triangle a(0,0) b(100,0) c(50,80); `e0` a→b quad `[[0.5,-0.5]]`, then straight b→c, c→a | `edge_tangent(e0, start) = (0.70710678, -0.70710678)`, `end = (0.70710678, 0.70710678)` (exact, control polygon); corner angles a 102.994617, b 102.994617, c 64.010766; area 5666.187877046825; perimeter 303.45868251375737 |
| F28 | Points of measure on `D([R("r",100,50)])` with flat meas `{chest: 540}`: width at_vertex c, dy −10; vertical a→c with body_scale 0.1, body_offset −4; path `["r/bottom+right", "r/top"]` × 0.5, target [−500,0,500] | finished 100.0, ease −440.0, LOW, residual −440.0, `body_sigma_mm null`; finished 50.0, body 50.0, ease 0.0, IN_BAND; finished 125.0, ease −415.0, IN_BAND |
| F29 | path `["r/top"]`, target [−500,−480,−470], meas `{"fused": {"chest": {"value_mm": 540, "sigma_mm": 20}}}`; again with sigma 5.04 | HIGH, residual 30.0, sigma 20.0, note `body chest is only known to ±20 mm, wider than half the target band; a tape measurement would decide this point`; with 5.04: sigma 5.0, no note |
| F30 | Stubbed `landmark_h`: torso waist 500, chest 620; arm_R shoulder_tip 20, elbow −150. Piece `t` qty 2: a(0,−50) b(120,−50) c(100,150) d(0,150), placement `{part: torso, y0_landmark: waist, y0: 10}`; piece `s` qty 1: a(−60,−300) b(60,−300) c(80,−100) d(0,0) e(−80,−100), placement `{part: arm, y0_landmark: shoulder_tip}`. girth `[t.R, t.L]` landmark chest dy −20 body chest (400); width `s` landmark elbow body upper_arm (150); width `s` landmark waist | girth: line y = 110, finished 208.0 (104 per side), ease −192.0, LOW; width: line y = −170, finished 146.0, ease −4.0; landmark `waist` on an arm → UNMEASURABLE (KeyError) |
| F31 | `validate(D([R("a") without grain, R("b") with cut {}]), {"points": []}, meas)` | `NOT_CONVERGED`, completeness `["a: no grainline", "b: no cut instruction"]`, everything else empty; table in §14.4 |
| F32 | Eight vertical points on `R("a")` (finished 50) against flat body x = 40 (ease 10), targets `[t,t,t]` for t = 0, 60, 45, 10, 100, −30, 50, 70 | `worst = ["p4: LOW (residual -90.0)", "p7: LOW (residual -60.0)", "p1: LOW (residual -50.0)", "p5: HIGH (residual 40.0)", "p6: LOW (residual -40.0)", "p2: LOW (residual -35.0)"]` (stable on the 40/−40 tie) |
| F33 | `slash_spread(D([R("r",100,50)]), "r", "x", 50, 12)` | vertices `a [0.0,0.0] b [112.0,0.0] c [112.0,50.0] d [0.0,50.0]`, grain `[[10.0,10.0],[10.0,40.0]]` (not moved: x = 10 ≤ 50), width at y = 25 = 112.0, meta `{history: [{op: "slash-spread", args: {piece: "r", axis: "x", at: 50.0, add: 12.0}, why: "Slashed r at x=50 mm and spread +12.0 mm to add width there."}], origin: "VECTOR_EDIT"}` |
| F34 | `R("r",100,50)` with legacy grain `{p0:[60,10], p1:[60,40]}`, `internal [{kind: draw, pts: [[0,25],[100,25]]}]`, `drills [{p:[80,25], d:3}]`; `slash_spread(…, "y", 25, -5)` | c, d → y 45.0; grain `[[60.0,10.0],[60.0,35.0]]`; internal unchanged (y = 25 is not > 25); drills unchanged |
| F35 | `slash_spread(…, "x", 50, 0)` | why `Slashed r at x=50 mm and spread +0.0 mm to remove width there.` |
| F36 | `move(D([R("r")]), "r", "c", 1.25, -2.0)`; `move(…, "q", …)` | `c = [101.25, 48.0]`, why `Moved r:c by (+1.2, -2.0) mm.`; KeyError `no piece 'q'` |
| F37 | `true_seam(F12 doc, "s", "end")`; `"start"` | b's `a` → `[0.0, 10.0]`, args `{seam: s, end: end, moved_mm: -10.0}`, why `Trued seam s: moved b:a -10.0 mm along the seam so side b is 50.0 mm (side a / 1.000).`; start: b's `d` → `[0.0, 50.0]`, `moved b:d -10.0 mm` |
| F38 | F12 seam with `ratio_band [0.9, 1.1]`, `ratio 0.5`; `true_seam(…, "s")` | uses the band mean 1.0 (not 0.5): same result as F37 end |
| F39 | `transfer_curve(D([R("r",100,50)]), D([R("s") with right = quad [[0.5,0.2]]]), "s/right", "r/right")` | `r/right` curve poly with 20 points, first `[0.01695, 0.00666], [0.0678, 0.02528], [0.11864, 0.04183], [0.16949, 0.05631]`, last `[0.9322, 0.02528], [0.98305, 0.00666]`; new edge length 51.29994557770592 (source quad 51.30300026025734) |
| F40 | `transfer_curve` from a straight source edge | destination curve `{"type": "poly", "c": []}` |
| F41 | `bisect(x ↦ x − 100, −60, 60)`, `bisect(x ↦ x + 100, −60, 60)` | both `60.0` (no root: converges to hi) |
| F42 | `PieceBuilder("p","P").v("a", 1.0005, 2.0015)` | `[1.0, 2.002]` |

F11, structure errors (expected lists, exact):

| Input | Expected |
|---|---|
| `D([R("x")])` with schema `fabric-pattern/2`, units `cm` | `["schema must be 'fabric-pattern/1'", "units must be mm"]` |
| `R("x")` with edge `bottom` renamed `bad.id` | `["x: id 'bad.id' must not contain '.', '/', '+' or spaces"]` |
| `R("x y")` | `["x y: id 'x y' must not contain '.', '/', '+' or spaces"]` |
| `R("y")` with c = (100,−50), d = (0,−50) | `["y: edge loop is clockwise or degenerate (area -5000 mm²); loops must be CCW"]` |
| `R("z", {"qty":1,"on_fold":"nope"})` | `["z: on_fold edge 'nope' missing"]` |
| two seams with id `s` | `["duplicate seam id 's'"]` |
| `R("panel", fold left)` with edge 0 renamed `right` | `["panel: duplicate edge ids"]` |
| … with `bottom.from = "missing"` | `["panel/bottom: unknown vertex 'missing'", "panel: edge left ends at a but bottom starts at missing"]` |
| … with the fold edge a quad | `["panel: fold edge 'left' must be straight"]` |
| … with d = a | `["panel: fold edge 'left' must have finite, distinct endpoints"]` |
| `R("panel")` with `right` type `spline` | `["panel/right: curve type 'spline'"]` |
| `R("panel", {"qty":3})` | `["panel: cut.qty must be 1 or 2 (use separate pieces for more)"]` |
| `R("panel",100)`, `R("panel",200)` | `["duplicate piece id 'panel'", "duplicate instance id 'panel'"]` |
| `R("panel",{"qty":2})`, `R("panel.R")` | `["panel.R: id 'panel.R' must not contain '.', '/', '+' or spaces", "duplicate instance id 'panel.R'"]` |
| `R("panel")` without edge `top` | `["panel: edge right ends at c but left starts at d"]` |
| `R("panel")` with edge `right` lacking `id` | `["panel: cannot validate piece definition: 'id'"]` |
| `D([R("a"),R("b")])`, seam `a: missing/right` | `["seam s: \"seam ref 'missing/right': unknown instance 'missing' (have ['a', 'b'])\""]` |
| seam `a: a/missing` | `["seam s: \"a: no edge 'missing' (have ['bottom', 'right', 'top', 'left'])\""]` |
| seam `a: a/right+top+left` | `[]` (three consecutive edges) |
| seam `a: a/right+left` | `["seam s: seam ref 'a/right+left': edges right and left are not consecutive in loop order"]` |
| seam `a: aright` | `["seam s: not enough values to unpack (expected 2, got 1)"]` (port: any message naming the missing `/`) |
| second seam `t: a/right ↔ b/top` | `["seam t: edge a/right already used by seam s"]` |
| seam `s: a/right ↔ a/right` | `["seam s: edge a/right already used by seam s"]` |
| two seams without ids | `["duplicate seam id None"]` |
| `R("panel", {"qty": 2.7})` | `[]`, instances `panel.R`, `panel.L` (Python `int()`; see §15) |

F12, seam walk rows (`a = 50`, `b = 60`, measured ratio 0.8333):

| Seam extras | status | target_ratio | residual_mm | message |
|---|---|---|---|---|
| none (equal 50/50 rects instead) | PASS | `1.000` | 0.0 | — |
| none | FAIL | `1.000` | −10.0 | `b/left is 10.0 mm too long for the declared ratio 1.000` |
| `ratio 1.2` | FAIL | `1.200` | −22.0 | `b/left is 22.0 mm too long for the declared ratio 1.200` |
| `ratio_band [0.8, 0.9]` | PASS | `0.800–0.900` | 0.0 | — |
| `band [1.0, 1.3]` (legacy key) | FAIL | `1.000–1.300` | −10.0 | `ratio 0.833 is outside 1.000–1.300 by 10.0 mm` |
| `ratio_band [0.9, 1.0]`, `ratio 0.95` | FAIL | `0.900–1.000` | −4.0 | `ratio 0.833 is outside 0.900–1.000 by 4.0 mm` |
| `ratio_band null`, `ratio 1.2` | FAIL | `1.200` | −22.0 | as `ratio 1.2` (null band does not fall back to `band`) |
| `ratio_band []` | FAIL | `1.000` | −10.0 | as none |
| `reverse false` | FAIL | `1.000` | −10.0 | as none |
| side `a/zzz` (walk called directly) | ERROR | — | — | `"a: no edge 'zzz' (have ['bottom', 'right', 'top', 'left'])"` (quoted) |

### 14.3 Integration goldens (need the measurement-priors, body and drafting ports)

Body: the reference wearer at stature 920 mm, 30 months (public priors; no real person). Drafts with
default style. All three converge.

Bolero (`zouave_bolero`), instances `back` (C: `hem.R side.R armhole.R shoulder.R neck.R neck.L
shoulder.L armhole.L side.L hem.L`), `front.R`, `front.L` (`side armhole shoulder neck front_edge hem`),
`sleeve.R` (`hem under_f cap_f cap_b under_b`), `sleeve.L` (reversed). Seams (all `reverse: true`):
`side_R/L` (`front.R/side` ↔ `back/side.R`), `shoulder_R/L` (`ratio 1.05`, band [1.0, 1.08], ease),
`armhole_f_R/L`, `armhole_b_R/L` (band [1.0, 1.08], ease), `underarm_R/L` (`sleeve.R/under_f` ↔
`sleeve.R/under_b`). Fit spec truing: `side_R start, side_L end, shoulder_R start, shoulder_L end,
underarm_R end, underarm_L start`.

```
STATUS: CONVERGED
Finished measurements (authority: flat pattern)
  point                            body  finished    ease  target(min/nom/max)  status
  Chest ease (fronts edge to e    540.0     612.4    72.4  70/100/130           IN_BAND
  Across-back ease                225.7     250.7    25.0  10/25/45             IN_BAND
  Bicep ease                      156.8     206.8    50.0  35/50/70             IN_BAND
  Sleeve length over arm lengt    320.0     335.0    15.0  0/15/35              IN_BAND
  Sleeve hem passes the hand      142.7     170.0    27.3  10/25/60             IN_BAND
  Neckline ease (open front)      251.2     301.2    50.0  30/50/90             IN_BAND
  Back length vs nape-to-waist    225.0     235.0    10.0  -10/10/30            IN_BAND
Seam walk
  side_R                 a=  103.5 b=  103.5 ratio=1.000 target=1.000       resid=   0.0  PASS
  side_L                 a=  103.5 b=  103.5 ratio=1.000 target=1.000       resid=   0.0  PASS
  shoulder_R             a=   84.0 b=   80.0 ratio=1.050 target=1.000–1.080 resid=   0.0  PASS
  shoulder_L             a=   84.0 b=   80.0 ratio=1.050 target=1.000–1.080 resid=   0.0  PASS
  armhole_f_R            a=  133.2 b=  127.5 ratio=1.045 target=1.000–1.080 resid=   0.0  PASS
  armhole_f_L            a=  133.2 b=  127.5 ratio=1.045 target=1.000–1.080 resid=   0.0  PASS
  armhole_b_R            a=  152.9 b=  146.3 ratio=1.045 target=1.000–1.080 resid=   0.0  PASS
  armhole_b_L            a=  152.9 b=  146.3 ratio=1.045 target=1.000–1.080 resid=   0.0  PASS
  underarm_R             a=  243.9 b=  243.9 ratio=1.000 target=1.000       resid=   0.0  PASS
  underarm_L             a=  243.9 b=  243.9 ratio=1.000 target=1.000       resid=   0.0  PASS
Truing (angle sums across seams)
  side_R                 start armhole / armhole.R             174.5°  PASS
  side_R                 end   hem / hem.R                     185.5°  PASS
  side_L                 start hem / hem.L                     185.5°  PASS
  side_L                 end   armhole / armhole.L             174.5°  PASS
  shoulder_R             start armhole.R / armhole             180.0°  PASS
  shoulder_R             end   neck.R / neck                   180.0°  PASS
  shoulder_L             start neck.L / neck                   180.0°  PASS
  shoulder_L             end   armhole.L / armhole             180.0°  PASS
  underarm_R             start hem / hem                       180.0°  PASS
  underarm_R             end   cap_f / cap_b                   180.0°  PASS
  underarm_L             start cap_f / cap_b                   180.0°  PASS
  underarm_L             end   hem / hem                       180.0°  PASS
```

Bolero details: all four armhole seams are `notch_match: piecewise` with `notch_segments_mm [0.0, 0.0]`
(`armhole_f_L` reads `[-0.0, 0.0]`); `armhole_f_R` sampling at spacing 5 gives 28 samples,
`notches_a [0.45]`, `notches_b [0.45]`; truing `at` strings: `front.R:ua + back:ua`,
`front.R:side_hem + back:side_hem`, `front.L:side_hem + back:side_hem.L`, `front.L:ua + back:ua.L`,
`back:tip + front.R:tip`, `back:snp + front.R:snp`, `back:snp.L + front.L:snp`, `back:tip.L + front.L:tip`,
`sleeve.R:hem_f + sleeve.R:hem_b`, `sleeve.R:ua_f + sleeve.R:ua_b` (and `sleeve.L:` mirrors).
Body sigmas: chest 30.0, across_back 15.9, upper_arm 14.0, arm_length 13.0, hand_pass 10.0,
neck_base 15.3, back_neck_to_waist 15.0 (no notes).

Trousers (`zouave_trousers`):

```
  Seat fullness (design ease)     580.0     872.3   292.3  200/320/480          IN_BAND
  Total crotch length ease        396.6     426.6    30.0  15/30/50             IN_BAND
  Elastic casing passes over t    580.0     610.0    30.0  20/40/120            IN_BAND
  Relaxed elastic vs waist (mu    530.0     503.5   -26.5  -63/-26/-5           IN_BAND
  Cuff passes over the foot       205.7     229.5    23.8  10/25/60             IN_BAND
  Cuff ease at the calf           204.5     229.5    25.0  10/25/45             IN_BAND
  Leg length blousing over the    354.4     394.4    40.0  15/40/80             IN_BAND
Seam walk
  crotch_front_R         a=  200.3 b=  200.3 ratio=1.000 target=1.000       resid=   0.0  PASS
  inseam_R               a=  209.0 b=  209.0 ratio=1.000 target=1.000       resid=  -0.0  PASS
  inseam_L               a=  209.0 b=  209.0 ratio=1.000 target=1.000       resid=  -0.0  PASS
  outseam_R              a=  395.5 b=  395.5 ratio=1.000 target=1.000       resid=  -0.0  PASS
  outseam_L              a=  395.5 b=  395.5 ratio=1.000 target=1.000       resid=  -0.0  PASS
  cuff_front_R           a=  194.2 b=  107.9 ratio=1.800 target=1.400–2.400 resid=   0.0  PASS
  cuff_front_L           a=  194.2 b=  107.9 ratio=1.800 target=1.400–2.400 resid=   0.0  PASS
  cuff_back_R            a=  218.9 b=  121.6 ratio=1.800 target=1.400–2.400 resid=   0.0  PASS
  cuff_back_L            a=  218.9 b=  121.6 ratio=1.800 target=1.400–2.400 resid=   0.0  PASS
  cuff_end_R             a=   60.0 b=   60.0 ratio=1.000 target=1.000       resid=   0.0  PASS
  cuff_end_L             a=   60.0 b=   60.0 ratio=1.000 target=1.000       resid=   0.0  PASS
  crotch_back            a=  226.3 b=  226.3 ratio=1.000 target=1.000       resid=  -0.0  PASS
  casing_bot_bR          a=  220.9 b=  154.6 ratio=1.429 target=1.000–2.000 resid=   0.0  PASS
  casing_bot_fR          a=  215.0 b=  150.4 ratio=1.429 target=1.000–2.000 resid=   0.0  PASS
  casing_bot_fL          a=  215.0 b=  150.4 ratio=1.429 target=1.000–2.000 resid=   0.0  PASS
  casing_bot_bL          a=  220.9 b=  154.6 ratio=1.429 target=1.000–2.000 resid=   0.0  PASS
  casing_cb              a=   60.0 b=   60.0 ratio=1.000 target=1.000       resid=   0.0  PASS
Truing (angle sums across seams)
  cuff_end_R             start bottom / bottom                 180.0°  PASS
  cuff_end_L             end   bottom / bottom                 180.0°  PASS
  casing_cb              end   top / top                       180.0°  PASS
```

The `elastic_hold` row's `target_mm` is `[-63.6, -26.5, -5.0]` (printed `-63/-26/-5`: truncation toward
zero); the `crotch` (σ 40.0) and `elastic_hold` (σ 35.0) rows carry the σ note. Fez (`fez`):
`head_fit` 483.8 / 493.8 / 10.0 IN_BAND with the σ note (σ 15.0); `height` 154.0 / 97.0 / −57.0,
target `[-80.1, -57.0, -33.9]`; seams `back` 97.0/97.0 and `tip` 385.2/385.2 (`resid -0.1`) with seam
tolerance 2.0; one truing row `back start base_1 / base_0 180.0°`.

Integration alteration goldens on the bolero:

| Op | Expected |
|---|---|
| Mutation M1 (below) then `true_seam(…, "side_R", "start")` | back `side_hem` `[160.0, -10.0] → [159.518, -19.978]`; args `moved_mm 9.99`; why `Trued seam side_R: moved back:side_hem +10.0 mm along the seam so side b is 113.5 mm (side a / 1.000).`; `side_R` and `side_L` then PASS at 113.5/113.5, residual −0.0 |
| `true_seam(…, "side_L", "start")` | ValueError `true the drafted (.R) side; the mirrored copy follows` |
| `cap_ease(bolero, "sleeve", ["armhole_f_R", "armhole_b_R"], 1.06)` | only `top` moves: `[12.832, 0.0] → [12.832, 2.961448104437153]` (y_ua = −91.777); `height_scale 1.0323`; why `Scaled the sleeve cap height by 1.032 so cap ease goes from 4.5% to 6.0% of the armhole.`; total ratio 1.06; per-seam ratios `armhole_f 1.0622`, `armhole_b 1.0581` |
| `transfer_curve(D([R("r",100,50)]), bolero, "front.R/front_edge", "r/right")` (source cubic `[[0.09504, -0.20163], [0.48247, -0.12963]]`) | 20 poly points, first `[0.00508, -0.01002], [0.02331, -0.0373], [0.0459, -0.06057]`; endpoints (100,0), (100,50); length 52.83778853868992 |

Role-based default spec on the bolero with roles back = `bodice_back`, front = `bodice_front`,
sleeve = `sleeve`: points `chest` (instances `front.R, front.L, back`), `across_back` (`back`),
`neckline` (refs `front.R/neck, front.L/neck, back/neck.R, back/neck.L`, target [20, 50, 90]),
`cuff_pass` (`sleeve.R/hem`), `bicep` (`sleeve.R`, `at_vertex ua_f`); CONVERGED with the same values as
the drafted spec (`Neckline ease 251.2 301.2 50.0 20/50/90`). On the trousers (roles `pant_front`,
`pant_back`, `waistband`, `cuff`) the only point is `seat` over `tr_front.R, tr_front.L, tr_back.R,
tr_back.L`, target [60, 160, 480]: 580.0 / 872.3 / 292.3 IN_BAND. A pattern without roles gets
`points: []`.

### 14.4 Mutation tests (seed a known defect, the check that claims to cover it must fire)

| # | Seed (on the converged bolero unless noted) | Expected |
|---|---|---|
| M1 | front `side_hem.y −= 10` (front side seam 10 mm longer) | NOT_CONVERGED, `seams_failing 2`: `side_R` and `side_L` FAIL, len 113.5/103.5, ratio 1.0965, residual 10.0, message `front.R/side is 10.0 mm too long for the declared ratio 1.000` (`front.L/…` for L); `worst` = those two messages |
| M2 | draft with style `chest_ease 10.0` | chest row finished 522.4, ease −17.6, residual −87.6, LOW; worst `Chest ease (fronts edge to edge): LOW (residual -87.6)` |
| M3 | remove the sleeve `hem` curve (straight hem on a tapered sleeve) | truing FAIL `underarm_R start` and `underarm_L end`, line `hem / hem`, 188.7°, deviation 8.7, message `the hem/hem line dips by 9° at this seam; true the corner so the joined line runs straight through` |
| M4 | spec point `{id: x, kind: width, instance: sleeve.R, at_vertex: nope, body: upper_arm, target [0,1,2]}` | UNMEASURABLE, message `x: cannot measure on this pattern ('nope'); rename the vertex/edge or adapt the fit spec`; table line `  x                                   —         —       —  0/1/2                UNMEASURABLE` |
| M5 | point of kind `spiral`, label `Spiral` | message `Spiral: cannot measure on this pattern (unknown point kind 'spiral'); rename the vertex/edge or adapt the fit spec` |
| M6 | girth on `sleeve.R` at landmark `chest` | UNMEASURABLE (`('chest')`: arms only know shoulder_tip/elbow/wrist) |
| M7 | point with body `knee_to_moon` (not measured) | status BLOCKED, `missing_measurements ["knee_to_moon"]`, the row UNMEASURABLE, `points_out_of_band 1`, worst = its message; the table prints all sections and does not list the missing name |
| M8 | trousers: hem vertices of `tr_front` and `tr_back` 150 mm lower | `leg_blouse` HIGH (from IN_BAND 40.0) |
| M9 | trousers style `elastic_frac 1.05` | `elastic_hold` HIGH, status ≠ CONVERGED |
| M10 | fez drafted for a head 30 mm smaller, validated against the real head | `head_fit` LOW |
| M11 | schema `x` | `{status: BLOCKED, structure_errors: ["schema must be 'fabric-pattern/1'"]}`; table `STATUS: BLOCKED\n  structure: schema must be 'fabric-pattern/1'` |
| M12 | flat measurement dict (no `fused`) | bolero still CONVERGED; every `body_sigma_mm` null |

F31's table (empty points and seams):

```
STATUS: NOT_CONVERGED
Finished measurements (authority: flat pattern)
  point                            body  finished    ease  target(min/nom/max)  status
Seam walk
  completeness: a: no grainline
  completeness: b: no cut instruction
```

### 14.5 What a conformance harness should compare

| Output | Comparison |
|---|---|
| `status`, every row `status`, `notch_match`, ids, labels, `target_ratio`, `missing_measurements`, `completeness` | exact |
| Rounded report numbers (`*_mm`, `ratio`, `angle_sum_deg`, `deviation_deg`) | exact is expected with identical sampling; allow ±1 in the last printed digit only where the unrounded value lies within 1e-9 of a rounding tie |
| Raw geometry (lengths, areas, sample points, tangents) | relative 1e-9 |
| Messages built from templates in this spec | exact (including `-0.0`, `±`, `°`, `–`, `²`) |
| Messages embedding Python exception text (`not enough values to unpack …`) | substring on the stable part |
| Alteration outputs | semantic JSON equality (numbers numerically), with the moved coordinates exact after the specified rounding; `cap-ease` vertices (unrounded) to 1e-9 |
| Ordering of check-1 id errors (§7.2) | multiset |

### 14.6 Tolerances and constants (one place)

| Constant | Value | Where |
|---|---|---|
| Edge / side sampling | 200 samples | §4.2 |
| Outline for area / CCW | 60 per curved edge | §6.2 |
| Outline for girth / width | 120 per curved edge | §9.3 |
| Seam sample spacing | 5.0 mm | §9.1 |
| End-notch exclusion | 1e-4 (fraction) | §8.3 |
| Breakpoint dedupe rounding | 9 decimals | §8.4 |
| Plausible side length | 0 < L < 1e5 mm | §8.4 |
| Seam tolerance | 3.0 mm (spec `seam_tol_mm`, seam `tol_mm`) | §9.1 |
| Notch-segment tolerance | `max(tol, 0.08·len_a/segments)` | §9.1 |
| Truing tolerance | 8.0° | §9.2 |
| Tangent fit window | `[min(2, 0.1L), min(14, 0.5L)]` mm, ≥ 4 samples | §5 |
| Degenerate control-point distance | 1e-9 mm | §6.2 |
| Bisection | 60 iterations | §10.1 |
| true-seam bracket | [−60, 60] mm | §10.4 |
| cap-ease bracket | [0.3, 3.0] | §10.5 |
| transfer-curve | 60 samples, every 3rd interior | §10.6 |
| `worst` length | 6 | §9.4 |
| Default chest line offset | −24.0 mm | §9.5 |

---

## 15. Reference crash paths (behaviour undefined by the reference)

The reference raises an uncaught exception on these inputs. A port MUST NOT reproduce the crash; the
recommended behaviour keeps the reference's verdicts on every valid input.

| Input | Reference | Recommended port behaviour |
|---|---|---|
| Piece without `id` | KeyError in `Pattern()` | structure error `piece {index}: missing id` and no instances |
| `cut: null` | AttributeError | treat as absent (qty 1) for geometry; completeness still reports `no cut instruction` |
| quad/cubic with the wrong control-point count | ValueError escapes `structure_errors()` (area loop) | structure error `{pid}/{eid}: {kind} curve needs N control points` from the definition checks |
| Seam without `id` (sides valid) | KeyError in the edge-use check | report `duplicate seam id None` style errors; reference the seam by index |
| Missing and string seam ids mixed with duplicates | TypeError sorting `None` with strings | sort with `None` first |
| `ratio: null` with no band | TypeError | treat as the default 1.0 |
| `ratio_band` not of length 2 | ValueError escapes `seam_walk` | ERROR row `seam {id}: ratio_band must be [lo, hi]` |
| Zero-length seam side carrying a notch | ZeroDivisionError in `notch_fractions` | ERROR row with the implausible-length message |
| Point without `body` | KeyError in `missing_measurements` | UNMEASURABLE row; do not list it as missing |
| Measurement entry without `value_mm`/`value_deg` | TypeError | treat as missing (BLOCKED) |
| ERROR seam row without an id in `format_table` | TypeError (`None:22s`) | print `None` padded |
| `true-seam` unknown seam id; `cap-ease` no matching seams; `transfer-curve` unknown destination edge | StopIteration / IndexError | error naming the missing id |
| `cut.qty` 2.7 or `"2"` | accepted as 2 | structure error unless an integer 1 or 2 (documented deviation; no fixture depends on it) |

---

## 16. Exclusions / not to port

| Item | Why |
|---|---|
| `reverse_curve` | Never called (spec it only if a future op needs it). |
| `Instance.edge_index` | Never called. |
| `Pattern.save`, module-level `load()` | Unused (the CLIs write JSON themselves; `Pattern.load` is just read-and-construct). |
| `_corner_angle_at` | Trivial alias of `corner_angle`. |
| `sys.path` manipulation, argparse wiring, `np.errstate` | Python plumbing. |
| Aliasing of `.R` instance verts/edges with the base instance | Python object sharing; instances are immutable values in a port. |
| Double evaluation of `f(mid)` in `bisect` | Side-effect free; evaluate once. |
| Rebuilding a whole `Pattern` per bisection step | Performance artefact; recompute only the affected side, preserving results. |
| Monkeypatch observability of `piece_instances` | Tests use it to prove no construction happens on invalid definitions; a port keeps the guarantee (§7.1 step 3), not the hook. |
| Legacy `band` seam key, legacy `{p0, p1}` grain | Keep **reading** them; never write them. |

Everything else in §2–§11 is live and in scope.

---

## 17. Surprises and discrepancies (decide before porting)

1. **`default_fit_spec` bands differ from the drafted specs and the fit contract**: neckline
   `[20, 50, 90]` vs `[30, 50, 90]`, seat `[60, 160, 480]` vs the trousers' `[200, 320, 480]`. Port as
   is; the difference is deliberate in spirit (generic outerwear vs a full Zouave), but undocumented.
2. **cap-ease balances the total, not each half**: on the bolero, 1.045/1.045 becomes 1.0622/1.0581
   at a 1.06 target, although the fit contract asks for cap ease "on both halves".
3. **true-seam ignores a declared `ratio` when a band exists** and trues to the band midpoint (a cap
   seam with band [1.0, 1.08] is trued to 1.04).
4. **Bisection never reports failure**: without a sign change it returns the upper bracket (60 mm, or a
   cap height scale of 3.0) and records it in history as if solved.
5. **slash-spread and cap-ease do not move drills or placement anchors**; slash-spread also rounds every
   vertex to 0.001 mm, moved or not, and treats any axis other than `x` as `y`.
6. **The id grammar check omits `:`** although the documentation lists it; only `. / + space` are
   rejected (tabs and other whitespace pass).
7. **The `.L` twin rule is documented but not validated.**
8. **`format_table` never prints `missing_measurements`**; a BLOCKED-by-measurement report shows only
   UNMEASURABLE rows.
9. **Count-mismatch notches override the residual message** even when the row FAILs on length.
10. **A `ratio_band` key set to null disables the legacy `band` fallback** (key presence, not value,
    decides).
11. **transfer-curve is asymmetric and lossy**: the source ref is an instance, the destination a piece;
    a poly source keeps only every third control point; "size kept" means endpoints, while the depth
    scales with the destination chord.
12. **Tangent fit doc drift**: the code comment says "first ~8 mm"; the window is actually 2–14 mm
    (shorter edges: 10–50 % of their length).
13. **Validate CLI exit code 2** means both NOT_CONVERGED and a usage error.
14. **Short traced edges** (fewer than 4 samples) get their end tangent from the interpolating quadratic,
    not the first segment (F5), which can move corner angles by several degrees.
15. **Check-1 error order is nondeterministic** in the reference (vertex ids iterated as a set).
