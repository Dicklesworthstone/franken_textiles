//! Qualification bands (`ftx qualify`) and tightness gating (spec 04 §7).
//!
//! Before simulation tightness flags (`{id}: fabric stretched around the body (mean X %, p90 Y %)`)
//! are allowed to block a workspace round (`SIM_GATING_FLAGS`), the simulator must show on THAT
//! specific body and fabric that it can distinguish three known-answer chest bands at zero gravity:
//! `-20 mm` (tight or escaped), `0 mm` (snug or tight in place), and `+20 mm` (easy or snug with
//! mean stretch `< 1.0 %` in place).

use ftx_core::py::{repr_f64, round};
use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::{BodyMeasure, Measurements, SCHEMA};
use serde_json::{Value, json};

/// The three qualification chest-band eases (mm, spec 04 §7.1).
pub const QUALIFY_EASES: [i32; 3] = [-20, 0, 20];

/// Zero-gravity sewing steps for each qualification band (`--sew-frames 70`).
pub const QUALIFY_SEW_STEPS: usize = 70;

/// Gravity drape steps for each qualification band (`--drape-frames 0`).
pub const QUALIFY_DRAPE_STEPS: usize = 0;

/// Default height of each chest band (mm).
pub const DEFAULT_BAND_HEIGHT_MM: f64 = 60.0;

fn bad_input(m: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, m)
}

/// The fit specification shared by all three qualification bands (spec 04 §7.1).
#[must_use]
pub fn band_fit_spec() -> Value {
    json!({
        "schema": "fabric-fit/1",
        "points": [{
            "id": "chest",
            "kind": "girth",
            "instances": ["band_front", "band_back"],
            "landmark": "chest",
            "body": "chest",
            "target_mm": [-50, 0, 50]
        }]
    })
}

/// Builds the known-answer front + back chest band pattern for `ease_mm` (spec 04 §7.1).
///
/// Each band is a rectangle `w × height_mm` with `w = (chest + ease_mm) / 2`.
///
/// # Errors
/// Returns [`RefusalCode::InvalidDocument`] if `chest` is missing or non-positive, or if
/// `w <= 0.0` or `height_mm <= 0.0`.
pub fn band_pattern(
    meas: &Value,
    ease_mm: i32,
    height_mm: f64,
    fabric: Option<&Value>,
) -> Result<Value, Refusal> {
    Refusal::finite("band height", height_mm)?;
    if height_mm <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "band height must be positive",
        ));
    }
    let chest = Measurements(meas.clone())
        .value("chest")
        .map_err(|e| bad_input(format!("band_pattern needs chest measurement ({e})")))?;
    Refusal::finite("chest", chest)?;
    let ease = f64::from(ease_mm);
    let w = (chest + ease) / 2.0;
    if w <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("band width (chest + ease)/2 = {w} mm must be positive"),
        ));
    }
    let h2 = height_mm / 2.0;
    let w_r = round(w, 3);
    let h2_r = round(h2, 3);
    let x_mid = w / 2.0;
    let mut pieces = Vec::with_capacity(2);
    for (pid, theta) in [("band_front", 0.0_f64), ("band_back", 180.0_f64)] {
        pieces.push(json!({
            "id": pid,
            "name": pid.to_ascii_uppercase(),
            "vertices": {
                "a": [0.0, -h2_r],
                "b": [w_r, -h2_r],
                "c": [w_r, h2_r],
                "d": [0.0, h2_r],
            },
            "edges": [
                {"id": "bottom", "from": "a", "to": "b"},
                {"id": "right", "from": "b", "to": "c"},
                {"id": "top", "from": "c", "to": "d"},
                {"id": "left", "from": "d", "to": "a"},
            ],
            "notches": [],
            "cut": {"qty": 1},
            "grain": [[x_mid, -20.0], [x_mid, 20.0]],
            "placement": {
                "part": "torso",
                "theta_deg": theta,
                "x0": x_mid,
                "y0_landmark": "chest",
                "y0": 0.0,
                "clearance_mm": 15,
            }
        }));
    }
    let default_fabric = json!({"id": "test", "kind": "woven", "areal_g_m2": 200});
    let fab = match fabric {
        Some(v) if v.is_object() => v.clone(),
        _ => default_fabric,
    };
    Ok(json!({
        "schema": SCHEMA,
        "style": format!("KAT_BAND_{ease_mm:+}"),
        "units": "mm",
        "meta": {
            "origin": "QUALIFICATION",
            "ease_mm": ease,
        },
        "fabric": fab,
        "pieces": pieces,
        "seams": [
            {"id": "side_R", "a": "band_front/left", "b": "band_back/right"},
            {"id": "side_L", "a": "band_front/right", "b": "band_back/left"},
        ]
    }))
}

/// Extracts `fabric_key = [areal_g_m2, tensile_N_m, bending_Nm]` of a pattern's `fabric` block,
/// or `Value::Null` when no fabric block is present (spec 04 §7.1, §16.8).
#[must_use]
pub fn fabric_key(fabric: Option<&Value>) -> Value {
    let Some(obj) = fabric.and_then(Value::as_object) else {
        return Value::Null;
    };
    json!([
        obj.get("areal_g_m2").cloned().unwrap_or(Value::Null),
        obj.get("tensile_N_m").cloned().unwrap_or(Value::Null),
        obj.get("bending_Nm").cloned().unwrap_or(Value::Null),
    ])
}

/// Evaluates one qualification band's `sim_report.json` against spec 04 §7.1.
///
/// Returns `(row_json, printed_line, passed)`.
#[must_use]
pub fn evaluate_band(ease_mm: i32, sim_report: &Value) -> (Value, String, bool) {
    let expected = match ease_mm {
        x if x < 0 => "TIGHT",
        0 => "SNUG_OR_TIGHT",
        _ => "EASY_OR_SNUG",
    };
    let lm = &sim_report["landmarks"]["chest"];
    let raw_reading = lm["reading"].as_str().unwrap_or("?");
    let mean_opt = lm["ring_strain_mean_pct"].as_f64();
    let shift_opt = lm["ring_shift_mm"].as_f64();
    let contact_opt = lm["contact_frac"].as_f64();
    let shift = shift_opt.unwrap_or(0.0).abs();

    let stretched = raw_reading == "TIGHT" && mean_opt.unwrap_or(0.0) > 2.0;
    let escaped = shift > 15.0 && contact_opt.unwrap_or(1.0) < 0.3;
    let reading = if escaped { "ESCAPED" } else { raw_reading };

    let good = match expected {
        "TIGHT" => stretched || escaped,
        "SNUG_OR_TIGHT" => matches!(reading, "SNUG" | "TIGHT") && shift <= 15.0,
        _ => matches!(reading, "EASY" | "SNUG") && mean_opt.unwrap_or(9.0) < 1.0 && shift <= 15.0,
    };

    let max_seam_gap_mm = sim_report["seams"].as_object().and_then(|seams| {
        seams
            .values()
            .filter_map(|s| s["gap_final_mm"].as_f64())
            .reduce(f64::max)
    });

    let ease_f = f64::from(ease_mm);
    let row = json!({
        "ease_mm": ease_f,
        "reading": reading,
        "ring_strain_mean_pct": lm.get("ring_strain_mean_pct").cloned().unwrap_or(Value::Null),
        "ring_shift_mm": lm.get("ring_shift_mm").cloned().unwrap_or(Value::Null),
        "contact_frac": lm.get("contact_frac").cloned().unwrap_or(Value::Null),
        "expected": expected,
        "pass": good,
        "max_seam_gap_mm": max_seam_gap_mm,
    });

    let fmt_opt = |v: Option<f64>| v.map_or_else(|| "None".to_string(), repr_f64);
    let line = format!(
        "ease {:+5.0} mm: {:7} mean ring strain {} %  contact {}  shift {} mm  -> {} (expected {expected})",
        ease_f,
        reading,
        fmt_opt(mean_opt),
        fmt_opt(contact_opt),
        fmt_opt(shift_opt),
        if good { "PASS" } else { "FAIL" },
    );
    (row, line, good)
}

/// Result of checking `WS/qualification/qualify.json` for a round (spec 04 §7.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualificationStatus {
    /// Whether tightness gating is qualified for this body and fabric.
    pub pass: bool,
    /// Human-readable reason recorded in `round` notes when tightness flags are demoted.
    pub why: String,
}

/// Checks `WS/qualification/qualify.json` in exact spec 04 §7.2 order.
#[must_use]
pub fn check_qualification(
    qualify_doc: Option<Result<&Value, &str>>,
    qualify_path_display: &str,
    ws_display: &str,
    measurements_sha256: &str,
    current_fabric: Option<&Value>,
) -> QualificationStatus {
    let Some(doc_res) = qualify_doc else {
        return QualificationStatus {
            pass: false,
            why: format!(
                "no qualification run in this workspace (`ftx qualify --ws {ws_display}`)"
            ),
        };
    };
    let Ok(rec) = doc_res else {
        return QualificationStatus {
            pass: false,
            why: format!(
                "{qualify_path_display} is unreadable (an interrupted run?): rerun `ftx qualify --ws {ws_display}`"
            ),
        };
    };
    if !rec.is_object() {
        return QualificationStatus {
            pass: false,
            why: format!(
                "{qualify_path_display} is unreadable (an interrupted run?): rerun `ftx qualify --ws {ws_display}`"
            ),
        };
    }
    if rec.get("pass").and_then(Value::as_bool) != Some(true) {
        return QualificationStatus {
            pass: false,
            why: "the qualification run did not pass: the sim cannot tell tight from easy here"
                .into(),
        };
    }
    if rec.get("measurements_sha256").and_then(Value::as_str) != Some(measurements_sha256) {
        return QualificationStatus {
            pass: false,
            why: "the measurements changed since the qualification run: rerun `ftx qualify`".into(),
        };
    }
    let expected_key = fabric_key(current_fabric);
    if rec.get("fabric_key").unwrap_or(&Value::Null) != &expected_key {
        return QualificationStatus {
            pass: false,
            why: "the fabric changed since the qualification run: rerun `ftx qualify`".into(),
        };
    }
    if let Some(engine) = rec.get("engine").and_then(Value::as_str)
        && engine != "ftx"
    {
        return QualificationStatus {
            pass: false,
            why: format!(
                "qualified on the {engine} engine but this round runs on ftx: rerun `ftx qualify`"
            ),
        };
    }
    QualificationStatus {
        pass: true,
        why: "qualified for this body and fabric (ftx engine)".into(),
    }
}

/// Applies spec 04 §7.3 tightness gating to `sim_report`.
///
/// 1. Sets `sim_report["tightness_qualified"] = qual.pass`.
/// 2. Finds `tight`: entries of `tightness_gating` that are also in `gating`.
/// 3. If `tight` is empty or `qual.pass` is true, returns `None`.
/// 4. Otherwise removes `tight` from `gating` (keeping them in `flags` and updating `advisory` if
///    present) and returns `Some("{n} tightness flag(s) kept advisory: {qual.why}")`.
pub fn gate_tightness(sim_report: &mut Value, qual: &QualificationStatus) -> Option<String> {
    sim_report["tightness_qualified"] = json!(qual.pass);
    let tight_candidates: Vec<String> = sim_report["tightness_gating"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let gating: Vec<String> = sim_report["gating"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let tight: Vec<String> = tight_candidates
        .into_iter()
        .filter(|g| gating.contains(g))
        .collect();
    if tight.is_empty() || qual.pass {
        return None;
    }
    let remaining_gating: Vec<String> = gating.into_iter().filter(|g| !tight.contains(g)).collect();
    sim_report["gating"] = json!(remaining_gating);
    if sim_report.get("advisory").is_some()
        && let Some(flags) = sim_report["flags"].as_array()
    {
        let advisory: Vec<String> = flags
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .filter(|f| !remaining_gating.contains(f))
            .collect();
        sim_report["advisory"] = json!(advisory);
    }
    Some(format!(
        "{} tightness flag(s) kept advisory: {}",
        tight.len(),
        qual.why
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_pattern::{Pattern, validate};

    fn sample_meas() -> Value {
        json!({
            "stature": 920.0,
            "chest": 540.0,
            "waist": 510.0,
            "hip": 550.0,
            "neck_base": 250.0,
            "shoulder_length": 72.0,
            "shoulder_slope_deg": 18.0,
            "across_back": 220.0,
            "back_neck_to_waist": 225.0,
            "armscye_depth": 110.0,
            "waist_height": 510.0,
            "crotch_height": 380.0,
            "knee_height": 235.0,
            "head": 495.0,
            "upper_arm": 160.0,
            "arm_length": 310.0,
            "chest_width": 175.0,
            "hip_width": 180.0
        })
    }

    #[test]
    fn band_pattern_flat_validates_to_chest_plus_ease_and_seams_walk() {
        let meas = sample_meas();
        let spec = band_fit_spec();
        for ease in QUALIFY_EASES {
            let raw = band_pattern(&meas, ease, DEFAULT_BAND_HEIGHT_MM, None).unwrap();
            assert_eq!(raw["style"], format!("KAT_BAND_{ease:+}"));
            assert_eq!(raw["meta"]["origin"], "QUALIFICATION");
            assert_eq!(raw["meta"]["ease_mm"], f64::from(ease));
            let pat = Pattern::new(raw);
            assert!(pat.structure_errors().is_empty());
            let rep = validate(&pat, &spec, &Measurements(meas.clone()));
            assert_eq!(rep.status(), "CONVERGED", "{rep:?}");
            let rep_val = serde_json::to_value(&rep).unwrap();
            let chest_pt = &rep_val["points"][0];
            let finished = chest_pt["finished_mm"].as_f64().unwrap();
            assert!((finished - (540.0 + f64::from(ease))).abs() <= 0.01);
            for s in rep_val["seams"].as_array().unwrap() {
                assert_eq!(s["status"], "PASS");
                assert!(s["residual_mm"].as_f64().unwrap().abs() <= 1e-6);
            }
        }
    }

    #[test]
    fn evaluate_band_matches_all_spec_pass_and_escape_rules() {
        // -20: stretched in place -> PASS
        let rep_tight = json!({
            "landmarks": {"chest": {"reading": "TIGHT", "ring_strain_mean_pct": 2.45, "ring_shift_mm": -3.2, "contact_frac": 0.95}},
            "seams": {"side_R": {"gap_final_mm": 0.4}, "side_L": {"gap_final_mm": 0.7}}
        });
        let (row, line, pass) = evaluate_band(-20, &rep_tight);
        assert!(pass, "{line}");
        assert_eq!(row["reading"], "TIGHT");
        assert_eq!(row["max_seam_gap_mm"], 0.7);

        // -20: escaped off landmark -> reading becomes ESCAPED, PASS
        let rep_escaped = json!({
            "landmarks": {"chest": {"reading": "EASY", "ring_strain_mean_pct": 0.1, "ring_shift_mm": 22.0, "contact_frac": 0.0}},
            "seams": {"side_R": {"gap_final_mm": 0.2}}
        });
        let (row, line, pass) = evaluate_band(-20, &rep_escaped);
        assert!(pass, "{line}");
        assert_eq!(row["reading"], "ESCAPED");

        // 0: SNUG in place -> PASS; shifted > 15 mm -> FAIL
        let rep_snug = json!({
            "landmarks": {"chest": {"reading": "SNUG", "ring_strain_mean_pct": 0.8, "ring_shift_mm": -4.0, "contact_frac": 0.92}},
            "seams": {}
        });
        assert!(evaluate_band(0, &rep_snug).2);
        let rep_snug_shifted = json!({
            "landmarks": {"chest": {"reading": "SNUG", "ring_strain_mean_pct": 0.8, "ring_shift_mm": 18.0, "contact_frac": 0.85}},
            "seams": {}
        });
        assert!(!evaluate_band(0, &rep_snug_shifted).2);

        // +20: EASY or SNUG with mean < 1.0 and shift <= 15 -> PASS; mean >= 1.0 -> FAIL
        let rep_easy = json!({
            "landmarks": {"chest": {"reading": "EASY", "ring_strain_mean_pct": 0.2, "ring_shift_mm": -2.5, "contact_frac": 0.45}},
            "seams": {}
        });
        assert!(evaluate_band(20, &rep_easy).2);
        let rep_snug_high = json!({
            "landmarks": {"chest": {"reading": "SNUG", "ring_strain_mean_pct": 1.05, "ring_shift_mm": -2.5, "contact_frac": 0.75}},
            "seams": {}
        });
        assert!(!evaluate_band(20, &rep_snug_high).2);
    }

    #[test]
    fn qualification_checks_every_condition_in_spec_order() {
        let fab = json!({"areal_g_m2": 260, "tensile_N_m": 8000, "bending_Nm": 3e-5});
        let q = check_qualification(
            None,
            "ws/qualification/qualify.json",
            "ws",
            "abc",
            Some(&fab),
        );
        assert!(!q.pass);
        assert!(q.why.contains("no qualification run"));

        let q = check_qualification(
            Some(Err("bad json")),
            "ws/qualification/qualify.json",
            "ws",
            "abc",
            Some(&fab),
        );
        assert!(!q.pass);
        assert!(q.why.contains("is unreadable"));

        let failed_doc =
            json!({"pass": false, "measurements_sha256": "abc", "fabric_key": [260, 8000, 3e-5]});
        let q = check_qualification(
            Some(Ok(&failed_doc)),
            "ws/qualification/qualify.json",
            "ws",
            "abc",
            Some(&fab),
        );
        assert!(!q.pass);
        assert!(q.why.contains("did not pass"));

        let passed_doc = json!({"pass": true, "measurements_sha256": "abc", "fabric_key": [260, 8000, 3e-5], "engine": "ftx"});
        let q = check_qualification(
            Some(Ok(&passed_doc)),
            "ws/qualification/qualify.json",
            "ws",
            "different_sha",
            Some(&fab),
        );
        assert!(!q.pass);
        assert!(q.why.contains("measurements changed"));

        let other_fab = json!({"areal_g_m2": 200, "tensile_N_m": 8000, "bending_Nm": 3e-5});
        let q = check_qualification(
            Some(Ok(&passed_doc)),
            "ws/qualification/qualify.json",
            "ws",
            "abc",
            Some(&other_fab),
        );
        assert!(!q.pass);
        assert!(q.why.contains("fabric changed"));

        let ok = check_qualification(
            Some(Ok(&passed_doc)),
            "ws/qualification/qualify.json",
            "ws",
            "abc",
            Some(&fab),
        );
        assert!(ok.pass);
        assert!(ok.why.contains("qualified for this body and fabric"));
    }

    #[test]
    fn gate_tightness_demotes_only_when_unqualified_and_records_note() {
        let flag = "chest: fabric stretched around the body (mean 2.5 %, p90 3.4 %)";
        let other = "seam side_R did not close (6.2 mm): check its placement and lengths";
        let mut sim = json!({
            "flags": [flag, other],
            "gating": [flag, other],
            "tightness_gating": [flag],
            "advisory": []
        });
        let unqual = QualificationStatus {
            pass: false,
            why: "no qualification run in this workspace (`ftx qualify --ws ws`)".into(),
        };
        let note = gate_tightness(&mut sim, &unqual).unwrap();
        assert_eq!(sim["tightness_qualified"], false);
        assert_eq!(sim["gating"], json!([other]));
        assert_eq!(sim["flags"], json!([flag, other]));
        assert_eq!(sim["advisory"], json!([flag]));
        assert!(note.starts_with("1 tightness flag(s) kept advisory: no qualification run"));

        // When qualified, tightness stays in gating.
        let mut sim_qual = json!({
            "flags": [flag],
            "gating": [flag],
            "tightness_gating": [flag],
            "advisory": []
        });
        let qual = QualificationStatus {
            pass: true,
            why: "qualified for this body and fabric (ftx engine)".into(),
        };
        assert!(gate_tightness(&mut sim_qual, &qual).is_none());
        assert_eq!(sim_qual["tightness_qualified"], true);
        assert_eq!(sim_qual["gating"], json!([flag]));
    }
}
