#!/usr/bin/env python3
"""Capture shapely's `buffer` (GEOS 3.14.1) where its topology decides the answer, as an oracle fixture:
    OUT_DIR/buffer_topology.json   offset curves that cross themselves (where the result ring starts), insets
                                   that split or vanish (a dumbbell, an eroded triangle), the distance-0
                                   clean-up (a spike, a pinch, a bowtie, the trace's own outlines with a
                                   one-pixel leader), holes, multipolygons, and traced stroke outlines offset
                                   to their centres; each with the polygons in GEOS's order, rings as GEOS
                                   starts them, and whether GEOS needed its reduced-precision retry
                                   (`BufferOp::bufferReducedPrecision`: every result coordinate on the 1e-9
                                   grid its 12 digits imply, the input's not).
Usage:
    python capture_buffer_topology.py --reference SKILL/scripts OUT_DIR
Needs the reference's requirements (numpy, opencv-python-headless, shapely). Deterministic.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import platform
import random
import sys
from pathlib import Path

import numpy as np

SCHEMA = "ftx-oracle/1"
FILES = ("vectorize.py",)


def on_grid(values, scale=1e9) -> bool:
    return all(abs(v * scale - round(v * scale)) < 1e-4 for v in values)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--reference", required=True, type=Path)
    ap.add_argument("out_dir", type=Path)
    args = ap.parse_args()
    ref = args.reference.resolve()
    sys.path.insert(0, str(ref))
    sys.dont_write_bytecode = True
    import cv2
    import shapely
    from shapely import geometry
    import vectorize as V

    meta = {"reference_files": {f: hashlib.sha256((ref / f).read_bytes()).hexdigest() for f in FILES},
            "numpy_version": np.__version__, "opencv_version": cv2.__version__, "shapely_version": shapely.__version__,
            "geos_version": shapely.geos_version_string, "python_version": platform.python_version(),
            "machine": platform.machine()}
    random.seed(5)
    cases = []

    def buf(cid, polys, dist, join):
        shp = [geometry.Polygon(p["shell"], p.get("holes") or []) for p in polys]
        g = shp[0] if len(shp) == 1 else geometry.MultiPolygon(shp)
        out = g.buffer(dist, join_style="mitre", mitre_limit=4.0) if join == "mitre" else g.buffer(dist)
        parts = list(out.geoms) if out.geom_type == "MultiPolygon" else ([] if out.is_empty else [out])
        res = [{"shell": [list(c) for c in p.exterior.coords], "holes": [[list(c) for c in h.coords] for h in p.interiors]}
               for p in parts]
        inp = [v for p in polys for c in p["shell"] for v in c]
        got = [v for p in res for c in p["shell"] for v in c]
        retried = bool(got) and on_grid(got) and not on_grid(inp)
        cases.append({"id": cid, "input": {"polys": polys, "distance": dist, "join": join},
                      "output": {"polygons": res, "reduced_precision": retried}})

    def ring(pts):
        pts = [list(map(float, p)) for p in pts]
        return pts + [pts[0]]

    def star(n, r0, r1, cx, cy, jit):
        return [[cx + (r0 if i % 2 == 0 else r1) * (1 + random.uniform(-jit, jit)) * math.cos(math.pi * i / n),
                 cy + (r0 if i % 2 == 0 else r1) * (1 + random.uniform(-jit, jit)) * math.sin(math.pi * i / n)]
                for i in range(2 * n)]

    # an offset curve crossing itself: GEOS starts the ring at its lowest node, not at the curve's start
    buf("crossing-mitre", [{"shell": ring([[93.399, 138.479], [78.997, 108.668], [78.166, 78.057], [142.495, 67.495],
                                           [123.953, 87.002], [133.247, 82.023]])}], 2.0, "mitre")
    # erosion: a triangle past its in-radius, a dumbbell split at its neck
    buf("triangle-eroded", [{"shell": ring([[0, 0], [10, 0], [5, 10 * math.sqrt(3) / 2]])}], -3.2, "mitre")
    buf("dumbbell-split", [{"shell": ring([[0, 0], [12, 0], [12, 4.9], [18, 4.9], [18, 0], [30, 0], [30, 10], [18, 10],
                                           [18, 5.1], [12, 5.1], [12, 10], [0, 10]])}], -2.0, "mitre")
    # the distance-0 clean-up
    buf("spike-0", [{"shell": ring([[0, 0], [10, 0], [10, 5], [18, 5], [10, 5], [10, 10], [0, 10]])}], 0.0, "round")
    buf("pinch-0", [{"shell": ring([[0, 0], [10, 0], [10, 10], [20, 10], [20, 20], [10, 20], [10, 10], [0, 10]])}], 0.0,
        "round")
    buf("bowtie-0", [{"shell": ring([[0, 0], [10, 10], [10, 0], [0, 10]])}], 0.0, "round")
    # holes and multipolygons
    sq = lambda x, y, s: ring([[x, y], [x + s, y], [x + s, y + s], [x, y + s]])  # noqa: E731
    for d in (-2.0, 1.5, 6.0):
        buf(f"holed-{d}", [{"shell": sq(0, 0, 40), "holes": [sq(10, 10, 15)]}], d, "mitre")
        buf(f"multi-{d}", [{"shell": sq(0, 0, 20)}, {"shell": sq(26, 3, 12)}, {"shell": sq(60, 0, 30), "holes": [sq(68, 8, 10)]}],
            d, "round")
    # stars both ways round, offsets both ways
    for k in range(12):
        s = ring(star(random.randint(4, 8), random.uniform(30, 60), random.uniform(5, 25), 100, 100, 0.3))
        if k % 2:
            s = s[::-1]
        for d in (0.0, 2.0, -4.5):
            buf(f"star-{k}-{d}", [{"shell": s}], d, "mitre" if k % 3 else "round")
    # traced outlines as the trace sees them (resampled, smoothed): strokes offset to their centres, then the
    # clean-up and offset of outlines with a one-pixel leader (the reference's --open 0)
    for t in range(3, 10):
        m = np.zeros((420, 520), np.uint8)
        cv2.rectangle(m, (60, 50), (460, 370), 255, t)
        for k, e in enumerate(V.find_piece_contours(m)):
            P = V._smooth_closed(V._resample_closed(e["contour"][:, 0, :].astype(float), 1.0), 1.2)
            off = -e["stroke_px"] / 2 if e["mode"] == "ring" else 0.5 - e["stroke_px"] / 2
            buf(f"stroke-rect{t}-{k}", [{"shell": ring(P)}], off, "mitre")
    for k in range(3):
        m = np.zeros((420, 420), np.uint8)
        cv2.fillPoly(m, [np.array(star(random.randint(5, 8), 160, 110, 210, 210, 0.15), np.int32)], 255)
        cv2.line(m, (210, 210), (410, 230 + 10 * k), 255, 1)
        for j, e in enumerate(V.find_piece_contours(m, open_px=0)):
            P = V._smooth_closed(V._resample_closed(e["contour"][:, 0, :].astype(float), 1.0), 1.2)
            buf(f"leader{k}-{j}-clean", [{"shell": ring(P)}], 0.0, "round")
            clean = geometry.Polygon(ring(P)).buffer(0)
            parts = list(clean.geoms) if clean.geom_type == "MultiPolygon" else [clean]
            buf(f"leader{k}-{j}-offset", [{"shell": [list(c) for c in p.exterior.coords],
                                           "holes": [[list(c) for c in h.coords] for h in p.interiors]} for p in parts],
                0.5, "mitre")
    path = args.out_dir / "buffer_topology.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "trace", "fixture": "buffer_topology", "meta": meta,
                                "cases": cases}, separators=(",", ":")) + "\n")
    print(f"wrote {len(cases)} cases to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
