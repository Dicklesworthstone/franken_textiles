#!/usr/bin/env python3
"""Capture the raster operations the reference's trace calls (docs/spec/04 §10.2) as oracle fixtures.

Writes, from the reference's vectorize.py and the libraries it runs on (Pillow, OpenCV, numpy):
    OUT_DIR/raster_image.json   load_image on PNGs of every bit depth and colour type the format allows (all five
                                scanline filters, Adam7 interlacing at awkward sizes, palette, gray and RGB
                                transparency, partial alpha): the file and the BGR array the reference gets
    OUT_DIR/raster_color.json   cvtColor BGR->Lab over every 8-bit colour (SHA-256), Lab, gray and HSV
                                saturation on a 16,384-pixel sample, and the trace's `auto` channel for four
                                backgrounds

The PNGs are written here byte by byte (a small encoder below), so every variant is covered whatever Pillow
would choose to write. The reference is identified by the SHA-256 of its files, not by a path. Usage:
    python capture_raster.py --reference SKILL/scripts OUT_DIR
Needs the reference's requirements (numpy, opencv-python-headless, pillow). Deterministic.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import platform
import struct
import sys
import tempfile
import zlib
from pathlib import Path

import numpy as np

SCHEMA = "ftx-oracle/1"
FILES = ("vectorize.py",)
ADAM7 = ((0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2))
CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}


def chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)


def pack_row(samples, depth: int) -> bytes:
    if depth == 16:
        return b"".join(struct.pack(">H", v) for v in samples)
    if depth == 8:
        return bytes(samples)
    out, acc, n = bytearray(), 0, 0
    for v in samples:
        acc = (acc << depth) | v
        n += depth
        if n == 8:
            out.append(acc)
            acc, n = 0, 0
    if n:
        out.append(acc << (8 - n))
    return bytes(out)


def paeth(a, b, c):
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    return a if pa <= pb and pa <= pc else (b if pb <= pc else c)


def filter_rows(rows: list[bytes], bpp: int, start: int) -> bytes:
    """Each row with filter type (start + row index) % 5, so every type is exercised."""
    out = bytearray()
    prev = bytes(len(rows[0])) if rows else b""
    for k, row in enumerate(rows):
        f = (start + k) % 5
        enc = bytearray()
        for i, x in enumerate(row):
            a = row[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            pred = (0, a, b, (a + b) // 2, paeth(a, b, c))[f]
            enc.append((x - pred) & 0xFF)
        out.append(f)
        out += enc
        prev = row
    return bytes(out)


def png(w, h, depth, color, pixels, *, plte=None, trns=None, interlace=0, start=0) -> bytes:
    """pixels[y][x] is a tuple of samples."""
    ch = CHANNELS[color]
    bpp = max(1, (depth * ch + 7) // 8)
    raw = b""
    passes = ADAM7 if interlace else ((0, 0, 1, 1),)
    for x0, y0, dx, dy in passes:
        xs, ys = list(range(x0, w, dx)), list(range(y0, h, dy))
        if not xs or not ys:
            continue
        rows = [pack_row([s for x in xs for s in pixels[y][x]], depth) for y in ys]
        raw += filter_rows(rows, bpp, start)
    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, color, 0, 0, interlace))
    if plte is not None:
        out += chunk(b"PLTE", bytes(v for rgb in plte for v in rgb))
    if trns is not None:
        out += chunk(b"tRNS", trns)
    return out + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


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

    meta = {"reference_files": {f: hashlib.sha256((ref / f).read_bytes()).hexdigest() for f in FILES},
            "numpy_version": np.__version__, "opencv_version": cv2.__version__, "pillow_version": PIL.__version__,
            "python_version": platform.python_version(), "machine": platform.machine()}
    rng = np.random.default_rng(11)

    def px(w, h, depth, color, k=None):
        ch = CHANNELS[color]
        top = (k or (1 << depth)) - 1
        return [[tuple(int(v) for v in rng.integers(0, top + 1, ch)) for _ in range(w)] for _ in range(h)]

    cases = []

    def add(cid, data: bytes):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "x.png"
            p.write_bytes(data)
            try:
                img, _ = V.load_image(str(p))
                out = {"shape": list(img.shape), "bgr": img.reshape(-1).tolist()}
            except Exception as exc:                    # noqa: BLE001 - the refusal is the answer
                out = {"error": type(exc).__name__, "message": str(exc)}
        cases.append({"id": cid, "input": {"png": base64.b64encode(data).decode()}, "output": out})

    sizes = ((9, 7), (1, 1), (17, 3))
    for depth in (1, 2, 4, 8, 16):
        for w, h in sizes:
            add(f"gray{depth}-{w}x{h}", png(w, h, depth, 0, px(w, h, depth, 0), start=w))
    add("gray16-high", png(9, 7, 16, 0, [[(int(v),) for v in rng.integers(0, 600, 9)] for _ in range(7)]))
    add("gray8-trns", png(9, 7, 8, 0, px(9, 7, 8, 0), trns=struct.pack(">H", 17)))
    for depth in (8, 16):
        for w, h in sizes:
            add(f"rgb{depth}-{w}x{h}", png(w, h, depth, 2, px(w, h, depth, 2), start=h))
    add("rgb8-trns", png(9, 7, 8, 2, px(9, 7, 8, 2), trns=struct.pack(">HHH", 1, 2, 3)))
    for depth in (1, 2, 4, 8):
        n = 1 << depth
        plte = [tuple(int(v) for v in rng.integers(0, 256, 3)) for _ in range(n)]
        add(f"pal{depth}", png(9, 7, depth, 3, px(9, 7, depth, 3), plte=plte))
    plte = [tuple(int(v) for v in rng.integers(0, 256, 3)) for _ in range(16)]
    add("pal4-simple-trns", png(9, 7, 4, 3, px(9, 7, 4, 3), plte=plte, trns=bytes([255] * 5 + [0] + [255] * 3)))
    add("pal8-trns", png(13, 5, 8, 3, px(13, 5, 8, 3, 40), plte=[tuple(int(v) for v in rng.integers(0, 256, 3)) for _ in range(40)],
                         trns=bytes(int(v) for v in rng.integers(0, 256, 25))))
    for depth in (8, 16):
        add(f"la{depth}", png(9, 7, depth, 4, px(9, 7, depth, 4)))
        add(f"rgba{depth}", png(9, 7, depth, 6, px(9, 7, depth, 6)))
    for color, depth in ((6, 8), (0, 2), (2, 16), (3, 4), (4, 8)):
        for w, h in ((9, 7), (17, 3), (1, 1), (3, 13)):
            plte = [tuple(int(v) for v in rng.integers(0, 256, 3)) for _ in range(16)] if color == 3 else None
            add(f"adam7-c{color}-d{depth}-{w}x{h}", png(w, h, depth, color, px(w, h, depth, color), plte=plte, interlace=1))
    alpha = [[(10 * x, 20 * y, 200, a) for x, a in enumerate((0, 1, 2, 127, 128, 200, 254, 255, 77))] for y in range(7)]
    add("rgba8-alpha-ramp", png(9, 7, 8, 6, alpha))

    args.out_dir.mkdir(parents=True, exist_ok=True)
    write(args.out_dir, "raster_image", meta, cases)

    # ---------------------------------------------------------------- colour
    every = np.arange(1 << 24, dtype=np.uint32)
    allc = np.stack([every & 255, (every >> 8) & 255, every >> 16], axis=1).astype(np.uint8).reshape(4096, 4096, 3)
    lab_all = cv2.cvtColor(allc, cv2.COLOR_BGR2LAB)
    sample = rng.integers(0, 256, (128, 128, 3), dtype=np.uint8)
    sample[:2] = np.stack([np.arange(256)] * 3, axis=1).reshape(2, 128, 3)  # every gray level
    ccases = [{"id": "lab-every-colour", "input": {"order": "index = b + 256 g + 65536 r, 4096 x 4096"},
               "output": {"sha256": hashlib.sha256(lab_all.tobytes()).hexdigest()}},
              {"id": "sample", "input": {"bgr": sample.reshape(-1).tolist()},
               "output": {"lab": cv2.cvtColor(sample, cv2.COLOR_BGR2LAB).reshape(-1).tolist(),
                          "gray": cv2.cvtColor(sample, cv2.COLOR_BGR2GRAY).reshape(-1).tolist(),
                          "sat": cv2.cvtColor(sample, cv2.COLOR_BGR2HSV)[:, :, 1].reshape(-1).tolist()}}]
    for bg in ((255.0, 255.0, 255.0), (80.5, 35.2, 25.9), (0.0, 0.0, 0.0), (254.5, 251.0, 260.0)):
        ch, known = V.make_channel(sample, "auto", bg)
        ccases.append({"id": f"auto-{bg}", "input": {"bg": list(bg)}, "output": {"channel": ch.reshape(-1).tolist()}})
    write(args.out_dir, "raster_color", meta, ccases)

    # ---------------------------------------------------------------- thresholds and morphology
    def gray(a):
        return {"w": int(a.shape[1]), "h": int(a.shape[0]), "data": a.reshape(-1).tolist()}
    imgs = {
        "uniform": rng.integers(0, 256, (48, 64), dtype=np.uint8),
        "bimodal": np.clip(np.where(rng.random((48, 64)) < 0.3, rng.normal(60, 15, (48, 64)),
                                    rng.normal(190, 20, (48, 64))), 0, 255).astype(np.uint8),
        "constant": np.full((20, 30), 77, np.uint8),
        "two-level": np.where(rng.random((20, 30)) < 0.5, 10, 240).astype(np.uint8),
        "sparse": np.where(rng.random((40, 50)) < 0.02, 255, 3).astype(np.uint8),
        "ramp": np.tile(np.arange(256, dtype=np.uint8), (5, 1)),
    }
    tcases = []
    for name, a in imgs.items():
        t, m = cv2.threshold(a, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
        tcases.append({"id": f"otsu-{name}", "input": gray(a), "output": {"t": float(t), "mask": m.reshape(-1).tolist()}})
    binimg = (rng.random((48, 64)) < 0.45).astype(np.uint8) * 255
    for k in range(2, 10):
        se = cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (k, k))
        tcases.append({"id": f"ellipse-{k}", "input": {"k": k}, "output": {"se": se.reshape(-1).tolist()}})
        for op, name in ((cv2.MORPH_CLOSE, "close"), (cv2.MORPH_OPEN, "open")):
            tcases.append({"id": f"{name}-{k}", "input": {**gray(binimg), "k": k},
                           "output": {"out": cv2.morphologyEx(binimg, op, se).reshape(-1).tolist()}})
    g = imgs["uniform"]
    tcases.append({"id": "gradient-3x3", "input": gray(g),
                   "output": {"out": cv2.morphologyEx(g, cv2.MORPH_GRADIENT, np.ones((3, 3), np.uint8)).reshape(-1).tolist()}})
    se = cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (5, 3))
    tcases.append({"id": "dilate-5x3", "input": gray(g), "output": {"out": cv2.dilate(g, se).reshape(-1).tolist()}})
    tcases.append({"id": "erode-5x3", "input": gray(g), "output": {"out": cv2.erode(g, se).reshape(-1).tolist()}})
    write(args.out_dir, "raster_binarize", meta, tcases)

    # ---------------------------------------------------------------- contours
    sys.path.insert(0, str(ref / "tests"))
    import test_vectorize as T

    def contour_info(mask, mode):
        cs, hier = cv2.findContours(mask, mode, cv2.CHAIN_APPROX_NONE)
        rows = []
        for c in cs:
            m = cv2.moments(c)
            ap = cv2.approxPolyDP(c, 0.7, True)
            rows.append({"pts": c[:, 0, :].tolist(), "area": cv2.contourArea(c), "arc": cv2.arcLength(c, True),
                         "approx": ap[:, 0, :].tolist(), "approx_arc": cv2.arcLength(ap, True),
                         "m": [m["m00"], m["m10"], m["m01"]], "rect": list(cv2.boundingRect(c))})
        return {"contours": rows, "hierarchy": [] if hier is None else hier[0].tolist()}

    kcases = []
    blobs = np.zeros((60, 80), np.uint8)
    for k in range(25):
        cx, cy, r = int(rng.integers(0, 80)), int(rng.integers(0, 60)), int(rng.integers(1, 12))
        cv2.circle(blobs, (cx, cy), r, 255, -1)
        if k % 3 == 0:
            cv2.circle(blobs, (cx, cy), max(r - 3, 0), 0, -1)
    blobs[5, 5] = 255
    blobs[50:52, 3:40] = 255
    nested = np.zeros((40, 40), np.uint8)
    for r, v in ((18, 255), (14, 0), (10, 255), (6, 0), (2, 255)):
        cv2.rectangle(nested, (20 - r, 20 - r), (20 + r, 20 + r), v, -1)
    noise = ((rng.random((50, 70)) < 0.35) * 255).astype(np.uint8)
    for name, img in (("blobs", blobs), ("nested", nested), ("noise", noise), ("full", np.full((9, 7), 255, np.uint8)),
                      ("empty", np.zeros((9, 7), np.uint8))):
        for mode, mname in ((cv2.RETR_CCOMP, "ccomp"), (cv2.RETR_EXTERNAL, "external")):
            kcases.append({"id": f"{name}-{mname}", "input": {**gray(img), "mode": mname}, "output": contour_info(img, mode)})
    sheet_masks = {}
    with tempfile.TemporaryDirectory() as tmp:
        for name, kw in (("plain", {}), ("navy", {"fg": (255, 255, 255), "bg": (25, 35, 80)}),
                         ("filled", {"fg": (30, 20, 20), "fill": (175, 30, 35)})):
            sheet = T.render_sheet(Path(tmp) / f"{name}.png", **kw)
            img, _ = V.load_image(str(sheet))
            ch, known = V.make_channel(img, "auto", V.estimate_background(img))
            mask, diag = V.binarize(ch, fg_bright_known=known)
            ok, enc = cv2.imencode(".png", mask)
            kcases.append({"id": f"sheet-{name}", "input": {"mask_png": base64.b64encode(enc.tobytes()).decode(),
                                                            "mode": "ccomp"},
                           "output": contour_info(mask, cv2.RETR_CCOMP)})
            sheet_masks[name] = enc
    write(args.out_dir, "raster_contours", meta, kcases)

    # ---------------------------------------------------------------- distance transform, components
    dcases = []
    for name, img in (("blobs", blobs), ("nested", nested), ("noise", noise)):
        for m in (3, 5):
            dt = cv2.distanceTransform((img > 0).astype(np.uint8), cv2.DIST_L2, m)
            dcases.append({"id": f"dt{m}-{name}", "input": {**gray(img), "mask": m},
                           "output": {"dt": [float(v) for v in dt.reshape(-1)]}})
        n, lab, stats, _ = cv2.connectedComponentsWithStats((img > 0).astype(np.uint8), connectivity=8)
        dcases.append({"id": f"cc-{name}", "input": gray(img),
                       "output": {"n": int(n), "labels": lab.reshape(-1).tolist(), "stats": stats.tolist()}})
    for name, enc in sheet_masks.items():
        mask = cv2.imdecode(enc, cv2.IMREAD_GRAYSCALE)
        out = {}
        for m in (3, 5):
            out[f"dt{m}"] = hashlib.sha256(cv2.distanceTransform((mask > 0).astype(np.uint8), cv2.DIST_L2, m)
                                           .astype("<f4").tobytes()).hexdigest()
        n, lab, stats, _ = cv2.connectedComponentsWithStats((mask > 0).astype(np.uint8), connectivity=8)
        out.update({"n": int(n), "labels_sha256": hashlib.sha256(lab.astype("<u4").tobytes()).hexdigest(),
                    "stats": stats.tolist()})
        dcases.append({"id": f"sheet-{name}", "input": {"mask_png": base64.b64encode(enc.tobytes()).decode()},
                       "output": out})
    write(args.out_dir, "raster_distance", meta, dcases)

    # ---------------------------------------------------------------- probabilistic Hough
    hcases = []

    def hough(cid, img, rho, theta, thr, minlen, gap, stored=None):
        segs = cv2.HoughLinesP(img, rho, theta, threshold=thr, minLineLength=minlen, maxLineGap=gap)
        segs = [] if segs is None else np.asarray(segs).reshape(-1, 4).tolist()
        if stored is None:
            stored = {"mask_png": base64.b64encode(cv2.imencode(".png", img)[1].tobytes()).decode()}
        hcases.append({"id": cid, "input": {**stored, "rho": rho, "theta": theta, "threshold": thr,
                                            "min_len": minlen, "max_gap": gap}, "output": {"lines": segs}})
    for name, enc in sheet_masks.items():
        mask = cv2.imdecode(enc, cv2.IMREAD_GRAYSCALE)
        s = min(mask.shape)
        hough(f"dashes-{name}", mask, 1, np.pi / 360, max(20, int(0.05 * s)), int(0.12 * s), int(0.025 * s),
              {"mask_png": base64.b64encode(enc.tobytes()).decode()})
    from PIL import Image, ImageDraw
    dash = Image.new("L", (600, 400), 0)
    d = ImageDraw.Draw(dash)
    d.rectangle([50, 50, 225, 325], outline=255, width=4)
    x0, y0, x1, y1 = 325, 60, 500, 330
    d.line([(x1, y0), (x0, y0), (x0, y1), (x1, y1)], fill=255, width=4)
    for y in range(y0, y1, 26):
        d.line([(x1, y), (x1, min(y + 14, y1))], fill=255, width=4)
    dash = np.asarray(dash)
    s = min(dash.shape)
    hough("dashes-synthetic", dash, 1, np.pi / 360, max(20, int(0.05 * s)), int(0.12 * s), int(0.025 * s))
    lines_img = ((rng.random((120, 160)) < 0.02) * 255).astype(np.uint8)
    for k in range(6):
        p, q = rng.integers(0, 160, 2), rng.integers(0, 120, 2)
        cv2.line(lines_img, (int(p[0]), int(q[0])), (int(p[1]), int(q[1])), 255, 1 + k % 3)
    size = 160.0
    hough("grain-lines", lines_img, 1, np.pi / 180, max(20, int(0.08 * size)), 0.2 * size, max(3, int(0.01 * size)))
    hough("grain-lines-thin", lines_img, 1, np.pi / 180, 10, 15.0, 2.0)
    write(args.out_dir, "raster_hough", meta, hcases)
    return 0


def write(out_dir: Path, name: str, meta: dict, cases: list) -> None:
    path = out_dir / f"{name}.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "trace", "fixture": name, "meta": meta,
                                "cases": cases}, separators=(",", ":")) + "\n")
    print(f"wrote {len(cases)} cases to {path}")


if __name__ == "__main__":
    raise SystemExit(main())
