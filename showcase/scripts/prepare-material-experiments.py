"""Record the synthetic FTX material experiments for the browser gallery."""

import argparse
import json
import math
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def solve(binary, kind, request, scratch):
    source = scratch / f"{kind}.json"
    source.write_text(json.dumps(request), encoding="utf-8")
    result = subprocess.run([binary, f"{kind}-experiment", source], check=True, capture_output=True)
    return json.loads(result.stdout)


def save(path, data):
    encoded = json.dumps(data, separators=(",", ":"), allow_nan=False).encode() + b"\n"
    if path.exists() and path.read_bytes() == encoded:
        return
    with path.open("xb") as handle:
        handle.write(encoded)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/ftx-materials")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as folder:
        scratch = Path(folder)
        cylinder = json.loads((ROOT / "tests/fixtures/materials/synthetic-cylinder-experiment.json").read_text())
        outfit=json.loads((ROOT/"tests/fixtures/materials/synthetic-joint-outfit.json").read_text())
        fabric=outfit["garments"][0]["pattern"]["fabric"]["material"]
        cylinder["material"]=fabric
        cylinder["fixture"].update(cells=[32,8],self_contact=True)
        cylinder["controls"].update(max_newton=300,velocity_tolerance_m_s=1e-8,max_step_s=0.125,max_steps=256)
        detailed=solve(args.binary,"cylinder",cylinder,scratch)
        save(args.output/"cylinder.json",detailed)
        cylinder["fixture"]["cells"]=[24,8]
        coarser=solve(args.binary,"cylinder",cylinder,scratch)
        assert [s["time_s"] for s in detailed["states"]]==[s["time_s"] for s in coarser["states"]]
        further_meshes = []
        for cells in ([36, 10], [40, 8], [48, 12]):
            cylinder["fixture"]["cells"] = cells
            try:
                run = solve(args.binary, "cylinder", cylinder, scratch)
            except subprocess.CalledProcessError as error:
                refusal = json.loads(error.stderr)["refusal"]
                if refusal["code"] != "not_converged":
                    raise
                further_meshes.append({"cells": cells, "refusal": refusal})
            else:
                further_meshes.append({"cells": cells, "inputSha256": run["input_sha256"],
                    "readings": [{"time": s["time_s"], "force": s["upper_force_n"]} for s in run["states"]]})
        save(args.output/"cylinder-refinement.json",{"cells":[24,8],"inputSha256":coarser["input_sha256"],
             "readings":[{"time":s["time_s"],"force":s["upper_force_n"]} for s in coarser["states"]],
             "additionalMeshes":further_meshes})
        strip = json.loads((ROOT / "examples/strip-identification/synthetic-forward.json").read_text())
        strip["material"]=fabric
        strip["controls"]["max_newton"]=300
        strip["fixture"]["cells"] = [16, 24]
        strip["extensions"] = [n / 100 for n in range(9)]
        reports = []
        for name, axis in [("Warp", [0, 1]), ("Weft", [1, 0]), ("Bias", [math.sqrt(0.5)] * 2)]:
            strip["fixture"]["warp_axis"] = axis
            reports.append({"name": name, "report": solve(args.binary, "strip", strip, scratch)})
        save(args.output / "strip.json", reports)
        source=ROOT/"crates/ftx-pattern/src/material_catalog/knit-strip-measurements.json"
        actual=json.loads(subprocess.check_output([args.binary,"strip-acquisition",source]))
        curves=[next(c for c in actual["curves"] if c["specimen"]=="SJ1" and c["direction"]==d) for d in ("warp","weft","bias")]
        rows=[]
        for extension in strip["extensions"]:
            observations=[next(r for r in c["observations"] if r["branch"]=="loading" and r["extension_fraction"]==extension) for c in curves]
            rows.append({"extension":extension,"force":[r["force_n"] for r in observations],"csvLines":[r["csv_line"] for r in observations]})
        save(args.output/"jersey-readings.json",{"specimen":"SJ1","units":"N","directions":["warp","weft","bias"],"rows":rows,
              "sources":[c["source"] for c in curves],"protocol":actual["protocol"],"license":actual["license"]})
