# Heat histories through prescribed garment poses

`ftx-materials thermal-motion MOTION.json` connects saved geometric coverage maps to the
existing conservative material-coordinate remapper and thermal solvers. It carries
nonuniform temperature through pose changes, body retessellation and depth-cell changes,
and writes a resumable state. It does not reset a garment to a reservoir temperature at
each pose. No material names or alternate constitutive models enter the solver.

## Run a sequence

First generate each pose's mapping with the existing command:

```sh
ftx-materials thermal-map rest/sim_report.json rest/body.obj projection.json > rest-map.json
ftx-materials thermal-map posed/sim_report.json posed/body.obj projection.json > posed-map.json
ftx-materials thermal-motion motion.json > heat-run.new.json
```

Use fresh output filenames to retain earlier runs. The referenced drape reports need
complete thermal material selections. `projection.json` and each map use the schemas in
[thermal-coverage.md](thermal-coverage.md). A pose may use a different projection request
or depth discretization, but the source cloth topology, reference areas, material
snapshots and physical layer identities must remain the same. The covered **spatial
material domain** must also remain the same; equal aggregate areas are not sufficient.

Example `motion.json` (geometry paths relative to this file's directory):

```json
{
  "schema": "ftx-thermal-motion/1",
  "initial": {
    "kind": "uniform",
    "time_seconds": 0.0,
    "drape_report": "rest/sim_report.json",
    "coverage": "rest-map.json",
    "temperature_c": 10.0
  },
  "frames": [
    {
      "time_seconds": 2.0,
      "drape_report": "rest/sim_report.json",
      "coverage": "rest-map.json",
      "boundaries": [{
        "inside_c": 30.0,
        "outside_c": 0.0,
        "inside_resistance_m2_k_w": 0.1,
        "outside_resistance_m2_k_w": 0.2
      }],
      "steps": 4
    },
    {
      "time_seconds": 4.0,
      "drape_report": "posed/sim_report.json",
      "coverage": "posed-map.json",
      "boundaries": [{
        "inside_c": 0.0,
        "outside_c": 30.0,
        "inside_resistance_m2_k_w": 0.1,
        "outside_resistance_m2_k_w": 0.2
      }],
      "steps": 4
    }
  ]
}
```

These temperatures, film resistances and times are illustrative, not a measured garment
experiment. Temperatures must lie in the materials' common validity interval. One
boundary entry broadcasts to all body triangles; otherwise supply exactly one entry
per destination body triangle in the map's retained order. Boundary values may change
between intervals. No wind model or film coefficient is inferred.

**Times are physical seconds**, finite, nonnegative and strictly increasing. `steps`
subdivides an interval into equal backward-Euler heat steps. The destination geometry
and boundary fields apply throughout that interval (right-endpoint, frozen-pose
approximation). Correspondence changes instantaneously before its heat steps, with no
elapsed time during remapping. This does not interpolate mechanical motion. Simulation
playback frame numbers or frame rates do not establish physical timestamps.

## Resume without temperature reset

A continuation manifest replaces `initial` with:

```json
{"kind": "resume", "checkpoint": "heat-run.new.json"}
```

Keep the normal top-level schema and `frames`, whose first time must be strictly after
the saved time. The complete previous report can be supplied directly, or its
`checkpoint` object can be saved separately. There is no need to extract temperature
arrays, reselect materials, or manually reconstruct a geometric frame.

The `ftx-thermal-motion-checkpoint/1` wrapper retains physical time and a
`ftx-thermal-motion-state/1` library checkpoint. Restoration rebuilds the source-locked
surface, reruns the geometric projection and compares the complete frame before
admitting the exact saved temperature rows. Cell dimensions and material temperature
domains are checked even when an input's digest has been recomputed. Hashes detect
content changes; they are not authentication or proof that a history was physically
simulated. A source-material edit must start a separately initialized experiment,
not reuse this state through an implicit conversion.

Splitting a sequence at an interval boundary and resuming with the same remaining
intervals/substeps reproduces the uninterrupted final checkpoint in the CLI regression.
Energy totals in each report cover **that invocation only**. Sum physical-energy totals
across segments; do not count the prior segment again as part of a resumed run.

## Conservation and output

The `ftx-thermal-motion-report/1` result includes input/geometry identities, a per-interval
trace, final per-region and per-body-triangle heat fields, a complete checkpoint, work
counts, numerical residuals and explicit scope. No successful partial report is emitted
when a later interval refuses; inputs and previous checkpoints are not modified.

Remapping intersects material-coordinate polygons and through-thickness cell intervals,
then averages **specific-heat integrals**, not temperatures. Stored heat follows allocated
reference mass, not current cloth or body area. The positive heat-capacity law is inverted
within the donor temperature range. It is a first-order mixing operation: coarsening
loses unresolved gradients, and remapping back cannot recreate them. Projection and
remap accuracy must be studied separately from physical time-step refinement.

The trace reports remap energy separately from physical boundary heat:

- `physical_stored_change_j` and `net_boundary_energy_j` are the time-integrated heat
  calculation. The net is accumulated before cancellation of bare-body heat paths.
- `signed_remap_residual_j` and `absolute_remap_residual_j` describe numerical transfer
  imbalance, **not** an environmental heat source. Per-cloth errors cannot cancel out
  unnoticed. `material_energy_change_including_remap_j` includes this residual explicitly.
- `polygon_roundoff_m2` records area changes from canonicalizing floating-point inward
  corners in mathematically convex material footprints. Original geometric receipts
  remain unchanged. Genuine nonconvex polygons refuse; material-allocation and energy
  acceptance tolerances are not relaxed to accommodate missing donors.

The polygon correction addresses a real sewn-band regression: inverse barycentric
roundoff created a microscopic inward edge whose infinite clipping plane discarded a
finite donor wedge. Exact-sign convex hulls are admitted only when their area difference
is within the stated floating-point bound; the original observation/map is never edited.

## Limits and refusal boundaries

There are at most 128 destination intervals, 100,000 total substeps, ten million combined
cell/geometry steps and twenty million projection/remap operations per invocation.
Existing per-projection/remap limits also apply. Bare scenes still consume geometry
budget. Referenced files are limited to 64 MiB each and 256 MiB total bytes read, including
repeated reads; the manifest uses the existing 64 MiB input limit. Compact interval
traces are bounded to 16 MiB and complete pretty-printed reports to less than 64 MiB so
the emitted checkpoint is readable by the same command. Split long schedules at interval
boundaries rather than deleting detail or silently truncating work.

Manual area-only maps cannot initialize motion: the remapper needs verified polygons
and oriented material cells. Changed source materials, cloth remeshing, invalid time or
boundaries, changed receipts, unsupported geometry, out-of-domain temperatures and failed
energy checks refuse. Body coverage itself may change. Each source cloth triangle retains
one first-order through-thickness temperature profile for its currently uncovered material
fraction; covered-to-exposed and exposed-to-covered transfers mix integrated enthalpy in
reference material coordinates. No body or ambient reservoir initializes newly covered cloth.

Uncovered cloth is **adiabatic by default**: stored heat is retained but no environmental
exchange is invented. An interval may add `exposed_boundaries`. One entry broadcasts to all
source cloth triangles; otherwise provide one entry per retained source triangle. The two
faces are the source material column's canonical inside/outside faces, not the nearest body
normal. Supplying exposed boundaries when no cloth is uncovered refuses, catching stale
conditions. This is a first-order state: it does not preserve in-plane gradients between
separate uncovered portions of one source triangle.

## Tests and no-claim boundaries

`crates/ftx-drape/tests/thermal_motion.rs` tests spatial/depth remapping, independent
nonlinear enthalpy, layer-specific laws, body-side reversal, uniform preservation,
checkpoint restart, tamper/domain rejection, all-bare state handling, conservative
covered/exposed/re-covered transitions and explicit exposed-face heating. Three direct
polygon unit tests retain the failing microscopic edge, an independent rational-coordinate
intersection answer, convex invariance and refusal of substantive concavity.

`crates/ftx-cli/tests/thermal_coverage_workflow.rs` runs the actual binary on a synthetic
sewn garment and generated body maps. The motion cases include a translated/retessellated
body, two-to-three depth cells, reversed boundary temperatures, independent reference-mass
energy, exact restart, relative file paths, material edits, malformed/manual maps and
work-budget refusals. It also checks that a fully exposed sewn garment remains adiabatic
without `exposed_boundaries`, then heats only when those face conditions are explicitly
supplied. They do not measure textile behavior.

The model still excludes general folded/concave-body correspondence, mechanical-time
interpolation, cloth remeshing, spatially resolved in-plane exposed-cloth gradients,
physical air-gap transport, moisture, compressed loft, in-plane conduction and human
thermoregulation. Compilation
for WebAssembly is not browser execution-parity evidence. Numerical conservation and
source-locked replay are not measured garment warmth, cold-weather safety, PPE
certification or manufacturing approval.
