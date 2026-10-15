# Directional bending calibration from coupons

`ftx-materials calibrate-bending COUPONS.json [--allow-estimates]` fits the four directional
panel-bending coefficients used by `orthotropic_normal_patch/1` and returns the ordinary frozen
material selection consumed by binding, per-piece sewn garments, Newton stepping and body-motion
continuation. It does not introduce a calibration-only bending model.

The bundled fixture is entirely synthetic. It exists to exercise the acquisition-to-solver path:

```sh
cargo run -p ftx-cli --bin ftx-materials -- calibrate-bending \
  tests/fixtures/materials/synthetic-bending-coupons.json \
  --allow-estimates > bending-fit.json
python3 -c 'import json; print(json.dumps(json.load(open("bending-fit.json"))["selection"], indent=2))' \
  > bending-selection.json
cargo run -p ftx-cli --bin ftx-materials -- bind \
  YOUR-PATTERN.json bending-selection.json bending-pattern.json
```

## Measurement contract

The schema is `ftx-bending-calibration/1`. Each sample supplies normal material-frame curvature
`[k_uu,k_vv,2*k_uv]` in inverse reference metres and the observed bending-moment resultants
`[M_uu,M_vv,M_uv]` in N per unit undeformed reference width. A missing component is `null`, never
an assumed zero. `scale_n` weights residuals; it is not automatically a standard deviation.

The acquisition method must state coupon dimensions, fixture geometry, how surface curvature was
reconstructed, and how measured forces/torques were reduced to moment resultants. The software does
not infer `D` from membrane modulus, fiber identity, thickness, cantilever marketing data or
`E*t^3`. If a test reports specimen flexural rigidity in N m², the dimensional conversion to a
sheet coefficient requires the actual coupon geometry and belongs in the source-bearing method.

By default, a complete preferred-curvature triplet already present in the material is held fixed. Set `fit_preferred_curvature: true` only when the coupon program contains enough independent curvature states to identify moment offsets as well as stiffness. That mode fits `M = D*k + c` and recovers `k0 = -D^-1*c`; any existing preferred-curvature evidence requires explicit replacement consent. The recovered target is estimate-class evidence even when the coupon forces were measured.

The fitted law is

```
M = D (k - k0)
D = [[D11,D12,0],[D12,D22,0],[0,0,D66]]
```

where `k0` is zero unless the input material already carries a complete, admitted source-bearing
preferred-curvature triplet. The fitter uses scaled least squares but refuses a rank-deficient
experiment, an unstable/non-positive-definite tensor, unapproved estimated observations, or residuals
outside the separately declared fit and held-out budgets. Check coupons never affect the fit.

All fitted coefficients are model estimates even when forces and geometry were measured. The output
retains the full input, prior material evidence, source IDs, predictions/residuals, an input hash and
the excitation diagnostic. Existing directional bending data require `replace_existing: true`;
old values remain in the retained input receipt.

Without `--allow-estimates`, identification still succeeds but the returned selection does not grant
the solver permission to consume fitted coefficients. This is independent of permission to use
estimated observations during fitting.

## Limits

This calibrates the current linear orthotropic bending tensor at one declared conditioning state.
It does not identify bending hysteresis, rate dependence, damage, plastic folds, nonlinear
curvature response, natural seam curvature or a stress-free 3D reference metric. The six-node
normal-curvature discretization still requires mesh-refinement checks.

The runnable synthetic fixture exercises the joint mode and recovers preferred curvature `[1,-0.5,0.25] 1/m` together with all four directional coefficients. The synthetic regressions prove software arithmetic and evidence propagation, not textile characterization. Closing `ftx-tm2`, `ftx-tm4`, `ftx-md1` or the fixed-jacket gate still requires
matched real linen, leather and performance-knit coupons with independent held-out measurements.
