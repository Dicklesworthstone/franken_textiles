#!/usr/bin/env python3
"""Capture alteration oracle fixtures from the Python reference (docs/spec/01 §10).

Every case is (pattern [, source pattern], op, args) -> the reference's output document, its last history
sentence and the structure errors of the result (what `alter.py` prints and writes), or the error the
reference raises (type and message). Patterns are the drafted bolero and trousers of the public reference
wearers (80, 92 and 116 cm), plus one mutated bolero (front side seam 10 mm long).

    OUT_DIR/alterations.json

Usage: python capture_alterations.py --reference SKILL/scripts OUT_DIR   (deterministic; no real person)
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
FILES = ("alter.py", "pattern_model.py", "drafting.py", "measurements.py", "body.py")


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
    import alter
    import drafting
    import measurements
    from pattern_model import Pattern

    wearers = {s: measurements.reference_wearer(s, m) for s, m in ((800.0, 13.0), (920.0, 30.0), (1160.0, 72.0))}
    drafts = {}
    for stature, meas in wearers.items():
        for program in ("zouave_bolero", "zouave_trousers"):
            drafts[f"{program}-{int(stature)}"] = drafting.PROGRAMS[program](copy.deepcopy(meas), None)[0]
    m1 = copy.deepcopy(drafts["zouave_bolero-920"])
    next(p for p in m1["pieces"] if p["id"] == "front")["vertices"]["side_hem"][1] -= 10
    drafts["bolero_long_front_side-920"] = m1

    cases = []

    def add(cid, draft, op, op_args, src=None):
        pat = json.loads(json.dumps(drafts[draft]))
        src_pat = json.loads(json.dumps(drafts[src])) if src else None
        try:
            if op == "slash-spread":
                out = alter.slash_spread(copy.deepcopy(pat), **op_args)
            elif op == "move":
                out = alter.move(copy.deepcopy(pat), **op_args)
            elif op == "true-seam":
                out = alter.true_seam(copy.deepcopy(pat), **op_args)
            elif op == "cap-ease":
                out = alter.cap_ease(copy.deepcopy(pat), **op_args)
            else:
                out = alter.transfer_curve(copy.deepcopy(pat), copy.deepcopy(src_pat), **op_args)
            output = {"doc": json.loads(json.dumps(out)), "why": out["meta"]["history"][-1]["why"],
                      "structure_errors": Pattern(copy.deepcopy(out)).structure_errors()}
        except (KeyError, ValueError) as e:
            output = {"error": {"type": type(e).__name__, "message": str(e.args[0]) if e.args else ""}}
        inp = {"pattern": pat, "op": op, "args": op_args}
        if src_pat is not None:
            inp["source"] = src_pat
        cases.append({"id": cid, "input": inp, "output": output})

    # slash-spread: width, removed length, a sleeve with decor, trousers, every size
    add("slash_front_width-920", "zouave_bolero-920", "slash-spread", {"piece": "front", "axis": "x", "at": 80.0, "add": 12.0})
    add("slash_back_length_remove-920", "zouave_bolero-920", "slash-spread", {"piece": "back", "axis": "y", "at": 100.0, "add": -8.0})
    add("slash_sleeve_length-800", "zouave_bolero-800", "slash-spread", {"piece": "sleeve", "axis": "y", "at": 150.0, "add": 20.0})
    add("slash_trouser_front-1160", "zouave_trousers-1160", "slash-spread", {"piece": "tr_front", "axis": "x", "at": 100.0, "add": 15.5})
    add("slash_missing_piece", "zouave_bolero-920", "slash-spread", {"piece": "nope", "axis": "x", "at": 1.0, "add": 1.0})
    # move
    add("move_sleeve_top-920", "zouave_bolero-920", "move", {"piece": "sleeve", "vertex": "top", "dx": 0.0, "dy": 6.0})
    add("move_front_tip-1160", "zouave_bolero-1160", "move", {"piece": "front", "vertex": "tip", "dx": 3.5, "dy": -2.25})
    add("move_missing_vertex", "zouave_bolero-920", "move", {"piece": "front", "vertex": "nope", "dx": 1.0, "dy": 0.0})
    # true-seam: no ratio, a ratio band (its mean wins over the declared ratio), both ends, a mutation, the .L refusal
    add("true_side_R_end-920", "zouave_bolero-920", "true-seam", {"seam_id": "side_R", "end": "end"})
    add("true_side_R_mutated-920", "bolero_long_front_side-920", "true-seam", {"seam_id": "side_R", "end": "end"})
    add("true_shoulder_R_end-800", "zouave_bolero-800", "true-seam", {"seam_id": "shoulder_R", "end": "end"})
    add("true_shoulder_R_start-1160", "zouave_bolero-1160", "true-seam", {"seam_id": "shoulder_R", "end": "start"})
    add("true_inseam_R_end-920", "zouave_trousers-920", "true-seam", {"seam_id": "inseam_R", "end": "end"})
    add("true_side_L_refused", "zouave_bolero-920", "true-seam", {"seam_id": "side_L", "end": "end"})
    # cap-ease
    add("cap_ease_1045-920", "zouave_bolero-920", "cap-ease", {"piece": "sleeve", "seams": ["armhole_f_R", "armhole_b_R"], "target": 1.045})
    add("cap_ease_1070-800", "zouave_bolero-800", "cap-ease", {"piece": "sleeve", "seams": ["armhole_b_R", "armhole_f_R"], "target": 1.07})
    add("cap_ease_1020-1160", "zouave_bolero-1160", "cap-ease", {"piece": "sleeve", "seams": ["armhole_f_R", "armhole_b_R"], "target": 1.02})
    # transfer-curve: a cubic source, a curve from another garment, a line source, a missing source instance
    add("transfer_cap_to_armhole-920", "zouave_bolero-920", "transfer-curve",
        {"src_ref": "sleeve.R/cap_f", "dst_ref": "front/armhole"}, src="zouave_bolero-920")
    add("transfer_crotch_to_neck-920", "zouave_bolero-920", "transfer-curve",
        {"src_ref": "tr_front.R/crotch", "dst_ref": "front/neck"}, src="zouave_trousers-920")
    add("transfer_line_source-800", "zouave_bolero-800", "transfer-curve",
        {"src_ref": "front.R/side", "dst_ref": "back/hem"}, src="zouave_bolero-800")
    add("transfer_missing_source", "zouave_bolero-920", "transfer-curve",
        {"src_ref": "nope/side", "dst_ref": "front/neck"}, src="zouave_bolero-920")

    meta = {
        "reference_files": {f: hashlib.sha256((args.reference / f).read_bytes()).hexdigest() for f in FILES},
        "numpy_version": np.__version__, "python_version": platform.python_version(), "machine": platform.machine(),
    }
    args.out_dir.mkdir(parents=True, exist_ok=True)
    path = args.out_dir / "alterations.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "alter", "fixture": "alterations", "meta": meta,
                                "cases": cases}, separators=(",", ":"), ensure_ascii=False) + "\n")
    print(f"wrote {len(cases)} cases to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
