# Published textile thermal fits: NISTIR 7282

The built-in material catalog now includes ten historical material groups from NISTIR 7282
(June 2005). Seven provide a complete **effective thermal estimate** with explicit estimate
consent; three retain conflicting source observations but refuse complete thermal evaluation.
These are not current supplier specifications, raw-fiber properties or qualified garment designs.
No measured warp/weft stiffness, bending or friction is invented to make them drape-ready.

## Use the real material records

```sh
cargo run -p ftx-cli --bin ftx-materials -- catalog nist/7282/
cargo run -p ftx-cli --bin ftx-materials -- \
  thermal-at nist/7282/aralite 50 --allow-estimates
cargo run -p ftx-cli --bin ftx-materials -- \
  thermal-transient tests/fixtures/materials/nist-7282-transient.json
```

`thermal-at` returns the effective resistance, areal heat capacity, common temperature
interval, complete immutable material snapshot and a portable locked selection. Without
`--allow-estimates`, inferred mass and fitted thermal functions are not admitted. The output
sets uncertainty propagation, empirical validation and comfort claims to false. It also works
for other complete scalar or tabulated catalog records, not just this source.

The transient example uses the actual NIST Aralite and Nomex III-Defender fits, but its area,
layer/interface arrangement, film resistances, temperatures and time schedule are **illustrative
user inputs**, not a measured experiment. Swap only a selected catalog ID to recompute transport.
The same `Selection` and compiled thermal laws reach the existing layer, surface and coverage
solvers. Mechanical simulation still refuses any required missing mechanical coefficients.

## Coverage

All IDs start with `nist/7282/`. Construction is `sheet` unless established more specifically;
a thermal barrier name does not establish weave, knit, laminate architecture or fiber fractions.

| ID suffix | Source's reported role | Complete effective thermal model |
| --- | --- | --- |
| aralite | Thermal liner | Yes, with estimate consent |
| breathe-tex | Moisture barrier | Yes, with estimate consent |
| breathe-tex-plus | Moisture barrier | Yes, with estimate consent |
| cotton-duck | Outer shell | No: conflicting specific-heat equations |
| nomex-e89-crosstech | Moisture barrier | No: conflicting specific-heat coefficient |
| neo-guard | Moisture barrier | Yes, with estimate consent |
| nomex-iii-defender | Outer shell | Yes, with estimate consent |
| nomex-iiia-pajama-crosstech | Moisture barrier | Yes, with estimate consent |
| pbi-kevlar-kombat | Outer shell | Yes, with estimate consent |
| scotchlite | Trim | No: inconsistent density/mass observations |

The tranche adds ten canonical records, bringing that seed to 51. Compatibility-imported
legacy entries remain additional discovery records. The seven complete thermal models are
**not seven fully measured mechanical/thermal garment materials**. They are empirical-source
fits plus explicitly derived effective mass, with unmodeled uncertainties and construction limits.

## Sources and normalization

Primary source, accessed 2026-10-14:

Lawson, J. R., Walton, W. D., Bryner, N. P., and Amon, F. K., *Estimates of Thermal Properties
for Fire Fighters' Protective Clothing Materials*, NISTIR 7282, June 2005.
https://nvlpubs.nist.gov/nistpubs/Legacy/IR/nistir7282.pdf

Main Tables 1 and 2 (printed page 4; PDF page 9) supply material identity, historic manufacturer,
reported role, mean thickness and bulk density. Tables 3 and 4 (printed page 14; PDF page 19)
supply conductivity and specific-heat equations, their R-squared values and separately printed
20 C values. These main tables were visually checked. Appendix A tables/equation labels were
available as extracted text but page rendering failed in this environment; conflicts found there
are retained as unresolved leads, **not silently corrected or claimed facsimile-verified**.

The private table adapter emits ordinary `Material` records through the existing catalog; there
is no alternative public material schema. It preserves original specific-heat coefficients in
J/(g K), then converts every coefficient by 1000 to J/(kg K). Polynomial powers use **Celsius**,
not Kelvin. Linear conductivity fits are in W/(m K). The closed ranges are 20..70 C for
conductivity (Table 3), 0..100 C for specific heat, and therefore 20..70 C for the combined law.
The report's broader introductory temperature discussion does not expand a fit's validity.

Areal mass is explicitly an **estimate** from Table 2 mean thickness times reported bulk density.
It is not a measured mass/area or the mean of paired density/thickness products. Their covariance
is unavailable. Material-group identity is retained rather than pretending that all measurements
used one identical coupon; exact batch identifiers and construction details remain unreported.
Source conditioning is mass equilibrium at 23 +/-3 C and 50 +/-10% relative humidity. Specific-heat
specimens were conditioned for more than six months; the DSC procedure used three specimens.
Effective single-layer conductivity was inferred from stacked-specimen measurements, not measured
directly on a single unconstrained sheet. The model holds reference mass and nominal geometry fixed;
pressure-dependent compression, moisture transport and thermal expansion are not characterized.

The source's method-level relative combined-standard / expanded uncertainties are retained:
conductivity 5.8% / 11.7%, specific heat 27% / 54%, density 5% / 10%; expanded values use coverage
factor 2. They are **not** coefficient covariance, sample standard deviation, hard bounds or computed
prediction intervals. R-squared is never repurposed as uncertainty. Rounded printed 20 C values
remain separate from equation evaluations; the latter are not adjusted to hit the rounded column.

Only attributed numerical facts and equations are retained, not source pages or images.

## Conflicting values are not simulation inputs

Cotton Duck's cubic term differs in sign between the main specific-heat table and Appendix A's
extracted table; the extracted figure equation also differs in the linear term. The appendix
reports an experimental 50 C value of 1.62 J/(g K) versus a calculated 1.32. Nomex E89's quadratic
coefficient is 7e-5 in the main table but 7e-6 in the extracted appendix table. Both specific-heat
laws are withheld. The printed main coefficients and diagnostics remain discoverable; no average,
preferred source variant or inferred correction is chosen.

Scotchlite's 81.6 kg/m3 density conflicts with the mass/geometry in the related NISTIR 6512
Table 2 (visually inspected): 55.0 g, 305 by 305 mm, 0.7493 mm thickness. That table itself
prints 0.0816 g/cm3. The reported density is preserved, but derived areal mass is withheld.
https://nvlpubs.nist.gov/nistpubs/Legacy/IR/nistir6512.pdf#page=10

`--allow-estimates` does **not** restore these missing inputs. Resolving them requires checking
facsimiles/errata and/or acquiring consistent measurements, not choosing the value that makes a
simulation run. The broader `ftx-tma`, `ftx-tme`, `ftx-tmf` and `ftx-tm2` empirical gates remain open.

## Numerical and validation boundary

Thermal series now support either observed points or a bounded polynomial of degree at most three,
never both. A polynomial cannot declare measured-knot evidence. The compiler checks endpoints and
stationary points for positivity, rejects near-cancellation, and refuses extrapolation. Its local
Simpson integral is algebraically exact for the supported degree; floating-point error remains.
The finite-volume solver continues to integrate specific heat against reference mass rather than
multiplying endpoint heat capacity by a temperature increment. Old point-series serialization and
arithmetic are unchanged, and snapshots distinguish polynomial storage without changing old receipts.

Tests independently transcribe source-table numerical answers, check withheld inputs and estimate
admission, compare integrated enthalpy with quartic primitives, exercise the actual CLI, and replay
locked source records. These establish data plumbing and numerical consistency, **not independent
held-out agreement with real layered coupons, garments or wearer heat loss**. Whole-wearer comfort,
thermal injury/PPE certification, flame resistance, machine process approval, and extrapolation to
cold weather are outside this implementation.
