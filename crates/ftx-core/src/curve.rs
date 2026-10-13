//! Chord-relative edge curves (`docs/spec/01` §2.5, §4).
//!
//! Control points are stored relative to the edge chord: `[t, h]` is `p0 + t·d + h·perp(d)` with
//! `d = p1 − p0` and `perp(d) = (−d_y, d_x)`, so moving a vertex keeps the curve's character and the
//! points copy unchanged to and from GarmentCode. `h > 0` lies left of the chord, which on a
//! counter-clockwise loop is into the piece.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::det;
use crate::np;
use crate::py;
use crate::vec::Vec2;

/// Samples per curved edge for lengths, seam sides, notch fractions and seam sampling.
pub const EDGE_DENSE: usize = 200;
/// Samples per curved edge for an instance outline used for area and the winding check.
pub const OUTLINE_DENSE: usize = 60;
/// Samples per curved edge for points of measure (girth and width lines).
pub const MEASURE_DENSE: usize = 120;
/// Samples of the source edge in transfer-curve.
pub const TRANSFER_DENSE: usize = 60;

/// An edge's `curve` object. Absent, `null`, `{}` or `{"type": "line"}` is a straight edge.
///
/// Unknown keys are kept in `extra` and survive mirroring. `kind` is kept as text so that an
/// unknown type is reported by [`Curve::kind`] with the reference's message rather than failing to
/// parse. (A JSON `"type": null` reads as absent, i.e. straight; the reference would call it an
/// unknown type `None`.)
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    /// `"line"`, `"quad"`, `"cubic"` or `"poly"`; absent means line.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Control points `[t, h]` relative to the chord.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub c: Option<Vec<[f64; 2]>>,
    /// Other keys, preserved.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The four curve types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveKind {
    /// Straight chord.
    Line,
    /// Quadratic Bézier, one control point.
    Quad,
    /// Cubic Bézier, two control points.
    Cubic,
    /// Polyline through the control points (no densification).
    Poly,
}

impl CurveKind {
    /// The JSON spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Quad => "quad",
            Self::Cubic => "cubic",
            Self::Poly => "poly",
        }
    }
}

/// Curve geometry errors, displayed with the reference's messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurveError {
    /// `abs_to_rel` on a chord of exactly zero length.
    ZeroLengthChord,
    /// A Bézier with the wrong number of control points.
    ControlPointCount {
        /// `Quad` or `Cubic`.
        kind: CurveKind,
    },
    /// A `type` other than the four known ones.
    UnknownType(String),
}

impl fmt::Display for CurveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroLengthChord => f.write_str("zero-length chord"),
            Self::ControlPointCount { kind } => {
                let need = if *kind == CurveKind::Quad { 1 } else { 2 };
                write!(f, "{} curve needs {need} control points", kind.as_str())
            }
            Self::UnknownType(t) => write!(f, "unknown curve type {}", py::repr_str(t)),
        }
    }
}

impl std::error::Error for CurveError {}

impl Curve {
    /// A curve of the given kind with these control points and no extra keys.
    #[must_use]
    pub fn new(kind: CurveKind, c: Vec<[f64; 2]>) -> Self {
        Self {
            kind: Some(kind.as_str().to_string()),
            c: Some(c),
            extra: Map::new(),
        }
    }

    /// The curve type.
    ///
    /// # Errors
    /// [`CurveError::UnknownType`] for a `type` other than line, quad, cubic or poly.
    pub fn kind(&self) -> Result<CurveKind, CurveError> {
        match self.kind.as_deref() {
            None | Some("line") => Ok(CurveKind::Line),
            Some("quad") => Ok(CurveKind::Quad),
            Some("cubic") => Ok(CurveKind::Cubic),
            Some("poly") => Ok(CurveKind::Poly),
            Some(other) => Err(CurveError::UnknownType(other.to_string())),
        }
    }

    /// True for an absent type or `"line"`, the reference's test before mirroring.
    #[must_use]
    pub fn is_line(&self) -> bool {
        matches!(self.kind.as_deref(), None | Some("line"))
    }

    /// The control points (empty when absent).
    #[must_use]
    pub fn controls(&self) -> &[[f64; 2]] {
        self.c.as_deref().unwrap_or(&[])
    }
}

/// `p0 + t·d + h·perp(d)`, evaluated in the reference's order.
#[must_use]
pub fn rel_to_abs(p0: Vec2, p1: Vec2, th: [f64; 2]) -> Vec2 {
    let d = p1 - p0;
    let pd = d.perp();
    Vec2::new(
        (p0.x + th[0] * d.x) + th[1] * pd.x,
        (p0.y + th[0] * d.y) + th[1] * pd.y,
    )
}

/// The chord-relative `[t, h]` of `q`.
///
/// # Errors
/// [`CurveError::ZeroLengthChord`] when `p0 == p1` exactly.
pub fn abs_to_rel(p0: Vec2, p1: Vec2, q: Vec2) -> Result<[f64; 2], CurveError> {
    let d = p1 - p0;
    let l2 = d.dot(d);
    if l2 == 0.0 {
        return Err(CurveError::ZeroLengthChord);
    }
    let w = q - p0;
    Ok([w.dot(d) / l2, w.dot(d.perp()) / l2])
}

/// Bézier of degree `ctrl.len() − 1` sampled at numpy's `linspace(0, 1, n)`, summed in Bernstein
/// form term by term like the reference (`comb(k,i)·tⁱ·(1−t)^(k−i)·P_i`). Powers 2 are products,
/// as in numpy; power 3 is the correctly rounded [`det::cube`] (numpy calls the platform `pow`).
#[must_use]
pub fn bezier(ctrl: &[Vec2], n: usize) -> Vec<Vec2> {
    let k = ctrl.len() - 1;
    let pow = |x: f64, e: usize| match e {
        0 => 1.0,
        1 => x,
        2 => x * x,
        3 => det::cube(x),
        _ => det::powi(x, e as i32),
    };
    np::linspace(0.0, 1.0, n)
        .into_iter()
        .map(|t| {
            let mut out = Vec2::ZERO;
            for (i, c) in ctrl.iter().enumerate() {
                let w = binomial(k, i) * pow(t, i) * pow(1.0 - t, k - i);
                out.x += w * c.x;
                out.y += w * c.y;
            }
            out
        })
        .collect()
}

fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1u64, |acc, i| acc * (n - i) as u64 / (i as u64 + 1)) as f64
}

/// The polyline of an edge from `p0` to `p1`, endpoints included: 2 points for a line, `dense`
/// samples for a Bézier, the control polygon for `poly`.
///
/// # Errors
/// [`CurveError::ControlPointCount`] for a quad without exactly 1 or a cubic without exactly 2
/// control points, [`CurveError::UnknownType`] for an unknown type.
pub fn curve_points(
    p0: Vec2,
    p1: Vec2,
    curve: Option<&Curve>,
    dense: usize,
) -> Result<Vec<Vec2>, CurveError> {
    let Some(curve) = curve else {
        return Ok(vec![p0, p1]);
    };
    let kind = curve.kind()?;
    let ctrl = || curve.controls().iter().map(|&th| rel_to_abs(p0, p1, th));
    match kind {
        CurveKind::Line => Ok(vec![p0, p1]),
        CurveKind::Quad | CurveKind::Cubic => {
            let need = if kind == CurveKind::Quad { 1 } else { 2 };
            if curve.controls().len() != need {
                return Err(CurveError::ControlPointCount { kind });
            }
            let mut poly = vec![p0];
            poly.extend(ctrl());
            poly.push(p1);
            Ok(bezier(&poly, dense))
        }
        CurveKind::Poly => {
            let mut pts = vec![p0];
            pts.extend(ctrl());
            pts.push(p1);
            Ok(pts)
        }
    }
}

/// The curve after mirroring the piece and reversing the edge (the two always happen together,
/// which keeps loops counter-clockwise): `c' = reverse([[1 − t, h]])`. Straight curves and absent
/// ones are returned unchanged; other keys are kept.
#[must_use]
pub fn mirror_curve(curve: Option<&Curve>) -> Option<Curve> {
    map_controls(curve, |[t, h]| [1.0 - t, h])
}

/// The same geometry traversed the other way: `c' = reverse([[1 − t, −h]])`.
#[must_use]
pub fn reverse_curve(curve: Option<&Curve>) -> Option<Curve> {
    map_controls(curve, |[t, h]| [1.0 - t, -h])
}

fn map_controls(curve: Option<&Curve>, f: impl Fn([f64; 2]) -> [f64; 2]) -> Option<Curve> {
    let curve = curve?;
    if curve.is_line() {
        return Some(curve.clone());
    }
    let mut out = curve.clone();
    if let Some(c) = &curve.c {
        out.c = Some(c.iter().rev().map(|&th| f(th)).collect());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polyline::polyline_length;

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    /// Spec 01 §14.2 F1.
    #[test]
    fn f1_rel_abs() {
        let th = abs_to_rel(v(3.0, 4.0), v(40.0, -10.0), v(20.0, 15.0)).unwrap();
        assert_eq!(th, [0.3035143769968051, 0.41214057507987223]);
        let back = rel_to_abs(v(3.0, 4.0), v(40.0, -10.0), th);
        assert!((back - v(20.0, 15.0)).norm() < 1e-12);
        assert_eq!(
            rel_to_abs(v(0.0, 0.0), v(100.0, 0.0), [0.5, 0.2]),
            v(50.0, 20.0)
        );
        let err = abs_to_rel(v(1.0, 1.0), v(1.0, 1.0), v(2.0, 2.0)).unwrap_err();
        assert_eq!(err.to_string(), "zero-length chord");
    }

    /// Spec 01 §14.2 F2.
    #[test]
    fn f2_curve_points() {
        let (p0, p1) = (v(0.0, 0.0), v(100.0, 0.0));
        assert_eq!(
            curve_points(p0, p1, None, EDGE_DENSE).unwrap(),
            vec![p0, p1]
        );
        let poly = Curve::new(CurveKind::Poly, vec![[0.25, 0.1], [0.75, -0.1]]);
        assert_eq!(
            curve_points(p0, p1, Some(&poly), EDGE_DENSE).unwrap(),
            vec![p0, v(25.0, 10.0), v(75.0, -10.0), p1]
        );
        let quad = Curve::new(CurveKind::Quad, vec![[0.5, 0.5]]);
        let q = curve_points(p0, p1, Some(&quad), EDGE_DENSE).unwrap();
        assert_eq!(q.len(), 200);
        assert_eq!(q[1], v(0.5025125628140703, 0.49998737405621063));
        assert_eq!(polyline_length(&q), 114.77905987262527);
        let cubic = Curve::new(CurveKind::Cubic, vec![[0.25, 0.3], [0.75, 0.3]]);
        let c = curve_points(p0, p1, Some(&cubic), EDGE_DENSE).unwrap();
        assert_eq!(c.len(), 200);
        assert!((polyline_length(&c) - 113.24051269018844).abs() < 1e-12);
        let c2000 = curve_points(p0, p1, Some(&cubic), 2000).unwrap();
        assert!((polyline_length(&c2000) - 113.24087703591515).abs() < 1e-11);
        let bad = Curve::new(CurveKind::Quad, vec![[0.5, 0.5], [0.6, 0.1]]);
        assert_eq!(
            curve_points(p0, p1, Some(&bad), EDGE_DENSE)
                .unwrap_err()
                .to_string(),
            "quad curve needs 1 control points"
        );
        let bad = Curve::new(CurveKind::Cubic, vec![[0.5, 0.5]]);
        assert_eq!(
            curve_points(p0, p1, Some(&bad), EDGE_DENSE)
                .unwrap_err()
                .to_string(),
            "cubic curve needs 2 control points"
        );
        let spline: Curve = serde_json::from_str(r#"{"type": "spline", "c": []}"#).unwrap();
        assert_eq!(
            curve_points(p0, p1, Some(&spline), EDGE_DENSE)
                .unwrap_err()
                .to_string(),
            "unknown curve type 'spline'"
        );
    }

    /// Spec 01 §14.2 F3.
    #[test]
    fn f3_mirror_and_reverse() {
        let c: Curve =
            serde_json::from_str(r#"{"type": "cubic", "c": [[0.2, 0.1], [0.7, -0.3]], "x": 1}"#)
                .unwrap();
        let m = mirror_curve(Some(&c)).unwrap();
        let mc = m.controls();
        assert!(
            (mc[0][0] - 0.3).abs() < 1e-15
                && mc[0][1] == -0.3
                && (mc[1][0] - 0.8).abs() < 1e-15
                && mc[1][1] == 0.1
        );
        assert_eq!(m.extra.get("x"), Some(&Value::from(1)));
        let r = reverse_curve(Some(&c)).unwrap();
        let rc = r.controls();
        assert!(
            (rc[0][0] - 0.3).abs() < 1e-15
                && rc[0][1] == 0.3
                && (rc[1][0] - 0.8).abs() < 1e-15
                && rc[1][1] == -0.1
        );
        assert_eq!(mirror_curve(None), None);
        let line: Curve = serde_json::from_str(r#"{"type": "line"}"#).unwrap();
        assert_eq!(mirror_curve(Some(&line)), Some(line.clone()));
        assert_eq!(
            serde_json::to_value(&line).unwrap(),
            serde_json::json!({"type": "line"})
        );
        let round_trip: Value = serde_json::to_value(&m).unwrap();
        assert_eq!(round_trip["x"], Value::from(1));
        assert_eq!(round_trip["type"], Value::from("cubic"));
    }
}
