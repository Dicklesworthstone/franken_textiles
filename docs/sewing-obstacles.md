# Sewing around internal openings

Offset stitch plans and explicit flat-fold assemblies consume source cutouts, open slits and drill
holes through one geometry implementation. The existing cutter's mirror/on-fold maps place every
feature in the same expanded source-instance frame used by seam stations. No new material selector,
constitutive law or machine-control protocol is introduced.

A piece can declare, for example:

```json
{
  "internal": [
    {"kind": "cut", "closed": true, "pts": [[5, 30], [9, 30], [9, 40], [5, 40]]},
    {"kind": "cut", "closed": false, "pts": [[13, 20], [13, 30]]}
  ],
  "drills": [{"p": [12, 70], "d": 2}]
}
```

These are synthetic millimetre examples, not machining recommendations. Sewing requires explicit
boolean `closed` on each cut and positive numeric `d` on each drill. A drill's `d` is its full diameter;
clearance is measured from its rim. It never borrows the general exporter's display-default diameter.
An explicitly closed path may repeat its first point once at the end. Self-crossing/degenerate cuts,
conflicting drill diameters at the same exact centre, missing dimensions and obstacles reaching the
exterior refuse. Exterior-connected cuts must instead be represented in the outer outline. Open
slits have no inferred kerf or removed area; closed cutouts and drill disks contain no cloth.

## Offset paths

`SewingPass.stitch_path.minimum_cut_clearance_mm` applies to the exterior **and** all declared
cut/slit edges and drill rims. Both the entire offset polyline and the straight chords between needle
stations must pass. Valid station endpoints cannot conceal a hole between them. A path wholly inside
a closed hole is refused even when it is far from the rim. A harmless opening leaves the original
stations, advances and material snapshots unchanged; the complete source-pattern digest changes.
The existing path receipt reports the smallest measured clearance over these checks.

## Folded assemblies

`SewingPass.assembly.minimum_boundary_clearance_mm` additionally qualifies source preimages after
all declared flat folds. The fold remains a source-space mask plus isometry; openings are subtracted
from the original material, not reconstructed in another coordinate system. A hole can remove an
incidental folded ply from the needle stack. The actual remaining plies then determine thickness,
side-layer indices, material order and the downstream machine/qualification checks. Removing an
intended A or B seam target is an error, not a valid thinner seam.

Layer membership must remain constant along the complete operation. A chord which enters cloth
between stations refuses, including a material tongue inside a concave aperture. An absent region
is skipped only when the whole clipped chord is proven inside one declared hole/disk with the
required rim clearance, or outside the source sheet. Overlapping holes are not unioned to manufacture
an absence proof: a chord covered only by their union can conservatively refuse. Open slits cannot
remove plies. Split genuinely changing-layer operations explicitly; no automatic route repair,
stitch omission or contour reshaping is attempted.

Both ordinary native operations use these same checks:

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-plan pattern.json sewing-options.json
cargo run -p ftx-cli --bin ftx-materials -- sew-stitches pattern.json sewing-options.json
```

The shared file-free Rust planner is also the browser boundary's authority. Native binary replay is
tested; this change does not claim a live-browser replay. The separate sewing SVG preview still has
its pre-existing internal-obstacle restriction; plan and stitch-program JSON retain the checked
source identities and coordinates. Legacy `plies` jobs without offsets or a flat assembly do not
acquire an implicit geometric-clearance policy.

## Bounds and claims

At most 256 source internal marks and 256 drills, and 8192 expanded obstacle points per instance,
are admitted before expensive checks. All geometry work shares the existing 50-million-operation
budget; no truncated obstacle set is treated as complete. No-obstacle paths retain their arithmetic.
A failed job returns a refusal, not a partial plan.

This is declared centreline and removed-material geometry. It does not include cut kerf, a needle or
presser-foot radius, notch-incision clearance, deformation around an opening, tooling access,
collision-free folding motion or physical seam-quality validation. Hardware authorization remains
false. Tests use synthetic geometry and materials: `sewing_paths`, `sewing_cutouts`, module tests in
`sewing/obstacles.rs`, and the actual-binary `sewing_obstacles_workflow`.
