# Immutable fabric substitutions

`ftx draft`, `ftx round` and `ftx drape` accept `--fabric FILE.json` as an explicit
whole-fabric replacement. The supplied object replaces the pattern's effective `fabric` block;
it is never merged with a second physical law.

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
or estimated material data into a fit claim. Missing or unapproved load-bearing properties,
invalid directional grain, and inconsistent snapshot digests retain the material resolver's
fail-closed behavior.

Browser fold-up has no filesystem override switch, but it preflights the pattern's effective
material before the expensive body fit using the same drape resolver. Malformed material evidence
therefore cannot silently fall back to default cloth because a body model happens to be loaded.
