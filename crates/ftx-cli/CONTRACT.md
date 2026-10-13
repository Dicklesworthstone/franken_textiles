# ftx-cli contract

**Purpose.** The `ftx` binary (plan §14): the reference's `fab` commands, so the skill's scripts can call
FrankenTextiles subsystem by subsystem as each reaches parity. This first cut has `ftx validate` and
`ftx drape`; `round`, `draft`, `body fit`, `measure`, `export`, `trace`, `alter`, `viewer` and `report`
follow their subsystems.

**Commands.**
- `ftx validate PATTERN --measurements M [--fit F] [--json]`: the flat validation of `ftx-pattern`, the
  reference's text table, exit 0 CONVERGED, 2 NOT_CONVERGED, 3 BLOCKED.
- `ftx drape PATTERN --measurements M --out DIR [--fit F] [--sew N] [--drape N] [--model PATH] [--json]`:
  an Anny body of the wearer's age (`wearer.age_months`) and sex (`wearer.sex`; unknown = sexes
  averaged), fitted on the drape pose to whichever of stature (`wearer.stature_mm`, else the fused
  stature), chest, waist, hip, waist height and crotch height the file has fused values for
  (`ftx_body::fit_tape`; `Pose::drape`, hands and feet left out of contact), then
  `ftx_drape::run::drape`, writing `sim_report.json` (the metrics and flags plus run fields: pattern path
  as given, measurements SHA-256, wall time, and `body` with the fit's target and body value per
  measurement, its free parameters and every tape reading), `fold.pc2`, `mesh_map.json` and
  `fold_viewer.html`. Exit 0 when no gating flag, 2 with gating flags. Without `--fit`, no girth landmarks
  are read.

**Output.** Human text, or with `--json` exactly one JSON envelope on stdout:
`{"schema": "ftx-cli/1", "command", "ok", "exit", "result"}` or, for refused input,
`{..., "ok": false, "exit": 3, "refusal": {code, message, repairs}}`. Progress lines go to stderr.

**Error model.** Every failure is a `Refusal` printed with its repairs, exit 3: an option the command
does not take (`invalid_document`, so a misspelt `--fit` cannot silently drop the fit spec), unreadable
or malformed files (`io`, `invalid_document`), an output directory that cannot be created (checked
before the drape starts), a wearer without age or stature, a stature the body model cannot reach
at that age (`out_of_range`; other measurements out of reach stay residuals in `body.fit.tape`), a start that crosses the body (`intersecting_start`).

**No claims.** The round and its statuses; stills and videos; a body that matches the wearer anywhere
but the six fitted measurements; speed (a drape takes minutes).
