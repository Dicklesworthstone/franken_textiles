# From a measured swatch to the same pattern's cloth simulation

The `ftx-materials calibrate` command identifies the existing effective orthotropic membrane from
planar force/deformation observations. It emits the ordinary source-bearing material selection used
by `bind`, `resolve`, `ftx drape`, rounds and motion; there is no separate calibration-only solver.
This closes the measurement-to-consumer software path, not the empirical-characterization gate.

## Runnable example

The bundled fixture is **entirely synthetic**, not linen, leather or a commercial performance knit.
Run from the repository root, using new output paths:

```sh
cargo run -p ftx-cli --bin ftx-materials -- calibrate \
  tests/fixtures/materials/synthetic-swatch.json --allow-estimates > swatch-fit.json
python3 -c 'import json; print(json.dumps(json.load(open("swatch-fit.json"))["selection"], indent=2))' \
  > swatch-selection.json
cargo run -p ftx-cli --bin ftx-materials -- bind \
  YOUR-PATTERN.json swatch-selection.json characterized-pattern.json
cargo run -p ftx-cli --bin ftx-materials -- resolve characterized-pattern.json
```

`bind` preserves pattern geometry and refuses to overwrite its destination. The calibrated pattern
then enters the existing `ftx drape` workflow with the same body and numerical run options.
The independent sewn-band integration test actually runs the solver twice: only observed swatch forces
change, while pattern pieces, seam topology, mass and bending stay fixed; the fitted membrane and final
positions change. This is a causal software test, not real garment fit evidence.

Without `--allow-estimates`, calibration still reports the fit, but its portable selection does not
admit estimated stiffness to simulation. `simulation_ready` is false and `mechanics_refusal` explains
why. Allowing estimated *input observations* is a separate explicit choice in the input JSON.

## Supply observations, not fiber-name defaults

`ftx-swatch-calibration/1` inputs contain the actual specimen `material`, source map, method,
conditioning, analysis date and `samples`. Every sample has a distinct ID, a source key, evidence class,
`fit` or held-out `check` partition, positive force weighting `scale_n_m`, deformation gradient
`deformation`, and measured `nominal_n_m` force-resultant components. The fixture is the complete schema
example. Keep different construction, finish, humidity, temperature and loading-rate conditions in
separate fits; attach raw instrument records and explain the deformation reconstruction.

The two columns of F are the deformed warp/weft (wale/course) axes per unit undeformed length. P is first
Piola force per **undeformed width** in N/m, not Cauchy stress, breaking force, bulk Young's modulus or
force divided by current width. In an axis-aligned strip, P11 is axial force divided by original width;
lateral deformation must still be observed. `null` means an unmeasured component; zero is a physical
traction observation and must be justified by the fixture. Independent warp, weft and shear excitation
is required; one percent-stretch number cannot identify the law.

The model uses `E = (F^T F - I)/2`, `S = Q [E11,E22,2E12]`, `P = F S`, matching the unsoftened drape
membrane. Weighted least squares identifies Q11, Q22, Q12 and G12. Column scaling and an excitation
check refuse deficient fits; positive-definiteness and separate fit/check RMS and peak residual
budgets refuse unstable or incompatible laws. Check observations never influence identification.
No held-out samples means no independent validation, explicitly reported as `check: null`.

The complete input, prior values, strain envelope, force predictions/residuals and input hash survive
in the calibration receipt. Retain `swatch-fit.json` beside the frozen pattern; its source reference
identifies that receipt, not an automatically hosted file. Fitted properties are model **estimates**,
even when observations were measured. Replacement of existing stiffness evidence requires
`replace_existing: true`, with the old values retained in the receipt. No confidence interval is
invented from the declared weighting scale or residual budget.

The FIT-only `fitted_green_strain_range` is also an enforced operating domain. All four fitted
coefficients must retain the same source and range; incomplete or mixed metadata refuses. Ordinary
material resolution carries these bounds through each piece's grain frame into its retained cloth
triangles. Scene construction, post-untangling admission, Newton trials and accepted-state reactions
check raw physical Green strain before numerical compression softening. An out-of-range trial is
backtracked, not evaluated by unrestricted extrapolation. The box does not establish measured
accuracy at every combined strain inside it, and held-out samples never enlarge it.

The synthetic fixture includes explicit signed compression and shear force observations so its sewn
band example stays within its own supported range. A regression retains the original narrow fit and
verifies refusal when scene construction requires negative shear. Replacing sheet materials clears
old scalar-fit domains together with old nonlinear laws; the new material must bind its own bounds.
Existing Maxwell and permanent-set histories also reject a changed domain, even when scalar
coefficients happen to be identical.

Mass, thickness, bending, skin/self friction and thermal properties must be characterized separately.
They remain missing unless present in the supplied source-bearing material. The command reports those
gaps rather than inventing values from composition. Strongly nonlinear stretch, shear locking, cyclic
recovery, rate dependence, irreversible deformation and construction homogenization need richer laws;
a small fit residual does not license extrapolation outside the reported strain envelope. Gather-zone
compression softening remains the solver's separate existing model.

Plan §25 / bead `ftx-tm2`: real, independently checked linen/leather/knit specimens and directional
bending/contact data remain the next acceptance work. This implementation does not close that bead.
