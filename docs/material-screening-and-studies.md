# Source-qualified material screening and fixed-pattern manufacturing studies

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

The study takes ONE original pattern and shared `cutting`, `sewing`, `assembly_mass`, and optional
`purchasing` inputs. At least one manufacturing consumer is required; mass-only studies are allowed.
Purchasing contains the existing `quotes` book and
`market` scope and requires cutting: required stock comes from actual markers, including waste,
width, margins, minimum orders and sale increments, never just net pattern-piece area.

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

Each row independently reports binding, cutting, sewing, requested assembly mass and purchasing as `available`, `refused` or
`not_requested`. `available` purchasing can still have `complete_offer_coverage: false`: its total
then remains null. Missing mechanical parameters do not prevent source-qualified cutting or sewing.
Excessive thickness can refuse sewing while cutting remains available. Missing width refuses cutting
and its dependent pricing but does not prevent sewing. Every result is the existing planner's normal
output, including its provenance and physical/hardware limitations.

Summary metrics distinguish gross stock area, outer cut area, complete stock mass, least observed
independent order total, stitch intervals and feed-only time. Unknown mass for ANY roll makes total
stock mass unknown, not a deceptively smaller partial mass. Feed time excludes handling, acceleration,
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

Optional `seam_allowance_mm` uses the same edge/piece/export hierarchy as cutting. When both consumers
are requested, their effective export allowances must match; disagreement refuses mass but preserves
the independent cutting result. Cutting's `copies` counts the purchased order, not this **one-assembly**
mass. Stock includes waste; this inventory does not. Simulation omission/scaling never changes it.
Finishing losses, seam grading, washing, coatings and other unmodeled changes remain outside this
pre-finishing cut inventory. The result is not a weighed final-garment prediction.

`reported_bounds_kg` scales and adds input endpoints only when every contribution reports bounds.
Missing bounds stay absent, never zero-width. These ranges hold authored geometry and inventory
fixed; they are not confidence intervals or propagated geometric/measurement uncertainty. Study
comparison remains explicitly nominal: `assembly_mass_kg` can be an objective or inclusive constraint,
and `assembly_mass` a required consumer. Unknown/incomplete totals exclude a candidate without
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
sewing. Other metrics are `stock_area_m2`, `external_cut_area_m2`, `stitch_intervals` and
`feed_seconds`. Dimensions are fixed by their names; order cost uses the exact request currency.
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

`nominal` is an explicit required choice. Source ranges are **not** numerical output intervals;
`reported_bounds` comparison is refused rather than pretending uncertainty has been propagated.
This finite-candidate frontier is conditional on upstream screening, process options, quantities,
market date and input data. It is not optimal nesting, a global material optimum or physical validation.

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

## Budgets, tests and remaining gates

Native and browser boundaries limit combined input to 64 MiB. Screening permits 1024 candidates,
64 constraints and 16 objectives. Studies permit 32 evaluated variants, 16,384 aggregate expanded
cut instances, 4096 aggregate sewing passes and 250,000 retained sewing stations; individual planners
retain their own limits. Over-budget studies refuse, not return a truncated winner list.

Regression tests cover units, interval overlap, ties, evidence/condition/method policies, missing
values, malformed requests, immutable replay, real marker/whole-order/seam equality, material-only
sewing refusal, heterogeneous assignments, complete-or-unknown mass, explicit screening pruning,
and both real native binaries. Native-callable wasm tests use the same library outputs; wasm32
compilation alone is not a browser-runtime or native/wasm execution-parity claim.

Still separate: fully characterized linen/cowhide/performance-knit lots, held-out garment validation,
measured manufacturing recipes, calibrated robotic execution, live supplier coverage, propagated
scenario uncertainty and empirical qualification of computed objectives. Screening/study
software does not close those empirical gates or grant a fire, cut-resistance or cold-weather rating.
