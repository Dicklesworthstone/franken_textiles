# Purchasable textile variants and reviewed live sources

The built-in catalog adds twelve primary supplier product records in
`crates/ftx-pattern/src/material_catalog/supplier-apparel.json`. These use the existing
`Material`, normalization, frozen selection and capability gates. They immediately support
material discovery, source inspection, mass/width screening and the applicable purchasing
calculations. They are partial product characterizations; none contains a complete measured
membrane, bending, friction or thermal law.

All rows were reviewed from public supplier HTML on **2026-10-18** with the repository's
required user agent. Each source note retains the exact page hash. Only factual observations
and source identifiers are committed; copyrighted page bodies, images and customer reviews
are not redistributed. Published values are not independent laboratory measurements.

## Added material records

| Catalog ID | Construction and source facts | Unresolved data kept explicit |
| --- | --- | --- |
| `blackbird/FL9910` | Black linen, 100% flax; 210 g/m²; 130 cm nominal width | Yarn count, usable selvage, thickness, mechanics and thermal response |
| `blackbird/FNOVAWHT` | White washed cotton poplin; 110 g/m²; 145 cm nominal width | Exact weave counts, shrinkage law, thickness and physical response |
| `blackbird/FT4148999` | Black jersey: 66% bamboo-derived rayon, 28% cotton, 6% elastane; 200 g/m²; 152 cm nominal width | Supplier stretch percentages have no load or recovery protocol; exact variant was out of stock |
| `nydesigner/CM-065` | Black 100% silk charmeuse; 44 in nominal width; supplier silk-weight label 19 MM | No thickness inferred from MM; areal-weight conversion remains unqualified |
| `greenpepper/merino-46280585609436` | Moroccan Teal 100% merino knit; 200 g/m²; 59 in nominal width | Missing supplier SKU and detailed stitch/loop characterization; 50% stretch is not stiffness |
| `organiccottonplus/INT-SOLIDS-BLACK` | Black 100% cotton interlock; 260 g/m²; nominal 60 in, described as at least 60 in | Actual roll width, usable selvage, thickness and mechanical laws |
| `organiccottonplus/FELT400` | Black 100% merino felt; 1 mm nominal thickness; 72 in nominal width | Thickness pressure/conditioning absent; 13–14 oz/yard is not assigned an area basis or midpoint |
| `organiccottonplus/THR4800-5000WHT` | White two-ply cotton sewing thread; 35 tex; 5000 m cone; supplier soy-based coating | Coating mass, strength and process qualification; thread does not become a shell |
| `organiccottonplus/ORBY-96` | Natural needle-punched cotton batting without added scrim/binders; 3.7 oz/yd²; 96 in nominal width | Thickness range 1/8–1/4 in has no nominal; 3–5% shrinkage lacks test protocol |
| `discovery/power-stretch-94010-olive-mesh` | Polartec 94010 Olive Mesh: 92% nylon, 8% elastane; 203 g/m²; 66 in nominal width | No measured load-extension, recovery, friction or thermal response |
| `discovery/power-stretch-94065-black-mesh` | Polartec 94065 Black Mesh: 92% polyester, 8% elastane; 203 g/m²; 60 in nominal width | Kept distinct from nylon 94010; 55% stretch lacks force/gauge/conditioning |
| `rbtr/hyperdry-850-goose-down` | Allied HyperDRY treated goose down; supplier 850 FP label; three-ounce packs | FP test protocol/units absent; no inferred clo, sheet geometry, density or warmth rating |

Original quantities retain their source units. Metric and imperial supplier labels may be
rounded independently: the catalog selects an explicitly named original column rather than
averaging them. Width is the supplier's nominal fabric width, not a measured defect-free usable
width. A real cutting layout still needs its own edge/selvage allowance and lot inspection.

### Primary source pages

- Blackbird: [linen](https://www.blackbirdfabrics.com/products/everyday-linen-black),
  [cotton poplin](https://www.blackbirdfabrics.com/products/washed-cotton-poplin-white),
  [bamboo/cotton jersey](https://www.blackbirdfabrics.com/products/bamboo-cotton-jersey-black).
- NY Designer Fabrics: [CM-065 silk charmeuse](https://nydesignerfabrics.com/products/black-charmeuse-silk-fabric).
- The Green Pepper: [merino wool knit](https://thegreenpepper.com/products/merino-wool).
- Organic Cotton Plus: [interlock](https://organiccottonplus.com/products/solid-interlock-60),
  [felt](https://organiccottonplus.com/products/felt-72),
  [sewing thread](https://organiccottonplus.com/products/thread-5000-meter-cone-1),
  [cotton batting](https://organiccottonplus.com/products/heirloom-cotton-batting-by-the-yard).
- Discovery Fabrics: [style-specific Polartec jersey specifications](https://discoveryfabrics.com/products/polartec-power-stretch-jersey).
- Ripstop by the Roll: [HyperDRY goose down](https://ripstopbytheroll.com/products/hyperdry-850-fp-dwr-goose-down).

## Refresh actual offers

`docs/examples/textile-supplier-refresh.json` is a reviewed real-source manifest for the
existing `jsonld_v1` adapter. It selects three exact Blackbird SKUs in the Canadian CAD
storefront and CM-065 in the NY Designer Fabrics US/USD storefront. It contains no hard-coded
prices. The corresponding material IDs bind sourced widths into the existing price consumer.

```sh
python3 scripts/refresh_textile_prices.py docs/examples/textile-supplier-refresh.json \
  --out /tmp/textile-quotes-NEW.json --evidence /tmp/textile-source-evidence-NEW
```

Choose new output paths for every refresh. Inspect the current source and renew the reviewed
basis deliberately after the 30-day review window. A refresh is an anonymous public product-page
GET; it does not request a cart endpoint, add a product to a cart or execute checkout. Blackbird's
half-metre selling rule appears outside its JSON-LD Product description, so `required_page_text`
guards that exact FAQ sentence as well as the existing exact-SKU/description checks.

The complete live refresh succeeded on **2026-10-18**. The resulting factual snapshot is retained in
[`textile-quotes-2026-10-18.json`](examples/textile-quotes-2026-10-18.json), including each source hash,
observation date and expiry. These values were fetched from actual product HTML, not seeded fixtures:

| Exact SKU | Listed price and sale unit | Stock at observation | Source price-valid-until |
| --- | --- | --- | --- |
| Blackbird FL9910 | CAD 15.50 per 0.5 m | In stock | 2026-10-28 |
| Blackbird FNOVAWHT | CAD 9.25 per 0.5 m | In stock | 2026-10-28 |
| Blackbird FT4148999 | CAD 11.00 per 0.5 m | Out of stock | 2026-10-28 |
| NY Designer Fabrics CM-065 | USD 32.95 per linear yard | In stock | 2026-10-28 |

These are listed merchandise amounts, with tax/shipping and supplier quality grade explicitly
unqualified. The scope labels are `supplier-retail-grade-unspecified` and
`listed-merchandise-tax-shipping-unqualified`. They are not landed costs. CAD and USD never
share a price sample, and different material SKUs are not pooled into one supposed market
distribution. With one eligible quote per material, equal p10/median/p90 describe one observed
offer. An unavailable variant produces no eligible offer rather than a zero-price substitute.

The real `ftx-materials price` consumer was also exercised with this snapshot, the built-in selections
and a declared requirement of **2 m²** per material. This is an area-purchasing example, not a garment
marker or measured usable-yield claim:

| Material | Rate per nominal m² | Rounded merchandise order | Order amount |
| --- | --- | --- | --- |
| Black linen FL9910 | CAD 23.8461538462 | 2 linear metres | CAD 62.00 |
| White poplin FNOVAWHT | CAD 12.7586206897 | 1.5 linear metres | CAD 27.75 |
| Black bamboo jersey FT4148999 | No eligible rate | Out of stock | Unknown |
| Black silk CM-065 | USD 32.2428043846 | 2 linear yards | USD 65.90 |

The existing engine derives each area rate from the selected material's source-qualified nominal
width. It uses the quoted minimum and increment for the actual order, and keeps the two currencies
separate. Equal p10/median/p90 for each available row correctly reflect its single observed offer.

Other reviewed pages intentionally have no price manifest: Organic Cotton Plus publishes
AggregateOffer/quantity tiers; Discovery's multi-variant offers lack the exact `@id` selectors
supported by the current importer; The Green Pepper omits Product SKU; Ripstop by the Roll
does not publish the required Product/Offer JSON-LD and excludes `cart.js` in its robots policy.
The source properties remain useful without relaxing these quote-admission requirements.
Full raw evidence remains in the caller's local evidence directory; the repository should retain
only normalized factual snapshots and source hashes, not supplier page copies.

`supplier_apparel_catalog` regressions check authored-versus-normalized units, frozen selections,
exact performance-mesh style identity, original yarn/fill levels, missing-range midpoints and
manifest-to-material identity. They verify the software/data boundary; they do not establish
garment fit, warmth, measured cloth behavior or machine safety.

## Coats Epic sewing-thread observations (August 2026)

Four additional `Form::Yarn` records in
`crates/ftx-pattern/src/material_catalog/catalog.json` preserve exact articles from the
manufacturer's [Coats Epic Product Information Sheet](https://www.coats.com/wp-content/uploads/2025/04/Coats-Epic-Product-Information-Sheet-2026.pdf).
The Product Range table is on page 2; the August 2026 edition date is on page 3.
The PDF was retrieved on **2026-10-26** and its table checked visually.

| Catalog ID | Ticket | Tex | Put-up length, m | Supplier average strength, cN |
| --- | ---: | ---: | ---: | ---: |
| `coats/epic-2994120-2026` | 120 | 24 | 5000 | 1190 |
| `coats/epic-2994100-2026` | 100 | 30 | 5000 | 1490 |
| `coats/epic-2994080-2026` | 80 | 40 | 5000 | 1960 |
| `coats/epic-2994050-2026` | 50 | 60 | 5000 | 3040 |

Source PDF SHA-256:
`b08de6e730a7f3993f21219ce189b2da0038091b9976f53fe0a154687bed0ca6`.

All numeric observations carry `published` evidence, original units, source table and
edition, and exact article identity. The publisher describes indicative current averages,
without lot bounds or independent calibration. The source names polyester corespun
construction with polyester wrap and a lubricant finish, but no constituent mass fractions;
composition remains incomplete. Reported elongation and recommended needle-size ranges
remain descriptive observations. No midpoint, constitutive law or machine qualification
is inferred.

The existing resolver normalizes `yarn_linear_density` from tex to kg/m while preserving
authored observations and frozen selection identity. These yarn selections can be embedded
in qualified sewing `thread_consumption` channels. With separately sourced metres-per-traversal
coefficients and start/end allowances, the sewing consumables consumer computes operation,
per-copy and production-batch length requirements. Its purchasing consumer can evaluate
separately observed length offers for those exact yarn snapshots. The Coats sheet supplies
neither consumption coefficients nor price/stock evidence.

`put_up_length` remains a package-length observation: it does not automatically establish
order increments, uninterrupted available stock or cone rounding. Current pricing does not
convert yarn length requirements into mass. `supplier_average_thread_strength` remains an
unknown extension in cN, with no automatic unit conversion or interpretation as seam strength,
a shell force limit or an allowable load. Mechanical and thermal capability gates continue
to refuse missing physical properties.
