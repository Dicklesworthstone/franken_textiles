# ftx-drape contract

**Purpose.** The cloth solver that replaces Blender's (plan §11): an orthotropic Green-strain membrane
rotated to the grain, quadratic hinge bending, lumped inertia, and implicit Euler solved by Newton on a
sparse LDLᵀ with an Armijo line search; a static solve; and contact with a static body (IPC barrier on the
thickness-offset gap, additive CCD, lagged smoothed Coulomb friction); and the garment mesh of spec 04 §2
(`garment.rs`). Placement, pre-sew, the drape schedule and the reference's metrics are described below.
Optional cloth self-contact adds moving cloth–cloth barriers and independent friction.

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

**Scenario transient heat** (`study::heat`, plan §25.10). Optional independent-sheet and geometric
body/layer consumers follow every accepted prescribed-motion sample through the existing Surface
and conservative Frame/remap APIs. Time-zero temperatures and spatially uniform interval films are
explicit. Heat storage follows reference mass; changing coverage preserves covered/exposed material
enthalpy, with remap residual separated from physical heat. Thermal failures never erase complete
mechanics or another consumer. Complete histories retain time/geometry/source identities and final
fields; coverage includes a restorable checkpoint. Per-history/session work is bounded. Prescribed
films, qualified exterior exchange and independently opted-in plastic-yield/Maxwell-dashpot heating retain
separate source ledgers. Both thermal domains qualify their own conversion observations and sum selected
source heat once; mechanical work arrays are captured once per channel across the domains. Exact sample
admission uses retained sewn triangles, while conservative session reservations include both channels.
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

**Scene** (`scene.rs`; plan §11.3–11.4). A pattern meshed, placed with a body's part frames (any
`PartFrames`; `ftx_body::BodyFrames` reads them off a posed Anny body), started (wrap, pre-sew, relax
against the drape body), untangled, welded and handed to the solver with contact over the body:
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
`intersecting_start` (the cloth starts on or through the body or another cloth layer). A step that does not converge is reported
(`SolveReport`), not refused.

Self-contact's digest (the folded strip after twelve steps, `tests/self_contact.rs`) is identical native
debug, native release and wasm32-wasip1.

**No claims.** Self-contact matching Blender's self-collision response (only its distance is mirrored);
a moving body; an elastic's own mass or its friction in the casing; fabric parameters (they are inputs;
plan §11, research 07 §2.7). The §11.7 budgets are measured above, on one machine (native and node's
wasm), not on a browser's.

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
