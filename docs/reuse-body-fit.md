# Use a selected model body for drafting, flat checks and draping

An accepted `ftx measure fit-body` report can supply the actual body shape to
`ftx draft`, `ftx validate`, `ftx body`, `ftx drape`, and workspace rounds. Selection
is explicit. `draft --model` fits the supplied tape evidence on that model; adding
`--body-fit` reuses the accepted shape instead. With an accepted report, no tape fit,
parameter rounding, fallback fit, or silent shape adjustment occurs.

The browser workshop also accepts this saved report in its body panel. Select
the same converted model, choose the accepted-fit report, and explicitly associate
it with the current wearer. By default the report selects the checked and draped
body. Select **Use this body for drafting and flat checks** to use its dimensions
and placement frames for a new pattern, or to check an imported or altered pattern
without changing its geometry. The checkbox also works with a model and tape
evidence alone. Neither mode changes measurement readiness. Explicitly clearing
the report selects tape fitting; an invalid or rejected replacement never falls back to a previous
shape or to tape fitting. The arm-angle control selects the ordinary drape pose,
not an image-frame pose; a refused start remains a refusal at that angle.

**Choose materials** carries the current round's exact files, selected body sources,
leg posture/fraction, settling limit, self-contact and second sewing-path choice into
the Material Workbench. Review the controls, bind materials, and confirm the accepted
fit's wearer association again before draping. Changing a source control while the
new tab is opening cancels that transfer; received snapshots stay local and unchanged.
The optional second path runs a second full solve and reports sewing sensitivity,
not a confidence interval or physical fit.

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
A round without simulation can use `--model` or `--body-fit` for flat checking.
For an ordinary pattern, existing simulated rounds keep their simulation-only
body selection; add `--flat-body` to check the flat pattern on that body too. A
model-derived pattern always requires an explicit body for its later flat checks.
These commands
consume the saved `ftx-body-fit/1` report, not the CLI's enclosing `ftx-cli/1` JSON.
`--model` may be omitted when the converted default is the exact model used by
the fit. Only the shared shape is reused: frame-specific translations, rotations
and joint poses from the images are **not** garment placement poses. `body --pose`
and `drape --arm-deg` select the usual native poses.

## Initial patterns from the selected shape

All three existing programs can use either a tape-fitted model or an accepted body:

```sh
ftx draft zouave_bolero --measurements measurements.json --out tape-bolero \
  --model anny.ftxbody
ftx draft zouave_bolero --measurements measurements.json --out new-bolero \
  --body-fit accepted-fit.json --model anny.ftxbody
ftx draft zouave_trousers --measurements measurements.json --ws workspace \
  --body-fit accepted-fit.json --model anny.ftxbody
ftx draft fez --measurements measurements.json --out new-fez \
  --body-fit accepted-fit.json --model anny.ftxbody
```

The tape path uses the same wearer interpretation, target selection and fitter as
the ordinary body and drape workflows. Supply the required age and tape evidence
and a model with the supported fitting controls. It records the tape fit's
residuals and source identity; it does not create an accepted image-fit report.
Omitting both model flags retains the original analytic drafting path.

The programs retain their existing construction and style controls. Dimensions,
landmarks, local section radii and placement origins come from the selected model's
canonical 35-degree A-pose. Image-frame poses and the later drape arm-angle control
do not change that drafting frame. Missing required model anatomy refuses instead
of borrowing a size-chart dimension. The trouser `body_rise` is the model's vertical
waist-to-crotch distance; it includes no inferred seated allowance. The program's
authored `rise_ease` remains separate, and the recorded basis makes this explicit.

A bolero also needs a separately supplied `hand_pass`; trousers need `foot_pass`.
The body model does not infer these passage measurements. Automatically filled
chart priors and MODEL_FIT records cannot satisfy them. The fez needs no passage
measurement. Other supplied dimensions remain independent evidence and comparison
targets; they cannot silently replace the explicitly selected shape.

A standalone body-derived draft requires a fresh output directory. Alongside
`pattern.json` and `fit_spec.json`, it writes `draft_body.json`,
`validation_body.json`, and selected-body `validation.json`. If an accepted report
was supplied, its exact original bytes are saved in `body_fit.json`; the tape path
creates no such file. The pattern metadata binds the reference body, model and
input identity, any accepted source hash, and supplemental
passage evidence. Workspace drafts save these in a new immutable round, preserving
the original measurement file and calculating readiness from its original records.

In the browser, open **Use a fitted body (optional)** before drafting, load the
model, optionally select an accepted report and confirm its wearer association,
then enable **Use this body for drafting and flat checks**. Changing a source
withdraws the old result and its downloads. The result shows its MODEL_FIT basis
and includes the body evidence and any original report in the downloads and ZIP.
The shared portable entry point is `page::run_on_selected_body`; the WebAssembly
binding is `run_on_selected_body(WebBody, request, today, created, optional_report)`.
The existing accepted-report `run_on_body` entry points remain available.
Each initial draft is flat-validated against the same body that constructed it.

This adds explicit initial drafting; it does not automatically redraft existing
patterns or promote predicted dimensions into measurement records. Synthetic
regressions compare native and portable output, two distinct accepted shapes,
preserved sources, unchanged readiness, refusals and output immutability. They do
not establish anatomical accuracy or physical garment fit.

## Recheck an imported or altered pattern

Choose the intended body explicitly when checking existing geometry:

```sh
ftx validate pattern.json --measurements measurements.json --fit fit_spec.json \
  --model anny.ftxbody --body-fit accepted-fit.json
ftx round workspace --model anny.ftxbody --body-fit accepted-fit.json
ftx round workspace --sim --flat-body --model anny.ftxbody \
  --body-fit accepted-fit.json
```

Omit `--body-fit` to fit the supplied tape evidence on the selected model.
`--flat-body` can also explicitly select the configured default model. Existing
pattern geometry and historical `meta.draft_body` remain intact; the current
check's reference body, passage evidence and residuals are returned as
`validation_body`; workspace and browser rounds save them in `validation_body.json`.
A model-derived pattern refuses an ordinary check without
an explicit body selection, rather than silently using analytic dimensions.

Workspace rounds admit the body before reserving their output and preserve any
accepted report in the round. A combined flat check and simulation consume that
same report snapshot. The simulation also verifies the model hash selected for
the flat check; replacement of the model file produces a saved simulation refusal.
The review shows the flat body's MODEL_FIT basis, residuals and source downloads,
while readiness continues to use the original measurement evidence.

The browser uses the same checkbox for initial drafting, imported patterns and
the current altered pattern. Applying or undoing an alteration keeps the same
wearer inputs and selected body for the next check. Replacing a measurement or
body source still withdraws the old results and accepted-fit association.

## Continue into fit-driven material and manufacturing studies

Selected-body studies use the same reference-pose geometry for their `design_fit`
checks. Supply the ordinary `fabric-fit/1` contract in the study request and the
original measurements in its `ftx-scenario-body/1` body input. Use
`ftx materials study-on-body` for manufacturing alone, or
`ftx materials scenario-study` for manufacturing and numerical scenarios. The
Material Workbench's body-constrained studies use the same portable path.

Bolero and trouser passage points retain their separate evidence: a point naming
`hand_pass` or `foot_pass` requires that supplied value, just as initial drafting
and ordinary selected-body validation do. Automatic chart priors and any used
MODEL_FIT passage source refuse the fit-dependent study. A valid value outside
the original ease band remains a failed fit check and cannot reach manufacturing
or win a study comparison. Other body dimensions always come from the selected
model, even when supplied tapes disagree.

Each fit report binds the used passage value and its original source under
`body_identity.supplemental_measurements`; scenario descriptors retain the same
identity in `design_fit_body`. A study without passage points retains its previous
identity. The study reuses its already selected body, without fitting again or
changing its simulation pose. Source measurements, readiness and historical
drafting metadata remain unchanged. These checks establish numerical fit-contract
and manufacturing admission, not physical garment fit.

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
