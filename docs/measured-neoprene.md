# Measured neoprene sheets and lined assemblies

The built-in catalog includes 33 neoprene specimen groups with 231 source measurements:
reference thickness, areal mass, dry thermal resistance and four compression displacements
per group. The measured resistance is immediately usable by the existing steady conduction
and layer-stack consumers. Changing the ordinary material selection changes the resistance
used by those consumers without a separate neoprene solver.

## Source and specimen identities

[Potočić Matković, Salopek Čubrić and Pavko Čuden (2025), *Dependence of Thermal Comfort of
Diving Suit on Neoprene Properties and Diving Depth*, Polymers 17(21):2820](https://www.mdpi.com/2073-4360/17/21/2820),
[DOI 10.3390/polym17212820](https://doi.org/10.3390/polym17212820), Tables 1 and 2 and the
section 2 methods. Reviewed 2026-10-19. The article is CC BY 4.0, copyright 2025 the authors.
The catalog retains attributed numerical facts and paraphrased protocol descriptions;
article text and figures are not bundled.

IDs begin with `zagreb/neoprene-2025/`, followed by the lower-case source mark. For example,
study sample `M3NOb5` is `zagreb/neoprene-2025/m3nob5`. The following prefixes each have
the suffixes `3`, `5` and `7`, accounting for all 33 records:

| ID suffix prefix | Source construction | Catalog form |
|---|---|---|
| `m1osa`, `m1osb` | One-sided lined neoprene | `laminate` |
| `m1no` | Unlined neoprene | `sheet` |
| `m2bs` | Both-sided lined neoprene | `laminate` |
| `m2no` | Unlined neoprene | `sheet` |
| `m2os` | One-sided lined neoprene | `laminate` |
| `m3noa`, `m3nob`, `m3noc` | Unlined neoprene | `sheet` |
| `m4bs` | Both-sided lined neoprene | `laminate` |
| `m4os` | One-sided lined neoprene | `laminate` |

`M1`–`M4` identify four anonymized manufacturers. `NO`, `OS` and `BS` describe the lining;
letter variants remain distinct study groups. The final digit is nominal thickness in
millimetres. The measured thickness may differ substantially: `M2BS5`, for example, is
6.9833 mm at the paper's 1 kPa protocol, despite its nominal 5 mm label. No supplier, retail
product, lot or individual coupon identity is inferred. No mass fractions are invented for
the neoprene polymer, foam additives, adhesive or linings.

## What each number means

| Catalog quantity | Original unit | Source and retained test conditions |
|---|---|---|
| `thickness` | mm | Table 1; ISO 5084, DM-2000 gauge, 1 kPa, 20 cm² contact/sample area, mean of 10 locations |
| `areal_density` | g/m² | Table 1; ISO 3801, Kern ALJ 220-4 analytical scale |
| `thermal_resistance` | m² K/W | Table 1 dry sample-only Rct; Thermetrics sweating-guarded hotplate, ambient 20 °C, RH 65 ± 1%, air speed 1 m/s; Equation (1) subtracts the bare-plate resistance |
| `compression_displacement_50kpa` | mm | Table 2; Instron 5567, applied compressive stress 50 kPa, mean of five measurements |
| `compression_displacement_100kpa` | mm | Same compression protocol at 100 kPa |
| `compression_displacement_150kpa` | mm | Same compression protocol at 150 kPa |
| `compression_displacement_200kpa` | mm | Same compression protocol at 200 kPa |

Authored quantities retain their original units; ordinary resolution converts thickness to
metres and areal mass to kg/m². Displacements remain millimetre-valued extension observations
with their own `pressure_pa` conditions. They are not automatically consumed as thickness
or as a constitutive law. The source mark connects measurements at the specimen-group
level; it does not prove that the same individual coupon was used in every instrument.

The hotplate's numerical plate temperature, applied measurement pressure and thermal
replicate count are not reported in the methods. Its ambient temperature is not copied
into the thickness or compression protocols. Preconditioning duration, specimen moisture
content and numerical uncertainty are also unreported. Every quantity preserves those
limits, and no uncertainty bounds are inferred from decimal precision or replicate count.

Table 1 supplies the numerical authority for the catalog. Its `M3NOa3` resistance is
0.0535 m² K/W; the narrative instead gives 0.052. That discrepancy is retained in the
source note rather than silently averaging or replacing the table entry.

## Pressure observations remain separate from pressure qualification

Table 2 reports **compressive displacement**, not final loaded thickness. Figure 1 relates
displacement and initial thickness, but the Instron preload and displacement zero are not
specified. The reference thickness was measured separately with a different instrument at
1 kPa. Loading rate, dwell time, unloading, recovery and compatibility of the two instrument
references remain unspecified. The catalog therefore preserves the four original
displacements without deriving loaded thickness or fabricating a pressure-conditioning
input table. The 1 kPa reference is also not a measurement of unloaded loft.

Figures 2 and 4 give estimated resistance under pressure using cross-sample regression.
Those values are not measured loaded resistance and are not imported. The Table 1 Rct
does not become invariant under compression, and reference areal mass is not asserted to
be pressure invariant. These records do not automatically qualify a material for
`pressure-condition`; that preparation still needs the matching reference definition,
complete source-qualified pressure rows and explicit evidence for any retained invariant.
See [pressure-conditioned materials](pressure-conditioned-materials.md).

The usable thermal property has the existing scalar, fixed-characterization semantics.
It supplies neither a measured temperature validity interval nor a wet, submerged,
pressure-dependent, loading/unloading or recovery law. Specific heat, membrane response,
bending, friction and optical face observations remain missing. Steady conduction can
use the measured Rct; transient heat storage, drape and radiation qualification continue
to require their own inputs.

## Run the example and change the selected material

```sh
cargo run --locked -p ftx-cli --bin ftx-materials -- \
  show zagreb/neoprene-2025/m3nob5

cargo run --locked -p ftx-cli --bin ftx-materials -- \
  thermal-steady tests/fixtures/materials/measured-neoprene-steady.json \
  > neoprene-steady-result.json
```

The fixture selects measured `M3NOb5` with estimates disabled. Its area, reservoirs and
films are **synthetic scenario inputs**, not a reconstruction of the paper's apparatus,
a measured wetsuit or a diving exposure. The declared 0.6 m² area, 35/20 °C reservoirs,
0.03/0.05 m² K/W films and measured Rct 0.1720 produce a calculated 59.5238 W/m²,
35.7143 W total. The two calculated material faces are approximately 33.2143 °C and
22.9762 °C. These are consequences of the declared steady model and source resistance.

Changing only the layer ID to `zagreb/neoprene-2025/m1no5` selects another nominal 5 mm
study group, with measured Rct 0.0915, and gives approximately 52.4781 W at the same
boundaries. This is a comparison of separate characterized groups; it does not isolate a
single composition, lining or thickness effect.

The result freezes all seven original observations, their separate protocols, citations,
estimate policy and material identity. It can be replayed offline:

```sh
jq '.request' neoprene-steady-result.json > neoprene-steady-replay.json
cargo run --locked -p ftx-cli --bin ftx-materials -- \
  thermal-steady neoprene-steady-replay.json > neoprene-steady-replayed-result.json
cmp neoprene-steady-result.json neoprene-steady-replayed-result.json
```

`neoprene_catalog.rs` tests the 33-record coverage, independent table entries, unit
normalization, protocol separation, refusal of missing physical inputs, pressure-context
hash locks and material substitution through the existing conduction/stack evaluators.
`neoprene_workflow.rs` runs the actual CLI fixture, verifies its face temperatures and
heat flow against the declared equations, changes the material selection and replays the
frozen request with an empty catalog. These establish data and software integration;
they do not constitute independent physical or wearer-comfort validation.
