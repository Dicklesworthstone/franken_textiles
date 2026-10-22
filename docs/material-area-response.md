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
source. Identification without an explicit area policy refuses a material carrying this response;
it cannot silently discard it or claim to have identified the two additional coefficients.

## Identify an area law from homogeneous swatches

`materials calibrate-nonlinear` jointly identifies directional knot resultants, normal coupling and
the two area coefficients when its `ftx-nonlinear-swatch-calibration/1` input includes:

```json
"area_response": {
  "model": "area_green_strain/1",
  "area_ratio_range": [0.7, 1.5]
}
```

Supply full, measured deformation gradients and whichever nominal force-resultant channels were
actually observed. Grip extension alone is not a homogeneous deformation gradient. The nonlinear
swatch fitter uses the same work-conjugate force definition and analytical area basis as the
production material. It fits all coefficients together and refuses insufficient independent
excitation, negative/invalid area coefficients, unstable directional response or excessive FIT or
held-out CHECK residuals. This mode requires explicit shared `specimen`, matching `specimen_id`, and
`conditioning` values; it does not infer a construction from the specimen's fiber composition.

The declared area interval limits admissible data. The published `fitted_area_ratio_range` is limited
to actual FIT area ratios, including the undeformed reference and intersecting prior qualification.
CHECK observations cannot extend that domain. Directional Green-strain domains are likewise bounded
by FIT data. Every fitted coefficient remains a source-bearing estimate even when the acquisition
observations were measured. Input estimate admission and output estimate consumption are separate
decisions.

The runnable [synthetic swatch example](../examples/area-identification/swatch.json) uses six FIT
deformations and one held-out CHECK deformation. All four nominal force components are supplied.
Its forces were calculated independently as `P = F S`, with compression/tension warp slopes
200/350 N/m, compression/tension weft slopes 150/250 N/m, negative/positive engineering-shear slopes
70/90 N/m, coupling 20 N/m, quadratic coefficient 40 N/m and log-squared coefficient 15 N/m.
The example declares three zero interior knots; all nine unknown coefficients are independently
excited. Its FIT area ratios cover approximately 0.748331 to 1.449138. These numbers are synthetic
software observations, not measurements of a fabric or validation of a garment.

```sh
cargo build --locked -p ftx-cli --bin ftx-materials

target/debug/ftx-materials calibrate-nonlinear examples/area-identification/swatch.json \
  --allow-estimates > /tmp/area-swatch-identification.json
```

The output contains the complete calibration report, an ordinary frozen `selection`, and its exact
portable `selection_json`. `ftx materials calibrate-nonlinear ... --allow-estimates --json` provides
the same result inside the primary CLI envelope. The WASM `material_calibrate_nonlinear` entry point
calls the shared identification engine and publishes that same selection.

Use the identified material immediately in a membrane-only finite-strip experiment, without editing
the material compiler or solver:

```sh
python3 - <<'PY'
import json

with open("/tmp/area-swatch-identification.json") as source:
    identified = json.load(source)
request = {
    "schema": "ftx-strip-experiment/1",
    "material": identified["selection"],
    "fixture": {
        "gauge_length_m": 0.1,
        "width_m": 0.06,
        "warp_axis": [0, 1],
        "cells": [2, 4],
    },
    "extensions": [0, 0.02],
}
with open("/tmp/area-identified-strip-input.json", "x") as output:
    json.dump(request, output, indent=2)
    output.write("\n")
PY

target/debug/ftx-materials strip-experiment /tmp/area-identified-strip-input.json
```

The material's source and domain declarations travel with its frozen selection. Editing a coefficient
without updating its content identity refuses at resolution. Forward replay uses the report's exact
`input`; neither native nor WASM callers fit again during replay. The example deliberately supplies no
mass, bending, thickness, friction or thermal evidence, so full garment mechanics remain unavailable.

## Identify directionality with a physical-area law

The existing `ftx-strip-calibration/1` request accepts an optional top-level `area_response` policy
with a scalar orthotropic base, `response_family`, or independent signed `nonlinear` intervals.
An existing source-qualified area law supplies its positive J interval and matched conditions.
Fresh identification instead requires an explicit `area_ratio_range`; no prior coefficient values
are needed. The request's conditions must retain every common specimen/conditioning qualifier.
A scalar base also requires explicit signed trial support, as described below. Neither a fiber
label nor nominal stretch supplies missing coefficients or domains.

To retain independently qualified coefficients while identifying directionality:

```json
"area_response": { "mode": "preserve", "allow_estimates": true }
```

This leaves both quantities, sources, methods, reported bounds and J interval unchanged. Consent
to estimated fixed coefficients is independent of estimated observations, assumed shape curves and
fitted-output admission. Known prior fitted-law metadata and shared CHECK source references refuse:
coefficients fitted from the validation acquisitions cannot serve as an independent fixed prior.
Different source declarations alone do not prove physical independence; source review remains needed.

To identify both coefficients jointly with the directional parameters:

```json
"area_response": { "mode": "identify", "parameter_bounds": [[10, 50], [5, 35]] }
```

These **synthetic example bounds**, in N/m, append quadratic then log-squared coordinates after the
base parameters. Each interval is finite, nonnegative and nonempty; at least one lower endpoint
is positive so the whole box retains a nonzero reference area tangent. Original coefficient values
do not choose starts or priors. The same three bounded searches, complete forward-solve budget,
column-normalized excitation, stationarity, equivalent-minimum, CHECK and doubled-mesh gates apply
to the entire joint vector. Weak excitation returns diagnostics without a material selection.

### Start from acquisitions without prefilled material coefficients

For fresh identification, add `area_ratio_range` to the Identify policy. It declares admissible
trial area ratios, not measured support or an output confidence interval. If the input already
has an area law, the explicit interval intersects the inherited interval and cannot enlarge it.
Preserve mode still requires an independently qualified existing law.

With a scalar orthotropic base, omit `response_family` and `nonlinear`, supply the ordinary four
`parameter_bounds` for `[E1, E2, nu12, G12]`, and declare negative and positive trial support for
`[E11, E22, 2 E12]`:

```json
"parameter_bounds": [[270, 320], [145, 180], [-0.52, -0.37], [80, 112]],
"area_response": {
  "mode": "identify",
  "parameter_bounds": [[10, 50], [5, 35]],
  "area_ratio_range": [0.65, 1.45],
  "scalar_green_strain_range": [[-0.4, 0.8], [-0.4, 0.8], [-0.4, 0.8]]
}
```

These bounds are **synthetic software-example assumptions**. Scalar trial support is also required
for scalar Preserve mode, and must be omitted when a declared family or independent intervals already
define the base domain. Missing, unsigned, nonfinite or incompatible support refuses before fitting.
Fresh area identification requires explicit nonempty `specimen` and `conditioning` declarations.

The four scalar coefficients describe the **base** orthotropic law. Its exact constitutive matrix
is represented by ordinary signed linear curves and constant coupling; the separate physical-area
energy then uses the existing production compiler. The reported base Poisson ratio is therefore not
the effective Poisson ratio of the combined area-augmented material. No second solver or approximate
conversion is used, and publication replaces the scalar mechanical properties with the complete
canonical response so consumers cannot accidentally omit its area contribution.

The [fresh scalar request](../tests/fixtures/materials/synthetic-scalar-area-clamped-strip.json)
contains all 13 observation rows and fixtures from the existing synthetic strip example, with no
prefilled mechanical properties, membrane response or shape family. It recovers the six coordinates
`[294.44444444444446, 160.6060606060606, -0.4444444444444444, 96, 31, 19]`: the first four are
base `E1, E2, nu12, G12`, followed by the quadratic and log-squared area coefficients. The base matrix
has `Q11 = 330`, `Q22 = 180`, `Q12 = -80`, and `Q33 = 96` N/m. The returned frozen selection can be
used directly by `strip-experiment` and other compatible material consumers:

```sh
ftx --json materials calibrate-strip \
  tests/fixtures/materials/synthetic-scalar-area-clamped-strip.json --allow-estimates
ftx-materials calibrate-strip \
  tests/fixtures/materials/synthetic-scalar-area-clamped-strip.json --allow-estimates
```

The same request works with the existing WASM `material_calibrate_strip` export and browser strip-fit
controls. The report retains the input trial domains, fitted physical-area support, coefficient units,
independent residuals and exact frozen selection. The fixture supplies no empirical fabric, bending,
mass, contact or thermal qualification.

### Publish only observed FIT support and replay the complete law

Every trial uses the production area compiler and physical Green-strain/J domain. Publication clips
the directional law to FIT-only local strain support. Identify mode also clips its J interval to the
actual coarse and refined FIT element states, including the undeformed reference, intersected with
the inherited area domain. CHECK cannot enlarge either support. The report retains these exact bounds
as `fitted_area_ratio_range`, and all six fitted components share their source and domain metadata.
Preserve mode retains the independent coefficients and original J interval unchanged. Both modes
replay the complete published response, including its area term, through ordinary production solves.
Identified coefficients are estimates with their acquisition receipt; search intervals are not
published as uncertainty bounds. Preserved coefficients retain their original evidence. Both modes
use the existing native, portable and browser strip-calibration entry points and frozen selections.

The [complete synthetic inverse request](../examples/area-response/identify.json) supplies three FIT
directions and a separate numerical CHECK acquisition, modest net coupling, explicit midpoint widths
and a 4 × 6 mesh. It was captured from the accepted numerical experiment, not from a physical textile
or a parity oracle. It demonstrates recovery of `[1.1, 0.9, 1.2, -80, 31, 19]` in the reported parameter
order. Run it through either native command or upload it to the existing browser strip-fit input:

```sh
ftx --json materials calibrate-strip examples/area-response/identify.json --allow-estimates
ftx-materials calibrate-strip examples/area-response/identify.json --allow-estimates
```

The first three coordinates are dimensionless shape scales; the last three are N/m. Successful
publication still leaves bending, contact, mass and thermal observations absent. Stronger coupling,
knotted shapes or other acquisitions can fail refinement or strict domain replay and remain rejected;
this example does not establish convergence for every admissible request.

Numerical recovery of invented coefficients is software evidence. Published shell parameters fitted
using all cut directions cannot be relabeled as an independent bias CHECK prior. Matched acquired
transverse/biaxial observations, independent coupons and bending/contact qualification remain under
`ftx-ki4`; this extension does not establish a solver-ready empirical garment or parameter confidence.

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
