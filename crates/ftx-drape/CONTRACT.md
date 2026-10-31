# ftx-drape contract

**Source-qualified vapor potentials** (`thermal::vapor`, `ftx-vc1`). Characterized drapes retain an
independent material/ownership/geometry frame without requiring dry conductivity or heat capacity.
Explicit vapor conditions reuse `ftx-pattern::material_vapor` per source-locked material column,
then integrate potential heat and mass flux over current cloth triangles once, with piece totals.
Nonuniform conditions require the exact column-model identity; stale geometry/material receipts,
unadmitted source protocols and missing Ret refuse independently of mechanical/dry-heat results.
Optional study evaluation uses each freshly draped alternative and each actual motion endpoint.
Frozen replay preserves exact source policy and current/reference area distinctions. Independent
sheet sums are not covered-body evaporation, water availability, latent heat feedback or a warmth/
comfort/physical-validation claim. See `docs/vapor-resistance.md`; analytical/source-only tests and
`material_scenario` exercise canonical consumer equality and real sewn geometry.

Complete positive source Ret endpoints also propagate through these same current-area sheets,
covered-body regions and positive-duration motion samples. Each optional range is conditional on
the fixed geometry, boundary inputs and retained integration rule. A missing or unrepresentable
contribution keeps the aggregate range unavailable while preserving nominal results. Source repeats
do not require an independence assumption; no SD/CV conversion, confidence level, unsampled-motion
error bound, moisture supply or latent cooling follows from these ranges.

**Qualified parallel material memory.** `Solver::with_membrane_histories` atomically binds matching
per-triangle elastoplastic/Maxwell branches from one qualified complete material. The original
individual binders refuse a joint half-law; absent joint declarations retain mutual rejection.
The existing incremental potential, exact derivative chain and reactions add Maxwell response to
the elastoplastic equilibrium branch once. Both histories and their separate physical dissipation
ledgers commit with accepted geometry/velocity/contact state; failed steps roll back both. Unit
compression and original grain/rest ownership are required. Canonical sewn scenes and timed strips
use the same binder. `tests/parallel_membrane.rs`, `tests/strip_history.rs`, `tests/relaxation.rs` and
the internal geometric derivative test cover known answers, source/domain refusal, independent
ledgers, real sewn ownership and rollback. This is a declared parallel network, not same-branch
viscoplasticity or measured textile acceptance; see `docs/material-parallel-memory.md`.

**Purpose.** The cloth solver that replaces Blender's (plan §11): an orthotropic Green-strain membrane
rotated to the grain, quadratic hinge bending, lumped inertia, and implicit Euler solved by Newton on a
sparse LDLᵀ with an Armijo line search; a static solve; and contact with a static body (IPC barrier on the
thickness-offset gap, additive CCD, lagged smoothed Coulomb friction); and the garment mesh of spec 04 §2
(`garment.rs`). Placement, pre-sew, the drape schedule and the reference's metrics are described below.
Optional cloth self-contact adds moving cloth–cloth barriers and independent friction.

**Anny clothing collider.** CLI, portable fold and fitted studies share `ftx_body::clothing_skin`:
dominant-bone wrist/foot regions and paired `levator05` facial details are replaced by centroid caps.
The complete detailed source can self-intersect; that negative must still fail `audit_surface`.
No audit exclusion or tolerance relaxation is applied to the proxy. Retained source coordinates
are unchanged, and measurement/display geometry remains complete. Reports retain the selected
roots and synthetic cap count in `body_contact_proxy` (inside the study body report for studies).
This supports the scoped clothing calculation, not facial/hand/foot contact or physical fit.

**Declared simulation coverage.** `garment::simulation_scope` validates all instance and seam
references and requires explicit placement on each instance whose `sim` is not false. Missing
placement refuses before production meshing, including disconnected panels. Explicit exclusions
remain legal: the report retains source-ordered simulated and excluded identities, affected seams,
and a `declared_subset` advisory. The native command and portable fold check before body fitting;
studies check before charging mesh work. The low-level `build_garment` keeps reference selection.
This establishes coverage of the authored document, not completeness of an actual garment.

Requested girth metrics require every named contributor, a finite level and sampled ring edges
from each. An unavailable contributor produces an `UNMEASURABLE` landmark with reasons and a
gating flag; no partial circumference reading is published. This gate is independent of tightness
qualification and also applies to deliberately excluded contributors. Complete-ring arithmetic
and fields are unchanged. Explicit physical limb sides select the same landmark frame as placement
and flat validation. Direct synthetic regressions live in `garment.rs`, `metrics.rs`, `run.rs`
and the native CLI tests; none establishes physical fit.

**Numerical acceptance** (`ftx-6en`). Every Newton solve, including friction lag rounds, must converge;
iteration exhaustion or a failed line search returns `not_converged` with the residual (N), last
correction (m), iteration count and repair suggestions. A refused step or static solve preserves
positions, velocities, contact lag and linear-solver caches. `PhaseReport.samples` retains the accepted
steps' Newton diagnostics and instantaneous maximum vertex speeds. Native drape reports expose these
under `numerical`; gravity draping additionally requires three consecutive accepted steps at at most
1 mm/s, otherwise a numerical-settling gating flag propagates to CLI, round and browser consumers.
The fixed phase schedule is not automatically extended. Sewing-only qualification reports numerical
settledness as unavailable. The reference's top-level `settled` and other oracle metrics retain their
legacy window-mean definition; they do not override the numerical gate. Neither criterion certifies
static stress equilibrium, material-history relaxation or real-person fit. Historical timings and
verdicts below predate this stricter acceptance boundary and require fresh production-model checks.

At unresolved floating-point energy differences, a failed Armijo trial may use approximate Wolfe
slopes only for a full, domain- and CCD-admitted step. Both the predicted decrement and absolute
energy discrepancy must be at most `16 * EPSILON * max(abs(old_energy), abs(new_energy))`, an explicit
numerical error estimate. Finite free-gradient infinity norm must strictly decrease. This admits
another Newton iterate, never early convergence or a looser correction tolerance; timed strips
retain their separate force gate. `SolveReport.roundoff_steps` counts these steps through lag rounds,
and actual energy values remain visible. Scalar rejection tests, nominal-density 4%/8% cyclic strip
regressions and existing contact/history rollback tests cover the correction. See
`docs/material-parallel-memory.md` for the primary reference and limitations.

**Accepted contact observations** (`ftx-onn.17`). Every accepted phase endpoint retains separate
body/self primitive-pair counts, the minimum active thickness-offset gap with its pair and effective
offset (m), and frozen friction-lag counts. Disabled domains and absent active gaps remain unavailable.
The initial endpoint and final sampled running minima are retained; these are activation-range samples,
not an exact minimum over continuous motion. A failed step publishes no sample. The final indexed
welded-vertex map contains barrier reactions in N from the existing exact energy gradient, including
edge mollifiers and excluding Coulomb friction. Geometry/topology, active policy and material-report
identities accompany it. No physical contact area is inferred, so pressure in Pa remains unavailable.
Each elastic casing also reports signed and peak sampled mean-height displacement relative to the
static body during the declared gravity phase, with duration and sample count. This window includes
settling; it is not contact-point slip or a stationary hold experiment. The ideal-ring grip estimate
remains separate. These outputs establish no measured pressure, comfort or real-person fit.
An explicit `elastic_hold_steps` (CLI `--elastic-hold-steps`, also forwarded by `round --sim`)
selects the last N accepted gravity intervals without adding steps or changing physics. N may not
exceed `drape_steps`; zero requests no hold window and is omitted from serialized scenario options.
The report retains duration, step indices, sampled displacement, maximum vertex speed and whether
every accepted endpoint is at most 1 mm/s. A moving window remains visible with false quietness;
it cannot become a stationary hold, stress-equilibrium or fit claim by selecting it.

**Attached trim/accessory mass** (`attached_mass`). Optional source-qualified total groups bind named
physical pattern corners through the actual seam weld map. Explicit shares add once to sheet masses,
entering both gravitational load and implicit inertia in sewn drape, static solves and prescribed
motion. The receipt separates simulated cloth, attached and total inertial mass and retains exact
source/target mappings and arithmetic residuals. Sheet thermal storage remains source/reference-area
based; no accessory geometry, thermal properties, contact or attachment strength is inferred. Stale
inertia/reference/topology refuses before accepted state or receipt publication. Absent/empty input
preserves the existing path and wire output. Contract: `docs/attached-mass.md`; tests:
`tests/attached_mass.rs` (independent static/dynamic balances, welded/mirrored mapping, substitution,
thermal separation, deterministic replay and motion rollback).

**Fixed retained-liquid inertia** (`ftx-vc2.2`). The canonical material mechanical mass consumer
qualifies and sums dry substrate plus closed co-moving liquid per reference area. Existing source
piece triangle assembly distributes each contribution once through the real weld map; static
gravity, implicit inertia, casing loads and prescribed body motion retain that constant mass.
Timed clamped strips use the same consumer and report the separate source densities. Accessory
mass remains additive once. Dry thermal/vapor and changing-water histories cannot silently reuse
the wet state. No variable mass, liquid slip or unmeasured wet constitutive change is inferred.
Tests: `attached_mass`; source contract: `docs/attached-mass.md`.

**Evaporating liquid inertia** (`liquid_mass`, bounded `ftx-vc2.3` slice). An independent
`ftx-evaporating-liquid-mechanics/1` qualification binds the complete material/interface report
and every retained instance's exact material and temperature/loading domain. Reference triangle
liquid distributes once through the real weld map on top of dry sheet and attached accessory mass.
`Solver::evaporate_liquid` removes only nonincreasing co-moving liquid: velocity is preserved,
while exported momentum, bulk kinetic energy and signed gravitational energy retain separate
transfer residuals. Those mechanical terms are not latent or sensible thermal energy. Source,
mass, topology, constraints and frozen-law mutations refuse before changing state; pinned DOFs,
already-fixed retained liquid and incoming/draining liquid are outside this closure. Numerical
cache invalidation preserves symbolic structure and accepted contact/material histories. Tests
cover analytic mass/momentum/energy transfer, free-fall invariance, variable-mass spring time-step
convergence and actual sewn accessory ownership. These establish numerical behavior, not a
measured wet law or garment fit.

Optional `motion.transient_surface.mechanical_coupling` integrates this ledger with initial-only
surface water `/1`. Each fully accepted mechanical interval evolves heat/water on its accepted
geometry, removes co-moving vapor, then uses the new mass for the next interval. The qualification
must cover the full thermal loading and pressure-table temperature domains. Initial liquid is
co-moving at the end of the dry drape, not a silently inferred wet equilibrium. The first-order
split requires mechanical time-step refinement independently of thermal subdivision. Heat failure
refuses coupled motion and restores the pre-admission solver; it cannot publish a dry fallback.
Native/portable named-case studies and comparison admission preserve exact source, initial/final
inventory and every accepted transfer, with bulk mechanical energy separate from thermal enthalpy.
See `docs/textile-surface-water.md`, `tests/material_scenario/liquid_mass.rs`, the private
late-failure rollback test and the CLI accepted-body surface-water workflow.

The separate `ftx-coupled-liquid-mechanics/1` / `co_moving_mass_exchange_frozen_mechanics/1`
qualification extends the SAME frozen binding and accepted-step seam to water `/1`, `/2` and `/3`.
Exact thermal supplied/condensed/evaporated triangle increments update reference-owned lumped
inertia and gravity for the next solve. Both incoming and outgoing mean velocity equal local
accepted cloth velocity; no impulse from an unspecified jet, rain or vapor speed is inferred.
Per-triangle balance, complete source/interface invariance, domain containment, unconstrained DOFs,
attachment integrity and representable positive transfers are required before atomic acceptance.
`exchange` receipts add initial and incoming momentum, bulk kinetic and signed gravitational
energy to the existing outgoing ledger. Dry cloth and accessory receipts remain unchanged, with
`exchanging_liquid_mass_kg` separately accounting for water. Full-path failures restore cloth,
thermal/water state, material histories and numerical caches together. Tests cover growing-mass
analytic convergence, simultaneous nonuniform flow, next-substep feedback, condensation/re-evaporation,
late failure with attached trim, and actual native/portable named-case objective admission.
Relative outgoing flow velocity, noninvariant wet laws and measured wet-garment qualification
remain outside this bounded coupling.

`ftx-coupled-liquid-mechanics/2` additionally qualifies one finite incoming world velocity per
retained instance. The same accepted thermal flows mix inelastically with previous welded mass
before outgoing evaporation leaves at the mixed velocity. Actual solver velocity and inertia
therefore change even for equal-net throughflow. Incoming momentum/kinetic energy, outgoing bulk
energy and nonnegative `mixing_dissipation_j` remain separately conserved; no mixing work is
deposited into the thermal sensible/latent ledger. Initial liquid stays co-moving after dry drape,
and dry/accessory ownership, exact source laws, domain checks and whole-motion rollback remain
mandatory. Older models reject the velocity field and preserve their existing behavior.
Independent control-volume accretion/refinement, welded multiple-stream conservation, Galilean
invariance and sewn/accessory replay tests cover this bounded numerical capability. Prescribed
velocity applies to all incoming phases for an instance.

`ftx-coupled-liquid-mechanics/4` instead requires separate constant finite `supply` and
`condensation` world velocities in each binding's `incoming_stream_velocities_world_m_s`.
The shared version-2 velocity field is forbidden, and older versions reject the new field.
Accepted thermal increments retain their phase identities until lumped at welded vertices; both
streams mix once with all previous dry/accessory/liquid inertia before evaporation leaves.
Phase mass must reconcile to aggregate incoming mass before mutation. Opposing momentum does
not cancel incoming kinetic energy or nonnegative mixing loss. Result schema `/4` retains the
qualification and existing separate mechanical ledger. Independent known answers, Galilean and
whole-motion rollback tests and native/portable named-case replay cover this numerical closure.
Phase-specific time-varying streams, pinned-coordinate impulses, heat deposition and
empirical wet-garment claims remain open.

Published schema `ftx-coupled-liquid-mechanics/3`, model `scheduled_incoming_velocity_mass_exchange_frozen_mechanics/1`,
qualifies an explicit common incoming world velocity for every
mechanical keyframe interval and retained instance. Each accepted substep uses its enclosing
interval's vector, including after thermal event subdivision. Exact schedule coverage, finite
vectors and exclusive constant/scheduled fields are required before admission. The same welded
mixing law and mass, momentum, energy, source and rollback checks remain authoritative. Changing
directions affects actual solver velocity and the next cloth step. Complete schedules survive
native/portable study receipts, comparison protocols and browser reports. No interpolation,
rain direction, separate liquid/vapor velocities or new wet-material qualification is inferred.
Phase-specific streams stay on schema `/4` and result `/4`. Scheduled intervals stay on schema `/3` and result `/3`. Their fields cannot be combined. Each result retains the full exact qualification.

All four incoming exchange models additionally admit explicitly qualified water `/4` drainage.
Accepted thermal increments preserve evaporation and liquid drainage separately while their sum
updates reference-owned inertia and exports co-moving momentum, kinetic and gravitational energy
after incoming mixing. Optional `drained_liquid_mass_kg` (full) and `drained_liquid_kg` (summary)
include initialization zero only when drainage is admitted. Evaporation fields retain vapor mass;
`exchange.outgoing_mass_kg` includes both outgoing phases. Missing/malformed phase ledgers refuse
atomically. Evaporation-only qualification still rejects drainage. Independent conservation and
whole-motion late-refusal replay cover the numerical closure, including scheduled and separate
liquid/vapor incoming velocities; these are not measured wet-textile qualifications.

**Scenario transient heat** (`study::heat`, plan §25.10). Optional independent-sheet and geometric
body/layer consumers follow every accepted prescribed-motion sample through the existing Surface
and conservative Frame/remap APIs. Time-zero temperatures and spatially uniform interval films are
explicit. Heat storage follows reference mass; changing coverage preserves covered/exposed material
enthalpy, with remap residual separated from physical heat. Without the explicit liquid
mechanical coupling above, thermal failures preserve complete mechanics and other consumers.
Complete histories retain time/geometry/source identities and final
fields; coverage includes a restorable checkpoint. Per-history/session work is bounded. Prescribed
films, qualified exterior exchange and independently opted-in membrane-yield/Maxwell-dashpot/bending-yield heating retain
separate source ledgers. Both thermal domains qualify their own conversion observations and sum selected
source heat once; mechanical work arrays are captured once per channel across the domains. Exact sample
admission uses retained sewn triangles, while conservative session reservations include all selected channels.
Bending work is integrated once over each source triangle's reference area, not its six-node stencil
extent; accepted per-triangle arrays retain explicit zeros for inactive triangles. Initial drape work
is excluded from every motion heat source. `docs/bending-heat.md` and `tests/relaxation_heating.rs`
cover source qualification, shared capture, covered/exposed conservation and independent refusal.
There is no inferred ambient state, moisture, thermal feedback, human
thermoregulation or empirical warmth claim. Schemas, accounting and
limits are in `docs/material-scenario-studies.md`; production regressions in `material_scenario.rs`
compare every sample against direct solvers and independent reference-mass heat accounting.

**Garment mesh** (`build_garment`, spec 04 §2). Simulated instances (`sim` not false, a placement),
matched seam samples at `spacing` (12 mm), boundary samples, the hex interior, constrained Delaunay inside
the outline, sewing pairs, seam records, corner and edge-index maps, then `sim_scale`. Triangulation is
`spade` 2.15.1 (allowlisted; closure hashbrown, foldhash, allocator-api2, equivalent, num-traits, robust,
smallvec; builds for wasm32-unknown-unknown); faces are kept by constraint parity from the outer face,
not by centroid (centroids of slivers along straight edges round onto the line). Against the reference
(Blender's `delaunay_2d_cdt`; `tests/fixtures/oracle/drape/garment_mesh.json`, seven garments): owners,
ranges, sewing pairs, seam records, corners and index lists exact; vertices 99.0 % bit-exact, worst
3.8e-15 relative (cubic samples, ftx-core); all 42,003 triangles identical. A self-crossing outline is
refused (`degenerate_geometry`); exactly coincident points merge (the reference merges within 1e-6 mm).
The digest of a two-piece mesh is identical native and wasm.

**Start state** (`start.rs`, `nearest.rs`; spec 04 §3–4). Isometric wrap per instance from a
`WrapPlacement` (part frame, anchor height, radius, anchor column; mirrored instances and the leg
own-side rule), gather weights, sewing groups, pre-sew (group means pushed 8 mm outside the body,
harmonic spreading, push-out to 4.8 mm) and blockwise relaxation to p95 plain-edge strain ≤ 12 %, against
any body given as a `NearestTree` (an AABB tree; equidistant crease triangles resolve to the
best-aligned normal). Against `tests/fixtures/oracle/drape/start_state.json` (92 cm bolero and trousers,
the reference's dress form as triangles): the wrap with the reference's frames and gather weights to
1.1e-15 relative (91 % bit-exact); pre-sew, relaxation and the whole start state agree with the
reference's own functions run in float64 with an exact nearest-point query to 1e-14–1.9e-12 m, with the
same relaxation blocks. Blender's float32 BVH (the reference as shipped) breaks crease ties its own way
(2 of 128 bolero groups, 5 of 295 trouser groups); there the start differs by up to 19 mm and the bolero
relaxes for two blocks instead of one.

**Physical limb placement** (native extension to spec 04 §3.4). Explicit `placement.side: R|L`
on a quantity-one, non-folded arm/leg panel selects that side's frame, landmarks and section radii.
Flat geometry and grain are unchanged; no generated-instance reflection is applied. A properly
handed frame preserves public normals through both wrap and partial roll. Completed leg wraps
retain the own-side rule, with x ≤ −1 mm on the right and x ≥ +1 mm on the left; partial rolls
remain unclamped. The new `WrapPlacement::own_side_left` is false on all legacy placements.
Material-study receipts retain it when true. Existing pair, fold and absent-side behavior is
unchanged. Direct scene tests cover asymmetric frame providers, selected anchors/radii, actual
imported sewn geometry, public normals, roll endpoints and both leg constraints.

The `BodyFrames` adapter uses each explicit limb's actual posed joints and skinned region. Left
arm shoulder/wrist offsets and radius, and left leg axis, ankle/calf/knee levels and section radii,
are independent of the right limb. Both frames retain `lat = up × front`. Generated mirror pairs
continue to use the reference's right-then-reflect convention. Synthetic posed-model controls
exercise unequal left/right angles, positions, lengths and radii through the ordinary mesher,
placement and roll, plus a symmetric case and unchanged generated-pair output. These establish
geometric side selection, not anatomical landmark, convergence or physical-fit qualification.

**Scene** (`scene.rs`; plan §11.3–11.4). A pattern meshed, placed with a body's part frames (any
`PartFrames`; `ftx_body::BodyFrames` reads them off a posed Anny body), started (wrap, pre-sew, relax
against the drape body), untangled, welded and handed to the solver with contact over the body:

Explicit `meta.solved.elastic_casings` retain 1–64 independent casing instances, positive relaxed
lengths in metres and physical `load_instances`. Joint composition namespaces these declarations
from each source's original cut-minus-overlap length without changing source geometry or metadata.
Each casing keeps two existing tension-only rings and its own gathered compression. Its grip report
uses that instance's body friction and unique welded load-domain mass, including attached inertia;
capacities are not summed and load sharing is not inferred. Frozen composed patterns replay through
ordinary drape and study meshing. Unselected bands retain the caller's assumed EA; an optional frozen
band selection supplies its own source-qualified stiffness (see `docs/elastic-band-materials.md`).
Ambiguous legacy/explicit metadata, duplicate/inactive targets and malformed load scopes refuse.
EA zero validates explicit declarations but disables rings and cut-length restoration. The original
single-waistband convention and singular `elastic` report remain unchanged; explicit cases report
`elastics` separately. Tests: `outfit` (two actual sewn casings, analytic mass/rest lengths, independent
friction, attached weight, disabled behavior, frozen replay and drafted jacket/trouser composition).

- Rest coordinates per piece in metres with the grainline along +u; welding merges each sewing group into
  one vertex and keeps every triangle's own rest coordinates, so eased and gathered seams are strained,
  not rewritten. Fabric from `pattern.fabric` over the reference's defaults (spec 04 §5.3); membrane
  E1 = E2 = tensile, ν = 0, G = 0.2·tensile; compression as stiff as tension by default (the reference's
  `tension = compression = E_t·L`, spec 04 §5.3) and in gather zones softer, down to 3 % at a gathered
  seam (the reference's `GATHER_COMPRESSION`), linearly in each element's mean gather weight
  (`ClothModel::set_compression`, the reference's vertex-weighted structural stiffness). An earlier
  default softened compression to 5 % everywhere: against it, on the tape-fitted toddler, the bolero
  took 436 Newton iterations instead of 808 (none unconverged) with every fit reading within 3 mm and the
  same verdict, and the trousers 838 instead of 924 with their bands within 3 mm and the two crotch-seam
  "distorted" advisories gone (p90 strain 4.8–4.9 % instead of 9–10 %: crowding of near-free
  compression).
- Untangling: crossings the exact audit still finds after relaxation are removed by moving the crossing
  cloth edge's ends (or the pierced cloth triangle) 2 mm above the crossed body triangle (or ridge), in
  rounds of up to 50; a piece wholly inside the body is refused.
- Offset ramp: when an edge of the start passes closer to the body than the cloth offset ξ, contact
  starts at half the smallest distance and `run_phase` raises ξ by 0.25 mm a step while every gap stays
  above ĝ/10.
- Elastic casing: a drafted casing (the one-instance `waistband` piece) is simulated at its cut length
  (`sim_scale` 1, gathered: compression at 3 %) and drawn up by two rings of `ElasticSegment`s, along its
  fold (`top`) and along its seam line, each with half of EA = `ELASTIC_EA` (15 N,
  `DrapeOptions::elastic_ea`; 0 simulates the casing at its finished size as the reference does), each
  relaxed at `meta.solved.elastic_cut_mm` − the overlap: an elastic in a narrow channel gathers the whole
  channel (a ring on the fold alone left the seam line near its cut length, 639 of 704 mm on the 92 cm
  trousers). The report's `elastic` field gives the relaxed and worn length (the mean of the rings, and each
  ring), stretch, tension (the rings' sum), grip `2πμT` (KAT 4's law), the garment's weight and `holds`
  (grip ≥ weight); an elastic that does not hold is an advisory flag. KAT 7 (`tests/kat_elastic.rs`) drapes
  the 92 cm trousers (1.08 N) on the tape-fitted toddler with the elastic at 0.85, 0.95, 1.0 and 1.2 × the
  530 mm waist, at μ 0.3 and 0.5: it ends 22.9 %, 12.9 % and 9.1 % stretched with a grip of 6.7, 3.8 and
  2.7 N at μ 0.3 (11.1, 6.4 and 4.6 N at 0.5) and the waistband 12–15 mm below where it was drafted; at 1.2
  it grips 0.13 N (0.21 N; flagged) and the waistband rests 14 mm down on the hips and crotch, within a
  millimetre of the snug 1.0 elastic (the test allows 1 mm: there the hips, not the grip, stop it). Without
  the elastic the waistband sits 6.5 mm down: a casing gathered from its cut length hangs lower than one cut
  to the finished size.
- `run_phase` runs implicit steps under a gravity (the reference's sew then drape).
The drape body (`ftx_body::trimmed_skin`) leaves out the hands and feet; with Anny's bent elbows a
straight sleeve crossed the forearm, so the drape pose straightens the arms (`Pose::drape`). The drafted
92 cm bolero builds a clean start on the 92 cm Anny toddler (no crossings, 600 relaxation iterations,
p95 strain 0.115). Speed is the open problem. The first sewing steps take up to 100 projected Newton
iterations per solve (full steps creeping through soft wrinkle modes after the CCD-bounded first
twenty); profiled on 4 sewing steps, the numeric factorization took 36 % of the time, the Jacobi
eigendecompositions of the PSD projection 31 %, contact queries 8 % and assembly 9 %. Two changes
since, measured interleaved against the previous build on 6 sewing steps of the bolero (same 347
iterations): skipping the projection of element Hessians that are PSD by construction (66–75 s → 56 s)
and deferring the factorization's panel updates (→ 45–50 s); assembly now fills the fixed pattern in
place (bit-identical, no triplet sort). With compression at the reference's stiffness (above) the whole
bolero drape takes about 70 s and the trousers about 7.5 minutes; on the body fitted to 15 tape
measurements (ftx-body), 50 s and 5.4 minutes; without the friction lag round while sewing
(`DrapeOptions::lag_rounds` = [0, 1], the default) 29 s and 2.4 minutes: the bolero 312 Newton
iterations instead of 448, the trousers 318 instead of 680, the same verdicts (no flag, settled),
readings within a few millimetres (bands within 3.7 mm, hem rise within 6 mm, hem balance −2.2 → 6.0 mm,
seam strain p90 within 1 %) — the run-to-run spread the reference itself shows. Contacts made during a
sewing step get friction from the next step on. On the body fitted to 21 measurements (neck, head,
shoulders, crotch length and outseam added) the verdicts are unchanged (no flag, settled) and the work
grows: the bolero 374 Newton iterations, the trousers 563 (one sewing step unconverged, the drape
converged). Newton to 1e-3 m/s with the inexact Newton below (the defaults since 2026-10-13) brings them
to 11 and 83 CPU-seconds on an M4 Pro (bolero 175 iterations, 41 factorizations; trousers 334, 108;
measured as CPU time on a heavily loaded machine), the same verdicts and readings within 0.3 mm; the
vectorized factorization (below) then 8.3 and 59 s, bit for bit the same drape; the translation-free
projection and the backward solve by partial sums 6.7 and 36 s, the same iterations and every reading
equal at the report's precision. In the browser build (wasm32-wasip1 under node, one thread) the
solver's multiply-adds are what cost: wasm has no fused multiply-add, so every `f64::mul_add` was a call to
the software `fma`, 68 % of a bolero's time (plus 7 % in its `scalbn`). With the kernels written as a
multiply then an add, `ftx drape` takes 14.7 CPU-seconds for the bolero (was 62) and 53 for the trousers
(was 454; §11.7's browser budget is five minutes), the same Newton iterations; native 9.8 and 37 s, unchanged.

**Units and frames.** Metres, kilograms, seconds; z up. Rest coordinates are the flat piece in its material
frame (+u along the grain).

**Energy.** `E(x) = ½h⁻²(x − x̃)ᵀM(x − x̃) + Ψ_membrane + Ψ_bend + Ψ_elastic + B + D`, `x̃ = xₙ + h vₙ + h² g`
(B, D: contact barrier and friction, below; the static solve drops inertia and friction).
- Membrane: StVK on Green strain with plane-stress resultants Q (N/m): warp E1, weft E2, Poisson ν12,
  bias G12. Each normal strain is softened under compression by
  `φ(e) = r·e + (1 − r)·δ·tanh(e/δ)` (r = √s, δ = 1e-3): C² at zero strain (a plain kink made Newton cycle
  100 iterations a step on a sagging sheet; the blend converges in 1–2) and stiffness → s·Q in compression.
- Bending: Bergou's quadratic hinges, weight `3a/(A₀+A₁)²` with each triangle's area handed to its interior
  hinges; constant PSD Hessian.
- Elastic (`ElasticSegment`): a tension-only spring per segment, `(EA/r)·g(l − r)` with g zero below
  −δ, cubic across [−δ, 0] and `e²/2 + δe/2 + δ²/6` above (δ = `ELASTIC_ONSET` = 0.1 mm): C², no force
  when slack, so a casing's elastic only ever pulls it in; PSD Hessian.

**Solver.** Exact Hessian first; when the LDLᵀ inertia shows a negative eigenvalue, element blocks are
projected to PSD (negative eigenvalues zeroed) for the rest of that solve (retrying the exact Hessian
cost a second assembly and factorization nearly every iteration on a garment); steepest descent if even
that is singular. A block that is PSD by construction is not projected (`Local::psd`): every hinge (a
Gram matrix), and a membrane whose geometric stress matrix [[σ₁φ₁', σ₁₂], [σ₁₂, σ₂φ₂']] is PSD with no
negative softening term σᵢφᵢ''. Projecting those had only rebuilt them from Jacobi eigenvectors after a
rounding-level negative eigenvalue: the change moved the bolero's positions by at most 9e-13 m, left its
report identical and re-pinned the contact digest (native and wasm agree). Tried on the bolero and not kept: flipping negative eigenvalues instead of zeroing them
(more iterations), and a membrane Hessian with its stress clamped to the non-negative part instead of
eigen-projection (cheaper iterations, 2.7× as many in the drape, slower overall). Element blocks
(membranes, hinges, elastic segments) depend on their vertices only through differences, so every
rigid translation is in their null space: they are projected on its complement (`eigen::
project_psd_translation_free`: a Helmert basis ⊗ I₃, a 6 × 6 Jacobi for a triangle instead of 9 × 9),
the same projection in exact arithmetic (a unit test compares the two) at about a third of the cost;
contact blocks, whose body side is fixed, keep the full projection. For body-only contact the symbolic analysis is computed once,
ordered by AMD on the vertex graph and expanded to the free DOFs (the pattern never changes: every
element block is always assembled). Per-DOF constraints (rollers, pins). Convergence: `‖p‖∞/h < tol`
(step) or `‖p‖∞ < tol` (static); a garment's default `tol` is 1e-3 m/s (`DrapeOptions`; 42 µm per step):
at 1e-4 Newton chased the cloth's tiny buckling modes on projected Hessians for twice the factorizations
(the 92 cm bolero 374 → 180 iterations, the trousers 563 → 334) with hems and bands within 0.3 mm and seam
strains within 0.05 % (the casing's 1 %). `SolveReport` and the drape's `PhaseReport` count the
factorizations, the iterations the CCD bound cut and the line-search halvings: on the trousers almost every
iteration is a full projected-Newton step (CCD cut 18 of 563, 32 halvings), and the factorization is 78 %
of the time (a samply profile), the eigen projection 11 %, contact queries 3 %.

**Inexact Newton** (`Solver::pcg_iters`, `DrapeOptions::pcg_iters`, default 20; 0 in `Solver::new`, so the
solver KATs factorize every iteration). The last positive-definite factorization is kept and preconditions
conjugate gradients (`sparse/precond.rs`) on later iterations and steps, to a relative residual of 1e-4
within the cap; only a failure factorizes anew. A search direction of non-positive curvature in the exact
Hessian means it is indefinite, so the solve goes straight to the projected Hessian without the
factorization that would have shown it; a converged direction is a descent direction (CG from zero on
positive curvature) even where the Hessian is indefinite elsewhere. A factorization costs about 250 CG
iterations: on the 92 cm trousers 349 factorizations become 108 plus 5,639 CG iterations, CPU 164 → 83 s
(cap 10: 116 s; 40: 87 s; 80: 91 s), and the bolero's 186 become 41, 18.7 → 11.4 s, every hem, band and
seam reading within 0.2 mm. Sewing still factorizes on about half its iterations: consecutive sewing
Hessians differ too much for a stale factor.

**Contact** (`contact.rs`, `distance.rs`, `ad.rs`; plan §11.2, §11.5). The body is a static triangle mesh
(closed for a real body; open surfaces are accepted for tests). Pairs: cloth vertex–body triangle, body
vertex–cloth triangle, cloth edge–body edge; each touches only cloth vertices that share an element, so the
Hessian's pattern never changes.
- Barrier `b(g) = −κ(g − ĝ)² ln(g/ĝ)` on the gap `g = distance − ξ` (ξ = half the cloth thickness), so
  clearance is measured to the cloth's real surface and no collider inset is needed. Edge–edge pairs are
  mollified near parallel (IPC's `(2 − c/ε)c/ε`, ε = 1e-3·|e_c|²·|e_b|² at rest).
- Derivatives: exact gradient everywhere; Gauss–Newton Hessian `b''∇g∇gᵀ` (PSD, exact for a point
  against a plane) for ordinary pairs; the full Hessian (second-order forward AD, `Jet<12>`) for
  mollified pairs, projected to PSD per block.
- Broad phase: a dense uniform grid in CSR form over the body's bounding cells (cell ≈ the body's mean
  edge length), queried per cloth primitive with sorted, deduplicated results, then an exact box-overlap
  test; no BVH. It replaced cells in `BTreeMap`s with the same candidates in the same order (both digests
  unchanged, the bolero's drape identical), about 15 % faster on the bolero in an interleaved A/B on a
  shared machine (25–30 s against 28–36 s).
- CCD: additive CCD (Li et al. 2021, Algorithm 1) caps each Newton step so no gap falls below a tenth of
  its value (C-IPC's s = 0.1).
- Friction: IPC's smoothed Coulomb `μλ f₀(|u|)`, ε = ε_v·h, with normal forces, tangent bases and
  contact weights frozen; a step solves once with the pairs of its start state, then re-solves after
  refreshing them at the new iterate (`friction_rounds`, default 1), so a contact made during the step has
  friction in the same step.
- Start audit: `with_contact` refuses a start with a non-positive gap, an edge of either mesh through a
  triangle of the other (exact orientation predicates, `ftx_core::exact`; coplanar cases fall to the
  gap check), or a cloth piece wholly inside a closed body (winding number above ½ at one vertex per
  piece).

**Self-contact** (`Solver::with_self_contact`, `DrapeOptions::self_contact`; FTX-12b / `ftx-d0h`).
Non-incident cloth vertex–triangle and edge–edge pairs use the same barrier and mollifier, with all
four vertices moving. A bounding-volume hierarchy over swept boxes (lifted from frankensim fs-query;
every overlapping pair, sorted, so the tree's shape never reaches the result) includes both surfaces'
trajectories; shared vertices (including welded seams) are excluded, edge pairs are unique, and CCD
uses relative motion on both sides. Self-contact has its own thickness (the sum of both layers'
half-thicknesses), coefficient of friction and smoothing. Body `min_gap` / `closest_pair` stay body-only;
`min_self_gap` uses the cloth–cloth offset, unaffected by the body's offset ramp.

**Start creases.** A garment's start has pairs one edge apart that already sit inside the thickness:
where a welded seam meets at a sharp angle (a back vertex 0.57 mm from the sleeve triangles at the
armhole) or where a gathered hem pulls a cuff into a crease (both trouser cuffs). Their barrier could not
be satisfied, and ignoring them outright let a seam crease fold through itself while sewing. So a
one-ring pair (joined by a cloth edge, not sharing a vertex) inside the thickness when self-contact is
enabled keeps its barrier at half its start distance: it cannot cross and needs no motion. Every other
pair keeps the full thickness, and the exact crossing audit covers every non-incident pair. On the
92 cm toddler at 2 mm (Blender's `self_distance_min` in the reference): the bolero has 14 start creases
(the smallest offset 0.007 mm), settles with no flag and every solver-state audit passed, closest final
cloth–cloth gap 1.8 mm, 21.8 CPU-seconds (9.8 without self-contact); the trousers 62 creases, settled,
no flag, audits passed, closest gap 0.09 mm, 56 s (37 without). `sim_report.json`'s `self_contact`
records `start_creases` and `start_crease_min_offset_mm`.

Self-contact assemblies use triplets; symbolic LDLᵀ analysis and the contact-aware vertex ordering
are rebuilt when the actual sparse pattern changes. Body-only runs keep the original fixed-pattern
assembly. Enabling self-contact audits the start for exact edge-through-triangle crossings and
non-positive thickness gaps (including coplanar overlaps). Every implicit/static solve audits before
and after; a failed solve/audit restores positions before returning a refusal. There is no automatic
cloth–cloth untangler. Self-contact-only simulations use `Body::empty()` through the solver API.

`ftx drape --self-contact-mm MM [--self-mu MU]` enables it on the welded solver mesh. It is off by
default, preserving body-only behavior for existing placements. The report explicitly distinguishes
`self_contact.audit = not_simulated` from an enabled, audited run, and records thickness, friction and
nearby final clearance. The kinematic fold/zip animation is outside that audit. Synthetic tests cover
brute-force candidate equivalence, two moving layers, force balance and relative friction, dynamic
couplings, cloth-on-cloth support, invalid starts and a connected guided folded strip (which crosses
in the no-contact control). Repeated folded-strip runs are bit-identical on the same build. These are
mechanics regressions, not calibrated fabric, real-garment fit, cross-target digest or performance evidence.

**Fold-up animation and files** (`animation.rs`; spec 04 §9). The pattern board (instances left to
right, 40 mm apart, on y = −0.55 m); per instance a staggered rigid flight off the board (slerp from the
identity to the rotation onto its k = 0 roll, centroid on a raised path) then the isometric roll-up
(`start::roll_points`, §3.6) to its wrap; the 18-frame zip from wrap to the sewn start; then the solver's
frames. Rotations use Horn's quaternion method with the deterministic Jacobi solver where the reference
uses numpy's SVD Kabsch. Written as `fold.pc2` (float32 point cache), `mesh_map.json`, seam and strain
colours, and the `fabric-fold-viewer/1` payload embedded in `assets/fold_viewer_template.html` (the
skill's page with offline module packaging). The page's verdict line shows the run's own gating
and advisory flags from `metrics`.

**Offline viewer dependency.** `viewer_html` embeds the existing Three.js 0.160.0 renderer and
OrbitControls as base64 data-URL imports, with the complete MIT notice in each HTML file. The
template requires exactly one payload, renderer, controls and license marker. It is shared by
native draping, `ftx viewer` and browser fold-up; numerical payloads and controls are unchanged.
No JavaScript loader API or external resource is used by this view. The unmodified primary-source
files live in `assets/three-r160/`, pinned to the upstream `mrdoob/three.js` **r160** tag:

| File | Primary source | SHA-256 |
|---|---|---|
| `three.module.min.js` (670,681 bytes) | [build](https://raw.githubusercontent.com/mrdoob/three.js/r160/build/three.module.min.js) | `3e690ac7d180b0aadf0891bea39eec643e29e2d3e75c99b18689518665f69ba6` |
| `OrbitControls.js` (29,868 bytes) | [controls](https://raw.githubusercontent.com/mrdoob/three.js/r160/examples/jsm/controls/OrbitControls.js) | `5a44a9e86a2a0fb11933eed69bc2cd33c76a496854c1aed6ed776efa87d7b064` |
| `LICENSE` (1,081 bytes) | [notice](https://raw.githubusercontent.com/mrdoob/three.js/r160/LICENSE) | `852e0e8699169bf9f6fdc6bda3e682d078dcbc738b5d33e74df594721bff271d` |

Used APIs remain buffer geometry/attributes, meshes, lights, camera, WebGL renderer and orbit
controls. The module closure is those two files: OrbitControls imports only `three`; the core module
has no imports. This removes demonstrated offline failure without adding a crate or changing the
renderer version. `tools/check_web.cjs --viewer HTML` executes offline file rendering, frame changes
and controls. Native/WASM build outcomes and actual artifact execution are recorded in the reality-check
follow-through; this dependency change is not numerical or empirical cloth evidence.

**Metrics and flags** (`metrics.rs`; spec 04 §6). From the baked frames, the wrapped start and the true
body: seam gaps after sewing and at the end, seam strain; ring strain, contact and clearance at each
fit-spec girth level (TIGHT, SNUG or EASY); contact, penetration and corner slip per piece; hem heights,
rises and balance; band shifts; drift-based settledness; global strain; with the reference's exact flag
texts, their order and the gating, advisory and tightness-gating split. Against
`tests/fixtures/oracle/drape/metrics.json` (the reference's `metrics()` on four synthetic sequences built
from the exact start states, rebuilt bit for bit in the test): 443 numbers bit-exact, all 51 flags
identical. On a body other than the reference's dress form the level heights come from `BodyFrames`
(`examples/drape_drafted.rs`), and the true body is the drape body the cloth collided with (no hands
or feet).

**Meshes.** Use `hex_grid` (near-equilateral) for anything that bends: hinge bending on it is isotropic
(cylinder energy 0.98–1.02 of theory at 0°/30°/45°/90°). Right-triangle grids (`grid`) have a zig-zag mode
along clamped edges that made a cantilever ~25 % too soft; they are fine for in-plane tests.

**Known answers** (`tests/kat.rs`, plan §11.9): bending scale on a cylinder (0.99 at 2.5 mm spacing,
O(h)); KAT 2 membrane anisotropy (uniaxial moduli along warp, weft, 30° and 45° within 4e-5; compression
law exact to 1e-6 across the blend; rigid rotation stores nothing); KAT 3 Peirce cantilever (tip within
1.5 % of l of the exact heavy elastica at 1 mm spacing, ASTM droop 42.6° vs 41.5°, B recovered +1.4 %;
release only); KAT 9 for a small drape (identical SHA-256 native debug, native release and wasm32-wasip1
under Node). Element gradients and Hessians are checked against finite differences.
Contact (`tests/kat_contact.rs`; welded `hex_tube` bands on surfaces of revolution, ĝ = 0.2 mm,
ξ = 0.5 mm): KAT 1 barrier (zero with zero slope at ĝ, every pair type's gradient and Hessian incl. the
mollified one against central differences; unit test); KAT 6 ease sweep on an 80 mm tube (−10 and −20 mm
ease rest within ĝ + 0.15 mm of the offset surface, min gap > 0, hoop strain within 0.003 of
(2π(R + ξ) − C)/C; +30 mm keeps its clearance to 0.014 mm); KAT 4 ring on a cylinder (2πμT = 2.53 N
at μ = 0.4: at 0.9 of it the ring moves 0.012 mm, at 1.1 it slides 3.0 mm in 20 ms); KAT 5 ring on a
cone (tan α = μ/2 holds, +0.11 mm; tan α = 2μ slides 42 mm); KAT 9 for contact (a frictional band
digest, identical native debug, release and wasm32-wasip1). KATs 4–6 are release only (26 s; 140 s
unoptimised). Bodies and bands are built with `det::sin_cos`: the platform's `sin`/`cos` made the wasm
digest differ.

Fitted bodies (`tests/kat_fitted.rs`; the toddler fitted to the reference wearer's 21 tape readings, release,
needs the Anny model): KAT 8 armpit wedge, the drafted bolero at 80, 92 and 116 cm with the arms at 35°: no
gating flag, settled, every hem within ±15 mm of its drafted level (the 80 cm front hangs 13 mm higher than
its back, an advisory: the belly lifts it); KAT 10 two starts, the 92 cm bolero sewn in 12 and in 18 steps:
the same minimum, every hem, clearance and seam strain equal at the report's precision; KAT 9 the whole
pipeline (tape fit, scene, three sewing and three draping steps at the shipped settings) by one SHA-256,
identical native debug, release and wasm32-wasip1 (re-pinned when `ftx_core::np::interp` became a fused
multiply-add, as the reference's numpy computes it: the garment mesh moved to 99.9 % bit-exact). 79 s
release for KATs 8 and 10.

**Lifted code** (frankensim `93fc9b26`, MIT + rider, provenance headers): `sparse/` (CSR assembly and the
multifrontal LDLᵀ with AMD and inertia; adds `analyze_with_permutation`; assembly orders triplets by a
row-bucket pass instead of a global stable sort, the same order; the numeric factorization applies the
panel block's Schur updates after the fully-summed columns, column by column and in pivot order, keeping
only its lower triangle: bit-identical, both KAT digests unchanged; the dense kernels then became slice
loops over disjoint columns, which the compiler vectorizes, and the fully-summed block's upper triangle
is mirrored from the lower one only when the pivot search goes past the current column: factorization
2.9× and solve 1.3× faster, bit-identical on positive definite and shifted indefinite systems with 2×2
and delayed pivots, every KAT digest unchanged; then the backward solve sums each column's panel rows
first by four partial sums, 2.1× faster with different rounding: the three KAT 9 digests re-pinned,
each identical native debug, release and wasm32-wasip1), `eigen.rs` (`jacobi_eigh`);
`sparse/precond.rs` (`pcg`, from fs-sparse at `6eb72a0c`: any preconditioner closure, starts from zero,
stops with the reason on non-positive curvature or the cap). The lifted kernels' `mul_add`s (LDLᵀ updates
and solves, `dot4`, SpMV, PCG, the Jacobi rotations) are now a multiply then an add, the same on every
target and 8.5× faster in wasm (above): the three KAT 9 digests re-pinned again, each identical native
debug, release and wasm32-wasip1; KATs 1–8 and 10 pass unchanged. `ftx_core::det` keeps its fused
multiply-adds, which its correctly rounded functions need.

**Error model.** `Refusal`: `degenerate_geometry` (bad triangles, non-manifold edges), `out_of_range`
(non-positive density or step), `non_finite` (state blew up), `not_converged` (symbolic analysis failed),
`intersecting_start` (the cloth starts on or through the body or another cloth layer). A stalled
Newton step refuses with `not_converged` and leaves the accepted geometry, velocity, contact and
material history unchanged. Accepted `SolveReport` diagnostics remain in the phase report;
the separate numerical settling gate is described above.

Self-contact's digest (the folded strip after twelve steps, `tests/self_contact.rs`) is identical native
debug, native release and wasm32-wasip1.

**No claims.** Self-contact matching Blender's self-collision response (only its distance is mirrored);
a moving body; an elastic's own mass or its friction in the casing; fabric parameters (they are inputs;
plan §11, research 07 §2.7). The §11.7 budgets are measured above, on one machine (native and node's
wasm), not on a browser's.

**Standalone thermal requests** (`thermal::request`). Source-qualified point evaluation and dry
layer heating/cooling use one request authority for both native commands and portable/browser APIs.
Successful transient results retain `replay_request_json`, serialized by Rust from the exact settings
and canonical frozen layer selections. Initial temperature, independent estimate policies, interfaces,
films, exterior observations and integration/trace controls survive empty-catalog replay, including
negative zero. Expanded frozen input over 64 MiB refuses before integration. This adds no solver,
coefficient, temperature extrapolation or physical validation claim. Tests: portable
`transient_thermal`, actual native `nist_thermal_workflow` and the generated-WASM workbench regression.

**Adaptive compressed thermal holds** (`thermal::compression`, `ftx-cp2.1`). Optional explicit
time-integration tolerances reuse `thermal::adaptive` in both monotone and qualified cyclic histories.
Actual accepted-half-step boundary heats enter the separate thermal ledger; final fluxes remain
instantaneous. One request-wide budget charges attempted/rejected solves across holds and reversals,
at most 262144 cell solves. Physical-state rollback does not refund an external adaptive budget.
Omission preserves original fixed-step serialization/arithmetic and source qualification. Local
temperature/heat error estimates are not global/spatial error bounds, empirical material evidence,
mechanical heating, a recovery law or garment/comfort qualification. Tests: `compressed_thermal`
and `compressed_thermal_path`; consumer contract: `docs/compressed-thermal-history.md`.

**Prescribed pressure reaches loft and heat** (`thermal::compression`, history schema `/3`). Each
hold explicitly selects pressure or clearance while sharing the existing conditioned materials,
reference-mass cells, conservative heat step and source-qualified branch joins. The forward pressure
path admits thickness plateaus without inventing an inverse; its preload clearance is checked and
the final loft is an output. All accepted temperatures/work/heat histories and one adaptive budget
continue through mixed controls and qualified reversals. A failure rolls back the full physical
state. Reports distinguish prescribed pressure, including zero, from inferred pressure and preserve
ordinary frozen layer selections. Original `/1` and `/2` behavior is retained. Analytical layer/work
and RC tests, plateau/direction/source refusals, rollback and actual native/portable consumer replay
cover the software composition; no inferred garment contact pressure, physical compression recovery,
mechanical heating or empirical clothing warmth follows. See `docs/compressed-thermal-history.md`.

**Explicit thermal coverage** (`thermal::surface::coverage`, plan §25.6 / `ftx-tmb`).
`Coverage` composes the existing source-locked independent-sheet `Surface` into declared disjoint
body-area fractions and inside-to-outside layers. It checks both body and cloth allocation budgets,
consistent layer identities and acyclic layer order. Uncovered body fractions use explicitly supplied
films; unallocated cloth is reported and excluded. Temperature state follows allocated reference
cloth mass while resistance uses current cloth area; films/interfaces use covered body area. The
existing constant and temperature-dependent column solvers are reused without modifying material
observations. Native field calculations are in metres, watts and joules. Snapshot replay has no
catalog lookup; material/topology edits invalidate state, while pose changes do not. Tests:
`thermal_coverage` (analytic series/parallel networks, independent enthalpy and moving-area answers,
partition invariance, input refusals, source-locked replay). The added direct `serde` dependency is
the already pinned workspace 1.0.229 with std/derive, used only for layout/report serialization;
no additional package/version or transitive dependency is introduced.
For authored layouts alone, there is no claim of geometry-derived overlap or spatial disjointness.
Neither layout mode establishes air-gap transport, in-plane conduction, tissue storage, human comfort
or measured garment validation.
The `ftx-materials thermal-coverage` consumer verifies saved drape/frame identity before reading
indices, retains declared body geometry and mapping, and emits regional/bare heat and bounded
transient histories. See `docs/thermal-coverage.md` and the actual sewn-drape CLI regressions in
`crates/ftx-cli/tests/thermal_coverage_workflow.rs`.

**Geometry-derived vapor coverage** (`vapor::coverage`, `tests/vapor_coverage.rs`). Frozen retained
material selections and current body/cloth geometry feed the same projection/order/allocation kernel
without requiring dry resistance or heat capacity. Each disjoint covered region invokes canonical Ret
transport with actual allocated layer areas and explicitly supplied vapor interfaces and films.
Covered potential-flow subtotals exclude unknown uncovered body fractions; no sweat, latent-heat
coupling, comfort or measured garment claim is made. Complete source geometry and exact offline replay
are bounded; missing Ret refuses only this consumer. See `docs/vapor-coverage.md`.

**Finite body wet film** (`vapor::coverage::history`, `ftx-wf1`). Optional, sourced initial liquid
and constant supply bind to the exact initial selected-body mesh. Canonical regional Ret demand is
summed per stable body triangle before the declared interval quadrature; available liquid caps modeled
covered evaporation without duplicating water across regions or layers. Step and cumulative receipts
retain supply, evaporation, unmet demand, remaining mass, prescribed-body-reservoir latent energy and
signed mass/demand residuals. Invalid inputs or failed numerical/retention checks refuse the history
transactionally, preserving independent mechanics and endpoints. Exact replay includes the water
declaration. No textile-temperature feedback, textile moisture storage, inter-triangle water transport,
bare-body evaporation, uncertainty propagation, physiology or empirical garment claim follows.

**Textile-owned initial free liquid** (`study::heat::surface_water`, bounded `ftx-vc2` slice).
Optional `motion.transient_surface.surface_water` binds every cloth instance to its exact frozen
material and explicit kg/m² reference-area loading, including dry zeros. Independently admitted
source evidence qualifies the authored liquid capacity, latent heat, pressure/temperature table,
vapor films and retention of constant dry thermal/Ret observations over the complete temperature
and [0, maximum loading] domains. Missing evidence or Ret refuses this heat consumer; when explicit
mechanical coupling is selected it also refuses the dependent motion.
The inner thermal cell and its free liquid share temperature. Each substep samples pressure and
vapor sensible outflow at the old temperature using the canonical retained Ret flux authority.
The earliest exact mass-depletion time splits the step. The existing implicit dry heat/exterior
solver advances with remaining-liquid sensible capacity and signed latent energy; after dryout its
dry remainder uses the same solver. Dry storage + liquid storage change + exported vapor sensible
energy + latent energy balances signed boundary input plus separately opted-in mechanical heat.
Reference material area owns water and current area conducts vapor. Cumulative and per-triangle
receipts retain initial/remaining/evaporated water, dryout counts and signed mass/energy residuals.
Any late domain, arithmetic or work-budget failure rolls back the whole accepted heat interval.
Replenishment must be explicitly zero. Raw temperature-domain departure and condensation refuse;
there is no sorption, wicking, wet-property inference, body-water transfer or empirical validation.
Added mechanical mass requires the separate liquid coupling above. Explicit mass transfer needs time/space
refinement; numerical conservation alone does not establish accuracy. Nominal comparison admission
independently checks this complete mass/energy ownership through canonical material and Ret replay;
thermal and water uncertainty bounds remain unsupported. Native/portable scenario JSON carries the
same policy. Analytic/refinement/rollback
tests live with the module; sewn-garment and portable study tests exercise the actual moving solver.
Complete request and ledger definitions: `docs/textile-surface-water.md`.

**Source-qualified textile liquid delivery** (`study::heat::surface_water`, `ftx-vc2`).
Request version 2 replaces the initial-only zero replenishment declaration with independently
sourced per-instance, per-keyframe-interval reference-area delivery rates and incoming liquid
temperatures. Complete instance bindings include explicit zero rates; missing qualification,
unadmitted estimates or temperatures outside the material/liquid domain refuse. Version 1 keeps
its original input/output semantics. Current area conducts evaporation while reference area owns
initial liquid and supplied mass. The net depletion rate defines exact dryout events; dry patches
can evaporate delivered liquid at the lesser of supply and potential demand, or rewet under excess
delivery. A qualified maximum-loading crossing refuses the whole interval without inferred drainage.
The existing implicit thermal solver uses remaining liquid capacity and signed source
`mechanical heat - latent + supplied mass * cp * (incoming temperature - old cell temperature)`.
Final/sample/patch receipts retain supplied mass and incoming sensible enthalpy. The full energy
ledger subtracts that incoming enthalpy; the mass ledger includes initial plus supplied inventory.
Supply shares complete accepted-interval rollback with liquid, heat, source work and trace.
Analytic tests cover hot/cold feed, interval changes, dryout/rewetting, source/domain refusal,
reference invariance, time refinement and late overflow rollback; a moving sewn-garment regression
checks actual reference-area delivery, Maxwell coexistence, unchanged mechanics and exact replay.
No body transfer, absorption, capillary transport, inferred wet-property law or empirical
fabric/comfort claim is supplied by this boundary condition. Mechanical liquid mass requires the
separately qualified incoming/outgoing coupling above.

**Source-qualified condensation and re-evaporation** (`study::heat::surface_water`, `ftx-vc2`).
Version 3 independently qualifies reverse use of the locked canonical Ret, wet/dry phase transfer,
incoming vapor temperature and the existing constant-latent/effective liquid-sensible enthalpy
convention. Signed current-area flux condenses onto reference-owned liquid when ambient vapor
pressure exceeds the film pressure. The same implicit solver adds latent release and incoming
sensible mixing, then can re-evaporate that inventory after a boundary reversal. Condensed mass,
incoming vapor sensible energy and released latent energy remain separate from liquid supply and
evaporative removal. Maximum-loading/domain failure rolls back the complete accepted interval;
there is no drainage, source default, sorption or whole-layer/wearer claim. Mechanical mass uses
only the separate qualified incoming/outgoing coupling above.
Comparison admission checks the complete source/material/patch inventory and reconstructs incoming
enthalpy from each interval's condensed mass and declared vapor temperature. Existing v1/v2
positive-only semantics remain unchanged. Analytic warming, dry/wet reversals, reference-area
accounting, time refinement, rollback and native/portable scenario replay qualify software only.

**Source-qualified prescribed drainage** (`study::heat::surface_water`, `ftx-vc2`).
Version 4 requires separately sourced liquid supply and drainage, and optionally admits the same
condensation qualification as version 3. Per-instance, per-keyframe drainage demand uses reference
area; evaporation still uses current area. Evaporation and drainage share incoming flow in their
potential-rate ratio at empty stock, and joint dryout splits the complete available inventory
without inventing or losing a positive phase. Remaining liquid capacity enters the actual implicit
thermal solve. Drained liquid exports sensible enthalpy at the old inner-cell temperature and
declared reference, with no latent term. Final/sample/patch result `/4` adds drained mass and liquid
sensible export, preserving vapor and condensation ledgers. Insufficient drainage still refuses
qualified maximum-loading overflow; it never invents runoff. Full rollback, independent phase and
energy balances, source admission, and the `textile_water_drained_mass_kg` decision metric cover
this prescribed numerical capability. Gravity runoff, capillarity, wet constitutive changes and
empirical whole-garment fit remain outside it. Water `/1`–`/3` reject drainage fields and retain
their published behavior.

**Qualified centerline thermal bridges** (`thermal::steady::coverage::bridges`).

The existing steady coverage request additionally accepts `thermal_bridges`, a qualified centerline
excess over the unchanged unbridged network. Exact source-surface models, local ordered material
hashes, interfaces/films and authored temperature/length/area-ratio domains admit each observation.
Current mesh-edge lengths are partitioned through the same projection footprints; shared boundaries
choose a deterministic owner only when their exact physical qualification and reservoir conditions
agree. Prescribed temperatures and films may differ per body triangle; each projected interval uses
its local boundary and uniquely matched qualification before signed contributions are summed.
Simple closed paths repeat the first vertex only at the end, with at least three distinct vertices;
their closing edge contributes once. Open paths have no implicit closure. Repeated or
coincident paths, uncovered intervals, stale sources and incompatible domains refuse without partial
power. Baseline, signed excess and total remain distinct globally and per body patch. Ordinary
scenario/motion-endpoint coverage uses the same authority; omitted policies preserve legacy output.
No transient bridge storage, generic heterogeneous seam
microgeometry or exterior-radiation closure is inferred. Synthetic core, native/portable and actual
material-frontier regressions are described in `docs/steady-thermal-coverage.md`.

**Geometry-derived thermal correspondence** (`thermal::surface::projection`, `ftx-tmb`).
`project` partitions body triangles by local-normal cloth projection using bounded convex clipping,
retains inverse-mapped cloth subpolygons and mid-surface distance ranges, and emits the existing
`coverage::Layout`. It enforces ordering over complete polygons and rejects grazing/crossing sheets,
unresolved folds, horizon cuts and spatially duplicated cloth allocations even below scalar budgets.
Physical layer identities and directed interface resistances remain explicit. The caller supplies an
outward non-self-intersecting boundary; edge consistency is checked, not global body validity. Fixed
ordering, the existing BVH and geometry/output budgets bound the calculation. Numerical partition
residuals, tiny unresolved footprints and bounded allocation-roundoff corrections are reported.
`ftx-materials thermal-map` accepts saved drape + triangular body OBJ + instance-to-layer settings;
`thermal-coverage` recomputes generated projection receipts before reporting geometric verification
under this closure. Tests: `thermal_projection` and the actual sewn-drape CLI workflow above. No
automatic interface physics, true surface air-gap construction, general concave/folded correspondence,
temperature remapping, wearer thermoregulation, measured material/fit or browser execution-parity claim.

**Resumable thermal motion** (`thermal::surface::coverage::motion`, `ftx-tmb`).
`Frame::checkpoint` and `Frame::from_checkpoint` retain/rebuild the complete material and
geometric frame and exact cell temperatures. Restoration reruns projection and enforces
cell shape, source identity and characterized domains; a digest is not authentication.
`ftx-materials thermal-motion` chains prescribed timed poses through instantaneous
material-polygon/depth-cell enthalpy remap and the existing heat solver, without resetting
heat at a new pose. Physical heat and remap numerical residuals remain distinct. The
complete source-cloth material domain is conserved while body coverage may change. Each
source triangle retains one first-order uncovered through-thickness temperature profile;
covered/exposed transfers preserve reference-mass enthalpy. Uncovered cloth is adiabatic
unless the caller supplies explicit canonical-face boundaries; no ambient condition is
inferred. Body/depth discretization may change, while material substitution and cloth
remeshing still refuse. A bounded
convexification removes only roundoff-scale inward polygon corners, reports the reference
area correction and leaves original projection snapshots unchanged. Genuine nonconvexity
and unmatched donors still refuse. Tests: `thermal_motion`, motion polygon unit cases,
and actual saved-drape/production-CLI restart/exposure cases in `thermal_coverage_workflow`.
See `docs/thermal-motion.md` for timing, checkpoint schemas, budgets and no-claim boundaries.

`Frame::advance_with_exchange_and_heat` accepts nonnegative joules in retained source-triangle
order and partitions them over covered/exposed reference area and depth. The same bounded enthalpy
kernel preserves distinct layer capacities and heats adiabatic exposed cloth without heating bare
body. Source allocation and inversion residuals remain in the physical balance; remap residuals
stay separate. Invalid sources or temperatures leave input checkpoints unchanged. Production
coverage reuses the independent-sheet material conversion authority and accepted-work capture,
excluding initial drape work, rejected trials, reversible storage and numerical spring loss.
Plastic-yield and Maxwell-dashpot conversion each need independent source-qualified fractions and
temperature domains. Their whole-triangle heat is summed once before covered/exposed depth allocation;
either source can be selected independently, including in coverage-only motion. Both histories may
consume the same motion without duplicating retained work; their energy totals remain separate model
domains. `tests/relaxation_heating.rs` checks sewn-garment work capture, independent reference-mass
enthalpy, mixed-source conservation and independent source/temperature/budget refusals. Tests in
`thermal_motion` integrate enthalpy independently through coverage transitions, and the native
accepted-body workflow compares conversion-only substitutions and coverage-only source capture.
That supported workflow also exercises fully covered overlap roundoff: only a positive retained
covered stencil within the existing allocation tolerance can avoid a nonexistent exposed donor.
Weights are not inflated; area and material-local enthalpy residuals remain reported. A reduced
real-geometry regression checks nonuniform variable-capacity heat and changed depth resolution;
finite missing wedges and empty microscopic stencils still refuse.

**Clamped-strip experiments and inverse identification** (`strip`, `strip::fit`).
The production membrane and static Newton solver model declared full-width ideal grips, prescribed
axial separation, free in-plane interior displacement and a planar constraint. Reactions are total
newtons; grip extension is never substituted for an element strain. Scalar and tabulated forward
laws retain source/estimate admission, material form and characterized domains. No mass, bending,
friction, slip, rate or out-of-plane behavior is identified by this experiment.

`ftx-strip-calibration/1` fits the four scalar orthotropic coefficients through three deterministic
bounded local searches. Whole acquisitions belong to FIT or CHECK; CHECK never selects coefficients,
minima or strain bounds. Publication requires stationary starts, interior coefficients, independent
local sensitivity, no discovered equivalent separated minima, held-out residuals, doubled-mesh
agreement and actual production replay inside the final FIT-only domain. Every attempted forward
solve, including sensitivities, rejected candidates, refinement and domain replay, consumes one
aggregate budget. Failed gates preserve candidate diagnostics with null material/selection; malformed
inputs, exhausted work and failed forward solves refuse. Returned coefficients remain estimates with
an explicit selection admission policy and exact `selection_json` for source-stable browser reuse.

`tests/strip_experiment.rs` includes independent analytical Green-strain force/energy answers and a
virtual-work check under nonuniform contraction. `tests/strip_identification.rs` checks synthetic
coefficient recovery, held-out isolation, mesh refusal, bounded work and fitted-domain replay. These
establish mechanics/software properties for the declared model, not real-knit adequacy, global
uniqueness, confidence intervals, missing downstream material properties or garment validation.

Replacement identification retains any prior fitted Green-strain domain. It constrains every
candidate, probe, coarse/refined FIT/CHECK evaluation and final replay; published FIT support is
intersected with that prior authority and includes the pristine reference. A refit cannot silently
widen previously characterized response. `strip_identification_limits` checks both refused narrow
support and accepted replacement with identical actual states under original and published domains.

Optional `area_response` identification policy (`ftx-ai1`, `ftx-ai4`) explicitly preserves qualified
fixed area coefficients or appends two nonnegative N/m coordinates to the scalar, declared-shape or
signed-interval base. Fresh Identify mode needs explicit positive J trial support and matched
specimen/conditioning but no prefilled coefficient observations. Explicit J intersects any inherited
area interval. Scalar bases additionally require signed Green-strain trial support and convert their
exact base Q to canonical curves/coupling; the physical-area term stays separate. Scalar coordinates
are labeled as base coefficients, and publication carries the complete area-augmented response.
The existing production compiler evaluates every trial and clipped publication; no second optimizer
or constitutive implementation is used. All coordinates participate in the same bounded starts,
excitation and aggregate solve accounting. Common source conditions and the prior J interval remain
authoritative; fixed fitted/CHECK-derived priors refuse. Only FIT states set the final strain support,
and complete published-law replay precedes selection. Omitted policy retains the original refusal.
`strip_nonlinear_identification` checks synthetic recovery, fixed-source preservation, actual frozen
replay, CHECK isolation, insufficient excitation, source/domain failures and complete work budgets.
These are conditional numerical tests, not independent empirical knit or garment validation.

**Explicit strip extensometers** (`strip::measurement`, plan §25.50). Optional bounded observation
channels declare a reference side-edge station and either its Euclidean material-point chord or
signed projection on an explicit strip-frame unit axis. The same evaluator uses actual production
positions in fitting, sensitivities, refinement, CHECK and published-law replay. Stations interpolate
in the original mesh coordinates and remain fixed through refinement. Nonfinite coordinates, invalid
axes, incompatible meshes, duplicate IDs and invalid scales refuse. The original midpoint channel and
omitted-field serialization remain unchanged. Force remains required; unknown source operators are
not guessed. `strip_measurements` checks independent affine/interpolation answers, actual inverse
recovery, sensitivity, held-out isolation and exact frozen forward replay. Numerical recovery does not
qualify a measured coupon or supply missing material properties.

**Timed clamped-strip history** (`strip::history`, `ftx-sh1`). `ftx-strip-history/1` loads, holds and
unloads a planar, fully gripped sheet through the existing implicit dynamic solver. Canonical membrane
observations and real reference areal mass are required; unrelated bending/contact/thermal evidence
is not fabricated. The pristine stationary initialization, explicit increasing clock, piecewise-linear
grip motion and globally bounded subdivisions preserve accepted Maxwell, permanent-strain or explicitly
qualified parallel-network histories throughout. Every accepted step reports membrane reactions, interior momentum residuals,
stored/kinetic energy and separate existing physical viscous/plastic work. A failed complete request
publishes no partial success. Frozen inputs replay offline. Synthetic analytical Green-strain bar
reactions/work, continuous Maxwell ramp/hold refinement, mass-dependent dynamics and refusal tests
are in `tests/strip_history.rs`. This is constrained planar dynamics, not buckling, parameter fitting,
measured coupon qualification, inferred heat or actual garment fit.

**Material-frame strain consumer** (`study::strain`, `ftx-ds2`). Optional scenario `material_strain`
requests raw `[E11, E22, 2E12]` on explicitly selected retained instances or the whole retained garment.
The ordinary production membrane gradients and welded triangle ownership define material axes;
no edge-percentage, global percentile or softened strain is substituted. Dimensionless signed extrema
retain the responsible source triangle, instance, exact material and earliest accepted sample. The
initial state and every accepted motion state contribute, with exact geometry/time identities and
one aggregate bounded triangle-sample reservation across alternatives. An unavailable or exhausted
strain consumer preserves independent drape, motion and thermal outputs. Full source snapshots,
fitted/tabulated strain boxes and any physical-area support remain visible; their characterization
domains are not rupture limits or safe-fit thresholds. Comparison bands are authored design
requirements, never an inferred comfort criterion or continuous-time bound. Analytic frame/shear,
deterministic witness and transactional checks accompany actual sewn static/motion replay and
source/scope/budget refusal tests in `tests/material_scenario/strain.rs`.

**Named operating cases.** `study::matrix` admits `ftx-drape-scenario-matrix/1` through the existing
scenario adapters. The common body is selected once; each case owns a fresh ordinary session and
each material evaluation starts with independent cloth, constitutive and thermal state. Every case
must prepare the same exact original pattern/outfit under its own options. Declared vertex-frame
allocations sum to 64000000 and active motion-trial allocations to 32768; the export layer separately
reserves at most 64 case/alternative callbacks. Thermal/vapor work and retained motion geometry
share the existing session limits across cases, including refusals. Only work counters cross session
boundaries. Case environments and motion paths are explicit sensitivity inputs, not a probability
distribution or a claim about untested operating conditions. The native/file-free adapters retain
identical descriptors and independent consumer refusals; existing single-case behavior is preserved.
