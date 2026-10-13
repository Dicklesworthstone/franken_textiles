# Photo landmarks to measurement evidence

The photo commands turn **explicitly identified reference coordinates and landmark clicks**
into `fabric-measurements/1` records, for the existing fusion and readiness commands. They
run locally and do not open or upload the image. They do not detect paper or markers, apply
EXIF orientation, infer body landmarks, or establish garment fit.

## Plain Letter or A4 paper

Use coordinates in the full-resolution, EXIF-oriented image: x right, y down. Mark the four
corners clockwise from the upper-left corner of an actual flat sheet of known size. `corner_sigma_px` is the
one-sigma uncertainty of each manually marked coordinate, not a detector quality score.
This synthetic `reference.json` represents a Letter sheet at 0.5 mm per pixel:

```json
{
  "schema": "ftx-photo-reference/1",
  "reference": "plain_sheet",
  "paper": "letter",
  "image": "synthetic-camera",
  "image_size_px": [1500, 2000],
  "corners_px": [[234.1, 820.6], [665.9, 820.6], [665.9, 1379.4], [234.1, 1379.4]],
  "corner_sigma_px": 3
}
```

`paper` accepts `letter`, `a4`, or `auto`. The calibration checks both portrait and landscape
orientations against the camera rectangle constraints. `auto` refuses an ambiguous size;
explicitly choosing the wrong size is not a bypass of the rectangle check. Four corners alone
cannot establish that the object is paper, that it is flat, or that its nominal dimensions
are correct. Confirm those conditions yourself.

```bash
ftx measure photo-scale --reference reference.json --out scale.json --json
```

The report includes local scale, a plane homography, perspective warnings, and the original
reference input. `ok: false` means no usable calibration; the command returns exit 3 and can
save that diagnostic report, but it cannot produce measurements from it.

For `points.json`, supply named pairs, optionally with a view, depth offset and definition:

```json
{
  "arm_length@right": {
    "points": [[100, 900], [900, 900]],
    "view": "P1",
    "depth_offset_mm": 75
  }
}
```

```bash
ftx measure measure-photo --reference reference.json --points points.json \
  --camera-distance-mm 2500 --click-px 3 --bootstrap 200 --seed 0 \
  --taken-on 2026-10-13 --out photo-measurements.json --json
ftx measure validate photo-measurements.json --json
ftx measure fuse tape-measurements.json photo-measurements.json --out fused.json --json
ftx measure ready fused.json --garment set_in_sleeve --as-of 2026-10-13 --json
```

The synthetic pair measures **400.0 mm** in the reference plane. At 75 mm depth uncertainty
and 2500 mm camera distance, its depth uncertainty alone is **12.0 mm**. The output reports
that contribution separately from corner calibration and landmark clicking. This example
does not show real anatomy or validate where to put an arm-length tape.

For clicks made on a uniformly resized image, `--points-from-width 750` scales the landmark
coordinates to a 1500-pixel original width. It does **not** scale reference corners or click
sigma: those are always expressed in full-resolution pixels. Cropped or nonuniformly resized
images need an explicit coordinate transform before this command.

Angle names, including `shoulder_slope_deg`, return the absolute line angle below horizontal
in the sheet plane (0–90 degrees). `--sheet-level-sigma-deg` defaults to 1.5; the reference
sheet must be upright for horizontal to mean body horizontal. A rotated image or sheet is
not automatically corrected. `--view` labels the method but does not alter its geometry.

## Imported ChArUco detections

An external detector can supply a `ftx-photo-reference/1` document with:

```json
{
  "schema": "ftx-photo-reference/1",
  "reference": "charuco",
  "image_size_px": [1500, 2000],
  "board": {"squares_x": 6, "squares_y": 8, "square_mm": 22, "origin_mm": [12, 20]},
  "corners": [
    {"id": 0, "point_px": [244, 344]},
    {"id": 4, "point_px": [420, 344]},
    {"id": 10, "point_px": [244, 432]},
    {"id": 14, "point_px": [420, 432]},
    {"id": 30, "point_px": [244, 608]},
    {"id": 34, "point_px": [420, 608]}
  ]
}
```

This minimal example can establish a homography, but **supply at least twelve well-spread
corners for measurement**. Sparse detections can fail the bootstrap stability requirement.
Use the same `photo-scale` and `measure-photo` commands; no separate image runtime is needed.

IDs are zero-based, row-major internal chessboard corners: board position is
`[(id % (squares_x - 1) + 1) * square_mm, (id / (squares_x - 1) + 1) * square_mm]`.
`origin_mm` records where that board sits on the paper; the homography remains in board
coordinates. Optional `markers_detected`, `paper`, `image`, and `blur_sigma_px_estimate`
describe the external detection. The library does not compute blur from a file name.

The deterministic RANSAC uses a 2 mm threshold and at most 512 hypotheses, then refits the
stable consensus with normalized algebraic least squares. At least six inliers are required.
Excluded IDs, residuals and inlier counts are returned; duplicate/out-of-range IDs and invalid
coordinates are errors. This is not a claim of identical OpenCV RANSAC choices on noisy data.

## Uncertainty and failure behavior

Plain-sheet bootstrap perturbs the manual corner clicks and nominal paper dimensions
(0.5 mm sigma on each side). ChArUco bootstrap resamples inlier correspondences with
replacement, keeping draws with at least six unique points. Both use an explicit seed,
with a bounded 20–2000 draws and a minimum of 80% usable draws. Insufficient draws, rank
loss, or a landmark across the projective horizon is a refusal, never zero uncertainty.
The random stream is deliberately not NumPy-stream parity.

`--depth-offset-mm` adds uncertainty for distance from the sheet plane, not a perspective
bias correction. Default zero produces an explicit coplanarity warning. Camera distance must
be positive and exceed the magnitude of any depth offset. A straight photo distance does
not become a surface tape path or a circumference merely by naming it after one.

Every resulting record remains `PHOTO_ESTIMATED`. Fusion/readiness still enforce source
quality, definition compatibility and uncertainty; producing a record does not mean the
garment is ready. Print scaling, lens distortion, occlusion and incorrect landmark placement
are not solved by a small calibration residual. Keep tape girths and a muslin as independent
evidence.

No capture date is inferred: absent `--taken-on`, it remains null. New photo outputs use
exclusive creation (0600 on Unix) and never overwrite existing files or input aliases.
Failed measurement requests create no measurement output. With `--out`, the file contains
domain JSON; `--json` stdout contains the existing `ftx-cli/1` envelope. Without `--json`, a
successful write reports `wrote FILE` on stderr and leaves stdout empty.

The commands accept `--reference`, **not** the reference Python program's image-detection
`--image`/`--sheet`/`--paper` interface. Automatic image detection and calibration-sheet
generation remain separate, unfinished portions of FTX-19.
