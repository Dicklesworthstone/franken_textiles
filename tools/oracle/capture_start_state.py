#!/usr/bin/env python3
"""Capture start-state oracle fixtures from the Python reference (docs/spec/04 sections 3-4).

Runs in Blender's Python, like the reference's `blender_sew.py`:

    PYTHONDONTWRITEBYTECODE=1 Blender --background --factory-startup -noaudio --python-exit-code 1 \
        --python capture_start_state.py -- --reference SKILL/scripts OUT_DIR

For the 92 cm Zouave bolero and trousers of tests/fixtures/oracle/pattern/drafts.json (public reference
wearer) at 12 mm it records, per simulated instance, the part frame, the anchor height h0 and the wrap
radius the reference used (so the wrap formula can be checked without porting the dress form), the
wrapped start positions, the gather weights, and the pre-sewn and relaxed start states. The true form is
the reference's dress form at zero inset; its polygons are fan-triangulated here and that exact triangle
soup (recorded) is what the reference's pre-sew and relaxation run against, so both sides see the same
triangles.

The reference's nearest-point queries go through Blender's BVH in float32 (`mathutils`), and where the
nearest point lies on a crease the BVH keeps whichever tied triangle it meets first. So pre-sew and
relaxation are captured twice: once as the reference runs them ("blender"), and once with the same
reference functions but `mathutils.Vector` replaced by float64 arrays and the BVH by an exact
nearest-point query that breaks ties toward the best-aligned normal ("exact"). Writes
OUT_DIR/start_state.json; deterministic; no real person.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import platform
import sys
from pathlib import Path

SCHEMA = "ftx-oracle/1"
FILES = ("pattern_model.py", "blender_sew.py", "body.py")
DRAFTS = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "oracle" / "pattern" / "drafts.json"
CASES = ("zouave_bolero-920", "zouave_trousers-920")


class ExactNearest:
    """Nearest point on a triangle soup in float64 (Ericson's closest point, vectorised), ties within
    1e-12 relative broken toward the largest (p - q).n; the BVH's `find_nearest` interface."""

    def __init__(self, np, vertices, triangles):
        self.np = np
        v, t = np.asarray(vertices, float), np.asarray(triangles)
        self.A, self.B, self.C = v[t[:, 0]], v[t[:, 1]], v[t[:, 2]]
        n = np.cross(self.B - self.A, self.C - self.A)
        self.N = n / np.linalg.norm(n, axis=1)[:, None]

    def find_nearest(self, p):
        np = self.np
        p = np.asarray(p, float)
        A, B, C = self.A, self.B, self.C
        ab, ac, ap = B - A, C - A, p - A
        d1, d2 = (ab * ap).sum(1), (ac * ap).sum(1)
        bp, cp = p - B, p - C
        d3, d4 = (ab * bp).sum(1), (ac * bp).sum(1)
        d5, d6 = (ab * cp).sum(1), (ac * cp).sum(1)
        vc, vb, va = d1 * d4 - d3 * d2, d5 * d2 - d1 * d6, d3 * d6 - d5 * d4
        den = va + vb + vc
        den = np.where(den == 0, 1.0, den)
        q = A + ab * (vb / den)[:, None] + ac * (vc / den)[:, None]
        with np.errstate(divide="ignore", invalid="ignore"):
            m6 = (va <= 0) & ((d4 - d3) >= 0) & ((d5 - d6) >= 0)
            q[m6] = (B + (C - B) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)))[:, None])[m6]
            m5 = (vb <= 0) & (d2 >= 0) & (d6 <= 0)
            q[m5] = (A + ac * (d2 / (d2 - d6))[:, None])[m5]
            m3 = (vc <= 0) & (d1 >= 0) & (d3 <= 0)
            q[m3] = (A + ab * (d1 / (d1 - d3))[:, None])[m3]
        m4 = (d6 >= 0) & (d5 <= d6)
        q[m4] = C[m4]
        m2 = (d3 >= 0) & (d4 <= d3)
        q[m2] = B[m2]
        m1 = (d1 <= 0) & (d2 <= 0)
        q[m1] = A[m1]
        d = np.linalg.norm(p - q, axis=1)
        tie = np.where(d <= d.min() * (1 + 1e-12))[0]
        k = int(tie[np.argmax(((p - q[tie]) * self.N[tie]).sum(1))])
        return q[k].copy(), self.N[k].copy(), k, float(d[k])


def run_start(bs, np, G, wrapped, nearest, gather_w):
    presewn, groups = bs.presew(G, wrapped, nearest)
    rest_e = np.array(bs.face_edges(G["faces"]))
    plain = (gather_w[rest_e[:, 0]] >= 1.0) & (gather_w[rest_e[:, 1]] >= 1.0)
    start, relaxed, blocks = presewn, 0, []
    while relaxed < bs.RELAX_MAX_ITERS:
        start = bs.relax(G, start, groups, nearest, iters=200, weights=gather_w)
        relaxed += 200
        p95 = float(np.percentile(np.abs(bs.edge_strain(start, G["verts2d"], rest_e[plain])), 95))
        blocks.append(p95)
        if p95 <= bs.RELAX_TARGET_P95:
            break
    return {
        "presewn": np.asarray(presewn).tolist(),
        "relax_blocks_p95": blocks,
        "relaxed_iterations": relaxed,
        "relaxed": np.asarray(start).tolist(),
    }, groups


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--reference", required=True, type=Path)
    ap.add_argument("out_dir", type=Path)
    args = ap.parse_args(argv)
    for f in FILES:
        if not (args.reference / f).is_file():
            print(f"no {f} in {args.reference}", file=sys.stderr)
            return 2
    sys.path.insert(0, str(args.reference.resolve()))
    import bpy
    import numpy as np
    from mathutils.bvhtree import BVHTree

    import blender_sew as bs
    import body
    import pattern_model as pm

    drafts = {c["id"]: c for c in json.loads(DRAFTS.read_text())["cases"]}
    out_cases = []
    for cid in CASES:
        case = drafts[cid]
        pattern = case["input"]["pattern"]
        meas = case["input"]["measurements"]
        pat = pm.Pattern(json.loads(json.dumps(pattern)))
        G = bs.build_garment(pat, meas, 12.0)
        fr = body.frames(meas)
        instances = []
        for inst in G["insts"]:
            pl = bs._placement(inst)
            part = pl["part"]
            key = part if part == "torso" else f"{part}_R"
            lo, hi = G["inst_range"][inst.id]
            pts = G["verts2d"][lo:hi]
            if inst.mirrored:
                pts = body.mirror_x(np.c_[pts, np.zeros(len(pts))])[:, :2]
            h0 = body.landmark_h(meas, key, pl["y0_landmark"]) - pl.get("y0", 0.0)
            _, info = body.wrap(pts, meas, pl)
            f = fr[key]
            instances.append(
                {
                    "id": inst.id,
                    "part": key,
                    "mirrored": bool(inst.mirrored),
                    "side": inst.side,
                    "placement": pl,
                    "frame": {k: [float(x) for x in f[k]] for k in ("origin", "up", "front", "lat")},
                    "h0": float(h0),
                    "radius_mm": float(info["radius_mm"]),
                }
            )
        wrapped = bs.placed_positions(G, pat, meas)
        # The true form (zero inset), every part, fan-triangulated.
        parts = body.dress_form(meas, 0.0)
        tv, tf = [], []
        for name in sorted(parts):
            base = len(tv)
            tv += [[float(c) * bs.MM for c in v] for v in parts[name]["verts"]]
            for poly in parts[name]["faces"]:
                for k in range(1, len(poly) - 1):
                    tf.append([base + int(poly[0]), base + int(poly[k]), base + int(poly[k + 1])])
        bvh = BVHTree.FromPolygons([tuple(v) for v in tv], [tuple(t) for t in tf])
        gather_w = bs.gather_weights(G)
        blender, groups = run_start(bs, np, G, wrapped, bvh, gather_w)
        vector = bs.Vector
        bs.Vector = lambda v: np.asarray(v, dtype=float)  # float64 arithmetic in the reference's code
        try:
            exact, _ = run_start(bs, np, G, wrapped, ExactNearest(np, tv, tf), gather_w)
        finally:
            bs.Vector = vector
        out_cases.append(
            {
                "id": f"{cid}@12mm",
                "input": {"pattern": pattern, "spacing": 12.0},
                "output": {
                    "instances": instances,
                    "wrapped": np.asarray(wrapped).tolist(),
                    "gather_weights": np.asarray(gather_w).tolist(),
                    "form": {"vertices": tv, "triangles": tf},
                    "groups": [sorted(int(v) for v in g) for g in groups],
                    "blender": blender,
                    "exact": exact,
                },
            }
        )
        print(
            f"{cid}: blender {blender['relax_blocks_p95']}, exact {exact['relax_blocks_p95']}",
            file=sys.stderr,
        )

    meta = {
        "reference_files": {f: hashlib.sha256((args.reference / f).read_bytes()).hexdigest() for f in FILES},
        "drafts_fixture_sha256": hashlib.sha256(DRAFTS.read_bytes()).hexdigest(),
        "blender_version": bpy.app.version_string,
        "numpy_version": np.__version__,
        "python_version": platform.python_version(),
        "machine": platform.machine(),
        "math": f"pi={math.pi!r}",
    }
    out = {"schema": SCHEMA, "subsystem": "start_state", "fixture": "start_state", "meta": meta, "cases": out_cases}
    args.out_dir.mkdir(parents=True, exist_ok=True)
    (args.out_dir / "start_state.json").write_text(json.dumps(out, separators=(",", ":")) + "\n")
    return 0


if __name__ == "__main__":
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    raise SystemExit(main(argv))
