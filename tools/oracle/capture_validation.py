#!/usr/bin/env python3
"""Capture validation oracle fixtures from the Python reference (docs/spec/01 §9, §14.2 F12, F22-F32, §14.4).

Every case is (pattern, fit spec, measurements) -> the reference's validation report and its text table, so
the Rust side compares validation alone; mutations the reference drafts (M2, M8-M10) arrive as patterns.
Cases: the nine drafted garments (bolero, trousers, fez for public reference wearers of 80, 92 and 116 cm),
the mutation tests M1-M12, the seam-walk variants of F12, truing F22-F25, points of measure F28-F32 (F30 with
the stubbed landmark heights recorded in the case), and role-based default fit specs.

    OUT_DIR/validation.json

Usage: python capture_validation.py --reference SKILL/scripts OUT_DIR   (deterministic; no real person)
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import platform
import sys
from pathlib import Path

import numpy as np

SCHEMA = "ftx-oracle/1"
FILES = ("pattern_model.py", "validate.py", "body.py", "drafting.py", "measurements.py")


def rect(pid, w=100.0, h=50.0, cut=None, grain=True):
    p = {"id": pid, "name": pid.upper(),
         "vertices": {"a": [0, 0], "b": [w, 0], "c": [w, h], "d": [0, h]},
         "edges": [{"id": "bottom", "from": "a", "to": "b"}, {"id": "right", "from": "b", "to": "c"},
                   {"id": "top", "from": "c", "to": "d"}, {"id": "left", "from": "d", "to": "a"}],
         "notches": [], "cut": cut if cut is not None else {"qty": 1}}
    if grain:
        p["grain"] = [[10, 10], [10, 40]]
    return p


def doc(pieces, seams=None):
    return {"schema": "fabric-pattern/1", "units": "mm", "pieces": pieces, "seams": seams or []}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--reference", required=True, type=Path)
    ap.add_argument("out_dir", type=Path)
    args = ap.parse_args(argv)
    for f in FILES:
        if not (args.reference / f).is_file():
            print(f"no {f} in {args.reference}", file=sys.stderr)
            return 2
    sys.path.insert(0, str(args.reference.resolve()))
    import body
    import drafting
    import measurements
    import validate
    from pattern_model import Pattern

    cases = []

    def add(cid, pattern, spec, meas, landmarks_stub=None):
        pattern, spec, meas = json.loads(json.dumps(pattern)), json.loads(json.dumps(spec)), json.loads(json.dumps(meas))
        saved = body.landmark_h
        if landmarks_stub is not None:
            body.landmark_h = lambda m, part, name: landmarks_stub[part][name]
        try:
            rep = validate.validate(Pattern(copy.deepcopy(pattern)), copy.deepcopy(spec), copy.deepcopy(meas))
        finally:
            body.landmark_h = saved
        rep = json.loads(json.dumps(rep))
        inp = {"pattern": pattern, "fit_spec": spec, "measurements": meas}
        if landmarks_stub is not None:
            inp["landmarks_stub"] = landmarks_stub
        cases.append({"id": cid, "input": inp, "output": {"report": rep, "table": validate.format_table(rep)}})

    wearers = {s: measurements.reference_wearer(s, m) for s, m in ((800.0, 13.0), (920.0, 30.0), (1160.0, 72.0))}
    drafts = {}
    for stature, meas in wearers.items():
        for program in sorted(drafting.PROGRAMS):
            pat, spec = drafting.PROGRAMS[program](copy.deepcopy(meas), None)
            drafts[(program, stature)] = (pat, spec)
            add(f"{program}-{int(stature)}", pat, spec, meas)

    meas = wearers[920.0]
    bolero, bspec = copy.deepcopy(drafts[("zouave_bolero", 920.0)])
    # M1: the front side seam 10 mm longer.
    m1 = copy.deepcopy(bolero)
    front = next(p for p in m1["pieces"] if p["id"] == "front")
    front["vertices"]["side_hem"][1] -= 10
    add("M1_side_seam_mismatch", m1, bspec, meas)
    # M2: drafted with too little chest ease.
    m2, m2spec = drafting.zouave_bolero(copy.deepcopy(meas), {"chest_ease": 10.0})
    add("M2_chest_too_small", m2, bspec, meas)
    # M3: straight sleeve hem.
    m3 = copy.deepcopy(bolero)
    for e in next(p for p in m3["pieces"] if p["id"] == "sleeve")["edges"]:
        if e["id"] == "hem":
            e.pop("curve", None)
    add("M3_truing_kink", m3, bspec, meas)
    extra_points = {
        "M4_missing_vertex": {"id": "x", "kind": "width", "instance": "sleeve.R", "at_vertex": "nope", "body": "upper_arm",
                              "target_mm": [0, 1, 2]},
        "M5_unknown_kind": {"id": "s", "kind": "spiral", "label": "Spiral", "body": "chest", "target_mm": [0, 1, 2]},
        "M6_arm_landmark": {"id": "g", "kind": "girth", "instances": ["sleeve.R"], "landmark": "chest", "body": "chest",
                            "target_mm": [0, 1, 2]},
        "M7_unmeasured_body": {"id": "k", "kind": "vertical", "instance": "back", "from": "snp", "to": "tip",
                               "body": "knee_to_moon", "target_mm": [0, 1, 2]},
    }
    for cid, point in extra_points.items():
        spec = copy.deepcopy(bspec)
        spec["points"].append(point)
        add(cid, bolero, spec, meas)
    trousers, tspec = copy.deepcopy(drafts[("zouave_trousers", 920.0)])
    m8 = copy.deepcopy(trousers)
    for p in m8["pieces"]:
        if p["id"] in ("tr_front", "tr_back"):
            for k, v in p["vertices"].items():
                if "hem" in k:
                    v[1] -= 150
    add("M8_trouser_hem_lower", m8, tspec, meas)
    m9, m9spec = drafting.zouave_trousers(copy.deepcopy(meas), {"elastic_frac": 1.05})
    add("M9_elastic_too_long", m9, m9spec, meas)  # the elastic point's scale comes from this draft's spec
    small = copy.deepcopy(meas)
    small["fused"]["head"]["value_mm"] -= 30
    m10, _ = drafting.fez(small, None)
    fez_spec = drafts[("fez", 920.0)][1]
    add("M10_fez_too_small", m10, fez_spec, meas)
    m11 = copy.deepcopy(bolero)
    m11["schema"] = "x"
    add("M11_bad_schema", m11, bspec, meas)
    flat = {k: v["value_mm"] if "value_mm" in v else v.get("value_deg") for k, v in meas["fused"].items()}
    add("M12_flat_measurements", bolero, bspec, flat)

    # F12 seam-walk variants (a: 100 x 50, b: 100 x 60).
    for cid, extras in {"F12_equal": None, "F12_plain": {}, "F12_ratio": {"ratio": 1.2},
                        "F12_band_in": {"ratio_band": [0.8, 0.9]}, "F12_legacy_band": {"band": [1.0, 1.3]},
                        "F12_band_out": {"ratio_band": [0.9, 1.0], "ratio": 0.95},
                        "F12_band_null": {"ratio_band": None, "ratio": 1.2}, "F12_band_empty": {"ratio_band": []},
                        "F12_no_reverse": {"reverse": False}, "F12_tol": {"tol_mm": 12.0}}.items():
        b_h = 50 if extras is None else 60
        add(cid, doc([rect("a", 100, 50), rect("b", 100, b_h)], [dict({"id": "s", "a": "a/right", "b": "b/left"}, **(extras or {}))]),
            {"points": []}, {})
    # Notches (F13-F15).
    def notched(bn, **seam):
        a, b = rect("a", 100, 50), rect("b", 100, 60)
        a["notches"] = [{"edge": "right", "s": 0.25}]
        b["notches"] = [{"edge": "left", "s": s} for s in bn]
        return doc([a, b], [dict({"id": "s", "a": "a/right", "b": "b/left"}, **seam)])
    add("F13_notch_misplaced", notched([0.5]), {"points": []}, {})
    add("F14_notch_ratio", notched([0.75], ratio=50 / 60), {"points": []}, {})
    add("F15_count_mismatch_ratio", notched([0.5, 0.8], ratio=50 / 60), {"points": []}, {})
    add("F15_count_mismatch", notched([0.5, 0.8]), {"points": []}, {})
    add("F19_multi_edge", doc([rect("a", 100, 50), rect("b", 100, 60)], [{"id": "s", "a": "a/bottom+right", "b": "b/left+bottom"}]),
        {"points": []}, {})
    zero = rect("a", 100, 50)
    zero["vertices"]["c"], zero["vertices"]["d"] = [100, 0], [0, 50]
    add("F21_zero_side", doc([zero, rect("b", 100, 60)], [{"id": "s", "a": "a/right", "b": "b/left"}]), {"points": []}, {})
    # Truing F22-F25.
    two = doc([rect("a"), rect("b")], [{"id": "s", "a": "a/right", "b": "b/left"}])
    add("F22_truing", two, {"points": []}, {})
    tilted = copy.deepcopy(two)
    tilted["pieces"][1]["vertices"]["a"] = [0, -5]
    add("F23_truing_tilted", tilted, {"points": []}, {})
    three = copy.deepcopy(two)
    three["seams"].append({"id": "t", "a": "a/top", "b": "b/top"})
    add("F24_truing_sewn", three, {"points": []}, {})
    add("F24_truing_explicit", three, {"points": [], "truing": [{"seam": "s", "end": "end"}]}, {})
    norev = copy.deepcopy(two)
    norev["seams"][0]["reverse"] = False
    add("F25_truing_no_reverse", norev, {"points": []}, {})
    # Points F28-F32.
    r = doc([rect("r", 100, 50)])
    pts = [
        {"id": "w", "kind": "width", "instance": "r", "at_vertex": "c", "dy": -10, "body": "chest", "target_mm": [-500, 0, 500]},
        {"id": "v", "kind": "vertical", "instance": "r", "from": "a", "to": "c", "body": "chest", "body_scale": 0.1,
         "body_offset_mm": -4, "target_mm": [-500, 0, 500]},
        {"id": "p", "kind": "path", "refs": ["r/bottom+right", "r/top"], "scale": 0.5, "body": "chest", "target_mm": [-500, 0, 500]},
    ]
    add("F28_points_flat", r, {"points": pts}, {"chest": 540})
    p29 = [{"id": "q", "kind": "path", "refs": ["r/top"], "body": "chest", "target_mm": [-500, -480, -470]}]
    add("F29_sigma_note", r, {"points": p29}, {"fused": {"chest": {"value_mm": 540, "sigma_mm": 20}}})
    add("F29_sigma_small", r, {"points": p29}, {"fused": {"chest": {"value_mm": 540, "sigma_mm": 5.04}}})
    t = {"id": "t", "vertices": {"a": [0, -50], "b": [120, -50], "c": [100, 150], "d": [0, 150]},
         "edges": [{"id": "e0", "from": "a", "to": "b"}, {"id": "e1", "from": "b", "to": "c"},
                   {"id": "e2", "from": "c", "to": "d"}, {"id": "e3", "from": "d", "to": "a"}],
         "cut": {"qty": 2}, "grain": [[10, 0], [10, 100]], "placement": {"part": "torso", "y0_landmark": "waist", "y0": 10}}
    s = {"id": "s", "vertices": {"a": [-60, -300], "b": [60, -300], "c": [80, -100], "d": [0, 0], "e": [-80, -100]},
         "edges": [{"id": f"s{i}", "from": v, "to": w} for i, (v, w) in enumerate(zip("abcde", "bcdea"))],
         "cut": {"qty": 1}, "grain": [[0, -250], [0, -50]], "placement": {"part": "arm", "y0_landmark": "shoulder_tip"}}
    p30 = [{"id": "g", "kind": "girth", "instances": ["t.R", "t.L"], "landmark": "chest", "dy": -20, "body": "chest", "target_mm": [0, 10, 20]},
           {"id": "u", "kind": "width", "instance": "s", "landmark": "elbow", "body": "upper_arm", "target_mm": [0, 10, 20]},
           {"id": "z", "kind": "width", "instance": "s", "landmark": "waist", "body": "upper_arm", "target_mm": [0, 10, 20]}]
    stub = {"torso": {"waist": 500.0, "chest": 620.0}, "arm_R": {"shoulder_tip": 20.0, "elbow": -150.0}}
    add("F30_landmark_lines", doc([t, s]), {"points": p30}, {"chest": 400, "upper_arm": 150}, landmarks_stub=stub)
    add("F31_completeness", doc([rect("a", grain=False), rect("b", cut={})]), {"points": []}, {})
    p32 = [{"id": f"p{i}", "kind": "vertical", "instance": "a", "from": "a", "to": "d", "body": "x", "target_mm": [tv, tv, tv]}
           for i, tv in enumerate([0, 60, 45, 10, 100, -30, 50, 70])]
    add("F32_worst_order", doc([rect("a")]), {"points": p32}, {"x": 40})

    default_specs = []
    for (program, stature), (pat, _) in sorted(drafts.items()):
        default_specs.append({"id": f"{program}-{int(stature)}", "input": {"pattern": json.loads(json.dumps(pat))},
                              "output": validate.default_fit_spec(Pattern(copy.deepcopy(pat)), program)})

    meta = {
        "reference_files": {f: hashlib.sha256((args.reference / f).read_bytes()).hexdigest() for f in FILES},
        "numpy_version": np.__version__, "python_version": platform.python_version(), "machine": platform.machine(),
    }
    args.out_dir.mkdir(parents=True, exist_ok=True)
    for name, body_cases in (("validation", cases), ("default_fit_spec", default_specs)):
        path = args.out_dir / f"{name}.json"
        path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "validate", "fixture": name, "meta": meta,
                                    "cases": body_cases}, separators=(",", ":"), ensure_ascii=False) + "\n")
        print(f"wrote {len(body_cases)} cases to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
