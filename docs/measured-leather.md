# Measured bovine leather: tanning, finish and sampling region

The built-in catalog adds ten study sample groups from Kopitar, Zuvela Bosnjak, Akalovic
and Skenderi (2022), [DOI 10.1177/15280837221077048](https://doi.org/10.1177/15280837221077048).
Each `zagreb/leather-2022/` identifier distinguishes the paper's treatment and neck/back
sampling region. Synthetic tanning is a treatment of bovine hide, not synthetic leather.
Supplier, hide identity, commercial lot and component mass fractions were not reported.

The adapter transcribes 40 numerical properties from the publisher's Tables 4–5:
thickness, dry thermal resistance, reported effective conductivity and water-vapor
resistance. Standard deviations and coefficients of variation are retained by name in
each relevant quantity's conditions, in their stated units. They are not uncertainty
bounds, confidence intervals or a qualified sewing tolerance. Source rounding is kept;
SD and CV are not forced to agree. Only numerical facts are bundled, with attribution.
Article text, figures and the source PDF are not redistributed.

## What can run now

These observations use the existing `Material`, normalization, frozen selection,
`ThermalStack` and `ConductionLaw` paths. Changing the treatment or sampling region
changes dry heat-flow comparisons without modifying physics code. Measured resistance
takes precedence over reconstructing a resistance from rounded thickness/conductivity.
Thickness is available to the existing manufacturing checks. Vapor resistance is
available to discovery and screening; this addition does not implement moisture transport.

Thickness follows ISO 2589:2016. The paper does not numerically state its pressure or
sample count, so it is not unloaded loft or a machine-foot compression curve. The dry
ISO 11092:2014 test uses air at 20 °C, 65% RH and 1 m/s. The vapor test instead uses
35 °C, 40% RH and 1 m/s. Resistance means use three specimens; that count is not assigned
to thickness. Specimen preconditioning duration and moisture content are unreported.

The source provides one thermal protocol, not a temperature-dependent law or validity
interval. Existing scalar consumers retain their constant-property semantics. Their use
at other conditions is an approximation unsupported by these measurements. Missing
areal mass, heat capacity, membrane/bending laws and interface friction remain missing:
these records do not become transient-thermal-ready or drape-ready materials. Numerical
propagation tests do not establish garment fit, outdoor comfort or machine qualification.

## Try a controlled dry comparison

Inspect the exact treatment and source conditions:

```bash
cargo run -p ftx-cli --bin ftx-materials -- show zagreb/leather-2022/cr-d-h-neck
```

For `ftx-materials thermal leather-stack.json`, use:

```json
{
  "layers": [{"id": "zagreb/leather-2022/cr-d-h-neck"}],
  "area_m2": 0.6,
  "inside_c": 35.0,
  "outside_c": 20.0,
  "boundary_resistance_m2_k_w": 0.08
}
```

The area, face temperatures and boundary resistance are explicit illustrative inputs.
For these fixed inputs, substituting `syn-d-h-neck`, `cr-d-h-neck` and `cr-d-h-back`
gives approximately 81.08, 85.71 and 90.91 W respectively. This exercises the constant
dry comparison; it is not a reproduction of the paper's hotplate experiment or a
prediction of total human heat loss. The three tests in `material_catalog/leather.rs`
cover this consumer propagation, source replay and capability-specific missing data.
