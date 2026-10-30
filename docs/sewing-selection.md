# Choose qualified sewing recipes on the actual pattern

`ftx materials sew-select PATTERN.json REQUEST.json --json` searches explicit needle, thread,
maximum-pitch and stitch-rate choices using the **existing complete sewing planner**. It selects the
admitted complete job with the least generated continuous-feed time, explicitly qualified feed-plus-setup
time in version 2, or complete source-qualified thread order cost in version 3. It does not manufacture process
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

Recipe candidates change only those four process settings. This is not continuous optimization of all
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
Thread purchasing is separately bounded to 200000 configured quote/stock-alias work units across
every allowed recipe trial; cost mode also reserves its one common-market preflight. Studies reserve
this entire search bound for every expanded material/design row before evaluating alternatives.

The report retains every attempted choice and its original refusal or actual generated time/count.
`search_complete` means every listed combination was examined, not that unlisted recipes were ruled
out. `budget_exhausted` describes the outer recipe-evaluation cap. Inner resource/convergence failures
are counted separately as `unresolved_trials`; neither category supports an infeasibility or optimality
claim. `optimality_proven` requires an admitted plan, exhaustive enumeration and no unresolved trial.
`no_feasible_listed_recipe` requires exhaustive, definitive refusals or proved deadline exclusions;
it is not physical impossibility.
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

## Choose recipes by complete observed thread order cost

Set **`schema: "ftx-sewing-selection/3"`** and
**`objective: "minimize_thread_observed_order_cost"`** to select by the actual complete thread
orders. The ordinary template must explicitly include `options.consumables.purchasing`, its quote
book and market. Each candidate still passes the complete production sewing planner; the selector
then consumes that plan's unchanged `consumables.purchasing.least_observed_independent_order_total`.
It does not estimate consumption from seam length, assign a universal thread ratio, or reconstruct
prices from nominal unit costs.

The admitted qualification for every operation must supply its own independently sourced
`thread_consumption` model and declare all required channels. The shared consumer counts the actual
generated stitch intervals, including reverse travel, backtacks and reinforcement, then adds the
declared start/end allowance once per operation/channel. It aggregates each exact frozen yarn
snapshot across the complete job **before** the canonical quote engine applies order minimums,
increments, all-unit price thresholds, stock and continuous-length limits. Different recipe setups
may use different qualified consumption observations and yarn selections; each observation remains
applicable only inside its admitted parent process window.

```sh
cargo run -p ftx-cli --bin ftx -- materials sew-select \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/sewing-thread-cost-selection.json --json > thread-cost-selection.json
```

This example is entirely synthetic. Its original four-second recipe needs 1.7 metres across the two
declared thread channels. A two-metre minimum/increment produces a USD 10 order. The qualified
two-second alternative uses 3.5 metres under its independently declared consumption model: a
four-metre order costs USD 14 after its all-unit threshold. Cost selection retains the USD 10
template; feed-only selection chooses the two-second alternative. Neither number is a real supplier
quote or a measured thread-use claim. Separate regressions also cover a larger-demand recipe whose
whole-order threshold makes it cheaper, so the search never prunes on an assumed monotone cost.

The quote book and market remain identical across all recipes, including currency, region, condition,
cost basis, evaluation time and stock-observation policy. The canonical market selector validates
them before enumeration. There is no currency conversion, revival of superseded locked offers,
independent reuse of aliased supplier stock, or relaxation of expiry and capacity checks. The
market's `require_observed_quantity` and `require_observed_continuous_length` settings remain
explicit: a complete observed price under a policy allowing unknown stock does not establish
observed stock sufficiency or guaranteed availability. Those coverage flags stay in the selected
plan's canonical purchasing receipt.

Version-3 trials retain `thread_order_cost` only when the entire sewing demand and every exact-yarn
order are covered. A successfully planned recipe with missing consumption, partial channel coverage
or an excluded/missing order instead retains `objective_unavailable`, independently of the ordinary
production `refusal`. These are **unresolved** trials, never zero-cost winners or proof that the
listed sewing recipes are infeasible. A complete priced incumbent remains available, but an
exhaustive search with any such unknown cannot claim `optimality_proven`. A source or machine
qualification refusal remains the original production refusal. Budget exhaustion likewise retains
only a complete incumbent and no optimum claim.

The report supplies `selected_thread_order_cost` and the one `objective_currency`; the latter is
retained even if no complete priced incumbent exists. Exact cost ties prefer the original template,
then the existing operation/recipe ID traversal. The selected plan retains its full consumption
sources, exact yarn snapshots and canonical supplier order receipts, and its options replay through
the ordinary sewing commands. Feed/setup objectives keep their existing schemas and serialized
fields. The cost objective does not require an optimal operation ordering because ordering cannot
change the declared operation set, traversed intervals or per-operation thread allowances.

This objective covers the independently priced thread orders for the complete declared production
quantity. Set `options.consumables.production_copies` for standalone selection; material/design studies
inherit their cutting quantity when it is omitted and refuse mismatches. The same per-garment recipe
is repeated, then all yarn demand is aggregated before source price tiers, minimums and stock apply.
The selected options retain that count for exact replay. Geometry and feed/setup time remain per garment.
It does not combine thread with fabric cutting costs, value worker time, or optimize a coupled supplier
shipping bundle or cross-garment execution schedule. Production quality and hardware permission retain their
independent gates. Material-study comparisons continue applying their explicit source-protocol
policy to each actual selected plan.

## Choose a complete recipe that meets a deadline

Any existing selection request can include an explicit inclusive
`completion_deadline_milliseconds`. For example, this fragment makes complete thread cost the
objective while requiring the selected job to finish within 25 seconds:

```json
{
  "schema": "ftx-sewing-selection/3",
  "objective": "minimize_thread_observed_order_cost",
  "completion_deadline_milliseconds": 25000,
  "max_plan_evaluations": 32
}
```

Keep the complete qualified `options` and `alternatives` alongside these fields. The deadline does
not supply missing timing observations: serial jobs need the existing source-qualified setup and
handling policy, while parallel and routed jobs use their complete retained schedules. Feed-only or
feed-plus-setup time cannot establish complete production time. The deadline is an exact integer
from zero through 9007199254740991 milliseconds, measured from the existing job origin. Routed and
parallel decisions compare their integer clocks directly; serial decisions retain the canonical
continuous-seconds accounting without rounding a late schedule into compliance.

Selection filters complete plans by the deadline **before** choosing the least objective value.
A cheap, late recipe therefore cannot hide a more expensive recipe that meets the requirement.
The original objective values remain visible on excluded and unresolved trials. The same path
works with fixed specialist routes, qualified calendars, fixtures, attending workers and transfers.
Existing eligibility and allocation restrictions on cost/feed objectives still apply: this constraint
does not turn a makespan-selected cell assignment into a joint cost/assignment optimizer.

Each trial retains `completion_deadline` with its limit, timing basis, available selected completion,
valid lower bound, timing proof and reason:

| Status | Meaning |
|---|---|
| `feasible` | A complete incumbent finishes on or before the deadline, even if its minimum time is unproved. |
| `excluded` | A valid completion lower bound or proved minimum places every admitted schedule after the deadline. |
| `unresolved` | Full timing is unavailable, or the retained schedule is late without a proof ruling out a better one. |

A trial whose deadline status is `unresolved` cannot compete and prevents an outer optimality or
all-recipes-infeasible claim unless the production planner independently refused it definitively.
A definitively excluded candidate cannot win, even if its independent cost is unknown. A
feasible cost-directed incumbent does not need a separate timing-optimality proof when ordering
cannot change its thread demand; time-directed objectives retain their existing inner proof rules.
A production-planner refusal remains independently visible, and the ordinary resource/refusal
classification continues to govern it.

The standalone result uses `ftx-sewing-selection-report/7` when a deadline is supplied. Its retained
request, every trial and exact selected plan preserve the decision. Ordinary selected-options replay
reproduces that complete plan, while rerunning the retained selection request rechecks the deadline
and listed alternatives. Material/design/scenario studies consume the same selected plan for sewing,
thread orders and combined fabric-plus-thread purchasing. Their sewing/thread comparison protocol
includes the complete constrained selection request, including unused candidates and work limits.
An outer study timing constraint does not silently become a recipe-selection deadline; declare it
in the selector when the search must find a feasible alternative. The workbench shows each trial's
complete time, lower bound and admission beside its cost-search outcome.

The synthetic integration case changes the choice from a USD 10 job completing at 4.3 seconds to a
USD 14 job completing at exactly 3.3 seconds under a 3300-millisecond deadline. A 3299-millisecond
limit admits neither job and leaves independent manufacturing outcomes available. These fixture
timings and prices establish software behavior, not measured throughput or purchasing authorization.

## Compare independently qualified specialist routes

`ftx-sewing-selection/6` with `minimize_parallel_makespan` can compare an authored finite set of
complete specialist-cell routes. The original `options.production.routing` remains the first
candidate. Supply `route_candidates: [{"id": "another-route", "routing": ...}]`, where `routing`
is a complete `fixed_operation_route/1` or `eligible_operation_route/1` policy, including its exact cell configurations,
process and timing qualifications, content locks, availability, staging and transfer resource.
There are one to fifteen alternatives; IDs must be unique and `template` is reserved.

Only the routing policy is replaced. The original pattern, operation set, construction, garment
quantity, thread market and qualification requirement remain unchanged. Each complete candidate is
admitted by the ordinary sewing planner and retains its actual stitches, feed, handling, transfers,
yarn demand and purchasing. Missing or stale source evidence cannot be borrowed from another route.
An eligible policy additionally searches the explicitly qualified cell choices for each physical
copy and operation. Machine eligibility must be authored and independently qualified; it is never
inferred from a machine name or from another candidate's evidence.

The outer selector compares makespan for both routing models. When the template and every candidate
use fixed operation routes, version 6 also accepts `minimize_thread_observed_order_cost`. It minimizes
the complete canonical thread orders for each route's actual stitch intervals and qualified channel
quantities. Supplier minimums, increments, price tiers and stock checks apply to the full order;
unknown or incomplete orders remain unresolved. The thread market stays shared across routes.
Because fixed assignment makes these quantities independent of dispatch order, a complete priced
plan can establish its cost without proving the minimum makespan. An optional inclusive
`completion_deadline_milliseconds` still requires an admitted complete schedule before selection.

Eligible assignment changes the actual recipe, stitch intervals and yarn demand while optimizing
makespan. Feed-only or thread-cost selection therefore remains refused whenever the template or
any candidate uses eligible cells. Material comparisons may still report that selected batch's
actual feed and thread quantities. The full eligible-route or fixed-route cost-search request,
including unused candidates and its evaluation cap, participates in the relevant study protocol.

Recipe `alternatives` may be empty in version 6. When supplied, their Cartesian product is searched
within each route: the template route first, then route IDs in lexical order, with the existing
operation/recipe ordering inside each route. The **same** `max_plan_evaluations` cap of at most 64
includes every complete route/recipe attempt and refusal. Scheduling, source-plan and purchasing
reservations use the largest complete candidate before multiplying by that shared cap or by study
alternatives. No candidate may increase a hidden inner work allowance.

Version-6 reports retain `route_choice` on each trial and `selected_route` for the complete
incumbent; material studies keep the same identities beside their ordinary selected sewing plan.
`selected_options_json` freezes the winning route for ordinary plan, stitch and cell-preview replay.
Exact objective ties retain the first admitted candidate. Cost-mode reports retain
`selected_thread_order_cost`, `objective_currency` and per-trial `thread_order_cost`; they do not
label money as elapsed seconds. An incomplete schedule, missing order or unresolved deadline remains
visible and prevents an optimum claim even after all listed combinations are visited. In makespan
mode, an unproved inner timing optimum also prevents an outer optimum claim.
Route-specific timing admission runs on each actual attempt. Missing timing retains the original
refusal plus `objective_unavailable`, so another qualified route remains usable without claiming
the unscored route is slower or infeasible. Existing recipe-only timing preflight is unchanged.
Any proof is conditional on the listed choices and their qualified dispatch models, never all
possible manufacturing routes, measured throughput, seam quality or hardware authorization.

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
