"""Package the fixed-cut bolero experiment. Run from the repo root with Python and NumPy.

With --solve, generate the synthetic tapes and run ftx in a new scratch directory.
Without it, use those same saved runs. The body and cloth retain native float32
coordinates. The coupled outfit has its own solver archive.
"""
import argparse
import base64
import gzip
import hashlib
import json
import struct
import subprocess
from pathlib import Path
from runpy import run_path

import numpy as np

archive = run_path(str(Path(__file__).with_name("prepare-exhibits.py")))
check_contact, FTX = archive["check_contact"], archive["FTX"]
PUBLIC = Path(__file__).resolve().parents[1] / "public"
PATTERN = PUBLIC / "patterns/bolero.json"
GRID = [(chest, angle) for chest in (480, 520, 540) for angle in (35, 75, 90)]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def integrity(data):
    return "sha256-" + base64.b64encode(hashlib.sha256(data).digest()).decode("ascii")


def synthetic_tapes(chest):
    return {
        "schema": "fabric-measurements/1", "taken_on": "2026-10-13", "fused": {},
        "wearer": {"age_months": 30.0, "bmi": 16.0, "label": "synthetic",
                   "sex": "F", "stature_mm": 920.0},
        "records": [{"name": key, "value_mm": value, "sigma_mm": sigma,
                     "method": "synthetic tape", "provenance": "MEASURED_TAPE"}
                    for key, value, sigma in (("stature", 920, 10), ("head", 480, 5),
                                              ("foot_pass", 200, 5), ("chest", chest, 3))],
    }


def publish_tapes(output, chest, tapes):
    # The gallery solve used four tapes. A full pattern round also asks for the
    # synthetic hand passage used to draft this fixed sleeve.
    if tapes != synthetic_tapes(chest):
        raise ValueError(f"Unexpected recorded tapes for the {chest} mm case")
    supplemental = json.loads(PATTERN.read_bytes())["meta"]["draft_body"]["supplemental_measurements"]
    source, = [r for r in supplemental["hand_pass"]["source"]["sources"]
               if r["used"] and r["provenance"] == "MEASURED_TAPE"]
    hand = {"name": "hand_pass", **{k: source[k] for k in ("method", "provenance", "value_mm", "sigma_mm")}}
    document = {**tapes, "records": [*tapes["records"], hand]}
    (output/f"chest-{chest}.json").write_text(json.dumps(document, indent=2)+"\n")


def read_obj(path):
    vertices, faces = [], []
    for line in path.read_text().splitlines():
        if line.startswith("v "):
            vertices.append([float(x) for x in line.split()[1:]])
        elif line.startswith("f "):
            faces.append([int(x)-1 for x in line.split()[1:]])
    return np.array(vertices), np.array(faces)


def quantize(metres):
    values = np.rint(metres * 10000)
    if not np.isfinite(values).all() or np.abs(values).max() > 32767:
        raise ValueError("Position exceeds the signed display encoding")
    return values.astype("<i2").tobytes()


def cut_drawing(xyz, template):
    edges = {}
    for face in np.array(template["triangles"]).reshape(-1, 3):
        for i in range(3):
            pair = tuple(sorted((face[i], face[(i+1) % 3])))
            edges[pair] = edges.get(pair, 0) + 1
    x, y = xyz[:, 0] * 1000, -xyz[:, 2] * 1000
    path = " ".join(f"M{x[a]:.1f},{y[a]:.1f}L{x[b]:.1f},{y[b]:.1f}"
                    for (a, b), n in edges.items() if n == 1)
    view = f"{x.min()-5:.2f} {y.min()-5:.2f} {np.ptp(x)+10:.2f} {np.ptp(y)+10:.2f}"
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}"><path fill="none" '
            f'stroke="#5b594e" stroke-width="5" stroke-linecap="round" d="{path}"/></svg>\n')


def export_run(directory, key, template, output):
    check_contact(directory)
    report = json.loads((directory/"sim_report.json").read_text())
    mesh = json.loads((directory/"mesh_map.json").read_text())
    cache = (directory/"fold.pc2").read_bytes()
    magic, version, vertices, first, rate, frames = struct.unpack_from("<12siiffi", cache)
    if (magic, version, vertices, first, rate, frames) != (
            b"POINTCACHE2\x00", 1, template["vertices"], 1., 1., sum(mesh["frames"].values())):
        raise ValueError("Inconsistent PC2 header")
    xyz = np.frombuffer(cache, dtype="<f4", offset=32).reshape(frames, vertices, 3)
    colored, triangles = read_obj(directory/"garment_strain.obj")
    if (not np.array_equal(xyz[-1], colored[:, :3].astype("<f4"))
            or triangles.flatten().tolist() != template["triangles"]
            or mesh["seams"] != template["seams"]
            or mesh["inst_range"] != template["panels"]):
        raise ValueError("Replay and pattern topology disagree")
    supplied = json.loads(Path(report["pattern"]).read_bytes())
    if supplied != starting_pattern():
        raise ValueError(f"{key}: run used a different pattern or starting placement")
    if key == "520-35":
        (output/"flat-cut.svg").write_text(cut_drawing(xyz[0], template))
    body, faces = read_obj(directory/"body.obj")
    indices, remap = np.unique(faces, return_inverse=True)
    colors = np.rint(255 * colored[:, 3:]).astype("u1").tobytes()
    reactions = report["contact_diagnostics"]["final_reactions"]["result"]
    welded = np.asarray(reactions["positions_m"], dtype="<f4")
    lookup = {tuple(point): i for i, point in enumerate(welded)}
    if len(lookup) != len(welded):
        raise ValueError("Ambiguous welded endpoint")
    display_indices = [lookup[tuple(point)] for point in xyz[-1]]
    loads = np.linalg.norm(reactions["body_force_on_cloth_n"], axis=1)
    force = loads[display_indices].astype("<f4").tobytes()
    samples = archive["replay_frames"](mesh["frames"])
    payload = b"".join([struct.pack("<II", len(indices), faces.size), body[indices].astype("<f4").tobytes(),
                        remap.astype("<u4").tobytes(), xyz[samples].tobytes(), force, colors])
    geometry = gzip.compress(payload, compresslevel=9, mtime=0)
    (output/(key+".bin.gz")).write_bytes(geometry)
    tape = report["body"]["fit"]["tape"]
    return {"arm": report["arm_deg"], "chest": tape["chest"], "stature": tape["stature"],
            "chestClearance": report["landmarks"]["chest"]["clearance_mean_mm"],
            "contactFraction": report["landmarks"]["chest"]["contact_frac"],
            "edgeStrainPercent": report["global_strain_pct"], "settled": report["numerical"]["settled"],
            "flags": report["gating"], "selfContact": report["self_contact"]["audit"],
            "peakBodyForceN": float(loads.max()),
            "sourcePc2Sha256": digest(cache), "geometryIntegrity": integrity(geometry), "tapeResiduals": tape,
            "frames": samples, "sourceFrames": frames, "phases": mesh["frames"]}


def starting_pattern():
    pattern = json.loads(PATTERN.read_bytes())
    for piece in pattern["pieces"]:
        placement = piece["placement"]
        placement["y0"] = placement.get("y0", 0) - 25
    return pattern


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scratch", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--solve", action="store_true")
    parser.add_argument("--model", type=Path, help="use this converted Anny model for the nine native solves")
    args = parser.parse_args()
    if args.solve:
        args.scratch.mkdir(parents=True, exist_ok=False)
        for chest, angle in GRID:
            tapes = args.scratch/f"chest-{chest}.json"
            if not tapes.exists():
                tapes.write_text(json.dumps(synthetic_tapes(chest)))
            key = f"{chest}-{angle}"
            pattern = args.scratch/f"bolero-{key}.json"
            pattern.write_text(json.dumps(starting_pattern(), indent=2)+"\n")
            command = [FTX, "drape", pattern, "--measurements", tapes, "--fit", PUBLIC/"exhibits/poses/fit-spec.json",
                       "--spacing-mm", "12", "--self-contact-mm", "2", "--self-mu", "0.4",
                       "--arm-deg", str(angle), "--drape", "96", "--max-settle-steps", "256",
                       "--out", args.scratch/f"chest-{key}", "--json"]
            if args.model:
                command.extend(["--model", args.model])
            run = subprocess.run(command, stdout=subprocess.PIPE, check=False)
            (args.scratch/f"result-{key}.json").write_bytes(run.stdout)
    layout = (PUBLIC/"exhibits/bolero.json").read_bytes()
    template = json.loads(layout)
    args.output.mkdir(parents=True, exist_ok=False)
    reports = {}
    for chest in sorted({chest for chest, _ in GRID}):
        publish_tapes(args.output, chest, json.loads((args.scratch/f"chest-{chest}.json").read_bytes()))
    for chest, angle in GRID:
        key = f"{chest}-{angle}"
        run = json.loads((args.scratch/f"result-{key}.json").read_text())
        if "refusal" in run:
            raise ValueError(f"{key}: {run['refusal']['message']}")
        reports[key] = export_run(args.scratch/("chest-"+key), key, template, args.output)
    result = {"patternSha256": digest(PATTERN.read_bytes()), "layoutIntegrity": integrity(layout), "cases": reports}
    (args.output/"bolero-start.json").write_text(json.dumps(starting_pattern(), indent=2)+"\n")
    (args.output/"runs.json").write_text(json.dumps(result, separators=(",", ":"))+"\n")


if __name__ == "__main__":
    main()
