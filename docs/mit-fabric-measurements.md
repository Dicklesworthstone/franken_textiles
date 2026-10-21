# MIT measured fabric specimens in the shared catalog

The built-in catalog includes 30 distinct specimens from Bouman, Xiao, Battaglia
and Freeman, *Estimating the Material Properties of Fabric from Video*, ICCV 2013.
Their primary dataset page identifies Lowell ACMTRL Laboratory as the measurement
laboratory. The source table supplies **60 numerical observations**: area weight and
test-specimen bending stiffness. Original specimen numbers are preserved as
`mit.fabric-2013.01` through `mit.fabric-2013.30`.

Sources inspected 2026-10-14:

- Dataset and aggregation conventions:
  https://people.csail.mit.edu/klbouman/pw/projects/materialproperties/dataset.html
- Published numerical table:
  https://people.csail.mit.edu/klbouman/pw/projects/materialproperties/images/materialproptable.png

Only numerical facts and short specimen descriptors are transcribed. Source
photographs, video and archives are not redistributed; no dataset-wide license
grant is asserted. These IDs remain a transcription of the published summary.
The separately identified source-row records below preserve the subsequently
inspected workbook; neither collection is independently repeated laboratory testing.

```sh
cargo run -p ftx-cli --bin ftx-materials -- catalog mit.fabric-2013.
cargo run -p ftx-cli --bin ftx-materials -- show mit.fabric-2013.08
```

## Identity and quantities

The entries include linen, separate cotton/silk/wool/Lycra specimens, denim,
corduroy, fleece, faux fur, velvet, canvas, nylon rip stop, terry knit, laminated
cotton, pleather, backed vinyl and multiple upholstery specimens. Similar names
do not merge different samples. Descriptors are not composition assays; a sample
called Lycra is not assigned 100% elastane, and pleather is not animal hide.

Original area weight in `oz/yd2` is normalized through the existing catalog to
`kg/m2`. Original bending stiffness in `lbf*in2` becomes the distinct property
`specimen_flexural_rigidity`, in `N*m2`. It does **not** populate sheet
`bending_rigidity`, whose dimension is `N*m`. Coupon geometry/width is absent from
the summary, so dividing by a guessed width or treating the two as interchangeable
would invent a constitutive parameter. The directional averages also cannot be
split into warp/weft coefficients.

The table's asterisk (one measurement) is preserved. Other rows retain the authors'
qualification that most were averaged over two orthogonal directions; an exact
replicate count is not invented. No standard deviation, confidence interval,
conditioning regime, test standard, thickness, usable roll width, full composition,
friction or modulus is manufactured from the photographs or labels.

All entries use the same `Material`, normalization, catalog discovery, frozen
selection and readiness checks as every existing physics/manufacturing consumer.
They are **partial characterization**, not fully drape-ready or sew-ready presets.
Matching additional measurements may extend a source-bearing record, but must not
be borrowed from another supplier's similarly named textile. Tests check all 30
IDs, the 60 observations, original/SI units, source-lock replay, aggregation scope
and rejection of the specimen-to-sheet unit confusion.

## Source workbook rows, inspected 2026-10-21

The same catalog now exposes **52 lettered source rows for those same 30 fabrics**
under `mit.raw-2013.01a` through `mit.raw-2013.30b` (letters/counts vary by fabric).
They add 208 source-reported scalar values: thickness, bulk density, area weight
and specimen flexural rigidity. This is not 52 new fabrics, 208 independent
measurements, or a claim that sample letters identify warp/weft or independent
replicates. The archive README identifies the letters as samples within a fabric,
and its confidence rating applies only to bending, with 1 highest confidence.
That ordinal remains text metadata, without statistical bounds or weighting.

Primary source: [ground-truth archive](https://people.csail.mit.edu/klbouman/pw/data/materialproperties/material_properties.zip),
45,664 bytes, SHA-256 `99021ff230ac8f08b87e111ff5d30fc4fc23303c10a1ffa1f1ac05eb934bc01d`.
Member `material_properties (2) 2/matprop.xlsx`, SHA-256
`94f71fbd0dbd67b108f0472f73a5b1c048b2917d3403f7863c981faaa84575c1`,
Sheet1 rows 2–53, columns A–G. Values are transcribed numerical facts; the workbook,
README, page and source media are not redistributed in Git. Sources retain both
hashes, row identity, citation and access date. The public page/README requests
citation and provides no dataset-wide redistribution license; none is asserted here.

Each row uses `published` evidence: primary laboratory data, not independently
verified measurements. Thickness in inches becomes metres; bulk density in
`lb/in3` becomes `kg/m3` using 0.45359237 kg/lb and 0.0254 m/in. The thickness
property is **`reported_thickness`**, not the qualified `thickness` consumed by
contact/sewing: unknown gauge pressure cannot establish unloaded stack clearance.
Density is the
bulk specimen value, not fiber-solid density. Column F includes cached spreadsheet
formula results (`density * thickness * 16 * 36 * 36`); its original value is
preserved, not counted as an independent mass acquisition or recomputed to remove
source rounding. Gauge pressure, conditioning, geometry, direction and quantitative
uncertainty are not supplied. Thickness must not be labelled unloaded loft or
qualified for an assumed pressure/humidity.

Known differences stay visible instead of silently replacing either source:

| Fabric | Published summary | Workbook rows |
|---|---|---|
| Silk 4, area weight (oz/yd2) | 1.975 | a: 2.583798789647521; b: 2.6949155381823258 |
| Lycra 29, specimen EI (lbf*in2) | 0.012 | a: 0.05 |
| Taffeta 7 | two-sample summary averages | a, b **and c**, each retained separately |

These differences are unresolved source discrepancies, not corrections or
permission to pool the records. Other small summary/row differences reflect
reported aggregation/rounding and remain unchanged too.

```sh
ftx materials catalog mit.raw-2013.
ftx materials show mit.raw-2013.08a
```

Linen rows 8a/8b report 0.0085/0.0095 in (0.2159/0.2413 mm), respectively.
Existing source-frozen binding and nominal assembly-mass accounting can use the
row's published area weight without a modulus. Scalar inspection can read the
reported thickness; sewing still refuses missing qualified thickness. Condition-filtered requests
for an unreported 20 C/65% RH observation refuse; missing sheet mechanics or heat
transport still refuses. Full raw-to-table reconciliation, matched coupon geometry,
direction, uncertainty and independent physical acceptance keep **`ftx-md1` open**.

## Next empirical gate

Resolve workbook/summary discrepancies and acquire exact coupon geometry, direction,
conditioning, specimen identity and source terms before deriving effective sheet
parameters. Independently characterize qualified thickness, membrane response,
directional bending and interface friction on matched linen, treated cowhide and
performance-knit specimens. Compare held-out drape and seam experiments without
using those acceptance cases to tune the material. These 30 summary records expand
measured coverage; they do not close that physical-validation gate.
