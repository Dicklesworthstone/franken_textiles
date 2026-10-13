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

**The page.** `web/index.html` is the skill's `browser/index.html` with its Pyodide worker replaced by
`web/worker.js` (a module worker loading `web/pkg`, built by `tools/build_web.sh`: release wasm32 build plus
`wasm-bindgen --target web`, 2.1 MB). In headless Chromium a bolero round (draft, checks, every export, the
zip) takes about a second; the tape form, the fez, NOT_READY gates and refusals behave as on the Pyodide page.
The fold-up on a fitted body is not in the page yet (`basis.sim` says the command line runs it, as the
reference's page does).

**Error model.** `page` returns the reference's message as a `String` for a request it refuses, and a
refusal's message for a draft or export the library refuses; `web` turns them into `JsError`s.
