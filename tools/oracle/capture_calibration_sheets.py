#!/usr/bin/env python3
"""Supplement spec 02 A.9 with pinned OpenCV dictionary/board pixel evidence.

Run with opencv-python 4.13.0.92 and numpy 2.2.6. This calls only the upstream
provider, never FrankenTextiles. It leaves the Python skill pin and old oracles
unchanged. The output is create-only; typography/PNG/PDF bytes are not an oracle.
"""
import hashlib
import json
import sys
from pathlib import Path

import cv2
import numpy as np

if cv2.__version__ != "4.13.0" or np.__version__ != "2.2.6":
    raise SystemExit("requires OpenCV 4.13.0 and NumPy 2.2.6; do not move pins silently")

sha = lambda b: hashlib.sha256(b).hexdigest()
names = sorted(n for n in dir(cv2.aruco)
               if n.startswith("DICT_") and isinstance(getattr(cv2.aruco, n), int))
tables, cases = [], []
for name in names:
    dictionary = cv2.aruco.getPredefinedDictionary(getattr(cv2.aruco, name))
    size = dictionary.markerSize
    codes = []
    for marker_id in range(50 if name.startswith("DICT_4X4_") else 28):
        # Read canonical row-major data bits from an actual rendered marker,
        # independent of OpenCV's rotation-interleaved bytesList representation.
        bits = cv2.aruco.generateImageMarker(dictionary, marker_id, size + 2)[1:-1, 1:-1]
        code = 0
        for bit in bits.flat:
            code = (code << 1) | int(bit == 255)
        codes.append(f"{code:x}")
    tables.append({"name": name, "marker_bits": size, "codes_hex": codes})
    for paper, rows, square, marker in [("letter", 6, 22, 16), ("a4", 7, 22, 16),
                                        ("letter", 6, 23, 15), ("a4", 7, 18, 11)]:
        board = cv2.aruco.CharucoBoard((8, rows), square, marker, dictionary)
        board.setLegacyPattern(False)
        width, height = 8 * square * 12, rows * square * 12
        pixels = board.generateImage((width, height), marginSize=0, borderBits=1)
        cases.append({"paper": paper, "dictionary": name, "square_mm": square,
                      "marker_mm": marker, "width": width, "height": height,
                      "board_pixels_sha256": sha(pixels.tobytes())})

default = next(t for t in tables if t["name"] == "DICT_4X4_50")
assert sha(b"".join(int(c, 16).to_bytes(2, "big") for c in default["codes_hex"])) == \
    "372c6d504da619d970a5153562953a92c5a082d51cd48b5ca9ffe87bb78357b7"
output = {"schema": "ftx-oracle/1", "fixture": "calibration_sheets_opencv413",
          "meta": {"opencv": cv2.__version__, "numpy": np.__version__,
                   "opencv_build_sha256": sha(cv2.getBuildInformation().encode()),
                   "capture_sha256": sha(Path(__file__).read_bytes()),
                   "boundary": "Upstream predefined dictionary and 12-px/mm board oracle; not physical print/camera/fit qualification or whole-skill byte parity."},
          "dictionaries": tables, "cases": cases}
with Path(sys.argv[1]).open("x") as f:
    json.dump(output, f, indent=2)
    f.write("\n")
print(f"Captured {len(tables)} names and {len(cases)} board cases from OpenCV only.")
