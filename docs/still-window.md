# Propose a reviewed still window from calibrated video landmarks

`ftx measure still-window` fills the stationarity gap between decoded-frame selection and
single-landmark triangulation. It uses calibrated cameras plus the same named sparse landmarks in
four or more frames from one stream. It does **not** silently turn a numerical check into the
reviewed stillness assertion required by `ftx measure triangulate`.

```sh
ftx measure still-window still-input.json --out still-report.json --json
```

A `STILL_CANDIDATE` result (exit 0) means every checked landmark passed three geometric checks:

1. the first half of the interval triangulates with at least 3 degrees of parallax and at most
   3 px reprojection error;
2. the second half independently passes the same checks;
3. a fit over the complete interval also stays within 3 px, and the independent early/late 3D
   points differ by no more than the caller-declared `max_motion_mm`.

`REJECTED` or `UNOBSERVABLE` returns exit 2 and publishes diagnostics but no claim of stillness.
Malformed inputs return exit 3; the JSON input is capped at 16 MiB and budget overflow returns 4.
Output is create-only and cannot replace the source or an earlier report.

## Local browser route

Build and serve the ordinary browser bundle, open `web/photo.html`, and choose **Assess a sparse
still-window candidate** under **Reconstruct landmarks from calibrated views**. Select the same
prepared `ftx-still-window-input/1` document, limited to 256 KiB in this browser route. The existing
native assessor runs in the local disposable worker. No media decoder, tracker or camera fit is
inferred from the selected document.

Review each FIT/held-out landmark's early/late displacement, failed geometric checks and the complete
native report, including the selected filename and exact input-byte hash. Download `still-window.json`
and the exact `still-window-input.json` for the native command above; the replay filename is reported
as supplied, so renaming the source changes that field alone. Mode/source changes or cancellation
withdraw the old result and downloads. Separate triangulation click-noise input is cleared, and the refinement iteration limit is inactive; the
stationarity threshold comes only from the original input. Other photo evidence stays independent.
All processing stays local; no source is uploaded or stored by the page. Even `STILL_CANDIDATE`
requires human review before constructing any explicit triangulation capture assertion.

## Input

The schema is `ftx-still-window-input/1`:

```json
{
  "schema": "ftx-still-window-input/1",
  "metric_scale_source": "stationary metric reference shared by every camera",
  "observation_evidence": "decode/frame-selection/correspondence receipt",
  "max_motion_mm": 5.0,
  "frames": [
    {
      "frame": {"stream": "clip", "timestamp_us": 100000},
      "camera": {"...": "the shared ftx camera schema"},
      "landmarks": [
        {"name": "marked_a", "pixel_px": [640, 480], "partition": "fit"},
        {"name": "marked_holdout", "pixel_px": [700, 510], "partition": "held_out"}
      ]
    }
  ]
}
```

There must be 4–64 frames, all with one stream and unique decoded timestamps. Every frame must
contain the same 1–128 landmark names and partitions; at least three landmarks are `fit`.
Coordinates are in the already-oriented image used by each calibrated camera. Cameras must be in
one stationary metric world frame; moving-sheet calibration poses cannot be combined as though
they were a common rig.

`max_motion_mm` is required, finite, positive and at most 100 mm. It is an acquisition/review
choice, **not** a calibrated measurement-accuracy constant. Fit landmarks define the candidate set;
held-out landmarks never rescue it and independently reject it when they move. With no held-out
landmarks the report says so explicitly.

## What the report does not prove

Sparse tracked landmarks can stay still while an untracked body region moves. Correspondences can be
wrong. Rolling shutter, lens/calibration error and reference motion can produce misleading geometry.
The early/late split is deterministic by timestamp count, not a statistical change-point detector.
Therefore `STILL_CANDIDATE` is deliberately not a `triangulate::Capture::StillWindow` receipt.

Review the relevant body region and acquisition evidence before constructing the explicit
`still_window` capture assertion used by triangulation. This command is a strong rejection/filtering
step, not a replacement for that review and not a body-fit or real-wearer accuracy claim.
