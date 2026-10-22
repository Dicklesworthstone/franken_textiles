# Potential vapor transport through actual layered coverage

`ftx_drape::vapor::coverage` recomputes disjoint body/cloth overlap from the same retained geometry
and frozen material assignments as the drape. Each covered region becomes an ordered series network
evaluated by the canonical [vapor-resistance consumer](vapor-resistance.md). A shell and lining over
the same body patch form one series path; they do not become two independently exposed surfaces.

This is an isothermal potential-transfer comparison under explicitly prescribed vapor reservoirs.
It does not establish actual evaporation, sweat supply, condensation, wearer comfort or a measured
garment's performance. Dry thermal properties, heat capacity and inferred skin properties are not
prerequisites. The request has no network access and needs no body-model installation for replay.

## Source geometry and boundary map

The portable request schema is `ftx-vapor-coverage/1`:

- `source` contains retained current `positions_m`, `patches` with reference areas and material-column
  indices, embedded and digest-locked `materials`, original `instances`, `triangle_instance` owners,
  and the originating `material_report_sha256`. Its schema is `ftx-vapor-coverage-source/1`.
- `map` contains body vertices/triangles in the same metre frame, a complete `instance_layers` map,
  and `settings`. Several sewn pieces can share a physical layer name.
- `settings` declares the local-normal search horizon `max_distance_m`, `min_normal_cosine`,
  `separation_tolerance_m`, and directed vapor `interfaces`.
- `conditions` supplies one uniform boundary or one per body triangle, source filters and the
  explicit `allow_mixed_protocols` policy. Nonuniform boundaries require the exact body mesh hash.

`Source::from_scene` extracts this source directly from the production scene and its material report.
`Source::matches_material` checks the report hash, complete frozen selections and per-patch material
assignment. A hash establishes input identity, not authenticity or empirical qualification.

The shared projection kernel checks complete clipped polygons, not just triangle centres. It refuses
crossing or unresolved layer order, overlapping folds of one physical layer, grazing contributing
sheets, a search horizon that cuts a contributing sheet, invalid body winding, spatial reuse of a
cloth portion and oversubscribed body/reference-cloth area. Global outward winding and the physical
validity of the local-normal closure remain caller preconditions. Numerical partition and allocation
corrections are reported explicitly.

## Explicit vapor interfaces and unequal areas

Every encountered adjacent physical-layer pair needs an interface such as:

```json
{"inner":"lining","outer":"shell","resistance_m2_pa_w":0,
 "basis":"Explicit zero-interface modeling assumption for this declared comparison"}
```

Zero is an explicit assumption. Missing interfaces refuse; dry heat resistance is never converted
to vapor resistance. Geometric separation does not supply a diffusion, ventilation or gap coefficient.
Unused configured interfaces cannot name absent physical layers.

Each covered region uses its actual body area for the two boundary films and its declared interfaces.
Every layer uses its own allocated **current cloth area** from the geometric correspondence. Its
nominal source Ret contributes `Ret * body_area / layer_area` to the canonical series resistance.
The source Ret is retained unchanged beside this effective resistance. This area scaling does not
invent a measured strain-dependent Ret law. Reference areas are preserved for allocation accounting;
the calculation neither creates material nor computes moisture or heat storage.

Each boundary explicitly supplies:

```json
{"inside_vapor_pressure_pa":2000,"outside_vapor_pressure_pa":1000,
 "inside_film_resistance_m2_pa_w":1,"outside_film_resistance_m2_pa_w":4,
 "latent_heat_j_kg":2500000}
```

These example numbers are illustrative assumptions. Pressures must satisfy inside ≥ outside ≥ 0 Pa;
films and latent heat must be positive and finite. Latent heat converts potential watts to potential
kg/s. No saturation-pressure or relative-humidity conversion is inferred. Exact method/condition
filters and each selection's estimate policy are enforced. Differing full reported protocols across
consumed layer occurrences require explicit admission.

## Outputs, unknowns and replay

The `ftx-vapor-coverage-report/1` report retains the complete frozen `request`, its identity, exact
`replay_request_json`, source/body identities and geometry-only projection diagnostics. Each
`coverage.regions` entry includes body area, allocated layer areas and the complete canonical vapor
report, including observations, sources, pressure profiles and its own standalone replay.

`covered_potential_heat_flow_w` and `covered_potential_mass_flow_kg_s` sum only the declared covered
regions. Body area is counted once regardless of layer count. `uncovered_area_m2` remains explicit:
transfer over that area is **unknown and excluded**, not zero. Unallocated cloth reference area is
also reported. No total wearer flux is inferred from a covered subtotal.

Substituting material or changing pose recomputes source identity, overlap, layer areas and potential
flows. Missing Ret refuses this independent consumer; it does not invalidate an otherwise supported
mechanical drape or separate dry-heat evaluation. Frozen replay uses embedded selections and the
saved body/cloth geometry, with no live catalog or hidden previous projection.

Input/output are bounded to 64 MiB. The vapor report permits at most 4,096 covered regions and 16,384
layer occurrences, with a conservative source-expansion check before generating repeated reports.
The shared geometry kernel retains its existing bounded work and finite allocation rules.

## Runnable example and software verification

Run the [layered example](../examples/vapor-coverage/layered.json) with:

```sh
ftx-materials thermal-vapor-coverage examples/vapor-coverage/layered.json
ftx materials thermal-vapor-coverage examples/vapor-coverage/layered.json --json
```

It repeats an existing measured leather Ret selection on two synthetic layers over an explicitly
declared triangular boundary. Geometry, films, interface and pressure values are assumptions, not
an acquired layered specimen. The original specimen qualification and source observations remain
visible; repeated material occurrences do not create new measurements.
The example's source-report hash identifies the exact UTF-8 declaration
`declared synthetic two-layer vapor coverage example; not a drape receipt` (without a newline),
not a production drape. Its 0.002 m² boundary has two equal-area layers at Ret 11.27 m²·Pa/W;
the explicit zero interface and film resistances 1 and 4 produce the independent series answer
`1000 * 0.002 / (1 + 11.27 + 0 + 11.27 + 4)` potential watts.

`material_vapor` tests independently check unequal-area resistor and pressure balances. The drape
`vapor_coverage` tests check series layers counted once, actual sloped cloth area, partial body
coverage, unused reference area, material substitution, absent dry properties, missing interfaces,
invalid geometry and exact offline replay. They establish software behavior, not empirical textile
or garment acceptance.
