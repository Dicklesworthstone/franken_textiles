# Joint metric camera and landmark refinement

`ftx measure bundle-adjust` jointly refines camera poses and stationary landmarks from
calibrated images. Unlike `measure triangulate`, the camera extrinsics are variables, not
assumed exact. Known metric control points anchor every camera to one common reference.
Intrinsics and lens distortion remain fixed; there is no guessed body scale.

Run the exact synthetic example from the repository root:

```sh
cargo run -p ftx-cli -- measure bundle-adjust docs/examples/bundle.json --json
cargo run -p ftx-cli -- measure bundle-adjust docs/examples/bundle.json --out bundle-report.json
```

The example starts with perturbed camera centers and a perturbed free landmark. It recovers
camera centers `[-0.5,-2,1]`, `[0,-2,1]`, `[0.5,-2,1]` and the off-sheet landmark `[0,-0.4,1.2]`
in metres. Four sheet controls per camera and the first two landmark observations are fitted;
the center control in all three images and the landmark in the third image are held out.
The real-binary regression executes this exact example, including failure and overwrite cases.

## Inputs and gauge

`ftx-bundle-input/1` has `metric_scale_source`, `capture`, `views`, `controls`, `landmarks` and
`observations`. Unknown fields are errors. The example is a complete input, not a pseudocode
schema. All positions are **metres, z up**. Controls are fixed world positions; landmark
positions and cameras are initial guesses for a local solve, not extra observations or priors.
Each point has a globally unique `id` and `position_m`. Control positions must also be distinct.

Each view uses the shared camera representation from `sheet-pose` and `triangulate`:
`frame: {stream, timestamp_us}` and `camera: {intrinsics, world_to_camera, center_m}`.
The matrix obeys `X_camera = R * (X_world - center_m)`; camera coordinates are right/down/forward.
Pixels must already match the full-resolution oriented camera calibration. This command does
not apply EXIF orientation, rescale intrinsics, decode video or load an image named in metadata.

Each observation names `frame` and `point`, supplies `pixel_px`, `sigma_px`, and declares
`partition: fit|held_out`. Sigma is a positive per-coordinate pixel noise weight in
`[1e-6,1e6]`; it is not a metric accuracy claim. Duplicate frame/point observations are errors.
Choose partitions before fitting. Never use the same click as both fit and held-out evidence.

Fixed controls remove translation, rotation and scale freedom. At least four well-spread
fit controls must be visible in **each** camera. They may lie on a sheet or a calibrated 3D
reference. Four controls alone do not prove observability: collinear or badly conditioned
geometry is rejected numerically. All controls must represent the **same stationary reference
and origin** across frames. A moving sheet or an incorrectly sized print can invalidate the
metric result even if pixel residuals are small. Reference uncertainty is not optimized here.

Use initial poses from `sheet-pose` and initial landmark guesses from a calibrated triangulation
or other stated estimate. The initial landmark projections must be valid. This is a local
refinement, not global camera-pose initialization; an arbitrary starting pose can be refused.

## Motion, fitting and acceptance

The shared `capture` contract requires a reviewed `still_window` in one decoded stream or
`synchronized` frames with caller-aligned clocks and a declared maximum exposure skew.
For this solver, the evidence must apply to **every landmark in the bundle**, not merely the
reference sheet. A moving child across unrelated frames is not a stationary bundle. The
timestamp and synchronization checks verify the declaration's consistency, not its truth.

Only fit observations enter the sigma-weighted pixel objective, derivatives, rank checks and
stopping criterion. The solver uses analytic pose/landmark derivatives and column-scaled
Levenberg–Marquardt in a normalized metric frame. Accepted steps strictly reduce the objective.
It tests the **undamped** normalized Hessian (Cholesky pivot floor `1e-10`) before allowing
damping; this conservative numerical gate is not a covariance or global-observability proof.

Free landmarks need at least two fit views with 3 degrees of parallax initially and after
refinement. All fit **and held-out** observations must project in front of the camera, within
the calibrated lens domain, and within **3 pixels**, regardless of sigma. No outlier is trimmed
or relabeled. A missing held-out partition is explicitly advisory, not an independent check.

The report includes canonical input, per-observation residuals and the initial/accepted-step
objective history. `solution` contains refined cameras and landmarks only after every gate
passes. Initial guesses remain in `input` on failure; they are **not** an accepted solution.
Existing reports and source evidence are never overwritten, including through file aliases.

Exit codes: **0** accepted; **2** rejected, unobservable or not converged; **3** invalid input
or output refusal; **4** iteration budget exhausted. Diagnostic failure reports can be saved
with `--out`, but have `solution: null`. Default budget is 50 accepted steps; `--iterations`
accepts 1–100. Hard limits are 2–8 cameras, 4–128 controls, 1–32 free landmarks and 2048
observations. Requests exceeding those limits are refused, not truncated.

## What remains outside this implementation

This is an anchored, calibrated stationary-scene slice of FTX-20. It does not estimate lens
parameters, reference scale/uncertainty, camera covariance or the global minimum. It is not
the FTX-21 shared-shape/per-frame-pose body fit and emits no fused tape/photo measurement,
measurement sigma, READY status or real-wearer fit claim. Actual stillness detection and
multi-frame body fitting remain separate work.

The objective follows the standard bundle-adjustment formulation described in the
[Ceres nonlinear least-squares tutorial](https://ceres-solver.org/nnls_tutorial.html#bundle-adjustment);
this implementation uses the existing Rust camera model and no Ceres dependency.
