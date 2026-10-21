# Exterior convection and radiation during dry heat storage

Dry transient heat can use a source-qualified convection coefficient and an
explicit radiant environment. Each exposed column takes emissivity from its
**outermost selected material**. Changing that observation updates the temperature
history, steady limit and separate convective/radiative energies through the same
material resolver and heat solver.

| Entry point | Location and shape of `exterior_exchange` |
| --- | --- |
| `ftx-materials thermal-transient STACK.json` | One object in the `ftx-thermal-transient/1` stack request. |
| `ftx-materials thermal-surface REPORT.json CONDITIONS.json` | An array in `ftx-thermal-surface-conditions/1`: one spatially uniform entry, or one per frozen material column. |
| `ftx-materials scenario-study PATTERN STUDY SCENARIO BODY MODEL` | An array at `motion.transient_surface.exterior_exchange` in the scenario: exactly one spatially uniform entry per mechanical keyframe interval. |

The motion path also runs through `ftx materials scenario-study ... --json` and the
existing model-backed WASM scenario entry point. The transient stack and saved
surface commands shown here use the companion CLI. Omit `exterior_exchange` to
retain the existing film-only calculation and omit the new exchange output fields.

## Run a complete synthetic stack

From the repository root:

```sh
cargo run -p ftx-cli --bin ftx-materials -- thermal-transient \
  tests/fixtures/materials/synthetic-transient-exterior.json > transient-exterior.json
```

The [fixture](../tests/fixtures/materials/synthetic-transient-exterior.json) is
self-contained and needs no network lookup. **Every material number and boundary
coefficient is an explicitly admitted synthetic estimate.** It is an executable
numerical example, not a characterized textile or a complete mechanical material.

| Input | Synthetic value |
| --- | --- |
| Sheet area and areal mass | 0.5 m²; 0.2 kg/m² |
| Specific heat and total heat capacity | 1000 J/(kg K); 100 J/K over the whole sheet |
| Through-thickness resistance | 0.4 m² K/W |
| Initial uniform sheet temperature | 10 °C |
| Inside reservoir and film | 35 °C; 0.1 m² K/W |
| Exterior air and prescribed convection | 5 °C; 3 W/(m² K) |
| Radiant reservoir and emissivity | -10 °C; 0.8 on the `public` face |
| Numerical policy | Four cells; 40 steps of 0.5 s; record every ten steps |

The report contains the initial temperature state and samples at 5, 10, 15 and
20 seconds, plus a separately solved steady limit. Twenty seconds is an authored
duration, not a claim that the sheet has reached equilibrium. `record_every`
controls saved samples; all steps still contribute to the accumulated energies.

To exercise a material-only optical substitution, create a second synthetic input:

```sh
python3 - <<'PY'
import json
from pathlib import Path
request = json.loads(Path("tests/fixtures/materials/synthetic-transient-exterior.json").read_text())
request["layers"][0]["record"]["properties"]["emissivity"]["value"] = 0.05
with Path("synthetic-reflective-transient.json").open("x") as out:
    json.dump(request, out, indent=2)
    out.write("\n")
PY
cargo run -p ftx-cli --bin ftx-materials -- thermal-transient \
  synthetic-reflective-transient.json > reflective-transient.json
```

Compare `trace`, `steady_temperature_c`, `steady_heat_w` and `exterior_exchange`
between the two reports. The selected synthetic observation changes; the solver,
area, resistance, capacity and prescribed environment remain the same. The emitted
`model` retains the material observations and evidence policy used by each run.

## Qualify the exterior boundary

The new exchange **replaces the outside film**. In every corresponding boundary,
`outside_resistance_m2_k_w` must equal zero and `outside_c` must equal
`exterior_exchange.air_c`. A nonzero film or conflicting air temperature refuses
instead of silently counting or ignoring a second exterior boundary.

The fixture shows the complete exterior object. Its convection coefficient must
carry `convection_source`, `convection_evidence`, nonempty
`convection_conditions`, and a `surface_temperature_range_c`. These describe a
prescribed coefficient; wind, pose and garment names do not generate one.
`allow_estimates` here authorizes an estimated boundary coefficient only. Each
material selection has its own independent estimate policy.

With `radiation`, `emissivity_property` names a dimensionless observation on the
exact outermost material. The observation must identify the requested
`exposed_face`, `spectral_basis: "total_hemispherical"`, a nonempty specimen and
conditioning, and numeric Celsius `temperature_min_c`/`temperature_max_c` bounds.
Specimen and conditioning must agree with the qualified transport observations
where present. The emissivity value and its bounds must lie in [0, 1]; its
temperature range must contain both the solved exterior face and radiant
reservoir. A camera-band value or an unrelated fiber's emissivity is insufficient.

`allow_opaque_gray_approximation: true` explicitly admits opaque, diffuse, gray
radiation to a large isothermal enclosure with unit view factor. Omit `radiation`
for prescribed convection alone. Missing or incompatible emissivity refuses a
radiative request; it does not become zero. See
[the steady exterior model](exterior-thermal-exchange.md) for equations and source
references. Transients additionally require qualified areal mass and specific heat;
available steady transport data alone do not supply heat storage.

## Reuse an actual saved drape

Start with an existing `round/sim_report.json` whose
`thermal_surface.available` is true. Its materials must also satisfy the optical
qualification above. The command reads the retained material model and geometry;
it does not replace that model with the synthetic fixture's sheet. The following
creates only illustrative environment and integration conditions:

```sh
python3 - <<'PY'
import json
from pathlib import Path
example = json.loads(Path("tests/fixtures/materials/synthetic-transient-exterior.json").read_text())
film_keys = ("inside_c", "outside_c", "inside_resistance_m2_k_w", "outside_resistance_m2_k_w")
time_keys = ("initial_temperature_c", "step_seconds", "steps", "record_every")
conditions = {
    "schema": "ftx-thermal-surface-conditions/1",
    "boundaries": [{key: example[key] for key in film_keys}],
    "exterior_exchange": [example["exterior_exchange"]],
    "transient": {key: example[key] for key in time_keys},
}
with Path("surface-exterior-conditions.json").open("x") as out:
    json.dump(conditions, out, indent=2)
    out.write("\n")
PY
cargo run -p ftx-cli --bin ftx-materials -- thermal-surface \
  round/sim_report.json surface-exterior-conditions.json > surface-exterior.json
```

One boundary and one exterior entry each broadcast across columns. For spatially
different conditions, provide exactly one entry per
`thermal_surface.model.columns` in its retained order and set the conditions'
`model_sha256` to `round/sim_report.json`'s
`thermal_surface.model.sha256`. **Either** nonuniform array requires this binding,
including a nonuniform film-only request. A supplied stale hash refuses even if
the arrays are uniform. Arrays address material columns, not triangle or piece
order; patches refer to their column through the saved topology.

Uniform requests may omit the hash for deliberate material substitution. Rerun
the mechanical drape with the changed selected material, then apply the same
uniform conditions to its new report. Do not edit a frozen report or reuse a
temperature state whose material identity has changed. The saved-surface command
initializes the requested transient temperature and holds that drape's final
geometry fixed. Omit `transient` to calculate only its steady field.

## Carry the environment through an authored motion

Begin with a working scenario containing `motion.keyframes`, along with its
pattern, material study, body evidence and converted body model. The
[scenario guide](material-scenario-studies.md) describes those inputs. The thermal
fixture alone supplies neither mechanical characterization nor a body/motion model.

This snippet adds an illustrative uniform environment to **each** existing
keyframe interval while retaining the authored trajectory:

```sh
python3 - <<'PY'
import copy
import json
from pathlib import Path
example = json.loads(Path("tests/fixtures/materials/synthetic-transient-exterior.json").read_text())
scenario = json.loads(Path("scenario.json").read_text())
motion = scenario["motion"]
intervals = len(motion["keyframes"])
assert 1 <= intervals <= 16
film_keys = ("inside_c", "outside_c", "inside_resistance_m2_k_w", "outside_resistance_m2_k_w")
film = {key: example[key] for key in film_keys}
motion["transient_surface"] = {
    "schema": "ftx-motion-surface-heat/1",
    "initial_temperature_c": example["initial_temperature_c"],
    "cells_per_layer": 4,
    "max_step_s": 0.01,
    "max_cell_steps": 1000000,
    "boundaries": [copy.deepcopy(film) for _ in range(intervals)],
    "exterior_exchange": [copy.deepcopy(example["exterior_exchange"]) for _ in range(intervals)],
}
with Path("scenario-exterior.json").open("x") as out:
    json.dump(scenario, out, indent=2)
    out.write("\n")
PY
cargo run -p ftx-cli --bin ftx-materials -- scenario-study \
  pattern.json study.json scenario-exterior.json body-input.json model.ftxbody \
  > scenario-exterior-result.json
```

Both arrays must contain exactly as many entries as `motion.keyframes`. There is
**no single-entry temporal broadcast** for a multi-interval motion. Entry zero
applies from motion time zero to the first keyframe; later entries apply from the
preceding keyframe to that keyframe. An interval's environment is spatially uniform
across columns, each with its own material emissivity. To change air temperature
in one interval, update both that exterior entry and its matching film's
`outside_c`. The environment is piecewise constant; no interpolation is inferred.

The initial temperature applies after the initial sewing/drape at motion time
zero. The same temperature state then advances through every accepted mechanical
sample without resetting at keyframes. `max_step_s` subdivides thermal integration
at each sample's right-endpoint geometry. It does not create additional cloth or
body poses. The example's thermal resolution and work budget are numerical policy;
choose a sufficient budget for the actual mesh, trajectory and material alternatives.

The complete heat outcome is at
`simulations[i].evaluation.result.motion.result.transient_surface`. Check its
`status` before reading `result`. A thermal failure publishes a refusal instead
of a partial heat history, while retaining successfully completed mechanics and
independent consumers. The main `ftx ... --json` command adds its normal outer
`result` envelope; the companion output above has none.

## Read the heat and energy results

`ExteriorFlux` reports `surface_c`, `convective_flux_w_m2`,
`radiative_flux_w_m2`, `radiation_enabled`, and `balance_residual_w_m2`.
`surface_c` is the solved outer face, distinct from the outermost cell temperature,
air temperature and radiant reservoir. Positive convection or radiation leaves
the cloth; negative values represent incoming heat. Neither contribution is clipped
when their directions oppose each other.

| Report | Source inputs and fields |
| --- | --- |
| Stack | Material sources in `model`; boundary source and policy in `exterior_exchange.conditions`; steady flux in `exterior_exchange.steady`; sampled flux in `trace[i].exterior_exchange.field`. |
| Saved surface | Original boundary sources and policies in `conditions`; frozen identity in `model_sha256`; patch fluxes in `steady.exterior_exchange.patches` and, for a transient, `final.exterior_exchange.patches`. |
| Motion heat `result` | Original interval environments in `request.exterior_exchange`; material sources in `model`; final patch fluxes in `final.exterior_exchange.patches`. Trace rows retain `sample_index`, `interval_index` and position identities. |

Surface patch arrays follow the retained patch order and include local exterior
face states. Their aggregate exchange objects contain `convective_heat_w`,
`radiative_heat_w` and `balance_residual_w`. Stack samples provide the same two
powers alongside their single local flux field. The initial stack/surface trace
row has no invented exchange flux.

Accumulated `convective_energy_j` and `radiative_energy_j` are under
`exterior_exchange` for a stack or motion heat result, and under
`transient.exterior_exchange` for a saved-surface transient. Recorded transient
trace rows also carry the running pair under their own `exterior_exchange`.
Their sum reconstructs the signed outgoing boundary energy within the reported
exterior balance residual. Incoming minus outgoing energy accounts for physical
stored energy within the time solver's reported conservation residual. Instantaneous
watts, local W/m² and accumulated joules are distinct; the steady limit is never
multiplied by elapsed time to invent a transient energy.

## Geometry and current limits

Exchange power uses each patch's **current cloth area**. Stored heat follows its
**undeformed reference material mass**, so stretching does not create heat capacity.
An optical-only material edit can change heat without changing the mechanical
trajectory; edits to mechanical properties can also change current area and thus
exchange power. This remains one-way motion-to-heat coupling. The existing dry
resistance law supplies transport, with no inferred compression-dependent loft or
mechanical-work heating. See [surface conservation](thermal-surface.md) for the
area/reference-mass equations.

Explicit coextensive series layers are supported in a stack or library surface
column; only the outermost layer supplies exterior emissivity. Independent draped
sheets do not infer overlap, shielding or a shell/lining series path. Summing those
sheets is not a whole-wearer heat-loss calculation.

This transient exterior option does **not yet** extend to geometric body/layer
coverage (`thermal-coverage`, `thermal-motion`, or `motion.transient_coverage`).
The separate **steady** coverage exterior calculation remains available. Moisture,
evaporation, interlayer/finite-view-factor radiation, solar spectra, inferred wind
correlations, thermoregulation and uncertainty propagation remain outside this
model. Refine time steps and through-thickness cells to assess numerical error;
source preservation and conservation checks do not establish measured textile
accuracy or a garment comfort rating.
