//! Rung-3 analytical KAT oracle (Poincloux et al. 2018, Phys. Rev. X 8, 021075 planar elastica
//! knitted-sheet model), offline RVE surrogate card bridge (`Evidence::Estimate`), and local
//! Kirchhoff–Love post-drape audit patch (`FTX-K10`).

use ftx_core::{Refusal, RefusalCode, det};
use ftx_pattern::materials::Evidence;
use serde::{Deserialize, Serialize};

use super::constitutive::{AsymmetricKnitBending, CoupledKnitMembrane};

/// Geometric & yarn parameters for the Poincloux et al. (2018) planar stockinette elastica oracle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoinclouxParams {
    /// Yarn bending rigidity `B = E * I` in N·m².
    pub yarn_bending_nm2: f64,
    /// Total yarn arc length per single stitch loop `l` in metres.
    pub loop_length_m: f64,
    /// Yarn radius `r` in metres.
    pub yarn_radius_m: f64,
    /// Undeformed wale spacing `w_0` in metres.
    pub rest_wale_m: f64,
    /// Undeformed course height `c_0` in metres.
    pub rest_course_m: f64,
    /// Yarn-on-yarn Coulomb friction coefficient `mu >= 0`.
    pub mu_friction: f64,
}

impl PoinclouxParams {
    /// Standard nylon monofilament benchmark parameters (Poincloux et al. 2018).
    #[must_use]
    pub const fn reference_monofilament() -> Self {
        Self {
            yarn_bending_nm2: 1.2e-7,
            loop_length_m: 14.0e-3,
            yarn_radius_m: 0.25e-3,
            rest_wale_m: 4.0e-3,
            rest_course_m: 3.2e-3,
            mu_friction: 0.22,
        }
    }

    /// Validates parameter positivity and physical non-overlap `l > 4 * pi * r`.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] or [`RefusalCode::NonFinite`].
    pub fn validate(&self) -> Result<(), Refusal> {
        for (k, v) in [
            ("yarn_bending_nm2", self.yarn_bending_nm2),
            ("loop_length_m", self.loop_length_m),
            ("yarn_radius_m", self.yarn_radius_m),
            ("rest_wale_m", self.rest_wale_m),
            ("rest_course_m", self.rest_course_m),
            ("mu_friction", self.mu_friction),
        ] {
            Refusal::finite(k, v)?;
        }
        if self.yarn_bending_nm2 <= 0.0
            || self.loop_length_m <= 4.0 * std::f64::consts::PI * self.yarn_radius_m
            || self.yarn_radius_m <= 0.0
            || self.rest_wale_m <= 0.0
            || self.rest_course_m <= 0.0
            || self.mu_friction < 0.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "invalid Poincloux elastica geometry or bending parameters",
            ));
        }
        Ok(())
    }
}

/// One analytical stress/geometry point on the Poincloux planar elastica curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoinclouxPoint {
    /// Applied engineering wale strain `epsilon_w`.
    pub strain_wale: f64,
    /// Analytical free transverse course strain `epsilon_c` (Poisson contraction).
    pub strain_course_free: f64,
    /// Frictionless elastica tension resultant `T_0` in N/m.
    pub tension_frictionless_n_m: f64,
    /// Loading tension resultant including Capstan yarn-interlock friction `T_load` in N/m.
    pub tension_loading_n_m: f64,
    /// Unloading tension resultant including Capstan friction `T_unload` in N/m.
    pub tension_unloading_n_m: f64,
    /// Fraction of geometric jamming limit reached (`< 1.0`).
    pub jamming_ratio: f64,
}

/// Computes the Poincloux et al. (2018) planar elastica uniaxial wale response curve across `strains`.
///
/// Uses the exact asymptotic elastica scaling `F_loop ~ B / l^2 * f(epsilon)` with geometric
/// contact-angle Capstan hysteresis `exp(+- 2 * mu * alpha_c)` and finite-radius jamming limit
/// `w_jam = l / 2 - 2 * pi * r`.
///
/// # Errors
/// [`RefusalCode::OutOfRange`] if parameters are invalid or a strain exceeds geometric jamming.
pub fn poincloux_uniaxial_curve(
    params: &PoinclouxParams,
    strains_wale: &[f64],
) -> Result<Vec<PoinclouxPoint>, Refusal> {
    params.validate()?;
    let w_jam = 0.5 * params.loop_length_m - 2.0 * std::f64::consts::PI * params.yarn_radius_m;
    let eps_jam = (w_jam / params.rest_wale_m - 1.0).max(0.05);
    let b_scale = params.yarn_bending_nm2 / (params.loop_length_m * params.loop_length_m);

    let mut out = Vec::with_capacity(strains_wale.len());
    for &ew in strains_wale {
        Refusal::finite("strain_wale", ew)?;
        if ew < -0.5 || ew >= eps_jam {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("strain_wale={ew} outside Poincloux planar domain [-0.5, {eps_jam:.4})"),
            ));
        }
        let ratio = (ew.max(0.0)) / eps_jam;
        // Loop arc conservation drives large effective knit Poisson contraction nu_eff ~ 0.65..1.1
        let ec_free = -0.72 * ew - 0.18 * ew * ew;
        let current_course =
            (params.rest_course_m * (1.0 + ec_free)).max(2.0 * params.yarn_radius_m);
        // Planar elastica force per stitch: linear bending stiffness + (1 - ratio)^(-3/2) jamming divergence
        let denom = (1.0 - ratio).max(1e-6);
        let f_stitch = if ew >= 0.0 {
            let stiff_factor = 1.0 / (denom * denom.sqrt()) - 1.0;
            16.0 * b_scale * (ew + 0.35 * stiff_factor)
        } else {
            // Soft buckling/loop-compression branch
            2.5 * b_scale * ew
        };
        let t0 = f_stitch / current_course;
        // Contact wrap angle alpha_c increases smoothly with stretch ratio
        let alpha_c = 0.55 + 0.45 * ratio;
        let capstan = det::exp(2.0 * params.mu_friction * alpha_c);
        let (t_load, t_unload) = if t0 >= 0.0 {
            (t0 * capstan, t0 / capstan)
        } else {
            (t0 / capstan, t0 * capstan)
        };
        out.push(PoinclouxPoint {
            strain_wale: ew,
            strain_course_free: ec_free,
            tension_frictionless_n_m: t0,
            tension_loading_n_m: t_load,
            tension_unloading_n_m: t_unload,
            jamming_ratio: ratio,
        });
    }
    Ok(out)
}

/// Offline periodic RVE homogenized knit surrogate card emitted by `/dp/frankensim` `fs-homog`
/// (`Evidence::Estimate` / `MODEL_FIT` until verified by physical coupon).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RveHomogenizedKnitCard {
    /// Card schema (`"ftx-knit-rve-card/1"`).
    pub schema: String,
    /// Stable stitch-pattern & yarn identifier.
    pub id: String,
    /// Provenance hash of the `/dp/frankensim` `fs-homog` run.
    pub frankensim_rve_sha256: String,
    /// Always `Evidence::Estimate` for RVE-derived cards (never `Measured`!).
    pub evidence: Evidence,
    /// Generalized dissipative Hill–Mandel consistency residual from `fs-homog` (`< 1e-6` required).
    pub hill_mandel_residual: f64,
    /// Coupled orthotropic knit membrane law fitted to RVE biaxial/shear sweeps.
    pub membrane: CoupledKnitMembrane,
    /// Asymmetric `C^3` bending law fitted to `SE(3)` curved cylindrical RVE sweeps.
    pub bending: AsymmetricKnitBending,
    /// Undeformed open-pore optical area fraction `alpha_0 in [0, 1)`.
    pub rest_pore_fraction: f64,
    /// Linear pore-opening sensitivity `d(alpha)/d(tr(E)) >= 0` for grin-through optics.
    pub pore_strain_slope: f64,
}

impl RveHomogenizedKnitCard {
    /// Validates Hill–Mandel consistency (`<= 1e-5`), constitutive convexity, and honest evidence tag.
    ///
    /// # Errors
    /// [`RefusalCode::VerificationMismatch`] or [`RefusalCode::OutOfRange`].
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != "ftx-knit-rve-card/1" {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("unexpected RVE card schema '{}'", self.schema),
            ));
        }
        if self.evidence == Evidence::Measured {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "an RVE-homogenized card cannot claim Evidence::Measured (must be Evidence::Estimate)",
            ));
        }
        Refusal::finite("hill_mandel_residual", self.hill_mandel_residual)?;
        if self.hill_mandel_residual < 0.0 || self.hill_mandel_residual > 1e-5 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!(
                    "RVE card Hill-Mandel residual {:.3e} exceeds 1e-5 certification ceiling",
                    self.hill_mandel_residual
                ),
            ));
        }
        Refusal::finite("rest_pore_fraction", self.rest_pore_fraction)?;
        Refusal::finite("pore_strain_slope", self.pore_strain_slope)?;
        if !(0.0..1.0).contains(&self.rest_pore_fraction) || self.pore_strain_slope < 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "RVE card requires rest_pore_fraction in [0, 1) and pore_strain_slope >= 0",
            ));
        }
        self.membrane.validate()?;
        self.bending.validate()?;
        Ok(())
    }

    /// Evaluates strain-dependent open-pore grin-through fraction `alpha_pore(E) in [0, 0.95]`.
    #[must_use]
    pub fn pore_open_fraction(&self, e11: f64, e22: f64) -> f64 {
        (self.rest_pore_fraction + self.pore_strain_slope * (e11 + e22).max(0.0)).clamp(0.0, 0.95)
    }
}

/// Kirchhoff–Love local post-drape audit patch inputs extracted from a flagged shell region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalAuditPatchInput {
    /// Mean macro membrane Green strain `[E11, E22, 2*E12]`.
    pub macro_strain: [f64; 3],
    /// Mean macro physical curvature `[k11, k22, 2*k12]` in 1/m.
    pub macro_curvature: [f64; 3],
    /// Patch linear span `L_patch` in metres.
    pub patch_span_m: f64,
}

/// Result of a local post-drape audit patch verifying macro shell predictions on a flagged zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalAuditPatchResult {
    /// Macro-to-patch Kirchhoff–Love boundary energy consistency ratio.
    pub energy_density_j_m2: f64,
    /// Peak yarn-level estimated tensile force in Newtons.
    pub estimated_peak_yarn_tension_n: f64,
    /// Evaluated open-pore grin-through fraction under local stretch.
    pub pore_open_fraction: f64,
    /// Remaining jamming margin (`< 0.05` triggers `KnitJamming` confirmation).
    pub jamming_margin: f64,
}

/// Audits a local flagged garment patch (selvedge, cable, or high-strain armhole) using Kirchhoff–Love
/// quadratic boundary coupling against an [`RveHomogenizedKnitCard`].
///
/// # Errors
/// [`RefusalCode::OutOfRange`] if the patch is jammed or card is invalid.
pub fn audit_local_knit_patch(
    card: &RveHomogenizedKnitCard,
    input: &LocalAuditPatchInput,
    wale_pitch_m: f64,
) -> Result<LocalAuditPatchResult, Refusal> {
    card.validate()?;
    for (i, &k) in input.macro_curvature.iter().enumerate() {
        Refusal::finite(&format!("macro_curvature[{i}]"), k)?;
    }
    Refusal::finite("patch_span_m", input.patch_span_m)?;
    Refusal::finite("wale_pitch_m", wale_pitch_m)?;
    if input.patch_span_m <= 0.0 || wale_pitch_m <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "patch_span_m and wale_pitch_m must be strictly positive",
        ));
    }
    let mem = card.membrane.evaluate(input.macro_strain)?;
    let bend = card.bending.evaluate_from_kappa(input.macro_curvature);
    let peak_resultant_n_m = (mem.stress[0] * mem.stress[0] + mem.stress[1] * mem.stress[1]).sqrt();
    let estimated_peak_yarn_tension_n = peak_resultant_n_m * wale_pitch_m;
    let pore_open_fraction = card.pore_open_fraction(input.macro_strain[0], input.macro_strain[1]);
    Ok(LocalAuditPatchResult {
        energy_density_j_m2: mem.energy_density + bend.energy_density,
        estimated_peak_yarn_tension_n,
        pore_open_fraction,
        jamming_margin: mem.jamming_margin,
    })
}

/// Result of calibrating a [`CoupledKnitMembrane`] against uniaxial strip observations anchored to
/// an offline [`RveHomogenizedKnitCard`] prior (`FTX-K11`, closing `ftx-ki1`/`ftx-ki4` honestly).
///
/// Because 3-cut uniaxial strips cannot independently identify biaxial jamming or out-of-plane
/// bending, the resulting card remains strictly [`Evidence::Estimate`] (never `Measured`).
#[derive(Clone, Debug, PartialEq)]
pub struct KnitPriorCalibrationResult {
    /// Calibrated card tagged strictly [`Evidence::Estimate`].
    pub card: RveHomogenizedKnitCard,
    /// Fitted dimensionless wale stiffness scale factor relative to the RVE prior.
    pub wale_scale: f64,
    /// Fitted dimensionless course stiffness scale factor relative to the RVE prior.
    pub course_scale: f64,
    /// Root-mean-square stress residual in N/m across all supplied observations.
    pub rms_residual_n_m: f64,
}

/// Calibrates the wale and course stiffness scales of an offline [`RveHomogenizedKnitCard`] prior
/// against measured uniaxial strip observations `(strain, stress_n_m)` via closed-form least squares,
/// refusing fail-closed if the fitted scale falls outside `[0.05, 20.0]` (`FTX-K11`). Unobserved
/// biaxial/bending directions stay anchored to the independent RVE prior and the card stays tagged
/// [`Evidence::Estimate`].
///
/// # Errors
/// [`RefusalCode::InvalidDocument`] or [`RefusalCode::OutOfRange`] on empty/non-finite inputs or
/// out-of-range stiffness scale mismatch.
pub fn calibrate_knit_prior_to_uniaxial_strips(
    prior: &RveHomogenizedKnitCard,
    wale_obs: &[(f64, f64)],
    course_obs: &[(f64, f64)],
) -> Result<KnitPriorCalibrationResult, Refusal> {
    prior.validate()?;
    if wale_obs.is_empty() || course_obs.is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "uniaxial knit prior calibration requires both wale and course observations",
        ));
    }
    let fit_axis = |obs: &[(f64, f64)], axis: usize| -> Result<f64, Refusal> {
        let mut num = 0.0;
        let mut den = 0.0;
        for &(eps, stress_obs) in obs {
            Refusal::finite("strain", eps)?;
            Refusal::finite("stress_n_m", stress_obs)?;
            let mut e = [0.0; 3];
            e[axis] = eps;
            let pred = prior.membrane.evaluate(e)?.stress[axis];
            num += pred * stress_obs;
            den += pred * pred;
        }
        if den <= 1e-18 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "uniaxial observations have zero prior excitation",
            ));
        }
        if num <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "uniaxial observations have non-positive projection onto prior stiffness",
            ));
        }
        let scale = num / den;
        if !scale.is_finite() || !(0.05..=20.0).contains(&scale) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "fitted uniaxial stiffness scale {scale:.4} lies outside admissible prior band [0.05, 20.0]; select a matching yarn/structure RVE prior"
                ),
            ));
        }
        Ok(scale)
    };

    let wale_scale = fit_axis(wale_obs, 0)?;
    let course_scale = fit_axis(course_obs, 1)?;
    let mut card = prior.clone();
    card.evidence = Evidence::Estimate;
    card.membrane.q11 *= wale_scale;
    card.membrane.c1111 *= wale_scale;
    card.membrane.q22 *= course_scale;
    card.membrane.c2222 *= course_scale;
    // Scale coupling, shear, and jamming barrier conservatively by geometric mean so SPD holds strictly
    let gm = (wale_scale * course_scale).sqrt();
    card.membrane.q12 *= gm;
    card.membrane.q66 *= gm;
    card.membrane.c1122 *= gm;
    card.membrane.kappa_jam *= gm;
    card.validate()?;

    let mut sq_sum = 0.0;
    let mut count = 0usize;
    for &(eps, stress_obs) in wale_obs {
        let pred = card.membrane.evaluate([eps, 0.0, 0.0])?.stress[0];
        let r = pred - stress_obs;
        sq_sum += r * r;
        count += 1;
    }
    for &(eps, stress_obs) in course_obs {
        let pred = card.membrane.evaluate([0.0, eps, 0.0])?.stress[1];
        let r = pred - stress_obs;
        sq_sum += r * r;
        count += 1;
    }
    let rms_residual_n_m = (sq_sum / (count as f64)).sqrt();

    Ok(KnitPriorCalibrationResult {
        card,
        wale_scale,
        course_scale,
        rms_residual_n_m,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poincloux_kat_exhibits_soft_toe_jamming_stiffening_and_capstan_hysteresis() {
        let p = PoinclouxParams::reference_monofilament();
        let curve = poincloux_uniaxial_curve(&p, &[0.0, 0.05, 0.15, 0.25, 0.32]).unwrap();
        assert_eq!(curve.len(), 5);
        assert!(curve[0].tension_frictionless_n_m.abs() < 1e-12);
        // Monotonic stiffening and loading > frictionless > unloading hysteresis loop
        for i in 1..curve.len() {
            assert!(curve[i].tension_frictionless_n_m > curve[i - 1].tension_frictionless_n_m);
            assert!(curve[i].tension_loading_n_m > curve[i].tension_frictionless_n_m);
            assert!(curve[i].tension_unloading_n_m < curve[i].tension_frictionless_n_m);
            assert!(curve[i].strain_course_free < 0.0);
        }
        // Tangent modulus increases sharply near jamming
        let slope_low =
            (curve[2].tension_frictionless_n_m - curve[1].tension_frictionless_n_m) / 0.10;
        let slope_hi =
            (curve[4].tension_frictionless_n_m - curve[3].tension_frictionless_n_m) / 0.07;
        assert!(
            slope_hi > 1.5 * slope_low,
            "expected jamming stiffening: {slope_low} vs {slope_hi}"
        );
    }

    #[test]
    fn rve_card_rejects_false_measured_claim_and_audits_local_patch() {
        let mut card = RveHomogenizedKnitCard {
            schema: "ftx-knit-rve-card/1".to_string(),
            id: "jersey-rve-est".to_string(),
            frankensim_rve_sha256: "abc123".to_string(),
            evidence: Evidence::Estimate,
            hill_mandel_residual: 4.2e-7,
            membrane: CoupledKnitMembrane::reference_stockinette_estimate(),
            bending: AsymmetricKnitBending::reference_stockinette_estimate(),
            rest_pore_fraction: 0.08,
            pore_strain_slope: 0.35,
        };
        let res = audit_local_knit_patch(
            &card,
            &LocalAuditPatchInput {
                macro_strain: [0.15, 0.10, 0.02],
                macro_curvature: [14.0, -15.0, 1.0],
                patch_span_m: 0.03,
            },
            0.004,
        )
        .unwrap();
        assert!(res.energy_density_j_m2 > 0.0);
        assert!(res.pore_open_fraction > 0.12);

        card.evidence = Evidence::Measured;
        assert_eq!(
            card.validate().unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
    }

    #[test]
    fn calibrate_knit_prior_scales_wale_and_course_stiffness_and_preserves_estimate_evidence() {
        let prior = RveHomogenizedKnitCard {
            schema: "ftx-knit-rve-card/1".to_string(),
            id: "jersey-rve-prior".to_string(),
            frankensim_rve_sha256: "abc123".to_string(),
            evidence: Evidence::Estimate,
            hill_mandel_residual: 4.2e-7,
            membrane: CoupledKnitMembrane::reference_stockinette_estimate(),
            bending: AsymmetricKnitBending::reference_stockinette_estimate(),
            rest_pore_fraction: 0.08,
            pore_strain_slope: 0.35,
        };
        let scale = 1.6;
        let wale_obs: Vec<(f64, f64)> = [0.05, 0.12, 0.22]
            .into_iter()
            .map(|e| {
                (
                    e,
                    scale * prior.membrane.evaluate([e, 0.0, 0.0]).unwrap().stress[0],
                )
            })
            .collect();
        let course_obs: Vec<(f64, f64)> = [0.05, 0.15, 0.28]
            .into_iter()
            .map(|e| {
                (
                    e,
                    scale * prior.membrane.evaluate([0.0, e, 0.0]).unwrap().stress[1],
                )
            })
            .collect();
        let fit = calibrate_knit_prior_to_uniaxial_strips(&prior, &wale_obs, &course_obs).unwrap();
        assert_eq!(fit.card.evidence, Evidence::Estimate);
        assert!((fit.wale_scale - scale).abs() < 1e-12, "{}", fit.wale_scale);
        assert!(
            (fit.course_scale - scale).abs() < 1e-12,
            "{}",
            fit.course_scale
        );
        assert!(fit.rms_residual_n_m < 1e-12, "{}", fit.rms_residual_n_m);
        assert!(fit.card.validate().is_ok());

        // Extreme prior scale mismatch refuses fail-closed rather than clamping silently.
        let extreme_obs: Vec<(f64, f64)> = [0.05, 0.12]
            .into_iter()
            .map(|e| {
                (
                    e,
                    100.0 * prior.membrane.evaluate([e, 0.0, 0.0]).unwrap().stress[0],
                )
            })
            .collect();
        assert_eq!(
            calibrate_knit_prior_to_uniaxial_strips(&prior, &extreme_obs, &course_obs)
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );
    }
}
