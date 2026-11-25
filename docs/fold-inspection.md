# Inspect a completed native fold-up

## Start with independent left and right arm poses

The ordinary drape, placement preview, chest-band qualification and simulated round accept a
complete pair of straight-arm abductions measured from vertical:

```sh
ftx place pattern.json --measurements measurements.json --model /path/to/anny-model \
  --left-arm-deg 30 --right-arm-deg 70 --out new-placement-directory --json
ftx drape pattern.json --measurements measurements.json --model /path/to/anny-model \
  --left-arm-deg 30 --right-arm-deg 70 --out new-drape-directory --json
```

Both flags are required together; each angle must be finite and within 0..=180 degrees.
The pair overrides `--arm-deg`. With distinct angles, generated sleeves use each physical
arm's posed frame instead of reflecting the right sleeve's frame. Omitted pairs and explicit
equal pairs retain the existing symmetric placement convention. Flat pattern geometry and
the fitted shape's reference tape readings remain unchanged. Actual body and cloth contact
checks still decide whether a requested start can be simulated.

The workshop and material workbench expose the same **independent initial arms** choice.
Selected-body previews, transfers, saved cases and motion studies retain the complete pair.
Simulation reports and exported or rebuilt viewers name both angles; whole-outfit reviews
require matching actual arm poses across their component drapes. Continuing motion starts
from these initial angles, while each endpoint still owns its explicitly requested pose.

## Adjust a sleeve's starting position without changing its cut

When a sleeve's initial tube intersects a hand, the piece's `placement` record can now
shift and tilt the wrap axis explicitly. Keep the existing anchors and radius, and add
fields such as this illustrative adjustment:

```json
{
  "frame_offset_mm": [8, 0, 0],
  "frame_rotation": {"axis": [0, 1, 0], "angle_deg": 6}
}
```

Both vectors use the selected body's original local axes in the order **front, lateral,
up**. Here the translation is 8 mm toward the local front, and the rotation is 6 degrees
about the local lateral axis. Rotation pivots at the wrap centerline's `y0_landmark`, then
translation applies in the original axes. Angles must be between −180 and 180 degrees;
an axis must be finite and nonzero. Omit either field when it is not needed.

These are authored positioning controls, not dimensions inferred from the hand. They
retain the pattern's flat vertices, cut length, seams and grain. A generated mirror pair
uses the adjusted right frame and its reflection for symmetric arm poses; independent initial
arms use each physical limb's frame. A quantity-one panel with `placement.side`
uses its independently selected physical side. The existing leg midline clamp still applies.

Save the revised pattern under a new filename and inspect its actual placement:

```sh
ftx place revised-pattern.json --measurements measurements.json --model /path/to/anny-model \
  --hands knuckle --out new-placement-directory --json
```

Review `wrap/` and `start/` separately: sewing and relaxation can move cloth away from its
authored cylinder. `placement.json` retains the authored controls and each instance's exact
`world_frame`, including generated mirroring; the `body` section retains the original body
frames. Its `display_contact` section checks both stages against the full displayed body,
including fingers omitted from a trimmed collision proxy, and against the cloth itself.
`FULL DISPLAY CROSSING` identifies an intersecting stage in the human-readable output.
A clear preview means only that these two drawn stages have no proper triangle crossings;
it does not establish thickness clearance or validate intervening motion. A crossing in the
`start` preview is not a refusal either: a drape first pushes its start out of the collision
body, and the 92 cm bolero's 350 to 360 mm sleeves cross the hand here and drape with
`--hands full`. Where the part crossed is left out of the collider, pushing out cannot help
and the drape can only begin from an earlier checkpoint that clears it. A drape may choose
an earlier contact-admitted relaxation checkpoint than the fully relaxed start drawn here.
When self-contact is enabled, that checkpoint must also clear the full displayed-body audit.
The same bounded search now runs when a finger omitted from the proxy crosses the fully relaxed
start. If body clearance already selected an earlier complete relaxation block, the joint search
retains the original rejected iteration count: later fine checkpoints can still clear both the
collision body and the displayed body. Every candidate is rebuilt and checked against the same
material domains and contact requirements. The drape reports the chosen iteration count and its
actual strain; if no checkpoint passes, the original refusal is retained.
Run the revised pattern through the ordinary drape, with the same body and collider options,
to test its actual contact and convergence.
If the cloth crosses itself at start admission, `refused/still_*.png` shows the actual
start after the body push-out. Yellow marks the first reported crossing, and the repair
names the involved pattern instances. Use those stills with the refused round's pixel
inspector to locate the source pattern edges.
No particular adjustment here qualifies the 385 mm sleeve or establishes physical fit.
Tried on 2026-11-22 without success: a 3.08 degree tilt about `[19, -6, 0]`, which centres the
cylinder on the hand at the thumb, with cylinders of 40 to 55 mm and cuffs of 201.7 and 230.0 mm
(spec 05, "Clothing collider", has each case).

## Optional static-equilibrium comparison

An ordinary garment run can request a separate conservative equilibrium solve after its dynamic
sewing, draping and settling schedule:

```sh
ftx drape pattern.json --measurements measurements.json --model /path/to/anny-model \
  --out new-drape-directory --static-equilibrium --static-tol-mm 0.001 --static-max-newton 300
```

The same switches work with `ftx round --sim` or `--stills`. The tolerance is a Newton correction
**distance** (default 0.001 mm), distinct from the dynamic solver's correction-per-time tolerance.
The Newton cap is explicitly bounded to 1–1000 iterations. Supplying a static tolerance or cap
without `--static-equilibrium` refuses before reading input or creating output.

The existing solver minimizes elastic/gravitational energy with normal contact on a cloned final
dynamic state. **This comparison excludes Coulomb friction and advances no simulated time.**
It cannot establish a friction-supported waistband hold, material-history relaxation, a unique
equilibrium or physical fit. Maxwell/permanent-set history laws retain the solver's explicit
refusal. An incomplete physical contact-thickness ramp also refuses the comparison.
An explicitly requested `--max-settle-steps` tail continues until both the ordinary quiet window and
the requested body offset are complete, or its step cap is exhausted. A quiet garment with an
unfinished offset retains the offset gate; its tail reports `step_limit` when the cap is used up.

Inspect `sim_report.json` → `static_equilibrium`. An `available` result retains the numerical solve
report, initial and solved welded positions, triangles, mesh/weld and instance mappings,
material/body geometry identities, and maximum/RMS displacement from the dynamic endpoint.
The native command also writes `static_equilibrium.obj`. A `refused` result retains its structured
reason and repairs, and publishes no trial geometry. The completed dynamic run stays available;
its normal exit status and gating flags remain based on the ordinary drape.

`fold.pc2`, `garment.obj`, the viewer, reported dynamic duration, flat/drape metrics, source coverage
and subsequent prescribed motion keep their original dynamic state. The optional OBJ is for
inspecting the separate conservative result. Two-start runs retain a separate comparison in each
run's report while their spread and gates continue to use the dynamic outputs.

Configured portable folds and scenario `options` accept the same explicit policy in metres:

```json
{
  "static_equilibrium": {
    "correction_tolerance_m": 0.000001,
    "max_newton_iterations": 300
  }
}
```

Scenario work reserves the additional state, retained geometry and bounded solve before running
the garment. Omitting this option preserves the ordinary schedule and serialized output.

## Inspect corner displacement

```sh
ftx sim-points path/to/sim --pattern pattern.json --measurements measurements.json
ftx sim-points path/to/sim --pattern pattern.json --measurements measurements.json --json
```

The command reads `fold.pc2`, `mesh_map.json` and `body.obj`; it does not run the solver or
modify the simulation. It compares each torso instance's corner with its unscaled drafted
level (spec 04 §9.8). Mirrored pieces keep their own corner indices. Sleeve/leg corners are
excluded. Rows are sorted by `INSTANCE:VERTEX` and contain `drafted_z`, `final_z` and `rise`,
all in millimetres rounded to one decimal. The JSON result is `ftx-sim-points/1` inside the
usual `ftx-cli/1` envelope.

Final positions are the mean of the last 15 available samples, or all samples when fewer
exist. The output records that count. Native Anny positions are root-relative: the saved
body mesh's lowest used vertex establishes the floor, so an origin offset cannot masquerade
as garment rise. Unused OBJ vertices do not change the floor. Drafted heights use the
supplied measurements and the piece's placement anchor, without `sim_scale`.

Shoulders near their drafted levels with high hems suggest support from below; every
corner low suggests the garment is not held. These are inspection clues, not a fit verdict.
A tail mean alone does not establish settledness. Different supplied measurements or a
modified pattern change the comparison basis; their numbers are not observations of the
wearer. The existing simulation report remains the authority for construction flags.

Input counts, all PC2 coordinates, segment totals, corner identities and instance ranges
must agree. Truncated/overlong caches, non-finite coordinates, invalid references and
unscaled patterns are refused (exit 3); input-budget stops exit 4. PC2 input is capped at
256 MiB. Unlike the old grounded Blender command, this native command requires `body.obj`.
