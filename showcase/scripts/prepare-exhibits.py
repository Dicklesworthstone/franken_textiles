"""Package a synthetic ftx drape as a garment-only WebGL replay.

From the repo root, run
  python3 showcase/scripts/prepare-exhibits.py bolero SYNTHETIC_DRAPE_DIR OUTPUT_DIR
or substitute trousers. The source is a complete native drape with fold.pc2.
Only garment geometry, construction lines, endpoint strain colors, and aggregate
report values enter the exhibit. Never use a real wearer's workspace.
"""

import argparse
import hashlib
import json
import math
from pathlib import Path
import struct


def package(name, source, destination):
    mesh = json.loads((source / "mesh_map.json").read_text())
    report = json.loads((source / "sim_report.json").read_text())
    cache = (source / "fold.pc2").read_bytes()
    header = struct.Struct("<12siiffi")
    signature, version, vertices, first, rate, count = header.unpack_from(cache)
    frame_bytes = vertices * 12
    phases = mesh["frames"]
    if (
        (signature, version, first, rate) != (b"POINTCACHE2\0", 1, 1.0, 1.0)
        or vertices != mesh["vertices"]
        or mesh["units"] != "m"
        or len(cache) != header.size + count * frame_bytes
        or count != sum(phases.values())
    ):
        raise ValueError("Inconsistent PC2 point cache and garment mesh map")

    # Copy the original float32 samples without re-quantizing positions. Keep
    # the start and end of every phase even when off the regular sampling grid.
    indices = set(range(0, count, 3))
    start = 0
    for phase in ("fold", "zip", "sew", "drape"):
        end = start + phases[phase]
        if end > start:
            indices.update((start, end - 1))
        start = end
    frames = sorted(indices)
    points = memoryview(cache)[header.size:]
    positions = b"".join(points[i * frame_bytes : (i + 1) * frame_bytes] for i in frames)

    triangles = []
    rgb = []
    endpoint = bytearray()
    for line in (source / "garment_strain.obj").read_text().splitlines():
        fields = line.split()
        if fields[:1] == ["v"]:
            x, y, z, *channels = map(float, fields[1:])
            if len(channels) != 3 or any(not 0 <= c <= 1 for c in channels):
                raise ValueError("Invalid strain vertex colors")
            endpoint.extend(struct.pack("<fff", x, y, z))
            rgb.extend(round(c * 255) for c in channels)
        elif fields[:1] == ["f"]:
            face = [int(v) - 1 for v in fields[1:]]
            if len(face) != 3 or any(not 0 <= v < vertices for v in face):
                raise ValueError("Invalid garment triangle")
            triangles.extend(face)
    if endpoint != points[-frame_bytes:] or len(triangles) != report["faces"] * 3:
        raise ValueError("The strain mesh and the final replay frame disagree")
    if not all(math.isfinite(value) for (value,) in struct.iter_unpack("<f", positions)):
        raise ValueError("Nonfinite garment position")

    metadata = {
        "units": "m",
        "vertices": vertices,
        "triangles": triangles,
        "panels": mesh["inst_range"],
        "seams": mesh["seams"],
        "strainRgb": rgb,
        "frames": frames,
        "sourceFrames": count,
        "phases": phases,
        "sourcePc2Sha256": hashlib.sha256(cache).hexdigest(),
        "sourceStyle": report["style"],
        "summary": {
            "numericallySettled": report["numerical"]["settled"],
            "gating": report["gating"],
            "selfContactAudit": report["self_contact"]["audit"],
            "edgeStrainPercent": report["global_strain_pct"],
            "sewingPairs": report["sewing_pairs"],
        },
    }
    destination.mkdir(parents=True, exist_ok=True)
    outputs = {
        destination / f"{name}.json": (json.dumps(metadata, separators=(",", ":")) + "\n").encode(),
        destination / f"{name}.bin": positions,
    }
    for path, data in outputs.items():
        if path.exists() and path.read_bytes() != data:
            raise ValueError(f"{path} already contains a different replay. Use a new output directory.")
    for path, data in outputs.items():
        if not path.exists():
            with path.open("xb") as file:
                file.write(data)
    print(f"{name}: {vertices} garment vertices, {len(frames)}/{count} native frames, "
          f"{len(positions):,} bytes of positions")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("name", choices=("bolero", "trousers"))
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    package(args.name, args.source, args.destination)
