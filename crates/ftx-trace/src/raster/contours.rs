//! OpenCV 5.0 `findContours` for 8-bit images with `CHAIN_APPROX_NONE` (`contours_new.cpp`,
//! `contours_common.*`: Suzuki's border following on a 1-pixel zero-padded copy, the contour tree and its
//! output order), and the contour functions the trace uses (`contourArea`, `arcLength`, `moments`,
//! `boundingRect`, `approxPolyDP`; `modules/geometry` in 5.0).
//!
//! The scanner is OpenCV's, step for step: border pixels are marked in the image as it goes (`nbd` labels,
//! the "right" flag in the sign bit), holes find their parent through the label table, and the result is a
//! preorder walk of the tree in which each parent's children come newest first (OpenCV returns contours
//! bottom to top).

use super::image::Gray;

/// A contour point (x, y) in image pixels.
pub type Pt = (i32, i32);

/// Retrieval mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `RETR_EXTERNAL`: outermost borders only.
    External,
    /// `RETR_CCOMP`: outer borders and their holes, two levels.
    CComp,
}

/// The contours and, per contour, `[next, previous, first child, parent]` (−1 for none).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contours {
    /// Border pixels in tracing order.
    pub contours: Vec<Vec<Pt>>,
    /// OpenCV's hierarchy rows.
    pub hierarchy: Vec<[i32; 4]>,
}

const DELTAS: [Pt; 8] = [
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];
const RIGHT: i8 = i8::MIN; // 0x80
const NEW: i8 = 2;
const FLAGS: i8 = -2; // 0xFE
const BLACK: i8 = 1;
const LVAL: i8 = 0x7f;
const MAX_SIZE: i8 = 16;

struct Node {
    parent: i32,
    first_child: i32,
    prev: i32,
    next: i32,
    ctable_next: i32,
    pts: Vec<Pt>,
    brect: (i32, i32, i32, i32),
    origin: Pt,
    is_hole: bool,
}

impl Node {
    fn new() -> Self {
        Self {
            parent: -1,
            first_child: -1,
            prev: -1,
            next: -1,
            ctable_next: -1,
            pts: Vec::new(),
            brect: (0, 0, 0, 0),
            origin: (0, 0),
            is_hole: false,
        }
    }
}

struct Scanner {
    img: Vec<i8>,
    w: i32,
    h: i32,
    pt: Pt,
    lnbd: Pt,
    nbd: i8,
    mode: Mode,
    nodes: Vec<Node>,
    ctable: [i32; 128],
}

impl Scanner {
    fn at(&self, x: i32, y: i32) -> i8 {
        self.img[(y * self.w + x) as usize]
    }

    fn delta(&self, s: i8) -> i32 {
        let d = DELTAS[(s % 8) as usize];
        d.0 + d.1 * self.w
    }

    fn simple(&self) -> bool {
        self.mode == Mode::External
    }

    fn add_child(&mut self, parent: usize, child: usize) {
        let fc = self.nodes[parent].first_child;
        if fc != -1 {
            self.nodes[fc as usize].prev = child as i32;
            self.nodes[child].next = fc;
        }
        self.nodes[parent].first_child = child as i32;
        self.nodes[child].parent = parent as i32;
        self.nodes[child].prev = -1;
    }

    /// `icvFetchContourEx<schar>` with `CHAIN_APPROX_NONE`.
    fn fetch(&mut self, start: Pt, nbd: i8, node: usize) {
        let i0 = start.1 * self.w + start.0;
        let mut pt = self.nodes[node].origin;
        let mut rect = (pt.0, pt.1, pt.0, pt.1);
        let mut s_end: i8 = if self.nodes[node].is_hole { 0 } else { 4 };
        let mut s = s_end;
        let mut i1;
        loop {
            s = (s - 1) & 7;
            i1 = i0 + self.delta(s);
            if self.img[i1 as usize] != 0 || s == s_end {
                break;
            }
        }
        if s == s_end {
            self.img[i0 as usize] = nbd | RIGHT;
            self.nodes[node].pts.push(pt);
        } else {
            let mut i3 = i0;
            let mut prev_s = s ^ 4;
            loop {
                s_end = s;
                s = s.min(15);
                let mut i4 = i3;
                while s < MAX_SIZE - 1 {
                    s += 1;
                    i4 = i3 + self.delta(s);
                    if self.img[i4 as usize] != 0 {
                        break;
                    }
                }
                s &= 7;
                if ((i32::from(s) - 1) as u32) < (i32::from(s_end) as u32) {
                    self.img[i3 as usize] = nbd | RIGHT;
                } else if self.img[i3 as usize] == BLACK {
                    self.img[i3 as usize] = nbd;
                }
                self.nodes[node].pts.push(pt);
                if s != prev_s {
                    if pt.0 < rect.0 {
                        rect.0 = pt.0;
                    } else if pt.0 > rect.2 {
                        rect.2 = pt.0;
                    }
                    if pt.1 < rect.1 {
                        rect.1 = pt.1;
                    } else if pt.1 > rect.3 {
                        rect.3 = pt.1;
                    }
                }
                prev_s = s;
                let d = DELTAS[s as usize];
                pt = (pt.0 + d.0, pt.1 + d.1);
                if i4 == i0 && i3 == i1 {
                    break;
                }
                i3 = i4;
                s = (s + 4) & 7;
            }
        }
        rect.2 -= rect.0 - 1;
        rect.3 -= rect.1 - 1;
        self.nodes[node].brect = rect;
    }

    /// `icvTraceContour<schar>`.
    fn trace(&self, start: Pt, end: Pt, is_hole: bool) -> bool {
        let stop = end.1 * self.w + end.0;
        let i0 = start.1 * self.w + start.0;
        let s_end: i8 = if is_hole { 0 } else { 4 };
        let mut s = s_end;
        let mut i1;
        loop {
            s = (s - 1) & 7;
            i1 = i0 + self.delta(s);
            if self.img[i1 as usize] != 0 || s == s_end {
                break;
            }
        }
        let mut i3 = i0;
        if s == s_end {
            return i3 == stop;
        }
        loop {
            s = s.min(15);
            let mut i4 = i3;
            while s < MAX_SIZE - 1 {
                s += 1;
                i4 = i3 + self.delta(s);
                if self.img[i4 as usize] != 0 {
                    break;
                }
            }
            if i3 == stop {
                if self.img[i3 as usize] & RIGHT == 0 {
                    return true;
                }
                let mut t = s;
                loop {
                    t = (t - 1) & 7;
                    if self.img[(i3 + self.delta(t)) as usize] != 0 {
                        break;
                    }
                    if t == 0 {
                        return true;
                    }
                }
            }
            if i4 == i0 && i3 == i1 {
                break;
            }
            i3 = i4;
            s = (s + 4) & 7;
        }
        false
    }

    fn make_contour(&mut self, nbd: &mut i8, is_hole: bool, x: i32, y: i32) -> usize {
        let start = (x - i32::from(is_hole), y);
        let idx = self.nodes.len();
        let mut node = Node::new();
        node.is_hole = is_hole;
        node.origin = (start.0 - 1, start.1 - 1); // offset (−1, −1): the padding
        self.nodes.push(node);
        if self.simple() {
            self.fetch(start, NEW, idx);
        } else {
            let lval = *nbd;
            *nbd = nbd.wrapping_add(1) & LVAL; // OpenCV adds in int: 127 + 1 masks to 0
            if *nbd == 0 {
                *nbd = BLACK | NEW;
            }
            self.fetch(start, lval, idx);
            self.nodes[idx].brect.0 += 1;
            self.nodes[idx].brect.1 += 1;
            self.nodes[idx].ctable_next = self.ctable[lval as usize];
            self.ctable[lval as usize] = idx as i32;
        }
        self.nodes[idx].origin = start;
        idx
    }

    fn first_bounding(&self, last: Pt, y: i32, lval: i8, par: i32) -> i32 {
        let end = (last.0, y);
        let mut res = par;
        let mut cur = self.ctable[lval as usize];
        while cur != -1 {
            let c = &self.nodes[cur as usize];
            if last.0 - c.brect.0 < c.brect.2 && last.1 - c.brect.1 < c.brect.3 {
                if res != -1 {
                    let r = &self.nodes[res as usize];
                    if self.trace(r.origin, end, r.is_hole) {
                        break;
                    }
                }
                res = cur;
            }
            cur = c.ctable_next;
        }
        res
    }

    fn contour_scan(&mut self, prev: i8, p: i8, last: &mut Pt, x: i32, y: i32) -> bool {
        let mut is_hole = false;
        if !(prev == 0 && p == 1) {
            if p != 0 || prev < 1 {
                return false;
            }
            if prev & FLAGS != 0 {
                last.0 = x - 1;
            }
            is_hole = true;
        }
        if self.mode == Mode::External && (is_hole || self.at(last.0, last.1) > 0) {
            return false;
        }
        let mut main_parent: i32 = -1;
        if self.simple() || !is_hole || last.0 <= 0 {
            main_parent = 0;
        } else {
            let lval = self.at(last.0, last.1) & LVAL;
            main_parent = self.first_bounding(*last, y, lval, main_parent);
            let mp = &self.nodes[main_parent as usize];
            if mp.is_hole == is_hole {
                main_parent = if mp.parent != -1 { mp.parent } else { 0 };
            }
        }
        last.0 = x - i32::from(is_hole);
        let mut nbd = self.nbd;
        let c = self.make_contour(&mut nbd, is_hole, x, y);
        self.add_child(main_parent as usize, c);
        self.pt = (x + 1, y);
        self.nbd = nbd;
        true
    }

    fn find_next(&mut self) -> bool {
        let (mut x, mut y) = self.pt;
        let (width, height) = (self.w - 1, self.h - 1);
        let mut last = self.lnbd;
        let mut prev = self.at(x - 1, y);
        while y < height {
            let mut p: i8 = 0;
            while x < width {
                while x < width {
                    p = self.at(x, y);
                    if p != prev {
                        break;
                    }
                    x += 1;
                }
                if x >= width {
                    break;
                }
                if self.contour_scan(prev, p, &mut last, x, y) {
                    self.lnbd = last;
                    return true;
                }
                prev = p;
                if prev & FLAGS != 0 {
                    last.0 = x;
                }
                x += 1;
            }
            last = (0, y + 1);
            x = 1;
            prev = 0;
            y += 1;
        }
        false
    }
}

/// `findContours(img, mode, CHAIN_APPROX_NONE)` (any non-zero pixel is foreground).
#[must_use]
pub fn find_contours(img: &Gray, mode: Mode) -> Contours {
    let (w, h) = (img.w as i32 + 2, img.h as i32 + 2);
    let mut pad = vec![0i8; (w * h) as usize];
    for y in 0..img.h {
        for x in 0..img.w {
            pad[(y + 1) * w as usize + x + 1] = i8::from(img.data[y * img.w + x] != 0);
        }
    }
    let mut root = Node::new();
    root.is_hole = true;
    root.brect = (0, 0, w, h);
    let mut sc = Scanner {
        img: pad,
        w,
        h,
        pt: (1, 1),
        lnbd: (0, 1),
        nbd: 2,
        mode,
        nodes: vec![root],
        ctable: [-1; 128],
    };
    while sc.find_next() {}
    // preorder, each parent's children newest first
    let n = sc.nodes.len();
    let mut order = Vec::with_capacity(n);
    let mut stack = vec![0usize];
    while let Some(i) = stack.pop() {
        order.push(i);
        let mut cur = sc.nodes[i].first_child;
        if cur != -1 {
            while sc.nodes[cur as usize].next != -1 {
                cur = sc.nodes[cur as usize].next;
            }
        }
        while cur != -1 {
            stack.push(cur as usize);
            cur = sc.nodes[cur as usize].prev;
        }
    }
    let mut index = vec![-1i32; n + 1];
    let mut k = 0;
    for &i in &order {
        if i != 0 {
            index[i + 1] = k;
            k += 1;
        }
    }
    let map = |v: i32| index[(v + 1) as usize];
    let mut out = Contours::default();
    for &i in order.iter().filter(|&&i| i != 0) {
        let nd = &sc.nodes[i];
        out.contours.push(nd.pts.clone());
        out.hierarchy.push([
            map(nd.next),
            map(nd.prev),
            map(nd.first_child),
            map(nd.parent),
        ]);
    }
    out
}

/// `contourArea(c)` (unsigned; the shoelace sum over float32-converted points, in double).
#[must_use]
pub fn area(c: &[Pt]) -> f64 {
    if c.is_empty() {
        return 0.0;
    }
    let f = |p: Pt| (f64::from(p.0 as f32), f64::from(p.1 as f32));
    let mut prev = f(c[c.len() - 1]);
    let mut a = 0.0;
    for &p in c {
        let q = f(p);
        a += prev.0 * q.1 - prev.1 * q.0;
        prev = q;
    }
    (a * 0.5).abs()
}

/// `arcLength(c, closed)`: float32 steps, summed in double.
#[must_use]
pub fn arc_length(c: &[Pt], closed: bool) -> f64 {
    if c.len() <= 1 {
        return 0.0;
    }
    let f = |p: Pt| (p.0 as f32, p.1 as f32);
    let mut prev = f(if closed { c[c.len() - 1] } else { c[0] });
    let mut perimeter = 0.0f64;
    for &p in c {
        let q = f(p);
        let (dx, dy) = (q.0 - prev.0, q.1 - prev.1);
        perimeter += f64::from((dx * dx + dy * dy).sqrt());
        prev = q;
    }
    perimeter
}

/// The spatial moments `(m00, m10, m01)` of a contour (`moments(c)`; zero when |m00| ≤ FLT_EPSILON).
#[must_use]
pub fn moments(c: &[Pt]) -> (f64, f64, f64) {
    if c.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let (mut a00, mut a10, mut a01) = (0.0f64, 0.0f64, 0.0f64);
    let last = c[c.len() - 1];
    let (mut xi_1, mut yi_1) = (f64::from(last.0), f64::from(last.1));
    for &p in c {
        let (xi, yi) = (f64::from(p.0), f64::from(p.1));
        let dxy = xi_1 * yi - xi * yi_1;
        a00 += dxy;
        a10 += dxy * (xi_1 + xi);
        a01 += dxy * (yi_1 + yi);
        xi_1 = xi;
        yi_1 = yi;
    }
    if a00.abs() <= f64::from(f32::EPSILON) {
        return (0.0, 0.0, 0.0);
    }
    let (h, s) = if a00 > 0.0 {
        (0.5, 0.166_666_666_666_666_66)
    } else {
        (-0.5, -0.166_666_666_666_666_66)
    };
    (a00 * h, a10 * s, a01 * s)
}

/// `boundingRect(c)`: `(x, y, w, h)`.
#[must_use]
pub fn bounding_rect(c: &[Pt]) -> (i32, i32, i32, i32) {
    if c.is_empty() {
        return (0, 0, 0, 0);
    }
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for &(x, y) in c {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    (x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

/// A point type `approxPolyDP` runs on: OpenCV subtracts in the point's own type, then works in double.
pub trait DpPoint: Copy + PartialEq {
    /// `a.x − b.x` and `a.y − b.y` in the point type, as doubles.
    fn sub(self, o: Self) -> (f64, f64);
}

impl DpPoint for Pt {
    fn sub(self, o: Self) -> (f64, f64) {
        (f64::from(self.0 - o.0), f64::from(self.1 - o.1))
    }
}

impl DpPoint for (f32, f32) {
    fn sub(self, o: Self) -> (f64, f64) {
        (f64::from(self.0 - o.0), f64::from(self.1 - o.1))
    }
}

/// `approxPolyDP(curve, eps, closed)`: OpenCV's Ramer–Douglas–Peucker (its farthest-pair start for closed
/// curves, its stack order and its final clean-up of nearly straight points).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn approx_poly_dp<P: DpPoint>(src: &[P], eps: f64, closed: bool) -> Vec<P> {
    let count0 = src.len();
    if count0 == 0 {
        return Vec::new();
    }
    let count = count0;
    let mut dst: Vec<P> = Vec::with_capacity(count);
    let eps = eps * eps;
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut init_iters = 3;
    let mut is_closed = closed;
    let (mut slice, mut right_slice) = ((0usize, 0usize), (0usize, 0usize));
    let mut start_pt = src[0];
    let mut pos = 0usize;
    let mut le_eps = false;
    let read = |pos: &mut usize| -> P {
        let p = src[*pos];
        *pos += 1;
        if *pos >= count {
            *pos = 0;
        }
        p
    };
    if !is_closed {
        right_slice.0 = count;
        let end = src[0];
        start_pt = src[count - 1];
        if start_pt != end {
            slice = (0, count - 1);
            stack.push(slice);
        } else {
            is_closed = true;
            init_iters = 1;
        }
    }
    if is_closed {
        right_slice.0 = 0;
        for _ in 0..init_iters {
            let mut max_dist = 0.0;
            pos = (pos + right_slice.0) % count;
            start_pt = read(&mut pos);
            for j in 1..count {
                let p = read(&mut pos);
                let (dx, dy) = p.sub(start_pt);
                let dist = dx * dx + dy * dy;
                if dist > max_dist {
                    max_dist = dist;
                    right_slice.0 = j;
                }
            }
            le_eps = max_dist <= eps;
        }
        if le_eps {
            dst.push(start_pt);
        } else {
            slice.0 = pos % count;
            right_slice.1 = slice.0;
            right_slice.0 = (right_slice.0 + slice.0) % count;
            slice.1 = right_slice.0;
            stack.push(right_slice);
            stack.push(slice);
        }
    }
    while let Some(sl) = stack.pop() {
        slice = sl;
        let end_pt = src[slice.1];
        pos = slice.0;
        start_pt = read(&mut pos);
        if pos != slice.1 {
            let (dx, dy) = end_pt.sub(start_pt);
            let seg2 = dx * dx + dy * dy;
            let mut max_d = 0.0;
            while pos != slice.1 {
                let p = read(&mut pos);
                let (px, py) = p.sub(start_pt);
                let projection = px * dx + py * dy;
                let d = if projection < 0.0 {
                    (px * px + py * py) * seg2
                } else if projection > seg2 {
                    let (ex, ey) = p.sub(end_pt);
                    (ex * ex + ey * ey) * seg2
                } else {
                    let dist = py * dx - px * dy;
                    dist * dist
                };
                if d > max_d {
                    max_d = d;
                    right_slice.0 = (pos + count - 1) % count;
                }
            }
            le_eps = max_d <= eps * seg2;
        } else {
            le_eps = true;
            start_pt = src[slice.0];
        }
        if le_eps {
            dst.push(start_pt);
        } else {
            right_slice.1 = slice.1;
            slice.1 = right_slice.0;
            stack.push(right_slice);
            stack.push(slice);
        }
    }
    if !is_closed {
        dst.push(src[count - 1]);
    }
    // clean-up: points on nearly straight lines
    let is_closed = closed;
    let count = dst.len();
    let mut new_count = count;
    let rd = |pos: &mut usize, dst: &[P]| -> P {
        let p = dst[*pos];
        *pos += 1;
        if *pos >= count {
            *pos = 0;
        }
        p
    };
    let mut pos = if is_closed { count - 1 } else { 0 };
    let mut start = rd(&mut pos, &dst);
    let mut wpos = pos;
    let mut pt = rd(&mut pos, &dst);
    let lo = usize::from(!is_closed);
    let mut i = lo;
    while i < count - lo && new_count > 2 {
        let end = rd(&mut pos, &dst);
        let (dx, dy) = end.sub(start);
        let (px, py) = pt.sub(start);
        let dist = (px * dy - py * dx).abs();
        let (ex, ey) = end.sub(pt);
        let inner = px * ex + py * ey;
        if dist * dist <= 0.5 * eps * (dx * dx + dy * dy) && dx != 0.0 && dy != 0.0 && inner >= 0.0
        {
            new_count -= 1;
            dst[wpos] = end;
            start = end;
            wpos += 1;
            if wpos >= count {
                wpos = 0;
            }
            pt = rd(&mut pos, &dst);
            i += 2;
            continue;
        }
        dst[wpos] = pt;
        start = pt;
        wpos += 1;
        if wpos >= count {
            wpos = 0;
        }
        pt = end;
        i += 1;
    }
    if !is_closed {
        dst[wpos] = pt;
    }
    dst.truncate(new_count);
    dst
}
