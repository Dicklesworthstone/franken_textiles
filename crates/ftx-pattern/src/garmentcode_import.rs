//! Bounded import of evaluated GarmentCode net sewing geometry.
//!
//! Panels are already physical instances: each becomes one native piece. Manufacturing intent,
//! native body placement and fit are supplied separately, never inferred from a simulation mesh.
//! The geometric audit uses sampled outlines; it does not certify analytic Bezier simplicity.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::curve::{EDGE_DENSE, curve_points, rel_to_abs, reverse_curve};
use ftx_core::exact::{Sign, orient2d};
use ftx_core::polyline::signed_area;
use ftx_core::receipt::sha256_hex;
use ftx_core::{Curve, Refusal, RefusalCode, Vec2};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::Pattern;

/// Maximum bytes in each original JSON input, before parsing or hashing.
pub const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
const MAX_PANELS: usize = 128;
const MAX_VERTICES: usize = 16_384;
const MAX_EDGES: usize = 16_384;
const MAX_SAMPLES: usize = 65_536;
const MAX_PAIRS: usize = 4_000_000;
const MAX_MM: f64 = 1_000_000.0;

/// Converted native geometry and an explicit review report; neither establishes garment fit.
#[derive(Clone, Debug, Serialize)]
pub struct ImportResult {
    /// Ordinary fabric-pattern/1 JSON.
    pub pattern: Value,
    /// Source mappings, limitations, missing semantics and optional supplied fit specification.
    pub report: Value,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn budget(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}

fn geometry(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message)
}

fn object<'a>(v: &'a Value, at: &str) -> Result<&'a Map<String, Value>, Refusal> {
    v.as_object()
        .ok_or_else(|| invalid(format!("{at}: expected an object")))
}

fn keys(v: &Value, allowed: &[&str], at: &str) -> Result<(), Refusal> {
    for key in object(v, at)?.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid(format!("{at}: unsupported field {key}")));
        }
    }
    Ok(())
}

fn array<'a>(v: &'a Value, at: &str) -> Result<&'a [Value], Refusal> {
    v.as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid(format!("{at}: expected an array")))
}

fn number(v: &Value, at: &str) -> Result<f64, Refusal> {
    let x = v
        .as_f64()
        .ok_or_else(|| invalid(format!("{at}: expected a number")))?;
    Refusal::finite(at, x)
}

fn point(v: &Value, scale: f64, at: &str) -> Result<Vec2, Refusal> {
    let a = array(v, at)?;
    if a.len() != 2 {
        return Err(invalid(format!("{at}: expected two coordinates")));
    }
    let p = Vec2::new(number(&a[0], at)? * scale, number(&a[1], at)? * scale);
    if !p.is_finite() || p.x.abs() > MAX_MM || p.y.abs() > MAX_MM {
        return Err(invalid(format!(
            "{at}: coordinates must be finite and within {MAX_MM} mm"
        )));
    }
    Ok(p)
}

fn index(v: &Value, n: usize, at: &str) -> Result<usize, Refusal> {
    let i = v
        .as_u64()
        .and_then(|x| usize::try_from(x).ok())
        .filter(|&x| x < n)
        .ok_or_else(|| invalid(format!("{at}: index is outside 0..{n}")))?;
    Ok(i)
}

// The Value entry point is bounded too; its caller need not have used our text parser.
fn inspect_json(v: &Value, depth: usize, remaining: &mut usize) -> Result<(), Refusal> {
    if depth > 64 || *remaining < 8 {
        return Err(budget("GarmentCode JSON exceeds its depth or data budget"));
    }
    *remaining -= 8;
    let mut debit = |n: usize| -> Result<(), Refusal> {
        *remaining = remaining
            .checked_sub(n)
            .ok_or_else(|| budget("GarmentCode JSON exceeds its data budget"))?;
        Ok(())
    };
    match v {
        Value::String(s) => debit(s.len())?,
        Value::Number(_) => {
            number(v, "JSON number")?;
        }
        Value::Array(a) => {
            for x in a {
                inspect_json(x, depth + 1, remaining)?;
            }
        }
        Value::Object(o) => {
            for (k, x) in o {
                *remaining = remaining
                    .checked_sub(k.len())
                    .ok_or_else(|| budget("GarmentCode JSON exceeds its data budget"))?;
                inspect_json(x, depth + 1, remaining)?;
            }
        }
        Value::Null | Value::Bool(_) => {}
    }
    Ok(())
}

fn curve(v: Option<&Value>, relative: bool, at: &str) -> Result<Option<Curve>, Refusal> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    if !relative {
        return Err(invalid(format!(
            "{at}: curved input requires properties.curvature_coords = relative"
        )));
    }
    let (kind, params) = if v.is_array() {
        ("quad", vec![point(v, 1.0, at)?])
    } else {
        keys(v, &["type", "params"], at)?;
        let p = array(&v["params"], at)?;
        match v["type"].as_str() {
            Some("quadratic") if p.len() == 2 && p[0].is_number() => {
                ("quad", vec![point(&v["params"], 1.0, at)?])
            }
            Some("quadratic") if p.len() == 1 => ("quad", vec![point(&p[0], 1.0, at)?]),
            Some("cubic") if p.len() == 2 => (
                "cubic",
                vec![point(&p[0], 1.0, at)?, point(&p[1], 1.0, at)?],
            ),
            _ => {
                return Err(invalid(format!(
                    "{at}: only quadratic and cubic relative Beziers are supported"
                )));
            }
        }
    };
    Ok(Some(Curve {
        kind: Some(kind.into()),
        c: Some(params.into_iter().map(|p| [p.x, p.y]).collect()),
        extra: Map::new(),
    }))
}

#[derive(Default)]
struct Work {
    vertices: usize,
    edges: usize,
    samples: usize,
    pairs: usize,
}

fn on_segment(p: Vec2, a: Vec2, b: Vec2) -> bool {
    a.x.min(b.x) <= p.x && p.x <= a.x.max(b.x) && a.y.min(b.y) <= p.y && p.y <= a.y.max(b.y)
}

fn orientation(a: Vec2, b: Vec2, c: Vec2) -> Sign {
    orient2d([a.x, a.y], [b.x, b.y], [c.x, c.y])
}

fn meets(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let (s0, s1, s2, s3) = (
        orientation(a, b, c),
        orientation(a, b, d),
        orientation(c, d, a),
        orientation(c, d, b),
    );
    let opposite = |x, y| {
        matches!(
            (x, y),
            (Sign::Negative, Sign::Positive) | (Sign::Positive, Sign::Negative)
        )
    };
    (opposite(s0, s1) && opposite(s2, s3))
        || (s0 == Sign::Zero && on_segment(c, a, b))
        || (s1 == Sign::Zero && on_segment(d, a, b))
        || (s2 == Sign::Zero && on_segment(a, c, d))
        || (s3 == Sign::Zero && on_segment(b, c, d))
}

fn audit_outline(p: &[Vec2], work: &mut Work, at: &str) -> Result<f64, Refusal> {
    let n = p.len();
    let mut boxes = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = (p[i], p[(i + 1) % n]);
        if a == b {
            return Err(geometry(format!("{at}: zero-length sampled segment")));
        }
        let c = p[(i + 2) % n];
        if orientation(a, b, c) == Sign::Zero && (a - b).dot(c - b) > 0.0 {
            return Err(geometry(format!("{at}: adjacent outline segments overlap")));
        }
        boxes.push((a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y), i));
    }
    boxes.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.4.cmp(&b.4)));
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            if b.0 > a.1 {
                break;
            }
            work.pairs += 1;
            if work.pairs > MAX_PAIRS {
                return Err(budget("outline intersection work exceeds 4000000 pairs"));
            }
            let (ia, ib) = (a.4, b.4);
            if b.2 > a.3 || a.2 > b.3 || (ia + 1) % n == ib || (ib + 1) % n == ia {
                continue;
            }
            if meets(p[ia], p[(ia + 1) % n], p[ib], p[(ib + 1) % n]) {
                return Err(geometry(format!(
                    "{at}: sampled outline intersects or touches itself"
                )));
            }
        }
    }
    let area = signed_area(p);
    if !area.is_finite() || area == 0.0 {
        return Err(geometry(format!("{at}: degenerate outline")));
    }
    Ok(area)
}

struct Panel {
    id: String,
    flipped: bool,
    edge_count: usize,
    piece: Value,
}

fn panel(
    source: &Value,
    name: &str,
    id: String,
    scale: f64,
    relative: bool,
    work: &mut Work,
) -> Result<Panel, Refusal> {
    keys(
        source,
        &["vertices", "edges", "translation", "rotation"],
        name,
    )?;
    for field in ["translation", "rotation"] {
        if let Some(v) = source.get(field) {
            let a = array(v, field)?;
            if a.len() != 3 {
                return Err(invalid(format!("{name}.{field}: expected three numbers")));
            }
            for x in a {
                number(x, field)?;
            }
        }
    }
    let vv = array(&source["vertices"], name)?;
    let ee = array(&source["edges"], name)?;
    work.vertices += vv.len();
    work.edges += ee.len();
    if work.vertices > MAX_VERTICES || work.edges > MAX_EDGES {
        return Err(budget("too many GarmentCode vertices or edges"));
    }
    if vv.len() < 3 || ee.len() < 2 {
        return Err(geometry(format!("{name}: not a closed panel")));
    }
    let verts: Vec<Vec2> = vv
        .iter()
        .map(|v| point(v, scale, name))
        .collect::<Result<_, _>>()?;
    let mut edges = Vec::new();
    let mut outline = Vec::new();
    let mut used = BTreeSet::new();
    for (i, e) in ee.iter().enumerate() {
        keys(e, &["endpoints", "curvature", "label"], name)?;
        let ends = array(&e["endpoints"], name)?;
        if ends.len() != 2 {
            return Err(invalid(format!("{name}: edge needs two endpoints")));
        }
        let (a, b) = (
            index(&ends[0], vv.len(), name)?,
            index(&ends[1], vv.len(), name)?,
        );
        if a == b || verts[a] == verts[b] {
            return Err(geometry(format!("{name}: zero-length edge chord")));
        }
        used.insert(a);
        used.insert(b);
        let c = curve(e.get("curvature"), relative, name)?;
        if let Some(c) = &c {
            for &th in c.controls() {
                let q = rel_to_abs(verts[a], verts[b], th);
                if !q.is_finite() || q.x.abs() > MAX_MM || q.y.abs() > MAX_MM {
                    return Err(geometry(format!(
                        "{name}: curve control exceeds coordinate bounds"
                    )));
                }
            }
        }
        let sampled = curve_points(verts[a], verts[b], c.as_ref(), EDGE_DENSE)
            .map_err(|e| geometry(e.to_string()))?;
        work.samples += sampled.len() - 1;
        if work.samples > MAX_SAMPLES {
            return Err(budget("sampled outlines exceed 65536 points"));
        }
        outline.extend_from_slice(&sampled[..sampled.len() - 1]);
        let mut edge =
            json!({"id":format!("e{i:04}"),"from":format!("v{a:04}"),"to":format!("v{b:04}")});
        if let Some(c) = c {
            edge["curve"] = serde_json::to_value(c).map_err(|e| invalid(e.to_string()))?;
        }
        if let Some(label) = e.get("label") {
            edge["garmentcode_label"] = label.clone();
        }
        edges.push(edge);
    }
    for i in 0..edges.len() {
        if edges[i]["to"] != edges[(i + 1) % edges.len()]["from"] {
            return Err(invalid(format!(
                "{name}: edges must be one consecutive closed loop"
            )));
        }
    }
    if used.len() != verts.len() {
        return Err(invalid(format!("{name}: unused vertices are unsupported")));
    }
    let flipped = audit_outline(&outline, work, name)? < 0.0;
    if flipped {
        edges.reverse();
        for e in &mut edges {
            let from = e["from"].clone();
            e["from"] = e["to"].clone();
            e["to"] = from;
            if let Some(c) = e.get("curve") {
                let c: Curve =
                    serde_json::from_value(c.clone()).map_err(|e| invalid(e.to_string()))?;
                e["curve"] = serde_json::to_value(reverse_curve(Some(&c)))
                    .map_err(|e| invalid(e.to_string()))?;
            }
        }
    }
    let vertices: Map<String, Value> = verts
        .iter()
        .enumerate()
        .map(|(i, p)| (format!("v{i:04}"), json!([p.x, p.y])))
        .collect();
    let piece = json!({"id":id,"name":name,"cut":{"qty":1},"vertices":vertices,"edges":edges,
        "garmentcode_source":{"panel":name,"translation":source.get("translation"),"rotation":source.get("rotation")}});
    Ok(Panel {
        id,
        flipped,
        edge_count: ee.len(),
        piece,
    })
}

fn nonnegative(v: &Value, at: &str) -> Result<(), Refusal> {
    let x = number(v, at)?;
    if !(0.0..=MAX_MM).contains(&x) {
        return Err(invalid(format!("{at}: must be within 0..{MAX_MM}")));
    }
    Ok(())
}

fn annotate_panel(p: &mut Panel, a: &Value) -> Result<(), Refusal> {
    keys(
        a,
        &[
            "role",
            "grain",
            "placement",
            "sa",
            "sim",
            "note",
            "material",
            "notches",
        ],
        "panel annotation",
    )?;
    for (k, v) in object(a, "panel annotation")? {
        match k.as_str() {
            "role" | "note" | "material" if !v.is_string() => {
                return Err(invalid(format!("annotation {k}: expected a string")));
            }
            "sim" if !v.is_boolean() => return Err(invalid("annotation sim must be boolean")),
            "sa" => nonnegative(v, "seam allowance")?,
            "grain" => {
                let g = array(v, "grain")?;
                if g.len() != 2 || point(&g[0], 1.0, "grain")? == point(&g[1], 1.0, "grain")? {
                    return Err(invalid("grain needs two distinct native-mm points"));
                }
            }
            "placement" => {
                keys(
                    v,
                    &[
                        "part",
                        "y0_landmark",
                        "y0",
                        "x0",
                        "theta_deg",
                        "radius_mm",
                        "clearance_mm",
                    ],
                    "placement",
                )?;
                if matches!(v["part"].as_str(), Some("arm" | "leg")) {
                    return Err(invalid(
                        "expanded GarmentCode panels need explicit physical limb-side placement; native quantity-one limb wrapping would use the right frame",
                    )
                    .repair("Omit limb placement for a flat-only import; qualify explicit native limb-side support before draping."));
                }
                if v["part"] != "torso" {
                    return Err(invalid("imported placement.part must be torso"));
                }
                for (key, value) in object(v, "placement")? {
                    match key.as_str() {
                        "part" => {}
                        "y0_landmark" if value.is_string() => {}
                        "y0_landmark" => {
                            return Err(invalid("placement landmark must be a string"));
                        }
                        "radius_mm" => {
                            nonnegative(value, key)?;
                            if number(value, key)? == 0.0 {
                                return Err(invalid("placement radius must be positive"));
                            }
                        }
                        "clearance_mm" => nonnegative(value, key)?,
                        _ => {
                            number(value, key)?;
                        }
                    }
                }
            }
            "notches" => {
                let mut notches = Vec::new();
                for n in array(v, "notches")? {
                    keys(n, &["id", "edge", "s", "type", "depth", "width"], "notch")?;
                    let edge = index(&n["edge"], p.edge_count, "notch edge")?;
                    let s = number(&n["s"], "notch fraction")?;
                    if !(0.0..=1.0).contains(&s) {
                        return Err(invalid("notch fraction must be in 0..1"));
                    }
                    for key in ["id", "type"] {
                        if n.get(key).is_some_and(|v| !v.is_string()) {
                            return Err(invalid(format!("notch {key} must be a string")));
                        }
                    }
                    for key in ["depth", "width"] {
                        if let Some(x) = n.get(key) {
                            nonnegative(x, key)?;
                        }
                    }
                    let mut n = n.clone();
                    n["edge"] = json!(format!("e{edge:04}"));
                    n["s"] = json!(if p.flipped { 1.0 - s } else { s });
                    notches.push(n);
                }
                p.piece[k] = json!(notches);
                continue;
            }
            _ => {}
        }
        p.piece[k] = v.clone();
    }
    if p.piece["sim"] == true && p.piece.get("placement").is_none() {
        return Err(invalid("sim:true requires explicit native placement"));
    }
    Ok(())
}

fn annotate_stitch(seam: &mut Value, a: &Value) -> Result<(), Refusal> {
    keys(
        a,
        &["ratio", "ratio_band", "tol_mm", "kind"],
        "stitch annotation",
    )?;
    if let Some(v) = a.get("ratio")
        && number(v, "seam ratio")? <= 0.0
    {
        return Err(invalid("seam ratio must be positive"));
    }
    if let Some(v) = a.get("ratio_band") {
        let b = array(v, "ratio_band")?;
        if b.len() != 2
            || number(&b[0], "ratio_band")? <= 0.0
            || number(&b[0], "ratio_band")? > number(&b[1], "ratio_band")?
        {
            return Err(invalid("ratio_band must have positive ordered bounds"));
        }
        if let Some(r) = a.get("ratio") {
            let r = number(r, "ratio")?;
            if r < number(&b[0], "ratio_band")? || r > number(&b[1], "ratio_band")? {
                return Err(invalid("ratio must lie inside ratio_band"));
            }
        }
    }
    if let Some(v) = a.get("tol_mm") {
        nonnegative(v, "seam tolerance")?;
    }
    if a.get("kind")
        .is_some_and(|v| !matches!(v.as_str(), Some("plain" | "ease" | "gather" | "dart")))
    {
        return Err(invalid("unsupported annotated seam kind"));
    }
    for (k, v) in object(a, "stitch annotation")? {
        seam[k] = v.clone();
    }
    Ok(())
}

fn fit_name<'a>(v: &'a Value, at: &str) -> Result<&'a str, Refusal> {
    v.as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(format!("{at}: expected a nonempty name")))
}

fn fit_spec(fit: &Value, pattern: &Pattern) -> Result<(), Refusal> {
    if fit["schema"] != "fabric-fit/1" {
        return Err(invalid("annotation fit.schema must be fabric-fit/1"));
    }
    let points = array(&fit["points"], "fit.points")?;
    if points.is_empty() || points.len() > 512 {
        return Err(invalid("fit needs 1..512 explicit points of measure"));
    }
    let mut point_ids = BTreeSet::new();
    let mut references = 0usize;
    for p in points {
        let id = fit_name(&p["id"], "fit point id")?;
        if !point_ids.insert(id) {
            return Err(invalid(format!("duplicate fit point id {id}")));
        }
        if !matches!(
            p["kind"].as_str(),
            Some("girth" | "width" | "vertical" | "path")
        ) || p["body"].as_str().is_none_or(str::is_empty)
        {
            return Err(invalid(
                "fit points need a supported kind and body measurement",
            ));
        }
        let t = array(&p["target_mm"], "fit target")?;
        if t.len() != 3
            || number(&t[0], "fit target")? > number(&t[1], "fit target")?
            || number(&t[1], "fit target")? > number(&t[2], "fit target")?
        {
            return Err(invalid("fit target_mm must be [minimum, nominal, maximum]"));
        }
        for field in ["scale", "body_scale"] {
            if let Some(v) = p.get(field)
                && number(v, field)? <= 0.0
            {
                return Err(invalid(format!("fit {field} must be positive")));
            }
        }
        for field in ["dy", "body_offset_mm"] {
            if let Some(v) = p.get(field) {
                number(v, field)?;
            }
        }
        match p["kind"].as_str() {
            Some("path") => {
                let refs = array(&p["refs"], "fit path refs")?;
                if refs.is_empty() {
                    return Err(invalid("fit path needs nonempty refs"));
                }
                for r in refs {
                    let r = fit_name(r, "fit path ref")?;
                    references += r.split('+').count();
                    if references > 1024 {
                        return Err(budget("fit exceeds 1024 instance/edge references"));
                    }
                    pattern
                        .side(r)
                        .map_err(|e| invalid(format!("fit point {id}: {e}")))?;
                }
            }
            Some("girth") => {
                fit_name(&p["landmark"], "fit girth landmark")?;
                let ids = array(&p["instances"], "fit girth instances")?;
                if ids.is_empty() {
                    return Err(invalid("fit girth needs nonempty instances"));
                }
                let mut seen = BTreeSet::new();
                for v in ids {
                    let name = fit_name(v, "fit instance")?;
                    if !seen.insert(name) {
                        return Err(invalid(format!("fit girth repeats instance {name}")));
                    }
                    references += 1;
                    if references > 1024 {
                        return Err(budget("fit exceeds 1024 instance/edge references"));
                    }
                    if pattern.instance(name).is_none() {
                        return Err(invalid(format!("fit names unknown instance {name}")));
                    }
                }
            }
            Some("width" | "vertical") => {
                let name = fit_name(&p["instance"], "fit instance")?;
                let inst = pattern
                    .instance(name)
                    .ok_or_else(|| invalid(format!("fit names unknown instance {name}")))?;
                references += 1;
                if references > 1024 {
                    return Err(budget("fit exceeds 1024 instance/edge references"));
                }
                let vertex = |v: &Value| -> Result<(), Refusal> {
                    let name = fit_name(v, "fit vertex")?;
                    if !inst.verts.contains(name) {
                        return Err(invalid(format!("fit names unknown vertex {name}")));
                    }
                    Ok(())
                };
                if p["kind"] == "vertical" {
                    vertex(&p["from"])?;
                    vertex(&p["to"])?;
                    if p["from"] == p["to"] {
                        return Err(invalid("vertical fit endpoints must be distinct vertices"));
                    }
                } else if let Some(v) = p.get("at_vertex") {
                    vertex(v)?;
                } else {
                    fit_name(&p["landmark"], "fit width landmark")?;
                }
            }
            _ => return Err(invalid("unsupported fit point kind")),
        }
    }
    for field in ["seam_tol_mm", "truing_tol_deg"] {
        if let Some(v) = fit.get(field) {
            nonnegative(v, field)?;
        }
    }
    if let Some(truing) = fit.get("truing") {
        let seam_ids: BTreeSet<&str> = pattern.seams().iter().filter_map(crate::Seam::id).collect();
        let mut seen = BTreeSet::new();
        for row in array(truing, "fit truing")? {
            let seam = fit_name(&row["seam"], "truing seam")?;
            let end = fit_name(&row["end"], "truing end")?;
            if !seam_ids.contains(seam)
                || !matches!(end, "start" | "end")
                || !seen.insert((seam, end))
            {
                return Err(invalid(
                    "truing must name a known seam and a distinct start/end",
                ));
            }
        }
    }
    Ok(())
}

/// Imports an evaluated GarmentCode value, with optional fabric-garmentcode-annotations/1.
///
/// Exact source-byte identity is unavailable through this API; use the JSON-text wrapper for it.
/// Every successful result remains REVIEW_REQUIRED. Normal validation, measurement readiness,
/// placement/contact qualification and export acceptance remain separate operations.
///
/// # Errors
/// Unsupported or malformed geometry/annotations, invalid references, sampled intersections,
/// degenerate panels, or exceeded deterministic input/work budgets.
#[allow(clippy::too_many_lines)]
pub fn import_garmentcode(
    input: &Value,
    annotations: Option<&Value>,
) -> Result<ImportResult, Refusal> {
    let mut input_budget = MAX_JSON_BYTES;
    inspect_json(input, 0, &mut input_budget)?;
    keys(
        input,
        &[
            "pattern",
            "properties",
            "parameters",
            "parameter_order",
            "fabric_pattern_export",
        ],
        "GarmentCode",
    )?;
    if input
        .get("parameters")
        .is_some_and(|v| v.as_object().is_none_or(|o| !o.is_empty()))
        || input
            .get("parameter_order")
            .is_some_and(|v| v.as_array().is_none_or(|a| !a.is_empty()))
    {
        return Err(invalid(
            "only evaluated geometry with empty parameters is supported",
        ));
    }
    let props = &input["properties"];
    keys(
        props,
        &[
            "units_in_meter",
            "curvature_coords",
            "normalize_panel_translation",
            "normalized_edge_loops",
        ],
        "properties",
    )?;
    let units = number(&props["units_in_meter"], "units_in_meter")?;
    let scale = 1000.0 / units;
    if units <= 0.0 || !scale.is_finite() {
        return Err(invalid(
            "units_in_meter must be positive with a finite mm conversion",
        ));
    }
    if props
        .get("normalize_panel_translation")
        .is_some_and(|v| v != false)
    {
        return Err(invalid("normalized panel translation must be disabled"));
    }
    if props
        .get("normalized_edge_loops")
        .is_some_and(|v| !v.is_boolean())
    {
        return Err(invalid("normalized_edge_loops must be boolean"));
    }
    let raw = &input["pattern"];
    keys(raw, &["panels", "stitches", "panel_order"], "pattern")?;
    let sources = object(&raw["panels"], "panels")?;
    if sources.is_empty() || sources.len() > MAX_PANELS {
        return Err(budget("GarmentCode needs 1..128 panels"));
    }
    let names: Vec<String> = if let Some(order) = raw.get("panel_order") {
        array(order, "panel_order")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| invalid("panel_order needs names"))
            })
            .collect::<Result<_, _>>()?
    } else {
        sources.keys().cloned().collect()
    };
    let name_set: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    if names.len() != sources.len()
        || name_set.len() != names.len()
        || names
            .iter()
            .any(|n| !sources.contains_key(n) || n.is_empty() || n.len() > 256)
    {
        return Err(invalid(
            "panel_order must name every panel exactly once; names need 1..256 bytes",
        ));
    }
    let empty = json!({});
    let (pa, sa) = if let Some(a) = annotations {
        let mut annotation_budget = MAX_JSON_BYTES;
        inspect_json(a, 0, &mut annotation_budget)?;
        keys(
            a,
            &["schema", "panels", "stitches", "fit", "provenance"],
            "annotations",
        )?;
        if a["schema"] != "fabric-garmentcode-annotations/1" {
            return Err(invalid(
                "annotations.schema must be fabric-garmentcode-annotations/1",
            ));
        }
        if let Some(provenance) = a.get("provenance") {
            object(provenance, "provenance")?;
        }
        (
            a.get("panels").unwrap_or(&empty),
            a.get("stitches").unwrap_or(&empty),
        )
    } else {
        (&empty, &empty)
    };
    for name in object(pa, "panel annotations")?.keys() {
        if !sources.contains_key(name) {
            return Err(invalid(format!("annotation names unknown panel {name}")));
        }
    }
    let mut work = Work::default();
    let mut panels = BTreeMap::new();
    let mut mappings = Vec::new();
    let mut missing = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let mut p = panel(
            &sources[name],
            name,
            format!("p{i:04}"),
            scale,
            props["curvature_coords"] == "relative",
            &mut work,
        )?;
        if let Some(a) = pa.get(name) {
            annotate_panel(&mut p, a)?;
        }
        for field in ["grain", "sa", "role"] {
            if p.piece.get(field).is_none() {
                missing.push(format!("{}:{field}", p.id));
            }
        }
        if p.piece.get("placement").is_none() && p.piece["sim"] != false {
            missing.push(format!("{}:simulation_scope", p.id));
        }
        mappings.push(json!({"source_panel":name,"piece":p.id,"winding_reversed":p.flipped,
            "edges":(0..p.edge_count).map(|e| json!({"source_edge":e,"edge":format!("e{e:04}")})).collect::<Vec<_>>()}));
        panels.insert(name.clone(), p);
    }
    let no_stitches = json!([]);
    let stitches = array(raw.get("stitches").unwrap_or(&no_stitches), "stitches")?;
    if stitches.len() > MAX_EDGES / 2 {
        return Err(budget("too many GarmentCode stitches"));
    }
    for key in object(sa, "stitch annotations")?.keys() {
        let i = key
            .parse::<usize>()
            .ok()
            .filter(|&i| i < stitches.len() && i.to_string() == *key);
        if i.is_none() {
            return Err(invalid(format!("annotation names unknown stitch {key}")));
        }
    }
    let mut used = BTreeSet::new();
    let mut seams = Vec::new();
    let mut stitch_map = Vec::new();
    for (i, stitch) in stitches.iter().enumerate() {
        let s = array(stitch, "stitch")?;
        if s.len() != 2 && !(s.len() == 3 && s[2] == "right_wrong") {
            return Err(invalid("stitch needs two sides and optional right_wrong"));
        }
        let mut sides = Vec::new();
        let mut reverse = s.len() == 2;
        for side in &s[..2] {
            keys(side, &["panel", "edge"], "stitch side")?;
            let name = side["panel"]
                .as_str()
                .ok_or_else(|| invalid("stitch panel must be a name"))?;
            let p = panels
                .get(name)
                .ok_or_else(|| invalid(format!("unknown stitch panel {name}")))?;
            let e = index(&side["edge"], p.edge_count, "stitch edge")?;
            if !used.insert((name.to_string(), e)) {
                return Err(invalid(format!(
                    "panel {name} edge {e} is stitched more than once"
                )));
            }
            reverse ^= p.flipped;
            sides.push(format!("{}/e{e:04}", p.id));
        }
        let id = format!("s{i:04}");
        let mut seam = json!({"id":id,"a":sides[0],"b":sides[1],"reverse":reverse});
        if let Some(a) = sa.get(i.to_string()) {
            annotate_stitch(&mut seam, a)?;
        }
        if seam.get("ratio").is_none() && seam.get("ratio_band").is_none() {
            missing.push(format!("{id}:seam_intent"));
        }
        stitch_map.push(json!({"source_stitch":i,"seam":id}));
        seams.push(seam);
    }
    if seams.is_empty() {
        missing.push("declared_seam_inventory".into());
    }
    let pieces: Vec<Value> = names.iter().map(|n| panels[n].piece.clone()).collect();
    let mut pattern = json!({"schema":"fabric-pattern/1","units":"mm","style":"GARMENTCODE_IMPORT",
        "meta":{"import":{"schema":"ftx-garmentcode-import/1","status":"REVIEW_REQUIRED",
            "scope":"evaluated_net_seam_lines","source_units_per_meter":units,
            "face_orientation":"Winding normalization does not certify the public/fabric face."}},
        "pieces":pieces,"seams":seams});
    if let Some(source_export) = input.get("fabric_pattern_export") {
        object(source_export, "fabric_pattern_export")?;
        pattern["meta"]["import"]["source_export"] = source_export.clone();
    }
    if let Some(provenance) = annotations.and_then(|a| a.get("provenance")) {
        pattern["meta"]["import"]["provenance"] = provenance.clone();
        pattern["meta"]["import"]["provenance_status"] = json!("caller_supplied_unverified");
    }
    let native = Pattern::new(pattern.clone());
    let errors = native.structure_errors();
    if !errors.is_empty() {
        return Err(invalid(format!("converted pattern: {}", errors.join("; "))));
    }
    let mut report = json!({"schema":"ftx-garmentcode-import/1","status":"REVIEW_REQUIRED",
        "scope":"evaluated_net_seam_lines","source_units_per_meter":units,
        "counts":{"panels":names.len(),"edges":work.edges,"stitches":stitches.len()},
        "panels":mappings,"stitches":stitch_map,
        "geometry_audit":{"method":"sampled_outline","bezier_samples":EDGE_DENSE,"segment_pairs":work.pairs},
        "limitations":["Original cut/fold layout and omitted components are not reconstructed.",
            "Original 3D transforms are metadata, not native body placement.",
            "Winding normalization does not certify the public/fabric face.",
            "Sampled outline checks do not prove analytic Bezier simplicity or garment fit."]});
    if let Some(fit) = annotations.and_then(|a| a.get("fit")) {
        fit_spec(fit, &native)?;
        report["fit_spec"] = fit.clone();
    } else {
        missing.push("fit_spec".into());
    }
    if let Some(provenance) = annotations.and_then(|a| a.get("provenance")) {
        report["provenance"] = provenance.clone();
        report["provenance_status"] = json!("caller_supplied_unverified");
    }
    report["missing_semantics"] = json!(missing);
    Ok(ImportResult { pattern, report })
}

/// Parses bounded source text and binds its exact bytes, including whitespace, to the result.
///
/// # Errors
/// Oversized/invalid JSON or any refusal from import_garmentcode.
pub fn import_garmentcode_json(
    input: &str,
    annotations: Option<&str>,
) -> Result<ImportResult, Refusal> {
    if input.len() > MAX_JSON_BYTES || annotations.is_some_and(|a| a.len() > MAX_JSON_BYTES) {
        return Err(budget("each GarmentCode input is limited to 4 MiB"));
    }
    let source: Value =
        serde_json::from_str(input).map_err(|e| invalid(format!("GarmentCode JSON: {e}")))?;
    let notes: Option<Value> = annotations
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| invalid(format!("annotation JSON: {e}")))?;
    let mut out = import_garmentcode(&source, notes.as_ref())?;
    for (key, digest) in [
        ("source_sha256", Some(sha256_hex(input.as_bytes()))),
        (
            "annotations_sha256",
            annotations.map(|a| sha256_hex(a.as_bytes())),
        ),
    ] {
        if let Some(digest) = digest {
            out.report[key] = json!(digest);
            out.pattern["meta"]["import"][key] = json!(digest);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Measurements, validate};

    fn source() -> Value {
        let panel = json!({"vertices":[[0,0],[10,0],[10,10],[0,10]],"edges":[
            {"endpoints":[0,1]},{"endpoints":[1,2]},{"endpoints":[2,3]},{"endpoints":[3,0]}]});
        json!({"properties":{"units_in_meter":100,"curvature_coords":"relative"},
            "pattern":{"panels":{"front.R":panel.clone(),"back on fold":panel},"panel_order":["front.R","back on fold"],
                "stitches":[[{"panel":"front.R","edge":1},{"panel":"back on fold","edge":3}]]}})
    }

    fn clockwise(v: &mut Value, panel: &str) {
        let edges = v["pattern"]["panels"][panel]["edges"]
            .as_array_mut()
            .unwrap();
        edges.reverse();
        for e in edges {
            e["endpoints"].as_array_mut().unwrap().reverse();
        }
    }

    #[test]
    fn expands_no_instances_and_preserves_unknown_intent() {
        let out = import_garmentcode(&source(), None).unwrap();
        assert_eq!(
            out.pattern["pieces"][0]["vertices"]["v0002"],
            json!([100.0, 100.0])
        );
        assert_eq!(out.pattern["pieces"][0]["cut"]["qty"], 1);
        assert_eq!(Pattern::new(out.pattern.clone()).instances.len(), 2);
        assert_eq!(out.pattern["seams"][0]["reverse"], true);
        assert!(out.pattern["pieces"][0].get("grain").is_none());
        assert!(out.pattern["pieces"][0].get("placement").is_none());
        assert!(out.pattern["seams"][0].get("ratio").is_none());
        assert_eq!(out.report["status"], "REVIEW_REQUIRED");
        assert!(
            out.report["missing_semantics"]
                .as_array()
                .unwrap()
                .contains(&json!("s0000:seam_intent"))
        );
    }

    #[test]
    fn winding_and_right_wrong_preserve_stitch_correspondence() {
        for right_wrong in [false, true] {
            for flips in 0..3 {
                let mut v = source();
                if flips > 0 {
                    clockwise(&mut v, "front.R");
                }
                if flips > 1 {
                    clockwise(&mut v, "back on fold");
                }
                if right_wrong {
                    v["pattern"]["stitches"][0]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("right_wrong"));
                }
                let out = import_garmentcode(&v, None).unwrap();
                assert_eq!(
                    out.pattern["seams"][0]["reverse"],
                    json!(!right_wrong ^ (flips == 1))
                );
                assert!(Pattern::new(out.pattern).structure_errors().is_empty());
            }
        }
    }

    #[test]
    fn cubic_reversal_keeps_asymmetric_shape_and_notches() {
        let mut v = source();
        clockwise(&mut v, "front.R");
        v["pattern"]["panels"]["front.R"]["edges"][0]["curvature"] =
            json!({"type":"cubic","params":[[0.2,0.3],[0.8,-0.1]]});
        let a = json!({"schema":"fabric-garmentcode-annotations/1","panels":{"front.R":{
            "grain":[[50,10],[50,90]],"sa":10,"sim":false,"notches":[{"edge":0,"s":0.25}]}}});
        let out = import_garmentcode(&v, Some(&a)).unwrap();
        let edge = out.pattern["pieces"][0]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == "e0000")
            .unwrap();
        let c: Curve = serde_json::from_value(edge["curve"].clone()).unwrap();
        assert!((c.controls()[0][0] - 0.2).abs() < 1e-14);
        assert_eq!(c.controls()[0][1], 0.1);
        assert_eq!(c.controls()[1], [0.8, -0.3]);
        assert_eq!(out.pattern["pieces"][0]["notches"][0]["s"], 0.75);
    }

    #[test]
    fn quadratic_controls_are_relative_to_the_unnormalized_chord() {
        let mut v = source();
        v["pattern"]["panels"]["front.R"]["vertices"] = json!([[1, 2], [4, 6], [-3, 6], [-3, 2]]);
        for params in [json!([[0.25, 0.5]]), json!([0.25, 0.5])] {
            v["pattern"]["panels"]["front.R"]["edges"][0]["curvature"] =
                json!({"type":"quadratic","params":params});
            let out = import_garmentcode(&v, None).unwrap();
            let c: Curve =
                serde_json::from_value(out.pattern["pieces"][0]["edges"][0]["curve"].clone())
                    .unwrap();
            assert_eq!(
                rel_to_abs(
                    Vec2::new(10.0, 20.0),
                    Vec2::new(40.0, 60.0),
                    c.controls()[0]
                ),
                Vec2::new(-2.5, 45.0)
            );
        }
    }

    #[test]
    fn unequal_seam_lengths_are_not_rewritten_into_ease() {
        let mut v = source();
        v["pattern"]["panels"]["back on fold"]["vertices"] =
            json!([[0, 0], [10, 0], [10, 20], [0, 20]]);
        let out = import_garmentcode(&v, None).unwrap();
        let report = validate(
            &Pattern::new(out.pattern),
            &json!({"points":[]}),
            &Measurements(json!({})),
        );
        let r = serde_json::to_value(report).unwrap();
        assert_eq!(r["seams"][0]["status"], "FAIL");
    }

    #[test]
    fn own_panel_seam_is_valid_but_reusing_an_edge_refuses() {
        let mut v = source();
        v["pattern"]["stitches"][0][1] = json!({"panel":"front.R","edge":3});
        import_garmentcode(&v, None).unwrap();
        v["pattern"]["stitches"][0][1]["edge"] = json!(1);
        assert!(import_garmentcode(&v, None).is_err());
    }

    #[test]
    fn expanded_limb_placement_never_silently_uses_the_right_frame() {
        for part in ["arm", "leg"] {
            let a = json!({"schema":"fabric-garmentcode-annotations/1",
                "panels":{"front.R":{"placement":{"part":part},"sim":true}}});
            let refusal = import_garmentcode(&source(), Some(&a)).unwrap_err();
            assert_eq!(refusal.code, RefusalCode::InvalidDocument);
            assert!(refusal.message.contains("physical limb-side"));
        }
        let a = json!({"schema":"fabric-garmentcode-annotations/1",
            "panels":{"front.R":{"placement":{"part":"torso"},"sim":true}}});
        let imported = import_garmentcode(&source(), Some(&a)).unwrap();
        assert_eq!(imported.pattern["pieces"][0]["placement"]["part"], "torso");
        assert_eq!(imported.report["status"], "REVIEW_REQUIRED");
    }

    #[test]
    fn malformed_inputs_refuse_instead_of_dropping_information() {
        let original = source();
        for (pointer, replacement) in [
            ("/properties/units_in_meter", json!(0)),
            ("/pattern/panels/front.R/edges/0/endpoints", json!([0, 0])),
            (
                "/pattern/panels/front.R/edges/0/curvature",
                json!({"type":"circle","params":[1,1]}),
            ),
            ("/pattern/panels/front.R/edges/0/endpoints", json!([0, 99])),
            ("/pattern/panel_order", json!(["front.R", "front.R"])),
            ("/parameters", json!({"waist":1})),
            (
                "/pattern/panels/front.R/vertices",
                json!([[0, 0], [10, 10], [0, 10], [10, 0]]),
            ),
        ] {
            let mut v = original.clone();
            if pointer == "/parameters" {
                v["parameters"] = replacement;
            } else if pointer.ends_with("/curvature") {
                v["pattern"]["panels"]["front.R"]["edges"][0]["curvature"] = replacement;
            } else {
                *v.pointer_mut(pointer).unwrap() = replacement;
            }
            assert!(import_garmentcode(&v, None).is_err(), "{pointer}");
        }
        let a = json!({"schema":"fabric-garmentcode-annotations/1","panels":{"front.R":{"cut":{"qty":2}}}});
        assert!(import_garmentcode(&original, Some(&a)).is_err());
    }

    #[test]
    fn exact_text_identity_and_annotated_fit_are_retained() {
        let input = source().to_string();
        let a = json!({"schema":"fabric-garmentcode-annotations/1","stitches":{"0":{"ratio":1,"tol_mm":2}},
            "fit":{"schema":"fabric-fit/1","points":[{"id":"height","kind":"vertical","instance":"p0000",
                "from":"v0000","to":"v0003","body":"stature","target_mm":[-1000,0,1000]}]}}).to_string();
        let out = import_garmentcode_json(&input, Some(&a)).unwrap();
        assert_eq!(out.report["source_sha256"], sha256_hex(input.as_bytes()));
        assert_eq!(
            out.pattern["meta"]["import"]["annotations_sha256"],
            sha256_hex(a.as_bytes())
        );
        assert_eq!(out.report["fit_spec"]["schema"], "fabric-fit/1");
        assert_eq!(out.report["status"], "REVIEW_REQUIRED");
        let other = import_garmentcode_json(&(input + "\n"), Some(&a)).unwrap();
        assert_ne!(out.report["source_sha256"], other.report["source_sha256"]);
        assert_eq!(out.pattern["pieces"], other.pattern["pieces"]);
        assert!(import_garmentcode_json(&" ".repeat(MAX_JSON_BYTES + 1), None).is_err());
    }

    #[test]
    fn fit_requires_real_nonempty_references_and_distinct_point_ids() {
        let valid = json!({"id":"cuff","kind":"path","refs":["p0000/e0000"],
            "body":"wrist","target_mm":[0,0,0]});
        let notes = json!({"schema":"fabric-garmentcode-annotations/1",
            "fit":{"schema":"fabric-fit/1","points":[valid.clone()],
                "truing":[{"seam":"s0000","end":"start"}]}});
        import_garmentcode(&source(), Some(&notes)).unwrap();
        for bad in [
            json!({"kind":"path","refs":[]}),
            json!({"kind":"path","refs":["p0000/absent"]}),
            json!({"kind":"girth","instances":[],"landmark":"chest"}),
            json!({"kind":"girth","instances":["absent"],"landmark":"chest"}),
            json!({"kind":"girth","instances":["p0000"]}),
            json!({"kind":"width","instance":"p0000"}),
            json!({"kind":"width","instance":"p0000","at_vertex":"absent"}),
            json!({"kind":"vertical","instance":"p0000","from":"absent","to":"v0001"}),
            json!({"kind":"vertical","instance":"p0000","from":"v0000","to":"v0000"}),
            json!({"scale":0}),
            json!({"body_scale":-1}),
            json!({"id":""}),
        ] {
            let mut a = notes.clone();
            for (key, value) in bad.as_object().unwrap() {
                a["fit"]["points"][0][key] = value.clone();
            }
            assert!(import_garmentcode(&source(), Some(&a)).is_err(), "{bad}");
        }
        let mut duplicate = notes.clone();
        duplicate["fit"]["points"]
            .as_array_mut()
            .unwrap()
            .push(valid);
        assert!(import_garmentcode(&source(), Some(&duplicate)).is_err());
        for truing in [
            json!([{"seam":"absent","end":"start"}]),
            json!([{"seam":"s0000","end":"middle"}]),
        ] {
            let mut a = notes.clone();
            a["fit"]["truing"] = truing;
            assert!(import_garmentcode(&source(), Some(&a)).is_err());
        }
    }

    #[test]
    fn attribution_and_source_export_notes_are_retained_without_promotion() {
        let mut v = source();
        let export = json!({"edge_map":{"front.R":["armhole"]},"warnings":["shell only"]});
        v["fabric_pattern_export"] = export.clone();
        let provenance = json!({"source":"synthetic manual","page":12,
            "checks":[{"label":"side","expected_mm":100,"actual_mm":100}]});
        let a = json!({"schema":"fabric-garmentcode-annotations/1","provenance":provenance});
        let out = import_garmentcode(&v, Some(&a)).unwrap();
        assert_eq!(out.pattern["meta"]["import"]["source_export"], export);
        assert_eq!(out.pattern["meta"]["import"]["provenance"], a["provenance"]);
        assert_eq!(out.report["provenance"], a["provenance"]);
        assert_eq!(
            out.report["provenance_status"],
            "caller_supplied_unverified"
        );
        assert_eq!(out.pattern["meta"]["import"]["status"], "REVIEW_REQUIRED");
    }
}
