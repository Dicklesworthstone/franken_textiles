# ftx-wasm

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

**The fold-up.** `fold` is `ftx drape` without files: `load_body(bytes)` reads a converted model the page
was given (`.ftxbody`, with array hashes checked, at most 512 MiB), and `fold_up(model, pattern,
measurements, fit_spec, self_contact_mm, progress)` fits Anny to the tape in the drape pose, trims the
collision skin, sews and drapes (self-contact at `self_contact_mm` when positive, the skill's 2 mm), and
returns the fit report (`sim_report.json` without the CLI's `pattern` and `wall_s`) and the viewer page.
Its `body` diagnostics retain unrounded target/body/signed residual mm, supplied-weight residual kg,
fit iterations and all predicted readings as MODEL_FIT. They use the already-computed TapeFit, not a
second fit. The wasm boundary adds the exact loaded model and saved measurement document hashes.
The page shows residuals beside the cloth verdict, with unavailable data explicit; existing fold
invalidation withdraws both. Targets can be priors, and these diagnostics supply no new measured
evidence, sigma, fit threshold or readiness approval. The CLI's historical rounded body fields are
not a byte-parity claim for this extension. The shared generator embeds the pinned renderer, orbit controls and
MIT notice; downloaded HTML and blob previews need no network for rendering. `web` exposes `WebBody(bytes)`, `fold_up(body, pattern, measurements, fit,
mm)` → `WebFold {report(), viewer()}`, and calls the worker's `self.ftxProgress(line)` with the drape's
progress lines.

The opt-in accepted-fit fold uses `ftx_body::accepted_shape::prepare`, retaining the
original report bytes and exact parameters without tape refitting or image-pose
reuse. It checks the same model/wearer identity and exposes the existing native
arm-angle choice. Report replacement, edits and failed reads invalidate the fold;
no stale shape or tape fallback is allowed. Draft/readiness remain independent.

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

**Timed strips and current-geometry vapor.** `materials::strip_history` and `thermal::vapor_surface`
are file-free adapters to the shared production consumers, exported as `material_strip_history`
and `thermal_vapor_surface`. The material workbench retains complete state/source reports and
exact Rust-generated replay JSON. New request files withdraw their own previous results; material,
fit and geometry changes also withdraw draped vapor outputs. Scenario displays retain independent
initial and motion-endpoint vapor outcomes. These controls do not create fitted selections, replace
existing assignments, infer wearer evaporation or couple latent cooling into dry heat.
