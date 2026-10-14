# Source refresh into the existing material-price workbench

`scripts/refresh_textile_prices.py` reads public Shopify Ajax product data and emits the existing
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
python3 -m unittest discover -s scripts -p 'test_refresh_textile_prices.py' -v
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
No automatic recurring refresh is configured. Broader JSON-LD/CSV suppliers and policy-aware scheduling
remain follow-up work. Live supplier endpoints were inaccessible from the implementation environment;
the adapter was verified with offline response fixtures, not presented as a successful live scrape.

Primary interface references (checked 2026-10-13):
- Shopify Ajax Product API: https://shopify.dev/docs/api/ajax/reference/product
- Shopify Ajax Cart API: https://shopify.dev/docs/api/ajax/reference/cart
- Shopify variant monetary representation: https://shopify.dev/docs/api/liquid/objects/variant

The source tests cover exact variant selection, price-field distinction, currency drift, the Shopify
hundredths representation, unavailable offers, changed/old quantity evidence, malformed JSON and
non-finite numbers, locale/origin conflicts, anonymous-only reads, batch failure and immutable publication.
