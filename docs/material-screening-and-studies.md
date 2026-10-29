# Source-qualified material screening and manufacturing/design studies

This is a product path over the existing material authority and production planners, not another
materials database, guessed fiber mixing law or a process checklist. It turns the existing catalog
and custom source-bearing selections into reproducible alternatives, then actually reruns their
manufacturing consequences on the same source pattern. It does not measure textile properties.

## Run the supplied examples

```sh
cargo run -p ftx-cli --bin ftx -- materials screen \
  tests/fixtures/materials/screen-insulation.json --json
cargo run -p ftx-cli --bin ftx-materials -- study \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/study-insulation.json
```

Both `ftx materials screen REQUEST.json` and `ftx-materials screen REQUEST.json` use exactly the
same evaluator. So do `ftx materials study PATTERN.json REQUEST.json` and the companion's `study`.
The browser bindings are `material_screen(request_json)` and
`material_study(pattern_json, request_json)`. The file-free Rust entry points are
`ftx_pattern::material_screen::Request::evaluate`,
`ftx_export::material_study::Request::evaluate`, and `ftx_wasm::materials::{screen,study}`.
Section 4 of `materials.html` runs these same requests and displays source-qualified results.

The examples use the existing source-bearing 3M insulation records and a clearly synthetic pattern.
They compare reported mass/thickness/resistance, not whole-jacket warmth. Some candidate records
have no stock width: the study retains their cutting refusal instead of inventing a roll width.
All eligible candidates reach manufacturing unless `frontier_only` is explicitly true. A material
that loses a scalar-property comparison can still produce a better marker or cheaper whole order.

## Screening contract: `ftx-material-screen/1`

`basis` is required. `nominal` compares reported central values. `reported_bounds` requires actual
reported bounds for EVERY requested constraint/objective; it never converts missing uncertainty to
zero-width intervals. Bounds are reported ranges, not inferred confidence intervals.

A constraint has `attribute`, `minimum` and/or `maximum`, with inclusive endpoints. An attribute
names an existing property `key`, its canonical `unit`, an optional exact `method`, and an optional
required subset of `conditions`. Known properties use `material_catalog::property_unit`: for example
`areal_density` in `kg/m2`, `thickness` in `m`, and `thermal_resistance` in `m2*K/W`. Authored `g/m2`,
`mm` and other supported units are normalized by the existing adapter while original observations
remain in each frozen selection. Extension properties require an exact requested unit match;
there is no dimensional guess, property-name fuzzy match or conversion of breaking force to stiffness.

Objectives specify `minimize` or `maximize`. There is no hidden weighted sum. Under nominal
comparison, A dominates B only when no objective is worse and at least one is strictly better.
Under reported bounds, the entire interval must be no worse: for minimization A.upper <= B.lower;
for maximization A.lower >= B.upper, again with at least one strict inequality. Overlapping intervals
and exact ties remain alternatives. By default, different reported methods or complete observation
conditions make a pair incomparable. `allow_mixed_protocols: true` explicitly relaxes that safeguard;
it does not establish experimental comparability. Exact equality of unspecified protocols is not
physical evidence either. The report carries the policy and original qualifiers for review.

`query` searches identity, construction and composition. `forms` filters physical form. Missing or
excluded records stay in `alternatives` with structured `exclusions`; they do not receive a zero score.
An empty `candidates` array discovers the supplied catalog, admitting estimates only when
`allow_catalog_estimates` is true. Explicit candidates have unique `key` and existing `selection`
objects and retain their own `allow_estimates` policy; a global estimate flag cannot override them.
Successful resolution emits the same frozen `Selection` used by `fabric.material`.

`required_capabilities` invokes actual existing compilers, independently:

- `mechanics`: sheet mechanics plus optional nonlinear membrane, relaxation, directional bending and
  plasticity compilation. A list of nine scalar names is not substituted for the real compiler.
- `steady_conduction`: existing `ConductionLaw`, without requiring heat capacity or areal mass.
- `transient_thermal`: existing `ThermalLaw`, including reference mass, capacity and qualified domains.

Readiness is not successful draping or a valid prediction at an arbitrary strain/temperature. Thermal
curves are not converted to scalar objectives at an unstated temperature. Prices remain dated,
quantity- and market-qualified offers, not a timeless material score.

## Study contract: `ftx-material-study/1`

The study takes ONE original pattern and shared `cutting`, `finite_stock`, `sewing` or
`sewing_selection`, `assembly_mass`, and optional `purchasing` inputs. At least one manufacturing consumer is required; finite-stock-only
and mass-only studies are allowed. Purchasing contains the existing `quotes` book and
`market` scope and requires cutting: required stock comes from actual markers, including waste,
width, margins, minimum orders and sale increments, never just net pattern-piece area.

When separate suppliers must fulfill one material requirement, set
`purchasing.market.split_orders` to
`{"schema":"ftx-cut-order-allocation/1","max_search_nodes":50000}`. The existing bounded
markers remain indivisible: each is assigned once to an exact material-revision-locked offer.
Minimum quantities, increments and observed capacity apply once to each assigned supplier order;
the longest assigned marker determines the continuous-cut check. Each quote's base SKU and all
declared price-break SKUs are treated as aliases of one physical stock; offers from one supplier
sharing any such alias cannot multiply available stock. The limit is shared across material groups and
charges attempted assignments and uncached canonical quote evaluations. The optional path supports
up to 32 markers, 12 per material and eight current locked offers per material; omission preserves
ordinary single-offer whole-material purchasing. No inventory is reserved.

Before evaluating alternatives, a study reserves each requested `max_search_nodes` for every
alternative, including later failures and dormant cutting-job routes. The combined reservation for
ordinary and mixed-job purchasing must not exceed 200,000 work units. Reduce the number of alternatives
or each limit when the request exceeds that bound.

The purchasing result retains `material_orders[].allocation.selected_orders`, including the exact
quote, rounded quantity, cost, original marker indices and evaluated market scope. The actual order
SKU is `offer.applied_price_break.sku` when present, otherwise `offer.quote.sku`; the original quote
and its complete price schedule remain available for replay. A complete
feasible result can survive search exhaustion: `selected_observed_order_total` remains available,
while `least_observed_independent_order_total` requires proven optimality over the admitted offers.
`search_complete`, `budget_exhausted` and `order_allocation_optimality_proven` distinguish these
outcomes. With no complete allocation, both totals remain unknown; no partial purchase becomes a
cheap completed job. The browser displays assignments and both cost meanings separately.

Select exactly one alternative source:

```json
{
  "schema": "ftx-material-study/1",
  "variants": [
    {"key": "shell-A", "changes": {
      "schema": "ftx-material-bindings/1",
      "default": {"id": "EXACT-CATALOG-ID-OR-FROZEN-SELECTION"}
    }}
  ],
  "cutting": {"copies": 1, "gap_mm": 2, "edge_margin_mm": 5, "end_margin_mm": 10}
}
```

Alternatively, supply `screened: {request: SCREEN_REQUEST, target: {scope: "default"},
frontier_only: false}` and omit `variants`. A named-piece target is
`{scope: "pieces", ids: ["front", "back"]}`. Default substitution preserves explicitly assigned
lining/cuff materials. Named-piece substitution changes only those source IDs. It never invents
layer stacks, remaps instance IDs, changes grainlines, rewrites pair friction or changes geometry.
Every candidate begins from the SAME original pattern; substitutions never accumulate across rows.

### Choose qualified sewing recipes for each material

Use `sewing_selection` instead of `sewing` to rerun the existing `ftx-sewing-selection/1` search on
each frozen material alternative. The same choice works with explicit variants, screening,
heterogeneous combinations and model-backed scenario studies. The search needs a complete template,
`require_qualification: true`, exact source-qualified stacks, and explicitly listed needle, thread,
maximum stitch pitch and stitch-rate alternatives. It never invents a recipe from a fiber name.
Machine limits, assemblies, pressure settings, geometry and source-admission policy remain fixed.

Source-qualified independent production cells use `ftx-sewing-selection/5` with
`objective: "minimize_parallel_makespan"`. Each complete recipe reruns the ordinary planner,
every cell's serial scheduling and the whole-garment allocation. The dedicated nominal metric
`sewing_parallel_makespan_seconds` works as an objective or inclusive constraint in manufacturing,
design and scenario studies. It includes availability and final unloading under the explicitly
declared millisecond planning convention; it is distinct from the sum of machine work.
The complete cell-independence, source, timing and fixture policy belongs to the comparison protocol.
An unproved complete incumbent can be compared, while recipe optimality requires all examined
inner allocations to be proved. Missing cell qualification withholds the sewing result and its
objective without erasing independent mass or cutting. Reported-bound makespan is unsupported.

Parallel plans leave the old serial setup/handling/time metrics unavailable, and serial timed
recipe objectives refuse a parallel request. Generated feed, stitch counts and thread orders retain
their complete-batch meanings. Recipe/material work reservations include the entire parallel cap
in addition to the serial reference. See [sewing-production.md](sewing-production.md#concurrent-independent-machine-cells)
and the complete `examples/sewing-parallel-options.json` for input and replay.

The timed `ftx-sewing-selection/2` request is also supported: `selected_objective_seconds` preserves
feed plus qualified setup time, while the study's `feed_seconds` remains the selected plan's feed
time. An admitted timed plan additionally supplies `sewing_setup_seconds` and
`sewing_feed_and_setup_seconds`, whether it came from fixed `sewing` options or recipe selection.
These are the selected schedule's actual setup duration and feed-plus-setup duration from its
source-qualified directed timing table. Both can be Pareto objectives or inclusive constraints:

```json
{
  "basis": "nominal",
  "objectives": [
    {"metric": "sewing_feed_and_setup_seconds", "direction": "minimize"}
  ],
  "constraints": [{"metric": "sewing_setup_seconds", "maximum": 60}],
  "required_consumers": ["sewing"]
}
```

The study must supply `ordering.objective: "minimize_setup_time"`, an explicit initial setup and
an admitted complete `setup_times` table through the shared sewing request. A plan with no timing
table, even one with zero counted setup changes, leaves these duration metrics unknown. A material
alternative can therefore remain sewing-ready while being excluded from a timed comparison. Zero
setup seconds is valid only when the admitted production timing calculation actually reports zero.
The existing search status still distinguishes a complete feasible incumbent from a proved optimum.
These durations exclude handling, acceleration, final reset and other unmodeled work; they are not
total production cycle times or measured material quality.

For an executable **synthetic software example**, embed the existing selector fixture in a study:

```sh
jq -n --slurpfile search tests/fixtures/materials/synthetic-sewing-selection.json '
  {schema: "ftx-material-study/1",
   variants: [{key: "original", changes: {schema: "ftx-material-bindings/1"}}],
   sewing_selection: $search[0],
   comparison: {basis: "nominal", required_consumers: ["sewing"],
     objectives: [{metric: "feed_seconds", direction: "minimize"}]}}
' > /tmp/ftx-sewing-study.json
cargo run -p ftx-cli --bin ftx -- materials study \
  tests/fixtures/materials/synthetic-sewing-pattern.json /tmp/ftx-sewing-study.json --json
```

The example selects `faster-qualified` and reports 2 feed-only seconds. These are invented test
windows, not empirical sewing recommendations. In a real study, supply qualifications for each
exact material stack and candidate recipe. Replacing a material invalidates an old qualification
even when the new material has the same nominal thickness; another admitted candidate may then
be selected automatically. Without one, sewing stays unavailable while valid cutting, purchasing,
mass and simulation outcomes remain visible.

Each row's `sewing.result` is the unmodified selected production plan and supplies the existing
`feed_seconds`, `stitch_intervals`, optional source-qualified timing metrics and required-`sewing`
comparison checks. The separate `sewing_selection.result` retains every trial/refusal, selected
operation-to-recipe choice,
source pattern hash and search status. Geometry is stored only once. An available search with no
incumbent is not an available sewing plan; its missing metrics cannot win as zeroes. A bounded
search may produce a usable complete incumbent without proving it is the best listed recipe.
The study compares that actual plan and exposes the incomplete search, without claiming a global
material/process optimum. The browser displays these statuses alongside the existing sewing result.

Configured work across all material alternatives is capped at 256 complete-plan evaluations,
4096 pass evaluations and 200,000 optional ordering-search nodes, before evaluating alternatives.
The selector's per-job limits and the study's 250,000 retained-station cap also apply. Reduce the
number of material alternatives or `max_plan_evaluations` when this combined budget is exceeded.
Source qualifications still do not establish measured seam quality or authorize hardware execution.

### Thread quantities and dated whole orders

Set `sewing.consumables` (or `sewing_selection.options.consumables`) to `{}` to evaluate the
selected plan's thread demand. Each admitted process qualification must supply its independently
sourced `thread_consumption` model: a complete channel assertion, exact frozen yarn selections,
metres per traversed stitch interval, and separate start/end allowances. See
[the sewing consumption contract](material-sewing-plans.md#source-qualified-thread-inventory-and-purchasing)
for the input fields. Reverse travel, backtacks and reinforcement count every traversal; allowances
apply once per operation and channel. A recipe search reports the consumption of its actual selected
plan, even when a faster recipe requires more thread.

Add `consumables.purchasing` with `quote_book` and the existing purchasing `market` to request
dated orders. Each exact yarn's complete demand is aggregated before the canonical length-price
engine applies supplier minimums, increments, price tiers, availability and requested stock limits.
Missing qualifications, incomplete channel lists or missing eligible offers leave complete quantities
or prices unknown. Known contributions remain inspectable in `sewing.result.consumables`.

Two nominal comparison metrics are available:

| Metric | Required consumer | Meaning |
| --- | --- | --- |
| `thread_required_length_m` | `sewing_consumables` | Complete metres across every selected operation and thread channel, including declared allowances |
| `thread_observed_order_cost` | `sewing_purchasing` | Complete independent whole-yarn orders in the thread market's own currency |

For example, this policy minimizes complete thread cost subject to a declared demand cap:

```json
{
  "basis": "nominal",
  "objectives": [{"metric": "thread_observed_order_cost", "direction": "minimize"}],
  "constraints": [{"metric": "thread_required_length_m", "maximum": 40}],
  "required_consumers": ["sewing_consumables", "sewing_purchasing"]
}
```

Protocol comparison includes the independently reported consumption method, conditions, evidence
and channel identities, plus the methods and evidence of the process qualifications actually used.
Unused candidate models do not change the selected plan's protocol. Reported bounds are not inferred
for these computed quantities or costs.

Thread and cutting costs remain separate, with their own currencies and complete-coverage checks.
The thread inventory uses the same production quantity as every requested cutting route. Omitted
`consumables.production_copies` inherits roll, finite-stock or mixed-job copies; an explicit mismatch
refuses sewing while leaving independent cutting results visible. Requirements aggregate across
garments before whole-order purchasing, while continuous length remains per operation/channel.
The report retains both per-garment contributions and production totals. Neither thread demand nor
purchased cone length establishes retained garment mass or a combined assembly purchase total.
Sewing geometry, feed duration and setup schedules remain per garment. Recipe selection optimizes
its explicitly declared objective, including complete batch thread order cost when requested.
At most 200,000 configured thread-purchasing work units are admitted across all expanded material
and design alternatives and recipe trials. Named scenario cases reuse the manufacturing result.

### Heterogeneous material combinations

`combinations` is a third alternative source, exclusive with `variants` and `screened`. It evaluates
the complete Cartesian product of material choices on independently declared targets. For example,
a shell, lining and cuff can each have their own choices; the ordinary manufacturing and scenario
consumers then recompute every resulting assignment before the existing comparison selects its
Pareto frontier. A material's scalar screening score cannot silently discard a better mixed design.

This complete request works with the supplied synthetic two-piece pattern. Its source-bearing
insulation selections support cutting and mass comparison; no missing mechanics are inferred:

```json
{
  "schema": "ftx-material-study/1",
  "combinations": {
    "groups": [
      {
        "key": "front-insulation",
        "target": {"scope": "pieces", "ids": ["front"]},
        "choices": [
          {"key": "C100", "selection": {"id": "3m_thinsulate_c100_2020"}},
          {"key": "C200", "selection": {"id": "3m_thinsulate_c200_2020"}}
        ]
      },
      {
        "key": "back-insulation",
        "target": {"scope": "pieces", "ids": ["back"]},
        "choices": [
          {"key": "C100", "selection": {"id": "3m_thinsulate_c100_2020"}},
          {"key": "C200", "selection": {"id": "3m_thinsulate_c200_2020"}}
        ]
      }
    ]
  },
  "cutting": {"copies": 1, "gap_mm": 2, "edge_margin_mm": 5, "end_margin_mm": 10},
  "assembly_mass": {
    "schema": "ftx-assembly-mass/1",
    "inventory_complete": true,
    "extras": []
  },
  "comparison": {
    "basis": "nominal",
    "objectives": [
      {"metric": "assembly_mass_kg", "direction": "minimize"},
      {"metric": "stock_area_m2", "direction": "minimize"}
    ]
  }
}
```

There must be one to eight groups, with one to 32 choices per group and at most **32 complete
combinations**. A larger product refuses before resolving selections or running consumers; it is
never truncated. Group keys are unique, choice keys are unique within each group, and each key is
nonempty and at most 256 UTF-8 bytes. Keys identify choices independently of material IDs, so two
specimen lots or explicit overrides may share one material ID. Named-piece targets must be disjoint,
nonempty and refer to existing source IDs. One optional `default` group uses ordinary default
substitution: existing explicitly assigned pieces remain unchanged. A named-piece group deliberately
overrides its target even when a default group is also present.

Groups and choices are evaluated in sorted-key order. Each generated row retains a `choices` map
from group key to selected choice key; its unambiguous row `key` is the JSON serialization of that
sorted map. The usual `frozen_changes` contains complete resolved material records and exact
`expected_sha256` locks, including unchanged effective assignments. It replays through the ordinary
binder and can be used as an explicit manual variant. Missing selections refuse only combinations
that use them; unavailable cutting, sewing, thermal or other consumers remain separate outcomes.

Heterogeneous contact still needs qualified pair evidence. Optional `combinations.interfaces` is a
pool of the **existing** `fabric.interfaces` records, with exact lowercase SHA-256 material hashes, friction observations,
sources and estimate-consent policy. Every pool record is validated even when unused, and duplicate
or same-material pairs refuse. Each combination selects exactly the pool entries whose two hashes
occur among its effective physical piece assignments, then submits that complete selected table to
the normal atomic binder. No pair coefficient or estimate consent is inferred. The resulting table
is retained in that row's `frozen_changes.interfaces`. Omitting the pool preserves the original
authored table; an empty pool explicitly clears it. Missing or stale pair evidence is handled by
the ordinary mechanical consumer, without erasing independent mass/cutting results.

The generated frontier is conditional on these supplied choices, process/scenario inputs and
comparison policy. Exhausting a finite choice set does not establish a global textile-design optimum
or experimentally validated garment fit. Native `study`/`scenario-study` and their existing WASM
entry points consume the same request.

Each row independently reports binding, roll cutting, finite stock, sewing, requested assembly mass
and purchasing as `available`, `refused` or `not_requested`. `available` purchasing can still have
`complete_offer_coverage: false`: its total
then remains null. Missing mechanical parameters do not prevent source-qualified cutting or sewing.
Excessive thickness can refuse sewing while cutting remains available. Missing width refuses cutting
and its dependent pricing but does not prevent sewing. Every result is the existing planner's normal
output, including its provenance and physical/hardware limitations.

Summary metrics distinguish gross stock area, outer cut area, complete stock mass, least observed
independent order total, stitch intervals, feed-only time and separately admitted setup/joint time.
Unknown mass for ANY roll makes total stock mass unknown, not a deceptively smaller partial mass.
Feed time excludes handling, acceleration,
setup and robot motion. There is no aggregate feasibility/quality score or hidden currency conversion.

The binding result contains lossless `pattern_json` suitable for the existing drape, thermal and
export paths. Do not parse/stringify that text in JavaScript before source-locked replay: numerical
representations such as `1.0` and `-0.0` participate in existing receipts. Each successfully bound row
also includes `frozen_changes`; applying them to the SAME original pattern reproduces that binding,
including unchanged explicit assignments. Modified frozen observations fail their source locks.
Inputs are never overwritten. Hardware and physical-validation flags are always false.

## Complete declared cut-assembly mass

`assembly_mass` invokes `ftx_export::assembly_mass::Request::evaluate` on every bound alternative.
It needs neither roll width, explicit cutting grain, mechanical coefficients nor a body. All physical
cut instances are counted, including unsimulated lining, unplaced pieces, mirror pairs and on-fold
unfolding. The actual exporter cutter path retains seam/hem allowances and outward notch tabs.
Strictly contained, disjoint closed internal cut polygons are subtracted once; crossing, nested,
touching or malformed cutouts refuse rather than inventing the removed area. Open slits and drill
marks have no authored removal area, so no kerf or punched-out disk is assumed. Implicit layers
refuse: represent each physical layer as actual cut pieces or select the measured complete laminate,
never both for the same retained material.

For example, add this inventory to an existing study request:

```json
{
  "assembly_mass": {
    "schema": "ftx-assembly-mass/1",
    "inventory_complete": false,
    "extras": [
      {"id": "all-retained-thread", "mass": null},
      {"id": "complete-closure-set", "mass": null}
    ]
  }
}
```

`inventory_complete` and `extras` are required explicit choices. The declaration is the author's
accounting claim, not automatic discovery of buttons, thread, bindings, embroidery, loose fill or
other hidden components. Replace each unknown group with its **total retained mass for ONE
assembly**, using the existing `Quantity` vocabulary: positive `value`, `unit: "kg"`, `evidence`,
`source`, `method`, optional full `conditions` and reported `bounds`. The group also carries a
`sources` map with the referenced `reference`, `accessed_on` and `note`. Estimated group mass requires
that group's `allow_estimates: true`; it cannot authorize estimates in sheet selections. Do not
insert an arbitrary thread allowance or identify purchased packaging as retained garment mass.
Omit a truly absent group rather than supplying a fake zero observation.

Each report retains the request, exact pattern/request identities, per-instance source records,
external/removed/retained areas, individual mass contributions and unavailable reasons. `sheet_mass`
is complete only when every cut piece has admitted source areal density. `assembly_mass` additionally
requires all extras and an explicitly complete inventory. A partial subtotal is never relabeled a
complete total. Successful consumer execution can therefore produce an available report with
`assembly_mass.kg: null`; it is not successful complete-mass admission in the comparator.

Optional `seam_allowance_mm` uses the same edge/piece/export hierarchy as cutting. Mass must match
the effective export allowances of every requested roll-cutting and finite-stock consumer;
disagreement refuses mass but preserves each independent cutting result. Each cutting consumer's
`copies` counts the allocated order, not this **one-assembly** mass. Stock includes waste; this
inventory does not. Simulation omission/scaling never changes it.
Finishing losses, seam grading, washing, coatings and other unmodeled changes remain outside this
pre-finishing cut inventory. The result is not a weighed final-garment prediction.

`reported_bounds_kg` scales and adds input endpoints only when every contribution reports bounds.
Missing bounds stay absent, never zero-width. These ranges hold authored geometry and inventory
fixed; they are not confidence intervals or propagated geometric/measurement uncertainty. Study
comparison accepts `assembly_mass_kg` as an objective or inclusive constraint under explicit
`nominal` or `reported_bounds` policies, and `assembly_mass` as a required consumer. Unknown/incomplete totals exclude a candidate without
erasing its other results. Density methods, conditions and evidence remain part of the existing
protocol comparison; unrelated width observations do not block a mass-only comparison.

The executable example supplies sourced insulation densities and **synthetic geometry and extras**:

```sh
ftx materials study tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/study-assembly-mass.json --json
```

Both native binaries and section 4 of the material workbench use the same request/engine. The browser
shows cut-sheet mass, declared assembly mass, ranges, inventory and per-piece sources separately
from purchased stock and simulated cloth. It displays the Rust total, never sums display numbers.
The synthetic extra mass in this fixture is not a recommended garment allowance or new empirical
textile observation. Maximum work is 20 million polygon operations for a standalone mass call and
40 million shared across all successful/refused study alternatives. The existing 16,384 aggregate
instance limit and an additional 4,096 aggregate extra-group limit apply before evaluation; exhausted
aggregate geometry work refuses the study rather than returning a truncated frontier.

## Finite hides and remnants in the same study

`finite_stock` accepts the existing complete `ftx-finite-stock-request/1` object: physical stock
IDs, exact material selections, real outside polygons, defects, prior occupied cuts, directed axes,
geometry evidence and shared search controls. See `docs/finite-stock-cutting.md` for its schema and
the executable synthetic stock fixture. It can be the only manufacturing consumer, or accompany
roll cutting, sewing and scenario drapes. No roll width is required for a hide/remnant allocation.

Every material alternative starts with the **same original inventory and options**. A row does not
consume inventory for the next alternative. The existing planner still requires an exact material
snapshot and evidence-policy match; selecting a differently characterized lot with the same name
does not rebind the stock to that lot. Unknown stock coverage or an unsuccessful bounded placement
is a `finite_stock` refusal for that row, while its other consumers continue independently. Source
pattern geometry, quantities, mirrors, folds, grainlines, seam allowances and occupied regions keep
their existing manufacturing semantics.

The `finite_stock` outcome contains the unmodified `ftx-finite-stock-plan/1` production result,
including frozen inventory, complete geometry and replay identities. Its summary fields are:

| Metric / comparison name | Meaning |
|---|---|
| `used_stock_count` | Number of individually identified physical items used, each counted once |
| `used_stock_gross_area_m2` | Whole polygon area of those items, including defects and prior occupied cuts |
| `used_stock_mass_kg` | Complete source-qualified mass of those items; unknown when any **used** item lacks density |
| `finite_stock_external_cut_area_m2` | Newly allocated exterior cut area, with internal holes still occupied |

Used-item area and mass describe inventory engaged by a plan, not the newly consumed area, recovered
offcuts or a purchasing requirement. Unused inventory cannot inflate these metrics or erase known
used-stock mass. Roll metrics retain their original meanings and are reported separately when both
consumers are requested. Purchasing still requires the roll `cutting` consumer: no price per hide,
offcut valuation or area-to-hide conversion is inferred.

For example, with `finite_stock` supplied in the study request, this comparison minimizes the number
of hides/remnants needed, retaining inventory area as a separate objective:

```json
{
  "basis": "nominal",
  "objectives": [
    {"metric": "used_stock_count", "direction": "minimize"},
    {"metric": "used_stock_gross_area_m2", "direction": "minimize"}
  ],
  "required_consumers": ["finite_stock"]
}
```

Each alternative must have a complete accepted allocation. A missing required value does not score
zero. Density methods/conditioning/evidence remain part of protocol comparison when mass is an
objective; unrelated roll-width measurements do not block a finite-stock-only comparison. Omitting
`finite_stock` preserves existing request identities and result fields. As with every study, choose
and review one alternative before using its separate stock plan in a later inventory workflow.

## Computed manufacturing objectives

An optional `comparison` in `ftx-material-study/1` computes a Pareto frontier from the **actual
planner outputs**, independently of scalar catalog screening:

```json
{
  "basis": "nominal",
  "objectives": [
    {"metric": "stock_mass_kg", "direction": "minimize"},
    {"metric": "observed_order_cost", "direction": "minimize"}
  ],
  "constraints": [{"metric": "feed_seconds", "maximum": 120}],
  "required_consumers": ["sewing"],
  "allow_mixed_protocols": false
}
```

These objectives require cutting and purchasing in the same study request; the constraint requires
sewing. Other metrics are `stock_area_m2`, `external_cut_area_m2`, `stitch_intervals`,
`feed_seconds`, `sewing_setup_seconds` and `sewing_feed_and_setup_seconds`. Timing objectives use
the selected production plan and cannot turn missing source-qualified setup durations into zeroes.
Dimensions are fixed by their names; order cost uses the exact request currency.
`selected_observed_order_cost` compares the complete feasible purchasing allocation actually
selected, including an incumbent retained at the work limit. `observed_order_cost` requires the
least cost to be proved. Both are conditional on the unchanged marker plan and the supplied dated
offers; neither establishes the cheapest possible nesting or an unlisted supplier's price.
One to sixteen unique objectives, up to 64 constraints and unique required consumers are accepted.
Every requested value must be available and finite, every inclusive constraint must pass, and
required purchasing must have complete offer coverage. An unrequested consumer is an invalid
comparison policy; a requested consumer's refusal excludes only its candidate. Unrelated
consumer failures do not disqualify a candidate unless explicitly required.

The report's `comparison` retains readings, eligibility exclusions, protocol identities,
`dominated_by`, `frontier`, and explicit incomparable pairs. Ties and tradeoffs remain alternatives.
Default pairwise comparison requires equal reported methods, conditions, canonical units and
evidence for relevant width/density/thickness inputs by source piece. Unrelated properties do not
block it. `allow_mixed_protocols` deliberately relaxes this check, never the selection's estimate
policy. Equal protocols are not empirical comparability. Full source observations remain in the
binding and ordinary planner reports; those outputs do not change when comparison is requested.

`basis` is required. `nominal` explicitly compares computed central values and preserves the existing
behavior for every metric. The separate `reported_bounds` mode below supports complete declared
assembly mass and vapor-potential transfer. It does not turn source ranges into intervals for cutting
choices, sewing, cost, mechanical response or dry heat.
This finite-candidate frontier is conditional on upstream screening, process options, quantities,
market date and input data. It is not optimal nesting, a global material optimum or physical validation.

### Assembly-mass limits over the complete reported range

Use `basis: "reported_bounds"` when a mass requirement must hold throughout the source-reported
range, instead of only at its central value. For example, this comparison accepts a maximum of
0.35 kg as an **illustrative authored requirement**, not a measured garment limit:

```json
{
  "basis": "reported_bounds",
  "objectives": [{"metric": "assembly_mass_kg", "direction": "minimize"}],
  "constraints": [{"metric": "assembly_mass_kg", "maximum": 0.35}],
  "required_consumers": ["sewing"],
  "allow_mixed_protocols": false
}
```

The containing study must request `assembly_mass` with complete inventory and source-reported bounds
for every retained sheet density and extra group. This example also requires its existing sewing
request to succeed. Other consumers can be required as usual; their ordinary completion/source gates
do not become uncertain-output predictions. Both native `study` commands, scenario studies and the
material workbench use the same policy and Rust comparison, without a separate solver.

An inclusive maximum checks the upper mass endpoint; an inclusive minimum checks the lower endpoint.
A nominally acceptable candidate whose range crosses either requirement is excluded with its actual
range and reason retained. A missing sheet or extra bound excludes the candidate even when nominal
mass is available. Its central value remains visible, and no zero-width range is invented. The
`readings.assembly_mass_kg.reported_bounds` field contains the exact admitted consumer endpoints in kg.

For minimization, A dominates B only when A's upper endpoint is strictly below B's lower endpoint;
maximization reverses that ordering. Overlapping, touching and equal ranges keep both alternatives
on the frontier with an explicit unresolved-range reason. They cannot be ordered by their central
values. Reported methods, conditions, evidence, exact material binding and complete-inventory gates
still apply; allowing mixed protocols never bypasses missing bounds or a failed required consumer.

`assembly_mass_kg` and the vapor metrics below support this mode. Unsupported metrics refuse the
policy before evaluation, including mixing a nominal cost objective into a reported-bounds
comparison. Use explicit nominal mode when comparing those other computed objectives.
The guarantee is conditional on the reported endpoints, fixed authored geometry and additive
pre-finishing inventory. It does not cover omitted components, finishing changes, geometric error,
source truth or confidence levels. `uncertainty_propagated` and `physical_validation_claim` remain
false; this is the specific mass-endpoint propagation already reported by the mass consumer.

### Vapor-potential limits over reported resistance bounds

The same `reported_bounds` policy admits `vapor_potential_heat_flow_w` and
`vapor_potential_mass_flow_kg_s` from the declared stack. Scenario studies additionally support the
independent-sheet, covered-body and motion-integrated vapor objectives. Every consumed Ret
observation must have complete positive source-reported endpoints; standard deviation or coefficient
of variation is not converted into a range. Missing endpoints or unrepresentable range arithmetic
retain the nominal consumer result but exclude it from a range comparison.

The canonical vapor engine sums resistance endpoints using the fixed areas, films and interfaces,
then reverses the resistance endpoints when computing transfer. Coverage sums complete regional
ranges, and motion sums interval ranges using the declared right-endpoint sampling. Geometry,
boundary pressures, latent heat, material assignments and sample times remain fixed. Repeated
occurrences of a material do not become statistically independent observations.

Limits must contain the entire returned transfer range. Overlapping ranges remain unresolved even
when their nominal transfer rates differ; the browser displays the Rust comparison without ranking
those central values. Current-geometry, full-coverage, complete-motion and source-protocol admission
still apply. These conditional source ranges are not confidence intervals, uncertainty in garment
geometry, water availability or actual evaporative cooling. See
[vapor coverage](vapor-coverage.md#conditional-ranges-from-source-reported-resistance-bounds).

`tests/fixtures/materials/study-compared-insulation.json` is an executable example using the same
synthetic sewing pattern and source-bearing insulation records. It compares gross marker mass and
area, retaining missing-width exclusions; it is not an insulation warmth or safety comparison:

```sh
ftx materials study tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/study-compared-insulation.json --json
```

Scenario studies use the same optional `comparison` at the top of their result, additionally
supporting signed heat, reported edge strain and simulated-cloth mass. See
`docs/material-scenario-studies.md` for mandatory settledness, identity checks and scope limits.

## Bounded garment alterations: `ftx-design-study/1`

Design studies cross explicit width, length, vertex, seam-truing or cap-ease changes with the same
material alternatives and production consumers. Each geometry sequence starts from the original
pattern. Its complete altered document then receives the ordinary material binding; cutting,
finite stock, mixed jobs, purchasing, assembly mass, sewing and optional qualified recipe selection
all rerun on that actual result. A scenario study additionally builds a fresh production drape for
each admitted pair. This makes an authored geometry change affect stock, seams and simulation
together without adding another alteration or cloth solver.

The existing study commands and browser workbench accept the explicit new schema. Run this
**synthetic software example**:

```sh
ftx materials study tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/design-study.json --json
```

The example compares two source-bearing artificial sheet selections across the unaltered baseline,
added width/ease, added length, and one intentionally unbalanced seam: eight retained rows. The two
unbalanced rows fail the canonical validation gate. The six valid rows produce new real cutting and
sewing plans, including the longer seam's extra stitches and each material's different stock mass.
The declared objectives concern mass and continuous-feed time; they do not establish that the
smallest pattern fits a wearer. The supplied dimensions and material properties are test data.

Keep the usual `variants`, `screened` or `combinations` source, and add `designs`:

```json
{
  "schema": "ftx-design-study/1",
  "designs": [{
    "key": "wider-front",
    "operations": [
      {"op": "slash_spread", "piece": "front", "axis": "x", "at_mm": 10, "add_mm": 8}
    ]
  }],
  "variants": [{"key": "original-materials", "changes": {"schema": "ftx-material-bindings/1"}}],
  "cutting": {"copies": 1, "gap_mm": 2, "edge_margin_mm": 0, "end_margin_mm": 0}
}
```

The reserved `baseline` design is always added for **every** requested material alternative. It
preserves the original geometry; retaining the original material assignments is the explicit
empty binding change illustrated above. Geometry candidates use unique nonempty names and ordered
nonempty operation lists. `ftx-material-study/1` rejects a `designs` array and keeps its existing
fixed-geometry result shape.

The workbench's material-group builder accepts either study schema. For a design policy, retain its
`designs` and process/comparison fields and omit `variants`, `screened` and `combinations`; choose the
materials in the page. The builder adds the frozen groups without rewriting the original operations,
numeric source text or policy schema. The shared engine still limits the complete design/material
product, including baseline and invalid designs, to 32 rows.

| Operation | Required fields | Canonical effect |
| --- | --- | --- |
| `slash_spread` | `piece`, `axis`, `at_mm`, `add_mm` | Shift coordinates strictly beyond the cut; `x` changes width and `y` changes length. Negative spreads remove width/length. |
| `move` | `piece`, `vertex`, `dx_mm`, `dy_mm` | Move one drafted vertex; relative curves follow its endpoints. |
| `true_seam` | `seam`, `end` | Explicitly slide the seam's B-side drafted start/end vertex within the existing ±60 mm search to its declared ratio or band midpoint. |
| `cap_ease` | `piece`, `seams`, `target_ratio` | Scale above the existing underarm within the canonical factor range to the declared total cap/armhole ratio. |

Operations preserve the existing alteration semantics and history. Slash-spread rounds transformed
vertices and marks to 0.001 mm. Slash-spread and cap-ease transform grain, internal, decor and trace
mark points; they leave notches, drills and placement anchors unchanged. Move and true-seam change
drafted vertices without relocating marks. Mirrored/folded geometry follows the canonical pattern
expansion, and true-seam refuses unsupported mirrored-side edits. Cap-ease adjusts the aggregate
ratio; each individual seam still has to pass validation. No seam ratio, machine policy or material
property is rewritten to make a candidate succeed.

### Admission, replay and comparisons

Before any binding or consumer evaluation, each design—including the baseline—passes the existing
flat validator's structural checks, seam walk, free-edge truing and cut/grain completeness checks.
The seam tolerance is the shared sewing/recipe-template tolerance when supplied, otherwise 3 mm;
existing per-seam overrides remain authoritative. Free-edge truing uses the canonical 8-degree
tolerance. The retained validation spec has no inferred body-measurement points. An additional
full-boundary simplicity check uses the existing 60-sample-per-curve outline and shared polygon
predicates; it catches positive-area self-intersections that signed area alone would miss. This is
the canonical sampled-geometry scope, not an analytic proof for arbitrary curves.

A refused operator or failed validation preserves the row and its reason, prevents manufacturing
and scenario callbacks for that row, and prevents it from reaching a comparison frontier. Other
design/material pairs continue. Invalid geometry cannot win a mass-only or price-only objective.
Successful binding still does not imply successful sewing, a settled drape or measured wearer fit;
the normal consumer-specific and scenario comparison admission rules remain in force.

Each design row has a `design` receipt containing its original `key` and `material_key`, exact
`operations`, original `source_pattern_sha256`, and complete `altered_pattern_json` with its hash.
`validation` retains the unmodified canonical flat report, outline outcome, exact validation spec
and combined `passed` flag. Altered JSON survives a later validation refusal for inspection; an
operator refusal leaves it null rather than returning a partially applied sequence as a design.
`geometry_changed` distinguishes a real piece/mark change from history-only no-ops. Row keys are
canonical JSON strings encoding `[design_key, material_key]`, so arbitrary quotes and delimiters in
authored names cannot collide. Combination choices retain their usual separate `choices` map.

Replay a successful row by applying `frozen_changes` to its **exact `design.altered_pattern_json`**.
The row's `binding.result.pattern_json` already contains both alteration and material binding and
can go directly to existing cutter, pattern-export, sewing and drape commands. The binding's source
hash identifies the altered document; the outer study and design receipt retain the original hash.
JavaScript consumers should pass these retained strings directly instead of reparsing and
stringifying floating-point source numbers.

Manufacturing reports use `ftx-design-study-report/1`; single-scenario and named-scenario-matrix
reports use `ftx-design-scenario-study/1` and `ftx-design-scenario-matrix-study/1`. The existing native
and portable scenario entry points accept the new study request with their ordinary scenario or
matrix input. Manufacturing runs once per design/material pair, and each named case starts fresh
on that pair's retained bound pattern. The ordinary explicit objectives, constraints, reported-bound
rules and cross-case comparisons apply; no geometry-fit score, weighted winner or continuous/global
optimum is synthesized.

The entire design/material product, including baseline and later failures, is limited to 32 rows.
At most 31 explicit designs, 16 operators per sequence and 256 expanded operator applications are
admitted. Source-document plus authored-operation bytes times the reserved document-pass count must
fit a 256 MiB processing reservation: each row and move/slash-spread charges one pass, while each
true-seam/cap-ease reserves 64 passes for the canonical root search and final document, including
trials that later refuse early. Serialized material-binding payload bytes are also reserved once
per design, including baseline, before expanding large source records across rows. Outline checking shares 40 million work units across
the study. Existing instance, sewing, mass, finite-stock, recipe and supplier budgets charge every
expanded row. A named matrix reserves `expanded_rows × cases` before any alteration, manufacturing
or scenario callback and still obeys its caller-supplied maximum of 64 evaluations. Excess work
refuses the study instead of silently dropping candidates or returning a partial frontier.

## Search declared alterations against the selected body

An opt-in `design_grid` replaces `designs` within `ftx-design-study/1`. It generates linked canonical
slash-and-spread or vertex-move operations from one to three scalar variables. Each variable declares
a unique key, 1–64 strictly increasing finite `values_mm`, and one or more `sites`. A slash site
contains `piece`, `axis`, `at_mm` and a nonzero signed `multiplier`; its `add_mm` is the variable value
times that multiplier. A move site explicitly names `op: "move"`, `piece`, `vertex`, `dx_multiplier`
and `dy_multiplier`; its horizontal and vertical displacements are the same scalar value times
those dimensionless multipliers. At least one move multiplier must be nonzero.

Sites of either kind can be mixed and run in authored order; optional `after` operations run last.
This expresses shared ease, length and local shaping across front/back pieces. Every candidate
still passes the ordinary seam and outline gates; linked edits do not silently repair a seam.

```json
{
  "design_grid": {
    "variables": [{
      "key": "length",
      "values_mm": [20, 30, 40],
      "sites": [
        {"piece": "front", "axis": "y", "at_mm": 50, "multiplier": 1},
        {"piece": "back", "axis": "y", "at_mm": 50, "multiplier": 1}
      ]
    }],
    "after": [],
    "max_designs": 3
  },
  "design_fit": {
    "fit_spec": {
      "schema": "fabric-fit/1",
      "points": [{
        "id": "synthetic-scaled-stature-length",
        "kind": "vertical",
        "instance": "front",
        "from": "a",
        "to": "d",
        "body": "stature",
        "body_scale": 0.1,
        "target_mm": [-1, 0, 1]
      }]
    }
  }
}
```

These coordinates and targets are a synthetic software example, not pattern-making advice.

Local shaping uses the same variable structure in both `design_grid` and `design_adaptive`. For
example, the following linked sites widen only the upper endpoints of the synthetic front/right
and back/left seam while leaving both hems fixed:

```json
{
  "key": "upper-width",
  "values_mm": [-4, 0, 4, 6],
  "sites": [
    {"op": "move", "piece": "front", "vertex": "c", "dx_multiplier": 1, "dy_multiplier": 0},
    {"op": "move", "piece": "back", "vertex": "d", "dx_multiplier": -1, "dy_multiplier": 0}
  ]
}
```

At `+6 mm`, each 100 mm high panel becomes a trapezoid with 20 mm and 26 mm widths, its halfway
width is 23 mm, and both declared sides have length `sqrt(100² + 6²) mm`. These are geometric
consequences of this synthetic pattern, not body-fit measurements. Vertical or diagonal move sites
can similarly vary shoulder, neckline or armhole endpoints when the author supplies the relevant
vertices and explicit targets. For adaptive search, replace `values_mm` with the existing bounds,
initial coordinate and step fields; the site syntax stays the same.

Generated operations retain exact requested `dx_mm` and `dy_mm`; canonical moves round edited
vertices to 0.001 mm. Relative curve controls follow their endpoints, but grain, notches, drills,
internal marks and placement anchors are not moved by this operation. Every trial starts from the
unchanged original document. Unknown vertices, broken seams and invalid outlines remain failed
candidates, with no partially altered pattern admitted downstream. Mixed slash/move fields,
unknown operation tags, no-op multiplier pairs and displacement overflow or underflow refuse.
Complete authored grid values or adaptive endpoints are checked before any truncated search, and
each actual adaptive coordinate is checked again. Existing operation and document-work limits apply.

[The complete length-search request](../examples/design-fit-search.json) uses the existing synthetic sewing
pattern and an explicitly artificial sheet. For an independently selected 1300 mm model, its
100 mm original panel and +20/+30/+40 mm length choices yield 120/130/140 mm; only +30 is in the
declared ±1 mm band around one tenth of that model stature. A different selected body can yield a
different candidate or no feasible candidate. Run it through the ordinary model-backed boundary:

```sh
ftx --json materials scenario-study \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  examples/design-fit-search.json scenario.json body-input.json model.ftxbody
```

Supply an existing compatible scenario and converted model; body inputs may reuse an intact
accepted fit as described in [material-scenario-studies.md](material-scenario-studies.md).
A standalone manufacturing request with `design_fit` has no selected body and explicitly refuses
those rows. Library callers can use `evaluate_with_fit_body`, `evaluate_with_fit` or
`evaluate_matrix_with_fit` with one immutable live `design::fit::Context`.

### One selected body authority

Manufacturing-only fit studies can use that same selected body without running a
cloth or thermal scenario:

```sh
ftx materials study-on-body pattern.json study.json body-input.json model.ftxbody
ftx-materials study-on-body pattern.json study.json body-input.json model.ftxbody
```

The study must declare `design_fit`. `body-input.json` accepts the same
`ftx-scenario-body/1` request (including an intact accepted fit) or legacy
measurements as scenario studies. The model, source request, reference-pose
geometry and MODEL_FIT readings identify every design's fit gate. Cutting and
sewing run only for admitted designs; no drape, heat, strain/pressure or physical
fit result is inferred. Scenario-dependent objectives refuse on this route.

In the workbench, enable **Constrain manufacturing to the selected body**, load
the local model and body-source request (or measurements), and use the ordinary
manufacturing comparison button. The original body input is downloadable beside
the study. Changing any selected body source withdraws this result. With the
option off, the existing body-independent manufacturing path is unchanged.

The native and portable model adapters construct `FittedBody::flat_fit_body` once, only when fit
targets are requested. It reuses the selected shape without refitting and evaluates existing tape
sections and actual model-frame landmarks in the standard 35-degree A-pose. Reference measurements
do not follow the requested drape pose, motion frames or conflicting newly supplied tapes. Available
stature is retained independently when complete tape sections are unavailable. Unknown body fields
and landmarks remain unavailable; no analytic dress-form or acquired-tape fallback replaces model
geometry. Values retain `MODEL_FIT` provenance and no invented propagated sigma.

Each candidate's `design.fit` retains the canonical full flat report, exact target specification and
model/shape/source/reference-geometry/value identities. Source identities are also bound into the
manufacturing, single-scenario and matrix request hashes. Explicit fit requires 1–32 unique points
with finite ordered [minimum, nominal, maximum] ease targets. Existing girth, width, vertical and
path point semantics apply. Canonical `IN_BAND`/report status controls admission, before display
rounding; a LOW/HIGH residual that rounds to zero still fails. Seam, truing, completeness and sampled
outline gates remain in force. A failed or unavailable fit cannot bind materials, invoke a scenario,
produce manufacturing plans or enter a frontier. The retained altered pattern remains inspectable.

The raw `on_body` API has no selected model measurement authority and refuses requested fit; it
does not guess body measurements from arbitrary collision geometry. Invalid accepted-body receipts
remain refusals and cannot fall back to a tape fit. The ordinary no-fit workflows preserve their
existing independent manufacturing/scenario behavior.

### Explicit finite search and its limits

The last variable changes fastest. Generated keys are `grid:0000`, `grid:0001`, etc.; each row
retains its `design.variables_mm`, exact applied operations, altered pattern and frozen material
changes. The unchanged baseline is additional to the required `max_designs` allowance of 1–31.
The existing 32-row design × material cap and case budgets still apply, even to candidates later
refused by fit. Work is reserved before invoking consumers.

A small explicit allowance visits a deterministic prefix of the grid. `design_search` retains
the entire domain and reports `total_grid_points`, `generated_designs`, `omitted_designs`,
`complete_grid_enumeration` and `budget_exhausted`. An incomplete frontier describes only visited
designs; omitted points may contain better feasible designs. Full enumeration establishes coverage
of the declared grid, not solver convergence, empirical fit, a continuous optimum or a global optimum.
No hidden scalar fit score or automatic winner is introduced. Ordinary explicit manufacturing,
drape/motion strain and thermal objectives/constraints still use their existing authorities.

### Find qualified designs beyond the short downstream prefix

An optional `fit_screening` allowance searches a larger authored grid prefix with inexpensive
canonical flat measurements before spending the material/scenario allowance:

```json
{
  "design_grid": {
    "variables": [{
      "key": "linked-width",
      "values_mm": [0, 4, 8, 12],
      "sites": [
        {"piece": "front", "axis": "x", "at_mm": 10, "multiplier": 1},
        {"piece": "back", "axis": "x", "at_mm": 10, "multiplier": 1}
      ]
    }],
    "max_designs": 1,
    "fit_screening": {"max_candidates": 4}
  }
}
```

This fragment belongs in an ordinary `ftx-design-study/1` request with material choices and an
explicit `design_fit` contract, evaluated through a model-backed entry point. Screening requires
that selected-body context; standalone or raw-mesh calls cannot invent it. `max_candidates` is
1–256 and excludes the always retained original baseline. Each inspected point uses the unchanged
original pattern, its exact linked operations, canonical seam/truing/completeness checks, sampled
outline checks and explicit body-fit targets. No candidate is selected solely because it has the
smallest displayed residual.

In the synthetic 20 mm wide rectangle example, a declared ease band of 7–9 mm accepts the 8 mm
choice. With `max_designs: 1`, the ordinary prefix would inspect only the 0 mm choice. Screening
instead reaches and selects `grid:0002`, then crosses that exact geometry with each material and
runs the ordinary cutting, sewing, mass and requested simulation consumers. Its altered pattern and
frozen material bindings replay through the same existing APIs. Flat screening is performed once
per geometry, including the baseline, and reused across materials and named operating cases.

`design_search.fit_screening` distinguishes the actual outcomes:

| Field | Meaning |
|---|---|
| `screened_designs` | Grid points inspected, excluding baseline |
| `qualified_designs` / `rejected_designs` | Points passing or failing the complete canonical flat gates |
| `selected_designs` | First qualifying points sent to the material cross product, up to `max_designs` |
| `qualified_designs_omitted` | Passing points withheld by the downstream design allowance; their production outcomes remain unknown |
| `unvisited_designs` | Points beyond the screening allowance; their fit and production outcomes remain unknown |
| `complete_fit_screening` | Every declared grid point was inspected |
| `all_qualified_designs_selected` | Every passing point in the inspected prefix was selected |
| `rows` | Authored-order exact geometry/fit receipts, selection status and any structured refusal |

Screening rows have no `material_key`, because no material substitution has happened yet. They retain
the exact original and altered pattern identities, operations, variables, altered JSON, canonical
validation and selected-body fit receipt. Failed or unmeasurable points remain inspectable. A passing
point omitted after `max_designs` has no fabricated refusal; its `selected` flag is false.

With screening, top-level `generated_designs` counts selected grid points sent downstream, while
`omitted_designs` counts every point not sent downstream, including explicitly rejected ones.
`complete_grid_enumeration` is true only when each point was sent downstream or explicitly rejected
by flat screening: both `unvisited_designs` and `qualified_designs_omitted` must be zero. Exhausting
either allowance retains any selected feasible incumbent and marks incomplete decision coverage.
Even complete screening with no qualifying point establishes only the recorded canonical outcomes,
not empirical infeasibility or missing evidence. A complete screening pass with unselected passing
points is not a complete material/scenario comparison.

All inspected operations, root-search document passes, retained source/body/fit data, measurement
sampling and outline work are reserved before screening starts. Screening admits at most 4,096
alteration operations, 256 MiB of aggregate document-processing reservation, and 40 million units
each for measurement and outline validation. Existing material/scenario caps apply to the reserved
selected cross product. Omitting `fit_screening` preserves the original deterministic-prefix mode.

Grid domain data, fit specification and body identity are each bounded to 256 KiB; selected edge,
measurement and document-processing work is charged across the expanded rows. Nonzero values whose
site multiplication underflows to zero refuse instead of silently dropping the alteration.
The workbench displays search completeness, parameter choices, target admission, every canonical
point and the complete body/fit receipt. Existing exact altered/bound downloads remain the replay
path. Source-qualified strain limits, physical garment validation and contact-pressure constraints
require their own evidence and remaining implementation.

### Find selected-body fits between authored grid values

`design_adaptive` replaces `designs` or `design_grid` when a useful alteration may lie between the
listed grid points. It requires the same explicit `design_fit` and selected model/body authority:

```json
{
  "design_adaptive": {
    "objective": "fit_band_violation_mm",
    "variables": [{
      "key": "length",
      "bounds_mm": [0, 60],
      "initial_mm": 0,
      "initial_step_mm": 20,
      "minimum_step_mm": 0.0001,
      "sites": [
        {"piece": "front", "axis": "y", "at_mm": 50, "multiplier": 1},
        {"piece": "back", "axis": "y", "at_mm": 50, "multiplier": 1}
      ]
    }],
    "after": [],
    "max_evaluations": 96,
    "max_designs": 1
  }
}
```

[The complete synthetic request](../examples/design-adaptive-search.json) asks the original 100 mm
panel to finish 3.699–3.701 mm above one tenth of the selected model's stature. For an independently
selected 1300 mm model, this needs approximately 33.7 mm of linked length spread. The old
20/30/40 mm grid misses that narrow band; adaptive polling can find it and submit the exact altered
panel to the existing material binding, cutting and assembly-mass consumers. Use this request in
the ordinary `ftx materials scenario-study` command shown above, or load it in the model-backed
workbench study. These dimensions and bands exercise software; they do not prescribe wearer ease.

All bounds, initial coordinates and steps are explicit. One to three variables are supported,
using the same linked slash or vertex-move sites and optional canonical `after` operations. The search polls
variables in authored order, first negative then positive, within their declared bounds. Its
explicit discovery objective minimizes the largest absolute distance outside a fit band, then the
sum of those distances to resolve ties between multiple points. Both use canonical **unrounded**
millimetre readings; an out-of-band residual displayed as `0.0` still cannot qualify. If a poll has
no strict improvement, steps halve down to the declared minimum. Geometry, seam, truing,
completeness, outline and missing-body failures cannot supply a numerical search score.
The canonical slash-spread and vertex-move operators round edited points to 0.001 mm. Retained variable values
are the exact requested spreads; `raw_fit` measures the resulting geometry. A smaller polling
step helps find that operator's accepted coordinates without increasing its geometric precision.

The first `max_designs` completely qualified trials are retained alongside the original baseline.
Each retained geometry is evaluated once and reused across material alternatives and named cases;
downstream manufacturing and scenario objectives still decide their own feasible frontier. The
search objective alone never selects a production winner. Trials use the unchanged original
pattern, preserving the exact operations, altered JSON, body identity and frozen material changes
needed for replay.

The `ftx-design-adaptive-search/1` receipt in `design_search` retains every unique trial, its exact
coordinates, `raw_fit` readings, two explicit residual values, selection status and any refusal.
It also reports `evaluations`, `selected_designs`, final steps and the best admitted trial, which
may still be out of band. `stop_reason` is `qualified_design_limit`, `evaluation_budget` or
`minimum_steps`; only the second sets `budget_exhausted`. A budget-limited result preserves any
already qualified candidate and its actual downstream outcomes. `complete_search` and
`global_optimum_claim` are always false: a local step limit or empty result cannot establish that
the remaining continuous domain has no feasible solution. A zero residual only establishes the
declared numerical band checks, without empirical fit or propagated uncertainty claims.

`max_evaluations` admits 1–256 distinct parameter trials, excluding the separately checked original
baseline; repeated exact coordinates reuse their receipt. `max_designs` is 1–31, and the reserved
baseline-plus-designs × material cross product still cannot exceed 32. The whole requested domain
and maximum work are admitted before search: at most 16 operations per trial, 4,096 total
operations, 256 MiB of document-processing reservation, and 40 million measurement and outline
work units each. Missing selected-body context refuses the request. Failed trials remain
inspectable without invoking their production or simulation callbacks.

### Guide adaptive designs with actual manufacturing or scenario outcomes

Use `objective: "consumer_comparison"` when a passing flat target is only the admission gate and
the next alteration should improve a real production result. Nominate exactly one existing
material alternative with `material_key`, and supply an ordinary `comparison` with exactly one
nominal objective. Its existing constraints, required consumers, source qualifications and
protocol rules also govern every search probe. For example, a broad declared length band can
admit several designs while the production assembly-mass consumer guides the search toward a
lighter complete assembly:

```json
{
  "design_adaptive": {
    "objective": "consumer_comparison",
    "material_key": "light-sheet",
    "variables": [{
      "key": "length", "bounds_mm": [0, 60], "initial_mm": 40,
      "initial_step_mm": 20, "minimum_step_mm": 5,
      "sites": [
        {"piece": "front", "axis": "y", "at_mm": 50, "multiplier": 1},
        {"piece": "back", "axis": "y", "at_mm": 50, "multiplier": 1}
      ]
    }],
    "max_evaluations": 32, "max_designs": 1
  },
  "assembly_mass": {
    "schema": "ftx-assembly-mass/1", "inventory_complete": true, "extras": []
  },
  "comparison": {
    "basis": "nominal",
    "objectives": [{"metric": "assembly_mass_kg", "direction": "minimize"}],
    "required_consumers": ["cutting", "sewing"]
  }
}
```

This fragment still needs the source pattern, material alternatives, cutting/sewing requests and
explicit `design_fit` body bands. The numbers demonstrate software behavior, not wearer ease or
manufacturing recommendations. With the synthetic two-panel fixture, a 120–160 mm finished-length
band admits the initial 140 mm design. The ordinary fit-only mode stops there when `max_designs`
is one; consumer guidance continues and retains the 120 mm design with its smaller actual cut
inventory and source-qualified mass. Sewing and cutting must still pass because this policy
explicitly requires them.

The same request works through the model-backed `study-on-body` manufacturing command. Through a
single `scenario-study`, the sole objective can instead be an existing scenario metric such as
`simulated_cloth_mass_kg`, `max_warp_green_strain` or `transient_coverage_inside_energy_j`. The
ordinary production scenario must request and admit that consumer. A drape must meet the same
settledness, gating, source and descriptor checks as a normal comparison; an unfinished frame,
missing modulus, unknown mass, failed constraint or refused callback cannot supply a zero or
usable search value. No new physics, weighted score or inferred comfort/failure limit is added.

Every coordinate starts from the original pattern and passes complete canonical geometry and
selected-body fit checks before binding the nominated material or calling production consumers.
Outside the declared fit bands, the original raw residuals can guide the search toward admission.
Once a production-admitted incumbent exists, flat-only or consumer-excluded candidates cannot
replace it. Inside the admissible region the existing comparator's nominal dominance rule chooses
strict improvements. The first admitted probe fixes the relevant input protocol; a different
protocol remains visible but cannot guide replacement unless `allow_mixed_protocols` was explicitly
enabled. The search never silently combines material models or scenario environments.

`max_designs` limits the retained geometries; reaching it does **not** stop consumer-guided
polling. Better admitted trials replace earlier incumbents until the unique-evaluation allowance
or minimum steps stop discovery. Ties retain earlier trials. The original baseline and each
retained geometry then cross all authored material alternatives. The nominated material's
retained production outputs are reused exactly, including its bound pattern, source evidence and
scenario result; other material alternatives receive fresh ordinary evaluations. The final
comparison still determines the conditional frontier over those evaluated design/material pairs.

The optional `consumer_guidance` object in the existing `ftx-design-adaptive-search/1` receipt
identifies the material, sole objective, best trial, existing reading with units and input protocol.
Its `evaluations` counts actual nominated-material probes after flat qualification;
`reserved_evaluations` is the conservative probe-plus-final consumer-slot upper bound. Each probe
retains the unchanged `consumer_comparison` candidate, including all readings and exclusions, and
any original binding/callback `consumer_refusal`. Displaced and failed trials remain inspectable.
`qualified_designs` counts flat-qualified trials independently of final `selected_designs`.

All aggregate manufacturing count limits and document reservations include the entire possible
probe allowance plus the final material cross-product before work starts. Geometry, mass and
sewing-station work counters are shared across probes and final evaluations. The caller's same
scenario session reserves each callback's actual mesh/motion/heat work; the slot count does not
guarantee that all requested scenarios will fit its numerical budget. Evaluation/step stops and
later per-callback refusals preserve an already retained successful nominee. Structural errors
and global manufacturing hard-budget failures still refuse the whole request. Only up to
`max_designs` full probe outputs are retained; rejected probes keep bounded decision evidence.

For a named scenario matrix, set `design_adaptive.case_key` to the authored case that guides
polling. Its effective comparison policy must declare exactly one nominal objective. Every case
must have an explicit effective policy, either its own `comparison` or the study fallback, and
every case must admit a trial before it can become an incumbent. A better guidance-case objective
cannot bypass another case's constraint or protocol rule. Other cases may use their own ordinary
multi-objective policies; the engine never combines unlike quantities into a weighted score.

For example, nominate `"case_key": "weight"` to minimize `simulated_cloth_mass_kg`, and give a second
case a declared mass or strain constraint. Both cases execute the existing production consumers on
the actual altered garment. If shortening improves the nominated objective but violates the other
case's limit, its exact geometry and readings remain visible while the earlier admitted design stays
selected. All-case admission follows each policy's **required** consumers; an optional independent
consumer may refuse without invalidating an otherwise admitted manufacturing-only policy.

The receipt retains `consumer_guidance.case_key`, authored `case_order`, and each trial's
`consumer_cases` with original decisions and independent refusals. Protocol compatibility is checked
separately against each case's first fully admitted incumbent. Manufacturing runs once per probe;
the selected nominee reuses its exact cached manufacturing and complete case results. Final baseline
and other material combinations receive fresh ordinary evaluations. Both reserved and actual matrix
callback counts include probes; the conservative slot reservation is
`(max_evaluations + (max_designs + 1) * material_alternatives) * cases`, within the existing 64-slot
matrix limit. Case sessions retain their cumulative geometry/motion/thermal budgets.

Missing or unknown case keys, missing effective policies, an inadmissible guidance objective or an
insufficient aggregate reservation refuse before callbacks. Single-case and manufacturing-only
guidance omit `case_key`; original `fit_band_violation_mm` requests reject it and retain their
previous serialization. Multiple guidance objectives, reported-bounds guidance and unknown material
keys remain unsupported.
Both modes retain `complete_search: false` and `global_optimum_claim: false`; neither an empty
result nor a local improvement establishes empirical garment fit or a global optimum.

## Constrain material-direction strain on selected garment regions

Add `material_strain` to the scenario to retain the production membrane's raw, dimensionless
warp `E11`, weft `E22` and engineering shear `2E12` on selected physical instances:

```json
{
  "material_strain": {
    "schema": "ftx-material-strain/1",
    "instances": ["band"],
    "max_triangle_samples": 100000
  }
}
```

Use exact retained instance names; omit `instances` to include every retained instance. The budget
counts each selected triangle in the initial state and every accepted motion sample. An exhausted
or invalid strain consumer keeps the independent drape, motion and manufacturing outputs available.

Both material studies and garment-design studies can consume the extrema through their ordinary
`comparison` policy. This illustrative policy minimizes the largest sampled warp strain while
enforcing an authored signed shear band:

```json
{
  "comparison": {
    "basis": "nominal",
    "objectives": [
      {"metric": "motion_max_warp_green_strain", "direction": "minimize"}
    ],
    "constraints": [
      {"metric": "motion_min_engineering_shear_green_strain", "minimum": -0.08},
      {"metric": "motion_max_engineering_shear_green_strain", "maximum": 0.08}
    ]
  }
}
```

These example limits are synthetic design requirements. They supply no fabric failure or comfort
evidence. Green strain is not extension percent. Use `min_` or `max_` with `warp_green_strain`,
`weft_green_strain` or `engineering_shear_green_strain` for the six initial-state readings; prepend
`motion_` for the six readings over the initial state plus every accepted sample. A motion request
and its complete, converged trajectory are required for motion metrics. A settled, gating-clear
initial drape remains required for both scopes.

Reports retain extrema per selected instance, the responsible source triangle and sample time,
exact material/frame identities and the material's characterized domains. The workbench shows
these witnesses alongside comparison eligibility. `material_strain` or `motion_material_strain`
may also be required independently through `required_consumers`. Sampled compliance does not
establish continuous-time bounds, rupture limits or physical garment fit.

## Compare textile cooling and supplied liquid

Qualified `motion.transient_surface.surface_water` histories can drive the existing nominal
temperature and energy objectives. Additional metrics read actual textile-owned water:
`textile_water_evaporated_mass_kg`, `textile_water_remaining_mass_kg`,
`textile_water_latent_energy_j` and, for version 2 delivery, `textile_water_supplied_mass_kg`.
They remain separate from prescribed body-water metrics.

The [complete textile-liquid policy](textile-surface-water.md) declares the source-qualified domain,
locked material/instance bindings and vapor/thermal conditions. Version 2 independently sources
delivery rates and incoming temperatures for each mechanical interval; dry patches can consume
their supply or rewet. Incoming sensible heat and delivered mass enter the conserved history.
Comparison reconstructs the supplied totals and requires the complete source, geometry, water and
energy accounting. A missing source, unsupported loading or partial history excludes that consumer
without erasing independent manufacturing results. These decisions remain nominal, without
propagated wet-material uncertainty or empirical wearer validation.

## Budgets, tests and remaining gates

Explicit variant `changes` can also include a complete `interfaces` replacement using the existing
source-qualified pair contract. This allows a shell substitution and its new shell-to-lining evidence
to reach the same bound alternative. Omitted/null retains the original table; `[]` clears it.
`frozen_changes` preserves an explicit replacement for exact replay. Neither screening nor binding
infers pair coefficients, rewrites stale hashes or grants estimate consent. The actual mechanical
scenario still requires complete admitted contact evidence. See [piece-materials.md](piece-materials.md)
for native and browser use.

Native and browser boundaries limit combined input to 64 MiB. Screening permits 1024 candidates,
64 constraints and 16 objectives. Studies permit 32 evaluated variants, 16,384 aggregate expanded
cut instances across both roll and finite-stock consumers, 4096 aggregate sewing passes and 250,000
retained sewing stations. The sum of declared finite-stock `work_limit` budgets over all alternatives
must not exceed 20 million; reduce that per-plan limit or evaluate fewer alternatives when necessary.
Individual planners retain their own limits. Over-budget studies refuse, not return a truncated
winner list.

Regression tests cover units, interval overlap, ties, evidence/condition/method policies, missing
values, malformed requests, immutable replay, real marker/whole-order/seam equality, material-only
sewing refusal, heterogeneous assignments, complete-or-unknown mass, explicit screening pruning,
real hide/remnant planner equality, unchanged inventory across alternatives, stock source mismatch,
finite-stock-only and scenario composition, and both real native binaries. Native-callable wasm
tests use the same library outputs; wasm32
compilation alone is not a browser-runtime or native/wasm execution-parity claim.

Still separate: fully characterized linen/cowhide/performance-knit lots, held-out garment validation,
measured manufacturing recipes, calibrated robotic execution, live supplier coverage, propagated
scenario uncertainty and empirical qualification of computed objectives. Screening/study
software does not close those empirical gates or grant a fire, cut-resistance or cold-weather rating.
