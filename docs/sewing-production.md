# Sewing production across garment copies

Optional `SewOptions.production` schedules the actual operations of several garment copies on one
declared machine. Each copy retains its own seam dependencies; compatible operations from different
copies can share an installed needle/thread setup. The existing bounded ordering solver minimizes
either identity changes or explicitly sourced, directed setup durations.

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

## Bounds and remaining scope

The expanded graph supports at most **63 copy/operation pairs** and 1–200000 examined search nodes.
For example, three copies with eight source operations use 24 pairs. Existing one-garment ordering
keeps its 20-operation limit. A search limit returns a complete feasible incumbent and a sound lower
bound with `budget_exhausted: true` and `optimality_proven: false`; it never returns a partial garment
or proves unvisited schedules infeasible. Full search proves only the declared count/time objective
under the declared predecessor graph and source table.

This slice covers a single declared machine/configuration and needle/thread transitions. It does
not infer additional machines, fixture/tool inventory, workholding capacity, physical seam access,
inter-copy transfers, handling, acceleration or measured seam quality. Those require their own
qualified constraints and remain open under `ftx-pb2`/`ftx-sp2`. Hardware authorization remains false.

The focused `sewing_production` tests cover shared setups, directed timing, complete incumbents,
quantity conflicts, unchanged legacy output, source-material refusal, actual repeated stitch travel,
allocation limits and 63-operation arithmetic. Existing `sewing` and `sewing_program` tests preserve
the original numerical and geometric behavior.
