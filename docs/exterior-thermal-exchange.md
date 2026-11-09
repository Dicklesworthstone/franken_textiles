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

## Source-reported material ranges with qualified exterior exchange

Positive convection without radiation now retains complete source-reported material resistance
ranges in steady columns and independent draped sheets. The coefficient, its source/conditions,
reservoirs, films and geometry remain fixed. The existing conduction authority uses the equivalent
outside resistance 1/h for an independent sheet; this changes the mathematical representation,
not the prescribed boundary or its evidence. No heat capacity is required.

Both endpoint outer-face temperatures must remain inside the coefficient's qualified
`surface_temperature_range_c`. Missing material intervals, endpoint escape or unsupported
temperature-dependent laws leave nominal flux and power available with an explicit unavailable-range
reason. An omitted source interval is never replaced with the nominal number. Layered body coverage
uses the same authority with each actual body/cloth area factor and separately declared bare films.

With opaque-gray radiation, the actual selected outer-face emissivity also needs complete source
bounds, including explicitly reported singleton or zero endpoints. The shared material authority
solves all four constant-resistance/emissivity corners. Each corner solves
`Ts - T_inside + R * (q_conv + q_rad) / area_factor = 0`, a strictly increasing function of `Ts`.
Signed flux is monotone in either source coordinate while the other is fixed, even when convection
and radiation oppose or net flux changes direction across the source intervals. Corner envelopes
therefore include every combination without assuming statistical independence. Pure radiation and
an explicitly zero-emissivity corner are supported; convection-only zero-coefficient behavior is
unchanged.

Every endpoint temperature must remain in the intersection of the convection and optical domains.
The original observation still qualifies the radiant enclosure, specimen, conditioning and physical
face. A mapped coverage face follows the actual outer cloth/body normals; hidden layers do not
contribute additional emissivity observations. Missing optical bounds or an escaping corner withholds
the range while retaining the independently qualified nominal solution.

Radiative column bounds include `exterior.model: "opaque_gray_source_endpoints/1"`, the original
emissivity interval and the enclosing `surface_temperature_c` range. Their
`total_resistance_m2_k_w` is explicitly labelled `inside_film_interfaces_and_materials_only`;
it excludes the nonlinear exterior instead of inventing a radiative film. The bounded root search
retains its numerical enclosure, and unrepresentable arithmetic or insufficient nominal precision
withholds the range. Old film and convection-only receipts retain their original shape and arithmetic.

Material and design studies can use these ranges in existing `independent_surface_power_w` and
`body_coverage_power_w` objectives and guards with `basis: "reported_bounds"`.
Admission preserves the exact exterior source and per-column model identity, recomputes complete
source/area endpoints, and checks the selected optical observation, duplicated conduction receipts
and separate convective/radiative powers. Reversed exchange keeps signed endpoints; equal reservoirs
give zero exchange. These conditional material ranges
do not propagate uncertainty in wind, convection, boundary temperatures or garment geometry, and
do not establish statistical confidence, wearer comfort or measured fabric performance.

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

## Layered garment/body coverage

`thermal-steady-coverage` now accepts `conditions.exterior_exchange` too, through
both CLIs and the existing browser API. It is an array: one uniform environment,
or one per body triangle with the exact `body_mesh_sha256`. The existing map still
recomputes geometric overlap, layer ordering and cloth allocation from the saved
drape. Each covered series path exposes **only its outermost material**, even
where the surrounding body triangle has several differently layered regions.
The public/reverse optical face remains explicitly selected by the environment;
projection does not guess a textile face from a material name.

Exterior convection and radiation are integrated over the allocated current
**outer cloth area**, not the body area or the sum of all lining/fill/shell areas.
For example, when an outer shell has twice the covered body area, its exterior
conductance is twice the per-area value. The same area conversion applies to
its existing integrated Fourier law. Each covered body patch is still counted
once, and hidden inner-layer emissivity does not become an additional exterior
radiative path. Partial material exposure follows the geometric partitions.

Any uncovered body fraction requires `conditions.bare_boundaries`: one explicit
film boundary or one per body triangle. These use the usual film object with
`inside_c`, `outside_c`, `inside_resistance_m2_k_w` and
`outside_resistance_m2_k_w`. The inside temperature must agree with the covered
path on that same body triangle. No skin emissivity, tissue model or bare-skin
heat-transfer coefficient is inferred from fabric properties. Omission refuses
when bare area exists; supplying this option outside exterior mode also refuses.
Nonuniform bare or exterior conditions are body-mesh-bound, independently of
whether the covered inner conditions are uniform.

The output retains `steady` body and layer fields and adds
`exterior_exchange.regions` with solved outer face temperatures and signed fluxes,
covered-cloth convective/radiative powers and a local power-balance residual.
`steady.bare_power_w` remains a distinct prescribed-film contribution, already
included in total `steady.power_w`. Summing the covered exterior contributions
and bare power reconstructs the total; do not add the bare power twice. Frozen
requests retain these boundary sources and policies for both CLI and browser
replay, without a new material lookup. This remains a prescribed-boundary dry
model, not validated whole-wearer thermoregulation or a comfort rating.
