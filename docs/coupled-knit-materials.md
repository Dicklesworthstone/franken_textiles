# Coupled knit materials in ordinary garment draping

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
`ftx-drape/tests/coupled_knit_materials.rs`, and the knit-report tests in
`ftx-drape/src/run.rs`. Empirical calibration and the complete K09 benchmark
remain separate work.
