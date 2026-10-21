# Grain-aligned panel bending from the shared material record

A material can explicitly select orthotropic panel bending: changing only its warp, weft, coupling
or twist coefficient changes the actual sewn-cloth simulation. The same frozen material selection
reaches uniform garments, source-piece assignments, sparse Newton assembly and force reporting.
No new crate or dependency is needed. This is a numerical model/software delivery within `ftx-tm4`,
not a measured bending library for linen, cowhide or performance knits.

## Select the model

Set `material.structure.bending_model` to `orthotropic_quadratic_patch/1` and provide four ordinary
source-bearing quantities, all normalized to `N*m`:

| Property | Meaning |
|---|---|
| `bending_warp` | D11, curvature along warp/wale/grain |
| `bending_weft` | D22, curvature along weft/course |
| `bending_coupling` | D12, independent normal-curvature coupling |
| `bending_twist` | D66, conjugate to engineering twist curvature 2 k_uv |

The bending matrix is `[[D11,D12,0],[D12,D22,0],[0,0,D66]]`. D11, D22 and D66 must be finite and
positive, and the matrix positive definite. D12 may be negative when stability permits. These are
sheet bending resultants, not membrane stiffness in N/m, bulk modulus in Pa, or a bending Poisson
ratio. No modulus mixing or solid-plate thickness-cubed conversion is inferred. A measured warp
bending observation alone cannot identify the whole tensor. Unknown or partial data may be stored,
but an explicitly enabled incomplete/unstable model refuses before meshing. Merely storing these
properties without the explicit model does not change legacy mechanics.

All quantities use the existing evidence, conditions, source and unit contract. Estimated parameters
require `allow_estimates` in the normal material selection. Model choice and its observations are
part of the existing frozen material hash; edits invalidate its digest lock. No material schema field
or independent catalog is introduced. The original scalar `bending_rigidity` remains required for the
separate seam approximation, even on a tensor-selected panel.

## Runnable synthetic workflow

The example is entirely synthetic, not a recommended law for a named textile. Bind it to a pattern
with explicit grainlines, using a new destination:

```sh
cargo run -p ftx-cli --bin ftx-materials -- bind \
  YOUR-PATTERN.json tests/fixtures/materials/synthetic-panel-bending.json \
  directional-pattern.json
cargo run -p ftx-cli --bin ftx-materials -- resolve directional-pattern.json
```

Then use `directional-pattern.json` with the existing `ftx drape` workflow and the same body, fit
contract and numerical options. Each source panel must be meshed finely enough to resolve quadratic
curvature. When the geometry refusal asks for refinement, reduce the drape `--spacing-mm` value rather
than substituting a scalar law. Readiness in `bind` is a material-property check, not a guarantee that
a particular panel mesh is resolved. Binding preserves pattern geometry and never overwrites its
output. A source piece can use its own `piece.fabric.material`; an explicitly scalar lining does not
inherit the default shell's tensor law. Existing heterogeneous contact evidence requirements remain.

## Discretization and seam policy

Each retained source triangle integrates one six-node quadratic curvature patch over its own full
reference area. Coordinates are in metres, with u along the already scaled/mirrored grainline.
The patch reconstructs the vector derivatives `[x_uu,x_vv,2 x_uv]`. Its energy is

```
U = (reference_area / 2) * sum_ij Dij * dot(curvature_i, curvature_j).
```

The stencil reproduces quadratic fields, annihilates affine maps and is objective under a rigid
rotation of the whole spatial configuration. It has an exact gradient and a constant positive
semidefinite Hessian. The sparse assembly includes all six-node couplings, not the former four-node
hinge sparsity. Adding a panel field to a previously used low-level solver invalidates its old
symbolic/numerical cache before the next solve. Once bound, changing a material requires rebuilding
the model. Construction is transactional: an unresolved patch leaves the previous model unchanged.

Support is chosen deterministically from the **unwelded source mesh**, expanding inward by at most
three edge rings and bounded to 96 candidate nodes. Selection requires three independent quadratic
residuals in addition to the triangle; ill-conditioned or too-coarse patches refuse. Coincident points,
seam welds, slits and separate layers do not create source neighbors. No support is borrowed across a
seam to make a poorly resolved panel pass. Mixed scalar/tensor triangles in one connected source panel
must instead be separated by an explicit seam.

Internal scalar hinges on a tensor-selected panel are **replaced**, not added to the tensor energy.
Scalar hinges across sewn source-panel boundaries retain the existing scalar seam approximation;
scalar panels also retain their original hinges. The seam value is not inferred from D or represented
as a newly calibrated physical seam stiffness. The report identifies this policy, each record's
matrix, patch count, retained material-triangle ownership, panel energy and active scalar hinges.
Source-piece mass, membrane coefficients, rest geometry and welds are otherwise unchanged.

The panel contribution enters both the solver potential and reported elastic forces. Per-triangle
Maxwell membrane history remains a separate additive response: cloning, accepted-step updates and
failure rollback preserve it. A scalar-only or absent tensor selection keeps the original numerical
path; homogeneous default versus identical explicit piece selections is tested with self-contact.

## Important model limit

For an explicit normal-projected alternative that removes tangential-curvature energy under
nonuniform planar stretch, see `normal-panel-bending.md`. The model described below remains
unchanged and is not silently replaced for existing material selections.

This is **quasi-isometric vector-Hessian bending**, not a finite-stretch Koiter shell. For an isometric
embedding the vector Hessian represents the second fundamental form times the normal. For general
nonuniform in-plane deformation it also contains tangential derivatives and can store energy that a
full shell-curvature law would not. Affine stretch is annihilated, but that does not make arbitrary
large textile stretch correct. The six-node displacement interpolation also requires refinement on
curved configurations. Cylinder and small-slope cantilever convergence in the tests is not a proof
of convergence for every irregular mesh or garment. Do not use this model as a validated highly
stretchable-knit bending law without independent physical and discretization checks.

The model does not provide nonlinear bending moment/curvature curves, natural rest curvature, fold
plasticity, bending hysteresis, rate-dependent bending, yarn/loop homogenization, a measured seam law
or empirical material parameters. It is stress-free only for affine-flat reference geometry. No
performance improvement, real-garment fit or machine-process suitability is claimed.

## Direct verification

Tests cover source-qualified tensor stability/units/permissions and frozen identities; independent
quadratic curvature and twist energy; rigid motion and affine invariance; exact nodal derivatives and
PSD curvature Hessians; directional cylinder-energy refinement; a small-slope cantilever tip known
answer; actual free-cloth sag; stale sparse-factor invalidation; scalar bit-identical paths; source-
piece ownership through sewn garments; material-only swapped warp/weft dynamics; self-contact;
transactional membrane-history coexistence; and actual binary bind/resolve/readiness refusals.
All physical input fixtures are synthetic. The remaining `ftx-tm2`/`ftx-tm4` acceptance requires measured
construction-specific warp/weft/bias bending, held-out drape/cantilever observations and qualified seam
behavior. Keep these separate from numerical derivative checks and source hashes.

Primary continuum convention cross-checks (independent implementation; no external code copied):
- FEniCS-Shells Kirchhoff-Love plate demo:
  https://fenics-shells.readthedocs.io/en/latest/demo/kirchhoff-love-clamped/demo_kirchhoff-love-clamped.py.html
- FEniCS-Shells orthotropic von Karman example, engineering-curvature Voigt convention:
  https://fenics-shells.readthedocs.io/en/latest/demo/von-karman-mansfield/demo_von-karman-mansfield.py.html
