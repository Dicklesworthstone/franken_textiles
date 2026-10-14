#!/usr/bin/env python3
"""Capture the reference's load_image and trace on WebP files as oracle fixtures (docs/spec/04 §10.2).

Writes OUT_DIR/raster_webp.json: WebP files and the BGR array vectorize.py's load_image gets from them (Pillow
12.3 with its bundled libwebp: RGBA when the file reports alpha, composited on white), or its refusal; and
OUT_DIR/trace_webp.json: the whole trace_image on WebP pattern sheets, as raster_trace.json records it for
PNGs. The load_image files cover
- Pillow's encoder: awkward sizes, qualities, every method, lossless (palettes of 2 to 200 colours), alpha
  (lossy and lossless, alpha_quality, exact);
- OpenCV's encoder, lossy and lossless;
- when installed, cwebp (filter strength, simple and strong filters, segments, sharpness, alpha methods and
  filters, near-lossless, metadata) and libvpx through ffmpeg (several token partitions, loop-filter deltas
  libwebp's encoder never writes); their versions go into the fixture's meta;
- container variants: extended files with ICCP/EXIF/XMP and unknown chunks, an alpha flag without ALPH, ALPH
  without the flag;
- damaged files: truncations, flipped bytes and cut slices. Both decoders must refuse them, or decode them to
  the same pixels;
- headers only the demuxer reads (RIFF sizes under 12, VP8X chunks over 10 bytes), and palette-only VP8L
  alpha planes built field by field for libwebp's 8-bit path, whole and cut;
- animations (Pillow's encoder, and img2webp and webpmux when installed; their versions go into the meta) and
  hand-built ANIM/ANMF containers: offset first frames with and without the alpha flag, and the demuxer's
  frame rules (bounds, chunk order, empty and overrunning frames, ANIM/ANMF chunks in still files).
Images over 1,024 pixels record the SHA-256 of the BGR bytes instead of the array. The reference is
identified by the SHA-256 of its files, not by a path. Usage:
    python capture_webp.py --reference SKILL/scripts OUT_DIR
Needs the reference's requirements (numpy, opencv-python-headless, pillow). Deterministic for given
encoder versions.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import platform
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

SCHEMA = "ftx-oracle/1"
FILES = ("vectorize.py",)


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
            "libwebp_version": features.version("webp"),
            "python_version": platform.python_version(), "machine": platform.machine()}
    cwebp, ffmpeg = shutil.which("cwebp"), shutil.which("ffmpeg")
    if cwebp:
        meta["cwebp_version"] = subprocess.run([cwebp, "-version"], capture_output=True, text=True).stdout.strip()
    if ffmpeg:
        meta["ffmpeg_version"] = subprocess.run([ffmpeg, "-version"], capture_output=True,
                                                text=True).stdout.split("\n")[0]
    rng = np.random.default_rng(31)
    cases = []
    work = Path(tempfile.mkdtemp(prefix="ftx-capture-webp-"))   # encoder inputs; left in the temp dir

    def load(data: bytes):
        p = work / "x.webp"
        p.write_bytes(data)
        try:
            img, _ = V.load_image(str(p))
        except Exception as exc:                        # noqa: BLE001 - the refusal is the answer
            return {"error": type(exc).__name__, "message": str(exc).replace(str(p), "x.webp")}
        out = {"shape": list(img.shape)}
        if img.shape[0] * img.shape[1] <= 1024:
            out["bgr"] = img.reshape(-1).tolist()
        else:
            out["sha256"] = hashlib.sha256(img.tobytes()).hexdigest()
        return out

    def add(cid: str, data: bytes):
        cases.append({"id": cid, "input": {"webp": base64.b64encode(data).decode()}, "output": load(data)})

    def pil(a, **kw) -> bytes:
        b = io.BytesIO()
        Image.fromarray(a).save(b, "WEBP", **kw)
        return b.getvalue()

    def photo(h, w):
        y, x = np.mgrid[0:h, 0:w] / max(h, w, 1)
        a = np.stack([128 + 100 * np.sin(7 * x + 3 * y), 128 + 90 * np.cos(5 * y - 2 * x * x),
                      128 + 80 * np.sin(11 * x * y + 1)], -1)
        a[h // 3:h // 2, w // 4:w // 2] = 20
        return np.clip(a + rng.normal(0, 8, a.shape), 0, 255).astype(np.uint8)

    def alpha_of(h, w):
        y, x = np.mgrid[0:h, 0:w]
        return np.clip(255 * (x / max(w - 1, 1)) * (y % 7 > 1) + 30 * ((x + y) % 5 == 0), 0, 255).astype(np.uint8)

    def chunk(fourcc: bytes, payload: bytes) -> bytes:
        return fourcc + struct.pack("<I", len(payload)) + payload + (b"\0" if len(payload) & 1 else b"")

    def riff(chunks: bytes) -> bytes:
        return b"RIFF" + struct.pack("<I", 4 + len(chunks)) + b"WEBP" + chunks

    def chunks_of(data: bytes):
        out, i = [], 12
        while i + 8 <= len(data):
            n = struct.unpack("<I", data[i + 4:i + 8])[0]
            out.append((data[i:i + 4], data[i + 8:i + 8 + n]))
            i += 8 + n + (n & 1)
        return out

    def vp8x(flags: int, w: int, h: int) -> bytes:
        return chunk(b"VP8X", bytes([flags, 0, 0, 0]) + (w - 1).to_bytes(3, "little") + (h - 1).to_bytes(3, "little"))

    # Pillow: sizes x quality, lossless, alpha
    for h, w in ((1, 1), (2, 2), (1, 17), (17, 1), (7, 9), (16, 16), (17, 33), (33, 17), (64, 3)):
        a = photo(h, w)
        for q in (10, 75, 100):
            add(f"pil-{h}x{w}-q{q}", pil(a, quality=q))
        add(f"pil-{h}x{w}-lossless", pil(a, lossless=True))
        rgba = np.dstack([a, alpha_of(h, w)])
        add(f"pil-{h}x{w}-alpha-q80", pil(rgba, quality=80))
        add(f"pil-{h}x{w}-alpha-lossless", pil(rgba, lossless=True))
    big = photo(61, 83)
    for m in range(7):
        add(f"pil-method{m}", pil(big, quality=70, method=m))
        add(f"pil-method{m}-lossless", pil(big, lossless=True, method=m, quality=50))
    add("pil-lossless-q100", pil(big, lossless=True, quality=100, method=6))
    rgba = np.dstack([big, alpha_of(61, 83)])
    add("pil-alpha-exact", pil(rgba, quality=60, exact=True))
    add("pil-alpha-quality5", pil(rgba, quality=50, alpha_quality=5))
    add("pil-gray", pil(big[..., 0].copy(), quality=80))
    for k in (2, 4, 16, 200):
        pal = rng.integers(0, 256, (k, 3)).astype(np.uint8)
        add(f"pil-palette{k}", pil(pal[rng.integers(0, k, (37, 53))], lossless=True))
    add("pil-photo-203x297", pil(photo(203, 297), quality=80))
    add("pil-photo-203x297-lossless", pil(photo(203, 297), lossless=True))
    # OpenCV
    for q in (1, 50, 100, 101):
        _, enc = cv2.imencode(".webp", big[..., ::-1], [cv2.IMWRITE_WEBP_QUALITY, q])
        add(f"cv-q{q}", enc.tobytes())
    # cwebp
    if cwebp:
        src, srca = work / "src.png", work / "srca.png"
        Image.fromarray(big).save(src)
        Image.fromarray(rgba).save(srca)

        def cw(cid, opts, inp=src):
            path = work / f"{cid}.webp"
            subprocess.run([cwebp, "-quiet", *opts, str(inp), "-o", str(path)], check=True)
            add(cid, path.read_bytes())
        for f in (0, 30, 100):
            for strong in ("-strong", "-nostrong"):
                cw(f"cwebp-f{f}{strong}", ["-q", "60", "-f", str(f), strong])
        for s in (1, 4):
            cw(f"cwebp-segments{s}", ["-q", "40", "-segments", str(s), "-sns", "100"])
        for sh in (3, 7):
            cw(f"cwebp-sharpness{sh}", ["-q", "50", "-f", "80", "-sharpness", str(sh), "-strong"])
        cw("cwebp-sharp-yuv", ["-q", "80", "-sharp_yuv"])
        cw("cwebp-m0", ["-q", "90", "-m", "0"])
        for am in (0, 1):
            for af in ("none", "fast", "best"):
                cw(f"cwebp-alpha-method{am}-{af}", ["-q", "70", "-alpha_method", str(am), "-alpha_filter", af], srca)
        cw("cwebp-alpha-q0", ["-q", "70", "-alpha_q", "0"], srca)
        cw("cwebp-z9", ["-z", "9"])
        cw("cwebp-near-lossless40", ["-lossless", "-near_lossless", "40"])
        cw("cwebp-lossless-alpha-exact", ["-lossless", "-exact"], srca)
    # libvpx keyframes (several token partitions, loop-filter deltas), wrapped as simple WebP files
    if ffmpeg:
        src = work / "vpx.png"
        Image.fromarray(photo(61, 83)).save(src)
        for slices in (1, 4, 8):
            for opts, tag in ((["-crf", "4", "-b:v", "8M"], "hq"), (["-crf", "40", "-b:v", "200k"], "mq"),
                              (["-crf", "20", "-b:v", "2M", "-sharpness", "5"], "sharpness5")):
                ivf = work / f"vpx{slices}{tag}.ivf"
                subprocess.run([ffmpeg, "-loglevel", "error", "-y", "-i", str(src), "-frames:v", "1", "-c:v",
                                "libvpx", "-slices", str(slices), *opts, "-f", "ivf", str(ivf)], check=True)
                data = ivf.read_bytes()
                size = struct.unpack("<I", data[32:36])[0]
                add(f"libvpx-partitions{slices}-{tag}", riff(chunk(b"VP8 ", data[44:44 + size])))

    # container variants built from Pillow files
    lossy = chunks_of(pil(big, quality=70))
    alpha_file = chunks_of(pil(rgba, quality=70))     # VP8X, ALPH, VP8
    vp8 = dict(lossy)[b"VP8 "]
    alph = dict(alpha_file)[b"ALPH"]
    add("vp8x-iccp-exif-xmp", riff(vp8x(0x20 | 0x08 | 0x04, 83, 61) + chunk(b"ICCP", b"\0" * 12)
                                   + chunk(b"VP8 ", vp8) + chunk(b"EXIF", b"II*\0") + chunk(b"XMP ", b"<x/>")))
    add("vp8x-unknown-chunk", riff(vp8x(0, 83, 61) + chunk(b"ABCD", b"xyz") + chunk(b"VP8 ", vp8)))
    add("vp8x-alpha-flag-no-alph", riff(vp8x(0x10, 83, 61) + chunk(b"VP8 ", vp8)))
    add("vp8x-alph-without-flag", riff(vp8x(0, 83, 61) + chunk(b"ALPH", alph) + chunk(b"VP8 ", vp8)))
    add("vp8x-alph-after-image", riff(vp8x(0x10, 83, 61) + chunk(b"VP8 ", vp8) + chunk(b"ALPH", alph)))
    add("vp8x-canvas-mismatch", riff(vp8x(0, 84, 61) + chunk(b"VP8 ", vp8)))
    add("simple-trailing-chunk", riff(chunk(b"VP8 ", vp8) + chunk(b"JUNK", b"12345")))

    # damaged files
    bases = {"lossy": pil(big, quality=70), "alpha": pil(rgba, quality=70), "lossless": pil(big, lossless=True),
             "alpha-lossless": pil(rgba, lossless=True)}
    for name, data in bases.items():
        for k, cut in enumerate((0.3, 0.7, 0.98)):
            add(f"{name}-truncated-{k}", data[:int(len(data) * cut)])
            d = bytearray(data[:int(len(data) * cut)])
            d[4:8] = struct.pack("<I", len(d) - 8)
            add(f"{name}-truncated-riff-fixed-{k}", bytes(d))
        for t in range(5):
            d = bytearray(data)
            for _ in range(1 + t % 3):
                d[int(rng.integers(40, len(d)))] = int(rng.integers(0, 256))
            add(f"{name}-corrupt-{t}", bytes(d))
        j = len(data) // 2
        add(f"{name}-slice-cut", data[:j] + data[j + 7:])

    # headers WebPGetFeatures refuses (so WebPAnimDecoderNew does) though the demuxer reads them
    add("riff-size-10", b"RIFF" + struct.pack("<I", 10) + b"WEBP" + chunk(b"VP8 ", vp8))
    add("riff-size-8-vp8x", b"RIFF" + struct.pack("<I", 8) + b"WEBP" + vp8x(0, 83, 61) + chunk(b"VP8 ", vp8))
    for extra in (1, 2):
        body = bytes(4) + (82).to_bytes(3, "little") + (60).to_bytes(3, "little") + bytes(extra)
        add(f"vp8x-chunk-size-{10 + extra}", riff(chunk(b"VP8X", body) + chunk(b"VP8 ", vp8)))

    # libwebp's 8-bit path for a palette-only VP8L alpha plane checks the end of the stream only after
    # each literal: built field by field (LSB first), whole and cut by one or two bytes
    def palette_alpha(pixels: list[int]) -> bytes:
        one = [(1, 1), (0, 1), (0, 1), (0, 1)]                        # a simple code of the one symbol 0
        f = [(1, 1), (3, 2), (1, 8)]                                  # colour indexing, two colours
        f += [(0, 1), (1, 1), (0, 1), (1, 1), (0x60, 8)] + one * 4    # palette: green 0x60, then zeros
        f += [(0, 1), (0, 1), (0, 1)]                                 # no more transforms, cache or meta
        f += [(1, 1), (1, 1), (1, 1), (0x00, 8), (0xA5, 8)] + one * 4  # green: literals 0x00 and 0xA5
        f += [(p, 1) for p in pixels]
        acc = n = 0
        for v, k in f:
            acc |= v << n
            n += k
        return acc.to_bytes((n + 7) // 8, "little")

    for w, h in ((16, 6), (16, 2), (9, 7)):        # 89, 81 and 91 bits: one missing bit is still decoded
        y, x = np.mgrid[0:h, 0:w]
        gray = np.dstack([(x * 37 + y * 11) % 256] * 3).astype(np.uint8)
        frame = chunk(b"VP8 ", dict(chunks_of(pil(gray, quality=80)))[b"VP8 "])
        stream = palette_alpha([(i * 7 + 3) % 5 % 2 for i in range((w + 7) // 8 * h)])
        for cut in (0, 1, 2):
            alph = chunk(b"ALPH", bytes([1]) + stream[:len(stream) - cut])
            add(f"alpha-8bit-path-{w}x{h}-cut{cut}", riff(vp8x(0x10, w, h) + alph + frame))

    # animations: Image.open shows the first frame at its offset on a zeroed canvas (white once composited,
    # black for a file without the alpha flag); every frame must still parse and fit. Own generator, so the
    # cases above keep their noise.
    arng = np.random.default_rng(43)

    def frame_img(h, w, phase):
        y, x = np.mgrid[0:h, 0:w] / max(h, w, 1)
        a = np.stack([128 + 100 * np.sin(7 * x + 3 * y + phase), 128 + 90 * np.cos(5 * y - 2 * x * x + phase),
                      128 + 80 * np.sin(11 * x * y + 1 + phase)], -1)
        return np.clip(a + arng.normal(0, 6, a.shape), 0, 255).astype(np.uint8)

    def pil_anim(frames, **kw) -> bytes:
        b = io.BytesIO()
        ims = [Image.fromarray(f) for f in frames]
        ims[0].save(b, "WEBP", save_all=True, append_images=ims[1:], duration=80, **kw)
        return b.getvalue()

    def image_chunks(data: bytes) -> bytes:
        return b"".join(chunk(f, p) for f, p in chunks_of(data) if f in (b"ALPH", b"VP8 ", b"VP8L"))

    def anim(bg=0xFFFFFFFF, loops=0) -> bytes:
        return chunk(b"ANIM", struct.pack("<IH", bg, loops))

    def anmf(x, y, w, h, frame: bytes, bits=0) -> bytes:
        head = b"".join(v.to_bytes(3, "little") for v in (x // 2, y // 2, w - 1, h - 1, 100)) + bytes([bits])
        return chunk(b"ANMF", head + frame)

    fa, fb = frame_img(21, 30, 0.0), frame_img(21, 30, 1.3)
    aa = alpha_of(21, 30)
    add("anim-pil-lossy", pil_anim([fa, fb], quality=70))
    add("anim-pil-lossless", pil_anim([fa, fb], lossless=True))
    add("anim-pil-alpha", pil_anim([np.dstack([fa, aa]), np.dstack([fb, aa])], quality=70))
    add("anim-pil-alpha-lossless", pil_anim([np.dstack([fa, aa]), np.dstack([fb, aa])], lossless=True))
    img2webp, webpmux = shutil.which("img2webp"), shutil.which("webpmux")
    if img2webp:
        meta["img2webp_version"] = subprocess.run([img2webp, "-version"], capture_output=True,
                                                  text=True).stdout.strip()
        pngs = []
        for i, f in enumerate((fa, fb, np.dstack([fa, aa]))):
            pngs.append(work / f"anim{i}.png")
            Image.fromarray(f).save(pngs[-1])
        for opts, tag in ((["-lossy", "-q", "60"], "lossy"), (["-lossless"], "lossless"), (["-mixed"], "mixed")):
            path = work / f"img2webp-{tag}.webp"
            subprocess.run([img2webp, "-loop", "2", *opts, "-d", "70", *map(str, pngs), "-o", str(path)],
                           check=True, capture_output=True)
            add(f"anim-img2webp-{tag}", path.read_bytes())
    if webpmux:
        meta["webpmux_version"] = subprocess.run([webpmux, "-version"], capture_output=True,
                                                 text=True).stdout.strip()
        big_frame = work / "mux-big.webp"
        big_frame.write_bytes(pil(frame_img(40, 50, 2.0), quality=75))
        for tag, data, off in (("lossy", pil(fa, quality=75), "+10+6"), ("lossless", pil(fa, lossless=True), "+4+18"),
                               ("alpha", pil(np.dstack([fa, aa]), quality=75), "+8+8")):
            first, path = work / f"mux-{tag}-in.webp", work / f"mux-{tag}.webp"
            first.write_bytes(data)
            subprocess.run([webpmux, "-frame", str(first), "+100" + off + "+0+b", "-frame", str(big_frame),
                            "+100+0+0+1-b", "-loop", "0", "-bgcolor", "255,0,0,255", "-o", str(path)],
                           check=True, capture_output=True)
            add(f"anim-webpmux-offset-{tag}", path.read_bytes())
    lossy, lossless = image_chunks(pil(fa, quality=75)), image_chunks(pil(fa, lossless=True))
    with_alph = image_chunks(pil(np.dstack([fa, aa]), quality=75))          # ALPH, then VP8
    k = with_alph.index(b"VP8 ")
    second = image_chunks(pil(frame_img(40, 50, 2.0), quality=60))           # 50 x 40
    for flags, ftag in ((0x02, "no-alpha-flag"), (0x12, "alpha-flag")):
        for fr, frtag in ((lossy, "vp8"), (with_alph, "alph-vp8"), (lossless, "vp8l")):
            add(f"anim-{ftag}-{frtag}-at-10-6", riff(vp8x(flags, 64, 48) + anim() + anmf(10, 6, 30, 21, fr)
                                                      + anmf(0, 0, 50, 40, second)))
    A = vp8x(0x12, 64, 48) + anim()
    damaged = bytearray(second)
    damaged[40:60] = bytes(20)
    for cid, body in (
            ("anim-frame-header-size-ignored", A + anmf(2, 2, 7, 9, lossy)),      # the bitstream's size counts
            ("anim-dispose-no-blend-bits", A + anmf(6, 4, 30, 21, with_alph, bits=3) + anmf(0, 0, 50, 40, second, 1)),
            ("anim-second-frame-damaged", A + anmf(0, 0, 30, 21, lossy) + anmf(0, 0, 50, 40, bytes(damaged))),
            ("anim-second-anim-chunk", A + anim(0, 3) + anmf(4, 4, 30, 21, lossy)),
            ("anim-unknown-chunk-in-frame", A + anmf(0, 0, 30, 21, lossy + chunk(b"ABCD", b"xy"))),
            ("anim-empty-frame-then-frame", A + chunk(b"ANMF", bytes(16)) + anmf(0, 0, 30, 21, lossy)),
            ("anim-frame-outside-canvas", A + anmf(36, 0, 30, 21, lossy)),
            ("anim-second-frame-outside-canvas", A + anmf(0, 0, 30, 21, lossy) + anmf(20, 20, 50, 40, second)),
            ("anim-anmf-before-anim", vp8x(0x12, 64, 48) + anmf(0, 0, 30, 21, lossy) + anim()),
            ("anim-no-frames", A),
            ("anim-anim-chunk-short", vp8x(0x12, 64, 48) + chunk(b"ANIM", b"12345") + anmf(0, 0, 30, 21, lossy)),
            ("anim-anmf-short", A + chunk(b"ANMF", bytes(15))),
            ("anim-empty-frame-last", A + anmf(0, 0, 30, 21, lossy) + chunk(b"ANMF", bytes(16))),
            ("anim-unknown-chunk-before-image", A + anmf(0, 0, 30, 21, chunk(b"ABCD", b"xy") + lossy)),
            ("anim-alph-without-image", A + anmf(0, 0, 30, 21, with_alph[:k])),
            ("anim-alph-after-image", A + anmf(0, 0, 30, 21, with_alph[k:] + with_alph[:k])),
            ("anim-alph-with-vp8l", A + anmf(0, 0, 30, 21, with_alph[:k] + lossless)),
            ("anim-image-outside-anmf", A + lossy),
            ("anim-frame-overruns-anmf", A + chunk(b"ANMF", anmf(0, 0, 30, 21, b"")[8:24] + lossy[:20]) + lossy[20:]),
            ("still-trailing-anim", vp8x(0x10, 30, 21) + with_alph + anim()),
            ("still-trailing-anim-and-frame", vp8x(0x10, 30, 21) + with_alph + anim() + anmf(0, 0, 30, 21, lossy)),
            ("still-trailing-frame-alph-only", vp8x(0x10, 30, 21) + with_alph + anim() + anmf(0, 0, 30, 21,
                                                                                               with_alph[:k])),
            ("still-anim-before-image", vp8x(0x10, 30, 21) + anim() + with_alph),
            ("still-anmf-before-anim", vp8x(0x10, 30, 21) + with_alph + anmf(0, 0, 30, 21, lossy))):
        add(cid, riff(body))

    args.out_dir.mkdir(parents=True, exist_ok=True)
    path = args.out_dir / "raster_webp.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "trace", "fixture": "raster_webp", "meta": meta,
                                "cases": cases}, separators=(",", ":")) + "\n")
    print(f"wrote {len(cases)} cases to {path}")

    # ------------------------------------------------ the whole trace on WebP sheets (as raster_trace.json)
    tcases = []
    W, H, AA, K = 360, 260, cv2.LINE_AA, (20, 20, 20)
    pent = np.array([(30, 40), (150, 30), (170, 120), (110, 200), (40, 180)], np.int32)

    def sheet(bg=(255, 255, 255), fg=K, light=0.0, noise=0.0):
        img = np.full((H, W, 3), bg, np.uint8)
        cv2.polylines(img, [pent], True, fg, 3, AA)
        cv2.arrowedLine(img, (90, 115), (90, 70), fg, 2, AA, tipLength=0.2)
        cv2.arrowedLine(img, (90, 115), (90, 165), fg, 2, AA, tipLength=0.2)
        cv2.ellipse(img, (262, 125), (68, 92), 0, 0, 360, fg, 3, AA)
        if light or noise:
            ramp = 1.0 - light * np.linspace(0, 1, W)[None, :, None]
            img = np.clip(img * ramp + rng.normal(0, noise, img.shape), 0, 255).astype(np.uint8)
        return img

    def trace_case(cid, data: bytes, **kw):
        p = work / "sheet.webp"
        p.write_bytes(data)
        try:
            r = V.trace_image(str(p), None, **kw)
            r["pattern"]["meta"]["source"]["image"] = "sheet.webp"   # the temp path is not part of the answer
            _, mask = cv2.imencode(".png", r["mask"])
            out = {"pattern": r["pattern"], "diagnostics": r["diagnostics"],
                   "mask_png": base64.b64encode(mask.tobytes()).decode()}
        except Exception as exc:                        # noqa: BLE001 - the refusal is the answer
            out = {"error": type(exc).__name__, "message": str(exc).replace(str(p), "sheet.webp")}
        tcases.append({"id": cid, "input": {"webp": base64.b64encode(data).decode(), "opts": kw}, "output": out})

    trace_case("lossy-q85", pil(sheet()[..., ::-1].copy(), quality=85))
    trace_case("lossless", pil(sheet()[..., ::-1].copy(), lossless=True))
    rgba_sheet = np.dstack([sheet()[..., ::-1], np.full((H, W), 255, np.uint8)])
    rgba_sheet[:, :40, 3] = 0                          # a transparent margin, white after the composite
    trace_case("alpha-margin", pil(rgba_sheet, quality=90))
    trace_case("photo-q70", pil(sheet(light=0.25, noise=4.0)[..., ::-1].copy(), quality=70))
    trace_case("navy-q80", pil(sheet(bg=(80, 35, 25), fg=(245, 245, 245))[..., ::-1].copy(), quality=80))
    trace_case("photo-adaptive", pil(sheet(light=0.4, noise=3.0)[..., ::-1].copy(), quality=85),
               threshold="adaptive")
    # an animation whose first frame is the sheet, offset on a larger canvas left transparent (white)
    frame = image_chunks(pil(sheet()[..., ::-1].copy(), quality=85))
    trace_case("animated-offset-q85", riff(vp8x(0x12, W + 40, H + 20) + anim() + anmf(20, 10, W, H, frame)
                                           + anmf(0, 0, W, H, frame)))
    path = args.out_dir / "trace_webp.json"
    path.write_text(json.dumps({"schema": SCHEMA, "subsystem": "trace", "fixture": "trace_webp", "meta": meta,
                                "cases": tcases}, separators=(",", ":")) + "\n")
    print(f"wrote {len(tcases)} cases to {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
