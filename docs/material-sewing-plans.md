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
inferred. Multiple passes, reverse feed and explicit start/end backtacks are supported
as described below. Offset topstitch paths, physical folding and automatic assembly-order
inference remain separate work: repeating a seam does not invent another stitch line.

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
direction or locks use `ftx-sewing-plan/2`. Each operation contains ordered `runs`
over its unchanged `stations.a` and `stations.b` arrays. A run visits every consecutive
integer index from `from_station` to `to_station`. Adjacent runs share an endpoint;
that shared station is not an extra interval.

`sew-stitches` calls `ftx_export::sewing::program::SewProgram::build` and emits
`ftx-sewing-program/1`. Its nested `plan` always uses explicit version-2 runs, including
for legacy input. Its `stitches` enumerates every actual interval with operation/run
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
