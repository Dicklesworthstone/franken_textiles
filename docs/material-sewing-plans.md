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

For each pattern seam, declare one pass and the machine/configuration envelope:

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
inferred. Multi-pass/topstitch/locking recipes and automatic assembly-order inference
remain separate work; every seam currently has exactly one declared forward pass.

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
IDs and cycles; lexicographic seam-ID order breaks ties deterministically. This is
the declared precedence graph, not proof that the seam is physically accessible.
Simulation-only scale and `sim:false` cannot change manufacturing geometry or omit
seams. Unscaled patterns refuse. Every pattern seam must have exactly one pass.

The plan reports forward stitch intervals and continuous feed seconds at the chosen
stitch rate, excluding needle locking/backtacks, acceleration, loading, handling,
repositioning and changes of thread/needle. It reports neither thread consumption
nor an entire garment's production time. Budgets are 4096 passes and 250000
conservative station slots; oversized requests refuse before station allocation.

## Acceptance and remaining work

Synthetic regressions exercise material substitution, mixed-material plies,
thickness bounds, notch-local differential feed, curve/edge correspondence,
allowance hierarchy, order dependencies, evidence refusals and allocation limits.
They establish software propagation, not measured pucker, seam strength, appearance,
fit or safe operation. Both `hardware_execution_authorized` and
`physical_seam_quality_validated` are always false.

The next physical gate is matched seam coupons linking exact textile lots,
needle/thread/stitch/feed/tension/workholding settings and measured seam outcomes.
Robotics additionally needs machine-specific kinematics, accessible assembly
topology, calibration, sensing, collision checks and interlocks. Unknown settings
or finishes must not become machine instructions merely because this plan parses.