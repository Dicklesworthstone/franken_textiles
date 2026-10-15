#!/usr/bin/env python3
"""Import identified Wintherm conductivity runs into the shared textile catalog.

Offline, standard-library only. A manifest locks exact source bytes and specimen names;
there is no fiber mixing rule, inferred density, heat capacity, or simulation permission.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
from dataclasses import dataclass
import datetime as dt
from decimal import Decimal, InvalidOperation, ROUND_HALF_EVEN
import hashlib
import json
import math
from pathlib import Path
import re
import statistics
import sys

MAX_SOURCE = 8 * 1024 * 1024
MAX_TOTAL = 32 * 1024 * 1024
MAX_MANIFEST = 1024 * 1024
MAX_RUNS = 128
MAX_ROWS = 4096
FORMS = {"sheet", "woven", "knit", "nonwoven", "hide", "fill", "laminate"}
NUMBER = r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?"


def fail(message: str):
    raise ValueError(message)


def keys(value: dict, expected: set[str], name: str) -> None:
    if not isinstance(value, dict) or set(value) != expected:
        fail(f"{name}: expected exactly {sorted(expected)}")


def text(value, name: str) -> str:
    if not isinstance(value, str) or not value.strip() or len(value) > 8192:
        fail(f"{name}: nonempty bounded text required")
    return value


def finite(value, name: str, *, positive=False) -> float:
    if type(value) not in (int, float) or not math.isfinite(value):
        fail(f"{name}: finite number required")
    if positive and value <= 0:
        fail(f"{name}: positive number required")
    return float(value)


def load_json(raw: bytes) -> dict:
    if len(raw) > MAX_MANIFEST:
        fail("manifest exceeds 1 MiB")

    def pairs(items):
        out = {}
        for key, value in items:
            if key in out:
                fail(f"duplicate JSON key: {key}")
            out[key] = value
        return out

    try:
        return json.loads(raw, object_pairs_hook=pairs,
                          parse_constant=lambda value: fail(f"nonfinite JSON: {value}"))
    except (UnicodeError, RecursionError) as error:
        raise ValueError("invalid manifest encoding or nesting") from error


def decode_source(raw: bytes) -> str:
    """Wintherm UTF-16 with BOM, or an explicitly retained UTF-8 transcription."""
    if not raw or len(raw) > MAX_SOURCE:
        fail("HFM source must contain 1 byte..8 MiB")
    encoding = "utf-16" if raw.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8-sig"
    try:
        decoded = raw.decode(encoding)
    except UnicodeError as error:
        raise ValueError("HFM source must be BOM-marked UTF-16 or UTF-8") from error
    if "\x00" in decoded:
        fail("NUL in HFM text; unmarked UTF-16 is not guessed")
    return decoded


def field(lines: list[str], name: str) -> str:
    matches = [line.split(":", 1)[1].strip() for line in lines
               if re.match(rf"^\s*{re.escape(name)}\s*:", line)]
    if len(matches) != 1 or not matches[0]:
        fail(f"HFM needs exactly one {name} header")
    return matches[0]


def decimal(token: str, name: str) -> Decimal:
    if len(token) > 64 or not re.fullmatch(NUMBER, token):
        fail(f"invalid {name}: {token!r}")
    try:
        value = Decimal(token)
        if not value.is_finite() or not math.isfinite(float(value)):
            fail(f"nonfinite {name}")
        # Bound exponents before arithmetic/quantization; finite subnormals are not useful data.
        if value and not -100 <= value.adjusted() <= 100:
            fail(f"unsupported magnitude for {name}")
        return value
    except InvalidOperation as error:
        raise ValueError(f"invalid {name}") from error


@dataclass(frozen=True)
class ConductivityRun:
    """One instrument run, not a collection of independent replicate coupons."""
    sample_name: str
    thickness_mm: float
    thickness_method: str
    instrument: str
    instrument_serial: str
    # Preserve source order, all repeated setpoints, and all four printed columns.
    rows: tuple[tuple[float, float, float, float], ...]


def parse_conductivity(raw: bytes) -> ConductivityRun:
    lines = decode_source(raw).splitlines()
    markers = [i for i, line in enumerate(lines) if line.strip() == "Results Table -- SI Units"]
    if len(markers) != 1:
        fail("HFM needs exactly one complete SI results table")
    before, after = lines[:markers[0]], lines[markers[0] + 1:]
    if any(re.search(r"^\s*\**\s*(?:warning|error|aborted)\b|setpoint was not reached", line, re.I)
           for line in before):
        fail("instrument reports a warning/error or unreached setpoint; qualify the run before use")
    sample = field(before, "Sample Name")
    instrument = field(before, "Instrument")
    serial = field(before, "Instrument Serial Number")
    thickness = re.fullmatch(rf"({NUMBER})\s*mm", field(before, "Thickness"))
    if thickness is None:
        fail("instrument thickness must explicitly use mm")
    mm = float(decimal(thickness[1], "thickness"))
    finite(mm, "instrument thickness", positive=True)
    method = field(before, "Thickness obtained")
    if method.casefold() != "from instrument":
        fail("this importer requires instrument-reported thickness, not an unqualified manual input")
    count_text = field(before, "Number of Setpoints")
    if not re.fullmatch(r"[0-9]{1,4}", count_text):
        fail("invalid setpoint count")
    count = int(count_text)
    if not 2 <= count <= MAX_ROWS:
        fail("conductivity needs 2..4096 setpoints")
    table = [line.strip() for line in after if line.strip()]
    if not table or re.sub(r"\s+", " ", table[0]) != "Mean Temp Upper Cond Lower Cond Average Cond":
        fail("expected conductivity columns, not volumetric heat capacity or an unknown table")
    rows = []
    for line in table[1:]:
        cells = line.split()
        if len(cells) != 4:
            fail("every conductivity result needs exactly four numeric columns")
        values = [decimal(cell, "conductivity result") for cell in cells]
        if values[0] < Decimal("-273.15") or any(x <= 0 for x in values[1:]):
            fail("nonphysical conductivity observation")
        # Each printed column may have rounded its own underlying value. Check consistency using
        # only that printed precision, not an invented physical or instrument-error tolerance.
        rounding = [Decimal(10) ** x.as_tuple().exponent / 2 for x in values[1:]]
        allowed = rounding[2] + (rounding[0] + rounding[1]) / 2
        if abs(values[3] - (values[1] + values[2]) / 2) > allowed:
            fail("Average Cond conflicts with the printed upper/lower conductivity")
        rows.append(tuple(float(x) for x in values))
        if len(rows) > MAX_ROWS:
            fail("too many conductivity rows")
    if len(rows) != count:
        fail(f"truncated/extra HFM results: {count} setpoints but {len(rows)} rows")
    return ConductivityRun(sample, mm, method, instrument, serial, tuple(rows))


def grouped(run: ConductivityRun, tolerance_c: float) -> list[dict]:
    """Explicit nominal-degree grouping, retaining within-run counts and actual temperatures."""
    tolerance = finite(tolerance_c, "maximum_temperature_deviation_c")
    if not 0 <= tolerance < 0.5:
        fail("nominal-temperature grouping tolerance must be in [0, 0.5) C")
    groups = defaultdict(list)
    for index, row in enumerate(run.rows):
        nominal = float(Decimal(str(row[0])).quantize(Decimal("1"), rounding=ROUND_HALF_EVEN))
        if abs(row[0] - nominal) > tolerance + 1e-12:
            fail("actual temperature exceeds the explicitly admitted nominal-temperature tolerance")
        groups[nominal].append((index, row))
    if len(groups) < 2:
        fail("a bounded thermal series needs at least two distinct nominal temperatures")
    out = []
    for temperature, observations in sorted(groups.items()):
        values = [row[3] for _, row in observations]
        out.append({"temperature_c": temperature,
                    "mean": math.fsum(values) / len(values),
                    "standard_deviation": statistics.stdev(values) if len(values) > 1 else None,
                    "reading_count": len(values), "run_count": 1,
                    "source_row_indices": [index for index, _ in observations],
                    "actual_temperatures_c": [row[0] for _, row in observations]})
    return out


def convert(entry: dict, raw: bytes) -> tuple[dict, dict]:
    """Return a canonical Material and an acquisition receipt. Never mutate another specimen."""
    keys(entry, {"id", "name", "form", "path", "sha256", "source", "sample_name",
                 "conditioning", "temperature_grouping", "retention_note"}, "run entry")
    for key in ("id", "name", "path", "sample_name", "conditioning", "retention_note"):
        text(entry[key], key)
    if not isinstance(entry["form"], str) or entry["form"] not in FORMS:
        fail("HFM material must be a sheet/fill construction, not fiber or yarn")
    source = entry["source"]
    keys(source, {"reference", "accessed_on", "note"}, "source")
    for key in source:
        text(source[key], f"source.{key}")
    dt.date.fromisoformat(source["accessed_on"])
    if not re.fullmatch(r"[0-9a-f]{64}", str(entry["sha256"])):
        fail("source SHA-256 must be 64 lowercase hexadecimal characters")
    digest = hashlib.sha256(raw).hexdigest()
    if digest != entry["sha256"]:
        fail("source bytes differ from the reviewed manifest SHA-256")
    policy = entry["temperature_grouping"]
    keys(policy, {"method", "maximum_deviation_c"}, "temperature grouping")
    if policy["method"] != "within_run_nearest_integer_c":
        fail("explicit within_run_nearest_integer_c grouping consent required")
    run = parse_conductivity(raw)
    if run.sample_name != entry["sample_name"]:
        fail("instrument Sample Name differs from the exact reviewed specimen identity")
    observations = grouped(run, policy["maximum_deviation_c"])
    conditions = {"specimen": run.sample_name, "conditioning": entry["conditioning"],
                  "instrument": run.instrument, "instrument_serial": run.instrument_serial,
                  "source_sha256": digest,
                  "scope": "One HFM run; pressure, ply count and lot linkage are not established by this import"}
    material = {
        "id": entry["id"], "name": entry["name"], "form": entry["form"],
        "composition": [], "composition_complete": False,
        "structure": {
            "identity_scope": "Exact instrument run, not generic fiber or pooled conditioning group",
            "raw_source_sha256": digest,
            "retention_note": entry["retention_note"],
            "uncertainty_scope": "Within-run repeated-reading sample SD, not independent specimen SD, uncertainty of mean or confidence interval",
            "missing_characterization": "Matched mass/area, specific heat, ply count, pressure, lot, mechanical coefficients and physical validation",
        },
        "properties": {"thickness": {
            "value": run.thickness_mm, "unit": "mm", "evidence": "measured", "source": "hfm",
            "method": f"Instrument-reported full specimen thickness; {run.thickness_method}. Not inferred from density or another run",
            "conditions": conditions, "bounds": None}},
        "thermal_response": {
            "specimen": run.sample_name, "conditioning": entry["conditioning"],
            "conductivity": {
                "unit": "W/(m*K)", "evidence": "measured", "source": "hfm",
                "method": "Arithmetic mean of printed Average Cond within this run at explicitly grouped nearest-integer temperatures; interpolation remains an estimate",
                "conditions": {**conditions,
                    "temperature_grouping": json.dumps(policy, sort_keys=True, separators=(",", ":")),
                    "reading_counts_by_temperature": json.dumps({str(p["temperature_c"]): p["reading_count"] for p in observations}, sort_keys=True),
                    "variability": "Within-run sample SD (n-1); repeated setpoints are not independent specimen replicates"},
                "points": [{key: p[key] for key in ("temperature_c", "mean", "standard_deviation")}
                           for p in observations]}},
        "sources": {"hfm": source},
    }
    receipt = {"material_id": entry["id"], "source": source, "source_sha256": digest,
               "source_bytes": len(raw), "sample_name": run.sample_name,
               "instrument": run.instrument, "instrument_serial": run.instrument_serial,
               "thickness_mm": run.thickness_mm, "thickness_method": run.thickness_method,
               "columns": ["Mean Temp (C)", "Upper Cond (W/mK)", "Lower Cond (W/mK)", "Average Cond (W/mK)"],
               "rows": run.rows, "groups": observations, "manifest_entry": entry,
               "transient_ready": False, "mechanics_ready": False,
               "physical_validation_claimed": False}
    return material, receipt


def prepare(manifest: dict, base: Path) -> tuple[dict, dict, dict[str, bytes]]:
    """Validate all sources before creating any output directory; no symlink/path escape."""
    keys(manifest, {"schema", "runs"}, "HFM manifest")
    if manifest["schema"] != "ftx-hfm-import/1":
        fail("unsupported HFM manifest schema")
    entries = manifest["runs"]
    if not isinstance(entries, list) or not 1 <= len(entries) <= MAX_RUNS:
        fail("manifest needs 1..128 runs")
    base = base.resolve(strict=True)
    materials, receipts, evidence = [], [], {}
    ids, samples, paths = set(), set(), set()
    total = 0
    for entry in entries:
        if not isinstance(entry, dict):
            fail("run entry must be an object")
        relative = Path(text(entry.get("path"), "path"))
        path = (base / relative).resolve(strict=True)
        if relative.is_absolute() or not path.is_relative_to(base) or not path.is_file():
            fail("source must be a file within the manifest directory")
        if path in paths:
            fail("duplicate source path is not another replicate")
        paths.add(path)
        with path.open("rb") as stream:
            raw = stream.read(MAX_SOURCE + 1)
        total += len(raw)
        if total > MAX_TOTAL:
            fail("source batch exceeds 32 MiB")
        material, receipt = convert(entry, raw)
        if material["id"] in ids or receipt["sample_name"] in samples or entry["sha256"] in evidence:
            fail("duplicate material, Sample Name or source bytes are not another replicate")
        ids.add(material["id"])
        samples.add(receipt["sample_name"])
        evidence[entry["sha256"]] = raw
        materials.append(material)
        receipts.append(receipt)
    # Catalog order never depends on filesystem enumeration or manifest order.
    materials.sort(key=lambda m: m["id"])
    receipts.sort(key=lambda r: r["material_id"])
    return ({"schema": "ftx-textile-catalog/1", "materials": materials},
            {"schema": "ftx-hfm-acquisition/1", "complete": True, "runs": receipts}, evidence)


def publish(manifest_path: Path, destination: Path) -> dict:
    with manifest_path.open("rb") as stream:
        manifest = load_json(stream.read(MAX_MANIFEST + 1))
    catalog, receipt, evidence = prepare(manifest, manifest_path.parent)
    # Exclusive directory creation is the publication boundary. Validation failures leave nothing;
    # I/O failures may leave an incomplete directory without the completion receipt, never replace it.
    destination.mkdir(exist_ok=False)
    (destination / "evidence").mkdir()
    for digest, raw in sorted(evidence.items()):
        with (destination / "evidence" / f"{digest}.tst").open("xb") as stream:
            stream.write(raw)
    for name, value in (("catalog.json", catalog), ("acquisition.json", receipt)):
        with (destination / name).open("x", encoding="utf-8") as stream:
            json.dump(value, stream, ensure_ascii=False, allow_nan=False, sort_keys=True, indent=2)
            stream.write("\n")
    return receipt


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--out", type=Path, required=True, help="new immutable output directory")
    args = parser.parse_args(argv)
    try:
        receipt = publish(args.manifest, args.out)
    except (ValueError, OSError) as error:
        print(f"HFM import refused: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"catalog": str(args.out / "catalog.json"), "runs": len(receipt["runs"]),
                      "mechanical_or_transient_readiness_claimed": False}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
