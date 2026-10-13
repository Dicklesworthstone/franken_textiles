#!/usr/bin/env python3
"""Supplement the pinned OpenCV 5 trace fixture with independent OpenCV 4.13 mask checks.

This does not change the skill pin or any existing oracle. Run with OpenCV 4.13.0:
    python tools/oracle/capture_adaptive.py OUT.json
Only synthetic integer-defined images are used; Rust candidate code is never called.
"""

import hashlib
import json
import sys
from pathlib import Path

import cv2
import numpy as np

if cv2.__version__ != "4.13.0":
    raise SystemExit("capture requires opencv-python 4.13.0; do not silently move the oracle")

cases = []
for width, height in [(1, 1), (1, 37), (43, 1), (3, 2), (47, 39), (360, 260), (343, 341), (623, 621)]:
    y, x = np.indices((height, width), dtype=np.int64)
    for kind in ["texture", "ramp", "impulse"]:
        if kind == "texture":
            pixels = (x * 79 + y * 37 + x * y * 11) % 256
        elif kind == "ramp":
            pixels = (x * 130 // max(1, width - 1) + y * 80 // max(1, height - 1))
        else:
            pixels = np.zeros((height, width), dtype=np.int64)
            pixels[0, 0] = 255
            pixels[height // 2, width // 2] = 255
        pixels = pixels.astype(np.uint8)
        block = max(15, (min(width, height) // 20) | 1)
        mask = cv2.adaptiveThreshold(pixels, 255, cv2.ADAPTIVE_THRESH_GAUSSIAN_C,
                                     cv2.THRESH_BINARY, block, -10)
        cases.append({"id": f"{kind}-{width}x{height}", "width": width, "height": height,
                      "kind": kind, "block": block,
                      "input_sha256": hashlib.sha256(pixels.tobytes()).hexdigest(),
                      "mask_sha256": hashlib.sha256(mask.tobytes()).hexdigest(),
                      "foreground_pixels": int(np.count_nonzero(mask))})

output = {"schema": "ftx-oracle/1", "subsystem": "trace", "fixture": "adaptive_opencv413",
          "meta": {"opencv": cv2.__version__, "optimized": cv2.useOptimized(),
                   "build_sha256": hashlib.sha256(cv2.getBuildInformation().encode()).hexdigest(),
                   "capture_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                   "boundary": "Supplemental pixel oracle, not a replacement for pinned OpenCV 5 whole-trace parity."},
          "cases": cases}
with Path(sys.argv[1]).open("x") as f:
    json.dump(output, f, indent=2)
    f.write("\n")
