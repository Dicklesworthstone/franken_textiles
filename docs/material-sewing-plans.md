# Material-aware sewing plans

`ftx-materials sew-plan PATTERN.json OPTIONS.json` produces JSON using the same
per-piece material selections as draping and roll cutting. The library entry point
is `ftx_export::sewing::SewPlan::build`. This is an offline manufacturing review,
not robot control, a qualified sewing recipe, or a physical seam-quality claim.

An executable synthetic example is included:

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-plan \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-options.json
```

For a basic job, declare one forward pass per pattern seam and the machine envelope:

```json
{
  "machine": {
    "id": "example-configuration-not-a-qualified-machine",
    "stitch_mm": [0.5, 5.0],
    "feed_b_over_a": [0.5, 2.0],
    "max_stack_mm": 3.0,
    "max_stitches_per_minute": 1000.0
  },
  "seam_tolerance_mm": 0.5,
  "passes": [{
    "seam": "side",
    "max_stitch_mm": 2.5,
    "plies": [1, 1],
    "minimum_allowance_mm": 6.0,
    "stitches_per_minute": 600.0,
    "needle": "operator-selected-needle",
    "thread": "operator-selected-thread",
    "after": []
  }]
}
```

These are invented example settings, not recommendations for any machine or textile.
Replace them with the actual configuration and process requirements. Needle/thread
identifiers are retained but their compatibility, penetration and strength are not
inferred unless an explicit source qualification is supplied as described below. Multiple passes, reverse feed and explicit start/end backtacks are supported
as described below. Explicit offset stitch lines and their review preview are also supported.
Physical folding and automatic assembly-order inference remain separate work: a declared
ply count or offset does not establish how the folded layers meet under a needle.

For several garment copies, [production scheduling](sewing-production.md) expands per-copy
dependencies and shares qualified needle/thread setups across the complete batch. It uses these same
material and process checks, keeps source geometry once, and emits actual repeated stitch travel.

## What follows a material change

Each seam side resolves `piece.fabric` or the inherited global selection through
`ftx_pattern::textile_import`, including its evidence policy and digest lock. Sourced
sheet or hide clearance comes from the shared `unloaded_thickness()` admission helper,
then is normalized to millimetres. An explicit source-qualified `unloaded_thickness`
observation takes precedence over an ordinary `thickness`. Pressure/force gauge
observations cannot establish unloaded staging clearance; an ordinary unconditioned
thickness retains its compatibility fallback. The separate gauge remains available
to thermal consumers. The [material contract](spec/06-textile-materials.md#unloaded-manufacturing-clearance)
specifies source, conditioning, wet-state and contradiction checks. The reported upper thickness
bound is used when present (without relabeling its statistical meaning); otherwise
the central value is used and no unreported tolerance is invented. Thickness is
multiplied by the explicit side ply counts, then summed at the needle. Thus changing
a thin linen selection to a thick hide can refuse a stack that previously passed,
without modifying the pattern geometry or sewing code. No compression law is guessed.
Loose fill, fiber and yarn records cannot substitute for a characterized sheet.

Every actual folded/lining ply uses this same unloaded authority. A separately qualified
presser-foot pressure or clearance calculation still consumes its acquired loaded-thickness
curve and checks the machine's loaded envelope. Passing that loaded check does not relax
unloaded staging clearance, and adding an unloaded observation changes the frozen material
identity used by existing recipe qualifications.

The output retains portable material snapshots keyed by their resolved SHA-256 and
a digest of the complete input pattern. A prior JSON plan is an immutable result,
not a live view: rerun the command after changing a pattern, material or machine.
The planner needs thickness, not invented membrane moduli; incomplete mechanical
characterization can therefore leave this manufacturing check usable.

## Source-locked sewing qualifications

Version 5 can require external process evidence without inventing a recipe from fiber or brand names.
Set `require_qualification: true` and supply `qualifications`. Each qualification names the exact
resolved material stack by SHA-256, machine configuration, needle, thread, actual stitch-advance
range, local b/a feed range, uncompressed stack range, continuous stitch-rate range and whether reverse
travel was qualified. It also carries evidence, source, method, conditions and separate consent for
estimates.

The planner matches the **generated** operation, not merely the requested maximum stitch pitch. Every
actual station interval on both sides, every local feed ratio, the derived stack thickness and requested
rate must be inside one qualification. Backtacks and reverse passes require `allow_reverse: true`.
An explicit folded assembly uses its complete bottom-to-top material preimage. Legacy ply-count jobs
use their deterministic A-plies-then-B-plies sequence because no richer physical stack order was declared.
Changing an otherwise identical material record changes its snapshot hash and invalidates the qualification.

Exactly one qualification may admit a required operation. Missing, stale, out-of-envelope or ambiguous
evidence refuses the entire plan. When qualifications are supplied but not required, a uniquely matching
one is attached to the operation; absence remains an unqualified review rather than a hidden failure.
Estimated process evidence requires its own `allow_estimates`; this is independent of the material
selection's evidence policy.

A version-5 match is **source qualification admission**, not independent seam validation. The planner does
not reinterpret the source's test, infer pucker/strength/needle damage, or verify machine calibration,
workholding, accessibility or operator setup. Consequently `hardware_execution_authorized` and
`physical_seam_quality_validated` remain false even when every operation has a qualification.

## Source-qualified thread inventory and purchasing

Add `"consumables": {}` to the ordinary sewing options to compute a thread bill of materials from
the **actual selected production plan**. The same opt-in works through `sew-plan`, `sew-stitches`,
the shared native/WASM consumers and recipe selection. It selects `ftx-sewing-plan/11`; omitted
consumption data and policy preserve existing outputs. No new stitch geometry or price engine is used.

Each applicable `SewingQualification` supplies an optional `thread_consumption` object. Its
`evidence`, `source`, `method`, `conditions` and `allow_estimates` describe consumption independently
of the parent process envelope. Permission to use estimated process or material data does not grant
permission to use estimated thread consumption. The method must support the declared effective
coefficient over the qualification's actual stitch/feed/stack/rate/reverse/pressure envelope.

`channels_complete` explicitly states whether the source represents **every required thread channel**.
Each entry in `channels` has these fields:

| Field | Meaning |
| --- | --- |
| `id` | Unique channel identity, such as needle, bobbin or a named looper. |
| `material` | Complete embedded, digest-locked `Selection` of a yarn record. |
| `metres_per_traversal` | Positive, finite thread metres per generated stitch interval. |
| `start_allowance_m` | Explicit nonnegative metres added once at this operation/channel's start. |
| `end_allowance_m` | Explicit nonnegative metres added once at this operation/channel's end. |

A traversal is **one generated station interval**, counted every time the program traverses it.
Reverse travel, both legs of each backtack and separate reinforcement passes therefore consume
thread. Reversing direction does not subtract consumption. Start/end allowances apply once per
complete operation/channel, not per station or directed run. The channel list is explicit: a needle
thread never silently implies an identical bobbin thread, another looper or an unreported channel.
The source must qualify the supplied coefficients; no universal stitch-class ratio is inferred.

The report retains each operation/channel contribution and its exact qualification, all frozen yarn
selections, and complete requirements aggregated by material SHA-256. Repeated channels or operations
using the same snapshot share one requirement; different snapshots stay separate even when their
thread/setup identifiers or material IDs match. Accumulation uses stable operation/channel order so
changing the sewing schedule does not change quantities. `total_required_length_m` is absent when
any operation lacks admitted consumption or when any channel inventory is incomplete. Known
contributions remain inspectable, but no partial aggregate is published as a whole-job requirement.

For dated purchasing, set `consumables.purchasing.quote_book` to an existing `ftx-textile-quotes/1`
or `/2` book, and `consumables.purchasing.market` to the ordinary procurement market. Each complete
yarn requirement goes through `QuoteBook` with `basis: "length"`. Linear offers need no sheet
width. Kilogram offers additionally require a positive, source-qualified `yarn_linear_density`
from that exact frozen yarn; the ordinary catalog normalizes tex and denier to kg/m. Supplier
minimums, increments, all-unit price thresholds,
available quantities, exact dates and validity limits apply **after** all matching channels and
operations have been aggregated. Each operation/channel's complete demand, including allowances,
is one uninterrupted segment; the largest determines the existing continuous-length check.
The market's quantity and continuity policies retain their meanings. Supplier splitting is not
supported by this consumer and a supplied `split_orders` policy refuses explicitly.

An admitted linear density also supplies each complete requirement's optional `mass` receipt:
`linear_density_kg_m` and `required_mass_kg`. `complete_mass_coverage` and
`total_required_mass_kg` report a complete nominal inventory mass only when all channels and yarn
densities are available. Missing or unapproved density leaves lengths and length-denominated
offers usable, while `mass_unavailable_reason` records the missing capability. The calculation
includes start/end allowances and every repeated traversal. It does **not** establish the mass
retained after trimming, add mass to the garment's dynamics, or duplicate an assembly extra.
Reported density bounds remain attached to the frozen source; these nominal calculations do not
claim uncertainty propagation or a conservative mass requirement.

For a kilogram offer, quantity, minimum, ordering increment, all-unit tiers and observed stock
remain in original kilograms. Only the canonical acquired quantity and unit rate are converted
back to metres and currency/metre, allowing comparison with linear offers. The selected offer
retains `yarn_linear_density_kg_m` as its conversion receipt. Neither tex nor a nominal cone's
put-up length establishes actual stock, an order increment or an uninterrupted segment. Because
the current quote schema only records uninterrupted limits on linear sale units, a kilogram
offer has unknown continuity and cannot pass `require_observed_continuous_length: true`.
Permissive continuity can admit it while retaining that unknown status. No mass-to-length
conversion is performed for fiber, sheet or fill records.

Only currently eligible offers locking the exact consumed yarn hash can be selected. An unlocked
latest observation cannot revive an older locked price. Supplier/base/tier SKU aliases are connected
across canonical latest observations in the exact market at the requested time, including unused
bridging offers. Future observations and superseded aliases cannot change an earlier purchase.
A component that could supply different consumed snapshots is excluded from separate independent
orders, preventing the same inventory from being counted twice. This is conservative: ambiguous
current stock identity may withhold a total rather than prove that no real purchase is possible.
Other independent offers remain eligible. Retained canonical samples and excluded offer IDs explain
those decisions.

`purchasing.least_observed_independent_order_total` is present only when every required material has
an eligible complete order. Missing consumption prevents pricing altogether; missing eligible prices
keep the whole-job total unknown. This value is merchandise on the declared comparable terms, not a
checkout price, shipping calculation, reserved inventory or supplier-bundle optimum. Consumption
models and complete-channel assertions are source declarations, not physical seam validation.

Limits are eight channels per qualification, 32 declared yarn snapshots, 4096 quote observations and
200000 preflight quote/stock-alias work units. The public `purchasing_work_bound` permits a study or
recipe search to reserve the work for all possible plan evaluations; recipe selection enforces the
same 200000-unit ceiling across its entire configured trial budget. Existing operation/station and
output-size limits also apply. In `sewing_consumables` synthetic regressions, 90 actual intervals
across two passes and two declared channels require 3.7 metres after the explicit operation
allowances; one four-metre rounded order reaches its declared price threshold. Those invented
coefficients exercise arithmetic and source propagation, not real thread consumption.

### Production quantities share cutting and thread purchasing

Standalone sewing accepts `"consumables": {"production_copies": 3}` to request a three-garment
thread inventory. Counts are integers in `1..4096`. Omitting the field preserves the original
single-garment `ftx-sewing-consumables/1` output. An explicit count selects
`ftx-sewing-consumables/2`, retaining `production_copies` and each operation/channel's `per_copy`
interval count, traversal length, start/end allowances and complete required length alongside its
batch contribution.

The consumer repeats every actual traversal and every start/end allowance once per garment, then
aggregates by exact yarn snapshot **before** applying supplier minimums, increments, price tiers
and available stock. It prices a complete production order, not a sum of independently rounded
one-garment orders. The largest complete operation/channel on a single garment still determines
the continuous-length requirement; separate garments do not require a single unspliced thread
segment spanning the whole batch. Incomplete consumption remains unknown for every quantity.

Material/design studies inherit the cutting quantity from roll, finite-stock or mixed cutting-job
requests when the sewing count is omitted. Every requested cutting route must agree. An explicitly
different thread count refuses the sewing consumer while preserving independent cutting results.
Both fixed plans and cost-directed recipe selection receive the quantity before evaluation, and
the selected options retain it for standalone replay.

The regression's two-garment 7.4 m demand becomes one 8 m order costing 16 in its synthetic market;
pricing two separate one-garment orders would cost 28. It also checks finite-stock refusal, repeated
allowances, unchanged 1.2 m continuity, actual cutting routes and native/portable replay. These are
arithmetic examples, not live quotes. Sewing paths, feed durations and setup schedules still describe
one garment: a batch execution schedule and cross-garment setup optimization are separate work.

## Geometry, feed and sequencing

### Optional setup-change optimization

Add an `ordering` policy to the existing sewing options to minimize changes between
the declared needle and thread identities while respecting every `after` prerequisite:

```json
{
  "ordering": {
    "objective": "minimize_setup_changes",
    "max_search_nodes": 20000,
    "initial_setup": {"needle": "installed-needle", "thread": "installed-thread"}
  }
}
```

The explicit objective is **needle changes + thread changes**. Changing both counts
as two; keeping both counts as zero. Exact identifiers determine equality. This is
a count of declared setup changes, without invented changeover seconds, thread
consumption, quality weights or a whole-job production-time estimate. The optional
`initial_setup` counts changes from that declared installed setup to the first pass.
When omitted, first installation is outside the count. No final reset is assumed.

For example, four independent operations `A, B, C, D` with setup pairs `X, Y, X, Y`
normally use their alphabetical order. If both the needle and thread differ between
X and Y, that order has three needle and three thread changes. The selected order
`A, C, B, D` has one of each, reducing the explicit count from six to two. Prerequisites
can prevent this grouping. The planner never relaxes them to improve the objective.

Every original seam, material, recipe, geometry, qualification and allocation check
finishes before optimization. The optimizer moves complete admitted operations;
stations, offsets, folds, backtacks, source locks and feed-only duration are retained.
Each declared flat assembly remains an operation-local input. A better order does
not establish physical accessibility, infer missing prerequisites or simulate how
the garment moves from one operation to the next.

The optional search supports at most **20 passes** and requires `max_search_nodes`
in **1..200000**. Larger jobs can use the existing planner without this policy.
It starts with the complete normal lexicographic topological schedule, then searches
feasible prefixes deterministically by immediate change count and operation ID.
Equal total counts retain the incumbent; there is no hidden secondary objective.
Each examined feasible prefix extension, including a pruned trial, uses one search
node. Search memory holds at most 20 frames with at most 20 candidate indices each;
the original station and geometry budgets still apply independently.

The version-6 plan's `ordering` report retains `baseline_order`, `selected_order`,
separate needle/thread/total counts for both, `search_nodes`, and
`lower_bound_total_changes`. `search_complete` and `optimality_proven` are true only
when all better schedules have been eliminated by the search or its valid count
bound. This proves the count optimum only for the declared prerequisites and setup
identities. If the work limit is reached first, `budget_exhausted` is true and both
proof fields are false; the returned order is still a complete validated incumbent,
never an unfinished prefix, and cannot be worse than the baseline.

Omitting `ordering` preserves existing order, arithmetic and serialized output.
The existing `sew-plan`, `sew-stitches`, native/WASM and material-study consumers
use the same planner; an expanded stitch program follows the selected order.

### Paired geometry and machine limits

`Pattern::seam_pairs` supplies the actual curved/mirrored seam geometry and matches
notches in the existing reverse-side convention. Stations include edge junctions
and matched notches. The requested pitch is a maximum on either side, not an exact
uniform spacing. Every resulting interval must respect the declared machine's
minimum/maximum advance and local **b/a** feed ratio; a whole-seam average is not
enough. These are arclength advances along the sampled curves, not endpoint chord
distances. Coordinates remain in **two separate pattern-side frames**, never in a
shared sewn or robot coordinate frame.

Allowance uses the existing edge → piece → export-default hierarchy. The minimum
over every edge on each side must meet the pass requirement. Flat seam-walk checks
must also pass, with the existing per-seam tolerance and ratio semantics. Unmatched
or duplicate-position notches refuse. `after` dependencies are checked for unknown
operation IDs and cycles; lexicographic operation-ID order breaks ties deterministically. This is
the declared precedence graph, not proof that the seam is physically accessible.
Simulation-only scale and `sim:false` cannot change manufacturing geometry or omit
seams. Unscaled patterns refuse. Every pattern seam must have at least one pass.

The plan reports all traversed stitch intervals and continuous feed seconds at the chosen
stitch rate, including repeated backtack/reinforcement travel but excluding acceleration, loading, handling,
repositioning and changes of thread/needle. Without the explicit consumption policy above it reports neither thread consumption
nor an entire garment's production time. Budgets are 4096 passes and 250000
conservative station/travel slots; repeated lock travel consumes budget and oversized
requests refuse before station allocation.

## Multiple passes, reverse feed and backtacks

An operation's optional `id` defaults to its `seam`, preserving legacy one-pass jobs.
Two passes over the same seam need distinct operation IDs. `after` names these IDs,
not another pass's array index; it may refer to an operation declared later in the input.
The scheduler accepts a complete acyclic graph, not omitted seams or unknown predecessors.
Each pass independently specifies pitch, rate, plies, allowance and needle/thread IDs.
Each therefore rechecks the same selected material; a thicker substitution can pass
the initial join but fail the reinforcement stack without changing any stitch geometry.

`direction` is `forward` (default) or `reverse` through the canonical paired stations.
It is independent of the pattern seam's b-side `reverse` flag. Both material sides
reverse together; the feed ratio remains **b/a**, not its reciprocal. Any reverse
movement requires `machine.reverse` with its own `stitch_mm`, `feed_b_over_a` and
`max_stitches_per_minute`. Forward limits are not silently reused for reverse feed.
Every actually traversed interval is checked against its direction's envelope.

`backtack.start_stitches` and `backtack.end_stitches` are counts of existing station
intervals, not millimetres. They default to zero and cannot exceed the seam's interval
count. A start lock travels the requested number of intervals, returns to the initial
station, then traverses the complete seam. An end lock returns into the sewn seam and
travels back to its endpoint. For a 40-interval forward seam, start 3 and end 2 give:

```text
0 -> 3 -> 0 -> 40 -> 38 -> 40
```

That is 50 traversed intervals. Reverse traversal mirrors the indices, without
changing station geometry: `40 -> 37 -> 40 -> 0 -> 2 -> 0`. Lock phases do not prove
that a physical stitch is secured or model stitch-class-specific thread mechanics.

Run the complete synthetic two-pass example:

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-plan \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-multipass-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-stitches \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-multipass-options.json
```

The join has 50 intervals at 600 stitches/minute, followed by 40 reverse reinforcement
intervals at 300 stitches/minute: **90 intervals and 13 continuous-feed seconds**.
The reinforcement's extra declared ply is a thickness check, not a simulated fold.
The pattern, materials and machine in this fixture are synthetic, not calibrated data.

## Partial-seam operations and local construction changes

A pass can now select `source_range: [start, end]`, with finite
`0 <= start < end <= 1`. These fractions refer to the **original side-A seam arclength**,
before an offset or feed reversal. They are not Bézier parameters. Omission retains a whole-seam
operation. Side B follows the existing piecewise notch correspondence, including the pattern seam's
`reverse` setting; a user does not supply an independent B interval that could lose that pairing.

This makes the existing assembly instruction to split a changing construction into operations
actionable: a lower section can join two sheets while an upper section includes an explicitly
positioned reinforcement or folded flap. Each section uses its own recipe, selected material stack,
feed direction, offset and locks. Distinct passes need distinct operation IDs and explicit `after`
dependencies when their order matters. The explicit folded assembly still has to be valid and
constant-layer along each selected operation; no physical fold transition or workholding sequence
is inferred.

Every original seam must be completely covered on **both source sides** by the union of its passes.
Overlap is allowed for reinforcement; any gap refuses. B coverage is checked independently because
different offset recipes can map the same A split to different B positions. A whole-seam pass covers
both sides and may be followed by a local reinforcement. The original complete seam-walk acceptance
still applies: selecting an easier span does not rescue a rejected source seam.

Exact selected endpoints, enclosed notches and edge junctions become stations on the original dense
arclength geometry. Sampling respects maximum pitch on both paired sides. Offset paths map the
source interval through the existing offset correspondence, and both the traveled dense polyline
and between-stitch chords receive the existing cut/drill clearance checks. An obstacle outside the
selected span does not invalidate that operation. Reverse feed and backtacks traverse only the
selected station table and retain the existing machine and qualification limits.

Run the executable synthetic split-seam example:

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-stitches \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-span-options.json
```

Its 100 mm source seam is split at 0.6. `lower-side` travels 60 mm forward; `upper-side` then travels
40 mm in reverse. With 2.5 mm maximum pitch the job has **24 + 16 = 40 intervals** and **4 seconds**
of continuous feed. This does not include handling/setup time or qualify the synthetic machine.
`sew-plan` and `sew-svg` accept the same options; preview an operation by its exact ID.

Any explicit range produces a nested `ftx-sewing-plan/10`, alongside all other requested capabilities.
Each partial operation's `source_span` retains `source_ranges` for A/oriented B, the **full original**
`source_lengths_mm`, and the **one-way selected** `sewn_lengths_mm`. Repeated lock travel is counted
in the ordinary interval/feed totals instead. Station fractions and lengths keep their full-path
reference; offset receipt polylines contain only the traveled span. Omitted ranges preserve the
existing whole-seam serialization and arithmetic. The browser material workbench displays both
source intervals and full/selected lengths without recomputing the geometry.

## Compact plans, expanded travel and browser bindings

Unchanged legacy jobs still serialize as `ftx-sewing-plan/1`, without added default
fields or altered arithmetic. Jobs requesting operation IDs, reverse capability,
direction or locks use `ftx-sewing-plan/2`; explicit offset paths use `ftx-sewing-plan/3`, explicit folded assemblies use `/4`, and source-qualified jobs use `/5`. Source-qualified thread consumption/purchasing uses `/11` while retaining those geometric capabilities.
An explicit ordering policy uses `/6`, retaining any requested offsets, assemblies
and qualifications alongside its search result.
Each operation contains ordered `runs`
over its unchanged `stations.a` and `stations.b` arrays. A run visits every consecutive
integer index from `from_station` to `to_station`. Adjacent runs share an endpoint;
that shared station is not an extra interval.

`sew-stitches` calls `ftx_export::sewing::program::SewProgram::build` and emits
`ftx-sewing-program/1` for centerline jobs or `/2` for offset jobs. Its nested `plan`
uses explicit version-2 runs for centerline input and version 3 for offset input.
With an ordering policy, the nested plan retains version 6 and the expanded program
recognizes any actual offset or folded-assembly geometry independently of that version.
Its `stitches` enumerates every actual interval with operation/run
indices, source and destination station indices, both destination coordinates,
signed arclength advances, the b/a ratio and continuous-feed duration. Reverse feed
makes both advances negative. `begins_run` marks direction/operation boundaries.
No joining travel between different operations is generated. The builder resolves
and validates original inputs afresh rather than trusting an edited saved report.

The same JSON entry points are exported by `ftx-wasm` after rebuilding its bindings:

```js
import init, { sew_plan, sew_stitches } from "./pkg/ftx_wasm.js";
await init();
const plan = JSON.parse(sew_plan(patternJson, optionsJson));
const program = JSON.parse(sew_stitches(patternJson, optionsJson));
```

Both functions take JSON strings and return JSON strings, with the same planner and
material authority as the native CLI. They do not read files, use clocks, upload
inputs or control a device. The native-callable `ftx_wasm::manufacturing` module
contains their shared implementation. These are browser integration APIs, not a new
interactive sewing page. Output coordinates are still separate local pattern frames,
not robot trajectories or needle-up/down commands.

## Offset stitch lines and topstitch review

Add `stitch_path` to an individual pass to compute a different stitch line, not merely
translate its displayed stations. The executable topstitch fixture adds this policy
to a reverse reinforcement pass after an ordinary joining operation:

```json
{"offset_mm": [3.0, 4.0], "mitre_limit": 4.0, "minimum_cut_clearance_mm": 1.0}
```

These values are synthetic, not sewing recommendations. Each signed offset is in the
original CCW material-instance frame: positive goes inward, negative into the seam
allowance. Mirroring and b-side reversal preserve that convention; reversing a pass
does not swap sides or invert the b/a feed ratio. Source pattern geometry is unchanged.

The implementation offsets the existing 200-sample Bézier/polyline seam geometry.
Adjacent offset segments meet at a mitre; endpoints of an open side move along its
one-sided normal. Closed sides use cyclic corner normals. An excessive mitre, singular
turn, collapsed/reversed span or self-intersection refuses rather than beveling,
trimming or silently repairing the path. It is a sampled polygonal offset, not an
exact analytic parallel curve; refine and independently validate it for fabrication.

Source-to-offset correspondence is affine between corresponding polyline vertices in
arclength. Notches and edge junctions are transported by that explicit monotone map,
then paired across the two resulting stitch lines. This is not nearest-point projection
or a physical material-deformation model; at a sharp mitre the mapped notch may move
tangentially. The receipt retains full polylines, original lengths and source fractions
for every station so consumers can inspect that mapping instead of guessing it.

The planner resamples the actual offset arclength, recalculates both advances and
local feed, checks the forward/reverse machine envelopes, and charges all backtack
travel against the shared station budget. Consequently, offsetting two equal-length
120 mm corner paths by 5 mm and 3 mm gives 110 mm and 114 mm paths: the new b/a ratio
is 114/110, not the original 1. An unchanged tight-feed machine must refuse that job.
Distinct notch/junction breaks are not rounded together to avoid a minimum-stitch
refusal. Explicit zero offsets retain original station arithmetic while adding checks.

Clearance uses the existing exported exterior cut outline and edge/piece/default
allowance precedence. Every full path segment **and** every between-stitch chord must
stay inside and meet `minimum_cut_clearance_mm`; testing isolated stations alone could
miss a chord crossing a concave boundary. The reported minimum is over both geometries.
This exterior check does not model notch incisions, defects or the swept footprint of
a presser foot. Internal cuts and drills refuse until explicit obstacle geometry is
supported. It does not certify needle access, workholding or a folded assembly.

Offset evaluation has an 8192-vertex contour limit and a shared 50-million conservative
geometry-pair budget; excessive jobs refuse. The existing 4096-operation/250000-slot
job limits still apply, including offset growth and lock travel.

```sh
cargo run -p ftx-cli --bin ftx-materials -- sew-stitches \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-topstitch-options.json

cargo run -p ftx-cli --bin ftx-materials -- sew-svg \
  tests/fixtures/materials/synthetic-sewing-pattern.json \
  tests/fixtures/materials/synthetic-sewing-topstitch-options.json \
  topstitch topstitch-review.svg
```

The straight synthetic fixture retains 90 total intervals and 13 continuous-feed
seconds, but the second pass follows x=17 mm on side A and x=4 mm on side B rather
than the original x=20 mm/x=0 mm seam lines. This is a geometric example, not evidence
that those two lines are physically coincident after folding. Each operation still
resolves sourced thickness; a replacement material can invalidate only the thicker
reinforcement stack while leaving every offset coordinate unchanged.

`sew-svg` calls `ftx_export::sewing::preview::svg` and validates the complete job before
creating a new file. Existing paths are never overwritten. The returned JSON includes
the SVG digest. The preview distinguishes exterior cut, original seam, actual stitch
path, all needle stations and between-stitch chords; an arrow shows the primary pass
direction and a run list records lock/reverse travel. The two source-side frames are
laid out separately, with full pattern/material hashes in escaped XML metadata.
Display translations are not machine registrations. There are no scripts, external
resources or cutter/robot instructions. Preview budgets refuse oversized diagrams
instead of omitting stations (20000 stations across both sides, at most ten piece
edges with bounded curve controls); six-decimal rendering is not a manufacturing tolerance.

The same file-free browser entry point returns SVG text after rebuilding bindings:

```js
import init, { sew_svg } from "./pkg/ftx_wasm.js";
await init();
const svg = sew_svg(patternJson, optionsJson, "topstitch");
```

This is an integration API, not an interactive sewing page. No hardware permission,
process qualification, thread consumption or full production time is asserted.

## Acceptance and remaining work

Synthetic regressions exercise material substitution, mixed-material plies,
thickness bounds, notch-local differential feed, curve/edge correspondence,
allowance hierarchy, order dependencies, evidence refusals and allocation limits.
Directed-program tests additionally check exact lock traversal, repeated operations,
reverse-only machine limits, per-operation material substitution, legacy serialization
and full stitch expansion. CLI and WebAssembly consumers share these implementations.
They establish software propagation, not measured pucker, seam strength, appearance,
fit or safe operation. Both `hardware_execution_authorized` and
`physical_seam_quality_validated` are always false.

The next physical gate is matched seam coupons linking exact textile lots,
needle/thread/stitch/feed/tension/workholding settings and measured seam outcomes.
Robotics additionally needs machine-specific kinematics, accessible assembly
topology, calibration, sensing, collision checks and interlocks. Unknown settings
or finishes must not become machine instructions merely because this plan parses.
