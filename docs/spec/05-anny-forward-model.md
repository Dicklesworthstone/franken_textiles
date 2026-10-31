# 05 · Anny forward model (`ftx-body`)

Essence specification extracted on 2026-10-12 from the anny 0.6.1 package (pin and wheel hash in
`tools/body-model/PIN.md`) by reading and running it. Sources: `models/__init__.py` (`create_fullbody_model`,
`build_model_data`), `models/legacy.py`, `models/model_data.py` (`RigConfig`, `TopologyConfig`, `ModelData`,
`resolve_blendshape_mask`, `cache_builder`, `PHENOTYPE_VARIATIONS`), `models/full_model.py` (`load_data`,
`load_mesh`, `load_macrodetails`, `load_all_blendshapes`, `load_rig`, `get_edited_mesh_faces`,
`build_anny_model_data`), `models/model_transforms.py` (`filter_blendshapes`, `edit_mesh`,
`remove_unattached_vertices`, `compact_skinning_weights`, `triangulate`), `models/phenotype.py` (`Anny`),
`models/rigged_model.py` (`RiggedModelWithLinearBlendShapes`), `utils/interpolation.py`, `utils/kinematics.py`,
`utils/mesh_utils.py`, `skinning/skinning.py`, `skinning/warp_skinning.py`, `anthropometry.py`,
`shape_distribution.py` and the bundled MPFB2 assets (`data/mpfb2/...`).

You should not need to read the upstream code to implement anything below. The upstream package is the oracle:
where this document and the package disagree, the package wins until a deliberate change is recorded. Every rule
here was checked by an independent NumPy re-implementation written from this text, which matched the package to
2e-15 m on rest and posed vertices (phenotypes in and out of range, local changes beyond ±1, root and limb poses).
Section 9 lists the quirks and the parts that make a Rust port hard.

## Contents

0 Conventions and the configuration specified · 1 Data the forward model consumes · 2 Phenotype interpolation ·
3 Local changes · 4 Rest shape and rest skeleton · 5 Pose: local-bone kinematics and skinning · 6 Anthropometry ·
7 Shape distribution (age, height priors) · 8 Reference values for the parity suite · 9 Quirks and porting hazards ·
Appendix A Local-change labels · Appendix B Bone hierarchy · Appendix C Waist loop

---

## 0. Conventions and the configuration specified

| Item | Rule |
|---|---|
| Units | metres, kilograms, radians inside the model (the reference poses in §8 are stated in degrees). |
| Axes | +Z up; the body faces −Y; +X is the body's **left** (`*.L` bones and the left hand are at +X). This is Blender's convention and the same frame as `docs/spec/04` §0. |
| Precision | The package builds and runs the model in **float64**. The only float32 step is the bone-roll matrices (§1.6, §9.3). |
| Matrices | 4×4 homogeneous, column-vector convention: `p' = A·[p; 1]`. `[R | t]` is the rigid transform with rotation R and translation t. |
| Rigid inverse | `inv([R | t]) = [Rᵀ | −Rᵀ·t]`. The package always inverts rest poses this way (`roma.Rigid.inverse`), never with a general 4×4 inverse. |
| Rodrigues | `rot(r)` for a rotation vector r (axis × angle): `I + sin θ·K + (1 − cos θ)·K²`, θ = ‖r‖, K = cross-product matrix of r/θ; `rot(0) = I`. |
| Sizes | V = 13,718 vertices, F = 27,420 triangles, J = 163 bones, K = 9 influences per vertex, S = 1,136 blend shapes, P = 624 phenotype blend shapes, L = 256 local changes. |
| Batch | Every input carries a leading batch axis B. Phenotype, local-change and pose batches must be equal or 1 (size 1 broadcasts). |
| Indices | 0-based. "Base index" means an index into the MakeHuman base mesh (`data/mpfb2/3dobjs/base.obj`, 19,158 vertices); "final index" means an index into the 13,718-vertex model mesh. |

**The configuration specified.** The call `create_fullbody_model(rig="default", topology="default",
local_changes="all", remove_unattached_vertices=True, triangulate_faces=True)` with the defaults
`pose_parameterization="local-bone"`, `bone_orientation="blender-rootidentity"`, `extrapolate_phenotypes=False`,
`all_phenotypes=False`. In 0.6.1 this function is deprecated and resolves (`models/legacy.py`) to:

| Setting | Resolved value |
|---|---|
| Rig | `RigConfig(base_rig="makehuman", bone_orientation="blender", root_identity_orientation=True, bones_to_remove=∅, subtree_root=None)`: MPFB2 `rig.default.json` with `weights.default.json`, all 163 bones kept |
| Topology | `"anny"` = `TopologyConfig(base_mesh="makehuman", submodel="body", nudity_edits=True, eyes=True, tongue=True, remove_unattached_vertices=True, triangulate_faces=True)` |
| Blend shapes | all phenotype blocks, all 256 local changes, no facial actions |
| Phenotypes exposed | `gender, age, muscle, weight, height, proportions` (in that order). `cupsize`, `firmness` and the three race values are fixed at 0.5 |
| Pose parameterization | `local-bone` (§5) |
| Skinning | linear blend skinning. With `skinning_method=None` the package picks NVIDIA Warp's kernel if `warp` imports, else the torch kernel; they agree to 4e-16 m (§9.8) |

`data/cached/anny.pth` is **not** used by this configuration. It feeds only the "cached" bone orientation of the
newer `rig="anny"` preset, which is what a bare `Anny()` builds (it also prunes bones and defaults to the
`local-ref` pose parameterization). That model is out of scope here.

---

## 1. Data the forward model consumes

### 1.1 How the package builds it

`build_anny_model_data` runs once and caches the result (§9.9). The steps, in order:

1. **`load_data`** (float64, its own cache entry). Loads the base mesh, every blend shape and the rig.
   - **World transform** `W = 0.1·Rx(+90°)`: a MakeHuman point (x, y, z) in decimetres, Y up, becomes
     (0.1x, −0.1z, 0.1y) in metres, Z up. Applied to the base-mesh vertices and to every target offset.
   - **Base mesh** (`load_mesh`): all 19,158 `v` lines of `base.obj`. Faces are the quads of the OBJ groups
     `body` (13,378), `helper-l-eye` (70), `helper-r-eye` (70) and `helper-tongue` (224), concatenated in that
     order. The other groups (tights, skirt, hair, teeth, genital, eyelashes, 125 `joint-*` cubes) keep their
     vertices for the rig regressors but contribute no faces.
   - **Blend shapes** (§1.3) and **rig** (§1.5–1.7).
2. **`_filter_rig`**: a no-op here (no bones removed, no subtree).
3. **`filter_blendshapes`** with the mask from `resolve_blendshape_mask`: keeps every phenotype row and every
   local-change row, drops the facial-action rows. Applied to the vertex, bone-head and bone-tail blend shapes alike.
4. **`edit_mesh`** (`get_edited_mesh_faces`, the "nudity edits"): drops every face that touches base vertices
   1778–1793 or 8450–8465 (32 vertices around the nipples, 46 faces) and appends 14 cap quads (7 per side, listed
   literally in that function) that close the holes.
5. **`remove_unattached_vertices`**: keeps exactly the vertices that some face uses, in ascending base index.
   `base_mesh_vertex_indices[final] = base` records the map. Faces are re-indexed. Template vertices, blend
   shapes and skinning rows are subset the same way. Bone heads and tails are **not** touched: they were already
   reduced to per-bone values in step 1, which is why the joint cubes can disappear.

   | Part | Final indices | Base indices |
   |---|---|---|
   | body | 0–13,347 | 0–13,379 without the 32 removed vertices |
   | tongue | 13,348–13,573 | 13,380–13,605 |
   | left eye | 13,574–13,645 | 14,598–14,669 |
   | right eye | 13,646–13,717 | 14,670–14,741 |

6. **`compact_skinning_weights`**: repeatedly removes one padding slot per vertex (the arg-min slot) while every
   vertex still has a zero slot. Leaves K = 9.
7. **`triangulate`**: each quad (a, b, c, d) is split along its shorter diagonal measured on the template:
   if ‖a − c‖ < ‖b − d‖ then (a, b, c), (c, d, a), else (a, b, d), (d, b, c). The 13,710 quads become
   27,420 triangles, in quad order. The converter dumps the result, so the port never triangulates.

The final mesh is four closed, consistently wound, outward-facing components (body, tongue, two eyes): 41,130
edges, no boundary edge, no non-manifold edge, positive signed volume.

### 1.2 Tensors to convert

| Tensor | Shape | dtype (package) | Meaning | Needed |
|---|---|---|---|---|
| `template_vertices` | V×3 | f64 | template positions T (MakeHuman base mesh, metres). Every value is a multiple of 1e-5 m (to rounding) | yes |
| `faces` | F×3 | i64 | triangles, final indices, outward winding | yes |
| `blendshapes` | S×V×3 | f64 | vertex blend shapes B_k, row order in §1.3 | yes |
| blend-shape labels | S strings | – | `block:components` (`universal:male-baby-minmuscle-minweight`, `local_change:measure-waist-circ-incr`, ...) | yes (or the mask below plus the local-change labels) |
| `stacked_phenotype_blend_shapes_mask` | P×26 | f64 (0/1) | which of the 26 phenotype components (§2.1) each phenotype row uses | yes (derivable from the labels) |
| `local_change_labels` | L strings | – | the 256 local-change names = the positive target of each pair (Appendix A) | yes |
| `template_bone_heads` | J×3 | f64 | rest bone heads of the template | yes |
| `bone_heads_blendshapes` | S×J×3 | f64 | bone-head blend shapes (same row order as `blendshapes`) | yes |
| `template_bone_tails` | J×3 | f64 | rest bone tails of the template | yes |
| `bone_tails_blendshapes` | S×J×3 | f64 | bone-tail blend shapes | yes |
| `bone_rolls_rotmat` | 1×J×3×3 | f64 (float32 values) | roll rotation about each bone's local Y | yes (dump as stored) |
| `vertex_bone_indices` | V×K | i64 | influencing bones per vertex | yes |
| `vertex_bone_weights` | V×K | f64 | their weights; each row sums to 1 within 2e-16 | yes |
| `bone_labels`, `bone_parents` | J | str, int | hierarchy (Appendix B); `parents[j] < j`, root parent −1 | yes |
| `base_mesh_vertex_indices` | V | i64 | final → base index map | yes (landmarks, the waist loop) |
| `texture_coordinates`, `face_texture_coordinate_indices` | 21,334×2, F×3 | f64, i64 | UV layout | only for rendering |
| `y_axis`, `degenerate_rotation`, phenotype anchors, newborn scale, density | – | – | constants given in this document | no |

**Not needed:** facial-action blend shapes (`faceunits01`), the SMPL/SMPL-X/SOMA topologies and rigs
(`data/soma`, `data/cached/soma.pth`, the non-commercial download), `data/cached/anny.pth`, the keypoint
regressors (`data/keypoints/coco.pth`), segmentation images, the procrustes buffers, the warp module, the
alternative rigs (`cmu_mb`, `game_engine`, `mixamo`), and the other `topology/*.obj` files.

**Size.** `blendshapes` is 374 MB dense in f64 and 187 MB in f32; `bone_*_blendshapes` are 4.4 MB each in f64.
32 % of the S×V vertex rows are non-zero (53 % of the phenotype rows, 7 % of the local-change rows), so a sparse
layout (u32 vertex index + 3×f32) of the non-zero rows needs 80 MB. Two reductions exist (§2.4): the 216 breast
rows always get coefficient 0 in this configuration (exact), and the 30 race rows always combine with fixed
weights (exact up to rounding). Every non-newborn row is an integer multiple of 1e-4 m to rounding (|n| ≤ 10,670,
fits i16); the 60 newborn
rows are derived (§2.5). The converter should record the SHA-256 of each array (§8.1 lists the package's own).

### 1.3 Blend-shape rows

Rows are stacked in this order (`load_all_blendshapes`); within a block the components vary in the nested-loop
order shown (outermost first), each over its value list in §2.1:

| Rows | Block | Count | Components (loop order) | Files (`data/mpfb2/targets/`) |
|---|---|---|---|---|
| 0–89 | `universal` | 90 | gender, age, muscle, weight | `macrodetails/universal-{gender}-{age}-{muscle}-{weight}` |
| 90–119 | `race` | 30 | race, gender, age | `macrodetails/{race}-{gender}-{age}` |
| 120–299 | `height` | 180 | gender, age, muscle, weight, height | `macrodetails/height/{gender}-{age}-{muscle}-{weight}-{height}` |
| 300–407 | `proportions` | 108 | gender, age ∈ {child, young, old}, muscle, weight, proportions | `macrodetails/proportions/...` |
| 408–623 | `breast` | 216 | female only; age ∈ {child, young, old}, muscle, weight, cupsize, firmness; the 27 `averagecup-averagefirmness` files do not exist and are skipped | `breast/female-...` |
| 624–1135 | `local_change` | 512 | label j gives rows 624 + 2j (positive target) and 625 + 2j (negative target) | `target.json` order, §3 |

A target file is gzip text, one line per displaced vertex: `base_index dx dy dz` in decimetres with three
decimals; vertices not listed have zero offset. Each target becomes W·offset. Files for `newborn` do not exist;
newborn rows are derived from the `baby` file (§2.5). The eight `universal-{gender}-{baby|child|young|old}-averagemuscle-averageweight`
targets are empty (all-zero rows): in MakeHuman the age-and-gender shape lives in the race targets, and the
universal targets only add muscle and weight deviations.

### 1.4 Bone heads and tails (rig regressors)

Each bone's head and tail in `rig.default.json` names one regressor:

| Strategy | Point | Count (heads / tails) |
|---|---|---|
| `VERTEX` | the base vertex `vertex_index` | 37 / 47 |
| `CUBE` | mean of the 8 distinct vertices used by the faces of the OBJ group `cube_name` (a `joint-*` helper cube) | 113 / 103 |
| `MEAN` | mean of the listed `vertex_indices` (always 2 here) | 13 / 13 |

`template_bone_heads[j] = mean(T_base[regressor])` and `bone_heads_blendshapes[k, j] = mean(B_k,base[regressor])`,
over the full 19,158-vertex base mesh before any vertex removal. Tails are built the same way; the code adds a
per-bone tail offset that is always zero. Because the regressors are linear, rest heads and tails are linear in the
blend coefficients (§4.2). The joint cubes are not in the final mesh, so the port cannot recompute these: it must
read the converted arrays.

### 1.5 Bone hierarchy

The root is the one bone without a parent (`root`). Bones are numbered by a depth-first pre-order walk from it,
visiting children in the key order of `rig.default.json` (`load_rig.parse_recursively`). Hence `parents[j] < j`
for every j > 0 and forward kinematics can run in index order. 134 bones carry skinning weight; `root` and 28
face/tongue bones carry none. Appendix B lists all 163.

### 1.6 Bone rolls

`bone_rolls_rotmat[0, j] = R_y(roll_j)` with `roll_j` the bone's `roll` (radians) from `rig.default.json` and
`R_y(φ) = [[cos φ, 0, sin φ], [0, 1, 0], [−sin φ, 0, cos φ]]`. The package evaluates this in **float32**
(`torch.tensor([rolls])` defaults to float32) and widens to float64, so the stored matrices deviate from exact
rotations by up to 1.2e-7 per entry (some middle entries are 1.00000012 instead of 1) and from orthonormality by
up to 2.9e-7. Dump them as stored (§9.3).

### 1.7 Skinning weights

`weights.default.json` (its own description: "Default Anny skinning weights, symmetrized and island-free",
version 110, CC0) maps each bone label to `[base_vertex, weight]` pairs. Per vertex, entries are collected in
bone-index order, padded with (bone 0, weight 0) to the longest list, divided by their row sum (float64), subset
by step 5 and compacted by step 6. Result: K = 9 slots; non-zero entries first, in ascending bone index; padding
slots hold index 0 and weight 0 (the root has no weight, so padding is inert). Influence counts per vertex:
1: 2,415 · 2: 4,359 · 3: 2,826 · 4: 1,476 · 5: 1,093 · 6: 781 · 7: 391 · 8: 359 · 9: 18. No renormalisation
happens at runtime.

---

## 2. Phenotype interpolation

### 2.1 The 26 phenotype components

`PHENOTYPE_VARIATIONS` (this order is also the column order of the mask):

| Feature | Components (anchor value) | Exposed |
|---|---|---|
| race | african, asian, caucasian | no: each fixed at 0.5 |
| gender | male (0), female (1) | yes |
| age | newborn (−1/3), baby (0), child (1/3), young (2/3), old (1) | yes |
| muscle | minmuscle (0), averagemuscle (0.5), maxmuscle (1) | yes |
| weight | minweight (0), averageweight (0.5), maxweight (1) | yes |
| height | minheight (0), maxheight (1) | yes |
| proportions | idealproportions (0), uncommonproportions (1) | yes |
| cupsize | mincup (0), averagecup (0.5), maxcup (1) | no: fixed at 0.5 |
| firmness | minfirmness (0), averagefirmness (0.5), maxfirmness (1) | no: fixed at 0.5 |

**gender = 0 is male, 1 is female.** Age anchors are `linspace(−1/3, 1, 5)`; every other feature uses
`linspace(0, 1, n)`. An omitted input is 0.5 (`_parse_parameter_kwargs`). The tensor form is a B×6 array in the
order gender, age, muscle, weight, height, proportions.

### 2.2 Anchor weights (`linear_interpolation_coefficients`)

For a value x and anchors a₀ < a₁ < … < a_{n−1}:

1. Clamp (default, `extrapolate_phenotypes=False`): x ← a₀ if x < a₀; x ← a_{n−1} if x > a_{n−1}.
2. i = `searchsorted(a, x, side="left")` (the first index with aᵢ ≥ x), then i ← clamp(i, 1, n − 1).
3. α = (x − a_{i−1}) / (aᵢ − a_{i−1}); w_{i−1} = 1 − α; wᵢ = α; all other weights 0.

So a value exactly on an inner anchor aₖ gets wₖ = 1 (via the segment ending at aₖ), x = a₀ gets w₀ = 1, and the
weights of one feature always sum to 1. With extrapolation on, step 1 is skipped and the end segment is
extended linearly (weights may be negative or exceed 1). Race weights are the race values divided by their sum,
here 1/3 each (all-zero values would give NaN, replaced by 1/3).

At 0.5, muscle and weight give `average…` = 1; height gives min = max = 0.5; proportions gives ideal =
uncommon = 0.5; cupsize and firmness give `average…` = 1.

### 2.3 Coefficient of each phenotype row

Let w_m be the weight of component m from §2.2 (race: 1/3). A phenotype row k whose label names the components
C_k (e.g. `height:female-child-averagemuscle-averageweight-maxheight`) gets

  c_k = Π_{m ∈ C_k} w_m  (= Π_{m=0}^{25} (w_m·M_km + 1 − M_km) with the 0/1 mask M).

Examples: `universal:g-a-mu-we` = w_g·w_a·w_mu·w_we; `race:r-g-a` = (1/3)·w_g·w_a; `height:g-a-mu-we-h` =
w_g·w_a·w_mu·w_we·w_h; `proportions:g-a-mu-we-p` = w_g·w_a·w_mu·w_we·w_p (zero unless age > 0, since
proportions rows exist only for child, young, old); `breast:female-a-mu-we-cup-firm` =
w_female·w_a·w_mu·w_we·w_cup·w_firm.

**Worked example** (§8 case 4, girl, age 0.1325): w_baby = 0.6025, w_child = 0.3975, w_female = 1,
average muscle/weight = 1. Non-zero coefficients: `universal:female-{baby,child}-averagemuscle-averageweight`
0.6025 / 0.3975 (empty rows); `race:{african,asian,caucasian}-female-baby` 0.200833… each and `…-child`
0.1325 each; `height:female-{baby,child}-averagemuscle-averageweight-{min,max}height` 0.30125 / 0.19875 each;
`proportions:female-child-averagemuscle-averageweight-{ideal,uncommon}proportions` 0.19875 each.

### 2.4 Inert and foldable rows in this configuration

- **Breast rows are always 0**: cupsize = firmness = 0.5 puts all weight on `averagecup-averagefirmness`, which has
  no file. A port fixed to these six phenotypes can drop rows 408–623.
- **Race rows always combine as (1/3)·Σ_race**: they can be folded into 10 per-(gender, age) rows. Folding changes
  results only by rounding (~1e-16 m).

### 2.5 Newborn rows

There are no newborn targets. For each `baby` target of the universal, race and height blocks, the newborn row is

  B_newborn = s ⊙ B_baby + ((s − 1) / 3) ⊙ T,  s = (0.922, 0.922, 0.75) (x, y, z in the final frame).

At age = −1/3 the newborn coefficients of those three blocks each sum to 1, 3 in total, so the shape is
X = s ⊙ (T + Σ c·B_baby): the baby body scaled about the template origin. The MakeHuman targets keep the soles
at z ≈ −0.82 m for every body from baby to old, but this scaling about the origin lifts them to −0.61 m at
age −1/3 (§9.4). The bone-head and bone-tail rows follow by linearity (§1.4).

### 2.6 What the age parameter means

The age parameter is morphological, on the anchor scale above, not in years. The package's WHO-calibrated map
(§7.1) puts 0 y at 0.0, 1 y at 0.05, 4 y at 0.215, 11 y at 0.415, 16 y at 0.67, 18 y at 0.77, 64 y at 0.83 and
110 y at 1.0. The default 0.5 is about 12.7 y, an adult of 30 is 0.7857, and the newborn anchor −1/3 lies outside
the calibrated range.

---

## 3. Local changes

- **Labels.** 256, in the order of `data/mpfb2/targets/target.json`: for each region key except `genitals`, for
  each category that has `opposites`, for side in (left, right, unsided), when both the negative and positive
  target names are non-empty, one local change named after the **positive** target (Appendix A). The categories
  without opposites (head shapes, `chin-triangle`, `eye-eyefold-concave-convex`) are not used.
- **Coefficients.** For value v_j (default 0; B×256 tensor form in label order):
  row 624 + 2j gets max(v_j, 0) (positive target), row 625 + 2j gets max(−v_j, 0) (negative target).
  So v = +0.5 applies half the `…-incr` (or `up`, `out`, `forward`…) target, v = −0.5 half the opposite target.
- **Range.** None is enforced: values beyond ±1 extrapolate linearly (§8 case 13 uses +1.5). ±1 is the authoring
  range of the MakeHuman targets.
- **Gradient at 0.** The package writes the split so that autograd sees d/dv = B_pos − B_neg at v = 0 (both
  sides active); a Rust fitter should choose a one-sided or central rule deliberately.
- **Absolute, asymmetric offsets.** Targets are fixed displacements authored on the adult base mesh; they do not
  scale with the body. Measured on the rest mesh (§6) for a 2.5-year-old girl, an adult woman and an adult man:
  `measure-waist-circ-incr` +1 adds 10.21 / 10.10 / 10.19 cm of waist and −1 removes 9.50 / 9.68 / 9.84 cm;
  `measure-hips-circ-incr` ±1 moves the waist loop by only +0.4 / −0.23 cm; `measure-upperleg-height-incr` +1 adds
  10.37 cm of stature and −1 removes 7.50 cm, at every age. Fitting priors for local changes must therefore
  depend on body size.
- **The skeleton follows.** Local-change rows of `bone_heads_blendshapes` and `bone_tails_blendshapes` are non-zero
  wherever a regressor vertex moves (376 of the 512 rows move at least one head). `nipple-point-incr/decr` move no
  final vertex (their vertices were removed by the nudity edit) but still move the breast bone tails (§9.7).
- **The twenty `measure-*` changes** (local-change index j in brackets): `measure-upperarm-length-incr` [6],
  `measure-upperarm-circ-incr` [15], `measure-lowerarm-length-incr` [20], `measure-ankle-circ-incr` [118],
  `measure-wrist-circ-incr` [133], `measure-calf-circ-incr` [151], `measure-lowerleg-height-incr` [152],
  `measure-upperleg-height-incr` [173], `measure-knee-circ-incr` [176], `measure-thigh-circ-incr` [177],
  `measure-neck-circ-incr` [202], `measure-neck-height-incr` [203], `measure-underbust-circ-incr` [241],
  `measure-napetowaist-dist-incr` [244], `measure-waisttohip-dist-incr` [245], `measure-shoulder-dist-incr` [246],
  `measure-hips-circ-incr` [247], `measure-waist-circ-incr` [251], `measure-frontchest-dist-incr` [252],
  `measure-bust-circ-incr` [254]. Each negative target is the same name with `-decr`.

---

## 4. Rest shape and rest skeleton

### 4.1 Coefficient vector

c ∈ R^S: rows 0–623 from §2.3, rows 624–1135 from §3 (no facial-action rows in this configuration).

### 4.2 Rest vertices, heads, tails (`apply_linear_blendshape`)

  X = T + Σ_k c_k·B_k  (V×3),  h = h_T + Σ_k c_k·H_k,  t = t_T + Σ_k c_k·U_k  (J×3 each),

with B = `blendshapes`, H = `bone_heads_blendshapes`, U = `bone_tails_blendshapes`. The package evaluates each
as `template + einsum("cpd,bc->bpd")`.

### 4.3 Rest bone frames (`kinematics.get_bone_poses`, orientation "blender")

For each bone j, with Ŷ = (0, 1, 0):

1. y = (t_j − h_j) / ‖t_j − h_j‖.
2. n = y × Ŷ; θ = atan2(‖n‖, y·Ŷ); a = n / ‖n‖.
3. R⁰ = rot(−θ·a) when |a·a − 1| < 0.1, else the fixed `degenerate_rotation` diag(1, −1, −1). The second branch
   fires only when ‖n‖ = 0 (a is NaN), i.e. y exactly ±Ŷ. R⁰ is the minimal rotation taking Ŷ to y (R⁰·Ŷ = y).
4. R_j = R⁰ · `bone_rolls_rotmat[j]` (roll about the bone's own Y axis).
5. Root: R_0 = I (`root_identity_orientation=True`); its translation stays h_0.
6. G_j = [R_j | h_j]: the rest pose of bone j (`rest_bone_poses`).

Local +Y of every non-root bone points from head to tail; X and Z come from the roll (Blender's bone-matrix
convention). The closest approach to the degenerate case over a grid of extreme phenotypes is `tongue02`
(|n| = 8e-4, pointing almost exactly −Y), where the frame is ill-conditioned but well-defined (§9.5).

Frames used by the reference poses (adult body, rest): `upperarm01.L` points (0.68, −0.02, −0.73) and its
local +Z is about world −Y, so a positive rotation about local +Z raises the left arm; `upperarm01.R`'s local +Z
is also about world −Y, so the same positive rotation lowers the right arm. `upperleg01.L/R` point about −Z with
local +X ≈ world +X: −90° about local +X swings the thigh forward (hip flexion). `lowerleg01.L/R` have local
+X ≈ world +X: +90° about local +X bends the knee.

---

## 5. Pose: local-bone kinematics and skinning

### 5.1 Pose input

A B×J×4×4 array Δ of homogeneous transforms, one per bone in bone order (`parse_delta_transforms_dict`).
Accepted forms: the full array; a dict {bone label: B×4×4} with missing bones set to identity; `None` = identity
for batch 1. The rotation part should be a rotation (the package does not check); the translation part is
allowed on every bone. Rotation representation is up to the caller (the package's examples build Δ from
axis-angle with `roma`).

Meaning under `local-bone`: Δ_0 is the **world pose of the root bone** (identity places the root head at the origin
with world-aligned axes); Δ_j for j > 0 is a rigid motion of bone j about its own head, expressed in its own rest
frame (§4.3), applied after the rest pose.

### 5.2 Forward kinematics (`get_bone_transforms` → `parallel_forward_kinematic`)

The reference poses equal the rest poses (no `reference_bone_orientations` in this configuration). With
G_j from §4.3 and Δ_j from §5.1, in index order:

  M_j = G_j · Δ_j
  P_0 = inv(G_0) · M_0  (= Δ_0 up to rounding),  P_j = A_{parent(j)} · M_j for j > 0
  A_j = P_j · inv(G_j)

P_j = `bone_poses` (world pose of each bone); A_j is the skinning transform. The package computes A twice
(inside the FK and again as `bone_poses @ inv(rest_bone_poses)`), with identical values. Bones of one tree depth
are computed together, which does not change the arithmetic of each product.

**Identity pose.** With Δ = I every A_j = inv(G_0) = translation by −h_0, so the output is the rest mesh moved
so that the root head (≈ pelvis, 0.06 m behind and 0.03 m above the template origin for the default body) is at
the origin. The package's result differs from X − h_0 by up to 2e-7 m because the float32 roll matrices are not
exactly orthonormal (§9.3).

### 5.3 Linear blend skinning (`skinning.linear_blend_skinning`)

For vertex i with slots (b_ik, w_ik), k = 1..9:

  x'_i = (Σ_k w_ik · A_{b_ik})[0:3, :] · [X_i; 1]

The transforms are blended first, then applied (the Warp kernel applies each transform and sums the points;
the results agree to rounding). Weights are not renormalised at runtime. Dual-quaternion skinning exists in the
package but is not the default.

### 5.4 Outputs of `forward`

`rest_vertices` B×V×3 (X), `rest_bone_heads` B×J×3, `rest_bone_tails` B×J×3, `rest_bone_poses` B×J×4×4 (G),
`vertices` B×V×3 (skinned), `bone_poses` B×J×4×4 (P). With `return_bone_ends=True` also posed heads and tails,
computed as (P_j·inv(G_j)) applied to the rest head and tail.

### 5.5 Inverse: pose parameters from bone poses (`get_pose_parameterization`)

Root: Δ_0 = P_0. Other bones: Δ_j = inv(inv(G_{p})·G_j) · (inv(P_{p})·P_j), p = parent(j), with rigid inverses.
Useful for transferring poses between bodies and for initialising a pose fit.

---

## 6. Anthropometry (`anthropometry.Anthropometry`)

All five quantities are computed on `rest_vertices` (the A-posed rest mesh before skinning) of the triangulated
model:

| Quantity | Definition |
|---|---|
| height | max over vertices of z − min over vertices of z (m). Every vertex counts, including eyes and tongue |
| waist circumference | the closed polyline through 46 fixed vertices (Appendix C), in the listed order: Σ_{i} ‖v_i − v_{i−1}‖ with v_{−1} = v_45 (m). A fixed vertex loop, not a tape: it follows hollows |
| volume | \|Σ_f (v0_f × v1_f)·v2_f\| / 6 over all 27,420 triangles (m³). The mesh is closed, so the value does not depend on the origin. It includes the tongue and both eyeballs (together 7.6e-5 m³ = 0.076 L at the default body), which sit inside the head and are counted on top of the body volume |
| mass | 980 × volume (kg); density 980 kg/m³ |
| BMI | mass / height² (kg/m²) |

The model rejects non-triangular faces and requires all 46 waist vertices to survive vertex removal.

---

## 7. Shape distribution (`shape_distribution.SimpleShapeDistribution`)

A sampler of plausible phenotypes, "calibrated to match the height vs age distribution of WHO data". It reads
`data/shape_calibration/boys.pth` and `girls.pth` (float64 tables, given in full below).

### 7.1 Morphological age ↔ age parameter

Piecewise-linear with linear **extrapolation** beyond both ends (§2.2 without clamping); identical for both sexes:

| Years | 0 | 1 | 4 | 11 | 16 | 18 | 64 | 110 |
|---|---|---|---|---|---|---|---|---|
| Age parameter | 0.0 | 0.05 | 0.215 | 0.415 | 0.67 | 0.77 | 0.83 | 1.0 |

### 7.2 Sampling

1. Years ~ Uniform(0, 90) (default); age = map(years). Gender ~ Uniform(0, 1).
2. Boys' tables if gender ≤ 0.5, girls' otherwise.
3. height ~ Beta(α(age), β(age)) where α and β are interpolated (clamped, §2.2) over the age-parameter anchors
   (0, 0.05, 0.215, 0.415, 0.67); beyond 0.67 (16 y) they stay at the last anchor.
4. weight, muscle ~ Beta(α, β) interpolated the same way, but over the anchors (0, 1, 4, 11, 16, 18, 64, 110),
   which are in **years**, evaluated at the age **parameter** ∈ [0, 1]. Only the first segment (the 0 y and 1 y
   entries) is ever used (§9.11).
5. proportions ~ Beta(1.0000005, 1.0000005) at every age (uniform).
6. Returns (years, {gender, age, muscle, weight, height, proportions}).

Height Beta parameters (the useful prior for fitting; mean = α/(α+β), sd = √(αβ/((α+β)²(α+β+1)))):

| Age parameter (years) | Boys α | Boys β | Boys mean ± sd | Girls α | Girls β | Girls mean ± sd |
|---|---|---|---|---|---|---|
| 0.0 (0 y) | 21.5957 | 58.6005 | 0.2693 ± 0.0492 | 29.5207 | 96.7224 | 0.2338 ± 0.0375 |
| 0.05 (1 y) | 51.9783 | 57.3600 | 0.4754 ± 0.0475 | 66.2820 | 74.5405 | 0.4707 ± 0.0419 |
| 0.215 (4 y) | 22.1473 | 45.8586 | 0.3257 ± 0.0564 | 31.8161 | 59.4907 | 0.3485 ± 0.0496 |
| 0.415 (11 y) | 13.4045 | 22.1050 | 0.3775 ± 0.0802 | 21.2642 | 23.9753 | 0.4700 ± 0.0734 |
| 0.67 (16 y) | 16.4956 | 25.6562 | 0.3913 ± 0.0743 | 13.6839 | 25.7476 | 0.3470 ± 0.0749 |

Full-precision values: boys α (21.59569876450918, 51.9783334557359, 22.147254386939128, 13.404505930812913,
16.495562848906868), β (58.60050612100851, 57.360044726733896, 45.85863887866753, 22.104951191986984,
25.656183288468334); girls α (29.520749046886845, 66.28195905142273, 31.81614447180559, 21.26417254409934,
13.683852173198092), β (96.72243255765622, 74.54048997863622, 59.49074917535684, 23.975297996662132,
25.74761089445589).

Weight and muscle tables (anchors in years 0, 1, 4, 11, 16, 18, 64, 110):

| Table | α | β |
|---|---|---|
| boys weight | 1.5128728, 0.2730224, 0.9053267, 0.3063071, 0.2341491, 0.7016360, 0.3144573, 0.2512586 | 0.9485069, 0.0093687, 1.2030187, 0.9829350, 0.6836293, 1.2365860, 0.2214497, 0.1742256 |
| girls weight | 1.5597290, 0.3190704, 2.2728324, 0.7031491, 0.7232687, 0.8814059, 1.8921328, 1.9019638 | 0.9010457, 0.0108792, 0.9199190, 0.6017749, 0.5567191, 0.6262190, 0.6995137, 0.6808693 |
| boys muscle | 1.3058923, 0.0099563, 0.9128996, 0.5553824, 0.6273819, 0.6080248, 1.4601254, 0.9803529 | 1.0371088, 0.2929062, 0.9840489, 2.3419950, 0.7151289, 1.1924582, 1.6300795, 0.8095288 |
| girls muscle | 0.9999917, 0.0141861, 1.1285272, 0.5115916, 0.6257698, 0.5737932, 0.4601105, 0.5306272 | 1.0046303, 0.3945418, 0.9525918, 0.5593905, 0.6426863, 0.7388192, 1.9971594, 2.2776384 |

**For fitting priors:** the calibration lives in parameter space. The height parameter at a given age and sex has
the Beta prior above (mean ≈ 0.27–0.48, sd ≈ 0.04–0.08); stature in metres is whatever the forward model makes of
that parameter together with age, gender and the rest. Height 0.5 is a tall child (case 5 in §8 is 131.6 cm at
6 y). The weight and muscle priors are not age-resolved in practice (§9.11); treat them as weak.

---

## 8. Reference values for the parity suite

### 8.1 How to reproduce

```
python tools/body-model/capture_anny_reference.py OUT.json [--cache-dir DIR] [--timing-repeats N]
```

The script refuses any installed version other than 0.6.1 (unless `--allow-other-version`), builds the model with

```
anny.create_fullbody_model(rig="default", topology="default", local_changes="all",
    remove_unattached_vertices=True, triangulate_faces=True, pose_parameterization="local-bone",
    bone_orientation="blender-rootidentity", skinning_method="lbs")
```

and calls `model(pose_parameters=Δ (1×163×4×4 float64), phenotype_kwargs={name: tensor([v])},
local_changes_kwargs={label: tensor([v])})` for each case, then `anny.Anthropometry(model)` on
`output["rest_vertices"]`. Everything is float64; capture runs single-threaded. Two runs, and a run against a
freshly rebuilt model cache, gave bit-identical JSON (apart from timings). The JSON (`ftx-anny-reference/1`)
holds, per case: the exact inputs (with the 4×4 pose matrices), the 13 selected vertices of `rest_vertices` and
`vertices`, both bounding boxes, three checksums of each full vertex array, rest heads, tails and poses and posed
poses of 11 bones, bone checksums, the anthropometry, every non-zero phenotype coefficient (label and value),
and the deviation of the same model run in float32. It also stores the bone and local-change label lists, the
waist loop, and SHA-256 hashes of every model array (little-endian bytes, package dtypes):

| Array | Shape | SHA-256 |
|---|---|---|
| `template_vertices` | 13718×3 f64 | `c164a0971ec308792290e47fdc79c2acb24d1031c48635a83f04db188c48dac1` |
| `faces` | 27420×3 i64 | `72a1d21fc87e04b194835769265008a4bcac5fdb5a1b1be4846d850960c5bca1` |
| `blendshapes` | 1136×13718×3 f64 | `017ef11b1e72715c6c5157e31f5a154a15d90aff70cefce8af3a5462a5b26cd3` |
| `stacked_phenotype_blend_shapes_mask` | 624×26 f64 | `a6812444e83276c8ed37c1dd5ec47918199f1e1d64dda6edcbb9a3d066c5dc96` |
| `template_bone_heads` | 163×3 f64 | `9c32b2db4c4ddfe3ebd9ba8315c87b56072b79bbc1377569af6544d7e057a02b` |
| `bone_heads_blendshapes` | 1136×163×3 f64 | `5b04bc24fe5c0d709b4558f6694d3fd5ed07f6efa44e1eb5ac91a4a0e199990d` |
| `template_bone_tails` | 163×3 f64 | `747116aaf41f316ab92f61b030dd5811f58ff07fea0e166c40b8ff1d4f24b2ef` |
| `bone_tails_blendshapes` | 1136×163×3 f64 | `0b6effb52144da709b9c4843b7c2e806dac82198a7c541dc861099512926d578` |
| `bone_rolls_rotmat` | 1×163×3×3 f64 | `8a1ec0eba8a5a311d11b832db7595fb8bb36727d39687255e4978adf9a911bbf` |
| `vertex_bone_weights` | 13718×9 f64 | `3ef657203687297cbeec4fe3af14cf7e07798653a795542b98ba8cd86f47fdbd` |
| `vertex_bone_indices` | 13718×9 i64 | `8e2a604bc918dba96dcf4fae66fa9e202b69f44ea61d50025f3cd71391567e77` |
| `base_mesh_vertex_indices` | 13718 i64 | `705a0fbcf1aa5ae17de1ca3740765099a8df911d654b2a208de9c9228861a194` |

Label hashes (SHA-256 of the labels joined by `\n`): blend-shape labels
`913a1030e67b6841e25536a8b100722f161749f77f25cea83d650ea90dd88b5a`, bone labels
`6e0b89c6823e9902c841f7624d61a5d78809165decf2da117edc9ffcd9289de9`.

Checksums of an N×3 array: `sum` = Σ of all coordinates; `sum_sq` = Σ of squared coordinates;
`index_weighted` = Σ_{i=1..N} i·(x_i + 2y_i + 3z_i) / N (catches permutations).

### 8.2 Selected vertices

Chosen once on the default body and frozen (final index, base index): `first_vertex` 0 (0), `nose_tip` 297 (297),
`top_of_head` 881 (881), `right_hand_outermost` 2301 (2317), `right_elbow_near_joint` 3370 (3386),
`back_or_buttock_rearmost` 4202 (4218), `sole_lowest` 6148 (6164), `left_hand_outermost` 8953 (8985),
`waist_front` 10725 (10757, on the waist loop), `left_knee_near_joint` 11206 (11238), `tongue_first` 13348 (13380),
`left_eye_first` 13574 (14598), `last_vertex` 13717 (14741). The names describe the default body; on other bodies
they are only labels.

All 16 cases have 13,718 vertices and 27,420 faces.

### 8.3 Inputs and anthropometry (rest vertices)

"≈ years" is the §7.1 map of the clamped age; values in brackets are extrapolations below 0 y and mean nothing.
Height, waist, volume and mass are shown to 1 µm, 1 µm, 0.1 cm³ and 0.1 g; the JSON has full precision.

| # | Case | gender, age, muscle, weight, height, proportions | Local changes | ≈ years | Height m | Waist m | Volume m³ | Mass kg | BMI |
|---|---|---|---|---|---|---|---|---|---|
| 0 | `default_all_half` | 0.5, 0.5, 0.5, 0.5, 0.5, 0.5 | – | 12.67 | 1.626172 | 0.717960 | 0.0510139 | 49.9936 | 18.9052 |
| 1 | `newborn_anchor` | 0.5, −1/3, 0.5, 0.5, 0.5, 0.5 | – | (−6.67) | 0.498240 | 0.346948 | 0.0033080 | 3.2418 | 13.0592 |
| 2 | `birth_who` | 0.5, 0, 0.5, 0.5, 0.5, 0.5 | – | 0.00 | 0.664320 | 0.376780 | 0.0051885 | 5.0847 | 11.5217 |
| 3 | `one_year` | 0.5, 0.05, 0.5, 0.5, 0.5, 0.5 | – | 1.00 | 0.776866 | 0.414083 | 0.0074453 | 7.2964 | 12.0896 |
| 4 | `toddler_girl_2_5y` | 1, 0.1325, 0.5, 0.5, 0.5, 0.5 | – | 2.50 | 0.943810 | 0.459187 | 0.0116533 | 11.4203 | 12.8206 |
| 5 | `boy_6y` | 0, 0.2721, 0.5, 0.5, 0.5, 0.5 | – | 6.00 | 1.316477 | 0.628699 | 0.0291707 | 28.5873 | 16.4948 |
| 6 | `twelve_years_mixed` | 0.5, 0.466, 0.5, 0.5, 0.5, 0.5 | – | 12.00 | 1.583060 | 0.700992 | 0.0471361 | 46.1934 | 18.4326 |
| 7 | `adult_man` | 0, 0.7857, 0.5, 0.5, 0.5, 0.5 | – | 30.04 | 1.897511 | 0.888293 | 0.0834516 | 81.7826 | 22.7139 |
| 8 | `adult_woman` | 1, 0.7857, 0.5, 0.5, 0.5, 0.5 | – | 30.04 | 1.764029 | 0.755186 | 0.0616933 | 60.4594 | 19.4291 |
| 9 | `extreme_corner` | 0, 1, 1, 1, 0, 1 | – | 110.00 | 1.699973 | 0.990518 | 0.0855319 | 83.8213 | 29.0048 |
| 10 | `out_of_range_clamped` | 1.3, -0.6, -0.2, 1.5, 2, -1 | – | (−6.67) | 0.668308 | 0.471305 | 0.0070367 | 6.8960 | 15.4398 |
| 11 | `woman_waist_up_hips_down` | 1, 0.7857, 0.5, 0.5, 0.5, 0.5 | `measure-waist-circ-incr` +0.5, `measure-hips-circ-incr` -0.5 | 30.04 | 1.764029 | 0.804100 | 0.0619290 | 60.6904 | 19.5033 |
| 12 | `man_bust_leg_belly` | 0, 0.7857, 0.5, 0.5, 0.5, 0.5 | `measure-bust-circ-incr` +1, `measure-upperleg-height-incr` -1, `stomach-pregnant-incr` +0.6 | 30.04 | 1.822511 | 0.954681 | 0.0888533 | 87.0763 | 26.2156 |
| 13 | `toddler_waist_beyond_one` | 1, 0.1325, 0.5, 0.5, 0.5, 0.5 | `measure-waist-circ-incr` +1.5, `torso-scale-horiz-incr` -0.4 | 2.50 | 0.943810 | 0.578243 | 0.0124978 | 12.2478 | 13.7496 |
| 14 | `pose_arms_left_up_right_down` | 1, 0.7857, 0.5, 0.5, 0.5, 0.5 | – (posed, §8.6) | 30.04 | 1.764029 | 0.755186 | 0.0616933 | 60.4594 | 19.4291 |
| 15 | `pose_seated` | 0, 0.7857, 0.5, 0.5, 0.5, 0.5 | – (posed, §8.6) | 30.04 | 1.897511 | 0.888293 | 0.0834516 | 81.7826 | 22.7139 |

### 8.4 Rest bounding boxes and checksums

| # | Case | bbox min (m) | bbox max (m) | root head (m) | `sum` | `sum_sq` | `index_weighted` |
|---|---|---|---|---|---|---|---|
| 0 | `default_all_half` | (-0.516782, -0.329778, -0.819417) | (0.516782, 0.089694, 0.806755) | (0.000000, 0.060986, 0.032298) | 397.149225000 | 5266.654859834 | 566.281057286 |
| 1 | `newborn_anchor` | (-0.196958, -0.132501, -0.614768) | (0.196958, 0.049889, -0.116527) | (0.000000, 0.026544, -0.385320) | -5286.158317520 | 2065.462771403 | -7772.955239406 |
| 2 | `birth_who` | (-0.213620, -0.143710, -0.819690) | (0.213620, 0.054110, -0.155370) | (0.000000, 0.028790, -0.513760) | -6885.038666667 | 3556.339533825 | -10272.500923125 |
| 3 | `one_year` | (-0.248010, -0.164933, -0.819835) | (0.248010, 0.053252, -0.042969) | (0.000000, 0.032189, -0.450359) | -6038.738115417 | 3052.230168123 | -9012.495837496 |
| 4 | `toddler_girl_2_5y` | (-0.298440, -0.195339, -0.820930) | (0.298440, 0.054262, 0.122880) | (0.000000, 0.035925, -0.352720) | -4756.968154542 | 2640.825273161 | -7123.393220232 |
| 5 | `boy_6y` | (-0.413740, -0.268672, -0.819882) | (0.413740, 0.073692, 0.496595) | (0.000000, 0.051129, -0.154411) | -2044.334919007 | 3215.433060190 | -3026.039829162 |
| 6 | `twelve_years_mixed` | (-0.501715, -0.320685, -0.819712) | (0.501715, 0.086967, 0.763347) | (0.000000, 0.059040, 0.007128) | 62.551644800 | 4887.948880262 | 68.776449737 |
| 7 | `adult_man` | (-0.616451, -0.398224, -0.820891) | (0.616451, 0.108713, 1.076620) | (0.000000, 0.072413, 0.196600) | 2538.053788773 | 8345.949621066 | 3765.323609898 |
| 8 | `adult_woman` | (-0.549752, -0.361356, -0.818856) | (0.549752, 0.093543, 0.945174) | (0.000000, 0.062760, 0.119858) | 1437.362250138 | 6667.535211644 | 2133.160760058 |
| 9 | `extreme_corner` | (-0.497070, -0.323537, -0.820270) | (0.497070, 0.152660, 0.879703) | (0.000000, 0.060823, 0.074957) | 1295.056483333 | 5893.223335810 | 1757.848904871 |
| 10 | `out_of_range_clamped` | (-0.258178, -0.175604, -0.615035) | (0.258178, 0.052563, 0.053273) | (0.000000, 0.037608, -0.275707) | -3967.516095887 | 1618.817682820 | -5796.197945554 |
| 11 | `woman_waist_up_hips_down` | (-0.549752, -0.361356, -0.818856) | (0.549752, 0.088143, 0.945174) | (0.000000, 0.058560, 0.119858) | 1437.710300138 | 6667.681235915 | 2133.439244426 |
| 12 | `man_bust_leg_belly` | (-0.616451, -0.398224, -0.745891) | (0.616451, 0.140969, 1.076620) | (0.000000, 0.072413, 0.196600) | 2759.526368773 | 8071.281212778 | 4205.251192289 |
| 13 | `toddler_waist_beyond_one` | (-0.284920, -0.195339, -0.820930) | (0.284920, 0.065251, 0.122880) | (0.000000, 0.035925, -0.352720) | -4756.494904542 | 2613.775534055 | -7137.833386918 |
| 14 | `pose_arms_left_up_right_down` | (-0.549752, -0.361356, -0.818856) | (0.549752, 0.093543, 0.945174) | (0.000000, 0.062760, 0.119858) | 1437.362250138 | 6667.535211644 | 2133.160760058 |
| 15 | `pose_seated` | (-0.616451, -0.398224, -0.820891) | (0.616451, 0.108713, 1.076620) | (0.000000, 0.072413, 0.196600) | 2538.053788773 | 8345.949621066 | 3765.323609898 |

### 8.5 Selected rest-vertex coordinates (mm, 4 decimals)

Columns are the cases by number (§8.3). Each cell is `x, y, z` in millimetres.

| Vertex (index) | 0 `default_all_half` | 1 `newborn_anchor` | 2 `birth_who` | 3 `one_year` |
|---|---|---|---|---|
| first_vertex (0) | -34.1783, -136.3933, 700.1650 | -21.5010, -59.5243, -170.4325 | -23.3200, -64.5600, -227.2433 | -24.7025, -73.7775, -119.1271 |
| nose_tip (297) | 0.0000, -161.0867, 659.0833 | 0.0000, -76.5905, -184.9500 | 0.0000, -83.0700, -246.6000 | 0.0000, -92.8300, -141.2187 |
| top_of_head (881) | 0.0000, -51.3767, 806.7550 | 0.0000, -12.5945, -116.5275 | 0.0000, -13.6600, -155.3700 | 0.0000, -18.5375, -42.9688 |
| right_hand_outermost (2301) | -516.7825, -273.8767, 41.5450 | -196.9115, -109.4045, -381.9225 | -213.5700, -118.6600, -509.2300 | -247.9725, -136.5300, -444.5450 |
| right_elbow_near_joint (3370) | -347.8342, -5.2400, 284.0383 | -131.5971, 2.7752, -304.4775 | -142.7300, 3.0100, -405.9700 | -165.7813, 2.2712, -326.0438 |
| back_or_buttock_rearmost (4202) | -57.7775, 89.6942, 31.6275 | -15.2499, 34.0126, -380.1075 | -16.5400, 36.8900, -506.8100 | -21.0425, 42.8050, -444.0463 |
| sole_lowest (6148) | -152.0933, -165.9667, -819.4167 | -48.7369, -51.8625, -614.4500 | -52.8600, -56.2500, -819.2667 | -65.2100, -68.5925, -819.5067 |
| left_hand_outermost (8953) | 516.7825, -273.8767, 41.5450 | 196.9115, -109.4045, -381.9225 | 213.5700, -118.6600, -509.2300 | 247.9725, -136.5300, -444.5450 |
| waist_front (10725) | 18.1008, -128.5333, 160.7025 | 6.9888, -75.9728, -344.4450 | 7.5800, -82.4000, -459.2600 | 8.8488, -86.9288, -387.6362 |
| left_knee_near_joint (11206) | 170.8817, -61.9150, -339.1533 | 64.5308, -27.1898, -485.0775 | 69.9900, -29.4900, -646.7700 | 82.1475, -34.5175, -611.1150 |
| tongue_first (13348) | 0.0000, -131.1517, 624.8900 | 0.0000, -57.7572, -200.9075 | 0.0000, -62.6433, -267.8767 | 0.0000, -71.3008, -164.0729 |
| left_eye_first (13574) | 27.5217, -135.3000, 697.3133 | 17.8223, -59.6995, -171.9150 | 19.3300, -64.7500, -229.2200 | 20.3050, -73.7825, -121.3038 |
| last_vertex (13717) | -25.5383, -103.6617, 696.4917 | -13.1201, -35.6537, -173.3250 | -14.2300, -38.6700, -231.1000 | -15.6775, -47.0375, -123.0587 |

| Vertex (index) | 4 `toddler_girl_2_5y` | 5 `boy_6y` | 6 `twelve_years_mixed` | 7 `adult_man` |
|---|---|---|---|---|
| first_vertex (0) | -26.6259, -82.9642, 41.0625 | -31.5782, -127.0886, 398.5050 | -33.8434, -134.2751, 658.0118 | -36.3391, -152.5913, 958.5631 |
| nose_tip (297) | 0.0000, -103.3955, 15.4983 | 0.0000, -147.5577, 362.1285 | 0.0000, -158.4449, 617.6424 | 0.0000, -178.5418, 909.2089 |
| top_of_head (881) | 0.0000, -20.7421, 122.8800 | 0.0000, -52.2030, 496.5952 | 0.0000, -50.3159, 763.3472 | 0.0000, -64.7720, 1076.6195 |
| right_hand_outermost (2301) | -298.4296, -161.5238, -350.3360 | -413.7404, -225.1327, -131.5008 | -501.7145, -266.5157, 17.1585 | -616.4505, -334.0331, 213.3374 |
| right_elbow_near_joint (3370) | -198.8702, 3.1756, -208.6709 | -278.3310, -5.3707, 58.7768 | -337.3426, -4.5617, 251.9763 | -417.7856, -10.7732, 492.6061 |
| back_or_buttock_rearmost (4202) | -30.0351, 50.3189, -347.7570 | -37.8318, 73.6915, -150.3182 | -55.4885, 86.9665, 7.1449 | -68.0534, 108.7125, 187.3107 |
| sole_lowest (6148) | -80.5657, -84.4394, -820.9300 | -130.3813, -132.6965, -818.5334 | -148.6457, -160.3703, -819.7125 | -176.2446, -192.4293, -818.4880 |
| left_hand_outermost (8953) | 298.4296, -161.5238, -350.3360 | 413.7404, -225.1327, -131.5008 | 501.7145, -266.5157, 17.1585 | 616.4505, -334.0331, 213.3374 |
| waist_front (10725) | 10.6209, -91.1119, -276.0456 | 15.1444, -113.8003, -55.9534 | 17.6801, -125.2812, 131.6385 | 20.8110, -148.6329, 343.1326 |
| left_knee_near_joint (11206) | 100.3590, -45.8935, -557.5511 | 139.9469, -50.5233, -441.9195 | 166.8340, -62.1377, -353.4163 | 204.5804, -42.1567, -247.1601 |
| tongue_first (13348) | 0.0000, -79.7591, -9.1786 | 0.0000, -121.7404, 330.9634 | 0.0000, -128.9502, 583.9387 | 0.0000, -144.1297, 872.6344 |
| left_eye_first (13574) | 21.5891, -82.6242, 38.4499 | 25.3026, -126.3534, 395.6576 | 27.1766, -133.1920, 655.0666 | 29.5253, -151.4218, 957.0768 |
| last_vertex (13717) | -17.8804, -55.0337, 36.5831 | -22.4882, -96.1375, 395.1109 | -25.2000, -101.7832, 654.1991 | -27.6251, -117.7561, 958.1063 |

| Vertex (index) | 8 `adult_woman` | 9 `extreme_corner` | 10 `out_of_range_clamped` | 11 `woman_waist_up_hips_down` |
|---|---|---|---|---|
| first_vertex (0) | -34.3427, -148.3064, 836.2799 | -32.9533, -134.3267, 766.2900 | -23.9904, -70.5883, -7.3075 | -34.3427, -148.3064, 836.2799 |
| nose_tip (297) | 0.0000, -176.3699, 795.5709 | 0.0000, -157.5700, 719.8000 | 0.0000, -89.7751, -24.3750 | 0.0000, -176.3699, 795.5709 |
| top_of_head (881) | 0.0000, -62.0344, 945.1735 | 0.0000, -56.3433, 878.7967 | 0.0000, -17.9882, 53.2725 | 0.0000, -62.0344, 945.1735 |
| right_hand_outermost (2301) | -549.7520, -300.6291, 130.4941 | -497.0700, -274.4100, 150.6533 | -258.1323, -146.1462, -279.8100 | -549.7520, -300.6291, 130.4941 |
| right_elbow_near_joint (3370) | -373.3907, -27.4019, 392.7023 | -355.0133, -3.0733, 367.4133 | -174.2396, -0.4979, -179.1150 | -373.3907, -27.4019, 392.7023 |
| back_or_buttock_rearmost (4202) | -67.8602, 93.5429, 109.6308 | -57.2067, 103.9733, 55.2067 | -18.5691, 51.3001, -268.7700 | -64.6602, 88.1429, 109.6308 |
| sole_lowest (6148) | -153.4454, -185.1695, -818.8558 | -143.2433, -165.0000, -818.2167 | -61.3222, -67.3982, -614.6875 | -153.4454, -185.1695, -818.8558 |
| left_hand_outermost (8953) | 549.7520, -300.6291, 130.4941 | 497.0700, -274.4100, 150.6533 | 258.1323, -146.1462, -279.8100 | 549.7520, -300.6291, 130.4941 |
| waist_front (10725) | 19.2002, -144.6083, 272.5761 | 21.0467, -166.7333, 215.2400 | 9.4782, -93.7674, -224.1450 | 19.5502, -156.0083, 272.5761 |
| left_knee_near_joint (11206) | 177.1817, -81.8885, -292.0393 | 197.5733, -37.2067, -309.2533 | 80.5275, -30.9239, -426.8400 | 177.1817, -81.8885, -292.0393 |
| tongue_first (13348) | 0.0000, -144.2207, 762.0238 | 0.0000, -122.4933, 688.4900 | 0.0000, -68.7443, -42.2575 | 0.0000, -144.2207, 762.0238 |
| left_eye_first (13574) | 27.7646, -147.1118, 834.0686 | 25.6467, -133.1000, 765.7967 | 19.8968, -70.7174, -8.9025 | 27.7646, -147.1118, 834.0686 |
| last_vertex (13717) | -27.1729, -115.5604, 832.7743 | -26.9467, -101.7700, 767.9333 | -15.0102, -44.4127, -10.3500 | -27.1729, -115.5604, 832.7743 |

| Vertex (index) | 12 `man_bust_leg_belly` | 13 `toddler_waist_beyond_one` | 14 `pose_arms_left_up_right_down` | 15 `pose_seated` |
|---|---|---|---|---|
| first_vertex (0) | -36.3391, -152.5913, 958.5631 | -26.6259, -82.9642, 41.0625 | -34.3427, -148.3064, 836.2799 | -36.3391, -152.5913, 958.5631 |
| nose_tip (297) | 0.0000, -178.5418, 909.2089 | 0.0000, -103.3955, 15.4983 | 0.0000, -176.3699, 795.5709 | 0.0000, -178.5418, 909.2089 |
| top_of_head (881) | 0.0000, -64.7720, 1076.6195 | 0.0000, -20.7421, 122.8800 | 0.0000, -62.0344, 945.1735 | 0.0000, -64.7720, 1076.6195 |
| right_hand_outermost (2301) | -616.4505, -334.0331, 213.3374 | -284.9096, -161.5238, -350.3360 | -549.7520, -300.6291, 130.4941 | -616.4505, -334.0331, 213.3374 |
| right_elbow_near_joint (3370) | -417.7856, -10.7732, 492.6061 | -185.3502, 3.1756, -208.6709 | -373.3907, -27.4019, 392.7023 | -417.7856, -10.7732, 492.6061 |
| back_or_buttock_rearmost (4202) | -68.0534, 108.7125, 187.3107 | -30.0351, 50.3189, -347.7570 | -67.8602, 93.5429, 109.6308 | -68.0534, 108.7125, 187.3107 |
| sole_lowest (6148) | -177.5446, -192.4293, -743.4880 | -80.5657, -84.4394, -820.9300 | -153.4454, -185.1695, -818.8558 | -176.2446, -192.4293, -818.4880 |
| left_hand_outermost (8953) | 616.4505, -334.0331, 213.3374 | 284.9096, -161.5238, -350.3360 | 549.7520, -300.6291, 130.4941 | 616.4505, -334.0331, 213.3374 |
| waist_front (10725) | 20.9910, -200.6529, 324.7726 | 10.7809, -127.1119, -276.0456 | 19.2002, -144.6083, 272.5761 | 20.8110, -148.6329, 343.1326 |
| left_knee_near_joint (11206) | 205.8804, -42.1567, -172.1601 | 100.3590, -45.8935, -557.5511 | 177.1817, -81.8885, -292.0393 | 204.5804, -42.1567, -247.1601 |
| tongue_first (13348) | 0.0000, -144.1297, 872.6344 | 0.0000, -79.7591, -9.1786 | 0.0000, -144.2207, 762.0238 | 0.0000, -144.1297, 872.6344 |
| left_eye_first (13574) | 29.5253, -151.4218, 957.0768 | 21.5891, -82.6242, 38.4499 | 27.7646, -147.1118, 834.0686 | 29.5253, -151.4218, 957.0768 |
| last_vertex (13717) | -27.6251, -117.7561, 958.1063 | -17.8804, -55.0337, 36.5831 | -27.1729, -115.5604, 832.7743 | -27.6251, -117.7561, 958.1063 |

### 8.6 Posed cases

**`pose_arms_left_up_right_down`**: adult woman; upperarm01.L +60 deg about its local +Z (raises the left arm), upperarm01.R +40 deg about its local +Z (lowers the right arm).

| Bone | local-bone delta (rotation vector, degrees; translation m) |
|---|---|
| `upperarm01.L` | rotvec (0, 0, 60) |
| `upperarm01.R` | rotvec (0, 0, 40) |

Posed bbox min (-0.238565, -0.427959, -0.938714) m, max (0.767594, 0.030783, 0.825315) m; checksums `sum` 624.877556684, `sum_sq` 6900.578219007, `index_weighted` 972.509956686.

| Vertex (index) | posed x, y, z (mm) |
|---|---|
| first_vertex (0) | -34.3427, -211.0660, 716.4215 |
| nose_tip (297) | 0.0000, -239.1296, 675.7124 |
| top_of_head (881) | 0.0000, -124.7941, 825.3151 |
| right_hand_outermost (2301) | -158.2253, -368.0236, -124.8343 |
| right_elbow_near_joint (3370) | -198.1047, -93.1607, 187.2071 |
| back_or_buttock_rearmost (4202) | -67.8602, 30.7833, -10.2277 |
| sole_lowest (6148) | -153.4454, -247.9292, -938.7145 |
| left_hand_outermost (8953) | 765.8687, -342.6807, 576.0945 |
| waist_front (10725) | 19.2002, -207.3680, 152.7176 |
| left_knee_near_joint (11206) | 177.1817, -144.6482, -411.8979 |
| tongue_first (13348) | -0.0000, -206.9804, 642.1653 |
| left_eye_first (13574) | 27.7646, -209.8715, 714.2102 |
| last_vertex (13717) | -27.1729, -178.3200, 712.9159 |

| Bone | posed head (m) |
|---|---|
| `root` | (0.000000, 0.000000, 0.000000) |
| `lowerleg01.L` | (0.139580, -0.111423, -0.428288) |
| `foot.L` | (0.182676, -0.071193, -0.855322) |
| `lowerarm01.L` | (0.436222, -0.105479, 0.531866) |
| `wrist.L` | (0.609421, -0.249745, 0.575554) |

**`pose_seated`**: adult man; root yawed 15 deg about world +Z and moved (0.05, -0.10, -0.30) m; hips flexed 90 deg (upperleg01.L/R -90 deg about local +X); knees flexed 90 deg (lowerleg01.L/R +90 deg about local +X).

| Bone | local-bone delta (rotation vector, degrees; translation m) |
|---|---|
| `root` | rotvec (0, 0, 15), translation (0.05, -0.1, -0.3) |
| `upperleg01.L` | rotvec (-90, 0, 0) |
| `upperleg01.R` | rotvec (-90, 0, 0) |
| `lowerleg01.L` | rotvec (90, 0, 0) |
| `lowerleg01.R` | rotvec (90, 0, 0) |

Posed bbox min (-0.444655, -0.819542, -0.834249) m, max (0.751158, -0.044470, 0.580020) m; checksums `sum` -4499.085947773, `sum_sq` 6860.662905344, `index_weighted` -6233.338579862.

| Vertex (index) | posed x, y, z (mm) |
|---|---|
| first_vertex (0) | 73.1345, -326.7425, 461.9635 |
| nose_tip (297) | 114.9518, -342.4035, 412.6093 |
| top_of_head (881) | 85.5060, -232.5103, 580.0200 |
| right_hand_outermost (2301) | -440.2496, -652.1458, -283.2623 |
| right_elbow_near_joint (3370) | -332.0198, -288.4823, -3.9934 |
| back_or_buttock_rearmost (4202) | -18.4609, -106.8650, -332.2697 |
| sole_lowest (6148) | 158.5083, -777.5746, -826.4439 |
| left_hand_outermost (8953) | 750.6415, -333.0475, -283.2623 |
| waist_front (10725) | 127.2512, -307.8337, -153.4578 |
| left_knee_near_joint (11206) | 332.5784, -559.6381, -253.7859 |
| tongue_first (13348) | 106.0453, -309.1639, 376.0349 |
| left_eye_first (13574) | 136.4519, -308.5658, 460.4773 |
| last_vertex (13717) | 72.5355, -290.8389, 461.5067 |

| Bone | posed head (m) |
|---|---|
| `root` | (0.050000, -0.100000, -0.300000) |
| `lowerleg01.L` | (0.290298, -0.573454, -0.267449) |
| `foot.L` | (0.283115, -0.543200, -0.749816) |
| `lowerarm01.L` | (0.456868, -0.091517, -0.009347) |
| `wrist.L` | (0.641673, -0.245346, -0.143097) |

Non-posed cases: `vertices` (the posed output) equals the rest coordinates minus the root head of §8.4 to within
3e-7 m (§5.2); the JSON records both.

### 8.7 Time per forward pass (upstream, CPU)

Batch 1, float64, torch 2.14.1, arm64 macOS, after warm-up; median of 50 calls of `model(...)` (phenotype and
local-change parsing, blend shapes, rest skeleton, FK and torch LBS):

| Threads | Identity pose, default body | Seated pose + 3 local changes |
|---|---|---|
| 1 | 7.3 ms | 7.4 ms |
| 10 | 6.5 ms | 6.5 ms |

Building the model data from the bundled assets takes about 14 s once; later loads read the cache (§9.9).

### 8.8 Tolerances for the Rust port

| Port precision | Expected deviation from the JSON | Note |
|---|---|---|
| f64, same formulas, data dumped as stored | ≤ 1e-12 m on vertices and bone heads; anthropometry to ~1e-12 relative | the NumPy re-implementation reached 2e-15 m (8e-13 on bone matrices) |
| f32 runtime | ≤ 1e-6 m | the package itself run in float32 deviates by at most 3.3e-7 m (rest) and 6.4e-7 m (posed) on these cases |
| Gate G1a | 5e-5 m (0.05 mm) | both of the above pass with a wide margin |

Recommended: test the f64 path tightly (1e-9 m) and the f32 path at 1e-6 m. The `index_weighted` checksum needs
a relative tolerance (its magnitude reaches 1e4).

---

### Complete surface companion

`capture_anny_reference.py --full-vertices-from` captures every rest/posed coordinate for the
unchanged 16 JSON inputs using the pinned upstream package, after RECORD and model-hash checks.
The external `anny-0.6.1-full-vertices.bin` and its digest are documented in
`tools/body-model/PIN.md`. The existing sampled fixture is not regenerated. The companion adds
438,976 checked vertices, with the same 1e-9 m comparison tolerance; its input/header/file locks
and unsampled-coordinate mutation control distinguish a full-surface bound from sampled agreement.
Neither comparison independently qualifies the tape definitions, anatomical landmarks or physical fit.

## 9. Quirks and porting hazards

1. **Gender polarity.** gender 0 = male, 1 = female (§2.1); the package's shape sampler agrees (gender ≤ 0.5 uses
   the boys' tables). Swapping it silently produces plausible bodies of the other sex, so parity tests must
   include a sex-specific case (cases 4, 5, 7, 8). The first probe of the package for this project (the numbers in
   plan §9.1 and the README picture) used gender 0 for the "girl" and "woman" and gender 1 for the "boy" and
   "man"; read with this rule, each of those bodies is the other sex.
2. **Age is not in years.** The age domain is [−1/3, 1], 0.5 is about 12.7 y, adults are about 0.77–0.83, and the
   "newborn" anchor −1/3 lies below the calibrated 0 y (§2.6, §7.1). Anything that labels bodies must go through
   the §7.1 map. The same probe's "adults" at 0.55 and 0.6 are about 13.6 and 14.6 years old, its "toddler" at 0.12
   about 2.3 years and its "six-year-old" at 0.3 about 7 years.
3. **float32 inside a float64 model.** The roll matrices are float32 values (§1.6). Consequences: rest frames are
   not exactly orthonormal (2.9e-7), and the identity pose reproduces the rest mesh only to 2e-7 m. Dump the stored
   matrices and use the rigid inverse (transpose) exactly as §0 says; do not re-orthonormalise them, or bone
   matrices drift from the oracle by ~1e-7 (harmless against the gate, visible in tight f64 tests).
4. **The posed mesh is root-relative and nothing is grounded.** The output `vertices` put the root head (pelvis) at
   the origin. The rest mesh sits in the template frame (origin near mid-height of the default body): the soles
   stay at z ≈ −0.82 m from baby to old and the head top moves (0.12 m for the 2.5-year-old, 1.08 m for the
   adult man), but newborn weight lifts the soles (§2.5) and leg-length local changes move them too
   (`measure-upperleg-height-incr` −1 puts them at −0.74 m, case 12). Any "stand on the floor" placement is the
   caller's job.
5. **Degenerate bone direction.** If a bone pointed exactly along ±Y the package would use diag(1, −1, −1), even
   for +Y where the identity is correct. It never happens in practice; port the rule as written. `tongue02` comes
   within |y × Ŷ| = 8e-4 of the −Y singularity on extreme bodies, where small shape changes swing its frame. The
   minimal rotation from Ŷ to y is unique for θ < π, so any correct formula matches the package; use Rodrigues
   with θ = atan2 as §4.3 does to stay close in rounding.
6. **Volume and mass include the eyes and tongue** (§6), and the waist is a fixed vertex loop, not a tape.
7. **Inert data and odd targets.** Breast rows are always 0 here (§2.4); eight universal rows are empty (§1.3);
   `nipple-point-*` moves only the breast bones (§3); positive and negative targets of a pair differ in size
   (§3); local changes are absolute displacements, not scaled by body size (§3).
8. **Skinning backend.** With `skinning_method=None` the result depends on whether NVIDIA Warp is installed
   (Warp kernel vs torch kernel, equal to 4e-16 m). The capture forces `lbs`.
9. **Model cache.** The package caches built data under `~/.cache/anny/v12/` (or `$ANNY_CACHE_DIR`): a 535 MB
   `load_data_*.safetensors` and a 369 MB `build_model_data_*.safetensors` for this configuration. The cache key
   hashes the builder arguments but **not** the package version (only a data-format number, 12), so a cache left by
   another anny version with the same format number would be reused. The converter should build into a fresh
   `--cache-dir` and check the §8.1 hashes.
10. **API drift.** `create_fullbody_model` is deprecated in 0.6.1 and kept for its legacy defaults; `Anny()` with
    no arguments builds a different model (pruned `anny` rig, cached procrustes orientations from
    `data/cached/anny.pth`, `local-ref` poses). Moving the pin means re-checking §0.
11. **Shape-sampler units.** In `SimpleShapeDistribution`, weight and muscle Beta tables are indexed by years but
    queried with the age parameter (§7.2 step 4), so only their 0 y and 1 y entries matter, and at age 1.0 the boys'
    weight prior is Beta(0.27, 0.009) (mean 0.97). `ConditionalBetaDistribution.get_distribution_params` also
    computes the β interpolation coefficients twice (harmless). Port the height prior; treat the others as weak or
    replace them deliberately.
12. **Performance shape.** Upstream cost is dominated by the dense S×V×3 contraction (1,136 × 13,718 × 3 multiply-adds)
    plus a Python loop over the 256 local changes. A Rust port can skip zero coefficients (at most 92 of the 624
    phenotype rows are non-zero for any input; local changes are usually sparse) and store blend shapes
    sparsely (§1.2).
13. **Torch- and roma-specific operations, and what replaces them.** `searchsorted` (a linear scan over at most 5
    anchors), `einsum` (plain loops), `roma.rotvec_to_rotmat` (Rodrigues), `torch.linalg.cross`, `atan2`,
    `roma.Rigid.inverse` (transpose). No SVD, no procrustes, no autograd is needed for the forward model in this
    configuration. Fitting will need derivatives: everything up to the rest mesh is multilinear in the phenotype
    weights and piecewise linear in the inputs, and FK/LBS are smooth in Δ.
14. **Large data.** 374 MB of dense f64 blend shapes (§1.2). The converted file must stay out of the repository, and
    the wasm build needs the sparse or i16 layout.
15. **Batch broadcasting.** Batch sizes must match or be 1 (§0); the package raises otherwise. A Rust API with
    batch 1 only is a deliberate narrowing worth recording.

---

### Clothing collider (FrankenTextiles, not upstream parity)

Current public-prior 920 mm fitted bodies expose intersections between detailed facial triangles
whose vertices are dominated by `levator05.R`; the admitted file and array hashes are unchanged.
Forward-model parity does not establish a valid clothing collider. `clothing_skin` uses the
existing dominant-bone trim and reversed-boundary centroid caps for `levator05.R/L`, in addition
to wrists and feet. Missing facial groups do not create new geometry. Remaining source vertices
must be unchanged; all resulting topology and intersections must pass the ordinary strict audit.
This clothing collision approximation preserves measured/display body surfaces. It grants no
anatomical, facial-contact or empirical fit claim. The original elastic ladder, three-size armpit
wedge, two-start and fitted digest assertions pass unchanged with this proxy. Reports retain the
selected roots and synthetic cap count; a failed cap remains a refusal.

## Appendix A. Local-change labels (index j; blend-shape rows 624 + 2j and 625 + 2j)

Labels are listed in index order within each region; the regions are contiguous.

| Region | Index range | Positive labels (negative target in brackets where it is not the `-incr` → `-decr` swap) |
|---|---|---|
| arms | 0–24 | `l-lowerarm-scale-depth-incr`, `r-lowerarm-scale-depth-incr`, `l-upperarm-scale-horiz-incr`, `r-upperarm-scale-horiz-incr`, `l-upperarm-fat-incr`, `r-upperarm-fat-incr`, `measure-upperarm-length-incr`, `l-lowerarm-fat-incr`, `r-lowerarm-fat-incr`, `l-lowerarm-scale-horiz-incr`, `r-lowerarm-scale-horiz-incr`, `l-lowerarm-muscle-incr`, `r-lowerarm-muscle-incr`, `l-upperarm-shoulder-muscle-incr`, `r-upperarm-shoulder-muscle-incr`, `measure-upperarm-circ-incr`, `l-upperarm-scale-depth-incr`, `r-upperarm-scale-depth-incr`, `l-lowerarm-scale-vert-incr`, `r-lowerarm-scale-vert-incr`, `measure-lowerarm-length-incr`, `l-upperarm-muscle-incr`, `r-upperarm-muscle-incr`, `l-upperarm-scale-vert-incr`, `r-upperarm-scale-vert-incr` |
| breast | 25–30 | `breast-point-incr`, `nipple-point-incr`, `nipple-size-incr`, `breast-dist-incr`, `breast-volume-vert-up` [`breast-volume-vert-down`], `breast-trans-up` [`breast-trans-down`] |
| buttocks | 31–31 | `buttocks-volume-incr` |
| cheek | 32–39 | `l-cheek-trans-up` [`l-cheek-trans-down`], `r-cheek-trans-up` [`r-cheek-trans-down`], `l-cheek-volume-incr`, `r-cheek-volume-incr`, `l-cheek-inner-incr`, `r-cheek-inner-incr`, `l-cheek-bones-incr`, `r-cheek-bones-incr` |
| chin | 40–46 | `chin-jaw-drop-incr`, `chin-height-incr`, `chin-prominent-incr`, `chin-bones-incr`, `chin-width-incr`, `chin-cleft-incr`, `chin-prognathism-incr` |
| ears | 47–68 | `l-ear-lobe-incr`, `r-ear-lobe-incr`, `l-ear-shape-round` [`l-ear-shape-square`], `r-ear-shape-round` [`r-ear-shape-square`], `l-ear-trans-forward` [`l-ear-trans-backward`], `r-ear-trans-forward` [`r-ear-trans-backward`], `l-ear-rot-forward` [`l-ear-rot-backward`], `r-ear-rot-forward` [`r-ear-rot-backward`], `l-ear-trans-up` [`l-ear-trans-down`], `r-ear-trans-up` [`r-ear-trans-down`], `l-ear-scale-vert-incr`, `r-ear-scale-vert-incr`, `l-ear-flap-incr`, `r-ear-flap-incr`, `l-ear-shape-triangle` [`l-ear-shape-pointed`], `r-ear-shape-triangle` [`r-ear-shape-pointed`], `l-ear-scale-incr`, `r-ear-scale-incr`, `l-ear-scale-depth-incr`, `r-ear-scale-depth-incr`, `l-ear-wing-incr`, `r-ear-wing-incr` |
| eyebrows | 69–71 | `eyebrows-angle-up` [`eyebrows-angle-down`], `eyebrows-trans-up` [`eyebrows-trans-down`], `eyebrows-trans-forward` [`eyebrows-trans-backward`] |
| eyes | 72–103 | `l-eye-corner1-up` [`l-eye-corner1-down`], `r-eye-corner1-up` [`r-eye-corner1-down`], `l-eye-height1-incr`, `r-eye-height1-incr`, `l-eye-scale-incr`, `r-eye-scale-incr`, `l-eye-push1-out` [`l-eye-push1-in`], `r-eye-push1-out` [`r-eye-push1-in`], `l-eye-eyefold-up` [`l-eye-eyefold-down`], `r-eye-eyefold-up` [`r-eye-eyefold-down`], `l-eye-height3-incr`, `r-eye-height3-incr`, `l-eye-height2-incr`, `r-eye-height2-incr`, `l-eye-bag-incr`, `r-eye-bag-incr`, `l-eye-push2-out` [`l-eye-push2-in`], `r-eye-push2-out` [`r-eye-push2-in`], `l-eye-trans-up` [`l-eye-trans-down`], `r-eye-trans-up` [`r-eye-trans-down`], `l-eye-bag-height-incr`, `r-eye-bag-height-incr`, `l-eye-epicanthus-out` [`l-eye-epicanthus-in`], `r-eye-epicanthus-out` [`r-eye-epicanthus-in`], `l-eye-eyefold-angle-up` [`l-eye-eyefold-angle-down`], `r-eye-eyefold-angle-up` [`r-eye-eyefold-angle-down`], `l-eye-bag-out` [`l-eye-bag-in`], `r-eye-bag-out` [`r-eye-bag-in`], `l-eye-corner2-up` [`l-eye-corner2-down`], `r-eye-corner2-up` [`r-eye-corner2-down`], `l-eye-trans-out` [`l-eye-trans-in`], `r-eye-trans-out` [`r-eye-trans-in`] |
| feet | 104–118 | `l-foot-scale-horiz-incr`, `r-foot-scale-horiz-incr`, `l-foot-trans-out` [`l-foot-trans-in`], `r-foot-trans-out` [`r-foot-trans-in`], `l-foot-scale-vert-incr`, `r-foot-scale-vert-incr`, `l-foot-trans-forward` [`l-foot-trans-backward`], `r-foot-trans-forward` [`r-foot-trans-backward`], `l-foot-scale-depth-incr`, `r-foot-scale-depth-incr`, `l-foot-scale-incr`, `r-foot-scale-incr`, `l-foot-trans-up` [`l-foot-trans-down`], `r-foot-trans-up` [`r-foot-trans-down`], `measure-ankle-circ-incr` |
| forehead | 119–122 | `forehead-temple-incr`, `forehead-trans-forward` [`forehead-trans-backward`], `forehead-scale-vert-incr`, `forehead-nubian-incr` |
| hands | 123–133 | `l-hand-trans-out` [`l-hand-trans-in`], `r-hand-trans-out` [`r-hand-trans-in`], `l-hand-fingers-length-incr`, `r-hand-fingers-length-incr`, `l-hand-scale-incr`, `r-hand-scale-incr`, `l-hand-fingers-diameter-incr`, `r-hand-fingers-diameter-incr`, `l-hand-fingers-distance-incr`, `r-hand-fingers-distance-incr`, `measure-wrist-circ-incr` |
| head | 134–143 | `head-angle-out` [`head-angle-in`], `head-scale-vert-incr`, `head-trans-forward` [`head-trans-backward`], `head-trans-up` [`head-trans-down`], `head-fat-incr`, `head-trans-out` [`head-trans-in`], `head-back-scale-depth-incr`, `head-age-incr`, `head-scale-depth-incr`, `head-scale-horiz-incr` |
| hip | 144–150 | `hip-trans-out` [`hip-trans-in`], `hip-scale-horiz-incr`, `hip-scale-depth-incr`, `hip-waist-up` [`hip-waist-down`], `hip-trans-forward` [`hip-trans-backward`], `hip-scale-vert-incr`, `hip-trans-up` [`hip-trans-down`] |
| legs | 151–179 | `measure-calf-circ-incr`, `measure-lowerleg-height-incr`, `l-upperleg-fat-incr`, `r-upperleg-fat-incr`, `l-lowerleg-muscle-incr`, `r-lowerleg-muscle-incr`, `l-leg-valgus-incr`, `r-leg-valgus-incr`, `l-lowerleg-scale-depth-incr`, `r-lowerleg-scale-depth-incr`, `l-lowerleg-scale-vert-incr`, `r-lowerleg-scale-vert-incr`, `l-upperleg-scale-depth-incr`, `r-upperleg-scale-depth-incr`, `l-upperleg-scale-horiz-incr`, `r-upperleg-scale-horiz-incr`, `upperlegs-height-incr`, `l-lowerleg-fat-incr`, `r-lowerleg-fat-incr`, `l-lowerleg-scale-horiz-incr`, `r-lowerleg-scale-horiz-incr`, `lowerlegs-height-incr`, `measure-upperleg-height-incr`, `l-upperleg-muscle-incr`, `r-upperleg-muscle-incr`, `measure-knee-circ-incr`, `measure-thigh-circ-incr`, `l-upperleg-scale-vert-incr`, `r-upperleg-scale-vert-incr` |
| mouth | 180–201 | `mouth-lowerlip-height-incr`, `mouth-scale-horiz-incr`, `mouth-cupidsbow-incr`, `mouth-laugh-lines-out` [`mouth-laugh-lines-in`], `mouth-scale-depth-incr`, `mouth-lowerlip-volume-incr`, `mouth-angles-up` [`mouth-angles-down`], `mouth-upperlip-ext-up` [`mouth-upperlip-ext-down`], `mouth-lowerlip-width-incr`, `mouth-trans-forward` [`mouth-trans-backward`], `mouth-upperlip-width-incr`, `mouth-scale-vert-incr`, `mouth-dimples-out` [`mouth-dimples-in`], `mouth-upperlip-height-incr`, `mouth-upperlip-volume-incr`, `mouth-cupidsbow-width-incr`, `mouth-lowerlip-ext-up` [`mouth-lowerlip-ext-down`], `mouth-upperlip-middle-up` [`mouth-upperlip-middle-down`], `mouth-trans-up` [`mouth-trans-down`], `mouth-trans-out` [`mouth-trans-in`], `mouth-lowerlip-middle-up` [`mouth-lowerlip-middle-down`], `mouth-philtrum-volume-incr` |
| neck | 202–211 | `measure-neck-circ-incr`, `measure-neck-height-incr`, `neck-scale-depth-incr`, `neck-trans-forward` [`neck-trans-backward`], `neck-double-incr`, `neck-trans-out` [`neck-trans-in`], `neck-back-scale-depth-incr`, `neck-scale-horiz-incr`, `neck-scale-vert-incr`, `neck-trans-up` [`neck-trans-down`] |
| nose | 212–232 | `nose-nostrils-angle-up` [`nose-nostrils-angle-down`], `nose-flaring-incr`, `nose-curve-convex` [`nose-curve-concave`], `nose-scale-depth-incr`, `nose-scale-vert-incr`, `nose-point-width-incr`, `nose-scale-horiz-incr`, `nose-width1-incr`, `nose-trans-forward` [`nose-trans-backward`], `nose-trans-up` [`nose-trans-down`], `nose-hump-incr`, `nose-nostrils-width-incr`, `nose-trans-out` [`nose-trans-in`], `nose-greek-incr`, `nose-point-up` [`nose-point-down`], `nose-width2-incr`, `nose-width3-incr`, `nose-septumangle-incr`, `nose-base-up` [`nose-base-down`], `nose-compression-uncompress` [`nose-compression-compress`], `nose-volume-incr` |
| pelvis | 233–234 | `bulge-incr`, `pelvis-tone-incr` |
| stomach | 235–238 | `stomach-pregnant-incr`, `stomach-navel-out` [`stomach-navel-in`], `stomach-navel-up` [`stomach-navel-down`], `stomach-tone-incr` |
| torso | 239–255 | `torso-trans-forward` [`torso-trans-backward`], `torso-muscle-pectoral-incr`, `measure-underbust-circ-incr`, `torso-scale-horiz-incr`, `torso-scale-depth-incr`, `measure-napetowaist-dist-incr`, `measure-waisttohip-dist-incr`, `measure-shoulder-dist-incr`, `measure-hips-circ-incr`, `torso-vshape-incr`, `torso-trans-up` [`torso-trans-down`], `torso-muscle-dorsi-incr`, `measure-waist-circ-incr`, `measure-frontchest-dist-incr`, `torso-scale-vert-incr`, `measure-bust-circ-incr`, `torso-trans-out` [`torso-trans-in`] |

---

## Appendix B. Bone hierarchy (index, label, ←parent index)

Depth-first pre-order from `root` (§1.5). `.L` is the body's left (+X).

```
  0 root            (root)    1 pelvis.L        ←0        2 upperleg01.L    ←1        3 upperleg02.L    ←2
  4 lowerleg01.L    ←3        5 lowerleg02.L    ←4        6 foot.L          ←5        7 toe1-1.L        ←6
  8 toe1-2.L        ←7        9 toe2-1.L        ←6       10 toe2-2.L        ←9       11 toe2-3.L        ←10
 12 toe3-1.L        ←6       13 toe3-2.L        ←12      14 toe3-3.L        ←13      15 toe4-1.L        ←6
 16 toe4-2.L        ←15      17 toe4-3.L        ←16      18 toe5-1.L        ←6       19 toe5-2.L        ←18
 20 toe5-3.L        ←19      21 pelvis.R        ←0       22 upperleg01.R    ←21      23 upperleg02.R    ←22
 24 lowerleg01.R    ←23      25 lowerleg02.R    ←24      26 foot.R          ←25      27 toe1-1.R        ←26
 28 toe1-2.R        ←27      29 toe2-1.R        ←26      30 toe2-2.R        ←29      31 toe2-3.R        ←30
 32 toe3-1.R        ←26      33 toe3-2.R        ←32      34 toe3-3.R        ←33      35 toe4-1.R        ←26
 36 toe4-2.R        ←35      37 toe4-3.R        ←36      38 toe5-1.R        ←26      39 toe5-2.R        ←38
 40 toe5-3.R        ←39      41 spine05         ←0       42 spine04         ←41      43 spine03         ←42
 44 spine02         ←43      45 breast.L        ←44      46 breast.R        ←44      47 spine01         ←44
 48 clavicle.L      ←47      49 shoulder01.L    ←48      50 upperarm01.L    ←49      51 upperarm02.L    ←50
 52 lowerarm01.L    ←51      53 lowerarm02.L    ←52      54 wrist.L         ←53      55 finger1-1.L     ←54
 56 finger1-2.L     ←55      57 finger1-3.L     ←56      58 metacarpal1.L   ←54      59 finger2-1.L     ←58
 60 finger2-2.L     ←59      61 finger2-3.L     ←60      62 metacarpal2.L   ←54      63 finger3-1.L     ←62
 64 finger3-2.L     ←63      65 finger3-3.L     ←64      66 metacarpal3.L   ←54      67 finger4-1.L     ←66
 68 finger4-2.L     ←67      69 finger4-3.L     ←68      70 metacarpal4.L   ←54      71 finger5-1.L     ←70
 72 finger5-2.L     ←71      73 finger5-3.L     ←72      74 clavicle.R      ←47      75 shoulder01.R    ←74
 76 upperarm01.R    ←75      77 upperarm02.R    ←76      78 lowerarm01.R    ←77      79 lowerarm02.R    ←78
 80 wrist.R         ←79      81 finger1-1.R     ←80      82 finger1-2.R     ←81      83 finger1-3.R     ←82
 84 metacarpal1.R   ←80      85 finger2-1.R     ←84      86 finger2-2.R     ←85      87 finger2-3.R     ←86
 88 metacarpal2.R   ←80      89 finger3-1.R     ←88      90 finger3-2.R     ←89      91 finger3-3.R     ←90
 92 metacarpal3.R   ←80      93 finger4-1.R     ←92      94 finger4-2.R     ←93      95 finger4-3.R     ←94
 96 metacarpal4.R   ←80      97 finger5-1.R     ←96      98 finger5-2.R     ←97      99 finger5-3.R     ←98
100 neck01          ←47     101 neck02          ←100    102 neck03          ←101    103 head            ←102
104 jaw             ←103    105 special04       ←104    106 oris02          ←105    107 oris01          ←106
108 oris06.L        ←105    109 oris07.L        ←108    110 oris06.R        ←105    111 oris07.R        ←110
112 tongue00        ←104    113 tongue01        ←112    114 tongue02        ←113    115 tongue03        ←114
116 tongue04        ←115    117 tongue07.L      ←115    118 tongue07.R      ←115    119 tongue06.L      ←114
120 tongue06.R      ←114    121 tongue05.L      ←113    122 tongue05.R      ←113    123 levator02.L     ←103
124 levator03.L     ←123    125 levator04.L     ←124    126 levator05.L     ←125    127 levator02.R     ←103
128 levator03.R     ←127    129 levator04.R     ←128    130 levator05.R     ←129    131 special01       ←103
132 oris04.L        ←131    133 oris03.L        ←132    134 oris04.R        ←131    135 oris03.R        ←134
136 oris06          ←131    137 oris05          ←136    138 special03       ←103    139 levator06.L     ←138
140 levator06.R     ←138    141 special06.L     ←103    142 special05.L     ←141    143 eye.L           ←142
144 orbicularis03.L ←142    145 orbicularis04.L ←142    146 special06.R     ←103    147 special05.R     ←146
148 eye.R           ←147    149 orbicularis03.R ←147    150 orbicularis04.R ←147    151 temporalis01.L  ←103
152 oculi02.L       ←151    153 oculi01.L       ←152    154 temporalis01.R  ←103    155 oculi02.R       ←154
156 oculi01.R       ←155    157 temporalis02.L  ←103    158 risorius02.L    ←157    159 risorius03.L    ←158
160 temporalis02.R  ←103    161 risorius02.R    ←160    162 risorius03.R    ←161
```

---

## Appendix C. Waist loop (`anthropometry.BASE_MESH_WAIST_VERTICES`)

The 46 vertices in loop order. Base indices:

```
4121, 10763, 10760, 10757, 10777, 10776, 10779, 10780, 10778, 10781, 10771, 10773, 10772, 10775, 10774, 10814, 10834, 10816, 10817, 10818, 10819, 10820, 10821, 4181, 4180, 4179, 4178, 4177, 4176, 4175, 4196, 4173, 4131, 4132, 4129, 4130, 4128, 4138, 4135, 4137, 4136, 4133, 4134, 4108, 4113, 4118
```

Final indices (via `base_mesh_vertex_indices`):

```
4105, 10731, 10728, 10725, 10745, 10744, 10747, 10748, 10746, 10749, 10739, 10741, 10740, 10743, 10742, 10782, 10802, 10784, 10785, 10786, 10787, 10788, 10789, 4165, 4164, 4163, 4162, 4161, 4160, 4159, 4180, 4157, 4115, 4116, 4113, 4114, 4112, 4122, 4119, 4121, 4120, 4117, 4118, 4092, 4097, 4102
```
