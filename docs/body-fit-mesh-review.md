# Inspect the accepted fitted poses

```sh
ftx measure fit-body request.json --model model.ftxbody \
  --iterations 50 --mesh-out fitted-frames --json
```

`--mesh-out` writes one full OBJ mesh per accepted decoded frame and `fit.json`
inside a new directory. It is an alternative to `--out`, not an additional report
destination. The parent directory must already exist. Existing paths, including
empty directories and dangling links, are refused before model loading.

The fitted shared shape and each accepted root/joint pose are evaluated through the
same Anny transforms used by the sparse fitting path. No new optimization, grounding,
remeshing, pose reset or millimetre rounding occurs. OBJ vertices retain round-trip
f64 metre coordinates in the common calibrated world frame, +Z up; all model faces
and vertex indices are kept, including internal shells. These diagnostic meshes are
**not certified drape collision skins**, watertight bodies or evidence of real fit.

Open `index.html` for a self-contained, offline keypoint review. Orange circles mark
observations, blue crosses mark the corresponding full-mesh projections, and joining
lines show residuals. Labels retain fit/held-out partitions. No photograph is loaded
or embedded. When silhouette evidence was supplied, orange segments show observed
foreground intervals and dashed blue segments show full-mesh coverage at the exact
horizontal/vertical slice coordinates. Gaps are retained, not replaced by a hull.

Each `frame-0000.json` retains the camera, oriented image size, recorded rotation,
original observations/sigmas/partitions, full-mesh world points, projected pixels and
unrounded residuals. The corresponding SVG uses those oriented pixel coordinates;
it does not rotate or undistort them again. Surface correspondences interpolate
posed full-mesh vertices before camera projection, including cross-joint triangles.
The full-mesh projection must independently satisfy the existing 3-pixel gate before
any review file is published.

The `mesh_review` object in `fit.json` maps deterministic filenames to original
stream/timestamp identities, byte counts and SHA-256 hashes for the OBJ, SVG, JSON
and HTML files. The numeric filename is an artifact index, not a video frame ordinal
or inferred frame rate. Display labels are XML-escaped and invalid XML characters
are replaced only in the display; original names remain unchanged in JSON.

Rejected, ambiguous or budget-stopped fits do not create the directory. Their usual
JSON diagnostics remain on stdout. Every mesh is prepared before publication and the
completion report is written last. An I/O failure can leave an incomplete directory
without `fit.json`; it is retained, never silently reused. On Unix, newly created
directories and files have modes 0700 and 0600 respectively.

`--mesh-out` also works with `--silhouettes masks/manifest.json`. The fit uses the
existing silhouette gates and the bundle retains the complete mask-source receipts
and scanline evidence in `fit.json`. Export does not rerun optimization or change
its objective history. The exported full mesh independently passes every supplied
fit and held-out silhouette slice through the existing triangle-union and 3-pixel
boundary gates before publication; a discrepancy refuses the bundle. Coordinate
JSON records original evidence, intervals, contour sigmas, partitions, predicted
coverage, boundary errors and symmetric differences. Empty/background slices are
retained. This remains sampled-line agreement, not full-mask or occlusion validation.

Export is limited to 100,000 vertices, 200,000 faces, eight accepted poses and 256 MiB
of total output. It performs one rest-body evaluation and one full pose evaluation per
accepted frame, recorded as `export_pose_evaluations`; these bounded output evaluations
are separate from the fitter's `--max-evaluations` callback budget. No additional input
file or photo is opened from metadata. Model-estimated geometry remains `MODEL_FIT`.
## Inspect a browser-fitted frame

After an accepted fit on the local photo page, use **Inspect an accepted frame**, choose the
frame and select **Inspect frame and prepare mesh**. This reuses the exact model bytes and
original accepted report from that fit; it evaluates one saved pose without another optimization.

To continue an earlier native or browser fit, select its exact converted model and expand
**Resume a saved accepted fit**. Choose the original `ftx-body-fit/1` report and select
**Open saved fit without refitting**. The same native review authority checks the accepted
shape, model identity, configuration, frame/pose agreement and bounds, then rechecks the first
frame's full-mesh keypoints and supplied silhouette slices before publishing a reusable body.
The first frame opens immediately; choose other frames for their own full-mesh review.
The fitting controls and their current numerical limits are not used to reopen a report.

The result is labelled **Saved report** and keeps the original report bytes, historical attempts
and residuals, exact timestamps and selected model snapshot. Its report download and the
Pattern Workbench handoff preserve those sources. The report retains its numerical policy, but
not the original request bytes or original options download; loading it therefore does not
construct input or options files.
Source photographs and masks are not automatically reread when reopening; a saved accepted fit
retains its original evidence and limitations. Rejected, inconsistent or wrong-model reports refuse,
and cancelling or replacing a pending file read or worker cannot publish an older body.

The drawing overlays orange observations and blue full-mesh projections in the original
oriented pixel coordinates. Labels retain FIT/CHECK roles and unrounded residuals remain in
the JSON. Supplied silhouette scanlines are independently rechecked on the complete mesh
through the same native coverage gate. The projected mesh can be hidden to inspect observations.
For a distorted camera, only projected vertices are drawn; straight triangle edges would
misrepresent lens-warped edges. Vertices outside the camera's projection domain are counted
explicitly and omitted from the drawing, while the OBJ retains the complete geometry.

### Show the original capture photograph

When a frame retains its original capture image identity, **Optional original photograph** shows
the expected filename. Select that original PNG or JPEG and choose **Verify and show original
photograph**. The locally checked photograph appears beneath the observations and fitted
projections, so you can inspect the correspondences against the captured person and scene.

Native capture preparation preserves these image identities in `prepared-input.json`, and both
native and browser fits retain them in their reports. Saved native capture reports can also supply
their original capture receipts. Earlier prepared inputs or reports without either source identity
still support geometry review, but the photograph control stays disabled. Recreate the prepared
input from the original capture to retain the required image identities.

The native image decoder checks the original encoded bytes against the retained SHA-256, along
with the exact frame identity, stored dimensions and recorded rotation. A resaved, cropped or
otherwise different file is refused even if it looks similar. Only the checked PNG preview reaches
the browser drawing. The frame's recorded clockwise rotation has already been applied to pixels,
annotations and calibration; the image is not rotated again. JPEGs that still request a nonidentity
EXIF orientation are refused. The display reports the verified source hash and stored orientation.

**Clear photograph / cancel loading** removes the background or cancels its pending read and
decode. Image refusal or cancellation leaves the accepted fit, frame geometry and their downloads
available. Choosing another photograph immediately removes the previous background. Viewing a
photograph is a visual inspection aid; it neither adds new evidence to the fitter nor changes an
acceptance decision, and no image bytes are embedded in the fit or frame-review JSON.

Download the per-frame **JSON** and **OBJ** for local review. The JSON preserves the original
full-width timestamp, camera, orientation, observations, coverage comparisons and exact report
and model hashes. The OBJ retains unrounded metre coordinates and every original face,
including internal shells. No filename or path is followed automatically. This review
does not establish full-mask agreement, point visibility, occlusion, collision-skin validity
or physical fit.

Changing the selected frame, cancelling a review, changing the fit sources or limits, or
leaving the page withdraws stale review downloads and releases any loaded photograph. Delayed
image reads, worker replies and preview decodes cannot replace a newer selection. Review refusal
or cancellation preserves the accepted fit report and its separate Pattern Workbench handoff. The existing 16 MiB
report, 512 MiB model, 100,000-vertex and 200,000-face bounds apply; optional photographs are
limited to 128 MiB and 40 million stored pixels, with a frame review up to 16 MiB. Only one frame
is evaluated at a time. Keep the native `--mesh-out` route for a complete immutable multi-frame bundle.
