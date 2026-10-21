# Fujii cotton-weave mechanical measurements

Three records, `fujii/2026/sheeting-a`, `fujii/2026/oxford-b`, and
`fujii/2026/twill-c`, preserve specimen-specific data from Fujii, Nakashima,
Santo, Kinari and Okura, *Empirical hyperelastic modeling of textile fabrics for
measuring nonlinear bending rigidity*, Engineering with Computers 42, 107 (2026),
[doi:10.1007/s00366-026-02343-z](https://doi.org/10.1007/s00366-026-02343-z).
The article is [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
This catalog transcribes numerical tables and converts the explicitly identified
units; it includes no article illustrations. Retrieved 2026-10-18. The paper
states that implementations and data are available on reasonable request;
raw replicate data have **not** been ingested.

```sh
cargo run -p ftx-cli --bin ftx-materials -- catalog fujii/2026/
cargo run -p ftx-cli --bin ftx-materials -- show fujii/2026/oxford-b
cargo run -p ftx-cli --bin ftx -- materials compare fujii/2026/sheeting-a fujii/2026/twill-c
```

## Directly usable observations

Table 1, page 6, identifies undyed, uncoated cotton sheeting, Oxford basket weave
and 2/1 twill. Source coordinates are **x = weft, y = warp**. Conditioning is
20 +/- 2 degrees C and 65 +/- 4 percent RH under JIS L0105. Thickness is the
average of 25 KES-FB3-A readings at 0.5 gf/cm2, not an unloaded contact thickness.
Thread counts are retained as published observations with their conversion from
counts per 5 cm.

Table 3, page 9, reports low-angle KES-FB1-A shear slopes. Appendix C.3,
pages 21-22, defines the first right-loading branch, a 0.5-2.5 degree slope
interval, and the mean of ten tests. Equation (43) converts force per width to
stress by dividing by the measured specimen thickness. Its inverse is

```text
membrane_shear [N/m] = G_low [Pa] * thickness [m]
Pa * m = (N/m2) * m = N/m
```

| Specimen | Thickness (mm) | G_low (Pa) | Canonical shear stiffness (N/m) |
| --- | ---: | ---: | ---: |
| A, sheeting | 0.660 | 98,070 | 64.7262 |
| B, Oxford | 0.822 | 262,100 | 215.4462 |
| C, twill | 0.769 | 94,470 | 72.64743 |

These slopes describe the stated loading interval. They do not supply a
finite-strain response, unloading law, uncertainty distribution, or validity
outside the conditioning and test protocol.

## Original fitted laws remain qualified extensions

All Table 3 coefficients are retained under `fujii_*` properties. The nonlinear
fits and reciprocity-corrected Poisson ratios have `estimate` evidence. They
require explicit estimate consent even when requested individually. The
following formulas identify the original model; no current solver evaluates
these extension fields.

For nonnegative engineering tensile strain `e`, equation (17) is
`E(e) = d*ln((b+exp(a*e))/(b+1)) + c*e + d*(1-exp(-k*e))`, where
`k=a*sqrt(b)/(b+1)`. Here `E(e)` is a stress response in Pa; its derivative is a
modulus. The source's low and elastic summary moduli are also retained, with
their definitions. They are not substituted for the solver's membrane law.

For nonnegative shear angle `g` in radians, equation (18) is
`G(g) = a*ln(1+b*g) + c*g + d*(1-exp(-k*g))`, where
`k=sqrt(-a*b*b/d)`. Both stress laws use odd continuation for negative input.
Equation (19) fits apparent Poisson ratio as `b*e^c*exp(a*e)+d`. Equation (42)
then changes the two initial ratios to satisfy reciprocity using the **elastic**
moduli. Combining those ratios with the **low** moduli would change the model.
No such combination is made in the catalog.

## Deliberately unresolved source quantities

The Table 1 weight values have no stated unit in either the HTML or rendered
PDF. They remain verbatim metadata with the missing-unit issue identified.
No mass or areal density is calculated from them.

The two KES bending moduli per specimen are retained in their reported Pa units.
Appendix D.2, page 22, gives the protocol: five tests per direction, first
downward-loading branch, slope between 0.5 and 1.5 cm^-1. Its printed equation
(44) uses `bending_strain = z * curvature * 10^2`, while the adjacent text labels
`z` in mm and curvature in cm^-1. Those units require a factor of `10^-1`.
The stress conversion is consistent with mm, so silently choosing another unit
for `z` would not resolve both equations. **Canonical bending rigidity is
therefore absent.** Clarification or original moment/curvature measurements
are needed before a directional tensor can be calibrated. The source does not
measure twisting stiffness, and its assumed shear/twist relationship is not
copied as empirical bending data.

These records also lack contact-pair friction, full calibrated membrane and
bending laws, thermal transport, and mass. They can be inspected, compared,
frozen, and extended with sourced measurements; a complete mechanical simulation
continues to refuse them. No supplier-wide cotton, linen, leather or performance
knit equivalence follows from these three specimens.
