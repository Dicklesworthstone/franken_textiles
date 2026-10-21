# Material-selected elastic bands in garment casings

A casing's cloth and its inserted elastic tape are different physical components. The existing casing
rings can now take stiffness from an independently selected **finished band or cord** rather than from
one global chosen EA. This works for a single drafted waistband, multiple declared casings, a joint
outfit, and the existing study/motion paths. No new numerical elastic law is introduced.

## Select a band without editing the pattern's cloth

A band uses the ordinary catalog `Selection`: exact ID, optional embedded record, source-bearing
overrides, independent `allow_estimates`, and optional `expected_sha256`. Its material must declare
`structure.elastic_model: "linear_tension_only/1"` and a positive `axial_stiffness` quantity in N
(or explicitly convertible kN). The complete band selection, source records, original units, bounds,
conditioning and evidence policy survive normalization and reporting.

`axial_stiffness` is force per unit **engineering strain of the finished band** with its actual width
and cross-section. It is not a sheet stiffness in N/m, a bulk modulus in Pa, or a breaking force. No
cross-section, blend rule, or coefficient is inferred from the fiber name. A cord can be admitted
without inventing sheet bending, density or contact properties. Individual fibers and loose fill do
not qualify as characterized bands.

For an existing drafted single-waistband pattern with `meta.solved.elastic_cut_mm`, put the band
selection in `meta.solved.elastic_material`. The original cut-minus-overlap rest length and pattern
geometry remain unchanged. For an explicit multi-casing declaration, each entry carries its own
optional `elastic_material`:

```json
{
  "instance": "waistband",
  "relaxed_m": 0.6,
  "load_instances": ["waistband", "front.R", "front.L", "back.R", "back.L"],
  "elastic_material": {
    "id": "my-characterized-band",
    "allow_estimates": false
  }
}
```

This illustrates a catalog selection, not a built-in band ID or garment recipe. The names, length and
material record must come from the actual pattern/specimen. The explicit list lives at
`meta.solved.elastic_casings` and cannot also carry legacy `elastic_cut_mm` or a root-level band
selection. Unknown/missing material data, null selections, mismatched snapshot locks and unapproved
estimates refuse; they do not restore a default EA. An estimate in the band needs the band's own
permission, regardless of whether the casing fabric permits estimates.

The committed `tests/fixtures/materials/synthetic-elastic-band.json` is an **entirely synthetic**
15 N tape record, not an approximation for any named commercial elastic. To exercise the software
using a single-waistband pattern and a new output file:

```sh
python3 - <<'PY'
import json
with open('trousers.json') as f:
    pattern = json.load(f)
assert 'elastic_cut_mm' in pattern['meta']['solved']
with open('tests/fixtures/materials/synthetic-elastic-band.json') as f:
    pattern['meta']['solved']['elastic_material'] = json.load(f)
with open('trousers-with-band.json', 'x') as f:
    json.dump(pattern, f, indent=2)
PY
```

Run `trousers-with-band.json` through ordinary `ftx drape` with the same admitted body, measurements,
fit specification and numerical settings. The example is numerical software evidence only.

## Interaction with the global option and joint garments

A positive `DrapeOptions.elastic_ea` / `--elastic-ea` value remains the fallback for **unselected**
casings. It neither multiplies nor overrides a selected band's sourced stiffness. Zero remains the
explicit global disable switch: declarations/evidence are still validated, but no rings or cut-length
restoration are performed. An invalid band cannot be concealed by switching elastics off.

Each band's total EA is split between the existing fold and seam-line rings; it is not multiplied by
the number of rings or by the number of mesh segments. Each casing uses its own band. Body-contact
friction still belongs to the casing cloth, not an invented exposed-band surface. The existing independent
ideal-ring grip estimates use that casing's simulated load domain; they are not integrated contact forces
or a sum of outfit support capacities.

An explicit `load_instances` scope counts only the retained cloth triangles and accessory allocations
owned by those physical instances. A seam weld shares motion, not source ownership: an accessory
targeted at a neighboring panel does not become part of this load merely because both targets weld to
one solver vertex. A group's mass is apportioned by its declared target fractions, not charged in full
to every casing touching any target. Each explicit `elastics` row includes `load_mass.cloth_kg`,
`load_mass.attached_kg`, and `load_mass.inertial_kg`; the last supplies the nominal weight comparison.
The low-level `loaded_vertices` list describes geometric support only, not permission to sum entire
vertex masses. These reference-mass quantities are assembled once with the scene and retained through
motion; rebuild the scene when material density or attachment inputs change.

Declared load instances must be active and placed, even when the elastic switch is zero. Missing
simulated loads cannot silently become a lighter garment. Full-load scopes retain the ordinary
inertial mass, and disjoint source scopes partition it even when their boundary vertices coincide.
Changing only a load scope changes the ideal-ring comparison, not force transmission, cloth dynamics
or the body's actual support reactions. Overlapping scopes are separate comparisons, not additive
support capacities; neither source mass accounting nor a `holds` value establishes real retention.

The joint composer freezes each selected band into its namespaced casing entry. Changing only a band
therefore changes the composed-pattern identity while leaving source pieces, seams and sheet/contact
material identities unchanged. The saved `outfit_pattern.json` replays the same band laws through ordinary
drape; no live catalog lookup is required by those frozen selections. Returned `elastic` (legacy) or
`elastics` (explicit) rows retain `band_material`, including its source snapshot, and `ea_basis` alongside
the EA actually used by the solver. Unselected legacy records and outputs retain their previous behavior.

## Physical and verification boundary

This is the existing regularized, tension-only **linear EA model**, not nonlinear rubber elasticity,
cyclic recovery, stress relaxation, damage, band failure or a measured safe extension range. Its slack-to-
taut smoothing remains the existing numerical model; source selection does not calibrate it. Specimen
conditions and reported bounds are retained, not enforced as runtime strain/rate domains or propagated
as force uncertainty. The caller must establish that this model is applicable to the intended motion.

Band mass, geometry, friction, heat storage and manufacturing consumption are NOT inferred from stiffness.
Use the separate source-qualified `attached_mass` allocation when its mass must enter gravity/inertia;
do not double count a band already represented by a physical cut piece. This feature adds no automatic
band procurement, sewing qualification, hardware authorization or empirical garment-fit claim.

Tests cover dimensional normalization, independent evidence policy, stale locks, exact legacy numerical
identity, independently sewn casings, material-only force/motion changes, body-motion continuation, and
real native command publication/replay with identical cache/mesh outputs. Plan §11.2 and §25.2–25.3 /
`ftx-tm4` still require matched real band data and richer construction-dependent laws for empirical use.
