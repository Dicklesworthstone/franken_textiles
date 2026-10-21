#!/usr/bin/env python3
"""Prepare a real-source strip request through the public acquisition and catalog commands.

The numerical bounds and residual scales are declared analysis choices, not measured properties.
This script does not synthesize forces, interpolate rows, infer width observations, or fit a law.
Run the resulting request with `ftx-materials calibrate-strip` and inspect its acceptance report.
"""

import argparse
import json
import math
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[2]
ACQUISITION = ROOT / "crates/ftx-pattern/src/material_catalog/knit-strip-measurements.json"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="New request JSON path; must not exist")
    parser.add_argument("--specimen", choices=("SJ1", "SJ3", "DKIN8"), default="SJ1")
    parser.add_argument("--workbench", type=Path, help="Built ftx-materials binary; otherwise cargo run")
    args = parser.parse_args()
    command = [str(args.workbench.resolve())] if args.workbench else [
        "cargo", "run", "--quiet", "-p", "ftx-cli", "--bin", "ftx-materials", "--"
    ]

    def call(*words):
        try:
            result = subprocess.run(command + list(words), cwd=ROOT, check=True, capture_output=True, text=True)
        except subprocess.CalledProcessError as error:
            raise SystemExit((error.stderr or error.stdout or str(error)).strip()) from None
        return json.loads(result.stdout)

    # The Rust adapter verifies original hashes, metadata links, MIT license and acquisition identity.
    acquired = call("strip-acquisition", str(ACQUISITION))
    material = call("show", f"sperl2022/{args.specimen}")["selection"]["record"]
    protocol = acquired["protocol"]
    axes = {"warp": [0.0, 1.0], "weft": [1.0, 0.0], "bias": [math.sqrt(0.5), math.sqrt(0.5)]}
    experiments = []
    for direction in ("warp", "weft", "bias"):
        curve, = [c for c in acquired["curves"] if c["specimen"] == args.specimen and c["direction"] == direction]
        key = f"strip_{direction}"
        # Keep the parser's exact citation/hash and its unreported-condition disclosures.
        material["sources"][key] = curve["source"]
        observations = []
        for extension in (0.1, 0.2, 0.3):
            row, = [r for r in curve["observations"] if r["branch"] == "loading" and r["extension_fraction"] == extension]
            observations.append({
                "id": f"{args.specimen}/{direction}/{curve['sha256']}/line-{row['csv_line']}",
                "extension_fraction": row["extension_fraction"],
                "force_n": row["force_n"],
                "scale_n": 0.05,
                "mid_width": None,
            })
        experiments.append({
            "id": f"Sperl2022/{args.specimen}/{direction}",
            "fixture": {
                "gauge_length_m": protocol["free_gauge_length_m"],
                "width_m": protocol["reference_width_m"],
                "warp_axis": axes[direction],
                "cells": [2, 4],
            },
            "source": key,
            "evidence": "measured",
            "partition": "check" if direction == "bias" else "fit",
            "observations": observations,
        })
    conditions = dict(protocol["conditions"])
    conditions.update({
        "specimen": f"Sperl2022:{args.specimen}",
        "retained_acquisition_sha256": acquired["acquisition_sha256"],
        "analysis_selection": "Exact loading rows at grip extensions 0.1, 0.2 and 0.3; warp/weft FIT, entire bias cut CHECK",
        "measurement_operator": "Total grip force only; source transverse curves are retained but their spatial sampling is not equivalent to the solver mid_width by evidence",
        "numerical_weights": "0.05 N residual scale is an analyst-selected comparison scale, not instrument uncertainty or a confidence interval",
        "parameter_bounds": "Exploratory positive-definite search box; not measured specimen bounds or a claim of global identification",
    })
    request = {
        "schema": "ftx-strip-calibration/1",
        "material": material,
        "method": "Sperl et al. (2022) section 4.3/Figure 4 source geometry and original signed loading-force rows; explicitly assumed planar quasistatic membrane and ideal full-width no-slip grips",
        "conditions": conditions,
        "fitted_on": "2026-10-21",
        "experiments": experiments,
        "parameter_bounds": [[5.0, 200.0], [5.0, 100.0], [-0.1, 0.2], [1.0, 200.0]],
        "admit_planar_elastic_model": True,
        "allow_estimated_observations": False,
        "replace_existing": False,
        "allow_estimates": False,
        "controls": {"tolerance_m": 1e-9, "force_tolerance_n": 1e-6, "max_newton": 100},
        "policy": {
            "max_rms_scaled": 1.0,
            "max_peak_scaled": 2.0,
            "max_mesh_scaled": 1.0,
            "min_excitation_pivot": 1e-5,
            "max_iterations": 50,
            "max_forward_solves": 18000,
        },
    }
    with args.output.open("x", encoding="utf-8") as file:
        json.dump(request, file, indent=2, ensure_ascii=False, allow_nan=False)
        file.write("\n")
    print(f"Wrote {args.output}: 6 FIT and 3 CHECK force states; no inferred membrane law or transverse width")


if __name__ == "__main__":
    main()
