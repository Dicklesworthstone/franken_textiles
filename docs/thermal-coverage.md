# Explicit layered garment/body thermal coverage

The `ftx-materials thermal-coverage` command connects the existing source-locked thermal
surface in a saved drape to an **explicitly declared** layer/coverage map. Unlike
`thermal-surface`, which exposes each cloth sheet independently to its boundaries,
this command connects lining, shell and other assigned sheets **in series** over each
covered body region and evaluates uncovered body fractions separately.

This is a dry heat-transfer calculation on a prescribed-temperature boundary. The map
is an authored disjoint-area contract, **not** automatic geometric overlap detection,
a validated human skin model or an outdoor comfort rating. Real specimen calibration,
geometry-derived overlap partitions, moisture and compressed loft remain open gates.

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

This is a delivered slice of plan §25.6 / bead `ftx-tmb`, not closure of its broader
geometric and empirical requirements. Automatic overlap/air-gap construction, topology
changes and temperature remapping, in-plane conduction, radiation, moisture, wind,
compressed loft and tissue/metabolic thermoregulation are not implemented here.
Native tests and wasm32 compilation do not establish browser execution parity,
measured warmth, fire safety, PPE certification or machine-process permission.
