# ftx-drape contract

**Purpose.** The cloth solver that replaces Blender's (plan §11). This stage is the core without contact:
an orthotropic Green-strain membrane rotated to the grain, quadratic hinge bending, lumped inertia, and
implicit Euler solved by Newton on a sparse LDLᵀ with an Armijo line search; plus a static solve.
Contact (IPC barrier, CCD, friction), seams, the garment scene and the reference's metrics come next
(FTX-12, FTX-13).

**Units and frames.** Metres, kilograms, seconds; z up. Rest coordinates are the flat piece in its material
frame (+u along the grain).

**Energy.** `E(x) = ½h⁻²(x − x̃)ᵀM(x − x̃) + Ψ_membrane + Ψ_bend`, `x̃ = xₙ + h vₙ + h² g`.
- Membrane: StVK on Green strain with plane-stress resultants Q (N/m): warp E1, weft E2, Poisson ν12,
  bias G12. Each normal strain is softened under compression by
  `φ(e) = r·e + (1 − r)·δ·tanh(e/δ)` (r = √s, δ = 1e-3): C² at zero strain (a plain kink made Newton cycle
  100 iterations a step on a sagging sheet; the blend converges in 1–2) and stiffness → s·Q in compression.
- Bending: Bergou's quadratic hinges, weight `3a/(A₀+A₁)²` with each triangle's area handed to its interior
  hinges; constant PSD Hessian.

**Solver.** Exact Hessian first; element blocks projected to PSD only when the LDLᵀ inertia shows a
negative eigenvalue; steepest descent if even that is singular. The symbolic analysis is computed once,
ordered by AMD on the vertex graph and expanded to the free DOFs (the pattern never changes: every
element block is always assembled). Per-DOF constraints (rollers, pins). Convergence: `‖p‖∞/h < tol`
(step) or `‖p‖∞ < tol` (static).

**Meshes.** Use `hex_grid` (near-equilateral) for anything that bends: hinge bending on it is isotropic
(cylinder energy 0.98–1.02 of theory at 0°/30°/45°/90°). Right-triangle grids (`grid`) have a zig-zag mode
along clamped edges that made a cantilever ~25 % too soft; they are fine for in-plane tests.

**Known answers** (`tests/kat.rs`, plan §11.9): bending scale on a cylinder (0.99 at 2.5 mm spacing,
O(h)); KAT 2 membrane anisotropy (uniaxial moduli along warp, weft, 30° and 45° within 4e-5; compression
law exact to 1e-6 across the blend; rigid rotation stores nothing); KAT 3 Peirce cantilever (tip within
1.5 % of l of the exact heavy elastica at 1 mm spacing, ASTM droop 42.6° vs 41.5°, B recovered +1.4 %;
release only); KAT 9 for a small drape (identical SHA-256 native debug, native release and wasm32-wasip1
under Node). Element gradients and Hessians are checked against finite differences.

**Lifted code** (frankensim `93fc9b26`, MIT + rider, provenance headers): `sparse/` (CSR assembly and the
multifrontal LDLᵀ with AMD and inertia; adds `analyze_with_permutation`), `eigen.rs` (`jacobi_eigh`).

**Error model.** `Refusal`: `degenerate_geometry` (bad triangles, non-manifold edges), `out_of_range`
(non-positive density or step), `non_finite` (state blew up), `not_converged` (symbolic analysis failed).
A step that does not converge is reported (`SolveReport`), not refused.

**No claims.** Contact, friction, collisions with a body, seams, gathers and elastics; fabric parameters
(they are inputs; plan §11, research 07 §2.7); performance targets (§11.7; assembly re-sorts triplets each
iteration, a precomputed scatter map is the planned speed-up).
