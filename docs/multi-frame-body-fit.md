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

## Input structure

The top-level document has exactly `schema: "ftx-anny-body-fit-input/1"`, `model`, and `fit`.
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

## Silhouettes from rectified masks

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
