#!/usr/bin/env python3
"""Capture drape-pose reference values from the pinned Anny package (anny==0.6.1).

The parity oracle for `ftx-body`'s drape module (crates/ftx-body/src/drape.rs, FTX-10). The poses are
FrankenTextiles definitions, not Anny's: this script implements them again in NumPy from their written
definition (the module notes of drape.rs, repeated below), runs the package's forward model with the
resulting local-bone deltas, and records the posed vertices and the skin shell (the closed shell enclosing
the most volume) for the Rust side to match.

Definition. A joint turns its bone about its head by an angle phi about a unit axis a given in the rest
pose's world frame; its local-bone delta is the rotation vector phi * R_rest^T a (R_rest the bone's rest
frame). Arms (upperarm01.L/R): d = R_rest[:, 1] (the bone's +Y), theta0 = acos(-d_z),
a = normalize(d x (0, 0, -1)), phi = theta0 - theta for an abduction theta from vertical. Legs, about
(1, 0, 0): seated = upperleg01.L/R -90, lowerleg01.L/R +90; squat = upperleg01.L/R -100,
lowerleg01.L/R +110, foot.L/R -10 (degrees). Straight arms (optional): upperarm02 onto upperarm01's rest
direction and lowerarm01 onto upperarm02's, each the minimal rotation a = normalize(d x d'),
phi = acos(d.d'), left arm then right. A ramp t scales every phi. Nappy of size s:
buttocks-volume-incr += s, hip-scale-depth-incr += s / 2.

Usage:
    python capture_anny_poses.py OUT.json [--cache-dir DIR] [--allow-other-version]

Needs the same environment as capture_anny_reference.py (whose helpers it imports from this directory).
Writes only OUT.json; the same values on every run on the same machine.
"""

from __future__ import annotations

import argparse
import json
import math
import platform
import sys
from pathlib import Path

import numpy as np
import torch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import capture_anny_reference as ref  # noqa: E402

SCHEMA = "ftx-anny-poses/1"
A_POSE_DEG = 35.0
ARMS_UP_DEG = 150.0
LEGS = {
    "standing": [],
    "seated": [("upperleg01.L", -90.0), ("upperleg01.R", -90.0), ("lowerleg01.L", 90.0), ("lowerleg01.R", 90.0)],
    "squat": [
        ("upperleg01.L", -100.0),
        ("upperleg01.R", -100.0),
        ("lowerleg01.L", 110.0),
        ("lowerleg01.R", 110.0),
        ("foot.L", -10.0),
        ("foot.R", -10.0),
    ],
}

# name, description, phenotype, nappy size, arm abduction (deg or None), legs, ramp t, straight arms
CASES = [
    ("girl_2_5y_a35", "2.5 y girl, A-pose 35 deg", ref.GIRL_2_5Y, 0.0, A_POSE_DEG, "standing", 1.0, False),
    ("girl_2_5y_a50", "2.5 y girl, arms at 50 deg", ref.GIRL_2_5Y, 0.0, 50.0, "standing", 1.0, False),
    ("girl_2_5y_arms_up", "2.5 y girl, arms up (150 deg)", ref.GIRL_2_5Y, 0.0, ARMS_UP_DEG, "standing", 1.0, False),
    ("girl_2_5y_seated", "2.5 y girl, seated, A-pose", ref.GIRL_2_5Y, 0.0, A_POSE_DEG, "seated", 1.0, False),
    ("girl_2_5y_squat", "2.5 y girl, squat, A-pose", ref.GIRL_2_5Y, 0.0, A_POSE_DEG, "squat", 1.0, False),
    ("girl_2_5y_nappy_a35", "2.5 y girl wearing a size-1 nappy, A-pose", ref.GIRL_2_5Y, 1.0, A_POSE_DEG, "standing", 1.0, False),
    ("boy_6y_a35", "6 y boy, A-pose 35 deg", ref.BOY_6Y, 0.0, A_POSE_DEG, "standing", 1.0, False),
    ("woman_seated", "adult woman, seated, A-pose", ref.ADULT_WOMAN, 0.0, A_POSE_DEG, "seated", 1.0, False),
    ("man_squat_half", "adult man, squat ramp at t = 0.5", ref.ADULT_MAN, 0.0, A_POSE_DEG, "squat", 0.5, False),
    ("girl_2_5y_drape", "2.5 y girl, the drape pose: A-pose 35 deg, straight arms", ref.GIRL_2_5Y, 0.0, A_POSE_DEG,
     "standing", 1.0, True),
]


def rodrigues(r: np.ndarray) -> np.ndarray:
    theta = float(np.linalg.norm(r))
    if theta == 0.0:
        return np.eye(3)
    k = r / theta
    kx = np.array([[0.0, -k[2], k[1]], [k[2], 0.0, -k[0]], [-k[1], k[0], 0.0]])
    return np.eye(3) + math.sin(theta) * kx + (1.0 - math.cos(theta)) * (kx @ kx)


def joints(labels: list[str], rest_frames: np.ndarray, abduction, legs: str, straight: bool) -> list[dict]:
    out = []
    if abduction is not None:
        for name in ("upperarm01.L", "upperarm01.R"):
            b = labels.index(name)
            d = rest_frames[b, :3, 1]
            d = d / np.linalg.norm(d)
            theta0 = math.acos(max(-1.0, min(1.0, -d[2])))
            a = np.cross(d, np.array([0.0, 0.0, -1.0]))
            out.append(dict(bone=name, axis=a / np.linalg.norm(a), angle=theta0 - math.radians(abduction)))
    if straight:
        for side in ("L", "R"):
            def unit(name):
                d = rest_frames[labels.index(f"{name}.{side}"), :3, 1]
                return d / np.linalg.norm(d)
            d1, d2, d3 = unit("upperarm01"), unit("upperarm02"), unit("lowerarm01")
            for name, a, b in (("upperarm02", d2, d1), ("lowerarm01", d3, d2)):
                axis = np.cross(a, b)
                if np.linalg.norm(axis) > 1e-12:
                    out.append(dict(bone=f"{name}.{side}", axis=axis / np.linalg.norm(axis),
                                    angle=math.acos(max(-1.0, min(1.0, float(a @ b))))))
    for name, deg in LEGS[legs]:
        out.append(dict(bone=name, axis=np.array([1.0, 0.0, 0.0]), angle=math.radians(deg)))
    return out


def skin(vertices: np.ndarray, faces: np.ndarray) -> dict:
    """The closed shell enclosing the most volume, its counts, volume and area, and its vertex checksums."""
    root = list(range(len(vertices)))

    def find(i):
        while root[i] != i:
            root[i] = root[root[i]]
            i = root[i]
        return i

    for f in faces:
        for k in (1, 2):
            a, b = find(int(f[0])), find(int(f[k]))
            root[max(a, b)] = min(a, b)
    tri_root = np.array([find(int(f[0])) for f in faces])
    v = vertices[faces]
    signed = np.einsum("ij,ij->i", np.cross(v[:, 0], v[:, 1]), v[:, 2]) / 6.0
    shells = {}
    for r, s in zip(tri_root.tolist(), signed.tolist()):
        shells[r] = shells.get(r, 0.0) + s
    best = max(sorted(shells), key=lambda r: abs(shells[r]))
    keep = faces[tri_root == best]
    used = []
    seen = set()
    for f in keep:
        for i in f.tolist():
            if i not in seen:
                seen.add(i)
                used.append(i)
    sv = vertices[np.array(used)]
    kv = vertices[keep]
    area = 0.5 * np.linalg.norm(np.cross(kv[:, 1] - kv[:, 0], kv[:, 2] - kv[:, 0]), axis=1)
    return {
        "vertices": len(used),
        "triangles": int(len(keep)),
        "shells_dropped": len(shells) - 1,
        "triangles_dropped": int(len(faces) - len(keep)),
        "volume": float(signed[tri_root == best].sum()),
        "area": float(area.sum()),
        "first_source_vertices": used[:5],
        "checksums": ref.checksums(sv),
    }


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("output", help="path of the JSON file to write")
    ap.add_argument("--cache-dir", default=None, help="Anny model-data cache directory (default: Anny's own)")
    ap.add_argument("--allow-other-version", action="store_true", help="do not refuse a version other than the pin")
    args = ap.parse_args(argv)

    import anny

    if anny.__version__ != ref.PINNED_VERSION and not args.allow_other_version:
        print(f"anny {anny.__version__} is installed; the pin is {ref.PINNED_VERSION}", file=sys.stderr)
        return 2
    torch.set_num_threads(1)
    model = ref.build_model(args.cache_dir)
    dtype = model.dtype
    labels = list(model.bone_labels)
    faces = model.faces.numpy()

    cases = []
    for name, description, phenotype, nappy, abduction, legs, t, straight in CASES:
        local_changes = {}
        if nappy:
            local_changes = {"buttocks-volume-incr": nappy, "hip-scale-depth-incr": 0.5 * nappy}
        rest_out, _, _ = ref.run_forward(model, phenotype, local_changes, {})
        rest_frames = rest_out["rest_bone_poses"][0].numpy()
        js = joints(labels, rest_frames, abduction, legs, straight)
        deltas = torch.eye(4, dtype=dtype).repeat(1, len(labels), 1, 1)
        recorded = []
        for j in js:
            b = labels.index(j["bone"])
            rotvec = (rest_frames[b, :3, :3].T @ j["axis"]) * (j["angle"] * t)
            m = np.eye(4)
            m[:3, :3] = rodrigues(rotvec)
            deltas[0, b] = torch.as_tensor(m, dtype=dtype)
            recorded.append(
                dict(bone=j["bone"], axis=ref.to_list(j["axis"]), angle=j["angle"], rotvec_local=ref.to_list(rotvec))
            )
        ph = {k: torch.tensor([v], dtype=dtype) for k, v in phenotype.items()}
        lc = {k: torch.tensor([v], dtype=dtype) for k, v in local_changes.items()}
        with torch.no_grad():
            out = model(pose_parameters=deltas, phenotype_kwargs=ph, local_changes_kwargs=lc)
        posed = out["vertices"][0].numpy()
        cases.append(
            {
                "name": name,
                "description": description,
                "inputs": {
                    "phenotype": phenotype,
                    "nappy": nappy,
                    "arm_abduction_deg": abduction,
                    "legs": legs,
                    "ramp_t": t,
                    "straight_arms": straight,
                },
                "joints": recorded,
                "posed_vertices": {
                    "selected": {k: ref.to_list(posed[i]) for k, i in ref.SELECTED_VERTICES.items()},
                    "bbox_min": ref.to_list(posed.min(0)),
                    "bbox_max": ref.to_list(posed.max(0)),
                    "checksums": ref.checksums(posed),
                },
                "skin": skin(posed, faces),
            }
        )
        print(f"{name}: skin volume {cases[-1]['skin']['volume']:.6f} m3", file=sys.stderr)

    result = {
        "schema": SCHEMA,
        "meta": {
            "anny_version": anny.__version__,
            "torch_version": torch.__version__,
            "numpy_version": np.__version__,
            "python_version": platform.python_version(),
            "dtype": str(dtype).replace("torch.", ""),
            "model_call": "capture_anny_reference.build_model (spec 05 section 0)",
            "definition": "crates/ftx-body/src/drape.rs module notes; repeated in this script's docstring",
            "checksums": "sum, sum of squares, index-weighted (capture_anny_reference.checksums)",
        },
        "cases": cases,
    }
    Path(args.output).write_text(json.dumps(result, indent=1, sort_keys=False) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
