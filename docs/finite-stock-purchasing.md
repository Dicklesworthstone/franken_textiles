# Purchasing exact hides and remnants

`stock-price` prices the physical items used by a complete finite-stock allocation. It reuses the
existing material quote engine for observation history, dates, intraday instants, expiry, capacity,
currency and comparable terms. It does not treat a hide as fungible roll stock, infer missing prices,
reserve inventory, or invent missing costs. The same stock planner can now explicitly optimize
the layout for admitted whole-item purchase costs, as described below.

## Run the synthetic example

```sh
cargo run -p ftx-cli --bin ftx-materials -- stock-identities \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  tests/fixtures/materials/synthetic-stock-request.json

cargo run -p ftx-cli --bin ftx-materials -- stock-price \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  tests/fixtures/materials/synthetic-stock-request.json \
  tests/fixtures/materials/synthetic-stock-pricing.json
```

The example is entirely synthetic: both cutters use one miniature L-shaped stock item, whose
declared whole-item price is 17 USD. That item is charged once, not once per cutter. Its geometry,
material and price are not a measured hide or a live supplier observation.

Both commands also run through `ftx materials`, with optional `--json`. The primary executable wraps
results in its usual `ftx-cli/1` envelope; the companion emits the result directly. Neither command
modifies input files. All input files together are limited to 64 MiB, followed by the existing stock
and quote limits. A failed allocation cannot publish a purchasing result.

## Bind observations to a physical item

`stock-identities` returns an array in the original inventory order, including unused items. Each
entry contains the physical ID, material digest, gross area and `stock_sha256`: a digest of the frozen
stock entry, including geometry, source, material policy, zones and already-consumed regions. It is
an integrity identifier, not proof that someone measured or owns the stock.

The pricing document has schema `ftx-finite-stock-pricing/1`, an existing `QuoteBook` in `quotes`, a
comparable `market`, and `bindings`. Each binding explicitly links one `stock_id` and its
`stock_sha256` to one or more alternative `offer_ids` from that book. An offer history cannot be
bound to two physical items. The supplier/stock association is caller-supplied evidence; the program
does not authenticate it or invent it from a product name. See the executable fixture for full syntax.

Every admitted whole-item quote must carry the exact material digest and explicitly normalize the
source's whole-item price to the known stock area: `unit` is `square_meter`, while `quantity`,
`minimum_quantity` and `order_increment` each equal the identity's **gross area**. `price` remains
the price of that one whole item, not a price per square metre. Use the returned numeric area without
rounding it. Defect-free area, cutter area, linear-yard rates and a generic leather product price are
not interchangeable with this item. No width or yield is inferred.

When capacity is observed, `available_quantity` represents the same area and cannot exceed one
physical item. `market.require_observed_quantity` excludes unknown capacity. Without that policy,
an otherwise eligible quote can be retained with unknown capacity, but the report explicitly marks
`complete_observed_quantity_coverage` false.

The current whole-item normalization is deliberately strict. It is not a generic parser for arbitrary
per-hide supplier selling units. A real source must establish that the offer describes this exact
physical item and its whole-item terms before authoring the normalized record.

## History, missing data and replay

The original quote engine selects the latest applicable observation before admission. An expired,
unavailable, incompatible or unlocked latest observation cannot revive an older price. Calendar
books and nanosecond-qualified UTC intraday books retain their original contracts. A failed offer
history is reported independently and does not erase another eligible offer for the same item.

Each used item is charged once. Unused inventory needs no price. A missing binding, changed stock
digest or absent eligible offer leaves that item's cost unknown; any unknown required cost makes
the total `null`, never zero or a misleading partial sum. Source exclusions and per-history refusals
remain visible. Equal costs use deterministic offer-ID tie breaking.

The result retains the original pricing request and hashes of that request and the exact complete
plan. Replay uses the same pattern, frozen stock request and pricing inputs, preserving original
Rust JSON text. A residual stock request changes consumed geometry and therefore invalidates the
old price binding: deliberately characterize and rebind the remaining physical item instead of
carrying its original whole-hide price forward silently.

## Cost-directed allocation

The stock request accepts `options.optimize: "minimize_purchase_cost"` with an explicit
`backtrack_limit` and a top-level `pricing` object using the existing
`ftx-finite-stock-pricing/1` contract above. Other objectives must omit `pricing` so an unused
commercial policy is never silently accepted. Obtain the exact stock identities using the same
inventory in a geometry-only request before deliberately binding its offers.

```sh
cargo run -p ftx-cli --bin ftx-materials -- stock-plan \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  tests/fixtures/materials/synthetic-stock-cost.json
```

This miniature synthetic fixture offers one large sheet for 100 USD and two small remnants for
3 and 4 USD. The cost objective selects both remnants for 7 USD, although the stock-count objective
would choose one large sheet. With other prices, cost minimization may instead prefer the large
sheet over the area-minimizing pair. Each chosen item is charged once, not per cutter. These values
are explicitly synthetic; they establish no live supplier price or measured production savings.

Every candidate item's offers are evaluated **once before search** by the existing quote evaluator.
No initial first-fit layout is needed to qualify prices. Unknown, stale, unavailable, changed-lock,
insufficient-capacity or otherwise unqualified items are excluded from cost search, not assigned
zero or an estimated substitute price. A first-fit layout using an excluded item does not prevent
another, fully priced layout from succeeding. The final `purchasing` section retains all candidate
offers and exclusions, actual used-item flags, the pricing-input digest, currency and selected total.
Its `comparison_scope` is `admitted_whole_item_offers_only`: unknown offers are not proved more
expensive or physically unusable. Unknown capacity can be admitted only under the existing market
policy, and keeps `complete_observed_quantity_coverage` false.

The geometric acceptance authority is unchanged: complete cutter paths must satisfy exact material,
region, defect, prior-cut, clearance and orientation constraints. `optimization.baseline.purchase_cost`
and `optimization.selected.purchase_cost` use the reported currency. Ties retain the first equal
computed cost, without a hidden area/count objective. The existing quote arithmetic is retained;
this does not introduce monetary settlement or currency-rounding rules.

Nonnegative whole-item costs permit monotone lower-bound pruning. Once a complete priced layout
exists, work or backtrack exhaustion retains that complete incumbent; before then exhaustion refuses.
`lattice_optimal` refers only to the declared lattice/orientation choices **among admitted priced
items**, with independent per-item orders and the fixed as-of inputs. It is not a continuous-space,
shipping/bundle, future-market or reusable-offcut-value optimum. Existing quote/link caps apply to
the one pricing pass, independently of the geometry work and undo limits.

Frozen replay retains the original pricing inputs and locks. Residual requests retain those locks
without rewriting them: newly consumed items require deliberate requalification, while unchanged
unused stock can retain its existing eligible offers. `stock-price` can re-evaluate the selected
layout with other observations, but does not retroactively re-optimize it.

The ordinary native commands, file-free APIs and material-study stock consumer all call this same
planner. The single-search `stock_job` additionally retains exact Rust `purchasing_json` text; the
material worker publishes it as `stock-purchasing.json`, without rerunning pricing or passing its
receipt-bound numbers through JavaScript serialization. Unpriced jobs retain their old output shape.
The study's existing independent outcomes stay separate; this does not add a new cross-material
cost metric or graphical quote editor. Existing original plan and residual downloads remain authoritative.

## Read the stock result without inspecting JSON

The material workbench's existing **Allocate finite stock & preview** operation also downloads
`stock-review.html`. Open it locally for a self-contained, printable review of the first complete
and selected layouts, the actual objective, whole-item costs, observation instants, comparable terms,
and selected versus excluded inventory. Offer exclusions and source failures remain visible beside
eligible alternatives. An unused item with no admitted price is not presented as free stock or as
proved more expensive. Unpriced jobs explicitly say that costs were not evaluated.

Budget exhaustion is displayed separately from a completed grid search. A grid-only proof is not
labelled a global optimum, and incomplete quantity evidence is not labelled available inventory.
The review displays native values without computing savings, ranking offers again, rounding monetary
settlement, or changing the original JSON. A cost prefixed with `≈` is rounded for display to 12
significant digits, with full precision retained in the original JSON. It runs no scripts and loads no external resources;
source references are escaped text rather than executable markup or network links.

Review output is limited to 4 MiB with a 32,768-character limit per displayed field. An oversized or
inconsistent display produces `stock-review-refusal.json` instead; the already complete plan,
residual request, purchasing receipt and SVG remain available. The HTML is a readable result, not
a pricing editor, replay input, reservation or machine-execution file.

## Portable API and scope

`ftx_wasm::stock_pricing::{stock_identities, stock_price}` exposes the same library without files.
The wasm-bindgen exports have those same names, accept JSON text, and retain original Rust result
text. Their combined input and serialized output budgets are each 64 MiB. Serialization exceeding
the output budget refuses without returning a partial JSON document.

This increment supplies native commands and browser-callable APIs; it does not add graphical pricing
controls to the material workbench. It also does not add supplier records, network refreshes,
supplier-bundle/shipping optimization, currency conversion, accounting rounding rules, inventory
mutation or checkout. Area/count objectives remain distinct from the explicitly requested cost objective.
These are explicit remaining purchasing tasks under `ftx-tm6`/`ftx-tm7`, not claims made by a
successful synthetic price calculation.
