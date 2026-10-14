# Dry heat maps from a draped garment

`ftx-materials thermal-surface` evaluates the **same frozen material observations and retained
triangles** as an actual mechanical drape. Changing `fabric.material` or a piece's physical selection,
then rerunning the ordinary drape, changes the thermal inputs without changing solver code. It does
not fetch a fresh catalogue record after the mechanical simulation.

```sh
cargo run -p ftx-cli --bin ftx-materials -- thermal-surface \
  round/sim_report.json tests/fixtures/materials/dry-surface-conditions.json > heat-map.json
```

The conditions fixture contains **illustrative prescribed reservoirs and films**, not measured human
skin/environment boundaries. Choose conditions supported by the particular material observations.
A thermal-only selection change cannot change mechanical results; a change to density, directional
mechanics or contact coefficients can change the actual geometry and therefore the thermal exchange.

## What is connected

A characterized `DrapeRun` automatically includes `thermal_surface` in its report. Every retained
membrane triangle contributes exactly once, using its welded vertex indices, undeformed area in m²,
current positions in metres, instance owner and source-locked material. Discarded seam-collapse
triangles and expanded viewer vertices are not counted a second time. Uniform and heterogeneous
piece assignments share this path. Columns are ordered by source hash, not catalogue or piece order.

Every used material must have dry thermal resistance (or characterized thickness/conductivity),
areal density and specific heat, with its own evidence consent. Missing data publishes
`thermal_surface.available = false` with a refusal, **without invalidating the mechanical drape**.
Legacy uncertain/default fabrics do not acquire invented thermal properties or new report fields.
The CLI refuses unavailable data; it never writes zero heat flow as a substitute for unknown heat flow.

The frame digest binds positions, retained topology, material-report identity and source-complete
thermal model. Reading a changed frame, normalized coefficient, policy or source receipt refuses.
A digest detects change; it is not authentication of the source or empirical validation.

## Model and conservation

Each triangle is an independent through-thickness finite-volume column. Its material's effective
resistance per **current area** is constant. Its heat capacity follows **reference material mass**:

- Current area: `A = 0.5 * |(x1 - x0) × (x2 - x0)|`.
- Area ratio: `J = A / A_ref`.
- Total cell heat capacity: `A_ref * areal_mass * specific_heat / cells_per_layer`.
- Boundary power: `A * flux_per_current_area`.
- Stored heat change: reference capacity times temperature change.

The existing dry column's backward-Euler equation is reused with `dt_column = J * dt`.
This is algebraically the equation with current-area heat capacity `C_ref / J`; stretching cannot
create material or stored heat. A rigid motion has no effect. Thermal state identity excludes current
positions but includes material, reference areas and topology, so the library can advance on changing
poses without resetting temperatures and refuses reuse after material substitution.

This is an explicit **constant-resistance deformation closure**. It does not infer thickness changes,
compression-dependent loft, mechanical work heating or area-dependent constitutive corrections.
The generic `Surface::new` API also accepts explicitly coextensive series layer stacks with interface
resistances, but the drape adapter does **not** infer stacks from nearby or overlapping triangles.
A shell and lining treated as independent sheets are not their combined insulation.

## Conditions and outputs

Conditions use schema `ftx-thermal-surface-conditions/1`. `boundaries` contains either one uniformly
broadcast boundary or exactly one boundary per column in the model receipt's order. Every boundary
requires `inside_c`, `outside_c`, `inside_resistance_m2_k_w` and `outside_resistance_m2_k_w`.
Temperatures must be finite and at least absolute zero; films are finite and nonnegative.
There is no inferred wind coefficient, skin temperature or unreported unit conversion.

Without `transient`, the command returns the steady heat map. With it, the required fields are
`initial_temperature_c`, positive `step_seconds`, `steps`, and positive `record_every`.
Thermal integration holds the final drape geometry fixed; mechanical animation frame timing is not
silently interpreted as thermal history. The library supports explicitly supplied moving geometry.

The report contains source/frame hashes, conditions, positions, per-triangle topology and instance
ownership, steady per-triangle fluxes and cell temperatures, and (when requested) final transient fields
and a decimated aggregate energy trace. Positive power flows from inside toward outside. Stored heat
is in J; boundary power in W; local flux in W/m². Incoming minus outgoing energy must account for
stored heat, with step and accumulated residuals reported.

Limits are 65,536 triangles, 196,608 vertices, 1,000,000 temperature cells and 10,000,000 CLI cell steps;
requests over the limit refuse instead of truncating. Refine temporal/spatial discretization to assess
accuracy. Stability and energy conservation do not establish agreement with an actual textile.

## Claims and remaining work

This is **independent dry sheet exchange**, not whole-wearer heat loss, thermal comfort, a cold-weather
rating or safety guidance. It does not determine body coverage, overlapping-sheet transport, draped air
gaps, radiation, wind, moisture/evaporation or thermal bridges through unmodelled seam structures.
Synthetic tests establish numerical behavior, source propagation and refusal policy, not real garment
fit or measured textile accuracy. Bead `ftx-tm5` remains open for that coupled/validated transport work.

The tests separately verify backward Euler against a one-cell formula, moving-area mass/energy
conservation, heterogeneous and explicit multilayer columns, rigid motion, stale-state rejection,
actual sewn-drape source/element propagation, unchanged mechanical results for thermal-only edits,
portable report round trips, missing-data isolation, and the CLI heat map/energy trace.
