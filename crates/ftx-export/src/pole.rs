//! The pole of inaccessibility as the reference computes it: shapely's `polylabel` is GEOS's
//! `MaximumInscribedCircle` (3.14). A convex triangle or quadrilateral gets its exact centre; anything else a
//! branch-and-bound over square cells, seeded with one cell over the envelope and with GEOS's interior point,
//! in a priority queue by the cell's potential distance. The queue is a binary heap ordered like libc++'s
//! `std::priority_queue` (sift-up on push, Floyd's sift-down on pop), so equal potentials come off in the
//! same order as in the reference on macOS (the ROBUST collar's default grain agrees to 1e-9 mm).

use ftx_core::Vec2;

/// A polygon: its exterior ring and holes (open rings).
pub type Rings<'a> = &'a [&'a [Vec2]];

/// GEOS `Distance::pointToSegment`.
fn point_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let dist = |q: Vec2| ((p.x - q.x) * (p.x - q.x) + (p.y - q.y) * (p.y - q.y)).sqrt();
    if a.x == b.x && a.y == b.y {
        return dist(a);
    }
    let len2 = (b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y);
    let r = ((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y)) / len2;
    if r <= 0.0 {
        return dist(a);
    }
    if r >= 1.0 {
        return dist(b);
    }
    let s = ((a.y - p.y) * (b.x - a.x) - (a.x - p.x) * (b.y - a.y)) / len2;
    s.abs() * len2.sqrt()
}

/// Distance from `p` to the nearest edge of any ring.
#[must_use]
pub fn boundary_distance(rings: Rings, p: Vec2) -> f64 {
    let mut best = f64::INFINITY;
    for r in rings {
        let n = r.len();
        for i in 0..n {
            best = best.min(point_to_segment(p, r[i], r[(i + 1) % n]));
        }
    }
    best
}

/// Whether `p` is inside the polygon (even-odd over all rings; a point on the boundary may go either way,
/// which no caller can tell from its distance 0).
#[must_use]
pub fn covers(rings: Rings, p: Vec2) -> bool {
    let mut inside = false;
    for r in rings {
        let n = r.len();
        for i in 0..n {
            let (a, b) = (r[i], r[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) {
                let xc = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
                if p.x < xc {
                    inside = !inside;
                }
            }
        }
    }
    inside
}

/// Signed distance to the boundary: positive inside, negative outside (one pass).
#[must_use]
pub fn signed_distance(rings: Rings, p: Vec2) -> f64 {
    let mut inside = false;
    let mut best = f64::INFINITY;
    for r in rings {
        let n = r.len();
        for i in 0..n {
            let (a, b) = (r[i], r[(i + 1) % n]);
            best = best.min(point_to_segment(p, a, b));
            if (a.y > p.y) != (b.y > p.y) {
                let xc = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
                if p.x < xc {
                    inside = !inside;
                }
            }
        }
    }
    if inside { best } else { -best }
}

fn envelope(rings: Rings) -> (Vec2, Vec2) {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in rings.iter().flat_map(|r| r.iter()) {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    (lo, hi)
}

/// GEOS `InteriorPointArea` for one polygon: the midpoint of the widest interior section of a horizontal
/// scan line placed halfway between the vertex heights nearest the envelope's middle.
#[must_use]
pub fn interior_point(rings: Rings) -> Vec2 {
    let (lo, hi) = envelope(rings);
    let centre_y = (lo.y + hi.y) / 2.0;
    let (mut lo_y, mut hi_y) = (lo.y, hi.y);
    for p in rings.iter().flat_map(|r| r.iter()) {
        if p.y <= centre_y {
            if p.y > lo_y {
                lo_y = p.y;
            }
        } else if p.y < hi_y {
            hi_y = p.y;
        }
    }
    let scan_y = (hi_y + lo_y) / 2.0;
    let mut crossings = Vec::new();
    for r in rings {
        let n = r.len();
        for i in 0..n {
            let (p0, p1) = (r[i], r[(i + 1) % n]);
            if p0.y.min(p1.y) > scan_y || p0.y.max(p1.y) < scan_y || p0.y == p1.y {
                continue;
            }
            if (p0.y == scan_y && p1.y < scan_y) || (p1.y == scan_y && p0.y < scan_y) {
                continue;
            }
            let x = if p0.x == p1.x {
                p0.x
            } else {
                let m = (p1.y - p0.y) / (p1.x - p0.x);
                p0.x + (scan_y - p0.y) / m
            };
            crossings.push(x);
        }
    }
    crossings.sort_by(f64::total_cmp);
    let mut best = rings
        .first()
        .and_then(|r| r.first())
        .copied()
        .unwrap_or(Vec2::ZERO);
    let mut width = 0.0;
    for pair in crossings.as_chunks::<2>().0 {
        if pair[1] - pair[0] > width {
            width = pair[1] - pair[0];
            best = Vec2::new((pair[0] + pair[1]) / 2.0, scan_y);
        }
    }
    best
}

#[derive(Clone, Copy)]
struct Cell {
    x: f64,
    y: f64,
    h: f64,
    dist: f64,
    max_dist: f64,
}

impl Cell {
    fn new(x: f64, y: f64, h: f64, dist: f64) -> Self {
        Self {
            x,
            y,
            h,
            dist,
            max_dist: dist + h * std::f64::consts::SQRT_2,
        }
    }
}

/// A max-heap on `max_dist` with libc++'s algorithms.
struct Heap(Vec<Cell>);

impl Heap {
    fn less(a: &Cell, b: &Cell) -> bool {
        a.max_dist < b.max_dist
    }

    fn sift_up(v: &mut [Cell], len: usize) {
        if len > 1 {
            let mut last = len - 1;
            let mut parent = (len - 2) / 2;
            if Self::less(&v[parent], &v[last]) {
                let t = v[last];
                loop {
                    v[last] = v[parent];
                    last = parent;
                    if parent == 0 {
                        break;
                    }
                    parent = (parent - 1) / 2;
                    if !Self::less(&v[parent], &t) {
                        break;
                    }
                }
                v[last] = t;
            }
        }
    }

    fn push(&mut self, c: Cell) {
        self.0.push(c);
        let len = self.0.len();
        Self::sift_up(&mut self.0, len);
    }

    fn pop(&mut self) -> Option<Cell> {
        let len = self.0.len();
        if len == 0 {
            return None;
        }
        let top = self.0[0];
        if len > 1 {
            // Floyd's sift-down: move the larger child into the hole until a leaf
            let v = &mut self.0;
            let mut hole = 0;
            let mut child = 0;
            loop {
                child = 2 * child + 1;
                let mut ci = child;
                if child + 1 < len && Self::less(&v[ci], &v[ci + 1]) {
                    ci += 1;
                    child += 1;
                }
                v[hole] = v[ci];
                hole = ci;
                if child > (len - 2) / 2 {
                    break;
                }
            }
            let last = len - 1;
            if hole == last {
                v[hole] = top;
            } else {
                v[hole] = v[last];
                v[last] = top;
                Self::sift_up(v, hole + 1);
            }
        }
        self.0.pop()
    }
}

/// The exact centre for a convex triangle or quadrilateral, as GEOS 3.14 returns it: of the circles tangent
/// to three edge lines that fit inside, the largest, the last of the equal ones in edge order (which picks
/// the left end of a wide rectangle's plateau and the top end of a tall one's, like GEOS).
fn convex_small_centre(ring: &[Vec2]) -> Option<Vec2> {
    let n = ring.len();
    if !(3..=4).contains(&n) {
        return None;
    }
    let area = ftx_core::polyline::signed_area(ring);
    let sgn = if area > 0.0 { 1.0 } else { -1.0 };
    let turns = (0..n).map(|i| {
        let (a, b, c) = (ring[i], ring[(i + 1) % n], ring[(i + 2) % n]);
        sgn * ((b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x))
    });
    if turns.clone().any(|t| t <= 0.0) {
        return None;
    }
    // inward unit normal and offset of each edge line: n·p ≥ d inside
    let lines: Vec<(Vec2, f64)> = (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let d = b - a;
            let len = (d.x * d.x + d.y * d.y).sqrt();
            let nrm = Vec2::new(-d.y / len * sgn, d.x / len * sgn);
            (nrm, nrm.x * a.x + nrm.y * a.y)
        })
        .collect();
    let mut best: Option<(f64, Vec2)> = None;
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let [(ni, di), (nj, dj), (nk, dk)] = [lines[i], lines[j], lines[k]];
                // n·c − r = d for the three lines
                let det3 = |m: [[f64; 3]; 3]| {
                    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
                        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
                };
                let m = [[ni.x, ni.y, -1.0], [nj.x, nj.y, -1.0], [nk.x, nk.y, -1.0]];
                let det = det3(m);
                if det.abs() < 1e-12 {
                    continue;
                }
                let rhs = [di, dj, dk];
                let col = |c: usize| {
                    let mut mm = m;
                    for r in 0..3 {
                        mm[r][c] = rhs[r];
                    }
                    det3(mm) / det
                };
                let (cx, cy, r) = (col(0), col(1), col(2));
                if r <= 0.0 {
                    continue;
                }
                let tol = 1e-9 * r.max(1.0);
                if lines
                    .iter()
                    .any(|&(nm, dm)| nm.x * cx + nm.y * cy - dm < r - tol)
                {
                    continue;
                }
                if best.is_none_or(|(br, _)| r >= br - tol) {
                    best = Some((r, Vec2::new(cx, cy)));
                }
            }
        }
    }
    best.map(|(_, c)| c)
}

/// The centre of the largest circle inside the polygon, within `tolerance` (GEOS
/// `MaximumInscribedCircle::getCenter`; exact for a convex triangle or quadrilateral).
#[must_use]
pub fn pole_of_inaccessibility(rings: Rings, tolerance: f64) -> Vec2 {
    if rings.len() == 1
        && let Some(c) = convex_small_centre(rings[0])
    {
        return c;
    }
    let (lo, hi) = envelope(rings);
    let size = (hi.x - lo.x).max(hi.y - lo.y);
    let ip = interior_point(rings);
    let mut farthest = Cell::new(ip.x, ip.y, 0.0, signed_distance(rings, ip));
    if size == 0.0 {
        return Vec2::new(farthest.x, farthest.y);
    }
    let mut queue = Heap(Vec::new());
    let (cx, cy) = ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
    queue.push(Cell::new(
        cx,
        cy,
        size / 2.0,
        signed_distance(rings, Vec2::new(cx, cy)),
    ));
    let diam = ((hi.x - lo.x) * (hi.x - lo.x) + (hi.y - lo.y) * (hi.y - lo.y)).sqrt();
    let factor = (ftx_core::det::ln(diam / tolerance) as i64).max(1);
    let max_iter = 2000 + 2000 * factor as usize;
    let mut iter = 0;
    while iter < max_iter {
        let Some(cell) = queue.pop() else { break };
        iter += 1;
        if cell.dist > farthest.dist {
            farthest = cell;
        }
        if cell.max_dist - farthest.dist > tolerance {
            let h = cell.h / 2.0;
            for (dx, dy) in [(-h, -h), (h, -h), (-h, h), (h, h)] {
                let (x, y) = (cell.x + dx, cell.y + dy);
                queue.push(Cell::new(x, y, h, signed_distance(rings, Vec2::new(x, y))));
            }
        }
    }
    Vec2::new(farthest.x, farthest.y)
}
