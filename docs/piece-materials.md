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

## Physical contact and explicit interfaces

Each piece now retains its own thickness and skin friction in the actual contact
solver. Cloth/body offsets use the local half-thickness; cloth/cloth offsets sum
both sides' half-thicknesses. Global contact numbers are broad-phase upper
envelopes, not averages applied to every collision. At welded vertices or edges
shared by unlike sheets, contact uses the maximum adjacent thickness and friction:
an explicit conservative junction envelope, not resolved seam bulk or seam geometry.

Two self-friction measurements do not establish mutual friction between unlike
sheets. With self-contact enabled, every pair of distinct assigned snapshot hashes
needs an observation in `fabric.interfaces`. The diagonal comes from each sheet's
own `friction_self`. Every observation uses the canonical quantity/source fields:

```json
{
  "materials": ["<first exact record_sha256>", "<second exact record_sha256>"],
  "friction": {
    "value": 0.3, "unit": "1", "source": "pair-test",
    "evidence": "estimate", "method": "Illustrative synthetic scalar coefficient",
    "bounds": [0.3, 0.3],
    "conditions": {"orientation": "symmetric scalar law, both sides"}
  },
  "sources": {"pair-test": {
    "reference": "test:synthetic-interface", "accessed_on": "2026-10-13",
    "note": "Not a real fabric measurement"
  }},
  "allow_estimates": true
}
```

This is one list element; the example value is not empirical material data. Use the
hashes in the resolved material receipt, not display IDs. Changing either material
record invalidates its interface lock. Reversed pair order is canonicalized;
duplicates, unknown/stale hashes, partial coverage, wrong units, invalid bounds,
unknown fields and unsourced observations refuse. Estimate permission is specific
to the interface: sheet-level permission does not grant it. Supplied observations
are validated and retained even with self-contact off; no pair is invented for a
later self-contact run. At most 64 distinct snapshots and 4096 source pieces are
accepted. Off-diagonal friction is symmetric and scalar, not directional friction.

Unlike-sheet scenes must pass the full physical body-offset audit at construction.
They do not begin with a reduced-thickness ramp. A too-close start refuses; change
its placement instead of claiming that a relaxed thickness represents the fabric.
Uniform one-snapshot patterns retain the existing start/ramp behavior exactly.

`sim_report.json.material` records all source snapshots, per-piece inheritance,
an assignment hash, instantiated piece mappings, retained-triangle owners, rest
areas and masses, hash-ordered contact IDs, the full interface matrix and its
observations. `fabric.basis` becomes `per_piece` instead of reporting the
default as a global material. Uniform patterns retain their existing report and
numerical paths; explicit equal piece assignments give bit-identical states and
animations while adding the assignment receipt. Solver clones and prescribed
motion retain the assembled field; no material substitution is done during motion.

Contact reports label their global thickness/friction numbers as upper envelopes.
The elastic-ring grip diagnostic uses the casing's skin friction, not the maximum
elsewhere in the garment; it remains an ideal ring estimate, not an integration of
the heterogeneous contact tractions. Material fields and interface tables survive
checked body-motion continuation and its transactional failure path.

This does not generate layers or add new textile constitutive laws, resolved seam
cross-sections, empirical characterization or a real-wearer fit claim. Source-record
hashes establish identity, not experimental validity. The existing contact, CCD,
friction, sparse Newton and frankensim-derived BVH kernels remain the only solver.
