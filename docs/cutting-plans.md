# Material-driven roll cutting and purchasing

`ftx-materials cut-plan` uses the same source-piece material selectors as the drape, but applies the
properties needed for cutting and purchasing: a stock form, sourced width and explicit grain. A material
without measured elasticity can be laid out and priced without inventing a constitutive law.

```sh
cargo run -p ftx-cli --bin ftx-materials -- cut-plan pattern.json cut-options.json
cargo run -p ftx-cli --bin ftx-materials -- cut-svg pattern.json cut-options.json 0 marker.svg
cargo run -p ftx-cli --bin ftx-materials -- cut-price pattern.json cut-options.json quotes.json market.json
```

The SVG destination must be a new path. The index selects a roll in the returned `rolls` array; each
exact material snapshot has its own roll. Commands read locally and do not fetch prices, reserve goods,
change a pattern, or operate a machine. JSON errors are printed on stderr with a nonzero exit status.

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

`quotes.json` is the existing `ftx-textile-quotes/1` observation book. It can come from the source adapter
in `textile-price-refresh.md`. Dates, availability, exact material/SKU, currency, region, quality, terms,
minimum quantities, sale quantities and order increments retain that evaluator's checks. No synthetic
or stale quote becomes a live supplier observation. A `requirement` field in this market file is rejected:
the command derives requirements from the actual cut plan.

## What is planned

The planner uses the external cutter polygon, including seam allowances and outward notch tabs. It
rotates all directed grainlines to +y along the roll, preserving one-way nap direction rather than
silently choosing a 180-degree reversal or a crossgrain rotation to fit. Missing/generated/default grain
is insufficient. All material-frame transforms, internal cut paths and drills are retained.

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

## Shape-aware nesting

Add `"nesting": {"passes": 3, "work_limit": 1000000}` to the same cut options to search for better
placements using actual cutter outlines, including concavities and outward notch tabs. Omitting
`nesting` retains the shelf algorithm. An empty nesting object requests the defaults shown above.
This is a translation-only bottom-left compaction heuristic, not an optimal no-fit-polygon solver.
It preserves each directed grainline, mirror identity, cut path, internal path, drill and transform.
Interior cut holes are still occupied: the planner does not put another piece inside a hole.

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
