# ftx-wasm

**Local multi-frame Anny fitting.** `body_fit::fit_body(model_bytes, input, iterations,
max_evaluations)` and wasm32 `fit_body` accept the original prepared
`ftx-anny-body-fit-input/1` text and exact converted model bytes. Inputs are bounded to 16 MiB
and models to 512 MiB; SHA-256 identity and every converted array are checked before fitting.
The two work controls are strings parsed as native integers (1..=100 iterations and
1..=1,000,000 evaluations), avoiding JavaScript ABI truncation. The shared
`ftx-body::correspondence` evaluator and existing `ftx-measure` solver preserve all shape/pose
bounds, calibration, evidence, observability, multistart, held-out and budget behavior.
The ordinary `ftx-body-fit/1` report retains exact full-width frame identities, original input
hash, every attempt, residual and refusal; only accepted solutions contain reusable Anny
parameters. Inline reviewed silhouette slices use the same native rules. File-backed capture
inputs and separate mask paths are not followed; their acquisition remains native. No source
photos are loaded, anatomy inferred, MODEL_FIT predictions promoted to evidence or physical-fit
acceptance added. Native/portable integration uses independently authored articulated synthetic
observations, held-out mutations, unobservable motion, capped solves and raster silhouettes.

Optional `capture_images` preserves the native capture's image-only bindings through prepared
inputs and fit reports: exact decoded frame identity, encoded-byte SHA-256, display name,
already-oriented dimensions and recorded rotation. A supplied collection must cover all frames
exactly once; inconsistent, duplicate or incomplete bindings refuse before fitting. Omission
preserves ordinary prepared inputs. These labels never cause file access or certify authenticity.

**Accepted image-frame inspection.** `body_fit::review_body_fit_frame(model_bytes, report, frame_index)`
and its wasm32 export re-evaluate one saved accepted pose without optimization. The report and model
retain the same 16 MiB/512 MiB bounds; model identity/arrays, accepted shape, complete named layout,
frame/pose pairing, parameter bounds and multistart pose agreement must agree before export.
The ordinal is native-parsed text and original u64 frame identities remain exact. Full-mesh keypoints
use the same `ftx-measure::body_fit::review_keypoints` kernel as native mesh export; all supplied FIT/CHECK
silhouette slices use the unchanged triangle-coverage gate. An inconsistent projection refuses the
review without replacing the accepted report. One evaluation supports at most 100,000 vertices and
200,000 faces. OBJ coordinates retain full precision in metres and all model topology.
The photo page draws the original oriented pixel projections and retains the raw review JSON.
Distorted cameras show vertices without invented straight lens-warped edges; unprojectable vertices
remain explicit and the OBJ keeps them. Frame/source changes, cancellation and page exit terminate
the review worker and withdraw its downloads; accepted report/model reuse remains independent.
Pose evaluation loads no photograph, no point visibility or full-mask agreement is inferred, and these diagnostic
meshes are not certified collision skins, measurement evidence or real-wearer fit.

**Declared initialization inspection.** `body_fit::review_body_fit_start(model_bytes, input,
start_index, frame_index)` and its wasm32 export inspect an exact prepared input without invoking
the fitter. Text ordinals select one of at most four starts and eight original-order frames;
canonical preparation joins the selected pose by full-width `FrameId`. All supplied starts,
observations, bounds, model identity and arrays remain checked. A one-start draft can be previewed,
but ordinary fitting still requires two to four distinct starts. The successful preview evaluates
one full requested pose and derives its targets from that same surface, without tape measurement
evaluation. The 16 MiB input, 512 MiB model and 100,000-vertex/200,000-face bounds remain explicit.
`ftx-body-start-frame-review/1` retains exact input/model hashes, named declared values, full OBJ,
all observed keypoints and silhouette samples, raw residuals and unavailable-projection diagnostics.
Large residuals and unprojectable points do not become acceptance refusals or invented predictions.
The local page labels every preview `INITIALIZATION_REVIEW`, never publishes an accepted body or
garment handoff, and retains any separate accepted result independently. Source/ordinal changes,
cancel, delayed file reads and stale worker replies withdraw obsolete previews and image overlays.
Held-out evidence stays fixed; inspecting it does not justify tuning starts to CHECK residuals.

**Local anatomical target authoring.** `body_fit::body_fit_targets(model_bytes, input, edits)` and
its wasm32 export open the same unfitted rest-mesh picker used by native `body-target-review`.
The shared `CorrespondenceModel::target_preview` supplies actual vertex indices, triangles,
bone endpoints and midpoint-shape metadata; no separate browser geometry or inference is used.
An empty edit opens exact prepared source bytes, including an empty or incomplete target map.
Opening adds unresolved observed names to the inspection layout only, so they can be explicitly
defined without inventing locations; all ordinary camera, evidence, pixel, start and budget checks
remain enforced. Explicit application replaces only `model.keypoints` and
requires matching input/model SHA-256 bindings, all observed names, and ordinary target validation.
Invalid indices, duplicate locations, unknown bones and invalid barycentric weights refuse.
Every other JSON value, including exact u64 frame timestamps, remains in Rust; the returned edited
input stays opaque in JavaScript. Limits are 16 MiB input, 512 MiB model and 256 KiB edits.
The photo page embeds the shared picker in a script-only opaque-origin iframe with downloads
enabled. An explicit return requires the current iframe window, a random one-use channel and
matching source hashes. A current native result updates the common prepared snapshot and can
continue into observation editing, start authoring and fitting. Pending definitions block those
operations; closing the picker discards unapplied work, and source changes or page exit invalidate
reads, worker replies and old channels. Targets do not establish anatomy, consent, an accepted body,
measurement evidence or readiness. Existing accepted-body reuse still needs its wearer confirmation.

**Local initialization authoring.** `body_fit::body_fit_starts(model_bytes, input, edits)` and its
wasm32 export prepare named shape/pose controls from the same checked model and input. Empty edit
text returns the original input bytes unchanged, including zero-start drafts. Explicit edits
replace only one to four starts, joining original frame ordinals to native full-width identities.
All observations, bounds, cameras, source images and FIT/CHECK roles stay fixed. Duplicate starts,
missing or repeated frame ordinals, nonfinite values and boundary/out-of-domain values refuse
through ordinary native admission. The returned input can immediately be previewed, downloaded
and fitted; a one-start draft is inspectable but fitting still requires two to four distinct starts.
Edits are limited to 64 KiB alongside the existing 16 MiB input and 512 MiB model limits. Every
result names both original and returned input hashes, with no optimization or accepted body.
Edit requests may bind `input_sha256` and `model_sha256` to the exact supplied source bytes; the
pair must be complete and match when present. Older callers may omit both. The browser always
sends both, retains drafts across cancelled file choices, and invalidates in-flight applications
when a newer edit arrives. Reset and actual source selection discard the editor; a cancelled
file choice never silently reloads the original input over applied values.

**Manual prepared observations.** `body_fit::body_fit_observations(model_bytes, input, edits)` and
its wasm32 export open empty or populated keypoint/contour drafts and replace explicitly supplied
per-frame keypoints and manual silhouette slices. Both source hashes are required for edits; original frame ordinals join to
native full-width identities. Canonical `prepare_capture_authoring` retains camera, target, pixel,
uncertainty and budget checks. Existing point uncertainty and FIT/CHECK roles cannot change;
new points require an explicit declared target, pixel, sigma and role. An optional per-frame
`silhouette_edit` explicitly replaces or removes manual contours; omission preserves the original
silhouette field, including absence or null. Replacements supply evidence and the canonical
1–64 distinct horizontal/vertical slices with at most eight ordered disjoint foreground intervals
each. Existing slices at the same axis/coordinate retain their sigma and FIT/CHECK partition.
Zero lens distortion is required. Imported-mask source receipts remain read-only and cannot be
asserted through a manual replacement. Every other input field, including starts, cameras, targets
and source images, remains fixed. Limits are 16 MiB input, 512 MiB
model and 256 KiB edits. Each frame's opaque `OBSERVATION_REVIEW` image receipt can use the existing
strict image verifier before a start exists; its already-oriented pixels receive no second rotation.
The photo page annotates named targets and sampled intervals on those verified original pixels or
accepts explicit stored pixel coordinates. Each frame reports whether manual silhouette editing
is available and why. Observation and initialization editors share one exact native prepared snapshot;
each application binds the current snapshot hash. Pending changes withdraw stale previews, fits and
handoffs, while cancellation restores the last applied values and reset restores the uploaded source.
This authoring path performs no model evaluation, fitting, anatomy inference or readiness promotion.

**Original capture image inspection.** An accepted frame review retains `source_image` when its
report has complete `capture_images` or an older native `capture_input` receipt. Both must agree
when present. `body_fit::BodyFitImage` and wasm32 `WebBodyFitImage` accept only explicitly supplied
image bytes and the exact review text. Initialization reviews retain the same source binding and
are admitted only with `status: INITIALIZATION_REVIEW` and `body_fitted: false`; their image receipt
keeps that review kind. Observation reviews likewise require their explicit status and unfitted flag.
All three review modes verify the original encoded hash, frame, dimensions
and rotation before publishing a lossless PNG and an image/review identity receipt. The shared
native capture decoder accepts opaque 8-bit PNG or strict JPEG with absent/identity EXIF; recorded
rotation has already been applied and is never applied again. Bounds are 16 MiB review, 128 MiB
encoded image (the JPEG decoder retains its 64 MiB limit), and 40 million pixels. Only the original
basename is retained for display; no source directory is exported. Pixels stay outside saved fit
reports and garment handoffs; selecting a matching image changes neither acceptance nor measurement readiness.

**Still-window candidates.** `photo::still_window(input, source)` and the matching wasm32 export delegate to
`ftx-measure::still::assess_still_window`, with the existing 256 KiB browser JSON limit. The shared
photo geometry worker preserves exact source/report text, the original filename and source-byte hash,
early/late displacement and native FIT, held-out, reprojection and parallax checks. A passing
`STILL_CANDIDATE` is only a filter for human
review; it creates no triangulation capture assertion, measurement record, READY or fit claim.

**Anchored joint refinement.** `photo::bundle_adjust(input, iterations)` and the matching wasm32
export call the existing `ftx-measure::bundle::adjust`. Input JSON is bounded to 256 KiB; the step
limit is text parsed in Rust and must be an integer in 1..=100, avoiding JavaScript ABI truncation.
Fixed metric controls, lens/capture/rank/parallax/held-out/convergence guards and failure reports
remain native. The existing photo geometry section shares source/report/download cancellation
between this mode and fixed-camera triangulation. Joint mode clears separate click-noise input;
its original observation sigmas supply fit weights. Exact input/report and submitted limit text
remain downloadable. Failed/budget-stopped initial guesses never become a solution. No covariance,
global optimum, measurement record, READY status, body-fit or empirical accuracy claim follows.

**Calibrated landmarks.** `photo::triangulate` and the wasm32 `triangulate(input, click_noise)`
export call the existing `ftx-measure` geometry and conditional click-noise estimators. Each source
is bounded to 256 KiB; empty noise requests geometry alone. Native parallax, lens, capture and
held-out gates remain authoritative. A valid point with unavailable requested covariance retains
both statuses. Exact input, optional noise and report text are downloaded without JavaScript
rewriting frame timestamps. Results create no measurement record, READY state or body-fit claim.

**Photo section estimates.** `photo::photo_section` and the wasm32 `photo_section(width_source,
depth_source, options)` export delegate to the shared `ftx-measure` source-qualified section
engine. Exact original measurement and option texts survive in `section_sources`; native fusion,
same-wearer/provenance/conflict guards and input limits apply before estimation. An empty depth
text reuses one combined document. The result is an ordinary PHOTO_ESTIMATED girth document, or
a coefficient-only tape calibration without usable measurement records. No browser numerical
model, inferred landmarks, readiness or real-person fit claim is added.

**Configured garment drapes.** `fold::fold_up_configured` and the browser export
`fold_up_configured(body, pattern, measurements, fit_spec, settings_json, body_fit?)`
accept explicit `self_contact_mm`, `two_start`, `arm_abduction_deg`, `legs`
(`standing`, `seated`, `squat`), `leg_fraction` (0..=1), and `max_settle_steps` (0..=256).
Omitted settings mean 0 mm self-contact, one start, 35° arms, standing legs, full leg fraction,
and no additional settling. Unknown fields and invalid values refuse. Imported body reports remain
exact source text; the configured arm angle must agree with `BodyFitSource.arm_deg` in Rust calls.
Tape fitting and nappy calibration keep their reference pose; requested poses change only garment
placement, collision and display geometry. The bounded settling tail continues the sewn state at
the original time step, retains its actual duration, and obeys the shared history-law restrictions.
Legacy fold exports retain their signatures and default behavior. Browser callers require the new
export for nondefault leg/settling settings, so older bundles cannot silently discard them.

Configured settings also accept optional `static_equilibrium` with explicit
`correction_tolerance_m` and `max_newton_iterations`. The same native independent conservative
solve adds an available/refused report and exact static geometry, without changing the dynamic
fold, elapsed time, flags or subsequent motion. Friction and material-history evolution are not
part of this static comparison. Omission preserves the existing settings serialization.

Configured settings also accept optional `time_subdivision: { max_halvings, max_trials }`.
The integer limits are 0..=16 halvings and 1..=65535 attempted steps; the allowance must cover the
requested sewing and draping intervals and is shared with any extra settling. Only a stalled Newton
solve retries at smaller intervals. The native transactional solver retains the nominal duration and
output frames, reports each accepted partition and refuses an exhausted allowance without committing
a partial interval. Omission preserves ordinary fold behavior. This recovers convergence without
loosening its tolerance; it does not estimate time-discretization error. Portable material/scenario
studies use the same option and reserve its full trial allowance within their vertex-frame budget.
Both browser fold forms expose an explicit opt-in with default limits of 8 halvings and 4096 trials.
Both workers validate the exact two-field policy, require the configured export when enabled and
preserve it in saved settings and checked pattern/material handoffs. Disabled forms
omit it entirely. Editing the checkbox or either limit withdraws stale drape and downstream outputs
and cancels in-flight work and pending handoffs through the existing lifecycle guards.

**Purpose.** FrankenTextiles in the browser (plan §15): the page's round behind a thin `wasm-bindgen`
boundary, replacing the skill's Pyodide page. `page` is the reference's `webapp.py` (`program_info`,
`form_fields`, `body_file`, `run`): a body from a size chart, tape readings or a measurements file; a
measured draft (`zouave_bolero`, `zouave_trousers`, `fez`, with style overrides; nappies set the trousers'
`nappy` style) or someone else's pattern (with its fit spec, or role-based default points); flat validation,
the measurement gate (`round_readiness`), every export with its acceptance checks, the round status and its
basis; the documents (`pattern.json`, `fit_spec.json`, `measurements.json`, `validation.json`,
`readiness.json`), the exports and one zip, all in memory. `web` (wasm32 only) exposes `program_info()`,
`form_fields(args, today)` and `run(req, today, created)` → `WebRound {page(), zip_name(), zip(), file(name)}`
to the page's worker as JSON text and bytes. The numerical crates never see browser types.

Uploaded source metadata retains signed `i64` and full unsigned `u64` JSON integers through
the shared `PyJson` adapter and saved round documents. In particular, an actual photo report's
`bootstrap.seed` and `measurement_options.seed` remain exact when used in the primary
measurements file. Numerical conversion to a physical floating-point input still follows the
ordinary range checks; out-of-range integer literals refuse instead of being rounded. This
does not add arbitrary-precision numerical calculations or fit evidence. Test:
`page::full_width_photo_seed_and_source_metadata_survive_an_ordinary_round`.

**Direct alterations.** `alteration::targets` and `alteration::apply` expose the existing five native
operations to the primary Pattern Workbench: slash-spread, vertex movement, seam truing, cap ease and
curve transfer. Controls use actual drafted IDs and expanded physical-instance edges. Each pattern is
limited to 4 MiB, 128 pieces and 8192 drafted edges; operation text to 16 KiB. Source and result pass
ordinary structure checks before publication. The CLI's ordered serializer preserves unrelated source
fields, signed zero and full-width integer metadata while appending the canonical history sentence.
The browser adopts only a successful current candidate, retains one-step undo, and reuses the ordinary
round for seam, fit, measurement and export checks. Refusals retain the working pattern. Pending edits,
replacement files and superseded messages withdraw earlier round/drape/material handoffs; an explicitly
refused curve source never falls back to another pattern. No CAD curve editor or physical-fit claim is
introduced. Tests reuse captured alteration bytes and the existing page/worker lifecycle seams.

**Material treatments and production.** `materials::treatment` and `treatment_pattern` expose the
shared source-qualified before/after state and grain-axis geometry consumers within a combined
64 MiB input budget. Browser exports `material_treatment` and `material_treatment_pattern` retain
exact report, selection, source and derived-pattern JSON for replay; no browser-side physics law
or property fill is introduced. Existing sewing/study APIs accept the shared optional production
policy and retain copy-scoped traversal, actual batch totals and qualified ordering proof. Tests:
`material_treatment`, `sewing_production`, and the material worker/page regressions.

The same adapters accept `production.parallel` with independently qualified whole-garment cells.
Plan `/16` and stitch program `/5` retain actual cell/global-copy assignment, per-cell serial histories,
raw feed, explicit millisecond padding and elapsed makespan with its lower bound. The recipe selector
accepts `/5` for `minimize_parallel_makespan`; studies use the distinct nominal
`sewing_parallel_makespan_seconds` metric. Aggregate configured work includes every nested schedule
and allocation. Browser results display actual selected cells rather than the outer serial reference,
preserve exact source downloads and withdraw stale views. Complete native/portable and compiled-worker
replay cases exercise the same shared authorities; no browser scheduler, machine transfer or hardware
authorization is introduced.

The existing study boundary also accepts canonical treatment design operations, including
grid/adaptive post-operations, without a separate browser law. Exact intermediate stage strings
remain in the ordinary report. The page exposes before/after/request downloads for selected,
screened and adaptive designs while retaining final admission/refusal status; stale edits revoke
their links. Sewing and study pages show finite fixture capacities and before/during/after
occupancy from the source-qualified production report. They do not recompute scheduling or infer
fixture geometry/hardware approval. Native command parity cases and the opt-in compiled-WASM
worker case cover measured treatment studies and capacity-dependent constrained schedules.

The same sewing exports enforce optional source-qualified `passes[].access` policies over every
complete needle chord in the explicit operation-local assembly. `ftx-sewing-plan/14` retains tool
clearances, limiting witnesses and exact machine/process locks; an edited receipt is never an input
shortcut. Direct sewing and study pages display those retained results and withdraw stale views.
Missing receipts stay unknown. This static planar model grants no grip, machine-registration,
three-dimensional approach or hardware-execution claim. The complete synthetic access example is
covered by core, native/portable and opt-in compiled-WASM worker regressions.

**Clock.** An input, never read: `today` (`YYYY-MM-DD`) dates tape files and judges staleness, `created`
(`YYYY-MM-DDTHH:MM`) stamps the exports and the zip entries. The worker passes the wearer's local date and
minute, as `webapp.py` reads them.

**Parity.** `tests/fixtures/oracle/web/webapp.json` (`tools/oracle/capture_webapp.py`, the reference's
`webapp.py` with its clock fixed) is reproduced by `tests/page.rs`: 25 cases — `program_info`; `form_fields`
for every program and for someone else's pattern, with and without nappies, at two statures; `run` on chart,
tape and file bodies, all three programs, a style override, someone else's pattern with and without a fit
spec, a BLOCKED pattern, an UNSCALED trace, and five refusals (an unknown program, a stature, an unknown or
non-positive tape reading, an invalid file) with the reference's messages. Every page field (status, basis,
validation, readiness, exports, table, solved, title, the key files) and every document is equal (numbers
to 1e-9 relative); the exports' bytes are the exporters' own (their fixtures). Style defaults list their keys
alphabetically (the drafting programs return sorted maps); the reference lists them in definition order.

**Uploaded evidence freshness.** File-mode measurement documents are validated and re-fused from their
records even when `fused` is nonempty. Cached values cannot override edits, invent independent evidence
from MODEL_FIT records or supply a stale readiness result. The original records and caller input remain
unchanged; the round's saved measurements, validation, draft and later fold inputs use current fusion.
Readiness already re-fused records; this repair aligns the draft and saved provenance with that gate.
This explicit reference exception is covered beyond the unchanged oracle cases in `tests/page.rs` and
the actual compiled-browser upload/download checks in `tools/check_web.cjs`. It does not estimate new
uncertainty or establish real-body fit.

**Supplemental measurements.** Tape/file bodies may import up to eight original measurement-document
strings (16 MiB total, 4096 combined records), with explicit same-wearer association. Shared validation
and fusion consume their records, never cached values; source text and photo uncertainty reports remain
in `measurement_imports`. Reference/demo mixing refuses. An unknown source date keeps the aggregate date
unknown; otherwise the oldest date is retained. This is no per-record freshness or independence proof.
The original primary date remains in `measurement_primary_taken_on` when the aggregate date changes.
Source/body edits reset consent, invalidate downloads and discard superseded reads/replies. The imports
remain local and never enter browser storage. The numerical fusion and readiness rules are unchanged.

The Material Workbench uses the same `measurement_file(body, today)` binding before exports,
drapes and body studies that select its primary measurements. It retains the original supplemental
strings and uses the fused document throughout each consumer; drapes also download that document.
Explicit same-wearer confirmation is required again after evidence replacement. Separate body-request
studies retain their own evidence. Malformed or refused supplements never fall back to the primary alone.

**The fold-up.** `fold` is `ftx drape` without files: `load_body(bytes)` reads a converted model the page
was given (`.ftxbody`, with array hashes checked, at most 512 MiB), and `fold_up(model, pattern,
measurements, fit_spec, self_contact_mm, two_start, progress)` fits Anny to the tape in the drape pose, trims the
collision skin, sews and drapes (self-contact at `self_contact_mm` when positive, the skill's 2 mm), and
returns the fit report (`sim_report.json` without the CLI's `pattern` and `wall_s`) and the viewer page.
Its `body` diagnostics retain unrounded target/body/signed residual mm, supplied measurement
`sigma_mm`, the effective fitting scale and the residual divided by that scale, supplied-weight residual
kg, fit iterations and all predicted readings as MODEL_FIT. The shared fitter uses the larger of each
supplied uncertainty and its existing tape-functional tolerance; absent uncertainty retains the
existing tolerance. Explicit invalid uncertainty refuses. The native commands and material studies
retain the same diagnostics. These are fitting weights and residuals, not confidence intervals or
newly estimated measurement uncertainty. They use the already-computed TapeFit, not a second fit.
Only requested tape readings must be available to fit the body. Unavailable unrequested sections
remain absent from `readings_mm`; `tape_evaluation` identifies partial coverage and its exact
`unavailable_fields`. A missing requested section refuses instead of dropping its constraint.
Explicit `wearer.nappies: true` requires fused `seat_over_nappy` and applies the same separately
reported calibrated occupancy used by native body export and material studies. Bare shape/source,
mass and fit residuals remain intact. `nappy_occupancy` retains the original target evidence and
occupied reference geometry; `readings_mm.seat_over_nappy` is its computed MODEL_FIT seat. Every
requested pose reuses the one 35-degree calibration. Missing/unreachable active targets refuse
instead of falling back to bare geometry; no nappy material properties or readiness are inferred.
The fold report includes `qualification_basis`, `tightness_qualified: false`, and `collider_tightness_qualified: false`;
because a browser fold-up has no workspace `qualify.json` (`ftx qualify`), `fold_on_source` applies
`ftx_drape::qualify::gate_tightness` before rendering `fold_viewer.html`, demoting any unqualified
`tightness_gating` flags out of `gating` into `advisory` in both `Fold.report` and `Fold.viewer`
(spec 04 §7.3). When `tightness_gating` is empty, `Fold.viewer` remains byte-for-byte identical to
standalone `ftx drape`. The wasm boundary adds the exact loaded model and saved measurement document hashes.
The page shows residuals beside the cloth verdict, with unavailable data explicit; existing fold
invalidation withdraws both. Targets can be priors, and these diagnostics supply no new measured
evidence, inferred sigma, fit threshold or readiness approval. The CLI's historical rounded body fields are
not a byte-parity claim for this extension. The shared generator embeds the pinned renderer, orbit controls and
MIT notice; downloaded HTML and blob previews need no network for rendering. `web` exposes `WebBody(bytes)`, `fold_up(body, pattern, measurements, fit,
mm)` → `WebFold {report(), viewer()}`, and calls the worker's `self.ftxProgress(line)` with the drape's
progress lines.

The accepted-fit Rust entry groups self-contact thickness (mm) and the paired-sewing choice in
`fold::FoldOptions`; JavaScript keeps its existing arguments. These controls reach the same fold
implementation for both body sources, with no added defaults or changed numerical settings.
The opt-in accepted-fit fold uses `ftx_body::accepted_shape::prepare`, retaining the
original report bytes and exact parameters without tape refitting or image-pose
reuse. It checks the same model/wearer identity and exposes the existing native
arm-angle choice. Report replacement, edits and failed reads invalidate the fold;
no stale shape or tape fallback is allowed. Draft/readiness remain independent.

**Standalone body check.** `fold::inspect_body` and the WASM `inspect_body` export use the same tape
fitter/raw diagnostics or exact accepted-shape importer without constructing or simulating a garment.
The `ftx-body-check/1` result retains the body description, exact measurement/model hashes and
`cloth_simulated: false`. Record-bearing inputs are validated and re-fused; each JSON source is bounded
at 16 MiB. The worker checks round/model identity and explicit accepted-report association. Shared
model/report controls remain available for flat-only garments; source edits and stale responses withdraw
the separate check. MODEL_FIT predictions and residuals cannot change measurement readiness or certify
physical fit. The ordinary fold-up remains a separate action with its existing placement/contact gates.

**The page.** `web/index.html` is the skill's `browser/index.html` with its Pyodide worker replaced by
`web/worker.js` (a module worker loading `web/pkg`, built by `tools/build_web.sh`: release wasm32 build plus
`wasm-bindgen --target web`, 3.2 MB). A round (draft, checks, every export, the zip) takes about a second.
After a round whose pattern has fold-up placements, the page offers the fold-up: the wearer picks the
converted model file (145 MB, read in about 0.1 s, kept for later rounds), and the worker drapes the round's
own `pattern.json`, `measurements.json` and `fit_spec.json`, then shows the flags, the viewer in the page
and both files to download. `basis.sim` keeps the reference's "not run" text, since the round itself does not
drape. `tools/check_web.cjs` (Playwright) drives it in headless Chromium: every program's round, an empty
tape form NOT_READY, a refused stature, and with `--model` both garments folded up (bolero 25 s, trousers
76–85 s; natively 22 s and 57 s) with viewer pages byte-identical to `ftx drape`'s on the same documents
(bolero `c8a4ae42…`, trousers `3c5b499e…`), so the drape is bit-identical across native and the browser.
These sizes, timings and viewer digests describe the historical browser checks of 2026-10-13;
they are not benchmarks or acceptance of a newer artifact under the numerical settling gate.

The workshop withdraws round downloads on measurement, garment or style edits, and withdraws only
the drape when the body model or self-contact setting changes. Worker replies and fold source
documents carry request identities; superseded replies and asynchronous file reads cannot publish
results. A cleared or refused replacement model leaves folding disabled until a current model loads.
Round and fold handles are freed even when extraction refuses. `check_materials_frontend.cjs` covers
these lifecycle paths with explicit DOM/WASM doubles; those checks establish no numerical or fit claim.
Obsolete synchronous solver work may finish in the worker, but its result is not published.

**Error model.** `page` returns the reference's message as a `String` for a request it refuses, and a
refusal's message for a draft or export the library refuses; `web` turns them into `JsError`s.

**Local photo measurements.** `photo::Photo` and wasm32 `WebPhoto` share the native strict photo
decoder, plain Letter/A4/auto detector and unrounded uncertainty calibration. `report` retains
scale diagnostics and exact source hash/orientation without a filesystem path. `preview` encodes
the actual oriented pixels losslessly; `measure` accepts full-resolution named point pairs and
native uncertainty options, producing ordinary `fabric-measurements/1` PHOTO_ESTIMATED records
with unknown capture date. JSON is limited to 256 KiB per document; encoded images to 64 MiB and
decoded images to 16 million pixels. Existing engine bootstrap/point/detector limits still apply.
The disposable worker/page in `web/photo*` keeps source pixels local, cancels old requests and
withdraws affected downloads on edits. Plain-paper detection does not identify physical paper,
body landmarks or girths. Explicit ChArUco sidecars and retained same-lens calibrations use
`Photo::with_reference`; empirical camera/print accuracy and fit remain outside this browser slice.
See `docs/photo-measurement.md`.

**Printable references.** `WebCalibrationSheet` delegates to `ftx-trace::calibration_sheet::render`,
the same renderer as native `measure calib-sheet`. PNG, lossless PDF and path-free sidecar downloads
retain identical native bytes. It performs no file IO/upload, inference or print-scale qualification.
Invalid names/dimensions/layouts refuse without a partial sheet. See spec 02 A.9 and the trace contract.

**Local camera calibration.** `photo::camera_calibration` / wasm32 `camera_calibration` fit original
coordinate JSON with optional original corner-noise JSON through `calibrate_document`.
`photo::CameraCalibration` / wasm32 `WebCameraCalibration` accept an original image manifest and
sheet, expose exact `image_ids`, acquire one selected PNG/JPEG per `add_image`, and solve the complete
batch with non-consuming `finish`. The shared acquisition authority freezes capture and held-out
identities, checks duplicate decoded pixels and retains all raw corners; it runs no photo-scale
RANSAC. JSON is bounded to 16 MiB; the native 32-image, 64-MiB encoded/image, 16-million-pixel/image
and 128-million-pixel batch limits apply. Original text, image identities, detector receipts and
requested uncertainty survive in the ordinary retained report, which `Photo::with_reference`
recomputes before reuse. Rejected calibrations remain inspectable but supply no usable camera.
The disposable browser worker reads selected files sequentially and uploads nothing. Source edits
cancel pending acquisition and withdraw stale generated calibration/photo results. Tests cover known
coordinate answers, Brown covariance, independently rendered multi-view images, native receipt
equality, direct photo reuse, refusal paths and actual compiled-WASM replay. See
`docs/camera-calibration.md`; these are synthetic software checks, not empirical camera qualification.

**Potential vapor transport.** `thermal::vapor` and wasm32 `thermal_vapor(input)` delegate the
bounded `ftx-vapor-stack/1` request to `ftx_pattern::material_vapor`. They preserve canonical Ret
observations, source/evidence refusals, explicit boundaries and exact frozen replay JSON. Their
potential heat/water outputs do not establish sweat supply, comfort or coupled dry/wet histories.
See `docs/vapor-resistance.md`.

`thermal::vapor_coverage` / wasm32 `thermal_vapor_coverage(input)` expose the same bounded
geometry-derived layered coverage consumer as the native command. Retained material selections,
body/cloth geometry, explicit vapor boundaries and frozen replay stay with the report. Missing dry
thermal properties do not prevent this independent consumer; uncovered body transfer remains unknown.

`thermal::vapor_coverage_history` / wasm32 `thermal_vapor_coverage_history(input)` replay the same
bounded moving-coverage potential as the native history command. Scenario motion can opt in with
`vapor_coverage_history` while reusing its existing `vapor_coverage` geometry and boundary policy.
The browser displays complete sample coverage and potential integrals, preserves exact replay bytes,
and withdraws stale histories on dependent edits. This evolves no moisture state or latent cooling.

**Selected-body manufacturing.** `materials::scenario::manufacturing_on_model`
and `material_study_on_body` reuse the ordinary selected-body and flat-fit
authorities with `Request::evaluate_with_fit_body`, retaining the existing report
schema and identities. Explicit `design_fit` is required; body setup and
scenario-dependent objective failures refuse rather than falling back. No cloth
session or thermal solver is invoked. Browser downloads keep original pattern,
study and body-input text; dependent edits/cancellation withdraw stale outputs.

**Complete mixed-stock jobs.** `manufacturing::cut_job` / wasm32
`material_cut_job(pattern, request)` invoke the canonical explicit roll/finite-stock routing planner.
The combined JSON input limit is 64 MiB. Every original instance/copy is accounted for once; pricing
is independently reported, and the exact frozen pattern/request strings can replay the result offline.
Existing material-study/scenario transports expose this outcome without another browser planner.

**Same-plan roll preview.** `manufacturing::roll_job` / wasm32 `roll_job` builds the complete
`CutPlan` once and renders only the requested marker from that same object. The plan remains exact
Rust JSON text; the escaped combined response is bounded to 64 MiB using the same serializer as
`stock_job`. An invalid marker index refuses; a preview-only refusal preserves the complete plan.
The current material worker uses this combined adapter and keeps the earlier exports for older
bundles. This removes a repeated planning call, without a measured latency or optimality claim.

**Finite-strip characterization.** `materials::strip_experiment` and `calibrate_strip` expose
the shared `strip::evaluate` and `strip::fit` authorities without filesystem access. Their wasm32
exports are `material_strip_experiment(input)` and `material_calibrate_strip(input, allow_estimates)`.
Input-size, evidence, physical-model, numerical-work and constitutive-domain guards are shared with
native consumers. Explicit output estimate consent overrides the effective request's output policy
but does not admit estimated observations. Complete unaccepted fits retain reasons and predictions
with null material/selection fields; exhausted work and failed solves become ordinary errors.

The material workbench displays finite geometry and warp axes, FIT/CHECK residuals, search bounds,
local strain support, mesh refinement, source-domain replay and actual reaction/width predictions.
Accepted exact selection JSON enters the existing pattern-binding path; absent bending/contact or
other evidence remains unavailable. Replaced, refused or unaccepted results cannot leave a stale
selection assigned. Native adapter tests cover actual physics, immutable source identities and
refusals; `check_materials_frontend.cjs` additionally executes the compiled WASM through the worker,
with optional complete native calibration replay under `FTX_STRIP_REPLAY`. See
`docs/clamped-strip-calibration.md` for the scope and executable synthetic acquisition.

The same boundary accepts independent signed tangent intervals under `nonlinear`, separately from
the existing fixed-shape `response_family`. The page displays every returned coefficient with its
canonical descriptor and bound, and shows the declared fixed strain knots. Compiled-worker checks
replay the accepted ordinary material on a refined held-out strip and optionally compare the complete
native report and exact selection under `FTX_INTERVAL_STRIP_REPLAY`. These synthetic checks do not
qualify a measured textile or supply absent bending, mass, contact or thermal evidence.

**Measured membrane, bending and contact.** `materials::calibrate_linear`, `calibrate_bending` and
`calibrate_friction` expose the existing source-qualified calibration engines with a 64 MiB input
budget. Their wasm32 exports use the `material_` prefix and preserve exact Rust-generated selections
and bindings. Fitted-output permission never admits estimated input observations or missing material
properties. Pair friction produces a separately scoped interface and per-sheet readiness, with no
replacement selection. `material_stage_interface` delegates to `Changes::with_interface`: it retains
the effective table, requires explicit same-pair replacement consent, validates all final assigned
hashes, and returns exact `interfaces_json` / `changes_json` without changing source geometry.
`tests/materials.rs` replays actual material/contact consumers and preserves incomplete readiness,
independent consent, retained numeric evidence, and refusal of stale or duplicate interfaces.

**Evaluated GarmentCode import.** `import_garmentcode(input, annotations)` calls the canonical
`ftx-pattern` importer without files or network access. Both original inputs retain their separate
4 MiB limits and exact source hashes. The response keeps the converted `pattern` and review `report`,
and supplies Rust-generated `pattern_json`, `report_json` and optional `fit_spec_json` strings. These
pretty JSON strings include a final newline and match native CLI artifacts byte-for-byte, preserving
signed zero and round-trip-sensitive numbers without a browser serialization step. The complete
escaped response must fit the existing 16 MiB output limit.

The materials workbench separates conversion/review from explicit pattern adoption. Adoption uses the
existing original-pattern lifecycle, clearing old assignments, interface substitutions, outfit maps,
combinations and downstream results. Source/annotation edits cancel pending work and withdraw stale
reviews and adopted inputs; a delayed read or worker reply cannot replace a later pattern. Original
inputs and all native artifacts stay downloadable. Optional fit remains an explicit separately chosen
consumer input. `REVIEW_REQUIRED`, missing construction semantics and source limitations remain visible;
conversion does not supply measured body readiness, physical fit or complete garment construction.

**Joint-outfit material workbench.** `materials::outfit::prepare` and its wasm32 export
`material_outfit_prepare(input)` use the production outfit composer to freeze an `ftx-outfit-drape/1`
request into one ordinary pattern. The import retains exact request bytes, their SHA-256, exact
composed pattern JSON and the source composition receipt. Whole-outfit, per-garment and per-piece
material substitutions enter the existing binding, drape, cutting, purchasing and study consumers.
The initial composition receipt remains explicitly scoped before substitutions; consumers retain
their current pattern separately. Imported outfits require self-contact, enforced both by the page
and the worker. Replaced imports invalidate dependent results and reject stale asynchronous replies.
The workbench uses the ordinary composed-pattern drape route; it does not append the native
`Prepared::drape` runtime garment-ownership report or claim successful physical dressing or fit.

**Timed strips, cylindrical holders, and current-geometry vapor.** `materials::strip_history`,
`materials::cylinder_experiment`, `materials::calibrate_cylinder`, and `thermal::vapor_surface`
are file-free adapters to the shared production consumers, exported as `material_strip_history`,
`material_cylinder_experiment`, `material_calibrate_cylinder`, and `thermal_vapor_surface` (interface stamp `ftx-wasm-interface/2026-11-11.4`). The material workbench retains complete state/source reports and
exact Rust-generated replay or conditional `Selection` JSON (`cylinderFit` adopts an accepted selection only when `calibration.accepted === true`, never promotes `reported_thickness` to unloaded contact `thickness`, and withdraws stale outputs on request changes). New request files withdraw their own previous results; material,
fit and geometry changes also withdraw draped vapor outputs. Scenario displays retain independent
initial and motion-endpoint vapor outcomes. Forward experiment controls do not create fitted selections, replace
existing assignments, infer wearer evaporation or couple latent cooling into dry heat.
