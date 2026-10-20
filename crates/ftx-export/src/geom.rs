//! Piece geometry (`docs/spec/03` §5): the cut and sew rings of a piece as drafted or of an instance, with
//! its notches, grainline, fold marks, internal lines, drills, decor and labels, in the model frame.

use ftx_core::np::pairwise_sum;
use ftx_core::polyline::cumulative;
use ftx_core::{Refusal, RefusalCode, Vec2, det, py};
use ftx_pattern::{Instance, Pattern};
use serde_json::Value;

use crate::outline::{EdgeLoop, cut_outline, drafted_instance, edge_loops, sew_breaks, sew_ring};
use crate::pole::{covers, interior_point, pole_of_inaccessibility, signed_distance};
use crate::text::{ascii_text, fnum, text_width};

/// Helvetica cap height over font size.
pub const CAP: f64 = 0.718;

/// A block of text lines: anchor, `(text, relative size, bold)` lines, base size (mm) and kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    /// Anchor (the block's centre).
    pub p: Vec2,
    /// Lines: text, size relative to `size`, bold.
    pub lines: Vec<(String, f64, bool)>,
    /// Base size (mm) of a line at relative size 1.
    pub size: f64,
    /// `label`, `fold` or `internal`.
    pub kind: String,
}

/// A notch: where it sits on the cut line, which way it points and its mark polylines.
#[derive(Clone, Debug, PartialEq)]
pub struct Notch {
    /// Notch id.
    pub id: String,
    /// `V`, `T`, `castle`, `check`, `U`, `slit`, ...
    pub kind: String,
    /// On the cut line (the offset of its edge).
    pub base: Vec2,
    /// Unit vector into the piece.
    pub inward: Vec2,
    /// Unit tangent of the edge there.
    pub tangent: Vec2,
    /// Depth (mm).
    pub depth: f64,
    /// Width (mm).
    pub width: f64,
    /// Mark polylines.
    pub marks: Vec<Vec<Vec2>>,
    /// The matching point on the sew line.
    pub sew_point: Vec2,
}

/// Braid or embroidery along a polyline.
#[derive(Clone, Debug, PartialEq)]
pub struct Decor {
    /// `soutache`, `embroidery`, ...
    pub kind: String,
    /// Points.
    pub pts: Vec<Vec2>,
    /// Width (mm).
    pub width: f64,
    /// Closed.
    pub closed: bool,
}

/// How a piece cut on the fold is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldMode {
    /// The drafted half, with the place-on-fold bracket.
    Half,
    /// Unfolded, with the fold line.
    Unfolded,
    /// Not cut on the fold.
    None,
}

impl FoldMode {
    /// `half`, `unfolded` or `none`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Half => "half",
            Self::Unfolded => "unfolded",
            Self::None => "none",
        }
    }
}

/// Facts about a piece every target prints.
#[derive(Clone, Debug, PartialEq)]
pub struct GeomMeta {
    /// Pieces to cut.
    pub qty: i64,
    /// A mirror pair.
    pub mirror: bool,
    /// Material (`cut.material`, else the pattern's fabric name); `None` when JSON null.
    pub material: Option<String>,
    /// The fold edge id, or empty.
    pub on_fold: String,
    /// How the fold is shown.
    pub fold_mode: FoldMode,
    /// `R`, `L` or `C`.
    pub side: String,
    /// The default seam allowance.
    pub sa_mm: f64,
    /// Distinct seam allowances above 0, rounded to 0.1 mm, ascending.
    pub sa_values: Vec<f64>,
    /// `piece.sim` (true when absent).
    pub sim: Value,
}

/// The geometry of a piece as drafted or of an instance.
#[derive(Clone, Debug, PartialEq)]
pub struct Geom {
    /// Piece id.
    pub pid: String,
    /// Instance id (the piece id for a drafted half).
    pub iid: String,
    /// Piece name, else its id.
    pub name: String,
    /// Facts every target prints.
    pub meta: GeomMeta,
    /// Cut ring.
    pub cut: Vec<Vec2>,
    /// Sew ring.
    pub sew: Vec<Vec2>,
    /// Index in the sew ring where each edge starts.
    pub sew_breaks: Vec<usize>,
    /// Notches.
    pub notches: Vec<Notch>,
    /// Grainline.
    pub grain: Option<(Vec2, Vec2)>,
    /// The grainline was placed by default.
    pub grain_default: bool,
    /// Fold marks: polyline and style (`bracket` or `dashdot`).
    pub fold_lines: Vec<(Vec<Vec2>, String)>,
    /// Drawn internal lines and whether each is closed.
    pub internal_draw: Vec<(Vec<Vec2>, bool)>,
    /// Cut internal lines.
    pub internal_cut: Vec<(Vec<Vec2>, bool)>,
    /// Drills: point and diameter.
    pub drills: Vec<(Vec2, f64)>,
    /// Labels, the main block first.
    pub labels: Vec<Label>,
    /// Decor.
    pub decor: Vec<Decor>,
}

impl Geom {
    /// The geometry under `P ↦ A·P + b` (directions as `unit(A·d)`; texts, sizes and kinds unchanged).
    #[must_use]
    pub fn transform(&self, m: &Affine) -> Self {
        let f = |p: Vec2| m.apply(p);
        let fs = |q: &[Vec2]| q.iter().map(|&p| m.apply(p)).collect::<Vec<_>>();
        Self {
            pid: self.pid.clone(),
            iid: self.iid.clone(),
            name: self.name.clone(),
            meta: self.meta.clone(),
            cut: fs(&self.cut),
            sew: fs(&self.sew),
            sew_breaks: self.sew_breaks.clone(),
            notches: self
                .notches
                .iter()
                .map(|n| Notch {
                    id: n.id.clone(),
                    kind: n.kind.clone(),
                    base: f(n.base),
                    inward: m.direction(n.inward),
                    tangent: m.direction(n.tangent),
                    depth: n.depth,
                    width: n.width,
                    marks: n.marks.iter().map(|k| fs(k)).collect(),
                    sew_point: f(n.sew_point),
                })
                .collect(),
            grain: self.grain.map(|(a, b)| (f(a), f(b))),
            grain_default: self.grain_default,
            fold_lines: self
                .fold_lines
                .iter()
                .map(|(q, st)| (fs(q), st.clone()))
                .collect(),
            internal_draw: self
                .internal_draw
                .iter()
                .map(|(q, c)| (fs(q), *c))
                .collect(),
            internal_cut: self.internal_cut.iter().map(|(q, c)| (fs(q), *c)).collect(),
            drills: self.drills.iter().map(|&(p, d)| (f(p), d)).collect(),
            labels: self
                .labels
                .iter()
                .map(|lb| Label {
                    p: f(lb.p),
                    ..lb.clone()
                })
                .collect(),
            decor: self
                .decor
                .iter()
                .map(|d| Decor {
                    pts: fs(&d.pts),
                    ..d.clone()
                })
                .collect(),
        }
    }

    /// Bounding box of the cut ring: `(min x, min y, max x, max y)`.
    #[must_use]
    pub fn bbox(&self) -> (f64, f64, f64, f64) {
        self.cut.iter().fold(
            (
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ),
            |(a, b, c, d), p| (a.min(p.x), b.min(p.y), c.max(p.x), d.max(p.y)),
        )
    }
}

/// An affine map `P ↦ A·P + b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    /// Linear part (rows).
    pub a: [[f64; 2]; 2],
    /// Translation.
    pub b: Vec2,
}

impl Affine {
    /// The identity.
    pub const IDENTITY: Self = Self {
        a: [[1.0, 0.0], [0.0, 1.0]],
        b: Vec2::ZERO,
    };

    /// `A·p + b`.
    #[must_use]
    pub fn apply(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            p.x * self.a[0][0] + p.y * self.a[0][1] + self.b.x,
            p.x * self.a[1][0] + p.y * self.a[1][1] + self.b.y,
        )
    }

    /// `unit(A·d)`.
    #[must_use]
    pub fn direction(&self, d: Vec2) -> Vec2 {
        unit(Vec2::new(
            d.x * self.a[0][0] + d.y * self.a[0][1],
            d.x * self.a[1][0] + d.y * self.a[1][1],
        ))
    }
}

fn norm(v: Vec2) -> f64 {
    (v.x * v.x + v.y * v.y).sqrt()
}

fn unit(v: Vec2) -> Vec2 {
    let n = norm(v);
    if n > 0.0 {
        Vec2::new(v.x / n, v.y / n)
    } else {
        v
    }
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn point(v: &Value) -> Option<Vec2> {
    Some(Vec2::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?))
}

fn points(v: &Value) -> Vec<Vec2> {
    v.as_array()
        .map(|a| a.iter().filter_map(point).collect())
        .unwrap_or_default()
}

/// Python truthiness of a JSON value.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python `str()` of a JSON scalar.
fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n
            .as_i64()
            .map(|i| i.to_string())
            .or_else(|| n.as_f64().map(py::repr_f64))
            .unwrap_or_default(),
        other => other.to_string(),
    }
}

/// The piece's grainline as two points (`[[x0, y0], [x1, y1]]` or the legacy `{p0, p1}`).
fn grain_points(piece: &Value) -> Option<(Vec2, Vec2)> {
    let g = piece.get("grain")?;
    if !truthy(g) {
        return None;
    }
    if g.is_object() {
        return Some((point(&g["p0"])?, point(&g["p1"])?));
    }
    Some((point(&g[0])?, point(&g[1])?))
}

/// Affine maps from drafted piece coordinates to the geometry's frame: the identity, plus for an instance
/// of a piece cut on the fold the reflection across the fold edge; all pre-composed with `x → −x` for a
/// mirrored instance.
pub(crate) fn feature_maps(piece: &Value, inst: Option<&Instance>) -> Vec<Affine> {
    let Some(inst) = inst else {
        return vec![Affine::IDENTITY];
    };
    let mut maps = vec![Affine::IDENTITY];
    if let Some(fold) = piece["cut"]["on_fold"].as_str().filter(|f| !f.is_empty()) {
        let fe = piece["edges"]
            .as_array()
            .and_then(|es| es.iter().find(|e| e["id"] == fold));
        if let Some(fe) = fe {
            let pa = point(&piece["vertices"][fe["from"].as_str().unwrap_or("")]);
            let pb = point(&piece["vertices"][fe["to"].as_str().unwrap_or("")]);
            if let (Some(pa), Some(pb)) = (pa, pb) {
                let d = unit(pb - pa);
                let a = [
                    [2.0 * (d.x * d.x) - 1.0, 2.0 * (d.x * d.y) - 0.0],
                    [2.0 * (d.y * d.x) - 0.0, 2.0 * (d.y * d.y) - 1.0],
                ];
                let ap = Vec2::new(
                    a[0][0] * pa.x + a[0][1] * pa.y,
                    a[1][0] * pa.x + a[1][1] * pa.y,
                );
                maps.push(Affine { a, b: pa - ap });
            }
        }
    }
    if inst.mirrored {
        for m in &mut maps {
            m.a = [[-m.a[0][0], -m.a[0][1]], m.a[1]];
            m.b = Vec2::new(-m.b.x, m.b.y);
        }
    }
    maps
}

fn notch_marks(
    kind: &str,
    base: Vec2,
    inward: Vec2,
    tan: Vec2,
    depth: f64,
    width: f64,
) -> Vec<Vec<Vec2>> {
    let apex = base + inward * depth;
    let h = 0.5 * width;
    match kind {
        "V" => vec![vec![
            base + tan * (0.5 * width),
            apex,
            base - tan * (0.5 * width),
        ]],
        "T" => vec![
            vec![base, apex],
            vec![apex - tan * (0.4 * width), apex + tan * (0.4 * width)],
        ],
        "castle" => vec![vec![
            base - tan * h,
            apex - tan * h,
            apex + tan * h,
            base + tan * h,
        ]],
        "check" => vec![vec![
            base,
            apex,
            apex + tan * (0.5 * width) - inward * (0.3 * depth),
        ]],
        "U" => {
            let mut m = vec![base + tan * h];
            for a in ftx_core::np::linspace(0.0, std::f64::consts::PI, 9) {
                let (s, c) = det::sin_cos(a);
                m.push(apex - inward * h + (tan * c + inward * s) * h);
            }
            m.push(base - tan * h);
            vec![m]
        }
        _ => vec![vec![base, apex]],
    }
}

/// Point at arclength fraction `s` along `pts` and the unit direction of its segment.
fn point_on(pts: &[Vec2], s: f64) -> (Vec2, Vec2) {
    let cum = cumulative(pts);
    let target = s.clamp(0.0, 1.0) * cum[cum.len() - 1];
    let left = cum.partition_point(|&c| c < target);
    let k = (left as i64 - 1).clamp(0, pts.len() as i64 - 2) as usize;
    let seg = cum[k + 1] - cum[k];
    let u = if seg > 0.0 {
        (target - cum[k]) / seg
    } else {
        0.0
    };
    let p = pts[k] + (pts[k + 1] - pts[k]) * u;
    (p, unit(pts[k + 1] - pts[k]))
}

/// The default notch depth for a seam allowance: half of it, between 3 and 6 mm.
#[must_use]
pub fn notch_depth(sa: f64) -> f64 {
    3f64.max(6f64.min(0.5 * sa))
}

fn stem(eid: &str) -> &str {
    if eid.ends_with(".R") || eid.ends_with(".L") {
        &eid[..eid.len() - 2]
    } else {
        eid
    }
}

/// Where a vertical line at `x` lies inside the ring: its longest inside interval `(y_lo, y_hi)`.
fn vertical_chord(ring: &[Vec2], x: f64) -> Option<(f64, f64)> {
    let n = ring.len();
    let mut ys = Vec::new();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a.x > x) != (b.x > x) {
            ys.push(a.y + (x - a.x) * (b.y - a.y) / (b.x - a.x));
        }
    }
    ys.sort_by(f64::total_cmp);
    ys.as_chunks::<2>()
        .0
        .iter()
        .map(|c| (c[0], c[1]))
        .filter(|(a, b)| b > a)
        .fold(None, |best: Option<(f64, f64)>, c| match best {
            Some(b) if b.1 - b.0 >= c.1 - c.0 => Some(b),
            _ => Some(c),
        })
}

/// What a geometry is built from.
#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
    /// A piece as drafted (a piece on the fold shows its drafted half), by index.
    Piece(usize),
    /// An instance.
    Instance(&'a Instance),
}

/// The geometry of a piece as drafted or an instance, in the model frame (y up); `text_size` caps the main
/// label's base size (6 for the master and PDFs, 10 for DXF-ASTM, 8 for flat DXF, 17 for the projector).
///
/// # Errors
/// As [`cut_outline`]; a piece index out of range or an untyped pattern.
pub fn build_geom(
    pattern: &Pattern,
    src: Source,
    sa_default: f64,
    mitre_limit: f64,
    text_size: f64,
) -> Result<Geom, Refusal> {
    let doc = pattern
        .doc
        .as_ref()
        .ok_or_else(|| invalid("the pattern has no typed document"))?;
    let raw = pattern.raw();
    let (drafted, inst, is_inst) = match src {
        Source::Piece(i) => {
            let piece = doc
                .pieces
                .get(i)
                .ok_or_else(|| invalid(format!("no piece {i}")))?;
            (Some(drafted_instance(piece, i)), None, false)
        }
        Source::Instance(inst) => (None, Some(inst), true),
    };
    let inst: &Instance = match (&drafted, inst) {
        (Some(d), _) => d,
        (None, Some(i)) => i,
        (None, None) => unreachable!(),
    };
    let piece = &doc.pieces[inst.piece_index];
    let pv = &raw["pieces"][inst.piece_index];
    let on_fold = piece.on_fold().filter(|f| !f.is_empty());
    let fold = if is_inst { None } else { on_fold };
    let loops: Vec<EdgeLoop> = edge_loops(inst, piece, fold, sa_default)?;
    let (cut, _) = cut_outline(inst, piece, fold, sa_default, mitre_limit)?;
    let sew = sew_ring(&loops);
    let breaks = sew_breaks(&loops);
    let cutd = &pv["cut"];
    let qty = cutd["qty"].as_f64().map_or(1, |q| q as i64);
    let mode = match (on_fold.is_some(), is_inst) {
        (true, false) => FoldMode::Half,
        (true, true) => FoldMode::Unfolded,
        _ => FoldMode::None,
    };
    let mut sas: Vec<f64> = loops
        .iter()
        .filter(|l| l.sa > 0.0)
        .map(|l| py::round(l.sa, 1))
        .collect();
    sas.sort_by(f64::total_cmp);
    sas.dedup();
    let material = match cutd.get("material") {
        Some(Value::Null) => None,
        Some(v) => Some(py_str(v)),
        None => match raw
            .get("fabric")
            .filter(|f| truthy(f))
            .and_then(|f| f.get("name"))
        {
            Some(Value::Null) => None,
            Some(v) => Some(py_str(v)),
            None => Some(String::new()),
        },
    };
    let meta = GeomMeta {
        qty,
        mirror: qty == 2,
        material,
        on_fold: on_fold.unwrap_or("").to_string(),
        fold_mode: mode,
        side: inst.side.as_str().to_string(),
        sa_mm: sa_default,
        sa_values: sas.clone(),
        sim: pv.get("sim").cloned().unwrap_or(Value::Bool(true)),
    };
    let name = pv.get("name").map_or_else(|| piece.id.clone(), py_str);
    let mut g = Geom {
        pid: piece.id.clone(),
        iid: inst.id.clone(),
        name,
        meta,
        cut,
        sew: sew.clone(),
        sew_breaks: breaks,
        notches: Vec::new(),
        grain: None,
        grain_default: false,
        fold_lines: Vec::new(),
        internal_draw: Vec::new(),
        internal_cut: Vec::new(),
        drills: Vec::new(),
        labels: Vec::new(),
        decor: Vec::new(),
    };
    let maps = feature_maps(pv, if is_inst { Some(inst) } else { None });

    // notches: on the cut line, pointing inward
    let notches = pv["notches"].as_array().cloned().unwrap_or_default();
    for l in &loops {
        let rev = is_inst && (l.id.ends_with(".L") != inst.mirrored);
        for n in &notches {
            if n["edge"].as_str() != Some(stem(&l.id)) {
                continue;
            }
            let s0 = n["s"].as_f64().unwrap_or(0.0);
            let s = if rev { 1.0 - s0 } else { s0 };
            let (p, t) = point_on(&l.pts, s);
            let nout = Vec2::new(t.y, -t.x);
            let base = p + nout * l.sa;
            let depth = n
                .get("depth")
                .and_then(Value::as_f64)
                .unwrap_or_else(|| notch_depth(l.sa));
            let width = n.get("width").and_then(Value::as_f64).unwrap_or(3.0);
            let kind = n.get("type").map_or_else(|| "V".to_string(), py_str);
            let inward = Vec2::new(-nout.x, -nout.y);
            g.notches.push(Notch {
                id: n.get("id").map_or_else(String::new, py_str),
                marks: notch_marks(&kind, base, inward, t, depth, width),
                kind,
                base,
                inward,
                tangent: t,
                depth,
                width,
                sew_point: p,
            });
        }
    }

    let sew_rings: [&[Vec2]; 1] = [&sew];
    if let Some((a, b)) = grain_points(pv) {
        g.grain = Some((maps[0].apply(a), maps[0].apply(b)));
    } else {
        let c = pole_of_inaccessibility(&sew_rings, 1.0);
        if let Some((ya, yb)) = vertical_chord(&sew, c.x) {
            let (qa, qb) = (Vec2::new(c.x, ya), Vec2::new(c.x, yb));
            let mid = Vec2::new((qa.x + qb.x) / 2.0, (qa.y + qb.y) / 2.0);
            let half = (qb - qa) * 0.35;
            g.grain = Some((mid - half, mid + half));
            g.grain_default = true;
        }
    }

    // fold marks
    if let Some(fold_id) = on_fold {
        let fe = pv["edges"]
            .as_array()
            .and_then(|es| es.iter().find(|e| e["id"] == fold_id))
            .ok_or_else(|| invalid(format!("{}: no fold edge {fold_id}", piece.id)))?;
        let vert = |k: &str| {
            point(&pv["vertices"][fe[k].as_str().unwrap_or("")])
                .ok_or_else(|| invalid(format!("{}: fold edge vertex missing", piece.id)))
        };
        let (pa, pb) = (maps[0].apply(vert("from")?), maps[0].apply(vert("to")?));
        let d = pb - pa;
        if mode == FoldMode::Half {
            let nin = unit(Vec2::new(-d.y, d.x));
            let (q1, q2) = (pa + d * 0.2, pa + d * 0.8);
            let tu = unit(d);
            g.fold_lines.push((
                vec![q1, q1 + nin * 12.0, q2 + nin * 12.0, q2],
                "bracket".into(),
            ));
            for q in [q1, q2] {
                g.fold_lines.push((
                    vec![q + nin * 3.0 + tu * 1.5, q, q + nin * 3.0 - tu * 1.5],
                    "bracket".into(),
                ));
            }
            let tw = text_width("PLACE ON FOLD", 4.0, true);
            let shift = 2.0 + nin.x.abs() * tw / 2.0 + nin.y.abs() * 3.0;
            g.labels.push(Label {
                p: pa + d * 0.9 + nin * shift,
                lines: vec![("PLACE ON FOLD".into(), 1.0, true)],
                size: 4.0,
                kind: "fold".into(),
            });
        } else {
            g.fold_lines.push((vec![pa, pb], "dashdot".into()));
            let txt = format!("FOLD LINE ({fold_id})");
            let nrm = unit(Vec2::new(-d.y, d.x));
            let shift = 2.0 + nrm.x.abs() * text_width(&txt, 3.5, false) / 2.0 + nrm.y.abs() * 3.0;
            g.labels.push(Label {
                p: pa + d * 0.85 + nrm * shift,
                lines: vec![(txt, 1.0, false)],
                size: 3.5,
                kind: "fold".into(),
            });
        }
    }

    for item in pv["internal"].as_array().cloned().unwrap_or_default() {
        let pts = points(&item["pts"]);
        let closed = match item.get("closed") {
            Some(v) => truthy(v),
            None => {
                pts.len() > 2 && {
                    let (a, b) = (pts[0], pts[pts.len() - 1]);
                    (a.x - b.x).abs() <= 1e-8 + 1e-5 * b.x.abs()
                        && (a.y - b.y).abs() <= 1e-8 + 1e-5 * b.y.abs()
                }
            }
        };
        for m in &maps {
            let q: Vec<Vec2> = pts.iter().map(|&p| m.apply(p)).collect();
            if item["kind"] == "cut" {
                g.internal_cut.push((q.clone(), closed));
            } else {
                g.internal_draw.push((q.clone(), closed));
            }
            if let Some(lab) = item.get("label").filter(|l| truthy(l)) {
                let lab = py_str(lab);
                let at = if closed {
                    let s = q.iter().fold(Vec2::ZERO, |acc, &p| acc + p);
                    Vec2::new(s.x / q.len() as f64, s.y / q.len() as f64)
                } else {
                    let xmax = q.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
                    let ys: Vec<f64> = q.iter().map(|p| p.y).collect();
                    Vec2::new(
                        xmax + 2.0 + text_width(&lab, 3.0, false) / 2.0,
                        pairwise_sum(&ys) / ys.len() as f64,
                    )
                };
                g.labels.push(Label {
                    p: at,
                    lines: vec![(lab, 1.0, false)],
                    size: 3.0,
                    kind: "internal".into(),
                });
            }
        }
    }
    for d in pv["drills"].as_array().cloned().unwrap_or_default() {
        if let Some(p) = point(&d["p"]) {
            for m in &maps {
                g.drills.push((
                    m.apply(p),
                    d.get("d").and_then(Value::as_f64).unwrap_or(3.0),
                ));
            }
        }
    }
    for d in pv["decor"].as_array().cloned().unwrap_or_default() {
        let pts = points(&d["pts"]);
        for m in &maps {
            g.decor.push(Decor {
                kind: d.get("kind").map_or_else(|| "soutache".to_string(), py_str),
                pts: pts.iter().map(|&p| m.apply(p)).collect(),
                width: d.get("width_mm").and_then(Value::as_f64).unwrap_or(3.0),
                closed: d.get("closed").is_some_and(truthy),
            });
        }
    }
    // drills that map onto themselves (on the fold line) once
    let all = g.drills.clone();
    g.drills = all
        .iter()
        .enumerate()
        .filter(|(i, x)| all[..*i].iter().all(|y| norm(x.0 - y.0) > 1e-6))
        .map(|(_, x)| *x)
        .collect();

    // main label block
    let style = raw
        .get("style")
        .filter(|s| truthy(s))
        .map_or_else(|| "STYLE".to_string(), py_str);
    let size = raw["meta"]
        .get("size_label")
        .map_or_else(String::new, py_str);
    let cut_txt = match (mode, qty == 1) {
        (FoldMode::Half, true) => format!("Cut {qty} on fold"),
        (FoldMode::Half, false) => format!("Cut {qty} on fold (2 folded pieces)"),
        (FoldMode::Unfolded, true) => "Cut 1 (shown unfolded)".to_string(),
        (FoldMode::Unfolded, false) => "Cut 2 mirror pair (shown unfolded)".to_string(),
        (FoldMode::None, true) => "Cut 1".to_string(),
        (FoldMode::None, false) => "Cut 2 (mirror pair)".to_string(),
    };
    let mut lines = vec![
        (g.name.clone(), 1.0, true),
        (
            format!("{style}  size {size}").trim().to_string(),
            0.65,
            false,
        ),
        (cut_txt, 0.65, false),
    ];
    if let Some(m) = g.meta.material.as_ref().filter(|m| !m.is_empty()) {
        lines.push((format!("Fabric: {m}"), 0.65, false));
    }
    if !sas.is_empty() {
        let list: Vec<String> = sas.iter().map(|&s| fnum(s, 1)).collect();
        lines.push((format!("SA included: {} mm", list.join("/")), 0.65, false));
    }
    if let Some(r) = raw["meta"].get("round").filter(|r| !r.is_null()) {
        lines.push((format!("Round {}", py_str(r)), 0.65, false));
    }
    if let Some(note) = pv.get("note").filter(|n| truthy(n)) {
        lines.push((py_str(note), 0.65, false));
    }
    let region = Region::new(&g, &sew);
    let (anchor, size) = region.place(&lines, text_size);
    g.labels.insert(
        0,
        Label {
            p: anchor,
            lines,
            size: size.max(2.0),
            kind: "label".into(),
        },
    );
    Ok(g)
}

/// Something a label must keep clear of: a polyline (or point, or closed polygon with its inside) and how
/// far.
struct Obstacle {
    pts: Vec<Vec2>,
    polygon: bool,
    r: f64,
}

/// Where the main label may go: the sew polygon inset by 2 mm, less every obstacle buffered (grain 7, internal
/// lines 4, drills d + 4, fold marks 5, decor half its width + 4).
struct Region<'a> {
    sew: &'a [Vec2],
    inset: f64,
    obstacles: Vec<Obstacle>,
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let l2 = d.x * d.x + d.y * d.y;
    let t = if l2 > 0.0 {
        (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    norm(p - (a + d * t))
}

fn polyline_dist(p: Vec2, pts: &[Vec2], closed: bool) -> f64 {
    if pts.len() == 1 {
        return norm(p - pts[0]);
    }
    let n = pts.len();
    let segs = if closed { n } else { n - 1 };
    (0..segs)
        .map(|i| seg_dist(p, pts[i], pts[(i + 1) % n]))
        .fold(f64::INFINITY, f64::min)
}

fn crosses(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let o = |p: Vec2, q: Vec2, r: Vec2| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

/// Distance from segment `ab` to the axis-aligned box (0 when they meet).
fn seg_box_dist(a: Vec2, b: Vec2, lo: Vec2, hi: Vec2) -> f64 {
    let inside = |p: Vec2| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
    if inside(a) || inside(b) {
        return 0.0;
    }
    let c = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
    if (0..4).any(|i| crosses(a, b, c[i], c[(i + 1) % 4])) {
        return 0.0;
    }
    let mut d = c
        .iter()
        .map(|&q| seg_dist(q, a, b))
        .fold(f64::INFINITY, f64::min);
    for p in [a, b] {
        let dx = (lo.x - p.x).max(0.0).max(p.x - hi.x);
        let dy = (lo.y - p.y).max(0.0).max(p.y - hi.y);
        d = d.min((dx * dx + dy * dy).sqrt());
    }
    d
}

impl<'a> Region<'a> {
    fn new(g: &Geom, sew: &'a [Vec2]) -> Self {
        let mut obstacles = Vec::new();
        if let Some((a, b)) = g.grain {
            obstacles.push(Obstacle {
                pts: vec![a, b],
                polygon: false,
                r: 7.0,
            });
        }
        for (q, closed) in g.internal_draw.iter().chain(&g.internal_cut) {
            obstacles.push(Obstacle {
                pts: q.clone(),
                polygon: *closed && q.len() > 3,
                r: 4.0,
            });
        }
        for &(p, d) in &g.drills {
            obstacles.push(Obstacle {
                pts: vec![p],
                polygon: false,
                r: d + 4.0,
            });
        }
        for (q, _) in &g.fold_lines {
            obstacles.push(Obstacle {
                pts: q.clone(),
                polygon: false,
                r: 5.0,
            });
        }
        for dc in &g.decor {
            obstacles.push(Obstacle {
                pts: dc.pts.clone(),
                polygon: false,
                r: dc.width / 2.0 + 4.0,
            });
        }
        Self {
            sew,
            inset: 2.0,
            obstacles,
        }
    }

    /// Signed clearance: how far `p` is inside the region (negative outside).
    fn clearance(&self, p: Vec2) -> f64 {
        let rings: [&[Vec2]; 1] = [self.sew];
        let mut f = signed_distance(&rings, p) - self.inset;
        for o in &self.obstacles {
            let dist = polyline_dist(p, &o.pts, o.polygon);
            let inside = o.polygon && covers(&[o.pts.as_slice()], p);
            f = f.min(if inside { -dist - o.r } else { dist - o.r });
        }
        f
    }

    /// Whether the box centred on `c` with half sizes `hw`, `hh` lies in the region.
    fn contains_box(&self, c: Vec2, hw: f64, hh: f64) -> bool {
        // the disc of the clearance at the centre decides most boxes at once
        let f = self.clearance(c);
        if f < 0.0 {
            return false;
        }
        if f * f >= hw * hw + hh * hh {
            return true;
        }
        let (lo, hi) = (Vec2::new(c.x - hw, c.y - hh), Vec2::new(c.x + hw, c.y + hh));
        let rings: [&[Vec2]; 1] = [self.sew];
        let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
        if !corners.iter().all(|&q| covers(&rings, q)) {
            return false;
        }
        let n = self.sew.len();
        let near = |a: Vec2, b: Vec2| {
            a.x.min(b.x) - hi.x < self.inset
                && lo.x - a.x.max(b.x) < self.inset
                && a.y.min(b.y) - hi.y < self.inset
                && lo.y - a.y.max(b.y) < self.inset
        };
        if (0..n).any(|i| {
            let (a, b) = (self.sew[i], self.sew[(i + 1) % n]);
            near(a, b) && seg_box_dist(a, b, lo, hi) < self.inset
        }) {
            return false;
        }
        self.obstacles.iter().all(|o| {
            let m = o.pts.len();
            let segs = if o.polygon { m } else { m.saturating_sub(1) };
            let d = if m == 1 {
                seg_box_dist(o.pts[0], o.pts[0], lo, hi)
            } else {
                (0..segs)
                    .map(|i| seg_box_dist(o.pts[i], o.pts[(i + 1) % m], lo, hi))
                    .fold(f64::INFINITY, f64::min)
            };
            d >= o.r && !(o.polygon && covers(&[o.pts.as_slice()], c))
        })
    }

    /// Anchors worth trying, best clearance first: the pole of the region by branch and bound on the
    /// clearance (to 1 mm), then the best evaluated cell of each `best clearance`-sized patch of the plane
    /// with at least half its clearance (up to 48), then the centroid if it is inside. Each with its
    /// clearance.
    fn candidates(&self) -> Vec<(Vec2, f64)> {
        let rings: [&[Vec2]; 1] = [self.sew];
        let seed = interior_point(&rings);
        let mut seen = vec![(seed, self.clearance(seed))];
        let (mut lo, mut hi) = (
            Vec2::new(f64::INFINITY, f64::INFINITY),
            Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for p in self.sew {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        let h0 = (hi.x - lo.x).max(hi.y - lo.y) / 2.0;
        let c0 = Vec2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        let mut best = seen[0].1;
        let mut queue = vec![(c0, h0, self.clearance(c0))];
        let mut iter = 0;
        while let Some(k) = (0..queue.len()).max_by(|&a, &b| {
            let pot = |i: usize| queue[i].2 + queue[i].1 * std::f64::consts::SQRT_2;
            pot(a).total_cmp(&pot(b))
        }) {
            let (c, h, f) = queue.swap_remove(k);
            iter += 1;
            seen.push((c, f));
            best = best.max(f);
            if iter > 4_000 || f + h * std::f64::consts::SQRT_2 - best <= 1.0 {
                continue;
            }
            let h2 = h / 2.0;
            for (dx, dy) in [(-h2, -h2), (h2, -h2), (-h2, h2), (h2, h2)] {
                let q = Vec2::new(c.x + dx, c.y + dy);
                queue.push((q, h2, self.clearance(q)));
            }
        }
        seen.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut out: Vec<(Vec2, f64)> = Vec::new();
        let patch = best.max(1.0);
        for &(c, f) in &seen {
            if f < 0.5 * best || out.len() >= 49 {
                break;
            }
            let key = |p: Vec2| ((p.x / patch).floor(), (p.y / patch).floor());
            if out.iter().all(|&(q, _)| key(q) != key(c)) {
                out.push((c, f));
            }
        }
        let cen = centroid(self.sew);
        let f = self.clearance(cen);
        if f >= 0.0 {
            out.push((cen, f));
        }
        out
    }

    /// The label's anchor and size: where the largest block fits, up to `text_size`. A branch and bound
    /// over anchors: the size that fits at an anchor drops by at most `e / (0.53 · min(wmax, hsum))` when the
    /// anchor moves by `e` in x and y, which bounds a cell from its centre; seeded with the region's pole and
    /// its centroid, cells are split until no cell can beat the best by 2 % of `text_size`. When nothing of
    /// the region is left, the whole sew polygon (as in the reference).
    fn place(&self, lines: &[(String, f64, bool)], text_size: f64) -> (Vec2, f64) {
        let wmax = lines
            .iter()
            .map(|(t, rel, b)| text_width(t, *rel, *b))
            .fold(f64::NEG_INFINITY, f64::max)
            .max(1e-6);
        let hsum: f64 = lines.iter().map(|(_, rel, _)| 1.3 * rel).sum();
        let seeds = self.candidates();
        if seeds.first().is_none_or(|c| c.1 < 0.0) && self.inset > 0.0 {
            let whole = Region {
                sew: self.sew,
                inset: 0.0,
                obstacles: Vec::new(),
            };
            return whole.place(lines, text_size);
        }
        // the largest size whose block fits at c (0 if none)
        let fits = |c: Vec2, s: f64| self.contains_box(c, 0.53 * wmax * s, 0.53 * hsum * s);
        let diag = 0.53 * (wmax * wmax + hsum * hsum).sqrt();
        let size_at = |c: Vec2| {
            let f = self.clearance(c);
            if f < 0.0 || !fits(c, 1e-3) {
                return 0.0;
            }
            if fits(c, text_size) {
                return text_size;
            }
            // a block within the clearance disc fits
            let (mut lo, mut hi) = ((f / diag).clamp(1e-3, text_size), text_size);
            for _ in 0..14 {
                let mid = 0.5 * (lo + hi);
                if fits(c, mid) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            lo
        };
        let slope = 1.0 / (0.53 * wmax.min(hsum));
        let tol = 0.02 * text_size;
        let mut best = (seeds[0].0, size_at(seeds[0].0));
        for &(c, _) in seeds.iter().skip(1).take(4).chain(seeds.last()) {
            let s = size_at(c);
            if s > best.1 + 1e-9 {
                best = (c, s);
            }
        }
        if best.1 < text_size - tol {
            let (mut lo, mut hi) = (
                Vec2::new(f64::INFINITY, f64::INFINITY),
                Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
            );
            for p in self.sew {
                lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
            let e0 = (hi.x - lo.x).max(hi.y - lo.y) / 2.0;
            let c0 = Vec2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
            let bound = |s: f64, e: f64| (s + e * slope).min(text_size);
            let s0 = size_at(c0);
            let mut queue = vec![(c0, e0, s0)];
            let mut iter = 0;
            while let Some(k) = (0..queue.len()).max_by(|&a, &b| {
                bound(queue[a].2, queue[a].1).total_cmp(&bound(queue[b].2, queue[b].1))
            }) {
                let (c, e, s) = queue.swap_remove(k);
                if s > best.1 + 1e-9 {
                    best = (c, s);
                }
                iter += 1;
                if iter > 4_000 || bound(s, e) <= best.1 + tol {
                    if bound(s, e) <= best.1 + tol {
                        // the best cell left cannot beat it: done
                        break;
                    }
                    continue;
                }
                let h = e / 2.0;
                for (dx, dy) in [(-h, -h), (h, -h), (-h, h), (h, h)] {
                    let q = Vec2::new(c.x + dx, c.y + dy);
                    queue.push((q, h, size_at(q)));
                }
            }
        }
        best
    }
}

/// Whether a main label block (`lines` at `size`, centred on `p`) lies in the region the label may use: the
/// sew polygon inset by 2 mm, clear of the grainline (7 mm), internal lines (4), drills (d + 4), fold marks
/// (5) and decor (half its width + 4); the whole sew polygon when nothing of that is left.
#[must_use]
pub fn main_label_fits(g: &Geom, p: Vec2, lines: &[(String, f64, bool)], size: f64) -> bool {
    let region = Region::new(g, &g.sew);
    let region = if region.candidates().first().is_none_or(|c| c.1 < 0.0) {
        Region {
            sew: &g.sew,
            inset: 0.0,
            obstacles: Vec::new(),
        }
    } else {
        region
    };
    let wmax = lines
        .iter()
        .map(|(t, rel, b)| text_width(t, *rel, *b))
        .fold(f64::NEG_INFINITY, f64::max);
    let hsum: f64 = lines.iter().map(|(_, rel, _)| 1.3 * rel).sum();
    region.contains_box(p, 0.53 * wmax * size, 0.53 * hsum * size)
}

/// Area centroid of a ring.
fn centroid(ring: &[Vec2]) -> Vec2 {
    let n = ring.len();
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (p, q) = (ring[i], ring[(i + 1) % n]);
        let w = p.x * q.y - q.x * p.y;
        a += w;
        cx += (p.x + q.x) * w;
        cy += (p.y + q.y) * w;
    }
    if a == 0.0 {
        return ring.first().copied().unwrap_or(Vec2::ZERO);
    }
    Vec2::new(cx / (3.0 * a), cy / (3.0 * a))
}

/// A label block's lines as `(x, y baseline, text, size, bold)` (text centred on `x`).
#[must_use]
pub fn label_texts(
    lb: &Label,
    y_down: bool,
    min_size: f64,
    scale: f64,
) -> Vec<(f64, f64, String, f64, bool)> {
    let sizes: Vec<f64> = lb
        .lines
        .iter()
        .map(|(_, rel, _)| (lb.size * scale * rel).max(min_size))
        .collect();
    let total: f64 = sizes.iter().map(|s| 1.3 * s).sum();
    let sgn = if y_down { 1.0 } else { -1.0 };
    let mut acc = 0.0;
    lb.lines
        .iter()
        .zip(&sizes)
        .map(|((t, _, bold), &s)| {
            let y = lb.p.y + sgn * (-total / 2.0 + acc + 0.15 * s + CAP * s);
            acc += 1.3 * s;
            (lb.p.x, y, ascii_text(t), s, *bold)
        })
        .collect()
}

/// The grainline and its two arrowheads.
#[must_use]
pub fn grain_marks(g: &Geom, arrow: f64) -> Vec<Vec<Vec2>> {
    let Some((a, b)) = g.grain else {
        return Vec::new();
    };
    let t = unit(b - a);
    let n = Vec2::new(-t.y, t.x);
    let w = arrow * 0.4;
    vec![
        vec![a, b],
        vec![b - t * arrow + n * w, b, b - t * arrow - n * w],
        vec![a + t * arrow + n * w, a, a + t * arrow - n * w],
    ]
}

/// The rotation (degrees, counter-clockwise, |·| ≤ 90) that makes the grainline vertical; 0 without one.
#[must_use]
pub fn grain_vertical_rotation(g: &Geom) -> f64 {
    let Some((a, b)) = g.grain else {
        return 0.0;
    };
    let d = b - a;
    let ang = det::atan2(d.y, d.x).to_degrees();
    let mut rot = (90.0 - ang + 180.0).rem_euclid(360.0) - 180.0;
    if rot > 90.0 {
        rot -= 180.0;
    } else if rot < -90.0 {
        rot += 180.0;
    }
    rot
}
