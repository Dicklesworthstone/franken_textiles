# ftx-drape contract

**Purpose.** The cloth solver that replaces Blender's (plan §11): an orthotropic Green-strain membrane
rotated to the grain, quadratic hinge bending, lumped inertia, and implicit Euler solved by Newton on a
sparse LDLᵀ with an Armijo line search; a static solve; and contact with a static body (IPC barrier on the
thickness-offset gap, additive CCD, lagged smoothed Coulomb friction); and the garment mesh of spec 04 §2
(`garment.rs`). Placement, pre-sew, the drape schedule and the reference's metrics come next (FTX-13);
cloth self-contact is not implemented.

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
  E1 = E2 = tensile, ν = 0, G = 0.2·tensile, compression softened to a given fraction.
- Untangling: crossings the exact audit still finds after relaxation are removed by moving the crossing
  cloth edge's ends (or the pierced cloth triangle) 2 mm above the crossed body triangle (or ridge), in
  rounds of up to 50; a piece wholly inside the body is refused.
- Offset ramp: when an edge of the start passes closer to the body than the cloth offset ξ, contact
  starts at half the smallest distance and `run_phase` raises ξ by 0.25 mm a step while every gap stays
  above ĝ/10.
- `run_phase` runs implicit steps under a gravity (the reference's sew then drape).
The drape body (`ftx_body::trimmed_skin`) leaves out the hands and feet; with Anny's bent elbows a
straight sleeve crossed the forearm, so the drape pose straightens the arms (`Pose::drape`). The drafted
92 cm bolero builds a clean start on the 92 cm Anny toddler (no crossings, 600 relaxation iterations,
p95 strain 0.115). Speed is the open problem: about 0.2 s per Newton iteration and 80–110 projected
iterations per 1/24 s step for its 5,800 DOFs (factorization 40 %, PSD projection 19 %, contact 8 % of
the time before the assembly sort was replaced).

**Units and frames.** Metres, kilograms, seconds; z up. Rest coordinates are the flat piece in its material
frame (+u along the grain).

**Energy.** `E(x) = ½h⁻²(x − x̃)ᵀM(x − x̃) + Ψ_membrane + Ψ_bend + B + D`, `x̃ = xₙ + h vₙ + h² g`
(B, D: contact barrier and friction, below; the static solve drops inertia and friction).
- Membrane: StVK on Green strain with plane-stress resultants Q (N/m): warp E1, weft E2, Poisson ν12,
  bias G12. Each normal strain is softened under compression by
  `φ(e) = r·e + (1 − r)·δ·tanh(e/δ)` (r = √s, δ = 1e-3): C² at zero strain (a plain kink made Newton cycle
  100 iterations a step on a sagging sheet; the blend converges in 1–2) and stiffness → s·Q in compression.
- Bending: Bergou's quadratic hinges, weight `3a/(A₀+A₁)²` with each triangle's area handed to its interior
  hinges; constant PSD Hessian.

**Solver.** Exact Hessian first; when the LDLᵀ inertia shows a negative eigenvalue, element blocks are
projected to PSD (negative eigenvalues zeroed) for the rest of that solve (retrying the exact Hessian
cost a second assembly and factorization nearly every iteration on a garment); steepest descent if even
that is singular. Tried on the bolero and not kept: flipping negative eigenvalues instead of zeroing them
(more iterations), and a membrane Hessian with its stress clamped to the non-negative part instead of
eigen-projection (cheaper iterations, 2.7× as many in the drape, slower overall). The symbolic analysis is computed once,
ordered by AMD on the vertex graph and expanded to the free DOFs (the pattern never changes: every
element block is always assembled). Per-DOF constraints (rollers, pins). Convergence: `‖p‖∞/h < tol`
(step) or `‖p‖∞ < tol` (static).

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
- Broad phase: a uniform grid in `BTreeMap`s (cell ≈ the body's mean edge length), queried per cloth
  primitive with sorted, deduplicated results, then an exact box-overlap test; no BVH.
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

**Fold-up animation and files** (`animation.rs`; spec 04 §9). The pattern board (instances left to
right, 40 mm apart, on y = −0.55 m); per instance a staggered rigid flight off the board (slerp from the
identity to the rotation onto its k = 0 roll, centroid on a raised path) then the isometric roll-up
(`start::roll_points`, §3.6) to its wrap; the 18-frame zip from wrap to the sewn start; then the solver's
frames. Rotations use Horn's quaternion method with the deterministic Jacobi solver where the reference
uses numpy's SVD Kabsch. Written as `fold.pc2` (float32 point cache), `mesh_map.json`, seam and strain
colours, and the `fabric-fold-viewer/1` payload embedded in `assets/fold_viewer_template.html` (the
skill's page, unchanged; SHA-256 `7de25b18…a878`). The page's verdict line shows the run's own gating
and advisory flags from `metrics`.

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

**Lifted code** (frankensim `93fc9b26`, MIT + rider, provenance headers): `sparse/` (CSR assembly and the
multifrontal LDLᵀ with AMD and inertia; adds `analyze_with_permutation`; assembly orders triplets by a
row-bucket pass instead of a global stable sort, the same order), `eigen.rs` (`jacobi_eigh`).

**Error model.** `Refusal`: `degenerate_geometry` (bad triangles, non-manifold edges), `out_of_range`
(non-positive density or step), `non_finite` (state blew up), `not_converged` (symbolic analysis failed),
`intersecting_start` (the cloth starts on or through the body). A step that does not converge is reported
(`SolveReport`), not refused.

**No claims.** Cloth self-contact; a moving body; seams, gathers and elastics; fabric parameters (they are inputs; plan §11, research 07 §2.7); performance
targets (§11.7; assembly re-sorts triplets and contact re-queries the grid each iteration, a precomputed
scatter map and cached candidates are the planned speed-ups).
