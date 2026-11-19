"""Drape the garden sundress on its synthetic reference form and package it for the fitting room.

From the repository root:
  python3 showcase/scripts/prepare-sundress-drape.py NEW_WORK_DIR OUTPUT_DIR --solve --model MODEL.ftxbody
  python3 showcase/scripts/prepare-sundress-drape.py WORK_DIR OUTPUT_DIR          # repackage a finished drape

The pattern, the form's tapes and the fit points are those of prepare-sundress.py, unchanged. The
packing and the command line are prepare-hussar-drape.py's. Never use a real wearer's workspace.
"""
from pathlib import Path
from runpy import run_path

HERE = Path(__file__).resolve().parent
dress = run_path(str(HERE / "prepare-sundress.py"), run_name="sundress")
packing = run_path(str(HERE / "prepare-hussar-drape.py"), run_name="packing")

# Wrap radii of the two bodice and two skirt panels (mm): where the cloth starts. The settled skirt is not
# independent of its start. From 255 mm it settles standing well away from the waist front and back; from
# 200 mm the gathers tangle and the solver reports the skirt sliding 62 mm; 220 mm settles clean with the
# smallest step of the three. All three were run at 20 mm with the arguments below.
START_RADIUS = {"bodice": 177, "skirt": 220}
# Cloth collides with the form, not yet with itself: the gathered waist starts with its folds crossed, and
# the solver refuses self-contact from a crossed start. 15 degrees of arm is too close to the trunk for this
# form's posed surface; 22 passes its audit.
DRAPE_ARGS = ["--spacing-mm", "20", "--time-subdivision", "--arm-deg", "22", "--drape", "150", "--max-settle-steps", "256"]


def inputs():
    pattern = dress["dress"]()
    for piece in pattern["pieces"]:
        piece["placement"]["radius_mm"] = START_RADIUS[piece["id"].split("_")[0]]
    return {"pattern.json": pattern, "measures.json": dress["body"](), "fit.json": dress["fit_spec"]()}


if __name__ == "__main__":
    packing["solve_and_package"](__doc__, inputs(), DRAPE_ARGS, "dress")
