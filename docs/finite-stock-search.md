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

The result is the first complete checked layout found in this search order, **not
a minimum-waste marker or global optimum**. Exhausting the declared lattice is not
proof that no physical placement exists. Refining `step_mm` changes the candidate
set; even this backtracking search can miss a layout outside that set.

## Budgets, rollback and replay

`backtrack_limit` counts undone accepted placement decisions, not physical cuts.
The existing `work_limit` remains one monotonically consumed allowance for stock
validation, all candidate checks, undo-related retries and geometry preparation.
Backtracking does not refund it. Exceeding either limit returns `BudgetExceeded`
without a partial marker, residual inventory or replacement SVG.

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