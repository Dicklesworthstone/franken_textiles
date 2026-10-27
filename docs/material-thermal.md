# Material-driven dry thermal transients

`ftx-materials thermal-transient STACK.json` advances a dry multilayer thermal model using the same
source-complete, hash-locked selections as material binding, mechanical drape and purchasing. Changing
a layer's resistance changes heat transfer; changing its areal mass or specific heat changes sensible
heat storage. No fiber or brand name chooses hidden coefficients.

```sh
cargo run -p ftx-cli --bin ftx-materials -- thermal-transient \
  tests/fixtures/materials/synthetic-thermal.json > thermal-history.json
```

The fixture is entirely synthetic. It is a numerical example, not measured insulation or a human
comfort assessment. Its `ftx-thermal-transient/1` JSON is the full input example: ordered `layers` from
inside to outside, one explicit `interface_resistance_m2_k_w` per layer interface, `cells_per_layer`,
area, inner/outer temperatures, separate film resistances, uniform initial temperature, time step,
step count and trace interval. An empty interface array is correct for a one-layer stack. Zero means
declared perfect thermal contact, not an estimated air gap. Zero film resistance prescribes a surface
temperature; it does not supply a wind model.

## Browser and shared Rust API

The generated WebAssembly module exposes `thermal_transient(input_json)` with the same request
and complete report as the native command. `thermal_at(selection_json, temperature_c)` returns
the same `ftx-thermal-point/1` report as `ftx-materials thermal-at`, and accepts either a catalog ID
selection or a complete frozen selection. Both are local, synchronous calls with no file or network
access; callers can place repeated runs in a Web Worker.

```js
import init, { thermal_at, thermal_transient } from "./ftx_wasm.js";
await init();
const selection = {
  id: "nist/7282/cotton-duck-linear-cp",
  allow_estimates: true
};
const point = JSON.parse(thermal_at(JSON.stringify(selection), 50));
const history = JSON.parse(thermal_transient(JSON.stringify({
  schema: "ftx-thermal-transient/1",
  layers: [point.selection],
  interface_resistance_m2_k_w: [],
  cells_per_layer: 4,
  area_m2: 1,
  inside_c: 35,
  outside_c: 20,
  inside_resistance_m2_k_w: 0.1,
  outside_resistance_m2_k_w: 0.1,
  initial_temperature_c: 23,
  step_seconds: 0.5,
  steps: 20,
  record_every: 5
})));
```

This example's area, boundaries and schedule are illustrative. The historical Cotton Duck model
requires estimate consent and admits only the common 20–70 °C characterization interval. Its frozen
selection carries the source record and digest; changing an observation while keeping its digest
refuses. Missing heat capacity, disallowed estimates, source conflicts and extrapolation remain
errors in the browser. See [the NIST source qualifications](nist-thermal-materials.md).

Rust callers use `ftx_drape::thermal::request::point` and `Request::evaluate` with an explicit
catalog; complete frozen selections replay with an empty catalog. The native and browser adapters
delegate to this one authority. Requests retain the existing limits: 64 MiB JSON at the adapters,
4096 cells, 100,000 steps, ten million cell steps and one million retained temperature values.
Invalid stack dimensions refuse before material snapshots are expanded. A failed step returns
no partial history. Optional source-qualified `exterior_exchange` supports the same convection
and radiation behavior in both adapters; see [exterior exchange](exterior-thermal-exchange.md).

## What is solved

Each layer supplies effective resistance R (m² K/W), areal mass (kg/m²) and specific heat (J/(kg K)).
It is subdivided into equal finite volumes: each cell has R/n and areal heat capacity mass*cp/n.
Adjacent cell centres connect through the sum of their half-cell resistances, plus the declared
interface resistance at a layer boundary. The boundary reservoirs connect through a half-cell and
its corresponding film resistance. Backward Euler solves the tridiagonal conservation equations in
linear time and bounded memory. The implementation solves temperature increments, checks positive
pivots, checks the maximum principle, and reports the residual of stored heat minus net boundary heat.
A failed step never mutates the caller's previous state.

The report contains the full frozen material snapshots, per-cell capacities/resistances, sampled
cell temperatures, boundary heat in watts, integrated incoming/outgoing energy and stored-energy
change in joules, numerical energy-balance error and the exact steady solution of the same network.
Area scales heat and storage, not temperature. Trace and integration budgets are bounded. Independently
reduce `step_seconds` and increase `cells_per_layer` until the quantities of interest converge;
unconditional stability is not accuracy.

All layer evidence policies are checked individually. One layer permitting estimates does not
relax another layer's policy. Missing heat capacity is an error, never a polymer-average default.
For resistance-only supplier data use the existing `ftx-materials thermal STACK.json` static command;
it does not need specific heat. A thermal-ready record is not necessarily mechanically characterized.

For measured vapor resistance, [potential evaporative transfer](vapor-resistance.md) provides the
separate `thermal-vapor` consumer. It uses canonical Ret observations and explicit vapor pressures,
films and latent heat; it does not add evaporation to this dry transient model.

## Scope and next acceptance work

This is an effective **dry one-dimensional model** with constant or source-qualified
temperature-dependent properties, not a prediction of garment
comfort, safe outdoor temperature or survival time. It does not yet extract air gaps from the draped
mesh, change loft with pressure, model liquid/vapor transport or evaporation, infer wind-dependent
films, include seam thermal bridges in this transient, or simulate human thermoregulation. Effective
R and cp must be qualified for the specimen and relevant conditions; a small energy residual only
checks the numerical balance, not the empirical model. Geometry and material parameters are held
fixed throughout each run; time-varying boundaries are available through the Rust API.

Plan §25 / `ftx-tm5` remains open for those couplings and independent measured validation. The swatch
identification workflow in `material-calibration.md` similarly implements a measurement-to-solver
software path while `ftx-tm2` remains open for actual construction-specific empirical records.
