#!/usr/bin/env python3
"""Capture the reference's browser round (`webapp.py`: program_info, form_fields, run) as oracle fixtures.

The page's Pyodide worker calls exactly these three functions; `ftx-wasm` answers the same requests. Records,
with the clock fixed at 2026-10-13 12:00 (the date stamps tape files and judges staleness; the time stamps
exports):
    program_info   the programs, labels and style defaults
    form_fields    each program (and none: someone else's pattern), with and without nappies
    run            chart, tape and file bodies; all three programs; style overrides; someone else's pattern with
                   and without a fit spec; a BLOCKED pattern; an UNSCALED trace; refusals
For `run`: what the page shows (status, basis, validation, readiness, exports, table, solved, title, the key
files' names and labels) and the documents written (pattern, fit_spec, measurements, validation, readiness);
paths, sizes and export bytes are left out (exports have their own fixtures).

Writes OUT_DIR/webapp.json. Usage:
    python capture_webapp.py --reference SKILL/scripts OUT_DIR
Needs the reference's requirements. Deterministic.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import json
import platform
import sys
import tempfile
from pathlib import Path

SCHEMA = "ftx-oracle/1"
FILES = ("webapp.py", "fabric.py", "drafting.py", "measurements.py", "validate.py", "export.py", "pattern_model.py")
DOCS = ("pattern.json", "fit_spec.json", "measurements.json", "validation.json", "readiness.json")


class FixedDate(_dt.date):
    @classmethod
    def today(cls):
        return cls(2026, 10, 13)


class FixedDatetime(_dt.datetime):
    @classmethod
    def now(cls, tz=None):
        return cls(2026, 10, 13, 12, 0)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--reference", required=True, type=Path)
    ap.add_argument("out_dir", type=Path)
    args = ap.parse_args()
    ref = args.reference.resolve()
    sys.path.insert(0, str(ref))
    _dt.date, _dt.datetime = FixedDate, FixedDatetime
    import drafting
    import measurements as ms
    import webapp

    meta = {"reference_files": {f: hashlib.sha256((ref / f).read_bytes()).hexdigest() for f in FILES},
            "clock": "2026-10-13T12:00", "python_version": platform.python_version()}
    cases = [{"id": "program_info", "fn": "program_info", "input": {}, "output": webapp.program_info()}]
    for program in ("zouave_bolero", "zouave_trousers", "fez", None):
        for nappies in (False, True):
            kw = {"program": program, "stature_cm": 92.0, "age_months": 30.0, "nappies": nappies}
            cases.append({"id": f"form_fields-{program or 'pattern'}-{'nappies' if nappies else 'plain'}",
                          "fn": "form_fields", "input": kw, "output": webapp.form_fields(**kw)})
    cases.append({"id": "form_fields-trousers-104", "fn": "form_fields",
                  "input": {"program": "zouave_trousers", "stature_cm": 104.0, "age_months": None, "nappies": False},
                  "output": webapp.form_fields("zouave_trousers", 104.0, None, False)})

    reference = ms.reference_wearer(920.0, 30.0)
    bolero, bolero_spec = drafting.zouave_bolero(reference)
    fez, _ = drafting.fez(reference)
    broken = json.loads(json.dumps(bolero))
    broken["pieces"][0]["edges"][0]["to"] = "nowhere"
    trace = json.loads((Path(__file__).resolve().parents[2] / "tests/fixtures/oracle/trace/raster_trace.json")
                       .read_text())
    unscaled = next(c for c in trace["cases"] if c["id"] == "outlines")["output"]["pattern"]
    tape_bolero = {"chest": 54.0, "waist": 51.5, "back_neck_to_waist": 22.4, "neck_base": 25.5,
                   "across_back": 22.0, "shoulder_length": 7.2, "arm_length": 31.0, "upper_arm": 16.0,
                   "wrist": 11.2, "hand_pass": 13.0}
    requests = [
        ("chart-bolero", {"program": "zouave_bolero", "body": {"mode": "chart", "stature_cm": 92, "age_months": 30}}),
        ("chart-trousers-nappies", {"program": "zouave_trousers",
                                    "body": {"mode": "chart", "stature_cm": 100, "sex": "F", "nappies": True}}),
        ("chart-trousers-style", {"program": "zouave_trousers", "style": {"cuff_level": "ankle"},
                                  "body": {"mode": "chart", "stature_cm": 92, "age_months": 30}}),
        ("tape-fez", {"program": "fez",
                      "body": {"mode": "tape", "stature_cm": 92, "age_months": 30, "tape": {"head": 50.5}}}),
        ("tape-bolero", {"program": "zouave_bolero",
                         "body": {"mode": "tape", "stature_cm": 92, "age_months": 30, "sex": "M", "tape": tape_bolero}}),
        ("tape-bolero-few", {"program": "zouave_bolero",
                             "body": {"mode": "tape", "stature_cm": 92, "tape": {"chest": 54, "waist": "", "neck": None}}}),
        ("file-pattern-spec", {"pattern": bolero, "fit_spec": bolero_spec,
                               "body": {"mode": "file", "measurements": reference}}),
        ("file-pattern-default-spec", {"pattern": fez, "body": {"mode": "chart", "stature_cm": 92}}),
        ("blocked", {"pattern": broken, "body": {"mode": "chart", "stature_cm": 92}}),
        ("unscaled", {"pattern": unscaled, "body": {"mode": "chart", "stature_cm": 92}}),
        ("bad-program", {"program": "kilt", "body": {"mode": "chart", "stature_cm": 92}}),
        ("bad-stature", {"program": "fez", "body": {"mode": "chart", "stature_cm": 9}}),
        ("bad-tape-name", {"program": "fez", "body": {"mode": "tape", "stature_cm": 92, "tape": {"ankel": 14}}}),
        ("bad-tape-value", {"program": "fez", "body": {"mode": "tape", "stature_cm": 92, "tape": {"head": -3}}}),
        ("bad-file", {"program": "fez", "body": {"mode": "file", "measurements": {"schema": "nope"}}}),
    ]
    for cid, req in requests:
        with tempfile.TemporaryDirectory() as tmp:
            try:
                res = webapp.run(json.loads(json.dumps(req)), tmp)
            except (ValueError, KeyError) as exc:
                cases.append({"id": cid, "fn": "run", "input": req,
                              "output": {"error": type(exc).__name__, "message": str(exc)}})
                continue
            docs = {d: json.loads((Path(tmp) / d).read_text()) for d in DOCS if (Path(tmp) / d).exists()}
        for k in ("zip", "work"):
            res.pop(k, None)
        if "files" in res:
            res["files"] = [{"label": f["label"], "name": f["name"]} for f in res["files"]]
        if "preview_svg" in res:
            res["preview_svg"] = res["preview_svg"] is not None
        cases.append({"id": cid, "fn": "run", "input": req, "output": {"page": res, "documents": docs}})

    out = args.out_dir / "webapp.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"schema": SCHEMA, "subsystem": "web", "fixture": "webapp", "meta": meta,
                               "cases": cases}, separators=(",", ":")) + "\n")
    print(f"wrote {len(cases)} cases to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
