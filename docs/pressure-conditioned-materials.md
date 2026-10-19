# Prepare a material at a characterized pressure

`ftx-materials pressure-condition` prepares an ordinary frozen material selection from
source-bearing pressure observations. The selected thickness and thermal transport then reach the
existing material, contact and heat consumers when their other required inputs are available.
There is no separate compressed-material solver or fiber-based property inference.

The operation represents a **prescribed dry state at fixed reference area**. Supply pressure in Pa;
a measured force without its loaded area does not establish pressure. It does not calculate garment
contact pressure, evolve loft during motion, infer compression recovery, or model moisture and
mechanical heating. To compare another pressure, change the original request and prepare a new
selection before rebuilding the simulation.

## Runnable example and portable selection

Run from the repository root with the pinned Rust toolchain. The complete
[synthetic fixture](../tests/fixtures/materials/synthetic-compression.json) supplies invented
observations and explicit invariance assumptions for software verification; it characterizes no real
textile. Its `allow_estimates: true` admits those inputs and interpolation.

```sh
pressure_demo=$(mktemp -d)
cargo run --locked -p ftx-cli --bin ftx-materials -- pressure-condition \
  tests/fixtures/materials/synthetic-compression.json > "$pressure_demo/report.json"

python3 - "$pressure_demo/report.json" "$pressure_demo/selection.json" <<'PY'
import json
import sys
from pathlib import Path

report = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
Path(sys.argv[2]).write_text(report["selection_json"], encoding="utf-8")
print("Selected pressure (Pa):", report["input"]["pressure_pa"])
print("Discarded properties:", report["discarded_properties"])
PY

cargo run --locked -p ftx-cli --bin ftx-materials -- bind \
  tests/fixtures/materials/synthetic-stock-pattern.json \
  "$pressure_demo/selection.json" "$pressure_demo/pattern.json"
cargo run --locked -p ftx-cli --bin ftx-materials -- resolve "$pressure_demo/pattern.json"
```

This binds the selection to a miniature synthetic pattern and resolves its actual mechanical inputs
without changing the piece geometry. `bind` requires a new destination file. The example stops before
meshing or draping; normal body, placement, interface and solver requirements still apply.

Write **`selection_json` directly**, as above. Parsing the outer report to obtain this string does
not reserialize its embedded selection. Rebuilding that selection with JavaScript
`JSON.stringify(report.selection)` can change numbers such as `-0.0` and invalidate the content lock.
Keep `report.json` beside the selection: it contains the complete acquisition and invariance receipt.

The unified native route invokes the same operation:

```sh
cargo run --locked -p ftx-cli --bin ftx -- materials pressure-condition \
  tests/fixtures/materials/synthetic-compression.json --json
```

Its `ftx-cli/1` envelope places the report under `result`; the standalone command emits the report
directly. Estimate consent belongs in the request JSON, not a pressure-conditioning CLI flag.

### Known answers

The fixture holds reference mass at `100 g/m2` and specific heat at `1000 J/(kg*K)`.

| Prescribed pressure | Full thickness | Resistance | Heat flux for a 20 K face difference | Reference areal heat capacity |
|---|---|---|---|---|
| 0 Pa, observed knot | 20 mm | 0.5 m²·K/W | 40 W/m² | 100 J/(m²·K) |
| 250 Pa, interpolated | 15 mm | 0.375 m²·K/W | 53⅓ W/m² | 100 J/(m²·K) |
| 500 Pa, observed knot | 1 cm | 0.25 m²·K/W | 80 W/m² | 100 J/(m²·K) |

These heat-flux values exclude boundary films. At 250 Pa the mechanical resolver uses a 7.5 mm
cloth/body half-thickness offset. Independent membrane and bending coefficients retain only their
explicitly qualified values; no thickness-cubed stiffness scaling occurs. Original bulk density and
conductivity are intentionally discarded by this fixture, which selects resistance as its transport
authority. These are arithmetic checks, not warmth or real garment-fit measurements.

## Input and qualification rules

The [Rust API](../crates/ftx-pattern/src/material_compression.rs) is
`ftx_pattern::material_compression::condition(&Input) -> Result<Report, Refusal>`.
The fixture is a complete `ftx-material-compression/1` request.

| Input fields | Required meaning |
|---|---|
| `schema`, `material` | Supported schema and exact original `Material`, including all referenced sources. Fiber and yarn forms are unsupported; characterized sheets and fills are accepted. |
| `specimen`, `conditioning`, `loading_branch` | One specimen, fixed acquisition state and declared loading/unloading branch. These exact fields must appear in each observation's and invariance declaration's `conditions`. |
| `method`, `conditioned_on` | Acquisition/reduction method and receipt date. Document any source-supported conversion to pressure. |
| `pressure_pa` | Finite nonnegative target within the observed interval. No extrapolation. |
| `samples` | 2–128 strictly pressure-ordered rows, each with `pressure_pa` and the same supported scalar `properties` keys. Every row requires positive thickness; normalized loft must be nonincreasing as pressure increases. |
| `invariants` | Map from original scalar property names to independent source-qualified pressure-invariance declarations. Absence does not establish applicability. |
| `allow_estimates` | Explicit permission for estimated observations, estimated invariance and interior interpolation; defaults to false. |

Each row quantity uses the ordinary `Quantity` fields: value, unit, evidence, source, method, conditions
and optional bounds. Per-property conditions must match across rows apart from an optional
`pressure_pa`, which must equal that row's pressure. Unknown property keys, incomplete row keysets,
missing sources and mixed acquisition conditions refuse.

An invariant supplies `pressure_range_pa`, `source`, `evidence`, `method` and `conditions`. Its range
must cover **the entire pressure table**. It must preserve the original property's conditions; if that
property records `pressure_pa`, the value must be finite, nonnegative and covered by the same invariant
range. A value acquired at 2000 Pa cannot be reused using a constancy claim covering only 0–500 Pa.
The declaration itself cannot contain a fixed `pressure_pa`: its range is the applicability authority.
Invariance does not improve the original evidence class.

Sampled pressure-dependent `areal_density` is refused. If retained through an admitted invariant,
reference areal mass keeps its original value, units and bounds. Otherwise mass remains unavailable;
it is never reconstructed from bulk density times compressed thickness. A pressure-invariant bulk
density is also refused when loft varies. The output admits at most one transport authority,
`thermal_resistance` or `thermal_conductivity`; either may remain missing.

## Knots, interpolation and omitted capabilities

An exact observed pressure retains that row's authored quantities, units and bounds. Interior
selection uses piecewise-linear interpolation in pressure after canonical unit conversion and marks
the selected observations as **estimates**, including when both endpoints were measured. If both
endpoints supply bounds, their envelopes are interpolated under that same assumed linear law; these
are not new confidence intervals. If either endpoint lacks bounds, the selected bounds remain absent.

`ftx-material-compression-report/1` returns the complete `input`, `input_sha256`, ordinary `material`,
frozen `selection`, exact `selection_json`, full `pressure_range_pa`, selected `bracket_pa` and
`interpolated` flag. Exact knots use `[p,p]` as their bracket.

`discarded_properties` lists original scalar keys neither sampled nor qualified as invariant.
`discarded_responses` lists removed `membrane_response`, `membrane_relaxation` and `thermal_response`
blocks. `discarded_structure` lists removed `*_model` selectors, including directional bending and
plasticity selectors. This scalar operation cannot qualify those response models for a new pressure.
The normal consumers refuse any resulting missing inputs: a fill may support steady conduction
while lacking storage measurements or a shell constitutive law.

The output reserves `structure.compression_model = "pressure_conditioned_scalar/1"`,
`compression_pressure_pa` and `compression_input_sha256`. An input already carrying any
`compression_` structure key refuses reconditioning; recover its original report input instead.

## Browser and sewing behavior

Build and serve the [material workbench](browser-material-workbench.md):

```sh
sh tools/build_web.sh
python3 -m http.server -d web 8000
```

Open `http://localhost:8000/materials.html`. In **Prepare a compressed material state**, load the
characterization JSON and choose **Prepare material at the requested pressure**. Review the reported
domain, interpolation, omissions and capability checks, then assign the inspected selection to the
pattern default or a source piece and choose **Freeze selections & bind**. The worker calls the same
Rust function through `material_pressure_condition(request_json)` and downloads the original request,
complete report and exact `pressure-conditioned-selection.json`. It retains the Rust selection text
through assignment and binding.

The current sewing planner requires qualified **uncompressed** stack thickness. It refuses every
pressure-conditioned selection, including one prepared at an observed 0 Pa knot, rather than using
its thickness to pass unloaded clearance or presser-foot limits. A pressure-qualified sewing recipe
and appropriate unloaded geometry are separate work. Likewise, preparing or binding a selection
does not authorize hardware execution or establish dynamic compression, moisture response or empirical
fit and warmth.
