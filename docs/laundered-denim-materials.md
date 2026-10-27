# Measured five-cycle laundering of cotton/elastane denim

The built-in catalog contains ten measured material states from Kanat and Ozdil (2026):
five fabric codes, each before washing and after the study's complete five-cycle treatment.
The source adapter builds a canonical `ftx-material-treatment/1` request using embedded,
content-locked selections. The same material resolver supplies areal mass and measured steady
thermal resistance; the dimensional map uses the source's directional shrinkage.

## Source and exact treatment

Primary study: Zehra Evrim Kanat and Nilgun Ozdil, *Influence of Washing on the Thermal Comfort
Parameters of Denim Fabrics with Varying Elastane Content*, Materials 19(15), 3264 (2026),
[DOI 10.3390/ma19153264](https://doi.org/10.3390/ma19153264).
Sections 2.1-2.2 and Tables 1-5 were checked against the publisher's PDF, pages 3-6.
The [publisher PDF](https://mdpi-res.com/d_attachment/materials/materials-19-03264/article_deploy/materials-19-03264.pdf)
has SHA-256 `0d9fc029c60b79745b2c123a4d4d9bc34fa69734b511cf20d1bccc94fef01e2c`.
Only selected numerical observations and original attribution are retained in the repository.

The treatment is **five ISO 6330 washing cycles**, using a Type A washing machine, 100 g of
standard 3-ECE reference detergent, a 2 kg fabric load, 30 degrees C and 40 minutes per cycle.
Samples were flat dried before testing. Testing took place at 20 +/- 2 degrees C and
65 +/- 4 percent RH; conditioning duration is not reported. These are total before/after changes,
not per-cycle rates. The dataset does not support repeating, dividing or interpolating this treatment.

All samples use 2/1 Z twill, initially woven at 27 warp yarns/cm and 20 weft yarns/cm. Warp is cotton,
Ne 8, twist coefficient alpha_e 4.5. Weft is Ne 14, twist coefficient alpha_e 4.2; core-spun yarns
have a cotton sheath and 78 dtex elastane filament core at draft ratio 5.2. Plain weft yarns are
interleaved as shown below. The source reports an elastane percentage without an explicit mass
denominator or composition test, so the catalog preserves that label without creating a complete
dry-mass composition. It also does not claim composition remains invariant after washing.

| Fabric code | Core-spun/plain weft repeat | Reported elastane ratio | Warp contraction | Weft contraction |
|---|---|---:|---:|---:|
| 1 | All core-spun | 1.50% | 4.83% | 5.50% |
| 2 | 3 core-spun + 1 plain | 1.05% | 3.60% | 5.30% |
| 3 | 2 core-spun + 1 plain | 0.95% | 3.50% | 5.30% |
| 4 | 1 core-spun + 1 plain | 0.70% | 1.83% | 4.60% |
| 5 | 1 core-spun + 2 plain | 0.50% | 1.60% | 3.00% |

Table 4 prints **weft before warp**. The canonical request explicitly maps those columns to
`shrinkage_warp` and `shrinkage_weft`, with positive values denoting contraction.

## Paired material observations

Each catalog ID is `kanat-ozdil-2026/denim-N/before-washing` or
`kanat-ozdil-2026/denim-N/after-5-washes`, for N from 1 through 5.

| Code | Areal mass before / after (g/m2) | Alambeta thickness before / after (mm) | Thermal resistance before / after (m2 K/W) |
|---|---:|---:|---:|
| 1 | 339.93 / 370.10 | 0.84 / 1.23 | 0.0152 / 0.0245 |
| 2 | 339.83 / 364.80 | 0.83 / 1.20 | 0.0148 / 0.0220 |
| 3 | 339.70 / 361.30 | 0.81 / 1.12 | 0.0139 / 0.0214 |
| 4 | 339.35 / 355.60 | 0.77 / 1.08 | 0.0128 / 0.0195 |
| 5 | 339.08 / 344.70 | 0.75 / 1.01 | 0.0124 / 0.0196 |

Table 3 also supplies fractional cover. Table 5 supplies measured air permeability and relative
water vapor permeability, with the sample standard deviations of these two observations and thermal
resistance. All original values and units are retained. Standard deviations are separate observations,
never silently converted to property bounds or confidence intervals.

Alambeta gauge pressure and plate temperatures are not specified. Thickness therefore remains
`reported_thickness`; it cannot supply unloaded contact or sewing clearance. Measured thermal
resistance is available to the existing constant-property steady consumer. No conductivity or heat
capacity is reconstructed from thickness. Air-permeability pressure difference and test area are
also unspecified, and relative water vapor permeability (%) is not evaporative resistance (m2 Pa/W).
These two transport observations remain reported attributes, without invented flow or vapor laws.

The paper identifies fabric-code groups, not retail products, lots or individual paired coupons.
Initial thread counts are available only in the before state. The after state does not silently
inherit them. The observed post-wash areal mass remains authoritative: area contraction does not
authorize an assumption of conserved mass or a quantitative fiber-shedding estimate. Mechanical,
friction, cutting-force, compression, transient thermal and sewing qualifications remain absent.

## Generate a portable request

```sh
cargo run -p ftx-pattern --example laundered_denim_treatment -- 1 > /tmp/denim-1-treatment.json
cargo run -p ftx-pattern --example laundered_denim_treatment -- 5 > /tmp/denim-5-treatment.json
```

The example calls `material_catalog::laundered_denim::treatment_request`, resolves the normal catalog,
freezes both source records, and validates the transition through `material_treatment::evaluate`.
It does not mint independent physics values or manually specify material hashes. The resulting JSON
is a reusable request for the shared treatment and pattern-transformation APIs. Fabric codes outside
1 through 5 refuse; relabeling five cycles as one or changing a frozen material value also refuses.

Generated requests for [fabric 1](../tests/fixtures/materials/kanat-ozdil-2026-denim-1-treatment.json)
and [fabric 5](../tests/fixtures/materials/kanat-ozdil-2026-denim-5-treatment.json) are included.
They carry the complete original records, source, process and conditioning together with the
resolver's actual content locks; they can be replayed without retrieving a mutable catalog entry.

`tests/laundered_denim_catalog.rs` checks the source columns, units, uncertainty semantics,
before/after identity, missing-property handling, request replay and real steady thermal consumption.
Its fixed-area heat-flow checks are numerical regressions, not measured wearer comfort or garment fit.
