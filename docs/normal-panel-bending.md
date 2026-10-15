# Normal-projected bending and material-defined preferred curvature

Select `material.structure.bending_model = "orthotropic_normal_patch/1"` to reconstruct
normal curvature rather than penalize tangential second derivatives. This is consumed through
ordinary global or `piece.fabric.material` selections, meshing, welding, Newton forces and body
motion. `orthotropic_quadratic_patch/1` keeps its existing vector-Hessian behavior unchanged.

## Mechanics

The same six-node source patch reconstructs tangents and second derivatives at the integrated
triangle's centroid. Its current normal is `n = normalize(x_u cross x_v)`. With derivatives in
reference material metres and the existing independent tensor D in N m:

```
b = [dot(n,x_uu), dot(n,x_vv), 2*dot(n,x_uv)]
energy = reference_area * (b - preferred)^T D (b - preferred) / 2
```

The normal's derivatives are included in the exact energy gradient and Hessian. Geometric stiffness
is not presumed positive semidefinite; the solver's existing projected-Newton path handles it.
Singular or non-finite tangent frames refuse before stepping. In the absence of preferred curvature,
a regular nonuniform planar stretch produces zero bending energy and force. These are covariant
curvatures, not current principal curvatures: a material-coordinate cylinder stretched by lambda in
its circumferential direction has `b_uu = lambda^2 / radius`.

## Preferred shape from the material

The optional source-bearing quantities `preferred_curvature_warp`, `preferred_curvature_weft`, and
`preferred_curvature_twist` specify `[b0_uu,b0_vv,2*b0_uv]`. They use inverse reference metres (`1/m`);
`1/cm`, `1/mm`, and `1/in` normalize explicitly. A value is a curvature, not a radius. Positive warp
curvature bends toward the oriented current normal. In reference coordinates +u follows the directed
grain and +v is its counterclockwise perpendicular as viewed from the public face of the piece.
Changing the intended face or handedness requires an explicit signed material declaration; the
system never guesses a face from a marketing label. Grain transforms continue through each instance.

Omission of all three quantities retains zero preferred curvature and the original numerical path.
Declaring any one requires all three, including explicit zeros, and the normal-projected model.
The vector-Hessian model refuses preferred-curvature observations rather than silently ignoring them.
Every target keeps its source, method, test conditions, units and evidence policy; estimates require
the selection's existing opt-in. Material-only changes invalidate frozen identities. The complete
source record, including the preferred curvatures, remains in the normal drape report. Low-level
`BendingPatch::normal_curvature` returns geometry; `bending_strain` returns geometry minus target.

The runnable selection is synthetic, not measured leather, linen or a recommended fabric recipe:

```sh
cargo run -p ftx-cli --bin ftx-materials -- bind \
  YOUR-PATTERN.json tests/fixtures/materials/synthetic-preferred-curvature.json \
  curved-pattern.json
cargo run -p ftx-cli --bin ftx-materials -- resolve curved-pattern.json
# Continue with the existing ftx drape workflow, using new output paths.
```

Pattern geometry, mass, membrane stiffness and contact parameters are not changed by these curvature
observations. An explicitly flat or scalar panel does not inherit another panel's preferred curvature.
Support remains on the unwelded source mesh; it does not cross a seam or borrow nodes from another
layer. Interior scalar hinges are replaced; scalar seam bending remains a separate approximation.
Use `ftx drape --spacing-mm` for mesh refinement when a panel is unresolved.

## Limits and verification

Preferred curvature is NOT a supplied stress-free three-dimensional reference surface. The membrane
reference metric remains the flat pattern metric. An incompatible target, such as spherical curvature
with that flat metric, may retain residual stress. This model neither modifies that metric nor infers
pressing, thermal activation, plastic set, natural seam curvature, damage or bending hysteresis. The
curvature is fixed for a run; it is not an evolving actuation or manufacturing-process model.

The six-node reconstruction is nonconforming and centroid-integrated. Mesh refinement and independent
specimen measurements remain necessary; no general convergence, garment-fit or performance guarantee
is asserted. Scalar seam bending is not an experimentally identified seam law.

The tests cover independent quadratic-surface and preferred-curvature energies, exact nodal derivatives,
rigid motion, signed reflection, zero-target equivalence, cylinder refinement, planar membrane forces,
singular-frame refusal, sparse Newton, and opposite spontaneous curls from identical gravity-free
initial conditions. Source-selected targets survive per-piece sewing and body-motion continuation.
The real CLI tests verify immutable geometry, normalized observations and incomplete-target refusal.
All such fixtures are synthetic; the empirical `ftx-tm2` and broader `ftx-tm4` gates stay open.

Primary continuum references (independent implementation; no external code copied):
- Alpern, Kupferman and Maor, *Asymptotic rigidity for shells in non-Euclidean elasticity*,
  https://arxiv.org/abs/2012.12075 (metric/second-form reference data and compatibility).
- *Stretch-Based Hyperelastic Material Formulations for Isogeometric Kirchhoff-Love Shells with
  Application to Wrinkling*, equation (5), https://doi.org/10.1016/j.cad.2021.103075.
