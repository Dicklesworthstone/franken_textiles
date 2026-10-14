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
grant is asserted. The raw ground-truth archive was not ingested. This is a
transcription of the published summary, not raw-replicate replay or independently
repeated laboratory testing.

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

## Next empirical gate

Acquire retained raw observations and exact coupon geometry, direction,
conditioning, specimen identity and source terms before deriving effective sheet
parameters. Independently characterize missing mass/thickness, membrane response,
directional bending and interface friction on matched linen, treated cowhide and
performance-knit specimens. Compare held-out drape and seam experiments without
using those acceptance cases to tune the material. These 30 summary records expand
measured coverage; they do not close that physical-validation gate.
