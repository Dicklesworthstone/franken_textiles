# Construction-specific flax and hemp/linen specimens

The shared catalog contains six woven specimen groups from Vasile, Vermeire, Vandepitte,
Troch and De Raeve, *Effect of Weave and Weft Type on Mechanical and Comfort Properties
of Hemp–Linen Fabrics*, Materials 17(7), 1650 (2024),
DOI [10.3390/ma17071650](https://doi.org/10.3390/ma17071650).

The [published PDF mirrored by Semantic Scholar](https://pdfs.semanticscholar.org/f845/0aecca9c9b78efa94ff19a3e44389e3d522e.pdf)
was read as text and visually checked on 2026-11-03. Its SHA-256 is
`823e79cdf4ebf902bfd4443524361817f9ca5610b041cf4b65ae3937a363849b`.
Tables 1, 2 and 4 appear on PDF pages 3, 6 and 10. The article is CC BY 4.0;
the adapter retains attributed factual observations and explicit unit conversions.
The PDF, photographs and figure images are not redistributed.

## Exact construction variants

All six fabrics use flax warp yarn, nominal Nm 26/1, with 24 ends/cm. Weft construction
is 23 picks/cm, using either hemp Nm 26/1 or flax Nm 24/1. These are source-described
construction settings, not independently acquired finished-fabric count measurements.
The 300 cm loom width does not become purchasing or cutting width.

| Catalog suffix after `vasile/2024/` | Weave | Areal mass, g/m² | Gauge thickness, mm | Bulk density, kg/m³ | Abrasion cycles |
|---|---|---:|---:|---:|---:|
| `hemp-plain` | Plain 1/1 | 193.2 ± 2.1 | 0.52 ± 0.01 | 375.2 ± 10.6 | 5000 |
| `hemp-pan` | Panama 2/2 | 181.3 ± 2.8 | 0.54 ± 0.01 | 338.4 ± 9.7 | 4000 |
| `hemp-twill` | Twill 2/2 | 184.0 ± 3.2 | 0.55 ± 0.01 | 333.4 ± 8.6 | 6000 |
| `flax-plain` | Plain 1/1 | 199.7 ± 1.7 | 0.51 ± 0.01 | 392.0 ± 10.8 | 5000 |
| `flax-pan` | Panama 2/2 | 188.9 ± 1.9 | 0.55 ± 0.02 | 346.2 ± 9.7 | 4000 |
| `flax-twill` | Twill 2/2 | 188.9 ± 2.6 | 0.54 ± 0.01 | 350.1 ± 5.9 | 6000 |

The source identifies both yarn directions as flax for the three linen records. For the
hybrid records it identifies the fibers by direction but only **assumes** equal mass
fractions in its porosity calculation. Their exact composition remains incomplete; no
50:50 mass fraction or fiber-mixture law is invented.

## Acquisition and consumer meaning

The 36 scalar observations share the source's 24-hour conditioning at 20 ± 2 °C and
65 ± 4% relative humidity (ISO 139:2005). Their original units, source method, specimen
group and acquisition qualifiers survive normalized and frozen selections.

- `areal_density`: measured mean from ten 100 cm² coupons, ISO 3801:1977. This is
  conditioned mass, not an independently measured oven-dry substrate mass.
- `reported_thickness`: measured mean from ten Hess HDM-3 gauge readings at **1 kPa**,
  ISO 5084:1996. It remains separate from contact `thickness` and manufacturing
  `unloaded_thickness`; the records establish neither clearance nor compression recovery.
- `bulk_density`: the paper's calculated fabric density, retained as `published` evidence.
  It is not constituent-fiber density. Recomputing from rounded mean mass and thickness
  would change several table values, so the source values remain authoritative.
- `thread_density_warp` and `thread_density_weft`: published construction counts,
  normalized from 1/cm to 1/m without deriving a membrane law.
- `abrasion_cycles`: measured Martindale result under **12 kPa**, ISO 12947-2:2016.
  The study used four specimens per fabric, inspected every 1000 cycles, with a 6000-cycle
  test limit and breakdown of the first two yarns as its criterion. Table 4 gives one
  count per fabric, without trial-level variability. These are protocol-qualified
  observations, not continuous failure times or guaranteed garment lifetimes.

Table 2 explicitly labels its plus/minus values as standard deviations. They are retained
in `conditions.reported_standard_deviation` with their original units; `bounds` stays
absent. Neither screening nor mass accounting may treat them as a guaranteed interval.
Table precision is preserved despite the methods prose's coarser rounding statements.

Tensile, bending, tear, air-flow and thermal results appear in plots without a complete
numeric table for these observations; no bar heights have been digitized into nominal
coefficients. The air test prose also labels its pressure drop in N, and the thermal
conductivity is measured during FTT compression. No unrestricted air-permeability or
conductivity law is inferred. The MMT indices in Table 6 do not establish equilibrium
sorption, vapor permeability or a wet constitutive model and are outside this tranche.

## Use through existing material selection

```sh
cargo run -p ftx-cli --bin ftx-materials -- show vasile/2024/flax-plain
cargo run -p ftx-cli --bin ftx -- materials freeze vasile/2024/hemp-twill --json
```

Ordinary catalog screening can combine an explicit areal-mass limit with the observed
abrasion count and exact test qualifiers. A frozen selection binds to a pattern through
the existing `material_binding::Changes` path. Changing only the selected record changes
the shared sheet-mass result while preserving the pattern's pieces and seams. Missing
mechanics, thermal storage/conduction and unloaded clearance remain unavailable.

`cargo test -p ftx-pattern --test flax_hemp_catalog` checks the primary table transcription,
SI conversion, source-preserving offline replay, unchanged-pattern mass substitution,
protocol-specific screening and independent refusal of absent capabilities. These are
data/consumer checks; they do not validate a garment's drape, sewn fit, warmth or durability.
