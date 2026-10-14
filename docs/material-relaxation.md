# Material-driven relaxation, creep and recovery

A source-bearing material can now declare `membrane_relaxation` alongside its equilibrium membrane.
The same frozen selection drives uniform or per-piece sewn garments, Newton stepping, elastic reactions
and prescribed-body continuation. Each retained material triangle owns its accepted internal strain;
seam welding never merges unlike material histories. No new crate or runtime dependency is added.

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

The optional record has model `green_maxwell/1`, one to sixteen branches, initialization,
`strain_range`, source, method and conditions. Each branch has a distinct ID, a source-bearing
positive `stiffness_ratio` in `1` and `relaxation_time` in `s`. Ratios multiply the **equilibrium origin
tangent**, not the instantaneous stiffness. The original scalar or tabulated nonlinear membrane is
the equilibrium branch. Supplying a modulus measured immediately after loading as equilibrium and
then adding transient branches would double-count stiffness. The existing static swatch calibration
command does not identify relaxation times or separate instantaneous and equilibrium measurements.
The timed `ftx-materials calibrate-relaxation` workflow now identifies ratios and times from complete
force/deformation histories with independently supplied equilibrium; see `relaxation-calibration.md`.

Estimates require the material selection's existing explicit `allow_estimates` policy. Every source,
parameter, condition and initialization choice participates in the frozen material identity. Omitting
`membrane_relaxation` preserves existing source snapshots and the memoryless numerical path.

## Constitutive and numerical contract

Let e be the material-frame Green strain vector [E11,E22,2E12], after the existing reversible
compression/gather mapping. For branch b, Qb = ratio_b * Q_equilibrium_origin and eta_b = tau_b Qb.
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

The regression suite checks closed-form standard-linear-solid relaxation, creep and unloading recovery;
multiple time scales; temporal refinement; constitutive and assembled nodal gradients/Hessians;
nonlinear equilibrium and gather compression; actual clamped reactions, free dynamics, sewn
per-piece ownership, cloning, failed-step rollback, and prescribed-body motion. All specimen data
are synthetic. These are model/software checks, not measured garment fit or textile characterization.

This is a proportional, constant-condition generalized Maxwell model in Green strain. It is not yarn
homogenization, anisotropic relaxation-spectrum identification, plastic set, failure/damage, aging,
thermal activation, wet relaxation or a physically qualified model for arbitrary large textile strains.
Independent hold/cycle/recovery measurements and fitted time spectra are still required for actual
linen, leather and performance-knit variants. Bead `ftx-tm4` remains open for those broader physical
and constitutive acceptance conditions, along with the empirical-data gate `ftx-tm2`.

Formula cross-checks (independent implementation, no source code copied):
- Numerical Tours of Continuum Mechanics, linear viscoelasticity and incremental energy:
  https://comet-fenics.readthedocs.io/en/latest/demo/viscoelasticity/linear_viscoelasticity.html
- MOOSE generalized Maxwell material and time integration:
  https://mooseframework.inl.gov/source/materials/GeneralizedMaxwellModel.html
