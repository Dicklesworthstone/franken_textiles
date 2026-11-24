# Changing wind during a continuous garment motion study

A prescribed motion can change the uniform ambient air velocity between authored
body-pose intervals. The same sewn garment continues through the whole trajectory:
positions, velocities, contact state and material history are retained. Aerodynamic
loading uses cloth velocity relative to the prescribed world-space air velocity.

## Drag across a moving cloth triangle

Enabled aerodynamic drag integrates the triangle's interpolated velocity at three
interior area-quadrature points. The positive, degree-two rule retains resistance
to rotation, flapping and other velocity differences across a triangle even when
its center is stationary. Each point contributes force to its three vertices
through the same barycentric weights used for velocity. Opposing local forces can
therefore produce a resisting torque without a net force.

The step-start area and normal remain fixed during each solve. The potential,
gradient, Hessian and generalized-alpha initial acceleration use the same
distributed law; no additional coupling outside the existing triangle is needed.
The degree-two rule exactly integrates the linear low-speed limit for a linear
triangle velocity field. Nonlinear quadratic drag still requires spatial mesh
refinement; three samples do not resolve arbitrary airflow or subtriangle folds.

No additional option is needed. Existing `air_drag` settings enable this law in
native drapes, portable folds and motion studies. Disabled drag is unchanged, and
omitted wind and explicit zero wind remain equivalent. Active-drag trajectories
with nonuniform triangle velocities change from the former center-only sampling.

## Author a schedule in the Material Workbench

1. Enable aerodynamic drag in the scenario options and choose its density,
   normal/tangential coefficients and velocity regularization.
2. Choose authored motion endpoints and enable **Prescribe wind separately for each
   motion interval**.
3. Enter all three world-space velocity components in metres per second, then Add
   or Replace the endpoint. Explicit zeros prescribe still air.
4. Give every saved endpoint its own wind vector before evaluating or freezing a
   named case. Older endpoints can be completed one at a time.

The vector saved at an endpoint applies throughout the interval ending there.
Unsaved input edits are drafts. Turning the schedule off retains saved vectors
without applying them; endpoints added while it is off do not inherit a vector.
Removing an endpoint removes its wind entry as well. A named case retains the
complete schedule that was saved with it.

The scenario's ordinary ambient velocity controls the initial sewing/draping
phase. The motion schedule starts at motion time zero, after that initial drape.

## Original JSON and native consumers

The optional `air_velocity_schedule_m_s` field belongs to
`ftx-drape-motion/1`, inside `scenario.motion`. It is an array with exactly
one three-component vector for each original authored keyframe:

```json
{
  "schema": "ftx-drape-motion/1",
  "keyframes": [
    {"time_s": 0.4, "arm_abduction_deg": 35},
    {"time_s": 0.8, "arm_abduction_deg": 50},
    {"time_s": 1.2, "arm_abduction_deg": 35}
  ],
  "max_step_s": 0.01,
  "max_trials": 256,
  "max_pairs": 100000,
  "max_sample_vertices": 500000,
  "source_pose_sampling": {"max_step_s": 0.02, "max_segments": 60},
  "air_velocity_schedule_m_s": [
    [1.5, 0, 0],
    [-1, 0, 0.25],
    [0, 0, 0]
  ]
}
```

This example prescribes the following boundaries. The work limits remain explicit
limits, not promises that a particular mesh will finish within them.

| Motion time | World-space air velocity, m/s |
| --- | --- |
| `(0, 0.4]` | `[1.5, 0, 0]` |
| `(0.4, 0.8]` | `[-1, 0, 0.25]` |
| `(0.8, 1.2]` | `[0, 0, 0]` |

The scenario must already contain `options.damping.air_drag`. Its admitted
density, coefficients and regularization stay unchanged. The schedule supplies
only velocities; it does not enable drag or choose coefficients implicitly.
The scenario's aggregate `max_motion_trials` and geometry limits still apply.

Every vector must have exactly three finite numeric components. Missing entries,
wrong lengths and values that overflow squared speed, cubed speed or the actual
drag coefficient scale are refused before model work. The original-file route
preserves the input JSON text through saved scenarios and named-case replay.

Omission or `null` leaves the existing constant-wind behavior unchanged. The
native serializer omits the absent field. Explicitly requesting a schedule,
including an all-zero schedule, requires aerodynamic drag.

## Integration and receipts

Wind is piecewise constant on the **original authored intervals**. Intermediate
source-pose evaluations and adaptive mechanical subdivisions retain the original
interval index; they do not interpolate wind or advance the schedule early.
At a changed wind boundary, the solver clears its cached generalized-alpha
acceleration while retaining the garment's position, velocity and histories.
Repeating the same vector leaves that acceleration history intact.

The requested schedule participates in the motion path's request hash. Editing
wind therefore invalidates an old path binding. Failed trajectories restore the
whole original solver, including damping and acceleration state, rather than
publishing partially advanced cloth.

When requested, the motion result records:

- `initial_sample.air_velocity_m_s`: the initial drape's ambient velocity.
- `samples[].air_velocity_m_s` and `authored_interval_index`: the boundary
  used for each accepted mechanical sample.
- `keyframes[].air_velocity_m_s`: the boundary used to reach each endpoint.
- `air_velocity_schedule_m_s` and `air_velocity_basis`: the complete schedule
  and its interval semantics.

Motion-qualified comparisons reconcile the requested schedule, initial ambient velocity,
every accepted sample's interval and wind, and every endpoint's wind. Missing or changed
receipts exclude the affected motion result while independent manufacturing output remains
available. An unscheduled request cannot acquire injected interval-wind receipt metadata.

After success, the last wind remains installed in the continuing solver. The
original scenario options describe the initial boundary; use the motion receipts
to identify the boundary at the final state.

## Scope and checks

This is a prescribed uniform aerodynamic boundary. It supplies no turbulence,
shielding, spatial airflow solution, measured drag calibration or garment-fit
qualification. Thermal convection retains its own source-qualified conditions;
changing mechanical wind does not silently alter heat-transfer coefficients.

The core regression tests cover admission and omitted-field compatibility,
original-interval mapping under source-pose sampling, generalized-alpha cache
continuity, actual sewn-garment motion against manually prescribed wind
intervals, and complete rollback/replay after a late failure. Frontend tests
cover explicit endpoint completion, invalid inputs, saved cases, replay and
stale-result cancellation.

See [material scenario studies](material-scenario-studies.md) for the surrounding
study workflow and [thermal motion](thermal-motion.md) for the independent heat
histories.
