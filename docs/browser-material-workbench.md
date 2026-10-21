# Local browser material workbench

`web/materials.html`, linked from the main drafting page, connects material selection to the existing
native engines. Build `web/pkg` with `sh tools/build_web.sh`, serve `web/` with
`python3 -m http.server -d web 8000`, and open `http://localhost:8000/materials.html`.
The build needs the repository's pinned Rust toolchain, wasm target and wasm-bindgen CLI; the page
has no Python or native-library runtime dependency. It does not install or operate a machine.

## Select once, reuse the existing consumers

Search the unified built-in catalog, select an exact record, and inspect its original observations,
normalized units, conditioning, sources and independent capability refusals. Alternatively, load a
portable `Selection` JSON containing a local characterization record. The page does not fetch supplier
pages or add a second database. Estimates remain disabled unless the selection explicitly admits them.
A source-locked record cannot be modified while keeping its old digest.

Load an original `fabric-pattern/1` pattern and assign an inspected selection to its default or an exact
source-piece ID. A source-piece assignment affects that piece's mirrored/cut instances. Freeze and bind
creates a new in-memory pattern through `ftx_pattern::material_binding::Changes`; it never edits the
uploaded file. It freezes all effective selections, including untouched explicit pieces. Changing the
default preserves an already assigned lining or cuff. Grain, vertices, curves, seams, simulation roles
and cut quantities retain their original values. The optional **Replacement material-pair friction
evidence** file replaces the complete interface list in the same operation. It uses exact final
material hashes and independently sourced observations; the Rust binding validates those identities.
Omitting the file preserves the original list, uploading `[]` explicitly clears it, and **Keep original
interfaces** discards the pending replacement. See [piece-materials.md](piece-materials.md) for the
pair schema and the equivalent native `bind-changes` command.

Changing a pair file withdraws the current bound pattern and dependent outputs. Invalid or unfinished
reads disable binding until corrected or explicitly cleared; delayed reads cannot attach to a newer
pattern. Estimate admission belongs to each pair and remains separate from sheet-material consent.
Binding downloads include `original-pattern.json` and the exact `material-changes.json` request for
replay, alongside `pattern.json` and `material-bindings.json`.

`material-bindings.json` contains original/bound pattern hashes, the selected identity per source piece
and the production drape resolver's result with and without self-contact. These are material-input
checks, not evidence that meshing, contact or a complete simulation will succeed. Partial materials
can still be selected for a supported manufacturing or thermal consumer. No missing coefficient is
filled from a fiber name, construction label or a brand.

The bridge retains a `pattern_json` string and original Rust drape/report JSON. JavaScript parses
copies for display but does **not** reserialize source-bound numeric data for subsequent consumers or
downloads. Otherwise values such as `1.0` and `-0.0` change representation and invalidate the existing
material/geometry receipts. Nested coverage requests preserve validated JSON fragments verbatim;
they do not replace the hash checks or manufacture new receipts.
The existing drafting worker and its `sim_report.json` download use the same lossless rule.

The remaining operations consume this same bound pattern:

| Operation | Additional local inputs | Existing authority and output |
|---|---|---|
| Validate/export | Measurements and fit-specification JSON | `ftx_wasm::page::run`; current verdict basis, measurement readiness and checked export ZIP. No implicit drape/fit claim. |
| Mechanical drape | Measurements, fit specification, converted `.ftxbody` model | Existing `fold_up` fits the same supplied body model and invokes the production drape engine. Current report and fold-up viewer, or an explicit refusal. |
| Cutting | `CutOptions` JSON | `ftx_export::cutting::CutPlan`; all material-specific roll requirements and a bounded first-roll SVG preview. Not a machine-coordinate program or proof of optimal nesting. |
| Purchasing | Cut options, dated `QuoteBook`, comparable `Market` scope | Existing marker procurement evaluates gross stock including layout waste and order terms. Missing offers remain unknown, not zero-cost purchases. No checkout or live refresh runs in the page. |
| Sewing | Complete `SewOptions` recipes and machine envelope | Existing `SewProgram`; every declared stitch interval and process refusals. Execution remains unauthorized. |
| Independent sheet heat | A fresh workbench drape and `SurfaceConditions` JSON | Existing `thermal_steady_surface`; material-following fields, optional declared exterior exchange and source-locked replay. |
| Layered body coverage | A fresh workbench drape, body/instance-layer map and coverage conditions | Existing `thermal_steady_coverage`; recomputed overlaps, covered/bare accounting and exposed-face exchange. No guessed layer map or environmental data. |

Material and source data requirements are unchanged. See `material-sewing-plans.md`,
`material-platform-progress.md`, `steady-conduction.md`, `steady-thermal-coverage.md`,
`exterior-thermal-exchange.md`, and `textile-price-refresh.md` for their input semantics and limitations.
The UI deliberately asks for the existing JSON contracts rather than inventing convenient physical
constants. The synthetic sewing pattern/options under `tests/fixtures/materials/` illustrate process
inputs, not a real qualified textile or recommended machine configuration.

Sewing recipes may opt into `ordering` with `objective: "minimize_setup_changes"` and an explicit
`max_search_nodes` work limit. The existing Rust planner counts needle and thread changes separately
and minimizes their sum while honoring declared predecessors. An optional `initial_setup` names the
initial needle and thread; without it, the first installation is excluded. The workbench displays
baseline and selected counts, the selected operation sequence, and whether optimality was established
or the search stopped at its budget with a complete feasible schedule. The downloaded stitch program
retains the same result and source identities. This count does not infer handling time, undeclared
assembly access, machine calibration or permission to execute hardware. Editing the recipe withdraws
the old program and optimization result together.

An operation may also select `source_range` on its original side-A seam. The planner derives side B
through the existing notch/offset correspondence and checks complete coverage of both source sides.
This allows separate constant-layer recipes for a plain section and a folded or reinforced section
of one join. The workbench displays both source ranges, full original lengths and selected traveled
lengths even without an ordering policy. Reverse feed, backtacks, qualification and clearance keep
their ordinary checks. See the runnable split-seam example in
[material-sewing-plans.md](material-sewing-plans.md#partial-seam-operations-and-local-construction-changes).

Purchasing also accepts `ftx-textile-quotes/2` histories with exact UTC `observed_at` instants and an
explicit market `as_of_at`. Retain the matching `observed_on` and `as_of` dates. The result displays
the Rust evaluator's latest eligible and excluded observations for each roll, their exact times,
source expiry, rounded order quantities and merchandise costs. The source detail retains the
selected order and stock evidence. An unavailable, expired or insufficient latest observation
cannot revive an older offer. Version-1 books remain date-only and display that limitation;
the browser never supplies a clock value, chooses a different offer or recomputes a price.
Changing the quote book or market withdraws the old result. Input JSON and downloaded reports
retain their original numeric text. See [source refresh](textile-price-refresh.md) for creating
version-2 snapshots with `--intraday` and the supported timestamp precision.

Optional material `optical_faces` assignments survive inspection, freezing, piece binding and the
retained drape receipt. In the existing heat-condition JSON, set
`radiation.automatic_material_face: true` and omit explicit face/property strings. Independent
sheets use their canonical positive outside face; layered coverage selects the actual outer side
from cloth/body normals. The result retains the chosen observation and material identity.
No face editor or missing optical data is supplied by the page. See
[material optical faces](material-optical-faces.md) for the source, specimen, spectral and
temperature qualifications and a runnable synthetic stack. The same stack request is available
through the exported `thermal_steady` WASM API; it is not an additional workbench upload mode.

## Potential evaporative transfer

Section 1 accepts a standalone `ftx-vapor-stack/1` request through **Calculate potential vapor
transfer**. It needs no bound pattern or mechanical/heat-storage capability. The request owns its
ordered material selections, common area, boundary vapor pressures, positive film resistances,
latent-heat conversion and source-admission policy. The worker calls the same `thermal_vapor`
WASM export as the native consumer; JavaScript performs no transport calculation.

The result displays total resistance, potential latent heat/water flux and flow, film pressure
drops, every layer's face pressures and its original measurement method/conditions/source.
Source conditions are retained observations, not an inferred operating domain. Mixed protocols
and estimated observations require the existing explicit permissions. Potential transfer does not
determine available sweat, actual wearer cooling, condensation, comfort or a dry/wet heat history.

Downloads preserve the exact Rust `vapor-report.json`, original `vapor-request.json` and
`vapor-replay-request.json` with complete frozen material selections. Running this calculation
preserves the independently inspected material; it creates no replacement material selection.
Replacing or refusing a request withdraws its previous results, and delayed reads or stale worker
replies cannot publish them. See [vapor-resistance.md](vapor-resistance.md) for the schema, equations
and a runnable measured-leather example with explicitly illustrative boundary assumptions.

## Identify material behavior from local observations

Section 1 accepts `ftx-nonlinear-swatch-calibration/1` observations for nonlinear stretch and shear,
`ftx-relaxation-calibration/1` complete loading histories for relaxation, creep and recovery, or
`ftx-membrane-plasticity-calibration/1` cycles for permanent stretch and directional hardening.
Choose the matching response, load the source-bearing JSON and run **Identify material behavior**.
Timed requests preserve their independent long-time equilibrium. Directional recovery accepts either
fixed `orthotropic.times_seconds` or one to three nonoverlapping `orthotropic.time_bounds_seconds`
windows for identifying the times. Omit or leave empty the other field; supplying both is refused.
Proportional ratio/time search uses its separate top-level time bounds. The shared Rust
calibrators own the models, fitting, evidence admission, identifiability and residual gates; the page
neither fits coefficients itself nor rewrites the supplied measurements.

For permanent stretch, supply complete pristine-start loading/unloading/reversal histories in raw
Green strain and second-Piola sheet resultants. Missing force channels remain null, while every
recorded strain node still advances the existing permanent-state law. The material supplies matched
elastic coefficients and source-bearing total/accumulated strain limits; explicit search ranges
bound identification of all three yields and all three hardening values. Those ranges do not define
confidence intervals or replace the independently supplied applicability limits.

The result displays the selected FIT parameters, original search bounds, independent partition
errors, held-out status, actual work and excitation diagnostics. Accepted, ambiguous, rejected and
budget-exhausted outcomes remain distinct. A failed CHECK retains the same FIT choice rather than
promoting another candidate. Nonaccepted outcomes clear the previous selection and offer only the
complete report and original request for download; accepted results additionally publish the normal
source-locked selection for existing binding and drape consumers. Complete histories, permanent and
accumulated strain, stored energy, dissipation and force residuals remain inspectable. See
[material-membrane-plasticity.md](material-membrane-plasticity.md) and its deliberately synthetic
calibration fixture; no source identity or observed force is inferred from a fiber or brand label.

The **Admit the fitted estimates** checkbox is off initially. It only sets the returned selection's
estimate policy. Estimated input observations, estimated equilibrium and replacement of an existing
response require their separate explicit declarations in the uploaded request. A successful fit can
therefore remain unavailable to simulation; unrelated missing mass, bending and contact evidence
also remain visible through the ordinary material capability report.

Results separate fit and held-out component counts, scaled RMS/peak errors, their requested budgets,
and the applicable Green-strain domain. An absent held-out partition says **not supplied**. Nonlinear
domain endpoints come from fitting observations; timed applicability is source-declared. Complete
observations, predictions, residuals and fitted recovery branches remain inspectable. These quantities
are not confidence intervals or independent garment validation.

For directional recovery, the table reports each time and its published warp, weft, normal
coupling and shear tensor entries in N/m. Fixed candidate times remain supplied inputs, not
automatically optimized. In this fixed mode, inactive candidates remain visible with their explicit
zero coefficients; an entirely inactive spectrum retains the independently supplied equilibrium response.
Searched times each require a retained observable recovery branch; individual directional coefficients
may still be zero. When time windows are
declared instead, the page shows the identified time beside both bounds, its scaled force/log-time
sensitivity with fitted tensors held fixed, and its distance to the closest window boundary as a
fraction of the log-time window.
A fraction of zero is an endpoint and 0.5 is the center. The local optimizer table reports converged
and attempted starts, iterations in the selected solve, total objective evaluations, projected tensor
iterations across all solves, maximum scaled gradient and joint stiffness/time excitation pivot.
The gradient describes the objective slope after fitting the tensors; the pivot describes the
column-normalized FIT information for tensor coefficients and log-times. These diagnostics accompany the held-out errors;
they do not establish a global optimum, a unique spectrum or statistical confidence.

The full report separately preserves
unconstrained, physically constrained and explicitly thresholded values. Fit and held-out errors refer
to the final published law, including threshold decisions. Small errors do not establish a unique
spectrum or extend its source-qualified strain applicability.

`tests/fixtures/materials/synthetic-orthotropic-relaxation-search.json` illustrates bounded time
identification with independent directional responses. It is an artificial software fixture, not a
measured textile characterization.

The generated selection is ready for the same default or source-piece assignment and binding steps.
Its exact Rust `selection_json` is retained through assignment and download, preserving numeric source
identities. Download `material-calibration.json`, `calibration-input.json` and `calibrated-selection.json`
for replay. Changing the response, uploaded observations or fitted-estimate consent withdraws the
current characterization and its assignable selection; an explicitly assigned frozen material remains
an independent pending choice. Loading a pattern preserves completed characterization downloads.
Cancellation, newer selection edits and failed replacement fits cannot republish a previous result.

## State, privacy and failure behavior

The original pattern is retained separately from pending assignments and the bound result. Assigning
another material immediately withdraws the bound output and all dependent drape/heat/process/cost
outputs. A change to body or measurements withdraws drape and heat; changing only thermal conditions
withdraws that heat result but preserves mechanics. Changing stock options withdraws cutting and cost;
quote/market changes withdraw cost only. Results are explicitly current-input results, not an unlabeled
comparison with an old run. Previously downloaded files remain the user's own saved versions.

Each worker request includes the necessary current inputs. Epoch and request identities discard late
responses, and revision checks discard stale asynchronous file reads. Cancel terminates the worker,
including synchronous Rust calculations, and starts a new one. Failed replacements do not republish a
previous success. Temporary WASM body/fold/export objects are freed on both successful and failed
operations; a WASM trap requires a fresh engine. Withdrawn download URLs are revoked. Inputs remain
in this tab: no localStorage, sessionStorage, indexedDB, server upload, device connection or background
refresh. The existing fold viewer uses a Three.js CDN, explicitly disclosed on the page; its sandbox
cannot access the parent workbench. Closing the tab drops this unsaved session.

Combined JSON requests are limited to 64 MiB and each body-model input to 256 MiB, followed by the
existing native geometry, candidate, solver, stitch and allocation limits. Ordinary incomplete data
produces the specific native refusal rather than a generic success. A numerical result is not empirical
textile validation, a real-person fit claim, a warmth/safety rating or a hardware approval.

## Checks and delivery scope

`cargo test -p ftx-wasm --test materials --test manufacturing` exercises the actual shared Rust
binding, downstream material resolver, cutting, SVG, sewing and procurement APIs. Seven new regressions
cover source freezing, geometry and explicit-piece preservation, width-driven stock changes,
thickness-driven sewing refusal, separate partial-record capabilities, missing pair evidence and
combined input budgets and lossless floating-point JSON identity. Existing manufacturing tests remain independent.

`node --experimental-vm-modules --test tools/check_trace_frontend.cjs` includes ten new tests of the
actual material page and worker with **explicit DOM/WASM doubles**. They test lifecycle and input
routing, not physical results or browser execution: stale worker messages/file reads, catalog identity
changes, cancel, dependent-output withdrawal, native-handle cleanup, initialization/trap failures,
blocked exports and page-exit download cleanup.

`node --experimental-vm-modules tools/check_materials_wasm.cjs --native target/debug/ftx-materials`
executes the **actual compiled browser WASM and actual worker** under Node, without a mocked numerical
engine. It compares binding (including negative-zero coordinates), stock, SVG, sewing, procurement,
and source-locked thermal surface/coverage results against the native executable. This is runtime
WASM/native agreement on those cases, not browser navigation, fitted-body or empirical validation.

`tools/check_materials_web.cjs` is the separate real Chromium/WASM lane:

```sh
sh tools/build_web.sh
cargo build -p ftx-cli
NODE_PATH=$(npm root -g) node tools/check_materials_web.cjs --native target/debug/ftx-materials
```

It needs the Playwright Node package and Chromium (or `CHROMIUM_EXECUTABLE` pointing to an installed
Chromium). It compares browser binding, cutting, preview, sewing and procurement with the real native
binary and tests changed-material refusal/output invalidation. Source-locked synthetic thermal reports
exercise the actual worker/WASM surface and coverage APIs against native results. They are explicitly
not freshly fitted body drapes. The existing `tools/check_web.cjs --model PATH.ftxbody` lane remains
necessary for successful model-backed browser draping; no model asset or private wearer record is
bundled by this feature.

This delivers the local operator integration slice of plan §25.9. It does not close the full empirical
linen/leather/knit garment gate, acquire new measured records, qualify live supplier coverage, or provide
interactive body/layer-map editing, a simultaneous scenario comparison, transient UI controls or
robot execution. Those remain distinct follow-on capabilities, not inferred from a working page.
