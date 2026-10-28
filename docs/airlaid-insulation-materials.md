# Source-qualified air-laid insulation materials

The built-in catalog contains nine specimen groups from Lupescu, Robert, Sanchez-Diaz and Elkoun,
“Development of Thermally Insulating Nonwovens from Milkweed Fibers Using an Air-Laid Spike Process,”
*Textiles* **5**(1), 5 (2025), [DOI: 10.3390/textiles5010005](https://doi.org/10.3390/textiles5010005).
The article was published on 22 January 2025 under CC BY 4.0. Its version-of-record
[PDF mirror](https://pdfs.semanticscholar.org/d257/e9122f20f0ab3afbb374b161347b4dec5206.pdf)
has SHA-256 `21d5a283bb6ccc65b737d38a9e1f03ad8efcc8c471270bb45e1143a9893a3644`.
The transcription uses Tables 2–6 and the corresponding experimental methods, with table facsimiles
checked on 27 October 2026. No article pages or images are redistributed.

These records add measured nonwoven insulation to the ordinary selection, pattern binding, mass,
steady conduction and offline snapshot consumers. Each has twelve scalar observations or
source-derived quantities, for 108 stored quantities in total. This count does not describe 108
independent measurements. The records preserve the source units and test conditions before canonical
normalization; they do not introduce a separate material model.

## Available specimens

All identifiers below have the prefix `lupescu/2025/`. Numbers in this table are directly tabulated
source values, including the source's independent rounding of resistance.

| Identifier suffix | Source specimen | Mass (g/m²) | Gauge thickness (mm) | Baseline conductivity (mW/(m·K)) | Resistance (m²·K/W) |
| --- | --- | ---: | ---: | ---: | ---: |
| `sample-1` | Milkweed/LM 2.2/PLA feed 30/10/60 | 55.34 | 5.05 | 35.32 | 0.143 |
| `sample-2` | Milkweed/LM 2.2/PLA feed 70/10/20 | 60.75 | 4.80 | 33.13 | 0.145 |
| `sample-3` | Milkweed/LM 2.2/PLA feed 50/20/30 | 60.51 | 5.12 | 33.96 | 0.151 |
| `sample-4` | Milkweed/LM 2.2/PLA feed 50/10/40 | 161.45 | 8.40 | 32.81 | 0.256 |
| `sample-5` | Milkweed/LM 2.2/PLA feed 50/20/30 | 113.20 | 6.65 | 33.44 | 0.199 |
| `sample-6` | Milkweed/LM 2.2/PLA feed 50/40/10 | 170.15 | 10.45 | 33.65 | 0.311 |
| `sample-7` | Milkweed/LM 2.2/PLA feed 50/20/30 | 56.90 | 4.55 | 32.55 | 0.140 |
| `thinsulate` | Study's commercial PET comparator | 166.84 | 5.02 | 33.31 | 0.151 |
| `primaloft` | Study's commercial PET comparator | 86.86 | 5.08 | 34.14 | 0.149 |

Feed ratios are mass percentages of the input fibers. LM 2.2 is the study's PET-core/polyolefin-sheath
bicomponent fiber. The paper reports preferential milkweed loss during processing, so input recipes
remain process metadata and the seven experimental records have unknown final composition. The
two commercial comparators are reported as 100% PET, but their grade, supplier SKU and lot are absent.
Their brand names must not be used to join observations from other catalog entries or current products.

## Evidence retained and consumer scope

- **Mass and gauge geometry:** `areal_density`, `reported_thickness`, `bulk_density`. The study used
  ISO 9073-1 for mass and ISO 9073-2 at 0.1 kPa for thickness. Gauge thickness is deliberately separate
  from the mechanical `thickness` property. The source's bulk density is calculated from mass and
  gauge thickness, rather than a constituent-fiber density measurement.
- **Baseline heat transport:** `thermal_conductivity`, `thermal_resistance`,
  `reported_specific_thermal_resistance`, `reported_thermal_insulation`. ASTM C518 heat-flow-meter
  observations used plates at 37 °C and 21 °C. Conductivity converts from mW/(m·K) to W/(m·K), preserving
  the authored observation. The source calculates resistance from gauge thickness and conductivity;
  this is marked `published`, and its tabulated rounding is retained. Direct resistance supports
  existing steady conduction without promoting gauge thickness to a mechanical/contact law. Clo and
  resistance-per-bulk-density values remain source-specific attributes.
- **Compression observations:** `compression_ratio`, `short_term_compression_recovery`. ASTM
  D6571-22 used 7.26 kg with the stated loading and recovery durations. The recovery denominator is
  the recovered height after the first loading cycle (`J/E`), not initial height. Loaded area is not
  numerically supplied. These endpoints cannot define pressure, a compression curve or history law.
- **Humidity observations:** `moisture_regain`, `porosity`, `thermal_conductivity_rh90`. Moisture
  conditioning was at 21 °C and 90% RH, until successive mass readings differed by less than 0.01%.
  Regain uses dry specimen mass as its denominator; it is not wet-mass fraction or available liquid.
  Porosity is source-calculated. Humid conductivity remains a separately qualified observation in
  its original mW/(m·K) unit, searchable with an exact RH condition. The paper does not supply matched
  humid mass per area, thickness, resistance or heat capacity, so it does not activate a wet law.

The baseline humidity, preconditioning duration, moisture content and heat-flow-meter contact
pressure are not numerically reported. The 100 Pa thickness-gauge setting does not establish thermal
contact pressure. Scalar conduction retains the existing constant-property semantics; the single
plate protocol does not establish an empirical temperature range or cold-weather comfort validity.

Where tables explicitly label standard deviation, it is preserved under
`conditions.reported_standard_deviation` in source units. Table 5 and the moisture-regain/porosity
columns of Table 6 print ± values without naming their statistic; those use
`conditions.reported_plus_minus`. Neither becomes a hard bound, uncertainty of the mean or propagated
prediction interval. Consequently, screening with `basis: "reported_bounds"` cannot use these nominal
observations as bounded evidence.

## Use through existing tools

```sh
ftx-materials catalog lupescu/2025/
ftx-materials show lupescu/2025/sample-3
```

The `show` result includes the source record, normalized snapshot and frozen selection. Select an
exact ID through the ordinary material binding workflow to replace insulation on a pattern without
editing its geometry. A frozen selection embeds its observations and content lock for offline replay.

For example, `sample-3` and `thinsulate` have the same tabulated resistance, 0.151 m²·K/W, but source
masses of 60.51 and 166.84 g/m². The same-area steady comparison therefore retains heat flow while
changing accounted mass. `sample-6` has resistance 0.311 m²·K/W, which changes the same-boundary steady
calculation automatically. These calculations describe the selected source observations and the
specified area/boundaries; they do not predict garment fit, warmth, survival or manufacturing success.

All nine records lack the complete membrane, bending, friction and heat-capacity observations needed
for mechanical draping and transient heat storage. Those capabilities continue to refuse incomplete
inputs. The regression tests exercise unit normalization, scoped screening, unchanged pattern geometry,
ordinary mass/steady consumers, and source-locked offline replay. They do not satisfy the empirical
linen/leather/performance-knit material-swap validation gate.
