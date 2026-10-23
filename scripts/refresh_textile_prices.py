#!/usr/bin/env python3
"""Read reviewed public supplier offers into the existing textile quote workbench.

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

AGENT = "OpenAI File Downloader, XaiImageApiFetch/1.0"
# REP product token, as advertised in the required HTTP identification above.
ROBOTS_AGENT = "XaiImageApiFetch"
LIMIT = 4 * 1024 * 1024
SCHEMA = "ftx-textile-quotes/1"
INTRADAY_SCHEMA = "ftx-textile-quotes/2"
FIELDS = ("offer_id", "material_id", "supplier", "sku", "quantity", "unit",
          "minimum_quantity", "order_increment", "currency", "region", "quality", "terms")
OPTIONAL_FIELDS = {"material_sha256", "maximum_continuous_quantity", "continuous_cut_text"}


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


def robots_octets(value: str) -> str:
    """RFC 9309 comparison form: decode unreserved escapes, retain reserved octets."""
    unreserved = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~"
    data = value.encode("utf-8")
    result, index = [], 0
    while index < len(data):
        byte = data[index]
        if byte == 37 and re.fullmatch(b"[0-9A-Fa-f]{2}", data[index + 1:index + 3]):
            byte = int(data[index + 1:index + 3], 16)
            result.append(chr(byte) if byte in unreserved else f"%{byte:02X}")
            index += 3
        else:
            # Literal * and $ in a URL are data; only the rule parser supplies operators.
            result.append(f"%{byte:02X}" if byte < 33 or byte >= 127 or byte in b"%*$" else chr(byte))
            index += 1
    return "".join(result)


def robots_match(parts: tuple[str, ...], anchored: bool, path: str) -> bool:
    """Prefix glob matching without backtracking regexes from untrusted rules."""
    if not path.startswith(parts[0]):
        return False
    offset = len(parts[0])
    if len(parts) == 1:
        return not anchored or offset == len(path)
    for part in parts[1:-1]:
        found = path.find(part, offset)
        if found < 0:
            return False
        offset = found + len(part)
    if anchored:
        return path.endswith(parts[-1]) and len(path) - len(parts[-1]) >= offset
    return path.find(parts[-1], offset) >= 0


class RobotsPolicy:
    """RFC 9309 groups and longest path rules, with conservative pacing extensions.

    https://www.rfc-editor.org/rfc/rfc9309.html#section-2.2
    Equal-specificity agent groups merge; * supplies rules only without a named match.
    """
    def __init__(self, text: str, agent: str = ROBOTS_AGENT):
        groups, names, rules, delays = [], [], [], []
        in_rules = False
        for line in text.lstrip("\ufeff").splitlines():
            field, separator, value = line.partition("#")[0].partition(":")
            if not separator:
                continue
            field, value = field.strip().lower(), value.strip()
            if field == "user-agent":
                # Accept common /version and trailing-* decorations on product tokens.
                token = value.split("/", 1)[0].lower()
                token = token if token == "*" else token.rstrip("*")
                if token != "*" and not re.fullmatch(r"[a-z_-]+", token):
                    continue
                if in_rules:
                    groups.append((names, rules, delays))
                    names, rules, delays, in_rules = [], [], [], False
                names.append(token)
            elif names and field in ("allow", "disallow", "crawl-delay", "request-rate"):
                in_rules = True
                if field in ("allow", "disallow"):
                    if not value or not value.startswith(("/", "*")):
                        continue
                    anchored = value.endswith("$")
                    parts = tuple(robots_octets(p) for p in (value[:-1] if anchored else value).split("*"))
                    length = sum(map(len, parts)) + len(parts) - 1 + anchored
                    rules.append((length, field == "allow", parts, anchored))
                elif field == "crawl-delay" and re.fullmatch(r"[0-9]+(?:\.[0-9]+)?", value):
                    delay = float(value)
                    if math.isfinite(delay):
                        delays.append(delay)
                elif field == "request-rate" and re.fullmatch(r"[0-9]+\s*/\s*[0-9]+", value):
                    requests, seconds = map(int, value.split("/"))
                    if requests and seconds:
                        delay = seconds / requests
                        if math.isfinite(delay):
                            delays.append(delay)
        if names:
            groups.append((names, rules, delays))
        scores = [max((len(n) for n in names if n != "*" and n in agent.lower()), default=0)
                  for names, _, _ in groups]
        best = max(scores, default=0)
        selected = [g for g, score in zip(groups, scores) if score == best and (best or "*" in g[0])]
        self.rules = [rule for _, rules, _ in selected for rule in rules]
        # Keep the existing global pacing fallback, conservatively honoring the largest interval.
        pacing = selected + [g for g in groups if "*" in g[0]]
        self.delay = max([1.0] + [d for _, _, delays in pacing for d in delays])

    def can_fetch(self, url: str) -> bool:
        parsed = urllib.parse.urlsplit(url)
        path = robots_octets((parsed.path or "/") + ("?" + parsed.query if parsed.query else ""))
        if path == "/robots.txt":
            return True
        matches = [(length, allow) for length, allow, parts, anchored in self.rules
                   if robots_match(parts, anchored, path)]
        return max(matches, default=(0, True))[1]


class SameOrigin(urllib.request.HTTPRedirectHandler):
    def __init__(self, before_request):
        super().__init__()
        self.before_request = before_request

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if origin(req.full_url) != origin(newurl):
            raise ValueError("cross-origin redirect refused; review the supplier URL")
        self.before_request(newurl)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


class PublicSession:
    """A fresh anonymous cookie jar, bounded responses, robots checks and serial request pacing."""
    def __init__(self):
        self.opener = urllib.request.build_opener(
            SameOrigin(self._before_request), urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.policies = {}
        self.next_request = {}

    def _before_request(self, url):
        host = origin(url)
        policy = self.policies.get(host)
        if policy is None:  # Initial robots.txt acquisition, including same-origin redirects.
            return
        if not policy.can_fetch(url):
            raise ValueError(f"robots.txt disallows {url}; no bypass attempted")
        time.sleep(max(0.0, self.next_request[host] - time.monotonic()))
        self.next_request[host] = time.monotonic() + policy.delay

    def _get(self, url, *, accept="*/*"):
        self._before_request(url)
        req = urllib.request.Request(url, headers={"User-Agent": AGENT, "Accept": accept})
        with self.opener.open(req, timeout=20) as response:
            if origin(response.url) != origin(url):
                raise ValueError("source origin changed")
            raw = response.read(LIMIT + 1)
        if len(raw) > LIMIT:
            raise ValueError("source exceeds 4 MiB")
        return raw

    def get(self, url: str, *, accept="*/*") -> bytes:
        host = origin(url)
        if host not in self.policies:
            try:
                robots = self._get(host + "/robots.txt", accept="text/plain").decode("utf-8")
            except urllib.error.HTTPError as error:
                if error.code != 404:
                    raise
                robots = "User-agent: *\nDisallow:"
            self.policies[host] = RobotsPolicy(robots)
            self.next_request[host] = time.monotonic() + self.policies[host].delay
        policy = self.policies[host]
        try:
            return self._get(url, accept=accept)
        finally:
            self.next_request[host] = time.monotonic() + policy.delay


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
    if "material_sha256" in entry and (not isinstance(entry["material_sha256"], str)
            or not re.fullmatch(r"[0-9a-fA-F]{64}", entry["material_sha256"])):
        raise ValueError("material_sha256 must be the reviewed exact material snapshot hash")
    maximum = entry.get("maximum_continuous_quantity")
    if "maximum_continuous_quantity" in entry:
        if (type(maximum) not in (int, float) or not math.isfinite(maximum) or maximum <= 0
                or entry["unit"] not in ("linear_yard", "linear_meter")):
            raise ValueError("maximum_continuous_quantity needs a positive finite linear sale quantity")
        if entry.get("adapter") == "csv_v1":
            if (not isinstance(entry.get("columns"), dict)
                    or "maximum_continuous_quantity" not in entry["columns"]
                    or "continuous_cut_text" in entry):
                raise ValueError("CSV continuous-cut limits need an explicit source column")
        else:
            fragment = entry.get("continuous_cut_text")
            if (not isinstance(fragment, str) or not fragment.strip()
                    or not isinstance(entry.get("required_description_text"), list)
                    or fragment not in entry.get("required_description_text", [])):
                raise ValueError("continuous-cut limits need their exact reviewed description fragment")
    elif ("continuous_cut_text" in entry
            or (isinstance(entry.get("columns"), dict)
                and "maximum_continuous_quantity" in entry["columns"])):
        raise ValueError("continuous-cut source mapping requires its reviewed maximum quantity")
    dt.date.fromisoformat(entry["basis_reviewed_on"])


def quote_basis(entry: dict) -> dict:
    return {key: entry[key] for key in (*FIELDS, *sorted(OPTIONAL_FIELDS - {"continuous_cut_text"}))
            if key in entry}


def observation_time(entry: dict, observed: dt.datetime) -> dt.datetime:
    if observed.tzinfo is None or observed.utcoffset() is None:
        raise ValueError("observation timestamp must have a timezone")
    observed = observed.astimezone(dt.timezone.utc)
    age = (observed.date() - dt.date.fromisoformat(entry["basis_reviewed_on"])).days
    if not 0 <= age <= 30:
        raise ValueError("selling-basis review is future-dated or older than 30 days")
    return observed


def utc_instant(value: dt.datetime) -> str:
    """Preserve Python's exact microsecond clock resolution, normalized to explicit UTC."""
    if value.tzinfo is None or value.utcoffset() is None:
        raise ValueError("quote instant needs an explicit timezone")
    return value.astimezone(dt.timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z")


def stamp_intraday(quote: dict, receipt: dict, observed: dt.datetime, intraday: bool) -> None:
    if intraday:
        quote["observed_at"] = utc_instant(observed)
        receipt["quote_schema"] = INTRADAY_SCHEMA


def validate_entry(entry: dict) -> None:
    required = set(FIELDS) | {"product_url", "cart_url", "variant_id", "basis_source",
                              "basis_note", "basis_reviewed_on", "required_description_text"}
    if set(entry) - OPTIONAL_FIELDS != required:
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
            observed: dt.datetime, *, intraday=False) -> tuple[dict, dict]:
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
    quote = quote_basis(entry)
    quote.update(price=amount / 100.0, observed_on=observed.date().isoformat(),
                 available=variant["available"], source=entry["product_url"], source_sha256=digest)
    # Cart response tokens/cookies are NOT persisted. Only the needed currency attestation is retained.
    receipt = {"schema": "ftx-source-refresh/1", "observed_at": observed.isoformat(),
               "manifest_entry": entry, "product_sha256": digest,
               "currency_before": before["currency"], "currency_after": after["currency"],
               "adapter": "shopify_ajax_product_v1", "quantity_basis": "reviewed_manifest_not_inferred",
               "limits": ["No checkout, tax, delivery, geographic availability or textile-physics claim.",
                          "Width comes from the resolved material, not this price response."]}
    stamp_intraday(quote, receipt, observed, intraday)
    return quote, receipt


CSV_FIELDS = {"sku", "price", "currency", "available", "quantity", "unit",
              "minimum_quantity", "order_increment"}


def decimal_number(text: str, *, allow_zero=False) -> Decimal:
    """No currency-symbol, thousands-separator, exponent or locale guessing."""
    if not isinstance(text, str) or not re.fullmatch(r"(?:0|[1-9][0-9]*)(?:\.[0-9]+)?", text):
        raise ValueError("expected a plain unsigned decimal number")
    try:
        value = Decimal(text)
        number = float(value)
    except (InvalidOperation, OverflowError) as error:
        raise ValueError("unrepresentable decimal number") from error
    if not math.isfinite(number) or number < 0 or (number == 0 and (not allow_zero or value != 0)):
        raise ValueError("number must be finite and representable; zero is allowed only for observed stock")
    return value


def validate_public_product(entry: dict) -> None:
    """A reviewed public-product currency source, without requiring a cart endpoint."""
    required = set(FIELDS) | {"adapter", "product_url", "public_url", "product_id", "variant_id",
        "variant_title", "basis_source", "basis_note", "basis_reviewed_on",
        "required_description_text", "inventory_mode", "reviewed_valid_through"}
    if set(entry) - OPTIONAL_FIELDS != required or entry["adapter"] != "shopify_public_product_v1":
        raise ValueError("public product manifest fields differ from shopify_public_product_v1")
    validate_basis(entry)
    product = entry["product_url"]
    if (not re.fullmatch(r"/(?:[a-zA-Z-]+/)?products/[a-zA-Z0-9_-]+\.js",
                         urllib.parse.urlsplit(product).path)
            or entry["public_url"] != product[:-3] + ".json"
            or origin(product) != origin(entry["basis_source"])):
        raise ValueError("public JSON and Ajax must identify the same reviewed supplier product")
    for key in ("product_id", "variant_id"):
        if type(entry[key]) is not int or not 0 < entry[key] <= 2**53 - 1:
            raise ValueError(f"{key} must be an exact positive integer")
    if not isinstance(entry["variant_title"], str) or not entry["variant_title"].strip():
        raise ValueError("exact reviewed variant title is required")
    fragments = entry["required_description_text"]
    if not isinstance(fragments, list) or not fragments or any(not isinstance(s, str) or not s.strip() for s in fragments):
        raise ValueError("reviewed description fragments are required")
    if entry["inventory_mode"] not in {"tracked_deny", "unreported"}:
        raise ValueError("inventory_mode must explicitly select tracked_deny or unreported")
    if entry["reviewed_valid_through"] is not None:
        dt.date.fromisoformat(entry["reviewed_valid_through"])


def public_variant(product: dict, entry: dict) -> dict:
    if type(product.get("id")) is not int or product["id"] != entry["product_id"]:
        raise ValueError("reviewed product identity changed")
    variants = product.get("variants")
    if not isinstance(variants, list) or any(not isinstance(v, dict) for v in variants):
        raise ValueError("product variants missing or malformed")
    selected = [v for v in variants if type(v.get("id")) is int and v["id"] == entry["variant_id"]]
    if len(selected) != 1:
        raise ValueError("exact variant absent or ambiguous")
    variant = selected[0]
    if variant.get("sku") != entry["sku"] or variant.get("title") != entry["variant_title"]:
        raise ValueError("variant SKU or reviewed title changed")
    return variant


def extract_public_product(entry: dict, product_raw: bytes, before_raw: bytes, after_raw: bytes,
                           observed: dt.datetime, *, intraday=False) -> tuple[dict, dict]:
    """Cross-check exact public observations; numeric stock is never inferred from availability."""
    validate_public_product(entry)
    observed = observation_time(entry, observed)
    product = load(product_raw)
    before, after = (load(raw).get("product") for raw in (before_raw, after_raw))
    if not isinstance(before, dict) or not isinstance(after, dict):
        raise ValueError("public JSON needs a product object")
    variants = [public_variant(p, entry) for p in (before, product, after)]
    b, variant, a = variants
    descriptions = [before.get("body_html"), product.get("description"), after.get("body_html")]
    if (any(not isinstance(d, str) or not all(s in d for s in entry["required_description_text"])
            for d in descriptions) or len(set(descriptions)) != 1):
        raise ValueError("reviewed description changed or differs between source representations")
    amount = variant.get("price")
    if type(amount) is not int or not 0 < amount <= 2**53 - 1:
        raise ValueError("Ajax price must be positive integer hundredths")
    for public in (b, a):
        if (public.get("product_id") != entry["product_id"]
                or public.get("price_currency") != entry["currency"]
                or decimal_number(public.get("price")) != Decimal(amount) / 100):
            raise ValueError("public currency or price changed; no presentment or FX inference")
    for v in variants:
        rule = v.get("quantity_rule")
        if (not isinstance(rule, dict) or set(rule) != {"min", "max", "increment"}
                or rule["max"] is not None
                or any(type(rule[k]) is not int or rule[k] <= 0 for k in ("min", "increment"))
                or Decimal(rule["min"]) * Decimal(str(entry["quantity"])) != Decimal(str(entry["minimum_quantity"]))
                or Decimal(rule["increment"]) * Decimal(str(entry["quantity"])) != Decimal(str(entry["order_increment"]))
                or v.get("quantity_price_breaks") != []):
            raise ValueError("quantity rules changed, are bounded, or contain conditional prices")
    if product.get("requires_selling_plan") or variant.get("requires_selling_plan"):
        raise ValueError("selling-plan prices require separate qualification")
    if type(variant.get("available")) is not bool:
        raise ValueError("exact variant availability must be explicit")
    capacity = None
    if entry["inventory_mode"] == "tracked_deny":
        quantities = [v.get("inventory_quantity") for v in variants]
        if (any(v.get("inventory_management") != "shopify" or v.get("inventory_policy") != "deny" for v in variants)
                or any(type(q) is not int or not 0 <= q <= 2**53 - 1 for q in quantities)
                or len(set(quantities)) != 1 or (quantities[0] == 0 and variant["available"])):
            raise ValueError("tracked finite stock missing, conflicting, changing, or permits overselling")
        capacity = float(Decimal(quantities[0]) * Decimal(str(entry["quantity"])))
        if not math.isfinite(capacity) or (quantities[0] > 0 and capacity == 0):
            raise ValueError("observed stock is not representable in the reviewed sale unit")
    elif any("inventory_quantity" in v for v in variants):
        raise ValueError("numeric stock appeared; review the inventory mapping before publication")
    expiry = entry["reviewed_valid_through"]
    if expiry is not None and observed.date() > dt.date.fromisoformat(expiry):
        raise ValueError("independently reviewed source expiry has passed; review it again")
    digest = hashlib.sha256(product_raw).hexdigest()
    quote = quote_basis(entry)
    quote.update(price=amount / 100.0, observed_on=observed.date().isoformat(),
                 available=variant["available"], source=entry["product_url"], source_sha256=digest)
    if capacity is not None:
        quote["available_quantity"] = capacity
    if expiry is not None:
        quote["valid_through"] = expiry
    receipt = {"schema": "ftx-source-refresh/1", "adapter": entry["adapter"],
               "observed_at": observed.isoformat(), "manifest_entry": entry, "product_sha256": digest,
               "public_before_sha256": hashlib.sha256(before_raw).hexdigest(),
               "public_after_sha256": hashlib.sha256(after_raw).hexdigest(),
               "quantity_basis": "reviewed_manifest_cross_checked_against_quantity_rule",
               "limits": ["Public source declarations do not reserve inventory or establish delivery/checkout terms.",
                          "Source expiry outside the product APIs is separately reviewed, never extended by refresh.",
                          "No textile mechanics, usable width, continuous cut length or market population is inferred."]}
    stamp_intraday(quote, receipt, observed, intraday)
    return quote, receipt


def validate_csv(entry: dict) -> None:
    required = set(FIELDS) | {"adapter", "source_url", "basis_source", "basis_note",
                              "basis_reviewed_on", "columns", "match", "availability", "delimiter"}
    if set(entry) - OPTIONAL_FIELDS != required or entry["adapter"] != "csv_v1":
        raise ValueError("CSV manifest fields differ from csv_v1")
    validate_basis(entry)
    if origin(entry["source_url"]) != origin(entry["basis_source"]):
        raise ValueError("CSV and selling-basis evidence must share the supplier origin")
    columns = entry["columns"]
    if (not isinstance(columns, dict)
            or set(columns) - {"available_quantity", "maximum_continuous_quantity", "price_breaks"} != CSV_FIELDS
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


def price_breaks(value, minimum_quantity):
    """Complete all-unit schedule in original sale units, never inferred package counts."""
    if not isinstance(value, list) or len(value) > 64:
        raise ValueError("price_breaks must be an array of at most 64 all-unit thresholds")
    previous = minimum_quantity
    for tier in value:
        if not isinstance(tier, dict) or set(tier) - {"sku"} != {"minimum_quantity", "price"}:
            raise ValueError("price break needs minimum_quantity and price, with only an optional sku")
        if "sku" in tier and (not isinstance(tier["sku"], str) or not tier["sku"].strip()):
            raise ValueError("a source tier SKU must be nonempty text")
        for key in ("minimum_quantity", "price"):
            number = tier[key]
            try:
                finite = type(number) in (int, float) and math.isfinite(number)
            except OverflowError:
                finite = False
            if not finite or number <= 0:
                raise ValueError("price-break quantities and prices must be finite and positive")
        if tier["minimum_quantity"] <= previous:
            raise ValueError("price-break quantities must increase strictly above the base minimum")
        previous = tier["minimum_quantity"]
    return value


def extract_csv(entry: dict, source_raw: bytes, observed: dt.datetime, *, intraday=False) -> tuple[dict, dict]:
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
    checked_quantities = ("quantity", "minimum_quantity", "order_increment")
    if "maximum_continuous_quantity" in columns:
        checked_quantities += ("maximum_continuous_quantity",)
    for key in checked_quantities:
        if decimal_number(row[columns[key]]) != Decimal(str(entry[key])):
            raise ValueError(f"CSV {key} changed from the reviewed selling basis")
    stock = row[columns["available"]]
    if stock not in entry["availability"]["available"] + entry["availability"]["unavailable"]:
        raise ValueError("unknown CSV availability; missing stock is not in stock")
    digest = hashlib.sha256(source_raw).hexdigest()
    quote = quote_basis(entry)
    quote.update(price=float(decimal_number(row[columns["price"]])), observed_on=observed.date().isoformat(),
                 available=stock in entry["availability"]["available"],
                 source=entry["source_url"], source_sha256=digest)
    if "available_quantity" in columns:
        quote["available_quantity"] = float(decimal_number(row[columns["available_quantity"]], allow_zero=True))
    if "price_breaks" in columns:
        schedule = price_breaks(load(row[columns["price_breaks"]].encode(), object_only=False),
                                entry["minimum_quantity"])
        if schedule:
            quote["price_breaks"] = schedule
    receipt = {"schema": "ftx-source-refresh/1", "adapter": "csv_v1", "observed_at": observed.isoformat(),
               "manifest_entry": entry, "source_sha256": digest, "selected_record": number,
               "quantity_basis": "source_columns_checked_against_reviewed_manifest",
               "limits": ["No FX, tax/shipping, delivery, market-survey or textile-physics claim.",
                          "Variant, region, quality and terms require reviewed exact row selectors."]}
    stamp_intraday(quote, receipt, observed, intraday)
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
    if set(entry) - OPTIONAL_FIELDS - {"required_page_text"} != required or entry["adapter"] != "jsonld_v1":
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
    if "required_page_text" in entry:
        fragments = entry["required_page_text"]
        if (entry["source_format"] != "html" or not isinstance(fragments, list) or not fragments
                or any(not isinstance(s, str) or not s.strip() for s in fragments)):
            raise ValueError("required_page_text needs nonempty exact text fragments and an HTML source")


def valid_period(node: dict, observed: dt.datetime, *, intraday=False) -> tuple[str | None, str | None]:
    expiries, instant_expiries = [], []
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
            if not before and not intraday:
                raise ValueError("intraday expiry cannot be represented by the calendar-date quote schema")
            if intraday and (not re.fullmatch(
                    r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,6})?(?:Z|[+-][0-9]{2}:[0-9]{2})", value)
                    or value.endswith("-00:00")):
                raise ValueError("intraday source validity needs explicit ISO timezone and at most six fractional digits")
            # Python 3.14+ accepts 24:00:00 as the next midnight; the source schema does not.
            if intraday and (int(value[11:13]) > 23 or int(value[14:16]) > 59 or int(value[17:19]) > 59):
                raise ValueError("intraday source clock hours/minutes/seconds are out of range")
            # datetime.fromisoformat normalizes offset minutes such as +01:60. Reject those
            # spellings before conversion; a supplier's malformed clock is not a new instant.
            if intraday and not value.endswith("Z") and (int(value[-5:-3]) > 23 or int(value[-2:]) > 59):
                raise ValueError("intraday source timezone hours/minutes are out of range")
            instant = dt.datetime.fromisoformat(value)
            if instant.tzinfo is None or instant.utcoffset() is None:
                raise ValueError("JSON-LD validity datetime needs an explicit timezone")
            valid = observed >= instant if before else observed <= instant
            if not before:
                instant_expiries.append(instant.astimezone(dt.timezone.utc))
        if not valid:
            raise ValueError(f"offer is not currently valid: {key}")
    expiry_at = min(instant_expiries, default=None)
    return min(expiries, default=None), utc_instant(expiry_at) if expiry_at is not None else None


def extract_jsonld(entry: dict, source_raw: bytes, observed: dt.datetime, *, intraday=False) -> tuple[dict, dict]:
    validate_jsonld(entry)
    observed = observation_time(entry, observed)
    nodes, identifiers = jsonld_nodes(source_raw, entry["source_format"])
    if "required_page_text" in entry:
        page = source_raw.decode("utf-8-sig")
        if not all(fragment in page for fragment in entry["required_page_text"]):
            raise ValueError("JSON-LD reviewed page selling-basis text changed or is missing")
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
    expiry, expiry_at = valid_period(offer, observed, intraday=intraday)
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
    quote = quote_basis(entry)
    quote.update(price=float(amounts[0]), observed_on=observed.date().isoformat(), available=stock_values[stock],
                 source=entry["source_url"], source_sha256=digest)
    if expiry is not None:
        quote["valid_through"] = expiry
    if expiry_at is not None:
        quote["valid_through_at"] = expiry_at
    receipt = {"schema": "ftx-source-refresh/1", "adapter": "jsonld_v1", "observed_at": observed.isoformat(),
               "manifest_entry": entry, "source_sha256": digest, "product_ref": product.get("@id"),
               "offer_ref": offer.get("@id"), "quantity_basis": "reviewed_manifest_not_inferred",
               "limits": ["No execution, remote context expansion, checkout, FX or textile-physics inference.",
                          "Declared structured data is not independent supplier/stock/terms verification."]}
    stamp_intraday(quote, receipt, observed, intraday)
    return quote, receipt


def refresh(manifest: dict, get, now=None, *, intraday=False) -> tuple[dict, dict[str, bytes]]:
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
        elif entry.get("adapter") == "shopify_public_product_v1":
            validate_public_product(entry)
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
            accept = ("text/csv" if entry["adapter"] == "csv_v1" else "text/html"
                      if entry["source_format"] == "html" else "application/ld+json,application/json")
            source_key = (url, accept)
            if source_key not in sources:
                sources[source_key] = (get(url, accept=accept), clock())
            product, observed = sources[source_key]
            if entry["adapter"] == "csv_v1":
                quote, receipt = extract_csv(entry, product, observed, intraday=intraday)
                extension = ".csv"
            else:
                quote, receipt = extract_jsonld(entry, product, observed, intraday=intraday)
                extension = ".html" if entry["source_format"] == "html" else ".json"
        elif entry.get("adapter") == "shopify_public_product_v1":
            # Bracket the Ajax price/availability with the explicit public currency and stock.
            before = get(entry["public_url"], accept="application/json")
            product = get(entry["product_url"], accept="application/json")
            after = get(entry["public_url"], accept="application/json")
            quote, receipt = extract_public_product(entry, product, before, after, clock(), intraday=intraday)
            for source in (before, after):
                evidence[hashlib.sha256(source).hexdigest() + ".json"] = source
            extension = ".json"
        else:
            before = get(entry["cart_url"], accept="application/json")
            product = get(entry["product_url"], accept="application/json")
            after = get(entry["cart_url"], accept="application/json")
            quote, receipt = extract(entry, product, before, after, clock(), intraday=intraday)
            extension = ".json"
        quotes.append(quote)
        evidence[quote["source_sha256"] + extension] = product
        raw = (json.dumps(receipt, sort_keys=True, indent=2, allow_nan=False) + "\n").encode()
        evidence[hashlib.sha256(raw).hexdigest() + ".receipt.json"] = raw
    quotes.sort(key=lambda q: q["offer_id"])
    return {"schema": INTRADAY_SCHEMA if intraday else SCHEMA, "quotes": quotes}, evidence


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
    parser.add_argument("--intraday", action="store_true",
                        help="emit version-2 quotes with exact UTC retrieval instants and timed source expiry")
    parser.add_argument("--specifications-out", type=Path,
                        help="NEW snapshot of the selected supplier's non-price declarations")
    parser.add_argument("--compare-specifications", type=Path,
                        help="replay a retained specification snapshot and gate quote publication on changes")
    parser.add_argument("--previous-evidence", type=Path,
                        help="original source evidence for comparison (defaults to --evidence)")
    parser.add_argument("--review-out", type=Path,
                        help="NEW detailed revision report; required with --compare-specifications")
    args = parser.parse_args()
    try:
        outputs = [p for p in (args.out, args.specifications_out, args.review_out) if p is not None]
        if len({p.resolve() for p in outputs}) != len(outputs):
            raise ValueError("quote, specification and review outputs must be distinct new paths")
        if any(p.exists() for p in outputs):
            raise ValueError("output exists; use new timestamped snapshot paths")
        if args.compare_specifications and (not args.specifications_out or not args.review_out):
            raise ValueError("comparison requires --specifications-out and --review-out")
        if (args.review_out or args.previous_evidence) and not args.compare_specifications:
            raise ValueError("review outputs and previous evidence require --compare-specifications")
        previous = None
        if args.specifications_out:
            import textile_specifications as specifications
            if args.compare_specifications:
                previous = specifications.replay(load(args.compare_specifications.read_bytes()),
                                                 args.previous_evidence or args.evidence)
        manifest = load(args.manifest.read_bytes())
        book, evidence = refresh(manifest, PublicSession().get, intraday=args.intraday)
        current = specifications.capture(book, evidence) if args.specifications_out else None
        review = specifications.compare(previous, current) if previous is not None else None
        for name, raw in evidence.items():
            target = args.evidence / name
            try:
                create_snapshot(target, raw)
            except FileExistsError:
                if target.read_bytes() != raw:
                    raise ValueError("existing evidence does not match its digest")
        if current is not None:
            create_snapshot(args.specifications_out, specifications.encoded(current))
        if review is not None:
            create_snapshot(args.review_out, specifications.encoded(review))
            if not review["quote_publication_allowed"]:
                parser.exit(2, f"supplier revision needs review: {args.review_out}; no new quotes published\n")
        create_snapshot(args.out, (json.dumps(book, indent=2, allow_nan=False) + "\n").encode())
        print(f"wrote {len(book['quotes'])} exact offers to {args.out}; evidence: {args.evidence}")
        return 0
    except (ValueError, KeyError, TypeError, OverflowError, OSError, urllib.error.URLError) as error:
        parser.exit(1, f"textile refresh refused: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
