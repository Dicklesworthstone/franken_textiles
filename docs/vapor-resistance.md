# Potential evaporative transfer from measured vapor resistance

`ftx materials thermal-vapor REQUEST.json --json` and the companion
`ftx-materials thermal-vapor REQUEST.json` evaluate an ordered, common-area stack through
`ftx_pattern::material_vapor::Request`. The file-free WASM export is `thermal_vapor(input)`.
Each layer resolves the existing canonical `vapor_resistance` observation in m²·Pa/W. Missing
mechanics, dry conductivity and heat capacity do not prevent this resistance-only calculation.

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
