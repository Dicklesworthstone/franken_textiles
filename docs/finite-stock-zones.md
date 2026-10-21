# Named regions for finite hide and remnant cutting

The existing finite-stock planner can require a source piece to lie inside a named region of an
individually identified hide or sheet. A visible front panel can require a reviewed `prime` region
while an internal piece uses any otherwise admissible part of the same stock. This is an authored
placement constraint, not a measured grade, inferred stretch map, or new material constitutive law.
All consumers reuse the same `stock::Request`; there is no separate browser or study planner.

## Run the synthetic example

```sh
cargo run -p ftx-cli --bin ftx -- materials stock-plan \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  tests/fixtures/materials/synthetic-stock-zones.json --json

cargo run -p ftx-cli --bin ftx-materials -- stock-svg \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  tests/fixtures/materials/synthetic-stock-zones.json 0 region-review.svg

cargo run -p ftx-cli --bin ftx-materials -- stock-residual \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  tests/fixtures/materials/synthetic-stock-zones.json > residual-stock.json
```

Use a new preview/output path. `stock-svg` refuses an existing path; ordinary shell redirection is
controlled by the shell, not by the program. The companion executable emits the request directly;
`ftx materials stock-residual --json` wraps it in the usual `ftx-cli/1` result envelope. Extract that
result without a lossy numerical round trip before using it as another stock request.

The fixture is deliberately synthetic and miniature. It retains the original L-shaped stock,
interior defect, exact material selection, front and pocket cutters. It requires only the front to
fit the declared rectangular region; it does not certify the physical suitability of leather.

## Declare regions and requirements

Each stock entry may contain `zones`. Coordinates are in that stock's **original authored mm
frame**, not a second coordinate system. For example:

```json
"zones": [{
  "id": "prime",
  "outline_mm": [[24, 2], [98, 2], [98, 43], [24, 43]],
  "geometry_source": {
    "reference": "test:prime-region",
    "accessed_on": "2026-10-20",
    "note": "Synthetic placement region, not a measured leather grade"
  }
}]
```

The request's existing `options` object adds:

```json
"piece_zones": {"front": "prime"},
"zone_margin_mm": 1
```

Keys are **source-piece IDs**, not expanded mirror/copy IDs. A requirement applies to every cut
instance and every requested garment copy of that source piece. Unmapped pieces remain unrestricted.
Identifiers are matched exactly and region IDs are unique within one stock. A shared name across
several stock items declares the same requested placement category, but never substitutes for the
existing exact material snapshot and evidence-policy match. Missing required regions on matching
stock refuse; the planner does not fall back to another material or silently drop a requirement.

Regions must be simple polygons strictly inside the stock outline, with nonempty source reference,
date and explanatory note. They can overlap each other. They can include existing defects or prior
cut regions, but **those areas remain excluded**. A region adds no usable stock and subtracts no
area: gross, defect-free and available-area accounting retain their original definitions. A name
such as `low-stretch` does not assign a local modulus, grain direction, thermal law or quality score.

`zone_margin_mm` is finite and nonnegative. It applies only to required region boundaries and is
independent of the stock/defect boundary margin and the inter-piece gap. A positive zone margin
without any piece requirement refuses instead of accepting an unused control. Empty region lists,
empty mappings and zero margin are omitted from serialized requests, preserving the no-region
request identity and calculation path.

## Geometry, allocation and limits

The **whole external cutter** must fit the required region, including seam allowances and outward
notch tabs. The shared full-polygon containment test catches edges crossing a concavity even when
vertices or bounding boxes appear to fit. No outline sampling or bounding-box-only acceptance is
introduced. Internal cut holes are not reused as stock pockets.

Region-constrained pieces are allocated before unrestricted pieces, then by the existing decreasing
exterior area and deterministic tie breaks. This prevents a larger unrestricted lining from using a
scarce prime region before a front panel is considered. Stock order remains the caller's preference.
The candidate lattice is restricted to the stock/region bounding-box intersection to avoid searching
unrelated areas; every candidate still faces the full stock, region, defect, prior-cut and other-piece
checks. This is a bounded heuristic, not global optimization or an infeasibility proof.

The existing material-specific `grain_reversal_materials` policy applies unchanged. No region label
permits an unapproved half-turn, crossgrain rotation or reflection. Regions transform with their
stock into the marker frame. Residual requests preserve the original authored regions, sources,
material locks and piece requirements while retaining consumed cutter polygons as occupied areas.
They do not reserve live stock or discover disconnected free offcut polygons.

There are at most 256 regions per stock and 4,096 piece requirements. Region vertices count toward
the shared 32,768-stock-vertex limit and the existing per-ring bounds. Validation and placement use
the same bounded `Work` counter as other finite-stock geometry; material studies additionally bound
the sum of per-alternative work. Malformed inputs or exhausted work publish no partial plan.

## Existing CLI, browser and study integration

Both native command surfaces (`ftx materials` and `ftx-materials`), the file-free `stock_plan`,
`stock_svg` and `stock_residual` APIs, and the local `web/materials.html` workflow accept these fields
through their existing stock input. A material study's `finite_stock` request uses the same rules.
An alternative without the required matching stock region reports its finite-stock refusal; other
independent consumer results, such as sewing, remain visible. A comparison requiring finite stock
excludes that alternative from the eligible frontier.

The material workbench accepts the region-aware request through its existing stock operation.
The downloaded plan retains piece requirements, margin, available areas and region sources.
SVG reviews show regions in a distinct noncutting `stock-zones` group and attach
`data-required-zone` to constrained piece groups. Identifiers and notes are XML-escaped; displayed
sources use text, not interpreted HTML. Original Rust JSON remains the download/replay authority.
Stock/material edits withdraw stale results through the existing dependency tracking. The residual
request download retains the regions for a subsequent job.

## Verification and physical boundary

`ftx-export/tests/finite_stock_zones.rs` covers constrained-first allocation, full concave containment,
overlapping regions, defect/prior-cut exclusion, exact material matching, rotated frames, mirror/copy
expansion, explicit half-turns, residual replay, malformed input, work limits and no-region defaults.
The existing finite-stock CLI and native WASM-boundary suites check exact plan/preview/residual
agreement, unchanged inputs and refusal behavior. The planner is also used by the existing
manufacturing-study consumer; region failure remains an independent finite-stock refusal.

These are synthetic software checks, not hide metrology, measured local mechanical characterization,
physical cutting validation or full graphical browser execution. Acquisition of source-qualified
regions, independent coupon/sewn-part comparisons and machine calibration remain open under
`ftx-tm7`. Region names never authorize hardware execution.
