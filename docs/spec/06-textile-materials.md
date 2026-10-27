# Textile materials — common data and resolution contract

Status: initial implementation, 2026-10-13. This extends the comprehensive plan's fabric input (§1.1)
and drape material field (§11); it is not evidence of measured commercial-fabric physics or real fit.

## One authority, with honest coverage

`ftx-pattern::materials` owns `ftx-textile-catalog/1` records so the drape, exports, CLI and browser can
consume the same data without depending on a physics crate. No additional crate or runtime dependency.
The design follows frankensim's distinction between material cards, physical consumers and uncertain
inputs; no frankensim source code is copied. A material is a specimen/product variant, not just a fiber
name. Fiber, yarn/thread, woven, knit, nonwoven/felt, hide, fill and laminate are different record levels.
Construction descriptors identify spinning, weave/knit, finishes, tanning, thickness condition and SKU.
A blend's dry mass fractions are not a justified homogenization law. Yarn/fiber/fill records cannot be
passed to the shell solver even when someone supplies similarly named numbers.

Every numeric property carries a canonical unit, a source key, a method, evidence class, optional bounds,
and explicit test conditions. Sources carry a reference, access/measurement date and a note. Published
supplier specifications are not independently measured. Estimates require explicit opt-in. Unknown is an
absent property, not zero; a misspelled material identifier never falls back to a cotton-like default.
The catalog supports additional attributes without pretending that every consumer understands them.

## Mechanical consumer vocabulary

| Property key | Unit | Meaning |
|---|---|---|
| `areal_density` | `kg/m2` | Dry fabric mass per undeformed area, including its stated finish |
| `warp_stiffness`, `weft_stiffness` | `N/m` | Effective membrane moduli; wale/course axes for knits |
| `poisson_warp_weft` | `1` | Major Poisson ratio, with reciprocal nu21 = nu12 E2/E1 |
| `shear_stiffness` | `N/m` | Engineering in-plane shear modulus |
| `bending_rigidity` | `N*m` | Effective sheet bending rigidity, independent of membrane modulus |
| `thickness` | `m` | Full sheet thickness at the recorded compression/conditioning state |
| `friction_skin`, `friction_self` | `1` | Interface-specific coefficients at recorded conditions |

Moduli, mass and thickness must be positive, bending/friction nonnegative, and the orthotropic stiffness
positive definite. Constituent bulk Young's modulus in Pa is NOT accepted as sheet stiffness in N/m.
The solver retains its existing grain rotation and gathered-seam compression field. This first effective
law is not a nonlinear yarn/loop homogenizer, hysteresis law, damage law or anisotropic bending model.

The extensible property map also admits test-qualified tensile/tear/cut resistance, abrasion cycles,
flammability results, ignition and decomposition observations, thermal conductivity/resistance, vapor
resistance, air permeability, absorbency, shrinkage, colorfastness, UV performance, yarn tex/denier,
twist, thread counts, stitch geometry, fill power and loft. These are observations, not universal
certifications. A cut-resistance force needs its test standard; a burn-test class needs its jurisdiction,
method and conditioning. Unknown coatings/finishes never imply laser-safe or child-sleepwear-safe.

Direct tensile-test observations use `tensile_maximum_force` and `tensile_force_at_break` in N,
with original kgf normalized by exactly 9.80665 N/kgf. `tensile_displacement_at_break` is the
reported instrument displacement in m, not local strain. Source-axis labels and specimen geometry
stay in conditions; neither force becomes a warp/weft stiffness or strength per unit width.
`reported_thickness` is inspectable gauge thickness in m and does not supply the separately
qualified `thickness` required for contact or sewing clearance.

## Thermal consumer

`ThermalStack::evaluate` computes dry one-dimensional steady conduction through series layers, with
explicit area, inner/outer temperatures and a combined boundary-film resistance. It prefers tested
`thermal_resistance` in `m2*K/W`, otherwise requires `thickness` in `m` and `thermal_conductivity` in
`W/(m*K)`. It never derives warmth from fiber name, fill power or a marketing label. The output is signed
heat flow in W, not a comfort index, clo certification, temperature rating or survival guarantee.
Wind, sweating, evaporative transport, air-gap geometry, layer compression, transient storage, seams,
thermal bridges, metabolism and thermoregulation remain separate, unimplemented coupling work.

## Dated purchasing and finite observed stock

`ftx-textile-quotes/1` may bind an offer to `material_sha256`, the exact resolved material
snapshot used for its width and specimen identity. A supplied lock must match before any price
conversion; a same-ID material edit cannot inherit that offer. Legacy observations without the
optional lock retain their existing ID-based behavior and do not establish snapshot qualification.
The optional `available_quantity` is the supplier's observed capacity in the quote's original
`unit`, not packages or the area of pattern pieces. It is finite and nonnegative; absence means
unknown capacity. The complete minimum/increment-rounded order must fit a stated capacity.
Neither these dated observations nor their hashes reserve inventory or guarantee stock at checkout.

Price scopes and cut-procurement markets may set `require_observed_quantity: true` to exclude
unknown capacity; omission preserves legacy observational pricing. Select the latest observation
in the exact market before checking expiry, availability, snapshot identity or quantity. A later
insufficient or mismatched observation must not revive an older offer. Eligible offers retain
whether quantity sufficiency was observed, and excluded latest observations retain their reason.
Marker procurement must keep total cost unknown when any material has no eligible whole-order
offer. Missing stock numbers never mean infinite stock or zero cost.

## Verification

Synthetic unit tests cover anisotropy, explicit estimate admission, missing fields, dimensional mistakes,
non-finite inputs, constitutive stability, zero friction/bending, unsupported material levels, blend mass
balance, source references/bounds, duplicate catalog IDs and a known-answer series thermal stack. These
are software/physics checks, not measurements of any commercial fabric. Existing parity paths must remain
unchanged unless a material is explicitly selected.

## Prescribed pressure conditioning

`ftx-material-compression/1` prepares an ordinary, content-locked `Selection` through
`material_compression::condition`; it does not add a parallel material or contact solver.
The request owns the original material, sources, fixed specimen/conditioning/loading branch,
pressure table and target Pa. Positive loft is nonincreasing on the declared branch. Exact knots
retain authored observations; in-domain interpolation is an explicitly admitted estimate.

Every retained scalar must be observed in the complete pressure-row keyset or have an explicit
source-qualified invariant covering the entire table. An original fixed-pressure observation must
also lie inside that invariant range; malformed or conflicting pressure qualifiers refuse.
Sampled areal mass is prohibited, and retained reference mass is unchanged. No bulk-density/loft
mass reconstruction, conductivity mixing or thickness-derived stiffness is inferred. At most one
thermal resistance/conductivity authority survives. Unqualified scalar properties, response blocks
and `*_model` selectors are removed and named in the receipt; missing consumer inputs remain missing.

The report's `selection_json` is the exact portable selection text; display-layer numeric
reserialization must not replace it. Reserved `compression_` structure markers identify the prescribed
state and prevent reconditioning its incomplete result. Existing unloaded-stack sewing checks refuse
these selections until an appropriate pressure-qualified process path exists. This establishes neither
dynamic pressure/loft coupling nor moisture, recovery or empirical garment performance.
See [Pressure-conditioned materials](../pressure-conditioned-materials.md) for the runnable native and
browser workflow, full qualification rules and synthetic known answers.
