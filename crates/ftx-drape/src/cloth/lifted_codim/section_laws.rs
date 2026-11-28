//! Pure-functional, trial-safe 1D rod section constitutive laws.
//!
//! [`RodSectionLaw::evaluate_section`] accepts `&SectionHistory` immutably and
//! returns `(SectionStressTangent, SectionHistory)`, ensuring rejected Newton or
//! line-search trials never mutate committed internal state.

use super::{CodimError, DetMath};

/// Committed or trial internal constitutive state of a rod segment.
#[derive(Clone, Debug, PartialEq)]
pub enum SectionHistory {
    /// Path-independent elastic state (no history evolution).
    Elastic,
    /// Superelastic shape-memory alloy state tracking martensite volume fraction
    /// `xi in [0, 1]` and last committed axial strain.
    SuperelasticSma {
        /// Martensite phase volume fraction in `[0, 1]`.
        martensite_frac: f64,
        /// Last committed axial strain.
        prev_axial_strain: f64,
    },
    /// Rate-independent elastoplastic state tracking committed plastic strains
    /// `[eps_p, 0, 0, 0, kappa_p1, kappa_p2]` and accumulated equivalent plastic
    /// variable `alpha >= 0`.
    Elastoplastic {
        /// Committed plastic strain vector `[axial, shear1, shear2, twist, kappa1, kappa2]`.
        plastic_strain_6: [f64; 6],
        /// Accumulated isotropic hardening variable `alpha >= 0`.
        equiv_plastic: f64,
    },
}

impl SectionHistory {
    /// Returns `true` if this history belongs to a path-dependent inelastic law.
    #[must_use]
    pub fn is_path_dependent(&self) -> bool {
        !matches!(self, Self::Elastic)
    }
}

/// Output of a section constitutive evaluation: strain energy density, 6D
/// generalized stress resultant `[N, V1, V2, Mt, M1, M2]`, 6x6 consistent
/// tangent, and 4x4 DER tangent slice `[axial, twist, kappa1, kappa2]`.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionStressTangent {
    /// Elastic/stored strain energy density per unit rest length (`J/m`).
    pub energy_density_j_m: f64,
    /// Generalized section forces/moments `[N, V1, V2, Mt, M1, M2]` (`N`, `N*m`).
    pub stress_6: [f64; 6],
    /// Consistent algorithmic tangent `d(stress_6) / d(strain_6)`.
    pub tangent_6x6: [[f64; 6]; 6],
    /// 4-DOF DER tangent slice over `[Gamma0, kappa0, kappa1, kappa2]`.
    pub tangent_4x4: [[f64; 4]; 4],
}

impl SectionStressTangent {
    fn from_6x6(energy_density_j_m: f64, stress_6: [f64; 6], tangent_6x6: [[f64; 6]; 6]) -> Self {
        let idx4 = [0usize, 3usize, 4usize, 5usize];
        let mut tangent_4x4 = [[0.0; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                tangent_4x4[r][c] = tangent_6x6[idx4[r]][idx4[c]];
            }
        }
        Self {
            energy_density_j_m,
            stress_6,
            tangent_6x6,
            tangent_4x4,
        }
    }
}

/// Pure-functional trial-safe trait for 1D rod cross-section constitutive laws.
pub trait RodSectionLaw {
    /// Evaluate stress resultants, consistent tangents, and candidate next
    /// history from generalized strains `(axial_strain, shear_strain,
    /// curvature_m_inv, twist_rad_m)` without mutating `committed`.
    fn evaluate_section<M: DetMath>(
        &self,
        axial_strain: f64,
        shear_strain: [f64; 2],
        curvature_m_inv: [f64; 2],
        twist_rad_m: f64,
        committed: &SectionHistory,
    ) -> Result<(SectionStressTangent, SectionHistory), CodimError>;

    /// Construct the initial committed history for this law.
    fn initial_history(&self) -> SectionHistory;

    /// Returns `true` iff this law is conservative and path-independent (admits
    /// single-solve elastic IFT adjoints).
    fn is_reversible(&self) -> bool;
}

/// Linear elastic isotropic Cosserat/DER cross-section law.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearElasticIsotropic {
    /// Axial stiffness `EA > 0` (`N`).
    pub ea: f64,
    /// Shear stiffness `GA > 0` (`N`).
    pub ga: f64,
    /// Torsional stiffness `GJ > 0` (`N*m^2`).
    pub gj: f64,
    /// Bending stiffness `EI > 0` (`N*m^2`).
    pub ei: f64,
}

impl LinearElasticIsotropic {
    /// Validate positive finite stiffness parameters.
    pub fn validate(&self) -> Result<(), CodimError> {
        if !self.ea.is_finite()
            || !self.ga.is_finite()
            || !self.gj.is_finite()
            || !self.ei.is_finite()
        {
            return Err(CodimError::NonFiniteInput {
                context: "LinearElasticIsotropic",
            });
        }
        if self.ea <= 0.0 || self.ga <= 0.0 || self.gj <= 0.0 || self.ei <= 0.0 {
            return Err(CodimError::InvalidParameter {
                detail: "LinearElasticIsotropic stiffnesses must be strictly positive",
            });
        }
        Ok(())
    }
}

impl RodSectionLaw for LinearElasticIsotropic {
    fn evaluate_section<M: DetMath>(
        &self,
        axial_strain: f64,
        shear_strain: [f64; 2],
        curvature_m_inv: [f64; 2],
        twist_rad_m: f64,
        committed: &SectionHistory,
    ) -> Result<(SectionStressTangent, SectionHistory), CodimError> {
        let _ = M::sqrt(1.0);
        self.validate()?;
        validate_strains(axial_strain, shear_strain, curvature_m_inv, twist_rad_m)?;
        if !matches!(committed, SectionHistory::Elastic) {
            return Err(CodimError::HistoryLawMismatch);
        }
        let s = [
            self.ea * axial_strain,
            self.ga * shear_strain[0],
            self.ga * shear_strain[1],
            self.gj * twist_rad_m,
            self.ei * curvature_m_inv[0],
            self.ei * curvature_m_inv[1],
        ];
        let e = 0.5
            * (s[0] * axial_strain
                + s[1] * shear_strain[0]
                + s[2] * shear_strain[1]
                + s[3] * twist_rad_m
                + s[4] * curvature_m_inv[0]
                + s[5] * curvature_m_inv[1]);
        let mut t = [[0.0; 6]; 6];
        t[0][0] = self.ea;
        t[1][1] = self.ga;
        t[2][2] = self.ga;
        t[3][3] = self.gj;
        t[4][4] = self.ei;
        t[5][5] = self.ei;
        Ok((
            SectionStressTangent::from_6x6(e, s, t),
            SectionHistory::Elastic,
        ))
    }

    fn initial_history(&self) -> SectionHistory {
        SectionHistory::Elastic
    }

    fn is_reversible(&self) -> bool {
        true
    }
}

/// Smooth convex hyperelastic nonlinear filament-bundle law capturing crimp /
/// geometric compaction stiffening in both axial stretch and bending curvature.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NonlinearFilamentBundle {
    /// Base linear axial stiffness `EA > 0` (`N`).
    pub ea: f64,
    /// Nonlinear crimp-stiffening axial modulus `EA_crimp >= 0` (`N`).
    pub ea_crimp: f64,
    /// Characteristic crimp strain scale `eps_crimp > 0`.
    pub crimp_strain: f64,
    /// Shear stiffness `GA > 0` (`N`).
    pub ga: f64,
    /// Torsional stiffness `GJ > 0` (`N*m^2`).
    pub gj: f64,
    /// Base bending stiffness `EI > 0` (`N*m^2`).
    pub ei: f64,
    /// Dimensionless quadratic bending compaction factor `beta_b >= 0` (`m^2`).
    pub bending_hardening: f64,
}

impl NonlinearFilamentBundle {
    /// Validate parameters.
    pub fn validate(&self) -> Result<(), CodimError> {
        if !self.ea.is_finite()
            || !self.ea_crimp.is_finite()
            || !self.crimp_strain.is_finite()
            || !self.ga.is_finite()
            || !self.gj.is_finite()
            || !self.ei.is_finite()
            || !self.bending_hardening.is_finite()
        {
            return Err(CodimError::NonFiniteInput {
                context: "NonlinearFilamentBundle",
            });
        }
        if self.ea <= 0.0
            || self.ea_crimp < 0.0
            || self.crimp_strain <= 0.0
            || self.ga <= 0.0
            || self.gj <= 0.0
            || self.ei <= 0.0
            || self.bending_hardening < 0.0
        {
            return Err(CodimError::InvalidParameter {
                detail: "NonlinearFilamentBundle parameters must be positive (hardening/crimp >= 0)",
            });
        }
        Ok(())
    }
}

impl RodSectionLaw for NonlinearFilamentBundle {
    fn evaluate_section<M: DetMath>(
        &self,
        axial_strain: f64,
        shear_strain: [f64; 2],
        curvature_m_inv: [f64; 2],
        twist_rad_m: f64,
        committed: &SectionHistory,
    ) -> Result<(SectionStressTangent, SectionHistory), CodimError> {
        let _ = M::sqrt(1.0);
        self.validate()?;
        validate_strains(axial_strain, shear_strain, curvature_m_inv, twist_rad_m)?;
        if !matches!(committed, SectionHistory::Elastic) {
            return Err(CodimError::HistoryLawMismatch);
        }
        let ec2 = self.crimp_strain * self.crimp_strain;
        let eps = axial_strain;
        let eps2 = eps * eps;
        let eps3 = eps2 * eps;
        let eps4 = eps2 * eps2;
        let crimp_coeff = self.ea_crimp / ec2;

        let e_axial = 0.5 * self.ea * eps2 + 0.25 * crimp_coeff * eps4;
        let n_axial = self.ea * eps + crimp_coeff * eps3;
        let k_axial = self.ea + 3.0 * crimp_coeff * eps2;

        let k1 = curvature_m_inv[0];
        let k2 = curvature_m_inv[1];
        let k_sq = k1 * k1 + k2 * k2;
        let bh = self.bending_hardening;
        let b_fac = 1.0 + bh * k_sq;
        let e_bend = 0.5 * self.ei * k_sq + 0.25 * self.ei * bh * k_sq * k_sq;
        let m1 = self.ei * b_fac * k1;
        let m2 = self.ei * b_fac * k2;

        let v1 = self.ga * shear_strain[0];
        let v2 = self.ga * shear_strain[1];
        let mt = self.gj * twist_rad_m;
        let e_other = 0.5 * (v1 * shear_strain[0] + v2 * shear_strain[1] + mt * twist_rad_m);

        let mut t = [[0.0; 6]; 6];
        t[0][0] = k_axial;
        t[1][1] = self.ga;
        t[2][2] = self.ga;
        t[3][3] = self.gj;
        t[4][4] = self.ei * b_fac + 2.0 * self.ei * bh * k1 * k1;
        t[4][5] = 2.0 * self.ei * bh * k1 * k2;
        t[5][4] = t[4][5];
        t[5][5] = self.ei * b_fac + 2.0 * self.ei * bh * k2 * k2;

        Ok((
            SectionStressTangent::from_6x6(
                e_axial + e_bend + e_other,
                [n_axial, v1, v2, mt, m1, m2],
                t,
            ),
            SectionHistory::Elastic,
        ))
    }

    fn initial_history(&self) -> SectionHistory {
        SectionHistory::Elastic
    }

    fn is_reversible(&self) -> bool {
        true
    }
}

/// 1D superelastic shape-memory alloy (SMA) constitutive model tracking
/// martensite volume fraction `xi in [0, 1]` with forward/reverse transformation
/// hysteresis (`sigma_af < sigma_as < sigma_ms < sigma_mf`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuperelasticSma {
    /// Austenite axial stiffness `EA_A > 0` (`N`).
    pub ea_austenite: f64,
    /// Martensite axial stiffness `EA_M > 0` (`N`).
    pub ea_martensite: f64,
    /// Forward transformation start axial force `sigma_ms > 0` (`N`).
    pub sigma_ms: f64,
    /// Forward transformation finish axial force `sigma_mf > sigma_ms` (`N`).
    pub sigma_mf: f64,
    /// Reverse transformation start axial force `sigma_as > sigma_af` (`N`).
    pub sigma_as: f64,
    /// Reverse transformation finish axial force `sigma_af > 0` (`N`).
    pub sigma_af: f64,
    /// Maximum transformation strain `eps_L > 0`.
    pub max_transform_strain: f64,
    /// Shear stiffness `GA > 0` (`N`).
    pub ga: f64,
    /// Torsional stiffness `GJ > 0` (`N*m^2`).
    pub gj: f64,
    /// Bending stiffness `EI > 0` (`N*m^2`).
    pub ei: f64,
}

impl SuperelasticSma {
    /// Validate SMA phase-transformation and elastic parameters.
    pub fn validate(&self) -> Result<(), CodimError> {
        let vals = [
            self.ea_austenite,
            self.ea_martensite,
            self.sigma_ms,
            self.sigma_mf,
            self.sigma_as,
            self.sigma_af,
            self.max_transform_strain,
            self.ga,
            self.gj,
            self.ei,
        ];
        if !vals.iter().all(|v| v.is_finite()) {
            return Err(CodimError::NonFiniteInput {
                context: "SuperelasticSma",
            });
        }
        if self.ea_austenite <= 0.0
            || self.ea_martensite <= 0.0
            || self.max_transform_strain <= 0.0
            || self.ga <= 0.0
            || self.gj <= 0.0
            || self.ei <= 0.0
            || self.sigma_af <= 0.0
            || self.sigma_as <= self.sigma_af
            || self.sigma_ms <= self.sigma_as
            || self.sigma_mf <= self.sigma_ms
        {
            return Err(CodimError::InvalidParameter {
                detail: "SuperelasticSma requires 0 < sigma_af < sigma_as < sigma_ms < sigma_mf and positive moduli",
            });
        }
        Ok(())
    }

    fn effective_ea(&self, xi: f64) -> f64 {
        let inv_ea = (1.0 - xi) / self.ea_austenite + xi / self.ea_martensite;
        1.0 / inv_ea
    }

    fn solve_xi_branch<M: DetMath>(
        &self,
        sigma0: f64,
        dsigma: f64,
        eps: f64,
        xi_lo: f64,
        xi_hi: f64,
    ) -> (f64, f64, f64) {
        let inv_ea_a = 1.0 / self.ea_austenite;
        let dc = 1.0 / self.ea_martensite - inv_ea_a;
        let eps_l = self.max_transform_strain;
        let a = dsigma * dc;
        let b = dsigma * inv_ea_a + sigma0 * dc + eps_l;
        let c = sigma0 * inv_ea_a - eps;
        let xi = if a.abs() <= 1e-18 {
            (-c / b).clamp(xi_lo, xi_hi)
        } else {
            let disc = (b * b - 4.0 * a * c).max(0.0);
            let root = (-b + M::sqrt(disc)) / (2.0 * a);
            root.clamp(xi_lo, xi_hi)
        };
        let n = sigma0 + xi * dsigma;
        let d_eps_d_xi = 2.0 * a * xi + b;
        let k_tan = if d_eps_d_xi.abs() > 1e-15 {
            dsigma / d_eps_d_xi
        } else {
            self.effective_ea(xi)
        };
        (xi, n, k_tan)
    }
}

impl RodSectionLaw for SuperelasticSma {
    fn evaluate_section<M: DetMath>(
        &self,
        axial_strain: f64,
        shear_strain: [f64; 2],
        curvature_m_inv: [f64; 2],
        twist_rad_m: f64,
        committed: &SectionHistory,
    ) -> Result<(SectionStressTangent, SectionHistory), CodimError> {
        self.validate()?;
        validate_strains(axial_strain, shear_strain, curvature_m_inv, twist_rad_m)?;
        let SectionHistory::SuperelasticSma {
            martensite_frac: xi_n,
            prev_axial_strain: _,
        } = *committed
        else {
            return Err(CodimError::HistoryLawMismatch);
        };
        if !xi_n.is_finite() || !(0.0..=1.0).contains(&xi_n) {
            return Err(CodimError::InvalidParameter {
                detail: "SMA martensite_frac must be in [0, 1]",
            });
        }

        let ea_n = self.effective_ea(xi_n);
        let eps_el_trial = axial_strain - xi_n * self.max_transform_strain;
        let n_trial = ea_n * eps_el_trial;

        let forward_thresh = self.sigma_ms + xi_n * (self.sigma_mf - self.sigma_ms);
        let reverse_thresh = self.sigma_af + xi_n * (self.sigma_as - self.sigma_af);

        let (xi_new, n_axial, k_axial) = if n_trial > forward_thresh && xi_n < 1.0 {
            let dsig = self.sigma_mf - self.sigma_ms;
            let (xi, n_cand, k_cand) =
                self.solve_xi_branch::<M>(self.sigma_ms, dsig, axial_strain, xi_n, 1.0);
            if xi >= 1.0 - 1e-15 {
                let ea_1 = self.effective_ea(1.0);
                (1.0, ea_1 * (axial_strain - self.max_transform_strain), ea_1)
            } else {
                (xi, n_cand, k_cand)
            }
        } else if n_trial < reverse_thresh && xi_n > 0.0 {
            let dsig = self.sigma_as - self.sigma_af;
            let (xi, n_cand, k_cand) =
                self.solve_xi_branch::<M>(self.sigma_af, dsig, axial_strain, 0.0, xi_n);
            if xi <= 1e-15 {
                (0.0, self.ea_austenite * axial_strain, self.ea_austenite)
            } else {
                (xi, n_cand, k_cand)
            }
        } else {
            (xi_n, n_trial, ea_n)
        };

        let ea_new = self.effective_ea(xi_new);
        let eps_el = axial_strain - xi_new * self.max_transform_strain;
        let e_axial = 0.5 * ea_new * eps_el * eps_el;

        let v1 = self.ga * shear_strain[0];
        let v2 = self.ga * shear_strain[1];
        let mt = self.gj * twist_rad_m;
        let m1 = self.ei * curvature_m_inv[0];
        let m2 = self.ei * curvature_m_inv[1];
        let e_other = 0.5
            * (v1 * shear_strain[0]
                + v2 * shear_strain[1]
                + mt * twist_rad_m
                + m1 * curvature_m_inv[0]
                + m2 * curvature_m_inv[1]);

        let mut t = [[0.0; 6]; 6];
        t[0][0] = k_axial;
        t[1][1] = self.ga;
        t[2][2] = self.ga;
        t[3][3] = self.gj;
        t[4][4] = self.ei;
        t[5][5] = self.ei;

        Ok((
            SectionStressTangent::from_6x6(e_axial + e_other, [n_axial, v1, v2, mt, m1, m2], t),
            SectionHistory::SuperelasticSma {
                martensite_frac: xi_new,
                prev_axial_strain: axial_strain,
            },
        ))
    }

    fn initial_history(&self) -> SectionHistory {
        SectionHistory::SuperelasticSma {
            martensite_frac: 0.0,
            prev_axial_strain: 0.0,
        }
    }

    fn is_reversible(&self) -> bool {
        false
    }
}

/// Rate-independent elastoplastic cross-section law with coupled axial-bending
/// yield surface `sqrt((N/Ny)^2 + (M1^2 + M2^2)/My^2) <= 1 + h * alpha` and
/// exact symmetric consistent algorithmic tangent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElastoplasticYield {
    /// Axial stiffness `EA > 0` (`N`).
    pub ea: f64,
    /// Shear stiffness `GA > 0` (`N`).
    pub ga: f64,
    /// Torsional stiffness `GJ > 0` (`N*m^2`).
    pub gj: f64,
    /// Bending stiffness `EI > 0` (`N*m^2`).
    pub ei: f64,
    /// Initial axial yield force `N_y > 0` (`N`).
    pub yield_force_n: f64,
    /// Initial bending yield moment `M_y > 0` (`N*m`).
    pub yield_moment_nm: f64,
    /// Dimensionless isotropic hardening slope `h >= 0`.
    pub hardening_ratio: f64,
}

impl ElastoplasticYield {
    /// Validate elastoplastic section parameters.
    pub fn validate(&self) -> Result<(), CodimError> {
        let vals = [
            self.ea,
            self.ga,
            self.gj,
            self.ei,
            self.yield_force_n,
            self.yield_moment_nm,
            self.hardening_ratio,
        ];
        if !vals.iter().all(|v| v.is_finite()) {
            return Err(CodimError::NonFiniteInput {
                context: "ElastoplasticYield",
            });
        }
        if self.ea <= 0.0
            || self.ga <= 0.0
            || self.gj <= 0.0
            || self.ei <= 0.0
            || self.yield_force_n <= 0.0
            || self.yield_moment_nm <= 0.0
            || self.hardening_ratio < 0.0
        {
            return Err(CodimError::InvalidParameter {
                detail: "ElastoplasticYield requires positive moduli/yield thresholds and hardening_ratio >= 0",
            });
        }
        Ok(())
    }
}

impl RodSectionLaw for ElastoplasticYield {
    fn evaluate_section<M: DetMath>(
        &self,
        axial_strain: f64,
        shear_strain: [f64; 2],
        curvature_m_inv: [f64; 2],
        twist_rad_m: f64,
        committed: &SectionHistory,
    ) -> Result<(SectionStressTangent, SectionHistory), CodimError> {
        self.validate()?;
        validate_strains(axial_strain, shear_strain, curvature_m_inv, twist_rad_m)?;
        let SectionHistory::Elastoplastic {
            plastic_strain_6: ep_n,
            equiv_plastic: alpha_n,
        } = *committed
        else {
            return Err(CodimError::HistoryLawMismatch);
        };
        if !alpha_n.is_finite() || alpha_n < 0.0 || !ep_n.iter().all(|v| v.is_finite()) {
            return Err(CodimError::InvalidParameter {
                detail: "Elastoplastic history contains non-finite or negative alpha",
            });
        }

        let eps_tr = [
            axial_strain - ep_n[0],
            curvature_m_inv[0] - ep_n[4],
            curvature_m_inv[1] - ep_n[5],
        ];
        let s_tr = [
            self.ea * eps_tr[0],
            self.ei * eps_tr[1],
            self.ei * eps_tr[2],
        ];
        let ny2 = self.yield_force_n * self.yield_force_n;
        let my2 = self.yield_moment_nm * self.yield_moment_nm;
        let q_tr =
            M::sqrt((s_tr[0] * s_tr[0]) / ny2 + (s_tr[1] * s_tr[1] + s_tr[2] * s_tr[2]) / my2);
        let y_n = 1.0 + self.hardening_ratio * alpha_n;

        let v1 = self.ga * shear_strain[0];
        let v2 = self.ga * shear_strain[1];
        let mt = self.gj * twist_rad_m;
        let e_st = 0.5 * (v1 * shear_strain[0] + v2 * shear_strain[1] + mt * twist_rad_m);

        let mut t = [[0.0; 6]; 6];
        t[1][1] = self.ga;
        t[2][2] = self.ga;
        t[3][3] = self.gj;

        if q_tr <= y_n {
            let e_ab = 0.5 * (s_tr[0] * eps_tr[0] + s_tr[1] * eps_tr[1] + s_tr[2] * eps_tr[2]);
            t[0][0] = self.ea;
            t[4][4] = self.ei;
            t[5][5] = self.ei;
            return Ok((
                SectionStressTangent::from_6x6(
                    e_ab + e_st,
                    [s_tr[0], v1, v2, mt, s_tr[1], s_tr[2]],
                    t,
                ),
                committed.clone(),
            ));
        }

        let ca = self.ea / ny2;
        let cb = self.ei / my2;
        let h = self.hardening_ratio;
        let a2 = (s_tr[0] * s_tr[0]) / ny2;
        let b2 = (s_tr[1] * s_tr[1] + s_tr[2] * s_tr[2]) / my2;
        let eval_g = |u: f64| -> (f64, f64, f64) {
            let da = 1.0 + ca * u;
            let db = 1.0 + cb * u;
            let q = M::sqrt(a2 / (da * da) + b2 / (db * db));
            let dq_du = if q > 1e-30 {
                -(a2 * ca / (da * da * da) + b2 * cb / (db * db * db)) / q
            } else {
                0.0
            };
            let g = q - y_n - h * u * q;
            let dg_du = dq_du * (1.0 - h * u) - h * q;
            (g, dg_du, q)
        };

        let mut u_lo = 0.0;
        let mut u_hi = (q_tr - y_n) / (q_tr * ca.min(cb).max(1e-30) + 1e-30);
        if u_hi <= 0.0 {
            u_hi = 1.0;
        }
        while eval_g(u_hi).0 > 0.0 {
            u_lo = u_hi;
            u_hi *= 2.0;
            if !u_hi.is_finite() {
                return Err(CodimError::NewtonStalled {
                    iters: 64,
                    residual: eval_g(u_lo).0,
                });
            }
        }
        let mut u = 0.5 * (u_lo + u_hi);
        for _ in 0..64 {
            let (g, dg, _) = eval_g(u);
            if g.abs() <= 1e-14 * y_n {
                break;
            }
            if g > 0.0 {
                u_lo = u;
            } else {
                u_hi = u;
            }
            let cand = if dg.abs() > 1e-30 { u - g / dg } else { u };
            u = if cand > u_lo && cand < u_hi {
                cand
            } else {
                0.5 * (u_lo + u_hi)
            };
        }
        let (_, _, q_final) = eval_g(u);
        let da = 1.0 + ca * u;
        let db = 1.0 + cb * u;
        let s = [s_tr[0] / da, s_tr[1] / db, s_tr[2] / db];
        let a_diag = [self.ea / da, self.ei / db, self.ei / db];
        let n_dir = [
            s[0] / (q_final * ny2),
            s[1] / (q_final * my2),
            s[2] / (q_final * my2),
        ];
        let an = [
            a_diag[0] * n_dir[0],
            a_diag[1] * n_dir[1],
            a_diag[2] * n_dir[2],
        ];
        let n_an = n_dir[0] * an[0] + n_dir[1] * an[1] + n_dir[2] * an[2];
        let h_term = if (1.0 - h * u).abs() > 1e-15 {
            h / (1.0 - h * u)
        } else {
            h * 1e15
        };
        let denom = n_an + h_term;

        let map_idx = [0usize, 4usize, 5usize];
        for i in 0..3 {
            for j in 0..3 {
                let base = if i == j { a_diag[i] } else { 0.0 };
                t[map_idx[i]][map_idx[j]] = base - (an[i] * an[j]) / denom;
            }
        }

        let d_ep0 = u * s[0] / ny2;
        let d_ep4 = u * s[1] / my2;
        let d_ep5 = u * s[2] / my2;
        let new_ep = [
            ep_n[0] + d_ep0,
            0.0,
            0.0,
            0.0,
            ep_n[4] + d_ep4,
            ep_n[5] + d_ep5,
        ];
        let new_alpha = alpha_n + u * q_final;
        let eps_el = [
            axial_strain - new_ep[0],
            curvature_m_inv[0] - new_ep[4],
            curvature_m_inv[1] - new_ep[5],
        ];
        let e_ab = 0.5 * (s[0] * eps_el[0] + s[1] * eps_el[1] + s[2] * eps_el[2]);

        Ok((
            SectionStressTangent::from_6x6(e_ab + e_st, [s[0], v1, v2, mt, s[1], s[2]], t),
            SectionHistory::Elastoplastic {
                plastic_strain_6: new_ep,
                equiv_plastic: new_alpha,
            },
        ))
    }

    fn initial_history(&self) -> SectionHistory {
        SectionHistory::Elastoplastic {
            plastic_strain_6: [0.0; 6],
            equiv_plastic: 0.0,
        }
    }

    fn is_reversible(&self) -> bool {
        false
    }
}

/// Tagged union of all built-in rod cross-section constitutive laws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RodSectionModel {
    /// Linear elastic isotropic rod section.
    LinearElasticIsotropic(LinearElasticIsotropic),
    /// Hyperelastic nonlinear filament bundle section.
    NonlinearFilamentBundle(NonlinearFilamentBundle),
    /// Superelastic shape-memory alloy (path-dependent).
    SuperelasticSma(SuperelasticSma),
    /// Coupled axial-bending elastoplastic yield (path-dependent).
    ElastoplasticYield(ElastoplasticYield),
}

impl RodSectionModel {
    /// Validate the active model variant.
    pub fn validate(&self) -> Result<(), CodimError> {
        match self {
            Self::LinearElasticIsotropic(l) => l.validate(),
            Self::NonlinearFilamentBundle(l) => l.validate(),
            Self::SuperelasticSma(l) => l.validate(),
            Self::ElastoplasticYield(l) => l.validate(),
        }
    }
}

impl RodSectionLaw for RodSectionModel {
    fn evaluate_section<M: DetMath>(
        &self,
        axial_strain: f64,
        shear_strain: [f64; 2],
        curvature_m_inv: [f64; 2],
        twist_rad_m: f64,
        committed: &SectionHistory,
    ) -> Result<(SectionStressTangent, SectionHistory), CodimError> {
        match self {
            Self::LinearElasticIsotropic(l) => l.evaluate_section::<M>(
                axial_strain,
                shear_strain,
                curvature_m_inv,
                twist_rad_m,
                committed,
            ),
            Self::NonlinearFilamentBundle(l) => l.evaluate_section::<M>(
                axial_strain,
                shear_strain,
                curvature_m_inv,
                twist_rad_m,
                committed,
            ),
            Self::SuperelasticSma(l) => l.evaluate_section::<M>(
                axial_strain,
                shear_strain,
                curvature_m_inv,
                twist_rad_m,
                committed,
            ),
            Self::ElastoplasticYield(l) => l.evaluate_section::<M>(
                axial_strain,
                shear_strain,
                curvature_m_inv,
                twist_rad_m,
                committed,
            ),
        }
    }

    fn initial_history(&self) -> SectionHistory {
        match self {
            Self::LinearElasticIsotropic(l) => l.initial_history(),
            Self::NonlinearFilamentBundle(l) => l.initial_history(),
            Self::SuperelasticSma(l) => l.initial_history(),
            Self::ElastoplasticYield(l) => l.initial_history(),
        }
    }

    fn is_reversible(&self) -> bool {
        match self {
            Self::LinearElasticIsotropic(l) => l.is_reversible(),
            Self::NonlinearFilamentBundle(l) => l.is_reversible(),
            Self::SuperelasticSma(l) => l.is_reversible(),
            Self::ElastoplasticYield(l) => l.is_reversible(),
        }
    }
}

fn validate_strains(
    axial_strain: f64,
    shear_strain: [f64; 2],
    curvature_m_inv: [f64; 2],
    twist_rad_m: f64,
) -> Result<(), CodimError> {
    if !axial_strain.is_finite()
        || !shear_strain[0].is_finite()
        || !shear_strain[1].is_finite()
        || !curvature_m_inv[0].is_finite()
        || !curvature_m_inv[1].is_finite()
        || !twist_rad_m.is_finite()
    {
        return Err(CodimError::NonFiniteInput {
            context: "section strain vector",
        });
    }
    Ok(())
}
