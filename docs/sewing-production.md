# Sewing production across garment copies

Optional `SewOptions.production` schedules the actual operations of several garment copies on a
declared machine, or across the independently qualified cells described below. Each copy retains
its own seam dependencies; compatible operations from different
copies can share an installed needle/thread setup. The existing bounded ordering solver minimizes
identity changes, explicitly sourced directed setup durations, or complete declared serial
feed/setup/handling time when the independent handling model is requested.

Every source operation first passes the ordinary material, thickness, geometry, stitch/feed, seam,
pressure and requested process-qualification checks. A material substitution reruns those checks.
Production scheduling adds no substitute material model or assumed process qualification.

## Run a complete example

The example uses the existing synthetic two-piece pattern, two sequential seam passes and three
copies. The machine envelope and timing table are invented software inputs, not measured recipes.

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-plan \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/sewing-production-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-stitches \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/sewing-production-options.json
```

The primary `ftx materials` commands and portable/browser adapters consume the same options.
In this example, the baseline finishes `join` then `reinforce` on each copy in turn. The selected
schedule joins all three copies before reinforcing them. Each copy's own join remains a predecessor
of its reinforcement. Including the explicitly declared initial thread installation, the result is:

| Quantity | Baseline | Selected |
|---|---:|---:|
| Actual stitch intervals across three copies | 240 | 240 |
| Continuous feed | 24 s | 24 s |
| Directed setup time | 6.5 s | 1.5 s |
| Feed plus declared setup time | 30.5 s | 25.5 s |
| Thread identity changes, including initial installation | 6 | 2 |

This is an exact conditional software example, not an elapsed manufacturing-time measurement.

## Policy and quantity

Add the following to otherwise ordinary sewing options for a count-only schedule:

```json
{
  "production": {
    "production_copies": 3,
    "ordering": {
      "objective": "minimize_setup_changes",
      "max_search_nodes": 2000
    }
  }
}
```

For timed scheduling use `minimize_setup_time`, an explicit `initial_setup` and the existing
source-bearing `setup_times` table, as shown in the full example. Every required directed setup
transition must have a duration; missing evidence never becomes zero. The same-setup transition
has zero *setup-change* duration. It does not establish zero garment handling time. Estimated
durations need their own explicit consent, independently of material or seam qualification.

One-garment `ordering` and `production` are mutually exclusive. If `production_copies` is omitted,
the standalone planner inherits an explicitly requested consumables quantity, otherwise uses one.
Material/design studies also inherit the common requested cutting quantity. Explicit scheduling,
thread and cutting counts must agree. The admitted plan freezes the resolved count into its options
so it can be replayed independently.

Thread consumption still starts with each qualified source operation's actual traversal and its
once-per-operation/channel allowances. It repeats that contribution on every copy, then aggregates
complete yarn demands before existing minimum orders, increments, tiers and inventory checks. A
thread inventory request by itself retains its earlier behavior and does not create a schedule.

## Report and stitch program

The opt-in produces `ftx-sewing-plan/12`. Source station arrays remain in `plan.seams` once per
original operation. `plan.production.operations` references those arrays by `seam_index` and retains
the original operation ID, zero-based garment `copy`, exact qualification ID, scoped prerequisites,
traversals and feed duration. Its unambiguous scoped ID is `copy/operation_id`; original IDs may
contain slashes without colliding with another copy.

`plan.production.ordering` contains the complete baseline and selected sequences, separate
needle/thread counts, qualified timing, search work, lower bound and proof/budget status. The initial
setup is charged once for the entire batch. No final reset is assumed. Timing sums use 64-bit integer
milliseconds on native and WASM, so large complete batch totals cannot wrap at 32 bits.

`plan.stitch_intervals` and `plan.feed_seconds` retain their one-garment scope. Batch totals live at
`plan.production.stitch_intervals` and `plan.production.feed_seconds`. Rust callers can use
`SewPlan::total_stitch_intervals()`, `total_feed_seconds()` and `ordering_report()` to consume the
active scope; `SewOptions::ordering_policy()` exposes the corresponding request policy. Material
study and qualified recipe-selection consumers use these same authorities.

`sew-stitches` emits `ftx-sewing-program/4` when production is requested. Every traversed interval
follows the selected batch sequence and carries its physical `copy` plus the original operation
index. Reverse passes, locks, partial seams and qualified flat-fold coordinates keep their existing
meanings. The original station geometry is retained once; the complete repeated output is charged
against the existing 250000-interval expansion limit before allocation.

## Finite fixtures and workholding

Optional `production.workholding` enforces finite reusable resources while choosing the actual
copy-scoped sewing sequence. A hold acquires its complete integer demand **before** its acquisition
operation starts and releases it **after** its release operation finishes. Every hold repeats for
each garment copy. A fixture held between `join` and `reinforce` therefore prevents starting more
unfinished garments than the declared inventory supports.

Using the same three-copy setup table above, a qualification covering one fixture per garment gives:

| Available holding fixtures | Selected declared setup time | Maximum occupied fixtures |
|---:|---:|---:|
| 1 | 6.5 s | 1 |
| 2 | 4.0 s | 2 |
| 3 | 1.5 s | 3 |

Feed remains 24 seconds. These are independent exhaustive software checks over synthetic process
observations. The resource constraint changes the sequence, rather than merely annotating an
otherwise unconstrained result. Loading, unloading and handling are included only by the independent
handling policy below; a shared needle/thread identity does not establish zero handling time.

Run the complete two-fixture example, including its actual source locks and explicit synthetic
qualification/estimate permissions:

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-plan \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/sewing-workholding-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-stitches \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/sewing-workholding-options.json
```

The following is the policy shape. Replace the descriptive digest placeholders with the exact
typed machine and qualification content locks, and supply applicable source evidence:

```json
{
  "workholding": {
    "machine_sha256": "FULL_MACHINE_SHA256",
    "qualifications": {
      "join": [{"id": "qualified-join", "sha256": "FULL_JOIN_QUALIFICATION_SHA256"}],
      "reinforce": [{"id": "qualified-reinforce", "sha256": "FULL_REINFORCEMENT_QUALIFICATION_SHA256"}]
    },
    "resources": [{"id": "holding-jig", "capacity": 2}],
    "holds": [{
      "id": "garment-in-jig", "resource": "holding-jig", "units": 1,
      "acquire": "join", "release": "reinforce"
    }],
    "source": {
      "reference": "retained-local-qualification-identity",
      "accessed_on": "2026-10-27",
      "note": "Identify the observed inventory and supported fixture/process combinations"
    },
    "method": "Describe the actual inventory and workholding qualification protocol",
    "conditions": {"configuration": "exact qualified fixture configuration"},
    "evidence": "estimate",
    "allow_estimates": true
  }
}
```

The source policy covers its stated finite inventory and lifecycle applicability. Every original
operation must have an **actually admitted** sewing qualification, and the policy must list its full
content lock. A reference never substitutes for material estimate consent or an operation passing
the ordinary stack, feed, speed, reverse-travel and pressure checks. Several explicitly supported
qualifications may be listed for one operation, so qualified recipe selection can still compare
supported alternatives. Every listed reference must resolve to unchanged source evidence, including
unselected alternatives. Reusing a qualification ID after changing its materials, process envelope,
method or source does not preserve its lock. Changing any part of the machine configuration also
requires a new admitted workholding policy.

Rust callers can construct the locks with
`production::workholding::machine_sha256(&options.machine)` and
`production::workholding::qualification_sha256(&qualification)`. These hash the complete typed value
after JSON object keys are sorted; explicit typed defaults and normalized numeric representations
are included. Hashing the original file bytes is a different operation. The complete source policy
is retained in `plan.options.production.workholding` for exact native and portable replay.

Distinct acquisition/release anchors add a real release-after-acquire predecessor in the expanded
copy graph. Cycles refuse. If both anchors are the same operation, its resources still have to be
available before start; they cannot be reused until that operation completes. Likewise, an operation
cannot borrow resources from a different hold that it will release only at completion.

The preferred baseline finishes each copy in the source plan's topological order. When that order
violates an augmented predecessor or finite resource capacity, the same bounded optimizer searches
for a complete feasible replacement before pruning against an incumbent or taking a zero-cost
optimality shortcut. It backtracks out of fixture deadlocks without changing the qualified resource
capacities or adding invented dependencies. Feasibility discovery and objective improvement share
the existing node cap. The first complete replacement becomes the reported baseline and sets
`production.ordering.baseline_replaced: true`; the field is omitted for an already feasible seed.

Exhausting the budget before finding a complete schedule returns `not_converged`, leaving recipe
selection unresolved. Only exhaustive search without a complete schedule establishes infeasibility
under the declared prerequisite and acquire-before-start/release-after-completion model. Once a
complete incumbent is found, a work limit retains it. Baseline and selected fixture, handling and
stitch histories replay their actual respective orders. Existing lower bounds remain valid because
ignoring resources only relaxes the problem; none of these results establish physical fixture adequacy.

Requested workholding produces `ftx-sewing-plan/13`, `ftx-sewing-production/2` and the nested
`ftx-sewing-workholding/1` report. The report retains each expanded hold, actual qualified operation
locks, resource capacities, and complete baseline/selected histories. Every operation records
occupancy before acquisition, during execution, and after completion-time release, together with
acquired/released hold IDs. Peak occupancy includes holds used by only one operation. The selected
stitch program follows the same constrained sequence; its geometry and continuous-feed calculations
are unchanged. Omitted workholding preserves existing serialization and results.

The bounded policy admits 1–16 resources, 1–64 source holds, at most 1024 expanded holds, and at most
256 qualification references. Each resource capacity and hold demand is an integer in 1–4096.
Fixture geometry, mechanical grip adequacy, seam accessibility and machine execution are not inferred
from these inventory counts. `fixture_geometry_and_access_validated` remains false.

The materials browser shows each resource's capacity and baseline/selected peak, followed by the
selected operation timeline with occupancy before acquisition, during execution and after release.
The same display is available in direct stitch-program results and manufacturing/scenario
comparisons. Expand the source detail to inspect the retained policy, machine/qualification locks
and both complete timelines. All values come from the Rust report; edits withdraw stale results.

## Handling changes the production schedule

Optional `production.handling` adds explicit initial loading, directed between-operation handling
and final unloading to the actual optimizer. Use `production.ordering.objective:
"minimize_production_time"`, the ordinary complete setup-time table and an explicit initial setup.
Handling and the production-time objective require each other. Feed remains determined by the
qualified operation geometry and rate; the variable search objective is setup plus handling.

The model is `serial_operation_transitions/1`: the source protocol must identify nonoverlapping
feed, setup and handling phases whose durations are added. It cannot express simultaneous worker
activity, parallel machines, queueing, unobserved breakdowns or probabilistic durations. Do not put
needle/thread setup or continuous stitch feed into the handling rows a second time.

`initial_milliseconds` and `final_milliseconds` are maps from every original operation ID to its
explicit loading/unloading duration. Each `transitions` row supplies `from`, `to`, `relation` and
`milliseconds`. The relation is `same_copy` or `different_copy`; switching between garments must
remain explicit even if the needle, thread and original operation are identical. Same-copy
self-transitions cannot occur and refuse. Every other pair in the expanded operation domain needs
a row, including pairs excluded by prerequisites. This conservative coverage check prevents an
unmeasured transfer from becoming zero. Unused rows are allowed for a reusable source table.

Every duration is an integer from 0 through 86400000 milliseconds. Zero must be explicit. There
are at most 63 original operations, 8192 transition rows and 256 qualification references; the
existing 63 expanded-operation and 200000 search-node limits still apply. Integer accumulated
durations use u64 on native and WASM, including the complete final unload. The search lower bound
includes a valid minimum possible final unloading duration, so an expensive final operation can
change which operation is scheduled last.

The policy carries the same full `machine_sha256` and per-operation `qualifications` content locks
as workholding, plus independent `source`, `method`, `conditions`, `evidence` and `allow_estimates`.
Use the existing `production::workholding::machine_sha256` and `qualification_sha256` functions to
construct these typed locks. Every actual admitted sewing qualification must be covered. A machine,
material/process qualification or source revision invalidates its old applicability. Missing
transition evidence or uncovered admitted qualifications yield `NotConverged`; recipe search retains
these as unresolved trials instead of claiming the unmeasured recipe is infeasible.

For the synthetic three-copy, two-pass job, specify 100 ms first loading, 100 ms for the same-copy
join-to-reinforcement transition, 5000 ms for changing garments, and 200 ms final unloading. Minimizing
setup alone groups garments and uses 1.5 seconds of setup. Minimizing setup plus handling instead
finishes each garment in turn: 6.5 seconds of setup plus 10.6 seconds of handling plus 24 seconds of
feed gives **41.1 modeled seconds**. With only a 2000 ms inter-copy duration, a mixed schedule can win;
the optimizer evaluates these costs jointly with finite workholding and all ordinary seam/access
requirements. Independent exhaustive software enumeration checks capacities one, two and three.
These timings are synthetic test inputs, not observed manufacturing performance.

Requested handling produces plan `/15`, production `/3` and `ftx-sewing-handling/1`. The complete
policy remains in ordinary replay options. `production.handling.baseline` and `.selected` retain
every scoped transition plus initial/final steps and exact handling totals. The emitted stitch
program follows that same selected order. The ordinary `ordering.timing` still reports the actual
setup total and feed-plus-setup total of that order; joint optimality does not establish that setup
alone is minimal. Its setup-only lower bound is conservatively zero in combined mode.

`ordering.handling_timing` reports separate baseline/selected handling milliseconds,
baseline/selected modeled production seconds, and the combined lower bound
`lower_bound_setup_and_handling_milliseconds`. Add the unchanged feed duration to that bound when
comparing complete modeled time. `SewPlan::modeled_production_seconds()` returns the selected full
model only when qualified handling is present. Budget exhaustion retains complete handling,
resource and stitch replay for the incumbent while leaving optimality unproved. Omission preserves
existing wire output and timing meanings.

For recipe selection, use `ftx-sewing-selection/4` and
`objective: "minimize_modeled_production_seconds"`. Every complete recipe runs the same joint
scheduler, and the selector uses its actual modeled total. Missing timing or an incomplete inner
search prevents an optimum claim. In the three-copy synthetic case, a slower common-thread recipe
loses the feed-plus-setup comparison but wins with handling at 38.0 modeled seconds. Template and
alternative process/source qualifications remain mandatory. Source timing and software feasibility
do not establish empirical seam quality or authorize machine execution.

## Concurrent independent machine cells

Optional `production.parallel` assigns **whole garments** to source-qualified physical cells. Every
cell is an explicitly qualified instance of the same complete admitted machine configuration. It
has its own initial setup, setup-time table, handling observations, availability time and optional
fixture inventory. A garment stays in its selected cell for all operations; this model contains no
inter-cell transfers. The source must explicitly establish independent machines, fixture units and
handling labor. One shared operator cannot be counted as simultaneously loading two cells.

Run the complete source-locked synthetic example, with two independent jig inventories and two
independently declared handling workers:

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-plan \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/sewing-parallel-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-stitches \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/sewing-parallel-options.json
```

The policy requires these exact declarations:

```json
{
  "model": "independent_whole_garment_cells/1",
  "objective": "minimize_makespan",
  "time_basis": "ceil_cell_feed_milliseconds/1",
  "independence": "independent_machines_fixtures_and_handling_labor/1",
  "max_search_nodes": 10000,
  "machine_sha256": "FULL_TYPED_CONFIGURATION_HASH",
  "qualifications": {"join": [{"id": "qualified-join", "sha256": "FULL_QUALIFICATION_HASH"}]},
  "cells": [{
    "id": "physical-cell-a",
    "available_at_milliseconds": 0,
    "ordering": "COMPLETE_EXISTING_PRODUCTION_TIME_ORDERING_POLICY",
    "handling": "COMPLETE_EXISTING_SOURCE_QUALIFIED_HANDLING_POLICY",
    "workholding": "OPTIONAL_INDEPENDENT_FIXTURE_POLICY"
  }],
  "source": "COMPLETE_SOURCE_OBJECT",
  "method": "Observed qualification of these exact configurations and independent resources",
  "conditions": {"resources": "Identify the distinct machine, fixture units and handling labor"},
  "evidence": "estimate",
  "allow_estimates": true
}
```

The descriptive placeholders above stand for the ordinary complete typed objects, not literal
accepted strings. Every original operation needs its actual admitted qualification content lock.
Each cell's `ordering.objective` is `minimize_production_time`; its `handling` and optional
`workholding` use the existing schemas above. These inputs are independently admitted on every
cell/copy-count subproblem, including cells that are idle in the final allocation. Missing evidence
is an unresolved result, never a zero duration or proof that the cell is infeasible.

The planner solves each cell's serial schedule for every possible positive assigned copy count,
using the existing scheduler and all its seam, access, setup, handling and fixture authorities.
An exact dynamic program then selects copy counts summing to the full batch, minimizing the maximum
used-cell finish. A second dynamic program uses valid inner-schedule lower bounds. It can prove the
selected makespan only if the feasible assignment attains that global bound. An exhausted inner
search still supplies a complete feasible schedule and its lower bound; it does not manufacture a
global optimum. The complete baseline assigns all garments to the first lexicographic cell.

The explicit time convention rounds **total generated feed upward once per used cell** to an
integer millisecond, then adds that cell's exact integer setup, handling and availability durations.
Raw continuous feed and the less-than-one-millisecond padding remain in the report. The proved
optimum is conditional on this declared millisecond planning convention, not a sub-millisecond
physical timing claim. An idle cell incurs no setup, loading, final unloading or availability delay.

For two independent copies of the synthetic cell in `sewing-handling-options.json`, three garments
split into two plus one. The single-cell baseline finishes at 41.1 modeled seconds and the selected
two-cell job at **25.5 modeled seconds**. Aggregate feed remains 24 seconds, selected setup work is
5.5 seconds and handling work is 5.9 seconds. Their sum, 35.4 seconds of work across two cells, is
distinct from the 25.5-second elapsed span. These are synthetic arithmetic checks, not measured
factory throughput or a claim that an actual operator or fixture can perform concurrent work.

There are at most eight cells and 63 total copy/operation pairs. The complete batch must also fit
the ordinary 250000 expanded stitch-interval limit before any cell/count search begins, so an
accepted parallel plan cannot exceed the ordinary stitch program's output bound.
`parallel.max_search_nodes` is one global cap covering every nested serial search, reconstruction
of the returned schedules and both complete allocation dynamic programs. Allocation work is
reserved first; the remaining cap covers all cell/count trials, one baseline replay and up to
`min(cells, copies)` selected-cell replays, also respecting each cell's own ordering cap. The outer
serial-reference cap plus the parallel cap must total at most 200000.
`SewOptions::scheduling_work_bound()` reserves that complete configured amount for recipe and
material cross products; increasing inherited production quantity cannot silently evade the
reservation. Too-small budgets refuse before inner work rather than omit counts.

The count cache holds fixed-size timing, bound and search receipts plus a streamed digest of each
ordinary schedule. Full operation and fixture histories are retained only for the returned baseline
and selected allocation. Reconstruction repeats the ordinary solver at the exact recorded node cap
and verifies the complete receipt and schedule digest; a mismatch returns `NotConverged`. Reported
actual search work includes these reconstructions. Every inner trial must establish a complete
supported baseline; an unsupported baseline refuses the requested parallel result without proving
all allocations infeasible.

Parallel output uses plan `/16`, production `/4` and nested `ftx-sewing-parallel/1`. The outer
production `ordering`, `handling` and `workholding` remain **single-machine reference reports**.
`SewPlan::ordering_report()` and `modeled_production_seconds()` therefore return no serial selected
result for a parallel job. Use `parallel_makespan_seconds()` and `parallel_optimality_proven()` for
the active parallel objective, and `parallel.selected` for aggregate actual work.

Each baseline/selected cell retains its complete ordinary serial schedule. IDs in those nested
operation, dependency, fixture and handling records are cell-local; `cell.copies[local_copy]` maps
every record to its global physical garment. This map is shared by every nested history, not only
the operation table. Stitch program `/5` emits a physical `cell` and global `copy` on every interval,
grouped by cell and that cell's selected order. The file is a collection of independent streams,
not one chronological needle stream across machines. Geometry, feed, source identities and actual
traversal counts retain their ordinary meanings. Omitted parallel policy preserves earlier output.

The existing recipe selector accepts `ftx-sewing-selection/5` with
`objective: "minimize_parallel_makespan"`. It evaluates every complete qualified recipe with this
same cell allocator and compares elapsed completion. If an inner allocation is unproved, its
complete recipe remains usable but unresolved for a recipe-optimum claim. Old serial time objectives
refuse parallel options rather than score the retained serial reference. Feed-only and complete
thread-order objectives retain their batch meanings.

Material and design studies expose `sewing_parallel_makespan_seconds` as a nominal objective or
inclusive constraint. The same metric is available to model-backed single-case and named-case
scenario comparisons. The complete cell resource/source policy qualifies comparisons, and absent
evidence cannot produce a zero score. Browser direct sewing and study views show original versus
selected completion, aggregate work, physical copy assignments, each cell's local order, fixture
occupancy and handling. The full source-bearing allocations remain downloadable, and changed
inputs withdraw obsolete results.

## Different machine configurations and actual cell recipes

Version 2 of the existing parallel policy, `independent_whole_garment_cells/2`, supports a different
fully qualified machine configuration in each independent cell. Every cell supplies a
`configuration` with a complete `machine`, its typed `machine_sha256`, ordinary full process
`qualifications`, and a `qualification_refs` map locking the qualifications admitted for every
operation. Version 1 retains its original shared-configuration behavior and serialized output.

The optional `configuration.recipes` map uses the existing recipe model, keyed by original
operation ID: `id`, `needle`, `thread`, `max_stitch_mm` and `stitches_per_minute`. Explicit overrides
remain fixed. Omitted operations inherit the current parent template or recipe-search candidate.
The actual pattern, seam topology, operation dependencies, assemblies, paths, pressure requests,
backtacks and source material assignments remain unchanged. A machine-specific optional `access`
map supplies replacement source-qualified access policies for existing operations. Omission retains
the original clearance requirement; changing a machine never silently removes it.

Each cell builds a **fresh ordinary sewing plan** on the original pattern before allocation.
The planner regenerates its actual stitch stations, traversal counts and feed durations, and reruns
machine, material-stack, process, reverse-feed, pressure and requested access checks. Matching a
machine name is insufficient: full configuration and qualification locks must match. Every cell
is admitted, including one left idle by the selected allocation. Missing or changed applicability
refuses the requested job rather than becoming a zero-cost cell or unproved ineligibility.

The existing copy-count allocation and serial schedule optimizer then use those actual per-cell
plans. No equivalent speed, stitch pitch or needle/thread suitability is inferred across machines.
The complete ordinary cell plans are retained once in `parallel.cell_plans`; per-count schedules
reuse their cell-local seam indices. The outer plan and serial ordering remain a reference.
`SewPlan::parallel_cell_plan(cell)` retrieves the source actually used by a cell, and the active
`total_feed_seconds()` and `total_stitch_intervals()` sum selected physical cell schedules.
Stitch program `/6` resolves each `operation` index in the identified cell's own source plan, so
coordinates, advances, rate, lock travel and global copy identity all come from the selected recipe.

Thread inventory follows the **selected cell allocation**, including every actual traversal and
per-operation/channel allowance. Contributions and missing-coverage records identify their cell.
The same exact yarn's demand is summed across all used cells before applying one supplier minimum,
increment, tier, stock check and complete order. The largest complete operation/channel demand
still controls required continuous length; whole batch length is not mistaken for a continuous
piece. Idle cells contribute no demand. Missing coverage in any used cell withholds complete
quantity, mass and purchase totals, while retaining its partial diagnostics. Purchasing preflight
reserves the union of yarn identities from all supplied cell qualifications, including unused ones.
This uses the original canonical price engine and does not reserve or purchase inventory.

Run the complete source-locked example:

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-plan \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/sewing-heterogeneous-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-stitches \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/sewing-heterogeneous-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-cell-svg \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/sewing-heterogeneous-options.json cell-b join NEW-cell-b-join.svg
```

`sew-cell-svg` is also available through `ftx materials` and the file-free native/browser
`sew_cell_svg(pattern_json, options_json, cell, operation)` API. It rebuilds the entire job before
rendering the explicitly named cell's actual station spacing, recipe rate and feed duration.
The SVG retains its exact machine and qualification content locks, cell identity and selected
global copy IDs. An admitted idle cell can be reviewed with an empty copy list. Plain `sew-svg`
refuses configured-cell jobs until a cell is named, so the outer serial reference cannot be
mistaken for actual selected-cell geometry. Shared-configuration and ordinary previews retain
their existing output. All SVG files use create-new semantics and remain planning views in
independent source-pattern frames.

The synthetic example keeps the two-pass garment and changes the second cell to an explicitly
declared higher-rate configuration with a 5 mm recipe at 900 stitches/minute. Its separate source
observations govern the changed setup, handling and consumption. The selected three-garment job
assigns one copy to cell A and two to cell B:

| Quantity | Selected result |
|---|---:|
| Modeled elapsed completion | 9.9 s |
| Cell B completion, with explicit feed ceiling | 7.334 s |
| Actual stitch intervals | 160 |
| Aggregate continuous feed | 13.333333333333332 s |
| Aggregate setup work | 2.0 s |
| Aggregate handling work | 1.9 s |
| Same-yarn requirement across both cells | 3.0 m |
| Consumed inventory mass at the retained nominal 24 tex | 0.000072 kg |
| One minimum/increment-rounded order | 4.0 m |
| Complete observed synthetic order cost | USD 14.00 |

The retained Coats Epic yarn density is an existing source observation. The cell configurations,
seam/process envelopes, thread-consumption coefficients, handling times and offer are explicitly
synthetic examples. Their arithmetic establishes neither measured seam quality nor factory throughput.
Pricing each cell separately would miss the combined 4 m price tier and incorrectly report USD 20.

Configured output uses plan `/17`, production `/5`, parallel `/2`, consumables `/3` and stitch `/6`.
Complete frozen replay preserves each source plan, the selected allocation and its combined order.
The existing makespan objective remains the allocation authority: actual feed, inventory and cost
are conditional outputs of that assignment. Heterogeneous recipe searches therefore require
`minimize_parallel_makespan`; they do not claim that its selected allocation also minimizes feed or
thread cost. Joint time/cost allocation remains a separate future objective.

The same eight-cell, 63-expanded-operation and aggregate 200000 scheduling-work bounds apply.
The retained outer and all configured cell source plans additionally share a 250000-station bound.
Before count search, every cell's all-copy possibility must also fit the ordinary 250000-interval
stitch-program bound; this conservatively bounds every mixed allocation. Material/design studies
reserve source operations across the outer plan, configured cells, variants and recipe trials,
and count every retained source station table, including idle cells, once toward their aggregate
limits. Scheduling replays and complete-order purchasing retain their own separate work reservations.
Every garment still stays in one cell, with independently qualified machines, fixture units and
handling labor. Shared labor requires the separately qualified optional protocol below. Inter-cell
garment transfer, hardware control and empirical seam qualification remain outside this model.

## Finite cell availability windows

Either shared-configuration or separately configured cells can now supply
`available_until_milliseconds` alongside `available_at_milliseconds`. Both instants use the same
job origin. The end is inclusive: a used cell must finish its initial setup/loading, complete
generated feed, every setup and handling transition, and **final unloading** by that instant.
The complete parallel policy's retained source must qualify the declared window for that physical
cell and its independently available labor/fixtures. A date, shift label or assumed opening time
does not supply a duration. Omission preserves the previous open-ended behavior.

For example, in `examples/sewing-heterogeneous-options.json`, adding these two fields to the
existing respective cells changes the actual manufacturing decision:

```json
{
  "cell-a": {"available_until_milliseconds": 9899},
  "cell-b": {"available_until_milliseconds": 11100}
}
```

These keyed objects illustrate which fields to add; the actual policy keeps its existing `cells`
array and full machine/process/source qualifications. The artificial slow cell needs 9900 ms for
even one garment including unloading, so the 9899 ms window admits none. All three garments move
to the faster configuration and complete at exactly 11100 ms. Its actual recipes produce 120 stitch
intervals and 2.4 m of qualified yarn demand, changing the previous 160 intervals and 3 m. The same
one-order minimum/increment rules still yield a synthetic USD 14 order. These values describe the
software example and its supplied observations, not a measured factory shift.

The report uses `ftx-sewing-parallel/3`. Every baseline/selected cell retains the supplied end, and
`availability.cells[].counts[]` exposes the complete cached incumbent finish, valid lower bound,
feasibility and proved-infeasibility status for every positive copy count. The same two bounded
allocation passes operate on actual feasible finishes and valid lower bounds respectively. A
window that excludes a slow incumbent must not exclude a potentially feasible shorter schedule
from the lower-bound pass. Thus an incomplete search cannot manufacture infeasibility or a proof
that its current assignment is optimal.

If the previous all-copies-on-first-cell baseline exceeds its window, the planner returns the
complete selected feasible allocation as the baseline too and sets `availability.baseline_replaced`.
Both reported assignments satisfy the windows; the out-of-window count remains visible in the
bounded admission receipts. Reconstruction still consumes the existing shared work reservation.
If no complete incumbent fits, but the lower-bound allocation still permits one, the planner
returns `not_converged` and identifies the unresolved search. If even the full allocation over
valid lower bounds cannot fit, it definitively refuses the requested quantity and windows.

An end equal to the start represents a cell with no usable interval and leaves it idle. An end
before the start refuses even if that cell would have been unused. Ends above the exact JSON
integer limit 9007199254740991 refuse. The explicit once-per-cell millisecond feed ceiling remains
part of the planning convention, and the final unload cannot be hidden behind a boundary rounding.

The ordinary stitch program, selected thread inventory, material studies, both native commands
and portable/browser APIs all consume this same constrained assignment and frozen replay options.
No calendar lookup, reservation, source update or hardware execution is implied. Shared labor requires
the additional protocol below; machine breaks/preemption and garment transfers remain unsupported.

## One operator tending autonomous-feed cells

Optional `production.parallel.shared_operator` supports a qualified person tending several machines.
The machines and fixture inventories remain independent, and every garment stays in one cell.
The source must explicitly qualify unattended feed, safe waiting, the actual handling/setup protocol
and the operator's travel. Existing independent-cell mode keeps its original inputs and output.

Shared mode requires these two declarations on the enclosing parallel policy:

```json
{
  "time_basis": "ceil_operation_feed_milliseconds/1",
  "independence": "independent_machines_and_fixtures_shared_operator/1"
}
```

The new operator policy has this shape, alongside the existing complete cell policies:

```json
{
  "model": "exclusive_setup_handling_autonomous_feed/1",
  "operator": "qualified-person-identity",
  "available_at_milliseconds": 0,
  "available_until_milliseconds": 3600000,
  "cells": {"cell-a": "FULL_CELL_SOURCE_HASH", "cell-b": "FULL_CELL_SOURCE_HASH"},
  "initial_arrival_milliseconds": {"cell-a": 100, "cell-b": 500},
  "transfers": [
    {"from": "cell-a", "to": "cell-b", "milliseconds": 400},
    {"from": "cell-b", "to": "cell-a", "milliseconds": 450}
  ],
  "max_search_nodes": 20000,
  "source": {"reference": "RETAINED_OBSERVATION", "accessed_on": "YYYY-MM-DD", "note": "EXACT_APPLICABILITY"},
  "method": "QUALIFIED_PHASE_TRAVEL_AND_UNATTENDED_FEED_PROTOCOL",
  "evidence": "measured"
}
```

The example numbers illustrate the input fields and are not physical observations. Actual inputs
need the complete source and their own durations. Estimated applicability or travel additionally
requires the operator policy's separate `allow_estimates: true`; material/process or setup consent
does not supply it. The optional operator end is inclusive and omission leaves the shift open ended.
All cell ends still apply. Every possible initial destination and every directed pair of different
cells needs explicit travel, including a zero if zero is qualified. Travel is additional to the
existing handling/setup timings and must be measured with compatible phase boundaries. Staying at
the same cell adds no inter-cell travel.

Use `production::parallel::operator::cell_sha256(&cell, &options)` to compute each content lock. It
covers the full physical cell policy, availability, setup/handling tables, fixture policy and actual
effective inherited/overridden recipes, access and process qualifications. Global production quantity,
allocation and purchasing are excluded. All supplied cells are checked even when idle; changing a
cell's recipe, timing or availability requires renewed qualification and its new lock. A hash alone
does not establish source applicability or qualify safe autonomous machinery.

For each operation the person travels, waits if necessary, handles the garment, changes the setup,
then releases the cell to autonomous feed. Handling and setup form one nonpreemptive attended block.
The person may travel to another cell during that feed. Final unloading is an explicit attended
operation after the last feed; both the cell and person stay reserved until it ends. Waiting at a
chosen destination occupies the person, so the search also considers different dispatch orders.
Existing fixture lifecycles remain active through any added waits in their cell.

Shared mode rounds **each complete operation's feed upward independently** to integer milliseconds.
The cell and all copy-count lower bounds use the sum of those operation ceilings. Padding before an
intervening setup, handling interval or wait cannot release later positive feed early: two 0.2 ms feeds
separated by attended work each receive a full 1 ms slot. Raw feed and total numerical padding remain
separate; padding can exceed 1 ms per cell. Independent-labor mode retains its prior once-per-cell
rounding and unchanged serialization.

The planner searches whole-garment allocations and all dispatch interleavings of the existing
cell/count serial incumbents. It retains only compact integer phase durations and operation indices
for these count trials; full source geometry and fixture histories are not duplicated. Seeds,
allocation-prefix extensions and dispatch extensions consume the operator's reserved portion of the
enclosing parallel work limit, alongside all ordinary count solves, lower-bound DPs and exact returned
schedule replays. The operator cap must at least cover every operation plus final unloading of a
complete first-cell seed. The combined serial-reference and parallel limit remains 200000 units.

`ftx-sewing-parallel/4` includes `shared_operator.baseline` and `.selected` timelines with departure,
arrival, handling start, setup start, operator release and feed completion, plus actual global garment
identities. Travel, busy time and waiting are separate from aggregate machine feed and elapsed
makespan. If the first-cell seed fails a cell or operator limit, the complete selected feasible
assignment replaces it and `shared_operator.baseline_replaced` is true. Existing cell/count
`availability` receipts are explicitly a **cell-only relaxation**: a count that fits without
operator contention need not fit the combined timeline.

The proof scope is `retained_serial_incumbents/1`. Completing that restricted search does not
establish the unrestricted joint optimum, because another serial operation order may interleave
better. Independent complete DPs supply valid lower bounds for cell completion and total attended
work. The parent `optimality_proven` is true only when the actual feasible makespan meets their valid
global bound. A missing incumbent is unresolved unless one of those independent bounds proves the
windows impossible; unfinished or restricted search never proves unexamined operation orders fail.

Synthetic one-operation tests give two four-second feeds with 1100 ms initial loading/setup and
200 ms final unloading. Independent workers finish two cells in 5300 ms; one operator finishes in
6400 ms while attending them exclusively. Independent exhaustive allocation/dispatch checks add
asymmetric arrivals, directed travel, delayed cell starts and inclusive limits. Actual heterogeneous
recipes, generated stitch streams, combined yarn orders and bound-material studies use the selected
operator-constrained allocation through both native commands and the portable/browser interface.
The page displays these exact timelines and withdraws them when inputs change. These checks establish
the declared software model; physical seam quality, safe unattended equipment and hardware execution
still require their own evidence and authorization.

## Shared fixture and tool pools across cells

Optional `production.parallel.shared_workholding` lets independently qualified cells draw from one
finite fixture inventory while the existing shared operator handles their garments. This changes the
actual allocation and dispatch search: a cell cannot start handling until its requested pool units
are available. It retains the existing whole-garment cell assignment and each cell's admitted serial
operation order.

Use `independence: "independent_machines_shared_operator_and_workholding_pools/1"` together with
`shared_operator` and `time_basis: "ceil_operation_feed_milliseconds/1"`. The pool policy has this shape:

```json
{
  "model": "shared_workholding_pools/1",
  "release_protocol": "available_at_feed_completion_with_access_in_cell_handling/1",
  "cells": {"cell-a": "FULL_CELL_SOURCE_HASH", "cell-b": "FULL_CELL_SOURCE_HASH"},
  "resources": [{"id": "common-jig-pool", "capacity": 1}],
  "bindings": [
    {"cell": "cell-a", "resource": "local-jig-a", "pool": "common-jig-pool"},
    {"cell": "cell-b", "resource": "local-jig-b", "pool": "common-jig-pool"}
  ],
  "source": {"reference": "RETAINED_OBSERVATION", "accessed_on": "YYYY-MM-DD", "note": "EXACT_APPLICABILITY"},
  "method": "QUALIFIED_COMMON_POOL_ACCESS_AND_RETURN_WITHIN_CELL_HANDLING",
  "evidence": "measured"
}
```

The names and capacity above illustrate the schema, not an observed factory inventory. Every binding
names an existing resource in that cell's source-qualified `workholding` policy. Its existing per-copy
holds remain authoritative, including integer units and acquire/release operation identities. Unbound
local resources remain independent. A local resource can draw from only one pool, and the pool policy
must lock every declared cell through `operator::cell_sha256`, including idle cells. Recipe, material,
timing, fixture or availability changes invalidate those locks. Estimated inventory or applicability
requires separate `shared_workholding.allow_estimates: true` consent.

The release protocol must be independently supported by the source: pool access and return are
included in the existing cell handling times, and units become available when the declared release
operation's feed completes. No extra transfer duration is guessed. Acquisition occurs before handling
starts and continues through setup, feed, intervening operations and waits. Release and another
acquisition may occur at the same instant. The search waits for in-flight releases when possible and
rejects dispatch branches blocked by holds that have not reached their release operation. Operator
and cell windows still include all waiting and final unloading.

Parallel report `/5` includes `shared_workholding` with the exact source locks, capacities, bindings,
and complete baseline/selected reservation histories. Each hold records its physical cell, global
garment copy, local operation identities, units, acquisition time and release time. An independent
interval sweep verifies capacities, processes equal-time releases before acquisitions, and requires
zero final occupancy. The workbench displays these histories in sewing, manufacturing and scenario
results; ordinary report and options downloads preserve exact replay.

The search still considers retained serial incumbents. Shared pools can make an allocation wait or
fail, but they do not turn the cell-only timing lower bounds into a proof about shared-resource
feasibility. A restricted search with no complete result remains unresolved unless an independent
bound proves impossibility. Pool accounting does not establish physical access, safe unattended
operation, garment transfers between cells or hardware permission.

## Route one garment through specialist cells

Optional `production.routing` lets each garment visit several qualified machines: for example,
join at one cell, reinforce at another, then return to the first for finishing. Every copy follows
the same explicitly supplied total route. Each original operation appears exactly once and every
authored predecessor must occur earlier. The scheduler can interleave copies and reuse installed
setups, while each specialist needs process qualification only for the operations it actually owns.

The following structural excerpt shows the routing fields inside `production`. Its durations are
illustrative. A runnable request must also provide the source-bearing `cells`, `cell_locks`, routing
source and transfer source described below; this excerpt supplies no factory qualification or hashes.

```json
{
  "production_copies": 2,
  "ordering": {"objective": "minimize_setup_changes", "max_search_nodes": 1},
  "routing": {
    "model": "fixed_operation_route/1",
    "objective": "minimize_makespan",
    "time_basis": "ceil_operation_feed_milliseconds/1",
    "protocol": "unloaded_between_operations/1",
    "independence": "independent_attended_cells_and_transfer_resource/1",
    "max_search_nodes": 10000,
    "staging_capacity": 2,
    "route": [
      {"operation": "join", "cell": "cell-a"},
      {"operation": "reinforce", "cell": "cell-b"},
      {"operation": "finish", "cell": "cell-a"}
    ],
    "transfer": {
      "id": "handoff-worker",
      "model": "serialized_unloaded_handoffs/1",
      "available_at_milliseconds": 0,
      "transfers": [
        {"from": "cell-a", "to": "cell-b", "milliseconds": 700},
        {"from": "cell-b", "to": "cell-a", "milliseconds": 700}
      ]
    }
  }
}
```

`cells` uses the existing configured-cell shape: unique `id`, availability, required
`configuration`, timed `ordering`, and `handling`. Each configuration supplies its complete
machine and `machine_sha256`, process qualifications, `qualification_refs`, and optional fixed
recipe/access overrides. Overrides and qualification locks may name only operations assigned to
that cell. Each cell's setup table must admit all of its actual directed setup transitions and its
initial setup. The existing handling model `serial_operation_transitions/1` supplies explicit
`initial_milliseconds` and `final_milliseconds` for **every owned operation**, with an empty
`transitions` list: routing applies a complete load and unload each time, including same-cell
successors. Source locks and estimate consent retain their ordinary independent admission rules.

Populate `cell_locks` as a map from each physical cell ID to
`ftx_export::sewing::production::parallel::operator::cell_sha256(cell, &options)`. This hashes the
complete typed cell and effective template, including recipes and timing. Recompute it from actual
inputs; neither a configuration name nor a reused qualification ID protects changed source content.
Both routing and transfer policies additionally require their own `source`, `method`, optional
`conditions`, and `evidence`; estimated applicability needs that policy's own `allow_estimates`.

The outer `machine` and qualification fields are retained template metadata in routing mode.
No all-capable serial reference machine is required. The legacy required `production.ordering`
field must be untimed count metadata, as in the excerpt, without initial setup or timing tables;
it does not control routed dispatch. Routing refuses an outer one-garment ordering, production
handling/workholding, or parallel allocation. Source geometry is built separately for each
specialist on the unchanged original pattern. All passes together must still cover both full
seam sides, including partial operations split across cells, and preserve all original dependencies.

Every cell has its own independently qualified attending worker. Loading, setup, feed and unloading
occupy that cell and worker consecutively; **feed does not release the worker**. Unloading releases
the cell while the garment waits in staging or moves elsewhere. Needle/thread setup persists until
the cell's next operation. All copies start in the first cell's unloaded staging, and the declared
`staging_capacity` applies at **every used cell** and must be at least the complete copy count.

One separately qualified resource performs all inter-cell handoffs without overlap. Each directed
row includes the complete positioning, pickup, movement and deposit work and return to its qualified
ready state before another handoff. Its duration is additional to source unloading and destination
loading; neither empty travel nor carrying time is inferred from a cell name. Supply exactly the
directed edges actually used by the route. Same-cell successors need no handoff. Cell and transfer
availability may have `available_until_milliseconds`; these inclusive limits cover complete
unloading and complete handoffs. A cell can finish one garment while another waits for the mover.

Optional cell `workholding` policies now admit fixtures confined to one complete operation, with
separately qualified common pools as described below. Existing per-operation planar access checks
still apply. Retained fixtures across operations, moving an occupied fixture, sharing a cell worker,
autonomous feed and insufficient staging require separately qualified extensions. Explicit
eligible-machine domains use the model below and retain the selected cell's fixture obligations.

The bounded search appends a copy's next handoff and cell operation at their earliest admitted
times. It considers copy dispatch permutations for the fixed route; it does not enumerate arbitrary
prestaging and later cell reordering. The baseline executes each copy's route in copy order, while
independent cell clocks still permit overlap. A source-window failure of that seed allows a complete
feasible searched replacement. A work limit retains a complete incumbent; if none has been found,
the result is `not_converged`, not proof that the factory job is impossible.

`search_complete` concerns `coupled_handoff_cell_append_dispatch/1`. `optimality_proven` is stronger:
the selected completion must equal an independent lower bound from mandatory cell work, relaxed
directed setup entries, garment paths and transfer-resource work. Restricted search exhaustion alone
does not prove an unrestricted optimum. Up to 63 expanded operations and eight used cells are
admitted. `max_search_nodes` reserves at least a complete seed; two full baseline/selected replays
are reserved separately, and search plus replay cannot exceed 200000 work units.

Plan `/18`, production `/6`, routing `/1`, and stitch program `/7` retain the actual specialist
geometry, exact global garment IDs, per-operation phase boundaries and serialized handoffs. The
outer ordering report carries only dispatch identities and summed per-cell setup changes; it has
no serial timing or optimization proof. `ordering_report()` and `ordering_policy()` therefore
return no serial authority. Existing `parallel_makespan_seconds()` and
`sewing_parallel_makespan_seconds` consumers use actual routed completion. Thread requirements
repeat only each specialist's assigned operations, then combine exact yarn demands before one
ordinary whole-order purchasing evaluation. Explicit-cell previews use that cell's actual passes.

Both native command families, portable interfaces, recipe/material studies and the workbench use
these ordinary sewing inputs. Browser views show the route, actual cell recipes, complete operation
and transfer times, sources and exact reports. Synthetic `sewing_routing` regressions specify
specialist-only qualification, reentry, source changes, seam coverage, shared handoffs, inclusive
windows, bounded incumbents, actual stitches, purchasing, selection and study replay. The focused
Rust regressions pass. The separate real-command workflow and current compiled-WASM
acceptance remain distinct checks. No measured factory timing, physical seam quality or hardware
permission is established.

### Finite fixtures during routed operations

Each routed cell may supply the existing `workholding` policy with its full machine content lock,
actual process/material qualification references, finite `resources`, per-copy `holds`, source,
method, conditions, evidence and separate estimate consent. Qualification coverage names exactly
that specialist's owned operations. Every hold must have the same `acquire` and `release` operation:
the unloaded routing protocol does not carry an occupied fixture to another operation or cell.
All holds on one operation accumulate; their combined demand must fit the declared local capacity.
The cell's exclusive execution then prevents those local reservations from overlapping between copies.
Matching resource names at different cells remain independent inventories.

To share an actual finite pool, add `routing.shared_workholding` using the existing
`shared_workholding_pools/1` shape and change routing `independence` to
`independent_attended_cells_shared_workholding_and_transfer_resource/1`. For example, this
structural excerpt binds two independently qualified local resources to one explicitly observed pool:

```json
{
  "model": "shared_workholding_pools/1",
  "release_protocol": "available_after_unload_with_access_in_cell_handling/1",
  "cells": {"cell-a": "FULL_CELL_LOCK", "cell-b": "FULL_CELL_LOCK"},
  "resources": [{"id": "common-jigs", "capacity": 1}],
  "bindings": [
    {"cell": "cell-a", "resource": "jig", "pool": "common-jigs"},
    {"cell": "cell-b", "resource": "jig", "pool": "common-jigs"}
  ]
}
```

The complete request must also provide the pool policy's independent `source`, `method`,
`conditions`, `evidence` and, for estimated applicability, `allow_estimates: true`. Its `cells` map
must cover every exact cell lock, including machine, recipe, process qualification, fixture policy,
handling and availability. Each binding names an existing cell-local resource and one declared
pool; a resource can draw from only one pool. Unbound local resources remain independent. Multiple
resources bound to the same pool contribute their full summed demand. A material/process or cell
change cannot reuse an old pool lock merely because its IDs are unchanged.

The release protocol explicitly qualifies access/acquisition inside the existing loading phase,
and complete return/release inside unloading. A reservation covers **loading, setup, feed and the
entire unloading phase**. It cannot release at feed completion or borrow from a hold that has not
finished unloading. Equal-time release may supply the next acquisition. No fixture travel duration
is guessed: its access and return must already be covered by the independent source qualification
and the locked cell handling observations. Garment handoffs keep their own separately qualified
resource and durations.

The actual dispatch search checks complete time intervals against all retained reservations.
This matters because dispatch construction order is not chronological: a later-dispatched cell
operation can start before an earlier-dispatched operation on another cell. A clear start instant
is insufficient if the operation would collide with a future reservation. Search finds the earliest
complete admissible interval, retains complete feasible incumbents under its existing shared work
cap, and replaces a copy-order seed when resource delays cause it to miss a window. Failure to find
an incumbent stays `not_converged`, scoped to the bounded fixed-route dispatch model.

Requested local or shared fixtures produce plan `/20`, production `/8`, routing `/3`, and nested
`ftx-sewing-routed-workholding/1`; the stitch payload remains `/7`. The nested report retains local
source qualifications/capacities/lifecycles, the exact optional pool policy and both complete hold
timelines. Separate event sweeps verify local and shared capacities, simultaneous release ordering,
peaks and zero final occupancy. Every hold identifies its actual cell, global garment copy, source
resource, optional pool and exact operation. Existing consumers use constrained routed completion,
while emitted stitches, source geometry and thread demand keep their ordinary authorities. Omitted
fixture policies preserve previous schemas and output.

In the synthetic two-copy specialist regression, one common jig changes selected completion from
12,600 to 14,700 milliseconds and the copy-order baseline becomes 16,100 milliseconds. Two supplied
units recover 12,600 milliseconds. These are exact software examples, not measured factory timing.
Direct regressions also cover full unloading, future reservations, accumulated/multiple pool demands,
independent local names, source invalidation, deadlines, bounded incumbents, stitch replay and material
study substitution. Total routed fixture inputs are bounded at 64 source holds, 1024 expanded holds
and 256 qualification references; each existing cell policy retains its own resource limits and
common pools retain the existing 16-pool/128-binding bound. Grip adequacy, physical pool access,
seam quality and machine execution authorization remain separate requirements.

## Choose eligible cells separately for each garment operation

`eligible_operation_route/1` retains the authored operation sequence but lets each physical copy
choose among independently qualified cells at each step. A bottleneck operation can therefore be
split between machines without requiring either cell to make the complete garment. This differs
from comparing several fully authored routes: eligible assignment is performed inside each admitted
policy, and the existing route-selection search can compare those complete policies separately.

Within an otherwise complete source-qualified routing request, replace the model and route fields:

```json
{
  "model": "eligible_operation_route/1",
  "route": [
    {"operation": "join", "cell": "cell-a", "eligible_cells": ["cell-a", "cell-b"]},
    {"operation": "reinforce", "cell": "cell-b", "eligible_cells": ["cell-b"]}
  ]
}
```

`cell` is the explicit seed assignment, not a preferred-machine cost or inferred eligibility.
Every nonempty `eligible_cells` list must contain that seed, contain no duplicates and name only
configured cells. Every configured cell must occur in at least one domain, although the selected
schedule may leave it idle. The fixed model rejects eligible lists and retains its prior reports.
The complete policy source must qualify the eligible domain, independently attending workers and
staging protocol. The modeled job begins with each garment already in its chosen first cell's
unloaded staging; transport into that initial distribution is outside this schedule. Subsequent
inter-cell moves always occupy the separately qualified handoff resource.

Each cell admits every operation for which it is eligible against the original pattern and its
own machine, recipe, material/process and access evidence. Loading/unloading observations and
setup transitions cover that cell's complete eligible subset. Recompute the ordinary full cell
locks after any configuration, recipe, timing or availability change. Merely leaving an alternative
unused cannot admit a stale source. Supply exactly every directed cross-cell pair between adjacent
eligible domains; staying at the same cell requires no transfer. Missing transfer evidence refuses
the complete declared domain rather than silently removing an inconvenient choice.

The search visits the seed cell first and other eligible cells in authored order for each next
copy operation. Its actual preceding cell determines the handoff, and each selected cell supplies
its own feed duration, setup, loading, unloading and inclusive operating window. An infeasible seed
can be replaced by a complete feasible mixed assignment. The existing search-and-replay budget,
63 expanded-operation limit and eight-cell limit apply. All eligible source plans share the station
reservation, and the largest eligible stitch count at each stage bounds the complete output before
search. A work limit never publishes a partially manufactured garment.

Eligible reports without fixtures use plan `/19`, production `/7`, routing `/2` and the existing
stitch `/7`. Adding local or shared fixtures uses plan `/20`, production `/8` and routing `/3`.
`routing.reference_scope: "seed_route_one_garment/1"` explicitly identifies the outer `seams`, feed
and stitch totals as one seed-route reference garment. **Actual batch totals and stitches come from
the selected timeline.** Each eligible `production.operations[]` row includes its actual `cell`;
its `seam_index`, qualification, stitch count and feed describe that cell's source operation.
The ordinary `total_*` methods and elapsed-completion consumer use the selected batch. Explicit-cell
previews identify precisely which global copies use that operation at that cell.

Thread channels and their start/end allowances are multiplied by the actual selected
`(cell, operation)` execution count. An admitted but unused recipe contributes no demand and does
not require consumption evidence for that unexecuted operation. Exact yarn totals aggregate across
all selected cells before one ordinary supplier order; the largest single operation/channel still
sets the continuous-length requirement. Comparisons retain only executed recipe qualifications,
while their protocol identity also locks the entire eligible routing policy: changing an unused
alternative or search cap must not silently preserve the same decision protocol.

Because cell assignment minimizes makespan, recipe/route selection with an eligible policy requires
`minimize_parallel_makespan`. Feed and yarn cost remain actual consequences of the selected schedule;
this inner search does not optimize them. The policy cannot establish a feed-minimum or thread-cost-
minimum result merely because one retained assignment has attractive values.

The relaxed lower bound combines minimum eligible operation work, unavoidable transfers, total cell
capacity and work forced onto a single cell. Optional setups and unused late-opening cells do not
become fictitious mandatory work. `search_complete` refers only to
`coupled_eligible_handoff_cell_append_dispatch/1`; `optimality_proven` additionally requires equality
with the independent lower bound. Arbitrary prestaging and later cell reordering are not enumerated.
The workbench shows every eligible recipe, its seed flag and actual selected copy count, including
zero-use alternatives, alongside the complete selected operation and transfer timeline.

Independent synthetic regressions enumerate small assignment/dispatch combinations and check mixed
window feasibility, actual geometry, yarn demand, complete-incumbent limits, source invalidation and
protocol identity. They establish this bounded software model, not measured factory throughput,
physical seam quality, cross-operation retained fixtures or hardware authorization. Optional local
and shared inventories follow the selected cell for every actual operation as described above.

## Bounds and remaining scope

The expanded graph supports at most **63 copy/operation pairs** and 1–200000 examined search nodes.
For example, three copies with eight source operations use 24 pairs. Existing one-garment ordering
keeps its 20-operation limit. A search limit returns a complete feasible incumbent and a sound lower
bound with `budget_exhausted: true` and `optimality_proven: false`; it never returns a partial garment
or proves unvisited schedules infeasible. Full search proves only the declared count/time objective
under the declared predecessor graph and source table.

Serial mode covers a single declared machine/configuration; qualified parallel mode assigns whole
garments to explicitly independent instances, with separately admitted configurations in version 2.
Those modes retain needle/thread
transitions, optional source-qualified finite workholding and explicit serial operation/copy handling
inside each used cell. Routing instead admits the specialist operation, staging and handoff
protocol above, with fixed assignments or explicit eligible-cell domains. None infers additional
machines, undeclared inventory, grip adequacy, three-dimensional machine approach,
acceleration or measured seam quality. Planar seam access requires its own independently qualified
policy. These other physical constraints remain open under `ftx-pb2`/`ftx-sp2`; hardware
authorization remains false.

The focused `sewing_production` tests cover shared setups, directed timing, complete incumbents,
quantity conflicts, unchanged legacy output, source-material refusal, actual repeated stitch travel,
allocation limits and 63-operation arithmetic. Workholding cases additionally compare constrained
count/time optima to independent exhaustive enumeration, verify finite integer demands, lifecycle
precedence, complete bounded incumbents, source invalidation and exact stitch replay. Existing
`sewing` and `sewing_program` tests preserve the original numerical and geometric behavior.
