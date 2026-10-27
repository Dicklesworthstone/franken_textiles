# Fabric-101 measured material metadata

The built-in catalog includes **96 additional released Fabric-101 specimens**, with **192 measured
observations**: areal density and reported test thickness for each exact specimen. These records work
with the existing material selection, source freezing, mass accounting and scalar screening paths.
They bring the complete bundled catalog to **371 records**.

The source is Gong, Mao, Yang, Lu, Wang and Komura, *Learnable Persistent Wrinkle Formation in Cloth
Simulation*, [arXiv:2609.13707v1](https://arxiv.org/html/2609.13707v1), and the authors'
[Fabric-101 data release](https://github.com/GongDeshan/Fabric_101_for_Wrinkles/tree/510519be0657a341a1a84fdb5c24be156ac83c9f).
The release was checked on 2026-10-27. Its README still said “Coming Soon,” but the pinned data tree
contained the specimen files used here.

## What can be selected

Every ID starts with `fabric101-2026/`; the suffix is the original release ID lowercased, with spaces
replaced by hyphens. `structure.source_fabric_id` retains the exact original capitalization and spacing.
Source categories are descriptors, not inferred purity or construction. For example, the source calls
Drape 11 a cotton-category fabric, while its concordant paper composition is 65% polyester/35% cotton.

| Example ID | Exact release specimen | Areal density, g/m² (display rounded) | Reported thickness, mm |
|---|---|---:|---:|
| `fabric101-2026/l01` | Linen Plain | 117.384 | 0.4 |
| `fabric101-2026/l03` | Ramie Plain | 65.771 | 0.3 |
| `fabric101-2026/drape-8` | Washed Linen Aqua | 240.260 | 0.8 |
| `fabric101-2026/kn01` | Cotton Jersey | 174.337 | 0.7 |
| `fabric101-2026/kn02` | Cotton Rib | 309.959 | 0.9 |
| `fabric101-2026/ex-08` | Coolmax Jersey | 128.734 | 0.6 |
| `fabric101-2026/ex-05` | Bamboo Rayon Plain | 251.123 | 0.6 |
| `fabric101-2026/e-c05` | Polyester/Cotton Plain | 102.973 | 0.4 |
| `fabric101-2026/ex-17` | Tencel/Linen Blend | 138.151 | 0.4 |
| `fabric101-2026/e-f02` | Waterproof Breathable | 126.124 | 0.2 |

The 96 rows comprise 32 cotton-category, four linen/ramie, seven wool, 15 silk, nine other
cellulose-derived fiber, ten polyester, four nylon, one acrylic, ten blend, three coated/functional,
and one paper specimen. The paper specimen is the study's printing-paper comparison sheet. Nine
records have explicit knit construction; ambiguous constructions use `sheet`. The instrument's
`fabrictype=woven` field is also printed on jersey and rib specimens and does not override their
actual descriptions.

Nominal compositions are included only where the article's Tables 11–13 can be joined to the same
release ID, description and matching mass/thickness. The paper's DP 1–12 correspond to the release's
Drape 1–12 by description and both measurements. Unquantified blends and ambiguous/conflicting
identities retain descriptive names without invented fractions. Bamboo rayon is explicitly
regenerated viscose, distinct from a bamboo bast fiber.

## Units, conditions and source identity

`areal_density` retains the original full-precision CSV value in `g/m2` and normalizes through the
ordinary material authority to `kg/m2`. `reported_thickness` retains `mm` and normalizes to `m`.
The article describes a balance with 0.001 g resolution and a fabric thickness tester. It does not
give thickness pressure, foot area, dwell time, density/thickness repeat count or measurement
uncertainty. Consequently, the adapter supplies no uncertainty bounds and does not interpret the
CSV's decimal digits as measurement accuracy. Test thickness is not installed as unloaded physical
`thickness`, contact separation or a fabric compression law.

The globally reported conditioning protocol is 24 hours at 20 ± 2 °C and 65 ± 5% RH, following
ISO 139:2005. Individual conditioning logs and moisture content are unavailable. Each observation
retains its exact instrument specimen ID, release CSV line and revision. Each material retains its
raw metadata path and Git blob hash, actual experiment directory, article citation and discrepancies.
For example, C01 is tied to `data/C01/002/head.dat`, not an invented `001` run.

| Source | Immutable identity |
|---|---|
| Release Git commit | `510519be0657a341a1a84fdb5c24be156ac83c9f` |
| `fabric_catalog.csv`, 5,835 bytes | SHA-256 `6a460054c3031554733c7706a8327c89e323ff4a4657352f5bcec0494d7cf96a` |
| `data_folder_mapping.csv`, 4,154 bytes | SHA-256 `57d7e0225b37ee7ca556b91f1e20d0d89450dd1927f3992ac467a42b4d36933c` |
| Article | arXiv `2609.13707v1`, §3.1, Tables 11–13 |
| Per-specimen instrument metadata | Exact pinned path and Git blob SHA-1 in every material |

Only the selected numerical and descriptive observations are transcribed. No upstream implementation
or complete raw-curve archive is redistributed. The README advertises MIT, but the pinned tree has
no LICENSE file; this adapter does not assert a license grant over those upstream assets.

## Deliberate exclusions and conflicting primary observations

There are 103 rows in the release CSV. Only 98 map to released raw specimen folders. All 98 mapped
headers were checked; 96 agree with the catalog within the metadata's printed numerical precision.

| Excluded identity | Reason |
|---|---|
| Drape 4 | CSV 114.61675579322639 g/m² conflicts with instrument 116.1056129 g/m² |
| E-KN06 | CSV 217.86096256684493 g/m² conflicts with instrument 172.13 g/m² |
| EX-03, R04, EX-15, Paper 2, Paper 3 | CSV rows without a matching released raw specimen folder |
| Cotton, Linen, Wool, PET, Silk demonstration folders | Authors' mapping explicitly says these are not catalog-ID matches |

For S08–S12, the article and release assign different silk descriptions/measurements to the same IDs.
The release and its own instrument metadata agree; those exact observations are retained, with no
composition or mechanical-property join to the article's conflicting IDs. EX-06 has a similar
cross-ID article discrepancy. E-KN07's release and instrument agree on 321.03 g/m², while the article
prints 231.71 g/m²; the adapter retains the release value and records the discrepancy.

## Raw experiments are locatable, not converted to sheet laws

The source tests a 260 × 110 mm fabric rectangle around radius-40 mm cylindrical holders, with a
nominal free height of 50 mm. It applies five twist cycles, five axial compression cycles, then a
stretch/slip pull. Files record force in N, displacement in mm, torque in N·m, rotation in degrees and
time in seconds. These are coupled, history-dependent cylindrical fixture responses.

For L01 warp `s1`, raw timestamps give a median positive twist rate of about 0.499854 °/s and
compression rate of 0.75 mm/s. The article reports 0.1 °/s and 1 mm/s. Its measured peak compression
of 16.14125 mm includes 1.14125 mm of pretension displacement in addition to the nominal 15 mm.
These differences remain visible in the record. Reusing the curves requires an explicitly qualified
cylinder fixture, actual pretension/reference geometry, tare, timing and sequential history.

No membrane modulus, Poisson coupling, shear response, bending rigidity, friction coefficient,
thermal insulation, laundering law or retail price is supplied by this adapter. A mass-based
screen can select these observations; a request that requires mechanics or thermal readiness
correctly refuses them until those measurements are supplied. Frozen selections replay against
an empty catalog with their exact sources and checksums intact.

## Focused checks

`crates/ftx-pattern/tests/fabric101_catalog.rs` verifies the 96-row subset, exact representative
measurements and units, source paths and field budgets, conflict handling, composition identity,
immutable replay, stale-digest refusal, ordinary mass screening, and missing-physics refusal.
The source implementation is `crates/ftx-pattern/src/material_catalog/fabric101.rs`.
