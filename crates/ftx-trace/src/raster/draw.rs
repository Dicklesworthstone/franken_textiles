//! OpenCV 5.0 rasterization (`drawing.cpp`) as the trace uses it, on 8-bit images: `fillPoly` /
//! `drawContours(…, FILLED)` (each edge drawn as an 8-connected line, then the even-odd scanline fill in
//! 16-bit fixed point), and `line` with a thickness (a filled convex quadrilateral with `Line2` edges plus
//! round caps).

use super::image::Gray;

const XY_SHIFT: i64 = 16;
const XY_ONE: i64 = 1 << XY_SHIFT;

fn put(img: &mut Gray, x: i64, y: i64, v: u8) {
    if x >= 0 && y >= 0 && (x as usize) < img.w && (y as usize) < img.h {
        img.data[y as usize * img.w + x as usize] = v;
    }
}

fn hline(img: &mut Gray, y: i64, x1: i64, x2: i64, v: u8) {
    if y < 0 || y as usize >= img.h {
        return;
    }
    let (a, b) = (x1.max(0), x2.min(img.w as i64 - 1));
    for x in a..=b {
        img.data[y as usize * img.w + x as usize] = v;
    }
}

/// `clipLine(size, p1, p2)`: whether any of the segment lies inside; the ends clipped to the image.
fn clip_line(w: i64, h: i64, p1: &mut (i64, i64), p2: &mut (i64, i64)) -> bool {
    let (right, bottom) = (w - 1, h - 1);
    if w <= 0 || h <= 0 {
        return false;
    }
    let code = |p: (i64, i64)| {
        i32::from(p.0 < 0)
            + i32::from(p.0 > right) * 2
            + i32::from(p.1 < 0) * 4
            + i32::from(p.1 > bottom) * 8
    };
    let (mut c1, mut c2) = (code(*p1), code(*p2));
    if c1 & c2 == 0 && (c1 | c2) != 0 {
        if c1 & 12 != 0 {
            let a = if c1 < 8 { 0 } else { bottom };
            p1.0 += ((a - p1.1) as f64 * (p2.0 - p1.0) as f64 / (p2.1 - p1.1) as f64) as i64;
            p1.1 = a;
            c1 = i32::from(p1.0 < 0) + i32::from(p1.0 > right) * 2;
        }
        if c2 & 12 != 0 {
            let a = if c2 < 8 { 0 } else { bottom };
            p2.0 += ((a - p2.1) as f64 * (p2.0 - p1.0) as f64 / (p2.1 - p1.1) as f64) as i64;
            p2.1 = a;
            c2 = i32::from(p2.0 < 0) + i32::from(p2.0 > right) * 2;
        }
        if c1 & c2 == 0 && (c1 | c2) != 0 {
            if c1 != 0 {
                let a = if c1 == 1 { 0 } else { right };
                p1.1 += ((a - p1.0) as f64 * (p2.1 - p1.1) as f64 / (p2.0 - p1.0) as f64) as i64;
                p1.0 = a;
                c1 = 0;
            }
            if c2 != 0 {
                let a = if c2 == 1 { 0 } else { right };
                p2.1 += ((a - p2.0) as f64 * (p2.1 - p1.1) as f64 / (p2.0 - p1.0) as f64) as i64;
                p2.0 = a;
                c2 = 0;
            }
        }
    }
    (c1 | c2) == 0
}

/// `Line(img, p1, p2, color, 8)`: OpenCV's 8-connected `LineIterator` (left to right, clipped).
pub fn line8(img: &mut Gray, p1: (i64, i64), p2: (i64, i64), v: u8) {
    let (w, h) = (img.w as i64, img.h as i64);
    let (mut a, mut b) = (p1, p2);
    let outside = |p: (i64, i64)| p.0 < 0 || p.0 >= w || p.1 < 0 || p.1 >= h;
    if (outside(a) || outside(b)) && !clip_line(w, h, &mut a, &mut b) {
        return;
    }
    let (mut dx, mut dy) = (b.0 - a.0, b.1 - a.1);
    let (mut delta_x, mut delta_y) = (1i64, 1i64);
    let mut p = a;
    if dx < 0 {
        dx = -dx;
        dy = -dy;
        p = b;
    }
    if dy < 0 {
        dy = -dy;
        delta_y = -1;
    }
    let vert = dy > dx;
    if vert {
        std::mem::swap(&mut dx, &mut dy);
        std::mem::swap(&mut delta_x, &mut delta_y);
    }
    let mut err = dx - (dy + dy);
    let (plus_delta, minus_delta) = (dx + dx, -(dy + dy));
    // (x, y) moves: every step `minus`, and `plus` when err < 0
    let (minus, plus) = if vert {
        ((0, delta_x), (delta_y, 0))
    } else {
        ((delta_x, 0), (0, delta_y))
    };
    for _ in 0..=dx {
        put(img, p.0, p.1, v);
        let neg = err < 0;
        err += minus_delta + if neg { plus_delta } else { 0 };
        p.0 += minus.0 + if neg { plus.0 } else { 0 };
        p.1 += minus.1 + if neg { plus.1 } else { 0 };
    }
}

/// `fillPoly(img, polygons, v)` (8-connected, no shift): the polygons' edges drawn, then the even-odd fill
/// of all their edges together.
pub fn fill_poly(img: &mut Gray, polys: &[Vec<(i64, i64)>], v: u8) {
    struct Edge {
        y0: i64,
        y1: i64,
        x: i64,
        dx: i64,
    }
    let (w, h) = (img.w as i64, img.h as i64);
    let mut edges: Vec<Edge> = Vec::new();
    for poly in polys {
        if poly.is_empty() {
            continue;
        }
        let mut pt0 = poly[poly.len() - 1];
        for &pt1 in poly {
            let (mut t0, mut t1) = (pt0, pt1);
            line8(img, t0, t1, v);
            let (mut y0c, mut y1c) = (pt0.1, pt1.1);
            let outside = |p: (i64, i64)| p.0 < 0 || p.0 >= w || p.1 < 0 || p.1 >= h;
            if outside(t0) || outside(t1) {
                clip_line(w, h, &mut t0, &mut t1);
                if t0.1 != t1.1 {
                    y0c = t0.1;
                    y1c = t1.1;
                }
            }
            let (x0c, x1c) = (t0.0 << XY_SHIFT, t1.0 << XY_SHIFT);
            if pt0.1 != pt1.1 {
                let dx = (x1c - x0c) / (y1c - y0c);
                edges.push(if pt0.1 < pt1.1 {
                    Edge {
                        y0: pt0.1,
                        y1: pt1.1,
                        x: x0c + (pt0.1 - y0c) * dx,
                        dx,
                    }
                } else {
                    Edge {
                        y0: pt1.1,
                        y1: pt0.1,
                        x: x1c + (pt1.1 - y1c) * dx,
                        dx,
                    }
                });
            }
            pt0 = pt1;
        }
    }
    if edges.len() < 2 {
        return;
    }
    let (mut y_min, mut y_max) = (i64::MAX, i64::MIN);
    let (mut x_min, mut x_max) = (i64::MAX, i64::MIN);
    for e in &edges {
        let x1 = e.x + (e.y1 - e.y0) * e.dx;
        y_min = y_min.min(e.y0);
        y_max = y_max.max(e.y1);
        x_min = x_min.min(e.x).min(x1);
        x_max = x_max.max(e.x).max(x1);
    }
    if y_max < 0 || y_min >= h || x_max < 0 || x_min >= w << XY_SHIFT {
        return;
    }
    let delta = XY_ONE - 1;
    let mut xs: Vec<i64> = Vec::new();
    for y in y_min..y_max.min(h) {
        xs.clear();
        xs.extend(
            edges
                .iter()
                .filter(|e| e.y0 <= y && y < e.y1)
                .map(|e| e.x + (y - e.y0) * e.dx),
        );
        if y < 0 {
            continue;
        }
        xs.sort_unstable();
        for &[left, right] in xs.as_chunks::<2>().0 {
            let (x1, x2) = ((left + delta) >> XY_SHIFT, right >> XY_SHIFT);
            if x1 < w && x2 >= 0 {
                hline(img, y, x1, x2, v);
            }
        }
    }
}

/// `Line2`: the fixed-point (16-bit) line `FillConvexPoly` draws a shifted polygon's edges with.
fn line2(img: &mut Gray, mut p1: (i64, i64), mut p2: (i64, i64), v: u8) {
    if !clip_line(
        (img.w as i64) << XY_SHIFT,
        (img.h as i64) << XY_SHIFT,
        &mut p1,
        &mut p2,
    ) {
        return;
    }
    let (mut dx, mut dy) = (p2.0 - p1.0, p2.1 - p1.1);
    let j = if dx < 0 { -1 } else { 0 };
    let ax = (dx ^ j) - j;
    let i = if dy < 0 { -1 } else { 0 };
    let ay = (dy ^ i) - i;
    let (x_step, y_step, ecount);
    if ax > ay {
        dy = (dy ^ j) - j;
        if j != 0 {
            std::mem::swap(&mut p1, &mut p2);
        }
        x_step = XY_ONE;
        y_step = dy * XY_ONE / (ax | 1);
        ecount = (p2.0 - p1.0) >> XY_SHIFT;
    } else {
        dx = (dx ^ i) - i;
        if i != 0 {
            std::mem::swap(&mut p1, &mut p2);
        }
        x_step = dx * XY_ONE / (ay | 1);
        y_step = XY_ONE;
        ecount = (p2.1 - p1.1) >> XY_SHIFT;
    }
    p1.0 += XY_ONE >> 1;
    p1.1 += XY_ONE >> 1;
    put(
        img,
        (p2.0 + (XY_ONE >> 1)) >> XY_SHIFT,
        (p2.1 + (XY_ONE >> 1)) >> XY_SHIFT,
        v,
    );
    let mut e = ecount;
    if ax > ay {
        p1.0 >>= XY_SHIFT;
        while e >= 0 {
            put(img, p1.0, p1.1 >> XY_SHIFT, v);
            p1.0 += 1;
            p1.1 += y_step;
            e -= 1;
        }
    } else {
        p1.1 >>= XY_SHIFT;
        while e >= 0 {
            put(img, p1.0 >> XY_SHIFT, p1.1, v);
            p1.0 += x_step;
            p1.1 += 1;
            e -= 1;
        }
    }
}

/// `FillConvexPoly(img, v, npts, color, 8, 16)`: points in 16-bit fixed point.
fn fill_convex_poly16(img: &mut Gray, pts: &[(i64, i64)], v: u8) {
    let shift = XY_SHIFT;
    let delta = 1i64 << shift >> 1;
    let (delta1, delta2) = (XY_ONE >> 1, XY_ONE >> 1);
    let (w, h) = (img.w as i64, img.h as i64);
    let n = pts.len();
    let mut p0 = pts[n - 1];
    let (mut xmin, mut xmax, mut ymin, mut ymax) = (pts[0].0, pts[0].0, pts[0].1, pts[0].1);
    let mut imin = 0;
    for (i, &p) in pts.iter().enumerate() {
        if p.1 < ymin {
            ymin = p.1;
            imin = i;
        }
        ymax = ymax.max(p.1);
        xmax = xmax.max(p.0);
        xmin = xmin.min(p.0);
        line2(img, p0, p, v);
        p0 = p;
    }
    let (xmin, xmax) = ((xmin + delta) >> shift, (xmax + delta) >> shift);
    let (ymin, ymax) = ((ymin + delta) >> shift, (ymax + delta) >> shift);
    if n < 3 || xmax < 0 || ymax < 0 || xmin >= w || ymin >= h {
        return;
    }
    let ymax = ymax.min(h - 1);
    #[derive(Clone, Copy)]
    struct E {
        idx: usize,
        di: usize,
        x: i64,
        dx: i64,
        ye: i64,
    }
    let mut edge = [
        E {
            idx: imin,
            di: 1,
            x: -XY_ONE,
            dx: 0,
            ye: ymin,
        },
        E {
            idx: imin,
            di: n - 1,
            x: -XY_ONE,
            dx: 0,
            ye: ymin,
        },
    ];
    let mut edges = n as i64;
    let mut y = ymin;
    loop {
        for e in &mut edge {
            if y >= e.ye {
                let mut idx0 = e.idx;
                let mut idx = idx0 + e.di;
                if idx >= n {
                    idx -= n;
                }
                loop {
                    edges -= 1;
                    if edges < 0 {
                        break;
                    }
                    let ty = (pts[idx].1 + delta) >> shift;
                    if ty > y {
                        let (xs, xe) = (pts[idx0].0, pts[idx].0);
                        e.ye = ty;
                        e.dx = ((xe - xs) * 2 + (ty - y)) / (2 * (ty - y));
                        e.x = xs;
                        e.idx = idx;
                        break;
                    }
                    idx0 = idx;
                    idx += e.di;
                    if idx >= n {
                        idx -= n;
                    }
                }
            }
        }
        if edges < 0 {
            break;
        }
        if y >= 0 {
            let (l, r) = if edge[0].x > edge[1].x {
                (1, 0)
            } else {
                (0, 1)
            };
            let (xx1, xx2) = (
                (edge[l].x + delta1) >> XY_SHIFT,
                (edge[r].x + delta2) >> XY_SHIFT,
            );
            if xx2 >= 0 && xx1 < w {
                hline(img, y, xx1, xx2, v);
            }
        }
        edge[0].x += edge[0].dx;
        edge[1].x += edge[1].dx;
        y += 1;
        if y > ymax {
            break;
        }
    }
}

/// `Circle(img, c, r, color, fill=1)`: OpenCV's midpoint circle, filled with horizontal runs.
fn circle_filled(img: &mut Gray, c: (i64, i64), radius: i64, v: u8) {
    let (mut err, mut dx, mut dy, mut plus, mut minus) =
        (0i64, radius, 0i64, 1i64, (radius << 1) - 1);
    while dx >= dy {
        for (yy, x1, x2) in [
            (c.1 - dy, c.0 - dx, c.0 + dx),
            (c.1 + dy, c.0 - dx, c.0 + dx),
            (c.1 - dx, c.0 - dy, c.0 + dy),
            (c.1 + dx, c.0 - dy, c.0 + dy),
        ] {
            if x1 < img.w as i64 && x2 >= 0 {
                hline(img, yy, x1, x2, v);
            }
        }
        dy += 1;
        err += plus;
        plus += 2;
        let mask: i64 = if err <= 0 { 0 } else { -1 };
        err -= minus & mask;
        dx += mask;
        minus -= mask & 2;
    }
}

/// `line(img, p0, p1, v, thickness)` with `LINE_8`: one pixel wide via [`line8`]; thicker as OpenCV's
/// `ThickLine` (a convex quadrilateral and a filled circle at each end).
pub fn line(img: &mut Gray, p0: (i64, i64), p1: (i64, i64), v: u8, thickness: i64) {
    if thickness <= 1 {
        line8(img, p0, p1, v);
        return;
    }
    let (q0, q1) = (
        (p0.0 << XY_SHIFT, p0.1 << XY_SHIFT),
        (p1.0 << XY_SHIFT, p1.1 << XY_SHIFT),
    );
    let inv = 1.0 / XY_ONE as f64;
    let dx = (q0.0 - q1.0) as f64 * inv;
    let dy = (q1.1 - q0.1) as f64 * inv;
    let r = dx * dx + dy * dy;
    let odd = thickness & 1;
    let t = thickness << (XY_SHIFT - 1);
    if r.abs() > f64::EPSILON {
        let rr = (t as f64 + odd as f64 * XY_ONE as f64 * 0.5) / r.sqrt();
        let dp = (
            (dy * rr).round_ties_even() as i64,
            (dx * rr).round_ties_even() as i64,
        );
        let quad = [
            (q0.0 + dp.0, q0.1 + dp.1),
            (q0.0 - dp.0, q0.1 - dp.1),
            (q1.0 - dp.0, q1.1 - dp.1),
            (q1.0 + dp.0, q1.1 + dp.1),
        ];
        fill_convex_poly16(img, &quad, v);
    }
    for q in [q0, q1] {
        let c = (
            (q.0 + (XY_ONE >> 1)) >> XY_SHIFT,
            (q.1 + (XY_ONE >> 1)) >> XY_SHIFT,
        );
        circle_filled(img, c, (t + (XY_ONE >> 1)) >> XY_SHIFT, v);
    }
}
