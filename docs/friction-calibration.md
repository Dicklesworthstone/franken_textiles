# From sliding acquisitions to material-selected cloth contact

`ftx-materials calibrate-friction SLIDING.json [--allow-estimates]` identifies the existing scalar
kinetic Coulomb coefficient from corrected normal load, pulling force and slip velocity. It emits
ordinary source-locked sheet/interface inputs consumed by the production contact solver. No new
contact law, fiber mixing rule, body model or numerical smoothing parameter is introduced.

```sh
cargo run -p ftx-cli --bin ftx-materials -- calibrate-friction \
  tests/fixtures/materials/synthetic-friction-sliding.json --allow-estimates > friction-fit.json
```

The complete bundled input is **synthetic**, not a recommended linen/leather/knit friction preset.
It identifies 0.4 from differently oriented pulls and checks a separate acquisition. The regression
in `ftx-drape/tests/piece_materials/friction_acquisition.rs` runs actual meshing, two-sheet contact and
Newton stepping. Changing only acquired pulling forces changes slip while geometry, initial state,
mass and membrane response stay fixed. This is software/physics evidence, not textile qualification.

## Acquisition and units

The input schema is `ftx-friction-calibration/1`. Supply the exact `target`, acquisition `sources`,
`method`, shared `conditions`, explicit `fitted_on` date and whole `experiments`. Each experiment has
an ID, source key, evidence class, `fit`/`check` partition and steady sliding `samples`:

- `normal_n`: measured compressive contact load in N, strictly positive.
- `pull_n`: two signed corrected pulling-force components in N, in a common tangent frame. A component
  can be `null` when unmeasured; zero is a real zero-force observation, not a missing sensor channel.
- `velocity_m_s`: nonzero relative tangent velocity in the same frame. The corrected pull acts WITH
  slip; the resisting friction on the sliding body has the opposite sign.
- `scale_n`: positive force residual weighting scale, not automatically a standard deviation.

State the contacting faces, grain alignment, finish, conditioning, apparatus, normal-load measurement,
speed and force corrections in the source/protocol. Remove apparatus drag, gravity, acceleration and
other forces before passing steady pulling forces. This command is not raw time-series segmentation,
a static breakaway estimator, a friction standard implementation, or an apparatus calibration.
A single-axis force sensor is supported, but observing only the force perpendicular to slip cannot
identify the unknown coefficient. Stationary and zero-load inputs refuse.

For the observed components, minimize the weighted squared residual of
`pull_j = mu * normal_n * velocity_j / |velocity|`. Both force magnitude AND direction matter;
transverse forces, adhesion offsets or direction-dependent resistance can exceed the declared
`max_rms_scaled`/`max_peak_scaled` budgets and refuse the scalar model. No intercept is fitted or
quietly attributed to friction. Zero and coefficients greater than one are allowed; nothing is clamped.
Entire check experiments stay outside identification. `check: null` explicitly means none were supplied.

## Three explicit destinations, one source authority

`target.kind = "pair"` takes `materials: [SELECTION-A, SELECTION-B]`. The output's `binding.materials`
contains the unchanged frozen targets and `binding.interface` is the existing `fabric.interfaces`
wire type. Put that entry into a pattern using those exact selections. Do not replace an entire
existing interface list or rewrite hashes to make it pass; a material edit requires reviewing its
pair evidence. Assembly still requires every unlike pair when self-contact is enabled. A pair fit
never changes either sheet's same-cloth or body-friction property.

`target.kind = "self_contact"` takes one `material` selection and updates only `friction_self`.
`target.kind = "body"` takes one `material` plus a `counterface` source record (reference, accessed_on,
note) and updates only the existing `friction_skin` slot. A surrogate is identified as a surrogate,
not relabeled human skin. These two modes return `binding.selection`, usable by the ordinary `bind`
command. Replacing an existing value requires `replace_existing: true`; the pre-fit frozen record
remains in the report. Missing mass, thickness, elasticity or other friction values remain missing.

For a body/self result, extract the selection and bind to a NEW pattern output:

```sh
python3 -c 'import json; print(json.dumps(json.load(open("friction-fit.json"))["binding"]["selection"]))' \
  > friction-selection.json
cargo run -p ftx-cli --bin ftx-materials -- bind \
  YOUR-PATTERN.json friction-selection.json characterized-pattern.json
```

The example above needs a body/self acquisition; the bundled fixture is a pair acquisition and emits
an interface instead. The source-bearing `Interface` contract now lives in `ftx-pattern` and is shared
with drape, not independently reconstructed by the command. Existing parsing, permissions, pairwise
coverage requirements and numerical contact equations are retained.

All fitted coefficients are **estimates**, even from measured forces. Permission to fit synthetic
observations (`allow_estimated_observations`) is independent of `--allow-estimates` for the output.
Pair consent is also independent of each sheet's policy. `readiness.scope = "interface_only"` reports
only the pair's admission and separate sheet readiness, not whole-garment readiness. Body/self results
use `material_mechanics` readiness. Retain the full report beside the bound pattern: its digest covers
the original input and frozen counterfaces; its local source locator is not an automatically hosted file.

## Physical limits and next evidence gate

The result is a constant, symmetric, isotropic effective kinetic coefficient at the supplied conditions.
The report preserves fit-only load and speed envelopes, but the current solver does NOT enforce them
as a runtime material domain, identify pressure/speed-dependent friction, or match an identified body
counterface to its mesh. The caller must qualify the intended operating conditions; a surrogate result
does not establish human skin behavior. The solver still uses its existing low-speed regularization;
its sticking response is not independently calibrated static friction. No parameter confidence interval,
uncertainty propagation, machine permission or garment-fit claim follows from a small residual.

Plan §25.2–25.3 / `ftx-tm2` and `ftx-tm4` remain open for matched real counterfaces, directions,
loads/speeds, conditioning, repeatability, and independent held-out textile/garment tests. For the
underlying contact family see the primary IPC and C-IPC references already used by the solver:
https://ipc-sim.github.io/ and https://ipc-sim.github.io/C-IPC/ .
