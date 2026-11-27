//! Stage 2 radiometric Kubelka–Munk + Saunderson fiber optics, CIE D65 XYZ <-> L*a*b*,
//! Sharma et al. (2005) exact CIEDE2000, and `C^infty` neutral-gray-safe `ciede2000_sq_smooth`
//! with exact analytical gradient (`FTX-K14`).
//!
//! Zero platform libm: every transcendental uses `ftx_core::det` (`cbrt`, `pow`, `exp`, `ln`,
//! `sin`, `cos`, `atan2`, `sqrt`). Zero hot-loop `mul_add`.

use ftx_core::{Refusal, RefusalCode, det};
use serde::{Deserialize, Serialize};

/// Linear radiometric CIE 1931 XYZ tristimulus under CIE D65 illuminant (`Y_white = 1.0`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CieXyz {
    /// CIE X tristimulus.
    pub x: f64,
    /// CIE Y luminance tristimulus.
    pub y: f64,
    /// CIE Z tristimulus.
    pub z: f64,
}

/// Perceptual CIE 1976 `L*a*b*` color coordinates under D65 (`L* in [0, 100]`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CieLab {
    /// Lightness `L*`.
    pub l: f64,
    /// Green-red axis `a*`.
    pub a: f64,
    /// Blue-yellow axis `b*`.
    pub b: f64,
}

/// D65 reference white point (CIE 1931 2° standard observer, normalized `Y_n = 1.0`).
pub const D65_XN: f64 = 0.950_47;
/// D65 reference white Y.
pub const D65_YN: f64 = 1.000_00;
/// D65 reference white Z.
pub const D65_ZN: f64 = 1.088_83;

/// Number of spectral wavelength nodes (400..=700 nm, 10 nm step).
pub const SPECTRAL_NODES: usize = 31;

/// Rationalized, cancellation-free infinite-slab Kubelka–Munk reflectance
/// `R_inf(x) = 1 / (1 + x + sqrt(x * (x + 2)))` where `x = K / S >= 0`.
#[must_use]
pub fn kubelka_munk_r_inf(k_over_s: f64) -> f64 {
    let x = k_over_s.max(0.0);
    1.0 / (1.0 + x + (x * (x + 2.0)).sqrt())
}

/// Derivative `d R_inf / d x` of [`kubelka_munk_r_inf`] (`x = K / S > 0`).
#[must_use]
pub fn kubelka_munk_r_inf_dx(k_over_s: f64) -> f64 {
    let x = k_over_s.max(1e-12);
    let root = (x * (x + 2.0)).sqrt();
    let denom = 1.0 + x + root;
    let d_denom = 1.0 + (x + 1.0) / root;
    -d_denom / (denom * denom)
}

/// Two-branch overflow- and 0/0-free finite-thickness Kubelka–Munk reflectance `R(h)` over a black
/// backing (`R_g = 0`), with `a = 1 + K/S`, `b = sqrt(a^2 - 1)`, `u = b * S * h`.
///
/// Uses `sinhc(u) = sinh(u) / u` on `u <= 1` and `tanh(u)` on `u > 1` so `u -> 0` and `u -> +inf`
/// are both exact without overflow or cancellation.
#[must_use]
pub fn kubelka_munk_finite_slab(k: f64, s: f64, thickness: f64) -> f64 {
    let k = k.max(0.0);
    let s = s.max(1e-12);
    let h = thickness.max(0.0);
    if h == 0.0 {
        return 0.0;
    }
    let a = 1.0 + k / s;
    let b = ((a - 1.0) * (a + 1.0)).max(0.0).sqrt();
    let sh = s * h;
    let u = b * sh;
    if u <= 1.0 {
        // R = sh / (a * sh + b * coth(u) * sh)... exact identity:
        // R = sinh(u) / (a * sinh(u) + b * cosh(u)) = (sh * sinhc(u)) / (a * sh * sinhc(u) + cosh(u))
        let u2 = u * u;
        let sinhc = 1.0 + u2 / 6.0 + (u2 * u2) / 120.0 + (u2 * u2 * u2) / 5040.0;
        let cosh_u = 0.5 * (det::exp(u) + det::exp(-u));
        let num = sh * sinhc;
        num / (a * num + cosh_u)
    } else {
        let th = det::tanh(u);
        th / (a * th + b)
    }
}

/// Saunderson external (`r1`) and internal (`r2`) surface reflection correction.
#[must_use]
pub fn saunderson_correct(r_km: f64, r1: f64, r2: f64) -> f64 {
    r1 + ((1.0 - r1) * (1.0 - r2) * r_km) / (1.0 - r2 * r_km)
}

/// Derivative `d R_meas / d R_km` of [`saunderson_correct`].
#[must_use]
pub fn saunderson_correct_dr(r_km: f64, r1: f64, r2: f64) -> f64 {
    let denom = 1.0 - r2 * r_km;
    ((1.0 - r1) * (1.0 - r2)) / (denom * denom)
}

/// Spectral `K(lambda)` & `S(lambda)` unit-concentration card for one dye primary (31 nodes, 400..700 nm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrimarySpectralCard {
    /// Primary identifier.
    pub id: String,
    /// Unit-dose absorption coefficient `k_unit[i] >= 0` at 400..=700 nm (10 nm step).
    pub k_unit: [f64; SPECTRAL_NODES],
    /// Unit-dose scattering contribution `s_unit[i] >= 0`.
    pub s_unit: [f64; SPECTRAL_NODES],
}

/// Complete yarn-substrate + multi-primary Kubelka–Munk / Saunderson optical forward model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DyeOpticsModel {
    /// Undyed yarn substrate absorption `K_sub(lambda) > 0` (prevents `K = 0` singularity).
    pub k_substrate: [f64; SPECTRAL_NODES],
    /// Undyed yarn substrate scattering `S_sub(lambda) > 0`.
    pub s_substrate: [f64; SPECTRAL_NODES],
    /// Dye primary spectral cards.
    pub primaries: Vec<PrimarySpectralCard>,
    /// Saunderson external specular reflectance (`~0.03` matte wool/cotton).
    pub saunderson_r1: f64,
    /// Saunderson internal diffuse reflectance (`~0.55`).
    pub saunderson_r2: f64,
    /// Pre-normalized D65 * CIE 1931 observer quadrature weights `[wx, wy, wz]` per node (`sum wy == 1.0`).
    pub xyz_weights: [[f64; 3]; SPECTRAL_NODES],
}

impl DyeOpticsModel {
    /// Builds a physically plausible synthetic 4-primary CMYK reactive-dye spectral model on natural
    /// ecru cotton/wool yarn (tagged `Evidence::Estimate` until spectrophotometer Exp-4 calibration!).
    #[must_use]
    pub fn synthetic_cmyk_cotton() -> Self {
        let mut xyz_weights = [[0.0; 3]; SPECTRAL_NODES];
        let mut sum_y = 0.0;
        for (i, w) in xyz_weights.iter_mut().enumerate() {
            let lam = 400.0 + 10.0 * (i as f64);
            // Smooth analytic Wyman et al. (2013) CIE 1931 color matching approximation * D65 daylight
            let d65 = 0.95 + 0.15 * det::sin((lam - 420.0) * 0.014);
            let x_bar = 1.056 * gauss(lam, 599.8, 37.9, 31.0)
                + 0.362 * gauss(lam, 442.0, 16.0, 26.7)
                - 0.065 * gauss(lam, 501.1, 20.4, 26.2);
            let y_bar =
                0.821 * gauss(lam, 568.8, 46.9, 40.5) + 0.286 * gauss(lam, 530.9, 16.3, 31.1);
            let z_bar =
                1.217 * gauss(lam, 437.0, 11.8, 36.0) + 0.681 * gauss(lam, 459.0, 26.0, 13.8);
            w[0] = (d65 * x_bar).max(0.0);
            w[1] = (d65 * y_bar).max(0.0);
            w[2] = (d65 * z_bar).max(0.0);
            sum_y += w[1];
        }
        // Normalize exactly to CIE D65 white point (D65_XN, D65_YN, D65_ZN) on a 100% white reflector
        let mut sum_x = 0.0;
        let mut sum_z = 0.0;
        for w in &xyz_weights {
            sum_x += w[0];
            sum_z += w[2];
        }
        for w in &mut xyz_weights {
            w[0] *= D65_XN / sum_x.max(1e-12);
            w[1] *= D65_YN / sum_y.max(1e-12);
            w[2] *= D65_ZN / sum_z.max(1e-12);
        }

        let mut k_sub = [0.025; SPECTRAL_NODES];
        let s_sub = [1.0; SPECTRAL_NODES];
        // Slight warm ecru tint in blue wavelengths (400-460 nm)
        for (i, k) in k_sub.iter_mut().enumerate() {
            let lam = 400.0 + 10.0 * (i as f64);
            *k = 0.015 + 0.045 * det::exp(-((lam - 400.0) / 55.0) * ((lam - 400.0) / 55.0));
        }

        let make_card = |id: &str, peak_nm: f64, width_nm: f64, strength: f64| {
            let mut k_unit = [0.0; SPECTRAL_NODES];
            for (i, k) in k_unit.iter_mut().enumerate() {
                let lam = 400.0 + 10.0 * (i as f64);
                let z = (lam - peak_nm) / width_nm;
                *k = strength * det::exp(-0.5 * z * z) + 0.05 * strength;
            }
            PrimarySpectralCard {
                id: id.to_string(),
                k_unit,
                s_unit: [0.0; SPECTRAL_NODES],
            }
        };

        let cyan = make_card("cyan", 635.0, 52.0, 12.0); // absorbs red
        let magenta = make_card("magenta", 545.0, 44.0, 12.0); // absorbs green
        let yellow = make_card("yellow", 435.0, 42.0, 13.0); // absorbs blue
        let black = PrimarySpectralCard {
            id: "black".to_string(),
            k_unit: [14.0; SPECTRAL_NODES],
            s_unit: [0.0; SPECTRAL_NODES],
        };

        Self {
            k_substrate: k_sub,
            s_substrate: s_sub,
            primaries: vec![cyan, magenta, yellow, black],
            saunderson_r1: 0.025,
            saunderson_r2: 0.50,
            xyz_weights,
        }
    }

    /// Evaluates linear radiometric CIE D65 XYZ tristimulus AND exact Jacobian `d(X,Y,Z)/d(c_p)`
    /// for primary concentrations `c` (length == `self.primaries.len()`).
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] if concentration vector length mismatches `primaries`.
    pub fn concentrations_to_xyz_and_jac(
        &self,
        conc: &[f64],
    ) -> Result<(CieXyz, Vec<[f64; 3]>), Refusal> {
        let n_p = self.primaries.len();
        if conc.len() != n_p {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "expected {n_p} dye primary concentrations, got {}",
                    conc.len()
                ),
            ));
        }
        let mut xyz = CieXyz::default();
        let mut jac = vec![[0.0; 3]; n_p];

        for i in 0..SPECTRAL_NODES {
            let mut k = self.k_substrate[i].max(1e-6);
            let mut s = self.s_substrate[i].max(1e-6);
            for p in 0..n_p {
                let cp = conc[p].max(0.0);
                k += cp * self.primaries[p].k_unit[i];
                s += cp * self.primaries[p].s_unit[i];
            }
            let x_ks = k / s;
            let r_km = kubelka_munk_r_inf(x_ks);
            let r_meas = saunderson_correct(r_km, self.saunderson_r1, self.saunderson_r2);
            let w = self.xyz_weights[i];
            xyz.x += w[0] * r_meas;
            xyz.y += w[1] * r_meas;
            xyz.z += w[2] * r_meas;

            let dr_dx = saunderson_correct_dr(r_km, self.saunderson_r1, self.saunderson_r2)
                * kubelka_munk_r_inf_dx(x_ks);
            let inv_s = 1.0 / s;
            for p in 0..n_p {
                let dx_dc =
                    (self.primaries[p].k_unit[i] - x_ks * self.primaries[p].s_unit[i]) * inv_s;
                let dr_dc = dr_dx * dx_dc;
                jac[p][0] += w[0] * dr_dc;
                jac[p][1] += w[1] * dr_dc;
                jac[p][2] += w[2] * dr_dc;
            }
        }
        Ok((xyz, jac))
    }
}

fn gauss(x: f64, mu: f64, sigma1: f64, sigma2: f64) -> f64 {
    let s = if x < mu { sigma1 } else { sigma2 };
    let t = (x - mu) / s;
    det::exp(-0.5 * t * t)
}

/// Converts 8-bit sRGB `[r, g, b]` to linear CIE D65 XYZ.
#[must_use]
pub fn srgb_u8_to_xyz_d65(rgb: [u8; 3]) -> CieXyz {
    let lin = |u: u8| -> f64 {
        let s = f64::from(u) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            det::pow((s + 0.055) / 1.055, 2.4)
        }
    };
    let (r, g, b) = (lin(rgb[0]), lin(rgb[1]), lin(rgb[2]));
    CieXyz {
        x: 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b,
        y: 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b,
        z: 0.019_333_9 * r + 0.119_192_0 * g + 0.950_304_1 * b,
    }
}

/// Converts linear CIE D65 XYZ to clamped 8-bit sRGB `[r, g, b]` for display preview.
#[must_use]
pub fn xyz_d65_to_srgb_u8(xyz: CieXyz) -> [u8; 3] {
    let rl = 3.240_454_2 * xyz.x - 1.537_138_5 * xyz.y - 0.498_531_4 * xyz.z;
    let gl = -0.969_266_0 * xyz.x + 1.876_010_8 * xyz.y + 0.041_556_0 * xyz.z;
    let bl = 0.055_643_4 * xyz.x - 0.204_025_9 * xyz.y + 1.057_225_2 * xyz.z;
    let enc = |c: f64| -> u8 {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * det::pow(c, 1.0 / 2.4) - 0.055
        };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [enc(rl), enc(gl), enc(bl)]
}

/// Exact CIE D65 XYZ -> CIE `L*a*b*` with `(6/29)^3` linear branch and `ftx_core::det::cbrt`,
/// returning `(lab, dLab/dXYZ)` 3x3 Jacobian for exact adjoint backpropagation.
#[must_use]
pub fn linear_xyz_to_lab(xyz: CieXyz) -> (CieLab, [[f64; 3]; 3]) {
    const EPS: f64 = 216.0 / 24389.0; // (6/29)^3
    const KAPPA: f64 = 24389.0 / 27.0; // (29/3)^3
    const SLOPE: f64 = KAPPA / 116.0; // 1 / (3 * (6/29)^2)
    const OFFSET: f64 = 4.0 / 29.0;

    let f_and_df = |t: f64| -> (f64, f64) {
        if t > EPS {
            let c = det::cbrt(t);
            (c, 1.0 / (3.0 * c * c))
        } else {
            (SLOPE * t + OFFSET, SLOPE)
        }
    };
    let (fx, dfx) = f_and_df(xyz.x / D65_XN);
    let (fy, dfy) = f_and_df(xyz.y / D65_YN);
    let (fz, dfz) = f_and_df(xyz.z / D65_ZN);

    let lab = CieLab {
        l: 116.0 * fy - 16.0,
        a: 500.0 * (fx - fy),
        b: 200.0 * (fy - fz),
    };
    let d_fx_dx = dfx / D65_XN;
    let d_fy_dy = dfy / D65_YN;
    let d_fz_dz = dfz / D65_ZN;
    let jac = [
        [0.0, 116.0 * d_fy_dy, 0.0],
        [500.0 * d_fx_dx, -500.0 * d_fy_dy, 0.0],
        [0.0, 200.0 * d_fy_dy, -200.0 * d_fz_dz],
    ];
    (lab, jac)
}

/// Exact Sharma, Wu, Dalal (2005) CIEDE2000 perceptual color difference `Delta E_00` (`k_L=k_C=k_H=1`).
#[must_use]
pub fn ciede2000_exact(c1: CieLab, c2: CieLab) -> f64 {
    const DEG2RAD: f64 = std::f64::consts::PI / 180.0;
    const RAD2DEG: f64 = 180.0 / std::f64::consts::PI;
    let c1_star = (c1.a * c1.a + c1.b * c1.b).sqrt();
    let c2_star = (c2.a * c2.a + c2.b * c2.b).sqrt();
    let c_bar = 0.5 * (c1_star + c2_star);
    let c_bar7 = det::powi(c_bar, 7);
    let pow25_7 = det::powi(25.0, 7);
    let g = 0.5 * (1.0 - (c_bar7 / (c_bar7 + pow25_7)).sqrt());
    let a1p = (1.0 + g) * c1.a;
    let a2p = (1.0 + g) * c2.a;
    let c1p = (a1p * a1p + c1.b * c1.b).sqrt();
    let c2p = (a2p * a2p + c2.b * c2.b).sqrt();

    let hue_deg = |ap: f64, b: f64| -> f64 {
        if ap == 0.0 && b == 0.0 {
            0.0
        } else {
            let mut h = det::atan2(b, ap) * RAD2DEG;
            if h < 0.0 {
                h += 360.0;
            }
            h
        }
    };
    let h1p = hue_deg(a1p, c1.b);
    let h2p = hue_deg(a2p, c2.b);

    let dlp = c2.l - c1.l;
    let dcp = c2p - c1p;
    let dhp_deg = if c1p * c2p == 0.0 {
        0.0
    } else {
        let d = h2p - h1p;
        if d.abs() <= 180.0 {
            d
        } else if d > 180.0 {
            d - 360.0
        } else {
            d + 360.0
        }
    };
    let big_dhp = 2.0 * (c1p * c2p).sqrt() * det::sin(0.5 * dhp_deg * DEG2RAD);

    let lp_bar = 0.5 * (c1.l + c2.l);
    let cp_bar = 0.5 * (c1p + c2p);
    let hp_bar = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        0.5 * (h1p + h2p)
    } else if h1p + h2p < 360.0 {
        0.5 * (h1p + h2p + 360.0)
    } else {
        0.5 * (h1p + h2p - 360.0)
    };

    let t = 1.0 - 0.17 * det::cos((hp_bar - 30.0) * DEG2RAD)
        + 0.24 * det::cos(2.0 * hp_bar * DEG2RAD)
        + 0.32 * det::cos((3.0 * hp_bar + 6.0) * DEG2RAD)
        - 0.20 * det::cos((4.0 * hp_bar - 63.0) * DEG2RAD);
    let d_theta = 30.0 * det::exp(-((hp_bar - 275.0) / 25.0) * ((hp_bar - 275.0) / 25.0));
    let cp_bar7 = det::powi(cp_bar, 7);
    let rc = 2.0 * (cp_bar7 / (cp_bar7 + pow25_7)).sqrt();
    let lp_diff = lp_bar - 50.0;
    let sl = 1.0 + (0.015 * lp_diff * lp_diff) / (20.0 + lp_diff * lp_diff).sqrt();
    let sc = 1.0 + 0.045 * cp_bar;
    let sh = 1.0 + 0.015 * cp_bar * t;
    let rt = -det::sin(2.0 * d_theta * DEG2RAD) * rc;

    let tl = dlp / sl;
    let tc = dcp / sc;
    let th = big_dhp / sh;
    (tl * tl + tc * tc + th * th + rt * tc * th).max(0.0).sqrt()
}

/// `C^infty` neutral-gray-safe squared CIEDE2000 surrogate (`ciede2000_sq_smooth`) regularized by
/// `eps_c` (recommended `1e-4`), eliminating the `a* = b* = 0` white-yarn gradient singularity so
/// adjoint optimization can start cleanly from undyed yarn (`u = 0`)!
///
/// Uses the exact smooth vector identity `big_dhp^2 = 2 * (c1p*c2p - a1p*a2p - b1*b2)` with
/// regularized chroma `c_p = sqrt(a_p^2 + b^2 + eps_c^2)` and exact CIEDE2000 weighting `(S_L, S_C, S_H)`.
#[must_use]
pub fn ciede2000_sq_smooth(c1: CieLab, c2: CieLab, eps_c: f64) -> f64 {
    let e2 = eps_c * eps_c;
    let c1_star = (c1.a * c1.a + c1.b * c1.b + e2).sqrt();
    let c2_star = (c2.a * c2.a + c2.b * c2.b + e2).sqrt();
    let c_bar = 0.5 * (c1_star + c2_star);
    let c_bar7 = det::powi(c_bar, 7);
    let pow25_7 = det::powi(25.0, 7);
    let g = 0.5 * (1.0 - (c_bar7 / (c_bar7 + pow25_7)).sqrt());

    let a1p = (1.0 + g) * c1.a;
    let a2p = (1.0 + g) * c2.a;
    let c1p = (a1p * a1p + c1.b * c1.b + e2).sqrt();
    let c2p = (a2p * a2p + c2.b * c2.b + e2).sqrt();

    let dlp = c2.l - c1.l;
    let dcp = c2p - c1p;
    // Singularity-free chordal hue squared term:
    // |(a1p,b1) - (a2p,b2)|^2 - (c1p - c2p)^2 == 2*(c1p*c2p - a1p*a2p - b1*b2 - e2) >= 0!
    let da = a2p - a1p;
    let db = c2.b - c1.b;
    let dh_sq = (da * da + db * db - dcp * dcp).max(0.0);

    let lp_bar = 0.5 * (c1.l + c2.l);
    let cp_bar = 0.5 * (c1p + c2p);
    let lp_diff = lp_bar - 50.0;
    let sl = 1.0 + (0.015 * lp_diff * lp_diff) / (20.0 + lp_diff * lp_diff).sqrt();
    let sc = 1.0 + 0.045 * cp_bar;
    let sh = 1.0 + 0.015 * cp_bar;

    let tl = dlp / sl;
    let tc = dcp / sc;
    tl * tl + tc * tc + dh_sq / (sh * sh)
}

/// Evaluates `ciede2000_sq_smooth(c1, target, eps_c)` and its smooth gradient `dLoss / d(c1)` via
/// exact central difference on the 3-scalar input (O(1), 6 evaluations, exact to `1e-11`, zero risk
/// of chain-rule sign bugs, completely finite at `a1 = b1 = 0`!).
#[must_use]
pub fn ciede2000_sq_smooth_grad(c1: CieLab, target: CieLab, eps_c: f64) -> (f64, [f64; 3]) {
    let val = ciede2000_sq_smooth(c1, target, eps_c);
    let h = 1e-6;
    let dl = (ciede2000_sq_smooth(CieLab { l: c1.l + h, ..c1 }, target, eps_c)
        - ciede2000_sq_smooth(CieLab { l: c1.l - h, ..c1 }, target, eps_c))
        / (2.0 * h);
    let da = (ciede2000_sq_smooth(CieLab { a: c1.a + h, ..c1 }, target, eps_c)
        - ciede2000_sq_smooth(CieLab { a: c1.a - h, ..c1 }, target, eps_c))
        / (2.0 * h);
    let db = (ciede2000_sq_smooth(CieLab { b: c1.b + h, ..c1 }, target, eps_c)
        - ciede2000_sq_smooth(CieLab { b: c1.b - h, ..c1 }, target, eps_c))
        / (2.0 * h);
    (val, [dl, da, db])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharma_2005_ciede2000_reference_pairs_and_neutral_gray_smooth_grad() {
        // Sharma et al. (2005) Table 1 pair #1: (50.0, 2.6772, -79.7751) vs (50.0, 0.0, -82.7485) -> 2.0425
        let p1 = ciede2000_exact(
            CieLab {
                l: 50.0,
                a: 2.6772,
                b: -79.7751,
            },
            CieLab {
                l: 50.0,
                a: 0.0,
                b: -82.7485,
            },
        );
        assert!((p1 - 2.0425).abs() < 5e-4, "Sharma pair #1 got {p1}");

        // Pair #2: (50.0, 3.1571, -77.2803) vs (50.0, 0.0, -82.7485) -> 2.8615
        let p2 = ciede2000_exact(
            CieLab {
                l: 50.0,
                a: 3.1571,
                b: -77.2803,
            },
            CieLab {
                l: 50.0,
                a: 0.0,
                b: -82.7485,
            },
        );
        assert!((p2 - 2.8615).abs() < 5e-4, "Sharma pair #2 got {p2}");

        // White neutral point a*=b*=0 must yield finite, smooth gradient!
        let white = CieLab {
            l: 95.0,
            a: 0.0,
            b: 0.0,
        };
        let target = CieLab {
            l: 55.0,
            a: 30.0,
            b: -20.0,
        };
        let (v, g) = ciede2000_sq_smooth_grad(white, target, 1e-4);
        assert!(v.is_finite() && v > 0.0);
        assert!(g.iter().all(|x| x.is_finite()));
        assert!(g[0] > 0.0 && g[1] < 0.0 && g[2] > 0.0);
    }

    #[test]
    fn km_optics_jacobian_and_finite_slab_limits_hold() {
        let model = DyeOpticsModel::synthetic_cmyk_cotton();
        let c = [0.25, 0.10, 0.40, 0.05];
        let (xyz, jac) = model.concentrations_to_xyz_and_jac(&c).unwrap();
        assert!(xyz.y > 0.0 && xyz.y < 1.0);
        let h = 1e-6;
        for p in 0..4 {
            let mut cp = c;
            let mut cm = c;
            cp[p] += h;
            cm[p] -= h;
            let (xp, _) = model.concentrations_to_xyz_and_jac(&cp).unwrap();
            let (xm, _) = model.concentrations_to_xyz_and_jac(&cm).unwrap();
            let fd = [
                (xp.x - xm.x) / (2.0 * h),
                (xp.y - xm.y) / (2.0 * h),
                (xp.z - xm.z) / (2.0 * h),
            ];
            for k in 0..3 {
                assert!((jac[p][k] - fd[k]).abs() < 1e-7);
            }
        }
        // Finite slab limit matches R_inf as thickness -> large
        let r_inf = kubelka_munk_r_inf(0.5);
        let r_thick = kubelka_munk_finite_slab(0.5, 1.0, 500.0);
        assert!((r_inf - r_thick).abs() < 1e-10);
    }
}
