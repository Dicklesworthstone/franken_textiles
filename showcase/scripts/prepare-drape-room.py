"""Package the fixed-cut bolero experiment. Run from the repo root with Python and NumPy.

With --solve, generate the synthetic tapes and run ftx in a new scratch directory.
Without it, use those same saved runs. The display payloads contain an upper-body
surface, actual cloth frames at 0.1 mm precision, and solver endpoint colors.
"""
import argparse
import gzip
import hashlib
import json
import struct
import subprocess
from pathlib import Path

import numpy as np

PUBLIC = Path("showcase/public")
PATTERN = PUBLIC / "patterns/bolero.json"
GRID = [(chest, angle) for chest in (480, 520, 540) for angle in (35, 75, 90)]


def digest(data):
    return hashlib.sha256(data).hexdigest()


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


def trouser_endpoint(directory, key):
    result = json.loads((directory.parent/f"trousers-result-{key}.json").read_text())
    if "refusal" in result:
        return b"", {"available": False, "reason": result["refusal"]["message"]}
    trousers = directory.parent/("trousers-"+key)
    report = json.loads((trousers/"sim_report.json").read_text())
    if (trousers/"body.obj").read_bytes() != (directory/"body.obj").read_bytes():
        raise ValueError("Outfit components used different posed bodies")
    garment, triangles = read_obj(trousers/"garment.obj")
    template = json.loads((PUBLIC/"exhibits/trousers.json").read_text())
    if (triangles.flatten().tolist() != template["triangles"] or
            digest(Path(report["pattern"]).read_bytes()) != digest((PUBLIC/"patterns/trousers.json").read_bytes())):
        raise ValueError("Trouser draft differs from the gallery pattern")
    return quantize(garment[:, :3]), {"available": True, "vertices": len(garment),
                                      "flags": report["gating"], "settled": report["numerical"]["settled"],
                                      "waistbandShiftMm": report["bands"]["waistband"]["shift_mm"]}


def export_run(directory, key, template, output):
    report = json.loads((directory/"sim_report.json").read_text())
    mesh = json.loads((directory/"mesh_map.json").read_text())
    cache = (directory/"fold.pc2").read_bytes()
    magic, version, vertices, first, rate, frames = struct.unpack_from("<12siiffi", cache)
    if (magic, version, vertices, first, rate, frames) != (
            b"POINTCACHE2\x00", 1, template["vertices"], 1., 1., template["sourceFrames"]):
        raise ValueError("Inconsistent PC2 header")
    xyz = np.frombuffer(cache, dtype="<f4", offset=32).reshape(frames, vertices, 3)
    colored, triangles = read_obj(directory/"garment_strain.obj")
    if (not np.array_equal(xyz[-1], colored[:, :3].astype("<f4"))
            or triangles.flatten().tolist() != template["triangles"]
            or mesh["frames"] != template["phases"] or mesh["seams"] != template["seams"]
            or mesh["inst_range"] != template["panels"]):
        raise ValueError("Replay and pattern topology disagree")
    if digest(Path(report["pattern"]).read_bytes()) != digest(PATTERN.read_bytes()):
        raise ValueError("Run used a different cut")
    if key == "520-35":
        (output/"flat-cut.svg").write_text(cut_drawing(xyz[0], template))
    body, faces = read_obj(directory/"body.obj")
    reference, _ = read_obj(directory.parent/"chest-520-35/body.obj")
    head = reference[:, 2] > .32
    def scene(vertices):
        return vertices[:, [0, 2, 1]] * [1, 1, -1]
    # Register the two portrait projections to corresponding head vertices in each fitted shape.
    xyz_head = np.column_stack((scene(body[head]), np.ones(head.sum())))
    fit, *_ = np.linalg.lstsq(xyz_head, scene(reference[head]), rcond=None)
    registration = np.eye(4)
    registration[:, :3] = fit
    indices, remap = np.unique(faces, return_inverse=True)
    if len(indices) >= 65536:
        raise ValueError("Upper-body surface exceeds index width")
    colors = np.rint(255 * colored[:, 3:]).astype("u1").tobytes()
    reactions = report["contact_diagnostics"]["final_reactions"]["result"]
    welded = np.asarray(reactions["positions_m"], dtype="<f4")
    lookup = {tuple(point): i for i, point in enumerate(welded)}
    if len(lookup) != len(welded):
        raise ValueError("Ambiguous welded endpoint")
    display_indices = [lookup[tuple(point)] for point in xyz[-1]]
    loads = np.linalg.norm(reactions["body_force_on_cloth_n"], axis=1)
    force = loads[display_indices].astype("<f4").tobytes()
    trousers, lower = trouser_endpoint(directory, key)
    payload = b"".join([struct.pack("<II", len(indices), faces.size), quantize(body[indices]),
                        remap.astype("<u2").tobytes(), quantize(xyz[template["frames"]]), trousers, force, colors])
    (output/(key+".bin.gz")).write_bytes(gzip.compress(payload, compresslevel=9, mtime=0))
    tape = report["body"]["fit"]["tape"]
    return {"arm": report["arm_deg"], "chest": tape["chest"], "stature": tape["stature"],
            "chestClearance": report["landmarks"]["chest"]["clearance_mean_mm"],
            "contactFraction": report["landmarks"]["chest"]["contact_frac"],
            "edgeStrainPercent": report["global_strain_pct"], "settled": report["numerical"]["settled"],
            "flags": report["gating"], "selfContact": report["self_contact"]["audit"],
            "peakBodyForceN": float(loads.max()), "trousers": lower,
            "headRegistration": registration.round(9).flatten().tolist(),
            "sourcePc2Sha256": digest(cache), "tapeResiduals": tape}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scratch", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--solve", action="store_true")
    args = parser.parse_args()
    if args.solve:
        args.scratch.mkdir(parents=True, exist_ok=False)
        for chest, angle in GRID:
            tapes = args.scratch/f"chest-{chest}.json"
            if not tapes.exists():
                tapes.write_text(json.dumps(synthetic_tapes(chest)))
            for garment, target, filename, prefix in (("bolero", "chest", "fit-spec", ""),
                                                      ("trousers", "trousers", "trouser-fit", "trousers-")):
                command = ["target/release/ftx", "drape", PUBLIC/f"patterns/{garment}.json", "--measurements", tapes,
                           "--fit", PUBLIC/f"exhibits/poses/{filename}.json", "--spacing-mm", "12",
                           "--self-contact-mm", "2", "--self-mu", "0.4", "--arm-deg", str(angle),
                           "--out", args.scratch/f"{target}-{chest}-{angle}", "--json"]
                run = subprocess.run(command, stdout=subprocess.PIPE, check=False)
                (args.scratch/f"{prefix}result-{chest}-{angle}.json").write_bytes(run.stdout)
    template = json.loads((PUBLIC/"exhibits/bolero.json").read_text())
    args.output.mkdir(parents=True, exist_ok=True)
    reports = {}
    for chest, angle in GRID:
        key = f"{chest}-{angle}"
        run = json.loads((args.scratch/f"result-{key}.json").read_text())
        if "refusal" in run:
            reports[key] = {"refusal": run["refusal"]}
        else:
            reports[key] = export_run(args.scratch/("chest-"+key), key, template, args.output)
    result = {"patternSha256": digest(PATTERN.read_bytes()), "cases": reports}
    (args.output/"runs.json").write_text(json.dumps(result, separators=(",", ":"))+"\n")


if __name__ == "__main__":
    main()
