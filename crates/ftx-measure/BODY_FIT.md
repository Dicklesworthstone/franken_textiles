# Articulated multi-frame fitting boundary (FTX-21)

`body_fit::fit_body` estimates one shared model shape and an independent pose for each
calibrated decoded frame. `BodyModel` supplies ordered keypoints, rest-body tape functionals,
parameter bounds and model identity; ftx-measure does not depend on ftx-body or a detector.
This is the keypoint/tape slice, not the complete silhouette or real-photo acceptance gate.

The input is `ftx-body-fit-input/1`: 2–8 fixed metric cameras with oriented image metadata,
explicit named pixel correspondences and uncertainties, optional independent tape targets in
metres, and 2–4 distinct starts with frame-identified pose vectors. Every pose is independently
variable; the subject need not be stationary between frames. A frozen image must still be a
valid exposure of the stated pose: rolling shutter and motion blur are not modeled.

One vector of shape parameters is shared by all frames. Shape and per-frame root distance can
compensate for one another even with known camera intrinsics. Tape evidence or other genuinely
observable constraints are needed to resolve that gauge. No chart prior, initial guess or LM
damping is allowed to turn deficient data into an observable solution.

The local solver uses central differences in normalized bounded parameter coordinates,
column-scaled Levenberg–Marquardt and strict objective descent. It tests the undamped normalized
Gram matrix with a 1e-9 Cholesky pivot floor before adding damping. This is a conservative local
rank/conditioning check, not a covariance or global-uniqueness proof. Parameters never silently
clamp: starts are strictly interior and boundary stalls require an explicit repair. Hard budgets
are 12 shared shape variables, 18 pose variables per frame, 128 variables overall, 128 named
keypoints per frame, 32 scalar tape targets and 100 accepted steps per start (default 50).

Only fit observations affect objective, derivatives, rank, stopping or choice of the lowest-cost
solution. All fit and held-out image residuals must be at most 3 px regardless of declared sigma.
All tape residuals must be within three declared sigmas. A failed start remains a failed cell;
no optimized geometry is published unless every start converges and passes every residual gate.
Distinct successful shared-shape solutions must agree within the configured fraction of their
parameter ranges (default 0.01), otherwise the result is AMBIGUOUS with no solution. Agreement
between a few starts does not prove global uniqueness or pose uniqueness.

Reports retain canonical inputs, parameter/correspondence layout, numerical policy, every
initial/accepted-step cost, fit/held-out residual checks and normalized shared-shape spread.
BUDGET_STOP, NOT_CONVERGED, UNOBSERVABLE, REJECTED and AMBIGUOUS reports have no solution.
Accepted measurements are explicitly model predictions, without an invented sigma or a record
eligible for measurement fusion. No READY or real-wearer fit claim is made.

`tests/body_fit.rs` supplies analytical articulated geometry, not private measurements or a
candidate-generated oracle: shared-shape/independent-pose recovery, scale-depth ambiguity,
held-out independence and rejection, noisy fitting, order invariance, budget-stop publication,
strict schema/bounds and two distinct full-local-rank shape minima. The Anny adapter and CLI
are separate consumers of this boundary. Cameras/lens calibration, correspondence definitions,
metric-reference uncertainty, silhouette fitting, external inference and real-photo validation
remain outside the numerical core implemented here.
