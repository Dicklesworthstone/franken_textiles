# Permanent set and delayed recovery in the same textile sheet

A source-qualified material can now combine an elastoplastic equilibrium branch with Maxwell
branches in parallel. One ordinary material Selection drives both histories in sewn drapes,
prescribed-body motion and timed clamped strips. Changing that selection changes permanent set,
transient stiffness and delayed recovery together, while mass, bending, contact and thermal
observations retain their independent requirements.

This is a particular bounded phenomenological network. It does not identify a real textile from
its name or composition, infer joint coefficients from unrelated experiments, put plastic flow
inside each Maxwell arm, or promote any built-in catalog entry to measured garment readiness.

## Declare the network

Keep the permanent-strain selector
`structure.membrane_plasticity_model = "orthotropic_green_return_map/1"`, its four scalar
elastic observations and its twelve directional yield, hardening and domain observations.
Keep the ordinary `membrane_relaxation` declaration and its proportional or orthotropic
Maxwell branches. Add the following object inside `membrane_relaxation`:

```json
{
  "parallel_plasticity": {
    "model": "parallel_green_maxwell_plastic/1",
    "evidence": "estimate",
    "source": "joint-characterization",
    "method": "Explicit parallel-network qualification for the stated specimen and loading range",
    "conditions": {
      "specimen_id": "your-specimen",
      "conditioning": "your-qualification-state"
    }
  }
}
```

These strings are placeholders, not supplied evidence. The source must exist in the material's
source map. The network declaration has its **own** evidence class and estimate-consent check:
published individual coefficients do not silently qualify an estimated branch interpretation.

The joint conditions must equal the Maxwell declaration's shared conditions. Every elastic,
plastic and Maxwell coefficient must include those conditions with the same values; extra
acquisition-specific conditions can differ. Both `specimen_id` and `conditioning` are
mandatory. A different moisture state, specimen or declared shared temperature cannot be mixed
without an explicitly matching characterization.

The four elastic coefficients describe the **elastoplastic equilibrium branch**, not the total
instantaneous tensor of the network. A proportional Maxwell stiffness ratio multiplies that
branch's elastic tensor. Using a measured instantaneous stiffness as the equilibrium stiffness
would double-count stiffness; the compiler does not infer a correct split for the caller.

Only complete-material compilation creates matching private locks for the two laws. These include
the full resolved material identity, normalization and estimate policy, so a source or metadata
change cannot reuse another material's history. The effective raw Green-strain range is the
intersection of the Maxwell box, the permanent-strain model's symmetric total-strain bounds and
any declared fitted equilibrium domain. No equivalent-strain coordinate conversion or wider
domain is invented. Physical current/permanent metrics and accumulation limits are still checked.
The network requires unit compression factors and refuses gather softening.

## What each branch does

All branches use the same total physical Green strain
`e = [E11, E22, 2*E12]` in the retained material frame. Let `p` be accepted permanent strain,
`alpha` accumulated absolute plastic increments, `Q0` the equilibrium elastic tensor,
`H` directional hardening, and `z_b` each Maxwell arm's viscous strain. Stored energy per
reference area is

```text
W = 0.5 (e-p)^T Q0 (e-p)
    + 0.5 sum_i H_i alpha_i^2
    + 0.5 sum_b (e-z_b)^T Q_b (e-z_b).
```

The existing plastic return map sees only `Q0*(e-p)`. Maxwell stress never enters that branch's
yield test. Every Maxwell arm sees **total** `e-z_b`; it does not subtract `p`.
For a physical step of duration `h` and arm relaxation time `tau_b`, backward Euler gives

```text
a_b = tau_b / (tau_b + h)
z_b_next = a_b*z_b_old + (1-a_b)*e.
```

Its condensed incremental potential is
`0.5*a_b*(e-z_b_old)^T*Q_b*(e-z_b_old)`.
The production solver adds this to the plastic branch's condensed potential and composes the
sum with the exact Green-strain geometry derivatives. The plastic branch already replaces the
ordinary equilibrium elastic contribution; that elastic contribution is not added a second time.
Frozen reactions and storage use accepted histories without advancing either one.

This network allows permanent set and delayed recovery to coexist. It is not a same-branch
viscoplastic model, a multiplicative finite-strain plastic model, damage, fatigue or rupture.
Its finite domain is part of its declaration. Maxwell `relaxed_start` initializes transient
stress to zero at the supplied pose; it does not anneal or initialize plastic strain from that pose.

## Binding, stepping and reports

`ResolvedMaterial::membrane_plasticity()` and `membrane_relaxation()` compile the matching
halves under the same source policy. The public standalone Maxwell compiler rejects joint
declarations because it lacks the complete material. Use
`Solver::with_membrane_histories(relaxation, plasticity)` to bind one optional pair per retained
triangle atomically. Both branches must be present and carry the same lock. The original individual
binders reject a joint half-law. Independent plastic or Maxwell laws on different triangles continue
to work, and unqualified combinations on the same triangle continue to refuse.

Canonical sewn scenes map both arrays through the same retained triangle ownership, grain and
welded reference geometry. Timed strips use the same binder. Newton trials read immutable histories;
an accepted physical step commits both histories with positions, velocities and contact state.
Failure leaves all accepted state unchanged. Cloning and motion continuation preserve both.
Rebinding changed material, topology, grain or compression refuses. `static_solve` and speed-only
settling cannot substitute for an explicit loading history. Geometric settledness is not stress
equilibrium.

Ordinary plastic and Maxwell reports remain separate. Their per-element `parallel_network`
objects expose the full material identity, model and each branch's strain/stress basis.
`membrane_energy()` sums actual equilibrium elastic, hardening and Maxwell storage once.
`plastic_dissipation_j` records irreversible yield work, while `viscous_step_work_j` records
the existing backward-Euler endpoint quadrature of physical dashpot work. Neither includes
numerical time-integration dissipation or becomes heat merely because it is positive.
Thermal consumers still need independently qualified conversion fractions and thermal properties
for each requested work source. They consume the separate accepted ledgers.

## Run the synthetic example

[synthetic-parallel-membrane.json](../tests/fixtures/materials/synthetic-parallel-membrane.json)
is a complete ordinary Selection. Every coefficient, domain and network qualification is explicitly
an artificial estimate for software tests; none is a measured fabric value.

The existing timed-strip command accepts the same Selection. Create a new request file:

```sh
python3 - <<'PY' > parallel-history.json
import json
with open('tests/fixtures/materials/synthetic-parallel-membrane.json') as f:
    material = json.load(f)
print(json.dumps({
    'schema': 'ftx-strip-history/1',
    'material': material,
    'fixture': {
        'gauge_length_m': 0.1, 'width_m': 0.06,
        'warp_axis': [0, 1], 'cells': [2, 2]
    },
    'loading': [
        {'time_s': t, 'extension_fraction': e}
        for t, e in [(0, 0), (0.05, 0.02), (0.15, 0.02), (0.2, 0), (0.25, 0)]
    ],
    'controls': {
        'max_step_s': 0.01, 'max_steps': 512,
        'velocity_tolerance_m_s': 1e-9, 'force_tolerance_n': 1e-6,
        'max_newton': 100
    }
}))
PY
cargo run -p ftx-cli --bin ftx -- --json materials strip-history parallel-history.json
```

The prescribed path loads, holds, unloads and holds again. The report retains every accepted
endpoint, both dissipation ledgers and a frozen request for offline replay. Refine time step and
mesh for a particular physical claim; convergence of a single synthetic path is not garment
qualification. Ordinary pattern bindings and per-piece overrides consume the same material without
editing sewing geometry. Existing separate elastic, plastic and relaxation calibration commands
retain their stated model scopes; this declaration does not add a joint inverse-identification
algorithm.

## Verification and reference

The direct tests independently evaluate plastic return and Maxwell recurrence formulas, stored
energy, branch stress, reaction forces and both physical dissipation ledgers. They also check
source/domain qualification, half-network refusal, initialization, exact geometric energy
derivatives, failed-step rollback, actual sewn ownership and timed-coupon replay. The small-mass
synthetic coupon comparison retains its declared finite numerical mass and compares with a
homogeneous bar; the ordinary example keeps its original reference density.

The parallel equilibrium/Maxwell construction and backward-Euler recurrence were checked against
Jeremy Bleyer's primary computational mechanics demonstration,
[Linear viscoelasticity](https://comet-fenics.readthedocs.io/en/latest/demo/viscoelasticity/linear_viscoelasticity.html).
The elastoplastic equilibrium-branch extension here is an explicitly declared model choice, not a
claim that the reference validates a real textile or this combined material. No source code was
copied. The permanent-strain algorithm and its remaining limits are documented in
[Material-driven permanent membrane stretch](material-membrane-plasticity.md).
