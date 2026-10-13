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

**Section girths** (spec 02 A.5). `girth_from_section` applies the documented uncertain
chest/waist/hip shape factors to the Ramanujan ellipse perimeter, propagating width, depth
and shape-factor uncertainty separately. Clothing offsets and calibrated k ratios are
explicit. `calibrate_k` estimates a coefficient from a tape girth; it is not another girth
observation. Published D.5 values and prior-section round trips are covered by direct tests.
The CLI emits PHOTO_ESTIMATED records and accepts `--from` only for consistent photo-derived
width/depth evidence, recomputed from source records. It does not relabel charts or model
predictions as photo evidence. See `docs/section-girth.md` for commands and independence limits.
