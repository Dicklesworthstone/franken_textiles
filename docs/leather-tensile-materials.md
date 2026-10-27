# Measured bovine tensile specimens in the shared catalog

The built-in catalog includes ten individual bovine-leather tensile trials from Elizabeth
Schneider's **MBL Sustainability and Characterization Testing Data**, version 1, Virginia Tech,
published 24 August 2026, [DOI 10.17632/2pvf4x9crc.1](https://data.mendeley.com/datasets/2pvf4x9crc/1).
The release is [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). This integration transcribes
40 numerical observations and records the conversions and unresolved source inconsistencies below.
The author does not endorse this application.

IDs `schneider/2026/bovine-d1-1` through `bovine-d1-5` and `bovine-d2-1` through `bovine-d2-5`
retain the five trials in each source direction. D1 and D2 are perpendicular source labels;
their relationship to hide backbone, sampling region, warp or weft is unreported. Records are
individual test specimens, not ten separate commercial products or an established population sample.
Supplier, lot, tanning, finish, temperature, humidity and conditioning duration remain unknown.

## Data that existing consumers can use

The values come from **Tensile Test Data.xlsx**, sheets `Bovine D1` and `Bovine D2`,
`Results Table 1`, rows 4–8. Original units and exact cell addresses remain in each frozen record.

| Property | Workbook column / original unit | Canonical use |
|---|---|---|
| `reported_thickness` | C / mm | Inspectable pre-test caliper thickness in m |
| `tensile_maximum_force` | G / kgf | Maximum total specimen force in N |
| `tensile_force_at_break` | D / kgf | Instrument-reported break force in N, independently of the maximum |
| `tensile_displacement_at_break` | E / mm | Instrument displacement at its break criterion in m |

Force normalization uses exactly **1 kgf = 9.80665 N**. The source's rounded instrument results
are retained; peaks are not recomputed from raw time-series nodes. The caliper's stated ±0.01 mm
precision is instrument metadata, not a specimen uncertainty interval. Missing bounds stay absent.
Maximum and at-break force are distinct observations; neither is a material stiffness or a
garment's allowable load. Instrument travel is not a local strain measurement.

The ordinary catalog inspection, comparison, freezing and nominal screening paths use these values.
For example, save this ordinary `ftx-material-screen/1` request and run
`ftx materials screen REQUEST.json --json` (or `ftx-materials screen REQUEST.json`):

```json
{
  "schema": "ftx-material-screen/1",
  "basis": "nominal",
  "query": "schneider/2026/bovine-",
  "constraints": [{
    "attribute": {
      "key": "tensile_maximum_force",
      "unit": "N",
      "conditions": {"source_direction": "D2"}
    },
    "minimum": 200.0
  }]
}
```

This selects specimen `schneider/2026/bovine-d2-1`, whose reported maximum is 22.31 kgf
(218.7863615 N). The other nine trials fail either the direction restriction or that force
threshold. Returned frozen selections replay against an empty catalog; source or value changes
invalidate their original digest. This is source-qualified specimen screening, not a population
strength bound, supplier specification or mechanical material calibration.

## Why the source does not yet supply a simulation law

The landing-page protocol describes an Instron 5944, nominal ASTM D412-C dogbones, ISO 3376 tensile
testing and 100 mm/min crosshead speed. It gives 115 mm total length, 33 mm gauge, 6 mm waist and
25 mm ends. The workbook instead labels its gauge as **65 mm** and divides displacement by 65.
The workbook also labels calculated force/area columns as MPa while its force columns are kgf
and its formulas omit the kgf-to-N conversion. Instrument stress values and the separate 6 mm
area calculations are inconsistent. These discrepancies are preserved in every observation's
conditions; no preferred gauge, width-based stress, strain or Young's modulus is selected.

The current homogeneous swatch and rectangular clamped-strip inverse solvers cannot admit these
dogbone acquisitions without resolving actual geometry, grip state and deformation measurement.
Accordingly these records provide no membrane law, directional strain domain, recovery, bending,
friction, density or thermal model. The caliper measurement is `reported_thickness`, so it cannot
silently supply contact thickness or sewing clearance. Catalog mechanics and thermal readiness
remain unavailable, independently of the usable force-screening attributes.

## Exact source and verification

[Released workbook](https://data.mendeley.com/datasets/2pvf4x9crc/1/files/d1702584-64ca-4593-852a-f557c4349c2c):
4,385,263 bytes; published SHA-256, independently checked before transcription:

```text
56794ef58b20d7da55c2f9155561ee0a38784dbe424b2581266ac034919f387d
```

The binary workbook, unrelated leather alternatives and derived stress/strain columns are not
bundled. Focused regressions in `crates/ftx-pattern/tests/leather_tensile_catalog.rs` check the
40 source fields, SI conversion, exact-source direction filtering, real threshold selection,
offline frozen replay, and absent mechanics/transport authority. They establish software/data
behavior; the broader measured material and unchanged-garment acceptance gates remain open.
