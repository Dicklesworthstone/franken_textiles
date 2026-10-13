//! The raster trace (`vectorize.py trace`): a pattern-sheet image becomes an UNSCALED `fabric-pattern/1` draft
//! (1 unit = 1 crop px, y up, loops counter-clockwise) with per-piece trace diagnostics and best-effort marks
//! (notches, grainline, dashed fold candidates, inner lines), ported step by step from `trace_image`,
//! `find_piece_contours`, `_px_polygon`, `detect_corners`, `_edges_from_loop`, `detect_marks` and
//! `bridge_dashes`.
//!
//! The raster steps are OpenCV's own (bit-identical, [`crate::raster`]), the stroke-centre offset is GEOS's
//! buffer ([`crate::buffer`]), and the numpy steps keep numpy's evaluation: the fused `interp` of the
//! resampling, `small_correlate` for the smoothing, `argsort`'s tie order for the corners, float32 medians
//! and percentiles of distance values. Where the reference goes through Accelerate (`np.cov` and `eigh` for a
//! mark's axis, `svd` for an edge's straightness, BLAS `dot`s) the values agree to about 1e-16, not bit for
//! bit; they decide thresholds and then get rounded (3 decimals for points), so a draft differs only where a
//! value sits on such a boundary.

use ftx_core::curve::{Curve, abs_to_rel, curve_points};
use ftx_core::{Vec2, det, np, polyline, py};
use ftx_export::pyjson::PyJson;
use ftx_pattern::Pattern;

use crate::buffer::{Join, Polygon, buffer, line_distance, line_project_normalized};
use crate::geos;
use crate::raster::color;
use crate::raster::contours::{self, Mode, Pt};
use crate::raster::distance::{self, GrayF};
use crate::raster::draw;
use crate::raster::hough::hough_lines_p;
use crate::raster::image::{Bgr, Gray};
use crate::raster::morph::{self, Kernel};
use crate::raster::threshold;

/// The channels `--channel` accepts.
pub const CHANNELS: [&str; 10] = [
    "auto", "bgdist", "dark", "light", "gray", "sat", "r", "g", "b", "edge",
];

const PI: f64 = std::f64::consts::PI;
const RAD_TO_DEG: f64 = 180.0 / PI;

/// `trace_image`'s keyword arguments, with the reference's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceOptions {
    /// `x, y, w, h` in source pixels.
    pub crop: Option<[f64; 4]>,
    /// Outer contours smaller than this fraction of the crop are dropped.
    pub min_area_frac: f64,
    /// One of [`CHANNELS`].
    pub channel: String,
    /// Force (`Some(true)`) or forbid (`Some(false)`) flipping the mask; `None` decides from the frame.
    pub invert: Option<bool>,
    /// `"otsu"` (global) or `"adaptive"` (local Gaussian foreground threshold).
    pub threshold: String,
    /// Background `R, G, B` (overrides the frame estimate).
    pub bg_rgb: Option<[f64; 3]>,
    /// Outline width in px.
    pub stroke: Option<f64>,
    /// Closing kernel px.
    pub close_px: i64,
    /// Opening kernel px on each solid piece.
    pub open_px: Option<i64>,
    /// Sharp-corner turning threshold.
    pub corner_deg: f64,
    /// Rounded-corner threshold over a 3× window (0 = off).
    pub soft_deg: f64,
    /// Straight-run tolerance as a fraction of piece size.
    pub simplify_frac: f64,
    /// Contour smoothing sigma in px.
    pub smooth_px: f64,
    /// Source-pixel rectangles painted with the background first.
    pub erase: Vec<[f64; 4]>,
    /// Detect marks.
    pub marks: bool,
    /// Bridge dashed straight runs.
    pub dashes: bool,
}

impl Default for TraceOptions {
    fn default() -> Self {
        Self {
            crop: None,
            min_area_frac: 0.01,
            channel: "auto".into(),
            invert: None,
            threshold: "otsu".into(),
            bg_rgb: None,
            stroke: None,
            close_px: 3,
            open_px: None,
            corner_deg: 35.0,
            soft_deg: 70.0,
            simplify_frac: 0.003,
            smooth_px: 1.2,
            erase: Vec::new(),
            marks: true,
            dashes: true,
        }
    }
}

/// One accepted outer contour (`find_piece_contours`'s dicts).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Border pixels.
    pub contour: Vec<Pt>,
    /// `"ring"` (outline strokes) or `"fill"` (solid piece).
    pub mode: &'static str,
    /// Stroke width, rounded to 3 decimals.
    pub stroke_px: f64,
    /// How the width was found: `given`, `area`, `distance-transform` or `none`.
    pub stroke_how: &'static str,
    /// Ink fraction of the hole-filled region, rounded to 3 decimals.
    pub fg_frac: f64,
    /// The opening kernel used.
    pub open_px: i64,
}

/// The result of [`trace_image`].
#[derive(Clone, Debug)]
pub struct Trace {
    /// The draft (`fabric-pattern/1`), in the reference's key order.
    pub pattern: PyJson,
    /// `retries`, `dash_bridged_pieces`, `threshold`, `bg_rgb`, `n_candidates`.
    pub diagnostics: PyJson,
    /// The binarized mask the pieces came from.
    pub mask: Gray,
    /// Each piece's stroke-centre polygon (crop px, y down, closed).
    pub polys: Vec<Vec<Vec2>>,
}

fn f(x: f64) -> PyJson {
    PyJson::Float(x)
}

fn int(x: i64) -> PyJson {
    PyJson::Int(x)
}

fn pair(p: [f64; 2]) -> PyJson {
    PyJson::floats(&p)
}

fn usize_i64(x: usize) -> i64 {
    i64::try_from(x).unwrap_or(i64::MAX)
}

/// Python `round(x)` to an int (ties to even).
fn round_int(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// Python's float `%` (the result takes the divisor's sign; a zero result is +0).
fn py_mod(x: f64, m: f64) -> f64 {
    let r = x % m;
    if r == 0.0 {
        0.0f64.copysign(m)
    } else if (m < 0.0) != (r < 0.0) {
        r + m
    } else {
        r
    }
}

/// The first index of the largest value (numpy `argmax`, Python `max` with a key).
fn first_max_by<T, K: PartialOrd>(items: &[T], key: impl Fn(&T) -> K) -> usize {
    let mut best = 0;
    for i in 1..items.len() {
        if key(&items[i]) > key(&items[best]) {
            best = i;
        }
    }
    best
}

/// The first index of the smallest value (Python `min` with a key).
fn first_min_by<T, K: PartialOrd>(items: &[T], key: impl Fn(&T) -> K) -> usize {
    let mut best = 0;
    for i in 1..items.len() {
        if key(&items[i]) < key(&items[best]) {
            best = i;
        }
    }
    best
}

fn to_i64(c: &[Pt]) -> Vec<(i64, i64)> {
    c.iter()
        .map(|&(x, y)| (i64::from(x), i64::from(y)))
        .collect()
}

/// `_odd(n, lo)`: `round(n)`, at least `lo`, made odd upwards.
fn odd(n: f64, lo: i64) -> i64 {
    let n = lo.max(round_int(n));
    if n % 2 != 0 { n } else { n + 1 }
}

// ================================================================================== image -> mask

/// `_mode_color`: the median colour of the most common 4-bit-per-channel bucket.
///
/// # Errors
/// No pixels (numpy's "attempt to get argmax of an empty sequence").
pub fn mode_color(pix: &[[u8; 3]]) -> Result<[f64; 3], String> {
    if pix.is_empty() {
        return Err("attempt to get argmax of an empty sequence".into());
    }
    let key = |p: &[u8; 3]| {
        let q = p.map(|v| u32::from(v >> 4));
        q[0] * 256 + q[1] * 16 + q[2]
    };
    let mut counts = vec![0usize; 4096];
    for p in pix {
        counts[key(p) as usize] += 1;
    }
    // np.unique sorts the keys; argmax takes the first (smallest) key with the most pixels
    let best = first_max_by(&counts, |&c| c) as u32;
    let mut out = [0.0; 3];
    for (ch, o) in out.iter_mut().enumerate() {
        let vals: Vec<f64> = pix
            .iter()
            .filter(|p| key(p) == best)
            .map(|p| f64::from(p[ch]))
            .collect();
        *o = np::median(&vals);
    }
    Ok(out)
}

fn border_pixels(img: &Bgr, b: usize) -> Vec<[u8; 3]> {
    let (w, h) = (img.w, img.h);
    let (bh, bw) = (b.min(h), b.min(w));
    let mut pix = Vec::new();
    for y in (0..bh).chain(h - bh..h) {
        pix.extend((0..w).map(|x| img.at(x, y)));
    }
    for cols in [0..bw, w - bw..w] {
        for y in 0..h {
            pix.extend(cols.clone().map(|x| img.at(x, y)));
        }
    }
    pix
}

/// `estimate_background`: the dominant colour (BGR) of the 3-pixel frame.
///
/// # Errors
/// An empty image.
pub fn estimate_background(img: &Bgr) -> Result<[f64; 3], String> {
    mode_color(&border_pixels(img, 3))
}

/// `np.uint8(np.clip(bg, 0, 255))`: clipped, then truncated.
fn bg_u8(bg: [f64; 3]) -> [u8; 3] {
    bg.map(|v| v.clamp(0.0, 255.0) as u8)
}

fn plane(img: &Bgr, ch: usize) -> Gray {
    Gray {
        w: img.w,
        h: img.h,
        data: img.data.iter().skip(ch).step_by(3).copied().collect(),
    }
}

/// `make_channel`: the single channel to threshold and whether "foreground is bright" is known.
///
/// # Errors
/// An unknown channel.
pub fn make_channel(img: &Bgr, channel: &str, bg: [f64; 3]) -> Result<(Gray, bool), String> {
    let invert = |g: Gray| Gray {
        data: g.data.iter().map(|v| 255 - v).collect(),
        ..g
    };
    Ok(match channel {
        "auto" | "bgdist" => (color::lab_distance(img, bg_u8(bg)), true),
        "dark" => (invert(color::bgr_to_gray(img)), true),
        "light" => (color::bgr_to_gray(img), true),
        "gray" => (color::bgr_to_gray(img), false),
        "sat" => (color::bgr_saturation(img), false),
        "r" | "g" | "b" => (plane(img, "bgr".find(channel).unwrap_or(0)), false),
        "edge" => {
            let lab = color::bgr_to_lab(img);
            let k = Kernel::rect(3, 3);
            let grads: Vec<Gray> = (0..3)
                .map(|i| morph::gradient(&plane(&lab, i), &k))
                .collect();
            let data = (0..img.w * img.h)
                .map(|i| grads.iter().map(|g| g.data[i]).max().unwrap_or(0))
                .collect();
            (
                Gray {
                    w: img.w,
                    h: img.h,
                    data,
                },
                true,
            )
        }
        _ => {
            let names: Vec<String> = CHANNELS.iter().map(|c| py::repr_str(c)).collect();
            return Err(format!(
                "unknown channel {} (choose from ({}))",
                py::repr_str(channel),
                names.join(", ")
            ));
        }
    })
}

/// `binarize`: an Otsu or adaptive mask (255 = ink / piece), with Otsu's frame deciding
/// polarity in both modes, then closed.
///
/// # Errors
/// A malformed raster or an unknown method.
pub fn binarize(
    ch: &Gray,
    method: &str,
    fg_bright_known: bool,
    invert: Option<bool>,
    close_px: i64,
) -> Result<(Gray, PyJson), String> {
    if ch.w == 0 || ch.h == 0 || ch.w.checked_mul(ch.h) != Some(ch.data.len()) {
        return Err("binarize needs a nonempty, correctly sized grayscale raster".into());
    }
    let (t, mut m) = threshold::otsu_binary(ch);
    let (w, h) = (m.w, m.h);
    let (bh, bw) = (2.min(h), 2.min(w));
    let (mut fg, mut total) = (0usize, 0usize);
    let mut count = |x: usize, y: usize| {
        total += 1;
        fg += usize::from(m.data[y * w + x] > 0);
    };
    for y in (0..bh).chain(h - bh..h) {
        (0..w).for_each(|x| count(x, y));
    }
    for cols in [0..bw, w - bw..w] {
        for y in 0..h {
            cols.clone().for_each(|x| count(x, y));
        }
    }
    let border_fg = fg as f64 / total as f64;
    let flip = invert.unwrap_or(!fg_bright_known && border_fg > 0.5);
    match method {
        "otsu" => {
            if flip {
                m.data.iter_mut().for_each(|v| *v = 255 - *v);
            }
        }
        "adaptive" => {
            let inverted;
            let source = if flip {
                inverted = Gray {
                    w,
                    h,
                    data: ch.data.iter().map(|v| 255 - v).collect(),
                };
                &inverted
            } else {
                ch
            };
            m = threshold::adaptive_gaussian(source)?;
        }
        _ => return Err("threshold must be 'otsu' or 'adaptive'".into()),
    }
    if close_px >= 2 {
        let k = Kernel::ellipse(close_px as usize, close_px as usize);
        m = morph::close(&m, &k);
    }
    let thr = PyJson::dict(vec![
        ("otsu_threshold", f(t)),
        ("flipped", PyJson::Bool(flip)),
        ("frame_fg_frac", f(py::round(border_fg, 3))),
        ("method", PyJson::str(method)),
    ]);
    Ok((m, thr))
}

// ================================================================================== mask -> pieces

/// `_stroke_width`: mean ring width `2(Ao − Ai)/(Po + Pi)`, perimeters of the contours simplified to 0.7 px.
fn stroke_width(outer: &[Pt], hole: &[Pt]) -> f64 {
    let ao = contours::area(outer);
    let po = contours::arc_length(&contours::approx_poly_dp(outer, 0.7, true), true);
    let ai = contours::area(hole);
    let pi = contours::arc_length(&contours::approx_poly_dp(hole, 0.7, true), true);
    2.0 * (ao - ai) / (po + pi).max(1e-9)
}

/// `cv2.dilate(dt, ones(3, 3))` on float32: the 3×3 maximum (pixels outside never count).
fn dilate3(img: &GrayF) -> Vec<f32> {
    let (w, h) = (img.w, img.h);
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut m = f32::NEG_INFINITY;
            for yy in y.saturating_sub(1)..(y + 2).min(h) {
                for xx in x.saturating_sub(1)..(x + 2).min(w) {
                    m = m.max(img.data[yy * w + xx]);
                }
            }
            out[y * w + x] = m;
        }
    }
    out
}

/// `_stroke_width_dt`: twice the median ridge of the ring's distance transform, minus one (float32).
fn stroke_width_dt(ring: &Gray) -> f64 {
    let dt = distance::distance_l2(ring, 5);
    let dil = dilate3(&dt);
    let vals: Vec<f32> = dt
        .data
        .iter()
        .zip(&dil)
        .filter(|&(&d, &m)| d > 0.0 && d >= m - 1e-6f32)
        .map(|(&d, _)| d)
        .collect();
    if vals.is_empty() {
        2.0
    } else {
        f64::from(2.0f32 * np::median_f32(&vals) - 1.0f32)
    }
}

/// `find_piece_contours`: outer contours (`RETR_CCOMP`) above the area floor, largest first, classified ring
/// or fill, each solid region opened then closed with a kernel just wider than the outline.
#[must_use]
pub fn find_piece_contours(
    mask: &Gray,
    min_area_frac: f64,
    stroke: Option<f64>,
    open_px: Option<i64>,
) -> Vec<Entry> {
    let (w, h) = (mask.w, mask.h);
    let stroke = stroke.filter(|&s| s != 0.0); // the reference tests `if stroke:`
    let found = contours::find_contours(mask, Mode::CComp);
    let (cs, hier) = (&found.contours, &found.hierarchy);
    let areas: Vec<f64> = cs.iter().map(|c| contours::area(c)).collect();
    let amin = min_area_frac * (h * w) as f64;
    let mut tops: Vec<usize> = (0..cs.len())
        .filter(|&i| hier[i][3] < 0 && areas[i] >= amin)
        .collect();
    tops.sort_by(|&a, &b| areas[b].total_cmp(&areas[a]));
    let mut occupied = vec![false; w * h];
    let mut out = Vec::new();
    for i in tops {
        let c = &cs[i];
        let (m00, m10, m01) = contours::moments(c);
        let (cx, cy) = ((m10 / m00.max(1e-9)) as i64, (m01 / m00.max(1e-9)) as i64);
        if (0..usize_i64(h)).contains(&cy)
            && (0..usize_i64(w)).contains(&cx)
            && occupied[cy as usize * w + cx as usize]
        {
            continue; // a large mark inside an accepted piece
        }
        let mut solid = Gray::filled(w, h, 0);
        draw::fill_poly(&mut solid, &[to_i64(c)], 255);
        let n_solid = solid.data.iter().filter(|&&v| v != 0).count().max(1);
        let both = mask
            .data
            .iter()
            .zip(&solid.data)
            .filter(|&(&a, &b)| a != 0 && b != 0)
            .count();
        let fg_frac = both as f64 / n_solid as f64;
        let ring = fg_frac < 0.5;
        let mut holes = Vec::new();
        let mut j = hier[i][2];
        while j >= 0 {
            holes.push(j as usize);
            j = hier[j as usize][0];
        }
        let (wd, how) = match (ring, stroke) {
            (_, Some(s)) => (s, "given"),
            (false, None) => (0.0, "none"),
            (true, None) => {
                let mut wd = 0.0;
                let mut how = "area";
                if !holes.is_empty() {
                    let hole = holes[first_max_by(&holes, |&k| areas[k])];
                    if areas[hole] > 0.4 * areas[i] {
                        wd = stroke_width(c, &cs[hole]);
                    }
                }
                if !(0.8 <= wd && wd <= 0.06 * areas[i].sqrt()) {
                    let ring_px = Gray {
                        w,
                        h,
                        data: mask
                            .data
                            .iter()
                            .zip(&solid.data)
                            .map(|(&a, &b)| u8::from(a != 0 && b != 0))
                            .collect(),
                    };
                    wd = stroke_width_dt(&ring_px);
                    how = "distance-transform";
                }
                (wd, how)
            }
        };
        let k = open_px.unwrap_or_else(|| if ring { odd(1.5 * wd + 1.0, 3) } else { 5 });
        let solid_o = if k >= 2 {
            let se = Kernel::ellipse(k as usize, k as usize);
            morph::close(&morph::open(&solid, &se), &se)
        } else {
            solid.clone()
        };
        for c2 in contours::find_contours(&solid_o, Mode::External).contours {
            if contours::area(&c2) >= amin {
                out.push(Entry {
                    contour: c2,
                    mode: if ring { "ring" } else { "fill" },
                    stroke_px: py::round(wd, 3),
                    stroke_how: how,
                    fg_frac: py::round(fg_frac, 3),
                    open_px: k,
                });
            }
        }
        for (o, &s) in occupied.iter_mut().zip(&solid.data) {
            *o |= s != 0;
        }
    }
    out
}

/// `_resample_closed`: `n = max(16, round(L/ds))` points at arclength `i·L/n` of the closed loop.
#[must_use]
pub fn resample_closed(p: &[Vec2], ds: f64) -> Vec<Vec2> {
    let mut q = p.to_vec();
    q.push(p[0]);
    let s = polyline::cumulative(&q);
    let total = s[s.len() - 1];
    let n = round_int(total / ds).max(16) as usize;
    let step = total / n as f64;
    let (xs, ys): (Vec<f64>, Vec<f64>) = q.iter().map(|p| (p.x, p.y)).unzip();
    (0..n)
        .map(|i| {
            let t = i as f64 * step + 0.0;
            Vec2::new(np::interp(t, &s, &xs), np::interp(t, &s, &ys))
        })
        .collect()
}

/// `_smooth_closed`: a Gaussian of `sigma` samples, wrapped.
#[must_use]
pub fn smooth_closed(p: &[Vec2], sigma: f64) -> Vec<Vec2> {
    if sigma <= 0.0 || p.len() < 8 {
        return p.to_vec();
    }
    let r = (3.0 * sigma).ceil() as usize;
    let mut k: Vec<f64> = (-(usize_i64(r))..=usize_i64(r))
        .map(|j| {
            let u = j as f64 / sigma;
            det::exp(-0.5 * (u * u))
        })
        .collect();
    let sum = np::pairwise_sum(&k);
    k.iter_mut().for_each(|v| *v /= sum);
    let n = p.len();
    let ext: Vec<Vec2> = p[n.saturating_sub(r)..]
        .iter()
        .chain(p)
        .chain(&p[..r.min(n)])
        .copied()
        .collect();
    let (xs, ys): (Vec<f64>, Vec<f64>) = ext.iter().map(|p| (p.x, p.y)).unzip();
    np::convolve_valid(&xs, &k)
        .into_iter()
        .zip(np::convolve_valid(&ys, &k))
        .map(|(x, y)| Vec2::new(x, y))
        .collect()
}

/// `_px_polygon`: the piece's cut/stitch line (crop px, y down, rings closed) after the stroke-centre
/// correction: the cleaned outline (`buffer(0)`) offset to the stroke centre, its largest part.
///
/// # Errors
/// The offset erodes the piece away, or a topology the buffer refuses.
pub fn px_polygon(e: &Entry, smooth_px: f64) -> Result<Polygon, String> {
    let p: Vec<Vec2> = e
        .contour
        .iter()
        .map(|&(x, y)| Vec2::new(f64::from(x), f64::from(y)))
        .collect();
    let p = smooth_closed(&resample_closed(&p, 1.0), smooth_px);
    let mut polys = buffer(&[Polygon::new(p)], 0.0, Join::Round(16))?;
    let w = e.stroke_px;
    // contours run through boundary-pixel centres: the region edge is 0.5 px further out; for rings the two
    // half-pixel corrections cancel
    let off = if e.mode == "ring" {
        -w / 2.0
    } else {
        0.5 - w / 2.0
    };
    if off.abs() > 1e-9 {
        polys = buffer(&polys, off, Join::Mitre(4.0))?;
    }
    if polys.is_empty() {
        return Err(format!(
            "a {} piece vanishes under its {off} px stroke-centre offset",
            e.mode
        ));
    }
    let k = first_max_by(&polys, Polygon::area);
    Ok(polys.swap_remove(k))
}

// ================================================================================== corners and edges

/// `_turning`: the signed turn (degrees) between the chords `k` samples back and ahead.
fn turning(q: &[Vec2], k: usize) -> Vec<f64> {
    let n = usize_i64(q.len());
    let at = |i: i64| q[i.rem_euclid(n) as usize];
    (0..n)
        .map(|i| {
            let (k, p) = (usize_i64(k), at(i));
            let a = p - at(i - k);
            let b = at(i + k) - p;
            det::atan2(a.x * b.y - a.y * b.x, a.x * b.x + a.y * b.y) * RAD_TO_DEG
        })
        .collect()
}

fn circ(i: usize, j: usize, n: usize) -> usize {
    let (i, j, n) = (usize_i64(i), usize_i64(j), usize_i64(n));
    (i - j).rem_euclid(n).min((j - i).rem_euclid(n)) as usize
}

/// `detect_corners`: corner indices (sorted), each one's kind (`corner`, `soft`, `split`) and the turn at L.
#[must_use]
pub fn detect_corners(
    q: &[Vec2],
    ds: f64,
    size: f64,
    corner_deg: f64,
    soft_deg: f64,
) -> (Vec<usize>, Vec<Option<&'static str>>, Vec<f64>) {
    let n = q.len();
    let k = round_int(8.0f64.max(0.02 * size) / ds).max(2) as usize;
    let turn = turning(q, k);
    let absd: Vec<f64> = turn.iter().map(|t| t.abs()).collect();
    let mut chosen: Vec<usize> = Vec::new();
    let mut kinds: Vec<Option<&'static str>> = vec![None; n];
    let neg: Vec<f64> = absd.iter().map(|a| -a).collect();
    for i in np::argsort(&neg) {
        if absd[i] < corner_deg {
            break;
        }
        if chosen.iter().all(|&j| circ(i, j, n) > k) {
            chosen.push(i);
            kinds[i] = Some("corner");
        }
    }
    let k3 = 3 * k;
    if soft_deg != 0.0 && n > 4 * k3 {
        let t3: Vec<f64> = turning(q, k3).iter().map(|t| t.abs()).collect();
        let neg3: Vec<f64> = t3.iter().map(|a| -a).collect();
        for i in np::argsort(&neg3) {
            if t3[i] < soft_deg {
                break;
            }
            if chosen.iter().all(|&j| circ(i, j, n) > k3) {
                chosen.push(i);
                kinds[i] = Some("soft");
            }
        }
    }
    while chosen.len() < 3 {
        let i = if chosen.is_empty() {
            first_max_by(q, |p| p.y)
        } else {
            let dist: Vec<usize> = (0..n)
                .map(|idx| chosen.iter().map(|&j| circ(idx, j, n)).min().unwrap_or(0))
                .collect();
            first_max_by(&dist, |&d| d)
        };
        chosen.push(i);
        kinds[i] = Some("split");
    }
    chosen.sort_unstable();
    (chosen, kinds, turn)
}

/// numpy `eigh` of a symmetric 2×2 `[[a, b], [b, c]]`: the eigenvector of the larger eigenvalue as
/// `dsyevd` → `dsteqr` → `dlaev2` signs it (or the unit axis when `b` is negligible).
fn eigh2_top(a: f64, b: f64, c: f64) -> Vec2 {
    let eps = f64::EPSILON / 2.0;
    if b == 0.0 || b.abs() <= (a.abs().sqrt() * c.abs().sqrt()) * eps {
        return if c < a {
            Vec2::new(1.0, 0.0)
        } else {
            Vec2::new(0.0, 1.0)
        };
    }
    let (sm, df) = (a + c, a - c);
    let (adf, tb) = (df.abs(), b + b);
    let ab = tb.abs();
    let (acmx, acmn) = if a.abs() > c.abs() { (a, c) } else { (c, a) };
    let rt = if adf > ab {
        adf * (1.0 + (ab / adf) * (ab / adf)).sqrt()
    } else if adf < ab {
        ab * (1.0 + (adf / ab) * (adf / ab)).sqrt()
    } else {
        ab * 2.0f64.sqrt()
    };
    let (rt1, rt2, sgn1) = if sm < 0.0 {
        let rt1 = 0.5 * (sm - rt);
        (rt1, (acmx / rt1) * acmn - (b / rt1) * b, -1)
    } else if sm > 0.0 {
        let rt1 = 0.5 * (sm + rt);
        (rt1, (acmx / rt1) * acmn - (b / rt1) * b, 1)
    } else {
        (0.5 * rt, -0.5 * rt, 1)
    };
    let (cs, sgn2) = if df >= 0.0 {
        (df + rt, 1)
    } else {
        (df - rt, -1)
    };
    let (mut cs1, mut sn1) = if cs.abs() > ab {
        let ct = -tb / cs;
        let sn1 = 1.0 / (1.0 + ct * ct).sqrt();
        (ct * sn1, sn1)
    } else {
        let tn = -cs / tb;
        let cs1 = 1.0 / (1.0 + tn * tn).sqrt();
        (cs1, tn * cs1)
    };
    if sgn1 == sgn2 {
        (cs1, sn1) = (-sn1, cs1);
    }
    // eigenvectors (cs1, sn1) for rt1 and (−sn1, cs1) for rt2, sorted by eigenvalue
    if rt2 < rt1 {
        Vec2::new(cs1, sn1)
    } else {
        Vec2::new(-sn1, cs1)
    }
}

fn mean_rows(p: &[Vec2]) -> Vec2 {
    let mut s = p[0];
    for &q in &p[1..] {
        s += q;
    }
    s / p.len() as f64
}

/// `np.linalg.svd(X − mean)[2][0]`: the principal axis (sign arbitrary).
fn principal_axis(p: &[Vec2], ctr: Vec2) -> Vec2 {
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for q in p {
        let d = *q - ctr;
        sxx += d.x * d.x;
        sxy += d.x * d.y;
        syy += d.y * d.y;
    }
    eigh2_top(sxx, sxy, syy)
}

type Verts = Vec<(String, [f64; 2])>;

fn vertex(verts: &Verts, id: &str) -> Vec2 {
    verts
        .iter()
        .find(|(k, _)| k == id)
        .map_or(Vec2::ZERO, |(_, v)| Vec2::new(v[0], v[1]))
}

/// `_edges_from_loop`: vertices `c1…` and edges `e1…` (a line, or a relative `poly` curve when the run is
/// not straight) between consecutive corners.
///
/// # Errors
/// Two corners at one point (a zero-length chord for a curved edge).
pub fn edges_from_loop(
    q: &[Vec2],
    vidx: &[usize],
    size: f64,
    simplify_frac: f64,
) -> Result<(Verts, Vec<PyJson>), String> {
    let line_tol = 1.5f64.max(simplify_frac * size);
    let curve_tol = 0.3f64.max(simplify_frac * size / 6.0);
    let n = vidx.len();
    let verts: Verts = vidx
        .iter()
        .enumerate()
        .map(|(j, &i)| {
            (
                format!("c{}", j + 1),
                [py::round(q[i].x, 3), py::round(q[i].y, 3)],
            )
        })
        .collect();
    let mut edges = Vec::with_capacity(n);
    for j in 0..n {
        let (a, b) = (vidx[j], vidx[(j + 1) % n]);
        let mut seg: Vec<Vec2> = if b > a {
            q[a..=b].to_vec()
        } else {
            q[a..].iter().chain(&q[..=b]).copied().collect()
        };
        let (va, vb) = (format!("c{}", j + 1), format!("c{}", (j + 1) % n + 1));
        let (p0, p1) = (vertex(&verts, &va), vertex(&verts, &vb));
        let last = seg.len() - 1;
        (seg[0], seg[last]) = (p0, p1);
        // straightness: total-least-squares line through the run minus its corner zones
        let s_arc = polyline::cumulative(&seg);
        let total = s_arc[last];
        let trim = (0.06 * total).min(0.02 * size + 2.0);
        let mut mid: Vec<Vec2> = seg
            .iter()
            .zip(&s_arc)
            .filter(|&(_, &s)| s >= trim && s <= total - trim)
            .map(|(p, _)| *p)
            .collect();
        if mid.len() < 3 {
            mid.clone_from(&seg);
        }
        let ctr = mean_rows(&mid);
        let u = principal_axis(&mid, ctr);
        let dev = mid
            .iter()
            .map(|p| ((p.x - ctr.x) * -u.y + (p.y - ctr.y) * u.x).abs())
            .fold(0.0, f64::max);
        let mut edge = PyJson::dict(vec![
            ("id", PyJson::str(format!("e{}", j + 1))),
            ("from", PyJson::str(&va)),
            ("to", PyJson::str(&vb)),
        ]);
        if seg.len() > 2 && dev > line_tol {
            let simp = geos::simplify(&seg, curve_tol);
            if simp.len() > 2 {
                let c = simp[1..simp.len() - 1]
                    .iter()
                    .map(|&p| {
                        abs_to_rel(p0, p1, p)
                            .map(|th| pair([py::round(th[0], 5), py::round(th[1], 5)]))
                            .map_err(|_| "float division by zero".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                edge.set(
                    "curve",
                    PyJson::dict(vec![("type", PyJson::str("poly")), ("c", PyJson::List(c))]),
                );
            }
        }
        edges.push(edge);
    }
    Ok((verts, edges))
}

// ================================================================================== internal marks

/// `_rasterize`: each polygon's shell filled with 1, then its holes with 0 (coordinates rounded half to
/// even).
fn rasterize(polys: &[Polygon], w: usize, h: usize) -> Gray {
    let mut m = Gray::filled(w, h, 0);
    let px = |r: &[Vec2]| -> Vec<(i64, i64)> {
        r.iter()
            .map(|p| (p.x.round_ties_even() as i64, p.y.round_ties_even() as i64))
            .collect()
    };
    for p in polys {
        draw::fill_poly(&mut m, &[px(&p.shell)], 1);
        for h in &p.holes {
            draw::fill_poly(&mut m, &[px(h)], 0);
        }
    }
    m
}

/// One ink component (`_components`'s dicts).
#[derive(Clone, Debug)]
struct Comp {
    pts: Vec<Vec2>,
    c: Vec2,
    u: Vec2,
    area: usize,
    long: f64,
    short: f64,
    proj: Vec<f64>,
    perp: Vec<f64>,
    solidity: f64,
    dmin: f64,
    dmed: f64,
    diqr: f64,
}

fn ptp(v: &[f64]) -> f64 {
    let (lo, hi) = v
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
    hi - lo
}

fn min_of(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::INFINITY, f64::min)
}

fn max_of(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

fn hull_of(pts: &[Vec2]) -> Vec<(f32, f32)> {
    let p: Vec<(f32, f32)> = pts.iter().map(|q| (q.x as f32, q.y as f32)).collect();
    contours::convex_hull(&p)
}

/// `_components`: 8-connected ink components above `min_area`, with their axis, extents and distances.
fn components(ink: &Gray, dt: &GrayF, min_area: f64) -> Vec<Comp> {
    let (labels, stats) = distance::components8(ink);
    let w = ink.w;
    let mut out = Vec::new();
    for (lab, st) in stats.iter().enumerate().skip(1) {
        if (st.area as f64) < min_area {
            continue;
        }
        let mut pts = Vec::new();
        let mut d = Vec::new();
        for y in st.y..st.y + st.h {
            for x in st.x..st.x + st.w {
                if labels[y * w + x] as usize == lab {
                    pts.push(Vec2::new(x as f64, y as f64));
                    d.push(dt.data[y * w + x]);
                }
            }
        }
        let ctr = mean_rows(&pts);
        let u = if pts.len() >= 3 {
            // np.cov: rows centred (again) by their mean, X·Xᵀ times 1/(n − 1), plus 1e-9·I
            let n = pts.len();
            let (xs, ys): (Vec<f64>, Vec<f64>) =
                pts.iter().map(|p| (p.x - ctr.x, p.y - ctr.y)).unzip();
            let (mx, my) = (
                np::pairwise_sum(&xs) / n as f64,
                np::pairwise_sum(&ys) / n as f64,
            );
            let (xs, ys): (Vec<f64>, Vec<f64>) =
                xs.iter().zip(&ys).map(|(x, y)| (x - mx, y - my)).unzip();
            let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(p, q)| p * q).sum::<f64>();
            let fct = 1.0 / (n - 1) as f64;
            eigh2_top(
                dot(&xs, &xs) * fct + 1e-9,
                dot(&xs, &ys) * fct,
                dot(&ys, &ys) * fct + 1e-9,
            )
        } else {
            Vec2::new(1.0, 0.0)
        };
        let proj: Vec<f64> = pts
            .iter()
            .map(|p| (p.x - ctr.x) * u.x + (p.y - ctr.y) * u.y)
            .collect();
        let perp: Vec<f64> = pts
            .iter()
            .map(|p| (p.x - ctr.x) * -u.y + (p.y - ctr.y) * u.x)
            .collect();
        let hull: Vec<Pt> = hull_of(&pts)
            .iter()
            .map(|&(x, y)| (x as i32, y as i32))
            .collect();
        out.push(Comp {
            c: ctr,
            u,
            area: st.area,
            long: ptp(&proj) + 1.0,
            short: ptp(&perp) + 1.0,
            solidity: st.area as f64 / contours::area(&hull).max(1.0),
            dmin: f64::from(d.iter().copied().fold(f32::INFINITY, f32::min)),
            dmed: f64::from(np::median_f32(&d)),
            diqr: f64::from(np::percentile_f32(&d, 75.0) - np::percentile_f32(&d, 25.0)),
            pts,
            proj,
            perp,
        });
    }
    out
}

/// `_dashed_groups`: unions of near-collinear dashes; groups of at least four, in order of first member.
fn dashed_groups(cands: &[&Comp]) -> Vec<Vec<usize>> {
    let n = cands.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let cos12 = det::cos(12.0 * (PI / 180.0));
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (cands[i], cands[j]);
            let v = b.c - a.c;
            let dist = v.dot(v).sqrt();
            if dist < 1e-6 || dist > 3.5 * a.long.max(b.long) {
                continue;
            }
            let v = v / dist;
            if a.u.dot(b.u).abs() > cos12 && v.dot(a.u).abs() > cos12 && v.dot(b.u).abs() > cos12 {
                let rj = find(&mut parent, j);
                let ri = find(&mut parent, i);
                parent[ri] = rj;
            }
        }
    }
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        match groups.iter_mut().find(|(root, _)| *root == r) {
            Some((_, g)) => g.push(i),
            None => groups.push((r, vec![i])),
        }
    }
    groups
        .into_iter()
        .map(|(_, g)| g)
        .filter(|g| g.len() >= 4)
        .collect()
}

/// A piece's marks: notches, internal marks and the grain (with its meta keys).
struct Marks {
    notches: Vec<PyJson>,
    internal: Vec<PyJson>,
    grain: Option<PyJson>,
}

fn norm_or_one(v: Vec2) -> f64 {
    let n = v.dot(v).sqrt();
    if n == 0.0 { 1.0 } else { n }
}

/// `detect_marks`: best-effort internal marks of one piece; everything carries a `confidence`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn detect_marks(
    mask: &Gray,
    poly: &Polygon,
    mode: &str,
    w: f64,
    size: f64,
    verts: &Verts,
    edges: &[PyJson],
) -> Result<Marks, String> {
    let (iw, ih) = (mask.w, mask.h);
    let hf = ih as f64;
    let to_pat = |p: Vec2| [py::round(p.x, 3), py::round(hf - p.y, 3)];
    let ring = mode == "ring";
    let inset = if ring { w / 2.0 + 1.5 } else { 2.0 + w / 2.0 };
    let poly = std::slice::from_ref(poly);
    let reg = rasterize(&buffer(poly, -inset, Join::Round(16))?, iw, ih);
    let ink = Gray {
        w: iw,
        h: ih,
        data: mask
            .data
            .iter()
            .zip(&reg.data)
            .map(|(&m, &r)| u8::from(((m > 0) == ring) && r != 0))
            .collect(),
    };
    let dt = distance::distance_l2(&rasterize(poly, iw, ih), 5);
    let wm1 = w.max(1.0);
    let comps = components(&ink, &dt, 4.0f64.max(0.25 * wm1 * wm1));
    let depth = (3.0 * w).max(0.025 * size).max(4.0);
    let (mut notches, mut internal, mut grain) = (Vec::new(), Vec::new(), None);
    let mut used = vec![false; comps.len()];

    // dashed lines first (their dashes would otherwise pass as notches)
    let dash_c: Vec<usize> = (0..comps.len())
        .filter(|&k| {
            let c = &comps[k];
            c.long <= 0.08 * size && c.long >= 1.8 * c.short && c.long >= (2.0 * w).max(4.0)
        })
        .collect();
    let line_edges: Vec<(String, Vec2, Vec2)> = edges
        .iter()
        .filter(|e| e.get("curve").is_none())
        .map(|e| {
            let s = |k: &str| match e.get(k) {
                Some(PyJson::Str(s)) => s.clone(),
                _ => String::new(),
            };
            (s("id"), vertex(verts, &s("from")), vertex(verts, &s("to")))
        })
        .collect();
    let cands: Vec<&Comp> = dash_c.iter().map(|&k| &comps[k]).collect();
    let cos6 = det::cos(6.0 * (PI / 180.0));
    for g in dashed_groups(&cands) {
        let members: Vec<&Comp> = g.iter().map(|&k| cands[k]).collect();
        for &k in &g {
            used[dash_c[k]] = true;
        }
        let u0 = members[0].u;
        let sign = |x: f64| {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        };
        let aligned: Vec<Vec2> = members.iter().map(|m| m.u * sign(m.u.dot(u0))).collect();
        let mut u = mean_rows(&aligned);
        u = u / u.dot(u).sqrt();
        let centres: Vec<Vec2> = members.iter().map(|m| m.c).collect();
        let ctr = mean_rows(&centres);
        let t: Vec<f64> = members.iter().map(|m| (m.c - ctr).dot(u)).collect();
        let half = members
            .iter()
            .map(|m| m.long)
            .fold(f64::NEG_INFINITY, f64::max)
            / 2.0;
        let p0 = ctr + u * (min_of(&t) - half);
        let p1 = ctr + u * (max_of(&t) + half);
        let nd = members.len();
        let mut item = PyJson::dict(vec![
            ("kind", PyJson::str("dashed")),
            (
                "pts",
                PyJson::List(vec![pair(to_pat(p0)), pair(to_pat(p1))]),
            ),
            ("n_dashes", int(usize_i64(nd))),
            (
                "confidence",
                f(py::round(0.9f64.min(0.4 + 0.08 * nd as f64), 2)),
            ),
        ]);
        let (a0, a1) = (Vec2::from(to_pat(p0)), Vec2::from(to_pat(p1)));
        let dirn = (a1 - a0) / norm_or_one(a1 - a0);
        let mut best: Option<(&str, f64)> = None;
        for (id, q0, q1) in &line_edges {
            let d = (*q1 - *q0) / norm_or_one(*q1 - *q0);
            if d.dot(dirn).abs() < cos6 {
                continue;
            }
            let off = [a0, a1]
                .iter()
                .map(|a| (d.x * (a.y - q0.y) - d.y * (a.x - q0.x)).abs())
                .fold(f64::NEG_INFINITY, f64::max);
            if off < 0.06 * size && best.is_none_or(|(_, b)| off < b) {
                best = Some((id, off));
            }
        }
        if let Some((id, off)) = best {
            item.set("kind", PyJson::str("fold_candidate"));
            item.set("near_edge", PyJson::str(id));
            item.set("offset", f(py::round(off, 2)));
        }
        internal.push(item);
    }

    // grainline: long, thin, straight; arrowheads make the ends wider than the middle
    let mut grains: Vec<(f64, usize, bool)> = Vec::new();
    for (k, c) in comps.iter().enumerate() {
        if used[k] || c.long < 0.15 * size || c.long / c.short < 6.0 {
            continue;
        }
        let (lo, hi) = (min_of(&c.proj), max_of(&c.proj));
        if c.dmin <= inset + 2.5 {
            used[k] = true; // touches the outline: a dimension arrow, not a grainline
            internal.push(PyJson::dict(vec![
                ("kind", PyJson::str("line")),
                (
                    "pts",
                    PyJson::List(vec![
                        pair(to_pat(c.c + c.u * lo)),
                        pair(to_pat(c.c + c.u * hi)),
                    ]),
                ),
                ("confidence", f(0.3)),
                (
                    "note",
                    PyJson::str("long straight mark touching the outline (dimension arrow?)"),
                ),
            ]));
            continue;
        }
        let l = c.long;
        let band = |keep: &dyn Fn(f64) -> bool| -> f64 {
            let v: Vec<f64> = c
                .proj
                .iter()
                .zip(&c.perp)
                .filter(|&(&p, _)| keep(p.abs()))
                .map(|(_, &q)| q)
                .collect();
            if v.is_empty() { 1.0 } else { ptp(&v) + 1.0 }
        };
        let wm = band(&|a| a < 0.25 * l);
        let we = band(&|a| a > 0.38 * l);
        grains.push((l, k, we > 1.6 * wm));
    }
    grains.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    let ng = grains.len();
    for (rank, &(_, k, arrows)) in grains.iter().enumerate() {
        let c = &comps[k];
        used[k] = true;
        let (lo, hi) = (min_of(&c.proj), max_of(&c.proj));
        let (p0, p1) = (to_pat(c.c + c.u * lo), to_pat(c.c + c.u * hi));
        if rank == 0 {
            let ang = py_mod(det::atan2(p1[1] - p0[1], p1[0] - p0[0]) * RAD_TO_DEG, 180.0);
            let conf = (if arrows { 0.7 } else { 0.45 }) - (if ng > 1 { 0.15 } else { 0.0 });
            grain = Some(PyJson::dict(vec![
                ("p0", pair(p0)),
                ("p1", pair(p1)),
                ("angle_deg", f(py::round(ang, 2))),
                ("arrowheads", PyJson::Bool(arrows)),
                ("confidence", f(py::round(conf, 2))),
                ("source", PyJson::str("trace")),
            ]));
        } else {
            internal.push(PyJson::dict(vec![
                ("kind", PyJson::str("line")),
                ("pts", PyJson::List(vec![pair(p0), pair(p1)])),
                ("arrowheads", PyJson::Bool(arrows)),
                ("confidence", f(0.3)),
                (
                    "note",
                    PyJson::str("another long straight mark (dimension arrow?)"),
                ),
            ]));
        }
    }

    if grain.is_none() {
        // fallback: long straight segments far from the outline
        let thr = (0.06 * size) as f32;
        let far = Gray {
            w: iw,
            h: ih,
            data: ink
                .data
                .iter()
                .zip(&dt.data)
                .map(|(&i, &d)| if i != 0 && d > thr { 255 } else { 0 })
                .collect(),
        };
        let segs = hough_lines_p(
            &far,
            1.0,
            PI / 180.0,
            20.max((0.08 * size) as i32),
            0.2 * size,
            3.0f64.max((0.01 * size).trunc()),
        );
        if !segs.is_empty() {
            // the longest segment (the first of the longest, as a stable sort by length leaves it)
            let len = |s: &[i32; 4]| det::hypot(f64::from(s[2] - s[0]), f64::from(s[3] - s[1]));
            let s = segs[first_max_by(&segs, len)].map(f64::from);
            let (p0, p1) = (to_pat(Vec2::new(s[0], s[1])), to_pat(Vec2::new(s[2], s[3])));
            let ang = py_mod(det::atan2(p1[1] - p0[1], p1[0] - p0[0]) * RAD_TO_DEG, 180.0);
            grain = Some(PyJson::dict(vec![
                ("p0", pair(p0)),
                ("p1", pair(p1)),
                ("angle_deg", f(py::round(ang, 2))),
                ("arrowheads", PyJson::Null),
                ("confidence", f(0.35)),
                ("source", PyJson::str("trace-hough")),
            ]));
        }
    }

    // parallel inner line (often the seam line / allowance boundary of an AI sheet)
    for (k, c) in comps.iter().enumerate() {
        if used[k] || c.long < 0.3 * size || c.long / c.short >= 6.0 {
            continue;
        }
        if c.dmed < 0.1 * size && c.diqr < 0.35 * c.dmed.max(1.0) {
            used[k] = true;
            internal.push(PyJson::dict(vec![
                ("kind", PyJson::str("parallel_line")),
                ("offset", f(py::round(c.dmed, 2))),
                ("confidence", f(0.5)),
                (
                    "note",
                    PyJson::str("inner line at ~constant offset (seam line?)"),
                ),
            ]));
        }
    }

    // notches: compact blobs touching the inside of the outline
    let mut edge_lines: Vec<(String, Vec<Vec2>)> = Vec::new();
    for e in edges {
        let s = |k: &str| match e.get(k) {
            Some(PyJson::Str(s)) => s.clone(),
            _ => String::new(),
        };
        let curve: Option<Curve> = e
            .get("curve")
            .map(|c| serde_json::from_value(c.to_value()))
            .transpose()
            .map_err(|x| x.to_string())?;
        let pts = curve_points(
            vertex(verts, &s("from")),
            vertex(verts, &s("to")),
            curve.as_ref(),
            120,
        )
        .map_err(|x| format!("{x:?}"))?;
        edge_lines.push((s("id"), pts));
    }
    let mut n_id = 0;
    for (k, c) in comps.iter().enumerate() {
        if used[k] || c.dmin > inset + 2.0 || c.long > 2.5 * depth || c.dmed > inset + depth {
            continue;
        }
        if (c.area as f64) < 6.0f64.max(0.5 * w * w) || c.solidity < 0.6 {
            continue;
        }
        let q = Vec2::from(to_pat(c.c));
        let (eid, line) = &edge_lines[first_min_by(&edge_lines, |(_, l)| line_distance(l, q))];
        let s = line_project_normalized(line, q);
        if !(0.02..=0.98).contains(&s) {
            continue; // blobs at corners are usually stroke joins
        }
        let tri = contours::approx_poly_dp(&hull_of(&c.pts), 0.15 * c.long, true).len() == 3;
        n_id += 1;
        notches.push(PyJson::dict(vec![
            ("id", PyJson::str(format!("n{n_id}"))),
            ("edge", PyJson::str(eid)),
            ("s", f(py::round(s, 4))),
            ("type", PyJson::str("V")),
            (
                "confidence",
                f(py::round(
                    0.45 + if tri { 0.25 } else { 0.0 } + if c.solidity > 0.8 { 0.1 } else { 0.0 },
                    2,
                )),
            ),
            ("source", PyJson::str("trace")),
        ]));
    }
    Ok(Marks {
        notches,
        internal,
        grain,
    })
}

// ================================================================================== trace

type Rect = (i32, i32, i32, i32);

/// `_box_iou` of two `(x, y, w, h)` boxes.
fn box_iou(a: Rect, b: Rect) -> f64 {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    let ix = 0.max((ax + aw).min(bx + bw) - ax.max(bx));
    let iy = 0.max((ay + ah).min(by + bh) - ay.max(by));
    let inter = i64::from(ix) * i64::from(iy);
    let union = i64::from(aw) * i64::from(ah) + i64::from(bw) * i64::from(bh) - inter;
    inter as f64 / if union == 0 { 1.0 } else { union as f64 }
}

/// `bridge_dashes`: long sparse (dashed) straight runs drawn solid so a dashed fold edge closes the outline;
/// the mask and the number of runs drawn.
#[must_use]
pub fn bridge_dashes(mask: &Gray) -> (Gray, usize) {
    let (w, h) = (mask.w, mask.h);
    let s = w.min(h) as f64;
    let lines = hough_lines_p(
        mask,
        1.0,
        PI / 360.0,
        20.max((0.05 * s) as i32),
        (0.12 * s).trunc(),
        (0.025 * s).trunc(),
    );
    if lines.is_empty() {
        return (mask.clone(), 0);
    }
    let mut out = mask.clone();
    let bin = Gray {
        w,
        h,
        data: mask.data.iter().map(|&v| u8::from(v > 0)).collect(),
    };
    let dt = distance::distance_l2(&bin, 3);
    let pos: Vec<f32> = dt.data.iter().copied().filter(|&d| d > 0.0).collect();
    let width = if pos.is_empty() {
        3
    } else {
        2.max(round_int(2.0 * f64::from(np::median_f32(&pos))))
    };
    let mut n = 0;
    for [x1, y1, x2, y2] in lines {
        let l = det::hypot(f64::from(x2 - x1), f64::from(y2 - y1)) as usize;
        if l < 2 {
            continue;
        }
        let along = |a: i32, b: i32, hi: usize| -> Vec<usize> {
            np::linspace(f64::from(a), f64::from(b), l)
                .iter()
                .map(|v| (v.round_ties_even() as i64).clamp(0, usize_i64(hi) - 1) as usize)
                .collect()
        };
        let (xs, ys) = (along(x1, x2, w), along(y1, y2, h));
        let on = xs
            .iter()
            .zip(&ys)
            .filter(|&(&x, &y)| mask.data[y * w + x] > 0)
            .count();
        if (on as f64 / l as f64) < 0.85 {
            draw::line(
                &mut out,
                (i64::from(x1), i64::from(y1)),
                (i64::from(x2), i64::from(y2)),
                255,
                width,
            );
            n += 1;
        }
    }
    (out, n)
}

fn crop_image(full: &Bgr, x0: usize, y0: usize, cw: usize, ch: usize) -> Bgr {
    let mut data = Vec::with_capacity(3 * cw * ch);
    for y in y0..y0 + ch {
        data.extend_from_slice(&full.data[3 * (y * full.w + x0)..3 * (y * full.w + x0 + cw)]);
    }
    Bgr { w: cw, h: ch, data }
}

fn paint(img: &mut Bgr, erased: &[bool], bg: [f64; 3]) {
    let c = bg_u8(bg);
    for (px, _) in img
        .data
        .as_chunks_mut::<3>()
        .0
        .iter_mut()
        .zip(erased)
        .filter(|(_, e)| **e)
    {
        *px = c;
    }
}

/// `trace_image`: the draft, diagnostics, mask and piece polygons of a pattern sheet. `source` is the
/// image's resolved path when it came from a file (`meta.source.image`, and the draft's name).
///
/// # Errors
/// An unknown channel or threshold, an empty crop, a degenerate piece, or a traced draft that
/// fails the structure checks.
#[allow(clippy::too_many_lines)]
pub fn trace_image(full: &Bgr, source: Option<&str>, opts: &TraceOptions) -> Result<Trace, String> {
    let (wf, hf) = (usize_i64(full.w), usize_i64(full.h));
    let (x0, y0, mut img) = match opts.crop {
        Some(c) => {
            let r = c.map(round_int);
            let (x0, y0) = (r[0].max(0), r[1].max(0));
            let (cw, ch) = (r[2].min(wf - x0), r[3].min(hf - y0));
            if cw <= 0 || ch <= 0 {
                return Err("the crop is empty".into());
            }
            (
                x0,
                y0,
                crop_image(full, x0 as usize, y0 as usize, cw as usize, ch as usize),
            )
        }
        None => (0, 0, full.clone()),
    };
    let (w, h) = (img.w, img.h);
    if w == 0 || h == 0 {
        return Err("the image is empty".into());
    }
    if !CHANNELS.contains(&opts.channel.as_str()) {
        let names: Vec<String> = CHANNELS.iter().map(|c| py::repr_str(c)).collect();
        return Err(format!("channel must be one of ({})", names.join(", ")));
    }
    let mut bg = match opts.bg_rgb {
        Some(rgb) => [rgb[2], rgb[1], rgb[0]],
        None => estimate_background(&img)?,
    };
    let mut erased = vec![false; w * h];
    for r in &opts.erase {
        let [ex, ey, ew, eh] = r.map(round_int);
        let (xa, ya) = ((ex - x0).max(0), (ey - y0).max(0));
        let (xb, yb) = (
            (ex - x0 + ew).min(usize_i64(w)),
            (ey - y0 + eh).min(usize_i64(h)),
        );
        if xb > xa && yb > ya {
            for y in ya as usize..yb as usize {
                erased[y * w + xa as usize..y * w + xb as usize]
                    .iter_mut()
                    .for_each(|e| *e = true);
            }
        }
    }
    let any_erased = erased.iter().any(|&e| e);
    if any_erased {
        paint(&mut img, &erased, bg); // UI overlays / watermarks: paint with the background
    }
    let mut diag = PyJson::dict(vec![("retries", PyJson::List(Vec::new()))]);
    let mut attempt_out = None;
    for attempt in 0..2 {
        let (ch, known) = make_channel(&img, &opts.channel, bg)?;
        let (mask, thr) = binarize(&ch, &opts.threshold, known, opts.invert, opts.close_px)?;
        let entries = piece_entries(&mask, opts, &mut diag);
        let big = entries
            .iter()
            .find(|e| contours::area(&e.contour) > 0.6 * (h * w) as f64);
        if let Some(big) =
            big.filter(|_| attempt == 0 && opts.channel == "auto" && opts.bg_rgb.is_none())
        {
            // a coloured panel inside a white frame: the panel is the real background
            let mut sel = Gray::filled(w, h, 0);
            draw::fill_poly(&mut sel, &[to_i64(&big.contour)], 255);
            let pix: Vec<[u8; 3]> = (0..w * h)
                .filter(|&i| sel.data[i] > 0 && mask.data[i] > 0)
                .map(|i| img.at(i % w, i / w))
                .collect();
            bg = mode_color(&pix)?;
            if any_erased {
                paint(&mut img, &erased, bg);
            }
            if let Some(PyJson::List(r)) = diag.get("retries").cloned() {
                let mut r = r;
                r.push(PyJson::str(
                    "panel detected: background re-estimated inside it",
                ));
                diag.set("retries", PyJson::List(r));
            }
            continue;
        }
        attempt_out = Some((mask, thr, entries));
        break;
    }
    let Some((mask, thr, entries)) = attempt_out else {
        return Err("no trace attempt finished".into());
    };
    diag.set("threshold", thr);
    diag.set(
        "bg_rgb",
        PyJson::List(bg.iter().rev().map(|&v| int(v.trunc() as i64)).collect()),
    );
    diag.set("n_candidates", int(usize_i64(entries.len())));
    let ctx = Ctx {
        source,
        image_size: (wf, hf),
        origin: (x0, y0),
        erased: &erased,
    };
    build_draft(mask, entries, diag, &ctx, opts)
}

/// Where a mask came from: what [`trace_image`] knows besides the mask itself.
#[derive(Clone, Debug)]
pub struct MaskSource<'a> {
    /// The image's resolved path (`meta.source.image`).
    pub source: Option<&'a str>,
    /// The full image's `(w, h)`.
    pub image_size: (usize, usize),
    /// The crop's top-left corner in the full image.
    pub origin: (usize, usize),
    /// Erased crop pixels, row-major (empty: none).
    pub erased: Vec<bool>,
    /// The retries the channel stage recorded.
    pub retries: Vec<String>,
    /// `binarize`'s diagnostics (`otsu_threshold`, `flipped`, `frame_fg_frac`, `method`).
    pub threshold: PyJson,
    /// The background as `R, G, B` integers.
    pub bg_rgb: [i64; 3],
}

/// The trace from an already binarized mask (the crop's): pieces, corners, edges and marks exactly as
/// [`trace_image`] continues once its channel stage is done.
///
/// # Errors
/// As [`trace_image`] after the channel stage.
pub fn trace_mask(mask: Gray, src: &MaskSource<'_>, opts: &TraceOptions) -> Result<Trace, String> {
    let retries = src.retries.iter().map(PyJson::str).collect();
    let mut diag = PyJson::dict(vec![("retries", PyJson::List(retries))]);
    let entries = piece_entries(&mask, opts, &mut diag);
    diag.set("threshold", src.threshold.clone());
    diag.set(
        "bg_rgb",
        PyJson::List(src.bg_rgb.iter().map(|&v| int(v)).collect()),
    );
    diag.set("n_candidates", int(usize_i64(entries.len())));
    let erased = if src.erased.is_empty() {
        vec![false; mask.w * mask.h]
    } else {
        src.erased.clone()
    };
    let ctx = Ctx {
        source: src.source,
        image_size: (usize_i64(src.image_size.0), usize_i64(src.image_size.1)),
        origin: (usize_i64(src.origin.0), usize_i64(src.origin.1)),
        erased: &erased,
    };
    build_draft(mask, entries, diag, &ctx, opts)
}

/// The pieces of a mask: its contours, plus (with `dashes`) the ones that exist only once dashed runs are
/// bridged (additive: pieces already found, and the mask used for marks, are untouched).
fn piece_entries(mask: &Gray, opts: &TraceOptions, diag: &mut PyJson) -> Vec<Entry> {
    let mut entries = find_piece_contours(mask, opts.min_area_frac, opts.stroke, opts.open_px);
    if opts.dashes {
        let (bridged, nb) = bridge_dashes(mask);
        if nb > 0 {
            let boxes: Vec<Rect> = entries
                .iter()
                .map(|e| contours::bounding_rect(&e.contour))
                .collect();
            let new: Vec<Entry> =
                find_piece_contours(&bridged, opts.min_area_frac, opts.stroke, opts.open_px)
                    .into_iter()
                    .filter(|e| {
                        let bx = contours::bounding_rect(&e.contour);
                        boxes.iter().all(|&b| box_iou(bx, b) < 0.3)
                    })
                    .collect();
            diag.set("dash_bridged_pieces", int(usize_i64(new.len())));
            entries.extend(new);
        }
    }
    entries
}

struct Ctx<'a> {
    source: Option<&'a str>,
    image_size: (i64, i64),
    origin: (i64, i64),
    erased: &'a [bool],
}

#[allow(clippy::too_many_lines)]
fn build_draft(
    mask: Gray,
    entries: Vec<Entry>,
    diag: PyJson,
    ctx: &Ctx<'_>,
    opts: &TraceOptions,
) -> Result<Trace, String> {
    let (w, h) = (mask.w, mask.h);
    let (x0, y0) = ctx.origin;
    let (wf, hf) = ctx.image_size;
    let (source, erased) = (ctx.source, ctx.erased);
    let any_erased = erased.iter().any(|&e| e);

    // reading order: rows by centroid y, then x
    let mut placed: Vec<(Entry, Rect, f64, f64)> = entries
        .into_iter()
        .map(|e| {
            let b = contours::bounding_rect(&e.contour);
            let (cy, cx) = (
                f64::from(b.1) + f64::from(b.3) / 2.0,
                f64::from(b.0) + f64::from(b.2) / 2.0,
            );
            (e, b, cy, cx)
        })
        .collect();
    if !placed.is_empty() {
        let mh = np::median(&placed.iter().map(|p| f64::from(p.1.3)).collect::<Vec<_>>());
        placed.sort_by(|a, b| a.2.total_cmp(&b.2));
        let mut rows: Vec<Vec<(Entry, Rect, f64, f64)>> = Vec::new();
        let mut cur = vec![placed.remove(0)];
        for e in placed {
            if e.2 - cur[0].2 > 0.5 * mh {
                rows.push(std::mem::take(&mut cur));
            }
            cur.push(e);
        }
        rows.push(cur);
        placed = rows
            .into_iter()
            .flat_map(|mut r| {
                r.sort_by(|a, b| a.3.total_cmp(&b.3));
                r
            })
            .collect();
    }

    let hf64 = h as f64;
    let to_px = PyJson::List(vec![
        PyJson::floats(&[1.0, 0.0, x0 as f64]),
        PyJson::floats(&[0.0, -1.0, (y0 + usize_i64(h)) as f64]),
    ]);
    let mut pieces = Vec::new();
    let mut polys = Vec::new();
    for (n, (e, (bx, by, bw, bh), _, _)) in placed.iter().enumerate() {
        let pid = format!("p{}", n + 1);
        let poly = px_polygon(e, opts.smooth_px)?;
        let mut q: Vec<Vec2> = poly.shell[..poly.shell.len() - 1]
            .iter()
            .map(|p| Vec2::new(p.x, hf64 - p.y))
            .collect();
        if polyline::signed_area(&q) < 0.0 {
            q.reverse();
        }
        let size = f64::from(*bw.max(bh));
        let ds = 0.5f64.max(size / 1500.0);
        let q = resample_closed(&q, ds);
        let (mut vidx, kinds, turn) = detect_corners(&q, ds, size, opts.corner_deg, opts.soft_deg);
        let mnx = q.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let mxy = q.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        let start = first_min_by(&vidx, |&i| (q[i].x - mnx) + (mxy - q[i].y));
        vidx.rotate_left(start);
        let (verts, edges) = edges_from_loop(&q, &vidx, size, opts.simplify_frac)?;
        let (wi, hi) = (usize_i64(w), usize_i64(h));
        let (bx, by, bw, bh) = (
            i64::from(*bx),
            i64::from(*by),
            i64::from(*bw),
            i64::from(*bh),
        );
        let mut touches = bx <= 2 || by <= 2 || bx + bw >= wi - 2 || by + bh >= hi - 2;
        let near_erased = any_erased && {
            let r = morph::dilate(
                &rasterize(std::slice::from_ref(&poly), w, h),
                &Kernel::rect(7, 7),
            );
            erased.iter().zip(&r.data).any(|(&e, &v)| e && v > 0)
        };
        touches = touches || near_erased;
        let n_corner = vidx
            .iter()
            .filter(|&&i| matches!(kinds[i], Some("corner" | "soft")))
            .count();
        let conf = if touches {
            0.3
        } else if n_corner >= 3 {
            0.85
        } else {
            0.5
        };
        let corners = PyJson::Dict(
            vidx.iter()
                .enumerate()
                .map(|(j, &i)| {
                    (
                        format!("c{}", j + 1),
                        PyJson::dict(vec![
                            ("turn_deg", f(py::round(turn[i], 1))),
                            ("kind", PyJson::str(kinds[i].unwrap_or("split"))),
                        ]),
                    )
                })
                .collect(),
        );
        let mut trace = PyJson::dict(vec![
            ("mode", PyJson::str(e.mode)),
            ("stroke_px", f(e.stroke_px)),
            ("stroke_how", PyJson::str(e.stroke_how)),
            ("open_px", int(e.open_px)),
            ("fg_frac", f(e.fg_frac)),
            (
                "bbox_px",
                PyJson::List(vec![int(bx), int(by), int(bw), int(bh)]),
            ),
            ("touches_border", PyJson::Bool(touches)),
            ("touches_erased", PyJson::Bool(near_erased)),
            ("confidence", f(conf)),
            ("to_px", to_px.clone()),
            ("corners", corners),
        ]);
        let vertices = PyJson::Dict(verts.iter().map(|(k, v)| (k.clone(), pair(*v))).collect());
        let mut piece = PyJson::dict(vec![
            ("id", PyJson::str(&pid)),
            ("name", PyJson::str(&pid)),
            ("cut", PyJson::dict(vec![("qty", int(1))])),
            ("vertices", vertices),
            ("edges", PyJson::List(edges.clone())),
            ("notches", PyJson::List(Vec::new())),
            ("trace_marks", PyJson::List(Vec::new())),
        ]);
        let mut grain = None;
        if opts.marks {
            match detect_marks(&mask, &poly, e.mode, e.stroke_px, size, &verts, &edges) {
                Ok(mk) => {
                    piece.set("notches", PyJson::List(mk.notches));
                    piece.set("trace_marks", PyJson::List(mk.internal));
                    grain = mk.grain;
                }
                Err(msg) => trace.set("marks_error", PyJson::Str(msg)),
            }
        }
        piece.set("trace", trace);
        if let Some(PyJson::Dict(g)) = grain {
            let get = |k: &str| {
                g.iter()
                    .find(|(key, _)| key == k)
                    .map_or(PyJson::Null, |(_, v)| v.clone())
            };
            piece.set("grain", PyJson::List(vec![get("p0"), get("p1")]));
            piece.set(
                "grain_meta",
                PyJson::Dict(
                    g.iter()
                        .filter(|(k, _)| k != "p0" && k != "p1")
                        .cloned()
                        .collect(),
                ),
            );
        }
        pieces.push(piece);
        polys.push(poly.shell);
    }

    let file_name = source.map(|s| s.rsplit('/').next().unwrap_or(s).to_string());
    let mut trace_meta = vec![
        ("channel".to_string(), PyJson::str(&opts.channel)),
        ("threshold".to_string(), PyJson::str(&opts.threshold)),
        (
            "invert".to_string(),
            opts.invert.map_or(PyJson::Null, PyJson::Bool),
        ),
        ("min_area_frac".to_string(), f(opts.min_area_frac)),
        ("corner_deg".to_string(), f(opts.corner_deg)),
        ("soft_deg".to_string(), f(opts.soft_deg)),
        ("simplify_frac".to_string(), f(opts.simplify_frac)),
        ("smooth_px".to_string(), f(opts.smooth_px)),
        ("close_px".to_string(), int(opts.close_px)),
        (
            "erase".to_string(),
            PyJson::List(opts.erase.iter().map(|r| PyJson::floats(r)).collect()),
        ),
    ];
    // `**diag`: a key already present (threshold) takes the diagnostic's value in place
    if let PyJson::Dict(d) = &diag {
        for (k, v) in d {
            match trace_meta.iter_mut().find(|(key, _)| key == k) {
                Some(slot) => slot.1 = v.clone(),
                None => trace_meta.push((k.clone(), v.clone())),
            }
        }
    }
    let data = PyJson::dict(vec![
        ("schema", PyJson::str("fabric-pattern/1")),
        ("units", PyJson::str("mm")),
        (
            "name",
            PyJson::str(format!(
                "draft traced from {}",
                file_name.as_deref().unwrap_or("array")
            )),
        ),
        (
            "meta",
            PyJson::dict(vec![
                ("scale_status", PyJson::str("UNSCALED")),
                ("px_per_unit", f(1.0)),
                (
                    "units_note",
                    PyJson::str(
                        "coordinates are source pixels (1 px = 1 unit placeholder) until scale-fit; y is up (image rows \
                         flipped); loops CCW",
                    ),
                ),
                (
                    "source",
                    PyJson::dict(vec![
                        ("image", source.map_or(PyJson::Null, PyJson::str)),
                        (
                            "crop",
                            PyJson::List(vec![
                                int(x0),
                                int(y0),
                                int(usize_i64(w)),
                                int(usize_i64(h)),
                            ]),
                        ),
                        ("image_size", PyJson::List(vec![int(wf), int(hf)])),
                    ]),
                ),
                ("trace", PyJson::Dict(trace_meta)),
                ("generator", PyJson::str("vectorize.py trace")),
            ]),
        ),
        ("pieces", PyJson::List(pieces)),
        ("seams", PyJson::List(Vec::new())),
    ]);
    if !polys.is_empty() {
        let errs = Pattern::new(data.to_value()).structure_errors();
        if !errs.is_empty() {
            return Err(format!(
                "traced draft failed structure checks: {}",
                errs.join("; ")
            ));
        }
    }
    Ok(Trace {
        pattern: data,
        diagnostics: diag,
        mask,
        polys,
    })
}
