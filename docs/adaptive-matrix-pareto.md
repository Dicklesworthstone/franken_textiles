# Adaptive tradeoffs across all named cases

`design_adaptive.objective: "consumer_matrix_pareto"` makes every named case's objectives guide
bounded garment alteration search. It does not nominate one case and does not combine quantities
into a weighted score. Retain the authored `material_key`, variables, fit contract, limits and
optional restart points; omit `case_key`.

Use the existing `ftx-design-study/1` study with `ftx-drape-scenario-matrix/1` through
`ftx --json materials scenario-study PATTERN STUDY MATRIX BODY MODEL` or
`ftx-materials scenario-study PATTERN STUDY MATRIX BODY MODEL`. The portable `on_model` API returns
the same complete report. The Material Workbench runs these authored requests through its
existing named-scenario workflow. Its multi-case adaptive view shows every case's effective
policy and original trial readings, alongside the separate evaluated and retained frontiers.
See [the browser workbench guide](browser-material-workbench.md#explore-adaptive-tradeoffs-across-every-named-case).

Each of one to eight named cases needs an explicit effective comparison policy (its override or
the study fallback), with one to sixteen objectives. There must be at least two case/objective
coordinates in total. Cases may use different metrics, directions, and nominal or reported-range
bases. A mass-minimizing case and a heat-transfer-maximizing case therefore preserve their actual
tradeoffs even if neither case has multiple objectives on its own. Source ranges are only admitted
for metrics the ordinary comparator supports; no new uncertainty model is introduced.

## Shared decision authority

Discovery and the final matrix frontier use the same comparison rule. A candidate dominates
another only when it is no worse in every objective of every case and strictly better somewhere.
Each case keeps its own units, inclusive constraints, required consumers, range requirements and
source-protocol permission. Permission to mix protocols in one case never relaxes another case.
A missing, refused or excluded case cannot steer discovery, and no zero fills an absent reading.

All observed nondominated coordinates guide polling, including coordinates whose complete outputs
would exceed `max_designs`. The existing deterministic frontier traversal, explicit restarts,
coordinate cache and global evaluation/manufacturing/scenario budgets remain shared. Retained
results are reused exactly for the final material cross-product; restarting or retaining a result
does not reset an allowance or rerun a callback. A later failed case leaves earlier admitted outputs
available. The original baseline remains separate from discovery.

## Report interpretation

`consumer_guidance.case_aggregation` is `pareto`. `case_order` retains authored order and
`case_policies` maps each case key to its complete effective policy. There is no common `objective`, `objectives`, `basis`,
`case_key`, or synthetic aggregate protocol. Scalar `best_trial_key` and `best_reading` are null.
Each trial's `consumer_cases` holds the unchanged original decisions and refusals. There is no
`consumer_comparison` pretending one case represents the entire matrix.

The existing `pareto_frontier` separates evaluated nondominated trial keys, exact retained outputs,
and capacity-omitted frontier trials. It is conditional on the evaluated coordinates, source
admission and named cases, not an exhaustive continuous frontier. All cases' consumer slots are
reserved before probing; invalid modes, policies, case nominations and aggregate allowances refuse
before callbacks. Single-scenario and manufacturing-only entry points cannot run this mode.

The tests use synthetic bodies, materials and explicit mathematical controls. Successful software
replay establishes neither empirical garment fit nor thermal comfort, source independence,
jointly attainable range endpoints, or global optimality.
