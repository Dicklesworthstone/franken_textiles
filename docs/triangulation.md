# Calibrated multi-view landmark triangulation

This implements the camera and gated-triangulation slice of FTX-20 (plan §10.2).
It is not automatic video measurement, a moving-body fit, or a replacement for tape evidence.

## Run the synthetic known-answer example

From the repository root:

```sh
cargo run -p ftx-cli -- measure triangulate docs/examples/triangulation.json --json
cargo run -p ftx-cli -- measure triangulate docs/examples/triangulation.json --out /tmp/ftx-point.json
```

The example reconstructs `[0, 0, 1.3]` metres in a z-up world. Two views contribute to
fitting and a third is held out. The example's cameras, pixels and timestamps are fictional.
The JSON envelope uses the existing CLI format; `result` contains `ftx-triangulation/1`.
Exit codes: **0** accepted conditional geometry, **2** rejected/unobservable geometry,
**3** invalid input or output refusal, **4** resource-budget refusal. A rejected report has
`point_m: null`, not a
plausible-looking partial estimate. `--out` creates a new file exclusively and refuses to
replace evidence or an earlier report, including through symlink/hard-link aliases.

## Local browser route

Open `web/photo.html` after building and serving the browser bundle. Under **Reconstruct landmarks
from calibrated views**, choose **One landmark with fixed cameras**, select the same prepared input and,
optionally, an `ftx-click-noise/1` document declaring an independent positive pixel sigma for every FIT
frame. Input and click-noise files are limited to 256 KiB each. The separate shared-lens evidence
control accepts `ftx-triangulation-lens-noise/1` or `/2` up to 16 MiB, retaining complete calibration
reports and the exact assignment of FIT frames to shared lenses. The separate sheet-corner control
accepts `ftx-triangulation-sheet-noise/1` up to 16 MiB, retaining the original sheet-pose reports. The existing Rust engines run in a
disposable local worker; no source is uploaded. Download the complete report and exact original
input/noise files for native replay:

```sh
ftx measure triangulate triangulation-input.json --click-noise triangulation-click-noise.json --json
```

Omit `--click-noise` when none was selected; add `--lens-noise triangulation-lens-noise.json` and
`--sheet-noise triangulation-sheet-noise.json` for the corresponding selected evidence. The page
displays each requested component separately and preserves valid geometry when any requested
covariance is unavailable. An older bundle cannot silently ignore selected camera evidence:
rebuild it with `tools/build_web.sh` when the engine reports a missing entry point.
Conditional click uncertainty differentiates the actual
pixel-refined estimator with fixed cameras, scale and capture assumptions. Available covariance
does not include calibration, scale, motion or systematic error. Valid geometry and unavailable
requested covariance remain separately visible; neither becomes a measurement sigma or fused record.
Changing any source, removing any noise input, cancelling or leaving the page withdraws that tool's reports
and downloads. Other photo evidence is independent. Frame timestamps and original numeric text are
preserved in downloads; display summaries do not rewrite the report.

The standard `tools/check_all_web.sh` runner captures the synthetic shared-lens native results and
compares both lens-only and click-plus-lens reports with the actual compiled WASM export. Its
`FTX_TRIANGULATION_LENS_FIXTURES` capture is separate from other photo fixtures. The corresponding
`FTX_TRIANGULATION_CAMERA_FIXTURES` capture exercises sheet-only, sheet-plus-click, sheet-plus-lens
and all-three requests through the four-input export; `tools/check_photo_web.cjs --camera-noise DIR`
replays those native fixtures in Chromium, including exact source downloads. DOM/worker regressions
also exercise original-byte replay, unavailable covariance, source-size limits, cancellation during
file reads, source removal, operation changes and stale bundles. These are numerical and workflow
checks, not physical camera qualification.

For `docs/examples/triangulation.json`, this optional noise file uses a synthetic one-pixel
assumption. A real acquisition needs its own supported click evidence; the held-out view is not
included among the noise inputs:

```json
{"schema":"ftx-click-noise/1","evidence":"Synthetic one-pixel assumption; not camera accuracy","observations":[{"frame":{"stream":"synthetic-left","timestamp_us":1000000},"sigma_px":1},{"frame":{"stream":"synthetic-right","timestamp_us":1000000},"sigma_px":1}]}
```

## Shared lens calibration uncertainty

The optional `--lens-noise` input carries the full nine-parameter Brown-lens covariance from
saved camera calibrations through the same pixel-refined triangulation estimator:

```sh
ftx measure triangulate triangulation-input.json --lens-noise triangulation-lens-noise.json --json
ftx measure triangulate triangulation-input.json --click-noise triangulation-click-noise.json --lens-noise triangulation-lens-noise.json --out triangulation-result.json
```

Use `ftx-triangulation-lens-noise/1` for fixed supplied poses, with these fields:

| Field | Required content |
|---|---|
| `schema` | `ftx-triangulation-lens-noise/1` |
| `independence_evidence` | The basis for treating calibration acquisitions and measurement clicks as independent, and different lens groups as independent |
| `groups` | One to eight groups, covering every FIT frame exactly once |
| `groups[].key` | A unique name for this shared lens group |
| `groups[].calibration` | The complete saved `ftx-camera-calibration/1` report from an accepted Brown-lens calibration with available intrinsic covariance; use the report itself, not its CLI envelope |
| `groups[].frames` | The exact `(stream, timestamp_us)` identities of the FIT observations using that lens |

Retain one group for all frames using the same calibrated lens. Group order and frame order
are canonicalized. Every camera in the group must match the replayed lens exactly, including
its certified radius; rescaled or approximately matching intrinsics refuse. The calibration
report is recomputed from its retained source observations and declared corner noise. Edited
covariance, unavailable calibration uncertainty, reused lens identities, overlapping fitting
acquisitions and CHECK/unknown/missing frame assignments refuse. The complete lens-noise
document is limited to 16 MiB. Independence evidence is a caller assertion, not proof that
detector errors or physical acquisitions are independent.

Each covariance mode perturbs all cameras in its lens group together. This retains correlations
between focal lengths, principal point and distortion, and the dependence shared by views using
one lens. Checked central differences at two step sizes rerun the actual FIT-only estimator;
the report retains group contributions, mode sensitivities and canonical source identities.
The accepted point and all original geometry checks stay unchanged.

`conditional_lens_uncertainty` reports AVAILABLE with covariance in square metres and marginal
one-sigma values in metres, or UNAVAILABLE with null covariance and explanatory flags. Requested
but unavailable uncertainty gives CLI exit 2 while preserving usable geometry. Invalid evidence
gives exit 3 without writing output. Supplying both flags retains the original click component
and the new lens component separately; no total uncertainty field is inferred.

Version `/1` is **conditional intrinsic-lens precision with camera centers and rotations held
fixed**. Use the explicit `/2` source-pose mode below when the camera poses came from the same lens.
Neither mode establishes real-camera accuracy, a body measurement sigma or physical fit.

## Lens-dependent sheet poses

Use `ftx-triangulation-lens-noise/2` with the same lens-group fields and a required `sheet_poses`
array. Each entry has `frame` (the exact FIT frame identity) and `report` (the complete accepted
`ftx-sheet-pose/1` report, including its original input). Version `/1` refuses this additional
evidence so its fixed-pose meaning, serialized input and source identity remain unchanged.

Every original sheet report is replayed through the ordinary sheet-pose authority. It must reproduce
the exact assigned measurement camera, including its intrinsics, rotation and metric center. The
reports must cover every FIT frame exactly once, share one stationary `reference_id`, and retain the
assigned calibration's exact oriented image size and rotation. CHECK and unknown frame assignments,
changed reports, incomplete sources and merely approximate camera matches refuse. Native
`sheet-pose --calibration ... --lens-id ...` annotations are admitted only after recomputing their
retained calibration input and checking the lens/settings identity. For lens `/2`, that complete
calibration input must also match the assigned lens group's acquisition; equal numeric intrinsics
alone cannot transfer covariance from a different acquisition. Their byte hashes remain
provenance for separately retained source files; a hash alone does not authenticate absent bytes.

For every correlated lens-covariance mode, all assigned camera poses are refitted from their original
FIT sheet observations with the perturbed lens. The same landmark estimator then runs with those
poses. The original sheet coordinates are used for fitting before the recovered camera is normalized;
changing the sheet origin before a noisy fit would change the estimator being differentiated.
Held-out sheet and landmark observations remain independent baseline checks and never steer these
derivatives. The accepted landmark and its original geometry report remain unchanged.

The result uses `ftx-triangulation-lens-uncertainty/2` and retains the complete source reports and
mode sensitivities. It includes the lens-induced pose response, conditional on exact sheet corners
and metric reference coordinates. It does not include independent corner detection noise,
print/flatness error, scale error, drift, synchronization, motion or correspondence error.

## Independent sheet-corner precision

The optional `--sheet-noise` document describes original corner observations for the same stationary
reference. It uses these fields:

| Field | Required content |
|---|---|
| `schema` | `ftx-triangulation-sheet-noise/1` |
| `independence_evidence` | Source/method and basis for independent isotropic fitted-corner errors, independent frames, and independence from lens calibration and measurement clicks |
| `observations` | Exactly one entry for every FIT measurement frame |
| `observations[].frame` | The exact `(stream, timestamp_us)` identity |
| `observations[].report` | Complete accepted original `ftx-sheet-pose/1` report, not its CLI envelope |
| `observations[].sigma_px` | Declared one-sigma x/y corner error in oriented pixels, finite and in `(0, 1000]` |

The complete document is limited to 16 MiB. Across all supplied sheet reports, at most 256 FIT corners
and 1024 total retained corners are admitted. The original report must replay exactly, including its
held-out checks. No corner sigma is inferred from small fitted residuals.

Each original FIT corner coordinate is perturbed at two checked differential steps. The ordinary
camera-pose and landmark estimators run again with fixed intrinsics and metric sheet coordinates.
`conditional_sheet_uncertainty` retains the full world covariance in square metres, marginal sigma
in metres, original FIT point indices, each corner's 3-by-2 sensitivity and each frame's contribution.
Canonical parsed-source hashes are distinct from original file-byte identities.

Unstable derivatives, differential geometry failures or unrepresentable covariance produce
UNAVAILABLE, null covariance and no partial sensitivities. The original accepted geometry is still
retained. Requested unavailable precision gives CLI exit 2; invalid source/noise evidence gives exit 3
without creating output. Resource-budget refusals retain the CLI's existing exit 4; lens and sheet
file-size limits are checked before JSON parsing. Output remains create-only, including source aliases.

All three components can be requested together:

```sh
ftx measure triangulate triangulation-input.json \
  --click-noise triangulation-click-noise.json \
  --lens-noise triangulation-lens-noise.json \
  --sheet-noise triangulation-sheet-noise.json --out triangulation-result.json
```

When source-pose lens `/2` and sheet noise are combined, their complete sheet reports must match
exactly for every FIT frame. Two different acquisitions that happen to fit the same camera are not
interchangeable. Click, lens and sheet components remain separate; no total covariance, fused body
record or physical accuracy claim is inferred. Correlated detector errors, reference manufacturing
error and real capture motion require further evidence.

## Input contract

`ftx-triangulation-input/1` describes one landmark with 1–128 observations. At least two
**fit** views are required to reconstruct it. Unknown fields and enum variants are refused.
Each observation supplies a decoded frame identity, camera, oriented pixel and explicit
`fit` or `held_out` partition. Frame identity is `(stream, timestamp_us)`, not frame number.
Timestamps are nonnegative microseconds after decoding and any required clock alignment.
Duplicate identities are refused; view order cannot change the reduction order or report.

Camera centers are **metres** in one shared, right-handed, z-up world. A nonempty
`metric_scale_source` explains or identifies the calibration receipt establishing that
scale/frame. The camera API does not verify that the receipt is true.
`world_to_camera.rows` is a rotation matrix, not an arbitrary projection matrix:

```
X_camera = world_to_camera * (X_world - center_m)
```

Camera coordinates are x right, y down, z forward. Intrinsics and pixel coordinates must
refer to the same EXIF-oriented image and resolution; this command does not orient images,
read EXIF, detect lens switches, or rescale intrinsics. Scaled, reflected or non-orthonormal
rotations and nonpositive focal lengths are errors. Do not pass the extrinsic translation
vector as `center_m`: for a conventional `[R|t]`, the center is `-R^T t`.

Optional `intrinsics.distortion` has `radial: [k1,k2,k3]`,
`tangential: [p1,p2]`, and `max_radius` in undistorted normalized camera coordinates.
Omitting it means an undistorted pinhole on radius ≤2. This is the polynomial Brown model,
not a fisheye/rational/thin-prism model. Its equations correspond to the polynomial subset
of [OpenCV's calibration model](https://docs.opencv.org/4.x/d9/d0c/group__calib3d.html).
No OpenCV implementation or library dependency is used.

The inverse is allowed only when a conservative lower bound certifies a positive-definite
lens Jacobian throughout the declared disk. This is a sufficient, not necessary, test:
a valid lens can be refused on an overly large domain. Use the genuinely calibrated domain,
not a smaller fabricated radius merely to get a pass. Bounded Newton inversion must decrease
the residual and stay in the disk; failure or extrapolation produces a refusal, never an
arbitrary inverse branch. This certificate concerns the mathematical map, not calibration accuracy.

The shared Rust API also supports `Camera::from_sheet(intrinsics, sheet_xz_m, pixels)` for
metric pose initialization from 4–512 correspondences on the **same stationary sheet** in
all views. The common sheet frame is world `(x, 0, z)`, in metres; known intrinsics are still
required. Do not independently reorient the sheet or use separately moving references as
one world frame. `project_with_jacobian` provides analytic world-position derivatives for
later fitting. A synthetic integration test recovers an off-sheet landmark after deriving
both camera poses from this common sheet.

## Motion assumptions must be explicit

Choose one capture declaration:

* `still_window`: `start_us`, `end_us`, and nonempty `evidence`. All observations must belong
  to one stream and lie within those inclusive bounds. The evidence must establish that
  this landmark is stationary throughout the interval; the interval alone cannot do so.
* `synchronized`: `max_skew_us` and nonempty `evidence`. Exactly one frame per stream is
  allowed. The maximum-minus-minimum timestamp must not exceed the declared skew. These
  timestamps must already be aligned to a common clock. The caller is responsible for a
  skew small enough for the subject's motion, exposure duration and rolling-shutter behavior.

Evidence is a caller assertion/receipt, not a stillness or clock-synchronization detector.
A moving child across unrelated frames must not be described as a static triangulation.
Still-window detection, decoding, intrinsic calibration, bundle refinement and multi-frame body
fitting use their separate native workflows; this tool does not execute those stages automatically.

## Geometry gates and interpretation

Only fit rays enter the initial perpendicular-ray-distance least-squares solve. The
initialization uses the existing deterministic Jacobi SVD in a translated/baseline-normalized world.
Both observed fit rays and reconstructed fit rays must have a pair with at least **3° acute
parallax**, from distinct camera centers. Opposed rays are not treated as a good baseline.
Coincident centers, parallel rays and insufficient fitting views are unobservable.

The point is then refined against **equal-weight squared pixel residuals in fit views only**,
with analytic camera derivatives and at most 30 Gauss–Newton steps. Each accepted step must
strictly reduce the objective while remaining inside every fit camera's valid projection
domain. The fixed cameras and metric scale are never optimized. A column-normalized gradient
or near-zero pixel residual must establish convergence; a tiny backtracking step alone cannot.
Nonconvergence publishes no usable point. The report's `refinement` retains initial/final sums
of squared pixel residuals, accepted-step count and convergence status, or is null if the
geometry/projection could not be refined. These are optimization diagnostics, not uncertainties.
This corrects the ray-distance initializer's objective when focal lengths or depths differ;
it is point refinement, not joint camera/landmark bundle adjustment or a global-minimum guarantee.

The candidate must project in front of every camera, remain inside every declared lens
domain, and have **≤3 pixels reprojection error in every view**, including held-out views.
No outlier is silently removed. Held-out views neither move the fitted point nor rescue
weak fitting geometry. With no held-out observations, the report explicitly says its
reprojection check is fitted-only. Inputs, partitions and per-view errors are retained.

A small reprojection residual does not prove an accurate metric point: incorrect but
self-consistent scale, calibration or timing can still pass. Geometry alone supplies no covariance;
optional click-only covariance excludes camera and metric-scale uncertainty. No garment-fit claim or tape/photo measurement
record is generated for fusion. In particular, do not use the pixel residual as a measurement
sigma. The report is geometry evidence for later validated fitting, not READY status.
