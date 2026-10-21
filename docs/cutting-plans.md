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

The SVG destination must be a new path. The index selects a roll in the returned `rolls` array; each
exact material snapshot has its own roll. Commands read locally and do not fetch prices, reserve goods,
change a pattern, or operate a machine. The primary command uses the usual `ftx-cli/1` JSON envelope;
the companion emits its result directly and prints JSON refusals on stderr with a nonzero exit status.

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
required length, gross stock area, summed outer-piece area and utilization. Gross stock mass is optional:
missing/unapproved density is a diagnostic, not zero mass and not a reason to block width-based planning.

`cut-price` evaluates every material's **gross stock area = width × required roll length**, not merely
summed piece area. Thus waste, margins and shelf gaps reach the purchase requirement automatically.
Linear prices are converted using that same resolved width. Each reported offer includes its
minimum/increment-rounded quantity and merchandise cost. `least_observed_independent_order_total` is
present only when every roll has an eligible offer; otherwise it is null. It is a sum of the lowest
observed whole-order costs per snapshot, not the sum of percentiles or a representative market estimate.
Different snapshots, even sharing a product ID, are conservatively ordered separately.

## Qualifying one uninterrupted cut per marker

Every roll marker requires one uninterrupted full-width segment of its reported `length_m`, including
end margins and layout gaps. `cut-price` passes that computed length into the canonical quote
evaluator alongside gross area. A supplier's `available_quantity` describes total stock; it does not
establish that the marker fits into a single piece. The separate optional quote field
`maximum_continuous_quantity` records a source-stated upper limit in the original linear sale unit.
For example, a five-yard maximum excludes a five-metre marker even if total inventory is hundreds of
yards. The planner does not split the marker or any cut piece to satisfy that limit.

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
