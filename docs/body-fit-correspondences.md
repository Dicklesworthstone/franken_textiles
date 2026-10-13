# Surface correspondences and bounded multi-frame fitting

`ftx measure fit-body` keeps the input described in `multi-frame-body-fit.md`, with
an additional surface correspondence and a total model-evaluation limit:

```sh
ftx measure fit-body request.json --model model.ftxbody \
  --iterations 50 --max-evaluations 100000 --out fit.json --json
```

The output must not already exist. A numerical budget stop exits 4 and may save
diagnostics, but its `solution` is null. Rejected or ambiguous evidence exits 2;
invalid inputs/options exit 3. No fitted shape/pose is published on these paths.

## A point inside a mesh triangle

A keypoint mapping in `model.keypoints` can be a `surface` instead of a `vertex`,
`bone_head` or `bone_tail`:

```json
{
  "authored_surface_landmark": {
    "kind": "surface",
    "face": 123,
    "barycentric": [0.2, 0.3, 0.5]
  }
}
```

The face index is zero-based in the **exact model file bound by `model_sha256`**;
123 is illustrative, not a supplied anatomical mapping. Weights correspond to
that triangle's stored corner order. They must be finite, nonnegative and at most
one, with sum one to 1e-12. They are not normalized or clamped. The observation's
name still explicitly maps to this authored model location; anatomy is not guessed.

The adapter skins the three vertices first, interpolates the posed 3D triangle,
then projects that point. Interpolating rest vertices or image pixels would be
wrong for triangles crossing joint influences or with perspective depth variation.
Duplicate surface supports are refused, including a vertex also expressed as a
triangle corner and a shared-edge point expressed through adjacent faces.

Only the distinct surface vertices needed by observations are skinned per pose.
The same Anny forward-kinematics and linear-blend-skinning kernels are used, with
unchanged accumulation order. Rest-shape/tape evaluation is unchanged; this is
not a reduced body model. The library's `SelectedPose` is deliberately distinct
from a complete `PosedBody` and cannot be passed off as a full collision skin.
Synthetic numerical tests establish bit-identical selected/full skinning; no
production-model speed claim is made.

## Work and ambiguity gates

`max-evaluations` is in 1..1000000, default 100000. One evaluation means one
`BodyModel::points` or `BodyModel::measurements` call. The counter is global across
all starts and includes central differences, rejected trial steps, held-out
validation, failed calls and the final measurement call. Layout queries are not
charged. It bounds callback count, not the time or memory used inside a callback.
Both total and per-attempt `model_evaluations` are reported. The total may include
one final call outside the attempts. An exhausted later start remains a reported
`BUDGET_STOP` cell rather than disappearing. Even exhaustion after all starts
converge prevents solution publication.

Converged starts must agree on **both shape and each frame's pose parameters**.
`shape_spread_fraction` and frame-keyed `pose_spread_fraction` report high minus
low over starts, divided by each parameter's declared range. The default gate is
0.01 for each. The library policy permits 1e-6..0.1. This is conservative parameter
agreement, not covariance, calibrated uncertainty, an angle-wrapping equivalence
test or a proof of global uniqueness. Symmetry-related parameterizations may be
refused; specify a suitable nonredundant pose chart rather than hiding disagreement.
A small image residual or agreement on shape alone cannot authorize conflicting
pose estimates. Reports name the hard failure in `flags`.
