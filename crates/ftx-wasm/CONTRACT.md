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

**The fold-up.** `fold` is `ftx drape` without files: `load_body(bytes)` reads a converted model the page
was given (`.ftxbody`, not verified array by array, as the CLI loads it), and `fold_up(model, pattern,
measurements, fit_spec, self_contact_mm, progress)` fits Anny to the tape in the drape pose, trims the
collision skin, sews and drapes (self-contact at `self_contact_mm` when positive, the skill's 2 mm), and
returns the fit report (`sim_report.json` without the CLI's `pattern`, `measurements_sha256`, `body` and
`wall_s`) and the viewer page. `web` exposes `WebBody(bytes)`, `fold_up(body, pattern, measurements, fit,
mm)` → `WebFold {report(), viewer()}`, and calls the worker's `self.ftxProgress(line)` with the drape's
progress lines.

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

**Error model.** `page` returns the reference's message as a `String` for a request it refuses, and a
refusal's message for a draft or export the library refuses; `web` turns them into `JsError`s.
