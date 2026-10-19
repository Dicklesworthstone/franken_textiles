# Measured fleece, spacer and quilted insulation under fixed loads

Six built-in selections expose three study specimen groups at two applied forces. Changing
the selected state changes the directly measured resistance used by `ConductionLaw`,
`ThermalStack` and the steady thermal workbench. The records also preserve material-group
areal mass and the observations used to calculate compressed thickness.

## Source and identities

[Rogale et al. (2023), *Measurement Methods of the Thermal Resistance of Materials Used in
Clothing*, Materials 16(10):3842](https://pmc.ncbi.nlm.nih.gov/articles/PMC10224179/),
[DOI 10.3390/ma16103842](https://doi.org/10.3390/ma16103842), Tables 1 and 3, section 3,
Figure 7 and Equation (8). Reviewed 2026-10-18. The article is CC BY 4.0, copyright 2023
the authors. Only attributed numerical facts and the explicitly derived thicknesses are
retained; no article or images are bundled. The source note includes the reviewed HTML hash.

| Study group | Material description | Table 1 mass (g/m²) | Composition scope |
|---|---|---:|---|
| M1 | Fleece | 298.8 | 100% polyester |
| M2 | Spacer material | 315.9 | 100% polyester |
| M4 | Double-faced diamond-quilted lining | 278.3 | Polyester covers and padding; polypropylene lining |

M4 is an effective quilted assembly. Its component compositions and masses remain in the
record's structure; whole-assembly fiber fractions are not explicitly published and are left
unfilled. All three use `sheet` because the source text does not establish the required
weave/knit/bonding classification. Supplier, commercial product, lot and individual coupon
identifiers are unreported. These records cannot qualify similarly named retail products.

## Preserved states

IDs begin with `zagreb/rogale-2023/`, followed by the suffix below.

| ID suffix | Force (N) | Thickness (mm) | Evidence for thickness | Measured Rct (m² K/W) |
|---|---:|---:|---|---:|
| `m1/force-0.95n` | 0.95 | 2.92 | Measured initial reference | 0.0172 |
| `m1/force-9.5n` | 9.5 | 2.25 | Estimate: 2.92 − 0.67 | 0.0164 |
| `m2/force-0.95n` | 0.95 | 3.28 | Measured initial reference | 0.1045 |
| `m2/force-9.5n` | 9.5 | 2.59 | Estimate: 3.28 − 0.69 | 0.0853 |
| `m4/force-0.95n` | 0.95 | 5.36 | Measured initial reference | 0.2229 |
| `m4/force-9.5n` | 9.5 | 2.25 | Estimate: 5.36 − 3.11 | 0.1095 |

Table 3 measures conductive contact resistance with a multi-purpose differential
conductometer. Section 3 cites EN ISO 5084:1996 for the initial 0.95 N reference. Figure 7
and Equation (8) identify `x1` as displacement from that reference and `x2` as initial
thickness. The compressed records retain both original quantities in millimetres, with their
separate measurement-force conditions; the subtraction is classified `estimate` and has no
invented uncertainty. Initial thickness is already under preload and is not unloaded loft.
Table 1 mass is retained as a published group property, without claiming repeated weighing
under each force. Resistance at 9.5 N is directly measured, not reconstructed from thickness.

**Loaded contact area is not reported. Force in newtons cannot supply pressure in pascals.**
No pressure law, intermediate-load interpolation, hysteresis or recovery curve is provided.
Every force-conditioned quantity and each state retain this missing-area limitation. These
observations must not be supplied as pressure knots to `pressure-condition`. Table 3 also
does not specify numerical plate temperatures, conditioning, loading rate, dwell time,
replicate count or uncertainty. The scalar resistance has the existing constant-property
consumer semantics; it does not establish a measured temperature validity interval.

## Use and verification

```sh
ftx-materials show zagreb/rogale-2023/m4/force-0.95n
ftx-materials show zagreb/rogale-2023/m4/force-9.5n
```

Use either ID as a layer's ordinary material selection in a steady thermal request.
Measured resistance is usable with `allow_estimates: false` because the conduction path
does not require the derived thickness. Reading or consuming compressed thickness requires
explicit estimate consent. Frozen selections retain the force, original observations,
derivation, citation and selection hash for offline replay.

The 9.5 N states carry `compression_model: "fixed_force_observation/1"`. The common sewing
gate therefore refuses their loaded thickness as uncompressed stack clearance, including
when estimate consent is enabled. This marker adds no pressure value or pressure law and
does not change measured-resistance use. The 0.95 N states remain initial protocol
measurements, with their existing qualification that the reference thickness is not unloaded loft.

`tests/loaded_insulation_catalog.rs` checks all six source states, original units,
normalization, estimate admission, composition scope, the loaded-state marker and force-context
hash locks. It also substitutes both states of each group through the existing conduction and thermal-stack
consumers at fixed numerical boundaries, checking the result against the independently
transcribed Rct values. Those test boundaries are not the paper's unreported apparatus area
or temperatures. No specific heat, membrane, bending or contact-friction data are invented;
transient storage and drape remain unqualified. The source and tests establish neither
whole-garment thermal behavior nor wearer comfort.
