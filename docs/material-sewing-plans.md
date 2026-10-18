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

## What follows a material change

Each seam side resolves `piece.fabric` or the inherited global selection through
`ftx_pattern::textile_import`, including its evidence policy and digest lock. Sourced
sheet or hide thickness is normalized to millimetres. The reported upper thickness
bound is used when present (without relabeling its statistical meaning); otherwise
the central value is used and no unreported tolerance is invented. Thickness is
multiplied by the explicit side ply counts, then summed at the needle. Thus changing
a thin linen selection to a thick hide can refuse a stack that previously passed,
without modifying the pattern geometry or sewing code. No compression law is guessed.
Loose fill, fiber and yarn records cannot substitute for a characterized sheet.

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

## Geometry, feed and sequencing

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
repositioning and changes of thread/needle. It reports neither thread consumption
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

## Compact plans, expanded travel and browser bindings

Unchanged legacy jobs still serialize as `ftx-sewing-plan/1`, without added default
fields or altered arithmetic. Jobs requesting operation IDs, reverse capability,
direction or locks use `ftx-sewing-plan/2`; explicit offset paths use `ftx-sewing-plan/3`, explicit folded assemblies use `/4`, and source-qualified jobs use `/5`.
Each operation contains ordered `runs`
over its unchanged `stations.a` and `stations.b` arrays. A run visits every consecutive
integer index from `from_station` to `to_station`. Adjacent runs share an endpoint;
that shared station is not an extra interval.

`sew-stitches` calls `ftx_export::sewing::program::SewProgram::build` and emits
`ftx-sewing-program/1` for centerline jobs or `/2` for offset jobs. Its nested `plan`
uses explicit version-2 runs for centerline input and version 3 for offset input.
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
