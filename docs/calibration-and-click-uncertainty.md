# Native calibration sheets and conditional 3D precision

## Download a reference in the browser

In the Pattern Workbench's **Body** section, open **Print a photo scale reference**.
Letter and A4 each offer a PDF, 300-DPI PNG and matching JSON sidecar. The six
files are local static downloads and work with page JavaScript disabled. Use the
PDF for printing and retain its sidecar with the reference. Check both ruler
directions and paper flatness before using a printed sheet.

These are the default `DICT_4X4_50` boards: 22 mm squares and 16 mm markers.
Custom layouts use the native command below; the browser does not generate
custom boards or perform photo measurements through these download controls.
The packaged bytes in `web/calibration/` come from the unchanged native generator
at `22dfe7ef214bf5ed5f105f5d9246c157d1f0d52d` (generator source SHA-256
`d4aa9cdca4121bea47a19181df43e443966d35a0f0d3426e780366cf8fd4d805`).
Reproduce them with the two commands below; each output's PDF, PNG and saved
sidecar match the distributed files exactly. Their native executable SHA-256 is
`a0590e52110cbc9e3bcd9092eb7daa538d24bee6b2675260c639b7b138db7bc3`.
Changing the generator requires deliberately regenerating and checking both
paper sets; these product files are not oracle fixtures.

## Print a metric reference

```sh
ftx measure calib-sheet --paper letter --out ./calibration-letter --json
ftx measure calib-sheet --paper a4 --out ./calibration-a4 --json
```

Each command requires a **new directory** and writes `calib-sheet-<paper>.png`,
`.pdf`, and `.json`. The PNG has 300-DPI metadata; the lossless, one-bit PDF uses
the same raster and the spec's physical page dimensions. The JSON is a
`fabric-calib-sheet/1` sidecar with the board definition, rendered pixel-edge
coordinates, 100 mm / 4 in bars in both directions, and 10 cm / 1 in test squares.
The command's JSON result also includes output paths; the saved sidecar does not.

Defaults are `--square-mm 22 --marker-mm 16 --dictionary DICT_4X4_50`.
Letter uses an 8 x 6 board; A4 uses 8 x 7. Other integer square/marker sizes are
accepted when all marks fit without overlap, with `0 < marker < square`.
Only `DICT_4X4_50` is implemented; another dictionary is refused, not substituted.
The canonical non-legacy board starts with a black top-left square, with marker
IDs increasing row by row across white squares.

Print at **100% / Actual size**, not Fit to page. Check both ruler directions
before using the sheet. A digital reference does not prove that a printer has
preserved scale or that the paper is flat. The sidecar is a reference definition,
not a detection result: image corner detection remains external. In particular,
its page-layout origin is not an estimated camera pose.

Implementation boundaries relative to spec 02 A.9: the export stroke font replaces
the unpinned reference font; the PDF stores an uncompressed one-bit image rather
than requiring CCITT. Very small boards whose vertical bars overlap lower marks
are refused instead of drawing an ambiguous reference, and output directories
are never reused. Native generation supplies the browser's two default download
sets. These boundaries do not constitute completion of all FTX-19 work.

Eight Rust regressions cover the D.7 physical coordinates, dictionary bits, nearest
sampling ties, PNG round-trip and density, PDF dimensions/xref, invalid layouts,
and exclusive multi-file output. Independent checks on the default Letter/A4
outputs with OpenCV 4.13.0 found all 35/42 corners and 24/28 markers; both board
rasters matched OpenCV generation plus Pillow nearest resampling pixel-for-pixel.
PyMuPDF decoded both PDF images identically to the PNG pixels. Those are digital
interoperability checks, not a physical print or real-wearer accuracy test.

## Add click-noise precision to triangulation

```sh
ftx measure triangulate point.json --click-noise noise.json --out result.json --json
```

`point.json` is the existing `ftx-triangulation-input/1` document. Noise must
identify exactly its fitting frames, not held-out frames. For two fitting frames:

```json
{
  "schema": "ftx-click-noise/1",
  "evidence": "Synthetic example: independent one-pixel error per axis",
  "observations": [
    {"frame": {"stream": "left", "timestamp_us": 0}, "sigma_px": 1.0},
    {"frame": {"stream": "right", "timestamp_us": 0}, "sigma_px": 1.0}
  ]
}
```

The result adds `conditional_click_uncertainty`: full `covariance_m2` (matrix
`rows` in square metres), marginal `sigma_m: [x,y,z]`, per-frame
`jacobian_m_per_px`, sorted noise evidence, status and limitations. The Rust API
is `ftx_measure::triangulate::uncertainty::triangulate_with_click_noise`.

The estimate and geometric gates do not change. Noise propagates through the
actual pixel-refined triangulation pipeline, not a separate or differently
weighted estimator. Checked central differences at 0.01 and 0.005 pixels must
agree to 0.1%, with camera-center normalization avoiding large-origin
cancellation. Held-out pixels never contribute to the differential or covariance.

This is **first-order conditional click-noise precision**, assuming independent
isotropic clicks and fixed cameras, scale and capture. It excludes calibration,
metric-scale, synchronization, motion, correspondence and systematic error. Large
noise can invalidate linearization even when the differential is numerically
stable. The result is not total uncertainty, a simultaneous confidence region,
a body-measurement record, or a garment-fit claim.

Missing, duplicate, unknown or held-out noise frames and non-finite/nonpositive
sigmas are refused. Sigmas above 1000 pixels are also refused. Weak/rejected
geometry, unstable differentiation, lens-domain boundaries and numeric
underflow/overflow return `UNAVAILABLE` with null covariance/sigma, never perfect
zero uncertainty. A valid geometric point can remain available when its precision
is unavailable; requesting this extension then returns exit 2. Malformed evidence
is exit 3. Omitting `--click-noise` preserves the original report and exit behavior.
Output creation cannot replace a previous report or either input.

Ten library tests cover analytic stereo covariance and cross covariance,
noise/metric scaling, large origins, order invariance, held-out isolation,
distortion, invalid evidence and unavailable estimates. Four CLI tests cover
backward compatibility, evidence protection, invalid input and partial availability.
