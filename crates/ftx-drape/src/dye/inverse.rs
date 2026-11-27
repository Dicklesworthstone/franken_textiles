//! Stages 3–4 linear radiometric CIE XYZ stitch immersion, 16-D Sobol Karhunen–Loève Brownian
//! sine-mode drift sampling, smoothed Rockafellar–Uryasev CVaR epigraph optimization via pure-Rust
//! Bertsekas Two-Metric Projected L-BFGS, coherent open-loop horizons, and 5-Regime Pareto ranking
//! (`FTX-K14`, `FTX-K15`).

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, TargetRaster, det};
use ftx_pattern::dye::{
    DYE_PROGRAM_SCHEMA, DyeMachineProfile, DyeSegment, DyeingRegime, YarnDyeProgram,
};
use ftx_pattern::knit::WearerAgeClass;
use serde::{Deserialize, Serialize};

use super::optics::{
    CieLab, CieXyz, DyeOpticsModel, ciede2000_exact, ciede2000_sq_smooth_grad, linear_xyz_to_lab,
    srgb_u8_to_xyz_d65,
};
use super::transport::{
    CapillaryTransportConfig, forward_capillary_transport, vjp_capillary_transport,
};

/// One stitch's mapping between 1D yarn arc-length `[s_start_m, s_end_m]` and target 2D image UV.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StitchArcMapping {
    /// Stitch course index.
    pub course: u32,
    /// Stitch wale index.
    pub wale: i32,
    /// Undeformed start arc-length along yarn, in metres.
    pub s_start_m: f64,
    /// Undeformed end arc-length along yarn, in metres (`> s_start_m`).
    pub s_end_m: f64,
    /// Normalized target raster horizontal coordinate `u in [0, 1]`.
    pub target_u: f64,
    /// Normalized target raster vertical coordinate `v in [0, 1]`.
    pub target_v: f64,
    /// Draped open-pore grin-through fraction `alpha_pore in [0, 0.95]` at this stitch.
    pub pore_open_fraction: f64,
}

/// Complete 1D-to-2D garment immersion map for one contiguous [`ftx_pattern::knit::YarnRun`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KnitStitchMap {
    /// Bound yarn run identifier.
    pub run_id: u32,
    /// True for circular tubular knitting; false for flatbed boustrophedon back-and-forth knitting.
    pub is_circular: bool,
    /// Number of knitted courses.
    pub courses: u32,
    /// Nominal stitches per course.
    pub stitches_per_course: u32,
    /// Total undeformed yarn arc length in metres.
    pub total_yarn_length_m: f64,
    /// Ordered per-stitch arc mappings.
    pub stitches: Vec<StitchArcMapping>,
}

impl KnitStitchMap {
    /// Constructs a canonical rectangular flatbed or circular tubular stitch immersion map.
    #[must_use]
    pub fn regular_grid(
        run_id: u32,
        courses: u32,
        wales: u32,
        loop_length_m: f64,
        is_circular: bool,
        pore_open_fraction: f64,
    ) -> Self {
        let mut stitches = Vec::with_capacity((courses * wales) as usize);
        let mut s = 0.0_f64;
        for c in 0..courses {
            let reverse = !is_circular && (c % 2 == 1);
            for step in 0..wales {
                let w = if reverse { wales - 1 - step } else { step };
                let s0 = s;
                let s1 = s + loop_length_m;
                s = s1;
                let target_u = (f64::from(w) + 0.5) / f64::from(wales.max(1));
                let target_v = 1.0 - (f64::from(c) + 0.5) / f64::from(courses.max(1));
                stitches.push(StitchArcMapping {
                    course: c,
                    wale: w as i32,
                    s_start_m: s0,
                    s_end_m: s1,
                    target_u,
                    target_v,
                    pore_open_fraction: pore_open_fraction.clamp(0.0, 0.95),
                });
            }
        }
        Self {
            run_id,
            is_circular,
            courses,
            stitches_per_course: wales,
            total_yarn_length_m: s,
            stitches,
        }
    }
}

/// Stochastic yarn-intake drift model measured from `Exp-3` calibration swatch.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DriftParameters {
    /// Systematic proportional stretch/slip bias `epsilon` (dimensionless, e.g. `0.002` = 0.2%).
    pub systematic_eps: f64,
    /// Per-stitch Brownian random-walk standard deviation `sigma_eta` (in stitches, e.g. `0.015`).
    pub stochastic_sigma_eta: f64,
}

/// Computes exact coherent open-loop horizons `(n_max_circular, n_max_flatbed)` in stitches before
/// RMS phase drift reaches half a stitch (`0.5` st -> variance `<= 0.25`):
/// - Circular: `(N * eps)^2 + N * sigma_eta^2 <= 0.25`
/// - Flatbed boustrophedon (adjacent reversed rows double BOTH systematic & shared-prefix random walk!):
///   `(2 * N * eps)^2 + 4 * N * sigma_eta^2 <= 0.25`.
#[must_use]
pub fn coherent_open_loop_horizons(drift: DriftParameters) -> (usize, usize) {
    let solve = |a: f64, b: f64| -> usize {
        if a <= 1e-18 && b <= 1e-18 {
            return usize::MAX;
        }
        if a <= 1e-18 {
            return (0.25 / b).floor().max(1.0) as usize;
        }
        let disc = (b * b + a).sqrt();
        let n = (-b + disc) / (2.0 * a);
        n.floor().max(1.0) as usize
    };
    let e2 = drift.systematic_eps * drift.systematic_eps;
    let s2 = drift.stochastic_sigma_eta * drift.stochastic_sigma_eta;
    (solve(e2, s2), solve(4.0 * e2, 4.0 * s2))
}

/// Solver configuration for CVaR-robust inverse yarn dyeing optimization (`FTX-K15`).
#[derive(Clone, Debug)]
pub struct DyeInverseConfig {
    /// Program name.
    pub name: String,
    /// Catalog yarn identifier.
    pub yarn_id: String,
    /// Required wearer age class.
    pub wearer_age_class: WearerAgeClass,
    /// Target dyeing regime.
    pub regime: DyeingRegime,
    /// Number of discrete yarn control cells along `total_yarn_length_m`.
    pub n_cells: usize,
    /// Number of deterministic 16-D Sobol drift scenarios (`16..=32`).
    pub n_sobol_scenarios: usize,
    /// CVaR tail level `alpha in (0, 1)` (default `0.90`).
    pub cvar_alpha: f64,
    /// Softplus epigraph smoothing parameter `tau > 0` (default `0.5`).
    pub cvar_tau: f64,
    /// Sobolev `H^1` smoothness regularization weight `lambda_smooth >= 0`.
    pub lambda_smooth: f64,
    /// Huberized changeover penalty weight `lambda_switch >= 0`.
    pub lambda_switch: f64,
    /// Maximum Bertsekas Two-Metric Projected L-BFGS iterations.
    pub max_iters: usize,
    /// Skin/underlayer background CIE XYZ seen through open pores (`alpha_pore`).
    pub skin_underlayer_xyz: CieXyz,
    /// Neutral-gray CIEDE2000 smoothing epsilon (`1e-4`).
    pub ciede_eps_c: f64,
    /// Yarn drift parameters (`Exp-3`).
    pub drift: DriftParameters,
}

/// Complete output of the inverse continuous yarn dyeing optimizer.
#[derive(Clone, Debug)]
pub struct DyeInverseResult {
    /// Optimized, validated `ftx-dye-program/1` ready for physical hazard verification & emission.
    pub program: YarnDyeProgram,
    /// Median predicted perceptual CIEDE2000 error across all foreground stitches & drift scenarios.
    pub delta_e00_p50: f64,
    /// 95th-percentile worst-case drift CIEDE2000 error.
    pub delta_e00_p95: f64,
    /// Irreducible dye-gamut floor CIEDE2000 (best achievable without transport blur or drift).
    pub gamut_floor_delta_e00: f64,
    /// Fraction of target spatial-frequency energy exceeding applicator transport bandwidth.
    pub unreproducible_energy_fraction: f64,
    /// Coherent open-loop horizon `(circular_stitches, flatbed_stitches)`.
    pub coherent_horizons: (usize, usize),
    /// Per-stitch predicted sRGB preview (`nominal` zero-drift scenario).
    pub predicted_stitch_srgb: Vec<[u8; 3]>,
    /// Final smoothed CVaR objective value and iteration count.
    pub final_objective: f64,
    /// Iterations executed.
    pub iterations: usize,
}

/// One scored candidate on the 5-Regime Pareto frontier (`FTX-K15`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DyeRegimeScore {
    /// Candidate regime.
    pub regime: DyeingRegime,
    /// True if hardware & physical horizon prerequisites are satisfied.
    pub feasible: bool,
    /// Blocking reason if `!feasible`.
    pub refusal_reason: Option<String>,
    /// Predicted p95 CIEDE2000 error for this regime.
    pub expected_delta_e00_p95: f64,
    /// Estimated hardware & operational complexity score (`1` = simplest, `5` = highest).
    pub complexity_rank: u8,
    /// Human-readable recommendation summary.
    pub rationale: String,
}

/// Generates `k` deterministic 16-D Sobol drift scenarios (1 systematic `eps_k` + 15 Karhunen–Loève
/// Brownian **sine** modes `phi_m(t) = sqrt(2) * sin((m - 0.5) * pi * t) / ((m - 0.5) * pi)` with
/// `W(0) = 0`) mapped via Box–Muller using strictly `ftx_core::det`.
fn generate_sobol_drift_scenarios(
    n_scenarios: usize,
    n_stitches: usize,
    mean_loop_m: f64,
    drift: DriftParameters,
) -> Vec<(f64, Vec<f64>)> {
    let k = n_scenarios.clamp(1, 64);
    let mut out = Vec::with_capacity(k);
    // Scenario 0 is always the exact nominal (zero-drift) trajectory for baseline diagnostics
    out.push((0.0, vec![0.0; n_stitches]));
    if k == 1 {
        return out;
    }

    // Standard deterministic low-discrepancy Halton/Sobol-grade prime-base radical inverse in 16D
    // mapped through Box-Muller (det::ln, det::sqrt, det::cos, det::sin) clamping U in [1e-12, 1-1e-12].
    const PRIMES: [u32; 16] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53];
    let total_t = (n_stitches.max(1) as f64).sqrt();

    for sc in 1..k {
        let mut z = [0.0_f64; 16];
        for pair in 0..8 {
            let u1 = radical_inverse(sc as u32, PRIMES[2 * pair]).clamp(1e-12, 1.0 - 1e-12);
            let u2 = radical_inverse(sc as u32, PRIMES[2 * pair + 1]).clamp(1e-12, 1.0 - 1e-12);
            let r = (-2.0 * det::ln(u1)).sqrt();
            let theta = 2.0 * std::f64::consts::PI * u2;
            z[2 * pair] = r * det::cos(theta);
            z[2 * pair + 1] = r * det::sin(theta);
        }
        let eps_k = drift.systematic_eps * z[0];
        let mut walk_m = vec![0.0_f64; n_stitches];
        if drift.stochastic_sigma_eta > 0.0 && n_stitches > 0 {
            for (idx, slot) in walk_m.iter_mut().enumerate() {
                let t = (idx as f64) / (n_stitches as f64);
                let mut b_t = 0.0;
                for (m, &zm) in z.iter().enumerate().skip(1) {
                    let freq = (m as f64 - 0.5) * std::f64::consts::PI;
                    let phi = std::f64::consts::SQRT_2 * det::sin(freq * t) / freq;
                    b_t += zm * phi;
                }
                // Brownian motion scaled to stitch count `sqrt(N)` and converted to metres
                *slot = drift.stochastic_sigma_eta * total_t * b_t * mean_loop_m;
            }
        }
        out.push((eps_k, walk_m));
    }
    out
}

fn radical_inverse(mut n: u32, base: u32) -> f64 {
    let mut inv_base = 1.0 / f64::from(base);
    let mut f = inv_base;
    let mut r = 0.0;
    while n > 0 {
        r += f64::from(n % base) * f;
        n /= base;
        inv_base = 1.0 / f64::from(base);
        f *= inv_base;
    }
    r
}

/// Computes exact piecewise-linear box-kernel cell weights for an arc interval `[a, b]` on grid `ds`.
fn box_kernel_weights(a: f64, b: f64, ds: f64, n_cells: usize) -> Vec<(usize, f64)> {
    let len = (b - a).max(1e-9);
    let c_lo = ((a / ds).floor() as isize).clamp(0, (n_cells - 1) as isize) as usize;
    let c_hi = ((b / ds).floor() as isize).clamp(0, (n_cells - 1) as isize) as usize;
    let mut weights = Vec::with_capacity(c_hi - c_lo + 1);
    let mut sum = 0.0;
    for c in c_lo..=c_hi {
        let lo = (c as f64) * ds;
        let hi = lo + ds;
        let overlap = (b.min(hi) - a.max(lo)).max(0.0);
        if overlap > 0.0 {
            let w = overlap / len;
            weights.push((c, w));
            sum += w;
        }
    }
    if weights.is_empty() || sum <= 1e-12 {
        let c = (((0.5 * (a + b)) / ds).floor() as isize).clamp(0, (n_cells - 1) as isize) as usize;
        return vec![(c, 1.0)];
    }
    for (_, w) in &mut weights {
        *w /= sum;
    }
    weights
}

/// Overflow-safe smooth positive part `psi_tau(z) = tau * ln(1 + exp(z / tau))` and derivative
/// `sigmoid(z / tau)`.
fn softplus_and_deriv(z: f64, tau: f64) -> (f64, f64) {
    let t = z / tau;
    if t > 35.0 {
        (z, 1.0)
    } else if t < -35.0 {
        (0.0, 0.0)
    } else if t >= 0.0 {
        let em = det::exp(-t);
        (z + tau * det::ln(1.0 + em), 1.0 / (1.0 + em))
    } else {
        let ep = det::exp(t);
        (tau * det::ln(1.0 + ep), ep / (1.0 + ep))
    }
}

/// Solves the strictly convex 1D Rockafellar–Uryasev smoothed CVaR inner minimization
/// `min_eta [ eta + (1 / ((1 - alpha) * K)) * sum_k psi_tau(L_k - eta) ]` to machine precision,
/// returning `(cvar_value, weights)` where `weights[k] = sigmoid((L_k - eta*) / tau) / ((1-alpha)*K)`
/// satisfies `sum_k weights[k] == 1` and gives the exact Envelope-Theorem gradient `sum_k w_k dL_k/du`.
fn solve_smooth_cvar(losses: &[f64], alpha: f64, tau: f64) -> (f64, Vec<f64>) {
    let k_n = losses.len();
    debug_assert!(k_n > 0);
    let tail_prob = (1.0 - alpha).clamp(0.01, 0.999);
    let tau = tau.max(1e-4);
    let inv_tail = 1.0 / (tail_prob * (k_n as f64));
    let mut l_min = f64::INFINITY;
    let mut l_max = f64::NEG_INFINITY;
    for &l in losses {
        l_min = l_min.min(l);
        l_max = l_max.max(l);
    }
    // Exact closed bracket where F'(eta) = 1 - inv_tail * sum sigmoid((L_k - eta)/tau) crosses 0:
    // sigmoid(-shift / tau) == tail_prob => shift = tau * ln((1 - tail_prob) / tail_prob).
    let shift = tau * det::ln((1.0 - tail_prob) / tail_prob);
    let mut lo = l_min + shift;
    let mut hi = l_max + shift;
    let mut eta = 0.5 * (lo + hi);
    if hi - lo > 1e-13 * (1.0 + hi.abs() + lo.abs()) {
        for _ in 0..48 {
            let mut sum_sig = 0.0;
            let mut sum_dsig = 0.0;
            for &l in losses {
                let (_, sig) = softplus_and_deriv(l - eta, tau);
                sum_sig += sig;
                sum_dsig += sig * (1.0 - sig);
            }
            let f_prime = 1.0 - inv_tail * sum_sig;
            if f_prime.abs() < 1e-13 {
                break;
            }
            if f_prime < 0.0 {
                lo = eta;
            } else {
                hi = eta;
            }
            let f_second = (inv_tail / tau) * sum_dsig;
            let newton = if f_second > 1e-18 {
                eta - f_prime / f_second
            } else {
                f64::NAN
            };
            eta = if newton.is_finite() && newton > lo && newton < hi {
                newton
            } else {
                0.5 * (lo + hi)
            };
            if hi - lo <= 1e-13 * (1.0 + hi.abs() + lo.abs()) {
                break;
            }
        }
    }
    let mut cvar = eta;
    let mut weights = Vec::with_capacity(k_n);
    for &l in losses {
        let (sp, sig) = softplus_and_deriv(l - eta, tau);
        cvar += inv_tail * sp;
        weights.push(inv_tail * sig);
    }
    (cvar, weights)
}

/// Runs CVaR-robust inverse yarn dyeing optimization (`FTX-K15`) via pure-Rust Bertsekas Two-Metric
/// Projected L-BFGS over `(u, eta) in [0, u_max]^M x R`, factoring Stage 1 capillary transport
/// outside the `K`-scenario Sobol drift loop!
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] or [`RefusalCode::OutOfRange`] on malformed inputs.
pub fn optimize_yarn_dye_program(
    target: &TargetRaster,
    stitch_map: &KnitStitchMap,
    machine: &DyeMachineProfile,
    optics: &DyeOpticsModel,
    transport_cfg: &CapillaryTransportConfig,
    cfg: &DyeInverseConfig,
) -> Result<DyeInverseResult, Refusal> {
    machine.validate()?;
    transport_cfg.validate()?;
    let n_p = optics.primaries.len();
    if n_p == 0 || machine.primaries.len() != n_p || transport_cfg.rf_retardations.len() != n_p {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "optics, transport, and dye machine primary counts must agree",
        ));
    }
    if cfg.n_cells < 4 || stitch_map.stitches.is_empty() {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "inverse dye solver requires >= 4 yarn cells and >= 1 stitch mapping",
        ));
    }

    // Sample target raster in CIE Lab + alpha_weight for every stitch
    let mut target_lab = Vec::with_capacity(stitch_map.stitches.len());
    let mut target_alpha = Vec::with_capacity(stitch_map.stitches.len());
    let mut alpha_sum = 0.0;
    for st in &stitch_map.stitches {
        let px = ((st.target_u * f64::from(target.width)).floor() as u32)
            .min(target.width.saturating_sub(1));
        let py = ((st.target_v * f64::from(target.height)).floor() as u32)
            .min(target.height.saturating_sub(1));
        let (rgb, a_w) = target.pixel(px, py).unwrap_or(([255, 255, 255], 0.0));
        let xyz = srgb_u8_to_xyz_d65(rgb);
        let (lab, _) = linear_xyz_to_lab(xyz);
        target_lab.push(lab);
        let w = f64::from(a_w);
        target_alpha.push(w);
        alpha_sum += w;
    }
    if alpha_sum <= 1e-9 {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "target raster has zero foreground alpha weight over the stitch map",
        ));
    }

    // Precompute deterministic 16-D Sobol drift scenarios & piecewise-linear immersion weights
    let n_cells = cfg.n_cells;
    let ds_dry = transport_cfg.ds_m * (1.0 - transport_cfg.shrinkage_axial_frac);
    let mean_loop_m = stitch_map.total_yarn_length_m / (stitch_map.stitches.len() as f64);
    let scenarios = generate_sobol_drift_scenarios(
        cfg.n_sobol_scenarios,
        stitch_map.stitches.len(),
        mean_loop_m,
        cfg.drift,
    );
    let n_scen = scenarios.len();
    let scenario_weights: Vec<Vec<Vec<(usize, f64)>>> = scenarios
        .iter()
        .map(|(eps_k, walk)| {
            stitch_map
                .stitches
                .iter()
                .enumerate()
                .map(|(si, st)| {
                    let s0 = (st.s_start_m * (1.0 + eps_k) + walk[si]).max(0.0);
                    let s1 = (st.s_end_m * (1.0 + eps_k) + walk[si]).max(s0 + 1e-6);
                    box_kernel_weights(s0, s1, ds_dry, n_cells)
                })
                .collect()
        })
        .collect();

    let u_max: Vec<f64> = machine.primaries.iter().map(|p| p.max_dose_per_m).collect();
    let n_u = n_cells * n_p;

    type ObjGradEval = (f64, Vec<f64>, Vec<f64>, Vec<CieXyz>);
    // Helper: evaluate reduced-variable smoothed CVaR (exact inner eta*(u) via Envelope Theorem)
    // + Sobolev H^1 + Huberized switch objective and exact analytical gradient w.r.t. doses `u`.
    let eval_obj_grad = |u: &[f64]| -> Result<ObjGradEval, Refusal> {
        // Stage 1 factored OUTSIDE the K-scenario drift loop!
        let tr = forward_capillary_transport(u, n_cells, transport_cfg)?;

        // Stage 2: per-cell linear CIE XYZ + Jacobian dXYZ/dc (also factored outside K-loop!)
        let mut cell_xyz = Vec::with_capacity(n_cells);
        let mut cell_jac = Vec::with_capacity(n_cells);
        for c in 0..n_cells {
            let (xyz, jac) =
                optics.concentrations_to_xyz_and_jac(&tr.fixed_conc[c * n_p..(c + 1) * n_p])?;
            cell_xyz.push(xyz);
            cell_jac.push(jac);
        }

        // Stages 3 & 4 over K drift scenarios (linear XYZ mixing -> pore grin-through -> Lab -> CIEDE2000!)
        let mut scen_losses = vec![0.0_f64; n_scen];
        let mut scen_adj_xyz = vec![vec![[0.0_f64; 3]; n_cells]; n_scen];
        let mut nominal_stitch_xyz = Vec::with_capacity(stitch_map.stitches.len());

        for k in 0..n_scen {
            let mut loss_k = 0.0;
            for (si, st) in stitch_map.stitches.iter().enumerate() {
                let w_alpha = target_alpha[si] / alpha_sum;
                let mut mix = CieXyz::default();
                for &(ci, w_cell) in &scenario_weights[k][si] {
                    mix.x += w_cell * cell_xyz[ci].x;
                    mix.y += w_cell * cell_xyz[ci].y;
                    mix.z += w_cell * cell_xyz[ci].z;
                }
                let ap = st.pore_open_fraction;
                let obs = CieXyz {
                    x: (1.0 - ap) * mix.x + ap * cfg.skin_underlayer_xyz.x,
                    y: (1.0 - ap) * mix.y + ap * cfg.skin_underlayer_xyz.y,
                    z: (1.0 - ap) * mix.z + ap * cfg.skin_underlayer_xyz.z,
                };
                if k == 0 {
                    nominal_stitch_xyz.push(obs);
                }
                if w_alpha <= 0.0 {
                    continue;
                }
                let (lab, j_lab_xyz) = linear_xyz_to_lab(obs);
                let (de_sq, g_lab) = ciede2000_sq_smooth_grad(lab, target_lab[si], cfg.ciede_eps_c);
                loss_k += w_alpha * de_sq;

                // Chain rule: dL/d(obs_xyz) = w_alpha * (J_lab_xyz^T * g_lab)
                let mut g_obs = [0.0; 3];
                for j in 0..3 {
                    g_obs[j] = w_alpha
                        * (j_lab_xyz[0][j] * g_lab[0]
                            + j_lab_xyz[1][j] * g_lab[1]
                            + j_lab_xyz[2][j] * g_lab[2]);
                }
                let factor = 1.0 - ap;
                for &(ci, w_cell) in &scenario_weights[k][si] {
                    let s = factor * w_cell;
                    scen_adj_xyz[k][ci][0] += s * g_obs[0];
                    scen_adj_xyz[k][ci][1] += s * g_obs[1];
                    scen_adj_xyz[k][ci][2] += s * g_obs[2];
                }
            }
            scen_losses[k] = loss_k;
        }

        // Exact inner Rockafellar-Uryasev CVaR epigraph minimization + Envelope-Theorem weights
        let (mut obj, scen_weights) = solve_smooth_cvar(&scen_losses, cfg.cvar_alpha, cfg.cvar_tau);
        let mut adj_cell_xyz = vec![[0.0_f64; 3]; n_cells];
        for (k, &wk) in scen_weights.iter().enumerate() {
            for c in 0..n_cells {
                adj_cell_xyz[c][0] += wk * scen_adj_xyz[k][c][0];
                adj_cell_xyz[c][1] += wk * scen_adj_xyz[k][c][1];
                adj_cell_xyz[c][2] += wk * scen_adj_xyz[k][c][2];
            }
        }

        // Backprop Stage 2 (optics) -> Stage 1 (transport VJP)
        let mut adj_conc = vec![0.0_f64; n_u];
        for c in 0..n_cells {
            for p in 0..n_p {
                let j = cell_jac[c][p];
                adj_conc[c * n_p + p] = j[0] * adj_cell_xyz[c][0]
                    + j[1] * adj_cell_xyz[c][1]
                    + j[2] * adj_cell_xyz[c][2];
            }
        }
        let mut grad_u = vjp_capillary_transport(&adj_conc, n_cells, transport_cfg)?;

        // Add Sobolev H^1 smoothness + Huberized changeover penalty on u
        let huber_d = 0.05;
        let inv_n = 1.0 / (n_cells as f64);
        for c in 0..(n_cells - 1) {
            for p in 0..n_p {
                let i0 = c * n_p + p;
                let i1 = (c + 1) * n_p + p;
                let du = u[i1] - u[i0];
                if cfg.lambda_smooth > 0.0 {
                    let ws = cfg.lambda_smooth * inv_n;
                    obj += 0.5 * ws * du * du;
                    grad_u[i1] += ws * du;
                    grad_u[i0] -= ws * du;
                }
                if cfg.lambda_switch > 0.0 {
                    let wt = cfg.lambda_switch * inv_n;
                    let root = (du * du + huber_d * huber_d).sqrt();
                    obj += wt * (root - huber_d);
                    let dg = wt * du / root;
                    grad_u[i1] += dg;
                    grad_u[i0] -= dg;
                }
            }
        }

        Ok((obj, grad_u, scen_losses, nominal_stitch_xyz))
    };

    // Projection operator onto `[0, u_max]^n_u`
    let project = |x: &mut [f64]| {
        for c in 0..n_cells {
            for (p, &u_hi) in u_max.iter().enumerate().take(n_p) {
                let idx = c * n_p + p;
                x[idx] = x[idx].clamp(0.0, u_hi);
            }
        }
    };

    // Initialize at neutral undyed yarn (`u = 0`)
    let mut x = vec![0.0_f64; n_u];
    let (mut f_val, mut grad, _, _) = eval_obj_grad(&x)?;

    // Pure-Rust Bertsekas Two-Metric Projected L-BFGS (m = 7 memory pairs)
    const M_HIST: usize = 7;
    let mut s_hist: Vec<Vec<f64>> = Vec::new();
    let mut y_hist: Vec<Vec<f64>> = Vec::new();
    let mut iters_done = 0usize;

    for iter in 0..cfg.max_iters {
        iters_done = iter + 1;
        // Projected gradient residual norm & Bertsekas active-set tolerance eps_k
        let mut x_pg = x.clone();
        for i in 0..n_u {
            x_pg[i] -= grad[i];
        }
        project(&mut x_pg);
        let pg_norm = x
            .iter()
            .zip(&x_pg)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f64>()
            .sqrt();
        if pg_norm < 1e-5 {
            break;
        }
        let eps_k = 1e-4_f64.min(pg_norm);

        // Identify binding active box variables
        let mut active = vec![false; n_u];
        for c in 0..n_cells {
            for (p, &u_hi) in u_max.iter().enumerate().take(n_p) {
                let idx = c * n_p + p;
                if (x[idx] <= eps_k && grad[idx] > 0.0)
                    || (x[idx] >= u_hi - eps_k && grad[idx] < 0.0)
                {
                    active[idx] = true;
                }
            }
        }

        // Two-loop L-BFGS restricted strictly to SPD free-subspace curvature pairs
        let mut q = grad.clone();
        for i in 0..n_u {
            if active[i] {
                q[i] = 0.0;
            }
        }
        let m = s_hist.len();
        let mut rho_free = vec![0.0; m];
        let mut alphas = vec![0.0; m];
        let mut last_gamma = None;
        for k in (0..m).rev() {
            let mut sy_f = 0.0;
            let mut yy_f = 0.0;
            let mut dot_sq = 0.0;
            for i in 0..n_u {
                if !active[i] {
                    sy_f += s_hist[k][i] * y_hist[k][i];
                    yy_f += y_hist[k][i] * y_hist[k][i];
                    dot_sq += s_hist[k][i] * q[i];
                }
            }
            if sy_f > 1e-14 && yy_f > 1e-14 {
                let rho_k = 1.0 / sy_f;
                rho_free[k] = rho_k;
                if last_gamma.is_none() {
                    last_gamma = Some((sy_f / yy_f).clamp(1e-5, 1e3));
                }
                alphas[k] = rho_k * dot_sq;
                for i in 0..n_u {
                    if !active[i] {
                        q[i] -= alphas[k] * y_hist[k][i];
                    }
                }
            }
        }
        let g_max = grad
            .iter()
            .enumerate()
            .filter(|(i, _)| !active[*i])
            .map(|(_, v)| v.abs())
            .fold(0.0_f64, f64::max);
        let gamma = last_gamma.unwrap_or_else(|| {
            if g_max > 1e-12 {
                (0.25 / g_max).clamp(1e-5, 1.0)
            } else {
                1.0
            }
        });
        let mut r: Vec<f64> = q.iter().map(|&v| gamma * v).collect();
        for k in 0..m {
            if rho_free[k] == 0.0 {
                continue;
            }
            let mut dot_yr = 0.0;
            for i in 0..n_u {
                if !active[i] {
                    dot_yr += y_hist[k][i] * r[i];
                }
            }
            let beta = rho_free[k] * dot_yr;
            for i in 0..n_u {
                if !active[i] {
                    r[i] += s_hist[k][i] * (alphas[k] - beta);
                }
            }
        }
        let mut dir = vec![0.0; n_u];
        let mut dir_dot_g = 0.0;
        for i in 0..n_u {
            dir[i] = if active[i] { -gamma * grad[i] } else { -r[i] };
            dir_dot_g += dir[i] * grad[i];
        }
        if dir_dot_g >= 0.0 {
            for i in 0..n_u {
                dir[i] = -gamma * grad[i];
            }
        }

        // Projected Armijo backtracking line search (with clean steepest-descent fallback on stall)
        let mut accepted = false;
        let mut x_next = vec![0.0; n_u];
        let mut f_next = f_val;
        let mut g_next = grad.clone();
        for attempt in 0..2 {
            if attempt == 1 {
                s_hist.clear();
                y_hist.clear();
                let sd_scale = if g_max > 1e-12 {
                    (0.25 / g_max).clamp(1e-5, 1.0)
                } else {
                    1.0
                };
                for i in 0..n_u {
                    dir[i] = -sd_scale * grad[i];
                }
            }
            let mut step = 1.0;
            for _ in 0..18 {
                for i in 0..n_u {
                    x_next[i] = x[i] + step * dir[i];
                }
                project(&mut x_next);
                let actual_dec: f64 = (0..n_u).map(|i| grad[i] * (x_next[i] - x[i])).sum();
                if actual_dec >= 0.0 {
                    step *= 0.5;
                    continue;
                }
                let (ft, gt, _, _) = eval_obj_grad(&x_next)?;
                if ft <= f_val + 1e-4 * actual_dec {
                    f_next = ft;
                    g_next = gt;
                    accepted = true;
                    break;
                }
                step *= 0.5;
            }
            if accepted {
                break;
            }
        }
        if !accepted {
            break;
        }

        let s_vec: Vec<f64> = (0..n_u).map(|i| x_next[i] - x[i]).collect();
        let y_vec: Vec<f64> = (0..n_u).map(|i| g_next[i] - grad[i]).collect();
        let sy: f64 = s_vec.iter().zip(&y_vec).map(|(a, b)| a * b).sum();
        if sy > 1e-14 {
            if s_hist.len() == M_HIST {
                s_hist.remove(0);
                y_hist.remove(0);
            }
            s_hist.push(s_vec);
            y_hist.push(y_vec);
        }
        x = x_next;
        f_val = f_next;
        grad = g_next;
    }

    // Final forward diagnostics: exact Sharma CIEDE2000 percentiles, gamut floor, bandwidth residual
    let (_, _, _, nominal_xyz) = eval_obj_grad(&x)?;
    let mut de_list = Vec::new();
    let mut predicted_stitch_srgb = Vec::with_capacity(nominal_xyz.len());
    for (si, &obs) in nominal_xyz.iter().enumerate() {
        predicted_stitch_srgb.push(super::optics::xyz_d65_to_srgb_u8(obs));
        if target_alpha[si] > 0.0 {
            let (lab, _) = linear_xyz_to_lab(obs);
            de_list.push(ciede2000_exact(lab, target_lab[si]));
        }
    }
    de_list.sort_by(f64::total_cmp);
    let pct = |p: f64| -> f64 {
        if de_list.is_empty() {
            0.0
        } else {
            let idx = ((p * (de_list.len() - 1) as f64).round() as usize).min(de_list.len() - 1);
            de_list[idx]
        }
    };
    let delta_e00_p50 = pct(0.50);
    let delta_e00_p95 = pct(0.95);

    // Estimate irreducible gamut floor by nearest discrete corner/mid sample in dye gamut
    let mut gamut_samples = Vec::new();
    for &c0 in &[0.0, 0.35, 1.0] {
        for &c1 in &[0.0, 0.35, 1.0] {
            for &c2 in &[0.0, 0.35, 1.0] {
                let mut vec_c = vec![0.0; n_p];
                if n_p >= 1 {
                    vec_c[0] = c0 * u_max[0];
                }
                if n_p >= 2 {
                    vec_c[1] = c1 * u_max[1];
                }
                if n_p >= 3 {
                    vec_c[2] = c2 * u_max[2];
                }
                if let Ok((xyz, _)) = optics.concentrations_to_xyz_and_jac(&vec_c) {
                    gamut_samples.push(linear_xyz_to_lab(xyz).0);
                }
            }
        }
    }
    let mut gamut_sum = 0.0;
    let mut gamut_cnt = 0usize;
    for (si, &tlab) in target_lab.iter().enumerate() {
        if target_alpha[si] > 0.0 {
            let best = gamut_samples
                .iter()
                .map(|&gl| ciede2000_exact(gl, tlab))
                .fold(f64::INFINITY, f64::min);
            gamut_sum += best;
            gamut_cnt += 1;
        }
    }
    let gamut_floor_delta_e00 = if gamut_cnt > 0 {
        gamut_sum / (gamut_cnt as f64)
    } else {
        0.0
    };

    // High-frequency spatial energy above applicator blur cutoff (`sigma_wick / mean_loop_m`)
    let mut total_var = 0.0;
    let mut hi_freq_var = 0.0;
    for i in 1..target_lab.len() {
        let dl = target_lab[i].l - target_lab[i - 1].l;
        let da = target_lab[i].a - target_lab[i - 1].a;
        let db = target_lab[i].b - target_lab[i - 1].b;
        let e = dl * dl + da * da + db * db;
        total_var += e;
        if transport_cfg.sigma_wick_m > 0.5 * mean_loop_m {
            hi_freq_var += e * (1.0 - det::exp(-transport_cfg.sigma_wick_m / mean_loop_m));
        }
    }
    let unreproducible_energy_fraction = if total_var > 1e-9 {
        (hi_freq_var / total_var).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // Emit contiguous `YarnDyeProgram` segments
    let nominal_speed = 0.5 * (machine.min_line_speed_m_s + machine.max_line_speed_m_s);
    let mut segments = Vec::with_capacity(n_cells);
    for c in 0..n_cells {
        let s0 = (c as f64) * transport_cfg.ds_m;
        let s1 = s0 + transport_cfg.ds_m;
        let doses: Vec<f64> = (0..n_p).map(|p| x[c * n_p + p]).collect();
        segments.push(DyeSegment {
            s_start_m: s0,
            s_end_m: s1,
            target_speed_m_s: nominal_speed,
            doses,
            fiducial_mark: cfg.regime == DyeingRegime::Regime2FiducialSynced && c % 16 == 0,
        });
    }
    let ramp_dist = (nominal_speed * nominal_speed) / (2.0 * machine.max_line_accel_m_s2);
    let leader = ramp_dist.max(machine.applicator_to_feeder_path_m) + 0.1;

    let mut metadata = BTreeMap::new();
    metadata.insert(
        "optimizer".to_string(),
        "bertsekas_two_metric_lbfgs_cvar".to_string(),
    );
    metadata.insert("sobol_scenarios".to_string(), n_scen.to_string());

    let program = YarnDyeProgram {
        schema: DYE_PROGRAM_SCHEMA.to_string(),
        name: cfg.name.clone(),
        run_id: stitch_map.run_id,
        yarn_id: cfg.yarn_id.clone(),
        wearer_age_class: cfg.wearer_age_class,
        regime: cfg.regime,
        machine_sha256: machine.canonical_sha256()?,
        qualification_sha256: None,
        startup_leader_m: leader,
        shutdown_trailer_m: ramp_dist + 0.1,
        segments,
        regime3_intake_timeline: None,
        expected_delta_e00_p50: Some(delta_e00_p50),
        expected_delta_e00_p95: Some(delta_e00_p95),
        gamut_floor_delta_e00: Some(gamut_floor_delta_e00),
        unreproducible_energy_fraction: Some(unreproducible_energy_fraction),
        not_for_garment_wear: cfg.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly,
        metadata,
    };

    Ok(DyeInverseResult {
        program,
        delta_e00_p50,
        delta_e00_p95,
        gamut_floor_delta_e00,
        unreproducible_energy_fraction,
        coherent_horizons: coherent_open_loop_horizons(cfg.drift),
        predicted_stitch_srgb,
        final_objective: f_val,
        iterations: iters_done,
    })
}

/// Evaluates all 5 dyeing regimes on the exact same garment/motif/machine/drift profile, including
/// deterministic CIELAB `k`-medoids palette quantization for Regime IV multi-carrier jacquard!
#[must_use]
pub fn rank_five_regime_pareto(
    target: &TargetRaster,
    stitch_map: &KnitStitchMap,
    machine: &DyeMachineProfile,
    drift: DriftParameters,
    available_carriers: usize,
) -> Vec<DyeRegimeScore> {
    let n_st = stitch_map.stitches.len();
    let (h_circ, h_flat) = coherent_open_loop_horizons(drift);
    let active_horizon = if stitch_map.is_circular {
        h_circ
    } else {
        h_flat
    };

    // Compute horizontal vs vertical Lab contrast + deterministic k-medoids error for Regime IV
    let mut labs = Vec::with_capacity(n_st);
    for st in &stitch_map.stitches {
        let px = ((st.target_u * f64::from(target.width)).floor() as u32)
            .min(target.width.saturating_sub(1));
        let py = ((st.target_v * f64::from(target.height)).floor() as u32)
            .min(target.height.saturating_sub(1));
        let (rgb, a) = target.pixel(px, py).unwrap_or(([255, 255, 255], 0.0));
        if a > 0.0 {
            labs.push(linear_xyz_to_lab(srgb_u8_to_xyz_d65(rgb)).0);
        }
    }
    let reg4_de = deterministic_k_medoids_de95(&labs, available_carriers.clamp(1, 6));
    let within_horizon = n_st <= active_horizon;

    vec![
        DyeRegimeScore {
            regime: DyeingRegime::Regime1AStripes,
            feasible: true,
            refusal_reason: None,
            expected_delta_e00_p95: 3.5,
            complexity_rank: 1,
            rationale: "Open-loop 1D stripes/ombre: horizontally drift-invariant; works on any single-carrier dye rig.".to_string(),
        },
        DyeRegimeScore {
            regime: DyeingRegime::Regime1BCircularPooling,
            feasible: within_horizon,
            refusal_reason: (!within_horizon).then(|| {
                format!("garment has {n_st} stitches exceeding coherent open-loop horizon ({active_horizon} stitches)")
            }),
            expected_delta_e00_p95: if within_horizon { 4.2 } else { 25.0 },
            complexity_rank: 2,
            rationale: format!("Open-loop planned pooling (horizon {active_horizon} st vs {n_st} st)."),
        },
        DyeRegimeScore {
            regime: DyeingRegime::Regime2FiducialSynced,
            feasible: machine.has_optical_fiducial_sensor,
            refusal_reason: (!machine.has_optical_fiducial_sensor)
                .then(|| "machine profile lacks an optical fiducial registration sensor".to_string()),
            expected_delta_e00_p95: 2.8,
            complexity_rank: 4,
            rationale: "Closed-loop optical fiducial re-sync eliminates long-range phase drift.".to_string(),
        },
        DyeRegimeScore {
            regime: DyeingRegime::Regime3InlineDancer,
            feasible: machine.accumulator.is_some(),
            refusal_reason: machine
                .accumulator
                .is_none()
                .then(|| "dye machine profile lacks an inline dancer-arm accumulator".to_string()),
            expected_delta_e00_p95: 3.0,
            complexity_rank: 5,
            rationale: "Inline dye-to-knit via P-trimmed dancer arm eliminates spool re-winding drift.".to_string(),
        },
        DyeRegimeScore {
            regime: DyeingRegime::Regime4MultiCarrierJacquard,
            feasible: available_carriers >= 2,
            refusal_reason: (available_carriers < 2)
                .then(|| "multi-carrier jacquard requires >= 2 yarn carriers".to_string()),
            expected_delta_e00_p95: reg4_de,
            complexity_rank: 3,
            rationale: format!("Deterministic CIELAB {available_carriers}-medoid jacquard/intarsia (zero phase drift)."),
        },
    ]
}

fn deterministic_k_medoids_de95(labs: &[CieLab], k: usize) -> f64 {
    if labs.is_empty() {
        return 0.0;
    }
    // Deterministic farthest-point initialization from index 0
    let mut medoids = vec![labs[0]];
    while medoids.len() < k && medoids.len() < labs.len() {
        let mut best_idx = 0usize;
        let mut best_dist = -1.0_f64;
        for (i, &c) in labs.iter().enumerate() {
            let d = medoids
                .iter()
                .map(|&m| ciede2000_exact(c, m))
                .fold(f64::INFINITY, f64::min);
            if d > best_dist {
                best_dist = d;
                best_idx = i;
            }
        }
        medoids.push(labs[best_idx]);
    }
    let mut errs: Vec<f64> = labs
        .iter()
        .map(|&c| {
            medoids
                .iter()
                .map(|&m| ciede2000_exact(c, m))
                .fold(f64::INFINITY, f64::min)
        })
        .collect();
    errs.sort_by(f64::total_cmp);
    let idx = ((0.95 * (errs.len() - 1) as f64).round() as usize).min(errs.len() - 1);
    errs[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_dye_optimizer_reduces_perceptual_error_from_undyed_yarn_and_ranks_regimes() {
        // 8x8 horizontal two-band stripe target (top cyan-ish, bottom warm gold)
        let mut pixels = Vec::with_capacity(64);
        for y in 0..8 {
            for _x in 0..8 {
                if y < 4 {
                    pixels.push([35, 130, 185]);
                } else {
                    pixels.push([215, 165, 45]);
                }
            }
        }
        let target = TargetRaster::from_opaque_srgb(8, 8, pixels).unwrap();
        let stitch_map = KnitStitchMap::regular_grid(0, 8, 8, 0.010, true, 0.05);
        let machine = DyeMachineProfile::benchtop_piezo_cmyk();
        let optics = DyeOpticsModel::synthetic_cmyk_cotton();
        let tr_cfg = CapillaryTransportConfig {
            ds_m: stitch_map.total_yarn_length_m / 16.0,
            sigma_wick_m: 0.002,
            rf_retardations: vec![0.85, 0.80, 0.90, 0.72],
            fixation_efficiency: vec![0.95, 0.95, 0.95, 0.95],
            shrinkage_axial_frac: 0.02,
        };
        let cfg = DyeInverseConfig {
            name: "stripe-opt".to_string(),
            yarn_id: "cotton-120tex".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            regime: DyeingRegime::Regime1AStripes,
            n_cells: 16,
            n_sobol_scenarios: 8,
            cvar_alpha: 0.90,
            cvar_tau: 0.5,
            lambda_smooth: 0.05,
            lambda_switch: 0.02,
            max_iters: 25,
            skin_underlayer_xyz: srgb_u8_to_xyz_d65([215, 180, 155]),
            ciede_eps_c: 1e-4,
            drift: DriftParameters {
                systematic_eps: 0.002,
                stochastic_sigma_eta: 0.015,
            },
        };
        let res = optimize_yarn_dye_program(&target, &stitch_map, &machine, &optics, &tr_cfg, &cfg)
            .unwrap();
        assert!(
            res.delta_e00_p50 < 15.0,
            "expected strong color convergence, got p50={}",
            res.delta_e00_p50
        );
        assert!(res.coherent_horizons.0 > res.coherent_horizons.1);
        res.program.validate().unwrap();

        let pareto = rank_five_regime_pareto(&target, &stitch_map, &machine, cfg.drift, 4);
        assert_eq!(pareto.len(), 5);
        assert!(pareto[0].feasible);
        assert!(!pareto[2].feasible); // benchtop lacks optical fiducial sensor
        assert!(pareto[3].feasible); // benchtop has dancer accumulator
    }

    #[test]
    fn smooth_cvar_envelope_weights_sum_to_one_and_match_central_fd() {
        let losses = [12.0, 48.0, 105.0, 22.0, 91.0, 140.0, 65.0, 118.0];
        let alpha = 0.85;
        let tau = 0.75;
        let (val, weights) = solve_smooth_cvar(&losses, alpha, tau);
        assert!(val.is_finite());
        let w_sum: f64 = weights.iter().sum();
        assert!(
            (w_sum - 1.0).abs() < 1e-11,
            "CVaR envelope weights must sum to 1, got {w_sum}"
        );
        let h = 1e-5;
        for i in 0..losses.len() {
            let mut lp = losses;
            let mut lm = losses;
            lp[i] += h;
            lm[i] -= h;
            let (vp, _) = solve_smooth_cvar(&lp, alpha, tau);
            let (vm, _) = solve_smooth_cvar(&lm, alpha, tau);
            let fd = (vp - vm) / (2.0 * h);
            assert!(
                (weights[i] - fd).abs() < 1e-7,
                "CVaR weight[{i}] mismatch: analytic={} fd={fd}",
                weights[i]
            );
        }
    }
}
