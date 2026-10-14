# Material-platform implementation and acceptance map

This implements parts of comprehensive plan §25 and its existing beads `ftx-tm0`–`ftx-tm8`.
The roadmap already includes the broader fiber/yarn/fabric/fill hierarchy, nonlinear construction
response, material-specific contact, layered transport, live purchasing and machine-aware design.
The following are shipped software paths, not closure of the parent empirical/coupling gates.

| Bead | Implemented here | Still required before closing the parent |
|---|---|---|
| `ftx-tm2` | Source-bearing finite-strain swatch identification, held-out checks, immutable fitted selection, CLI binding and actual sewn-band material-only dynamics test. See `material-calibration.md`. | Independent measured construction-specific linen, leather and knit specimens; directional bending, contact and condition-qualified validation. Synthetic observations are not those specimens. |
| `ftx-tm5` | Dry constant-property multilayer transient heat/storage model with explicit interfaces and films, numerical energy checks, shared material selections and CLI histories. See `material-thermal.md`. | Draped air-gap extraction, measured compressed loft, moisture/evaporation, wind films, thermal bridges, coupled validation and calibrated wearer boundary conditions. |
| `ftx-tm8` | Fixed-geometry swatch-force substitution changes real cloth dynamics; source-qualified insulation substitution changes the existing dry heat-flow consumer. | Real linen/leather/performance-knit fixed-jacket comparisons, heterogeneous contact, native/wasm execution parity and independent fit/thermal measurements. |

The first calibration implementation passed GitHub Rust checks in run 38490928739 at commit
`36fda86908696c5ac30db9457ccd3872e3ee6d81`. Subsequent workbench and dry-thermal commits have focused
local tests on the recovered pinned toolchain; their latest combined-head CI status is separate.

The factual expansion in `insulation-data.md` adds six exact manufacturer insulation designations with
thermal resistance, mass, pressure-qualified thickness and source dates. Unknown specific heat,
directional constitutive response, friction and process safety remain missing. These entries are usable
for the existing static thermal comparison, not automatically for cloth mechanics or transient storage.

Priority remains measured construction-specific material coverage plus heterogeneous assembly/contact,
not more unqualified property columns or a brand-name shortcut to constitutive parameters. Keep actual
specimen/garment validation separate from synthetic known answers, numerical conservation and provenance
hashes. Machine execution remains a separate, explicit and interlocked hardware boundary.
