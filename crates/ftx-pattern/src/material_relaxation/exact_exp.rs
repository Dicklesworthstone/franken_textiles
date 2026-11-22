//! Exact-exponential Prony recursion and Fractional-Zener → Prony NNLS lowering with certified
//! in-band supremum error bounds.
//!
//! Provenance: adapted from FrankenSim `crates/fs-material/src/visco.rs`
//! (`frankensim-source-manifest.json` `source_root`
//! `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`; MIT + OpenAI/Anthropic
//! rider). Adaptations for FrankenTextiles:
//! - uses [`ftx_core::det::{exp, expm1, ln, sin, cos, sqrt}`] (zero platform `libm`, deterministic
//!   across native and `wasm32-unknown-unknown`);
//! - zero `unsafe`, zero `mul_add`;
//! - structured [`Refusal`] errors (`OutOfRange` for invalid parameters, out-of-band evaluation, or
//!   unmet NNLS error gate);
//! - adds both scalar 1D [`ExactPronyModel`] (matching FrankenSim's piecewise-linear exact
//!   exponential recursion + energy/dissipation ledger) AND 3-component Green-strain Voigt helper
//!   [`exact_exp_weights`] (`a = exp(-h/tau)`, `b = -tau * expm1(-h/tau) / h`) usable directly
//!   for multi-decade relaxation without backward-Euler step-size distortion.

use std::f64::consts::{FRAC_PI_2, PI};

use ftx_core::{Refusal, RefusalCode, det};
use serde::Deserialize;
use serde_json::json;

fn out_of_range(msg: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, msg)
}

/// Exact-exponential recurrence weights `(a, b)` for a Maxwell branch with relaxation time `tau`
/// over a piecewise-linear strain step `dt >= 0`:
/// - `a = exp(-dt / tau)` (decay of old internal spring strain);
/// - `b = tau * (1 - a) / dt = -tau * expm1(-dt / tau) / dt` (exact ramp integration factor,
///   with `b = 1.0` at `dt = 0.0` so no `0/0` cancellation occurs for `dt << tau`).
///
/// # Errors
/// [`RefusalCode::OutOfRange`] unless `tau > 0` and `dt >= 0` are finite.
pub fn exact_exp_weights(tau: f64, dt: f64) -> Result<(f64, f64), Refusal> {
    if !(tau.is_finite() && tau > 0.0 && dt.is_finite() && dt >= 0.0) {
        return Err(out_of_range(
            "exact-exponential weights require finite tau > 0 and dt >= 0",
        ));
    }
    if dt == 0.0 {
        return Ok((1.0, 1.0));
    }
    let ratio = dt / tau;
    if ratio == 0.0 {
        return Ok((1.0, 1.0));
    }
    if !ratio.is_finite() {
        return Ok((0.0, 0.0));
    }
    let a = det::exp(-ratio);
    let b = -det::expm1(-ratio) / ratio;
    Ok((a, b))
}

#[inline]
fn prony_basis(wt: f64) -> (f64, f64) {
    if wt.abs() > 1e150 {
        (1.0, 1.0 / wt)
    } else {
        let wt2 = wt * wt;
        let den = 1.0 + wt2;
        (wt2 / den, wt / den)
    }
}

// ---------------------------------------------------------------------------
// Fractional Zener constitutive model (4 parameters, power-law textile creep/loss)
// ---------------------------------------------------------------------------

/// Four-parameter fractional Zener viscoelastic solid:
/// `E*(ω) = (E₀ + E_∞ (iωτ)^α) / (1 + (iωτ)^α)`, with `0 < E₀ < E_∞`, `0 < α ≤ 1`, `τ > 0`.
///
/// Captures broad, nearly frequency-constant damping/relaxation spectra typical of polymeric
/// fibers, knits, and elastane blends with only 4 parameters before lowering to a runtime Prony
/// series via [`lower_to_prony`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FractionalZener {
    /// Equilibrium (long-time) modulus `E₀` [N/m or Pa].
    pub e0: f64,
    /// Instantaneous (glassy) modulus `E_∞` [N/m or Pa].
    pub e_inf: f64,
    /// Fractional order `α ∈ (0, 1]`.
    pub alpha: f64,
    /// Characteristic relaxation time `τ` [s].
    pub tau: f64,
}

impl FractionalZener {
    /// Validate physical admissibility (`0 < E₀ < E_∞`, `0 < α ≤ 1`, `τ > 0`).
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] on invalid parameters.
    pub fn new(e0: f64, e_inf: f64, alpha: f64, tau: f64) -> Result<Self, Refusal> {
        let finite = e0.is_finite() && e_inf.is_finite() && alpha.is_finite() && tau.is_finite();
        if !finite || !(e0 > 0.0 && e_inf > e0 && alpha > 0.0 && alpha <= 1.0 && tau > 0.0) {
            return Err(out_of_range(
                "fractional Zener requires finite 0 < E0 < E_inf, 0 < alpha <= 1, tau > 0",
            ));
        }
        Ok(Self {
            e0,
            e_inf,
            alpha,
            tau,
        })
    }

    /// Dimensionless complex relaxation transition `h(ω) = (iωτ)^α / (1 + (iωτ)^α)` computed without
    /// overflow at large `ωτ` or cancellation when `E_∞ ≈ E_0`.
    fn normalized_response(&self, omega: f64) -> (f64, f64) {
        if omega.is_nan() || omega < 0.0 {
            return (f64::NAN, f64::NAN);
        }
        if omega == 0.0 {
            return (0.0, 0.0);
        }
        let wt = omega * self.tau;
        let x = if wt.is_infinite() {
            f64::INFINITY
        } else {
            det::exp(self.alpha * det::ln(wt))
        };
        let (s, c) = det::sin_cos(self.alpha * FRAC_PI_2);
        if x <= 1.0 {
            let xr = x * c;
            let xi = x * s;
            let dr = 1.0 + xr;
            let den = dr * dr + xi * xi;
            ((xr + x * x) / den, xi / den)
        } else {
            let y = 1.0 / x;
            let yr = y * c;
            let yi = y * s;
            let dr = 1.0 + yr;
            let den = dr * dr + yi * yi;
            (dr / den, yi / den)
        }
    }

    /// Storage and loss moduli `(E′, E″)` at angular frequency `omega ≥ 0` [rad/s].
    #[must_use]
    pub fn modulus(&self, omega: f64) -> (f64, f64) {
        let (h_re, h_im) = self.normalized_response(omega);
        let delta_e = self.e_inf - self.e0;
        (self.e0 + delta_e * h_re, delta_e * h_im)
    }

    /// Loss factor `η(ω) = E″ / E′`.
    #[must_use]
    pub fn loss_factor(&self, omega: f64) -> f64 {
        let (ep, epp) = self.modulus(omega);
        epp / ep
    }
}

// ---------------------------------------------------------------------------
// Exact-exponential Prony runtime model + thermodynamic ledger
// ---------------------------------------------------------------------------

/// Runtime Generalized Maxwell (Prony) series:
/// `E*(ω) = E_eq + Σ_j E_j (iωτ_j) / (1 + iωτ_j)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExactPronyModel {
    /// Equilibrium modulus `E_eq` [N/m or Pa].
    pub e_eq: f64,
    /// `(E_j, τ_j)` Maxwell branch pairs (`E_j ≥ 0`, `τ_j > 0`).
    pub terms: Vec<(f64, f64)>,
}

/// Internal-variable state and thermodynamic work/storage/dissipation ledger for
/// [`ExactPronyModel`].
#[derive(Debug, Clone, PartialEq)]
pub struct ExactPronyState {
    /// Current internal Maxwell spring strains `q_j`.
    pub q: Vec<f64>,
    /// Current total strain `ε`.
    pub strain: f64,
    /// Current total stress `σ`.
    pub stress: f64,
    /// Accumulated trapezoidal external work.
    pub work: f64,
    /// Stored Helmholtz elastic energy `½ E_eq ε² + Σ ½ E_j q_j²`.
    pub stored: f64,
    /// Ledgered viscous dissipation `work − stored` (nonnegative for admissible parameters).
    pub dissipated: f64,
}

impl ExactPronyModel {
    /// Admit a Prony series with `e_eq > 0` and every branch having `E_j ≥ 0, τ_j > 0`.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] on invalid parameters.
    pub fn new(e_eq: f64, terms: Vec<(f64, f64)>) -> Result<Self, Refusal> {
        if !(e_eq.is_finite() && e_eq > 0.0) {
            return Err(out_of_range(
                "equilibrium modulus must be finite and positive",
            ));
        }
        for &(e, tau) in &terms {
            if !(e.is_finite() && e >= 0.0 && tau.is_finite() && tau > 0.0) {
                return Err(out_of_range(
                    "Prony branches require finite E_j >= 0 and tau_j > 0",
                ));
            }
        }
        Ok(Self { e_eq, terms })
    }

    /// Storage and loss moduli `(E′, E″)` at angular frequency `omega` [rad/s].
    #[must_use]
    pub fn modulus(&self, omega: f64) -> (f64, f64) {
        let mut ep = self.e_eq;
        let mut epp = 0.0;
        for &(e, tau) in &self.terms {
            let (storage, loss) = prony_basis(omega * tau);
            ep += e * storage;
            epp += e * loss;
        }
        (ep, epp)
    }

    /// Fresh relaxed state at zero strain.
    #[must_use]
    pub fn state(&self) -> ExactPronyState {
        ExactPronyState {
            q: vec![0.0; self.terms.len()],
            strain: 0.0,
            stress: 0.0,
            work: 0.0,
            stored: 0.0,
            dissipated: 0.0,
        }
    }

    /// Advance one piecewise-linear strain step via exact exponential integration
    /// `q_j ← a_j q_j + b_j Δε` (unconditionally stable, convolution-free, exact for step holds
    /// and constant strain-rate ramps). Returns the updated stress.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] if `strain_new` or `dt` is non-finite or `dt < 0`.
    pub fn step(
        &self,
        state: &mut ExactPronyState,
        strain_new: f64,
        dt: f64,
    ) -> Result<f64, Refusal> {
        if !strain_new.is_finite() || !(dt.is_finite() && dt >= 0.0) {
            return Err(out_of_range(
                "exact Prony step requires finite strain and dt >= 0",
            ));
        }
        if state.q.len() != self.terms.len() {
            return Err(out_of_range("Prony state branch count mismatch"));
        }
        if !state.strain.is_finite()
            || !state.stress.is_finite()
            || !state.work.is_finite()
            || state.q.iter().any(|v| !v.is_finite())
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "Prony state contains non-finite values",
            ));
        }
        let d_eps = strain_new - state.strain;
        if !d_eps.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "Prony strain increment overflowed",
            ));
        }
        let stress_old = state.stress;
        let mut q_next = Vec::with_capacity(self.terms.len());
        for (j, &(_e, tau)) in self.terms.iter().enumerate() {
            let (a, b) = exact_exp_weights(tau, dt)?;
            let qj = a * state.q[j] + b * d_eps;
            if !qj.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "Prony internal spring strain overflowed",
                ));
            }
            q_next.push(qj);
        }
        let mut stress = self.e_eq * strain_new;
        let mut stored = 0.5 * self.e_eq * strain_new * strain_new;
        for (j, &(e, _tau)) in self.terms.iter().enumerate() {
            stress += e * q_next[j];
            stored += 0.5 * e * q_next[j] * q_next[j];
        }
        let work = state.work + 0.5 * (stress_old + stress) * d_eps;
        let dissipated = work - stored;
        if !(stress.is_finite() && stored.is_finite() && work.is_finite() && dissipated.is_finite())
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "Prony step stress or energy ledger overflowed",
            ));
        }
        state.q = q_next;
        state.strain = strain_new;
        state.stress = stress;
        state.work = work;
        state.stored = stored;
        state.dissipated = dissipated;
        Ok(stress)
    }
}

// ---------------------------------------------------------------------------
// Certified Fractional-Zener -> Prony NNLS lowering
// ---------------------------------------------------------------------------

/// A Prony model lowered from [`FractionalZener`], carrying its certified angular-frequency band
/// and measured 512-point in-band supremum relative complex-modulus error.
#[derive(Debug, Clone, PartialEq)]
pub struct LoweredProny {
    /// Runtime Prony series.
    pub model: ExactPronyModel,
    /// Certified angular-frequency band `[w_lo, w_hi]` [rad/s].
    pub band_rad_s: (f64, f64),
    /// Measured `sup |E*_prony − E*_frac| / |E*_frac|` over the verification grid.
    pub sup_rel_err: f64,
    /// Number of verification grid points.
    pub verification_points: usize,
}

impl LoweredProny {
    /// Evaluate `(E′, E″)` at `omega`, refusing outside the certified band fail-closed.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] outside `self.band_rad_s`.
    pub fn modulus_checked(&self, omega: f64) -> Result<(f64, f64), Refusal> {
        if !(omega.is_finite() && omega >= self.band_rad_s.0 && omega <= self.band_rad_s.1) {
            return Err(out_of_range(format!(
                "angular frequency {omega:.3e} rad/s outside certified Prony band [{:.3e}, {:.3e}]",
                self.band_rad_s.0, self.band_rad_s.1
            )));
        }
        Ok(self.model.modulus(omega))
    }
}

/// Dense log-spaced supremum relative complex-modulus error of `gm` against `fz` on `[w_lo, w_hi]`.
#[must_use]
pub fn sup_rel_modulus_error(
    fz: &FractionalZener,
    gm: &ExactPronyModel,
    w_lo: f64,
    w_hi: f64,
    points: usize,
) -> f64 {
    if points == 0 || !(w_lo.is_finite() && w_hi.is_finite() && w_lo > 0.0 && w_hi >= w_lo) {
        return f64::NAN;
    }
    let mut worst = 0.0f64;
    let log_lo = det::ln(w_lo);
    let log_hi = det::ln(w_hi);
    for i in 0..points {
        let t = i as f64 / (points - 1).max(1) as f64;
        let w = det::exp(log_lo + t * (log_hi - log_lo));
        let (fe, fi) = fz.modulus(w);
        let (ge, gi) = gm.modulus(w);
        let num = det::hypot(ge - fe, gi - fi);
        let den = det::hypot(fe, fi);
        worst = worst.max(num / den);
    }
    worst
}

/// Lower a [`FractionalZener`] model to an `n_terms` [`ExactPronyModel`] certified on `[f_lo, f_hi]`
/// Hz via deterministic log-spaced relaxation ladder + Lawson–Hanson NNLS + 512-point verification
/// sweep. Refuses fail-closed if the measured in-band supremum error exceeds `tol`.
///
/// # Errors
/// [`RefusalCode::OutOfRange`] on invalid band/count/tol or when the achieved error exceeds `tol`.
pub fn lower_to_prony(
    fz: &FractionalZener,
    f_lo_hz: f64,
    f_hi_hz: f64,
    n_terms: usize,
    tol: f64,
) -> Result<LoweredProny, Refusal> {
    if !(f_lo_hz.is_finite()
        && f_hi_hz.is_finite()
        && f_lo_hz > 0.0
        && f_hi_hz > f_lo_hz
        && tol.is_finite()
        && tol > 0.0)
    {
        return Err(out_of_range(
            "Prony lowering needs finite 0 < f_lo < f_hi and positive tolerance",
        ));
    }
    if !(1..=64).contains(&n_terms) {
        return Err(out_of_range("Prony term count must be in 1..=64"));
    }
    let w_lo = 2.0 * PI * f_lo_hz;
    let w_hi = 2.0 * PI * f_hi_hz;
    const MARGIN: f64 = 10.0;
    let tau_hi = MARGIN / w_lo;
    let tau_lo = 1.0 / (w_hi * MARGIN);
    if !(w_lo.is_finite()
        && w_hi.is_finite()
        && w_lo > 0.0
        && w_hi > w_lo
        && tau_lo.is_finite()
        && tau_hi.is_finite()
        && tau_lo > 0.0
        && tau_hi > tau_lo)
    {
        return Err(out_of_range(
            "frequency band overflows or underflows Prony relaxation-time ladder",
        ));
    }
    let log_tau_lo = det::ln(tau_lo);
    let log_tau_hi = det::ln(tau_hi);
    let taus: Vec<f64> = (0..n_terms)
        .map(|j| {
            let t = if n_terms == 1 {
                0.5
            } else {
                j as f64 / (n_terms - 1) as f64
            };
            det::exp(log_tau_lo + t * (log_tau_hi - log_tau_lo))
        })
        .collect();
    if taus.iter().any(|&t| !(t.is_finite() && t > 0.0)) {
        return Err(out_of_range(
            "Prony relaxation-time ladder became non-finite or non-positive",
        ));
    }

    let m_samples = (8 * n_terms).max(48);
    let log_w_lo = det::ln(w_lo);
    let log_w_hi = det::ln(w_hi);
    let omegas: Vec<f64> = (0..m_samples)
        .map(|i| {
            let t = i as f64 / (m_samples - 1) as f64;
            det::exp(log_w_lo + t * (log_w_hi - log_w_lo))
        })
        .collect();
    let delta_e = fz.e_inf - fz.e0;
    let rows = 2 * m_samples;
    let mut a = vec![0.0f64; rows * n_terms];
    let mut rhs = vec![0.0f64; rows];
    // Solve in dimensionless branch weights `c_j = E_j / (E_inf - E_0)` with O(1) weights
    // `E_0 / |E*(w)|` so the NNLS optimality threshold and normal equations are invariant across
    // arbitrary modulus units (from sub-N/m fabrics to GPa fibers) and tiny relaxation ratios.
    for (i, &w) in omegas.iter().enumerate() {
        let (h_re, h_im) = fz.normalized_response(w);
        let fe = fz.e0 + delta_e * h_re;
        let fi = delta_e * h_im;
        let mag = det::hypot(fe, fi);
        let weight = fz.e0 / mag;
        if !(fe.is_finite()
            && fi.is_finite()
            && mag.is_finite()
            && weight.is_finite()
            && weight > 0.0)
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "complex modulus evaluation overflowed or underflowed during Prony lowering",
            ));
        }
        rhs[i] = h_re * weight;
        rhs[m_samples + i] = h_im * weight;
        for (j, &tau) in taus.iter().enumerate() {
            let (storage, loss) = prony_basis(w * tau);
            a[i * n_terms + j] = storage * weight;
            a[(m_samples + i) * n_terms + j] = loss * weight;
        }
    }
    let coeffs = nnls(&a, &rhs, rows, n_terms);
    let mut terms = Vec::new();
    for (&c, &tau) in coeffs.iter().zip(&taus) {
        if c > 0.0 {
            let e = c * delta_e;
            if !(e.is_finite() && e > 0.0) {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "Prony branch modulus overflowed during rescaling",
                ));
            }
            terms.push((e, tau));
        }
    }
    let model = ExactPronyModel::new(fz.e0, terms)?;
    let verification_points = 512;
    let sup_rel_err = sup_rel_modulus_error(fz, &model, w_lo, w_hi, verification_points);
    if !(sup_rel_err.is_finite() && sup_rel_err <= tol) {
        return Err(out_of_range(format!(
            "NNLS Prony lowering sup relative error {sup_rel_err:.3e} exceeds tolerance {tol:.3e}"
        )));
    }
    Ok(LoweredProny {
        model,
        band_rad_s: (w_lo, w_hi),
        sup_rel_err,
        verification_points,
    })
}

/// Deterministic Lawson–Hanson NNLS (`min ‖Ax − b‖₂` s.t. `x ≥ 0`) for small systems (`cols ≤ 64`).
fn nnls(a: &[f64], b: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    let mut passive = vec![false; cols];
    let mut x = vec![0.0f64; cols];
    let a_scale = a.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
    let b_scale = b.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
    let grad_tol = 1e-12 * (a_scale * b_scale).max(f64::MIN_POSITIVE);
    for _ in 0..(2 * cols + 4) {
        let mut r = b.to_vec();
        for (i, ri) in r.iter_mut().enumerate() {
            for j in 0..cols {
                *ri -= a[i * cols + j] * x[j];
            }
        }
        let mut best = usize::MAX;
        let mut best_w = grad_tol;
        for j in 0..cols {
            if !passive[j] {
                let mut wj = 0.0;
                for i in 0..rows {
                    wj += a[i * cols + j] * r[i];
                }
                if wj > best_w {
                    best_w = wj;
                    best = j;
                }
            }
        }
        if best == usize::MAX {
            break;
        }
        passive[best] = true;
        let mut inner_guard = 0usize;
        while inner_guard < 4 * cols + 8 {
            inner_guard += 1;
            let idx: Vec<usize> = (0..cols).filter(|&j| passive[j]).collect();
            let z = ls_solve_subset(a, b, rows, cols, &idx);
            if z.iter().any(|v| !v.is_finite()) {
                break;
            }
            if z.iter().all(|&v| v > 0.0) {
                for (k, &j) in idx.iter().enumerate() {
                    x[j] = z[k];
                }
                for (j, slot) in x.iter_mut().enumerate() {
                    if !passive[j] {
                        *slot = 0.0;
                    }
                }
                break;
            }
            let mut alpha = 1.0f64;
            let mut drop_j = usize::MAX;
            for (k, &j) in idx.iter().enumerate() {
                if z[k] <= 0.0 {
                    let step = if x[j] <= 0.0 {
                        0.0
                    } else {
                        x[j] / (x[j] - z[k])
                    };
                    if step < alpha || (step == alpha && drop_j == usize::MAX) {
                        alpha = step;
                        drop_j = j;
                    }
                }
            }
            for (k, &j) in idx.iter().enumerate() {
                x[j] += alpha * (z[k] - x[j]);
            }
            let mut dropped = false;
            for &j in &idx {
                if x[j] <= 0.0 || j == drop_j {
                    passive[j] = false;
                    x[j] = 0.0;
                    dropped = true;
                }
            }
            if !dropped {
                break;
            }
        }
    }
    x
}

fn ls_solve_subset(a: &[f64], b: &[f64], rows: usize, cols: usize, idx: &[usize]) -> Vec<f64> {
    let k = idx.len();
    let mut ata = vec![0.0f64; k * k];
    let mut atb = vec![0.0f64; k];
    for (p, &jp) in idx.iter().enumerate() {
        for (q, &jq) in idx.iter().enumerate() {
            let mut acc = 0.0;
            for i in 0..rows {
                acc += a[i * cols + jp] * a[i * cols + jq];
            }
            ata[p * k + q] = acc;
        }
        let mut acc = 0.0;
        for i in 0..rows {
            acc += a[i * cols + jp] * b[i];
        }
        atb[p] = acc;
    }
    for col in 0..k {
        let mut piv = col;
        for r in (col + 1)..k {
            if ata[r * k + col].abs() > ata[piv * k + col].abs() {
                piv = r;
            }
        }
        if piv != col {
            for c in 0..k {
                ata.swap(col * k + c, piv * k + c);
            }
            atb.swap(col, piv);
        }
        let d = ata[col * k + col];
        if d.abs() < 1e-300 {
            continue;
        }
        for r in (col + 1)..k {
            let f = ata[r * k + col] / d;
            for c in col..k {
                ata[r * k + c] -= f * ata[col * k + c];
            }
            atb[r] -= f * atb[col];
        }
    }
    let mut z = vec![0.0f64; k];
    for r in (0..k).rev() {
        let mut acc = atb[r];
        for c in (r + 1)..k {
            acc -= ata[r * k + c] * z[c];
        }
        let d = ata[r * k + r];
        z[r] = if d.abs() < 1e-300 { 0.0 } else { acc / d };
    }
    z
}

/// Request schema identifier for [`evaluate_lowering_json`].
pub const LOWERING_REQUEST_SCHEMA: &str = "ftx-fractional-zener-lowering/1";
/// Report schema identifier emitted by [`evaluate_lowering_json`].
pub const LOWERING_REPORT_SCHEMA: &str = "ftx-fractional-zener-lowering-report/1";

const MAX_LOWERING_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_REPLAY_STEPS: usize = 65_536;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoweringRequest {
    schema: String,
    e0: f64,
    e_inf: f64,
    alpha: f64,
    tau_s: f64,
    f_lo_hz: f64,
    f_hi_hz: f64,
    n_terms: usize,
    rel_tol: f64,
    #[serde(default)]
    replay_steps: Vec<ReplayStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayStep {
    dt_s: f64,
    strain: f64,
}

/// Evaluate a bounded `ftx-fractional-zener-lowering/1` JSON request: lower a 4-parameter
/// [`FractionalZener`] solid to a certified nonnegative [`ExactPronyModel`] via Lawson–Hanson NNLS
/// over `[f_lo_hz, f_hi_hz]`, optionally replaying piecewise-linear strain steps through exact
/// exponential recursion with full thermodynamic work/stored/dissipated-energy accounting.
///
/// # Errors
/// [`RefusalCode::BudgetExceeded`] on input over 16 MiB or more than 65,536 replay steps;
/// [`RefusalCode::InvalidDocument`] on invalid JSON, unknown fields, or mismatched schema;
/// [`RefusalCode::OutOfRange`] on invalid parameters or unmet NNLS error gate.
pub fn evaluate_lowering_json(input: &str) -> Result<String, Refusal> {
    if input.len() > MAX_LOWERING_INPUT_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "fractional-Zener lowering input exceeds 16 MiB",
        ));
    }
    let req: LoweringRequest = serde_json::from_str(input)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    if req.schema != LOWERING_REQUEST_SCHEMA {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            format!(
                "expected schema {LOWERING_REQUEST_SCHEMA:?}, got {:?}",
                req.schema
            ),
        ));
    }
    if req.replay_steps.len() > MAX_REPLAY_STEPS {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "replay_steps length {} exceeds cap {MAX_REPLAY_STEPS}",
                req.replay_steps.len()
            ),
        ));
    }
    let fz = FractionalZener::new(req.e0, req.e_inf, req.alpha, req.tau_s)?;
    let lowered = lower_to_prony(&fz, req.f_lo_hz, req.f_hi_hz, req.n_terms, req.rel_tol)?;
    let terms: Vec<_> = lowered
        .model
        .terms
        .iter()
        .map(|&(modulus, tau_s)| json!({ "modulus": modulus, "tau_s": tau_s }))
        .collect();
    let replay = if req.replay_steps.is_empty() {
        None
    } else {
        let mut st = lowered.model.state();
        let mut t_s = 0.0f64;
        let mut trace = Vec::with_capacity(req.replay_steps.len());
        for s in &req.replay_steps {
            let stress = lowered.model.step(&mut st, s.strain, s.dt_s)?;
            t_s += s.dt_s;
            if !t_s.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    "replay cumulative time overflowed",
                ));
            }
            trace.push(json!({
                "time_s": t_s,
                "strain": st.strain,
                "stress": stress,
                "work": st.work,
                "stored": st.stored,
                "dissipated": st.dissipated,
            }));
        }
        Some(json!({
            "final_time_s": t_s,
            "final_strain": st.strain,
            "final_stress": st.stress,
            "work": st.work,
            "stored": st.stored,
            "dissipated": st.dissipated,
            "steps": trace,
        }))
    };
    let report = json!({
        "schema": LOWERING_REPORT_SCHEMA,
        "e_eq": lowered.model.e_eq,
        "terms": terms,
        "band_hz": [req.f_lo_hz, req.f_hi_hz],
        "band_rad_s": [lowered.band_rad_s.0, lowered.band_rad_s.1],
        "sup_rel_err": lowered.sup_rel_err,
        "verification_points": lowered.verification_points,
        "replay": replay,
        "no_claim": "Certified in-band NNLS Prony approximation and exact-exponential 1D recurrence only; valid solely inside band_hz and not an empirical textile characterization or physical fit claim."
    });
    serde_json::to_string(&report)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_exponential_prony_is_step_size_invariant_on_relaxation_hold_and_dissipates_nonnegatively()
     {
        let model = ExactPronyModel::new(100.0, vec![(40.0, 0.5), (25.0, 12.0)]).unwrap();
        // Instantaneous strain jump dt = 0 then hold for T = 2.0 s in 1 big step vs 200 substeps.
        let mut coarse = model.state();
        model.step(&mut coarse, 0.05, 0.0).unwrap();
        let s_coarse = model.step(&mut coarse, 0.05, 2.0).unwrap();

        let mut fine = model.state();
        model.step(&mut fine, 0.05, 0.0).unwrap();
        let mut s_fine = 0.0;
        for _ in 0..200 {
            s_fine = model.step(&mut fine, 0.05, 0.01).unwrap();
        }
        assert!(
            (s_coarse - s_fine).abs() < 1e-12,
            "exact exponential hold must be step-size invariant: coarse={s_coarse}, fine={s_fine}"
        );
        // Analytic relaxation stress: E_eq * eps + Σ E_j * eps * exp(-T / tau_j).
        let expected =
            100.0 * 0.05 + 40.0 * 0.05 * det::exp(-2.0 / 0.5) + 25.0 * 0.05 * det::exp(-2.0 / 12.0);
        assert!((s_coarse - expected).abs() < 1e-12);

        // Cyclic ramp-load and unload must yield nonnegative, monotone dissipation.
        let mut cyc = model.state();
        let mut prev_diss = 0.0;
        for k in 1..=40 {
            let t = k as f64 * 0.1;
            let eps = 0.04 * det::sin(t);
            model.step(&mut cyc, eps, 0.1).unwrap();
            assert!(
                cyc.dissipated >= prev_diss - 1e-12,
                "dissipation must be non-decreasing: prev={prev_diss}, now={}",
                cyc.dissipated
            );
            prev_diss = cyc.dissipated;
        }
        assert!(cyc.dissipated > 0.0);

        // Overflowing strain must refuse NonFinite transactionally without mutating state.
        let snapshot = cyc.clone();
        let err = model.step(&mut cyc, 1e200, 0.1).unwrap_err();
        assert_eq!(err.code, RefusalCode::NonFinite);
        assert_eq!(cyc, snapshot);
    }

    #[test]
    fn fractional_zener_lowers_to_certified_prony_and_refuses_out_of_band() {
        // Broad-spectrum polymeric textile / elastane-blend relaxation: α = 0.35.
        let fz = FractionalZener::new(200.0, 500.0, 0.35, 1.0).unwrap();
        let lowered = lower_to_prony(&fz, 0.01, 100.0, 14, 0.02).unwrap();
        assert!(lowered.sup_rel_err <= 0.02, "err = {}", lowered.sup_rel_err);
        // Overflowing frequency band must refuse cleanly instead of propagating NaN.
        assert!(lower_to_prony(&fz, 1e-308, 1e308, 8, 0.05).is_err());
        assert!(!lowered.model.terms.is_empty());

        // In-band query succeeds; out-of-band query refuses fail-closed.
        let mid_w = 2.0 * PI * 1.0;
        let (ep, epp) = lowered.modulus_checked(mid_w).unwrap();
        let (fe, fi) = fz.modulus(mid_w);
        assert!(((ep - fe).hypot(epp - fi) / fe.hypot(fi)) <= 0.02);
        assert!(lowered.modulus_checked(1e-6).is_err());
        assert!(lowered.modulus_checked(1e6).is_err());

        // Impossible tolerance refuses fail-closed.
        assert!(lower_to_prony(&fz, 0.01, 100.0, 3, 1e-6).is_err());
    }

    #[test]
    fn evaluate_lowering_json_certifies_prony_replays_history_and_refuses_bad_band() {
        let req = serde_json::json!({
            "schema": LOWERING_REQUEST_SCHEMA,
            "e0": 200.0,
            "e_inf": 500.0,
            "alpha": 0.35,
            "tau_s": 1.0,
            "f_lo_hz": 0.01,
            "f_hi_hz": 100.0,
            "n_terms": 14,
            "rel_tol": 0.02,
            "replay_steps": [
                { "dt_s": 0.0, "strain": 0.05 },
                { "dt_s": 0.5, "strain": 0.05 },
                { "dt_s": 0.5, "strain": 0.0 }
            ]
        });
        let raw = evaluate_lowering_json(&req.to_string()).unwrap();
        let rep: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(rep["schema"], LOWERING_REPORT_SCHEMA);
        assert!(rep["sup_rel_err"].as_f64().unwrap() <= 0.02);
        assert!(!rep["terms"].as_array().unwrap().is_empty());
        assert!(rep["replay"]["dissipated"].as_f64().unwrap() > 0.0);
        assert_eq!(rep["replay"]["steps"].as_array().unwrap().len(), 3);

        // Unmet tolerance and unknown fields refuse fail-closed.
        let mut tight = req.clone();
        tight["n_terms"] = serde_json::json!(2);
        tight["rel_tol"] = serde_json::json!(1e-6);
        assert!(evaluate_lowering_json(&tight.to_string()).is_err());

        let mut extra = req;
        extra["bogus"] = serde_json::json!(1);
        assert!(evaluate_lowering_json(&extra.to_string()).is_err());
    }

    #[test]
    fn exact_exp_weights_and_prony_modulus_stay_finite_on_extreme_ratios() {
        assert_eq!(exact_exp_weights(1e200, 1e-200).unwrap(), (1.0, 1.0));
        assert_eq!(exact_exp_weights(1e-200, 1e200).unwrap(), (0.0, 0.0));
        let model = ExactPronyModel::new(100.0, vec![(40.0, 1.0)]).unwrap();
        let (ep, epp) = model.modulus(1e200);
        assert!(ep.is_finite() && (ep - 140.0).abs() < 1e-12);
        assert!(epp.is_finite() && (0.0..1e-190).contains(&epp));
    }

    #[test]
    fn fractional_zener_lowering_is_scale_invariant_and_overflow_free() {
        // Real polymeric/cellulosic fibers span Pa to GPa (1e9–1e12 Pa). Dimensionless NNLS
        // normalization must yield identical relative accuracy across 15+ decades of modulus scale.
        let baseline = FractionalZener::new(200.0, 500.0, 0.35, 1.0).unwrap();
        let base_low = lower_to_prony(&baseline, 0.01, 100.0, 14, 0.02).unwrap();
        for scale in [1e-6_f64, 1.0, 1e7, 1e10, 1e12] {
            let fz = FractionalZener::new(200.0 * scale, 500.0 * scale, 0.35, 1.0).unwrap();
            let low = lower_to_prony(&fz, 0.01, 100.0, 14, 0.02).unwrap();
            assert!(
                low.sup_rel_err <= 0.02,
                "scale={scale:.1e} err={:.3e}",
                low.sup_rel_err
            );
            assert!(
                (low.sup_rel_err - base_low.sup_rel_err).abs() < 1e-12,
                "scale={scale:.1e} drifted from baseline: {} vs {}",
                low.sup_rel_err,
                base_low.sup_rel_err
            );
            assert_eq!(low.model.terms.len(), base_low.model.terms.len());
        }

        // Extreme frequency (omega * tau = 1e200) must converge smoothly to (E_inf, 0) without NaN,
        // omega = 0 gives (E_0, 0), and tiny relaxation ratio keeps nonnegative accurate loss E''.
        let (hi_re, hi_im) = baseline.modulus(1e200);
        assert_eq!(hi_re, 500.0);
        assert!((0.0..1e-60).contains(&hi_im));
        assert_eq!(baseline.modulus(0.0), (200.0, 0.0));

        let tiny = FractionalZener::new(1e10, 1e10 * (1.0 + 1e-10), 0.5, 1.0).unwrap();
        let (re, im) = tiny.modulus(1.0);
        // At omega * tau = 1, alpha = 0.5: h_im = sin(pi/4) / (2 + 2 cos(pi/4)) = sqrt(2)/(4 + 2 sqrt(2)).
        let want_im = (tiny.e_inf - tiny.e0) * (std::f64::consts::FRAC_1_SQRT_2)
            / (2.0 + 2.0 * std::f64::consts::FRAC_1_SQRT_2);
        assert!(re >= tiny.e0 && re <= tiny.e_inf);
        assert!(
            im > 0.0 && (im - want_im).abs() <= 1e-12 * want_im,
            "im={im:.12e} want={want_im:.12e}"
        );
    }
}
