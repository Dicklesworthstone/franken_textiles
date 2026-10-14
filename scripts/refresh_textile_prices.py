#!/usr/bin/env python3
"""Read public Shopify offers into the existing ftx-textile-quotes/1 workbench format.

No shopping/cart mutation, credentials, browser automation, guessed units or material-law inference.
Each successful run creates a NEW immutable quote snapshot and source-byte evidence files.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import http.cookiejar
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


def load(raw: bytes) -> dict:
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
    value = json.loads(raw, object_pairs_hook=pairs, parse_constant=nonfinite)
    if not isinstance(value, dict):
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
        req = urllib.request.Request(url, headers={"User-Agent": AGENT, "Accept": "application/json,text/plain"})
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


def validate_entry(entry: dict) -> None:
    required = set(FIELDS) | {"product_url", "cart_url", "variant_id", "basis_source",
                              "basis_note", "basis_reviewed_on", "required_description_text"}
    if set(entry) != required:
        raise ValueError(f"manifest entry fields differ: {sorted(set(entry) ^ required)}")
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
    dt.date.fromisoformat(entry["basis_reviewed_on"])
    fragments = entry["required_description_text"]
    if not isinstance(fragments, list) or not fragments or any(not isinstance(s, str) or not s.strip() for s in fragments):
        raise ValueError("at least one reviewed description fragment is required to detect selling-basis drift")


def extract(entry: dict, product_raw: bytes, cart_before_raw: bytes, cart_after_raw: bytes,
            observed: dt.datetime) -> tuple[dict, dict]:
    """Pure adapter. Presence of an offer is not permission to substitute a different material/SKU."""
    validate_entry(entry)
    if observed.tzinfo is None:
        raise ValueError("observation timestamp must have a timezone")
    observed = observed.astimezone(dt.timezone.utc)
    age = (observed.date() - dt.date.fromisoformat(entry["basis_reviewed_on"])).days
    if not 0 <= age <= 30:
        raise ValueError("selling-basis review is future-dated or older than 30 days")
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


def refresh(manifest: dict, get, now=None) -> tuple[dict, dict[str, bytes]]:
    if set(manifest) != {"schema", "offers"} or manifest["schema"] != "ftx-shopify-refresh/1":
        raise ValueError("unsupported refresh manifest")
    entries = manifest["offers"]
    if not isinstance(entries, list) or not entries or len(entries) > 100:
        raise ValueError("manifest needs 1..100 explicitly reviewed offers")
    ids = set()
    for entry in entries:
        validate_entry(entry)
        if entry["offer_id"] in ids:
            raise ValueError("duplicate offer identity in refresh manifest")
        ids.add(entry["offer_id"])
    clock = now or (lambda: dt.datetime.now(dt.timezone.utc))
    quotes, evidence = [], {}
    for entry in entries:
        before = get(entry["cart_url"])
        product = get(entry["product_url"])
        after = get(entry["cart_url"])
        quote, receipt = extract(entry, product, before, after, clock())
        quotes.append(quote)
        evidence[quote["source_sha256"] + ".json"] = product
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
