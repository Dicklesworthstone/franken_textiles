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

The existing baseline finishes each copy in the source plan's topological order. This complete
baseline must satisfy every augmented predecessor and resource capacity **before** search, pruning
or a zero-cost optimality shortcut. If it does not, the current slice returns an explicit
unsupported-baseline refusal with the unresolved `not_converged` code. Recipe selection consequently
retains an unresolved trial rather than claiming no feasible listed recipe or a proved optimum.
The refusal does not prove that all possible source orders are infeasible.
Declare a resource-feasible source order through the ordinary `after` dependencies when needed.
An admitted search always retains a complete resource-feasible incumbent, including at its work
limit. Existing lower bounds remain valid because ignoring resources only relaxes the problem.

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
handling labor. Shared-worker overlap, inter-cell transfer, hardware control and empirical seam
qualification remain outside this model.

## Bounds and remaining scope

The expanded graph supports at most **63 copy/operation pairs** and 1–200000 examined search nodes.
For example, three copies with eight source operations use 24 pairs. Existing one-garment ordering
keeps its 20-operation limit. A search limit returns a complete feasible incumbent and a sound lower
bound with `budget_exhausted: true` and `optimality_proven: false`; it never returns a partial garment
or proves unvisited schedules infeasible. Full search proves only the declared count/time objective
under the declared predecessor graph and source table.

Serial mode covers a single declared machine/configuration; qualified parallel mode assigns whole
garments to explicitly independent instances, with separately admitted configurations in version 2.
Both retain needle/thread
transitions, optional source-qualified finite workholding and explicit serial operation/copy handling
inside each used cell. Neither infers additional machines, undeclared inventory, grip adequacy, three-dimensional machine approach,
acceleration or measured seam quality. Planar seam access requires its own independently qualified
policy. These other physical constraints remain open under `ftx-pb2`/`ftx-sp2`; hardware
authorization remains false.

The focused `sewing_production` tests cover shared setups, directed timing, complete incumbents,
quantity conflicts, unchanged legacy output, source-material refusal, actual repeated stitch travel,
allocation limits and 63-operation arithmetic. Workholding cases additionally compare constrained
count/time optima to independent exhaustive enumeration, verify finite integer demands, lifecycle
precedence, complete bounded incumbents, source invalidation and exact stitch replay. Existing
`sewing` and `sewing_program` tests preserve the original numerical and geometric behavior.
