# Calibrate a shared pinhole camera from several sheet views

`ftx measure calibrate-camera` estimates both focal lengths and the principal
point from identified points on a planar reference. It supplies intrinsics for
the existing sheet-pose, triangulation and body-fit camera model instead of
requiring you to guess a focal length. No images or model assets are downloaded.

## Calibrate directly from ChArUco photographs

The image path detects corners locally with the same optional OpenCV adapter used by
photo measurement, then gives **all** identified corners to the existing Rust camera
solver. It does not run photo-scale RANSAC, discard inconvenient corners, or re-detect
with a fitted camera. It consumes the sidecar produced by `ftx measure calib-sheet`:

```sh
ftx measure calib-sheet --paper letter --out sheet --json
ftx measure calibrate-camera --images capture/views.json \
  --sheet sheet/calib-sheet-letter.json \
  --detector-python .venv-charuco/bin/python --out camera.json --json
```

The detector environment needs `opencv-python-headless==4.13.0.92`; installation is
explicit, never performed by `ftx`. The coordinate-only command below remains Python-free.
Print at actual size, check both reference rulers, and use a rigid flat board. Photograph
it at different **out-of-plane** tilts without changing the camera settings or image
geometry. Apply orientation before exporting opaque 8-bit RGB/grayscale PNGs without EXIF.

This is still a **zero-skew pinhole calibration**, not a Brown-distortion fit. Supply
rectified images or independently justified negligible distortion. A successful residual
check alone does not establish that a raw phone image follows the pinhole model.

The manifest uses `ftx-camera-calibration-images/1`. The following paths and evidence
strings are an example structure, not a camera calibration or verified capture:

```json
{
  "schema": "ftx-camera-calibration-images/1",
  "image_model": "pinhole_zero_skew",
  "lens_id": "my-fixed-camera-settings",
  "pinhole_evidence": "Replace with evidence for rectification or negligible distortion",
  "reference_evidence": "Replace with the board flatness and ruler checks",
  "image_size_px": [1600, 1200],
  "rotation_cw_deg": 0,
  "held_out_corner_ids": [0, 6, 28, 34],
  "views": [
    {"image": "left.png", "frame": {"stream": "capture", "timestamp_us": 0}, "partition": "fit"},
    {"image": "right.png", "frame": {"stream": "capture", "timestamp_us": 100000}, "partition": "fit"},
    {"image": "up.png", "frame": {"stream": "capture", "timestamp_us": 200000}, "partition": "fit"},
    {"image": "check.png", "frame": {"stream": "capture", "timestamp_us": 300000}, "partition": "held_out"}
  ]
}
```

Image paths are relative to the **manifest directory**, not the process working directory.
Timestamps are actual decoded presentation times in microseconds on the declared stream,
not image ordinals inferred from filenames. The lens/settings and rotation fields are
capture assertions, not transformations. Every photograph must have the exact declared size.
All views share the same physical board and lens geometry; occlusion may hide different
subsets of corners. Board IDs determine metric controls, in metres, without hand transcription.

Assign both kinds of held-out evidence **before** fitting. This image workflow requires
at least three fit views and one held-out view, plus at least eight detected fit corners and
four detected held-out corners in **every** image. `held_out_corner_ids` refers to physical
row-major internal chessboard IDs and is shared across images. The example IDs are the four
extreme corners on the generated Letter board; use `[0,6,35,41]` for generated A4. Unlisted
detected IDs are fit corners. Detection may use adjacent ArUco markers, but held-out
chessboard corners never enter camera or pose fitting. Detection errors within one image
may still be correlated; this is not an independent pixel-noise covariance estimate.

No-board, sparse-board, missing backend, changed dimensions, invalid IDs and duplicate
images are explicit refusals, not permission to drop a view or change its partition.
Duplicates are checked using decoded BGR pixels, so renaming or re-encoding a fit image
cannot turn it into independent held-out evidence. Valid but geometrically inconsistent
or insufficiently tilted acquisitions produce the existing rejected/unobservable report
with `solution: null`. The unchanged native 3-pixel residual and rank gates apply to all views.

The saved result is the existing `ftx-camera-calibration/1` report. It can be passed directly
to `sheet-pose --calibration` or ChArUco photo commands with `--lens camera.json --lens-id ID
--rotation-cw-deg N`. Both handoffs recompute the retained observations before using the
camera. `input` contains all metric controls, detected pixels and partitions; an additional
`image_acquisition` receipt retains the manifest and sidecar, their byte hashes, each image's
byte and decoded-pixel hashes, detector version and embedded-adapter hash. No image pixels
are embedded. `input_sha256` hashes the expanded coordinate input supplied to the native
solver; `image_acquisition.manifest_sha256` hashes the original manifest bytes. None of these
hashes proves physical camera settings or authenticity of the photographed reference.

The batch is limited to 32 images, 16 million pixels and 64 MiB encoded per image, and
128 million pixels across the batch; manifest/sidecar JSON retain the 16 MiB input limit.
One image is decoded and released at a time. The existing 30-second detector deadline
applies to each view. Limits refuse without resizing, truncating a batch or publishing a
partial calibration. New outputs never overwrite earlier evidence or input aliases.

Run the synthetic image-to-calibration-to-pose workflow, including optional OpenCV tests:

```sh
cargo test -p ftx-cli --test camera_image_workflow -- --include-ignored
cargo test -p ftx-cli --bin ftx calibration_images
```

## Run the complete synthetic example

```sh
ftx measure calibrate-camera docs/examples/camera-calibration.json \
  --out calibration.json --json

ftx measure sheet-pose --reference docs/examples/calibrated-sheet-pose.json \
  --calibration calibration.json --lens-id synthetic-pinhole \
  --out camera.json --json
```

The supplied example contains **analytically projected synthetic coordinates**,
not a real camera calibration. It recovers focal lengths 960 and 1000 pixels and
principal point `[640,480]` in a 1280-by-960 image. Three views determine the
intrinsics; a fourth is held out. Each view also has held-out corners. The second
command uses the saved calibration with that fourth view's sheet observations.
All output paths must be new: inputs, previous reports and their file aliases
are never overwritten. Without `--out`, the result is printed but not saved.

## Acquisition and input contract

This slice fits **zero-skew pinhole intrinsics only**. Pixels must already be
rectified or have negligible distortion, with evidence for that assumption.
It does not estimate radial/tangential lens distortion or silently remove it.
The same lens, focus, zoom, crop, stabilization geometry, resolution and applied
orientation must hold throughout the acquisition and subsequent reuse. Matching
metadata is necessary but does not verify those physical conditions.

Use a rigid flat reference with accurately known point locations, observed at
different out-of-plane tilts. Merely translating the reference or rotating it
within its plane cannot supply the required independent constraints. More
independent views and well-spread controls provide redundancy; they do not fix
an incorrect reference, lens model or correspondence assignment.

The top-level schema is `ftx-camera-calibration-input/1`. Its fields are:

| Field | Meaning |
|---|---|
| `image_model` | Exactly `pinhole_zero_skew` |
| `lens_id` | Explicit identifier for the unchanged lens/settings |
| `pinhole_evidence` | Receipt explaining why these pixels follow the pinhole assumption |
| `reference_evidence` | Receipt for the physical reference, dimensions and flatness |
| `image_size_px` | Common oriented width and height |
| `rotation_cw_deg` | Rotation **already applied**, one of 0, 90, 180 or 270 |
| `controls` | Distinct `{id, sheet_xz_m: [x,z]}` reference locations in **metres** |
| `views` | `{frame: {stream, timestamp_us}, partition, corners}` observations |

Each corner is `{id, pixel_px: [x,y], partition}`; identifiers refer to `controls`.
Unknown fields and IDs, duplicate identities or locations, and out-of-image
pixels are errors. All views use unique decoded timestamps within one stream.
At least three fit views are required, each with at least four noncollinear fit
controls. Different subsets of the reference may be visible in different views.

Partitions are `fit` or `held_out`, assigned before calibration. A held-out view
does **not** determine intrinsics; its fit corners determine only its own sheet
pose after intrinsics are fixed. Held-out corners influence neither intrinsics
nor any pose. Every original observation is checked; none is trimmed as an outlier.

## Results and checked reuse

The saved `ftx-camera-calibration/1` report contains canonical input, the raw
input digest, numerical rank diagnostics, every available per-corner residual,
flags and advisories. Only `ACCEPTED` includes `solution.intrinsics` and the
sheet-local poses. Every corner must project within 3 pixels and every pose must
pass the existing rigid-sheet, positive-depth and calibrated-domain checks.
The rank test rejects insufficient orientation information before solving.
Missing held-out views or corners are visible advisories, not invented evidence.

| Exit | Calibration outcome |
|---|---|
| 0 | Accepted conditional calibration |
| 2 | Rejected or unobservable; diagnostic report, no usable intrinsics or poses |
| 3 | Invalid input or output refusal |
| 4 | Input budget exceeded |

The `sheet-pose --calibration` handoff requires the saved report, **not** its
`ftx-cli/1` envelope. The reference must omit inline `intrinsics`; conflicting
sources refuse instead of overriding one another. `--lens-id`, image dimensions
and the reference's `rotation_applied_degrees` must match the calibration exactly.
The command recomputes calibration from the saved inputs and compares the saved
intrinsics before using them. It records both source digests and the complete
calibration input in the resulting pose report. This is internal consistency
checking, not cryptographic authentication or proof that lens settings stayed fixed.
Existing sheet-pose commands with explicit inline intrinsics are unchanged.

**Calibration-sheet poses are not automatically a shared world frame.** The
reference may move between calibration views; each returned
`camera_in_sheet_frame` is relative to its own sheet pose. For triangulation,
reuse the intrinsics with a stationary reference defining the same metric frame
in each image. Do not combine moving-sheet extrinsics as though they were a
synchronized or stationary-world camera rig.

## Numerical and verification boundaries

This is an algebraic planar-constraint fit, not nonlinear maximum likelihood.
It fixes skew to zero and publishes no uncertainty estimate. The principal point
must lie inside the image; off-image optical centers from some crops are not
supported. The existing pinhole's normalized radius limit of 2 applies. A small
residual does not establish metric accuracy, body measurements or garment fit.

Limits are 128 controls, 32 views, 128 corners per view and 16 MiB per JSON input.
Budgets refuse rather than truncate. The numerical library and coordinate-only command
use no runtime Python/OpenCV or new dependency; image acquisition uses the optional detector
described above. Algorithm details and the primary formula source are in
`crates/ftx-measure/CAMERA_CALIBRATION.md`.

Ten direct synthetic tests cover known intrinsics/poses, noise, ordering,
degenerate views, missing controls, both held-out partitions, metric scaling,
invalid inputs and nonprojective warps. Five real-CLI tests cover the documented
calibration-to-pose sequence, diagnostic exits, strict handoff consistency,
source/report/link protection and input budgets. Existing inline sheet-pose tests
are retained. These do not qualify a production lens or real-person capture.
