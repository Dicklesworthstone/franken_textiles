//! `fez` (spec 02 §C.4): a cone-frustum crown with a back seam and a flat round tip, both developable.

use std::f64::consts::PI;

use ftx_core::py::round;
use ftx_core::{Refusal, RefusalCode, Vec2};
use serde_json::{Map, Value, json};

use super::builder::{PieceBuilder, SCHEMA, merge_style, sf, size_label};
use super::form::Meas;

/// The fez's style defaults (`FEZ_DEFAULTS`).
#[must_use]
pub fn defaults() -> Value {
    json!({"head_ease": 10.0, "height_frac": 0.62, "top_frac": 0.78})
}

/// `_arc_ctrl(c, rho, a0, a1)`: the cubic controls of a circular arc of at most 90°.
fn arc_ctrl(c: Vec2, rho: f64, a0: f64, a1: f64) -> (Vec2, Vec2) {
    let k = 4.0 / 3.0 * ((a1 - a0) / 4.0).tan() * rho;
    let p0 = c + Vec2::new(a0.cos(), a0.sin()) * rho;
    let p3 = c + Vec2::new(a1.cos(), a1.sin()) * rho;
    (
        p0 + Vec2::new(-a0.sin(), a0.cos()) * k,
        p3 - Vec2::new(-a1.sin(), a1.cos()) * k,
    )
}

fn out_of_range(m: &str) -> Refusal {
    Refusal::new(RefusalCode::OutOfRange, m)
}

/// `fez(meas, style)`: the pattern and its fit spec.
///
/// # Errors
/// An unknown style key or one out of its range; a missing head girth.
pub fn fez(meas: &Value, style: Option<&Value>) -> Result<(Value, Value), Refusal> {
    let s = merge_style(&defaults(), style)?;
    let (top_frac, height_frac, head_ease) = (
        sf(&s, "top_frac")?,
        sf(&s, "height_frac")?,
        sf(&s, "head_ease")?,
    );
    if !(0.3..0.98).contains(&top_frac) {
        return Err(out_of_range(
            "top_frac must be in [0.3, 0.98): a cylinder or a sharp cone is not a fez",
        ));
    }
    if !(0.2..=2.0).contains(&height_frac) {
        return Err(out_of_range(
            "height_frac (height / head diameter) must be in [0.2, 2]",
        ));
    }
    if !(-10.0..=40.0).contains(&head_ease) {
        return Err(out_of_range("head_ease must be in [-10, 40] mm"));
    }
    let m = Meas(meas);
    let head = m.get("head")?;
    let d_head = head / PI;
    let r_base = (head + head_ease) / (2.0 * PI);
    let r_top = top_frac * r_base;
    let height = height_frac * d_head;
    let slant = ftx_core::det::hypot(height, r_base - r_top);
    let rho_b = slant * r_base / (r_base - r_top);
    let rho_t = rho_b - slant;
    let phi = 2.0 * PI * r_base / rho_b;
    let n: usize = if phi <= PI { 2 } else { 4 };
    if phi > 2.0 * PI * 0.9 {
        return Err(out_of_range(
            "taper too strong for a one-piece crown: raise top_frac or the height",
        ));
    }
    let o = Vec2::new(0.0, 0.0);
    let ang: Vec<f64> = (0..=n)
        .map(|i| -PI / 2.0 - phi / 2.0 + phi * i as f64 / n as f64)
        .collect();

    let mut c = PieceBuilder::new(
        "crown",
        "FEZ CROWN",
        json!({"material": "felt", "cut": {"qty": 1}, "role": "hat_crown", "sim": false,
               "note": "Stiffen with buckram or heavy fusible interfacing", "grain": null}),
    );
    for (i, a) in ang.iter().enumerate() {
        c.v(&format!("b{i}"), rho_b * a.cos(), rho_b * a.sin());
        c.v(&format!("t{i}"), rho_t * a.cos(), rho_t * a.sin());
    }
    for i in 0..n {
        let (c1, c2) = arc_ctrl(o, rho_b, ang[i], ang[i + 1]);
        c.cubic_abs(
            &format!("base_{i}"),
            &format!("b{i}"),
            &format!("b{}", i + 1),
            c1,
            c2,
            Some(json!({"sa": 0})),
        )?;
    }
    c.line("back_r", &format!("b{n}"), &format!("t{n}"));
    for i in (0..n).rev() {
        let (c1, c2) = arc_ctrl(o, rho_t, ang[i + 1], ang[i]);
        c.cubic_abs(
            &format!("top_{i}"),
            &format!("t{}", i + 1),
            &format!("t{i}"),
            c1,
            c2,
            None,
        )?;
        let fracs: &[f64] = if n == 2 { &[0.25, 0.75] } else { &[0.5] };
        for (k, frac) in fracs.iter().enumerate() {
            c.notch(&format!("n_top_{i}_{k}"), &format!("top_{i}"), *frac, "V");
        }
    }
    c.line("back_l", "t0", "b0");
    let mid = 0.5 * (rho_b + rho_t);
    c.set(
        "grain",
        json!([[0.0, -mid - 0.3 * slant], [0.0, -mid + 0.3 * slant]]),
    );

    let mut t = PieceBuilder::new(
        "tip",
        "FEZ TIP",
        json!({"material": "felt", "cut": {"qty": 1}, "role": "hat_tip", "sim": false,
               "note": "Tassel at the centre mark", "grain": [[0, -0.5 * r_top], [0, 0.5 * r_top]]}),
    );
    let quarter: Vec<f64> = (0..5)
        .map(|i| -PI / 2.0 + PI / 2.0 * f64::from(i))
        .collect();
    for (i, q) in quarter.iter().take(4).enumerate() {
        t.v(&format!("p{i}"), r_top * q.cos(), r_top * q.sin());
    }
    for i in 0..4 {
        let (c1, c2) = arc_ctrl(o, r_top, quarter[i], quarter[i + 1]);
        t.cubic_abs(
            &format!("q{i}"),
            &format!("p{i}"),
            &format!("p{}", (i + 1) % 4),
            c1,
            c2,
            None,
        )?;
        t.notch(&format!("n_q{i}"), &format!("q{i}"), 0.5, "V");
    }
    t.set("drills", json!([{"p": [0.0, 0.0], "d": 3}]));

    let mut meta = Map::new();
    meta.insert("title".into(), json!("Fez (one-piece crown, round tip)"));
    meta.insert("size_label".into(), json!(size_label(m)));
    meta.insert("origin".into(), json!("DRAFT"));
    meta.insert("round".into(), json!(0));
    meta.insert("sa_mm".into(), json!(7));
    meta.insert("program".into(), json!("drafting.fez"));
    meta.insert("style".into(), Value::Object(s.clone()));
    meta.insert(
        "solved".into(),
        json!({"base_girth_mm": round(2.0 * PI * r_base, 1), "top_girth_mm": round(2.0 * PI * r_top, 1),
               "height_mm": round(height, 1), "slant_mm": round(slant, 1), "sector_deg": round(phi.to_degrees(), 1)}),
    );
    let tops: Vec<String> = (0..n).rev().map(|i| format!("top_{i}")).collect();
    let pat = json!({
        "schema": SCHEMA, "style": "FEZ", "units": "mm", "meta": meta,
        "fabric": {"id": "felt", "name": "wool felt", "kind": "felt", "areal_g_m2": 350, "width_mm": 900},
        "pieces": [c.build(), t.build()],
        "seams": [{"id": "back", "a": "crown/back_r", "b": "crown/back_l", "reverse": true},
                  {"id": "tip", "a": format!("crown/{}", tops.join("+")), "b": "tip/q0+q1+q2+q3", "reverse": true}]
    });
    let spec = json!({
        "schema": "fabric-fit/1", "garment": "fez", "seam_tol_mm": 2.0, "truing_tol_deg": 8.0,
        "points": [
            {"id": "head_fit", "kind": "path", "refs": (0..n).map(|i| format!("crown/base_{i}")).collect::<Vec<_>>(),
             "body": "head", "target_mm": [0, 10, 20], "label": "Base edge over the head girth"},
            {"id": "height", "kind": "path", "refs": ["crown/back_r"], "body": "head", "body_scale": 1.0 / PI,
             "target_mm": [round(slant - 1.15 * d_head, 1), round(slant - d_head, 1), round(slant - 0.85 * d_head, 1)],
             "label": "Slant height (back seam) against the head diameter (design proportion)"},
        ]
    });
    Ok((pat, spec))
}
