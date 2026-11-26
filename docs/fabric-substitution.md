# Immutable fabric substitutions

`ftx draft`, `ftx round`, `ftx drape`, `ftx qualify` and `ftx place` accept `--fabric FILE.json`
as an explicit whole-fabric replacement. The supplied object replaces the pattern's effective
`fabric` block; it is never merged with a second physical law.

A source-backed `material_id` or `material` selection is resolved through the shared textile
catalog and evidence policy before body fitting or output reservation. The resolved authored
record is embedded into the pattern with its SHA-256 lock, so later rounds consume the frozen
snapshot rather than reopening mutable source evidence. Legacy scalar `areal_g_m2`,
`tensile_N_m` and `bending_Nm` retain their existing UNCERTAIN semantics.

Draft/workspace outputs created with the option retain `fabric.source.json` and
`fabric_override.json`. A standalone draft with `--fabric` requires a fresh output directory
and will not replace an existing result. Round patterns inherit the effective fabric into the next
round. Direct drape stores the override receipt in `sim_report.json`.

The operation changes no geometry, measurements, or fit contract. It does not promote published
or estimated material data into a fit claim. Flat drafting and export preserve validated source
records without requiring unused mechanical properties. Physical consumers still refuse missing
or unapproved properties they require; drape requires same-sheet friction only when self-contact
is enabled. Invalid directional grain and inconsistent snapshot digests remain fail-closed.

Browser fold-up has no filesystem override switch, but it preflights the pattern's effective
material before the expensive body fit using the same drape resolver. Malformed material evidence
therefore cannot silently fall back to default cloth because a body model happens to be loaded.

## Assign separate materials to one garment

Pass an `ftx-material-bindings/1` file with `--material-bindings` to draft, round, drape,
qualify or place. It uses the same binding operation as `ftx materials bind-changes`. For example, on the
zouave bolero draft, this assigns the default linen selection to inherited panels and a different
source record to the sleeve:

```json
{
  "schema": "ftx-material-bindings/1",
  "default": {"id": "fs_il019_natural"},
  "pieces": {"sleeve": {"id": "fsri/hfm/cotton-sheet/dry/220330-r1"}}
}
```

```sh
ftx draft zouave_bolero --measurements measurements.json --ws jacket --material-bindings bindings.json
ftx round jacket --export
```

Keys in `pieces` are source-piece IDs. Both mirrored sleeve instances inherit the same source
assignment. The binding file can also specify the full interface table or name finished elastic
casings. Pass either a binding file or a whole-fabric override. Use a pattern with source pieces
for per-piece qualification. Joint outfit requests take their materials from their component patterns.

The example exercises flat drafting and export. Running a drape also requires applicable mechanical
and contact properties. Each saved override includes the original input bytes and its digest.
The binding receipt records the source and bound pattern digests and the effective material hash
for every piece. A successful standalone drape or `ftx place` also saves `effective_pattern.json`
alongside its receipt (recording `fabric_override` and `effective_pattern_sha256` in
`sim_report.json` or `placement.json`). The viewer checks that snapshot's digest when rebuilding
from the simulation directory. An explicit replacement pattern must reproduce the same canonical
pattern, including its materials.
