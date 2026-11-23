//! HPGL for plotters and plotter-cutters (`docs/spec/03` §7.9): the master layout at 40 units per mm, drawing
//! (pen 1) before cutting (pen 2) per piece.

use ftx_core::{Refusal, RefusalCode, Vec2, py};

use crate::ctx::Ctx;
use crate::geom::{CAP, grain_marks, label_texts};
use crate::pyjson::PyJson;
use crate::text::{Anchor, stroke_text};

// 40 HPGL units per mm; 1e15 fits strictly inside f64 exact integers (< 2^53) and i64.
const MAX_HPGL_UNITS: f64 = 1e15;

fn hpgl_unit(mm: f64, what: &str) -> Result<i64, Refusal> {
    if !mm.is_finite() {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            format!("non-finite HPGL {what}"),
        ));
    }
    let scaled = (mm * 40.0).round_ties_even();
    if !scaled.is_finite() {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("HPGL {what} overflows plotter coordinates"),
        ));
    }
    if scaled.abs() > MAX_HPGL_UNITS {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("HPGL {what} exceeds plotter coordinate range"),
        ));
    }
    Ok(scaled as i64)
}

/// The HPGL file `<S>_plot.plt`, its name and the manifest's `hpgl` entry.
///
/// # Errors
/// As [`Ctx::master`], or when cut geometry is missing, non-finite, or exceeds plotter range.
pub fn export_hpgl(
    ctx: &Ctx,
    rotate90: bool,
    mirror: bool,
) -> Result<(String, String, PyJson), Refusal> {
    let lay = ctx.master()?;
    let (w, h) = (lay.width, lay.height);
    if !w.is_finite() || !h.is_finite() || w < 0.0 || h < 0.0 {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            "HPGL layout dimensions must be finite and nonnegative",
        ));
    }
    let map = |p: Vec2| -> Result<(i64, i64), Refusal> {
        if !p.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "HPGL layout contains non-finite coordinates",
            ));
        }
        let (mut x, mut y) = (p.x, h - p.y);
        if mirror {
            x = w - x;
        }
        if rotate90 {
            (x, y) = (y, w - x);
        }
        Ok((hpgl_unit(x, "x coordinate")?, hpgl_unit(y, "y coordinate")?))
    };
    let mut out: Vec<String> = ["IN;", "PU;", "SP1;", "LT;", "PA;", "PU0,0;"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let mut pen = 1;
    let mut sp = |out: &mut Vec<String>, n: i32| {
        if pen != n {
            out.push(format!("SP{n};"));
            pen = n;
        }
    };
    let poly = |out: &mut Vec<String>, pts: &[Vec2], closed: bool| -> Result<(), Refusal> {
        if pts.is_empty() {
            return Ok(());
        }
        let mut q: Vec<(i64, i64)> = pts.iter().map(|&p| map(p)).collect::<Result<_, _>>()?;
        if closed {
            q.push(q[0]);
        }
        out.push(format!("PU{},{};", q[0].0, q[0].1));
        for chunk in q[1..].chunks(100) {
            let xy: Vec<String> = chunk.iter().map(|(a, b)| format!("{a},{b}")).collect();
            out.push(format!("PD{};", xy.join(",")));
        }
        out.push("PU;".into());
        Ok(())
    };
    for g in &lay.items {
        sp(&mut out, 1);
        poly(&mut out, &g.sew, true)?;
        for (p, c) in &g.internal_draw {
            poly(&mut out, p, *c)?;
        }
        for d in &g.decor {
            poly(&mut out, &d.pts, d.closed)?;
        }
        for mk in grain_marks(g, 6.0) {
            poly(&mut out, &mk, false)?;
        }
        for (p, _) in &g.fold_lines {
            poly(&mut out, p, false)?;
        }
        for &(p, dia) in &g.drills {
            if !dia.is_finite() || dia <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("HPGL drill diameter must be finite and > 0, got {dia}"),
                ));
            }
            let (x, y) = map(p)?;
            let radius = hpgl_unit(dia / 2.0, "drill radius")?;
            out.push(format!("PU{x},{y};CI{radius};PU;"));
        }
        for lb in &g.labels {
            for (x, y, t, s, _) in label_texts(lb, true, 0.0, 1.0) {
                for stroke in stroke_text(&t, x, y, s * CAP, Anchor::Middle, true) {
                    poly(&mut out, &stroke, false)?;
                }
            }
        }
        sp(&mut out, 2);
        for (p, c) in &g.internal_cut {
            poly(&mut out, p, *c)?;
        }
        for n in &g.notches {
            for mk in &n.marks {
                poly(&mut out, mk, false)?;
            }
        }
        poly(&mut out, &g.cut, true)?;
    }
    out.extend(["PU;", "SP0;", "PG;"].iter().map(|s| (*s).to_string()));
    let name = format!("{}_plot.plt", ctx.style);
    let mut bb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in lay.items.iter().flat_map(|g| g.cut.iter()) {
        if !p.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "HPGL layout contains non-finite cut coordinates",
            ));
        }
        bb = [
            bb[0].min(p.x),
            bb[1].min(p.y),
            bb[2].max(p.x),
            bb[3].max(p.y),
        ];
    }
    if !bb.iter().all(|v| v.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "HPGL layout has no cut geometry",
        ));
    }
    let entry = PyJson::dict(vec![
        ("file", PyJson::str(&name)),
        ("units_per_mm", PyJson::Int(40)),
        ("rotate_90", PyJson::Bool(rotate90)),
        ("mirror", PyJson::Bool(mirror)),
        (
            "sheet_mm",
            PyJson::floats(&[py::round(w, 3), py::round(h, 3)]),
        ),
        ("cut_bbox", PyJson::floats(&bb.map(|v| py::round(v, 3)))),
    ]);
    Ok((name, out.join("\n") + "\n", entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExportOptions;
    use ftx_pattern::Pattern;

    #[test]
    fn export_hpgl_refuses_empty_pattern_and_handles_empty_decor_without_panicking() {
        let empty = serde_json::json!({
            "schema": "fabric-pattern/1",
            "style": "EMPTY",
            "units": "mm",
            "pieces": []
        });
        let pat = Pattern::new(empty);
        let ctx = Ctx::new(&pat, ExportOptions::default()).unwrap();
        let err = export_hpgl(&ctx, false, false).unwrap_err();
        assert_eq!(err.code, RefusalCode::DegenerateGeometry);

        let with_empty_decor = serde_json::json!({
            "schema": "fabric-pattern/1",
            "style": "DECOR",
            "units": "mm",
            "meta": {"size_label": "2T", "sa_mm": 10},
            "pieces": [{
                "id": "p1",
                "name": "P1",
                "vertices": {"a": [0, 0], "b": [100, 0], "c": [100, 80], "d": [0, 80]},
                "edges": [
                    {"id": "e0", "from": "a", "to": "b"},
                    {"id": "e1", "from": "b", "to": "c"},
                    {"id": "e2", "from": "c", "to": "d"},
                    {"id": "e3", "from": "d", "to": "a"}
                ],
                "decor": [{"kind": "soutache", "pts": [], "width_mm": 3.0, "closed": false}],
                "cut": {"qty": 1}
            }]
        });
        let pat = Pattern::new(with_empty_decor);
        let ctx = Ctx::new(&pat, ExportOptions::default()).unwrap();
        let (_name, text, entry) = export_hpgl(&ctx, false, false).unwrap();
        assert!(text.contains("SP2;"));
        assert!(entry.get("cut_bbox").is_some());
    }

    #[test]
    fn export_hpgl_refuses_coordinates_overflowing_plotter_range() {
        let huge = serde_json::json!({
            "schema": "fabric-pattern/1",
            "style": "HUGE",
            "units": "mm",
            "meta": {"size_label": "2T", "sa_mm": 0},
            "pieces": [{
                "id": "p1",
                "name": "P1",
                "vertices": {
                    "a": [0, 0],
                    "b": [1e14, 0],
                    "c": [1e14, 1e14],
                    "d": [0, 1e14]
                },
                "edges": [
                    {"id": "e0", "from": "a", "to": "b"},
                    {"id": "e1", "from": "b", "to": "c"},
                    {"id": "e2", "from": "c", "to": "d"},
                    {"id": "e3", "from": "d", "to": "a"}
                ],
                "cut": {"qty": 1}
            }]
        });
        let pat = Pattern::new(huge);
        let ctx = Ctx::new(&pat, ExportOptions::default()).unwrap();
        let err = export_hpgl(&ctx, false, false).unwrap_err();
        assert_eq!(err.code, RefusalCode::OutOfRange);
    }
}
