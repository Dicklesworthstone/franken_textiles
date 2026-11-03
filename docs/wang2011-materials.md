# Wang 2011 source-origin cloth observations

The shared offline catalog adds ten study specimens and **50 published numerical observations**
from Wang, O'Brien and Ramamoorthi, *Data-Driven Elastic Models for Cloth: Modeling and Measurement*,
ACM TOG 30(4), Article 71 (2011), DOI [10.1145/2010324.1964966](https://doi.org/10.1145/2010324.1964966).
Each record contains its reported areal density and four entries from the fitted membrane matrix's
**Origin** column. It retains source composition, specimen identity and conditioning limits.

Primary sources inspected on 2026-11-03:

- [Author-hosted parameter supplement](https://jamesobrien.com/papers/Wang-DDE-2011-08/material_parameters.pdf),
  three pages, 2,260,286 bytes; SHA-256
  `f39b2b5270e3588321228d8579bacd844d0b5466be9b43b0c03ce61d1cd47749`.
- [Author-hosted paper](https://wanghmin.github.io/publication/wang-2011-dde/Wang-2011-DDE.pdf),
  section 3.1 equations (1)-(3), section 4.1, Table 1 and section 6.2.

The supplement was checked both as extracted text and as rendered pages. No PDF, source photographs,
textures, raw measurement archive or ARCSim code is included. This is an attributed factual numerical
transcription, not a relicensing of the source data. The source's terms require citation for scholarly
publications, notification of the authors for other uses, and permission for endorsement or other
uses. The repository does not assert that notification has occurred or that a broader license exists.

## Records and scope

| ID suffix after `wang/2011/` | Source specimen | Areal mass (kg/m2) |
|---|---|---:|
| `ivory-rib-knit` | Ivory Rib Knit | 0.276 |
| `pink-ribbon-brown` | Pink Ribbon Brown | 0.228 |
| `white-dots-on-blk` | White Dots on Blk | 0.128 |
| `navy-sparkle-sweat` | Navy Sparkle Sweat | 0.224 |
| `camel-ponte-roma` | Camel Ponte Roma | 0.284 |
| `gray-interlock` | Gray Interlock | 0.187 |
| `11oz-black-denim` | 11oz Black Denim | 0.324 |
| `white-swim-solid` | White Swim Solid | 0.204 |
| `tango-red-jet-set` | Tango Red Jet Set | 0.113 |
| `royal-target` | Royal target | 0.220 |

These are the study's specimens, not current supplier SKUs or generic fiber averages. Source spandex
is represented as elastane; rayon and nylon are not assigned unreported chemical subtypes. Named
rib/interlock knits and the paper's denim twill have those forms; other sheets do not acquire a
construction from a garment-use label or a photograph.

The four source-specific properties are `wang2011_origin_c11`, `wang2011_origin_c12`,
`wang2011_origin_c22` and `wang2011_origin_c33`, in N/m. All retain `published` evidence because they
are reported inverse-fit parameters, not independently repeated mechanical measurements. Their
conditions explicitly identify `strain_basis: green_lagrange_tensor_shear` and
`source_node: lambda_max=0`; temperature, humidity, specimen variability and coefficient uncertainty
remain unreported. A zero-strain node does not establish a finite strain-validity interval.

In the paper, the strain vector is `[E11, E22, E12]` and the fitted matrix maps it to sheet stress.
The existing membrane consumer uses `[E11, E22, 2 E12]`, so the source `c33` must not be relabelled
as its engineering-shear modulus. Negative off-diagonal values are preserved. The Navy Sparkle Sweat
origin has a negative normal-block determinant; it must be refused by a positive-definite local
membrane approximation, rather than repaired by a projection or clamp.

The source's finite-strain interpolation has strain-dependent coupling, and its bending parameters
belong to a particular dihedral-force model. Neither is silently converted to this project's
separable hyperelastic law or sheet bending rigidity. No source thickness, contact friction, thermal
transport, damping or sewing properties are supplied. The paper says its animation friction and
damping were manually chosen; those choices cannot establish empirical interface coefficients.

These records use ordinary catalog discovery, source-preserving normalization and frozen selections.
A consumer that needs only mass can use the reported mass directly. Further constitutive use must
be explicit about its approximation, domain and estimate policy; a raw record alone is not a
complete drape-ready material, a fitted garment or independent empirical acceptance.

## Run an explicit local membrane approximation

`ftx materials linearize-origin REQUEST.json`, the companion
`ftx-materials linearize-origin REQUEST.json`, and portable
`ftx_wasm::materials::linearize_origin` use one evaluator. The browser binding is
`material_linearize_origin`. Start with the checked-in example:

```sh
cargo run -p ftx-cli --bin ftx -- materials linearize-origin examples/wang2011-origin-linearization.json --json
```

The request selects an exact source, explicitly sets `material.allow_estimates: true`, supplies
three closed Green-strain intervals and a dated `approximation_source` explaining the choice.
Every interval must strictly span zero, and the entire box must have a positive-definite surface
metric. The example's +/-0.02 bounds are an illustrative operator choice, not measured accuracy
limits. Unknown conditioning remains unknown. Replacing the example's ID with
`wang/2011/11oz-black-denim` changes the selected source while preserving the approximation choice.

The evaluator takes the source's origin normal block directly: Q11 = C11, Q12 = C12 and Q22 = C22.
It converts tensor shear to the production engineering shear using G12 = C33 / 2. Thus its energy
per reference area is

```text
W = 0.5 * C11 * E11^2 + C12 * E11 * E22 + 0.5 * C22 * E22^2 + 0.5 * G12 * gamma^2
gamma = 2 * E12
```

This is an integrable constant-tangent approximation, not the source's strain-dependent stiffness
interpolation. Existing `TabulatedMembrane` curves carry its exact bounds, work-conjugate stresses,
energy and tangent; ordinary solver assembly needs no new physical path. The production stability
gate admits nine origins and refuses Navy Sparkle Sweat. Existing membrane laws/history are not
replaced. No source coefficient is clamped, projected or relabelled as a direct measurement.

The report contains the original frozen input and digest, source material hash, converted tangent,
ordinary `selection`, and exact `selection_json`. Use that selection as the material in an existing
`ftx-strip-experiment/1` request; the planar static strip needs only the membrane response. An
ordinary full drape still requires separately supplied bending, thickness and interface properties.
Missing warmth, durability and manufacturing data stay missing. Source scalar observations remain
unchanged in the derived record; the added curves and coupling are labelled estimates, and removing
estimate consent refuses their use.

Regressions compare independent quadratic energy/stress answers, numerical energy derivatives,
shear normalization, provenance and domain refusals. The native workflow test uses the same ideal
40 x 100 mm strip and imposed extension for Gray Interlock and black denim, checks the actual
production solver's grip reactions and free-force residuals, and replays each frozen result.
This verifies model integration; it does not compare the approximation to held-out physical coupons.
