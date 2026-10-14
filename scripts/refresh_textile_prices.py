#!/usr/bin/env python3
"""Read reviewed public supplier offers into the ftx-textile-quotes/1 workbench format.

No shopping/cart mutation, credentials, browser automation, guessed units or material-law inference.
Each successful run creates a NEW immutable quote snapshot and source-byte evidence files.
"""
from __future__ import annotations

import argparse
import csv
import datetime as dt
from decimal import Decimal, InvalidOperation
import hashlib
import http.cookiejar
from html.parser import HTMLParser
import io
import json
import math
import os
from pathlib import Path
import re
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import urllib.robotparser

AGENT = "FrankenTextilesSourceRefresh/1.0"
LIMIT = 4 * 1024 * 1024
SCHEMA = "ftx-textile-quotes/1"
FIELDS = ("offer_id", "material_id", "supplier", "sku", "quantity", "unit",
          "minimum_quantity", "order_increment", "currency", "region", "quality", "terms")


def load(raw: bytes, *, object_only=True):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result
    def nonfinite(value):
        raise ValueError(f"non-finite JSON number: {value}")
    if len(raw) > LIMIT:
        raise ValueError("source exceeds 4 MiB")
    try:
        value = json.loads(raw, object_pairs_hook=pairs, parse_constant=nonfinite)
    except RecursionError as error:
        raise ValueError("JSON nesting exceeds parser bounds") from error
    if object_only and not isinstance(value, dict):
        raise ValueError("expected a JSON object")
    return value


def origin(url: str) -> str:
    p = urllib.parse.urlsplit(url)
    if (p.scheme != "https" or not p.hostname or p.username or p.password
            or p.port not in (None, 443) or p.query or p.fragment
            or any(c.isspace() for c in url)):
        raise ValueError("source must be an explicit credential-free HTTPS URL")
    return f"https://{p.hostname}"


class SameOrigin(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if origin(req.full_url) != origin(newurl):
            raise ValueError("cross-origin redirect refused; review the supplier URL")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


class PublicSession:
    """A fresh anonymous cookie jar, bounded responses, robots checks and serial request pacing."""
    def __init__(self):
        self.opener = urllib.request.build_opener(
            SameOrigin(), urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.policies = {}
        self.next_request = {}

    def _get(self, url):
        req = urllib.request.Request(url, headers={"User-Agent": AGENT, "Accept": "application/json,text/csv,text/html,text/plain"})
        with self.opener.open(req, timeout=20) as response:
            if origin(response.url) != origin(url):
                raise ValueError("source origin changed")
            raw = response.read(LIMIT + 1)
        if len(raw) > LIMIT:
            raise ValueError("source exceeds 4 MiB")
        return raw

    def get(self, url: str) -> bytes:
        host = origin(url)
        if host not in self.policies:
            parser = urllib.robotparser.RobotFileParser()
            try:
                robots = self._get(host + "/robots.txt").decode("utf-8")
            except urllib.error.HTTPError as error:
                if error.code != 404:
                    raise
                robots = "User-agent: *\nDisallow:"
            parser.parse(robots.splitlines())
            self.policies[host] = parser
            self.next_request[host] = time.monotonic() + 1.0
        policy = self.policies[host]
        if not policy.can_fetch(AGENT, url):
            raise ValueError(f"robots.txt disallows {url}; no bypass attempted")
        delay = max(1.0, policy.crawl_delay(AGENT) or policy.crawl_delay("*") or 0.0)
        rate = policy.request_rate(AGENT) or policy.request_rate("*")
        if rate and rate.requests:
            delay = max(delay, rate.seconds / rate.requests)
        time.sleep(max(0.0, self.next_request[host] - time.monotonic()))
        try:
            return self._get(url)
        finally:
            self.next_request[host] = time.monotonic() + delay


def validate_basis(entry: dict) -> None:
    """One selling-basis contract for all adapters; no chemistry or width inference."""
    for key in ("offer_id", "material_id", "supplier", "sku", "region", "quality", "terms", "basis_note"):
        if not isinstance(entry[key], str) or not entry[key].strip():
            raise ValueError(f"missing explicit {key}")
    if not isinstance(entry["currency"], str) or not re.fullmatch(r"[A-Z]{3}", entry["currency"]):
        raise ValueError("currency must be explicit three-letter uppercase code")
    if entry["unit"] not in {"linear_yard", "linear_meter", "square_meter", "kilogram"}:
        raise ValueError("unsupported sale unit; no hide-area or mass inference")
    for key in ("quantity", "minimum_quantity", "order_increment"):
        n = entry[key]
        if type(n) not in (int, float) or not math.isfinite(n) or n <= 0:
            raise ValueError(f"{key} must be finite and positive")
    dt.date.fromisoformat(entry["basis_reviewed_on"])


def observation_time(entry: dict, observed: dt.datetime) -> dt.datetime:
    if observed.tzinfo is None or observed.utcoffset() is None:
        raise ValueError("observation timestamp must have a timezone")
    observed = observed.astimezone(dt.timezone.utc)
    age = (observed.date() - dt.date.fromisoformat(entry["basis_reviewed_on"])).days
    if not 0 <= age <= 30:
        raise ValueError("selling-basis review is future-dated or older than 30 days")
    return observed


def validate_entry(entry: dict) -> None:
    required = set(FIELDS) | {"product_url", "cart_url", "variant_id", "basis_source",
                              "basis_note", "basis_reviewed_on", "required_description_text"}
    if set(entry) != required:
        raise ValueError(f"manifest entry fields differ: {sorted(set(entry) ^ required)}")
    validate_basis(entry)
    if type(entry["variant_id"]) is not int or entry["variant_id"] <= 0:
        raise ValueError("variant_id must be an exact positive integer")
    product = entry["product_url"]
    if origin(product) != origin(entry["cart_url"]) or origin(product) != origin(entry["basis_source"]):
        raise ValueError("product, cart and quantity-basis evidence must share the supplier origin")
    product_path = urllib.parse.urlsplit(product).path
    cart_path = urllib.parse.urlsplit(entry["cart_url"]).path
    if not re.fullmatch(r"/(?:[a-zA-Z-]+/)?products/[a-zA-Z0-9_-]+\.js", product_path):
        raise ValueError("expected locale-aware Shopify /products/HANDLE.js endpoint")
    if cart_path != product_path.split("products/")[0] + "cart.js":
        raise ValueError("cart must use the same locale prefix as the product")
    fragments = entry["required_description_text"]
    if not isinstance(fragments, list) or not fragments or any(not isinstance(s, str) or not s.strip() for s in fragments):
        raise ValueError("at least one reviewed description fragment is required to detect selling-basis drift")


def extract(entry: dict, product_raw: bytes, cart_before_raw: bytes, cart_after_raw: bytes,
            observed: dt.datetime) -> tuple[dict, dict]:
    """Pure adapter. Presence of an offer is not permission to substitute a different material/SKU."""
    validate_entry(entry)
    observed = observation_time(entry, observed)
    product, before, after = map(load, (product_raw, cart_before_raw, cart_after_raw))
    for cart in (before, after):
        if cart.get("currency") != entry["currency"]:
            raise ValueError("presentment currency differs from the reviewed market; no FX guessing")
        if cart.get("item_count") != 0 or cart.get("items") != []:
            raise ValueError("refresh requires an empty anonymous read-only cart session")
    variants = product.get("variants")
    if not isinstance(variants, list) or any(not isinstance(v, dict) for v in variants):
        raise ValueError("product variants missing or malformed")
    matches = [v for v in variants if type(v.get("id")) is int and v["id"] == entry["variant_id"]]
    if len(matches) != 1:
        raise ValueError("exact variant absent or ambiguous; product-level minimum price is not a substitute")
    variant = matches[0]
    if variant.get("sku") != entry["sku"]:
        raise ValueError("variant SKU changed; review the material mapping")
    description = product.get("description")
    if not isinstance(description, str) or not all(s in description for s in entry["required_description_text"]):
        raise ValueError("reviewed selling-basis text changed or is missing")
    amount = variant.get("price")
    if type(amount) is not int or not 0 < amount <= 2**53 - 1:
        raise ValueError("Shopify price must be positive integer hundredths, not a range or compare-at price")
    if type(variant.get("available")) is not bool:
        raise ValueError("exact variant availability must be explicit")
    if product.get("requires_selling_plan") or variant.get("requires_selling_plan"):
        raise ValueError("subscription-only prices need a separate selling-plan adapter")
    digest = hashlib.sha256(product_raw).hexdigest()
    quote = {key: entry[key] for key in FIELDS}
    quote.update(price=amount / 100.0, observed_on=observed.date().isoformat(),
                 available=variant["available"], source=entry["product_url"], source_sha256=digest)
    # Cart response tokens/cookies are NOT persisted. Only the needed currency attestation is retained.
    receipt = {"schema": "ftx-source-refresh/1", "observed_at": observed.isoformat(),
               "manifest_entry": entry, "product_sha256": digest,
               "currency_before": before["currency"], "currency_after": after["currency"],
               "adapter": "shopify_ajax_product_v1", "quantity_basis": "reviewed_manifest_not_inferred",
               "limits": ["No checkout, tax, delivery, geographic availability or textile-physics claim.",
                          "Width comes from the resolved material, not this price response."]}
    return quote, receipt


CSV_FIELDS = {"sku", "price", "currency", "available", "quantity", "unit",
              "minimum_quantity", "order_increment"}


def decimal_number(text: str) -> Decimal:
    """No currency-symbol, thousands-separator, exponent or locale guessing."""
    if not isinstance(text, str) or not re.fullmatch(r"(?:0|[1-9][0-9]*)(?:\.[0-9]+)?", text):
        raise ValueError("expected a plain unsigned decimal number")
    try:
        value = Decimal(text)
        number = float(value)
    except (InvalidOperation, OverflowError) as error:
        raise ValueError("unrepresentable decimal number") from error
    if not math.isfinite(number) or number <= 0:
        raise ValueError("number must be finite, positive and representable")
    return value


def validate_csv(entry: dict) -> None:
    required = set(FIELDS) | {"adapter", "source_url", "basis_source", "basis_note",
                              "basis_reviewed_on", "columns", "match", "availability", "delimiter"}
    if set(entry) != required or entry["adapter"] != "csv_v1":
        raise ValueError("CSV manifest fields differ from csv_v1")
    validate_basis(entry)
    if origin(entry["source_url"]) != origin(entry["basis_source"]):
        raise ValueError("CSV and selling-basis evidence must share the supplier origin")
    columns = entry["columns"]
    if (not isinstance(columns, dict) or set(columns) != CSV_FIELDS
            or any(not isinstance(c, str) or not c.strip() for c in columns.values())
            or len(set(columns.values())) != len(columns)):
        raise ValueError("CSV needs distinct, explicit column names for every observed field")
    matches = entry["match"]
    if (not isinstance(matches, dict) or not matches
            or any(not isinstance(k, str) or not k.strip() or not isinstance(v, str) or not v
                   for k, v in matches.items())
            or matches.get(columns["sku"]) != entry["sku"]):
        raise ValueError("CSV match must include the exact SKU column and reviewed variant/market selectors")
    availability = entry["availability"]
    if not isinstance(availability, dict) or set(availability) != {"available", "unavailable"}:
        raise ValueError("CSV requires explicit available/unavailable token mappings")
    tokens = []
    for choices in availability.values():
        if not isinstance(choices, list) or not choices or any(not isinstance(c, str) or not c for c in choices):
            raise ValueError("CSV availability token lists must be nonempty")
        tokens.extend(choices)
    if len(tokens) != len(set(tokens)):
        raise ValueError("CSV availability tokens overlap or repeat")
    if entry["delimiter"] not in (",", ";", "\t"):
        raise ValueError("CSV delimiter must explicitly be comma, semicolon or tab")


def extract_csv(entry: dict, source_raw: bytes, observed: dt.datetime) -> tuple[dict, dict]:
    """Select one row, checking the source quantity and unit against the reviewed material mapping."""
    validate_csv(entry)
    observed = observation_time(entry, observed)
    if len(source_raw) > LIMIT:
        raise ValueError("source exceeds 4 MiB")
    text = source_raw.decode("utf-8-sig")
    if "\x00" in text:
        raise ValueError("NUL in CSV source")
    columns = entry["columns"]
    try:
        reader = csv.reader(io.StringIO(text, newline=""), delimiter=entry["delimiter"], strict=True)
        header = next(reader, [])
        if not header or any(not h.strip() for h in header) or len(set(header)) != len(header):
            raise ValueError("CSV needs unique nonempty column headers")
        if not (set(columns.values()) | set(entry["match"])) <= set(header):
            raise ValueError("reviewed CSV columns disappeared")
        selected = []
        for number, values in enumerate(reader, start=2):
            if number > 100_001 or len(values) != len(header):
                raise ValueError("CSV has too many rows or an inconsistent column count")
            row = dict(zip(header, values))
            if all(row[key] == value for key, value in entry["match"].items()):
                selected.append((number, row))
                if len(selected) > 1:
                    raise ValueError("CSV exact offer is ambiguous; add variant/market/tier selectors")
    except csv.Error as error:
        raise ValueError(f"malformed CSV: {error}") from error
    if len(selected) != 1:
        raise ValueError("exact CSV offer absent; no similar-SKU fallback")
    number, row = selected[0]
    for key in ("sku", "currency", "unit"):
        if row[columns[key]] != entry[key]:
            raise ValueError(f"CSV {key} changed from the reviewed selling basis")
    for key in ("quantity", "minimum_quantity", "order_increment"):
        if decimal_number(row[columns[key]]) != Decimal(str(entry[key])):
            raise ValueError(f"CSV {key} changed from the reviewed selling basis")
    stock = row[columns["available"]]
    if stock not in entry["availability"]["available"] + entry["availability"]["unavailable"]:
        raise ValueError("unknown CSV availability; missing stock is not in stock")
    digest = hashlib.sha256(source_raw).hexdigest()
    quote = {key: entry[key] for key in FIELDS}
    quote.update(price=float(decimal_number(row[columns["price"]])), observed_on=observed.date().isoformat(),
                 available=stock in entry["availability"]["available"],
                 source=entry["source_url"], source_sha256=digest)
    receipt = {"schema": "ftx-source-refresh/1", "adapter": "csv_v1", "observed_at": observed.isoformat(),
               "manifest_entry": entry, "source_sha256": digest, "selected_record": number,
               "quantity_basis": "source_columns_checked_against_reviewed_manifest",
               "limits": ["No FX, tax/shipping, delivery, market-survey or textile-physics claim.",
                          "Variant, region, quality and terms require reviewed exact row selectors."]}
    return quote, receipt


class JsonLdScripts(HTMLParser):
    """Read data scripts only: never execute JavaScript, fetch src, or expand remote contexts."""
    def __init__(self):
        super().__init__(convert_charrefs=False)
        self.scripts = []
        self.current = None

    def handle_starttag(self, tag, attrs):
        if tag != "script":
            return
        attributes = dict(attrs)
        mime = attributes.get("type") or ""
        if mime.strip().lower() == "application/ld+json":
            if len(attributes) != len(attrs) or "src" in attributes:
                raise ValueError("JSON-LD must be inline with unambiguous attributes")
            if len(self.scripts) >= 64:
                raise ValueError("too many JSON-LD scripts")
            self.current = []

    def handle_data(self, data):
        if self.current is not None:
            self.current.append(data)

    def handle_endtag(self, tag):
        if tag == "script" and self.current is not None:
            self.scripts.append("".join(self.current))
            self.current = None


def schema_type(node, name):
    types = node.get("@type", [])
    types = types if isinstance(types, list) else [types]
    return any(t in (name, "https://schema.org/" + name, "http://schema.org/" + name) for t in types)


def jsonld_nodes(source_raw: bytes, source_format: str) -> tuple[list[dict], dict[str, dict]]:
    if len(source_raw) > LIMIT:
        raise ValueError("source exceeds 4 MiB")
    if source_format == "html":
        parser = JsonLdScripts()
        parser.feed(source_raw.decode("utf-8-sig")); parser.close()
        if parser.current is not None or not parser.scripts:
            raise ValueError("missing or unclosed JSON-LD data script")
        documents = [load(s.encode(), object_only=False) for s in parser.scripts]
    else:
        documents = [load(source_raw, object_only=False)]
    nodes, identifiers = [], {}
    stack = [(d, 0, False) for d in documents]
    visited = 0
    while stack:
        value, depth, context_known = stack.pop()
        visited += 1
        if visited > 16384 or depth > 32:
            raise ValueError("JSON-LD graph exceeds node/depth bounds")
        if isinstance(value, list):
            stack.extend((v, depth + 1, context_known) for v in value)
        elif isinstance(value, dict):
            if "@context" in value:
                context = value["@context"]
                if isinstance(context, dict) and set(context) == {"@vocab"}:
                    context = context["@vocab"]
                if context not in ("http://schema.org", "https://schema.org", "http://schema.org/", "https://schema.org/"):
                    raise ValueError("unsupported JSON-LD context; aliases/remapping are not inferred")
                context_known = True
            if "@type" in value and not context_known:
                raise ValueError("typed JSON-LD node lacks a supported schema.org context")
            nodes.append(value)
            if "@id" in value:
                identifier = value["@id"]
                if not isinstance(identifier, str) or not identifier:
                    raise ValueError("invalid JSON-LD identifier")
                if set(value) != {"@id"}:
                    if identifier in identifiers:
                        raise ValueError("ambiguous JSON-LD node identity")
                    identifiers[identifier] = value
            stack.extend((v, depth + 1, context_known) for k, v in value.items() if k != "@context")
    return nodes, identifiers


def validate_jsonld(entry: dict) -> None:
    required = set(FIELDS) | {"adapter", "source_url", "basis_source", "basis_note", "basis_reviewed_on",
                              "source_format", "offer_ref", "tax_included", "required_description_text"}
    if set(entry) != required or entry["adapter"] != "jsonld_v1":
        raise ValueError("JSON-LD manifest fields differ from jsonld_v1")
    validate_basis(entry)
    if origin(entry["source_url"]) != origin(entry["basis_source"]):
        raise ValueError("JSON-LD and selling-basis evidence must share the supplier origin")
    if entry["source_format"] not in ("html", "json"):
        raise ValueError("JSON-LD source_format must be html or json")
    if entry["offer_ref"] is not None and (not isinstance(entry["offer_ref"], str) or not entry["offer_ref"]):
        raise ValueError("offer_ref must be null or an exact nonempty @id")
    if entry["tax_included"] is not None and type(entry["tax_included"]) is not bool:
        raise ValueError("tax_included must be null or a reviewed boolean")
    fragments = entry["required_description_text"]
    if not isinstance(fragments, list) or not fragments or any(not isinstance(s, str) or not s.strip() for s in fragments):
        raise ValueError("JSON-LD needs reviewed product-description fragments for its selling basis")


def valid_period(node: dict, observed: dt.datetime) -> str | None:
    expiries = []
    for key, before in (("priceValidUntil", False), ("validFrom", True), ("validThrough", False),
                        ("availabilityStarts", True), ("availabilityEnds", False)):
        if key not in node:
            continue
        value = node[key]
        if not isinstance(value, str):
            raise ValueError(f"invalid {key}")
        if re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", value):
            date = dt.date.fromisoformat(value)
            valid = observed.date() >= date if before else observed.date() <= date
            if not before:
                expiries.append(value)
        else:
            if not before:
                raise ValueError("intraday expiry cannot be represented by the calendar-date quote schema")
            instant = dt.datetime.fromisoformat(value)
            if instant.tzinfo is None or instant.utcoffset() is None:
                raise ValueError("JSON-LD validity datetime needs an explicit timezone")
            valid = observed >= instant if before else observed <= instant
        if not valid:
            raise ValueError(f"offer is not currently valid: {key}")
    return min(expiries, default=None)


def extract_jsonld(entry: dict, source_raw: bytes, observed: dt.datetime) -> tuple[dict, dict]:
    validate_jsonld(entry)
    observed = observation_time(entry, observed)
    nodes, identifiers = jsonld_nodes(source_raw, entry["source_format"])
    def resolve(node):
        if isinstance(node, dict) and set(node) == {"@id"}:
            node = identifiers.get(node["@id"])
        if not isinstance(node, dict):
            raise ValueError("missing embedded JSON-LD node; external references are not fetched")
        return node
    products = [n for n in nodes if schema_type(n, "Product") and n.get("sku") == entry["sku"]]
    if len(products) != 1:
        raise ValueError("exact JSON-LD Product SKU absent or ambiguous")
    product = products[0]
    description = product.get("description")
    if not isinstance(description, str) or not all(s in description for s in entry["required_description_text"]):
        raise ValueError("JSON-LD reviewed selling-basis description changed")
    offered = product.get("offers")
    offers = [resolve(n) for n in (offered if isinstance(offered, list) else [offered])]
    if entry["offer_ref"] is not None:
        offers = [o for o in offers if o.get("@id") == entry["offer_ref"]]
    if len(offers) != 1 or not schema_type(offers[0], "Offer") or schema_type(offers[0], "AggregateOffer"):
        raise ValueError("need one exact ordinary Offer, not an aggregate/minimum price")
    offer = offers[0]
    if "sku" in offer and offer["sku"] != entry["sku"]:
        raise ValueError("JSON-LD Offer SKU differs from Product")
    if "itemOffered" in offer and resolve(offer["itemOffered"]).get("sku") != entry["sku"]:
        raise ValueError("JSON-LD Offer references a different product")
    for key in ("eligibleCustomerType", "eligibleQuantity", "eligibleDuration", "eligibleTransactionVolume",
                "validForMemberTier", "addOn", "includesObject", "leaseLength"):
        if key in offer:
            raise ValueError(f"conditional/bundled JSON-LD offer is unsupported: {key}")
    if offer.get("businessFunction", "http://purl.org/goodrelations/v1#Sell") != "http://purl.org/goodrelations/v1#Sell":
        raise ValueError("only ordinary sales are supported")
    for key in ("eligibleRegion", "ineligibleRegion"):
        if key in offer:
            regions = offer[key] if isinstance(offer[key], list) else [offer[key]]
            if not regions or any(not isinstance(r, str) or not r for r in regions):
                raise ValueError("JSON-LD region geometry is not an exact market code")
            matches = entry["region"] in regions
            if matches != (key == "eligibleRegion"):
                raise ValueError("JSON-LD offer excludes the reviewed market")
    expiry = valid_period(offer, observed)
    amounts = []
    specifications = [offer]
    if "priceSpecification" in offer:
        spec = resolve(offer["priceSpecification"])
        if not schema_type(spec, "PriceSpecification") or set(spec) - {"@id", "@type", "price", "priceCurrency", "valueAddedTaxIncluded"}:
            raise ValueError("conditional/unit/list price specifications are unsupported")
        if "valueAddedTaxIncluded" in spec and type(spec["valueAddedTaxIncluded"]) is not bool:
            raise ValueError("invalid tax inclusion observation")
        specifications.append(spec)
    for spec in specifications:
        if "valueAddedTaxIncluded" in spec and spec["valueAddedTaxIncluded"] is not entry["tax_included"]:
            raise ValueError("source tax inclusion differs from the reviewed basis")
        if "priceCurrency" in spec and spec["priceCurrency"] != entry["currency"]:
            raise ValueError("JSON-LD currency differs from reviewed market")
        if "price" in spec:
            if spec.get("priceCurrency") != entry["currency"] or type(spec["price"]) not in (str, int, float):
                raise ValueError("JSON-LD price needs its own explicit currency and scalar number")
            amounts.append(decimal_number(str(spec["price"])))
    if not amounts or any(a != amounts[0] for a in amounts):
        raise ValueError("missing or conflicting JSON-LD prices")
    if entry["tax_included"] is not None and not any("valueAddedTaxIncluded" in s for s in specifications):
        raise ValueError("reviewed source tax-inclusion observation disappeared")
    stock = offer.get("availability")
    stock_values = {prefix + name: available for prefix in ("http://schema.org/", "https://schema.org/")
                    for name, available in (("InStock", True), ("LimitedAvailability", True),
                                             ("OutOfStock", False), ("SoldOut", False), ("Discontinued", False))}
    if not isinstance(stock, str) or stock not in stock_values:
        raise ValueError("JSON-LD stock missing, unknown, preorder or backorder")
    digest = hashlib.sha256(source_raw).hexdigest()
    quote = {key: entry[key] for key in FIELDS}
    quote.update(price=float(amounts[0]), observed_on=observed.date().isoformat(), available=stock_values[stock],
                 source=entry["source_url"], source_sha256=digest)
    if expiry is not None:
        quote["valid_through"] = expiry
    receipt = {"schema": "ftx-source-refresh/1", "adapter": "jsonld_v1", "observed_at": observed.isoformat(),
               "manifest_entry": entry, "source_sha256": digest, "product_ref": product.get("@id"),
               "offer_ref": offer.get("@id"), "quantity_basis": "reviewed_manifest_not_inferred",
               "limits": ["No execution, remote context expansion, checkout, FX or textile-physics inference.",
                          "Declared structured data is not independent supplier/stock/terms verification."]}
    return quote, receipt


def refresh(manifest: dict, get, now=None) -> tuple[dict, dict[str, bytes]]:
    if (set(manifest) != {"schema", "offers"}
            or manifest["schema"] not in {"ftx-shopify-refresh/1", "ftx-textile-refresh/1"}):
        raise ValueError("unsupported refresh manifest")
    entries = manifest["offers"]
    if not isinstance(entries, list) or not entries or len(entries) > 100:
        raise ValueError("manifest needs 1..100 explicitly reviewed offers")
    ids = set()
    for entry in entries:
        if not isinstance(entry, dict):
            raise ValueError("each reviewed offer must be an object")
        if manifest["schema"] == "ftx-shopify-refresh/1" or "adapter" not in entry:
            validate_entry(entry)
        elif entry.get("adapter") == "csv_v1":
            validate_csv(entry)
        elif entry.get("adapter") == "jsonld_v1":
            validate_jsonld(entry)
        else:
            raise ValueError("unknown source adapter")
        if entry["offer_id"] in ids:
            raise ValueError("duplicate offer identity in refresh manifest")
        ids.add(entry["offer_id"])
    clock = now or (lambda: dt.datetime.now(dt.timezone.utc))
    quotes, evidence = [], {}
    sources = {}
    for entry in entries:
        if entry.get("adapter") in ("csv_v1", "jsonld_v1"):
            url = entry["source_url"]
            if url not in sources:
                sources[url] = (get(url), clock())
            product, observed = sources[url]
            if entry["adapter"] == "csv_v1":
                quote, receipt = extract_csv(entry, product, observed)
                extension = ".csv"
            else:
                quote, receipt = extract_jsonld(entry, product, observed)
                extension = ".html" if entry["source_format"] == "html" else ".json"
        else:
            before = get(entry["cart_url"])
            product = get(entry["product_url"])
            after = get(entry["cart_url"])
            quote, receipt = extract(entry, product, before, after, clock())
            extension = ".json"
        quotes.append(quote)
        evidence[quote["source_sha256"] + extension] = product
        raw = (json.dumps(receipt, sort_keys=True, indent=2, allow_nan=False) + "\n").encode()
        evidence[hashlib.sha256(raw).hexdigest() + ".receipt.json"] = raw
    quotes.sort(key=lambda q: q["offer_id"])
    return {"schema": SCHEMA, "quotes": quotes}, evidence


def create_snapshot(path: Path, raw: bytes) -> None:
    """Publish a complete local file without overwriting any previous evidence or snapshot."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=".ftx-refresh-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        os.link(name, path)  # atomic, fails if destination exists; no partially published output
    finally:
        os.unlink(name)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--out", required=True, type=Path, help="NEW quotes snapshot; existing files are never overwritten")
    parser.add_argument("--evidence", required=True, type=Path)
    args = parser.parse_args()
    try:
        if args.out.exists():
            raise ValueError("output exists; use a new timestamped snapshot path")
        manifest = load(args.manifest.read_bytes())
        book, evidence = refresh(manifest, PublicSession().get)
        for name, raw in evidence.items():
            target = args.evidence / name
            try:
                create_snapshot(target, raw)
            except FileExistsError:
                if target.read_bytes() != raw:
                    raise ValueError("existing evidence does not match its digest")
        create_snapshot(args.out, (json.dumps(book, indent=2, allow_nan=False) + "\n").encode())
        print(f"wrote {len(book['quotes'])} exact offers to {args.out}; evidence: {args.evidence}")
        return 0
    except (ValueError, KeyError, TypeError, OverflowError, OSError, urllib.error.URLError) as error:
        parser.exit(1, f"textile refresh refused: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
