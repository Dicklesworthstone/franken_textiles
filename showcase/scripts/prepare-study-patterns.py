"""Rebuild the 1:1 A4 downloads from their published synthetic, millimetre pattern sources.

The bolero and trousers are the pattern files identified by the sim_report.json accompanying
each exhibit's exact sourcePc2Sha256. The fez is the Pattern Studio's WebAssembly chart draft
at 92 cm and height_frac=0.62. All three retain the native seam allowance and cut instructions.

Run from the repository with a built ftx CLI and MuPDF's mutool on PATH:
    python3 showcase/scripts/prepare-study-patterns.py NEW_DIRECTORY
Pass --bolero-replay or --trousers-replay to verify a recovered original simulation directory.
"""
import argparse
import hashlib
import json
import math
import subprocess
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PUBLIC = ROOT / "showcase/public"
SOURCE_DIGESTS = {
    "bolero": "e4080af83fd89eadca521bbea81576a375ca698c19fdb7668d9bf1a21a9508f3",
    "trousers": "2c8b9bfbc004c872d3932c9748ecb43ae5770eb4268359f59f605f752c58e10e",
    "fez": "95150d32d51c0deb1ef6987d41e4d5ceca51362ff7913d3b90acae34470c67f4",
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_replay(name, directory, pattern):
    gallery = json.loads((PUBLIC / "exhibits" / (name + ".json")).read_text())
    if digest(directory / "fold.pc2") != gallery["sourcePc2Sha256"]:
        raise ValueError(f"{name}: the supplied cache is a different performance")
    report = json.loads((directory / "sim_report.json").read_text())
    if digest(ROOT / report["pattern"]) != digest(pattern):
        raise ValueError(f"{name}: the source named by the simulation differs from the download")


def verify_paper(pdf):
    trace = subprocess.run(["mutool", "draw", "-q", "-F", "trace", "-o", "-", pdf, "1"],
                           capture_output=True, check=True).stdout
    page = ET.fromstring(trace).find("page")
    x0, y0, x1, y1 = map(float, page.attrib["mediabox"].split())
    mm = 25.4 / 72
    if abs((x1-x0)*mm-210) > .01 or abs((y1-y0)*mm-297) > .01:
        raise ValueError(f"{pdf.name}: cover media box is not A4")
    squares = set()
    for path in page.findall("stroke_path"):
        if [p.tag for p in path] != ["moveto","lineto","lineto","lineto","closepath"]:
            continue
        a,b,c,d,e,f = map(float, path.attrib["transform"].split())
        points = []
        for p in list(path)[:4]:
            x,y = float(p.attrib["x"]), float(p.attrib["y"])
            points.append(((a*x+c*y+e)*mm,(b*x+d*y+f)*mm))
        edges = [math.dist(points[i],points[(i+1)%4]) for i in range(4)]
        for expected in (100.0,101.6):
            if all(abs(edge-expected)<.01 for edge in edges):
                squares.add(expected)
    if squares != {100.0,101.6}:
        raise ValueError(f"{pdf.name}: printed calibration paths are missing or mis-scaled")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--ftx", type=Path, default=ROOT / "target/release/ftx")
    parser.add_argument("--bolero-replay", type=Path)
    parser.add_argument("--trousers-replay", type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True,exist_ok=False)
    for name, expected in SOURCE_DIGESTS.items():
        pattern = PUBLIC / "patterns" / (name + ".json")
        if digest(pattern) != expected:
            raise ValueError(f"{name}: pattern differs from the exhibit source")
        replay = getattr(args,name+"_replay",None)
        if replay:
            verify_replay(name,replay,pattern)
        with tempfile.TemporaryDirectory() as scratch:
            dest = Path(scratch)/"export"
            result = subprocess.run([args.ftx, "export", "a4", pattern, "--out", dest, "--created",
                                     "2026-11-19T00:00:00Z","--json"],capture_output=True,check=True,text=True)
            pdf, = map(Path,json.loads(result.stdout)["result"]["files"])
            subprocess.run([args.ftx,"check",dest,"--pattern-ref",pattern],check=True)
            verify_paper(pdf)
            with (args.output/(name+".pdf")).open("xb") as out:
                out.write(pdf.read_bytes())
            print(f"{name}: verified native export, A4 paper, 100 mm and 101.6 mm squares")


if __name__ == "__main__":
    main()
