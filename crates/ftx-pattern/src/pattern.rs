//! Pattern construction, structure checks, seam sides, notch fractions and seam sampling
//! (`docs/spec/01` §7–8).
//!
//! The definition checks run on the raw JSON, as the reference runs them on its dictionaries, so a
//! malformed document gets the reference's messages. Deviations, all on inputs the reference crashes on
//! or silently accepts (spec 01 §15): a piece without an id is `piece {i}: missing id`; `cut: null` reads
//! as absent; `cut.qty` must be the integer 1 or 2 (the reference truncates 2.7 to 2); a Bézier with the
//! wrong number of control points is a definition error; a seam ref without `/` gets its own message; a
//! zero-length seam side with notches reports the implausible length instead of dividing by zero.

use std::collections::BTreeMap;

use ftx_core::curve::EDGE_DENSE;
use ftx_core::polyline::{at_fractions, polyline_length};
use ftx_core::{Vec2, np, py};
use serde::Serialize;
use serde_json::Value;

use crate::doc::{PatternDoc, SCHEMA, Seam, truthy};
use crate::instance::{Instance, PyError, physical_side, piece_instances};

/// Python `repr` of a JSON scalar as the reference would print the decoded value.
#[must_use]
pub fn py_repr_value(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::String(s) => py::repr_str(s),
        Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => i.to_string(),
            (_, Some(u), _) => u.to_string(),
            (_, _, Some(f)) => py::repr_f64(f),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

fn key_err(k: &str) -> String {
    py::repr_str(k)
}

fn str_field<'a>(o: &'a Value, k: &str) -> Result<&'a str, String> {
    match o.get(k) {
        None => Err(key_err(k)),
        Some(Value::String(s)) => Ok(s),
        Some(other) => Err(format!("{k} must be a string, not {other}")),
    }
}

/// `cut.qty` as the instance count, if it is the integer 1 or 2 (absent → 1).
fn qty_of(piece: &Value) -> Option<u8> {
    let cut = piece.get("cut").filter(|c| !c.is_null());
    match cut.and_then(|c| c.get("qty")) {
        None => Some(1),
        Some(q) => match q.as_f64() {
            Some(1.0) => Some(1),
            Some(2.0) => Some(2),
            _ => None,
        },
    }
}

const CURVE_TYPES: [&str; 4] = ["line", "quad", "cubic", "poly"];

/// The piece-definition checks (spec 01 §7.2) on the raw piece object. `Err` carries the exception text
/// the reference reports as `{pid}: cannot validate piece definition: {exc}`.
///
/// # Errors
/// A required key (`id`, `vertices`, `edges`, an edge's `id`/`from`/`to`) is missing or of the wrong type.
pub fn piece_definition_errors(p: &Value) -> Result<Vec<String>, String> {
    let mut errs = Vec::new();
    let pid = str_field(p, "id")?;
    let verts = p
        .get("vertices")
        .ok_or_else(|| key_err("vertices"))?
        .as_object()
        .ok_or("vertices must be an object")?;
    let lp = p
        .get("edges")
        .ok_or_else(|| key_err("edges"))?
        .as_array()
        .ok_or("edges must be a list")?;
    let mut edge_ids = Vec::with_capacity(lp.len());
    for e in lp {
        edge_ids.push(str_field(e, "id")?);
    }
    // Check 1 (the reference walks the vertex ids in set order: compare these messages as a multiset).
    for name in verts
        .keys()
        .map(String::as_str)
        .chain(edge_ids.iter().copied())
        .chain([pid])
    {
        if name.contains(['.', '/', '+', ' ']) {
            errs.push(format!(
                "{pid}: id {} must not contain '.', '/', '+' or spaces",
                py::repr_str(name)
            ));
        }
    }
    // Checks 2a, 2b and (port) 2c, per edge.
    for (e, eid) in lp.iter().zip(&edge_ids) {
        for k in ["from", "to"] {
            let v = str_field(e, k)?;
            if !verts.contains_key(v) {
                errs.push(format!("{pid}/{eid}: unknown vertex {}", py::repr_str(v)));
            }
        }
        let curve = e.get("curve").filter(|c| truthy(c));
        let ctype = match curve {
            None => Value::String("line".into()),
            Some(Value::Object(c)) => c
                .get("type")
                .cloned()
                .unwrap_or_else(|| Value::String("line".into())),
            Some(other) => {
                return Err(format!("{pid}/{eid}: curve must be an object, not {other}"));
            }
        };
        match ctype.as_str().filter(|t| CURVE_TYPES.contains(t)) {
            None => errs.push(format!("{pid}/{eid}: curve type {}", py_repr_value(&ctype))),
            Some(kind @ ("quad" | "cubic")) => {
                let need = if kind == "quad" { 1 } else { 2 };
                let have = curve
                    .and_then(|c| c.get("c"))
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                if have != need {
                    errs.push(format!(
                        "{pid}/{eid}: {kind} curve needs {need} control points"
                    ));
                }
            }
            Some(_) => {}
        }
    }
    // Check 3.
    for (i, e) in lp.iter().enumerate() {
        let nxt = &lp[(i + 1) % lp.len()];
        let (to, nfrom) = (str_field(e, "to")?, str_field(nxt, "from")?);
        if to != nfrom {
            errs.push(format!(
                "{pid}: edge {} ends at {to} but {} starts at {nfrom}",
                edge_ids[i],
                edge_ids[(i + 1) % lp.len()]
            ));
        }
    }
    // Check 4.
    let mut unique = edge_ids.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != edge_ids.len() {
        errs.push(format!("{pid}: duplicate edge ids"));
    }
    // Checks 5–7.
    let cut = match p.get("cut") {
        None | Some(Value::Null) => None,
        Some(Value::Object(c)) => Some(c),
        Some(other) => return Err(format!("cut must be an object, not {other}")),
    };
    if qty_of(p).is_none() {
        errs.push(format!(
            "{pid}: cut.qty must be 1 or 2 (use separate pieces for more)"
        ));
    }
    let fold = cut.and_then(|c| c.get("on_fold")).filter(|f| truthy(f));
    if let Some(fold) = fold {
        let found = fold
            .as_str()
            .and_then(|f| lp.iter().zip(&edge_ids).find(|(_, id)| **id == f));
        match found {
            None => errs.push(format!(
                "{pid}: on_fold edge {} missing",
                py_repr_value(fold)
            )),
            Some((f, _)) => {
                let straight = match f.get("curve").filter(|c| truthy(c)) {
                    None => true,
                    Some(c) => c.get("type").is_none_or(|t| t == "line"),
                };
                let fname = py_repr_value(fold);
                if !straight {
                    errs.push(format!("{pid}: fold edge {fname} must be straight"));
                }
                let (a, b) = (str_field(f, "from")?, str_field(f, "to")?);
                if verts.contains_key(a) && verts.contains_key(b) {
                    let pt = |v: &str| -> Option<Vec2> {
                        let arr = verts.get(v)?.as_array()?;
                        Some(Vec2::new(arr.first()?.as_f64()?, arr.get(1)?.as_f64()?))
                            .filter(|q| q.is_finite())
                    };
                    let ok =
                        matches!((pt(a), pt(b)), (Some(pa), Some(pb)) if (pb - pa).norm() > 0.0);
                    if !ok {
                        errs.push(format!(
                            "{pid}: fold edge {fname} must have finite, distinct endpoints"
                        ));
                    }
                }
            }
        }
    }
    if let Err(error) = physical_side(p) {
        errs.push(error.to_string());
    }
    Ok(errs)
}

/// Matched sample points on both sides of a seam (spec 01 §8.4).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SeamPairs {
    /// Points on side a.
    pub a: Vec<Vec2>,
    /// Matching points on side b (in a's direction).
    pub b: Vec<Vec2>,
    /// Arclength fractions on a.
    pub fa: Vec<f64>,
    /// Arclength fractions on b.
    pub fb: Vec<f64>,
    /// Length of side a (mm).
    pub len_a: f64,
    /// Length of side b (mm).
    pub len_b: f64,
    /// Internal edge junctions of a (fractions).
    pub a_junctions: Vec<f64>,
    /// Internal edge junctions of b, in a's direction.
    pub b_junctions: Vec<f64>,
    /// Notch fractions on a.
    pub notches_a: Vec<f64>,
    /// Notch fractions on b, in a's direction.
    pub notches_b: Vec<f64>,
    /// `piecewise`, `none` or `count-mismatch {na}:{nb}`.
    pub notch_match: String,
}

/// A pattern: the document, its instances and the problems found while building them.
#[derive(Clone, Debug)]
pub struct Pattern {
    raw: Value,
    /// The typed document (`None` when the raw document could not be typed).
    pub doc: Option<PatternDoc>,
    /// Instances in piece order (`[R, L]` or `[base]` within a piece).
    pub instances: Vec<Instance>,
    /// Problems found while building; when there are any, no instances are built.
    pub build_errors: Vec<String>,
}

impl Pattern {
    /// Builds a pattern from a document (spec 01 §7.1). Never fails: problems land in `build_errors`
    /// and in [`Pattern::structure_errors`].
    #[must_use]
    pub fn new(data: Value) -> Self {
        let doc = serde_json::from_value::<PatternDoc>(data.clone()).map_err(|e| e.to_string());
        Self::build(data, doc)
    }

    fn build(data: Value, doc: Result<PatternDoc, String>) -> Self {
        let mut build_errors = Vec::new();
        let mut piece_ids: Vec<String> = Vec::new();
        let mut instance_ids: Vec<String> = Vec::new();
        match data.get("pieces").and_then(Value::as_array) {
            None => build_errors.push("pattern: pieces must be a list".to_string()),
            Some(pieces) => {
                for (i, p) in pieces.iter().enumerate() {
                    let pid = match p.get("id") {
                        Some(Value::String(s)) => s.clone(),
                        Some(other) => {
                            build_errors
                                .push(format!("piece {i}: id must be a string, not {other}"));
                            continue;
                        }
                        None => {
                            build_errors.push(format!("piece {i}: missing id"));
                            continue;
                        }
                    };
                    if piece_ids.contains(&pid) {
                        build_errors.push(format!("duplicate piece id {}", py::repr_str(&pid)));
                    } else {
                        piece_ids.push(pid.clone());
                    }
                    match piece_definition_errors(p) {
                        Ok(errs) => {
                            build_errors.extend(errs);
                            let ids = if qty_of(p) == Some(2) {
                                vec![format!("{pid}.R"), format!("{pid}.L")]
                            } else {
                                vec![pid.clone()]
                            };
                            for iid in ids {
                                if instance_ids.contains(&iid) {
                                    build_errors.push(format!(
                                        "duplicate instance id {}",
                                        py::repr_str(&iid)
                                    ));
                                }
                                instance_ids.push(iid);
                            }
                        }
                        Err(exc) => build_errors
                            .push(format!("{pid}: cannot validate piece definition: {exc}")),
                    }
                }
            }
        }
        let doc = match doc {
            Ok(d) => Some(d),
            Err(e) => {
                if build_errors.is_empty() {
                    build_errors.push(format!("pattern: {e}"));
                }
                None
            }
        };
        let mut instances: Vec<Instance> = Vec::new();
        if let (true, Some(d)) = (build_errors.is_empty(), &doc) {
            for (i, piece) in d.pieces.iter().enumerate() {
                match piece_instances(piece, i) {
                    Ok(list) => {
                        for inst in list {
                            if instances.iter().any(|x| x.id == inst.id) {
                                build_errors.push(format!(
                                    "duplicate instance id {}",
                                    py::repr_str(&inst.id)
                                ));
                            } else {
                                instances.push(inst);
                            }
                        }
                    }
                    Err(e) => {
                        build_errors.push(format!("{}: cannot build instances: {e}", piece.id))
                    }
                }
            }
        }
        Self {
            raw: data,
            doc,
            instances,
            build_errors,
        }
    }

    /// Builds a pattern from JSON text. Unlike [`Pattern::new`] on a `serde_json::Value` (whose maps are
    /// sorted), the typed document keeps the vertex order of the text.
    ///
    /// # Errors
    /// The text is not JSON.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let raw: Value =
            serde_json::from_str(text).map_err(|e| format!("pattern is not JSON: {e}"))?;
        let doc = serde_json::from_str::<PatternDoc>(text).map_err(|e| e.to_string());
        Ok(Self::build(raw, doc))
    }

    /// Builds a pattern from a typed document, keeping its vertex order (as [`Pattern::from_json`]).
    #[must_use]
    pub fn from_doc(doc: PatternDoc) -> Self {
        let raw = serde_json::to_value(&doc).unwrap_or(Value::Null);
        Self::build(raw, Ok(doc))
    }

    /// The document as given.
    #[must_use]
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    /// The instance with this id.
    #[must_use]
    pub fn instance(&self, id: &str) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    /// The seams of the typed document (empty when there is none).
    #[must_use]
    pub fn seams(&self) -> &[Seam] {
        self.doc.as_ref().map_or(&[], PatternDoc::seams)
    }

    /// Whole-pattern structure checks (spec 01 §7.3), with the reference's messages and order.
    #[must_use]
    pub fn structure_errors(&self) -> Vec<String> {
        let mut errs = Vec::new();
        if self.raw.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
            errs.push(format!("schema must be {}", py::repr_str(SCHEMA)));
        }
        if self
            .raw
            .get("units")
            .is_some_and(|u| u.as_str() != Some("mm"))
        {
            errs.push("units must be mm".to_string());
        }
        for p in self
            .raw
            .get("pieces")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
        {
            let Some(pid) = p.get("id").and_then(Value::as_str) else {
                continue; // reported by the build errors
            };
            match piece_definition_errors(p) {
                Ok(e) => errs.extend(e),
                Err(exc) => errs.push(format!("{pid}: cannot validate piece definition: {exc}")),
            }
        }
        let empty = Vec::new();
        let seams = self
            .raw
            .get("seams")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        let ids: Vec<Value> = seams
            .iter()
            .map(|s| s.get("id").cloned().unwrap_or(Value::Null))
            .collect();
        let mut dups: Vec<&Value> = ids
            .iter()
            .filter(|v| ids.iter().filter(|w| w == v).count() > 1)
            .collect();
        let rank = |v: &Value| match v {
            Value::Null => 0,
            Value::Number(_) => 1,
            Value::String(_) => 2,
            _ => 3,
        };
        dups.sort_by(|a, b| {
            rank(a).cmp(&rank(b)).then_with(|| match (a, b) {
                (Value::Number(x), Value::Number(y)) => x
                    .as_f64()
                    .unwrap_or(0.0)
                    .total_cmp(&y.as_f64().unwrap_or(0.0)),
                _ => a.to_string().cmp(&b.to_string()),
            })
        });
        dups.dedup();
        errs.extend(
            dups.iter()
                .map(|v| format!("duplicate seam id {}", py_repr_value(v))),
        );
        for e in &self.build_errors {
            if !errs.contains(e) {
                errs.push(e.clone());
            }
        }
        if !errs.is_empty() {
            return errs;
        }
        for inst in &self.instances {
            match inst.area() {
                Ok(a) if a > 0.0 => {}
                Ok(a) => errs.push(format!(
                    "{}: edge loop is clockwise or degenerate (area {} mm²); loops must be CCW",
                    inst.id,
                    py::fixed(a, 0)
                )),
                Err(e) => errs.push(format!("{}: {e}", inst.id)),
            }
        }
        let mut used: BTreeMap<(String, String), String> = BTreeMap::new();
        for s in self.seams() {
            let sid = s.id_text();
            for k in ["a", "b"] {
                let side = match s.0.get(k) {
                    None => Err(PyError::Key(k.to_string())),
                    Some(Value::String(r)) => self.side(r),
                    Some(other) => Err(PyError::Value(format!(
                        "side {k} must be a seam ref string, not {other}"
                    ))),
                };
                match side {
                    Err(e) => errs.push(format!("seam {sid}: {e}")),
                    Ok((inst, eids)) => {
                        for e in eids {
                            let key = (inst.id.clone(), e.clone());
                            if let Some(other) = used.get(&key) {
                                errs.push(format!(
                                    "seam {sid}: edge {}/{e} already used by seam {other}",
                                    inst.id
                                ));
                            }
                            used.insert(key, sid.clone());
                        }
                    }
                }
            }
        }
        errs
    }

    /// Resolves a seam ref `INSTANCE/EDGE[+EDGE...]` (spec 01 §8.1).
    ///
    /// # Errors
    /// No `/`; an unknown instance or edge (key errors); edges not consecutive in loop order.
    pub fn side(&self, r: &str) -> Result<(&Instance, Vec<String>), PyError> {
        let Some((inst_id, path)) = r.split_once('/') else {
            return Err(PyError::Value(format!(
                "seam ref {}: no '/' between the instance and its edges",
                py::repr_str(r)
            )));
        };
        let Some(inst) = self.instance(inst_id) else {
            let mut ids: Vec<&str> = self.instances.iter().map(|i| i.id.as_str()).collect();
            ids.sort_unstable();
            return Err(PyError::Key(format!(
                "seam ref {}: unknown instance {} (have {})",
                py::repr_str(r),
                py::repr_str(inst_id),
                py::repr_str_list(&ids)
            )));
        };
        let eids: Vec<String> = path.split('+').map(str::to_string).collect();
        for e in &eids {
            inst.edge(e)?;
        }
        for w in eids.windows(2) {
            if inst.edge(&w[0])?.to != inst.edge(&w[1])?.from {
                return Err(PyError::Value(format!(
                    "seam ref {}: edges {} and {} are not consecutive in loop order",
                    py::repr_str(r),
                    w[0],
                    w[1]
                )));
            }
        }
        Ok((inst, eids))
    }

    /// A seam side's polyline (200 samples per curved edge, shared junction points once) and the
    /// arclength fractions of its internal edge junctions (spec 01 §8.2).
    ///
    /// # Errors
    /// As [`Pattern::side`].
    pub fn side_polyline(&self, r: &str) -> Result<(Vec<Vec2>, Vec<f64>), PyError> {
        let (inst, eids) = self.side(r)?;
        let mut pts = Vec::new();
        let mut lens = Vec::new();
        for (k, e) in eids.iter().enumerate() {
            let q = inst.edge_dense(e, EDGE_DENSE)?;
            lens.push(polyline_length(&q));
            pts.extend_from_slice(if k == 0 { &q[..] } else { &q[1..] });
        }
        let total = lens.iter().fold(0.0, |a, l| a + l);
        let junctions = if total == 0.0 {
            Vec::new()
        } else {
            let cs = np::cumsum(&lens);
            cs[..cs.len() - 1].iter().map(|c| c / total).collect()
        };
        Ok((pts, junctions))
    }

    /// Length of a seam side.
    ///
    /// # Errors
    /// As [`Pattern::side`].
    pub fn side_length(&self, r: &str) -> Result<f64, PyError> {
        Ok(polyline_length(&self.side_polyline(r)?.0))
    }

    /// Arclength fractions of the notches on a seam side, in the side's direction, end notches dropped
    /// (spec 01 §8.3).
    ///
    /// # Errors
    /// As [`Pattern::side`]; a zero-length side carrying a notch.
    pub fn notch_fractions(&self, r: &str) -> Result<Vec<f64>, PyError> {
        let (inst, eids) = self.side(r)?;
        let piece = self
            .doc
            .as_ref()
            .and_then(|d| d.pieces.get(inst.piece_index));
        let notches = piece.map_or(&[][..], |p| p.notches());
        let lens: Vec<f64> = eids
            .iter()
            .map(|e| inst.edge_length(e))
            .collect::<Result<_, _>>()?;
        let total = lens.iter().fold(0.0, |a, l| a + l);
        let mut out = Vec::new();
        for n in notches {
            for (k, e) in eids.iter().enumerate() {
                let stem = e
                    .strip_suffix(".R")
                    .or_else(|| e.strip_suffix(".L"))
                    .unwrap_or(e);
                if stem != n.edge {
                    continue;
                }
                let s = if e.ends_with(".L") != inst.mirrored {
                    1.0 - n.s
                } else {
                    n.s
                };
                if total == 0.0 {
                    return Err(PyError::Value(format!(
                        "seam side {} has zero length",
                        py::repr_str(r)
                    )));
                }
                let before = lens[..k].iter().fold(0.0, |a, l| a + l);
                let f = (before + s * lens[k]) / total;
                if 1e-4 < f && f < 1.0 - 1e-4 {
                    out.push(f);
                }
            }
        }
        out.sort_by(f64::total_cmp);
        Ok(out)
    }

    /// Matched sample points on both sides of a seam, piecewise-proportional between matched notches
    /// (spec 01 §8.4). The seam walk uses `spacing = 5.0`.
    ///
    /// # Errors
    /// A side that does not resolve; implausible side lengths (not in (0, 1e5) mm).
    pub fn seam_pairs(&self, seam: &Seam, spacing: f64) -> Result<SeamPairs, PyError> {
        let side_ref = |k: &str| seam.side_ref(k).ok_or_else(|| PyError::Key(k.to_string()));
        let (ra, rb) = (side_ref("a")?, side_ref("b")?);
        let (a_pts, a_j) = self.side_polyline(ra)?;
        let (mut b_pts, mut b_j) = self.side_polyline(rb)?;
        let reverse = seam.reverse();
        if reverse {
            b_pts.reverse();
            b_j = b_j.iter().rev().map(|j| 1.0 - j).collect();
        }
        let (la, lb) = (polyline_length(&a_pts), polyline_length(&b_pts));
        if !(0.0 < la && la < 1e5 && 0.0 < lb && lb < 1e5) {
            return Err(PyError::Value(format!(
                "seam {}: implausible side lengths {} / {} mm",
                seam.id_text(),
                py::fixed(la, 0),
                py::fixed(lb, 0)
            )));
        }
        let na = self.notch_fractions(ra)?;
        let mut nb = self.notch_fractions(rb)?;
        if reverse {
            nb = nb.iter().map(|f| 1.0 - f).collect();
            nb.sort_by(f64::total_cmp);
        }
        let (ka, kb, notch_match) = if !na.is_empty() && na.len() == nb.len() {
            let wrap = |v: &[f64]| [&[0.0][..], v, &[1.0][..]].concat();
            (wrap(&na), wrap(&nb), "piecewise".to_string())
        } else if na.is_empty() && nb.is_empty() {
            (vec![0.0, 1.0], vec![0.0, 1.0], "none".to_string())
        } else {
            (
                vec![0.0, 1.0],
                vec![0.0, 1.0],
                format!("count-mismatch {}:{}", na.len(), nb.len()),
            )
        };
        let to_b = |f: f64| np::interp(f, &ka, &kb);
        let to_a = |f: f64| np::interp(f, &kb, &ka);
        let mut brk: Vec<f64> = ka
            .iter()
            .chain(&a_j)
            .copied()
            .chain(b_j.iter().map(|&f| to_a(f)))
            .map(|f| py::round(f, 9))
            .collect();
        brk.sort_by(|x, y| x.partial_cmp(y).unwrap_or_else(|| x.total_cmp(y)));
        brk.dedup_by(|x, y| x == y);
        let (mut fa, mut fb) = (vec![0.0], vec![0.0]);
        for w in brk.windows(2) {
            let (u0, u1) = (w[0], w[1]);
            let span = ((u1 - u0) * la).max((to_b(u1) - to_b(u0)) * lb);
            let n = ((span / spacing).ceil() as usize).max(1);
            let us = np::linspace(u0, u1, n + 1);
            for &u in &us[1..] {
                fa.push(u);
                fb.push(to_b(u));
            }
        }
        Ok(SeamPairs {
            a: at_fractions(&a_pts, &fa),
            b: at_fractions(&b_pts, &fb),
            fa,
            fb,
            len_a: la,
            len_b: lb,
            a_junctions: a_j,
            b_junctions: b_j,
            notches_a: na,
            notches_b: nb,
            notch_match,
        })
    }

    /// The typed document as JSON (semantically equal to the input when it typed cleanly).
    #[must_use]
    pub fn to_value(&self) -> Option<Value> {
        self.doc.as_ref().and_then(|d| serde_json::to_value(d).ok())
    }
}
