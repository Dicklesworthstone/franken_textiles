# Source refresh into the existing material-price workbench

`scripts/refresh_textile_prices.py` reads public Shopify Ajax, reviewed CSV and schema.org Product/Offer data and emits the existing
`ftx-textile-quotes/1` format, or explicit intraday `/2` with `--intraday`, consumed by `ftx-materials price`. This is an external data-ingestion
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

The request's Accept header follows the reviewed source format: HTML pages request `text/html`, JSON-LD
feeds request JSON-LD/JSON, CSV feeds request CSV, and Shopify Ajax requests JSON. This prevents an HTML
product URL from negotiating a different JSON product representation that lacks the reviewed page data.

Requests identify as `OpenAI File Downloader, XaiImageApiFetch/1.0`, with REP product token
`XaiImageApiFetch`. The collector combines equally specific matching agent groups, uses the most
specific matching path rule, and prefers Allow on equal-length ties. It handles `*`, terminal `$`,
UTF-8 and percent-encoded paths; a leading `Allow: /` cannot override a longer cart exclusion.
Same-origin source redirects are checked before the destination request. Declared Crawl-delay and
Request-rate intervals apply from the first source request and between redirect hops. Only a robots
404 is treated as an absent policy; other retrieval failures stop collection. Policy-file redirects
remain restricted to the supplier origin and the resulting rules govern that origin.
Protocol reference: [RFC 9309, section 2.2](https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2).

The manifest owns the reviewed quantity basis. The resolved material owns sourced width, so the existing
Rust price consumer performs yard-to-area conversion and order-quantity rounding. Neither adapter nor
consumer infers hide area, fabric density, thermal resistance, elasticity or safe machine settings from
price data. Quote currency does not prove delivery-region eligibility. Market/quality/tax/shipping labels
are reviewed declarations, not properties established by the Ajax response.

The version-1 Rust quote schema records calendar dates and rejects conflicting same-day rows.
Use the opt-in version-2 path below when a historical book needs multiple observations on one date;
both versions retain immutable source snapshots.
A successful fetch is not a purchase guarantee or a statistically representative world-market survey.
No automatic recurring refresh is configured. A reviewed four-offer live manifest is now available in
`docs/examples/textile-supplier-refresh.json`; its full public-source refresh succeeded on 2026-10-18.
The dated factual snapshot is `docs/examples/textile-quotes-2026-10-18.json`. The existing Rust price
consumer resolved the corresponding built-in material widths, converted metre/yard prices to area
rates, rounded actual sale increments and excluded the out-of-stock variant. See
`docs/supplier-apparel-catalog.md` for exact source scope and remaining supplier coverage. Policy-aware
scheduling and broader supplier qualification remain follow-up work.

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
retained by hash. All SKUs drawn from one URL and representation use the same fetched bytes and
observation timestamp; distinct negotiated representations are cached separately.

A direct Offer price or one plain PriceSpecification can supply the price, with explicit currency.
When both exist, their amounts/currencies must agree. UnitPriceSpecification, list/sale-price arrays,
conditional specifications and hidden conversion assumptions refuse. `tax_included` is a reviewed
boolean when the source explicitly publishes `valueAddedTaxIncluded`, otherwise null. A conflicting
or disappearing tax observation refuses. This checks published metadata, not checkout charges.
Exact Product-description fragments still guard the reviewed quantity basis; JSON-LD prices do not
establish roll length, yardage, dye lot, delivery eligibility or calibrated textile physics.

When a reviewed selling-basis statement appears elsewhere on the product page, HTML entries may also
declare `"required_page_text": ["All fabric is sold by the half meter."]`. Every fragment must be a
nonempty exact substring of the fetched UTF-8 HTML. Changed or missing fragments refuse the offer;
whitespace, markup and entities are not normalized. This optional check supplements the mandatory
`required_description_text` checks and is retained in the existing source receipt. It is unavailable
for `source_format: "json"`. Matching raw page text does not establish visibility or interpret the
statement: the reviewed manifest still declares the sale unit and quantity, and no scripts run.

### Source expiry is a purchasing constraint, not just a receipt note

The quote model accepts optional `valid_through: "YYYY-MM-DD"` (last eligible UTC date, inclusive).
Existing quotes omit it and round-trip unchanged; absence means no known expiry, not guaranteed
future validity. JSON-LD retains the earliest dated price/offer/availability end boundary in this
field. It rejects already-expired or not-yet-valid observations. Intraday end timestamps still refuse
in version 1; version 2 retains them as precise instants rather than rounding them into longer-lived
offers. A compatible build is required to consume the declared schema with its expiry semantics.

The Rust evaluator first chooses the latest observation per exact offer/scope and **then** filters
expiry and staleness. Consequently an expired or unavailable latest offer cannot resurrect an older,
cheaper/unbounded quote. Empty samples retain unknown cost rather than zero. This is shared by
material-price and cutting procurement consumers; no second pricing formula was added.

Primary structured-data interface reference, checked 2026-10-14: https://schema.org/Offer .
Offline regressions cover graph/variant selection, conflicting prices, explicit stock and tax,
conditional offers, source/quote expiry, broken JSON-LD, bounded traversal, and complete CLI evidence
publication. They do not establish live supplier qualification or worldwide price coverage.

### Intraday histories through the existing procurement and study consumers

Use `--intraday` to publish `ftx-textile-quotes/2`. The reviewed manifest and source adapters remain
the same; each quote adds its actual source-observation `observed_at`, taken from the same clock as
its retained receipt, and keeps `observed_on` as the matching UTC calendar date. CSV responses shared
by several selected rows retain one fetch time. Shopify, CSV and JSON-LD all use this path:

```sh
python3 scripts/refresh_textile_prices.py docs/examples/textile-supplier-refresh.json \
  --intraday --out quotes-next.json --evidence supplier-evidence
```

Each version-2 quote requires `observed_at` in UTC
`YYYY-MM-DDTHH:MM:SSZ` or `YYYY-MM-DDTHH:MM:SS.fractionZ` form. Rust accepts one to nine fractional
digits and compares integer seconds/nanoseconds without floating-point rounding. Offsets, local
times, named zones, leap-second spellings, `24:00` and more than nine fractional digits refuse at
this boundary. The Python adapter normalizes its timezone-aware retrieval clock to UTC and preserves
its six-digit microsecond resolution. No time is invented for an older calendar-only observation.

Keep full source observations when combining version-2 snapshots into a history. The same offer ID
must continue to identify the same material/product/SKU and comparable market. Distinct instants may
contain changed price, availability or quantified capacity. Exact duplicate records are harmless;
different records at the same parsed instant refuse, including different fractional spellings of
that instant. This conservatively leaves conflicting source evidence for review rather than choosing
one by input order. Version 1 keeps its original date-only conflict and serialization behavior;
timestamp fields cannot silently change a version-1 book's semantics.

For pricing, supply `as_of_at` as well as the existing matching `as_of` date in `Scope` or the cutting
procurement `Market`. For example, these are explicit illustrative evaluation choices:

```json
{
  "as_of": "2026-10-20",
  "as_of_at": "2026-10-20T14:30:00.123456Z"
}
```

Include the ordinary currency, region, quality, terms, age and requirement/market fields as before.
The evaluator selects the newest observation **at or before that instant**, then applies expiry,
availability, material hash, age and whole-order stock checks. A latest expired, unavailable or
insufficient-stock observation excludes the offer; it never promotes an earlier cheaper or available
one. Future observations cannot leak into historical evaluations. `max_age_days` continues to count
UTC calendar-day boundaries, not elapsed 24-hour intervals. Missing or inconsistent evaluation time
refuses a version-2 request; version-1 requests omit `as_of_at`.

JSON-LD can retain the earliest timed source end boundary as inclusive `valid_through_at`, alongside
the earliest calendar-date `valid_through` when both kinds are present. Both limits apply, and expiry
before observation refuses. A source DateTime must include seconds, an explicit `Z` or numeric
`+/-HH:MM` offset, and no more than six fractional digits, so Python does not silently truncate a
more precise source. Unknown `-00:00` offsets refuse. Original timezone spelling remains in the
retained source bytes; emitted quotes use UTC. A time-only offer change remains a purchasing change,
not a material specification change, when using `--compare-specifications`.

The native price/cut-price commands, shared WASM procurement entry point and manufacturing/scenario
studies use the same `QuoteBook` evaluator. Marker-derived whole-order quantities and market comparisons
therefore use the requested instant without another pricing formula. Reports retain the exact chosen
or excluded observation and complete evaluation scope. These are source observations, not reserved
inventory, checkout guarantees, intraday market population statistics or automatic refresh scheduling.

## Review supplier specification revisions

The refresh command can retain and compare the selected supplier's declarations before a new
quote is published for an existing material revision. Add `--specifications-out` to a successful
refresh to save a new `ftx-supplier-specifications/1` snapshot alongside its original source bytes
and receipts:

```sh
python3 scripts/refresh_textile_prices.py manifest.json \
  --out quotes-initial.json --evidence supplier-evidence \
  --specifications-out specifications-initial.json
```

For subsequent observations, replay that exact baseline and request a detailed comparison:

```sh
python3 scripts/refresh_textile_prices.py manifest.json \
  --out quotes-next.json --evidence supplier-evidence \
  --compare-specifications specifications-initial.json \
  --specifications-out specifications-next.json --review-out revision-review.json
```

Every output path must be new. `--previous-evidence DIRECTORY` can locate the baseline's evidence
when it differs from the new evidence directory. The replay verifies source and receipt SHA-256s,
reselects the exact product/variant, and reconstructs the retained declarations; editing a baseline's
facts or changing its material hash without matching original evidence refuses before retrieval.

The comparison preserves original text, numbers and units. It covers the selected Shopify product
and variant, selected schema.org Product/Offer and locally referenced property nodes, or the complete
matched CSV row. Composition, construction, finish, nominal dimensions, variant options and supplier
lot declarations are retained wherever that representation actually supplies them. Selected source
and selling-basis/material bindings are compared too. Added, removed and changed offers receive
field paths with old/new values, observation dates and source/receipt hashes. Missing and null facts
remain distinct. A source's product-level minimum price or another variant cannot replace the exact
selected offer. Media, external references and prose are not interpreted as measurements.

Ordinary price and availability changes remain price observations. An unchanged declaration can
publish a newly unavailable quote, preserving the existing latest-observation semantics. Publication
through comparison additionally requires the same explicit `material_sha256` in both reviewed
manifests. An absent hash is unresolved; an ID alone does not qualify the material revision.

When a specification, selection or material binding changes, the command saves the new source
evidence, specification snapshot and `ftx-supplier-revision-review/1` report, exits with code **2**,
and leaves the requested quote output absent. Previous quotes, material selections and completed
studies remain intact. Existing adapter refusals, such as a missing SKU, unsupported quantity basis
or invalid price, still stop collection before a new snapshot; the comparison does not guess a
replacement offer or relax those checks.

Review the actual changed declarations before creating a new canonical material revision and
freezing its selection. Record that new selection's hash in the reviewed manifest and start a new
baseline. The comparison never transfers an older lot's mechanical, thermal or process observations
to a replacement SKU. Reports explicitly leave measured-specimen binding unresolved: a supplier
declaration, even unchanged, cannot establish that two manufactured lots are physically identical.
Broader real-supplier qualification and source-to-measured-lot binding remain open under `ftx-sr2`.

## Exact material snapshots and finite source quantities

An offer can now include two optional fields in the existing quote format:

```json
{
  "material_sha256": "<64 hexadecimal digits from the reviewed resolved material>",
  "available_quantity": 1.25
}
```

These are additions to a complete quote, not a standalone observation. The material hash binds
the quote to the exact source-bearing material and evidence policy, including its usable width.
Changing a same-ID fabric's width, finish, source observations or estimate policy invalidates a
locked offer before unit conversion. An omitted lock preserves existing ID-based pricing and
does not establish snapshot qualification. Old quotes still serialize without either new field.

`available_quantity` is the source's observed maximum supply in the quote's **original sale unit**.
For a price per half linear yard (`quantity: 0.5`, `unit: "linear_yard"`), a capacity of `1.25`
means 1.25 yards, not 1.25 half-yard packages. Capacity must be finite and nonnegative. A known
zero cannot supply any positive order; an omitted capacity remains unknown. It is not a price-tier
ceiling, a continuous-cut-length guarantee or a reason to guess usable hide area.

The order calculation includes both the minimum quantity and the selling increment. For example,
a 2.1 m² marker with 0.5 m² order increments needs 2.5 m² of purchasable stock; a source reporting
2.25 m² is excluded even though it exceeds the net requirement. Endpoint comparison accounts for
one final binary multiplication rounding step (such as `3 × 0.05`); it does not add an inventory
allowance or change the reported order quantity.

All three adapters accept the optional reviewed `material_sha256` in their manifest entry and
preserve it in the source receipt and quote. Only the CSV adapter currently imports quantified
stock: add the mapping below to its existing `columns` object, and document the sale-unit meaning
in the reviewed `basis_note`:

```json
"available_quantity": "AVAILABLE_YARDS"
```

The exact selected row must then contain a plain nonnegative decimal in that column. Missing,
ambiguous, nonfinite or unrepresentable values refuse the refresh; they never become zero or
unlimited supply. CSV variant, region, SKU, unit and selling-basis checks remain in force, and the
complete source response is retained. Without this reviewed mapping, numeric stock columns are
ignored. Shopify/JSON-LD availability labels still provide only a boolean, so their capacity
remains unknown. No static inventory number is copied from the reviewed manifest into live quotes.

Set `"require_observed_quantity": true` in a `price` scope or `cut-price` market to require source
quantity sufficient for every complete rounded order. Omission retains observational price
comparison with unknown capacity. Every accepted offer reports `quantity_sufficiency_observed`.
The marker report's `complete_observed_quantity_coverage` describes the chosen least-cost orders;
it can be false even when a legacy observational cost is available. If any material has no eligible
offer, the total remains unknown. `excluded_offers` retain the latest source row and a reason such
as `insufficient_quantity`, `unknown_quantity` or `material_snapshot_mismatch`; insufficient-stock
rows also report `required_order_quantity` in their source sale unit.

The latest observation is selected before these checks, so a later exhausted or rebound offer
cannot resurrect an older unlimited or cheaper observation. Independent material orders are not
pooled. None of these results reserves inventory, establishes market population percentiles,
confirms a live supplier's current stock or authorizes a purchase. Tests use synthetic data.
