"""Build the fixed synthetic sundress and its native FTX pattern files in a new directory."""

import argparse
import json
import subprocess
import tempfile
from pathlib import Path


def panel(name, label, angle, nodes, lines, grain, note):
    vertices = {key: [x, y] for key, x, y in nodes}
    edges = []
    for i, (tag, curve) in enumerate(lines):
        edge = {"id": tag, "from": nodes[i][0], "to": nodes[(i + 1) % len(nodes)][0]}
        if curve:
            edge["curve"] = {"type": "cubic", "c": curve}
        edges.append(edge)
    return {"id": name, "name": label, "cut": {"qty": 1}, "vertices": vertices, "edges": edges,
            "grain": grain, "note": note, "placement": {"part": "torso", "y0_landmark": "waist",
            "x0": 0, "y0": 0, "theta_deg": angle}}


def dress(gather_ratio=1.3):
    waist_edge = 390 * gather_ratio
    waist_note = (f"Ease {waist_edge:g} mm evenly onto the 390 mm bodice waist."
                  if gather_ratio == 1.05 else f"Gather {waist_edge:g} mm evenly to the 390 mm bodice waist.")
    pieces = []
    for side, angle, neck in [("front", 0, 300), ("back", 180, 321)]:
        nodes = [("wl",-195,0),("wr",195,0),("br",235,180),("ur",235,235),("ro",128,384),
                 ("rt",128,410),("ri",103,410),("rn",103,375),("nc",0,neck),("ln",-103,375),
                 ("li",-103,410),("lt",-128,410),("lo",-128,384),("ul",-235,235),("bl",-235,180)]
        # Cubic control points are fractions along and perpendicular to each chord.
        lines = [("waist",None),("side_lower_R",[(.3,-.035),(.72,-.07)]),("side_upper_R",None),
                 ("arm_R",[(.19,.19*149/107),(.68,.13)]),("strap_outer_R",None),("shoulder_R",None),
                 ("strap_inner_R",None),("neck_R",[(.32,.16),(.70,.15)]),("neck_L",[(.30,.15),(.68,.16)]),
                 ("strap_inner_L",None),("shoulder_L",None),("strap_outer_L",None),
                 ("arm_L",[(.32,.13),(.81,.19*149/107)]),("side_upper_L",None),
                 ("side_lower_L",[(.28,-.07),(.70,-.035)])]
        pieces.append(panel("bodice_"+side, side.title()+" bodice", angle, nodes, lines,
                     [[0,40],[0,240]],"Bind armholes and neckline; finish shoulder seams. Allow for a side opening."))
        nodes = [("hl",-420,-600),("hr",420,-600),("lr",420,-570),
                 ("wr",waist_edge / 2,0),("wl",-waist_edge / 2,0),("ll",-420,-570)]
        lines = [(tag,None) for tag in ["hem","side_hem_R","side_R","gather","side_L","side_hem_L"]]
        pieces.append(panel("skirt_"+side, side.title()+" skirt", angle, nodes, lines,
                     [[0,-470],[0,-70]],waist_note))
    seams = []
    def join(name, a, b, **extra):
        seams.append({"id": name, "a": a, "b": b, "reverse": True, **extra})
    for side, other in [("R","L"),("L","R")]:
        front = "side_lower_R+side_upper_R" if side=="R" else "side_upper_L+side_lower_L"
        back = "side_upper_L+side_lower_L" if other=="L" else "side_lower_R+side_upper_R"
        join("bodice_side_"+side,"bodice_front/"+front,"bodice_back/"+back)
        join("shoulder_"+side,"bodice_front/shoulder_"+side,"bodice_back/shoulder_"+other)
        front = "side_hem_R+side_R" if side=="R" else "side_L+side_hem_L"
        back = "side_L+side_hem_L" if other=="L" else "side_hem_R+side_R"
        join("skirt_side_"+side,"skirt_front/"+front,"skirt_back/"+back)
    for side in ["front","back"]:
        join("gather_"+side,"skirt_"+side+"/gather","bodice_"+side+"/waist",kind="gather",ratio=gather_ratio)
    return {"schema":"fabric-pattern/1","units":"mm","style":"GARDEN_SUNDRESS",
            "meta":{"title":"Garden sundress","size_label":"Synthetic adult study","sa_mm":10,
                    "origin":"VECTOR_EDIT","note":"Design study. Verify a muslin and choose a closure and finishes."},
            "pieces":pieces,"seams":seams}


def body(stature=1680, chest=900, waist=720, hip=960):
    # Synthetic illustration inputs only. The native flat landmark provider consumes these controls.
    values = {"stature":stature,"chest":chest,"waist":waist,"hip":hip,"waist_height":1010,
              "back_neck_to_waist":425,"shoulder_slope_deg":15,"neck_base":350,"shoulder_length":125,
              "armscye_depth":215,"crotch_height":770,"head":550,"knee_height":470,"arm_length":565,
              "hip_width":345}
    return {"schema":"fabric-measurements/1","wearer":{"label":"Synthetic adult form","stature_mm":stature,
            "sex":"female","age_months":360},"fused":{k:{"value_deg" if k.endswith("_deg") else "value_mm":v}
            for k,v in values.items()}}


def fit_spec():
    return {"schema":"fabric-fit/1","garment":"Garden sundress","points":[
        {"id":"bust","label":"Finished bodice bust","kind":"girth","instances":["bodice_front","bodice_back"],
         "landmark":"waist","dy":200,"body":"chest","target_mm":[20,40,65]},
        {"id":"waist","label":"Finished waist","kind":"path","refs":["bodice_front/waist","bodice_back/waist"],
         "body":"waist","target_mm":[25,60,90]}]}


def write_json(path, data):
    with path.open("x") as handle:
        json.dump(data,handle,indent=2,ensure_ascii=False)
        handle.write("\n")


def record_drape(ftx, pattern, measure, fit, scratch, model):
    placement = {"bodice": 177, "skirt": 255}
    arranged = json.loads(pattern.read_text())
    for piece in arranged["pieces"]:
        kind = piece["id"].split("_")[0]
        piece["placement"]["radius_mm"] = placement[kind]
    arranged_path = scratch / "wrapped.json"
    write_json(arranged_path, arranged)
    model_args = ["--model", str(model)] if model else []
    native = [str(ftx)]
    subprocess.run(native + ["body", "--measurements", str(measure), "--pose", "drape",
                             "--out", str(scratch / "body"), "--json", *model_args],
                   capture_output=True, text=True, check=True)
    fitted = json.loads((scratch / "body/body.json").read_text())
    settings = ["--spacing-mm", "45", "--self-contact-mm", "1", "--time-subdivision",
                "--sew", "12", "--drape", "36"]
    attempt = subprocess.run(native + ["drape", str(arranged_path), "--measurements", str(measure),
                             "--fit", str(fit), *settings, "--out", str(scratch / "simulation"),
                             "--json", *model_args], capture_output=True, text=True)
    envelope = json.loads(attempt.stdout)
    if envelope["ok"]:
        outcome = json.loads((scratch / "simulation/sim_report.json").read_text())
    elif envelope["refusal"]["code"] == "not_converged":
        outcome = envelope["refusal"]
    else:
        raise RuntimeError(attempt.stdout)
    return {"form": "Reference", "model": fitted["model"], "pose": fitted["pose"],
            "tape_evaluation": fitted["tape_evaluation"], "body_fit": fitted["fit"]["tape"],
            "starting_wrap_radius_mm": placement, "native_options": settings,
            "fabric": "FTX default solver parameters",
            "stages": [line for line in attempt.stderr.splitlines()
                       if line.startswith(("start:", "self-contact:", "sew:", "drape:"))],
            "exit": attempt.returncode, "solver": outcome}


def comparisons(ftx, pattern, fit, directory):
    result = []
    for label, chest, waist in [("Reference", 900, 720), ("Close", 935, 770), ("Beyond garment", 965, 805)]:
        measure = directory / (label + ".json")
        write_json(measure, body(chest=chest, waist=waist))
        check = subprocess.run([str(ftx), "validate", str(pattern), "--measurements", str(measure),
                                "--fit", str(fit), "--json"], capture_output=True, text=True)
        report = json.loads(check.stdout).get("result", {})
        if check.returncode not in (0,2) or not report.get("points"):
            raise ValueError(f"Unexpected native validation outcome for {label}: {check.stdout}")
        result.append({"form": label, "status": report["status"], "points": report["points"]})
    return {"basis": "Native flat validation of one fixed pattern against three synthetic adult forms.",
            "fit": fit_spec(), "runs": result}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--gather-ratio", type=float, choices=(1.3, 1.05), default=1.3)
    parser.add_argument("--ftx", type=Path, default=Path(__file__).resolve().parents[2] / "target/release/ftx")
    parser.add_argument("--record-drape", action="store_true",
                        help="Also run the contact solver and record its outcome (requires the local Anny model).")
    parser.add_argument("--model", type=Path, help="Local converted Anny model for --record-drape.")
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    pattern = out / "pattern.json"
    write_json(pattern, dress(args.gather_ratio))

    def native(*argv):
        process = subprocess.run([str(args.ftx), *map(str, argv), "--json"], capture_output=True, text=True)
        if process.returncode:
            raise RuntimeError(process.stdout or process.stderr)
        return json.loads(process.stdout)

    with tempfile.TemporaryDirectory() as scratch:
        tmp = Path(scratch)
        measure = tmp / "synthetic.json"
        fit = tmp / "fit.json"
        write_json(measure, body())
        write_json(fit, fit_spec())
        checked = native("validate", pattern, "--measurements", measure, "--fit", fit)
        write_json(out / "flat-checks.json", checked)
        write_json(out / "fit-comparison.json", comparisons(args.ftx, pattern, fit, tmp))
        exports = [("svg", "_master_mm.svg", "master.svg"), ("a4", "_A4.pdf", "pattern-A4.pdf")]
        for target, suffix, filename in exports:
            destination = tmp / target
            native("export", target, pattern, "--out", destination, "--created", "2026-11-19T00:00")
            native("check", destination, "--pattern-ref", pattern)
            generated, = destination.glob("*" + suffix)
            with (out / filename).open("xb") as stream:
                stream.write(generated.read_bytes())
        if args.record_drape:
            write_json(out / "drape-result.json", record_drape(args.ftx, pattern, measure, fit, tmp, args.model))


if __name__ == "__main__":
    main()
