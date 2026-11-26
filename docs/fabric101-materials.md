# Fabric-101 measured material metadata

The built-in catalog includes **96 additional released Fabric-101 specimens**, with **192 measured
observations**: areal density and reported test thickness for each exact specimen. These records work
with the existing material selection, source freezing, mass accounting and scalar screening paths.
They bring the complete bundled catalog to **371 records**.

The source is Gong, Mao, Yang, Lu, Wang and Komura, *Learnable Persistent Wrinkle Formation in Cloth
Simulation*, [arXiv:2609.13707v1](https://arxiv.org/html/2609.13707v1), and the authors'
[Fabric-101 data release](https://github.com/GongDeshan/Fabric_101_for_Wrinkles/tree/510519be0657a341a1a84fdb5c24be156ac83c9f).
The release was checked on 2026-10-27. Its README still said “Coming Soon,” but the pinned data tree
contained the specimen files used here.

## What can be selected

Every ID starts with `fabric101-2026/`; the suffix is the original release ID lowercased, with spaces
replaced by hyphens. `structure.source_fabric_id` retains the exact original capitalization and spacing.
Source categories are descriptors, not inferred purity or construction. For example, the source calls
Drape 11 a cotton-category fabric, while its concordant paper composition is 65% polyester/35% cotton.

| Example ID | Exact release specimen | Areal density, g/m² (display rounded) | Reported thickness, mm |
|---|---|---:|---:|
| `fabric101-2026/l01` | Linen Plain | 117.384 | 0.4 |
| `fabric101-2026/l03` | Ramie Plain | 65.771 | 0.3 |
| `fabric101-2026/drape-8` | Washed Linen Aqua | 240.260 | 0.8 |
| `fabric101-2026/kn01` | Cotton Jersey | 174.337 | 0.7 |
| `fabric101-2026/kn02` | Cotton Rib | 309.959 | 0.9 |
| `fabric101-2026/ex-08` | Coolmax Jersey | 128.734 | 0.6 |
| `fabric101-2026/ex-05` | Bamboo Rayon Plain | 251.123 | 0.6 |
| `fabric101-2026/e-c05` | Polyester/Cotton Plain | 102.973 | 0.4 |
| `fabric101-2026/ex-17` | Tencel/Linen Blend | 138.151 | 0.4 |
| `fabric101-2026/e-f02` | Waterproof Breathable | 126.124 | 0.2 |

The 96 rows comprise 32 cotton-category, four linen/ramie, seven wool, 15 silk, nine other
cellulose-derived fiber, ten polyester, four nylon, one acrylic, ten blend, three coated/functional,
and one paper specimen. The paper specimen is the study's printing-paper comparison sheet. Nine
records have explicit knit construction; ambiguous constructions use `sheet`. The instrument's
`fabrictype=woven` field is also printed on jersey and rib specimens and does not override their
actual descriptions.

Nominal compositions are included only where the article's Tables 11–13 can be joined to the same
release ID, description and matching mass/thickness. The paper's DP 1–12 correspond to the release's
Drape 1–12 by description and both measurements. Unquantified blends and ambiguous/conflicting
identities retain descriptive names without invented fractions. Bamboo rayon is explicitly
regenerated viscose, distinct from a bamboo bast fiber.

## Units, conditions and source identity

`areal_density` retains the original full-precision CSV value in `g/m2` and normalizes through the
ordinary material authority to `kg/m2`. `reported_thickness` retains `mm` and normalizes to `m`.
The article describes a balance with 0.001 g resolution and a fabric thickness tester. It does not
give thickness pressure, foot area, dwell time, density/thickness repeat count or measurement
uncertainty. Consequently, the adapter supplies no uncertainty bounds and does not interpret the
CSV's decimal digits as measurement accuracy. Test thickness is not installed as unloaded physical
`thickness`, contact separation or a fabric compression law.

The globally reported conditioning protocol is 24 hours at 20 ± 2 °C and 65 ± 5% RH, following
ISO 139:2005. Individual conditioning logs and moisture content are unavailable. Each observation
retains its exact instrument specimen ID, release CSV line and revision. Each material retains its
raw metadata path and Git blob hash, actual experiment directory, article citation and discrepancies.
For example, C01 is tied to `data/C01/002/head.dat`, not an invented `001` run.

| Source | Immutable identity |
|---|---|
| Release Git commit | `510519be0657a341a1a84fdb5c24be156ac83c9f` |
| `fabric_catalog.csv`, 5,835 bytes | SHA-256 `6a460054c3031554733c7706a8327c89e323ff4a4657352f5bcec0494d7cf96a` |
| `data_folder_mapping.csv`, 4,154 bytes | SHA-256 `57d7e0225b37ee7ca556b91f1e20d0d89450dd1927f3992ac467a42b4d36933c` |
| Article | arXiv `2609.13707v1`, §3.1, Tables 11–13 |
| Per-specimen instrument metadata | Exact pinned path and Git blob SHA-1 in every material |

Only the selected numerical and descriptive observations are transcribed. No upstream implementation
or complete raw-curve archive is redistributed. The README advertises MIT, but the pinned tree has
no LICENSE file; this adapter does not assert a license grant over those upstream assets.

## Deliberate exclusions and conflicting primary observations

There are 103 rows in the release CSV. Only 98 map to released raw specimen folders. All 98 mapped
headers were checked; 96 agree with the catalog within the metadata's printed numerical precision.

| Excluded identity | Reason |
|---|---|
| Drape 4 | CSV 114.61675579322639 g/m² conflicts with instrument 116.1056129 g/m² |
| E-KN06 | CSV 217.86096256684493 g/m² conflicts with instrument 172.13 g/m² |
| EX-03, R04, EX-15, Paper 2, Paper 3 | CSV rows without a matching released raw specimen folder |
| Cotton, Linen, Wool, PET, Silk demonstration folders | Authors' mapping explicitly says these are not catalog-ID matches |

For S08–S12, the article and release assign different silk descriptions/measurements to the same IDs.
The release and its own instrument metadata agree; those exact observations are retained, with no
composition or mechanical-property join to the article's conflicting IDs. EX-06 has a similar
cross-ID article discrepancy. E-KN07's release and instrument agree on 321.03 g/m², while the article
prints 231.71 g/m²; the adapter retains the release value and records the discrepancy.

## Raw experiments are locatable, not converted to sheet laws

The source tests a 260 × 110 mm fabric rectangle around radius-40 mm cylindrical holders, with a
nominal free height of 50 mm. It applies five twist cycles, five axial compression cycles, then a
stretch/slip pull. Files record force in N, displacement in mm, torque in N·m, rotation in degrees and
time in seconds. These are coupled, history-dependent cylindrical fixture responses.

For L01 warp `s1`, raw timestamps give a median positive twist rate of about 0.499854 °/s and
compression rate of 0.75 mm/s. The article reports 0.1 °/s and 1 mm/s. Its measured peak compression
of 16.14125 mm includes 1.14125 mm of pretension displacement in addition to the nominal 15 mm.
These differences remain visible in the record. Reusing the curves requires an explicitly qualified
cylinder fixture, actual pretension/reference geometry, tare, timing and sequential history.

No membrane modulus, Poisson coupling, shear response, bending rigidity, friction coefficient,
thermal insulation, laundering law or retail price is supplied by this adapter. A mass-based
screen can select these observations; a request that requires mechanics or thermal readiness
correctly refuses them until those measurements are supplied. Frozen selections replay against
an empty catalog with their exact sources and checksums intact.

### Read the released linen and knit instrument recordings

The offline importer reads both directions of four release specimens, each from run 001, repeat s1.
Choose `L01`, `L02`, `KN01` or `KN02` with `--specimen`. Omitting the flag selects L01.
For each acquisition, the importer checks the original specimen header, direction-specific tare
sheet and three independently pinned instrument files. The acquisition identifies the exact
source specimen and catalog material. The candidate material in a replay template can differ.

The following row counts refer to the raw instrument files. Each phase retains its own clock,
signed force and torque channels, and measured displacements and angles. The last column shows
the number of waypoints needed to reproduce the twist angles within 0.01° at every sensor sample.

| Catalog specimen | Direction | Twist samples | Compression samples | Stretch/slip samples | Twist waypoints |
|---|---|---:|---:|---:|---:|
| `fabric101-2026/l01` | Warp | 787 | 2,126 | 35,993 | 60 |
| `fabric101-2026/l01` | Weft | 786 | 2,249 | 34,537 | 62 |
| `fabric101-2026/l02` | Warp | 789 | 2,154 | 35,445 | 66 |
| `fabric101-2026/l02` | Weft | 786 | 2,237 | 34,988 | 62 |
| `fabric101-2026/kn01` | Warp | 792 | 2,261 | 35,990 | 59 |
| `fabric101-2026/kn01` | Weft | 772 | 2,207 | 40,574 | 60 |
| `fabric101-2026/kn02` | Warp | 776 | 2,126 | 38,978 | 67 |
| `fabric101-2026/kn02` | Weft | 777 | 2,199 | 38,905 | 68 |

```sh
node scripts/import_fabric101_cylinders.cjs /path/to/Fabric_101_for_Wrinkles warp > l01-warp.json
node scripts/import_fabric101_cylinders.cjs --specimen KN01 /path/to/Fabric_101_for_Wrinkles weft > kn01-weft.json
```

To predict a candidate material's response to the **initial twist**, supply an ordinary
`ftx-cylinder-experiment/1` template with a memoryless material, the measured apparatus dimensions
and a grain axis for the selected mounting. Declare whether the direction named by the release
folder runs axially or circumferentially; the acquisition headers do not resolve this mapping.
The importer checks the template’s grain axis against that convention for each direction.
The template supplies the proposed material, reaction offsets, mesh and solver controls. Set
its integration budget for the complete twist interval of the chosen recording. The example declares
that the named direction is axial and uses a 0.01° angular interpolation tolerance.

```sh
node scripts/import_fabric101_cylinders.cjs /path/to/Fabric_101_for_Wrinkles warp \
  candidate-cylinder.json measured-twist.json 0.01 named-direction-axial > warp-acquisition.json
ftx materials cylinder-experiment measured-twist.json --json > prediction.json
```

The generated request uses the measured mounting displacement. It retains the ends of every hold
and reversal, bounds linear angle interpolation error at every sensor timestamp, and refuses a
path needing more than 129 waypoints. The acquisition records selected row indices, clock origin, maximum angle
error and the digest of the generated request. The template and the raw recordings are preserved.

Section 3.1 of the paper locates the torque sensor and rotary drive on the lower holder and the
axial force sensor on the upper holder. The lower holder turns counterclockwise when viewed from
above. The importer sets `fixture.rotating_holder` to `"lower"` and preserves the recorded
angle sign. Each state reports both holder torques. Forward prediction and inverse calibration
subtract the declared torque tare at the driven ring and the force tare at the upper ring.
Requests omitting the selector still drive and read torque at the upper ring. These reactions
describe loads from the cloth; spindle inertia and instrument friction require separate evidence.
The model assumes zero axial grip slip during the initial twist. Internal-history laws require
a pristine timed mounting history, which these files do not provide. The native solver enforces
that requirement. Cross-stage elapsed time, measured grip slip, sensor-to-holder reaction signs
and offsets still require independent evidence before a full inverse experiment. These
recordings qualify actuator replay and instrument observations; they supply no calibrated law.
To run both source-backed regressions, point the L01 and four-specimen test inputs at the local release.

```sh
FABRIC101_RELEASE=/path/to/Fabric_101_for_Wrinkles \
FABRIC101_COHORT_RELEASE=/path/to/Fabric_101_for_Wrinkles \
node --test scripts/test_import_fabric101_cylinders.cjs
```

## Cylindrical-holder forward experiment (`ftx-cylinder-experiment/1`)

`ftx_drape::cylinder::evaluate` (`ftx materials cylinder-experiment REQUEST.json`, `ftx-materials
cylinder-experiment REQUEST.json`, browser `material_cylinder_experiment`, worker
`cylinderExperiment`, and `web/materials.html` `#cylinderExperiment`) implements the 3D cylindrical-holder forward boundary condition for source-qualified
sheets (`260 × 110 mm` specimen wrapped around coaxial `R = 40 mm` holders, `50 mm` nominal free height
plus explicit `pretension_displacement_m`, explicit tare force/torque subtraction, grain orientation
`warp_axis`, sequential twist / compression / stretch-slip schedules with optional `clamp_slip_m` and
`grip_slip_force_limit_n`, optional Maxwell/membrane-plastic/panel-bending-plastic histories, optional
unloaded-thickness cloth self-contact, and automatic raw-timestamp vs `nominal_protocol` discrepancy
auditing).

Strict boundaries enforced by the forward experiment:
- **Never `delta / nominal_height`**: inner-clamp separation is `nominal_free_height_m + pretension_displacement_m + axial_displacement_m - clamp_slip_m`; history-bearing laws must start at pristine unstretched separation and ramp pretension explicitly in time.
- **No fabricated thickness or mechanics**: raw `fabric101-2026/*` records continue to refuse until constitutive moduli are explicitly supplied; enabling `self_contact` requires unloaded `thickness` and refuses presser-foot `reported_thickness`; axial compression below `nominal_free_height_m` requires positive `bending_rigidity` or a `BendingTensor`.
- **No silent readiness or empirical claim**: `mechanics_readiness_promoted`, `fit_claim`, and `empirical_validation_claim` are all `false`.

## Cylindrical-holder inverse identification (`ftx-cylinder-calibration/1`)

`ftx_drape::cylinder::fit::calibrate` (`ftx materials calibrate-cylinder REQUEST.json [--allow-estimates]`,
`ftx-materials calibrate-cylinder REQUEST.json [--allow-estimates]`, browser `material_calibrate_cylinder`,
worker `cylinderFit`, and `web/materials.html` `#cylinderFit`) identifies caller-declared scalar
orthotropic membrane and/or bending parameters (`warp_stiffness`, `weft_stiffness`, `poisson_warp_weft`,
`shear_stiffness`, `bending_rigidity`) from `FIT` cylindrical-holder acquisitions while solving the full
3D `cylinder` forward model at every trial.

When identifying scalar bending, an existing source-qualified nonlinear membrane stays active;
it does not need duplicate scalar membrane properties. When identifying membrane parameters,
an existing directional panel-bending law stays active, including compression experiments with
zero scalar hinge rigidity. Every candidate, sensitivity, refined mesh, held-out observation and
publication replay uses those same unchanged laws and their source/estimate/domain admission.
Targeting scalar coefficients of an already declared nonlinear or directional response still
refuses, as do unsupported history laws. Version 2 below admits explicitly supplied Maxwell
histories. Calibration cannot silently fit a scalar surrogate and
then publish the coefficients alongside a different retained response.

Acceptance and publication gates enforced before any `Selection` is emitted:
- **Three-start convergence & interior minimum**: deterministic starts (`0.25`, `0.5`, `0.75`) in the normalized parameter box are first searched on the coarse mesh, then refined on the same doubled mesh used for reported predictions. Both stages share the original per-start iteration and total forward-solve budgets. All three refined solutions must converge inside bounds without materially distinct equal-SSE minima; a stationary coarse approximation alone cannot qualify the published coefficients.
- **Excitation Gram pivot (`min_excitation_pivot`)**: normalized sensitivity Cholesky pivot at the doubled-mesh optimum must exceed `policy.min_excitation_pivot` so unexcited moduli (for example weft stiffness under warp-only torsion) are rejected rather than guessed.
- **Mesh doubling (`max_mesh_scaled`), held-out `CHECK`, and `fitted_green_strain_range` replay**: every `FIT` and independent `CHECK` experiment is replayed on both the declared `[n_theta, n_z]` mesh and `[2 * n_theta, 2 * n_z]`, held-out `CHECK` strains must remain inside `fitted_green_strain_range`, and a final `StrainDomain::Box` replay verifies the published material.
- **No fabricated thickness or silent readiness**: identified quantities carry `Evidence::Estimate` and require explicit `--allow-estimates` (or request `allow_estimates: true` in the browser worker) to admit; presser-foot `reported_thickness` is never promoted to unloaded contact `thickness`, so sheets lacking unloaded thickness or body friction keep `mechanics()` unready (`simulation_ready: false`). External unbundled raw Fabric-101 curve archives (upstream `GongDeshan/Fabric_101_for_Wrinkles@510519be` has no repository license for raw archive redistribution) and physical held-out coupon validation remain open under `ftx-tm2.1`.

For the lower-driven fixture, independent small-angle torsion calculations provide synthetic
torque observations on two grain mountings and a held-out loading. Both finite-difference and
adjoint calibration recover the prescribed shear stiffness of 400 N/m to within 0.05 N/m and
pass replay on the coarse and doubled meshes.

A synthetic mixed-holder case at revision `030a0ee` illustrates the bounded replay check.
The five-coefficient static experiment drove the lower holder for both weft acquisitions and
the upper holder for the warp and bias acquisitions. Its adjoint gradients agreed with central
finite differences. The finite-difference candidate passed publication, while the adjoint
candidate stalled when replaying the upper-driven warp-extension case on the coarse mesh with
its fitted domain enforced. Its last Newton correction was 1.36 × 10⁻¹⁰ m against a 1 × 10⁻¹⁰ m
tolerance. The solver withheld that selection. An accurate local gradient alone cannot establish
that a fitted material will reproduce the ordinary constrained forward solve.

### Identify relaxation from complete timed cylinder histories (`/2`)

An explicit `"schema": "ftx-cylinder-calibration/2"` request uses the same forward cylinder
solver with its supplied, source-qualified Maxwell law. It identifies selected branch
quantities from actual holder forces, torques and/or mid-height radii over the complete
loading history. The solver determines local cloth strains; no conversion from nominal
holder travel to uniform material strain is substituted for the 3D experiment.

The source material must already declare `membrane_relaxation` and its equilibrium response,
branch identities, initialization and integration scheme. Select one to five parameters in
total, using the existing scalar targets and/or these branch targets:

```json
"targets": [
  {"parameter": "maxwell_stiffness_ratio", "branch_id": "recovery", "bounds": [0.2, 1.0]},
  {"parameter": "maxwell_relaxation_time", "branch_id": "recovery", "bounds": [0.05, 0.3]}
]
```

These illustrative bounds are dimensionless for the stiffness ratio and seconds for the
recovery time. Every branch target needs its exact existing `branch_id`. Stiffness-ratio
targets apply to proportional branches; recovery-time targets also support independently
qualified orthotropic branches. The fitter does not create branches or infer an unmeasured
direction's tensor. `replace_existing: true` explicitly permits replacement of the selected
source quantities. Untargeted branch tensors, equilibrium properties and original acquired
strain limits remain intact. Specimen and conditioning must agree with the source law.

Histories must begin at the pristine, unstretched nominal holder separation, with zero rotation
and slip at time zero, followed by strictly increasing physical times. An authored pretension
offset must be canceled by the initial axial displacement, then reached through actual timed
loading waypoints. Holds are repeated displacement
and rotation at later times. Every waypoint advances the same material memory, whether or
not it carries an observation; a static solve cannot erase a prehistory. Plastic/damage laws
and parallel plasticity remain outside this inverse mode.

For non-contact Maxwell cylinders, affine holder motion supplies an initial Newton guess. The
previous physical positions, inertia and accepted material memory remain unchanged until the solve
converges. This lets a fitted material replay within its supported domain without treating a moved
holder and an untouched interior as the required starting geometry or widening the source limits.
The prescribed-boundary step checks the actual accepted pose before the holder moves, verifies
that only fixed coordinates changed, and checks the new guess against the active material domains.

Version 2 requires finite-difference sensitivity and an explicit finite positive numerical
time-refinement tolerance, for example:

```json
"policy": {
  "max_rms_scaled": 0.2,
  "max_peak_scaled": 0.3,
  "max_mesh_scaled": 0.2,
  "max_time_scaled": 0.2,
  "min_excitation_pivot": 0.0001,
  "max_iterations": 40,
  "max_forward_solves": 32768,
  "sensitivity": "finite_difference"
}
```

After the selected candidate passes the existing search, excitation, interior-minimum and
spatial-refinement checks, it replays on the doubled spatial mesh with **two substeps for
every ordinary time step**. This refines short authored intervals too. The largest absolute
change in any observed force, torque or radius channel, divided by that channel's residual
scale, must not exceed `max_time_scaled`. This is a numerical acceptance criterion, not
measurement uncertainty. `temporal_refinement` records the subdivision factor, both step
upper bounds, the actual scaled change and acceptance. The same `controls.max_steps`,
vertex-state and aggregate forward-solve budgets charge all probes and final replays.

Optimization and strain support use FIT histories only. CHECK histories can reject the
fixed candidate; they cannot select different coefficients or enlarge its support. A
successful frozen material marks only the replaced quantities as estimates, removes their
old uncertainty bounds and binds their FIT-only strain domain. That domain intersects any
independently fitted equilibrium support and remains inside the original acquired Maxwell
domain. Ordinary downstream forward and garment consumers enforce the resulting support.
Failed temporal, held-out, identifiability or domain checks publish a diagnostic report
without a selectable material.
Prediction histories identify the candidate source law. Publishing fitted-source provenance
changes the frozen law's identity while preserving its physical coefficients and memory evolution.

Run the supplied synthetic example with either ordinary command:

```sh
ftx materials calibrate-cylinder examples/cylinder-maxwell-calibration.json --allow-estimates --json
ftx-materials calibrate-cylinder examples/cylinder-maxwell-calibration.json --allow-estimates
```

The Material Workbench's **Cylindrical-holder inverse identification** accepts the same JSON,
retains exact original and frozen-selection bytes, and displays branch IDs and time-refinement
results. Version 1 requests retain their existing elastic behavior and report format.
The example's reactions are independently calculated synthetic axial and torsional histories. They test
numerical identification, not measured Fabric-101 material behavior or garment fit. Source
acquisition and physical specimen qualification remain open under `ftx-tm2.1` and `ftx-rc1`.

## Focused checks

`crates/ftx-pattern/tests/fabric101_catalog.rs` verifies the 96-row subset, exact representative
measurements and units, source paths and field budgets, conflict handling, composition identity,
immutable replay, stale-digest refusal, ordinary mass screening, and missing-physics refusal.
`crates/ftx-drape/src/cylinder.rs`, `crates/ftx-drape/src/cylinder/fit.rs`,
`crates/ftx-cli/tests/strip_experiment_workflow.rs`, `tools/check_strip_wasm.cjs`,
`tools/check_materials_frontend.cjs`, and `tools/check_materials_web.cjs`
verify closed-form uniaxial tension and `n_theta`-gon apothem torsion
convergence, 4-parameter orthotropic + compression-bending inverse recovery, unexcited/corrupted-`CHECK`/budget rejections, `L01` pretension/timestamp rate discrepancy auditing, Maxwell + plastic-bending dissipation,
uncalibrated `fabric101-2026/l01` and `reported_thickness` refusals, and native/WASM/worker/`web/materials.html` UI parity.
The source catalog implementation is `crates/ftx-pattern/src/material_catalog/fabric101.rs`.

The refinement regressions keep the original recovery tolerances: 3% for warp, weft and shear
stiffness, and 8% for bending rigidity. They also require all three starts to converge within the
shared iteration caps, the optimized FIT error to equal the reported FIT error, and timed
twist/compression predictions to agree with the unchanged public-forward truth on the same mesh
(peak scaled residual below `1e-4`). A separate actual-solver control consumes the entire budget at
an exactly stationary coarse solution and requires refinement to return `BudgetExceeded`. These
checks establish numerical consistency and bounded execution; independent physical specimen
and raw-curve qualification remain open.
