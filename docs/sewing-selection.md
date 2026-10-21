# Choose qualified sewing recipes on the actual pattern

`ftx materials sew-select PATTERN.json REQUEST.json --json` searches explicit needle, thread,
maximum-pitch and stitch-rate choices using the **existing complete sewing planner**. It selects the
admitted complete job with the least generated continuous-feed time, or explicitly qualified feed-plus-setup
time in version 2. It does not manufacture process
qualification from fiber names, machine capabilities alone, or synthetic material presets.

## Runnable synthetic job

```sh
cargo run -p ftx-cli --bin ftx -- materials sew-select \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-selection.json --json > selection.json

python3 -c 'import json; s=json.load(open("selection.json"))["result"]["selected_options_json"]; assert s is not None, "No admitted complete recipe"; open("selected-sewing-options.json", "x").write(s)'

cargo run -p ftx-cli --bin ftx-materials -- sew-stitches \
  tests/fixtures/materials/synthetic-sewing-pattern.json selected-sewing-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-svg \
  tests/fixtures/materials/synthetic-sewing-pattern.json selected-sewing-options.json \
  side selected-sewing-review.svg
```

Use new output paths. The fixture, its material and both process windows are **invented for software
verification**. They are not instructions or qualified settings for a real machine. The unchanged
100-mm seam has 40 intervals at 600 stitches/minute in the template: four continuous-feed seconds.
The selected alternative changes the needle/thread identifiers, chooses 25 intervals at 750/minute,
and gives two seconds. A nominally faster 950/minute alternative is refused by its source process
window, although it remains below the declared machine's maximum rate. The new result still has
`hardware_execution_authorized: false` and `physical_seam_quality_validated: false`.

## Input and what stays fixed

`ftx-sewing-selection/1` contains the ordinary `options` sewing job, the explicit objective
`minimize_feed_seconds`, `max_plan_evaluations`, and `alternatives`. Each alternatives group names an
existing **operation ID** and contains recipes with a distinct `id`, `needle`, `thread`,
`max_stitch_mm` and `stitches_per_minute`. `template` is reserved for the unchanged pass. The original
recipe always participates; unlisted passes are never removed or altered.

The template must explicitly set `require_qualification: true`. Every candidate goes through
`SewPlan::build` with all original machine limits, exact material/qualification sources, plies/folds,
pressure settings, stitch paths, allowances, backtacks, directions and predecessors unchanged. There
is no precomputed lookup that can miss an actual notch-local advance, a changed pressure stack, an
unsupported reverse movement or an unqualified material substitution. A failed template does not
prevent the search from finding an admitted combination of changes across multiple passes.

Candidates can change only those four process settings. This is not continuous optimization of all
machine variables, automatic seam topology, tension/workholding identification, pressure selection,
or a physical seam-quality model. The supplied process qualifications must already support the
intended specimen, conditions and settings. Reducing stitch count is only an admissible choice when
that resulting operation remains inside the independent process window.

## Search outcomes and limits

The template is evaluated first. Groups and recipes are then traversed by operation/recipe ID,
independent of input array order; exact time ties retain the first admitted job. The version-1 objective is
actual generated **continuous feed seconds**, including repeated backtack travel. It does not include
setup duration, handling, acceleration or loading and is not an estimate of garment production time.
Existing optional setup-change ordering is executed for each admitted job under the unchanged policy;
its separate count objective and proof stay in the selected plan, not in the recipe-time claim.

There are at most 64 complete-plan evaluations, including refusals, up to 64 template operations,
12 variable operations and 15 alternatives each. A smaller evaluation limit is explicit. Each ordinary
plan retains its existing bounded geometry/station/pressure work. With ordering enabled, the product
of the recipe evaluation cap and each job's ordering-node cap cannot exceed 200000. This bounds
repeated ordering work without silently reducing the caller's chosen per-job limit.

The report retains every attempted choice and its original refusal or actual generated time/count.
`search_complete` means every listed combination was examined, not that unlisted recipes were ruled
out. `budget_exhausted` describes the outer recipe-evaluation cap. Inner resource/convergence failures
are counted separately as `unresolved_trials`; neither category supports an infeasibility or optimality
claim. `optimality_proven` requires an admitted plan, exhaustive enumeration and no unresolved trial.
`no_feasible_listed_recipe` requires exhaustive, definitive refusals; it is not physical impossibility.
The best **complete** incumbent remains available after interruption. No partial garment plan is saved.

## Directed setup durations and the joint objective

An ordinary job may now request `ordering.objective: "minimize_setup_time"` with a known
`initial_setup` (exact needle/thread IDs) and `setup_times`. This selects `ftx-sewing-plan/9` and uses
the existing dependency scheduler, preserving complete operation geometry, pressure/fold handling,
qualification and backtack checks. Count-only ordering and untimed plans retain their previous outputs.

The timing table names the exact `machine` ID, a `source` (reference, accessed_on, note), `method`,
`conditions`, `evidence`, independent `allow_estimates`, and directed `transitions`. Each row has
`from` and `to` needle/thread setups and `milliseconds`, an integer from zero to 86400000. Timing a
combined needle/thread change is one whole-setup observation, not two inferred independent durations.
Reverse changes need separate rows. No change costs zero; explicit no-change rows and duplicates
refuse. Missing changes are **unknown**, not zero. The known initial setup is charged; no final reset
is assumed. The table is limited to 4096 rows, all under the same source/protocol and evidence policy.

The scheduler compares integer milliseconds with a conservative lower bound valid for asymmetric,
nonmetric durations. A longer sequence of cheap changes can beat fewer expensive ones. Its
`ordering.timing` reports baseline/selected setup milliseconds, the proved lower bound and generated
`feed_and_setup_seconds`. Existing change counts are also retained but are not optimized in time mode.
Scheduling remains limited to 20 passes and the declared search-node budget. A budget stop returns a
complete feasible incumbent and explicitly does not claim a duration optimum.

To choose recipes as well as their order, set the request schema to **`ftx-sewing-selection/2`** and
`objective` to **`minimize_feed_and_setup_seconds`**. Its ordinary `options.ordering` must use the
timed policy above. Every candidate is regenerated and scheduled by the same production planner;
selection compares that planner's actual feed-plus-setup result, not a lookup by nominal recipe pitch.

```sh
cargo run -p ftx-cli --bin ftx -- materials sew-select \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-timed-selection.json --json > timed-selection.json
```

The new fixture is entirely synthetic. Two prerequisite-ordered operations use eight feed seconds
without a setup change. Their faster recipes need only four feed seconds but require a ten-second
changeover, so the selector retains the eight-second template, not the fourteen-second alternative.
Changing only the declared entry-change duration to three seconds makes both faster recipes win at
seven seconds. The reverse change is independently declared at seven seconds; mixed jobs are compared
with their actual directed changes. Neither example is a measured production-time improvement.

The timing domain is checked **before** the recipe search: all directed pairs among up to 64 distinct
candidate setups and initial-to-candidate changes need source rows, even when prerequisites would
exclude some transitions. Unused rows are allowed so one table can cover the alternatives. This
conservative complete-domain requirement avoids declaring an unmeasured recipe infeasible and then
claiming a false optimum among only the known durations. Invalid or missing timings refuse the
request; they are never dropped from the candidate list or repaired with count weights.

Version-2 reports retain each admitted trial's `objective_seconds` and
`ordering_optimality_proven`, plus `selected_objective_seconds` for the complete incumbent. The outer
`search_complete` and `budget_exhausted` still refer only to recipe enumeration. Every incomplete
inner schedule is now an `unresolved_trial`, even though its complete incumbent remains admissible.
Thus exhaustive recipe enumeration alone cannot prove the joint optimum. Version-1 feed-only results
remain independent of schedule proof, and their original serialized fields are unchanged.

Feed and setup seconds exclude handling, loading, acceleration, parallel machines, stoppages and final
reset. Source timing conditions are retained, not matched automatically to machine sensors or workers.
Neither timing measurements nor successful recipe qualification demonstrate empirical seam quality or
total garment cycle time. Timing-estimate permission cannot relax any material or process evidence gate.

## Replay and browser boundary

The command returns the ordinary `ftx-cli/1` envelope; `result.report.selected_plan` holds the complete
source-bearing production result. `result.selected_options_json` holds the exact options document
for the existing sewing commands, or null when no incumbent exists. Retain that string unchanged when
saving it rather than parsing and reformatting it in JavaScript. The source pattern is unchanged and
its digest is in the report. If a live catalog selection changes later, re-evaluation still must pass
its exact qualification locks; the saved report is not authorization to relabel a changed material.

Exit 0 means a complete admitted recipe exists, **even when search was interrupted**. Inspect the
proof/budget fields independently. Exit 2 retains the report but means no complete incumbent was
found. Malformed requests and incompatible search caps are structured CLI refusals, not successful
empty selections. There is no command-line flag that relaxes material or qualification estimate policy.

The shared file-free boundary is `ftx_export::sewing_selection::select_json`, also available natively
as `ftx_wasm::sew_select` and exported as `sew_select(patternJson, requestJson)` in a rebuilt WASM bundle.
It returns the same job JSON without the CLI envelope. Combined input and each complete escaped output
are capped at 64 MiB. There is no separate browser scorer. This delivery supplies the callable boundary,
not a new interactive recipe-selection page or a browser-runtime parity measurement.

Plan §25.4 / `ftx-tm7` and `ftx-sp2`: numerical and real-command tests establish selection and replay.
Real matched seam coupons, process quality, machine registration, workholding, sensing and interlocks
remain independent acceptance work. None of the computed outputs authorizes hardware execution.
