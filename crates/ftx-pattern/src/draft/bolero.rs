//! `zouave_bolero` (spec 02 §C.1–C.2): open front with a curved cutaway, set-in sleeve, back cut on the fold,
//! and the soutache braid layout.

use std::f64::consts::PI;

use ftx_core::curve::curve_points;
use ftx_core::polyline::horizontal_width;
use ftx_core::{Curve, CurveKind, Refusal, RefusalCode, Vec2};
use serde_json::{Map, Value, json};

use super::builder::{
    PieceBuilder, SCHEMA, bisect, len_cubic, merge_style, mirror_seams, num, pt, sf, size_label,
    unit,
};
use super::form::{Meas, landmarks, section_radius};
use super::geos;
use crate::Pattern;

/// The bolero's style defaults (`BOLERO_DEFAULTS`).
#[must_use]
pub fn defaults() -> Value {
    json!({
        "chest_ease": 100.0, "hem_y": -10.0, "armhole_drop": 12.0, "back_neck_rise": 12.0, "neck_ease": 5.0,
        "front_neck_extra": 12.0, "neckline_ease": 50.0, "back_slope_deg": 16.0, "front_slope_deg": 20.0,
        "back_shoulder_ease": 4.0, "across_back_ease": 25.0, "front_gap_mm": 10.0, "front_cutaway": 45.0,
        "front_belly_mm": 0.0, "bicep_ease": 50.0, "cap_ease": 0.045, "sleeve_extra": 15.0, "cuff_ease": 25.0,
        "soutache": true, "soutache_inset": 12.0, "soutache_width": 3.0, "sleeve_band": 35.0, "motif": 0.035
    })
}

fn bad(m: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, m)
}

/// A drafted pattern's instance edge length.
fn edge_len(p: &Pattern, inst: &str, edge: &str) -> Result<f64, Refusal> {
    p.instance(inst)
        .ok_or_else(|| bad(format!("no instance {inst}")))?
        .edge_length(edge)
        .map_err(|e| bad(e.message().to_string()))
}

struct Fixed {
    nb: f64,
    sl: f64,
    wb: f64,
    wf: f64,
    y_ua: f64,
    hem: f64,
    y_cbn: f64,
    y_snp: f64,
}

/// `_bolero_bodice(s, k, ...)`: the back half and the right front for neck scale `k` and armhole `scoop`.
fn bodice(
    s: &Map<String, Value>,
    x: &Fixed,
    k: f64,
    scoop: f64,
) -> Result<(PieceBuilder, PieceBuilder), Refusal> {
    let neck_w = k * (x.nb / 6.0 + sf(s, "neck_ease")?);
    let bs = sf(s, "back_slope_deg")?.to_radians();
    let bsl = x.sl + sf(s, "back_shoulder_ease")?;
    let mut b = PieceBuilder::new(
        "back",
        "BACK",
        json!({"material": "shell", "cut": {"qty": 1, "on_fold": "cb"}, "role": "bodice_back", "grain": [[20, 0], [20, 150]]}),
    );
    b.v("cb_hem", 0.0, x.hem);
    b.v("side_hem", x.wb - 5.0, x.hem);
    b.v("ua", x.wb, x.y_ua);
    let tip_b = Vec2::new(neck_w + bsl * bs.cos(), x.y_snp - bsl * bs.sin());
    b.v("tip", tip_b.x, tip_b.y);
    b.v("snp", neck_w, x.y_snp);
    b.v("cbn", 0.0, x.y_cbn);
    b.line("hem", "cb_hem", "side_hem");
    b.line("side", "side_hem", "ua");
    let sh_dir_b = unit(Vec2::new(neck_w, x.y_snp) - tip_b);
    let mut arr_b = Vec2::new(sh_dir_b.y, -sh_dir_b.x);
    if arr_b.y < 0.0 {
        arr_b = -arr_b;
    }
    let depth_b = tip_b.y - x.y_ua;
    b.cubic_abs(
        "armhole",
        "ua",
        "tip",
        Vec2::new(x.wb - scoop * (x.wb - tip_b.x + 40.0), x.y_ua),
        tip_b - arr_b * (0.42 * depth_b),
        None,
    )?;
    b.line("shoulder", "tip", "snp");
    let mut leave_b = Vec2::new(-sh_dir_b.y, sh_dir_b.x);
    if leave_b.y > 0.0 {
        leave_b = -leave_b;
    }
    b.cubic_abs(
        "neck",
        "snp",
        "cbn",
        Vec2::new(neck_w, x.y_snp) + leave_b * (0.35 * neck_w),
        Vec2::new(0.45 * neck_w, x.y_cbn),
        None,
    )?;
    b.line("cb", "cbn", "cb_hem");
    b.notch("n_back_arm", "armhole", 0.45, "double");
    b.set(
        "placement",
        json!({"part": "torso", "theta_deg": 180.0, "x0": 0.0, "y0_landmark": "waist", "clearance_mm": 20}),
    );

    let fs = sf(s, "front_slope_deg")?.to_radians();
    let mut f = PieceBuilder::new(
        "front",
        "FRONT",
        json!({"material": "shell", "cut": {"qty": 2}, "role": "bodice_front",
               "grain": [[x.wf - 40.0, 0], [x.wf - 40.0, 150]]}),
    );
    let cfx = x.wf - sf(s, "front_gap_mm")?;
    let snp_f = Vec2::new(x.wf - neck_w + 3.0, x.y_snp);
    let tip_f = snp_f + Vec2::new(-fs.cos(), -fs.sin()) * x.sl;
    let y_cfn = x.y_snp - k * (x.nb / 6.0 + sf(s, "front_neck_extra")?);
    f.v("side_hem", 5.0, x.hem);
    let belly = sf(s, "front_belly_mm")?;
    let cut = sf(s, "front_cutaway")?;
    let hem_f = x.hem - belly;
    f.v("front_hem", cfx - cut, hem_f);
    f.v("cfn", cfx, y_cfn);
    f.v("snp", snp_f.x, snp_f.y);
    f.v("tip", tip_f.x, tip_f.y);
    f.v("ua", 0.0, x.y_ua);
    if belly != 0.0 {
        let (x0, x1) = (5.0, cfx - cut);
        f.cubic_abs(
            "hem",
            "side_hem",
            "front_hem",
            Vec2::new(x0 + 0.45 * (x1 - x0), x.hem),
            Vec2::new(x1 - 0.35 * (x1 - x0), hem_f),
            None,
        )?;
    } else {
        f.line("hem", "side_hem", "front_hem");
    }
    f.cubic_abs(
        "front_edge",
        "front_hem",
        "cfn",
        Vec2::new(cfx - cut + 0.9 * cut, hem_f + 8.0),
        Vec2::new(cfx, y_cfn - 0.55 * (y_cfn - hem_f)),
        None,
    )?;
    let sh_dir_f = unit(tip_f - snp_f);
    let mut into_neck = Vec2::new(sh_dir_f.y, -sh_dir_f.x);
    if into_neck.y < 0.0 {
        into_neck = -into_neck;
    }
    f.cubic_abs(
        "neck",
        "cfn",
        "snp",
        Vec2::new(cfx - 0.55 * (cfx - snp_f.x), y_cfn),
        snp_f - into_neck * (0.45 * (x.y_snp - y_cfn)),
        None,
    )?;
    f.line("shoulder", "snp", "tip");
    let mut down_f = Vec2::new(-sh_dir_f.y, sh_dir_f.x);
    if down_f.y > 0.0 {
        down_f = -down_f;
    }
    let depth_f = tip_f.y - x.y_ua;
    f.cubic_abs(
        "armhole",
        "tip",
        "ua",
        tip_f + down_f * (0.42 * depth_f),
        Vec2::new(0.30 * (tip_f.x + 45.0), x.y_ua),
        None,
    )?;
    f.line("side", "ua", "side_hem");
    f.notch("n_front_arm", "armhole", 0.55, "V");
    f.set(
        "placement",
        json!({"part": "torso", "theta_deg": -(sf(s, "front_gap_mm")? / 120.0).to_degrees(), "x0": cfx,
               "y0_landmark": "waist", "clearance_mm": 20}),
    );
    Ok((b, f))
}

fn two_piece(b: &PieceBuilder, f: &PieceBuilder) -> Pattern {
    Pattern::new(
        json!({"schema": SCHEMA, "units": "mm", "pieces": [b.build(), f.build()], "seams": []}),
    )
}

/// `zouave_bolero(meas, style)`: the pattern and its fit spec.
///
/// # Errors
/// An unknown style key; a missing measurement; geometry that cannot be drafted.
pub fn zouave_bolero(meas: &Value, style: Option<&Value>) -> Result<(Value, Value), Refusal> {
    let s = merge_style(&defaults(), style)?;
    let m = Meas(meas);
    let l = landmarks(m)?;
    let (chest, nb, sl) = (
        m.get("chest")?,
        m.get("neck_base")?,
        m.get("shoulder_length")?,
    );
    let bnw = m.get("back_neck_to_waist")?;
    let q = (chest + sf(&s, "chest_ease")?) / 4.0;
    let x = Fixed {
        nb,
        sl,
        wb: q + 5.0,
        wf: q - 5.0,
        y_ua: (l.get("chest")? - l.get("waist")?) - sf(&s, "armhole_drop")?,
        hem: sf(&s, "hem_y")?,
        y_cbn: bnw,
        y_snp: bnw + sf(&s, "back_neck_rise")?,
    };
    let neck_target = nb + sf(&s, "neckline_ease")?;
    let y_ab = l.get("across_back")? - l.get("waist")?;
    let ab_target = m.get("across_back")? + sf(&s, "across_back_ease")?;
    let neckline = |k: f64, scoop: f64| -> Result<f64, Refusal> {
        let (b, f) = bodice(&s, &x, k, scoop)?;
        let t = two_piece(&b, &f);
        Ok(2.0 * (edge_len(&t, "front.R", "neck")? + edge_len(&t, "back", "neck.R")?))
    };
    let across_back = |k: f64, scoop: f64| -> Result<f64, Refusal> {
        let (b, f) = bodice(&s, &x, k, scoop)?;
        let t = two_piece(&b, &f);
        let o = t
            .instance("back")
            .ok_or_else(|| bad("no back instance"))?
            .outline(120)
            .map_err(|e| bad(e.message().to_string()))?;
        Ok(horizontal_width(&o, y_ab))
    };
    let (mut k_neck, mut scoop) = (1.0, 0.32);
    for _ in 0..2 {
        k_neck = bisect(|k| Ok(neckline(k, scoop)? - neck_target), 0.6, 2.2)?;
        scoop = bisect(|sc| Ok(across_back(k_neck, sc)? - ab_target), 0.05, 1.2)?;
    }
    let (b, f) = bodice(&s, &x, k_neck, scoop)?;
    let bsl = sl + sf(&s, "back_shoulder_ease")?;
    let mut meta = Map::new();
    meta.insert(
        "title".into(),
        json!("Zouave bolero (open front, set-in sleeve)"),
    );
    meta.insert("size_label".into(), json!(size_label(m)));
    meta.insert("origin".into(), json!("DRAFT"));
    meta.insert("round".into(), json!(0));
    meta.insert("sa_mm".into(), json!(10));
    meta.insert("program".into(), json!("drafting.zouave_bolero"));
    meta.insert("style".into(), Value::Object(s.clone()));
    let mut pat = json!({
        "schema": SCHEMA, "style": "ZOUAVE_BOLERO", "units": "mm", "meta": meta,
        "fabric": {"id": "shell", "name": "wool flannel, lined", "kind": "woven", "areal_g_m2": 380, "width_mm": 1500,
                   "tensile_N_m": 10000.0, "bending_Nm": 1.2e-4, "sim_note": "lined-jacket composite (UNCERTAIN)"},
        "pieces": [b.build(), f.build()], "seams": []
    });
    let tmp = Pattern::new(pat.clone());
    let arm_f = edge_len(&tmp, "front.R", "armhole")?;
    let arm_b = edge_len(&tmp, "back", "armhole.R")?;
    finish(m, &s, &mut pat, arm_f, arm_b, bsl, k_neck).map(|spec| (pat, spec))
}

type Cubic = [Vec2; 4];

/// `_bolero_finish`: the sleeve (cap solved to the armhole), seams, solved values, the fit spec, the braid.
fn finish(
    m: Meas,
    s: &Map<String, Value>,
    pat: &mut Value,
    arm_f: f64,
    arm_b: f64,
    bsl: f64,
    k_neck: f64,
) -> Result<Value, Refusal> {
    let sl = m.get("shoulder_length")?;
    let bic = m.get("upper_arm")? + sf(s, "bicep_ease")?;
    let slen = m.get("arm_length")? + sf(s, "sleeve_extra")?;
    let hem_w = (m.get("hand_pass")? + sf(s, "cuff_ease")?).max(m.get("wrist")? + 60.0);
    let target = (arm_f + arm_b) * (1.0 + sf(s, "cap_ease")?);
    let caps = |hc: f64, xt: f64| -> (Cubic, Cubic) {
        let (uf, ub, top) = (
            Vec2::new(bic / 2.0, -hc),
            Vec2::new(-bic / 2.0, -hc),
            Vec2::new(xt, 0.0),
        );
        let (wf_, wb_) = (bic / 2.0 - xt, bic / 2.0 + xt);
        let d_f = unit(uf - Vec2::new(hem_w / 2.0, -slen));
        let d_b = unit(ub - Vec2::new(-hem_w / 2.0, -slen));
        let cf1 = uf + Vec2::new(-d_f.y, d_f.x) * (0.30 * wf_);
        let cb2 = ub + Vec2::new(d_b.y, -d_b.x) * (0.26 * wb_);
        let (cf2, cb1) = (
            top + Vec2::new(0.42 * wf_, 0.0),
            top + Vec2::new(-0.48 * wb_, 0.0),
        );
        ([uf, cf1, cf2, top], [top, cb1, cb2, ub])
    };
    let solve_hc = |xt: f64| -> Result<f64, Refusal> {
        bisect(
            |h| {
                let (a, b) = caps(h, xt);
                Ok(len_cubic(a) + len_cubic(b) - target)
            },
            5.0,
            0.9 * bic,
        )
    };
    let imbalance = |xt: f64| -> Result<f64, Refusal> {
        let (cf, cb) = caps(solve_hc(xt)?, xt);
        Ok(len_cubic(cf) / arm_f - len_cubic(cb) / arm_b)
    };
    let xt = bisect(imbalance, -0.3 * bic, 0.3 * bic)?;
    let hc = solve_hc(xt)?;
    let ([uf, cf1, cf2, top], [_, cb1, cb2, ub]) = caps(hc, xt);
    let mut sv = PieceBuilder::new(
        "sleeve",
        "SLEEVE",
        json!({"material": "shell", "cut": {"qty": 2}, "role": "sleeve",
               "grain": [[0, -hc - 20.0], [0, -slen + 30.0]]}),
    );
    sv.v("hem_b", -hem_w / 2.0, -slen);
    sv.v("hem_f", hem_w / 2.0, -slen);
    sv.v("ua_f", uf.x, uf.y);
    sv.v("top", top.x, top.y);
    sv.v("ua_b", ub.x, ub.y);
    let (hb, hf) = (
        Vec2::new(-hem_w / 2.0, -slen),
        Vec2::new(hem_w / 2.0, -slen),
    );
    let (d_f, d_b) = (unit(uf - hf), unit(ub - hb));
    let out_f = Vec2::new(d_f.y, -d_f.x);
    let out_b = Vec2::new(d_b.y, -d_b.x);
    sv.cubic_abs(
        "hem",
        "hem_b",
        "hem_f",
        hb + out_b * (0.33 * hem_w),
        hf - out_f * (0.33 * hem_w),
        None,
    )?;
    sv.line("under_f", "hem_f", "ua_f");
    sv.cubic_abs("cap_f", "ua_f", "top", cf1, cf2, None)?;
    sv.cubic_abs("cap_b", "top", "ua_b", cb1, cb2, None)?;
    sv.line("under_b", "ua_b", "hem_b");
    sv.notch("n_cap_f", "cap_f", 0.45, "V");
    sv.notch("n_cap_b", "cap_b", 0.55, "double");
    let radius = (bic / (2.0 * PI)).max(section_radius(m, "arm_R", 0.0)? + 6.0);
    sv.set(
        "placement",
        json!({"part": "arm", "theta_deg": -90.0, "x0": ftx_core::py::round(xt, 3), "y0_landmark": "shoulder_tip",
               "radius_mm": radius}),
    );
    if let Some(Value::Array(pieces)) = pat.get_mut("pieces") {
        pieces.push(sv.build());
    }
    let r = |x: f64, n: usize| num(ftx_core::py::round(x, n));
    pat["seams"] = Value::Array(mirror_seams(&[
        json!({"id": "side", "a": "front.R/side", "b": "back/side.R"}),
        json!({"id": "shoulder", "a": "back/shoulder.R", "b": "front.R/shoulder",
               "ratio": r(bsl / sl, 4), "ratio_band": [1.0, 1.08], "kind": "ease"}),
        json!({"id": "armhole_f", "a": "sleeve.R/cap_f", "b": "front.R/armhole", "ratio_band": [1.0, 1.08], "kind": "ease"}),
        json!({"id": "armhole_b", "a": "sleeve.R/cap_b", "b": "back/armhole.R", "ratio_band": [1.0, 1.08], "kind": "ease"}),
        json!({"id": "underarm", "a": "sleeve.R/under_f", "b": "sleeve.R/under_b"}),
    ]));
    pat["meta"]["solved"] = json!({
        "cap_height_mm": r(hc, 1), "armhole_mm": r(arm_f + arm_b, 1), "cap_target_mm": r(target, 1),
        "cap_top_offset_mm": r(xt, 1), "neck_scale": r(k_neck, 4)
    });
    let drop = sf(s, "armhole_drop")?;
    let spec = json!({
        "schema": "fabric-fit/1", "garment": "zouave_bolero", "seam_tol_mm": 3.0, "truing_tol_deg": 8.0,
        "truing": [{"seam": "side_R", "end": "start"}, {"seam": "side_L", "end": "end"},
                   {"seam": "shoulder_R", "end": "start"}, {"seam": "shoulder_L", "end": "end"},
                   {"seam": "underarm_R", "end": "end"}, {"seam": "underarm_L", "end": "start"}],
        "points": [
            {"id": "chest", "kind": "girth", "instances": ["front.R", "front.L", "back"], "landmark": "chest",
             "dy": -(drop + 12.0), "body": "chest", "target_mm": [70, 100, 130],
             "label": "Chest ease (fronts edge to edge)"},
            {"id": "across_back", "kind": "girth", "instances": ["back"], "landmark": "across_back",
             "body": "across_back", "target_mm": [10, 25, 45], "label": "Across-back ease"},
            {"id": "bicep", "kind": "width", "instance": "sleeve.R", "at_vertex": "ua_f", "body": "upper_arm",
             "target_mm": [35, 50, 70], "label": "Bicep ease"},
            {"id": "sleeve_length", "kind": "vertical", "instance": "sleeve.R", "from": "top", "to": "hem_f",
             "body": "arm_length", "target_mm": [0, 15, 35], "label": "Sleeve length over arm length"},
            {"id": "cuff_pass", "kind": "path", "refs": ["sleeve.R/hem"], "body": "hand_pass",
             "target_mm": [10, 25, 60], "label": "Sleeve hem passes the hand"},
            {"id": "neckline", "kind": "path", "refs": ["front.R/neck", "front.L/neck", "back/neck.R", "back/neck.L"],
             "body": "neck_base", "target_mm": [30, 50, 90], "label": "Neckline ease (open front)"},
            {"id": "back_length", "kind": "vertical", "instance": "back", "from": "cbn", "to": "cb_hem",
             "body": "back_neck_to_waist", "target_mm": [-10, 10, 30], "label": "Back length vs nape-to-waist"},
        ]
    });
    soutache(pat, s, m.get("chest")?)?;
    Ok(spec)
}

/// Each edge of a piece, sampled.
type SampledEdges = Vec<(String, Vec<Vec2>)>;

/// A drafted piece's edges sampled (120 per Bézier) in its own coordinates, and its outline.
fn piece_outline(piece: &Value) -> Result<(Vec<Vec2>, SampledEdges), Refusal> {
    let vert = |id: &str| -> Result<Vec2, Refusal> {
        let v = &piece["vertices"][id];
        match (v[0].as_f64(), v[1].as_f64()) {
            (Some(x), Some(y)) => Ok(Vec2::new(x, y)),
            _ => Err(bad(format!("no vertex {id}"))),
        }
    };
    let mut edges = Vec::new();
    for e in piece["edges"].as_array().into_iter().flatten() {
        let (a, b) = (
            vert(e["from"].as_str().unwrap_or(""))?,
            vert(e["to"].as_str().unwrap_or(""))?,
        );
        let curve = match e.get("curve") {
            Some(c) if !c.is_null() => {
                let kind = match c["type"].as_str() {
                    Some("cubic") => CurveKind::Cubic,
                    Some("quad") => CurveKind::Quad,
                    Some("poly") => CurveKind::Poly,
                    _ => CurveKind::Line,
                };
                let ctrl: Vec<[f64; 2]> = c["c"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| [p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)])
                    .collect();
                Some(Curve::new(kind, ctrl))
            }
            _ => None,
        };
        let pts = curve_points(a, b, curve.as_ref(), 120).map_err(|e| bad(e.to_string()))?;
        edges.push((e["id"].as_str().unwrap_or("").to_string(), pts));
    }
    let mut outline = Vec::new();
    for (_, p) in &edges {
        outline.extend_from_slice(&p[..p.len() - 1]);
    }
    Ok((outline, edges))
}

fn np_round2(x: f64) -> f64 {
    (x * 100.0).round_ties_even() / 100.0
}

fn rounded(pts: &[Vec2]) -> Value {
    Value::Array(
        pts.iter()
            .map(|p| json!([np_round2(p.x), np_round2(p.y)]))
            .collect(),
    )
}

/// `_braid_border(piece, chain, inset)`: the braid's centre line `inset` inside a run of finished edges,
/// carried out to the seam lines at both ends (unrounded, before `np.round`).
///
/// # Errors
/// A piece that cannot be sampled.
pub fn braid_border(
    piece: &Value,
    chain: &[&str],
    inset: f64,
) -> Result<Option<Vec<Vec2>>, Refusal> {
    let (outline, edges) = piece_outline(piece)?;
    let edge = |id: &str| {
        edges
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, p)| p.clone())
            .ok_or_else(|| bad(format!("no edge {id}")))
    };
    let mut pts = edge(chain[0])?;
    for id in &chain[1..] {
        pts.extend_from_slice(&edge(id)?[1..]);
    }
    let n = pts.len();
    // pts[0] + 3 * inset * t0 / norm(t0)
    let ext_dir = |a: Vec2, b: Vec2| {
        let t = a - b;
        let l = (t.x * t.x + t.y * t.y).sqrt();
        a + Vec2::new(3.0 * inset * t.x / l, 3.0 * inset * t.y / l)
    };
    let mut ext = vec![ext_dir(pts[0], pts[1])];
    ext.extend_from_slice(&pts);
    ext.push(ext_dir(pts[n - 1], pts[n - 2]));
    let mut parts: Vec<Vec<Vec2>> = Vec::new();
    for sec in geos::offset_curve(&ext, inset, 3.0) {
        parts.extend(
            geos::clip_inside(&sec, &outline)
                .into_iter()
                .filter(|p| geos::length(p) > 0.0),
        );
    }
    // max(parts, key=length): the first longest
    let mut best: Option<Vec<Vec2>> = None;
    for p in parts {
        if best
            .as_ref()
            .is_none_or(|b| geos::length(&p) > geos::length(b))
        {
            best = Some(p);
        }
    }
    Ok(best)
}

/// `_trefoil(centre, r)`: the three-loop motif in one stroke, 241 points.
fn trefoil(c: Vec2, r: f64) -> Vec<Vec2> {
    ftx_core::np::linspace(0.0, 2.0 * PI, 241)
        .into_iter()
        .map(|t| {
            Vec2::new(
                c.x + r / 3.0 * (t.sin() + 2.0 * (2.0 * t).sin()),
                c.y - r / 3.0 * (t.cos() - 2.0 * (2.0 * t).cos()),
            )
        })
        .map(|p| Vec2::new(np_round2(p.x), np_round2(p.y)))
        .collect()
}

fn soutache(pat: &mut Value, s: &Map<String, Value>, chest: f64) -> Result<(), Refusal> {
    if !crate::doc::truthy(&s["soutache"]) {
        return Ok(());
    }
    let (inset, width) = (sf(s, "soutache_inset")?, sf(s, "soutache_width")?);
    let piece = |pat: &Value, id: &str| -> Result<Value, Refusal> {
        pat["pieces"]
            .as_array()
            .and_then(|p| p.iter().find(|q| q["id"] == id))
            .cloned()
            .ok_or_else(|| bad(format!("no piece {id}")))
    };
    let (front, back, sleeve) = (
        piece(pat, "front")?,
        piece(pat, "back")?,
        piece(pat, "sleeve")?,
    );
    let gap = 8.0;
    let clear = inset + width + gap;
    let lines = [
        (
            "front",
            braid_border(&front, &["hem", "front_edge", "neck"], inset)?,
        ),
        ("back", braid_border(&back, &["hem"], inset)?),
        (
            "sleeve",
            braid_border(&sleeve, &["hem"], sf(s, "sleeve_band")?)?,
        ),
    ];
    let mut motifs: Vec<(&str, Option<Vec<Vec2>>)> = Vec::new();
    let motif = sf(s, "motif")?;
    if motif > 0.0 {
        let r = (motif * chest).max(6.0 * width);
        let v = |p: &Value, id: &str| {
            Vec2::new(
                p["vertices"][id][0].as_f64().unwrap_or(f64::NAN),
                p["vertices"][id][1].as_f64().unwrap_or(f64::NAN),
            )
        };
        let keep = |p: &Value, c: Vec2, r: f64| -> Result<Option<Vec<Vec2>>, Refusal> {
            let m = trefoil(c, r);
            let (outline, _) = piece_outline(p)?;
            Ok(geos::keeps_clear(&m, &outline, clear).then_some(m))
        };
        let corner = v(&front, "front_hem") + Vec2::new(-(clear + r), clear + r);
        motifs.push(("front", keep(&front, corner, r)?));
        let centre = Vec2::new(
            0.5 * v(&back, "side_hem").x,
            v(&back, "cb_hem").y + clear + 1.2 * r,
        );
        motifs.push(("back", keep(&back, centre, 1.2 * r)?));
    }
    for (pid, line) in lines {
        let mut decor = Vec::new();
        if let Some(l) = line {
            decor.push(
                json!({"kind": "soutache", "pts": rounded(&l), "width_mm": width, "closed": false}),
            );
        }
        if let Some((_, Some(m))) = motifs.iter().find(|(k, _)| *k == pid) {
            decor.push(json!({"kind": "soutache", "pts": m.iter().map(|p| pt(*p)).collect::<Vec<_>>(), "width_mm": width, "closed": true}));
        }
        if !decor.is_empty()
            && let Some(p) = pat["pieces"]
                .as_array_mut()
                .and_then(|ps| ps.iter_mut().find(|q| q["id"] == pid))
        {
            p["decor"] = Value::Array(decor);
        }
    }
    Ok(())
}
