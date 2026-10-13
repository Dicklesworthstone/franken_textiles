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
or embedded, and no silhouette or occlusion agreement is inferred from these plots.

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
its objective history. A failed held-out silhouette prevents mesh publication.
The SVG page still displays keypoints only; it is not a full-mask validation image.

Export is limited to 100,000 vertices, 200,000 faces, eight accepted poses and 256 MiB
of total output. It performs one rest-body evaluation and one full pose evaluation per
accepted frame, recorded as `export_pose_evaluations`; these bounded output evaluations
are separate from the fitter's `--max-evaluations` callback budget. No additional input
file or photo is opened from metadata. Model-estimated geometry remains `MODEL_FIT`.