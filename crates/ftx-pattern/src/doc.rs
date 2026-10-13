//! The `fabric-pattern/1` document as typed values (`docs/spec/01` §2).
//!
//! Every object keeps the keys this crate does not interpret in `extra`, so reading and writing a
//! document loses nothing: the result is semantically equal JSON (spec 01 §2.10, §14.6). Key order of
//! known fields is canonical; the vertex map keeps document order. A typed document is only built after
//! the structure checks of [`crate::pattern`] pass on the raw JSON, so malformed input is reported with
//! the reference's messages rather than as a parse error.

use ftx_core::{Curve, Vec2};
use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// The schema tag.
pub const SCHEMA: &str = "fabric-pattern/1";

/// Python truthiness of a JSON value (`null`, `false`, `0`, `""`, `[]`, `{}` are false).
#[must_use]
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// A pattern document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternDoc {
    /// Must be `fabric-pattern/1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Must be `mm` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// Style tag (export file stem and labels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Provenance and drafting metadata; alteration appends `history` and sets `origin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Map<String, Value>>,
    /// Fabric description (read by the drape and the exports).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fabric: Option<Value>,
    /// Pieces, each drafted as the wearer's right side seen from the public side.
    pub pieces: Vec<Piece>,
    /// Seams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seams: Option<Vec<Seam>>,
    /// Other keys, preserved.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl PatternDoc {
    /// The seams (empty when absent).
    #[must_use]
    pub fn seams(&self) -> &[Seam] {
        self.seams.as_deref().unwrap_or(&[])
    }

    /// The piece with this id.
    #[must_use]
    pub fn piece(&self, id: &str) -> Option<&Piece> {
        self.pieces.iter().find(|p| p.id == id)
    }
}

/// A piece.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Piece {
    /// Unique among pieces.
    pub id: String,
    /// Label text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Informational material id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Cut instruction; absent (or `null`) means one piece.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<Cut>,
    /// Vertex coordinates in document order.
    pub vertices: Vertices,
    /// The closed counter-clockwise edge loop.
    pub edges: Vec<Edge>,
    /// Notches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notches: Option<Vec<Notch>>,
    /// Grainline: `[[x0, y0], [x1, y1]]` or the legacy `{"p0", "p1"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<Grain>,
    /// Drawn or cut internal lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal: Option<Vec<Mark>>,
    /// Braid and embroidery lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decor: Option<Vec<Mark>>,
    /// Drill holes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drills: Option<Vec<Drill>>,
    /// The tracer's unverified detections (never exported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_marks: Option<Vec<Mark>>,
    /// Piece default seam allowance (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sa: Option<f64>,
    /// `false`: exported but not folded up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sim: Option<bool>,
    /// Finished size factors in the fold-up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sim_scale: Option<[f64; 2]>,
    /// Making-up instruction printed on the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Role (`bodice_front`, `sleeve`, `pant_back`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Where the piece sits on the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
    /// Other keys (tracer extras, …), preserved.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Piece {
    /// Number of instances cut: 2 for a mirror pair, else 1.
    #[must_use]
    pub fn qty(&self) -> u8 {
        match self
            .cut
            .as_ref()
            .and_then(|c| c.qty.as_ref())
            .and_then(Value::as_f64)
        {
            Some(2.0) => 2,
            _ => 1,
        }
    }

    /// The fold edge, if the piece is cut on the fold.
    #[must_use]
    pub fn on_fold(&self) -> Option<&str> {
        self.cut
            .as_ref()
            .and_then(|c| c.on_fold.as_deref())
            .filter(|f| !f.is_empty())
    }

    /// The notches (empty when absent).
    #[must_use]
    pub fn notches(&self) -> &[Notch] {
        self.notches.as_deref().unwrap_or(&[])
    }

    /// The role (empty when absent).
    #[must_use]
    pub fn role(&self) -> &str {
        self.role.as_deref().unwrap_or("")
    }
}

/// Cut instruction.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cut {
    /// 1 or 2 (kept as JSON so `2` stays an integer on the way out).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qty: Option<Value>,
    /// Edge id the piece is unfolded across.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_fold: Option<String>,
    /// Material label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Other keys, preserved.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// An edge of a piece's loop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// Unique within the piece.
    pub id: String,
    /// Start vertex.
    pub from: String,
    /// End vertex.
    pub to: String,
    /// Curve relative to the chord; absent (or `null`) is straight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
    /// Per-edge seam allowance (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sa: Option<f64>,
    /// Other keys, copied into every instance edge.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A notch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Notch {
    /// Name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Edge id as drafted (no `.R`/`.L`).
    pub edge: String,
    /// Arclength fraction along the edge as drafted.
    pub s: f64,
    /// Rendering type (`V`, `double`, …).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Other keys (`depth`, `width`, …), preserved.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A grainline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Grain {
    /// `[[x0, y0], [x1, y1]]` (canonical; any length is read, two points are meaningful).
    Points(Vec<Vec2>),
    /// `{"p0": [..], "p1": [..]}` (legacy, read only).
    Legacy {
        /// Start.
        p0: Vec2,
        /// End.
        p1: Vec2,
    },
}

impl Grain {
    /// The two end points, if the grainline has them.
    #[must_use]
    pub fn points(&self) -> Option<[Vec2; 2]> {
        match self {
            Self::Points(p) if p.len() == 2 => Some([p[0], p[1]]),
            Self::Points(_) => None,
            Self::Legacy { p0, p1 } => Some([*p0, *p1]),
        }
    }
}

/// An internal line, decoration or trace mark.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    /// `draw`, `cut`, `soutache`, `embroidery`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Points (mm).
    #[serde(default)]
    pub pts: Vec<Vec2>,
    /// Other keys (`label`, `closed`, `width_mm`, …), preserved.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A drill hole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drill {
    /// Centre (mm).
    pub p: Vec2,
    /// Other keys (`d`, …), preserved.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Placement on the body.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// `torso`, `arm` or `leg`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    /// The landmark that piece `y0` sits on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y0_landmark: Option<String>,
    /// Piece y of that landmark.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y0: Option<f64>,
    /// Other keys (sim placement), preserved.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A seam, kept as its JSON object: validation reads it with the reference's dictionary semantics
/// (an absent `ratio_band` differs from a `null` one, `reverse: null` is false), and it round-trips
/// exactly.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Seam(pub Map<String, Value>);

impl Seam {
    /// The id, if it is a string.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.0.get("id").and_then(Value::as_str)
    }

    /// The id as the reference prints it (`None` when missing).
    #[must_use]
    pub fn id_text(&self) -> String {
        match self.0.get("id") {
            None | Some(Value::Null) => "None".to_string(),
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
        }
    }

    /// Side `a` or `b` (`key` is `"a"` or `"b"`), if it is a string.
    #[must_use]
    pub fn side_ref(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(Value::as_str)
    }

    /// `reverse` with Python semantics: absent → true, otherwise its truthiness.
    #[must_use]
    pub fn reverse(&self) -> bool {
        self.0.get("reverse").is_none_or(truthy)
    }
}

/// Vertex coordinates in document order. Reading a duplicate key keeps the first position and the
/// last value, like a Python dict.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vertices(pub Vec<(String, Vec2)>);

impl Vertices {
    /// The point of a vertex.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Vec2> {
        self.0.iter().find(|(k, _)| k == id).map(|(_, p)| *p)
    }

    /// Whether the vertex exists.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.0.iter().any(|(k, _)| k == id)
    }

    /// Sets a vertex, keeping its position if it exists, else appending it.
    pub fn insert(&mut self, id: &str, p: Vec2) {
        match self.0.iter_mut().find(|(k, _)| k == id) {
            Some((_, q)) => *q = p,
            None => self.0.push((id.to_string(), p)),
        }
    }

    /// Ids and points in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, Vec2)> {
        self.0.iter().map(|(k, p)| (k.as_str(), *p))
    }
}

impl Serialize for Vertices {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, p) in &self.0 {
            m.serialize_entry(k, p)?;
        }
        m.end()
    }
}

impl<'de> Deserialize<'de> for Vertices {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Vertices;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object of vertex id → [x, y]")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Vertices, A::Error> {
                let mut out = Vertices::default();
                while let Some((k, p)) = a.next_entry::<String, Vec2>()? {
                    out.insert(&k, p);
                }
                Ok(out)
            }
        }
        d.deserialize_map(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip_keeps_unknown_keys_and_vertex_order() {
        let v = json!({
            "schema": "fabric-pattern/1", "units": "mm", "meta": {"round": 3, "history": []}, "custom": [1, 2],
            "pieces": [{
                // Typed numbers are written back as floats, so they are floats here; integers in
                // untyped places (extras, meta, qty) stay integers.
                "id": "p", "vertices": {"z": [0.0, 0.0], "a": [10.0, 0.0], "m": [10.0, 10.0]},
                "edges": [{"id": "e1", "from": "z", "to": "a", "sa": 0.0, "tag": "x"},
                          {"id": "e2", "from": "a", "to": "m", "curve": {"type": "quad", "c": [[0.5, 0.2]], "k": 1}},
                          {"id": "e3", "from": "m", "to": "z"}],
                "cut": {"qty": 2, "material": "wool"}, "grain": {"p0": [1.0, 1.0], "p1": [1.0, 5.0]},
                "notches": [{"edge": "e1", "s": 0.5, "type": "V", "depth": 4}],
                "decor": [{"kind": "soutache", "pts": [[1.0, 1.0], [2.0, 2.0]], "width_mm": 3}],
                "drills": [{"p": [3.0, 3.0], "d": 3}], "trace": {"anything": true}
            }],
            "seams": [{"id": "s", "a": "p.R/e1", "b": "p.L/e1", "ratio_band": null, "reverse": null}]
        });
        // Parsed from text, the vertex map keeps document order (a `serde_json::Value` sorts keys).
        let doc: PatternDoc = serde_json::from_str(&serde_json::to_string(&v).unwrap().replace(
            r#""a":[10.0,0.0],"m":[10.0,10.0],"z":[0.0,0.0]"#,
            r#""z":[0.0,0.0],"a":[10.0,0.0],"m":[10.0,10.0]"#,
        ))
        .unwrap();
        let ids: Vec<&str> = doc.pieces[0].vertices.iter().map(|(k, _)| k).collect();
        assert_eq!(ids, ["z", "a", "m"]);
        assert_eq!(doc.pieces[0].qty(), 2);
        assert!(!doc.seams()[0].reverse(), "reverse: null is falsy");
        assert_eq!(
            doc.pieces[0].grain.as_ref().unwrap().points().unwrap()[1],
            Vec2::new(1.0, 5.0)
        );
        let back = serde_json::to_value(&doc).unwrap();
        assert_eq!(back, v, "semantic round trip");
    }

    #[test]
    fn duplicate_vertex_keys_keep_the_last_value() {
        let v: Vertices =
            serde_json::from_str(r#"{"a": [0, 0], "b": [1, 1], "a": [5, 5]}"#).unwrap();
        assert_eq!(
            v.0,
            vec![
                ("a".into(), Vec2::new(5.0, 5.0)),
                ("b".into(), Vec2::new(1.0, 1.0))
            ]
        );
    }

    #[test]
    fn truthiness() {
        for (v, t) in [
            (json!(null), false),
            (json!(0), false),
            (json!(""), false),
            (json!([]), false),
            (json!(2), true),
            (json!("x"), true),
        ] {
            assert_eq!(truthy(&v), t, "{v}");
        }
    }
}
