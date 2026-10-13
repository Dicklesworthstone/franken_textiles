//! GarmentCode JSON (`docs/spec/03` §7.11): the net sew-line shapes of every simulated instance as panels with
//! relative-curvature edges, and the seams as 1:1 stitches after splitting multi-edge sides at matched arclength
//! points (notches included), for a 3D sewing check in GarmentCode.

use std::collections::BTreeMap;

use ftx_core::curve::{abs_to_rel, curve_points, rel_to_abs};
use ftx_core::polyline::{cumulative, polyline_length};
use ftx_core::{Refusal, RefusalCode, Vec2, det, np, py};
use ftx_pattern::Instance;
use serde_json::Value;

use crate::ctx::Ctx;
use crate::pyjson::PyJson;

#[derive(Clone, Debug)]
struct Seg {
    /// 1 line, 2 quadratic, 3 cubic (the Bézier degree).
    degree: usize,
    ctrl: Vec<Vec2>,
    edge: String,
    length: f64,
    id: u64,
}

/// A Bézier in Bernstein form at parameters `ts`, summed term by term like the reference.
fn bez(ctrl: &[Vec2], ts: &[f64]) -> Vec<Vec2> {
    let k = ctrl.len() - 1;
    let pow = |x: f64, e: usize| match e {
        0 => 1.0,
        1 => x,
        2 => x * x,
        3 => det::cube(x),
        _ => det::powi(x, e as i32),
    };
    let comb =
        |n: usize, r: usize| (0..r).fold(1u64, |a, i| a * (n - i) as u64 / (i as u64 + 1)) as f64;
    ts.iter()
        .map(|&t| {
            let mut out = Vec2::ZERO;
            for (i, c) in ctrl.iter().enumerate() {
                let w = comb(k, i) * pow(t, i) * pow(1.0 - t, k - i);
                out.x += w * c.x;
                out.y += w * c.y;
            }
            out
        })
        .collect()
}

fn seg_len(s: &Seg) -> f64 {
    if s.degree == 1 {
        let d = s.ctrl[1] - s.ctrl[0];
        return (d.x * d.x + d.y * d.y).sqrt();
    }
    polyline_length(&bez(&s.ctrl, &np::linspace(0.0, 1.0, 201)))
}

struct Ids(u64);

impl Ids {
    fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
}

/// Split at arclength fraction `u`: a line at that point, a Bézier at the parameter found by interpolating 801
/// samples, by de Casteljau.
fn split_seg(s: &Seg, u: f64, ids: &mut Ids) -> (Seg, Seg) {
    let mk = |degree, ctrl: Vec<Vec2>, ids: &mut Ids| {
        let mut seg = Seg {
            degree,
            ctrl,
            edge: s.edge.clone(),
            length: 0.0,
            id: ids.next(),
        };
        seg.length = seg_len(&seg);
        seg
    };
    if s.degree == 1 {
        let m = s.ctrl[0] + (s.ctrl[1] - s.ctrl[0]) * u;
        return (
            mk(1, vec![s.ctrl[0], m], ids),
            mk(1, vec![m, s.ctrl[1]], ids),
        );
    }
    let ts = np::linspace(0.0, 1.0, 801);
    let cum = cumulative(&bez(&s.ctrl, &ts));
    let t = np::interp(u * cum[cum.len() - 1], &cum, &ts);
    let mut pts = s.ctrl.clone();
    let (mut left, mut right) = (vec![pts[0]], vec![pts[pts.len() - 1]]);
    while pts.len() > 1 {
        pts = pts
            .windows(2)
            .map(|w| w[0] * (1.0 - t) + w[1] * t)
            .collect();
        left.push(pts[0]);
        right.push(pts[pts.len() - 1]);
    }
    right.reverse();
    (mk(s.degree, left, ids), mk(s.degree, right, ids))
}

fn edge_segs(inst: &Instance, eid: &str, ids: &mut Ids) -> Result<Vec<Seg>, Refusal> {
    let bad = |m: String| Refusal::new(RefusalCode::InvalidDocument, m);
    let e = inst.edge(eid).map_err(|x| bad(x.to_string()))?;
    let get = |v: &str| {
        inst.verts
            .get(v)
            .ok_or_else(|| bad(format!("{}: no vertex {v}", inst.id)))
    };
    let (p0, p1) = (get(&e.from)?, get(&e.to)?);
    let kind = e
        .curve
        .as_ref()
        .map_or(Ok(ftx_core::curve::CurveKind::Line), ftx_core::Curve::kind)
        .map_err(|x| bad(x.to_string()))?;
    let mut segs = Vec::new();
    match kind {
        ftx_core::curve::CurveKind::Line => segs.push((1, vec![p0, p1])),
        ftx_core::curve::CurveKind::Quad | ftx_core::curve::CurveKind::Cubic => {
            let c = e
                .curve
                .as_ref()
                .map(ftx_core::Curve::controls)
                .unwrap_or_default();
            let mut ctrl = vec![p0];
            ctrl.extend(c.iter().map(|&th| rel_to_abs(p0, p1, th)));
            ctrl.push(p1);
            segs.push((ctrl.len() - 1, ctrl));
        }
        ftx_core::curve::CurveKind::Poly => {
            let pts =
                curve_points(p0, p1, e.curve.as_ref(), 200).map_err(|x| bad(x.to_string()))?;
            segs.extend(pts.windows(2).map(|w| (1, vec![w[0], w[1]])));
        }
    }
    Ok(segs
        .into_iter()
        .map(|(degree, ctrl)| {
            let mut s = Seg {
                degree,
                ctrl,
                edge: eid.to_string(),
                length: 0.0,
                id: ids.next(),
            };
            s.length = seg_len(&s);
            s
        })
        .collect())
}

type Segs = BTreeMap<String, Vec<Seg>>;

/// Split a seam side's segments at side fractions `fr`; the side's sub-segments in order.
fn split_side(segs: &mut Segs, eids: &[String], fr: &[f64], ids: &mut Ids) -> Vec<Seg> {
    let flat: Vec<(String, Seg)> = eids
        .iter()
        .flat_map(|e| segs[e].iter().map(move |s| (e.clone(), s.clone())))
        .collect();
    let tot: f64 = flat.iter().map(|(_, s)| s.length).sum();
    let mut acc = 0.0;
    let mut new: BTreeMap<String, Vec<Seg>> =
        eids.iter().map(|e| (e.clone(), Vec::new())).collect();
    for (e, s) in flat {
        let (f0, f1) = (acc / tot, (acc + s.length) / tot);
        acc += s.length;
        let local: Vec<f64> = fr
            .iter()
            .filter(|&&f| f0 + 1e-9 < f && f < f1 - 1e-9)
            .map(|&f| (f - f0) / (f1 - f0))
            .collect();
        let (mut cur, mut done) = (s, 0.0);
        for u in local {
            let uu = (u - done) / (1.0 - done);
            let (left, right) = split_seg(&cur, uu, ids);
            if let Some(v) = new.get_mut(&e) {
                v.push(left);
            }
            cur = right;
            done = u;
        }
        if let Some(v) = new.get_mut(&e) {
            v.push(cur);
        }
    }
    let mut out = Vec::new();
    for e in eids {
        let v = new.remove(e).unwrap_or_default();
        out.extend(v.iter().cloned());
        segs.insert(e.clone(), v);
    }
    out
}

/// The GarmentCode file `<S>.garmentcode.json`, its name and the manifest entry (panels, stitches, warnings).
///
/// # Errors
/// A malformed edge or curve; a stitch naming a sub-edge no panel kept (the reference raises there too).
pub fn export_garmentcode(ctx: &Ctx) -> Result<(String, String, PyJson), Refusal> {
    let pat = ctx.pattern;
    let raw = pat.raw();
    let insts: Vec<&Instance> = pat
        .instances
        .iter()
        .filter(|i| raw["pieces"][i.piece_index].get("sim") != Some(&Value::Bool(false)))
        .collect();
    let mut ids = Ids(0);
    let mut segs: BTreeMap<String, Segs> = BTreeMap::new();
    for inst in &insts {
        let mut m = Segs::new();
        for e in &inst.edges {
            m.insert(e.id.clone(), edge_segs(inst, &e.id, &mut ids)?);
        }
        segs.insert(inst.id.clone(), m);
    }
    let mut warnings: Vec<String> = Vec::new();
    struct Plan {
        seam_id: String,
        ia: String,
        ea: Vec<String>,
        sa: Vec<f64>,
        ib: String,
        eb: Vec<String>,
        sb_own: Vec<f64>,
        rev: bool,
    }
    let mut plan = Vec::new();
    let seams = raw["seams"].as_array().cloned().unwrap_or_default();
    for seam in &seams {
        let sid = match seam.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => "None".to_string(),
        };
        let (a_ref, b_ref) = (
            seam["a"].as_str().unwrap_or(""),
            seam["b"].as_str().unwrap_or(""),
        );
        let (ia, ea) = match pat.side(a_ref) {
            Ok((i, e)) => (i, e),
            Err(err) => {
                warnings.push(format!("seam {sid}: {err}"));
                continue;
            }
        };
        let (ib, eb) = match pat.side(b_ref) {
            Ok((i, e)) => (i, e),
            Err(err) => {
                warnings.push(format!("seam {sid}: {err}"));
                continue;
            }
        };
        if !segs.contains_key(&ia.id) || !segs.contains_key(&ib.id) {
            warnings.push(format!(
                "seam {sid}: instance excluded from simulation (sim: false)"
            ));
            continue;
        }
        let bounds = |iid: &str, eids: &[String]| -> (Vec<f64>, f64) {
            let ls: Vec<f64> = eids
                .iter()
                .flat_map(|e| segs[iid][e].iter().map(|s| s.length))
                .collect();
            let tot: f64 = ls.iter().sum();
            let cs = np::cumsum(&ls);
            (
                cs[..cs.len().saturating_sub(1)]
                    .iter()
                    .map(|c| c / tot)
                    .collect(),
                tot,
            )
        };
        let (ba, la) = bounds(&ia.id, &ea);
        let (bb_own, _lb) = bounds(&ib.id, &eb);
        let rev = seam
            .get("reverse")
            .is_none_or(|v| v.as_bool().unwrap_or(true));
        let na = pat.notch_fractions(a_ref).unwrap_or_default();
        let mut nb = pat.notch_fractions(b_ref).unwrap_or_default();
        if rev {
            nb = nb.iter().map(|f| 1.0 - f).collect();
            nb.sort_by(f64::total_cmp);
        }
        let (ka, kb) = if !na.is_empty() && na.len() == nb.len() {
            let mut ka = vec![0.0];
            ka.extend(&na);
            ka.push(1.0);
            let mut kb = vec![0.0];
            kb.extend(&nb);
            kb.push(1.0);
            (ka, kb)
        } else {
            (vec![0.0, 1.0], vec![0.0, 1.0])
        };
        let bb: Vec<f64> = if rev {
            let mut v: Vec<f64> = bb_own.iter().map(|f| 1.0 - f).collect();
            v.sort_by(f64::total_cmp);
            v
        } else {
            bb_own
        };
        let to_b = |f: f64| np::interp(f, &ka, &kb);
        let to_a = |f: f64| np::interp(f, &kb, &ka);
        let mut cands: Vec<(f64, f64, String)> =
            ba.iter().map(|&f| (f, to_b(f), "a".to_string())).collect();
        cands.extend(bb.iter().map(|&f| (to_a(f), f, "b".to_string())));
        cands.extend(
            ka[1..ka.len() - 1]
                .iter()
                .zip(&kb[1..kb.len() - 1])
                .map(|(&fa, &fb)| (fa, fb, "k".to_string())),
        );
        cands.sort_by(|x, y| {
            x.0.total_cmp(&y.0)
                .then(x.1.total_cmp(&y.1))
                .then(x.2.cmp(&y.2))
        });
        let tol = 0.05 / la.max(1e-9);
        let mut pairs: Vec<(f64, f64, String)> = Vec::new();
        for (fa, fb, src) in cands {
            if fa <= tol || fa >= 1.0 - tol {
                continue;
            }
            match pairs.last_mut() {
                Some(last) if (fa - last.0).abs() <= tol => {
                    let na_ = if src == "a" { fa } else { last.0 };
                    let nb_ = if src == "b" { fb } else { last.1 };
                    *last = (na_, nb_, format!("{}{src}", last.2));
                }
                _ => pairs.push((fa, fb, src)),
            }
        }
        let sa: Vec<f64> = pairs.iter().map(|p| p.0).collect();
        let aligned: Vec<f64> = pairs.iter().map(|p| p.1).collect();
        let sb_own = if rev {
            let mut v: Vec<f64> = aligned.iter().map(|f| 1.0 - f).collect();
            v.sort_by(f64::total_cmp);
            v
        } else {
            aligned
        };
        plan.push(Plan {
            seam_id: sid,
            ia: ia.id.clone(),
            ea,
            sa,
            ib: ib.id.clone(),
            eb,
            sb_own,
            rev,
        });
    }
    let mut sides: Vec<(Vec<u64>, Vec<u64>, bool)> = Vec::new();
    for p in &plan {
        let Some(ma) = segs.get_mut(&p.ia) else {
            continue;
        };
        let a = split_side(ma, &p.ea, &p.sa, &mut ids);
        let Some(mb) = segs.get_mut(&p.ib) else {
            continue;
        };
        let b = split_side(mb, &p.eb, &p.sb_own, &mut ids);
        if a.len() != b.len() {
            warnings.push(format!(
                "seam {}: could not pair sub-edges ({} vs {})",
                p.seam_id,
                a.len(),
                b.len()
            ));
            continue;
        }
        sides.push((
            a.iter().map(|s| s.id).collect(),
            b.iter().map(|s| s.id).collect(),
            p.rev,
        ));
    }
    let mut panels = Vec::new();
    let mut order = Vec::new();
    let mut edge_map = Vec::new();
    let mut index: BTreeMap<u64, (String, usize)> = BTreeMap::new();
    let top = raw
        .get("placement3d")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or(Value::Null);
    for inst in &insts {
        let name = inst.id.replace('.', "_");
        let flat: Vec<&Seg> = inst
            .edges
            .iter()
            .flat_map(|e| segs[&inst.id][&e.id].iter())
            .collect();
        let n = flat.len();
        let mut verts = Vec::new();
        let mut edges = Vec::new();
        let mut emap = Vec::new();
        for (k, s) in flat.iter().enumerate() {
            verts.push(PyJson::floats(&[
                py::round(s.ctrl[0].x / 10.0, 5),
                py::round(s.ctrl[0].y / 10.0, 5),
            ]));
            let mut ed = vec![(
                "endpoints",
                PyJson::List(vec![
                    PyJson::Int(k as i64),
                    PyJson::Int(((k + 1) % n) as i64),
                ]),
            )];
            if s.degree > 1 {
                let (a, b) = (s.ctrl[0], s.ctrl[s.ctrl.len() - 1]);
                let params: Vec<PyJson> = s.ctrl[1..s.ctrl.len() - 1]
                    .iter()
                    .map(|&c| {
                        let [t, h] = abs_to_rel(a, b, c).unwrap_or([0.0, 0.0]);
                        PyJson::floats(&[py::round(t, 6), py::round(h, 6)])
                    })
                    .collect();
                ed.push((
                    "curvature",
                    PyJson::dict(vec![
                        (
                            "type",
                            PyJson::str(if s.degree == 2 { "quadratic" } else { "cubic" }),
                        ),
                        ("params", PyJson::List(params)),
                    ]),
                ));
            }
            edges.push(PyJson::dict(ed));
            emap.push(PyJson::str(&s.edge));
            index.insert(s.id, (name.clone(), k));
        }
        let piece_pl = &raw["pieces"][inst.piece_index]["placement3d"];
        let mut pl = match piece_pl {
            Value::Object(o) => o
                .get(&inst.id)
                .or_else(|| o.get(inst.side.as_str()))
                .cloned()
                .unwrap_or_else(|| piece_pl.clone()),
            _ => Value::Null,
        };
        if let Some(t) = top
            .get(&inst.id)
            .filter(|t| t.as_object().is_some_and(|o| !o.is_empty()))
        {
            pl = t.clone();
        }
        let vec3 = |key: &str, scale: f64, digits: usize| -> PyJson {
            let v: Vec<f64> = pl
                .get(key)
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_else(|| vec![0.0, 0.0, 0.0]);
            PyJson::floats(
                &v.iter()
                    .map(|x| {
                        if scale != 1.0 {
                            py::round(x / scale, digits)
                        } else {
                            *x
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        };
        panels.push((
            name.clone(),
            PyJson::dict(vec![
                ("translation", vec3("translation", 10.0, 4)),
                ("rotation", vec3("rotation", 1.0, 0)),
                ("vertices", PyJson::List(verts)),
                ("edges", PyJson::List(edges)),
            ]),
        ));
        order.push(PyJson::str(&name));
        edge_map.push((name, PyJson::List(emap)));
    }
    let mut stitches = Vec::new();
    for (a, b, rev) in &sides {
        let k_n = a.len();
        for k in 0..k_n {
            let sb = if *rev { b[k_n - 1 - k] } else { b[k] };
            let side = |id: u64| -> Result<PyJson, Refusal> {
                let (p, e) = index.get(&id).ok_or_else(|| {
                    Refusal::new(
                        RefusalCode::InvalidDocument,
                        "a stitch names a sub-edge no panel kept",
                    )
                })?;
                Ok(PyJson::dict(vec![
                    ("panel", PyJson::str(p)),
                    ("edge", PyJson::Int(*e as i64)),
                ]))
            };
            let mut st = vec![side(a[k])?, side(sb)?];
            if !rev {
                st.push(PyJson::str("right_wrong"));
            }
            stitches.push(PyJson::List(st));
        }
    }
    let (n_panels, n_stitches, n_warnings) = (panels.len(), stitches.len(), warnings.len());
    let spec = PyJson::dict(vec![
        (
            "pattern",
            PyJson::dict(vec![
                ("panels", PyJson::Dict(panels)),
                ("stitches", PyJson::List(stitches)),
                ("panel_order", PyJson::List(order)),
            ]),
        ),
        ("parameters", PyJson::Dict(Vec::new())),
        ("parameter_order", PyJson::List(Vec::new())),
        (
            "properties",
            PyJson::dict(vec![
                ("curvature_coords", PyJson::str("relative")),
                ("normalize_panel_translation", PyJson::Bool(false)),
                ("normalized_edge_loops", PyJson::Bool(true)),
                ("units_in_meter", PyJson::Int(100)),
            ]),
        ),
        (
            "fabric_pattern_export",
            PyJson::dict(vec![
                ("source_schema", PyJson::from_value(raw.get("schema").unwrap_or(&Value::Null))),
                ("style", PyJson::str(&ctx.style)),
                ("size", PyJson::str(&ctx.size)),
                ("edge_map", PyJson::Dict(edge_map)),
                ("warnings", PyJson::List(warnings.into_iter().map(PyJson::Str).collect())),
                (
                    "notes",
                    PyJson::List(
                        [
                            "net sew-line shapes only: no seam allowance, notches, grain, internal lines or decor",
                            "multi-edge seam sides are split at matched arclength points (incl. notches) into 1:1 stitches",
                            "poly edges become straight sub-edges; translation/rotation are zero unless placement3d is given",
                            "loops are CCW as drafted (public side toward +z for rotation 0)",
                        ]
                        .iter()
                        .map(|s| PyJson::str(*s))
                        .collect(),
                    ),
                ),
            ]),
        ),
    ]);
    let name = format!("{}.garmentcode.json", ctx.style);
    let entry = PyJson::dict(vec![
        ("file", PyJson::str(&name)),
        ("panels", PyJson::Int(n_panels as i64)),
        ("stitches", PyJson::Int(n_stitches as i64)),
        ("warnings", PyJson::Int(n_warnings as i64)),
    ]);
    Ok((name, spec.dumps(Some(1)) + "\n", entry))
}
