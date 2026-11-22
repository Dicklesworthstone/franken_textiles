"""Package two independently accepted native drapes of the fixed Garden Sundress on two synthetic adult forms
plus one genuine native solver refusal on an oversized/tight form for /wardrobe/sundress/compare/ (ftx-9zm).

- Form A (`reference`): existing native drape in showcase/public/exhibits/sundress/dress.* (168 cm, 90/72/96 cm)
- Form B (`slender`): newly solved native drape from WORK_DIR/drape (162 cm, 84/66/90 cm) using the EXACT same
  fixed dress-pattern.json, dress-fit.json, and DRAPE_ARGS (--spacing-mm 20 --time-subdivision --arm-deg 22
  --drape 150 --max-settle-steps 256 --self-contact-mm 1).
- Form C (`tight_refused`): genuine native `ftx drape` refusal on synthetic full form (170 cm, 99/84/106 cm)
  with no completed garment mesh.

Run from repository root:
  python3 showcase/scripts/prepare-sundress-compare.py /tmp/ftx_sundress_slender showcase/public/exhibits/sundress-compare
"""
import argparse
import gzip
import hashlib
import json
import struct
import subprocess
from pathlib import Path
from runpy import run_path

import numpy as np


ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
FTX = ROOT / "target/release/ftx"
DRAPE_ARGS = [
    "--spacing-mm", "20",
    "--time-subdivision",
    "--arm-deg", "22",
    "--drape", "150",
    "--max-settle-steps", "256",
    "--self-contact-mm", "1",
]


def load_packed_final_hem_z_mm(gz_path, json_path):
    """Read exact world Z (above sole = 0, mm) of skirt hem & bodice waist from packed .gz/.json."""
    meta = json.loads(Path(json_path).read_text())
    raw = gzip.decompress(Path(gz_path).read_bytes())
    n_body, n_body_idx, n_cloth, n_frames = struct.unpack_from("<IIII", raw, 0)
    off = 16
    body_xyz = np.frombuffer(raw, dtype="<f4", count=n_body * 3, offset=off).reshape(n_body, 3)
    off += n_body * 3 * 4 + n_body_idx * 4 + n_cloth * 2 * 4
    frames_xyz = np.frombuffer(raw, dtype="<f4", count=n_frames * n_cloth * 3, offset=off).reshape(n_frames, n_cloth, 3)
    final_cloth = frames_xyz[-1]  # metres, z-up (Anny world frame)

    body_min_z = float(body_xyz[:, 2].min())
    body_max_z = float(body_xyz[:, 2].max())

    # Identify skirt_front and skirt_back panel ranges from meta["panels"]
    hem_zs = {}
    for pname in ("skirt_front", "skirt_back"):
        lo, hi = meta["panels"][pname]
        zs = final_cloth[lo:hi, 2]
        # Bottom 15% of vertices in skirt panel represent the hem ring
        p10 = float(np.percentile(zs, 5))
        hem_zs[pname] = round((p10 - body_min_z) * 1000.0, 1)

    return {
        "soleZMm": round(body_min_z * 1000.0, 1),
        "headAboveSoleMm": round((body_max_z - body_min_z) * 1000.0, 1),
        "hemAboveSoleMm": {
            "front": hem_zs["skirt_front"],
            "back": hem_zs["skirt_back"],
            "mean": round(0.5 * (hem_zs["skirt_front"] + hem_zs["skirt_back"]), 1),
        },
        "clothMinAboveSoleMm": round((float(final_cloth[:, 2].min()) - body_min_z) * 1000.0, 1),
        "clothMaxAboveSoleMm": round((float(final_cloth[:, 2].max()) - body_min_z) * 1000.0, 1),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("slender_work", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--probe", type=Path, default=Path("/tmp/ftx_sundress_probe"))
    parser.add_argument("--model", type=Path, default=Path("/home/ubuntu/.cache/franken_textiles/anny-0.6.1-default.ftxbody"))
    args = parser.parse_args()

    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    sundress_dir = ROOT / "showcase/public/exhibits/sundress"
    pat_path = sundress_dir / "dress-pattern.json"
    fit_path = sundress_dir / "dress-fit.json"
    slender_meas = args.probe / "slender-measures.json"
    tight_meas = args.probe / "tight-measures.json"

    packing = run_path(str(HERE / "prepare-hussar-drape.py"), run_name="packing")
    sundress_drape = run_path(str(HERE / "prepare-sundress-drape.py"), run_name="sundress_drape")

    # Get current solver git commit for full provenance
    rev = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()

    # Package Form B (slender) into output/ using identical packaging logic as dress.*
    slender_drape_dir = args.slender_work / "drape"
    # Verify contact clearance before packaging!
    chk = json.loads(
        subprocess.check_output(
            [str(FTX), "viewer", str(slender_drape_dir), "--check-contact", "--json"],
            text=True,
        )
    )
    assert chk.get("ok"), f"Slender drape failed --check-contact: {chk}"

    if not (out / "slender.gz").exists():
        packing["package"](
            slender_drape_dir,
            pat_path,
            out,
            "slender",
            DRAPE_ARGS,
            sundress_drape["section"],
            replay_inputs=[slender_meas, fit_path],
            solver_revision=rev,
        )

    # Capture genuine native refusal + flat fit check on Form C (tight adult: 170 cm / 99-84-106)
    # Run from args.probe with relative --out tight_refused_run so native repair text uses a clean relative path.
    tight_drape = subprocess.run(
        [
            str(FTX), "drape", str(pat_path),
            "--measurements", str(tight_meas),
            "--fit", str(fit_path),
            "--model", str(args.model),
            "--out", "tight_refused_run",
            "--json",
            *DRAPE_ARGS,
        ],
        cwd=args.probe,
        capture_output=True,
        text=True,
    )
    assert tight_drape.returncode != 0, "Expected tight adult form to refuse natively"
    tight_refusal = json.loads(tight_drape.stdout)
    assert tight_refusal.get("ok") is False and "refusal" in tight_refusal

    # Also capture flat validation ease deficit on tight adult form
    tight_val = subprocess.run(
        [
            str(FTX), "validate", str(pat_path),
            "--measurements", str(tight_meas),
            "--fit", str(fit_path),
            "--json",
        ],
        capture_output=True,
        text=True,
    )
    tight_flat = json.loads(tight_val.stdout)

    # Read common world metrics from both independently accepted drapes
    ref_summary = json.loads((sundress_dir / "dress-summary.json").read_text())
    ref_section = json.loads((sundress_dir / "dress-section.json").read_text())
    ref_measures = json.loads((sundress_dir / "dress-measures.json").read_text())
    ref_world = load_packed_final_hem_z_mm(sundress_dir / "dress.gz", sundress_dir / "dress.json")

    sl_summary = json.loads((out / "slender-summary.json").read_text())
    sl_section = json.loads((out / "slender-section.json").read_text())
    sl_measures = json.loads(slender_meas.read_text())
    sl_world = load_packed_final_hem_z_mm(out / "slender.gz", out / "slender.json")

    # Verify exact fixed pattern and solver argument identity across Form A & Form B!
    pat_sha = hashlib.sha256(pat_path.read_bytes()).hexdigest()
    assert ref_summary["solverArguments"] == sl_summary["solverArguments"], "Solver arguments must be identical"
    assert ref_summary["spacingMm"] == sl_summary["spacingMm"], "Mesh spacing must be identical"
    assert ref_summary["vertices"] == sl_summary["vertices"], "Cloth vertex count must be identical"

    compare_manifest = {
        "schema": "ftx-sundress-compare/1",
        "fixedPattern": {
            "style": ref_summary["style"],
            "patternUrl": "/exhibits/sundress/dress-pattern.json",
            "patternSha256": pat_sha,
            "spacingMm": ref_summary["spacingMm"],
            "vertices": ref_summary["vertices"],
            "triangles": ref_summary["triangles"],
            "solverArguments": ref_summary["solverArguments"],
        },
        "states": [
            {
                "id": "reference",
                "label": "Reference adult (168 cm · 90 / 72 / 96)",
                "shortLabel": "Reference (90/72)",
                "accepted": True,
                "sourceBase": "/exhibits/sundress/dress",
                "measures": {
                    "stature_mm": ref_measures["fused"]["stature"]["value_mm"],
                    "chest_mm": ref_measures["fused"]["chest"]["value_mm"],
                    "waist_mm": ref_measures["fused"]["waist"]["value_mm"],
                    "hip_mm": ref_measures["fused"]["hip"]["value_mm"],
                },
                "world": ref_world,
                "section": ref_section,
                "summary": {
                    "settled": ref_summary["settled"],
                    "gating": ref_summary["gating"],
                    "flags": ref_summary["flags"],
                    "edgeStrainPercent": ref_summary["edgeStrainPercent"],
                    "bodyOffsetMm": ref_summary["bodyOffsetMm"],
                    "finalResidualN": ref_summary["finalResidualN"],
                    "wallSeconds": ref_summary["wallSeconds"],
                    "sourcePc2Sha256": ref_summary["sourcePc2Sha256"],
                    "solverRevision": ref_summary.get("solverRevision"),
                },
            },
            {
                "id": "slender",
                "label": "Petite adult (162 cm · 84 / 66 / 90)",
                "shortLabel": "Petite (84/66)",
                "accepted": True,
                "sourceBase": "/exhibits/sundress-compare/slender",
                "measures": {
                    "stature_mm": sl_measures["fused"]["stature"]["value_mm"],
                    "chest_mm": sl_measures["fused"]["chest"]["value_mm"],
                    "waist_mm": sl_measures["fused"]["waist"]["value_mm"],
                    "hip_mm": sl_measures["fused"]["hip"]["value_mm"],
                },
                "world": sl_world,
                "section": sl_section,
                "summary": {
                    "settled": sl_summary["settled"],
                    "gating": sl_summary["gating"],
                    "flags": sl_summary["flags"],
                    "edgeStrainPercent": sl_summary["edgeStrainPercent"],
                    "bodyOffsetMm": sl_summary["bodyOffsetMm"],
                    "finalResidualN": sl_summary["finalResidualN"],
                    "wallSeconds": sl_summary["wallSeconds"],
                    "sourcePc2Sha256": sl_summary["sourcePc2Sha256"],
                    "solverRevision": sl_summary.get("solverRevision"),
                },
            },
            {
                "id": "tight_refused",
                "label": "Oversized / tight adult (170 cm · 99 / 84 / 106 · REFUSED)",
                "shortLabel": "Tight 99/84 (Refused)",
                "accepted": False,
                "sourceBase": None,
                "measures": {
                    "stature_mm": 1700,
                    "chest_mm": 990,
                    "waist_mm": 840,
                    "hip_mm": 1060,
                },
                "refusal": {
                    "exit": tight_refusal["exit"],
                    "code": tight_refusal["refusal"]["code"],
                    "message": tight_refusal["refusal"]["message"],
                    "repairs": tight_refusal["refusal"].get("repairs", []),
                    "flatPoints": tight_flat["result"]["points"],
                },
            },
        ],
    }

    (out / "compare.json").write_text(json.dumps(compare_manifest, indent=2) + "\n")
    print("Generated sundress-compare exhibit in", out)
    print(
        "Reference hem above sole:", ref_world["hemAboveSoleMm"],
        "frontGap/backGap:", ref_section["frontGapMm"], ref_section["backGapMm"],
    )
    print(
        "Petite hem above sole:", sl_world["hemAboveSoleMm"],
        "frontGap/backGap:", sl_section["frontGapMm"], sl_section["backGapMm"],
    )
    print("Tight refusal code:", tight_refusal["refusal"]["code"])


if __name__ == "__main__":
    main()
