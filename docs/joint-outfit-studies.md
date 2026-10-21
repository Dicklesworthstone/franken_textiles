# Joint-outfit material, motion and heat studies

Both existing scenario commands accept `ftx-outfit-drape/1` in place of a single `fabric-pattern/1`.
The production composer freezes and namespaces the independently authored garments once. Every material
alternative then starts a **fresh joint drape**, not independent drapes of its garments or a continuation
of the previous alternative's history. Existing numerical acceptance and separate manufacturing,
mechanical and thermal outcomes remain in effect. See `joint-outfit-drape.md` for the source contract.

```sh
cargo run -p ftx-cli --bin ftx -- materials scenario-study \
  outfit.json study.json scenario.json body-input.json model.ftxbody --json > outfit-study.json

# Same computation without the ftx-cli/1 envelope:
cargo run -p ftx-cli --bin ftx-materials -- scenario-study \
  outfit.json study.json scenario.json body-input.json model.ftxbody > outfit-study-direct.json
```

These commands need complete existing request documents and an admitted body model; no generic fixture
can prescribe plausible placements for an arbitrary body. `scenario.options.self_contact` must be
explicitly enabled. The initial outfit must already have admitted mechanical properties, interfaces and
placements before body loading; this entry point does not repair an invalid source composition by
substituting a later alternative. Ordinary `fabric-pattern/1` study inputs retain their existing
per-consumer validation and partial-coverage behavior.

## Substitutions and retained source identity

The composer freezes an **explicit selection on every source piece**. Material alternatives must address
names such as `jacket::front` in `changes.pieces`; changing `changes.default` alone cannot override them.
Seam, fit, layer-map and manufacturing inputs likewise use the composed names. Every distinct replacement
snapshot still needs its own admitted contact-pair evidence. Even a thermal-only edit changes the
material identity: an old pair cannot be silently relabeled. A refused physical alternative does not
erase independent cutting or other available manufacturing outcomes.

The returned `scenario.outfit` retains the parsed original `source_request`, source garment/piece/seam
correspondence, composition digest and the exact baseline `pattern_json` string. This is the canonical
document identity, not the original file-byte identity used by direct `ftx drape`. Save that string
unchanged and pass it as an ordinary pattern with the same request, scenario and body to replay the
manufacturing and simulation results. Per-alternative frozen patterns remain in the existing binding
results. Ordinary-pattern replay omits the separate outfit annotation; it does not change the physics.

```sh
python3 - <<'PY'
import json
with open('outfit-study.json') as f:
    report = json.load(f)['result']
with open('composed-study-pattern.json', 'x') as f:
    f.write(report['scenario']['outfit']['pattern_json'])
PY
```

The original source documents are not modified. The shared preparation retains no body, mesh, solver or
thermal state between alternatives. Each resulting binding and simulation has its own material hashes;
the initial composition receipt is not misrepresented as the identity of a substituted material state.

## Motion and explicitly mapped layered heat

Existing scenario `motion` continues each accepted combined solver with the same body path, contact
interfaces, attachments and constitutive histories. Optional `motion.transient_surface` evaluates the
requested **independent-sheet** heat histories. Those sheet powers must not be summed and described as
whole-wearer heat loss through a layered outfit.

For series heat flow through overlapping garments, supply the existing explicit `coverage` request:
body triangle IDs, namespaced physical `instance_layers` such as `shirt::front.R -> lining`, projection
settings, thermal interface resistances and boundary conditions. The production geometric projection
determines covered portions and their order under its declared local-normal assumptions; garment list
order is not thermal ordering. It can produce a region containing portions of both garments in series
while retaining uncovered body paths. No thermal contact resistance, air gap, wearer boundary or
convection coefficient is invented by this adapter. `motion.transient_coverage` uses the same existing
mapped-layer continuation when explicitly supplied and supported, not a separate outfit heat solver.

The shared boundary is `ftx_drape::study_input::PreparedInput`. The existing file-free
`ftx_wasm::materials::scenario::{on_body,on_model}` interfaces use it too, retaining their input limits
and ordinary-pattern behavior. The separate browser outfit importer can still provide its already
composed ordinary pattern; this addition does not replace its source/assignment controls.

## Verification and physical scope

Synthetic native-command tests run both binaries, retain accessory source inputs, continue a joint body
motion, and show that changing only thermal conductivity preserves final positions and velocities while
changing heat output. A file-free adapter test checks an actual two-garment series region, changed body
heat flow, equality with the direct production consumers and exact frozen-pattern replay. Missing pair
evidence is refused independently; contact-disabled inputs fail before attempting model loading.

These tests establish the software connection, not measured layered-garment insulation, a real person's
fit or comfort, sequential dressing, inferred thermal topology, pressure-dependent interlayer conductance
or heat stored in accessories. The optional source-qualified histories and thermal closures keep their
existing applicability limits. No new browser page, browser-runtime parity result, empirical textile
qualification, complete-cycle manufacturing claim or hardware authorization follows from this feature.
