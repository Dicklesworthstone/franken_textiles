# Coupled knit materials in garment draping and coupon experiments

A material can now select the existing coupled knit shell through its normal
source-bearing material record. This works for a uniform garment or individual
pieces in a mixed-material sewn garment. The ordinary drape path used by native
and portable consumers binds the same selected laws after sewing.

## Select an explicit law

Set the material's `form` to `knit` and add `knit_response` with
`model: "coupled_knit_shell/1"`. A knit form by itself selects no constitutive law.

The required `membrane` block and optional `bending` and `hysteresis` blocks each
contain `parameters`, a material `source` key, `evidence`, a nonempty `method`,
and `conditions` identifying the exact `specimen` and `conditioning`.
Estimated blocks require the ordinary selection's explicit `allow_estimates`
consent. Selecting a block never upgrades its evidence.

The complete runnable input shape is illustrated by
[`synthetic-coupled-knit.json`](../tests/fixtures/materials/synthetic-coupled-knit.json).
Every numerical coefficient in that fixture is an explicit software-test estimate.
It is not a measured material, yarn-lot qualification, or recommended physical model.

| Block | Parameters and units |
| --- | --- |
| Membrane | `q11, q22, q12, q66`, quartic `c1111, c2222, c1122`, and `kappa_jam` in N/m; `e1_jam, e2_jam, gamma_jam` dimensionless |
| Bending | `d_plus, d_minus, d12, d66` in N·m; preferred `kappa0` and transition `delta_kappa` in 1/m |
| Hysteresis | Dimensionless `thresholds` and `huber_eps`; `weights` in N/m; at most 32 modes |

The explicit grainline is the wale axis. The course axis is its in-plane normal.
Membrane shear and bending twist use engineering components. Preferred bending
curvature is signed in the physical material frame.

Areal density, unloaded physical thickness, body/self friction, and scalar seam
bending remain independent observations. They must refer to the same specimen
and conditioning as the knit law. A membrane fit supplies none of those values.

## What the garment solver consumes

Each retained material triangle receives its selected coupled membrane. The
quartic and jamming terms participate in the actual energy, gradient and tangent.
Asymmetric knit bending and ordinary tensor bending can occupy different source
panels in one garment. Scalar seam hinges retain their independent approximation.

Optional yarn-slip hysteresis starts with pristine backstrain after start recovery.
Only accepted implicit-Euler steps advance it. Rejected Newton trials and rejected
motion requests retain the accepted state. Body-motion continuation carries this
history forward.

Competing membrane laws, mismatched source states and missing physical inputs
refuse. Unsupported scalar/tabulated consumers explicitly refuse a selected
coupled law instead of silently using its origin stiffness. Asymmetric bending
refuses DiscreteShells and Rayleigh bending damping; knit hysteresis refuses
unsupported integrators and static solves. Retained-liquid mechanical binding is
not admitted for these selected knit records.

## Drape a saved stitch graph

`ftx drape` also accepts a saved `ftx-knit/1` document. Supply the actual selected
material with `--fabric`; the file contains `{"material": SELECTION}`, where
`SELECTION` is the canonical source-bearing selection described above.

```sh
ftx drape pattern.knit.json --measurements measurements.json \
  --body-fit body_fit.json --model body.ftxbody \
  --fabric fabric.json --drape 36 --out knit-drape --json
```

The surface uses the document's actual loop centres, parent connections and
blocked gauge. Shaped edges and held neckline openings are retained; full- and
half-gauge torso tubes remain closed tubes. Each output vertex and triangle maps
back to source stitch IDs in `knit_surface.json`. Free edges stop at the outer
loop centres: this discretization adds no fictitious half-stitch boundary width.
Unsupported branch, cable and anchored short-row surface cells refuse instead
of becoming a filled rectangle. The machine compiler can support a topology
before this continuum surface consumer supports it.

This path currently admits at most 12,000 source vertices and 2,000,000 retained
vertex-frames. These are explicit refusal bounds, not requests to reduce the
mesh or simulation schedule. The initial placement uses the actual torso frame
and two disclosed source-loop supports. Generic annuli, sleeves and cuffs need
their own placement; they are not automatically placed as torso garments.

The selected record supplies mass, thickness, friction and the coupled law.
There is no reference-stockinette fallback. Material identity retained in the
KnitDoc or supplied as `--material-sha256` must match the consumed record. An
optional `--qual swatch.json` checks the exact washed gauge, machine, cam and
retained yarn lot; a gauge qualification does not characterize the mechanical
law. The output preserves the original fabric file and frozen selection for
replay, alongside the material report and drape receipt.

`--audit-patch all`, `boundary` or `selvedge` evaluates the same selected continuum
law on the indicated triangles. It is a continuum diagnostic, not an independent
yarn-level patch solve. It does not invent pore optics from an unrelated prior.
The audit requires the unmodified source compression response and a supported
normal-curvature diagnostic. Ordinary vector-Hessian bending remains available
for draping with the optional patch audit omitted.
Newton convergence and numerical settling are reported separately; neither
establishes physical knitting or wearer fit.

## Run a finite-grip coupon

The existing coupon requests now accept the same canonical material selection,
source records, estimate policy and coupled membrane used by garment draping.
Both full-width grips constrain transverse displacement; the interior contracts
freely in plane. Local Green strain comes from the solved triangles, not from
converting grip extension into a homogeneous strain.

| Consumer | Coupled equilibrium membrane | Selected yarn-slip bank | Independent inputs and limits |
| --- | --- | --- | --- |
| `strip::simulate`, `ftx materials strip-experiment` | Supported, including quartic coupling and jamming | Refused; use the timed path | Declared geometry and wale axis; no mass, bending or contact properties required |
| `ftx materials strip-history` | Supported through the same production solver | Retained across accepted loading, holds and unloading | Qualified reference areal mass and an explicit clock required; planar backward-Euler dynamics |
| Existing finite-grip coefficient identification | Coupled parameter identification remains unimplemented | Unimplemented | Forward simulation does not establish measured FIT/CHECK acceptance |

Use the existing request schemas and commands:

```sh
ftx materials strip-experiment strip-request.json
ftx materials strip-history history-request.json
```

A `ftx-strip-experiment/1` request contains `material`, `fixture`, `extensions`
and optional `controls`. A `ftx-strip-history/1` request contains `material`,
`fixture`, `loading` and optional `controls`; loading starts at
`{"time_s": 0, "extension_fraction": 0}` and increases in time.
The shared fixture declares `gauge_length_m`, `width_m`, `warp_axis` and even
`cells: [across_width, along_length]`. Its wale axis `[0, 1]` aligns with the
loading direction; `[1, 0]` loads the course direction. The native
`ftx-materials` binary and portable `material_strip_experiment` /
`material_strip_history` entry points use these same consumers.

The frozen report retains the complete selected law and source identity. Timed
reports include every accepted triangle's `knit_hysteresis` backstrain and
`knit_hysteresis_dissipation_j`, separately from recoverable play-spring storage.
Grip reactions use the return map active during that accepted step; reading the
report does not apply another slip update. Missing mass, refused evidence,
jamming-domain escape, nonconvergence or a complete-path work-budget failure
refuses the request without a successful partial report.

These are planar membrane experiments. Optional knit bending is outside their
scope; they do not measure curl, buckling, grip slip, thermal conversion, or
garment fit. Numerical dissipation is not counted as physical yarn-slip work.

## Read the result

The material report retains the model, source record, law digest and evidence.
The ordinary run's `knit_shell` report covers only the triangles and patches
actually selecting knit laws. It includes jamming margin, principal stretch,
asymmetric curvature indicators, and accepted hysteresis history.

Jamming, excessive local curvature indicators and large accepted backstrain enter
ordinary gating flags. The curvature indicator is physical curvature times the
square root of reference patch area; it is not a measured free-edge curl angle.
Rib-transition snap and pore grin-through remain explicitly unavailable.
`complete_knit_assessment` stays false, and this report does not manufacture a
Seam-4 receipt or establish physical garment fit.

The focused software regressions are
`ftx-pattern/tests/material_knit.rs`,
`ftx-drape/tests/coupled_knit_materials.rs`,
`ftx-drape/tests/coupled_knit_strips.rs`, and the knit-report tests in
`ftx-drape/src/run.rs`. Empirical calibration and the complete K09 benchmark
remain separate work.
