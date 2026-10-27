# Sewing production across garment copies

Optional `SewOptions.production` schedules the actual operations of several garment copies on one
declared machine. Each copy retains its own seam dependencies; compatible operations from different
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

## Bounds and remaining scope

The expanded graph supports at most **63 copy/operation pairs** and 1–200000 examined search nodes.
For example, three copies with eight source operations use 24 pairs. Existing one-garment ordering
keeps its 20-operation limit. A search limit returns a complete feasible incumbent and a sound lower
bound with `budget_exhausted: true` and `optimality_proven: false`; it never returns a partial garment
or proves unvisited schedules infeasible. Full search proves only the declared count/time objective
under the declared predecessor graph and source table.

This slice covers a single declared machine/configuration, needle/thread transitions, optional
source-qualified finite workholding and explicit serial operation/copy handling. It does not infer
additional machines, undeclared inventory, grip adequacy, three-dimensional machine approach,
acceleration or measured seam quality. Planar seam access requires its own independently qualified
policy. These other physical constraints remain open under `ftx-pb2`/`ftx-sp2`; hardware
authorization remains false.

The focused `sewing_production` tests cover shared setups, directed timing, complete incumbents,
quantity conflicts, unchanged legacy output, source-material refusal, actual repeated stitch travel,
allocation limits and 63-operation arithmetic. Workholding cases additionally compare constrained
count/time optima to independent exhaustive enumeration, verify finite integer demands, lifecycle
precedence, complete bounded incumbents, source invalidation and exact stitch replay. Existing
`sewing` and `sewing_program` tests preserve the original numerical and geometric behavior.
