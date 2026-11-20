"""Drape the published 1761 dolman on the synthetic adult form and package it for the hussar exhibit.

From the repository root:
  python3 showcase/scripts/prepare-hussar-drape.py NEW_WORK_DIR OUTPUT_DIR --solve --model MODEL.ftxbody
  python3 showcase/scripts/prepare-hussar-drape.py WORK_DIR OUTPUT_DIR          # repackage a finished drape

`--solve` writes WORK_DIR/pattern.json and measures.json from the published pattern and the fitted
synthetic body, then runs `ftx drape` into WORK_DIR/drape. Packaging refuses a WORK_DIR whose inputs
differ from the ones this script would write.

Nothing here simulates or adjusts cloth: positions are the solver's float32 samples, copied. The cord
and button stations are the pattern's own `decor` and `internal` lines, located once on the flat mesh
so the page can carry them on the draped surface. Never use a real wearer's workspace.
"""
import argparse
import gzip
import hashlib
import json
import os
import struct
import subprocess
from pathlib import Path

import numpy as np

HEADER = struct.Struct("<12siiffi")
CORD_STEP_MM = 5.0
PUBLIC = Path(__file__).resolve().parents[1] / "public"
FTX = Path(os.environ.get("FTX_BIN", Path(__file__).resolve().parents[2] / "target/release/ftx"))

# Where each dolman panel starts round the form (spec 04 section 3.1). Torso panels sit on one cylinder
# of 205 mm, anchored where the pattern's neck point (y = 455) meets the form's side neck point; the two
# sleeve panels tile one cylinder round the arm, the upper sleeve outside and the under sleeve inside.
# Found by looking at `ftx place` stills, not derived: a 244 mm default radius left the shoulder seams
# 460 mm apart.
PLACEMENTS = {
    "dolman_back": {"part": "torso", "theta_deg": 180, "x0": 0, "y0_landmark": "snp", "y0": 455, "radius_mm": 205},
    "dolman_front": {"part": "torso", "theta_deg": -3, "x0": 0, "y0_landmark": "snp", "y0": 455, "radius_mm": 205},
    "dolman_side": {"part": "torso", "theta_deg": 249, "x0": 65, "y0_landmark": "snp", "y0": 455, "radius_mm": 205},
    "dolman_topsleeve": {"part": "arm", "theta_deg": -90, "x0": 0, "y0_landmark": "shoulder_tip", "y0": 146, "radius_mm": 62},
    "dolman_undersleeve": {"part": "arm", "theta_deg": 90, "x0": 0, "y0_landmark": "shoulder_tip", "y0": 146, "radius_mm": 62},
    "dolman_collar": {"part": "torso", "theta_deg": 180, "x0": 206, "y0_landmark": "snp", "y0": 0, "clearance_mm": 10},
}
# The front fastens edge to edge with loops and buttons. It is modelled as a closed seam, which takes the
# place of the facing's seam on that edge; the facing itself is left out of the drape.
LEFT_OUT = ["dolman_facing"]
CLOSURE = {"id": "dolman_front_closure", "a": "dolman_front.R/opening", "b": "dolman_front.L/opening", "reverse": True}
# Placeholder constants for a lined wool, the same as the bolero exhibit's. They have no measured source.
FABRIC = {"areal_g_m2": 380, "bending_Nm": 0.00012, "id": "shell", "kind": "woven", "name": "wool broadcloth, lined",
          "sim_note": "lined-jacket composite (UNCERTAIN)", "tensile_N_m": 10000.0, "width_mm": 1500}
# At 20 degrees the sleeve crosses the torso cloth. At 45 both cloth and body start audits pass.
DRAPE_ARGS = ["--spacing-mm", "20", "--time-subdivision", "--arm-deg", "45", "--drape", "60",
              "--self-contact-mm", "1", "--max-settle-steps", "120"]


def inputs():
    """The drape's two input documents: the dolman of the published pattern with placements, and the form's tapes."""
    source = json.loads((PUBLIC / "patterns/hussar-blue.json").read_text())
    pieces = []
    for piece in source["pieces"]:
        if not piece["id"].startswith("dolman_") or piece["id"] in LEFT_OUT:
            continue
        piece = dict(piece)
        if piece["id"] in PLACEMENTS:
            piece["placement"] = PLACEMENTS[piece["id"]]
        else:
            piece["sim"] = False
        pieces.append(piece)
    kept = {p["id"] for p in pieces}
    owner = lambda ref: ref.split("/")[0].split(".")[0]
    seams = [s for s in source["seams"] if owner(s["a"]) in kept and owner(s["b"]) in kept] + [CLOSURE]
    pattern = {**source, "pieces": pieces, "seams": seams, "fabric": FABRIC}
    fitted = json.loads((PUBLIC / "fitted-bodies/hussar.json").read_text())
    measures = {"schema": "fabric-measurements/1",
                "wearer": {"label": "Synthetic hussar", "stature_mm": 1780, "sex": "male", "age_months": 360},
                "fused": {k: {"value_mm": v["target_mm"]} for k, v in fitted["native_tapes"].items()}}
    return pattern, measures


def read_obj(path):
    vertices, faces = [], []
    for line in path.read_text().splitlines():
        fields = line.split()
        if fields[:1] == ["v"]:
            vertices.append([float(v) for v in fields[1:4]])
        elif fields[:1] == ["f"]:
            faces.append([int(v) - 1 for v in fields[1:4]])
    return np.array(vertices), np.array(faces, dtype=np.int64)


def replay_frames(phases):
    """Source frame indices kept for the page: a thinned fold-up, every phase boundary, the endpoint."""
    keep, start = set(), 0
    # A long settle is thinned further, to about fourteen gravity frames.
    for phase, stride in (("fold", 7), ("zip", 6), ("sew", 2), ("drape", max(6, -(-phases["drape"] // 14)))):
        end = start + phases[phase]
        if end > start:
            keep.update(range(start, end, stride))
            keep.update((start, end - 1))
        start = end
    return sorted(keep)


def resample(points, step):
    """A polyline resampled at no more than `step` between samples, keeping its own vertices."""
    out = [np.array(points[0], dtype=float)]
    for a, b in zip(points, points[1:]):
        a, b = np.array(a, dtype=float), np.array(b, dtype=float)
        pieces = max(1, int(np.ceil(np.linalg.norm(b - a) / step)))
        out.extend(a + (b - a) * (i / pieces) for i in range(1, pieces + 1))
    return np.array(out)


def locate(points, flat, triangles):
    """Triangle index and barycentric weights of each 2D point on a flat mesh (nearest triangle if outside)."""
    a, b, c = (flat[triangles[:, i]] for i in range(3))
    v0, v1 = b - a, c - a
    den = v0[:, 0] * v1[:, 1] - v1[:, 0] * v0[:, 1]
    anchors = []
    for p in points:
        v2 = p - a
        u = (v2[:, 0] * v1[:, 1] - v1[:, 0] * v2[:, 1]) / den
        v = (v0[:, 0] * v2[:, 1] - v2[:, 0] * v0[:, 1]) / den
        w = 1 - u - v
        outside = np.maximum(0, -np.minimum(np.minimum(u, v), w))
        best = int(np.argmin(outside))
        weights = np.clip([w[best], u[best], v[best]], 0, None)
        anchors.append((best, *(weights / weights.sum())))
    return anchors


def package(source, pattern_path, destination, name, drape_args=(), section=None):
    """Pack a finished drape. `drape_args` are the solver arguments it was run with, recorded as given."""
    subprocess.run([FTX, "viewer", source, "--check-contact"], check=True)
    drape_args = [str(a) for a in drape_args]
    spacing = float(drape_args[drape_args.index("--spacing-mm") + 1]) if "--spacing-mm" in drape_args else None
    mesh = json.loads((source / "mesh_map.json").read_text())
    report = json.loads((source / "sim_report.json").read_text())
    pattern = json.loads(pattern_path.read_text())
    cache = (source / "fold.pc2").read_bytes()
    signature, version, n, first, rate, count = HEADER.unpack_from(cache)
    phases = mesh["frames"]
    if ((signature, version, first, rate) != (b"POINTCACHE2\0", 1, 1.0, 1.0) or n != mesh["vertices"]
            or mesh["units"] != "m" or len(cache) != HEADER.size + count * n * 12 or count != sum(phases.values())):
        raise ValueError("Inconsistent PC2 point cache and garment mesh map")
    xyz = np.frombuffer(cache, dtype="<f4", offset=HEADER.size).reshape(count, n, 3)
    endpoint, triangles = read_obj(source / "garment.obj")
    if not np.array_equal(endpoint.astype("<f4"), xyz[-1]) or len(triangles) != report["faces"]:
        raise ValueError("The garment mesh and the final replay frame disagree")
    if not np.isfinite(xyz).all():
        raise ValueError("Nonfinite garment position")
    body, body_faces = read_obj(source / "body.obj")
    used, remap = np.unique(body_faces, return_inverse=True)

    # Frame 0 is the flat pattern on the board: x and z are the cut's own millimetres, in metres.
    flat = xyz[0][:, [0, 2]].astype(np.float64)
    pieces = {p["id"]: p for p in pattern["pieces"]}
    owner = np.full(n, -1)
    names = list(mesh["inst_range"])
    for k, key in enumerate(names):
        lo, hi = mesh["inst_range"][key]
        owner[lo:hi] = k
    cords, buttons = [], []
    for k, key in enumerate(names):
        piece = pieces[key.rsplit(".", 1)[0] if key.rsplit(".", 1)[-1] in ("R", "L", "C") else key]
        # Button stations are the short drawn marks on a corded panel; other pieces' drawn marks are match marks.
        if not piece.get("decor"):
            continue
        lines = [(d, "cord") for d in piece["decor"]] + \
                [(d, "button") for d in piece.get("internal", []) if d.get("kind") == "draw"]
        # Pattern millimetres to board metres for this instance, from its named corners (a mirrored
        # copy comes out with a reflection; nothing is assumed about the board layout).
        corners = [(piece["vertices"][c.split(":", 1)[1]], i) for c, i in mesh["corners"].items()
                   if c.split(":", 1)[0] == key and c.split(":", 1)[1] in piece["vertices"]]
        if len(corners) < 3:
            raise ValueError(f"{key}: too few named corners to locate its decoration")
        source_xy = np.array([[x, y, 1.0] for (x, y), _ in corners])
        target_xy = flat[[i for _, i in corners]]
        affine, *_ = np.linalg.lstsq(source_xy, target_xy, rcond=None)
        if np.abs(source_xy @ affine - target_xy).max() > 5e-4:
            raise ValueError(f"{key}: the board layout is not an affine image of the pattern piece")
        local = triangles[owner[triangles[:, 0]] == k]
        global_index = np.flatnonzero(owner[triangles[:, 0]] == k)
        for line, kind in lines:
            pts = np.array(line["pts"], dtype=float)
            if kind == "button":
                pts = pts.mean(axis=0, keepdims=True)
            else:
                pts = resample(pts, CORD_STEP_MM)
            on_board = np.c_[pts, np.ones(len(pts))] @ affine
            anchors = [(int(global_index[t]), round(float(b0), 5), round(float(b1), 5))
                       for t, b0, b1, _ in locate(on_board, flat, local)]
            if kind == "button":
                buttons.append({"instance": key, "anchor": anchors[0]})
            else:
                cords.append({"instance": key, "width_mm": line.get("width_mm", 3), "closed": bool(line.get("closed")),
                              "anchors": [v for anchor in anchors for v in anchor]})

    frames = replay_frames(phases)
    payload = b"".join([
        struct.pack("<IIII", len(used), body_faces.size, n, len(frames)),
        body[used].astype("<f4").tobytes(), remap.astype("<u4").tobytes(),
        flat.astype("<f4").tobytes(), xyz[frames].astype("<f4").tobytes()])
    numerical = report["numerical"]
    metadata = {
        "units": "m", "vertices": n, "triangles": triangles.flatten().tolist(),
        "panels": mesh["inst_range"], "seams": mesh["seams"],
        "frames": frames, "sourceFrames": count, "phases": phases,
        "cords": cords, "buttons": buttons,
        "body": {"vertices": int(len(used)), "indices": int(body_faces.size)},
        "sourcePc2Sha256": hashlib.sha256(cache).hexdigest(),
        "sourcePatternSha256": hashlib.sha256(pattern_path.read_bytes()).hexdigest(),
        "summary": {
            "style": report["style"], "spacingMm": spacing, "solverArguments": drape_args, "armDeg": report["arm_deg"],
            "settled": numerical["settled"], "gating": report["gating"], "flags": report["flags"],
            "edgeStrainPercent": report["global_strain_pct"], "massKg": report["mass_kg"],
            "areaM2": report["area_m2"], "sewingPairs": report["sewing_pairs"],
            "selfContact": report["self_contact"].get("enabled", False),
            "scope": report["simulation_scope"], "fabric": report["fabric"],
            "bodyFit": report["body"].get("fit", {}).get("tape"), "wallSeconds": report["wall_s"],
        },
    }
    # A softened compression factor or a seam's declared band of pleats is a different cloth model, so a summary
    # made with one names it. A reference-model drape's report has neither, and its summary is unchanged.
    for key, field in (("compressionFactor", "compression_factor"), ("gatherRelief", "gather_relief")):
        if field in report:
            metadata["summary"][key] = report[field]
    # The numbers the page states in words, small enough to import without the mesh.
    digest = {"vertices": n, "triangles": len(triangles), "frames": len(frames), "phases": phases,
              "cords": len(cords), "buttons": len(buttons), "instances": names,
              "sourcePc2Sha256": metadata["sourcePc2Sha256"], **metadata["summary"]}
    destination.mkdir(parents=True, exist_ok=True)
    outputs = {destination / f"{name}.json": (json.dumps(metadata, separators=(",", ":")) + "\n").encode(),
               destination / f"{name}-pattern.json": pattern_path.read_bytes(),
               destination / f"{name}-summary.json": (json.dumps(digest, indent=1) + "\n").encode(),
               destination / f"{name}.gz": gzip.compress(payload, mtime=0)}
    if section:
        result = section(body, body_faces, xyz[-1], triangles, mesh, report)
        outputs[destination / f"{name}-section.json"] = (json.dumps(result, separators=(",", ":"), allow_nan=False) + "\n").encode()
    for path, data in outputs.items():
        if path.exists() and path.read_bytes() != data:
            raise ValueError(f"{path} already holds a different drape. Use a new output directory.")
    for path, data in outputs.items():
        if not path.exists():
            with path.open("xb") as file:
                file.write(data)
    print(f"{name}: {n} cloth vertices, {len(frames)}/{count} frames, {len(cords)} cords, {len(buttons)} buttons, "
          f"{len(used)} body vertices, {len(outputs[destination / (name + '.gz')]):,} bytes")


def solve_and_package(description, documents, drape_args, default_name, section=None, argv=None):
    """The command line shared by every exhibit packed this way (the sundress script calls it too).

    `documents` maps `pattern.json`, `measures.json` and optionally `fit.json` to the drape's inputs.
    """
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument("work", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--name", default=default_name)
    parser.add_argument("--solve", action="store_true", help="Create WORK and run the drape first.")
    parser.add_argument("--model", type=Path, help="Converted Anny model for --solve.")
    args = parser.parse_args(argv)
    if args.solve:
        if not args.model:
            parser.error("--solve needs --model")
        args.work.mkdir(parents=True, exist_ok=False)
        for name, document in documents.items():
            (args.work / name).write_text(json.dumps(document, indent=1))
        fit = ["--fit", args.work / "fit.json"] if "fit.json" in documents else []
        # Exit code 2 is a finished drape that carries gating flags; the page reports them.
        done = subprocess.run([FTX, "drape", args.work / "pattern.json", "--measurements", args.work / "measures.json", *fit,
                               "--model", args.model, "--out", args.work / "drape", "--json", *drape_args],
                              stdout=subprocess.DEVNULL)
        if done.returncode not in (0, 2):
            raise SystemExit(f"ftx drape refused (exit {done.returncode}); run `ftx place` on {args.work}/pattern.json to look")
    for name, document in documents.items():
        # Compare as JSON: a tuple in `document` is a list once written.
        if json.loads((args.work / name).read_text()) != json.loads(json.dumps(document)):
            raise SystemExit(f"{args.work}/{name} is not what this script drapes; solve again into a new directory")
    package(args.work / "drape", args.work / "pattern.json", args.destination, args.name, drape_args, section)


if __name__ == "__main__":
    pattern, measures = inputs()
    solve_and_package(__doc__, {"pattern.json": pattern, "measures.json": measures}, DRAPE_ARGS, "dolman")
