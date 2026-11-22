"""Build the checked wrap-skirt pattern (fabric-pattern/1) and slice native Anny rest vs seated
synthetic adult forms for the adjustable-overlap wrap skirt exhibit (ftx-kod).

Run from repository root:
  python3 showcase/scripts/prepare-wrap-skirt.py showcase/public/exhibits/wrap-skirt \
    --model /home/ubuntu/.cache/franken_textiles/anny-0.6.1-default.ftxbody
"""
import argparse
import json
import math
import subprocess
import tempfile
from pathlib import Path

import numpy as np


def wrap_skirt_pattern():
    # Exact flat dimensions (mm):
    # - Back waist: 360 mm, each overlapping front waist: 330 mm -> total flat waist sweep = 1020 mm
    # - Skirt length: 600 mm (waist y=0 down to hem y=-600)
    # - Side seam hip flare: 110 mm per side -> back hem = 580 mm, each front hem = 480 mm (total 1540 mm)
    # - Waistband: 1020 mm x 70 mm (folded to 35 mm finished height), tie attachment on lower 35 mm end
    # - Ties (cut 2): 780 mm x 35 mm each (1560 mm combined tie length)
    hip_curve_up = {"type": "cubic", "c": [[0.32, -0.018], [0.70, -0.032]]}
    hip_curve_dn = {"type": "cubic", "c": [[0.30, -0.032], [0.68, -0.018]]}

    pieces = [
        {
            "id": "skirt_back",
            "name": "Back skirt",
            "cut": {"qty": 1},
            "vertices": {
                "hl": [-290, -600],
                "hr": [290, -600],
                "wr": [180, 0],
                "wl": [-180, 0],
            },
            "edges": [
                {"id": "hem", "from": "hl", "to": "hr"},
                {"id": "side_R", "from": "hr", "to": "wr", "curve": hip_curve_up},
                {"id": "waist", "from": "wr", "to": "wl"},
                {"id": "side_L", "from": "wl", "to": "hl", "curve": hip_curve_dn},
            ],
            "grain": [[0, -520], [0, -80]],
            "notches": [{"id": "cb_waist", "edge": "waist", "s": 0.5, "type": "double"}],
            "note": "Join side seams to both overlapping fronts. Match centre-back notch to waistband.",
        },
        {
            "id": "skirt_front",
            "name": "Wrap front panel",
            "cut": {"qty": 2},
            "vertices": {
                "h_side": [-110, -600],
                "h_wrap": [370, -600],
                "w_wrap": [330, 0],
                "w_side": [0, 0],
            },
            "edges": [
                {"id": "hem", "from": "h_side", "to": "h_wrap"},
                {"id": "wrap_edge", "from": "h_wrap", "to": "w_wrap"},
                {"id": "waist", "from": "w_wrap", "to": "w_side"},
                {"id": "side", "from": "w_side", "to": "h_side", "curve": hip_curve_dn},
            ],
            "grain": [[140, -520], [140, -80]],
            "internal": [
                {"kind": "fold", "label": "Wrap edge hem fold (20 mm)", "pts": [[350, -600], [310, 0]]},
                {"kind": "mark", "label": "Centre-front reference (+180 mm from side)", "pts": [[180, -40], [180, 0]]},
            ],
            "notches": [{"id": "cf_mark", "edge": "waist", "s": round(150 / 330, 4), "type": "V"}],
            "note": "Cut mirror pair (R overwrap, L underwrap). Hem front wrap edges before attaching waistband.",
        },
        {
            "id": "waistband",
            "name": "Wrap waistband (fold)",
            "cut": {"qty": 1},
            "vertices": {
                "b0": [0, 0],
                "b1": [330, 0],
                "b2": [690, 0],
                "b3": [1020, 0],
                "r_mid": [1020, 35],
                "r_top": [1020, 70],
                "l_top": [0, 70],
                "l_mid": [0, 35],
            },
            "edges": [
                {"id": "front_L", "from": "b0", "to": "b1"},
                {"id": "back", "from": "b1", "to": "b2"},
                {"id": "front_R", "from": "b2", "to": "b3"},
                {"id": "end_R_tie", "from": "b3", "to": "r_mid"},
                {"id": "end_R_upper", "from": "r_mid", "to": "r_top"},
                {"id": "top", "from": "r_top", "to": "l_top"},
                {"id": "end_L_upper", "from": "l_top", "to": "l_mid"},
                {"id": "end_L_tie", "from": "l_mid", "to": "b0"},
            ],
            "grain": [[60, 35], [960, 35]],
            "internal": [
                {"kind": "fold", "label": "Waistband lengthwise fold", "pts": [[0, 35], [1020, 35]]},
                {"kind": "mark", "label": "Right side-seam tie pass-through slot", "pts": [[690, 8], [690, 27]]},
            ],
            "notches": [
                {"id": "cf_band_L", "edge": "front_L", "s": round(150 / 330, 4), "type": "V"},
                {"id": "cb_band", "edge": "back", "s": 0.5, "type": "double"},
                {"id": "cf_band_R", "edge": "front_R", "s": round(180 / 330, 4), "type": "V"},
            ],
            "note": "Attach to joined skirt waist (1020 mm total). Work a 20 mm buttonhole at the right side-seam slot for the inner tie.",
        },
        {
            "id": "waist_tie",
            "name": "Waist tie strap",
            "cut": {"qty": 2},
            "vertices": {
                "a0": [0, 0],
                "a1": [780, 0],
                "a2": [780, 35],
                "a3": [0, 35],
            },
            "edges": [
                {"id": "bottom", "from": "a0", "to": "a1"},
                {"id": "tip", "from": "a1", "to": "a2"},
                {"id": "top", "from": "a2", "to": "a3"},
                {"id": "attach", "from": "a3", "to": "a0"},
            ],
            "grain": [[40, 17.5], [740, 17.5]],
            "note": "Cut two 780 mm ties. Press into finished narrow straps and insert into the folded waistband ends.",
        },
    ]

    seams = [
        {"id": "side_R", "a": "skirt_back/side_R", "b": "skirt_front.R/side", "reverse": True},
        {"id": "side_L", "a": "skirt_back/side_L", "b": "skirt_front.L/side", "reverse": True},
        {"id": "band_back", "a": "waistband/back", "b": "skirt_back/waist", "reverse": True},
        {"id": "band_front_R", "a": "waistband/front_R", "b": "skirt_front.R/waist", "reverse": True},
        {"id": "band_front_L", "a": "waistband/front_L", "b": "skirt_front.L/waist", "reverse": True},
        {"id": "tie_R", "a": "waist_tie.R/attach", "b": "waistband/end_R_tie", "reverse": True},
        {"id": "tie_L", "a": "waist_tie.L/attach", "b": "waistband/end_L_tie", "reverse": True},
    ]

    return {
        "schema": "fabric-pattern/1",
        "units": "mm",
        "style": "WRAP_SKIRT",
        "meta": {
            "title": "Adjustable-overlap wrap skirt",
            "size_label": "Fixed 1020 mm wrap · synthetic adult study",
            "sa_mm": 10,
            "origin": "VECTOR_EDIT",
            "note": "Fixed wrap skirt with 1020 mm total waist sweep, 1540 mm hem sweep, and two 780 mm waist ties. Check seated overlap and confirm fit in a muslin.",
        },
        "pieces": pieces,
        "seams": seams,
    }


def fit_spec():
    return {
        "schema": "fabric-fit/1",
        "garment": "Adjustable-overlap wrap skirt",
        "points": [
            {
                "id": "waist_total",
                "label": "Total flat waist sweep (with wrap overlap)",
                "kind": "path",
                "refs": ["skirt_front.L/waist", "skirt_back/waist", "skirt_front.R/waist"],
                "body": "waist",
                "target_mm": [150, 300, 420],
            },
        ],
    }


def make_measures(label, stature_mm, chest_mm, waist_mm, hip_mm):
    values = {
        "stature": stature_mm,
        "chest": chest_mm,
        "waist": waist_mm,
        "hip": hip_mm,
        "waist_height": round(stature_mm * 0.601),
        "back_neck_to_waist": round(stature_mm * 0.253),
        "shoulder_slope_deg": 15,
        "neck_base": 350,
        "shoulder_length": 125,
        "armscye_depth": 215,
        "crotch_height": round(stature_mm * 0.458),
        "head": 550,
        "knee_height": round(stature_mm * 0.280),
        "arm_length": round(stature_mm * 0.336),
        "hip_width": round(hip_mm * 0.36),
    }
    return {
        "schema": "fabric-measurements/1",
        "wearer": {"label": label, "stature_mm": stature_mm, "sex": "female", "age_months": 360},
        "fused": {
            k: {"value_deg" if k.endswith("_deg") else "value_mm": v}
            for k, v in values.items()
        },
    }


def convex_hull_2d(pts):
    pts = sorted(set((round(float(x), 2), round(float(y), 2)) for x, y in pts))
    if len(pts) <= 2:
        return pts
    def cross(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lower = []
    for p in pts:
        while len(lower) >= 2 and cross(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    upper = []
    for p in reversed(pts):
        while len(upper) >= 2 and cross(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    return lower[:-1] + upper[:-1]


def slice_mesh_plane(verts_mm, faces, z_target):
    """Exact triangle-plane intersection segments at z = z_target, returning convex hull & perimeter."""
    pts = []
    for a, b, c in faces:
        vs = [verts_mm[a], verts_mm[b], verts_mm[c]]
        ds = [v[2] - z_target for v in vs]
        if (ds[0] > 0 and ds[1] > 0 and ds[2] > 0) or (ds[0] < 0 and ds[1] < 0 and ds[2] < 0):
            continue
        for i in range(3):
            j = (i + 1) % 3
            d0, d1 = ds[i], ds[j]
            if (d0 <= 0 <= d1) or (d1 <= 0 <= d0):
                if abs(d1 - d0) < 1e-9:
                    pts.append((vs[i][0], vs[i][1]))
                else:
                    t = -d0 / (d1 - d0)
                    x = vs[i][0] + t * (vs[j][0] - vs[i][0])
                    y = vs[i][1] + t * (vs[j][1] - vs[i][1])
                    pts.append((x, y))
    if not pts:
        return {"perimeter_mm": 0.0, "width_mm": 0.0, "depth_mm": 0.0, "hull": []}
    # Exclude detached arm/hand vertices far outside torso/hip band when arms hang near waist/hip
    # (in rest/seated poses arms hang at |x| > 240 mm near hip). Keep torso+pelvis+thigh envelope.
    arr = np.array(pts)
    hull = convex_hull_2d(arr)
    perim = sum(math.hypot(hull[(i + 1) % len(hull)][0] - hull[i][0], hull[(i + 1) % len(hull)][1] - hull[i][1]) for i in range(len(hull)))
    xs = [p[0] for p in hull]
    ys = [p[1] for p in hull]
    return {
        "perimeter_mm": round(perim, 1),
        "width_mm": round(max(xs) - min(xs), 1),
        "depth_mm": round(max(ys) - min(ys), 1),
        "hull": [[round(p[0], 1), round(p[1], 1)] for p in hull],
    }


def read_obj_mm(path):
    verts, faces = [], []
    for line in Path(path).read_text().splitlines():
        if line.startswith("v "):
            verts.append([float(x) * 1000.0 for x in line.split()[1:4]])
        elif line.startswith("f "):
            faces.append([int(tok.split("/")[0]) - 1 for tok in line.split()[1:4]])
    return np.array(verts), faces


def sample_pose_sections(ftx, model, meas_path, pose, out_dir):
    subprocess.run(
        [str(ftx), "body", "--measurements", str(meas_path), "--pose", pose, "--model", str(model), "--out", str(out_dir), "--json"],
        check=True,
        capture_output=True,
        text=True,
    )
    body_json = json.loads((out_dir / "body.json").read_text())
    verts, faces = read_obj_mm(out_dir / "body.obj")
    floor_z = float(verts[:, 2].min())
    waist_h = float(body_json["fit"]["tape"]["waist_height"]["body_mm"])
    # In Anny seated pose, hips remain near pelvis height while legs fold forward from hip toward knees.
    # Anchor plane z relative to the top of the head (unchanged torso height between rest and seated!).
    head_z = float(verts[:, 2].max())
    stature = float(body_json["fit"]["tape"]["stature"]["body_mm"])
    waist_z = head_z - (stature - waist_h)
    hip_z = waist_z - 180.0
    lap_z = waist_z - 310.0

    # Exclude arms/hands by keeping torso, pelvis, and leg faces (|x| <= 235 mm in both rest and seated poses).
    kept_faces = []
    for f in faces:
        xs = verts[f, 0]
        if np.all(np.abs(xs) <= 235.0):
            kept_faces.append(f)

    return {
        "pose": pose,
        "waist_z_mm": round(waist_z - floor_z, 1),
        "waist_tape_mm": round(float(body_json["fit"]["tape"]["waist"]["body_mm"]), 1),
        "hip_tape_mm": round(float(body_json["fit"]["tape"]["hip"]["body_mm"]), 1),
        "sections": {
            "waist": slice_mesh_plane(verts, kept_faces, waist_z),
            "hip": slice_mesh_plane(verts, kept_faces, hip_z),
            "lap": slice_mesh_plane(verts, kept_faces, lap_z),
        },
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--ftx", type=Path, default=Path(__file__).resolve().parents[2] / "target/release/ftx")
    parser.add_argument("--model", type=Path, default=Path("/home/ubuntu/.cache/franken_textiles/anny-0.6.1-default.ftxbody"))
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)

    pat = wrap_skirt_pattern()
    fit = fit_spec()
    pat_path = out / "wrap-skirt-pattern.json"
    fit_path = out / "wrap-skirt-fit.json"
    pat_path.write_text(json.dumps(pat, indent=2) + "\n")
    fit_path.write_text(json.dumps(fit, indent=2) + "\n")

    forms_spec = [
        ("petite", "Slender adult (66 / 90 cm)", 1640, 840, 660, 900),
        ("reference", "Reference adult (72 / 96 cm)", 1680, 900, 720, 960),
        ("full", "Full adult (82 / 106 cm)", 1700, 1000, 820, 1060),
    ]

    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        forms_out = []
        for fid, label, st, ch, wa, hi in forms_spec:
            m_path = tmp / f"{fid}.json"
            m_path.write_text(json.dumps(make_measures(label, st, ch, wa, hi), indent=2) + "\n")
            proc = subprocess.run(
                [str(args.ftx), "validate", str(pat_path), "--measurements", str(m_path), "--fit", str(fit_path), "--json"],
                capture_output=True,
                text=True,
            )
            if proc.returncode not in (0, 2):
                raise RuntimeError(f"ftx validate failed ({proc.returncode}): {proc.stdout or proc.stderr}")
            val = json.loads(proc.stdout)
            seams = val["result"]["seams"]
            bad = [s for s in seams if s["status"] != "PASS"]
            if bad:
                raise RuntimeError(f"Seam check failed for {fid}: {bad}")
            rest_sec = sample_pose_sections(args.ftx, args.model, m_path, "rest", tmp / f"{fid}_rest")
            seated_sec = sample_pose_sections(args.ftx, args.model, m_path, "seated", tmp / f"{fid}_seated")
            forms_out.append({
                "id": fid,
                "label": label,
                "stature_mm": st,
                "chest_mm": ch,
                "waist_mm": wa,
                "hip_mm": hi,
                "flat_validation": {
                    "status": val["result"]["status"],
                    "seams": seams,
                    "points": val["result"]["points"],
                },
                "rest": rest_sec,
                "seated": seated_sec,
            })

        # Export verified A4 PDF & master SVG directly from native `ftx export`.
        for target, suffix, dest_name in [("a4", "_A4.pdf", "wrap-skirt-A4.pdf"), ("svg", "_master_mm.svg", "wrap-skirt-master.svg")]:
            d = tmp / target
            subprocess.run([str(args.ftx), "export", target, str(pat_path), "--out", str(d), "--created", "2026-11-21T00:00", "--json"], check=True, capture_output=True)
            chk = json.loads(subprocess.check_output([str(args.ftx), "check", str(d), "--pattern-ref", str(pat_path), "--json"], text=True))
            assert chk["ok"] and chk["result"]["passed"] == chk["result"]["total"], chk
            src, = d.glob("*" + suffix)
            (out / dest_name).write_bytes(src.read_bytes())

    summary = {
        "schema": "ftx-wrap-skirt-exhibit/1",
        "scope": "Native flat pattern validation (fabric-pattern/1) paired with horizontal slices of the fitted Anny 0.6.1 neutral (rest) and seated meshes. No 3D dynamic seated cloth drape is simulated; physical coverage and tie comfort must be checked in a sewn muslin.",
        "flat_geometry": {
            "back_waist_mm": 360.0,
            "front_each_waist_mm": 330.0,
            "total_waist_sweep_mm": 1020.0,
            "hip_sweep_180mm_below_waist_mm": 1176.0,
            "lap_sweep_310mm_below_waist_mm": 1289.0,
            "total_hem_sweep_mm": 1540.0,
            "skirt_length_mm": 600.0,
            "tie_each_length_mm": 780.0,
            "slot_station_from_left_end_mm": 690.0,
        },
        "forms": forms_out,
    }
    (out / "forms.json").write_text(json.dumps(summary, indent=2) + "\n")
    print("Generated wrap-skirt exhibit in", out)
    for f in forms_out:
        print(
            f["id"],
            "seams:", len(f["flat_validation"]["seams"]),
            "rest hip/lap hull:", f["rest"]["sections"]["hip"]["perimeter_mm"], f["rest"]["sections"]["lap"]["perimeter_mm"],
            "seated hip/lap hull:", f["seated"]["sections"]["hip"]["perimeter_mm"], f["seated"]["sections"]["lap"]["perimeter_mm"],
        )


if __name__ == "__main__":
    main()
