# Temperature-dependent textile transport

Implemented 2026-10-14. This extends the shared `Material` / `ResolvedMaterial` system;
there is no second physics catalog and no network access in the solver.

## Run the complete software example

```sh
cargo run -p ftx-cli --bin ftx-materials -- \
  thermal-transient tests/fixtures/materials/synthetic-variable-thermal.json
```

The fixture is **synthetic**, not a cotton, leather, down or performance-fabric measurement.
It contains a fully specified, source-labelled temperature law and companion geometry.
The first five-second step has a cell temperature of about 10.769593 C. In the retained
native execution the largest step heat-balance error was below 1.1e-8 J. These are
software known answers, not real-world accuracy or thermal-protection claims.

The same material selection works through `DrapeRun::thermal_surface`, the thermal
exchange attached to a drape report, saved-snapshot replay, and the existing thermal
surface adapter. Editing only the selected thermal observations changes heat transport
without changing solver code. A changed selection invalidates the old surface state;
the engine does not silently reuse temperatures belonging to another material.

## Source-bearing observations

An optional `thermal_response` on a material identifies an exact `specimen` and a
`conditioning` state. Its independently optional `conductivity` and `specific_heat`
series contain canonical units, source keys, method, test conditions and ordered
`points` with `temperature_c`, `mean` and optional `standard_deviation`.

The distinction between a reported observation and a constitutive model is explicit:
measured knots remain measured, but adopting the piecewise-linear interpolant requires
`allow_estimates: true`. Sample standard deviation is retained as variability, **not**
converted to a confidence interval, uncertainty of the mean, probability distribution
or solver tolerance. The current calculation uses the means; uncertainty propagation
remains unimplemented. Duplicate temperatures, wrong units, nonpositive means,
nonfinite numbers, conflicting scalar/table properties and missing sources refuse.

For a complete transient layer, provide areal density and either resistance or matching
thickness/conductivity, plus specific heat. Every companion scalar of a tabulated
response must explicitly carry the same `conditions.specimen` and
`conditions.conditioning`. Fiber composition, nominal retail loft and a parent's
unconditioned dimensions do not establish those facts. A partial record remains
searchable even when a simulation consumer refuses it.

## Actual data added

The offline catalog now adds five FSRI specimen/conditioning records containing 25
reported temperature-specific means, each with its published standard deviation:

| Record family | Conditioning | Conductivity points | Specific-heat points |
| --- | --- | ---: | ---: |
| Cotton sheet | Unconditioned | 2 | 4 |
| Cotton sheet | Dried | 2 | 4 |
| Cotton batting | Unconditioned | 2 | 4 |
| Cotton batting | Dried | 2 | 0 |
| Overstuffed-chair polyester batting | Unconditioned | 5 | 0 |

Sources, accessed 2026-10-14:

- https://materials.fsri.org/materialdetail/cotton-sheet
- https://materials.fsri.org/materialdetail/cotton-batting
- https://materials.fsri.org/materialdetail/overstuffed-chair-polyester-batting
- Source metadata and apparatus organization: https://github.com/ulfsri/fsri_materials_database

Attribution: McKinnon, M., Weinschenk, C., Dow, N., DiDomizio, M., and Madrzykowski, D.,
*Materials and Products Database* (Version 1.0.0), Fire Safety Research Institute,
UL Research Institutes, 2023. The records retain dated factual transcriptions; this
repository does not redistribute the source pages or images or assert a redistribution
license for those works. Reported zero standard deviations remain rounded published
zeros, not exact certainty.

**None of these five records is yet a complete garment-simulation input.** Matching
conditioned HFM geometry/areal mass was not acquired, and dried cotton batting and the
polyester batting have no specific-heat series on the retained pages. No missing
quantity is filled using another specimen, conditioning state or generic fiber value.
The catalog has 41 canonical seed records after this tranche; compatibility-imported
legacy entries are additional discovery records, not additional validated specimens.

## Numerical contract

For reference-area mass `m`, the stored heat increment is

```
Delta H = m * integral(T_old..T_new, Cp(T) dT)
```

The integral is exact for the declared piecewise-linear interpolant and is evaluated
segment-locally to avoid subtracting large absolute enthalpies near equilibrium.
It is not approximated by `m * Cp(T_new) * (T_new - T_old)`.

The existing finite-volume network evaluates effective resistance `thickness/k(T)`
at cell centres and joins half-cell paths in series with explicitly declared interface
and boundary-film resistances. Backward Euler uses positive enthalpy secants and a
fixed-order tridiagonal solve. Damped iteration is bounded at 256 updates; acceptance
checks the original nonlinear local equations, the global energy balance and the
maximum principle. Nonconvergence or overflow refuses rather than publishing a partial
or unconverged state. Refine both spatial cells and time steps to establish numerical
accuracy; stability alone is insufficient.

All state and reservoir temperatures must lie inside the common intersection of the
characterized property and layer intervals. This is deliberately conservative: no
extrapolation, clamping of authored inputs, assumed phase change or inferred wet-state
law is allowed. Even a uniform initial surface is checked against that interval.

On a moving triangle, resistance is per **current area**, while integrated sensible
heat follows **reference mass**. The geometry-scaled time step implements
`Delta H_ref / dt = (A_current/A_reference) * net_flux`. Stretching therefore changes
exchange area without creating material or stored heat. The closure does not infer
compression, altered thickness, air-gap topology or mechanical-work heating.

Legacy all-constant models retain their original arithmetic and snapshot format.
Temperature-dependent models use `ftx-dry-thermal-model/2`, retain the complete source
records and domain, and do not publish misleading fixed cell capacities/resistances.
Saved surface snapshots rebuild the laws and compare the complete input receipt.

## Evidence and remaining acceptance gates

Direct checks cover an independent one-cell quadratic enthalpy answer; the integrated
Fourier flux and spatial refinement of a temperature-dependent conductivity profile;
heterogeneous and mixed constant/tabulated stacks; boundary reversal; reference-mass
surface conservation; atomic refusal; hash-locked replay; and a real sewn-drape software
path whose only edit is the thermal record. Native tests and wasm32 compilation are
not measured textile validation or native/wasm execution-parity evidence.

Next product gates are matching raw HFM geometry/conditioning and repeatability;
independent measured layered swatches and sewn garments; pressure/loft and moisture
laws; explicit shell/lining/body-contact topology; thermal uncertainty propagation;
and measured mechanical/thermal linen-to-leather-to-knit substitution on the same
pattern. Whole-wearer warmth, wind, sweating, metabolism, radiant exchange, flammability,
PPE certification and automatic cutting/sewing safety remain outside this model.
