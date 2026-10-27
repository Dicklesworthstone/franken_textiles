# Material-driven relaxation, creep and recovery

A source-bearing material can now declare `membrane_relaxation` alongside its equilibrium membrane.
The same frozen selection drives uniform or per-piece sewn garments, Newton stepping, elastic reactions
and prescribed-body continuation. Each retained material triangle owns its accepted internal strain;
seam welding never merges unlike material histories. No new crate or runtime dependency is added.

An explicitly qualified [parallel network](material-parallel-memory.md) can also retain permanent
strain in its equilibrium branch. Maxwell branches still see total physical Green strain, while
plastic flow sees only the elastoplastic equilibrium branch's stress. Full-material compilation and
atomic binding admit both histories together; unrelated independent laws retain their existing
mutual refusal. Ordinary Maxwell selections and their wire format remain unchanged.

## Use it

`tests/fixtures/materials/synthetic-relaxation.json` is a complete **synthetic** selection, not a
measured textile or a recommended approximation for any particular fabric. Bind it to a pattern with
explicit grainlines using a new destination:

```sh
cargo run -p ftx-cli --bin ftx-materials -- bind \
  YOUR-PATTERN.json tests/fixtures/materials/synthetic-relaxation.json \
  relaxing-pattern.json
cargo run -p ftx-cli --bin ftx-materials -- resolve relaxing-pattern.json
# With your existing measurements, fit contract and installed body model:
ftx drape relaxing-pattern.json --measurements M.json --fit F.json --out NEW-DIR
```

A source piece can instead select its own `piece.fabric.material`, using the ordinary per-piece
selection syntax. An explicit equilibrium-only piece does not inherit the default material's memory.
Mixed-material self-contact still requires its independently sourced interface pairs; adding a
relaxation law does not authorize guessing unlike-cloth friction.

The optional record has model `green_maxwell/1`, initialization, `strain_range`, source, method and
conditions, with one to sixteen branches in total. The original `branches` array contains distinct IDs,
source-bearing positive `stiffness_ratio` in `1` and `relaxation_time` in `s`. Ratios multiply the
**equilibrium origin tangent**, not the instantaneous stiffness. The original scalar or tabulated
nonlinear membrane remains the equilibrium response. Supplying an instantaneous modulus as equilibrium
and then adding transient branches would double-count stiffness.

Optional `orthotropic_branches` allow independently characterized warp/wale, weft/course and bias
recovery. Each entry has an `id`, a sourced positive `relaxation_time` in `s`, and a `stiffness` object
with four ordinary source-bearing quantities: `warp`, `weft`, `coupling` and `shear`, all in `N/m`.
These are tensor entries in the work-conjugate `[E11,E22,2E12]` frame, not Young moduli or bulk fiber
properties. The normal block is `[[warp,coupling],[coupling,weft]]`; the engineering-shear entry is
`shear`, with no normal/shear coupling. Explicit zero components permit a warp-only, weft-only or
bias-only branch. Different branches can then have different recovery times. The complete branch
tensor must be nonzero and positive semidefinite; unstable normal coupling refuses. No absent entry
becomes zero and no stiffness is inferred from a fiber, weave, knit or brand name.

For an orthotropic spectrum, the law's `conditions` must include nonempty `specimen_id` and
`conditioning`. Every branch coefficient and time, including any accompanying proportional branches,
must match all declared shared conditions. The consumed equilibrium stiffness observations or nonlinear
curves must match too. Test-specific extra conditions, such as direction, can remain on observations.
Purely directional spectra use `branches: []`. IDs are unique across both arrays; internal-strain
reports retain proportional branches first, then orthotropic branches, each in authored order.
Omitting `orthotropic_branches` preserves the original source serialization and proportional arithmetic.

The timed `ftx-materials calibrate-relaxation` workflow identifies **proportional** ratios and times from
complete histories with independently supplied equilibrium; see `relaxation-calibration.md`. It does
not yet fit independent directional tensors. Such tensor/time observations must be supplied explicitly
with their characterization source and conditions. Static swatch calibration does not determine times.

Estimates require the material selection's existing explicit `allow_estimates` policy. Every source,
parameter, condition and initialization choice participates in the frozen material identity. Omitting
`membrane_relaxation` preserves existing source snapshots and the memoryless numerical path.

## Constitutive and numerical contract

Let e be the material-frame Green strain vector [E11,E22,2E12], after the existing reversible
compression/gather mapping. For branch b, Qb is either `ratio_b * Q_equilibrium_origin` or the explicit
orthotropic branch tensor, and eta_b = tau_b Qb.
It stores 1/2 (e-zb)^T Qb (e-zb) per reference area, where zb evolves as (e-zb)/tau_b. The raw physical
strain must remain inside the declared characterization box as well; compression softening does not
permit extrapolation. The domain must contain zero in its interior. Normal lower bounds exceed -1/2.

For an actual time step h, backward Euler gives a=tau/(tau+h), b=h/(tau+h), zb_next=a zb_old+b e.
Eliminating zb gives the additional incremental potential 1/2 a (e-zb_old)^T Qb (e-zb_old), with its
exact stress and tangent. The solver adds this to the existing equilibrium membrane exactly once,
using the shared exact nodal/grain/compression derivative chain. Every Newton line-search evaluation
and friction lag round reads the same accepted history. A converged, audited physical step commits
new geometry, velocities, contact state and histories together. Rejected steps leave all of them
unchanged, including when prescribed body motion subdivides and retries an interval.

Initialization is explicit: `stress_free_reference` starts internal strains at zero before geometric
deformation; `relaxed_start` starts them at the supplied initial effective strain, so the transient
branches initially carry no stress. The equilibrium branch is not removed by either choice. History
is initialized once after start placement/welding, preserved by cloning and motion continuation, and
cannot be silently rebound after changing a triangle's material or rest frame.

`Solver::relaxation_report` and drape `membrane_history` report law identities, per-triangle viscous
strains, effective branch stress, accepted time/step count, physical spring storage and endpoint
quadrature of viscous step work. Sewing and draping both advance the reported clock; graphical fold-up
interpolation is not extra physical time. Viscous step work excludes backward-Euler numerical
dissipation and is not automatically injected into the thermal model. This output is not a serialized
restart importer. The public low-level `ClothModel::elastic_energy` remains equilibrium-only; the solver
potential includes the internal branches, and their separate physical storage is in the history report.

An unconverged step refuses on this opt-in path. A static solve or velocity-only automatic settling
also refuses: neither specifies elapsed physical time nor proves stress equilibrium. Use explicit
time steps, inspect transient stress and refine the time step for accuracy. A geometrically settled
drape is not necessarily a relaxed material. The transactional history path currently copies solver
state per physical step; no performance improvement is claimed.

## Evidence and limits

The regression suite includes independent directional hold/unload answers, coupled tensor derivatives
and zero-energy null modes; source/condition refusals; standard-linear-solid creep/recovery; temporal
refinement; assembled nodal derivatives; nonlinear equilibrium and gather compression; actual clamped
directional reactions, changed free dynamics, sewn per-piece ownership, cloning, rollback and body
motion. All specimen data are synthetic; these checks do not establish measured garment accuracy.

This is a constant-condition generalized Maxwell model in Green strain. Independent directional
tensors are supported, but their automatic identification is still absent. It is not yarn homogenization,
plastic set, failure/damage, aging, thermal activation, wet relaxation or a physically qualified model
for arbitrary large textile strains.
Independent hold/cycle/recovery measurements and fitted time spectra are still required for actual
linen, leather and performance-knit variants. Bead `ftx-tm4` remains open for those broader physical
and constitutive acceptance conditions, along with the empirical-data gate `ftx-tm2`.

Formula cross-checks (independent implementation, no source code copied):
- Numerical Tours of Continuum Mechanics, linear viscoelasticity and incremental energy:
  https://comet-fenics.readthedocs.io/en/latest/demo/viscoelasticity/linear_viscoelasticity.html
- MOOSE generalized Maxwell material and time integration:
  https://mooseframework.inl.gov/source/materials/GeneralizedMaxwellModel.html
