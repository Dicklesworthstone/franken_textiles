# Measured production-knit strip screening

The built-in catalog contains all 33 released knit specimens from Sperl, Sánchez-Banderas,
Li, Wojtan and Otaduy, *Estimation of Yarn-Level Simulation Models for Production Fabrics*
(SIGGRAPH 2022). These records let the existing material screener compare measured mass
and directional grip force without granting an unsupported simulation capability.

Primary sources:

- [Author project and dataset](https://mslab.es/projects/YarnLevelFabrics/)
- [Paper, DOI 10.1145/3528223.3530167](https://doi.org/10.1145/3528223.3530167), §4.3 and Figure 4
- [Original dataset archive](https://mslab.es/projects/YarnLevelFabrics/dataset/dataset.zip),
  `README.md`, `data/fabrics.json`, `data/{specimen}/stretch-{direction}.csv`, and
  `data/{specimen}/poisson-{direction}.csv`

## Original three specimens and usable observations

The original three records and their frozen-source identities remain unchanged. The
additional 30 specimens are described below; they use the same catalog and screening path.

| Catalog ID | Construction and reported yarn fractions | Areal mass (g/m²) | Reported gauge (mm) | Finish |
| --- | --- | ---: | ---: | --- |
| `sperl2022/SJ1` | Single jersey; 90% polyester, 10% spandex | 159 | 0.43 | Antimicrobial |
| `sperl2022/SJ3` | Single jersey; 87% polyester, 13% spandex | 238 | 0.63 | Water repellent |
| `sperl2022/DKIN8` | Double-knit interlock; 39% polyester, 38% Celliant-labelled polyester, 23% spandex | 267 | 0.62 | Antimicrobial |

Composition refers to the source's yarn mass fractions. Original yarn names, denier,
filament counts and yarn counts remain in each record. Celliant additive composition,
finish mass, purchasable supplier SKU and lot equivalence are not supplied.

The paper describes displacement-controlled strips with **100 mm free gauge length**
and **60 mm reference width**. It measures total force in newtons. Warp, weft and bias
are the source's 90°, 0° and 45° cuts. Extension is the change in grip separation divided
by its initial value. Each value below is an exact row tagged `loading` in the author CSV.

| Specimen | Warp at 10% (N) | Weft at 10% (N) | Bias at 10% (N) | Warp at 30% (N) | Weft at 30% (N) | Bias at 30% (N) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| SJ1 | 0.40 | 0.04 | 0.27 | 1.94 | 0.84 | 1.12 |
| SJ3 | 0.71 | 0.80 | 0.53 | 2.11 | 2.48 | 1.78 |
| DKIN8 | 1.21 | 1.56 | 1.15 | 3.39 | 5.00 | 3.39 |

The six force properties are `strip_force_10pct_warp`, `strip_force_10pct_weft`,
`strip_force_10pct_bias`, `strip_force_30pct_warp`, `strip_force_30pct_weft`, and
`strip_force_30pct_bias`, all in `N`. `areal_density` uses the existing mass consumer
path and normalizes from source `g/m2` to `kg/m2`. Together with the gauge extension,
the three records supply **24 source-qualified scalar observations**.

The paper uses loading forces up to 5 N as its working range. The retained CSVs include
unloading and higher forces; this import only exposes the exact 10% and 30% rows above.
DKIN8's weft value at 30% is the reported 5.00 N point, not an imposed cap: its neighboring
30.2% and 30.4% source rows are 4.98 N and 5.05 N. The publication does not establish
whether the CSVs are unprocessed instrument output. Their upstream processing and
instrument-level rawness remain unspecified. This import performs no additional
smoothing, interpolation, force inversion or extrapolation.

## Use the existing material screener

This request chooses the lightest of these specimens whose reported weft grip force
at 30% extension is at least 2 N on the declared strip geometry:

```json
{
  "schema": "ftx-material-screen/1",
  "basis": "nominal",
  "candidates": [
    {"key": "sperl2022/SJ1", "selection": {"id": "sperl2022/SJ1"}},
    {"key": "sperl2022/SJ3", "selection": {"id": "sperl2022/SJ3"}},
    {"key": "sperl2022/DKIN8", "selection": {"id": "sperl2022/DKIN8"}}
  ],
  "forms": ["knit"],
  "allow_mixed_protocols": true,
  "constraints": [{
    "attribute": {
      "key": "strip_force_30pct_weft",
      "unit": "N",
      "conditions": {
        "branch": "loading",
        "free_gauge_length_mm": "100",
        "reference_strip_width_mm": "60"
      }
    },
    "minimum": 2.0
  }],
  "objectives": [{
    "attribute": { "key": "areal_density", "unit": "kg/m2" },
    "direction": "minimize"
  }]
}
```

The frontier is `sperl2022/SJ3` (238 g/m²; 2.48 N). The explicit mixed-protocol consent
allows comparison across distinct specimen identities; the report retains every source
and condition. Without it SJ3 and DKIN8 remain incomparable. Source-reported bounds
are absent, so a `reported_bounds` request cannot rank them. No estimate consent is
needed for these exact reported observations.

## Matched transverse acquisitions for finite-strip identification

The same source archive also provides nine `poisson-{direction}.csv` acquisitions for
these exact fabric codenames. The existing source fixture retains all **66 original
rows** (1,736 CSV bytes) under `transverse_curves`, with the same specimen/direction,
archive-entry, SHA-256 and original CSV fields as the force acquisitions, plus each
transverse file's retrieval date. The original units and license README is retained
under `transverse_readme` with its own hash and retrieval date. Original `fabrics.json`
links each file to its fabric and cut direction. No transverse law or additional
catalog scalar is inferred from this retention.

Their two columns are `elongation,compression`, both dimensionless fractions. Paper
§4.3 defines the reported compression as `c = 1 - w / w0`, where `w0 = 60 mm` and `w`
is the deformed swatch width. The following values are rounded for display; the fixture
preserves all original decimal strings and line endings.

| Specimen | Direction | Rows | Last grip extension | Compression at 20% | Compression at 40% |
| --- | --- | ---: | ---: | ---: | ---: |
| SJ1 | Warp | 6 | 91.4% | 0.009692 | 0.044539 |
| SJ1 | Weft | 10 | 161.8% | 0.027712 | 0.034851 |
| SJ1 | Bias | 7 | 112.4% | 0.092863 | 0.193365 |
| SJ3 | Warp | 7 | 112.2% | 0.017545 | 0.069127 |
| SJ3 | Weft | 8 | 135.2% | 0.057209 | 0.109497 |
| SJ3 | Bias | 7 | 115.6% | 0.097683 | 0.187596 |
| DKIN8 | Warp | 7 | 104.8% | 0.017299 | 0.040780 |
| DKIN8 | Weft | 7 | 101.0% | 0.000162 | 0.000442 |
| DKIN8 | Bias | 7 | 102.2% | 0.043747 | 0.089512 |

These files do **not** supply a loading/unloading tag, timestamps, an exact width
measurement station, an edge statistic or the measurement algorithm. The paper's
identification uses loading, but that does not turn an absent CSV tag into a recorded
branch. A finite-strip consumer must explicitly declare its width observation operator
and any assumed branch mapping; midpoint width, minimum width, average width and local
transverse strain are not interchangeable. The acquisition parser's
`TransverseCurve.measurement_scope` preserves this ambiguity alongside the measurements.
The canonical strip calibrator's optional `mid_width` specifically denotes distance
between opposite side material points at the reference mid-gauge. The source's reported
compression cannot automatically be mapped to that operator: Figure 4 depicts width near
the center but does not establish which image locations or material points are sampled,
particularly for bias cuts. There is no interpolation or silent match between neighboring
force and compression nodes. The retained terminal extensions can exceed the paper's
stated 5 N working range and are not automatically qualified for use.

A fresh inspection of the author archive on 2026-10-21 confirms all eighteen retained
force/transverse CSV hashes, metadata and README unchanged. The archive SHA-256 is
`4ef059e1a33844fbd082c3662ac2df4444ab9b5280b0f1be49f66b11c44d7b8d`.
Its only Python helper, `explore.py` (SHA-256
`337b48a649a3fa5d6bb7f8f518cb471c0b3305824a6c72fe3cf7959b483465e6`),
loads and plots the supplied columns; it contains no width-acquisition algorithm or
loading-branch assignment for the transverse data. The notebook only invokes that
helper. Paper §4.3/Figure 4 and the supplementary specimen tables do not resolve the
material-point/edge sampling. This inspection supplies no new width qualification;
the original `ftx-ki4` acceptance remains open.

Paper §5.1, equations (1)–(2), fits both force and transverse compression using finite
swatch equilibrium and six parameters: four orthotropic StVK coefficients plus two
area-preservation terms. The existing four-coefficient orthotropic sheet law is a
different, more restricted model. Three directional force curves alone do not establish
that all its coefficients are identifiable: in the small-strain limit their three axial
slopes supply at most three independent scalar observables. Finite-strain sensitivity
and the additional transverse channel must be assessed under the declared model and
observation operator, with held-out states and residuals. Retaining these measurements
does not publish a fitted constitutive law or establish independent-coupon agreement.

## Physical limits and source retention

Temperature, relative humidity, conditioning duration, displacement rate, replicate
uncertainty and thickness gauge pressure/instrument are not supplied. Consequently,
the reported gauge is `specimen_gauge_thickness` in `mm`; it does **not** populate
canonical unloaded `thickness` or authorize clearance, thermal or layering calculations.

The clamps produce spatially nonuniform deformation. Grip extension is not a homogeneous
deformation gradient, and dividing grip force by strip width does not supply the missing
biaxial constitutive characterization. These records have no membrane law, shear modulus,
bending rigidity, viscoelastic recovery law, thermal resistance or validated garment fit.
Mechanics and thermal capability requests therefore remain unavailable. A future inverse
identification of these records must qualify the actual test geometry, boundary conditions,
observation operator and accepted model. The generic finite-strip inverse is available
through `ftx materials calibrate-strip` and `material_calibrate_strip`; it models ideal
grips and planar elastic equilibrium, and evaluates identifiability, refinement and
held-out checks before publishing estimated coefficients. Its synthetic numerical recovery
tests do not establish that the four-parameter model fits these production knits. These
primary catalog records remain unfitted.

`crates/ftx-pattern/src/material_catalog/knit-strip-measurements.json` retains the original
nine stretch and nine transverse CSV byte strings, the full original `fabrics.json`,
the original units/license README, individual SHA-256 hashes, archive paths, recorded
retrieval dates and complete source MIT license/copyright notice. CSV line endings,
signed values and the stretch files'
loading/unloading tags are preserved. No branch tag is invented for transverse files.
The upstream metadata
SHA-256 is `258e3cc2a40ba40d1e0152f80b1fb08c67d14c08db10efbc2ec385669887a6d4`.
The source archive was read in bounded ZIP ranges; photographs and geometry are not bundled.

Normal catalog construction uses compact transcriptions. Tests independently reconstruct
all three records from the retained original source files, verify their hashes and exact
loading rows, and compare the complete records. Separate checks preserve all transverse
source bytes, metadata links and 66 observation nodes without changing the existing
catalog records. The source fixture is excluded from the normal native/browser catalog
binary.

## Complete released construction cohort

The 2026-10-30 acquisition adds the remaining **30 records**, with **238 scalar
observations**. The complete cohort has 33 fabrics and 262 observations: source areal
mass and gauge thickness for every fabric, and 196 exact directional loading-force
readings. The two missing readings remain absent. A specimen is one source fabric
identity, not an independently sampled supplier, a distinct fiber or a retail SKU.

| Construction from the author supplement | Source specimens | Records |
| --- | --- | ---: |
| All needle | A1–A3 | 3 |
| Double-knit interlock | DKIN1–DKIN11 | 11 |
| Double-knit pique | DKP | 1 |
| Links | L1–L3 | 3 |
| Single jersey | SJ1–SJ15 | 15 |

Construction names come from the named sections of the
[author supplementary document](https://mslab.es/projects/YarnLevelFabrics/contents/sperl2022eylsmpf_supplementary.pdf),
not from a specimen-prefix guess. Original yarn names, denier/filament labels, yarn
counts, mass fractions and finishes remain attached to each record. The source-only
`75D/72F CD` label in DKIN7 and `50D/34F Lycra T400` in SJ10 do not independently name
constituent fibers. Their unresolved fractions are preserved in source yarn metadata;
they are not silently reclassified as polyester or elastane. DKIN7 consequently has
unknown fiber composition, and SJ10 has a known 60% polyester fraction with incomplete
whole-fabric composition.

All three original source directions are retained for every fabric. DKIN4 and DKIN5
have no exact loading row at 30% warp extension. Their `strip_force_30pct_warp` property
is absent; their 10% warp values and measured weft/bias values remain usable. No
neighboring row, unloading row, interpolation or extrapolation fills that gap.

Some additional exact loading rows exceed the paper's stated 5 N test/working range.
The disagreement between that description and the released source rows stays visible.
For example,
DKIN1 reports 7.22 N at 30% warp extension. The observation is retained with
`paper_working_force_limit_n: "5"` and
`within_paper_working_force_range: "false"`. The same fields mark readings within the
range. These qualifiers apply to the additional records; the original three records
retain their previous content locks. A retained measurement beyond the paper's working
range does not expand the validity of its model or qualify a canonical material law.

To compare all 33 specimens, replace the preceding example's explicit `candidates`
with `"query": "sperl2022/"`. With the same minimum 2 N weft-force condition and
minimum-mass objective, the frontier becomes **SJ11**: 105 g/m² and 3.67 N at 30%
weft grip extension. This is a useful change in a measured tradeoff, not a claim that
SJ11 predicts the best garment fit. Missing mechanics, transport, gauges, supplier
quotes and physical validation remain separate capabilities and still refuse.

The fresh complete archive is 142,552,558 bytes with SHA-256
`4ef059e1a33844fbd082c3662ac2df4444ab9b5280b0f1be49f66b11c44d7b8d`, identical to the
previous source pin. Metadata and the dataset MIT license are unchanged. The separate
`tests/fixtures/materials/knit-strip-expanded.json` retention preserves all **90 additional
force CSVs and 103,242 ordered observations**, complete original metadata and the MIT
copyright/permission notice. SHA-256 of this retention is
`d5aa7b247a84dc2158d91e7e2c859cfa331876bd54849b7e8f128753da432a9d`. Original byte strings,
signed forces and loading/unloading order survive intact. Photos, yarn geometry,
supplementary pages and plots are not redistributed.

The existing `parse_acquisition` API now also admits this exact 2026-10-30 acquisition
or subsets of its pinned curves. It accepts at most 99 histories and 4 MiB. A new-curve
request cannot adopt the earlier retrieval date; renamed specimens, paths, cuts or
altered-and-rehashed observations refuse. The previous nine-force and nine-transverse
acquisition continues unchanged. No extra transverse observations or unknown width
measurement operators are inferred.

Normal catalog construction loads only the compact 29,337-byte factual summary, with
SHA-256 `675b7217773a7cc7e0f0e08a90213141b2d9a5466e9efd925c6c0888c94ffd68`; the 2.6 MB
original acquisition is excluded from normal native/browser binaries. Focused regressions
replay every compact force from its original CSV, verify all source metadata and the two
missing rows, exercise the actual 33-candidate screen, preserve incomplete composition,
and check offline frozen selection and absent-physics refusals. The supplementary PDF
used for construction labels is 10,226,857 bytes, SHA-256
`b12a6771a72120111c092273ecfddfa293cd427ec53ea07255ec91bd328edf6d`; its copyright remains
with the source, and it is not bundled.
