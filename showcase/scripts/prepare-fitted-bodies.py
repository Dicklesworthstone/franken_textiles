"""Bake the two wardrobe displays from synthetic targets and the locally converted Anny model.

Requires Python 3 with NumPy, ImageMagick, the pinned Rust toolchain, the Anny 0.6.1 wheel
and a locally converted .ftxbody file.
Write to a new directory. Native coefficients, the template and unpacked intermediate meshes
are never part of the published bundle.
"""
import argparse
import concurrent.futures
import gzip
import hashlib
import importlib.util
import json
import os
import subprocess
import struct
import tempfile
import zipfile
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
spec = importlib.util.spec_from_file_location("sundress", HERE / "prepare-sundress.py")
sundress = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sundress)
BASE = {"height": 168, "bust": 90, "waist": 72, "weight": 52}
DOMAINS = {"height": (154, 184), "bust": (78, 108), "waist": (60, 94), "weight": (45, 74)}
ANGLES = [4, 35, 65, 95, 125]


def cases():
    yield "reference", BASE
    for axis, limits in DOMAINS.items():
        for suffix, value in zip(("Low", "High"), limits):
            yield axis + suffix, {**BASE, axis: value}
    yield "hussar", {"height": 178, "bust": 100, "waist": 86, "weight": 73}


def measurements(case, controls):
    male = case == "hussar"
    height = controls["height"] * 10
    document = sundress.body(height, controls["bust"] * 10, controls["waist"] * 10, 980 if male else 960)
    document["wearer"].update(weight_kg=controls["weight"], sex="male" if male else "female")
    document["wearer"]["label"] = "Synthetic wardrobe display"
    # Keep the leg and torso proportions coherent as stature changes.
    for key in ("waist_height", "back_neck_to_waist", "crotch_height", "arm_length", "knee_height"):
        document["fused"][key]["value_mm"] = round(document["fused"][key]["value_mm"] * height / 1680)
    if male:
        for key, value in {"neck_base": 390, "shoulder_length": 145, "head": 580}.items():
            document["fused"][key]["value_mm"] = value
    return document


def native_sample(args):
    case, controls, evaluator, model, working = args
    folder = working / case
    folder.mkdir()
    measure = folder / "synthetic.json"
    measure.write_text(json.dumps(measurements(case, controls)))
    subprocess.run([evaluator, model, measure, folder, "male" if case == "hussar" else "female"], check=True)
    return case


def compile_evaluator(working):
    env = dict(os.environ, CARGO_TARGET_DIR=str(working / "target"))
    subprocess.run(["cargo", "build", "--locked", "--release", "-p", "ftx-body", "--example",
                    "pose-display-form"], cwd=ROOT, env=env, check=True)
    return working / "target/release/examples" / ("pose-display-form.exe" if os.name=="nt" else "pose-display-form")


def topology(folder):
    source = json.loads((folder / "topology.json").read_text())
    skin = np.array(source["skin_order"])
    skin_faces = np.array(source["skin_faces"])
    faces = np.array(source["faces"])
    rest = np.fromfile(folder / "4.f32", "<f4").reshape(-1, 3)
    extras = faces[~np.isin(faces[:, 0], skin)]
    adjacency = {}
    for a, b, c in extras.tolist():
        for u, v in [(a,b), (b,c), (c,a)]:
            adjacency.setdefault(u, set()).add(v)
            adjacency.setdefault(v, set()).add(u)
    components = []
    unseen = set(adjacency)
    while unseen:
        stack = [unseen.pop()]
        found = []
        while stack:
            node = stack.pop()
            found.append(node)
            for neighbor in adjacency[node] & unseen:
                unseen.remove(neighbor)
                stack.append(neighbor)
        components.append(np.array(sorted(found)))
    if len(components) != 3:
        raise ValueError("Expected the tongue and the two eye surfaces in addition to skin")
    eyes = sorted(sorted(components, key=lambda c: rest[c,1].mean())[-2:], key=lambda c: -rest[c,0].mean())
    order = np.concatenate([skin, *eyes])
    remap = np.full(len(rest), -1)
    remap[order] = np.arange(len(order))
    eye_faces = extras[np.isin(extras[:,0], np.concatenate(eyes))]
    index = np.vstack([skin_faces, remap[eye_faces]])
    region = np.fromfile(folder / "regions.bin", "u1")[order]
    return order, index, region, len(skin_faces)*3, eyes


def center_at_level(xyz, regions, y):
    band = xyz[(np.abs(xyz[:,1]-y) < 1.6) & (regions == 0)]
    return float((band[:,2].min() + band[:,2].max()) * .5)


def read_form(folder, layout):
    order, _, _, _, eyes = layout
    regions = np.fromfile(folder / "regions.bin", "u1")
    fit = json.loads((folder / "fit.json").read_text())
    tape_chest = next((p["landmarks"]["chest"][1] for p in fit["poses"] if p["degrees"] == 35), None)
    geometry, marks = [], []
    for pose in fit["poses"]:
        xyz = np.fromfile(folder / f'{pose["degrees"]}.f32', "<f4").reshape(-1,3)
        anchors = pose["landmarks"]
        # The armpit moves with abduction. A bust tape always refers to the standard tape pose.
        if tape_chest is not None:
            anchors["chest"][1] = tape_chest
        waist_z = center_at_level(xyz, regions, anchors["waist"][1])
        for name in ("waist", "chest", "hip"):
            anchors[name][2] = center_at_level(xyz, regions, anchors[name][1])
        anchors["headTop"] = xyz[xyz[:,1].argmax()].astype(float)
        for name, eye in zip(("eyeLeft","eyeRight"), eyes):
            points = xyz[eye]
            anchors[name] = (points.max(0) + points.min(0)) / 2
        eye_y = anchors["eyeLeft"][1]
        front_head = xyz[(regions == 5) & (np.abs(xyz[:,0]) < 2.0) & (xyz[:,1] < eye_y-5)
                         & (xyz[:,2] > anchors["eyeLeft"][2]-2)]
        anchors["chin"] = front_head[front_head[:,1].argmin()]
        shift = [0,0,waist_z]
        marks.append({k:np.round(np.asarray(v)-shift,3).tolist() for k,v in anchors.items()})
        geometry.append(xyz[order]-shift)
    outcomes = {"height":fit["tapes"]["stature"]["body_mm"]/10, "bust":fit["tapes"]["chest"]["body_mm"]/10,
                "waist":fit["tapes"]["waist"]["body_mm"]/10, "weight":fit["mass_kg"]}
    return np.array(geometry), marks, outcomes, fit["tapes"]


def quantize(coords):
    values = np.rint(np.asarray(coords)*100)
    if not np.isfinite(values).all() or np.abs(values).max() >= 32768:
        raise ValueError("Coordinates outside the signed display range")
    return values.astype("<i2")


def save(folder, name, parts, info):
    with (folder / (name + ".bin.gz")).open("xb") as f:
        f.write(gzip.compress(b"".join(parts), compresslevel=9, mtime=0))
    with (folder / (name + ".json")).open("x") as f:
        json.dump(info, f, separators=(",",":"), allow_nan=False)


def skin_regions(model_path, wheel):
    """Transfer the pinned source OBJ's corner UVs to the native fitted skin mesh.

    Sample each corner before averaging a vertex. This keeps islands on opposite sides of
    a seam distinct while sampling; the fitted solver surface can retain its exact topology.
    """
    with model_path.open("rb") as f:
        prefix = f.read(24)
        if prefix[:8] != b"FTXBODY\0":
            raise ValueError("Expected ftxbody/1 model")
        length = struct.unpack_from("<Q", prefix, 16)[0]
        table = json.loads(f.read(length))["arrays"]["base_mesh_vertex_indices"]
        f.seek(24 + length + table["offset"])
        mapped = np.frombuffer(f.read(table["nbytes"]), "<u4")
    if hashlib.sha256(wheel.read_bytes()).hexdigest() != "9dbd3d6c2e5dae20a4f5e0b80e104f50fd13d2e15e42c90fc288e00d86e60e09":
        raise ValueError("Use the pinned Anny 0.6.1 wheel (tools/body-model/PIN.md)")
    channels = ["mpfb_face.jpg", "mpfb_lips.jpg", "mpfb_eyelids.jpg", "mpfb_ears.jpg",
                "mpfb_fingernails.jpg", "mpfb_toenails.jpg", "mpfb_inside-mouth.jpg", "sss.png"]
    with zipfile.ZipFile(wheel) as archive:
        prefix = "anny/data/mpfb2/"
        vt, corners = [], []
        group = ""
        for raw in archive.read(prefix+"3dobjs/base.obj").splitlines():
            line = raw.split()
            if not line:
                continue
            if line[0] == b"vt":
                vt.append((float(line[1]),float(line[2])))
            elif line[0] == b"g":
                group = line[1].decode()
            elif group == "body" and line[0] == b"f":
                for pair in line[1:]:
                    vertex, uv = pair.split(b"/")[:2]
                    corners.append((int(vertex)-1,int(uv)-1))
        vertex, uv_index = np.array(corners).T
        uv = np.array(vt)[uv_index]
        samples = []
        count = np.maximum(1,np.bincount(vertex,minlength=int(mapped.max())+1))
        for filename in channels:
            raw = archive.read(prefix+"textures/"+filename)
            pixels = subprocess.run(["magick","-","-alpha","off","-colorspace","gray","-resize","1024x1024!",
                                     "-depth","8","gray:-"],input=raw,capture_output=True,check=True).stdout
            atlas = np.frombuffer(pixels,np.uint8).reshape(1024,1024)
            x = np.rint(uv[:,0]*1023).astype(int)
            y = np.rint((1-uv[:,1])*1023).astype(int)
            s = np.bincount(vertex,weights=atlas[y,x],minlength=len(count))/count
            samples.append(np.rint(s[mapped]).astype(np.uint8))
    return np.stack(samples,axis=1)


def save_masks(out, name, arrays):
    with (out/(name+".surface.gz")).open("xb") as f:
        f.write(gzip.compress(b"".join(a.tobytes() for a in arrays),compresslevel=9,mtime=0))


def pack(source, out, masks=None):
    female = topology(source / "reference")
    order, indices, regions, skin_count, _ = female
    parts = [indices.astype("<u2").tobytes(), regions.tobytes(), bytes(len(regions) % 2)]
    meta = {"source":"FrankenTextiles ftx-body, Anny 0.6.1. Synthetic adult display samples.",
            "replay":"Independent shape-axis offsets and piecewise linear pose interpolation in centimetres.",
            "quantization_cm":.01, "vertices":len(order), "indices":indices.size, "skinIndices":skin_count,
            "angles":ANGLES, "base":BASE, "domains":DOMAINS, "samples":[]}
    base_coords = None
    for name, controls in cases():
        if name == "hussar":
            break
        geometry, marks, outcome, tapes = read_form(source/name, female)
        coords = quantize(geometry)
        if base_coords is None:
            base_coords = coords
            # Absolute first pose, then consecutive pose differences (unchanged torso compresses).
            encoded = np.concatenate([coords[:1], np.diff(coords, axis=0)])
        else:
            encoded = coords-base_coords
        parts.append(encoded.tobytes())
        meta["samples"].append({"name":name, "targets":controls, "outcomes":outcome, "landmarks":marks,
                                "native_tapes":tapes})
    save(out, "sundress", parts, meta)
    if masks is not None:
        save_masks(out, "sundress", [masks[order]])

    male = topology(source / "hussar")
    order, faces, regions, skin_count, eyes = male
    coords, poses, outcome, tapes = read_form(source / "hussar", male)
    xyz = coords[0]
    marks = poses[0]
    buffers, entries, skin_parts = [], {}, []
    # Each part is remapped locally. Head retains a short neck. Hands include the wrist under the cuff.
    for name, origin_key, eligible in [
        ("head","chin",xyz[:,1] >= marks["neck"][1]-1),
        ("leftHand","wristLeft",(regions == 3) | ((regions==1) & (xyz[:,1]<marks["wristLeft"][1]+3))),
        ("rightHand","wristRight",(regions == 4) | ((regions==2) & (xyz[:,1]<marks["wristRight"][1]+3))),
    ]:
        keep = eligible[faces].all(axis=1)
        triangles = faces[keep]
        used, remap = np.unique(triangles, return_inverse=True)
        local = xyz[used] - marks[origin_key]
        entries[name] = {"vertices":len(used), "indices":triangles.size,
                         "skinIndices":int(keep[:skin_count//3].sum()*3), "bounds":[local.min(0).tolist(),local.max(0).tolist()]}
        buffers.extend([remap.astype("<u2").tobytes(), quantize(local).tobytes()])
        if masks is not None:
            skin_parts.append(masks[order[used]])
    eyes_local = {k:(np.array(marks[k])-marks["chin"]).tolist() for k in ("eyeLeft","eyeRight")}
    eye_radius = float(np.ptp(xyz[-len(eyes[1]):,0]) / 2)
    save(out, "hussar", buffers, {"source":meta["source"], "parts":entries, "targets":dict(cases())["hussar"],
                                 "outcomes":outcome, "native_tapes":tapes, **eyes_local, "eyeRadius":eye_radius})
    if masks is not None:
        save_masks(out,"hussar",skin_parts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--model", type=Path, default=Path(os.getenv("FTX_ANNY_MODEL",
                        Path.home()/".cache/franken_textiles/anny-0.6.1-default.ftxbody")))
    parser.add_argument("--evaluator", type=Path, help="Previously compiled pose-display-form program")
    parser.add_argument("--skin-wheel", type=Path, default=os.getenv("FTX_ANNY_WHEEL"),
                        help="Pinned Anny 0.6.1 wheel for the MPFB material artwork (or set FTX_ANNY_WHEEL).")
    args = parser.parse_args()
    if not args.skin_wheel:
        parser.error("supply --skin-wheel PATH to the pinned Anny 0.6.1 wheel for the display's material masks")
    masks = skin_regions(args.model,args.skin_wheel)
    args.output.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryDirectory() as directory:
        working = Path(directory)
        evaluator = args.evaluator or compile_evaluator(working)
        jobs = [(k,c,evaluator,args.model,working) for k,c in cases()]
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            list(pool.map(native_sample, jobs))
        pack(working, args.output, masks)


if __name__ == "__main__":
    main()
