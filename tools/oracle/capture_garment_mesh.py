#!/usr/bin/env python3
"""Capture garment-mesh oracle fixtures from the Python reference (docs/spec/04 section 2).

The reference builds its cloth mesh inside Blender (`blender_sew.build_garment`, with Blender's
`delaunay_2d_cdt`), so this script runs in Blender's own Python:

    PYTHONDONTWRITEBYTECODE=1 Blender --background --factory-startup -noaudio --python-exit-code 1 \
        --python capture_garment_mesh.py -- --reference SKILL/scripts OUT_DIR

Cases: the six simulated drafted garments of tests/fixtures/oracle/pattern/drafts.json (Zouave bolero and
trousers for public reference wearers of 80, 92 and 116 cm) at the default 12 mm spacing, and the 92 cm
bolero at 8 mm. Each records the 2D vertices (after sim_scale), triangles, vertex owners, instance ranges,
edge index lists, corners and seam records. Writes OUT_DIR/garment_mesh.json; deterministic; no real person.
The reference is identified by the SHA-256 of its files, never by a path.
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
DRAFTS = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "oracle" / "pattern" / "drafts.json"


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

    import blender_sew
    import pattern_model as pm

    drafts = json.loads(DRAFTS.read_text())
    runs = [(c, 12.0) for c in drafts["cases"] if not c["id"].startswith("fez")]
    runs += [(c, 8.0) for c in drafts["cases"] if c["id"] == "zouave_bolero-920"]
    cases = []
    for case, spacing in runs:
        pattern = case["input"]["pattern"]
        meas = case["input"]["measurements"]
        g = blender_sew.build_garment(pm.Pattern(json.loads(json.dumps(pattern))), meas, spacing)
        verts = np.asarray(g["verts2d"], dtype=np.float64)
        cases.append(
            {
                "id": f"{case['id']}@{spacing:g}mm",
                "input": {"pattern": pattern, "spacing": spacing},
                "output": {
                    "instances": [i.id for i in g["insts"]],
                    "verts2d": verts.tolist(),
                    "faces": [[int(j) for j in f] for f in g["faces"]],
                    "owner": list(g["owner"]),
                    "inst_range": {k: [int(a), int(b)] for k, (a, b) in g["inst_range"].items()},
                    "pairs": [[int(a), int(b)] for a, b in g["pairs"]],
                    "seams": [
                        {
                            "id": sid,
                            "pairs": [[int(a), int(b)] for a, b in rec["pairs"]],
                            "side_a": [int(j) for j in rec["side_a"]],
                            "side_b": [int(j) for j in rec["side_b"]],
                            "ratio": rec["ratio"],
                        }
                        for sid, rec in g["seam_pairs"].items()
                    ],
                    "corner": [[i, v, int(j)] for (i, v), j in g["corner"].items()],
                    "index": [[i, e, [int(j) for j in idx]] for (i, e), idx in g["index"].items()],
                },
            }
        )
        print(f"{cases[-1]['id']}: {len(verts)} vertices, {len(g['faces'])} triangles", file=sys.stderr)

    meta = {
        "reference_files": {f: hashlib.sha256((args.reference / f).read_bytes()).hexdigest() for f in FILES},
        "drafts_fixture_sha256": hashlib.sha256(DRAFTS.read_bytes()).hexdigest(),
        "blender_version": bpy.app.version_string,
        "numpy_version": np.__version__,
        "python_version": platform.python_version(),
        "machine": platform.machine(),
    }
    out = {"schema": SCHEMA, "subsystem": "garment_mesh", "fixture": "garment_mesh", "meta": meta, "cases": cases}
    args.out_dir.mkdir(parents=True, exist_ok=True)
    (args.out_dir / "garment_mesh.json").write_text(json.dumps(out, separators=(",", ":")) + "\n")
    return 0


if __name__ == "__main__":
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    raise SystemExit(main(argv))
