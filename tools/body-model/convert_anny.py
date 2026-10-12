#!/usr/bin/env python3
"""Convert the pinned Anny body model (anny==0.6.1) into a `.ftxbody` file for `ftx-body`.

The Rust forward model reads only this file; it never runs Python. The file holds exactly the arrays of
docs/spec/05-anny-forward-model.md section 1.2, as the package builds them, plus the labels, phenotype anchors and
constants the forward model needs. Converted data stays outside the repository (default
~/.cache/franken_textiles/anny-0.6.1-default.ftxbody; override with FTX_ANNY_MODEL or --output).

Checks before anything is written:
1. the installed package is version 0.6.1 and every installed file matches the wheel's RECORD hash;
2. the model is built into a fresh cache directory (the package's cache key ignores its version, spec 05 §9.9);
3. every array's SHA-256 equals the table in spec 05 §8.1 (the package's own bytes);
4. the sparse blend-shape layout reconstructs the dense array exactly.

Usage:
    python convert_anny.py [--output PATH] [--cache-dir DIR] [--force]

Environment: anny==0.6.1 with torch (CPU is enough), e.g.
    uv venv anny-env && uv pip install --python anny-env "anny==0.6.1"

File layout (`ftxbody/1`, little-endian):
    bytes 0..8    b"FTXBODY\\0"
    bytes 8..12   u32 format version (1)
    bytes 12..16  u32 reserved (0)
    bytes 16..24  u64 header length H
    bytes 24..24+H  UTF-8 JSON header, space-padded so the data section starts 8-byte aligned
    data section  arrays at header["arrays"][name]["offset"] (relative to the data section, 8-byte aligned)
Blend shapes are stored as CSR over rows: `blendshape_row_ptr` (S+1, u32), `blendshape_vertex` (nnz, u32) and
`blendshape_delta` (nnz x 3, f64), holding every vertex whose offset in that row is not exactly zero.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import struct
import sys
import tempfile
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from capture_anny_reference import PINNED_VERSION, build_model, sha256_array  # noqa: E402

FORMAT_VERSION = 1
SCHEMA = "ftxbody/1"

# Spec 05 §8.1: SHA-256 of each array's little-endian bytes in the package's dtype.
EXPECTED_SHA256 = {
    "template_vertices": "c164a0971ec308792290e47fdc79c2acb24d1031c48635a83f04db188c48dac1",
    "faces": "72a1d21fc87e04b194835769265008a4bcac5fdb5a1b1be4846d850960c5bca1",
    "blendshapes": "017ef11b1e72715c6c5157e31f5a154a15d90aff70cefce8af3a5462a5b26cd3",
    "stacked_phenotype_blend_shapes_mask": "a6812444e83276c8ed37c1dd5ec47918199f1e1d64dda6edcbb9a3d066c5dc96",
    "template_bone_heads": "9c32b2db4c4ddfe3ebd9ba8315c87b56072b79bbc1377569af6544d7e057a02b",
    "bone_heads_blendshapes": "5b04bc24fe5c0d709b4558f6694d3fd5ed07f6efa44e1eb5ac91a4a0e199990d",
    "template_bone_tails": "747116aaf41f316ab92f61b030dd5811f58ff07fea0e166c40b8ff1d4f24b2ef",
    "bone_tails_blendshapes": "0b6effb52144da709b9c4843b7c2e806dac82198a7c541dc861099512926d578",
    "bone_rolls_rotmat": "8a1ec0eba8a5a311d11b832db7595fb8bb36727d39687255e4978adf9a911bbf",
    "vertex_bone_weights": "3ef657203687297cbeec4fe3af14cf7e07798653a795542b98ba8cd86f47fdbd",
    "vertex_bone_indices": "8e2a604bc918dba96dcf4fae66fa9e202b69f44ea61d50025f3cd71391567e77",
    "base_mesh_vertex_indices": "705a0fbcf1aa5ae17de1ca3740765099a8df911d654b2a208de9c9228861a194",
}
EXPECTED_LABEL_SHA256 = {
    "blendshape_labels": "913a1030e67b6841e25536a8b100722f161749f77f25cea83d650ea90dd88b5a",
    "bone_labels": "6e0b89c6823e9902c841f7624d61a5d78809165decf2da117edc9ffcd9289de9",
}
DTYPE_NAMES = {("f", 8): "f64", ("u", 4): "u32", ("u", 1): "u8"}


def default_output() -> Path:
    env = os.environ.get("FTX_ANNY_MODEL")
    if env:
        return Path(env)
    return Path.home() / ".cache" / "franken_textiles" / f"anny-{PINNED_VERSION}-default.ftxbody"


def verify_installation() -> list[str]:
    """Version pin and RECORD hashes of the installed distribution; returns problems (empty = fine)."""
    from importlib.metadata import distribution

    problems = []
    dist = distribution("anny")
    if dist.version != PINNED_VERSION:
        problems.append(f"anny {dist.version} is installed; the pin is {PINNED_VERSION}")
    checked = 0
    for f in dist.files or []:
        if f.hash is None:
            continue
        if f.hash.mode != "sha256":
            problems.append(f"{f}: RECORD hash mode {f.hash.mode}")
            continue
        data = Path(f.locate()).read_bytes()
        got = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
        if got != f.hash.value:
            problems.append(f"{f}: installed file differs from the wheel's RECORD")
        checked += 1
    if checked == 0:
        problems.append("the installed distribution has no RECORD hashes to check")
    return problems


def as_u32(a: np.ndarray, what: str) -> np.ndarray:
    if a.min() < 0 or a.max() >= 2**32:
        raise SystemExit(f"{what}: values do not fit u32")
    return a.astype(np.uint32)


def sparse_rows(blendshapes: np.ndarray):
    """CSR over rows of an S x V x 3 array: the vertices whose offset is not exactly zero."""
    s, v, _ = blendshapes.shape
    row_ptr = np.zeros(s + 1, dtype=np.uint64)
    verts, deltas = [], []
    for k in range(s):
        nz = np.flatnonzero(np.any(blendshapes[k] != 0.0, axis=1))
        verts.append(nz.astype(np.uint32))
        deltas.append(blendshapes[k, nz])
        row_ptr[k + 1] = row_ptr[k] + len(nz)
    if row_ptr[-1] >= 2**32:
        raise SystemExit("too many non-zero blend-shape entries for u32 row pointers")
    return row_ptr.astype(np.uint32), np.concatenate(verts), np.concatenate(deltas, axis=0)


def dense_from_sparse(row_ptr, verts, deltas, s, v) -> np.ndarray:
    out = np.zeros((s, v, 3), dtype=np.float64)
    for k in range(s):
        a, b = int(row_ptr[k]), int(row_ptr[k + 1])
        out[k, verts[a:b]] = deltas[a:b]
    return out


def write_ftxbody(path: Path, header: dict, arrays: list[tuple[str, np.ndarray]]) -> None:
    parts, entries, offset = [], {}, 0
    for name, a in arrays:
        a = np.ascontiguousarray(a)
        a = a.astype(a.dtype.newbyteorder("<"), copy=False)
        dtype = DTYPE_NAMES[(a.dtype.kind, a.dtype.itemsize)]
        raw = a.tobytes()
        pad = (-offset) % 8
        offset += pad
        entries[name] = {
            "dtype": dtype,
            "shape": list(a.shape),
            "offset": offset,
            "nbytes": len(raw),
            "sha256": hashlib.sha256(raw).hexdigest(),
        }
        parts.append((pad, raw))
        offset += len(raw)
    header = dict(header, arrays=entries)
    hj = json.dumps(header, separators=(",", ":"), ensure_ascii=True).encode()
    hj += b" " * ((-(24 + len(hj))) % 8)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=path.name + ".", suffix=".part", dir=path.parent)
    with os.fdopen(fd, "wb") as f:
        f.write(b"FTXBODY\0")
        f.write(struct.pack("<IIQ", FORMAT_VERSION, 0, len(hj)))
        f.write(hj)
        for pad, raw in parts:
            f.write(b"\0" * pad)
            f.write(raw)
    os.replace(tmp, path)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--output", type=Path, default=None, help="the .ftxbody file to write")
    ap.add_argument("--cache-dir", default=None, help="build the package's model cache here (default: a fresh temp dir)")
    ap.add_argument("--force", action="store_true", help="replace an existing output file")
    args = ap.parse_args(argv)

    out = args.output or default_output()
    if out.exists() and not args.force:
        print(f"{out} exists; pass --force to replace it", file=sys.stderr)
        return 2
    problems = verify_installation()
    if problems:
        print("refusing to convert:\n  " + "\n  ".join(problems[:20]), file=sys.stderr)
        return 2

    import torch

    torch.set_num_threads(1)
    with tempfile.TemporaryDirectory(prefix="anny-cache-") as tmp_cache:
        model = build_model(args.cache_dir or tmp_cache)
        arr = {name: getattr(model, name).detach().cpu().numpy() for name in EXPECTED_SHA256}
        anchors = {k: [float(x) for x in v.detach().cpu().numpy()] for k, v in model.anchors.named_buffers()}
        y_axis = [float(x) for x in model.y_axis.detach().cpu().numpy()]
        degenerate = model.degenerate_rotation.detach().cpu().numpy().tolist()
        bone_labels = list(model.bone_labels)
        bone_parents = [int(p) for p in model.bone_parents]
        blend_labels = list(model.blendshape_labels)
        local_labels = list(model.local_change_labels)
        phenotype_labels = list(model.phenotype_labels)

    bad = [n for n, a in arr.items() if sha256_array(a) != EXPECTED_SHA256[n]]
    for name, labels in (("blendshape_labels", blend_labels), ("bone_labels", bone_labels)):
        if hashlib.sha256("\n".join(labels).encode()).hexdigest() != EXPECTED_LABEL_SHA256[name]:
            bad.append(name)
    if bad:
        print(f"refusing to convert: arrays differ from spec 05 §8.1: {', '.join(bad)}", file=sys.stderr)
        return 2

    from anny.anthropometry import BASE_MESH_WAIST_VERTICES
    from anny.models.model_data import PHENOTYPE_VARIATIONS

    base = arr["base_mesh_vertex_indices"].tolist()
    waist_final = [base.index(i) for i in BASE_MESH_WAIST_VERTICES]

    s, v, _ = arr["blendshapes"].shape
    row_ptr, verts, deltas = sparse_rows(arr["blendshapes"])
    if not np.array_equal(dense_from_sparse(row_ptr, verts, deltas, s, v), arr["blendshapes"]):
        print("refusing to convert: the sparse blend shapes do not reconstruct the dense array", file=sys.stderr)
        return 2

    header = {
        "schema": SCHEMA,
        "model": "anny",
        "anny_version": PINNED_VERSION,
        "configuration": (
            'create_fullbody_model(rig="default", topology="default", local_changes="all", '
            "remove_unattached_vertices=True, triangulate_faces=True, pose_parameterization=\"local-bone\", "
            'bone_orientation="blender-rootidentity", skinning_method="lbs")'
        ),
        "licence": "Anny code Apache-2.0; MakeHuman/MPFB2-derived assets CC0 1.0",
        "package_sha256": EXPECTED_SHA256,
        "counts": {
            "vertices": v,
            "faces": int(arr["faces"].shape[0]),
            "bones": len(bone_labels),
            "blendshapes": s,
            "phenotype_blendshapes": int(arr["stacked_phenotype_blend_shapes_mask"].shape[0]),
            "local_changes": len(local_labels),
            "influences": int(arr["vertex_bone_indices"].shape[1]),
            "blendshape_nonzero_vertices": int(row_ptr[-1]),
        },
        "phenotype": {
            "features": list(PHENOTYPE_VARIATIONS.keys()),
            "components": [[f, c] for f, comps in PHENOTYPE_VARIATIONS.items() for c in comps],
            "anchors": anchors,
            "exposed": phenotype_labels,
            "fixed_value": 0.5,
        },
        "y_axis": y_axis,
        "degenerate_rotation": degenerate,
        "labels": {
            "blendshapes": blend_labels,
            "bones": bone_labels,
            "bone_parents": bone_parents,
            "local_changes": local_labels,
        },
        "waist_loop_final": waist_final,
    }
    arrays = [
        ("template_vertices", arr["template_vertices"]),
        ("faces", as_u32(arr["faces"], "faces")),
        ("blendshape_row_ptr", row_ptr),
        ("blendshape_vertex", verts),
        ("blendshape_delta", deltas),
        ("phenotype_mask", arr["stacked_phenotype_blend_shapes_mask"].astype(np.uint8)),
        ("template_bone_heads", arr["template_bone_heads"]),
        ("template_bone_tails", arr["template_bone_tails"]),
        ("bone_heads_blendshapes", arr["bone_heads_blendshapes"]),
        ("bone_tails_blendshapes", arr["bone_tails_blendshapes"]),
        ("bone_rolls", arr["bone_rolls_rotmat"][0]),
        ("vertex_bone_indices", as_u32(arr["vertex_bone_indices"], "vertex_bone_indices")),
        ("vertex_bone_weights", arr["vertex_bone_weights"]),
        ("base_mesh_vertex_indices", as_u32(arr["base_mesh_vertex_indices"], "base_mesh_vertex_indices")),
    ]
    if not np.array_equal(arr["stacked_phenotype_blend_shapes_mask"], arrays[5][1].astype(np.float64)):
        print("refusing to convert: the phenotype mask is not 0/1", file=sys.stderr)
        return 2
    write_ftxbody(out, header, arrays)
    print(f"wrote {out} ({out.stat().st_size / 1e6:.1f} MB, {int(row_ptr[-1]):,} non-zero blend-shape vertices)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
