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
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    contact::ContactParams,
    material::{Fabric, MaterialInputs},
    run::DrapeBody,
};

/// The three qualification chest-band eases (mm, spec 04 §7.1).
pub const QUALIFY_EASES: [i32; 3] = [-20, 0, 20];

/// Zero-gravity sewing steps for each qualification band (`--sew-frames 70`).
pub const QUALIFY_SEW_STEPS: usize = 70;

/// Gravity drape steps for each qualification band (`--drape-frames 0`).
pub const QUALIFY_DRAPE_STEPS: usize = 0;

/// Default height of each chest band (mm).
pub const DEFAULT_BAND_HEIGHT_MM: f64 = 60.0;

/// Maximum absolute collider-vs-target girth residual (mm) permitted before a qualified collider
/// tightness flag is demoted back to `advisory` on a wearer round (25 % of one 20 mm band step).
pub const MAX_QUALIFIED_GIRTH_RESIDUAL_MM: f64 = 5.0;

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
    let chest = Measurements(meas.clone())
        .value("chest")
        .map_err(|e| bad_input(format!("band_pattern needs chest measurement ({e})")))?;
    band_pattern_from_girth(chest, ease_mm, height_mm, fabric)
}

/// Builds the same band from an explicit chest section girth in millimetres.
///
/// # Errors
/// Refuses invalid chest girth, band width or height.
pub fn band_pattern_from_girth(
    chest_mm: f64,
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
    Refusal::finite("chest girth", chest_mm)?;
    if chest_mm <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "chest girth must be positive",
        ));
    }
    let ease = f64::from(ease_mm);
    let w = (chest_mm + ease) / 2.0;
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
    /// Whether wearer-blocking tightness gating is qualified for this body, target and fabric.
    pub pass: bool,
    /// Whether the posed collider and sheet material(s) passed the three chest-band separation checks.
    pub collider_pass: bool,
    /// Human-readable reason recorded in `round` notes when tightness flags are demoted.
    pub why: String,
}

impl QualificationStatus {
    /// Unqualified status (neither collider nor wearer tightness is qualified).
    #[must_use]
    pub fn unqualified(why: impl Into<String>) -> Self {
        Self {
            pass: false,
            collider_pass: false,
            why: why.into(),
        }
    }
}

fn digest(value: &impl Serialize) -> Result<String, Refusal> {
    serde_json::to_vec(value)
        .map(|bytes| ftx_core::receipt::sha256_hex(&bytes))
        .map_err(|error| bad_input(error.to_string()))
}

fn sheet_material_sha256(
    record_sha256: &Value,
    fabric: Fabric,
    compression: f64,
    contact: ContactParams,
) -> Result<String, Refusal> {
    digest(&json!([
        record_sha256,
        fabric.areal_kg_m2,
        fabric.tensile_n_m,
        fabric.bending_nm,
        compression,
        contact.thickness,
        contact.mu,
        contact.dhat,
        contact.kappa,
        contact.eps_v
    ]))
}

/// Returns the active simulated per-piece fabric selectors (`(piece_id, fabric_selector)`) when
/// `pattern` enters per-piece characterization (`piece.fabric` present on any piece or
/// `fabric.interfaces` present, with at least one active piece carrying a source-bearing selector).
/// Returns an empty vector for uniform patterns.
#[must_use]
pub fn active_piece_fabric_selectors(pattern: &Value) -> Vec<(String, Value)> {
    let Some(pieces) = pattern["pieces"].as_array() else {
        return Vec::new();
    };
    let has_per_piece = pieces.iter().any(|p| p.get("fabric").is_some())
        || pattern["fabric"].get("interfaces").is_some();
    if !has_per_piece {
        return Vec::new();
    }
    let active_has_source = pieces.iter().filter(|p| p["sim"] != false).any(|p| {
        let sel = p.get("fabric").unwrap_or(&pattern["fabric"]);
        sel.get("material").is_some() || sel.get("material_id").is_some()
    });
    if !active_has_source {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (idx, piece) in pieces.iter().enumerate() {
        if piece["sim"] == false {
            continue;
        }
        let pid = piece["id"]
            .as_str()
            .map_or_else(|| format!("piece_{idx}"), String::from);
        let sel = piece.get("fabric").unwrap_or(&pattern["fabric"]).clone();
        out.push((pid, sel));
    }
    out
}

/// Identity of the posed contact body and either the uniform fabric (`material_sha256`) or all
/// active simulated piece materials (`piece_material_sha256s`) exercised by chest-band qualification.
/// Inputs must be the body and material admitted for the drape.
///
/// # Errors
/// Missing torso/chest frames, missing per-piece metadata, or serialization failure.
pub fn qualification_basis(
    body: &DrapeBody<'_>,
    material: &MaterialInputs,
) -> Result<Value, Refusal> {
    let frame = body
        .frames
        .frame("torso")
        .ok_or_else(|| bad_input("qualification needs the torso frame"))?;
    let chest = body
        .frames
        .landmark_h("torso", "chest")
        .ok_or_else(|| bad_input("qualification needs the chest landmark"))?;
    let body_sha256 = digest(&(
        body.collision,
        body.arm_deg,
        [frame.origin, frame.up, frame.front, frame.lat],
        chest,
    ))?;
    if let Some(piece_fabrics) = &material.piece_fabrics {
        let order = material.report["contact_material_order"]
            .as_array()
            .ok_or_else(|| {
                bad_input("per-piece material report is missing contact_material_order")
            })?;
        let bindings = material.report["pieces"]
            .as_array()
            .ok_or_else(|| bad_input("per-piece material report is missing piece bindings"))?;
        let mut shas = Vec::with_capacity(order.len());
        for (idx, entry) in order.iter().enumerate() {
            let record_sha = entry
                .as_str()
                .filter(|s| s.len() == 64)
                .ok_or_else(|| bad_input("invalid per-piece record_sha256"))?;
            let piece_index = bindings
                .iter()
                .find(|b| {
                    b["mechanics_consumed"] != json!(false)
                        && b["record_sha256"].as_str() == Some(record_sha)
                })
                .and_then(|b| b["piece_index"].as_u64())
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| bad_input("active per-piece material has no bound source piece"))?;
            let piece_fabric = piece_fabrics
                .get(piece_index)
                .and_then(|f| *f)
                .ok_or_else(|| bad_input("active per-piece material is missing sheet mechanics"))?;
            let mut piece_contact = material.options.contact;
            if let Some(cm) = &material.contact_materials {
                let surf = cm
                    .surfaces
                    .get(idx)
                    .ok_or_else(|| bad_input("missing per-piece contact surface"))?;
                piece_contact.thickness = surf.half_thickness_m;
                piece_contact.mu = surf.friction_body;
            }
            shas.push(sheet_material_sha256(
                &json!(record_sha),
                piece_fabric,
                material.options.compression,
                piece_contact,
            )?);
        }
        shas.sort_unstable();
        shas.dedup();
        return Ok(json!({
            "body_sha256": body_sha256,
            "material_sha256": Value::Null,
            "piece_material_sha256s": shas,
        }));
    }
    let material_sha256 = sheet_material_sha256(
        &material.report["record_sha256"],
        material.fabric,
        material.options.compression,
        material.options.contact,
    )?;
    Ok(json!({"body_sha256": body_sha256, "material_sha256": Some(material_sha256)}))
}

fn is_sha256(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Checks spec 04 §7.2 followed by native posed-body, uniform/per-piece material identity, and
/// collider-vs-target chest girth alignment (`<= ±5.0 mm`).
#[must_use]
pub fn check_qualification(
    qualify_doc: Option<Result<&Value, &str>>,
    qualify_path_display: &str,
    ws_display: &str,
    measurements_sha256: &str,
    current_fabric: Option<&Value>,
    current_basis: &Value,
) -> QualificationStatus {
    let Some(doc_res) = qualify_doc else {
        return QualificationStatus::unqualified(format!(
            "no qualification run in this workspace (`ftx qualify --ws {ws_display}`)"
        ));
    };
    let Ok(rec) = doc_res else {
        return QualificationStatus::unqualified(format!(
            "{qualify_path_display} is unreadable (an interrupted run?): rerun `ftx qualify --ws {ws_display}`"
        ));
    };
    if !rec.is_object() {
        return QualificationStatus::unqualified(format!(
            "{qualify_path_display} is unreadable (an interrupted run?): rerun `ftx qualify --ws {ws_display}`"
        ));
    }
    if rec.get("pass").and_then(Value::as_bool) != Some(true) {
        return QualificationStatus::unqualified(
            "the qualification run did not pass: the sim cannot tell tight from easy here",
        );
    }
    if rec.get("measurements_sha256").and_then(Value::as_str) != Some(measurements_sha256) {
        return QualificationStatus::unqualified(
            "the measurements changed since the qualification run: rerun `ftx qualify`",
        );
    }
    let per_piece_shas = current_basis
        .get("piece_material_sha256s")
        .and_then(Value::as_array);
    if let Some(required_shas) = per_piece_shas {
        if rec["engine"] != "ftx" {
            return QualificationStatus::unqualified(
                "qualification needs the ftx engine and current body/material identities: rerun `ftx qualify`",
            );
        }
        if !is_sha256(&current_basis["body_sha256"])
            || rec["qualification_basis"]["body_sha256"] != current_basis["body_sha256"]
        {
            return QualificationStatus::unqualified(
                "the posed collision body is unqualified or changed since qualification: rerun `ftx qualify` with this body and fabric",
            );
        }
        if required_shas.is_empty() || required_shas.iter().any(|v| !is_sha256(v)) {
            return QualificationStatus::unqualified(
                "this round has invalid per-piece material qualification identities: rerun `ftx qualify`",
            );
        }
        let Some(recorded_shas) = rec["qualification_basis"]
            .get("piece_material_sha256s")
            .and_then(Value::as_array)
        else {
            return QualificationStatus::unqualified(
                "per-piece assemblies require per-piece qualification coverage: rerun `ftx qualify` with this pattern",
            );
        };
        let piece_records = rec.get("piece_materials").and_then(Value::as_array);
        for req in required_shas {
            let covered = recorded_shas.contains(req)
                && piece_records.is_none_or(|entries| {
                    entries.iter().any(|entry| {
                        &entry["material_sha256"] == req
                            && entry.get("pass").and_then(Value::as_bool) == Some(true)
                    })
                });
            if !covered {
                return QualificationStatus::unqualified(
                    "a piece material or body-contact policy is unqualified or changed since qualification: rerun `ftx qualify` with this pattern, body and fabric",
                );
            }
        }
    } else {
        let expected_key = fabric_key(current_fabric);
        if rec.get("fabric_key").unwrap_or(&Value::Null) != &expected_key {
            return QualificationStatus::unqualified(
                "the fabric changed since the qualification run: rerun `ftx qualify`",
            );
        }
        if rec["engine"] != "ftx" {
            return QualificationStatus::unqualified(
                "qualification needs the ftx engine and current body/material identities: rerun `ftx qualify`",
            );
        }
        if current_basis["material_sha256"].is_null() {
            return QualificationStatus::unqualified(
                "this round has no uniform material qualification identity; per-piece assemblies need separate qualification coverage",
            );
        }
        for (key, label) in [
            ("body_sha256", "posed collision body"),
            ("material_sha256", "uniform material/body-contact policy"),
        ] {
            if !is_sha256(&current_basis[key])
                || rec["qualification_basis"][key] != current_basis[key]
            {
                return QualificationStatus::unqualified(format!(
                    "the {label} is unqualified or changed since qualification: rerun `ftx qualify` with this body and fabric"
                ));
            }
        }
    }

    if rec.get("posed_chest_girth_mm").is_some() {
        let Some(posed) = rec["posed_chest_girth_mm"]
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.0)
        else {
            return QualificationStatus {
                pass: false,
                collider_pass: true,
                why: "collider bands passed, but posed_chest_girth_mm is invalid; wearer tightness stays advisory".into(),
            };
        };
        let Some(target) = rec
            .get("input_chest_mm")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v > 0.0)
        else {
            return QualificationStatus {
                pass: false,
                collider_pass: true,
                why: "collider bands passed, but chest target is absent from measurements so wearer tightness stays advisory".into(),
            };
        };
        let residual = posed - target;
        if residual.abs() > MAX_QUALIFIED_GIRTH_RESIDUAL_MM {
            return QualificationStatus {
                pass: false,
                collider_pass: true,
                why: format!(
                    "collider bands passed, but posed chest girth ({posed:.1} mm) differs from input chest ({target:.1} mm) by {residual:+.1} mm (max ±{MAX_QUALIFIED_GIRTH_RESIDUAL_MM:.1} mm); wearer tightness stays advisory"
                ),
            };
        }
    }

    QualificationStatus {
        pass: true,
        collider_pass: true,
        why: "qualified for this body and fabric (ftx engine)".into(),
    }
}

fn landmark_body_residual_issue(
    flag: &str,
    sim_report: &Value,
    fit_spec: Option<&Value>,
) -> Option<String> {
    let (point_id, _) = flag.split_once(": fabric stretched around the body (")?;
    let body_key = fit_spec
        .and_then(|spec| spec["points"].as_array())
        .and_then(|pts| {
            pts.iter()
                .find(|p| p["id"].as_str() == Some(point_id))
                .and_then(|p| p["body"].as_str())
        })
        .unwrap_or(point_id);
    if !ftx_body::is_girth(body_key) {
        return None;
    }
    let body = sim_report.get("body")?;
    if let Some(tape) = body["fit"]["tape"].as_object() {
        let Some(entry) = tape.get(body_key) else {
            return Some(format!(
                "{body_key} was not fitted to a measured target (MODEL_FIT prior)"
            ));
        };
        let Some(res) = entry["residual_mm"].as_f64().filter(|v| v.is_finite()) else {
            return Some(format!("{body_key} has no finite body-fit residual"));
        };
        if res.abs() > MAX_QUALIFIED_GIRTH_RESIDUAL_MM {
            return Some(format!(
                "{body_key} body-fit residual ({res:+.1} mm) exceeds ±{MAX_QUALIFIED_GIRTH_RESIDUAL_MM:.1} mm"
            ));
        }
        return None;
    }
    if let Some(cmp) = body["measurement_comparison"].as_array() {
        let Some(entry) = cmp.iter().find(|c| c["name"].as_str() == Some(body_key)) else {
            return Some(format!(
                "{body_key} has no measurement comparison on the accepted body shape"
            ));
        };
        let Some(res) = entry["residual_mm"].as_f64().filter(|v| v.is_finite()) else {
            return Some(format!(
                "{body_key} has no finite measurement comparison residual"
            ));
        };
        if res.abs() > MAX_QUALIFIED_GIRTH_RESIDUAL_MM {
            return Some(format!(
                "{body_key} accepted-shape residual ({res:+.1} mm) exceeds ±{MAX_QUALIFIED_GIRTH_RESIDUAL_MM:.1} mm"
            ));
        }
    }
    None
}

fn rewrite_gating_and_advisory(sim_report: &mut Value, demote: &[String]) {
    let gating: Vec<String> = sim_report["gating"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .filter(|g| !demote.contains(g))
        .collect();
    sim_report["gating"] = json!(gating);
    if sim_report.get("advisory").is_some()
        && let Some(flags) = sim_report["flags"].as_array()
    {
        let advisory: Vec<String> = flags
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .filter(|f| !gating.contains(f))
            .collect();
        sim_report["advisory"] = json!(advisory);
    }
}

/// Applies spec 04 §7.3 tightness gating to `sim_report` without an explicit fit specification.
pub fn gate_tightness(sim_report: &mut Value, qual: &QualificationStatus) -> Option<String> {
    gate_tightness_with_spec(sim_report, qual, None)
}

/// Applies spec 04 §7.3 tightness gating and per-landmark body-residual checks to `sim_report`.
pub fn gate_tightness_with_spec(
    sim_report: &mut Value,
    qual: &QualificationStatus,
    fit_spec: Option<&Value>,
) -> Option<String> {
    sim_report["tightness_qualified"] = json!(qual.pass);
    sim_report["collider_tightness_qualified"] = json!(qual.collider_pass);
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
    if tight.is_empty() {
        return None;
    }
    if !qual.pass {
        rewrite_gating_and_advisory(sim_report, &tight);
        return Some(format!(
            "{} tightness flag(s) kept advisory: {}",
            tight.len(),
            qual.why
        ));
    }
    let mut demoted = Vec::new();
    let mut reasons = Vec::new();
    for flag in &tight {
        if let Some(why) = landmark_body_residual_issue(flag, sim_report, fit_spec) {
            demoted.push(flag.clone());
            reasons.push(why);
        }
    }
    if demoted.is_empty() {
        return None;
    }
    rewrite_gating_and_advisory(sim_report, &demoted);
    Some(format!(
        "{} tightness flag(s) kept advisory: {}",
        demoted.len(),
        reasons.join("; ")
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
        let basis = json!({"body_sha256":"b".repeat(64),"material_sha256":"a".repeat(64)});
        let q = check_qualification(
            None,
            "ws/qualification/qualify.json",
            "ws",
            "abc",
            Some(&fab),
            &basis,
        );
        assert!(!q.pass);
        assert!(q.why.contains("no qualification run"));

        let q = check_qualification(
            Some(Err("bad json")),
            "ws/qualification/qualify.json",
            "ws",
            "abc",
            Some(&fab),
            &basis,
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
            &basis,
        );
        assert!(!q.pass);
        assert!(q.why.contains("did not pass"));

        let passed_doc = json!({"pass": true, "measurements_sha256": "abc", "fabric_key": [260, 8000, 3e-5], "engine": "ftx", "qualification_basis":basis});
        let q = check_qualification(
            Some(Ok(&passed_doc)),
            "ws/qualification/qualify.json",
            "ws",
            "different_sha",
            Some(&fab),
            &basis,
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
            &basis,
        );
        assert!(!q.pass);
        assert!(q.why.contains("fabric changed"));

        let ok = check_qualification(
            Some(Ok(&passed_doc)),
            "ws/qualification/qualify.json",
            "ws",
            "abc",
            Some(&fab),
            &basis,
        );
        assert!(ok.pass);
        assert!(ok.why.contains("qualified for this body and fabric"));

        for changed in [
            json!({}),
            json!({"body_sha256":"c".repeat(64),"material_sha256":basis["material_sha256"]}),
            json!({"body_sha256":basis["body_sha256"],"material_sha256":"d".repeat(64)}),
            json!({"body_sha256":basis["body_sha256"],"material_sha256":null}),
        ] {
            let result = check_qualification(
                Some(Ok(&passed_doc)),
                "ws/qualification/qualify.json",
                "ws",
                "abc",
                Some(&fab),
                &changed,
            );
            assert!(!result.pass, "{changed}");
        }
        let mut legacy = passed_doc.clone();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("qualification_basis");
        assert!(!check_qualification(Some(Ok(&legacy)), "q", "ws", "abc", Some(&fab), &basis).pass);
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
            collider_pass: false,
            why: "no qualification run in this workspace (`ftx qualify --ws ws`)".into(),
        };
        let note = gate_tightness(&mut sim, &unqual).unwrap();
        assert_eq!(sim["tightness_qualified"], false);
        assert_eq!(sim["collider_tightness_qualified"], false);
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
            collider_pass: true,
            why: "qualified for this body and fabric (ftx engine)".into(),
        };
        assert!(gate_tightness(&mut sim_qual, &qual).is_none());
        assert_eq!(sim_qual["tightness_qualified"], true);
        assert_eq!(sim_qual["collider_tightness_qualified"], true);
        assert_eq!(sim_qual["gating"], json!([flag]));
    }

    #[test]
    fn qualification_gates_chest_girth_residual_against_target() {
        let fab = json!({"areal_g_m2": 260, "tensile_N_m": 8000, "bending_Nm": 3e-5});
        let basis = json!({"body_sha256":"b".repeat(64),"material_sha256":"a".repeat(64)});
        let aligned = json!({
            "pass": true,
            "measurements_sha256": "abc",
            "fabric_key": [260, 8000, 3e-5],
            "engine": "ftx",
            "qualification_basis": basis,
            "posed_chest_girth_mm": 536.2,
            "input_chest_mm": 540.0
        });
        let ok = check_qualification(Some(Ok(&aligned)), "q", "ws", "abc", Some(&fab), &basis);
        assert!(ok.pass && ok.collider_pass, "{ok:?}");

        let unaligned = json!({
            "pass": true,
            "measurements_sha256": "abc",
            "fabric_key": [260, 8000, 3e-5],
            "engine": "ftx",
            "qualification_basis": basis,
            "posed_chest_girth_mm": 520.44,
            "input_chest_mm": 540.0
        });
        let demoted =
            check_qualification(Some(Ok(&unaligned)), "q", "ws", "abc", Some(&fab), &basis);
        assert!(demoted.collider_pass);
        assert!(!demoted.pass);
        assert!(demoted.why.contains("-19.6 mm") && demoted.why.contains("±5.0 mm"));

        let missing_target = json!({
            "pass": true,
            "measurements_sha256": "abc",
            "fabric_key": [260, 8000, 3e-5],
            "engine": "ftx",
            "qualification_basis": basis,
            "posed_chest_girth_mm": 540.0,
            "input_chest_mm": null
        });
        let missing = check_qualification(
            Some(Ok(&missing_target)),
            "q",
            "ws",
            "abc",
            Some(&fab),
            &basis,
        );
        assert!(missing.collider_pass);
        assert!(!missing.pass);
        assert!(missing.why.contains("chest target is absent"));
    }

    #[test]
    fn qualification_validates_per_piece_fabric_assemblies() {
        let m1 = "1".repeat(64);
        let m2 = "2".repeat(64);
        let body_sha = "b".repeat(64);
        let basis = json!({
            "body_sha256": body_sha,
            "material_sha256": null,
            "piece_material_sha256s": [m1, m2]
        });
        let doc = json!({
            "pass": true,
            "measurements_sha256": "abc",
            "fabric_key": [null, null, null],
            "engine": "ftx",
            "qualification_basis": basis,
            "posed_chest_girth_mm": 538.5,
            "input_chest_mm": 540.0,
            "piece_materials": [
                {"material_sha256": m1, "pass": true},
                {"material_sha256": m2, "pass": true}
            ]
        });
        let ok = check_qualification(Some(Ok(&doc)), "q", "ws", "abc", None, &basis);
        assert!(ok.pass && ok.collider_pass, "{ok:?}");

        let missing_second = json!({
            "body_sha256": body_sha,
            "material_sha256": null,
            "piece_material_sha256s": [m1, "3".repeat(64)]
        });
        assert!(!check_qualification(Some(Ok(&doc)), "q", "ws", "abc", None, &missing_second).pass);

        let mut failed_piece = doc.clone();
        failed_piece["piece_materials"][1]["pass"] = json!(false);
        assert!(!check_qualification(Some(Ok(&failed_piece)), "q", "ws", "abc", None, &basis).pass);
    }

    #[test]
    fn gate_tightness_with_spec_demotes_unaligned_landmark_body_residuals() {
        let chest_flag = "chest: fabric stretched around the body (mean 2.3 %, p90 3.1 %)";
        let hip_flag = "hip_line: fabric stretched around the body (mean 2.6 %, p90 3.5 %)";
        let fit_spec = json!({
            "points": [
                {"id": "chest", "body": "chest"},
                {"id": "hip_line", "body": "hip"}
            ]
        });
        let qual = QualificationStatus {
            pass: true,
            collider_pass: true,
            why: "qualified for this body and fabric (ftx engine)".into(),
        };
        let mut sim = json!({
            "flags": [chest_flag, hip_flag],
            "gating": [chest_flag, hip_flag],
            "tightness_gating": [chest_flag, hip_flag],
            "advisory": [],
            "body": {
                "fit": {
                    "tape": {
                        "chest": {"residual_mm": -1.8},
                        "hip": {"residual_mm": -9.4}
                    }
                }
            }
        });
        let note = gate_tightness_with_spec(&mut sim, &qual, Some(&fit_spec)).unwrap();
        assert_eq!(sim["tightness_qualified"], true);
        assert_eq!(sim["collider_tightness_qualified"], true);
        assert_eq!(sim["gating"], json!([chest_flag]));
        assert_eq!(sim["advisory"], json!([hip_flag]));
        assert!(note.contains("hip body-fit residual (-9.4 mm) exceeds ±5.0 mm"));
    }
}
