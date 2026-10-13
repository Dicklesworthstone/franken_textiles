//! Pattern alterations (`docs/spec/01` §10), in millimetres and drafted-piece coordinates.
//!
//! Operations return a new raw document so unknown fields, source provenance and unrelated geometry
//! survive unchanged. Relative curves follow their endpoints. Marks follow a slash, but drills,
//! notches and placement anchors deliberately do not (reference behaviour, not a fit claim).

use ftx_core::curve::{TRANSFER_DENSE, abs_to_rel};
use ftx_core::{Refusal, RefusalCode, Vec2, py};
use serde_json::{Value, json};

use crate::Pattern;

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn built(doc: &Value) -> Result<Pattern, Refusal> {
    let pattern = Pattern::new(doc.clone());
    if pattern.build_errors.is_empty() {
        Ok(pattern)
    } else {
        Err(invalid(pattern.build_errors.join("; ")))
    }
}

fn piece_mut<'a>(doc: &'a mut Value, pid: &str) -> Result<&'a mut Value, Refusal> {
    doc.get_mut("pieces")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("pieces must be an array"))?
        .iter_mut()
        .find(|p| p["id"].as_str() == Some(pid))
        .ok_or_else(|| invalid(format!("no piece '{pid}'")))
}

fn point(value: &Value) -> Result<Vec2, Refusal> {
    let xy = value
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or_else(|| invalid("a point must be [x, y]"))?;
    let number = |v: &Value| {
        v.as_f64()
            .ok_or_else(|| invalid("point coordinates must be numbers"))
    };
    Ok(Vec2::new(number(&xy[0])?, number(&xy[1])?))
}

fn point_value(p: Vec2) -> Result<Value, Refusal> {
    Refusal::finite("altered x", p.x)?;
    Refusal::finite("altered y", p.y)?;
    Ok(json!([p.x, p.y]))
}

fn rounded(p: Vec2) -> Vec2 {
    Vec2::new(py::round(p.x, 3), py::round(p.y, 3))
}

fn vertex_mut<'a>(piece: &'a mut Value, vid: &str) -> Result<&'a mut Value, Refusal> {
    piece
        .get_mut("vertices")
        .and_then(Value::as_object_mut)
        .and_then(|v| v.get_mut(vid))
        .ok_or_else(|| invalid(format!("no vertex '{vid}'")))
}

fn map_vertices(piece: &mut Value, f: &impl Fn(Vec2) -> Vec2) -> Result<(), Refusal> {
    let vertices = piece
        .get_mut("vertices")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("vertices must be an object"))?;
    for value in vertices.values_mut() {
        *value = point_value(f(point(value)?))?;
    }
    Ok(())
}

fn shift_marks(piece: &mut Value, f: &impl Fn(Vec2) -> Vec2) -> Result<(), Refusal> {
    if let Some(grain) = piece.get_mut("grain").filter(|v| !v.is_null()) {
        let (a, b) = if grain.is_object() {
            (point(&grain["p0"])?, point(&grain["p1"])?)
        } else {
            let pair = grain
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or_else(|| invalid("grain must have two points"))?;
            (point(&pair[0])?, point(&pair[1])?)
        };
        *grain = json!([point_value(f(a))?, point_value(f(b))?]);
    }
    for key in ["internal", "decor", "trace_marks"] {
        if let Some(marks) = piece.get_mut(key).filter(|v| !v.is_null()) {
            let marks = marks
                .as_array_mut()
                .ok_or_else(|| invalid(format!("{key} must be an array")))?;
            for mark in marks {
                if let Some(points) = mark.get_mut("pts") {
                    let points = points
                        .as_array_mut()
                        .ok_or_else(|| invalid(format!("{key} pts must be an array")))?;
                    for value in points {
                        *value = point_value(f(point(value)?))?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn history(doc: &mut Value, op: &str, args: Value, why: String) -> Result<(), Refusal> {
    let meta = doc
        .as_object_mut()
        .ok_or_else(|| invalid("pattern must be an object"))?
        .entry("meta")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid("meta must be an object"))?;
    meta.entry("history")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| invalid("meta.history must be an array"))?
        .push(json!({"op": op, "args": args, "why": why}));
    meta.insert("origin".into(), json!("VECTOR_EDIT"));
    Ok(())
}

/// Spread a drafted piece above the strict `x > at` or `y > at` cut, rounding points to 0.001 mm.
/// Grain, internal lines, decoration and trace marks follow; drills and placement do not.
///
/// # Errors
/// Unknown piece, malformed points/history, non-finite inputs or output, or an axis other than x/y.
pub fn slash_spread(
    doc: &Value,
    pid: &str,
    axis: &str,
    at: f64,
    add: f64,
) -> Result<Value, Refusal> {
    Refusal::finite("at", at)?;
    Refusal::finite("add", add)?;
    if !matches!(axis, "x" | "y") {
        return Err(Refusal::new(
            RefusalCode::UnknownParameter,
            "axis must be x or y",
        ));
    }
    let transform = |mut p: Vec2| {
        if axis == "x" && p.x > at {
            p.x += add;
        } else if axis == "y" && p.y > at {
            p.y += add;
        }
        rounded(p)
    };
    let mut out = doc.clone();
    let piece = piece_mut(&mut out, pid)?;
    map_vertices(piece, &transform)?;
    shift_marks(piece, &transform)?;
    let verb = if add > 0.0 { "add" } else { "remove" };
    let dimension = if axis == "x" { "width" } else { "length" };
    history(
        &mut out,
        "slash-spread",
        json!({"piece": pid, "axis": axis, "at": at, "add": add}),
        format!(
            "Slashed {pid} at {axis}={at:.0} mm and spread {add:+.1} mm to {verb} {dimension} there."
        ),
    )?;
    Ok(out)
}

/// Move one drafted vertex by `(dx, dy)` mm. Relative curves follow it; marks do not.
///
/// # Errors
/// Unknown piece/vertex, malformed points/history, or non-finite input/output.
pub fn move_vertex(doc: &Value, pid: &str, vid: &str, dx: f64, dy: f64) -> Result<Value, Refusal> {
    Refusal::finite("dx", dx)?;
    Refusal::finite("dy", dy)?;
    let mut out = doc.clone();
    let vertex = vertex_mut(piece_mut(&mut out, pid)?, vid)?;
    *vertex = point_value(rounded(point(vertex)? + Vec2::new(dx, dy)))?;
    history(
        &mut out,
        "move",
        json!({"piece": pid, "vertex": vid, "dx": dx, "dy": dy}),
        format!("Moved {pid}:{vid} by ({dx:+.1}, {dy:+.1}) mm."),
    )?;
    Ok(out)
}

fn edge_ref(reference: &str) -> Result<(&str, &str), Refusal> {
    reference
        .split_once('/')
        .filter(|(id, edge)| !id.is_empty() && !edge.is_empty() && !edge.contains(['/', '+']))
        .ok_or_else(|| {
            invalid(format!(
                "expected one piece/edge reference, got '{reference}'"
            ))
        })
}

/// Copy curve character from a source INSTANCE/EDGE to a destination PIECE/EDGE, keeping endpoints.
/// Uses 60 samples, every third interior point, and chord-relative controls rounded to five places.
///
/// # Errors
/// Invalid references, unknown pieces/edges, a zero-length source chord or non-finite controls.
pub fn transfer_curve(
    doc: &Value,
    source: &Value,
    src_ref: &str,
    dst_ref: &str,
) -> Result<Value, Refusal> {
    let (iid, src_edge) = edge_ref(src_ref)?;
    let (pid, dst_edge) = edge_ref(dst_ref)?;
    let pattern = built(source)?;
    let instance = pattern
        .instance(iid)
        .ok_or_else(|| invalid(format!("no instance '{iid}'")))?;
    let points = instance
        .edge_dense(src_edge, TRANSFER_DENSE)
        .map_err(|e| invalid(e.to_string()))?;
    let p0 = points[0];
    let p1 = points[points.len() - 1];
    Refusal::finite("source chord squared length", (p1 - p0).norm2())?;
    // Check even a straight source: its empty control list must not hide a zero-length chord.
    abs_to_rel(p0, p1, p0)
        .map_err(|e| Refusal::new(RefusalCode::DegenerateGeometry, e.to_string()))?;
    let mut controls = Vec::new();
    for i in (1..points.len() - 1).step_by(3) {
        let [t, h] = abs_to_rel(p0, p1, points[i])
            .map_err(|e| Refusal::new(RefusalCode::DegenerateGeometry, e.to_string()))?;
        Refusal::finite("curve t", t)?;
        Refusal::finite("curve h", h)?;
        controls.push([py::round(t, 5), py::round(h, 5)]);
    }
    let mut out = doc.clone();
    let edge = piece_mut(&mut out, pid)?
        .get_mut("edges")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("edges must be an array"))?;
    let edge = edge
        .iter_mut()
        .find(|e| e["id"].as_str() == Some(dst_edge))
        .ok_or_else(|| invalid(format!("{pid}: no edge '{dst_edge}'")))?;
    edge["curve"] = json!({"type": "poly", "c": controls});
    history(
        &mut out,
        "transfer-curve",
        json!({"from": src_ref, "to": dst_ref}),
        format!(
            "Gave {dst_ref} the curve character of {src_ref} (shape copied relative to the edge, size kept)."
        ),
    )?;
    Ok(out)
}

/// The reference's 60 halvings, with the plan's deliberate NoRoot exception: impossible adjustments
/// are refusals, never silently saturated bounds presented as successfully trued geometry.
fn bisect(
    mut f: impl FnMut(f64) -> Result<f64, Refusal>,
    mut lo: f64,
    mut hi: f64,
) -> Result<f64, Refusal> {
    let mut flo = Refusal::finite("lower-bound residual", f(lo)?)?;
    let fhi = Refusal::finite("upper-bound residual", f(hi)?)?;
    if flo == 0.0 {
        return Ok(lo);
    }
    if fhi == 0.0 {
        return Ok(hi);
    }
    if (flo > 0.0) == (fhi > 0.0) {
        return Err(Refusal::new(
            RefusalCode::NoRoot,
            format!("no bracketed alteration root in [{lo}, {hi}] (residuals {flo}, {fhi})"),
        )
        .repair("change the target ratio, or use slash-spread/move before solving again"));
    }
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        let fm = Refusal::finite("alteration residual", f(mid)?)?;
        if (fm > 0.0) == (flo > 0.0) {
            lo = mid;
            flo = fm;
        } else {
            hi = mid;
        }
    }
    let root = 0.5 * (lo + hi);
    let residual = Refusal::finite("solved residual", f(root)?)?;
    if residual.abs() > 1e-8 {
        return Err(Refusal::new(
            RefusalCode::NotConverged,
            format!("alteration residual {residual} after 60 bisections"),
        ));
    }
    Ok(root)
}

fn positive(name: &str, value: f64) -> Result<f64, Refusal> {
    Refusal::finite(name, value)?;
    if value <= 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("{name} must be positive"),
        ));
    }
    Ok(value)
}

fn query<T>(r: Result<T, crate::PyError>) -> Result<T, Refusal> {
    r.map_err(|e| invalid(e.message().to_string()))
}

fn drafted_vertex(doc: &Value, pid: &str, vid: &str) -> Result<Vec2, Refusal> {
    let mut probe = doc.clone();
    point(vertex_mut(piece_mut(&mut probe, pid)?, vid)?)
}

/// Make side b of `seam_id` equal side a over the seam's ratio (the mean of its `ratio_band`, else its
/// legacy `band`, when one is truthy; else its `ratio`, else 1) by sliding b's drafted vertex at `end`
/// (`"end"` or `"start"`) along b's own direction there, ±60 mm at most. Edges, mirrored copies and other
/// seams through that vertex follow.
///
/// # Errors
/// An unknown seam, an `end` other than start/end, side b on a mirrored (`.L`) copy, unknown pieces,
/// edges or vertices, invalid ratios/bands, non-finite results, or no bracketed root within ±60 mm.
pub fn true_seam(doc: &Value, seam_id: &str, end: &str) -> Result<Value, Refusal> {
    if !matches!(end, "start" | "end") {
        return Err(Refusal::new(
            RefusalCode::UnknownParameter,
            "end must be start or end",
        ));
    }
    let pattern = built(doc)?;
    if pattern
        .seams()
        .iter()
        .filter(|s| s.id() == Some(seam_id))
        .count()
        > 1
    {
        return Err(invalid(format!("duplicate seam id '{seam_id}'")));
    }
    let seam = pattern
        .seams()
        .iter()
        .find(|s| s.id() == Some(seam_id))
        .ok_or_else(|| invalid(format!("no seam '{seam_id}'")))?;
    let m = &seam.0;
    let band = if m.contains_key("ratio_band") {
        m.get("ratio_band")
    } else {
        m.get("band")
    };
    let ratio = match band.filter(|b| crate::truthy(b)) {
        Some(b) => {
            let xs: Vec<f64> = b
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or_else(|| invalid("ratio_band must be [lo, hi]"))?
                .iter()
                .map(|v| {
                    v.as_f64()
                        .ok_or_else(|| invalid("ratio_band must contain numbers"))
                })
                .collect::<Result<_, _>>()?;
            positive("ratio band lower", xs[0])?;
            positive("ratio band upper", xs[1])?;
            if xs[0] > xs[1] {
                return Err(invalid("ratio_band must be ordered [lo, hi]"));
            }
            ftx_core::np::pairwise_sum(&xs) / xs.len() as f64
        }
        None => match m.get("ratio") {
            None => 1.0,
            Some(r) => r
                .as_f64()
                .ok_or_else(|| invalid(format!("seam {seam_id}: ratio must be a number")))?,
        },
    };
    positive("seam ratio", ratio)?;
    let side = |k: &str| {
        seam.side_ref(k)
            .ok_or_else(|| invalid(format!("seam {seam_id} has no side {k}")))
    };
    let (a, b) = (side("a"), side("b"));
    let (a, b) = (a?, b?);
    let (inst, eids) = query(pattern.side(b))?;
    if inst.mirrored || inst.side == crate::Side::L || eids.iter().any(|e| e.ends_with(".L")) {
        return Err(invalid(
            "true the drafted (.R) side; the mirrored copy follows",
        ));
    }
    let at_end = end == "end";
    let eid = if at_end { eids.last() } else { eids.first() }
        .ok_or_else(|| invalid(format!("seam {seam_id}: side b has no edge")))?;
    let edge = query(inst.edge(eid))?;
    let vid = if at_end { &edge.to } else { &edge.from }.clone();
    let pid = inst.piece.clone();
    let base = drafted_vertex(doc, &pid, &vid)?;
    let dense = query(inst.edge_dense(eid, ftx_core::curve::EDGE_DENSE))?;
    let n = dense.len();
    let t = if at_end {
        dense[n - 1] - dense[n - 2]
    } else {
        dense[0] - dense[1]
    };
    let norm = positive("seam end tangent length", (t.x * t.x + t.y * t.y).sqrt())?;
    let t = Vec2::new(t.x / norm, t.y / norm);
    let target = positive("target seam length", query(pattern.side_length(a))? / ratio)?;
    let d = bisect(
        |d| {
            let mut trial = doc.clone();
            *vertex_mut(piece_mut(&mut trial, &pid)?, &vid)? =
                point_value(Vec2::new(base.x + d * t.x, base.y + d * t.y))?;
            Ok(query(Pattern::new(trial).side_length(b))? - target)
        },
        -60.0,
        60.0,
    )?;
    let mut out = doc.clone();
    *vertex_mut(piece_mut(&mut out, &pid)?, &vid)? =
        point_value(rounded(Vec2::new(base.x + d * t.x, base.y + d * t.y)))?;
    history(
        &mut out,
        "true-seam",
        json!({"seam": seam_id, "end": end, "moved_mm": py::round(d, 2)}),
        format!(
            "Trued seam {seam_id}: moved {pid}:{vid} {} mm along the seam so side b is {} mm (side a / {}).",
            py::signed_fixed(d, 1),
            py::fixed(target, 1),
            py::fixed(ratio, 3)
        ),
    )?;
    Ok(out)
}

/// Scale the height of `pid` above its underarm line (the lowest end of its cap sides in `seams`) so the
/// total cap length over the total armhole length is `target`; marks above the line follow. The two cap
/// halves are not kept equal (spec 01 §17).
///
/// # Errors
/// Unknown/duplicate seams or piece, ambiguous cap refs, non-positive or non-finite target, malformed
/// geometry, or no bracketed root within 0.3–3 × cap height.
pub fn cap_ease(doc: &Value, pid: &str, seams: &[String], target: f64) -> Result<Value, Refusal> {
    positive("target", target)?;
    let pattern = built(doc)?;
    piece_mut(&mut doc.clone(), pid)?;
    for id in seams {
        let count = pattern
            .seams()
            .iter()
            .filter(|s| s.id() == Some(id.as_str()))
            .count();
        if count != 1 {
            return Err(invalid(format!("expected one seam '{id}', found {count}")));
        }
    }
    let (mut caps, mut arms) = (Vec::new(), Vec::new());
    for s in pattern.seams() {
        let Some(id) = s.id() else { continue };
        if !seams.iter().any(|x| x == id) {
            continue;
        }
        let side = |k: &str| {
            s.side_ref(k)
                .ok_or_else(|| invalid(format!("seam {id} has no side {k}")))
        };
        let (a, b) = (side("a")?, side("b")?);
        let (ia, _) = query(pattern.side(a))?;
        let (ib, _) = query(pattern.side(b))?;
        match (ia.piece == pid, ib.piece == pid) {
            (true, false) => {
                caps.push(a.to_string());
                arms.push(b.to_string());
            }
            (false, true) => {
                caps.push(b.to_string());
                arms.push(a.to_string());
            }
            _ => {
                return Err(invalid(format!(
                    "seam {id} must join {pid} to another piece"
                )));
            }
        }
    }
    let first = caps
        .first()
        .ok_or_else(|| invalid(format!("no seam of '{pid}' among {}", seams.join(", "))))?;
    let (inst, _) = query(pattern.side(first))?;
    let y_of = |vid: &str| {
        inst.verts
            .get(vid)
            .map(|p| p.y)
            .ok_or_else(|| invalid(format!("no vertex '{vid}'")))
    };
    let mut y_ua = f64::INFINITY;
    for r in &caps {
        let (cap, eids) = query(pattern.side(r))?;
        if cap.id != inst.id {
            return Err(invalid("cap-ease seams must use one cap instance"));
        }
        if eids.iter().any(|e| e.ends_with(".L")) {
            return Err(invalid(
                "use drafted (.R) cap edges; the mirrored copy follows",
            ));
        }
        for e in &eids {
            let edge = query(cap.edge(e))?;
            y_ua = y_ua.min(y_of(&edge.from)?).min(y_of(&edge.to)?);
        }
    }
    let mut arm = 0.0;
    for r in &arms {
        arm += query(pattern.side_length(r))?;
    }
    positive("armhole length", arm)?;
    Refusal::finite("underarm height", y_ua)?;
    let lift = |k: f64| {
        move |p: Vec2| {
            if p.y > y_ua {
                Vec2::new(p.x, y_ua + k * (p.y - y_ua))
            } else {
                p
            }
        }
    };
    let scaled = |k: f64| -> Result<Value, Refusal> {
        let mut trial = doc.clone();
        map_vertices(piece_mut(&mut trial, pid)?, &lift(k))?;
        Ok(trial)
    };
    let k = bisect(
        |k| {
            let sp = Pattern::new(scaled(k)?);
            let mut sum = 0.0;
            for r in &caps {
                sum += query(sp.side_length(r))?;
            }
            Ok(sum / arm - target)
        },
        0.3,
        3.0,
    )?;
    let mut out = scaled(k)?;
    shift_marks(piece_mut(&mut out, pid)?, &lift(k))?;
    let mut old = 0.0;
    for r in &caps {
        old += query(pattern.side_length(r))?;
    }
    let old = Refusal::finite("original cap ease", old / arm - 1.0)?;
    history(
        &mut out,
        "cap-ease",
        json!({"piece": pid, "seams": seams, "target": target, "height_scale": py::round(k, 4)}),
        format!(
            "Scaled the {pid} cap height by {} so cap ease goes from {}% to {}% of the armhole.",
            py::fixed(k, 3),
            py::fixed(old * 100.0, 1),
            py::fixed((target - 1.0) * 100.0, 1)
        ),
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(id: &str, height: f64) -> Value {
        json!({
            "id": id, "name": id.to_uppercase(), "cut": {"qty": 1},
            "vertices": {"a": [0, 0], "b": [100, 0], "c": [100, height], "d": [0, height]},
            "edges": [
                {"id": "bottom", "from": "a", "to": "b"},
                {"id": "right", "from": "b", "to": "c"},
                {"id": "top", "from": "c", "to": "d"},
                {"id": "left", "from": "d", "to": "a"}
            ],
            "notches": [], "grain": [[10, 10], [10, 40]]
        })
    }

    fn document(pieces: Vec<Value>) -> Value {
        json!({"schema": "fabric-pattern/1", "units": "mm", "pieces": pieces, "seams": []})
    }

    #[test]
    fn f33_f35_slash_spread_and_history() {
        let original = document(vec![rectangle("r", 50.0)]);
        let before = original.clone();
        let out = slash_spread(&original, "r", "x", 50.0, 12.0).unwrap();
        assert_eq!(original, before);
        assert_eq!(out["pieces"][0]["vertices"]["c"], json!([112.0, 50.0]));
        assert_eq!(
            out["pieces"][0]["grain"],
            json!([[10.0, 10.0], [10.0, 40.0]])
        );
        assert_eq!(out["meta"]["origin"], "VECTOR_EDIT");
        assert_eq!(
            out["meta"]["history"][0],
            json!({
                "op": "slash-spread", "args": {"piece": "r", "axis": "x", "at": 50.0, "add": 12.0},
                "why": "Slashed r at x=50 mm and spread +12.0 mm to add width there."
            })
        );
        let zero = slash_spread(&out, "r", "x", 50.0, 0.0).unwrap();
        assert_eq!(zero["meta"]["history"].as_array().unwrap().len(), 2);
        assert_eq!(
            zero["meta"]["history"][1]["why"],
            "Slashed r at x=50 mm and spread +0.0 mm to remove width there."
        );
    }

    #[test]
    fn f34_marks_follow_strict_cut_but_drills_and_anchors_do_not() {
        let mut piece = rectangle("r", 50.0);
        piece["grain"] = json!({"p0": [60, 10], "p1": [60, 40]});
        piece["internal"] = json!([{"kind": "draw", "pts": [[0, 25], [100, 25]]}]);
        piece["decor"] = json!([{"pts": [[80, 40]], "custom": 7}]);
        piece["trace_marks"] = json!([{"pts": [[80, 40]]}]);
        piece["drills"] = json!([{"p": [80, 40], "d": 3}]);
        piece["placement"] = json!({"y0": 40});
        let mut original = document(vec![piece]);
        original["meta"] = json!({"source": "synthetic", "extra": 42});
        let out = slash_spread(&original, "r", "y", 25.0, -5.0).unwrap();
        let p = &out["pieces"][0];
        assert_eq!(p["vertices"]["c"], json!([100.0, 45.0]));
        assert_eq!(p["grain"], json!([[60.0, 10.0], [60.0, 35.0]]));
        assert_eq!(p["internal"][0]["pts"], json!([[0.0, 25.0], [100.0, 25.0]]));
        for key in ["decor", "trace_marks"] {
            assert_eq!(p[key][0]["pts"], json!([[80.0, 35.0]]));
        }
        for key in ["drills", "placement", "notches"] {
            assert_eq!(p[key], original["pieces"][0][key]);
        }
        assert_eq!(p["decor"][0]["custom"], 7);
        assert_eq!(out["meta"]["source"], "synthetic");
        assert_eq!(out["meta"]["extra"], 42);
    }

    #[test]
    fn f36_move_rounds_half_even_and_keeps_relative_curves() {
        let mut piece = rectangle("r", 50.0);
        piece["edges"][1]["curve"] = json!({"type": "quad", "c": [[0.5, 0.2]]});
        let original = document(vec![piece]);
        let out = move_vertex(&original, "r", "c", 1.25, -2.0).unwrap();
        assert_eq!(out["pieces"][0]["vertices"]["c"], json!([101.25, 48.0]));
        assert_eq!(
            out["meta"]["history"][0]["why"],
            "Moved r:c by (+1.2, -2.0) mm."
        );
        assert_eq!(out["pieces"][0]["edges"], original["pieces"][0]["edges"]);
        assert_eq!(out["pieces"][0]["grain"], original["pieces"][0]["grain"]);
        assert!(move_vertex(&original, "q", "c", 0.0, 0.0).is_err());
        assert!(move_vertex(&original, "r", "q", 0.0, 0.0).is_err());
    }

    #[test]
    fn f39_f40_transfer_sample_counts_controls_and_endpoints() {
        let destination = document(vec![rectangle("r", 50.0)]);
        let mut source = document(vec![rectangle("s", 50.0)]);
        let line = transfer_curve(&destination, &source, "s/right", "r/right").unwrap();
        assert_eq!(
            line["pieces"][0]["edges"][1]["curve"],
            json!({"type": "poly", "c": []})
        );
        source["pieces"][0]["edges"][1]["curve"] = json!({"type": "quad", "c": [[0.5, 0.2]]});
        let out = transfer_curve(&destination, &source, "s/right", "r/right").unwrap();
        let controls = out["pieces"][0]["edges"][1]["curve"]["c"]
            .as_array()
            .unwrap();
        assert_eq!(controls.len(), 20);
        assert_eq!(controls[0], json!([0.01695, 0.00666]));
        assert_eq!(controls[1], json!([0.0678, 0.02528]));
        assert_eq!(controls[19], json!([0.98305, 0.00666]));
        assert_eq!(
            out["pieces"][0]["vertices"],
            destination["pieces"][0]["vertices"]
        );
        let length = built(&out).unwrap().side_length("r/right").unwrap();
        assert!((length - 51.29994557770592).abs() < 1e-10);
        assert!(transfer_curve(&destination, &source, "s/right", "r/nope").is_err());
        assert!(transfer_curve(&destination, &source, "s/right+top", "r/right").is_err());
    }

    #[test]
    fn refusals_do_not_mutate_the_input_or_turn_overflow_into_null() {
        let mut doc = document(vec![rectangle("r", 50.0)]);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                move_vertex(&doc, "r", "a", value, 0.0).unwrap_err().code,
                RefusalCode::NonFinite
            );
        }
        assert!(slash_spread(&doc, "r", "z", 0.0, 2.0).is_err());
        doc["pieces"][0]["vertices"]["a"] = json!([f64::MAX, 0.0]);
        let before = doc.clone();
        assert_eq!(
            move_vertex(&doc, "r", "a", f64::MAX, 0.0).unwrap_err().code,
            RefusalCode::NonFinite
        );
        assert_eq!(doc, before);
        doc["meta"] = json!({"history": "not a list"});
        let before = doc.clone();
        assert!(move_vertex(&doc, "r", "c", 1.0, 1.0).is_err());
        assert_eq!(doc, before);
    }
}
