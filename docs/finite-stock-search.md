# Reconsidering finite-stock placements

Add `"backtrack_limit": 100` to the existing finite-stock `options` object to let
the planner reconsider an earlier accepted placement when a later piece cannot
fit. Omission retains the original deterministic first-fit algorithm and its
serialized request. The explicit limit must be between 1 and 100,000.

This addresses two different first-fit failures: moving a cutter elsewhere on
the same irregular hide, and moving it to a different matching stock item so a
later cutter can use the only sheet that accommodates it. No new stock is created
and no requested garment copy or source piece is dropped to obtain success.

## Search and correctness

Pieces retain their existing order: required-region pieces first, then decreasing
exterior area and deterministic instance/copy tie breaks. Search visits stocks in
input order; within each stock it visits the bounded bottom-left lattice in the
forward direction followed by an explicitly permitted material-specific half-turn.
After a dead end it removes the preceding placement and continues from that
placement's next candidate. Deeper choices restart against the revised inventory.

All candidates use the **same full-polygon acceptance checks** as first-fit:
external boundary, required source-bearing region, defects, prior consumed cuts,
existing placements, and their separate clearance policies. No bounding-box fit,
new reflection, crossgrain rotation, guessed kerf or weakened material match is
introduced. Internal cuts, drills, grain lines and instance transformations travel
with the exact candidate. Named regions do not grant grain-reversal permission.

Without an `optimize` objective, the result is the first complete checked layout found in this search order, **not
a minimum-waste marker or global optimum**. Exhausting the declared lattice is not
proof that no physical placement exists. Refining `step_mm` changes the candidate
set; even this backtracking search can miss a layout outside that set.

## Budgets, rollback and replay

`backtrack_limit` counts undone accepted placement decisions, not physical cuts.
The existing `work_limit` remains one monotonically consumed allowance for stock
validation, all candidate checks, undo-related retries and geometry preparation.
Backtracking does not refund it. Without optimization, exceeding either limit returns `BudgetExceeded`
without a partial marker, residual inventory or replacement SVG. With optimization, exhaustion before
the first complete layout still refuses; after a complete layout exists the best incumbent is retained
with an explicit budget-limited status, as described below.

The iterative search does not use one recursive call per piece. Undo removes only
the corresponding trial placement and restores the saved area sum exactly, rather
than accumulating subtraction error. Original pattern/request inputs are immutable.
A successful report adds `backtracks_used`; this field is absent for legacy first-fit
requests. Existing `work_used` and `candidates` include failed trials.

Frozen requests retain the explicit search policy. Residual requests include only
the selected complete layout's occupied regions, never abandoned trial cuts. They
retain physical stock identity, exact material observations, source-region geometry
and requirements. This is a planning receipt, not proof that cutting has occurred,
an external reservation, or permission to mutate a live inventory database.

## Existing interfaces

No new command or alternate planner is needed. Both `ftx materials` and
`ftx-materials` accept the option through `stock-plan`, `stock-svg` and
`stock-residual`. File-free `stock_plan`, `stock_svg`, `stock_residual`, the existing
material workbench's stock operation and material studies' `finite_stock` input
use the same implementation. A study search refusal does not erase its independent
material-binding or other consumer results. Keep raw Rust JSON for frozen replay.

The synthetic `finite_stock_search` regressions include an L-shaped hide and a
two-sheet inventory where first-fit refuses but reconsideration produces complete
placements. They also check region/grain restrictions, exact rollback, replay,
legacy omission and shared budgets. Native CLI/WASM-boundary regressions compare
the resulting plans, previews and residuals directly with the library. These are
software geometry checks, not real-hide quality or physical machine qualification.

## Minimize physical stock use

An explicit `optimize` field continues the same search past the first feasible layout:

```json
"options": {
  "copies": 1,
  "gap_mm": 2,
  "boundary_margin_mm": 2,
  "step_mm": 20,
  "work_limit": 2000000,
  "backtrack_limit": 10000,
  "optimize": "minimize_gross_area"
}
```

Choose `minimize_stock_count` for the number of distinct physical items receiving new cuts, or
`minimize_gross_area` for their whole original polygon areas, each counted once. Gross area includes
defects and previously consumed regions; this objective is **not** newly usable area, recoverable
offcut value, purchased area, mass, or monetary cost. Existing occupied cuts alone do not count an
item as newly used. The two objectives can disagree. There is no implicit weighting or secondary
objective: an equally good layout leaves the earlier complete incumbent in place.

Optimization requires an explicit `backtrack_limit`. Omitted `optimize` preserves the previous
request serialization and first-complete behavior. No stock, margin, material match, region or
grain permission is changed to obtain a lower objective. The search prunes only when the stock use
of a partial placement cannot strictly improve the incumbent; it never substitutes that bound for
the full polygon checks. Only complete feasible layouts can become incumbents.

The optional `optimization` result retains `objective`, `baseline` (the first complete layout),
`selected`, `complete_layouts`, `improvements`, `termination`, `lattice_optimal` and
`budget_exhausted`. `termination` is `lattice_exhausted`, `work_limit` or `backtrack_limit`.
`lattice_optimal: true` means no strictly better result remains on **this exact candidate lattice,
orientation set and declared constraints**. It is not continuous-space minimality or an empirical
cutting guarantee; the plan's global `optimality_claim` and `hardware_execution_authorized` stay false.

If work or undo allowance runs out after a feasible layout exists, the result restores that complete
incumbent, sets `budget_exhausted: true`, and withholds lattice optimality. Snapshot copying is also
charged to the shared work allowance. A budget exhausted while copying a newly improved complete
layout returns that layout directly, without an uncharged backup copy. Neither partial search paths
nor abandoned cuts appear in previews or residual requests. Other invalid-input or numerical errors
are not converted into successful optimization. Studies consume the selected complete result and
retain this status; requiring `finite_stock` asserts feasibility, not optimizer completion. Comparing
budget-limited results does not prove the optimum material choice.

## Executable synthetic tradeoff

```sh
cargo run -p ftx-cli --bin ftx-materials -- stock-plan \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  tests/fixtures/materials/synthetic-stock-optimization.json
```

The miniature fixture offers a 70 x 30 mm sheet first, then two 25 x 25 mm remnants. The unchanged
front and pocket use one large item under first-fit (0.002100 m²). Area optimization uses the two
smaller items (0.001250 m²); minimizing item count instead retains the larger sheet. These are
synthetic geometry answers, not measured hides, prices, a savings forecast or a benchmark.

## One browser job, one search

The file-free `stock_job(pattern_json, request_json)` entry point builds the plan once and returns
`ftx-stock-job/1`. Its `plan_json` and `residual_json` are original Rust JSON strings, and
`stock_index` identifies the first allocated item. `preview_svg` or `preview_refusal` reports preview
generation independently. A preview-only refusal does not erase a successful layout or next-stock
request. Inputs, each JSON product and the final escaped job response are bounded to 64 MiB; no
partial job response is published after serialization-budget failure.

The existing material-workbench stock button uses this entry point, rather than performing three
optimization runs for plan, residual and preview. A separate `stock-optimization.json` download
exposes the retained objective, baseline, selected usage and termination without rescoring in
JavaScript; the complete plan remains the authority for work counters and source-bound replay.
Downloads preserve the raw Rust strings, including receipt-sensitive `1.0` and `-0.0` spellings.
Cancellation/stale-result invalidation remain unchanged. Older bundles retain their existing
single-product workflow; the worker never strips options to make an unsupported request succeed.
Build a matching `web/pkg` to use the single-run path. Existing single-product APIs and both native
command surfaces remain supported.
