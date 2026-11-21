"""Compare native drape or refusal stills beside the same physical panel's flat outlines."""

import argparse
import base64
import json
import re
import struct
import subprocess
from html import escape
from pathlib import Path, PureWindowsPath
from typing import NamedTuple
from xml.etree import ElementTree


class Panel(NamedTuple):
    entry: dict
    title: str
    scene: Path
    png: bytes
    size: tuple[int, int]
    outline: list[tuple[float, float]]


def load_panel(directory, view, instance):
    entry = json.loads((directory / "round.json").read_bytes())
    number = entry["round"]
    sim = entry.get("sim") or {}
    refused = isinstance(sim.get("refusal"), dict)
    scene = directory / "sim"
    if scene.is_symlink():
        raise ValueError(f"round {number} has a linked scene directory")
    title = "Drape"
    rendering = entry.get("rendering") or {}
    dimensions = None if refused else rendering
    filename = f"still_{view}.png"
    if refused:
        scene = scene / "refused"
        if scene.is_symlink():
            raise ValueError(f"round {number} has a linked refusal directory")
        title = "Refused pose"
        placement = scene / "placement.json"
        if sim["refusal"].get("code") == "intersecting_start":
            if placement.is_symlink():
                raise ValueError(f"round {number} has a linked placement report")
            if placement.exists():
                # The native report can contain paths from before the workspace was moved.
                # Check its stage and filename, then read only from this round.
                report = json.loads(placement.read_bytes())
                stills = report.get("stills") if isinstance(report, dict) else None
                listed = stills.get("start") if isinstance(stills, dict) else None
                if not isinstance(listed, list) or not any(
                    isinstance(p, str) and PureWindowsPath(p).parts[-2:] == ("start", filename)
                    for p in listed
                ):
                    raise ValueError(f"round {number} did not publish the requested start view")
                scene = scene / "start"
                title = "Refused start"
    path = scene / filename
    name = path.relative_to(directory).as_posix()
    if not refused and (not sim or "refusal" in sim or rendering.get("status") != "PASS"
                        or f"rounds/{number:03d}/{name}" not in entry["images"]):
        raise ValueError(f"round {number} has no published {view} still")
    if scene.is_symlink() or path.is_symlink():
        raise ValueError(f"round {number} has a linked still or scene directory")
    png = path.read_bytes()
    if not png.startswith(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"):
        raise ValueError(f"round {number} has an invalid native PNG")
    if not png.endswith(b"\0\0\0\0IEND\xae\x42\x60\x82"):
        raise ValueError(f"round {number} has an incomplete native PNG")
    size = struct.unpack(">II", png[16:24])
    if refused:
        mapping = scene / "mesh_map.json"
        if mapping.is_symlink():
            raise ValueError(f"round {number} has a linked diagnostic mesh map")
        if mapping.exists():
            dimensions = json.loads(mapping.read_bytes())["refused_pose"]
            if (not isinstance(dimensions, dict) or not isinstance(dimensions.get("phase"), str)
                    or type(dimensions.get("accepted_steps")) is not int or dimensions["accepted_steps"] < 0):
                raise ValueError(f"round {number} has no identified pose in its diagnostic mesh map")
            placement_view = title == "Refused start"
            if ((dimensions["phase"] == "placement") != placement_view
                    or (placement_view and dimensions["accepted_steps"] != 0)):
                raise ValueError(f"round {number} has a mesh map from a different stage")
    if not all(size) or (dimensions is not None and size != (dimensions["width"], dimensions["height"])):
        raise ValueError(f"still differs from the dimensions recorded in round {number}")
    preview = ElementTree.parse(directory / "preview.svg")
    outline = next((p.get("d") for p in preview.iter("{http://www.w3.org/2000/svg}path")
                    if p.get("data-instance") == instance), None)
    if outline is None:
        raise ValueError(f"round {number} has no flat outline for {instance}")
    # diagnostic_svg emits M/L polylines at a fixed scale and arranges them on shelves.
    # Remove just the shelf offsets: top-centre alignment keeps length changes visible.
    pts = [(float(x), float(y)) for x, y in re.findall(r"[ML](-?[\d.]+) (-?[\d.]+)", outline)]
    if len(pts) < 3:
        raise ValueError(f"unrecognised flat outline for {instance}")
    x_mid = (min(x for x, _ in pts) + max(x for x, _ in pts)) / 2
    y_top = min(y for _, y in pts)
    return Panel(entry, title, scene, png, size, [(x-x_mid, y-y_top) for x, y in pts])


def common_view(rows, view, out, ftx):
    width, height = rows[0].size
    command = [
        ftx, "render", str(rows[0].scene), "--compare", str(rows[1].scene),
        "--view", view, "--res", f"{width}x{height}", "--out", str(out), "--json",
    ]
    result = json.loads(subprocess.run(command, stdout=subprocess.PIPE, check=True).stdout)["result"]
    aligned = [row._replace(png=(out / name).read_bytes(), size=(width, height))
               for row, name in zip(rows, ("before.png", "after.png"), strict=True)]
    body = "Same posed body" if result["same_body_geometry"] else "Posed bodies differ"
    return aligned, f"Shared baseline camera · {body}"


def contact_status(row, ftx):
    if row.title != "Drape":
        return "Diagnostic state"
    command = [ftx, "viewer", str(row.scene), "--check-contact", "--json"]
    process = subprocess.run(command, stdout=subprocess.PIPE, check=False)
    try:
        reply = json.loads(process.stdout)
    except ValueError:
        return "Full-body contact check unavailable"
    if process.returncode == 0 and reply.get("ok") is True:
        count = reply["result"]["checked_frames"]
        if type(count) is int and count > 0:
            return f"Full-body contact clear · {count} recorded frames"
    if (reply.get("refusal") or {}).get("code") == "intersecting_trajectory":
        return "Full-body contact · crossing detected"
    return "Full-body contact check unavailable"


def compare(before, after, view, instance, crop, views_dir=None, ftx="ftx"):
    rows = [load_panel(rd, view, instance) for rd in (before, after)]
    sizes = [rows[0].size] if views_dir is not None else [row.size for row in rows]
    if crop:
        left, top, cw, ch = crop
        for w, h in sizes:
            if min(left, top) < 0 or min(cw, ch) <= 0 or left+cw > w or top+ch > h:
                raise ValueError(f'crop outside the {w} × {h} image')
    points = [p for row in rows for p in row.outline]
    half_width, height = max(abs(x) for x, _ in points), max(y for _, y in points)
    if not half_width or not height:
        raise ValueError("flat outline has no drawable area")
    scale = min(114 / half_width, 278 / height)
    framing = None
    if views_dir is not None:
        rows, framing = common_view(rows, view, views_dir, ftx)
    canvas_height = 466 if framing else 410
    parts = ['<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
             f'width="1200" height="{canvas_height}" viewBox="0 0 1200 {canvas_height}">',
             f'<rect width="1200" height="{canvas_height}" fill="#f5f1e7"/>']
    def label(x, y, content, size=17, colour="#293934"):
        parts.append(f'<text x="{x}" y="{y}" font-family="DejaVu Sans,sans-serif" '
                     f'font-size="{size}" fill="{colour}">{escape(str(content))}</text>')
    outlines = []
    for i, (entry, title, _, png, (w, h), outline) in enumerate(rows):
        x = 15 + 446*i
        ink = ["#8a5327", "#126552"][i]
        refused = "refusal" in entry["sim"]
        label(x, 29, f'{title} · round {entry["round"]:03d} · {view.replace("_"," ")}', 19, ink)
        left, top, cw, ch = crop or (0, 0, w, h)
        # Fit the entire requested rectangle, including fingertips at wide aspect ratios.
        factor = min(436 / cw, 300 / ch)
        dw, dh = cw * factor, ch * factor
        parts.append(f'<svg x="{x + (436-dw)/2}" y="{42 + (300-dh)/2}" width="{dw}" height="{dh}" '
                     f'viewBox="{left} {top} {cw} {ch}" overflow="hidden"><image width="{w}" height="{h}" '
                     f'xlink:href="data:image/png;base64,{base64.b64encode(png).decode()}"/></svg>')
        label(x, 367, f'Flat · {entry["basis"]["flat"]}', 15)
        state = entry["sim"]["refusal"]["code"] if refused else entry["basis"]["sim"]
        report_title = "Saved solver report" if framing and not refused else title
        label(x, 391, f'{report_title} · {state}', 15)
        if framing:
            contact = contact_status(rows[i], ftx)
            label(x, 415, contact, 15, "#9a3229" if "crossing" in contact else "#293934")
        coords = " ".join(f"{1053+x*scale:.2f},{56+y*scale:.2f}" for x,y in outline)
        dash = ' stroke-dasharray="6 4"' if i == 0 else ''
        outlines.append(f'<polygon points="{coords}" fill="none" stroke="{ink}" stroke-width="2.5"{dash}/>')
        label(930, 367+24*i, f'{"Dashed" if i == 0 else "Solid"} · round {entry["round"]:03d}', 15, ink)
    # Where the two cuts coincide, both colours should remain visible.
    parts.extend(reversed(outlines))
    label(930, 29, f'{instance} · flat overlay', 18)
    if framing:
        label(15, 444, framing, 15)
    parts.append("</svg>")
    return "\n".join(parts)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--instance", required=True, help="Physical panel ID, e.g. sleeve.L")
    parser.add_argument("--view", default="front", choices=["front","side","back","three_quarter","strain_front","strain_back"])
    parser.add_argument("--crop", type=int, nargs=4, metavar=("LEFT","TOP","WIDTH","HEIGHT"),
                        help="Crop both images in pixels (baseline image coordinates with --shared-camera)")
    parser.add_argument("--out", type=Path, required=True, help="New SVG, or PNG using the showcase's installed sharp package")
    parser.add_argument("--shared-camera", action="store_true",
                        help="Render both scenes through the baseline camera with ftx")
    parser.add_argument("--ftx", default="ftx", help="Native ftx executable for --shared-camera")
    args = parser.parse_args()
    try:
        suffix = args.out.suffix.lower()
        if suffix not in (".svg", ".png"):
            raise ValueError("choose an output filename ending in .svg or .png")
        if args.out.exists() or args.out.is_symlink():
            raise FileExistsError(f"{args.out} already exists")
        views_dir = args.out.with_name(args.out.name + ".views") if args.shared_camera else None
        svg = compare(args.before, args.after, args.view, args.instance, args.crop, views_dir, args.ftx)
        data = svg.encode()
        if suffix == ".png":
            convert = ('import sharp from "sharp"; const chunks=[]; for await (const c of process.stdin) chunks.push(c); '
                       'const png=await sharp(Buffer.concat(chunks),{density:120}).png().toBuffer(); process.stdout.write(png);')
            data = subprocess.run(["node", "--input-type=module", "-e", convert], input=data,
                                  stdout=subprocess.PIPE, check=True, cwd=Path(__file__).resolve().parents[4]/"showcase").stdout
        with args.out.open("xb") as file:
            file.write(data)
    except (KeyError, OSError, ValueError, ElementTree.ParseError, struct.error, subprocess.CalledProcessError) as error:
        parser.exit(1, f"compare_rounds: {error}\n")
