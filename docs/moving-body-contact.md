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
Those endpoint requests support separately declared arm angles and fractional seated/squat leg
postures. They reuse the existing joint rotations and occupied source shape while retaining the
selected initial pose, sewn cloth history and all contact/rollback checks described below.

Source-model keyframes can also supply `root_translation_m: [x,y,z]` and
`root_rotation_rad: [rx,ry,rz]`. These are absolute endpoint controls in the existing world frame
(+Z up, facing −Y before rotation; anatomical axes are defined in `docs/spec/05`). The rotation is
an axis-times-angle vector in radians, with finite norm at most π. It rotates the complete posed
source surface about its root-relative origin before the metre translation and collider extraction.
It is not an Euler-angle tuple, a turn about the feet, or an inferred squat descent. Omitted/null
fields mean zero independently at each endpoint; explicit zero retains the original geometry
and its authored receipt. A later endpoint without a rotation returns to the unrotated source pose,
without inheriting a previous turn. Finite-vector and magnitude admission use one deterministic
rotation law with a scaled norm, so tiny nonzero vectors are not silently squared to zero.

By default, root controls prescribe endpoint geometry only. The same checked linear vertex chords
connect those endpoints; a large root turn does not request a rotational arc. Author short explicit
increments or request the source-pose sampling policy below. Gravity, fixed cloth constraints and independently authored world-frame
environments retain their declared frames. Actual body/cloth positions feed the existing contact,
coverage, vapor and qualified liquid-mass/heat paths at accepted samples. Initial placement,
tape diagnostics and accepted body evidence are not moved or refitted. Endpoint reports retain the
exact optional controls beside their computed geometry, and changing either control invalidates
the retained source path.

## Sampling the source pose between authored endpoints

Material scenarios can request intermediate evaluations of the same selected source body:

```json
"motion": {
  "schema": "ftx-drape-motion/1",
  "keyframes": [
    {"time_s": 0.1, "arm_abduction_deg": 55,
     "root_rotation_rad": [0, 0, 1.5707963267948966]},
    {"time_s": 0.2, "arm_abduction_deg": 35}
  ],
  "source_pose_sampling": {"max_step_s": 0.025, "max_segments": 16},
  "max_step_s": 0.01,
  "max_trials": 64,
  "max_pairs": 100000,
  "max_sample_vertices": 500000
}
```

The first interval starts at the exact selected initial pose, including any seated/squat leg
fraction. Each later interval starts at its previous authored endpoint. Supported source joint
angles interpolate on their fixed rest-frame axes; each intermediate pose runs the original
source model's forward kinematics and skinning on the unchanged shape. Root rotation follows the
shortest unit-quaternion arc, so equivalent +π/−π endpoint orientations do not create an
unrequested full turn. Root translation interpolates in the existing fixed world frame. The
source surface is posed and transformed before collision-cap extraction. Neither matrices nor
skin vertices are blended to manufacture an intermediate source pose.

An interval of duration `d` is split into `ceil(d / source_pose_sampling.max_step_s - 1e-12)`
equal segments. The small tolerance preserves integral ratios across floating-point arithmetic;
authored endpoint times and their established source-pose evaluation are retained exactly. The
sampling step must be finite in `1e-9..=1` seconds, and `max_segments` must be in `1..=512` across
the **whole trajectory**. Any resulting interval below the mechanical kernel's `1e-9` second
minimum, including loss of time resolution at a later timestamp, refuses before model work.
This is a time-resolution policy, not an angular error guarantee: increase the source resolution
explicitly for a large turn.

Source sampling and the mechanical `max_step_s` serve different purposes. The former evaluates
new source poses; the latter subdivides the checked vertex chord between two evaluated poses.
The solver may take additional adaptive substeps on each short chord. The existing whole-path
`max_trials`, pair-query cap and retained-sample allowance remain unchanged; the engine refuses
an insufficient budget. Initial plus generated source geometry also must fit the existing
2,000,000-vertex path limit. No policy raises another budget or drops intermediate geometry to
finish a request.

All accepted mechanical samples remain in the result. The optional `source_pose_sampling`
receipt records the exact policy, `generated_segments`, generated `source_times_s` and zero-based
`authored_interval_indices`. Original keyframe receipts remain one per authored endpoint.
Thermal boundaries, liquid supply and independently qualified velocity schedules keep those
original interval indices across every generated source segment. A late mechanical or coupled
thermal refusal restores the entire pre-motion solver, including material and liquid histories.
Omitting the sampling policy, or supplying null, retains the original endpoint-only path and
serialized output.

The swept checks still certify the accepted **short linear vertex chords**, subject to the
existing numerical limits. Sampling does not turn them into exact curved-trajectory CCD or
certify anatomical poses, a production-body motion benchmark, physical fit or comfort.

## Static scene admission

Both the uniform-fabric and resolved-material scene builders audit the supplied
collision surface before placement, nearest-surface relaxation or sewing. The
same exact shared-feature predicates used by motion reject crossing, coincident
and folded-over faces, including defects far from the current garment. Static
admission allows at most 1,000,000 overlapping triangle pairs and refuses an
exhausted audit instead of silently admitting the unchecked remainder.

Admission also requires at most two oppositely traversing faces per shared edge
and one connected face fan at every referenced vertex. Every closed component
must have positive signed volume, evaluated about a component-local origin;
an outward large component cannot hide an inward small one. No source faces or
vertices are repaired automatically. Coherent open surfaces remain supported,
but a mixture of open and closed components is refused because the current
contact containment query applies only to a wholly closed collider. An open
appendage must not disable detection of cloth wholly inside the closed body.

This checks the collision mesh, not a separate display mesh. Valid open surfaces
and legal shared edges remain supported; the low-level `Body::new` and general
nearest-point API are unchanged. Surface separation does not establish closure,
anatomical plausibility, thickness or real fit. Open surfaces do not acquire a
closed-body inside/outside interpretation. Production Anny pose qualification
remains a separate requirement.

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
The starting, requested and trial body surfaces also pass a bounded exact
self-intersection audit. Legal shared vertices and edges are retained; crossing,
coincident and folded-over faces are refused with their source triangle indices.
The audit reuses the existing BVH and shared-feature predicates, does not move
vertices, and refuses rather than treating an exhausted budget as a clear surface.
Between these configurations, nonincident body vertex/triangle and edge/edge
features use the same additive CCD as cloth contact, at zero body-surface offset.
Only the exact shared indexed feature is excluded, not every pair of neighbouring
triangles. The existing noncollapse test remains separate. A shortened CCD bound
causes subdivision before solving cloth; an unresolved crossing refuses the whole
request, including any provisional coupled heat or material history.
Lagged Coulomb friction measures **relative** cloth/body material displacement,
with the same frozen contact weights at both body endpoints. The combined
cloth/body trajectory must pass CCD, conservative face-noncollapse checks and
endpoint intersection/clearance audits before the trial is accepted. Difficult
trials subdivide; non-finite values and exhausted budgets refuse.

The entire call is transactional. A late failure restores positions, velocities,
collider geometry and indices, friction history, symbolic analysis and cached
factors. No partially completed motion report is returned as a successful result.

`MotionOptions` defaults are 1/60 second maximum step, 512 trial attempts,
1,000,000 candidate pairs per swept query or body-surface audit and 4,000,000
retained body-plus-cloth vertex positions. These bound subdivision and output,
not the wall time of a
Newton iteration. The solver's existing Newton/friction/PCG policies still apply.
Contact thickness and cloth material parameters are never relaxed to finish motion.
`MotionReport.pairs_tested` includes completed initial-contact queries, body-surface
audits and continuous body/body feature queries as well as cloth swept queries,
including rejected trials. A body feature query shares one `max_pairs` allowance
across both feature families; no candidate is dropped when it runs out.

## Evidence and boundaries

`tests/moving_contact.rs` covers tunnelling with clear endpoints, co-translation,
self-contact, candidate/geometry refusals, frictional surface transport, a lifting
collider with every returned substep rechecked, interior face collapse, body-surface
intersections at initial/target/intermediate configurations, and rollback after
accepted internal substeps. Valid shared-edge motion retains exact audit work.
The streamed BVH query also matches exhaustive
feature enumeration in the contact module's unit test. Existing static contact
known-answer tests and pinned digest remain unchanged.

This is conservative continuation for an already valid prescribed body path, not
IPC's augmented-Lagrangian treatment of large moving boundary displacements. Fast,
blocked, ill-conditioned or excessive-turning motion may refuse rather than finish.
The exact audits concern sampled configurations; additive CCD additionally bounds
nonincident feature separation along each accepted linear chord, with the same
floating-point and conservative-advancement limits as existing cloth CCD. This is
not an exact time-of-impact solver or a check of an unprovided curved trajectory.
Anatomically valid motion, moving
pins, mesh remapping, pose extraction and human-motion accuracy are not certified.
The returned solver
can continue normally after a successful motion, but success is neither settledness
nor a garment-fit verdict. Body-driven motion can add mechanical energy. No
production Anny motion benchmark or browser-runtime qualification is claimed.
