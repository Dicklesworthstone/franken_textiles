# Worst-case adaptive garment design

`design_adaptive.objective: "consumer_worst_case"` is an explicit named-scenario-matrix
alternative to nominating `case_key`. It uses the same selected-body fit, canonical alterations,
real manufacturing and scenario consumers as ordinary adaptive design.

Every effective case policy must declare exactly one identical **metric, direction and comparison
basis**. For nominal `minimize`, discovery minimizes the largest case reading; for `maximize`, it
maximizes the smallest. The original limiting case's reading is retained, including its unit and
source protocol. Authored case order breaks equal-reading ties. No averages, weights, mixed units,
synthetic uncertainty or substitute readings are introduced.

Use the existing `ftx-design-study/1` request with an existing selected-body `design_fit` contract,
material alternatives and bounded adaptive variables. Set the adaptive objective to
`consumer_worst_case`, retain its exact `material_key`, and omit `case_key`. Supply named scenarios
through the ordinary matrix entry point. For example, every case can inherit this study policy:

```json
{
  "basis": "nominal",
  "objectives": [{"metric": "simulated_cloth_mass_kg", "direction": "minimize"}]
}
```

Case overrides may add different constraints or required consumers, but must retain that metric,
direction and basis. Every case must pass its own ordinary admission and its source-protocol checks
across trials, even when the limiting case changes. Different cases are not required to have
identical protocol hashes: each is compared with its own first admitted protocol, unless its
original policy explicitly permits mixed protocols. A failed, missing or excluded case cannot
supply a zero or make a candidate eligible.

The search receipt identifies `consumer_guidance.case_aggregation: "worst_case"` and
`best_case_key`. Each admitted trial identifies its `consumer_case_key`; its existing
`consumer_comparison` is the original decision from that case. `consumer_cases` retains all
original case decisions and refusals. The aggregate single `input_protocol_sha256` is null:
per-case protocols remain authoritative rather than inventing a joint protocol.

The matrix report's ordinary `notes` also identify the retained trial, limiting case, original
metric value and unit. The existing workbench displays those Rust-authored notes as text; neither
the browser nor the CLI computes a second score. When no trial is admitted in every case, the
note explicitly reports that no limiting case or objective value is available, not a zero. Every
trial's structured per-case results remain inspectable alongside this summary.

## Source-reported ranges across cases

Set every effective policy's `basis` to `reported_bounds` to use the ordinary comparator's supported
assembly-mass, vapor or final finite-water ranges. Unsupported metrics still refuse. Every case
must supply complete admitted objective endpoints. Each constraint uses its explicit basis or
inherits its case's objective basis: bounded guards admit the entire original range, while nominal
guards test the original computed value. A nominal guard cannot relax a bounded objective.
Missing bounds retain the original nominal reading but exclude the trial; they are never filled
with zero-width intervals.

For case intervals `[lo_i, hi_i]`, the conditional worst-case enclosure is:

* `minimize`: `[max(lo_i), max(hi_i)]`, enclosing the largest case result.
* `maximize`: `[min(lo_i), min(hi_i)]`, enclosing the smallest case result.

The nominal limiting case, lower-endpoint case and upper-endpoint case can all differ. For example,
nominal values `8, 7, 5` with ranges `[6, 9], [7, 8], [1, 12]` give a minimizing worst-case nominal
value of `8` and enclosure `[7, 12]`. Selecting only the nominal case's `[6, 9]` would miss the
third case's upper endpoint. Equal endpoint witnesses retain authored case order.

Each admitted range trial adds `consumer_worst_case`, containing the aggregate `reading` and
`reported_bound_case_keys` in lower/upper order. `consumer_comparison` remains the untouched
nominal limiting case's decision; `consumer_cases` retains every original decision, constraint
reading, source protocol and refusal. The guidance `best_reading` contains the aggregate interval
and `best_reported_bound_case_keys` identifies its witnesses. `best_case_key` still names the nominal
limiting case. These additional range fields are absent in nominal mode.

Polling and incumbent replacement require **strict interval separation**, not improvement of
nominal values, midpoints or only the adverse endpoint. Overlapping, equal or touching intervals
remain unresolved. Once retention is full, only a strictly dominated incumbent can be replaced;
the bounded retained set is not a complete Pareto frontier. Original per-case protocol anchoring,
consumer refusals and callback budgets apply unchanged, including to nonlimiting cases.

This monotone enclosure assumes only that every case value stays inside its admitted source range.
It does not assert that all endpoint combinations are jointly attainable for a shared specimen,
assign probabilities, infer source independence, or propagate uncertainty in geometry, body fit,
boundary conditions or the declared scenario set. The final ordinary per-case and Pareto comparisons
remain separate from the discovery envelope.

Manufacturing runs once per probe. Selected complete probe outputs are reused, without rerunning
callbacks or resetting their cumulative budgets. The existing probe-plus-final reservation still
multiplies by the case count. Single-scenario/manufacturing entry points, `case_key`, missing case
policies and mixed metrics/directions/bases refuse. Existing nominated-case
range guidance is unchanged.

The final ordinary per-case and Pareto comparisons are unchanged. A bounded worst-case incumbent
does not prove that unvisited designs are infeasible, establish a global optimum, or establish
physical garment fit. The regression controls use opposing **synthetic** scenario readings and
real cutting/manufacturing; they do not claim measured textile behavior.
