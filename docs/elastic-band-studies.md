# Change casing bands without changing the garment

The ordinary `ftx-material-bindings/1` request accepts an optional `elastic_casings` map. Keys are
exact **physical casing instance IDs**, including mirror suffixes and outfit prefixes, not cloth
piece IDs, roles or arbitrary names. Values are ordinary finished-band selections consumed by
`linear_tension_only/1`. For example, a composed outfit can change just its trouser band:

```json
{
  "schema": "ftx-material-bindings/1",
  "elastic_casings": {
    "trousers::waistband": {
      "id": "your-characterized-band",
      "record": "REPLACE WITH THE COMPLETE SOURCE-BEARING MATERIAL OBJECT"
    }
  }
}
```

The abbreviated record above illustrates the target, not a runnable material. Supply a complete
record or an existing catalog ID with the model, dimensional whole-band stiffness and evidence
required by `elastic-band-materials.md`. Estimated observations need the selection's own permission.

Use `ftx materials bind-changes PATTERN.json CHANGES.json NEW.json --json` or the existing
`ftx-materials bind-changes` command. The same changes object can be used in each material-study
variant, including joint-outfit scenario studies. The shared browser-facing binding interface uses
the same implementation; this feature does not add a separate interactive band selector.

## Existing geometry is authoritative

An edit must identify an already declared `meta.solved.elastic_casings` entry, or the one physical
waistband under the legacy `elastic_cut_mm` convention. Binding never invents a casing, rest length,
load scope or placement. Explicit entries retain their `relaxed_m` and `load_instances`; legacy
cut length and overlap remain unchanged. Unknown, duplicated, inactive, unplaced or ambiguous targets
refuse. Mirrored casings must be addressed individually. The drape consumer still validates the
complete geometry, load domain, numerical options and starting configuration.

Changing a band does not change its surrounding sheet, source-piece materials, contact-pair hashes,
seam topology, attached masses or thermal observations. Existing callers without `elastic_casings`
retain their original binding/report semantics. An empty map behaves exactly like omission.

## Freeze, replay, and independent alternatives

On an explicit band edit, every selected band in that pattern is resolved and frozen, including
unchanged selected casings. A casing without a material remains an explicit caller-EA fallback; the
binder does not invent a material for it. All replacements and retained band selections must resolve
before a result is returned. Invalid units, missing stiffness, denied estimates and stale source locks
cannot silently become the global fallback.

`frozen_elastic_casings` in the binding report carries the resulting source-locked selections. Material
studies preserve them in each alternative's `frozen_changes`, alongside sheet and interface selections.
Applying those changes again to the original pattern reproduces the exact bound `pattern_json`.
Every alternative begins with the original source, not the preceding alternative's band or history.

The finished-band coefficient feeds the existing casing rings. No nonlinear band response, band
mass, contact surface, procurement cost, heat capacity or uncertainty propagation is inferred from
axial stiffness. Manufacturing results based only on unchanged sheet geometry remain unchanged;
they are not a purchase or inventory account for the replacement band. Numerical comparisons do not
establish real retention, comfort, seam qualification or garment fit. The empirical gates in
`ftx-tm2`, `ftx-tm8`, and the broader `ftx-ms2` study remain open.
