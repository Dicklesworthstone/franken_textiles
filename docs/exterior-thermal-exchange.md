# Material-following exterior heat exchange

`thermal-steady` and `thermal-steady-surface` now accept an optional
`exterior_exchange` input through both `ftx materials` and `ftx-materials`, and the
same file-free WASM entry points. It replaces the outside film with prescribed
convection plus optional source-qualified ambient radiation. The solved outer
face temperature is generally neither the air temperature nor the radiant
reservoir temperature. Changing the selected finish's emissivity changes this
exchange without changing a solver or replacing the material database.

## Run the synthetic example

```sh
cargo run -p ftx-cli --bin ftx -- materials thermal-steady \
  tests/fixtures/materials/synthetic-exterior-exchange.json --json
cargo run -p ftx-cli --bin ftx-materials -- thermal-steady \
  tests/fixtures/materials/synthetic-exterior-exchange.json
```

All material numbers, the convection coefficient, area and reservoirs in this
example are explicitly **synthetic**. Its half-square-metre sheet has resistance
0.4 m² K/W, an inner film of 0.1 m² K/W, inside temperature 35 °C, air 5 °C,
prescribed convection 3 W/(m² K), and radiant surroundings -10 °C. With emissivity
0.8, the solution is approximately 5.672490 °C at the exterior face and
29.327510 W leaving the inner reservoir. Changing only emissivity to 0.05 gives
approximately 15.768068 °C and 19.231932 W. These are numerical examples, not
measurements of a reflective fabric, a real jacket or a wearer.

The output retains the normal `state` and `power_w`, plus `exterior_exchange`
containing the exposed temperature, separate signed convective and radiative
fluxes, the radiation-enabled flag and an equation-balance residual. A zero
radiative flux with `radiation_enabled: false` means the term was explicitly
omitted, not that emissivity was measured to be zero. Save the emitted `request`
for frozen offline replay; changing its material observations without updating
the explicit selection is rejected.

## Material qualification and boundary assumptions

The emissivity property name and exposed physical face are explicit. The exact
outermost selected material supplies the observation through the canonical
source/units/evidence resolver, not a separate optical catalog or a fiber-name
lookup. Its dimensionless value and reported bounds must lie in [0, 1]. Required
observation conditions are:

- `face`: equal to the request's `exposed_face`;
- `spectral_basis`: `total_hemispherical`, not an 8–14 µm camera-band measurement;
- `specimen` and `conditioning`: nonempty and consistent with qualified transport
  observations where present;
- `temperature_min_c` and `temperature_max_c`: numeric Celsius bounds containing
  both the solved surface and prescribed radiant reservoir temperatures.

The user must explicitly admit the opaque, diffuse, gray, large-isothermal-
enclosure approximation with unit view factor. This is not a claim that the
program measured opacity or that this approximation fits every textile. The
convection coefficient needs its own source, evidence class, conditions and
surface-temperature validity interval. It is **prescribed**, not inferred from
wind, pressure, orientation, air composition or garment names. The caller is
responsible for qualifying its non-temperature conditions. Permission to use an
estimated boundary coefficient never relaxes the material's estimate policy.

`outside_resistance_m2_k_w` must be zero and `outside_c` must equal the declared
`air_c`; an old outside film cannot accidentally be ignored or counted twice.
Omit `radiation` for explicitly convection-only exchange. A radiative request
without qualified emissivity refuses. Missing heat capacity still does not
prevent this steady calculation and is not fabricated to enable transients.

## Retained drape geometry

For `thermal-steady-surface REPORT CONDITIONS`, add an `exterior_exchange` **array**
to the existing `ftx-steady-surface-conditions/1` document. Each entry has the same
shape as the example's exterior object. One entry broadcasts; otherwise provide
one per frozen column and the exact `model_sha256`. Uniform environments can be
reused for a deliberate material substitution; frozen replay binds the resulting
material identity, and nonuniform assignments cannot silently follow a reordered
catalog. Each column selects its own outermost material's emissivity.

The result preserves `steady` geometry, column order and patch powers, and adds
per-column exterior states and integrated convective/radiative powers. Power
uses **current retained cloth area**, not a guessed wearer skin area. A change of
mechanical drape can therefore change the exposed area, while an optical-only
change leaves the mechanical trajectory unchanged. Actual sewn-drape and real
CLI/native-browser regressions exercise these paths. Native entry-point agreement
and WASM compilation are not measured fabric validation or browser-runtime parity.

This independent-sheet mode does not infer which neighboring sheets shield one
another. Use explicit coextensive layers for a series column, or the separate
geometric coverage model for body/layer mapping. Do not count an independent
lining and shell as two fully exposed paths and call that whole-wearer heat loss.

## Equations and scope

For an exposed face at `Ts`, the outer heat flux per current cloth area is

```
q_conv = h (Ts - T_air)
q_rad  = epsilon sigma ((Ts + 273.15)^4 - (T_rad + 273.15)^4)
```

Radiation uses a factored fourth-power difference to retain small signed
changes. The solver integrates each existing material conductivity law across
its thickness, solves the shared boundary balance, and checks the original
Fourier/contact/film residuals and material domains. It supports reversed and
opposing convective/radiative flow without clipping either contribution.
Out-of-domain trial points are only root-search bounds; no extrapolated
coefficient is accepted. Iteration or arithmetic failure returns no partial field.

Primary formula references:
https://doc.comsol.com/6.3/doc/com.comsol.help.heat/heat_ug_ht_features.09.088.html
and the NIST CODATA 2022 Stefan–Boltzmann constant table:
https://physics.nist.gov/cuu/Constants/Table/allascii.txt .
The stored constant is rounded from its exact SI expression.

No wind correlation, view-factor/radiosity scene, interlayer radiation, solar
spectrum, moisture/evaporation, compression-dependent loft, transient storage,
human thermoregulation, uncertainty propagation, empirical garment fit, outdoor
comfort temperature, PPE rating or hardware authorization is supplied by this
extension. Qualified optical specimens and independent held-out boundary
experiments remain required for physical acceptance.
