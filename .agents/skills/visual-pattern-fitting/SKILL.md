---
name: visual-pattern-fitting
description: Revise a sewing pattern from a garment brief or visual reference by inspecting FrankenTextiles drape stills, identifying the visible pattern panel, editing the flat cut, and comparing dressed rounds.
---

# Visual pattern fitting

Keep the wearer's brief in view. “Expose the wrist while keeping a full sleeve” is a decision you
can make from the pictures and the flat dimensions. A solver can tell you that the garment settled
even while the sleeve covers the fingers. Each verdict in `round.json` answers a different question.

From the repository root, build `ftx` with `cargo build --release -p ftx-cli --bin ftx` and put
`target/release` on your PATH. The simulator needs the locally converted Anny model described in
`tools/body-model/PIN.md`. Use a private workspace for a real wearer.

## See the garment

```sh
ftx round "$work" --pattern pattern.json --measurements measurements.json --fit fit_spec.json \
  --stills --res 840x620 --self-contact-mm 2 --note "Inspect the cuff relative to the hand" --json
```

Use the returned `result.dir` relative to the workspace. A round can exit 2 and still have
inspectable pictures. Read its `basis`, `sim.gating`, `sim.advisory` and `rendering.status`.
For a refused solve with saved diagnostics, open `sim/refused/still_front.png` and read
`sim.refusal`. The image shows the state associated with the failure. Use the same `inspect-pixel`
command below on that image to trace an edge. The result retains the refusal, phase and accepted
step count when recorded. Older archives can still be compared visually. With a convergence
refusal, try a smaller `--time-step-s` from the start and increase `--sew` and `--drape`
proportionally to preserve the physical durations. For a tighter dynamic solve, lower
`--newton-tol-m-s` and compare the reported force residuals as well as the images.

A solve refused at its start (`intersecting_start`) has no cloth state. It leaves the views of
`ftx place` instead. Open `sim/refused/start/still_back.png` and `still_front.png`: cloth inside the
body is magenta and cloth crossing cloth is yellow. The first repair in `sim.refusal` names the
piece pairs that cross, and `sim/refused/placement.json` gives each piece's angles, heights and
radius. Layers that only touch or cloth inside the clearance distance can leave both colours absent.
Read the refusal message. If the posed body failed admission, change the pose first.
Use `inspect-pixel` on the start PNG to identify its panel and nearest authored edge. The result
records the placement phase and zero accepted steps. Older starts without a map support body and
background picks. While you adjust placements, `ftx place pattern.json --measurements measurements.json
--out NEW_DIR` writes the same views in seconds without the solver. A joint outfit leaves them
too, with each piece named `garment::piece`.

For a completed solve, open the current round's `sim/still_front.png`
as an image, then a second angle or strain view as the question requires. `preview.svg` shows
the flat pieces and construction diagnostics. `report.html` in the workspace ties the evidence together.

Pick a pixel **on the original PNG** where the garment shape matters. If your image viewer resizes
it, convert the display coordinates back to the PNG dimensions. For example:

```sh
ftx inspect-pixel "$work/rounds/000" --view front --pixel 597,415 --json
```

For a garment hit, `hit.instance` distinguishes the physical left and right panels.
`hit.piece`, `hit.nearest_edge.source_edge` and both named endpoints refer to the editable
`pattern.json`. Read `posed_distance_mm`: a point in the middle of a wide panel can be far from
any boundary. A nearby edge in a fold can be close in space without causing the fold. A body hit
is useful evidence when checking where the cloth ends.

## Change the cut and inspect it again

State what you see and what you expect one edit to change. For measured drafts, changing the style
parameter and redrafting also rebuilds related curves and markings. For an authored piece,
`ftx alter move`, `slash-spread`, `true-seam` and `cap-ease` operate on named geometry. Write a
fresh candidate. Review connected seams, notches and trim affected by the edit.

A gathered seam (`gather_ratio` well away from 1) sews, but the solver's cloth resists shortening
as strongly as stretching, so the cloth below it can stand away from the body like a shelf. If the
sewn garment would pleat there, declare `gather_relief_mm` and `gather_compression` on that seam
(`docs/spec/04-simulation-vectorize-cli.md` §5.4). The drape report lists the band under
`gather_relief`. Both numbers are authored, with no measured source.

```sh
ftx round "$work" --pattern revised/pattern.json --stills --res 840x620 --self-contact-mm 2 \
  --note "Shortened sleeves; compare cuff and hand in front and side views" --json
```

Keep body, pose, fabric, solver options and workspace fit contract fixed when judging a cut
change. Use a shared camera so an edited hem or sleeve cannot shift the wearer in the frame.
To see two native views alongside the same flat panel at a common scale, run:

```sh
python3 .agents/skills/visual-pattern-fitting/scripts/compare_rounds.py \
  "$work/rounds/000" "$work/rounds/001" --instance sleeve.L --shared-camera \
  --out "$work/sleeve-comparison.svg"
```

Open the self-contained SVG. The two renders use the baseline camera and raster size. A crop
is in baseline PNG pixels and applies to both renders. The comparison labels whether the posed
body geometry matches and keeps the new PNGs beside the SVG in a directory ending in `.views`.
The view uses the baseline field of view, so look at the original views as well if a change
extends beyond the frame. Leave off `--shared-camera` to read an archive that only has original
stills. Use the original PNGs for `inspect-pixel` on each round.

Shared-camera comparisons check the recorded frames of completed drapes against the displayed
body and cloth. The displayed garment must match the last recorded frame at PC2 precision.
Treat a crossing as a failed contact check even if the archived solver report shows no gating
flags. Missing, mismatched or unsupported recordings show the check as unavailable.

Each side is labelled with the evidence it shows. “Refused start”
shows the pre-sewn placement before dynamics. “Refused pose” shows the state recorded by a failed
solve. Both retain the original refusal code.
For a vision tool that accepts raster images, use a new `.png`
output path with Node and the showcase dependencies installed. The outlines are aligned at their top centres.
Inspect the new images even when the flat verdict improves. Retain a revision if it advances
the brief and its construction checks support it. A competing silhouette can start from any
saved `rounds/NNN/pattern.json`. Use `--export` on the chosen candidate for full-size files.
Questions of comfort or real fit go back to the sewist with the relevant views and a specific
wearing check.

## Reproduce the sleeve example

From the repository root, these inputs come from the public-prior draft fixture. Set `work` to a
new local directory, then run:

```sh
mkdir -p "$work"
jq '.cases[] | select(.id=="zouave_bolero-920") | .input.measurements' \
  tests/fixtures/oracle/draft/programs.json > "$work/measurements.json"
printf '%s\n' '{"sleeve_extra":65}' > "$work/long.json"
ftx draft zouave_bolero --measurements "$work/measurements.json" --style "$work/long.json" --out "$work/long"
ftx round "$work" --pattern "$work/long/pattern.json" --fit "$work/long/fit_spec.json" \
  --measurements "$work/measurements.json" --as-of 2026-10-13 --stills --res 840x620 --self-contact-mm 2
ftx inspect-pixel "$work/rounds/000" --view front --pixel 597,415 --json
printf '%s\n' '{"sleeve_extra":15}' > "$work/shorter.json"
ftx draft zouave_bolero --measurements "$work/measurements.json" --style "$work/shorter.json" --out "$work/shorter"
ftx round "$work" --pattern "$work/shorter/pattern.json" --as-of 2026-10-13 \
  --stills --res 840x620 --self-contact-mm 2 --hands knuckle --note "Check cuff clearance after shortening the sleeve by 50 mm"
ftx viewer "$work/rounds/001/sim" --check-contact --view front --json
```

The original archive failed the flat sleeve-length check at 385 mm. Full-body replay checks find
crossings in both archived wrist-collider runs at frame 87. A fresh 335 mm run with
`--hands knuckle` passed the flat check and all 49 recorded physical frames. It has its own
posed body file. Use its still to assess the simulated cuff. The 385 mm cut remains unresolved.
