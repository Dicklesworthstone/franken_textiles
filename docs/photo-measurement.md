# Photo landmarks to measurement evidence

The photo commands turn **a reference sheet and landmark clicks** into `fabric-measurements/1`
records for the existing fusion and readiness commands. They run locally without uploading
images. Supply an oriented PNG and a calibration-sheet sidecar for ChArUco detection, an
oriented PNG for plain-paper detection, or explicit reference coordinates. None of these
paths infers body landmarks, applies EXIF orientation, or establishes fit.

## Detect plain paper directly from a PNG

Show a whole, flat Letter or A4 sheet on a darker background. Apply the photograph's orientation
before export, then click landmarks on that same image. Use an opaque 8-bit grayscale or RGB PNG,
without EXIF metadata. JPEG/HEIC/WebP, transparency, palette/16-bit images and animation require
conversion first; the command refuses them rather than silently changing measurement pixels.

```bash
ftx measure photo-scale --image oriented.png --paper letter --out scale.json --json
ftx measure measure-photo --image oriented.png --paper letter --points points.json \
  --camera-distance-mm 2500 --click-px 3 --bootstrap 200 --seed 0 \
  --out photo-measurements.json --json
```

The `points.json` format and uncertainty controls below also apply to this path. `--paper`
accepts `letter`, `a4`, or `auto` (default). `--image` and `--reference` are mutually exclusive;
coordinate documents specify their own paper. An image is limited to 16 million pixels and
64 MiB encoded; exceeding a budget returns exit 4 before large decoded allocations. Nothing
is silently resized. Ordinary malformed inputs and rejected calibrations return exit 3.

The report retains detected corners, straight-edge support, blur and perspective diagnostics.
`image_input` records the image path, SHA-256, dimensions and stored-pixel orientation convention,
not the image itself. Measurement uses the unrounded calibration and bootstraps detected edge
samples, rather than treating detected corners as manual clicks. Review warnings and the detected
outline: a bright rectangle is not proof of nominal paper size, and scale applies only in the
sheet's plane. A failed detector can emit a diagnostic scale report, never measurement records.

## Detect a ChArUco calibration board directly from a PNG

Generate the board and keep its JSON sidecar with the printed sheet. Print at **100% / Actual
size**, disable fit-to-page, and verify both the 100 mm and 4 in bars with a ruler. A good
homography cannot detect a uniformly mis-scaled print. Letter uses an 8 by 6 board; A4 uses
8 by 7. The generator defaults to 22 mm squares, 16 mm markers, `DICT_4X4_50` and the
non-legacy checkerboard layout.

```bash
ftx measure calib-sheet --paper letter --out calibration --json
```

This creates `calib-sheet-letter.png`, `.pdf` and `.json` in a new directory. Photograph
**one** flat board, then export the oriented, opaque PNG under the same pixel restrictions
as the plain-paper path. The sidecar describes the physical board, not the dimensions or
position of that board in the photograph.

The optional detector uses a local Python environment with OpenCV. Nothing is downloaded or
installed implicitly by `ftx`; the measurement library itself has no Python/OpenCV dependency.
The tested dependency can be installed in a separate environment:

```bash
python3 -m venv .venv-charuco
.venv-charuco/bin/python -m pip install opencv-python-headless==4.13.0.92

ftx measure photo-scale --image oriented.png \
  --sheet calibration/calib-sheet-letter.json \
  --detector-python .venv-charuco/bin/python --out scale.json --json

ftx measure measure-photo --image oriented.png \
  --sheet calibration/calib-sheet-letter.json \
  --detector-python .venv-charuco/bin/python --points points.json \
  --camera-distance-mm 2500 --click-px 3 --bootstrap 200 --seed 0 \
  --out photo-measurements.json --json
```

`--detector-python` defaults to `python3`; it names an executable, not a shell command.
`--sheet` selects ChArUco detection and requires `--image`. It cannot be combined with
`--paper` or `--reference`; `--detector-python` is only valid with `--sheet`. Without
`--sheet`, the existing plain-paper behavior is unchanged and does not start Python.

The adapter is embedded in the binary and runs in isolated Python mode. Rust decodes and
hashes the photograph once, then passes the same full-resolution BGR pixels to the detector
through stdin. OpenCV identifies physical chessboard corner IDs and refines their pixel
positions. It does **not** fit the measurement homography. Rust validates the returned frame
and IDs, rebuilds the metric board from the original sidecar, and uses the same deterministic
RANSAC, unrounded homography and inlier bootstrap as imported ChArUco evidence. Rust also
computes the Sobel/percentile blur diagnostic on the board region.

The scale report says `coordinate_source: "detected_charuco"`. Its
`image_input.charuco_detection` receipt retains the complete sidecar, detected reference
coordinates, marker IDs, OpenCV version, detector settings, embedded-adapter SHA-256 and blur
method. It retains no photograph pixels. Measurement reports carry the same receipt, so a
scale and its uncertainty can be traced to the original image hash and board configuration.

**No fallback:** a plain rectangle, unreadable markers or failed OpenCV installation never
silently becomes a plain-paper calibration when `--sheet` was requested. Unsupported or
inconsistent board metadata, duplicate marker IDs from multiple boards, IDs outside the
selected board, and invalid detector output are refusals. Fewer than six identified corners
or a failed stable six-inlier homography produces no usable calibration; measurement then
refuses without creating an output. Twelve well-spread **inlier** corners are recommended;
rejected detections cannot suppress the sparse/partial-board warnings. Even an accepted
six-corner scale can fail the measurement bootstrap stability gate described below.

The adapter has a 30-second execution deadline, a 64 KiB request-header limit and a 256 KiB
limit on each output stream, in addition to the photo budgets above. Budget exhaustion is a
refusal, not permission to drop corners, resize the image or reuse a previous calibration.
OpenCV remains an optional external detector as permitted by plan section 10.1 / `ftx-0eq`;
this is not a native Rust ChArUco detector or a browser image-detection implementation.

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

## Correct a known lens without resampling the photograph

For a camera with known Brown radial/tangential distortion, both photo commands accept
`--lens lens.json --lens-id ID --rotation-cw-deg N`. Use these with ChArUco image detection
(`--image` plus `--sheet`) or imported ChArUco coordinates (`--reference`). The plain-sheet
detector is unchanged: its straight-edge evidence cannot be replaced by four distorted
corners, so it refuses this option rather than silently ignoring lens distortion.

A lens file has the following form. **These coefficients are synthetic test data, not a
calibration for your camera.** Supply a calibration measured for the exact lens, focus,
zoom, crop/stabilization geometry, resolution and already-applied rotation of your image.
`calibration_evidence` identifies that calibration and its assumptions; it is not validated
by matching an identifier. This command consumes intrinsics, it does not estimate them.
`calibrate-camera` defaults to zero-distortion pinhole calibration. Its explicit
`brown5_zero_skew` model also fits all five Brown coefficients from coordinate observations
or ChArUco photographs; see `docs/camera-calibration.md` for the stronger view, corner,
held-out and non-folding-domain requirements.

For either calibrated model, `--lens` also accepts the saved `ftx-camera-calibration/1` report
from `ftx measure calibrate-camera`. It re-runs calibration from the retained observations
and held-out assignments, requires an accepted solution, and verifies the stored intrinsics
against the recomputed values. Failed/altered reports and CLI envelopes are refused. There
is no need to transcribe the matrix into a second file; the original report and its hash are
retained with `lens_input.recomputed: true`. Replay uses the original declared model; it
never upgrades a pinhole calibration to Brown or establishes that physical lens settings
stayed unchanged. Fitted Brown parameters remain conditional estimates without covariance.

```bash
ftx measure calibrate-camera camera-views.json --out camera.json --json
ftx measure measure-photo --image pinhole-oriented.png \
  --sheet calibration/calib-sheet-letter.json --lens camera.json \
  --lens-id YOUR_LENS_ID --rotation-cw-deg 0 \
  --points points.json --out measurements.json --json
```

```json
{
  "schema": "ftx-photo-lens/1",
  "lens_id": "synthetic-brown",
  "image_size_px": [1000, 800],
  "rotation_cw_deg": 0,
  "calibration_evidence": "Synthetic Brown lens for regression tests; not a real camera.",
  "intrinsics": {
    "focal_px": [700.0, 650.0],
    "principal_px": [500.0, 360.0],
    "distortion": {
      "radial": [0.25, 0.015, 0.0],
      "tangential": [0.004, -0.003],
      "max_radius": 1.2
    }
  }
}
```

The radial array is `[k1,k2,k3]`; tangential is `[p1,p2]`. The adapter translates this to
OpenCV's `[k1,k2,p1,p2,k3]` order. `max_radius` is the calibrated **undistorted normalized**
radius, not a pixel count. The shared Rust camera model requires a certified, non-folding
inverse throughout this disk. An unsupported lens model, invalid coefficients, a board or
landmark outside the disk, or failure to invert is an explicit refusal.

```bash
ftx measure measure-photo --image oriented.png \
  --sheet calibration/calib-sheet-letter.json \
  --detector-python .venv-charuco/bin/python \
  --lens lens.json --lens-id YOUR_LENS_ID --rotation-cw-deg 0 \
  --points points.json --out corrected-measurements.json --json
```

`--lens-id` and `--rotation-cw-deg` are required capture assertions, not transformations.
The rotation must be 0, 90, 180 or 270 and match the lens file exactly. Image dimensions
must also match exactly. `--points-from-width` still rescales only landmark clicks, never
calibration parameters or the original image. Mismatches refuse before detector execution;
no crop, alternate lens, resizing or rotation is inferred. Lens JSON is bounded to 16 MiB.
An imported ChArUco reference with inline `intrinsics` cannot also select `--lens`;
neither source silently overrides the other. The image detector cannot inject its own lens.

OpenCV uses these same intrinsics when locating ChArUco corners, but returns **original
distorted pixel coordinates**. Rust rectifies the identified corners before fitting the
homography. It retains original landmark pixels, rectifies them before every homography
evaluation, and propagates click uncertainty through the full inverse-lens/homography
Jacobian. This handles unequal focal lengths and tangential distortion; multiplying a
click sigma by an average scale is not substituted for the Jacobian.

The resulting mapping is nonlinear. Its report therefore publishes
`homography_undistorted_px_to_board_mm`, **not** `homography_px_to_board_mm`. A consumer must
apply the supplied intrinsics' inverse distortion before this matrix. The reported centre,
pixel reprojection residuals and local mm/px refer to original pixels; perspective estimates
refer to the rectified plane. Scale variation samples the board centre and outer corners,
not a guaranteed maximum over its interior. `lens_input` retains the complete lens document,
its byte hash and the capture assertions in both scale and measurement reports.

**Uncertainty remains conditional on the supplied lens calibration.** Its focal-length,
principal-point and distortion-coefficient uncertainty is not estimated by the corner
bootstrap. The report states this explicitly, and records remain `PHOTO_ESTIMATED`. Lens
correction does not solve print scaling, depth differences, nonplanar surfaces or landmark
placement. Unknown calibration accuracy still needs independent ruler/tape checks.

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
Use the same `photo-scale` and `measure-photo` commands with `--reference`; no separate
image runtime is needed for imported detections.

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

Plain-sheet bootstrap resamples detected edge points for image input, or perturbs manual corner
clicks for coordinate input, and perturbs nominal paper dimensions (0.5 mm sigma on each side).
ChArUco bootstrap resamples inlier correspondences with replacement, keeping draws with at least
six unique points. All use an explicit seed,
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

## Synthetic conformance and CLI regression coverage

The independent OpenCV-rendered synthetic tests cover Letter/A4 boards, even-row legacy
layout, rotations, perspective, noise, blur, partial occlusion, physical corner identity,
wrong dictionaries/layouts, duplicate boards, malformed sidecars and the subprocess wire
protocol. The end-to-end Rust CLI tests additionally use the actual `calib-sheet` generator,
independently calculated 88 mm landmark pairs and a known projective camera. They verify
scale, inlier counts, click/depth uncertainty, bootstrap acceptance, repeatability,
validation/fusion, provenance, input protection and explicit failures.
Known-lens tests additionally render distorted pixels from a rigid tilted camera, recover
132 mm and 200 mm distances (including landmarks beyond the board), check inverse-lens
Jacobians independently for both lengths and angles, and reject mismatched capture metadata
before starting the detector. Core tests include a case where an uncorrected homography
passes calibration yet gives an incorrect off-board distance.

```bash
.venv-charuco/bin/python tools/photo/test_charuco_detector.py
FTX_DETECTOR_PYTHON="$PWD/.venv-charuco/bin/python" \
  cargo test -p ftx-cli --test photo_charuco_workflow -- --include-ignored
cargo test -p ftx-cli --test camera_calibration_workflow --test photo_image_workflow --test photo_workflow
cargo test -p ftx-cli --bin ftx charuco_image
cargo test -p ftx-measure --test photo --test charuco_evidence --test photo_lens
```

Set `FTX_DETECTOR_PYTHON` to an **absolute** interpreter path when running the CLI tests:
their working directories are temporary fixture directories. The four tests that require
OpenCV are explicitly ignored by default; the `ChArUco photo conformance` CI workflow installs
the pinned backend and runs them with `--include-ignored`. Plain-paper, imported-coordinate,
option-conflict and absent-backend tests remain runnable without OpenCV.

This establishes synthetic known-answer detection and an executable external-detector
workflow, not identical upstream image-oracle/RANSAC results on arbitrary noisy scenes,
NumPy bootstrap-stream parity, native Rust detector qualification or real-wearer measurement
accuracy. Those broader qualification claims remain separate from the implemented image path.
