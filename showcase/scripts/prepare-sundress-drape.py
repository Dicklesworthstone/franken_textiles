"""Drape the garden sundress on its synthetic reference form and package it for the fitting room.

From the repository root:
  python3 showcase/scripts/prepare-sundress-drape.py NEW_WORK_DIR OUTPUT_DIR --solve --model MODEL.ftxbody
  python3 showcase/scripts/prepare-sundress-drape.py NEW_WORK_DIR OUTPUT_DIR --lightly-eased --solve --model MODEL.ftxbody
  python3 showcase/scripts/prepare-sundress-drape.py WORK_DIR OUTPUT_DIR          # repackage a finished drape

The pattern, the form's tapes and the fit points are those of prepare-sundress.py, unchanged. The
packing and the command line are prepare-hussar-drape.py's. Never use a real wearer's workspace.
"""
import argparse
from pathlib import Path
from runpy import run_path

HERE = Path(__file__).resolve().parent
dress = run_path(str(HERE / "prepare-sundress.py"), run_name="sundress")
packing = run_path(str(HERE / "prepare-hussar-drape.py"), run_name="packing")

# At a 220 mm skirt radius the 20 mm start has 72 crossing pairs. At 270 mm the
# same four cut pieces pass the audit and can sew with the cloth contact barrier active.
START_RADIUS = {"bodice": 177, "skirt": 270}
DRAPE_ARGS = ["--spacing-mm", "20", "--time-subdivision", "--arm-deg", "22", "--drape", "150", "--max-settle-steps", "256", "--self-contact-mm", "1"]


def inputs(gather_ratio=1.3):
    pattern = dress["dress"](gather_ratio)
    for piece in pattern["pieces"]:
        piece["placement"]["radius_mm"] = START_RADIUS[piece["id"].split("_")[0]]
    return {"pattern.json": pattern, "measures.json": dress["body"](), "fit.json": dress["fit_spec"]()}

def section(body, body_faces, cloth, triangles, mesh, report):
    waist_edges = [mesh["seams"]["gather_" + side]["side_a"] for side in ("front", "back")]
    waist_height = report["body"]["fit"]["tape"]["waist_height"]["body_mm"]
    measure = run_path(str(HERE / "prepare-skirt-atelier.py"))["waist_section"]
    return measure(body, body_faces, cloth, triangles, waist_edges, waist_height, height_above_sole_mm=waist_height)

if __name__ == "__main__":
    cuts = argparse.ArgumentParser(add_help=False)
    cuts.add_argument("--lightly-eased", action="store_true")
    options, rest = cuts.parse_known_args()
    name, ratio = ("light", 1.05) if options.lightly_eased else ("dress", 1.3)
    packing["solve_and_package"](__doc__, inputs(ratio), DRAPE_ARGS, name, section, rest)
