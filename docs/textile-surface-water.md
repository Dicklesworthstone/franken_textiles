# Textile-owned free liquid and evaporative cooling

`motion.transient_surface.surface_water` evolves explicitly declared **free liquid on the textile**
together with the existing independent-sheet temperature history. Version 1 starts from an initial
inventory. Version 2 also admits independently sourced delivery rates and incoming liquid temperatures.
Evaporation removes latent energy from that textile/liquid system, so cooling changes subsequent
vapor demand. Water remains attached to its source material triangles as the cloth moves.

This is a conditional numerical model. No repository material is automatically qualified for this
coupling. The request must cite evidence for the liquid/boundary inputs and for retaining the exact
selected material's constant dry thermal and effective vapor resistance across the entire specified
temperature and water-loading domain. This does not identify absorption, sorption, capillary flow,
changing wet conductivity, physiology or real garment performance.

## Authored request

Add an optional `surface_water` object to `ftx-motion-surface-heat/1`. The same typed scenario passes
through the native and browser material-scenario workflows; no separate portable solver exists.
The following lists required fields and their meaning. Numbers are supplied by the caller, never
defaulted from fiber names, a saturation formula or a different measured conditioning state.

| Field | Contract |
| --- | --- |
| `schema` | `ftx-textile-surface-water/1` for initial-only liquid; `/2` for prescribed supply |
| `model` | `inner_cell_equilibrium_free_film_frozen_properties/1` |
| `bindings` | Map containing every retained instance name, including mirrored instances separately |
| `bindings.<instance>.material_sha256` | Exact normalized source material hash already used by mechanics and dry heat |
| `bindings.<instance>.initial_liquid_kg_m2` | Nonnegative kg per **reference** material m²; use explicit zero for dry instances |
| `maximum_liquid_loading_kg_m2` | Positive upper end of the qualified loading domain; lower end is zero |
| `liquid_specific_heat_j_kg_k` | Positive constant liquid sensible capacity |
| `latent_heat_j_kg` | Positive constant latent energy relative to liquid at the sampled temperature |
| `enthalpy_reference_c` | Explicit common reference for liquid storage and exported sensible enthalpy |
| `wet_vapor_pressure_pa` | 2–128 `[temperature_c, pressure_pa]` knots; temperature strictly increases, pressure is nonnegative and nondecreasing |
| `outside_vapor_pressure_pa` | Nonnegative ambient pressure for each mechanical keyframe interval |
| `inside_film_resistance_m2_pa_w` | Positive declared vapor film before the material Ret |
| `outside_film_resistance_m2_pa_w` | Positive declared vapor film after the material Ret |
| `replenishment_kg_m2_s` | Version 1 requires exactly zero; version 2 omits this field and requires `supply` |
| `source` | Existing `{reference, accessed_on, note}` provenance object |
| `evidence` | `measured`, `published` or `estimate` for the explicit coupling inputs |
| `allow_estimates` | Independent opt-in required for `estimate`; never relaxes a material selection's evidence policy |
| `method` | Method for the fluid and pressure inputs |
| `qualification` | Source basis for retaining locked dry thermal/Ret observations throughout the full temperature/loading domain |

The selected materials still need their ordinary source-qualified dry heat capacity, areal mass,
thermal resistance and `vapor_resistance`. This first slice accepts **constant** dry thermal properties.
Temperature-dependent dry laws refuse explicitly. A material substitution changes its hash and requires
new coupling qualification; the old binding cannot silently authorize a new specimen.

### Prescribed liquid supply

Version 2 requires a `supply` object with schema `ftx-textile-liquid-supply/1`. Its `bindings` map
contains exactly the same instance names as the top-level water bindings, including dry/zero-feed
instances. Each entry has `rate_kg_m2_s` and `temperature_c` arrays, one value per mechanical keyframe
interval. Rates are nonnegative kilograms per **reference material square metre per second**;
temperatures describe the incoming liquid, not a replacement textile temperature. Every incoming
temperature must lie inside the raw qualified temperature domain. Rates and temperatures remain
constant within their declared interval while accepted mechanical samples and thermal substeps
can subdivide that interval.

The supply also declares its own `source: {reference, accessed_on, note}`, `evidence`,
`allow_estimates` and `method`. Estimate consent is independent of both the free-film qualification
and material selection. The source must describe delivery of the same qualified liquid. A feed
does not imply sweat, rain interception, absorbed moisture, transfer from the body reservoir or an
empirically characterized wet textile. Those connections need separately supported models.

For example, the supply portion of an explicitly synthetic two-interval request can be:

```json
{
  "schema": "ftx-textile-liquid-supply/1",
  "bindings": {
    "front": {"rate_kg_m2_s": [0.00001, 0.00002], "temperature_c": [20, 25]},
    "back": {"rate_kg_m2_s": [0, 0], "temperature_c": [20, 20]}
  },
  "source": {
    "reference": "test:prescribed-liquid-feed",
    "accessed_on": "2026-10-23",
    "note": "Synthetic delivery to demonstrate the request; not measured fluid or textile data"
  },
  "evidence": "estimate",
  "allow_estimates": true,
  "method": "Artificial piecewise-constant reference-area delivery"
}
```

## Discrete physical model

Each independent material triangle retains its initial liquid mass as loading times reference area.
Its free liquid shares the **inner cell-centre temperature**. It is not an independently solved outer
surface film or a body reservoir. There is no extra mechanical water mass.

At a thermal substep's initial temperature, interpolate the authored wet-pressure table without
extrapolation and ask the canonical qualified Ret network for potential mass flux. Multiply that
flux by the current triangle area. The earliest positive `remaining_mass / evaporation_rate` over
all wet triangles limits the substep, alongside the existing thermal maximum step. At a dryout event,
remove exactly the remaining water; the remaining physical time still advances through dry heat.
No unmet demand evaporates from a dry triangle.

With version 2, the supplied mass rate is the authored rate times reference triangle area; current
area still controls potential evaporation. A wet triangle's depletion time becomes
`remaining_mass / (evaporation_rate - supply_rate)` when evaporation exceeds supply. At zero stored
liquid, actual evaporation is limited to the smaller of potential demand and supply. Excess delivery
rewets that triangle. This continuous, well-mixed discrete closure can sustain evaporation after
dryout without inventing water. A step that would exceed the qualified maximum reference-area loading
refuses the complete accepted interval; it never clips the inventory or invents overflow/drainage.

For an accepted substep, let `m0` and `m1` be old and remaining liquid masses, `e = m0 - m1`, `c` the
liquid specific heat, `L` the latent heat and `Tref` the explicit sensible reference. Vapor mass and
exported vapor sensible energy both sample `T0`, the initial inner-cell temperature. The conserved
balance, including independently requested mechanical heat `Q`, is:

```text
ΔH_dry + c [m1 (T1 - Tref) - m0 (T0 - Tref)]
       + e c (T0 - Tref) + e L = E_inside - E_outside + Q
```

The two liquid sensible terms reduce to `m1 c (T1 - T0)`. Therefore the existing implicit thermal
solver temporarily receives **remaining** liquid capacity in the inner cell and signed source
`Q - e L`. Its source material, dry capacities and public dry model identity stay unchanged. This
also keeps a dryout remainder on the original dry heat authority. Qualified exterior convection/
radiation and independently selected membrane/Maxwell/bending heat can coexist with the film.

For version 2, let `s` be the supplied mass and `Ts` its incoming temperature during the substep.
Then `m1 = m0 + s - e`, and the right-hand side of the same full balance additionally contains
`s c (Ts - Tref)`. The remaining-capacity heat equation receives the signed source
`Q - e L + s c (Ts - T0)`. This mixing term changes the actual computed textile temperature;
incoming enthalpy is not merely added to a report. The final and every retained sample separately
report supplied mass and sensible enthalpy, and the full energy residual subtracts that incoming
enthalpy. Changing the common sensible reference changes storage/flow labels but leaves temperatures
and the complete energy balance invariant.

This is an explicit mass-transfer / implicit heat discretization. Halving thermal step size is
needed to assess accuracy; energy conservation is not a convergence certificate. All material-cell
temperatures must remain inside the raw table's qualified domain. Wet pressure below ambient, a
post-step crossing into condensation, unrepresentable mass/time/capacity, or a budget failure
refuses. A smaller `max_step_s` can resolve numerical overshoot; it does not create missing evidence.
The whole accepted mechanical interval stages its temperature, liquid and source counters together
and publishes no partial heat state on failure. The mechanical result and other consumers survive.

## Reports and scope

The available transient heat result retains the full policy at `request.surface_water` and a
`surface_water` receipt containing:

- `initial_liquid_kg`, `remaining_liquid_kg`, `evaporated_liquid_kg` and `mass_balance_error_kg`;
- `latent_energy_j`, `exported_vapor_sensible_energy_j` and `liquid_stored_change_j`;
- `dryout_events` and the combined `balance_error_j`;
- `unit_pressure_networks`, the canonical source-qualified material networks used for transport;
- `patches`, including source triangle/instance identities and explicit zero entries for dry triangles.

Version 2 uses result schema `ftx-textile-surface-water-result/2` and additionally retains
`supplied_liquid_kg` and `supplied_liquid_sensible_energy_j` in every sample, the final receipt and
each final patch row. Its mass residual is `remaining + evaporated - initial - supplied`; its energy
balance includes the supplied sensible term described above. Version 1 retains its original schema
and fields and cannot silently enable a feed.

Each retained thermal sample includes cumulative water and energy quantities. The existing top-level
`stored_change_j` remains **dry textile sensible storage**. With surface water enabled, top-level
`balance_error_j` includes liquid storage, vapor sensible export and latent energy as shown above.
Temperatures and boundary fluxes are actual results of that coupled equation. Nominal transient-surface
comparisons admit matching source-qualified histories after canonical material/Ret replay, retained
patch/reference ownership, cumulative mass/latent/enthalpy consistency and the full source/boundary
balance. Sampled temperature and boundary metrics plus textile evaporated/remaining mass and latent
energy are available; supplied-mass admission requires matching version 2 receipts and independent
schedule reconstruction. These comparisons do not propagate water or thermal bounds.

The separate `vapor_coverage_history.wet_film` belongs to a prescribed **body** reservoir. Its latent
energy is never deducted from this textile film. A request that enables both specifies two separate
water owners; this feature does not transfer water between them or assert that their experiments
are physically compatible. There is no inferred water delivery, absorption/sorption, in-plane transport,
wet-dependent material law, mechanical mass change, thermoregulation or comfort/fit claim.

Analytic tests check sampled-temperature sensible export, reference invariance, rate feedback,
time refinement, exact dryout, dry remainders, incoming sensible heat, supply-limited evaporation,
rewetting, reference/current area separation and transactional overflow/source/domain refusals.
Moving sewn-garment tests exercise thermal cooling, reference-area delivery, Maxwell heat, frozen replay and
independent mechanical results. The portable scenario-study regression uses the same actual runtime
and verifies source/missing-data refusals and manufacturing independence. All these inputs are
explicitly synthetic numerical tests, not measured fabric/fluid qualification.
