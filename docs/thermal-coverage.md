# Explicit layered garment/body thermal coverage

The `ftx-materials thermal-coverage` command connects the existing source-locked thermal
surface in a saved drape to a layer/coverage map. Maps may be **explicitly declared** or
generated from actual body/cloth geometry by `thermal-map` under the local-normal
projection model described below. Unlike
`thermal-surface`, which exposes each cloth sheet independently to its boundaries,
this command connects lining, shell and other assigned sheets **in series** over each
covered body region and evaluates uncovered body fractions separately.

This is a dry heat-transfer calculation on a prescribed-temperature boundary, not a
validated human skin model or an outdoor comfort rating. Real specimen calibration,
general folded/concave-body correspondence, moisture and compressed loft remain open
gates. An authored map remains an allocation contract rather than a geometric proof.

## Command

```sh
cargo run -p ftx-cli --bin ftx-materials -- \
  thermal-coverage SIM_REPORT.json COVERAGE.json CONDITIONS.json
```

It prints a complete JSON report on success. Invalid maps, missing thermal data, stale
source identities, invalid temperatures and excessive schedules print a structured
refusal to stderr with exit code 2, without a partial success on stdout. Input files
are read locally and never modified. Each input is bounded to 64 MiB.

`SIM_REPORT.json` must be a saved, characterized `ftx drape` report with an available
`thermal_surface`. Both its material receipt and final-position receipt are verified.
It must contain independent single-layer source triangles; an already-stacked source
column is refused to avoid interpreting the same layer twice. No catalog lookup or
material-name substitution occurs during this calculation.

## Coverage input

### Generate the map from a saved drape and exported body

```sh
cargo run -p ftx-cli --bin ftx-materials -- \
  thermal-map SIMDIR/sim_report.json SIMDIR/body.obj PROJECTION.json > coverage.json
cargo run -p ftx-cli --bin ftx-materials -- \
  thermal-coverage SIMDIR/sim_report.json coverage.json CONDITIONS.json
```

Use a new output filename to retain previous maps. No handwritten triangle numbers,
body-area fractions or cloth allocation fractions are needed in this path. The first
command reads the **final solver cloth mesh**, not viewer/exported garment triangles,
and the supplied triangular body OBJ. It partitions overlaps, orders the contributing
layers and emits `coverage.json` directly in the existing consumer's format. The second
command reconstructs the geometry and repeats projection before accepting that map.

`PROJECTION.json` for a synthetic two-piece shell has this shape. Instance names and
all numerical controls below are examples, **not a qualified prescription for a real
garment**:

```json
{
  "schema": "ftx-drape-thermal-projection/1",
  "instance_layers": {"front": "shell", "back": "shell"},
  "settings": {
    "max_distance_m": 0.025,
    "min_normal_cosine": 0.25,
    "separation_tolerance_m": 1e-8,
    "cells_per_layer": 2,
    "interfaces": []
  }
}
```

Name every instance from `SIM_REPORT.thermal_surface.instances` exactly once. Extra,
missing, empty or duplicate identities refuse. Several sewn pieces can belong to the
same physical layer; different materials within one shell need not become different
layers. A lining and shell, however, must have distinct physical-layer identities.
Whenever geometry finds two adjacent layers, supply their directed interface entry:

```json
{
  "inner": "lining",
  "outer": "shell",
  "resistance_m2_k_w": 0.05,
  "basis": "Illustrative assumption only; replace with qualified interface data"
}
```

Place such entries in `settings.interfaces`. The number above is synthetic. A zero
contact resistance also requires an explicit entry and basis. Neither distance nor
fiber identity automatically supplies an air-gap resistance.

The body must use **metres in the same coordinate frame and intended pose as the drape**.
There is no automatic alignment, unit conversion, normal reversal or shrink-wrapping.
The adapter accepts the triangular OBJ written by `ftx`: `v x y z` (optional RGB) and
positive 1-based triangular faces. Non-triangular faces, homogeneous-coordinate vertices
and unsupported geometry refuse rather than being silently fan-triangulated. Visual
OBJ metadata is ignored; no referenced material files are opened. The supplied body
may represent a selected boundary patch, but then uncovered-body and heat totals refer
only to that patch. The saved drape's receipt does **not** authenticate an external
BODY.obj as its original body; its imported byte hash and actual geometry are retained.

### Geometric closure, proof and limitations

For each outward-wound body triangle, the mapper projects cloth triangles along that
face's normal. Convex clipping in dimensionless barycentric coordinates splits the
whole face into disjoint polygons. This is not a centroid-ray vote or sampled coverage
estimate. An inverse affine map retains each polygon on every contributing cloth
triangle, so an inclined layer allocates its actual current area and the same fraction
of its reference mass. Regions with no contributing sheet remain bare.

The mapper sorts intersections by distance and checks ordering across every polygon
vertex. It rejects crossing/touching/indistinguishable layers, repeated overlapping
portions of one physical layer, contributing grazing sheets, and a search horizon
that cuts a contributing sheet. It also intersects allocated polygons on the cloth
side: two body faces cannot reuse the same physical cloth portion merely because the
sum of their allocation fractions is below one. The existing coverage consumer checks
the resulting global layer order and area/mass budgets as well.

The positive `max_distance_m` defines the outward search region. Entire sheets outside
it or wholly behind a body face do not count as coverage. Thus a deliberately short
horizon can produce bare regions; this is a declared modeling scope, not a guarantee
that no more distant garment exists. `min_normal_cosine` bounds contributing inclination
and must be in (0,1]. `separation_tolerance_m` is a positive geometric resolution less
than the horizon; it is not a textile thickness or a contact offset.

The output retains body and cloth barycentric polygons and minimum/maximum **mid-surface
normal distances**. These are not air-gap thicknesses: half-thickness, surface treatment,
compression, convection and radiation have not been resolved. Spatial consistency is
checked under the local-normal model. The mapper does not establish a globally closed,
outward, non-self-intersecting human body; outward/non-self-intersecting boundary geometry
is a caller precondition, with shared-edge consistency checked. Concave areas whose
normal projections reuse cloth and folded layers without unambiguous identities refuse
rather than choosing a nearest sheet. General garment correspondence remains open.

`coverage.json` retains a `projection` receipt containing the request, imported OBJ byte
hash, generated layout, polygons, input identity and numerical diagnostics. Heat
evaluation recomputes all geometric results and compares them to the retained receipt
**and** the consumer layout; it does not trust an authored `verified` flag. Modifying a
layout, polygon, control, body position or material without a matching regenerated result
refuses. Removing the receipt makes the document an ordinary authored map, never a
geometrically verified one. The report's `mapping_is_geometrically_verified: true` is
qualified by `geometric_verification_scope`; it is not empirical validation or a body
authenticity certificate.

Re-running `thermal-map` after a material or pose change regenerates the correspondence
and source/frame identities. It does not transfer temperatures across changed topology;
that requires a separate conservative state-remapping model.

Clipping uses floating point, not an exact-arithmetic certificate. Subpolygons at or
below 1e-12 barycentric area may be below numerical resolution. The result reports
`partition_roundoff_m2`, `unresolved_footprint_area_m2` and body/reference-cloth areas
removed solely to correct fractional floating-point overshoot of at most 2e-10. Larger
allocation errors refuse. The unresolved-footprint sum can double-count overlapping
tiny candidate footprints and is a numerical bound, not extra covered area. Refine
geometry/controls and inspect these diagnostics before treating a result as accurate.

Projection uses a BVH and fixed-order clipping, with limits of five million geometry
operations, 4,096 partitions per body face, 256 polygon vertices, 64 layers per region
and 65,536 covered regions. Existing body/source/cell budgets also apply. A generated
map's actual pretty-printed JSON must fit the downstream 64 MiB input limit; budget
failure emits no partial successful map.

### Author an allocation explicitly

`COVERAGE.json` has this structure (indices, positions, mapping basis, fractions and
resistances below are **illustrative**, not measurements or a mapping of your garment):

```json
{
  "schema": "ftx-drape-thermal-coverage/1",
  "drape_thermal_frame_sha256": "COPY_THE_EXACT_THERMAL_SURFACE_SHA256_FROM_SIM_REPORT",
  "body_positions_m": [[0, 0, 0], [0.01, 0, 0], [0, 0.01, 0]],
  "layout": {
    "schema": "ftx-thermal-coverage-layout/1",
    "mapping_basis": "Replace with the source and rationale for these disjoint physical allocations",
    "body_vertex_count": 3,
    "body_triangles": [[0, 1, 2]],
    "cells_per_layer": 2,
    "regions": [{
      "id": "covered-region",
      "body_triangle": 0,
      "body_fraction": 0.5,
      "layers": [
        {"layer": "lining", "cloth_triangle": 0, "fraction": 0.5},
        {"layer": "shell", "cloth_triangle": 1, "fraction": 0.5}
      ],
      "interface_resistance_m2_k_w": [0.07]
    }]
  }
}
```

Cloth triangle indices refer to `thermal_surface.model.patches` in the saved report,
**not** viewer faces, exported OBJ indices or source pattern piece numbers. The report's
`triangle_instance` and `instances` identify their owners. Supply actual body boundary
triangles in metres and select the intended physical cloth portions explicitly.

A region's layers are ordered from inside to outside. Each portion allocates the same
fraction of a retained triangle's reference and current area. Multiple regions may
split one cloth triangle, and multiple different layers may cover one body region.
Within each region, sheets are taken to be coextensive according to the declared map;
nearby triangles do not establish this relationship automatically.

Body fractions per triangle and cloth allocations per triangle must each sum to at
most one. Repeated physical-layer identities in one region, assigning one cloth
triangle to two layer identities, duplicated/reversed body faces, and cyclic layer
order across regions refuse. Fractions are not silently normalized. These checks
validate the allocation contract, not spatial disjointness of actual 3D overlaps.

Every adjacent layer pair needs one nonnegative interface resistance in m² K/W of
**covered body area**. Zero explicitly means no additional contact resistance. Positive
values must be characterized or declared as assumptions in the mapping basis; the
software does not infer a value from a gap, finish, fiber or brand.

Unallocated body fractions are bare paths. Unallocated cloth reference area is reported
and excluded, never quietly added as another heat path or duplicated stored mass.

## Boundary and time input

```json
{
  "schema": "ftx-thermal-coverage-conditions/1",
  "boundaries": [{
    "inside_c": 30,
    "outside_c": 0,
    "inside_resistance_m2_k_w": 0.1,
    "outside_resistance_m2_k_w": 0.2
  }],
  "transient": {
    "initial_temperature_c": 10,
    "step_seconds": 0.5,
    "steps": 20,
    "record_every": 5
  }
}
```

These boundary values are illustrative. One boundary broadcasts over the body mesh;
otherwise supply exactly one per `layout.body_triangles` entry, not per material
column. Each bare path needs a positive combined film resistance. Omit `transient` for
steady results only. Unknown fields, including unsupported wind/moisture parameters,
refuse rather than being ignored. Temperature-dependent materials require their
existing interpolation consent and common validity interval; no extrapolation occurs.

A map is bound to the exact source thermal frame, including final positions and
material observations. After changing a material or rerunning a drape, deliberately
review/rebind the map to that new frame. An old hash is not silently rewritten. The
library separately supports pose changes for an unchanged material-following topology.

## Conservation and outputs

For a covered body area `A_b`, current allocated layer area `A_i` and reference allocated
layer area `A_0i`, the existing column solver uses

```
R_effective_i = R_i(T) * A_b / A_i
Delta H_effective_i = Delta H_reference_i(T_old, T_new) * A_0i / A_b
```

Films and interfaces use covered-body area. Multiplying the solved per-body-area heat
by `A_b` gives watts and joules. Thus body coverage is counted **once**, each textile's
storage follows its **allocated reference mass**, and changing either current cloth
area or body area cannot create material. For temperature-dependent heat capacity,
storage integrates the piecewise-linear `Cp(T)` law rather than using endpoint `Cp`.
The original material observations are never modified to encode geometry factors.

The report contains source material/layout snapshots; source-frame and mapping hashes;
actual cloth and body positions; regional and per-body-triangle fields; covered area;
bare and covered heat contributions; unassigned cloth reference area; steady cell
temperatures; and optional final temperatures and a bounded transient energy trace.
A result hash binds conditions and geometry as well as material identity.
Generated maps additionally retain the recomputed projection receipt and its explicit
geometric-verification scope. Manual maps retain `mapping_is_geometrically_verified: false`.

Positive heat flows from inside towards outside. Incoming and outgoing totals both
include bare heat. `net_energy_into_storage_j` accumulates covered net heat **before**
adding cancelling bare contributions, avoiding loss of precision when bare heat is
large. Global and per-step balance residuals remain visible. A small numerical energy
residual is not evidence of agreement with a measured garment.

Limits are 65,536 regions/body faces, 196,608 body vertices, 64 layers per region,
4,096 cells per region and one million total cells. The CLI additionally bounds the
schedule to 100,000 steps and ten million combined cell/geometry steps; an all-bare
scene cannot bypass this budget by having zero textile cells.

## Executed acceptance and remaining work

`cargo test -p ftx-drape --test thermal_coverage` checks analytic series/parallel heat,
partition invariance, split-sheet mass allocation, changing cloth/body areas, integrated
enthalpy, snapshot replay and invalid inputs. `cargo test -p ftx-cli --test
thermal_coverage_workflow` builds an actual synthetic two-piece sewn drape, saves its
report and invokes the production CLI. It checks the independent series/bare answer,
reproducible output, thermal-only material substitution with identical mechanical
positions, nonlinear heat storage and refusal without partial output.

`cargo test -p ftx-drape --test thermal_projection` checks full and partial geometric
coverage, mismatched body/cloth tessellations, inverse-mapped inclined area/enthalpy,
spatial reuse rejection, layer-order/horizon/grazing refusals, repeatability and rigid
transforms. Twelve offset rectangular-layer cases independently check the analytic
series/parallel heat answer. The CLI suite also runs actual sewn drape -> exported
synthetic cylinder body OBJ -> `thermal-map` -> `thermal-coverage`, with no manual
triangle allocations, including material-only changes, variable-Cp reference-mass
conservation, receipt tampering and input-file preservation.

This is a delivered slice of plan §25.6 / bead `ftx-tmb`, not closure of its broader
geometric and empirical requirements. Local-normal overlap partitions are implemented;
general folded/concave-body correspondence, physical air-gap construction, arbitrary
cloth topology changes, in-plane conduction, radiation, moisture, wind,
compressed loft and tissue/metabolic thermoregulation are not implemented here.
Native tests and wasm32 compilation do not establish browser execution parity,
measured warmth, fire safety, PPE certification or machine-process permission.

For prescribed pose sequences, conservative material-temperature remapping and resumable
heat histories, use `ftx-materials thermal-motion`; see [thermal-motion.md](thermal-motion.md).
This path requires geometry-derived maps and an unchanged covered material domain, not
arbitrary appearance/disappearance of cloth or automatic temperature initialization.
