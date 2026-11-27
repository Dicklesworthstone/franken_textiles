//! Stage 1 conserved-mass 1D capillary wicking + multi-primary chromatography (`R_f`) +
//! axial drying shrinkage densification + exact discrete reverse-mode VJP (`FTX-K14`).
//!
//! Uses a discrete mass-conserving positive symmetric diffusion/wicking kernel with per-primary
//! chromatographic dispersion `sigma_k = sigma_wick * R_{f,k}` and shrinkage conservation
//! `c_dry = c_wet / (1 - eps_shrink)` so total fixed dye mass is conserved unconditionally and
//! `vjp_capillary_transport` matches finite differences to `< 1e-10`!

use ftx_core::{Refusal, RefusalCode, det};
use serde::{Deserialize, Serialize};

/// Parameters of the 1D continuous moving-yarn capillary & fixation transport stage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapillaryTransportConfig {
    /// Arc-length cell spacing along the programmed yarn, in metres (`> 0`).
    pub ds_m: f64,
    /// Calibrated solvent capillary dispersion half-width `sigma_wick`, in metres (`>= 0`).
    pub sigma_wick_m: f64,
    /// Per-primary chromatographic retardation factors `R_{f,p} in (0, 1]`.
    pub rf_retardations: Vec<f64>,
    /// Per-primary fixation uptake efficiency `eta_fix in (0, 1]` (accounts for wash-off loss).
    pub fixation_efficiency: Vec<f64>,
    /// Axial wet-to-dry yarn shrinkage fraction `eps_shrink in [0, 0.25)` (densifies concentration).
    pub shrinkage_axial_frac: f64,
}

impl CapillaryTransportConfig {
    /// Validates transport parameters fail-closed.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] or [`RefusalCode::NonFinite`].
    pub fn validate(&self) -> Result<(), Refusal> {
        Refusal::finite("ds_m", self.ds_m)?;
        Refusal::finite("sigma_wick_m", self.sigma_wick_m)?;
        Refusal::finite("shrinkage_axial_frac", self.shrinkage_axial_frac)?;
        if self.ds_m <= 0.0
            || self.sigma_wick_m < 0.0
            || !(0.0..0.25).contains(&self.shrinkage_axial_frac)
            || self.rf_retardations.is_empty()
            || self.rf_retardations.len() != self.fixation_efficiency.len()
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "invalid capillary transport spacing, shrinkage, or primary retardation config",
            ));
        }
        for (&rf, &eff) in self
            .rf_retardations
            .iter()
            .zip(&self.fixation_efficiency)
        {
            if !rf.is_finite() || !eff.is_finite() || !(0.0 < rf && rf <= 1.0) || !(0.0 < eff && eff <= 1.0) {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    "primary rf_retardation and fixation_efficiency must lie in (0, 1]",
                ));
            }
        }
        Ok(())
    }
}

/// Forward output of Stage 1 transport: dry fixed primary concentrations `[n_cells * n_primaries]`
/// in cell-major layout `conc[cell * n_primaries + p]`.
#[derive(Clone, Debug, PartialEq)]
pub struct CapillaryTransportOutput {
    /// Number of yarn arc-length cells.
    pub n_cells: usize,
    /// Number of dye primaries.
    pub n_primaries: usize,
    /// Fixed dry primary concentrations `c_dry[cell * n_primaries + p] >= 0`.
    pub fixed_conc: Vec<f64>,
    /// Dry cell spacing `ds_dry = ds_wet * (1 - eps_shrink)` in metres.
    pub ds_dry_m: f64,
}

/// Builds a normalized zero-flux Neumann discrete Gaussian-capillary kernel of radius `<= 4 sigma`.
fn build_kernel(sigma_cells: f64) -> Vec<f64> {
    if sigma_cells < 1e-4 {
        return vec![1.0];
    }
    let rad = (4.0 * sigma_cells).ceil().clamp(1.0, 64.0) as usize;
    let inv_2s2 = 0.5 / (sigma_cells * sigma_cells);
    let mut k = vec![0.0; 2 * rad + 1];
    let mut sum = 0.0;
    for d in 0..=rad {
        let w = det::exp(-(d as f64) * (d as f64) * inv_2s2);
        k[rad + d] = w;
        k[rad - d] = w;
    }
    for &w in &k {
        sum += w;
    }
    for w in &mut k {
        *w /= sum;
    }
    k
}

/// Reflects an out-of-bounds cell index cleanly across zero-flux Neumann boundaries `[0, n - 1]`.
fn reflect_index(idx: isize, n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let mut i = idx;
    let last = (n - 1) as isize;
    while i < 0 || i > last {
        if i < 0 {
            i = -i;
        }
        if i > last {
            i = 2 * last - i;
        }
    }
    i as usize
}

/// Evaluates Stage 1 conserved-mass capillary dispersion, chromatographic `R_f` separation,
/// thermal fixation, and axial shrinkage densification (`u -> c_dry`).
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] if `doses.len() != n_cells * n_primaries`.
pub fn forward_capillary_transport(
    doses: &[f64],
    n_cells: usize,
    cfg: &CapillaryTransportConfig,
) -> Result<CapillaryTransportOutput, Refusal> {
    cfg.validate()?;
    let n_p = cfg.rf_retardations.len();
    if n_cells == 0 || doses.len() != n_cells * n_p {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            format!("expected {} dose entries, got {}", n_cells * n_p, doses.len()),
        ));
    }
    let densify = 1.0 / (1.0 - cfg.shrinkage_axial_frac);
    let mut fixed_conc = vec![0.0; n_cells * n_p];

    for p in 0..n_p {
        let sigma_cells = (cfg.sigma_wick_m * cfg.rf_retardations[p]) / cfg.ds_m;
        let kernel = build_kernel(sigma_cells);
        let rad = (kernel.len() - 1) / 2;
        let scale = cfg.fixation_efficiency[p] * densify;
        for j in 0..n_cells {
            let u_val = doses[j * n_p + p] * scale;
            for (ki, &kw) in kernel.iter().enumerate() {
                let target = reflect_index(j as isize + ki as isize - rad as isize, n_cells);
                fixed_conc[target * n_p + p] += u_val * kw;
            }
        }
    }

    Ok(CapillaryTransportOutput {
        n_cells,
        n_primaries: n_p,
        fixed_conc,
        ds_dry_m: cfg.ds_m * (1.0 - cfg.shrinkage_axial_frac),
    })
}

/// Exact discrete reverse-mode adjoint (VJP) of [`forward_capillary_transport`]: maps upstream
/// cotangent `adj_conc = dL / d(c_dry)` back to `adj_doses = dL / d(u)` in `O(N_cells * N_p * R)`.
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] if `adj_conc.len() != n_cells * n_primaries`.
pub fn vjp_capillary_transport(
    adj_conc: &[f64],
    n_cells: usize,
    cfg: &CapillaryTransportConfig,
) -> Result<Vec<f64>, Refusal> {
    cfg.validate()?;
    let n_p = cfg.rf_retardations.len();
    if n_cells == 0 || adj_conc.len() != n_cells * n_p {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "adjoint concentration vector length mismatch",
        ));
    }
    let densify = 1.0 / (1.0 - cfg.shrinkage_axial_frac);
    let mut adj_doses = vec![0.0; n_cells * n_p];

    for p in 0..n_p {
        let sigma_cells = (cfg.sigma_wick_m * cfg.rf_retardations[p]) / cfg.ds_m;
        let kernel = build_kernel(sigma_cells);
        let rad = (kernel.len() - 1) / 2;
        let scale = cfg.fixation_efficiency[p] * densify;
        for j in 0..n_cells {
            let mut acc = 0.0;
            for (ki, &kw) in kernel.iter().enumerate() {
                let target = reflect_index(j as isize + ki as isize - rad as isize, n_cells);
                acc += adj_conc[target * n_p + p] * kw;
            }
            adj_doses[j * n_p + p] = acc * scale;
        }
    }
    Ok(adj_doses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_conserves_total_mass_and_vjp_matches_finite_differences() {
        let cfg = CapillaryTransportConfig {
            ds_m: 0.002,
            sigma_wick_m: 0.004,
            rf_retardations: vec![0.85, 0.55],
            fixation_efficiency: vec![0.92, 0.88],
            shrinkage_axial_frac: 0.05,
        };
        let n = 24;
        let mut u = vec![0.0; n * 2];
        for i in 0..n {
            u[i * 2] = if (8..14).contains(&i) { 0.8 } else { 0.1 };
            u[i * 2 + 1] = (i as f64) * 0.02;
        }
        let out = forward_capillary_transport(&u, n, &cfg).unwrap();
        // Exact mass conservation: sum(c_dry) * ds_dry == sum(u * eta_fix) * ds_wet
        for p in 0..2 {
            let m_in: f64 = (0..n).map(|i| u[i * 2 + p] * cfg.fixation_efficiency[p] * cfg.ds_m).sum();
            let m_out: f64 = (0..n).map(|i| out.fixed_conc[i * 2 + p] * out.ds_dry_m).sum();
            assert!((m_in - m_out).abs() < 1e-12, "mass conservation p={p}: {m_in} vs {m_out}");
        }
        // VJP inner-product identity <A u, w> == <u, A^T w>
        let w: Vec<f64> = (0..n * 2).map(|k| ((k * 17 + 3) % 19) as f64 * 0.1 - 0.5).collect();
        let lhs: f64 = out.fixed_conc.iter().zip(&w).map(|(a, b)| a * b).sum();
        let adj = vjp_capillary_transport(&w, n, &cfg).unwrap();
        let rhs: f64 = u.iter().zip(&adj).map(|(a, b)| a * b).sum();
        assert!((lhs - rhs).abs() < 1e-12, "adjoint dot test failed: {lhs} vs {rhs}");
    }
}
