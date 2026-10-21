# Prescribed body motion and cloth response

`ftx_drape::motion::advance_body` continues an existing `Solver` while its collider
moves to a supplied vertex array. This is the moving-contact numerical slice of
plan sections 9.5 and 11.4. Material scenario studies expose it through the native
commands, model-backed WASM adapter and material workbench; the ordinary `ftx drape`
command remains a separate static path.
The array uses **metres, the same world frame, vertex order and topology** as
`solver.contact.body`. A `scene::Scene` exposes this solver as `scene.solver`.
Do not replace the collider by editing its public vertex array: that leaves its
spatial indices stale and loses the swept trajectory and relative-friction input.

```rust,ignore
use ftx_drape::motion::{advance_body, MotionOptions};

// next_body_vertices is an explicitly authored, topology-matched keyframe.
let motion = advance_body(
    &mut scene.solver,
    &next_body_vertices,
    0.1, // physical seconds, not an animation frame number
    MotionOptions::default(),
)?;
for sample in &motion.samples {
    // sample.body and sample.cloth are the accepted endpoint coordinates.
    // sample.cloth uses welded solver indices; scene.weld maps garment indices to them.
    record(sample.time_s, &sample.body, &sample.cloth);
}
```

The body follows a linear vertex trajectory. For articulated rotation, supply
successive short keyframes generated from the same posed body and skin mapping;
a single endpoint chord is not a rotational arc. The returned physical timestamps
may be nonuniform because the integrator subdivides difficult intervals. Preserve
all accepted samples and their timestamps. Drawing one chord between only the
first and last states, or assigning a uniform PC2 rate to adaptive samples, changes
the checked motion. The ordinary static fold-up viewer has no moving-body track.
The material workbench's motion inspector selects exact retained samples, displays
their physical timestamps and uses one fixed metric scale across the trajectory.
See `docs/material-scenario-studies.md` for source-model endpoint requests and the
independent motion, steady-heat and transient-heat consumers.

## What runs

The swept query enumerates cloth-vertex/body-triangle, body-vertex/cloth-triangle,
and edge/edge pairs using the existing frankensim-derived `fs-query` BVH. Enabled
self-contact participates too, retaining its original force-pair exclusions and
per-crease offsets. Bounds include both trajectory endpoints and outward rounding;
pairs are streamed, not stored as a Cartesian product. A candidate cap refuses
rather than dropping collision pairs. The existing additive CCD checks motion
between endpoints, including a collider that crosses cloth and emerges clear.

Each trial first admits the prescribed body motion against the old cloth. It then
runs the existing implicit-Euler/Newton cloth solve at the endpoint collider.
Lagged Coulomb friction measures **relative** cloth/body material displacement,
with the same frozen contact weights at both body endpoints. The combined
cloth/body trajectory must pass CCD, conservative face-noncollapse checks and
endpoint intersection/clearance audits before the trial is accepted. Difficult
trials subdivide; non-finite values and exhausted budgets refuse.

The entire call is transactional. A late failure restores positions, velocities,
collider geometry and indices, friction history, symbolic analysis and cached
factors. No partially completed motion report is returned as a successful result.

`MotionOptions` defaults are 1/60 second maximum step, 512 trial attempts,
1,000,000 candidate pairs per swept query and 4,000,000 retained body-plus-cloth
vertex positions. These bound subdivision and output, not the wall time of a
Newton iteration. The solver's existing Newton/friction/PCG policies still apply.
Contact thickness and cloth material parameters are never relaxed to finish motion.

## Evidence and boundaries

`tests/moving_contact.rs` covers tunnelling with clear endpoints, co-translation,
self-contact, candidate/geometry refusals, frictional surface transport, a lifting
collider with every returned substep rechecked, interior face collapse, and rollback
after accepted internal substeps. The streamed BVH query also matches exhaustive
feature enumeration in the contact module's unit test. Existing static contact
known-answer tests and pinned digest remain unchanged.

This is conservative continuation for an already valid prescribed body path, not
IPC's augmented-Lagrangian treatment of large moving boundary displacements. Fast,
blocked, ill-conditioned or excessive-turning motion may refuse rather than finish.
Body self-intersection, anatomically valid motion, moving pins, mesh remapping,
pose extraction and human-motion accuracy are not certified. The returned solver
can continue normally after a successful motion, but success is neither settledness
nor a garment-fit verdict. Body-driven motion can add mechanical energy. No
production Anny motion benchmark or browser-runtime qualification is claimed.
