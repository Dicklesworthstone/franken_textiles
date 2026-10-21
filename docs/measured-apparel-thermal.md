# Measured thermal comparisons for ordinary woven and knitted apparel

The shared material catalog includes eight fabric groups from De Mey et al. (2025),
*Short-Time Transient Thermal Behaviour in Textile Fabrics—The Dual Phase Approach*,
Textiles 5(4), 66, DOI [10.3390/textiles5040066](https://doi.org/10.3390/textiles5040066).
These add real linen blends and knitted apparel to the existing steady heat-transport
comparison, alongside their measured mass and gauge thickness. Selecting another record
changes the same canonical material inputs used by the workbench and thermal consumers.

## Included measurements

All IDs begin with `demey/2025/`. Values below are the means printed in Table 1, independently
checked against the primary PDF's page 5. Resistance uses a Celsius temperature difference;
`m2*degrees C/W` and the catalog's `m2*K/W` have identical scales.

| ID suffix | Published composition and construction | Mass, g/m² | Thickness, mm | Resistance, m² K/W |
| --- | --- | ---: | ---: | ---: |
| `angora-jersey` | 100% angora wool, single jersey | 305 | 1.65 | 0.0413 |
| `outlast-pet-jersey` | 100% PET, single jersey, descriptor “Outlast” | 180 | 1.09 | 0.0259 |
| `cotton-lycra-jersey` | 97% cotton / 3% Lycra, single jersey | 255 | 2.50 | 0.0350 |
| `cotton-jersey` | 100% cotton, single jersey | 225 | 1.50 | 0.0368 |
| `linen65-cotton35-woven` | 65% linen / 35% cotton, complex weave | 354 | 0.90 | 0.0164 |
| `wool70-linen20-cotton10-woven` | 70% wool / 20% linen / 10% cotton, plain weave | 559 | 1.46 | 0.0289 |
| `linen60-cotton40-woven` | 60% linen / 40% cotton, complex weave | 400 | 0.75 | 0.0145 |
| `cotton70-linen30-chenille` | 70% cotton / 30% linen, chenille, twill 1/2 Z | 466 | 1.25 | 0.0235 |

The original units, specimen figure, method, acquisition conditions and primary source remain
in each immutable selection. Composition is the paper's reported fiber composition; it is
not a separate composition assay. The catalog uses `elastane` for the component printed as
Lycra. Fabric counts are preserved as source descriptors, including the repeated cotton
label in Figure 1h's weft-count cell; the adapter does not guess a correction or a weave repeat.

## Measurement and model scope

Sections 3.1–3.4 specify preconditioning at 20 ± 2 °C and 65 ± 5% RH. Mass was measured
on 100 cm² coupons, using a balance specified as ±0.001 g, with five independent readings
averaged after at least 24 hours' conditioning. Thickness is the mean of five points measured
with a 10 mm presser foot at **0.5 kPa**. That is a gauge-pressure thickness, not unloaded loft
or a pressure/compression law. Instrument specifications are not inferred uncertainty bounds.

The guarded-hotplate method is ISO 8302:1991, with warm and cold plates at **35 ± 0.1 °C
and 25 ± 0.1 °C**, respectively. Three measurements were averaged. The article does not supply
hotplate pressure, ply count, separately quantified contact resistance or an interface correction.
Accordingly, the adapter preserves the reported effective plate-to-plate resistance directly.
It does **not** derive conductivity from a thickness measured under a different protocol.

`ConductionLaw` and `ThermalStack` can use this reported resistance without estimate consent.
Their existing scalar semantics treat it as constant; the data does not establish a measured
temperature-dependent range. A comparison at the reported 35/25 °C plate conditions is a
protocol-specific calculation. A field-temperature extrapolation or a series combination with
different contacts requires additional physical justification. Exact method/condition filters
in the existing material screen can restrict which observations are admitted.

Specific heat and latent heat remain absent. The paper's “Outlast” descriptor does not establish
a current commercial product, PCM composition, transition interval, enthalpy or a universal PET
law. No phase-change model is activated, and the transient thermal consumer correctly refuses
all eight records even with estimate consent. The fingertip measurements and dual-phase model
discussed elsewhere in the paper are not imported. These data do not establish wearer comfort,
safe outdoor temperatures, garment fit or machine suitability.

## Use the shared catalog

```sh
cargo run -p ftx-cli --bin ftx-materials -- catalog demey/2025/
cargo run -p ftx-cli --bin ftx-materials -- show demey/2025/linen65-cotton35-woven
cargo run -p ftx-cli --bin ftx -- materials freeze demey/2025/cotton-jersey --json
```

The returned frozen selection can be used by the existing binding, screening and layer
comparison APIs. Complete mechanical simulation still needs matched directional stretching,
shear, bending and interface measurements; no fiber label fills those missing properties.

The focused tests independently transcribe the source means in SI, preserve different gauge
and hotplate conditions, replay frozen selections without a live catalog, detect changed
source conditions, and exercise actual steady flux and material-screen selection. Their
fixed-area arithmetic checks are software acceptance, not held-out measured heat-flux validation.

## Source and retention

Primary [version-of-record PDF](https://www.mdpi.com/2673-7248/5/4/66/pdf), published
2025-12-08, accessed 2026-10-19. The article is CC BY 4.0. Only attributed material facts are
transcribed; source pages, images and participant observations are not redistributed.
The inspected PDF SHA-256 is
`67733339c08f2bca5cf851674158785d3edcdd5d05ad0e900a0fc9f5250a1d02`.

The canonical implementation is `crates/ftx-pattern/src/material_catalog/thermal_apparel.rs`.
Matched-specimen characterization and physical garment validation remain open under `ftx-tm2`,
`ftx-tma` and `ftx-tm8`.
