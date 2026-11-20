"""Compare native drape or refusal stills beside the same physical panel's flat outlines."""

import argparse
import base64
import json
import re
import struct
import subprocess
from html import escape
from pathlib import Path
from xml.etree import ElementTree


def load_panel(directory, view, instance):
    entry = json.loads((directory / "round.json").read_bytes())
    number = entry["round"]
    sim = entry.get("sim") or {}
    refused = isinstance(sim.get("refusal"), dict)
    folder = "sim/refused" if refused else "sim"
    name = f"{folder}/still_{view}.png"
    rendering = entry.get("rendering") or {}
    if not refused and (not sim or "refusal" in sim or rendering.get("status") != "PASS"
                        or f"rounds/{number:03d}/{name}" not in entry["images"]):
        raise ValueError(f"round {number} has no published {view} still")
    path = directory / name
    if any(p.is_symlink() for p in (directory / "sim", directory / folder, path)):
        raise ValueError(f"round {number} has a linked still or scene directory")
    png = path.read_bytes()
    if not png.startswith(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"):
        raise ValueError(f"round {number} has an invalid native PNG")
    size = struct.unpack(">II", png[16:24])
    dimensions = None if refused else rendering
    if refused:
        mapping = directory / folder / "mesh_map.json"
        if mapping.is_symlink():
            raise ValueError(f"round {number} has a linked diagnostic mesh map")
        if mapping.exists():
            dimensions = json.loads(mapping.read_bytes())["refused_pose"]
            if (not isinstance(dimensions, dict) or not isinstance(dimensions.get("phase"), str)
                    or type(dimensions.get("accepted_steps")) is not int or dimensions["accepted_steps"] < 0):
                raise ValueError(f"round {number} has no identified pose in its diagnostic mesh map")
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
    return entry, base64.b64encode(png).decode(), size, [(x-x_mid, y-y_top) for x, y in pts]


def compare(before, after, view, instance, crop):
    rows = [load_panel(rd, view, instance) for rd in (before, after)]
    points = [p for _, _, _, outline in rows for p in outline]
    half_width, height = max(abs(x) for x, _ in points), max(y for _, y in points)
    if not half_width or not height:
        raise ValueError("flat outline has no drawable area")
    scale = min(114 / half_width, 278 / height)
    parts = ['<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
             'width="1200" height="410" viewBox="0 0 1200 410">',
             '<rect width="1200" height="410" fill="#f5f1e7"/>']
    def label(x, y, content, size=17, colour="#293934"):
        parts.append(f'<text x="{x}" y="{y}" font-family="DejaVu Sans,sans-serif" '
                     f'font-size="{size}" fill="{colour}">{escape(str(content))}</text>')
    outlines = []
    for i, (entry, png, (w, h), outline) in enumerate(rows):
        x = 15 + 446*i
        ink = ["#8a5327", "#126552"][i]
        refused = "refusal" in entry["sim"]
        title = "Refused pose" if refused else "Drape"
        label(x, 29, f'{title} · round {entry["round"]:03d} · {view.replace("_"," ")}', 19, ink)
        left, top, cw, ch = crop or (0, 0, w, h)
        if min(left, top) < 0 or min(cw, ch) <= 0 or left+cw > w or top+ch > h:
            raise ValueError(f'crop outside the {w} × {h} native image')
        # Fit the entire requested rectangle, including fingertips at wide aspect ratios.
        factor = min(436 / cw, 300 / ch)
        dw, dh = cw * factor, ch * factor
        parts.append(f'<svg x="{x + (436-dw)/2}" y="{42 + (300-dh)/2}" width="{dw}" height="{dh}" '
                     f'viewBox="{left} {top} {cw} {ch}" overflow="hidden"><image width="{w}" height="{h}" '
                     f'xlink:href="data:image/png;base64,{png}"/></svg>')
        label(x, 367, f'Flat · {entry["basis"]["flat"]}', 15)
        state = entry["sim"]["refusal"]["code"] if refused else entry["basis"]["sim"]
        label(x, 391, f'{title} · {state}', 15)
        coords = " ".join(f"{1053+x*scale:.2f},{56+y*scale:.2f}" for x,y in outline)
        dash = ' stroke-dasharray="6 4"' if i == 0 else ''
        outlines.append(f'<polygon points="{coords}" fill="none" stroke="{ink}" stroke-width="2.5"{dash}/>')
        label(930, 367+24*i, f'{"Dashed" if i == 0 else "Solid"} · round {entry["round"]:03d}', 15, ink)
    # Where the two cuts coincide, both colours should remain visible.
    parts.extend(reversed(outlines))
    label(930, 29, f'{instance} · flat overlay', 18)
    parts.append("</svg>")
    return "\n".join(parts)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--instance", required=True, help="Physical panel ID, e.g. sleeve.L")
    parser.add_argument("--view", default="front", choices=["front","side","back","three_quarter","strain_front","strain_back"])
    parser.add_argument("--crop", type=int, nargs=4, metavar=("LEFT","TOP","WIDTH","HEIGHT"),
                        help="Show the same region in both original PNGs, in pixels")
    parser.add_argument("--out", type=Path, required=True, help="New SVG, or PNG using the showcase's installed sharp package")
    args = parser.parse_args()
    try:
        svg = compare(args.before, args.after, args.view, args.instance, args.crop)
        data = svg.encode()
        if args.out.suffix.lower() == ".png":
            convert = ('import sharp from "sharp"; const chunks=[]; for await (const c of process.stdin) chunks.push(c); '
                       'const png=await sharp(Buffer.concat(chunks),{density:120}).png().toBuffer(); process.stdout.write(png);')
            data = subprocess.run(["node", "--input-type=module", "-e", convert], input=data,
                                  stdout=subprocess.PIPE, check=True, cwd=Path(__file__).resolve().parents[4]/"showcase").stdout
        elif args.out.suffix.lower() != ".svg":
            raise ValueError("choose an output filename ending in .svg or .png")
        with args.out.open("xb") as file:
            file.write(data)
    except (KeyError, OSError, ValueError, ElementTree.ParseError, struct.error, subprocess.CalledProcessError) as error:
        parser.exit(1, f"compare_rounds: {error}\n")
