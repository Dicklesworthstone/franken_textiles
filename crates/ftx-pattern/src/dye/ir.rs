//! Versioned continuous yarn dye program IR (`ftx-dye-program/1`) and Regime III intake timeline.

use std::collections::BTreeMap;

use ftx_core::receipt::canonical_json_sha256;
use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use crate::knit::WearerAgeClass;

/// Schema version for continuous yarn dye programs.
pub const DYE_PROGRAM_SCHEMA: &str = "ftx-dye-program/1";

/// Five-regime dyeing architecture taxonomy (Pareto frontier over drift & hardware complexity).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DyeingRegime {
    /// Regime I-A: Open-loop 1D stripes, gradients, and ombrés (drift-invariant horizontally).
    Regime1AStripes,
    /// Regime I-B: Open-loop planned pooling / circular spiral motifs within coherent horizon.
    Regime1BCircularPooling,
    /// Regime II: Hardware-gated optical fiducial mark synchronization (requires sensor).
    Regime2FiducialSynced,
    /// Regime III: Inline dyeing coupled directly to knitting via P-trimmed dancer-arm accumulator.
    Regime3InlineDancer,
    /// Regime IV: Multi-carrier solid-yarn jacquard / intarsia (zero single-strand phase drift).
    Regime4MultiCarrierJacquard,
}

/// Kind of yarn intake phase in a Regime III inline dyeing timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeEventKind {
    /// Active knitting pass consuming yarn at `yarn_intake_speed_m_s > 0`.
    KnitPass,
    /// Carriage turnaround reversal pause (`yarn_intake_speed_m_s == 0`).
    ReversalDwell,
    /// Transfer (`xfer`) pass with zero yarn feed (`yarn_intake_speed_m_s == 0`).
    TransferPause,
    /// Bed racking dwell with zero yarn feed (`yarn_intake_speed_m_s == 0`).
    RackPause,
    /// Planned operator pause (`yarn_intake_speed_m_s == 0`).
    OperatorPause,
}

/// One timed segment of downstream knitting yarn intake for Regime III accumulator verification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeEvent {
    /// Duration of this intake event, in seconds (`> 0`).
    pub duration_s: f64,
    /// Downstream knitting yarn intake velocity during this event, in m/s (`>= 0`).
    pub yarn_intake_speed_m_s: f64,
    /// Semantic event kind.
    pub kind: IntakeEventKind,
}

/// Complete deterministic downstream intake timeline required for Regime III inline verification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Regime3IntakeTimeline {
    /// Ordered intake events spanning the complete knitted run.
    pub events: Vec<IntakeEvent>,
    /// Integration sub-step size for accumulator ODE verification, in seconds (`> 0`).
    pub dt_verify_s: f64,
}

impl Regime3IntakeTimeline {
    /// Total elapsed duration across all events, in seconds.
    #[must_use]
    pub fn total_duration_s(&self) -> f64 {
        self.events.iter().map(|e| e.duration_s).sum()
    }

    /// Total yarn length consumed by downstream knitting, in metres.
    #[must_use]
    pub fn total_intake_m(&self) -> f64 {
        self.events
            .iter()
            .map(|e| e.duration_s * e.yarn_intake_speed_m_s)
            .sum()
    }
}

/// One contiguous arc-length segment along the yarn to be dyed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyeSegment {
    /// Start arc-length along yarn, in metres (`>= 0`).
    pub s_start_m: f64,
    /// End arc-length along yarn, in metres (`> s_start_m`).
    pub s_end_m: f64,
    /// Commanded yarn draw speed through applicator & oven, in m/s (`> 0`).
    pub target_speed_m_s: f64,
    /// Primary channel doses aligned to `DyeMachineProfile::primaries` (`>= 0`).
    pub doses: Vec<f64>,
    /// Optional optical fiducial registration mark at `s_start_m` (Regime II).
    #[serde(default)]
    pub fiducial_mark: bool,
}

/// Complete `ftx-dye-program/1` continuous yarn-dyeing program.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YarnDyeProgram {
    /// Must equal [`DYE_PROGRAM_SCHEMA`] (`"ftx-dye-program/1"`).
    pub schema: String,
    /// Program name.
    pub name: String,
    /// Bound [`crate::knit::YarnRun::run_id`].
    pub run_id: u32,
    /// Catalog yarn identifier.
    pub yarn_id: String,
    /// Required, non-defaulting wearer age classification.
    pub wearer_age_class: WearerAgeClass,
    /// Active dyeing regime.
    pub regime: DyeingRegime,
    /// SHA-256 digest of the target [`super::machine::DyeMachineProfile`].
    pub machine_sha256: String,
    /// Optional SHA-256 digest of [`super::qualification::YarnDyeingQualification`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualification_sha256: Option<String>,
    /// Sacrificial undyed/ramp startup leader length, in metres (must cover ramp + path length).
    pub startup_leader_m: f64,
    /// Sacrificial ramp-down trailer length, in metres.
    pub shutdown_trailer_m: f64,
    /// Contiguous, monotonic dye segments covering the programmed yarn arc length.
    pub segments: Vec<DyeSegment>,
    /// Required intake timeline when `regime == DyeingRegime::Regime3InlineDancer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regime3_intake_timeline: Option<Regime3IntakeTimeline>,
    /// Predicted median perceptual color error (CIEDE2000), if computed by inverse solver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_delta_e00_p50: Option<f64>,
    /// Predicted 95th-percentile worst-case drift CIEDE2000, if computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_delta_e00_p95: Option<f64>,
    /// Irreducible dye-gamut floor CIEDE2000 (separated from transport blur).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gamut_floor_delta_e00: Option<f64>,
    /// Fraction of target image spatial-frequency energy above applicator bandwidth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreproducible_energy_fraction: Option<f64>,
    /// Must be `true` for unwashed/calibration dye swatches (`WearerAgeClass::CalibrationSwatchOnly`).
    #[serde(default)]
    pub not_for_garment_wear: bool,
    /// Deterministic metadata tags.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl YarnDyeProgram {
    /// Total programmed yarn length excluding leaders/trailers, in metres.
    #[must_use]
    pub fn active_yarn_length_m(&self) -> f64 {
        match (self.segments.first(), self.segments.last()) {
            (Some(a), Some(b)) => (b.s_end_m - a.s_start_m).max(0.0),
            _ => 0.0,
        }
    }

    /// Structural validation of schema, monotonic contiguous segments, finite doses, and Regime III timeline presence.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`], [`RefusalCode::OutOfRange`], or [`RefusalCode::NonFinite`].
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.schema != DYE_PROGRAM_SCHEMA {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "expected schema '{DYE_PROGRAM_SCHEMA}', got '{}'",
                    self.schema
                ),
            ));
        }
        if self.name.trim().is_empty() || self.yarn_id.trim().is_empty() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "dye program requires non-empty name and yarn_id",
            ));
        }
        if self.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly
            && !self.not_for_garment_wear
        {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                "calibration dye swatch programs must set not_for_garment_wear = true",
            ));
        }
        Refusal::finite("startup_leader_m", self.startup_leader_m)?;
        Refusal::finite("shutdown_trailer_m", self.shutdown_trailer_m)?;
        if self.startup_leader_m < 0.0 || self.shutdown_trailer_m < 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "startup_leader_m and shutdown_trailer_m cannot be negative",
            ));
        }
        if self.segments.is_empty() {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "dye program contains zero segments",
            ));
        }
        let n_ch = self.segments[0].doses.len();
        if n_ch == 0 {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "dye program segments must specify >= 1 primary dose channel",
            ));
        }
        let mut prev_end: Option<f64> = None;
        for (idx, seg) in self.segments.iter().enumerate() {
            Refusal::finite("s_start_m", seg.s_start_m)?;
            Refusal::finite("s_end_m", seg.s_end_m)?;
            Refusal::finite("target_speed_m_s", seg.target_speed_m_s)?;
            if seg.s_start_m < 0.0 || seg.s_end_m <= seg.s_start_m {
                return Err(Refusal::new(
                    RefusalCode::DegenerateGeometry,
                    format!(
                        "segment {idx} has invalid arc range [{}, {}]",
                        seg.s_start_m, seg.s_end_m
                    ),
                ));
            }
            if let Some(pe) = prev_end
                && (seg.s_start_m - pe).abs() > 1e-9
            {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!(
                        "segment {idx} start {} is not contiguous with previous end {pe}",
                        seg.s_start_m
                    ),
                ));
            }
            prev_end = Some(seg.s_end_m);
            if seg.target_speed_m_s <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::MaterialProcessHazard,
                    format!(
                        "segment {idx} has non-positive line speed {}",
                        seg.target_speed_m_s
                    ),
                ));
            }
            if seg.doses.len() != n_ch {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!(
                        "segment {idx} dose channel count {} != {n_ch}",
                        seg.doses.len()
                    ),
                ));
            }
            for &d in &seg.doses {
                Refusal::finite("dose", d)?;
                if d < 0.0 {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!("segment {idx} contains negative dye dose {d}"),
                    ));
                }
            }
        }
        for (name, opt) in [
            ("expected_delta_e00_p50", self.expected_delta_e00_p50),
            ("expected_delta_e00_p95", self.expected_delta_e00_p95),
            ("gamut_floor_delta_e00", self.gamut_floor_delta_e00),
        ] {
            if let Some(v) = opt {
                Refusal::finite(name, v)?;
                if v < 0.0 {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!("{name} cannot be negative (got {v})"),
                    ));
                }
            }
        }
        if let Some(frac) = self.unreproducible_energy_fraction {
            Refusal::finite("unreproducible_energy_fraction", frac)?;
            if !(0.0..=1.0).contains(&frac) {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("unreproducible_energy_fraction must be in [0, 1] (got {frac})"),
                ));
            }
        }
        if self.regime == DyeingRegime::Regime3InlineDancer
            && self.regime3_intake_timeline.is_none()
        {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "Regime III inline dancer dyeing requires explicit regime3_intake_timeline",
            ));
        }
        if let Some(tl) = &self.regime3_intake_timeline {
            Refusal::finite("dt_verify_s", tl.dt_verify_s)?;
            if tl.events.is_empty() || tl.dt_verify_s <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    "regime3_intake_timeline requires >= 1 event and dt_verify_s > 0",
                ));
            }
            for (idx, ev) in tl.events.iter().enumerate() {
                Refusal::finite("duration_s", ev.duration_s)?;
                Refusal::finite("yarn_intake_speed_m_s", ev.yarn_intake_speed_m_s)?;
                if ev.duration_s <= 0.0 || ev.yarn_intake_speed_m_s < 0.0 {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!("intake event {idx} has invalid duration or negative speed"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Canonical SHA-256 hash of this dye program.
    ///
    /// # Errors
    /// Same as [`canonical_json_sha256`].
    pub fn canonical_sha256(&self) -> Result<String, Refusal> {
        canonical_json_sha256(self)
    }
}
