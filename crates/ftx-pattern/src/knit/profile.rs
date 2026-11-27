//! Wearer age classification, calibration swatch caps, and knitting machine hardware profiles.

use std::collections::BTreeSet;

use ftx_core::receipt::canonical_json_sha256;
use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use super::ir::CarrierId;

/// Required, non-defaulting wearer age classification governing child-safety gates.
///
/// Deliberately has NO `Default` implementation so callers cannot silently downgrade an infant
/// garment to adult rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WearerAgeClass {
    /// Children under 3 years: strict float, eyelet aperture, cut-tail, drawstring, and fastness gates.
    InfantUnder3,
    /// Children 3 to 14 years: strict drawstring (EN 14682) and snag-float gates.
    Child3To14,
    /// Adult wearer (standard machine and garment safety checks).
    Adult,
    /// Non-wearable stepped calibration swatch ONLY (hard-capped geometry, speed, and takedown).
    CalibrationSwatchOnly,
}

impl WearerAgeClass {
    /// True if child-garment safety gates apply.
    #[must_use]
    pub const fn is_child(self) -> bool {
        matches!(self, Self::InfantUnder3 | Self::Child3To14)
    }

    /// True if infant (<3 yr) stricter caps (cut tails, eyelet aperture, wet rubbing/saliva fastness) apply.
    #[must_use]
    pub const fn is_infant(self) -> bool {
        matches!(self, Self::InfantUnder3)
    }
}

/// Hard numeric safety caps enforced on every [`WearerAgeClass::CalibrationSwatchOnly`] program.
pub struct CalibrationSwatchCaps;

impl CalibrationSwatchCaps {
    /// Maximum active needles in a calibration swatch.
    pub const MAX_NEEDLES: u32 = 80;
    /// Maximum courses in a calibration swatch.
    pub const MAX_COURSES: u32 = 200;
    /// Maximum carriage speed fraction of machine maximum (`0.50`).
    pub const MAX_SPEED_FRACTION: f64 = 0.50;
    /// Maximum takedown / roller tension fraction (`0.60`).
    pub const MAX_TAKEDOWN_FRACTION: f64 = 0.60;
    /// Maximum yarn length per swatch, in metres (`40.0 m`).
    pub const MAX_YARN_M: f64 = 40.0;
    /// Transfers are forbidden in uncalibrated swatches.
    pub const ALLOW_TRANSFERS: bool = false;
    /// Bed racking is forbidden in uncalibrated swatches.
    pub const ALLOW_RACK: bool = false;
}

/// Supported knitting target backend classes (Tier A open vs Tier B proprietary handoff).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineBackendKind {
    /// Open CMU Knitout v2 (`.k`) intermediate interchange format.
    KnitoutV2,
    /// Kniterate 7E flatbed machine (`.kc`, open `knitout-backend-kniterate` dialect).
    KniterateKc,
    /// Brother KH-930 / KH-270 with open AYAB shield (1-bit indexed PNG + checklist).
    BrotherAyab,
    /// Human hand-knitting CYC vector chart & instructions (PDF / SVG).
    HandKnitChart,
    /// Tier B proprietary Shima Seiki (handoff Knitout v2 ONLY — direct binary refused).
    ShimaSeikiHandoff,
    /// Tier B proprietary Stoll (handoff Knitout v2 ONLY — direct binary refused).
    StollHandoff,
}

impl MachineBackendKind {
    /// True if direct proprietary machine binary emission MUST be refused (`OpaqueProprietaryTarget`).
    #[must_use]
    pub const fn is_opaque_proprietary(self) -> bool {
        matches!(self, Self::ShimaSeikiHandoff | Self::StollHandoff)
    }
}

/// Typed hardware capability and safety envelope for a knitting machine or manual process.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineProfile {
    /// Stable profile identifier.
    pub id: String,
    /// Human-readable machine name.
    pub name: String,
    /// Target backend classification.
    pub backend_kind: MachineBackendKind,
    /// Total needles per bed.
    pub needle_count: u32,
    /// Machine gauge in needles per inch (`E`).
    pub gauge_e: f64,
    /// Physical needle spacing in mm (`25.4 / gauge_e`).
    pub needle_pitch_mm: f64,
    /// True if machine has a back needle bed (V-bed or ribber).
    pub has_back_bed: bool,
    /// True if machine has dedicated transfer sliders (`fs`/`bs`).
    pub has_sliders: bool,
    /// True if machine has motorized yarn-inserting hooks (`inhook`/`releasehook`/`outhook`).
    /// False for Kniterate (grippers `in`/`out`) and Brother AYAB.
    pub has_yarn_inserting_hooks: bool,
    /// True if carriage motion is motorized/automated; false for manual-carriage AYAB and hand knitting.
    pub automated_carriage_control: bool,
    /// Available yarn carrier identifiers.
    pub carrier_ids: Vec<CarrierId>,
    /// Minimum needle clearance between parked/active carriers (e.g. `2` on Kniterate).
    pub min_carrier_spacing: u32,
    /// Maximum absolute racking in integer quarter-pitches (e.g. `16` = `±4` full pitches on Kniterate).
    pub max_racking_quarter_pitches: i32,
    /// Maximum simultaneous loops permitted on one needle hook.
    pub max_loops_per_hook: u8,
    /// Maximum consecutive tucked loops without knitting off.
    pub max_tuck_stack: u8,
    /// Valid dimensionless cam stitch setting range `[min, max]`.
    pub cam_setting_range: [f64; 2],
    /// Recommended yarn linear density range `[min_tex, max_tex]`.
    pub recommended_tex_range: [f64; 2],
    /// Maximum course-count imbalance between left and right short-row regions before roller warning/refusal.
    pub max_roller_imbalance_courses: u32,
    /// Maximum single-span interior float length for child garments, in mm (default `12.0 mm`).
    pub child_float_cap_mm: f64,
    /// Maximum interior cut-tail length for infant garments, in mm (default `20.0 mm`).
    pub infant_tail_cap_mm: f64,
    /// Maximum decorative eyelet aperture diameter for infant garments, in mm (default `6.0 mm`).
    pub infant_eyelet_cap_mm: f64,
    /// True if production garment compilation requires a qualified swatch matching `material_sha256` & `machine_sha256`.
    pub requires_calibration_swatch: bool,
}

impl MachineProfile {
    /// Kniterate 7E flatbed V-bed profile (`252` needles, 6 gripper carriers, `±4` pitches, dial `0..=15`).
    #[must_use]
    pub fn kniterate_7e() -> Self {
        Self {
            id: "kniterate-7e".to_string(),
            name: "Kniterate 7E Flatbed V-Bed (Open knitout-backend-kniterate dialect)".to_string(),
            backend_kind: MachineBackendKind::KniterateKc,
            needle_count: 252,
            gauge_e: 7.0,
            needle_pitch_mm: 25.4 / 7.0,
            has_back_bed: true,
            has_sliders: false,
            has_yarn_inserting_hooks: false,
            automated_carriage_control: true,
            carrier_ids: (1..=6).map(|i| CarrierId(i.to_string())).collect(),
            min_carrier_spacing: 2,
            max_racking_quarter_pitches: 16, // +-4 integer pitches
            max_loops_per_hook: 3,
            max_tuck_stack: 3,
            cam_setting_range: [0.0, 15.0],
            recommended_tex_range: [60.0, 450.0],
            max_roller_imbalance_courses: 8,
            child_float_cap_mm: 12.0,
            infant_tail_cap_mm: 20.0,
            infant_eyelet_cap_mm: 6.0,
            requires_calibration_swatch: true,
        }
    }

    /// Brother KH-930 standard-gauge (`4.5 mm`, `200` needles) with open AYAB shield (manual carriage).
    #[must_use]
    pub fn brother_kh930_ayab() -> Self {
        Self {
            id: "brother-kh930-ayab".to_string(),
            name: "Brother KH-930 Standard Gauge (4.5 mm, AYAB Manual Carriage)".to_string(),
            backend_kind: MachineBackendKind::BrotherAyab,
            needle_count: 200,
            gauge_e: 25.4 / 4.5,
            needle_pitch_mm: 4.5,
            has_back_bed: true, // optional KR-850 ribber
            has_sliders: false,
            has_yarn_inserting_hooks: false,
            automated_carriage_control: false,
            carrier_ids: vec![CarrierId("A".to_string()), CarrierId("B".to_string())],
            min_carrier_spacing: 0,
            max_racking_quarter_pitches: 16,
            max_loops_per_hook: 3,
            max_tuck_stack: 3,
            cam_setting_range: [0.0, 10.0],
            recommended_tex_range: [50.0, 260.0],
            max_roller_imbalance_courses: 12,
            child_float_cap_mm: 12.0,
            infant_tail_cap_mm: 20.0,
            infant_eyelet_cap_mm: 6.0,
            requires_calibration_swatch: true,
        }
    }

    /// Brother KH-270 bulky-gauge (`9.0 mm`, `114` needles) with open AYAB shield (manual carriage).
    #[must_use]
    pub fn brother_kh270_ayab() -> Self {
        Self {
            id: "brother-kh270-ayab".to_string(),
            name: "Brother KH-270 Bulky Gauge (9.0 mm, AYAB Manual Carriage)".to_string(),
            backend_kind: MachineBackendKind::BrotherAyab,
            needle_count: 114,
            gauge_e: 25.4 / 9.0,
            needle_pitch_mm: 9.0,
            has_back_bed: false,
            has_sliders: false,
            has_yarn_inserting_hooks: false,
            automated_carriage_control: false,
            carrier_ids: vec![CarrierId("A".to_string()), CarrierId("B".to_string())],
            min_carrier_spacing: 0,
            max_racking_quarter_pitches: 0,
            max_loops_per_hook: 3,
            max_tuck_stack: 3,
            cam_setting_range: [0.0, 10.0],
            recommended_tex_range: [220.0, 1000.0],
            max_roller_imbalance_courses: 12,
            child_float_cap_mm: 12.0,
            infant_tail_cap_mm: 20.0,
            infant_eyelet_cap_mm: 6.0,
            requires_calibration_swatch: true,
        }
    }

    /// Human hand-knitting profile at a stated needle diameter in mm.
    #[must_use]
    pub fn hand_knit(needle_mm: f64) -> Self {
        Self {
            id: format!("hand-knit-{needle_mm:.1}mm"),
            name: format!("Human Hand Knitting ({needle_mm:.1} mm needles)"),
            backend_kind: MachineBackendKind::HandKnitChart,
            needle_count: 1000,
            gauge_e: 25.4 / needle_mm.max(1.0),
            needle_pitch_mm: needle_mm.max(1.0),
            has_back_bed: true,
            has_sliders: false,
            has_yarn_inserting_hooks: false,
            automated_carriage_control: false,
            carrier_ids: (1..=8).map(|i| CarrierId(i.to_string())).collect(),
            min_carrier_spacing: 0,
            max_racking_quarter_pitches: 32,
            max_loops_per_hook: 4,
            max_tuck_stack: 4,
            cam_setting_range: [1.0, 25.0],
            recommended_tex_range: [30.0, 2000.0],
            max_roller_imbalance_courses: 64,
            child_float_cap_mm: 12.0,
            infant_tail_cap_mm: 20.0,
            infant_eyelet_cap_mm: 6.0,
            requires_calibration_swatch: false,
        }
    }

    /// Shima Seiki 12E Tier-B handoff profile (emits verified Knitout v2 ONLY; direct binary refused).
    #[must_use]
    pub fn shima_seiki_12e_handoff() -> Self {
        Self {
            id: "shima-seiki-12e-handoff".to_string(),
            name: "Shima Seiki 12E Fine Gauge (Knitout v2 Handoff Only)".to_string(),
            backend_kind: MachineBackendKind::ShimaSeikiHandoff,
            needle_count: 600,
            gauge_e: 12.0,
            needle_pitch_mm: 25.4 / 12.0,
            has_back_bed: true,
            has_sliders: true,
            has_yarn_inserting_hooks: true,
            automated_carriage_control: true,
            carrier_ids: (1..=10).map(|i| CarrierId(i.to_string())).collect(),
            min_carrier_spacing: 1,
            max_racking_quarter_pitches: 48,
            max_loops_per_hook: 4,
            max_tuck_stack: 4,
            cam_setting_range: [0.0, 99.0],
            recommended_tex_range: [20.0, 140.0],
            max_roller_imbalance_courses: 10,
            child_float_cap_mm: 12.0,
            infant_tail_cap_mm: 20.0,
            infant_eyelet_cap_mm: 6.0,
            requires_calibration_swatch: true,
        }
    }

    /// Validates internal invariants of this machine profile.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] or [`RefusalCode::OutOfRange`].
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.id.trim().is_empty() || self.needle_count == 0 || self.carrier_ids.is_empty() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "machine profile requires non-empty id, needle_count > 0, and >= 1 carrier",
            ));
        }
        let mut seen_carriers = BTreeSet::new();
        for cid in &self.carrier_ids {
            CarrierId::new(cid.as_str())?;
            if !seen_carriers.insert(cid.as_str()) {
                return Err(Refusal::new(
                    RefusalCode::InvalidDocument,
                    format!(
                        "machine profile contains duplicate carrier '{}'",
                        cid.as_str()
                    ),
                ));
            }
        }
        for (name, val) in [
            ("gauge_e", self.gauge_e),
            ("needle_pitch_mm", self.needle_pitch_mm),
            ("cam_setting_range[0]", self.cam_setting_range[0]),
            ("cam_setting_range[1]", self.cam_setting_range[1]),
            ("recommended_tex_range[0]", self.recommended_tex_range[0]),
            ("recommended_tex_range[1]", self.recommended_tex_range[1]),
            ("child_float_cap_mm", self.child_float_cap_mm),
            ("infant_tail_cap_mm", self.infant_tail_cap_mm),
            ("infant_eyelet_cap_mm", self.infant_eyelet_cap_mm),
        ] {
            Refusal::finite(name, val)?;
        }
        if self.gauge_e <= 0.0 || self.needle_pitch_mm <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "machine gauge_e and needle_pitch_mm must be positive",
            ));
        }
        if self.max_loops_per_hook == 0
            || self.max_tuck_stack == 0
            || self.max_racking_quarter_pitches < 0
            || self.child_float_cap_mm <= 0.0
            || self.infant_tail_cap_mm <= 0.0
            || self.infant_eyelet_cap_mm <= 0.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "machine hook/tuck limits, racking cap, and child safety caps must be valid positive bounds",
            ));
        }
        if self.cam_setting_range[0] > self.cam_setting_range[1]
            || self.recommended_tex_range[0] <= 0.0
            || self.recommended_tex_range[0] > self.recommended_tex_range[1]
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "invalid cam_setting_range or recommended_tex_range",
            ));
        }
        Ok(())
    }

    /// Canonical SHA-256 digest of this hardware profile.
    ///
    /// # Errors
    /// Same as [`canonical_json_sha256`].
    pub fn canonical_sha256(&self) -> Result<String, Refusal> {
        canonical_json_sha256(self)
    }
}
