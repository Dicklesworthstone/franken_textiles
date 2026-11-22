"""Draft, validate, and export the documented 1780s waistcoat reconstruction (V&A 256-1880 / O112019)
for both the waistcoat construction exhibit (ftx-n4x) and the inside-out lined-garment explorer (ftx-z55).

Collection provenance (verified from V&A Museum public catalogue API):
  - Museum: Victoria and Albert Museum, London
  - Accession number: 256-1880 (system number O112019)
  - Catalogue URL: https://collections.vam.ac.uk/item/O112019/
  - Date / origin: 1780s, Great Britain or France (possibly)
  - Published dimensions: chest circumference 107.5 cm (1075 mm), front length 69 cm (690 mm),
    back length 67 cm (670 mm).
  - Documented construction: sleeveless square-cut fronts of ivory striped silk satin with 12 buttons
    and a pair of welt pockets; plain ivory silk satin facings and collar back; linen back and body
    lining with three pairs of centre-back waist adjustment tapes.

Run from repository root:
  python3 showcase/scripts/prepare-waistcoat.py showcase/public/exhibits/waistcoat
"""
import argparse
import hashlib
import json
import math
import re
import subprocess
import tempfile
from pathlib import Path


PROVENANCE = {
    "institution": "Victoria and Albert Museum, London",
    "accessionNumber": "256-1880",
    "systemNumber": "O112019",
    "catalogUrl": "https://collections.vam.ac.uk/item/O112019/",
    "objectType": "Man's sleeveless waistcoat",
    "date": "1780s",
    "origin": "Great Britain or France (possibly)",
    "materials": "Ivory striped silk satin fronts and front collar; plain ivory silk satin facings and under-collar; unbleached linen back, body lining, pocket bags, and three pairs of back adjustment tapes.",
    "publishedDimensionsMm": {
        "chestCircumference": 1075.0,
        "frontLength": 690.0,
        "backLength": 670.0,
    },
    "reconstructionScope": (
        "Original flat cutting pattern reconstructed in millimetres from the V&A curatorial description and "
        "published chest/front/back dimensions of 256-1880 (O112019), not a direct trace of the surviving museum "
        "garment. Test the cut in a toile before cutting silk."
    ),
}


def cubic_controls(a, b, p, q):
    """Convert absolute cubic Bezier handles p, q into fabric-pattern/1 chord-relative (u, v)."""
    dx, dy = b[0] - a[0], b[1] - a[1]
    l2 = dx * dx + dy * dy
    return {
        "type": "cubic",
        "c": [
            [
                round(((x - a[0]) * dx + (y - a[1]) * dy) / l2, 6),
                round(((y - a[1]) * dx - (x - a[0]) * dy) / l2, 6),
            ]
            for x, y in (p, q)
        ],
    }


def cubic_length(p0, p1, p2, p3, n=400):
    pts = []
    for i in range(n + 1):
        t = i / n
        u = 1.0 - t
        x = u**3 * p0[0] + 3 * u**2 * t * p1[0] + 3 * u * t**2 * p2[0] + t**3 * p3[0]
        y = u**3 * p0[1] + 3 * u**2 * t * p1[1] + 3 * u * t**2 * p2[1] + t**3 * p3[1]
        pts.append((x, y))
    return sum(math.hypot(pts[i + 1][0] - pts[i][0], pts[i + 1][1] - pts[i][1]) for i in range(n))


def waistcoat_pattern():
    # Exact published V&A 256-1880 dimensions:
    # - Half-chest = 275.0 (front) + 262.5 (back) = 537.5 mm -> full chest = 1075.0 mm (107.5 cm)
    # - Front length = 690.0 mm (69.0 cm, y=0 square hem to y=690 high neck-shoulder point)
    # - Back length  = 670.0 mm (67.0 cm, y=0 square hem to y=670 centre-back neck)
    #
    # Corner truing discipline: every seam corner is squared cleanly (90° + 90° = 180.0°)
    # so native `ftx validate` achieves CONVERGED with 0 seam failures and 0 truing failures.
    # Shoulder vector from snp (82, 690) to sh_tip (212, 646): d_sh = (130, -44), len = 137.2443 mm.
    # Unit downward perpendicular to shoulder (pointing into the garment):
    #   n_down = (-44, -130) / 137.2443 = (-0.320596, -0.947216)
    sh_len = math.hypot(130.0, -44.0)
    nx_dn, ny_dn = -44.0 / sh_len, -130.0 / sh_len

    # Front neckline: snp (82, 690) -> cf_neck (0, 582)
    # Leaves snp perpendicular to shoulder (n_down * 36 mm); arrives at cf_neck horizontally (-1, 0) (90° to vertical CF).
    fn_p0 = (82.0, 690.0)
    fn_p1 = (82.0 + nx_dn * 36.0, 690.0 + ny_dn * 36.0)
    fn_p2 = (38.0, 582.0)
    fn_p3 = (0.0, 582.0)
    front_neck_curve = cubic_controls(fn_p0, fn_p3, fn_p1, fn_p2)
    front_neck_len = cubic_length(fn_p0, fn_p1, fn_p2, fn_p3)

    # Front armscye: arm_side (275, 435) -> sh_tip (212, 646)
    # Leaves arm_side horizontally LEFT (-1, 0) (90° to vertical side seam);
    # arrives at sh_tip along -n_down (perpendicular to shoulder -> 90° corner at shoulder tip!).
    fa_p0 = (275.0, 435.0)
    fa_p1 = (200.0, 435.0)
    fa_p2 = (212.0 + nx_dn * 65.0, 646.0 + ny_dn * 65.0)
    fa_p3 = (212.0, 646.0)
    front_arm_curve = cubic_controls(fa_p0, fa_p3, fa_p1, fa_p2)

    # Back neckline & armscye in standard CCW orientation (x in [-262.5, 0], CB at x=0, side at x=-262.5):
    # Shoulder on back goes from snp (-82, 690) to sh_tip (-212, 646): d_sh_b = (-130, -44).
    # Unit downward perpendicular into back piece: (+44, -130) / sh_len = (-nx_dn, ny_dn).
    bnx_dn, bny_dn = -nx_dn, ny_dn

    # Back neckline: cb_neck (0, 670) -> snp (-82, 690)
    # Leaves cb_neck horizontally LEFT (-1, 0) (90° to vertical CB -> 180° across CB!);
    # arrives at snp along (-bnx_dn, -bny_dn) (90° to shoulder -> 180° across shoulder at SNP!).
    bn_p0 = (0.0, 670.0)
    bn_p1 = (-36.0, 670.0)
    bn_p2 = (-82.0 + bnx_dn * 18.0, 690.0 + bny_dn * 18.0)
    bn_p3 = (-82.0, 690.0)
    back_neck_curve = cubic_controls(bn_p0, bn_p3, bn_p1, bn_p2)
    back_neck_len = cubic_length(bn_p0, bn_p1, bn_p2, bn_p3)

    # Back armscye: sh_tip (-212, 646) -> arm_side (-262.5, 435)
    # Leaves sh_tip along (bnx_dn, bny_dn) (90° to shoulder -> 180° at shoulder tip!);
    # arrives at arm_side horizontally LEFT (-1, 0) (90° to vertical side seam -> 180° across side!).
    ba_p0 = (-212.0, 646.0)
    ba_p1 = (-212.0 + bnx_dn * 65.0, 646.0 + bny_dn * 65.0)
    ba_p2 = (-196.0, 435.0)
    ba_p3 = (-262.5, 435.0)
    back_arm_curve = cubic_controls(ba_p0, ba_p3, ba_p1, ba_p2)

    # Collar exact stations matching front + back necklines to 0.01 mm:
    c_f = round(front_neck_len, 2)
    c_b = round(back_neck_len, 2)
    c_half = round(c_f + c_b, 2)
    c_h = 48.0  # 1780s 48 mm stand collar height

    # Twelve covered button / buttonhole stations along centre front (V&A 256-1880 record):
    # Pocket welt slash sits at y = 176 mm (box y = 164..188 mm) so 140 mm deep linen pocket bags
    # end cleanly at y = 36 mm above the square hem (y = 0 mm).
    button_ys = [round(36.0 + i * (528.0 / 11.0), 1) for i in range(12)]
    front_internals = [
        {"kind": "mark", "label": "Pocket welt slash (136 mm cut line)", "pts": [[96, 176], [232, 176]]},
        {"kind": "mark", "label": "Pocket welt upper stitch line (12 mm above slash)", "pts": [[96, 188], [232, 188]]},
        {"kind": "mark", "label": "Pocket welt lower stitch line (12 mm below slash)", "pts": [[96, 164], [232, 164]]},
        {"kind": "mark", "label": "Pocket left corner clip triangle", "pts": [[108, 188], [96, 176], [108, 164]]},
        {"kind": "mark", "label": "Pocket right corner clip triangle", "pts": [[220, 188], [232, 176], [220, 164]]},
        {"kind": "mark", "label": "Facing-to-lining inner seam reference (x = 82 mm)", "pts": [[82, 0], [82, 690]]},
    ] + [
        {"kind": "mark", "label": f"Button / buttonhole #{i + 1}", "pts": [[14, y], [30, y]]}
        for i, y in enumerate(button_ys)
    ]

    # Three pairs of centre-back linen waist-adjustment tapes documented in V&A 256-1880:
    back_internals = [
        {"kind": "mark", "label": "Upper waist-adjustment tape station", "pts": [[-45, 270], [0, 270]]},
        {"kind": "mark", "label": "Middle waist-adjustment tape station", "pts": [[-45, 210], [0, 210]]},
        {"kind": "mark", "label": "Lower waist-adjustment tape station", "pts": [[-45, 150], [0, 150]]},
    ]

    pieces = [
        {
            "id": "wc_front",
            "name": "Waistcoat front (shell)",
            "cut": {"qty": 2, "material": "ivory striped silk satin"},
            "vertices": {
                "cf_hem": [0, 0],
                "hem_split": [82, 0],
                "side_hem": [275, 0],
                "side_arm": [275, 435],
                "sh_tip": [212, 646],
                "snp": [82, 690],
                "cf_neck": [0, 582],
            },
            "edges": [
                {"id": "hem_facing", "from": "cf_hem", "to": "hem_split"},
                {"id": "hem_lining", "from": "hem_split", "to": "side_hem"},
                {"id": "side", "from": "side_hem", "to": "side_arm"},
                {"id": "armscye", "from": "side_arm", "to": "sh_tip", "curve": front_arm_curve},
                {"id": "shoulder", "from": "sh_tip", "to": "snp"},
                {"id": "neck", "from": "snp", "to": "cf_neck", "curve": front_neck_curve},
                {"id": "cf_edge", "from": "cf_neck", "to": "cf_hem"},
            ],
            "grain": [[145, 220], [145, 560]],
            "internal": front_internals,
            "notches": [
                {"id": "pocket_level", "edge": "side", "s": round(176.0 / 435.0, 4), "type": "V"},
                {"id": "waist_level", "edge": "side", "s": round(240.0 / 435.0, 4), "type": "V"},
            ],
            "note": "Shell front (V&A 256-1880). Slash 136 mm pocket welt opening and clip corners cleanly to stitch line before turning pocket bags inside.",
        },
        {
            "id": "wc_facing",
            "name": "Front facing (silk satin)",
            "cut": {"qty": 2, "material": "plain ivory silk satin"},
            "vertices": {
                "cf_hem": [0, 0],
                "inner_hem": [82, 0],
                "snp": [82, 690],
                "cf_neck": [0, 582],
            },
            "edges": [
                {"id": "hem", "from": "cf_hem", "to": "inner_hem"},
                {"id": "lining_join", "from": "inner_hem", "to": "snp"},
                {"id": "neck", "from": "snp", "to": "cf_neck", "curve": front_neck_curve},
                {"id": "cf_edge", "from": "cf_neck", "to": "cf_hem"},
            ],
            "grain": [[41, 140], [41, 500]],
            "notches": [
                {"id": "facing_mid", "edge": "lining_join", "s": 0.5, "type": "V"},
            ],
            "note": "Plain ivory silk satin inside front facing. Joins shell at CF, neck, and lower hem, and joins linen front lining along inner edge.",
        },
        {
            "id": "wc_front_lining",
            "name": "Front body lining (linen)",
            "cut": {"qty": 2, "material": "unbleached linen"},
            "vertices": {
                "inner_hem": [82, 0],
                "side_hem": [275, 0],
                "side_arm": [275, 435],
                "sh_tip": [212, 646],
                "snp": [82, 690],
            },
            "edges": [
                {"id": "hem", "from": "inner_hem", "to": "side_hem"},
                {"id": "side", "from": "side_hem", "to": "side_arm"},
                {"id": "armscye", "from": "side_arm", "to": "sh_tip", "curve": front_arm_curve},
                {"id": "shoulder", "from": "sh_tip", "to": "snp"},
                {"id": "facing_join", "from": "snp", "to": "inner_hem"},
            ],
            "grain": [[175, 160], [175, 520]],
            "notches": [
                {"id": "facing_mid", "edge": "facing_join", "s": 0.5, "type": "V"},
                {"id": "waist_level", "edge": "side", "s": round(240.0 / 435.0, 4), "type": "V"},
            ],
            "note": "Unbleached linen front lining. Sits directly behind the pocket bag assembly and joins plain silk facing along x = 82 mm.",
        },
        {
            "id": "wc_back",
            "name": "Waistcoat back (linen outer)",
            "cut": {"qty": 2, "material": "unbleached linen"},
            "vertices": {
                "side_hem": [-262.5, 0],
                "cb_hem": [-16, 0],
                "cb_vent_in": [-16, 120],
                "cb_vent_top": [0, 120],
                "cb_neck": [0, 670],
                "snp": [-82, 690],
                "sh_tip": [-212, 646],
                "side_arm": [-262.5, 435],
            },
            "edges": [
                {"id": "hem", "from": "side_hem", "to": "cb_hem"},
                {"id": "cb_vent_edge", "from": "cb_hem", "to": "cb_vent_in"},
                {"id": "cb_vent_step", "from": "cb_vent_in", "to": "cb_vent_top"},
                {"id": "cb_upper", "from": "cb_vent_top", "to": "cb_neck"},
                {"id": "neck", "from": "cb_neck", "to": "snp", "curve": back_neck_curve},
                {"id": "shoulder", "from": "snp", "to": "sh_tip"},
                {"id": "armscye", "from": "sh_tip", "to": "side_arm", "curve": back_arm_curve},
                {"id": "side", "from": "side_arm", "to": "side_hem"},
            ],
            "grain": [[-130, 140], [-130, 560]],
            "internal": back_internals,
            "notches": [
                {"id": "waist_level", "edge": "side", "s": round((435.0 - 240.0) / 435.0, 4), "type": "V"},
                {"id": "pocket_level", "edge": "side", "s": round((435.0 - 176.0) / 435.0, 4), "type": "V"},
                {"id": "cb_mid", "edge": "cb_upper", "s": 0.5, "type": "double"},
            ],
            "note": "Linen back (670 mm CB length). Seam upper CB above y = 120 mm; leave lower 120 mm notched vent open and attach three pairs of linen adjustment tapes.",
        },
        {
            "id": "wc_back_lining",
            "name": "Back body lining (linen)",
            "cut": {"qty": 2, "material": "unbleached linen"},
            "vertices": {
                "side_hem": [-262.5, 0],
                "cb_hem": [-16, 0],
                "cb_vent_in": [-16, 120],
                "cb_vent_top": [0, 120],
                "cb_neck": [0, 670],
                "snp": [-82, 690],
                "sh_tip": [-212, 646],
                "side_arm": [-262.5, 435],
            },
            "edges": [
                {"id": "hem", "from": "side_hem", "to": "cb_hem"},
                {"id": "cb_vent_edge", "from": "cb_hem", "to": "cb_vent_in"},
                {"id": "cb_vent_step", "from": "cb_vent_in", "to": "cb_vent_top"},
                {"id": "cb_upper", "from": "cb_vent_top", "to": "cb_neck"},
                {"id": "neck", "from": "cb_neck", "to": "snp", "curve": back_neck_curve},
                {"id": "shoulder", "from": "snp", "to": "sh_tip"},
                {"id": "armscye", "from": "sh_tip", "to": "side_arm", "curve": back_arm_curve},
                {"id": "side", "from": "side_arm", "to": "side_hem"},
            ],
            "grain": [[-130, 140], [-130, 560]],
            "notches": [
                {"id": "waist_level", "edge": "side", "s": round((435.0 - 240.0) / 435.0, 4), "type": "V"},
                {"id": "cb_mid", "edge": "cb_upper", "s": 0.5, "type": "double"},
            ],
            "note": "Unbleached linen back lining. Bags armscye and lower hem cleanly with back outer, leaving lower 120 mm CB vent open.",
        },
        {
            "id": "wc_collar",
            "name": "Stand collar (outer shell)",
            "cut": {"qty": 1, "material": "ivory striped silk / plain silk satin"},
            "vertices": {
                "fl": [-c_half, 0],
                "sl": [-c_b, 0],
                "cb": [0, 0],
                "sr": [c_b, 0],
                "fr": [c_half, 0],
                "fr_t": [c_half, c_h],
                "fl_t": [-c_half, c_h],
            },
            "edges": [
                {"id": "front_L", "from": "fl", "to": "sl"},
                {"id": "back_L", "from": "sl", "to": "cb"},
                {"id": "back_R", "from": "cb", "to": "sr"},
                {"id": "front_R", "from": "sr", "to": "fr"},
                {"id": "end_R", "from": "fr", "to": "fr_t"},
                {"id": "top", "from": "fr_t", "to": "fl_t"},
                {"id": "end_L", "from": "fl_t", "to": "fl"},
            ],
            "grain": [[-c_half * 0.7, c_h * 0.5], [c_half * 0.7, c_h * 0.5]],
            "internal": [
                {"kind": "mark", "label": "Centre-back collar station", "pts": [[0, 0], [0, c_h]]},
                {"kind": "mark", "label": "Left shoulder-neck station", "pts": [[-c_b, 0], [-c_b, 14]]},
                {"kind": "mark", "label": "Right shoulder-neck station", "pts": [[c_b, 0], [c_b, 14]]},
            ],
            "note": "1780s 48 mm standing collar. No separate undercollar canvas/interlining piece is drafted in this pattern (surfaced explicitly as missing collar lining).",
        },
        {
            "id": "wc_welt",
            "name": "Pocket welt strip (fold)",
            "cut": {"qty": 2, "material": "ivory silk satin"},
            "vertices": {
                "w0": [0, 0],
                "w1": [136, 0],
                "w2": [136, 48],
                "w3": [0, 48],
            },
            "edges": [
                {"id": "bag_join", "from": "w0", "to": "w1"},
                {"id": "end_R", "from": "w1", "to": "w2"},
                {"id": "slit_attach", "from": "w2", "to": "w3"},
                {"id": "end_L", "from": "w3", "to": "w0"},
            ],
            "grain": [[16, 24], [120, 24]],
            "internal": [
                {"kind": "fold", "label": "Welt lengthwise fold (finished lip 24 mm)", "pts": [[0, 24], [136, 24]]},
            ],
            "note": "Cut 136 x 48 mm, fold lengthwise to 24 mm lip, stitch along lower welt line of the 136 mm front slash, turn inside, and catch ends cleanly to clipped corners.",
        },
        {
            "id": "wc_pocket_outer",
            "name": "Pocket bag (lower / welt side)",
            "cut": {"qty": 2, "material": "unbleached linen"},
            "vertices": {
                "p0": [0, 0],
                "p1": [136, 0],
                "p2": [136, 140],
                "p3": [0, 140],
            },
            "edges": [
                {"id": "bottom", "from": "p0", "to": "p1"},
                {"id": "right", "from": "p1", "to": "p2"},
                {"id": "top", "from": "p2", "to": "p3"},
                {"id": "left", "from": "p3", "to": "p0"},
            ],
            "grain": [[68, 22], [68, 118]],
            "note": "Lower linen pocket bag (136 x 140 mm, hanging from y = 176 mm to 36 mm above the hem). Joins pocket welt along top edge (136 mm) and closes around left, bottom, and right edges with upper pocket bag.",
        },
        {
            "id": "wc_pocket_inner",
            "name": "Pocket bag (upper / body side)",
            "cut": {"qty": 2, "material": "unbleached linen"},
            "vertices": {
                "p0": [0, 0],
                "p1": [136, 0],
                "p2": [136, 140],
                "p3": [0, 140],
            },
            "edges": [
                {"id": "bottom", "from": "p0", "to": "p1"},
                {"id": "right", "from": "p1", "to": "p2"},
                {"id": "slit_top", "from": "p2", "to": "p3"},
                {"id": "left", "from": "p3", "to": "p0"},
            ],
            "grain": [[68, 22], [68, 118]],
            "note": "Upper linen pocket bag (136 x 140 mm). Top 136 mm edge stitches directly to upper seam allowance of the 136 mm internal slash on waistcoat front; perimeter closes to lower bag cleanly above the hem.",
        },
    ]

    # Exact native seams (0-based indices preserved verbatim across ftx-n4x and ftx-z55!).
    # Note on orientation: mirrored `.L` pieces flip x and reverse loop winding so every piece stays CCW.
    # For mirrored pairs joining a symmetric partner, we verify every reverse flag directly via `ftx validate`.
    seams = [
        # Shell main body seams (indices 0..8)
        {"id": "shell_side_R", "a": "wc_front.R/side", "b": "wc_back.R/side", "reverse": True},
        {"id": "shell_side_L", "a": "wc_front.L/side", "b": "wc_back.L/side", "reverse": True},
        {"id": "shell_shoulder_R", "a": "wc_front.R/shoulder", "b": "wc_back.R/shoulder", "reverse": True},
        {"id": "shell_shoulder_L", "a": "wc_front.L/shoulder", "b": "wc_back.L/shoulder", "reverse": True},
        {"id": "shell_cb_upper", "a": "wc_back.R/cb_upper", "b": "wc_back.L/cb_upper", "reverse": True},
        {"id": "collar_front_R", "a": "wc_collar/front_R", "b": "wc_front.R/neck", "reverse": False},
        {"id": "collar_back_R", "a": "wc_collar/back_R", "b": "wc_back.R/neck", "reverse": False},
        {"id": "collar_back_L", "a": "wc_collar/back_L", "b": "wc_back.L/neck", "reverse": True},
        {"id": "collar_front_L", "a": "wc_collar/front_L", "b": "wc_front.L/neck", "reverse": True},

        # Facing-to-shell and facing-to-lining seams (indices 9..14)
        # Note: wc_facing/neck and wc_back_lining/neck stay honestly unpaired because no separate
        # undercollar lining piece is drafted (surfaced explicitly as MISSING_LINING in ftx-z55).
        {"id": "facing_cf_R", "a": "wc_front.R/cf_edge", "b": "wc_facing.R/cf_edge", "reverse": True},
        {"id": "facing_cf_L", "a": "wc_front.L/cf_edge", "b": "wc_facing.L/cf_edge", "reverse": True},
        {"id": "facing_hem_R", "a": "wc_front.R/hem_facing", "b": "wc_facing.R/hem", "reverse": True},
        {"id": "facing_hem_L", "a": "wc_front.L/hem_facing", "b": "wc_facing.L/hem", "reverse": True},
        {"id": "facing_to_lining_R", "a": "wc_facing.R/lining_join", "b": "wc_front_lining.R/facing_join", "reverse": True},
        {"id": "facing_to_lining_L", "a": "wc_facing.L/lining_join", "b": "wc_front_lining.L/facing_join", "reverse": True},

        # Lining interior body seams (indices 15..19)
        {"id": "lining_side_R", "a": "wc_front_lining.R/side", "b": "wc_back_lining.R/side", "reverse": True},
        {"id": "lining_side_L", "a": "wc_front_lining.L/side", "b": "wc_back_lining.L/side", "reverse": True},
        {"id": "lining_shoulder_R", "a": "wc_front_lining.R/shoulder", "b": "wc_back_lining.R/shoulder", "reverse": True},
        {"id": "lining_shoulder_L", "a": "wc_front_lining.L/shoulder", "b": "wc_back_lining.L/shoulder", "reverse": True},
        {"id": "lining_cb_upper", "a": "wc_back_lining.R/cb_upper", "b": "wc_back_lining.L/cb_upper", "reverse": True},

        # Shell-to-lining bagging seams (armscye and hem: indices 20..27)
        {"id": "bag_hem_front_R", "a": "wc_front.R/hem_lining", "b": "wc_front_lining.R/hem", "reverse": True},
        {"id": "bag_hem_front_L", "a": "wc_front.L/hem_lining", "b": "wc_front_lining.L/hem", "reverse": True},
        {"id": "bag_hem_back_R", "a": "wc_back.R/hem", "b": "wc_back_lining.R/hem", "reverse": True},
        {"id": "bag_hem_back_L", "a": "wc_back.L/hem", "b": "wc_back_lining.L/hem", "reverse": True},
        {"id": "bag_armscye_front_R", "a": "wc_front.R/armscye", "b": "wc_front_lining.R/armscye", "reverse": True},
        {"id": "bag_armscye_front_L", "a": "wc_front.L/armscye", "b": "wc_front_lining.L/armscye", "reverse": True},
        {"id": "bag_armscye_back_R", "a": "wc_back.R/armscye", "b": "wc_back_lining.R/armscye", "reverse": True},
        {"id": "bag_armscye_back_L", "a": "wc_back.L/armscye", "b": "wc_back_lining.L/armscye", "reverse": True},

        # Pocket welt & bag assembly seams (indices 28..35)
        {"id": "welt_to_bag_R", "a": "wc_welt.R/bag_join", "b": "wc_pocket_outer.R/top", "reverse": True},
        {"id": "welt_to_bag_L", "a": "wc_welt.L/bag_join", "b": "wc_pocket_outer.L/top", "reverse": True},
        {"id": "pocket_left_R", "a": "wc_pocket_outer.R/left", "b": "wc_pocket_inner.R/left", "reverse": True},
        {"id": "pocket_left_L", "a": "wc_pocket_outer.L/left", "b": "wc_pocket_inner.L/left", "reverse": True},
        {"id": "pocket_bottom_R", "a": "wc_pocket_outer.R/bottom", "b": "wc_pocket_inner.R/bottom", "reverse": True},
        {"id": "pocket_bottom_L", "a": "wc_pocket_outer.L/bottom", "b": "wc_pocket_inner.L/bottom", "reverse": True},
        {"id": "pocket_right_R", "a": "wc_pocket_outer.R/right", "b": "wc_pocket_inner.R/right", "reverse": True},
        {"id": "pocket_right_L", "a": "wc_pocket_outer.L/right", "b": "wc_pocket_inner.L/right", "reverse": True},
    ]

    return {
        "schema": "fabric-pattern/1",
        "units": "mm",
        "style": "WAISTCOAT_1780S_VAM_256_1880",
        "meta": {
            "title": "1780s Men's Waistcoat (after V&A 256-1880)",
            "size_label": "Chest 107.5 cm · Front 69 cm · Back 67 cm",
            "sa_mm": 10,
            "origin": "VECTOR_EDIT",
            "note": (
                "Reconstructed from Victoria and Albert Museum 256-1880 (O112019, 1780s). "
                "Separate ivory silk fronts, plain silk facings, linen back and body lining, stand collar, "
                "and slit welt pockets. Check cut in a toile."
            ),
        },
        "pieces": pieces,
        "seams": seams,
    }


def fit_spec():
    return {
        "schema": "fabric-fit/1",
        "garment": "1780s Men's Waistcoat (after V&A 256-1880)",
        "points": [
            {
                "id": "chest_shell",
                "label": "Finished waistcoat chest girth (V&A 256-1880: 1075 mm)",
                "kind": "path",
                "refs": [
                    "wc_front.R/hem_facing",
                    "wc_front.R/hem_lining",
                    "wc_back.R/hem",
                    "wc_back.R/cb_vent_step",
                    "wc_back.L/cb_vent_step",
                    "wc_back.L/hem",
                    "wc_front.L/hem_lining",
                    "wc_front.L/hem_facing",
                ],
                "body": "chest",
                "target_mm": [55, 75, 105],
            }
        ],
    }


def synthetic_measures():
    return {
        "schema": "fabric-measurements/1",
        "wearer": {"label": "Synthetic adult male (178 cm / C100 W86)", "stature_mm": 1780, "sex": "male", "age_months": 360},
        "fused": {
            "stature": {"value_mm": 1780},
            "chest": {"value_mm": 1000},
            "waist": {"value_mm": 860},
            "hip": {"value_mm": 980},
            "waist_height": {"value_mm": 1070},
            "back_neck_to_waist": {"value_mm": 450},
            "shoulder_slope_deg": {"value_deg": 18.7},
            "neck_base": {"value_mm": 390},
            "shoulder_length": {"value_mm": 137},
            "armscye_depth": {"value_mm": 230},
            "crotch_height": {"value_mm": 816},
            "head": {"value_mm": 580},
            "knee_height": {"value_mm": 498},
            "arm_length": {"value_mm": 599},
            "hip_width": {"value_mm": 350},
        },
    }


LAYER_OF_PIECE = {
    "wc_front": "shell",
    "wc_back": "shell",
    "wc_collar": "shell",
    "wc_welt": "pocket",
    "wc_facing": "facing",
    "wc_front_lining": "lining",
    "wc_back_lining": "lining",
    "wc_pocket_outer": "pocket",
    "wc_pocket_inner": "pocket",
}


def audit_unpaired_and_missing(pat):
    """Inspect every perimeter edge across all cut instances and report unpaired edges & missing linings honestly."""
    paired = set()
    for s in pat["seams"]:
        paired.add(s["a"])
        paired.add(s["b"])

    unpaired = []
    for p in pat["pieces"]:
        insts = [f"{p['id']}.R", f"{p['id']}.L"] if p["cut"]["qty"] == 2 else [p["id"]]
        for inst in insts:
            for e in p["edges"]:
                ref = f"{inst}/{e['id']}"
                if ref not in paired:
                    if p["id"] == "wc_collar":
                        kind = "MISSING_LINING"
                        reason = (
                            "No separate undercollar or buckram interlining piece is drafted in this pattern; "
                            "collar top and front ends remain unbacked rather than inventing a lining panel."
                        )
                    elif e["id"] == "neck" and p["id"] in ("wc_facing", "wc_back_lining"):
                        kind = "MISSING_LINING"
                        reason = (
                            "Inner neckline edge waits to be felled onto an undercollar lining, which is not "
                            "drafted as a separate template in this pattern (shell neckline is already joined to wc_collar)."
                        )
                    elif e["id"] in ("cb_vent_edge", "cb_vent_step"):
                        kind = "UNPAIRED_EDGE"
                        reason = (
                            "Lower 120 mm notched centre-back vent intentionally left open below y = 120 mm to accommodate "
                            "the three pairs of linen waist-adjustment lacing tapes documented on V&A 256-1880."
                        )
                    elif p["id"] in ("wc_welt", "wc_pocket_inner"):
                        kind = "INTERNAL_SLASH_ATTACHMENT"
                        reason = (
                            "Attaches directly to the 136 mm internal welt slash cut inside wc_front (y = 176 mm) "
                            "rather than a perimeter edge; welt ends catch the clipped diagonal slash corners."
                        )
                    else:
                        kind = "UNPAIRED_EDGE"
                        reason = "Open perimeter edge not joined to another template."
                    unpaired.append({
                        "ref": ref,
                        "instance": inst,
                        "piece": p["id"],
                        "edge": e["id"],
                        "layer": LAYER_OF_PIECE[p["id"]],
                        "kind": kind,
                        "reason": reason,
                    })
    return unpaired


def parse_svg_dimensions(svg_text):
    w = re.search(r'\bwidth="([^"]+)"', svg_text)
    h = re.search(r'\bheight="([^"]+)"', svg_text)
    vb = re.search(r'\bviewBox="([^"]+)"', svg_text)
    return {
        "widthAttr": w.group(1) if w else None,
        "heightAttr": h.group(1) if h else None,
        "viewBox": vb.group(1) if vb else None,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--ftx", type=Path, default=Path(__file__).resolve().parents[2] / "target/release/ftx")
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)

    pat = waistcoat_pattern()
    fit = fit_spec()
    meas = synthetic_measures()

    pat_path = out / "waistcoat-pattern.json"
    fit_path = out / "waistcoat-fit.json"
    meas_path = out / "waistcoat-measures.json"
    pat_path.write_text(json.dumps(pat, indent=2) + "\n")
    fit_path.write_text(json.dumps(fit, indent=2) + "\n")
    meas_path.write_text(json.dumps(meas, indent=2) + "\n")

    proc = subprocess.run(
        [str(args.ftx), "validate", str(pat_path), "--measurements", str(meas_path), "--fit", str(fit_path), "--json"],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        print("VALIDATE STDOUT:", proc.stdout)
        print("VALIDATE STDERR:", proc.stderr)
        raise RuntimeError(f"ftx validate failed with exit {proc.returncode}")
    val = json.loads(proc.stdout)
    res = val["result"]
    seams_fail = [s for s in res["seams"] if s["status"] != "PASS"]
    truing_fail = [t for t in res.get("truing", []) if t["status"] != "PASS"]
    if not val.get("ok") or res["status"] != "CONVERGED" or seams_fail or truing_fail:
        print("SEAMS FAIL:", seams_fail)
        print("TRUING FAIL:", truing_fail)
        raise RuntimeError(f"Pattern not CONVERGED: status={res['status']}, seams_fail={len(seams_fail)}, truing_fail={len(truing_fail)}")

    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        for target, suffix, dest_name in [
            ("svg", "_master_mm.svg", "waistcoat-master.svg"),
            ("a4", "_A4.pdf", "waistcoat-A4.pdf"),
        ]:
            d = tmp / target
            subprocess.run(
                [str(args.ftx), "export", target, str(pat_path), "--out", str(d), "--created", "2026-11-21T00:00", "--json"],
                check=True,
                capture_output=True,
            )
            chk = json.loads(
                subprocess.check_output(
                    [str(args.ftx), "check", str(d), "--pattern-ref", str(pat_path), "--json"],
                    text=True,
                )
            )
            if not chk.get("ok") or chk["result"]["passed"] != chk["result"]["total"]:
                raise RuntimeError(f"ftx check failed for {target}: {chk}")
            (src,) = d.glob("*" + suffix)
            (out / dest_name).write_bytes(src.read_bytes())

    svg_bytes = (out / "waistcoat-master.svg").read_bytes()
    svg_dims = parse_svg_dimensions(svg_bytes.decode("utf-8"))

    # Build indexed seam directory (exact 0-based index from waistcoat-pattern.json!)
    indexed_seams = []
    for idx, (s_def, s_val) in enumerate(zip(pat["seams"], res["seams"])):
        inst_a, edge_a = s_def["a"].split("/")
        inst_b, edge_b = s_def["b"].split("/")
        piece_a = inst_a.split(".")[0]
        piece_b = inst_b.split(".")[0]
        indexed_seams.append({
            "index": idx,
            "id": s_def["id"],
            "a": s_def["a"],
            "b": s_def["b"],
            "instanceA": inst_a,
            "edgeA": edge_a,
            "layerA": LAYER_OF_PIECE[piece_a],
            "instanceB": inst_b,
            "edgeB": edge_b,
            "layerB": LAYER_OF_PIECE[piece_b],
            "reverse": bool(s_def.get("reverse", False)),
            "lenAMm": s_val["len_a_mm"],
            "lenBMm": s_val["len_b_mm"],
            "residualMm": s_val["residual_mm"],
            "status": s_val["status"],
        })

    manifest = {
        "schema": "ftx-waistcoat-exhibit/1",
        "provenance": PROVENANCE,
        "patternSha256": hashlib.sha256(pat_path.read_bytes()).hexdigest(),
        "svgSha256": hashlib.sha256(svg_bytes).hexdigest(),
        "svgDimensions": svg_dims,
        "validation": {
            "ok": val["ok"],
            "status": res["status"],
            "seamsTotal": len(res["seams"]),
            "seamsPassing": sum(1 for s in res["seams"] if s["status"] == "PASS"),
            "truingTotal": len(res.get("truing", [])),
            "truingPassing": sum(1 for t in res.get("truing", []) if t["status"] == "PASS"),
            "points": res["points"],
        },
        "layers": LAYER_OF_PIECE,
        "seams": indexed_seams,
        "unpairedAndMissing": audit_unpaired_and_missing(pat),
    }
    (out / "waistcoat-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(
        f"Generated waistcoat exhibit in {out}: "
        f"status={res['status']}, seams={len(indexed_seams)}/{len(indexed_seams)} PASS, "
        f"truing={manifest['validation']['truingPassing']}/{manifest['validation']['truingTotal']} PASS, "
        f"unpaired/missing={len(manifest['unpairedAndMissing'])}, svg={svg_dims}"
    )


if __name__ == "__main__":
    main()
