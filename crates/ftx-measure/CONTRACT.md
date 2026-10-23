# ftx-measure

Measurement evidence in millimetres (degrees only for angle names), per spec 02 Part A.
No images, personal data, body-model dependency, I/O or runtime Python. Catalogue and chart
constants come from the specification, not a new oracle. Records retain provenance,
definition and one-sigma uncertainty. Repeats of the same method are correlated: they never
reduce uncertainty. Incompatible definitions are excluded, not silently fused. Disagreement
above z=3 requires remeasurement; a chart cannot override out-of-bounds evidence.

The library exposes the 35-item catalogue, record validation/unit parsing, normalization,
public-data priors, single-measurement and whole-document fusion, the explicitly fictional
reference wearer, and garment-specific readiness. Built-in priors cover 800–1280 mm stature.
Outside that interval, whole-file fusion retains measured evidence and supplied priors and
reports the absent chart bounds. Disabling automatic priors does not disable sanity bounds
or discard supplied evidence. A numeric wearer.stature_mm is retained as evidence even
without an age/automatic stature prior. Original records are not changed, and a stored fused
cache is always recomputed.

Readiness checks bodices, set-in sleeves, trousers, gathered trousers and hats, including
pass-over, closed-cuff, nappy and elastic-waist options. Missing, conflicting, prior-only and
overly uncertain required evidence blocks readiness; optional evidence yields advisories.
A used direct source can qualify as OK_TAPE_LIMITED up to twice the target sigma. Reference
wearers are READY_REFERENCE, never ordinary READY, and keep the real-wearer measurement
requests in for_real_child. The caller supplies an ISO as_of date: no hidden system clock.
Staleness after 42 days is advisory, not a fit claim. blocking contains measurement names;
remeasure_request contains actionable instructions.

Tests pin every value/sigma/bound/method in spec 02 D.2, all five D.6 size columns, D.5 fusion
and readiness examples, garment options, staleness boundaries and invalid inputs. Oracle parity:
`tests/fixtures/oracle/measure/` (`tools/oracle/capture_measurements.py`, compared in
`ftx-conformance/tests/measurements.rs`) holds the reference's own outputs for priors across
statures, ages and sexes, the CDC table, reference wearers, fusion of synthetic evidence, the
schema checks and 284 readiness cases; all match key for key (numbers within 1e-9) except the
wearer-block stature kept as evidence above, a recorded parity exception. Fused `notes` are
lists and chart-prior sources carry the prior's note, as in the reference. None of this is a
real-wearer fit claim.
Coordinate-based photo measurement is available in `photo` (spec 02 A.10–12; boundary below).
Automatic sheet detection remains separate work. JSON maps are sorted;
numerical reductions use input order; formatting uses ftx-core's Python rounding. Invalid or
non-finite inputs get typed refusals.

**Photo coordinate boundary.** `photo::analyze_reference` reads `ftx-photo-reference/1`:
`reference: plain_sheet`, `paper: letter|a4|auto`, `image_size_px: [width,height]`, four
`corners_px` clockwise on screen, optional `corner_sigma_px` (default 3) and `image` label.
These are full-resolution EXIF-oriented coordinates, supplied by the caller; the library does
not read, orient, detect or verify the image. It enforces convexity and the spec's rectangle
check, refusing to calibrate wrong/ambiguous paper. A rejected scale has `ok: false` and no
usable matrix. Manual coordinates cannot prove that the object actually is the named paper.

`photo::measure_photo` maps pairs to distances/angles, with Jacobian click uncertainty, depth
uncertainty and sheet-level uncertainty for angles, producing only PHOTO_ESTIMATED records.
Its manual-corner bootstrap perturbs each coordinate by the declared sigma and each sheet
side by 0.5 mm (rather than resampling detected edge pixels). SplitMix64/Box–Muller is seeded,
clock-free and deliberately not NumPy-stream parity. Bootstrap draws are bounded to 20–2000;
fewer than 80% usable draws, rank loss, projective horizons and malformed inputs are refusals,
not zero-sigma/partial results. The unrounded matrix is retained for calculations.

Known-answer tests recover spec D.7 exact-camera residuals/focal search and analytic distances,
angles and uncertainty terms. No automatic detector, image-oracle or real-wearer fit claim.

**Imported ChArUco.** The same input schema accepts `reference: charuco`, board dimensions
(`squares_x`, `squares_y`, `square_mm`, optional `origin_mm`) and identified image corners
`corners: [{id, point_px}]`. IDs are row-major internal corners. Fixed-seed RANSAC (512 trials,
2 mm threshold) followed by normalized algebraic least squares requires six consistent
inliers; reports retain excluded IDs and residuals. Detector ordering cannot alter results.
Bootstrap resamples only inliers and requires six unique points per draw; the 80% usable-draw
gate can refuse a sparse board despite successful calibration. At least twelve well-spread
corners are recommended. Duplicate/invalid IDs, excessive input, invalid coordinates and
unstable consensus are explicit errors. Blur, if supplied, is external metadata, not a
computed image diagnostic. No claim of OpenCV RANSAC/noisy-fit or NumPy RNG parity.
Runnable CLI and input examples are in `docs/photo-measurement.md`.

**Portable camera acquisition.** `photo::calibration_images::Acquisition` shares the native
manifest, board, capture and predeclared FIT/held-out admission with file-free callers. It freezes
the original manifest and sheet before detection, admits exactly one decoded image at a time in
canonical frame order, rejects duplicate decoded pixels before detection and retains every identified
corner without photo-scale RANSAC or fitted-lens redetection. Failed additions commit no state;
incomplete batches cannot calibrate. The existing 32-image, 16-million-pixel/image, 128-million-pixel
batch and 16-MiB JSON bounds apply. Receipts preserve original text hashes, capture labels and
partitions, detector identity and decoded/encoded hashes without retaining pixels.
`intrinsics::calibrate_document` supplies the common coordinate/noise source envelope around the
unchanged calibration and covariance engines. Tests cover partition/order invariance, raw Brown
admission, transactional acquisition and exact native/portable source replay. Synthetic raster
calibration in `ftx-wasm/tests/photo.rs` establishes software behavior, not empirical camera,
reference print-scale, lens-setting or wearer-fit accuracy.

**Conditional intrinsic covariance.** Opt-in Brown `corner_noise` declares independent isotropic
per-coordinate pixel noise and its evidence, never derived from residual RMS. The undamped
full fit-only Jacobian yields a nine-parameter covariance after marginalizing fitted poses;
every existing rank/held-out gate still applies. `PhotoScale::with_lens_uncertainty` requires
the identical lens and explicit independent-acquisition evidence. It propagates correlated
lens modes through refits of the same original inlier pixels, adds a separate intrinsic sigma
term, and retains the assumptions. No print/flatness error, lens drift, empirical calibration,
joint acquisition noise, nonlinear confidence region or general cross-record covariance claim.
Tests in `camera_brown` and `photo_lens` include independent pixel-difference/SVD marginalization, 80 independent
noisy refits, held-out independence, parameter correlations, pinhole-gauge cancellation and
horizontal-angle derivatives. The image-to-measurement CLI test includes recomputation, tamper
refusal, explicit independence and decoded-pixel acquisition-reuse rejection.

**Section girths** (spec 02 A.5). `girth_from_section` applies the documented uncertain
chest/waist/hip shape factors to the Ramanujan ellipse perimeter, propagating width, depth
and shape-factor uncertainty separately. Clothing offsets and calibrated k ratios are
explicit. `calibrate_k` estimates a coefficient from a tape girth; it is not another girth
observation. Published D.5 values and prior-section round trips are covered by direct tests.
The CLI emits PHOTO_ESTIMATED records and accepts `--from` only for consistent photo-derived
width/depth evidence, recomputed from source records. It does not relabel charts or model
predictions as photo evidence. See `docs/section-girth.md` for commands and independence limits.

**Calibrated sheet poses (FTX-20).** The shared `camera::Camera` (also exported as
`photo::Camera`) supports `from_sheet` and `project_with_jacobian`. Sheet pose recovery uses
known intrinsics and metric `(x,z)` points on the common stationary plane `y=0`; it rejects
non-rigid homographies and residuals above 3 px. Metres and z-up world coordinates are distinct
from x-right/y-down/z-forward camera coordinates. Projection derivatives follow the shared
Brown–Conrady calibration and its certified lens domain. Exact synthetic pose/scale recovery,
off-plane held-out projection and analytic-Jacobian finite differences are tested in
`photo::camera::tests`. This is an algebraic calibrated-pose slice, not unknown-focal
calibration, image detection, noisy-PnP parity, bundle adjustment or a real-wearer fit claim.

**Calibrated 3D geometry** (plan §10.2, FTX-20). `camera` provides oriented-image intrinsics,
polynomial Brown distortion and bounded certified inversion; rotations must be right-handed
and orthonormal. Its 3D coordinates are metres, z up, unlike the millimetre evidence records.
`triangulate::triangulate` consumes `ftx-triangulation-input/1`: calibrated cameras in one
metric frame, unique decoded stream/timestamp identities, explicit fit/held-out assignments,
and a caller-reviewed still-window or synchronized-capture assertion. It does not detect
stillness, synchronize clocks or establish calibration/scale accuracy itself.

Ray-distance initialization uses the existing SVD with origin/baseline normalization
and canonical frame ordering. Bounded Gauss–Newton then minimizes equal-weight fit-view pixel
residuals using analytic projection derivatives, fixed cameras and strict objective descent.
Nonconvergence publishes no point; initial/final cost and convergence diagnostics are retained.
Closed-form unequal-focal stereo, noisy distorted-camera stationarity, metric-frame invariance
and held-out independence are covered by `tests/pixel_refinement.rs`.
Fit geometry must have at least 3-degree acute parallax before
and after solving. Every fit and held-out view must project in front of its camera, inside its
lens domain and within 3 pixels. Held-out views never fit or rescue weak geometry. Weak,
inconsistent or behind-camera geometry publishes diagnostics with no usable point; malformed
inputs produce typed refusals. Reports retain the full input receipt and distinguish fitted
from held-out errors. No measurement sigma, fused record, READY status or body-fit claim is
created. See `docs/triangulation.md` for the CLI, schema and remaining FTX-20 work.

**Anchored joint bundle refinement (FTX-20).** `bundle::adjust` jointly refines all camera
extrinsics and free landmark positions against sigma-weighted fit pixel residuals. Fixed
metric controls define the gauge and absolute scale; intrinsics and lens distortion remain
fixed. This initial slice requires at least four distinct fit controls per camera, 2–8 cameras,
1–32 free landmarks and at most 2048 observations. It is for a caller-reviewed still window
or synchronized acquisition of stationary landmarks, not a moving child's cross-frame shape.

Assembly is canonical and uses analytic pose/point derivatives, a normalized metric frame,
column-scaled dense Levenberg–Marquardt and strict objective descent. An undamped normalized
Hessian Cholesky-pivot check (floor 1e-10) rejects rank loss or severe ill-conditioning before
damping; this is a conservative numerical gate, not a statistical observability certificate.
Held-out observations never enter assembly, convergence or rank checks. Both observed and
reconstructed free-point parallax must reach 3 degrees after refinement, and every observation
must project within its lens domain and 3 pixels, regardless of sigma. Budget stops, rank loss,
nonconvergence and failed held-out checks publish diagnostics but no optimized geometry.

Synthetic tests cover joint metric recovery, nine-coordinate derivative differences, noisy
overdetermined fitting, fixed intrinsics, held-out independence, gauge/order invariance,
collinear controls, missing fit information, strict schemas and budget-stop atomicity.
No external numerical dependency. No estimated reference-scale parameter, joint intrinsic
calibration, covariance, global-minimum guarantee, body fit or real-wearer accuracy claim.

**Sampled body silhouettes (FTX-21).** Optional frame silhouettes participate in the shared
shape/per-frame pose fitter through `BodyModel::surface`. Rectified mesh triangle unions are
compared on fixed horizontal/vertical image lines; limb gaps and overlapping surfaces retain
their projected coverage. Fit-only edge/coverage residuals and symmetric 3-pixel gates preserve
the existing multistart, rank and held-out rules. Binary-mask sampling retains half-pixel
boundaries and refuses clipping, soft evidence and excess components. No full-mask, segmentation
accuracy, covariance or real-wearer fit claim. See `BODY_FIT.md` and
`docs/multi-frame-body-fit.md` for exact input conventions, limits and failure behavior.

**Still-window candidates (FTX-20).** `still::assess_still_window` checks one 4–64-frame,
one-stream interval with calibrated cameras in a common stationary-reference frame and the same
named sparse landmarks in every frame. At least three fit landmarks are required. Each landmark is
triangulated independently in the early and late temporal halves and over the complete interval;
each subset needs at least 3-degree parallax and at most 3 px reprojection error. The early/late
metric displacement must stay within a caller-supplied positive threshold (at most 100 mm). Held-out
landmarks never rescue fit evidence and independently reject the candidate. Weak parallax is
UNOBSERVABLE, not evidence of stillness. No outlier is trimmed. A `STILL_CANDIDATE` remains only a
filter for explicit human review: it does not create `triangulate::Capture::StillWindow`, prove that
untracked anatomy was still, or establish camera/scale accuracy. See `docs/still-window.md`.
