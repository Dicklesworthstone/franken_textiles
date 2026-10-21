# Whole-outfit review from native drapes

`ftx outfit` implements the native frozen-garment composition in spec 04 §9.9. It
lets you inspect several garments on the same body without rerunning their cloth
simulations, loading the Anny model again, or installing Blender.

```sh
ftx outfit --measurements measurements.json \
  --sim jacket-drape --sim trousers-drape \
  --out outfit-review --json
```

The result is **a proportions review, not a new simulation**. Each garment keeps
its own final positions. Inter-garment overlaps are neither resolved nor certified
absent. A successful visualization is not evidence that the combined outfit fits
or that its layers can coexist physically.

For a new joint cloth/contact solve from independent source patterns, use the
`ftx drape` outfit-request path described in [joint-outfit-drape.md](joint-outfit-drape.md).
It does not turn this frozen-review command into a simulation or reuse its final meshes.

## Required inputs

Supply one to sixteen completed `ftx drape` directories. Each must contain its
`sim_report.json`, `fold.pc2`, `garment.obj`, and `body.obj`. They may also be the
`sim` subdirectories of completed `ftx round --sim` rounds. No input is modified.

`--measurements` must contain the **exact bytes** used by every source drape.
For a workspace round this is that round's `measurements.json`, not necessarily
its original `measurements.source.json` or the workspace's current evidence.
Reformatting otherwise equivalent JSON changes its fingerprint. The command
validates the supplied document and checks the report fingerprints, every body's
vertex positions and topology, and the declared arm angles. Different bodies or
poses are refused rather than silently moving or refitting a garment.

The shared PC2 reader checks the complete cache, including early-frame finite
coordinates. Its final frame must agree exactly with `garment.obj` after the
OBJ coordinates are rounded to the cache's float32 precision. Vertex, face and
animation-frame counts must also agree with the report. Stale or damaged caches,
duplicate directory aliases and incomplete reports are refused. These consistency
checks are not an authenticity certificate for manually edited input files.

## Outputs and controls

`--out` must name a new directory. The command computes all four stills before
creating it and writes `outfit.json` last as its completion record. The outputs are:

- `outfit_front.png`, `outfit_three_quarter.png`, `outfit_side.png`, `outfit_back.png`;
- `report.html`, a static review with relative local image/data links and escaped
  source labels and flags;
- `outfit.json` (`ftx-outfit/1`), with source hashes, pose checks, final-frame
  selection, original gating/advisory flags, settledness, bands and hem readings.

The default resolution is 960×960. `--res 960x720` changes it; camera framing fits
both image axes. `--seams` keeps the source per-vertex colours instead of the
reference's plain navy bolero/red other-garment palette. Missing source colours
are an error in seam mode, not a reason to invent them. The HTML and images make
no external network requests. Keep the directory together when moving the review.

The JSON result uses `VISUALIZATION_ONLY` for settled sources with no gating
flags (exit 0), or `VISUALIZATION_WITH_FLAGS` when any source is unsettled or gated
(exit 2). Source problems remain visible; they are not converted into successful
fit checks. Invalid or incompatible inputs return a structured refusal (exit 3),
and input/geometry/rendering budgets return exit 4. The limits are 64 MiB per
source file, 250,000 total vertices and 500,000 total triangles, plus the shared
16-million-pixel per-view rendering budget. Budgets refuse, never truncate.

## Optional measured fez

```sh
ftx outfit --measurements measurements.json \
  --sim jacket-drape --sim trousers-drape --hat fez/pattern.json \
  --out outfit-with-fez --json
```

`--hat` accepts a valid metric pattern produced by `ftx draft fez`. The finished
base/top girths and height define a 64-segment frustum with an open base, a 2 mm
inward radial wall and a 2 mm top cap. Its pattern hash, dimensions, placement
basis and base/top heights above the floor are recorded in `outfit.json`.

The hat is **schematic, not draped or fitted**; hat/body contact is not simulated.
Placement uses the supplied measurements' analytic head centre and neck axis,
offset by the lowest referenced vertex in the saved body. This handles native
root-relative coordinates without moving any source garment or body vertex.
The full measurement set needed for the analytic neck placement is required;
a stature/head-only file is insufficient. Adding a hat cannot clear source
simulation flags or turn exit 2 into exit 0. The gallery labels this boundary.

The hat counts toward the existing combined geometry budget. Invalid dimensions
and missing/incorrect pattern metadata refuse before any review is published.
At least one completed garment run is still required; hat-only views are not
implemented. Three additional CLI regressions cover finished dimensions, root
offsets, unchanged source files, preserved flags and rejected hat inputs. A shell
unit test checks that every triangle edge has two opposite orientations and that
all faces have nonzero area. These checks do not certify real-wearer appearance.

## Direct drape outputs are immutable

```sh
ftx drape pattern.json --measurements measurements.json \
  --fit fit_spec.json --out new-drape --self-contact-mm 2 --json
```

The `--out` directory must **not exist**, even if empty. Do not pre-create it in an
adapter. A new directory is reserved before body fitting; existing completed or
failed runs, symlinks and dangling links are refused. All cache, map, viewer and
mesh serialization finishes before publication. Files are created exclusively,
and `sim_report.json` is written after all six geometry/viewer artifacts succeed.
A failed run can leave an incomplete directory for inspection, but never reuses an
old success at that destination. Retry with a new path. Round-created simulation
subdirectories already follow this rule.

## Parity and verification boundary

This is a supported subset, not full parity with `blender_outfit.py`: no
tassel, turntable video, Blender `.blend` output, or joint garment
simulation. Only completed native drapes are accepted. The native renderer is not
claimed pixel-identical to Blender. `--arm-deg` cannot re-pose frozen garments;
regenerate the source drapes at the desired angle instead.

Two deliberate stronger refusals replace reference warnings/reuse: mismatched or
missing measurement/body/pose evidence refuses the outfit, and existing output
directories are never reused. The data report explicitly labels visualization
rather than inheriting any garment's convergence as an outfit-fit verdict.

Seven real-CLI synthetic regressions cover final-cache use, deterministic outputs,
source flags and HTML escaping, body/pose/evidence mismatches, cache corruption,
exclusive output, aliases and seam colours. Three direct-publication regressions
exercise model-load failure, write failures at each artifact, complete output,
private file permissions and no-clobber/link behavior. They require no personal
data or body-model installation. These tests are not a dressed-person fit check.
