# Timed swatch measurements to the actual cloth solver

`ftx-materials calibrate-relaxation HISTORIES.json [--allow-estimates]` identifies Maxwell response
from timed force/deformation acquisitions, then returns the ordinary frozen material selection used
by `bind`, `resolve`, sewn garments and body-motion continuation. The original mode identifies
positive proportional stiffness ratios and relaxation times. The optional orthotropic mode identifies
independent warp/wale, weft/course, signed normal coupling and bias tensors at explicitly supplied
candidate times, or jointly identifies those tensors and one to three times inside separate declared
windows. The equilibrium membrane must already be independently characterized. This is the
measurement-to-consumer slice of `ftx-tm2`, `ftx-tm4` and `ftx-rc1`; physical specimen and garment
validation remain separate requirements.

## Runnable synthetic example

The fixture is entirely synthetic: ratios 1.7 and 0.65, times 0.3 and 8 seconds, one finite ramp/hold/
unload acquisition and a separate step-hold check. It is not a recommended law for any real fabric.
Use new output paths, retain the calibration report with the frozen selection, and use a pattern with
explicit grainlines:

```sh
cargo run -p ftx-cli --bin ftx-materials -- calibrate-relaxation \
  tests/fixtures/materials/synthetic-relaxation-history.json \
  --allow-estimates > relaxation-fit.json
python3 -c 'import json, sys; sys.stdout.write(json.load(open("relaxation-fit.json"))["selection_json"])' \
  > fitted-relaxation-selection.json
cargo run -p ftx-cli --bin ftx-materials -- bind \
  YOUR-PATTERN.json fitted-relaxation-selection.json relaxing-pattern.json
cargo run -p ftx-cli --bin ftx-materials -- resolve relaxing-pattern.json
```

The resulting pattern uses the existing `ftx drape` workflow with the same body and numerical options.
No pattern, physics code, mass, bending or contact parameter changes are needed to consume the fitted
timescales. The CLI reports both the full fit and `simulation_ready`/`mechanics_refusal`. Identification
can succeed even when thickness, mass, bending or friction are missing; it does not make that material
ready for a consumer requiring those properties. `bind` never overwrites its destination.

The primary CLI provides the same engine as `ftx materials calibrate-relaxation HISTORIES.json
[--allow-estimates] [--json]`. Its JSON envelope contains the result under `result`; the companion
command above returns that result directly. The shared WASM export is `material_calibrate_relaxation`,
also used by the browser calibration workbench. Use the returned exact `selection_json` string for
binding and portable downloads. Parsing the enclosing report is fine; reserializing the nested
selection with a different JSON implementation can change numeric spellings such as signed zero
and invalidate its frozen identity. Retain the original acquisition text for replay; the browser
provides it separately as `calibration-input.json`.

## Acquisition contract

The complete input schema is `ftx-relaxation-calibration/1`; the fixture gives all required fields.
Supply one actual specimen/product/construction/conditioning record, an acquisition protocol and
source records. Different moisture, temperature, finish, loading-rate protocols or damaged specimens
must not silently become one history. The program checks declared references, not physical provenance.

The `equilibrium_is_long_time` declaration is mandatory. An instantaneous modulus is not equilibrium
stiffness: adding transient branches to it would double-count stiffness. The equilibrium response may
be the existing scalar orthotropic law or a source-bearing nonlinear response. It remains fixed during
fitting. The nonlinear equilibrium is evaluated at each measured strain, not replaced by its tangent.
In proportional mode branch ratios multiply the equilibrium **origin tangent**, matching the existing
simulation law. Independent orthotropic branch tensors do not multiply that tangent.

Each experiment has a unique acquisition ID, source, evidence class, initialization, partition and
time-ordered samples. Sample times start at zero and increase strictly. Supply F in material axes and
nominal force resultants P in N/m of undeformed width, not breaking forces or stress in Pa. Observe the
lateral/shear deformation needed to reconstruct F; an unknown force component is `null`, not zero.
Keep strain nodes during ramps and unloading even when their force is unmeasured. The forward model
assumes Green strain is piecewise linear **between these nodes**, not that F or engineering strain is
piecewise linear. Acquisition duration is physical time, never solver iteration count.

Initialization is independent for each acquisition: `stress_free_reference` means internal strains
are zero at time zero, including an explicitly imposed initial deformation; `relaxed_start` means the
branches were relaxed at that initial pose. The input's top-level `initialization` declares the future
garment initialization, not a reset of the acquisitions. Unknown prehistory is not fitted or inferred.
The supplied `strain_range` and its `domain_source` qualify applicability. Every measured strain must
lie inside it; a low residual does not extend the domain or prove its physical validity.
Any preexisting scalar-fit `fitted_green_strain_range` and nonlinear equilibrium curve limits also
apply at every recorded node, including unobserved-force ramp or unloading nodes. A wider Maxwell
box cannot authorize equilibrium extrapolation. These checks are shared by proportional, fixed-time
directional and automatically searched directional fits; supplied equilibrium quantities stay intact.
During directional model comparison, CHECK constitutive states are evaluated after FIT selection,
so a held-out domain refusal rejects the same candidate without changing the chosen model or its fit.

## Identification and independent checks

For e = [E11,E22,2E12] and branch lag r = e-z, each time interval h is integrated analytically:

```
a = exp(-h/tau)
phi = (1-a)/(h/tau)
r_next = a*r_old + phi*(e_next-e_old)
S = S_equilibrium(e) + sum(ratio * Q_equilibrium_origin * r)
P = F*S
```

Small-argument series avoid cancellation in phi and its log-time derivative. The fit uses analytic
force sensitivities, positive log-parameters, scaled damped least squares and deterministic multiple
starts. One to three ordered non-overlapping `time_bounds_seconds` windows select the number of fitted
branches and constrain their times. They are search intervals, **not confidence bounds**. The existing
forward law supports more branches; this fitter deliberately bounds the inverse problem. It does not
select model order automatically or certify a unique global solution.

Unexcited, ill-conditioned, boundary-limited or unconverged solutions refuse. `scale_n_m` is an explicit
force-residual weighting scale, not automatically measurement uncertainty. RMS and maximum scaled
residuals must satisfy the declared limits separately for training and held-out partitions. Entire
`check` acquisitions are replayed only after fitting; their forces never select parameters or starts.
An absent check acquisition is reported as `check: null`. Distinct IDs do not prove experiments are
independent: that remains part of the acquisition protocol.

The output retains the original input and sources, old material evidence, every force prediction and
observed residual, the input hash, numerical excitation diagnostics and a normal `membrane_relaxation`
law. Prior relaxation evidence requires `replace_existing: true`; it remains in the report input.
Estimated equilibrium data and estimated acquisition observations have separate opt-ins. Fitted ratios
and times are **always model estimates**, even from measured forces. The command's `--allow-estimates`
separately admits estimated properties to subsequent simulation. Without it the fit can be inspected
but its frozen selection will not authorize those estimated inputs. No uncertainty bounds are invented.

## Recovery alongside an independently characterized permanent-set branch

Run the complete, explicitly synthetic acquisition
[relaxation-plastic-equilibrium.json](../examples/relaxation-plastic-equilibrium.json) through either
existing native entry point:

```sh
cargo run -p ftx-cli --bin ftx -- --json materials calibrate-relaxation \
  examples/relaxation-plastic-equilibrium.json --allow-estimates
cargo run -p ftx-cli --bin ftx-materials -- calibrate-relaxation \
  examples/relaxation-plastic-equilibrium.json --allow-estimates
```

The example retains nine FIT nodes and seven independently initialized CHECK nodes. Its artificial
warp equilibrium uses stiffness 1000 N/m, yield 20 N/m and hardening 100 N/m; the full material's
elastic, plastic, mass, bending and contact observations remain fixed. Its forces were constructed
independently from the scalar return-map formulas and analytic Maxwell convolution with stiffness
ratio **2** and relaxation time **0.05 s**. Those are recovery parameters to identify, not supplied
measured textile properties. At 0.03 s the FIT path reaches Green strain 0.08 with all four force
channels `null`. That unobserved-force peak still advances permanent strain and recovery memory;
the following forces must retain its effects. The source notes, complete paths and supplied network
qualification stay in the result alongside the ordinary frozen selection.

The same calibration command can identify Maxwell recovery when an independently characterized
elastoplastic equilibrium branch is already available. Add an explicit `plastic_equilibrium`
declaration to the ordinary acquisition input:

```json
"plastic_equilibrium": {
  "initialization": "pristine_reference",
  "allow_estimated_network": false,
  "network": {
    "model": "parallel_green_maxwell_plastic/1",
    "evidence": "measured",
    "source": "your-independent-network-source",
    "method": "Your evidence supporting this parallel branch interpretation",
    "conditions": {
      "specimen_id": "your-exact-specimen",
      "conditioning": "your-qualified-state"
    }
  }
}
```

These strings describe the input shape; they are not supplied measurements or a qualified specimen.
The source must exist in `material.sources`, and the network conditions must equal the acquisition's
shared `conditions`. The ordinary material must retain its existing
`orthotropic_green_return_map/1` selector, four elastic observations and twelve yield, hardening and
domain observations. Their values and source records stay fixed. Each must match the network's
declared specimen and conditioning. Additional temperature or moisture conditions stay explicit;
missing conditions never become temperature independence or a wet-fabric law.

`equilibrium_is_long_time` remains mandatory. Here the fixed elastic tensor belongs to the plastic
equilibrium branch, not the sum of instantaneous branch stiffnesses. The independently qualified
network is the existing parallel material described in [material-parallel-memory.md](material-parallel-memory.md).
Permanent flow sees only equilibrium-branch stress. Every Maxwell arm sees total physical Green
strain, and its force adds to the known plastic equilibrium force. The fitter never substitutes
`e - plastic_strain` for the Maxwell driving strain and never identifies yield or hardening in this mode.

Every acquisition must begin at a recorded zero-Green-strain node with zero permanent and accumulated
strain. The explicit preparation applies separately to each complete experiment; a `relaxed_start`
Maxwell setting does not erase unknown preload or initialize permanent strain from the first pose.
All later acquisition nodes advance the canonical plastic return map, including nodes whose four
forces are all `null`. Omitting an unloaded peak, reversal or hold can change subsequent forces and
cannot be repaired by treating missing forces as zero. The requested Maxwell domain is intersected
with the original plastic and fitted-equilibrium domains, and accumulated-strain limits remain active.

Proportional, fixed-time orthotropic, bounded directional-time and model-comparison inputs share
this preparation. Their existing excitation, stationarity, source, parameter-bound and residual gates
are unchanged. Only FIT forces select coefficients, times or competing models. A CHECK refusal or
excess residual rejects the fixed choice without selecting a replacement. Fixed plastic estimates,
acquisition estimates and an estimated network interpretation require their respective independent
opt-ins. Published Maxwell coefficients remain estimates, so using the resulting Selection still
requires ordinary estimate consent.

Each prediction additionally retains `plastic_equilibrium`: accepted `plastic_strain`,
`accumulated_strain`, known equilibrium `nominal_n_m`, `step_dissipated_j_m2` and
`cumulative_dissipated_j_m2`. Cumulative work starts again for each independently initialized
acquisition. The output is an ordinary material whose `membrane_relaxation.parallel_plasticity`
retains the supplied network qualification. The full material compiler admits both histories together;
the frozen selection can enter existing sewn or timed-strip consumers without another material engine.
An all-zero orthotropic spectrum refuses this requested parallel interpretation rather than quietly
publishing a different network. Omitting `plastic_equilibrium` preserves previous serialization and
the refusal to mix an undeclared permanent-strain branch with Maxwell identification.

The plastic part is an explicitly discrete recorded-node return-map experiment. The Maxwell
convolution is exact for the declared piecewise-linear Green-strain interpolation; this does not make
the nonlinear plastic path continuous-time exact. No unobserved turning points, time/space refinement,
parameter confidence, measured specimen agreement, heat conversion or garment fit are inferred.
Retain complete acquisition histories and verify temporal/spatial resolution independently when
using an identified material in a finite strip or garment simulation.

## Independent directional recovery at fixed candidate times

The same `ftx-relaxation-calibration/1` input can replace the proportional time search with:

```json
"orthotropic": {
  "times_seconds": [0.3, 2.0, 8.0, 32.0],
  "zero_threshold_n_m": 1e-7
}
```

Omit the top-level `time_bounds_seconds` or supply an empty array: those windows belong to the
proportional model. Supply one to eight strictly increasing positive candidate times; these times are
fixed modeling choices, not optimized decay measurements or uncertainty bounds. The fit identifies
four independent coefficients per candidate, in N/m:

\[
Q_i=\begin{bmatrix}a_i&c_i&0\\c_i&b_i&0\\0&0&g_i\end{bmatrix},
\qquad S=S_{\rm equilibrium}(e)+\sum_i Q_i r_i.
\]

The constraints are positive semidefiniteness of the normal block and nonnegative engineering-shear
stiffness: (a_i,b_i,g_i\geq0) and (c_i^2\leq a_i b_i). Coupling may have either sign. A spring can
therefore relax only warp, only weft, only shear, a coupled normal direction, or a combination, without
inventing stiffness in inactive directions. These are sheet tensor entries, not Young moduli or bulk
stresses. Both `specimen_id` and `conditioning` must be declared, and every declared shared condition
must also match the independently supplied equilibrium observations. The emitted tensor and time
quantities preserve those conditions and point to the complete calibration receipt.

All candidates use the same exact finite-ramp/unload integration above. Only whole `fit` histories
form the weighted linear force design. Every tensor coefficient must be independently observable
over the declared directional and time coverage; absent sensors remain absent and held-out histories
cannot rescue an unexcited design. The existing deterministic least-squares implementation supplies
the unconstrained optimum. If that optimum is inadmissible, a bounded projected-gradient solve fits
the stated convex PSD/nonnegative constraints in Frobenius tensor coordinates. It does not clamp the
observed forces or reuse a proportional law as a directional measurement. Nonconvergence and deficient
excitation refuse rather than publishing a partial spectrum.

`zero_threshold_n_m` is an explicit caller choice, defaulting to zero. Small coupling may become zero;
small warp/weft entries become zero only when their coupling is also below that threshold. Shear is
thresholded independently. A nonzero coupling is never retained beside a zero normal stiffness.
Entirely zero candidates are omitted from the simulation law but remain in the report. If every
candidate is inactive, the material contains the unchanged equilibrium response and no relaxation
law. That means no retained response under the supplied basis, threshold and budgets; it does not
establish that the physical textile has no recovery or creep.

The optional `orthotropic_fit` report retains every candidate in input order, its exact time, its
unconstrained coefficients, its constrained coefficients before thresholding, its published
`stiffness_n_m` and its `active` decision. `psd_projection_applied`, iteration count and the relative
projected-step diagnostic make the constrained solve explicit. `parameter_sensitivity` now describes
scaled-force sensitivity to each coefficient [warp, weft, coupling, shear] in candidate order; it is
not a standard error. Proportional-mode reports retain their original log-parameter interpretation.
After thresholding, all predictions are recomputed through the ordinary compiled material's physical
response with the continuous acquisition state. Both original residual budgets are checked again,
separately for fit and check acquisitions. Increasing the threshold cannot hide a discarded response.

The independently calculated fixture
`tests/fixtures/materials/synthetic-orthotropic-relaxation-history.json` has these artificial tensors:

| Candidate time (s) | Warp (N/m) | Weft (N/m) | Coupling (N/m) | Shear (N/m) |
|---:|---:|---:|---:|---:|
| 0.3 | 240 | 60 | −30 | 0 |
| 2 | 0 | 0 | 0 | 90 |
| 8 | 0 | 120 | 0 | 0 |
| 32 | 0 | 0 | 0 | 0 |

Three separately initialized step holds are used for identification. A mixed ramp, reversal, unload
and hold acquisition starts pre-relaxed and is held out in full. The fixture's forces come from an
independent direct exponential convolution of all preceding strain segments. The last candidate is
intentionally inactive. Run it through the same native command or browser history upload; its frozen
selection automatically reaches existing per-piece mechanics, sewing continuation and body-motion
history. It is a numerical software specimen, not measured performance-knit or leather data.

## Identifying independent directional times

When decay times are unknown, replace the fixed `orthotropic.times_seconds` with separate nested
directional windows:

```json
"orthotropic": {
  "time_bounds_seconds": [[0.1, 0.9], [1.0, 4.0], [5.0, 24.0]],
  "zero_threshold_n_m": 1e-7
}
```

Supply one to three positive, ordered, nonoverlapping windows. Omit `orthotropic.times_seconds` or
leave it empty; nonempty fixed times and nested windows together refuse. These windows declare the
number of branches and restrict their search. They are not confidence intervals. The existing
fixed-time mode retains its original arithmetic, report fields and source identities.

The bounded search operates on log-times. Every trial rebuilds the exact FIT force design and solves
the same constrained tensor problem used by the fixed-time mode. An analytic envelope gradient
differentiates this minimized objective; it does not treat a frozen tensor's residual Jacobian as the
derivative of the entire fit. Deterministic starts use every combination of 20%, 50% and 80% positions
inside the log-windows. BFGS steps use a bounded log-time movement and a decreasing-objective line
search. The selected local minimum has the smallest FIT squared residual among converged starts.
Held-out forces, acceptance budgets and the explicit zero threshold never choose times or starts.

Work is bounded across all starts: at most 1,024 objective evaluations, 262,144 projected tensor
iterations, 120 accepted time iterations per start and 20 backtracking trials per iteration. Exhausting
a global budget refuses instead of publishing a partly searched spectrum. These bounds are deliberate
for the shared native/browser engine; convergence does not certify a unique or global optimum.

An optimized time must have observable force sensitivity and be distinguishable from all fitted
amplitudes and other times using FIT observations alone. The complete column-normalized joint force
Jacobian must pass the reported excitation gate. A time within 0.001% of either log-window boundary
refuses. A whole branch that becomes inactive after the explicit threshold also refuses: a zero
spring does not identify a decay time. Individual inactive warp, weft, coupling or shear coefficients
remain allowed when the acquisition resolves the tensor and time. Actual compiled-law FIT/CHECK
replay and both residual gates still run after thresholding.

The optional `orthotropic_fit.time_search` receipt distinguishes identified times from supplied fixed
times. It records every declared window through the original input, each fitted tensor/time through
the candidates, attempted/converged starts, selected outer iterations, total objective evaluations
and total projected tensor iterations. `gradient_scaled_max` is
`max |dSSE/dlog(tau)| / (2 * sensitivity * (1 + sqrt(SSE)))`, with a convergence threshold of `1e-10`;
it is an objective stationarity diagnostic, not an uncertainty estimate. `time_sensitivity` contains
the scaled FIT force derivative norms with respect to log-times at the fitted tensors.
`joint_excitation_pivot` is the smallest Cholesky pivot of the column-normalized joint Gram matrix,
required to exceed `1e-7`; `boundary_distance_fraction` measures the closest boundary distance as a
fraction of the corresponding log-window width. These diagnostics describe local numerical behavior.

`tests/fixtures/materials/synthetic-orthotropic-relaxation-search.json` uses the same independently
calculated directional observations as the fixed fixture, with the three windows above and without
the inactive 32-second candidate. The independent truth is 0.3, 2 and 8 seconds with the first three
tensors in the table. Run it through the same native command, WASM export or browser history upload;
retain the report and use its exact `selection_json` for the ordinary material-binding workflow.

## Comparing model order and competing recovery spectra

The same command and WASM export accept `ftx-relaxation-model-comparison/1`. This is a bounded
comparison of two to four explicitly declared orthotropic models on one acquisition. A model can
use one to eight fixed candidate times, or one to three bounded time-search windows. The estimator
does not invent a noise distribution from the force weighting scales or search an unbounded set
of model orders.

| Field | Meaning |
|---|---|
| `calibration` | Complete `ftx-relaxation-calibration/1` acquisition, without `orthotropic` and with empty/omitted `time_bounds_seconds`; each alternative supplies its own time policy. |
| `candidates` | Two to four objects with unique `id` and the ordinary `orthotropic` options. Evaluation order is canonical ID order. |
| `fit_rms_equivalence_scaled` | Required finite, nonnegative RMS excess above the best FIT-admissible model still considered equivalent, in the acquisition's scaled-force units. |
| `select_candidate_id` | Optional reviewed, explicit choice. The named model must still pass FIT and independent CHECK acceptance. |
| `max_objective_evaluations` | Shared fixed-fit/time-search allowance, including failed trials; default and maximum 1024. |
| `max_projected_iterations` | Shared constrained tensor-iteration allowance; default and maximum 262144. |

For example, compare the three-branch independent synthetic truth against a four-time basis that
also admits a 32-second branch. Use new output filenames:

```sh
python3 - <<'PY'
import json
from pathlib import Path

calibration = json.loads(Path(
    "tests/fixtures/materials/synthetic-orthotropic-relaxation-history.json"
).read_text())
options = calibration.pop("orthotropic")
request = {
    "schema": "ftx-relaxation-model-comparison/1",
    "calibration": calibration,
    "fit_rms_equivalence_scaled": 1e-7,
    "candidates": [
        {"id": "complete", "orthotropic": {
            **options, "times_seconds": [0.3, 2, 8]}},
        {"id": "extra", "orthotropic": {
            **options, "times_seconds": [0.3, 2, 8, 32]}},
    ],
}
with open("recovery-comparison.json", "x") as output:
    json.dump(request, output, indent=2)
PY
cargo run -p ftx-cli --bin ftx-materials -- calibrate-relaxation \
  recovery-comparison.json --allow-estimates > recovery-comparison-report.json
```

Both alternatives explain these synthetic FIT forces, so the result is `AMBIGUOUS` and has no
`selection_json`. Review `comparison.candidates`, `prediction_differences` and `fit_decision`.
Adding `"select_candidate_id":"complete"` to a new request explicitly chooses that model;
ambiguity remains reported, and the chosen model must still pass its held-out checks. An accepted
result contains the ordinary `calibration`, `selection` and exact `selection_json` used by `bind`
and the actual drape resolver. Observation consent and simulation admission remain separate.

All candidates are numerically fitted using **FIT forces only**, with a common equilibrium,
acquisition, residual scales and FIT acceptance policy. Every finite fitted residual is retained,
including alternatives that exceed the RMS or peak gate. The report lists declared parameters
(four tensor coefficients per fixed time, five per optimized time); this is not effective degrees
of freedom of a constrained estimator. Among FIT-equivalent models it reports a parsimonious
preference, but this never silently resolves an ambiguity into a frozen material.

The complete `fit_decision`, including any choice, is fixed before CHECK replay. CHECK outcomes
can reject the same chosen candidate; they never rank alternatives or select a fallback. No CHECK
acquisition is explicitly `NOT_PROVIDED`, not a passed check. A numerical refusal leaves that
alternative unresolved and prevents automatic selection from an incomplete comparison. A user
can explicitly name a separately reviewed FIT-admissible candidate, but a global `BUDGET_STOP`
always suppresses publication, including an earlier successful or explicitly chosen candidate.

The work limits apply to **the entire comparison**, not separately to each candidate or search
start. Failed objective evaluations and failed constrained fits consume the same allowance.
Every candidate retains its outcome, refusal and consumed work; later candidates are `NOT_RUN`
when the shared allowance stops the request. A successful command may therefore return a useful
comparison report with no material selection. Check `comparison.status` and `selection_json`
before binding. The browser displays the model table and downloads the report and original input
for unresolved outcomes; it clears the previous assignable selection.

FIT equivalence only describes these supplied alternatives at this declared numerical tolerance.
Pairwise prediction differences use observed FIT force channels, not unobserved directions or
behavior outside the acquisition. Close force predictions do not establish unique recovery times,
parameter confidence intervals, fixed-equilibrium accuracy, empirical textile validity or garment
fit. Independent real specimens and garment checks remain necessary.

## Solver agreement and remaining work

The Maxwell part of calibration uses continuous-time analytic history integration. The actual cloth solver uses backward
Euler, so finite time-step force predictions differ by discretization error. Refine the simulation
time step; do not retune a material to hide integration error. An independent clamped-triangle test
compares the actual solver reaction to the discrete Maxwell decay, and another demonstrates convergence
to the continuous-time answer. A material-only regression multiplies all acquisition timestamps by
four: fitted times change, equilibrium/mass/geometry stay fixed, and loaded cloth moves differently.

Other tests cover independent convolution answers for one/two/three timescales, ramps and unloading,
partial force observations, nonlinear equilibrium, noisy data, rigid-frame rotation, finite-difference
sensitivities, frozen identities and refusal paths. These are synthetic numerical/software checks,
not a calibrated library of real linen, leather or performance-knit specimens. Additional checks cover
directional tensor recovery, inactive axes/candidates, signed coupling, constrained noisy forces,
partial sensors, nonlinear equilibrium, source/conditioning conflicts and actual compiled-law replay.
Additional automatic-time checks recover the independent three-branch truth, one/two-branch spectra
with inactive axes, and a constrained noisy spectrum with differently scaled force sensors. The exact
profile-objective gradient is checked against finite differences that re-fit the PSD tensors.
Unbounded model-order discovery, fixed-equilibrium error, strain uncertainty, temperature/wet
dependence, simultaneous identification of permanent-set coefficients and damage remain outside this estimator. Broader measurements and independent
garment checks are required.

Primary formula/identification references, checked 2026-10-14; no external source code copied:
- pyvisco Prony identification: https://pyvisco.readthedocs.io/en/stable/_autosummary/pyvisco.prony.html
- MOOSE Maxwell model: https://mooseframework.inl.gov/source/materials/GeneralizedMaxwellModel.html
