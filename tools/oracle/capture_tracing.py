#!/usr/bin/env python3
"""Capture the reference's post-trace commands (docs/spec/04 §10.10–10.15) as oracle fixtures.

Traces the reference's own synthetic test sheets (its tests/test_vectorize.py: a bodice front on the fold, a back
rectangle and a sleeve, drawn black on white, white on navy, and filled red with dark outlines), then records:
    parse_length   inputs with and without a default unit
    measure_ref    every reference form on traced drafts, annotated patterns and a drafted (mm) pattern, refusals
    annotate       names.json applied to the drafts (renames, cut, fold, mirroring, split, merge, seams with twins,
                   checks, drops) and every refusal the reference raises
    scale_fit      single and several targets, a disagreeing pair (warning), refusals
    audit          annotated, labelled, inventory, symmetric, fold, cut-off, nominal and fitted cases
Annotated patterns are stored once (`annotated`) and referenced by later cases.

Writes OUT_DIR/post.json. The reference is identified by the SHA-256 of its files, not by a path. Usage:
    python capture_tracing.py --reference SKILL/scripts OUT_DIR
Needs the reference's requirements (numpy, shapely, opencv-python-headless, pillow, pytest). Deterministic.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import platform
import sys
import tempfile
from pathlib import Path

import numpy as np

SCHEMA = "ftx-oracle/1"
FILES = ("vectorize.py", "pattern_model.py", "tests/test_vectorize.py")


def clean(x):
    """JSON-safe: tuples to lists, numpy scalars to Python, non-finite floats to None."""
    if isinstance(x, dict):
        return {str(k): clean(v) for k, v in x.items()}
    if isinstance(x, (list, tuple)):
        return [clean(v) for v in x]
    if isinstance(x, np.ndarray):
        return clean(x.tolist())
    if isinstance(x, (np.floating, float)):
        return float(x) if math.isfinite(float(x)) else None
    if isinstance(x, (np.integer,)):
        return int(x)
    if isinstance(x, np.bool_):
        return bool(x)
    return x


def outcome(fn):
    try:
        return {"ok": clean(fn())}
    except (KeyError, ValueError) as exc:
        return {"error": type(exc).__name__, "message": str(exc)}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--reference", required=True, type=Path)
    ap.add_argument("out_dir", type=Path)
    args = ap.parse_args()
    ref = args.reference.resolve()
    sys.path.insert(0, str(ref))
    sys.path.insert(0, str(ref / "tests"))
    import pattern_model as pm
    import test_vectorize as T
    import vectorize as V
    import shapely

    meta = {"reference_files": {f: hashlib.sha256((ref / f).read_bytes()).hexdigest() for f in FILES},
            "numpy_version": np.__version__, "python_version": platform.python_version(),
            "machine": platform.machine(), "geos_version": shapely.geos_version_string}

    gt = {p["id"]: p for p in T.gt_pieces()}
    drafts, matched = {}, {}
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        for name, kw, trace_kw in (("plain", {}, {}),
                                   ("navy", {"fg": (255, 255, 255), "bg": (25, 35, 80)}, {}),
                                   ("filled", {"fg": (30, 20, 20), "fill": (175, 30, 35)}, {"stroke": T.STROKE_PX})):
            sheet = T.render_sheet(tmp / f"{name}.png", **kw)
            res = V.trace_image(str(sheet), tmp / name, **trace_kw)
            data = json.loads(json.dumps(clean(res["pattern"])))
            data["meta"]["source"]["image"] = f"{name}.png"     # no temporary paths in the fixture
            data["name"] = f"draft traced from {name}.png"
            drafts[name] = data
            matched[name] = T.match_pieces(data)

    front, back, sleeve = T.gt_pieces()
    front["cut"] = {"qty": 1, "on_fold": "cf"}
    front["role"], back["role"], sleeve["role"] = "bodice_front", "bodice_back", "sleeve"
    back["cut"], sleeve["cut"] = {"qty": 2}, {"qty": 2}
    drafts["drafted"] = clean({"schema": pm.SCHEMA, "units": "mm", "name": "drafted test bodice",
                               "pieces": [front, back, sleeve],
                               "seams": [{"id": "side", "a": "front/side.R", "b": "back.R/right"},
                                         {"id": "underarm", "a": "sleeve.R/ur", "b": "sleeve.R/ul"}],
                               "checks": [{"id": "cap", "a": ["sleeve/cap"], "b": ["front/armhole"], "band": [2.0, 2.6]}]})

    def edge(name, gid, eid):
        return T.match_edge(gt[gid], eid, matched[name][gid])

    def pid(name, gid):
        return matched[name][gid]["id"]

    def vpair(name, gid, a, b):
        tp = matched[name][gid]

        def near(v):
            q = T.sheet_xy(gid, [gt[gid]["vertices"][v]])[0]
            return min(tp["vertices"], key=lambda k: np.linalg.norm(V._affine(tp["trace"]["to_px"], [tp["vertices"][k]])[0] - q))
        return f"{tp['id']}:{near(a)}-{near(b)}"

    # ------------------------------------------------------------------ parse_length
    cases = []
    texts = ["11 in", '10.5"', "10 1/2 in", "28cm", "12", "Cut 2.6 fabric", "279.4", "1,5 m", "3 inches", "2''",
             "7 ″", "5”", "abc", "", "10 1/0 in", "0.5m", "12 MM", "4 Inch", "about 30 cm wide", "1 m 20",
             "2 1/4", "6 inch hem", "0", "-5 cm"]
    for t in texts:
        for unit in ("mm", None):
            cases.append({"kind": "parse_length", "id": f"parse-{len(cases)}", "input": {"text": t, "default_unit": unit},
                          "output": V.parse_length(t, default_unit=unit)})
    for num in (12, 2.5):
        for unit in ("mm", None):
            cases.append({"kind": "parse_length", "id": f"parse-{len(cases)}", "input": {"number": num, "default_unit": unit},
                          "output": V.parse_length(num, default_unit=unit)})

    # ------------------------------------------------------------------ annotate
    annotated = {}

    def annotate_case(cid, draft, names, keep=True):
        src = drafts[draft] if isinstance(draft, str) else annotated[draft["annotated"]]
        out = outcome(lambda: V.annotate(copy.deepcopy(src), copy.deepcopy(names)))
        case = {"kind": "annotate", "id": cid, "input": {"pattern": draft if not isinstance(draft, str) else {"draft": draft},
                                                          "names": copy.deepcopy(names)}}
        if "ok" in out and keep:
            annotated[cid] = json.loads(json.dumps(out["ok"]))
            case["output"] = {"annotated": cid}
        else:
            if keep:
                print(f"{cid}: refused: {out['message']}", file=sys.stderr)
            case["output"] = out
        cases.append(case)

    names = {}
    for name in ("plain", "navy", "filled"):
        names[name] = T.names_for(matched[name], gt)
        annotate_case(f"ann-{name}", name, names[name])
    n = copy.deepcopy(names["plain"])
    n["mirror_seams"] = False
    annotate_case("ann-no-mirror", "plain", n)
    annotate_case("ann-mirror-left", "plain", {"pieces": {pid("plain", "front"): {"drawn_side": "L"},
                                                          pid("plain", "sleeve"): {"drawn_side": "l", "cut": {"qty": 2}}}})
    arm = edge("plain", "front", "armhole")
    fid = pid("plain", "front")
    annotate_case("ann-split", "plain", {"pieces": {fid: {"split": {arm: [0.5, 0.25]}}}})
    annotate_case("ann-split-one", "plain", {"pieces": {fid: {"split": {arm: 0.4}}}})
    annotate_case("ann-merge", {"annotated": "ann-split"}, {"pieces": {fid: {"merge_at": [arm + "s1", arm + "s2"]}}})
    first_v = list(matched["plain"]["back"]["vertices"])[1]
    annotate_case("ann-merge-rect", "plain", {"pieces": {pid("plain", "back"): {"merge_at": [first_v]}}})
    n = copy.deepcopy(names["plain"])
    n["seams"] = [s for s in n["seams"] if s["id"] != "cap"]
    del n["pieces"][pid("plain", "back")]["role"]
    n["pieces"][fid]["cut"]["on_fold"] = edge("plain", "front", "shoulder")
    annotate_case("ann-inventory", "plain", n)
    n = copy.deepcopy(names["plain"])
    sid = pid("plain", "sleeve")
    n["seams"].append({"id": "eased", "a": f"{sid}/{edge('plain', 'sleeve', 'cap')}",
                       "b": f"{fid}/{edge('plain', 'front', 'armhole')}", "band": [1.0, 1.2], "note": "cap ease"})
    n["seams"].append({"a": f"{pid('plain', 'back')}/{edge('plain', 'back', 'left')}",
                       "b": f"{fid}/{edge('plain', 'front', 'side')}", "ratio": 1.1})
    n["checks"] = [{"a": [f"{sid}/{edge('plain', 'sleeve', 'cap')}"], "b": [f"{fid}/{edge('plain', 'front', 'armhole')}"],
                    "band": [2.0, 2.6]},
                   {"id": "w", "a": [f"{pid('plain', 'back')}:width"], "b": [f"{fid}:width"], "ratio": 1.0, "note": "widths"}]
    n["pieces"][pid("plain", "back")]["edges"] = {edge("plain", "back", "bottom"): "hem",
                                                    edge("plain", "back", "right"): "side"}
    n["pieces"][pid("plain", "back")]["vertices"] = {first_v: "corner"}
    n["pieces"][pid("plain", "back")].update({"symmetric": True, "notes": "rectangle", "material": "main"})
    annotate_case("ann-reused", "plain", n, keep=False)          # edges sewn twice: the structure check refuses
    n["seams"] = [x for x in n["seams"] if x.get("id") != "cap"]
    n["seams"][-1]["b"] = f"{sid}/{edge('plain', 'sleeve', 'hem')}"
    annotate_case("ann-full", "plain", n)
    annotate_case("ann-drop", "plain", {"garment": "two pieces", "drop": [sid],
                                        "pieces": {fid: {"id": "front", "role": "bodice_front"}}})
    annotate_case("ann-drop-flag", "plain", {"pieces": {sid: {"drop": True}}})
    # refusals
    bad = [
        ("bad-edge", {"seams": [{"id": "x", "a": "p1/e99", "b": "p2/e1"}]}),
        ("bad-piece", {"pieces": {"p9": {"id": "x"}}}),
        ("bad-drop", {"drop": ["p7"]}),
        ("bad-id", {"pieces": {"p1": {"id": "front.R"}}}),
        ("bad-edge-name", {"pieces": {"p1": {"edges": {"e1": "a b"}}}}),
        ("bad-split", {"pieces": {fid: {"split": {arm: [1.2]}}}}),
        ("bad-split-edge", {"pieces": {fid: {"split": {"e99": [0.5]}}}}),
        ("bad-merge", {"pieces": {fid: {"merge_at": ["c99"]}}}),
        ("bad-rename-missing", {"pieces": {"p1": {"edges": {"e99": "x"}}}}),
        ("bad-vertex-missing", {"pieces": {"p1": {"vertices": {"c99": "x"}}}}),
        ("bad-collide", {"pieces": {"p1": {"edges": {"e1": "e2"}}}}),
        ("bad-vertex-collide", {"pieces": {"p1": {"vertices": {"c1": "c2"}}}}),
        ("bad-qty", {"pieces": {"p1": {"cut": {"qty": 3}}}}),
        ("bad-fold", {"pieces": {"p1": {"cut": {"qty": 1, "on_fold": "e99"}}}}),
        ("bad-side", {"pieces": {"p1": {"drawn_side": "X"}}}),
        ("bad-piece-collide", {"pieces": {"p1": {"id": "x"}, "p2": {"id": "x"}}}),
        ("bad-fold-sewn", {"pieces": {fid: {"cut": {"qty": 1, "on_fold": arm}}},
                           "seams": [{"a": f"{fid}/{arm}", "b": f"{sid}/{edge('plain', 'sleeve', 'cap')}"}]}),
        ("bad-side-ref", {"seams": [{"a": "p1", "b": "p2/e1"}]}),
        ("bad-check", {"checks": [{"a": ["p1/e99"], "b": ["p2:width"]}]}),
    ]
    for cid, nm in bad:
        annotate_case(f"ann-{cid}", "plain", nm, keep=False)

    def pat(src):
        return drafts[src] if src in drafts else annotated[src]

    def pref(src):
        return {"draft": src} if src in drafts else {"annotated": src}

    # ------------------------------------------------------------------ measure_ref
    def refs_for(src):
        P = pm.Pattern(copy.deepcopy(pat(src)))
        out = []
        for p in P.data["pieces"]:
            ids = [e["id"] for e in p["edges"]]
            vids = list(p["vertices"])
            out += [f"{p['id']}/{e}" for e in ids]
            out += [f"{p['id']}/{ids[0]}+{ids[1]}", f"{p['id']}:width", f"{p['id']}:height",
                    f"{p['id']}:{vids[0]}-{vids[2]}", f"{p['id']}:{vids[0]}-{vids[2]}:x", f"{p['id']}:{vids[1]}-{vids[2]}:y"]
        for s in P.seams:
            out += [s["a"], s["b"], f"seam:{s['id']}", f"seam:{s['id']}:b"]
        out += ["p9/e1", f"{P.data['pieces'][0]['id']}/e99", f"{P.data['pieces'][0]['id']}:c1-c99",
                f"{P.data['pieces'][0]['id']}:nonsense", "seam:none", "garbage", "nobody:width"]
        return out
    for src in ("plain", "filled", "drafted", "ann-plain", "ann-full", "ann-mirror-left"):
        for r in refs_for(src):
            res = outcome(lambda: V.measure_ref(copy.deepcopy(pat(src)), r)[0])
            cases.append({"kind": "measure_ref", "id": f"ref-{len(cases)}", "input": {"pattern": pref(src), "ref": r},
                          "output": res})

    # ------------------------------------------------------------------ scale_fit
    bottom = f"{pid('plain', 'back')}/{edge('plain', 'back', 'bottom')}"
    bottom_ann = f"back/{edge('plain', 'back', 'bottom')}"
    fits = [
        ("fit-draft", "plain", [[bottom, 300.0]]),
        ("fit-annotated", "ann-plain", [[bottom_ann, 300.0], ["seam:underarm", round(math.hypot(30, 230), 3)]]),
        ("fit-disagree", "ann-plain", [[bottom_ann, 300.0], ["front:width", 300.0]]),
        ("fit-filled", "filled", [[f"{pid('filled', 'back')}/{edge('filled', 'back', 'bottom')}", 300.0],
                                  [vpair("filled", "sleeve", "c", "d") + ":x", 300.0]]),
        ("fit-none", "plain", []),
        ("fit-zero", "plain", [[bottom, 0.0]]),
        ("fit-bad-ref", "plain", [["p9/e1", 100.0]]),
    ]
    for cid, src, targets in fits:
        def run(src=src, targets=targets):
            data, rep = V.fit_scale(copy.deepcopy(pat(src)), [tuple(t) for t in targets])
            return {"pattern": data, "report": rep}
        out = outcome(run)
        if "ok" in out:
            annotated[cid] = out["ok"]["pattern"]
            out = {"ok": {"pattern": {"annotated": cid}, "report": out["ok"]["report"]}}
        cases.append({"kind": "scale_fit", "id": cid, "input": {"pattern": pref(src), "targets": targets}, "output": out})

    # ------------------------------------------------------------------ audit
    b, f, s = matched["plain"]["back"], matched["plain"]["front"], matched["plain"]["sleeve"]
    labels = [
        {"text": "300 mm", "value_mm": 300.0, "ref": f"{b['id']}/{edge('plain', 'back', 'bottom')}"},
        {"text": f"{T.RECT_H} mm", "value_mm": T.RECT_H, "ref": vpair("plain", "back", "b", "c")},
        {"text": "300 mm", "value_mm": 300.0, "ref": f"{f['id']}/{edge('plain', 'front', 'cf')}"},
        {"text": "11.8 in", "ref": vpair("plain", "sleeve", "c", "d") + ":x"},
        {"text": "10 in hem", "value_mm": 254.0, "ref": f"{f['id']}/{edge('plain', 'front', 'hem')}"},
        {"text": "Cut 2.6 fabric"},
    ]
    rival = [
        {"text": "300 mm", "value_mm": 300.0, "ref": f"{b['id']}/{edge('plain', 'back', 'bottom')}"},
        {"text": "150 mm", "value_mm": 150.0, "ref": f"{f['id']}/{edge('plain', 'front', 'cf')}"},
        {"text": "12 in", "ref": f"{s['id']}:width"},
        {"text": "garbled", "ref": f"{s['id']}:width", "unparseable": True},
        {"text": "8 in", "ref": "p9/e1"},
        {"text": "20 cm"},
    ]
    lonely = [{"text": "300 mm", "value_mm": 300.0, "ref": f"{b['id']}/{edge('plain', 'back', 'bottom')}"},
              {"text": "100 mm", "value_mm": 100.0, "ref": f"{f['id']}/{edge('plain', 'front', 'cf')}"}]
    sym = copy.deepcopy(drafts["drafted"])
    for p in sym["pieces"]:
        p["symmetric"] = True
    sym["pieces"][2]["grain"] = [[150.0, 20.0], [150.0, 200.0]]
    drafts["drafted-symmetric"] = sym
    border = copy.deepcopy(annotated["ann-plain"])
    border["pieces"][0]["trace"]["touches_border"] = True
    border["pieces"][1]["trace"]["touches_border"] = True
    border["pieces"][1]["trace"]["touches_erased"] = True
    drafts["border"] = border
    roles = copy.deepcopy(drafts["drafted"])
    roles["pieces"][0]["role"], roles["pieces"][1]["role"] = "waistband", "pant_front"
    roles["pieces"][2]["role"] = "skirt_back"
    roles.pop("checks")
    drafts["roles"] = roles
    lone_band = copy.deepcopy(drafts["drafted"])
    lone_band["pieces"][0]["role"], lone_band["pieces"][1]["role"] = "waistband", "sleeve"
    lone_band["pieces"][2]["role"] = "sleeve"
    drafts["lone-band"] = lone_band
    audits = [
        ("audit-annotated", "ann-plain", None, {}),
        ("audit-navy", "ann-navy", None, {}),
        ("audit-filled", "ann-filled", None, {}),
        ("audit-labels", "plain", labels, {}),
        ("audit-labels-opts", "plain", labels, {"tol_mm": 1.0, "median_seam_mm": 300.0, "label_tol": 0.05}),
        ("audit-rival", "plain", rival, {}),
        ("audit-lonely", "plain", lonely, {}),
        ("audit-inventory", "ann-inventory", None, {}),
        ("audit-full", "ann-full", None, {}),
        ("audit-no-mirror", "ann-no-mirror", None, {}),
        ("audit-drafted", "drafted", None, {}),
        ("audit-symmetric", "drafted-symmetric", None, {}),
        ("audit-draft", "plain", None, {}),
        ("audit-fitted", "fit-annotated", None, {}),
        ("audit-border", "border", None, {}),
        ("audit-roles", "roles", None, {}),
        ("audit-lone-band", "lone-band", None, {}),
        ("audit-fold-tol", "ann-inventory", None, {"fold_tol_deg": 15.0}),
    ]
    for cid, src, labs, opts in audits:
        out = outcome(lambda: V.audit(copy.deepcopy(pat(src)), copy.deepcopy(labs), **opts))
        cases.append({"kind": "audit", "id": cid, "input": {"pattern": pref(src), "labels": labs, "options": opts},
                      "output": out})

    args.out_dir.mkdir(parents=True, exist_ok=True)
    path = args.out_dir / "post.json"
    body = {"schema": SCHEMA, "subsystem": "trace", "fixture": "post", "meta": meta, "drafts": clean(drafts),
            "annotated": clean(annotated), "cases": clean(cases)}
    path.write_text(json.dumps(body, separators=(",", ":"), allow_nan=False) + "\n")
    print(f"wrote {len(cases)} cases ({len(drafts)} drafts, {len(annotated)} stored patterns) to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
