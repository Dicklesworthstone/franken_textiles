# Timed swatch measurements to the actual cloth solver

`ftx-materials calibrate-relaxation HISTORIES.json [--allow-estimates]` identifies Maxwell response
from timed force/deformation acquisitions, then returns the ordinary frozen material selection used
by `bind`, `resolve`, sewn garments and body-motion continuation. The original mode identifies
positive proportional stiffness ratios and relaxation times. The optional orthotropic mode identifies
independent warp/wale, weft/course, signed normal coupling and bias tensors at explicitly supplied
candidate times. The equilibrium membrane must already be independently characterized. This is the
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

## Independent directional recovery at fixed candidate times

The same `ftx-relaxation-calibration/1` input can replace the proportional time search with:

```json
"orthotropic": {
  "times_seconds": [0.3, 2.0, 8.0, 32.0],
  "zero_threshold_n_m": 1e-7
}
```

Omit `time_bounds_seconds` or supply an empty array. Nonempty search windows and orthotropic mode
together refuse. Supply one to eight strictly increasing positive candidate times; these times are
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

## Solver agreement and remaining work

Calibration uses continuous-time analytic history integration. The actual cloth solver uses backward
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
Automatic directional time optimization, fixed-equilibrium error, strain uncertainty, temperature/wet
dependence, plastic set and damage remain outside this estimator. Broader measurements and independent
garment checks are required.

Primary formula/identification references, checked 2026-10-14; no external source code copied:
- pyvisco Prony identification: https://pyvisco.readthedocs.io/en/stable/_autosummary/pyvisco.prony.html
- MOOSE Maxwell model: https://mooseframework.inl.gov/source/materials/GeneralizedMaxwellModel.html
