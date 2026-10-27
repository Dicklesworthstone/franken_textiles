# Primary-source insulation records

The shared catalog now loads a second factual data tranche, `material_catalog/insulation.json`.
It contains sixteen exact designations from five 3M data sheets, rather than a generic synthetic-fill
property average. IDs end in `_2020` to identify the document's copyright year, not to claim current
lot specifications. The manufacturer describes the table values as typical averages across lots at
publication. They are not lot-specific guarantees, uncertainty bounds or new measurements by FTX.

| Catalog ID | Manufacturer mass (g/m²) | Manufacturer thickness (cm) | Manufacturer thermal resistance (m² K/W) |
|---|---:|---:|---:|
| `3m_thinsulate_c100_2020` | 105 | 0.70 | 0.20 |
| `3m_thinsulate_c200_2020` | 210 | 1.40 | 0.39 |
| `3m_thinsulate_ec80_2020` | 80 | 1.10 | 0.25 |
| `3m_thinsulate_ec150_2020` | 150 | 1.80 | 0.39 |
| `3m_thinsulate_fx70_2020` | 74 | 0.50 | 0.12 |
| `3m_thinsulate_fx100_2020` | 105 | 0.55 | 0.14 |
| `3m_thinsulate_g60_2020` | 60 | 0.80 | 0.22 |
| `3m_thinsulate_g80_2020` | 80 | 1.10 | 0.29 |
| `3m_thinsulate_g100_2020` | 100 | 1.30 | 0.34 |
| `3m_thinsulate_g120_2020` | 120 | 1.50 | 0.38 |
| `3m_thinsulate_g150_2020` | 150 | 1.70 | 0.43 |
| `3m_thinsulate_g200_2020` | 200 | 2.00 | 0.51 |
| `3m_thinsulate_b100_2020` | 105 | 0.20 | 0.061 |
| `3m_thinsulate_b200_2020` | 210 | 0.40 | 0.14 |
| `3m_thinsulate_b400_2020` | 420 | 0.80 | 0.28 |
| `3m_thinsulate_b600_2020` | 620 | 1.20 | 0.42 |

Sources, visually checked on 2026-10-14:

- Type C, document 70-0717-1276-7, table page 2; composition page 1:
  https://multimedia.3m.com/mws/media/2228171O/3m-thinsulate-insulation-original-type-c.pdf
- Type EC, document 70-0717-1287-4, table page 2:
  https://multimedia.3m.com/mws/media/2228182O/3m-thinsulate-insulation-advanced-eco-type-ec.pdf
- Type FX, document 70-0717-1291-6, table page 2; composition page 1:
  https://multimedia.3m.com/mws/media/2228178O/3m-thinsulate-insulation-stretch-type-fx.pdf

Additional primary tables visually checked on 2026-10-26:

- [Type G](https://multimedia.3m.com/mws/media/2228180O/3m-thinsulate-insulation-advanced-type-g.pdf),
  document 70-0717-1282-5, composition/construction page 1, properties page 2;
  SHA-256 `cced7450219c5b0863b7780bc2cb2b3033294b022de8886f7c2f696ef40b11dc`.
- [Type B](https://multimedia.3m.com/mws/media/2228175O/3m-thinsulate-insulation-water-resistant-for-footwear.pdf),
  document 70-0717-1283-3, manufactured state page 1, properties and nominal width page 2;
  SHA-256 `916eb6c048516a608efdca6404fa4d2305a7edc2ee6b21029387072a84d9bc8e`.

Type G is thermally bonded polyester insulation. Type B is a distinct pre-compressed footwear
product, not an unloaded apparel batt or a pressure-response curve. Its 2020 source does not state
fiber fractions, so composition remains unknown. All ten added grades report 1.52 m nominal width;
their source hashes and original SI units survive freezing and substitution. Water-resistance
marketing is not a measured wet-state transport law.

Thickness is qualified by the listed ASTM D5736 test at 0.002 psi. That pressure is **not** assigned to
the thermal observation: its method is listed separately as ASTM F1868. The direct SI thermal column
is retained; converting separately rounded clo figures could produce different values. Authored cm
survive in the frozen record and normalize explicitly to metres. C and FX roll widths use the published
1.52 m SI column; no EC width is invented. C and FX composition follows the stated olefin/polyester
fractions; the EC recycled-content claim is not used as a complete polymer composition.

## Use through the existing consumer

```sh
cargo run -p ftx-cli --bin ftx-materials -- show 3m_thinsulate_c100_2020
cargo run -p ftx-cli --bin ftx-materials -- thermal tests/fixtures/materials/thinsulate-static.json
```

The static example declares its area and temperatures/film resistance; those are scenario inputs, not
manufacturer claims. Replacing C100 with C200 changes the computed dry series heat flow without a code
change. The same existing consumer compares G100 with B100: at an explicitly chosen 1.2 m² area,
30 K temperature difference and 0.10 m² K/W boundary-film resistance, it computes
`36 / (0.34 + 0.10)` versus `36 / (0.061 + 0.10)` W. These are conditional steady-model results,
not measured garments. The regression retains the original observations and checks that unreported
conductivity, heat capacity and uncertainty bounds remain absent.

These records intentionally lack directional mechanics and specific heat: drape and transient
heat storage must refuse those missing inputs. Neither a published insulation value nor a stretch
marketing claim establishes garment comfort, mechanical suitability, safe processing or a cold-weather
rating. Never infer time constants from resistance alone, or add overlapping test/air-film resistances
without checking the test definition and intended stack boundary conditions.
