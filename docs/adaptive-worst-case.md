# Worst-case adaptive garment design

`design_adaptive.objective: "consumer_worst_case"` is an explicit named-scenario-matrix
alternative to nominating `case_key`. It uses the same selected-body fit, canonical alterations,
real manufacturing and scenario consumers as ordinary adaptive design.

Every effective case policy must declare exactly one identical **nominal metric and direction**.
For `minimize`, discovery minimizes the largest case reading; for `maximize`, it maximizes the
smallest. The original limiting case's reading is retained, including its unit and source
protocol. Authored case order breaks equal-reading ties. No averages, weights, mixed units,
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

Case overrides may add different constraints or required consumers, but must retain that metric
and direction. Every case must pass its own ordinary admission and its source-protocol checks
across trials, even when the limiting case changes. Different cases are not required to have
identical protocol hashes: each is compared with its own first admitted protocol, unless its
original policy explicitly permits mixed protocols. A failed, missing or excluded case cannot
supply a zero or make a candidate eligible.

The search receipt identifies `consumer_guidance.case_aggregation: "worst_case"` and
`best_case_key`. Each admitted trial identifies its `consumer_case_key`; its existing
`consumer_comparison` is the original decision from that case. `consumer_cases` retains all
original case decisions and refusals. The aggregate single `input_protocol_sha256` is null:
per-case protocols remain authoritative rather than inventing a joint protocol.

Manufacturing runs once per probe. Selected complete probe outputs are reused, without rerunning
callbacks or resetting their cumulative budgets. The existing probe-plus-final reservation still
multiplies by the case count. Single-scenario/manufacturing entry points, `case_key`, missing case
policies, mixed metrics/directions and reported-bounds guidance refuse. Existing nominated-case
range guidance is unchanged.

The final ordinary per-case and Pareto comparisons are unchanged. A bounded worst-case incumbent
does not prove that unvisited designs are infeasible, establish a global optimum, or establish
physical garment fit. The regression controls use opposing **synthetic** scenario readings and
real cutting/manufacturing; they do not claim measured textile behavior.
