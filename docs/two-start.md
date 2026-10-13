# Two sewing-path sensitivity checks

`ftx drape` and simulated rounds can run the same garment twice with different zero-gravity
sewing durations. This exposes the production pipeline behind plan §11.9 KAT 10: differences
between the resulting drapes are evidence of sewing-path sensitivity, not permission to choose
whichever result looks better.

## Run it

```sh
ftx drape pattern.json --measurements measurements.json --fit fit_spec.json \
  --out new-paired-drape --self-contact-mm 2 --two-start-sew 18

ftx round workspace --sim --self-contact-mm 2 --two-start-sew 18
```

The first run uses `--sew` (12 by default); the alternate uses `--two-start-sew` (18 in these
examples). They must differ. Explicit zero is accepted when the remaining schedule has enough
frames for metrics. The body is fitted once and shared. Body geometry, placement, pattern,
material, gravity schedule, friction, thickness and numerical settings are unchanged between runs.
The feature is opt-in because it executes two full solves; omitting it preserves single-run
simulation. Direct drapes require a fresh output directory. Rounds allocate their own new directory.

The comparison reuses the existing frankensim-derived Newton, sparse factorization, contact and
exact-audit path. It introduces no second solver, stochastic perturbation or sibling dependency.

## Read the result

`sim_report.json` gains `two_start` with schema `ftx-two-start/1`. `--json` also includes it in
the command result. A round preserves it in `round.json`, `rounds.jsonl`, and an HTML review table.
The primary animation remains unchanged; the alternate has diagnostics, not a second movie.

The record includes both phase summaries and final-position SHA-256 digests, maximum and RMS
corresponding-vertex displacement in the fixed body frame, every hem-height / landmark-clearance /
seam-strain comparison, the alternate report, and review flags. No rigid alignment is applied:
a garment sliding on the body must remain visible in the displacement measure. Metric identifiers
are unioned, so a reading missing from either run cannot disappear from the comparison.

| Optional policy setting | Default | Quantity |
|---|---:|---|
| `--spread-vertex-mm` | 10 | Maximum displacement of any corresponding final vertex |
| `--spread-hem-mm` | 6 | Absolute difference of each reported hem height |
| `--spread-clearance-mm` | 3 | Absolute difference of each landmark mean clearance |
| `--spread-seam-pp` | 1.5 | Absolute difference of seam p90 strain, in percentage points |

Bounds are inclusive, finite and positive. A bound without `--two-start-sew` is an error; a paired
option on a round without `--sim` or `--stills` is also an error. These checks precede input/output
work. The hem/clearance/seam bands follow the existing KAT 10 review policy. The vertex band is
an additional configurable review limit. None is calibrated real-wearer accuracy.

`WITHIN_BOUNDS` requires all comparisons to pass, both runs to settle, every solver step to converge,
both body-clearance ramps to reach the requested thickness, and neither run to have a gating flag.
Missing/non-finite readings and empty metric coverage cannot pass. A completed failing check is
`REVIEW_REQUIRED`, adds non-demotable simulation gating flags and exits 2. In a flat-converged round
it produces `SIM_GATING_FLAGS`, with the flat, measurement and export bases still reported separately.

A numerical refusal from either solve publishes no successful paired result. Refusal codes remain
intact (exit 4 for a budget stop, 3 for other direct-drape refusals). Rounds retain a failure record,
not an old viewer. The shared CLI publisher serializes outputs before writing, uses exclusive files
and writes the completion report last. Existing successful or failed output directories are never reused.

## Boundaries and regression coverage

This samples **two sewing durations at the same initial placement**, not two independently perturbed
placements. Agreement is not a statistical confidence interval, proof of a unique equilibrium, or
proof of fit. Both paths can share a wrong minimum. A muslin or dressed photo is still the fit evidence.
The ordinary drape is not silently upgraded to a two-path qualification, and browser controls do not
yet expose this opt-in study.

`DrapeBody` now accepts the existing thread-safe `PartFrames` interface. Anny uses the same adapter
as before; synthetic or application-supplied frames can exercise the complete garment pipeline without
the production Anny asset file. This is not a new body-fitting algorithm or an automatic mesh-to-landmark
inference: callers must supply consistent frames, landmarks, and metric collision/display meshes.

Regressions in `ftx-drape/tests/two_start.rs` run a sewn band on a synthetic cylindrical form, verify
primary animation preservation and repeatability, and require moving pairs to remain gated even when
their paths agree. Unit tests cover policy boundaries, unavailable readings, overflow, mesh displacement
and non-demotable flags. Real CLI tests cover preflight before workspace mutation, stale-output rejection
and failure recording. Full fitted-Anny paired-garment qualification requires the converted assets;
the asset-free tests do not replace it.
