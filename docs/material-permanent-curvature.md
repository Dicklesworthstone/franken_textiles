# Material-driven permanent curvature

`orthotropic_curvature_return_map/1` supplies a **rate-independent, orthotropic panel crease
model**, selected through the existing source-bearing textile material. It is not a new database,
a fiber-name lookup, an empirical wrinkle prediction, or a sewing-machine authorization.

The material kernel is in `ftx-pattern::material_plasticity`; the existing normal-projected panel
kinematics and sparse Newton solver consume it in `ftx-drape`. Ordinary material binding,
per-piece selections, sewn draping, two-start runs and prescribed-body continuation use this same
path. No plasticity-specific solver flag or alternative mesh pipeline is required. Thermal surfaces
continue to use the actual final solver geometry and the same material snapshots.

## Selection and evidence

A material opts in with both structure keys:

```json
{
  "bending_model": "orthotropic_normal_patch/1",
  "bending_plasticity_model": "orthotropic_curvature_return_map/1"
}
```

This is an extension of the [normal panel model](normal-panel-bending.md), including its grain axes,
engineering twist, preferred curvature, source topology and finite-stretch limitations. The four
elastic tensor entries remain required. Optional preferred curvature remains an all-or-none triple.
The scalar `bending_rigidity` still supplies the separately scoped seam approximation; it is not
a substitute for the panel tensor or a measured seam law.

For **each** suffix `warp`, `weft`, `twist`, supply source-bearing quantities:

| Property prefix | Canonical unit | Meaning |
| --- | --- | --- |
| `bending_yield_` | `N` | Positive initial yield moment per unit reference width |
| `bending_hardening_` | `N*m` | Nonnegative directional isotropic hardening; explicitly supply zero for perfect plasticity |
| `bending_curvature_limit_` | `1/m` | Positive symmetric bound on geometric curvature **departure from the authored preferred shape** |
| `bending_accumulation_limit_` | `1/m` | Positive bound on accumulated absolute plastic-curvature increments |

Every quantity retains its source, method, evidence, conditions, authored units and optional bounds.
Tensor, yield, hardening, validity and any preferred-curvature observations must share identical,
nonempty `conditions.specimen_id` and `conditions.conditioning`. Different measurement methods may
coexist; different lots or conditioning states may not be silently combined. These checks establish
identity consistency, not adequacy of the experiment. A curvature limit is not a fatigue or damage
threshold. Observation uncertainty is retained but not propagated through the current return map.

Estimated quantities require the selection's explicit `allow_estimates: true`. Unknown models,
incomplete directions, non-normal kinematics, nonphysical parameters, source/digest changes and
out-of-domain histories refuse rather than borrowing another textile's data, extrapolating, clamping,
resetting memory, or silently reverting to elasticity. Catalog records without this opt-in retain
their previous behavior; names such as linen, leather or performance knit do not activate it.

## Incremental material law

All curvatures are covariant in the **reference material frame**, ordered `[b_uu, b_vv, 2*b_uv]`,
not current principal curvatures. Let `k` be departure from authored preferred curvature, `z` accepted
permanent curvature, `a` accumulated absolute plastic curvature, `D` the positive-definite sheet
bending tensor, `Y` directional yield moments and `H` nonnegative directional hardening.

For each trial `k`, minimize over a proposed plastic increment `dz`:

```text
Phi(k; z, a) = min_dz [
    0.5 * (k-z-dz)^T D (k-z-dz)
    + sum_i ((Y_i + H_i*a_i)*abs(dz_i) + 0.5*H_i*dz_i^2)
]
```

The kernel enumerates the 27 inactive/positive/negative direction sets, solves active principal
blocks by scaled Cholesky, and checks the original yield/complementarity equations. There is no
stabilizing change to material coefficients. The moment is the first derivative of this condensed
potential. Its consistent tangent is `D - D_:A (D_AA + H_AA)^-1 D_A:`. The panel callback composes
these with exact current-normal curvature gradients and Hessians, including geometric stiffness.
A yielded panel **replaces** its elastic panel contribution; the solver never adds both energies.

After acceptance, `z <- z + dz`, `a <- a + abs(dz)`. Physical storage is elastic bending plus
`sum(0.5*H_i*a_i^2)`. Dissipation is `sum(Y_i*abs(dz_i))`, integrated over **reference** triangle
area. The incremental potential contains the change in hardening storage, not its previous
constant value. Reports distinguish storage from dissipation. Nothing injects this work into the
thermal solver; a validated thermomechanical coupling would require an explicit model and budget.

This law captures a limited form of permanent curvature, elastic unloading and hardening under
reversal. It does not provide kinematic hardening, textile damage, frictional yarn rearrangement,
rate dependence, thermal setting/ironing, humidity effects, seam plasticity or a conforming shell.

## Accepted state and continuation

History starts **pristine relative to the authored preferred shape** after the existing kinematic
placement/pre-sewing initialization. That graphical/kinematic initialization is not an inferred
physical loading history. The first accepted physical step may therefore generate plastic curvature
from its initially wrapped shape. Frame folding/zip animation does not constitute a measured
forming process. To study a specific loading path, prescribe and integrate that path explicitly.

Each retained source-panel patch owns its history, including after seam welding. An explicitly
elastic lining does not inherit the plastic shell's law. Newton trial evaluations, line searches,
friction lag rounds and contact queries read history without mutating it. Positions, velocities,
contact lag/caches, Maxwell memory and crease memory are advanced transactionally only after a
converged, audited physical step. Any failure leaves the previous accepted solver state intact.
Cloning and prescribed-body continuation preserve history; changing its material tensor, target,
rest metric, stencil or source ownership refuses reuse. Silent remeshing/annealing is not supported.

`Solver::static_solve` and speed-only `Solver::settle` refuse this history model: neither specifies
an irreversible loading path. Use explicit time steps. Geometric settledness is not a claim of
stress equilibrium or a stress-free garment. A library clone is supported; serialized restart
import, remeshing transfer and cross-version history migration are not yet implemented.

## Workbench and reports

The complete artificial fixture is `tests/fixtures/materials/synthetic-plastic-bending.json`.
It is **software test data**, not a broadly usable garment preset or measured textile calibration.
On a valid pattern with explicit grain and a sufficiently resolved panel mesh:

```sh
cargo run --bin ftx-materials -- bind pattern.json \
  tests/fixtures/materials/synthetic-plastic-bending.json bound-pattern.json
cargo run --bin ftx-materials -- resolve bound-pattern.json
```

The output pattern must not already exist. Binding preserves pieces, seams and unrelated fabric
metadata and embeds a source-locked selection. Binding can store incomplete evidence, but reports
`simulation_ready: false`; ordinary resolution/draping refuses to consume it. This readiness field
is a **material-input** check, not proof that a given mesh, placement or load will converge.

The normal drape report contains `bending_history` with schema `ftx-bending-history/1`: law identity,
accepted step count, per-patch source-triangle ownership and reference area, authored target,
current curvature departure, permanent/accumulated curvature, elastic moment and stored energy.
`plastic_dissipation_j` is separate from `panel_bending.panel_energy_j`. Numerical history and source
hashes are inspectable; `empirical_validation_claim` and `restart_import` are explicitly false.

A normal `sim_report.json` can then be passed to the existing source/geometry-bound heat workbench:

```sh
cargo run --bin ftx-materials -- thermal-steady-surface sim_report.json \
  tests/fixtures/materials/steady-surface-conditions.json
```

These are declared reservoir/film conditions, not weather, body coverage or a comfort prediction.
The returned replay request binds the actual deformed positions and the same material observations.
Changing stored positions without rebuilding their receipt is refused. It does not infer additional
warmth from a crease, air gap, loft compression or evaporative behavior.

## Executable verification and empirical gate

```sh
cargo test -p ftx-pattern --test material_plasticity
cargo test -p ftx-drape --test plastic_bending
cargo test -p ftx-cli --test plastic_bending_workflow
```

Six kernel tests cover closed-form return maps, 125 signed coupled regimes with independent
finite-difference gradients/tangents, unloading/reversal, monotonic-path subdivision, validity and
source replay. Six drape tests cover all nodal derivatives, objectivity, retained crease reactions,
material-only dynamics, transactional Maxwell/crease rollback, explicit elastic per-piece ownership
on an actual sewn pattern and prescribed-body continuation. Three real-workbench tests check
readiness/refusal consistency, immutable binding and actual drape-to-heat replay without fictitious
thermal dissipation. These demonstrate software propagation, not measured garment agreement.

Beads `ftx-pc1` (delivered software), `ftx-pc2` (matched cyclic specimens) and `ftx-pc3` (conditioning
and process-history qualification) extend `ftx-tm4`. The highest-impact next acceptance is retained,
permitted cyclic moment/curvature and unloaded-shape data for matched linen, treated cowhide and
performance-knit specimens, fitted and tested on independent load reversals and garment shapes.
Static elastic coupon fitting (`calibrate-bending`) must not be advertised as identifying yield or
hardening. The material platform, raw-data, unchanged-jacket and robotic-sewing gates remain open.
