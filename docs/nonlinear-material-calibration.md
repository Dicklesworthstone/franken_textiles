# Identify nonlinear fabric behavior from swatches

`ftx materials calibrate-nonlinear SWATCHES.json --allow-estimates` and
`ftx-materials calibrate-nonlinear SWATCHES.json --allow-estimates` identify the existing
tabulated membrane law from source-bearing planar deformation and force observations. The result
includes the complete input, fitted material, separate fit/held-out residuals, missing mechanics and
a portable, hash-locked `selection` plus its exact `selection_json` text. Bind that text with the existing material-binding commands;
the ordinary drape resolver and per-piece material path consume the curves without solver edits.

The browser binding `material_calibrate_nonlinear(inputJson, allowEstimates)` calls the same Rust
identification engine. Its output adds the existing material capability report. It performs no network
access or file writes. The [browser material workbench](browser-material-workbench.md) exposes this
same operation: choose nonlinear swatches, import the acquisition JSON, and identify the material.
Inspect the separate fit and held-out residuals, characterized strain domain, evidence and missing
capabilities before applying the selection to the pattern default or selected pieces. Fitted estimates
require explicit consent. The workbench also supports timed recovery histories through the shared
relaxation fitter.

The complete calibration report, original acquisition text and frozen selection can be downloaded
independently of a pattern. Preserve `selection_json` as text when transferring it between tools:
parsing and reserializing its embedded evidence can change signed zero and invalidate its source lock.
Failed identification, file replacement or changes to estimate consent invalidate the pending
selection, so a prior result cannot be applied accidentally.

## Input and observation frames

Use schema `ftx-nonlinear-swatch-calibration/1`. The executable numerical example is
[`synthetic-nonlinear-swatch.json`](../tests/fixtures/materials/synthetic-nonlinear-swatch.json).
Every value in that fixture is synthetic; it is not a recommended cotton, leather or knit material.

The input contains a normal `material`, source map, nonempty `method`, explicit `fitted_on` date,
`conditions` including `specimen_id` and `conditioning`, and 1–4096 samples. If the material declares
`structure.specimen_id`, it must match. Different humidity, finish, temperature or preconditioning
states need separate characterization. Fiber, yarn and loose-fill records cannot be calibrated as
effective sheets through this command.

Each sample uses the same frame as scalar swatch calibration:

- `deformation`: full 2×2 deformation gradient F. Columns are deformed warp/wale and weft/course
  directions per unit undeformed length; rows are spatial coordinates. The determinant must be positive.
- `nominal_n_m`: partial 2×2 first-Piola force resultants P, in N per metre of **reference** width.
  For an aligned strip, P11 is tensile force divided by undeformed strip width. `null` means that the
  channel was not measured; it never means a measured zero lateral reaction. Measure lateral strain.
- `scale_n_m`: positive weighting scale for that state's observed force components. It is not an
  automatically inferred measurement uncertainty or confidence interval.
- `id`, `source`, `evidence` and `partition`: distinct state identity, existing material source,
  observation evidence and either `fit` or independently held-out `check`.

The fit works directly with P = F S and material-frame Green strains
`[E11,E22,gamma12] = [(F11²+F21²−1)/2,(F12²+F22²−1)/2,F11F12+F21F22]`.
It does not treat finite-strain nominal loads as infinitesimal Young moduli.

## Identified law and its domain

The second-Piola resultants are:

```text
S11 = warp(E11) + coupling * E22
S22 = weft(E22) + coupling * E11
S12 = shear(gamma12)
```

The three stress curves are piecewise linear, with zero stress fixed at zero strain and one jointly
identified constant normal-coupling coefficient. This supports different tension/compression slopes,
toe regions, tensile stiffening and nonlinear shear within the existing separable material model.
It does not identify arbitrary multiaxial cross-coupling, hysteresis or damage.

`interior_knots` supplies three increasing arrays of 1–15 Green-strain values, each including zero.
The default is `[[0],[0],[0]]`. The two endpoints of each curve are the actual minimum and maximum
strain in the **fit** observations; every interior knot must lie strictly between them. Both negative
and positive strains must be characterized on all three axes. A tensile-only strip cannot invent a
compression branch, and an unsigned shear test cannot invent the reverse-loading response. Obtain
suitable effective-sheet compression/buckling evidence or leave the material unqualified.

The fit identifies nonzero-knot stresses and coupling by weighted least squares using the existing
deterministic SVD. It checks the column-normalized design's rank before solving; unexcited knots or
unidentifiable coupling refuse. More knots require more independent observations. No regularization,
positivity penalty, interpolation default or fabricated force fills an unobserved degree of freedom.

The production membrane compiler checks increasing stresses and positive coupled tangents over the
entire fitted strain box. An unstable fit refuses instead of changing coefficients to make it pass.
Both held-out predictions and simulation states must remain inside that box. Held-out observations
never change parameters or extend the fitted domain. The residuals are recomputed using the compiled
production law, not a separate prediction implementation.

### Joint physical-area response

The same input can additionally declare `area_response` with `model: "area_green_strain/1"` and
an admissible positive `area_ratio_range` containing one. This jointly identifies the two production
energy coefficients of `(J-1)²` and `(ln J)²`, in N/m, alongside directional knot stresses and normal
coupling. `J` is physical deformed area divided by reference area; no area-preserving assumption is
introduced. See the independent analytic [swatch example](../examples/area-identification/swatch.json).
Its observations are synthetic software inputs, not measured fabric data.

The declared J range limits admitted observations. The published `fitted_area_ratio_range` comes
only from FIT extrema including the reference state, intersected with that declaration. CHECK cannot
expand it or alter the coefficients. The resulting area quantities retain the same source, specimen
and conditioning as the directional law. Rank deficiency, unstable response, unsupported J or failed
residual budgets refuse the fit; small residuals do not establish uniqueness or confidence intervals.

In the workbench, choose nonlinear swatches and upload this ordinary request. The result displays
both native area coefficients, units and evidence, plus separate declared and FIT-supported J ranges.
The exact acquisition, full report and frozen selection use the existing downloads and assignment
controls. Omitting `area_response` retains separable identification and its existing display.

## Evidence, acceptance and reuse

`max_rms_scaled` and `max_peak_scaled` are positive acceptance budgets applied independently to the
fit and held-out partitions. The report preserves predictions for every force channel and residuals
only for observed channels. A missing held-out partition is reported as absent, not as a passed check.

`allow_estimated_observations` admits estimated **input** forces explicitly. Fitted curves remain
`estimate` evidence even when the observations are measured. The command's `--allow-estimates` flag
separately controls whether the returned selection admits those fitted values to simulation. Without
it the fitted report is still inspectable, but its estimated mechanics cannot drive the solver.

The result adds a source tied to the complete input's SHA-256, protocol and conditions. Existing mass,
thickness, bending, friction and thermal observations are retained. Missing properties remain absent.
Existing scalar or nonlinear equilibrium evidence requires explicit `replace_existing: true`; the
original material remains in the report input, and the output removes conflicting scalar stiffnesses.
A material with existing membrane-relaxation history refuses equilibrium replacement: its recovery
law must be recharacterized explicitly rather than silently attached to a different reference law.

The regression suite covers independent analytic compression/toe/stiffening/shear/coupling forces,
partial sensors, spatial-rotation invariance, rank deficiency, stability and evidence refusals,
held-out misfit, immutable replay, both native binaries, the portable API, and changed actual sewn-band
dynamics with identical pattern, mass and bending. These are numerical and integration checks. Real
linen, treated cowhide and performance-knit records still need matched source measurements and
independent coupons, followed by the unchanged-jacket/body validation in `ftx-tm8`.
