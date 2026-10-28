# 04 · Fold-up simulation, sheet vectorizing and the `fab` CLI

Essence specification extracted on 2026-10-12 from the Python reference implementation (fabric-pattern-creation-and-alteration
skill): `scripts/blender_sew.py`, `scripts/vectorize.py`, `scripts/fabric.py`, `scripts/report.py`, `scripts/outfit.py`,
`scripts/viewer_export.py`, `scripts/webapp.py`, `browser/index.html` and `browser/fold_viewer_template.html`, together
with their tests and the skill's BLENDER-SEWING, VECTORIZING and CASE-STUDY references.

You should not need to read the legacy code to implement anything below. The reference is the oracle: where this
document and the reference disagree, the reference wins until a deliberate change is recorded. Section 16 lists the
quirks found in the reference. For each one, either port the behaviour or record a deliberate fix.

## Contents

0 Conventions · 1 Fold-up pipeline · 2 Garment mesh · 3 Placement · 4 Kinematic start · 5 Dynamics contract ·
6 Metrics and flags · 7 Qualification and tightness gating · 8 Failure modes as requirements, and known-answer tests ·
9 Output formats · 10 Vectorizing (`vectorize.py`) · 11 The `fab` CLI · 12 Browser (`webapp.py`, workbench page) ·
13 Conformance oracle · 14 Exclusions / not to port · 15 Dependencies on companion specs · 16 Reference quirks

---

## 0. Conventions

| Item | Rule |
|---|---|
| Pattern units | mm. Piece 2D frame: x right, y up. Each piece is drafted as the **wearer's right**, public side up, with CCW edge loops (positive signed area). |
| 3D units | m, for start positions, all frames, PC2 and the viewer. World axes: +Z up, the wearer faces −Y, wearer's right = −X. |
| Angle around a body-part axis | θ = 0 is the part frame's `front`; +90° is its `lat` direction (`lat = up × front`, which is +X = wearer's left for the torso). A viewer outside the surface sees θ increase to their right, so piece x maps to θ increasing. |
| `MM` | 0.001 (mm → m). |
| Rounding | `rN(x)` = round to N decimals the way Python `round(x, N)` does (nearest; ties go to even on the binary value). Golden comparisons allow ±1 in the last digit. |
| Strain | `ε = L₃ / L₂ − 1`, where L₂ is the rest length (2D, after `sim_scale`, converted to m) and L₃ the 3D length. Positive means stretch. |
| Percentiles | linear interpolation (numpy default). |
| Flag text | Each flag is an exact string (templates in §6.10). Gating is decided by **string identity** between lists, so the templates are part of the contract. `{x}` means the shortest round-trip decimal of an already-rounded float (Python `repr`); `{x:.0f}` means formatted to integer. |
| "Instance" | One physical cut piece (`front.R`, `front.L`, `back`, ...). The companion pattern-model spec defines it (§15). |

---

## 1. Fold-up pipeline (solver-independent overview)

| Stage | Input | Output | Section |
|---|---|---|---|
| 1. Select | pattern, measurements | simulated instances (`sim` ≠ false **and** `placement` present), seams whose two sides both lie on simulated instances | 2.1 |
| 2. Mesh | instances, seams, `spacing` (12 mm) | 2D vertices (mm), CCW triangles, sewing pairs, index maps; `sim_scale` applied | 2 |
| 3. Board | 2D mesh | flat layout on a vertical board in front of the form (frame 1 of the animation) | 9.3 |
| 4. Wrap | 2D mesh, placements, body | isometric cylinder wrap per instance (`wrapped`) | 3 |
| 5. Pre-sew | wrapped, sewing pairs, true form | sewing groups moved to coincide outside the body | 4.2 |
| 6. Relax | pre-sewn start | start state with p95 plain-cloth strain ≤ 12 % (or iteration cap) | 4.3 |
| 7. Dynamics | start, rest = flat 2D, fabric, collider | `sew_frames` (45) at zero gravity, then `drape_frames` (120) with gravity | 5 |
| 8. Metrics | baked frames, true form, fit spec | `sim_report.json` (metrics, flags, gating subset) | 6 |
| 9. Animation | board, fold kinematics, zip, baked frames | `fold.pc2`, `mesh_map.json`, colour attributes, stills, videos, viewer | 9 |

The simulator never reports ease. Ease is analytic (flat validation). A tape girth measured around simulated cloth is
biased low by 0.4–2.1 cm and moves 1.7 cm with bending stiffness. Global strain p95 and µm-sized seam gaps are sanity
numbers, never gates.

---

## 2. Garment mesh

### 2.1 Selection and order

- Simulated instances are the pattern's instances in construction order (pieces in file order; within a piece, `.R`
  before `.L`). An instance qualifies when its piece has `placement` and `sim` is not `false`. This order fixes the
  global vertex numbering, `inst_range`, the board layout, the animation stagger and the viewer data.
- Seams are kept, in pattern order, when both side refs (`INST/EDGE[+EDGE]`) name simulated instances.

### 2.2 Seam sampling (matched pairs)

For each kept seam, matched sample sequences come from the pattern model's seam pairing with `spacing`:

1. **Side polylines.** Concatenate the dense polylines of the side's edges in loop order, dropping each duplicate
   junction point. Dense sampling per curve type:

   | Curve type | Sampling |
   |---|---|
   | `line` | the 2 endpoints |
   | `quad`, `cubic` | 200 points, uniform in t |
   | `poly` | the control polygon |

   Record each internal edge junction as an arclength fraction.
2. **Reverse.** If `reverse` (default **true**), reverse side b's polyline, map its junctions to `1 − j` (reversed
   order), and map its notch fractions to `1 − f`.
3. **Notch fractions.** A notch counts on a side when its edge's stem (the id without a trailing `.R`/`.L`) is one of
   the side's edges. Its `s` is flipped to `1 − s` when (the edge id ends in `.L`) XOR (the instance is mirrored).
   Its fraction along the side is `(Σ previous edge lengths + s · edge length) / side length`. Fractions ≤ 1e-4 or
   ≥ 1 − 1e-4 are dropped, and the rest are sorted.
4. **Knots.** If side a has notches and both sides have the same count, the knots are `[0, na…, 1]` ↔ `[0, nb…, 1]`
   (`notch_match = "piecewise"`). Otherwise the knots are `[0, 1]`, with `notch_match` = `"none"` or
   `"count-mismatch A:B"`. `to_b` is the piecewise-linear map from knots_a to knots_b, and `to_a` is its inverse.
5. **Breakpoints.** Take the sorted unique values (rounded to 9 decimals) of knots_a ∪ a-junctions ∪ `to_a`(b-junctions).
6. **Plausibility.** Both side lengths must satisfy 0 < L < 1e5 mm. Otherwise raise
   `seam ID: implausible side lengths A / B mm`.
7. **Sampling.** For each breakpoint interval [u0, u1], let
   `n = max(1, ⌈max((u1−u0)·La, (to_b(u1)−to_b(u0))·Lb) / spacing⌉)`. Append `linspace(u0,u1,n+1)[1:]` to `fa` (which
   starts at [0]) and the mapped values to `fb`. The points are taken at those arclength fractions on each polyline.

With these rules, declared ease and gathers land where the notches say: the longer side gets wider sample spacing, so
equal sample counts carry the ratio. Every junction falls exactly on a sample of both sides.

### 2.3 Boundary samples per instance

- For an edge covered by a seam side, the samples come from 2.2. Side a is used as sampled. Side b is used in its own
  loop order, which is the reversed list when `reverse` is true. A multi-edge side is split into per-edge lists at the
  sample nearest each internal junction vertex. Each per-edge list includes both of its end samples.
- An edge in no seam gets `n = max(1, ⌈length(dense) / spacing⌉)` segments at uniform arclength fractions
  `linspace(0,1,n+1)` of its dense polyline.
- The boundary loop of the instance is the concatenation, in edge loop order, of each edge's samples minus its last
  point (the shared corner belongs to the next edge).
- `edge_index[(inst, edge)]` is the list of global indices along the edge, both ends included; the last index wraps to
  the next edge's first.
- `corner[(inst, vertex)]` is the global index of the first sample of the edge that starts at that vertex. Every loop
  vertex appears.

### 2.4 Interior points (hex grid)

Let `s = spacing` and let the box (lo, hi) be the boundary samples' bounding box.

1. Columns: x = `lo.x + s/2, lo.x + 3s/2, …` (all values < hi.x).
2. Rows: y = `lo.y + s/2` stepping by `0.866·s` (all values < hi.y). Odd rows (row index 1, 3, …) shift x by `+s/2`.
3. Keep a point if it lies inside the boundary polygon (even-odd rule) **and** its distance to the nearest boundary
   sample is > `0.55·s`.

### 2.5 Triangulation

- Compute a constrained Delaunay triangulation of [boundary samples, then interior points], with the boundary loop as
  constraint edges. Keep only the triangles inside the loop. The reference uses ε = 1e-6.
- If the triangulator merges or adds vertices, keep the original indices and append any new vertices after the
  instance's points, inside its range.
- Orient every triangle CCW in 2D (positive signed area).
- `inst_range[inst] = [first, end)`, a half-open range of global vertex indices.

### 2.6 Sewing pairs and seam records

- For a seam: `pa` is the global index path of side a (edges concatenated, with each internal duplicate junction
  dropped). `pb` is built the same way and reversed if `reverse`. Their lengths must be equal, otherwise error
  `seam ID: NA vs NB samples`.
- Sewing pairs are the zipped `(pa[i], pb[i])` with `pa[i] ≠ pb[i]`. Identical indices occur where one instance sews
  to itself at a shared corner, such as a sleeve underarm.
- Seam record:

  | Field | Value |
  |---|---|
  | `pairs` | the sewing pairs |
  | `side_a` | `pa` |
  | `side_b` | `pb` in side b's own loop order |
  | `ratio` | `seam.ratio`, default 1.0 |

- The global sewing pairs list is the concatenation over seams in seam order.

### 2.7 `sim_scale`

When a piece has `sim_scale: [sx, sy]`, multiply that instance's 2D vertices by (sx, sy) **after** triangulation and
pairing. This applies to cut-double casings drawn in to the waist girth and to cuffs folded in half. Its rest lengths,
board layout, wrap and mass area are then the finished ones. The placement anchors `x0` and `y0` are scaled by the same
factors (§3.1).

---

## 3. Placement (start shape)

### 3.1 Placement record (per piece)

| Key | Type / default | Meaning |
|---|---|---|
| `part` | `torso` \| `arm` \| `leg` | body part the piece wraps around (arm and leg use the wearer's-right frame `arm_R` / `leg_R`) |
| `theta_deg` | 0 | angle of the anchor column x = x0 |
| `x0` | 0 | piece x of the anchor column (scaled by sim_scale x) |
| `y0_landmark` | `waist` | landmark that piece y = `y0` sits at |
| `y0` | 0 | piece y placed at the landmark (scaled by sim_scale y) |
| `clearance_mm` | 25 | added to the body section radius when no `radius_mm` |
| `radius_mm` | optional | fixed wrap radius |
| `inferred` | bool | set by role inference (§3.5) |

### 3.2 Part frames and landmark heights

The body/form spec (§15) defines these frames and heights. They are repeated here because placement depends on them.

| Frame | origin | up | front | notes |
|---|---|---|---|---|
| torso | (0,0,0) | +Z | −Y | h = height above floor (mm) |
| arm_R (default form) | (−(shoulder_half_width − 0.6·r_up), 0, z_shoulder − 0.8·r_up), with r_up = upper_arm/2π | (sin a, 0, cos a), a = arm abduction (default **35°**) | −Y | `tip_h = 0.8·r_up`; h is measured along `up` from the origin |
| leg_R | (−hip_width/4, 0, z_crotch + 25) | (sin 4°, 0, cos 4°) | −Y | length = (origin_z − 55)/cos 4° |

`landmark_h(part, name)`:

| Part | Landmark names | Value |
|---|---|---|
| torso | `floor`, `ankle` (55), `calf`, `knee`, `crotch`, `hip`, `waist`, `chest`, `across_back`, `shoulder`, `snp`, `c7`, `head_top`, `head_centre` | landmark height z (mm) |
| arm | `shoulder_tip` | `tip_h` |
| arm | `elbow` | `tip_h − 0.55·arm_length` |
| arm | `wrist` | `tip_h − arm_length` |
| leg | any torso landmark z | `−(origin_z − z) / up_z` |

### 3.3 Isometric wrap

For each 2D point (x, y) of an instance:

| Quantity | Value |
|---|---|
| h | `landmark_h(part, y0_landmark) + (y − y0)` |
| R | `radius_mm` if given; otherwise `max` over 9 heights `linspace(min h, max h)` of `section_radius(part, h)`, plus `clearance_mm` |
| θ | `rad(theta_deg) + (x − x0) / R` |
| position | `p = origin + h·up + R·(sin θ·lat + cos θ·front)` (mm, then × MM) |

The wrap preserves arclength along x at radius R and along y exactly. The tested guarantee is that consecutive-sample
3D/2D distance ratios stay within 1 % on the bolero front.

`section_radius(part, h)` (form spec) is:

- torso: interpolate (a, b, cy) of the torso sections at h, then take max over 96 angles of `hypot(a·sin t, cy − b·cos t)`;
- arm: `1.15·upper_arm / 2π`;
- leg: `1.05·thigh / 2π`. If the point's world z > origin_z − 20 mm, also clear the pelvis: let xj = |origin.x|. Take
  `reach = |cy| + b·√(1 − (xj/a)²)` if a > xj, else b. Then `r = max(r, reach, a − xj)`.

### 3.4 Mirrored instances and the leg midline

- A mirrored instance (the `.L` copy of a cut-2 piece) is handled in four steps: mirror its 2D x, wrap as a right
  piece, apply the own-side rule, then mirror the 3D x. On-fold instances (side `C`) are wrapped directly, spanning
  both sides.
- **Own-side rule.** For instances with side R or L whose placement part is `leg`, clamp the drafted-frame x to
  `min(x, −1.0 mm)` before mirroring. Whatever wraps past the midline, such as the crotch extension, is laid on the
  sagittal plane, where the crotch seam lies. Without this the right and left panels start crossed. This rule applies
  to the start wrap and to the roll at k ≥ 1, not to partial rolls.

**Native extension: physical limb panels.** A quantity-one panel with explicit
`placement.side: "R" | "L"` uses the selected physical limb frame directly, including its
landmark and section-radius queries. The supplied 2D coordinates and public side are unchanged;
neither generated-instance reflection is applied. The frame must remain properly handed
(`lat = up × front`), so selecting a left limb cannot turn the cloth inside out. The same frame,
anchor and radius drive the partial roll. For physical leg panels the own-side start rule is
`min(x, −1 mm)` on the right and `max(x, +1 mm)` on the left, applied only at the completed wrap.
Torso sides, mirror-pair quantities and folded panels with an explicit physical side refuse.

`PartFrames` providers may supply independent left and right frames. The existing Anny
`BodyFrames` drape adapter supplies the left start frame by reflecting the right origin, up and
front about x = 0 and recomputing `lat = up × front`; it retains the right-derived landmark
heights and section radii. This is the existing symmetric initial-placement convention, not
qualification of asymmetric anatomy, a different solver, or a physical-fit claim.

### 3.5 Inferred placement (pieces without one; traced sheets)

The round step (§11.5) runs this for every piece without a placement whose `sim` is not false, unless the pattern is
UNSCALED. Let v be the piece vertices; xmin, xmax, ymax their extents; L the landmarks.

| Role | Placement |
|---|---|
| `bodice_front` | torso, θ −4.8°, x0 = xmax (CF is +x), y0_landmark waist, y0 = r2(ymax − (snp + 10 − waist)), clearance 20 |
| `bodice_back` | torso, θ 180°, x0 = xmin, same y0, clearance 20 |
| `skirt_front` / `skirt_back` | torso, θ 0° / 180°, x0 = xmax / xmin, y0_landmark waist, y0 = r2(ymax), clearance 25 |
| `pant_front` / `pant_back` | leg, θ 0° / 180°, x0 = r2((xmin + xmax)/2), y0_landmark waist, y0 = r2(ymax), clearance 25 |
| `sleeve` | arm, θ −90°, x0 = x of the highest vertex, y0_landmark `shoulder_tip`, y0 = its y, radius = r2(max((xmax − xmin)/2π, section_radius(arm_R, 0) + 6)) |
| anything else (waistband, cuff, facing, unknown) | none: excluded from the fold-up |

Every inferred placement carries `inferred: true`. Each decision appends one note:

- `ID: placement inferred from role 'ROLE' (PART, theta Θ)`, or
- `ID: no placement (role 'ROLE' is not simulated); excluded from the fold-up`.

### 3.6 Roll (exact isometry used by the animation)

`roll(points, placement, k)`: at k ≥ 1 it returns the wrap. Below 1:

1. Let R be the wrap radius, `κ = max(k, 1e-6) / R` and `u = x − x0`.
2. Local coordinates: `lx = sin(κu)/κ`, `ly = (1 − cos(κu))/κ`.
3. `flat` = the wrap of the anchor column (x0, y) for each point.
4. Directions: `tangent = cos θ0·lat − sin θ0·front`; `normal_in = −(sin θ0·lat + cos θ0·front)`.
5. Result: `flat + lx·tangent + ly·normal_in`.

At k = 0 this is a flat sheet tangent to the wrap at the anchor column. At k = 1 it is the wrap.

---

## 4. Kinematic start: pre-sew and relaxation

All distances in this section are measured against the **true form**: the dress form with zero inset, made of all its
parts (torso, head, arms, legs) taken together as one triangle soup.

### 4.1 Signed distance and push-out

- `sd(p) = (p − q) · n(q)`, where q is the closest point on any form triangle and n(q) that triangle's outward unit
  normal. Positive means outside.
- A point with no nearest result reads +1 m.
- The form's faces must point outward (positive signed volume per part). Otherwise every test inverts.
- `push_out(Q, c)`: for each vertex with `sd < c`, set it to `q + n·c`.

### 4.2 Pre-sew

| Step | Rule |
|---|---|
| Groups | Union-find over all sewing pairs. Each connected component of paired vertices is one sewing group, so a shoulder tip shared by three seams has one target. |
| Target | The group's mean position m. If `sd(m) < 8 mm` (clearance 0.008 m), replace it with `q + n·0.008`. |
| Seam displacement | `disp[v] = 1.0·(m − P[v])` for every group member. Group members are fixed. |
| Spread | Jacobi harmonic smoothing on the triangle-edge graph, **400** iterations. A free vertex (not fixed, degree > 0) takes the mean of its neighbours' displacements from the previous iteration. |
| Result | `Q = P + disp`, then `push_out(Q, 0.6·0.008 = 4.8 mm)`. |

A plain wrap is not enough. It leaves front and back shoulders 96–208 mm apart, and their springs close through the
neck (the jacket fell off 1.6 m). Pre-sewing alone leaves ~30 % p95 / 228 % max strain, worst at the cap top.

### 4.3 Relaxation

Position-based Jacobi edge-length projection toward the flat rest lengths, with the sewing constraint enforced and
periodic push-out.

| Item | Rule |
|---|---|
| Edges | unique triangle edges (min, max) |
| Rest length | 2D length (after sim_scale) × MM |
| Gather weight per edge | `wE = (w[i] + w[j]) / 2` (§5.4), or 1 when gathers are off |
| Per iteration | `d = Q[j] − Q[i]`, `L = |d|`, `c = (L < L0 ? wE : 1) · (L − L0)/max(L, 1e-9) · d · 0.5`; accumulate `+c` into i and `−c` into j; then `Q += 0.8 · acc / max(deg, 1)` |
| Sewing constraint | after each iteration, set every group to its mean |
| Push-out | at iterations where `it mod 25 == 24`, and at the block's last iteration: `push_out(Q, 4.8 mm)` |
| Blocks | run blocks of `relax_iters` (default **200**) until p95 of `|ε|` over **plain** edges ≤ **0.12**, or total ≥ `relax_max` (**1600**). With `relax_iters = 0`, skip relaxation. |
| Plain edges | both endpoints have gather weight ≥ 1 |
| `--no-presew` | diagnostic: start from the plain wrap; neither pre-sew nor relaxation runs |

Because of the weighting, a gathered edge may be shorter than its rest length at little cost and can draw up to its
band. Without it, the 530 mm casing started stretched to 727 mm.

The 12 % target is empirical and the outcome is sensitive to it (§8). The bolero reaches it in 200 iterations (11 %),
the trousers in 600 (12 %).

### 4.4 Optional neck anchor (`--anchor neck`, experimental)

Instances whose role is `bodice_back` (or, with no role, whose piece id starts with `back`) contribute the vertices of
their edges whose stem is `neck`. Among those, the vertices with x between the 30th and 70th percentile are pinned at
their start position (mass vertex group). The report records `anchor_vertices` = their count.

---

## 5. Dynamics contract (what any solver must provide)

### 5.1 Rest and start

- **Rest shape = the flat 2D mesh** (after sim_scale). Every rest length is the 2D edge length.
- **Start = the relaxed state** of §4.
- Initial velocity is zero.

### 5.2 Contact

- The reference collider is the dress form inset by `thickness_outer + distance_min − underlayer` = 4 + 3 − 1.5 =
  **5.5 mm**, reported as `collider_inset_mm`. Every torso section semi-axis is reduced by the inset (min 3 mm), every
  limb ring radius too (min 2 mm), the head radius too (min 5 mm), and the crotch cap point is raised by the inset.
- Blender keeps cloth 7 mm outside its collider, so the cloth **mid-surface rests about `underlayer_mm` (1.5 mm:
  shirt plus half the fabric) outside the true form**.
- **Solver-independent requirement: the equilibrium contact distance between cloth mid-surface and true form equals
  `underlayer_mm`.** No girth may gain the 2π·offset bias. Uncompensated, 2π·7 mm = +4.4 cm on every girth: 4 cm of
  ease simulated as skin-tight and 0 cm was ejected.
- Shrinking the contact offsets instead of insetting tunnels cloth through the body.
- Metrics (§6) always use the true form, never the collider.
- Experimental options: `--anatomical` (corrected armpit form) and `--union` (one watertight boolean-union collider).
  Both are recorded in the report (§8, REQ-ARMPIT).

### 5.3 Fabric parameters and mapping

Source order: CLI override, then `pattern.fabric`, then default.

| Parameter | Source key | Default | Reference mapping (Blender mass-spring) |
|---|---|---|---|
| Areal density | `fabric.areal_g_m2` | 200 g/m² | vertex mass = (areal/1000 kg/m²) × total 2D mesh area (m², after sim_scale) / vertex count |
| Tensile stiffness E_t | `fabric.tensile_N_m`, `--tensile` | 8000 N/m | tension = compression = E_t · L, with L = spacing in m |
| Shear | — | — | 0.2 · E_t · L |
| Bending rigidity B | `fabric.bending_Nm`, `--bending` | 3e-5 N·m (single layer) | bending = max(10·B/L, 0.005) |
| Air damping | — | 5 | |
| Solver quality | `--quality` | 10 steps/frame | more is **worse** here (§8) |
| Cloth–body friction | `--friction` | 5 | |
| Collision | — | distance_min 3 mm, collision quality 4, collider thickness_outer 4 mm | |
| Self-collision | — | on, 2 mm | |
| Sewing | — | sewing springs on, max sewing force 0 (unclamped) | a clamp can freeze seams open |

The drafted bolero simulates a lined, interfaced jacket as **one composite layer** (380 g/m², 10 000 N/m, 1.2e-4 N·m;
UNCERTAIN). The drafted trousers use 280 g/m² with the default stiffnesses. On the same pattern, single-layer flannel
never settled (92 % of vertices drifting) and let the open fronts roll in. The composite settled (0.6 % drifting).

The report records `fabric: {areal_kg_m2, tensile_N_m, bending_Nm}` and `area_m2` = r4(area).

### 5.4 Gather zones

- **Gathered seam:** `|ratio − 1| > 0.15`, using the seam's declared ratio.
- **Long side:** the side whose 2D polyline (through its sample indices, after sim_scale) is longer. On a tie, side a.
- **Weight:** for every vertex of the instance that owns the long side's first sample,
  `w = min(w, clip(d / 50 mm, 0, 1))`, where d is the 2D distance to the nearest long-side sample. Weights start at 1.
- **Effect:** in the zone, compression stiffness drops toward 3 % of tension while tension stays uniform. The
  reference sets structural stiffness to `base + w·(max − base)`, with tension base = max = E_t·L and compression
  base = 0.03·E_t·L, max = E_t·L. So **compression = E_t·L·(0.03 + 0.97·w)**.
- **Why:** real gathers are pleats far smaller than the mesh.
- **Report:** `gather_zone_vertices` = the count of vertices with w < 1.
- **Evidence:** without gather zones, the 872 mm gathered trouser top pushed its 530 mm casing open by 40 % and the
  trousers slid off. With zones, gathered bands finish 2–10 % over their rest length; without, +11 to +59 %.

### 5.5 Phases, frames, determinism

| Phase | Frames (default) | Gravity |
|---|---|---|
| sew | `--sew-frames` 45 (frames 1…45) | 0 (frame 1 is the start state) |
| drape | `--drape-frames` 120 (frames 46…165) | 1 g, switched on at frame `sew_frames + 1` |

- The baked sequence `seq` has `sew + drape` frames: `seq[0]` is the start state and `co_sew = seq[sew_frames − 1]`.
- Time step: one frame per animation frame, subdivided by `quality`. The reference frame rate is Blender's default.
- **Determinism is required:** identical inputs give bit-identical baked frames. The reference bake equals stepping
  frame by frame, bit for bit.
- Qualification runs (§7) use `sew 70, drape 0, fold 2`.

---

## 6. Metrics and flags (`sim_report.json`)

### 6.1 Shared definitions

| Symbol | Definition |
|---|---|
| `co` | final shape: mean of the last **15** baked frames (`AVG_FRAMES`). Collision jitter is ~0.2–0.5 mm/frame even at rest. |
| `co_sew` | baked frame `sew_frames` (end of the zero-gravity phase) |
| `wrapped` | the isometric wrap (§3.3), before pre-sew: the "drafted level" in 3D |
| `edges` | unique triangle edges; `ε` over them is computed on `co` |
| contact | `sd < 3 mm` against the true form |
| `rest2d` | 2D vertices after sim_scale (mm) |

### 6.2 Seams (`seams.{id}`)

For each seam record:

| Field | Definition |
|---|---|
| `gap_after_sew_mm` | r2(max over pairs of `|co_sew[a] − co_sew[b]|` in mm); 0 if the seam has no pairs |
| `gap_final_mm` | same, on `co` |
| `seam_strain_pct` | [r2(min ε·100), r2(max ε·100)] over the chords between consecutive samples of side_a and side_b, both sides together |
| `seam_strain_p90_pct` | r2(p90(\|ε\|)·100) over the same chords |
| `gathered` | `|ratio − 1| > 0.15` |

Flags:

- `gap_final_mm > 5.0`: **gating** "did not close".
- `p90 > 8 %` and not gathered: **advisory** "distorted". A seam that walks flat but reads distorted is crowding or
  pull, not a length mismatch.

### 6.3 Landmarks (`landmarks.{id}`): ring strain at girth points

For each fit-spec point with `kind == "girth"`:

1. **Level.** For each listed instance that is simulated, find the piece-y of the level:
   `y = landmark_h(part, point.landmark) − landmark_h(part, placement.y0_landmark) + placement.y0 + point.dy`. Use
   part key `torso`, or `arm_R` / `leg_R` for arms and legs. Note: `y0` is **not** sim-scaled here (§16).
2. **Edges.** Select triangle edges with both endpoints in the instance, `|mid_y − y| < spacing`, and
   `|Δy| < 0.6·|Δx|` (horizontal-ish, in rest2d).
3. If no edge is selected over all instances, the landmark is omitted.

| Field | Definition |
|---|---|
| `ring_strain_mean_pct`, `ring_strain_max_pct`, `ring_strain_p90_pct` | r2 of mean / max / p90 of **signed** ε × 100 over the selected edges |
| `contact_frac` | r3(share of the selected edges' vertices with sd < 3 mm) |
| `clearance_mean_mm` | r1(mean sd over those vertices, mm) |
| `ring_shift_mm` | r1(mean z of those vertices on `co` − mean z on `seq[0]`, mm): vertical travel of the line. Negative ease escapes rather than stretches. |
| `reading` | `TIGHT` if mean > 1.0 or p90 > 3.0; else `SNUG` if contact_frac > 0.6; else `EASY` |

Flags:

- A `TIGHT` reading is advisory.
- It becomes **gating** and is also listed in `tightness_gating` when mean > 2.0 %. −2 cm of ease reads about +3 %,
  while ±1 % flipped between friction 1 and 5 on the same pattern, and p90-only hot spots stay advisory.

### 6.4 Pieces (`pieces.{instance}`)

| Field | Definition |
|---|---|
| `contact_frac` | r3(share of the instance's vertices with sd < 3 mm on `co`) |
| `penetration_max_mm` | r1(max(0, −min sd), mm), against the **true** form |
| `corner_drop_median_mm` | r1(median over the instance's loop corners (§2.3) of (z on `co` − z on `co_sew`), mm); 0 if none |

Flags:

- `penetration_max_mm > 3.0`: **gating**.
- A torso piece whose median corner drop is < −20 mm: **gating** "slid".

### 6.5 Hems, balance, LOST, ride-up

- **Hem edges** are instance edges whose id stem (before the first `.`) is `hem`. For each, keyed `INST/EDGE`:
  - `hem_heights_mm` = r1(mean z of the edge's vertices on `co`, mm);
  - `hem_rise_mm` = r1(that z − mean z on `wrapped`, mm).
- **Torso rise** is the list of hem rises from instances whose placement part is `torso`.
- **Balance.** If some hem key starts with `front` and some with `back`: `hem_balance_mm` = r1(mean front rises −
  mean back rises). It is advisory when |balance| > 10 mm. The field is absent when there are no such keys (the
  trousers' `tr_front…` keys do not qualify).
- **LOST.** If any |hem rise| > 150 mm: `verdict = "LOST"`, and the LOST flag is inserted at index 0 of both `flags`
  and `gating`. `verdict` is absent otherwise.
- **Ride-up.** Only when not LOST: if the torso rise list is non-empty and its mean is > 25 mm, raise the advisory
  ride-up flag.

### 6.6 Bands (`bands.{instance}`)

- **Which instances:** those whose role is `waistband` or `cuff`.
- **Shift:** `shift_mm` = r1(mean z of all the instance's vertices on `co` − mean on `wrapped`, mm). The record is
  `{role, shift_mm}`.
- **Flags, all advisory:**
  - a waistband with shift < −25 mm;
  - a cuff with |shift| > 25 mm, worded "below" or "above".
- **Why advisory:** they fired when the form was wrong, a crotch 40 mm short read only −8 mm, and the casing is
  simulated at the waist girth, so elastic grip is a flat check.

### 6.7 Settledness (drift, not speed)

- Window: `W = min(10, max(2, ⌊len(seq)/6⌋))`.
- Per-vertex drift: `drift = |mean(seq[−W:]) − mean(seq[−3W:−2W])| / (2W)` (m/frame).
- Jitter: `jitter = median` over vertices and the W − 1 consecutive frame pairs in the last window of
  `|Δposition|` (m/frame).

| Field | Definition |
|---|---|
| `drift_frac` | r3(share of vertices with drift > 1e-4 m/frame, i.e. 0.1 mm/frame) |
| `moving_frac` | alias of `drift_frac` (legacy name) |
| `jitter_mm_per_frame` | r3(jitter in mm) |
| `settled` | `drift_frac ≤ 0.02` |
| `drift_by_piece` | per instance, r3(share drifting) |
| `drift_p99_mm_per_frame` | r3(p99 of drift, mm) |

When not settled, an advisory flag names the worst instance (the max of `drift_by_piece`; first on ties). "Not
settled" means rerun and compare, not edit the pattern.

### 6.8 Global and initial strain

- `global_strain_pct` = {p95, max, min} of signed ε·100 over all edges on `co`, r2.
- `initial_strain_pct` = {p95, max} of **|ε|·100** over **plain** edges in the start state, r1. This is diagnostic only.
- `relax_iters` = the total relaxation iterations actually run.

### 6.9 Form crotch excess (trousers)

Only when some simulated instance has placement part `leg`:

- `form_crotch_excess_mm` = r1(form's own front-waist → crotch → back-waist path length − measured
  `total_crotch_length`). It is `null` if the measurement is missing.
- Advisory flag if > 10 mm: trousers then sit lower in the fold-up than on the wearer.
- The reference form solves its crotch ring so the path equals the measurement, unless the straight hip-to-crotch line
  is already longer.

### 6.10 Flag catalogue (exact templates)

| # | Condition | Template | Class |
|---|---|---|---|
| F1 | any \|hem rise\| > 150 | `the garment did not stay on the body ({n} hems moved > 150 mm): usually a girth with negative ease (it is squeezed off, see the validation table) or seams that cannot close; every other sim number is meaningless until this is fixed` | gating, index 0 |
| F2 | gap_final > 5.0 | `seam {sid} did not close ({gap_final_mm} mm): check its placement and lengths` | gating |
| F3 | seam p90 > 0.08 and not gathered | `seam {sid} is distorted in the drape (p90 {p90*100:.0f} %): if its seam walk passes, this is crowding or pull at that seam (look at the stills), not a length mismatch` | advisory |
| F4 | reading TIGHT | `{point_id}: fabric stretched around the body (mean {mean} %, p90 {p90} %)` | advisory; **gating + tightness_gating** when mean > 2.0 |
| F5 | penetration > 3.0 | `{inst} passes {penetration_max_mm} mm into the body` | gating |
| F6 | torso piece, median corner drop < −20 | `{inst} slid {abs(median):.0f} mm down after gravity: nothing is holding it up (too big at the shoulders/waist?)` | gating |
| F7 | waistband shift < −25 | `{inst} ended {abs(shift):.0f} mm below the waist: something drags it down (compare the crotch and rise rows of the validation table; look at the stills)` | advisory |
| F8 | cuff \|shift\| > 25 | `{inst} sits {abs(shift):.0f} mm {below|above} its drafted level: ` + (shift < 0: `its ease lets it slide down the leg, so the blousing is lost (snug the cuff or shorten the leg)`; else `the leg is too short to reach it`) | advisory |
| F9 | \|balance\| > 10 | `hem out of balance: the {front|back} hangs {abs:.0f} mm higher than the {back|front} (a protruding belly lifts the front: add front length)` | advisory |
| F10 | not LOST, mean torso rise > 25 | `the body rides up {mean:.0f} mm with arms at {arm_deg:.0f}° (set-in sleeve pulls the side up; a lower cap or more underarm room frees the arm)` | advisory |
| F11 | not settled | `not settled ({drift_frac*100:.0f} % of vertices still drifting > 0.1 mm/frame, most in {worst} ({by_piece*100:.0f} %), p99 {p99} mm/frame): drape numbers are provisional; rerun with more drape frames and compare` | advisory |
| F12 | form crotch excess > 10 | `the dress form's crotch path is {excess:.0f} mm longer than the measured total crotch length (its hip depth does not allow less): trousers sit lower in this fold-up than they will on the wearer` | advisory |

**Order of `flags`:** F2/F3 per seam in seam order, F4 per landmark, F5/F6 per instance, F7/F8 per band, F9, then
F1 at index 0 **or** F10, then F11, then F12. `gating` keeps the same relative order with F1 first.

**Gating summary:**

| Gating | Advisory |
|---|---|
| LOST | everything else |
| open seam > 5 mm | |
| penetration > 3 mm | |
| mean ring stretch > 2 % (tightness: only where qualified, §7) | |
| torso corners slipping > 20 mm | |

Advisory flags moved with friction and start state in testing, so they are not yet qualified to block a round.

### 6.11 `sim_report.json`, complete field list

| Field | Type | Notes |
|---|---|---|
| `seams`, `landmarks`, `pieces`, `bands` | objects | §6.2–6.6 |
| `flags`, `gating`, `tightness_gating` | string lists | §6.10 |
| `verdict` | `"LOST"` | only when lost |
| `hem_heights_mm`, `hem_rise_mm` | {`INST/EDGE`: mm} | |
| `hem_balance_mm` | number | only with front and back hems |
| `drift_frac`, `moving_frac`, `jitter_mm_per_frame`, `settled`, `drift_by_piece`, `drift_p99_mm_per_frame` | | §6.7 |
| `global_strain_pct` | {p95, max, min} | |
| `pattern` | string | absolute path of the pattern simulated (a port may store a relative path; §16) |
| `vertices`, `faces`, `sewing_pairs` | int | |
| `area_m2` | number | r4 |
| `measurements_sha256` | hex | sha256 of the measurements file bytes |
| `collider_inset_mm` | number | 5.5 by default |
| `fabric` | {areal_kg_m2, tensile_N_m, bending_Nm} | |
| `initial_strain_pct` | {p95, max} | §6.8 |
| `presew`, `arm_deg`, `anchor_vertices`, `gather_zone_vertices`, `relax_iters`, `anatomical_form`, `union_collider` | | |
| `frames` | {sew, drape} | |
| `form_crotch_excess_mm` | number \| null | only with leg pieces |
| `animation` | {pc2, frames, fold_frames, seam_legend {family: [r,g,b] r2}} | |
| `stills` | {front, side, back, three_quarter, strain_front, strain_back: path} | with `--stills`/`--render` |
| `video` | [paths] | with `--render` |
| added by the CLI | `wall_s` (r1 seconds) by `fab`; `tightness_qualified` by `round` (§7) | |

The simulator's stdout ends with the line `SIM_REPORT {"gating", "flags", "settled", "landmarks"}` (JSON).

---

## 7. Qualification (`fab qualify`) and tightness gating

### 7.1 Known-answer bands

`band_pattern(meas, ease, height = 60, fabric)` builds a front and a back chest band:

- **Size:** each band is a rectangle `w × 60` with `w = (chest + ease)/2`. Vertices a(0, −30), b(w, −30), c(w, 30),
  d(0, 30); edges `bottom` a→b, `right` b→c, `top` c→d, `left` d→a.
- **Grain** `[[w/2, −20], [w/2, 20]]`, cut qty 1.
- **Placement:** torso, θ 0° (front) / 180° (back), x0 = w/2, y0_landmark `chest`, y0 0, clearance 15.
- **Seams:** `side_R` (band_front/left ↔ band_back/right) and `side_L` (band_front/right ↔ band_back/left).
- **Fabric:** the given one, else `{id: test, kind: woven, areal_g_m2: 200}`.
- **Header:** `style` = `KAT_BAND_{ease:+d}`; `meta = {origin: QUALIFICATION, ease_mm}`.
- **Tested:** the summed flat width at y = 0 equals chest + ease to 0.01 mm, and both seams walk.

**Run.** Use eases −20, 0, +20 mm. Each band is in `OUT/band_{ease:+d}/` (`band_-20`, `band_+0`, `band_+20`) with its
`pattern.json`, a `fit_spec.json`, and `sim/` (no stills; `--sew-frames 70 --drape-frames 0 --fold-frames 2`). The
fit spec is `{schema: fabric-fit/1, points: [{id: chest, kind: girth, instances: [band_front, band_back], landmark:
chest, body: chest, target_mm: [−50, 0, 50]}]}`.

**Reading per band.** Let `lm` = `landmarks.chest` and `shift = |ring_shift_mm|`:

| Ease | Expected label | Pass rule |
|---|---|---|
| −20 | `TIGHT` | stretched (reading TIGHT **and** mean > 2.0) **or** escaped (shift > 15 **and** contact_frac < 0.3; reading is then shown as `ESCAPED`) |
| 0 | `SNUG_OR_TIGHT` | reading ∈ {SNUG, TIGHT} and shift ≤ 15 |
| +20 | `EASY_OR_SNUG` | reading ∈ {EASY, SNUG} and mean < 1.0 (missing → fail) and shift ≤ 15 |

**Output.** `OUT/qualify.json` = `{pass, rows: [{ease_mm, reading, ring_strain_mean_pct, ring_shift_mm, contact_frac,
expected, pass, max_seam_gap_mm}], fabric_key, measurements_sha256}`, where `fabric_key = [areal_g_m2, tensile_N_m,
bending_Nm]` of the pattern's fabric (null when no pattern was given).

Each band prints the line `ease ±E mm: READING mean ring strain X %  contact C  shift S mm  -> PASS|FAIL (expected L)`.
The run then prints `QUALIFIED` or `NOT QUALIFIED: do not let the sim gate on tightness for this body/fabric`. Exit
status 0 / 2.

**Recorded answer** (92 cm reference wearer, composite jacket fabric): −20 escapes (contact 0, slides off its level);
0 stays in place in full contact (0.98, mean stretch 1.8 %); +20 sits easy in place. Result: QUALIFIED.

### 7.2 `qualification(ws, measurements, fabric)` → {pass, why}

Check in order:

| Condition | `why` |
|---|---|
| no `WS/qualification/qualify.json` | `no qualification run in this workspace (`fab qualify --ws WS`)` |
| unreadable JSON | `… is unreadable (an interrupted run?): rerun …` |
| `pass` false | `the qualification run did not pass: the sim cannot tell tight from easy here` |
| sha256 of the current measurements file ≠ recorded | `the measurements changed since the qualification run: rerun `fab qualify`` |
| `fabric_key` ≠ the current pattern's | `the fabric changed since the qualification run: rerun `fab qualify`` |
| otherwise | pass: `qualified for this body and fabric` |

### 7.3 `gate_tightness(sim, qual)`

1. Set `sim.tightness_qualified = qual.pass`.
2. Let `tight` = the entries of `tightness_gating` that are also in `gating`.
3. If `tight` is empty or the qualification passed, return nothing.
4. Otherwise remove `tight` from `gating` (they stay in `flags`) and return the note
   `{n} tightness flag(s) kept advisory: {why}`.

---

## 8. Failure modes as requirements, and known-answer tests

These are the lessons the reference recorded. A new solver must meet the requirement. The baseline column is what the
reference measured.

| ID | Requirement for a new solver | Reference baseline (evidence) |
|---|---|---|
| REQ-OFFSET | No contact-offset girth bias. A tube of girth G + e over a cylinder of girth G must read: e = −2 cm tight (strain > 1 % or seam gap > 1 mm), e = 0 contact all round, e = +2 cm loose. Its mean clearance must equal (C − G)/2π − cloth half-thickness. | Uncompensated: +4.40 cm bias (2π·7 mm), 0 cm ease ejected, +4 cm read skin-tight. With inset: −2 → +3.05 % ring strain and 1.9 mm gaps; 0 → +0.10 % in contact; +2 → loose. |
| REQ-KAT-BANDS | `fab qualify` passes (§7.1) on every new body/fabric. Target: the −20 band holds its level and reads stretch, rather than escaping. | Passes on the 92 cm reference with the −20 band **escaping** (contact 0). |
| REQ-START | Start-state insensitivity. The same pattern from different relaxation depths (or two different pre-sew push-outs) settles each time, with identical gating flags. Report a **two-start spread** as the error bar. | Bolero 200 it → 11 % → 0.1 % drifting; 800 it → 9 % → 54 % drifting. Trousers 200 → 18 % → 52 % drifting and two seams 5.1/7.1 mm open; 600 → 12 % → 1.2 %; 1000 → 10 % → 20 %; 1500 → 8 % → 2.2 %. Across three clean trouser runs: casing ±4 mm, cuff 11 mm, hem 13 mm. |
| REQ-NO-LAUNCH | A high-strain start relaxes rather than exploding. | An unrelaxed pre-sew (30 % p95 / 228 % max) launched the jacket 1.3–1.5 m upward. |
| REQ-JITTER | At rest, jitter ≤ 0.2 mm/frame. More solver work (substeps or iterations) must never raise jitter or drift. | Settled bolero 0.18 mm/frame. `--quality 20`: 0.10 → 0.99 mm/frame, 75 % drifting, and from a second start it came off the form. |
| REQ-FRICTION | Tightness readings robust to friction: chest mean stretch at friction 1 vs 5 within 1 %, so a ±2 % gate means something. | Same pattern: chest mean −0.44 % (f5) vs +1.07 % (f1); hem balance +34 vs +23 mm. |
| REQ-ARMPIT | Anatomical form (armpit at C7 − armscye depth, sloped shoulder roof) drapes stably. Bolero at three sizes (80 / 92 / 116 cm reference wearers): no gating flags and hem rise within ±15 mm of the drafted level at 35°. Only then may balance and ride-up gate. | Default form armpit ~45 mm high; hems +42–52 mm at 35°, 56 mm at 50°. Anatomical: fell off; with roof slid ~100 mm; neck-anchored flipped up 200–330 mm, jitter 6–14 mm/frame; union collider launched 0.7–0.9 m. Default form at 80–92 cm: hem rise 22–61 mm; at 116 cm 48–121 mm. |
| REQ-ELASTIC | Elastic hold judgeable. An elastic ladder of 0.85, 0.95, 1.0 and 1.2 × waist reads hold / hold / hold / fall, at low and high friction (the reference used 0.5 and 5). | Not judgeable: 1.2 fell (LOST) in one run and stayed up on friction in the next; 0.95 and 0.85 slid 30–155 mm and one leg was torn by the churn. The casing is therefore simulated at the waist girth, and `elastic_hold` is a flat row. |
| REQ-CROTCH | Sensitive to rise. Trousers drafted with the crotch 40 mm short must raise a flag. | Casing only 8 mm low, no flag. Rise is owned by the flat `crotch` row. |
| REQ-GATHER | Gathered bands realise their declared length: casing and cuffs within a few % of rest length, without hand-tuned gather zones. | With gather zones +2–10 % (casing 560–584 mm for 530; cuffs 234–245 for 229.5). Without, +11 to +59 % and nothing settled. |
| REQ-SEAM-SIGNAL | A 10 % seam-length mismatch is visible in per-seam strain. | 10 % mismatch: gap 0.74 mm and global p95 0.77 % (both "pass"); per-seam edge strain −6.2 / +6.7 %. The flat seam walk owns lengths. |
| REQ-LOST | Negative ease at a girth ends as LOST (or, with a better contact model, stays on and reports hoop strain ≈ −e/G). | AI-sheet round 0, best-case scaled to the body (chest ease −27 mm): LOST. |
| REQ-DETERMINISM | Identical inputs give identical outputs (native ↔ native, and native ↔ wasm if offered). | Bake deterministic: round 3 reproduced an earlier run to the last digit. |
| REQ-STABLE-SIZES | Converged drafts at 80 / 92 / 116 cm stay on (no LOST) at default settings. | 116 cm corrected style came off at 200 relaxation iterations and stayed on at 300. |
| REQ-NO-EASE | Never report ease from cloth. | Tape girth around cloth biased −0.4 to −2.1 cm, ±1.7 cm with bending. |

**Golden readings on the 92 cm reference wearer** (one-sample values, regression only, never a tight equality):

| Run | Readings |
|---|---|
| Bolero rounds 5–8 (composite fabric, defaults) | no gating flags; settled (0.1 % drifting; jitter ~0.18 mm/frame); advisory: balance 11–14 mm, ride-up 49–52 mm (form bias), chest mean stretch 0.4–0.8 %; exports PASS 128/128 (183/183 with soutache DST); ~2100 vertices. Earlier round 3 (single layer, relaxed start): corner drift ≤ 4 mm, seams ≤ 3.4 mm, no penetration, global strain p95 5.3 %. |
| Trousers round 2 (defaults) | no gating flags; settled (1.2 % drifting); seams ≤ 2.9 mm; penetration 0.5 mm; casing −10 mm; cuffs +11 / +15 mm; seat EASY; 3738 vertices; 17 seams; advisory: one cuff end (9 %) and casing join (31 %) seams distorted, back crotch seam 9 % |

The spread between rounds 5 and 7, the same pattern, is the noise floor: balance 11 → 14 mm, chest 0.4 → 0.8 %.

**Additional closed-form tests a native solver should add** (none exist in the reference):

- ring on a cylinder with a hanging load: it holds iff W < 2πμT;
- ring on a cone: it slides iff tan α > μ;
- membrane anisotropy and rigid-rotation zero energy;
- a cantilever bending-length test.

---

## 9. Output formats

### 9.1 `fold.pc2`

- **Header**, 32 bytes, little-endian:

  | Bytes | Field |
  |---|---|
  | 12 | `"POINTCACHE2\0"` |
  | int32 | version = 1 |
  | int32 | num_points = vertex count |
  | float32 | start_frame = 1.0 |
  | float32 | sample_rate = 1.0 |
  | int32 | num_samples = frame count |

- **Body:** float32 x, y, z per point, frame-major, in m and world axes.
- **Frames** = `fold_frames + 18 + sew_frames + drape_frames` (default 70 + 18 + 45 + 120 = **253**). The last
  `sew + drape` samples are the baked frames, which lets any run be re-analysed without the solver.

### 9.2 `mesh_map.json`

| Key | Value |
|---|---|
| `vertices` | int |
| `inst_range` | {inst: [lo, hi)} |
| `corners` | {"INST:VERTEX": global index} |
| `edges` | {"INST/EDGE": [global indices, both ends]} |
| `seams` | {seam_id: {side_a: [...], side_b: [...]}} |
| `frames` | {fold, zip: 18, sew, drape} |
| `units` | `"m"` |

### 9.3 Animation layout and fold kinematics

| Segment | Frames | Content |
|---|---|---|
| board | — | Instances laid left to right in selection order, 40 mm gaps, centred on x = 0. Board plane y = −0.55 m. z = (piece y − its min y) + 0.15 m. |
| fold | `--fold-frames` (70) | Per instance: frames before its stagger start stay on the board. Start = ⌊idx·0.35·n/(N−1)⌋ (0 if N = 1). With u = (f − start)/max(1, n − start − 1): **rigid flight** for u < 0.55, then **roll-up** for u ≥ 0.55 (details below). |
| zip | 18 | `wrapped + w·(start − wrapped)` with w = ½ − ½cos(πt), t = linspace(0, 1, 18): wrap → pre-sewn/relaxed start |
| sew + drape | baked | `seq` |

**Rigid flight** (u < 0.55):

- `w = ½ − ½cos(πu/0.55)`.
- The rotation is the quaternion slerp from identity to the Kabsch rotation that maps the board copy onto the k = 0
  roll.
- The centroid moves linearly by w toward its target and rises by `0.08·sin(πw)` m.
- Never interpolate vertices linearly: a vertex lerp shrinks a piece rotating 90° to 71 %.

**Roll-up** (u ≥ 0.55): `k = ½ − ½cos(π(u − 0.55)/0.45)`, positions = `roll(k)` (§3.6).

### 9.4 Colour attributes (per vertex)

- **`seam_rgb`.** The seam family is the seam id with its last `_suffix` removed (`side_R` → `side`, `cap_f_R` →
  `cap_f`). Families are sorted, and colour = `SEAM_COLORS[index mod 10]`. Every vertex in a sewing pair gets its
  family colour. Other vertices get the base colour: (0.07, 0.10, 0.30) if the pattern `style` contains `BOLERO`,
  else (0.55, 0.06, 0.06).
- **`SEAM_COLORS`** (RGB 0–1, same order in the 2D preview):

  | Index | Colour |
  |---|---|
  | 0 | (0.95, 0.75, 0.10) |
  | 1 | (0.10, 0.80, 0.90) |
  | 2 | (0.95, 0.30, 0.80) |
  | 3 | (0.30, 0.95, 0.30) |
  | 4 | (1.00, 0.50, 0.10) |
  | 5 | (0.60, 0.50, 1.00) |
  | 6 | (0.00, 0.50, 0.45) |
  | 7 | (0.90, 0.20, 0.20) |
  | 8 | (0.50, 0.30, 0.10) |
  | 9 | (0.15, 0.30, 0.90) |

  There are ten, none white, because the trousers have nine families.
- **`strain_rgb`.** For each vertex, take v = the mean final ε of its incident edges and `t = clip(v/0.03, −1, 1)`.
  Then RGB = (t > 0 ? 1 : 1 + t, 1 − |t|, t < 0 ? 1 : 1 − t). This is white at 0, red at +3 %, blue at −3 %.

### 9.5 Stills and videos

- **Camera:** at `(tx + d·sin az, ty − d·cos az, height)`, tracking the target; 50 mm lens.
- **Stills target:** `(0, 0, clamp(zc, 0.25·head_top, 0.8·head_top))`, where zc is the mid z of the last baked frame.
  Distance 2.0 m, height target + 0.12 m.
- **Stills:**

  | File | Azimuth | Material |
  |---|---|---|
  | `still_front.png` | 0° | seam colours |
  | `still_side.png` | 90° | seam colours |
  | `still_back.png` | 180° | seam colours |
  | `still_three_quarter.png` | 35° | seam colours |
  | `still_strain_front.png` | 0° | strain colours |
  | `still_strain_back.png` | 180° | strain colours |

- Default resolution 960×540.
- **Video** (`--render`): azimuth 20°, target (0, −0.25, 0.5), distance 2.6 m, height 0.8 m. H.264 MP4 of all frames,
  `fold_0001-NNNN.mp4`.
- **Side by side** (`round --render`): `fold_side_by_side.mp4` = the 2D seam-coloured preview scaled to the video
  height, beside the video. The still loops for exactly the video's frame count (`-loop 1` with `-shortest` alone
  never ends). H.264, CRF 20, yuv420p.
- Render-only cosmetics: weld coincident sewn vertices, 1 level of subdivision, 1.5 mm solidify, smooth shading.

### 9.6 `garment.blend` (reference only)

The reference saves the Blender scene at the last frame. Consumers read:

- object `GarmentAnim`: board-pose mesh in vertex order, triangles, sewing pairs as loose edges, `seam_rgb` and
  `strain_rgb`, and a PC2 cache modifier (start frame 1);
- objects `Form_*`: the true form parts, or `Form_body` with `--union`;
- `Collider_*`: hidden.

A port replaces this file with a portable scene record holding the same content (§14).

### 9.7 3D viewer: `fabric-fold-viewer/1`

`fold_viewer.html` is one self-contained page: the template plus a JSON payload.

| Field | Content |
|---|---|
| `schema` | `"fabric-fold-viewer/1"` |
| `title` | pattern `meta.title`, else `style`, else `"fold-up"` |
| `vertices` | int |
| `frames` | kept frame count |
| `source_frames` | PC2 frame count |
| `lo`, `hi` | [x, y, z] bounds over all kept frames, r6, m |
| `positions` | base64 of uint16 LE, frames × vertices × 3; q = round((x − lo)/max(hi − lo, 1e-9)·65535) |
| `triangles` | base64 uint32 LE, flat (polygons fanned) |
| `colours` | {seam_rgb, strain_rgb}: base64 uint8 RGB per vertex (round(clip·255)) |
| `phase` | one label per kept frame |
| `body` | {vertices: base64 float32 LE xyz (m, z-up), triangles: base64 uint32}, the true form parts in name order |
| `report` | {gating, advisory = flags − gating, settled, arm_deg} |

- **Frame selection** (`--frames`, default 150): the phase starts are `cumsum([0, fold, zip, sew])` = [0, 70, 88, 133].
  Keep = sorted unique(round(linspace(0, n − 1, min(150, n))) ∪ the phase starts ∪ {n − 1}).
- **Phase labels:**

  | Phase | Label |
  |---|---|
  | 0 | `Flat pattern: the pieces leave the board and roll up` |
  | 1 | `Seams pulled together` |
  | 2 | `Sewn (no gravity yet)` |
  | 3 | `Draped under gravity` |

  Phase index = searchsorted(starts, frame, right) − 1.
- **Embedding:** the JSON replaces the literal placeholder `/*__FOLD_DATA__*/null`, with every `</` written as `<\/`.
  The page `<title>` is the title text before any `(`, each word's first letter upper-cased, plus ` Fold-up`
  (fallback `Pattern`).
- **Page behaviour:**
  - Three.js 0.160.0 via import map from jsdelivr.
  - Axis map (x, y, z) → (x, z, −y); dequantise as `lo + q·(hi − lo)/65535`.
  - 24 fps × speed (0.5 / 1 / 2). Hold 2.5 s on the last frame, then loop. Under reduced-motion the page opens paused.
  - Seam/strain colour toggle, form on/off, orbit controls.
  - Verdict line: `N gating flag(s)` or `no gating flags` · `M advisory` · `settled` / `not settled` · `arms at D°`.
  - Light and dark tokens.
  - Test hooks: `window.__fold = {frames, setFrame, pause}`; `body.dataset.ready` = `"1"` or `"error"`.
- Sizes: ~2.8 MB (jacket), ~4.8 MB (trousers).

**Rust packaging exception `viewer-offline-r160`.** The reference's import map above needs
jsdelivr even when the HTML is opened locally. An offline Chromium run of its generated page
requests both CDN modules and never creates a canvas. Rust embeds the same pinned Three.js
0.160.0 and OrbitControls modules, plus their MIT notice, as data-URL imports in each generated
page. The payload, axes, playback and controls remain as specified; HTML bytes and sizes change.
The upstream modules and hashes are recorded in `ftx-drape/CONTRACT.md`. Offline rendering and
controls must pass through `tools/check_web.cjs --viewer HTML` without HTTP(S) requests.

### 9.8 `sim-points`

- **Input:** `mesh_map.json` and `fold.pc2`. Final = mean of the last 15 frames × 1000 (mm).
- **Rows:** for each corner key (sorted as strings) of an instance whose placement part is torso (also when the
  instance has no placement):
  - `drafted_z = landmark_h(torso, y0_landmark | waist) − y0 + vertex y` (unscaled);
  - `final_z`, `rise = final − drafted` (r1).
- **Table:** columns `point`, `drafted z`, `final z`, `rise`.
- **Reading:** shoulders at the drafted level with hems high means something props the garment up from below.
  Everything low means nothing holds it.

### 9.9 Outfit (`outfit.json`)

- **Inputs:** measurements; one or more sim directories; an optional fez pattern; `--turntable`; `--seams` (keep seam
  colours); resolution 960×960.
- **Pose:** arm angle and anatomical flag come from the **first** sim's report. If the poses differ, append
  `; the runs used different form poses …` to the note.
- **Form:** built from `--measurements` at zero inset.
- **Garments:** each run's garment is frozen at its last PC2 frame (0-based index count − 1). Plain cloth is (0.05,
  0.07, 0.22) if the style contains BOLERO, else (0.55, 0.05, 0.05).
- **Per-garment record:** `{sim, style, gating, settled, bands, body_hem_rise_mm, same_body}`:
  - `body_hem_rise_mm` = r1(mean of the hem rises whose key starts with neither `sleeve` nor `tr_`);
  - `same_body` = whether the sha256 of the run's measurements equals the sha256 of `--measurements`. Older runs fall
    back to `WS/measurements.json` when the sim directory is `WS/rounds/NNN/sim`. It is null when unknown, and false
    appends a note.
- **Hat:** a frustum with r_b = base_girth/2π, r_t = top_girth/2π and height `height_mm` (from `meta.solved`). Base at
  `head_centre`, centred on the neck's y, 64 segments, top cap, 2 mm solidify, felt red. Record `{pattern,
  base_mm_above_floor, top_mm_above_floor, head_top_mm}`.
- **Cameras:** target (0, 0, ½·head_top), distance 1.8·head_top, height target + 0.1.
  - Stills `outfit_{front 0°, three_quarter 35°, side 90°, back 180°}.png`.
  - Turntable: 72 frames, 360°, `outfit_turntable_*.mp4`.
- **Note** starts `garments were simulated separately: overlaps between them are not resolved; read proportions only`.
- **File:** `outfit.json` = `{garments, hat, pose {arm_deg, anatomical_form}, note, stills, turntable?}`.

---

## 10. Vectorizing (`vectorize.py`)

The vectorizer turns a raster pattern sheet (AI image, scan, screenshot) into an **UNSCALED** `fabric-pattern/1`
draft (1 unit = 1 crop px, y up, CCW), then audits it scale-free. The sheet supplies piece inventory and curve
character only. Metric defects are fixed in vectors and are **never sent back to the image model**.

### 10.1 Commands

| Command | Arguments (defaults) | Output | Exit |
|---|---|---|---|
| `trace IMAGE --out D` | `--crop x,y,w,h`; `--min-area-frac 0.01`; `--channel auto` (auto, bgdist, dark, light, gray, sat, r, g, b, edge); `--invert` / `--no-invert`; `--threshold otsu` (otsu, adaptive); `--bg R,G,B`; `--stroke PX`; `--close 3`; `--open PX`; `--corner-deg 35`; `--soft-deg 70`; `--simplify-frac 0.003`; `--smooth 1.2`; `--erase X,Y,W,H` (repeat); `--no-dashes` | `D/draft.pattern.json`, `D/overlay.png`, `D/mask.png`, `D/labels/pN.png`. Prints {pieces: [{id, mode, stroke_px, vertices, edges, curved, notches, grain, touches_border, area_px}], files, bg_rgb, retries} | 0, or 3 if no piece |
| `annotate DRAFT --names N --out P` | `--overlay PNG` | validated pattern; prints {out, pieces, seams, notes} | 0; 2 on error (message on stderr) |
| `audit P --out A` | `--labels L`, `--image IMG`, `--tol-mm 3`, `--median-seam-mm 250`, `--label-tol 0.03` | `A/round0_audit.json`, `A/round0_audit.png`; prints {summary, scale_basis, findings ["F# [C] msg"], files} | 0; 2 on error |
| `scale-fit P --target REF=LEN … --out S` | targets accept units (`=10.5in`, `=230`) | FITTED pattern; prints {out, k, mm_per_px, targets, rms_pct, warning} | 0; 2 on error |

### 10.2 Trace pipeline (exact constants)

1. **Load.** Load as BGR 8-bit. Alpha (RGBA, LA, P, PA, BGRA) is composited on white; grey becomes 3 channels.
2. **Crop.** Round `--crop` to integers and clamp to the image.
3. **Background.** `--bg`, else the frame estimate. Take a 3-px border, quantise each channel `>> 4` (16 levels), find
   the most frequent bin, and use the per-channel median of the pixels in it.
4. **Erase.** Paint each `--erase` rectangle (source pixels, shifted by the crop origin, clipped) with the background
   colour.
5. **Channel.** Compute the channel to threshold:

   | Channel | Value | Polarity known? |
   |---|---|---|
   | auto / bgdist | Euclidean distance in 8-bit Lab from the background's Lab, clipped to 0–255 | yes |
   | dark | 255 − grey | yes |
   | light | grey | yes |
   | gray | grey | no |
   | sat | HSV saturation | no |
   | r, g, b | that colour channel | no |
   | edge | max over Lab channels of the 3×3 morphological gradient | yes |

6. **Binarize.** Compute Otsu. Frame foreground share = fg share in the 2-px border of the Otsu mask. Flip = the
   `--invert` value if given, else (polarity unknown **and** frame fg share > 0.5).
   - Adaptive: on (flipped ? 255 − ch : ch), Gaussian, block = odd(min(H, W)/20, min 15), C = −10.
   - Then close with an elliptical kernel of `--close` px (if ≥ 2).
   - Diagnostics: {otsu_threshold, flipped, frame_fg_frac (r3), method}.
7. **Piece contours** (§10.3), then **dashed-line bridging** (§10.5), **additively**.
8. **Panel retry.** On the first attempt with `--channel auto` and no `--bg`: if a candidate covers > 60 % of the crop,
   the panel is the real background. Take the bin mode of the crop pixels inside that candidate that are mask
   foreground, re-paint the erased areas, and retry once. Retry note: `panel detected: background re-estimated inside
   it`.
9. **Reading order.** Sort by bbox-centre y. Start a new row when cy − (row's first cy) > 0.5 × median bbox height.
   Within a row sort by cx. Ids are `p1…pN`.
10. **Per piece:** stroke-centre polygon (§10.4), corners and edges (§10.6), marks (§10.7), flags.
    - `touches_border`: the bbox is within 2 px of the crop edge, **or** the polygon raster dilated 7×7 overlaps an
      erased area (that case also sets `touches_erased`).
    - Confidence: 0.85 if ≥ 3 sharp or soft corners, else 0.5; 0.3 if touching.
11. **Structure check.** The draft must pass the pattern model's structure check, otherwise raise
    `traced draft failed structure checks: …`.

### 10.3 Piece contour extraction

- Use two-level contours (outer plus holes). The area floor `amin = min_area_frac·H·W`.
- Take the top-level contours with area ≥ amin, largest first. Skip a contour whose centroid lies inside an
  already-accepted solid (a large mark inside a piece).
- **Mode:** compute `fg_frac` = mask share inside the filled contour. It is `ring` (an outline stroke) if < 0.5, else
  `fill`.
- **Ring stroke width w:**
  - `--stroke` if given;
  - else, if the largest hole has area > 0.4 × the outer area, `w = 2(Ao − Ai)/(Po + Pi)`, with perimeters of
    0.7-px-simplified closed contours. This is exact for parallel curves.
  - If w is outside [0.8, 0.06·√Ao], fall back to the distance transform: take the ridge pixels of the ring's
    distance transform (dt ≥ 3×3 dilation − 1e-6) and set w = 2·median − 1 (2.0 if there are none).
  - Fill pieces: w = `--stroke` or 0.
- **Open/close.** Kernel k = `--open` if given, else ring: odd(1.5w + 1) (min 3), fill: 5. If k ≥ 2, the filled solid
  is opened, then closed, with an elliptical k kernel. The opening cuts touching dimension arrows; the closing heals
  slits from light arrows and ticks.
- **Output:** external contours of the result with area ≥ amin become pieces `{contour, mode, stroke_px r3,
  stroke_how (given | area | distance-transform | none), fg_frac r3, open_px}`.

### 10.4 Stroke centre (cut line)

1. Resample the contour closed at 1 px.
2. Smooth with a circular Gaussian (σ = `--smooth` 1.2 px, radius ⌈3σ⌉; skipped if σ ≤ 0 or fewer than 8 points).
3. Make a valid polygon, then offset with a mitre join (limit 4):
   - ring: by **−w/2** (the two half-pixel contour biases cancel);
   - fill: by **0.5 − w/2**.
4. If the offset gives several polygons, keep the largest.
5. Flip y (`H − y`) and orient CCW.

### 10.5 Dashed-line bridging (dashed fold edges)

- Prompts ask image models for a dashed fold line, and a dashed edge leaves the outline ring open, so the piece used to
  vanish. Find long collinear runs with a probabilistic Hough transform.
- **Hough settings** (s = min(H, W)): resolution 1 px and π/360; threshold max(20, ⌊0.05s⌋); min length ⌊0.12s⌋;
  max gap ⌊0.025s⌋.
- **Drawing.** A run is drawn solid only when the mean ink coverage along it is **< 0.85**, so solid edges, arrows
  and text are left alone. Width = max(2, round(2·median distance transform of the ink)), or 3 with no ink.
- **Adoption.** Contours found on the bridged mask are adopted only if their bbox IoU with every existing piece's bbox
  is < 0.3. Existing pieces and the mask used for marks are untouched. Diagnostic `dash_bridged_pieces`.
- **Tested:** a 350×540 px rectangle with a dashed right edge (14 px dash, 12 px gap) is recovered within 3 % area.
  Without bridging only the closed piece is found.

### 10.6 Corners and edges

- **Resampling:** size = max(bbox w, h); ds = max(0.5, size/1500) px. Resample the closed loop at ds (at least 16
  points).
- **Sharp corners:** let k = max(2, round(max(8 px, 0.02·size)/ds)). The turning angle at i is the signed angle between
  chords (Q_i − Q_{i−k}) and (Q_{i+k} − Q_i). Accept greedily by |turn| descending while ≥ `corner_deg` (35°), keeping
  candidates whose circular index distance from every accepted corner is > k.
- **Soft corners** (rounded): with k3 = 3k, only if `soft_deg` > 0 and N > 4·k3. Accept greedily by |turn over k3|
  while ≥ `soft_deg` (70°), with distance > k3 from all accepted corners.
- **At least 3 vertices:** with no corner, take the topmost point; otherwise take the point farthest (circularly) from
  the accepted ones. Kind `split`.
- **c1** is the corner minimising (x − min x) + (max y − y), the top-left one. Vertices `c1…cn` (r3) and edges `e1…en`
  run CCW from c1.
- **Edge type.**
  1. Trim the corner zones from the run, `trim = min(0.06·arc, 0.02·size + 2)`, and fit a total-least-squares line to
     the rest.
  2. If the max deviation > `max(1.5 px, simplify_frac·size)`, the edge is a relative `poly`: Douglas–Peucker at
     `max(0.3, simplify_frac·size/6)` (0.05 % of size) with endpoints excluded. Control points are `[t, h]` relative to
     the chord (r5).
  3. Otherwise it is a `line`.
- **Corner record:** `trace.corners.cN = {turn_deg r1, kind (corner | soft | split)}`.

### 10.7 Marks (best effort; each carries `confidence`)

**Region.** inset = ring: w/2 + 1.5; fill: 2 + w/2. Ink = (ring: mask fg; fill: mask bg) inside the polygon eroded by
the inset. dt = the distance transform inside the polygon (distance to the outline). Take 8-connected components of
area ≥ max(4, 0.25·max(w, 1)²), each with: principal axis, `long`/`short` extents (+1), solidity, and min/median/IQR of
dt. depth = max(3w, 0.025·size, 4).

| Mark | Rule | Output |
|---|---|---|
| Dashes (first) | Candidates: long ≤ 0.08·size, long ≥ 1.8·short, long ≥ max(2w, 4). Pair two candidates when the centroid distance is ≤ 3.5·max(long) and the axes and the joining vector are all within 12° of collinear. Groups need ≥ 4 dashes. | `{kind: dashed, pts [p0, p1], n_dashes, confidence min(0.9, 0.4 + 0.08n)}`. Becomes `fold_candidate` with `near_edge` and `offset` (r2) when a straight edge is parallel within 6° and both endpoints are < 0.06·size from its line (nearest wins). |
| Grainline | Unused components with long ≥ 0.15·size and long/short ≥ 6. A component touching the outline (dmin ≤ inset + 2.5) is instead `{kind: line, conf 0.3, note "long straight mark touching the outline (dimension arrow?)"}`. Arrowheads: end width (\|proj\| > 0.38L) > 1.6 × mid width (\|proj\| < 0.25L). Longest wins. | `grain = [p0, p1]` and `grain_meta = {angle_deg (mod 180, r2), arrowheads, confidence (0.7 with arrows, else 0.45; −0.15 if several candidates), source: trace}`. Others become `line` conf 0.3, "another long straight mark (dimension arrow?)". |
| Grain fallback | Hough on ink with dt > 0.06·size: threshold max(20, ⌊0.08·size⌋), min length 0.2·size, gap max(3, ⌊0.01·size⌋); longest | grain conf 0.35, arrowheads null, source `trace-hough` |
| Parallel inner line | Unused, long ≥ 0.3·size, long/short < 6, dmed < 0.1·size, IQR < 0.35·max(dmed, 1) | `{kind: parallel_line, offset dmed r2, conf 0.5, note "inner line at ~constant offset (seam line?)"}` |
| Notches | Unused, dmin ≤ inset + 2, long ≤ 2.5·depth, dmed ≤ inset + depth, area ≥ max(6, 0.5w²), solidity ≥ 0.6. Nearest edge; s = normalised projection; skip s < 0.02 or > 0.98 (stroke joins). Triangle = hull simplified at 0.15·long has 3 points. | `{id nK, edge, s r4, type V, confidence 0.45 + 0.25·triangle + 0.1·(solidity > 0.8), source trace}` |

Unverified detections go to `trace_marks` and are never exported. `internal` holds drawing instructions only. If mark
detection raises, `trace.marks_error` = repr and the outline is kept.

### 10.8 Draft fields written by `trace`

**Piece fields:**

| Field | Value |
|---|---|
| `id`, `name` | `pN` |
| `cut` | `{qty: 1}` |
| `vertices`, `edges` | §10.6 |
| `notches`, `trace_marks`, `grain`, `grain_meta` | §10.7 |
| `trace` | {mode, stroke_px, stroke_how, open_px, fg_frac, bbox_px [x, y, w, h], touches_border, touches_erased, confidence, `to_px`, corners} |

`to_px` = `[[1, 0, x0], [0, −1, y0 + H]]` is the affine map from piece coordinates to source pixels. Later mirroring
and scaling keep it current.

**Pattern fields:**

| Field | Value |
|---|---|
| `schema`, `units` | `fabric-pattern/1`, `mm` |
| `name` | `draft traced from FILE` |
| `meta.scale_status` | `UNSCALED` |
| `meta.px_per_unit` | 1.0 |
| `meta.units_note` | text |
| `meta.source` | {image, crop [x0, y0, W, H], image_size} |
| `meta.trace` | {channel, threshold, invert, min_area_frac, corner_deg, soft_deg, simplify_frac, smooth_px, close_px, erase, retries, dash_bridged_pieces?, bg_rgb, n_candidates}. `threshold` holds the binarize diagnostics (§16). |
| `meta.generator` | `vectorize.py trace` |
| `seams` | `[]` |

### 10.9 Images

- **Canvas.**
  - When the source image is readable and every piece has `to_px`: crop the source to `meta.source.crop` + 3 % margin,
    wash 45 % toward white, and scale the long side to 1600 px.
  - Otherwise use the pieces' `to_px` (no image), or pack the pieces left to right with gaps of 8 % of the tallest
    piece, y flipped, scaled to 0.9·1600 with 5 % padding.
  - Text scale s = max(0.8, long side/1600).
- **`overlay.png`** is for the VLM to read.
  - Each piece is drawn in its own palette colour. The 10 colours in BGR: (40, 40, 215), (190, 100, 20), (30, 140, 30),
    (150, 40, 150), (0, 120, 220), (130, 110, 0), (90, 50, 200), (20, 100, 130), (190, 40, 110), (70, 70, 70).
  - Grain is a double arrow; dashed and fold lines are drawn.
  - Each edge gets ticks at ¼, ½ and ¾ (lengths 5, 9, 5 × s) and its id label inside, near the midpoint.
  - Vertex dots carry their ids; notch triangles carry `nN`.
  - The piece id is large at the polylabel centre, with ` (edge-cut?)` when it touches the border.
  - Header strip: `{scale_status} draft, 1 unit = {1/px_per_unit:.3g} px | pN piece, cN corner, eN edge (CCW from c1,
    ticks at 1/4) | nN notch guess, arrow = grain guess`.
  - All text is ASCII-folded (° → " deg", × → x, ...).
- **`labels/pN.png`:** the piece's bbox + 4 % margin, with the outside washed (0.35·px + 0.65·255), upscaled to a
  600 px long side if smaller.
- **`mask.png`:** the binary mask.

### 10.10 `names.json` and `annotate`

**Format:**

| Key | Meaning |
|---|---|
| `garment` | text |
| `drop` | [traced ids] |
| `mirror_seams` | default true |
| `pieces.{pN}` | {`id`, `name`, `role`, `cut` {qty 1\|2, on_fold}, `drawn_side` R\|L (default R), `merge_at` [vertex ids], `split` {edge: [fractions] or one fraction}, `edges` {old: new}, `vertices` {old: new}, `symmetric`, `notes`, `material`, `drop`} |
| `seams` | [{`id`?, `a`, `b`, `ratio`?, `band`?, `kind`?, …}] |
| `checks` | [{`id`?, `a`: [refs], `b`: [refs], `band`? \| `ratio`?, `note`?}] |

**Per piece, in this order:**

1. `merge_at` and `split`, using the overlay's (traced) ids.
2. Renames. New ids must be non-empty with none of `. / + :` or space; they must exist and must not collide.
3. Notch edges and `near_edge` are renamed to match.
4. `cut`: qty must be 1 or 2. `on_fold` is renamed and must be an edge of the piece.
5. `drawn_side: L` mirrors the piece about its bbox-centre x:
   - x' = 2cx − x; edges reversed, from/to swapped, curves mirrored;
   - notch s → 1 − s; grain mirrored with angle → (180 − a) mod 180;
   - `internal`/`trace_marks` points mirrored; `to_px` composed with the mirror; `trace.mirrored_from_drawing = true`.
6. Copy `name`, `role`, `symmetric`, `notes`, `material`; set the new id.

Piece ids must be unique after renaming.

**`merge_at(v)`:**

- Needs an edge into v and more than 3 edges.
- The edge into v and the edge out of it become one edge, keeping the first id and its other keys.
- Its geometry is the concatenated dense polylines, re-fitted: a line if the chord deviation is ≤ max(1e-3·size,
  1e-6), else a relative poly simplified at tol/3.
- Notches are remapped by length share; `on_fold` moves to the first id; the vertex and its `trace.corners` entry are
  removed.

**`split(e, fractions)`:**

- Fractions must lie strictly in (0, 1).
- New vertices `{e}s1, {e}s2, …` sit at arclength fractions (r3). Parts are `e`, `{e}b`, `{e}c`, …, each re-fitted.
- Notches go to the part that contains them, with local s (r4).
- Collisions are errors.

**Seams:**

- Id defaults to `s{k}`. Both sides are resolved (§10.11). `band` is renamed to `ratio_band`. Every other key is
  copied.
- With `mirror_seams`, when either side was shorthand-expanded:
  - Twins: a cut-2 `.R` side becomes `.L` with the same path; an on-fold side swaps edges `.R` → `.L`; a piece that is
    both cut-2 and on-fold has no twin.
  - If both sides have twins, add `{…seam, id: "{id}_L", a, b, twin_of: id}`.
  - Otherwise note `seam ID: no automatic left-side twin (one side is a single piece); declare it explicitly`.

**Checks:** id defaults to `k{k}`. Refs containing `/` are resolved; other refs are kept as given.

**Meta:** `meta.annotated = {garment, piece_map, notes, convention: "pieces are the WEARER'S RIGHT, public side up
(drawn_side L pieces were mirrored)"}`, plus `meta.garment`.

**Errors:** the result must pass the pattern model's structure check. Errors are collected and raised as
`annotate failed:` followed by one problem per line. Unknown piece ids raise
`names.json mentions unknown pieces [...]`.

### 10.11 Reference grammar (`measure_ref`)

| Form | Meaning |
|---|---|
| `INST/eA[+eB]` where INST is an instance id that is not also a piece id (`front.R/side`) | seam-side length **as sewn** |
| `PIECE/eA[+eB]` | sum of edge lengths **as drawn** (`.R`/`.L` suffixes ignored; fold edges allowed) |
| `PIECE:cA-cB` | straight distance (split at the `-` where both halves are vertex ids) |
| `PIECE:cA-cB:x` / `:y` | horizontal / vertical span (what AI dimension arrows measure) |
| `PIECE:width` / `PIECE:height` | outline bounding box |
| `seam:ID[:a\|:b]` | that seam side (default a) |

**Side shorthand** (resolve_side, used by annotate and checks):

- A bare cut-2 piece means its `.R` instance.
- A bare edge of an on-fold piece means its drafted `.R` half.
- The fold edge itself is refused: `… is the fold line, which is not sewn`.
- Old (traced) piece and edge ids translate through the rename maps.

### 10.12 `labels.json` and the label consensus

**Format:** `[{"text": "11 in", "value_mm": 279.4, "ref": "p1:c2-c3:x"}, {"text": "Cut 2·6 fabric"}, …]`. Also
accepted: `"unparseable": true`.

1. **Value.** Use `value_mm`, else parse `text` with no default unit (§10.15).
2. **Unparseable.** No value, flagged unparseable, or no ref → `unparseable` with `why` ("no value" / "a value but no
   single-piece ref"). A ref that fails to measure → `unparseable` with the error text. L ≤ 0 is skipped.
3. **Unit.** `in` if the text contains in, inch, `"`, `”` or `″`; else mm.
4. **Implied scale.** Each label gives s_i = value / measured units.
5. **Exhaustive RANSAC.** For each i: inliers = |s/s_i − 1| ≤ `label_tol` (0.03). Score = (count, −Σ|log(s_j/s_i)|);
   the best score wins.
   - *Rival:* another candidate with the same count and a disjoint inlier set.
   - *Weak:* a rival exists, or inliers < max(2, ⌈n/2⌉).
6. **No consensus.** If fewer than 2 inliers and n ≥ 2: `no_consensus`, every label is an outlier, scale null.
7. **Scale.** Otherwise scale = exp(median log s over the inliers). Per label: `inlier`, `fitted_mm` r2, `dev_pct` =
   r2((measured·scale/value − 1)·100).

**Output:** `{scale_mm_per_unit, per_label, unparseable, inliers, outliers, n_labels, n_agree, weak, rival_group?,
no_consensus?}`.

### 10.13 Round-0 audit

All checks are ratios, so they hold at any scale. Tolerance is **3 mm-equivalent**.

**Scale basis**, in order:

| Basis | When | mm per unit |
|---|---|---|
| `FITTED` | not UNSCALED | 1 |
| `LABELS` | a non-weak label consensus exists | consensus scale (`n_labels` = inlier count) |
| `NOMINAL` | otherwise | the median seam-side length (or median instance edge length if there are no seams) is taken as `median_seam_mm` 250. Note text: `median seam side (X units) taken as 250 mm` |

The `-equiv` suffix is added to mm values unless the basis is FITTED.

**Findings:** `{id F#, class T|S|M, kind, severity error|warn|info, refs, message, route, extras}`.

`route` texts:

| Class | Route |
|---|---|
| T | `topology: regenerate the sheet (regeneration hygiene) or pick a program option; never invent topology silently` |
| S | `style: transfer the curve character onto the measured block; regenerate only if the user wants another style` |
| M | `metric: fix with vector edits or the measured block; never send numbers back to the image model` |

The audit never emits S; the agent routes style itself.

| Kind | Class | Rule |
|---|---|---|
| `structure` | T | each pattern structure error |
| `seam_mismatch` | M | Seams without `twin_of` are checked. A gather (`kind` gather/gathered with no band and no ratio) is not checked. With a band: target = clamp(ratio, lo, hi), dev = La − target·Lb. Else ratio r (`ratio`, `ease_ratio` or 1): dev = La − r·Lb. dev_mm = dev·mpu; dev_pct = (ratio/target − 1)·100. Finding when \|dev_mm\| > tol. Messages: plain `Seam ID does not walk: A is X mm{eq} longer/shorter than B (±P %). True the seam lengths in the vector pattern, not by regenerating the image.`; band and declared-ratio variants likewise. |
| `ratio_check` | M | `checks`: sums of `measure_ref` over a and b; band or ratio; same tolerance |
| `fold_not_square` | M | For on-fold pieces (skipped if there are structure errors): the edges before and after the fold must meet it at 90 ± 5°. Tangents are Richardson-extrapolated chord angles (2·a(s/2) − a(s)) over span = max(15/mpu, 10/px_per_unit). Shape word: `a point` (< 90) or `a V-shaped dip`. Extra `angle_deg`. |
| `asymmetric` | M | `symmetric: true` pieces: mirror the outline (80 samples per edge) about the grain direction (else vertical), with the axis offset swept over 41 positions within ±10 % of width. min Hausdorff × mpu > tol. Extra `hausdorff_mm`. |
| `cut_off` | T | `trace.touches_border`; wording "an erased (UI-covered) region" or "the image/crop border" |
| `inventory` | T | a sleeve with no bodice; a sleeve with no seam/check joining it to a bodice piece ("has no armhole partner"); front without back (or back without front) for bodice, pant, skirt; a waistband without any pant_front, skirt_front or pant_back |
| `unsewn` | T, warn | a piece joined to nothing, when seams exist |
| `no_seams` | T, info | no seams declared |
| `labels_inconsistent` | M | weak consensus with a scale: `Only N of M dimension labels agree on any one scale…` |
| `label_contradiction` | M | each non-inlier, with its measured length at the common scale (in the label's unit) and dev_pct; without a scale: "agrees with no other label" |
| `label_unparseable` | T if the text contains "cut", else M; warn | `metadata or garble, not geometry` |

- **Joins** for inventory are a graph over piece stems built from seams and checks.
- **Summary:** counts of T, S and M excluding info; `ok` = no error-severity finding.
- **Report:** `{schema: fabric-round0-audit/1, pattern, scale_status (default FITTED), scale_basis, tol_mm, seams
  [rows], labels, findings, summary, suggested_scale?}`. `suggested_scale` appears only when UNSCALED and the consensus
  is non-weak.
- **`round0_audit.png`.** Piece outlines are grey. Each non-info finding's ref geometry is drawn in red with an `F#`
  tag, and tags stack downward when they land within 34·s of each other. Below the picture is a legend: `Round-0
  audit: {summary} | scale basis KIND`, then one wrapped line per finding, `F# [C/severity] message`, with error lines
  in red.

### 10.14 Scale-fit

- With a_i = L_i/T_i: **k = Σa_i / Σa_i²**. This minimises Σ((k·L_i − T_i)/T_i)², so each target counts equally in
  percent.
- **Scaled by k:** vertices (r4), grain (r4), `internal`/`trace_marks` points (r4) and offsets (r3). `to_px[:, :2]`
  is divided by k.
- **Unchanged:** relative curves and notch fractions.
- **Meta:** `px_per_unit` → before/k; `scale_status` → FITTED.
- **Report** (also stored as `meta.scale_fit`): `{k, mm_per_px = k/px_per_unit_before, targets [{ref, target_mm,
  measured_before r4, fitted_mm r3, residual_pct r3}], rms_pct r3, warning}`. The warning is set when any |residual|
  > 2 %: `targets disagree by more than 2 %: at least one known length (or its ref) is wrong`.
- **Errors:** zero targets; a non-positive length.
- **Guidance:** prefer body or seam targets over the sheet's labels.

### 10.15 `parse_length`

- **Pattern:** number (`.` or `,` decimal) + optional ` N/D` fraction + optional unit: mm, cm, m, in, inch, inches,
  `"`, `”`, `″`, `''`.
- **Default unit:** mm for CLI targets, none for labels.
- **Examples:**

  | Input | Result |
  |---|---|
  | `11 in` | 279.4 |
  | `10.5"` | 266.7 |
  | `10 1/2 in` | 266.7 |
  | `28cm` | 280 |
  | `12` with no default unit | null |
  | `Cut 2.6 fabric` | null |

---

## 11. The `fab` CLI

### 11.1 Commands

The tool runs on Python 3.12. Pass-through: `fab measure …`, `fab vectorize …` and `fab alter …` forward their
arguments verbatim to the companion tools and return their exit code.

| Command | Flags | Behaviour | Exit |
|---|---|---|---|
| `env` | — | Reports Python modules numpy, shapely, cv2, ezdxf, PIL; the Blender path and version; the codex image_generation feature; ffmpeg; tesseract. Prefixes `ok` / `MISS` / `info`. | 0 if modules + Blender ok, else 1 |
| `draft PROGRAM` | `--measurements M` (req), `--ws WS` (req), `--style FILE`, `--update-measurements` | §11.4 | 0 |
| `round WS` | `--pattern P`, `--note T`, `--sim`, `--render` (implies sim), `--export`, `--arm-deg D`, `--measurements M`, `--update-measurements` | §11.5 | 0 if CONVERGED, else 2 |
| `validate P` | `--fit F` (req), `--measurements M` (req) | flat validation | 0 CONVERGED, 2 NOT_CONVERGED, 3 BLOCKED |
| `sim P` | `--measurements M` (req), `--fit F`, `--out DIR` (req), `--render` | Fold-up with stills; prints {flags, settled, landmarks, wall_s}. No tightness demotion. | 0 if **no flags at all**, else 2 |
| `qualify` | `--ws WS` \| (`--measurements M` [`--pattern P`] `--out DIR`) | §7. With `--ws`: measurements default `WS/measurements.json`, pattern = latest round, out = `WS/qualification`. | 0 / 2 |
| `viewer SIMDIR` | `--out HTML` (default `SIMDIR/fold_viewer.html`) | Requires garment.blend, fold.pc2, mesh_map.json and sim_report.json, else `… is not a finished fold-up (no F)`. Prints `PATH  (X.X MB; open it in a browser: drag to turn, play or scrub the fold)`. | 0 / 1 |
| `sim-points SIMDIR` | `--pattern P` (req), `--measurements M` (req) | §9.8 | 0 |
| `preview P` | `--out PNG` (req) | §11.8 | tool's exit |
| `report WS` | — | writes `WS/report.html` | tool's exit |
| `export P` | `--out DIR` (req), `--targets all\|svg,pdf,…` | each target in turn | OR of exit codes |
| `outfit` | `--measurements M` (req), `--sim DIR` (repeat), `--hat P`, `--out DIR` (req), `--turntable` | §9.9 | 0 |
| `imagegen "PROMPT"` | `--out PNG` (req) | §11.9 | 0 |

**`outfit` refusals**, before any render:

| Condition | Message |
|---|---|
| neither `--sim` nor `--hat` | `nothing to show` |
| measurements file missing | (refused) |
| a sim dir without garment.blend, fold.pc2 and sim_report.json | `… is not a finished fold-up` |
| a hat without `meta.solved.base_girth_mm` | `… not a pattern drafted by `fab draft fez`` |

**Simulator subprocess contract** (reference):

- The simulator is run headless, with its exit code propagated on script errors. stdout and stderr go to `OUT/log.txt`.
- The run fails (`Blender sim failed (exit N); see …/log.txt`) unless exit = 0 **and** `sim_report.json` is fresh
  (mtime ≥ start − 1 s). A stale report from an earlier run in the same directory is never read as this run's result.
- The viewer export is a picture of the run and never part of the verdict. Its failure prints
  `NOTE: 3D fold-up viewer not written: …` and the round continues.

### 11.2 Workspace layout

```
WS/
  measurements.json                     the body every round is judged against
  measurements.before-round-NNN[.SSS].json   replaced bodies (never overwritten)
  fit_spec.json                         the fit contract (first draft's, or a role-based default)
  rounds.jsonl                          one JSON object per `round` run
  report.html                           regenerated after every round
  qualification/qualify.json, band_-20/, band_+0/, band_+20/
  rounds/NNN/                           NNN = 3-digit, one past the highest numeric directory
    pattern.json  fit_spec.json (from draft)  validation.json  readiness.json  preview.png
    sim/   log.txt sim_report.json mesh_map.json fold.pc2 garment.blend still_*.png
           fold_viewer.html fold_0001-NNNN.mp4 fold_side_by_side.mp4
    exports/  every export target + acceptance checks
```

**Invariants:**

- Never delete prior rounds or user files.
- Round numbers come from the highest existing number, not a count: with 000 and 002 present, the next is 003.
- A directory without `pattern.json` (a failed draft) is skipped by "latest round" but still counts for numbering.
- Keep workspaces outside temporary directories that the OS prunes.

### 11.3 Measurement adoption (`--measurements`)

| Situation | Action |
|---|---|
| `--measurements` file missing | refuse: `measurements file not found: …` |
| `WS/measurements.json` absent | copy the given file in |
| same resolved path, or identical sha256 | nothing |
| different, without `--update-measurements` | refuse: `WS/measurements.json differs from SRC. A workspace is one body: add --update-measurements to adopt the new file (the old one is kept), or start a new workspace.` |
| different, with `--update-measurements` | Copy the old file to `measurements.before-round-{NNN}.json` (NNN = next round index), or `….{SSS}.json` with the first free 3-digit suffix (exclusive create, mode preserved, never overwritten). Copy the new file in. Print `NOTE: measurements updated; the previous file is kept as NAME`. |

### 11.4 `draft`

1. Refuse an unknown program **before** any directory is created. Programs: `zouave_bolero`, `zouave_trousers`, `fez`.
2. Refuse missing `--style` or `--measurements` files.
3. Adopt the measurements and create the next round directory.
4. Run the drafting program, which writes `pattern.json` and `fit_spec.json`. If no `pattern.json` results:
   `drafting failed (exit N, error above); DIR stays empty and `round` skips it`. A draft with structure errors prints a
   WARNING.
5. Copy the draft's `fit_spec.json` to `WS/fit_spec.json` if the workspace has none.
6. Print the measurement gate, then `round dir: DIR`.

`draft` never appends to `rounds.jsonl`.

### 11.5 `round`: step by step

1. **Refusals, before the workspace changes:**

   | Condition | Message |
   |---|---|
   | `--pattern` file missing | `pattern not found` |
   | no `--pattern` and no round with a pattern | `WS has no round with a pattern yet: `fab draft PROGRAM --ws WS` or `fab round WS --pattern P`` |
   | no `--measurements` and no `WS/measurements.json` | refused |
   | the given `--measurements` file is missing | refused |

2. Adopt the measurements (§11.3).
3. **Round directory.** With `--pattern`: a new round directory with a copy of the pattern. Without: the **latest
   round with a pattern, re-run in place**. Its files are overwritten and a new `rounds.jsonl` entry is appended with
   the same round number.
4. **Notes.**
   - UNSCALED (`meta.scale_status == "UNSCALED"`): note `pattern is UNSCALED (1 unit = 1 px): run `vectorize scale-fit`
     before judging fit or simulating`.
   - Otherwise infer the missing placements (§3.5) and write `pattern.json` back.
5. **Fit spec.** If `WS/fit_spec.json` is missing, write the role-based default and note it. Then compare the round's
   own `fit_spec.json` (if any) with the workspace's by `{point id: target_mm}`. On difference, note: `this draft's fit
   spec differs from fit_spec.json, the contract this round is judged by (new points [...], missing [...], changed bands
   [...]); copy … over it if the contract should change`. The workspace contract is never adopted silently.
6. Print every note as `NOTE: …`.
7. **Measurement gate** → `readiness.json`, then print it (§11.6).
8. **Flat validation** → `validation.json`, then print the table.
9. **Preview.** `preview.png` with failing seams in red. `images` = [preview].
10. **Sim**, if `--sim` or `--render`. `sim_basis` starts as `not run`. Skip the run with one of these bases:
    - `skipped (validation BLOCKED)`;
    - `skipped (pattern UNSCALED)`;
    - `not applicable (no piece has a fold-up placement; judged flat)` (a cone hat is developable, so this is a
      complete basis).

    Otherwise:
    - Run the simulator (stills on; `--arm-deg` passed only when given and non-zero) into `rounds/NNN/sim`.
    - Append the existing `still_front`, `still_back`, `still_side`, `still_strain_front` to `images`.
    - Write the viewer.
    - For each rendered video: build the side by side. On failure, fall back to the plain video.
    - `gate_tightness` against `qualification(ws, WS/measurements.json, pattern.fabric)`; record its note.
    - Print `SIM gating flags:` and `SIM advisory flags:` (`none` when empty).
11. **Exports**, if `--export`. If BLOCKED or UNSCALED, the basis is:
    - `skipped (validation BLOCKED)`, or
    - `skipped (pattern UNSCALED: true-size files need `vectorize scale-fit` first)`.

    Otherwise run every export target into `rounds/NNN/exports` and its acceptance checks: `exports = {dir, pass (exit
    0), checks (the last `PASS:`/`FAIL` line, else the last stderr line, else "export failed"), failures [lines starting
    `FAIL `]}`. Print `EXPORTS:` with the checks, the directory and the failures.
12. **Status** (§11.6). Append the entry to `rounds.jsonl` (§11.7), regenerate `report.html`, and print `report: PATH`.
13. **Final line:** `ROUND STATUS: {status} (flat: X; sim: Y; exports: Z; measurements: W)`.

### 11.6 Statuses

**`round_status(flat, sim, exports, ready, unscaled)`.** One function is shared by the CLI and the browser. First
match wins:

| Order | Condition | Status |
|---|---|---|
| 1 | flat == BLOCKED | `BLOCKED` |
| 2 | unscaled | `UNSCALED` (no length in a pixel-unit trace is a length yet) |
| 3 | flat ≠ CONVERGED | flat status (`NOT_CONVERGED`) |
| 4 | sim present and `sim.gating` non-empty (after tightness demotion) | `SIM_GATING_FLAGS` |
| 5 | exports present and not pass | `EXPORT_CHECKS_FAILED` |
| 6 | ready == NOT_READY | `MEASUREMENTS_NOT_READY` (the draft still exists; it fits the chart, not the wearer) |
| 7 | — | `CONVERGED` |

**`basis`:**

| Key | Value |
|---|---|
| `flat` | validation status |
| `sim` | `not run` \| a skip text \| `gating flags` \| `no gating flags` |
| `exports` | `not run` \| a skip text \| the checks line (e.g. `PASS: 183/183 checks`) |
| `measurements` | readiness status |

"sim: not run" or "measurements: NOT_READY" is not a finished pattern. "sim: not applicable" can be.

**Measurement gate (`readiness.json`, schema `fabric-round-readiness/1`).** Piece roles select the garment checks:

| Roles present | Check |
|---|---|
| bodice_front or bodice_back | `bodice` |
| sleeve | `set_in_sleeve` {closed_cuff: true} |
| pant_front or pant_back, with cuff | `full_trousers_gathered` {elastic_waist: waistband present, cuff_level: style.cuff_level or "calf", nappies: true if style.nappy else null} |
| pant_front or pant_back, without cuff | `trousers` {elastic_waist} |
| hat_crown or hat_tip | `hat_fez` |

Output: `{schema, status, checks [{garment, status, blocking}], blocking, remeasure_request, advisories, note?,
for_real_child?}`.

| Status | When |
|---|---|
| `NOT_CHECKED` | no roles (note), or no measurement `records` (provenance unknown), or the readiness computation failed |
| `NOT_READY` | any check is NOT_READY |
| `READY_REFERENCE` | every check is READY_REFERENCE (a size-chart body; its advisories are suppressed) |
| `READY` | otherwise |

Printed forms:

- `MEASUREMENTS: NOT_READY for KINDS. Re-measure before trusting the fit:` followed by one request per line;
- `MEASUREMENTS: READY_REFERENCE for KINDS (a reference wearer built from public size charts; a real wearer needs N
  measurements taken)`;
- `MEASUREMENTS: READY for KINDS`;
- `MEASUREMENTS: not checked (NOTE)`;
- then `  note: …` advisories.

### 11.7 `rounds.jsonl` entry

| Field | Content |
|---|---|
| `round` | int (directory number) |
| `dir` | `rounds/NNN` (relative to WS) |
| `note` | `--note` or "" |
| `auto_notes` | the notes list (placements, fit spec, demotion) |
| `status`, `basis` | §11.6 |
| `readiness` | {status, blocking, note} |
| `exports` | export record or null |
| `pattern_meta` | pattern `meta` |
| `validation` | full validation report |
| `images` | WS-relative PNG paths |
| `video`, `viewer` | WS-relative paths or null |
| `sim` | null, or the subset {flags, gating, tightness_qualified, settled, landmarks, seams, hem_heights_mm, hem_rise_mm, hem_balance_mm, bands, pieces, verdict, frames, collider_inset_mm, initial_strain_pct, wall_s} (missing keys null) |
| `at` | local time `YYYY-MM-DDTHH:MM:SS` |

### 11.8 `report.html` and `preview.png`

**`report.html`** is self-contained except for the linked video and viewer. It uses light and dark tokens; status
words are coloured by class (CONVERGED, PASS and IN_BAND green; NOT_CONVERGED, BLOCKED, FAIL, LOW, HIGH,
SIM_GATING_FLAGS, MEASUREMENTS_NOT_READY, EXPORT_CHECKS_FAILED and UNSCALED red). For each entry it shows:

1. `Round N — STATUS`, with the basis line;
2. measurements still to take (blocking), and export failures;
3. the note;
4. points table (label, body, finished, ease, target `min/nom/max`, status);
5. seams table (seam, a mm, b mm, ratio, target, resid mm, status);
6. simulation gating and advisory lists;
7. landmarks table (ring strain mean %, max %, contact, reading);
8. images embedded as base64 PNG at width 480;
9. the `Open the 3D fold-up` link;
10. a `<video>` element;
11. the auto notes.

Old entries without `status` fall back to the validation status.

**`preview.png`** (also the 2D half of the side by side):

- **Scale:** 1.6 px/mm; 25 mm gaps; outlines with 80 samples per edge.
- **Layout:** shelf packing aimed at about 16:10. `rows = max(1, round(√(Σ(w + gap) / (1.6·max h))))`;
  `row width = max(max w + gap, Σ/rows)`; start a new row when x + w + gap > 1.05·row width. The first row is on top.
- **Colours:** seams in family colours at width 4; failing seams (validation FAIL or ERROR) in red (220, 30, 30) at
  width 6. Instance ids are drawn at the outline mean. Light background white; `--dark` uses (24, 26, 34).
- The family palette and index rule match §9.4, but here the families are taken over **all** pattern seams (§16).

### 11.9 `imagegen`

1. Requires `codex` on the PATH.
2. Snapshot the `*.png` files under `$CODEX_HOME/generated_images` (default: `.codex` in the user's home).
3. Run `codex exec --skip-git-repo-check "$imagegen PROMPT\n\nGenerate exactly one image. Do not edit any files."`.
4. Take the newest new PNG with mtime ≥ start − 1 s; with none, error `no new image under …`.
5. Copy it to `--out` and write the sidecar `OUT.json` = {prompt, source, tool: "codex $imagegen", at}.

It spends the user's image quota. This is an adapter, not core logic (§14).

---

## 12. Browser (`webapp.py` and the workbench page)

### 12.1 `webapp.run(request, work)`: request

| Key | Values |
|---|---|
| `body` | `{mode: "chart", stature_cm, age_months?, sex?}` (size-chart reference wearer) · `{mode: "tape", stature_cm, age_months?, sex?, nappies?, tape: {name: value}}` (cm, degrees for `*_deg`; blanks skipped; the rest from the chart) · `{mode: "file", measurements: {...}}` (validated; fused if not already) |
| `program` + `style` | `zouave_bolero`, `zouave_trousers`, `fez`, with style overrides. A misspelt style key is an **error naming the key**, never a silent default. |
| `pattern` + `fit_spec`? | check an uploaded `fabric-pattern/1` (default fit spec by role) |

**Body rules:**

Native/WASM freshness exception: an uploaded `mode: "file"` document is validated and re-fused from
its source records even when it contains a nonempty cached `fused` object. The adapter at `00c3edf`
followed the reference's cache fast path: editing a valid record or retaining a once-READY cache over MODEL_FIT
records could leave drafting and saved fold inputs on obsolete values/provenance. The readiness gate
already re-fused records and rejected model estimates; this boundary now keeps its saved inputs and
draft consistent with that gate. The existing fusion, readiness, drafting and export engines remain
authorities; input records are retained and the caller's document is not mutated. This intentionally
strengthens the reference file boundary. Chart/tape modes,
the original oracle bytes and their valid consistent-file cases remain unchanged. Regression evidence
is in `ftx-wasm/tests/page.rs` and the actual compiled-browser check in `tools/check_web.cjs`.

- Stature must be 500–2200 mm, else `stature X cm: enter centimetres (50–220)`.
- Tape values: cm × 10 → mm. Degrees are taken as is.
- Measurement names are canonicalised. An unknown name raises `unknown measurement 'NAME'`; a value ≤ 0 raises
  `NAME: V is not a measurement`.
- Each tape record uses σ = the catalogue tape σ (default 10).
- With `nappies` and a program whose defaults have `nappy`, `style.nappy = true`.

### 12.2 Response

**Rust fold diagnostic extension.** A browser fold retains the existing Anny tape fitter's raw
`body.fit.tape` target/body values and signed `residual_mm = body_mm - target_mm`, iteration count,
all predicted `body.readings_mm`, model mass and any supplied-weight residual in kg. Predictions are
`MODEL_FIT`, including those whose targets came from chart priors; these are not new measurements
or uncertainty estimates. The wasm boundary records the exact loaded model and saved measurement
document SHA-256 identities. The page shows supplied-target residuals separately from cloth flags,
reports unavailable diagnostics explicitly and withdraws them with the old fold on edits. The
fitting/cloth kernels, readiness, status precedence, reference round cases and viewer geometry are
unchanged. Numerical settling does not establish target agreement, anatomy accuracy or garment fit.

- **Structure errors:** `{status: "BLOCKED", errors}`.
- **Otherwise:**

  | Field | Content |
  |---|---|
  | `status` | `round_status(flat, None, exports, readiness, unscaled)` |
  | `basis.flat` | flat status |
  | `basis.sim` | `not run (the Blender fold-up needs the command line: `fab round WS --sim`)`, or the not-applicable text for patterns with no fold-up |
  | `basis.exports` | checks line, or a skip text |
  | `basis.measurements` | readiness status |
  | `validation`, `readiness`, `exports` | {pass, checks, failures, skipped}, or null when skipped |
  | `table` | text validation table |
  | `solved` | `meta.solved` |
  | `title` | `meta.title`, else the stem |
  | `zip`, `files`, `preview_svg`, `work` | see below |

- **Files** written under `work`: `pattern.json`, `fit_spec.json`, `measurements.json`, `validation.json`,
  `readiness.json`, `exports/`.
  - `zip` = `{stem}_pattern_files.zip` of every file under `work` (posix relative names).
  - `stem` = the exporter's safe name of `style` (`[^A-Za-z0-9_-]+` → `_`, trimmed, ≤ 20 characters, default `X`).
  - `files` are the existing key files `{label, name, path, bytes}`, in this order:
    `_LETTER.pdf`, `_A4.pdf`, `_PROJECTOR.pdf`, `_COPYSHOP_A0.pdf`, `_master_mm.svg`, `_AAMA_METRIC.dxf`, `_plot.plt`,
    `_soutache_README.txt`.
  - `preview_svg` = the master SVG text.
- **Exports** are skipped when BLOCKED or UNSCALED: then no `exports/` directory is created and `files` is empty. A
  target that cannot import its writer (offline DXF library) is listed in `skipped` and fails the pass.
- **CLI:** `webapp.py REQUEST_JSON|@file [--work DIR]` prints {status, basis, zip, files}. Exit 0 if CONVERGED, else 2.
- **Helpers:**
  - `program_info()` → {program: {label, style defaults}}.
  - `form_fields(program, stature_cm = 92, age_months = None, nappies = False)` → rows `{name, role, unit cm|deg, how,
    chart}`, must-haves first. An unknown program (`audit`) gives the union over all programs, preferring `must`.

### 12.3 Workbench page (`browser/index.html`)

- **Engine:** Pyodide 0.29.5 in a Web Worker, with packages numpy, shapely and micropip, and the ezdxf 1.4.4 wheel by
  direct URL. Offline, DXF output is skipped.
- **Scripts:** fetched from `../scripts/`: pattern_model, body, drafting, validate, export, measurements, fabric,
  webapp.
- **Messages:**
  - page → worker: `init {base, pyodide, ezdxf}`, `fields {args}`, `run {req}`;
  - worker → page: `progress {text}`, `ready {info, dxf, version}`, `fields {rows, program}`, `result {res, blobs}`
    (the zip and key files as transferables), `error {message: last line, detail: full traceback}`.
- **Persistence:** localStorage keys prefixed `fab-wb-`: program, bodyMode, stature, age, sex, nappies, tape,
  `style-{program}`. A saved style keeps only the keys the current defaults still have.
- **Body section:** the nappies box shows only for tape mode on trousers or audit.
- **Test hook:** the counter `body.dataset.runs` increments after each result or error. Nothing typed leaves the page.

---

## 13. Conformance oracle

### 13.1 Unit and integration tests (no simulator needed)

| Suite | Test | What it pins |
|---|---|---|
| core | seam pairs proportional and notch-piecewise | equal counts; a notch at s 0.25 ↔ 0.5 maps exactly |
| core | wrap is isometric and the roll ends at the wrap | bolero front: \|d3/d2 − 1\| < 1 %; roll(k = 1) == wrap |
| core | dress form faces outward; torso girths; crotch path = measured ±0.5 mm; thigh gap | form invariants used by contact and metrics |
| core | default form armpit bias 30–60 mm above the chest level | the documented ride-up cause |
| core | infer placement by role | front x0 = xmax; top maps to snp + 10; sleeve on arm; waistband none |
| core | default fit spec measures a traced bolero | chest, across_back, neckline, cuff_pass, bicep; none UNMEASURABLE |
| core | qualification band has the exact girth | ±0.01 mm; seams walk |
| core | sim tightness gates only when qualified | demotion note; record must match body sha and fabric key; failed and changed records refuse |
| core | workspace measurement versions; archive slots never overwritten | §11.3 |
| core | workspace keeps one body and notices a new fit contract | drift text names `elastic_hold` |
| core | round CLI on a flat-only garment (fez) | unknown program refused with no directory left; `round` refused before the WS exists; re-runs 000; CONVERGED; basis sim `not applicable…`; measurements READY_REFERENCE; exports PASS; final line text |
| core | outfit refuses what it cannot show | three refusals |
| core | round numbers and simulator results cannot be stale | 000 + 002 → 003; exit-code flag before the script; freshness ±1 s |
| core | traced sheet flows through annotate, scale and export | exports pass on a traced, scaled pattern |
| vectorize | trace structure and files | 3 pieces p1..p3, c1/e1 first, CCW, ring mode, stroke 3–6 px, files exist |
| vectorize | corners match ground truth | count equal; each within 3 mm |
| vectorize | scale-fit, then perimeter and area | k within 1 % of 0.5 mm/px; residual 0; perimeter < 1 %, area < 1.5 % |
| vectorize | marks best effort | grain 90 ± 3°; armhole notch at s 0.5 ± 0.06; fold candidate near CF |
| vectorize | annotate and audit seams | twins `_L`; one side mismatch, dev_pct −9.09 ± 1; underarm ok; no fold or inventory findings; audit PNG > 10 kB |
| vectorize | label consensus catches a contradiction | scale within 1.5 %; outlier exactly `10 in hem`; one contradiction (M); one unparseable (T) |
| vectorize | inventory and fold findings | "no armhole partner", "no bodice back", fold_not_square |
| vectorize | annotate rejects bad refs; mirror drawn-left keeps CCW and lengths; split and merge preserve geometry | §10.10 |
| vectorize | coloured backgrounds | navy auto polarity; filled + dark outline with `stroke 4`: perimeter < 1 %, area < 1.5 %, all `fill` |
| vectorize | CLI round trip | scale-fit rms < 1 %; audit FITTED; side dev_mm −23 ± 3 |
| vectorize | parse_length | §10.15 |
| vectorize | audit a parametric pattern without a trace | FITTED; side dev exact to 0.05; `front/cf` = 300; `back:a-c:y` = RECT_H; packed renders |
| vectorize | dashed fold edge still closes the piece | 1 piece without bridging, 2 with; area ≈ 350 × 540 ± 3 % |
| webapp | fez round in the browser glue | CONVERGED; `PASS: 103/103 checks`; zip contents; first files `FEZ_LETTER.pdf`, `FEZ_A4.pdf` |
| webapp | tape body names what to measure; catches inches | must includes waist, hip, total_crotch_length, foot_pass; MEASUREMENTS_NOT_READY with arm_length and hand_pass; chest 21 "cm" → "outside the chart"; stature 0.92 → "centimetres"; unknown name refused |
| webapp | checking an uploaded pattern; a misspelt style key; nappies; file names; unscaled trace | §12 |
| webapp | round_status is one function | §11.6 precedence |

**Synthetic sheet fixture.** Generated in code, never checked in as an image:

- **Sheet:** 1050 × 480 mm at 2 px/mm, rendered 4× supersampled with LANCZOS down-sampling. Degraded by 0.7° rotation
  about the centre, Gaussian blur 1.2 and Gaussian noise σ 12 (seed 7).
- **Pieces:** a bodice front, a back rectangle 300 × 1.10·√(10² + 230²) mm, and a sleeve with a cubic cap.
- **Marks:** 4 px strokes, filled notch triangles, arrowed grainlines, a dashed line 10 mm inside CF, and text labels.
- **Variants:** black on white, white on navy (25, 35, 80), red fill (175, 30, 35) with a dark outline.

**Accuracy expectations:**

| Variant | Perimeter | Area | Time |
|---|---|---|---|
| black on white | −0.07 to −0.24 % | −0.18 to −0.50 % | 0.2–0.7 s |
| navy | −0.05 to −0.30 % | −0.16 to −0.28 % | 0.2 s |
| filled | −0.08 to +0.17 % | −0.08 to +0.29 % | 0.15 s |

### 13.2 Browser smoke test (headless Chromium)

- **Drafts on the chart body:**

  | Program | Expected |
  |---|---|
  | bolero | CONVERGED, `PASS: 183/183`, 5 soutache DST files |
  | trousers | CONVERGED, `PASS: 150/150`, 0 DST |
  | fez | CONVERGED, `PASS: 103/103`, sim basis "not applicable", 0 DST |

  Each zip holds pattern, fit_spec, measurements, validation, a `_LETTER.pdf` and an `_AAMA_METRIC.dxf`.
- **Errors and gates:**
  - Style typo `chest_eas` → a one-line error naming it.
  - Tape chest 54 and neck_base 25 → MEASUREMENTS_NOT_READY, with arm_length named.
  - Nappies offer `seat_over_nappy`; audit mode offers the union.
  - The downloaded bolero re-uploaded → CONVERGED.
- **Phone layout:** 390 px dark mode has no horizontal scroll.
- **Each viewer page:** opens from file://. Frame 0 phase starts `Flat pattern`; the last starts `Draped`. No
  horizontal scroll at 1100 px or 390 px. No page or console errors.

### 13.3 Simulator goldens (recorded, not checked in)

The reference suite has **no automated simulator test**. The simulator oracle is the recorded readings in §7.1 and §8.
For a port, capture goldens from the reference before retiring it:

- `sim_report.json`, `mesh_map.json` and the first and last PC2 frames for the drafted bolero, trousers and the three
  qualification bands, on the 92 cm / 30-month reference wearer (public size-chart priors only);
- the mesh build (vertex count, `inst_range`, seam sample counts, corner indices), which is deterministic and can be
  matched **exactly** without any dynamics;
- the kinematic stages (wrap, pre-sew, relax: `initial_strain_pct`, `relax_iters`), which are deterministic numpy and
  should match to float tolerance;
- the dynamics, compared by flag sets and the bands of §8, never bit for bit across solvers.

---

## 14. Exclusions (not to port)

| Reference mechanism | Why it existed | Native replacement |
|---|---|---|
| Rest shape key copy + pass-through geometry-nodes modifier | Blender bug #115321 ignores a rest key otherwise | rest = 2D mesh by construction |
| Shape keys ("Basis" = board, "rest", "start") | Blender's way to give start and rest | explicit arrays |
| Collider inset by thickness_outer + distance_min − underlayer | contact shell thickness is a solver parameter | contact model whose equilibrium gap = underlayer (§5.2) |
| `sewing_force_max = 0`, sewing-spring loose edges | Blender's sewing mechanism | sewing constraint between pairs |
| Vertex-group structural-stiffness trick for gathers | no per-direction compression control | tension/compression split as a material or per-vertex parameter (§5.4) |
| `quality`, `collision_quality`, impulse-based collision, air damping 5 | Blender solver controls | solver's own tolerances (REQ-JITTER) |
| Gravity effector keyframes | phase switch | explicit phase schedule |
| Point-cache bake in a context override; `frame_set` read-back | Blender plumbing | direct stepping |
| BVH nearest-point queries, `delaunay_2d_cdt` | Blender's library | any equivalent with §2.5 / §4.1 semantics |
| Exact boolean union collider (`--union`) | contradictory normals at the armpit | a single smooth contact potential |
| PC2 cache modifier (`frame_start = 1`), WELD, Subsurf, Solidify, EEVEE, FFMPEG `media_type`, `-noaudio`, `--factory-startup`, `--python-exit-code` | rendering and headless plumbing | a renderer of choice; keep camera table §9.5 |
| `garment.blend` and appending `GarmentAnim` into the outfit scene | scene persistence | portable scene record: mesh, colours, form, PC2 |
| `fab env` Blender discovery (`FABRIC_BLENDER`, macOS application bundle, `blender` on PATH) | locating Blender | none |
| codex `$imagegen` adapter | uses a logged-in third-party CLI | optional adapter, outside the core |
| Pyodide hosting of the Python modules | browser reach without a port | native wasm build; keep §12 request/response |

**Keep:** every metric and flag (§6), the kinematic start (§3–4, until a solver needs only an intersection-free
start), PC2 and `mesh_map.json` (so `sim-points` and the viewer work across solvers), and the viewer format.

---

## 15. Dependencies on companion specs

| Topic | Needed here for |
|---|---|
| Pattern model (`fabric-pattern/1`, instances, mirroring and on-fold unfolding, seam refs, `seam_pairs`, curve sampling, structure errors) | §2, §10 |
| Body and form (measurement names, landmarks, part frames, girth-matched loft, crotch ring solve, `section_radius`, anatomical variant) | §3, §4, §5.2, §6.9 |
| Validation and fit contract (statuses CONVERGED / NOT_CONVERGED / BLOCKED, points of measure, default fit spec, text table) | §6.3, §11 |
| Measurements (reference wearer, fusion, readiness per garment kind, catalogue) | §11.6, §12 |
| Drafting programs (bolero, trousers, fez, style defaults, `meta.solved`) | §11.4, §12 |
| Exports (targets, acceptance checks, `safe_name`) | §11.5, §12 |

---

## 16. Reference quirks (port or fix deliberately, and record the choice)

1. **Ring level ignores `sim_scale`.** The landmark ring level (§6.3) uses the placement's unscaled `y0` while rest2d
   is scaled. It is harmless today because only unscaled pieces carry girth points.
2. **Preview and 3D seam colours can disagree.** The 2D preview indexes seam families over **all** pattern seams; the
   3D `seam_rgb` and the legend index only the **simulated** seams. They agree only when every seam is simulated.
3. **`meta.trace.threshold` is overwritten.** The trace option string (`otsu` or `adaptive`) is replaced by the
   binarize diagnostics dict, which still carries `method`.
4. **A single label never sets a scale.** One label gives a consensus marked weak, so the scale is not used as the
   basis and `labels_inconsistent` reads "Only 1 of 1 …".
5. **Balance needs id prefixes.** `hem_balance_mm` requires instance ids starting with `front`/`back`; trouser
   instances (`tr_front…`) never get a balance. LOST considers every hem, sleeves included. Pieces without a `hem`
   edge (bands) can never be LOST.
6. **Arm angle 0 is ignored.** `round --arm-deg 0` is dropped, because the flag is only passed when truthy.
7. **`fab sim` exit code.** It exits 2 on **any** flag, advisory ones included, and applies no tightness demotion.
8. **Pattern-less qualification.** `fab qualify` without a pattern records `fabric_key: null`, which then qualifies
   only patterns with no `fabric` block.
9. **Re-run rounds share a number.** `round` without `--pattern` overwrites the latest round's files in place; its
   `rounds.jsonl` history has two entries with the same round number.
10. **Absolute path in the report.** `sim_report.pattern` is an absolute path. Consumers fall back to
    `SIMDIR/../pattern.json`. A port should store a workspace-relative path and keep that fallback, which also keeps
    machine paths out of shared artifacts.
11. **Doc vs code on the +20 band.** The docs say the +20 band must read "EASY"; the code accepts EASY or SNUG with mean
    stretch < 1 %. The code is the oracle.
12. **Viewer bounds are rounded.** The viewer's `lo`/`hi` are rounded to 1 µm after quantisation, a negligible
    dequantisation offset.
