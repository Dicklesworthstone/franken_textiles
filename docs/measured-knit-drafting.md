# Draft a pullover from body measurements

`ftx knit measured` turns the four canonical girths `chest`, `neck_base`, `upper_arm` and
`wrist` into a complete three-branch pullover stitch graph. The existing branched drafter forms
the body and both sleeves, joins them at the underarm, and reduces the joined yoke to the neck.
The same graph then passes through the ordinary knitting compiler, VM and export gates.

Choose either original measurement records or an explicitly accepted body-fit report with its
matching model. The accepted route uses `PreparedPatternBody`, so it reuses that exact shape
without fitting it again. Missing model girths refuse; supplied tapes do not replace missing
anatomy. No body model is required for the measurement-record route.

## Inputs and command

Keep original evidence in a `fabric-measurements/1` document. The command recomputes fusion from
`records`; it does not trust a saved `fused` cache or accept an unexplained flat map of values.
Supply a separate design file containing the machine, relaxed gauge, explicit prior policy and
all garment decisions. For example, this small **synthetic numerical example** describes the
input format, not a physically qualified garment:

```json
{
  "machine": "hand-knit",
  "use_priors": false,
  "gauge": {
    "wales_per_100mm": 20.0,
    "courses_per_100mm": 25.0,
    "needle_pitch_mm": 4.0,
    "yarn_tex": 180.0,
    "thickness_mm": 2.0,
    "relaxed_after_wash": true
  },
  "design": {
    "name": "synthetic-measured-pullover",
    "wearer_age_class": "adult",
    "chest_ease_mm": 21.0,
    "neck_ease_mm": 10.0,
    "upper_arm_ease_mm": 21.0,
    "wrist_ease_mm": -1.0,
    "yoke_depth_mm": 40.0,
    "body_length_below_arm_mm": 80.0,
    "sleeve_length_below_arm_mm": 60.0,
    "hem_rib_courses": 2,
    "collar_rib_courses": 2,
    "cuff_rib_courses": 2,
    "cam_setting": 13.0,
    "yarn_id": "synthetic-yarn",
    "body_carrier_id": "2",
    "left_sleeve_carrier_id": "1",
    "right_sleeve_carrier_id": "3",
    "has_hood_or_neck_drawstring": false
  }
}
```

```sh
ftx knit measured --input design.json --measurements measurements.json \
  --created 2026-11-28T12:00 --out round-001
```

To reuse an accepted fitted shape, add both `--body-fit accepted.json` and `--model model.ftxbody`.
Supplying only one refuses. The original report must be accepted and match the exact model bytes.
When a measurement document or accepted report supplies age, the explicit design class must
agree: younger than 36 months uses `infant_under3`, ages three through fourteen use
`child3_to14`, and older wearers use `adult`. A wearable pullover cannot use
`calibration_swatch_only` exemptions.

Production machine profiles retain the ordinary measured-swatch requirement. Supply
`--qual qualification.json --material-sha256 HEX` when applicable. `--drape-receipt receipt.json`
passes an independently sealed report through the existing exact-document/material gate.
Machine conversion refusals remain visible in the result; no successful export authorizes
hardware execution.

## Which dimensions come from the body

| Finished dimension | Body input | Explicit design contribution |
|---|---|---|
| Chest circumference | `chest` | `chest_ease_mm` |
| Neck circumference | `neck_base` | `neck_ease_mm` |
| Upper sleeve circumference | `upper_arm` | `upper_arm_ease_mm` |
| Cuff circumference | `wrist` | `wrist_ease_mm` |
| Underarm-to-hem length | None | `body_length_below_arm_mm`, including hem rib |
| Plain underarm sleeve length | None | `sleeve_length_below_arm_mm`, with cuff rib added below |
| Underarm-to-neck yoke height | None | `yoke_depth_mm`, with collar rib added above |

Ease is signed and measured around the whole circumference. Negative ease remains explicit;
an impossible result refuses instead of being silently changed. The existing drafter rounds
circumferences to integer stitch multiples and lengths to integer courses. `body_draft.json`
reports every source value, stated ease, requested dimension, actual relaxed-gauge dimension
and signed residual. It also reports total cuff-inclusive sleeve length and collar-inclusive yoke
height. Sleeves are symmetric. Shoulder-to-wrist `arm_length` is not an underarm sleeve length,
so it is never substituted for the explicit garment length.

## Output and evidence

The new output directory contains the ordinary `pattern.knit.json`, `program.k`, `cert.json`,
charts and instructions, plus `body_draft.json`, `measurements.fused.json` and the exact original
`input.source.json` and `measurements.source.json` bytes. Accepted-fit, qualification and drape
receipt source files are retained when supplied. Reusing an existing output directory refuses.

Body girths retain either `FUSED_MEASUREMENTS` or `MODEL_FIT` basis; authored lengths retain
`GARMENT_DESIGN`. Fused inputs also retain each used record's original provenance, uncertainty,
conflict flags and ordinary measurement readiness. Allowing priors does not make them measured
or ready. The accepted route retains its model/report identity and original tape-versus-model
comparisons, with `used_to_refit:false`; absent model uncertainty remains absent. The existing
bodice and closed-cuff sleeve readiness reports remain separate from knitting compilation.

Relaxed girths do not establish head or hand passage, stretch under load, physical knitting
quality or fit on a wearer. Those limits and the compiler's ordinary finishing requirements
remain in the exported artifacts; `fit_claimed`, `physical_knitting_validated` and
`hardware_execution_authorized` remain false.

## Portable and local browser use

The portable `ftx_wasm::knit_body::knit_body_json(request_json, model_bytes)` entry uses the same
body authority, drafter and export path. Request fields are `input_json`, `measurements_json`,
optional `body_fit_json`, `qualification_json`, `material_sha256`, `drape_receipt_json`, and
optional `created`. Each JSON document is passed as its original text string. Model bytes are
a separate byte slice, empty for measurements alone. The browser wrapper exposes the same
`knit_body_json` name and returns a structured success or refusal envelope.

Successful output follows the saved-knit export contract, including `pattern_json`, file hashes,
ZIP bytes as `archive_base64`, `archive_sha256`, `vm_report`, `cert`, machine conversion status,
`drafting` and `body_source`. This computation is local. The source strings and model bytes do
not require an upload.
