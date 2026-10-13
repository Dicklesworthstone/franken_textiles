//! Native review images for spec 04 §10.9/10.13. These are pixel diagnostics, never cutting files.
//! The existing export stroke font and nearest-pixel sampling deliberately replace OpenCV's
//! TrueType/antialiasing/cubic rasterizer. Geometry and audit documents are not modified.

use std::collections::BTreeSet;

use ftx_core::{Refusal, RefusalCode, Vec2, polyline};
use ftx_export::{
    pole,
    text::{Anchor, ascii_text, stroke_text},
};
use ftx_pattern::doc::{PatternDoc, Piece};
use serde_json::Value;

use crate::{
    Trace,
    raster::{Bgr, Gray, draw, image::encode_png_bgr},
    refs,
};

const LONG: f64 = 1600.0;
const HEADER: usize = 112;
const MAX_PIXELS: usize = 16_000_000;
const MAX_POINTS: usize = 100_000;
const MAX_PIECES: usize = 128;
const RED: [u8; 3] = [35, 35, 215];
const GREY: [u8; 3] = [120, 120, 120];
const PALETTE: [[u8; 3]; 10] = [
    [40, 40, 215],
    [190, 100, 20],
    [30, 140, 30],
    [150, 40, 150],
    [0, 120, 220],
    [130, 110, 0],
    [90, 50, 200],
    [20, 100, 130],
    [190, 40, 110],
    [70, 70, 70],
];

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}
fn budget() -> Refusal {
    Refusal::new(
        RefusalCode::BudgetExceeded,
        "review exceeds its pixel, piece, point or text budget",
    )
}
fn geometry(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message)
}
fn image_ok(image: &Bgr) -> Result<(), Refusal> {
    let pixels = image.w.checked_mul(image.h).ok_or_else(budget)?;
    if pixels == 0 || pixels > MAX_PIXELS {
        return Err(budget());
    }
    if pixels.checked_mul(3) != Some(image.data.len()) {
        return Err(bad("source BGR raster has inconsistent dimensions"));
    }
    Ok(())
}
fn checked(points: &[Vec2]) -> Result<(), Refusal> {
    if points
        .iter()
        .any(|p| !p.is_finite() || p.x.abs() > 1e12 || p.y.abs() > 1e12)
    {
        return Err(geometry(
            "review coordinates must be finite and within +/-1e12 units",
        ));
    }
    Ok(())
}
fn text_ok(text: &str) -> Result<(), Refusal> {
    if text.len() > 4096 {
        Err(budget())
    } else {
        Ok(())
    }
}

struct Canvas(Bgr);
impl Canvas {
    fn new(w: usize, h: usize) -> Result<Self, Refusal> {
        let pixels = w.checked_mul(h).ok_or_else(budget)?;
        if w == 0 || h == 0 || pixels > MAX_PIXELS || w > 8192 || h > 8192 {
            return Err(budget());
        }
        Ok(Self(Bgr {
            w,
            h,
            data: vec![255; 3 * pixels],
        }))
    }
    fn pixel(&mut self, x: i64, y: i64, colour: [u8; 3]) {
        if x >= 0 && y >= 0 && x < self.0.w as i64 && y < self.0.h as i64 {
            let i = 3 * (y as usize * self.0.w + x as usize);
            self.0.data[i..i + 3].copy_from_slice(&colour);
        }
    }
    fn dot(&mut self, p: Vec2, radius: i64, colour: [u8; 3]) {
        let (x, y) = (p.x.round() as i64, p.y.round() as i64);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= radius * radius {
                    self.pixel(x + dx, y + dy, colour);
                }
            }
        }
    }
    fn line(&mut self, a: Vec2, b: Vec2, colour: [u8; 3], radius: i64) {
        // Clip before converting to integers or stepping; an off-canvas grain/mark cannot cause
        // an unbounded raster loop. Layout construction already checked finite coordinates.
        let d = b - a;
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        for (start, delta, end) in [
            (a.x, d.x, self.0.w as f64 - 1.0),
            (a.y, d.y, self.0.h as f64 - 1.0),
        ] {
            if delta == 0.0 {
                if start < 0.0 || start > end {
                    return;
                }
            } else {
                let (u, v) = (-start / delta, (end - start) / delta);
                lo = lo.max(u.min(v));
                hi = hi.min(u.max(v));
            }
        }
        if lo > hi {
            return;
        }
        let (a, b) = (a + d * lo, a + d * hi);
        let n = (b.x - a.x).abs().max((b.y - a.y).abs()).ceil() as usize;
        for i in 0..=n {
            self.dot(a + (b - a) * (i as f64 / n.max(1) as f64), radius, colour);
        }
    }
    fn path(&mut self, pts: &[Vec2], colour: [u8; 3], radius: i64) {
        for p in pts.windows(2) {
            self.line(p[0], p[1], colour, radius);
        }
    }
    fn label(&mut self, s: &str, p: Vec2, cap: f64, colour: [u8; 3], center: bool) {
        let width = ascii_text(s).len() as f64 * cap;
        let x = (if center { p.x - width / 2.0 } else { p.x })
            .max(3.0)
            .min((self.0.w as f64 - width - 3.0).max(3.0));
        let y = p.y.max(cap + 3.0).min(self.0.h as f64 - 3.0);
        // A small opaque halo keeps identifiers readable on dark sheets and over stroke lines.
        for yy in (y - cap - 3.0) as i64..=(y + 3.0) as i64 {
            for xx in (x - 3.0) as i64..=(x + width + 2.0).min(self.0.w as f64) as i64 {
                self.pixel(xx, yy, [255; 3]);
            }
        }
        for path in stroke_text(s, x, y, cap, Anchor::Start, true) {
            self.path(&path, colour, 1);
        }
    }
    fn png(self) -> Result<Vec<u8>, Refusal> {
        encode_png_bgr(&self.0).map_err(bad)
    }
}

#[derive(Clone, Copy)]
struct Transform([[f64; 3]; 2]);
impl Transform {
    fn map(self, p: Vec2) -> Vec2 {
        Vec2::new(
            self.0[0][0] * p.x + self.0[0][1] * p.y + self.0[0][2],
            self.0[1][0] * p.x + self.0[1][1] * p.y + self.0[1][2],
        )
    }
}
fn to_px(piece: &Piece) -> Result<Option<Transform>, Refusal> {
    match piece.extra.get("trace").and_then(|v| v.get("to_px")) {
        None => Ok(None),
        Some(v) => {
            let matrix: [[f64; 3]; 2] = serde_json::from_value(v.clone())
                .map_err(|_| bad("trace.to_px must be a finite 2x3 affine matrix"))?;
            let a = matrix;
            let determinant = a[0][0] * a[1][1] - a[0][1] * a[1][0];
            if a.iter().flatten().any(|v| !v.is_finite())
                || determinant == 0.0
                || !determinant.is_finite()
            {
                return Err(geometry("trace.to_px is non-finite or singular"));
            }
            Ok(Some(Transform(matrix)))
        }
    }
}

struct Panel<'a> {
    piece: &'a Piece,
    outline: Vec<Vec2>,
    edges: Vec<Vec<Vec2>>,
    transform: Transform,
}
struct Layout<'a> {
    panels: Vec<Panel<'a>>,
    low: Vec2,
    scale: f64,
    w: usize,
    h: usize,
    image: Option<&'a Bgr>,
}
fn source_meta(doc: &PatternDoc) -> Value {
    doc.meta
        .as_ref()
        .and_then(|m| m.get("source"))
        .cloned()
        .unwrap_or(Value::Null)
}
impl<'a> Layout<'a> {
    fn new(doc: &'a PatternDoc, image: Option<&'a Bgr>) -> Result<Self, Refusal> {
        if doc.pieces.len() > MAX_PIECES {
            return Err(budget());
        }
        if let Some(image) = image {
            image_ok(image)?;
            let dims: [usize; 2] =
                serde_json::from_value(source_meta(doc)["image_size"].clone())
                    .map_err(|_| bad("an explicit source image needs meta.source.image_size"))?;
            if dims != [image.w, image.h] {
                return Err(bad(
                    "source image dimensions do not match the traced source",
                ));
            }
        }
        let transforms: Vec<_> = doc.pieces.iter().map(to_px).collect::<Result<_, _>>()?;
        let mapped = !transforms.is_empty() && transforms.iter().all(Option::is_some);
        if image.is_some() && !mapped && !doc.pieces.is_empty() {
            return Err(bad(
                "an explicit source image requires trace.to_px on every piece",
            ));
        }
        let (mut panels, mut total, mut ids) = (Vec::new(), 0usize, BTreeSet::new());
        for piece in &doc.pieces {
            text_ok(&piece.id)?;
            if !ids.insert(&piece.id) {
                return Err(bad("duplicate piece id in review"));
            }
            let count = piece
                .edges
                .len()
                .checked_mul(81)
                .and_then(|n| n.checked_add(piece.vertices.0.len()))
                .ok_or_else(budget)?;
            total = total.checked_add(count).ok_or_else(budget)?;
            if total > MAX_POINTS || piece.notches().len() > 1024 {
                return Err(budget());
            }
            let outline = refs::outline(piece, 80).map_err(|e| geometry(e.to_string()))?;
            checked(&outline)?;
            if outline.len() < 3 {
                return Err(geometry("piece needs at least three outline points"));
            }
            let edges = piece
                .edges
                .iter()
                .map(|e| {
                    text_ok(&e.id)?;
                    let pts =
                        refs::edge_points(piece, &e.id, 80).map_err(|e| geometry(e.to_string()))?;
                    checked(&pts)?;
                    Ok(pts)
                })
                .collect::<Result<Vec<_>, Refusal>>()?;
            for (id, p) in piece.vertices.iter() {
                text_ok(id)?;
                checked(&[p])?;
            }
            for notch in piece.notches() {
                if !notch.s.is_finite()
                    || !(0.0..=1.0).contains(&notch.s)
                    || !piece.edges.iter().any(|e| e.id == notch.edge)
                {
                    return Err(bad("invalid review notch edge or fraction"));
                }
            }
            if let Some(g) = &piece.grain {
                checked(
                    &g.points()
                        .ok_or_else(|| bad("grain must have two points"))?,
                )?;
            }
            for m in piece
                .internal
                .iter()
                .chain(piece.trace_marks.iter())
                .flatten()
            {
                total = total.checked_add(m.pts.len()).ok_or_else(budget)?;
                if total > MAX_POINTS {
                    return Err(budget());
                }
                checked(&m.pts)?;
            }
            panels.push(Panel {
                piece,
                outline,
                edges,
                transform: Transform([[1.0, 0.0, 0.0], [0.0, -1.0, 0.0]]),
            });
        }
        let heights: Vec<_> = panels
            .iter()
            .map(|p| {
                let (a, b) = refs::bbox(&p.outline);
                b.y - a.y
            })
            .collect();
        let gap = 0.08 * heights.iter().copied().fold(1.0, f64::max);
        let mut cursor = 0.0;
        for (i, p) in panels.iter_mut().enumerate() {
            let (a, b) = refs::bbox(&p.outline);
            p.transform = if mapped {
                transforms[i].unwrap()
            } else {
                Transform([[1.0, 0.0, cursor - a.x], [0.0, -1.0, b.y]])
            };
            cursor += b.x - a.x + gap;
            let extra: Vec<_> = p
                .piece
                .vertices
                .iter()
                .map(|(_, q)| q)
                .chain(
                    p.piece
                        .grain
                        .as_ref()
                        .and_then(|g| g.points())
                        .into_iter()
                        .flatten(),
                )
                .chain(
                    p.piece
                        .internal
                        .iter()
                        .chain(p.piece.trace_marks.iter())
                        .flatten()
                        .flat_map(|m| m.pts.iter().copied()),
                )
                .map(|q| p.transform.map(q))
                .collect();
            checked(&extra)?;
        }
        let transformed: Vec<_> = panels
            .iter()
            .flat_map(|p| p.outline.iter().map(|&q| p.transform.map(q)))
            .collect();
        checked(&transformed)?;
        let (mut low, mut high) = if transformed.is_empty() {
            (Vec2::ZERO, Vec2::new(800.0, 200.0))
        } else {
            refs::bbox(&transformed)
        };
        if let Some(image) = image {
            let crop: [f64; 4] = serde_json::from_value(source_meta(doc)["crop"].clone())
                .map_err(|_| bad("source image needs meta.source.crop [x,y,w,h]"))?;
            if crop.iter().any(|v| !v.is_finite())
                || crop[0] < 0.0
                || crop[1] < 0.0
                || crop[2] <= 0.0
                || crop[3] <= 0.0
                || crop[0] + crop[2] > image.w as f64
                || crop[1] + crop[3] > image.h as f64
            {
                return Err(bad("source crop is outside the supplied image"));
            }
            low = Vec2::new(crop[0], crop[1]);
            high = low + Vec2::new(crop[2], crop[3]);
        }
        let size = high - low;
        if !size.is_finite() || size.x <= 0.0 || size.y <= 0.0 {
            return Err(geometry("review has a degenerate extent"));
        }
        let margin = 0.03 * size.x.max(size.y);
        low -= Vec2::new(margin, margin);
        high += Vec2::new(margin, margin);
        let scale = LONG / (high - low).x.max((high - low).y);
        let w = ((high.x - low.x) * scale).ceil().max(640.0) as usize;
        let h = ((high.y - low.y) * scale).ceil() as usize + HEADER;
        Ok(Self {
            panels,
            low,
            scale,
            w,
            h,
            image,
        })
    }
    fn screen(&self, p: Vec2) -> Vec2 {
        (p - self.low) * self.scale + Vec2::new(0.0, HEADER as f64)
    }
    fn map(&self, panel: &Panel<'_>, p: Vec2) -> Vec2 {
        self.screen(panel.transform.map(p))
    }
    fn path(&self, panel: &Panel<'_>, pts: &[Vec2]) -> Vec<Vec2> {
        pts.iter().map(|&p| self.map(panel, p)).collect()
    }
    fn canvas(&self) -> Result<Canvas, Refusal> {
        let mut canvas = Canvas::new(self.w, self.h)?;
        if let Some(image) = self.image {
            for y in HEADER..self.h {
                for x in 0..self.w {
                    let q = self.low
                        + Vec2::new(
                            (x as f64 + 0.5) / self.scale,
                            (y as f64 - HEADER as f64 + 0.5) / self.scale,
                        );
                    if q.x >= 0.0 && q.y >= 0.0 && q.x < image.w as f64 && q.y < image.h as f64 {
                        let p = image
                            .at(q.x as usize, q.y as usize)
                            .map(|v| ((u32::from(v) * 55 + 255 * 45 + 50) / 100) as u8);
                        canvas.pixel(x as i64, y as i64, p);
                    }
                }
            }
        }
        Ok(canvas)
    }
}

fn closed(mut ring: Vec<Vec2>) -> Vec<Vec2> {
    if let Some(&p) = ring.first() {
        ring.push(p);
    }
    ring
}
fn arrow(canvas: &mut Canvas, a: Vec2, b: Vec2, colour: [u8; 3]) {
    canvas.line(a, b, colour, 1);
    let d = (b - a).normalized();
    let n = Vec2::new(-d.y, d.x);
    canvas.path(
        &[b - d * 12.0 + n * 5.0, b, b - d * 12.0 - n * 5.0],
        colour,
        1,
    );
}
fn lines(s: &str, width: usize) -> Vec<String> {
    let s = ascii_text(s);
    let mut out = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push(std::mem::take(&mut line));
        }
        for chunk in word.as_bytes().chunks(width.max(1)) {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(std::str::from_utf8(chunk).expect("ASCII"));
            if line.len() >= width {
                out.push(std::mem::take(&mut line));
            }
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// Labelled piece/edge/corner/notch overlay; source pixels are read only from the explicit image.
/// Without an image, affine source coordinates are kept when all pieces have them, otherwise the
/// drawn pieces are packed. Missing scale remains visible; this never establishes metric scale.
pub fn overlay(doc: &PatternDoc, image: Option<&Bgr>) -> Result<Vec<u8>, Refusal> {
    let layout = Layout::new(doc, image)?;
    let mut canvas = layout.canvas()?;
    for (i, p) in layout.panels.iter().enumerate() {
        let colour = PALETTE[i % PALETTE.len()];
        let orientation = polyline::signed_area(&layout.path(p, &p.outline)).signum();
        for (edge, pts) in p.piece.edges.iter().zip(&p.edges) {
            let path = layout.path(p, pts);
            canvas.path(&path, colour, 1);
            let samples = polyline::at_fractions(
                &path,
                &[0.24, 0.25, 0.26, 0.49, 0.5, 0.51, 0.74, 0.75, 0.76],
            );
            for (k, s) in samples.as_chunks::<3>().0.iter().enumerate() {
                let d = (s[2] - s[0]).normalized();
                let n = Vec2::new(-d.y, d.x) * orientation;
                let length = if k == 1 { 9.0 } else { 5.0 };
                canvas.line(s[1] - n * length, s[1] + n * length, colour, 1);
                if k == 1 {
                    canvas.label(&edge.id, s[1] + n * 20.0, 14.0, colour, true);
                }
            }
        }
        for (id, q) in p.piece.vertices.iter() {
            let q = layout.map(p, q);
            canvas.dot(q, 4, colour);
            canvas.label(id, q + Vec2::new(12.0, -12.0), 13.0, colour, false);
        }
        for (n, notch) in p.piece.notches().iter().enumerate() {
            let e = p
                .piece
                .edges
                .iter()
                .position(|e| e.id == notch.edge)
                .unwrap();
            let path = layout.path(p, &p.edges[e]);
            let s = polyline::at_fractions(
                &path,
                &[
                    (notch.s - 0.001).max(0.0),
                    notch.s,
                    (notch.s + 0.001).min(1.0),
                ],
            );
            let d = (s[2] - s[0]).normalized();
            let normal = Vec2::new(-d.y, d.x) * orientation;
            canvas.path(
                &[s[1] - d * 5.0, s[1] + normal * 9.0, s[1] + d * 5.0],
                colour,
                1,
            );
            canvas.label(
                &format!("n{}", n + 1),
                s[1] - normal * 20.0,
                13.0,
                colour,
                true,
            );
        }
        for m in p
            .piece
            .internal
            .iter()
            .chain(p.piece.trace_marks.iter())
            .flatten()
        {
            for pair in layout.path(p, &m.pts).windows(2) {
                let n = ((pair[1] - pair[0]).norm() / 8.0).ceil().min(4096.0) as usize;
                for i in (0..n).step_by(2) {
                    canvas.line(
                        pair[0] + (pair[1] - pair[0]) * (i as f64 / n as f64),
                        pair[0] + (pair[1] - pair[0]) * ((i + 1) as f64 / n as f64),
                        colour,
                        1,
                    );
                }
            }
        }
        if let Some(g) = p.piece.grain.as_ref().and_then(|g| g.points()) {
            let a = layout.map(p, g[0]);
            let b = layout.map(p, g[1]);
            arrow(&mut canvas, a, b, colour);
            arrow(&mut canvas, b, a, colour);
        }
        let ring = layout.path(p, &p.outline);
        let center = pole::pole_of_inaccessibility(&[&ring], 2.0);
        let border = p
            .piece
            .extra
            .get("trace")
            .is_some_and(|t| t["touches_border"] == true);
        let label = format!("{}{}", p.piece.id, if border { " (edge-cut?)" } else { "" });
        canvas.label(&label, center, 24.0, colour, true);
    }
    let status = doc
        .meta
        .as_ref()
        .and_then(|m| m.get("scale_status"))
        .and_then(Value::as_str)
        .unwrap_or("UNKNOWN");
    text_ok(status)?;
    if status.len() > 64 {
        return Err(bad("review scale status is too long"));
    }
    let head = format!(
        "{status} - REVIEW ONLY, NOT FOR CUTTING | piece, corner, edge IDs; ticks at 1/4, 1/2, 3/4. N = notch guess; arrow = grain guess. Uppercase stroke labels; exact IDs in JSON."
    );
    for (i, line) in lines(&head, (layout.w - 24) / 12).iter().enumerate() {
        canvas.label(
            line,
            Vec2::new(12.0, 22.0 + i as f64 * 22.0),
            12.0,
            [40; 3],
            false,
        );
    }
    canvas.png()
}

/// One source-text crop. The ID is data, not a trusted filename; callers must choose safe paths.
#[derive(Debug)]
pub struct LabelCrop {
    /// Exact traced piece ID.
    pub piece_id: String,
    /// Unannotated source crop, outside washed toward white; minimum long side 600 pixels.
    pub png: Vec<u8>,
}

/// Preserve original printed labels inside each stroke-centre polygon, washing only its outside.
/// Rings are the trace's crop-relative polygons, translated once to full-image coordinates.
/// A 16-million-pixel aggregate crop budget refuses rather than dropping pieces or downsampling.
pub fn label_crops(trace: &Trace, image: &Bgr) -> Result<Vec<LabelCrop>, Refusal> {
    image_ok(image)?;
    let data = trace.pattern.to_value();
    let dims: [usize; 2] = serde_json::from_value(data["meta"]["source"]["image_size"].clone())
        .map_err(|_| bad("trace needs source image_size"))?;
    if dims != [image.w, image.h] {
        return Err(bad("label source dimensions do not match the trace"));
    }
    let pieces = data["pieces"]
        .as_array()
        .ok_or_else(|| bad("trace pieces must be an array"))?;
    if pieces.len() != trace.polys.len() || pieces.len() > MAX_PIECES {
        return Err(bad("trace polygon count does not match the pieces"));
    }
    let crop: [f64; 4] = serde_json::from_value(data["meta"]["source"]["crop"].clone())
        .map_err(|_| bad("trace needs source crop"))?;
    if crop.iter().any(|v| !v.is_finite())
        || crop[0] < 0.0
        || crop[1] < 0.0
        || crop[2] <= 0.0
        || crop[3] <= 0.0
        || crop[0] + crop[2] > image.w as f64
        || crop[1] + crop[3] > image.h as f64
    {
        return Err(bad("invalid source crop"));
    }
    let mut images = Vec::new();
    let mut pixels = 0usize;
    let mut points = 0usize;
    for (piece, ring) in pieces.iter().zip(&trace.polys) {
        points = points.checked_add(ring.len()).ok_or_else(budget)?;
        if points > MAX_POINTS {
            return Err(budget());
        }
        checked(ring)?;
        if ring.len() < 3 {
            return Err(geometry("label polygon has fewer than three points"));
        }
        let ring: Vec<_> = ring
            .iter()
            .map(|p| *p + Vec2::new(crop[0], crop[1]))
            .collect();
        let (lo, hi) = refs::bbox(&ring);
        let margin = 0.04 * (hi - lo).x.max((hi - lo).y);
        let low = Vec2::new(
            (lo.x - margin).floor().max(0.0),
            (lo.y - margin).floor().max(0.0),
        );
        let high = Vec2::new(
            (hi.x + margin).ceil().min(image.w as f64),
            (hi.y + margin).ceil().min(image.h as f64),
        );
        let size = high - low;
        if size.x <= 0.0 || size.y <= 0.0 {
            return Err(geometry("label polygon is outside its source"));
        }
        let scale = (600.0 / size.x.max(size.y)).max(1.0);
        let w = (size.x * scale).ceil() as usize;
        let h = (size.y * scale).ceil() as usize;
        pixels = pixels
            .checked_add(w.checked_mul(h).ok_or_else(budget)?)
            .ok_or_else(budget)?;
        if pixels > MAX_PIXELS {
            return Err(budget());
        }
        let mut canvas = Canvas::new(w, h)?;
        let mut mask = Gray::filled(w, h, 0);
        let polygon: Vec<(i64, i64)> = ring
            .iter()
            .map(|p| {
                let q = (*p - low) * scale;
                (q.x.round() as i64, q.y.round() as i64)
            })
            .collect();
        draw::fill_poly(&mut mask, &[polygon], 255);
        for y in 0..h {
            for x in 0..w {
                let q = low + Vec2::new((x as f64 + 0.5) / scale, (y as f64 + 0.5) / scale);
                let mut p = image.at(
                    (q.x as usize).min(image.w - 1),
                    (q.y as usize).min(image.h - 1),
                );
                if mask.at(x, y) == 0 {
                    p = p.map(|v| ((u32::from(v) * 35 + 255 * 65 + 50) / 100) as u8);
                }
                canvas.pixel(x as i64, y as i64, p);
            }
        }
        let id = piece["id"]
            .as_str()
            .ok_or_else(|| bad("piece id must be text"))?;
        text_ok(id)?;
        images.push(LabelCrop {
            piece_id: id.into(),
            png: canvas.png()?,
        });
    }
    Ok(images)
}

fn stem(id: &str) -> &str {
    id.strip_suffix(".R")
        .or_else(|| id.strip_suffix(".L"))
        .unwrap_or(id)
}
fn highlight(layout: &Layout<'_>, reference: &str) -> Vec<Vec<Vec2>> {
    let (id, path) = reference.split_once('/').unwrap_or((reference, ""));
    let (id, span) = id.split_once(':').unwrap_or((id, ""));
    let Some(panel) = layout
        .panels
        .iter()
        .find(|p| p.piece.id == id)
        .or_else(|| layout.panels.iter().find(|p| p.piece.id == stem(id)))
    else {
        return vec![];
    };
    if !path.is_empty() {
        return path
            .split('+')
            .filter_map(|id| {
                panel
                    .piece
                    .edges
                    .iter()
                    .position(|e| e.id == id || e.id == stem(id))
            })
            .map(|i| layout.path(panel, &panel.edges[i]))
            .collect();
    }
    let span = span.split(':').next().unwrap_or("");
    if span == "width" || span == "height" {
        let (a, b) = refs::bbox(&panel.outline);
        let c = (a + b) * 0.5;
        let path = if span == "width" {
            vec![Vec2::new(a.x, c.y), Vec2::new(b.x, c.y)]
        } else {
            vec![Vec2::new(c.x, a.y), Vec2::new(c.x, b.y)]
        };
        return vec![layout.path(panel, &path)];
    }
    for (id, p) in panel.piece.vertices.iter() {
        if let Some(other) = span.strip_prefix(&format!("{id}-"))
            && let Some(q) = panel.piece.vertices.get(other)
        {
            return vec![vec![layout.map(panel, p), layout.map(panel, q)]];
        }
        if id == span {
            return vec![vec![layout.map(panel, p)]];
        }
    }
    vec![closed(layout.path(panel, &panel.outline))]
}

/// A round-0 audit image with grey outlines, red referenced geometry, stacked finding tags and
/// the complete wrapped legend. A structure failure gets a text-only panel, not invented geometry.
/// Unknown references retain their legend entry. The JSON verdict is never recomputed or modified.
pub fn audit_image(
    doc: &PatternDoc,
    report: &Value,
    image: Option<&Bgr>,
) -> Result<Vec<u8>, Refusal> {
    if report["schema"] != "fabric-round0-audit/1" {
        return Err(bad("expected fabric-round0-audit/1"));
    }
    let findings = report["findings"]
        .as_array()
        .ok_or_else(|| bad("audit needs a findings array"))?;
    if findings.len() > 512 {
        return Err(budget());
    }
    let mut reference_count = 0usize;
    for f in findings {
        reference_count = reference_count
            .checked_add(f["refs"].as_array().map_or(0, Vec::len))
            .ok_or_else(budget)?;
        if reference_count > 1024 {
            return Err(budget());
        }
    }
    let (layout, mut canvas) = match Layout::new(doc, image) {
        Ok(l) => {
            let c = l.canvas()?;
            (Some(l), c)
        }
        Err(e) if e.code == RefusalCode::DegenerateGeometry && image.is_none() => {
            let mut c = Canvas::new(1600, 240)?;
            c.label(
                "Geometry unavailable; inspect the structure findings below.",
                Vec2::new(15.0, 170.0),
                16.0,
                RED,
                false,
            );
            (None, c)
        }
        Err(e) => return Err(e),
    };
    if let Some(layout) = &layout {
        for panel in &layout.panels {
            canvas.path(&closed(layout.path(panel, &panel.outline)), GREY, 1);
        }
        let mut tags: Vec<Vec2> = Vec::new();
        for f in findings {
            if f["severity"] == "info" {
                continue;
            }
            let id = f["id"].as_str().unwrap_or("?");
            text_ok(id)?;
            let mut paths = Vec::new();
            for r in f["refs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                text_ok(r)?;
                if let Some(id) = r.strip_prefix("seam:") {
                    if let Some(s) = doc.seams().iter().find(|s| s.id() == Some(id)) {
                        for key in ["a", "b"] {
                            if let Some(side) = s.side_ref(key) {
                                paths.extend(highlight(layout, side));
                            }
                        }
                    }
                } else {
                    paths.extend(highlight(layout, r));
                }
            }
            for path in paths {
                canvas.path(&path, RED, 2);
                if let Some(&p) = path.first() {
                    let mut q = p;
                    while tags.iter().any(|t| (*t - q).norm() < 34.0)
                        && q.y + 34.0 < layout.h as f64
                    {
                        q.y += 34.0;
                    }
                    canvas.line(p, q, RED, 1);
                    canvas.dot(p, 4, RED);
                    canvas.label(id, q, 15.0, RED, false);
                    tags.push(q);
                }
            }
        }
    }
    canvas.label(
        "Round-0 audit - review only, not for cutting",
        Vec2::new(12.0, 25.0),
        15.0,
        [40; 3],
        false,
    );
    let title = format!(
        "T {} S {} M {} | scale basis {} | uppercase stroke labels; exact refs in JSON",
        report["summary"]["T"],
        report["summary"]["S"],
        report["summary"]["M"],
        report["scale_basis"]["kind"].as_str().unwrap_or("UNKNOWN")
    );
    for (i, line) in lines(&title, (canvas.0.w - 24) / 12).iter().enumerate() {
        canvas.label(
            line,
            Vec2::new(12.0, 50.0 + i as f64 * 20.0),
            12.0,
            [40; 3],
            false,
        );
    }
    let mut legend = Vec::new();
    let mut total = 0usize;
    for f in findings {
        let text = format!(
            "{} [{}/{}] {}",
            f["id"].as_str().unwrap_or("?"),
            f["class"].as_str().unwrap_or("?"),
            f["severity"].as_str().unwrap_or("?"),
            f["message"].as_str().unwrap_or("")
        );
        text_ok(&text)?;
        total += text.len();
        if total > 65_536 {
            return Err(budget());
        }
        for s in lines(&text, (canvas.0.w - 24) / 12) {
            legend.push((
                s,
                if f["severity"] == "error" {
                    RED
                } else {
                    [40; 3]
                },
            ));
        }
    }
    let old_height = canvas.0.h;
    let mut result = Canvas::new(canvas.0.w, old_height + 24 + 22 * legend.len())?;
    result.0.data[..canvas.0.data.len()].copy_from_slice(&canvas.0.data);
    for (i, (line, colour)) in legend.iter().enumerate() {
        result.label(
            line,
            Vec2::new(12.0, (old_height + 22 + i * 22) as f64),
            12.0,
            *colour,
            false,
        );
    }
    result.png()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::load_png;
    use ftx_export::pyjson::PyJson;
    use serde_json::json;

    fn document() -> PatternDoc {
        serde_json::from_value(json!({
            "schema":"fabric-pattern/1", "units":"mm",
            "meta":{"scale_status":"UNSCALED", "source":{"image_size":[200,120],"crop":[20,30,120,80]}},
            "pieces":[{"id":"p1", "vertices":{"c1":[10,10],"c2":[80,10],"c3":[80,60],"c4":[10,60]},
                "edges":[{"id":"e1","from":"c1","to":"c2"},{"id":"e2","from":"c2","to":"c3"},
                         {"id":"e3","from":"c3","to":"c4"},{"id":"e4","from":"c4","to":"c1"}],
                "grain":[[25,20],[25,50]], "notches":[{"edge":"e1","s":0.5}],
                "trace_marks":[{"kind":"fold","pts":[[15,15],[15,55]]}],
                "trace":{"to_px":[[1,0,20],[0,-1,100]],"touches_border":false}}],"seams":[]
        })).unwrap()
    }
    fn image() -> Bgr {
        Bgr {
            w: 200,
            h: 120,
            data: (0..200 * 120).flat_map(|_| [20, 40, 80]).collect(),
        }
    }
    fn trace(doc: &PatternDoc) -> Trace {
        Trace {
            pattern: PyJson::parse(&serde_json::to_string(doc).unwrap()).unwrap(),
            diagnostics: PyJson::Null,
            mask: Gray::filled(120, 80, 0),
            polys: vec![vec![
                Vec2::new(10.0, 10.0),
                Vec2::new(80.0, 10.0),
                Vec2::new(80.0, 60.0),
                Vec2::new(10.0, 60.0),
                Vec2::new(10.0, 10.0),
            ]],
        }
    }
    #[test]
    fn rgb_png_roundtrip_and_domain_checks() {
        let img = Bgr {
            w: 2,
            h: 2,
            data: vec![0, 10, 255, 240, 35, 5, 7, 9, 11, 255, 0, 0],
        };
        let bytes = encode_png_bgr(&img).unwrap();
        assert_eq!(load_png(&bytes).unwrap(), img);
        assert_eq!(bytes, encode_png_bgr(&img).unwrap());
        assert!(
            encode_png_bgr(&Bgr {
                w: 0,
                h: 1,
                data: vec![]
            })
            .is_err()
        );
        assert!(
            encode_png_bgr(&Bgr {
                w: 2,
                h: 1,
                data: vec![0; 3]
            })
            .is_err()
        );
        assert!(
            encode_png_bgr(&Bgr {
                w: usize::MAX,
                h: 2,
                data: vec![]
            })
            .is_err()
        );
    }
    #[test]
    fn crop_offset_and_y_flip_are_applied_once() {
        let doc = document();
        let img = image();
        let l = Layout::new(&doc, Some(&img)).unwrap();
        let p = &l.panels[0];
        assert_eq!(
            p.transform.map(Vec2::new(10.0, 10.0)),
            Vec2::new(30.0, 90.0)
        );
        let screen = l.map(p, Vec2::new(10.0, 10.0));
        assert!((screen.x - (30.0 - 16.4) * l.scale).abs() < 1e-9);
        assert!((screen.y - (90.0 - 26.4) * l.scale - HEADER as f64).abs() < 1e-9);
        let c = l.canvas().unwrap();
        assert_eq!(
            c.0.at(screen.x as usize, screen.y as usize),
            [126, 137, 159]
        );
    }
    #[test]
    fn overlay_is_deterministic_and_does_not_scale_or_mutate_the_pattern() {
        let doc = document();
        let before = doc.clone();
        let img = image();
        let a = overlay(&doc, Some(&img)).unwrap();
        assert_eq!(a, overlay(&doc, Some(&img)).unwrap());
        let out = load_png(&a).unwrap();
        assert_eq!(out.w, 1600);
        assert!(out.h > HEADER);
        assert!(out.data.as_chunks::<3>().0.contains(&PALETTE[0]));
        assert_eq!(doc, before);
        assert_eq!(doc.meta.as_ref().unwrap()["scale_status"], "UNSCALED");
    }
    #[test]
    fn source_label_crop_preserves_inside_pixels_and_washes_outside() {
        let doc = document();
        let img = image();
        let t = trace(&doc);
        let labels = label_crops(&t, &img).unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].piece_id, "p1");
        let crop = load_png(&labels[0].png).unwrap();
        assert_eq!(crop.w.max(crop.h), 600);
        assert_eq!(crop.at(crop.w / 2, crop.h / 2), [20, 40, 80]);
        assert_eq!(crop.at(0, 0), [173, 180, 194]);
        assert_eq!(labels[0].png, label_crops(&t, &img).unwrap()[0].png);
    }
    #[test]
    fn packed_unmapped_pieces_do_not_overlap() {
        let mut doc = document();
        doc.pieces[0].extra.clear();
        let mut second = doc.pieces[0].clone();
        second.id = "second".into();
        doc.pieces.push(second);
        let l = Layout::new(&doc, None).unwrap();
        let (_, a) = refs::bbox(&l.path(&l.panels[0], &l.panels[0].outline));
        let (b, _) = refs::bbox(&l.path(&l.panels[1], &l.panels[1].outline));
        assert!(a.x < b.x);
        assert!(overlay(&doc, None).is_ok());
        assert!(overlay(&doc, Some(&image())).is_err());
    }
    #[test]
    fn invalid_source_transforms_and_budgets_refuse() {
        let mut doc = document();
        let img = Bgr {
            w: 100,
            h: 100,
            data: vec![255; 30_000],
        };
        assert!(overlay(&doc, Some(&img)).is_err());
        doc.pieces[0].extra["trace"]["to_px"] = json!([[0, 0, 20], [0, 0, 100]]);
        assert!(overlay(&doc, None).is_err());
        let mut doc = document();
        doc.pieces[0]
            .vertices
            .insert("c1", Vec2::new(f64::NAN, 0.0));
        assert!(overlay(&doc, None).is_err());
        let mut doc = document();
        doc.pieces = vec![doc.pieces[0].clone(); 129];
        assert_eq!(
            overlay(&doc, None).unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
        let mut t = trace(&document());
        t.polys.clear();
        assert!(label_crops(&t, &image()).is_err());
    }
    #[test]
    fn audit_highlights_exact_refs_and_keeps_all_findings() {
        let doc = document();
        let report = json!({"schema":"fabric-round0-audit/1","summary":{"T":1,"S":0,"M":1},"scale_basis":{"kind":"NOMINAL"},"findings":[
            {"id":"F1","class":"M","severity":"error","refs":["p1/e1"],"message":"Wrong length"},
            {"id":"F2","class":"T","severity":"warn","refs":["unknown/e1"],"message":"Unknown reference stays in the legend"}
        ]});
        let before = report.clone();
        let png = audit_image(&doc, &report, None).unwrap();
        let img = load_png(&png).unwrap();
        assert!(img.data.as_chunks::<3>().0.contains(&RED));
        assert_eq!(report, before);
        let layout = Layout::new(&doc, None).unwrap();
        assert!(img.h > layout.h + 40);
        let p = highlight(&layout, "p1.L/e1");
        assert_eq!(p.len(), 1);
        assert_eq!(
            p[0],
            layout.path(&layout.panels[0], &layout.panels[0].edges[0])
        );
        assert!(highlight(&layout, "missing/e1").is_empty());
    }
    #[test]
    fn malformed_geometry_still_gets_a_text_only_audit_not_a_fake_drawing() {
        let mut doc = document();
        doc.pieces[0].vertices.0.clear();
        let report = json!({"schema":"fabric-round0-audit/1","summary":{"T":1,"S":0,"M":0},"scale_basis":{"kind":"NOMINAL"},"findings":[
            {"id":"F1","class":"T","severity":"error","refs":[],"message":"Missing vertices"}
        ]});
        let img = load_png(&audit_image(&doc, &report, None).unwrap()).unwrap();
        assert_eq!(img.w, 1600);
        assert!(img.h > 240);
        assert!(!img.data.as_chunks::<3>().0.contains(&GREY));
    }
}
