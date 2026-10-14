# Source refresh into the existing material-price workbench

`scripts/refresh_textile_prices.py` reads public Shopify Ajax, reviewed CSV and schema.org Product/Offer data and emits the existing
`ftx-textile-quotes/1` format consumed by `ftx-materials price`. This is an external data-ingestion
adapter, not a second catalog, pricing model, physics module, scheduler or checkout client.
It requires only Python 3's standard library; the solver remains offline and deterministic.

## Use

Prepare a reviewed manifest containing exact supplier origins, variants and catalog IDs. The following
is a **synthetic schema example**, not a real product, price or preconfigured supplier integration:

```json
{
  "schema": "ftx-shopify-refresh/1",
  "offers": [{
    "offer_id": "supplier/exact-variant/retail",
    "material_id": "YOUR-EXACT-CATALOG-ID",
    "supplier": "supplier",
    "sku": "EXACT-SUPPLIER-SKU",
    "quantity": 0.5,
    "unit": "linear_yard",
    "minimum_quantity": 1.0,
    "order_increment": 0.5,
    "currency": "USD",
    "region": "US",
    "quality": "first-quality",
    "terms": "merchandise-ex-tax-shipping",
    "product_url": "https://example.invalid/products/exact-handle.js",
    "cart_url": "https://example.invalid/cart.js",
    "variant_id": 42,
    "basis_source": "https://example.invalid/products/exact-handle",
    "basis_note": "Replace with the reviewed sale-unit, minimum/increment and market evidence.",
    "basis_reviewed_on": "2026-10-13",
    "required_description_text": ["Sold per half yard"]
  }]
}
```

Use the supplier's actual data; never infer a raw-textile quote from a finished garment's price.
`basis_reviewed_on` must be a real review within 30 days, not an automatically renewed timestamp.
Description fragments are exact substrings of the product's HTML description, selected to detect
selling-basis changes. They supplement a human-reviewed basis; they do not validate arbitrary prose,
shipping terms, geographic eligibility, hidden order limits or a supplier's truthfulness. Use trusted
primary supplier domains, not similarly named reseller/look-alike sites. No external links in fetched
content are followed. Product, cart and basis origins and locale prefixes must agree.

```sh
python3 scripts/refresh_textile_prices.py manifest.json \
  --out quotes-2026-10-13T180000.json --evidence ./supplier-evidence
cargo run -p ftx-cli --bin ftx-materials -- \
  price pattern.json quotes-2026-10-13T180000.json scope.json
python3 -m unittest discover -s scripts -p 'test*.py' -v
```

The output path **must be new**. A successful run publishes one complete snapshot atomically. Any
fetch/validation failure prevents quote publication; old snapshots are never erased or made fresh by
changing their timestamps. All offers in a manifest must succeed. Product bytes are retained under
their SHA-256, alongside receipts containing the reviewed manifest, actual UTC retrieval time and
currency before/after the product request. Cookies and anonymous cart tokens are not persisted.

## Contract and limits

Only the exact variant's price and availability are accepted; `price_min`, `compare_at_price`, missing
variants, SKU changes, subscription-only offers and absent stock status cannot become ordinary quotes.
A fresh anonymous session performs GET cart → GET product → GET cart and verifies explicit presentment
currency against the manifest. Reading an empty cart does not add items or place an order. A currency
mismatch refuses; no default currency or exchange rate is invented. Robots exclusions are honored,
requests are serial and paced, responses are bounded to 4 MiB, timeouts are bounded, and redirects to
other origins refuse. Authentication gates, rate limits and anti-bot responses are not bypassed.

The manifest owns the reviewed quantity basis. The resolved material owns sourced width, so the existing
Rust price consumer performs yard-to-area conversion and order-quantity rounding. Neither adapter nor
consumer infers hide area, fabric density, thermal resistance, elasticity or safe machine settings from
price data. Quote currency does not prove delivery-region eligibility. Market/quality/tax/shipping labels
are reviewed declarations, not properties established by the Ajax response.

The current Rust quote schema records calendar dates. Keep intraday refreshes as separate snapshots:
merging conflicting same-day rows into a single historical book is deliberately rejected by that schema.
A successful fetch is not a purchase guarantee or a statistically representative world-market survey.
No automatic recurring refresh is configured. Reviewed live supplier manifests and policy-aware scheduling
remain follow-up work. Live supplier endpoints were inaccessible from the implementation environment;
the adapter was verified with offline response fixtures, not presented as a successful live scrape.

Primary interface references (checked 2026-10-13):
- Shopify Ajax Product API: https://shopify.dev/docs/api/ajax/reference/product
- Shopify Ajax Cart API: https://shopify.dev/docs/api/ajax/reference/cart
- Shopify variant monetary representation: https://shopify.dev/docs/api/liquid/objects/variant

The source tests cover exact variant selection, price-field distinction, currency drift, the Shopify
hundredths representation, unavailable offers, changed/old quantity evidence, malformed JSON and
non-finite numbers, locale/origin conflicts, anonymous-only reads, batch failure and immutable publication.

## Bulk supplier CSV feeds

`ftx-textile-refresh/1` accepts reviewed CSV offers alongside the unchanged Shopify entries above.
Use `adapter: "csv_v1"` for an anonymous public UTF-8 supplier feed. This adapter emits exactly the
same Rust quote schema; it is not a separate price database. The source must explicitly carry SKU,
price, currency, stock, priced quantity, unit, minimum and increment. Only one exact row may match.

Start with the common fields (`offer_id` through `terms`, and `basis_source`, `basis_note`,
`basis_reviewed_on`) from the example above, remove the Shopify endpoint/variant/description fields,
and add the following **synthetic** source description:

```json
{
  "adapter": "csv_v1",
  "source_url": "https://example.invalid/prices.csv",
  "delimiter": ",",
  "columns": {
    "sku": "SKU", "price": "PRICE", "currency": "CURRENCY", "available": "STOCK",
    "quantity": "QTY", "unit": "UNIT", "minimum_quantity": "MIN", "order_increment": "STEP"
  },
  "match": {"SKU": "EXACT-SUPPLIER-SKU", "MARKET": "US", "SIZE": "yardage"},
  "availability": {"available": ["in stock"], "unavailable": ["out of stock"]}
}
```

The manifest's `match` must include the exact SKU and every column needed to disambiguate color,
variant, sample-versus-yardage or customer market. An ambiguous match refuses rather than
selecting the cheapest or first row. The source's quantity, canonical sale unit, minimum and increment
must equal the reviewed manifest; changing from half-yards to rolls cannot silently multiply cost.
Prices are plain decimal numbers in major currency units, not Shopify integer hundredths. Commas,
currency symbols, exponents, blank/unknown stock and non-finite or unrepresentable values refuse.
Comma, semicolon and tab delimiters are explicit, not guessed; UTF-8 BOMs and properly quoted multiline
cells are supported. Missing/duplicate headers and malformed rows refuse the entire snapshot.

One supplier-feed URL is fetched once per batch and all selected rows share its bytes and timestamp.
Original bytes are retained as `<sha256>.csv`; the receipt records the selected logical record number
(header is record 1), source hash and mapping. Bounded quantity tiers cannot be represented by the
current quote schema and must not be configured as unconditional offers. Authenticated feeds, conditional/negotiated prices and
unknown quantity conventions require separate qualification. CSV tests use synthetic observations,
not live supplier prices. The existing robots, pacing and immutable-publication policy is unchanged.

## Structured Product/Offer pages (JSON-LD)

The same `ftx-textile-refresh/1` manifest accepts `adapter: "jsonld_v1"`. Retain the common selling-basis
fields and `required_description_text` from the Shopify example, remove its endpoint/variant fields,
and add this **synthetic** source description:

```json
{
  "adapter": "jsonld_v1",
  "source_url": "https://example.invalid/products/exact-sheet",
  "source_format": "html",
  "offer_ref": null,
  "tax_included": null
}
```

`source_format` is `html` for inline `application/ld+json` scripts or `json` for a public JSON-LD feed.
The reader supports object/array documents, `@graph`, nested ProductGroup variants, and local `@id`
references. It requires exactly one Product with the manifest's exact string SKU. Its offers must
resolve to one ordinary Offer; multiple offers require an exact `offer_ref` matching an Offer `@id`.
The selected Offer cannot rebind the Product's SKU. Product-level minimums, AggregateOffer, preorder,
backorder, membership, quantity-conditioned, rental and bundled prices are not converted to ordinary
quotes. Empty, ambiguous, malformed or unsupported evidence refuses the whole snapshot.

Only standard schema.org contexts (or a sole schema.org `@vocab`) are supported, without aliases or
term remapping. JavaScript is never executed and referenced scripts, contexts and external nodes are
never fetched. There is no browser fallback or authentication bypass. HTML must be UTF-8, with at most
64 data scripts; graph traversal is bounded. Original HTML/JSON and source-selection receipts are
retained by hash. All SKUs drawn from one URL use the same fetched bytes and observation timestamp.

A direct Offer price or one plain PriceSpecification can supply the price, with explicit currency.
When both exist, their amounts/currencies must agree. UnitPriceSpecification, list/sale-price arrays,
conditional specifications and hidden conversion assumptions refuse. `tax_included` is a reviewed
boolean when the source explicitly publishes `valueAddedTaxIncluded`, otherwise null. A conflicting
or disappearing tax observation refuses. This checks published metadata, not checkout charges.
Exact Product-description fragments still guard the reviewed quantity basis; JSON-LD prices do not
establish roll length, yardage, dye lot, delivery eligibility or calibrated textile physics.

### Source expiry is a purchasing constraint, not just a receipt note

The quote model accepts optional `valid_through: "YYYY-MM-DD"` (last eligible UTC date, inclusive).
Existing quotes omit it and round-trip unchanged; absence means no known expiry, not guaranteed
future validity. JSON-LD retains the earliest dated price/offer/availability end boundary in this
field. It rejects already-expired or not-yet-valid observations. Intraday end timestamps refuse because
the current purchasing scope evaluates calendar dates, not instants; they are not rounded into a
longer-lived offer. A new build is required to consume this optional field with expiry semantics.

The Rust evaluator first chooses the latest observation per exact offer/scope and **then** filters
expiry and staleness. Consequently an expired or unavailable latest offer cannot resurrect an older,
cheaper/unbounded quote. Empty samples retain unknown cost rather than zero. This is shared by
material-price and cutting procurement consumers; no second pricing formula was added.

Primary structured-data interface reference, checked 2026-10-14: https://schema.org/Offer .
Offline regressions cover graph/variant selection, conflicting prices, explicit stock and tax,
conditional offers, source/quote expiry, broken JSON-LD, bounded traversal, and complete CLI evidence
publication. They do not establish live supplier qualification or worldwide price coverage.
