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

## Comparing and replaying alternatives

For a source pattern with an existing `inner::waistband` casing, construct a study from your
source-bearing band selections (the bundled band is synthetic, not a commercial recommendation):

```sh
python3 - <<'PY'
import json
with open('SOFT-BAND.json') as f:
    soft = json.load(f)
with open('FIRM-BAND.json') as f:
    firm = json.load(f)
variants = [
    {'key': key, 'changes': {'schema': 'ftx-material-bindings/1',
                            'elastic_casings': {'inner::waistband': selection}}}
    for key, selection in [('soft', soft), ('firm', firm)]
]
with open('band-study.json', 'x') as f:
    json.dump({'schema': 'ftx-material-study/1', 'variants': variants,
               'cutting': {'copies': 1, 'gap_mm': 2.0,
                           'edge_margin_mm': 0.0, 'end_margin_mm': 0.0}}, f)
PY
cargo run -p ftx-cli --bin ftx -- materials study \
  COMPOSED-PATTERN.json band-study.json --json > band-results.json
```

The study above requests cutting as an independent consumer; it needs the same admitted sheet
width and grain evidence as any cutting plan. It does not model band purchasing. To compare actual
motion and grip reports, use the same study document in `materials scenario-study` with a complete
scenario and body model, as described in `joint-outfit-studies.md`. These names and inputs must
refer to the actual composed pattern; neither command guesses a waistband or a body placement.

Each available row retains both `binding.result.pattern_json` and `frozen_changes`. Save the former
string unchanged to replay ordinary drape, or submit the latter to `bind-changes` against the
original source. A denied band is a refused study alternative, not a zero-stiffness candidate;
other alternatives remain independent. With no band edits, existing fallback behavior is retained.

The native integration tests exercise both command binaries, create-only binding, exact saved
selections and repeated study replay. The actual joint-solver test changes only one inner band's
coefficient from 6 to 24 N while keeping an outer 9 N band, cloth, geometry and contact fields fixed.
It verifies the fourfold constitutive force at identical coordinates, changed joint motion, exact
frozen simulation replay and body-motion continuation. These are synthetic numerical tests, not
measured textile or garment-retention validation. The file-free browser adapters use the same
binding/study implementation; native adapter tests are not actual browser execution evidence.

The finished-band coefficient feeds the existing casing rings. No nonlinear band response, band
mass, contact surface, procurement cost, heat capacity or uncertainty propagation is inferred from
axial stiffness. Manufacturing results based only on unchanged sheet geometry remain unchanged;
they are not a purchase or inventory account for the replacement band. Numerical comparisons do not
establish real retention, comfort, seam qualification or garment fit. The empirical gates in
`ftx-tm2`, `ftx-tm8`, and the broader `ftx-ms2` study remain open.
