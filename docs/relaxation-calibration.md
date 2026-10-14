# Timed swatch measurements to the actual cloth solver

`ftx-materials calibrate-relaxation HISTORIES.json [--allow-estimates]` identifies positive Maxwell
stiffness ratios and relaxation times, then returns the ordinary frozen material selection used by
`bind`, `resolve`, sewn garments and body-motion continuation. The equilibrium membrane must already
be independently characterized. This is the measurement-to-consumer slice of `ftx-tm2` and `ftx-tm4`,
not completion of their empirical textile or general constitutive-model acceptance.

## Runnable synthetic example

The fixture is entirely synthetic: ratios 1.7 and 0.65, times 0.3 and 8 seconds, one finite ramp/hold/
unload acquisition and a separate step-hold check. It is not a recommended law for any real fabric.
Use new output paths, retain the calibration report with the frozen selection, and use a pattern with
explicit grainlines:

```sh
cargo run -p ftx-cli --bin ftx-materials -- calibrate-relaxation \
  tests/fixtures/materials/synthetic-relaxation-history.json \
  --allow-estimates > relaxation-fit.json
python3 -c 'import json; print(json.dumps(json.load(open("relaxation-fit.json"))["selection"], indent=2))' \
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

## Acquisition contract

The complete input schema is `ftx-relaxation-calibration/1`; the fixture gives all required fields.
Supply one actual specimen/product/construction/conditioning record, an acquisition protocol and
source records. Different moisture, temperature, finish, loading-rate protocols or damaged specimens
must not silently become one history. The program checks declared references, not physical provenance.

The `equilibrium_is_long_time` declaration is mandatory. An instantaneous modulus is not equilibrium
stiffness: adding transient branches to it would double-count stiffness. The equilibrium response may
be the existing scalar orthotropic law or a source-bearing nonlinear response. It remains fixed during
fitting. The nonlinear equilibrium is evaluated at each measured strain, not replaced by its tangent.
Branch ratios multiply the equilibrium **origin tangent**, matching the existing simulation law.

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
not a calibrated library of real linen, leather or performance-knit specimens. Fixed-equilibrium error,
strain uncertainty, temperature/wet dependence, nonproportional directional spectra, plastic set and
damage remain outside this estimator. Broader measurements and independent garment checks are required.

Primary formula/identification references, checked 2026-10-14; no external source code copied:
- pyvisco Prony identification: https://pyvisco.readthedocs.io/en/stable/_autosummary/pyvisco.prony.html
- MOOSE Maxwell model: https://mooseframework.inl.gov/source/materials/GeneralizedMaxwellModel.html
