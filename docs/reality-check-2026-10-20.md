# FrankenTextiles reality check — 2026-10-20

Assessed source: `main` at `6410556378514874274e77c304efd0d4e2200a73`.
This initial assessment and delivery plan is not release certification. It preceded product changes;
the implementation follow-through at the end records subsequent work and current verification.
AGENTS.md and README.md were read in full before investigation. Oracle fixtures remain unchanged.

## Verdict

FrankenTextiles has a substantial implementation of the original garment engine and a much larger
material/thermal/manufacturing platform. It is beyond a skeleton or collection of placeholder APIs.
The strongest evidence is the spec-driven pattern/export port and its captured-reference corpus.
The weakest link is now establishing that accepted numerical results support the stronger physical
and user-facing conclusions, using production assets and matched independent measurements.

It is **not finished against the complete current plan**. In particular:

1. The production memoryless drape path can accept unconverged Newton steps; its published `settled`
   flag remains the reference's window-drift measure. This differs materially from the plan's
   residual/velocity and fail-closed promises.
2. The essential Anny and fitted-garment tests need an external model and explicit execution.
   Ordinary debug workspace tests omit those tests and three release-only contact tests.
3. Matched linen/leather/performance-knit characterization, measured garment comparisons,
   qualified thermal/process experiments and real-photo body-fitting acceptance remain incomplete.
4. Finishing all currently active Beads would still leave gaps: some promised work is absent from
   the active issue file, some original integration issues were closed while acceptance remains
   partial, and performance/reproducible-release work lacks concrete acceptance tasks.
5. The source has grown to **187,214 Rust lines**, including tests/examples, against the still-written
   **120,000-line ceiling**. The expansion needs an explicit scope/budget decision, not an unrequested
   mass refactor during this review.

Parity, physics, fit and performance have separate conclusions. No overall completion percentage
would convey these distinctions honestly.

## Evidence scope and limits

The assessment compares README/AGENTS promises, the comprehensive plan, FEATURE_PARITY, specification
boundaries, crate contracts, active and staged issue records, source control flow, capture scripts,
fixtures, integration tests and browser checks. Source inspection concentrated on actual dispatch,
solver acceptance/publication, measurement/model boundaries and shared material consumers. This is
not a claim that every line of the 187,214-line tree or every subsystem note received a bug audit.

Historical results in README, FEATURE_PARITY and Bead comments are identified as historical; they
were not silently promoted to a current-head pass. No internet source, remote release service or
private wearer data was consulted. Local Git has no tags; that does not establish the absence of
remote releases. No hosted product or installed `ftx` was verified.

### Checks actually run

| Command / inspection | Result and meaning |
|---|---|
| `cargo fmt --check` | PASS, exit 0. Formatting only. |
| `python3.13 -m unittest discover -s scripts -p 'test_*.py'` | PASS: 84 tests, Python 3.13.9. Offline supplier/HFM/specification behavior. |
| `python3 -m unittest discover -s scripts -p 'test_*.py'` | FAIL: 84 tests, one failure, Python 3.15.0. Invalid source timestamp `2026-10-13T24:00:00Z` is accepted. |
| `python3.14 -m unittest discover -s scripts -p 'test_*.py'` | Same failure, Python 3.14.6. |
| `PYTHONPATH=scripts python3.12 -m unittest test_textile_jsonld.JsonLdTests.test_intraday_source_time_refuses_unknown_offsets_and_precision_loss` | PASS on Python 3.12.12; the same focused test passes on 3.13.9. Establishes runtime dependence. |
| `node --experimental-vm-modules --test tools/check_trace_frontend.cjs tools/check_materials_frontend.cjs` | PASS: 104 test executions, 2 skips, Node 26.8.1. The tracing suite imports the materials suite, so this command runs its tests twice. These are DOM/WASM-double lifecycle/routing checks; the compiled-WASM case skips without its inputs. Do not count these as 104 distinct numerical proofs. |
| `ast-grep run --lang Rust --pattern 'fn $NAME($$$) { todo!() }' crates` | No matches, exit 1. Keyword scan also found no production `todo!()` or `unimplemented!()` bodies. Manual dispatch/control-flow inspection corroborates substantial implementations. This is not proof of completeness. |
| `sbh check --need 20G` | FAIL, exit 1: about 306 GB container space available, but 7.66% free is below SBH's active threshold. No local heavy build bypass was used. |
| `RCH_REQUIRE_REMOTE=1 RCH_VISIBILITY=verbose rch exec -- env CARGO_TARGET_DIR="${RCH_TARGET_BASE:-${TMPDIR:-/tmp}}/rch_target_ftx" cargo test --workspace --locked` | Refused, exit 103: no admissible worker; pressure, slots and OS exclusions. No Rust tests executed. |
| `RCH_REQUIRE_REMOTE=1 RCH_VISIBILITY=verbose rch exec -- cargo test --workspace --locked -j 1` | Worker `vmi1153651` selected, then source initialization refused: unfinished overlapping physical source owner. Retries exhausted; exit 103 before compilation. |
| `bv --robot-triage`, `bv --robot-insights`, `bv --robot-plan` | PASS on the active JSONL: 27 nonclosed, 18 actionable, 9 blocked; no cycles reported. Highest-impact item is `ftx-tm2`. |
| `br stats --json`, `br --no-auto-import stats --json`, `br --no-db list --status=open --limit 0 --json` | FAIL: duplicate import comment ID 1. Neither alternate read mode bypasses validation. |
| `br doctor --json` | Reports missing active database/recoverable workspace. The portable source collision prevents ordinary bootstrap. |
| `br create --title 'Restore lossless Beads import for the FrankenTextiles backlog' --type bug --priority 1 --description ... --json` | FAIL, exit 4 on the same duplicate comment. **No Bead created.** |
| `br sync --flush-only` | FAIL, exit 4 on the same duplicate comment. No successful backlog flush is claimed. |

`cargo clippy --all-targets -- -D warnings`, a completed `cargo test`, the wasm build, the ignored
model/physics lanes and Chromium numerical end-to-end checks did **not** run successfully here.
DSR quality was not run: no matching local repo configuration was found, and even DSR help/startup
did not finish normally during the bounded investigation. No GitHub Actions workflow was dispatched.

There is no `target/`, `web/pkg/`, installed `ftx`, or default converted Anny model in this checkout's
examined locations. These are setup/reproduction limitations, not evidence that the corresponding
implementations are fake. The advertised native draft/round/drape and browser numerical flows could
not be executed in this session. The Python source-adapter flows and frontend state logic did execute.

## Vision checklist

Status describes the capability, with verification qualified separately. `UNPROVEN` does not mean
missing source: here it often means implemented with historical evidence, but no fresh execution.
`NO_ACTIVE_BEAD` means missing actionable coverage, even where a broad parent mentions the theme.

| # | Testable goal and source | Reality / status | Concrete evidence and remaining coverage |
|---|---|---|---|
| 1 | Deterministic core, units, geometry and structured refusals (§6–7) | Implemented; current numerical execution UNPROVEN | `ftx-core/src/{det,curve,exact,linalg,refusal}.rs`; provenance, direct tests, `ftx-conformance/tests/no_platform_libm.rs`. |
| 2 | Read/write `fabric-pattern/1`, mirror/fold instances and structure checks (§8; spec 01) | Implemented; current execution UNPROVEN | `ftx-pattern/src/{doc,instance}.rs`, pattern oracle fixtures and model/geometry conformance tests. |
| 3 | Seam walk, truing, points of measure and fit-spec verdict (§8) | Implemented; current execution UNPROVEN | `ftx-pattern/src/validate.rs`, captured validation/default-fit cases. |
| 4 | Five alteration operations with reference behavior and safe refusals (§8) | Implemented; current execution UNPROVEN | `alter.rs`, oracle alterations, direct safety and real-CLI workflow tests. |
| 5 | Measured bolero, trousers and fez drafts (§8; spec 02 C) | Implemented for those three programs | `draft/`, captured programs; `draft_workflow.rs` actually invokes draft → validate → export. Not a general garment-idea interpreter inside Rust; agent judgment stays outside. |
| 6 | Catalogue, priors, fusion, provenance and readiness (§10.1) | Implemented; current execution UNPROVEN | `ftx-measure`, 35-item catalogue and fusion/readiness oracle cases. `MODEL_FIT` and reference readiness remain separate from measured evidence. |
| 7 | Pinned Anny conversion and forward parity, G1a (§9) | Implemented; historical gate, current UNPROVEN | Converter, pin, 16-case reference and 10-pose corpus; `anny_parity.rs`/`drape_poses.rs` ignored without external asset. |
| 8 | Tape functionals, fitting and honest unmeasured values, G1b (§9.3–4) | PARTIAL | 21 functionals, LM and residuals; `fit_tape.rs` and `body_model_fit.rs`. FEATURE_PARITY still marks fitting/tape/placement partial; anatomical heuristics/population priors and independent qualification remain. |
| 9 | Poses, nappy calibration and checked collider (§9.5) | Implemented with explicit limits | Posed skin checks and nappy bisection exist. Contract notes seated/squat volume loss and absence of pose self-intersection qualification. |
| 10 | Photo scale from Letter/A4 or ChArUco (§10.1) | Implemented numerically; empirical acceptance UNPROVEN | Pure-Rust detector, oriented JPEG/PNG paths, camera/lens and CLI tests. Recorded detector evaluation uses synthetic photographs, not real-photo acceptance. |
| 11 | Print calibration references with specified variants (§10.1; spec 02 A.9) | PARTIAL | Native Letter/A4 DICT_4X4_50 PNG/PDF generation exists. Other dictionaries and browser calibration controls remain absent; the parity map explicitly says so. |
| 12 | Moving-child frame identity, cameras, still windows and triangulation (§10.2) | PARTIAL | `video`, `still`, `camera`, `triangulate`, `bundle`; decoded image/mask manifests and separate commands. External decoding/segmentation is intentional, but complete capture-to-result orchestration and uncertainty acceptance are not established. |
| 13 | Multi-frame Anny fit from 2D and silhouettes (§9.4; §10.2) | PARTIAL | Real fitter, held-out checks, multistart/observability and accepted-body reuse. CLI tests use a tiny synthetic Anny-format model; production-asset/real-photo acceptance remains unverified. Closed FTX-21 is not proof of that acceptance. |
| 14 | Mesh, welded seams, grain, gathers and elastic (§11.4) | Implemented with strong historical parity | `garment`, `scene`, `start`; captured mesh/start states; 42,003-triangle correspondence is documented. Native cloth mechanics intentionally replaces Blender response. |
| 15 | Orthotropic membrane, bending, IPC, CCD and friction (§11) | Implemented; current physics execution UNPROVEN | Real solver/contact/sparse code and KATs. Some decisive KATs are skipped by ordinary debug tests. |
| 16 | Numerical stall refuses; settled means residual and velocity (§6; §11.1–3) | PARTIAL / acceptance defect | `cloth::step_inner`, `scene::run_phase`, `run::drape`, `metrics`; detailed finding A below. NO_ACTIVE_BEAD for the concrete repair. |
| 17 | Start sensitivity quantified on every drape (§11.1, KAT10) | PARTIAL | `--two-start-sew` exists and is tested, but is opt-in. Ordinary runs have no two-start error bar. Document that scope or implement the promised policy. |
| 18 | Contact-correct cloth layers (§11.4) | PARTIAL by invocation | Self-contact exists but `DrapeOptions::default().self_contact` is `None`. Skill/browser can enable 2 mm; the short native command does not automatically do so. |
| 19 | Metrics, PC2, mesh map, viewer and review (§11.6–8) | Implemented; reporting gaps remain | Reference metrics/flags and native rendering exist. Solver convergence telemetry is not the report's acceptance authority; viewer imports Three.js from a CDN. |
| 20 | Every export and 51 acceptance-check kinds (§12; spec 03) | Implemented; current execution UNPROVEN | Real SVG/PDF/DXF/HPGL/cutter/GarmentCode/DST writers and mutation/differential tests. File-format checks are not process or hardware qualification. |
| 21 | Trace, annotate, audit and scale-fit (§13) | PARTIAL parity | Extensive real raster/vector implementations and fixtures. `ftx-q3m` retains platform-libm/decoder-tail decisions; native review-image pixel exceptions are deliberate. |
| 22 | Immutable round with separate measurement/flat/sim/export bases (§14) | Implemented; inherits numerical acceptance gap | `ftx-cli/src/round.rs`, `ftx-pattern/src/round.rs`, real-CLI tests. Round status checks simulation `gating`, not independent Newton convergence. |
| 23 | Local browser draft/validate/export and small-garment drape, G4 (§15) | Implemented; historical G4, current numerical UNPROVEN | Real shared Rust APIs and browser workers. Lifecycle tests pass now; actual compiled-WASM/Chromium drapes were not rerun. |
| 24 | Whole-outfit review (§14) | Implemented visualization; broader claim overstated | `outfit.rs` consumes completed `--sim` directories and freezes garments. It does not jointly or sequentially simulate layers. `docs/outfit.md` states this; CLI CONTRACT still describes a different `--layer` flow. |
| 25 | One source-bearing material authority feeds relevant consumers (§25.1–3) | Substantial implementation; empirical PARTIAL | `material_catalog`, `material_binding`, `ftx-drape/src/material`, shared workbench. Capability-specific missing-data refusals and material/interface identity are real. |
| 26 | Calibrated nonlinear, directional, time-dependent and permanent response (§25.3) | PARTIAL | Constitutive/calibration/history paths exist, including bounded model comparisons and plastic heat. Matched measured validation, damage/rupture and qualified combined laws remain `ftx-tm4`, `ftx-rc1/2`, `ftx-ps1`. |
| 27 | Matched linen/leather/knit fixed-jacket substitution (§25.1, 25.5) | PARTIAL; principal empirical blocker | Synthetic propagation tests exist. `ftx-tm2`, `ftx-md1`, `ftx-tm8` and related acquisition gates remain open; catalogue breadth does not provide a fully characterized matched trio. |
| 28 | Layer coverage, motion, heat, compression and moisture (§25.4) | PARTIAL | Conservative dry steady/transient/coverage/exterior/motion paths are real. Measured local compression, coupled moisture/evaporation, ventilation and garment validation remain open. No wearer comfort claim follows. |
| 29 | Comparable current offers and procurement (§25.4) | PARTIAL, with reproduced runtime defect | Offline import/history/spec-review/procurement exists. Live source/stock qualification remains `ftx-tm6`, `ftx-pq1`, `ftx-sr2`; invalid-clock rejection breaks on newer Python. |
| 30 | Material-aware nesting, sewing and machine planning (§25.4) | PARTIAL | Real bounded nesting/search, assembly dependencies/setup optimization and stitch programs. Measured recipes/quality/accessibility and hardware remain `ftx-tm7`, `ftx-sp2`; hardware execution is deliberately unauthorized. |
| 31 | Native/debug/release/wasm determinism and budgeted performance (§11.7; §17.5) | Historical slices; full current acceptance UNPROVEN | Digests/check scripts exist. No Cargo benchmark directory was found; README itself says timed, not benchmarked. Material/history extensions require execution, not just wasm compilation. |
| 32 | G5, usable release and reference retirement (§17.6; §19) | NOT COMPLETE | `ftx-onn` open; no completed gauntlet/certification demonstrated. No release installation or hosted-browser flow was verified. |
| 33 | Real-person fit (§3) | UNPROVEN by this repository assessment | Requires private muslin/dressed-photo evidence. Synthetic bodies, computed flags and passing source tests cannot establish it; no personal data should be added to the repo. |

## Findings that change the delivery decision

### A. Physical convergence is not the published verdict authority — P0

The source path is explicit:

- `cloth.rs::step_inner` accepts the `SolveReport` returned by minimization, audits finiteness/contact,
  updates velocity, and returns `Ok(rep)` even when `rep.converged` is false. History-bearing laws take
  a separate stricter path; the defect described here is the memoryless path.
- `scene.rs::run_phase` increments `unconverged` and continues, collecting subsequent frames.
- `run.rs::drape` prints phase counts/speed to its progress callback, then builds the saved report
  from the reference metrics. It does not gate that report on the phase's unconverged count.
- `metrics.rs` explicitly computes settledness from drift of window means, not speed. A periodic
  motion can have equal window means while continuing to move; a small drift is not a residual check.
- `ftx-cli/src/main.rs` chooses drape exit 0 solely from an empty `gating` list. The ordinary round's
  status policy similarly considers `sim.gating`. The unsettled metric is advisory.

This is a source-established publication/acceptance gap, not a claim that a newly executed bolero
failed: the Rust runtime was unavailable here. Plan §6 says numerical stalls refuse; §11 promises
residual/velocity settledness and solver telemetry. Preserve the reference metrics for parity, and
add an independent native numerical acceptance basis. Do not silently redefine the oracle metric.

### B. Important test names do not imply routine gate coverage — P1

There are 1,506 `#[test]` declarations and 27 ignore annotations active in debug source, including
three `cfg_attr(debug_assertions, ignore=...)` contact tests. Anny forward/hash/pose/fitting tests,
elastic ladder, fitted armpit wedge, two-start and fitted determinism require the external model.
The ordinary workflow invokes debug `cargo test --workspace --locked`; it cannot establish those
gates. The separate photo workflow includes optional OpenCV tests, but does not close the Anny lane.

The existing Anny reference comparison uses selected vertices, bounding boxes, aggregate checksums,
bone summaries and data hashes. That is useful parity evidence. A claimed maximum error over every
vertex would require a full upstream/candidate comparison, rather than treating aggregate sums as
a maximum-norm certificate. Any added reference data must come from the pinned upstream.

Use the existing tests and capture scripts in the supported DSR/RCH path. No additional gate
framework, validator hierarchy or newly dispatched GitHub Actions workflow is needed.

### C. The expanded platform is waiting on experiments, not another catalogue abstraction — P0

The common selection/resolution boundary is in production code. Directional nonlinear mechanics,
Maxwell recovery, permanent strain, pressure-conditioned snapshots, thermal coverage/motion,
finite stock and process studies are much more extensive than the original Phase 0 proposal.
The newest commits continue that trajectory. However, source observations of thickness/resistance
or fibre composition cannot supply missing directional mechanics, pair friction, matched storage
or seam recipes. Existing documents often make that distinction correctly.

The highest-value next acceptance is a matched-specimen chain: conditioned coupon measurements →
compiled material → held-out coupon → unchanged jacket/body substitution → measured sewn behavior.
Use the existing `ftx-tm2/tm8/tme/tmf/md1/sp2` work instead of starting another simulator or database.
Thermal and manufacturing completion remain independently reportable if a mechanical specimen is
incomplete. Synthetic source-qualified records establish propagation, not experimental agreement.

### D. Photo/video implementation is useful engineering software, not a finished capture product — P1

`video-select` accepts decoded PNGs and reviewed masks/metadata; `fit-body` accepts explicit model
correspondences, calibrated cameras, bounded variables and independent initializations. Segmentation,
detector correspondence decisions and video decoding are external by design. These boundaries do
not justify calling the engine absent. They do mean a parent cannot simply give the current Rust
CLI an arbitrary phone clip and obtain a qualified body without preparing evidence.

The current body-fit documentation explicitly leaves production-asset qualification and a
ruler-in-frame real-photo check unverified. The original Phase 3 exit requires that real-photo check.
Closed `ftx-sdk`, `ftx-994` and `ftx-uql` do not supply a dedicated remaining acceptance task.
Do not reopen all their delivered software; track the missing product/empirical slice explicitly.

### E. Supplier timestamp refusal depends on Python version — P1

`scripts/refresh_textile_prices.py::valid_period` validates the spelling with a regex but lets
`datetime.fromisoformat` validate the clock. On Python 3.14.6 and 3.15.0,
`2026-10-13T24:00:00Z` normalizes to the next day's midnight. The existing regression expects refusal.
All 84 tests pass on 3.13.9; 83 pass and that test fails on both newer interpreters. The focused
test also passes on 3.12.12. This is an actual portability defect in source validity admission,
not a reason to regenerate a fixture or suppress the failing case. Validate source clock ranges
explicitly and retain the existing invalid-offset/precision and inclusive-expiry behavior.

### F. Scope, documentation and dependency rules have drifted — P1/P2

| Crate | Rust lines incl. tests/examples | Planned allocation |
|---|---:|---:|
| ftx-core | 5,154 | 8,000 |
| ftx-pattern | 28,696 | 16,000 |
| ftx-body | 5,407 | 9,000 |
| ftx-measure | 17,373 | 16,000 |
| ftx-drape | 45,827 | 14,000 |
| ftx-export | 24,022 | 14,000 |
| ftx-trace | 17,976 | 7,000 |
| ftx-cli | 29,780 | 4,000 |
| ftx-wasm | 4,313 | 3,000 |
| ftx-conformance | 8,666 | 9,000 |
| **Total** | **187,214** | **120,000 ceiling including contingency** |

The ceiling is exceeded by 67,214 lines, or 56.0%. This count includes substantial direct tests;
it is a scope/maintenance constraint discrepancy, not a claim that the extra code is worthless.
Plan §25 deliberately expands §1.4's original garment-only product, while AGENTS still describes
the original boundary. The plan wins, but the unchanged ceiling and current priority need resolving.
`ftx-trace/Cargo.toml` also depends on `ftx-export`, outside the documented core/pattern-only direction.
Inspect its concrete preview/text use before deciding whether a small shared-kernel move is justified.

Other concrete drift: README's phase badge says Phase 1 despite claimed G1–G4 passage; its fitting
description still says fifteen measurements while the implementation has 21; CLI CONTRACT's outfit
command describes sequential draping, whereas `docs/outfit.md` and source correctly describe frozen
composition. Spec 06's initial thermal boundary predates implemented transient/coverage work. The
fold viewer includes remote Three.js imports despite being described as self-contained. Local
computation is not the same claim as an offline-complete HTML artifact.

Correct these specific facts in existing truth sources after the capability decisions; do not create
another competing architecture plan or refactor the entire workspace to satisfy a line counter.

## Backlog reality and the “finish every open Bead” test

Read-only parsing of the active `.beads/issues.jsonl` gives **74 issues: 47 closed, 26 open and one
in progress**. These are parsed portable-source counts, not a successful `br stats` result.
`bv` can read that source and reports no cycles, but a clean graph cannot diagnose absent tasks.

Two distinct comments on `ftx-enu` both have `id: 1`, the same author/time and slightly different
wording. Current `br` rejects the import before reading or creating issues. The exact portable
source was preserved; neither comment was discarded or manually renumbered. Repeated attempts
left ignored recovery artifacts; no destructive recovery was attempted.

SHA-256 of the inspected backlog sources:

- `issues.jsonl`: `98cc1f05a150449051338c17a17beecba0af8d25e574b46b82262cccf020793c`
- `material-study-additions.jsonl`: `e9b365c30ce13c9e4fcaf474810385297aba246d8dd96b7a521fe3824f3af756`

The separate additions file contains `ftx-ms1` (closed), `ftx-ms2` (open P0) and `ftx-ms3` (open P1).
None is in the active issue file. The plan also refers to `ftx-cv1`, `ftx-cv2`, `ftx-cv3` and
`ftx-cv4`, which are absent from both active issue rows and the three-row additions file. Some of
their software is now implemented, so these references must be reconciled against actual delivered
scope rather than blindly recreated as new implementation work.

| Goal / gap | Existing actionable coverage | Would completing the present active backlog close it? |
|---|---|---|
| Matched material mechanics and fixed-jacket acceptance | `ftx-tm2`, `ftx-md1`, `ftx-tm8`, targeted source issues | Yes only if their empirical acceptance is actually met, rather than closed on synthetic tests. |
| Qualified thermal data, compression/moisture and garment exchange | `ftx-tma/b/c/e/f`, `ftx-cp2`, `ftx-ex2/3`, `ftx-op1`, `ftx-np1` | Substantial coverage; full-garment acceptance is more concrete in absent `ftx-cv2` and should be made active. |
| Live supplier and process qualification | `ftx-tm6/7`, `ftx-sr2`, `ftx-pq1`, `ftx-sp2` | Broadly covered; clock portability needs a concrete defect/test. |
| Recovery/plasticity/damage empirical work | `ftx-tm4`, `ftx-rc1/2`, `ftx-ps1` | Broadly covered, with several software portions already delivered. Do not reimplement them. |
| Scenario study and raw M1 source-unit follow-up | `ftx-ms2/3` only in the separate additions file | No active scheduling coverage; parent descriptions are not these actionable tasks. |
| Trace compatibility policy | `ftx-q3m` in progress | Covered, but its old description/comments need a final residual-scope decision. |
| Numerical completion/physical settledness and saved report basis | No dedicated open task; `ftx-onn` only names the gauntlet | No concrete repair/acceptance coverage. |
| Production-Anny multi-frame and real-photo exit acceptance | Original FTX-19/20/21 tasks are closed; parity map remains partial | No dedicated open acceptance task. |
| Repeated benchmarks, complete determinism lanes, fresh-install/user smoke | Broad `ftx-onn` only | Not sufficiently specified to ensure these deliverables. |
| Current scope/budget/direction and command-document reconciliation | Truth-sync `ftx-eoi` closed | No concrete active follow-up. |
| Private real-wearer muslin comparison | No dedicated acceptance task found | No. Do not infer it from model fitting or add personal evidence to Git. |

Thus the answer is **no**: the current open/in-progress list does not guarantee the complete vision.
The material work is largely tracked; numerical acceptance, realistic input acceptance and current
release proof need specific coverage. This does not call for a second large project-management system.

## Bridge plan and Bead-ready work

These are **proposed work identifiers, not created Bead IDs**. `br create` and final flush failed.
Use only `br` to register/revise issues once its lossless import is operational. Reuse existing
parents and source tasks. Each implementation item below has explicit companion verification work;
do not close either on a planning document or on unavailable infrastructure.

| Proposed work | Priority / size | Smallest implementation and completion condition | Companion verification / dependencies |
|---|---|---|---|
| R1 Restore lossless backlog operations | P1 / S, operational prerequisite | Preserve original source bytes, all 74 active rows, both comment texts and every relation; recover with supported `br` operations. Reconcile staged `ms1/2/3` without overwriting later edits. Match absent `cv*` references to delivered/remaining scope. | `br show`, `br doctor`, `br sync --status`, successful flush, and `bv` no cycles/dangling prerequisites. Recount/preserve all imported information. Do not skip the entire `ftx-enu` record. |
| R2 Make numerical completion an independent drape acceptance basis | P0 / M | In existing `cloth.rs`, `scene.rs`, `run.rs` and CLI/round consumers, stop or explicitly fail numerical acceptance on a stalled accepted solve; retain per-phase convergence/residual/speed evidence in the saved report. Implement or explicitly delimit production settle behavior. Preserve oracle metrics and identify the native verdict separately. | R2-test: force low Newton limits/stall and periodic equal-window-mean motion; no ordinary successful physical verdict or stale output. Cover direct CLI, round, shared WASM/scenario and history rollback. Existing oracle metrics must remain unchanged unless a named exception is deliberate. R1 only blocks recording, not implementation. |
| R3 Execute the existing G1/G2/G4 proof on current production assets | P1 / M execution; small script changes only if necessary | Obtain the pinned converted model outside Git, verify it, and use supported remote gates to execute forward/hash/pose/fitting, release contact KATs, fitted garment KATs, native debug/release and compiled-WASM/browser comparisons. Record failures and exclusions separately. | R3-test: prove the commanded tests actually ran; absent asset/browser/worker is unavailable, never green. Include default/sourced and representative history paths. Use existing scripts; no new gate framework. Depends on R2 for an honest final acceptance claim. |
| R4 Complete capture-to-fitted-body acceptance | P1 / M–L | Keep external decoder/model boundaries; provide a concrete reproducible local sequence from decoded timestamps, sheet calibration and reviewed correspondences/masks through `fit-body` to accepted body → round. Add production-asset and independent ruler-in-frame acceptance missing from closed FTX-19/20/21. | R4-test: full-scale Anny synthetic recovery, held-out observations, moving poses, focal/depth ambiguity and failed scale/still-window cases; then private independently measured photo/tape residuals. No tiny-model test may stand in for the production-asset result. Follow plan's 3 mm girth / 2 mm length synthetic gate where applicable; retain actual empirical residuals. |
| R5 Finish matched three-material and garment experiments | P0 / L, existing work | Execute `ftx-tm2`, `ftx-md1`, staged `ftx-ms3` and related acquisition tasks: exact lot, conditioned mass/thickness, warp/weft/shear/bending, pair friction, domains/replicates and compatible thermal/process observations. Use existing calibration/resolution; no generic fibre coefficients. | Companion acceptance is existing `ftx-tm8` plus `ftx-tmf`/`ftx-sp2`: independent held-out coupons first, unchanged jacket/body/material-only swaps next, measured drape/thermal/seam comparisons last. Compare failures, residuals and source bounds separately. R3 before final software parity claim; no false dependency that prevents independent acquisition now. |
| R6 Close qualified transport/process/purchasing slices | P1 / L, existing work | Continue existing `tm5/tmb/tmc/cp2/ex2/ex3/op1/np1`, `tm6/sr2/pq1`, `tm7/sp2`; restore missing full-garment coverage/study acceptance to active tracking. Complete local compression/moisture and physical face/recipe/source qualification only where observations support them. | Existing analytic conservation/remap/refusal tests plus held-out coupon/garment/process and permitted live-source acceptance. Distinguish capabilities, experiments and hardware authorization. Avoid a monolithic dependency that blocks useful steady transport on absent mechanics. |
| R7 Stabilize supplier source-clock validation | P1 / S | In `valid_period`, explicitly reject clock hours/minutes/seconds outside the schema before `fromisoformat`; preserve supported offsets, source precision and inclusive expiry. Existing test already demonstrates the defect. | R7-test: run the existing 84-test suite under supported Python versions, including 3.14/3.15 if supported; retain `24:00:00`, invalid offset minutes, leap-second and fractional-precision cases. Add focused cases only for newly exposed normalization holes. Relates to `ftx-tm6`; does not need new source acquisition. |
| R8 Align user-visible scope and numerical defaults | P1/P2 / S | Update existing README/FEATURE_PARITY/CONTRACT/spec statements: phase, 21 tapes, frozen `outfit --sim`, current thermal implementation, body-only versus self-contact, opt-in two-start, actual viewer network dependency. Make the explicit scope/budget decision for §25 and the 120k ceiling; resolve the trace→export rule against its actual uses. | R8-test: check documented commands against actual CLI tests/help and browser behavior; verify supported default/refusal examples. No command parser redesign, new crate, whole-tree refactor or automatic feature removal. R2/R3 determine justified gate wording. |
| R9 Produce bounded performance and delivery evidence | P1 / M | Extend existing examples/check scripts only as needed to repeatedly time fixed bolero/trousers and representative material/history/scenario cases. Retain machine, commit, inputs/model/material hashes, build options, workload, warm/cold policy and spread. Document executable/browser setup and asset conversion/discovery from a fresh checkout. | R9-test: compare §11.7 native <60 s / browser <300 s targets on the stated hardware; include full contact policy and no incomplete solves. Run real CLI/Chromium smoke and matched digests. Do not call an oversubscribed remote timing an M4 laptop benchmark. No new benchmark framework required. Depends on R2/R3, then feeds `ftx-onn`. |
| R10 Private muslin prediction check and G5 completion | P1 / empirical + existing gate | Compare one supported garment's predeclared predicted issues with a privately retained muslin/dressed result and independent measurements. Resolve discrepancies in geometry/body/material assumptions; report residuals without replacing tests with anecdotes. Complete `ftx-onn` within a declared release scope. | Local/private fit evidence never enters Git. G5 still requires its existing convergence/negative-ledger acceptance and two clean rounds; the assessment is not one of those passed rounds. Depends on the relevant R2–R5/R9 results; broader §25 research may remain a clearly labelled preview if release scope explicitly says so. |

### Dependency order and immediate priorities

The practical order is R2 numerical verdict correctness and R7's small existing regression, while
matched specimen acquisition R5 proceeds independently. Recover R1 so work is actually schedulable;
restore remote proof access and execute R3. R4/R5 supply realistic-input and empirical evidence;
R6 extends only qualified capabilities. R8 fixes exact claims; R9/R10 provide benchmark/release/fit
evidence. These are gates on claims, not a reason to withhold useful partial source-data consumers.

Do not select more optional constitutive features simply because the data/experiments are harder.
The current library already supports more material behavior than its matched empirical acceptance
can establish. Keep calibration work tied to acquired specimens and the fixed-pattern consumer path.

### Ambition and refinement outcome

Three challenge passes improved the plan: from missing-feature inventory to a complete user route;
from additional material breadth to matched, held-out experiments; and from persuasive pictures to
independent numerical, physical, performance and fit acceptance. No speculative mathematical subsystem
was added merely to make the plan appear ambitious.

Four refinement passes checked completeness, reuse, tests and dependencies. They retained the
external decoder/privacy boundaries, avoided recreating existing solvers/calibrators, separated
missing data from unavailable computation, added explicit stalled-solve and realistic-model tests,
and identified the active/staged backlog discrepancy. Final review found no reason to add another
implementation framework. `bv` validates the existing active graph only; the proposed additions could
not be inserted or graph-validated because `br` import is broken. No Bead-generation success is claimed.

## Handoff

Deliverable is this one report. Product sources, fixtures, plan, README and both backlog JSONLs were
left unchanged. No branches, worktrees, commits, releases or external messages were created. The
Agent Mail report reservation is released at closeout. An untracked
`.ee/hook-state/session_start_orient.last.json` also appeared during the session; it was preserved
rather than deleted.

The next high-value work is an honest numerical acceptance boundary and the matched three-material
experiment, backed by current production-asset execution. More module count or catalogue rows cannot
substitute for either result.

## Implementation follow-through — 2026-10-20

This checklist follows the user's request to implement the highest-value repairs and track the
remaining work. It is a session handoff, retired as an active checklist when its rows are resolved
or handed back to the named Beads. It is not another release gate or evidence of physical validity.
The original assessment above describes `6410556`, not the repaired tree.

- [x] R1: preserve the complete Beads directory before repair; retain all 74 original issues,
  114 dependencies and 65 comments, including both distinct `ftx-enu` comments formerly numbered 1.
- [x] R1: assign the second duplicate comment the unused ID 65; recover supported `br` operations.
- [x] R1: reconcile the three staged material-study issues additively, preserving five dependencies;
  verify canonical sync and successful flush. `br doctor` reports healthy data with warnings about
  its stale merge anchor and missing optional WAL-index ignore pattern; it is not an all-green result.
- [x] Register and complete `ftx-6en` (P0) and `ftx-dzf` (P1), with existing parent gates retained.
- [x] R7 / `ftx-dzf`: explicitly reject source-clock overflow before Python datetime normalization;
  retain valid offsets, expiry, source precision and source data. Implemented in `b23c281`.
- [x] R7: fresh solo rerun of `python3.12`, `python3.13`, `python3.14`, and `python3.15`, each with
  `-m unittest discover -s scripts`: **84 passed on each version**. Parent `ftx-tm6` stays open.
- [x] R2 / `ftx-6en`: require every inner Newton solve, including friction rounds, to converge;
  return a structured refusal with residual/correction diagnostics otherwise (`9cef987`).
- [x] R2: preserve accepted geometry, velocities, contact lag and caches on step/static-solve failure;
  preserve the phase's contact-offset ramp on failure too (`ed0fdee`).
- [x] R2: retain per-step Newton reports and actual maximum speeds in phase reports and saved
  `numerical` output; gate gravity drapes lacking three consecutive steps at at most 1 mm/s.
- [x] R2: preserve reference metric values/texts; mark sewing-only gravity settling unavailable;
  validate the existing adaptive `Solver::settle` limits rather than admitting an empty quiet window.
- [x] R2: direct exhaustion/rollback/replay, line-search failure, valid quiet window, oscillatory
  cancellation and qualification tests: **113 drape library tests passed remotely** on `vmi1227854`.
- [x] R2: actual material-to-drape diagnostic/gating and failed-phase frame/ramp rollback:
  **10 material integration tests passed remotely**. Direct CLI JSON and human output expose numerical status.
- [x] R2/R3: strict remote all-target clippy with `-D warnings`, and `ftx-wasm` compilation passed.
  Compiled Node/WASM admitted the full pinned model and refused malformed bytes. The synthetic
  articulated plastic-heating report matches the complete native JSON; a short loose-band run
  is numerically gated and excluded from the frontier in actual WASM. This is not browser navigation
  or G4. Final all-target clippy also passes on fixed base `4bb1129` with the four explicit Rust overlays.
- [x] R2: corrected full native CLI integration passes (1 test, 8 filtered; 236.94 s), retaining
  plastic work, heat balance, both-CLI equality, frontier and coverage-only assertions. Actual direct
  `ftx drape` and immutable `round --sim` also return exit 2 with the numerical gating flag saved.
- [x] R2/R3: execute the full debug workspace: 1,536 passed, 2 failed, 28 ignored on its older
  snapshot. Both failed targets have passing current focused reruns: CLI (1 test, 8 filtered) and
  WASM scenario (3 tests). This is not a claim that a full current-snapshot command returned zero.
- [x] R2/R3: final frozen release run passes 114 library tests and all 4 contact KATs. Static-ring
  gauge constraints and 4 mm mesh converge in 13 iterations with ~5.5 µm clearance error, within
  the unchanged 20 µm limit. Pinned contact digest and cone/cylinder friction inequalities pass.
- [x] R2/R3: verify the kernel's increased 300-iteration work budget against the full production
  elastic ladder: all eight garments at friction 0.3/0.5 finish with no gating flags; original grip,
  waistband and monotonicity assertions pass. Final frozen release total: 119 passed, zero ignored.
  Complete fresh solo review and close `ftx-6en`; performance, empirical fit and release remain separate.
- [x] R8: correct the 21-tape description, frozen `outfit --sim` command, and numerical/parity
  boundaries in README and touched contracts (`60ae9b4`). Full scope/budget review remains open.
- [x] R3 / `ftx-eju`: reproduce the pinned converted asset with unchanged integrity checks; record
  supported environment and SHA-256 in `tools/body-model/PIN.md`. Linux conversion drift remains
  refused. Install the hash-checked model at the local documented default cache path outside Git.
  No oracle, pin, array hash or comparison tolerance changed.
- [x] R3: run all normally ignored production-Anny body tests against that admitted asset:
  **33 passed, zero ignored** (25 unit, 3 forward parity, 1 pose, 4 fitting). Sixteen forward cases
  differ by at most 1.049e-14 m; ten pose cases by at most 8.82e-16 m. Synthetic tape recovery passes
  its existing 1.5 mm bound. Public-prior wearer residuals remain reported, not real-person fit proof.
- [x] R3: five non-contact KATs, all four production fitted-bolero tests and all four final contact
  KATs pass on the admitted asset where required; physical assertions and pinned digests are intact.
- [x] R3: finish the production elastic-trouser ladder. Its earlier 100-iteration refusal is retained;
  the 300-iteration candidate passes both friction values. G2's current-hardware budget proof remains
  separate (R9); oversubscribed remote test elapsed time is not an M4 benchmark.
- [ ] R4: complete production-model capture-to-body acceptance, then private photo/tape comparisons.
- [ ] R5: acquire matched specimens/raw observations and execute held-out coupons and fixed-jacket
  swaps (`ftx-tm2`, `ftx-md1`, restored `ftx-ms3`, `ftx-tm8`, `ftx-tmf`).
- [ ] R6: continue source-qualified thermal/process/purchasing slices through their existing tasks;
  restored `ftx-ms2` remains open. No new coefficients or physical acceptance are inferred here.
- [ ] R9/R10: current hardware/input-bound benchmarks, native/wasm/browser comparisons, private
  muslin evidence and the existing `ftx-onn` release gate remain required.
- [x] Finalize fresh self-review, Beads evidence/flush and handoff notes; report failed/unavailable
  checks separately. Reservation release follows the final commit.

Raw command output for this block is retained outside the repository at
`/var/folders/vt/n2xyn_s51b97_j3yh2qbqcnc0000gn/T/ftx-followthrough-20261020T214254Z/`.
Remote checks use `RCH_REQUIRE_REMOTE=1`; the laptop's `sbh check --need 20G` failed its free-space-ratio
gate, so no heavy local compilation was substituted. This is not a current garment benchmark or G2 pass.

At handoff the live backlog has 83 issues, 127 dependencies and 82 comments; the original 74 rows,
114 dependencies and 65 comments were preserved. New
`ftx-eju` is closed after admitted-asset reproduction, source-backed diagonal-drift diagnosis and
successful body/fitted-bolero execution; `ftx-6en` is also closed after the completed release run.
The corrected articulated
collar (`y0 = -600 mm`, because placement subtracts the offset) directly replays with no gates,
zero final speed, positive post-settle plastic work and the expected cooler unconverted frontier;
the compiled integration assertion run passes. The final staged four-Rust-file UBS scan exits 1
with one critical match
at `Command::new(binary)` in the existing helper: every caller supplies a Cargo `env!` binary path,
not user input. This is a reviewed false positive; 739 inventory warnings remain, not a clean scan.
Changed-line findings were reviewed; no scanner suppression or test weakening was added.

Final checks use RCH's existing clean-overlay mode: fixed base `4bb1129` plus the literal touched
Rust files, with no branch or worktree. Concurrent commits invalidated an earlier content-mode
receipt; its runtime output is diagnostic, not a confirmed gate. The frozen WASM build passes,
remote/local artifact SHA-256 matches, and its complete synthetic plastic report equals native
JSON; the short loose-band negative is also gated in that artifact. Browser navigation and G4
remain unclaimed. Final all-target clippy passes with the same four-file overlay fingerprint
`41fb42b9…`; release library/contact/elastic tests all pass. The broad
workspace failures and their passing focused reruns are listed separately above.

### Fresh self-review of this implementation block

Creation-gate worksheet: running code does not consume this checklist. Its consumer is the user who
explicitly requested a granular TODO list; its gate is an accurate implementation handoff; its observed
defects are the lost backlog rows and unsupported numerical verdict described above. The checklist
retires at handoff into its named Beads. Recovery snapshots are separate minimal integrity controls:
they preserve both DB-only work and incoming JSONL before reconciliation of demonstrated corruption;
both stores are needed, a copy is the smallest reliable protection, and no cheaper destructive rebuild
is justified. The ready capability priority is numerical acceptance (`ftx-6en`); an hour of additional
report machinery would have less value. Verdict: keep only this requested checklist and command logs.

Real-work worksheet, task-owned changes since the initial assessment: source-clock repair, numerical
solver/report behavior and user documentation are USER (3); direct regression execution is ENABLER
(1); backlog recovery and this requested handoff are PROCESS (2); UNKNOWN (0). Concurrent material
features committed by other sessions are excluded from this task's delivery credit. The most visible
result is a moving drape retaining its diagnostics and gaining a gating flag. Without the process work,
the capability would still exist but scheduling/evidence would again be lost to the duplicate-comment
import failure. Existing CLI/scenario/wasm consumers exercise the solver/report seam; no new framework
was built. The older release and measured-material tasks remain open because numerical correctness
precedes their acceptance and physical specimens cannot be invented. No swarm was dispatched. No plan
edit substitutes for implementation and no parent gate was closed by creating these repair items.
Verdict: capability work dominates; continue focused checks, not more machinery.

Honesty inventory (solo self-review, **not independent verification**):

1. No (checked: task-owned test diffs retain the positive frontier, heat-accounting, replay and KAT
   assertions; added a short-run negative and a physically supported positive input).
2. No (checked: rollback, phase and CLI tests use the actual solver; synthetic report arithmetic is
   labelled and is not counted as measured cloth proof).
3. No (checked: oracle capture scripts and oracle fixtures are unchanged).
4. Yes: this repair changes the runtime acceptance gate and its tests together. It strengthens
   acceptance; no tolerance, ignore, suppression or bypass was added. Original positive checks remain.
   Countermeasure RH-1: retain those assertions and execute the new negative and real positive paths.
5. Fixture inputs changed after stricter acceptance exposed unsupported positives: a 3 ms loose band
   and an inadequately hardened plastic band. This is disclosed, not a numerical-limit adjustment;
   the original short-run negative remains. The elastic positive passed before the later plastic
   comparison failed. The revised plastic positive still requires actual execution before credit.
6. No (checked: 84 Python tests per version and 112 initial drape tests actually ran; filtered focused
   commands are reported with their denominators, not as full workspace results).
7. No (checked: saved command output; pending or failed broader checks are not called passed).
8. No (checked: synthetic Anny interchange and invented materials remain software evidence only).
9. The initial workspace run failed at the comparison frontier; the next focused run reached and
   failed the plastic frontier. Both failures are retained in logs and this handoff; no all-green
   workspace claim or closure of `ftx-6en` is made on that evidence.
10. No (checked: command logs combine stdout and stderr; CASS timeout output was retained).
11. No (checked: only the narrow timestamp bug was closed after fresh four-version execution;
    numerical, release and empirical parents remain open).
12. No (checked: the technical plan was not changed by this task; docs distinguish legacy metrics
    from the stricter result and retain the unmet production-asset and empirical gates).
13–17. Not applicable: no delegated agents or panes were dispatched or accepted as independent proof;
    concurrent external commits are not credited as this task's verified delivery.
18. No (checked: test-command denominators and explicit exclusions; no aggregate completion score).
19. The moments requiring explanation are the physical test-input changes and the repeated upstream
    restoration of old backlog bytes. Neither is a reason to hide failures or overwrite DB-only work.
20. Strongest current evidence: real solver rollback/replay plus the direct CLI test reaching the
    genuinely settled elastic frontier with all positive assertions intact. Full completion still
    depends on the pending commands and plastic case, not this statement.

Older-session inventory is unavailable: CASS reported a healthy index, but the bounded project-scoped
search timed out. This block does not claim an audit of older sessions. Disposition: no test weakening,
oracle regeneration or false closure found in this bounded review; observed invalid positive fixtures
are being corrected with explicit negative coverage and original positive acceptance retained (RH-1,
RH-2). Final command outcomes below supersede the intermediate status in this inventory.

### Completed follow-through disposition

Closed `ftx-dzf`, `ftx-eju` and `ftx-6en`; the larger source acquisition, capture acceptance,
performance, browser and private-muslin Beads remain open. Fresh solo review, not independent
verification: original physical inequalities, heat balances, parity tolerances and oracle bytes are
retained. Synthetic inputs and the static-ring constraints/mesh were corrected openly; no failed
physical cell was reclassified as excluded or accepted by relaxing accuracy.

Confirmed remote results: 33 production-body tests; five non-contact KATs; four fitted-bolero tests;
119 final release library/contact/elastic tests; final all-target clippy with `-D warnings`; WASM
build and actual compiled-WASM positive/negative replay. The full older-snapshot workspace command
returned 101 (1,536 passed, 2 failed, 28 ignored); both failed targets have passing current focused
reruns. No full current-snapshot zero-exit workspace run, browser navigation, hardware-bound
performance result, empirical material qualification, real-person fit or complete G2/G4/G5 pass
is claimed. The pinned model is now installed in the documented local cache outside Git.

### Browser continuation — `ftx-onn.1`

- [x] Refresh the live tree, ready Beads and existing frontend lane (64 passed, two optional cases skipped).
- [x] Identify obsolete round/body reuse and missing round-handle cleanup in the primary workshop.
- [x] Withdraw round/fold downloads when their actual inputs change or a replacement starts.
- [x] Bind worker replies and fold source documents to current round/body request identities.
- [x] Reject superseded async file reads; clear an absent or refused replacement body.
- [x] Free round handles on every extraction path; retain valid latest round/body/fold behavior.
- [x] Exercise positive, stale, failed replacement and cleanup paths in the existing frontend tests.
  All four new regressions fail against the original `92e1cec` page/worker and pass on the repair.
- [x] Run the existing compiled-WASM frontend lane with the previously confirmed frozen artifact;
  record its source boundary, without calling it a current-source build or browser navigation.
- [x] Finish the actual Chromium production-Anny bolero run, malformed replacement and result-withdrawal checks.
- [x] Fresh solo review and scoped UBS scan; close the repair Bead and flush it.
- [x] Prepare the scoped commit and reservation handoff; final completion receipts are in the session output.

This repairs browser input/result association; it supplies no empirical fit, source-material or timing evidence.
`FTX_REAL_WASM=1 node --experimental-vm-modules --test tools/check_trace_frontend.cjs` passes
**66 tests, zero skips**. The ordinary lane passes 64 with its two optional WASM cases skipped.
The WASM input SHA-256 remains `25e19a8b0a5f059c132ee3919eadc71e994d8d6d39525298dbe0172faf97643f`:
the previous confirmed fixed-base/four-overlay build, not a new compilation. The current Rust differences
from that base are exactly those four paths; this block changes JavaScript and documentation only.

The first Chromium command passes the three chart programs, URL revocation, production-model loading,
malformed replacement refusal and actual production bolero drape (`gating: []`, numerical settled,
30 consecutive quiet steps). It returns nonzero afterward because the test driver tries to fill the
Style textarea without opening its enclosing details panel. That failure is retained; the corrected
driver opens the panel and is rerunning the complete flow. Its earlier report and screenshot remain at
`/var/folders/vt/n2xyn_s51b97_j3yh2qbqcnc0000gn/T/ftx-workbench-browser-6AYQtQ/`.
The final driver and UBS outputs use the sibling `ftx-workbench-browser-final-IBMQDZ/` directory.
Viewer HTML still depends on the CDN identified in R8. This block verifies reports and download
lifecycle, not offline viewer rendering or the complete G4/G5 gate.
`git diff --check`, `node --check web/worker.js`, and `node --check tools/check_materials_frontend.cjs`
pass. The staged `ubs $(git diff --name-only --cached)` exits zero: two JavaScript files scanned,
zero critical, 251 warning and 581 informational findings. Changed-line findings were reviewed:
the listener warning is the test DOM adapter, deep-chain warnings are test assertions, and worker
JSON extraction is inside its outer catch. HTML is outside that scanner lane and was reviewed
directly and executed by the Node/Chromium checks. No suppression was added. No Rust source changes,
fresh Cargo compile/test/clippy or hardware benchmark are part of this JavaScript repair.

Fresh solo audit of this browser block (not independent verification):

- Creation worksheet: this is the existing user-requested granular checklist, not runtime state.
  Consumer: the user; gate: accurate handoff of `ftx-onn.1`; observed defect: old garment/model reuse;
  retirement: completed checklist rows hand off to that Bead. No integrity-control exception is needed;
  no new process artifact or framework is created. Highest actionable software item is this browser
  repair; an hour extending the report would deliver less value. Verdict: keep this bounded extension.
- Real-work worksheet: purpose is sewing patterns that sew together and fit a specific body, with usable
  manufacturing files. USER 1 (browser repair/contract), ENABLER 1 (direct regressions), PROCESS 1
  (requested checklist/Bead), UNKNOWN 0. Demo: edit an input and obsolete downloads/folding disappear.
  Omitting process would change tracking, not runtime behavior. Existing page/worker code exercises the
  tests; nothing speculative is enabled. Older material acquisition remains open because raw specimens
  and physical validation are unavailable here. No swarm or plan edit substitutes for implementation;
  no parent's unfinished acceptance was moved into this repair to close it. Verdict: HEALTHY; finish checks.
- Honesty 1: No (checked: existing test assertions unchanged; four new tests added).
  2: No (checked: explicit DOM/WASM doubles exercise actual modules and are labelled lifecycle evidence;
  separate compiled-WASM and Chromium paths run actual production code).
  3: No (checked: no oracle/golden/capture changes).
  4: Runtime stale-result guards and their regressions change together; no accuracy, CI, ignore,
  timeout, suppression or parity gate is relaxed. Original-source negatives fail on observable behavior.
  5: No (checked: request identities apply to ordinary worker traffic, not fixture detection).
  6: No (checked: focused five-test run and complete 66-test compiled lane have nonzero denominators).
  7: No (checked: completed outputs distinguished from the pending corrected browser command).
  8: No (checked: doubles, compiled WASM, real Chromium and the earlier frozen build are distinct claims).
  9: The initial regression failed because its DOM double replaced rather than accumulated event
  listeners; corrected the double to match DOM semantics, retaining every product assertion. The first
  browser driver fails on the collapsed Style panel after the positive drape; its failure is disclosed.
  10: No (checked: stderr retained in tool outputs or combined command logs).
  11: No (checked: repair remains in progress pending final checks; empirical/release parents stay open).
  12: No (checked: no technical-plan/spec edits; contract adds lifecycle behavior and labels old timings).
  13–17: Not applicable because this block is solo, with no delegated reports or independent verifier.
  18: No (checked: existing complete frontend lane and its optional-case denominator were selected before execution).
  Older-session mining is outside this block; the earlier bounded CASS timeout remains disclosed above.
  19: Explain the initial test-double and closed-panel driver mistakes and the earlier artifact's source boundary.
  20: Strongest evidence is actual production-Anny browser settling plus original-source failing lifecycle
  regressions; all are reproducible. Disposition: no test weakening or false closure found; RH-1/RH-2
  countermeasures are retained assertions, original-source negatives and separately labelled runtime proof.

Final browser disposition: the corrected complete command returns **zero**. All three chart drafts,
empty-tape NOT_READY, invalid-stature refusal, revoked downloads, refused replacement model, actual
production-Anny bolero drape and post-drape style invalidation pass. It records zero page errors;
all four local HTTP requests carry the mandated user agent. Both browser runs produce byte-identical
`sim_report.json`, SHA-256 `d6f1d1cf8c89db4b456e2d3b3018ae86b3fa795ad75dea0ca07415630fa624d4`.
The bolero reports 30 consecutive quiet steps and no gating flags. No native comparison or benchmark
is inferred from those repeated browser reports. The initial driver's failure above is superseded,
not erased. `ftx-onn.1` is complete on fresh solo re-execution; parent `ftx-onn`, the physical-material,
capture, offline-viewer and performance work remain open.

### Offline-viewer continuation — `ftx-onn.2`

- [x] Reproduce the current downloaded viewer offline: two attempted CDN imports, no canvas and no ready state.
- [x] Retain exact Three.js r160 and OrbitControls bytes from the primary upstream tag, with hashes
  and the complete MIT license; keep the existing renderer version and two-module closure.
- [x] Document the named `viewer-offline-r160` HTML exception in spec 04 before implementation;
  keep numerical payloads, oracle fixtures and capture scripts unchanged.
- [x] Embed both modules and their notice through the existing shared `viewer_html` path;
  replace template markers before user payload insertion and refuse missing/duplicated markers.
- [x] Extend existing unit/CLI regressions and the existing browser checker with offline rendering,
  frame/pixel changes, controls and zero HTTP(S) requests. No new harness or crate.
- [x] Pass focused Rust tests and the actual native viewer reconstruction.
- [x] Pass all-target clippy and workspace tests on the named frozen remote inputs.
- [x] Build WASM and execute its viewer generation, then open that generated HTML fully offline.
- [x] Record actual native/WASM artifact and browser outcomes; do not credit preview templating as compiled proof.
- [x] Fresh solo audit, scoped UBS review, Beads evidence/flush and scoped commit/handoff preparation;
  final commit and reservation-release receipts are in the session output.

Checks are retained at
`/var/folders/vt/n2xyn_s51b97_j3yh2qbqcnc0000gn/T/ftx-offline-viewer-checks-pwtimqo3/`.
The laptop's fresh `sbh check --need 20G` fails its ratio threshold (200.6 GB available, 5.02% free),
so Rust compilation remains strictly remote. Local `cargo fmt --check` and Node syntax checking pass.
The first remote request refuses with RCH-E413/exit 103 because this task added the CLI fixture-path
print while admission was queued. No local compilation followed; selected Rust/assets are now frozen
and that request is retried. This is a source-freeze mistake, not a passing compiler result.
An external HTML preview reuses the prior real production payload to check the proposed module packaging;
it is labelled `preview-not-compiled.html` and cannot establish Rust/WASM generation acceptance.

The frozen native command now exits **zero** on `ovh-b`: `RCH_REQUIRE_REMOTE=1 rch exec --base
37e80c9 --clean-overlay --overlay-path crates/ftx-cli/tests/sim_points.rs -- cargo test --locked -j1
--no-fail-fast -- --nocapture`. Its complete denominator is **1,668 passed, zero failed, 28 ignored**
across 234 test-result groups (including empty binary/doc groups). Both new `animation` regressions
and the real CLI viewer reconstruction pass. This is the named frozen source, not later incoming
material/sewing changes on shared main. The sole overlay adds the printed artifact path; no assertion
or test selection is relaxed. `workspace-ovh.log` retains the remote exit and overlay receipt.

That CLI-produced synthetic 12-vertex, 20-frame HTML is retained as `native-fold-viewer.html`,
SHA-256 `844a3f2b5976afc3323552965467414febd9eb00cf5b1f0c4d6317841b691740`.
Two fresh offline Chromium executions pass file and blob rendering, upstream-module hashes and full
license, retained-frame navigation, differing board/draped pixels and playback, with **zero HTTP(S)
requests and zero page errors**. The second complete output is `native-offline.log`; this is actual
CLI generation/rendering evidence, not a physical cloth or real-person fit result.

The frozen no-overlay `cargo build --locked -j1 --target wasm32-unknown-unknown -p ftx-wasm`
also exits **zero**, remotely on `hz3`. Its returned raw WASM is 186,280,077 bytes, SHA-256
`dc71b587623b8de0c7444ac073c98593838d94d8338aa7125244e858a5f9a1a7`, independently reread from
the worker and matched locally. `wasm-any-linux.log` and `wasm-artifact.json` identify the source,
artifact and fresh external wasm-bindgen 0.2.128 Node bindings. Actual production-Anny `fold_up`
execution is running; its generation/rendering acceptance remains pending until that command finishes.

Resource failures remain part of the record: earlier explicit-worker requests time out with
RCH-I001/exit 103; Windows selection refuses this WASM target instead of bypassing target-OS policy.
Two obsolete queued requests were cancelled by their exact owned PID/parent/argv; no peer job was
stopped. Fresh probes found `ovh-b` healthy and `wsurf` healthy for its supported OS, so their stale
temporary bypasses were cleared using `rch workers enable`; critical-low-disk `hz4` remained bypassed.
No capacity threshold was lowered or worker data cleaned. DSR registry lookup and the prescribed
quality dry-run exit **4** because `franken_textiles` has no configured tool entry; no substitute
release/quality claim or new DSR configuration is made.

The full scoped UBS scan exits **2/partial**: ast-grep truncates output at 16,777,216 bytes while
analyzing the unmodified minified renderer. Its six critical JS findings were inspected in context:
two existing Playwright `$eval` callbacks misclassified as dynamic eval; two upstream pointer-number
loose comparisons; a constant-time warning on the minified line (which includes ordinary shader/cache
comparisons, not an authentication path); and two frozen `__proto__:null` constructor-name tables
matched by the prototype-pollution regex. The last finding was initially attributed too broadly to
generic upstream property assignment; rereading the exact scanner pattern and both source matches
corrects that attribution. This is
a bounded integration review, not an upstream security certification or a clean complete scanner pass.
The Rust lane reports zero critical findings. No vendor bytes, scanner rule or suppression was changed.

The matching frozen remote `cargo clippy --locked -j1 --all-targets -- -D warnings` exits **zero**
on `ovh-b` (`clippy-ovh.log`). Final local `cargo fmt --check`, Node syntax and `git diff --check`
pass. The current ordinary frontend lane passes **72/74 tests, zero failures, two optional compiled
cases skipped**; it explicitly uses DOM/WASM doubles and is not the new module's execution proof.
After the frozen builds, the stale CDN wording in `ftx-wasm/src/fold.rs` is corrected as a comment-only
change; no compiled behavior changes. Incoming peer material/sewing code is preserved and receives
no completion credit from this viewer block.

Creation worksheet for this bounded continuation (October 20/21): runtime does not branch on this
report. Consumer: the user who explicitly requested the granular TODO; gate: accurate `ftx-onn.2`
completion handoff; observed defect: the downloaded page attempts two CDN imports and renders no
canvas offline; retirement: these checklist rows become completed handoff history when the Bead
closes. No integrity-control exception is used (its four prompts are inapplicable because the explicit
consumer/gate already applies). Highest ready capability: making this existing viewer work offline;
an hour extending reporting would deliver less value than finishing its actual WASM execution.
Verdict: retain this minimal extension to the existing requested report; create no new framework.

Actual new compiled-WASM execution finishes successfully through `WebBody`, `run` and `fold_up`,
freeing every handle in `finally`. Inputs are the public-prior admitted Anny asset and the synthetic
92 cm / 30 month chart bolero, not a real wearer's data. Output lives in `wasm-bolero-RsL0xJ/`:
the 3,923,390-byte HTML has SHA-256
`0827c84c2ac2622dd7eddea1856958b9ad1cf04b66c524bbd09e2ba0b4ee2248`.
The complete `wasm-offline.log` confirms offline file/blob rendering, exact module hashes/license,
frame/pixel changes and playback, **zero HTTP(S) requests and zero page errors**. The actual final
canvas was visually inspected. The report has no gating flags and 30 consecutive numerically quiet
steps; this does not establish static stress equilibrium or real-person fit. Its SHA-256 remains
`d6f1d1cf8c89db4b456e2d3b3018ae86b3fa795ad75dea0ca07415630fa624d4`.
The embedded 2,978,334-byte payload also matches the earlier real browser output byte for byte,
SHA-256 `bcdc2dbd48bfdab0ca6f0f407dfb9f79ae691ccbe3dca129019f3b5b5ce76d87`.
This is one repeated bolero case across old/new WASM artifacts, not a new native/WASM differential
suite, oracle fixture, performance benchmark or entire browser/release gate.

Fresh solo audit of this offline block (no independent verifier or delegated agent):

- Real-work window: `ftx-onn.2`, from CDN failure reproduction through actual generation/rendering.
  Purpose from README/charter: sewing patterns that sew together and fit a specific body, with usable
  manufacturing files. Actual scoped history reviewed: `ba5bd20` USER (embedded renderer, generator
  and regressions), `460e9f6` PROCESS (contract provenance), `37e80c9` PROCESS (exception/TODO),
  `f15c26a` PROCESS (Bead evidence), `ceb43bf` ENABLER (executed offline browser checker).
  Tally by these five commits: USER 1, ENABLER 1, PROCESS 3, UNKNOWN 0; commit counts are not value
  or effort metrics. The remaining test-path print/comment and this handoff are bounded support.
  Demo in two minutes: disconnect networking, open the downloaded HTML and scrub/play the drape.
  Omitting PROCESS would change traceability and the requested checklist, not rendering; stop adding
  reporting after this handoff. Existing native/Chromium consumers exercise the ENABLER now.
  Oldest open user-facing item is the release gauntlet `ftx-onn`; this actual offline failure blocked
  a smaller deliverable, while measured-material and hardware-bound release proof remain unfinished.
  Swarm comparison is inapplicable: no agents were dispatched. No spec-only success or follow-up
  laundering: the named packaging exception accompanies the actual renderer, the parent stays open.
  Verdict HEALTHY by delivered capability/effort, with too many automatic support commits to treat
  commit counts as progress; correction is to finish this handoff and return to product work.
- Honesty 1: No (checked: complete owned unit/CLI/browser test diffs; assertions only added, no CI,
  fixture, ignore or tolerance edits). 2: No (checked: exact upstream modules and actual Rust/WASM/
  Chromium execution; the external preview remains explicitly labelled). 3: No (checked: no oracle
  or golden paths changed). 4: Product and checker changed together; reviewed the added positive
  assertions separately, including module hashes, nonidentical pixels and zero requests. No scanner
  bypass or tolerance relaxation; unsafe shell output redirection was refused and replaced with
  exclusive creation of a fresh log, preserving stderr. 5: No (checked: ordinary shared generator,
  no fixture detection or fabricated renderer). 6: No (checked: 1,668 executed native tests, actual
  generated pages and live canvases; empty result groups are included openly in the group count).
  7: No (checked: observed remote exits/artifact hash and both browser logs). 8: No (checked:
  preview, synthetic CLI cache, real compiled WASM and prior browser comparison are separate claims).
  9: The source-freeze mistake, admission failures, unavailable DSR entry and partial UBS scan remain
  disclosed; the prototype-warning attribution was corrected after reading exact source matches.
  10: No (checked: compiler/browser stderr is retained; existing helper suppression was removed).
  11: No (checked: only the positive offline-viewer acceptance is eligible for closure; parent and
  empirical/performance work stay open). 12: The source/spec exception deliberately changes CDN
  packaging, documented before implementation; original numerical/control/payload requirements and
  fixtures are preserved. No claim of reference-identical HTML bytes.
  13–17: Inapplicable because the work is solo; external shared-main commits are preserved and are
  not accepted as delegated verification or credited to this block. 18: No (checked: complete native
  suite and offline positive path were selected before results; failed admissions are not passes).
  Older-session mining is outside this bounded block; the earlier CASS timeout is already disclosed.
  19: Explain the late fixture print invalidating the first admission, the initial overbroad scanner
  attribution, and why frozen-source checks cannot certify later incoming shared-main features.
  20: Strongest reproducible evidence: actual CLI and new compiled-WASM HTML both create canvases,
  change rendered frames and play in real Chromium with networking disabled and zero HTTP requests.
  Disposition: positive capability verified by fresh solo re-execution, not independently verified.
  RH-1/RH-2 countermeasures are retained assertions, explicit proof classes and exact source/artifact
  boundaries; the failed requests/partial scanner remain visible. No accuracy gate was weakened.

Final local integration: the four previously owned ignored `web/pkg` outputs were copied to
`/var/folders/vt/n2xyn_s51b97_j3yh2qbqcnc0000gn/T/ftx-before-offline-web-pkg-ltyfaudn/` before
the existing wasm-bindgen web step refreshed them from the verified raw module. No Rust compilation
ran locally. The refreshed browser WASM is 39,202,236 bytes, SHA-256
`e372ee64bf7d1f46b631cf92d1a87f595c9e735676404f952146dbf5b22de4d7`; `web-pkg-refresh.json`
records all four hashes. `node tools/check_web.cjs` with the installed Playwright-core override
then exits **zero**: real module loading, all three chart programs, empty-tape NOT_READY, invalid
stature refusal and zero console errors (`web-current-wasm.log`). This command omits `--model`;
the new module's actual bolero generation/offline rendering is the separately recorded check above.

The exact staged `ubs $(git diff --name-only --cached)` exits **zero**: two Rust files scanned,
zero critical, 85 warning and 52 informational findings. Changed-line review identifies only the
intentional test fixture-path print; remaining assertions/unwraps/clones are test mechanics and
the JSON string-key indexing warnings do not establish array bounds failures. This pass does not
replace the disclosed partial scan of the already committed upstream renderer. No suppression or
pre-commit bypass was used. `ftx-onn.2` is now closed with source-bound positive acceptance and the
fresh-solo limitation, and `br sync --flush-only` reports nothing left to export. Parent `ftx-onn`,
empirical material/fit work and performance gates stay open. Both unrelated untracked roots and all
incoming peer edits remain untouched.

### Production image-fit continuation — `ftx-onn.3`

- [x] Refresh full AGENTS/README, live Beads/dependencies and the existing full-model/tiny-model boundary.
- [x] Read plan 9.4/10.2, camera/fitter/reuse contracts and the actual CLI adapters.
- [x] Add a direct opt-in production case to existing `body_model_fit.rs`; no new framework or asset.
- [x] Execute the admitted full-model case remotely: three moving poses, two nontruth starts,
  predeclared held-out pixels and withheld tape predictions at the 3 mm girth / 2 mm length bounds.
- [x] Verify actual accepted-shape body export without refitting or image-pose reuse.
- [x] Verify incompatible held-out evidence and exhausted budgets retain diagnostics without a solution.
- [x] Investigate exposed failures: correct two fixture errors; no production defect found in this case.
- [x] Run focused existing regressions, formatting and all-target Clippy; record exact source/results.
- [x] Update capability docs, fresh solo review, Beads/flush and scoped commit/handoff preparation;
  final commit/reservation-release receipts are in the session output.

This is a bounded numerical qualification of the existing fitting/reuse path. Known synthetic vertex
correspondences and calibrated cameras do not establish detector visibility, raw-image calibration,
segmentation accuracy, complete capture-to-round acceptance, real-person fit or performance. The admitted
model remains outside Git. Creation gate: this extends the user's requested TODO, consumed by the user
for `ftx-onn.3` handoff; the observed defect class is production-model acceptance absent from tiny-model
tests; rows retire to completed handoff history. No new process artifact or integrity exception.
The next value is executing the actual case, not extending this report.

The first remote release command against `f9afcc7` plus the test overlay returns **101** at
compilation: the new fixture incorrectly used `Mat3 * Vec3` where this crate requires `mul_vec`.
No production fit ran in that request. The test call is corrected; the next frozen request runs
the complete three directly relevant CLI targets (`body_model_fit`, `body_fit_workflow`,
`accepted_body_workflow`) with `--include-ignored --nocapture`, plus a separate all-target Clippy.
Only stature constrains the solve; **20** other tape predictions are withheld checks, alongside
24 held-out pixels among 144 observations. Added direct skin/topology comparison and rejected/budget
report import checks exercise the actual export, not just its description. These stay in the existing
test file. Logs are retained outside Git at
`/var/folders/vt/n2xyn_s51b97_j3yh2qbqcnc0000gn/T/ftx-production-image-fit-checks-f1_d_1x8/`.

That three-target request also returns **101**: 25 existing tests pass, while the new case fails
its full-resolution image-bounds assertion **before fitting**. The fixture's guessed camera height
does not frame the production body. The synthetic camera is corrected to face the posed vertical
midpoint from 3 m, with the same intrinsics/distortion and the original 1280×960 bounds assertion;
no optimizer, acceptance gate or target shape changes. The prior all-target Clippy exits zero on
`vmi1264463` for overlay `73a0c9760249810f598b406b860700f0de4f0f759bf0fbb7e0b2362462d39520`.
Both checks are rerunning the framed fixture; that earlier lint result is not its runtime acceptance.

Final frozen source: `f9afcc7ebbff93089f5751d598a9c0747bc02e27` plus only
`crates/ftx-cli/tests/body_model_fit.rs`, overlay
`d78ebafd25d6b7fdb90b3b8cee1ec9d0ef7f2a2754ca2629920717ea9cbaa178`.
`RCH_REQUIRE_REMOTE=1 rch exec --base f9afcc7 --clean-overlay --overlay-path
crates/ftx-cli/tests/body_model_fit.rs -- cargo test --locked --release -j1 -p ftx-cli
--test body_model_fit --test body_fit_workflow --test accepted_body_workflow --
--include-ignored --nocapture` exits **zero** on `ovh-b`: **26 passed, zero failed, zero ignored**
(9 + 15 + 2). `FTX_ANNY_MODEL` is forwarded to the hash-checked external asset; every new positive
and negative path actually executes. Matching all-target
`cargo clippy --locked -j1 --all-targets -- -D warnings` exits **zero** on `vmi1264463`.
Final local `cargo fmt --check` and `git diff --check` pass. No local Rust compilation ran: the
fresh disk guard fails its free-space ratio (193.9 GB / 4.85%). No runtime/library/WASM code changed;
a fresh WASM build or full-workspace test repetition was not run for this native integration test.

The actual body has 13,718 model vertices; its exported checked skin has 13,348. Both nontruth
starts converge, consuming 180 model calls each, plus one publication call. The two shared controls
and all 12 root/joint pose values recover within the test's predeclared `1e-5` recovery tolerances.
Every withheld tape prediction passes 3 mm girth / 2 mm length bounds, and every exported skin vertex
passes its 2 mm comparison with the known rest shape; topology is unchanged. These near-roundoff
synthetic residuals are numerical self-consistency, **not physical measurement precision**.
Only stature entered as a tape constraint; the 20 other values were not used by optimization.
Changing a held-out pixel by 20 px gives REJECTED with identical fitting histories and no solution.
A one-call budget gives BUDGET_STOP with no solution. Both saved failed reports refuse body export
with no output directory; there is no tape fallback or false READY evidence.

Fresh solo re-execution runs the same already-compiled test and CLI under RCH's non-compilation
job mode, with one test thread and one honestly declared slot. The test binary SHA-256 is
`8039ee3645f1c07e4310fbdce1ad3d31b7af8e4206c464d3f6bc27712dbcf307`; CLI SHA-256 is
`249871edc28a7c786c4eb789dc6ce30395de11f457d3ee7b118d5233024b358a`, unchanged before/after.
The replay exits **zero**, one test passed and its sibling explicitly filtered, returning all 20
evidence files under ignored `out/ftx-onn3-verification-20261021-0204/` (no source/framework changes).
The actual returned reports were read: two CONVERGED cells, 361 total model calls, largest image
residual below `1e-7` px, REJECTED/BUDGET_STOP solution null, identical accepted/rejected fit-only
histories and exact accepted-report bytes in `reused/body_fit.json`.
Accepted report SHA-256: `a3b2775c06e14daed7c0b563bd25b7af62d17e16786582aa5bc5e56c2664256e`;
exported OBJ: `7145309652d1a62284fe642dacaaf8346fc87a2c2fc6a691b9129a5eeb4e3965`.

The first artifact copy failed because RCH had already removed its managed temporary root.
The initial composed replay shell was rejected by DCG's static-analysis guard; the literal,
reviewable command was accepted. Its first RCH admission refused the default four-slot estimate
on the three-slot worker. The supported per-request one-slot setting matches the single-threaded
runtime replay, and admission then waited for peer jobs without stopping or altering them.
`production-replay-1core.log` retains that command, binary hashes and complete result transfer.
No guard was disabled, no local fallback occurred and no fixture/harness retention layer was built.

Fresh solo worksheets for this block (not independent verification):

- Creation: this existing report/TODO is PROCESS, consumed by the requesting user for `ftx-onn.3`
  handoff, justified by the observed tiny-model-only qualification gap, retired to completed history
  at closure. The integrity exception's four prompts are inapplicable because the explicit request
  supplies the gate. Highest actionable item was executing this full-model path; another hour of
  reporting would deliver less value. Verdict: legitimate bounded handoff; stop extending it.
- Real work: window is this Bead's actual diff/commands, not unrelated incoming material commits.
  Purpose from README: patterns that sew together and fit a specific body, with usable manufacturing
  files. USER 1 (user guide's precise supported numerical boundary), ENABLER 1 (direct production-model
  CLI qualification), PROCESS 1 (requested TODO/Bead), UNKNOWN 0. No runtime feature or defect fix is
  claimed. Demo: run the documented opt-in test and inspect its actual accepted body and residuals.
  Omitting PROCESS would affect tracking, not the result. The existing CLI consumes every enabler
  now. Oldest open user-facing item is the release gauntlet; this closes one tractable proof gap
  while its physical/capture/performance conditions stay open. Swarm comparisons do not apply:
  no agent was dispatched. No spec edit substitutes for code execution and no unmet acceptance
  condition was moved out of this Bead. Verdict: healthy bounded proof work; return to product gaps.
- Honesty 1: No (checked: original test unchanged; a new asset-dependent case is explicitly ignored
  by default and actually run with include-ignored). 2: No (checked: real binaries/full admitted
  model; synthetic projections labelled, no mock). 3: No (checked: no oracle/golden changes).
  4: New checks accompany the test seam; bounds/residuals/rank/convergence are unchanged. Only the
  invalid synthetic camera framing and wrong matrix API were corrected. No suppression, CI/validator
  edit or guard bypass. 5: No (checked: hash admission is a source pin, not a fabricated success
  branch). 6: No (checked: 26 actual tests and one separately counted replay, not zero-run green).
  7: No (checked: observed remote exits, binary hashes and returned reports). 8: No (checked:
  self-generated model projections are numerical recovery, not reference-oracle or real-photo proof).
  9: Both failed requests, lost first artifact copy and guarded/admission retries remain disclosed.
  10: No (checked: command stderr retained in logs and every child command's saved stderr file).
  11: No (checked: closure covers only this stated synthetic production-body path; R4/parent stay open).
  12: No (checked: no technical-plan/spec change; guides distinguish this case from broader acceptance).
  13–17: Inapplicable because this block is solo, with no delegated claims or peer agreement counted.
  18: No (checked: three workflow targets and withheld 24/144 pixels, 20/21 tapes fixed before results).
  Older-session mining is outside this block; the earlier bounded CASS timeout remains disclosed.
  19: Explain the two fixture mistakes, artifact-loss/replay cost and absence of a runtime defect.
  20: Strongest repeatable evidence is full admitted-model fitting followed by actual exported-skin
  comparison, with failed reports independently refusing export. Disposition: fresh solo source
  review/re-execution, no independent verifier; RH-1/RH-2 countermeasures are retained assertions,
  predeclared partitions and explicit numerical/empirical proof boundaries. Stop further paperwork.

The exact staged `ubs $(git diff --name-only --cached)` exits **zero**: one Rust test file,
zero critical, 134 warnings and 35 informational findings. Reviewed changed-line findings are
test assertions/expected unwraps, deliberate diagnostic prints, bounded casts/fixture formatting,
and writes inside the exclusively created per-test directory; no new untrusted runtime path or
suppression. Existing predictable-temp warnings in the older test remain outside this change.
`ftx-onn.3` is closed with the positive evidence and fresh-solo limitation; `br sync --flush-only`
reports nothing left to export. All peer Bead rows are semantically preserved. R4's broader
capture/garment/real-photo conditions and parent `ftx-onn` remain open.

### Release-browser continuation — `ftx-onn.4`

- [x] Refresh the live tree and R9 boundary; commit/flush/release the completed image-fit slice first.
- [x] Fingerprint the actual host: Apple M5 Ultra, 36 logical/physical cores, 256 GiB, macOS 27.0.1.
- [x] Start strict-remote existing-release-profile WASM build from frozen `6ce0c0e`, no overlay.
- [x] Extend only the existing browser driver with explicit `--timings N` mode (about 80 lines).
  Default native-byte comparison remains unchanged. Invalid zero/fraction counts refuse before launch.
- [x] Verify returned release artifact hash and preserve old owned `web/pkg` before standard regeneration.
- [x] Run actual Chromium: fixed 92 cm / 30 month chart inputs, bolero then trousers, three declared
  warmups and **20 retained runs per garment**, 2 mm self-contact, accepted Newton steps and no gating.
- [x] Retain every attempted row/report, input/model/source/artifact/browser/host hashes, empirical
  nearest-rank p50/p95 and min/max; no latency summary for incomplete or failed numerical/offline checks.
- [x] Verify first retained viewer for each garment with the existing offline file/blob/control checks.
- [x] Review results/limits, scoped UBS and fresh solo honesty checks; complete Beads/commit handoff
  with the final scoped command receipts in the session output.

This uses the existing release build and numerical path, with no algorithm/tolerance/solver change,
profiler attribution or OS tuning. Timings describe this shared M5 Ultra desktop and its explicit
warm policy; the M4 native target, broader material/history workloads and parent gauntlet stay open.
Creation gate: the requesting user consumes this existing TODO/evidence for `ftx-onn.4` handoff;
observed gap is the debug local bundle plus unqualified historical timings; rows retire at closure.
The immediate valuable action is delivering/executing the release artifact, not a new benchmark framework.
Initial `hz3` namespace setup refuses with status 73 (`Killed`); RCH retries remotely on `vmi1152480`.
Build/measurement logs use external `ftx-release-browser-checks-uvnlnenl/`; no local compilation.

The strict-remote `cargo build --locked --release -j1 --target wasm32-unknown-unknown -p ftx-wasm`
exits **zero** on `vmi1152480`, clean base `6ce0c0e0dd2fcf5c144468529fe462269ff8dc2f`, no overlay.
Raw WASM: 12,802,969 bytes, SHA-256
`d500626a2349a6fbfbd288617720c576448c865eddc2c34de1f255e6724e482c`, matched against the worker.
Standard wasm-bindgen **0.2.128** produces the 12,641,323-byte browser WASM, SHA-256
`70798715ddf6abd1493e96d0fc671097ee281274819ed568075f48cac0caae1b`.
The prior four files are preserved at external `ftx-before-release-web-pkg-8a4yoryu/`; hashes were
rechecked before replacement. `release-artifact.json` records the build and each generated file.
The new actual compiled-WASM frontend lane passes **74 tests, zero failures, zero skips**.
Node syntax/diff checks and the ordinary Chromium draft/refusal lane also pass.

The actual command exits **zero**:

```sh
FTX_PLAYWRIGHT_MODULE=playwright-core \
NODE_PATH=/Users/jemanuel/projects/frankenpiano_vst/web/node_modules \
FTX_WASM_PROFILE=release node tools/check_web.cjs \
  --model /Users/jemanuel/.cache/franken_textiles/anny-0.6.1-default.ftxbody --timings 20
```

Chromium **156.0.8078.4**, Node **26.8.1**, arm64 Darwin 27.0.0 (macOS 27.0.1), M5 Ultra,
36 logical cores, 256 GiB. Shared host, no affinity/clock/kernel tuning or cache flush; model
retained in the worker, one draft per garment, sequential bolero then trousers. Duration uses
Node's monotonic clock, fold click through completed UI result including tape fitting/sewing/drape;
initial model upload/parse, drafting, export checks and offline-viewer tests are outside that window.
All **46/46 attempted runs** pass: three warmups and 20 retained samples per garment. Every raw
report was reread: gravity simulated, every recorded Newton solve converged, numerical settled,
zero gating, enabled 2 mm self-contact and a stable per-garment digest. No run exceeded 300 s.

| Garment | Retained n | Minimum (s) | p50 (s) | p95 (s) | Maximum (s) |
|---|---:|---:|---:|---:|---:|
| Bolero, 2,073 vertices / 3,744 faces | 20 | 16.294 | 16.658 | 17.433 | 18.799 |
| Trousers, 3,738 vertices / 6,771 faces | 20 | 45.528 | 45.961 | 46.804 | 47.074 |

These are empirical nearest-rank quantiles, independently recomputed from the retained JSONL,
without tail-confidence estimates, cold-start claims or hardware extrapolation. Materials explicitly
report `legacy_uncertain`; numerical settling is neither empirical fabric validation nor real fit.
No controlled comparison against the prior debug artifact was run, so no speedup is claimed.
This is a source-bound browser workload measurement, not the M4/native/material-history/full G5 gate.

Evidence is hash-copied to ignored `out/ftx-onn4-browser-release-20261021-0312/`: `browser/` contains
the complete fingerprint, exact three input documents per garment, all 46 reports/rows, both summaries
and viewer HTML; `screenshots/` holds board/draped images; `checks/` retains complete build/frontend/
Chromium logs and artifact/backup receipts. The original temporary files are preserved too.
Web bindings and runtime sources matched their startup hashes after the complete run. Backend build
base is `6ce0c0e`; the uncommitted driver at execution is bound separately by SHA-256
`5e327b9c9e0f15fa1624fc00ae9e66c38b0f50a8669ab67c8e80bd7f94bee94b`.
The admitted model remains SHA-256 `8f0bb151ba19975437147d239b7ff237642a7fa63659a6fac49fa23c299a0915`.

Both saved viewers pass real offline file/blob initialization, canvas/frame/pixel/control/playback
checks, exact upstream renderer/license checks, **zero HTTP requests and zero page errors**; their
draped screenshots were also visually inspected. Bolero report/viewer SHA-256:
`d6f1d1cf8c89db4b456e2d3b3018ae86b3fa795ad75dea0ca07415630fa624d4` /
`0827c84c2ac2622dd7eddea1856958b9ad1cf04b66c524bbd09e2ba0b4ee2248`.
Trousers report/viewer:
`e8763844c5a93499d7e8264b3178aac4b7f86e0e45dcfe59e2ac70e74e46ade8` /
`e247d987f34701cbd2fb0f48b302c3907cb6fedb136871b8b4b4073d0f151aff`.

Fresh solo acceptance: reread the complete driver diff and original Bead criteria, then recomputed
both summaries, all input/source/artifact hashes and each raw Newton-step sample. A first inspection
assumed a top-level gravity field and stopped with `KeyError`; the actual field is
`numerical.gravity_simulated`, and the corrected inspection passes all 46 reports. No source, report
or gate changed. Standard wasm-bindgen nodejs postprocessing of the same verified raw release module
(no Rust compilation) then re-executes both exact saved input triples. Both generated reports and
viewer HTML match Chromium **byte for byte**. The two replayed HTML files separately pass the real
Chromium offline file/blob/control/pixel checks again, zero HTTP requests/errors. Replay results and
complete stdout/stderr are retained in `replay/`. These two extra executions are not timing samples
or native/WASM parity evidence. This is fresh solo verification, **not independent verification**.

Bounded worksheets for this block, using the actual diff/commands and live open/closed Beads:

- Creation (existing report/TODO and raw timing output, 2026-10-21): PROCESS, no runtime branch.
  Consumer: requesting user; gate: reviewable `ftx-onn.4` delivery; observed gap: debug bundle and
  historical timings without retained repeats; retirement: completed handoff history at closure.
  The user's explicit detailed-TODO request supplies the gate. Integrity exception Q1–Q4 do not
  apply because no exception is needed. Highest ready bounded action was the release-browser
  delivery; another hour of reporting would be worse. Verdict: legitimate minimal handoff; stop
  adding machinery. This report's size is already high; no new audit/report framework was created.
- Real work (this Bead only): README purpose is sewing patterns that sew together and fit a specific
  body, with usable files. USER 2 (usable release bundle and measured user guide), ENABLER 1
  (existing checker extended and actually exercised), PROCESS 2 (requested TODO and Bead), UNKNOWN 0.
  Demo: load the release page/model and open either resulting offline viewer. Without PROCESS,
  the product still works but the requested accounting/raw evidence is lost. The enabler's actual
  consumer ran 46 numerical cases plus both offline viewers. Oldest open user-facing item remains
  `ftx-onn`; this closes its tractable browser-delivery slice while its original acceptance remains
  open. No swarm/pane comparison applies (solo). No plan/spec edit replaces implementation, and no
  condition was removed from the parent to close the child. Verdict: bounded delivery/proof work;
  return to product work instead of more reporting.
- Honesty 1: No (checked: driver diff adds a lane; default assertions/native comparison unchanged).
  2: No (checked: real compiled WASM, model, Chromium and outputs; no new mocks).
  3: No (checked: no oracle/golden edits). 4: Yes, checker code changes alongside delivery: timing
  mode explicitly omits native comparison and labels that boundary; the default path is unchanged.
  Original numerical acceptance, 2 mm contact and 300 s budget are retained, with no new suppression,
  timeout widening or guard bypass. This diff was separately reread before closure.
  5: No (checked: both preselected garments, full contact, all attempts retained; no success branch).
  6: No (checked: 74 compiled frontend tests and 46 browser attempts actually executed).
  7: No (checked: remote build exit/hash, actual runtime logs and raw rows). 8: No (checked:
  replay and timing lanes are labelled; neither establishes native parity, empirical fabric or fit).
  9: No (checked: namespace retry, disk refusal, raw-inspection error and broader open gates disclosed).
  10: No (checked: cited commands preserve stderr in complete logs). 11: No (checked: this child
  covers its original positive delivery; parent/R9 remainder stay open). 12: No (checked: no plan,
  spec or requirement edit). 13–17: Inapplicable because there was no delegation or peer agreement.
  18: No (checked: three warmups/20 samples/two garments fixed before execution; 46/46 includes all
  attempts). Older-session mining was not repeated for this bounded slice; the earlier CASS timeout
  remains disclosed. 19: Explain the long shared remote build, non-independent review and initially
  wrong inspection field; none is a performance/accuracy result. 20: Strongest evidence is actual
  repeated Chromium folds with unchanged digests and a fresh exact-input replay producing identical
  reports/viewers. Disposition: the lane's native-comparison exclusion is explicit in code, Bead and
  report (RH-1/RH-2); original checks preserved, no self-review presented as independent authority.
  No weakened gate or fabricated result found in this bounded review.

Pre-commit `ubs $(git diff --name-only --cached)` returns **2, partial**, not a pass: its JavaScript
taint dataflow work limit prevents complete eval/command analysis. One JavaScript file is scanned,
with 2 critical, 22 warning and 48 informational findings. Both critical hits are existing Playwright
`page.$eval` calls with fixed authored callbacks, not JavaScript string `eval`; neither changed in this
block. Reviewed remaining findings cover synchronous file I/O in this sequential checker, exclusively
created output files, normal failure propagation, guarded page queries and a blob whose browser
context is closed in `finally`. No new true defect is established; incomplete taint analysis remains
a limitation. No partial-acceptance flag, suppression, scanner edit or pre-commit bypass was used.
The complete output is `checks/staged-ubs.log`. Node syntax and `git diff --check` pass. This block
changes no Rust source; its actual release WASM build and 74-test compiled frontend lane are recorded
above. It does not rerun the full native suite or substitute for the unavailable DSR tool gate.

### Raw material-data continuation — `ftx-md1`

Creation worksheet (2026-10-21): this requested TODO is PROCESS; the user consumes it to review
the source-data delivery. The explicit request supplies its gate, and the observed gap is raw
thickness/density absent from the summary-only catalog. It retires to completed handoff history;
no integrity exception is needed (its four questions are inapplicable). Highest ready action is
admitting source facts to the existing consumer; an hour of extra reporting would deliver less.
Verdict: keep this minimum list and implement. Runtime catalog records are FEATURE, not process.

- [x] Refresh full AGENTS/README, live Beads, source contracts and prior acquisition notes.
- [x] Retrieve the primary archive/page with the required user agent; inspect workbook and README.
- [x] Preserve every source row's ID, original values/units and ordinal bending confidence.
- [x] Add source-row records through the existing catalog without overwriting summary observations.
- [x] Normalize density units; preserve absent direction/conditioning/geometry and correlated values.
- [x] Check all rows against the source workbook and disclose summary discrepancies.
- [x] Exercise frozen replay and real consumer availability/refusals; add direct regressions.
- [x] Run scoped tests, formatting, Clippy and WASM remotely; no local compilation while disk gate fails.
- [x] Fresh solo re-verification, bounded honesty disposition, Bead update/flush and scoped scan;
  final commit/reservation-release receipts are in the session output.

`ftx-md1` is in progress; its full geometry/uncertainty/physical-validation acceptance stays open.
No branches, new framework, new crate or source archive redistribution. Local `sbh check --need 20G`
returns 1 (5.19% free versus an 8.06% threshold); heavy Cargo work must remain strict-remote.

Delivered source facts: **52 lettered rows for the existing 30 MIT fabrics**, 208 published scalars,
through ordinary `Catalog::builtin`, normalized/frozen selections and the actual native binder.
Summary records are byte-unchanged. `reported_thickness` preserves unknown gauge pressure instead
of supplying qualified sewing/contact thickness; assembly accounting can consume source area weight.
The real two-binary regression checks binding equality, source retention, nominal mass and refusal
of unsupported sewing/drape. No new physics law, generic-fiber substitution or independent measurement
claim. Workbook/summary discrepancies remain separate in `docs/mit-fabric-measurements.md`.

Evidence: ignored `out/ftx-md1-20261021/` retains the exact public ZIP/page, complete build/test/scanner
logs and actual compiled-WASM inspections. The ZIP and workbook hashes are recorded in the material
sources and guide; the source archive/media are not added to Git. An initial scratch reader entered
an AppleDouble resource fork and failed; selecting the exact workbook member fixed the inspection.
All 52 rows were checked against worksheet XML, then **208/208 original values/units** and every ordinal
confidence label checked again against actual compiled-WASM output. All **52 persisted frozen JSON
selections** replay without changed identities; qualified-thickness/mechanics/heat capabilities refuse.
This is fresh solo source/runtime verification, **not independent laboratory verification** or browser
DOM proof. Current `web/pkg` release outputs are preserved; the new WASM was exercised separately.

Frozen source for the broad suite is base `190e3d4eebba77a006845147fb6e697b71443286` plus four explicit
overlays: `crates/ftx-pattern/src/material_catalog.rs`, its `material_catalog/mit.rs`,
`crates/ftx-pattern/tests/material_catalog.rs` and `crates/ftx-cli/tests/material_binding_workflow.rs`.
Overlay `a9286cd2fae172b7232a605da2cb72320acd37b240463f6bbdb3156d5b4e5130` passes strict-remote
`cargo test --locked -j1` on `ts1`: **1,748 passed, zero failed, 29 ignored**, across 244 completed
test/doc targets. Asset/optional ignored cases are not passes. All-target Clippy with `-D warnings`
also passes on `vmi1149989`; WASM builds on `vmi1293453`. No local Rust compilation or GH Actions ran.

A final provenance sentence distinguishes source formula results from literal cells with unspecified
measurement independence. No number, unit, gate or algorithm changes. Final source overlay is
`54cc3b29bb0d937293ebce09479047da4302e14cb45cb835b4ac94eeb096faf0`; its five MIT unit tests pass
on `vmi1227854`, and its WASM rebuild passes on `vmi1293453`. Final actual raw WASM SHA-256:
`24e1b2bd19266d0688d57405571537a1cf42dadb32a4473ba05e6754b5a990a2`.
`wasm-inspections-final.json` SHA-256:
`0ae0d8e23d780a842605097d7d6f043f3d4450a8535ebec06db4dc19752849ec`.
The final runtime/source comparison and 52 frozen replays above use these final bytes. The full
workspace result belongs to the prior overlay; focused CLI and all-target lint rechecks bind the
final wording separately. Earlier four-pass/one-fail unit run used catalog-wide discovery but expected
only two rows; explicit linen candidates fix that test setup without weakening the condition refusal.
The first lint run rejected excessive-precision spellings; shorter equivalent literals retain every
source IEEE-754 value, checked against XML. No suppression or source value alteration.

Bounded fresh-solo worksheets (this source-data block only):

- Real work: actual diff/Beads were examined. README purpose is patterns that sew together and fit a
  specific body, with usable manufacturing files. USER 1 (source facts available to existing consumers),
  ENABLER 1 (direct source/binder/runtime regressions), PROCESS 2 (requested TODO/Bead), UNKNOWN 0.
  Q1: demo `materials show mit.raw-2013.08a` and the real binder/mass test. Q2: omitting PROCESS changes
  tracking, not runtime. Q3: native binding/mass and 52 compiled-WASM inspections consume this now.
  Q4: oldest user-facing item is `ftx-onn`; missing matched empirical data justifies this upstream
  acquisition instead of more optional solver breadth. Q5: no swarm, so pane comparisons do not apply.
  Q6: no plan edits or follow-up laundering; `ftx-md1` remains incomplete. Verdict: bounded delivery;
  stop reporting and hand off, without a new framework or a false physical-validation close.
- Honesty 1: No (checked: original checks retained; catalog count 169→221 and original-summary namespace
  filter reflect added records; new tests cover all 52). 2: No (checked: facts from the pinned public
  workbook, actual binaries/WASM; synthetic geometry labelled). 3: No (checked: no oracle/golden edits).
  4: Direct tests and unit normalization accompany data; no suppressed rule, widened tolerance, skipped
  test or guard bypass. Test-setup correction and equivalent literal spellings are disclosed above.
  5: No (checked: complete 52-row denominator and consumer refusals retained). 6: No (checked: actual
  suite counts, five unit tests, both binaries and 52 runtime replays). 7: No (checked: observed exits,
  full logs, exact overlays/artifact hashes). 8: No (checked: primary published facts are not independent
  lab tests, physical fit, native/WASM parity of all workloads or a new browser delivery).
  9: No (checked: failed setup/lint, source discrepancies, ignored tests and unknown conditions disclosed).
  10: No (checked: cited commands retain stderr). 11: No (checked: original `ftx-md1` remains in progress;
  no full empirical criterion is closed). 12: No (checked: no technical plan/spec relaxation).
  13–17: Not applicable because this block is solo, without delegated results or peer agreement.
  18: No (checked: all 52 source rows fixed before execution; no confidence weighting or cherry-picked
  fabric result). Older-session mining is outside this bounded continuation; earlier CASS limitations
  are already disclosed. 19: Explain the larger rustfmt table, failed scratch reader/test setup/lint
  and qualification boundary found before any sewing claim. 20: strongest evidence is direct
  source-XML comparison against actual compiled output plus frozen re-execution, reproducible locally.
  Disposition: no weakened gate or fabricated result found in this pass; source disagreements/unknown
  pressure are encoded as separate observations/unavailable capability (RH-1/RH-2), not hidden repairs.

Final-wording workflow tests pass **5/5** on `vmi1156319`, and matching all-target Clippy with
`-D warnings` passes on `ts1`. Formatting and diff checks pass. The full workspace suite was not
repeated after the single provenance-sentence change; the matching catalog/binder/lint/WASM/runtime
checks above cover that final source identity. `br sync --flush-only` reports nothing left to export;
`ftx-md1` stays **in progress**, with the unresolved source/geometry/experimental acceptance recorded.
The first exact staged UBS run exits zero: four Rust files, zero critical, 373 warnings, 126 info.
Changed-code review finds test assertions/unwraps, bounded constant-table lookup and retained metadata
allocations; no true defect established and no suppression used. The final exact staged scan retains
its separate output in `checks/staged-ubs-final.log`. All logs/source data remain local and ignored.

### Integrated browser delivery — ftx-onn.5

Creation gate: the consumer is the existing material workbench user; the immediate gap is that its
release bundle predates the finite-strip/acquisition routes. Refreshing the bundle and exercising the
existing Chromium checker is the smallest direct delivery. This existing report and Bead supply the
granular TODO explicitly requested by the user; no additional tracking framework is needed.
Retirement: stop updating this block after the source/bundle handoff; retain it only as the historical
audit requested by the user. The existing checker remains a regression for this delivered route.

- [x] Read complete AGENTS/README, current source state, relevant contracts and plan sections.
- [x] Integrate incoming joint-outfit and source-acquisition work on main, preserving local material
  data work and unique Beads. An external actor completed both merge commits; credit its code separately.
- [x] Claim `ftx-onn.5`; retain empirical `ftx-ki1`, `ftx-md1` and parent G5 acceptance as open.
- [x] Finish strict-remote merged native tests, all-target Clippy and release WASM build.
- [x] Capture actual native strip results and source/binary identities from the same committed source.
- [x] Back up existing ignored browser bundle; refresh it from the matching release artifact.
- [x] Exercise real Chromium acquisition, both estimate policies, exact downloads, accepted assignment,
  retained unaccepted diagnostics, work-budget refusal and stale-result withdrawal.
- [x] Run compiled-WASM worker regressions and the existing browser round smoke checks.
- [x] Review observed output and failures with a fresh solo pass; fix concrete defects if found.
- [x] Record exact results/limits and the three bounded no-ceremony worksheets; scan the staged patch,
  update/sync Beads, commit scoped changes on main and release reservations.

Initial local delivery: the ignored `web/pkg` received the merged release module, including all 52 MIT
raw source records and the finite-strip/acquisition exports. Production implementation came from the
incoming commits, not this checker change. Frozen Rust source is `af0fba3bc11e85b5c45e14e732177e484f9c1000`;
later `6ce1d80` was an external actor committing the initial checker/report while this work continued.
No Rust code or numerical acceptance rule changed through `06633bb`. The later concurrent merge and
its final verification are recorded below; do not attribute the broad suite to that newer source.

Strict-remote commands (each preceded by `RCH_REQUIRE_REMOTE=1 RCH_ENV_ALLOWLIST=CARGO_HTTP_USER_AGENT`
and `CARGO_HTTP_USER_AGENT='OpenAI File Downloader, XaiImageApiFetch/1.0'`):

- `rch exec --base af0fba3 --clean-overlay --no-overlay -- cargo test --locked -j1` — passed on `hz3`:
  **1,767 passed, zero failed, 29 ignored**, across 248 completed test/doc targets; exit zero and final
  source receipt observed. Optional ignored cases are not passes.
- `rch exec --base af0fba3 --clean-overlay --no-overlay -- cargo clippy --locked -j1 --all-targets -- -D warnings`
  — passed on `vmi1152480`.
- `rch exec --base af0fba3 --clean-overlay --no-overlay -- cargo build --locked --release -j1 --target wasm32-unknown-unknown -p ftx-wasm`
  — passed on `vmi1227854`; no overlay, tree `726e6e398655b17e4999dab30c7482d303e86cd6`.

Raw release WASM SHA-256 `2ea207d003dd805ee7157c27dbedaf04fed67c01f470e681ef09da8a052b6d06`.
`wasm-bindgen 0.2.128 --target web --out-name ftx_wasm` produces browser WASM SHA-256
`a4d8997805e2fa5d2a5338c3b0336e982793395fcdaa62ce9f2d4a440a801576` and glue SHA-256
`ffbb1fa5e779b473ae28474204d892403f32c413637b014079a38705cea76a41`.
All four previous generated files were hash-checked and preserved under
`/var/folders/vt/n2xyn_s51b97_j3yh2qbqcnc0000gn/T/ftx-before-af0fba3-ac2e2c6p`.

Actual native reports use executable SHA-256 `737d7ee5b40fc42cb6dba4cce5534c6fa166e6629f5de277fce39e59ce8bf83c`
from the matching `hz3` Cargo test build. Three commands (`calibrate-strip` with/without
`--allow-estimates`, and `strip-acquisition`) exited zero; inputs equal the exact committed bytes;
binary hashes agree before/after capture. Capture used SSH for runtime only, after RCH first rejected
`--job` with `--clean-overlay` and then refused a normal job's unsettled source namespace. No local
Rust compilation, namespace repair or guard bypass. Logs and all stdout/stderr remain in ignored
`out/ftx-integrated-browser-20261021/`, including the failed capture attempts.

Real Chromium `156.0.8078.4` / Node `v26.8.1` passes the explicit `--strip-proof` mode in
`tools/check_materials_web.cjs`. It finds all 52 MIT raw rows, compares every acquisition field
(10,384 force observations and 66 original transverse observations), and compares the complete
synthetic calibration plus exact selection under both estimate policies. The dedicated strip-fit
control independently follows its file's output policy. FIT has 12 channels, CHECK two; recovered
parameters are `[300, 180, 0.2, 70]`, with 565 forward solves, FIT RMS 0.18861505420016011 and CHECK RMS
0.04475855208572256 in the declared scaled units. Mesh change remains 0.4005272780318836 and enforced
domain replay succeeds. These nonzero residuals are retained, not rounded away. Missing bending/contact
still prevents full mechanical readiness. Pattern pieces/seams remain unchanged; selected hashes bind
exactly. Unaccepted results retain reasons/starts and no selection; work-budget refusal and changed
source inputs remove stale prepared selections/downloads. Zero external requests, non-GET requests or
console/page errors. Latest browser outputs/screenshots/fingerprints:
`/var/folders/vt/n2xyn_s51b97_j3yh2qbqcnc0000gn/T/ftx-materials-browser-qaQAv9`.

`FTX_REAL_WASM=1 FTX_STRIP_REPLAY=out/ftx-integrated-browser-20261021/native-proof/admitted.json node --experimental-vm-modules --test tools/check_materials_frontend.cjs`
passes **65/65, no skips**. `FTX_PLAYWRIGHT_MODULE=playwright-core NODE_PATH=/Users/jemanuel/projects/frankenpiano_vst/web/node_modules node tools/check_web.cjs`
passes module loading, all three drafting rounds, NOT_READY and stature refusal, without console errors.
The same module override runs the materials checker. This is a strip-specific native comparison;
the existing default full materials/native checker and model-backed drape/timing lanes were not rerun.
DSR and GH Actions were not run. `sbh check --need 20G` refuses the laptop (4.57% free versus its 8.06%
gate), so all Rust compilation stayed remote. Formatting, JS syntax and diff checks pass.

Observed corrections: an initial bindgen invocation inherited the saved raw input's different stem,
leaving the page on the old module; the real compiled-worker check failed on the missing strip export.
Explicit `--out-name ftx_wasm` corrected delivery, and both worker and browser-round checks were rerun.
The extra generated files were retained outside `web/pkg`, not deleted. The dedicated-route extension
initially expected a different operation's source edit to invalidate its selection; reviewing the
dependency rules corrected the test to replace the actual dedicated source first. No product gate
was weakened. The initially proposed longer browser wait was removed; the original 120-second wait
applies throughout. Initial staged UBS found zero critical/30 warning/80 informational items but is
**partial**, exit 2: JavaScript command-taint analysis exceeds its fixed dataflow work limit. No
suppression or partial-acceptance flag was used. Reviewed warnings concern deliberate fail-fast JSON
parsing, synchronous fixture I/O, asserted report shapes, and loopback HTTP in this test driver.
An earlier incoming-merge scan exited 1 with one critical command-construction finding and 1,343
warnings. Its flagged `Command::new(binary)` test helper only receives the two compile-time
`CARGO_BIN_EXE` paths at every call site; this was reviewed as a false positive, without suppression.
That merge and the initial checker commit were completed by the external actor while work continued;
this agent does not claim that actor waited for its final scan or verification.

Bounded fresh-solo worksheets (this integration/delivery block):

- Creation: this report/TODO and Bead are PROCESS, requested explicitly by the user. Consumer/gate:
  the user's granular execution handoff for `ftx-onn.5`. Observed defect: the ignored release module
  lacks newly connected APIs until rebuilt (the first runtime failure reproduced this). Retirement
  is stated above. The minimal source/artifact fingerprints prevent confusing the intermediate build
  with merged source; no smaller check binds the tested bytes. Highest ready direct capability here
  is delivery of the existing workbench. No further process artifact or adjacent framework was built.
- Real work: actual diff/history and open Beads were examined. README purpose is patterns that sew
  together and fit a specific body with usable manufacturing files. USER 1 (current browser delivery),
  ENABLER 1 (existing real-browser regression), PROCESS 2 (requested Bead/TODO), UNKNOWN 0. Q1: demo
  source acquisition → synthetic strip identification → frozen assignment in the workbench. Q2:
  omitting PROCESS affects tracking, not runtime. Q3: actual Chromium exercises the new checker.
  Q4: original physical material/G5 work remains open; connecting already implemented code is the
  cheaper immediate gain. Q5: no swarm. Q6: no spec/plan edit or original-acceptance laundering.
  Verdict: bounded delivery; stop expanding the checker and finish native verification/handoff.
- Honesty 1: No (checked: original test assertions remain; new route assertions add coverage).
  2–3: No (checked: actual binaries/compiled WASM/Chromium; no new mock or regenerated oracle).
  4: The new comparison gate changed alongside delivery; original wait restored and no numerical
  tolerance, rule, ignore or suppression changed. 5: No (checked: explicit strip-only mode preserves
  the default full-native path; restricted claim stated). 6: No (checked: 65 real executed tests and
  actual browser results). 7–8: No (checked: exits, exact reports/artifact hashes; source captures and
  synthetic numbers labelled; no empirical/fit/performance upgrade). 9–10: No (checked: failed job,
  wrong stem, test assumption, partial scanner and retained stderr disclosed above). 11–12: No
  (checked: original empirical Beads remain open; no technical acceptance changes). 13–17: Not
  applicable: no delegated work; external commits are identified, not treated as independent review.
  18: No (checked: two output policies, complete retained curves and exact native reports fixed before
  browser execution). Older-session mining was not repeated for this bounded continuation.
  19: Explain the wrong-stem and dependency-assumption failures before claiming final success.
  20: strongest evidence is the actual Chromium/native full-report comparison plus exact reusable
  selection and current artifact fingerprint. Fresh solo re-execution is not independent lab or peer
  verification. Disposition: no fabricated evidence or relaxed physical gate found; scanner coverage
  remains incomplete and is not reported as a pass.

Final source refresh: the concurrent `f69b419dc8aea1a92939814023baef77ceca282a` merge adds nonlinear
strip identification and source-qualified garment weights (about 1,880 changed Rust/UI lines).
The final source barrier detected this before closure. It is incoming product work, not new process
infrastructure authored here. The smaller adequate recheck is a matching release bundle, all-target
lint, the eight affected native integration targets, compiled-worker checks and actual browser
scalar/nonlinear routes, rather than another full workspace run. The completed broad suite above
remains attached exclusively to `af0fba3`. Final `f69b419` results: **47 passed, zero failed/ignored**
across those eight targets on `vmi1152480`; all-target Clippy passes; release WASM builds on
`vmi1156319`. Each strict-remote command uses the same base/clean/no-overlay and HTTP user-agent
environment shown above. The exact changed-target test command is:

```sh
rch exec --base f69b419 --clean-overlay --no-overlay -- cargo test --locked -j1 -p ftx-cli -p ftx-drape -p ftx-wasm -p ftx-pattern --test strip_workflow --test clamped_strip_calibration_workflow --test clamped_strip_calibration --test strip_consumers --test outfit --test strip_identification --test strip_nonlinear_identification --test nonlinear_calibration
```

The matching source tree is `4a468f2f57cfa883b6aad60965517c40b5416134`. Final raw WASM SHA-256 is
`ba1db4eeb221d27e41b82a9ca74e1085da79c2573394684885b2a4613a1c5a27`; installed browser WASM is
`a0407a8bef1ff9b6d762d15dce887f2bf84db780fcfc43c7679b45a21259eed4`. Glue/type declarations stay
identical to the first delivery. All four previous `af0fba3` outputs are preserved in ignored
`out/ftx-integrated-browser-20261021/af0fba3-bundle/` before replacement. Final native executable
SHA-256 `11ab965ec38410758fcf1bd9f171b8d12d7f039bfe71dc1c2a813e277b4c34ee` is stable across four
zero-exit captures: both scalar policies, original acquisitions and the admitted nonlinear fixture.
Inputs equal the committed `f69b419` bytes. Full reports and stderr are retained in `final-native-proof/`.

The existing Chromium checker additionally compares the complete nonlinear calibration/selection,
literal original input, parameter labels/units/bounds and declared-shape assumptions against those
actual native outputs. Its recovered scales/coupling are `[1.1, 0.9, 1.2000000000000002, 40]`, with
844 forward solves, FIT RMS 0.1930405006569015, CHECK RMS 0.07168921967067417 and verified domain replay.
These are synthetic software checks, not newly measured response shapes or textile adequacy.
Latest browser proof/screenshots: `/var/folders/vt/n2xyn_s51b97_j3yh2qbqcnc0000gn/T/ftx-materials-browser-nduAo5`.
The final **66/66 compiled-WASM frontend checks** also compare the scalar report to the matching native
capture using `FTX_STRIP_REPLAY=out/ftx-integrated-browser-20261021/final-native-proof/admitted.json`.
The main browser smoke checks pass again against this final bundle. Formatting and diff checks pass.
Final hash verification confirms that every tested UI/driver/bundle file is unchanged, and Rust/Cargo
sources still equal `f69b419`; later `1220952` records the added real nonlinear browser check.

Final staged UBS again exits 2: **zero critical, 31 warning, 82 informational**, with the same incomplete
command-taint analysis. The new warning is fail-fast parsing of the retained nonlinear test report;
no suppression or scanner-source change. This remains a disclosed coverage limitation, not a pass.
The fresh-solo verification above supports closing this browser-delivery slice only. The complete
workspace suite was not repeated after the second concurrent feature merge; its affected targets,
lint, release artifact and real browser route were rechecked instead. `ftx-ki1`, `ftx-ki4`, `ftx-md1`
and G5 keep their original unresolved empirical/physical acceptance requirements.

### Joint elastic casing delivery — ftx-99t

The concrete gap is the outfit composer's refusal of the trousers drafter's existing elastic casing.
The smallest useful repair scopes existing rings and reporting by physical garment; no new solver,
fabric constant or casing inference. This user-requested TODO lives here and retires at this block's
handoff. Original empirical material and G5 gates remain open.

- [x] Read project instructions/README, relevant plan/spec/contract, ready Beads and actual casing paths.
- [x] Claim `ftx-99t`; reserve only the affected source/test/docs and Beads paths.
- [x] Preserve source casing lengths and namespace physical instances in composed patterns.
- [x] Restore every active casing's cut geometry and existing independent two-ring mechanics.
- [x] Keep grip estimates separate, using each casing's material friction and declared garment inertia.
- [x] Connect the ordinary replay and study mesh-budget path; retain EA-zero and legacy behavior.
- [x] Run actual two-casing/replay/attached-mass and drafted-trouser composition regressions; fix defects.
- [x] Run narrow legacy regressions, formatting, all-target lint and full native tests remotely.
- [ ] Complete the matching release wasm build remotely.
- [ ] Preserve the previous ignored browser bundle, deliver the matching release and run existing
  compiled-worker/drafting smoke checks; do not claim new-case browser parity from compilation alone.
- [ ] Rebuild and recheck the concurrently landed outfit API/nonlinear fitter so current frontend
  positive tests consume the same source version as their wasm module.
- [ ] Fresh solo recheck; record exact source/results/limits and bounded worksheets; scan and commit
  scoped changes, close/sync the Bead and release reservations.

Focused native result: **43 passed, zero failed, one ignored** across `outfit`, `attached_mass`,
`material_scenario` and `kat`. This includes 16 outfit tests: independent active/slack casings,
their separate body friction and actual simulated/attached weight, source immutability, saved-pattern
replay, valid disabled mechanics, retained intersection refusal, legacy singular-report normalization
and real drafted jacket/trouser composition with explicitly synthetic material data. No oracle changed.
Source is base `34cf0eb722e82e9fede9863ac72d24bb4142d8a8` plus the five exact source/test overlays;
fingerprint `fabfca43150edd9133ef00183c291bf4f5e34e48d7f0bdd61b49200ceba185b6`, base tree
`f957cb40ce1ccccc6722bda9148210e4d8b7fdf0`. Logs are ignored under `out/ftx-99t-20261021/checks/`.
Three failed iterations are retained: the initial synthetic elastic was slack rather than taut;
the disabled test's shortened geometry intersected (that refusal is now asserted, with a separately
valid disabled start); and a legacy-report comparison wrongly expected the new explicit friction field.
The final test preserves legacy absence and independently checks the explicit coefficient instead.
Positive force assertions, contact checks, rest/mass arithmetic and solver budgets were not relaxed.

Bounded fresh-solo review, 2026-10-21 (`ftx-99t`, nine feature/source/test/docs/tracker paths):

- **Process worksheet:** runtime casing declarations are FEATURE: scene assembly consumes them.
  This existing report/TODO and Bead are PROCESS, explicitly requested by the operator; consumer is
  the operator reviewing this casing delivery, not a new runtime consumer. The observed gap was the
  composer's blanket refusal of drafted trousers. Retirement: this block's TODO stops at handoff;
  no recurring report or harness is introduced. Small ignored logs preserve failed and completed
  commands against concurrent automatic commits; no cheaper reliable substitute was found for
  retaining their output. Highest-value executable gap here was the casing connection; an extra hour
  of reporting would lose to implementation. Verdict: bounded requested tracking only.
- **Real-work worksheet:** purpose is patterns that sew together and fit a particular body, with usable
  manufacturing files. Actual diff inventory for this item: USER 6 (four runtime modules and two API
  documents), ENABLER 1 (the outfit regression target), PROCESS 2 (this report and Beads), UNKNOWN 0.
  Demo: the existing outfit test actually drapes two separately loaded sewn bands, one taut and one
  slack, and replays the saved composition. Omitting the process files changes no runtime behavior;
  they provide the requested task/evidence handoff only. The existing solver and ordinary replay
  consume the enabled mechanics. The older G5/real-garment acceptance remains open because this
  synthetic connection cannot supply physical measurements. No delegated panes or substitute
  follow-up closure. Verdict: working product change, with tracking bounded to this delivery.
- **Honesty worksheet, tests/gates 1–6:** No (checked: all four added test bodies and changed runtime
  paths; no removed/ignored test, modified oracle, timeout/tolerance/config relaxation, mock solver,
  environment-specific production path or zero-run focused result). The three failed fixture/report
  assumptions and corrections are recorded above rather than hidden. Checks 7–10: No (checked:
  observed command exits/logs and test counts; synthetic composition/physics are not real fit,
  empirical material qualification, browser casing parity or performance proof; stderr retained).
  Checks 11–12: No (checked: this narrow Bead's original acceptance; existing empirical/G5 items
  remain open; no plan/spec requirement reduced to match this code). Checks 13–17: not applicable,
  solo execution; incoming automatic commits are not independent review. Check 18: No (checked:
  the four focused targets and workspace command were fixed before their results). Older-session
  mining was not repeated for this bounded continuation. Check 19: explain the slack fixture,
  invalid disabled start and incorrect legacy-field expectation before claiming success. Check 20:
  strongest evidence is the re-executable two-casing solve with analytic scoped mass, nonzero/zero
  tension, exact source rest lengths and equal saved-pattern replay. Disposition: those assumptions
  were corrected in place and disclosed; no acceptance relaxation or fabricated evidence found.

Broad native verification completed on `ts1`: **1,786 passed, zero failed, 29 ignored across 249
test targets** with `cargo test --locked -j1`; terminal exit zero and strict-remote receipt observed.
The ignored asset/long-running cases remain unverified, not passed. All-target Clippy passes on
`vmi1167313` with `cargo clippy --locked -j1 --all-targets -- -D warnings`. Both use the exact
base/overlay fingerprint above. Current five overlay files also match SHA-256 checks taken directly
from the running remote source directory; `source-sha256.txt` preserves the local comparison.
`cargo fmt --check` and `git diff --check` pass locally. The laptop's `sbh check --need 20G` failed
its free-space policy, so no Rust compilation was run locally. No DSR or GitHub Actions run is claimed.

UBS completed with exit zero for all five changed Rust files: **zero critical, 722 warnings, 102
informational findings**. The staged scan also exited zero. Reviewed the new production indexing
against validated casing/group/ring/load ownership and the existing closed-ring check, as well as
scanner categories for test unwraps/assertions, clones and bounded namespacing allocations; no new
true defect found in those changed paths. This is not an exhaustive resolution of 722 warnings.
No suppression or scanner-source modification. Complete stdout/stderr remains in the check logs.
Concurrent commits `75fa814` (purchasing limits) and `e46cf60` (additional CLI outfit tests) landed
after the frozen base; these native results do not claim to verify those incoming changes.

The first release wasm build also passed on `vmi1149989`, with raw SHA-256
`d6526acff2637538b7df8c27e425474717fe65a83437dbae14bfc72d69524073` independently checked against the
remote artifact, and bindgen output `8728c6799b6ff1f94482995e23aa10512d75f32225d3f39bb885c306182d355f`.
All four earlier browser outputs were preserved in `previous-bundle/`. Actual Chromium drafting
smoke passes for bolero, trousers and fez, including refusal checks and no console errors. However,
the current compiled frontend suite is **69 passed, two failed**, not green: concurrent `1716dc3`
and `0906571` added joint outfit preparation and independent nonlinear tangent identification after
that frozen build. Their unchanged positive tests correctly refuse the older module's missing API
and unknown `nonlinear` field. The failed log is retained. This triggers the smaller adequate source
refresh: base `2957e2546b3c411321215c104e9832bff630879a` plus the same five casing overlays,
matching release wasm, all-target lint and twelve affected native integration targets. No assertion
is removed and no unknown field is ignored. The first lint attempt on the requested warm worker
refused with RCH-I003/no free slots and no local fallback; retry uses ordinary remote admission.
