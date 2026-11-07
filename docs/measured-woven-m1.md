# M1: a measured construction, not a generic cotton-blend law

`zagreb/polym17152013/M1` is one twill specimen from Mahnić Naglić, Petrak and Tomljenović,
*Polymers* 17(15), 2013 (2025), DOI [10.3390/polym17152013](https://doi.org/10.3390/polym17152013).
The [open-access article](https://pmc.ncbi.nlm.nih.gov/articles/PMC12349057/) is CC BY 4.0.
The catalog transcribes numerical observations from Tables 1–2 and §2.1, retaining source units,
construction, conditioning, sample counts where reported, and attribution. No article images are copied.

The material is 65% cotton, 33% polyester and 2% elastane: cotton warp and polyester-wrapped elastane
weft. Table 1 reports 226.58 g/m², 0.52 mm thickness, and 40/20 warp/weft threads per cm.
Tests were conditioned at 20 ± 2 °C and 65 ± 4% RH. The thickness is a ten-measurement mean at
**1 kPa**, not unloaded loft or a machine-foot compression law. Unreported bounds remain absent.

KES elongations remain `kes_emt_warp/weft`, not breaking strains or moduli. The printed bending
values remain extension properties `kes_bending_*_reported` in `cNcm`: their dimensional conversion
must be reconciled with the paper's CAD conversion table before use in a solver. Table 3 force
units and KES load normalization also need clarification. AI-derived CAD parameters are not imported
as measured physics. The three characterization methods do not create three independent fabrics.

A primary-source recheck (2026-11-07 UTC) makes the Table 2 bending discrepancy explicit:

| KES column | Literal KES conversion | Literal CAD conversion in the same column |
|---|---|---|
| B-1 (warp) | 0.356 cN cm = 3.56 × 10⁻⁵ N m | 3560 g mm²/s² = 3.56 × 10⁻⁶ N m |
| B-2 (weft) | 0.093 cN cm = 9.3 × 10⁻⁶ N m | 930 g mm²/s² = 9.3 × 10⁻⁷ N m |

The printed pairs differ by a factor of ten under these literal unit conversions. Both CAD bending
columns are labelled `Bend Warp`; this comparison preserves column order without resolving the
second direction. Intended units and width normalization remain unresolved; neither column is a
qualified sheet coefficient. Table 2's 490.35 cN load also lacks an explicit width basis. Section
2.1.2 gives 270 × 30 mm Fabric Kit strips and a 120 mm grip separation, but Table 3 in the examined
PMC XML does not state the force unit for its five readings. Those dimensions and the method's
force thresholds do not resolve that omission. Raw measurements or an explicit source clarification
remain necessary.

The record immediately participates in canonical discovery, unit normalization, source-locked replay
and condition-qualified screening. Missing modulus, sheet bending law, pair friction, usable fabric width, thermal
transport and price remain missing. No mechanical/thermal readiness, manufacturing qualification,
independent garment validation or applicability to another similarly named retail blend is inferred.
