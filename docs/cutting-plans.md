# Material-driven roll cutting and purchasing

`ftx materials cut-plan` uses the same source-piece material selectors as the drape, but applies the
properties needed for cutting and purchasing: a stock form, sourced width and explicit grain. A material
without measured elasticity can be laid out and priced without inventing a constitutive law.

```sh
cargo run -p ftx-cli --bin ftx -- materials cut-plan pattern.json cut-options.json --json
cargo run -p ftx-cli --bin ftx -- materials cut-price pattern.json cut-options.json quotes.json market.json --json
```

The companion executable exposes the same planner, purchasing evaluator and SVG preview:

```sh
cargo run -p ftx-cli --bin ftx-materials -- cut-plan pattern.json cut-options.json
cargo run -p ftx-cli --bin ftx-materials -- cut-svg pattern.json cut-options.json 0 marker.svg
cargo run -p ftx-cli --bin ftx-materials -- cut-price pattern.json cut-options.json quotes.json market.json
```

The SVG destination must be a new path. The index selects a marker in the returned `rolls` array;
an exact material snapshot can have several markers when a length limit is requested. Commands read
locally and do not fetch prices, reserve goods,
change a pattern, or operate a machine. The primary command uses the usual `ftx-cli/1` JSON envelope;
the companion emits its result directly and prints JSON refusals on stderr with a nonzero exit status.

The material workbench's **Plan rolls & preview** action uses `roll_job` to build one complete plan
and render the selected marker from it. The plan and SVG come from the same bounded search; preview
failure retains `cut-plan.json` plus an explicit `roll-preview-refusal.json`. Invalid marker numbers
still refuse. The standalone `cut-plan` and `cut-svg` commands retain their existing contracts.

## Inputs

A source piece without `fabric` inherits the pattern's `fabric`. A piece override is a complete
`piece.fabric.material` selection (or `piece.fabric.material_id`), with the same source locks and estimate
consent as simulation. `piece.material` remains an informational cutting-table string, not a SKU.
For details of the canonical record and frozen selections, see `material-catalog.md` and `piece-materials.md`.

Example `cut-options.json` (values are explicit allowances, not recommended machine settings):

```json
{
  "copies": 1,
  "gap_mm": 5,
  "edge_margin_mm": 10,
  "end_margin_mm": 20,
  "seam_allowance_mm": 8
}
```

Omit `seam_allowance_mm` to use the existing export allowance hierarchy. `copies` counts complete
garment sets using the pattern's own physical instances, not extra mirroring. Mirror pairs
and on-fold pieces use the existing cutter's expansion; on-fold pieces are unfolded onto single-ply
stock. `sim:false`, simulation-only scaling and missing body placements never discard or resize cut parts.

## Material-qualified cold-knife programs

Add `program` to the same cutting options to obtain a complete traversal and modeled feed/travel
time from the **final** nested or partitioned marker. Both CLIs, the portable/WASM cut planner,
material studies and the roll side of mixed cutting jobs use the same optional consumer. Omission
preserves ordinary marker output and needs no process or thickness data.

Process admission is independent of valid marker geometry. If the requested program lacks matching
evidence or cannot admit its paths, the result retains every ordinary marker and adds a structured
`program_refusal`; `program` is absent. Stock requirements and purchasing remain usable, but no
qualified cutting traversal is implied. A complete accepted program instead supplies `program` and
omits `program_refusal`. Invalid basic geometry, scale, width, grain or cutting options still refuse
the cut plan itself. Material studies keep these same independent outcomes after each substitution.

The model is `qualified_cold_knife_paths/1`. Its `machine` declares a cold blade, rectangular bed,
maximum sheet thickness and cutting feed, raised-tool travel speed, complete lift/lower-cycle
duration, starting head coordinate and whether to return there. These are explicit planning inputs,
not recommended settings. For example, this is a **synthetic software fixture**, not a qualified
real machine:

```json
{
  "model": "qualified_cold_knife_paths/1",
  "machine": {
    "id": "synthetic-table",
    "blade": "synthetic-cold-knife",
    "bed_mm": [500, 500],
    "maximum_thickness_mm": 2,
    "maximum_feed_mm_s": 100,
    "travel_mm_s": 100,
    "lift_lower_seconds": 0.25,
    "start_mm": [0, 0],
    "return_to_start": true
  },
  "qualifications": [],
  "ordering": "nearest_path",
  "work_limit": 10000
}
```

The empty `qualifications` list intentionally refuses. Supply independently obtained records for
the exact material snapshots being cut. Each record needs `id`, `material_sha256`, the complete
`machine_sha256`, a positive `thickness_mm` interval, a qualified selected `feed_mm_s`,
`directions: "all_in_plane"`, `source`, `method`, `conditions` and `evidence`. Estimated records
also require their own `allow_estimates: true`; material estimate consent does not authorize
estimated process evidence. `cutting::program::machine_sha256` computes the lock from the complete
typed configuration. Reusing a machine name after changing its blade, bed, feed, travel or lift
timing does not preserve applicability. Exactly one recipe per material is admitted, so no recipe
is silently selected from ambiguous alternatives.

The selected material supplies its ordinary source-qualified uncompressed thickness, including the
**entire** reported interval when present. Both ends must fit the recipe, and the upper end must fit
the machine. Missing thickness and pressure-conditioned sheet thickness refuse. Retained-liquid
materials use the existing conditioned-property admission. Changing the textile, its finish or its
source record invalidates the old material lock; a newly applicable recipe can then supply a different
cutting feed while all geometry and planning code stay unchanged. An all-direction recipe is required
because a closed outline cuts across more than one grain direction.

Every internal cut finishes before any exterior outline releases a piece. The program references
`rolls[marker].placements[placement].internal_cut_mm[index]` or that placement's `cut_mm`; it never
regenerates, rounds, reverses, rotates the start of or compensates those paths. It preserves source
piece, instance and physical copy IDs. Explicitly closed cutouts and open slits must be simple,
strictly enclosed and mutually disjoint. Nested cutouts, slits in removed regions, crossing cuts,
ambiguous closure and drills refuse instead of disappearing from the output. Drill marks are not
silently converted to knife circles. The complete marker must fit the declared bed.
The loaded stock uses its reported upper width when present; choosing a lower-width layout does
not make the physical roll narrower or authorize an overhanging stock envelope.

`program.markers` retains the complete authored two-phase baseline and selected traversal, material
and geometry hashes, applied recipe, thickness interval and every raised-tool travel segment.
`ordering: "nearest_path"` compares unchanged path starts within each phase under one shared bounded
candidate budget. An unfinished candidate or a candidate with longer travel retains the complete
baseline; the heuristic never claims a shortest-path proof. `ordering: "authored"` performs no
search. Optional `geometry_work_limit` bounds validation separately (one million by default,
at most twenty million); exhausting validation refuses instead of emitting an unchecked path.
Both budgets are shared by every marker. Material/design studies reserve the complete configured
limits across all alternatives or adaptive probe slots, capped at two million search comparisons and
twenty million geometry checks in total. Material width and nesting still determine purchasing
independently of this traversal.

For each traversal, `cutting_seconds` is complete path length divided by the qualified feed rate;
`travel_seconds` uses raised-tool distance and the locked travel rate; `lift_lower_seconds` counts
one complete declared cycle per cut. `modeled_path_seconds` is their sum. It excludes acceleration,
corner slowing, blade turning, loading, inspection and workholding changes, so it is **not a measured
production cycle time**. The output always reports false physical-cutting validation and hardware
authorization. It contains no G-code, laser/hot-wire processing, kerf compensation, collision or
interlock approval. Numerical source admission and synthetic tests do not establish real cut quality.

## Dated purchasing observations

`market.json` defines a comparable price sample, not a user-entered fabric requirement:

```json
{
  "currency": "USD",
  "region": "US",
  "quality": "first-quality",
  "terms": "merchandise-ex-tax-shipping",
  "as_of": "2026-10-14",
  "max_age_days": 7
}
```

`quotes.json` is a `ftx-textile-quotes/1` calendar-date observation book or a `/2` intraday book. For
version 2 add `as_of_at: "2026-10-14T12:00:00Z"` to the market above, with the same UTC date as `as_of`.
Every version-2 quote must retain its actual `observed_at`; date-only records are not assigned midnight.
The newest eligible-time observation controls the whole rounded order, including later stock loss or
expiry. See [textile-price-refresh.md](textile-price-refresh.md#intraday-histories-through-the-existing-procurement-and-study-consumers)
for timestamp precision and inclusive expiry. The book can come from the source adapter
in `textile-price-refresh.md`. Dates, availability, exact material/SKU, currency, region, quality, terms,
minimum quantities, sale quantities and order increments retain that evaluator's checks. No synthetic
or stale quote becomes a live supplier observation. A `requirement` field in this market file is rejected:
the command derives requirements from the actual cut plan.

## What is planned

The planner uses the external cutter polygon, including seam allowances and outward notch tabs. Its
initial shelf layout rotates all directed grainlines to +y along the roll. Optional nesting permits
180-degree reversal only for explicitly selected material snapshots, as described below; crossgrain
rotation remains unsupported. Missing/generated/default grain is insufficient. All material-frame
transforms, internal cut paths and drills are retained.

Pieces are sorted by decreasing bounding-box height/width with deterministic identity tie-breaks, and
placed in the first shelf with room. This is a bounded, non-overlapping rectangular-envelope layout,
**not optimal polygon nesting**. Optional shape-aware compaction is described below. It reports each placement and each material's nominal stock width,
plus each marker's required length, gross stock area, summed outer-piece area and utilization. Gross stock mass is optional:
missing/unapproved density is a diagnostic, not zero mass and not a reason to block width-based planning.

`cut-price` evaluates every material's **gross stock area = width × required roll length**, not merely
summed piece area. Thus waste, margins and shelf gaps reach the purchase requirement automatically.
Linear prices are converted using that same resolved width. Each reported offer includes its
minimum/increment-rounded quantity and merchandise cost. `least_observed_independent_order_total` is
present only when every material has an eligible whole-order offer; otherwise it is null. It is a sum
of the lowest
observed whole-order costs per snapshot, not the sum of percentiles or a representative market estimate.
Different snapshots, even sharing a product ID, are conservatively ordered separately.
Their offers must also represent independent stock. Base and quantity-tier SKUs from the same
supplier form transitive inventory aliases across the latest observations in the declared market.
Links through unused or unavailable offers still identify the same stock; future and superseded
observations do not. If one component offers stock to more than one required material snapshot,
its eligible offers stay in the canonical price samples but cannot supply independent orders.
Affected orders list `excluded_shared_stock_offer_ids`; another unambiguous offer may supply the
material. Otherwise the complete total and observed-stock coverage remain unavailable, preserving
the cut geometry and per-material price diagnostics. No cross-revision stock allocation is inferred.

## Planning at the source-reported lower width

Set `width_basis` to `reported_lower_bound` to fit the complete marker inside the lower end of each
material's reported width range. For example:

```json
{
  "copies": 3,
  "gap_mm": 5,
  "edge_margin_mm": 5,
  "end_margin_mm": 10,
  "maximum_marker_length_m": 0.12,
  "width_basis": "reported_lower_bound"
}
```

Each consumed material must have explicit `properties.width.bounds` in the width observation's unit.
The resolver normalizes the value and both bounds together; the range must be finite, positive and
contain nominal width. Width provenance and estimate consent still apply. Missing bounds refuse this
planning mode instead of substituting nominal width. Omitting `width_basis`, or setting it to
`nominal`, preserves the existing layout and output format.

Both edge margins are subtracted from the admitted lower width. Shelf placement, optional polygon
compaction and bounded-marker partitioning all use that narrower space; complete pieces can move to
additional shelves or markers, and a cutter too wide for it refuses. This is a conditional fit with
fixed cutter geometry and margins, not a confidence interval or a guarantee about delivered stock.

Each marker retains nominal `width_m` and adds `layout_width.width_m`,
`layout_width.reported_bounds_m` and explanatory `layout_width.notes`. Gross `stock_area_m2`,
`stock_mass_kg`, utilization and purchase-unit conversions use **nominal width and the newly planned
length**. Source width bounds do not become area, mass or cost intervals. For example, three 0.12 m
markers of nominally 0.1 m stock require 0.036 m² and 0.36 linear metres before order rounding, even
when their layout width is 0.075 m. Finite hide/remnant outlines and inventory are unchanged; in a
mixed cutting job this option belongs to the roll route.

In the browser, load these cutting options and use **Plan roll cuts**. Its marker summary retains
both widths, and the SVG shows the lower-width layout envelope inside the nominal stock rectangle.
Select **Preview marker number** for any additional marker and rerun the planner. **Compare
whole-order costs** uses all resulting marker lengths with the same nominal purchase-width basis.

## Bounded marker lengths and multiple cuts

Add `maximum_marker_length_m` when each marker must fit a declared cutting-table or continuous-cut
length. This is an explicit job constraint; it does not infer a supplier limit or guarantee available
stock. For example, this options file requests three complete garment sets with each marker no longer
than 2.5 metres, including its end margins:

```json
{
  "copies": 3,
  "gap_mm": 5,
  "edge_margin_mm": 10,
  "end_margin_mm": 20,
  "seam_allowance_mm": 8,
  "maximum_marker_length_m": 2.5
}
```

The existing commands consume this same `cut-options.json`:

```sh
cargo run -p ftx-cli --bin ftx -- materials cut-plan pattern.json cut-options.json --json
cargo run -p ftx-cli --bin ftx -- materials cut-price pattern.json cut-options.json quotes.json market.json --json
```

Inspect the returned `rolls` array. If it contains a second marker, render that marker with its zero-based
index and a new destination:

```sh
cargo run -p ftx-cli --bin ftx-materials -- cut-svg pattern.json cut-options.json 1 second-marker.svg
```

The planner assigns **whole physical cut instances** to deterministic first-fit shelves across
separate markers. Copies, mirrors, on-fold expansion, notches, internal cuts, drills and material hashes
retain their ordinary meaning. A piece whose cutter height plus both end margins exceeds the limit
refuses; the planner does not split the piece, insert a seam or divide its area into nominal purchases.
Every new marker has its own end margins, so splitting can increase total required stock. Optional
polygon compaction runs within each assigned marker and preserves its limit. Marker count and total
length are heuristic; the search does not claim a globally optimal partition or move pieces between
markers during compaction. The existing 4,096-instance job limit still applies.

Supplying the option produces `ftx-roll-cut-plan/2`, even if everything fits on one marker. Omitting
it preserves `ftx-roll-cut-plan/1`, one unbounded marker per exact material, and the previous options
serialization.

### One purchasing order for all markers of the same material

In the browser material workbench, load and bind the pattern, then select the same cutting-options,
quotes and market JSON files. **Plan roll cuts** lists the markers; **Preview marker number** uses
1 for the first marker, 2 for the second, and so on. Run the planner again to download that intact
marker's SVG. Changing only the preview number withdraws its old image without discarding the
purchasing result. Changing cutting options, quotes or market inputs invalidates affected results.

**Compare whole-order costs** displays each material order once, identifies its member markers,
and distinguishes their total length from the longest uninterrupted segment. Quantities, costs
and exclusions come directly from the shared purchasing evaluator. Download `cut-price.json`
to retain the full order/marker correspondence and dated source observations.

When an exact material occupies several markers, `cut-price` evaluates their **summed gross stock area**
as one whole order through the existing quote authority. Minimum quantities, order increments and
observed total stock apply once to that total. The **longest actual marker** determines the separate
continuous-cut upper-limit check. A source's stock cannot be counted independently for each marker.
For example, two 0.12 m markers require 0.24 m in total. A whole-order increment of 0.05 m rounds that
requirement to 0.25 m; an offer with only 0.20 m observed stock is insufficient even though either
individual marker would fit. A 0.12 m continuous-cut upper limit can admit both requested marker lengths
without implying that a 0.25 m uninterrupted piece is available.

If any material has multiple markers, the purchasing report uses `ftx-cut-procurement/2`:

| Field | Meaning |
|---|---|
| `material_orders[]` | One price sample and selected whole order per exact material snapshot |
| `material_orders[].roll_indices` | Indices of all markers supplied by that order |
| `material_orders[].marker_lengths_m` | Actual requested marker lengths, including their margins |
| `material_orders[].required_length_m` / `required_stock_area_m2` | Summed length and gross area for the material |
| `material_orders[].required_continuous_length_m` | Longest actual marker, used for the source upper-limit check |
| `roll_prices[].material_order_index` | Link from each marker's geometry requirements to its purchasing order |

`least_observed_order` and `sample` appear only on `material_orders` in this version; marker rows do
not repeat order costs. The top-level `least_observed_independent_order_total` sums the selected material
orders once and remains null if any required material lacks an eligible whole-order offer. When every
material has only one marker, procurement retains `/1` with its existing per-marker order and sample
fields, including when a marker-length limit was supplied but did not require partitioning.

Quote minimums and increments describe the **whole order**, not an unreported per-cut charging rule.
The marker list is a requested segmentation. Neither sufficient total stock nor a continuous-cut upper
limit establishes that the supplier can or will deliver that segmentation; no purchase or reservation
is executed.

## Qualifying one uninterrupted cut per marker

Every roll marker requires one uninterrupted full-width segment of its reported `length_m`, including
end margins and layout gaps. `cut-price` passes the longest actual marker for each material into the
canonical quote evaluator alongside that material's total gross area. A supplier's `available_quantity`
describes total stock; it does not
establish that the marker fits into a single piece. The separate optional quote field
`maximum_continuous_quantity` records a source-stated upper limit in the original linear sale unit.
For example, a five-yard maximum excludes a five-metre marker even if total inventory is hundreds of
yards. Procurement never changes the computed layout or divides a cut piece to satisfy that limit.
Request bounded marker planning explicitly as described above when separate markers are appropriate.

Known limits are always enforced. To also exclude sources whose continuous-cut limit is unknown,
set `require_observed_continuous_length` in the roll market. This complete example additionally
requires observed total stock for each rounded order; its market labels must match your actual quotes:

```json
{
  "currency": "USD",
  "region": "US",
  "quality": "first-quality",
  "terms": "merchandise-ex-tax-shipping",
  "as_of": "2026-10-21",
  "as_of_at": "2026-10-21T16:00:00Z",
  "max_age_days": 7,
  "require_observed_quantity": true,
  "require_observed_continuous_length": true
}
```

Use `as_of_at` for a version-2 book and omit it for version 1. Do not add `continuous_length_m` or a
quantity to this market: both requirements come from the actual marker. The continuity policy applies
to roll purchasing; finite hide/remnant purchasing has no uninterrupted roll segment to qualify.

Omitting the policy or setting it to `false` preserves price comparison when the source limit is
unknown, while making that uncertainty explicit. Each `roll_prices[].continuity` is
`within_observed_limit`, `source_limit_unknown`, or `no_eligible_offer`.
`complete_continuous_length_limit_coverage` describes the selected least-cost orders independently
of `complete_observed_quantity_coverage`. Accepted known-limit offers carry
`within_observed_continuous_limit: true`; unknown limits omit that field. Neither a known upper
limit nor adequate total stock guarantees an uninterrupted piece is actually available.

Excluded observations retain `insufficient_continuous_length` or `unknown_continuous_length`, the
required length in metres, and the original quote/source hashes. If any roll has no eligible offer,
the total remains null. The latest observation controls: a newer shorter or missing limit cannot
revive an older permissive one. Native commands, WASM purchasing and material studies share these
checks. See [the refresh guide](textile-price-refresh.md#source-qualified-continuous-cut-limits) for
source qualification and the dated APEX observation, whose catalog form remains fill rather than
supported roll-cutting stock.

## Shape-aware nesting

Add `"nesting": {"passes": 3, "work_limit": 1000000}` to the same cut options to search for better
placements using actual cutter outlines, including concavities and outward notch tabs. Omitting
`nesting` retains the shelf algorithm. An empty nesting object requests the defaults shown above.
This is a bounded bottom-left compaction heuristic, not an optimal no-fit-polygon solver.
By default it preserves each directed grainline. It always preserves mirror identity, cut path,
internal path, drill diameters and the mapping from the original instance.
Interior cut holes are still occupied: the planner does not put another piece inside a hole.

For fabric whose nap and print direction permit a half-turn, add
`"grain_reversal_materials": ["<exact resolved material SHA-256>"]` inside `nesting`.
Use the canonical material resolver's `sha256` or the preceding plan's `rolls[].material.sha256`.
Each listed snapshot must be unique and actually used by the job; stale permissions after a
material or finish change refuse. Product/fiber names never grant reversal. Other materials retain
one-way grain, and no reflection or crossgrain rotation is introduced. With permission, each move
compares the original and 180-degree orientations and selects the lowest checked position, then
the leftmost; ties keep the current direction. A half-turn must still improve the position.
This bounded search may miss an arrangement requiring an initial move upward or an in-place turn.

Reversed placements have `grain_reversed: true`, a grainline directed toward -y, and the complete
half-turn in their affine transform. The SVG labels their groups with
`data-grain-reversed="true"` and draws the transformed grain arrow. Full-resolution clearance
checks apply to both directions. The reversal-enabled search identifies itself as
`bounded_polygon_compaction/2`; the default translation search retains `/1`.

The complete shelf marker is the initial feasible solution. Each proposed relocation lowers the
piece's bottom edge, or moves it left at the same height, without increasing its top. Exact orientation
predicates reject segment crossings/contact; full-resolution edge distances and containment tests
check the requested Euclidean outline gap. No outline simplification is used for clearance checks.
Up to 16 vertices per outline generate candidate anchors; at most 256 coordinates per axis are retained.
This can miss good placements, but cannot justify accepting an unchecked one. New contact between
overlapping polygon bounds is conservatively rejected even with a zero requested gap.

The per-roll `nesting` report states the shelf baseline, final length and savings, accepted moves,
work consumed, and whether the search budget was exhausted. Budget exhaustion retains the last
fully checked, complete layout, never partially placed garment copies. `passes` is in 1..8 and
`work_limit` in 1..20000000 per roll; work includes anchor pairs, candidate vertices, broad-phase
pairs, and edge/containment checks. It is not a runtime or optimality guarantee. Prices, gross mass,
utilization and SVG preview all consume the resulting marker rather than the old shelf requirements.
The numerical gap checks are not a kerf, workholding, fabric-distortion or machine-safety guarantee.

## Scope and limits

Supported stock is rectangular woven, knit, nonwoven or laminate sheet rolls. Hides, yarns and loose fill
are refused rather than assigned fictitious rectangular stock dimensions. Separate physical layers need
explicit cut pieces; no implicit lining/fill generation. Limits are 4096 expanded cut pieces and garment
copies in 1..4096. Invalid units, widths, margins, partial selectors, unscaled patterns and pieces wider
than usable stock refuse rather than shrinking or rotating crossgrain.

The SVG uses the computed marker directly in millimetres, with distinct exterior/interior, drill, grain
and label groups, escaped text and source hashes. It includes no scripts or remote resources. This is a
planning preview, not compensated cutter commands, a tool-order program or a validated process recipe.

Not modeled: defects, hide outlines, plaid/repeat matching, shrinkage, automatic hole/offcut reuse,
seam/robot planning, supplier shipping aggregation, bundled discounts, inventory quantity reservations,
cutting kerf, workholding or machine interlocks. Source bounds remain attached but uncertainty is not
propagated into a guaranteed usable yield. Geometric layout is not material/process safety qualification.
