//! First-principles moist-air thermophysics, validity-gated convective Nusselt correlations,
//! and N-patch gray-diffuse radiative enclosure exchange for textile microclimates.
//!
//! Provenance: adapted from FrankenSim (`frankensim-source-manifest.json` `source_root`
//! `f6c2ec6714e374fa3253654042d5ce3aed56e19dffc166f7e212617b226751b3`; MIT + OpenAI/Anthropic
//! rider):
//! - [`GasSpec`], [`MoistAirState`], [`saturation_pressure_water_pa`]:
//!   `crates/fs-material/src/gas.rs` (USSA-1976 dry air, Buck 1996 liquid-water saturation,
//!   NIST-JANAF water vapor, Sutherland viscosity, Eucken relation);
//! - [`CorrelationId`], [`CorrelationInputs`], [`NusseltResult`], [`evaluate_convection`]:
//!   `crates/fs-convection/src/lib.rs` (all 12 validity-gated Nusselt relations with explicit
//!   dimensionless bounds and bibliographic provenance);
//! - [`ViewFactorMatrix`], [`GrayDiffuseEnclosure`], [`RadiosityResult`]:
//!   `crates/fs-conduction/src/radiation.rs` (row-sum closure + area-weighted reciprocity
//!   admission and dense N-patch radiosity solve).
//!
//! Adaptations for FrankenTextiles:
//! - `#![forbid(unsafe_code)]`, zero platform `libm` (transcendentals via [`ftx_core::det`]),
//!   zero `mul_add` in solver loops;
//! - structured [`Refusal`] error model and checked adapter [`NusseltResult::to_exterior`]
//!   producing [`crate::thermal::steady::exterior::Exterior`] with full citation and explicit
//!   `Evidence::Estimate` status (correlation outputs are model estimates, never measured films).

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, det};
use ftx_pattern::materials::{Evidence, Source};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::thermal::steady::exterior::{AmbientRadiation, Exterior};

/// USSA-1976 universal gas constant `R*` in J/(mol·K).
pub const R_USSA_1976: f64 = 8.31432;

/// Stefan–Boltzmann constant (CODATA 2019 exact SI), W/(m²·K⁴).
pub const STEFAN_BOLTZMANN_W_M2_K4: f64 = 5.670_374_419e-8;

const MAX_ENCLOSURE_SURFACES: usize = 256;

#[inline]
fn det_pow(base: f64, exponent: f64) -> f64 {
    det::exp(exponent * det::ln(base))
}

fn refusal(code: RefusalCode, msg: impl Into<String>) -> Refusal {
    Refusal::new(code, msg)
}

// ---------------------------------------------------------------------------
// First-principles ideal dry and moist air (USSA-1976 + Buck 1996 + NIST)
// ---------------------------------------------------------------------------

/// How [`GasSpec`] derives thermal conductivity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConductivityModel {
    /// USSA-1976 empirical dry-air conductivity fit.
    Ussa1976AirFit,
    /// Kinetic-theory Eucken relation `κ = μ (c_p + 5/4 R/M)`.
    Eucken,
}

/// Calorically-perfect ideal-gas specification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GasSpec {
    /// Molar mass `M` [kg/mol].
    pub molar_mass: f64,
    /// Heat-capacity ratio `γ`.
    pub gamma: f64,
    /// Sutherland viscosity constant `β` [kg/(s·m·K^0.5)].
    pub sutherland_beta: f64,
    /// Sutherland reference temperature `S` [K].
    pub sutherland_s: f64,
    /// Thermal-conductivity model.
    pub conductivity: ConductivityModel,
}

impl GasSpec {
    /// U.S. Standard Atmosphere 1976 dry air.
    #[must_use]
    pub const fn dry_air_ussa1976() -> Self {
        Self {
            molar_mass: 28.9644e-3,
            gamma: 1.4,
            sutherland_beta: 1.458e-6,
            sutherland_s: 110.4,
            conductivity: ConductivityModel::Ussa1976AirFit,
        }
    }

    /// Water vapor (CODATA/IUPAC molar mass, NIST-JANAF `γ`, White steam Sutherland constants).
    #[must_use]
    pub const fn water_vapor_nist() -> Self {
        Self {
            molar_mass: 18.01528e-3,
            gamma: 1.3291,
            sutherland_beta: 2.418e-6,
            sutherland_s: 1064.0,
            conductivity: ConductivityModel::Eucken,
        }
    }
}

/// Buck (1996) saturation vapor pressure of liquid water [Pa] for `T ∈ [253.15, 323.15] K`
/// (`−20..+50 °C`).
///
/// # Errors
/// [`RefusalCode::OutOfRange`] outside `[253.15, 323.15] K` or for non-finite temperature.
pub fn saturation_pressure_water_pa(temperature_k: f64) -> Result<f64, Refusal> {
    if !(temperature_k.is_finite() && (253.15..=323.15).contains(&temperature_k)) {
        return Err(refusal(
            RefusalCode::OutOfRange,
            format!(
                "temperature {temperature_k} K is outside Buck-1996 liquid-water window [253.15, 323.15] K"
            ),
        ));
    }
    let t = temperature_k - 273.15;
    Ok(611.21 * det::exp((18.678 - t / 234.5) * t / (257.14 + t)))
}

/// First-principles ambient gas/air thermophysical state derived from `(T, p, RH)`.
///
/// Prandtl number `Pr = μ c_p / κ` is DERIVED, never hardcoded. No-claim: for moist air,
/// transport coefficients `(μ, κ)` retain the dry-air fits (valid to ~1–2% up to `x_w ≤ 0.15`,
/// refused above) and supersaturation (`RH > 1`) is refused.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct MoistAirState {
    /// Absolute temperature [K].
    pub temperature_k: f64,
    /// Ambient pressure [Pa].
    pub pressure_pa: f64,
    /// Density `ρ` [kg/m³].
    pub density: f64,
    /// Speed of sound `c` [m/s].
    pub sound_speed: f64,
    /// Dynamic viscosity `μ` [Pa·s].
    pub dynamic_viscosity: f64,
    /// Kinematic viscosity `ν = μ / ρ` [m²/s].
    pub kinematic_viscosity: f64,
    /// Thermal conductivity `κ` [W/(m·K)].
    pub thermal_conductivity: f64,
    /// Thermal diffusivity `α = κ / (ρ c_p)` [m²/s].
    pub thermal_diffusivity: f64,
    /// Heat-capacity ratio `γ`.
    pub gamma: f64,
    /// Specific gas constant `R/M` [J/(kg·K)].
    pub specific_gas_constant: f64,
    /// Isobaric specific heat `c_p` [J/(kg·K)].
    pub specific_heat_cp: f64,
    /// Derived Prandtl number `Pr = μ c_p / κ`.
    pub prandtl: f64,
    /// Water-vapor mole fraction `x_w` (`0.0` for dry gas).
    pub water_mole_fraction: f64,
}

impl MoistAirState {
    /// Derive state for a single dry gas in `T ∈ [50, 2000] K`, `p ∈ (0, 1e7] Pa`.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] for non-physical spec or out-of-window `(T, p)`.
    pub fn try_new(spec: &GasSpec, temperature_k: f64, pressure_pa: f64) -> Result<Self, Refusal> {
        for (v, name) in [
            (spec.molar_mass, "molar mass"),
            (spec.sutherland_beta, "Sutherland beta"),
            (spec.sutherland_s, "Sutherland S"),
        ] {
            if !(v.is_finite() && v > 0.0) {
                return Err(refusal(
                    RefusalCode::OutOfRange,
                    format!("{name} must be finite and positive"),
                ));
            }
        }
        if !(spec.gamma.is_finite() && spec.gamma > 1.0 && spec.gamma < 5.0 / 3.0 + 1e-9) {
            return Err(refusal(
                RefusalCode::OutOfRange,
                format!("gamma {} outside (1, 5/3]", spec.gamma),
            ));
        }
        if !(temperature_k.is_finite() && (50.0..=2000.0).contains(&temperature_k)) {
            return Err(refusal(
                RefusalCode::OutOfRange,
                format!("temperature {temperature_k} K outside [50, 2000] K"),
            ));
        }
        if !(pressure_pa.is_finite() && pressure_pa > 0.0 && pressure_pa <= 1.0e7) {
            return Err(refusal(
                RefusalCode::OutOfRange,
                format!("pressure {pressure_pa} Pa outside (0, 1e7] Pa"),
            ));
        }
        let r_spec = R_USSA_1976 / spec.molar_mass;
        let density = pressure_pa / (r_spec * temperature_k);
        let sound_speed = (spec.gamma * r_spec * temperature_k).sqrt();
        let t32 = temperature_k * temperature_k.sqrt();
        let dynamic_viscosity = spec.sutherland_beta * t32 / (temperature_k + spec.sutherland_s);
        let specific_heat_cp = spec.gamma * r_spec / (spec.gamma - 1.0);
        let thermal_conductivity = match spec.conductivity {
            ConductivityModel::Ussa1976AirFit => {
                let ten_pow = det::exp((-12.0 / temperature_k) * std::f64::consts::LN_10);
                2.64638e-3 * t32 / (temperature_k + 245.4 * ten_pow)
            }
            ConductivityModel::Eucken => dynamic_viscosity * (specific_heat_cp + 1.25 * r_spec),
        };
        let prandtl = dynamic_viscosity * specific_heat_cp / thermal_conductivity;
        let kinematic_viscosity = dynamic_viscosity / density;
        let thermal_diffusivity = thermal_conductivity / (density * specific_heat_cp);
        Ok(Self {
            temperature_k,
            pressure_pa,
            density,
            sound_speed,
            dynamic_viscosity,
            kinematic_viscosity,
            thermal_conductivity,
            thermal_diffusivity,
            gamma: spec.gamma,
            specific_gas_constant: r_spec,
            specific_heat_cp,
            prandtl,
            water_mole_fraction: 0.0,
        })
    }

    /// Derive moist air from `(temperature_k, pressure_pa, relative_humidity ∈ [0, 1])`.
    /// `relative_humidity == 0.0` runs the identical dry-air path bit-for-bit.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] for `RH ∉ [0, 1]`, temperature outside Buck's window when
    /// `RH > 0`, vapor mole fraction `x_w > 0.15`, or invalid `(T, p)`.
    pub fn moist_air(
        temperature_k: f64,
        pressure_pa: f64,
        relative_humidity: f64,
    ) -> Result<Self, Refusal> {
        if !(relative_humidity.is_finite() && (0.0..=1.0).contains(&relative_humidity)) {
            return Err(refusal(
                RefusalCode::OutOfRange,
                format!("relative humidity {relative_humidity} outside [0, 1]"),
            ));
        }
        let dry = GasSpec::dry_air_ussa1976();
        if relative_humidity == 0.0 {
            return Self::try_new(&dry, temperature_k, pressure_pa);
        }
        let vapor = GasSpec::water_vapor_nist();
        let e_sat = saturation_pressure_water_pa(temperature_k)?;
        if !(pressure_pa.is_finite() && pressure_pa > 0.0) {
            return Err(refusal(
                RefusalCode::OutOfRange,
                format!("pressure {pressure_pa} Pa must be finite and positive"),
            ));
        }
        let x_w = relative_humidity * e_sat / pressure_pa;
        if x_w > 0.15 {
            return Err(refusal(
                RefusalCode::OutOfRange,
                format!("water mole fraction {x_w:.4} exceeds 0.15 dry-transport validity ceiling"),
            ));
        }
        let molar_mass = dry.molar_mass + x_w * (vapor.molar_mass - dry.molar_mass);
        let inv_gm1 = (1.0 - x_w) / (dry.gamma - 1.0) + x_w / (vapor.gamma - 1.0);
        let gamma = 1.0 + 1.0 / inv_gm1;
        let mixture = GasSpec {
            molar_mass,
            gamma,
            ..dry
        };
        let mut state = Self::try_new(&mixture, temperature_k, pressure_pa)?;
        state.water_mole_fraction = x_w;
        Ok(state)
    }

    /// Reynolds number `Re = U L / ν` for characteristic wind/air speed `speed_m_s` and length `L`.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] unless speed and length are finite and positive.
    pub fn reynolds(&self, speed_m_s: f64, length_m: f64) -> Result<f64, Refusal> {
        if !(speed_m_s.is_finite() && speed_m_s > 0.0 && length_m.is_finite() && length_m > 0.0) {
            return Err(refusal(
                RefusalCode::OutOfRange,
                "Reynolds number requires finite positive speed and characteristic length",
            ));
        }
        let re = speed_m_s * length_m / self.kinematic_viscosity;
        if !(re.is_finite() && re > 0.0) {
            return Err(refusal(
                RefusalCode::NonFinite,
                "Reynolds number overflowed or underflowed",
            ));
        }
        Ok(re)
    }

    /// Rayleigh number `Ra = g β |ΔT| L³ / (ν α)` for ideal-gas thermal expansion `β = 1/T_film`.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] unless `delta_t_k`, `length_m`, and `gravity_m_s2` are finite
    /// and strictly positive.
    pub fn rayleigh(
        &self,
        delta_t_k: f64,
        length_m: f64,
        gravity_m_s2: f64,
    ) -> Result<f64, Refusal> {
        if !(delta_t_k.is_finite()
            && delta_t_k > 0.0
            && length_m.is_finite()
            && length_m > 0.0
            && gravity_m_s2.is_finite()
            && gravity_m_s2 > 0.0)
        {
            return Err(refusal(
                RefusalCode::OutOfRange,
                "Rayleigh number requires finite positive |ΔT|, length, and gravity",
            ));
        }
        let beta = 1.0 / self.temperature_k;
        let l3 = length_m * length_m * length_m;
        let ra = gravity_m_s2 * beta * delta_t_k * l3
            / (self.kinematic_viscosity * self.thermal_diffusivity);
        if !(ra.is_finite() && ra > 0.0) {
            return Err(refusal(
                RefusalCode::NonFinite,
                "Rayleigh number overflowed or underflowed",
            ));
        }
        Ok(ra)
    }
}

// ---------------------------------------------------------------------------
// Validity-gated convective Nusselt correlations (12 standard formulas)
// ---------------------------------------------------------------------------

/// Stable identifier for the 12 validity-gated Nusselt convection correlations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrelationId {
    /// Circular duct, fully developed laminar, constant wall temperature (`Nu = 3.66`).
    CircularDuctLaminarCwt,
    /// Circular duct, fully developed laminar, constant wall heat flux (`Nu = 4.36`).
    CircularDuctLaminarChf,
    /// Circular duct, thermally developing laminar flow (Hausen).
    CircularDuctHausen,
    /// Rectangular duct, fully developed laminar, constant wall temperature (Shah–London).
    RectangularDuctLaminarCwt,
    /// Rectangular duct, fully developed laminar, constant wall heat flux (Shah–London).
    RectangularDuctLaminarChf,
    /// Rectangular duct, simultaneously developing laminar flow (`a = 0.5, Pr = 0.72`, Shah–London Table 52).
    RectangularDuctLaminarCwtDevelopingPr072,
    /// Smooth turbulent tube (Dittus–Boelter).
    DittusBoelter,
    /// Smooth turbulent tube (Gnielinski).
    Gnielinski,
    /// Isothermal laminar flat plate, average `Nu_L` (Pohlhausen).
    FlatPlateLaminarAverage,
    /// Isothermal mixed laminar-to-turbulent flat plate, average `Nu_L`.
    FlatPlateTurbulentAverage,
    /// Circular cylinder in crossflow (Churchill–Bernstein — torso/limb in crosswind).
    ChurchillBernsteinCylinder,
    /// Isothermal vertical plate in natural convection (Churchill–Chu — still-air garment panel).
    ChurchillChuVerticalPlate,
}

impl CorrelationId {
    /// Complete catalog in stable audit order.
    pub const ALL: [Self; 12] = [
        Self::CircularDuctLaminarCwt,
        Self::CircularDuctLaminarChf,
        Self::CircularDuctHausen,
        Self::RectangularDuctLaminarCwt,
        Self::RectangularDuctLaminarChf,
        Self::RectangularDuctLaminarCwtDevelopingPr072,
        Self::DittusBoelter,
        Self::Gnielinski,
        Self::FlatPlateLaminarAverage,
        Self::FlatPlateTurbulentAverage,
        Self::ChurchillBernsteinCylinder,
        Self::ChurchillChuVerticalPlate,
    ];

    /// Stable identifier string.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CircularDuctLaminarCwt => "convection.circular-duct-laminar-cwt",
            Self::CircularDuctLaminarChf => "convection.circular-duct-laminar-chf",
            Self::CircularDuctHausen => "convection.circular-duct-hausen-developing",
            Self::RectangularDuctLaminarCwt => "convection.rectangular-duct-laminar-cwt",
            Self::RectangularDuctLaminarChf => "convection.rectangular-duct-laminar-chf",
            Self::RectangularDuctLaminarCwtDevelopingPr072 => {
                "convection.rectangular-duct-laminar-cwt-developing-pr072"
            }
            Self::DittusBoelter => "convection.dittus-boelter",
            Self::Gnielinski => "convection.gnielinski",
            Self::FlatPlateLaminarAverage => "convection.flat-plate-laminar-average",
            Self::FlatPlateTurbulentAverage => "convection.flat-plate-turbulent-average",
            Self::ChurchillBernsteinCylinder => "convection.churchill-bernstein-cylinder",
            Self::ChurchillChuVerticalPlate => "convection.churchill-chu-vertical-plate",
        }
    }

    /// Bibliographic citation for provenance receipts.
    #[must_use]
    pub const fn citation(self) -> &'static str {
        match self {
            Self::CircularDuctLaminarCwt
            | Self::CircularDuctLaminarChf
            | Self::CircularDuctHausen
            | Self::RectangularDuctLaminarCwt
            | Self::RectangularDuctLaminarChf => {
                "R. K. Shah and A. L. London, Laminar Flow Forced Convection in Ducts, Academic Press, 1978 (ISBN 978-0-12-020051-1)"
            }
            Self::RectangularDuctLaminarCwtDevelopingPr072 => {
                "R. K. Shah and A. L. London, Laminar Flow Forced Convection in Ducts, Academic Press, 1978, Ch. VII Table 52 (a=0.5, Pr=0.72)"
            }
            Self::DittusBoelter => {
                "F. W. Dittus and L. M. K. Boelter, Heat Transfer in Automobile Radiators of the Tubular Type, Univ. Calif. Publ. Eng. 2(13), 1930, 443-461"
            }
            Self::Gnielinski => {
                "V. Gnielinski, New Equations for Heat and Mass Transfer in Turbulent Pipe and Channel Flow, Int. Chem. Eng. 16(2), 1976, 359-368"
            }
            Self::FlatPlateLaminarAverage => {
                "E. Pohlhausen, Der Waermeaustausch zwischen festen Koerpern und Fluessigkeiten mit kleiner Reibung und kleiner Waermeleitung, ZAMM 1, 1921, 115-121"
            }
            Self::FlatPlateTurbulentAverage => {
                "H. Lee, Thermal Design: Heat Sinks, Thermoelectrics, Heat Pipes, Compact Heat Exchangers, and Solar Cells, 2nd ed., Wiley, 2022, Eq. (1.62)"
            }
            Self::ChurchillBernsteinCylinder => {
                "S. W. Churchill and M. Bernstein, A Correlating Equation for Forced Convection from Gases and Liquids to a Circular Cylinder in Crossflow, J. Heat Transfer 99(2), 1977, 300-306"
            }
            Self::ChurchillChuVerticalPlate => {
                "S. W. Churchill and H. H. S. Chu, Correlating Equations for Laminar and Turbulent Free Convection from a Vertical Plate, Int. J. Heat Mass Transfer 18, 1975, 1323-1329"
            }
        }
    }

    /// Relative model-form discrepancy allowance (0.0 only for analytic ideal limits).
    #[must_use]
    pub const fn discrepancy_rel(self) -> f64 {
        match self {
            Self::CircularDuctLaminarCwt
            | Self::CircularDuctLaminarChf
            | Self::RectangularDuctLaminarCwt
            | Self::RectangularDuctLaminarChf => 0.0,
            Self::Gnielinski | Self::FlatPlateLaminarAverage => 0.10,
            Self::CircularDuctHausen
            | Self::RectangularDuctLaminarCwtDevelopingPr072
            | Self::FlatPlateTurbulentAverage => 0.15,
            Self::ChurchillBernsteinCylinder | Self::ChurchillChuVerticalPlate => 0.20,
            Self::DittusBoelter => 0.25,
        }
    }

    /// Inclusive validity bounds `(axis, low, high)` checked before evaluation.
    #[must_use]
    pub const fn validity_bounds(self) -> &'static [(&'static str, f64, f64)] {
        match self {
            Self::CircularDuctLaminarCwt | Self::CircularDuctLaminarChf => &[
                ("L_over_Dh", 50.0, 1.0e6),
                ("Pr", 0.6, 1_000.0),
                ("Re", 1.0, 2_300.0),
            ],
            Self::CircularDuctHausen => &[
                ("L_over_Dh", 0.05, 1_000.0),
                ("Pr", 0.6, 1_000.0),
                ("Re", 1.0, 2_300.0),
            ],
            Self::RectangularDuctLaminarCwt | Self::RectangularDuctLaminarChf => &[
                ("L_over_Dh", 50.0, 1.0e6),
                ("Pr", 0.6, 1_000.0),
                ("Re", 1.0, 2_300.0),
                ("aspect_ratio", 0.001, 1.0),
            ],
            Self::RectangularDuctLaminarCwtDevelopingPr072 => &[
                ("Gz", 0.01, 220.0),
                ("Pr", 0.72, 0.72),
                ("Re", 1.0, 2_300.0),
                ("aspect_ratio", 0.5, 0.5),
            ],
            Self::DittusBoelter => &[
                ("L_over_Dh", 10.0, 1.0e6),
                ("Pr", 0.7, 120.0),
                ("Re", 10_000.0, 120_000.0),
            ],
            Self::Gnielinski => &[
                ("L_over_Dh", 10.0, 1.0e6),
                ("Pr", 0.5, 2_000.0),
                ("Re", 3_000.0, 5.0e6),
            ],
            Self::FlatPlateLaminarAverage => &[("Pr", 0.6, 50.0), ("Re", 1.0, 5.0e5)],
            Self::FlatPlateTurbulentAverage => &[("Pr", 0.6, 60.0), ("Re", 5.0e5, 1.0e7)],
            Self::ChurchillBernsteinCylinder => &[
                ("Pe", 0.2, 1.0e10),
                ("Pr", 0.2, 1_000.0),
                ("Re", 1.0, 1.0e7),
            ],
            Self::ChurchillChuVerticalPlate => &[("Pr", 0.01, 1.0e5), ("Ra", 0.1, 1.0e12)],
        }
    }
}

/// Heat-flow direction for Dittus–Boelter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThermalDirection {
    /// Wall heats the fluid (`n = 0.4`).
    #[default]
    HeatingFluid,
    /// Wall cools the fluid (`n = 0.3`).
    CoolingFluid,
}

/// Explicit dimensionless inputs for [`evaluate_convection`]; omitted axes are never guessed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorrelationInputs {
    /// Reynolds number `Re`.
    pub reynolds: Option<f64>,
    /// Prandtl number `Pr`.
    pub prandtl: Option<f64>,
    /// Duct length-to-hydraulic-diameter ratio `L / D_h`.
    pub length_over_hydraulic_diameter: Option<f64>,
    /// Rectangular duct minor/major aspect ratio `a ∈ (0, 1]`.
    pub aspect_ratio: Option<f64>,
    /// Rayleigh number `Ra`.
    pub rayleigh: Option<f64>,
    /// Heat-flow direction (Dittus–Boelter).
    pub direction: ThermalDirection,
}

impl CorrelationInputs {
    /// Forced-convection inputs `(Re, Pr)`.
    #[must_use]
    pub const fn forced(reynolds: f64, prandtl: f64) -> Self {
        Self {
            reynolds: Some(reynolds),
            prandtl: Some(prandtl),
            length_over_hydraulic_diameter: None,
            aspect_ratio: None,
            rayleigh: None,
            direction: ThermalDirection::HeatingFluid,
        }
    }

    /// Natural-convection inputs `(Ra, Pr)`.
    #[must_use]
    pub const fn natural(rayleigh: f64, prandtl: f64) -> Self {
        Self {
            reynolds: None,
            prandtl: Some(prandtl),
            length_over_hydraulic_diameter: None,
            aspect_ratio: None,
            rayleigh: Some(rayleigh),
            direction: ThermalDirection::HeatingFluid,
        }
    }

    /// Attach `L / D_h`.
    #[must_use]
    pub const fn with_length_ratio(mut self, ratio: f64) -> Self {
        self.length_over_hydraulic_diameter = Some(ratio);
        self
    }

    /// Attach rectangular-duct minor/major aspect ratio.
    #[must_use]
    pub const fn with_aspect_ratio(mut self, ratio: f64) -> Self {
        self.aspect_ratio = Some(ratio);
        self
    }

    /// Set heating/cooling direction (Dittus–Boelter).
    #[must_use]
    pub const fn with_direction(mut self, direction: ThermalDirection) -> Self {
        self.direction = direction;
        self
    }

    fn groups(self) -> Result<BTreeMap<&'static str, f64>, Refusal> {
        let mut g = BTreeMap::new();
        let mut insert = |name: &'static str, v: f64| -> Result<(), Refusal> {
            if !(v.is_finite() && v > 0.0) {
                return Err(refusal(
                    RefusalCode::OutOfRange,
                    format!("dimensionless group {name} = {v} must be finite and positive"),
                ));
            }
            g.insert(name, v);
            Ok(())
        };
        if let Some(v) = self.reynolds {
            insert("Re", v)?;
        }
        if let Some(v) = self.prandtl {
            insert("Pr", v)?;
        }
        if let Some(v) = self.length_over_hydraulic_diameter {
            insert("L_over_Dh", v)?;
        }
        if let Some(v) = self.aspect_ratio {
            insert("aspect_ratio", v)?;
        }
        if let Some(v) = self.rayleigh {
            insert("Ra", v)?;
        }
        if let (Some(re), Some(pr)) = (self.reynolds, self.prandtl) {
            insert("Pe", re * pr)?;
        }
        if let (Some(re), Some(pr), Some(lr)) = (
            self.reynolds,
            self.prandtl,
            self.length_over_hydraulic_diameter,
        ) {
            insert("Gz", re * pr / lr)?;
        }
        Ok(g)
    }
}

/// Admitted Nusselt correlation result with explicit provenance and discrepancy allowance.
#[derive(Debug, Clone, PartialEq)]
pub struct NusseltResult {
    /// Correlation evaluated.
    pub id: CorrelationId,
    /// Dimensionless Nusselt number `Nu`.
    pub nusselt: f64,
    /// Relative model-form discrepancy allowance.
    pub discrepancy_rel: f64,
    /// Exact evaluated dimensionless groups.
    pub groups: BTreeMap<&'static str, f64>,
}

impl NusseltResult {
    /// Convert `Nu` to convective film coefficient `h = Nu · k / L` [W/(m²·K)].
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] on non-positive or non-finite inputs/output.
    pub fn heat_transfer_coefficient(
        &self,
        thermal_conductivity_w_m_k: f64,
        characteristic_length_m: f64,
    ) -> Result<f64, Refusal> {
        if !(thermal_conductivity_w_m_k.is_finite()
            && thermal_conductivity_w_m_k > 0.0
            && characteristic_length_m.is_finite()
            && characteristic_length_m > 0.0)
        {
            return Err(refusal(
                RefusalCode::OutOfRange,
                "Nu-to-h conversion requires finite positive conductivity and length",
            ));
        }
        let h = self.nusselt * thermal_conductivity_w_m_k / characteristic_length_m;
        if !(h.is_finite() && h > 0.0) {
            return Err(refusal(
                RefusalCode::NonFinite,
                "convective heat-transfer coefficient overflowed",
            ));
        }
        Ok(h)
    }

    /// Lower this correlation evaluation + ambient [`MoistAirState`] into a qualified
    /// [`Exterior`] boundary for [`crate::thermal::steady::SteadyConductionModel::solve_exterior`].
    ///
    /// Marks the resulting film coefficient as [`Evidence::Estimate`] (empirical correlations are
    /// model estimates, never measured garment films), so the caller must explicitly pass
    /// `allow_estimates = true` to admit it into a steady solve.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] on invalid characteristic length, temperature range, or
    /// non-positive film coefficient.
    pub fn to_exterior(
        &self,
        air: &MoistAirState,
        characteristic_length_m: f64,
        surface_temperature_range_c: [f64; 2],
        allow_estimates: bool,
        radiation: Option<AmbientRadiation>,
    ) -> Result<Exterior, Refusal> {
        let [t_lo, t_hi] = surface_temperature_range_c;
        if !(t_lo.is_finite()
            && t_hi.is_finite()
            && t_lo > -273.15
            && t_hi >= t_lo
            && (t_hi - t_lo).is_finite())
        {
            return Err(refusal(
                RefusalCode::OutOfRange,
                "exterior surface temperature range must be finite, > -273.15 C, and ordered",
            ));
        }
        let h =
            self.heat_transfer_coefficient(air.thermal_conductivity, characteristic_length_m)?;
        let air_c = air.temperature_k - 273.15;
        let groups_summary: Vec<String> = self
            .groups
            .iter()
            .map(|(k, v)| format!("{k}={v:.4e}"))
            .collect();
        let exterior = Exterior {
            air_c,
            convection_w_m2_k: h,
            surface_temperature_range_c,
            convection_source: Source {
                reference: self.id.citation().to_string(),
                accessed_on: "2026-11-19".to_string(),
                note: format!(
                    "Validity-gated {} correlation (discrepancy allowance {:.0}%): model estimate, not a measured garment film",
                    self.id.name(),
                    self.discrepancy_rel * 100.0
                ),
            },
            convection_evidence: Evidence::Estimate,
            convection_conditions: format!(
                "correlation={}; {}; L={characteristic_length_m:.4e} m; T_air={air_c:.2} C; p={:.0} Pa; x_w={:.5}; k_air={:.5} W/(m*K)",
                self.id.name(),
                groups_summary.join(", "),
                air.pressure_pa,
                air.water_mole_fraction,
                air.thermal_conductivity
            ),
            allow_estimates,
            radiation,
        };
        if allow_estimates {
            exterior.validate_convection()?;
        }
        Ok(exterior)
    }
}

/// Evaluate one of the 12 validity-gated convection correlations, refusing missing or
/// out-of-domain dimensionless groups fail-closed.
///
/// # Errors
/// [`RefusalCode::OutOfRange`] on missing/invalid/out-of-domain dimensionless groups;
/// [`RefusalCode::NonFinite`] if the formula produces a non-positive or non-finite Nusselt number.
pub fn evaluate_convection(
    id: CorrelationId,
    inputs: CorrelationInputs,
) -> Result<NusseltResult, Refusal> {
    let groups = inputs.groups()?;
    let mut violations = Vec::new();
    for &(axis, lo, hi) in id.validity_bounds() {
        match groups.get(axis).copied() {
            Some(v) if v.is_finite() && v >= lo && v <= hi => {}
            Some(v) => violations.push(format!("{axis}={v} not in [{lo}, {hi}]")),
            None => violations.push(format!("{axis} missing (required in [{lo}, {hi}])")),
        }
    }
    if !violations.is_empty() {
        return Err(refusal(
            RefusalCode::OutOfRange,
            format!(
                "{} refused outside validity domain: {}",
                id.name(),
                violations.join("; ")
            ),
        ));
    }
    let nu = match id {
        CorrelationId::CircularDuctLaminarCwt => 3.66,
        CorrelationId::CircularDuctLaminarChf => 4.36,
        CorrelationId::CircularDuctHausen => {
            let gz = groups["Gz"];
            3.66 + 0.0668 * gz / (1.0 + 0.04 * det_pow(gz, 2.0 / 3.0))
        }
        CorrelationId::RectangularDuctLaminarCwt => rectangular_laminar_cwt(groups["aspect_ratio"]),
        CorrelationId::RectangularDuctLaminarChf => rectangular_laminar_chf(groups["aspect_ratio"]),
        CorrelationId::RectangularDuctLaminarCwtDevelopingPr072 => {
            rectangular_laminar_cwt_developing_pr072(groups["Gz"])
        }
        CorrelationId::DittusBoelter => {
            let exp_pr = match inputs.direction {
                ThermalDirection::HeatingFluid => 0.4,
                ThermalDirection::CoolingFluid => 0.3,
            };
            0.023 * det_pow(groups["Re"], 0.8) * det_pow(groups["Pr"], exp_pr)
        }
        CorrelationId::Gnielinski => {
            let re = groups["Re"];
            let pr = groups["Pr"];
            let denom_f = 0.79 * det::ln(re) - 1.64;
            let f = 1.0 / (denom_f * denom_f);
            let f8 = f / 8.0;
            let num = f8 * (re - 1_000.0) * pr;
            let den = 1.0 + 12.7 * f8.sqrt() * (det_pow(pr, 2.0 / 3.0) - 1.0);
            num / den
        }
        CorrelationId::FlatPlateLaminarAverage => {
            0.664 * groups["Re"].sqrt() * det_pow(groups["Pr"], 1.0 / 3.0)
        }
        CorrelationId::FlatPlateTurbulentAverage => {
            (0.037 * det_pow(groups["Re"], 0.8) - 871.0) * det_pow(groups["Pr"], 1.0 / 3.0)
        }
        CorrelationId::ChurchillBernsteinCylinder => {
            let re = groups["Re"];
            let pr = groups["Pr"];
            let num = 0.62 * re.sqrt() * det_pow(pr, 1.0 / 3.0);
            let pr_den = det_pow(1.0 + det_pow(0.4 / pr, 2.0 / 3.0), 0.25);
            let re_fac = det_pow(1.0 + det_pow(re / 282_000.0, 5.0 / 8.0), 4.0 / 5.0);
            0.3 + (num / pr_den) * re_fac
        }
        CorrelationId::ChurchillChuVerticalPlate => {
            let ra = groups["Ra"];
            let pr = groups["Pr"];
            let den = det_pow(1.0 + det_pow(0.492 / pr, 9.0 / 16.0), 8.0 / 27.0);
            let term = 0.825 + 0.387 * det_pow(ra, 1.0 / 6.0) / den;
            term * term
        }
    };
    if !(nu.is_finite() && nu > 0.0) {
        return Err(refusal(
            RefusalCode::NonFinite,
            format!("{} produced invalid Nusselt number {nu}", id.name()),
        ));
    }
    Ok(NusseltResult {
        id,
        nusselt: nu,
        discrepancy_rel: id.discrepancy_rel(),
        groups,
    })
}

fn rectangular_laminar_cwt(a: f64) -> f64 {
    let shape = ((((-0.548 * a + 2.702) * a - 5.119) * a + 4.970) * a - 2.610) * a + 1.0;
    7.541 * shape
}

fn rectangular_laminar_chf(a: f64) -> f64 {
    let shape = ((((-0.1861 * a + 1.0578) * a - 2.4765) * a + 3.0853) * a - 2.0421) * a + 1.0;
    8.235 * shape
}

fn rectangular_laminar_cwt_developing_pr072(gz: f64) -> f64 {
    const GZ_KNOTS: [f64; 14] = [
        10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 80.0, 100.0, 120.0, 140.0, 160.0, 180.0, 200.0, 220.0,
    ];
    const NU_KNOTS: [f64; 14] = [
        4.20, 4.79, 5.23, 5.61, 5.95, 6.27, 6.88, 7.42, 7.91, 8.37, 8.80, 9.20, 9.60, 10.00,
    ];
    if gz <= GZ_KNOTS[0] {
        let fd = rectangular_laminar_cwt(0.5);
        return fd + (NU_KNOTS[0] - fd) * gz / GZ_KNOTS[0];
    }
    for i in 1..GZ_KNOTS.len() {
        if gz <= GZ_KNOTS[i] {
            let frac = (gz - GZ_KNOTS[i - 1]) / (GZ_KNOTS[i] - GZ_KNOTS[i - 1]);
            return NU_KNOTS[i - 1] + frac * (NU_KNOTS[i] - NU_KNOTS[i - 1]);
        }
    }
    NU_KNOTS[NU_KNOTS.len() - 1]
}

// ---------------------------------------------------------------------------
// N-patch gray-diffuse radiative enclosure (row-sum + reciprocity admitted)
// ---------------------------------------------------------------------------

/// Admission tolerances for an enclosure view-factor matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewFactorTolerance {
    /// Maximum allowed `|Σ_j F_ij − 1|`.
    pub row_sum_abs: f64,
    /// Maximum allowed relative area-weighted reciprocity residual `|A_i F_ij − A_j F_ji| / max(...)`.
    pub reciprocity_rel: f64,
}

impl Default for ViewFactorTolerance {
    fn default() -> Self {
        Self {
            row_sum_abs: 1e-12,
            reciprocity_rel: 1e-12,
        }
    }
}

/// Admitted view-factor matrix for an opaque gray-diffuse N-surface enclosure (e.g., skin,
/// inner/outer garment faces, sky, ground, walls).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewFactorMatrix {
    areas_m2: Vec<f64>,
    factors: Vec<Vec<f64>>,
    row_sums: Vec<f64>,
    max_reciprocity_residual: f64,
}

impl ViewFactorMatrix {
    /// Admit a view-factor matrix after checking non-negativity, row-sum closure, and
    /// area-weighted reciprocity `A_i F_ij = A_j F_ji`.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] on shape mismatch, non-positive areas, out-of-range factors,
    /// row closure violations, or reciprocity violations.
    pub fn admit(
        areas_m2: Vec<f64>,
        factors: Vec<Vec<f64>>,
        tol: ViewFactorTolerance,
    ) -> Result<Self, Refusal> {
        let n = areas_m2.len();
        if !(2..=MAX_ENCLOSURE_SURFACES).contains(&n) {
            return Err(refusal(
                RefusalCode::OutOfRange,
                format!("enclosure surface count {n} outside 2..={MAX_ENCLOSURE_SURFACES}"),
            ));
        }
        if !(tol.row_sum_abs.is_finite()
            && tol.row_sum_abs >= 0.0
            && tol.row_sum_abs < 1.0
            && tol.reciprocity_rel.is_finite()
            && tol.reciprocity_rel >= 0.0
            && tol.reciprocity_rel < 1.0)
        {
            return Err(refusal(
                RefusalCode::OutOfRange,
                "view-factor tolerances must lie in [0, 1)",
            ));
        }
        if factors.len() != n || factors.iter().any(|r| r.len() != n) {
            return Err(refusal(
                RefusalCode::OutOfRange,
                format!("view-factor matrix must be {n}x{n}"),
            ));
        }
        for (i, &a) in areas_m2.iter().enumerate() {
            if !(a.is_finite() && a > 0.0) {
                return Err(refusal(
                    RefusalCode::OutOfRange,
                    format!("enclosure surface {i} area {a} must be finite and positive"),
                ));
            }
        }
        let mut row_sums = Vec::with_capacity(n);
        for (i, row) in factors.iter().enumerate() {
            let mut sum = 0.0;
            for (j, &f) in row.iter().enumerate() {
                if !(f.is_finite() && (0.0..=1.0).contains(&f)) {
                    return Err(refusal(
                        RefusalCode::OutOfRange,
                        format!("view factor F[{i}][{j}] = {f} must lie in [0, 1]"),
                    ));
                }
                sum += f;
            }
            if (sum - 1.0).abs() > tol.row_sum_abs {
                return Err(refusal(
                    RefusalCode::OutOfRange,
                    format!(
                        "view-factor row {i} sum {sum} deviates from 1 beyond {}",
                        tol.row_sum_abs
                    ),
                ));
            }
            row_sums.push(sum);
        }
        let mut max_reciprocity_residual = 0.0f64;
        for i in 0..n {
            for j in (i + 1)..n {
                let lhs = areas_m2[i] * factors[i][j];
                let rhs = areas_m2[j] * factors[j][i];
                let scale = lhs.abs().max(rhs.abs()).max(f64::MIN_POSITIVE);
                let res = (lhs - rhs).abs() / scale;
                max_reciprocity_residual = max_reciprocity_residual.max(res);
                if res > tol.reciprocity_rel {
                    return Err(refusal(
                        RefusalCode::OutOfRange,
                        format!(
                            "view-factor reciprocity violated on ({i},{j}): A_i F_ij={lhs}, A_j F_ji={rhs} (rel {res:.3e})"
                        ),
                    ));
                }
            }
        }
        Ok(Self {
            areas_m2,
            factors,
            row_sums,
            max_reciprocity_residual,
        })
    }

    /// Two equal-area infinite parallel plates (`F12 = F21 = 1` — e.g., fabric facing skin or sky).
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] if `area_m2 <= 0`.
    pub fn infinite_parallel_plates(area_m2: f64) -> Result<Self, Refusal> {
        Self::admit(
            vec![area_m2, area_m2],
            vec![vec![0.0, 1.0], vec![1.0, 0.0]],
            ViewFactorTolerance {
                row_sum_abs: 0.0,
                reciprocity_rel: 0.0,
            },
        )
    }

    /// Number of surfaces.
    #[must_use]
    pub fn len(&self) -> usize {
        self.areas_m2.len()
    }

    /// Always false for an admitted enclosure.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.areas_m2.is_empty()
    }

    /// Surface areas [m²].
    #[must_use]
    pub fn areas_m2(&self) -> &[f64] {
        &self.areas_m2
    }

    /// Largest admitted reciprocity residual.
    #[must_use]
    pub const fn max_reciprocity_residual(&self) -> f64 {
        self.max_reciprocity_residual
    }
}

/// Result of solving the N-patch gray-diffuse radiosity equations
/// `J_i − (1 − ε_i) Σ_j F_ij J_j = ε_i σ T_i⁴`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RadiosityResult {
    /// Surface radiosities `J_i` [W/m²].
    pub radiosity_w_m2: Vec<f64>,
    /// Surface irradiations `G_i = Σ_j F_ij J_j` [W/m²].
    pub irradiation_w_m2: Vec<f64>,
    /// Net outward radiative flux `q_i = J_i − G_i` [W/m²].
    pub net_outward_flux_w_m2: Vec<f64>,
    /// Net outward radiative heat rate `Q_i = A_i q_i` [W].
    pub net_outward_heat_w: Vec<f64>,
    /// Enclosure energy conservation closure `Σ_i Q_i` [W] (zero up to rounding).
    pub enclosure_energy_closure_w: f64,
    /// Largest linear residual of the radiosity matrix solve [W/m²].
    pub linear_residual_max_w_m2: f64,
}

/// Opaque gray-diffuse N-patch radiative enclosure.
#[derive(Debug, Clone, PartialEq)]
pub struct GrayDiffuseEnclosure {
    emissivities: Vec<f64>,
    view_factors: ViewFactorMatrix,
}

impl GrayDiffuseEnclosure {
    /// Bind hemispherical total emissivities `ε_i ∈ (0, 1]` to an admitted [`ViewFactorMatrix`].
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] on length mismatch or emissivity outside `(0, 1]`.
    pub fn new(emissivities: Vec<f64>, view_factors: ViewFactorMatrix) -> Result<Self, Refusal> {
        if emissivities.len() != view_factors.len() {
            return Err(refusal(
                RefusalCode::OutOfRange,
                format!(
                    "{} emissivities supplied for {}-surface enclosure",
                    emissivities.len(),
                    view_factors.len()
                ),
            ));
        }
        for (i, &eps) in emissivities.iter().enumerate() {
            if !(eps.is_finite() && eps > 0.0 && eps <= 1.0) {
                return Err(refusal(
                    RefusalCode::OutOfRange,
                    format!("surface {i} emissivity {eps} must lie in (0, 1]"),
                ));
            }
        }
        Ok(Self {
            emissivities,
            view_factors,
        })
    }

    /// Solve the gray-diffuse enclosure at absolute surface temperatures `temperatures_k` [K].
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] on mismatched length or non-positive temperature;
    /// [`RefusalCode::NotConverged`] if the radiosity operator is singular.
    pub fn solve(&self, temperatures_k: &[f64]) -> Result<RadiosityResult, Refusal> {
        let n = self.emissivities.len();
        if temperatures_k.len() != n {
            return Err(refusal(
                RefusalCode::OutOfRange,
                format!(
                    "{} temperatures supplied for {n} enclosure surfaces",
                    temperatures_k.len()
                ),
            ));
        }
        let mut matrix = vec![vec![0.0; n]; n];
        let mut rhs = vec![0.0; n];
        for i in 0..n {
            let t = temperatures_k[i];
            if !(t.is_finite() && t > 0.0) {
                return Err(refusal(
                    RefusalCode::OutOfRange,
                    format!("surface {i} temperature {t} K must be finite and strictly positive"),
                ));
            }
            let eps = self.emissivities[i];
            let t2 = t * t;
            rhs[i] = eps * STEFAN_BOLTZMANN_W_M2_K4 * t2 * t2;
            if !rhs[i].is_finite() {
                return Err(refusal(
                    RefusalCode::NonFinite,
                    format!("surface {i} emissive power overflowed at {t} K"),
                ));
            }
            for j in 0..n {
                matrix[i][j] =
                    if i == j { 1.0 } else { 0.0 } - (1.0 - eps) * self.view_factors.factors[i][j];
            }
        }
        let radiosity_w_m2 = solve_dense(&matrix, &rhs)?;
        let mut irradiation_w_m2 = vec![0.0; n];
        let mut net_outward_flux_w_m2 = vec![0.0; n];
        let mut net_outward_heat_w = vec![0.0; n];
        let mut closure = 0.0;
        let mut residual_max = 0.0f64;
        for i in 0..n {
            let mut irr = 0.0;
            let mut lhs = 0.0;
            for j in 0..n {
                irr += self.view_factors.factors[i][j] * radiosity_w_m2[j];
                lhs += matrix[i][j] * radiosity_w_m2[j];
            }
            irradiation_w_m2[i] = irr;
            net_outward_flux_w_m2[i] = radiosity_w_m2[i] - irr;
            net_outward_heat_w[i] = self.view_factors.areas_m2[i] * net_outward_flux_w_m2[i];
            closure += net_outward_heat_w[i];
            residual_max = residual_max.max((lhs - rhs[i]).abs());
            if !(irr.is_finite()
                && net_outward_flux_w_m2[i].is_finite()
                && net_outward_heat_w[i].is_finite())
            {
                return Err(refusal(
                    RefusalCode::NonFinite,
                    format!("enclosure surface {i} radiative heat balance overflowed"),
                ));
            }
        }
        if !(closure.is_finite() && residual_max.is_finite()) {
            return Err(refusal(
                RefusalCode::NonFinite,
                "enclosure energy closure or linear residual became non-finite",
            ));
        }
        Ok(RadiosityResult {
            radiosity_w_m2,
            irradiation_w_m2,
            net_outward_flux_w_m2,
            net_outward_heat_w,
            enclosure_energy_closure_w: closure,
            linear_residual_max_w_m2: residual_max,
        })
    }
}

fn solve_dense(matrix: &[Vec<f64>], rhs: &[f64]) -> Result<Vec<f64>, Refusal> {
    let n = rhs.len();
    let mut a = matrix.to_vec();
    let mut b = rhs.to_vec();
    let scale = a
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(0.0f64, f64::max)
        .max(f64::MIN_POSITIVE);
    for col in 0..n {
        let mut piv = col;
        let mut piv_abs = a[col][col].abs();
        for (r, row) in a.iter().enumerate().skip(col + 1) {
            let c = row[col].abs();
            if c > piv_abs {
                piv = r;
                piv_abs = c;
            }
        }
        if !piv_abs.is_finite() || piv_abs <= 64.0 * f64::EPSILON * scale {
            return Err(refusal(
                RefusalCode::NotConverged,
                format!("radiosity matrix is singular at column {col} (pivot {piv_abs:.3e})"),
            ));
        }
        if piv != col {
            a.swap(col, piv);
            b.swap(col, piv);
        }
        let piv_row = a[col].clone();
        let piv_rhs = b[col];
        for r in (col + 1)..n {
            let fac = a[r][col] / piv_row[col];
            a[r][col] = 0.0;
            for c in (col + 1)..n {
                a[r][c] -= fac * piv_row[c];
            }
            b[r] -= fac * piv_rhs;
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut v = b[r];
        for c in (r + 1)..n {
            v -= a[r][c] * x[c];
        }
        x[r] = v / a[r][r];
        if !x[r].is_finite() {
            return Err(refusal(
                RefusalCode::NonFinite,
                format!("radiosity component {r} is non-finite"),
            ));
        }
    }
    Ok(x)
}

/// Request schema identifier for [`evaluate_json`].
pub const MICROCLIMATE_REQUEST_SCHEMA: &str = "ftx-microclimate/1";
/// Report schema identifier emitted by [`evaluate_json`].
pub const MICROCLIMATE_REPORT_SCHEMA: &str = "ftx-microclimate-report/1";

const MAX_MICROCLIMATE_INPUT_BYTES: usize = 16 * 1024 * 1024;

/// Moist-air ambient state input for validity-gated microclimate calculations.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoistAirInput {
    /// Ambient air dry-bulb temperature in degrees Celsius (`[-20, 50]` for Buck 1996 saturation).
    pub temperature_c: f64,
    /// Ambient total pressure in pascals (`> 0`).
    pub pressure_pa: f64,
    /// Relative humidity fraction in `[0, 1]`.
    pub relative_humidity: f64,
}

fn is_default_direction(direction: &ThermalDirection) -> bool {
    *direction == ThermalDirection::HeatingFluid
}

/// Correlation-derived exterior exchange input for steady conduction, surface, and coverage solves.
///
/// Computes USSA-1976 / NIST moist-air properties and evaluates one of the 12 validity-gated
/// Nusselt correlations to derive `h = Nu · k / L`, lowering to a qualified [`Exterior`] marked
/// [`Evidence::Estimate`]. Requires explicit `allow_estimates = true`; never infers wind speed,
/// geometry, or empirical garment-film accuracy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicroclimateExterior {
    /// Ambient moist-air state; `temperature_c` must match the boundary's `outside_c`.
    pub air: MoistAirInput,
    /// Validity-gated Nusselt correlation identifier.
    pub correlation: CorrelationId,
    /// Characteristic length `L` (or diameter `D` / `D_h`) in metres (`> 0`).
    pub characteristic_length_m: f64,
    /// Optional freestream wind speed in m/s (mutually exclusive with `reynolds`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_speed_m_s: Option<f64>,
    /// Optional surface-to-air driving temperature difference in K (mutually exclusive with `rayleigh`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_t_k: Option<f64>,
    /// Optional gravitational acceleration in m/s² (requires `delta_t_k`; defaults to `9.81`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gravity_m_s2: Option<f64>,
    /// Optional explicit Reynolds number `Re` (mutually exclusive with `wind_speed_m_s`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reynolds: Option<f64>,
    /// Optional explicit Rayleigh number `Ra` (mutually exclusive with `delta_t_k`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rayleigh: Option<f64>,
    /// Optional duct `L / D_h` ratio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_over_hydraulic_diameter: Option<f64>,
    /// Optional rectangular-duct aspect ratio `a ∈ (0, 1]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<f64>,
    /// Heat-flow direction for Dittus–Boelter.
    #[serde(default, skip_serializing_if = "is_default_direction")]
    pub direction: ThermalDirection,
    /// Admitted surface temperature range `[min_c, max_c]` in degrees Celsius.
    pub surface_temperature_range_c: [f64; 2],
    /// Explicit consent to admit correlation-derived `Evidence::Estimate` convection.
    #[serde(default)]
    pub allow_estimates: bool,
    /// Optional source-qualified opaque-gray ambient radiation on the exposed outer face.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radiation: Option<AmbientRadiation>,
}

/// Checked lowering of [`MicroclimateExterior`] into an [`Exterior`] boundary plus provenance receipt.
#[derive(Clone, Debug)]
pub struct ResolvedMicroclimateExterior {
    /// Checked exterior boundary fed into [`crate::thermal::steady::SteadyConductionModel::solve_exterior`].
    pub exterior: Exterior,
    /// Complete moist-air, correlation, dimensionless-group, and discrepancy provenance receipt.
    pub receipt: serde_json::Value,
    /// Whether the evaluated correlation used forced flow (`Re` / `wind_speed_m_s`).
    pub forced_convection: bool,
}

fn air_receipt(input: MoistAirInput, air: &MoistAirState) -> serde_json::Value {
    json!({
        "temperature_c": input.temperature_c,
        "temperature_k": air.temperature_k,
        "pressure_pa": air.pressure_pa,
        "relative_humidity": input.relative_humidity,
        "water_mole_fraction": air.water_mole_fraction,
        "density_kg_m3": air.density,
        "sound_speed_m_s": air.sound_speed,
        "dynamic_viscosity_pa_s": air.dynamic_viscosity,
        "kinematic_viscosity_m2_s": air.kinematic_viscosity,
        "thermal_conductivity_w_m_k": air.thermal_conductivity,
        "thermal_diffusivity_m2_s": air.thermal_diffusivity,
        "specific_heat_cp_j_kg_k": air.specific_heat_cp,
        "prandtl": air.prandtl,
    })
}

impl MicroclimateExterior {
    /// Evaluate moist-air thermophysics and the requested validity-gated Nusselt correlation,
    /// lowering to a checked [`Exterior`] boundary and complete provenance receipt.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] if `allow_estimates` is `false` or mutually exclusive
    /// dimensionless/physical inputs are combined; [`RefusalCode::OutOfRange`] or
    /// [`RefusalCode::NonFinite`] on out-of-domain air state, correlation bounds, or length.
    pub fn resolve(&self) -> Result<ResolvedMicroclimateExterior, Refusal> {
        if !self.allow_estimates {
            return Err(refusal(
                RefusalCode::InvalidDocument,
                "microclimate correlation convection is an empirical model estimate; explicit allow_estimates = true is required",
            ));
        }
        if !self.air.temperature_c.is_finite() {
            return Err(refusal(
                RefusalCode::OutOfRange,
                "air temperature_c must be finite",
            ));
        }
        let air = MoistAirState::moist_air(
            self.air.temperature_c + 273.15,
            self.air.pressure_pa,
            self.air.relative_humidity,
        )?;
        if self.wind_speed_m_s.is_some() && self.reynolds.is_some() {
            return Err(refusal(
                RefusalCode::InvalidDocument,
                "supply either wind_speed_m_s or reynolds, not both",
            ));
        }
        if self.delta_t_k.is_some() && self.rayleigh.is_some() {
            return Err(refusal(
                RefusalCode::InvalidDocument,
                "supply either delta_t_k or rayleigh, not both",
            ));
        }
        if self.gravity_m_s2.is_some() && self.delta_t_k.is_none() {
            return Err(refusal(
                RefusalCode::InvalidDocument,
                "gravity_m_s2 requires delta_t_k",
            ));
        }
        let reynolds = match (self.wind_speed_m_s, self.reynolds) {
            (Some(u), None) => Some(air.reynolds(u, self.characteristic_length_m)?),
            (None, r) => r,
            (Some(_), Some(_)) => unreachable!(),
        };
        let rayleigh = match (self.delta_t_k, self.rayleigh) {
            (Some(dt), None) => {
                let g = self.gravity_m_s2.unwrap_or(9.81);
                Some(air.rayleigh(dt, self.characteristic_length_m, g)?)
            }
            (None, r) => r,
            (Some(_), Some(_)) => unreachable!(),
        };
        let inputs = CorrelationInputs {
            reynolds,
            prandtl: Some(air.prandtl),
            length_over_hydraulic_diameter: self.length_over_hydraulic_diameter,
            aspect_ratio: self.aspect_ratio,
            rayleigh,
            direction: self.direction,
        };
        let nu_res = evaluate_convection(self.correlation, inputs)?;
        let h_w_m2_k = nu_res
            .heat_transfer_coefficient(air.thermal_conductivity, self.characteristic_length_m)?;
        let mut exterior = nu_res.to_exterior(
            &air,
            self.characteristic_length_m,
            self.surface_temperature_range_c,
            self.allow_estimates,
            self.radiation.clone(),
        )?;
        // Preserve the caller's exact Celsius temperature without `(t + 273.15) - 273.15` ULP drift
        // so `solve_exterior`'s exact `outside_c == exterior.air_c` gate matches deterministically.
        exterior.air_c = self.air.temperature_c;
        exterior.validate_convection()?;
        let forced_convection = self
            .correlation
            .validity_bounds()
            .iter()
            .any(|(axis, _, _)| *axis == "Re");
        let receipt = json!({
            "air": air_receipt(self.air, &air),
            "correlation": self.correlation,
            "name": nu_res.id.name(),
            "citation": nu_res.id.citation(),
            "characteristic_length_m": self.characteristic_length_m,
            "nu": nu_res.nusselt,
            "h_w_m2_k": h_w_m2_k,
            "discrepancy_rel": nu_res.discrepancy_rel,
            "groups": nu_res.groups,
            "evidence": Evidence::Estimate,
        });
        Ok(ResolvedMicroclimateExterior {
            exterior,
            receipt,
            forced_convection,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MicroclimateRequest {
    schema: String,
    air: MoistAirInput,
    #[serde(default)]
    convection: Option<ConvectionInput>,
    #[serde(default)]
    enclosure: Option<EnclosureInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConvectionInput {
    correlation: CorrelationId,
    characteristic_length_m: f64,
    #[serde(default)]
    wind_speed_m_s: Option<f64>,
    #[serde(default)]
    delta_t_k: Option<f64>,
    #[serde(default)]
    gravity_m_s2: Option<f64>,
    #[serde(default)]
    reynolds: Option<f64>,
    #[serde(default)]
    rayleigh: Option<f64>,
    #[serde(default)]
    length_over_hydraulic_diameter: Option<f64>,
    #[serde(default)]
    aspect_ratio: Option<f64>,
    #[serde(default)]
    direction: ThermalDirection,
    #[serde(default)]
    surface_temperature_range_c: Option<[f64; 2]>,
    #[serde(default)]
    allow_estimates: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnclosureInput {
    areas_m2: Vec<f64>,
    emissivities: Vec<f64>,
    temperatures_c: Vec<f64>,
    view_factors: Vec<Vec<f64>>,
    #[serde(default)]
    row_sum_abs_tol: Option<f64>,
    #[serde(default)]
    reciprocity_rel_tol: Option<f64>,
}

/// Evaluate a bounded `ftx-microclimate/1` JSON request (moist-air thermophysics, optional
/// validity-gated Nusselt convection + checked `Exterior`, and optional N-patch gray-diffuse
/// radiosity enclosure) and return canonical `ftx-microclimate-report/1` JSON.
///
/// # Errors
/// [`RefusalCode::BudgetExceeded`] if input exceeds 16 MiB; [`RefusalCode::InvalidDocument`] on
/// malformed JSON, unknown fields, mismatched schema, or contradictory inputs;
/// [`RefusalCode::OutOfRange`] or [`RefusalCode::NotConverged`] on physics-domain violations.
pub fn evaluate_json(input: &str) -> Result<String, Refusal> {
    if input.len() > MAX_MICROCLIMATE_INPUT_BYTES {
        return Err(refusal(
            RefusalCode::BudgetExceeded,
            "microclimate input exceeds 16 MiB",
        ));
    }
    let req: MicroclimateRequest = serde_json::from_str(input)
        .map_err(|e| refusal(RefusalCode::InvalidDocument, e.to_string()))?;
    if req.schema != MICROCLIMATE_REQUEST_SCHEMA {
        return Err(refusal(
            RefusalCode::InvalidDocument,
            format!(
                "expected schema {MICROCLIMATE_REQUEST_SCHEMA:?}, got {:?}",
                req.schema
            ),
        ));
    }
    if !req.air.temperature_c.is_finite() {
        return Err(refusal(
            RefusalCode::OutOfRange,
            "air temperature_c must be finite",
        ));
    }
    let air = MoistAirState::moist_air(
        req.air.temperature_c + 273.15,
        req.air.pressure_pa,
        req.air.relative_humidity,
    )?;

    let convection = if let Some(conv) = req.convection {
        if conv.wind_speed_m_s.is_some() && conv.reynolds.is_some() {
            return Err(refusal(
                RefusalCode::InvalidDocument,
                "supply either wind_speed_m_s or reynolds, not both",
            ));
        }
        if conv.delta_t_k.is_some() && conv.rayleigh.is_some() {
            return Err(refusal(
                RefusalCode::InvalidDocument,
                "supply either delta_t_k or rayleigh, not both",
            ));
        }
        if conv.gravity_m_s2.is_some() && conv.delta_t_k.is_none() {
            return Err(refusal(
                RefusalCode::InvalidDocument,
                "gravity_m_s2 requires delta_t_k",
            ));
        }
        let reynolds = match (conv.wind_speed_m_s, conv.reynolds) {
            (Some(u), None) => Some(air.reynolds(u, conv.characteristic_length_m)?),
            (None, r) => r,
            (Some(_), Some(_)) => unreachable!(),
        };
        let rayleigh = match (conv.delta_t_k, conv.rayleigh) {
            (Some(dt), None) => {
                let g = conv.gravity_m_s2.unwrap_or(9.81);
                Some(air.rayleigh(dt, conv.characteristic_length_m, g)?)
            }
            (None, r) => r,
            (Some(_), Some(_)) => unreachable!(),
        };
        let inputs = CorrelationInputs {
            reynolds,
            prandtl: Some(air.prandtl),
            length_over_hydraulic_diameter: conv.length_over_hydraulic_diameter,
            aspect_ratio: conv.aspect_ratio,
            rayleigh,
            direction: conv.direction,
        };
        let nu_res = evaluate_convection(conv.correlation, inputs)?;
        let h_w_m2_k = nu_res
            .heat_transfer_coefficient(air.thermal_conductivity, conv.characteristic_length_m)?;
        let exterior = match conv.surface_temperature_range_c {
            Some(range) => {
                let mut ext = nu_res.to_exterior(
                    &air,
                    conv.characteristic_length_m,
                    range,
                    conv.allow_estimates,
                    None,
                )?;
                // Preserve the caller's exact Celsius temperature without `(t + 273.15) - 273.15`
                // ULP drift so downstream `solve_exterior`'s `outside_c == exterior.air_c` gate
                // matches deterministically.
                ext.air_c = req.air.temperature_c;
                if conv.allow_estimates {
                    ext.validate_convection()?;
                }
                Some(ext)
            }
            None => None,
        };
        Some(json!({
            "correlation": conv.correlation,
            "name": nu_res.id.name(),
            "citation": nu_res.id.citation(),
            "nu": nu_res.nusselt,
            "h_w_m2_k": h_w_m2_k,
            "discrepancy_rel": nu_res.discrepancy_rel,
            "groups": nu_res.groups,
            "evidence": Evidence::Estimate,
            "exterior": exterior,
        }))
    } else {
        None
    };

    let enclosure = if let Some(enc) = req.enclosure {
        let mut tol = ViewFactorTolerance::default();
        if let Some(rs) = enc.row_sum_abs_tol {
            tol.row_sum_abs = rs;
        }
        if let Some(rr) = enc.reciprocity_rel_tol {
            tol.reciprocity_rel = rr;
        }
        let vf = ViewFactorMatrix::admit(enc.areas_m2, enc.view_factors, tol)?;
        let max_reciprocity_residual = vf.max_reciprocity_residual();
        let model = GrayDiffuseEnclosure::new(enc.emissivities, vf)?;
        let temps_k: Vec<f64> = enc
            .temperatures_c
            .iter()
            .map(|&t| {
                if !t.is_finite() {
                    return Err(refusal(
                        RefusalCode::OutOfRange,
                        "enclosure surface temperatures must be finite",
                    ));
                }
                Ok(t + 273.15)
            })
            .collect::<Result<_, _>>()?;
        let sol = model.solve(&temps_k)?;
        Some(json!({
            "radiosity_w_m2": sol.radiosity_w_m2,
            "irradiation_w_m2": sol.irradiation_w_m2,
            "net_outward_flux_w_m2": sol.net_outward_flux_w_m2,
            "net_outward_heat_w": sol.net_outward_heat_w,
            "enclosure_energy_closure_w": sol.enclosure_energy_closure_w,
            "linear_residual_max_w_m2": sol.linear_residual_max_w_m2,
            "max_reciprocity_residual": max_reciprocity_residual,
        }))
    } else {
        None
    };

    let report = json!({
        "schema": MICROCLIMATE_REPORT_SCHEMA,
        "air": {
            "temperature_c": req.air.temperature_c,
            "temperature_k": air.temperature_k,
            "pressure_pa": air.pressure_pa,
            "relative_humidity": req.air.relative_humidity,
            "water_mole_fraction": air.water_mole_fraction,
            "density_kg_m3": air.density,
            "sound_speed_m_s": air.sound_speed,
            "dynamic_viscosity_pa_s": air.dynamic_viscosity,
            "kinematic_viscosity_m2_s": air.kinematic_viscosity,
            "thermal_conductivity_w_m_k": air.thermal_conductivity,
            "thermal_diffusivity_m2_s": air.thermal_diffusivity,
            "specific_heat_cp_j_kg_k": air.specific_heat_cp,
            "prandtl": air.prandtl,
        },
        "convection": convection,
        "enclosure": enclosure,
        "no_claim": "First-principles ideal moist-air thermophysics, validity-gated convection correlations (Evidence::Estimate), and gray-diffuse enclosure radiosity; not measured garment film coefficients, whole-wearer comfort, or physical fit."
    });
    serde_json::to_string(&report).map_err(|e| refusal(RefusalCode::InvalidDocument, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ussa_1976_and_moist_air_match_published_anchors() {
        let sea = MoistAirState::try_new(&GasSpec::dry_air_ussa1976(), 288.15, 101_325.0).unwrap();
        assert!((sea.density - 1.2250).abs() < 5e-5);
        assert!((sea.sound_speed - 340.294).abs() < 5e-3);
        assert!((sea.dynamic_viscosity - 1.7894e-5).abs() < 5e-9);
        assert!((sea.thermal_conductivity - 2.5326e-2).abs() < 5e-5);
        assert!((0.70..0.72).contains(&sea.prandtl));

        // OIML conventional air density at 20 °C, 1 atm, 50% RH is 1.2 kg/m³; humid air is lighter.
        let moist = MoistAirState::moist_air(293.15, 101_325.0, 0.5).unwrap();
        let dry = MoistAirState::moist_air(293.15, 101_325.0, 0.0).unwrap();
        assert!((moist.density - 1.2).abs() < 2.4e-3);
        assert!(moist.density < dry.density);
        assert!(moist.sound_speed > dry.sound_speed);

        // Supersaturation and out-of-window temperatures refuse fail-closed.
        assert!(MoistAirState::moist_air(293.15, 101_325.0, 1.05).is_err());
        assert!(MoistAirState::moist_air(240.0, 101_325.0, 0.5).is_err());
    }

    #[test]
    fn all_twelve_convection_correlations_admit_in_domain_and_refuse_out_of_domain() {
        let air = MoistAirState::moist_air(293.15, 101_325.0, 0.5).unwrap();
        // Cylinder in 2 m/s wind (e.g., 0.18 m toddler torso diameter): Churchill-Bernstein.
        let re_cyl = air.reynolds(2.0, 0.18).unwrap();
        let cyl = evaluate_convection(
            CorrelationId::ChurchillBernsteinCylinder,
            CorrelationInputs::forced(re_cyl, air.prandtl),
        )
        .unwrap();
        let h_cyl = cyl
            .heat_transfer_coefficient(air.thermal_conductivity, 0.18)
            .unwrap();
        assert!((8.0..30.0).contains(&h_cyl), "h_cyl = {h_cyl}");

        // Still-air vertical garment panel (ΔT = 10 K, height = 0.4 m): Churchill-Chu.
        let ra = air.rayleigh(10.0, 0.4, 9.81).unwrap();
        let nat = evaluate_convection(
            CorrelationId::ChurchillChuVerticalPlate,
            CorrelationInputs::natural(ra, air.prandtl),
        )
        .unwrap();
        let h_nat = nat
            .heat_transfer_coefficient(air.thermal_conductivity, 0.4)
            .unwrap();
        assert!((2.0..8.0).contains(&h_nat), "h_nat = {h_nat}");

        // Lowering to Exterior carries citation, Estimate evidence, and validates when allowed.
        let ext = nat.to_exterior(&air, 0.4, [0.0, 50.0], true, None).unwrap();
        assert_eq!(ext.convection_evidence, Evidence::Estimate);
        assert!((ext.convection_w_m2_k - h_nat).abs() < 1e-12);
        assert!(ext.convection_source.reference.contains("Churchill"));

        // Verify remaining 10 correlations evaluate cleanly inside their domains and refuse outside.
        let valid_cases = [
            (
                CorrelationId::CircularDuctLaminarCwt,
                CorrelationInputs::forced(500.0, 0.71).with_length_ratio(100.0),
            ),
            (
                CorrelationId::CircularDuctLaminarChf,
                CorrelationInputs::forced(500.0, 0.71).with_length_ratio(100.0),
            ),
            (
                CorrelationId::CircularDuctHausen,
                CorrelationInputs::forced(500.0, 0.71).with_length_ratio(10.0),
            ),
            (
                CorrelationId::RectangularDuctLaminarCwt,
                CorrelationInputs::forced(500.0, 0.71)
                    .with_length_ratio(100.0)
                    .with_aspect_ratio(0.5),
            ),
            (
                CorrelationId::RectangularDuctLaminarChf,
                CorrelationInputs::forced(500.0, 0.71)
                    .with_length_ratio(100.0)
                    .with_aspect_ratio(0.5),
            ),
            (
                CorrelationId::RectangularDuctLaminarCwtDevelopingPr072,
                CorrelationInputs::forced(500.0, 0.72)
                    .with_length_ratio(10.0)
                    .with_aspect_ratio(0.5),
            ),
            (
                CorrelationId::DittusBoelter,
                CorrelationInputs::forced(20_000.0, 0.71)
                    .with_length_ratio(20.0)
                    .with_direction(ThermalDirection::CoolingFluid),
            ),
            (
                CorrelationId::Gnielinski,
                CorrelationInputs::forced(20_000.0, 0.71).with_length_ratio(20.0),
            ),
            (
                CorrelationId::FlatPlateLaminarAverage,
                CorrelationInputs::forced(10_000.0, 0.71),
            ),
            (
                CorrelationId::FlatPlateTurbulentAverage,
                CorrelationInputs::forced(1_000_000.0, 0.71),
            ),
        ];
        for (id, inp) in valid_cases {
            let r = evaluate_convection(id, inp).unwrap();
            assert!(r.nusselt > 0.0 && r.nusselt.is_finite(), "{id:?}");
        }
        // Out-of-domain refusals (e.g., laminar Re handed to turbulent Dittus-Boelter).
        assert!(
            evaluate_convection(
                CorrelationId::DittusBoelter,
                CorrelationInputs::forced(500.0, 0.71).with_length_ratio(20.0),
            )
            .is_err()
        );
        // Missing required axis (no Ra handed to Churchill-Chu).
        assert!(
            evaluate_convection(
                CorrelationId::ChurchillChuVerticalPlate,
                CorrelationInputs::forced(500.0, 0.71),
            )
            .is_err()
        );
    }

    #[test]
    fn gray_diffuse_enclosure_matches_exact_parallel_plate_formula_and_conserves_energy() {
        let vf = ViewFactorMatrix::infinite_parallel_plates(2.5).unwrap();
        let (e1, e2) = (0.90, 0.80);
        let (t1, t2) = (306.15, 288.15); // skin (~33 C) vs cool environment (~15 C)
        let enc = GrayDiffuseEnclosure::new(vec![e1, e2], vf).unwrap();
        let res = enc.solve(&[t1, t2]).unwrap();

        // Analytic parallel-plate solution: q1 = σ (T1⁴ − T2⁴) / (1/ε1 + 1/ε2 − 1).
        let q_exact =
            STEFAN_BOLTZMANN_W_M2_K4 * (t1.powi(4) - t2.powi(4)) / (1.0 / e1 + 1.0 / e2 - 1.0);
        assert!(
            (res.net_outward_flux_w_m2[0] - q_exact).abs() < 1e-11 * q_exact.abs(),
            "got {}, want {q_exact}",
            res.net_outward_flux_w_m2[0]
        );
        assert!(res.enclosure_energy_closure_w.abs() < 1e-11);
        assert!(res.linear_residual_max_w_m2 < 1e-11);

        // 3-surface enclosure (garment, sky, ground) with non-equal areas satisfies energy closure.
        // Areas [1, 2, 2]: F01=0.5, F02=0.5; by reciprocity F10=0.25, F20=0.25; F12=F21=0.75.
        let vf3 = ViewFactorMatrix::admit(
            vec![1.0, 2.0, 2.0],
            vec![
                vec![0.0, 0.5, 0.5],
                vec![0.25, 0.0, 0.75],
                vec![0.25, 0.75, 0.0],
            ],
            ViewFactorTolerance::default(),
        )
        .unwrap();
        let enc3 = GrayDiffuseEnclosure::new(vec![0.92, 0.98, 0.85], vf3).unwrap();
        let res3 = enc3.solve(&[300.0, 250.0, 285.0]).unwrap();
        assert!(res3.enclosure_energy_closure_w.abs() < 1e-10);
        assert!(res3.net_outward_flux_w_m2[0] > 0.0); // garment radiates net heat out

        // Broken reciprocity must be refused.
        assert!(
            ViewFactorMatrix::admit(
                vec![1.0, 2.0],
                vec![vec![0.0, 1.0], vec![1.0, 0.0]],
                ViewFactorTolerance::default(),
            )
            .is_err()
        );

        // Overflow in T^4 or A_i * q_i must refuse cleanly with NonFinite.
        let err_t4 = enc.solve(&[1e100, 300.0]).unwrap_err();
        assert_eq!(err_t4.code, ftx_core::RefusalCode::NonFinite);
        let huge_vf = ViewFactorMatrix::infinite_parallel_plates(1e306).unwrap();
        let huge_enc = GrayDiffuseEnclosure::new(vec![0.9, 0.8], huge_vf).unwrap();
        let err_area = huge_enc.solve(&[400.0, 300.0]).unwrap_err();
        assert_eq!(err_area.code, ftx_core::RefusalCode::NonFinite);
    }

    #[test]
    fn evaluate_json_runs_moist_air_convection_enclosure_and_refuses_out_of_domain() {
        let req = serde_json::json!({
            "schema": MICROCLIMATE_REQUEST_SCHEMA,
            "air": { "temperature_c": 20.0, "pressure_pa": 101325.0, "relative_humidity": 0.5 },
            "convection": {
                "correlation": "churchill_chu_vertical_plate",
                "characteristic_length_m": 0.35,
                "delta_t_k": 12.0,
                "surface_temperature_range_c": [18.0, 38.0],
                "allow_estimates": true
            },
            "enclosure": {
                "areas_m2": [1.0, 1.0],
                "emissivities": [0.92, 0.88],
                "temperatures_c": [33.0, 20.0],
                "view_factors": [[0.0, 1.0], [1.0, 0.0]]
            }
        });
        let raw = evaluate_json(&req.to_string()).unwrap();
        let rep: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(rep["schema"], MICROCLIMATE_REPORT_SCHEMA);
        assert!(rep["air"]["density_kg_m3"].as_f64().unwrap() > 1.15);
        assert!(rep["convection"]["h_w_m2_k"].as_f64().unwrap() > 1.0);
        assert_eq!(rep["convection"]["evidence"], "estimate");
        assert!(rep["convection"]["exterior"].is_object());
        assert!(
            rep["enclosure"]["enclosure_energy_closure_w"]
                .as_f64()
                .unwrap()
                .abs()
                < 1e-10
        );

        // Mutually exclusive wind_speed_m_s + reynolds and stray gravity_m_s2 must refuse.
        let mut bad = req.clone();
        bad["convection"]["wind_speed_m_s"] = serde_json::json!(1.0);
        bad["convection"]["reynolds"] = serde_json::json!(1000.0);
        assert!(evaluate_json(&bad.to_string()).is_err());

        let mut stray_g = serde_json::json!({
            "schema": MICROCLIMATE_REQUEST_SCHEMA,
            "air": { "temperature_c": 20.0, "pressure_pa": 101325.0, "relative_humidity": 0.5 },
            "convection": {
                "correlation": "churchill_chu_vertical_plate",
                "characteristic_length_m": 0.35,
                "rayleigh": 1.0e7,
                "gravity_m_s2": 9.81
            }
        });
        assert!(evaluate_json(&stray_g.to_string()).is_err());
        stray_g["convection"]["gravity_m_s2"] = serde_json::json!(-9.81);
        assert!(evaluate_json(&stray_g.to_string()).is_err());

        // Unknown field must refuse fail-closed.
        let mut unknown = req;
        unknown["air"]["guessed"] = serde_json::json!(true);
        assert!(evaluate_json(&unknown.to_string()).is_err());
    }

    #[test]
    fn evaluate_json_preserves_exact_celsius_air_temperature_without_kelvin_roundtrip_drift() {
        let t_c = 23.4_f64;
        assert_ne!(
            (t_c + 273.15) - 273.15,
            t_c,
            "test temperature must exercise Kelvin-Celsius round-trip drift"
        );
        let req = serde_json::json!({
            "schema": MICROCLIMATE_REQUEST_SCHEMA,
            "air": { "temperature_c": t_c, "pressure_pa": 101325.0, "relative_humidity": 0.5 },
            "convection": {
                "correlation": "churchill_bernstein_cylinder",
                "characteristic_length_m": 0.2,
                "wind_speed_m_s": 2.0,
                "surface_temperature_range_c": [10.0, 45.0],
                "allow_estimates": true
            }
        });
        let raw = evaluate_json(&req.to_string()).unwrap();
        let rep: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let air_c = rep["convection"]["exterior"]["air_c"].as_f64().unwrap();
        assert_eq!(air_c.to_bits(), t_c.to_bits());
        let ext: Exterior = serde_json::from_value(rep["convection"]["exterior"].clone()).unwrap();
        ext.validate_convection().unwrap();
    }
}
