//! What every drafting program shares (spec 02 §C.0): the piece builder, bisection, Bézier lengths, the style
//! merge, mirrored seams and the size label.

use ftx_core::curve::{abs_to_rel, bezier};
use ftx_core::polyline::polyline_length;
use ftx_core::py;
use ftx_core::{Refusal, RefusalCode, Vec2};
use serde_json::{Map, Value, json};

use super::form::Meas;
pub use crate::SCHEMA;

/// A JSON number for an f64 (the reference writes floats).
#[must_use]
pub fn num(x: f64) -> Value {
    json!(x)
}

/// A point as a JSON pair.
#[must_use]
pub fn pt(p: Vec2) -> Value {
    json!([p.x, p.y])
}

/// `PieceBuilder`: vertices (rounded to 0.001 mm) and edges in counter-clockwise order.
pub struct PieceBuilder {
    /// The piece object.
    pub p: Map<String, Value>,
    verts: Vec<(String, Vec2)>,
}

impl PieceBuilder {
    /// `PieceBuilder(pid, name, **meta)`.
    #[must_use]
    pub fn new(id: &str, name: &str, meta: Value) -> Self {
        let mut p = Map::new();
        p.insert("id".into(), json!(id));
        p.insert("name".into(), json!(name));
        p.insert("vertices".into(), json!({}));
        p.insert("edges".into(), json!([]));
        p.insert("notches".into(), json!([]));
        if let Value::Object(m) = meta {
            p.extend(m);
        }
        Self {
            p,
            verts: Vec::new(),
        }
    }

    /// `v(vid, x, y)`.
    pub fn v(&mut self, id: &str, x: f64, y: f64) {
        let q = Vec2::new(py::round(x, 3), py::round(y, 3));
        if let Some(e) = self.verts.iter_mut().find(|(k, _)| k == id) {
            e.1 = q;
        } else {
            self.verts.push((id.to_string(), q));
        }
        self.p["vertices"][id] = pt(q);
    }

    /// A vertex as stored (rounded).
    #[must_use]
    pub fn vertex(&self, id: &str) -> Vec2 {
        self.verts
            .iter()
            .find(|(k, _)| k == id)
            .map_or(Vec2::new(f64::NAN, f64::NAN), |(_, p)| *p)
    }

    /// `e(eid, a, b, curve=None, **meta)`.
    pub fn e(&mut self, id: &str, a: &str, b: &str, curve: Option<Value>, meta: Option<Value>) {
        let mut edge = Map::new();
        edge.insert("id".into(), json!(id));
        edge.insert("from".into(), json!(a));
        edge.insert("to".into(), json!(b));
        if let Some(Value::Object(m)) = meta {
            edge.extend(m);
        }
        if let Some(c) = curve {
            edge.insert("curve".into(), c);
        }
        if let Some(Value::Array(edges)) = self.p.get_mut("edges") {
            edges.push(Value::Object(edge));
        }
    }

    /// A straight edge.
    pub fn line(&mut self, id: &str, a: &str, b: &str) {
        self.e(id, a, b, None, None);
    }

    /// `cubic_abs(eid, a, b, c1, c2, **meta)`: controls given absolute, stored relative to the chord (0.00001).
    ///
    /// # Errors
    /// `degenerate_geometry` for an edge whose ends coincide.
    pub fn cubic_abs(
        &mut self,
        id: &str,
        a: &str,
        b: &str,
        c1: Vec2,
        c2: Vec2,
        meta: Option<Value>,
    ) -> Result<(), Refusal> {
        let (pa, pb) = (self.vertex(a), self.vertex(b));
        let rel = |c: Vec2| -> Result<Value, Refusal> {
            let th = abs_to_rel(pa, pb, c).map_err(|e| {
                Refusal::new(RefusalCode::DegenerateGeometry, format!("edge {id}: {e}"))
            })?;
            Ok(json!([py::round(th[0], 5), py::round(th[1], 5)]))
        };
        let curve = json!({"type": "cubic", "c": [rel(c1)?, rel(c2)?]});
        self.e(id, a, b, Some(curve), meta);
        Ok(())
    }

    /// `notch(nid, edge, s, kind)`.
    pub fn notch(&mut self, id: &str, edge: &str, s: f64, kind: &str) {
        if let Some(Value::Array(n)) = self.p.get_mut("notches") {
            n.push(json!({"id": id, "edge": edge, "s": py::round(s, 4), "type": kind}));
        }
    }

    /// Sets a top-level key of the piece.
    pub fn set(&mut self, key: &str, v: Value) {
        self.p.insert(key.into(), v);
    }

    /// `build()`.
    #[must_use]
    pub fn build(&self) -> Value {
        Value::Object(self.p.clone())
    }
}

/// `_bisect(f, lo, hi, it=60)`.
///
/// # Errors
/// As `f`.
pub fn bisect(
    mut f: impl FnMut(f64) -> Result<f64, Refusal>,
    lo: f64,
    hi: f64,
) -> Result<f64, Refusal> {
    let (mut lo, mut hi) = (lo, hi);
    let mut flo = f(lo)?;
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        let fm = f(mid)?;
        if (fm > 0.0) == (flo > 0.0) {
            (lo, flo) = (mid, fm);
        } else {
            hi = mid;
        }
    }
    Ok(0.5 * (lo + hi))
}

/// `_unit(v)`.
#[must_use]
pub fn unit(v: Vec2) -> Vec2 {
    let n = (v.x * v.x + v.y * v.y).sqrt();
    Vec2::new(v.x / n, v.y / n)
}

/// `_len_cubic(p0, c1, c2, p1)`: the length of the 400-sample polyline.
#[must_use]
pub fn len_cubic(c: [Vec2; 4]) -> f64 {
    polyline_length(&bezier(&c, 400))
}

/// `_style(defaults, style)`: the defaults overridden by `style`, refusing keys the program does not take.
///
/// # Errors
/// `unknown_parameter` with the reference's message.
pub fn merge_style(defaults: &Value, style: Option<&Value>) -> Result<Map<String, Value>, Refusal> {
    let Value::Object(d) = defaults else {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "style defaults must be an object",
        ));
    };
    let mut out = d.clone();
    match style {
        None | Some(Value::Null) => {}
        Some(Value::Object(s)) => {
            let mut unknown: Vec<&String> = s.keys().filter(|k| !d.contains_key(*k)).collect();
            if !unknown.is_empty() {
                unknown.sort();
                let mut known: Vec<&String> = d.keys().collect();
                known.sort();
                return Err(Refusal::new(
                    RefusalCode::UnknownParameter,
                    format!(
                        "unknown style key(s) {}; this program takes {}",
                        py::repr_str_list(&unknown),
                        py::repr_str_list(&known)
                    ),
                ));
            }
            for (k, v) in s {
                out.insert(k.clone(), v.clone());
            }
        }
        Some(_) => {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "a style must be a JSON object",
            ));
        }
    }
    Ok(out)
}

/// A style number.
///
/// # Errors
/// `invalid_document` when it is not a number.
pub fn sf(s: &Map<String, Value>, key: &str) -> Result<f64, Refusal> {
    s.get(key).and_then(Value::as_f64).ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            format!("style {key} must be a number"),
        )
    })
}

/// `_mirror_seams(seams)`: each seam as `<id>_R`, and its wearer's-left copy `<id>_L` when it touches `.R`.
#[must_use]
pub fn mirror_seams(seams: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for sm in seams {
        let mut r = Map::new();
        r.insert("reverse".into(), json!(true));
        if let Value::Object(m) = sm {
            r.extend(m.clone());
        }
        let id = sm["id"].as_str().unwrap_or_default();
        r.insert("id".into(), json!(format!("{id}_R")));
        let (a, b) = (
            sm["a"].as_str().unwrap_or_default(),
            sm["b"].as_str().unwrap_or_default(),
        );
        let touches = a.contains(".R") || b.contains(".R");
        out.push(Value::Object(r.clone()));
        if touches {
            let mut l = r;
            l.insert("id".into(), json!(format!("{id}_L")));
            l.insert("a".into(), json!(a.replace(".R", ".L")));
            l.insert("b".into(), json!(b.replace(".R", ".L")));
            out.push(Value::Object(l));
        }
    }
    out
}

/// `_size_label(meas)`: `<stature:.0f> mm stature`, or `custom`.
#[must_use]
pub fn size_label(m: Meas) -> String {
    m.get("stature").map_or_else(
        |_| "custom".into(),
        |s| format!("{} mm stature", py::fixed(s, 0)),
    )
}
