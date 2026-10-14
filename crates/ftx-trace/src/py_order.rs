//! The reference's key order for `annotate`, `scale-fit` and `alter` results (spec 04 §10.10, §10.14; spec
//! 01 §10).
//! Python's dicts keep insertion order and `json.dump` writes it, while the typed documents sort their
//! free-form maps.
//! So each result is laid out from a skeleton. The skeleton is the input with the reference's dict
//! operations replayed on it: every value it copies is kept exactly (an integer stays an integer, a `null`
//! stays), and every value it computes is marked. The typed result fills the marks, and
//! `PyJson::dumps(Some(1))` of the outcome is the reference's `json.dump(obj, fh, indent=1)`.

use ftx_export::pyjson::PyJson;
use ftx_pattern::PyError;
use ftx_pattern::doc::PatternDoc;
use serde_json::Value;

use crate::annotate::{Names, annotate_twins, lookup, py_str, rename_map};
use crate::ordered::OValue;
use crate::scale::fit_scale;

/// A node of the skeleton.
#[derive(Clone, Debug)]
enum Sk {
    /// A scalar the reference copies (from the input or names.json), written as it is.
    Keep(PyJson),
    /// A value the reference computes, taken from the typed result.
    Computed,
    /// An object in the reference's key order.
    Dict(Vec<(String, Sk)>),
    /// A list.
    List(Vec<Sk>),
}

impl Sk {
    fn from_py(v: &PyJson) -> Self {
        match v {
            PyJson::Dict(items) => Self::Dict(
                items
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::from_py(v)))
                    .collect(),
            ),
            PyJson::List(items) => Self::List(items.iter().map(Self::from_py).collect()),
            scalar => Self::Keep(scalar.clone()),
        }
    }

    /// A names.json value (numbers keep Python's int/float distinction).
    fn from_o(v: &OValue) -> Self {
        match v {
            OValue::Null => Self::Keep(PyJson::Null),
            OValue::Bool(b) => Self::Keep(PyJson::Bool(*b)),
            OValue::Number(n) => Self::Keep(match n.as_i64() {
                Some(i) if !n.is_f64() => PyJson::Int(i),
                _ => PyJson::Float(n.as_f64().unwrap_or(f64::NAN)),
            }),
            OValue::String(s) => Self::Keep(PyJson::Str(s.clone())),
            OValue::Array(a) => Self::List(a.iter().map(Self::from_o).collect()),
            OValue::Object(_) => Self::Dict(
                v.entries()
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), Self::from_o(v)))
                    .collect(),
            ),
        }
    }

    fn text(s: &str) -> Self {
        Self::Keep(PyJson::Str(s.to_string()))
    }

    fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Self> {
        match self {
            Self::Dict(items) => items.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Python's `d[key] = value`: in place, or appended.
    fn set(&mut self, key: &str, value: Self) {
        if let Self::Dict(items) = self {
            match items.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = value,
                None => items.push((key.to_string(), value)),
            }
        }
    }

    /// Python's `d.pop(key, None)`.
    fn remove(&mut self, key: &str) {
        if let Self::Dict(items) = self {
            items.retain(|(k, _)| k != key);
        }
    }

    /// The items of the list under `key` (none when absent).
    fn items_mut(&mut self, key: &str) -> &mut [Self] {
        match self.get_mut(key) {
            Some(Self::List(items)) => items,
            _ => Default::default(),
        }
    }

    /// Python `str()` of a copied scalar (ids are compared and renamed as text).
    fn as_text(&self) -> Option<String> {
        match self {
            Self::Keep(PyJson::Str(s)) => Some(s.clone()),
            Self::Keep(PyJson::Int(i)) => Some(i.to_string()),
            Self::Keep(PyJson::Float(f)) => Some(ftx_core::py::repr_f64(*f)),
            Self::Keep(PyJson::Bool(b)) => Some(if *b { "True" } else { "False" }.into()),
            Self::Keep(PyJson::Null) => Some("None".into()),
            _ => None,
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Self::Keep(PyJson::Null | PyJson::Bool(false)) => false,
            Self::Keep(PyJson::Int(i)) => *i != 0,
            Self::Keep(PyJson::Float(f)) => *f != 0.0,
            Self::Keep(PyJson::Str(s)) => !s.is_empty(),
            Self::Dict(items) => !items.is_empty(),
            Self::List(items) => !items.is_empty(),
            _ => true,
        }
    }

    /// The node as written when the typed result has nothing there (None if part of it is computed).
    fn copied(&self) -> Option<PyJson> {
        match self {
            Self::Keep(v) => Some(v.clone()),
            Self::Computed => None,
            Self::Dict(items) => items
                .iter()
                .map(|(k, v)| v.copied().map(|v| (k.clone(), v)))
                .collect::<Option<_>>()
                .map(PyJson::Dict),
            Self::List(items) => items
                .iter()
                .map(Self::copied)
                .collect::<Option<_>>()
                .map(PyJson::List),
        }
    }

    fn rename(&mut self, key: &str, map: &[(String, String)]) {
        if let Some(Self::Keep(PyJson::Str(s))) = self.get_mut(key) {
            *s = lookup(map, s).to_string();
        }
    }
}

/// Whether a copied scalar is the typed result's value (numbers compared as numbers, so `2` is `2.0`).
fn same(v: &PyJson, r: &Value) -> bool {
    match (v, r) {
        (PyJson::Null, Value::Null) => true,
        (PyJson::Bool(a), Value::Bool(b)) => a == b,
        (PyJson::Str(a), Value::String(b)) => a == b,
        (PyJson::Int(i), Value::Number(n)) => n.as_f64() == Some(*i as f64),
        (PyJson::Float(f), Value::Number(n)) => n.as_f64() == Some(*f),
        _ => false,
    }
}

/// The typed result `r` laid out as the skeleton `s` says: its key order, its copied scalars, and `r`'s
/// values where it computes. Keys only `r` has go last.
fn reorder(r: &Value, s: &Sk) -> PyJson {
    match (s, r) {
        (Sk::Keep(v), _) if same(v, r) => v.clone(),
        (Sk::Dict(items), Value::Object(m)) => {
            let mut out: Vec<(String, PyJson)> = Vec::new();
            for (k, sv) in items {
                if out.iter().any(|(q, _)| q == k) {
                    continue;
                }
                match m.get(k) {
                    Some(rv) => out.push((k.clone(), reorder(rv, sv))),
                    // a value the typed model leaves out (a null, a non-string name)
                    None => {
                        if let Some(v) = sv.copied() {
                            out.push((k.clone(), v));
                        }
                    }
                }
            }
            for (k, rv) in m {
                if !items.iter().any(|(q, _)| q == k) {
                    out.push((k.clone(), PyJson::from_value(rv)));
                }
            }
            PyJson::Dict(out)
        }
        (Sk::List(items), Value::Array(a)) if items.len() == a.len() => PyJson::List(
            items
                .iter()
                .zip(a)
                .map(|(sv, rv)| reorder(rv, sv))
                .collect(),
        ),
        _ => PyJson::from_value(r),
    }
}

/// [`crate::annotate`] laid out in the reference's key order, and the typed result. `draft` and `doc` are the
/// same document (`doc` parsed from the text, so its vertex order is the file's).
///
/// # Errors
/// As [`crate::annotate`].
pub fn annotate_py(
    draft: &PyJson,
    doc: &PatternDoc,
    names: &Names,
) -> Result<(PyJson, PatternDoc), String> {
    let (data, twins) = annotate_twins(doc, names)?;
    let r = serde_json::to_value(&data).map_err(|e| e.to_string())?;
    let sk = annotate_skeleton(draft, names, &r, &twins);
    Ok((reorder(&r, &sk), data))
}

/// The reference's `annotate` on the skeleton: the draft with dropped pieces gone, topology fixes, renames,
/// cut, mirroring and copied keys per piece, then seams, checks and `meta.annotated`.
fn annotate_skeleton(draft: &PyJson, names: &Names, r: &Value, twins: &[bool]) -> Sk {
    let mut s = Sk::from_py(draft);
    let specs: Vec<(&str, &OValue)> = names.get("pieces").map(OValue::entries).unwrap_or_default();
    let mut drop: Vec<String> = names
        .get("drop")
        .map(OValue::items)
        .unwrap_or_default()
        .iter()
        .map(py_str)
        .collect();
    drop.extend(
        specs
            .iter()
            .filter(|(_, v)| v.get("drop").is_some_and(OValue::truthy))
            .map(|(k, _)| (*k).to_string()),
    );
    let empty = OValue::Null;
    let mut piece_map: Vec<(String, Sk)> = Vec::new();
    if let Some(Sk::List(pieces)) = s.get_mut("pieces") {
        pieces.retain(|p| {
            !p.get("id")
                .and_then(Sk::as_text)
                .is_some_and(|id| drop.contains(&id))
        });
        for (i, p) in pieces.iter_mut().enumerate() {
            let old = p.get("id").and_then(Sk::as_text).unwrap_or_default();
            let sp = specs
                .iter()
                .rev()
                .find(|(k, _)| *k == old)
                .map_or(&empty, |(_, v)| *v);
            let new = piece_skeleton(p, sp, &r["pieces"][i]);
            match piece_map.iter_mut().find(|(o, _)| *o == old) {
                Some(slot) => slot.1 = new,
                None => piece_map.push((old, new)),
            }
        }
    }
    let mut seams = Vec::new();
    let names_seams = names.get("seams").map(OValue::items).unwrap_or_default();
    for (k, (sn, &twin)) in names_seams.iter().zip(twins).enumerate() {
        let mut seam = Sk::Dict(
            sn.entries()
                .into_iter()
                .filter(|(key, _)| !matches!(*key, "a" | "b" | "id" | "band"))
                .map(|(key, v)| (key.to_string(), Sk::from_o(v)))
                .collect(),
        );
        let sid = match sn.get("id") {
            Some(v) if v.truthy() => Sk::from_o(v),
            _ => Sk::text(&format!("s{}", k + 1)),
        };
        seam.set("id", sid.clone());
        seam.set("a", Sk::Computed);
        seam.set("b", Sk::Computed);
        if let Some(band) = sn.get("band") {
            seam.set("ratio_band", Sk::from_o(band));
        }
        let mut left = twin.then(|| seam.clone());
        seams.push(seam);
        if let Some(t) = left.as_mut() {
            t.set("id", Sk::Computed);
            t.set("a", Sk::Computed);
            t.set("b", Sk::Computed);
            t.set("twin_of", sid);
        }
        seams.extend(left);
    }
    let checks: Vec<Sk> = names
        .get("checks")
        .map(OValue::items)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(k, c)| {
            let mut out = Sk::from_o(c);
            let cid = match c.get("id") {
                Some(v) if v.truthy() => Sk::from_o(v),
                _ => Sk::text(&format!("k{}", k + 1)),
            };
            out.set("id", cid);
            out.set("a", Sk::Computed);
            out.set("b", Sk::Computed);
            out
        })
        .collect();
    s.set("seams", Sk::List(seams));
    if !checks.is_empty() {
        s.set("checks", Sk::List(checks));
    }
    if !s.has("meta") {
        s.set("meta", Sk::Dict(Vec::new()));
    }
    let garment = names.get("garment");
    if let Some(meta) = s.get_mut("meta") {
        let annotated = vec![
            (
                "garment".to_string(),
                garment.map_or(Sk::Keep(PyJson::Null), Sk::from_o),
            ),
            ("piece_map".to_string(), Sk::Dict(piece_map)),
            ("notes".to_string(), Sk::Computed),
            ("convention".to_string(), Sk::Computed),
        ];
        meta.set("annotated", Sk::Dict(annotated));
        if let Some(g) = garment.filter(|g| g.truthy()) {
            meta.set("garment", Sk::from_o(g));
        }
    }
    s
}

/// One piece of the reference's `annotate` loop; returns the piece's new id. `rp` is the typed result's
/// piece (it says which re-made edges came out curved).
fn piece_skeleton(p: &mut Sk, sp: &OValue, rp: &Value) -> Sk {
    // edges re-made by _edge_from_pts (merge_at, split): their curve, if any, is new and goes last
    let mut remade = vec![false; p.items_mut("edges").len()];
    for vid in sp.get("merge_at").map(OValue::items).unwrap_or_default() {
        merge_skeleton(p, &py_str(vid), &mut remade);
    }
    for (eid, fr) in sp.get("split").map(OValue::entries).unwrap_or_default() {
        let cuts = match fr {
            OValue::Array(a) => a.len(),
            _ => 1,
        };
        split_skeleton(p, eid, cuts, &mut remade);
    }
    let emap = rename_map(sp.get("edges"));
    let vmap = rename_map(sp.get("vertices"));
    for e in p.items_mut("edges") {
        e.rename("id", &emap);
        e.rename("from", &vmap);
        e.rename("to", &vmap);
    }
    if let Some(Sk::Dict(vs)) = p.get_mut("vertices") {
        for (k, _) in vs.iter_mut() {
            *k = lookup(&vmap, k).to_string();
        }
    }
    for n in p.items_mut("notches") {
        n.rename("edge", &emap);
    }
    for key in ["internal", "trace_marks"] {
        for it in p.items_mut(key) {
            it.rename("near_edge", &emap);
        }
    }
    if let Some(cut) = sp.get("cut") {
        let mut c = Sk::from_o(cut);
        if c.get("on_fold").is_some_and(Sk::truthy) {
            c.rename("on_fold", &emap);
        }
        p.set("cut", c);
    }
    let side = sp
        .get("drawn_side")
        .map_or_else(|| "R".to_string(), py_str)
        .to_uppercase();
    if side == "L" {
        mirror_skeleton(p, &mut remade);
    }
    for key in ["name", "role", "symmetric", "notes", "material"] {
        if let Some(v) = sp.get(key) {
            p.set(key, Sk::from_o(v));
        }
    }
    let new = sp
        .get("id")
        .map_or_else(|| p.get("id").cloned().unwrap_or(Sk::Computed), Sk::from_o);
    p.set("id", new.clone());
    for (k, e) in p.items_mut("edges").iter_mut().enumerate() {
        if remade.get(k) == Some(&true) && rp["edges"][k].get("curve").is_some() {
            let curve = vec![
                ("type".to_string(), Sk::Computed),
                ("c".to_string(), Sk::Computed),
            ];
            e.set("curve", Sk::Dict(curve));
        }
    }
    new
}

/// `merge_at`: the edge into `vid` and the next become the first (its keys, without its curve).
fn merge_skeleton(p: &mut Sk, vid: &str, remade: &mut Vec<bool>) {
    let (ea, eb) = {
        let edges = match p.get_mut("edges") {
            Some(Sk::List(e)) => e,
            _ => return,
        };
        let n = edges.len();
        let Some(i) = edges
            .iter()
            .position(|e| e.get("to").and_then(Sk::as_text).as_deref() == Some(vid))
        else {
            return;
        };
        if n <= 3 {
            return;
        }
        let j = (i + 1) % n;
        let ids = (
            edges[i].get("id").and_then(Sk::as_text),
            edges[j].get("id").and_then(Sk::as_text),
        );
        let mut new = edges[i].clone();
        if let Some(to) = edges[j].get("to").cloned() {
            new.set("to", to);
        }
        new.remove("curve");
        edges[i] = new;
        remade[i] = true;
        edges.remove(j);
        remade.remove(j);
        ids
    };
    for n in p.items_mut("notches") {
        let on = n.get("edge").and_then(Sk::as_text);
        if on.is_some() && on == ea {
            n.set("s", Sk::Computed);
        } else if on.is_some() && on == eb {
            n.set("edge", Sk::text(ea.as_deref().unwrap_or_default()));
            n.set("s", Sk::Computed);
        }
    }
    if let Some(vs) = p.get_mut("vertices") {
        vs.remove(vid);
    }
    if let Some(cut) = p.get_mut("cut") {
        let fold = cut.get("on_fold").and_then(Sk::as_text);
        if fold.is_some() && (fold == ea || fold == eb) {
            cut.set("on_fold", Sk::text(ea.as_deref().unwrap_or_default()));
        }
    }
    if let Some(corners) = p.get_mut("trace").and_then(|t| t.get_mut("corners")) {
        corners.remove(vid);
    }
}

/// `split_edge`: `eid` becomes `eid`, `eid`b, … through new vertices `eid`s1, … (appended).
fn split_skeleton(p: &mut Sk, eid: &str, cuts: usize, remade: &mut Vec<bool>) {
    let new_v: Vec<String> = (1..=cuts).map(|k| format!("{eid}s{k}")).collect();
    {
        let edges = match p.get_mut("edges") {
            Some(Sk::List(e)) => e,
            _ => return,
        };
        let Some(i) = edges
            .iter()
            .position(|e| e.get("id").and_then(Sk::as_text).as_deref() == Some(eid))
        else {
            return;
        };
        let e = edges[i].clone();
        let mut parts = Vec::new();
        for k in 0..=cuts {
            let mut part = e.clone();
            let id = match k {
                0 => eid.to_string(),
                _ => format!("{eid}{}", char::from_u32(97 + k as u32).unwrap_or('?')),
            };
            part.set("id", Sk::text(&id));
            if k > 0 {
                part.set("from", Sk::text(&new_v[k - 1]));
            }
            if k < cuts {
                part.set("to", Sk::text(&new_v[k]));
            }
            part.remove("curve");
            parts.push(part);
        }
        edges.splice(i..=i, parts);
        remade.splice(i..=i, vec![true; cuts + 1]);
    }
    if let Some(Sk::Dict(vs)) = p.get_mut("vertices") {
        vs.extend(new_v.into_iter().map(|v| (v, Sk::Computed)));
    }
    for n in p.items_mut("notches") {
        if n.get("edge").and_then(Sk::as_text).as_deref() == Some(eid) {
            n.set("edge", Sk::Computed);
            n.set("s", Sk::Computed);
        }
    }
}

/// `_mirror_piece`'s `mx` on one point: x is new, and `round(y, 3)` keeps an integer y.
fn mirror_point(q: &Sk) -> Sk {
    match q {
        Sk::List(xy) if xy.len() == 2 => Sk::List(vec![
            Sk::Computed,
            match &xy[1] {
                Sk::Keep(PyJson::Int(y)) => Sk::Keep(PyJson::Int(*y)),
                _ => Sk::Computed,
            },
        ]),
        _ => Sk::Computed,
    }
}

/// `_mirror_piece`: points mirrored, edges reversed (from and to swapped, non-line curves re-made in
/// place), notch fractions, grain and its angle, mark points, and the trace's `to_px`.
fn mirror_skeleton(p: &mut Sk, remade: &mut [bool]) {
    if let Some(Sk::Dict(vs)) = p.get_mut("vertices") {
        for (_, v) in vs.iter_mut() {
            *v = mirror_point(v);
        }
    }
    if let Some(Sk::List(edges)) = p.get_mut("edges") {
        edges.reverse();
        for e in edges.iter_mut() {
            let (from, to) = (e.get("from").cloned(), e.get("to").cloned());
            if let Some(to) = to {
                e.set("from", to);
            }
            if let Some(from) = from {
                e.set("to", from);
            }
            if let Some(c) = e.get_mut("curve").filter(|c| c.truthy()) {
                let kind = c.get("type").and_then(Sk::as_text);
                if kind.as_deref().unwrap_or("line") != "line" {
                    c.set("c", Sk::Computed);
                }
            }
        }
    }
    remade.reverse();
    for n in p.items_mut("notches") {
        n.set("s", Sk::Computed);
    }
    if p.get("grain").is_some_and(Sk::truthy) {
        let points = match p.get("grain") {
            Some(Sk::List(g)) if g.len() == 2 => Sk::List(g.iter().map(mirror_point).collect()),
            Some(g @ Sk::Dict(_)) => match (g.get("p0"), g.get("p1")) {
                (Some(a), Some(b)) => Sk::List(vec![mirror_point(a), mirror_point(b)]),
                _ => Sk::Computed,
            },
            _ => Sk::Computed,
        };
        p.set("grain", points);
        if !p.has("grain_meta") {
            p.set("grain_meta", Sk::Dict(Vec::new()));
        }
        if let Some(gm) = p.get_mut("grain_meta") {
            gm.set("angle_deg", Sk::Computed);
        }
    }
    for key in ["internal", "trace_marks"] {
        for it in p.items_mut(key) {
            if let Some(Sk::List(pts)) = it.get("pts") {
                let pts = Sk::List(pts.iter().map(mirror_point).collect());
                it.set("pts", pts);
            } else if it.has("pts") {
                it.set("pts", Sk::Computed);
            }
        }
    }
    if let Some(tr) = p.get_mut("trace").filter(|t| t.truthy() && t.has("to_px")) {
        tr.set("to_px", Sk::Computed);
        tr.set("mirrored_from_drawing", Sk::Keep(PyJson::Bool(true)));
    }
}

/// [`crate::fit_scale`] laid out in the reference's key order: the scaled pattern, its report (as
/// `meta.scale_fit` holds it) and the typed result. `pattern` and `doc` are the same document.
///
/// # Errors
/// As [`crate::fit_scale`].
pub fn fit_scale_py(
    pattern: &PyJson,
    doc: &PatternDoc,
    targets: &[(String, f64)],
) -> Result<(PyJson, PyJson, PatternDoc), PyError> {
    let (data, report) = fit_scale(doc, targets)?;
    let r = serde_json::to_value(&data).map_err(|e| PyError::Value(e.to_string()))?;
    let mut s = Sk::from_py(pattern);
    if let Some(Sk::List(pieces)) = s.get_mut("pieces") {
        for p in pieces {
            if let Some(Sk::Dict(vs)) = p.get_mut("vertices") {
                for (_, v) in vs.iter_mut() {
                    *v = Sk::Computed;
                }
            }
            if p.get("grain").is_some_and(Sk::truthy) {
                p.set("grain", Sk::Computed);
            }
            for key in ["internal", "trace_marks"] {
                for it in p.items_mut(key) {
                    for k in ["pts", "offset"] {
                        if it.has(k) {
                            it.set(k, Sk::Computed);
                        }
                    }
                }
            }
            if let Some(tr) = p.get_mut("trace").filter(|t| t.truthy() && t.has("to_px")) {
                tr.set("to_px", Sk::Computed);
            }
        }
    }
    let row = || {
        let keys = [
            "ref",
            "target_mm",
            "measured_before",
            "fitted_mm",
            "residual_pct",
        ];
        Sk::Dict(
            keys.iter()
                .map(|k| ((*k).to_string(), Sk::Computed))
                .collect(),
        )
    };
    let rep = Sk::Dict(vec![
        ("k".to_string(), Sk::Computed),
        ("mm_per_px".to_string(), Sk::Computed),
        (
            "targets".to_string(),
            Sk::List((0..targets.len()).map(|_| row()).collect()),
        ),
        ("rms_pct".to_string(), Sk::Computed),
        ("warning".to_string(), Sk::Computed),
    ]);
    if !s.has("meta") {
        s.set("meta", Sk::Dict(Vec::new()));
    }
    if let Some(meta) = s.get_mut("meta") {
        meta.set("px_per_unit", Sk::Computed);
        meta.set("scale_status", Sk::text("FITTED"));
        meta.set("scale_fit", rep.clone());
    }
    Ok((reorder(&r, &s), reorder(&report, &rep), data))
}

/// An `alter.py` result (`result`, the typed op's output) in the reference's key order: a deep copy of
/// `input` with the op's edits in place, then `{"op", "args", "why"}` appended to `meta.history` and
/// `meta.origin` set. `piece` is the op's `--piece` (slash-spread, move, cap-ease) or the target piece
/// (transfer-curve); `item` the moved vertex (move) or the target edge (transfer-curve).
#[must_use]
pub fn alter_py(
    input: &PyJson,
    result: &Value,
    op: &str,
    piece: Option<&str>,
    item: Option<&str>,
) -> PyJson {
    let args: &[&str] = match op {
        "slash-spread" => &["piece", "axis", "at", "add"],
        "move" => &["piece", "vertex", "dx", "dy"],
        "true-seam" => &["seam", "end", "moved_mm"],
        "cap-ease" => &["piece", "seams", "target", "height_scale"],
        _ => &["from", "to"],
    };
    let mut s = Sk::from_py(input);
    if let Some(Sk::List(pieces)) = s.get_mut("pieces")
        && let Some(p) = pieces
            .iter_mut()
            .find(|p| p.get("id").and_then(Sk::as_text).as_deref() == piece)
    {
        alter_marks(p, op, item);
    }
    if !s.has("meta") {
        s.set("meta", Sk::Dict(Vec::new()));
    }
    if let Some(meta) = s.get_mut("meta") {
        if !meta.has("history") {
            meta.set("history", Sk::List(Vec::new()));
        }
        if let Some(Sk::List(history)) = meta.get_mut("history") {
            let args = args.iter().map(|k| ((*k).to_string(), Sk::Computed));
            history.push(Sk::Dict(vec![
                ("op".to_string(), Sk::Computed),
                ("args".to_string(), Sk::Dict(args.collect())),
                ("why".to_string(), Sk::Computed),
            ]));
        }
        meta.set("origin", Sk::text("VECTOR_EDIT"));
    }
    reorder(result, &s)
}

/// What an alteration recomputes in its piece, beyond values that visibly change: slash-spread rewrites
/// every vertex as floats, move its vertex, and `_shift_marks` (slash-spread, cap-ease) the grain and
/// every internal, decoration and trace-mark point; transfer-curve sets a new `poly` curve.
fn alter_marks(p: &mut Sk, op: &str, item: Option<&str>) {
    match (op, item) {
        ("slash-spread", _) => {
            if let Some(Sk::Dict(vs)) = p.get_mut("vertices") {
                for (_, v) in vs.iter_mut() {
                    *v = Sk::Computed;
                }
            }
        }
        ("move", Some(vid)) => {
            if let Some(v) = p.get_mut("vertices").and_then(|vs| vs.get_mut(vid)) {
                *v = Sk::Computed;
            }
        }
        ("transfer-curve", Some(eid)) => {
            if let Some(e) = p
                .items_mut("edges")
                .iter_mut()
                .find(|e| e.get("id").and_then(Sk::as_text).as_deref() == Some(eid))
            {
                let curve = vec![
                    ("type".to_string(), Sk::Computed),
                    ("c".to_string(), Sk::Computed),
                ];
                e.set("curve", Sk::Dict(curve));
            }
        }
        _ => {}
    }
    if op == "slash-spread" || op == "cap-ease" {
        if p.get("grain").is_some_and(Sk::truthy) {
            p.set("grain", Sk::Computed);
        }
        for key in ["internal", "decor", "trace_marks"] {
            for it in p.items_mut(key) {
                if it.has("pts") {
                    it.set("pts", Sk::Computed);
                }
            }
        }
    }
}
