# ftx-export

**Treatment-aware design evaluation** (`material_study::design::treatment`, `ftx-care1.3`).
The canonical `treatment` operation accepts an ordinary pattern-treatment request in authored
designs and grid/adaptive `after` operations. Final geometry, seam, outline and selected-body fit
admission see the resulting dimensions before any material/manufacturing/scenario consumer.
Completed operations retain exact canonical intermediate reports; later refusal does not admit
those partial results. Final binding preserves each piece's last promised BEFORE/AFTER material
hash while allowing unrelated piece substitutions. Zero-dimensional property changes do not claim
changed geometry. Existing altered-pattern plus frozen-change replay is unchanged; treatment
receipts are omitted when absent, and fixed-geometry schemas still reject design operations.
Repeated geometry, per-piece record growth and retained report work are reserved across screened
and adaptive attempts. Tests: `design_treatment`, existing `design_study`, and both native commands
through `material_treatment_workflow`. Free coupon treatment does not establish sewn garment fit.

**Selected-body design synthesis** (`material_study::design`, plan §25.70 / `ftx-ds2`).
Optional finite variable grids generate linked canonical alterations and retain baseline, exact
coordinates/operations and incomplete-domain receipts. Both grids and adaptive searches accept original
slash sites and explicitly tagged vertex-move sites with signed x/y scalar multipliers. Mixed sites
produce ordinary operations in authored order; canonical rounding, curve/mark behavior and all flat
gates remain authoritative. Complete domains and actual adaptive displacements reject invalid or
overflowed/underflowed changes before consumers; failed candidates admit no partial altered document.
Explicit flat fit bands use a borrowed live
`BodyMeasure` context from the production adapter; the export crate never fits a body or guesses
landmarks. Canonical raw status gates binding and every consumer before comparison. Context identity
is part of manufacturing/scenario/matrix hashes. A missing body, point or landmark cannot become a
zero residual or implicit fit. Grid, fit-spec, identity, measurement and expanded consumer work are
bounded; a prefix frontier is conditional on visited designs. Existing authored/fixed studies keep
their behavior when these fields are absent. Numerical target admission is not empirical wearer fit,
safe strain/pressure qualification, a continuous optimum or machine authorization. Tests:
`design_study`, design fit/grid unit tests, and native/portable accepted-body workflow.

**Fit-directed grid screening** (`design_grid.fit_screening`). An explicit 1–256 candidate
allowance inspects the authored grid prefix once using the same alteration, flat, outline and
selected-body fit authorities. It requires a live fit context and explicit target contract, retains
every inspected geometry/fit/refusal receipt, and passes the first `max_designs` qualified designs
plus the original baseline to ordinary material, manufacturing and scenario consumers. Cached
canonical results prevent repeated flat work across materials or named cases. Complete screening,
qualified-but-unselected designs and unvisited points remain separate; no downstream result is
inferred for omitted candidates. Source/document/root-search, retained identity, measurement and
outline work are reserved before screening; ordinary expanded consumer limits remain in force.
Absent screening preserves the earlier prefix request/output semantics and budgets. Tests in
`design_study` cover a feasible candidate beyond the old prefix, exact marker replay, single versus
matrix evaluation, missing context, budget exhaustion and retained failures.

**Adaptive selected-body design discovery** (`design_adaptive`). Explicit finite bounds, initial
coordinates, polling/minimum steps and a 1–256 unique-trial allowance refine linked slash-spread and move
variables between authored grid values. Canonical unrounded point measurements minimize the
declared maximum out-of-band distance, then its sum; this discovery objective is separate from
production ranking. Full geometry, seam, truing, completeness, outline and selected-body gates
admit only zero-violation trials. Every trial retains exact operations, altered source, body
identity, raw readings and refusals; selected geometries are cached across materials/cases and
replay through ordinary consumers. Whole-request work is reserved before search. Evaluation and
minimum-step limits retain feasible incumbents without claiming continuous-domain completeness,
global optimality, empirical fit, or unavailable uncertainty. Tests: `design_study/adaptive`,
canonical `validate::raw_point_tests`, and native/portable accepted-body adaptive workflow.

**Consumer-guided adaptive design** (`design_adaptive.objective: consumer_comparison`, `ftx-ds2`).
One exact nominated `material_key` and one existing nominal comparison objective guide production
polling after the same canonical geometry/body-fit admission. Ordinary comparison constraints,
required consumers, source/protocol checks and dominance remain the only numerical authority;
missing or failed consumers cannot become zero scores. Polling continues after the retention
allowance fills, replaces worse incumbents, and retains every trial's comparison/exclusions plus
original binding/callback refusals. The first admitted protocol anchors ordering unless explicit
mixed-protocol permission applies. Only the best bounded set of full production outputs is cached;
final retained geometry crosses all material alternatives and reuses the nominated material's exact
manufacturing/scenario output. Probe-plus-final slots, repeated source documents and ordinary
manufacturing aggregate counts are reserved up front; mass/outline/station counters and the caller's
scenario Session remain shared. A slot count is not a promise that each physical simulation fits
its per-callback work reservation. Search/step limits and later callback refusals preserve successful
incumbents; structural/global manufacturing hard-budget errors still refuse the complete request.
Manufacturing and single scenarios are supported; matrices, multiple objectives, reported-bounds
guidance and unavailable nominees refuse before production callbacks. Optional receipt fields keep
fit-only serialization unchanged. No new surrogate score, physical law, empirical validation or
continuous/global optimality claim. Tests: real mass/constraint/source/reservation checks in
`design_study/adaptive` and real selected-body sewn-scenario guidance/cache replay through both CLIs
and the portable authority in `accepted_body_workflow/consumer_adaptive`.

**Moving zero-storage bridge decisions** (`motion.transient_coverage.thermal_bridges`). Existing
incoming-coverage energy objectives consume the complete signed baseline-plus-excess result. A
separate temporal qualification binds the exact steady excess observation, actual elapsed/sample
time, reservoir films and live material-temperature domain. Admission retains the export crate's
independence from drape solvers: it reconciles production source hashes, accepted edge lengths and
complete edge partitions, selected stack/temporal qualifications, per-body powers and every signed
interval/cumulative energy. Late, absent, injected or changed bridge data excludes that heat
consumer, without discarding independent manufacturing or mechanics. Equal bridge boundary terms
never enter physical storage; source/storage balance uses the retained baseline sides so rounding
of large cancelling totals cannot invent or erase stored heat. Consumed source protocols enter the
existing mixed-protocol gate. Core ownership/projection remains in ftx-drape; no second physical
solver, bridge heat capacity or empirical seam/garment performance is inferred. Tests: direct live
temperature/length/time/conservation/budget regressions and actual selected-body single/matrix
native/portable replay with thermal-frontier change and tampered-receipt exclusions.

**Finite-water scenario decisions.** The optional `motion_wet_film` comparison consumer reads
evaporated, remaining, unmet-demand and supplied mass plus prescribed-body latent energy from a
complete admitted moving-coverage history. It verifies exact authored body-bound liquid/supply,
canonical regional demand, per-step and cumulative conservation, source protocols and retention
limits. Nominal producer histories remain unchanged. For `basis: "reported_bounds"`, the comparator
replays complete canonical Ret demand endpoints through the same finite-water state authority to
derive conditional final cumulative ranges: evaporation, unmet demand and latent energy follow
low-to-high demand, while remaining liquid reverses that endpoint order. Supplied water has a fixed
singleton range and needs no Ret bounds, but still requires complete nominal history admission.
Missing Ret endpoints exclude the other bounded water metrics while preserving nominal readings.
Geometry, boundaries, initial liquid, supply, latent heat and integration stay fixed; these final
comparison ranges do not establish per-interval water bounds, general water-state uncertainty or
confidence intervals. The consumer shares the ordinary single-case and matrix decision rules and
preserves independent outputs on refusal. It introduces no wet-textile law or textile-temperature feedback.

**Purpose.** Every deliverable of a pattern as a projection of one model (spec 03). So far: the text and
number helpers (§2), the seam-allowance cut outline (§3–4), piece geometry (§5), shelf layouts (§6), the
master SVG (§7.1), the PDFs (§7.2–7.6), the DXFs (§7.7–7.8), HPGL (§7.9), the cutter files (§7.10),
GarmentCode (§7.11), the soutache DST files (§7.12), the manifest (§7.13), every acceptance check (§8) and the
orchestration of both (`export_all`, `check_files`). The `ftx export` / `ftx check` commands are FTX-18.

**Units and frames.** Millimetres, the pattern's own frame (x right, y up, loops counter-clockwise).

**Sewing setup order** (`sewing::ordering`, plan §25.4 / `ftx-tm7`). Optional
`SewOptions.ordering` minimizes the explicit sum of exact needle and thread identity
changes across an already fully admitted job, preserving every declared predecessor
and every complete operation's geometry, recipe, source qualification and feed time.
An optional initial setup is counted; absent initial installation and a final reset
are outside the objective. At most 20 passes and 200000 examined feasible prefix
extensions are admitted; iterative search holds at most 20 bounded frames. The normal
lexicographic topological schedule is the incumbent, equal counts retain it, and
trials are ordered by immediate count then operation ID. A limit returns a complete
feasible incumbent with explicit budget/proof status, never a partial schedule or an
unearned optimum claim. Count bounds ignore prerequisites only to underestimate the
remaining changes. Completed proof is conditional on declared identities/prerequisites,
not physical handling, seam accessibility, elapsed production time or hardware safety.
Omitted policy preserves legacy wire output; requested policy uses `ftx-sewing-plan/6`
through ordinary plan, stitch-program and study consumers. Tests: `sewing` and
`sewing_program`, including independent exhaustive schedule comparisons.

**Cross-copy sewing production** (`sewing::production`, `ftx-pb2`). Optional
`SewOptions.production` expands each already admitted operation and its predecessors within each
physical garment copy, then reuses the same count/directed-time scheduler over at most 63 operations
and 200000 examined prefixes. Explicit production and thread quantities must agree; studies also
resolve their common cutting count before admission. The effective quantity is frozen into options.
Source station geometry stays once in `seams`; `production.operations` retains copy/pass/qualification
identity and scoped dependencies. Initial setup is charged once across the batch, and millisecond
totals use u64 on native and WASM. Per-garment plan totals remain unchanged; shared total/order methods
select the explicit batch scope for consumers. The version-4 stitch program emits every selected
copy's real runs, charging the complete 250000-interval cap before expansion. Missing timing or stale
material qualification refuses; a search stop preserves a complete incumbent with no optimality claim.
Existing absent-policy output is unchanged. This single-configuration slice infers no undeclared
fixture changes, handling, seam access, machine execution or physical quality. Tests:
`sewing_production`, existing `sewing` and `sewing_program`; see `docs/sewing-production.md`.

**Finite workholding** (`production.workholding`). Optional source-qualified resources and per-copy
holds constrain the same batch optimizer. Full typed machine and admitted qualification hashes
bind resource applicability to the exact configuration, material stack and process; estimate consent
is independent. Acquisition precedes operation start, release follows completion, and same-operation
holds contribute to during/peak occupancy. Distinct anchors add real prerequisites; cycles refuse.
An infeasible source-order seed triggers bounded discovery of a complete resource-feasible baseline
before pruning or objective shortcuts. Discovery and optimization share one node cap; the first
complete replacement is retained with optional `baseline_replaced: true`. Budget exhaustion without
a complete schedule is `NotConverged`; only exhaustive search establishes declared-model infeasibility.
The selected stitch program uses the constrained order. Plan `/13`, production `/2` and workholding
`/1` retain expanded holds, exact source locks, full baseline/selected timelines and finite peaks.
Bounds are 16 resources, 64 source holds, 1024 expanded holds and 256 qualification references, with
integer capacities/demands in 1..4096. Omission preserves prior wire output. Fixture geometry, seam
access and hardware authorization remain unvalidated. Tests include exhaustive small count/time
optima, lifecycle boundaries, multiple resources, source invalidation, unresolved selector claims,
bounded incumbents, stitch replay and native/portable capacity-dependent schedules.

**Serial garment handling** (`production.handling`). Independent full machine/qualification locks
admit explicit first loading, directed same-copy/different-copy operation transitions and final
unloading. `minimize_production_time` adds these serial nonoverlapping durations to ordinary setup
and feed, with complete finite workholding/access admission preserved. Integer lower bounds include
the final unload; a work limit retains a complete source-bound incumbent. Plan `/15`, production
`/3` and handling `/1` preserve every baseline/selected scoped transition. Setup/feed/handling totals
stay separate, and joint proof never becomes setup-only optimality. Missing handling evidence is
`NotConverged`, keeping recipe selection unresolved. Selection `/4` minimizes the actual complete
modeled total across explicit qualified recipes. Limits remain 63 expanded operations/200000 search
nodes, with at most 8192 timing rows/256 qualification locks and one-day per-phase durations. Tests
in `sewing_production` compare independent exhaustive optima under finite fixtures, final unloading,
source changes, complete bounded replay and a handling-dependent recipe winner. No physical timing,
parallel resource execution or hardware qualification is inferred.

**Independent parallel cells** (`production.parallel`). An explicit source qualification binds
distinct physical machine instances, independent fixtures and handling labor to the same complete
machine/process configuration. Whole garments stay in one cell. Every cell/count trial reuses
ordinary serial scheduling, handling and fixture admission; every cell policy is checked even if
the selected allocation leaves it idle. Exact dynamic programming minimizes the maximum finish and
separately propagates valid inner bounds. Proof requires the complete incumbent to attain that
global bound; bounded inner searches retain all operations and their exact source identities.
The required `ceil_cell_feed_milliseconds/1` policy ceiling-schedules generated feed once per used
cell and retains raw feed and sub-millisecond padding. This is an explicit planning-resolution
objective. Setup/handling work is summed separately from elapsed makespan. Unused cells incur no
availability/setup/loading/unloading costs. The outer serial-reference cap plus one global parallel
cap (all cell/count searches and both DPs) is at most 200000; at most eight cells and 63 expanded
operations are supported. `SewOptions::scheduling_work_bound` reserves the complete cross-product
work. Plan `/16`, production `/4`, parallel `/1` and stitch `/5` retain complete per-cell schedules
with local-to-global physical-copy maps; every output stitch identifies its cell/global copy.
`ordering_report` withholds the serial reference from actual parallel scoring. Synthetic independent
exhaustive, budget, fixture, source and stitch cases live in `sewing_parallel`. This does not qualify
shared workers, cross-cell transfers, arbitrary configurations, grip or hardware execution.

**Configured independent cells** (`independent_whole_garment_cells/2`). Each physical cell supplies
a full machine/configuration hash, its own complete process qualifications/content references and
explicit optional operation recipe overrides. Omitted recipe settings inherit the current parent
candidate; source pattern/topology/assembly/paths/backtacks stay fixed. Optional per-operation access
replacements retain independent machine/source admission and never erase inherited requirements.
Each cell builds one fresh ordinary plan on the actual pattern, including idle cells; actual station
pitch, traversal count, feed, materials, pressure and clearance are rechecked. Existing count DP and
valid inner bounds operate on these cell-specific costs. Source geometry is retained once per cell;
the outer source and serial schedule remain a reference. Selected total feed/count and stitch `/6`
use actual cell geometry/rates and the existing global-copy mapping. The source plans share 250000
station slots; scheduling retains the global 200000-work cap. Consumables `/3` aggregates selected
cell-qualified yarn demand and per-operation continuity before one canonical complete order per yarn,
preserving minimum/increment/tier/stock/source rules. Idle demand is excluded; missing used-cell
coverage withholds complete quantity/mass/cost. Quote preflight unions all supplied cell yarn locks.
Plan `/17`, production `/5` and parallel `/2` retain all cell plans and original frozen options.
Conditional feed/cost reporting does not imply their optimization: heterogeneous recipe selection
requires makespan. Focused `sewing_parallel` cases cover actual machine/pitch/rate substitution,
source invalidation, selected-cell inventory, combined pricing, idle/partial demand and frozen
stitch replay. Version 1 output is preserved. No shared-labor, transfer or physical-execution claim.
Explicit `preview::svg_cell` rebuilds the whole original job before rendering the retained cell's
actual station geometry/rate and source machine/qualification locks. SVG `/2` identifies cell and
selected global copies, including an empty list for an idle source. Plain `svg` refuses configured
jobs without a named cell; legacy previews retain their bytes. Portable native/browser and both
CLI adapters use the same renderer; CLI files are create-new. The focused regression checks actual
2.5-versus-5 mm needle spacing, feed, full-job revalidation and escaped labels.

**Finite cell availability** (`ftx-pb2.2.7`). Optional source-qualified
`available_until_milliseconds` limits complete cell finish, including initial setup/loading,
once-ceiled feed and final unloading. Incumbent allocation admits only whole cell/count schedules
inside their inclusive windows; the independent lower-bound allocation still considers unfinished
searches whose lower bounds fit. A missing feasible incumbent is unresolved when the relaxed
allocation remains possible, and definitively refused only when valid lower bounds exclude every
complete allocation. An unavailable first-cell seed is explicitly replaced by the selected feasible
assignment. Parallel `/3` retains bounded count/admission receipts and supplied ends; no-window
schemas and serialization stay unchanged. Actual cell recipes, stitches, yarn inventory and material
studies follow the constrained assignment. Reconstruction shares the existing work reservation.

**One shared operator** (`production.parallel.shared_operator`). The explicit
`exclusive_setup_handling_autonomous_feed/1` protocol shares one qualified person across otherwise
independent machines and fixtures. Full cell/effective-recipe/timing hashes, separate estimate consent,
all initial arrivals and every directed inter-cell travel observation are required, including idle
cells. Travel, waiting, handling, setup and final unloading hold that one person; only explicitly
qualified feed releases them. The new `ceil_operation_feed_milliseconds/1` convention reserves every
positive operation's complete feed ceiling independently, so previous rounding never permits early
machine reuse after attended work. Each used cell and the operator must finish inside their inclusive
windows. Parallel `/4` retains both complete exclusive attendance/overlapping-feed timelines and
actual global garment mappings; ordinary cell source geometry, stitches and combined purchasing remain
the same authorities. Allocation and interleaving use retained serial incumbents, with all seeds and
extensions charged to a reserved portion of the existing whole-job cap. Independent cell-finish and
complete attended-work DPs supply unrestricted lower bounds; restricted exhaustion alone proves neither
global optimality nor infeasibility. A cap must cover a complete first-cell seed. The optional compact
phase cache contains only bounded integer durations and source indices, not duplicated station or
fixture histories. Omission preserves independent-mode wire output. Synthetic exhaustive, travel,
deadline, conservative submillisecond feed, source invalidation and native/portable study regressions
exercise this model. Source timing does not establish safe physical autonomy or hardware permission.
Tests: exhaustive and bounded `sewing_parallel`, native/portable `sewing_production_workflow`.
Declared windows do not imply calendar reservations, shared labor, preemption or physical execution.

**Shared fixture pools** (`production.parallel.shared_workholding`). Optional
`shared_workholding_pools/1` maps qualified cell-local workholding resources to finite common
inventories under the existing one-operator dispatch model. Exact cell locks, separate source and
estimate consent, and the explicit feed-completion/access-in-handling release protocol are required.
The actual search acquires units before handling, retains cross-operation holds through waiting and
feed, and releases them at their declared release operation's feed completion. It waits for pending
releases or rejects blocked dispatch branches. All local lifecycles and cell/operator windows remain
enforced. Parallel `/5` retains baseline and selected global-copy reservation intervals; an independent
sweep verifies capacities, same-time release-before-acquire ordering and zero final inventory held.
Unbound local resources remain independent and omission preserves prior output. Ordinary selected
stitch geometry, consumption, purchasing and study metrics use the constrained assignment. Restricted
serial-incumbent search never proves unrestricted infeasibility merely because pooled dispatch fails.
No physical pool access, inter-cell garment transport, factory timing or machine permission is inferred.
Tests: capacity, spanning holds, source invalidation, windows and bounded replay in `sewing_parallel`;
actual native/portable geometry, inventory, study and replay in `sewing_production_workflow`.

**Specialist garment routing** (`production.routing`). `fixed_operation_route/1` assigns every
original operation once per copy to a declared cell, preserving the global dependency order and
full source seam coverage. Private subset admission reuses ordinary geometry/process gates against
the unchanged pattern; only assigned passes must qualify on each specialist, and actual cell
geometry drives stitches, previews, combined yarn purchases and study results. Every cell has
independent attending labor through load/setup/feed/unload, with sufficient unloaded staging at
every cell. One separately sourced transfer resource serializes complete ready-state-to-ready-state
handoffs; cell and transfer finish windows are inclusive. Outer serial timing, parallel allocation,
retained/transported occupied fixtures and implicit fixture transfer refuse. Plan `/18`, production `/6`, routing `/1`
and stitch `/7` retain exact operation/copy/cell identities, source plans and complete timelines.
Bounded fixed-route append dispatch retains a complete incumbent or reports unresolved failure;
only matching an independent mandatory-work lower bound establishes `optimality_proven`.
Two complete replays share the 200000-work cap, and all retained source tables share the 250000
station bound. Serial ordering accessors withhold unrelated timing; the existing elapsed-completion
consumer uses routing. Tests: `sewing_routing`.
Measured factory qualification,
retained-fixture composition and hardware authorization remain open. See
`docs/sewing-production.md`.

**Routed finite fixtures** (`production.routing.workholding` report). Optional existing
cell-local workholding policies qualify each specialist's exact machine/material/process and
single-operation holds. Optional `routing.shared_workholding` reuses the existing pool authority
under the explicit `available_after_unload_with_access_in_cell_handling/1` protocol and pooled
independence declaration. Every hold covers loading/setup/feed/unloading, with no inferred fixture
transport or free access. Local demand remains bounded by its own capacity; bound local resources
add their full demand to the named common pool. Dispatch checks every complete interval against
retained reservations, including future intervals from nonchronological construction order. The
existing seed/improvement work cap and complete-incumbent semantics remain authoritative. Independent
baseline/selected event sweeps verify both local and pool capacities, release-before-acquire ties,
peaks and zero final inventory, retaining exact global copy/operation/cell identities. Plan `/20`,
production `/8`, routing `/3` and nested routed-workholding `/1` preserve exact source replay;
omission retains old output and actual stitches/purchasing retain the existing authorities.
For eligible routes, admission covers every qualified operation/cell pair; reservations follow
the actual selected cell for each copy. Unused eligible plans contribute zero occupied holds.
Bounds: 64 total source holds, 1024 expanded holds, 256 qualification references, existing per-cell
limits and at most 16 shared pools/128 bindings. Tests: `sewing_routing` known capacities, future
reservations, complete unloading, summed/multiple pools, source/material invalidation, windows,
bounded complete incumbents, exact stitch and material-study replay; `sewing_parallel` guards the
shared source validator. Integer inventory does not establish physical grip/access or hardware permission.

**Eligible specialist cells** (`production.routing`, `ftx-pb2.2`). Explicit
`eligible_operation_route/1` retains one authored operation sequence and a seed cell per step, while
bounded search chooses an independently qualified cell for each physical copy/operation. All eligible
source subsets and adjacent directed handoffs are admitted before search; actual preceding cells,
per-operation phases, source windows and cell-specific geometry govern complete timelines. Source
station/stitch reservations cover the entire choice domain. Plan `/19`, production `/7` and routing
`/2` mark the outer one-garment seed reference; actual production operations identify their cell and
cell-local seam index. Selected execution counts alone drive yarn channels and one aggregated order.
Comparisons retain executed recipes and the complete routing decision protocol. Makespan selection
does not claim feed- or cost-optimal assignments. The relaxed mandatory-work bound excludes optional
cell/setup assumptions; restricted search completion alone is not a minimum-time proof. Fixed inputs
retain prior serialization. Tests: independent assignment/dispatch enumeration, changed pitches and
yarn counts, source/window/budget refusals, actual native/portable replay cases and browser lifecycle
coverage in `sewing_routing`, `sewing_production_workflow` and `check_materials_frontend.cjs`.

**Planar sewing access** (`passes[].access`). Optional `planar_disk_sweep/1` requires an explicit
flat assembly and an admitted source process qualification. Full machine and qualification hashes
bind the independent tool/fixture acquisition to the current configuration and material stack.
Every complete canonical needle chord is checked against all filled simple fixture polygons using
a circular tool sweep in the existing operation-local millimetre frame; reverse and backtack runs
revisit the same checked chords. Tool contact refuses, while a positive declared margin is inclusive.
Plan `/14` and per-seam `ftx-sewing-access/1` receipts retain exact minimum distances and deterministic
closest-point witnesses; the full source policy stays in the ordinary replay options. Limits are
64 fixtures, 1024 total polygon vertices and 256 qualification references per operation, with 50000000
aggregate geometry work units per plan. Omission preserves existing wire output. This establishes
declared static planar separation, not grip, cloth support, three-dimensional access, approach motion,
machine registration or hardware authorization. Tests: `sewing_access`; see `docs/sewing-access.md`.

**Material-following recipe selection** (`material_study::recipes`, plan §25.10 / `ftx-ms2`).
Optional `Request.sewing_selection` is exclusive with fixed `sewing` and reruns the existing
qualified recipe selector on every freshly bound material alternative. Its unmodified selected
`SewPlan` supplies the ordinary sewing outcome, metrics and comparison; no incumbent means a
refused sewing outcome with unknown metrics. The separate typed receipt preserves every trial,
source identity, choice and proof/budget flag without duplicating station geometry. A complete
feasible incumbent remains usable after a finite search limit, with no optimum claim. An available
search receipt alone never satisfies a required sewing consumer. At most 256 configured plan
evaluations, 4096 pass evaluations and 200000 ordering-search nodes across the study are admitted
before alternative evaluation; per-job limits and the existing retained-station cap still apply.
No geometry, machine limits, qualification policy, pressure setting or unlisted recipe is changed.
Missing/stale source qualification preserves independent manufacturing and simulation outcomes.
Omitted selection retains legacy serialization. Tests: `material_study`, native and file-free WASM
`material_study_consumers`; source admission is not physical seam validation or hardware permission.

**Authored specialist-route selection** (`sewing_selection`, `ftx-pb2.2`). Opt-in selection `/6`
compares the template fixed route with one to fifteen independently qualified complete route policies,
optionally crossed with the existing recipe groups. Pattern, passes, production quantity, construction
and thread market stay fixed; ordinary `SewPlan::build` remains the sole feasibility and timing
authority. At most 64 full route/recipe attempts share one budget; source-plan, scheduling and
purchasing bounds cover the largest candidate before expansion. Template route then lexical route
IDs define traversal and equal-score ties. Trial `route_choice` and incumbent `selected_route` survive
the compact material-study receipt, and selected options preserve the exact winning route for all
existing manufacturing consumers. Complete incumbents survive bounded search; unresolved inner
routes prohibit optimum claims. Omitted candidates preserve prior schemas and serialization.
This is finite declared-route selection, not inferred machine eligibility, unrestricted routing
optimality, measured factory performance or hardware authorization. See `docs/sewing-selection.md`.

**Complete thread-cost recipe selection** (`sewing_selection`, plan §25.4 / `ftx-tm7`). The explicit
`ftx-sewing-selection/3` objective `minimize_thread_observed_order_cost` requires dated thread
purchasing and consumes only the complete canonical order total from each freshly admitted
`SewPlan`. No independent thread estimator, unit-price scorer or cost-monotonicity pruning exists.
The common market is validated canonically before enumeration, and its additional quote scan is
included in `Request::purchasing_work_bound()` alongside every configured trial. The total bound
must fit 200000 work units, including expanded study rows. Missing quantities, partial channels or
ineligible orders produce separate `objective_unavailable` trials: they cannot win, prove an optimum
or establish that all production recipes are infeasible. A complete priced incumbent survives
finite interruption; exact ties retain the original canonical recipe traversal. The report retains
`selected_thread_order_cost`, its own `objective_currency` and the unchanged selected source/price
receipts. Versions 1 and 2 retain their wire output and time objectives. Thread prices remain
independent of cutting copies, labor, supplier bundles and hardware permission. Tests:
`sewing_consumables::selection`, native/portable selection replay. See `docs/sewing-selection.md`.

**Qualified sewing consumables** (`sewing::consumables`, `ftx-tm6` / `ftx-tm7`). Optional
`SewOptions.consumables` produces `ftx-sewing-plan/11` from the actual selected operation traversals.
Independently sourced consumption models require a complete channel declaration, frozen yarn
selections, positive metres per traversal and explicit start/end allowances. Reverse travel and
reinforcement count every interval; allowances apply once per operation/channel. Missing models or
channels keep complete demand and purchasing unknown. Exact yarn demands aggregate before canonical
length orders apply dated quotes, minimums, increments, price tiers, capacity and continuity policies.
Transitive supplier aliases across current market observations cannot duplicate cross-yarn inventory.
No splitting, universal thread ratio or retained-mass conversion is inferred. Studies expose nominal
`thread_required_length_m` and `thread_observed_order_cost`, with independent required consumers and
the thread market's own currency. Thread quantities follow the resolved production count; geometric
and duration totals remain per garment unless cross-copy scheduling is explicitly requested. Comparison
protocols use only selected qualifications. All expanded material/design rows and configured recipe
trials reserve at most 200000 purchasing work units. Tests: `sewing_consumables`, canonical
`material_cost` units and workbench lifecycle checks. See `docs/material-sewing-plans.md`.

**Declared assembly mass** (`assembly_mass`, plan §25.10 / `ftx-ms2`). Every expanded physical
cut instance uses the same exporter outline, fold/mirror semantics, allowances and outward tabs.
Strictly contained disjoint closed cutouts are subtracted with the existing budgeted cutting
geometry kernel. Source-locked areal density supplies retained sheet mass; width, grain, body and
mechanics are unrelated requirements. `sim:false` and simulation scaling never alter this inventory.
Extra retained thread, trim, closures and loose fill need explicit source-bearing group masses in
kg using the existing `Quantity`/`Source` vocabulary and independent estimate consent. Missing
contributions and undeclared inventory completeness keep the assembly total unknown. Reported
endpoints are scaled/summed only with complete bounds and fixed authored geometry. This is one
pre-finishing cut assembly, not purchased stock, a simulated subset, propagated geometry uncertainty,
a weighed final garment or automatic discovery of unrepresented components. Studies optionally rerun
this consumer and compare `assembly_mass_kg` under explicit nominal or reported-bounds policies. Effective
cutting/mass allowances must match; missing complete mass never becomes a winning zero. Aggregate
instance, extra-group and polygon work limits apply before returning a study frontier. Tests:
`assembly_mass`, `material_study`; native/WASM adapters use the same evaluator.

Pattern-owned `attached_mass` extras share the `ftx-pattern::material_mass::Extra` authority, re-exported
at the existing path. Their source observations merge once with request extras by ID; identical
complete observations share one contribution and conflicting observations refuse. Optional
`attached_inventory` preserves the pattern groups/targets while `request` stays the original inventory
request. Inventory completeness is still explicit, and unknown mass stays unknown. No attachment
keeps the existing output shape. See `docs/attached-mass.md` and `tests/assembly_mass.rs`.

**Material-study comparison** (`material_study::comparison`, plan §25.10). Optional explicit
objectives/constraints consume actual cutting/sewing/dated procurement results on the same
unchanged pattern and shared process inputs. Canonical metric names fix m², kg, seconds, counts or
the exact market currency. Required unknown/refused/incomplete outputs exclude the candidate,
never score zero. Default dominance requires matching relevant reported protocols by source piece;
mixed protocols require deliberate consent. Nominal mode retains its existing point comparisons.
Reported-bounds mode supports complete `assembly_mass_kg` endpoints and all eight vapor potential
metrics from their canonical consumers. Mass holds authored geometry and additive inventory fixed;
vapor holds current geometry, boundaries, films, interfaces and integration policy fixed while
propagating complete source Ret endpoints. Inclusive constraints must contain
the whole range; dominance requires strictly separated ranges, while overlap/touching and missing
bounds remain explicit. Other objective/constraint metrics refuse this mode before evaluation;
source bounds are not relabeled as numerical-solver or physical-garment uncertainty. Required
consumer/source gates remain independent. Ties, tradeoffs, exclusions and incompatible pairs remain visible. Existing reports and
request identities are unchanged when the optional comparison is absent. See
`docs/material-screening-and-studies.md`; direct real-planner, native-binary and file-free WASM
regressions use synthetic geometry/data. No physical validation or global optimum is asserted.
Scenario comparisons also consume fresh production drape/heat outcomes by stable key, without
introducing a numerical crate dependency here. Signed heat, simulated-cloth mass and reported edge
strain require matching pattern/body/scenario identities, geometric settledness and clear gating;
required heat consumers retain their own validity domains. Callback serialization occurs only when
needed, one alternative at a time. Full scenario context qualifies the top-level comparison.
Explicit motion consumers/objectives additionally require the requested complete converged trajectory,
its path/initial/end geometry links and physical duration. Independent-sheet and body-coverage
transient objectives separately admit complete policy/time/geometry-linked histories for signed
incoming/net boundary energy and separately initial-inclusive sampled or final cloth-cell temperature
extrema. `motion_required` exposes whether the comparison policy requires movement. Heat-capacity protocols participate only
for transient comparisons. Required motion/history failures never borrow initial static outputs;
existing static metric names and default consumer requirements keep their original meaning.
See `docs/material-scenario-studies.md`; geometric settledness is not stress equilibrium or fit.
Opted-in plastic work is admitted separately for independent-sheet and covered/exposed histories.
Each requires the exact bound material conversion observation, accepted per-triangle cumulative
work, excluded initial dissipation and complete source-energy accounting at every sample. Coverage
uses physical storage, never remap residual, in the balance. Conversion protocols participate only
when that thermal domain is consumed. A refusal preserves independent domain and manufacturing
outputs; neither a boundary total nor the other thermal model can substitute for supplied heat.
The separately requested bending-yield source uses the same conservation admission, with its own
qualified observation and accepted per-triangle work. Initial and final patch records must retain
the same law, source-triangle prefix, six-node stencil and reference area, and reconcile with captured
work. Unconsumed conversion observations do not split mechanical or uncoupled thermal protocols.
Regressions: `tests/scenario_comparison/bending_heat.rs`; contract: `docs/bending-heat.md`.

Source-qualified textile-water histories also reach the existing transient-surface
temperature and boundary-energy decisions. Nominal `textile_water_evaporated_mass_kg`,
`textile_water_remaining_mass_kg` and `textile_water_latent_energy_j` objectives read the same
admitted producer, separately from prescribed body-water histories. Admission matches frozen
material assignments, initial thermal-frame reference topology/ownership, canonical Ret networks,
per-patch final water and final dry/liquid sensible storage. Every retained sample reconciles
water inventory, latent energy, source-domain enthalpy and the combined boundary/mechanical balance.
Missing, changed, unsupported or unrequested water cannot become zero or a feasible alternative.
Consumed Ret protocols and the explicit coupling policy participate in protocol comparability.
No wet-property characterization, propagated thermal/water intervals or continuous-time extrema
are inferred. Version 1 preserves initial-only ownership. Version 2 independently qualifies incoming
liquid evidence and integrates each instance's reference-area delivery and incoming sensible enthalpy
over every authored interval. `textile_water_supplied_mass_kg` reads that admitted cumulative delivery;
rewetting may increase remaining liquid. Mixed schemas, missing feed evidence, altered schedules and
unsupported loading cannot qualify. Incoming enthalpy is subtracted once in the full energy balance.
When `mechanical_coupling` opts into bidirectional co-moving transfer, admission also reconciles
every accepted mechanical inventory against supplied plus condensed water, outgoing evaporation,
the unchanged source substrate, and the initial/final/trace exchange ledgers. Independent incoming
and outgoing bulk momentum/energy never enter thermal latent/sensible objectives. Missing or
invented transfer/source fields refuse the dependent comparison; exact source assembly and cutting
remain independently comparable.

**Scoped material-direction requirements.** `material_strain` and `motion_material_strain` admit raw
dimensionless `[E11, E22, 2E12]` from the production membrane frame. Six static signed min/max metrics
cover warp, weft and engineering shear; the `motion_` prefix requires the initial state and every
accepted motion sample. Canonical materials/domains, complete selected triangle ownership, exact
position/source hashes, work allowance, consistent local/global extrema and deterministic sampled
witnesses are reconciled without duplicating the strain solver. Exact selected instances and strain
convention participate in protocol identity; geometry may vary across declared design alternatives.
Inclusive comparison bands are authored design requirements. Characterized domains are not failure
limits, and these nominal sampled values establish neither safe fit nor continuous-time bounds.
Unavailable motion strain preserves static strain readings and independent manufacturing results.
Regressions include all twelve inclusive signed constraints, changed-source/partial-history refusal,
actual portable design selection and native/compiled-WASM selected-body replay.

**Material-following vapor studies** (`material_study::vapor`, plan §25.49). Explicit ordered layer
occurrences resolve the final source-piece/default selections after every atomic material substitution.
The canonical `material_vapor` evaluator supplies its independent report/refusal and exact standalone
replay. Common area, pressures, films and latent heat are explicit shared assumptions; source-piece
membership does not infer geometry, coverage or gaps. Optional nominal potential-power/mass-flow
comparison requires available vapor results and uses only the consumed observations' complete
protocols. Conditional reported-bound comparisons additionally require complete canonical endpoint
propagation. Missing bounds retain nominal values but exclude robust ranking; endpoints do not
represent statistical confidence or uncertainty in geometry and boundary assumptions.
Omission preserves existing study serialization. Tests: `material_study_vapor` and the
native/portable `material_study_consumers`; no actual evaporation, comfort or empirical assembly claim.

**Draped vapor comparison** (`material_study::comparison`, plan §25.55 / `ftx-vc4`). Nominal
`independent_vapor_potential_heat_flow_w` and `independent_vapor_potential_mass_flow_kg_s` objectives
consume the initial scenario's actual independent-sheet vapor result. Optional required consumer
`vapor_surface` uses the same gates. Pattern/body/scenario, settledness, retained frame/material,
current geometry/ownership, frozen boundary policy, canonical column reports and area-integrated
totals must agree. Cross-alternative protocols follow consumed columns by stable cloth-instance
identity, independent of internal column order or unused columns. Explicit mixed-protocol consent
cannot bypass missing data or freshness checks. Dry heat/storage remain independent; motion endpoints
do not supply static readings or an inferred time integral. Source bounds are not propagated
uncertainty, summed sheets are not covered-body evaporation, and no physical comfort claim follows.
Canonical source-column merges/splits and source IDs do not create false protocol incompatibility;
the common-area `vapor` stack remains a separate consumer. Tests: `scenario_comparison`, production
file-free `material_scenario`, and the `ftx-drape` material-scenario tests.

**Text and numbers** (`text.rs`, §2). `fnum` (fixed-point, ties to even on the exact binary value, trailing
zeros stripped, `-0` → `0`), `ascii_text` (NFKD, `×` → `x`, `— – ·` → `-`, code points 32..126 kept),
`safe_name`, Helvetica `text_width`, greedy `wrap`, the single-stroke font (`stroke_text`), tile column
letters, page lists, PDF string literals and XML escapes. NFKD comes from `ascii_fold.tsv`, what
`ascii_text` keeps of each of the 2,138 non-ASCII code points that keep anything, generated with the
reference's Python 3.12 (Unicode 15.0.0) by `tools/oracle/gen_ascii_fold.py`: no dependency. Against
`tests/fixtures/oracle/export/helpers.json` every value is byte-exact (524 numbers and strings).

**Cut outline** (`outline.rs`, `union.rs`, §3–4). Edges are flattened as the reference does (Béziers at
`clip(ceil(control polygon / 1 mm), 24, 800)` samples, points closer than 1e-7 dropped); SA is
`edge.sa` → `piece.sa` → the default, 0 on the drafted fold edge of a half. The direct offset mitres each
corner (clipped square to the bisector at `mitre_limit × max(SA)`) and is kept when it is a simple polygon at
least as large as the sew ring; otherwise the union of the sew polygon, a single-sided buffer per edge and a
wedge per convex corner, whose largest part's exterior is the outline; then duplicate and collinear points
are dropped. The reference unions with GEOS; here (`union.rs`) every edge is split where it meets another,
the pieces with the union's outside on one side and inside on the other are kept (oriented with the inside
on the left), dangling chains are pruned and rings are traced turning as far left as possible. A polygon's
area is where its winding number has the sign of its signed area: GEOS's positive depth, which keeps the
lobes of a self-intersecting corner wedge that GEOS's `buffer(0)` keeps (the area rule predicted 184 of 184
random bow-ties; the turn at the top vertex, 117) and everything a self-overlapping edge buffer covers.
Edge buffers are built as GEOS builds them: the polyline and its offset, mitred at a joint turning left
(limit 10), joined at the crossing of the offset segments at one turning right, or by a closing segment
through the midpoints towards the vertex where they do not cross (a curve tighter than the SA); per-segment
rectangles were tried first and left sliver tails nanometres thin at nearly straight inside turns.

Against `tests/fixtures/oracle/export/outlines.json` (102 outlines: every piece as drafted and every
instance of TESTBODICE, also at SA 15 with mitre limit 2, ROBUST, the single test pieces and the drafted fez,
bolero and trousers at 80, 92 and 116 cm): sew rings and edge breaks equal (91 bit-exact, 11 within 1e-13 mm:
numpy raises to the third power with the platform `pow`), every method equal (58 mitre, 44 union), mitre
outlines within 1.2e-13 mm vertex for vertex and union outlines within a Hausdorff distance of 4.9e-6 mm
(the spec allows 0.002 and 0.05 mm).

**Piece geometry** (`geom.rs`, `pole.rs`, §5). `build_geom` of a piece as drafted (a piece on the fold shows
its half, fold edge SA 0, the place-on-fold bracket) or of an instance (unfolded, the fold line): facts
(qty, material, fold mode, SA values), cut and sew rings, notches on the cut line pointing inward with
their marks (V, T, castle, check, U, slit), the grainline (given, or by default 70 % of the vertical chord
through the pole of inaccessibility), fold marks and labels, internal lines, drills and decor through the
feature maps (the fold reflection, the mirror), and the main label block. The default grain's pole is
shapely's `polylabel`, GEOS 3.14's `MaximumInscribedCircle`: exact for a convex triangle or quadrilateral
(of the circles tangent to three edge lines, the last largest: the left end of a wide rectangle's plateau, the
top end of a tall one's), else its branch and bound with its interior-point seed and libc++'s heap order.
The main label goes where the largest block fits in the region the reference allows (the sew polygon inset by
2 mm, clear of the grainline by 7, internal lines by 4, drills by d + 4, fold marks by 5, decor by half its
width + 4), found by a branch and bound over anchors (the size that fits drops by at most
`e / (0.53 · min(wmax, hsum))` when the anchor moves by `e`) to 2 % of the text size, seeded with the region's
pole and its centroid. The reference instead takes GEOS's pole of the region's best part and a size bound it
does not check (spec 03 Q4), so its block can overlap the grainline; `main_label_fits` tells.

Against `tests/fixtures/oracle/export/geoms.json` (every piece as drafted and every instance of the outline
fixtures but SA 15, at text sizes 6 and 10; 376 geometries): 38,422 numbers within 1e-9 mm (worst 1.4e-14),
default grains included, every fact, notch, mark, fold, internal line, drill, decor and secondary label equal.
All 188 main labels fit their region with the same lines; 23 are more than 5 % larger than the reference's, 6
more than 5 % smaller where the reference's own block does not fit, the rest within 5 % (the spec asks for
"inside the region, no obstacle overlap, size within 5 %").

**Layouts and the master SVG** (`layout.rs`, `ctx.rs`, `svg.rs`, §6, §7.1). `Ctx` holds the options and the
names every file uses (`safe_name(style)`, the ASCII size label; a pattern traced in pixels is refused with the
reference's message). The master layout puts per piece its drafted half when cut on the fold (unless
`unfold`), else its primary instance, on shelves (tallest first, ties by instance id; 15 mm gaps, 10 mm margin,
width automatic or `layout_width`). `master_svg` writes the reference's SVG line for line (one element per
line, `fnum` numbers, Python `repr` stroke widths, JSON-null material as `None`) and the manifest's `master`
entry. Against `tests/fixtures/oracle/export/master_svg.json` (every fixture pattern and the NOTE piece, plus
`--unfold` and `--layout-width 300` for four of them; 25 SVGs, 2,479 lines): 2,080 lines byte-identical, every
other line equal but for the cut line of a union outline (within 0.05 mm) and the main label's lines (its own
anchor and size); manifest entries within 0.002 mm.

**PDFs** (`pdf.rs`, §7.2–7.6). The reference's hand-written writer: one content-stream line per operator group
(`fnum` numbers), the two standard fonts, the six layers (`Size …`, notches, seam lines, grainlines & labels,
placement marks, soutache; the projector's calibration grid off), FlateDecode streams (`miniz_oxide`, level 9:
valid, not zlib's bytes), `/PrintScaling /None`, `/FPTile` page dictionaries. The creation date comes from
`created` (the options' or the pattern's `meta.created`), so every file is reproducible; the producer is
FrankenTextiles. Targets: tiled Letter and A4 (information page with the test squares, instructions, 1 in square and
tile map; tiles with watermark, clipped content 5 mm past the frame, join half-diamonds, footer), copy-shop A0 and
36 × 48 in (the orientation with fewer printed sheets, the band with squares and scale bars), projector light and
dark (every piece once, unfolded, grain vertical, text at least 12 mm high, test squares, calibration grid); each
with its manifest entry. A tile is blank when no cut line reaches it (shrunk by 0.01 mm); a piece is drawn on a
tile when its cut line comes within 1 mm of the tile's bleed box.

Against the reference's own files (`tests/fixtures/oracle/export/pdf/`: TESTBODICE, ROBUST also at
`--layout-width 300` with and without `--keep-blank-pages`, TESTBODICE at `--projector-width 1100`, the 92 cm bolero
and trousers; 42 PDFs): the same files and manifest entries; the same objects and dictionaries (lengths, the
date and the producer aside); of 36,867 inflated content lines 34,672 byte-identical, every other number equal to
0.000 mm but for 119 union cut lines (within 0.05 mm) and 2,076 main-label text lines (ftx's own anchor and
size).

**HPGL, cutter files, manifest pieces** (`hpgl.rs`, `cutter.rs`, `ctx.rs`, `pyjson.rs`, §7.9, §7.10, §7.13).
HPGL plots the master layout at 40 units per mm (ties to even), pen 1 (sew, internal draw, decor, grain, fold
lines, drills, label stroke text) before pen 2 (internal cuts, notches, the cut line) per piece, optionally
mirrored and turned. Cutter SVGs: per instance (labels naming the side), grain vertical, the cut polygon united with
a 4 × 3 mm outward triangle per notch, 1 mm margin, with the mat report. JSON is written like Python's
`json.dumps(indent=1)` (key order kept, ASCII, `repr` floats; `PyJson`). Under
the same integer-format contract, `PyJson` preserves signed `i64` and unsigned `u64`
metadata through parsing, `serde_json::Value` conversion and writing; seeds and identifiers
above `i64::MAX` never pass through `f64`. Existing float and signed-zero formatting is
unchanged. Larger integer literals refuse explicitly (the named JSON-integer-range exception
in `FEATURE_PARITY.md`); this is bounded integer support, not arbitrary-precision Python arithmetic.
Boundary regressions are `pyjson::tests::full_width_unsigned_integers_keep_their_identity_through_every_conversion`;
the ordinary photo/round regression is in `ftx-wasm/tests/page.rs`. Against
`tests/fixtures/oracle/export/targets.json` (TESTBODICE also rotated and mirrored, ROBUST, the 92 cm fez, bolero
and trousers): every HPGL command and pen equal and every polyline identical but the main label's strokes (ftx's
anchor, the same strokes) and union cut lines (within 0.05 mm); cutter titles and reports equal, outlines within
0.05 mm as regions (where a notch triangle's base runs along a curved cut line GEOS, which grows the cut by 1e-4 mm
first, keeps the micrometre sliver between them as a hole); the manifest's `pieces`, `hpgl` and `cutter` entries
equal (cut perimeters within 0.05 mm).

**DXF** (`dxf.rs`, §7.7–7.8). Written here, not from a template. ASTM / AAMA (R12): header (`$ACADVER AC1009`,
`$HANDLING 0`, extents computed, quirk Q1), LTYPE, LAYER (all colour 7), STYLE, APPID tables; one block per piece
(its primary instance unfolded, text size 10) with the cut and sew lines (2 dp, ASTM copies on 84 and 87),
turn and curve points (15°, 10 mm), notches as POINTs (depth, width, angle; ASTM typed layers 80–83) and LINEs,
grainline, internal lines, fold lines, decor, drills and piece text (ASTM notes on layer 15); style text and
INSERTs in modelspace; dates from `created`. Flat (R2000): handles, owners, symbol tables, block records,
`*Model_Space`/`*Paper_Space`, a root dictionary, `$INSUNITS 4`; every instance laid out grain vertical, y up,
as LWPOLYLINEs and CIRCLEs on CUT, DRAW, NOTCH, DRILL, GRAIN and TEXT (single-stroke text). Both open in ezdxf 1.4.4
with a clean audit. Against the reference's files (`targets.json`, clocks and random ids fixed): the same blocks,
and per block and layer the same entities within the 2 dp rounding (union cut lines and their turn points within
0.05 mm; piece texts that hang off the main label compared by string); the same modelspace; flat files entity
for entity (main-label strokes at ftx's anchor). `check_dxf` and `check_flat_dxf` (a small DXF reader) give
exactly the reference checker's 268 items on its files and pass on ftx's.

**GarmentCode** (`gc.rs`, §7.11). Panels of every simulated instance (net sew lines in cm, relative-curvature
Béziers, poly edges as straight sub-edges, placement from `placement3d`) and 1:1 stitches after splitting each
seam's sides at matched arclength points (notch knots, candidates merged within 0.05 mm; Béziers split by de
Casteljau at the parameter of an 801-sample arclength table), `right_wrong` for unreversed seams, warnings for
unresolvable or excluded seams. All five reference files (`targets.json`) come out byte-identical; the manifest
entries are equal; `check_garmentcode` reproduces the reference checker's items.

**Soutache DSTs** (`dst.rs`, §7.12). Per instance carrying soutache: the design centre at the sew line's box
centre, a 5 mm placement run on the sew line, a stop, and the braid lines (joined where they meet at the fold,
ordered nearest first, reversed where nearer) at 2.5 mm; records in balanced ternary of at most 121 steps (moves
split with ties to even), a needle-down stitch after each jump, a 1 mm tie-off; the 512-byte header; the hoop;
the README. The record encoder matches the spec's goldens and round-trips every move. All seven reference DSTs
(TESTBODICE's fronts, the braided bolero) come out byte-identical, the README and manifest entries equal;
`check_dst` reproduces the reference checker's 140 items (with and without the pattern's braid lines) and catches
the spec's braid shifted 4 mm (on-braid fails, lengths pass).

**Everything, end to end** (`all.rs`, §1.3, §7.13, §8.1). `export_all` makes every target in the reference's
order and the manifest (`schema`, `style`, `size`, `created`, `source_schema`, `files` in target order, `pieces`,
`master`, `hpgl`; written like Python's `json.dumps(indent=1)`); `check_files` runs the checks over a set of files
in the reference's order (DXFs, master SVG, PDFs, cutter SVGs, HPGL, GarmentCode, DSTs; with the pattern the
sew references recomputed and the braid lines checked), a missing listed file a failed `file.missing` item (the
reference raises, plan §12); `read_dir_files` reads an output directory. Against `all.json` (TESTBODICE, ROBUST,
the 92 cm bolero with and without soutache): the same files, the same manifest line for line (numbers within
0.05 mm), and the same checks in the same order, all passing, as many as the spec's goldens (TESTBODICE 146, 150
with the pattern; ROBUST 125; the braided bolero 183 with the pattern; the plain one 128).

**Checks** (`checks.rs`, `xml.rs`, §8). `check_svg`: `svg.no_transform`, `svg.mm_units_viewbox`,
`svg.piece_subgroups` (master) or `cutter.single_closed_path` (cutter), with the reference's details, reading
the file back with a small XML reader. They pass on all 25 of ftx's and the reference's master SVGs, and the
reference's own mutation (a `transform` before the title), a piece without its fold group and a wrong viewBox
each fail exactly their check. `check_pdf` reads PDFs back as the reference does (objects, inflated streams,
OCG names, pages; a content interpreter for the CTM, tags, rectangles, tagged cut points and font sizes):
`pdf.print_scaling_none`, `pdf.ocg_unique`, `pdf.mediabox_exact`, `pdf.ocg_identical_across_pages`, the test
squares (page 1, every copy-shop sheet, the projector), `pdf.projector_text_cap_12mm`, `pdf.projector_under_200in`,
and with the master SVG's cut vertices `pdf.restitch_matches_master` and `pdf.tiles_cover_master`. On the
reference's 42 PDFs it gives exactly the reference checker's 294 items, details included; all 294 pass on ftx's;
a wrong paper, master vertices 1 mm off and a master vertex off every tile each fail exactly their check.
`check_hpgl` (same layout as the master, pen-2 bounding box against the master's) and the cutter SVG check give
exactly the reference checker's 113 items on its HPGL and cutter files and pass on ftx's.

**Partial sewing operations.** `SewingPass.source_range` selects original side-A arclength and
retains oriented-B notch correspondence. Selected stations, offsets, folds, reverse feed and locks
reuse the production checks; every seam's passes must jointly cover both full source sides. Explicit
ranges produce `ftx-sewing-plan/10` and a `source_span` with original ranges/full lengths and selected
one-way traveled lengths. Whole-seam source acceptance and hardware refusal remain intact. Omitted
ranges retain legacy serialization/arithmetic. See `docs/material-sewing-plans.md`.

**Complete mixed-stock cutting.** `cutting::job::Request` routes every original source piece to the
existing roll or finite-stock planner while retaining the complete frozen pattern. Matching copies
and effective seam allowance, bounded combined inventory, and exact instance/copy/material coverage
are mandatory. The canonical scoped subplans retain original geometry and whole-pattern digests;
the job's pattern/request JSON strings replay the full route. Same-market procurement adds whole
roll orders and used exact finite items only with complete offer coverage. Physical-item-bound
offers cannot also supply rolls. Missing prices preserve geometry and stay unknown. Material studies
rerun the complete job after substitution, independently of sewing and physics. See
`docs/mixed-stock-cutting.md` and `tests/cutting_job.rs`; synthetic verification is not machine approval.

**Source-reported lower roll width.** Optional `CutOptions.width_basis: "reported_lower_bound"`
uses each admitted width observation's positive lower bound for complete shelf placement, polygon
nesting and marker partitioning, before subtracting edge margins. Missing bounds refuse; source
records, estimate admission and cut geometry remain unchanged. Each marker's optional `layout_width`
retains the used width, complete normalized source range and scope notes; the SVG shows that envelope.
Nominal `width_m` still supplies gross area, mass and invoice-unit conversions using the newly planned
length. This policy does not produce area/mass/cost intervals or a physical stock guarantee. Existing
whole/split supplier orders, mixed-job roll routes and material studies consume the same replanned
markers; finite-stock polygons remain independent. Omission or explicit `nominal` preserves existing
schemas and nominal serialization. See `docs/cutting-plans.md` and `tests/{cutting,cutting_job}.rs`.

**Bounded split supplier orders.** Optional `market.split_orders` produces `ftx-cut-procurement/3`
by assigning indivisible markers to current exact-material-locked offers. Each aggregated order
reuses canonical quote rounding, capacity, continuous-cut and quantity-price evaluation. Connected
supplier/base/tier SKU aliases share one stock identity, including links through unselected offers
and other material groups. The shared work bound charges prefix extensions and uncached quote
evaluations; studies reserve configured work across every alternative against a 200000-unit cap.
Complete feasible selected costs remain available after exhaustion, while minimum-cost metrics
require completed proof. Mixed jobs add exact finite-item prices only when all routes are complete.
Neither an incomplete subtotal nor duplicated physical stock may supply a comparison value.

**Named scenario matrices.** `material_study::Request::evaluate_matrix_with` expands and binds one
unchanged-pattern material study, runs manufacturing once and retains every named case's independent
production outcomes. One to eight unique cases reserve at most 64 alternative/case slots before
manufacturing or callbacks. Each effective policy uses the ordinary source/geometry/consumer
admission; a case override may replace the fallback policy. An all-cases frontier exists only when
every case has an explicit policy. Eligibility requires all case constraints; dominance reuses the
ordinary nominal/source-range relation on every case/objective coordinate, with strict improvement
somewhere. Protocol admission stays local to each case. Missing results and contradictory preferences
stay visible; there is no averaging or inferred probability. `tests/material_scenario_matrix.rs`
checks standalone equality, complete admission, cross-case ties/tradeoffs, interval uncertainty,
protocol isolation, binding failures and bounded work. Numerical cases do not establish garment fit.

**Error model.** `Refusal` `invalid_document` for a missing vertex, a malformed curve, a seam allowance
outline without area; `unscaled` for a pattern traced in pixels; `out_of_range` for a projector page over
200 in; `io` for an unreadable output directory.

**No claims.** GEOS's exact vertex lists for union outlines (its ring starts where its overlay starts and
its sliver clean moves points by up to 1e-4 mm); the reference's main label anchors; zlib's compressed bytes; the reference's DXF template; a check that raises
on a missing file.
