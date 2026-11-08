"""Synthetic instrument-format regressions, not observations of real textiles."""
import copy
from decimal import localcontext
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from import_hfm_textiles import convert, decode_source, grouped, load_json, parse_conductivity, prepare, publish


SAMPLE = "synthetic_Cotton_HFM_Dry_conductivity_R1"
SOURCE = f"""Wintherm32v3 Version synthetic
Instrument: F200s
Instrument Serial Number: synthetic-1
Sample Name: {SAMPLE}
Thickness: 0.30mm
Thickness obtained    :  from instrument
Number of Setpoints: 4
Results Table -- SI Units

Mean Temp\tUpper Cond\tLower Cond\tAverage Cond
15.01\t0.02100\t0.01900\t0.02000
65.01\t0.03100\t0.02900\t0.03000
15.02\t0.02300\t0.02100\t0.02200
65.02\t0.03500\t0.03300\t0.03400
"""


def entry(raw=SOURCE.encode()):
    return {"id": "synthetic/hfm/cotton-r1", "name": "Synthetic instrument-format fixture",
            "form": "sheet", "path": "run.tst", "sha256": hashlib.sha256(raw).hexdigest(),
            "source": {"reference": "local:synthetic-hfm", "accessed_on": "2026-10-15",
                       "note": "Entirely synthetic; independent arithmetic expectations in tests"},
            "sample_name": SAMPLE, "conditioning": "synthetic dry protocol",
            "temperature_grouping": {"method": "within_run_nearest_integer_c", "maximum_deviation_c": 0.1},
            "retention_note": "Original synthetic test data; retain locally"}


def manifest(e=None):
    return {"schema": "ftx-hfm-import/1", "runs": [entry() if e is None else e]}


class HfmImportTests(unittest.TestCase):
    def test_utf16_endianness_bom_and_utf8(self):
        expected = parse_conductivity(SOURCE.encode())
        for raw in (SOURCE.encode("utf-16"), b"\xfe\xff" + SOURCE.encode("utf-16-be"),
                    SOURCE.encode("utf-8-sig")):
            self.assertEqual(expected, parse_conductivity(raw))
        for raw in (SOURCE.encode("utf-16-le"), b"\xff", b"", b"\x00bad"):
            with self.assertRaises(ValueError):
                decode_source(raw)

    def test_matched_thickness_units_rows_and_within_run_variability(self):
        material, receipt = convert(entry(), SOURCE.encode())
        self.assertEqual(material["properties"]["thickness"]["value"], 0.3)
        self.assertEqual(material["properties"]["thickness"]["unit"], "mm")
        response = material["thermal_response"]
        self.assertEqual(response["specimen"], SAMPLE)
        self.assertEqual(material["properties"]["thickness"]["conditions"]["specimen"], SAMPLE)
        points = response["conductivity"]["points"]
        self.assertEqual([p["temperature_c"] for p in points], [15, 65])
        self.assertAlmostEqual(points[0]["mean"], 0.021, places=15)
        self.assertAlmostEqual(points[1]["mean"], 0.032, places=15)
        self.assertAlmostEqual(points[0]["standard_deviation"], 0.002 / 2**0.5, places=15)
        self.assertEqual([g["reading_count"] for g in receipt["groups"]], [2, 2])
        self.assertEqual([g["run_count"] for g in receipt["groups"]], [1, 1])
        self.assertEqual(receipt["groups"][0]["source_row_indices"], [0, 2])
        self.assertEqual(receipt["groups"][0]["actual_temperatures_c"], [15.01, 15.02])
        self.assertEqual(len(receipt["rows"]), 4)
        self.assertNotIn("specific_heat", response)
        self.assertNotIn("areal_density", material["properties"])
        self.assertFalse(receipt["transient_ready"])
        self.assertFalse(receipt["physical_validation_claimed"])

    def test_source_hash_and_exact_sample_name_fail_closed(self):
        for change in ({"sha256": "0" * 64}, {"sample_name": "other lot"}, {"form": "yarn"},
                       {"form": "fiber"}, {"id": ""}, {"retention_note": ""}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                convert({**entry(), **change}, SOURCE.encode())
        e = entry()
        e["source"]["accessed_on"] = "2026-02-30"
        with self.assertRaises(ValueError):
            convert(e, SOURCE.encode())

    def test_ambiguous_headers_units_columns_nonfinite_and_truncation(self):
        changes = [
            SOURCE.replace("0.30mm", "0.30m"),
            SOURCE.replace("0.30mm", "0mm"),
            SOURCE.replace("Number of Setpoints: 4", "Number of Setpoints: 5"),
            SOURCE + "Results Table -- SI Units\n",
            SOURCE.replace("Instrument: F200s", "Instrument: F200s\nInstrument: F200s"),
            SOURCE.replace("Mean Temp\tUpper Cond\tLower Cond\tAverage Cond", "Mean Temp\tEnthalpy\tSpc Heat"),
            SOURCE.replace("0.02000", "0.09000"),
            SOURCE.replace("0.02000", "nan"), SOURCE.replace("0.02000", "1e999999"),
            SOURCE.replace("0.02000", "-0.02000"), SOURCE + "garbage\n",
            SOURCE.replace("15.01", "-274.01"),
        ]
        for changed in changes:
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                parse_conductivity(changed.encode())

    def test_printed_precision_not_physical_error_is_tolerance(self):
        source = SOURCE.replace("0.02100\t0.01900\t0.02000", "0.02101\t0.01900\t0.02000")
        parse_conductivity(source.encode())
        source = source.replace("0.02000", "0.02002")
        with self.assertRaises(ValueError):
            parse_conductivity(source.encode())

    def test_printed_consistency_is_exact_independently_of_decimal_context(self):
        matching = "0.02123456789012345678901234567890"
        conflicting = "0.02123456789012345678901234568000"
        for precision in (1, 6, 28, 80):
            with self.subTest(precision=precision), localcontext() as context:
                context.prec = precision
                for average in (matching, conflicting):
                    source = SOURCE.replace("0.02100\t0.01900\t0.02000",
                                            "\t".join((matching, matching, average)))
                    if average == matching:
                        self.assertEqual(parse_conductivity(source.encode()).rows[0][3], float(matching))
                    else:
                        with self.assertRaisesRegex(ValueError, "Average Cond conflicts"):
                            parse_conductivity(source.encode())

    def test_nominal_temperature_rounding_does_not_require_decimal_precision(self):
        large = SOURCE.replace("15.01", "1e30").replace("15.02", "1e30")
        for precision in (1, 6, 28, 80):
            with self.subTest(precision=precision), localcontext() as context:
                context.prec = precision
                ordinary = grouped(parse_conductivity(SOURCE.encode()), 0.1)
                self.assertEqual([p["temperature_c"] for p in ordinary], [15, 65])
                points = grouped(parse_conductivity(large.encode()), 0.1)
                self.assertEqual([p["temperature_c"] for p in points], [65, 1e30])
                self.assertEqual(points[1]["source_row_indices"], [0, 2])

    def test_explicit_temperature_grouping_and_missing_variability(self):
        for tolerance in (-1, 0.5, float("nan"), True, 0.001):
            with self.subTest(tolerance=tolerance), self.assertRaises(ValueError):
                grouped(parse_conductivity(SOURCE.encode()), tolerance)
        source = SOURCE.replace("Number of Setpoints: 4", "Number of Setpoints: 2")
        source = source[:source.index("15.02")]
        points = grouped(parse_conductivity(source.encode()), 0.1)
        self.assertTrue(all(p["standard_deviation"] is None for p in points))
        same_temperature = SOURCE.replace("65.01", "15.01").replace("65.02", "15.02")
        with self.assertRaises(ValueError):
            grouped(parse_conductivity(same_temperature.encode()), 0.1)

    def test_duplicate_json_fields_and_unknown_policy_fields(self):
        for raw in (b'{"runs":[],"runs":[]}', b'{"v":NaN}', b'{"v":Infinity}'):
            with self.assertRaises(ValueError):
                load_json(raw)
        e = entry()
        e["temperature_grouping"]["ignore_bad_readings"] = True
        with self.assertRaises(ValueError):
            convert(e, SOURCE.encode())

    def test_oversized_manifest_integers_refuse_without_traceback_or_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "run.tst").write_text(SOURCE)
            script = str(Path(__file__).with_name("import_hfm_textiles.py"))
            for index, value in enumerate((10**400, -(10**400))):
                e = entry()
                e["temperature_grouping"]["maximum_deviation_c"] = value
                with self.subTest(value=value), self.assertRaisesRegex(ValueError, "finite number required"):
                    convert(e, SOURCE.encode())
                (root / "manifest.json").write_text(json.dumps(manifest(e)))
                out = root / f"output-{index}"
                result = subprocess.run([sys.executable, script, str(root / "manifest.json"),
                                         "--out", str(out)], capture_output=True, text=True, check=False, timeout=10)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn("HFM import refused: maximum_temperature_deviation_c: finite number required", result.stderr)
                self.assertNotIn("Traceback", result.stderr)
                self.assertFalse(out.exists())

    def test_create_only_outputs_and_exact_evidence_replay(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            raw = SOURCE.encode("utf-16")
            (root / "run.tst").write_bytes(raw)
            (root / "manifest.json").write_text(json.dumps(manifest(entry(raw))))
            out = root / "output"
            receipt = publish(root / "manifest.json", out)
            saved = (out / "evidence" / f'{entry(raw)["sha256"]}.tst').read_bytes()
            self.assertEqual(raw, saved)
            self.assertEqual(receipt["runs"][0]["source_bytes"], len(raw))
            catalog = json.loads((out / "catalog.json").read_text())
            self.assertEqual(catalog["schema"], "ftx-textile-catalog/1")
            self.assertEqual(len(catalog["materials"]), 1)
            selection_path = out / "selections" / f'{entry(raw)["sha256"]}.json'
            selection_bytes = selection_path.read_bytes()
            self.assertEqual(json.loads(selection_bytes), {
                "id": entry(raw)["id"], "record": catalog["materials"][0],
                "allow_estimates": False,
            })
            self.assertTrue(json.loads((out / "acquisition.json").read_text())["complete"])
            before = (out / "catalog.json").read_bytes()
            with self.assertRaises(FileExistsError):
                publish(root / "manifest.json", out)
            self.assertEqual(before, (out / "catalog.json").read_bytes())
            self.assertEqual(selection_bytes, selection_path.read_bytes())

    def test_selection_paths_and_order_follow_exact_sources_not_specimen_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            other_raw = SOURCE.replace(SAMPLE, "synthetic-other-run").encode()
            first, second = entry(), entry(other_raw)
            second.update(id="synthetic/hfm/a/other", path="other.tst", sample_name="synthetic-other-run")
            (root / "run.tst").write_bytes(SOURCE.encode())
            (root / "other.tst").write_bytes(other_raw)
            for index, entries in enumerate(([first, second], [second, first])):
                (root / "manifest.json").write_text(json.dumps({"schema": "ftx-hfm-import/1", "runs": entries}))
                out = root / str(index)
                receipt = publish(root / "manifest.json", out)
                self.assertEqual([run["material_id"] for run in receipt["runs"]], [second["id"], first["id"]])
                self.assertEqual({p.name for p in (out / "selections").iterdir()},
                                 {f'{e["sha256"]}.json' for e in entries})
                for e in entries:
                    selected = json.loads((out / "selections" / f'{e["sha256"]}.json').read_text())
                    self.assertEqual(selected["id"], e["id"])
                    self.assertEqual(selected["record"]["thermal_response"]["specimen"], e["sample_name"])
                    self.assertEqual(selected["record"]["structure"]["raw_source_sha256"], e["sha256"])
            self.assertEqual({p.name: p.read_bytes() for p in (root / "0/selections").iterdir()},
                             {p.name: p.read_bytes() for p in (root / "1/selections").iterdir()})

    def test_duplicate_sources_and_traversal_are_not_replicates(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base = root / "batch"
            base.mkdir()
            (base / "run.tst").write_text(SOURCE)
            (root / "outside.tst").write_text(SOURCE)
            (base / "linked.tst").symlink_to(root / "outside.tst")
            e = entry()
            for path in ("../outside.tst", "linked.tst", str(root / "outside.tst")):
                with self.subTest(path=path), self.assertRaises(ValueError):
                    prepare(manifest({**e, "path": path}), base)
            with self.assertRaises(ValueError):
                prepare({"schema": "ftx-hfm-import/1", "runs": [e, copy.deepcopy(e)]}, base)
            (base / "run2.tst").write_text(SOURCE)
            with self.assertRaises(ValueError):
                prepare({"schema": "ftx-hfm-import/1", "runs": [e, {**e, "id": "other", "path": "run2.tst"}]}, base)

    def test_failed_import_creates_nothing_and_cli_runs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "run.tst").write_text(SOURCE)
            e = entry()
            e["sample_name"] = "wrong"
            (root / "manifest.json").write_text(json.dumps(manifest(e)))
            script = str(Path(__file__).with_name("import_hfm_textiles.py"))
            args = [sys.executable, script, str(root / "manifest.json"), "--out", str(root / "out")]
            result = subprocess.run(args, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertFalse((root / "out").exists())
            (root / "manifest.json").write_text(json.dumps(manifest()))
            result = subprocess.run(args, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(result.stdout)
            self.assertFalse(report["mechanical_or_transient_readiness_claimed"])
            self.assertEqual(report["selections"], [{"material_id": e["id"],
                "path": str(root / "out/selections" / f'{e["sha256"]}.json')}])
            self.assertEqual(json.loads(Path(report["selections"][0]["path"]).read_text())["id"], e["id"])


if __name__ == "__main__":
    unittest.main()
