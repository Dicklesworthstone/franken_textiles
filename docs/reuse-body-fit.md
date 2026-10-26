# Use an accepted image-fit shape for body export and draping

An accepted `ftx measure fit-body` report can now supply the actual body shape to
`ftx body`, `ftx drape`, and simulated workspace rounds. This is explicit: without
`--body-fit`, the existing tape-fitting path is unchanged. With it, no tape fit,
parameter rounding, fallback fit, or silent shape adjustment occurs.

The browser workshop also accepts this saved report in its body panel. Select
the same converted model, choose the accepted-fit report, and explicitly associate
it with the current wearer. The report selects the checked and draped body; it does
not change the flat draft or measurement-readiness verdict. Clearing the report
restores tape fitting. A rejected replacement never falls back to a previous
shape or to tape fitting. The arm-angle control selects the ordinary drape pose,
not an image-frame pose; a refused start remains a refusal at that angle.

After drafting a round, **Check body against targets** uses the loaded model without
running cloth. It works for flat-only garments such as the fez as well as foldable
patterns. With no accepted report selected it runs the existing tape fitter; with
an associated report it reuses that exact shape. Review signed target differences
and all available MODEL_FIT dimensions, then download `body_check.json`. Imported
checks also offer the original report unchanged. The check binds the exact model
and saved round-measurement hashes; changes withdraw its old display and files.
It neither changes measurement readiness nor establishes physical fit. Use the
separate fold-up action to obtain cloth results.

```sh
ftx measure fit-body observations.json --model anny.ftxbody --out accepted-fit.json
ftx body --body-fit accepted-fit.json --model anny.ftxbody \
  --pose rest --out fitted-body
ftx drape pattern.json --measurements measurements.json --fit fit_spec.json \
  --body-fit accepted-fit.json --model anny.ftxbody --out new-drape
ftx round workspace --sim --body-fit accepted-fit.json --model anny.ftxbody
```

`--stills` and the existing paired-sewing controls can also be used on rounds.
`--body-fit` without `--sim` or `--stills` is refused by `round`. These commands
consume the saved `ftx-body-fit/1` report, not the CLI's enclosing `ftx-cli/1` JSON.
`--model` may be omitted when the converted default is the exact model used by
the fit. Only the shared shape is reused: frame-specific translations, rotations
and joint poses from the images are **not** garment placement poses. `body --pose`
and `drape --arm-deg` select the usual native poses.

## Model and shape consistency

The exact model-file SHA-256 must match both identities in the report. The model
is hashed and parsed from the same bytes, with its array hashes verified. The
shared shape is reconstructed from named fixed values and bounded fitted values,
using the verified model's local-change label order. It must exactly reproduce
the report's expanded `MODEL_FIT` parameters. Rejected or incomplete reports,
failed starts or residual checks, missing multistart agreement, out-of-range
parameters, and contradictory identities are refused; there is no tape fallback.

This validates internal consistency, not authenticity: a hand-edited receipt is
not cryptographically certified, and source image residuals are not rerun. Keep
the original capture and fitting report. Explicitly associate the correct fit
with the wearer: equal age/sex or a matching model file does not identify a person.
When supplied measurements declare age or sex, a conflict with the fixed fitting
configuration is refused. `body` accepts optional `--measurements`; `drape` still
requires measurements for its normal pattern/workflow contract.

## Outputs and evidence boundaries

Imported body export requires a **new** output directory. It writes the checked
closed skin as `body.obj`, the exact original report bytes as `body_fit.json`, and
`body.json` last. Existing files, directories and link aliases are never replaced.
The imported body document remains `ftx-body/1`, with exact phenotype/local changes,
`shape_source`, full `source_fit`, and the requested pose. The source records its
report/request hashes, `refitted: false`, and `image_poses_reused: false`.

Draping uses these same reconstructed parameters through the existing posing,
skin validation, placement, contact and cloth solver; it embeds the description
and original fit diagnostics in `sim_report.json.body`. It also writes the exact
imported report bytes as `body_fit.json` beside that report. A simulated round
keeps both under its own `rounds/NNN/sim/` directory, so changing the external
fit file cannot revise its saved source. The construction review links this
original report and shows the imported body's signed differences against the
round's saved measurement targets. These targets keep their original evidence
basis, including chart priors; they are not necessarily tape measurements.
Residuals retain their stored numeric precision, and unavailable model predictions
stay blank instead of becoming zero. Review discrepancies before trusting the
drape; this table adds no pass/fail threshold or uncertainty estimate and changes
neither the shape nor measurement readiness.

The round retains the full simulation report in its simulation basis. A failed
import cannot reuse an old successful simulation or advertise its source/viewer
links. Usual new-directory publication and exit codes remain in force.

Body height/mass are recomputed from the model. Available tape functionals are
recomputed at the fitter's standard 35-degree A-pose, independent of the output
pose; missing anatomy/sections are reported as unavailable, not zero. Supplied
current tape targets are compared in millimetres in `measurement_comparison`,
including explicit residuals and `used_to_refit: false`. Differences do not alter
the imported shape and are not silently converted into a new fit approval.

These predictions remain **MODEL_FIT**. No sigma is invented, no new fused evidence
is produced, and no person's measurement-readiness gate becomes READY because an
image fit was accepted. `model_fit` is empty on imported body descriptions because
this handoff does not estimate uncertainty for those derived measurements. Flat
validation, measurement readiness, drape flags and export checks retain their
separate meanings. Neither shape reuse nor a successful drape proves garment fit.

The shared readiness gate also blocks required fused values that use MODEL_FIT
sources, even if a caller supplies a small sigma or mixes them with measured
records. Keep model estimates separate from independent measurement evidence.
Optional estimates stay advisory; fictional chart wearers remain READY_REFERENCE.

Input limits are 16 MiB for the fit report and 512 MiB for the converted model;
exceeding either returns a budget refusal (exit 4), never truncated input. Invalid
reports/models and output conflicts return exit 3. Import consistency, exact
synthetic mesh recovery, preserved residuals, no image-pose reuse, immutable output,
wrong-model and demographic refusals are covered by unit and real-CLI tests.
One regression runs the actual multi-frame fitter on independently projected
synthetic corners, then exports its saved accepted shape as the same closed body.
Another verifies that a rejected import is retained as a failed simulation in the
round verdict, without fallback fitting or a successful clip.
The opt-in production case in `crates/ftx-cli/tests/body_model_fit.rs` now runs the full admitted
Anny asset through synthetic multi-frame fitting and accepted-shape body export. It checks all
20 withheld tape predictions, the rest skin's topology and every exported vertex against the known
synthetic shape, exact report preservation, and refusal to export rejected or budget-stopped fits.
It fits two shared controls while the other controls stay at model defaults; it supplies no real
photographs, visibility inference or empirical accuracy evidence. The bounded garment check below
has its own scope; broader placement qualification and real-wearer accuracy remain open.

The same opt-in case also drafts a bolero for its declared synthetic body, then runs a
50-degree drape pose with 2 mm self-contact, 12 sewing and 72 gravity steps through a
workspace round. It checks every accepted Newton step, settling, all export checks,
exact fit-report retention, signed target comparisons and an immutable failed next
round after the external fit is changed to REJECTED. Model-derived target records
have explicitly synthetic test weights and an authored hand-passage assumption;
they remain NOT_READY. A numerical garment success must not override that gate.

This is a bounded 50-degree synthetic case, not general placement qualification.
The same-body 35-degree start was refused for a negative thickness gap of about
0.009 mm at the requested 2 mm separation. A different 920 mm chart garment also
failed when paired with this 1022 mm model, at both 35 and 50 degrees. These failed
starts remain recorded; no collision tolerance was reduced or solver guard bypassed.
Independent anatomy, camera, operator and real-wearer acceptance remain open.
