//! The catalogue and aliases from spec 02 A.1–A.2. The JSON is specification data, not an oracle.

use std::sync::OnceLock;

use serde_json::Value;

/// Measurement order in priors, fusion, readiness and catalogue output.
pub const NAMES: [&str; 35] = [
    "stature",
    "chest",
    "waist",
    "hip",
    "seat_over_nappy",
    "neck_base",
    "across_back",
    "shoulder_length",
    "shoulder_slope_deg",
    "back_neck_to_waist",
    "arm_length",
    "upper_arm",
    "wrist",
    "hand_pass",
    "body_rise",
    "total_crotch_length",
    "inseam",
    "outseam",
    "thigh",
    "knee",
    "calf",
    "ankle",
    "foot_pass",
    "head",
    "belly_depth",
    "chest_depth",
    "waist_depth",
    "hip_depth",
    "chest_width",
    "waist_width",
    "hip_width",
    "armscye_depth",
    "waist_height",
    "crotch_height",
    "knee_height",
];

/// Allowed provenance values, in reference order.
pub const PROVENANCES: [&str; 6] = [
    "MEASURED_TAPE",
    "PHOTO_ESTIMATED",
    "CHART_PRIOR",
    "MODEL_FIT",
    "SCAN_MEASURED",
    "GARMENT_DERIVED",
];

/// Specification catalogue keyed by canonical name. Use [`NAMES`] when order matters.
#[must_use]
pub fn catalogue() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("catalogue.json")).expect("catalogue data")
    })
}

/// Canonicalizes aliases without rejecting application-specific measurement names.
#[must_use]
pub fn canonical_name(name: &str) -> String {
    let key = name.trim().to_lowercase().replace([' ', '-'], "_");
    match key.as_str() {
        "seat" | "hips" | "hip_girth" => "hip",
        "bust" | "chest_girth" => "chest",
        "waist_girth" => "waist",
        "neck_base_girth" => "neck_base",
        "nape_to_waist" | "back_waist_length" => "back_neck_to_waist",
        "crotch_depth" | "rise" => "body_rise",
        "shoulder_slope" => "shoulder_slope_deg",
        "height" => "stature",
        "hand_girth" => "hand_pass",
        "crotch_length" | "total_rise" => "total_crotch_length",
        "inside_leg" => "inseam",
        "outside_leg" => "outseam",
        "head_girth" => "head",
        "upper_arm_girth" | "biceps" => "upper_arm",
        "thigh_girth" => "thigh",
        "scye_depth" => "armscye_depth",
        "heel_instep" => "foot_pass",
        _ => return key,
    }
    .to_string()
}

/// The definition for a measurement, including aliases; unknown names return `None`.
#[must_use]
pub fn definition(name: &str) -> Option<&'static Value> {
    catalogue().get(canonical_name(name))
}

/// Unit suffix, including application-specific angle names ending in `_deg`.
#[must_use]
pub fn unit(name: &str) -> &'static str {
    if canonical_name(name).ends_with("_deg") {
        "deg"
    } else {
        "mm"
    }
}
