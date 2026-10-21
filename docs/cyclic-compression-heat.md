# Loading, unloading and reloading without resetting heat

`ftx-compressed-thermal-history/2` adds an explicit path of independently acquired compression
branches to the existing `thermal-compression` command. Temperature profiles, reference mass,
heat capacity, elapsed time and the cumulative heat/work ledgers survive each reversal. There is
still one compression inverse and one thermal solver, not a second hysteresis implementation.

```sh
ftx materials thermal-compression cycle.json --json
ftx-materials thermal-compression cycle.json
```

Both commands accept the original version-1 single-branch requests unchanged. Their exit codes
remain 0 for success, 3 for unified-command refusal and 2 for standalone-command refusal. The
shared file-free `ftx_wasm::thermal::compression_history` and its existing WebAssembly export
`thermal_compression_history` accept both versions. The existing material workbench's
**Compression and heat histories** control runs either version, displays chronological results and
assigns an explicitly selected final layer. See the [browser workflow](compressed-thermal-history.md#browser-workbench).

## What a branch change means

A `leg` contains nonempty `holds`, each with the unchanged fields described in
[compressed-thermal-history.md](compressed-thermal-history.md). With no `branch_change`, it continues
the currently active branch. A change supplies:

* `entry`: a complete `ftx-compression-stack/1` request for the new branch, at the **current** gap
  and pressure, not at the first hold's destination. Its direction must reverse the current direction.
* `transitions`: one source receipt per physical layer, keyed by the existing layer ID. Each needs
  `source`, `evidence`, `method` and `conditions` matching `specimen`, `conditioning`,
  `from_loading_branch`, `to_loading_branch` and the actual `pressure_pa`.

The source key must already exist in the unchanged original material. The method must describe
why that branch follows this particular acquisition history, including peak, cycle and dwell
applicability. An intersection between two arbitrary curves is not sufficient. The software checks
these declared identities and numerical joins; it cannot establish the truth of a source assertion.

Area, physical layer order/IDs, original material records, specimen, conditioning and estimate
permissions must remain unchanged. Each source table and invariant is independently checked by the
ordinary conditioning engine. The new entry must involve **zero travel and zero work**. Every layer's
thickness must meet its preceding thickness at the reversal pressure, not merely preserve their sum.
Numerical agreement uses only a floating-point representation tolerance, not measurement error or
permission to shift a curve. No gap, pressure, thickness or source observation is fitted to make a
join succeed. Flat nonunique inverses retain the ordinary refusal behavior.

Mass and heat capacity must be independently qualified for each branch and numerically unchanged
in every material cell. A reference observation whose own conditions restrict it to `loading`
cannot be retained under `unloading` merely because it is pressure-invariant. Supply genuinely
applicable reference measurements and independent invariance evidence from the outset; **do not
remove real source restrictions to get a successful result**. New acquisition or source revisions
require a new complete path request, not substitution halfway through an existing state.

## Runnable synthetic cycle

This example is deliberately invented for software verification. Its reference properties are
explicitly assumed branch-independent; its transition receipts are estimates. It is not a measured
textile, and copying its curves or widening its assumptions does not qualify a physical material.
Run from the repository root:

```sh
cycle_demo=$(mktemp -d)
python3 - "$cycle_demo/cycle.json" <<'PY'
import copy
import json
import sys
from pathlib import Path

p = json.loads(Path("tests/fixtures/materials/synthetic-compression.json").read_text())
p["pressure_pa"] = 0.0
# This is a NEW synthetic experiment: reference values are independently applicable to both
# branches. The original fixture is not edited. Never strip restrictions from real observations.
for quantity in p["material"]["properties"].values():
    quantity["conditions"].pop("loading_branch", None)

initial = {
    "schema": "ftx-compression-stack/1", "gap_m": 0.020,
    "reference_area_m2": 0.25, "direction": "loading", "allow_estimates": True,
    "layers": [{"id": "sample", "preparation": p}],
}

def hold(gap):
    return {
        "seconds": 10.0, "gap_m": gap, "interface_resistance_m2_k_w": [],
        "inside_c": 40.0, "outside_c": 0.0,
        "inside_resistance_m2_k_w": 0.0, "outside_resistance_m2_k_w": 0.0,
    }

def reversal(unloading):
    entry = copy.deepcopy(initial)
    branch = entry["layers"][0]["preparation"]
    before, after, pressure, gap = (
        ("loading", "unloading-after-500", 500.0, 0.010) if unloading else
        ("unloading-after-500", "loading", 0.0, 0.020)
    )
    entry["gap_m"] = gap
    entry["direction"] = "unloading" if unloading else "loading"
    branch["pressure_pa"] = pressure
    branch["loading_branch"] = after
    if unloading:
        middle = copy.deepcopy(branch["samples"][0])
        middle["pressure_pa"] = 250.0
        middle["properties"]["thickness"]["value"] = 12.0  # mm
        middle["properties"]["thickness"].pop("bounds", None)
        middle["properties"]["thermal_resistance"]["value"] = 0.3
        for quantity in middle["properties"].values():
            quantity["conditions"]["pressure_pa"] = "250"
        branch["samples"].insert(1, middle)
    for row in branch["samples"]:
        for quantity in row["properties"].values():
            quantity["conditions"]["loading_branch"] = after
    for invariant in branch["invariants"].values():
        invariant["conditions"]["loading_branch"] = after
    link = {
        "source": "synthetic", "evidence": "estimate",
        "method": "Invented complete cycle after the 500 Pa peak and 0 Pa return; numerical check only",
        "conditions": {
            "specimen": p["specimen"], "conditioning": p["conditioning"],
            "from_loading_branch": before, "to_loading_branch": after,
            "pressure_pa": str(pressure),
        },
    }
    return {"entry": entry, "transitions": {"sample": link}}

request = {
    "schema": "ftx-compressed-thermal-history/2",
    "setup": {
        "stack": initial, "cells_per_layer": 1, "initial_temperature_c": [0.0],
        "thermal_evidence": "Synthetic constant storage across these temperatures and both branches; prescribed reservoirs/films; no mechanical heating",
    },
    "legs": [
        {"holds": [hold(0.010)]},
        {"branch_change": reversal(True), "holds": [hold(0.012), hold(0.020)]},
        {"branch_change": reversal(False), "holds": [hold(0.015)]},
    ],
}
with Path(sys.argv[1]).open("x") as stream:
    json.dump(request, stream, indent=2)
PY
cargo run --locked -p ftx-cli --bin ftx-materials -- thermal-compression \
  "$cycle_demo/cycle.json" > "$cycle_demo/report.json"
cargo run --locked -p ftx-cli --bin ftx -- materials thermal-compression \
  "$cycle_demo/cycle.json" --json > "$cycle_demo/envelope.json"
```

The stack has one cell with `C = 100 J/(m² K)` and reference mass `0.025 kg`. For each ten-second
hold, with material resistance `R`, the independent backward-Euler answer is

```
T_next = (10 T_previous + 80/R) / (10 + 4/R)
```

| End time | Active branch | Gap | Pressure | Cell temperature | Incremental external work |
|---|---|---:|---:|---:|---:|
| 10 s | loading | 10 mm | 500 Pa | 12.3076923077 °C | +0.625 J |
| 20 s | unloading after 500 Pa | 12 mm | 250 Pa | 16.7032967033 °C | −0.1875 J |
| 30 s | unloading after 500 Pa | 20 mm | 0 Pa | 18.1684981685 °C | −0.25 J |
| 40 s | loading again | 15 mm | 250 Pa | 19.1137894364 °C | +0.15625 J |

The first complete geometric cycle has net external work `0.1875 J`: the area between the declared
piecewise-linear loading and unloading curves, multiplied by `0.25 m²`. The fourth step brings the
cumulative total to `0.34375 J`. Neither value is injected as heat or classified as measured
irreversible dissipation. The temperature change follows the boundary heat and the retained cell
state. A separate uniform-temperature regression verifies positive loop work with no invented heating.

## Results, state and failure handling

The version-2 report keeps ordinary chronological `steps`, `initial_compression`, `final_compression`,
all authored `input` and its digest. `transitions` identifies each reversal by its zero-based global
`before_hold` index and retains the old compression identity, complete source link, zero-work entry
receipt and first accepted new-branch hold. The duplicate first-hold record is diagnostic context;
use `steps` once for integration, not both arrays.

All cumulative quantities span the entire path. Initial preparation work remains separate. Both
successful and refused branch changes use the existing conservation gates. The stateful API
`CompressedStack::advance_on` commits the source change and its first thermal hold together: even a
late bad boundary condition leaves the old branch, pressure, temperatures, work, time and step budget
unchanged. `path::evaluate` returns a complete report only if every leg succeeds.

The limits apply to the entire request: 1–32 nonempty legs, at most 512 holds and 262144 cell-holds,
plus the original 32-layer, 1024-cell and per-acquisition bounds. Starting a new leg does not reset
work limits. Both native commands and the shared API use the same 64 MiB JSON boundary.

Final material selections remain under
`final_compression.layers[].conditioned.selection_json`. Keep those exact strings for ordinary
pattern binding, as in the version-1 example; they describe the final static material state, not a
saved cyclic or thermal state. Keep the complete path report to retain its histories and evidence.
Commands write to standard output; shell redirection is not itself an overwrite-safe writer.

## Remaining physical scope

This supports explicit source-qualified load/unload/reload paths, not prediction of an unobserved
minor loop, an arbitrary peak, time-dependent recovery, creep, damage, wetting or thermomechanical
feedback. Gap changes remain quasi-static and have no assigned thermal duration; only the following
holds advance time. Interface and film resistances remain explicit inputs. There is no inference of
pressure-dependent contact conductance or garment-wide contact pressure.

A source may be internally self-consistent and physically wrong. No numerical join, energy ledger,
source digest or synthetic regression establishes measured specimen agreement, comfort, wearer fit,
seam quality or permission to operate hardware. The broader `ftx-cp2` physical gate remains open.
