# Material-following steady garment/body coverage

`thermal-steady-coverage` connects the same frozen material selections and retained triangles used
by draping to geometric body coverage and steady heat flow. It does **not** require specific heat
or invent thermal storage. A material with conductivity and matched thickness, or a qualified
sheet resistance, can therefore participate even when the transient thermal surface is unavailable.
Mechanical draping still requires its own characterized coefficients; a conductivity-only catalog
record does not become mechanically drape-ready.

This is prescribed-boundary, local-normal conduction, not a wearer comfort or safety rating. The
independent-sheet command documented in [steady-conduction.md](steady-conduction.md) remains a
separate mode. This command adds actual body/cloth overlap and explicitly identified physical layers.

## Run the executable example

From the repository root:

```sh
cargo run --locked -p ftx-cli --bin ftx -- materials thermal-steady-coverage \
  tests/fixtures/materials/synthetic-steady-surface-report.json \
  tests/fixtures/materials/steady-coverage-map.json \
  tests/fixtures/materials/steady-coverage-conditions.json --json
```

The primary CLI emits the usual `ftx-cli/1` envelope; the coverage calculation is in `result`.
The companion command emits the calculation directly:

```sh
cargo run --locked -p ftx-cli --bin ftx-materials -- thermal-steady-coverage \
  tests/fixtures/materials/synthetic-steady-surface-report.json \
  tests/fixtures/materials/steady-coverage-map.json \
  tests/fixtures/materials/steady-coverage-conditions.json
```

The fixture is explicitly synthetic. Two half-square-metre body triangles each have one sheet;
with 30 K reservoir difference, 0.3 m2 K/W total film resistance, and respective sheet resistances
of 0.4 and 0.8 m2 K/W, the independent answer is `15/0.7 + 15/1.1`, approximately 35.064935 W.
No source file is modified. Neither this number nor the fixture coefficients characterize a real jacket.

## Inputs and material substitution

The three files have distinct roles:

- **Drape report:** a saved report containing `material` and an available
  `steady_thermal_surface`. The retained material records, per-instance assignment, triangle owners,
  topology and current positions must agree with their hashes. An unavailable transient
  `thermal_surface` does not disable this independent capability.
- **Map:** body positions in metres in the same frame as the retained cloth, outward-wound body
  triangles, every retained instance name mapped to a physical layer name, and projection settings.
  Several sewn pattern pieces can be parts of one layer, such as `shell`; they are not separate
  layers merely because they have different piece IDs. Layer order is determined from geometry.
- **Conditions:** inside/outside reservoir temperatures in degrees Celsius and film resistances in
  m2 K/W of body area. Supply one uniform boundary or exactly one per body triangle. Nonuniform
  boundaries require `body_mesh_sha256` from an evaluation of the exact same body mesh; conditions
  for another pose or triangle order are not silently reused.

`steady-coverage-map.json` and `steady-coverage-conditions.json` are complete minimal examples.
Projection settings declare the outward distance horizon, minimum normal cosine, separation
resolution and directed adjacent-layer interfaces. `cells_per_layer` is retained and validated for
layout compatibility; the steady solver allocates no thermal cells.

Each adjacent pair found in a covered region needs an explicit interface with `inner`, `outer`,
`resistance_m2_k_w` and a nonempty `basis`. Zero resistance must also be declared. An optional
`stagnant_gap` uses the existing projection model: a sourced gas coefficient, explicit conditions,
a positive admitted gap interval, numerical tolerance, and affirmative conduction-only consent.
The gas path uses actual textile **face** separation, including the selected materials' thickness
and its reported bounds, not just cloth mid-surface separation. Missing thickness or invalid gaps
refuse. This is not an inferred convection or radiation coefficient.

For a material swap, change the pattern's existing frozen `fabric.material` or per-piece selection
and rerun the existing drape workflow. Feed its newly generated report to this command with the
same authored physical layer identities. Mechanics changes may change the cloth pose; transport-only
changes can preserve the pose but change the heat flow. The command recomputes polygons and material
transport from the new report. Do not hand-edit a saved report and reuse its old hashes, and do not
change the solver to insert a special case for linen, leather or a branded performance fabric.

## What the calculation means

Projection clips actual retained cloth triangles onto the supplied body faces. It creates disjoint
covered regions, preserves actual cloth-area allocations, and refuses ambiguous folds, intersecting
layer order or reuse of the same spatial cloth portion. Uncovered body fractions remain bare paths.
This local-normal closure assumes an appropriate outward, non-self-intersecting body boundary;
edge consistency checks are not a global body-topology certificate.

For a covered region of area `A_body`, layer `i` uses its allocated current cloth area `A_i`.
For constant resistance, the region's resistance per body area is:

```text
R_region = R_inside_film + sum(R_i * A_body/A_i)
           + sum(R_interface) + R_outside_film
P_region = A_body * (T_inside - T_outside) / R_region
```

Temperature-dependent conductivity uses the existing integrated Fourier law and each material's
own characterized temperature interval, not a mean-temperature approximation or extrapolation.
The solver reports inside/outside face temperatures and original-equation residuals. A temperature
jump across a declared interface stays explicit. Bare fractions exchange through the declared
films, which must have positive combined resistance for a bare path. Heat is signed positive from
the inside reservoir to the outside; reversed conditions can produce negative power.

`steady.regions` contains per-region power, layer areas and face temperatures; its flux and flux
residual are per **covered body area**. `steady.body` contains covered and bare contributions for
each body triangle. `steady.power_w` includes each bare or covered body portion once, not once per
textile layer. `unassigned_cloth_reference_area_m2` identifies cloth excluded from body exchange.
Source reference areas are retained for allocation accounting; they do not become invented mass.

`projection` retains clipped body/cloth polygons, ordering, gap diagnostics and numerical area
residuals. `assembly` retains all selected material observations and topology. Numerical residuals
and gap quadrature bounds are not propagated measurement uncertainty; the result explicitly sets
`uncertainty_propagated`, `comfort_claim` and `empirical_validation_claim` to false.

## Replay and browser API

The result contains a complete `request` with schema `ftx-steady-coverage-request/1`. Save that object
as a new JSON file and replay it directly:

```sh
ftx-materials thermal-steady-coverage frozen-coverage-request.json
ftx materials thermal-steady-coverage frozen-coverage-request.json --json
```

The frozen request retains the material and steady geometry frame, authored map and body-bound
conditions, but excludes unrelated report metadata. Replay recomputes geometry and heat without a
live catalog lookup. Hashes establish content consistency, not authenticity or experimental truth.
The command line accepts at most 64 MiB of combined input and refuses oversized replay output;
projection and solver budgets remain independently enforced. Unknown flags cannot weaken a
material's embedded evidence policy. A refusal does not emit a partial success calculation.

The Rust browser entry point is `ftx_wasm::thermal::steady_coverage(&str)`; the wasm-bindgen export
is `thermal_steady_coverage(input)`. Both consume the same complete request JSON and return result
JSON using the same evaluator as the CLI. The calculation does not upload data or perform network
requests. This export is a usable browser API, not a newly implemented graphical material picker.

## Remaining physical boundaries

The calculation does not model transient storage, compression-dependent loft, liquid/vapor
transport, evaporation, wind-dependent films, radiation, seam thermal bridges or human
thermoregulation. A modeled warm jacket is not evidence of safe exposure temperature, PPE
performance or real-person fit. It does not authorize any machine or robot operation.

Tests include unequal-area series laws, partial bare paths, nonlinear known answers, matched
geometry across storage-capable and conduction-only paths, actual CLI/browser replay, and a
transport-only substitution on a local body patch under an actual sewn-drape solution. The latter
is a software propagation test using synthetic records, not a full-garment empirical validation.
The implementation and next acceptance gates are recorded in plan section 25.9 and beads
`ftx-cv1` through `ftx-cv4`.
