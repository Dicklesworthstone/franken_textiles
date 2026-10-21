# Purchasing exact hides and remnants

`stock-price` prices the physical items used by a complete finite-stock allocation. It reuses the
existing material quote engine for observation history, dates, intraday instants, expiry, capacity,
currency and comparable terms. It does not treat a hide as fungible roll stock, infer missing prices,
reserve inventory, or optimize the layout for purchase cost.

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

## Portable API and scope

`ftx_wasm::stock_pricing::{stock_identities, stock_price}` exposes the same library without files.
The wasm-bindgen exports have those same names, accept JSON text, and retain original Rust result
text. Their combined input and serialized output budgets are each 64 MiB. Serialization exceeding
the output budget refuses without returning a partial JSON document.

This increment supplies native commands and browser-callable APIs; it does not add graphical pricing
controls to the material workbench. It also does not add supplier records, network refreshes,
supplier-bundle/shipping optimization, currency conversion, accounting rounding rules, inventory
mutation or checkout. Area/count-optimized layouts remain distinct from price-optimized layouts.
These are explicit remaining purchasing tasks under `ftx-tm6`/`ftx-tm7`, not claims made by a
successful synthetic price calculation.
