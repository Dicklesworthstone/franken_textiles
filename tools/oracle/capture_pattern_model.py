#!/usr/bin/env python3
"""Capture pattern-model oracle fixtures from the Python reference (docs/spec/01 §6-8, §14.2 F11-F27).

Writes, from the reference's pattern_model.py, drafting.py and measurements.py (the fabric-pattern skill's
scripts directory, passed as --reference):

    OUT_DIR/structure.json   structure_errors() of the F11 inputs and the instance ids they build
    OUT_DIR/seams.json       side lengths, notch fractions, seam pairs, edge tangents and corner angles
                             on the F12-F21, F26, F27 inputs
    OUT_DIR/drafts.json      the three drafted garments for public reference wearers (no real person):
                             measurements, pattern, fit spec, instances, side lengths, notch fractions
                             and seam pairs of every seam

The reference is identified by the SHA-256 of its files, not by a path. Usage:
    python capture_pattern_model.py --reference SKILL/scripts OUT_DIR
Needs the reference's requirements (numpy, shapely). Deterministic.
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
FILES = ("pattern_model.py", "drafting.py", "measurements.py")


def tolist(a):
    return np.asarray(a, dtype=float).tolist()


def rect(pid, w=100.0, h=50.0, cut=None):
    return {
        "id": pid,
        "name": pid.upper(),
        "vertices": {"a": [0, 0], "b": [w, 0], "c": [w, h], "d": [0, h]},
        "edges": [
            {"id": "bottom", "from": "a", "to": "b"},
            {"id": "right", "from": "b", "to": "c"},
            {"id": "top", "from": "c", "to": "d"},
            {"id": "left", "from": "d", "to": "a"},
        ],
        "notches": [],
        "cut": cut if cut is not None else {"qty": 1},
        "grain": [[10, 10], [10, 40]],
    }


def doc(pieces, seams=None, **top):
    return {"schema": "fabric-pattern/1", "units": "mm", "pieces": pieces, "seams": seams or [], **top}


def structure_inputs():
    """Spec 01 §14.2 F11, one input per row."""
    cases = {}
    cases["schema_and_units"] = dict(doc([rect("x")]), schema="fabric-pattern/2", units="cm")
    p = rect("x")
    p["edges"][0]["id"] = "bad.id"
    cases["edge_id_with_dot"] = doc([p])
    cases["piece_id_with_space"] = doc([rect("x y")])
    p = rect("y")
    p["vertices"]["c"], p["vertices"]["d"] = [100, -50], [0, -50]
    cases["clockwise"] = doc([p])
    cases["fold_edge_missing"] = doc([rect("z", cut={"qty": 1, "on_fold": "nope"})])
    cases["duplicate_seam_id"] = doc([rect("a"), rect("b")], [
        {"id": "s", "a": "a/right", "b": "b/left"}, {"id": "s", "a": "a/top", "b": "b/top"}])
    p = rect("panel", cut={"qty": 1, "on_fold": "left"})
    p["edges"][0]["id"] = "right"
    cases["duplicate_edge_ids"] = doc([p])
    p = rect("panel", cut={"qty": 1, "on_fold": "left"})
    p["edges"][0]["from"] = "missing"
    cases["unknown_vertex"] = doc([p])
    p = rect("panel", cut={"qty": 1, "on_fold": "left"})
    p["edges"][3]["curve"] = {"type": "quad", "c": [[0.5, 0.2]]}
    cases["curved_fold"] = doc([p])
    p = rect("panel", cut={"qty": 1, "on_fold": "left"})
    p["vertices"]["d"] = [0, 0]
    cases["degenerate_fold"] = doc([p])
    p = rect("panel")
    p["edges"][1]["curve"] = {"type": "spline", "c": []}
    cases["unknown_curve_type"] = doc([p])
    cases["qty_three"] = doc([rect("panel", cut={"qty": 3})])
    cases["duplicate_piece"] = doc([rect("panel", 100), rect("panel", 200)])
    cases["generated_instance_collision"] = doc([rect("panel", cut={"qty": 2}), rect("panel.R")])
    p = rect("panel")
    del p["edges"][2]
    cases["gap_in_loop"] = doc([p])
    p = rect("panel")
    del p["edges"][1]["id"]
    cases["edge_without_id"] = doc([p])
    for name, a in [("unknown_instance", "missing/right"), ("unknown_edge", "a/missing"),
                    ("three_consecutive", "a/right+top+left"), ("not_consecutive", "a/right+left")]:
        cases[name] = doc([rect("a"), rect("b")], [{"id": "s", "a": a, "b": "b/left"}])
    cases["edge_used_twice"] = doc([rect("a"), rect("b")], [
        {"id": "s", "a": "a/right", "b": "b/left"}, {"id": "t", "a": "a/right", "b": "b/top"}])
    cases["edge_on_both_sides"] = doc([rect("a"), rect("b")], [{"id": "s", "a": "a/right", "b": "a/right"}])
    cases["seams_without_ids"] = doc([rect("a"), rect("b")], [
        {"a": "a/right", "b": "b/left"}, {"a": "a/top", "b": "b/top"}])
    cases["valid_pair_and_fold"] = doc([rect("p", cut={"qty": 2}), rect("f", cut={"qty": 2, "on_fold": "left"})], [
        {"id": "s", "a": "p.R/right", "b": "f.R/bottom.R"}, {"id": "t", "a": "p.L/right", "b": "f.L/bottom.L"}])
    return cases


def seam_inputs():
    """Spec 01 §14.2 F12-F21: two rectangles a (100 x 50) and b (100 x 60) and the seam variants."""
    def ab(a_notches=(), b_notches=(), **seam):
        a, b = rect("a", 100, 50), rect("b", 100, 60)
        a["notches"] = [{"edge": e, "s": s} for e, s in a_notches]
        b["notches"] = [{"edge": e, "s": s} for e, s in b_notches]
        return doc([a, b], [dict({"id": "s", "a": "a/right", "b": "b/left"}, **seam)])

    cases = {
        "equal": doc([rect("a", 100, 50), rect("b", 100, 50)], [{"id": "s", "a": "a/right", "b": "b/left"}]),
        "plain": ab(),
        "no_reverse": ab(reverse=False),
        "reverse_null": ab(reverse=None),
        "notch_piecewise": ab([("right", 0.25)], [("left", 0.5)]),
        "notch_ratio": ab([("right", 0.25)], [("left", 0.75)], ratio=50 / 60),
        "notch_count_mismatch": ab([("right", 0.25)], [("left", 0.5), ("left", 0.8)]),
        "end_notches": ab([], [("left", 0.0), ("left", 1.0)]),
        "multi_edge": doc([rect("a", 100, 50), rect("b", 100, 60)], [{"id": "s", "a": "a/bottom+right", "b": "b/left+bottom"}]),
    }
    zero = rect("a", 100, 50)
    zero["vertices"]["c"], zero["vertices"]["d"] = [100, 0], [0, 50]
    cases["zero_side"] = doc([zero, rect("b", 100, 60)], [{"id": "s", "a": "a/right", "b": "b/left"}])
    m = rect("m", 60, 40, {"qty": 2})
    m["notches"] = [{"edge": "top", "s": 0.25}]
    cases["mirrored_notches"] = doc([m], [{"id": "s", "a": "m.R/right+top", "b": "m.L/top+right"}])
    f1 = rect("f", 60, 40, {"qty": 1, "on_fold": "left"})
    f1["notches"] = [{"edge": "top", "s": 0.25}]
    cases["fold_notches"] = doc([f1], [{"id": "s", "a": "f/top.R+top.L", "b": "f/bottom.L+bottom.R"}])
    f2 = rect("f", 60, 40, {"qty": 2, "on_fold": "left"})
    f2["notches"] = [{"edge": "top", "s": 0.25}]
    cases["fold_pair_notches"] = doc([f2], [{"id": "s", "a": "f.L/top.L", "b": "f.L/top.R"}])
    return cases


def l_shape_and_triangle():
    l = {"id": "l", "vertices": {"a": [0, 0], "b": [30, 0], "c": [30, 10], "d": [10, 10], "e": [10, 30], "f": [0, 30]},
         "edges": [{"id": f"e{i}", "from": v, "to": w} for i, (v, w) in enumerate(zip("abcdef", "bcdefa"))],
         "cut": {"qty": 1}}
    t = {"id": "t", "vertices": {"a": [0, 0], "b": [100, 0], "c": [50, 80]},
         "edges": [{"id": "e0", "from": "a", "to": "b", "curve": {"type": "quad", "c": [[0.5, -0.5]]}},
                   {"id": "e1", "from": "b", "to": "c"}, {"id": "e2", "from": "c", "to": "a"}],
         "cut": {"qty": 1}}
    traced = {"id": "tr", "vertices": {"a": [0, 0], "b": [80, 10], "c": [40, 70]},
              "edges": [{"id": "p0", "from": "a", "to": "b", "curve": {"type": "poly", "c": [[0.2, 0.05], [0.5, 0.08], [0.8, 0.04]]}},
                        {"id": "p1", "from": "b", "to": "c"}, {"id": "p2", "from": "c", "to": "a",
                                                                "curve": {"type": "poly", "c": [[0.5, -0.1]]}}],
              "cut": {"qty": 2}}
    return {"l_shape": doc([l]), "curved_triangle": doc([t]), "traced_pair": doc([traced])}


def describe_side(pm_pattern, ref):
    out = {}
    try:
        pts, junctions = pm_pattern.side_polyline(ref)
        out["length"] = pm_pattern.side_length(ref)
        out["junctions"] = [float(j) for j in junctions]
        out["points"] = len(pts)
        out["notch_fractions"] = pm_pattern.notch_fractions(ref)
    except (KeyError, ValueError, ZeroDivisionError) as exc:
        out["error"] = str(exc)
    return out


def describe_pairs(pm_pattern, seam, spacing=5.0):
    try:
        r = pm_pattern.seam_pairs(seam, spacing)
    except (KeyError, ValueError, ZeroDivisionError) as exc:
        return {"error": str(exc)}
    return {"a": tolist(r["a"]), "b": tolist(r["b"]), "fa": tolist(r["fa"]), "fb": tolist(r["fb"]),
            "len_a": r["len_a"], "len_b": r["len_b"], "a_junctions": tolist(r["a_junctions"]),
            "b_junctions": tolist(r["b_junctions"]), "notches_a": tolist(r["notches_a"]),
            "notches_b": tolist(r["notches_b"]), "notch_match": r["notch_match"]}


def describe_instances(pm_pattern):
    out = []
    for inst in pm_pattern.instances.values():
        out.append({
            "id": inst.id, "piece": inst.piece, "side": inst.side, "mirrored": bool(inst.mirrored),
            "area": inst.area(), "perimeter": inst.perimeter(),
            "edge_lengths": {e["id"]: inst.edge_length(e["id"]) for e in inst.edges},
            "tangents": {e["id"]: [tolist(inst.edge_tangent(e["id"], "start")), tolist(inst.edge_tangent(e["id"], "end"))]
                         for e in inst.edges},
            "corner_angles": {e["to"]: inst.corner_angle(e["to"]) for e in inst.edges},
        })
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--reference", required=True, type=Path, help="the fabric-pattern skill's scripts directory")
    ap.add_argument("out_dir", type=Path)
    args = ap.parse_args(argv)
    for f in FILES:
        if not (args.reference / f).is_file():
            print(f"no {f} in {args.reference}", file=sys.stderr)
            return 2
    sys.path.insert(0, str(args.reference.resolve()))
    import drafting
    import measurements
    import pattern_model as pm

    meta = {
        "reference_files": {f: hashlib.sha256((args.reference / f).read_bytes()).hexdigest() for f in FILES},
        "numpy_version": np.__version__,
        "python_version": platform.python_version(),
        "machine": platform.machine(),
    }

    structure = []
    for name, data in structure_inputs().items():
        given = copy.deepcopy(data)
        p = pm.Pattern(data)
        structure.append({"id": name, "input": given,
                          "output": {"structure_errors": p.structure_errors(), "instances": list(p.instances)}})

    seams = []
    for name, data in {**seam_inputs(), **l_shape_and_triangle()}.items():
        p = pm.Pattern(copy.deepcopy(data))
        out = {"structure_errors": p.structure_errors(), "instances": describe_instances(p) if not p.build_errors else []}
        out["seams"] = []
        for s in data["seams"]:
            out["seams"].append({"id": s.get("id"), "a": describe_side(p, s["a"]), "b": describe_side(p, s["b"]),
                                 "pairs": describe_pairs(p, s), "pairs_spacing_7": describe_pairs(p, s, 7.0)})
        seams.append({"id": name, "input": data, "output": out})

    drafts = []
    for stature, months in ((800.0, 13.0), (920.0, 30.0), (1160.0, 72.0)):
        meas = measurements.reference_wearer(stature, months)
        for program in sorted(drafting.PROGRAMS):
            pat, spec = drafting.PROGRAMS[program](copy.deepcopy(meas), None)
            pat = json.loads(json.dumps(pat))  # what a file would hold
            p = pm.Pattern(copy.deepcopy(pat))
            out = {"structure_errors": p.structure_errors(), "instances": describe_instances(p), "seams": []}
            for s in pat.get("seams", []):
                out["seams"].append({"id": s.get("id"), "a": describe_side(p, s["a"]), "b": describe_side(p, s["b"]),
                                     "pairs": describe_pairs(p, s)})
            drafts.append({"id": f"{program}-{int(stature)}", "input": {"measurements": meas, "pattern": pat, "fit_spec": spec},
                           "output": out})

    args.out_dir.mkdir(parents=True, exist_ok=True)
    for name, cases in (("structure", structure), ("seams", seams), ("drafts", drafts)):
        body = {"schema": SCHEMA, "subsystem": "pattern", "fixture": name, "meta": meta, "cases": cases}
        path = args.out_dir / f"{name}.json"
        path.write_text(json.dumps(body, separators=(",", ":")) + "\n")
        print(f"wrote {len(cases)} cases to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
