# Textile-owned free liquid, evaporation and condensation

## Evaporation coupled to cloth motion

An optional `motion.transient_surface.mechanical_coupling` qualification connects the
initial-liquid `/1` model to the ordinary accepted mechanical steps. Its
`ftx-evaporating-liquid-mechanics/1` source binds every retained material instance and
explicitly qualifies the frozen membrane, bending and contact behavior over the stated
temperature and liquid-loading domains. No dry constitutive law is silently promoted to a
wet law. Supply, condensation, drainage, pinned cloth coordinates and a material that already
contains fixed retained-liquid inertia are outside this first coupling model.

The existing dry drape supplies the initial geometry and velocity. At motion time zero,
the declared liquid is an initial co-moving inventory; this is not a wet equilibrium or an
inferred deposition process. Each accepted interval first solves cloth and contact with its
current liquid mass, then evolves the existing thermal/evaporation model on the accepted
geometry, and finally removes that interval's co-moving vapor mass. The next mechanical
interval uses the reduced inertia and gravitational load. Source-triangle reference areas
own water, and the real seam weld map distributes its lumped mass exactly once. Attached
accessory mass remains separate and unchanged.

Co-moving outflow leaves the remaining cloth velocity unchanged. Its exported momentum,
bulk kinetic energy and gravitational potential energy are mechanical boundary terms,
separate from the thermal model's latent and sensible enthalpy. This is a first-order
sequential coupling: mechanical step refinement is required, especially near dryout, even
if the thermal substeps are already small. A failed thermal or mechanical interval refuses
the coupled motion and restores all accepted-state histories; it cannot fall back to a
successful dry trajectory. Uncoupled requests retain their existing independent heat
failure behavior. Numerical conservation and synthetic convergence do not establish wet
fabric behavior or real garment fit.

### Mechanical qualification fields

The optional `mechanical_coupling` object requires the following fields. Its source is
independent of the thermal/water qualification; choosing the same material alone does not
authorize frozen wet mechanics.

| Field | Required value or meaning |
| --- | --- |
| `schema` | `ftx-evaporating-liquid-mechanics/1` |
| `model` | `co_moving_evaporation_frozen_mechanics/1` |
| `material_report_sha256` | SHA-256 of `serde_json::to_vec(&run.report["material"])` from the ordinary selected-source drape, including all material assignments and unlike-sheet interfaces |
| `bindings` | Every retained instance exactly once, including dry instances and separate mirrored instances |
| `bindings.<instance>.material_sha256` | Exact normalized material hash used by that instance's water binding |
| `bindings.<instance>.maximum_liquid_loading_kg_m2` | Positive upper bound; must cover Water's entire declared `[0, maximum]` loading domain |
| `bindings.<instance>.temperature_range_c` | Inclusive `[minimum, maximum]` covering the entire authored Water pressure-table domain |
| `source` | `{reference, accessed_on, note}` identifying the independent mechanical qualification |
| `evidence`, `allow_estimates` | Existing evidence class and explicit estimate consent, separate from the material and water consent |
| `method`, `qualification` | Nonempty method and basis for co-moving evaporation with the exact frozen membrane, bending, contact and material-history laws |

Obtain the material report from the same ordinary drape before opting into this coupling;
hash its canonical parsed JSON using the Rust serialization above, not pretty-printed file
bytes. A changed assignment, material or interface requires a new exact qualification.
For material studies, each alternative must match its own actual source; one variant's
permission cannot be reused for a different material. The first and final motion samples,
each thermal trace row, and the final heat report retain the relevant mechanical inventory.
The native/portable integration example is
`evaporating_mechanical_water_replays_in_native_portable_and_named_case_decisions` in
`crates/ftx-cli/tests/accepted_body_workflow/surface_water.rs`; its numbers are deliberately
synthetic, with no implied textile qualification.

## Thermal water model

`motion.transient_surface.surface_water` evolves explicitly declared **free liquid on the textile**
together with the existing independent-sheet temperature history. Version 1 starts from an initial
inventory. Version 2 also admits independently sourced delivery rates and incoming liquid temperatures.
Version 3 adds independently qualified condensation and re-evaporation, including incoming vapor
sensible energy and latent release.
Evaporation removes latent energy from that textile/liquid system, so cooling changes subsequent
vapor demand. Water remains attached to its source material triangles as the cloth moves.

This is a conditional numerical model. No repository material is automatically qualified for this
coupling. The request must cite evidence for the liquid/boundary inputs and for retaining the exact
selected material's frozen dry thermal law and effective vapor resistance across the specified
temperature and water-loading domain. The dry law can retain its independently characterized
temperature dependence. This does not identify absorption, sorption, capillary flow,
changing wet conductivity, physiology or real garment performance.

## Authored request

Add an optional `surface_water` object to `ftx-motion-surface-heat/1`. The same typed scenario passes
through the native and browser material-scenario workflows; no separate portable solver exists.
The following lists required fields and their meaning. Numbers are supplied by the caller, never
defaulted from fiber names, a saturation formula or a different measured conditioning state.

| Field | Contract |
| --- | --- |
| `schema` | `ftx-textile-surface-water/1` for initial-only liquid; `/2` for prescribed supply; `/3` for supplied liquid and qualified reverse phase transfer |
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
| `replenishment_kg_m2_s` | Version 1 requires exactly zero; versions 2/3 omit this field and require `supply` |
| `condensation` | Version 3 requires the independently qualified reverse-transfer object described below; versions 1/2 forbid it |
| `source` | Existing `{reference, accessed_on, note}` provenance object |
| `evidence` | `measured`, `published` or `estimate` for the explicit coupling inputs |
| `allow_estimates` | Independent opt-in required for `estimate`; never relaxes a material selection's evidence policy |
| `method` | Method for the fluid and pressure inputs |
| `qualification` | Source basis for retaining locked dry thermal/Ret observations throughout the full temperature/loading domain |

The selected materials still need their ordinary source-qualified dry heat capacity, areal mass,
thermal resistance and `vapor_resistance`. Constant properties and the canonical `thermal_response`
conductivity/specific-heat laws are accepted. The latter retain their exact source domains and
integrated enthalpy; a temperature-dependent material cannot fall back to a scalar average.
A material substitution changes its hash and requires new coupling qualification; the old binding
cannot silently authorize a new specimen.

### Temperature-dependent textile storage and transport

All three water versions use the existing nonlinear dry thermal authority when the selected material
has a tabulated or polynomial temperature law. No extra conductivity or heat-capacity fit is created.
The coupling qualification must support retaining those exact profiles over the declared liquid
loading range; dry characterization alone does not establish wet-property invariance.

For reference dry mass `md` in the inner cell and remaining liquid mass `m1`, the step's storage law is

```text
Hcombined(T1) - Hcombined(T0)
  = md * integral(Cp_dry(T), T0..T1) + m1 * cp_liquid * (T1 - T0).
```

The source-preload inverse uses this strictly increasing combined enthalpy for either heating or
cooling. It brackets against the applicable measured temperature endpoint and refuses an unsupported
preload instead of extending the source law. The same implicit finite-volume solver evaluates
conductivity at the solved cell temperatures. The liquid term appears only in the inner cell;
every other cell retains its dry enthalpy. Dry storage in reports removes the added liquid term,
so liquid storage, supplied/condensed enthalpy and latent exchange still reconcile separately.
At exact dryout the remaining capacity is zero and the ordinary dry law advances the rest of the
interval. The frozen dry material/model identity remains unchanged throughout.

Variable-law liquid histories reserve the existing 129 cell-operation allowance per cell and
thermal substep, covering the bounded enthalpy inversion. Constant-only histories keep their
previous work accounting. A late domain, inversion or budget failure rolls back the whole accepted
mechanical interval, including water, temperature and source counters.

Material/scenario comparison reconstructs final dry storage from the same source integrals. Sampled
temperature ranges bound each material's enthalpy inside its own qualified domain; `Cp(Tfinal)*ΔT`
is not admitted as a substitute. This enables the existing native, portable and browser scenario
consumers without changing the water request schemas or inventing a wet constitutive law.

### Prescribed liquid supply

Versions 2 and 3 require a `supply` object with schema `ftx-textile-liquid-supply/1`. Its `bindings` map
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
surface film or a body reservoir. Without the separately qualified `mechanical_coupling`
option above, water does not add mechanical mass.

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
temperatures must remain inside the raw table's qualified domain. Versions 1 and 2 refuse wet pressure
below ambient or a post-step crossing into condensation. Every version refuses unrepresentable
mass/time/capacity or a budget failure. A smaller `max_step_s` can resolve numerical overshoot;
it does not create missing evidence.
The whole accepted mechanical interval stages its temperature, liquid and source counters together
and publishes no partial heat state on failure. The mechanical result and other consumers survive.

## Condensation and subsequent re-evaporation

Version 3 explicitly admits reverse exchange in the same retained free-film model. Set
`schema: "ftx-textile-surface-water/3"`, keep the independently qualified version-2 `supply`
(including explicit zero rates when no liquid is delivered), and add:

```json
"condensation": {
  "schema": "ftx-textile-condensation/1",
  "vapor_temperature_c": [40.0, 10.0],
  "source": {
    "reference": "test:synthetic-reversible-film",
    "accessed_on": "2026-10-27",
    "note": "Invented software example; no measured fabric or vapor qualification"
  },
  "evidence": "estimate",
  "allow_estimates": true,
  "method": "Prescribed incoming vapor temperatures in two mechanical intervals",
  "qualification": "Synthetic reverse use of the exact locked Ret, phase transfer onto wet and dry patches, and constant latent plus cp_liquid effective sensible enthalpy over the full declared pressure, temperature and liquid-loading domains"
}
```

There must be one vapor temperature for every mechanical keyframe interval, inside the same qualified
temperature domain. Reverse-transport evidence and estimate consent are independent of both the base
film and liquid supply. A forward Ret observation or a material name does not establish reverse
permeation, nucleation on a dry textile, or validity of this enthalpy approximation. The source must
justify those assumptions for the exact frozen material and its declared domain. Versions 1 and 2
reject this extension; removing or failing its qualification does not silently enable condensation.

The canonical fixed Ret network supplies a signed flux from the initial substep temperature:
positive pressure difference evaporates available liquid; negative pressure difference condenses
incoming vapor onto that same triangle. Current area controls transfer, while reference area owns
the resulting liquid. Water can condense onto an initially dry patch and later evaporate when the
boundary pressure changes. This is an independent-sheet inner-cell film, not a prediction of dew
location inside an arbitrary garment layer stack.

Let `a` be condensed mass, `Tv` the explicit incoming vapor temperature and retain the earlier symbols.
The inventory becomes `m1 = m0 + s + a - e`. The full balance is:

```text
ΔH_dry + c [m1 (T1 - Tref) - m0 (T0 - Tref)]
       + e c (T0 - Tref) + e L
  = E_inside - E_outside + Q + s c (Ts - Tref) + a c (Tv - Tref) + a L
```

The existing implicit heat equation therefore receives remaining liquid capacity and source
`Q - e L + s c (Ts - T0) + a c (Tv - T0) + a L`. Both incoming sensible energy and latent release
change the solved textile temperature. The effective convention uses the already supplied **liquid**
sensible coefficient for both vapor enthalpy labels and a constant latent heat; it does not derive a
real gas heat capacity or temperature-dependent latent heat. That approximation requires explicit
qualification over the requested domain. Changing the common enthalpy reference only relabels the
storage/flow terms and does not change computed temperatures.

Version 3 uses `ftx-textile-surface-water-result/3`. It retains every version-2 field and adds
`condensed_liquid_kg`, `condensation_latent_energy_j` and `incoming_vapor_sensible_energy_j` to every
sample and the final receipt. Final patch rows retain condensed mass and incoming sensible energy.
Existing `latent_energy_j` remains nonnegative **evaporative removal**; condensation release is kept
separately. The mass residual is `remaining + evaporated - initial - supplied - condensed`, and the
full energy residual subtracts incoming vapor sensible energy and condensation latent release.

Nominal comparison metrics `textile_water_condensed_mass_kg` and
`textile_water_condensation_latent_energy_j` use the same admitted complete history. Admission checks
exact sources, per-patch inventory, mass/energy totals and interval-temperature-weighted incoming
enthalpy. No statistical bounds, unsampled-time guarantees or empirical condensation rates are
inferred. Maximum liquid loading still refuses transactionally; there is no invented runoff,
drainage, absorbed-water law or mechanical water mass. Refinement remains necessary when pressure
reverses within a thermal step.

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
energy are available; supplied-mass admission requires matching version 2/3 receipts and independent
schedule reconstruction. These comparisons do not propagate water or thermal bounds.

The separate `vapor_coverage_history.wet_film` belongs to a prescribed **body** reservoir. Its latent
energy is never deducted from this textile film. A request that enables both specifies two separate
water owners; this feature does not transfer water between them or assert that their experiments
are physically compatible. There is no inferred water delivery, absorption/sorption, in-plane transport,
wet-dependent material law, mechanical mass change, thermoregulation or comfort/fit claim.

Analytic tests check sampled-temperature sensible export, reference invariance, rate feedback,
time refinement, exact dryout, dry remainders, incoming sensible heat, supply-limited evaporation,
rewetting, condensation onto dry textile, latent warming, subsequent re-evaporation,
reference/current area separation and transactional overflow/source/domain refusals.
Moving sewn-garment tests exercise thermal cooling, reference-area delivery, Maxwell heat, frozen replay and
independent mechanical results. The portable scenario-study regression uses the same actual runtime
and verifies source/missing-data refusals and manufacturing independence. All these inputs are
explicitly synthetic numerical tests, not measured fabric/fluid qualification.
