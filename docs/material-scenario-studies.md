# Fixed-shape material scenarios with optional cloth motion

`ftx-ms2` now has an executable drape/heat/manufacturing composition, built on the existing
source-locked `ftx-material-study/1` request. Each alternative starts from the original pattern,
is bound through the existing material selection authority, and gets a **new production drape**.
An optional prescribed body trajectory then continues that same sewn cloth through movement.
No earlier alternative's cloth state, relaxation/plastic history, final pose or heat map is reused.
This is a software delivery, not closure of the empirical or optimization gates in plan §25.10.

## Native commands

```sh
ftx materials scenario-study pattern.json study.json scenario.json body-input.json model.ftxbody --json
ftx-materials scenario-study pattern.json study.json scenario.json body-input.json model.ftxbody
```

The four JSON inputs share a 64 MiB limit; the converted Anny model has a 512 MiB native read limit.
The commands read inputs without modifying them and print a report. The main CLI uses its usual
`result` envelope; the companion prints that result directly. Use the existing model converter to
obtain `model.ftxbody`. This command does not download model assets or upload body evidence.
The body input may be a legacy measurement document or an explicit `ftx-scenario-body/1` request.
Legacy measurements keep the existing tape-fit behavior: require `wearer.age_months`, fit once in
the standard 35-degree straight-arm reference pose, and reuse that shape for every material.
An explicit request can instead reuse an accepted multi-frame shape **without fitting again**,
and independently choose the standing straight-arm abduction angle. No material enters body fitting.

## Body evidence and independent standing poses

`ftx-scenario-body/1` accepts `measurements` (default `{}`), optional `accepted_body_fit`, and
`arm_abduction_deg` (default 35, finite, inclusive range 0..180). Unknown fields refuse. Omit
`accepted_body_fit` to perform a tape fit; an explicitly null, rejected or inconsistent report
refuses instead of falling back. Changing the requested pose never changes the tape-fitting
reference pose or the accepted shape parameters.

To reuse a fit, put the **complete intact `ftx-body-fit/1` report** in `accepted_body_fit`, not its
filename or the CLI envelope. For example, this creates a new request while retaining the report's
original JSON text; it neither edits the accepted source nor invents new body measurements:

```sh
python3 - <<'PY'
import json
from pathlib import Path
report = Path("body-fit.json").read_text()
json.loads(report)
with Path("body-input.json").open("x") as out:
    out.write('{"schema":"ftx-scenario-body/1","arm_abduction_deg":50.0,'
              '"accepted_body_fit":' + report + '}\n')
PY
```

The existing accepted-shape validator checks the exact model digest, pinned Anny identity, acceptance
status, starts/checks/spread, parameter bounds and reconstructed parameter consistency. The original
image-frame poses are never reused as drape poses. Receipt consistency is not cryptographic proof
of an experiment, and accepted model parameters remain `MODEL_FIT`, never measured body truth.
Optional supplied tapes are compared at the existing accepted-body measurement pose (35-degree
A-pose) without refitting; contradictory age or sex refuses. Missing predicted tape sections are
explicitly unavailable, not zero or guessed values. Source-fit residuals remain in `source_fit`.

Each request rebuilds the posed skin, collision mesh and placement frames. The report preserves
`shape_parameters`, `shape_sha256`, `measurements_sha256`, the original accepted source and its
canonical parsed-JSON digest, and the requested `pose`. A pose change retains the shape identity
but changes the body/scenario identity and reruns cloth and heat. Additional tapes change evidence
identity without changing an accepted shape. The original `body_input` is retained for replay.

Without `scenario.motion`, these are independent **static standing poses**, not a motion experiment.
The optional continuation below starts at that selected pose and keeps its shape fixed. Seated/squat
placement is not silently approximated by an arm-angle change. Invalid source/setup results remain
per-simulation refusals while independent manufacturing results survive. No previous body's drape
or temperature field is substituted.

## Numerical and material study policy

`study.json` is the existing manufacturing study format, including explicit variants or screening,
optional cutting/sewing and marker-derived purchasing. Here manufacturing may be omitted because
draping is explicitly requested. Ordinary `study` retains its manufacturing-consumer requirement.
Screening's scalar frontier is not reinterpreted as a garment-level Pareto frontier.

Minimal `scenario.json` (numerical policy, not measured fabric parameters):

```json
{
  "schema": "ftx-drape-scenario/1",
  "options": {"spacing": 12.0, "sew_steps": 12, "drape_steps": 36},
  "fit_spec": {},
  "max_mesh_points": 32768,
  "max_vertex_frames": 64000000
}
```

Supply the pattern's actual fit contract rather than `{}` for its normal points-of-measure reporting.
Omitted individual `options` use the production `DrapeOptions` defaults. Unknown fields refuse.
The per-run mesh budget counts conservative dense-outline, seam and lattice candidates before
meshing; it can exceed the eventual retained vertices. The aggregate vertex-frame budget is charged
in key order, including failed attempted drapes. Total sew/drape steps are capped at 256, lag rounds
at four and PCG iterations at 128. These are allocation/work limits, not a runtime guarantee.

## One material decision across named operating cases

The same native commands and `material_scenario_study` WASM function also accept
`ftx-drape-scenario-matrix/1` in the scenario input. The browser's existing scenario upload accepts
this file directly. See [the complete two-environment example](../examples/scenario-matrix.json).
Its temperatures and films are illustrative prescribed reservoirs, not measured service conditions;
provide your actual fit contract, qualified material observations and numerical settling policy.

A matrix has `schema`, `max_evaluations` and `cases`. Each case has a unique `key`, a complete existing
`scenario`, and an optional `comparison` override using the ordinary comparison policy. Omitted
overrides use `study.comparison`. Distinct cases may choose different objectives or directions, so
minimizing signed heat in one environment need not imply the same design preference in another.
If any case has no effective policy, its physical results and the other case comparisons remain
available, but no combined frontier is invented.

```sh
ftx materials scenario-study pattern.json study.json examples/scenario-matrix.json body-input.json model.ftxbody --json
ftx-materials scenario-study pattern.json study.json examples/scenario-matrix.json body-input.json model.ftxbody
```

The body shape and initial pose are selected once. Binding, scalar screening, cutting, sewing,
inventory and purchasing run once on the shared frozen alternatives. Every case/alternative starts
a fresh drape and independently evaluates its requested heat, vapor and motion consumers. Existing
motion programs may differ across cases; no previous case's final cloth state, material memory or
temperature field becomes another case's initial condition. Joint outfits are admitted separately
under every case's options and must produce the same exact prepared pattern.

The `ftx-material-scenario-matrix-study/1` result contains shared `manufacturing` and ordered `cases`.
Each case retains its descriptor, request identity, complete `simulations` and ordinary `comparison`.
The optional top-level matrix comparison retains case-keyed policies, readings, source protocols,
exclusions and a conditional all-cases `frontier`:

- A material is eligible only when **every case** admits its objectives, constraints and required
  consumers. A failed required case cannot disappear into an average or become a zero result.
- Dominance requires no worse performance in every declared case/objective coordinate and a strict
  improvement in at least one. A tie in one case can coexist with an improvement elsewhere; opposite
  improvements keep both choices. The existing nominal/source-range relation supplies each decision.
- Protocol compatibility is checked between materials **within each case**. Permission to mix source
  protocols in one case does not change another case. Different authored environments are retained
  as separate coordinates rather than asserted to be the same experiment.
- Reported-range constraints require the whole admitted range. Missing bounds, overlapping ranges
  and unsupported propagated uncertainties retain the existing comparator's behavior.

Matrices admit 1–8 cases and at most 64 case × alternative evaluations. The exact product is reserved
before manufacturing or physical callbacks; failed bindings remain in every case without starting
a solver. Case vertex-frame allocations must sum to at most 64,000,000, and active motion-trial
allocations to at most 32,768. Heat/vapor operation budgets and retained motion geometry are shared
across cases, including refused attempts; cases cannot multiply the ordinary session allowances.
Declared order determines execution and reservation order, while alternatives retain stable key order.

The browser shows manufacturing once, then the combined frontier and each case's actual outcomes.
Original input/report downloads retain their exact text; case-prefixed frozen replay downloads avoid
collisions. Changing the source, body, study or matrix cancels and invalidates all dependent output.
This is sensitivity to a finite set of explicitly authored cases, without inferred probabilities,
unseen operating conditions, measured garment validation or a whole-wearer comfort prediction.

## Prescribed motion of the sewn cloth

Add optional `motion` to the scenario request. Both native binaries and the model-backed WASM
entry point use the same implementation; no new command or browser-only solver is involved.
The following tiny motion is the **synthetic articulated-model regression policy**, not a measured
movement, recommended garment test duration or an anatomical acceptance case:

```json
{
  "motion": {
    "schema": "ftx-drape-motion/1",
    "keyframes": [
      {"time_s": 0.004, "arm_abduction_deg": 35.1},
      {"time_s": 0.008, "arm_abduction_deg": 35.2}
    ],
    "max_step_s": 0.001,
    "max_trials": 16,
    "max_pairs": 100000,
    "max_sample_vertices": 10000
  }
}
```

Times are cumulative physical seconds **after the initial drape**, strictly increasing; angles
are the same standing straight-arm convention as the body input. An unchanged angle at a later
time is an explicit hold. Endpoint skins are generated once from the exact selected shape and
source-model digest. No tape fit or accepted-shape adjustment occurs between endpoints or materials.
Collision topology and vertex order must remain unchanged.

Between endpoints, the existing transactional motion kernel follows **linear body-vertex chords**,
using adaptive implicit cloth substeps, relative body/cloth friction and continuous collision checks.
These chords are not interpolated anatomical joint rotations. Large increments can distort a body
or refuse; author suitably short source-model pose increments. Body self-intersections are not
certified. The solver continues the initial cloth's positions, velocities and constitutive history;
it does not place, weld, settle or zero velocities again at each keyframe. Each material alternative
still begins with a separate fresh initial drape. An unfinished physical-thickness ramp refuses motion.

The optional per-simulation `motion` outcome is **all-or-refused**. Even a late failure publishes
no partial trajectory; the complete initial static drape/heat and independent manufacturing remain
unchanged. A successful `ftx-drape-motion-result/1` retains every accepted adaptive sample, physical
time, cloth/body positions, topology, solve diagnostics, endpoint identities, final velocities and
material-history reports. A separate `initial_sample` retains the actual body and cloth at time zero;
`samples` remains the accepted continuation sequence, so `keyframes[].sample_index` is unchanged.
Initial material histories and final cloth/body identities permit direct continuation checks.
Its initial-position identity links back to the retained static result. A late mechanical failure
restores the complete solver, including velocities, contact caches and material memories; admitted
work stays charged even when the path refuses.
Peak absolute panel-edge strain includes the initial state and all accepted samples, using each
original flat panel chart across welded seams. It is not principal strain, a rupture criterion or fit.
Use the accepted sample sequence rather than inventing a long interpolation over omitted substeps.

Each endpoint independently recompiles source-locked steady sheet transport and body coverage from
the **new cloth and body geometry**. These are equilibrium snapshots, not a transient-temperature
history, energy accumulated during motion or a comfort prediction. A thermal refusal does not erase
the completed mechanical trajectory. The scenario's original `drape`, `surface`, `coverage`, positions
and existing static comparison metrics continue to describe the **initial static drape**.
Explicit motion/transient objectives
and the `motion`, `transient_surface` and `transient_coverage` required consumers below qualify a frontier
by the complete requested histories. A study using only static objectives still needs an explicit
motion consumer if movement must succeed. Neither initial geometric settledness nor successful
implicit substeps establish viscoelastic equilibrium or empirical garment accuracy.

Admission permits 1..16 endpoints through 60 seconds, 0..180-degree angles, maximum substeps in
1e-9..1 seconds, 1..512 attempted substeps for the **entire** trajectory, up to one million pairs
per continuous-collision query and up to two million retained sample units. Body/cloth vertex
positions and each optional captured plastic-work or Maxwell-work scalar consume one unit, including the initial
sample. Endpoint body meshes have an additional two-million-
vertex allocation cap. Omitted `max_sample_vertices` defaults to 500,000. The scenario's optional
`max_motion_trials` caps reserved trials across alternatives (default 4,096; range 1..32,768).
Static draping and motion have separate aggregate working-frame counters, each bounded by
`max_vertex_frames`: an earlier trajectory cannot spend a later alternative's static allowance.
Motion admission also reserves at most 2,000,000 retained vertex equivalents across the session,
conservatively accounting for samples, captured work, endpoint heat receipt geometry and final velocities. The
reservation uses conservative mesh counts and the requested trial/sample limits; a smaller actual
output does not retroactively release it. Each requested work channel is reserved once, even when both
thermal domains consume it; exact retained-sample admission uses the sewn triangle count. Admitted
attempts remain charged after a refusal.
The existing transient thermal work limits remain independent. Limits produce an independent
motion refusal instead of truncating a trajectory or erasing available static/manufacturing results.

Library callers with explicit meshes can construct `study::motion::Path` and attach it once with
`Session::set_motion_path` before evaluation. `FittedBody::motion_path` generates source-model
endpoints for the native and model-backed browser adapters. A fixed-body `on_body` call without an
attached source path refuses requested motion rather than pretending its body moved.

### Transient temperatures during the actual motion

Optional `motion.transient_surface` and `motion.transient_coverage` now execute the existing
transient solvers on **every accepted mechanical sample**, instead of restarting at each endpoint
or treating steady power as a temperature history. Both start at an explicitly authored uniform
cloth temperature at motion time zero, after the initial drape. They do not infer that temperature
from the static result or integrate heat during initial sewing/settling. Source heat capacity and
areal mass are required; conductivity-only data remain usable by the independent steady consumers.

For the two-keyframe example above, a synthetic sheet policy is:

```json
{
  "transient_surface": {
    "schema": "ftx-motion-surface-heat/1",
    "initial_temperature_c": 15.0,
    "cells_per_layer": 2,
    "max_step_s": 0.0005,
    "max_cell_steps": 100000,
    "boundaries": [
      {"inside_c": 40.0, "outside_c": 5.0,
       "inside_resistance_m2_k_w": 0.1, "outside_resistance_m2_k_w": 0.2},
      {"inside_c": 5.0, "outside_c": 40.0,
       "inside_resistance_m2_k_w": 0.1, "outside_resistance_m2_k_w": 0.2}
    ]
  }
}
```

Put these fields **inside** `motion`, alongside its keyframes. The values illustrate the synthetic
regression, not sourced environmental films, an initial wearer temperature or a recommended test.
Each boundary applies throughout its keyframe interval, spatially uniformly, with no interpolation.
The temperature solver uses backward Euler at the accepted sample's right-endpoint geometry;
`max_step_s` subdivides the thermal step but does not invent finer cloth/body motion. Storage follows
undeformed reference mass even when exchange area changes. Independent-sheet heat counts every
retained sheet separately; it is not body heat loss through a layered garment.

`transient_coverage` instead uses schema `ftx-motion-coverage-heat/1` and adds the exact
`body_triangle_ids`, `instance_layers` and projection `settings` described above. Set thermal
resolution in `settings.cells_per_layer`; do not add the sheet policy's top-level `cells_per_layer`.
Supply `initial_temperature_c`, `boundaries`, `max_step_s`, `max_cell_steps`, and
`max_geometry_operations` (at most 20000000). The layer map must name every retained instance
exactly. All body IDs refer to this selected shape's collision mesh, not another pose or an assumed
anatomical part. There is no automatic reuse of the steady coverage policy.

Coverage also requires `exposed_boundaries`, an array with exactly one choice per keyframe interval.
`null` explicitly makes cloth outside the selected body coverage adiabatic; a film object uses the
same fields as above and applies to those exposed faces in original source-material orientation.
If that interval has no exposed cloth, the field has no exchange area. No ambient temperature is
inferred. At each new accepted pose, actual clipped overlap is rebuilt, and the existing first-order
enthalpy remapper transfers covered/exposed cloth storage without elapsed time. Material entering
or leaving body coverage therefore keeps its heat rather than resetting to the initial temperature.
Bare body fractions remain single-counted parallel paths with no textile heat storage.

Each requested history is independently `available` or `refused` beneath `motion.result`. A missing
property, invalid thermal domain or even late work-budget failure retains the complete mechanical
trajectory, the other thermal consumer, manufacturing and the initial static results. No partial
heat history is published. If mechanics fails, neither its partial motion nor partial heat is a result.
Malformed JSON/unknown fields still refuse parsing; independent consumer refusal applies to validly
decoded policies and numerical inputs. Material substitutions always start separate temperature states.

Reports retain sample/interval indices, physical time, temperature extrema, signed boundary energy
and physical stored change in joules, plus the final temperature fields and source identities. A
coverage history additionally retains remap accounting, bare exchange, explicit body fields and a
source-complete final `ftx-thermal-motion-state/1` checkpoint, restorable by the existing Frame API.
The checkpoint has the final state; physical time belongs to its parent history. Remap residual is
reported **separately**, never disguised as environmental heat. Net storage heat is accumulated
before cancellation of bare-body paths; incoming/outgoing totals include those paths. Initial and
final frame identities tie the thermal geometry to the retained motion. Independent-sheet and
coverage totals are alternative model domains and must not be added together.

Each thermal policy permits 1..64 cells per layer, maximum substeps in 1e-9..1 seconds and up to
10000000 cell-step units (coverage additionally counts body-face steps; opted-in dissipation heating
charges 129 units per cloth cell/substep in either domain). Whole-session reservations
across both thermal consumers and all attempted material alternatives are capped at 20000000
cell steps and 40000000 coverage geometry operations. Reservations persist after a later refusal;
use realistic declared limits rather than maximum values for every candidate. Projection/remap work
is checked after each existing bounded kernel; limit exhaustion refuses rather than truncating.

These are **one-way** motion-to-heat calculations with prescribed films or explicit exterior
exchange. Mechanical heating requires the separate qualified material option below. They do not
include temperature feedback to mechanics, moisture, thermoregulation or empirical comfort prediction.
First-order remapping can mix unresolved gradients. Time/space refinement and held-out measurements
remain necessary; a small energy residual alone proves neither physical accuracy nor a warmth rating.
The comparator's existing metric names still describe the initial static result. The separate
motion metrics below compare signed cumulative energy and final cloth-cell temperatures.

### Qualified irreversible material heating

Add `"plastic_dissipation_heat": "uniform_reference_thickness"` inside the existing
`motion.transient_surface` or `motion.transient_coverage` object to convert accepted plastic membrane
work into material heat. Each history independently opts in; requesting both does not sum their
energies or capture the mechanical work twice.
Each active plastic material must supply the source-qualified `membrane_plastic_heat_fraction`
observation documented in [permanent membrane stretch](material-membrane-plasticity.md#convert-accepted-irreversible-work-to-sheet-heat).
It needs admitted evidence, matching specimen/conditioning, the irreversible membrane work basis
and a temperature domain. Missing conversion evidence refuses this thermal consumer; it supplies
no default fraction and does not disable otherwise valid mechanics. Explicit zero is allowed.

Either domain can also request `"relaxation_dissipation_heat": "uniform_reference_thickness"`
to convert physical Maxwell dashpot work. Each active relaxing material needs its own
`membrane_relaxation_heat_fraction` observation in unit `1`, within [0,1], with the law's matching
`specimen_id`/`conditioning`, `work_basis: "membrane_maxwell_viscous_dissipation"`, and finite
`temperature_min_c`/`temperature_max_c` conditions. The ordinary material authority also checks
the actual equilibrium tensor and every relaxation branch against that specimen. Explicit zero
is supported; absent or mismatched evidence refuses only the requested thermal history.

Either domain can independently request
`"bending_plastic_dissipation_heat": "uniform_reference_thickness"` for accepted irreversible
source-panel curvature work. Each active permanent-curvature material needs its own
`bending_plastic_heat_fraction` observation, with the matching specimen/conditioning, work basis
`irreversible_bending_plastic_dissipation`, fraction and bounds in [0,1], and explicit temperature
domain. The source triangle's reference area supplies the work; overlapping six-node stencils do
not multiply it or deposit it in neighboring material. See [qualified bending heat](bending-heat.md).

The consumer subtracts the accepted cumulative work at motion time zero, after initial drape.
Only subsequent accepted increments become heat; rejected trials, initial forming work,
elastic/hardening storage and backward-Euler numerical spring loss are excluded. Maxwell work uses
the accepted dashpot endpoint quadrature. Work stays with its exact retained material triangle and
is distributed uniformly through reference thickness and over each accepted mechanical interval.
The thermal solver still uses each sample's right-endpoint geometry. Independent sheets use
`stored_change_j`; covered/exposed material uses `physical_stored_change_j`. The balance in either
domain subtracts its separately supplied source energies (an unrequested source contributes zero):

```text
balance_error_j = stored_change_j - net_boundary_energy_j
                  - plastic_dissipation_heat.supplied_heat_j
                  - relaxation_dissipation_heat.supplied_heat_j
                  - bending_plastic_dissipation_heat.supplied_heat_j
```

Both trace entries and the final thermal result expose `plastic_dissipation_heat` with motion-only
dissipation, supplied heat, unconverted work and excluded initial dissipation. The final receipt
also preserves conversion observations and material/law identities. Mechanical `initial_sample`
and `samples` retain `membrane_plastic_dissipation_j` arrays in triangle order; their cumulative
values include the excluded initial work. `retained_dissipation_scalars` reports the extra output.
Each scalar counts as one vertex-equivalent under `max_sample_vertices` and session admission,
once even when both histories consume it.

The Maxwell receipt uses `relaxation_dissipation_heat.viscous_dissipation_j`,
`initial_viscous_dissipation_j`, `supplied_heat_j`, and `unconverted_dissipation_j`; accepted mechanical
arrays are named `membrane_relaxation_dissipation_j`. Plastic and Maxwell work retain independent
source observations and ledgers. When both channels are requested, their heat is summed once before
material-depth deposition and the physical storage balance. This permits different material triangles
to use their respective laws; it does not enable an uncharacterized joint same-triangle viscoplastic law.

Bending receipts use `bending_plastic_dissipation_heat.plastic_dissipation_j` and
`initial_plastic_dissipation_j` with their separate supplied/unconverted heat. Captured arrays are
`bending_plastic_dissipation_j`, including exact zeros for inactive triangles. All selected channels
sum once into each consumer's balance; the two thermal consumers share each captured channel without
combining their separate temperature histories or energy totals.

With heating enabled, all cloth cells cost 129 work units per thermal substep, even for zero
conversion or an elastic triangle. Coverage additionally charges one unit per selected body face.
Size `max_cell_steps` accordingly. Source deposition integrates
sensible enthalpy, including temperature-dependent heat capacity. Its equivalent adiabatic preload
must remain inside the characterized thermal domain; large sources can require smaller thermal
substeps. Initial and accepted cell temperatures also stay within each conversion observation's
temperature range. No domain is silently extended.

For `transient_coverage`, each retained triangle's joules are partitioned by reference material
area into the current covered series layers and exposed cloth. Each layer keeps its own mass,
heat capacity and material identity. Even adiabatic exposed cloth receives its share; bare body
receives none. Remapping transfers the resulting enthalpy when cloth becomes covered or exposed
again without another source deposition. `signed_remap_residual_j` stays separate from physical
storage and source energy. The last field retains covered/exposed source allocation and its
rounding residual. The shared comparator reconciles each accepted source increment, its material
receipt and the domain's energy balance before admitting thermal objectives; a failed coverage
history does not erase an independent sheet result or successful mechanics.

Both domains support the existing qualified exterior exchange. There is no temperature feedback,
inferred layer topology, or bending/friction heating. `ftx-drape`'s `relaxation_heating` tests exercise
the real sewn-garment consumer, independent reference-mass enthalpy across covered/exposed cloth,
mixed-source accounting, coverage-only capture, replay, and source/temperature/budget refusals that
preserve mechanics and independent thermal results. These are synthetic software checks; measured
conversion laws and independent temperature measurements remain necessary for a physical garment claim.

## Independent thermal consumers

Optional `surface` accepts the existing `ftx-steady-surface-conditions/1`: prescribed films or the
existing qualified exterior-exchange policy. It evaluates independent material-following sheet
columns. It does not require heat capacity merely to compute steady conduction.

Optional `coverage` has `body_triangle_ids`, `instance_layers`, `settings` and `conditions`.
Triangle IDs are unique indices into this session's **trimmed collision body**, not triangles from
an unrelated OBJ or an older pose. The adapter assembles the existing steady-coverage request from
that exact geometry and the new drape report. Explicit layer identities and existing projection,
interface, gas-gap and environmental validity rules remain authoritative. Uniform boundary
conditions broadcast; spatial boundary conditions use the selected triangle order. Invalid or stale
coverage conditions refuse independently of surface heat, draping and manufacturing.

`tests/fixtures/materials/synthetic-scenario.json` and `synthetic-scenario-pattern.json` are
executable **synthetic cylinder/band test inputs**, not an Anny wearer example or a characterized
textile. In particular body triangle 0 in that fixture is chosen for the synthetic cylinder and
must not be reused as an assumed chest patch on Anny. The small step count tests propagation; it
does not establish a settled garment or a usable real-person fit.

## Declared computed scenario objectives

The study request's optional `comparison` also applies to actual scenario outputs. It uses the same
nominal Pareto engine as manufacturing, not a browser score or a surrogate drape. For example:

```json
{
  "basis": "nominal",
  "objectives": [
    {"metric": "body_coverage_power_w", "direction": "minimize"},
    {"metric": "stock_mass_kg", "direction": "minimize"}
  ],
  "constraints": [{"metric": "max_absolute_edge_strain_pct", "maximum": 8}],
  "required_consumers": ["sewing"],
  "allow_mixed_protocols": false
}
```

This example requires actual cutting and sewing requests plus coverage geometry/conditions. The
8% constraint is an illustrative authored requirement, **not** a measured material limit or safe-fit
threshold. Other scenario metrics are `independent_surface_power_w` and `simulated_cloth_mass_kg`.
Heat retains its sign; minimizing negative power does not mean minimizing its magnitude or improving
warmth. Independent sheet power and declared body-domain power are different quantities. Simulated
cloth mass excludes omitted pieces, trims and hardware; it is not a complete garment mass estimate.
Maximum absolute edge strain uses the extrema and precision in the production report, not principal
strain or a physical fit score. Manufacturing-only objectives remain usable after simulation refusal.
For a complete declared cut inventory, request `assembly_mass` in the study and compare
`assembly_mass_kg` instead. It counts all physical cut pieces (including unsimulated lining) and
explicit sourced retained extras, independently of the body. Unknown components or an incomplete
inventory block that mass objective, not valid drape/heat or other manufacturing results. It is one
pre-finishing assembly, not purchased waste or a weighed finished garment. Explicit `reported_bounds`
comparison can use its complete conditional mass range for whole-range constraints and separated-range
dominance. The same mode supports all eight vapor potential objectives from source-reported Ret
endpoints, including common stacks, independent sheets, covered body regions and motion integrals.
It checks canonical source/network ranges and their area/time aggregates; motion follows the declared
right-endpoint or composite-trapezoid rule. Missing endpoints preserve nominal results but exclude
robust ranking. Additional required consumers keep their ordinary gates. See
`docs/material-screening-and-studies.md`.

Every scenario objective requires this bound pattern's identity, the retained body/scenario identities,
`drape.settled == true`, empty drape gating and `simulation_gating_clear == true`. Successful thermal
output on an unfinished frame is still excluded. Thermal consumers enforce their existing source,
temperature and geometry domains. Comparisons cannot substitute an older drape, mix environments,
or treat missing heat/mass as zero. Required consumers and failed constraints exclude a candidate
without erasing independent results. The top-level `comparison` is qualified by the entire scenario
request identity; there is no misleading manufacturing-only frontier nested beneath it.

Relevant reported methods, full conditions and evidence must match by source piece unless explicitly
relaxed. Different nonlinear/relaxation/temperature records and constitutive-model selectors use
conservative exact-record comparison. Protocol override never bypasses freshness, settledness or
missing-value checks. Nominal comparison requires explicit consent. Source ranges do not establish
mechanical or dry-thermal objective intervals. Reported assembly-mass bounds hold authored geometry
and inventory fixed; vapor bounds hold geometry, boundaries and integration policy fixed. Neither
establishes general scenario uncertainty. Geometric settledness is not viscoelastic equilibrium, measured fit, thermal
comfort or proof that the requested run duration was physically sufficient. Positive admission tests
use synthetic callback outcomes and an actual production sewing-only band after 120 sewing steps;
the latter explicitly reports that gravity was not simulated. The short cylinder integration also
demonstrates that clear-gating but drifting cloth is excluded. Neither establishes a gravity-settled
anatomical garment or measured fit.

### Finite body water through complete motion

When `scenario.motion.vapor_coverage_history.wet_film` declares an exact body-bound initial liquid
inventory and constant supply, the `motion_wet_film` consumer makes its conservative water results
available to nominal or conditional reported-bound objectives and constraints. It reuses the actual
moving layered coverage, source Ret, body identities, pressure boundaries and requested quadrature. The nominal readings use
`motion.result.vapor_coverage_history.result.wet_film.totals` and can also participate in individual
cases of a scenario matrix.

| Metric | Unit | Meaning |
|---|---|---|
| `motion_wet_film_evaporated_mass_kg` | kg | Modeled covered-domain transfer, limited by each body triangle's initial and supplied liquid. |
| `motion_wet_film_remaining_liquid_mass_kg` | kg | Liquid still stored on the declared body domain at the end of the complete motion. |
| `motion_wet_film_unmet_demand_mass_kg` | kg | Integrated canonical vapor demand that the finite liquid could not supply. |
| `motion_wet_film_supplied_mass_kg` | kg | Integrated prescribed supply, retained separately from the initial inventory. |
| `motion_wet_film_body_reservoir_latent_energy_j` | J | Modeled evaporation times the declared latent heat for its body triangle. The prescribed body reservoir supplies this energy. |

For example, a deliberately authored decision can maximize modeled evaporation while requiring
some liquid to remain:

```json
{
  "basis": "nominal",
  "required_consumers": ["motion_wet_film"],
  "objectives": [
    {"metric": "motion_wet_film_evaporated_mass_kg", "direction": "maximize"}
  ],
  "constraints": [
    {"metric": "motion_wet_film_remaining_liquid_mass_kg", "minimum": 0.00001}
  ],
  "allow_mixed_protocols": false
}
```

The threshold and direction above are illustrative decision inputs, not physiological
recommendations or textile measurements. A material that transfers more water can also exhaust its
supply earlier; multiple objectives preserve that tradeoff instead of introducing an undisclosed
score. Any of these objectives or constraints automatically requires `motion_wet_film`. Adding it
only to `required_consumers` lets a cost or mass objective require a complete finite-water history
without ranking its evaporation and sets `comparison.motion_required: true`. Missing film evidence
never becomes an assumed dry film or zero evaporation.

Admission requires the ordinary settled, gating-clear initial drape, complete prescribed motion,
and complete declared body coverage initially and at every retained mechanical sample. Unknown
uncovered evaporation cannot be treated as zero. It verifies the bound material assignments,
source protocols, boundary conditions, geometry, sample times and declared right-endpoint or
composite-trapezoid integrals, then checks the exact authored film, per-triangle supply and
finite-inventory limits, step and cumulative mass/demand balances, latent conversion, and
retained-state budget. Each liquid step is replayed through the same canonical finite-water
`State` authority used by production. Different projected regions and layers never receive separate
copies of a triangle's water.

Altering a request and recomputing a receipt hash cannot substitute a different initial inventory,
supply or body owner. The declared source and basis remain part of the retained inputs, and source
protocol mismatches remain explicit. Missing or stale water inputs, incomplete coverage or a refused
history exclude the candidate from policies requiring that consumer while preserving independent
manufacturing, mechanics and available thermal outputs.

Two materials with different potential vapor transfer can tie in realized evaporation when both
exhaust the same small initial film. With more available water, their realized amounts can differ
again. Unmet demand and remaining liquid retain those distinctions when explicitly requested; the
comparator does not choose a hidden preference among them. Higher unmet demand is not automatically
better or worse. Existing potential metrics retain their separate meaning.

These metrics also work in named scenario matrices. Each case can prescribe its own film, supply
and vapor conditions, with all alternatives in that case using identical prescribed water inputs.
Distinct cases retain their own climate, readings and exclusions. The combined frontier still
requires every case and uses the existing no-worse-in-all-cases relation; a water-limited tie in one
case can coexist with a strict improvement in another. Cases do not share liquid state.

With `basis: "reported_bounds"`, all five metrics also support conditional ranges for their final
cumulative results. The comparator replays low- and high-demand histories through the same canonical
finite-water `State`, using complete consumed source Ret endpoints. Geometry, body ownership,
coverage assignments, pressure boundaries, latent heat, initial liquid, prescribed supply and temporal
integration remain fixed. The result accounts for liquid depletion; it does not treat potential
vapor demand as available water or infer uncertainty in the declared water inputs.

Evaporation, unmet demand and latent energy use `[low-demand total, high-demand total]`. Remaining
liquid reverses that order: `[high-demand remaining, low-demand remaining]`. Declared supplied water
has the singleton range `[supplied total, supplied total]` and needs no Ret bounds, but still requires
complete canonical nominal coverage and water-history admission. If any of the other requested water
metrics lacks complete Ret bounds, the alternative is excluded from bounded ranking while its
available nominal readings remain visible. Inclusive constraints must contain the entire range;
strictly separated ranges can establish dominance, while overlapping or touching ranges remain
unresolved under the existing comparator. The same rules apply within each named matrix case.

Only comparator `readings.reported_bounds` carry these final ranges. The producer's cumulative
`wet_film` report and accepted-sample liquid ledgers remain nominal, with no per-interval water bounds.
These are conditional Ret-endpoint consequences, not confidence intervals or general water-state
uncertainty. The model does not cool textile heat storage, evolve body temperature, add wet textile
mass, infer sweating, bare-body transfer, sorption, condensation or whole-wearer loss. Its ledger
remains available for inspection, and exact retained histories replay the same nominal liquid
declaration and canonical balances through the ordinary coverage API. See
[vapor-coverage.md](vapor-coverage.md) for the source and physical-domain contract.

### Vapor potential on the current sewn geometry

With `scenario.vapor_surface` declared, comparisons can use the actual initial drape's independent
sheet vapor totals. These metrics differ from `vapor_potential_*`, which use an explicitly authored
common-area stack in the manufacturing study:

| Metric | Unit | Meaning |
|---|---|---|
| `independent_vapor_potential_heat_flow_w` | W | Potential heat flow integrated over current retained cloth triangles. |
| `independent_vapor_potential_mass_flow_kg_s` | kg/s | Potential mass flow using the explicitly supplied latent heat. |

For example, given common vapor pressure, film and latent-heat conditions:

```json
{
  "basis": "nominal",
  "objectives": [
    {"metric": "independent_vapor_potential_heat_flow_w", "direction": "maximize"}
  ],
  "constraints": [
    {"metric": "independent_vapor_potential_mass_flow_kg_s", "minimum": 0}
  ],
  "required_consumers": ["vapor_surface"],
  "allow_mixed_protocols": false
}
```

The objective direction and zero lower bound are authored requirements, not a comfort recommendation.
The underlying model requires inside vapor pressure at least as large as outside pressure; reverse
transport and condensation remain outside its domain.
Either metric automatically requires the vapor consumer. Adding `vapor_surface` explicitly also lets
a cost or mass comparison require successful draped vapor evaluation. Missing Ret, a refused result,
an unfinished drape or a mismatched material/geometry/condition receipt excludes the alternative;
the comparison cannot borrow an older frame or substitute zero. Dry conductivity and heat capacity
are not requirements for this independent consumer, and their refusals do not erase valid vapor output.

Admission checks the retained source selection, frame, current positions and triangle ownership,
frozen conditions, canonical column results and integrated totals. Only protocols of columns actually
consumed by each stable cloth-instance identity participate in cross-alternative matching. Column
reordering or deduplication does not change protocol meaning; exchanging protocols between different
instances does. Unused columns do not disqualify a pair. Cross-alternative `allow_mixed_protocols`
remains separate from the vapor request's within-column source admission.

These objectives are nominal, initial-frame sheet potentials. Source ranges are not propagated output
intervals, and motion endpoints do not become an unrequested time integral. Overlapping lining and
shell sheets remain separately prescribed surfaces; their sum does not represent covered-body
evaporation, water availability or latent cooling. See [vapor-resistance.md](vapor-resistance.md).

### Motion-qualified objectives

The same comparison policy now accepts the following separate metrics. They read the actual complete
production motion/heat results; they do not replace or reinterpret initial static metrics.

| Metric | Unit | Meaning |
|---|---|---|
| `motion_max_absolute_edge_strain_pct` | % | Peak absolute panel-edge strain across the initial state and every accepted mechanical sample. |
| `transient_surface_inside_energy_j` | J | Signed energy through the inward boundary of independent sheets over the complete motion. |
| `transient_surface_net_boundary_energy_j` | J | Signed inward minus outward boundary energy of independent sheets. |
| `transient_surface_min_sample_temperature_c` | C | Minimum sheet temperature over the initial state and all accepted sample endpoints. |
| `transient_surface_max_sample_temperature_c` | C | Maximum sheet temperature over the initial state and all accepted sample endpoints. |
| `transient_surface_final_min_temperature_c` | C | Minimum independent-sheet cell temperature at the final accepted sample. |
| `transient_surface_final_max_temperature_c` | C | Maximum independent-sheet cell temperature at the final accepted sample. |
| `transient_coverage_inside_energy_j` | J | Signed inward boundary energy of covered, exposed and bare paths in the declared coverage model. |
| `transient_coverage_net_boundary_energy_j` | J | Signed physical boundary energy into cloth storage, excluding remap residual and cancelling bare-body paths. |
| `transient_coverage_min_sample_temperature_c` | C | Minimum covered/exposed cloth temperature over the initial state and all accepted sample endpoints. |
| `transient_coverage_max_sample_temperature_c` | C | Maximum covered/exposed cloth temperature over the initial state and all accepted sample endpoints. |
| `transient_coverage_final_min_temperature_c` | C | Minimum final covered/exposed cloth-cell temperature. |
| `transient_coverage_final_max_temperature_c` | C | Maximum final covered/exposed cloth-cell temperature. |

For example, after authoring a common `scenario.motion` with `transient_coverage`, this study policy
compares peak motion strain, signed inward energy and complete declared assembly mass:

```json
{
  "basis": "nominal",
  "objectives": [
    {"metric": "motion_max_absolute_edge_strain_pct", "direction": "minimize"},
    {"metric": "transient_coverage_inside_energy_j", "direction": "minimize"},
    {"metric": "assembly_mass_kg", "direction": "minimize"}
  ],
  "required_consumers": ["sewing"],
  "allow_mixed_protocols": false
}
```

This needs the study's actual `assembly_mass` inventory and sewing request. Objectives automatically
require their consumers: transient metrics require both complete motion and their specific heat
history. Alternatively, add `motion`, `transient_surface` or `transient_coverage` to `required_consumers`
when completion matters even though the objective is cost, static heat or another metric. Successful
motion does not require unrequested heat; a refused independent-sheet history does not block a
coverage-only comparison, and a coverage refusal does not block a sheet-only comparison.
The returned `comparison.motion_required` states whether the policy requires completed movement;
merely supplying a motion scenario does not change an existing static comparison policy.

Admission retains the initial settled/gating-clear requirement. Motion must match the descriptor's
path and the initial drape's position identity, retain converged, contact-checked samples through the
exact requested duration, and link every requested endpoint time/pose to its accepted geometry.
Each required thermal history must retain the exact requested initial temperature, boundary/layer
policy and duration, with one time/interval/geometry-linked row for every accepted mechanical sample.
A late mechanical or thermal refusal, stale path, incomplete history or missing requested metric
excludes that candidate. Its successful manufacturing and initial static outputs remain available;
none can supply a substitute motion reading. Different heat-capacity methods, conditioning or evidence
make transient alternatives incomparable by default, alongside the existing mechanical and transport
protocol checks. Steady-only comparisons do not acquire a new heat-capacity requirement.

Energy retains its sign, and degrees Celsius can be negative. Minimizing signed inward energy is
not minimizing its absolute magnitude, and warmer cloth is not automatically a warmer wearer.
Independent-sheet and coverage histories are alternative domains whose totals must not be added.
Coverage temperature extrema include exposed cloth; they are neither average temperatures nor
anatomical body temperatures. Net physical boundary energy excludes enthalpy-remap residual, which
remains separately reported. Missing final temperature extrema never become the initial temperature.
The `*_sample_temperature_c` metrics include the initial state and all retained mechanical sample
endpoints; `*_final_*_temperature_c` metrics use only the last accepted sample. Neither includes
unreported thermal substep extrema or establishes a continuous-time temperature bound.
These remain nominal, one-way simulated outcomes, with no thermoregulation, comfort, fit, uncertainty
propagation or empirical accuracy claim.

## Reports and replay

`ftx-material-scenario-study/1` contains:

- `manufacturing`: the unchanged production study report with frozen changes, bound `pattern_json`,
  actual cutting/sewing/dated purchasing outcomes and their independent failures.
- `simulations`: key-matched outcomes. An available result retains actual `drape`, final positions,
  membrane triangles, resolved placements, gating flags, mass, and separate `surface` / `coverage`
  outcomes. Missing heat is null/refused, never zero heat. A body or mechanics refusal cannot erase
  a valid cutting or purchasing result.
- Optional `motion` inside each available simulation: complete continuation samples and endpoint
  steady heat, or its independent refusal. Initial static data is never replaced by endpoint data.
- `scenario`: numerical/environmental inputs, model hash, retained measurement evidence, model-fit
  residuals and actual body identity. The overall request identity binds the source pattern, study
  policy and scenario. Per-result placement hashes distinguish placement frames as well as skins.

A successful process exit means the study report was computed, not that every alternative passed.
Inspect each consumer's `status` and the drape's gating flags. Heat is evaluated on the computed final
frame even when those flags require review; an available number is not automatically admissible as a
settled-garment objective. Replay each alternative's `frozen_changes` against the original pattern,
using the same scenario, body input and model. Keep raw Rust-produced JSON text through browser
boundaries: parsing/stringifying source receipts in JavaScript can change numeric identities.

## Library and browser-callable API

`ftx_export::material_study::Request::evaluate_with` composes an additional production consumer with
existing manufacturing without adding a numerical dependency to the export crate. The numerical
implementation is `ftx_drape::study::Session`; `study::fitted::FittedBody::prepare` selects and poses
the common shape using `BodyRequest`. `FittedBody::fit` retains the legacy tape-only convenience API.
`ftx_wasm::materials::scenario::on_body` provides a file-free fixed-body entry point and
`on_model` the body-source adapter. The browser export is
`material_scenario_study(webBody, patternText, studyText, scenarioText, bodyInputText)`.
It uses the model bytes identified by `WebBody`, never a catalog material name as a physics shortcut.

## Material workbench

After `sh tools/build_web.sh`, open `web/materials.html` from the existing local web server.
Section 4 exposes screening, manufacturing studies and fresh model-backed scenarios through the
worker, with cancellation. Load the **original** pattern in section 2 and the study request in
section 4; a prior single-preview binding is neither required nor used. For scenarios, also select
the measurements and converted body in section 3 and the scenario request in section 4. The scenario
owns its fit/numerical/thermal policy; the separate single-preview controls do not override it.
For an accepted fit or another standing arm angle, load the optional **body-source request** in
section 4. It replaces only the scenario's measurement input, not other preview controls; shared
measurement files are no longer needed for that scenario. Clearing the optional file restores
legacy tape input. Changing the file withdraws previous results and cancels stale reads/work.
Malformed overrides never fall back to a valid shared measurement file. Downloads retain the exact
request as `body-input.json`, along with source-locked patterns and the complete study report.

The comparison shows source-qualified scalar readings with units/bounds, excluded candidates,
independent cutting/sewing/purchasing outcomes, signed heat and explicit drape gating. Missing
values remain unknown, and incomplete offer coverage is not a free order. Source/protocol details
are inspectable per alternative. Downloaded reports, input JSON and bound alternative patterns
retain the original text; display-only JSON is not used as a replay artifact. Changing actual
dependencies removes their outputs and download URLs, while unrelated preview edits preserve
study results. Declared computed objectives display the Rust frontier, units, exclusions and
incomparable pairs; no weighted score or Pareto frontier is fabricated in JavaScript.
Requested motion adds a separate completion/substep/strain table and per-endpoint steady-heat table,
including independent refusals and the initial settledness warning. Raw reports contain the full
sample arrays and material history. Expand an available trajectory to inspect the retained body
and cloth from front or side. The integer slider selects an actual recorded state and displays its
exact physical timestamp, including nonuniform adaptive steps. Both views and every state use one
fixed metric scale; the inspector does not interpolate between states or infer depth from its
wireframe projections. A separate initial state is shown only when the report retains it. Older
accepted-only reports start at their actual first positive timestamp. The existing single-fold
viewer remains a separate static workflow. Editing the scenario's time/pose policy invalidates
these results and their inspector through the existing request lifecycle.
Requested transient histories add separate time/temperature/energy tables with independent refusals,
remap residuals and lazy source/checkpoint inspection. The page displays Rust results without summing
energy or deriving a warmth score. Its existing scenario download retains the exact thermal policy.

The existing frontend tests include optional **real compiled WASM** execution without a model
asset or native executable:

```sh
sh tools/build_web.sh
FTX_REAL_WASM=1 node --experimental-vm-modules --test tools/check_materials_frontend.cjs
```

That mode checks the actual worker-to-WASM bridge, causal marker/sewing changes, independent
missing-data outcomes, exact source/request downloads, frozen replay, deterministic repeats and
malformed-model rejection. Its Node loader is not an HTTP/browser transport or a successful
anatomical fitting/drape test; the existing `tools/check_materials_web.cjs` remains the separate
Chromium/native comparison lane.

For positive model-backed motion equality, the existing synthetic native test leaves its files in
`${TMPDIR:-/tmp}/ftx-shape-*`. Choose the directory containing `motion-report.json`, then run:

```sh
cargo test -p ftx-cli --test accepted_body_workflow scenario_binaries --locked
FTX_REAL_WASM=1 FTX_MOTION_REPLAY=/path/to/generated/ftx-shape-directory \
  node --experimental-vm-modules --test tools/check_trace_frontend.cjs
```

This optional lane sends the intact pattern, study, scenario, accepted-body input and synthetic model
through the actual worker and compiled WASM. It compares the **entire** report against the native
binary, checks exact replay downloads and repeats the trajectory without retained prior state.

## Evidence and remaining gates

Synthetic tests compare against direct production drape, surface, coverage and manufacturing calls.
They exercise frozen replay, deterministic repeats, thermal-only substitutions preserving motion,
mechanical substitutions changing motion, independent refusals and admission before solver work.
Real-binary tests verify both commands retain manufacturing when model decoding refuses. Accepted
reuse additionally runs through both binaries on an explicitly synthetic articulated Anny-format
prism, with actual drape/steady heat, unchanged parameters across poses, visible tape disagreement,
identical native reports and transport-only material swaps. Prescribed motion additionally checks
every accepted sample against direct solver continuation, source-model endpoint equality, preserved
shape, timing-dependent response, fresh endpoint heat and late-failure isolation. Library tests
also verify transient sheet recurrence, reference-mass storage, conservative coverage remap, final
checkpoint restoration and independent thermal failures. The synthetic native motion fixture now
contains both transient consumers; the optional compiled-WASM replay compares their full reports.
These are deterministic software and numerical conservation checks, not measured heat-flux data.
This is a positive synthetic model-backed software run, not a successful anatomical garment fit,
and target compilation alone is not WASM
execution parity. Converted anatomical Anny assets were unavailable during this delivery.

`ftx-ms2` stays open for propagated uncertainty, finishing effects and empirical inventory/garment-mass
acceptance, expanded body/pose policies, two-way thermal coupling, qualified motion objectives and
physical acceptance of the nominal comparison path. Standing-arm motion continuation is available;
it does not establish an anatomical or empirical ride-up experiment.
Complete declared pre-finishing assembly accounting is available; it cannot discover an omitted component.
Synthetic coupons and numerical conservation
are not measured linen/leather/knit garment validation. No warmth rating, empirical fit,
whole-wearer accuracy, global optimum or hardware execution authorization is claimed.
