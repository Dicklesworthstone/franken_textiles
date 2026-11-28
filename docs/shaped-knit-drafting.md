# Draft a knitted tube from a 3D shell

`ftx knit shape` converts an explicitly supplied annular shell into a two-bed tubular
stitch graph, compiles it through the ordinary machine verifier, and exports the
program, charts, certificate and geometry report together. A tapered sleeve or a
widening tube can vary its circumference between courses. Every live parent loop
has one owner, decreases consume the removed loops, and the terminal course forms
a bind-off chain.

The supported source has one connected, oriented surface, one start ring and one
end ring. A complete pullover with multiple sleeve openings requires branching
that this geometry pathway does not yet implement. Such a shell refuses explicitly;
use the separate measurement-driven branched pullover drafter when its parametric
construction matches the design.

## Native command

Supply the original `EaseShellMesh` JSON, explicit garment choices, and an
independently obtained relaxed gauge:

```sh
ftx knit shape sleeve-shell.json --config shaping.json --gauge gauge.json \
  --machine hand-knit --created 2026-11-27T12:00 --out shaped-sleeve
```

`shaping.json` contains the same fields as `AutoKnitConfig`. The following is a
**synthetic numerical example**, used with the small source mesh in
`tests/fixtures/knit/synthetic-annular-shell.json`:

```json
{
  "name": "synthetic-annulus",
  "wearer_age_class": "adult",
  "yarn_id": "synthetic-yarn",
  "carrier_id": "1",
  "cam_setting": 13.0,
  "max_idt_flips": 1024,
  "has_hood_or_neck_drawstring": false
}
```

The corresponding example `gauge.json` is:

```json
{
  "wales_per_100mm": 20.0,
  "courses_per_100mm": 25.0,
  "needle_pitch_mm": 4.0,
  "yarn_tex": 180.0,
  "thickness_mm": 2.0,
  "relaxed_after_wash": true
}
```

These values describe a test input; they are not measured yarn or garment
qualification. Production machine profiles retain their ordinary swatch gate:
add `--qual qualification.json --material-sha256 HEX` for an independently selected
material and matching machine. An optional `--drape-receipt receipt.json` must match
the exact resulting document and material. Calibration-swatch exemptions do not
apply to shell-derived garments.

The earlier flag-based `ftx knit shape --input ...` workflow remains available.
The positional-shell command above requires both explicit configuration and gauge
files and uses the same shared exporter as the portable interface.

## What the geometry calculation establishes

The drafter validates the full boundary, manifold vertex links, connectedness,
orientation, annular topology and triangle intersections before sampling courses.
Intrinsic edge flips preserve the piecewise-flat surface metric: a replacement
diagonal is calculated by unfolding its two triangles, rather than using a chord
through three-dimensional space. The cotangent harmonic solution and its weights
are checked without replacing invalid values with clamps.

Every interval between distinct vertex times must contain one closed contour.
The course coordinate integrates the **coarea mean-normal contour span**. This
gives a defined approximation for distributing courses over the shell; it does
not solve geodesic distance, uniform local spacing or local short rows. The report
names this method explicitly.

Integer stitch and course counts introduce quantization. `shape_report.json`
retains the requested contour girth, actual stitch count, signed girth residual,
harmonic coordinate and mean-normal distance for every emitted course, plus the
signed overall height residual. Where the terminal bind-off requires completing
a width change one course early, the actual penultimate-course residual is
retained. No residual is hidden by adjusting the source geometry.

## Complete export and portable use

The new output directory contains the ordinary knitting artifacts plus:

- `source_shell.json`: the exact original shell text.
- `shaping_config.json` and `gauge.json`: the explicit drafting choices.
- `shape_report.json`: source identities, method and actual residuals.

The raw source-file digest and canonical shell digest are separate identities.
All geometry, qualification and export checks run before the output directory is
created. An existing destination refuses instead of being overwritten.

`ftx_wasm::knit_shape::knit_shape_json` takes a JSON request with `machine`, the raw
`shell_json` string, structured `config` and `gauge`, optional raw
`qualification_json` and `drape_receipt_json`, optional `material_sha256`, and
optional `created`. Its result includes the same artifacts as a ZIP, file hashes,
the `shape` report, VM report, certificate and any backend-conversion refusal.

In the local Knitting page, choose **3D sleeve/tube shell → shaped knitting**, select
the three input files, and run the export. The measurements mode accepts the
separate measured-pullover inputs described in `measured-knit-drafting.md`. Cancel
stops the numerical worker, and changing an input invalidates stale downloads.

The implementation bounds source size, geometry work, course count and generated
graph size before constructing an unbounded result. The stitch graph cap is
100,000 stitches; this is a work limit, not a peak-memory benchmark.

Passing the discrete VM establishes software-level program consistency. The
export keeps `physical_knitting_validated`, `hardware_execution_authorized` and
`fit_claimed` false. A shell-derived program is not physical evidence of drape,
stretch, head or hand passage, seam quality, or fit on a wearer.
