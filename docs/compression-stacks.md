# Infer stack pressure from available clearance

`compression-stack` solves a **quasi-static, uniform normal pressure** for one to 32 coextensive
layers confined between rigid parallel faces. Unlike `pressure-condition`, pressure is an output:
the input supplies the final clearance and each layer's source-qualified pressure/thickness branch.
The operation returns one ordinary frozen material selection per layer. Existing mechanical/contact
and thermal consumers use those selections; there is no alternate compressed-material catalog.

This is a bounded planar equilibrium calculation, not a 3D garment-contact or recovery solver.

## Runnable synthetic example

From the repository root, create a fresh example directory. This uses the existing explicitly
**synthetic** pressure characterization twice; it is not data for a real fabric or fill.

```sh
stack_demo=$(mktemp -d)
python3 - "$stack_demo/request.json" <<'PY'
import json
import sys
from pathlib import Path
p = json.loads(Path("tests/fixtures/materials/synthetic-compression.json").read_text())
p["pressure_pa"] = 0.0
request = {
    "schema": "ftx-compression-stack/1",
    "gap_m": 0.030,
    "reference_area_m2": 0.25,
    "direction": "loading",
    "allow_estimates": True,
    "layers": [{"id": name, "preparation": p} for name in ("outer", "inner")],
}
with Path(sys.argv[1]).open("x") as output:
    json.dump(request, output, indent=2)
PY
cargo run --locked -p ftx-cli --bin ftx-materials -- compression-stack \
  "$stack_demo/request.json" > "$stack_demo/report.json"
```

Each synthetic layer starts at 20 mm and decreases linearly to 10 mm at 500 Pa. A final total gap
of 30 mm therefore requires **250 Pa**, with **15 mm per layer**, **62.5 N** over the specified
0.25 m² area, **0.3125 J** signed external work, and **0.05 kg** retained total reference mass.
The series material resistance is **0.75 m²·K/W**, excluding interfaces and boundary films.
These are analytical software checks, not measured textile qualification.

The unified command has the same operation and input:

```sh
cargo run --locked -p ftx-cli --bin ftx -- materials compression-stack \
  "$stack_demo/request.json" --json
```

The standalone command emits the report directly. The unified command places it under the usual
`ftx-cli/1` envelope's `result`. Success exits 0; a refusal exits 2 for the standalone command and
3 for the unified command. No partial successful layer selection is returned on refusal.

## Reuse a layer, without altering its source or numeric identity

Extract the **exact `conditioned.selection_json` string**, not a reserialized selection object:

```sh
python3 - "$stack_demo/report.json" "$stack_demo/outer.json" <<'PY'
import json
import sys
from pathlib import Path
report = json.loads(Path(sys.argv[1]).read_text())
layer = next(layer for layer in report["layers"] if layer["id"] == "outer")
with Path(sys.argv[2]).open("x") as output:
    output.write(layer["conditioned"]["selection_json"])
PY
cargo run --locked -p ftx-cli --bin ftx-materials -- bind \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  "$stack_demo/outer.json" "$stack_demo/bound-pattern.json"
cargo run --locked -p ftx-cli --bin ftx-materials -- resolve "$stack_demo/bound-pattern.json"
```

Binding preserves pattern geometry and refuses existing destinations. The actual resolver uses the
selected full thickness and its normal half-thickness body offset, while retaining reference areal
mass. It does not infer stiffness by thickness-cubed scaling. Keep the complete stack report with
all layer acquisitions: each resulting material includes its digest, and changing the gap, area,
layer order or acquisition creates a different frozen selection. Do not mistake a layer selection
for an entire laminate, or silently assign the same layer to every piece in an outfit.

In `web/materials.html`, load the stack request in **Prepare a compressed material state**, enter
an exact **stack layer ID**, and prepare. The page displays inferred pressure, force, work, closure
residual, per-layer states and unavailable capabilities. All layer selections and the unmodified
request/report are downloadable. Only the explicitly selected layer enters the ordinary inspection
and piece-binding controls. Editing the layer ID cancels stale preparation, and no first layer is
silently chosen. For a prescribed-pressure request, leave the layer ID empty.

## Qualification, assumptions and refusal boundaries

Every `layers[].preparation` is a complete existing `ftx-material-compression/1` request, with its
original material, specimen, fixed conditioning, chosen loading/unloading branch, observations and
property-invariance qualifications. Here its `pressure_pa` means the **known initial common
pressure**, identical in every layer. It is not an optimization seed. Supply a source branch
appropriate to `direction`; a branch name cannot verify its physical history.

The solver intersects all characterized pressure intervals, then restricts movement from the
initial pressure according to `loading` or `unloading`. It inverts the sum of piecewise-linear
thicknesses on the union of all pressure knots. Different layers can have different knots and
compliances. A flat target interval cannot establish a unique pressure and is refused, even when
one of its endpoints exactly matches the requested gap. Out-of-range gaps, pressure domains with
no interval in common, mixed initial pressures, invalid source policies and unrepresentable
arithmetic refuse without extrapolation, silent clamping or an invented air gap.

Root `allow_estimates: true` admits the inverse and branch-path model. Each layer's own consent
still governs estimated acquisitions, retained properties and interior interpolation. Exact knots
retain the original authored quantities; interior quantities remain estimates. Observation bounds
are retained by conditioning, but are **not** propagated to a pressure or work confidence interval.

External work integrates `p * (-d thickness)` over every traversed segment of each layer's branch,
starting at the supplied preload. Loading is positive and unloading negative. This is not a split
into elastic storage and dissipation, not a hysteresis model, and **not generated heat**. It does
not model lateral spread, frictional confinement, buckling, transient recovery or branch switching.

Unqualified optional reference mass or scalar thermal transport remains unavailable, not zero.
Stack totals are present only when every layer has the corresponding admitted capability. Reference
mass is never inferred from bulk density times compressed thickness. Series resistance excludes
film, interlayer and air resistance; applicability remains conditional on the retained source
conditions. No pressure field on a garment, physiological pressure limit, sewing-process clearance,
comfort, thermal-safety or real-wearer fit claim follows from this result.

Rust: `ftx_pattern::material_compression::stack::close`. Shared browser API:
`ftx_wasm::materials::compression_stack`, exported as `material_compression_stack` in WebAssembly.
