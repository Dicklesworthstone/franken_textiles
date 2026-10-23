# Calibrate a shared camera from several sheet views

## Conditional intrinsic uncertainty

The shared `calibrate_camera_with_uncertainty` API is exposed by both coordinate and image
calibration with `--corner-noise noise.json`. The separate noise file uses:

```json
{
  "schema": "ftx-calibration-corner-noise/1",
  "sigma_px": 0.2,
  "evidence": "Replace with the source of the per-coordinate sigma",
  "independence_evidence": "Replace with the basis for independent fitting coordinates and images"
}
```

These numbers describe example assumptions, not measured OpenCV or camera performance.
The saved report retains `corner_noise_input` and `conditional_uncertainty`; an unavailable
estimate returns exit 2 without inventing zero precision. The noise model does not change
the fitted geometry. Checked photo reuse recomputes the estimate and verifies the entire
conditional report, including the presence of a result for the retained noise request.

```bash
ftx measure calibrate-camera --images capture/views.json --sheet sheet/calib-sheet-letter.json \
  --corner-noise noise.json --out camera.json --json
```

The inline option below is a convenience for self-contained manifests/coordinate documents.
It calls the **same published joint covariance estimator**, not a second numerical algorithm.
Do not combine the separate file/API argument with inline noise; conflicting sources refuse.

Brown calibration may additionally declare `distortion_fit.corner_noise` with
`model: "independent_isotropic"`, a positive `sigma_px` per observed pixel coordinate,
and nonempty `evidence` for that noise model. This is an explicit statistical assumption,
not an estimate from the calibration residuals. Correlated ChArUco detection errors,
reference-size/flatness error, camera drift and model mismatch are not represented.

After convergence and every existing held-out gate, compute the undamped fit-only
Gauss–Newton covariance `sigma_px² (JᵀJ)⁻¹`, then take its nine-intrinsic marginal block.
The full Jacobian includes every fitting view's six pose parameters; poses must not be
treated as known when reporting intrinsic uncertainty. Convert optimizer coordinates
(log focal lengths, scaled principal point) to `[fx, fy, cx, cy, k1, k2, k3, p1, p2]`.
Publish the full covariance, not just its diagonal, with the assumptions and fit counts.
No held-out observations, damping, variance floors or residual-derived noise rescaling
are permitted in this calculation. Invalid noise and singular/unstable covariance refuse;
rejected fits publish no covariance. Omitting noise adds no uncertainty fields; requesting
it never changes the fitted lens, objective history, or held-out tests.

For example, inside `distortion_fit` (the numbers below are assumptions for an example,
not a characterization of OpenCV or a real camera):

```json
"corner_noise": {
  "model": "independent_isotropic",
  "sigma_px": 0.2,
  "evidence": "Replace with the source of the per-coordinate sigma and independence assumption"
}
```

The inline path's optional `uncertainty` binds the full covariance to the exact fitted
lens. Consumers recompute and compare it, including its presence: deleting, inventing,
or modifying covariance does not silently turn the lens into an exact calibration.
Photo reuse requires `--independent-calibration-evidence TEXT`; see the photo guide.

The optimizer also recognizes projection-roundoff convergence: a stalled update is accepted
only when its remaining **undamped** Gauss–Newton decrement is below the estimated
residual-dot-projection arithmetic floor. No increasing update is taken, no rank check is
relaxed, and every held-out and three-pixel reprojection gate still applies. This avoids
refusing tiny-noise held-out pose fits that cannot make representable progress. The report
distinguishes this termination from ordinary gradient convergence.

This local linear approximation is not a confidence certificate or empirical camera
qualification. The mathematical convention follows Ceres Solver's *Covariance Estimation*
documentation (full-Jacobian marginal blocks and explicitly scaled observation noise):
https://ceres-solver.readthedocs.io/latest/nnls_covariance.html . No Ceres dependency is used.

## Native Brown lens fitting

Both coordinate input and the image manifest support `image_model: "brown5_zero_skew"`, estimating
`fx, fy, cx, cy, k1, k2, k3, p1, p2` jointly with each fitting view's rigid pose.
Omit `pinhole_evidence` for raw Brown input and add:

```json
"distortion_fit": {
  "model_evidence": "Replace with evidence for unchanged lens/focus/zoom and Brown image geometry",
  "max_radius": 0.9,
  "max_iterations": 80
}
```

The radius above is illustrative, not a calibration for a real camera. It is a fixed
**undistorted normalized-coordinate** disk, not pixels. Every optimizer step and published
lens must pass the shared camera model's conservative non-folding certificate throughout
this disk; observations and subsequent measurements outside it refuse. Mathematical
invertibility is not empirical validation of unsampled image regions. The report therefore
retains the minimum/maximum fit-point radius; choose the disk using capture coverage.

The implementation follows Zhang's algebraic initialization plus nonlinear pixel-error
refinement, using the existing Brown projection rather than an external optimizer. It fits
log focal lengths, scaled principal point, the five coefficients, and normalized rigid poses
with analytic derivatives, column-scaled Levenberg–Marquardt and strictly decreasing objective.
The undamped normalized Hessian must pass a 1e-10 Cholesky-pivot floor before damping: damping
cannot make missing calibration information observable. Four fit views and one independent
held-out view are required, with twelve fit and four held-out corners in every view. An
invalid/ill-conditioned algebraic start refuses rather than inventing a focal-length prior.

Only fit corners of fit views determine the shared camera, convergence and objective history.
Held-out views fit only their own pose after the lens is frozen, still without using their
held-out corners. Every original corner must then project within 3 pixels. No RANSAC trimming,
silent view dropping or fit-dependent redetection occurs. Nonconvergence, a budget stop or a
failed held-out gate publishes diagnostics with no usable solution. `max_iterations` is 1–200,
default 80, with at most 16 damping trials per update.

The unchanged `ftx-camera-calibration/1` report includes `refinement` for this opt-in path.
It retains the fit-only objective history, accepted update count, scaled-gradient norm,
rank floor, fit-radius range and termination reason. The existing `--lens` and
`sheet-pose --calibration` consumers recompute the Brown fit before using its intrinsics.
Inputs without `distortion_fit` retain the original pinhole algorithm and serialized fields.

This is a local least-squares solution, not a global-optimum or calibration-uncertainty
certificate. Conditional covariance is available only under the explicit noise assumptions
above. Correlated detector noise, reference flatness and print error remain outside
the statistical model; no real-camera or real-wearer qualification follows from synthetic
tests. Rational/fisheye/prism/rolling-shutter lens models are not supported.

Mathematical sources: Zhang, *A Flexible New Technique for Camera Calibration*, MSR-TR-98-71,
sections 2–3; OpenCV's official camera-calibration documentation, Brown radial/tangential
projection. Synthetic tests use independently evaluated projection polynomials, not fitted
output as an oracle.

`ftx measure calibrate-camera` estimates both focal lengths and the principal
point from identified points on a planar reference. It supplies intrinsics for
the existing sheet-pose, triangulation and body-fit camera model instead of
requiring you to guess a focal length. No images or model assets are downloaded.

## Calibrate directly from ChArUco photographs

The native and browser image paths share capture admission and the existing Rust camera solver.
The pure Rust detector gives **all** identified corners to that solver. Acquisition does not run
photo-scale RANSAC, discard inconvenient corners, or re-detect with a fitted camera. It consumes
the sidecar produced by `ftx measure calib-sheet`:

```sh
ftx measure calib-sheet --paper letter --out sheet --json
ftx measure calibrate-camera --images capture/views.json \
  --sheet sheet/calib-sheet-letter.json \
  --detector-python rust --out camera.json --json
```

The explicit `rust` backend needs no Python or OpenCV. The native command retains its existing
default `python3` adapter for compatibility; an explicit Python path also selects that optional
adapter and requires `opencv-python-headless==4.13.0.92`. Installation is never performed by `ftx`.
The browser and coordinate-only command require no Python.
Print at actual size, check both reference rulers, and use a rigid flat board. Photograph
it at different **out-of-plane** tilts without changing the camera settings or image
geometry. Apply orientation before exporting opaque 8-bit RGB/grayscale PNGs without EXIF.

Choose the image model explicitly. `pinhole_zero_skew` needs rectified images or independently
justified negligible distortion. For raw photographs, select `brown5_zero_skew`, omit
`pinhole_evidence`, and add the `distortion_fit` object described above. The same command then
estimates the Brown lens in Rust without passing guessed or fitted intrinsics to acquisition.
A successful residual check alone does not establish the physical validity of either model.

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

Assign both kinds of held-out evidence **before** fitting. Pinhole acquisition requires
at least three fit views and one held-out view, plus at least eight detected fit corners and
four detected held-out corners in **every** image. Brown acquisition instead requires at
least four fit views and twelve fit corners per image, retaining the independent held-out
view and four held-out corners per image. `held_out_corner_ids` refers to physical
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
One image is decoded and released at a time. The optional OpenCV adapter's existing 30-second
detector deadline applies to each view; the Rust detector enforces its own work bounds.
Limits refuse without resizing, truncating a batch or publishing a
partial calibration. New outputs never overwrite earlier evidence or input aliases.

Run the synthetic image-to-calibration-to-pose workflow, including optional OpenCV tests:

```sh
cargo test -p ftx-cli --test camera_image_workflow -- --include-ignored
cargo test -p ftx-measure calibration_images
cargo test -p ftx-wasm --test photo
cargo test -p ftx-measure --test camera_brown
```

## Acquire a camera in the browser

Open the photo-measurement page and choose camera calibration from coordinate observations or
from ChArUco images. Coordinate mode accepts the original `ftx-camera-calibration-input/1` JSON.
Image mode accepts the same `ftx-camera-calibration-images/1` manifest and explicit sheet sidecar
as the native command, plus local files whose names match every manifest `views[].image` exactly.
For nested names, select their directory; paths are resolved relative to that selected root.
The page checks the complete selection before the worker reads images. Files remain on the device.

Both modes accept an optional separate corner-noise document. This is the same conditional Brown
covariance request described above; it cannot silently replace inline noise or make algebraic
pinhole uncertainty available. Original text is retained, so whitespace and ordering remain bound
to the reported source hashes. Image FIT/held-out assignments and corner holdouts are fixed before
any detection. The worker decodes, detects and releases one image at a time and reports progress
in canonical frame order. Cancelling discards that worker; editing source documents withdraws the
generated calibration and any photo result that depended on it.

Download the original source documents and ordinary calibration report for replay. Rejected or
unobservable calibrations retain diagnostics and downloads, but cannot be activated for photo
measurement. An accepted calibration with any requested uncertainty available can be used directly
as the retained camera in the existing ChArUco photo workflow. Declare that measurement photo's
matching lens ID and already-applied rotation, plus independent-acquisition evidence when covariance
is used. Camera input is recomputed and checked on reuse; generated reports do not bypass the
existing covariance, capture-geometry or image-reuse guards.

The file-free Rust APIs are `ftx_wasm::photo::camera_calibration(input, corner_noise)` and
`CameraCalibration::new(manifest, sheet, corner_noise)`. The corresponding browser exports are
`camera_calibration` and `WebCameraCalibration`. The latter exposes `image_ids()`,
`add_image(image, bytes)`, non-consuming `finish()`, and `free()`; empty noise text means no
separate noise document. These adapters delegate acquisition and solving to the shared native
authorities and return the ordinary report, rather than defining another camera format.

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

The default fits **zero-skew pinhole intrinsics**. Those pixels must already be
rectified or have negligible distortion, with evidence for that assumption.
The explicit Brown mode above instead fits radial/tangential distortion from raw pixels.
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
| `image_model` | `pinhole_zero_skew`, or explicit `brown5_zero_skew` |
| `lens_id` | Explicit identifier for the unchanged lens/settings |
| `pinhole_evidence` | Receipt explaining why these pixels follow the pinhole assumption |
| `distortion_fit` | Brown-only model evidence, fixed normalized disk and iteration budget; omit `pinhole_evidence` in this mode |
| `reference_evidence` | Receipt for the physical reference, dimensions and flatness |
| `image_size_px` | Common oriented width and height |
| `rotation_cw_deg` | Rotation **already applied**, one of 0, 90, 180 or 270 |
| `controls` | Distinct `{id, sheet_xz_m: [x,z]}` reference locations in **metres** |
| `views` | `{frame: {stream, timestamp_us}, partition, corners}` observations |

Each corner is `{id, pixel_px: [x,y], partition}`; identifiers refer to `controls`.
Unknown fields and IDs, duplicate identities or locations, and out-of-image
pixels are errors. All views use unique decoded timestamps within one stream.
Pinhole coordinate input needs at least three fit views, each with at least four
noncollinear fit controls. Brown input needs four fit views and a held-out view, each
with twelve fit and four held-out corners. Different reference subsets may be visible.

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
Missing held-out views or corners are advisories for the original pinhole coordinate path;
Brown calibration and image acquisition require independent checks and refuse their absence.

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

The default is an algebraic planar-constraint fit; Brown fitting adds the bounded
pixel-error refinement described above. Both fix skew to zero; conditional Brown uncertainty
is opt-in under the explicit noise assumptions above. The principal point
must lie inside the image; off-image optical centers from some crops are not
supported. The pinhole's normalized radius limit is 2; Brown uses its explicit fixed disk. A small
residual does not establish metric accuracy, body measurements or garment fit.

Limits are 128 controls, 32 views, 128 corners per view and 16 MiB per JSON input.
Budgets refuse rather than truncate. The numerical library and coordinate-only command
use no runtime Python/OpenCV or new dependency; image acquisition uses the optional detector
described above. Pinhole initialization details and the primary formula source are in
`crates/ftx-measure/CAMERA_CALIBRATION.md`.

Ten direct synthetic tests cover known intrinsics/poses, noise, ordering,
degenerate views, missing controls, both held-out partitions, metric scaling,
invalid inputs and nonprojective warps. Five real-CLI tests cover the documented
calibration-to-pose sequence, diagnostic exits, strict handoff consistency,
source/report/link protection and input budgets. Existing inline sheet-pose tests
are retained. These do not qualify a production lens or real-person capture.
