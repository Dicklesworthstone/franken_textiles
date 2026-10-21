# Source-qualified steady textile conduction

`ftx-materials thermal-steady` connects the shared materials database to an integrated,
nonlinear layer-stack heat-flow solver. It needs sourced thermal resistance, or matched
thickness and conductivity. It does **not** require or invent areal mass or specific heat.
Those remain mandatory for transient heat storage. Mechanical readiness remains separate.

## Run a sourced example

```sh
cargo run --locked -p ftx-cli --bin ftx-materials -- \
  show fsri/hfm/cotton-sheet/dry/220330-r1 --allow-estimates

cargo run --locked -p ftx-cli --bin ftx-materials -- \
  thermal-steady tests/fixtures/materials/fsri-steady-conduction.json > steady-result.json
```

The fixture selects a real, individually matched FSRI instrument run. Its **area, reservoir
and film inputs are an illustrative scenario, not a reproduced measurement or garment**.
The bundled thickness and conductivity readings retain the original run identity and
conditioning limitations. See [the HFM acquisition notes](hfm-textile-acquisition.md) and
[the source-bearing catalog](../crates/ftx-pattern/src/material_catalog/hfm-runs.json).

In this scenario the dry-labelled run yields approximately 953.9194 W/m², 9.53919 W over
0.01 m², and material faces at 50.46081 °C and 39.07839 °C. Changing only the selected ID to
`fsri/hfm/cotton-sheet/wet/220318-r1` yields approximately 888.8546 W/m² and 8.88855 W.
These are **calculated scenario results**, not measured performance. The records are
separate instrument runs, with different reported thicknesses; this is not a controlled
wet/dry comparison and does not establish how wetting a garment changes its insulation.
No moisture content, pressure, ply count, mass or heat capacity is inferred from either label.

Both records have a 15–65 °C conductivity interval. Every material face must stay inside
its own interval. Reservoir temperatures behind a declared film need not be material
measurements, but a film never authorizes extrapolation of the material law. Linear
interpolation between reported means is an estimate: each relevant selection must opt in.
The three NIST records whose conflicting mass or specific-heat observations remain withheld
can likewise use their qualified conductivity without being marked transient-ready.

## Input and replay

The complete `ftx-thermal-steady/1` request is demonstrated by the fixture. Layers are ordered
inside-to-outside. Supply exactly one nonnegative `interface_resistance_m2_k_w` per adjacent
pair, a common positive `area_m2`, two reservoir temperatures, and both nonnegative film
resistances. Zero film means a prescribed material-face temperature; it is not a guessed
convective coefficient. Fiber and yarn observations do not become characterized sheets.
There are no default cells, time steps, latent heats or heat capacities. Unknown fields refuse.

The result includes signed through-flux, power, both faces of every layer, numerical residuals,
validity intervals and a complete, digest-locked `request` that is directly runnable again:

```sh
jq '.request' steady-result.json > steady-replay.json
cargo run --locked -p ftx-cli --bin ftx-materials -- \
  thermal-steady steady-replay.json > steady-replayed-result.json
cmp steady-result.json steady-replayed-result.json
```

Frozen records replay without a live catalog. Changing a locked source/property without
updating its material identity refuses. `request_sha256` identifies the complete frozen
scenario; it is not an authenticity signature, physical certification or machine permission.
A deliberate new scenario may change boundary inputs or unlock/reselect a different record.

`show ID [--allow-estimates]` now reports `steady_conduction` and `transient_thermal`
readiness, domains and explicit refusals alongside the existing mechanical readiness.
A successful steady solve does not remove a missing-property refusal from another consumer.

The shared Rust API is `ftx_drape::thermal::steady::SteadyConductionModel`, with
`request::Request` as the portable boundary. The file-free `ftx_wasm::thermal::steady`
entry point and wasm-bindgen export `thermal_steady` use the exact same request and evaluator.
Native entry-point equivalence and wasm32 compilation are tested; no new browser UI or
browser-runtime numerical-parity claim is made here.

## Equations, acceptance and limits

For each conductivity-based layer, with inside and outside material-face temperatures
`Ta` and `Tb`, the solver enforces `q = integral(k(T), Tb..Ta) / thickness`. Direct resistance
records instead use `q = (Ta-Tb)/R`. Tabulated segments and bounded cubic fits use their
exact antiderivatives, not conductivity sampled at a mean temperature. Contacts and films
satisfy their separately declared `temperature drop = q*R` equations.

A cold-to-hot monotone flux search handles nonlinear layers, reversed flow and separated
local domains connected by explicit contact drops. It never clamps or extrapolates a
material evaluation. At most 256 layers and 128 iterations per bracket/search/inversion
are admitted. A result must satisfy the maximum principle, layer-flux residuals within
`1e-9*abs(q)`, and film/contact temperature residuals within
`256*EPSILON*(1+max(abs(Tinside),abs(Toutside))) + 1e-11*abs(Tinside-Toutside)`.
These are numerical tolerances, not material uncertainty intervals. Unrepresentable power,
overflow, invalid inputs, incompatible domains or nonconvergence return no partial result.

The model assumes equal conducting area, fixed geometry and fixed conditioning. It does not
simulate moisture redistribution, evaporation, compression-dependent loft, inferred draped
air gaps, radiative exchange, thermal bridges or human thermoregulation. Contact and film
resistances are scenario declarations, not measured automatically from the material label.
It provides no warmth, comfort, survival, fire-safety or manufacturing-qualification rating.

Regression coverage includes resistance sums, exact cubic primitives, homogeneous splitting,
multisegment tables, manufactured nonlinear stacks at validity endpoints, disjoint local
domains, reversal, scalar/table mixtures, real-source substitution, frozen replay and refusal
paths. This is numerical and integration validation; independent physical validation and
matched transient characterization remain separate open work.

## Steady fields on a retained drape

`SteadySurface` uses the actual solver triangle positions, owners and frozen material records
retained in `sim_report.json`. It does not require heat capacity or fabricate a transient state.
A characterized drape records `steady_thermal_surface.available` separately from the existing
storage-bearing `thermal_surface.available`. Incomplete conductivity/resistance still refuses.

```bash
ftx-materials thermal-steady-surface sim_report.json conditions.json
ftx materials thermal-steady-surface sim_report.json conditions.json --json
```

Both commands call the same `SurfaceConditions::evaluate` method. The main `ftx` command wraps
its result in the standard `ftx-cli/1` envelope; the companion prints the result directly.
`ftx materials thermal-steady STACK.json --json` also exposes the equal-area stack command above.
All inputs are read-only, with a combined 64 MiB input limit. A global `--allow-estimates` switch
is not accepted here: each frozen material's original evidence consent remains authoritative.

Conditions use `ftx-steady-surface-conditions/1` and a `boundaries` array. Each boundary explicitly
specifies `inside_c`, `outside_c`, `inside_resistance_m2_k_w`, and
`outside_resistance_m2_k_w`. One boundary broadcasts uniformly. Multiple boundaries require one
per model column and its exact `model_sha256`; this prevents a material swap from silently
changing which conditions apply to which material. No wind coefficient or skin temperature is
inferred. Unknown fields (including transient settings), invalid films and out-of-domain material
face temperatures refuse rather than changing the calculation.

The `ftx-steady-surface-result/1` output includes current triangle areas, signed powers in W,
column fluxes in W/m², layer face temperatures in °C, numerical residuals, vertex positions,
triangle owners and a source-complete frozen `request`. To replay, save `request.report` and
`request.conditions` as the two command inputs. Frozen conditions bind the model hash even for a
uniform boundary. For a deliberate material comparison, reuse the original unbound uniform
conditions on each newly draped material; do not edit an old receipt or remove its material hashes.
Unrelated top-level report metadata is not copied to the replay request; geometry remains local
user data and should not be published without permission.

Runnable synthetic example (two declared triangles, **not** a simulated or measured garment):

```bash
cargo run --locked -p ftx-cli --bin ftx-materials -- thermal-steady-surface \
  tests/fixtures/materials/synthetic-steady-surface-report.json \
  tests/fixtures/materials/steady-surface-conditions.json
```

The declared two half-square-metre patches have resistances 0.4 and 0.8 m² K/W; with a 30 K
reservoir difference and films summing to 0.3 m² K/W, their independent total is
`0.5*30/0.7 + 0.5*30/1.1 = 35.064935064935064 W`. This is an analytic interface check, not
measured jacket heat loss. An additional actual synthetic sewn-drape test changes only transport,
verifies identical mechanical positions, changed heat flow and refusal of old bound conditions.

The browser export `thermal_steady_surface(report_json, conditions_json)` has the same data
contract and input budget; `ftx_wasm::thermal::steady_surface` is its native-testable entry point.
Native equality and wasm compilation are not browser-runtime parity. These remain independent
fixed-characterization sheet columns: adjacent triangles from different garments do not imply a
series layer stack or body coverage. The separate coverage/motion path is not silently upgraded
from storage-bearing materials to conduction-only records. No whole-wearer warmth, empirical
fit, uncertainty propagation or hardware authorization is claimed.
