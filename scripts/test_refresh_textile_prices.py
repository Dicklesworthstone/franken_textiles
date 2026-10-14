"""Offline primary-response fixtures; synthetic prices are NEVER advertised as live observations."""
import copy
import datetime as dt
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import refresh_textile_prices as r

NOW = dt.datetime(2026, 10, 13, 12, tzinfo=dt.timezone.utc)


def entry():
    return dict(offer_id="synthetic/42", material_id="test/sheet", supplier="synthetic",
                sku="TEST-42", quantity=0.5, unit="linear_yard", minimum_quantity=1.0,
                order_increment=0.5, currency="USD", region="US", quality="first-quality",
                terms="merchandise-ex-tax-shipping", product_url="https://example.invalid/products/sheet.js",
                cart_url="https://example.invalid/cart.js", variant_id=42,
                basis_source="https://example.invalid/products/sheet",
                basis_note="Synthetic fixture, not a supplier observation",
                basis_reviewed_on="2026-10-13", required_description_text=["Sold per half yard"])


def product():
    return dict(description="<p>Sold per half yard.</p>", price_min=1,
                variants=[dict(id=41, sku="OTHER", price=100, available=True),
                          dict(id=42, sku="TEST-42", price=1299, compare_at_price=9999, available=True)])


def raw(value):
    return json.dumps(value).encode()


CART = raw(dict(currency="USD", items=[], item_count=0, token="anonymous-do-not-persist"))


class RefreshTests(unittest.TestCase):
    def extract(self, e=None, p=None, before=CART, after=CART, now=NOW):
        return r.extract(e or entry(), raw(p or product()), before, after, now)

    def test_exact_variant_currency_units_and_source_bytes(self):
        q, receipt = self.extract()
        self.assertEqual(q["price"], 12.99)
        self.assertEqual(q["quantity"], 0.5)
        self.assertEqual(q["order_increment"], 0.5)
        self.assertEqual(q["source_sha256"], hashlib.sha256(raw(product())).hexdigest())
        self.assertEqual(q["observed_on"], "2026-10-13")
        self.assertNotIn("token", json.dumps(receipt))
        self.assertEqual(set(q), set(r.FIELDS) | {"price", "observed_on", "available", "source", "source_sha256"})

    def test_price_is_not_product_minimum_or_compare_at(self):
        p = product(); p["price_min"] = 1; p["variants"][1]["compare_at_price"] = 50000
        self.assertEqual(self.extract(p=p)[0]["price"], 12.99)

    def test_currency_changed_or_missing_refuses_without_fx(self):
        for c in ({"currency":"EUR", "items":[], "item_count":0}, {"items":[], "item_count":0}):
            with self.assertRaises(ValueError): self.extract(after=raw(c))
            with self.assertRaises(ValueError): self.extract(before=raw(c))

    def test_jpy_keeps_shopify_hundredths_convention(self):
        e = entry(); e["currency"] = "JPY"
        p = product(); p["variants"][1]["price"] = 100000
        cart = raw(dict(currency="JPY", items=[], item_count=0))
        self.assertEqual(self.extract(e=e, p=p, before=cart, after=cart)[0]["price"], 1000.0)

    def test_no_variant_fallback_ambiguous_id_or_rebound_sku(self):
        for mutation in range(3):
            p = product()
            if mutation == 0: p["variants"] = p["variants"][:1]
            elif mutation == 1: p["variants"].append(p["variants"][1])
            else: p["variants"][1]["sku"] = "WRONG"
            with self.assertRaises(ValueError): self.extract(p=p)

    def test_missing_availability_subscription_and_bad_price_refuse(self):
        for price in [True, 0, -1, 12.99, "1299", 2**53]:
            p = product(); p["variants"][1]["price"] = price
            with self.assertRaises(ValueError): self.extract(p=p)
        p = product(); p["variants"][1].pop("available")
        with self.assertRaises(ValueError): self.extract(p=p)
        p = product(); p["requires_selling_plan"] = True
        with self.assertRaises(ValueError): self.extract(p=p)

    def test_unavailable_is_an_observation_not_an_old_price_resurrection(self):
        p = product(); p["variants"][1]["available"] = False
        self.assertFalse(self.extract(p=p)[0]["available"])

    def test_sell_basis_drift_and_old_or_future_review_refuse(self):
        p = product(); p["description"] = "Now sold per full roll"
        with self.assertRaises(ValueError): self.extract(p=p)
        for date in ["2026-09-01", "2026-10-14"]:
            e = entry(); e["basis_reviewed_on"] = date
            with self.assertRaises(ValueError): self.extract(e=e)

    def test_only_anonymous_empty_cart_is_used(self):
        for cart in [dict(currency="USD", item_count=1, items=[{}]), dict(currency="USD")]:
            with self.assertRaises(ValueError): self.extract(before=raw(cart))

    def test_manifest_rejects_wrong_origin_locale_unknown_fields_or_guessed_units(self):
        for field, value in [("cart_url","https://other.invalid/cart.js"),
                ("cart_url","https://example.invalid/en/cart.js"), ("unit","hide"),
                ("currency","$"), ("quantity",True), ("minimum_quantity",0),
                ("variant_id",42.0), ("required_description_text",[]), ("unknown",1)]:
            e = entry(); e[field] = value
            with self.assertRaises(ValueError): r.validate_entry(e)

    def test_duplicate_json_keys_nonfinite_and_large_responses_refuse(self):
        for data in [b'{"price":1,"price":2}', b'{"price":NaN}', b'[]', b' '* (r.LIMIT+1)]:
            with self.assertRaises(ValueError): r.load(data)

    def test_refresh_orders_cart_product_cart_and_retains_product_evidence(self):
        calls = []
        def get(url):
            calls.append(url)
            return CART if url.endswith("cart.js") else raw(product())
        book, evidence = r.refresh(dict(schema="ftx-shopify-refresh/1", offers=[entry()]), get, lambda: NOW)
        self.assertEqual(calls, [entry()[k] for k in ["cart_url", "product_url", "cart_url"]])
        self.assertEqual(book["schema"], "ftx-textile-quotes/1")
        self.assertEqual(len(book["quotes"]), 1)
        self.assertEqual(evidence[book["quotes"][0]["source_sha256"]+".json"], raw(product()))
        self.assertEqual(len(evidence), 2)

    def test_batch_failure_publishes_nothing_and_preserves_old_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); manifest = root / "manifest.json"; out = root / "quotes.json"
            manifest.write_text(json.dumps(dict(schema="ftx-shopify-refresh/1", offers=[entry()])))
            argv = ["refresh", str(manifest), "--out", str(out), "--evidence", str(root / "evidence")]
            with patch("sys.argv", argv), patch.object(r.PublicSession, "get", side_effect=OSError("offline")):
                with self.assertRaises(SystemExit): r.main()
            self.assertFalse(out.exists())
            out.write_bytes(b"old evidence")
            with patch("sys.argv", argv), patch.object(r.PublicSession, "get") as get:
                with self.assertRaises(SystemExit): r.main()
                get.assert_not_called()
            self.assertEqual(out.read_bytes(), b"old evidence")

    def test_complete_snapshots_never_overwrite_previous_file(self):
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory) / "out.json"
            r.create_snapshot(p, b"first")
            with self.assertRaises(FileExistsError): r.create_snapshot(p, b"second")
            self.assertEqual(p.read_bytes(), b"first")
            self.assertEqual(list(p.parent.iterdir()), [p])

    def test_duplicate_offer_ids_refuse_before_any_network_read(self):
        with self.assertRaises(ValueError):
            r.refresh(dict(schema="ftx-shopify-refresh/1", offers=[entry(), entry()]), lambda _: self.fail("network"))


if __name__ == "__main__":
    unittest.main()
