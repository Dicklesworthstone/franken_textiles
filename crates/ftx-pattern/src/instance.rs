//! Instances: what is physically cut (`docs/spec/01` §6). A `qty: 2` piece gives `.R` (as drafted) and
//! `.L` (mirrored about x = 0, loop reversed so it stays counter-clockwise); an `on_fold` piece is unfolded
//! across its fold edge into one instance whose drafted-half edges are `e.R` and mirrored-half edges `e.L`.

use std::fmt;

use ftx_core::curve::{
    CurveKind, EDGE_DENSE, OUTLINE_DENSE, curve_points, mirror_curve, rel_to_abs,
};
use ftx_core::polyline::{polyline_length, signed_area, tangent_at};
use ftx_core::{Curve, End, Vec2, det, py};

use crate::doc::{Piece, Vertices, truthy};
use serde_json::Value;

/// An error with the reference's Python exception text: `str(KeyError(m))` is `repr(m)`, so key
/// errors print quoted; value errors print as they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PyError {
    /// A missing key (instance, edge, vertex).
    Key(String),
    /// Anything else.
    Value(String),
}

impl PyError {
    /// The unquoted message.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Key(m) | Self::Value(m) => m,
        }
    }
}

impl fmt::Display for PyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(m) => f.write_str(&py::repr_str(m)),
            Self::Value(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for PyError {}

/// Which physical piece an instance is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The drafted (wearer's right) member of a mirror pair.
    R,
    /// The mirrored member of a mirror pair.
    L,
    /// A single, centre or unfolded piece.
    C,
}

impl Side {
    /// `"R"`, `"L"` or `"C"`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::R => "R",
            Self::L => "L",
            Self::C => "C",
        }
    }
}

/// Explicit body side for a single physical limb panel, independent of instance mirroring.
/// An absent side keeps the reference's generated-pair placement convention.
///
/// # Errors
/// A side other than `R`/`L`, a non-limb part, a cut quantity other than one, or a fold.
pub fn physical_side(piece: &Value) -> Result<Option<Side>, PyError> {
    let placement = &piece["placement"];
    let Some(side) = placement.get("side") else {
        return Ok(None);
    };
    let id = piece["id"].as_str().unwrap_or("?");
    let invalid = |message: &str| PyError::Value(format!("{id}: {message}"));
    let side = match side.as_str() {
        Some("R") => Side::R,
        Some("L") => Side::L,
        _ => return Err(invalid("placement.side must be R or L")),
    };
    if !matches!(placement["part"].as_str(), Some("arm" | "leg")) {
        return Err(invalid("placement.side requires part arm or leg"));
    }
    let cut = &piece["cut"];
    if cut.get("qty").is_some_and(|q| q.as_f64() != Some(1.0)) || truthy(&cut["on_fold"]) {
        return Err(invalid(
            "placement.side requires a single physical panel (cut.qty 1, no on_fold)",
        ));
    }
    Ok(Some(side))
}

/// An edge of an instance's loop.
#[derive(Clone, Debug, PartialEq)]
pub struct InstEdge {
    /// Id (with `.R`/`.L` on the halves of an unfolded piece).
    pub id: String,
    /// Start vertex (in the instance's vertex map).
    pub from: String,
    /// End vertex.
    pub to: String,
    /// Curve relative to this edge's chord.
    pub curve: Option<Curve>,
    /// Index of the piece edge it comes from (for `sa` and other per-edge keys).
    pub source: usize,
}

/// One physical piece of fabric.
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    /// `pid`, `pid.R` or `pid.L`.
    pub id: String,
    /// Piece id.
    pub piece: String,
    /// Index of the piece in the document.
    pub piece_index: usize,
    /// R, L or C.
    pub side: Side,
    /// True only for the `.L` member of a mirror pair.
    pub mirrored: bool,
    /// Vertex points in this instance's frame.
    pub verts: Vertices,
    /// Counter-clockwise loop.
    pub edges: Vec<InstEdge>,
}

impl Instance {
    /// The first edge with this id.
    ///
    /// # Errors
    /// `KeyError: "{id}: no edge '{eid}' (have [...])"`.
    pub fn edge(&self, eid: &str) -> Result<&InstEdge, PyError> {
        self.edges.iter().find(|e| e.id == eid).ok_or_else(|| {
            let ids: Vec<&str> = self.edges.iter().map(|e| e.id.as_str()).collect();
            PyError::Key(format!(
                "{}: no edge {} (have {})",
                self.id,
                py::repr_str(eid),
                py::repr_str_list(&ids)
            ))
        })
    }

    fn point(&self, vid: &str) -> Result<Vec2, PyError> {
        self.verts
            .get(vid)
            .ok_or_else(|| PyError::Key(vid.to_string()))
    }

    /// The edge's polyline at `dense` samples per curved edge.
    ///
    /// # Errors
    /// Unknown edge or vertex; a malformed curve.
    pub fn edge_dense(&self, eid: &str, dense: usize) -> Result<Vec<Vec2>, PyError> {
        let e = self.edge(eid)?;
        curve_points(
            self.point(&e.from)?,
            self.point(&e.to)?,
            e.curve.as_ref(),
            dense,
        )
        .map_err(|err| PyError::Value(err.to_string()))
    }

    /// Length of the edge's 200-sample polyline.
    ///
    /// # Errors
    /// As [`Instance::edge_dense`].
    pub fn edge_length(&self, eid: &str) -> Result<f64, PyError> {
        Ok(polyline_length(&self.edge_dense(eid, EDGE_DENSE)?))
    }

    /// The closed outline (open ring, no repeated first point) at `dense` samples per curved edge.
    ///
    /// # Errors
    /// As [`Instance::edge_dense`].
    pub fn outline(&self, dense: usize) -> Result<Vec<Vec2>, PyError> {
        let mut out = Vec::new();
        for e in &self.edges {
            let pts = self.edge_dense(&e.id, dense)?;
            out.extend_from_slice(&pts[..pts.len() - 1]);
        }
        Ok(out)
    }

    /// Signed area of the 60-sample outline (positive when counter-clockwise).
    ///
    /// # Errors
    /// As [`Instance::edge_dense`].
    pub fn area(&self) -> Result<f64, PyError> {
        Ok(signed_area(&self.outline(OUTLINE_DENSE)?))
    }

    /// Sum of the edge lengths.
    ///
    /// # Errors
    /// As [`Instance::edge_dense`].
    pub fn perimeter(&self) -> Result<f64, PyError> {
        let mut total = 0.0;
        for e in &self.edges {
            total += self.edge_length(&e.id)?;
        }
        Ok(total)
    }

    /// Unit direction of travel at an edge end: exact from the control polygon for line, quad and cubic
    /// edges (first control point more than 1e-9 mm away), the least-squares fit for `poly` edges.
    ///
    /// # Errors
    /// As [`Instance::edge_dense`].
    pub fn edge_tangent(&self, eid: &str, end: End) -> Result<Vec2, PyError> {
        let e = self.edge(eid)?;
        let (p0, p1) = (self.point(&e.from)?, self.point(&e.to)?);
        let kind = e
            .curve
            .as_ref()
            .map_or(Ok(CurveKind::Line), Curve::kind)
            .map_err(|x| PyError::Value(x.to_string()))?;
        if kind == CurveKind::Poly {
            return Ok(tangent_at(&self.edge_dense(eid, EDGE_DENSE)?, end));
        }
        let mut ctrl = vec![p0];
        if matches!(kind, CurveKind::Quad | CurveKind::Cubic) {
            ctrl.extend(
                e.curve
                    .as_ref()
                    .map_or(&[][..], Curve::controls)
                    .iter()
                    .map(|&th| rel_to_abs(p0, p1, th)),
            );
        }
        ctrl.push(p1);
        if end == End::End {
            ctrl.reverse();
        }
        for &q in &ctrl[1..] {
            let d = if end == End::Start {
                q - ctrl[0]
            } else {
                ctrl[0] - q
            };
            let n = d.norm();
            if n > 1e-9 {
                return Ok(d / n);
            }
        }
        Ok(Vec2::new(1.0, 0.0))
    }

    /// Interior angle (degrees, in [0, 360)) at a loop vertex, between the first edge that ends there
    /// and the next edge.
    ///
    /// # Errors
    /// `KeyError(vid)` when no edge ends at the vertex.
    pub fn corner_angle(&self, vid: &str) -> Result<f64, PyError> {
        let n = self.edges.len();
        for (i, e) in self.edges.iter().enumerate() {
            if e.to == vid {
                let nxt = &self.edges[(i + 1) % n];
                let t_in = self.edge_tangent(&e.id, End::End)?;
                let t_out = self.edge_tangent(&nxt.id, End::Start)?;
                let turn = det::atan2(t_in.x * t_out.y - t_in.y * t_out.x, t_in.dot(t_out))
                    / (std::f64::consts::PI / 180.0);
                return Ok(180.0 - turn);
            }
        }
        Err(PyError::Key(vid.to_string()))
    }
}

/// Mirror and reverse a loop so it stays counter-clockwise; `vmap` renames vertices.
fn mirrored_loop(edges: &[InstEdge], vmap: impl Fn(&str) -> String) -> Vec<InstEdge> {
    edges
        .iter()
        .rev()
        .map(|e| InstEdge {
            id: e.id.clone(),
            from: vmap(&e.to),
            to: vmap(&e.from),
            curve: mirror_curve(e.curve.as_ref()),
            source: e.source,
        })
        .collect()
}

/// Reflection of `p` across the line through `a` and `b`, in the reference's evaluation order.
fn reflect_across(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let ab = b - a;
    let d = ab / ab.norm();
    let w = p - a;
    let k = 2.0 * w.dot(d);
    Vec2::new((a.x + k * d.x) - w.x, (a.y + k * d.y) - w.y)
}

/// The instances of a piece (spec 01 §6.3), in order `[R, L]` or `[base]`.
///
/// # Errors
/// A fold edge that does not exist or has unknown endpoints (structure checks rule both out first).
pub fn piece_instances(piece: &Piece, piece_index: usize) -> Result<Vec<Instance>, PyError> {
    let pid = piece.id.clone();
    let edges: Vec<InstEdge> = piece
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
        .collect();
    let base = match piece.on_fold() {
        Some(fold) => {
            let i = edges.iter().position(|e| e.id == fold).ok_or_else(|| {
                PyError::Value(format!(
                    "{pid}: on_fold edge {} missing",
                    py::repr_str(fold)
                ))
            })?;
            let (a, b) = (edges[i].from.clone(), edges[i].to.clone());
            let get = |v: &str| {
                piece
                    .vertices
                    .get(v)
                    .ok_or_else(|| PyError::Key(v.to_string()))
            };
            let (pa, pb) = (get(&a)?, get(&b)?);
            let rest: Vec<InstEdge> = edges[i + 1..].iter().chain(&edges[..i]).cloned().collect();
            let on_line = |v: &str| v == a || v == b;
            let right = rest.iter().map(|e| InstEdge {
                id: format!("{}.R", e.id),
                ..e.clone()
            });
            let left: Vec<InstEdge> = mirrored_loop(&rest, |v| {
                if on_line(v) {
                    v.to_string()
                } else {
                    format!("{v}.L")
                }
            })
            .into_iter()
            .map(|e| InstEdge {
                id: format!("{}.L", e.id),
                ..e
            })
            .collect();
            let mut nv = piece.vertices.clone();
            for (v, p) in piece.vertices.iter() {
                if !on_line(v) {
                    nv.insert(&format!("{v}.L"), reflect_across(p, pa, pb));
                }
            }
            Instance {
                id: pid.clone(),
                piece: pid.clone(),
                piece_index,
                side: Side::C,
                mirrored: false,
                verts: nv,
                edges: right.chain(left).collect(),
            }
        }
        None => Instance {
            id: pid.clone(),
            piece: pid.clone(),
            piece_index,
            side: Side::C,
            mirrored: false,
            verts: piece.vertices.clone(),
            edges,
        },
    };
    if piece.qty() == 2 {
        let lv = Vertices(
            base.verts
                .iter()
                .map(|(k, p)| (k.to_string(), p.mirror_x()))
                .collect(),
        );
        let le = mirrored_loop(&base.edges, str::to_string);
        let r = Instance {
            id: format!("{}.R", base.id),
            side: Side::R,
            ..base.clone()
        };
        let l = Instance {
            id: format!("{}.L", base.id),
            side: Side::L,
            mirrored: true,
            verts: lv,
            edges: le,
            ..base
        };
        return Ok(vec![r, l]);
    }
    Ok(vec![base])
}

#[cfg(test)]
mod physical_side_tests {
    use super::*;
    use crate::Pattern;
    use serde_json::json;

    fn panel() -> Value {
        json!({"id":"panel","cut":{"qty":1},
            "vertices":{"a":[10,20],"b":[80,20],"c":[65,110],"d":[20,110]},
            "edges":[{"id":"bottom","from":"a","to":"b"},
                {"id":"right","from":"b","to":"c"},
                {"id":"top","from":"c","to":"d"},
                {"id":"left","from":"d","to":"a"}],
            "grain":[[30,30],[30,90]],"placement":{"part":"arm","side":"L"}})
    }

    #[test]
    fn physical_side_never_expands_or_mirrors_the_panel() {
        let mut raw = panel();
        raw["placement"].as_object_mut().unwrap().remove("side");
        let original =
            Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm","pieces":[raw.clone()]}));
        for (part, side) in [("arm", "L"), ("arm", "R"), ("leg", "L"), ("leg", "R")] {
            raw["placement"] = json!({"part":part,"side":side});
            let pattern = Pattern::new(
                json!({"schema":"fabric-pattern/1","units":"mm","pieces":[raw.clone()]}),
            );
            assert!(pattern.structure_errors().is_empty());
            assert_eq!(pattern.instances, original.instances);
            assert_eq!(pattern.instances[0].side, Side::C);
            assert!(!pattern.instances[0].mirrored);
            assert_eq!(pattern.raw()["pieces"][0]["grain"], raw["grain"]);
            let restored = Pattern::from_doc(pattern.doc.unwrap());
            assert_eq!(restored.raw()["pieces"][0]["placement"]["side"], side);
        }
    }

    #[test]
    fn contradictory_physical_side_blocks_the_entire_pattern() {
        for (pointer, value) in [
            ("/placement/side", json!(null)),
            ("/placement/side", json!("C")),
            ("/placement/side", json!("left")),
            ("/placement/side", json!(true)),
            ("/placement/part", json!("torso")),
            ("/placement/part", json!("hand")),
            ("/cut/qty", json!(2)),
        ] {
            let mut raw = panel();
            *raw.pointer_mut(pointer).unwrap() = value;
            let pattern =
                Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm","pieces":[raw]}));
            assert!(pattern.instances.is_empty(), "{pointer}");
            assert!(!pattern.structure_errors().is_empty(), "{pointer}");
        }
        let mut raw = panel();
        raw["cut"]["on_fold"] = json!("left");
        let pattern =
            Pattern::new(json!({"schema":"fabric-pattern/1","units":"mm","pieces":[raw]}));
        assert!(pattern.instances.is_empty());
        assert!(
            pattern
                .build_errors
                .iter()
                .any(|e| e.contains("placement.side"))
        );
    }
}
