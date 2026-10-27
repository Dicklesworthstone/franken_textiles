# Live quarter-yard pricing and finite supplier inventory

The existing textile refresh script now admits a narrowly qualified Fabric Mart public-product
representation. It connects an exact activewear-knit catalog identity to ordinary source-locked
quote evaluation, minimum-order rounding and finite-stock exclusions. No additional price or
material database is introduced. This is the software/source slice `ftx-pq1.1`; it does not close
supplier fulfillment, complete physical characterization or garment acceptance.

## Observed source and the unit mismatch

The [primary supplier page](https://fabricmartfabrics.com/products/bravo-brown-nylon-lycra-activewear-knit-58w)
identifies Bravo Brown Nylon/Lycra Activewear Knit, SKU **DCC 3844**, product `10487001547044`,
variant `54020910219556`. The ordinary material ID is `fabricmart/DCC-3844`.
The final live three-response observation completed at **2026-10-27T20:33:12.801363Z**:

| Source observation | Meaning retained by the adapter |
|---|---|
| HTML price USD 25.00 per yard | Displayed yard-denominated price |
| Embedded variant price 625 hundredths; public JSON USD 6.25 | Price for one quarter-yard billing unit |
| UI minimum 1 yard, increment 0.25 yard | Actual published fabric ordering basis |
| Embedded tracked inventory 90 units with `deny` policy | 22.5 yards at the reviewed quarter-yard denomination |
| Displayed stock 22 yards | Explicitly selected conservative capacity under `inventory_basis: displayed_yards` |
| Nominal width 58 inches | Catalog width observation; usable selvage allowance remains unknown |

The generic Shopify unit minimum is one billing unit; it does not establish a quarter-yard fabric
minimum. Treating its USD 6.25 price as a full-yard price would understate the published price by a
factor of four. The adapter cross-checks four billing units against the displayed yard price, as
well as the explicit decimal-step and UI-minimum fields.

The **22 versus 22.5 yard difference is preserved** in the receipt. Selecting the displayed-yard
policy intentionally uses 22 yards for procurement; it does not claim the supplier refuses to sell
the remaining half yard. The receipt reports both observations and
`inventory_display_difference_yards: 0.5`. Arbitrary disagreement refuses: the current format must
match whole displayed yards to the tracked quarter-yard inventory's whole-yard part. No inventory
quantity establishes uninterrupted roll length, defect-free area or a checkout reservation.

The built-in record retains the supplier's nylon/Lycra description without inventing blend
fractions. The reported stretch has no load, gauge, recovery or conditioning protocol and remains
descriptive metadata. The record supplies no mass, mechanical, thermal, fire-performance or
machine-process parameters.

## Refresh and replay

Run from the repository root, selecting output paths which do not already exist:

```sh
python scripts/refresh_textile_prices.py docs/examples/fabricmart-stock-refresh.json \
  --intraday --out fabricmart-quotes.json --evidence fabricmart-evidence \
  --specifications-out fabricmart-specifications.json
```

The adapter GETs product HTML, its public `.json` currency/price representation, then HTML again.
It reads no cart, creates no session requiring credentials, submits no form and places no order.
The existing HTTP client checks robots rules and uses the repository's required user agent.
All raw responses and the receipt are retained by content hash. The source observations, rather
than volatile HTML bytes, must agree across the bracket.

Only one exact default variant with the reviewed product ID, SKU, quarter-yard denomination,
minimum/increment, currency, specification rows and tracked-deny inventory qualifies. A missing
stock field, extra variant, changed composition/width, price-denomination conflict, conditional
quantity pricing or changed observation refuses the entire new snapshot. Explicit zero inventory
with an out-of-stock declaration publishes zero capacity. A disabled variant that retains positive
inventory is outside this bounded source contract and refuses; it is not silently treated as zero.
Existing snapshots remain intact on every refusal. As with the other adapters, the reviewed selling
basis expires after 30 days and needs a deliberate fresh review.

`--compare-specifications`, `--previous-evidence` and `--review-out` use the existing supplier-revision
workflow. Replay verifies the original HTML bracket and public JSON before recomputing facts.
Price or capacity changes alone do not imply a textile revision; a changed source specification or
material lock requires review. The full product specification is retained, including qualitative
fields, so a silent finish or product revision cannot pass solely because the SKU still matches.

The checked-in [dated quote snapshot](examples/fabricmart-stock-quotes-2026-10-27.json) is a past
source observation, not an assurance of present price or inventory. Its material lock is
`e10dd31351adc464af6421ad6cd18e4ac615e55fcbc1831bebcd80352f20cffa`.

| Final live evidence | SHA-256 |
|---|---|
| HTML before | `3a3a7c2e9a02f23c4e08413842ce66a7fb17364d5f57c8a64938fe9878165c9a` |
| Public product JSON | `fab619f7400421388fbf5179fa94600e72398ab80c0eab818961f6c3f24db66e` |
| HTML after / quote source | `27679bfcf2b93952f3b3e3d71a2c15d5d73d70e0ff495067fb70d3eba0bf5a82` |

The raw pages and images are not redistributed in the repository; the snapshot and material record
retain selected factual observations, URLs and evidence hashes. Source rights remain with the supplier.

## Consumer verification

The actual native `ftx-materials bind` command freezes the new record into an ordinary synthetic panel
without changing its geometry or claiming simulation readiness. Feeding the live quote to
`ftx-materials price` produces a one-yard minimum order costing **USD 25.00**, excludes a **22.25-yard**
order as `insufficient_quantity`, and excludes a request for a source-qualified continuous segment
as `unknown_continuous_length`. These outcomes use the existing native quote evaluator.

The Python textile suites include synthetic template tests for denomination, stock discrepancy,
zero stock, contradictory/missing/duplicate fields, changing bracket responses, immutable evidence,
exact material binding and supplier-revision replay. The final live refresh and evidence replay also
passed. Native catalog tests check the exact material hash, nominal-width normalization, empty-catalog
frozen replay and honest missing physical capabilities. Synthetic tests do not substitute for live
source qualification; live source declarations do not establish fulfillment or physical textile laws.
