"""Supplier revision regressions using synthetic observations, never live material claims."""
import copy
import datetime as dt
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import refresh_textile_prices as r
import textile_specifications as s
from test_refresh_textile_prices import CART, NOW, csv_entry, csv_raw, entry, product, raw, public_entry, public_product


def locked(e=None):
    return dict(e or entry(), material_sha256="ab" * 32)


def snapshot(e=None, source=None, now=NOW):
    e = e or locked()
    source = source if source is not None else raw(product())
    def get(url, *, accept):
        return CART if url.endswith("cart.js") else source
    book, evidence = r.refresh({"schema": "ftx-textile-refresh/1", "offers": [e]}, get, lambda: now)
    return s.capture(book, evidence), evidence


def jsonld_fixture():
    e = locked()
    for k in ("product_url", "cart_url", "variant_id"):
        del e[k]
    e.update(adapter="jsonld_v1", source_url="https://example.invalid/products/sheet",
             source_format="json", offer_ref="offer", tax_included=None)
    document = {"@context": "https://schema.org", "@graph": [
        {"@id": "product", "@type": "Product", "sku": e["sku"],
         "description": "Sold per half yard; 100% linen.", "offers": {"@id": "offer"},
         "additionalProperty": {"@id": "width"}},
        {"@id": "offer", "@type": "Offer", "price": "12.99", "priceCurrency": "USD",
         "availability": "https://schema.org/InStock", "itemOffered": {"@id": "product"}},
        {"@id": "width", "@type": "PropertyValue", "name": "nominal width",
         "value": 140, "unitText": "cm"},
    ]}
    return e, document


class SpecificationTests(unittest.TestCase):
    def test_reviewed_continuous_limit_changes_require_new_source_review(self):
        e, p = locked(), product()
        fragment = "Maximum continuous cut: 5 yards."
        e.update(maximum_continuous_quantity=5, continuous_cut_text=fragment)
        e["required_description_text"].append(fragment)
        p["description"] += fragment
        old, evidence = snapshot(e, raw(p))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, data in evidence.items():
                (root / name).write_bytes(data)
            self.assertEqual(s.replay(old, root), old)
        # Even an edited numeric interpretation with identical source bytes changes the binding.
        e["maximum_continuous_quantity"] = 3
        new, _ = snapshot(e, raw(p))
        review = s.compare(old, new)
        self.assertFalse(review["quote_publication_allowed"])
        self.assertIn("/binding/maximum_continuous_quantity",
                      [x["path"] for x in review["offers"][0]["changes"]])
        del e["maximum_continuous_quantity"]
        del e["continuous_cut_text"]
        unknown, _ = snapshot(e, raw(p))
        self.assertFalse(s.compare(old, unknown)["quote_publication_allowed"])

    def test_public_product_replays_corroborating_sources_and_separates_stock_from_specification(self):
        p, public = public_product()
        e = locked(public_entry())
        def capture():
            def get(url, *, accept):
                return raw(public) if url.endswith(".json") else raw(p)
            book, evidence = r.refresh({"schema": "ftx-textile-refresh/1", "offers": [e]}, get,
                                       lambda: NOW, intraday=True)
            return s.capture(book, evidence), evidence
        old, original = capture()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, data in original.items():
                (root / name).write_bytes(data)
            self.assertEqual(s.replay(old, root), old)
            key = next(name for name, data in original.items() if data == raw(public))
            (root / key).write_bytes(b'{"product": {}}')
            with self.assertRaises(ValueError):
                s.replay(old, root)
        p["variants"][1].update(price=1550, inventory_quantity=0, available=False)
        public["product"]["variants"][0].update(price="15.50", inventory_quantity=0)
        refreshed, _ = capture()
        self.assertTrue(s.compare(old, refreshed)["quote_publication_allowed"])
        p["description"] += " Revised weave."
        public["product"]["body_html"] = p["description"]
        revised, _ = capture()
        self.assertFalse(s.compare(old, revised)["quote_publication_allowed"])
        missing = dict(original)
        missing.pop(key)
        with self.assertRaises(KeyError):
            s.capture({"quotes": [{"offer_id": e["offer_id"], "source_sha256": old["observations"][0]["source_sha256"]}]}, missing)

    def test_price_stock_and_unselected_variants_do_not_change_specification(self):
        p = product()
        p["variants"][1].update(unit_price=2598,
                                 unit_price_measurement={"quantity_value": 0.5, "quantity_unit": "yd"})
        old, _ = snapshot(source=raw(p))
        p["price_min"] = 400
        p["variants"][0].update(sku="ANOTHER", title="unselected revised item", price=400)
        p["variants"][1].update(price=1550, unit_price=3100, compare_at_price=1800, available=False)
        new, _ = snapshot(source=raw(p), now=NOW + dt.timedelta(hours=1))
        report = s.compare(old, new)
        self.assertTrue(report["quote_publication_allowed"])
        self.assertEqual(report["offers"][0]["status"], "unchanged")
        self.assertNotEqual(report["offers"][0]["previous"]["source_sha256"],
                            report["offers"][0]["current"]["source_sha256"])
        p["variants"][1]["unit_price_measurement"]["quantity_unit"] = "m"
        revised, _ = snapshot(source=raw(p))
        self.assertFalse(s.compare(old, revised)["quote_publication_allowed"])

    def test_description_finish_variant_and_original_dimensions_remain_visible(self):
        p = product()
        p.update(description="Sold per half yard; 100% linen; 140 cm; washed.")
        p["variants"][1].update(title="Black / yardage", weight=210, weight_unit="g")
        old, _ = snapshot(source=raw(p))
        p["description"] = "Sold per half yard; 55% linen / 45% cotton; 135 cm; coated."
        p["variants"][1]["title"] = "Black / sample"
        new, _ = snapshot(source=raw(p))
        report = s.compare(old, new)
        self.assertFalse(report["quote_publication_allowed"])
        changes = {x["path"]: x for x in report["offers"][0]["changes"]}
        self.assertEqual(changes["/selected/variant/title"]["after"], "Black / sample")
        self.assertIn("140 cm", changes["/selected/product/description"]["before"])
        self.assertIn("135 cm", changes["/selected/product/description"]["after"])
        self.assertEqual(new["observations"][0]["facts"]["selected"]["variant"]["weight_unit"], "g")
        self.assertEqual(report["offers"][0]["measured_specimen_binding"], "unresolved")

    def test_csv_extra_columns_are_specifications_and_source_inventory_is_not(self):
        e = locked(csv_entry())
        e["columns"]["available_quantity"] = "YARDS"
        row = dict(SKU="TEST-42", PRICE="12.99", CURRENCY="USD", AVAILABLE="yes",
                   QUANTITY="0.5", UNIT="linear_yard", MINIMUM_QUANTITY="1.0", ORDER_INCREMENT="0.5",
                   MARKET="US", SIZE="yardage", YARDS="12.5", WIDTH="140 cm", WEAVE="plain")
        old, _ = snapshot(e, csv_raw([row]))
        row.update(PRICE="15.00", YARDS="0", AVAILABLE="no")
        new, _ = snapshot(e, csv_raw([row]))
        self.assertTrue(s.compare(old, new)["quote_publication_allowed"])
        row.update(WIDTH="135 cm", WEAVE="twill")
        changed, _ = snapshot(e, csv_raw([row]))
        report = s.compare(old, changed)
        self.assertEqual({x["path"] for x in report["offers"][0]["changes"]},
                         {"/selected/row/WIDTH", "/selected/row/WEAVE"})

    def test_csv_price_break_rates_refresh_but_threshold_changes_require_review(self):
        e = locked(csv_entry())
        e["columns"]["price_breaks"] = "ALL_UNIT_PRICES"
        row = dict(SKU="TEST-42", PRICE="12.99", CURRENCY="USD", AVAILABLE="yes",
                   QUANTITY="0.5", UNIT="linear_yard", MINIMUM_QUANTITY="1.0", ORDER_INCREMENT="0.5",
                   MARKET="US", SIZE="yardage")
        row["ALL_UNIT_PRICES"] = json.dumps([{"minimum_quantity": 5, "price": 10.99}])
        old, evidence = snapshot(e, csv_raw([row]))
        with tempfile.TemporaryDirectory() as directory:
            for name, data in evidence.items():
                (Path(directory) / name).write_bytes(data)
            self.assertEqual(s.replay(old, Path(directory)), old)
        row["ALL_UNIT_PRICES"] = json.dumps([{"minimum_quantity": 5, "price": 9.99}])
        market, _ = snapshot(e, csv_raw([row]))
        self.assertTrue(s.compare(old, market)["quote_publication_allowed"])
        row["ALL_UNIT_PRICES"] = json.dumps([{"minimum_quantity": 6, "price": 9.99}])
        revised, _ = snapshot(e, csv_raw([row]))
        report = s.compare(old, revised)
        self.assertFalse(report["quote_publication_allowed"])
        self.assertEqual(report["offers"][0]["changes"][0]["path"],
                         "/selected/price_break_conditions")
        row["ALL_UNIT_PRICES"] = json.dumps([{"minimum_quantity": 5, "price": 10.99, "sku": "TEST-42-bulk"}])
        rebound, _ = snapshot(e, csv_raw([row]))
        self.assertFalse(s.compare(old, rebound)["quote_publication_allowed"])
        row["ALL_UNIT_PRICES"] = "[]"
        removed, _ = snapshot(e, csv_raw([row]))
        self.assertFalse(s.compare(old, removed)["quote_publication_allowed"])

    def test_jsonld_replays_linked_original_quantities_and_ignores_offer_prices(self):
        e, source = jsonld_fixture()
        old, _ = snapshot(e, raw(source))
        source["@graph"][1].update(price="20.00", availability="https://schema.org/OutOfStock")
        new, _ = snapshot(e, raw(source))
        self.assertTrue(s.compare(old, new)["quote_publication_allowed"])
        source["@graph"][2]["value"] = 54
        source["@graph"][2]["unitText"] = "in"
        changed, _ = snapshot(e, raw(source))
        report = s.compare(old, changed)
        self.assertFalse(report["quote_publication_allowed"])
        quantity = changed["observations"][0]["facts"]["selected"]["product"]["additionalProperty"]
        self.assertEqual((quantity["value"], quantity["unitText"]), (54, "in"))

    def test_jsonld_inventory_refresh_preserves_original_facts_and_material_revision(self):
        e, source = jsonld_fixture()
        source["@graph"][1]["inventoryLevel"] = {"@id": "stock"}
        source["@graph"].append({"@id": "stock", "@type": "QuantitativeValue",
                                  "value": 12.5, "unitCode": "YRD"})
        old, evidence = snapshot(e, raw(source))
        retained = s.encoded(old)
        # The snapshot representation stays compatible with pre-capacity captures: original stock
        # facts remain present and replay-checked, even though they are not textile specifications.
        self.assertEqual(old["observations"][0]["facts"]["selected"]["offer"]["inventoryLevel"]["value"], 12.5)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, data in evidence.items():
                (root / name).write_bytes(data)
            self.assertEqual(s.replay(old, root), old)
            edited = copy.deepcopy(old)
            edited["observations"][0]["facts"]["selected"]["offer"]["inventoryLevel"]["value"] = 100
            with self.assertRaisesRegex(ValueError, "differs"):
                s.replay(edited, root)
        source["@graph"][3]["value"] = 0
        source["@graph"][1].update(price="16.00", availability="https://schema.org/OutOfStock")
        depleted, _ = snapshot(e, raw(source), now=NOW + dt.timedelta(hours=1))
        review = s.compare(old, depleted)
        self.assertTrue(review["quote_publication_allowed"])
        self.assertTrue(review["offers"][0]["same_material_revision"])
        self.assertEqual(review["offers"][0]["status"], "unchanged")
        self.assertEqual(review["offers"][0]["changes"], [])
        self.assertNotEqual(review["offers"][0]["previous"]["source_sha256"],
                            review["offers"][0]["current"]["source_sha256"])
        self.assertEqual(s.encoded(old), retained)
        del source["@graph"][1]["inventoryLevel"]
        unknown, _ = snapshot(e, raw(source), now=NOW + dt.timedelta(hours=2))
        self.assertTrue(s.compare(depleted, unknown)["quote_publication_allowed"])
        source["@graph"][2]["value"] = 135
        changed, _ = snapshot(e, raw(source), now=NOW + dt.timedelta(hours=3))
        self.assertFalse(s.compare(unknown, changed)["quote_publication_allowed"])
        self.assertIn("/selected/product/additionalProperty/value",
                      [x["path"] for x in s.compare(unknown, changed)["offers"][0]["changes"]])

    def test_missing_null_type_and_json_pointer_paths_are_distinct(self):
        p = product()
        p["finish/a~b"] = None
        old, _ = snapshot(source=raw(p))
        del p["finish/a~b"]
        new, _ = snapshot(source=raw(p))
        change = s.compare(old, new)["offers"][0]["changes"][0]
        self.assertEqual(change["path"], "/selected/product/finish~1a~0b")
        self.assertTrue(change["before_present"])
        self.assertFalse(change["after_present"])
        self.assertTrue(s._changes(True, 1))
        self.assertTrue(s._changes([{"value": True}], [{"value": 1}]))

    def test_repeated_jsonld_references_have_a_total_expansion_budget(self):
        e, source = jsonld_fixture()
        source["@graph"][0]["additionalProperty"] = {"@id": "branch0"}
        for index in range(6):
            value = "terminal" if index == 5 else [{"@id": f"branch{index + 1}"}] * 5
            source["@graph"].append({"@id": f"branch{index}", "@type": "PropertyValue", "value": value})
        self.assertLess(len(raw(source)), 4096)
        with self.assertRaisesRegex(ValueError, "expansion exceeds"):
            snapshot(e, raw(source))

    def test_material_replacement_unknown_binding_and_removed_offers_require_review(self):
        old, _ = snapshot()
        for e in (entry(), dict(locked(), material_sha256="cd" * 32),
                  dict(locked(), material_id="different/material")):
            with self.subTest(entry=e):
                new, _ = snapshot(e)
                self.assertFalse(s.compare(old, new)["quote_publication_allowed"])
        unbound, _ = snapshot(entry())
        self.assertFalse(s.compare(unbound, unbound)["quote_publication_allowed"])
        other, _ = snapshot(dict(locked(), offer_id="other/42"))
        self.assertEqual({x["status"] for x in s.compare(old, other)["offers"]}, {"added", "removed"})

    def test_older_source_cannot_replace_current_revision(self):
        old, _ = snapshot()
        new, _ = snapshot(now=NOW - dt.timedelta(hours=1))
        with self.assertRaisesRegex(ValueError, "predates"):
            s.compare(old, new)

    def test_baseline_replays_evidence_and_refuses_edited_facts_or_source(self):
        baseline, evidence = snapshot()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, data in evidence.items():
                (root / name).write_bytes(data)
            self.assertEqual(s.replay(baseline, root), baseline)
            edited = copy.deepcopy(baseline)
            edited["observations"][0]["facts"]["binding"]["material_sha256"] = "cd" * 32
            with self.assertRaisesRegex(ValueError, "differs"):
                s.replay(edited, root)
            source = baseline["observations"][0]["source_sha256"] + ".json"
            (root / source).write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "source bytes"):
                s.replay(baseline, root)

    def test_changed_retained_receipt_and_path_like_hashes_refuse(self):
        baseline, evidence = snapshot()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, data in evidence.items():
                (root / name).write_bytes(data)
            receipt = baseline["observations"][0]["receipt_sha256"] + ".receipt.json"
            (root / receipt).write_bytes(b"{}")
            with self.assertRaisesRegex(ValueError, "receipt"):
                s.replay(baseline, root)
        baseline["observations"][0]["receipt_sha256"] = "../source"
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            s.replay(baseline, Path("."))

    def test_capture_requires_complete_exact_receipts(self):
        e = locked()
        book, evidence = r.refresh({"schema": "ftx-textile-refresh/1", "offers": [e]},
                                  lambda url, **_: CART if url.endswith("cart.js") else raw(product()), lambda: NOW)
        self.assertEqual(len(s.capture(book, evidence)["observations"]), 1)
        receipts = {k: v for k, v in evidence.items() if not k.endswith(".receipt.json")}
        with self.assertRaisesRegex(ValueError, "every exact offer"):
            s.capture(book, receipts)


class SpecificationCliTests(unittest.TestCase):
    def run_cli(self, root, source, suffix, comparison=False, intraday=False):
        manifest = root / "manifest.json"
        if not manifest.exists():
            manifest.write_bytes(raw({"schema": "ftx-textile-refresh/1", "offers": [locked()]}))
        args = ["refresh", str(manifest), "--out", str(root / (suffix + "-quotes.json")),
                "--evidence", str(root / "evidence"), "--specifications-out", str(root / (suffix + "-spec.json"))]
        if comparison:
            args.extend(["--compare-specifications", str(root / "old-spec.json"),
                         "--review-out", str(root / (suffix + "-review.json"))])
        if intraday:
            args.append("--intraday")
        original = r.refresh
        with patch("sys.argv", args), patch.object(r.PublicSession, "get", side_effect=lambda url, **_:
                CART if url.endswith("cart.js") else source), patch.object(r, "refresh",
                side_effect=lambda manifest, get, **kwargs: original(manifest, get, lambda: NOW, **kwargs)):
            return r.main()

    def test_changed_spec_saves_review_and_evidence_without_publishing_quotes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertEqual(self.run_cli(root, raw(product()), "old"), 0)
            retained = {p: p.read_bytes() for p in root.rglob("*") if p.is_file()}
            p = product()
            p["description"] += " Newly coated, 135 cm nominal width."
            with self.assertRaises(SystemExit) as result:
                self.run_cli(root, raw(p), "new", True)
            self.assertEqual(result.exception.code, 2)
            self.assertFalse((root / "new-quotes.json").exists())
            review = r.load((root / "new-review.json").read_bytes())
            self.assertFalse(review["quote_publication_allowed"])
            self.assertEqual(review["offers"][0]["status"], "changed")
            s.replay(r.load((root / "new-spec.json").read_bytes()), root / "evidence")
            for path, data in retained.items():
                self.assertEqual(path.read_bytes(), data)

    def test_unchanged_specs_allow_new_unavailable_quote_and_preserve_old_price(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.run_cli(root, raw(product()), "old")
            before = (root / "old-quotes.json").read_bytes()
            p = product()
            p["variants"][1].update(price=1600, available=False)
            self.assertEqual(self.run_cli(root, raw(p), "new", True), 0)
            new = r.load((root / "new-quotes.json").read_bytes())["quotes"][0]
            self.assertEqual(new["price"], 16)
            self.assertFalse(new["available"])
            self.assertEqual((root / "old-quotes.json").read_bytes(), before)
            self.assertTrue(r.load((root / "new-review.json").read_bytes())["quote_publication_allowed"])

    def test_intraday_source_expiry_replays_without_becoming_material_revision(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            e, source = jsonld_fixture()
            (root / "manifest.json").write_bytes(raw({"schema": "ftx-textile-refresh/1", "offers": [e]}))
            self.assertEqual(self.run_cli(root, raw(source), "old"), 0)
            previous = (root / "old-quotes.json").read_bytes()
            source["@graph"][1].update(validThrough="2026-10-13T16:00:00.123456Z", price="16.00",
                                        availability="https://schema.org/OutOfStock")
            self.assertEqual(self.run_cli(root, raw(source), "new", comparison=True, intraday=True), 0)
            book = r.load((root / "new-quotes.json").read_bytes())
            self.assertEqual(book["schema"], r.INTRADAY_SCHEMA)
            q = book["quotes"][0]
            self.assertEqual(q["observed_at"], "2026-10-13T12:00:00.000000Z")
            self.assertEqual(q["valid_through_at"], "2026-10-13T16:00:00.123456Z")
            self.assertFalse(q["available"])
            self.assertEqual(q["material_sha256"], e["material_sha256"])
            review = r.load((root / "new-review.json").read_bytes())
            self.assertTrue(review["quote_publication_allowed"])
            self.assertEqual(review["offers"][0]["status"], "unchanged")
            s.replay(r.load((root / "new-spec.json").read_bytes()), root / "evidence")
            self.assertEqual((root / "old-quotes.json").read_bytes(), previous)

    def test_jsonld_depletion_publishes_zero_capacity_and_retains_the_existing_binding(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            e, source = jsonld_fixture()
            source["@graph"][1]["inventoryLevel"] = {
                "@type": "QuantitativeValue", "value": 1.25, "unitCode": "YRD"}
            (root / "manifest.json").write_bytes(raw({"schema": "ftx-textile-refresh/1", "offers": [e]}))
            self.assertEqual(self.run_cli(root, raw(source), "old", intraday=True), 0)
            previous = (root / "old-quotes.json").read_bytes()
            source["@graph"][1]["inventoryLevel"]["value"] = 0
            source["@graph"][1].update(price="16.00", availability="https://schema.org/OutOfStock")
            self.assertEqual(self.run_cli(root, raw(source), "new", comparison=True, intraday=True), 0)
            book = r.load((root / "new-quotes.json").read_bytes())
            self.assertEqual(len(book["quotes"]), 1)
            quote = book["quotes"][0]
            self.assertEqual(quote["available_quantity"], 0.0)
            self.assertFalse(quote["available"])
            self.assertEqual((quote["quantity"], quote["unit"]), (0.5, "linear_yard"))
            self.assertEqual(quote["material_sha256"], e["material_sha256"])
            self.assertNotEqual(quote["source_sha256"], r.load(previous)["quotes"][0]["source_sha256"])
            self.assertEqual((root / "old-quotes.json").read_bytes(), previous)
            self.assertTrue(r.load((root / "new-review.json").read_bytes())["quote_publication_allowed"])
            s.replay(r.load((root / "old-spec.json").read_bytes()), root / "evidence")
            s.replay(r.load((root / "new-spec.json").read_bytes()), root / "evidence")
            source["@graph"][0]["description"] += " Newly coated textile."
            with self.assertRaises(SystemExit) as result:
                self.run_cli(root, raw(source), "changed", comparison=True, intraday=True)
            self.assertEqual(result.exception.code, 2)
            self.assertFalse((root / "changed-quotes.json").exists())


if __name__ == "__main__":
    unittest.main()
