"""Offline primary-response fixtures; synthetic prices are NEVER advertised as live observations."""
import copy
import csv
import datetime as dt
from email.message import Message
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.response import addinfourl

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


def public_entry():
    e = entry()
    del e["cart_url"]
    e.update(adapter="shopify_public_product_v1", public_url="https://example.invalid/products/sheet.json",
             product_id=7, variant_title="Black / half yard", inventory_mode="tracked_deny",
             reviewed_valid_through="2026-10-20")
    return e


def public_product():
    p = product()
    p["id"] = 7
    p["variants"][1].update(title="Black / half yard", quantity_rule={"min": 2, "max": None, "increment": 1},
                            quantity_price_breaks=[], inventory_quantity=20,
                            inventory_management="shopify", inventory_policy="deny")
    v = copy.deepcopy(p["variants"][1])
    v.update(price="12.99", price_currency="USD", product_id=7)
    return p, {"product": {"id": 7, "body_html": p["description"], "variants": [v]}}


CART = raw(dict(currency="USD", items=[], item_count=0, token="anonymous-do-not-persist"))


def csv_entry():
    e = entry()
    for key in ("product_url", "cart_url", "variant_id", "required_description_text"):
        del e[key]
    e.update(adapter="csv_v1", source_url="https://example.invalid/prices.csv", delimiter=",",
             columns={key: key.upper() for key in r.CSV_FIELDS},
             match={"SKU": "TEST-42", "MARKET": "US", "SIZE": "yardage"},
             availability={"available": ["yes"], "unavailable": ["no"]})
    return e


def csv_raw(rows=None, delimiter=","):
    rows = rows if rows is not None else [dict(SKU="TEST-42", PRICE="12.99", CURRENCY="USD", AVAILABLE="yes",
        QUANTITY="0.5", UNIT="linear_yard", MINIMUM_QUANTITY="1.0", ORDER_INCREMENT="0.5",
        MARKET="US", SIZE="yardage")]
    output = io.StringIO(newline="")
    writer = csv.DictWriter(output, fieldnames=list(rows[0]), delimiter=delimiter)
    writer.writeheader(); writer.writerows(rows)
    return output.getvalue().encode()


class PublicProductTests(unittest.TestCase):
    def test_continuous_cut_limit_requires_checked_source_text_and_preserves_linear_units(self):
        p, public = public_product()
        e = public_entry()
        fragment = "Maximum continuous cut: 5 yards."
        e.update(maximum_continuous_quantity=5, continuous_cut_text=fragment)
        e["required_description_text"].append(fragment)
        p["description"] += fragment
        public["product"]["body_html"] = p["description"]
        q, receipt = r.extract_public_product(e, raw(p), raw(public), raw(public), NOW)
        self.assertEqual(q["maximum_continuous_quantity"], 5)  # Not five half-yard packages.
        self.assertEqual(q["available_quantity"], 10)
        self.assertEqual(receipt["manifest_entry"]["continuous_cut_text"], fragment)
        self.assertNotIn("continuous_cut_text", q)
        revised = copy.deepcopy(public)
        revised["product"]["body_html"] = p["description"].replace("5 yards", "3 yards")
        with self.assertRaises(ValueError):
            r.extract_public_product(e, raw(p), raw(revised), raw(revised), NOW)
        for key, value in (("maximum_continuous_quantity", 0), ("maximum_continuous_quantity", True),
                           ("maximum_continuous_quantity", float("inf")), ("unit", "square_meter"),
                           ("continuous_cut_text", "Unchecked text")):
            invalid = dict(e); invalid[key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                r.validate_public_product(invalid)
        missing = dict(e); del missing["maximum_continuous_quantity"]
        with self.assertRaises(ValueError):
            r.validate_public_product(missing)

    def test_exact_offer_retains_capacity_sale_units_expiry_and_source_bytes(self):
        p, public = public_product()
        q, receipt = r.extract_public_product(public_entry(), raw(p), raw(public), raw(public), NOW,
                                               intraday=True)
        self.assertEqual((q["price"], q["available_quantity"], q["quantity"]), (12.99, 10.0, 0.5))
        self.assertEqual(q["valid_through"], "2026-10-20")
        self.assertEqual(q["observed_at"], "2026-10-13T12:00:00.000000Z")
        self.assertEqual(receipt["public_before_sha256"], hashlib.sha256(raw(public)).hexdigest())
        self.assertEqual(receipt["public_after_sha256"], receipt["public_before_sha256"])
        p["variants"][1].update(available=False, inventory_quantity=0)
        public["product"]["variants"][0]["inventory_quantity"] = 0
        q, _ = r.extract_public_product(public_entry(), raw(p), raw(public), raw(public), NOW)
        self.assertFalse(q["available"])
        self.assertEqual(q["available_quantity"], 0.0)

    def test_unreported_capacity_is_unknown_and_cannot_hide_new_numeric_evidence(self):
        p, public = public_product()
        e = public_entry(); e["inventory_mode"] = "unreported"
        with self.assertRaises(ValueError):
            r.extract_public_product(e, raw(p), raw(public), raw(public), NOW)
        del p["variants"][1]["inventory_quantity"]
        del public["product"]["variants"][0]["inventory_quantity"]
        q, _ = r.extract_public_product(e, raw(p), raw(public), raw(public), NOW)
        self.assertNotIn("available_quantity", q)

    def test_currency_variant_quantity_and_stock_drift_refuse_the_entire_observation(self):
        changes = {"price_currency": "CAD", "price": "13.00", "sku": "OTHER", "title": "sample",
                   "product_id": 8, "quantity_price_breaks": [{"minimum_quantity": 5, "price": "9.00"}],
                   "quantity_rule": {"min": 2, "max": 5, "increment": 1}, "inventory_quantity": 19,
                   "inventory_policy": "continue", "inventory_management": None}
        for key, value in changes.items():
            for which in ("before", "after"):
                p, public = public_product(); changed = copy.deepcopy(public)
                changed["product"]["variants"][0][key] = value
                before, after = (changed, public) if which == "before" else (public, changed)
                with self.subTest(key=key, which=which), self.assertRaises(ValueError):
                    r.extract_public_product(public_entry(), raw(p), raw(before), raw(after), NOW)
        for bad in (-1, True, 0.5, "20", 2**53):
            p, public = public_product()
            p["variants"][1]["inventory_quantity"] = bad
            public["product"]["variants"][0]["inventory_quantity"] = bad
            with self.subTest(stock=bad), self.assertRaises(ValueError):
                r.extract_public_product(public_entry(), raw(p), raw(public), raw(public), NOW)

    def test_expiry_identity_and_unconditional_source_contracts_are_required(self):
        for field, value in (("reviewed_valid_through", "2026-10-12"), ("product_id", 8),
                             ("variant_id", True), ("public_url", "https://other.invalid/products/sheet.json"),
                             ("order_increment", 1), ("minimum_quantity", 0.5)):
            p, public = public_product(); e = public_entry(); e[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                r.extract_public_product(e, raw(p), raw(public), raw(public), NOW)
        for field, value in (("requires_selling_plan", True), ("available", None), ("price", True),
                             ("quantity_price_breaks", None)):
            p, public = public_product(); p["variants"][1][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                r.extract_public_product(public_entry(), raw(p), raw(public), raw(public), NOW)
        p, public = public_product()
        public["product"]["body_html"] += " Changed finish."
        with self.assertRaises(ValueError):
            r.extract_public_product(public_entry(), raw(p), raw(public), raw(public), NOW)

    def test_refresh_never_reads_cart_and_retains_both_public_observations(self):
        p, public = public_product(); calls = []
        def get(url, *, accept):
            calls.append((url, accept))
            self.assertNotIn("cart", url)
            return raw(public) if url.endswith(".json") else raw(p)
        book, evidence = r.refresh({"schema": "ftx-textile-refresh/1", "offers": [public_entry()]},
                                   get, lambda: NOW, intraday=True)
        self.assertEqual([u.rsplit(".", 1)[1] for u, _ in calls], ["json", "js", "json"])
        self.assertTrue(all(a == "application/json" for _, a in calls))
        self.assertEqual(book["schema"], r.INTRADAY_SCHEMA)
        self.assertEqual(evidence[hashlib.sha256(raw(public)).hexdigest() + ".json"], raw(public))
        self.assertEqual(evidence[book["quotes"][0]["source_sha256"] + ".json"], raw(p))


class FabricMartTests(unittest.TestCase):
    """Synthetic response fragments reproduce a reviewed public template, never live offers."""
    def inputs(self, inventory=90, available=True):
        url = "https://fabricmartfabrics.com/products/synthetic-quarter-yard-knit"
        e = dict(offer_id="synthetic/fabricmart", material_id="synthetic/knit", material_sha256="a" * 64,
                 supplier="Synthetic Fabric Mart fixture", sku="SYNTHETIC", quantity=0.25,
                 unit="linear_yard", minimum_quantity=1, order_increment=0.25, currency="USD",
                 region="US", quality="synthetic", terms="synthetic-no-purchase",
                 adapter="fabricmart_decimal_html_v1", source_url=url, public_url=url + ".json",
                 product_id=7, variant_id=42, variant_title="Default Title", basis_source=url,
                 basis_note="Synthetic fixture; not observed inventory", basis_reviewed_on="2026-10-13",
                 inventory_basis="displayed_yards", specifications={"SKU": "SYNTHETIC",
                     "Content": "Nylon/elastane", "Width": '58"', "Type": "Knits - Activewear"})
        variant = dict(id=42, sku=e["sku"], title="Default Title", price=625, available=available,
                       inventory_management="shopify", inventory_policy="deny", inventory_quantity=inventory)
        product = dict(id=7, variants=[variant], tags=["decimal"])
        public = dict(product=dict(id=7, variants=[dict(variant, product_id=7, price="6.25",
            price_currency="USD", quantity_rule={"min": 1, "max": None, "increment": 1},
            quantity_price_breaks=[])]))
        html = self.page(e, product, inventory, available)
        return e, product, public, html

    @staticmethod
    def page(e, product, inventory=90, available=True):
        stock = f"In stock ({inventory // 4} yards available)" if available else "Out of stock"
        specs = "".join(f"<li><strong>{k}:</strong>{v}</li>" for k, v in e["specifications"].items())
        return (f'<section id="product-7" data-product-id="7" data-variant-inventory="{inventory}">'
                f'<script class="product-json" type="application/json">{json.dumps(product)}</script>'
                '<span class="price-item--regular-yards">$25.00 per yard</span>'
                f'<p class="stock-info"><span class="stock-dot"></span>{stock}</p>'
                '<label for="qty-7">Quantity</label>'
                '<label for="qty-7">Quantity : 1 yard, can only be changed in increments of 0.25.</label>'
                '<input name="properties[_decimal]" value="true">'
                '<input name="properties[_step]" value="0.25">'
                '<input name="properties[_minimum_qty]" value="1">'
                f'<ul class="product-specification">{specs}</ul></section>').encode()

    def test_quarter_yard_price_and_explicit_display_inventory_policy(self):
        e, _, public, html = self.inputs()
        q, receipt = r.extract_fabricmart(e, html, raw(public), html, NOW, intraday=True)
        self.assertEqual((q["price"], q["quantity"], q["minimum_quantity"], q["order_increment"]),
                         (6.25, 0.25, 1, 0.25))
        self.assertEqual(q["price"] / q["quantity"], 25)
        self.assertEqual(q["available_quantity"], 22)
        self.assertEqual(receipt["observed"]["inventory_yards"], 22.5)
        self.assertEqual(receipt["inventory_display_difference_yards"], 0.5)
        self.assertEqual(receipt["manifest_entry"]["inventory_basis"], "displayed_yards")
        self.assertNotIn("maximum_continuous_quantity", q)
        self.assertEqual(q["source_sha256"], hashlib.sha256(html).hexdigest())
        self.assertEqual(q["material_sha256"], e["material_sha256"])
        self.assertEqual(q["observed_at"], "2026-10-13T12:00:00.000000Z")

    def test_explicit_zero_stock_survives_and_never_becomes_unknown(self):
        e, _, public, html = self.inputs(inventory=0, available=False)
        q, _ = r.extract_fabricmart(e, html, raw(public), html, NOW)
        self.assertEqual(q["available_quantity"], 0)
        self.assertFalse(q["available"])
        for inventory, available in ((0, True), (90, False)):
            e, _, public, html = self.inputs(inventory, available)
            with self.assertRaises(ValueError):
                r.extract_fabricmart(e, html, raw(public), html, NOW)

    def test_quantity_price_identity_and_inventory_disagreements_refuse(self):
        e, product, public, html = self.inputs()
        changes = [(b"22 yards", b"90 yards"), (b"$25.00", b"$6.25"),
                   (b'value="0.25"', b'value="1"'), (b'"deny"', b'"continue"'),
                   (b'data-variant-inventory="90"', b'data-variant-inventory="92"'),
                   (b'"id": 42', b'"id": 43'), (b'58"', b'60"'),
                   (b'"inventory_quantity": 90', b'"inventory_quantity": -1')]
        for old, new in changes:
            changed = html.replace(old, new)
            self.assertNotEqual(changed, html)
            with self.subTest(old=old), self.assertRaises(ValueError):
                r.extract_fabricmart(e, changed, raw(public), changed, NOW)
        for key, value in (("price_currency", "CAD"), ("price", "7.00"),
                           ("sku", "OTHER"), ("title", "Sample"), ("product_id", 8),
                           ("quantity_price_breaks", [{"price": "5"}])):
            changed = copy.deepcopy(public); changed["product"]["variants"][0][key] = value
            with self.subTest(field=key), self.assertRaises(ValueError):
                r.extract_fabricmart(e, html, raw(changed), html, NOW)
        product["variants"].append(dict(product["variants"][0], id=43))
        ambiguous = self.page(e, product)
        with self.assertRaises(ValueError):
            r.extract_fabricmart(e, ambiguous, raw(public), ambiguous, NOW)

    def test_entire_bracket_refuses_later_valid_but_changed_stock(self):
        e, _, public, before = self.inputs()
        _, _, _, after = self.inputs(inventory=88)
        with self.assertRaisesRegex(ValueError, "changed during observation"):
            r.extract_fabricmart(e, before, raw(public), after, NOW)

    def test_missing_or_unscoped_html_cannot_supply_stock_or_specifications(self):
        e, _, public, html = self.inputs()
        for changed in (html + html, html.replace(b'stock-info', b'other-info'),
                        html.replace(b'</section>', b''),
                        html.replace(b'<section id="product-7"', b'<section id="product-8"')):
            with self.subTest(html=changed[:40]), self.assertRaises(ValueError):
                r.extract_fabricmart(e, changed, raw(public), changed, NOW)

    def test_manifest_requires_exact_source_material_and_inventory_policy(self):
        e, _, _, _ = self.inputs()
        for key, value in (("source_url", "https://example.invalid/products/sheet"),
                           ("material_sha256", ""), ("quantity", 1), ("minimum_quantity", 0.25),
                           ("inventory_basis", "automatically_choose_max"), ("variant_id", True)):
            changed = dict(e); changed[key] = value
            with self.subTest(field=key), self.assertRaises(ValueError):
                r.validate_fabricmart(changed)

    def test_refresh_is_read_only_bracketed_and_archives_all_source_bytes(self):
        e, _, public, html = self.inputs()
        calls = []
        def get(url, *, accept):
            calls.append((url, accept))
            self.assertNotIn("cart", url)
            return raw(public) if url.endswith(".json") else html
        book, evidence = r.refresh({"schema": "ftx-textile-refresh/1", "offers": [e]},
                                   get, lambda: NOW, intraday=True)
        self.assertEqual(calls, [(e["source_url"], "text/html"),
            (e["public_url"], "application/json"), (e["source_url"], "text/html")])
        self.assertEqual(book["quotes"][0]["available_quantity"], 22)
        self.assertEqual(evidence[hashlib.sha256(html).hexdigest() + ".html"], html)
        self.assertEqual(evidence[hashlib.sha256(raw(public)).hexdigest() + ".json"], raw(public))


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

    def test_intraday_retains_actual_utc_retrieval_precision_without_changing_legacy_output(self):
        local = dt.datetime(2026, 10, 12, 22, 30, 1, 123456,
                            tzinfo=dt.timezone(dt.timedelta(hours=-4)))
        old, old_receipt = self.extract(now=local)
        q, receipt = r.extract(entry(), raw(product()), CART, CART, local, intraday=True)
        self.assertEqual(q["observed_at"], "2026-10-13T02:30:01.123456Z")
        self.assertEqual(q["observed_on"], "2026-10-13")
        self.assertEqual(receipt["quote_schema"], r.INTRADAY_SCHEMA)
        self.assertEqual(r.utc_instant(dt.datetime.fromisoformat(receipt["observed_at"])), q["observed_at"])
        self.assertEqual({k: v for k, v in q.items() if k != "observed_at"}, old)
        self.assertEqual({k: v for k, v in receipt.items() if k != "quote_schema"}, old_receipt)
        with self.assertRaises(ValueError):
            r.extract(entry(), raw(product()), CART, CART, local.replace(tzinfo=None), intraday=True)

    def test_intraday_refresh_stamps_each_exact_supplier_response_in_shared_receipts(self):
        feed = csv_entry(); feed["offer_id"] = "synthetic/csv"
        ticks = iter([NOW.replace(microsecond=1), NOW.replace(microsecond=2)])
        def get(url, *, accept):
            return csv_raw() if url.endswith(".csv") else CART if url.endswith("cart.js") else raw(product())
        book, evidence = r.refresh(dict(schema="ftx-textile-refresh/1", offers=[feed, entry()]),
                                   get, lambda: next(ticks), intraday=True)
        self.assertEqual(book["schema"], "ftx-textile-quotes/2")
        observed = {q["offer_id"]: q["observed_at"] for q in book["quotes"]}
        self.assertEqual(observed, {"synthetic/csv": "2026-10-13T12:00:00.000001Z",
                                    "synthetic/42": "2026-10-13T12:00:00.000002Z"})
        for name, data in evidence.items():
            if name.endswith(".receipt.json"):
                receipt = r.load(data)
                self.assertEqual(receipt["quote_schema"], book["schema"])
                self.assertEqual(r.utc_instant(dt.datetime.fromisoformat(receipt["observed_at"])),
                                 observed[receipt["manifest_entry"]["offer_id"]])

    def test_reviewed_material_lock_is_optional_and_not_a_source_inventory_number(self):
        e = entry(); e["material_sha256"] = "a1" * 32
        q, receipt = self.extract(e=e)
        self.assertEqual(q["material_sha256"], e["material_sha256"])
        self.assertEqual(receipt["manifest_entry"]["material_sha256"], e["material_sha256"])
        self.assertNotIn("available_quantity", q)
        for digest in [None, True, "", "g" * 64, "0" * 63]:
            e["material_sha256"] = digest
            with self.subTest(digest=digest), self.assertRaises(ValueError): self.extract(e=e)
        e = entry(); e["available_quantity"] = 100
        with self.assertRaises(ValueError): self.extract(e=e)

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
        def get(url, *, accept):
            self.assertEqual(accept, "application/json")
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


class CsvRefreshTests(unittest.TestCase):
    def test_all_unit_price_breaks_are_read_from_exact_source_cells_in_sale_units(self):
        e = csv_entry()
        e["columns"]["price_breaks"] = "ALL_UNIT_PRICES"
        row = next(csv.DictReader(io.StringIO(csv_raw().decode())))
        schedule = [{"minimum_quantity": 2.5, "price": 10.99, "sku": "TEST-42-bulk"},
                    {"minimum_quantity": 5, "price": 9.99}]
        data = csv_raw([dict(row, ALL_UNIT_PRICES=json.dumps(schedule))])
        q, receipt = r.extract_csv(e, data, NOW, intraday=True)
        self.assertEqual(q["price_breaks"], schedule)
        self.assertEqual(q["quantity"], 0.5)  # Thresholds are yards, not half-yard packages.
        self.assertEqual(q["price"], 12.99)
        self.assertEqual(q["source_sha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(receipt["manifest_entry"]["columns"]["price_breaks"], "ALL_UNIT_PRICES")
        self.assertEqual(q["observed_at"], "2026-10-13T12:00:00.000000Z")
        empty, _ = r.extract_csv(e, csv_raw([dict(row, ALL_UNIT_PRICES="[]")]), NOW)
        self.assertNotIn("price_breaks", empty)
        unreviewed, _ = r.extract_csv(csv_entry(), data, NOW)
        self.assertNotIn("price_breaks", unreviewed)
        with self.assertRaises(ValueError):
            r.extract_csv(e, csv_raw(), NOW)

    def test_price_break_schedule_refuses_ambiguity_nonfinite_and_unsupported_tier_semantics(self):
        e = csv_entry()
        e["columns"]["price_breaks"] = "ALL_UNIT_PRICES"
        row = next(csv.DictReader(io.StringIO(csv_raw().decode())))
        invalid = ["", "null", "{}", '[{"minimum_quantity":2,"price":8,"price":7}]',
                   '[{"minimum_quantity":2,"price":NaN}]',
                   '[{"minimum_quantity":2,"price":1e999}]',
                   '[{"minimum_quantity":2,"price":8,"maximum_quantity":5}]',
                   '[{"minimum_quantity":2,"price":8,"sku":" "}]',
                   '[{"minimum_quantity":2,"price":8,"sku":3}]']
        for threshold, price in [(0, 8), (1, 8), (True, 8), (-1, 8), (2, 0), (2, -1),
                                 (2, True), ("2", 8), (2, "8"), (10**400, 8), (2, 10**400)]:
            invalid.append(json.dumps([{"minimum_quantity": threshold, "price": price}]))
        invalid += [json.dumps([{"minimum_quantity": n, "price": 8} for n in ns])
                    for ns in ([3, 2], [2, 2], range(2, 67))]
        for cell in invalid:
            with self.subTest(cell=cell), self.assertRaises(ValueError):
                r.extract_csv(e, csv_raw([dict(row, ALL_UNIT_PRICES=cell)]), NOW)
        # A manifest cannot smuggle assumed rates into observations.
        e["price_breaks"] = [{"minimum_quantity": 2, "price": 8}]
        with self.assertRaises(ValueError):
            r.validate_csv(e)

    def test_continuous_cut_column_is_checked_and_cannot_be_inferred_from_inventory(self):
        e = csv_entry()
        e["maximum_continuous_quantity"] = 5
        e["columns"]["maximum_continuous_quantity"] = "MAX_CUT_YD"
        e["columns"]["available_quantity"] = "TOTAL_YD"
        row = dict(SKU="TEST-42", PRICE="12.99", CURRENCY="USD", AVAILABLE="yes",
                   QUANTITY="0.5", UNIT="linear_yard", MINIMUM_QUANTITY="1.0", ORDER_INCREMENT="0.5",
                   MARKET="US", SIZE="yardage", MAX_CUT_YD="5", TOTAL_YD="200")
        q, _ = r.extract_csv(e, csv_raw([row]), NOW)
        self.assertEqual(q["maximum_continuous_quantity"], 5)
        self.assertEqual(q["available_quantity"], 200)
        row["MAX_CUT_YD"] = "3"
        with self.assertRaisesRegex(ValueError, "maximum_continuous_quantity changed"):
            r.extract_csv(e, csv_raw([row]), NOW)
        del e["maximum_continuous_quantity"]
        with self.assertRaises(ValueError):
            r.validate_csv(e)

    def test_exact_offer_and_original_bytes_reach_existing_quote_contract(self):
        data = csv_raw()
        q, receipt = r.extract_csv(csv_entry(), data, NOW)
        self.assertEqual(q["price"], 12.99)
        self.assertEqual(q["quantity"], 0.5)
        self.assertEqual(q["source_sha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(receipt["selected_record"], 2)
        self.assertEqual(set(q), set(r.FIELDS) | {"price", "observed_on", "available", "source", "source_sha256"})

    def test_swatch_and_other_markets_do_not_become_yardage(self):
        row = next(csv.DictReader(io.StringIO(csv_raw().decode())))
        rows = [dict(row, SIZE="swatch", PRICE="0.10"), dict(row, MARKET="CA", PRICE="8.99"), row]
        q, receipt = r.extract_csv(csv_entry(), csv_raw(rows), NOW)
        self.assertEqual(q["price"], 12.99)
        self.assertEqual(receipt["selected_record"], 4)
        with self.assertRaises(ValueError): r.extract_csv(csv_entry(), csv_raw(rows[:2]), NOW)
        with self.assertRaises(ValueError): r.extract_csv(csv_entry(), csv_raw([row, row]), NOW)

    def test_explicit_available_sale_units_and_material_lock_preserve_source_evidence(self):
        e = csv_entry(); e["material_sha256"] = "b2" * 32
        e["columns"]["available_quantity"] = "AVAILABLE_YARDS"
        row = next(csv.DictReader(io.StringIO(csv_raw().decode())))
        data = csv_raw([dict(row, AVAILABLE_YARDS="1.25")])
        q, receipt = r.extract_csv(e, data, NOW)
        self.assertEqual(q["available_quantity"], 1.25)
        self.assertEqual(q["unit"], "linear_yard")
        self.assertEqual(q["quantity"], 0.5)  # NOT 1.25 half-yard packages.
        self.assertEqual(q["material_sha256"], "b2" * 32)
        self.assertEqual(q["source_sha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(receipt["manifest_entry"]["columns"]["available_quantity"], "AVAILABLE_YARDS")
        zero, _ = r.extract_csv(e, csv_raw([dict(row, AVAILABLE_YARDS="0", AVAILABLE="no")]), NOW)
        self.assertEqual(zero["available_quantity"], 0.0)
        self.assertFalse(zero["available"])
        for value in ["", "-1", "unknown", "1e2", "0." + "0" * 400 + "1", "9" * 400]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                r.extract_csv(e, csv_raw([dict(row, AVAILABLE_YARDS=value)]), NOW)
        with self.assertRaises(ValueError): r.extract_csv(e, csv_raw(), NOW)
        # No reviewed column means capacity is unknown, even if the feed has unrelated numbers.
        unreviewed, _ = r.extract_csv(csv_entry(), data, NOW)
        self.assertNotIn("available_quantity", unreviewed)

    def test_basis_currency_and_stock_drift_refuse(self):
        row = next(csv.DictReader(io.StringIO(csv_raw().decode())))
        for key, value in [("QUANTITY", "1"), ("UNIT", "yard"), ("MINIMUM_QUANTITY", "5"),
                           ("ORDER_INCREMENT", "1"), ("CURRENCY", "CAD"), ("AVAILABLE", "")]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                r.extract_csv(csv_entry(), csv_raw([dict(row, **{key: value})]), NOW)
        q, _ = r.extract_csv(csv_entry(), csv_raw([dict(row, AVAILABLE="no")]), NOW)
        self.assertFalse(q["available"])

    def test_explicit_dialects_bom_and_quoted_cells(self):
        row = next(csv.DictReader(io.StringIO(csv_raw().decode())))
        for delimiter in (",", ";", "\t"):
            e = csv_entry(); e["delimiter"] = delimiter
            e["match"]["NOTES"] = 'test,"quoted"\nsecond line'
            data = b"\xef\xbb\xbf" + csv_raw([dict(row, NOTES=e["match"]["NOTES"])], delimiter)
            self.assertEqual(r.extract_csv(e, data, NOW)[0]["price"], 12.99)

    def test_malformed_structure_and_ambiguous_mapping_refuse(self):
        for data in [b"SKU,SKU\n1,2\n", csv_raw() + b"short,row\n", csv_raw() + b'"unfinished',
                     b"\xff", b"\0", b"x" * (r.LIMIT + 1)]:
            with self.subTest(data=data[:50]), self.assertRaises(ValueError):
                r.extract_csv(csv_entry(), data, NOW)
        for change in [{"match": {}}, {"availability": {"available": ["yes"], "unavailable": ["yes"]}},
                       {"delimiter": "|"}, {"source_url": "https://other.invalid/prices.csv"},
                       {"columns": {key: "SAME" for key in r.CSV_FIELDS}}, {"unexpected": True}]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                r.validate_csv(dict(csv_entry(), **change))

    def test_strict_decimal_no_locale_currency_exponent_or_nonfinite_guessing(self):
        row = next(csv.DictReader(io.StringIO(csv_raw().decode())))
        for price in ["", "$12.99", "1,299", "12,99", "1e2", "NaN", "Infinity", " 1", "-1", "0", "9" * 400]:
            with self.subTest(price=price), self.assertRaises(ValueError):
                r.extract_csv(csv_entry(), csv_raw([dict(row, PRICE=price)]), NOW)

    def test_one_feed_fetch_per_snapshot_and_legacy_shopify_in_same_batch(self):
        calls = []
        a = csv_entry(); b = copy.deepcopy(a); b["offer_id"] = "synthetic/42/other-scope"
        c = entry(); c["offer_id"] = "synthetic/shopify42"
        def get(url, *, accept):
            self.assertEqual(accept, "text/csv" if url.endswith(".csv") else "application/json")
            calls.append(url)
            return csv_raw() if url.endswith(".csv") else CART if url.endswith("cart.js") else raw(product())
        book, evidence = r.refresh(dict(schema="ftx-textile-refresh/1", offers=[c, b, a]), get, lambda: NOW)
        self.assertEqual(calls.count(a["source_url"]), 1)
        self.assertEqual([q["offer_id"] for q in book["quotes"]], sorted(e["offer_id"] for e in [a, b, c]))
        self.assertEqual(len(book["quotes"]), 3)
        self.assertEqual(sum(k.endswith(".csv") for k in evidence), 1)

    def test_invalid_adapter_old_review_and_naive_clock_refuse(self):
        for e, now in [(dict(csv_entry(), basis_reviewed_on="2026-09-01"), NOW),
                       (csv_entry(), NOW.replace(tzinfo=None))]:
            with self.assertRaises(ValueError): r.extract_csv(e, csv_raw(), now)
        with self.assertRaises(ValueError):
            r.refresh(dict(schema="ftx-textile-refresh/1", offers=[dict(csv_entry(), adapter="guess")]),
                      lambda _: self.fail("network"))


class RobotsTests(unittest.TestCase):
    def test_supplier_cart_exclusions_win_over_broad_allow_in_either_order(self):
        rules = ["Allow: /", "Disallow: /cart/", "Disallow: /cart.js", "Disallow: /*/cart.js"]
        for ordered in (rules, list(reversed(rules))):
            policy = r.RobotsPolicy("User-agent: *\n" + "\n".join(ordered))
            for path in ("/cart.js", "/cart/add", "/en/cart.js", "/fr-ca/cart.js"):
                with self.subTest(path=path, ordered=ordered):
                    self.assertFalse(policy.can_fetch("https://example.invalid" + path))
            self.assertTrue(policy.can_fetch(entry()["product_url"]))

    def test_longest_rule_allow_ties_and_glob_specificity(self):
        for rules, path, allowed in [
            (["Disallow: /", "Allow: /products/"], "/products/silk", True),
            (["Allow: /products/", "Disallow: /products/private"], "/products/private", False),
            (["Disallow: /same", "Allow: /same"], "/same", True),
            (["Allow: /page", "Disallow: /*.htm"], "/page.htm", False),
            (["Allow: /page", "Disallow: /*.ph"], "/page.php", True),
            (["Disallow: /$", "Allow: /"], "/", False),
        ]:
            for ordered in (rules, list(reversed(rules))):
                with self.subTest(rules=ordered, path=path):
                    policy = r.RobotsPolicy("User-agent: *\n" + "\n".join(ordered))
                    self.assertEqual(policy.can_fetch("https://example.invalid" + path), allowed)

    def test_specific_groups_merge_and_do_not_borrow_wildcard_rules(self):
        text = """Disallow: /outside-group
User-agent: *
Disallow: /fallback
User-agent: Xai
Disallow: /
User-agent: OtherBot
User-agent: XAIIMAGEAPIFETCH
Disallow: /first
Sitemap: https://example.invalid/sitemap.xml
Disallow: /after-sitemap

User-agent: XaiImageApiFetch/1.0
Disallow: /second
"""
        policy = r.RobotsPolicy(text)
        for path in ("/first", "/second", "/after-sitemap"):
            self.assertFalse(policy.can_fetch("https://example.invalid" + path))
        for path in ("/fallback", "/outside-group", "/products/silk"):
            self.assertTrue(policy.can_fetch("https://example.invalid" + path))
        self.assertFalse(r.RobotsPolicy(text, "UnlistedBot").can_fetch("https://example.invalid/fallback"))

    def test_empty_rules_groups_bom_comments_and_unknown_records(self):
        for text in ("", "User-agent: DifferentBot\nDisallow: /",
                     "User-agent: *\nDisallow: /\nUser-agent: XaiImageApiFetch",
                     "User-agent: *\nDisallow:\nAllow:"):
            self.assertTrue(r.RobotsPolicy(text).can_fetch(entry()["product_url"]))
        policy = r.RobotsPolicy("\ufeffUser-Agent: XaiImageApiFetch\n\nSitemap: ignored\n"
                                "User-agent: OtherBot\nDisallow: /private # inline comment\n")
        self.assertFalse(policy.can_fetch("https://example.invalid/private"))
        malformed = r.RobotsPolicy("User-agent: *\nAllow: /\nUser-agent:\nDisallow: /cart.js")
        self.assertFalse(malformed.can_fetch(entry()["cart_url"]))
        self.assertTrue(r.RobotsPolicy("User-agent: *\nDisallow: /").can_fetch("https://example.invalid/robots.txt"))

    def test_wildcards_end_anchor_query_and_case(self):
        policy = r.RobotsPolicy("User-agent: *\nDisallow: /*/cart.js$\nDisallow: /*.csv$\n"
                                "Disallow: /offer?preview=\nDisallow: /a*b*c$\n")
        for path in ("/en/cart.js", "/prices.csv", "/nested/prices.csv", "/offer?preview=yes", "/abbcc"):
            self.assertFalse(policy.can_fetch("https://example.invalid" + path), path)
        for path in ("/en/cart.js/more", "/prices.csv?version=1", "/prices.CSV", "/abdc/more"):
            self.assertTrue(policy.can_fetch("https://example.invalid" + path), path)

    def test_percent_encoding_preserves_reserved_separators_and_literal_operators(self):
        for rule, path, allowed in [
            ("/cart", "/%63art.js", False), ("/%63art", "/cart.js", False),
            ("/café", "/caf%C3%A9", False), ("/caf%c3%a9", "/café", False),
            ("/a%2Fb", "/a/b", True), ("/a/b", "/a%2fb", True),
            ("/a%2Fb", "/a%2fb", False), ("/a%252Fb", "/a%2Fb", True),
            ("/file%2A", "/file*", False), ("/file%2A", "/file-other", True),
            ("/cost%24", "/cost$", False), ("/x%23secret", "/x%23secret", False),
        ]:
            with self.subTest(rule=rule, path=path):
                policy = r.RobotsPolicy("User-agent: *\nDisallow: " + rule)
                self.assertEqual(policy.can_fetch("https://example.invalid" + path), allowed)
        policy = r.RobotsPolicy("User-agent: *\nDisallow: /%63art\nAllow: /cart")
        self.assertTrue(policy.can_fetch("https://example.invalid/cart"))


class PublicSessionTests(unittest.TestCase):
    def session(self, replies):
        """Keep urllib's redirects/errors/cookie handling; replace only HTTP transport."""
        calls = []
        class FixtureHttp(r.urllib.request.BaseHandler):
            handler_order = 0
            def https_open(self, request):
                calls.append(request)
                reply = replies[request.full_url]
                if callable(reply):
                    reply = reply(request)
                code, body, location = reply if isinstance(reply, tuple) else (200, reply, None)
                headers = Message()
                if location:
                    headers["Location"] = location
                response = addinfourl(io.BytesIO(body), headers, request.full_url, code)
                response.msg = "synthetic HTTP fixture"
                return response
        session = r.PublicSession()
        session.opener.add_handler(FixtureHttp())
        return session, calls

    def test_blocked_cart_never_reaches_http(self):
        host = "https://example.invalid"
        session, calls = self.session({host + "/robots.txt": b"User-agent: *\nAllow: /\nDisallow: /cart.js"})
        with patch.object(r.time, "sleep") as sleep:
            with self.assertRaisesRegex(ValueError, "robots.txt disallows"):
                session.get(host + "/cart.js")
        self.assertEqual([c.full_url for c in calls], [host + "/robots.txt"])
        sleep.assert_not_called()

    def test_manifest_requests_the_reviewed_representation_and_caches_it_separately(self):
        e = entry()
        for key in ("product_url", "cart_url", "variant_id"):
            del e[key]
        e.update(adapter="jsonld_v1", source_url="https://example.invalid/products/sheet",
                 source_format="html", offer_ref=None, tax_included=None)
        ld = {"@context": "https://schema.org", "@type": "Product", "sku": "TEST-42",
              "description": "Sold per half yard", "offers": {"@type": "Offer", "price": "12.99",
              "priceCurrency": "USD", "availability": "https://schema.org/InStock"}}
        page = b'<script type="application/ld+json">' + raw(ld) + b'</script>'
        def negotiated(request):
            # Ordinary Shopify product JSON is not the page's embedded JSON-LD.
            return {"text/html": page, "application/ld+json,application/json": raw(ld)}.get(
                request.get_header("Accept"), raw({"product": {"sku": "TEST-42"}}))
        session, calls = self.session({"https://example.invalid/robots.txt": b"User-agent: *\nDisallow:",
                                      e["source_url"]: negotiated})
        feed = dict(e, offer_id="synthetic/feed", source_format="json")
        same_page = dict(e, offer_id="synthetic/other-scope")
        with patch.object(r.time, "sleep"):
            book, evidence = r.refresh(dict(schema="ftx-textile-refresh/1", offers=[e, feed, same_page]),
                                      session.get, lambda: NOW)
        self.assertEqual([c.get_header("Accept") for c in calls],
                         ["text/plain", "text/html", "application/ld+json,application/json"])
        self.assertEqual(len(book["quotes"]), 3)
        self.assertEqual(evidence[hashlib.sha256(page).hexdigest() + ".html"], page)
        self.assertEqual(evidence[hashlib.sha256(raw(ld)).hexdigest() + ".json"], raw(ld))

    def test_headers_policy_cache_and_delays_include_first_source_request(self):
        host = "https://example.invalid"
        robots = b"User-agent: *\nCrawl-delay: 2\nUser-agent: XaiImageApiFetch\nCrawl-delay: 3.5\nRequest-rate: 2/10"
        session, calls = self.session({host + "/robots.txt": robots, host + "/prices.csv": b"prices"})
        with patch.object(r.time, "monotonic", return_value=100.0), patch.object(r.time, "sleep") as sleep:
            self.assertEqual(session.get(host + "/prices.csv"), b"prices")
            self.assertEqual(session.get(host + "/prices.csv"), b"prices")
        self.assertEqual([c.full_url for c in calls], [host + "/robots.txt", host + "/prices.csv", host + "/prices.csv"])
        self.assertEqual([c.args for c in sleep.call_args_list], [(5.0,), (5.0,)])
        for request in calls:
            self.assertEqual(request.get_header("User-agent"), "OpenAI File Downloader, XaiImageApiFetch/1.0")
            self.assertEqual(request.get_method(), "GET")
        self.assertEqual(r.RobotsPolicy("User-agent: *\nCrawl-delay: 4\nUser-agent: XaiImageApiFetch\nAllow: /").delay, 4)

    def test_only_robots_404_is_treated_as_no_policy(self):
        host = "https://example.invalid"
        for status in (404, 403, 429, 503):
            session, calls = self.session({host + "/robots.txt": (status, b"", None), host + "/price": b"price"})
            with self.subTest(status=status), patch.object(r.time, "sleep"):
                if status == 404:
                    self.assertEqual(session.get(host + "/price"), b"price")
                    self.assertEqual(len(calls), 2)
                else:
                    with self.assertRaises(r.urllib.error.HTTPError):
                        session.get(host + "/price")
                    self.assertEqual(len(calls), 1)

    def test_redirects_recheck_exclusions_and_pace_allowed_hops(self):
        host = "https://example.invalid"
        for destination in ("/cart.js", "https://other.invalid/price", "/public"):
            replies = {host + "/robots.txt": b"User-agent: *\nAllow: /\nDisallow: /cart.js\nCrawl-delay: 2",
                       host + "/price": (302, b"", destination), host + "/public": b"public"}
            session, calls = self.session(replies)
            with self.subTest(destination=destination), patch.object(r.time, "monotonic", return_value=100.0), \
                    patch.object(r.time, "sleep") as sleep:
                if destination == "/public":
                    self.assertEqual(session.get(host + "/price"), b"public")
                    self.assertEqual([c.args for c in sleep.call_args_list], [(2.0,), (2.0,)])
                    self.assertEqual(len(calls), 3)
                else:
                    with self.assertRaisesRegex(ValueError, "robots.txt disallows|cross-origin redirect"):
                        session.get(host + "/price")
                    self.assertEqual([c.full_url for c in calls], [host + "/robots.txt", host + "/price"])

    def test_robots_redirect_bootstrap_keeps_the_original_supplier_policy(self):
        host = "https://example.invalid"
        session, calls = self.session({host + "/robots.txt": (302, b"", "/private/rules"),
                                      host + "/private/rules": b"User-agent: *\nDisallow: /private\nCrawl-delay: 2",
                                      host + "/public": b"public"})
        with patch.object(r.time, "monotonic", return_value=100.0), patch.object(r.time, "sleep") as sleep:
            self.assertEqual(session.get(host + "/public"), b"public")
            with self.assertRaisesRegex(ValueError, "robots.txt disallows"):
                session.get(host + "/private/rules")
        self.assertEqual([c.full_url for c in calls], [host + "/robots.txt", host + "/private/rules", host + "/public"])
        self.assertEqual([c.args for c in sleep.call_args_list], [(2.0,)])
        self.assertTrue(all(c.get_header("User-agent") == r.AGENT for c in calls))


if __name__ == "__main__":
    unittest.main()
