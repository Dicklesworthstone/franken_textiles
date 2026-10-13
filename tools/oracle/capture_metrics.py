#!/usr/bin/env python3
"""Capture drape-metric oracle fixtures from the Python reference (docs/spec/04 section 6).

Runs in Blender's Python, like the reference's `blender_sew.py`:

    PYTHONDONTWRITEBYTECODE=1 Blender --background --factory-startup -noaudio --python-exit-code 1 \
        --python capture_metrics.py -- --reference SKILL/scripts OUT_DIR

The reference's `metrics()` turns a baked frame sequence into `sim_report.json` fields and flags. Baking
a Blender cloth sequence takes minutes and is not what is being tested, so each case feeds `metrics()` a
synthetic sequence built from the exact start state recorded in start_state.json ("exact" run) by a
recipe of additions and multiplications only (repeated here and in the Rust test, so both build the same
bits):

    frame k of N, t = k / (N - 1):
        x = relaxed + t * shift[v]        (per-vertex shift vector, by recipe below)
        stretch about the torso axis (0, y_c):  x' = (x * s, y_c + (y - y_c) * s, z), s = 1 + t * stretch
        jitter in the last W frames:            z += (+1 or -1) * jitter

Recipes per case: a sag of the whole garment, one instance moved sideways (opens its seams), one instance
moved toward the body axis (penetration), a radial stretch (tight rings), jitter. Signed distances use an
exact float64 nearest-point query (best-aligned normal on ties) against the recorded dress form, with
`mathutils.Vector` replaced by float64 arrays, as in capture_start_state.py. The girth-level piece heights
the reference computes (`_level_y`) are recorded per point and instance, so the Rust side needs no dress
form. Writes OUT_DIR/metrics.json; deterministic; no real person.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import sys
from pathlib import Path

SCHEMA = "ftx-oracle/1"
FILES = ("pattern_model.py", "blender_sew.py", "body.py")
ROOT = Path(__file__).resolve().parents[2]
DRAFTS = ROOT / "tests" / "fixtures" / "oracle" / "pattern" / "drafts.json"
START = ROOT / "tests" / "fixtures" / "oracle" / "drape" / "start_state.json"
sys.path.insert(0, str(Path(__file__).resolve().parent))

# (case id, drafted case, frames, sew_end, recipe)
CASES = [
    ("bolero_settled", "zouave_bolero-920", 40, 20,
     {"sag": 0.0, "move": None, "push": None, "stretch": 0.0, "jitter": 0.0002}),
    ("bolero_slipping", "zouave_bolero-920", 40, 20,
     {"sag": -0.06, "move": ["back", 0.009], "push": ["front.R", 0.02], "stretch": 0.0, "jitter": 0.0}),
    ("bolero_tight", "zouave_bolero-920", 30, 15,
     {"sag": 0.0, "move": None, "push": None, "stretch": 0.05, "jitter": 0.0001}),
    ("trousers_slipping", "zouave_trousers-920", 40, 20,
     {"sag": -0.04, "move": ["tr_back.L", 0.007], "push": None, "stretch": 0.03, "jitter": 0.0}),
]


def build_seq(np, relaxed, ranges, recipe, n_frames, y_c):
    shift = np.zeros_like(relaxed)
    shift[:, 2] = recipe["sag"]
    if recipe["move"]:
        lo, hi = ranges[recipe["move"][0]]
        shift[lo:hi, 0] = shift[lo:hi, 0] + recipe["move"][1]
    if recipe["push"]:
        lo, hi = ranges[recipe["push"][0]]
        shift[lo:hi, 1] = shift[lo:hi, 1] + recipe["push"][1]
    w = min(10, max(2, n_frames // 6))
    seq = []
    for k in range(n_frames):
        t = k / (n_frames - 1)
        x = relaxed + t * shift
        s = 1.0 + t * recipe["stretch"]
        x = np.stack([x[:, 0] * s, y_c + (x[:, 1] - y_c) * s, x[:, 2]], axis=1)
        if k >= n_frames - w and recipe["jitter"]:
            x[:, 2] = x[:, 2] + (1.0 if k % 2 else -1.0) * recipe["jitter"]
        seq.append(x)
    return seq


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

    import blender_sew as bs
    import pattern_model as pm
    from capture_start_state import ExactNearest

    drafts = {c["id"]: c for c in json.loads(DRAFTS.read_text())["cases"]}
    starts = {c["id"].split("@")[0]: c for c in json.loads(START.read_text())["cases"]}
    bs.Vector = lambda v: np.asarray(v, dtype=float)

    class Args:
        spacing = 12.0

    out_cases = []
    for cid, did, n_frames, sew_end, recipe in CASES:
        case = drafts[did]
        pattern = case["input"]["pattern"]
        meas = case["input"]["measurements"]
        spec = case["input"]["fit_spec"]
        pat = pm.Pattern(json.loads(json.dumps(pattern)))
        G = bs.build_garment(pat, meas, 12.0)
        st = starts[did]["output"]
        relaxed = np.asarray(st["exact"]["relaxed"], float)
        wrapped = np.asarray(st["wrapped"], float)
        form = ExactNearest(np, st["form"]["vertices"], st["form"]["triangles"])
        y_c = 0.0  # the reference's torso axis is x = 0, y = 0
        seq = build_seq(np, relaxed, G["inst_range"], recipe, n_frames, y_c)
        rep, _, _ = bs.metrics(G, pat, meas, spec, seq, sew_end, form, Args, wrapped)
        levels = {}
        for p in spec.get("points", []):
            if p.get("kind") != "girth":
                continue
            levels[p["id"]] = {
                iid: bs._level_y(pat, iid, meas, p["landmark"]) for iid in p["instances"] if iid in G["inst_range"]
            }
        out_cases.append(
            {
                "id": cid,
                "input": {
                    "draft": did,
                    "frames": n_frames,
                    "sew_end": sew_end,
                    "recipe": recipe,
                    "y_c": y_c,
                    "spacing": 12.0,
                    "arm_deg": 35.0,
                    "levels": levels,
                },
                "output": {"report": json.loads(json.dumps(rep))},
            }
        )
        print(f"{cid}: {len(rep['flags'])} flags, {len(rep['gating'])} gating", file=sys.stderr)

    meta = {
        "reference_files": {f: hashlib.sha256((args.reference / f).read_bytes()).hexdigest() for f in FILES},
        "drafts_fixture_sha256": hashlib.sha256(DRAFTS.read_bytes()).hexdigest(),
        "start_state_fixture_sha256": hashlib.sha256(START.read_bytes()).hexdigest(),
        "blender_version": bpy.app.version_string,
        "numpy_version": np.__version__,
        "python_version": platform.python_version(),
        "machine": platform.machine(),
    }
    out = {"schema": SCHEMA, "subsystem": "metrics", "fixture": "metrics", "meta": meta, "cases": out_cases}
    args.out_dir.mkdir(parents=True, exist_ok=True)
    (args.out_dir / "metrics.json").write_text(json.dumps(out, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    raise SystemExit(main(argv))
