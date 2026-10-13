# Photo sections to girth estimates

`ftx measure girth-from-section` connects calibrated front-view widths and side-view depths
to the measurement workflow. It uses the section model in spec 02 A.5: a shape-factor-corrected
ellipse, with separate width, depth and shape-factor uncertainty. The result is a
`PHOTO_ESTIMATED` record, not an independently measured circumference or proof of fit.

## From calibrated photo records

First use `ftx measure measure-photo` to create `waist_width` and `waist_depth` records (see
`photo-measurement.md`). A front and a side photo will normally need separate reference
calibrations. Combine their evidence, then derive the girth:

```bash
ftx measure fuse front-photo.json side-photo.json --no-priors --out sections.json
ftx measure girth-from-section --level waist --from sections.json --out waist-photo.json --json
ftx measure fuse tape-measurements.json waist-photo.json --out fused.json --json
```

`--from` recomputes fusion from the original records rather than trusting a cached `fused`
map. Both dimensions must have usable, nonconflicting photo evidence; chart/model values,
mixed-provenance estimates, missing dimensions and conflicting measurements are refused.
Inline dimension or sigma arguments cannot override `--from` evidence. The output keeps the
source entries, wearer and capture date. New outputs never overwrite existing files or input
aliases. Measurement readiness still runs independently after fusion.

Widths and depths must describe the same anatomical level, wearer, posture and clothing
condition. The command cannot establish those facts from a record name. It does not turn a
single straight image distance into a circumference.

## Explicit dimensions and uncertainty

For already calibrated dimensions, all lengths and sigmas below are millimetres:

```bash
ftx measure girth-from-section --level waist --width-mm 160 --depth-mm 130 \
  --sigma-width-mm 4 --sigma-depth-mm 5 --out waist-photo.json --json
```

This published spec example returns **484.2 mm ± 14.6 mm (one sigma)**. Its uncertainty
contributions are 6.99 mm from width, 7.87 mm from depth and 10.05 mm from the shape factor.
`chest`, `waist` and `hip` have explicitly uncertain shape defaults. They are not calibrated
child-specific facts. `--k`, `--k-sigma`, `--k-ratio`, and `--k-ratio-sigma` expose the model
assumptions; a custom level needs an explicit `--k`.

`--clothing-mm` subtracts the specified thickness from each side of each dimension. Negative
thickness, zero/negative unclothed dimensions, nonfinite input, negative uncertainty and
nonpositive factors are refused, never silently clamped. Clothing thickness has no separate
uncertainty term in the reference model: account for it in the input sigmas or retain that
limitation when interpreting the result.

Default named levels produce a schema-valid `fabric-measurements/1` document with a single
record, plus the section calculation and the reference's `record` field. Custom levels
produce a calculation report unless `--name` names the intended girth. A known width, length
or angle name is not accepted for a girth. No capture date is invented for inline dimensions.

## Calibrating the shape factor against a tape girth

```bash
ftx measure girth-from-section --level chest --width-mm 172 --depth-mm 150 \
  --tape-girth-mm 560 --sigma-tape-mm 10 --out shape-calibration.json --json
```

This published example returns `k_child = 1.1058`, `k_ratio = 1.0335`, and
`k_ratio_sigma = 0.0185`. It emits a coefficient report **without measurement records**:
using the same tape value to calibrate a model does not create a second independent
observation of that girth. The ratio and its uncertainty can be supplied to later section
estimates, retaining the section model's remaining uncertainty. Model override arguments
and `--name` are rejected while calibrating; `--sigma-tape-mm` requires `--tape-girth-mm`.

The propagated uncertainty assumes independent width, depth and shape-factor errors. Shared
camera-calibration error, pose mismatch and systematic landmark errors are not represented
by that formula. Keep correlated evidence and tape calibration provenance in mind when
fusing results; small reported uncertainty alone does not establish real-world accuracy.
