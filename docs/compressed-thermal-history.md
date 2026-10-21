# Heat history during prescribed layered compression

`thermal-compression` connects the existing inverse stack-closure and dry heat solvers. It solves
pressure from each prescribed clearance, updates the source-qualified transport coefficients, and
advances a thermal hold **without resetting the material temperatures or changing reference mass**.
It returns the history and the final ordinary frozen selection for every layer.

This is a one-way sequence: a quasi-static gap change, followed by a hold at that new state. It is
not a continuously moving squeeze, a recovery law, or a garment contact-pressure simulation.
Mechanical branch work is reported separately and is **not converted into generated heat**.

## Runnable numerical example

From the repository root, compose a request from the existing synthetic compression fixture:

```sh
heat_demo=$(mktemp -d)
python3 - "$heat_demo/history.json" <<'PY'
import json
import sys
from pathlib import Path

preparation = json.loads(Path("tests/fixtures/materials/synthetic-compression.json").read_text())
preparation["pressure_pa"] = 0.0

def hold(gap):
    return {
        "seconds": 10.0, "gap_m": gap,
        "interface_resistance_m2_k_w": [],
        "inside_c": 40.0, "outside_c": 0.0,
        "inside_resistance_m2_k_w": 0.0, "outside_resistance_m2_k_w": 0.0,
    }

request = {
    "schema": "ftx-compressed-thermal-history/1",
    "setup": {
        "stack": {
            "schema": "ftx-compression-stack/1",
            "gap_m": 0.020, "reference_area_m2": 0.25,
            "direction": "loading", "allow_estimates": True,
            "layers": [{"id": "layer", "preparation": preparation}],
        },
        "cells_per_layer": 1, "initial_temperature_c": [0.0],
        "thermal_evidence": "Synthetic constant-capacity numerical fixture; explicit reservoirs and films; mechanical heating omitted",
    },
    "holds": [hold(0.015), hold(0.010)],
}
with Path(sys.argv[1]).open("x") as stream:
    json.dump(request, stream, indent=2)
PY
cargo run --locked -p ftx-cli --bin ftx-materials -- thermal-compression \
  "$heat_demo/history.json" > "$heat_demo/report.json"
```

The fixture is explicitly synthetic: its thermal coefficients are extended as constants for this
numerical check, not empirically qualified across these temperatures. A real input needs its own
pressure and thermal applicability evidence.

The single material cell has reference areal heat capacity `C = 100 J/(m² K)`. At each hold the
independent backward-Euler answer is

```
T_next = [(C / dt) T_previous + (2 / R) (T_inside + T_outside)] / [C / dt + 4 / R]
```

| End time | Clearance | Pressure | Resistance | Cell temperature | Gap-change work |
|---|---|---|---|---|---|
| 10 s | 15 mm | 250 Pa | 0.375 m² K/W | 10.3225806452 °C | 0.15625 J |
| 20 s | 10 mm | 500 Pa | 0.25 m² K/W | 16.2779156328 °C | 0.46875 J |

Reference mass remains 0.025 kg. Cumulative work is 0.625 J. Sensible stored heat is determined by
the boundary heat and the retained temperatures, **not** by adding that work. Tests also compress a
uniform 20 °C stack between 20 °C reservoirs: positive mechanical work leaves every cell at 20 °C
with zero sensible-energy change. This verifies the stated omission, not absence of real heating.

The unified command evaluates exactly the same input:

```sh
cargo run --locked -p ftx-cli --bin ftx -- materials thermal-compression \
  "$heat_demo/history.json" --json
```

The standalone command emits the report directly; the unified command wraps it under `result`.
Success exits 0, refusal exits 2 standalone or 3 unified. A late refusal emits no successful partial
history or final selection. Inputs are only read, and no source path mentioned in evidence is opened.
The commands print to standard output; shell redirection itself is not an overwrite-safe writer.

## Source, time and discretization contract

`setup.stack` is the existing complete [compression-stack request](compression-stacks.md), including
all original acquisitions, fixed reference area, initial common pressure and one loading direction.
Its `gap_m` defines the prepared state at time zero. `initial_temperature_c` describes that state
**after preparation**, with one temperature per cell in inside-to-outside layer order. Initial
preparation work is retained in `initial_compression` and is excluded from the subsequent work sum.

Every layer must have independently source-qualified pressure-invariant `areal_density` and
`specific_heat`. Sampled or changing storage properties are refused rather than transferring heat
using a guessed reference enthalpy. The ordinary source/estimate policy still applies separately to
each layer. Root estimate consent cannot weaken a layer's consent. Missing thermal resistance or
conductivity, mass or capacity is a refusal; missing observations never become nominal defaults.

Cells follow equal portions of each layer's **reference mass**; they are not fixed-volume Eulerian
cells. Cell count, reference area, physical layer order and storage coefficients do not change.
A new pressure is always evaluated against the original acquisition, not recursively against a
previously prepared selection. The resistance and compressed thickness may change. Thickness-cubed
stiffness scaling and bulk-density-times-current-volume mass are not used.

Each hold requires its own `interface_resistance_m2_k_w` (one entry per layer interface), reservoir
temperatures, and inner/outer film resistances. Resistances are finite and nonnegative; zero denotes
an ideal interface or prescribed face temperature, not an inferred convection coefficient. These
are explicit supplied boundary assumptions, **not** properties inferred from compression pressure.
`setup.thermal_evidence` records their applicability, the constant-property temperature/time scope
and the intentional omission of mechanical heating; its text cannot prove those assumptions true.

At the start of each hold the prescribed clearance changes without assigning thermal time to that
change. The new transport coefficients are held throughout `seconds`; the existing conservative
backward-Euler solver advances the previous temperatures. Refine durations/cell counts to assess
time/spatial error: stability and conservation do not establish numerical or empirical accuracy.

No branch switching or cyclic recovery is inferred. Further loading must follow the original
loading branch; an unloading history requires a qualified unloading branch and initial preload.
A repeated endpoint gap is a legal thermal hold with zero incremental mechanical work.

## Reports and reuse

`ftx-compressed-thermal-history-report/1` retains the typed input and its digest, initial compression,
per-hold temperatures/pressure/thickness/source locks, and final compression with every original-format
selection. Heat flux is positive inside-to-outside. `inside_heat_j` enters the stack and
`outside_heat_j` leaves it. Per-step and cumulative sensible-energy balances check

```
stored_change = entering_boundary_heat - leaving_boundary_heat
```

The external-work ledger is separate. The report does not claim a complete first-law balance that
includes unmodeled elastic storage, dissipation, thermoelasticity, phase change or recovery.

A final layer remains usable through the normal material path. Preserve the **exact selection JSON
string**, rather than parsing and reserializing its numbers:

```sh
python3 - "$heat_demo/report.json" "$heat_demo/layer.json" <<'PY'
import json
import sys
from pathlib import Path
report = json.loads(Path(sys.argv[1]).read_text())
layer = next(x for x in report["final_compression"]["layers"] if x["id"] == "layer")
with Path(sys.argv[2]).open("x") as stream:
    stream.write(layer["conditioned"]["selection_json"])
PY
cargo run --locked -p ftx-cli --bin ftx-materials -- bind \
  tests/fixtures/materials/synthetic-stock-pattern.json "$heat_demo/layer.json" "$heat_demo/bound.json"
cargo run --locked -p ftx-cli --bin ftx-materials -- resolve "$heat_demo/bound.json"
```

The resolver uses 10 mm full thickness, 5 mm body offset and unchanged 0.1 kg/m² reference areal mass.
Binding protects existing output files and preserves pattern geometry. A material selection is not
a saved temperature state: retain the complete history report to keep the heat/work results.

## Bounds, atomicity and remaining scope

The existing inverse solver limits layers to 32. This history adds 1–64 cells per layer, 1024 total
cells, 1–512 holds, and 262144 cell-holds. Native/shared browser input is bounded to 64 MiB. Invalid
or duplicate source identities, out-of-domain gaps, nonunique pressure plateaus, unqualified storage,
nonrepresentable time/energy, or failed thermal conservation are explicit refusals.

The Rust `CompressedStack::advance` changes no field until the complete mechanical/thermal step
passes. Retry after a failed step starts from the preceding committed pressure, temperatures and
ledgers. `evaluate` publishes a complete history only if every hold succeeds. The shared API uses
the same path; there is no independent browser numerical implementation.

Rust: `ftx_drape::thermal::compression::{CompressedStack, evaluate}`. Shared file-free API:
`ftx_wasm::thermal::compression_history`, exported as `thermal_compression_history` in WebAssembly.
Tests: `ftx-drape/tests/compressed_thermal.rs`, `ftx-cli/tests/compressed_thermal_workflow.rs`, and
`ftx-wasm/tests/compressed_thermal.rs`.

This does not model moisture, evaporation, lateral spreading, pressure-dependent interfacial contact,
continuous squeeze dynamics, compression heating, changing heat capacity, garment pressure fields,
physiology or measured garment performance. Computational acceptance is not a real-textile,
comfort, thermal-safety or real-wearer-fit qualification.
