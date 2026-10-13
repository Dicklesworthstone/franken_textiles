#!/usr/bin/env python3
"""Capture the reference's measurement evidence model (docs/spec/02 Part A, no photo code) as oracle fixtures.

Writes, from the reference's measurements.py (the fabric-pattern skill's scripts):
    OUT_DIR/priors.json     chart priors across statures, ages and sexes; the CDC stature table and its inverse;
                            reference wearers
    OUT_DIR/fusion.json     whole-document fusion of synthetic evidence (repeats, a z > 2 disagreement, a z > 3
                            conflict, photo and garment-derived sources, angles, aliases, a definition mismatch,
                            priors on and off, statures outside the charts) and the schema checks on malformed files
    OUT_DIR/readiness.json  readiness of those documents for every garment kind and option, fresh and stale

All wearers are synthetic (chart priors or invented numbers); none is a real child. The reference is identified
by the SHA-256 of its file, not by a path. Usage:
    python capture_measurements.py --reference SKILL/scripts OUT_DIR
Needs the reference's requirements. Deterministic.
"""

from __future__ import annotations

import argparse
import copy
import datetime as dt
import hashlib
import json
import platform
import sys
from pathlib import Path

SCHEMA = "ftx-oracle/1"
FILES = ("measurements.py",)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--reference", required=True, type=Path)
    ap.add_argument("out_dir", type=Path)
    args = ap.parse_args()
    sys.path.insert(0, str(args.reference.resolve()))
    import measurements as M

    meta = {"reference_files": {f: hashlib.sha256((args.reference / f).read_bytes()).hexdigest() for f in FILES},
            "python_version": platform.python_version(), "machine": platform.machine()}

    def attempt(fn, *a, **kw):
        try:
            return {"value": fn(*a, **kw)}
        except (ValueError, KeyError, TypeError) as exc:
            return {"error": type(exc).__name__, "message": str(exc)}

    # ------------------------------------------------------------------ priors
    pcases = []
    for st in (800.0, 860.0, 920.0, 1000.0, 1080.0, 1160.0, 1280.0):
        for age in (None, 24.0, 48.0):
            for sex in (None, "M", "F"):
                pcases.append({"id": f"priors-{st:g}-{age}-{sex}", "kind": "priors_for",
                               "input": {"stature_mm": st, "age_months": age, "sex": sex},
                               "output": attempt(M.priors_for, st, age, sex)})
    for st in (700.0, 1400.0):
        pcases.append({"id": f"priors-{st:g}", "kind": "priors_for", "input": {"stature_mm": st, "age_months": None, "sex": None},
                       "output": attempt(M.priors_for, st, None, None)})
    for age in (20.0, 24.5, 27.0, 30.5, 36.5, 42.0, 48.5, 60.5, 66.5, 72.5, 80.0):
        for sex in (None, "M", "F"):
            pcases.append({"id": f"cdc-{age:g}-{sex}", "kind": "cdc_stature", "input": {"age_months": age, "sex": sex},
                           "output": attempt(M.cdc_stature, age, sex)})
    for st in range(750, 1350, 50):
        pcases.append({"id": f"age-{st}", "kind": "age_from_stature", "input": {"stature_mm": float(st)},
                       "output": attempt(M.age_from_stature, float(st))})
    for st, age, sex in ((800.0, 13.0, None), (920.0, 30.0, None), (1160.0, 72.0, None), (1000.0, None, "F"),
                         (1280.0, 96.0, "M")):
        pcases.append({"id": f"reference-{st:g}-{age}-{sex}", "kind": "reference_wearer",
                       "input": {"stature_mm": st, "age_months": age, "sex": sex},
                       "output": attempt(M.reference_wearer, st, age, sex)})

    # ------------------------------------------------------------------ documents
    rec = M.make_record
    docs = {}
    d = M.new_measurement_file("synthetic A", 30.0, None, 920.0, taken_on="2026-10-01")
    d["records"] += [
        rec("chest", 548.0, 5.0, "MEASURED_TAPE", "tape over vest"),
        rec("chest", 552.0, 5.0, "MEASURED_TAPE", "tape over vest"),          # a repeat: correlated
        rec("waist", 600.0, 8.0, "MEASURED_TAPE", "tape at navel"),           # z > 2 against the prior
        rec("hip", 760.0, 5.0, "MEASURED_TAPE", "tape at seat"),              # z > 3: conflict
        rec("upper_arm", 160.0, 12.0, "PHOTO_ESTIMATED", "photo, plain sheet"),
        rec("arm_length", 300.0, 18.0, "GARMENT_DERIVED", "sleeve of a fitting jacket"),
        rec("shoulder_slope", 21.0, 2.0, "MEASURED_TAPE", "inclinometer app"),  # an alias, degrees
        rec("bust", 545.0, 6.0, "SCAN_MEASURED", "phone scan"),               # alias of chest
        rec("neck_base", 270.0, 6.0, "MEASURED_TAPE", "tape", definition_id="SOMETHING:else"),
        rec("head", 495.0, 4.0, "MEASURED_TAPE", "tape"),
        rec("back_neck_to_waist", 215.0, 5.0, "MEASURED_TAPE", "tape"),
        rec("across_back", 222.0, 6.0, "MEASURED_TAPE", "tape"),
        rec("shoulder_length", 72.0, 4.0, "MEASURED_TAPE", "tape"),
    ]
    docs["synthetic-a"] = d
    d = M.new_measurement_file("synthetic B", None, "F", 1040.0, taken_on="2026-08-01", nappies=True)
    d["records"] += [rec(n, v, s, "MEASURED_TAPE", "tape") for n, v, s in (
        ("waist", 520.0, 5.0), ("hip", 590.0, 5.0), ("body_rise", 175.0, 6.0), ("outseam", 600.0, 6.0),
        ("total_crotch_length", 430.0, 8.0), ("seat_over_nappy", 640.0, 8.0), ("calf", 220.0, 4.0),
        ("foot_pass", 300.0, 6.0), ("knee", 230.0, 4.0), ("thigh", 320.0, 5.0))]
    docs["synthetic-b"] = d
    d = M.new_measurement_file("tall", 120.0, None, 1400.0, taken_on="2026-10-01")    # beyond the charts
    d["records"] += [rec("chest", 680.0, 5.0, "MEASURED_TAPE", "tape"), rec("head", 530.0, 5.0, "MEASURED_TAPE", "tape")]
    docs["outside-charts"] = d
    docs["reference-920"] = M.reference_wearer(920.0, 30.0)
    docs["empty"] = M.new_measurement_file("nothing yet", 30.0, None, None)

    fcases = []
    for did, doc in docs.items():
        for use in (True, False):
            fcases.append({"id": f"fuse-{did}-{use}", "kind": "fuse", "input": {"data": doc, "use_priors": use},
                           "output": attempt(M.fuse, copy.deepcopy(doc), use)})
    bad = {
        "not-an-object": [1, 2],
        "wrong-schema": {"schema": "fabric-measurements/0", "records": []},
        "bad-record": {"schema": M.SCHEMA, "wearer": {}, "records": [{"name": "chest", "value_mm": "55", "sigma_mm": -1,
                                                                        "provenance": "GUESS"}]},
        "unknown-name": {"schema": M.SCHEMA, "wearer": {}, "records": [{"name": "elbow_girth", "value_mm": 150.0,
                                                                          "sigma_mm": 5.0, "provenance": "MEASURED_TAPE"}]},
        "bad-wearer": {"schema": M.SCHEMA, "wearer": {"age_months": "two"}, "records": [{"name": "chest", "value_mm": None,
                                                                                            "sigma_mm": 5.0, "provenance": "MEASURED_TAPE"}]},
    }
    for bid, doc in bad.items():
        fcases.append({"id": f"validate-{bid}", "kind": "validate", "input": {"data": doc}, "output": attempt(M.validate, doc)})
    for did in ("synthetic-a", "reference-920"):
        fcases.append({"id": f"validate-{did}", "kind": "validate", "input": {"data": docs[did]},
                       "output": attempt(M.validate, docs[did])})

    # readiness cases name their document (the documents are stored once, under "docs")
    rcases = []
    kinds = ("bodice", "jacket", "set_in_sleeve", "trousers", "full_trousers_gathered", "hat_fez")
    options = [{}, {"passes_hip": True}, {"closed_cuff": True}, {"nappies": True}, {"nappies": False},
               {"elastic_waist": True}, {"cuff_level": "knee"}, {"cuff_level": "ankle", "cuff_opening": True},
               {"use_priors": False}]
    runs = [(did, kind, opt, "2026-10-02") for did in docs for kind in kinds for opt in options]
    # staleness: taken 2026-10-01, stale after 42 days
    runs += [("synthetic-a", kind, {}, day) for kind in kinds for day in ("2026-11-12", "2026-11-13")]
    runs += [("synthetic-a", "kilt", {}, "2026-10-02"), ("synthetic-b", "full_trousers_gathered", {"cuff_level": "hip"},
                                                          "2026-10-02")]
    for did, kind, opt, as_of in runs:
        rcases.append({"id": f"ready-{did}-{kind}-{json.dumps(opt, sort_keys=True)}-{as_of}", "kind": "readiness",
                       "input": {"doc": did, "garment": kind, "options": opt, "as_of": as_of},
                       "output": attempt(M.readiness, copy.deepcopy(docs[did]), kind,
                                         as_of=dt.date.fromisoformat(as_of), **opt)})

    args.out_dir.mkdir(parents=True, exist_ok=True)
    for name, cases in (("priors", pcases), ("fusion", fcases), ("readiness", rcases)):
        path = args.out_dir / f"{name}.json"
        body = {"schema": SCHEMA, "subsystem": "measure", "fixture": name, "meta": meta, "cases": cases}
        if name == "readiness":
            body["docs"] = docs
        path.write_text(json.dumps(body, separators=(",", ":"), allow_nan=True) + "\n")
        errors = sum("error" in c["output"] for c in cases)
        print(f"wrote {len(cases)} cases ({errors} errors) to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
