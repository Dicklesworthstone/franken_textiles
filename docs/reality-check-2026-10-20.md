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
- [x] Register `ftx-6en` (P0, in progress), and `ftx-dzf` (P1, now closed), with existing parent gates.
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
- [x] R2: direct exhaustion/rollback/replay, valid quiet window, oscillatory cancellation and
  qualification tests: **112 drape library tests passed remotely** on `vmi1227854`.
- [ ] R2: verify the actual material-to-drape diagnostic/gating path and failed-phase frame/ramp
  rollback regression; expose numerical status in the direct CLI JSON and human summary.
- [ ] R2/R3: complete workspace tests, all-target clippy and wasm compilation remotely, and fresh
  review against the original acceptance conditions. Keep `ftx-6en` open until the evidence suffices.
- [x] R8: correct the 21-tape description, frozen `outfit --sim` command, and numerical/parity
  boundaries in README and touched contracts (`60ae9b4`). Full scope/budget review remains open.
- [ ] R3: run production-Anny fitting and fitted garment gates with the converted pinned asset;
  ordinary synthetic/debug tests cannot substitute for these or the release-only contact tests.
- [ ] R4: complete production-model capture-to-body acceptance, then private photo/tape comparisons.
- [ ] R5: acquire matched specimens/raw observations and execute held-out coupons and fixed-jacket
  swaps (`ftx-tm2`, `ftx-md1`, restored `ftx-ms3`, `ftx-tm8`, `ftx-tmf`).
- [ ] R6: continue source-qualified thermal/process/purchasing slices through their existing tasks;
  restored `ftx-ms2` remains open. No new coefficients or physical acceptance are inferred here.
- [ ] R9/R10: current hardware/input-bound benchmarks, native/wasm/browser comparisons, private
  muslin evidence and the existing `ftx-onn` release gate remain required.
- [ ] Finalize fresh self-review, Beads evidence/flush and reservation release; report any failed or
  unavailable checks separately from passed checks.

Raw command output for this block is retained outside the repository at
`/var/folders/vt/n2xyn_s51b97_j3yh2qbqcnc0000gn/T/ftx-followthrough-20261020T214254Z/`.
Remote checks use `RCH_REQUIRE_REMOTE=1`; the laptop's `sbh check --need 20G` failed its free-space-ratio
gate, so no heavy local compilation was substituted. This is not a current garment benchmark or G2 pass.
