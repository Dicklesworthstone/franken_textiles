# Measured production-knit strip screening

The built-in catalog contains three matched knit specimens from Sperl, Sánchez-Banderas,
Li, Wojtan and Otaduy, *Estimation of Yarn-Level Simulation Models for Production Fabrics*
(SIGGRAPH 2022). These records let the existing material screener compare measured mass
and directional grip force without granting an unsupported simulation capability.

Primary sources:

- [Author project and dataset](https://mslab.es/projects/YarnLevelFabrics/)
- [Paper, DOI 10.1145/3528223.3530167](https://doi.org/10.1145/3528223.3530167), §4.3 and Figure 4
- [Original dataset archive](https://mslab.es/projects/YarnLevelFabrics/dataset/dataset.zip),
  `README.md`, `data/fabrics.json`, and `data/{specimen}/stretch-{direction}.csv`

## Specimens and usable observations

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
  "query": "sperl2022/",
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
solver must model the actual test geometry and boundary conditions before identifying a law.

`crates/ftx-pattern/src/material_catalog/knit-strip-measurements.json` retains the original
nine stretch CSV byte strings, the full original `fabrics.json`, individual SHA-256 hashes,
archive paths, retrieval date and complete source MIT license/copyright notice. CSV line
endings, signed values and loading/unloading tags are preserved. The upstream metadata
SHA-256 is `258e3cc2a40ba40d1e0152f80b1fb08c67d14c08db10efbc2ec385669887a6d4`.
The source archive was read in bounded ZIP ranges; photographs and geometry are not bundled.

Normal catalog construction uses compact transcriptions. Tests independently reconstruct
all three records from the retained original source files, verify their hashes and exact
loading rows, and compare the complete records. The source fixture is excluded from the
normal native/browser catalog binary.
