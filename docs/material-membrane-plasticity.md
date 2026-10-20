# Material-driven permanent membrane stretch

`orthotropic_green_return_map/1` adds bounded, rate-independent permanent in-plane strain to the
existing material selection and cloth solver. A selected panel can retain extension after unloading,
while an explicitly elastic lining keeps its own law. Default and per-piece selections, sewing,
fixed-pattern material scenarios and prescribed-body continuation use the existing mesh, grain and
source ownership. The supplied fixture is entirely synthetic; this delivery adds no measured
textile yields or empirical garment validation.

## Selection and matched observations

A sheet material opts in through its existing `structure` object:

```json
{"membrane_plasticity_model": "orthotropic_green_return_map/1"}
```

For **each** suffix `warp`, `weft`, `shear`, supply these four source-bearing property families:

| Property prefix | Canonical unit | Meaning |
| --- | --- | --- |
| `membrane_yield_` | `N/m` | Positive initial yield of the work-conjugate second-Piola sheet resultant |
| `membrane_hardening_` | `N/m` | Nonnegative directional isotropic hardening; supply explicit zero for perfect plasticity |
| `membrane_strain_limit_` | `1` | Positive symmetric bound on total raw Green strain in that coordinate |
| `membrane_accumulation_limit_` | `1` | Positive bound on accumulated absolute plastic Green-strain increments |

These **12 quantities** and the four elastic observations `warp_stiffness`, `weft_stiffness`,
`poisson_warp_weft` and `shear_stiffness` must share identical, nonempty `conditions.specimen_id`
and `conditions.conditioning`. Elastic stiffnesses use `N/m`; Poisson ratio uses `1`. The compiler
uses the existing plane-stress conversion to form the positive-definite orthotropic tensor.
Methods may differ between measurements of a matched specimen; lots and conditioning states may
not be silently mixed. Source, method, evidence, authored units and optional uncertainty bounds
remain in the ordinary material snapshot. Uncertainty is retained, not propagated through this law.

`ResolvedMaterial::membrane_plasticity()` compiles the normalized observations and binds the full
source-record hash. `Material::compiled_membrane_plasticity(allow_estimates)` also supports direct
canonical-unit compilation without requiring unrelated drape inputs. Ordinary draping still needs
the usual mass, thickness, bending, contact and grain evidence. Estimates require an explicit
selection policy; missing values stay missing. A tensile breaking force or engineering-extension
yield is not automatically a second-Piola/Green-strain calibration. No label such as linen, leather
or performance knit supplies the twelve quantities.

Readiness and drape resolution refuse incomplete or invalid opted-in laws, unknown selectors and a
same-material nonlinear or Maxwell membrane response. The sheet law is unavailable for fiber,
yarn and fill records. An ordinary material without the selector retains its existing behavior.

## Physical coordinates, work and bounds

The law uses raw physical Green strain `e = [E11, E22, 2*E12]` in the reference material frame.
Its conjugate sheet resultants are `S = [S11, S22, S12]` in `N/m`. Warp/wale/grain defines the
first axis. Rigid spatial rotations preserve these coordinates. This is a bounded additive,
phenomenological Green-strain model; it does not claim multiplicative finite-strain plasticity.

Let `z` be accepted signed plastic strain, `a` accumulated absolute plastic strain, `Q` the elastic
sheet tensor, `Y` positive directional yields and `H` nonnegative directional hardening. The trial
potential per **reference** area is:

```text
Phi(e; z, a) = min_dz [
    0.5 * (e-z-dz)^T Q (e-z-dz)
    + sum_i ((Y_i + H_i*a_i)*abs(dz_i) + 0.5*H_i*dz_i^2)
]
```

The associated box-yield return map uses a fixed ordering of the 27 possible
inactive/positive/negative direction sets and verifies the accepted set's original complementarity
equations. For active directions `A`, its condensed stress is
`Q*(e-z-dz)` and its consistent tangent is `Q - Q_:A*(Q_AA+H_AA)^-1*Q_A:`. The element composes
these with the exact Green-strain derivatives, including geometric stiffness. The selected
triangle's contribution replaces its elastic contribution once; it is not added in parallel.

On acceptance, `z <- z+dz` and `a <- a+abs(dz)`. Physical storage is
`0.5*(e-z)^T*Q*(e-z) + sum_i(0.5*H_i*a_i^2)`; plastic dissipation is
`sum_i(Y_i*abs(dz_i))`. Both have units `J/m2`, and retained triangle reference area converts them
to joules. The trial potential contains the change in hardening storage; frozen reactions and
energy reports use the full accepted storage. Dissipated work is reported separately and does not
automatically become a thermal heat source.

Total strain must satisfy every declared symmetric bound. Both the current and proposed permanent
metrics `[[1+2*x[0], x[2]], [x[2], 1+2*x[1]]]`, for `x=e` and `x=z`, must be positive definite.
Accumulated absolute increments must remain within their declared limits. Invalid geometry,
overflow or an exceeded strain/history domain refuses without clamping, resetting or extrapolating.
A positive local permanent metric does not establish global compatibility of a sewn garment.
For a uniaxial, shear-free stress-free state, residual length is `L*sqrt(1+2*z[0])`; `z[0]` itself
is not the fractional extension. Accumulation limits are not fatigue lives or damage thresholds.

## Accepted history and solver use

Each selected retained material triangle starts pristine relative to the **authored reference
metric**, after the existing placement/pre-sewing initialization. Initial wrapping is not inferred
forming history; the first accepted physical step can generate permanent strain from that shape.
To represent a particular loading path, integrate that path explicitly.

`Solver::with_membrane_plasticity(&[Option<PlasticMembrane>])` binds one optional law per retained
triangle after grain mapping and welding. The elastic tensor must match exactly. Selected triangles
require `compression = [1, 1]` and no gather-softening zone; the caller's mapping is never changed
to force compatibility. Maxwell or nonlinear membrane laws may remain on other triangles, while
their combination on the same triangle requires a future jointly characterized model.

Newton trials and line searches read history without advancing it. A converged, audited physical
step stages all material updates with positions, velocities and contact state, then commits them
together. A failed trial leaves the accepted state intact. Cloning and prescribed-body continuation
retain history. Each scenario alternative begins from its own pristine material state. Rebinding
history to changed material/reference ownership refuses. `static_solve` and speed-only `settle`
refuse irreversible models because they do not specify a physical loading path; use explicit steps.

## Workbench and inspection

The complete ordinary selection is
[`synthetic-membrane-plasticity.json`](../tests/fixtures/materials/synthetic-membrane-plasticity.json).
Every number, including its low yields and broad strain limits, is artificial software test data.
For a valid pattern with explicit grain, bind it to a new output path:

```sh
cargo run --locked -p ftx-cli --bin ftx-materials -- bind pattern.json \
  tests/fixtures/materials/synthetic-membrane-plasticity.json bound-pattern.json
cargo run --locked -p ftx-cli --bin ftx-materials -- resolve bound-pattern.json
```

The `bind` command above is the uniform-selection path. For different shell/lining or named-piece
selections, use `ftx-materials bind-changes PATTERN.json CHANGES.json NEW-PATTERN.json` as described
in [per-piece assignments](piece-materials.md). It freezes default and explicit
`pieces[].fabric.material` selections together, preserves geometry and seams, and can supply the
new material pair's qualified contact evidence in the same operation. The output must not already exist. Incomplete
physical evidence may be stored with `simulation_ready: false`, but ordinary resolution refuses to
simulate it. Readiness does not prove a mesh or load will converge. Existing
[material scenarios](material-scenario-studies.md) and [per-piece assignments](piece-materials.md)
consume the same selection; no plasticity-specific command or changed jacket pattern is needed.

The resolved material exposes `membrane_plasticity_law_sha256`. Normal drape reports and the motion
result contain `membrane_plasticity_history`, schema `ftx-membrane-plasticity-history/1`. It retains
accepted step count, law identity, per-triangle vertices/reference area, current and permanent Green
strain, accumulated plastic strain, `elastic_stress_n_m`, stored elastic-plus-hardening energy and
separate `plastic_dissipation_j`. `empirical_validation_claim` and `restart_import` remain false.

For library inspection, `Solver::membrane_plasticity_report()` returns that optional history;
`Solver::membrane_energy()` returns actual membrane storage, including Maxwell branches on other
triangles but excluding bending, contact, inertia and dissipated work.
`Solver::membrane_plastic_dissipation_j()` returns cumulative accepted joules in retained triangle
order, with zero for unselected triangles and `None` when no permanent membrane law is bound.
Each selected history element also exposes its own `plastic_dissipation_j`. These cumulative
values include initial drape work; a later heat history must subtract its time-zero baseline.
`Solver::checked_elastic_forces()` returns internal reactions from frozen accepted material storage
without evolving history, and reports invalid or stale state as a refusal. Source changes, invalid
strain and nonfinite responses must not be interpreted as usable reactions.

## Convert accepted irreversible work to sheet heat

An optional motion consumer can convert a qualified fraction of new irreversible membrane work
into the same sheet's transient sensible heat. It requires an active permanent membrane law and
the normal thermal transport, reference mass and heat-capacity observations. It does not infer a
conversion fraction from the yield law. Add `membrane_plastic_heat_fraction` to each active plastic
material's `properties`; an explicit zero is permitted. This **synthetic example** matches the
fixture above and refers to its existing `sources.test`:

```json
{
  "membrane_plastic_heat_fraction": {
    "value": 0.8,
    "unit": "1",
    "source": "test",
    "evidence": "estimate",
    "method": "Synthetic irreversible-yield heat conversion; not measured textile data",
    "conditions": {
      "specimen_id": "artificial-stretch-A",
      "conditioning": "synthetic constant dry state",
      "work_basis": "irreversible_membrane_plastic_dissipation",
      "temperature_min_c": "-20",
      "temperature_max_c": "80"
    }
  }
}
```

The normalized fraction and any supplied bounds must lie in `[0,1]`; dimensionless `%` values
normalize to unit `1` without rewriting the authored observation. Source and method must be
present, and estimated evidence requires the selection's `allow_estimates: true`. Specimen and
conditioning must match all elastic/plastic observations. The exact `work_basis` above excludes
recoverable elastic and hardening storage. Temperature conditions are decimal strings in degrees
Celsius, finite and ordered, with neither bound below -273.15. Initial and accepted thermal cell
temperatures must remain within that inclusive interval.

`ResolvedMaterial::membrane_plastic_heat_fraction()` performs this additional qualification only
when requested. A missing or semantically invalid heat observation does not make an otherwise
valid mechanical law unavailable. The ordinary material's structural/source/unit checks still
apply. The heat consumer checks each retained mechanical law against the exact selected material
fingerprint, so a material substitution cannot inherit another specimen's conversion.

Enable the consumer by adding this field to an otherwise complete `motion.transient_surface`
policy, described in [material scenarios](material-scenario-studies.md):

```json
{"plastic_dissipation_heat": "uniform_reference_thickness"}
```

For each accepted mechanical interval, the source is the fraction times the increase in that
triangle's cumulative dissipation. The time-zero baseline is taken **after initial drape**;
previous forming work and rejected trials are excluded. Energy is distributed uniformly through
the sheet's reference-thickness cells and over that accepted interval's thermal substeps. Reference
area converts joules to the material column's units; stretching cannot create or multiply heat.
The result's `plastic_dissipation_heat` reports motion-only `plastic_dissipation_j`,
`supplied_heat_j`, `unconverted_dissipation_j`, excluded `initial_plastic_dissipation_j`, and the
material/law identities and authored conversion observations. Unconverted work has no inferred
destination. The energy residual is `stored_change_j - net_boundary_energy_j - supplied_heat_j`.

The existing backward-Euler solver includes the source by an equivalent adiabatic enthalpy preload.
Temperature-dependent heat capacity is integrated and inverted, not approximated as endpoint
heat capacity times temperature rise. The preload must itself fit inside the characterized thermal
domain; excessive source energy refuses even if subsequent cooling might lower the final
temperature. Smaller thermal substeps may resolve that conservative admission limit.

This consumer heats independent sheets only, including their requested film or exterior exchange.
It does not heat `transient_coverage`, infer layer sharing, feed temperature back into mechanics,
or convert bending, Maxwell or contact-friction losses. A thermal refusal publishes no partial
heat history and preserves completed mechanics and other independent consumers.

For direct library capture, `motion::advance_body_with_dissipation` records per-triangle cumulative
work on every accepted `MotionSample`; ordinary `advance_body` omits those arrays. Scenario results
retain them in `initial_sample` and `samples` when qualified heating initializes successfully.
Each captured scalar costs one vertex-equivalent against the motion sample budget. With heating
enabled, every thermal cell/substep conservatively costs **129** `max_cell_steps` units, covering
bounded enthalpy inversion and transport, including steps with zero newly dissipated work.

## Verification and remaining material evidence

```sh
cargo test --locked -p ftx-pattern --test material_membrane_plasticity --test material_plasticity
cargo test --locked -p ftx-drape --test membrane_plasticity
cargo test --locked -p ftx-cli --test plastic_bending_workflow
```

The checks cover independent return/work answers, derivatives, unloading and reversal,
physical metrics, source qualification, nodal reactions, objectivity, welded grain frames and
accepted-state rollback. The sewn workflow exercises real `bind-changes`, a plastic front with an
elastic back, reference area/mass accounting, and exact shared-study/direct motion-history equality.

Heat checks cover conversion qualification, accepted local work capture, source-inclusive
conservation and a sewn/motion workflow with initial work excluded. A late thermal-budget refusal
discards the entire heat history while preserving the complete mechanical trajectory. Analytic
constant-capacity and nonlinear-enthalpy cases independently check the thermal source equation.

The existing model-backed native fixture also generates two otherwise identical synthetic
alternatives with different conversion fractions. It checks positive new dissipation, identical
mechanical samples, different temperatures and source-qualified thermal comparison. Set
`FTX_PLASTIC_HEAT_REPLAY` to its printed fixture directory when running
`FTX_REAL_WASM=1 node --experimental-vm-modules tools/check_materials_frontend.cjs` to compare the
complete saved native report with the compiled WASM worker. This checks actual numerical adapters;
it is not browser navigation or an empirical garment experiment. Synthetic values do not measure
a textile's conversion fraction or establish temperature agreement with a real specimen.

`ftx-ps1` and `ftx-tm4` remain open for matched cyclic warp/weft/shear acquisitions and independent
loading, unloading and reloading checks, plus stiffness degradation, damage and rupture. Joint
Maxwell/nonlinear plastic response, gathered compression, conditioning effects, serialized restart,
remeshing transfer and thermomechanical feedback remain separate missing capabilities. Existing
elastic or relaxation calibration commands do not identify permanent yields or hardening. The
linen/leather/knit substitution and independent garment gates need measured specimens, not the
synthetic fixture's values.
