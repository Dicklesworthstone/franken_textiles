# Attached trim and accessory mass

`fabric-pattern/1` can carry an optional `attached_mass` extension. It assigns the sourced total
mass of retained trims, fasteners, ornaments or other extras to explicitly named physical cloth
vertices. The same inventory feeds complete assembly accounting and the actual sewn drape.

The existing mass vector supplies both static gravitational load and implicit time-step inertia.
An attached group therefore changes the garment's mechanical response during sewing, draping and
prescribed body motion. The solver does not infer the accessory's shape, stiffness, contact surface,
thermal storage or attachment strength from its name or weight.

## Input

The following is a synthetic software example, not measured costume hardware:

```json
{
  "attached_mass": {
    "schema": "ftx-attached-mass/1",
    "groups": [
      {
        "extra": {
          "id": "closure-set",
          "mass": {
            "value": 0.03,
            "unit": "kg",
            "bounds": [0.028, 0.032],
            "evidence": "estimate",
            "source": "test",
            "method": "Synthetic total retained group"
          },
          "sources": {
            "test": {
              "reference": "test:attached-mass",
              "accessed_on": "2026-10-20",
              "note": "Synthetic software input"
            }
          },
          "allow_estimates": true
        },
        "targets": [
          {"instance": "front.R", "vertex": "hem_outer", "fraction": 0.25},
          {"instance": "front.L", "vertex": "hem_outer", "fraction": 0.75}
        ]
      }
    ]
  }
}
```

The named instances and vertices must exist in the pattern. Each `extra` is one total retained group
for one assembly, using the existing complete-assembly `Extra` schema. Here the entire closure set
weighs 0.03 kg; the two physical locations receive 0.0075 kg and 0.0225 kg. Neither the number of targets
nor a piece's cutting quantity multiplies this total.

An extra must represent matter absent from the pattern's physical cut sheets. A lining already
represented by a cut piece must not also be entered as an extra. Separate IDs describe separate
physical groups; the software cannot determine that two differently named observations describe the
same hardware.

Mass is expressed in kilograms, preserving the existing assembly accounting contract. Dynamics
requires a finite positive nominal value, a retained source with nonempty identity fields, a method,
valid reported bounds and explicit consent for an estimated mass. Bounds are retained in the receipt;
this point-mass model uses the admitted nominal value. It does not turn source bounds into simulated
statistical uncertainty. Unknown mass (`null`) and denied estimates remain unknown in accounting and
refuse a physical attachment run.

## Exact physical placement

Targets name the expanded physical instance and an existing boundary vertex. For a mirrored cut,
use the full instance names such as `front.R` and `front.L`; an authored piece name does not stand in
for both. An unfolded instance's vertex names include its actual expansion suffixes. The program
does not pick a nearest point, infer a mirror placement, invent new cloth geometry or silently discard
an unsimulated target.

Each group has one or more unique instance/vertex targets. Fractions lie in `(0, 1]` and must sum to
one within summation roundoff. The original fractions are retained, never renormalized. Groups have
unique nonempty IDs, with limits of 1,024 groups, 4,096 targets per group and 65,536 targets overall.

The source boundary corner is resolved through the actual garment mesh and seam weld map. Two
different named corners joined by a seam can resolve to one solver vertex. Their declared shares add
at that vertex; the group total is still allocated once. A target with no retained triangle support,
a missing or unsimulated corner, a stale name, an invalid fraction or unrepresentable arithmetic
refuses before a bound mass vector is published.

## Material substitution and physics

Sheet mass is first assembled from the resolved material of each retained triangle and its reference
area. The accessory shares are then added to the existing lumped vertex masses. For a free degree of
freedom this mass appears in both the static load `m g` and the implicit inertial term
`m (x - x_predicted) / h²`. The existing elastic and contact laws remain authoritative for the cloth.
Elastic-casing grip accounting uses the resulting total mechanical weight.

Rebuilding the scene with a different source sheet material recomputes its sheet mass, constitutive
response and other existing material consumers, then reapplies the original extra groups and named
targets. Pattern material studies preserve the extension when substituting material selections.
Changing an attached source's mass is an explicit input change and creates a different input hash.

The optional binding keeps the assigned mass vector, reference coordinates and retained membrane
elements. Changing any of those through a low-level mutable model makes step, static solve, prescribed
motion and attachment-receipt publication refuse. Rebuild the scene to change a bound model's sheet
materials; `set_materials` cannot silently erase or reuse accessory mass. Binding and motion updates
are transactional, including refusal after internally accepted motion samples.

Thermal surfaces continue to use the original sheet materials and reference triangle areas. An extra
can change heat exchange indirectly by changing the solved cloth geometry, but its mass does not
become invented cloth heat capacity. Modeling an accessory's heat storage, conduction or thermal
contact needs separately specified physical geometry and thermal properties; this extension supplies
none of those.

## Shared assembly accounting

`ftx_pattern::material_mass::Extra` is the shared source authority, re-exported from
`ftx_export::assembly_mass::Extra` for existing consumers. Assembly mass evaluation merges the
pattern-owned groups with its request's independent `extras`:

- A group that occurs only in the pattern is included once.
- A request extra with the same ID and identical complete typed observation is included once.
- A repeated ID with different mass, bounds, conditions, source identity or estimate policy refuses.

The original request remains visible as `request`. When nonempty attachments are present, the optional
`attached_inventory` field retains their typed sources and explicit targets. The existing pattern
hash includes the original extension. `inventory_complete` retains its existing meaning: the caller
declares that the enumerated cut sheets and merged extra inventory describe the complete assembly.
The presence of attachments does not imply completeness.

Unknown extras remain unknown rather than zero. Complete assembly mass is not available when a
required observation is unknown or the caller has not declared completeness. Accounting and dynamics
have different sheet scopes: assembly accounting includes its explicit retained cut areas, while
drape inertia includes the triangles retained by the simulation mesh. They must not be compared as
though seams, omitted pieces and other geometric differences vanished.

## Drape receipt

Existing `report.mass_kg` remains the mass of simulated cloth sheets. With nonempty attachments,
`report.attached_mass` contains:

| Field | Meaning |
|---|---|
| `schema` | `ftx-attached-mass-report/1` |
| `input`, `input_sha256` | Original extension and its canonical JSON identity, including sources and policies |
| `cloth_mass_kg` | Sum of the pre-attachment simulated sheet masses |
| `attached_mass_kg` | Sum of admitted total extra group masses |
| `total_inertial_mass_kg` | Actual sum of the mass vector consumed by the mechanical solver |
| `mass_balance_residual_kg` | Actual total minus cloth mass and source group masses |
| `groups` | Group masses, reported source bounds, allocation totals/residuals and every target mapping |

Each target reports `source_piece`, `instance`, `vertex`, original `mesh_vertex`, welded `solver_vertex`,
`fraction` and allocated `mass_kg`. Floating-point allocation and summation residuals are numerical
accounting fields, not source uncertainty estimates. Absent attachments and an empty group list keep
the original mass path and omit both optional reports.

## Validation and limits

Focused synthetic regressions cover source admission, mirrored physical names, exact shared-source
accounting, welded aliases, material substitution, static weight balance, implicit inertia, actual
sewn dynamics, source-sheet thermal storage, deterministic replay, prescribed motion and rollback,
and refusal of stale low-level state. They establish the stated software/mechanics behavior. They do
not establish empirical fit, seam or fastener safety, rigid accessory dynamics, distributed trim
bending, accessory/body contact or wearer warmth.

## Fixed retained liquid on a textile

A material can independently select
`structure.mechanical_mass_model: "dry_textile_plus_fixed_retained_liquid/1"`.
This adds a uniform, source-qualified `retained_liquid_areal_density` to an explicitly dry
`areal_density`, once per reference textile area. Both observations normalize to kg/m²; authored
g/m² values and bounds remain in the frozen source record. The dry observation must exclude this
liquid. An observed total wet density cannot be supplied as the dry term and added again.

This is a **closed, co-moving mechanical state**. Liquid stays attached to the same material area,
has the same velocity as the cloth, and neither enters nor leaves during sewing, draping or motion.
No variable-mass momentum term is therefore omitted. The model does not simulate sorption,
capillary migration, drainage, evaporation, condensation or liquid sloshing. Those require a joint
mass, momentum and thermal integrator; the existing changing surface-water history must not be
combined with this fixed mass as if it already provided that coupling.

The mass observations require these source conditions:

| Observation | Required conditions |
|---|---|
| Both | Identical nonempty `specimen_id` and `conditioning`; `reference_area_basis: "dry_textile_reference_area"` |
| `areal_density` | `mass_basis: "dry_textile_excluding_retained_liquid"` |
| `retained_liquid_areal_density` | `mass_basis: "additional_retained_liquid"`; `retention_basis: "closed_co_moving_no_exchange"`; nonempty source `qualification` describing this retention regime |

The liquid value is nonnegative, the dry value positive, and both use the selection's normal
evidence and estimate policy. The sum must be representable without losing either positive term.
Reported total endpoints are available only when **both** source observations provide complete
physical bounds. Missing liquid bounds do not imply zero uncertainty or reuse the dry interval.

Mechanical admission additionally requires every consumed scalar, directional/nonlinear curve,
Maxwell coefficient and network qualification to name that same `specimen_id`, `conditioning`,
and `retained_liquid_kg_m2` (a string containing the exact SI nominal loading). This is explicit
qualification of the wet mechanical response. The software does not copy a dry modulus, bending
law, thickness or friction coefficient and assume wetting leaves it unchanged. Synthetic examples
may deliberately use equal wet and dry coefficients, with explicit estimate admission, to isolate
the inertial effect in a numerical test.

The shared material resolver supplies the total to the ordinary triangle mass assembler. Piece
selection, mirroring, grain and seam welding retain their normal behavior; different wet panels
contribute their own reference-area shares at a shared welded vertex. Static gravity, implicit
inertia, elastic-casing weight, timed clamped strips and prescribed body motion all use this same
mass. Accessory groups remain separate and are added once afterwards. Material substitution
rebuilds the mass from the new frozen source observations without changing simulation code.

`material.retained_liquid_mass` in the uniform drape receipt (or the corresponding per-piece record)
retains separate dry, liquid and total densities and complete reported bounds. Timed-strip reports
add optional `retained_liquid`; their `reference_mass_kg` includes both terms. Complete assembly
accounting uses the same density on each actual retained cut area and adds the same optional receipt
to each `PieceMass`. Simulation and cutting still have different geometric scopes when allowances,
cutouts, omitted panels or simulation scales differ; identical reference areas use identical mass.
Roll and finite-stock gross mass use the same selected-state density on their declared gross areas,
including waste. Material comparison protocols retain the mass model and liquid acquisition method
and conditions, so incompatible dry/wet regimes or liquid protocols need explicit mixed-protocol
consent. Sewing requires its thickness observation to qualify the same loading, independently of
whether a membrane law exists. Static strip consumers apply the same wet-state admission while
preserving the existing membrane-only capability for ordinary dry records.

The current dry steady/transient thermal and vapor consumers refuse this selected fixed-liquid
state. Its mechanical loading does not supply liquid heat capacity, wet conductivity, changed Ret,
latent storage or a thermal boundary. Mechanical results remain usable while those capabilities
are unavailable. A fully coupled liquid thermal model needs its own matched state and evidence.
Without the selector, existing material, output and mass behavior remain unchanged.
The existing pressure-conditioning transform also refuses this selected regime: it may not strip
the mass model, discard retained liquid or infer that compression preserves its loading.

Direct synthetic regressions in the existing `attached_mass` and `assembly_mass` suites check
normalization, source and loading mismatches, denied estimates, whole reported bounds, actual
welded allocations, static weight balance, dynamic momentum balance, material substitution, real
motion continuation, timed-strip replay and independent thermal refusal. These tests do not qualify
the wet behavior of a real textile.
