//! Dye-rig hardware profile, primary colorant specs, capstan guides, and dancer-arm accumulator.

use ftx_core::receipt::canonical_json_sha256;
use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

/// Physical dye application mechanism along a moving yarn line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DyeApplicatorKind {
    /// Immersion dip bath with metered immersion time / squeezers.
    DipBath,
    /// Micro-dosing syringe / valve jet onto moving yarn.
    MicroJet,
    /// Kiss-roller / padding nip transfer.
    RollerPad,
    /// Piezo drop-on-demand micro-jet head (high spatial bandwidth).
    PiezoSpray,
}

/// One physical dye primary channel (colorant chemistry + retardation + fixation requirements).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyePrimarySpec {
    /// Primary identifier (e.g. `"cyan_reactive_19"`, `"magenta_acid_52"`, `"yellow_reactive_145"`).
    pub id: String,
    /// Maximum deliverable dose along yarn, in grams per metre (or normalized dimensionless dose).
    pub max_dose_per_m: f64,
    /// Chromatographic retardation factor `R_f` in `(0, 1]` (`1.0` = travels with solvent front).
    pub rf_retardation: f64,
    /// Minimum thermal/steam fixation dwell time required for wash-fast bonding, in seconds.
    pub min_fixation_dwell_s: f64,
    /// Representative sRGB preview swatch (display-only; real physics uses spectral `K/S` cards).
    pub preview_srgb: [u8; 3],
    /// Spectral absorption `K(lambda)` sampled on a regular wavelength grid (400..700 nm).
    #[serde(default)]
    pub k_spectrum: Vec<f64>,
    /// Spectral scattering `S(lambda)` sampled on the same grid.
    #[serde(default)]
    pub s_spectrum: Vec<f64>,
}

/// One guide pin / tensioner along the yarn path (modeled by Euler–Eytelwein Capstan law).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapstanPinSpec {
    /// Human label (e.g. `"entry_guide"`, `"bath_roller"`, `"oven_exit_pin"`).
    pub label: String,
    /// Contact wrap angle around the pin, in radians (`>= 0`).
    pub wrap_angle_rad: f64,
    /// Effective friction coefficient `mu` (`>= 0`).
    pub mu_friction: f64,
    /// True if yarn is wet at this pin (triggers wet-tenacity margin check).
    pub wet_zone: bool,
}

/// Inline dancer-arm accumulator buffering continuous dye-line speed against intermittent knitting intake.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DancerAccumulatorSpec {
    /// Total mechanical yarn storage capacity of the dancer arm, in metres (`> 0`).
    pub capacity_m: f64,
    /// Initial pre-loaded yarn length stored in accumulator at `t = 0`, in metres.
    pub initial_buffer_preload_m: f64,
    /// Nominal controller target fill length, in metres.
    pub target_buffer_m: f64,
    /// Proportional trim gain `k_p` (`s^-1`, `>= 0`) steering dye-line speed toward `target_buffer_m`.
    pub dancer_kp_s_inv: f64,
    /// Minimum safe fill fraction (default `0.05` — prevents slack/underflow).
    pub min_fill_fraction: f64,
    /// Maximum safe fill fraction (default `0.95` — prevents mechanical stop/overflow).
    pub max_fill_fraction: f64,
}

impl DancerAccumulatorSpec {
    /// Standard `[0.05, 0.95]` safety-margined dancer arm of given capacity.
    #[must_use]
    pub fn standard(capacity_m: f64, kp_s_inv: f64) -> Self {
        Self {
            capacity_m,
            initial_buffer_preload_m: 0.5 * capacity_m,
            target_buffer_m: 0.5 * capacity_m,
            dancer_kp_s_inv: kp_s_inv,
            min_fill_fraction: 0.05,
            max_fill_fraction: 0.95,
        }
    }

    /// Validates accumulator bounds and fill fractions.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`] or [`RefusalCode::NonFinite`].
    pub fn validate(&self) -> Result<(), Refusal> {
        for (name, v) in [
            ("capacity_m", self.capacity_m),
            ("initial_buffer_preload_m", self.initial_buffer_preload_m),
            ("target_buffer_m", self.target_buffer_m),
            ("dancer_kp_s_inv", self.dancer_kp_s_inv),
            ("min_fill_fraction", self.min_fill_fraction),
            ("max_fill_fraction", self.max_fill_fraction),
        ] {
            Refusal::finite(name, v)?;
        }
        if self.capacity_m <= 0.0
            || self.dancer_kp_s_inv < 0.0
            || !(0.0..1.0).contains(&self.min_fill_fraction)
            || !(0.0..=1.0).contains(&self.max_fill_fraction)
            || self.min_fill_fraction >= self.max_fill_fraction
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "invalid dancer accumulator capacity, gain, or fill fraction bounds",
            ));
        }
        let lo = self.min_fill_fraction * self.capacity_m;
        let hi = self.max_fill_fraction * self.capacity_m;
        if self.initial_buffer_preload_m < lo || self.initial_buffer_preload_m > hi {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "initial_buffer_preload_m lies outside [min_fill_fraction, max_fill_fraction] * capacity_m",
            ));
        }
        if self.target_buffer_m < lo || self.target_buffer_m > hi {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "target_buffer_m lies outside [min_fill_fraction, max_fill_fraction] * capacity_m",
            ));
        }
        Ok(())
    }
}

/// Typed hardware profile for an automated continuous yarn-dyeing rig.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyeMachineProfile {
    /// Stable profile identifier.
    pub id: String,
    /// Human-readable machine description.
    pub name: String,
    /// Applicator technology.
    pub applicator_kind: DyeApplicatorKind,
    /// Ordered dye primary channels.
    pub primaries: Vec<DyePrimarySpec>,
    /// Physical applicator contact zone length along yarn, in metres (`> 0`).
    pub bath_contact_length_m: f64,
    /// Heated fixation / drying oven tunnel length, in metres (`> 0`).
    pub oven_length_m: f64,
    /// Nominal oven temperature in Celsius.
    pub oven_temp_c: f64,
    /// Strict positive minimum continuous line speed (`> 0`, prevents oven dwell scorch), in m/s.
    pub min_line_speed_m_s: f64,
    /// Maximum continuous line speed, in m/s.
    pub max_line_speed_m_s: f64,
    /// Maximum line acceleration magnitude, in m/s^2 (`> 0`).
    pub max_line_accel_m_s2: f64,
    /// Maximum allowable oven residence time before yarn scorching/degradation, in seconds.
    pub scorch_max_dwell_s: f64,
    /// Path length from dye applicator to knitting feeder/spool uptake, in metres (`>= 0`).
    pub applicator_to_feeder_path_m: f64,
    /// Entry yarn tension upstream of first capstan guide, in Newtons (`>= 0`).
    pub entry_tension_n: f64,
    /// Ordered capstan guide pins along the yarn path.
    #[serde(default)]
    pub guide_capstan_pins: Vec<CapstanPinSpec>,
    /// Optional inline dancer-arm accumulator (required for Regime III inline knitting).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accumulator: Option<DancerAccumulatorSpec>,
    /// True if downstream knitter has an optical fiducial marker detector (required for Regime II).
    pub has_optical_fiducial_sensor: bool,
    /// True if rig has a closed-loop yarn-length encoder.
    pub has_inline_encoder: bool,
}

impl DyeMachineProfile {
    /// Reference benchtop 4-primary CMYK piezo-jet continuous yarn dyer (high spatial bandwidth).
    #[must_use]
    pub fn benchtop_piezo_cmyk() -> Self {
        Self {
            id: "benchtop-piezo-cmyk-v1".to_string(),
            name: "Benchtop 4-Channel Piezo-Jet Continuous Yarn Dyer".to_string(),
            applicator_kind: DyeApplicatorKind::PiezoSpray,
            primaries: vec![
                DyePrimarySpec {
                    id: "cyan".to_string(),
                    max_dose_per_m: 1.0,
                    rf_retardation: 0.85,
                    min_fixation_dwell_s: 4.0,
                    preview_srgb: [0, 150, 214],
                    k_spectrum: vec![],
                    s_spectrum: vec![],
                },
                DyePrimarySpec {
                    id: "magenta".to_string(),
                    max_dose_per_m: 1.0,
                    rf_retardation: 0.80,
                    min_fixation_dwell_s: 4.0,
                    preview_srgb: [212, 34, 118],
                    k_spectrum: vec![],
                    s_spectrum: vec![],
                },
                DyePrimarySpec {
                    id: "yellow".to_string(),
                    max_dose_per_m: 1.0,
                    rf_retardation: 0.90,
                    min_fixation_dwell_s: 4.0,
                    preview_srgb: [242, 204, 24],
                    k_spectrum: vec![],
                    s_spectrum: vec![],
                },
                DyePrimarySpec {
                    id: "black".to_string(),
                    max_dose_per_m: 1.0,
                    rf_retardation: 0.72,
                    min_fixation_dwell_s: 4.5,
                    preview_srgb: [28, 28, 32],
                    k_spectrum: vec![],
                    s_spectrum: vec![],
                },
            ],
            bath_contact_length_m: 0.004,
            oven_length_m: 0.60,
            oven_temp_c: 115.0,
            min_line_speed_m_s: 0.04,
            max_line_speed_m_s: 0.12,
            max_line_accel_m_s2: 0.25,
            scorch_max_dwell_s: 20.0,
            applicator_to_feeder_path_m: 1.2,
            entry_tension_n: 0.35,
            guide_capstan_pins: vec![
                CapstanPinSpec {
                    label: "entry_tensioner".to_string(),
                    wrap_angle_rad: 1.0,
                    mu_friction: 0.22,
                    wet_zone: false,
                },
                CapstanPinSpec {
                    label: "post_jet_guide".to_string(),
                    wrap_angle_rad: 0.8,
                    mu_friction: 0.28,
                    wet_zone: true,
                },
                CapstanPinSpec {
                    label: "oven_exit_guide".to_string(),
                    wrap_angle_rad: 0.9,
                    mu_friction: 0.20,
                    wet_zone: false,
                },
            ],
            accumulator: Some(DancerAccumulatorSpec::standard(1.5, 0.35)),
            has_optical_fiducial_sensor: false,
            has_inline_encoder: true,
        }
    }

    /// Reference 3-bath dip-immersion automated yarn draw-through machine (lower bandwidth, simpler mechanics).
    #[must_use]
    pub fn multi_bath_dip_rig() -> Self {
        Self {
            id: "multi-bath-dip-v1".to_string(),
            name: "3-Bath Automated Yarn Draw-Through Dye Rig".to_string(),
            applicator_kind: DyeApplicatorKind::DipBath,
            primaries: vec![
                DyePrimarySpec {
                    id: "indigo_blue".to_string(),
                    max_dose_per_m: 1.0,
                    rf_retardation: 0.78,
                    min_fixation_dwell_s: 6.0,
                    preview_srgb: [30, 72, 148],
                    k_spectrum: vec![],
                    s_spectrum: vec![],
                },
                DyePrimarySpec {
                    id: "madder_red".to_string(),
                    max_dose_per_m: 1.0,
                    rf_retardation: 0.75,
                    min_fixation_dwell_s: 6.0,
                    preview_srgb: [192, 46, 56],
                    k_spectrum: vec![],
                    s_spectrum: vec![],
                },
                DyePrimarySpec {
                    id: "weld_yellow".to_string(),
                    max_dose_per_m: 1.0,
                    rf_retardation: 0.86,
                    min_fixation_dwell_s: 5.5,
                    preview_srgb: [228, 186, 36],
                    k_spectrum: vec![],
                    s_spectrum: vec![],
                },
            ],
            bath_contact_length_m: 0.035,
            oven_length_m: 0.90,
            oven_temp_c: 105.0,
            min_line_speed_m_s: 0.04,
            max_line_speed_m_s: 0.12,
            max_line_accel_m_s2: 0.15,
            scorch_max_dwell_s: 28.0,
            applicator_to_feeder_path_m: 1.6,
            entry_tension_n: 0.40,
            guide_capstan_pins: vec![
                CapstanPinSpec {
                    label: "dip_plunge_pin".to_string(),
                    wrap_angle_rad: 1.5,
                    mu_friction: 0.30,
                    wet_zone: true,
                },
                CapstanPinSpec {
                    label: "nip_roller".to_string(),
                    wrap_angle_rad: 1.0,
                    mu_friction: 0.25,
                    wet_zone: true,
                },
            ],
            accumulator: None,
            has_optical_fiducial_sensor: false,
            has_inline_encoder: true,
        }
    }

    /// Validates physical geometry, speed floors, and dual-sided thermal dwell window.
    ///
    /// # Errors
    /// [`RefusalCode::OutOfRange`], [`RefusalCode::MaterialProcessHazard`], or [`RefusalCode::NonFinite`].
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.id.trim().is_empty() || self.primaries.is_empty() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "dye machine profile requires non-empty id and >= 1 primary channel",
            ));
        }
        for (name, v) in [
            ("bath_contact_length_m", self.bath_contact_length_m),
            ("oven_length_m", self.oven_length_m),
            ("oven_temp_c", self.oven_temp_c),
            ("min_line_speed_m_s", self.min_line_speed_m_s),
            ("max_line_speed_m_s", self.max_line_speed_m_s),
            ("max_line_accel_m_s2", self.max_line_accel_m_s2),
            ("scorch_max_dwell_s", self.scorch_max_dwell_s),
            (
                "applicator_to_feeder_path_m",
                self.applicator_to_feeder_path_m,
            ),
            ("entry_tension_n", self.entry_tension_n),
        ] {
            Refusal::finite(name, v)?;
        }
        if self.min_line_speed_m_s <= 0.0 || self.min_line_speed_m_s > self.max_line_speed_m_s {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                "dye line requires strictly positive min_line_speed_m_s <= max_line_speed_m_s (zero speed causes oven scorch)",
            ));
        }
        if self.bath_contact_length_m <= 0.0
            || self.oven_length_m <= 0.0
            || self.max_line_accel_m_s2 <= 0.0
            || self.scorch_max_dwell_s <= 0.0
            || self.applicator_to_feeder_path_m < 0.0
            || self.entry_tension_n < 0.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "dye machine physical dimensions and limits must be positive",
            ));
        }
        let mut seen_primaries = std::collections::BTreeSet::new();
        for p in &self.primaries {
            if p.id.trim().is_empty() {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "dye primary requires a non-empty id",
                ));
            }
            if !seen_primaries.insert(p.id.as_str()) {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!("dye machine profile contains duplicate primary '{}'", p.id),
                ));
            }
            Refusal::finite("max_dose_per_m", p.max_dose_per_m)?;
            Refusal::finite("rf_retardation", p.rf_retardation)?;
            Refusal::finite("min_fixation_dwell_s", p.min_fixation_dwell_s)?;
            if !(0.0 < p.rf_retardation && p.rf_retardation <= 1.0)
                || p.max_dose_per_m <= 0.0
                || p.min_fixation_dwell_s < 0.0
            {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "primary '{}' has invalid rf_retardation, max_dose_per_m, or min_fixation_dwell_s",
                        p.id
                    ),
                ));
            }
            if p.k_spectrum.len() != p.s_spectrum.len() {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!(
                        "primary '{}' k_spectrum and s_spectrum length mismatch",
                        p.id
                    ),
                ));
            }
            for &v in p.k_spectrum.iter().chain(p.s_spectrum.iter()) {
                Refusal::finite("spectral_ks", v)?;
                if v < 0.0 {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!("primary '{}' contains negative spectral K/S value", p.id),
                    ));
                }
            }
        }
        for pin in &self.guide_capstan_pins {
            if pin.label.trim().is_empty() {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    "capstan pin requires a non-empty label",
                ));
            }
            Refusal::finite("wrap_angle_rad", pin.wrap_angle_rad)?;
            Refusal::finite("mu_friction", pin.mu_friction)?;
            if pin.wrap_angle_rad < 0.0 || pin.mu_friction < 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "capstan pin '{}' wrap_angle_rad and mu_friction cannot be negative",
                        pin.label
                    ),
                ));
            }
        }
        let max_fix_dwell = self
            .primaries
            .iter()
            .map(|p| p.min_fixation_dwell_s)
            .fold(0.0_f64, f64::max);
        let dwell_at_vmax = self.oven_length_m / self.max_line_speed_m_s;
        let dwell_at_vmin = self.oven_length_m / self.min_line_speed_m_s;
        if dwell_at_vmax < max_fix_dwell {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "oven dwell at max speed ({dwell_at_vmax:.2}s) is below required fixation dwell ({max_fix_dwell:.2}s)"
                ),
            ));
        }
        if dwell_at_vmin > self.scorch_max_dwell_s {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                format!(
                    "oven dwell at min speed ({dwell_at_vmin:.2}s) exceeds scorch limit ({:.2}s)",
                    self.scorch_max_dwell_s
                ),
            ));
        }
        if let Some(acc) = &self.accumulator {
            acc.validate()?;
        }
        Ok(())
    }

    /// Canonical SHA-256 digest of this dye machine profile.
    ///
    /// # Errors
    /// Same as [`canonical_json_sha256`].
    pub fn canonical_sha256(&self) -> Result<String, Refusal> {
        canonical_json_sha256(self)
    }
}
