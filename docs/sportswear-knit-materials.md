# Measured yarns and knit constructions

The built-in catalog includes 30 records from Salopek Čubrić, Potočić Matković,
Pavlović and Pavko Čuden (2022), [Material and Structural Functionalization of
Knitted Fabrics for Sportswear](https://doi.org/10.3390/ma15093306), Materials
15(9):3306. Five original yarns have separate records from their 25 knitted
constructions. These are the study's specimen groups, not present-day supplier
products or generic constants for every fabric made from the same fiber.

## Source observations

The ID prefix is `zagreb/sportswear-2022/`. Yarn records end in `y1-yarn`
through `y5-yarn`; fabric records combine yarn and construction, such as `y2-st`.

| Yarn | Source composition | Measured linear density, tex | Threads used for knitting |
|---|---|---:|---:|
| Y1 | Cotton | 29.1 | 2 |
| Y2 | Wool | 68.6 | 1 |
| Y3 | PAN bouclé | 72.3 | 1 |
| Y4 | 50% wool / 50% PAN | 71.4 | 1 |
| Y5 | PA filament | 15.4 | 2 |

Every yarn was used in single plain (`sp`), single miss (`sm`), single tuck/piqué
(`st`), double plain (`dp`) and double Milano rib (`dm`) structures, knitted on a
Shima Seiki SES 122 RT 12E machine. The original yarn's tex is not multiplied by
the parallel knitting-thread count. PAN is retained as polyacrylonitrile and PA
as polyamide; no unreported grade, filament count or producer is invented.

Tables 2–3 supply yarn linear density, twist, tenacity, breaking elongation,
mass-variation coefficient and four measured hairiness counts. PA's unreported
twist, mass CV and hairiness remain absent. Hair counts retain their specified
protruding lengths and testing speed; the source does not provide the total
evaluated yarn length, so they are not interpreted as counts per metre.

Tables 4–7 supply fabric mass per area, gauge thickness, wale/course densities,
dry thermal resistance, vapor resistance and airflow. The 25 constructions keep
their original yarn links and acquisition conditions. Existing catalog selection,
mass accounting, scalar screening, dry steady resistance and vapor-stack consumers
use the same observations without additional physics defaults.

## Run a construction comparison

```sh
cargo run -p ftx-cli --bin ftx-materials -- screen \
  tests/fixtures/materials/screen-sportswear.json
```

The example compares the source's wool single plain (`y2-sp`) and single
tuck/piqué (`y2-st`) constructions through the existing mass, dry-resistance and
vapor-resistance screening objectives. Single plain is lighter (238.19 versus
286.40 g/m²); single tuck has greater measured dry resistance (0.0834 versus
0.0584 m²·K/W) and lower measured vapor resistance (3.5071 versus 4.4758
m²·Pa/W). Both remain on the Pareto frontier, making that tradeoff visible.

The request explicitly admits nominal comparison of different specimen records
while requiring the source's stated environmental conditions for each test.
It does not infer cross-test covariance, an uncertainty-aware ranking, warmth
rating or a simulated garment. The same JSON works in the browser workbench's
screening control; admitted results carry ordinary frozen material selections.

## Qualifications that affect use

- **Gauge thickness:** ISO 5084 at the source's stated `20 g/cm2` loading, with
  0.01 mm instrument accuracy. The source explicitly distinguishes this from
  unloaded thickness. It is stored as `reported_thickness`, so it cannot silently
  supply sewing clearance or cloth contact thickness. Instrument accuracy is not
  converted to a specimen uncertainty interval.
- **Dry resistance:** ISO 11092 with air at 20°C, 65% RH and 1 m/s. The hotplate
  pressure and numerical plate temperature are not explicitly stated. No
  conductivity is inferred from the separate gauge thickness.
- **Vapor resistance:** ISO 11092 with air at 35°C, 40% RH and 1 m/s. No humidity
  or temperature response interval is inferred. Potential vapor transport through
  declared boundary resistances is separate from actual sweating or comfort.
- **Airflow:** Table 6 reports **L/min**, a volume flow. The pressure drop is
  50 Pa, but the test area is 20 cm² for Y1–Y4 and 5 cm² for Y5. The catalog
  normalizes `air_volume_flow` to m³/s and keeps the test area attached; it does
  not mislabel these readings as `air_permeability` in m/s. Screening can require
  an exact test-area condition before comparing records.
- **Tensile data:** yarn tenacity normalizes from cN/tex to N·m/kg and breaking
  elongation from percent to a ratio. Neither is a sheet modulus, recoverable
  extension limit or proof that a garment can be draped accurately.

Numerical conditioning temperature/humidity, replicate counts, per-reading
standard deviations and confidence intervals are not reported for the full
matrix. These remain unknown. Fabric relaxation is explicitly 48 hours under the
source's description of standard conditions. Heat capacity, mechanics, contact,
sewing, optics and moisture-storage laws are absent.

Table discrepancies are retained: Y4 double plain has vapor resistance 6.0944
m²·Pa/W in Table 7, exceeding the prose's stated maximum of 5.7542; the table
value is used with that conflict recorded. Table 4 repeats 132.51 g/m² for both
Y5 single miss and single tuck; neither is silently corrected. The printed
`267,35` mass for Y1 single tuck is interpreted as a decimal comma.

## Retention and checks

The article is CC BY 4.0, copyright 2022 the authors. Only attributed numeric
facts and necessary qualifications are bundled; no article pages, images or
human data are redistributed. The reviewed [Europe PMC full-text XML](https://www.ebi.ac.uk/europepmc/webservices/rest/PMC9104909/fullTextXML)
has SHA-256 `d26ec767f236288486cc9145b6af5c62f7e23ee59b2032278c608193a11928df`.

`crates/ftx-pattern/tests/sportswear_catalog.rs` checks source values, conversions,
construction/yarn identity, dimensional refusals, capability limits, source-filtered
screening and actual dry/vapor consumer calculations with frozen offline replay.
The scenario boundary values in those tests are synthetic; they do not establish
experimental agreement for a garment, outdoor temperature ratings or machine
process qualification. Tracked as `ftx-kn1`; broader physical acceptance stays open.
