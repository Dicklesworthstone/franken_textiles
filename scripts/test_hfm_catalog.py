"""Replay retained source facts and refuse unqualified instrument reports."""
import json
from pathlib import Path
import unittest

from import_hfm_textiles import load_json, parse_conductivity, prepare
from test_import_hfm_textiles import SOURCE


class HfmSourceTests(unittest.TestCase):
    def test_instrument_warnings_and_manual_thickness_refuse(self):
        for changed in (
            SOURCE.replace("Results Table", "**** Warning: At least one setpoint was not reached! ****\nResults Table"),
            SOURCE.replace("Results Table", "Error: instrument failure\nResults Table"),
            SOURCE.replace("from instrument", "from user"),
        ):
            with self.assertRaises(ValueError):
                parse_conductivity(changed.encode())

    def test_pinned_fsri_transcriptions_reproduce_the_bundled_catalog(self):
        root = Path(__file__).resolve().parents[1]
        base = root / "tests/fixtures/materials/hfm"
        manifest_value = load_json((base / "fsri-manifest.json").read_bytes())
        catalog, receipt, _ = prepare(manifest_value, base)
        bundled = json.loads((root / "crates/ftx-pattern/src/material_catalog/hfm-runs.json").read_text())
        self.assertEqual(catalog, bundled)
        # Independent sums from the six printed results, not the importer's grouping function.
        dry, wet = catalog["materials"]
        self.assertAlmostEqual(dry["thermal_response"]["conductivity"]["points"][0]["mean"],
                               (0.02232 + 0.02276 + 0.02282) / 3, places=15)
        self.assertAlmostEqual(wet["thermal_response"]["conductivity"]["points"][1]["mean"],
                               (0.03076 + 0.03081 + 0.03078) / 3, places=15)
        self.assertEqual([m["properties"]["thickness"]["value"] for m in catalog["materials"]], [.30, .43])
        self.assertEqual(sum(len(r["rows"]) for r in receipt["runs"]), 12)
        self.assertTrue(all(g["reading_count"] == 3 and g["run_count"] == 1
                            for r in receipt["runs"] for g in r["groups"]))
        with self.assertRaisesRegex(ValueError, "warning/error"):
            parse_conductivity((base / "cotton-sheet-dry-r2-warning-excerpt.tst").read_bytes())



if __name__ == "__main__":
    unittest.main()
