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
`Solver::checked_elastic_forces()` returns internal reactions from frozen accepted material storage
without evolving history, and reports invalid or stale state as a refusal. Source changes, invalid
strain and nonfinite responses must not be interpreted as usable reactions.

## Verification and remaining material evidence

```sh
cargo test --locked -p ftx-pattern --test material_membrane_plasticity --test material_plasticity
cargo test --locked -p ftx-drape --test membrane_plasticity
cargo test --locked -p ftx-cli --test plastic_bending_workflow
```

The focused checks cover independent return/work answers, derivatives, unloading and reversal,
physical metrics, source qualification, nodal reactions, objectivity, welded grain frames and
accepted-state rollback. All **76 focused Rust tests** pass: 23 material-law/pressure tests,
25 solver/history tests, 10 native command/workflow tests and 18 shared WASM API tests. The new law
has 7 material tests, 9 solver integration tests and 1 solver unit test; existing bending and Maxwell
checks remain included. The sewn workflow exercises real `bind-changes`, a plastic front with an
elastic back, reference area/mass accounting, and exact shared-study/direct motion-history equality.

Pinned `cargo fmt --check`, strict workspace all-target Clippy and the
`wasm32-unknown-unknown` build pass. The compiled WASM and actual worker match the native executable
for permanent-law selection and source identity, including material-only yield changes and independent
readiness refusals, through `tools/check_materials_wasm.cjs`.

The complete workspace test run did not finish: `rustc` was killed with SIGKILL in the 8 GiB test
environment, including a retry with `-j1` and single-threaded tests. The separate Chromium check
also did not finish; its page crashed before the first catalog query. Neither complete-workspace
success nor browser navigation/model-backed browser draping is asserted. These checks establish
synthetic software behavior, not measured textile or garment agreement.

`ftx-ps1` and `ftx-tm4` remain open for matched cyclic warp/weft/shear acquisitions and independent
loading, unloading and reloading checks, plus stiffness degradation, damage and rupture. Joint
Maxwell/nonlinear plastic response, gathered compression, conditioning effects, serialized restart,
remeshing transfer and thermomechanical feedback remain separate missing capabilities. Existing
elastic or relaxation calibration commands do not identify permanent yields or hardening. The
linen/leather/knit substitution and independent garment gates need measured specimens, not the
synthetic fixture's values.
