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
step count. Older refusal archives lack the source-edge map and can still be compared visually.
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

```sh
ftx round "$work" --pattern revised/pattern.json --stills --res 840x620 --self-contact-mm 2 \
  --note "Shortened sleeves; compare cuff and hand in front and side views" --json
```

Keep body, pose, fabric, solver options and workspace fit contract fixed when judging a cut
change. The renderer centres each round on its garment, so use body landmarks to compare lengths.
To see two native views alongside one flat panel at a common scale, run:

```sh
python3 .agents/skills/visual-pattern-fitting/scripts/compare_rounds.py \
  "$work/rounds/000" "$work/rounds/001" --instance sleeve.L --out "$work/sleeve-comparison.svg"
```

Open the self-contained SVG. Either side may show a refused pose, marked with its failure code.
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
  --stills --res 840x620 --self-contact-mm 2 --note "Check cuff clearance after shortening the sleeve by 50 mm"
```

The baseline exits 2 for excess sleeve length. In the revised front view the hands are exposed.
The finished sleeve measures 335 mm against the 320 mm reference arm. The measurements are chart
estimates, and the cloth response is simulated.
