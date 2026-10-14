# Source-piece mechanics in the sewn garment

The ordinary `ftx drape` / simulated-round / browser fold-up path now accepts a
canonical material selection on `pieces[i].fabric`, using the same `material` or
`material_id` contract as `pattern.fabric`. There must be a characterized default
at `pattern.fabric`; an omitted piece fabric inherits it. An explicit piece fabric
must be a complete selection, not a few numeric overrides merged with hidden defaults.

```json
{
  "fabric": {"material": {"id": "shell", "record": {"...": "complete source-bearing record"}}},
  "pieces": [
    {"id": "front", "material": "cutting-table label", "...": "pattern geometry"},
    {"id": "cuff", "fabric": {"material": {"id": "cuff", "record": {"...": "complete source-bearing record"}}}, "...": "pattern geometry"}
  ]
}
```

This shows the placement of fields, **not a runnable specimen record**. Use the
canonical source/quantity/estimate-consent fields in `material-catalog.md` and
`spec/06-textile-materials.md`. A piece's plain `material` string remains a cutting
label. It never selects a constitutive law. Non-string `piece.material`, direct
`piece.material_id`, partial piece coefficients and implicit layer assignments
are refused rather than ignored. Selections on unplaced pieces are also validated
and retained; they do not create geometry or contribute mass to this run.

Each retained triangle receives its source piece's mass per area, directional
membrane coefficients and scalar bending rigidity, after the existing grain and
mirror transforms. Welds retain separate rest triangles and material identities;
shared vertex mass is the sum of incident area contributions. The existing
per-triangle material implementation integrates unlike-neighbor hinge rigidity
using the two bending-area shares. This is a shared-curvature discretization,
not empirical seam stiffness, seam bulk or laminate homogenization. Gather-zone
compression is reapplied without replacing the triangle's directional stiffness.

The initial contact scope requires exactly equal canonical thickness and skin
friction across all selected pieces. With self-contact enabled, all pieces must
resolve to the same snapshot hash. Two self-friction measurements do not establish
mutual friction between unlike sheets, even when their numeric values happen to
be equal. Such combinations currently refuse. No coefficient is averaged and
no collision candidate is discarded to make a heterogeneous garment pass.

`sim_report.json.material` records all source snapshots, per-piece inheritance,
an assignment hash, instantiated piece mappings, retained-triangle owners, rest
areas and masses. `fabric.basis` becomes `per_piece` instead of reporting the
default as a global material. Uniform patterns retain their existing report and
numerical paths; explicit equal piece assignments give bit-identical states and
animations while adding the assignment receipt. Solver clones and prescribed
motion retain the assembled field; no material substitution is done during motion.

This does not add layers, different-thickness contact, unlike-interface friction,
new textile constitutive laws, empirical characterization or a real-wearer fit
claim. Source-record hashes establish identity, not experimental validity.
