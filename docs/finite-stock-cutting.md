# Finite hides and fabric remnants

`ftx materials stock-plan` and `ftx-materials stock-plan` allocate the pattern's real cutter
outlines onto individually identified finite stock. Leather hides do not need a fictitious roll
width. Irregular remnants, nonwoven sheets and finite laminates use the same path. Loose fiber,
yarn and fill are not sheet inventory. Existing rectangular roll planning is unchanged.

## Run the synthetic example

```sh
cargo run -p ftx-cli --bin ftx -- materials stock-plan \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  tests/fixtures/materials/synthetic-stock-request.json --json

cargo run -p ftx-cli --bin ftx-materials -- stock-svg \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  tests/fixtures/materials/synthetic-stock-request.json 0 hide-review.svg
```

`stock-svg` also runs through `ftx materials`. Its final argument must name a **new** file;
input/planning/index failures create no preview, and existing files are never overwritten.
The primary CLI uses its standard JSON envelope; the companion emits the result directly.
Both read at most 64 MiB combined JSON and call the same planner. They do not reserve inventory,
connect to a cutter, or modify the pattern/request.

This miniature example is entirely synthetic: an L-shaped 100 mm stock boundary contains a
15 mm square exclusion and accepts 20 x 20 mm and 15 x 20 mm external cut outlines. The whole
stock area is 0.006975 m²; defect-free area is 0.006750 m²; allocated external cut area is
0.000700 m². Neither the shape nor its 0.5 kg/m² areal density is a measured hide or garment.
The stock accounting is not the bounding rectangle's area, nor merely the sum of cut pieces.

## Stock identity, geometry and material selection

The input is `ftx-finite-stock-request/1`, with `stocks` in preferred consumption order and
an explicit `options` object. Each stock entry contains:

- `id`: a unique physical inventory identity, available exactly once. Two otherwise identical
  hides require two distinct entries. `options.copies` multiplies garments, not stock availability.
- `material`: the same portable `Selection` used by the pattern's effective default/per-piece
  material. The resolved observation hash **and** estimate policy must match. A different finish,
  lot or updated observation with the same display name is not interchangeable inventory.
- `outline_mm`: a simple polygon in a declared local mm frame, either winding, without a repeated
  closing vertex. Notches/edge damage in the hide belong in this actual outside boundary.
- `defects_mm`: optional disjoint interior polygons strictly inside that boundary. Overlapping or
  nested exclusions refuse rather than double-subtracting their area. Mark all unusable zones;
  the planner does not detect defects or infer a hide's quality distribution from a photograph.
- `grain_mm`: two distinct points describing a directed preferred cutting axis. For hides this
  is an authored orientation constraint, **not** a claim that leather has a woven warp direction.
- `geometry_source`: source `reference`, `accessed_on` date and `note` explaining the boundary,
  units, exclusions and cutting axis. These are caller-supplied evidence, not verified metrology.

`options` requires `copies`, `gap_mm`, `boundary_margin_mm`, `step_mm` and `work_limit`; it also
accepts `seam_allowance_mm`. Use the executable fixture for complete syntax. Margins/gaps/allowances
must be finite and nonnegative; step must be positive. The stock boundary and every defect use
`boundary_margin_mm`, while piece-to-piece spacing uses `gap_mm`. There is no guessed kerf,
shrinkage, presser-foot compression, tolerance or quality-dependent clearance.

Selections are resolved through the canonical textile authority; there is no second hide material
schema or fiber-name constitutive shortcut. The output freezes the stock selections, including
observations, sources and evidence permission. Replay the emitted `request` against the same
original pattern. For a wholly portable workflow, freeze the **pattern** selections as well using
the existing source-piece binding API/workbench. A changed live pattern material cannot silently
reuse the old stock lock: it fails exact material matching. Preserve Rust JSON text across browser
round trips, as documented in `browser-material-workbench.md`.

## Geometry and search contract

The planner shares `Ctx::geom`, cutter outlines and placement transforms with roll cutting.
Source quantities, mirror pairs, on-fold unfolding, seam allowances, notch tabs, curves, internal
cut paths and drill discs are retained. Simulation-only omission or scaling does not change
manufacturing pieces. Each pattern piece needs an explicit nonzero directed grain/cutting axis;
a generated display grain is not accepted as a manufacturing constraint.

Each stock boundary is rigidly rotated to a +y cutting-axis frame; `source_to_marker` records the
transform from its authored coordinates. Piece transforms map expanded instance coordinates into
that same frame. Translation preserves handedness. No reflection, crossgrain rotation or 180-degree
reversal is inferred. Preview coordinates may be negative; the SVG viewBox preserves their mm scale.

Search places the largest exterior areas first, then tries stocks in input order and candidate
positions in bottom-left lattice order. `step_mm` sets that lattice spacing. Every accepted
candidate is checked against the **entire** outside polygon and all defects and placed pieces.
Exact orientation predicates decide crossings; distances enforce Euclidean margins. This catches
edges crossing an outside concavity even when all their vertices lie inside, and defects completely
enclosed by a proposed piece. Candidate sampling is never used as a substitute for collision checks.
Internal holes remain occupied: they are not pockets into which defects or other pieces may be hidden.
Internal cuts and drill discs must lie strictly inside their external cut outline; boundary-ending
slits are conservatively refused in this initial finite-stock mode.

A placement may be missed by this bounded heuristic. Failure means **no checked complete layout
was found**, not proof that the available stock is insufficient. Refine `step_mm`, adjust stock
order or supply additional matching inventory. No partial marker is returned when any piece cannot
be placed or the work budget is exhausted. The original inputs are unchanged on both success and
failure. Limits: 128 physical stocks, 4096 expanded instances/copies, 32768 total stock vertices,
4096 vertices per stock ring, 256 defects per item, and at most 20 million work units for validation
and search. Tiny steps may hit a budget before a valid layout is found; no results are truncated.

## Output and previews

`ftx-finite-stock-plan/1` includes pattern/request hashes, the frozen stock request, all inventory
markers (including unused items), placements, work counters and explicit limitations. Each marker
reports whole polygon area, area minus disjoint defects, allocated external cut area, and utilization
relative to whole stock. Used-stock totals count an individually used hide once, not once per piece.
Mass is optional and comes from admitted areal density times the whole stock area. Missing density
produces `stock_mass_kg: null` with a refusal explaining why; it is never zero or thickness times an
inferred bulk density. These areas are not a recovered-offcut inventory or a hide purchasing quote.

The SVG previews the already-computed geometry, with separate boundary, defect, exterior-cut,
internal-cut, drill, grain and label groups. Stock boundaries and exclusions are **not cutting paths**.
Labels are escaped, paths use six-decimal mm formatting, and no scripts or external resources are
included. It is a review file, not machine coordinates, a cutting-order program, calibrated tool
compensation, or permission to operate hardware. Numeric JSON retains the full computed coordinates.
Preview output is bounded to one million rendered points and 64 MiB.

## Browser integration

The file-free APIs `stock_plan(pattern_json, request_json)` and
`stock_svg(pattern_json, request_json, stock_index)` call exactly the same library; both are exported
by wasm-bindgen. In `web/materials.html`, bind current materials, load the finite-stock request and
choose **Allocate finite stock & preview**. The complete JSON covers all items; the bounded page
preview shows the first allocated stock. Use the API/CLI index to inspect any other item.

Stock-map edits invalidate only finite-stock outputs; material or pattern edits invalidate all
material-dependent consumers. Cancellation terminates the worker, and late file/worker results
cannot restore a stale layout. The page preserves original numeric JSON for sources and downloads.
This feature neither invokes roll procurement for a hide nor treats a roll price as its cost.

## Acceptance and next product gates

`ftx-export/tests/finite_stock.rs` plus shared geometry tests exercise irregular stock, concavities,
contained defects, exact physical quantities, material substitution, curved/mirrored/on-fold export
geometry, explicit axes, missing mass and invalid/budget refusals. `ftx-cli/tests/finite_stock_workflow.rs`
executes both real binaries, verifies library equality/replay and create-only outputs.
`ftx-wasm/tests/finite_stock.rs` checks native file-free equivalence, preview coordinates/escaping,
source locks and budgets. The actual page/worker modules have lifecycle tests with explicit DOM/WASM
doubles; these do not constitute a model-backed graphical browser or physical cutter validation.

This advances the finite-stock part of plan §25.4 / `ftx-tm7`; it does not close that broader gate.
Next substantial work is a calibrated hide/defect acquisition workflow, sourced local quality/stretch
zones with allowed orientations, more effective finite-stock optimization, validated residual-offcut
inventory, and exact per-hide purchase terms. Hardware handoff additionally needs calibrated frames,
kinematics, collision checks, workholding, sensing, interlocks and operator-approved machine limits.
These require product code and independent physical evidence, not a claim of optimality or safety
from a synthetic marker test. `hardware_execution_authorized` and `optimality_claim` remain false.
