//! `zouave_trousers` (spec 02 §C.3): full legs gathered into cuffs, an elastic casing.

use std::f64::consts::PI;

use ftx_core::py::{fixed, round};
use ftx_core::{Refusal, RefusalCode, Vec2};
use serde_json::{Map, Value, json};

use super::builder::{
    PieceBuilder, SCHEMA, bisect, merge_style, mirror_seams, num, sf, size_label, unit,
};
use super::form::{Meas, frame, landmark_h, landmarks, section_radius};
use crate::Pattern;

/// The trousers' style defaults (`TROUSER_DEFAULTS`).
#[must_use]
pub fn defaults() -> Value {
    json!({
        "seat_ease": 80.0, "fullness": 60.0, "rise_ease": 15.0, "nappy": false, "front_ext_frac": 0.065,
        "crotch_ease": 30.0, "back_rise_extra": 20.0, "cb_slant": 15.0, "cuff_height": 30.0, "cuff_level": "calf",
        "cuff_ease": 25.0, "leg_gather": 1.8, "blouse_drop": 40.0, "casing_height": 30.0, "elastic_frac": 0.95,
        "elastic_overlap": 20.0
    })
}

fn bad(m: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, m)
}

fn edge_len(p: &Pattern, inst: &str, edge: &str) -> Result<f64, Refusal> {
    p.instance(inst)
        .ok_or_else(|| bad(format!("no instance {inst}")))?
        .edge_length(edge)
        .map_err(|e| bad(e.message().to_string()))
}

fn one(pieces: Vec<Value>) -> Pattern {
    Pattern::new(json!({"schema": SCHEMA, "units": "mm", "pieces": pieces, "seams": []}))
}

/// `zouave_trousers(meas, style)`: the pattern and its fit spec.
///
/// # Errors
/// An unknown style key; a missing measurement; geometry that cannot be drafted.
#[allow(clippy::too_many_lines)]
pub fn zouave_trousers(meas: &Value, style: Option<&Value>) -> Result<(Value, Value), Refusal> {
    let s = merge_style(&defaults(), style)?;
    let m = Meas(meas);
    let l = landmarks(m)?;
    let nappy = crate::doc::truthy(&s["nappy"]);
    let nappy_mm = if nappy { 20.0 } else { 0.0 };
    let seat_name = if nappy { "seat_over_nappy" } else { "hip" };
    let seat = m.get(seat_name)?;
    let quarter = (seat + sf(&s, "seat_ease")?) / 4.0;
    let fullness = sf(&s, "fullness")?;
    let (wf, wb) = (quarter - 10.0 + fullness, quarter + 10.0 + fullness);
    let y_hip = l.get("hip")? - l.get("waist")?;
    let y_cr = -(m.get("body_rise")? + sf(&s, "rise_ease")? + nappy_mm);
    let ext_f = sf(&s, "front_ext_frac")? * seat;
    let cuff_level = s["cuff_level"]
        .as_str()
        .ok_or_else(|| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                "style cuff_level must be a string",
            )
        })?
        .to_string();
    let cuff_body = m.get(&cuff_level)?;
    let cuff_len = (cuff_body + sf(&s, "cuff_ease")?).max(m.get("foot_pass")? + 15.0);
    let gather = sf(&s, "leg_gather")?;
    let hem_total = cuff_len * gather;
    let (hem_f, hem_b) = (0.47 * hem_total, 0.53 * hem_total);
    let cuff_h = sf(&s, "cuff_height")?;
    let z_cuff_top = l.get(&cuff_level)? + cuff_h / 2.0;
    let y_hem = (z_cuff_top - l.get("waist")?) - sf(&s, "blouse_drop")?;
    let leg_fr = frame(m, "leg_R")?;
    let h_waist = landmark_h(m, "leg_R", "waist")?;
    let mut r_wrap = f64::NEG_INFINITY;
    for y in ftx_core::np::linspace(y_hem, 20.0, 12) {
        r_wrap = r_wrap.max(section_radius(m, "leg_R", h_waist + y)?);
    }
    r_wrap += 22.0;
    let mid_deg = (1.0f64.min(leg_fr.origin.x.abs() / r_wrap))
        .asin()
        .to_degrees();

    let front_piece = || -> Result<Value, Refusal> {
        let mut p = PieceBuilder::new(
            "tr_front",
            "TROUSER FRONT",
            json!({"material": "trouser", "cut": {"qty": 2}, "grain": null}),
        );
        let crease = (wf + ext_f) / 2.0;
        p.v("hem_side", crease - hem_f / 2.0, y_hem);
        p.v("hem_in", crease + hem_f / 2.0, y_hem);
        p.v("crotch_pt", wf + ext_f, y_cr);
        p.v("cf_hip", wf, y_hip);
        p.v("waist_cf", wf, 0.0);
        p.v("waist_side", 0.0, 0.0);
        p.line("hem", "hem_side", "hem_in");
        p.line("inseam", "hem_in", "crotch_pt");
        p.cubic_abs(
            "crotch",
            "crotch_pt",
            "cf_hip",
            Vec2::new(wf + 0.45 * ext_f, y_cr),
            Vec2::new(wf, y_hip - 0.45 * (y_hip - y_cr)),
            None,
        )?;
        p.line("cf", "cf_hip", "waist_cf");
        p.line("waist", "waist_cf", "waist_side");
        p.line("outseam", "waist_side", "hem_side");
        p.set(
            "grain",
            json!([[crease, y_hem + 60.0], [crease, y_cr - 20.0]]),
        );
        p.set(
            "placement",
            json!({"part": "leg", "theta_deg": round(mid_deg, 2), "x0": wf, "y0_landmark": "waist",
                   "radius_mm": round(r_wrap, 1)}),
        );
        p.set("role", json!("pant_front"));
        Ok(p.build())
    };
    let (cb_slant, rise_extra) = (sf(&s, "cb_slant")?, sf(&s, "back_rise_extra")?);
    let back_piece = |ext_b: f64, hem_dy: f64, hem_dx: f64| -> Result<Value, Refusal> {
        let mut p = PieceBuilder::new(
            "tr_back",
            "TROUSER BACK",
            json!({"material": "trouser", "cut": {"qty": 2}, "grain": null}),
        );
        let crease = (wb - ext_b) / 2.0 + hem_dx;
        p.v("hem_in", crease - hem_b / 2.0, y_hem + hem_dy);
        p.v("hem_side", crease + hem_b / 2.0, y_hem + hem_dy);
        p.v("waist_side", wb, 0.0);
        p.v("waist_cb", cb_slant, rise_extra);
        p.v("cb_hip", 0.0, y_hip);
        p.v("crotch_pt", -ext_b, y_cr);
        p.line("hem", "hem_in", "hem_side");
        p.line("outseam", "hem_side", "waist_side");
        p.line("waist", "waist_side", "waist_cb");
        p.line("cb", "waist_cb", "cb_hip");
        let cb_dir = unit(Vec2::new(0.0, y_hip) - Vec2::new(cb_slant, rise_extra));
        p.cubic_abs(
            "crotch",
            "cb_hip",
            "crotch_pt",
            Vec2::new(0.0, y_hip) + cb_dir * (0.5 * (y_hip - y_cr)),
            Vec2::new(-ext_b + 0.5 * ext_b, y_cr),
            None,
        )?;
        p.line("inseam", "crotch_pt", "hem_in");
        p.set(
            "grain",
            json!([[crease, y_hem + 60.0], [crease, y_cr - 20.0]]),
        );
        p.set(
            "placement",
            json!({"part": "leg", "theta_deg": round(180.0 - mid_deg, 2), "x0": 0.0, "y0_landmark": "waist",
                   "radius_mm": round(r_wrap, 1)}),
        );
        p.set("role", json!("pant_back"));
        Ok(p.build())
    };

    let fp = front_piece()?;
    let target_crotch = m.get("total_crotch_length")? + sf(&s, "crotch_ease")? + nappy_mm;
    let crotch_total = |ext_b: f64| -> Result<f64, Refusal> {
        let pat = one(vec![fp.clone(), back_piece(ext_b, 0.0, 0.0)?]);
        Ok(edge_len(&pat, "tr_front.R", "crotch")?
            + edge_len(&pat, "tr_front.R", "cf")?
            + (edge_len(&pat, "tr_back.R", "cb")? + edge_len(&pat, "tr_back.R", "crotch")?))
    };
    let ext_b = bisect(
        |e| Ok(crotch_total(e)? - target_crotch),
        0.04 * seat,
        0.20 * seat,
    )?;
    // true the legs: equal inseams and outseams (the front is the reference); damped 2x2 Newton
    let fpat = one(vec![fp.clone()]);
    let (f_in, f_out) = (
        edge_len(&fpat, "tr_front.R", "inseam")?,
        edge_len(&fpat, "tr_front.R", "outseam")?,
    );
    let leg_resid = |v: [f64; 2]| -> Result<[f64; 2], Refusal> {
        let bp = one(vec![back_piece(ext_b, v[0], v[1])?]);
        Ok([
            edge_len(&bp, "tr_back.R", "inseam")? - f_in,
            edge_len(&bp, "tr_back.R", "outseam")? - f_out,
        ])
    };
    let mut v = [0.0f64, 0.0];
    for _ in 0..25 {
        let r = leg_resid(v)?;
        if r[0].abs().max(r[1].abs()) < 0.05 {
            break;
        }
        let c0 = leg_resid([v[0] + 0.5, v[1] + 0.0])?;
        let c1 = leg_resid([v[0] + 0.0, v[1] + 0.5])?;
        let j = [
            [(c0[0] - r[0]) / 0.5, (c1[0] - r[0]) / 0.5],
            [(c0[1] - r[1]) / 0.5, (c1[1] - r[1]) / 0.5],
        ];
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if det == 0.0 || !det.is_finite() {
            return Err(bad("the leg truing Jacobian is singular"));
        }
        let step = [
            (-r[0] * j[1][1] + r[1] * j[0][1]) / det,
            (-r[1] * j[0][0] + r[0] * j[1][0]) / det,
        ];
        v = [
            v[0] + step[0].clamp(-40.0, 40.0),
            v[1] + step[1].clamp(-40.0, 40.0),
        ];
    }
    let bk = back_piece(ext_b, v[0], v[1])?;

    // waistband casing: bottom edge split where it meets each panel (CB, back R, front R, front L, back L)
    let tmp = one(vec![fp.clone(), bk.clone()]);
    let (wl_f, wl_b) = (
        edge_len(&tmp, "tr_front.R", "waist")?,
        edge_len(&tmp, "tr_back.R", "waist")?,
    );
    let band = (seat + 30.0).max((2.0 * (wl_f + wl_b)) / 1.6);
    let (seg_b, seg_f) = (
        band * wl_b / (2.0 * (wl_f + wl_b)),
        band * wl_f / (2.0 * (wl_f + wl_b)),
    );
    let casing = sf(&s, "casing_height")?;
    let h = 2.0 * casing;
    let waist = m.get("waist")?;
    let elastic = sf(&s, "elastic_frac")? * waist;
    let overlap = sf(&s, "elastic_overlap")?;
    let mut w = PieceBuilder::new(
        "waistband",
        "WAIST CASING",
        json!({"material": "trouser", "cut": {"qty": 1}, "role": "waistband",
               "sim_scale": [round(waist / band, 4), 0.5],
               "note": format!("Elastic {} mm wide: cut {} mm (includes {} mm overlap)",
                               fixed(casing - 5.0, 0), fixed(elastic + overlap, 0), fixed(overlap, 0)),
               "grain": [[40, h / 2.0 - 10.0], [band - 40.0, h / 2.0 - 10.0]]}),
    );
    w.set(
        "placement",
        json!({"part": "torso", "theta_deg": 180.0, "x0": 0.0, "y0_landmark": "waist", "y0": 0.0, "clearance_mm": 15}),
    );
    let mut x = 0.0;
    for (i, d) in [0.0, seg_b, seg_f, seg_f, seg_b].into_iter().enumerate() {
        x += d;
        w.v(&format!("b{i}"), x, 0.0);
    }
    w.v("t4", band, h);
    w.v("t0", 0.0, h);
    for (i, name) in ["bot_bR", "bot_fR", "bot_fL", "bot_bL"]
        .into_iter()
        .enumerate()
    {
        w.line(name, &format!("b{i}"), &format!("b{}", i + 1));
    }
    w.line("end_r", "b4", "t4");
    w.line("top", "t4", "t0");
    w.line("end_l", "t0", "b0");
    w.set(
        "internal",
        json!([{"kind": "draw", "label": "FOLD (elastic casing)", "pts": [[0, h / 2.0], [band, h / 2.0]]}]),
    );

    let h = 2.0 * cuff_h;
    let mut c = PieceBuilder::new(
        "cuff",
        "LEG CUFF",
        json!({"material": "trouser", "cut": {"qty": 2}, "role": "cuff", "sim_scale": [1.0, 0.5],
               "grain": [[20, 0.25 * h], [cuff_len - 20.0, 0.25 * h]]}),
    );
    c.set(
        "placement",
        json!({"part": "leg", "theta_deg": -90.0, "x0": 0.0, "y0_landmark": cuff_level, "y0": h / 2.0,
               "radius_mm": round((cuff_len / (2.0 * PI)).max(cuff_body / (2.0 * PI) + 6.0), 1)}),
    );
    let hf_share = hem_f / hem_total;
    c.v("b0", 0.0, 0.0);
    c.v("b1", cuff_len, 0.0);
    c.v("t1", cuff_len, h);
    c.v("tm", cuff_len * hf_share, h);
    c.v("t0", 0.0, h);
    c.line("bottom", "b0", "b1");
    c.line("end_r", "b1", "t1");
    c.line("top_back", "t1", "tm");
    c.line("top_front", "tm", "t0");
    c.line("end_l", "t0", "b0");
    c.set(
        "internal",
        json!([{"kind": "draw", "label": "FOLD", "pts": [[0, h / 2.0], [cuff_len, h / 2.0]]}]),
    );

    let mut meta = Map::new();
    meta.insert(
        "title".into(),
        json!("Zouave full trousers (elastic casing, gathered into cuffs)"),
    );
    meta.insert("size_label".into(), json!(size_label(m)));
    meta.insert("origin".into(), json!("DRAFT"));
    meta.insert("round".into(), json!(0));
    meta.insert("sa_mm".into(), json!(10));
    meta.insert("program".into(), json!("drafting.zouave_trousers"));
    meta.insert("style".into(), Value::Object(s.clone()));
    meta.insert(
        "solved".into(),
        json!({"back_crotch_ext_mm": round(ext_b, 1), "crotch_target_mm": round(target_crotch, 1),
               "band_mm": round(band, 1), "cuff_mm": round(cuff_len, 1),
               "elastic_cut_mm": round(elastic + overlap, 1), "leg_to_cuff_top_mm": round(-y_hem, 1)}),
    );
    let gather_w = 2.0 * (wl_f + wl_b) / band;
    let ratio = round(gather, 3);
    let mut seams: Vec<Value> = mirror_seams(&[
        json!({"id": "crotch_front", "a": "tr_front.R/crotch+cf", "b": "tr_front.L/cf+crotch"}),
        json!({"id": "inseam", "a": "tr_front.R/inseam", "b": "tr_back.R/inseam"}),
        json!({"id": "outseam", "a": "tr_front.R/outseam", "b": "tr_back.R/outseam"}),
        json!({"id": "cuff_front", "a": "tr_front.R/hem", "b": "cuff.R/top_front", "ratio": ratio,
               "ratio_band": [1.4, 2.4], "kind": "gather"}),
        json!({"id": "cuff_back", "a": "tr_back.R/hem", "b": "cuff.R/top_back", "ratio": ratio,
               "ratio_band": [1.4, 2.4], "kind": "gather"}),
        json!({"id": "cuff_end", "a": "cuff.R/end_r", "b": "cuff.R/end_l"}),
    ])
    .into_iter()
    .filter(|sm| sm["id"] != "crotch_front_L")
    .collect();
    seams.push(json!({"id": "crotch_back", "a": "tr_back.R/cb+crotch", "b": "tr_back.L/crotch+cb", "reverse": true}));
    for (name, inst) in [
        ("bot_bR", "tr_back.R"),
        ("bot_fR", "tr_front.R"),
        ("bot_fL", "tr_front.L"),
        ("bot_bL", "tr_back.L"),
    ] {
        seams.push(json!({"id": format!("casing_{name}"), "a": format!("{inst}/waist"), "b": format!("waistband/{name}"),
                          "reverse": true, "ratio": round(gather_w, 3), "ratio_band": [1.0, 2.0], "kind": "gather"}));
    }
    seams.push(
        json!({"id": "casing_cb", "a": "waistband/end_r", "b": "waistband/end_l", "reverse": true}),
    );
    let pat = json!({
        "schema": SCHEMA, "style": "ZOUAVE_TROUSER", "units": "mm", "meta": meta,
        "fabric": {"id": "trouser", "name": "wool flannel", "kind": "woven", "areal_g_m2": 280, "width_mm": 1500},
        "pieces": [fp, bk, w.build(), c.build()], "seams": seams
    });
    let casing_refs = json!([
        "waistband/bot_bR",
        "waistband/bot_fR",
        "waistband/bot_fL",
        "waistband/bot_bL"
    ]);
    let spec = json!({
        "schema": "fabric-fit/1", "garment": "zouave_trousers", "seam_tol_mm": 3.0, "truing_tol_deg": 8.0,
        "points": [
            {"id": "seat", "kind": "girth", "instances": ["tr_front.R", "tr_front.L", "tr_back.R", "tr_back.L"],
             "landmark": "hip", "body": seat_name, "target_mm": [200, 320, 480],
             "label": "Seat fullness (design ease)"},
            {"id": "crotch", "kind": "path", "refs": ["tr_front.R/crotch", "tr_front.R/cf", "tr_back.R/cb", "tr_back.R/crotch"],
             "body": "total_crotch_length", "target_mm": [15, 30, 50], "label": "Total crotch length ease"},
            {"id": "casing_pass", "kind": "path", "refs": casing_refs, "body": seat_name, "target_mm": [20, 40, 120],
             "label": "Elastic casing passes over the seat"},
            {"id": "elastic_hold", "kind": "path", "refs": casing_refs, "scale": round(elastic / band, 5),
             "body": "waist", "target_mm": [round(-0.12 * waist, 1), round(-0.05 * waist, 1), -5.0],
             "label": "Relaxed elastic vs waist (must be shorter to hold)"},
            {"id": "cuff_pass", "kind": "path", "refs": ["cuff.R/bottom"], "body": "foot_pass",
             "target_mm": [10, 25, 60], "label": "Cuff passes over the foot"},
            {"id": "cuff_fit", "kind": "path", "refs": ["cuff.R/bottom"], "body": cuff_level,
             "target_mm": [10, 25, 45], "label": format!("Cuff ease at the {cuff_level}")},
            {"id": "leg_blouse", "kind": "vertical", "instance": "tr_front.R", "from": "waist_side", "to": "hem_side",
             "body": "waist_height", "body_offset_mm": num(-round(z_cuff_top, 1)), "target_mm": [15, 40, 80],
             "label": format!("Leg length blousing over the {cuff_level} cuff")},
        ]
    });
    Ok((pat, spec))
}
