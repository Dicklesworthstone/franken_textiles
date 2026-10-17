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
material-history reports. Its initial-position identity links back to the retained static result.
Peak absolute panel-edge strain includes the initial state and all accepted samples, using each
original flat panel chart across welded seams. It is not principal strain, a rupture criterion or fit.
Use the accepted sample sequence rather than inventing a long interpolation over omitted substeps.

Each endpoint independently recompiles source-locked steady sheet transport and body coverage from
the **new cloth and body geometry**. These are equilibrium snapshots, not a transient-temperature
history, energy accumulated during motion or a comfort prediction. A thermal refusal does not erase
the completed mechanical trajectory. The scenario's original `drape`, `surface`, `coverage`, positions
and comparison objectives continue to describe the **initial static drape**, not the moved state.
The current comparator does not rank motion histories or require motion success. Inspect its separate
status; a static frontier is not a motion-qualified frontier. Neither initial geometric settledness
nor successful implicit substeps establish viscoelastic equilibrium or empirical garment accuracy.

Admission permits 1..16 endpoints through 60 seconds, 0..180-degree angles, maximum substeps in
1e-9..1 seconds, 1..512 attempted substeps for the **entire** trajectory, up to one million pairs
per continuous-collision query and up to two million retained body/cloth vertex samples. Initial
positions count against that sample budget. Endpoint body meshes have an additional two-million-
vertex allocation cap. Each attempted material run reserves eight times `max_sample_vertices`
against the shared scenario vertex-frame budget for motion samples and endpoint thermal receipts,
on top of static work. Limits refuse instead of truncating an alternative's trajectory.

Library callers with explicit meshes can construct `study::motion::Path` and attach it once with
`Session::set_motion_path` before evaluation. `FittedBody::motion_path` generates source-model
endpoints for the native and model-backed browser adapters. A fixed-body `on_body` call without an
attached source path refuses requested motion rather than pretending its body moved.

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
pre-finishing assembly, not purchased waste or a weighed finished garment. Its conditional reported
ranges do not relax the nominal-only comparison policy. See `docs/material-screening-and-studies.md`.

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
missing-value checks. Nominal comparison requires explicit consent: input ranges are not propagated
objective intervals. Geometric settledness is not viscoelastic equilibrium, measured fit, thermal
comfort or proof that the requested run duration was physically sufficient. Positive admission tests
use explicitly synthetic callback outcomes; the actual cylinder integration test demonstrates that
clear-gating but drifting cloth is excluded. No settled anatomical garment experiment is claimed.

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
sample arrays and material history. The existing single-fold viewer remains a static-workflow
viewer; these controls do not silently replace it with an untested endpoint animation. Editing the
scenario's time/pose policy invalidates these results through the existing request lifecycle.

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
compare against direct production consumers and verify pose-dependent geometry. This is a positive synthetic model-backed
software run, not a successful anatomical garment fit, and target compilation alone is not WASM
execution parity. Converted anatomical Anny assets were unavailable during this delivery.

`ftx-ms2` stays open for propagated uncertainty, finishing effects and empirical inventory/garment-mass
acceptance, expanded body/pose policies, transient thermal coupling, qualified motion objectives and
physical acceptance of the nominal comparison path. Standing-arm motion continuation is available;
it does not establish an anatomical or empirical ride-up experiment.
Complete declared pre-finishing assembly accounting is available; it cannot discover an omitted component.
Synthetic coupons and numerical conservation
are not measured linen/leather/knit garment validation. No warmth rating, empirical fit,
whole-wearer accuracy, global optimum or hardware execution authorization is claimed.
