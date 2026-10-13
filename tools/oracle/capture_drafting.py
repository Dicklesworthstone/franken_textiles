#!/usr/bin/env python3
"""Capture the reference's analytic body and drafting programs (docs/spec/02 Parts B and C) as oracle fixtures.

Writes, from the reference's body.py, drafting.py, pattern_model.py and measurements.py (the fabric-pattern
skill's scripts):
    OUT_DIR/body.json      landmarks, torso sections, part frames, section radii, landmark heights and the form's
                           crotch path for the reference wearers at 800/13, 920/30 and 1160/72 (mm/months) and a
                           flat measurement dict without a total crotch length
    OUT_DIR/programs.json  drafts with style overrides (belly room, no soutache, no motifs, nappy, knee and ankle
                           cuffs, a tall fez, a four-arc fez), drafts from flat dicts (custom size label), refusals
                           (misspelt style keys, fez ranges, a missing measurement) and, for every bolero, the
                           soutache border's offset curve and clipped line before rounding

The reference is identified by the SHA-256 of its files, not by a path. Usage:
    python capture_drafting.py --reference SKILL/scripts OUT_DIR
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
FILES = ("body.py", "drafting.py", "pattern_model.py", "measurements.py")
WEARERS = ((800.0, 13.0), (920.0, 30.0), (1160.0, 72.0))


def tolist(a):
    return np.asarray(a, float).tolist()


def flat(meas: dict) -> dict:
    return {k: (v.get("value_mm", v.get("value_deg"))) for k, v in meas["fused"].items()}


def body_case(body, meas: dict) -> dict:
    L = body.landmarks(meas)
    fr = body.frames(meas)
    frames = {k: {kk: (tolist(vv) if isinstance(vv, np.ndarray) else float(vv)) for kk, vv in v.items()}
              for k, v in fr.items()}
    hs = [float(h) for h in np.linspace(L["crotch"] - 40.0, L["snp"] + 30.0, 23)]
    limb = [float(h) for h in np.linspace(-450.0, 60.0, 18)]
    radii = {"torso": [[h, body.section_radius(meas, "torso", h)] for h in hs],
             "arm_R": [[h, body.section_radius(meas, "arm_R", h)] for h in limb[:3]]}
    for part in ("leg_R", "leg_L"):
        radii[part] = [[h, body.section_radius(meas, part, h)] for h in limb]
    lh = {"torso": {n: body.landmark_h(meas, "torso", n) for n in ("waist", "chest", "hip", "shoulder", "across_back")}}
    for part in ("arm_R", "arm_L"):
        lh[part] = {n: body.landmark_h(meas, part, n) for n in ("shoulder_tip", "elbow", "wrist")}
    for part in ("leg_R", "leg_L"):
        lh[part] = {n: body.landmark_h(meas, part, n) for n in ("waist", "hip", "knee", "calf", "ankle", "crotch", "floor")}
    return {"landmarks": L, "sections": [list(s) for s in body.torso_sections(meas)], "frames": frames,
            "section_radius": radii, "landmark_h": lh, "crotch_path_mm": body.crotch_path_mm(meas)}


def soutache_trace(drafting, pat: dict) -> dict:
    """The border's offset curve and clipped line before rounding, per piece, as _braid_border computes them."""
    from shapely.geometry import LineString, Polygon
    s = pat["meta"]["style"]
    pieces = {p["id"]: p for p in pat["pieces"]}
    out = {}
    for pid, chain, inset in (("front", ["hem", "front_edge", "neck"], s["soutache_inset"]),
                              ("back", ["hem"], s["soutache_inset"]), ("sleeve", ["hem"], s["sleeve_band"])):
        outline, edges = drafting._piece_outline(pieces[pid])
        pts = np.concatenate([edges[chain[0]]] + [edges[e][1:] for e in chain[1:]])
        t0, t1 = pts[0] - pts[1], pts[-1] - pts[-2]
        ext = np.vstack([pts[0] + 3 * inset * t0 / np.linalg.norm(t0), pts, pts[-1] + 3 * inset * t1 / np.linalg.norm(t1)])
        line = LineString(ext).offset_curve(inset, join_style="mitre", mitre_limit=3.0)
        cut = Polygon(outline).intersection(line)
        parts = [g for g in getattr(cut, "geoms", [cut]) if g.geom_type == "LineString" and g.length > 0]
        out[pid] = {"ext": tolist(ext), "outline": tolist(outline), "inset": inset,
                    "offset": [tolist(g.coords) for g in getattr(line, "geoms", [line])],
                    "parts": [tolist(g.coords) for g in parts]}
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--reference", required=True, type=Path)
    ap.add_argument("out_dir", type=Path)
    args = ap.parse_args()
    sys.path.insert(0, str(args.reference.resolve()))
    import body
    import drafting
    import measurements

    meta = {"reference_files": {f: hashlib.sha256((args.reference / f).read_bytes()).hexdigest() for f in FILES},
            "numpy_version": np.__version__, "python_version": platform.python_version(), "machine": platform.machine()}
    try:
        import shapely
        meta["geos_version"] = shapely.geos_version_string
    except ImportError:
        pass

    wearers = {int(s): measurements.reference_wearer(s, m) for s, m in WEARERS}
    no_crotch = flat(wearers[920])
    del no_crotch["total_crotch_length"]
    bcases = [{"id": f"wearer-{k}", "input": {"measurements": m}, "output": body_case(body, m)} for k, m in wearers.items()]
    bcases.append({"id": "flat-920-no-crotch-length", "input": {"measurements": no_crotch},
                   "output": body_case(body, no_crotch)})

    runs = []
    for k in wearers:
        for program in sorted(drafting.PROGRAMS):
            runs.append((f"{program}-{k}", program, wearers[k], None))
    m920 = wearers[920]
    runs += [
        ("zouave_bolero-920-belly", "zouave_bolero", m920, {"front_belly_mm": 20.0}),
        ("zouave_bolero-920-plain", "zouave_bolero", m920, {"soutache": False}),
        ("zouave_bolero-920-no-motif", "zouave_bolero", m920, {"motif": 0.0}),
        ("zouave_bolero-920-roomy", "zouave_bolero", m920, {"chest_ease": 140.0, "cap_ease": 0.06, "front_cutaway": 70.0}),
        ("zouave_bolero-1160-flat", "zouave_bolero", flat(wearers[1160]), None),
        ("zouave_trousers-920-nappy", "zouave_trousers", m920, {"nappy": True}),
        ("zouave_trousers-920-knee", "zouave_trousers", m920, {"cuff_level": "knee"}),
        ("zouave_trousers-920-ankle", "zouave_trousers", m920, {"cuff_level": "ankle", "leg_gather": 1.5}),
        ("fez-920-tall", "fez", m920, {"height_frac": 1.2, "top_frac": 0.9}),
        ("fez-920-four-arcs", "fez", m920, {"height_frac": 0.25, "top_frac": 0.4}),
        ("fez-custom", "fez", {"head": 480.0}, None),
        # refusals
        ("zouave_bolero-920-misspelt", "zouave_bolero", m920, {"chest_eas": 100.0, "cap": 1}),
        ("zouave_trousers-920-misspelt", "zouave_trousers", m920, {"nappies": True}),
        ("fez-920-top-frac", "fez", m920, {"top_frac": 0.99}),
        ("fez-920-height-frac", "fez", m920, {"height_frac": 3.0}),
        ("fez-920-head-ease", "fez", m920, {"head_ease": 50.0}),
        # the widest sector the ranges allow (314 deg): the taper refusal (> 324 deg) cannot be reached
        ("fez-920-widest-sector", "fez", m920, {"height_frac": 0.2, "top_frac": 0.3}),
        ("zouave_bolero-missing", "zouave_bolero", {k: v for k, v in flat(m920).items() if k != "across_back"}, None),
    ]
    pcases = []
    for cid, program, meas, style in runs:
        case = {"id": cid, "input": {"program": program, "measurements": meas, "style": style}}
        try:
            pat, spec = drafting.PROGRAMS[program](copy.deepcopy(meas), copy.deepcopy(style))
        except (ValueError, KeyError) as exc:
            case["output"] = {"error": type(exc).__name__, "message": str(exc)}
        else:
            pat = json.loads(json.dumps(pat))
            case["output"] = {"pattern": pat, "fit_spec": json.loads(json.dumps(spec))}
            if program == "zouave_bolero" and pat["meta"]["style"]["soutache"]:
                case["output"]["soutache_trace"] = soutache_trace(drafting, pat)
        pcases.append(case)

    args.out_dir.mkdir(parents=True, exist_ok=True)
    for name, cases in (("body", bcases), ("programs", pcases)):
        path = args.out_dir / f"{name}.json"
        path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "draft", "fixture": name, "meta": meta, "cases": cases},
                                   separators=(",", ":")) + "\n")
        print(f"wrote {len(cases)} cases to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
