//! HPGL for plotters and plotter-cutters (`docs/spec/03` §7.9): the master layout at 40 units per mm, drawing
//! (pen 1) before cutting (pen 2) per piece.

use ftx_core::{Refusal, Vec2, py};

use crate::ctx::Ctx;
use crate::geom::{CAP, grain_marks, label_texts};
use crate::pyjson::PyJson;
use crate::text::{Anchor, stroke_text};

/// The HPGL file `<S>_plot.plt`, its name and the manifest's `hpgl` entry.
///
/// # Errors
/// As [`Ctx::master`].
pub fn export_hpgl(
    ctx: &Ctx,
    rotate90: bool,
    mirror: bool,
) -> Result<(String, String, PyJson), Refusal> {
    let lay = ctx.master()?;
    let (w, h) = (lay.width, lay.height);
    let map = |p: Vec2| -> (i64, i64) {
        let (mut x, mut y) = (p.x, h - p.y);
        if mirror {
            x = w - x;
        }
        if rotate90 {
            (x, y) = (y, w - x);
        }
        (
            (x * 40.0).round_ties_even() as i64,
            (y * 40.0).round_ties_even() as i64,
        )
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
    let poly = |out: &mut Vec<String>, pts: &[Vec2], closed: bool| {
        let mut q: Vec<(i64, i64)> = pts.iter().map(|&p| map(p)).collect();
        if closed {
            q.push(q[0]);
        }
        out.push(format!("PU{},{};", q[0].0, q[0].1));
        for chunk in q[1..].chunks(100) {
            let xy: Vec<String> = chunk.iter().map(|(a, b)| format!("{a},{b}")).collect();
            out.push(format!("PD{};", xy.join(",")));
        }
        out.push("PU;".into());
    };
    for g in &lay.items {
        sp(&mut out, 1);
        poly(&mut out, &g.sew, true);
        for (p, c) in &g.internal_draw {
            poly(&mut out, p, *c);
        }
        for d in &g.decor {
            poly(&mut out, &d.pts, d.closed);
        }
        for mk in grain_marks(g, 6.0) {
            poly(&mut out, &mk, false);
        }
        for (p, _) in &g.fold_lines {
            poly(&mut out, p, false);
        }
        for &(p, dia) in &g.drills {
            let (x, y) = map(p);
            out.push(format!(
                "PU{x},{y};CI{};PU;",
                (dia / 2.0 * 40.0).round_ties_even() as i64
            ));
        }
        for lb in &g.labels {
            for (x, y, t, s, _) in label_texts(lb, true, 0.0, 1.0) {
                for stroke in stroke_text(&t, x, y, s * CAP, Anchor::Middle, true) {
                    poly(&mut out, &stroke, false);
                }
            }
        }
        sp(&mut out, 2);
        for (p, c) in &g.internal_cut {
            poly(&mut out, p, *c);
        }
        for n in &g.notches {
            for mk in &n.marks {
                poly(&mut out, mk, false);
            }
        }
        poly(&mut out, &g.cut, true);
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
        bb = [
            bb[0].min(p.x),
            bb[1].min(p.y),
            bb[2].max(p.x),
            bb[3].max(p.y),
        ];
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
