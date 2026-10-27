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
same-material nonlinear membrane response. Maxwell memory on the same triangle requires an explicit
source-qualified [parallel network](material-parallel-memory.md); independent laws still refuse by
default. The sheet law is unavailable for fiber, yarn and fill records. An ordinary material without
the selector retains its existing behavior.

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
to force compatibility. Maxwell or nonlinear membrane laws may remain on other triangles. A qualified
parallel Maxwell network uses the atomic `Solver::with_membrane_histories` binder; the individual
binders refuse joint half-laws. Same-triangle nonlinear plasticity remains unsupported.

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
triangles or in an explicitly qualified parallel network, but excluding bending, contact, inertia
and dissipated work.
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

## Identify yield and hardening from acquired histories

`ftx-membrane-plasticity-calibration/1` identifies the **six yield and hardening coefficients** of
the law above. An optional single-arm joint mode also identifies a proportional Maxwell ratio and
time. Both modes keep the four elastic observations and all six strain/accumulation limits fixed.
Those ten source-bearing quantities must already be present in `material.properties`, with matching
`specimen_id`, `conditioning` and any additional acquisition conditions. A range explored by the
optimizer is not a material validity interval or a confidence bound. Total strain and accumulation
limits must come from independent characterization; the calibration never derives them from the
largest observed load or replaces them with a search result.

If the supplied elastic observations retain a scalar swatch fit's `fitted_green_strain_range`,
that existing operating box also applies to every raw Green-strain node, including nodes with
unobserved loads. Yield/hardening identification cannot widen it or refit the equilibrium tensor.
An unsupported FIT node refuses acquisition admission. An unsupported CHECK node remains a refused
replay of the already selected FIT candidate: its parameters and FIT diagnostics stay available,
but no material is published. This matches the ordinary drape consumer's combined domain checks.

The complete executable example is
[`synthetic-membrane-plasticity-calibration.json`](../tests/fixtures/materials/synthetic-membrane-plasticity-calibration.json).
It is entirely artificial, with independently computed directional loading, unloading, reversed
loading and reloading. A deliberately unobserved load at a recorded higher warp strain still changes
the later permanent state. It does not supply constants for linen, leather or knit fabric.

### Identify the permanent branch of a qualified parallel network

The same request also accepts an independently supplied `material.membrane_relaxation` with the
explicit `parallel_plasticity` qualification described in [parallel membrane memory](material-parallel-memory.md).
It identifies the same **six permanent yield/hardening coefficients** from the measured **total**
second-Piola stress. The equilibrium tensor, proportional and directional Maxwell stiffnesses,
relaxation times, initialization, source observations and common operating domain remain fixed.
The four elastic observations belong to the equilibrium plastic branch; instantaneous stiffness
must not be substituted for them. This is an explicitly qualified parallel network, not a fit of
same-branch viscoplasticity or simultaneous identification of an unknown relaxation spectrum.

[`material-parallel-calibration.json`](../examples/material-parallel-calibration.json) is a complete,
entirely synthetic example with one proportional branch and one coupled directional branch. Its
total stresses were computed independently from the scalar plastic return equations and the matrix
Maxwell recurrence. It contains signed warp, weft and shear FIT cycles, a separate combined CHECK
cycle, an unobserved higher-strain node, and an irregularly timed unobserved dwell. All mass, bending
and contact inputs needed by ordinary selected mechanics are supplied separately. The input has no
previous yield/hardening observations; calibration does not fabricate temporary source records to
qualify its trial parameters.

The canonical Maxwell response uses **total imposed strain**, its own viscous history, and every
recorded time interval. Its fixed contribution is computed once per acquisition and is subtracted
only when constructing the algebraic plastic starting estimate. Every fitted residual compares the
original observed total stress against equilibrium-plastic plus Maxwell stress. Missing loads,
constant-strain dwells and reversals advance the fixed spectrum without resetting either history.
All shared spectrum/network conditions must match the independent elastic/domain observations and
the acquisition. The full ordinary compiler checks the completed material again before publication;
the accepted selection preserves the original fixed spectrum and binds both laws to the same source
identity. A change in source evidence cannot reuse a half-law from a different material.

### Jointly identify permanent stretch and one recovery arm

Add `joint_relaxation` to the same calibration request when neither the six permanent coefficients
nor one proportional Maxwell arm is known. The material needs **no provisional Maxwell coefficients**.
The network interpretation, source, specimen/conditioning, physical strain domain and new-simulation
initialization must still be independently declared. This synthetic declaration shows the interface;
its numbers are search choices, not measured textile properties:

```json
{
  "joint_relaxation": {
    "branch_id": "identified-arm",
    "stiffness_ratio_bounds": [0.1, 0.9],
    "relaxation_time_bounds_seconds": [0.1, 1.2],
    "strain_range": [[-0.2, 0.2], [-0.2, 0.2], [-0.2, 0.2]],
    "initialization": "stress_free_reference",
    "network": {
      "model": "parallel_green_maxwell_plastic/1",
      "source": "your-network-source",
      "evidence": "estimate",
      "method": "Qualified parallel interpretation, physical domain and initial state",
      "conditions": {
        "specimen_id": "your-exact-specimen",
        "conditioning": "your-characterized-state"
      }
    }
  }
}
```

The named source must already exist in `material.sources`; every network condition must match the
acquisition and independent elastic/domain observations. The two new bounds must be strictly ordered
and positive. They are optimizer bounds, never a time-validity interval or uncertainty estimate.
The domain is intersected with the original plastic and any fitted elastic domains. Initialization
can alternatively be the source-declared `relaxed_start`; because acquisitions start at zero strain,
these experiments do not identify which initialization a later garment should use.

All **eight** parameters vary in one bounded optimization. Each objective and sensitivity probe
replays the plastic return map and the canonical backward-Euler Maxwell recurrence over every
recorded node, including unobserved peaks and dwells. Maxwell uses total physical Green strain;
its stress never enters the plastic branch's yield check. There is no alternating fit that treats
an estimated branch as independently known. The full eight-column FIT sensitivity gate detects
local confounding, including ratio/time combinations that a single dwell interval cannot separate.
Four deterministic interior starts replace the plastic-only algebraic starting estimate.
Joint steps solve the damped local least-squares problem inside the parameter box, reoptimizing
free coordinates after an endpoint is reached and releasing endpoints with an inward gradient.
This bounded active-set solve uses the existing deterministic linear algebra and a fixed work limit;
simple clipping of an unconstrained coupled step cannot establish its completion.

The selected `parameters.relaxation` reports `stiffness_ratio` and `relaxation_time_seconds`.
`diagnostics.joint_relaxation` retains the two sensitivities, endpoint flags and `branch_steps`;
the ordinary excitation pivot covers all eight coefficients. Every objective, failed probe and
final replay reserves both the plastic nodes and Maxwell branch nodes under the same existing
work caps. No variable recovery history is reused from another trial. CHECK is evaluated only
after both branches and the FIT diagnostics are fixed.

Accepted output contains one ordinary source-locked proportional branch and the six permanent
coefficients, all classified as estimates under the same calibration receipt. Their source records,
uncertainty fields and independently supplied physics are not fabricated from optimizer bounds.
If the input already contains a Maxwell spectrum, replacement requires `replace_existing: true`
and exactly one proportional arm with the same `branch_id`; additional or directional arms refuse
instead of being dropped. The original supplied coefficients remain in the input receipt and do
not constrain or seed the joint fit. Omitting `joint_relaxation` preserves the previous fixed-spectrum
and pure-plastic arithmetic and wire fields.

This bounded mode does not identify multiple or directional Maxwell arms, equilibrium stiffness,
strain domains, network topology, damage, or temperature/moisture dependence. Recorded-node replay
does not establish continuous-path exactness or discretization convergence, and an identifiable
synthetic example does not establish that a real experiment separates all eight parameters.

The complete runnable [material-joint-calibration.json](../examples/material-joint-calibration.json)
example contains 27 FIT and five CHECK nodes, two irregular dwells, reversals and an unobserved
yielded peak. Independently tabulated scalar plastic observations are combined with an independent
backward-Euler Maxwell recurrence. Its synthetic targets are yields `[10,8,5]` N/m, hardening
`[100,50,25]` N/m, stiffness ratio `1.4` and recovery time `0.18` s. None of those eight coefficients
is supplied as an input material property. The ordinary native commands, frozen-pattern binding
and timed-strip consumers use the resulting complete material.

### Shared acquisition, admission and publication

Each experiment names a source, evidence class, unique acquisition ID, whole-history `fit` or `check`
partition and mandatory `initialization: "pristine"`. Every history starts at zero time and zero
strain in the unworked reference. Each subsequent node contains increasing `time_seconds`, raw
`green_strain: [E11,E22,2E12]`, `second_piola_n_m: [S11,S22,S12]`, and positive `scale_n_m` for residual
weighting. Missing load channels are `null`; their strain nodes still advance the existing material
memory. Do not replace a missing lateral force with zero or confuse nominal/Cauchy loads with these
work-conjugate second-Piola sheet resultants. The acquisition protocol must explain that reduction.
The solver replays the retained incremental nodes. It does not infer a continuous load path between
them or establish increment-size independence. Without a parallel spectrum, time labels preserve
acquisition order and the permanent law remains rate independent. With a parallel spectrum, actual
elapsed seconds determine Maxwell relaxation; compressing a dwell into one longer interval generally
changes the discrete response.

Set explicit, strictly ordered `yield_bounds_n_m` and `hardening_bounds_n_m`, each three pairs in
warp/weft/shear order. Yield lower bounds must be positive; hardening can include an explicit zero.
`allow_estimated_material` admits independently supplied estimated elastic/domain quantities and,
when present, the fixed spectrum and the separate fixed/joint network qualification;
`allow_estimated_observations` separately admits estimated or synthetic acquisitions. Neither changes
the fitted coefficients' `estimate` classification. `replace_existing` is necessary to replace a prior
permanent-law selector or yield/hardening observation; the original remains in the report.

The existing command and workbench paths expose this operation:

```sh
cargo run --locked -p ftx-cli --bin ftx-materials -- calibrate-membrane-plasticity \
  tests/fixtures/materials/synthetic-membrane-plasticity-calibration.json --allow-estimates
cargo run --locked -p ftx-cli --bin ftx -- --json materials calibrate-membrane-plasticity \
  tests/fixtures/materials/synthetic-membrane-plasticity-calibration.json --allow-estimates
cargo run --locked -p ftx-cli --bin ftx-materials -- calibrate-membrane-plasticity \
  examples/material-parallel-calibration.json --allow-estimates
cargo run --locked -p ftx-cli --bin ftx -- --json materials calibrate-membrane-plasticity \
  examples/material-joint-calibration.json --allow-estimates
```

The commands' final `--allow-estimates` controls **consumption** of the fitted material, separately
from permission to use the supplied acquisition. Library callers use
`material_membrane_plasticity::calibration::calibrate`; accepted reports supply an ordinary material
for the existing catalog/resolver/frozen-selection boundary. No calibration-specific solver or changed
pattern is needed to carry an admitted selection into sewn drape and prescribed-body motion.

Identification uses four deterministic starts and bounded damped Gauss–Newton steps in normalized
parameter coordinates. A complete observed FIT prefix may supply an algebraic starting estimate;
it adds no observations. Finite-difference probes replay complete FIT histories from their declared
initial states. Bound-constrained gradient stationarity is checked so a Newton step clipped to a
corner cannot be mistaken for convergence. All starts, attempted forward evaluations, rejected
trials and final probes/replays share `max_evaluations` (1–4096) and a fixed 2,000,000 recorded-node
budget. The input permits up to 32 histories and 2048 recorded nodes. These are work limits, not
runtime or accuracy guarantees.

For a fixed spectrum, each whole FIT or CHECK precomputation additionally costs one forward
evaluation and one recorded-work unit per branch per node, including its initial state. The full
partition is reserved before replay, including refused work. `fixed_maxwell_branch_steps` records
this portion of `history_steps`; the example has 64 branch-node states across its 32 nodes and two
branches. FIT caching happens before any starting estimate, while CHECK caching waits until all
FIT selection and excitation checks are complete. The cache is reused for every plastic coefficient
probe; it neither spends unbounded hidden work nor consults held-out loads during optimization.

The report retains original evidence, source identity, every entered start, the selected FIT
coefficients, work counts, observed-component residuals and the accepted permanent strain,
accumulation, storage and dissipated work at each successfully replayed node. FIT sensitivity and a
column-normalized Gram pivot describe local numerical excitation. Missing directions, indistinguishable
yield/hardening or joint recovery effects, and materially different residual-equivalent start solutions,
are `AMBIGUOUS`.
Iteration limits and incomplete sensitivity calculations cannot produce an assignable material.
Work exhaustion is `BUDGET_EXHAUSTED`, including exhaustion during final diagnostics or CHECK replay.
Numerical convergence and local excitation do not prove global uniqueness or coefficient confidence.

Joint predictions retain `second_piola_n_m` as the **total** stress. Existing `stored_j_m2` remains
equilibrium elastic-plus-hardening storage and `plastic_dissipation_j_m2` remains permanent work.
An additional `parallel_history` contains `maxwell_second_piola_n_m`, accepted
`viscous_green_strains` in proportional-then-directional authored order, `maxwell_stored_j_m2`,
cumulative `maxwell_dissipation_j_m2`, and `total_stored_j_m2`. The total counts the two actual
storage contributions once each. Maxwell dissipation is the canonical backward-Euler endpoint
dashpot-work quadrature, separately reported from plastic work; it is neither numerical integration
loss nor an inferred heat source. Pure permanent-law reports omit these additional spectrum fields.

All parameter selection and excitation analysis finish **before CHECK replay**. A CHECK failure
rejects the same selected candidate; another start is never promoted because it fits CHECK better.
FIT and CHECK must each meet the declared RMS and peak scaled-residual limits. When no CHECK load is
observed, its status is `NOT_PROVIDED`, with no passing independent validation claim. Only `ACCEPTED`
contains a material; ambiguous, rejected and incomplete results retain reviewable diagnostics but
cannot leave an old assignable calibration active. Source-locked fitted quantities remain estimates
with no manufactured uncertainty bounds. Empirical cyclic-coupon and garment acceptance remain open.

## Verification and remaining material evidence

### Calibration checks

The independent synthetic cycle fixture and focused calibration checks cover all six coefficients,
coupled yielding, missing load channels that still advance memory, a constrained optimum, explicit
replacement, insufficient excitation, FIT/CHECK isolation and exhaustion during final replay.
Joint checks additionally compare total stress, both accepted histories and separate energy/work
ledgers with independent scalar/matrix recurrences; verify exact dwell-factor products at irregular
times; preserve the fixed spectrum and ordinary source lock; and retain FIT diagnostics when a
held-out time history or common-domain replay fails. Fixed-spectrum work participates in the same
bounded budget, including final CHECK refusal, and the legacy pure-law wire fields remain unchanged.
Fresh joint-identification checks start without either branch's coefficients, compare all eight
recovered targets and every Maxwell node against independently constructed scalar recurrences,
retain both histories through an unobserved yielded peak, reject insufficient time excitation,
and preserve FIT decisions under rejected or refused CHECK data. Source/domain/replacement refusal
and charging both constitutive histories against one shared budget have focused regressions.
The command and shared WASM checks additionally exercise ordinary frozen selection, unchanged
pattern binding, material-hash tampering and separate acquisition/consumption admission. Browser
state checks cover all unresolved outcomes and withdrawal of a previously assignable selection.

```sh
cargo test --locked -p ftx-pattern --test membrane_plasticity_calibration
cargo test --locked -p ftx-cli --test membrane_plasticity_calibration_workflow
cargo test --locked -p ftx-wasm --test materials
node --experimental-vm-modules --test tools/check_materials_frontend.cjs
```

### Forward-law and qualified heat checks

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
loading, unloading and reloading checks, plus stiffness degradation, damage and rupture. Same-branch
viscoplasticity, nonlinear plastic response, gathered compression, conditioning effects, serialized restart,
remeshing transfer and temperature feedback to mechanics remain separate missing capabilities. Existing
elastic or relaxation calibration commands retain their own models; permanent yield/hardening uses
the separate existing-law identification request above, optionally with an already qualified fixed
parallel spectrum or jointly identified single proportional arm. Joint identification of multiple
or directional unknown Maxwell arms remains open. The linen/leather/knit substitution and
independent garment gates need measured specimens, not the synthetic fixture's values.
