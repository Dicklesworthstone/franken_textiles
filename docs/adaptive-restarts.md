# Bounded multi-start garment design

`design_adaptive.restart_points_mm` optionally supplies up to 31 additional starting points
for the existing adaptive alteration search. Each map must name every declared variable exactly
once, with a finite value inside that variable's inclusive bounds. The variables' `initial_mm`
coordinates remain the first start. Starts must be distinct, including that initial point;
positive and negative zero denote the same coordinate. An omitted or empty list preserves the
existing single-start request serialization and search behavior.

For an existing adaptive request with a `length` variable whose domain includes 40 and 60 mm:

```json
"restart_points_mm": [{"length": 60}, {"length": 40}]
```

For multiple variables, each map must include them all. Map key order is irrelevant; variable
poll order and restart order are exactly the authored orders. Unknown names, partial points,
nonfinite values, out-of-domain values and duplicate starts refuse before consumer execution.
The same canonical linked-site displacement checks apply to restart coordinates.

## When a restart runs

A restart runs only after the current local search stops at `minimum_steps`. It resets polling
steps to the declared `initial_step_mm` values, not the discovered best design. This allows
explicit exploration away from a stalled region, including flat-valid designs rejected by
production constraints. It does not reinterpret a failed constraint as an optimization score.

Every start shares the original discovery state: the global `max_evaluations`, selected-design
limit, coordinate cache, manufacturing work, cumulative scenario budgets, source-protocol anchors
and exact retained consumer outputs. The same coordinates never rerun a consumer within one
request, even when a later authored start was already visited by an earlier poll. Resetting a
polling step is not resetting a budget. Evaluation-budget and fit-only qualified-design-limit
stops terminate the whole search rather than beginning another start.

The ordinary geometry, selected-body fit, source, consumer and per-case constraint gates remain
unchanged. Only fully admitted designs are retained for the final material cross-product. A
previously admitted design remains available after a later restart fails. Nominal comparison,
reported-range strict separation and named/worst-case policies retain their existing meaning.
The separately retained original baseline is not an adaptive discovery incumbent.

## Reports and limits

When restarts are requested, `design_search.search_starts` records each attempted start, including
index 0 for the initial point: its initial and final trial keys, number of new unique evaluations,
stop reason and final polling steps. A cached initial trial may belong to an earlier start. A
final polling centre can be refused; it is not automatically a selected design. All original
trial decisions and refusals remain in `rows`. The sum of per-start evaluations equals the global
unique evaluation count.

The request retains all authored points. Any remaining entries after the last recorded start
were not explored. `final_steps_mm` at the search level describes the last attempted start.
`complete_search` and `global_optimum_claim` remain false: visiting several starting points does
not enumerate a continuous domain or prove that an unvisited feasible region does not exist.

The regression examples use artificial sheet densities and dimensions. They exercise actual
cutting, sewing and assembly-mass consumers, but do not establish physical fit, measured textile
behavior, physiological thresholds or a manufacturing recommendation. This is a bounded software
extension of the open `ftx-ds2` design-search work, not closure of its empirical acceptance gates.
