"""Synthetic schema.org supplier responses, not live textile measurements or market quotes."""
import copy
import datetime as dt
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
    def test_continuous_limit_retains_reviewed_source_and_refuses_description_drift(self):
        e, p = manifest_entry(), product()
        fragment = "Maximum continuous cut: 5 yards."
        e.update(maximum_continuous_quantity=5, continuous_cut_text=fragment)
        e["required_description_text"].append(fragment)
        p["description"] += fragment
        q, receipt = r.extract_jsonld(e, html(p), NOW)
        self.assertEqual(q["maximum_continuous_quantity"], 5)
        self.assertEqual(receipt["manifest_entry"]["continuous_cut_text"], fragment)
        self.assertNotIn("available_quantity", q)
        p["description"] = p["description"].replace("5 yards", "3 yards")
        with self.assertRaises(ValueError):
            r.extract_jsonld(e, html(p), NOW)

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

    def test_reviewed_material_snapshot_lock_survives_without_inventing_capacity(self):
        e = manifest_entry(); e["material_sha256"] = "c3" * 32
        e["required_page_text"] = ["Sold per half yard"]
        q, receipt = self.extract(e=e)
        self.assertEqual(q["material_sha256"], e["material_sha256"])
        self.assertEqual(receipt["manifest_entry"]["material_sha256"], e["material_sha256"])
        self.assertEqual(receipt["manifest_entry"]["required_page_text"], e["required_page_text"])
        self.assertNotIn("available_quantity", q)
        e["material_sha256"] = "not-a-hash"
        with self.assertRaises(ValueError): self.extract(e=e)

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

    def test_intraday_expiry_preserves_earliest_instant_and_separate_calendar_limit(self):
        p = product(); p["offers"].update(priceValidUntil="2026-10-14",
            validThrough="2026-10-13T20:30:00+08:00", availabilityEnds="2026-10-13T08:10:00.123456-04:00")
        q, receipt = r.extract_jsonld(manifest_entry(), html(p), NOW, intraday=True)
        self.assertEqual(q["valid_through"], "2026-10-14")
        self.assertEqual(q["valid_through_at"], "2026-10-13T12:10:00.123456Z")
        self.assertEqual(q["observed_at"], "2026-10-13T12:00:00.000000Z")
        self.assertEqual(receipt["quote_schema"], r.INTRADAY_SCHEMA)
        end = dt.datetime(2026, 10, 13, 12, 10, 0, 123456, tzinfo=dt.timezone.utc)
        self.assertEqual(r.extract_jsonld(manifest_entry(), html(p), end, intraday=True)[0]["valid_through_at"], q["valid_through_at"])
        with self.assertRaises(ValueError):
            r.extract_jsonld(manifest_entry(), html(p), end + dt.timedelta(microseconds=1), intraday=True)

    def test_intraday_source_time_refuses_unknown_offsets_and_precision_loss(self):
        for value in ("2026-10-13T13:00:00", "2026-10-13T13:00:00-00:00",
                      "2026-10-13T13:00:00.1234567Z", "2026-10-13T13:00:60Z",
                      "2026-10-13T24:00:00Z", "2026-10-13T13:00:00+25:00",
                      "2026-10-13T24:00:00.000000+00:00", "2026-10-13T24:00:00-04:00",
                      "2026-10-13T13:60:00Z", "2026-10-13T13:00:61Z",
                      "2026-10-13T13:00:00+24:00", "2026-10-13T13:00:00+01:60",
                      "2026-10-13T13:00:00+00:99", "2026-10-13T13:00:00-01:60",
                      "2026-10-13 13:00:00Z", "2026-10-13T13:00Z"):
            p = product(); p["offers"]["validThrough"] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                r.extract_jsonld(manifest_entry(), html(p), NOW, intraday=True)

    def test_stock_is_explicit_not_inferred_from_the_page(self):
        for stock, expected in [("InStock", True), ("LimitedAvailability", True), ("OutOfStock", False),
                                ("SoldOut", False), ("Discontinued", False)]:
            p = product(); p["offers"]["availability"] = "http://schema.org/" + stock
            self.assertIs(self.extract(p)[0]["available"], expected)
        for stock in (None, "InStock", "https://schema.org/BackOrder", "https://schema.org/PreOrder", True):
            p = product(); p["offers"]["availability"] = stock
            with self.subTest(stock=stock), self.assertRaises(ValueError): self.extract(p)

    def test_unit_qualified_offer_inventory_reaches_the_existing_quote_capacity(self):
        for code, unit, label in (("YRD", "linear_yard", "yards"),
                                  ("MTR", "linear_meter", "metres"),
                                  ("MTK", "square_meter", "m²"),
                                  ("KGM", "kilogram", "kg")):
            e, p = manifest_entry(), product()
            e.update(unit=unit, material_sha256="ab" * 32,
                     required_description_text=[f"Sold per half {label}."],
                     basis_note="Synthetic reviewed sale-unit example; not a real offer")
            p["description"] = e["required_description_text"][0]
            p["offers"]["inventoryLevel"] = {
                "@type": "QuantitativeValue", "value": "1.25", "unitCode": code, "unitText": label}
            with self.subTest(unit=unit):
                quote, receipt = self.extract(p, e)
                self.assertEqual(quote["available_quantity"], 1.25)
                self.assertEqual((quote["quantity"], quote["minimum_quantity"], quote["order_increment"]),
                                 (0.5, 1.0, 0.5))
                self.assertEqual(quote["material_sha256"], e["material_sha256"])
                self.assertEqual(quote["source_sha256"], hashlib.sha256(html(p)).hexdigest())
                self.assertEqual(receipt["offer_ref"], "#retail")

    def test_inventory_zero_unknown_and_unavailable_are_separate_observations(self):
        p = product()
        self.assertNotIn("available_quantity", self.extract(p)[0])
        p["offers"]["inventoryLevel"] = {"@type": "QuantitativeValue", "value": 0, "unitCode": "YRD"}
        quote = self.extract(p)[0]
        self.assertEqual(quote["available_quantity"], 0.0)
        self.assertTrue(quote["available"])  # Preserve the source flag; zero capacity cannot fill an order.
        p["offers"]["availability"] = "https://schema.org/OutOfStock"
        p["offers"]["inventoryLevel"]["value"] = 10
        quote = self.extract(p)[0]
        self.assertEqual(quote["available_quantity"], 10.0)
        self.assertFalse(quote["available"])  # A positive count never revives an unavailable offer.
        del p["offers"]["inventoryLevel"]
        quote = self.extract(p)[0]
        self.assertNotIn("available_quantity", quote)
        self.assertFalse(quote["available"])

    def test_inventory_uses_only_the_selected_offer_and_local_value_reference(self):
        p, e = product(), manifest_entry()
        e["offer_ref"] = "#retail"
        level = {"@id": "#stock", "@type": "QuantitativeValue", "value": 1.25, "unitCode": "YRD"}
        selected = p["offers"]
        selected["inventoryLevel"] = {"@id": "#stock"}
        p["offers"] = [dict(selected, **{"@id": "#other", "inventoryLevel": {
            "@type": "QuantitativeValue", "value": 999, "unitCode": "C62"}}), selected]
        p["inventoryLevel"] = {"@type": "QuantitativeValue", "value": 500, "unitCode": "YRD"}
        document = {"@context": "https://schema.org", "@graph": [p, level]}
        quote = self.extract(document, e)[0]
        self.assertEqual(quote["available_quantity"], 1.25)
        del selected["inventoryLevel"]
        self.assertNotIn("available_quantity", self.extract(document, e)[0])
        selected["inventoryLevel"] = {"@id": "https://other.invalid/stock"}
        with self.assertRaisesRegex(ValueError, "missing embedded"):
            self.extract(document, e)

    def test_inventory_ranges_counts_missing_units_and_qualifiers_refuse(self):
        point = {"@type": "QuantitativeValue", "value": 1.25, "unitCode": "YRD"}
        bad = [None, 1.25, "1.25 yards", [point], {},
               {"@type": "QuantitativeValue", "value": 1.25},
               {"@type": "QuantitativeValue", "value": 1.25, "unitText": "yards"}]
        for change in ({"minValue": 1}, {"maxValue": 2}, {"valueReference": "per roll"},
                       {"additionalProperty": {"name": "packs"}}, {"@type": "PropertyValue"},
                       {"@type": ["QuantitativeValue", "PropertyValue"]}, {"unitCode": "C62"},
                       {"unitCode": "MTR"}, {"unitCode": "MTK"}, {"unitCode": None},
                       {"unitCode": ["YRD"]}, {"unitText": "half yards"}, {"unitText": "rolls"},
                       {"unitText": "metres"}, {"unitText": None}, {"value": True}, {"value": -1},
                       {"value": None}, {"value": [1.25]}, {"value": {"@value": 1.25}},
                       {"value": "1,250"}, {"value": "about 1.25"}, {"value": "NaN"},
                       {"value": "1e309"}, {"value": float("inf")}):
            bad.append(dict(point, **change))
        for level in bad:
            p = product(); p["offers"]["inventoryLevel"] = level
            with self.subTest(level=level), self.assertRaises(ValueError):
                self.extract(p)

    def test_inventory_cannot_override_expiry_or_wrong_product_identity(self):
        for change in ({"priceValidUntil": "2026-10-12"}, {"validFrom": "2026-10-14"},
                       {"sku": "ANOTHER-LOT"}, {"availability": "https://schema.org/BackOrder"}):
            p = product()
            p["offers"].update(inventoryLevel={"@type": "QuantitativeValue", "value": 500,
                                              "unitCode": "YRD"}, **change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.extract(p)

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

    def test_reviewed_page_basis_outside_description_is_retained_and_checked(self):
        e = manifest_entry(); e["unit"] = "linear_meter"
        e["required_description_text"] = ["Content: 100% Linen"]
        e["required_page_text"] = ["All fabric is sold by the half meter.", "Quantité en mètres"]
        p = product(); p["description"] = "Content: 100% Linen. Synthetic fixture only."
        faq = "<section>All fabric is sold by the half meter. Quantité en mètres</section>".encode()
        data = html(p).replace(b"</html>", faq + b"</html>")
        quote, receipt = r.extract_jsonld(e, data, NOW)
        self.assertEqual((quote["quantity"], quote["unit"]), (0.5, "linear_meter"))
        self.assertEqual(quote["source_sha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(receipt["manifest_entry"]["required_page_text"], e["required_page_text"])
        for changed in (html(p), data.replace(b"half meter", b"full meter"),
                        data.replace("Quantité en mètres".encode(), b"Other text")):
            with self.subTest(changed=changed[-100:]), self.assertRaisesRegex(ValueError, "page selling-basis text"):
                r.extract_jsonld(e, changed, NOW)
        p["description"] = "Content: 100% Polyester"
        with self.assertRaisesRegex(ValueError, "product-description|selling-basis description"):
            r.extract_jsonld(e, html(p).replace(b"</html>", faq + b"</html>"), NOW)

    def test_page_guards_are_optional_but_nonempty_html_only_and_exact(self):
        for fragments in (None, "Sold per half yard", [], [""], ["   "], [1], ["valid", False]):
            e = manifest_entry(); e["required_page_text"] = fragments
            with self.subTest(fragments=fragments), self.assertRaisesRegex(ValueError, "required_page_text"):
                r.validate_jsonld(e)
        e = manifest_entry(); e.update(source_format="json", required_page_text=["Sold per half yard"])
        with self.assertRaisesRegex(ValueError, "required_page_text"):
            r.extract_jsonld(e, json.dumps(product()).encode(), NOW)
        e["source_format"] = "html"; e["required_page_text"] = ["Sold  per half yard"]
        with self.assertRaisesRegex(ValueError, "page selling-basis text"):
            r.extract_jsonld(e, html(product()), NOW)
        del e["required_page_text"]
        self.assertEqual(r.extract_jsonld(e, html(product()), NOW)[0]["price"], 12.99)

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
            def fixed_clock(m, get, **kwargs): return real_refresh(m, get, lambda: NOW, **kwargs)
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
