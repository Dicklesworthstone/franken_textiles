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

## Moving coverage and complete potential histories

The shared `ftx-drape-scenario/1` scenario accepts a `vapor_coverage` policy containing
`body_triangle_ids`, `instance_layers`, `settings` and `conditions`. The triangle IDs select the
scenario's actual collision-body domain in pressure-boundary order. Every material alternative
rebuilds its initial coverage from the newly sewn cloth and frozen assignments.

When a scenario requests `vapor_coverage` and prescribed motion, every motion keyframe receives a
fresh layered `vapor_coverage` result on its accepted body and sewn-cloth coordinates. Initial
overlap and old material-area values are never reused. Missing inputs or ambiguous coverage refuse
that independent transport result while complete mechanics and other consumers remain available.

To integrate covered potential across **every accepted mechanical sample**, explicitly add this
policy to the scenario's existing `motion` object:

```json
"vapor_coverage_history": {
  "schema": "ftx-motion-vapor-coverage/1",
  "integration": "right_endpoint",
  "max_geometry_operations": 1000000
}
```

This requires the scenario's existing `vapor_coverage` policy. Its source filters, physical layer
names, interlayer resistances and pressure/film boundaries stay fixed throughout the path.
Nonuniform pressure fields follow their original body triangle IDs in the declared order. Their
initial `body_mesh_sha256` must match before a new pose can rebind that identity; a stale or missing
nonuniform binding refuses instead of resetting the field.

For each accepted interval, `right_endpoint` evaluates the newly accepted geometry and multiplies
covered potential W and kg/s by elapsed seconds. The initial state is retained but contributes no
interval. Alternatively, set `integration` to `composite_trapezoid` to average the previous and
newly accepted potential rates over that interval, including the initial rate on the first interval.
Both rules use the same retained projections and geometry-operation budget. The trapezoid rule is
exact for linearly changing potential rates and gives second-order refinement for smooth rates;
coverage transitions still require adequate motion sampling. Neither rule establishes an adaptive
error bound or resolves unseen geometry. Ret observations and original reference areas stay unchanged;
current conducting layer and boundary areas come from each new projection.

When every contributing sample has complete reported Ret bounds, optional integral ranges use the
same endpoint weights as the nominal integral. For `composite_trapezoid`, this includes the initial
bounds on the first interval. A missing or unrepresentable contributing range leaves the affected
integral range unavailable while preserving nominal potential; source ranges do not bound sampling,
geometry or physical-model error.

The independent `motion.result.vapor_coverage_history` outcome contains a complete result or a
refusal. Its `ftx-vapor-coverage-history-report/1` result retains the initial coverage report and one
full report per accepted sample, with times, sample indices, body/cloth identities, covered/uncovered
areas and original observations. Each row carries `potential_energy_j` and `potential_mass_kg`;
`totals` retains their complete integrals. Duration, integration rule, geometry work, motion-path
identity, complete frozen `request` and exact `replay_request_json` are also retained.

Replay a downloaded request with either native command:

```sh
ftx-materials thermal-vapor-coverage-history history.json
ftx materials thermal-vapor-coverage-history history.json --json
```

The shared Rust API is `ftx_drape::vapor::coverage::history::Request::evaluate`. The portable
`ftx-vapor-coverage-history/1` request contains `policy`, `initial` (an ordinary coverage request),
`samples` (`time_s`, `positions_m`, `body_positions_m`) and optional `path_sha256`. The initial state
has time zero; accepted times increase through at most 60 seconds. Frozen replay reproduces the
complete report with no body-model installation, live catalog or hidden previous projection.

Histories allow at most 512 accepted samples, two million retained body/cloth vertex samples and
64 MiB of input/output. Full nested reports and escaped replay retention are charged before append.
`max_geometry_operations` is in 1–20,000,000 and includes the initial projection; each kernel receives
only the remaining allowance. Each material-study attempt reserves its complete history allowance
against 40,000,000 session operations, including histories that later refuse. Mechanical retention
budgets also account for geometry duplicated in the complete replay reports.

A failed later sample publishes no partial integral and does not contribute zero transfer. Complete
mechanics and independent instantaneous endpoints remain available. Uncovered body transfer is
unknown at each sample: partial coverage can be inspected but cannot rank as a complete covered-domain
comparison. These potential joules and kilograms are neither actual evaporation nor moisture storage
and do not enter the dry-heat solver. Moisture availability, conservative latent feedback and qualified
wet/dry garment experiments remain separate open work.

Independent series/area/time tests check accounting, source substitutions, nonuniform pressure
identity, stale-input refusal, transactional failures and exact replay. An analytic linearly increasing
area demonstrates first-order right-endpoint refinement and exact trapezoidal integration; quadratic
area checks second-order trapezoid refinement. Production tests verify accepted geometry throughout
the history and that failed transport reservations preserve complete mechanics.

## Conditional ranges from source-reported resistance bounds

Every canonical vapor stack also attempts to propagate the original positive `vapor_resistance`
`bounds`. It keeps the nominal result when bounds are missing or their arithmetic cannot be
represented, with `reported_bounds: null` and explicit `bounds_unavailable` reasons. It never infers
endpoints from standard deviations, coefficients of variation or an unbounded nominal observation.

With complete endpoints, fixed layer-area ratios scale both resistance bounds, and fixed film and
interface resistances enter both sums. Lower potential transfer uses the larger resistance endpoint;
upper transfer uses the smaller one. No statistical independence is assumed, including for repeated
occurrences of the same material. The frozen geometry, pressures, films, interfaces, latent-heat
conversion and measured transport qualification remain the calculation's conditions.

The coverage report places the complete regional sums in `coverage.reported_bounds`, with
`covered_potential_heat_flow_w` and `covered_potential_mass_flow_kg_s` endpoint arrays. A missing
consumed region bound makes the aggregate unavailable. Motion samples place interval bounds in
`sample.reported_bounds`, while `totals.reported_bounds` retains the complete integral ranges under
`potential_energy_j` and `potential_mass_kg`. These intervals use the same retained geometry and
selected time-integration rule as the nominal history. They do not bound unsampled motion or numerical
quadrature error. Independent-sheet reports expose corresponding complete potential-flow ranges.

Material comparisons can request `basis: "reported_bounds"` for the existing vapor objectives,
including covered and motion-integrated transfer. An inclusive constraint must contain the whole
range. Overlapping ranges remain unresolved; absent ranges exclude the candidate while leaving its
nominal result inspectable. The existing current-source, complete-body-coverage and motion-admission
checks still apply. The browser shows these returned intervals and reasons without another transport
or comparison implementation. These are conditional ranges from reported endpoints, not confidence
intervals, uncertain geometry, moisture availability, actual cooling or measured garment performance.

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
The `material_scenario` moving-layer regressions additionally exercise actual sewn cloth and body
motion, independently calculated current-area resistance answers, source-locked offline replay,
material substitution, verified boundary rebinding and stale initial geometry-lock refusal,
missing vapor properties, independent dry-heat
availability and aggregate retained-geometry accounting.
