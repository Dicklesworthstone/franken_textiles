# Measured wet-denim states at 1000 Pa

Six built-in selections supply directly reported thickness and thermal resistance at fixed moisture
states. They can be used as layers in existing steady thermal requests. Selecting another state changes
the material's effective resistance and the resulting heat flow without changing the solver.

## Primary source

Muhammad Mushtaq Ahmed Mangat (2012), *The Effect of Moisture and Finishing on Thermal Comfort and
Selected Mechanical Properties of Denims with a Portion of Synthetic Fibres*, PhD dissertation,
Faculty of Textile Engineering, Technical University of Liberec. The declaration is dated 2012-02-12.
The [full primary university PDF](https://dspace.tul.cz/bitstreams/57667674-5146-46c9-a46a-6aaeabbbedd0/download)
was reviewed on 2026-10-19. Its SHA-256 is
`385938cb5baa6dc1044196ac057b5427d09f8c6ea12abe5e97846ce78a9da994`.
Only attributed numerical observations and their conditions are bundled; the dissertation and images
remain outside the repository.

Table 4-9 spans printed and PDF pages 90-93. Its header explicitly states the **Alambeta 1000 Pa
setting** and reports thermal resistance in m² K/W, conductivity in W/(m K), and thickness in mm.
The six rows below are the first and last observations of three reported series. The table describes
means and supplies no per-row coupon identifiers, replicate counts, numerical uncertainties, washing
treatment IDs or lots. These are study observations and do not qualify an arbitrary retail denim.

All IDs begin with `tul/mangat-2012/`.

| ID suffix | Water wet-mass fraction µ | Thickness (mm) | R (m² K/W) | Reported k (W/(m K)) | Source page |
|---|---:|---:|---:|---:|---:|
| `denim-cotton/mu-0.012` | 0.012 | 0.86 | 0.0128 | 0.067 | 91 |
| `denim-cotton/mu-0.447` | 0.447 | 0.67 | 0.0032 | 0.209 | 92 |
| `denim-spun-pp/mu-0.017` | 0.017 | 0.91 | 0.0149 | 0.0609 | 90 |
| `denim-spun-pp/mu-0.557` | 0.557 | 0.79 | 0.0048 | 0.187 | 90 |
| `denim-pet/mu-0.045` | 0.045 | 0.78 | 0.0108 | 0.072 | 92 |
| `denim-pet/mu-0.400` | 0.400 | 0.67 | 0.0040 | 0.168 | 93 |

Warp is cotton; the suffix names the weft. Section 4.5, page 83, restricts subsequent analysis to
3/1 Z twill. Cotton/synthetic dry mass fractions are unreported for these rows and remain unfilled.
The cotton/cotton fiber composition excludes absorbed water.

**Moisture uses the wet-mass denominator.** Equation 2-39 and its explanation on pages 47-48 define
`µ = (wet fabric mass - dry fabric mass) / wet fabric mass`. It is dimensionless and is stored as
`water_mass_fraction_wet_basis`. It is neither dry-mass moisture regain nor ambient relative humidity.
No conversion or interpolation is performed.

Section 3.5, page 71, reports laboratory temperature as `20-22` without printing its unit and relative
humidity as 20-25%. The records preserve this omission instead of supplying a temperature unit or
plate-temperature interval. Page 89 describes the two plates limiting surface evaporation. The
observations therefore do not supply a free-air drying or evaporative heat-transfer law.

## Consumer authority and use

```sh
ftx-materials show tul/mangat-2012/denim-cotton/mu-0.012
ftx-materials show tul/mangat-2012/denim-cotton/mu-0.447
```

Use either ID as an ordinary material selection in the steady thermal workbench. The canonical
`thermal_resistance` is the transport authority for `ConductionLaw` and `ThermalStack`, including with
`allow_estimates: false`. The separately rounded conductivity and thermal absorptivity remain extension
observations (`reported_thermal_conductivity` and `reported_thermal_absorptivity`). They are never used
to reconstruct resistance or heat capacity. The scalar resistance retains the existing constant-property
consumer semantics; no measured temperature interval is asserted.

Every state carries `compression_model: "fixed_pressure_observation/1"`. The shared sewing gate
therefore refuses its wet, loaded thickness as uncompressed stack clearance. The marker also prevents
these already conditioned observations being silently supplied as an original material to
`pressure-condition`. Table 4-2's separate 200/1000 Pa thickness observations are not joined to these
rows: that table does not identify their moisture, finish or matching coupon. There is no pressure law,
moisture law, recovery curve or mass-invariance declaration.

No areal mass, heat capacity, membrane response, bending or friction is supplied for these wet states.
Transient heat storage and drape stay unqualified. Tests in `tests/wet_denim_catalog.rs` verify original
units, moisture basis, source conditions, frozen state identity and the loaded-state restriction. They
also exercise both thermal consumers using independently transcribed resistances and explicitly chosen
numerical boundary conditions. This verifies data integration, not garment comfort or wearer validation.
