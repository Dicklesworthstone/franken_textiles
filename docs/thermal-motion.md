# Thermal continuity across moving coverage maps

The `thermal::surface::coverage::motion` module transfers a temperature state between
geometry-derived coverage partitions without restarting the thermal simulation. It uses
the existing material laws, polygon clipping and layered finite-volume solver.

`Frame::new` computes correspondence from a frozen source surface, current cloth/body
positions, body triangles and a projection request. Its fields are private: an authored
polygon or a matching scalar area is not a geometrically verified remap stencil.
`initial` begins at an explicit temperature, `advance` performs a physical heat step,
and `remap` transfers a previous frame's state without advancing time. State carries
both the coverage identity and the actual mapped geometry identity. Equal area fractions
alone do not permit using a state from a different geometric frame.

## Conservative material-coordinate transfer

Cloth triangle identity and reference geometry remain fixed. The body may move or be
retessellated, cloth may move or stretch, the coverage partition may split or merge, and
the number of cells through each material's thickness may change. Each donor cell holds
a piecewise-constant material temperature. The transfer stencil is the intersection of
its cloth-barycentric polygon with a destination polygon, times the intersection of
their normalized through-thickness intervals.

For donor weights `w_i` and source-specific reference-area enthalpy `H(T)`, the new
temperature satisfies:

```
H(T_new) = sum(w_i * H(T_i)) / sum(w_i)
H(T) - H(T_ref) = areal_mass * integral(T_ref..T, Cp(u) du)
```

The existing temperature law integrates the declared piecewise-linear heat capacity.
Positive heat capacity provides a monotone inverse, bracketed by donor temperatures.
Each material is treated independently; a lining cannot donate its specific-heat law to
a shell. Temperatures are never interpolated directly when heat capacity varies with
temperature. Material-normal orientation maps the depth cells: viewing the opposite
side of a triangle reverses cell order instead of exchanging the material's two faces.

This is a **first-order conservative remap**, not a reversible interpolation. Merging
different donor temperatures mixes unresolved gradients. Refinement cannot recover
those lost gradients. Energy conservation and the donor maximum principle do not prove
spatial/time convergence, empirical fabric accuracy or garment comfort.

The integral-over-intersections approach is standard conservative-remapping background,
not a claim to implement the higher-order algorithms in these references:

- Dukowicz and Kodis, *Accurate Conservative Remapping (Rezoning) for Arbitrary
  Lagrangian–Eulerian Computations*, 1987, DOI 10.1137/0908037.
- Klima, Kucharik and Shashkov, *Combined swept region and intersection-based
  single-material remapping method*, 2017, DOI 10.1002/fld.4384.

## Accounting and refusal boundaries

Both donor and destination portions must be completely matched in material coordinates.
Equal total areas are insufficient: spatially disjoint domains refuse. The only allowance
is the projection's numerical area correction, bounded per portion by
`5e-10 * fraction + 64 * f64::EPSILON`. The actual reference-area residual is reported.
This tolerance is not permission to add a meaningful uncharacterized area.

Each source triangle has a declared enthalpy zero (its minimum donor temperature).
The transfer report retains those zeroes, before/after energies, signed residual and the
sum of absolute **per-triangle** residuals, so compensating material errors cannot hide.
The per-triangle acceptance is `2e-9 * (1 + |before| + |after|)` J; that is a numerical
guard, not measurement uncertainty. Remap error is not physical boundary heat.
Polygon, cell-intersection and thermal-law work share a bounded five-million-operation
budget, including characterization-table length. No partial state is returned on error.

Source observations/evidence policy, cloth triangle topology, reference areas and physical
layer identities must remain unchanged. Material substitution starts a new explicit
simulation; it does not reinterpret stored energy belonging to another fabric.

**The same spatial cloth domain must remain covered.** Regions of cloth may be unassigned
in both maps, but a previously unassigned portion has no modeled thermal history. Cloth
entering or leaving the modeled domain therefore refuses. An exposed-cloth thermal state
and appropriate boundary conditions are needed before supporting that transition. The
code never silently freezes, deletes, or resets the energy of entering/leaving cloth.

All the projection's geometric restrictions remain: outward, non-self-intersecting body
geometry is a caller precondition; ambiguous folds, grazing sheets and unresolved layer
order refuse. No air-gap law, moisture, compression, tissue storage, mechanical-work
heating, phase change or thermoregulating human model is added by remapping.

## Executable checks

`crates/ftx-drape/tests/thermal_motion.rs` covers independent nonlinear-enthalpy mixing,
body retessellation, moving hot cloth onto a colder body region, through-thickness
refinement/coarsening, material-side reversal, separate lining/shell laws, deterministic
repetition, uniform-state preservation, same-area/different-domain refusal, entering and
leaving material, changed-source/state refusal, and continued physical heat evolution.
All inputs are explicitly synthetic. Native tests and wasm compilation are separate
from browser execution parity and physical garment validation.
