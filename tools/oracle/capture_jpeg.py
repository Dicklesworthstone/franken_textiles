#!/usr/bin/env python3
"""Capture the reference's load_image and trace on JPEG files as oracle fixtures (docs/spec/04 §10.2).

Writes OUT_DIR/raster_jpeg.json: JPEG files and the BGR array vectorize.py's load_image gets from them (Pillow
12.3 with its bundled libjpeg-turbo), or its refusal; and OUT_DIR/trace_jpeg.json: the whole trace_image on
JPEG pattern sheets (Pillow and OpenCV encodings, a photographed sheet), as raster_trace.json records it for
PNGs. The load_image files cover
- both encoders the reference ships: Pillow, and OpenCV with every sampling factor (4:1:1, 4:2:0, 4:2:2, 4:4:0,
  4:4:4) and restart intervals;
- awkward sizes, baseline and progressive, gray, Adobe RGB (`keep_rgb`) and RGB-by-component-ID files;
- 16-bit and unit quantization tables, extreme content, the standard Huffman tables left out (libjpeg supplies
  them), extra APPn/COM segments and fill bytes;
- damaged streams: truncations, a missing EOI, trailing junk, a marker inside the entropy data, renumbered and
  missing restart markers, and random corruption.

Pillow's SIMD IDCT (Neon on arm64, AVX2 on x86-64) wraps dequantized coefficients beyond 16 bits, where its C
code does not, so a damaged file can decode differently from machine to machine. A damaged case is therefore
kept only when this machine's SIMD result equals the C result (JSIMD_FORCENONE=1 in a second process). Images
over 1,024 pixels record the SHA-256 of the BGR bytes instead of the array. The reference is identified by the
SHA-256 of its files, not by a path. Usage:
    python capture_jpeg.py --reference SKILL/scripts OUT_DIR
Needs the reference's requirements (numpy, opencv-python-headless, pillow). Deterministic.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
import platform
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

SCHEMA = "ftx-oracle/1"
FILES = ("vectorize.py",)
C_PATH = (
    "import sys, hashlib, numpy as np\n"
    "from PIL import Image\n"
    "try:\n"
    "    a = np.ascontiguousarray(np.asarray(Image.open(sys.argv[1]).convert('RGB'))[:, :, ::-1])\n"
    "    print(hashlib.sha256(a.tobytes()).hexdigest())\n"
    "except Exception as e:\n"
    "    print('error', type(e).__name__)\n"
)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--reference", required=True, type=Path)
    ap.add_argument("out_dir", type=Path)
    args = ap.parse_args()
    ref = args.reference.resolve()
    sys.path.insert(0, str(ref))
    import cv2
    import PIL
    import vectorize as V
    from PIL import Image, features

    meta = {"reference_files": {f: hashlib.sha256((ref / f).read_bytes()).hexdigest() for f in FILES},
            "numpy_version": np.__version__, "opencv_version": cv2.__version__, "pillow_version": PIL.__version__,
            "libjpeg_turbo_version": features.version("libjpeg_turbo"),
            "python_version": platform.python_version(), "machine": platform.machine()}
    rng = np.random.default_rng(23)
    cases = []
    skipped = []

    def load(data: bytes, damaged: bool):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "x.jpg"
            p.write_bytes(data)
            try:
                img, _ = V.load_image(str(p))
            except Exception as exc:                    # noqa: BLE001 - the refusal is the answer
                return {"error": type(exc).__name__, "message": str(exc).replace(str(p), "x.jpg")}
            sha = hashlib.sha256(img.tobytes()).hexdigest()
            if damaged:
                c = subprocess.run([sys.executable, "-I", "-c", C_PATH, str(p)], capture_output=True, text=True,
                                   env={**os.environ, "JSIMD_FORCENONE": "1"}, check=True).stdout.strip()
                if c != sha:
                    return None                         # SIMD and C differ on this damaged file
            out = {"shape": list(img.shape)}
            if img.shape[0] * img.shape[1] <= 1024:
                out["bgr"] = img.reshape(-1).tolist()
            else:
                out["sha256"] = sha
            return out

    def add(cid: str, data: bytes, damaged: bool = False):
        out = load(data, damaged)
        if out is None:
            skipped.append(cid)
            return
        cases.append({"id": cid, "input": {"jpeg": base64.b64encode(data).decode()}, "output": out})

    def pil(a, **kw) -> bytes:
        b = io.BytesIO()
        Image.fromarray(a).save(b, "JPEG", **kw)
        return b.getvalue()

    def ocv(a, params) -> bytes:
        ok, buf = cv2.imencode(".jpg", a[..., ::-1] if a.ndim == 3 else a, params)
        assert ok
        return buf.tobytes()

    def photo(h, w):
        y, x = np.mgrid[0:h, 0:w] / max(h, w, 1)
        a = np.stack([128 + 100 * np.sin(7 * x + 3 * y), 128 + 90 * np.cos(5 * y - 2 * x * x),
                      128 + 80 * np.sin(11 * x * y + 1)], -1)
        return np.clip(a + rng.normal(0, 6, a.shape), 0, 255).astype(np.uint8)

    def segments(data: bytes):
        """(marker, start, end) of each header segment before the first SOS."""
        out, i = [], 2
        while data[i + 1] != 0xDA:
            n = int.from_bytes(data[i + 2:i + 4], "big")
            out.append((data[i + 1], i, i + 2 + n))
            i += 2 + n
        return out

    def renumber(data: bytes, ids: bytes) -> bytes:
        """The file with its components renamed to `ids`, in the frame header and in every scan header."""
        d, i = bytearray(data), 2
        while i < len(d) - 1:
            m = d[i + 1]
            if m == 0xD9:
                break
            n = int.from_bytes(d[i + 2:i + 4], "big")
            if m in (0xC0, 0xC2):
                for k in range(d[i + 9]):
                    d[i + 10 + 3 * k] = ids[d[i + 10 + 3 * k] - 1]
            if m == 0xDA:
                for k in range(d[i + 4]):
                    d[i + 5 + 2 * k] = ids[d[i + 5 + 2 * k] - 1]
                i += 2 + n
                while not (d[i] == 0xFF and d[i + 1] not in (0x00, *range(0xD0, 0xD8))):
                    i += 1
                continue
            i += 2 + n
        return bytes(d)

    # Pillow: sizes x chroma subsampling x progressive, and gray
    for h, w in ((1, 1), (2, 2), (1, 9), (9, 1), (7, 9), (8, 8), (17, 33), (33, 17), (64, 3), (3, 64)):
        a = photo(h, w)
        for sub in (0, 1, 2):
            for prog in (False, True):
                add(f"pil-{h}x{w}-s{sub}-p{int(prog)}", pil(a, quality=85, subsampling=sub, progressive=prog))
        add(f"pil-{h}x{w}-gray", pil(a[..., 0].copy(), quality=85))
        add(f"pil-{h}x{w}-gray-p1", pil(a[..., 0].copy(), quality=85, progressive=True))
    big = photo(203, 297)
    add("pil-photo-420", pil(big, quality=80, subsampling=2))
    add("pil-photo-420-p1", pil(big, quality=80, subsampling=2, progressive=True))
    add("pil-photo-optimize", pil(big, quality=70, optimize=True))
    add("pil-restart-blocks", pil(big, quality=80, restart_marker_blocks=5))
    add("pil-restart-rows-p1", pil(big, quality=80, restart_marker_rows=2, progressive=True))
    add("pil-adobe-rgb", pil(big, quality=90, keep_rgb=True, subsampling=0))
    add("pil-adobe-rgb-p1", pil(big, quality=90, keep_rgb=True, subsampling=0, progressive=True))
    add("pil-qtables-16bit", pil(big, qtables=[[300 + i for i in range(64)], [2000 + 7 * i for i in range(64)]]))
    add("pil-qtables-unit", pil(big, qtables=[[1] * 64, [1] * 64], subsampling=0))
    add("pil-abbreviated", pil(big, streamtype=2))
    y, x = np.mgrid[0:64, 0:72]
    for s in (1, 3):
        c = ((x // s + y // s) % 2 * 255).astype(np.uint8)
        a = np.stack([c, 255 - c, c], -1)
        add(f"checker{s}-q100", pil(a, quality=100, subsampling=0))
        add(f"checker{s}-qunit-420", pil(a, qtables=[[1] * 64, [1] * 64], subsampling=2))
    for name, col in (("red", (255, 0, 0)), ("blue", (0, 0, 255)), ("black", (0, 0, 0)), ("white", (255, 255, 255))):
        a = np.zeros((24, 24, 3), np.uint8)
        a[:] = col
        a[6:18, 6:18] = 255 - np.array(col)
        add(f"flat-{name}", pil(a, quality=100, subsampling=2))

    # OpenCV: every sampling factor, with and without restarts and progression
    factors = (("411", cv2.IMWRITE_JPEG_SAMPLING_FACTOR_411), ("420", cv2.IMWRITE_JPEG_SAMPLING_FACTOR_420),
               ("422", cv2.IMWRITE_JPEG_SAMPLING_FACTOR_422), ("440", cv2.IMWRITE_JPEG_SAMPLING_FACTOR_440),
               ("444", cv2.IMWRITE_JPEG_SAMPLING_FACTOR_444))
    for tag, sf in factors:
        for rst, prog in ((0, 0), (1, 0), (3, 1)):
            for h, w in ((61, 83), (17, 2), (2, 17), (9, 5)):
                add(f"cv-{tag}-r{rst}-p{prog}-{h}x{w}",
                    ocv(big[:h, :w], [cv2.IMWRITE_JPEG_SAMPLING_FACTOR, sf, cv2.IMWRITE_JPEG_RST_INTERVAL, rst,
                                      cv2.IMWRITE_JPEG_PROGRESSIVE, prog, cv2.IMWRITE_JPEG_QUALITY, 90]))
    for q in (1, 50, 100):
        add(f"cv-q{q}-optimize", ocv(big[:61, :83], [cv2.IMWRITE_JPEG_QUALITY, q, cv2.IMWRITE_JPEG_OPTIMIZE, 1]))
    add("cv-gray-r2", ocv(big[:61, :83, 1].copy(), [cv2.IMWRITE_JPEG_RST_INTERVAL, 2]))

    # marker-level variants of one baseline and one progressive file
    small = photo(29, 37)
    base = pil(small, quality=80, subsampling=2)
    prog = pil(small, quality=80, subsampling=2, progressive=True)
    for name, data in (("base", base), ("prog", prog)):
        seg = segments(data)
        app0 = next((s, e) for m, s, e in seg if m == 0xE0)
        add(f"{name}-no-jfif", data[:app0[0]] + data[app0[1]:])
        add(f"{name}-ids-rgb", renumber(data[:app0[0]] + data[app0[1]:], b"RGB"))
        add(f"{name}-ids-other", renumber(data[:app0[0]] + data[app0[1]:], b"\x07\x08\x09"))
        adobe = b"\xff\xee\x00\x0eAdobe\x00\x64\x00\x00\x00\x00\x00"
        add(f"{name}-adobe-transform0", data[:2] + adobe + data[2:])
        add(f"{name}-extra-segments", data[:2] + b"\xff\xe1\x00\x08Exif\x00\x00" + b"\xff\xfe\x00\x05abc"
            + b"\xff\xe2\x00\x04\x00\x00" + data[2:])
        add(f"{name}-fill-bytes", data[:app0[1]] + b"\xff\xff\xff" + data[app0[1]:])
        add(f"{name}-junk-before-marker", data[:app0[1]] + b"\x12\x34" + data[app0[1]:])
        add(f"{name}-trailing-junk", data + b"garbage\xff\x00\x01")
        add(f"{name}-no-eoi", data[:-2], True)
        sos = data.find(b"\xff\xda")
        add(f"{name}-eoi-in-scan", data[:(sos + len(data)) // 2] + b"\xff\xd9" + data[(sos + len(data)) // 2:], True)
        for k, cut in enumerate((0.1, 0.5, 0.9)):
            add(f"{name}-truncated-{k}", data[:int(len(data) * cut)], True)
        add(f"{name}-tem-before-sos", data[:2] + b"\xff\x01" + data[2:])
    seg = segments(base)
    dht = [(s, e) for m, s, e in seg if m == 0xC4]
    stripped = base
    for s, e in reversed(dht):
        stripped = stripped[:s] + stripped[e:]
    add("base-standard-huffman-omitted", stripped)
    for name, data in (("pil", pil(big[:61, :83], quality=80, restart_marker_blocks=4)),
                       ("cv", ocv(big[:61, :83], [cv2.IMWRITE_JPEG_RST_INTERVAL, 2]))):
        sos = data.find(b"\xff\xda")
        idx = [i for i in range(sos, len(data) - 1) if data[i] == 0xFF and 0xD0 <= data[i + 1] <= 0xD7]
        d = bytearray(data)
        for j in idx[::3]:
            d[j + 1] = 0xD0 + (d[j + 1] - 0xD0 + 1) % 8
        add(f"{name}-restart-renumbered", bytes(d), True)
        d = bytearray(data)
        for j in reversed(idx[1::4]):
            del d[j:j + 2]
        add(f"{name}-restart-missing", bytes(d), True)

    # random corruption inside the entropy data
    for name, data in (("base", pil(big[:61, :83], quality=80)), ("prog", pil(big[:61, :83], quality=80, progressive=True)),
                       ("gray", pil(big[:61, :83, 0].copy(), quality=80, progressive=True)),
                       ("rst", ocv(big[:61, :83], [cv2.IMWRITE_JPEG_RST_INTERVAL, 1]))):
        sos = data.find(b"\xff\xda")
        for t in range(6):
            d = bytearray(data)
            for _ in range(1 + t % 3):
                d[int(rng.integers(sos + 20, len(d) - 2))] = int(rng.integers(0, 256))
            add(f"{name}-corrupt-{t}", bytes(d), True)

    meta["skipped_simd_differs"] = skipped
    args.out_dir.mkdir(parents=True, exist_ok=True)
    path = args.out_dir / "raster_jpeg.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "trace", "fixture": "raster_jpeg", "meta": meta,
                                "cases": cases}, separators=(",", ":")) + "\n")
    print(f"wrote {len(cases)} cases to {path} ({len(skipped)} damaged cases skipped: SIMD and C differ)")

    # ------------------------------------------------ the whole trace on JPEG sheets (as raster_trace.json)
    tcases = []
    W, H, AA, K = 360, 260, cv2.LINE_AA, (20, 20, 20)
    pent = np.array([(30, 40), (150, 30), (170, 120), (110, 200), (40, 180)], np.int32)

    def sheet(bg=(255, 255, 255), fg=K, light=0.0, noise=0.0):
        img = np.full((H, W, 3), bg, np.uint8)
        cv2.polylines(img, [pent], True, fg, 3, AA)
        cv2.arrowedLine(img, (90, 115), (90, 70), fg, 2, AA, tipLength=0.2)
        cv2.arrowedLine(img, (90, 115), (90, 165), fg, 2, AA, tipLength=0.2)
        cv2.ellipse(img, (262, 125), (68, 92), 0, 0, 360, fg, 3, AA)
        if light or noise:                              # a photographed sheet: uneven light, sensor noise
            ramp = 1.0 - light * np.linspace(0, 1, W)[None, :, None]
            img = np.clip(img * ramp + rng.normal(0, noise, img.shape), 0, 255).astype(np.uint8)
        return img

    def trace_case(cid, data: bytes, **kw):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "sheet.jpg"
            p.write_bytes(data)
            try:
                r = V.trace_image(str(p), None, **kw)
                r["pattern"]["meta"]["source"]["image"] = "sheet.jpg"   # the temp path is not part of the answer
                _, mask = cv2.imencode(".png", r["mask"])
                out = {"pattern": r["pattern"], "diagnostics": r["diagnostics"],
                       "mask_png": base64.b64encode(mask.tobytes()).decode()}
            except Exception as exc:                    # noqa: BLE001 - the refusal is the answer
                out = {"error": type(exc).__name__, "message": str(exc).replace(str(p), "sheet.jpg")}
        tcases.append({"id": cid, "input": {"jpeg": base64.b64encode(data).decode(), "opts": kw}, "output": out})

    trace_case("pil-420", pil(sheet()[..., ::-1].copy(), quality=92, subsampling=2))
    trace_case("pil-444-progressive", pil(sheet()[..., ::-1].copy(), quality=85, subsampling=0, progressive=True))
    trace_case("pil-gray", pil(cv2.cvtColor(sheet(), cv2.COLOR_BGR2GRAY), quality=90))
    trace_case("cv-422-restart", ocv(sheet()[..., ::-1].copy(), [cv2.IMWRITE_JPEG_SAMPLING_FACTOR,
                                    cv2.IMWRITE_JPEG_SAMPLING_FACTOR_422, cv2.IMWRITE_JPEG_RST_INTERVAL, 2]))
    trace_case("photo-q75", pil(sheet(light=0.25, noise=4.0)[..., ::-1].copy(), quality=75, subsampling=2))
    trace_case("navy-q80", pil(sheet(bg=(80, 35, 25), fg=(245, 245, 245))[..., ::-1].copy(), quality=80))
    trace_case("photo-adaptive", pil(sheet(light=0.4, noise=3.0)[..., ::-1].copy(), quality=85),
               threshold="adaptive")
    path = args.out_dir / "trace_jpeg.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "trace", "fixture": "trace_jpeg", "meta": meta,
                                "cases": tcases}, separators=(",", ":")) + "\n")
    print(f"wrote {len(tcases)} cases to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
