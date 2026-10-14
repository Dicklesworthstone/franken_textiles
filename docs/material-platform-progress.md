# Material-platform implementation and acceptance map

This implements parts of comprehensive plan §25 and its existing beads `ftx-tm0`–`ftx-tm8`.
The roadmap already includes the broader fiber/yarn/fabric/fill hierarchy, nonlinear construction
response, material-specific contact, layered transport, live purchasing and machine-aware design.
The following are shipped software paths, not closure of the parent empirical/coupling gates.

| Bead | Implemented here | Still required before closing the parent |
|---|---|---|
| `ftx-tm2` | Source-bearing finite-strain swatch identification, held-out checks, immutable fitted selection, CLI binding and actual sewn-band material-only dynamics test. See `material-calibration.md`. | Independent measured construction-specific linen, leather and knit specimens; directional bending, contact and condition-qualified validation. Synthetic observations are not those specimens. |
| `ftx-tm3` | Typed material schema (`ftx-material/1`), evidence/source/bounds discipline, immutable `ResolvedMaterial` with SHA-256 snapshot digest, per-piece pattern resolution (`ftx_pattern::textile_import`), and `ftx material list\|show\|validate\|compare`. See `material-catalog.md`, `piece-materials.md`, `insulation-data.md`. | Closed (`FTX-26c`); ongoing specimen additions belong to `ftx-tm2`. |
| `ftx-tm4` | Work-conjugate Green-strain nonlinear membrane law (`ftx_pattern::material_response::TabulatedMembrane`, monotone cubic and piecewise-linear interpolation, warp/weft coupling stability check, strict domain refusal) wired into `ftx-drape` triangle mechanics (`crates/ftx-drape/tests/material_drape.rs`). | Directional warp/weft/bias bending curves, cyclic loading/unloading hysteresis and permanent set, and measured construction-specific specimen validation. |
| `ftx-tm5` | Dry constant-property multilayer transient heat/storage model with explicit interfaces and films, numerical energy checks, shared material selections and CLI histories. See `material-thermal.md`. | Draped air-gap extraction, measured compressed loft, moisture/evaporation, wind films, thermal bridges, coupled validation and calibrated wearer boundary conditions. |
| `ftx-tm6` | Dated, source-locked `QuoteBook` evaluation (`ftx_pattern::material_cost`), marker-driven independent order pricing (`ftx_export::cutting::procurement`), `ftx material quote`, `ftx cut-plan --quotes`, and `scripts/refresh_textile_prices.py`. See `textile-price-refresh.md`. | Live multi-supplier acquisition adapters with raw source archival across >=2 regions. |
| `ftx-tm7` | Grain-constrained single-ply roll marker planning per material snapshot (`ftx_export::cutting::CutPlan`), stock width/length/area/mass/utilization accounting, SVG preview (`cutting::preview`), and `ftx cut-plan` (`crates/ftx-cli/tests/cut_plan_workflow.rs`). | Seam/needle/stitch/bulk process-planning rules and interlocked hardware/cutter qualification. |
| `ftx-tm8` | Fixed-geometry swatch-force substitution changes real cloth dynamics; end-to-end 92 cm bolero substitution across `linen_185`, `lambskin_080` and `cotton_elastane_jersey_210` plus 6-insulation thermal stack comparison (`crates/ftx-cli/tests/material_substitution_gate.rs`). | Independent physical linen/leather/performance-knit garment and thermal specimen measurements. |

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
