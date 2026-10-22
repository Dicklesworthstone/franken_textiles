# Potential evaporative transfer from measured vapor resistance

`ftx materials thermal-vapor REQUEST.json --json` and the companion
`ftx-materials thermal-vapor REQUEST.json` evaluate an ordered, common-area stack through
`ftx_pattern::material_vapor::Request`. The file-free WASM export is `thermal_vapor(input)`.
Each layer resolves the existing canonical `vapor_resistance` observation in m²·Pa/W. Missing
mechanics, dry conductivity and heat capacity do not prevent this resistance-only calculation.
Fiber and yarn records are constituent observations, so they cannot be promoted to vapor-transport
layers merely by carrying a Ret field. Source-qualified sheet, woven, knit, nonwoven, hide, laminate
and fill/batt records remain supported; the stated specimen/layer conditions stay authoritative.

```sh
cargo run -p ftx-cli --bin ftx-materials -- thermal-vapor \
  tests/fixtures/materials/leather-vapor-stack.json > vapor-report.json
```

This example uses the measured `zagreb/leather-2022/cr-neck` observation, Ret = 11.27 m²·Pa/W.
Its area, boundary pressures, film resistances and latent heat are illustrative caller assumptions,
not a reproduced experiment or a wearer prediction. Changing only the selected material re-resolves
Ret, the pressure profile, potential latent heat and potential water transfer.

## Explicit request and retained evidence

The `ftx-vapor-stack/1` request contains these fields:

| Field | Meaning |
|---|---|
| `layers` | One to 128 ordered `{key, selection}` occurrences, inside to outside; keys are unique even when a material repeats. |
| `area_m2` | Positive common area for the entire stack and both films. |
| `inside_vapor_pressure_pa`, `outside_vapor_pressure_pa` | Prescribed absolute vapor partial pressures; inside ≥ outside ≥ 0. No relative-humidity or saturation conversion is inferred. |
| `inside_film_resistance_m2_pa_w`, `outside_film_resistance_m2_pa_w` | Positive boundary-film resistances, separate from material Ret; no wind or contact model supplies them. |
| `latent_heat_j_kg` | Positive caller-supplied conversion from potential latent heat to potential mass transfer. |
| `required_method`, `required_conditions` | Optional exact method and condition-subset filters that every layer must satisfy. |
| `allow_mixed_protocols` | Defaults false. Differences in complete reported methods or conditions require explicit admission. This does not relax source filters or establish empirical comparability. |

Each ordinary material selection retains independent estimate consent and content-lock validation.
The report freezes every complete selection, observation, original source and material hash, alongside
the supplied boundaries and request hash. `replay_request_json` is the exact Rust JSON for replay
without a catalog lookup or a JavaScript number round trip. Unknown fields, absent/unadmitted Ret,
nonpositive resistance, invalid boundaries and unrepresentable arithmetic refuse.

The leather source reports Ret at **35°C air, 40% relative humidity and 1 m/s air speed**, as means
of three specimens. Those conditions, the specimen group and source SD/CV stay visible; variability
is not reclassified as a confidence interval or propagated through this nominal calculation.
See [the measured-leather guide](measured-leather.md) and
[Kopitar et al., Table 5 and methods](https://journals.sagepub.com/doi/full/10.1177/15280837221077048).

## What is computed

For the explicitly assumed constant-resistance series network:

```text
R_total = R_inside_film + sum(Ret_layer) + R_outside_film
potential_heat_flux = (p_inside - p_outside) / R_total
potential_heat_flow = area * potential_heat_flux
potential_mass_flux = potential_heat_flux / latent_heat
potential_mass_flow = area * potential_mass_flux
pressure_drop_segment = potential_heat_flux * resistance_segment
```

The Ret relation follows the definition in ISO 11092:2014 §2.2
([primary standard preview](https://cdn.standards.iteh.ai/samples/65962/b8d8acf527374b1ebe215b1346ab1de6/ISO-11092-2014.pdf)).
Adding separately measured layers into a constant series network is the declared effective model,
not a measured assembly. Output includes both film drops and every layer's face pressures and drop.
Equal boundary pressures give zero transfer. Reversed transport is outside this request's scope.

The model is isothermal and assumes no condensation. It does not determine water availability,
actual sweating, liquid flow, moisture storage, interface gaps, compression changes or a humidity/
temperature dependence of Ret. It does not couple latent transfer into the existing dry heat history,
infer comfort, or qualify a garment for weather. Source test conditions remain observations rather
than an inferred operating domain. `physical_validation_claim` is always false.

The [browser material workbench](browser-material-workbench.md#potential-evaporative-transfer)
accepts this request independently of a bound pattern. It displays the shared calculation and source
details and downloads the original input, exact report and frozen replay request. Replacing or
refusing an input withdraws the previous result without changing the inspected material selection.

Analytical layer/film balances, real catalog substitutions, source/filter refusals, offline replay and
native/portable consumer equality verify the software path. Measured coupled transport and garment
validation remain open under `ftx-tm5` and `ftx-tmc`.

## Recompute transport in a material study

The existing `materials study` commands also accept an optional `vapor` request. Its boundaries,
area, source filters and mixed-protocol policy use the same fields above. It has no nested `schema`,
and each layer selects an assignment from the final bound pattern:

```json
"layers": [
  {"key": "lining", "target": {"scope": "piece", "id": "lining"}},
  {"key": "shell", "target": {"scope": "default"}}
]
```

A piece target names an original source-piece ID and uses its effective material, including default
inheritance. A default target always selects the pattern default. These targets select material
observations only: they do not infer physical overlap, coverage, area, layer order or interlayer gaps.
The caller explicitly supplies those common-area network assumptions. Repeating a material is allowed
only as a separately named layer occurrence, so the same material can represent two physical plies.

Every alternative applies its ordinary atomic material substitution first, then invokes the canonical
vapor evaluator with the resulting frozen selections. Its independent `vapor` outcome retains the
complete source report or refusal. Missing Ret never becomes zero and does not erase cutting,
sewing, mass or scenario results. Missing mechanical properties do not prevent vapor evaluation.
Each available result's exact `replay_request_json` can be passed directly to `thermal-vapor` offline.

Comparison supports `vapor_potential_heat_flow_w` (W) and `vapor_potential_mass_flow_kg_s` (kg/s),
with optional `required_consumers: ["vapor"]`. These are nominal potential flows under the shared
explicit network; reported material bounds are not propagated output uncertainty. Comparison checks
the complete method, conditions and evidence of each consumed layer occurrence. Unrelated pieces'
vapor properties do not affect that check. Admission of mixed protocols *between alternatives* is
independent of admission of mixed protocols *within a stack*.

The [leather study example](../examples/vapor-study/leather.json) changes the `front` assignment on
the existing synthetic two-panel pattern. It uses two measured catalog observations and retains an
uncharacterized alternative to demonstrate the missing-property outcome. Its area, vapor pressures,
films and latent heat are illustrative assumptions, and its explicit mixed-protocol comparison is
not physical qualification of those different specimens:

```sh
ftx-materials study tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/vapor-study/leather.json
```

Upload the same pattern and study JSON in the browser material workbench to inspect independent
outcomes, retained source/boundary details and exact vapor replay downloads. A new or refused study
withdraws the preceding result through the existing study lifecycle.

## Material-following potential on sewn cloth

`ftx materials thermal-vapor-surface SIM_REPORT.json CONDITIONS.json --json` evaluates the
same canonical Ret observations over the **current retained cloth triangles**. Each characterized
drape now retains a `vapor_surface` geometry/material receipt independently of dry conductivity,
thermal resistance, density or heat capacity readiness. The vapor calculation itself still requires
admitted Ret observations. Missing vapor data never removes a supported mechanical or dry-heat result.

The `ftx-vapor-surface-conditions/1` request supplies `boundaries`, each containing the stack request's
two vapor pressures, two film resistances and latent heat. One entry applies uniformly to all sheets;
otherwise supply exactly one entry per retained material column and its exact `model_sha256` from
the saved frame. The same `required_method`, `required_conditions` and `allow_mixed_protocols` source
policy applies. Every material retains its own estimate policy. Material substitution invalidates
nonuniform assignments tied to the old column ordering; uniform policies can evaluate the new material.

Each distinct column uses the canonical one-square-metre stack calculation. Per-triangle and
per-piece reports integrate its flux over actual current cloth area, counting each triangle once,
and expose separate potential heat (W) and mass (kg/s) totals. The original reference area remains
in the receipt. A source-only Ret edit changes transport and material identity; it does not change
mechanical coefficients. Changes to cloth geometry produce fresh areas and a new frame/request
identity. Exact frozen `replay_request_json` retains only the source material, vapor frame and
conditions needed to replay, without an external catalog or unrelated wearer metadata.

An optional `vapor_surface` field in `ftx-drape-scenario/1` applies this same calculation to every
material alternative after its actual drape, and to every completed motion endpoint using the new
cloth geometry. It reports an independent `available` or `refused` outcome. Scenario metrics are
`independent_vapor_potential_heat_flow_w` and `independent_vapor_potential_mass_flow_kg_s`; these
describe the initial settled-or-flagged computed frame, while motion endpoints retain their own results.

These are **independent sheet potentials**: a lining under a shell still counts as a separately
prescribed sheet, so their sum is not garment evaporation, exposed-area transfer or wearer heat loss.
No body coverage, interlayer vapor paths, bare skin, water supply, humidity-dependent Ret, condensation,
moisture storage or latent heat feedback is inferred. `ftx-vc2` tracks the separate coverage coupling.
Analytic area/pressure balances, source refusals, frozen replay and real sewn-scene material substitution
verify this software path; physical garment acceptance remains open.
