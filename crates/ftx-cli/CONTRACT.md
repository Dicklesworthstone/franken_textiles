# ftx-cli contract

**Purpose.** The `ftx` binary (plan §14): the reference's `fab` commands, so the skill's scripts can call
FrankenTextiles subsystem by subsystem as each reaches parity. This first cut has `ftx draft`, `ftx validate`,
`ftx drape`, `ftx body`, `ftx alter`, `ftx measure`, `ftx export` and `ftx check`; `round`, `trace`,
`viewer` and `report` follow their subsystems.

**Commands.**
- `ftx draft zouave_bolero|zouave_trousers|fez --measurements M --out DIR [--style S.json] [--json]`: the
  reference's `drafting.py` on `ftx_pattern::draft`. Writes `DIR/pattern.json` and `DIR/fit_spec.json`
  (replacing them, as the reference does; Python's number formatting, keys sorted) and prints
  `{pattern, fit_spec, structure_errors, solved}`; exit 0, or 2 with structure errors (the reference exits
  1). An unknown program or style key, a fez style out of range and a missing measurement are refusals
  (exit 3) and write nothing.
- `ftx validate PATTERN --measurements M [--fit F] [--json]`: the flat validation of `ftx-pattern`, the
  reference's text table, exit 0 CONVERGED, 2 NOT_CONVERGED, 3 BLOCKED.
- `ftx drape PATTERN --measurements M --out DIR [--fit F] [--sew N] [--drape N] [--elastic-ea N] [--model PATH]
  [--self-contact-mm MM] [--self-mu MU] [--json]` (`--elastic-ea`: the stiffness EA in newtons of a drafted
  casing's elastic, default 15; 0 simulates the casing at its finished size as the reference does):
  an Anny body of the wearer's age (`wearer.age_months`) and sex (`wearer.sex`; unknown = sexes
  averaged), fitted on the drape pose to whichever of the 21 tape measurements of
  `ftx_body::TAPE_FIELDS` the file has fused values for (the stature from `wearer.stature_mm` first) and
  to `wearer.weight_kg` or `wearer.bmi` when given (`ftx_body::fit_tape`; `Pose::drape`, hands and feet
  left out of contact), then
  `ftx_drape::run::drape`, writing `sim_report.json` (the metrics and flags plus run fields: pattern path
  as given, measurements SHA-256, wall time, and `body` with the fit's target and body value per
  measurement, its free parameters, every tape reading and the body's mass; with `wearer.weight_kg` or
  `wearer.bmi` the mass is fitted too), `fold.pc2`, `mesh_map.json`,
  `fold_viewer.html`, and the last frame as OBJ for renders elsewhere (metres, z up): `garment.obj` with
  per-vertex seam colours and `garment_strain.obj` with strain colours (`v x y z r g b`, spec 04 §9.4),
  and `body.obj` (the full posed skin). Exit 0 when no gating flag, 2 with gating flags. Without `--fit`, no girth landmarks
  are read. `--self-contact-mm` opts into cloth–cloth contact on the welded solver mesh, with MM
  the sum of the layers' half-thicknesses (0 is allowed). `--self-mu` sets its independent friction
  coefficient (default 0.4); it is refused unless self-contact is enabled. Numerical options are
  validated before file IO or body fitting. Starts with crossing/touching cloth layers are refused,
  not silently untangled. Without the option, body-only behavior is preserved and output explicitly
  says self-contact was not simulated. Both `sim_report.json` and the CLI result contain a
  `self_contact` object with enabled/audit status, parameters and nearby final gap when enabled.
  This audit covers sewing/draping solver states, not the kinematic fold/zip animation or fit accuracy.
- `ftx body --measurements M --out DIR [--pose drape|rest|arms-up|seated|squat] [--model PATH] [--json]`:
  the same fitted body (fitted on the drape pose, then posed as asked) as `body.obj` (the closed skin)
  and `body.json` (`ftx-body/1`: the fit as `drape` reports it, plus the measurements' SHA-256, the pose,
  the phenotype and every local change in use). Exit 0; an unknown pose is refused. Both reports carry
  `model_fit`: every tape reading the file lacks, read off the fitted body, as a `fabric-measurements/1`
  record with provenance `MODEL_FIT` and the population spread as σ (the chart prior's at the wearer's
  stature and age, else the catalogue's 12 mm), so it can be added to the file as a model value, never
  as a measurement (`tests/body_model_fit.rs`, which needs the model).
- `ftx alter OP PATTERN --out OUT [...]` with OP `slash-spread` (`--piece --axis x|y --at --add`), `move`
  (`--piece --vertex [--dx] [--dy]`), `true-seam` (`--seam [--end start|end]`), `cap-ease` (`--piece
  --seams S1,S2 --target`) or `transfer-curve` (`--from SRC.json:INSTANCE/EDGE`, split at the last `:`,
  `--to PIECE/EDGE`): the reference's `alter.py` (spec 01 §10.7) on `ftx_pattern::alter`. Writes OUT even
  with structure errors and prints the op's history sentence; exit 0, or 2 with the structure errors
  listed (the reference exits 1); a reference error (missing piece or vertex, a `.L` side) is a refusal.
  OUT must be a new path: existing files (including the input itself or an alias of it) are never
  overwritten. Serialization completes before file creation; the write is flushed before success.
  This intentional no-clobber exception preserves previous pattern revisions. Impossible seam/cap
  targets are refused before any output is created, not saved as saturated adjustments.

- `ftx export TARGET PATTERN --out DIR [--unfold] [--sa MM] [--mitre-limit N] [--projector-width MM]
  [--layout-width MM] [--keep-blank-pages] [--hpgl-rotate90] [--hpgl-mirror] [--created YYYY-MM-DDTHH:MM]` with
  TARGET `svg`, `pdf`, `letter`, `a4`, `copyshop`, `projector`, `dxf`, `astm`, `aama`, `flat-dxf`, `hpgl`,
  `cutter`, `garmentcode`, `embroidery` or `all`: the reference's `export.py` (spec 03 §1.3) on `ftx-export`.
  Writes the target's files into DIR and updates `<STYLE>_export_manifest.json` there (an existing manifest
  keeps its entries and key order, so separate runs share one), printing each file's path. `all` runs every
  target, then every acceptance check with the pattern, printing `FAIL check [file] detail` lines and
  `PASS|FAIL: n/m checks`; exit 0, or 2 when a check fails (the reference exits 1). The structure errors are
  printed to stderr as warnings. A refused target (a pattern traced in pixels: `unscaled`; a projector page
  over 200 in) writes nothing. Without `--created` or the pattern's `meta.created`, the current UTC minute
  (the reference uses local time). `ftx export check DIR` is `ftx check DIR`.
- `ftx check DIR [--pattern-ref PATTERN]`: every acceptance check (spec 03 §8.1) over an export directory,
  printed as above; exit 0, or 2 when one fails. With the pattern the DXF sew references are recomputed and
  the DSTs' couching checked against the braid lines. A file the manifest lists but DIR lacks is a failed
  `file.missing` item (the reference raises); no manifest is a failed `manifest` item.

**Output.** Human text, or with `--json` exactly one JSON envelope on stdout:
`{"schema": "ftx-cli/1", "command", "ok", "exit", "result"}` or, for refused input,
`{..., "ok": false, "exit": 3, "refusal": {code, message, repairs}}`. Progress lines go to stderr.
Malformed option syntax and unknown commands also use that envelope when `--json` is requested.
Duplicate options (except repeated measurement `--value` readings), a flag in place of an option
value, and extra alteration positionals are refused rather than silently changing the requested
edit. Negative numeric values remain valid.

**Error model.** Every failure is a `Refusal` printed with its repairs, exit 3: an option the command
does not take (`invalid_document`, so a misspelt `--fit` cannot silently drop the fit spec), unreadable
or malformed files (`io`, `invalid_document`), an output directory that cannot be created (checked
before the drape starts), a wearer without age or stature, a stature the body model cannot reach
at that age (`out_of_range`; other measurements out of reach stay residuals in `body.fit.tape`), a start that crosses the body (`intersecting_start`).

**No claims.** The round and its statuses; stills and videos (the skill renders stills from the OBJs);
a body that matches the wearer anywhere but the fitted measurements; speed (a drape takes from about
half a minute to a few minutes).

## Measurement evidence commands

`measure catalogue|prior|init|add|validate|fuse|ready|reference` delegates calculations to
`ftx-measure`; details and examples are in `MEASUREMENTS.md`. Files written by `--out` are
raw domain JSON, while stdout `--json` remains the CLI envelope. The CLI supplies UTC today
unless an explicit date is given; the library is clock-free. Repeated --value readings are
preserved. Invalid edits never replace the input; successful writes use same-directory atomic
rename and mode 0600 on Unix. Exit 3 includes invalid measurement documents and NOT_READY.
The evidence gate distinguishes READY_REFERENCE from ordinary readiness and makes no fit claim.
