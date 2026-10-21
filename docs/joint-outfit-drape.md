# Joint contact simulation from independent garment patterns

`ftx drape` accepts `ftx-outfit-drape/1` requests in addition to ordinary `fabric-pattern/1` inputs.
It composes independent garment patterns and runs them **together** through the existing material-aware
mesher, seam welding, cloth Newton solver, body contact and cloth-cloth contact. It is not the frozen
proportions review performed by `ftx outfit`, nor a sequence of independently draped meshes.

## Input and command

The request embeds two to sixteen complete source patterns:

```json
{
  "schema": "ftx-outfit-drape/1",
  "garments": [
    {"id": "shirt", "pattern": {"schema": "fabric-pattern/1", "...": "complete shirt pattern"}},
    {"id": "jacket", "pattern": {"schema": "fabric-pattern/1", "...": "complete jacket pattern"}}
  ],
  "interfaces": []
}
```

The abbreviated pattern objects above are structural illustrations, not runnable patterns. For existing
files, assemble a new request without editing either original:

```sh
python3 - <<'PY'
import json
with open('shirt.json') as f:
    shirt = json.load(f)
with open('jacket.json') as f:
    jacket = json.load(f)
# Different material snapshots require their qualified interface entries, not guessed coefficients.
with open('outfit-interfaces.json') as f:
    interfaces = json.load(f)
with open('outfit.json', 'x') as f:
    json.dump({'schema': 'ftx-outfit-drape/1', 'garments': [
        {'id': 'shirt', 'pattern': shirt}, {'id': 'jacket', 'pattern': jacket}
    ], 'interfaces': interfaces}, f, indent=2)
PY

cargo run -p ftx-cli --bin ftx -- drape outfit.json \
  --measurements measurements.json --model model.ftxbody \
  --out new-joint-drape --self-contact-mm 2 --json
```

The usual admitted body model and measurement inputs are required. `--body-fit` can reuse an accepted
body result through the unchanged body-selection path. No real wearer evidence is included in the
repository. `tests/fixtures/materials/synthetic-joint-outfit.json` is a complete **synthetic two-panel**
input exercised by the real CLI tests against a synthetic Anny-format rig; its placement and material
numbers are not garment presets for an arbitrary body. It is not a ready-to-wear shirt/jacket example.

Cloth-cloth contact must be explicitly enabled. The ordinary material resolver still supplies the actual
sheet thicknesses and friction; the self-contact option is not permission to replace missing interface
evidence. `--fabric` and `--two-start-sew` are refused on this scoped path. Explicit selections and one
joint starting configuration must be retained rather than silently overridden.

## Source preservation and contact

Each garment receives a unique ASCII ID containing letters, digits, underscores or hyphens. Pieces and
physical mirror/fold instances are namespaced as `shirt::front.R`; seam side references receive the same
prefix, while their original edges, direction, ratios, notches and simulation flags are retained. No seam
is introduced between garments. Coincident source IDs therefore cannot weld two unrelated garments.

Vertices, grainlines, cuts and placements are unchanged. Every active source piece needs explicit
placement in the **same body frame**; `sim: false` remains an explicit exclusion. Garment list order does
not mean outer-to-inner order. The solver audits the combined starting configuration and refuses an
intersection or invalid thickness gap. It never invents a layer offset, scales a garment or untangles an
already intersecting assembly to manufacture a starting state. Refine or correct the authored placement
before retrying.

The garment default and all effective piece material selections are resolved and frozen before meshing.
The composed pattern uses those exact snapshots, preserving independent evidence consent, material roles
and existing constitutive/bending/history laws. Internal interface records and request-level additions
are combined. Identical observations shared by different sources are retained once; conflicts and duplicate
entries within one source refuse. Every unlike snapshot pair still needs explicit admitted friction when
self-contact is on. A same-material pair uses the existing sheet self-friction. Stale locks, unsupported
material laws and missing properties are not repaired by the outfit composer.

## Outputs and continuation

The command uses the ordinary create-only drape output directory and retains the existing numerical
acceptance, gating flags, PC2 animation, mesh map, seam/strain OBJ meshes, body mesh and viewer. A short
or unsettled joint run is not relabeled successful merely because its composition was valid. The report
is written last, after all geometry and source artifacts. A failure can leave an incomplete reserved
directory, never a partial success report; retry with a new destination.

Two additional files are published:

- `outfit_request.json`: exact original request bytes, unchanged;
- `outfit_pattern.json`: the actual frozen, namespaced `fabric-pattern/1` used by this run.

`sim_report.json.outfit` records original request/source hashes, the composed-pattern hash, piece/seam
correspondence, retained instance/triangle garment ownership and mass per garment. It asserts no
inter-garment weld and explicitly leaves `fit_claim` false. The original request can contain catalog IDs;
for exact resolved physics, replay the saved composed pattern with the same body, fit specification and
numerical options, including self-contact. That ordinary-pattern replay does not reconstruct the separate
outfit ownership annotation. Keep the request, composed pattern and original report together.

A caller using `ftx_drape::outfit::prepare` and `Prepared::drape` receives an ordinary `DrapeRun`. The
existing checked body-motion API can continue its combined solver without rebuilding its material or
contact fields. The core tests actually sew two independent bands, check their masses and separate welds,
then advance them against a moving body. Another test changes only cross-garment friction and verifies
changed relative slip with identical geometry, mass, membrane response and initial motion.

## Browser material workbench

The material workbench accepts the same `ftx-outfit-drape/1` request in **Original pattern or joint
outfit JSON**. Its WASM adapter calls the production outfit composer before publishing a source pattern.
No JavaScript implementation of garment composition, material resolution or cloth physics is involved.
After a successful import, the original request, initial composition receipt and exact composed pattern
are downloadable as `outfit-request.json`, `outfit-composition.json` and `outfit-pattern.json`.

Assignments can target every outfit piece, every source piece belonging to one garment, or one named
piece. These choices produce ordinary material bindings on the composed pattern. The composer freezes
each effective source-piece material, so changing only **Pattern default** deliberately preserves those
explicit selections; choose a garment or the whole outfit to replace them. Matching interface evidence
can be replaced in the same binding operation. Missing or stale contact evidence still refuses.

The existing drape, fixed-pattern studies, motion/thermal scenarios, cutting, procurement and sewing
controls consume that same pattern. Single-preview consumers use its current bound version; alternative
studies always begin with the original composed version. The self-contact checkbox stays selected and
locked for imported outfits. Scenario requests must include `options.self_contact`; the worker rejects
an omitted contact configuration before constructing the body. Each production consumer still applies
its own material, geometry, body and numerical checks.

Every successful operation keeps the exact original request and initial composition receipt alongside
the exact pattern it consumed or, for binding, produced. The initial receipt describes source ownership
**before substitutions**: its composed-pattern hash is not relabeled as the hash of a changed material.
The binding/study/drape outputs retain their own current source hashes and material evidence. Browser
drape uses the ordinary composed-pattern path, so it does not append the native `Prepared::drape`
per-garment runtime ownership annotation; retain the original composition mapping with those reports.
Replacing the input or refusing an import withdraws the previous binding and downstream results.

## Trims, closures and other attached mass

Each source garment may include its ordinary `ftx-attached-mass/1` document. The composer preserves
the source mass observation, uncertainty bounds, evidence policy, distribution fractions and physical
vertex names. It prefixes group IDs and physical instance references with the garment ID, so two
garments can both contain a `closure-set` without merging their mass. Mirrored instances and unfolded
vertex names keep their existing meaning. The total group mass is distributed once across its targets.

The original attachment document hash (`garments[].attached_mass.source_input_sha256`) and source-to-
composed group correspondence (`garments[].attached_mass.groups`) are retained in the composition
receipt. The actual cloth solver carries the added gravity and inertia through sewing, settling and
subsequent body motion. `sim_report.json.outfit.garment_mass_kg` includes each garment's simulated
cloth and attached mass; `garment_attached_mass_kg` separately reports the accessory contribution.
The existing top-level `mass_kg` remains cloth-only, and `attached_mass` retains its own allocation
and balance report. Replaying `outfit_pattern.json` reproduces the same mass allocation and dynamics.

Missing mass evidence, unadmitted estimates, invalid targets or fractions, and more than 1024 groups
or 65,536 targets across the combined outfit refuse. This models attached point/distributed mass;
it supplies no rigid accessory geometry, contact shape or thermal property. See
[attached-mass.md](attached-mass.md) for the unchanged source observation and targeting contract.

## Scope and limits

This is a simultaneous start/sew/settle solve, not sequential dressing or a physical dressing robot.
The command accepts one shared fit specification on the composed, namespaced pattern; it does not combine
independent source fit verdicts or infer source-specific body landmarks. Existing whole-run numerical
acceptance is not evidence of fit on a real person, nor of plausible initial layering.

The composer preserves drafted `meta.solved.elastic_cut_mm` casings. Each source cut-minus-overlap
length is retained in metres under the composed pattern's `meta.solved.elastic_casings`, alongside its
namespaced physical instance and `load_instances`. Source vertices, scale declarations and original
metadata remain unchanged; the ordinary run restores active casings to cut geometry as it does for a
standalone garment. Multiple casings use independent pairs of existing tension-only rings. The caller's
existing `elastic_ea` option applies to each; its default remains an assumption, not characterized elastic.
With EA zero, rings and cut-geometry restoration are disabled. Invalid authored geometry still refuses.

Explicit-casing reports use `elastics`, with one row per physical casing. Each ideal-ring estimate uses
that casing's material/body friction and its declared garment's simulated inertia, including supported
source-qualified `attached_mass`. It never compares every casing against the entire outfit's weight or
sums their grip capacities. Actual load sharing between casings/garments is not inferred. The original
standalone legacy convention retains its singular `elastic` report. Saved composed patterns retain
their declarations for ordinary `ftx drape` and study replay; keep the same numerical/elastic options.

Unsupported root/fabric fields refuse rather than being discarded. Per-piece supported physics is retained, but no
new garment-level closure, shared thermal boundary, inter-garment thermal coverage or per-garment force
metric is inferred. Browser consumers apply their existing thermal coverage and boundary contracts;
importing an outfit does not create unmeasured thermal interfaces or establish browser/native numerical
parity for arbitrary layered garments.

Inputs are bounded to 64 MiB, 4096 source pieces/seams and the existing material/contact/meshing limits.
Independent synthetic tests establish source isolation, contact causality and software propagation, not
real layered-costume validation. The measured textile and fixed-garment gates in plan §25 / `ftx-tm2`
and `ftx-tm8` remain open.
