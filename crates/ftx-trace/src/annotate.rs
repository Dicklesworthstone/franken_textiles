//! `names.json` and `annotate` (spec 04 §10.10): the agent's names, cut, fold, drawn side, topology fixes
//! (`merge_at`, `split`), seams (with left-side twins) and checks, applied to a traced draft.

use ftx_core::curve::{Curve, CurveKind, abs_to_rel, mirror_curve};
use ftx_core::polyline::{cumulative, polyline_length};
use ftx_core::{Vec2, np, py};
use ftx_pattern::doc::{Cut, Edge, Grain, PatternDoc, Piece, Seam};
use ftx_pattern::{Pattern, PyError};
use serde_json::{Map, Value, json};

use crate::geos::simplify;
use crate::ordered::OValue;
use crate::refs::{EdgeMaps, PieceMap, edge_points, outline_size, resolve_side, twin, vertex};

/// `names.json`, read in document order.
pub type Names = OValue;

fn r3(x: f64) -> f64 {
    py::round(x, 3)
}

/// Python `str(value)` for the few places the reference formats a JSON value.
pub(crate) fn py_str(v: &OValue) -> String {
    match v {
        OValue::String(s) => s.clone(),
        OValue::Null => "None".into(),
        OValue::Bool(b) => if *b { "True" } else { "False" }.into(),
        OValue::Number(n) => n.as_i64().map_or_else(
            || py::repr_f64(n.as_f64().unwrap_or(f64::NAN)),
            |i| i.to_string(),
        ),
        OValue::Array(_) | OValue::Object(_) => v.to_value().to_string(),
    }
}

fn check_id(name: &str, what: &str) -> Result<(), String> {
    if name.is_empty() || name.chars().any(|c| "./+ :".contains(c)) {
        return Err(format!(
            "{what} {} must be non-empty and contain no '.', '/', '+', ':' or spaces",
            py::repr_str(name)
        ));
    }
    Ok(())
}

fn trace_mut(piece: &mut Piece) -> Option<&mut Map<String, Value>> {
    piece.extra.get_mut("trace").and_then(Value::as_object_mut)
}

/// Mirrors a piece in x about its bounding-box centre (drawn as the wearer's left → wearer's right), keeping
/// it counter-clockwise: vertices (r3), edges reversed with mirrored curves, notch fractions, grain and its
/// angle, internal and trace-mark points, and the trace's `to_px` map.
pub fn mirror_piece(piece: &mut Piece) {
    let xs: Vec<f64> = piece.vertices.iter().map(|(_, p)| p.x).collect();
    let cx = 0.5
        * (xs.iter().copied().fold(f64::INFINITY, f64::min)
            + xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    let mx = |p: Vec2| Vec2::new(r3(2.0 * cx - p.x), r3(p.y));
    for (_, p) in &mut piece.vertices.0 {
        *p = mx(*p);
    }
    piece.edges = piece
        .edges
        .iter()
        .rev()
        .map(|e| {
            let mut m = e.clone();
            m.from.clone_from(&e.to);
            m.to.clone_from(&e.from);
            if e.curve
                .as_ref()
                .is_some_and(|c| c.kind.is_some() || c.c.is_some() || !c.extra.is_empty())
            {
                m.curve = mirror_curve(e.curve.as_ref());
            }
            m
        })
        .collect();
    for n in piece.notches.iter_mut().flatten() {
        n.s = py::round(1.0 - n.s, 4);
    }
    if let Some([g0, g1]) = piece.grain.as_ref().and_then(Grain::points) {
        piece.grain = Some(Grain::Points(vec![mx(g0), mx(g1)]));
        let meta = piece
            .extra
            .entry("grain_meta")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(meta) = meta.as_object_mut() {
            let a = meta
                .get("angle_deg")
                .and_then(Value::as_f64)
                .unwrap_or(90.0);
            let mut r = (180.0 - a).rem_euclid(180.0);
            if r == 0.0 {
                r = 0.0; // Python's float modulo gives +0.0
            }
            meta.insert("angle_deg".into(), json!(py::round(r, 2)));
        }
    }
    for it in piece
        .internal
        .iter_mut()
        .flatten()
        .chain(piece.trace_marks.iter_mut().flatten())
    {
        for p in &mut it.pts {
            *p = mx(*p);
        }
    }
    if let Some(tr) = trace_mut(piece)
        && let Some(a) = tr.get("to_px").and_then(Value::as_array)
    {
        // [A; 0 0 1] @ [[-1, 0, 2cx], [0, 1, 0], [0, 0, 1]], first two rows, each entry the full sum of
        // three products as numpy's matmul takes it (0·-1 + -1·0 + c·0 is +0.0; negating 0.0 gives -0.0)
        let m = [[-1.0, 0.0, 2.0 * cx], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let out: Vec<Value> = a
            .iter()
            .filter_map(|r| {
                let r: Vec<f64> = r.as_array()?.iter().filter_map(Value::as_f64).collect();
                let col = |j: usize| r[0] * m[0][j] + r[1] * m[1][j] + r[2] * m[2][j];
                (r.len() == 3).then(|| json!([col(0), col(1), col(2)]))
            })
            .collect();
        tr.insert("to_px".into(), Value::Array(out));
        tr.insert("mirrored_from_drawing".into(), json!(true));
    }
}

/// An edge through an absolute polyline: straight when every point is within `tol` of the chord, else a
/// relative `poly` of the points Douglas–Peucker keeps at `tol / 3` (ends excluded). Other keys come from
/// `meta`.
fn edge_from_points(
    eid: &str,
    va: &str,
    vb: &str,
    pts: &[Vec2],
    tol: f64,
    meta: &Edge,
) -> Result<Edge, PyError> {
    let (p0, p1) = (pts[0], pts[pts.len() - 1]);
    let d = p1 - p0;
    let l = match d.norm() {
        0.0 => 1e-9,
        l => l,
    };
    let dev = pts
        .iter()
        .map(|q| (d.x * (q.y - p0.y) - d.y * (q.x - p0.x)).abs() / l)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut curve = None;
    if pts.len() > 2 && dev > tol {
        let kept = simplify(pts, tol / 3.0);
        if kept.len() > 2 {
            let c = kept[1..kept.len() - 1]
                .iter()
                .map(|&q| {
                    abs_to_rel(p0, p1, q)
                        .map(|[t, h]| [py::round(t, 5), py::round(h, 5)])
                        .map_err(|e| PyError::Value(e.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            curve = Some(Curve::new(CurveKind::Poly, c));
        }
    }
    Ok(Edge {
        id: eid.to_string(),
        from: va.to_string(),
        to: vb.to_string(),
        curve,
        sa: meta.sa,
        extra: meta.extra.clone(),
    })
}

/// Removes a (spurious) corner: the edge into `vid` and the edge out of it become one edge with the first
/// id, re-fitted; notches move by length share; `on_fold` follows; the vertex and its trace corner go.
///
/// # Errors
/// An unknown vertex, or a loop that would drop below 3 edges.
pub fn merge_at(piece: &mut Piece, vid: &str) -> Result<(), PyError> {
    let n = piece.edges.len();
    let i = piece.edges.iter().position(|e| e.to == vid);
    let Some(i) = i.filter(|_| n > 3) else {
        return Err(PyError::Value(format!(
            "{}: cannot merge at {} (unknown vertex, or the loop would drop below 3 edges)",
            piece.id,
            py::repr_str(vid)
        )));
    };
    let j = (i + 1) % n;
    let (ea, eb) = (piece.edges[i].clone(), piece.edges[j].clone());
    let pa = edge_points(piece, &ea.id, 200)?;
    let pb = edge_points(piece, &eb.id, 200)?;
    let (la, lb) = (polyline_length(&pa), polyline_length(&pb));
    let size = outline_size(piece)?;
    let pts: Vec<Vec2> = pa.iter().chain(&pb[1..]).copied().collect();
    let new = edge_from_points(&ea.id, &ea.from, &eb.to, &pts, (1e-3 * size).max(1e-6), &ea)?;
    for nt in piece.notches.iter_mut().flatten() {
        if nt.edge == ea.id {
            nt.s = py::round(nt.s * la / (la + lb), 4);
        } else if nt.edge == eb.id {
            nt.edge.clone_from(&ea.id);
            nt.s = py::round((la + nt.s * lb) / (la + lb), 4);
        }
    }
    piece.edges[i] = new;
    piece.edges.remove(j);
    piece.vertices.0.retain(|(k, _)| k != vid);
    if let Some(cut) = piece.cut.as_mut()
        && cut
            .on_fold
            .as_deref()
            .is_some_and(|f| f == ea.id || f == eb.id)
    {
        cut.on_fold = Some(ea.id.clone());
    }
    if let Some(corners) = trace_mut(piece)
        .and_then(|t| t.get_mut("corners"))
        .and_then(Value::as_object_mut)
    {
        corners.remove(vid);
    }
    Ok(())
}

/// Inserts vertices at arclength fractions of an edge: parts keep `eid`, then `eid`+`b`, `c`, …; new
/// vertices are `eid`+`s1`, `s2`, … (r3); notches go to the part that holds them. Returns the new edge ids.
///
/// # Errors
/// An unknown edge, fractions outside (0, 1), or ids that already exist.
pub fn split_edge(piece: &mut Piece, eid: &str, fractions: &[f64]) -> Result<Vec<String>, PyError> {
    let Some(i) = piece.edges.iter().position(|e| e.id == eid) else {
        return Err(PyError::Value(format!(
            "{}: no edge {} to split",
            piece.id,
            py::repr_str(eid)
        )));
    };
    let mut fr = fractions.to_vec();
    fr.sort_by(f64::total_cmp);
    if fr.is_empty() || fr[0] <= 0.0 || fr[fr.len() - 1] >= 1.0 {
        return Err(PyError::Value(format!(
            "{}/{eid}: split fractions must lie strictly between 0 and 1",
            piece.id
        )));
    }
    let e = piece.edges[i].clone();
    let pts = edge_points(piece, eid, 200)?;
    let s = cumulative(&pts);
    let total = s[s.len() - 1];
    let cuts: Vec<f64> = fr.iter().map(|f| f * total).collect();
    let size = outline_size(piece)?;
    let mut vids = vec![e.from.clone()];
    vids.extend((1..=fr.len()).map(|k| format!("{eid}s{k}")));
    vids.push(e.to.clone());
    let (xs, ys): (Vec<f64>, Vec<f64>) = pts.iter().map(|p| (p.x, p.y)).unzip();
    for (k, &c) in cuts.iter().enumerate() {
        if piece.vertices.contains(&vids[k + 1]) {
            return Err(PyError::Value(format!(
                "{}: vertex id {} already exists",
                piece.id,
                py::repr_str(&vids[k + 1])
            )));
        }
        let p = Vec2::new(r3(np::interp(c, &s, &xs)), r3(np::interp(c, &s, &ys)));
        piece.vertices.insert(&vids[k + 1], p);
    }
    let mut bounds = vec![0.0];
    bounds.extend(&cuts);
    bounds.push(total);
    let mut new_ids = vec![eid.to_string()];
    new_ids.extend((0..fr.len()).map(|k| format!("{eid}{}", char::from(b'b' + k as u8))));
    let mut taken: Vec<&str> = piece
        .edges
        .iter()
        .map(|x| x.id.as_str())
        .filter(|x| *x != eid && new_ids.iter().any(|n| n == x))
        .collect();
    if !taken.is_empty() {
        taken.sort_unstable();
        taken.dedup();
        return Err(PyError::Value(format!(
            "{}: split ids {} already exist",
            piece.id,
            py::repr_str_list(&taken)
        )));
    }
    let tol = (1e-3 * size).max(1e-6);
    let mut parts = Vec::new();
    for k in 0..bounds.len() - 1 {
        let (a, b) = (bounds[k], bounds[k + 1]);
        let mut poly = vec![vertex(piece, &vids[k])?];
        poly.extend(
            pts.iter()
                .zip(&s)
                .filter(|(_, d)| **d > a && **d < b)
                .map(|(p, _)| *p),
        );
        poly.push(vertex(piece, &vids[k + 1])?);
        parts.push(edge_from_points(
            &new_ids[k],
            &vids[k],
            &vids[k + 1],
            &poly,
            tol,
            &e,
        )?);
    }
    for nt in piece.notches.iter_mut().flatten() {
        if nt.edge == eid {
            let pos = nt.s * total;
            let k = bounds.partition_point(|&b| b <= pos).saturating_sub(1);
            let k = k.min(parts.len() - 1);
            nt.edge.clone_from(&new_ids[k]);
            nt.s = py::round((pos - bounds[k]) / (bounds[k + 1] - bounds[k]), 4);
        }
    }
    piece.edges.splice(i..=i, parts);
    Ok(new_ids)
}

/// An ordered `{old: new}` map of strings from names.json.
pub(crate) fn rename_map(v: Option<&OValue>) -> Vec<(String, String)> {
    v.map(OValue::entries)
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k.to_string(), py_str(v)))
        .collect()
}

pub(crate) fn lookup<'a>(map: &'a [(String, String)], k: &'a str) -> &'a str {
    map.iter()
        .find(|(o, _)| o == k)
        .map_or(k, |(_, n)| n.as_str())
}

fn has_duplicates(ids: &[String]) -> bool {
    ids.iter().enumerate().any(|(i, x)| ids[..i].contains(x))
}

/// Applies `names.json` to a traced draft (spec 04 §10.10): per piece, in this order, `merge_at` and
/// `split` (traced ids), renames, notch and mark edge renames, `cut`, `drawn_side: L` mirroring and the
/// copied keys; then seams (sides resolved, left twins added with `mirror_seams`) and checks. The result
/// passes the structure check.
///
/// # Errors
/// The reference's message: unknown pieces, a bad id or rename, a bad cut or side, or `annotate failed:`
/// followed by every seam, check or structure problem.
pub fn annotate(draft: &PatternDoc, names: &Names) -> Result<PatternDoc, String> {
    annotate_twins(draft, names).map(|(data, _)| data)
}

/// [`annotate`], and for each seam of `names.json` whether a left-side twin followed it.
pub(crate) fn annotate_twins(
    draft: &PatternDoc,
    names: &Names,
) -> Result<(PatternDoc, Vec<bool>), String> {
    let mut data = draft.clone();
    let mut twins: Vec<bool> = Vec::new();
    let specs: Vec<(&str, &OValue)> = names.get("pieces").map(OValue::entries).unwrap_or_default();
    let mut drop: Vec<String> = Vec::new();
    for d in names.get("drop").map(OValue::items).unwrap_or_default() {
        let d = py_str(d);
        if !drop.contains(&d) {
            drop.push(d);
        }
    }
    for (k, v) in &specs {
        if v.get("drop").is_some_and(OValue::truthy) && !drop.iter().any(|d| d == k) {
            drop.push((*k).to_string());
        }
    }
    let known: Vec<&str> = data.pieces.iter().map(|p| p.id.as_str()).collect();
    let bad: Vec<&str> = specs
        .iter()
        .map(|(k, _)| *k)
        .chain(drop.iter().map(String::as_str))
        .filter(|k| !known.contains(k))
        .collect();
    if !bad.is_empty() {
        let mut have = known.clone();
        have.sort_unstable();
        have.dedup();
        return Err(format!(
            "names.json mentions unknown pieces {} (draft has {})",
            py::repr_str_list(&bad),
            py::repr_str_list(&have)
        ));
    }
    data.pieces.retain(|p| !drop.contains(&p.id));
    let mut piece_map_ordered: Vec<(String, String)> = Vec::new();
    let mut piece_map = PieceMap::new();
    let mut edge_maps = EdgeMaps::new();
    let empty = OValue::Null;
    for p in &mut data.pieces {
        let old = p.id.clone();
        let sp = specs
            .iter()
            .rev()
            .find(|(k, _)| *k == old)
            .map_or(&empty, |(_, v)| *v);
        for vid in sp.get("merge_at").map(OValue::items).unwrap_or_default() {
            merge_at(p, &py_str(vid)).map_err(|e| e.to_string())?;
        }
        for (eid, fr) in sp.get("split").map(OValue::entries).unwrap_or_default() {
            let fractions: Vec<f64> = match fr {
                OValue::Array(a) => a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect(),
                x => vec![x.as_f64().unwrap_or(f64::NAN)],
            };
            split_edge(p, eid, &fractions).map_err(|e| e.to_string())?;
        }
        let emap = rename_map(sp.get("edges"));
        let vmap = rename_map(sp.get("vertices"));
        for (k, v) in emap.iter().chain(&vmap) {
            check_id(v, &format!("{old}: new id for {k}"))?;
        }
        let edge_ids: Vec<String> = p.edges.iter().map(|e| e.id.clone()).collect();
        let mut missing: Vec<&str> = emap
            .iter()
            .map(|(k, _)| k.as_str())
            .filter(|k| !edge_ids.iter().any(|e| e == k))
            .collect();
        if !missing.is_empty() {
            missing.sort_unstable();
            missing.dedup();
            return Err(format!(
                "{old}: edges {} do not exist",
                py::repr_str_list(&missing)
            ));
        }
        let mut missing: Vec<&str> = vmap
            .iter()
            .map(|(k, _)| k.as_str())
            .filter(|k| !p.vertices.contains(k))
            .collect();
        if !missing.is_empty() {
            missing.sort_unstable();
            missing.dedup();
            return Err(format!(
                "{old}: vertices {} do not exist",
                py::repr_str_list(&missing)
            ));
        }
        let new_e: Vec<String> = edge_ids
            .iter()
            .map(|e| lookup(&emap, e).to_string())
            .collect();
        if has_duplicates(&new_e) {
            return Err(format!(
                "{old}: edge renames collide ({})",
                py::repr_str_list(&new_e)
            ));
        }
        let new_v: Vec<String> = p
            .vertices
            .iter()
            .map(|(k, _)| lookup(&vmap, k).to_string())
            .collect();
        if has_duplicates(&new_v) {
            return Err(format!(
                "{old}: vertex renames collide ({})",
                py::repr_str_list(&new_v)
            ));
        }
        for e in &mut p.edges {
            e.id = lookup(&emap, &e.id).to_string();
            e.from = lookup(&vmap, &e.from).to_string();
            e.to = lookup(&vmap, &e.to).to_string();
        }
        for (k, _) in &mut p.vertices.0 {
            *k = lookup(&vmap, k).to_string();
        }
        for n in p.notches.iter_mut().flatten() {
            n.edge = lookup(&emap, &n.edge).to_string();
        }
        for it in p
            .internal
            .iter_mut()
            .flatten()
            .chain(p.trace_marks.iter_mut().flatten())
        {
            if let Some(Value::String(ne)) = it.extra.get("near_edge") {
                let renamed = lookup(&emap, ne).to_string();
                it.extra.insert("near_edge".into(), Value::String(renamed));
            }
        }
        if let Some(cut) = sp.get("cut") {
            let mut cut = cut.to_value();
            let q = cut
                .get("qty")
                .and_then(Value::as_f64)
                .map_or(1, |q| q.trunc() as i64); // det-ok: a count
            if q != 1 && q != 2 {
                return Err(format!(
                    "{old}: cut.qty must be 1 or 2 (use separate pieces for more)"
                ));
            }
            if let Some(fold) = cut
                .get("on_fold")
                .filter(|f| f.as_str().is_some_and(|s| !s.is_empty()))
            {
                let fold = lookup(&emap, fold.as_str().unwrap_or_default()).to_string();
                if !p.edges.iter().any(|e| e.id == fold) {
                    return Err(format!(
                        "{old}: cut.on_fold {} is not an edge of this piece",
                        py::repr_str(&fold)
                    ));
                }
                cut["on_fold"] = Value::String(fold);
            }
            p.cut = Some(serde_json::from_value::<Cut>(cut).map_err(|e| e.to_string())?);
        }
        let side = sp
            .get("drawn_side")
            .map_or_else(|| "R".to_string(), py_str)
            .to_uppercase();
        if side != "R" && side != "L" {
            return Err(format!(
                "{old}: drawn_side must be 'R' or 'L' (wearer's side as drawn, public side up)"
            ));
        }
        if side == "L" {
            mirror_piece(p);
        }
        if let Some(v) = sp.get("name") {
            p.name = v.as_str().map(str::to_string);
        }
        if let Some(v) = sp.get("role") {
            p.role = v.as_str().map(str::to_string);
        }
        if let Some(v) = sp.get("material") {
            p.material = v.as_str().map(str::to_string);
        }
        for key in ["symmetric", "notes"] {
            if let Some(v) = sp.get(key) {
                p.extra.insert(key.into(), v.to_value());
            }
        }
        let new = sp.get("id").map_or_else(|| old.clone(), py_str);
        check_id(&new, &format!("{old}: new piece id"))?;
        piece_map.insert(old.clone(), new.clone());
        piece_map_ordered.retain(|(o, _)| *o != old);
        piece_map_ordered.push((old, new.clone()));
        edge_maps.insert(new.clone(), emap.into_iter().collect());
        p.id = new;
    }
    let ids: Vec<String> = data.pieces.iter().map(|p| p.id.clone()).collect();
    if has_duplicates(&ids) {
        return Err(format!(
            "piece ids collide after renaming: {}",
            py::repr_str_list(&ids)
        ));
    }
    let mut problems: Vec<String> = Vec::new();
    let mut seams: Vec<Seam> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mirror = names.get("mirror_seams").is_none_or(OValue::truthy);
    for (k, s) in names
        .get("seams")
        .map(OValue::items)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let sid = match s.get("id") {
            Some(v) if v.truthy() => v.clone(),
            _ => OValue::String(format!("s{}", k + 1)),
        };
        let sid_text = py_str(&sid);
        let side = |key: &str| -> Result<(String, bool), PyError> {
            let r = s.get(key).ok_or_else(|| PyError::Key(key.to_string()))?;
            resolve_side(&data, &py_str(r), &piece_map, &edge_maps)
        };
        let (a, ea, b, eb) =
            match side("a").and_then(|(a, ea)| side("b").map(|(b, eb)| (a, ea, b, eb))) {
                Ok(x) => x,
                Err(e) => {
                    problems.push(format!("seam {sid_text}: {e}"));
                    continue;
                }
            };
        let mut seam = Map::new();
        for (key, v) in s.entries() {
            if !matches!(key, "a" | "b" | "id" | "band") {
                seam.insert(key.to_string(), v.to_value());
            }
        }
        seam.insert("id".into(), sid.to_value());
        seam.insert("a".into(), Value::String(a.clone()));
        seam.insert("b".into(), Value::String(b.clone()));
        if let Some(band) = s.get("band") {
            seam.insert("ratio_band".into(), band.to_value());
        }
        seams.push(Seam(seam.clone()));
        twins.push(false);
        if mirror && (ea || eb) {
            let ta = if ea {
                twin(&data, &a).map_err(|e| e.to_string())?
            } else {
                None
            };
            let tb = if eb {
                twin(&data, &b).map_err(|e| e.to_string())?
            } else {
                None
            };
            if let (Some(ta), Some(tb)) = (ta, tb) {
                let mut t = seam;
                t.insert("id".into(), Value::String(format!("{sid_text}_L")));
                t.insert("a".into(), Value::String(ta));
                t.insert("b".into(), Value::String(tb));
                t.insert("twin_of".into(), sid.to_value());
                seams.push(Seam(t));
                if let Some(twinned) = twins.last_mut() {
                    *twinned = true;
                }
            } else {
                notes.push(format!(
                    "seam {sid_text}: no automatic left-side twin (one side is a single piece); declare it explicitly"
                ));
            }
        }
    }
    let mut checks: Vec<Value> = Vec::new();
    for (k, c) in names
        .get("checks")
        .map(OValue::items)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let cid = match c.get("id") {
            Some(v) if v.truthy() => v.clone(),
            _ => OValue::String(format!("k{}", k + 1)),
        };
        let refs = |key: &str| -> Result<Vec<Value>, PyError> {
            let list = c.get(key).ok_or_else(|| PyError::Key(key.to_string()))?;
            list.items()
                .iter()
                .map(|r| {
                    let r = py_str(r);
                    if r.contains('/') {
                        resolve_side(&data, &r, &piece_map, &edge_maps).map(|x| Value::String(x.0))
                    } else {
                        Ok(Value::String(r))
                    }
                })
                .collect()
        };
        match refs("a").and_then(|ra| refs("b").map(|rb| (ra, rb))) {
            Ok((ra, rb)) => {
                let mut out: Map<String, Value> = c
                    .entries()
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_value()))
                    .collect();
                out.insert("id".into(), cid.to_value());
                out.insert("a".into(), Value::Array(ra));
                out.insert("b".into(), Value::Array(rb));
                checks.push(Value::Object(out));
            }
            Err(e) => problems.push(format!("check {}: {e}", py_str(&cid))),
        }
    }
    data.seams = Some(seams);
    if !checks.is_empty() {
        data.extra.insert("checks".into(), Value::Array(checks));
    }
    let meta = data.meta.get_or_insert_with(Map::new);
    let garment = names.get("garment").map_or(Value::Null, OValue::to_value);
    let pm: Map<String, Value> = piece_map_ordered
        .into_iter()
        .map(|(o, n)| (o, Value::String(n)))
        .collect();
    meta.insert(
        "annotated".into(),
        json!({"garment": garment, "piece_map": pm, "notes": notes,
               "convention": "pieces are the WEARER'S RIGHT, public side up (drawn_side L pieces were mirrored)"}),
    );
    if names.get("garment").is_some_and(OValue::truthy) {
        meta.insert("garment".into(), garment);
    }
    if problems.is_empty() {
        problems = Pattern::from_doc(data.clone()).structure_errors();
    }
    if !problems.is_empty() {
        return Err(format!("annotate failed:\n  {}", problems.join("\n  ")));
    }
    Ok((data, twins))
}
