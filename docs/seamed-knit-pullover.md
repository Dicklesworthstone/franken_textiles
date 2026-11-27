# Four-piece seamed knit pullover

`draft_seamed_pullover` creates a front, back, left sleeve and right sleeve as four
ordinary `ftx-knit/1` documents. Ten explicit manual seams join their actual stitch
and row boundaries. The neck, hem and two cuffs remain open.

The sleeves use the quantized front-plus-back armhole lengths. The assembly keeps
each sleeve's fabric face consistent, pairs its midpoint with the shoulder seam,
and distributes stitch-to-row joins with integer groups that consume every edge
unit exactly once. Tapered selvedge lengths include lateral shaping movement.

## Native workflow

Prepare a specification and relaxed gauge, then run:

```sh
ftx knit pullover pullover-spec.json --gauge gauge.json \
  --machine hand-knit --created 2026-11-27T12:00 --out pullover-round-01
```

The following is a **synthetic software example**, not a real wearer's measurements,
a measured yarn gauge, or a fitted garment. Dimensions include the caller's chosen
ease; the drafter does not infer it.

`pullover-spec.json`:

```json
{
  "name": "synthetic-seamed-pullover",
  "wearer_age_class": "child3_to14",
  "chest_girth_mm": 600.0,
  "hem_girth_mm": 560.0,
  "body_length_mm": 320.0,
  "armhole_depth_mm": 96.0,
  "sleeve_length_mm": 240.0,
  "cuff_girth_mm": 120.0,
  "neck_width_mm": 100.0,
  "front_neck_depth_mm": 16.0,
  "back_neck_depth_mm": 0.0,
  "body_rib_courses": 0,
  "cuff_rib_courses": 0,
  "cam_setting": 13.0,
  "yarn_id": "synthetic-test-yarn",
  "carrier_id": "1",
  "max_seam_length_mismatch_mm": 1.0,
  "has_hood_or_neck_drawstring": false
}
```

`gauge.json`:

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

This example has 24 row edges per armhole half and 19 stitches per sleeve-head
half: 96 mm versus 95 mm. The explicit 1 mm tolerance admits that difference;
a smaller tolerance refuses the garment before creating the output directory.
The result reports the difference instead of silently enlarging a sleeve.

Each piece directory contains its pattern, machine profile, reviewed Knitout,
certificate, SVG/PDF charts and individual instructions. The top level contains
`garment.knit.json`, `assembly.json` and `assembly_instructions.txt`. The complete
bundle is prepared and checked before the CLI reserves a new output directory.

Production machine targets use the existing measured `--qual` and independently
selected `--material-sha256` gates. Gauge, yarn lot, cam and machine conditions must
match. `--drape-receipts RECEIPTS.json` optionally reads a map keyed by `front`,
`back`, `left_sleeve` and `right_sleeve`; each receipt must bind its exact piece
document and selected material. A receipt cannot be reused for another piece.

## Portable and browser bindings

`ftx_wasm::knit::knit_pullover_json` accepts a JSON object with `machine`, `spec`,
`gauge` and optional `created`, `qualification_json`, `material_sha256` and
`drape_receipts_json`. Sealed evidence stays a raw JSON string across JavaScript.
The WASM export of the same name wraps success/refusal in the ordinary envelope.

The result includes a downloadable ZIP, its SHA-256, an exact artifact manifest,
all four certificates, dimensional residuals, openings and any backend conversion
refusals. Requests are limited to 4 MiB, uncompressed artifacts to 64 MiB, and the
encoded response to 96 MiB. The existing single-piece limits are unchanged.
The command and portable binding are available independently of a browser form.

## Construction scope

Zero neck depth leaves the bound center edge unsewn. Positive depth uses the
existing independent shoulder programs and held center loops, which require
manual neckline finishing and remain subject to machine holding limits.

No extra seam-allowance wales, neckband or trim are generated. Requested dimensions
must account for any take-up from the chosen joining method. Sleeve-head width is
derived from armhole matching; upper-arm fit and head passage require independent
checking. This is a manually seamed garment, with separate machine programs for
its pieces. It does not supply seamless sleeve branching or automatic sewing.
Representable Kniterate programs include actual KCode and an independent read-back
and nominal take-down report for each piece. Unsupported machine operations or
fractional cam dials retain an explicit backend refusal with the reviewed Knitout.
No export authorizes hardware execution or claims physical fit.
