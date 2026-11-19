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
    result = subprocess.run([binary, f"{kind}-experiment", source], check=True, stdout=subprocess.PIPE)
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
        cylinder["controls"].update(max_step_s=0.125, max_steps=64)
        save(args.output / "cylinder.json", solve(args.binary, "cylinder", cylinder, scratch))
        strip = json.loads((ROOT / "examples/strip-identification/synthetic-forward.json").read_text())
        strip["fixture"]["cells"] = [4, 4]
        strip["extensions"] = [n / 100 for n in range(9)]
        reports = []
        for name, axis in [("Warp", [0, 1]), ("Weft", [1, 0]), ("Bias", [math.sqrt(0.5)] * 2)]:
            strip["fixture"]["warp_axis"] = axis
            reports.append({"name": name, "report": solve(args.binary, "strip", strip, scratch)})
        save(args.output / "strip.json", reports)
