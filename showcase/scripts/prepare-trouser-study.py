"""Run and package the trouser posture and elastic experiments. Use new output directories."""
import argparse
import gzip
import hashlib
import json
import struct
import subprocess
from pathlib import Path
from runpy import run_path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
PUBLIC = ROOT / "showcase/public"
PATTERN = PUBLIC / "patterns/trousers.json"
FIT = PUBLIC / "exhibits/poses/trouser-fit.json"
shared = run_path(str(Path(__file__).with_name("prepare-drape-room.py")))
read_obj, quantize = shared["read_obj"], shared["quantize"]
# The skirt and sundress studies' cloth model: the cloth resists shortening at 5 % of its tensile stiffness on
# the principal strains, so loose cloth folds instead of standing out as a shell, and hems and allowances carry
# the solver's uncalibrated construction weighting. Model choices, not a measured fabric.
CLOTH_MODEL = ["--compression", "0.05", "--compression-model", "principal", "--construction-standard"]
CASES = [
    ("standing", "Standing", "standing", 1, 15),
    ("sit-12", "Shallow seat", "seated", .125, 15),
    ("squat-12", "Shallow squat", "squat", .125, 15),
    ("squat-25", "Quarter squat", "squat", .25, 15),
    ("seated", "Full seat", "seated", 1, 15),
    ("squat", "Full squat", "squat", 1, 15),
    *((f"elastic-{ea}", f"{ea} N", "standing", 1, ea) for ea in (0, 5, 30, 60)),
]


def store(path, data):
    path.write_bytes(gzip.compress(data, compresslevel=9, mtime=0))


def run_cases(scratch):
    scratch.mkdir(parents=True, exist_ok=False)
    tapes = scratch / "tapes.json"
    tapes.write_text(json.dumps(shared["synthetic_tapes"](520)))
    ftx = shared["FTX"]
    for pose in ("drape", "seated", "squat"):
        subprocess.run([ftx, "body", "--measurements", tapes, "--pose", pose,
                        "--out", scratch / ("body-" + pose), "--json"], check=True, stdout=subprocess.DEVNULL)
    for key, _, posture, fraction, stiffness in CASES:
        command = [ftx, "drape", PATTERN, "--fit", FIT, "--measurements", tapes, "--spacing-mm", "20" if key=="squat-25" else "12",
                   "--self-contact-mm", "2", "--self-mu", "0.4", "--arm-deg", "35", "--elastic-ea",
                   str(stiffness), "--legs", posture, "--leg-fraction", str(fraction), "--out", scratch/key, "--json",
                   *CLOTH_MODEL]
        run = subprocess.run(command, capture_output=True)
        if not run.stdout or (run.returncode and "refusal" not in json.loads(run.stdout)):
            raise RuntimeError(run.stderr.decode())
        (scratch/(key+".json")).write_bytes(run.stdout)


def package(scratch, output):
    output.mkdir(parents=True, exist_ok=False)
    reference, reference_faces = read_obj(scratch/"body-drape/body.obj")
    corners = reference[reference_faces]
    # A cropped tailor's lower-body form. Preserve vertex correspondence for flexed postures.
    mask = (corners[:, :, 2].min(1) < .135) & (np.abs(corners[:, :, 0]).max(1) < .18)
    # Cropping an arms-down form can also intersect the forearms. Keep the connected pelvis and legs.
    cropped = reference_faces[mask]
    neighbors = {}
    for a, b, c in cropped:
        for vertex, pair in ((a, (b,c)), (b, (a,c)), (c, (a,b))):
            neighbors.setdefault(vertex, set()).update(pair)
    sole = min(neighbors, key=lambda i: reference[i, 2])
    keep, frontier = {sole}, [sole]
    while frontier:
        for neighbor in neighbors[frontier.pop()] - keep:
            keep.add(neighbor)
            frontier.append(neighbor)
    indices, faces = np.unique(cropped[np.isin(cropped[:, 0], list(keep))], return_inverse=True)
    template = json.loads((PUBLIC/"exhibits/trousers.json").read_text())
    count = template["vertices"]
    current = (scratch/"standing/fold.pc2").read_bytes()
    if struct.unpack_from("<12sii", current) != (b"POINTCACHE2\x00", 1, count):
        raise ValueError("Unexpected native point-cache layout")
    flat = np.frombuffer(current, dtype="<f4", count=count*3, offset=32).reshape(-1, 3)
    store(output/"mesh.gz", faces.astype("<u2").tobytes() + quantize(flat[:, [0, 2]]))
    pattern_hash = hashlib.sha256(PATTERN.read_bytes()).hexdigest()
    rows = []
    for key, label, posture, fraction, stiffness in CASES:
        outcome = json.loads((scratch/(key+".json")).read_text())
        row = dict(id=key, label=label, posture=posture, fraction=fraction, stiffness=stiffness)
        rows.append(row)
        if "refusal" in outcome:
            row["refusal"] = outcome["refusal"]
            continue
        else:
            folder = scratch/key
            shared["check_contact"](folder)
            report = json.loads((folder/"sim_report.json").read_text())
            if hashlib.sha256(Path(report["pattern"]).read_bytes()).hexdigest() != pattern_hash:
                raise ValueError("Different trouser cut in " + key)
            body, topology = read_obj(folder/"body.obj")
            cloth, triangles = read_obj(folder/"garment_strain.obj")
            pc2=(folder/"fold.pc2").read_bytes()
            mesh_map=json.loads((folder/"mesh_map.json").read_text())
            phases=mesh_map["frames"]
            steps=sum(phases.values())
            assert struct.unpack_from("<12siiffi",pc2)==(b"POINTCACHE2\0",1,len(cloth),1.,1.,steps)
            trajectory=np.frombuffer(pc2,dtype="<f4",offset=32).reshape(steps,len(cloth),3)
            assert np.array_equal(trajectory[-1],cloth[:,:3].astype("<f4"))
            kept=sorted(set(range(0,steps,4)) | {0,phases["fold"]-1,phases["fold"]+phases["zip"]-1,steps-1})
            store(output/(key+"-sewing.gz"),quantize(trajectory[kept]))
            row.update(frames=kept,phases=phases,pc2Sha256=hashlib.sha256(pc2).hexdigest())
            if key=="squat-25":
                flat_run=trajectory[0]
                store(output/("mesh-"+key+".gz"),faces.astype("<u2").tobytes()+quantize(flat_run[:,[0,2]]))
                (output/(key+"-topology.json")).write_text(json.dumps({"triangles":triangles.flatten().tolist(),"seams":mesh_map["seams"],"panels":mesh_map["inst_range"]},separators=(",",":"))+"\n")
                row.update(clothVertices=len(cloth),spacingMm=20,meshKey=key)
            elif triangles.flatten().tolist() != template["triangles"] or len(cloth) != count:
                raise ValueError("Different garment topology in " + key)
            colors = np.rint(cloth[:, 3:] * 255).astype("u1")
            payload = quantize(cloth[:, :3]) + colors.tobytes()
            row.update(seatGap=report["landmarks"]["seat"]["clearance_mean_mm"],
                       waistbandShift=report["bands"]["waistband"]["shift_mm"],
                       strain=report["global_strain_pct"], flags=report["gating"],
                       settled=report["numerical"]["settled"])
        if not np.array_equal(topology, reference_faces):
            raise ValueError("Unexpected body topology in " + key)
        store(output/(key+".gz"), quantize(body[indices]) + payload)
        row["surface"] = key+".gz"
    (output/"study.json").write_text(json.dumps(dict(cases=rows, bodyVertices=len(indices), bodyIndices=faces.size,
        patternSha256=pattern_hash, clothVertices=count, model="anny 0.6.1", spacingMm=12, selfContactMm=2,
        solverArguments=CLOTH_MODEL, tapes=shared["synthetic_tapes"](520)), separators=(",", ":"))+"\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scratch", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--solve", action="store_true")
    args = parser.parse_args()
    if args.solve:
        run_cases(args.scratch)
    package(args.scratch, args.output)
