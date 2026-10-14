"""Synthetic schema.org supplier responses, not live textile measurements or market quotes."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import refresh_textile_prices as r
from test_refresh_textile_prices import NOW, entry


def manifest_entry():
    e = entry()
    for key in ("product_url", "cart_url", "variant_id"):
        del e[key]
    e.update(adapter="jsonld_v1", source_url="https://example.invalid/products/sheet",
             source_format="html", offer_ref=None, tax_included=None)
    return e


def product():
    return {"@context": "https://schema.org", "@type": "Product", "@id": "#sheet", "sku": "TEST-42",
            "description": "Sold per half yard. Synthetic fixture only.",
            "offers": {"@type": "Offer", "@id": "#retail", "price": "12.99", "priceCurrency": "USD",
                       "availability": "https://schema.org/InStock"}}


def html(value):
    return ("<html><script>throw Error('must not run');</script><script type='application/ld+json'>"
            + json.dumps(value) + "</script></html>").encode()


class JsonLdTests(unittest.TestCase):
    def extract(self, p=None, e=None):
        return r.extract_jsonld(e or manifest_entry(), html(p or product()), NOW)

    def test_exact_product_and_offer_preserve_source_bytes_and_major_currency_units(self):
        data = html(product())
        q, receipt = r.extract_jsonld(manifest_entry(), data, NOW)
        self.assertEqual(q["price"], 12.99)
        self.assertEqual(q["quantity"], 0.5)
        self.assertEqual(q["source_sha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(receipt["product_ref"], "#sheet")
        self.assertEqual(receipt["offer_ref"], "#retail")
        self.assertEqual(set(q), set(r.FIELDS) | {"price", "observed_on", "available", "source", "source_sha256"})

    def test_local_graph_references_and_product_group_variants(self):
        p = product(); offer = p.pop("offers"); p.pop("@context")
        p["offers"] = {"@id": "#retail"}; offer["itemOffered"] = {"@id": "#sheet"}
        graph = {"@context": {"@vocab": "https://schema.org/"}, "@graph": [
            {"@type": "ProductGroup", "hasVariant": [p]}, offer]}
        e = manifest_entry(); e["source_format"] = "json"
        q, _ = r.extract_jsonld(e, json.dumps(graph).encode(), NOW)
        self.assertEqual(q["price"], 12.99)

    def test_explicit_offer_ref_not_cheapest_or_first_offer(self):
        p = product(); other = dict(p["offers"], **{"@id": "#other", "price": "0.01"})
        p["offers"] = [other, p["offers"]]
        with self.assertRaises(ValueError): self.extract(p)
        e = manifest_entry(); e["offer_ref"] = "#retail"
        self.assertEqual(self.extract(p, e)[0]["price"], 12.99)
        e["offer_ref"] = "#missing"
        with self.assertRaises(ValueError): self.extract(p, e)

    def test_aggregate_duplicate_missing_or_rebound_identity_refuses(self):
        for mutation in ("aggregate", "duplicate", "missing", "sku", "item"):
            p = product()
            if mutation == "aggregate": p["offers"]["@type"] = "AggregateOffer"
            elif mutation == "duplicate": p = [p, copy.deepcopy(p)]
            elif mutation == "missing": p["sku"] = "OTHER"
            elif mutation == "sku": p["offers"]["sku"] = "SWATCH"
            else: p["offers"]["itemOffered"] = {"@type": "Product", "sku": "OTHER"}
            with self.subTest(mutation=mutation), self.assertRaises(ValueError): self.extract(p)

    def test_unambiguous_plain_price_specification_and_tax_attestation(self):
        p = product(); offer = p["offers"]
        offer["priceSpecification"] = {"@type": "PriceSpecification", "price": 12.99,
                                       "priceCurrency": "USD", "valueAddedTaxIncluded": False}
        e = manifest_entry(); e["tax_included"] = False
        self.assertEqual(self.extract(p, e)[0]["price"], 12.99)
        del offer["price"]; del offer["priceCurrency"]
        self.assertEqual(self.extract(p, e)[0]["price"], 12.99)
        e["tax_included"] = True
        with self.assertRaises(ValueError): self.extract(p, e)
        e["tax_included"] = None
        with self.assertRaises(ValueError): self.extract(p, e)

    def test_currency_conflicts_conditional_specs_and_disappeared_tax_refuse(self):
        for change in ({"price": "13.99"}, {"priceCurrency": "CAD"}, {"@type": "UnitPriceSpecification"},
                       {"eligibleQuantity": {"minValue": 5}}, {"valueAddedTaxIncluded": "false"}):
            p = product(); p["offers"]["priceSpecification"] = dict(
                {"@type": "PriceSpecification", "price": "12.99", "priceCurrency": "USD"}, **change)
            with self.subTest(change=change), self.assertRaises(ValueError): self.extract(p)
        e = manifest_entry(); e["tax_included"] = False
        with self.assertRaises(ValueError): self.extract(e=e)

    def test_source_expiry_survives_into_quote_with_earliest_boundary(self):
        p = product(); p["offers"].update(priceValidUntil="2026-10-20", validThrough="2026-10-16",
                                         availabilityEnds="2026-10-18", validFrom="2026-10-12")
        self.assertEqual(self.extract(p)[0]["valid_through"], "2026-10-16")
        p["offers"]["validThrough"] = "2026-10-13"
        self.assertEqual(self.extract(p)[0]["valid_through"], "2026-10-13")

    def test_expired_future_naive_and_intraday_bounded_offers_refuse(self):
        for change in ({"priceValidUntil": "2026-10-12"}, {"validFrom": "2026-10-14"},
                       {"validFrom": "2026-10-13T13:00:00+00:00"}, {"validFrom": "2026-10-12T12:00:00"},
                       {"validThrough": "2026-10-14T12:00:00Z"}, {"priceValidUntil": "bad"}):
            p = product(); p["offers"].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError): self.extract(p)
        p = product(); p["offers"]["validFrom"] = "2026-10-13T07:59:59-04:00"
        self.assertEqual(self.extract(p)[0]["price"], 12.99)

    def test_stock_is_explicit_not_inferred_from_the_page(self):
        for stock, expected in [("InStock", True), ("LimitedAvailability", True), ("OutOfStock", False),
                                ("SoldOut", False), ("Discontinued", False)]:
            p = product(); p["offers"]["availability"] = "http://schema.org/" + stock
            self.assertIs(self.extract(p)[0]["available"], expected)
        for stock in (None, "InStock", "https://schema.org/BackOrder", "https://schema.org/PreOrder", True):
            p = product(); p["offers"]["availability"] = stock
            with self.subTest(stock=stock), self.assertRaises(ValueError): self.extract(p)

    def test_sale_basis_and_conditional_markets_are_not_silently_dropped(self):
        for key in ("eligibleCustomerType", "eligibleQuantity", "validForMemberTier", "addOn", "includesObject"):
            p = product(); p["offers"][key] = {}
            with self.subTest(key=key), self.assertRaises(ValueError): self.extract(p)
        for change in ({"eligibleRegion": "CA"}, {"ineligibleRegion": ["US"]},
                       {"eligibleRegion": {"@type": "GeoShape"}}, {"businessFunction": "LeaseOut"}):
            p = product(); p["offers"].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError): self.extract(p)
        p = product(); p["offers"].update(eligibleRegion=["US", "CA"], ineligibleRegion="GB")
        self.assertEqual(self.extract(p)[0]["price"], 12.99)
        p["description"] = "Sold per whole roll now"
        with self.assertRaises(ValueError): self.extract(p)

    def test_no_remote_fetch_execution_or_context_remapping(self):
        e = manifest_entry()
        for data in [b'<script type="application/ld+json" src="https://other.invalid/x"></script>',
                     b'<script type="application/ld+json" type="text/javascript">{}</script>',
                     b'<script>alert("not data")</script>']:
            with self.assertRaises(ValueError): r.extract_jsonld(e, data, NOW)
        for context in ({"@vocab": "https://schema.org/", "price": "evil"}, "https://other.invalid/context"):
            p = product(); p["@context"] = context
            with self.assertRaises(ValueError): self.extract(p)
        p = product(); p["offers"] = {"@id": "https://other.invalid/offer"}
        with self.assertRaises(ValueError): self.extract(p)

    def test_malformed_or_oversized_documents_refuse(self):
        for data in [b'<script type="application/ld+json">{}',
                     b'<script type="application/ld+json">{"price":1,"price":2}</script>',
                     b'<script type="application/ld+json">{"price":NaN}</script>', b"\xff", b"x" * (r.LIMIT+1)]:
            with self.assertRaises(ValueError): r.extract_jsonld(manifest_entry(), data, NOW)
        p = product(); p["irrelevant"] = [{}] * 16385
        with self.assertRaises(ValueError): self.extract(p)
        p = product(); p["irrelevant"] = json.loads("[" * 35 + "0" + "]" * 35)
        with self.assertRaises(ValueError): self.extract(p)

    def test_document_array_and_full_schema_type_uris(self):
        p = product(); p["@type"] = ["https://schema.org/Product"]
        p["offers"]["@type"] = "http://schema.org/Offer"
        other = product(); other["sku"] = "OTHER"; other["@id"] = "#other"
        other["offers"]["@id"] = "#other-offer"
        self.assertEqual(self.extract([other, p])[0]["price"], 12.99)

    def test_cli_publishes_quote_and_full_html_evidence_without_overwrite(self):
        e = manifest_entry(); p = product(); p["offers"]["priceValidUntil"] = "2026-10-14"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); manifest = root / "manifest.json"; out = root / "quotes.json"
            manifest.write_text(json.dumps(dict(schema="ftx-textile-refresh/1", offers=[e])))
            argv = ["refresh", str(manifest), "--out", str(out), "--evidence", str(root / "evidence")]
            real_refresh = r.refresh
            def fixed_clock(m, get): return real_refresh(m, get, lambda: NOW)
            with patch("sys.argv", argv), patch.object(r.PublicSession, "get", return_value=html(p)) as get, \
                    patch.object(r, "refresh", side_effect=fixed_clock):
                self.assertEqual(r.main(), 0)
                self.assertEqual(get.call_count, 1)
                saved = out.read_bytes()
                quote = json.loads(saved)["quotes"][0]
                self.assertEqual(quote["valid_through"], "2026-10-14")
                self.assertEqual((root / "evidence" / (quote["source_sha256"] + ".html")).read_bytes(), html(p))
                with self.assertRaises(SystemExit): r.main()
                self.assertEqual(out.read_bytes(), saved)
                self.assertEqual(get.call_count, 1)


if __name__ == "__main__":
    unittest.main()
