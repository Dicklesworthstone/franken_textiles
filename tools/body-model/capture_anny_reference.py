#!/usr/bin/env python3
"""Capture forward-model reference values from the pinned Anny package (anny==0.6.1).

These values are the parity oracle for the Rust port in `ftx-body` (docs/spec/05-anny-forward-model.md, section 8).
The script only calls the installed package. It needs no network, writes nothing except the JSON output file, and
gives the same values on every run on the same machine. Anny itself keeps a model-data cache under
~/.cache/anny (or $ANNY_CACHE_DIR, or --cache-dir); the first run builds it from the bundled MPFB2 assets, which
takes about 20 s.

Usage:
    python capture_anny_reference.py OUT.json [--cache-dir DIR] [--timing-repeats N] [--allow-other-version]

Model: create_fullbody_model(rig="default", topology="default", local_changes="all",
remove_unattached_vertices=True, triangulate_faces=True, pose_parameterization="local-bone",
bone_orientation="blender-rootidentity", skinning_method="lbs"). The model runs in float64 (the package's own
dtype). Skinning is forced to the pure-torch "lbs" path so that the result does not depend on whether NVIDIA Warp
is installed. Warp's kernel agrees with it to about 4e-16 m.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import platform
import statistics
import sys
import time
import warnings

import numpy as np
import torch

PINNED_VERSION = "0.6.1"
SCHEMA = "ftx-anny-reference/1"

# Vertex indices in the final 13,718-vertex mesh, chosen once on the default body (all phenotypes 0.5) and then
# frozen. The comment gives the base-mesh (MakeHuman base.obj) index.
SELECTED_VERTICES = {
    "first_vertex": 0,  # base 0, inner mouth/face area
    "nose_tip": 297,  # base 297
    "top_of_head": 881,  # base 881
    "right_hand_outermost": 2301,  # base 2317
    "right_elbow_near_joint": 3370,  # base 3386
    "back_or_buttock_rearmost": 4202,  # base 4218
    "sole_lowest": 6148,  # base 6164
    "left_hand_outermost": 8953,  # base 8985
    "waist_front": 10725,  # base 10757 (on the anthropometry waist loop)
    "left_knee_near_joint": 11206,  # base 11238
    "tongue_first": 13348,  # base 13380 (helper-tongue)
    "left_eye_first": 13574,  # base 14598 (helper-l-eye)
    "last_vertex": 13717,  # base 14741 (helper-r-eye)
}

SELECTED_BONES = [
    "root",
    "upperleg01.L",
    "lowerleg01.L",
    "foot.L",
    "spine01",
    "upperarm01.L",
    "lowerarm01.L",
    "wrist.L",
    "upperarm01.R",
    "lowerleg01.R",
    "head",
]


def P(gender=0.5, age=0.5, muscle=0.5, weight=0.5, height=0.5, proportions=0.5):
    return dict(gender=gender, age=age, muscle=muscle, weight=weight, height=height, proportions=proportions)


ADULT_MAN = P(gender=0.0, age=0.7857)
ADULT_WOMAN = P(gender=1.0, age=0.7857)
BOY_6Y = P(gender=0.0, age=0.2721)
GIRL_2_5Y = P(gender=1.0, age=0.1325)

# gender: 0 = male, 1 = female. age: anchors newborn -1/3, baby 0, child 1/3, young 2/3, old 1; the package's
# WHO-calibrated mapping puts 0 y at 0.0, 1 y at 0.05, 4 y at 0.215, 11 y at 0.415, 16 y at 0.67, 18 y at 0.77.
PHENOTYPE_CASES = [
    ("default_all_half", "every phenotype 0.5 (the package default; about 12.7 y)", P(), {}),
    ("newborn_anchor", "age -1/3 = the newborn anchor (baby targets scaled 0.922/0.922/0.75)", P(age=-1.0 / 3.0), {}),
    ("birth_who", "age 0.0 = baby anchor = 0 y on the WHO mapping", P(age=0.0), {}),
    ("one_year", "age 0.05 = 1 y on the WHO mapping", P(age=0.05), {}),
    ("toddler_girl_2_5y", "girl, age 0.1325 = 2.5 y", GIRL_2_5Y, {}),
    ("boy_6y", "boy, age 0.2721 = 6.0 y", BOY_6Y, {}),
    ("twelve_years_mixed", "gender 0.5, age 0.466 = 12 y", P(age=0.466), {}),
    ("adult_man", "man, age 0.7857 = 30 y", ADULT_MAN, {}),
    ("adult_woman", "woman, age 0.7857 = 30 y", ADULT_WOMAN, {}),
    (
        "extreme_corner",
        "old man, max muscle, max weight, min height, uncommon proportions",
        P(gender=0.0, age=1.0, muscle=1.0, weight=1.0, height=0.0, proportions=1.0),
        {},
    ),
    (
        "out_of_range_clamped",
        "inputs outside the anchor ranges; must equal (1, -1/3, 0, 1, 1, 0)",
        P(gender=1.3, age=-0.6, muscle=-0.2, weight=1.5, height=2.0, proportions=-1.0),
        {},
    ),
]

LOCAL_CHANGE_CASES = [
    (
        "woman_waist_up_hips_down",
        "adult woman, measure-waist-circ-incr +0.5, measure-hips-circ-incr -0.5",
        ADULT_WOMAN,
        {"measure-waist-circ-incr": 0.5, "measure-hips-circ-incr": -0.5},
    ),
    (
        "man_bust_leg_belly",
        "adult man, bust +1, upper-leg height -1, stomach-pregnant +0.6",
        ADULT_MAN,
        {"measure-bust-circ-incr": 1.0, "measure-upperleg-height-incr": -1.0, "stomach-pregnant-incr": 0.6},
    ),
    (
        "toddler_waist_beyond_one",
        "toddler girl, waist +1.5 (local changes are not clamped), torso-scale-horiz -0.4",
        GIRL_2_5Y,
        {"measure-waist-circ-incr": 1.5, "torso-scale-horiz-incr": -0.4},
    ),
]


def rotvec_deg_to_homogeneous(rotvec_deg, translation=(0.0, 0.0, 0.0)):
    """Rodrigues' formula in float64. The Rust side reads the recorded matrices, so it never has to match this."""
    r = np.radians(np.asarray(rotvec_deg, dtype=np.float64))
    theta = float(np.linalg.norm(r))
    rot = np.eye(3)
    if theta > 0.0:
        k = r / theta
        kx = np.array([[0.0, -k[2], k[1]], [k[2], 0.0, -k[0]], [-k[1], k[0], 0.0]])
        rot = np.eye(3) + math.sin(theta) * kx + (1.0 - math.cos(theta)) * (kx @ kx)
    out = np.eye(4)
    out[:3, :3] = rot
    out[:3, 3] = np.asarray(translation, dtype=np.float64)
    return out


# local-bone deltas: a 4x4 rigid transform per bone, in the bone's own rest frame (+Y along the bone), applied
# after the rest pose. For "root" the delta is the root's world pose (identity puts the root bone head at the origin).
POSE_CASES = [
    (
        "pose_arms_left_up_right_down",
        "adult woman; upperarm01.L +60 deg about its local +Z (raises the left arm), "
        "upperarm01.R +40 deg about its local +Z (lowers the right arm)",
        ADULT_WOMAN,
        {},
        {
            "upperarm01.L": dict(rotvec_deg=[0.0, 0.0, 60.0]),
            "upperarm01.R": dict(rotvec_deg=[0.0, 0.0, 40.0]),
        },
    ),
    (
        "pose_seated",
        "adult man; root yawed 15 deg about world +Z and moved (0.05, -0.10, -0.30) m; hips flexed 90 deg "
        "(upperleg01.L/R -90 deg about local +X); knees flexed 90 deg (lowerleg01.L/R +90 deg about local +X)",
        ADULT_MAN,
        {},
        {
            "root": dict(rotvec_deg=[0.0, 0.0, 15.0], translation=[0.05, -0.10, -0.30]),
            "upperleg01.L": dict(rotvec_deg=[-90.0, 0.0, 0.0]),
            "upperleg01.R": dict(rotvec_deg=[-90.0, 0.0, 0.0]),
            "lowerleg01.L": dict(rotvec_deg=[90.0, 0.0, 0.0]),
            "lowerleg01.R": dict(rotvec_deg=[90.0, 0.0, 0.0]),
        },
    ),
]


def sha256_array(a: np.ndarray) -> str:
    a = np.ascontiguousarray(a)
    a = a.astype(a.dtype.newbyteorder("<"), copy=False)
    return hashlib.sha256(a.tobytes()).hexdigest()


def checksums(points: np.ndarray) -> dict:
    """Order-sensitive checksums of an (N, 3) float64 array (definitions repeated in the JSON meta)."""
    n = points.shape[0]
    idx = np.arange(1, n + 1, dtype=np.float64)
    return {
        "sum": float(points.sum()),
        "sum_sq": float((points * points).sum()),
        "index_weighted": float((idx * (points @ np.array([1.0, 2.0, 3.0]))).sum() / n),
    }


def to_list(a) -> list:
    return np.asarray(a, dtype=np.float64).tolist()


def build_model(cache_dir: str | None):
    import anny

    if cache_dir:
        anny.paths.set_anny_cache_path(cache_dir)
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        model = anny.create_fullbody_model(
            rig="default",
            topology="default",
            local_changes="all",
            remove_unattached_vertices=True,
            triangulate_faces=True,
            pose_parameterization="local-bone",
            bone_orientation="blender-rootidentity",
            skinning_method="lbs",
        )
    model.eval()
    return model


def pose_tensor(model, pose_spec: dict, dtype) -> tuple[torch.Tensor, dict]:
    deltas = torch.eye(4, dtype=dtype).repeat(1, len(model.bone_labels), 1, 1)
    recorded = {}
    for bone, spec in pose_spec.items():
        mat = rotvec_deg_to_homogeneous(spec["rotvec_deg"], spec.get("translation", (0.0, 0.0, 0.0)))
        deltas[0, model.bone_labels.index(bone)] = torch.as_tensor(mat, dtype=dtype)
        recorded[bone] = dict(spec, matrix=mat.tolist())
    return deltas, recorded


def run_forward(model, phenotype: dict, local_changes: dict, pose_spec: dict):
    dtype = model.dtype
    pose, recorded_pose = pose_tensor(model, pose_spec, dtype)
    ph = {k: torch.tensor([v], dtype=dtype) for k, v in phenotype.items()}
    lc = {k: torch.tensor([v], dtype=dtype) for k, v in local_changes.items()}
    with torch.no_grad():
        out = model(pose_parameters=pose, phenotype_kwargs=ph, local_changes_kwargs=lc)
        # Private helpers, used only to record the blend-shape coefficients for debugging the port.
        _, phen, loc, fac = model.get_tensor_inputs(pose, ph, lc, None)
        coeffs = model._get_phenotype_blendshape_coefficients(phen, loc, fac)[0]
    return out, coeffs, recorded_pose


def describe_case(model, anthropometry, name, description, phenotype, local_changes, pose_spec, model32):
    out, coeffs, recorded_pose = run_forward(model, phenotype, local_changes, pose_spec)
    rest = out["rest_vertices"][0].numpy()
    posed = out["vertices"][0].numpy()
    labels = model.blendshape_labels
    n_phen = int(model.stacked_phenotype_blend_shapes_mask.shape[0])
    c = coeffs.numpy()
    nonzero_phen = [[labels[i], float(c[i])] for i in range(n_phen) if c[i] != 0.0]
    with torch.no_grad():
        anth = {k: float(v[0]) for k, v in anthropometry(out["rest_vertices"]).items()}
    bones = {}
    for b in SELECTED_BONES:
        i = model.bone_labels.index(b)
        bones[b] = {
            "index": i,
            "rest_head": to_list(out["rest_bone_heads"][0, i]),
            "rest_tail": to_list(out["rest_bone_tails"][0, i]),
            "rest_pose": to_list(out["rest_bone_poses"][0, i]),
            "posed_pose": to_list(out["bone_poses"][0, i]),
        }
    out32, _, _ = run_forward(model32, phenotype, local_changes, pose_spec)
    return {
        "name": name,
        "description": description,
        "inputs": {
            "phenotype": phenotype,
            "local_changes": local_changes,
            "pose_local_bone": recorded_pose,
        },
        "vertex_count": int(rest.shape[0]),
        "face_count": int(model.faces.shape[0]),
        "rest_vertices": {
            "selected": {k: to_list(rest[i]) for k, i in SELECTED_VERTICES.items()},
            "bbox_min": to_list(rest.min(0)),
            "bbox_max": to_list(rest.max(0)),
            "checksums": checksums(rest),
        },
        "posed_vertices": {
            "selected": {k: to_list(posed[i]) for k, i in SELECTED_VERTICES.items()},
            "bbox_min": to_list(posed.min(0)),
            "bbox_max": to_list(posed.max(0)),
            "checksums": checksums(posed),
        },
        "bones": bones,
        "bone_checksums": {
            "rest_heads": checksums(out["rest_bone_heads"][0].numpy()),
            "rest_tails": checksums(out["rest_bone_tails"][0].numpy()),
            "posed_translations": checksums(out["bone_poses"][0, :, :3, 3].numpy()),
        },
        "anthropometry_on_rest_vertices": anth,
        "nonzero_phenotype_coefficients": nonzero_phen,
        "float32_model_max_abs_dev_m": {
            "rest_vertices": float((out32["rest_vertices"][0].double() - out["rest_vertices"][0]).abs().max()),
            "posed_vertices": float((out32["vertices"][0].double() - out["vertices"][0]).abs().max()),
        },
    }


def time_forward(model, phenotype, local_changes, pose_spec, repeats: int) -> dict:
    for _ in range(5):
        run_forward(model, phenotype, local_changes, pose_spec)
    dtype = model.dtype
    pose, _ = pose_tensor(model, pose_spec, dtype)
    ph = {k: torch.tensor([v], dtype=dtype) for k, v in phenotype.items()}
    lc = {k: torch.tensor([v], dtype=dtype) for k, v in local_changes.items()}
    samples = []
    with torch.no_grad():
        for _ in range(repeats):
            t0 = time.perf_counter()
            model(pose_parameters=pose, phenotype_kwargs=ph, local_changes_kwargs=lc)
            samples.append((time.perf_counter() - t0) * 1000.0)
    return {
        "median_ms": statistics.median(samples),
        "min_ms": min(samples),
        "repeats": repeats,
    }


def data_hashes(model) -> dict:
    tensors = {
        "template_vertices": model.template_vertices,
        "faces": model.faces,
        "blendshapes": model.blendshapes,
        "stacked_phenotype_blend_shapes_mask": model.stacked_phenotype_blend_shapes_mask,
        "template_bone_heads": model.template_bone_heads,
        "bone_heads_blendshapes": model.bone_heads_blendshapes,
        "template_bone_tails": model.template_bone_tails,
        "bone_tails_blendshapes": model.bone_tails_blendshapes,
        "bone_rolls_rotmat": model.bone_rolls_rotmat,
        "vertex_bone_weights": model.vertex_bone_weights,
        "vertex_bone_indices": model.vertex_bone_indices,
        "base_mesh_vertex_indices": model.base_mesh_vertex_indices,
    }
    res = {}
    for k, t in tensors.items():
        a = t.detach().cpu().numpy()
        res[k] = {"shape": list(a.shape), "dtype": str(a.dtype), "sha256_le": sha256_array(a)}
    return res


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("output", help="path of the JSON file to write")
    ap.add_argument("--cache-dir", default=None, help="Anny model-data cache directory (default: Anny's own)")
    ap.add_argument("--timing-repeats", type=int, default=50)
    ap.add_argument("--allow-other-version", action="store_true", help="do not refuse a version other than the pin")
    args = ap.parse_args(argv)

    import anny
    from anny.anthropometry import BASE_MESH_WAIST_VERTICES
    from anny.shape_distribution import MorphologicalAgeMapping

    if anny.__version__ != PINNED_VERSION and not args.allow_other_version:
        print(f"anny {anny.__version__} is installed; the pin is {PINNED_VERSION}", file=sys.stderr)
        return 2

    default_threads = torch.get_num_threads()
    torch.set_num_threads(1)  # single-threaded reductions: the same bits on every run

    t0 = time.perf_counter()
    model = build_model(args.cache_dir)
    load_s = time.perf_counter() - t0
    model32 = copy.deepcopy(model).to(torch.float32)
    anthropometry = anny.Anthropometry(model)

    calib = torch.load(
        anny.paths.get_anny_root_dir() / "data" / "shape_calibration" / "boys.pth", weights_only=True, map_location="cpu"
    )["morphological_age_mapping"]
    age_map = MorphologicalAgeMapping()
    age_map.load_state_dict(calib)

    def years(age: float) -> float:
        return float(age_map.anny_to_morphological_age(torch.tensor([age], dtype=torch.float64))[0])

    bmi = model.base_mesh_vertex_indices.tolist()
    cases = []
    for group, specs in (
        ("phenotype", [(n, d, p, lc, {}) for n, d, p, lc in PHENOTYPE_CASES]),
        ("local_changes", [(n, d, p, lc, {}) for n, d, p, lc in LOCAL_CHANGE_CASES]),
        ("pose", POSE_CASES),
    ):
        for name, description, phenotype, local_changes, pose_spec in specs:
            case = describe_case(
                model, anthropometry, name, description, phenotype, local_changes, pose_spec, model32
            )
            case["group"] = group
            case["morphological_age_years"] = years(min(max(phenotype["age"], -1.0 / 3.0), 1.0))
            cases.append(case)

    timing = {
        "threads_1": {
            "identity_pose_default_body": time_forward(model, P(), {}, {}, args.timing_repeats),
            "posed_with_local_changes": time_forward(
                model, ADULT_MAN, LOCAL_CHANGE_CASES[1][3], POSE_CASES[1][4], args.timing_repeats
            ),
        }
    }
    torch.set_num_threads(default_threads)
    timing[f"threads_{default_threads}"] = {
        "identity_pose_default_body": time_forward(model, P(), {}, {}, args.timing_repeats),
        "posed_with_local_changes": time_forward(
            model, ADULT_MAN, LOCAL_CHANGE_CASES[1][3], POSE_CASES[1][4], args.timing_repeats
        ),
    }

    result = {
        "schema": SCHEMA,
        "meta": {
            "anny_version": anny.__version__,
            "torch_version": torch.__version__,
            "numpy_version": np.__version__,
            "python_version": platform.python_version(),
            "machine": platform.machine(),
            "system": platform.system(),
            "dtype": str(model.dtype).replace("torch.", ""),
            "capture_threads": 1,
            "model_call": (
                'anny.create_fullbody_model(rig="default", topology="default", local_changes="all", '
                "remove_unattached_vertices=True, triangulate_faces=True, pose_parameterization=\"local-bone\", "
                'bone_orientation="blender-rootidentity", skinning_method="lbs")'
            ),
            "forward_call": "model(pose_parameters=B x 163 x 4 x 4, phenotype_kwargs={name: tensor([v])}, "
            "local_changes_kwargs={label: tensor([v])})",
            "model_load_seconds": load_s,
            "units": "metres; +Z up; the body faces -Y; +X is the body's left",
            "checksum_definitions": {
                "sum": "sum of all coordinates",
                "sum_sq": "sum of squared coordinates",
                "index_weighted": "sum over rows i=1..N of i*(x_i + 2*y_i + 3*z_i), divided by N",
            },
            "rest_vs_posed": "rest_vertices = output['rest_vertices'] (template frame, before skinning); "
            "posed_vertices = output['vertices'] (after local-bone FK and LBS; the identity pose puts the root bone "
            "head at the origin)",
            "anthropometry": "anny.Anthropometry(model)(output['rest_vertices']); density 980 kg/m^3",
        },
        "model": {
            "vertex_count": int(model.template_vertices.shape[0]),
            "face_count": int(model.faces.shape[0]),
            "bone_count": len(model.bone_labels),
            "blendshape_count": int(model.blendshapes.shape[0]),
            "phenotype_blendshape_count": int(model.stacked_phenotype_blend_shapes_mask.shape[0]),
            "local_change_count": len(model.local_change_labels),
            "max_bones_per_vertex": int(model.vertex_bone_indices.shape[1]),
            "phenotype_labels": list(model.phenotype_labels),
            "bone_labels": list(model.bone_labels),
            "bone_parents": list(model.bone_parents),
            "local_change_labels": list(model.local_change_labels),
            "waist_loop_final_indices": [bmi.index(i) for i in BASE_MESH_WAIST_VERTICES],
            "selected_vertices": SELECTED_VERTICES,
            "data_hashes": data_hashes(model),
            "label_hashes": {
                "blendshape_labels": hashlib.sha256("\n".join(model.blendshape_labels).encode()).hexdigest(),
                "bone_labels": hashlib.sha256("\n".join(model.bone_labels).encode()).hexdigest(),
            },
        },
        "cases": cases,
        "timing_forward_batch1": timing,
    }
    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=1, sort_keys=False)
        f.write("\n")
    print(f"wrote {len(cases)} cases to {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
