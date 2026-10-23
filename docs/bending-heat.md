# Irreversible bending work as a qualified heat source

The existing permanent-curvature law can supply sensible heat to either prescribed-motion thermal
consumer. This is an explicit, one-way conversion of accepted irreversible work. It does not infer a
conversion efficiency from a fabric name, count reversible bending storage as heat, or change the
mechanical response with temperature.

## Material qualification

Select the existing `orthotropic_curvature_return_map/1` bending-plasticity model and supply its
complete source-qualified normal-curvature tensor, yield, hardening and validity observations.
Each active material additionally needs `properties.bending_plastic_heat_fraction` when bending heat
is requested. An explicit zero is supported; an absent observation is not a zero conversion.

| Observation field | Required meaning |
| --- | --- |
| `value`, `unit` | Dimensionless fraction in `[0,1]`, normalized to `1`; `%` is accepted. |
| `bounds`, if present | Both normalized endpoints must also lie in `[0,1]`. |
| `source`, `method`, `evidence` | The ordinary material observation requirements; estimates require the selection's explicit consent. |
| `conditions.specimen_id` | Exactly the specimen identified by the consumed bending tensor and history observations. |
| `conditions.conditioning` | Exactly the same characterized material state as those observations. |
| `conditions.work_basis` | `irreversible_bending_plastic_dissipation`. |
| `conditions.temperature_min_c`, `temperature_max_c` | Explicit, finite, ordered, inclusive material-temperature domain; neither endpoint may be below absolute zero. |

`ResolvedMaterial::bending_plastic_heat_fraction()` performs this qualification. Mechanical-only
consumers do not require the optional observation. A material without an active permanent-curvature
law contributes no bending heat and returns `None`. All source data remain in the frozen material
identity; the conversion does not replace the tensor, grain, preferred curvature or reference metric.

## Motion request

Set this option in either or both existing `motion.transient_surface` and
`motion.transient_coverage` requests:

```json
"bending_plastic_dissipation_heat": "uniform_reference_thickness"
```

The existing temperature, interval-boundary, geometry and work policies remain explicit. Independent
sheets use their current exchange area and original reference mass. The coverage consumer allocates
the same source-triangle energy across covered portions and exposed remainder; bare body receives no
internal energy. Its conservative remap moves material enthalpy without adding elapsed time or heat.

The option can coexist with separately qualified `plastic_dissipation_heat` and
`relaxation_dissipation_heat`. Each source retains its own work and conversion receipt. Their supplied
energies enter the thermal balance once, through the existing sensible-enthalpy transport and inversion
path, including source-defined temperature-dependent storage when present.

## Work and conservation

For source triangle `i`, the accepted permanent-curvature update supplies

```text
delta_work_i = reference_area_i * sum_axis(yield_axis * abs(delta_plastic_curvature_axis))
delta_heat_i = qualified_fraction_i * delta_work_i
```

Recoverable elastic and hardening storage are excluded. A six-node reconstruction is integrated over
its source triangle's reference area; overlapping stencil vertices do not multiply the work or deposit
heat in neighboring materials. `Solver::bending_plastic_dissipation_j()` returns cumulative joules in
retained triangle order, including explicit zeros for inactive triangles. Mechanical history receipts
also retain each active patch's `plastic_dissipation_j`. Failed mechanical attempts roll back both
memory and work.

The heat history starts after the initial drape. It records and subtracts the accepted time-zero
baseline, so sewing and initial-drape dissipation are not reheated during motion. Work increments are
spread uniformly over the accepted interval and through its reference-thickness cells. The accepted
geometry is the thermal interval's right endpoint, as for the existing motion heat solvers.

Both thermal consumers share one captured `bending_plastic_dissipation_j` array per accepted sample.
Captured scalars count against the existing motion sample budget. Each consumer independently checks
its source identity, conversion domain and thermal budget; a refused thermal history publishes no
partial result and does not erase complete mechanics or a successful other consumer. Temperature
domains are checked at initialization and before and after every thermal substep, including explicit
zero conversions.

The `bending_plastic_dissipation_heat` receipt reports motion-only `plastic_dissipation_j`,
`supplied_heat_j`, `unconverted_dissipation_j`, and the excluded `initial_plastic_dissipation_j`, plus
the exact qualified material observations. Unconverted work is reported without assigning an
unmeasured destination. Comparisons reconcile these ledgers against retained initial and final source
patches and every accepted work array before admitting heated temperature or energy objectives.

## Limits and regression evidence

This path excludes seam plasticity, contact/friction heating, damage, moisture, thermal activation,
temperature feedback to mechanics, thermoregulation and empirical garment performance claims.
No observed textile data are added by the implementation. The regression fixtures are explicitly
synthetic. Canonical material tests exercise fraction units, evidence consent, specimen matching and
temperature domains. Real sewn-motion tests check reference-area energy and temperature balances,
inactive triangles, covered/exposed conservation, initial-work exclusion, multiple independent
sources, single capture across both consumers, deterministic replay and independent thermal refusal.
