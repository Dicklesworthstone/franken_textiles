#!/usr/bin/env python3
"""Capture pattern-geometry oracle fixtures from the Python reference (docs/spec/01 §4-6, §14.2 F1-F10).

Runs the reference's `pattern_model.py` (the fabric-pattern skill's scripts directory, passed as --reference)
on the spec's fixture inputs plus seeded random curves and polygons, and writes the inputs with the
reference's outputs:

    OUT_DIR/geometry.json    curve sampling, chord-relative encoding, mirroring, polyline/polygon helpers
    OUT_DIR/instances.json   piece instances (mirror pairs, on-fold unfolding) with areas and perimeters

The reference is identified by the SHA-256 of the files it ran, not by a path. Usage:
    python capture_pattern_geometry.py --reference SKILL/scripts OUT_DIR
Needs numpy (the reference's own requirement). Deterministic: the same reference gives the same files.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import sys
from pathlib import Path

import numpy as np

SCHEMA = "ftx-oracle/1"
SEED = 20261012


def tolist(a):
    return np.asarray(a, dtype=float).tolist()


def rect(pid, w=100.0, h=50.0, cut=None):
    """Spec 01 §14.2 R(id, w, h, cut)."""
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


def doc(pieces, seams=None):
    return {"schema": "fabric-pattern/1", "units": "mm", "pieces": pieces, "seams": seams or []}


def geometry_cases(pm, rng):
    cases = []

    def add(fn, inp, out):
        cases.append({"id": f"{fn}-{sum(c['fn'] == fn for c in cases)}", "fn": fn, "input": inp, "output": out})

    def try_call(f, *a):
        try:
            return {"value": f(*a)}
        except (ValueError, KeyError) as exc:
            return {"error": str(exc)}

    # F1 and random chords.
    triples = [((3, 4), (40, -10), (20, 15)), ((1, 1), (1, 1), (2, 2))]
    triples += [tuple(tuple(rng.uniform(-500, 500, 2)) for _ in range(3)) for _ in range(40)]
    for p0, p1, q in triples:
        add("abs_to_rel", {"p0": list(p0), "p1": list(p1), "q": list(q)}, try_call(pm.abs_to_rel, p0, p1, q))
    for _ in range(40):
        p0, p1, th = rng.uniform(-500, 500, 2), rng.uniform(-500, 500, 2), rng.uniform(-1, 1.5, 2)
        add("rel_to_abs", {"p0": tolist(p0), "p1": tolist(p1), "th": tolist(th)}, {"value": tolist(pm.rel_to_abs(p0, p1, th))})

    # F2 and random curves at every sample count the reference uses.
    curves = [
        None,
        {"type": "line"},
        {"type": "poly", "c": [[0.25, 0.1], [0.75, -0.1]]},
        {"type": "quad", "c": [[0.5, 0.5]]},
        {"type": "cubic", "c": [[0.25, 0.3], [0.75, 0.3]]},
        {"type": "quad", "c": [[0.5, 0.5], [0.6, 0.1]]},
        {"type": "cubic", "c": [[0.5, 0.5]]},
        {"type": "spline", "c": []},
        {"type": "poly", "c": []},
    ]
    for c in curves:
        add("curve_points", {"p0": [0, 0], "p1": [100, 0], "curve": c, "dense": 200},
            try_call(lambda c=c: tolist(pm.curve_points((0, 0), (100, 0), c, 200))))
    for i in range(48):
        kind = ["quad", "cubic", "poly"][i % 3]
        n = {"quad": 1, "cubic": 2, "poly": int(rng.integers(0, 6))}[kind]
        c = {"type": kind, "c": tolist(rng.uniform(-0.6, 1.6, (n, 2)))}
        dense = [200, 120, 60, 400][i % 4]
        p0, p1 = rng.uniform(-400, 400, 2), rng.uniform(-400, 400, 2)
        pts = pm.curve_points(p0, p1, c, dense)
        add("curve_points", {"p0": tolist(p0), "p1": tolist(p1), "curve": c, "dense": dense}, {"value": tolist(pts)})

    # F3 mirror/reverse.
    for c in [{"type": "cubic", "c": [[0.2, 0.1], [0.7, -0.3]], "x": 1}, None, {"type": "line"}, {}, {"type": "quad", "c": [[0.3, 0.4]]}]:
        add("mirror_curve", {"curve": c}, {"value": pm.mirror_curve(c)})
        add("reverse_curve", {"curve": c}, {"value": pm.reverse_curve(c)})

    # F4-F6 and random polylines/polygons.
    polylines = [
        [(0, 0), (10, 0), (10, 0), (10, 10)],
        [(3, 4), (3, 4)],
        [(0, 0), (1, 0)],
        [(0, 0), (5, 0), (10, 0), (10, 10)],
        [(0, 0), (1, 0), (2, 1)],
        tolist(pm.curve_points((0, 0), (100, 0), {"type": "quad", "c": [[0.5, 0.5]]}, 200)),
    ]
    for _ in range(30):
        n = int(rng.integers(2, 40))
        steps = rng.normal(0, 1, (n, 2)) * rng.uniform(0.05, 20)
        polylines.append(tolist(np.cumsum(steps, axis=0)))
    fr = [0, 0.25, 0.5, 0.75, 1, 1.5, -1, 0.1, 0.333, 0.9]
    for p in polylines:
        a = np.asarray(p, float)
        add("polyline_length", {"pts": p}, {"value": pm.polyline_length(a)})
        add("cumulative", {"pts": p}, {"value": tolist(pm.cumulative(a))})
        add("at_fractions", {"pts": p, "fractions": fr}, {"value": tolist(pm.at_fractions(a, fr))})
        for end in ("start", "end"):
            add("tangent_at", {"pts": p, "end": end}, {"value": tolist(pm.tangent_at(a, end))})
    polygons = [
        [(0, 0), (10, 0), (10, 10), (0, 10)],
        [(0, 10), (10, 10), (10, 0), (0, 0)],
        [(0, 0), (1, 0)],
        [(0, 0), (30, 0), (30, 30), (20, 30), (20, 10), (10, 10), (10, 30), (0, 30)],
        [(0, 0), (10, 0), (5, 10)],
    ]
    for _ in range(20):
        n = int(rng.integers(3, 30))
        ang = np.sort(rng.uniform(0, 2 * np.pi, n))
        r = rng.uniform(20, 200, n)
        polygons.append(tolist(np.stack([r * np.cos(ang), r * np.sin(ang)], axis=1)))
    for poly in polygons:
        a = np.asarray(poly, float)
        add("signed_area", {"poly": poly}, {"value": pm.signed_area(a)})
        ys = sorted({float(v) for v in a[:, 1]} | {float(v) for v in rng.uniform(a[:, 1].min() - 5, a[:, 1].max() + 5, 6)})
        add("horizontal_width", {"poly": poly, "ys": ys}, {"value": [pm.horizontal_width(a, y) for y in ys]})
        pts = np.concatenate([a, rng.uniform(a.min(axis=0) - 5, a.max(axis=0) + 5, (12, 2))])
        add("point_in_polygon", {"poly": poly, "pts": tolist(pts)}, {"value": [bool(v) for v in pm.point_in_polygon(pts, a)]})
    return cases


def instance_cases(pm):
    tri = {
        "id": "t",
        "name": "T",
        "vertices": {"a": [0, 0], "b": [100, 0], "c": [0, 100]},
        "edges": [
            {"id": "e1", "from": "a", "to": "b", "curve": {"type": "cubic", "c": [
                pm.abs_to_rel((0, 0), (100, 0), (30, -10)), pm.abs_to_rel((0, 0), (100, 0), (70, -10))]}},
            {"id": "f", "from": "b", "to": "c"},
            {"id": "e3", "from": "c", "to": "a"},
        ],
        "cut": {"qty": 1, "on_fold": "f"},
        "grain": [[10, 10], [10, 40]],
    }
    curved = rect("cv", 80, 60, {"qty": 2})
    curved["edges"][1]["curve"] = {"type": "cubic", "c": [[0.3, 0.2], [0.7, -0.1]]}
    curved["edges"][2]["curve"] = {"type": "quad", "c": [[0.5, -0.3]]}
    curved_fold = rect("cf", 80, 60, {"qty": 2, "on_fold": "left"})
    curved_fold["edges"][2]["curve"] = {"type": "cubic", "c": [[0.2, 0.25], [0.8, 0.1]], "keep": "me"}
    patterns = {
        "F7": doc([rect("back", 60, 40, {"qty": 1, "on_fold": "left"})]),
        "F8": doc([rect("p", 60, 40, {"qty": 2})]),
        "F9": doc([rect("bk", 60, 40, {"qty": 2, "on_fold": "left"})]),
        "F10": doc([tri]),
        "curved_pair": doc([curved]),
        "curved_fold_pair": doc([curved_fold]),
    }
    out = []
    for name, data in patterns.items():
        p = pm.Pattern(data)
        insts = []
        for inst in p.instances.values():
            insts.append({
                "id": inst.id,
                "piece": inst.piece,
                "side": inst.side,
                "mirrored": bool(inst.mirrored),
                "verts": {k: tolist(v) for k, v in inst.verts.items()},
                "edges": [{"id": e["id"], "from": e["from"], "to": e["to"], "curve": e.get("curve")} for e in inst.edges],
                "area": inst.area(),
                "perimeter": inst.perimeter(),
                "edge_lengths": {e["id"]: inst.edge_length(e["id"]) for e in inst.edges},
            })
        out.append({"id": name, "input": data, "output": {"structure_errors": p.structure_errors(), "instances": insts}})
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--reference", required=True, type=Path, help="the fabric-pattern skill's scripts directory")
    ap.add_argument("out_dir", type=Path)
    args = ap.parse_args(argv)
    src = args.reference / "pattern_model.py"
    if not src.is_file():
        print(f"no pattern_model.py in {args.reference}", file=sys.stderr)
        return 2
    sys.path.insert(0, str(args.reference.resolve()))
    import pattern_model as pm

    meta = {
        "reference_files": {"pattern_model.py": hashlib.sha256(src.read_bytes()).hexdigest()},
        "numpy_version": np.__version__,
        "python_version": platform.python_version(),
        "machine": platform.machine(),
        "seed": SEED,
    }
    args.out_dir.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(SEED)
    for name, cases in (("geometry", geometry_cases(pm, rng)), ("instances", instance_cases(pm))):
        body = {"schema": SCHEMA, "subsystem": "pattern", "fixture": name, "meta": meta, "cases": cases}
        path = args.out_dir / f"{name}.json"
        path.write_text(json.dumps(body, separators=(",", ":")) + "\n")
        print(f"wrote {len(cases)} cases to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
