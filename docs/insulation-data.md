# Primary-source insulation records

The shared catalog now loads a second factual data tranche, `material_catalog/insulation.json`.
It contains six exact designations from three 3M data sheets, rather than a generic synthetic-fill
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

Sources, visually checked on 2026-10-14:

- Type C, document 70-0717-1276-7, table page 2; composition page 1:
  https://multimedia.3m.com/mws/media/2228171O/3m-thinsulate-insulation-original-type-c.pdf
- Type EC, document 70-0717-1287-4, table page 2:
  https://multimedia.3m.com/mws/media/2228182O/3m-thinsulate-insulation-advanced-eco-type-ec.pdf
- Type FX, document 70-0717-1291-6, table page 2; composition page 1:
  https://multimedia.3m.com/mws/media/2228178O/3m-thinsulate-insulation-stretch-type-fx.pdf

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
change. These records intentionally lack directional mechanics and specific heat: drape and transient
heat storage must refuse those missing inputs. Neither a published insulation value nor a stretch
marketing claim establishes garment comfort, mechanical suitability, safe processing or a cold-weather
rating. Never infer time constants from resistance alone, or add overlapping test/air-film resistances
without checking the test definition and intended stack boundary conditions.
