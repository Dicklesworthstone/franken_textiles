//! References into a pattern (spec 04 §10.11): seam-side shorthand ([`resolve_side`], [`twin`]) and the
//! measuring grammar ([`measure_ref`]).

use std::collections::BTreeMap;

use ftx_core::curve::curve_points;
use ftx_core::polyline::polyline_length;
use ftx_core::{Vec2, py};
use ftx_pattern::doc::{PatternDoc, Piece};
use ftx_pattern::{InstEdge, Instance, Pattern, PyError, Side};

/// Old (traced) piece id → new id.
pub type PieceMap = BTreeMap<String, String>;
/// New piece id → (old edge id → new edge id).
pub type EdgeMaps = BTreeMap<String, BTreeMap<String, String>>;

/// `id.R` / `id.L` → (`id`, `.R`/`.L`); anything else → (itself, `""`).
pub(crate) fn split_suffix(s: &str) -> (&str, &str) {
    if s.ends_with(".R") || s.ends_with(".L") {
        s.split_at(s.len() - 2)
    } else {
        (s, "")
    }
}

/// The piece an instance or piece id names (the last of that id, as a Python dict keeps it).
pub(crate) fn piece_by_stem<'a>(
    doc: &'a PatternDoc,
    inst: &'a str,
) -> Result<(&'a Piece, &'a str), PyError> {
    let (stem, suffix) = split_suffix(inst);
    match doc.pieces.iter().rev().find(|p| p.id == stem) {
        Some(p) => Ok((p, suffix)),
        None => {
            let mut have: Vec<&str> = doc.pieces.iter().map(|p| p.id.as_str()).collect();
            have.sort_unstable();
            have.dedup();
            Err(PyError::Key(format!(
                "unknown piece {} (have {})",
                py::repr_str(stem),
                py::repr_str_list(&have)
            )))
        }
    }
}

/// Python `int(cut.get("qty", 1))`.
pub(crate) fn qty(piece: &Piece) -> i64 {
    piece
        .cut
        .as_ref()
        .and_then(|c| c.qty.as_ref())
        .and_then(serde_json::Value::as_f64)
        .map_or(1, |q| q.trunc() as i64) // det-ok: an integer count
}

fn sorted_ids(piece: &Piece) -> Vec<&str> {
    let mut ids: Vec<&str> = piece.edges.iter().map(|e| e.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// A seam side `piece/eA[+eB]` or `inst/…` as an instance path, and whether shorthand was expanded: a bare
/// cut-2 piece means its `.R` instance, a bare edge of an on-fold piece its drafted `.R` half. Old (traced)
/// ids translate through the rename maps.
///
/// # Errors
/// The reference's `ValueError`s (no `/`, no such edge, the fold line) and `KeyError` (no such piece).
pub fn resolve_side(
    doc: &PatternDoc,
    r: &str,
    piece_map: &PieceMap,
    edge_maps: &EdgeMaps,
) -> Result<(String, bool), PyError> {
    let Some((inst, path)) = r.split_once('/') else {
        return Err(PyError::Value(format!(
            "side ref {} must look like piece/edge[+edge]",
            py::repr_str(r)
        )));
    };
    let (stem0, suffix0) = split_suffix(inst);
    let stem = piece_map.get(stem0).map_or(stem0, String::as_str);
    let (piece, _) = piece_by_stem(doc, stem)?;
    let empty = BTreeMap::new();
    let emap = edge_maps.get(stem).unwrap_or(&empty);
    let fold = piece.on_fold();
    let mut suffix = suffix0.to_string();
    let mut expanded = false;
    if qty(piece) == 2 && suffix.is_empty() {
        suffix = ".R".into();
        expanded = true;
    }
    let mut out = Vec::new();
    for e in path.split('+') {
        let (e0, esuf0) = split_suffix(e);
        let e = emap.get(e0).map_or(e0, String::as_str);
        if !piece.edges.iter().any(|x| x.id == e) {
            return Err(PyError::Value(format!(
                "side ref {}: piece {} has no edge {} (have {})",
                py::repr_str(r),
                py::repr_str(stem),
                py::repr_str(e),
                py::repr_str_list(&sorted_ids(piece))
            )));
        }
        let mut esuf = esuf0.to_string();
        if let Some(fold) = fold {
            if e == fold {
                return Err(PyError::Value(format!(
                    "side ref {}: {} is the fold line, which is not sewn",
                    py::repr_str(r),
                    py::repr_str(e)
                )));
            }
            if esuf.is_empty() {
                esuf = ".R".into();
                expanded = true;
            }
        }
        out.push(format!("{e}{esuf}"));
    }
    Ok((format!("{stem}{suffix}/{}", out.join("+")), expanded))
}

/// The left-side twin of a resolved side: a cut-2 `.R` side becomes `.L` with the same path; an on-fold side
/// swaps its `.R` edges to `.L`; a piece both cut 2 and on the fold has none.
///
/// # Errors
/// `KeyError` for an unknown piece.
pub fn twin(doc: &PatternDoc, r: &str) -> Result<Option<String>, PyError> {
    let (inst, path) = r.split_once('/').unwrap_or((r, ""));
    let (piece, suffix) = piece_by_stem(doc, inst)?;
    let (qty2, fold) = (qty(piece) == 2, piece.on_fold().is_some());
    if qty2 && fold {
        return Ok(None);
    }
    if qty2 && suffix == ".R" {
        return Ok(Some(format!("{}.L/{path}", &inst[..inst.len() - 2])));
    }
    if fold {
        let edges: Vec<String> = path
            .split('+')
            .map(|e| match e.strip_suffix(".R") {
                Some(b) => format!("{b}.L"),
                None => e.to_string(),
            })
            .collect();
        return Ok(Some(format!("{inst}/{}", edges.join("+"))));
    }
    Ok(None)
}

/// The piece as one instance in its drawn frame (the reference's `_piece_instance`).
#[must_use]
pub fn piece_instance(piece: &Piece) -> Instance {
    Instance {
        id: piece.id.clone(),
        piece: piece.id.clone(),
        piece_index: 0,
        side: Side::C,
        mirrored: false,
        verts: piece.vertices.clone(),
        edges: piece
            .edges
            .iter()
            .enumerate()
            .map(|(i, e)| InstEdge {
                id: e.id.clone(),
                from: e.from.clone(),
                to: e.to.clone(),
                curve: e.curve.clone(),
                source: i,
            })
            .collect(),
    }
}

/// A vertex of a piece (`KeyError` when missing).
pub(crate) fn vertex(piece: &Piece, vid: &str) -> Result<Vec2, PyError> {
    piece
        .vertices
        .get(vid)
        .ok_or_else(|| PyError::Key(vid.to_string()))
}

/// The polyline of a piece edge at `dense` samples (the reference's `_piece_edge_pts`).
pub(crate) fn edge_points(piece: &Piece, eid: &str, dense: usize) -> Result<Vec<Vec2>, PyError> {
    let e = piece
        .edges
        .iter()
        .find(|x| x.id == eid)
        .ok_or_else(|| PyError::Value("StopIteration".into()))?;
    curve_points(
        vertex(piece, &e.from)?,
        vertex(piece, &e.to)?,
        e.curve.as_ref(),
        dense,
    )
    .map_err(|err| PyError::Value(err.to_string()))
}

/// The closed outline at `dense` samples per curved edge (open loop).
pub(crate) fn outline(piece: &Piece, dense: usize) -> Result<Vec<Vec2>, PyError> {
    piece_instance(piece).outline(dense)
}

/// `np.ptp(outline, axis=0).max()`.
pub(crate) fn outline_size(piece: &Piece) -> Result<f64, PyError> {
    let o = outline(piece, 60)?;
    let (lo, hi) = bbox(&o);
    Ok((hi.x - lo.x).max(hi.y - lo.y))
}

pub(crate) fn bbox(o: &[Vec2]) -> (Vec2, Vec2) {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in o {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    (lo, hi)
}

/// Splits `cA-cB` at the `-` where both halves are vertex ids.
fn split_pair<'a>(piece: &Piece, s: &'a str) -> Result<(&'a str, &'a str), PyError> {
    for (i, ch) in s.char_indices() {
        if ch == '-' && piece.vertices.contains(&s[..i]) && piece.vertices.contains(&s[i + 1..]) {
            return Ok((&s[..i], &s[i + 1..]));
        }
    }
    let mut have: Vec<&str> = piece.vertices.iter().map(|(k, _)| k).collect();
    have.sort_unstable();
    Err(PyError::Value(format!(
        "{}: cannot split {} into two vertex ids (have {})",
        piece.id,
        py::repr_str(s),
        py::repr_str_list(&have)
    )))
}

/// The length (pattern units) of a reference: `INST/eA[+eB]` a seam side as sewn, `PIECE/eA[+eB]` edges as
/// drawn (fold edges allowed), `PIECE:cA-cB[:x|:y]` a vertex distance or its horizontal / vertical span,
/// `PIECE:width` / `PIECE:height` the outline's bounding box, `seam:ID[:a|:b]` a seam side.
///
/// # Errors
/// The reference's `KeyError` / `ValueError` texts.
pub fn measure_ref(pattern: &Pattern, r: &str) -> Result<f64, PyError> {
    let doc = pattern
        .doc
        .as_ref()
        .ok_or_else(|| PyError::Value("the pattern is not a fabric-pattern/1 document".into()))?;
    if let Some(rest) = r.strip_prefix("seam:") {
        let parts: Vec<&str> = std::iter::once("seam").chain(rest.split(':')).collect();
        let seam = pattern
            .seams()
            .iter()
            .find(|s| s.0.get("id").and_then(serde_json::Value::as_str) == Some(parts[1]))
            .ok_or_else(|| PyError::Key(format!("no seam {}", py::repr_str(parts[1]))))?;
        let key = parts.get(2).copied().unwrap_or("a");
        let side = seam
            .0
            .get(key)
            .ok_or_else(|| PyError::Key(key.to_string()))?;
        let side = side
            .as_str()
            .ok_or_else(|| PyError::Value(format!("cannot parse reference {side}")))?;
        return measure_ref(pattern, side);
    }
    if let Some((inst_id, path)) = r.split_once('/') {
        let is_piece = doc.pieces.iter().any(|p| p.id == inst_id);
        if pattern.instance(inst_id).is_some() && !is_piece {
            return pattern.side_length(r);
        }
        let (piece, _) = piece_by_stem(doc, inst_id)?;
        let eids: Vec<&str> = path.split('+').map(|e| split_suffix(e).0).collect();
        let missing: Vec<&str> = eids
            .iter()
            .copied()
            .filter(|e| !piece.edges.iter().any(|x| x.id == *e))
            .collect();
        if !missing.is_empty() {
            return Err(PyError::Value(format!(
                "ref {}: piece {} has no edge(s) {} (have {})",
                py::repr_str(r),
                py::repr_str(&piece.id),
                py::repr_str_list(&missing),
                py::repr_str_list(&sorted_ids(piece))
            )));
        }
        let mut total = 0.0;
        for e in eids {
            total += polyline_length(&edge_points(piece, e, 200)?);
        }
        return Ok(total);
    }
    if let Some((stem, rest)) = r.split_once(':') {
        let (piece, _) = piece_by_stem(doc, stem)?;
        if rest == "width" || rest == "height" {
            let (lo, hi) = bbox(&outline(piece, 60)?);
            return Ok(if rest == "width" {
                hi.x - lo.x
            } else {
                hi.y - lo.y
            });
        }
        let (rest, axis) = match rest.strip_suffix(":x") {
            Some(r) => (r, Some('x')),
            None => match rest.strip_suffix(":y") {
                Some(r) => (r, Some('y')),
                None => (rest, None),
            },
        };
        let (a, b) = split_pair(piece, rest)?;
        let d = vertex(piece, b)? - vertex(piece, a)?;
        return Ok(match axis {
            Some('x') => d.x.abs(),
            Some(_) => d.y.abs(),
            None => d.norm(),
        });
    }
    Err(PyError::Value(format!(
        "cannot parse reference {}",
        py::repr_str(r)
    )))
}
