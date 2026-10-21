# Material-controlled resistance to area change

An optional `membrane_response.area_response` adds a source-qualified area energy to the existing
directional membrane. Material selection controls this response through the same compiler, triangle
assembly and equilibrium solver as the base warp, weft, shear and coupling observations.

This supplies a mechanism for representing transverse behavior that a constant normal coupling alone
cannot reproduce over a large deformation. The coefficients and applicability range must be supplied
for the actual construction; a knit label or fiber composition does not determine them.

## Constitutive meaning

The model identifier is `area_green_strain/1`. With physical material-frame Green strain
`e = [E11, E22, 2 E12]`, the metric and positive area ratio are

```text
C = [[1 + 2 e[0], e[2]], [e[2], 1 + 2 e[1]]]
J = sqrt(det(C))
W_area = quadratic * (J - 1)^2 + log_squared * (ln J)^2
W_total = W_base + W_area
```

Both coefficients use **N/m**, equivalently J/m² of reference sheet area. There is no one-half
prefactor on either area term. The logarithm is natural. The compiler and solver use analytical
stress and tangent derivatives in the same work-conjugate strain convention as the base response.

At the reference state, area energy and stress vanish. Each normal diagonal tangent and the normal
cross tangent gain `2 * (quadratic + log_squared)`; the reference shear tangent is unchanged. At an
area-preserving deformation (`J = 1`), the area contribution has zero energy and stress. These are
numerical identities of the declared model, not fitted textile measurements.

The energy form follows Sperl et al. (2022), §5.1, equation (1), which augments anisotropic StVK
elasticity with two area terms. That study fitted the in-plane model using forces and transverse
contraction from all three cut directions. Its data do not supply an independent biaxial validation
set. Here the area terms also work with the existing authored directional curves; that extension is
an explicit modeling choice. No published fitted coefficient is automatically imported or assigned
to a catalog fabric.

Primary source: [Estimation of Yarn-Level Simulation Models for Production Fabrics](https://mslab.es/projects/YarnLevelFabrics/contents/sperl2022eylsmpf.pdf),
Sperl, Sánchez-Banderas, Li, Wojtan and Otaduy, *ACM Transactions on Graphics* 41(4), 2022,
[doi:10.1145/3528223.3530167](https://doi.org/10.1145/3528223.3530167).

## Source and domain contract

The optional object contains:

| Field | Meaning |
|---|---|
| `model` | Exactly `area_green_strain/1` |
| `quadratic` | Ordinary source-bearing `Quantity` multiplying `(J - 1)^2` |
| `log_squared` | Ordinary source-bearing `Quantity` multiplying `(ln J)^2` |
| `area_ratio_range` | Explicit closed `[minimum, maximum]` for physical `J` |
| `conditions` | Shared specimen, conditioning and any additional required qualifiers |

Each coefficient is finite and nonnegative, with at least one positive. Its unit is exactly `N/m`;
its method and source must resolve normally. Estimated observations require the existing explicit
selection estimate policy. Bounds, when supplied, retain their ordinary interpretation as reported
parameter bounds; the solver does not turn them into output confidence intervals.

The common conditions include nonempty `specimen` and `conditioning` values. Every common key/value
must also occur on the warp, weft, shear, normal-coupling and both area-coefficient observations.
This prevents silently joining one specimen's base law with another specimen's area coefficients.
Matching declarations are an admission requirement; their accuracy still depends on the evidence.

The area-ratio interval must have `0 < minimum <= 1 <= maximum`, with distinct endpoints. It intersects
the base Green-strain ranges and any inherited identification domain. A state outside any applicable
domain refuses; it is never clamped or assigned a fiber-dependent default. The physical metric must
remain positive definite. `J` is the unsigned surface-area ratio for an embedded sheet, so this energy
does not establish orientation preservation, collision freedom or global convexity.

The area energy acts on raw physical strain even when a numerical gather/compression treatment softens
the base directional contribution. It is stored elastic energy, not a plastic or viscoelastic heat
source. Existing scalar/separable identification workflows refuse a material carrying this response;
they cannot silently discard it or claim to have identified the two additional coefficients.

## Run the same coupon with two material responses

The committed [baseline request](../examples/area-response/baseline.json) and
[area-enabled request](../examples/area-response/enabled.json) contain the same base membrane, fixture,
loading states and solver settings. Their only input difference is the additional `area_response`.
All coefficients and qualifications are explicitly synthetic. The chosen `J` range is an example
domain, not a measured applicability range.

The strip is 100 mm between ideal full-width grips and 60 mm wide. Warp aligns with the loading
direction (`warp_axis: [0, 1]`). The mesh has four cells across its width and six along its length.
The states impose 2%, 6% and 10% grip extension. Interior points can contract and shear in the plane;
the reported width is the chord between material side points at half the original gauge length.

```sh
cargo build --locked -p ftx-cli --bin ftx-materials

target/debug/ftx-materials strip-experiment examples/area-response/baseline.json \
  > /tmp/area-baseline-report.json
target/debug/ftx-materials strip-experiment examples/area-response/enabled.json \
  > /tmp/area-enabled-report.json
```

Use new output filenames when retaining an earlier experiment. Both commands produce ordinary
`ftx-strip-experiment-report/1` output with grip reactions, midpoint widths, energy, residuals and
positions. Each report's `input.material` is a frozen source-bearing selection. Different area
coefficients or source/domain declarations change that selection's content identity.

The production forward solver gives the following numerical comparison for these synthetic inputs
and this 4 × 6 mesh. Widths are converted from the report's metres to millimetres for readability.

| Grip extension | Baseline reaction (N) | Area-enabled reaction (N) | Baseline midpoint width (mm) | Area-enabled midpoint width (mm) |
|---|---:|---:|---:|---:|
| 2% | 0.176738 | 0.198073 | 58.015149 | 58.325483 |
| 6% | 0.588190 | 0.685626 | 53.808034 | 55.207835 |
| 10% | 1.090489 | 1.304122 | 49.318030 | 52.395037 |

For this declared material, adding the area energy increases grip reaction and reduces lateral
contraction. Across the six accepted states the largest free-force residual is below `1.94e-8 N`,
and the largest grip-balance residual is below `1.81e-14 N`. Both reports replay byte-for-byte from
their frozen inputs. These checks demonstrate the material-to-solver connection; the table is not
a measured fabric comparison or a mesh-convergence claim.

Inspect the effects and retain the frozen enabled input for replay:

```sh
python3 - <<'PY'
import json

for name in ("baseline", "enabled"):
    with open(f"/tmp/area-{name}-report.json") as source:
        report = json.load(source)
    print(name, report["input_sha256"])
    for state in report["states"]:
        print(state["extension_fraction"], state["force_n"], state["mid_width_m"])
    if name == "enabled":
        with open("/tmp/area-enabled-frozen-input.json", "x") as output:
            json.dump(report["input"], output, indent=2)
            output.write("\n")
PY

target/debug/ftx-materials strip-experiment /tmp/area-enabled-frozen-input.json
```

The complete request can also be submitted to the existing browser strip-experiment workbench or
shared WASM forward interface. [Clamped-strip calibration](clamped-strip-calibration.md) documents
those entry points and their exact-selection workflow.

These examples deliberately leave mass, bending, thickness, friction and thermal properties unknown.
A successful membrane-only strip experiment therefore does not make either selection a complete
garment material. Real knit fitting, matched bending/contact evidence and independent garment
comparisons remain separate work under `ftx-ki4`, `ftx-tm2` and `ftx-tm8`.
