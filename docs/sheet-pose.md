# A metric camera from a stationary reference sheet

`ftx measure sheet-pose` turns **known camera intrinsics** and marked sheet coordinates into
one metric camera for 3D reconstruction. It is different from `photo-scale`: that command maps
pixels onto a plane; this one estimates where the camera is relative to the plane, allowing
rays to intersect at landmarks **away from the sheet**.

```sh
ftx measure sheet-pose --reference sheet.json --out camera-report.json --json
```

The output `camera` uses the shared `ftx_measure::camera::Camera` representation, ready for the
`camera` field of a triangulation observation. The report is not itself a camera: retain the
complete report as the calibration/metric-scale receipt, and use its `camera` only when `ok`
is true. A failed held-out check produces exit 3, diagnostics and `camera: null`. Malformed or
unobservable calibration is a typed refusal. Existing output files are never overwritten.

## Synthetic example

Save the following as `sheet.json`. It describes an exact synthetic camera at `(0,-2,1)` metres,
looking along world +y, with an A4-sized rectangle in the world plane y=0.

```json
{
  "schema": "ftx-sheet-pose-input/1",
  "reference_id": "synthetic-stationary-a4",
  "image_size_px": [1280, 1200],
  "rotation_applied_degrees": 0,
  "intrinsics": {
    "focal_px": [1000, 1000],
    "principal_px": [640, 480]
  },
  "points": [
    {"sheet_xz_m": [0, 0], "pixel_px": [640, 980]},
    {"sheet_xz_m": [0.21, 0], "pixel_px": [745, 980]},
    {"sheet_xz_m": [0.21, 0.297], "pixel_px": [745, 831.5]},
    {"sheet_xz_m": [0, 0.297], "pixel_px": [640, 831.5]},
    {"sheet_xz_m": [0.105, 0.1485], "pixel_px": [692.5, 905.75], "held_out": true}
  ]
}
```

`sheet_xz_m` is in **metres**, not pattern millimetres. Its two coordinates mean world `(x,0,z)`,
with x right and z up. `pixel_px` uses x right/y down in the full-resolution, already oriented
image. `rotation_applied_degrees` must be 0, 90, 180 or 270 and records rotation **already applied**
by the caller to both pixels and calibration; the command does not rotate coordinates or read EXIF.
An optional `image` string is retained as a label; no file or URL is opened from it.

Lens distortion is optional; omission means the shared camera's undistorted pinhole model.
For distorted coordinates, supply `intrinsics.distortion` with `radial: [k1,k2,k3]`,
`tangential: [p1,p2]` and `max_radius` (the calibrated undistorted normalized radius). This is
Brown–Conrady distortion, not a fisheye/rational model. Never copy a calibration from a different
crop, image size, orientation or lens/zoom setting without correctly transforming/recalibrating it.

## Gates and limitations

At least four distinct, well-spread **fit** points are required, with at most 512 total points.
`held_out` defaults to false. Declare held-out points before fitting; they are checked afterward
and cannot steer the solution. Duplicate sheet or pixel coordinates, off-image pixels, unknown
fields, singular geometry, a non-rigid homography, or reprojection errors above 3 px fail closed.
Without held-out points, the result carries an explicit advisory rather than implying an
independent check. All per-point residuals and the complete input are retained.

Keep the **same sheet stationary** for every camera/frame used together. Moving or redefining
the sheet origin between images breaks the common world frame even when each image fits well.
The reference identifier labels this assumption; it cannot prove that the physical sheet stayed
still. Global scale is supplied by the sheet coordinates, not recovered from body-size priors.

This algebraic pose estimate assumes known intrinsics and the caller's correct flat metric
reference. It does not estimate focal length, lens parameters, sheet flatness, calibration or
metric-scale uncertainty. A small pixel residual is not proof of metric accuracy. It does not
detect sheets, decode video, verify orientation, infer stillness, synchronize cameras, perform
bundle adjustment or establish real-wearer fit.

Rust callers can use `Camera::from_sheet` for typed camera recovery,
`Camera::sheet_pose_report` for this JSON boundary, and `Camera::project_with_jacobian` for the
analytic 2-by-3 derivative with respect to metric world position.
