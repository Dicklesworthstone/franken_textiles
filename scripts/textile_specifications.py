"""Compare retained supplier declarations without revising a frozen material or measured lot.

The existing price adapters select the exact product/variant. This module retains their
non-price declarations, replays the source evidence, and gates a subsequent quote refresh.
"""
from __future__ import annotations

import csv
import datetime as dt
import hashlib
import io
import json
from pathlib import Path
import re

import refresh_textile_prices as prices

SCHEMA = "ftx-supplier-specifications/1"
LIMITS = [
    "Supplier declarations are not independent measurements or a specimen/lot binding.",
    "Only the selected source representation is compared; images and external references are not interpreted.",
    "An unchanged declaration does not establish unchanged manufactured material or reserve inventory.",
]
# Price/stock changes are admitted by the existing quote validator. Everything else
# in a selected CSV row or variant remains a potential specification/order change.
SHOPIFY_MARKET = {"price", "unit_price", "compare_at_price", "available", "inventory_quantity"}
PRODUCT_MEDIA = {"images", "image", "featured_image", "media", "created_at", "updated_at",
                 "published_at", "aggregateRating", "review", "interactionStatistic"}
OFFER_MARKET = {"price", "priceCurrency", "availability", "priceValidUntil", "validFrom",
                "validThrough", "availabilityStarts", "availabilityEnds"}


def encoded(value) -> bytes:
    return (json.dumps(value, sort_keys=True, indent=2, allow_nan=False) + "\n").encode()


def digest(value: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
        raise ValueError("evidence needs an exact lowercase SHA-256")
    return value


def _extension(entry):
    if entry.get("adapter") == "csv_v1":
        return ".csv"
    return ".html" if entry.get("source_format") == "html" else ".json"


def _facts(entry, source, receipt):
    """Preserve original quantities/text; do not infer composition, width, or units."""
    observed = dt.datetime.fromisoformat(receipt["observed_at"])
    adapter = receipt["adapter"]
    if adapter == "csv_v1":
        _, checked = prices.extract_csv(entry, source, observed)
        if checked["selected_record"] != receipt["selected_record"]:
            raise ValueError("CSV source no longer reproduces its recorded selection")
        rows = list(csv.reader(io.StringIO(source.decode("utf-8-sig"), newline=""),
                               delimiter=entry["delimiter"], strict=True))
        row = dict(zip(rows[0], rows[checked["selected_record"] - 1]))
        omitted = {entry["columns"][k] for k in ("price", "available", "available_quantity")
                   if k in entry["columns"]}
        selected = {"row": {k: v for k, v in row.items() if k not in omitted}}
    elif adapter == "jsonld_v1":
        prices.extract_jsonld(entry, source, observed)
        nodes, identifiers = prices.jsonld_nodes(source, entry["source_format"])
        product = next(n for n in nodes if prices.schema_type(n, "Product")
                       and n.get("sku") == entry["sku"])

        work = [0, 0]

        def expand(value, seen=(), depth=0):
            if depth > 32:
                raise ValueError("specification references exceed depth bounds")
            # A small acyclic reference graph can expand exponentially. Charge every
            # traversal and copied scalar/key before building the expanded result.
            work[0] += 1
            if isinstance(value, dict):
                work[1] += 2 + sum(len(encoded(k)) + 2 for k in value)
            elif isinstance(value, list):
                work[1] += 2 + len(value)
            else:
                work[1] += len(encoded(value))
            if work[0] > 16384 or work[1] > prices.LIMIT:
                raise ValueError("specification reference expansion exceeds work/size budget")
            if isinstance(value, list):
                return [expand(v, seen, depth + 1) for v in value]
            if isinstance(value, dict):
                if set(value) == {"@id"} and value["@id"] in identifiers:
                    key = value["@id"]
                    if key in seen:
                        return {"@id": key}
                    return expand(identifiers[key], (*seen, key), depth + 1)
                return {k: expand(v, seen, depth + 1) for k, v in value.items()
                        if k not in PRODUCT_MEDIA | {"offers", "hasVariant"}}
            return value

        offered = product["offers"]
        offers = offered if isinstance(offered, list) else [offered]
        offers = [identifiers[o["@id"]] if set(o) == {"@id"} else o for o in offers]
        offer = next(o for o in offers if entry["offer_ref"] is None
                     or o.get("@id") == entry["offer_ref"])
        offer = {k: expand(v) for k, v in offer.items() if k not in OFFER_MARKET}
        if "priceSpecification" in offer:
            offer["priceSpecification"] = {k: v for k, v in offer["priceSpecification"].items()
                                           if k not in {"price", "priceCurrency"}}
        selected = {"product": expand(product), "offer": offer}
    elif adapter == "shopify_ajax_product_v1":
        prices.validate_entry(entry)
        prices.observation_time(entry, observed)
        # The retained receipt attests currency without retaining anonymous cart tokens.
        if any(receipt[k] != entry["currency"] for k in ("currency_before", "currency_after")):
            raise ValueError("receipt currency differs from its reviewed material mapping")
        product = prices.load(source)
        variants = [v for v in product["variants"] if type(v.get("id")) is int
                    and v["id"] == entry["variant_id"]]
        if len(variants) != 1 or variants[0].get("sku") != entry["sku"]:
            raise ValueError("retained Shopify variant does not reproduce its exact SKU")
        selected = {
            "product": {k: v for k, v in product.items()
                        if k not in PRODUCT_MEDIA | SHOPIFY_MARKET | {"variants", "price_min",
                            "price_max", "price_varies", "compare_at_price_min",
                            "compare_at_price_max", "compare_at_price_varies"}},
            "variant": {k: v for k, v in variants[0].items()
                        if k not in SHOPIFY_MARKET | PRODUCT_MEDIA},
        }
    else:
        raise ValueError("unknown retained source adapter")
    return {"adapter": adapter, "source": entry.get("source_url", entry.get("product_url")),
            "binding": {k: entry[k] for k in prices.FIELDS + ("material_sha256",) if k in entry},
            "selected": selected}


def _record(receipt_raw, source):
    receipt = prices.load(receipt_raw)
    if receipt.get("schema") != "ftx-source-refresh/1":
        raise ValueError("unsupported source receipt")
    source_hash = digest(receipt.get("source_sha256", receipt.get("product_sha256")))
    if hashlib.sha256(source).hexdigest() != source_hash:
        raise ValueError("retained source bytes do not match their receipt")
    if len(source) > prices.LIMIT:
        raise ValueError("retained source exceeds 4 MiB")
    entry = receipt["manifest_entry"]
    return {"offer_id": entry["offer_id"], "observed_at": receipt["observed_at"],
            "source_sha256": source_hash, "receipt_sha256": hashlib.sha256(receipt_raw).hexdigest(),
            "facts": _facts(entry, source, receipt)}


def capture(book, evidence):
    quotes = {q["offer_id"]: q for q in book["quotes"]}
    if len(quotes) != len(book["quotes"]):
        raise ValueError("duplicate quote identity")
    observations = {}
    for name, receipt_raw in evidence.items():
        if not name.endswith(".receipt.json"):
            continue
        if name != hashlib.sha256(receipt_raw).hexdigest() + ".receipt.json":
            raise ValueError("receipt bytes do not match their evidence name")
        receipt = prices.load(receipt_raw)
        source_hash = digest(receipt.get("source_sha256", receipt.get("product_sha256")))
        entry = receipt["manifest_entry"]
        record = _record(receipt_raw, evidence[source_hash + _extension(entry)])
        offer = record["offer_id"]
        if offer in observations or offer not in quotes or quotes[offer]["source_sha256"] != source_hash:
            raise ValueError("source receipt does not uniquely reproduce the quote book")
        observations[offer] = record
    if set(observations) != set(quotes) or not 1 <= len(observations) <= 100:
        raise ValueError("every exact offer needs one retained source receipt")
    result = {"schema": SCHEMA, "observations": [observations[k] for k in sorted(observations)],
              "limits": LIMITS}
    if len(encoded(result)) > prices.LIMIT:
        raise ValueError("specification snapshot exceeds 4 MiB")
    return result


def replay(document, evidence_dir: Path):
    """Recompute a baseline from its original source bytes, never trust edited facts."""
    if set(document) != {"schema", "observations", "limits"} or document["schema"] != SCHEMA:
        raise ValueError("unsupported supplier specification snapshot")
    observations = document["observations"]
    if not isinstance(observations, list) or not 1 <= len(observations) <= 100 or document["limits"] != LIMITS:
        raise ValueError("invalid supplier specification snapshot")
    ids = set()
    for record in observations:
        receipt_path = evidence_dir / (digest(record["receipt_sha256"]) + ".receipt.json")
        if receipt_path.stat().st_size > prices.LIMIT:
            raise ValueError("source receipt exceeds 4 MiB")
        receipt_raw = receipt_path.read_bytes()
        if hashlib.sha256(receipt_raw).hexdigest() != record["receipt_sha256"]:
            raise ValueError("retained receipt does not match its digest")
        receipt = prices.load(receipt_raw)
        source_path = evidence_dir / (digest(record["source_sha256"]) + _extension(receipt["manifest_entry"]))
        if source_path.stat().st_size > prices.LIMIT:
            raise ValueError("retained source exceeds 4 MiB")
        rebuilt = _record(receipt_raw, source_path.read_bytes())
        if encoded(rebuilt) != encoded(record) or record["offer_id"] in ids:
            raise ValueError("specification baseline differs from its original source evidence")
        ids.add(record["offer_id"])
    return document


def _changes(before, after, path="", depth=0):
    if depth > 32:
        raise ValueError("specification comparison exceeds depth bounds")
    if isinstance(before, dict) and isinstance(after, dict):
        result = []
        for key in sorted(before.keys() | after.keys()):
            location = path + "/" + key.replace("~", "~0").replace("/", "~1")
            if key in before and key in after:
                result.extend(_changes(before[key], after[key], location, depth + 1))
            else:
                result.append({"path": location, "before_present": key in before,
                               "after_present": key in after, "before": before.get(key), "after": after.get(key)})
            if len(result) > 16384:
                raise ValueError("specification comparison exceeds change bounds")
        return result
    if encoded(before) == encoded(after):
        return []
    return [{"path": path, "before_present": True, "after_present": True,
             "before": before, "after": after}]


def compare(previous, current):
    old = {r["offer_id"]: r for r in previous["observations"]}
    new = {r["offer_id"]: r for r in current["observations"]}
    reviews = []
    for offer in sorted(old.keys() | new.keys()):
        a, b = old.get(offer), new.get(offer)
        if a and b and dt.datetime.fromisoformat(b["observed_at"]) < dt.datetime.fromisoformat(a["observed_at"]):
            raise ValueError("new supplier observation predates the retained revision")
        changes = _changes(a["facts"] if a else {}, b["facts"] if b else {})
        status = "added" if a is None else "removed" if b is None else "changed" if changes else "unchanged"
        bound = bool(a and b and a["facts"]["binding"].get("material_sha256")
                     and a["facts"]["binding"].get("material_sha256") == b["facts"]["binding"].get("material_sha256"))
        def origin_record(r):
            return None if r is None else {k: r[k] for k in ("observed_at", "source_sha256", "receipt_sha256")}
        reviews.append({"offer_id": offer, "status": status, "previous": origin_record(a),
                        "current": origin_record(b), "changes": changes,
                        "same_material_revision": bound, "measured_specimen_binding": "unresolved",
                        "review_required": status != "unchanged" or not bound})
    return {"schema": "ftx-supplier-revision-review/1", "offers": reviews,
            "quote_publication_allowed": all(not r["review_required"] for r in reviews),
            "previous_snapshot_sha256": hashlib.sha256(encoded(previous)).hexdigest(),
            "current_snapshot_sha256": hashlib.sha256(encoded(current)).hexdigest(), "limits": LIMITS}
