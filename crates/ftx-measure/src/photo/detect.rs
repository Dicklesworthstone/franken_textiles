//! Plain-paper detection from oriented BGR pixels (spec 02 A.11). Search masks propose quads;
//! full-resolution brightness profiles, straight-edge support and the rectangle check decide.
//! No image decoder, filesystem, external runtime or body-model dependency lives here.

use ftx_core::{Refusal, RefusalCode, det, py};
use serde_json::{Value, json};

use super::geometry::{Point, convex_clockwise};
use super::{PhotoScale, RECT_OK, Samples, analyze_reference, invalid, paper_fit};

const SEARCH_SIDE: usize = 1200;
/// Maximum decoded image area accepted by the detector; oversized images are refused, not cropped.
pub const MAX_PHOTO_PIXELS: usize = 16_000_000;
const MAX_CANDIDATES: usize = 12;

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn length(a: Point) -> f64 {
    dot(a, a).sqrt()
}
fn area(p: &[Point]) -> f64 {
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .map(|(&a, &b)| cross(a, b))
        .sum::<f64>()
        * 0.5
}
fn percentile(values: &[f64], fraction: f64) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let t = fraction * (v.len() - 1) as f64;
    let i = t as usize;
    v[i] + (t - i as f64) * (v[(i + 1).min(v.len() - 1)] - v[i])
}

struct Raster {
    w: usize,
    h: usize,
    data: Vec<f64>,
}
impl Raster {
    fn sample(&self, p: Point) -> f64 {
        // Replicated image border for profile sampling, not a modification of user geometry.
        let x = p[0].clamp(0.0, (self.w - 1) as f64);
        let y = p[1].clamp(0.0, (self.h - 1) as f64);
        let (ix, iy) = (x as usize, y as usize);
        let (jx, jy) = ((ix + 1).min(self.w - 1), (iy + 1).min(self.h - 1));
        let (fx, fy) = (x - ix as f64, y - iy as f64);
        let row = |y| self.data[y * self.w + ix] * (1.0 - fx) + self.data[y * self.w + jx] * fx;
        row(iy) * (1.0 - fy) + row(jy) * fy
    }

    fn blur(&self, sigma: f64) -> Self {
        let radius = (sigma * 4.0).ceil() as isize;
        let mut kernel: Vec<f64> = (-radius..=radius)
            .map(|i| det::exp(-0.5 * (i as f64 / sigma).powi(2)))
            .collect();
        let sum: f64 = kernel.iter().sum();
        kernel.iter_mut().for_each(|v| *v /= sum);
        let reflect = |i: isize, n: usize| -> usize {
            let period = 2 * (n - 1) as isize;
            let i = i.rem_euclid(period);
            if i >= n as isize {
                (period - i) as usize
            } else {
                i as usize
            }
        };
        let mut temp = vec![0.0; self.data.len()];
        let mut data = vec![0.0; self.data.len()];
        for y in 0..self.h {
            for x in 0..self.w {
                temp[y * self.w + x] = kernel
                    .iter()
                    .enumerate()
                    .map(|(k, v)| {
                        v * self.data
                            [y * self.w + reflect(x as isize + k as isize - radius, self.w)]
                    })
                    .sum();
            }
        }
        for y in 0..self.h {
            for x in 0..self.w {
                data[y * self.w + x] = kernel
                    .iter()
                    .enumerate()
                    .map(|(k, v)| {
                        v * temp[reflect(y as isize + k as isize - radius, self.h) * self.w + x]
                    })
                    .sum();
            }
        }
        Self {
            w: self.w,
            h: self.h,
            data,
        }
    }
}

fn gray(p: [f64; 3]) -> f64 {
    ((p[0] * 3735.0 + p[1] * 19235.0 + p[2] * 9798.0 + 16384.0) / 32768.0).floor()
}

/// Area-resample the search copy; pixel centres follow (x + 0.5)/scale - 0.5.
fn search_image(bgr: &[u8], w: usize, h: usize) -> (Raster, Raster) {
    let s = (SEARCH_SIDE as f64 / w.max(h) as f64).min(1.0);
    let (sw, sh) = (
        (w as f64 * s).round_ties_even() as usize,
        (h as f64 * s).round_ties_even() as usize,
    );
    let (sw, sh) = (sw.max(2), sh.max(2));
    let (sx, sy) = (w as f64 / sw as f64, h as f64 / sh as f64);
    let (mut g, mut sat) = (Vec::with_capacity(sw * sh), Vec::with_capacity(sw * sh));
    for y in 0..sh {
        let (y0, y1) = (y as f64 * sy, (y + 1) as f64 * sy);
        for x in 0..sw {
            let (x0, x1) = (x as f64 * sx, (x + 1) as f64 * sx);
            let mut p = [0.0; 3];
            for yy in y0.floor() as usize..(y1.ceil() as usize).min(h) {
                let dy = (y1.min((yy + 1) as f64) - y0.max(yy as f64)).max(0.0);
                for xx in x0.floor() as usize..(x1.ceil() as usize).min(w) {
                    let weight =
                        dy * (x1.min((xx + 1) as f64) - x0.max(xx as f64)).max(0.0) / (sx * sy);
                    for k in 0..3 {
                        p[k] += weight * f64::from(bgr[3 * (yy * w + xx) + k]);
                    }
                }
            }
            let p = p.map(f64::round_ties_even);
            g.push(gray(p));
            let hi = p.iter().copied().fold(0.0, f64::max);
            let lo = p.iter().copied().fold(255.0, f64::min);
            sat.push(if hi == 0.0 {
                0.0
            } else {
                255.0 * (hi - lo) / hi
            });
        }
    }
    (
        Raster {
            w: sw,
            h: sh,
            data: g,
        }
        .blur(1.5),
        Raster {
            w: sw,
            h: sh,
            data: sat,
        }
        .blur(1.5),
    )
}

fn otsu(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut hist = [0.0; 256];
    for v in values {
        hist[v.round_ties_even().clamp(0.0, 255.0) as usize] += 1.0;
    }
    let n: f64 = hist.iter().sum();
    let total: f64 = hist.iter().enumerate().map(|(i, v)| i as f64 * v).sum();
    let (mut count, mut sum, mut best, mut threshold) = (0.0, 0.0, 0.0, None);
    for (i, v) in hist.iter().enumerate() {
        count += v;
        sum += i as f64 * v;
        if count == 0.0 || count == n {
            continue;
        }
        let delta = sum / count - (total - sum) / (n - count);
        let between = count * (n - count) * delta * delta;
        if between > best {
            best = between;
            threshold = Some(i as f64);
        }
    }
    threshold
}

fn opening(mask: &[u8], w: usize, h: usize) -> Vec<u8> {
    // OpenCV's 5x5 ellipse: 1,5,5,5,1 pixels in successive rows.
    let offsets: Vec<(isize, isize)> = (-2_isize..=2)
        .flat_map(|y| {
            let r = if y.abs() == 2 { 0 } else { 2 };
            (-r..=r).map(move |x| (x, y))
        })
        .collect();
    let apply = |src: &[u8], erode: bool| -> Vec<u8> {
        (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as isize, (i / w) as isize);
                let samples = offsets.iter().map(|(dx, dy)| {
                    let (x, y) = (x + dx, y + dy);
                    if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
                        erode
                    } else {
                        src[y as usize * w + x as usize] != 0
                    }
                });
                u8::from(if erode {
                    samples.clone().all(|v| v)
                } else {
                    samples.clone().any(|v| v)
                })
            })
            .collect()
    };
    apply(&apply(mask, true), false)
}

fn hull(mut points: Vec<Point>) -> Vec<Point> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    let chain = |iter: Box<dyn Iterator<Item = Point>>| {
        let mut out: Vec<Point> = Vec::new();
        for p in iter {
            while out.len() >= 2
                && cross(
                    sub(out[out.len() - 1], out[out.len() - 2]),
                    sub(p, out[out.len() - 1]),
                ) <= 0.0
            {
                out.pop();
            }
            out.push(p);
        }
        out.pop();
        out
    };
    let mut out = chain(Box::new(points.clone().into_iter()));
    out.extend(chain(Box::new(points.into_iter().rev())));
    out
}

fn simplify_open(points: &[Point], tolerance: f64, out: &mut Vec<Point>) {
    let (a, b) = (points[0], points[points.len() - 1]);
    let d = sub(b, a);
    let mut split = (0, tolerance);
    for (i, &p) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let distance = cross(d, sub(p, a)).abs() / length(d).max(1e-12);
        if distance > split.1 {
            split = (i, distance);
        }
    }
    if split.0 == 0 {
        out.push(a);
    } else {
        simplify_open(&points[..=split.0], tolerance, out);
        simplify_open(&points[split.0..], tolerance, out);
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Line {
    p: Point,
    d: Point,
}

fn intersection(a: Line, b: Line) -> Option<Point> {
    let den = cross(a.d, b.d);
    if den.abs() <= 1e-9 * length(a.d) * length(b.d) {
        return None;
    }
    let t = cross(sub(b.p, a.p), b.d) / den;
    let p = [a.p[0] + t * a.d[0], a.p[1] + t * a.d[1]];
    p.iter().all(|v| v.is_finite()).then_some(p)
}

fn hull_quad(points: Vec<Point>) -> Option<[Point; 4]> {
    let mut h = hull(points);
    if h.len() < 4 {
        return None;
    }
    let perimeter: f64 = (0..h.len())
        .map(|i| length(sub(h[i], h[(i + 1) % h.len()])))
        .sum();
    let opposite = (1..h.len())
        .max_by(|&i, &j| length(sub(h[i], h[0])).total_cmp(&length(sub(h[j], h[0]))))?;
    h.push(h[0]);
    let mut p = Vec::new();
    simplify_open(&h[..=opposite], 0.004 * perimeter, &mut p);
    simplify_open(&h[opposite..], 0.004 * perimeter, &mut p);
    while p.len() > 4 {
        let n = p.len();
        let i = (0..n).min_by(|&i, &j| {
            length(sub(p[i], p[(i + 1) % n])).total_cmp(&length(sub(p[j], p[(j + 1) % n])))
        })?;
        let prev = p[(i + n - 1) % n];
        let next = p[(i + 2) % n];
        p[i] = intersection(
            Line {
                p: prev,
                d: sub(p[i], prev),
            },
            Line {
                p: next,
                d: sub(p[(i + 1) % n], next),
            },
        )?;
        p.remove((i + 1) % n);
    }
    if p.len() != 4 {
        return None;
    }
    if area(&p) < 0.0 {
        p.reverse();
    }
    let first = (0..4).min_by(|&i, &j| (p[i][0] + p[i][1]).total_cmp(&(p[j][0] + p[j][1])))?;
    p.rotate_left(first);
    let q: [Point; 4] = p.try_into().ok()?;
    convex_clockwise(&q).then_some(q)
}

fn same_quad(a: &[Point; 4], b: &[Point; 4], tolerance: f64) -> bool {
    (0..4).any(|shift| (0..4).all(|i| length(sub(a[i], b[(i + shift) % 4])) <= tolerance))
}

fn candidates(bgr: &[u8], w: usize, h: usize) -> Result<Vec<[Point; 4]>, Refusal> {
    let (g, sat) = search_image(bgr, w, h);
    let Some(t) = otsu(g.data.iter().copied()) else {
        return Ok(Vec::new());
    };
    let t2 = otsu(g.data.iter().copied().filter(|v| *v > t));
    let mut quads = Vec::new();
    for mode in 0..3 {
        if mode == 2 && t2.is_none() {
            continue;
        }
        let mask: Vec<u8> = g
            .data
            .iter()
            .zip(&sat.data)
            .map(|(&v, &s)| {
                u8::from(v > if mode == 2 { t2.unwrap() } else { t } && (mode != 0 || s <= 60.0))
            })
            .collect();
        let mut mask = opening(&mask, g.w, g.h);
        for seed in 0..mask.len() {
            if mask[seed] == 0 {
                continue;
            }
            let mut stack = vec![seed];
            mask[seed] = 0;
            let (mut count, mut frame) = (0, false);
            let mut boundary = Vec::new();
            while let Some(i) = stack.pop() {
                let (x, y) = (i % g.w, i / g.w);
                count += 1;
                frame |= x <= 1 || y <= 1 || x + 2 >= g.w || y + 2 >= g.h;
                let mut edge = false;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (xx, yy) = (x as isize + dx, y as isize + dy);
                    if xx < 0 || yy < 0 || xx >= g.w as isize || yy >= g.h as isize {
                        edge = true;
                        continue;
                    }
                    let j = yy as usize * g.w + xx as usize;
                    // The original foreground mask is reconstructed, since the traversal marks it.
                    let foreground = g.data[j] > if mode == 2 { t2.unwrap() } else { t }
                        && (mode != 0 || sat.data[j] <= 60.0);
                    edge |= !foreground;
                    if mask[j] != 0 {
                        mask[j] = 0;
                        stack.push(j);
                    }
                }
                if edge {
                    boundary.push([x as f64, y as f64]);
                }
            }
            if frame || (count as f64) < 0.003 * (g.w * g.h) as f64 {
                continue;
            }
            let Some(q) = hull_quad(boundary) else {
                continue;
            };
            if (count as f64) < 0.85 * area(&q) {
                continue;
            }
            let q = q.map(|p| {
                [
                    (p[0] + 0.5) * w as f64 / g.w as f64 - 0.5,
                    (p[1] + 0.5) * h as f64 / g.h as f64 - 0.5,
                ]
            });
            if !quads
                .iter()
                .any(|p| same_quad(p, &q, 2.0 * w as f64 / g.w as f64))
            {
                quads.push(q);
                if quads.len() > MAX_CANDIDATES {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        "more than twelve sheet candidates; use a simpler background or explicit corners",
                    ));
                }
            }
        }
    }
    quads.sort_by(|a, b| area(b).total_cmp(&area(a)));
    Ok(quads)
}

fn weighted_line(points: &[Point], weights: &[f64]) -> Option<Line> {
    if points.len() < 2 {
        return None;
    }
    let sum: f64 = weights.iter().sum();
    let p: Point = std::array::from_fn(|k| {
        points
            .iter()
            .zip(weights)
            .map(|(p, w)| p[k] * w / sum)
            .sum()
    });
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for (&q, &w) in points.iter().zip(weights) {
        let d = sub(q, p);
        xx += w * d[0] * d[0];
        xy += w * d[0] * d[1];
        yy += w * d[1] * d[1];
    }
    if xx + yy <= 1e-12 {
        return None;
    }
    let (s, c) = det::sin_cos(0.5 * det::atan2(2.0 * xy, xx - yy));
    Some(Line { p, d: [c, s] })
}

pub(super) fn line_fit(points: &[Point]) -> Option<Line> {
    weighted_line(points, &vec![1.0; points.len()])
}

pub(super) fn corners_of(lines: &[Line; 4]) -> Option<[Point; 4]> {
    let mut q = [[0.0; 2]; 4];
    for i in 0..4 {
        q[i] = intersection(lines[(i + 3) % 4], lines[i])?;
    }
    convex_clockwise(&q).then_some(q)
}

struct Refined {
    corners: [Point; 4],
    edges: [Vec<Point>; 4],
    rms: f64,
    blur: f64,
}

fn refine(g: &Raster, q: [Point; 4], half: usize) -> Option<Refined> {
    let mut edges: [Vec<Point>; 4] = std::array::from_fn(|_| Vec::new());
    let mut lines = Vec::new();
    let mut widths = Vec::new();
    let mut squares = 0.0;
    for i in 0..4 {
        let delta = sub(q[(i + 1) % 4], q[i]);
        let len = length(delta);
        let d = delta.map(|v| v / len);
        let n = [d[1], -d[0]]; // outward for a clockwise screen quad
        let count = (0.76 * len / 3.0).clamp(16.0, 200.0) as usize;
        let need = 8.max(count / 5);
        let mut rows = Vec::new();
        for j in 0..count {
            let t = 0.12 + 0.76 * j as f64 / (count - 1) as f64;
            let base = [q[i][0] + t * delta[0], q[i][1] + t * delta[1]];
            let profile: Vec<f64> = (0..=2 * half)
                .map(|k| {
                    let offset = k as f64 - half as f64;
                    g.sample([base[0] + offset * n[0], base[1] + offset * n[1]])
                })
                .collect();
            let gradients: Vec<f64> = (1..profile.len() - 1)
                .map(|k| 0.5 * (profile[k - 1] - profile[k + 1]))
                .collect();
            let k = (1..gradients.len() - 1)
                .max_by(|&a, &b| gradients[a].total_cmp(&gradients[b]).then(b.cmp(&a)))?;
            let (g0, g1, g2) = (gradients[k - 1], gradients[k], gradients[k + 1]);
            let den = g0 - 2.0 * g1 + g2;
            let step = 0.5
                * (profile[0] + profile[1]
                    - profile[profile.len() - 2]
                    - profile[profile.len() - 1]);
            if den >= 0.0 || g1 < g0 || g1 < g2 || step < 20.0 {
                continue;
            }
            let offset = (k + 1) as f64 - half as f64 + 0.5 * (g0 - g2) / den;
            let width = step
                / ((2.0 * std::f64::consts::PI).sqrt() * (g1 - (g0 - g2).powi(2) / (8.0 * den)));
            rows.push((
                [base[0] + offset * n[0], base[1] + offset * n[1]],
                g1,
                width,
            ));
        }
        if rows.len() < need {
            return None;
        }
        let threshold = 0.35 * percentile(&rows.iter().map(|r| r.1).collect::<Vec<_>>(), 0.75);
        rows.retain(|r| r.1 > threshold);
        let points: Vec<_> = rows.iter().map(|r| r.0).collect();
        let mut line = line_fit(&points)?;
        for _ in 0..20 {
            let weights: Vec<f64> = points
                .iter()
                .map(|&p| {
                    let r = cross(sub(p, line.p), line.d).abs();
                    if r <= 1.345 { 1.0 } else { 1.345 / r }
                })
                .collect();
            line = weighted_line(&points, &weights)?;
        }
        let residuals: Vec<f64> = points
            .iter()
            .map(|&p| cross(sub(p, line.p), line.d).abs())
            .collect();
        let scatter = 1.4826 * percentile(&residuals, 0.5);
        if scatter > half as f64 / 4.0 {
            return None;
        }
        for (row, r) in rows.iter().zip(residuals) {
            if r <= (3.0 * scatter).max(0.5) {
                edges[i].push(row.0);
                widths.push(row.2);
            }
        }
        if edges[i].len() < need {
            return None;
        }
        let span: Vec<f64> = edges[i].iter().map(|&p| dot(sub(p, q[i]), d)).collect();
        if span.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - span.iter().copied().fold(f64::INFINITY, f64::min)
            < 0.38 * len
        {
            return None;
        }
        line = line_fit(&edges[i])?;
        squares += edges[i]
            .iter()
            .map(|&p| cross(sub(p, line.p), line.d).powi(2))
            .sum::<f64>();
        lines.push(line);
    }
    let corners = corners_of(&lines.try_into().ok()?)?;
    let ratio = area(&corners) / area(&q);
    if !(0.8..1.25).contains(&ratio)
        || corners.iter().any(|p| {
            p[0] < 1.0 || p[1] < 1.0 || p[0] >= (g.w - 1) as f64 || p[1] >= (g.h - 1) as f64
        })
    {
        return None;
    }
    let count: usize = edges.iter().map(Vec::len).sum();
    Some(Refined {
        corners,
        edges,
        rms: (squares / count as f64).sqrt(),
        blur: (percentile(&widths, 0.5).powi(2) - 1.5).max(0.0).sqrt(),
    })
}

/// Detect a blank Letter/A4 sheet in an already oriented, packed 8-bit BGR image (y down).
/// `paper` is `letter`, `a4`, or `auto`. No image or body data is retained after the call: accepted
/// calibrations retain only subpixel edge samples for bootstrap uncertainty.
///
/// No accepted straight-edged candidate, wrong paper or ambiguous paper returns `ok: false`
/// without a usable calibration. Invalid pixels and candidate/image budget overflow are typed
/// refusals. Detection is not proof that a bright rectangle is paper or that landmarks share its plane.
pub fn analyze_plain_sheet(
    bgr: &[u8],
    width: usize,
    height: usize,
    paper: &str,
) -> Result<PhotoScale, Refusal> {
    if !["letter", "a4", "auto"].contains(&paper) {
        return Err(invalid("paper must be letter, a4 or auto"));
    }
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| invalid("photo dimensions overflow"))?;
    if pixels > MAX_PHOTO_PIXELS {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "photo exceeds 16000000 pixels; resize the oriented image and click on that same image",
        ));
    }
    if width < 16 || height < 16 || bgr.len() != pixels * 3 {
        return Err(invalid(
            "photo needs dimensions at least 16x16 and exactly three BGR bytes per pixel",
        ));
    }
    let quads = candidates(bgr, width, height)?;
    let image = [width as f64, height as f64];
    let mut fits = Vec::new();
    if !quads.is_empty() {
        let g = Raster {
            w: width,
            h: height,
            data: bgr
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| gray([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]))
                .collect(),
        }
        .blur(1.0);
        for q in quads {
            let shortest = (0..4)
                .map(|i| length(sub(q[i], q[(i + 1) % 4])))
                .fold(f64::INFINITY, f64::min);
            let half = (0.05 * shortest).round_ties_even().max(8.0) as usize;
            if let Some(f) = refine(&g, q, half).and_then(|f| refine(&g, f.corners, 8))
                && !fits
                    .iter()
                    .any(|p: &Refined| same_quad(&p.corners, &f.corners, 2.0))
            {
                fits.push(f);
            }
        }
    }
    if fits.is_empty() {
        return Ok(PhotoScale {
            calibration: None,
            report: json!({"schema":"fabric-photo-scale/1", "reference":"plain_sheet", "coordinate_source":"detected_edges", "image_size_px":image, "paper_requested":paper, "paper":if paper == "auto" { Value::Null } else { json!(paper) }, "candidates":0, "ok":false, "warnings":["SHEET_NOT_FOUND: no bright four-sided shape with supported straight edges lies wholly inside the frame; show the whole sheet flat on a darker background"], "notes":[]}),
        });
    }
    let mut scored = Vec::new();
    for (i, f) in fits.iter().enumerate() {
        let letter = paper_fit(&f.corners, "letter", image)?.residual;
        let a4 = paper_fit(&f.corners, "a4", image)?.residual;
        let score = match paper {
            "letter" => letter,
            "a4" => a4,
            _ => letter.min(a4),
        };
        scored.push((i, score, letter.min(a4), area(&f.corners)));
    }
    let passing: Vec<_> = scored.iter().filter(|r| r.1 <= RECT_OK).collect();
    let largest = passing.iter().map(|r| r.3).fold(0.0, f64::max);
    let chosen = if passing.is_empty() {
        scored.iter().min_by(|a, b| a.2.total_cmp(&b.2))
    } else {
        passing
            .iter()
            .copied()
            .filter(|r| r.3 >= largest * 0.5)
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
    .expect("at least one scored candidate")
    .0;
    let f = &fits[chosen];
    let mut scale = analyze_reference(
        &json!({"schema":"ftx-photo-reference/1", "reference":"plain_sheet", "paper":paper, "image_size_px":image, "corners_px":f.corners}),
    )?;
    let report = &mut scale.report;
    report.as_object_mut().unwrap().remove("corner_sigma_px");
    report["coordinate_source"] = json!("detected_edges");
    report["candidates"] = json!(fits.len());
    report["edge_fit_rms_px"] = json!(py::round(f.rms, 4));
    report["edge_points"] = json!(f.edges.iter().map(Vec::len).sum::<usize>());
    report["edge_points_by_side"] = json!(f.edges.each_ref().map(Vec::len));
    report["blur_sigma_px_estimate"] = json!(py::round(f.blur, 2));
    report["notes"] = json!([
        "Plain sheet: four corners give no redundant scale evidence. Verify the detected outline, nominal paper size, flatness and darker background.",
        "Scale is exact only on the sheet's plane; lens distortion, a larger backing card or tape along an edge can bias it.",
        "Deterministic floating-point image kernels and Huber reweighting; not OpenCV pixel or random-stream parity."
    ]);
    if report["ok"] == true && passing.len() > 1 {
        report["warnings"].as_array_mut().unwrap().insert(0, json!(format!("SEVERAL_SHEETS: {} rectangles pass; verify the selected corners or isolate the intended sheet", passing.len())));
    }
    if let Some(c) = &mut scale.calibration {
        c.samples = Samples::Edges {
            points: f.edges.clone(),
        };
    }
    Ok(scale)
}
