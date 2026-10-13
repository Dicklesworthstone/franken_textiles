#!/usr/bin/env python3
"""Capture export oracle fixtures from the Python reference (docs/spec/03 §2-4, §10).

    OUT_DIR/helpers.json   text and number helpers: fnum, ascii_text, safe_name, text_width, wrap,
                           stroke_text, column letters, page lists
    OUT_DIR/geoms.json     piece geometry (spec 03 §5: notches, grain, fold marks, internal lines, drills, decor,
                           labels, label texts, grain marks and rotation) of every piece as drafted and every
                           instance, at text sizes 6 and 10
    OUT_DIR/master_svg.json the master SVG (spec 03 §6, §7.1) and its manifest entry for every pattern (and the
                           NOTE piece), plus --unfold and --layout-width 300 for TESTBODICE, ROBUST and the 92 cm
                           bolero and trousers
    OUT_DIR/outlines.json  seam-allowance cut outlines (ring and method), sew rings and edge breaks of every
                           piece as drafted and of every instance, for the spec's fixture patterns (TESTBODICE,
                           ROBUST, the single pieces) and the drafted garments of tests/fixtures/oracle/pattern
                           (fez, bolero with soutache, trousers at 80, 92 and 116 cm), plus option variants

Usage: python capture_exports.py --reference SKILL/scripts OUT_DIR   (deterministic; synthetic and public data)
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import platform
import sys
from pathlib import Path

import numpy as np

SCHEMA = "ftx-oracle/1"
FILES = ("export.py", "pattern_model.py", "tests/test_export.py")
DRAFTS = Path(__file__).resolve().parents[2] / "tests/fixtures/oracle/pattern/drafts.json"


def ring_list(r) -> list:
    return [[float(x), float(y)] for x, y in np.asarray(r, float)]


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
    sys.path.insert(0, str((args.reference / "tests").resolve()))
    import export as X
    import pattern_model as pm
    import test_export as T

    meta = {
        "reference_files": {f: hashlib.sha256((args.reference / f).read_bytes()).hexdigest() for f in FILES},
        "numpy_version": np.__version__, "python_version": platform.python_version(), "machine": platform.machine(),
    }
    args.out_dir.mkdir(parents=True, exist_ok=True)

    # ------------------------------------------------------------------ helpers
    fnum_cases = []
    for v in (12.0, -0.0004, 1e-9, 0.1 + 0.2, 0.125, 2.5, 3.5, -2.5, 1234.5678, -0.0, 1e15, 0.0005, 0.0015,
              -12.3456789, 100.0, 7.125, 0.875, 1 / 3, 2 / 3, 1e-4, 99.9995, 5e-324, -1e-300):
        for nd in (0, 1, 2, 3, 4):
            fnum_cases.append({"v": v, "nd": nd, "out": X.fnum(v, nd)})
    strings = ["Café × — – · ñ ø ß ﬁ ½", "Front bodice (left) ×2", "___", "920 mm stature", "",
               "Ünïcödé Ǆ Ⅻ ① ＡＢＣ ¹²³ µ", "tab\there", "Zouave bolero — 2T", "a/b\\c (d) [e]", "ÆŒæœ łđ"]
    text_cases = [{"s": s, "ascii": X.ascii_text(s), "safe20": X.safe_name(s), "safe5": X.safe_name(s, 5),
                   "safe40": X.safe_name(s, 40)} for s in strings]
    widths = [{"s": s, "size": size, "bold": bold, "out": X.text_width(s, size, bold)}
              for s in ("PLACE ON FOLD", "Front bodice", "Cut 2 (mirror pair)", "SA included: 25/6/10 mm", "é~\x7f")
              for size in (1.0, 3.5, 4.0, 6.0) for bold in (False, True)]
    note = "Elastic 25 mm wide: cut 524 mm, overlap 25 mm and stitch through all layers before closing the casing"
    wraps = [{"s": note, "size": size, "width": width, "out": X.wrap(note, size, width)}
             for size in (3.0, 4.0) for width in (40.0, 80.0, 150.0, 1000.0)]
    strokes = []
    for s, anchor, y_down in (("PLACE ON FOLD", "middle", True), ("Front 2T", "start", False),
                              ("A-Z 0-9 .,:/()+=_'#%?!<>*|&", "end", True), ("ü~", "start", True)):
        polys = X.stroke_text(s, 12.5, -3.0, 4.0 * X.CAP, anchor, y_down)
        strokes.append({"s": s, "x": 12.5, "y": -3.0, "cap": 4.0 * X.CAP, "anchor": anchor, "y_down": y_down,
                        "out": [ring_list(p) for p in polys]})
    cols = [{"c": c, "out": X._col(c)} for c in (0, 1, 25, 26, 27, 51, 52, 701, 702, 703, 18277, 18278)]
    pages = [{"nums": n, "out": X._page_list(n)} for n in ([2, 3, 4, 6, 8, 9], [1], [], [5, 7, 9], [1, 2, 3, 4])]
    helpers = {"fnum": fnum_cases, "text": text_cases, "text_width": widths, "wrap": wraps, "stroke_text": strokes,
               "col": cols, "page_list": pages}
    path = args.out_dir / "helpers.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "export", "fixture": "helpers", "meta": meta,
                                "cases": helpers}, separators=(",", ":"), ensure_ascii=False) + "\n")
    print(f"wrote helpers to {path}")

    # ------------------------------------------------------------------ outlines
    def single(piece: dict, style: str) -> dict:
        return {"schema": pm.SCHEMA, "style": style, "units": "mm", "meta": {"size_label": "base"},
                "pieces": [piece], "seams": []}

    rect_hem = T.rect_piece(False)
    rect_hem["edges"][0]["sa"] = 30
    tri = pm.PieceBuilder("tri", "Tri", cut={"qty": 1})
    tri.v("a", 0, 0)
    tri.v("b", 200, 0)
    tri.v("c", 200, 200 * math.tan(math.radians(20)))
    tri.e("ab", "a", "b")
    tri.e("bc", "b", "c")
    tri.e("ca", "c", "a")
    scoop = pm.PieceBuilder("scoop", "Scoop", cut={"qty": 1})
    for vid, x, y in (("a", 0, 0), ("b", 100, 0), ("c", 100, 80), ("d", 60, 80), ("e", 40, 80), ("f", 0, 80)):
        scoop.v(vid, x, y)
    scoop.e("ab", "a", "b")
    scoop.e("bc", "b", "c")
    scoop.e("cd", "c", "d")
    scoop.cubic_abs("de", "d", "e", (60, 60), (40, 60))
    scoop.e("ef", "e", "f")
    scoop.e("fa", "f", "a")

    patterns = [
        ("testbodice", T.fixture_pattern(), 10.0, 4.0),
        ("testbodice_sa15_mitre2", T.fixture_pattern(), 15.0, 2.0),
        ("robust", T.robustness_pattern(), 10.0, 4.0),
        ("rect", single(T.rect_piece(False), "RECT"), 10.0, 4.0),
        ("rect_fold", single(T.rect_piece(True), "RECT"), 10.0, 4.0),
        ("rect_hem30", single(rect_hem, "RECT"), 10.0, 4.0),
        ("triangle", single(tri.build(), "TRI"), 10.0, 4.0),
        ("scoop", single(scoop.build(), "SCOOP"), 15.0, 4.0),
    ]
    for case in json.loads(DRAFTS.read_text())["cases"]:
        patterns.append((case["id"], case["input"]["pattern"], X.pattern_sa(case["input"]["pattern"]), 4.0))

    cases = []
    for cid, doc, sa, limit in patterns:
        doc = json.loads(json.dumps(doc))
        pat = pm.Pattern(copy.deepcopy(doc))

        def outline(src):
            ring, method = X.cut_outline(src, sa, limit, return_method=True)
            inst, fold = X._as_instance(src)
            loops = X._edge_loops(inst, fold, sa)
            breaks, k = [], 0
            for _, pts, _ in loops:
                breaks.append(k)
                k += len(pts) - 1
            return {"ring": ring_list(ring), "method": method, "sew": ring_list(X.sew_ring(loops)),
                    "breaks": breaks, "sa": [float(s) for _, _, s in loops]}

        out = {"pieces": {p["id"]: outline(p) for p in pat.data["pieces"]},
               "instances": {iid: outline(inst) for iid, inst in pat.instances.items()}}
        cases.append({"id": cid, "input": {"pattern": doc, "sa": sa, "mitre_limit": limit}, "output": out})
    path = args.out_dir / "outlines.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "export", "fixture": "outlines", "meta": meta,
                                "cases": cases}, separators=(",", ":"), ensure_ascii=False) + "\n")
    methods = [o["method"] for c in cases for o in [*c["output"]["pieces"].values(), *c["output"]["instances"].values()]]
    print(f"wrote {len(cases)} cases ({methods.count('mitre')} mitre, {methods.count('union')} union) to {path}")

    # ------------------------------------------------------------------ piece geometry
    def geom_json(g):
        return {
            "pid": g.pid, "iid": g.iid, "name": g.name,
            "meta": {k: (list(v) if isinstance(v, (list, tuple)) else v) for k, v in g.meta.items()},
            "sew_breaks": list(g.sew_breaks),
            "notches": [{"id": n.id, "type": n.type, "base": ring_list([n.base])[0], "inward": ring_list([n.inward])[0],
                         "tangent": ring_list([n.tangent])[0], "depth": n.depth, "width": n.width,
                         "marks": [ring_list(m) for m in n.marks], "sew_point": ring_list([n.sew_point])[0]}
                        for n in g.notches],
            "grain": None if g.grain is None else ring_list(g.grain), "grain_default": g.grain_default,
            "fold_lines": [{"pts": ring_list(q), "style": st} for q, st in g.fold_lines],
            "internal_draw": [{"pts": ring_list(q), "closed": c} for q, c in g.internal_draw],
            "internal_cut": [{"pts": ring_list(q), "closed": c} for q, c in g.internal_cut],
            "drills": [{"p": ring_list([q])[0], "d": d} for q, d in g.drills],
            "labels": [{"p": ring_list([lb.p])[0], "lines": [list(x) for x in lb.lines], "size": lb.size,
                        "kind": lb.kind} for lb in g.labels],
            "decor": [{"kind": d["kind"], "pts": ring_list(d["pts"]), "width": d["width"], "closed": d["closed"]}
                      for d in g.decor],
            "grain_rotation": X.grain_vertical_rotation(g),
            "grain_marks": [ring_list(m) for m in X.grain_marks(g)] if g.grain is not None else [],
            "label_texts": [[[x, y, t, size, bold] for x, y, t, size, bold in X.label_texts(lb, yd, 0.0, 1.0)]
                            for lb in g.labels for yd in (True, False)],
        }

    gcases = []
    for cid, doc, sa, limit in patterns:
        if cid == "testbodice_sa15_mitre2":
            continue
        doc = json.loads(json.dumps(doc))
        pat = pm.Pattern(copy.deepcopy(doc))
        out = {}
        for ts in (6.0, 10.0):
            out[f"pieces@{ts:g}"] = {p["id"]: geom_json(X.build_geom(pat, p, sa, limit, ts)) for p in pat.data["pieces"]}
            out[f"instances@{ts:g}"] = {iid: geom_json(X.build_geom(pat, inst, sa, limit, ts))
                                        for iid, inst in pat.instances.items()}
        gcases.append({"id": cid, "input": {"pattern": doc, "sa": sa, "mitre_limit": limit}, "output": out})
    path = args.out_dir / "geoms.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "export", "fixture": "geoms", "meta": meta,
                                "cases": gcases}, separators=(",", ":"), ensure_ascii=False) + "\n")
    print(f"wrote {len(gcases)} geometry cases to {path}")

    # ------------------------------------------------------------------ master SVG
    import tempfile
    note = single(T.rect_piece(False), "NOTE")
    note["pieces"][0]["note"] = "Elastic 25 mm wide: cut 524 mm"
    svg_patterns = [(cid, doc) for cid, doc, _sa, _limit in patterns if cid not in ("testbodice_sa15_mitre2",)]
    svg_patterns.append(("note", note))
    scases = []
    for cid, doc in svg_patterns:
        for opt_id, opts in (("default", {}), ("unfold", {"unfold": True}), ("width300", {"layout_width": 300.0})):
            if opt_id != "default" and cid not in ("testbodice", "robust", "zouave_bolero-920", "zouave_trousers-920"):
                continue
            with tempfile.TemporaryDirectory() as tmp:
                ctx = X.Ctx(pm.Pattern(json.loads(json.dumps(doc))), Path(tmp), created="2026-10-11T12:00", **opts)
                path = X.export_svg(ctx)
                svg = path.read_text()
                master = ctx.manifest["master"]
            scases.append({"id": f"{cid}/{opt_id}", "input": {"pattern": doc, "options": opts},
                           "output": {"svg": svg, "master": master}})
    path = args.out_dir / "master_svg.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "export", "fixture": "master_svg", "meta": meta,
                                "cases": scases}, separators=(",", ":"), ensure_ascii=False) + "\n")
    print(f"wrote {len(scases)} master SVG cases to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
