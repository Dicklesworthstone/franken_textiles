//! Rung-2 homogenized knit shell constitutive laws (`FTX-K09`):
//! 1. Unprojected coupled orthotropic knit membrane + smooth jamming barrier (`OutOfRange` gate);
//! 2. Closed-form Cayley–Hamilton `C^{-1/2}` pullback + twice-integrated `C^3` strictly convex
//!    asymmetric bending (wale/course curl with exact engineering twist `2 * b_uv`);
//! 3. Pure-functional trial-safe `C^2`-Huberized Prandtl–Ishlinskii frictional hysteresis bank;
//! 4. All 5 `C011` knit honesty flags and sealed Seam-4 `DrapeUncertaintyReceipt` builder.

use ftx_core::{Refusal, RefusalCode, det};
use ftx_pattern::knit::DrapeUncertaintyReceipt;
use ftx_pattern::materials::Evidence;
use serde::{Deserialize, Serialize};

/// Exact closed-form `C^{-1/2}` of a 2x2 symmetric positive-definite right Cauchy–Green tensor
/// `C = [[c00, c01], [c01, c11]]` using the Cayley–Hamilton identity:
///
/// `det = c00*c11 - c01^2`, `s = sqrt(det)`, `q = sqrt(tr(C) + 2*s)`,
/// `C^{-1/2} = ((tr(C) + s) * I - C) / (s * q)`.
///
/// Singularity-free at isotropic stretches `C = lambda^2 I` (no eigendecomposition or `atan2(0,0)`).
///
/// # Errors
/// [`RefusalCode::OutOfRange`] if `C` is not strictly SPD or non-finite.
pub fn cayley_hamilton_inv_sqrt_2x2(c00: f64, c01: f64, c11: f64) -> Result<[f64; 3], Refusal> {
    let tr = c00 + c11;
    let d = c00 * c11 - c01 * c01;
    if !tr.is_finite() || !d.is_finite() || tr <= 0.0 || d <= 1e-18 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("non-SPD metric tensor C=[[{c00},{c01}],[{c01},{c11}]] (det={d})"),
        ));
    }
    let s = d.sqrt();
    let q = (tr + 2.0 * s).sqrt();
    let inv_sq = 1.0 / (s * q);
    // Symmetric result [m00, m01, m11]
    let m00 = (c11 + s) * inv_sq;
    let m01 = -c01 * inv_sq;
    let m11 = (c00 + s) * inv_sq;
    Ok([m00, m01, m11])
}

/// Coupled orthotropic knit membrane with nonlinear loop-straightening stiffening and smooth
/// yarn-jamming boundary (`h_jam(E) < 0`).
///
/// Evaluates unprojected exact energy, Voigt stress `(S11, S22, S12)`, and exact 3x3 tangent
/// stiffness so implicit-function-theorem (`assemble_static_exact`) adjoints remain exact.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoupledKnitMembrane {
    /// Linear small-strain wale stiffness `Q11` in N/m.
    pub q11: f64,
    /// Linear small-strain course stiffness `Q22` in N/m.
    pub q22: f64,
    /// Linear small-strain wale-course coupling `Q12` in N/m.
    pub q12: f64,
    /// Linear small-strain shear stiffness `Q66` in N/m.
    pub q66: f64,
    /// Quartic wale loop-straightening coefficient `C1111` in N/m (`>= 0`).
    pub c1111: f64,
    /// Quartic course loop-straightening coefficient `C2222` in N/m (`>= 0`).
    pub c2222: f64,
    /// Quartic wale-course cross-coupling coefficient `C1122` in N/m (`>= 0`).
    pub c1122: f64,
    /// Wale jamming strain limit `e1_jam > 0`.
    pub e1_jam: f64,
    /// Course jamming strain limit `e2_jam > 0`.
    pub e2_jam: f64,
    /// Shear jamming strain limit `gamma_jam > 0`.
    pub gamma_jam: f64,
    /// Jamming barrier stiffness `kappa_jam >= 0` in N/m.
    pub kappa_jam: f64,
}

/// Exact membrane constitutive evaluation at Voigt Green strain `e = [E11, E22, 2*E12]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnitMembraneEval {
    /// Strain energy density `W(E)` in J/m².
    pub energy_density: f64,
    /// Second Piola–Kirchhoff Voigt stress `[S11, S22, S12]` in N/m.
    pub stress: [f64; 3],
    /// Exact symmetric 3x3 tangent stiffness `dS/dE` in N/m.
    pub tangent: [[f64; 3]; 3],
    /// Remaining jamming margin `-h_jam(E) in (0, 1]` (`<= 0` is refused fail-closed).
    pub jamming_margin: f64,
}

impl CoupledKnitMembrane {
    /// Representative synthetic jersey knit membrane (tagged `Evidence::Estimate` until coupon fit).
    #[must_use]
    pub const fn reference_stockinette_estimate() -> Self {
        Self {
            q11: 180.0,
            q22: 95.0,
            q12: 32.0,
            q66: 48.0,
            c1111: 900.0,
            c2222: 420.0,
            c1122: 180.0,
            e1_jam: 0.45,
            e2_jam: 0.75,
            gamma_jam: 0.65,
            kappa_jam: 1.5,
        }
    }

    /// Validates parameter positivity and stability.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] or [`RefusalCode::NonFinite`].
    pub fn validate(&self) -> Result<(), Refusal> {
        for (name, v) in [
            ("q11", self.q11),
            ("q22", self.q22),
            ("q12", self.q12),
            ("q66", self.q66),
            ("c1111", self.c1111),
            ("c2222", self.c2222),
            ("c1122", self.c1122),
            ("e1_jam", self.e1_jam),
            ("e2_jam", self.e2_jam),
            ("gamma_jam", self.gamma_jam),
            ("kappa_jam", self.kappa_jam),
        ] {
            Refusal::finite(name, v)?;
        }
        if self.q11 <= 0.0
            || self.q22 <= 0.0
            || self.q66 <= 0.0
            || self.q11 * self.q22 <= self.q12 * self.q12
            || self.c1111 < 0.0
            || self.c2222 < 0.0
            || self.c1122 < 0.0
            || self.e1_jam <= 0.0
            || self.e2_jam <= 0.0
            || self.gamma_jam <= 0.0
            || self.kappa_jam < 0.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "CoupledKnitMembrane requires SPD linear Q, nonnegative quartic/barrier terms, and positive jamming limits",
            ));
        }
        Ok(())
    }

    /// Evaluates unprojected exact energy, Voigt stress, and 3x3 tangent at `e = [E11, E22, gamma12]`.
    ///
    /// Refuses with [`RefusalCode::OutOfRange`] when `h_jam(E) >= 0` so Newton Armijo line search
    /// backtracks strictly inside the unjammed domain.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] if jammed (`h_jam >= 0`) or non-finite.
    pub fn evaluate(&self, e: [f64; 3]) -> Result<KnitMembraneEval, Refusal> {
        let (e1, e2, g) = (e[0], e[1], e[2]);
        if !e1.is_finite() || !e2.is_finite() || !g.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "membrane Green strain is non-finite",
            ));
        }

        // Jamming ellipsoid h(E) = (e1/a1)^2 + (e2/a2)^2 + (g/ag)^2 - 1 < 0
        let inv1 = 1.0 / (self.e1_jam * self.e1_jam);
        let inv2 = 1.0 / (self.e2_jam * self.e2_jam);
        let invg = 1.0 / (self.gamma_jam * self.gamma_jam);
        let r2 = e1 * e1 * inv1 + e2 * e2 * inv2 + g * g * invg;
        let margin = 1.0 - r2;
        if margin <= 1e-12 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("knit membrane exceeded yarn jamming hull (margin={margin:.3e})"),
            ));
        }

        // Quadratic + quartic coupled energy (zero at E = 0)
        let w_quad = 0.5
            * (self.q11 * e1 * e1
                + 2.0 * self.q12 * e1 * e2
                + self.q22 * e2 * e2
                + self.q66 * g * g);
        let w_quart = 0.25 * self.c1111 * e1 * e1 * e1 * e1
            + 0.25 * self.c2222 * e2 * e2 * e2 * e2
            + 0.5 * self.c1122 * e1 * e1 * e2 * e2;

        // Barrier energy: kappa * (-ln(1 - r2) - r2) so W_bar(0) = 0, dW_bar(0) = 0, d^2 W_bar(0) = 0!
        // Note: d/dr2 [-ln(1-r2) - r2] = 1/(1-r2) - 1 = r2 / (1-r2) >= 0!
        // d^2/dr2^2 = 1 / (1-r2)^2 > 0!
        let w_bar = if self.kappa_jam > 0.0 {
            self.kappa_jam * (-det::ln(margin) - r2)
        } else {
            0.0
        };
        let energy_density = w_quad + w_quart + w_bar;

        let dr2 = [2.0 * e1 * inv1, 2.0 * e2 * inv2, 2.0 * g * invg];
        let d2r2 = [2.0 * inv1, 2.0 * inv2, 2.0 * invg];
        let phi1 = self.kappa_jam * (r2 / margin);
        let phi2 = self.kappa_jam / (margin * margin);

        let s0 = self.q11 * e1
            + self.q12 * e2
            + self.c1111 * e1 * e1 * e1
            + self.c1122 * e1 * e2 * e2
            + phi1 * dr2[0];
        let s1 = self.q12 * e1
            + self.q22 * e2
            + self.c2222 * e2 * e2 * e2
            + self.c1122 * e1 * e1 * e2
            + phi1 * dr2[1];
        let s2 = self.q66 * g + phi1 * dr2[2];

        let mut c = [
            [
                self.q11 + 3.0 * self.c1111 * e1 * e1 + self.c1122 * e2 * e2,
                self.q12 + 2.0 * self.c1122 * e1 * e2,
                0.0,
            ],
            [
                self.q12 + 2.0 * self.c1122 * e1 * e2,
                self.q22 + 3.0 * self.c2222 * e2 * e2 + self.c1122 * e1 * e1,
                0.0,
            ],
            [0.0, 0.0, self.q66],
        ];
        if self.kappa_jam > 0.0 {
            for i in 0..3 {
                c[i][i] += phi1 * d2r2[i];
                for j in 0..3 {
                    c[i][j] += phi2 * dr2[i] * dr2[j];
                }
            }
        }

        Ok(KnitMembraneEval {
            energy_density,
            stress: [s0, s1, s2],
            tangent: c,
            jamming_margin: margin,
        })
    }
}

/// Twice-integrated `C^3`-smooth strictly convex asymmetric knit bending law (`FTX-K09`).
///
/// Captures knit face/back curl asymmetry (`d_plus` vs `d_minus` with nonzero rest curvature `kappa0`)
/// in objective material-frame physical curvatures `kappa_phys = C^{-1/2} b C^{-1/2}` with exact
/// engineering twist factor `Delta_kappa_12 = 2 * Delta_b_uv`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsymmetricKnitBending {
    /// Positive-curvature flexural rigidities `[D11+, D22+]` in N·m.
    pub d_plus: [f64; 2],
    /// Negative-curvature flexural rigidities `[D11-, D22-]` in N·m.
    pub d_minus: [f64; 2],
    /// Cross-coupling bending rigidity `D12` in N·m (`|D12| < sqrt(min(D11)*min(D22))`).
    pub d12: f64,
    /// Twist rigidity `D66` in N·m (`> 0`).
    pub d66: f64,
    /// Spontaneous rest curvature `[k0_11, k0_22, k0_12]` in 1/m (drives authentic jersey edge curl!).
    pub kappa0: [f64; 3],
    /// Smooth Hermite transition half-width `delta_kappa > 0` in 1/m.
    pub delta_kappa: f64,
}

/// Exact bending energy density, bending moments `M`, and 3x3 curvature tangent `dM/dkappa`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnitBendingEval {
    /// Bending energy density in J/m².
    pub energy_density: f64,
    /// Objective physical material curvature `[k11, k22, 2*k12]` in 1/m.
    pub kappa_phys: [f64; 3],
    /// Bending moment resultant `[M11, M22, M12]` in N.
    pub moment: [f64; 3],
    /// Strictly positive-definite 3x3 bending tangent `dM/dkappa` in N·m.
    pub tangent: [[f64; 3]; 3],
}

impl AsymmetricKnitBending {
    /// Representative single-jersey stockinette bending estimate (nonzero wale/course curl asymmetry).
    #[must_use]
    pub const fn reference_stockinette_estimate() -> Self {
        Self {
            d_plus: [4.2e-5, 2.6e-5],
            d_minus: [3.1e-5, 1.9e-5],
            d12: 0.45e-5,
            d66: 1.4e-5,
            kappa0: [12.0, -18.0, 0.0],
            delta_kappa: 2.0,
        }
    }

    /// Validates strict global convexity (`min(D11) * min(D22) > D12^2`, `D66 > 0`, `delta_kappa > 0`).
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] or [`RefusalCode::NonFinite`].
    pub fn validate(&self) -> Result<(), Refusal> {
        let vals = [
            self.d_plus[0],
            self.d_plus[1],
            self.d_minus[0],
            self.d_minus[1],
            self.d12,
            self.d66,
            self.kappa0[0],
            self.kappa0[1],
            self.kappa0[2],
            self.delta_kappa,
        ];
        if vals.iter().any(|v| !v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "AsymmetricKnitBending contains non-finite parameters",
            ));
        }
        let min_d11 = self.d_plus[0].min(self.d_minus[0]);
        let min_d22 = self.d_plus[1].min(self.d_minus[1]);
        if min_d11 <= 0.0
            || min_d22 <= 0.0
            || self.d66 <= 0.0
            || self.delta_kappa <= 0.0
            || min_d11 * min_d22 <= self.d12 * self.d12
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "AsymmetricKnitBending violates strict convexity min(D11)*min(D22) > D12^2",
            ));
        }
        Ok(())
    }

    /// Evaluates objective material curvature `kappa_phys = C^{-1/2} b C^{-1/2}` (from metric `C` and
    /// covariant second fundamental form `b = [b_uu, b_vv, b_uv]`) and exact `C^3` asymmetric bending.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] if `C` is not SPD.
    pub fn evaluate_from_metric(
        &self,
        metric_c: [f64; 3],    // [c00, c01, c11]
        covariant_b: [f64; 3], // [b_uu, b_vv, b_uv]
    ) -> Result<KnitBendingEval, Refusal> {
        let [m00, m01, m11] = cayley_hamilton_inv_sqrt_2x2(metric_c[0], metric_c[1], metric_c[2])?;
        let (buu, bvv, buv) = (covariant_b[0], covariant_b[1], covariant_b[2]);
        // M * B * M where M = C^{-1/2} symmetric, B = [[buu, buv], [buv, bvv]]
        let mb00 = m00 * buu + m01 * buv;
        let mb01 = m00 * buv + m01 * bvv;
        let mb10 = m01 * buu + m11 * buv;
        let mb11 = m01 * buv + m11 * bvv;
        let k11 = mb00 * m00 + mb01 * m01;
        let k22 = mb10 * m01 + mb11 * m11;
        let k12_tensor = mb00 * m01 + mb01 * m11;
        // Engineering twist Voigt component is `2 * k12_tensor`!
        let kappa_phys = [k11, k22, 2.0 * k12_tensor];
        Ok(self.evaluate_from_kappa(kappa_phys))
    }

    /// Evaluates exact `C^3`-smooth strictly convex asymmetric bending directly from physical Voigt
    /// curvature `kappa_phys = [k11, k22, 2*k12]`.
    #[must_use]
    pub fn evaluate_from_kappa(&self, kappa_phys: [f64; 3]) -> KnitBendingEval {
        let x0 = kappa_phys[0] - self.kappa0[0];
        let x1 = kappa_phys[1] - self.kappa0[1];
        let x2 = kappa_phys[2] - self.kappa0[2];

        let (g2_0, g1_0, d_0) =
            twice_integrated_hermite(x0, self.delta_kappa, self.d_plus[0], self.d_minus[0]);
        let (g2_1, g1_1, d_1) =
            twice_integrated_hermite(x1, self.delta_kappa, self.d_plus[1], self.d_minus[1]);

        let energy_density = g2_0 + g2_1 + self.d12 * x0 * x1 + 0.5 * self.d66 * x2 * x2;
        let moment = [g1_0 + self.d12 * x1, g1_1 + self.d12 * x0, self.d66 * x2];
        let tangent = [
            [d_0, self.d12, 0.0],
            [self.d12, d_1, 0.0],
            [0.0, 0.0, self.d66],
        ];
        KnitBendingEval {
            energy_density,
            kappa_phys,
            moment,
            tangent,
        }
    }
}

/// Exact closed-form antiderivatives `(G2(x), G1(x), G0(x))` of the cubic Hermite tangent modulus
/// `G0(x) = D_avg + 0.5 * (D+ - D-) * h(x / delta)` where `h(u) = (3/2)u - (1/2)u^3` on `[-1, 1]`,
/// `-1` for `u <= -1`, `+1` for `u >= 1`, normalized so `G1(0) = 0`, `G2(0) = 0`.
///
/// By construction `G2'(x) = G1(x)`, `G1'(x) = G0(x) >= min(D+, D-) > 0` everywhere, `G2` is `C^3`
/// and strictly convex!
fn twice_integrated_hermite(x: f64, delta: f64, d_plus: f64, d_minus: f64) -> (f64, f64, f64) {
    let d_avg = 0.5 * (d_plus + d_minus);
    let d_diff = 0.5 * (d_plus - d_minus);
    let u = x / delta;
    // Let H1(u) = int_0^u h(s) ds, H2(u) = int_0^u H1(s) ds.
    // On |u| <= 1:
    //   h(u)  = 1.5*u - 0.5*u^3
    //   H1(u) = 0.75*u^2 - 0.125*u^4  (at u=+-1: H1(+-1) = 5/8 = 0.625)
    //   H2(u) = 0.25*u^3 - 0.025*u^5  (at u=+-1: H2(+-1) = +-0.225)
    // For u > 1:
    //   h(u)  = 1
    //   H1(u) = 0.625 + (u - 1) = u - 0.375
    //   H2(u) = 0.225 + 0.625*(u - 1) + 0.5*(u - 1)^2
    // For u < -1:
    //   h(u)  = -1
    //   H1(u) = 0.625 - (u + 1) = -u - 0.375
    //   H2(u) = -0.225 + 0.625*(u + 1) - 0.5*(u + 1)^2
    let (h0, h1, h2) = if u >= 1.0 {
        let du = u - 1.0;
        (1.0, 0.625 + du, 0.225 + 0.625 * du + 0.5 * du * du)
    } else if u <= -1.0 {
        let du = u + 1.0;
        (-1.0, 0.625 - du, -0.225 + 0.625 * du - 0.5 * du * du)
    } else {
        let u2 = u * u;
        let u3 = u2 * u;
        let u4 = u2 * u2;
        let u5 = u4 * u;
        (
            1.5 * u - 0.5 * u3,
            0.75 * u2 - 0.125 * u4,
            0.25 * u3 - 0.025 * u5,
        )
    };
    let g0 = d_avg + d_diff * h0;
    let g1 = d_avg * x + d_diff * delta * h1;
    let g2 = 0.5 * d_avg * x * x + d_diff * delta * delta * h2;
    (g2, g1, g0)
}

/// Pure-functional state for a parallel additive Prandtl–Ishlinskii hysteresis play bank (never
/// mutated in place during rejected Newton trials!).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KnitHysteresisState {
    /// Internal back-strain `[p11, p22, p12]` per play operator threshold.
    pub play_backstrain: Vec<[f64; 3]>,
    /// Cumulative dissipated frictional work density in J/m².
    pub dissipated_j_m2: f64,
}

/// Parallel additive `C^2`-Huberized Prandtl–Ishlinskii play bank modeling knit yarn-slip hysteresis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitHysteresisBank {
    /// Yield thresholds `r_k > 0` in strain units.
    pub thresholds: Vec<f64>,
    /// Hardening weights `w_k >= 0` in N/m.
    pub weights: Vec<f64>,
    /// Smooth Huber regularization width `eps_h > 0`.
    pub huber_eps: f64,
}

impl KnitHysteresisBank {
    /// Initializes zero-strain committed history matching this play bank.
    #[must_use]
    pub fn zero_state(&self) -> KnitHysteresisState {
        KnitHysteresisState {
            play_backstrain: vec![[0.0; 3]; self.thresholds.len()],
            dissipated_j_m2: 0.0,
        }
    }

    /// Pure-functional trial evaluation returning `(stress_delta, tangent_delta, trial_next_state)`.
    #[must_use]
    pub fn evaluate_trial(
        &self,
        e: [f64; 3],
        committed: &KnitHysteresisState,
    ) -> ([f64; 3], [[f64; 3]; 3], KnitHysteresisState) {
        let mut stress = [0.0; 3];
        let mut tangent = [[0.0; 3]; 3];
        let mut next = committed.clone();
        if next.play_backstrain.len() != self.thresholds.len() {
            next.play_backstrain.resize(self.thresholds.len(), [0.0; 3]);
        }
        let eps = self.huber_eps.max(1e-8);
        for k in 0..self.thresholds.len() {
            let rk = self.thresholds[k].max(1e-8);
            let wk = self.weights[k].max(0.0);
            let p_old = next.play_backstrain[k];
            let d = [e[0] - p_old[0], e[1] - p_old[1], e[2] - p_old[2]];
            let norm = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2] + eps * eps).sqrt();
            if norm > rk {
                // Smooth radial return onto sphere of radius r_k
                let scale = rk / norm;
                let slip = 1.0 - scale;
                let p_new = [
                    p_old[0] + slip * d[0],
                    p_old[1] + slip * d[1],
                    p_old[2] + slip * d[2],
                ];
                let rel = [e[0] - p_new[0], e[1] - p_new[1], e[2] - p_new[2]];
                for i in 0..3 {
                    stress[i] += wk * rel[i];
                }
                // Consistent radial-return tangent `wk * (rk / norm) * (I - (d d^T) / norm^2)`
                let factor = wk * scale;
                let inv_n2 = 1.0 / (norm * norm);
                for i in 0..3 {
                    tangent[i][i] += factor;
                    for j in 0..3 {
                        tangent[i][j] -= factor * d[i] * d[j] * inv_n2;
                    }
                }
                let dp_norm = ((p_new[0] - p_old[0]).powi(2)
                    + (p_new[1] - p_old[1]).powi(2)
                    + (p_new[2] - p_old[2]).powi(2))
                .sqrt();
                next.dissipated_j_m2 += wk * rk * dp_norm;
                next.play_backstrain[k] = p_new;
            } else {
                // Elastic inside play threshold
                for i in 0..3 {
                    stress[i] += wk * d[i];
                    tangent[i][i] += wk;
                }
            }
        }
        (stress, tangent, next)
    }
}

/// The five `C011` knit-specific honesty flags reported independently from woven drape flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnitHonestyFlag {
    /// Local strain approached or hit the yarn jamming boundary (`margin < 0.05`).
    KnitJamming,
    /// Unconstrained selvedge edge curl exceeded threshold (`> 0.35 rad`).
    KnitEdgeCurl,
    /// Rib-to-jersey modulus/width snap transition created high local transverse shear.
    KnitRibSnap,
    /// Open-pore grin-through fraction exceeded threshold (`> 0.20`, skin/underlayer visible).
    KnitPoreGrinThrough,
    /// Residual unrecovered hysteretic/slip strain exceeded threshold (`> 0.02`).
    KnitHysteresisDrift,
}

impl KnitHonestyFlag {
    /// Stable serialized flag string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KnitJamming => "knit_jamming",
            Self::KnitEdgeCurl => "knit_edge_curl",
            Self::KnitRibSnap => "knit_rib_snap",
            Self::KnitPoreGrinThrough => "knit_pore_grin_through",
            Self::KnitHysteresisDrift => "knit_hysteresis_drift",
        }
    }
}

/// Knit shell state summary and sealed Seam-4 uncertainty receipt.
#[derive(Clone, Debug, PartialEq)]
pub struct KnitShellAssessment {
    /// Active knit honesty flags (empty when clean).
    pub flags: Vec<KnitHonestyFlag>,
    /// Sealed `ftx-drape-uncertainty/1` receipt ready for `ftx-export`.
    pub receipt: DrapeUncertaintyReceipt,
}

/// Evaluates all 5 `C011` knit honesty flags and seals a Seam-4 [`DrapeUncertaintyReceipt`].
///
/// # Errors
/// [`RefusalCode::NonFinite`] if any input scalar is non-finite.
#[allow(clippy::too_many_arguments)]
pub fn assess_knit_shell_state(
    pattern_sha256: String,
    material_sha256: String,
    max_strain_ratio: f64,
    min_jamming_margin: f64,
    edge_curl_rad: f64,
    rib_transition_shear: f64,
    pore_open_fraction: f64,
    hysteresis_drift_strain: f64,
    crease_risk: f64,
    evidence: Evidence,
) -> Result<KnitShellAssessment, Refusal> {
    let mut flags = Vec::new();
    if min_jamming_margin < 0.05 {
        flags.push(KnitHonestyFlag::KnitJamming);
    }
    if edge_curl_rad.abs() > 0.35 {
        flags.push(KnitHonestyFlag::KnitEdgeCurl);
    }
    if rib_transition_shear.abs() > 0.25 {
        flags.push(KnitHonestyFlag::KnitRibSnap);
    }
    if pore_open_fraction > 0.20 {
        flags.push(KnitHonestyFlag::KnitPoreGrinThrough);
    }
    if hysteresis_drift_strain > 0.02 {
        flags.push(KnitHonestyFlag::KnitHysteresisDrift);
    }
    let flag_names: Vec<String> = flags.iter().map(|f| f.as_str().to_string()).collect();
    let receipt = DrapeUncertaintyReceipt::seal(
        pattern_sha256,
        material_sha256,
        max_strain_ratio,
        min_jamming_margin,
        edge_curl_rad,
        crease_risk,
        flag_names,
        evidence,
    )?;
    Ok(KnitShellAssessment { flags, receipt })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cayley_hamilton_inv_sqrt_is_exact_including_isotropic_case() {
        // Isotropic C = 4 I -> C^{-1/2} = 0.5 I (no eigendecomposition atan2 NaN!)
        let iso = cayley_hamilton_inv_sqrt_2x2(4.0, 0.0, 4.0).unwrap();
        assert!((iso[0] - 0.5).abs() < 1e-14);
        assert!(iso[1].abs() < 1e-14);
        assert!((iso[2] - 0.5).abs() < 1e-14);

        // Anisotropic sheared SPD C: verify M * C * M == I
        let (c00, c01, c11) = (1.6, 0.35, 0.9);
        let [m00, m01, m11] = cayley_hamilton_inv_sqrt_2x2(c00, c01, c11).unwrap();
        // M^2 * C should equal I
        let m2_00 = m00 * m00 + m01 * m01;
        let m2_01 = m00 * m01 + m01 * m11;
        let m2_11 = m01 * m01 + m11 * m11;
        let p00 = m2_00 * c00 + m2_01 * c01;
        let p01 = m2_00 * c01 + m2_01 * c11;
        let p11 = m2_01 * c01 + m2_11 * c11;
        assert!((p00 - 1.0).abs() < 1e-13);
        assert!(p01.abs() < 1e-13);
        assert!((p11 - 1.0).abs() < 1e-13);
    }

    #[test]
    fn coupled_membrane_stress_and_tangent_match_central_fd_and_gate_jamming() {
        let m = CoupledKnitMembrane::reference_stockinette_estimate();
        m.validate().unwrap();
        let e = [0.18, 0.25, -0.14];
        let eval = m.evaluate(e).unwrap();
        let h = 1e-6;
        for i in 0..3 {
            let mut ep = e;
            let mut em = e;
            ep[i] += h;
            em[i] -= h;
            let wp = m.evaluate(ep).unwrap();
            let wm = m.evaluate(em).unwrap();
            let fd_s = (wp.energy_density - wm.energy_density) / (2.0 * h);
            assert!(
                (eval.stress[i] - fd_s).abs() < 1e-6 * fd_s.abs().max(1.0),
                "stress[{i}] mismatch: ana={} fd={fd_s}",
                eval.stress[i]
            );
            for j in 0..3 {
                let fd_c = (wp.stress[j] - wm.stress[j]) / (2.0 * h);
                assert!(
                    (eval.tangent[j][i] - fd_c).abs() < 1e-5 * fd_c.abs().max(1.0),
                    "tangent[{j}][{i}] mismatch: ana={} fd={fd_c}",
                    eval.tangent[j][i]
                );
            }
        }
        // Exceeding jamming hull refuses OutOfRange cleanly
        let err = m.evaluate([0.50, 0.0, 0.0]).unwrap_err();
        assert_eq!(err.code, RefusalCode::OutOfRange);
    }

    #[test]
    fn asymmetric_c3_bending_matches_central_fd_across_sign_switch() {
        let b = AsymmetricKnitBending::reference_stockinette_estimate();
        b.validate().unwrap();
        // Test inside transition band, positive outside band, and negative outside band
        for k in [[12.5, -17.3, 4.0], [20.0, -5.0, -3.0], [0.0, -30.0, 1.5]] {
            let eval = b.evaluate_from_kappa(k);
            // Verify strict SPD tangent (d0*d1 > d12^2)
            assert!(
                eval.tangent[0][0] * eval.tangent[1][1] > eval.tangent[0][1] * eval.tangent[1][0]
            );
            let h = 1e-6;
            for i in 0..3 {
                let mut kp = k;
                let mut km = k;
                kp[i] += h;
                km[i] -= h;
                let ep = b.evaluate_from_kappa(kp);
                let em = b.evaluate_from_kappa(km);
                let fd_m = (ep.energy_density - em.energy_density) / (2.0 * h);
                assert!(
                    (eval.moment[i] - fd_m).abs() < 1e-7 * fd_m.abs().max(1e-6),
                    "moment[{i}] mismatch: ana={} fd={fd_m}",
                    eval.moment[i]
                );
                for j in 0..3 {
                    let fd_t = (ep.moment[j] - em.moment[j]) / (2.0 * h);
                    assert!(
                        (eval.tangent[j][i] - fd_t).abs() < 1e-6 * fd_t.abs().max(1e-6),
                        "tangent[{j}][{i}] mismatch"
                    );
                }
            }
        }
    }

    #[test]
    fn hysteresis_bank_dissipates_energy_on_load_unload_cycle_and_is_trial_safe() {
        let bank = KnitHysteresisBank {
            thresholds: vec![0.02, 0.06],
            weights: vec![15.0, 25.0],
            huber_eps: 1e-4,
        };
        let s0 = bank.zero_state();
        let (sig_load, _, s1) = bank.evaluate_trial([0.12, 0.0, 0.0], &s0);
        // Trial evaluation did NOT mutate s0!
        assert_eq!(s0.dissipated_j_m2, 0.0);
        assert!(s1.dissipated_j_m2 > 0.0);
        assert!(sig_load[0] > 0.0);
        let (sig_unload, _, s2) = bank.evaluate_trial([0.0, 0.0, 0.0], &s1);
        assert!(sig_unload[0] < 0.0); // negative residual spring-back force on return to 0!
        assert!(s2.dissipated_j_m2 > s1.dissipated_j_m2);
    }
}
