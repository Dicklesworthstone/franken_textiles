# Inspect a completed native fold-up

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
