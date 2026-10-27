# Shared Anny shape across changing poses

`ftx measure fit-body` fits one Anny shape to named 2D correspondences across independently
posed images, optionally constrained by independent tape measurements. Unlike triangulation
or a stationary bundle, it does not assume the body stayed still between decoded frames.
Optional sampled silhouettes constrain surface shape in addition to keypoints and tape.
This is not full-mask fitting or FTX-21's real-photo acceptance gate.

```sh
ftx measure fit-body observations.json --model /path/to/anny-0.6.1-default.ftxbody \
    --out body-fit-report.json --iterations 50 --json
```

The model is the user's locally converted Anny 0.6.1 file. Nothing downloads a model, opens a
photo URL, runs an external detector or uploads data. `model.model_sha256` must equal the
SHA-256 of the exact converted file. The reader also verifies the file's individual array
hashes. Keep private photos, measurements and fitted bodies outside the repository.

Before running the model fit on a reviewed capture input, inspect its actual images and named
points locally:

```sh
ftx measure capture-review capture/input.json --out capture-review-001 --json
# Open capture-review-001/index.html and review every FIT/CHECK point and model target.
ftx measure fit-body capture-review-001/capture.json --model /path/to/anny.ftxbody \
    --mesh-out fitted-review-001 --json
```

The first command needs no body model. It reruns selection and camera checks, copies the exact
selected images, and overlays the supplied points at stored image coordinates without another
rotation or rectification. The table shows every name, FIT/CHECK assignment, pixel, sigma and
authored model target. Supplied silhouettes are shown as observed scanlines after the
existing rectified-camera and interval checks. Imported mask constraints remain read-only;
inline constraints can be authored or corrected as described below. Dashed lines show sampled slices; thick segments
show foreground, and empty slices explicitly assert background. Every slice retains its original
FIT/CHECK role, axis, position, sigma and evidence. This does not validate the segmentation,
occluders or clothing against a body.
Before body annotation, expand **Review stationary sheet and camera** on every frame. Purple
circles show the original sheet observations; green crosses show the checked camera's projections.
The list retains each metric (x, z) position, observed/projected pixel, FIT/CHECK role and residual,
along with reference/lens identifiers and calibration/reference hashes. Verify that the points
actually identify the same stationary reference. Small residuals cannot establish flatness,
physical print scale or lens accuracy. This is read-only: correct the original reference input
before generating another review, preserving the predeclared held-out roles. No detector or
camera refitting runs in the page. The sheet SVG and script-disabled review remain available.

To correct a keypoint, select it in its table, then click that image or enter full-resolution
X/Y coordinates and choose **Apply coordinates**. The image transform accounts for display size,
scroll and zoom; numeric entry retains the supplied precision. Invalid/out-of-image values refuse.
Existing observations allow coordinates only: their names, sigmas and FIT/CHECK assignments stay
fixed. To start from an empty frame or add a missing observation, use **Place a new observation**:
choose the frame and an unused name already declared in `model.keypoints`, explicitly choose FIT
or CHECK and a sigma in `1e-6..1e6` pixels, then **Prepare new point**. Click that frame or apply
numeric coordinates. The target definition is shown for review; no anatomical mapping is inferred.
Duplicate names and more than 128 observations per frame refuse. Target definitions and all source
identities stay fixed. Correct annotations before fitting; do not tune held-out checks to a result.
Partial annotation drafts can be saved; the ordinary fitter still enforces observability, sufficient
FIT/CHECK evidence and acceptance. A saved draft does not establish a usable body fit.

Download `capture-edited.json`, then make a new immutable review before fitting:

```sh
ftx measure capture-review /path/to/capture-edited.json --out capture-review-002 --json
```

Pending/invalid edits or **Cancel placement** withdraw the old download. **Reset all corrections**
restores the original point lists and inline silhouettes, removing in-memory additions, and revokes the corrected input.
Original files and preparation evidence stay unchanged;
SVG/report links explicitly refer to that original review. Nothing is stored in browser storage.
If any input integer exceeds JavaScript's exact range, the editor refuses and the original native
review remains available. Use an integer-preserving JSON tool for that document; do not round timestamps.

`review.json` says `PREPARED`, not accepted shape or READY. It retains the original input bytes'
SHA-256 and complete source/camera evidence. `capture.json` rebases only local paths and adds
`reviewed_sources_sha256`; the ordinary fitter rechecks that evidence and refuses later source,
selection-policy or sheet-point changes. It still needs the original images, masks, calibration,
reference and manifest at those paths; the review directory is not a portable capture archive.
All files are private (0700 directory, 0600 files on Unix), create-only, and the completion report
is written last. The original static review needs no scripts; editing uses its supplied local
`review.js`. Neither needs a server, network or installed viewer. Its content policy permits only
local images/scripts and no network connections. Visual agreement does not establish correct anatomy,
lens accuracy, observability or physical fit.

## Inspect starting poses before fitting

Once the photo observations and targets are reviewed, inspect every declared initialization in
the calibrated image frame:

```sh
ftx measure fit-start-review capture-review-002/capture.json --model /path/to/anny.ftxbody \
    --out start-review-001 --json
# Open start-review-001/index.html and inspect every start and frame.
ftx measure fit-body capture-review-002/capture.json --model /path/to/anny.ftxbody \
    --mesh-out fitted-review-001 --json
```

This evaluates the actual declared shape and pose vectors through the ordinary native body model,
without optimization. The page shows each initial posed mesh over its checked source image,
observed keypoints, projected targets, and supplied silhouette slices. Parameter names and values,
residuals, contour comparisons and projection refusals are retained for every start/frame. Rectified
cameras show a wireframe; distorted cameras show individually projected vertices so straight screen
edges do not falsely represent lens distortion. Hidden surfaces remain visible in this diagnostic
view. The corresponding full initial meshes can be downloaded as OBJ files.

A misplaced start remains visible as a diagnostic: points behind the camera, missing foreground
coverage and residuals beyond the fitter's acceptance threshold are not omitted or renamed a pass.
Malformed observations, duplicate starts, missing frame identities and values outside the strictly
interior parameter bounds still refuse through the same preparation function used by the fitter.
The review checks source and model identity again, preserves exact original input bytes, and writes
`review.json` with `INITIALIZATION_REVIEW`, never an accepted body. Prepared-coordinate input works
without source photos; `--silhouettes MANIFEST` accepts the same explicit mask import as fitting.

Correct initialization values in the original input and regenerate into a new directory. Keep
predeclared CHECK observations fixed; this preview does not authorize fitting to their residuals.
The original input download retains its original relative paths and belongs at its original
location. Source images are copied privately for review, but the directory is not a portable
capture archive. Inspecting a start establishes neither observability nor anatomical accuracy;
ordinary `fit-body` remains responsible for convergence, ambiguity and held-out acceptance.

## Input structure

### Join reviewed capture sources locally

The same command also accepts `schema: "ftx-anny-capture-input/1"`. This joins
the existing decoded-frame selector and checked sheet-pose cameras before fitting.
It has `video_manifest` (relative local path), `reference_id`, the same `model`
configuration below, and `fit` with the same schema, evidence, measurements and
starts. Each `fit.frames` entry instead contains:

```json
{
  "source": {"...": "one exact selected_sources entry from video-select"},
  "lens_id": "the declared calibrated lens",
  "calibration": "calibration.json",
  "calibration_sha256": "SHA-256 of the exact accepted calibration report",
  "reference": "sheet-points-for-this-image.json",
  "keypoints": [{"name":"reviewed_target", "pixel_px":[640,480],
                 "sigma_px":1, "partition":"fit"}]
}
```

Relative paths resolve from the capture document except the video manifest's own
image/mask paths, which remain relative to that manifest. Copy each complete
`selected_sources` entry from `video-select`, preserving its decoded timestamp,
already-applied rotation, image/mask hashes and lens epoch. The reference is the
original `ftx-sheet-pose-input/1` document **without inline intrinsics**, as used
with `sheet-pose --calibration`. Its `image` must equal that selected source's
image label, and it must include at least one predeclared held-out sheet point.
The source record retains both original file hashes and the decoded grayscale
buffers used for selection; re-encoding a file therefore requires a new review.
These person/sheet masks govern sharpness selection. They add no body silhouette
constraints; optional fitting silhouettes require the separate review below.
Every frame uses the same stationary `reference_id`. The calibration must match
the declared lens, oriented image dimensions and rotation; one stream/epoch
cannot silently switch calibration. Declared focal metadata, when present,
must equal the checked calibration's focal lengths.

For the first review, copying those full records and computing calibration hashes is optional.
Use an explicit frame declaration instead:

```json
{
  "frame": {"stream":"camera-1", "timestamp_us":133333},
  "lens_id":"the declared calibrated lens",
  "calibration":"calibration.json",
  "reference":"sheet-points-for-this-image.json",
  "keypoints":[]
}
```

Run `capture-review` on this new, unbound input. It resolves only the exact declared timestamps
through the current selector, verifies the declared calibration and sheet references, and writes
complete `source` records and `calibration_sha256` values into `capture.json`. The original input
and bytes remain in the review evidence. Add observations in the local page, download the edited
input, and make its next review before fitting as above. Model targets, measurements, starts and
their frame identities are still caller-authored; no pose or anatomy is guessed.
Declare exactly one of `frame` and `source`. Supplied hashes must match. Full source records still
require hashes. Frame declarations cannot be fitted directly or used in an input with an existing
`reviewed_sources_sha256` binding; use the complete replay for that route. Unselected/missing or
duplicate identities refuse instead of substituting another view.

### Acquire the stationary sheet directly from selected images

For a printed ChArUco sheet, each frame's `reference` may instead be an explicit acquisition
declaration. This removes the need to transcribe sheet corner pixels into a separate JSON file:

```json
{
  "schema": "ftx-charuco-sheet-reference/1",
  "sheet": "calib-sheet-a4.json",
  "held_out_corner_ids": [0, 6, 35, 41],
  "board_bottom_left_xz_m": [-0.1, 0.4],
  "stationary_upright_evidence": "The original board is fixed upright; its print scale and unchanged placement were checked before capture."
}
```

Use the original sidecar from `ftx measure calib-sheet`. Declare at least four distinct held-out
corner IDs before acquisition; every declared CHECK corner must be detected, together with at
least four FIT corners. The example IDs belong to the default A4 board. The native ChArUco
detector runs on the exact selected image with its already-checked lens. Every detected corner
is retained; camera fitting never trims CHECK points or changes their role to rescue a result.
The ordinary sheet-pose residual and geometry gates still decide whether the camera is usable.
Acquisition uses the shared photo boundary's limits of 64 MiB encoded and 16 million decoded
pixels per selected image; it never resizes a frame or rescales its calibration to fit that budget.

`board_bottom_left_xz_m` is the world `(x, z)` location, in metres, of the **outer bottom-left
corner of the printed chessboard**, not the paper corner or an inner chessboard intersection.
The board must be upright and fixed in the plane `y = 0`, with printed right aligned to world
`+x` and printed up aligned to world `+z`. All acquired frames must use the same sidecar bytes
and bottom-left location. Detection does not verify these physical assumptions. The evidence
field records the operator's declaration; no gravity direction, print scale or stationarity is
inferred from the image.

`capture-review` displays the acquired FIT/CHECK points through the existing stationary-sheet
overlay. Its evidence retains the original sidecar, all detections, detector identity, exact
image/sidecar hashes and generated metric correspondences. `capture.json` rebases the sidecar
path; the reviewed source binding covers the complete acquisition. Subsequent review or fitting
reruns detection and refuses changed images, sidecars, declared partitions, board placement or
calibration instead of refreshing an old approval. Original file-based references retain their
existing behavior. No external detector, network request or body model is needed for this step.

The command reruns selection on current pixels, requires every requested source
to match exactly and still be selected, recomputes the retained calibration and
sheet pose, and then calls the unchanged articulated fitter. Original capture,
selection and camera evidence stay in `capture_input` in the fit report. A
changed image/mask, stale lens/calibration, failed camera or missing source
refuses before fitting or publishing output. This does not authenticate caller
labels or prove that the physical reference stayed stationary.
Individual JSON sources are limited to 16 MiB; the retained capture evidence is
limited to 4 MiB before model loading/fitting. Use a shorter reviewed decoded
manifest when that budget is exceeded; no frames or evidence are silently dropped.

Capture two or more sharp views with the same fixed, flat metric reference and
stable lens settings. Decode locally with original presentation timestamps and
apply orientation consistently to images, masks, keypoints and calibration;
the local FFmpeg boundary and manual mask/correspondence review remain explicit steps.
Choose fit/held-out observations and supported model targets before fitting.
Independent poses may differ between images. Sharpness selection never asserts
stillness; use the separate still-window/triangulation tools only for a reviewed
stationary region or synchronized capture. Keep independent tape evidence for
scale/shape observability. No accepted image fit makes measurements READY or
establishes real-body accuracy or garment fit.

With `--mesh-out NEW_DIR`, the existing local review also includes hash-checked
copies of these original images under the observed/fitted overlays. They remain
private workspace files (0700 directory, 0600 files); no images are embedded into
Git or uploaded. If an image changes during fitting, review publication refuses.
The existing 256 MiB complete review budget applies, including source images.

Use [`ftx measure video-decode`](video-frame-selection.md#import-a-local-phone-clip) to
prepare a clip for selection. Its strict request declares the chosen video
stream, camera epoch and clockwise turn; it snapshots the original clip locally,
checks every original PTS against the actual decoder log, and writes `frames.json`
with the oriented images. Use `video-mask-review` to author person/sheet sharpness
regions locally and `video-masks` to materialize their binary masks. Run `video-select`
on the resulting manifest, then use that selection in the capture declaration above.
You can also supply the original manifest's named binary masks or explicit
`{"rect_px":[left,top,width,height]}` sampling regions as documented in the selection guide.

Choose the actual display transform from the acquisition and verify it against
the source. Apply that same orientation to reviewed masks and coordinates;
calibrate the exact orientation, resolution and lens setting. The importer does
not infer masks, body correspondences, calibration or camera stationarity.
FFmpeg and ffprobe must be installed locally for this command; they are external
executables, not linked libraries or requirements for prepared-image workflows.

For an independently prepared decoded manifest, retain the original source,
probe, command and decoder log. Check the complete decoded PTS sequence against
the probe; refuse missing, duplicate or negative timestamps instead of deriving
them from frame rate. Keep the exact source ticks and timebase when converting
to microseconds, without shifting the stream origin. File names are transport
labels only: FFmpeg's `image2 -frame_pts 1` can wrap large timestamps in some
versions, so a timestamp-looking filename is not evidence of frame identity.

For the original prepared-coordinate input, the top-level document has exactly
`schema: "ftx-anny-body-fit-input/1"`, `model`, and `fit`.
Unknown keys, missing fields and misspelled parameters are errors. Vectors use JSON arrays,
not objects with x/y/z keys. Use the same model topology and correspondence definitions in all
images; there is deliberately no automatic mapping from a detector's "shoulder" to a mesh vertex.

The `model` object specifies:

| Field | Contract |
|---|---|
| `model_sha256` | Exact lowercase file digest, obtainable with `sha256sum` or `shasum -a 256` |
| `age_years`, `gender` | Fixed Anny inputs, 0–110 years and 0–1 respectively; not inferred |
| `fixed_shape` | Optional map of fixed phenotype/local-change values |
| `shape_parameters` | Ordered array of `{name, lower, upper}` for the shared variables |
| `root_translation_m`, `root_rotation_rad` | Optional fixed root translation and rotation vector, both defaulting to `[0,0,0]`; named free pose parameters override their respective components |
| `joints` | Optional map from exact bone label to `{axis: [x,y,z], angle_rad: 0}`; axis is a unit vector in that bone's **local rest frame**, not a world axis |
| `pose_parameters` | Ordered array of `{name, lower, upper}` repeated independently in every frame |
| `keypoints` | Map from correspondence name to an explicit vertex/bone target |
| `measurements` | Ordered array of requested rest-body tape functional names |

Shared shape names are `height`, `weight`, `muscle`, `proportions` (within 0–1), or exact
local-change labels from the converted model (within -1–1). `height` and `weight` are Anny
phenotype controls, **not** metres or kilograms. A free shape value cannot also be fixed.
Unselected phenotype controls remain at 0.5; unselected local changes remain at zero unless
explicitly fixed. These defaults are conditional model assumptions, not observations.

Pose names are `root_x_m`, `root_y_m`, `root_z_m`, `root_rx_rad`, `root_ry_rad`, `root_rz_rad`,
and `joint:EXACT_BONE_LABEL`. A free joint needs a corresponding `joints` entry defining its
axis. The root rotation's three components are an axis-times-angle vector, not Euler angles.
Joint angles and root rotation components use radians within +/-pi; root translation bounds
are finite metres within +/-1e6. Choose physically meaningful tight bounds around the starting
estimate, not enormous ranges that make finite-difference perturbations leave the camera domain.
Every free parameter starts and remains strictly inside its bounds: no silent clamping.

Keypoint targets have one of these forms (syntax examples, not anatomical mappings):

```json
{
  "marked_surface_point": {"kind": "vertex", "index": 123},
  "named_joint_head": {"kind": "bone_head", "bone": "EXACT_BONE_LABEL"},
  "named_joint_tail": {"kind": "bone_tail", "bone": "ANOTHER_EXACT_BONE_LABEL"}
}
```

Vertex indices are zero-based final-model indices, not MakeHuman base-mesh indices. Bone heads
and tails follow the same forward kinematics as the skinned surface. Unknown or out-of-range
targets, non-unit axes and duplicate target definitions under different names are refused.

The `fit` object has exactly the `ftx-body-fit-input/1` contract:

| Field | Contract |
|---|---|
| `schema` | `ftx-body-fit-input/1` |
| `metric_scale_source` | Nonempty receipt for the shared calibrated camera frame/scale |
| `observation_evidence` | Nonempty source and correspondence-definition receipt |
| `frames` | 2–8 frames, each with `frame: {stream, timestamp_us}`, `camera`, `image_size_px`, `rotation_cw_deg`, and `keypoints` |
| `measurements` | Up to 32 `{name, value_m, sigma_m, evidence, partition}` tape targets |
| `starts` | 2–4 distinct `{shape: [...], poses: [{frame, values: [...]}]}` initializations |

Each frame's keypoints are `{name, pixel_px: [x,y], sigma_px, partition}`. Partitions are
`fit` or `held_out`, chosen before fitting. Image dimensions and pixels are full-resolution
and already oriented; `rotation_cw_deg` records a rotation already applied, not one the tool
should perform. Calibration must describe that same orientation, crop and resolution.
Camera representation is the shared format documented in `triangulation.md` and produced by
`sheet-pose`. World coordinates are metres, z up; camera coordinates are right/down/forward.
The Anny body's +X is its left and it faces -Y before root placement.

Pose arrays are attached by decoded frame identity, never by whichever order a file happened
to use. Shape/pose vectors follow the declared parameter-array order. Keypoint names map to
targets by name; the report preserves the canonical sorted correspondence layout.

Tape values and sigmas are **metres**, unlike existing measurement JSON's millimetres. Supported
functionals are the 21 `ftx-body::TAPE_FIELDS` names, including `stature`, `chest`, `waist`, `hip`
and `arm_length`. Stature is the rest mesh's z extent; other functionals reuse the existing
body tape implementation at its standard 35-degree A-pose. They do not change as image poses
move. Give actual independent tape evidence, not a size-chart guess masquerading as tape.

## Author sampled silhouettes in capture review

The local capture-review page can author body-contour constraints directly on the selected images,
without first preparing a separate binary mask. These constraints describe the visible body contour
at explicit sampled lines. They are separate from the person/sheet regions used for video sharpness.

Choose a frame in the silhouette editor, then declare a horizontal or vertical slice, its fixed
full-resolution pixel coordinate, FIT or CHECK role, and contour-location sigma. Explain the
contour definition and the treatment of clothing and occluders in the evidence field. Add each
foreground interval by its two endpoints, either numerically or by picking endpoints on that
frame's image. Horizontal slices hold Y fixed and describe X intervals; vertical slices hold X
fixed and describe Y intervals. Display scaling and zoom do not change stored coordinates.

Keep separate intervals for disconnected foreground, such as gaps between limbs. An explicit
empty interval list asserts background on that slice. A complete silhouette needs at least one
foreground slice; background-only or unfinished drafts cannot become valid contour evidence.
The existing native limits apply: 1–64 unique slices, up to eight ordered, disjoint intervals per
slice, in-image coordinates, nonempty evidence, and sigma in `1e-6..1e6` pixels. The editor
refuses crossing, touching or out-of-order intervals instead of merging or clipping them.

Existing slice edits preserve their axis, fixed coordinate, FIT/CHECK role and sigma while
correcting endpoints. Remove an unwanted inline slice explicitly. Pending or invalid changes
withdraw the corrected-input download; cancel returns to the saved draft, and reset restores
the complete original input. Changes remain in memory until you download `capture-edited.json`.
Run `capture-review` on that downloaded document in a new output directory, then pass the reviewed
capture to the ordinary fitter. Source identities, calibration and model-target definitions remain
unchanged, and the native fitter still owns observability, optimization and acceptance.

Authoring is available only for checked cameras with zero radial and tangential distortion.
Rectify the images, keypoints and calibration together before generating the review; the page
cannot rectify them. Frames supplied by a silhouette-mask manifest remain read-only so edits
cannot replace or detach their source-bound evidence. Other frames may use explicit inline slices.
Choose held-out lines before fitting. A plausible outline, a successful download or agreement on
the sampled lines does not establish anatomical correctness or physical fit.

## Silhouettes from rectified masks

Review these constraints before loading a body model or fitting:

```sh
ftx measure capture-review capture/input.json --silhouettes masks/manifest.json \
    --out capture-review-with-masks --json
# Review capture-review-with-masks/index.html, then fit its capture.json without another mask flag.
```

The flag adds an absolute `silhouettes` manifest path to the replay capture document. A capture
document may instead declare that path itself, relative to the document. The flag cannot replace
an existing declaration, and masks cannot replace inline silhouettes. The ordinary fitter rereads
the manifest and masks; the review's source digest binds manifest bytes, samples and decoded mask
hashes, so later changes refuse. Keep the original mask files available. Captures without a mask
manifest retain their original source-digest calculation. Original input bytes and masks stay unchanged.
The masks used for video-selection sharpness remain separate from these body constraints.

For prepared ordinary observations, the original direct fitting route also remains available:

```sh
ftx measure fit-body observations.json --model /path/to/anny-0.6.1-default.ftxbody \
    --silhouettes masks/manifest.json --out silhouette-fit.json --json
```

The mask manifest has this structure; frame identities must match the fitting request exactly:

```json
{
  "schema": "ftx-body-silhouettes/1",
  "evidence": "Reviewed segmentation and rectification receipt; describe clothing/occluders",
  "frames": [
    {
      "frame": {"stream": "camera-1", "timestamp_us": 133333},
      "mask": "body-133333.png",
      "samples": [
        {"axis": "horizontal", "index": 420, "sigma_px": 1.0, "partition": "fit"},
        {"axis": "horizontal", "index": 480, "sigma_px": 1.0, "partition": "held_out"},
        {"axis": "vertical", "index": 640, "sigma_px": 1.0, "partition": "fit"}
      ]
    }
  ]
}
```

Mask paths are relative to the manifest, not the working directory. A mask must be an opaque
8-bit grayscale PNG containing only 0 (background) and 255 (body), with the frame's exact
oriented dimensions. Transparency, animation, EXIF metadata, soft masks, mismatched dimensions
and foreground touching an image border are refused. No resizing, threshold guessing,
segmentation inference or coordinate rotation is performed. Pixel centers are integer indices;
a foreground run at centers `i..j` has boundaries `i-0.5` and `j+0.5`.

**Rectify the mask, keypoints and camera calibration together before using silhouettes.**
The camera must have zero distortion coefficients. Otherwise projected triangle edges are
curved and cannot be represented by this straight-edge scanline implementation. Merely zeroing
coefficients without rectifying the evidence invalidates the measurement assumptions.

Choose fit/held-out rows and columns before fitting. Up to 64 distinct slices and eight
foreground intervals per slice are supported, in both the observation and prediction;
excess components are refused, not dropped.
Empty sampled rows/columns explicitly assert background, but at least one line must contain
foreground. Sampling does not establish what happens between the requested lines. Use both
orientations and appropriate coverage for the dimensions being fitted.

The actual posed Anny triangles are projected and unioned: gaps between limbs remain gaps,
overlapping surfaces do not create duplicate boundaries, and internal mesh edges do not become
silhouette evidence. The fitting objective adds signed boundary distances in both directions and
the signed foreground-length difference on fit lines only. Sixteen reverse-boundary slots are
zero-padded to keep residual dimensions fixed; missing/extra coverage remains diagnostic data.
Every line, including held-out lines, must pass a 3-pixel
symmetric boundary-displacement gate; an empty/nonempty mismatch always fails. A model that does
not intersect a foreground fit slice has no finite contour residual and needs a better start.
Silhouette topology changes can also prevent convergence; no failed attempt publishes a body.

Alternatively, supply `silhouette` directly inside a fit frame as
`{"evidence":"...", "lines":[{"axis":"horizontal", "at_px":420,
"intervals_px":[[560.5,719.5]], "sigma_px":1.0, "partition":"fit"}]}`.
Intervals must be ordered and disjoint. A mask cannot overwrite inline silhouette evidence.
The report retains the derived intervals, partitions and per-line checks. Mask imports also
retain the manifest, its byte digest, image dimensions and SHA-256 of each decoded row-major
grayscale buffer (not a hash of the PNG encoding). Original inputs/masks are never modified.

Contour sigmas are fitting weights, not a validated independent-noise model; neighboring
mask boundaries may be strongly correlated. Segmentation, loose clothing and occluders can
produce a self-consistent but wrong body estimate. Those systematic errors are not inferred.

## Observability, acceptance and output

Known cameras alone do not generally fix body scale when each pose has a free distance to
its camera. A tape constraint such as stature can resolve that shape/depth ambiguity. The
solver checks the **undamped fit-data** Jacobian for local rank/conditioning before allowing
LM damping. Priors or an initial guess cannot rescue rank-deficient data. Unseen parameters
should be fixed explicitly, not reported as recovered. Fixed unmeasured controls remain
model assumptions; a local rank check only covers the selected variables.

Only fit observations influence optimization, derivatives, stopping or solution selection.
Every fit and held-out image residual must be at most 3 px, regardless of sigma; tape residuals
must be within three declared sigmas. Every initialization must converge and pass all checks.
Successful shared-shape estimates must agree within 1% of each parameter's declared range.
Distinct fitting shapes produce AMBIGUOUS rather than an arbitrarily chosen body. Agreement
between a few starts is not proof of global shape or pose uniqueness.
Per-frame pose parameters must also agree within 1% of their declared ranges; pose ambiguity
cannot be hidden by shared-shape agreement. `--max-evaluations` bounds all model calls across
starts, derivatives, trials, surface comparisons and final checks (default 100,000; 1–1,000,000).
Exhaustion returns exit 4 without a solution, even when earlier starts converged.

`result` in the usual JSON envelope contains `ftx-body-fit/1`, full canonical input, the verified
model ID/configuration, raw input digest, numerical policy, every attempt's objective history,
per-observation checks and shared-shape spread. `solution` is null on every unsuccessful outcome.
An accepted solution includes shared shape, frame-specific poses, predicted metric functionals
and `anny_parameters` tagged MODEL_FIT. These are not fused measurement records, do not include
an invented sigma, and do not make a real person's measurement gate READY. This report is not
a replacement for the existing drape body's `ftx-body/1` file or its mesh-validation checks.

Exit codes are 0 accepted, 2 rejected/ambiguous/unobservable/not converged, 3 invalid input or
output refusal, and 4 iteration budget exhausted. `--iterations` accepts 1–100 (default 50).
`--out` creates a new report exclusively, including on diagnostic failures, and refuses to
overwrite source data, previous reports or their file aliases.

The core's hard budgets and algorithm are in `crates/ftx-measure/BODY_FIT.md`. Its analytical
tests exercise actual shape-depth degeneracy and multiple shape minima. Regular CLI tests execute
the real loader, forward model, skinning, fitter and file writes using a tiny synthetic two-bone
model. The opt-in production case in `crates/ftx-cli/tests/body_model_fit.rs` also exercises the
admitted full Anny 0.6.1 asset: three independently moving root/arm poses, two shared shape
controls, two nontruth starts, 144 projected points (24 held out), a stature constraint and
20 withheld tape predictions. Other shape controls stay at explicit model defaults. It checks
the 3 mm girth / 2 mm length bounds, accepted-shape body export, held-out rejection and exhausted
budgets. With the admitted asset selected through `FTX_ANNY_MODEL`, run
`cargo test -p ftx-cli --release --test body_model_fit -- --include-ignored --nocapture`.
The production case refuses a different model hash; body assets and private observations stay out of Git.

Known synthetic correspondences and cameras do not establish detector visibility or independent
physical accuracy. The production case does not use silhouettes. Full-mask validation, automatic
keypoint inference, camera/tape/segmentation systematic uncertainty, broader production capture
qualification and a ruler-in-frame real-photo check remain unverified or outside this slice.
No real-wearer or garment-fit claim follows from accepted geometry.
