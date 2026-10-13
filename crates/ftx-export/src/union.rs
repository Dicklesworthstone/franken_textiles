//! The exterior ring of a union of simple polygons, for the union fallback of the cut outline
//! (`docs/spec/03` §4.3; the reference unions with GEOS).
//!
//! Every edge is split where it meets another; a piece of edge is on the union's boundary when a point just
//! to its right (outside its own counter-clockwise polygon) is in no polygon and a point just to its left is
//! in one. The boundary pieces are chained into rings, turning as far left as possible at a vertex where
//! several leave (so parts that touch at a point come out as separate rings), and the counter-clockwise ring
//! of largest area is returned.

use std::collections::HashMap;

use ftx_core::Vec2;
use ftx_core::polyline::signed_area;

/// Points closer than this (mm) are one node.
const SNAP: f64 = 1e-9;
/// How far (mm) the side probes sit from a piece of edge.
const PROBE: f64 = 1e-6;

#[derive(Clone, Copy)]
struct Seg {
    a: Vec2,
    b: Vec2,
}

fn cross(u: Vec2, v: Vec2) -> f64 {
    u.x * v.y - u.y * v.x
}

/// A dense uniform grid of item ids over the cells their boxes touch.
struct Grid {
    lo: Vec2,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<usize>>,
}

impl Grid {
    fn new(lo: Vec2, hi: Vec2, cell: f64) -> Self {
        let nx = (((hi.x - lo.x) / cell).floor() as usize + 1).max(1);
        let ny = (((hi.y - lo.y) / cell).floor() as usize + 1).max(1);
        Self {
            lo,
            cell,
            nx,
            ny,
            cells: vec![Vec::new(); nx * ny],
        }
    }

    fn range(&self, lo: Vec2, hi: Vec2) -> (usize, usize, usize, usize) {
        let ix =
            |x: f64, n: usize| (((x - self.lo.x) / self.cell).floor().max(0.0) as usize).min(n - 1);
        let iy =
            |y: f64, n: usize| (((y - self.lo.y) / self.cell).floor().max(0.0) as usize).min(n - 1);
        (
            ix(lo.x, self.nx),
            ix(hi.x, self.nx),
            iy(lo.y, self.ny),
            iy(hi.y, self.ny),
        )
    }

    fn insert(&mut self, id: usize, lo: Vec2, hi: Vec2) {
        let (x0, x1, y0, y1) = self.range(lo, hi);
        for j in y0..=y1 {
            for i in x0..=x1 {
                self.cells[j * self.nx + i].push(id);
            }
        }
    }

    fn at(&self, p: Vec2) -> &[usize] {
        let (i, _, j, _) = self.range(p, p);
        &self.cells[j * self.nx + i]
    }
}

fn bbox(pts: &[Vec2]) -> (Vec2, Vec2) {
    pts.iter().fold(
        (
            Vec2::new(f64::INFINITY, f64::INFINITY),
            Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(lo, hi), p| {
            (
                Vec2::new(lo.x.min(p.x), lo.y.min(p.y)),
                Vec2::new(hi.x.max(p.x), hi.y.max(p.y)),
            )
        },
    )
}

/// A polygon with its edges bucketed in horizontal slabs for fast winding-number tests. Its area is where the
/// winding number has the sign `wind` of its signed area (GEOS's positive depth): all of a simple polygon, of
/// a self-intersecting one the lobes turning the way the whole ring does, which is what GEOS's `buffer(0)`
/// keeps (its ring orientation is by area; that rule predicted the kept lobes of 184 of 184 random bow-ties,
/// the turn at the top vertex 117), and of a self-overlapping offset ring everything it covers.
struct Indexed {
    ring: Vec<Vec2>,
    wind: i32,
    lo: Vec2,
    hi: Vec2,
    slab: f64,
    slabs: Vec<Vec<usize>>,
}

impl Indexed {
    fn new(ring: Vec<Vec2>) -> Self {
        let (lo, hi) = bbox(&ring);
        let k = (ring.len() / 8).clamp(1, 256);
        let slab = ((hi.y - lo.y) / k as f64).max(1e-12);
        let mut slabs = vec![Vec::new(); k];
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let s0 = (((a.y.min(b.y) - lo.y) / slab).floor().max(0.0) as usize).min(k - 1);
            let s1 = (((a.y.max(b.y) - lo.y) / slab).floor().max(0.0) as usize).min(k - 1);
            for s in &mut slabs[s0..=s1] {
                s.push(i);
            }
        }
        let wind = if signed_area(&ring) > 0.0 { 1 } else { -1 };
        Self {
            ring,
            wind,
            lo,
            hi,
            slab,
            slabs,
        }
    }

    fn contains(&self, p: Vec2) -> bool {
        if p.x < self.lo.x || p.x > self.hi.x || p.y < self.lo.y || p.y > self.hi.y {
            return false;
        }
        let k = self.slabs.len();
        let s = (((p.y - self.lo.y) / self.slab).floor().max(0.0) as usize).min(k - 1);
        let n = self.ring.len();
        let mut w = 0;
        for &i in &self.slabs[s] {
            let (a, b) = (self.ring[i], self.ring[(i + 1) % n]);
            let side = cross(b - a, p - a);
            if a.y <= p.y && b.y > p.y && side > 0.0 {
                w += 1;
            } else if b.y <= p.y && a.y > p.y && side < 0.0 {
                w -= 1;
            }
        }
        w * self.wind > 0
    }
}

/// Without repeated points; `None` without area.
fn normalize(poly: &[Vec2]) -> Option<Vec<Vec2>> {
    let mut ring: Vec<Vec2> = Vec::with_capacity(poly.len());
    for &p in poly {
        if ring.last() != Some(&p) {
            ring.push(p);
        }
    }
    while ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    if ring.len() < 3 {
        return None;
    }
    // no area: every point on the line through the first two distinct ones
    let (o, d) = (ring[0], ring[1] - ring[0]);
    if !ring
        .iter()
        .any(|&p| cross(d, p - o).abs() > 1e-12 * d.norm().max(1.0))
    {
        return None;
    }
    Some(ring)
}

/// The counter-clockwise exterior ring (open) of the largest part of the union of `polys`, or `None` when
/// the union has no area.
pub(crate) fn union_exterior(polys: &[Vec<Vec2>]) -> Option<Vec<Vec2>> {
    let rings: Vec<Vec<Vec2>> = polys.iter().filter_map(|p| normalize(p)).collect();
    if rings.is_empty() {
        return None;
    }
    let mut segs: Vec<Seg> = Vec::new();
    for r in &rings {
        let n = r.len();
        segs.extend((0..n).map(|i| Seg {
            a: r[i],
            b: r[(i + 1) % n],
        }));
    }
    let all: Vec<Vec2> = rings.iter().flatten().copied().collect();
    let (lo, hi) = bbox(&all);
    let mean = segs.iter().map(|s| (s.b - s.a).norm()).sum::<f64>() / segs.len() as f64;
    let extent = (hi.x - lo.x).max(hi.y - lo.y);
    let cell = (2.0 * mean).max(extent / 1024.0).max(1e-6);

    // split points per segment: (parameter, point)
    let mut splits: Vec<Vec<(f64, Vec2)>> =
        segs.iter().map(|s| vec![(0.0, s.a), (1.0, s.b)]).collect();
    let mut grid = Grid::new(lo, hi, cell);
    for (i, s) in segs.iter().enumerate() {
        let (l, h) = bbox(&[s.a, s.b]);
        grid.insert(i, l, h);
    }
    let on_segment = |s: &Seg, p: Vec2| -> Option<f64> {
        let d = s.b - s.a;
        let l2 = d.x * d.x + d.y * d.y;
        if l2 == 0.0 || p == s.a || p == s.b {
            return None;
        }
        let t = ((p.x - s.a.x) * d.x + (p.y - s.a.y) * d.y) / l2;
        let dist = cross(d, p - s.a).abs() / l2.sqrt();
        (t > 0.0 && t < 1.0 && dist <= SNAP).then_some(t)
    };
    for c in 0..grid.cells.len() {
        let ids = &grid.cells[c];
        for (k, &i) in ids.iter().enumerate() {
            for &j in &ids[k + 1..] {
                let (si, sj) = (segs[i], segs[j]);
                let (li, hi_) = bbox(&[si.a, si.b]);
                let (lj, hj) = bbox(&[sj.a, sj.b]);
                if li.x > hj.x || lj.x > hi_.x || li.y > hj.y || lj.y > hi_.y {
                    continue;
                }
                // handle the pair once: in the first cell of the overlap of their boxes
                let ov_lo = Vec2::new(li.x.max(lj.x), li.y.max(lj.y));
                let (x0, _, y0, _) = grid.range(ov_lo, ov_lo);
                if y0 * grid.nx + x0 != c {
                    continue;
                }
                let di = si.b - si.a;
                let dj = sj.b - sj.a;
                let d1 = cross(dj, si.a - sj.a);
                let d2 = cross(dj, si.b - sj.a);
                let d3 = cross(di, sj.a - si.a);
                let d4 = cross(di, sj.b - si.a);
                if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
                    && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
                {
                    let ti = d1 / (d1 - d2);
                    let tj = d3 / (d3 - d4);
                    let p = si.a + di * ti;
                    splits[i].push((ti, p));
                    splits[j].push((tj, p));
                    continue;
                }
                for p in [sj.a, sj.b] {
                    if let Some(t) = on_segment(&si, p) {
                        splits[i].push((t, p));
                    }
                }
                for p in [si.a, si.b] {
                    if let Some(t) = on_segment(&sj, p) {
                        splits[j].push((t, p));
                    }
                }
            }
        }
    }

    // nodes (snapped) and the pieces of edge between them
    let mut node_of: HashMap<(i64, i64), usize> = HashMap::new();
    let mut nodes: Vec<Vec2> = Vec::new();
    let mut node = |p: Vec2| -> usize {
        let key = ((p.x / SNAP).round() as i64, (p.y / SNAP).round() as i64);
        *node_of.entry(key).or_insert_with(|| {
            nodes.push(p);
            nodes.len() - 1
        })
    };
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    for sp in &mut splits {
        sp.sort_by(|a, b| a.0.total_cmp(&b.0));
        let ids: Vec<usize> = sp.iter().map(|&(_, p)| node(p)).collect();
        for w in ids.windows(2) {
            if w[0] != w[1] {
                pieces.push((w[0], w[1]));
            }
        }
    }

    // keep the pieces with the union's outside on their right
    let indexed: Vec<Indexed> = rings.into_iter().map(Indexed::new).collect();
    let mut pgrid = Grid::new(lo, hi, (extent / 64.0).max(cell));
    for (i, p) in indexed.iter().enumerate() {
        pgrid.insert(i, p.lo, p.hi);
    }
    let inside = |p: Vec2| pgrid.at(p).iter().any(|&i| indexed[i].contains(p));
    let mut boundary: Vec<(usize, usize)> = pieces
        .into_iter()
        .filter(|&(a, b)| {
            let (pa, pb) = (nodes[a], nodes[b]);
            let d = pb - pa;
            let len = d.norm();
            let m = Vec2::new(0.5 * (pa.x + pb.x), 0.5 * (pa.y + pb.y));
            let r = Vec2::new(d.y / len, -d.x / len) * PROBE.min(0.25 * len);
            inside(m + r) != inside(m - r)
        })
        .map(|(a, b)| {
            // oriented with the union on its left
            let (pa, pb) = (nodes[a], nodes[b]);
            let d = pb - pa;
            let len = d.norm();
            let m = Vec2::new(0.5 * (pa.x + pb.x), 0.5 * (pa.y + pb.y));
            let r = Vec2::new(d.y / len, -d.x / len) * PROBE.min(0.25 * len);
            if inside(m - r) { (a, b) } else { (b, a) }
        })
        .collect();
    boundary.sort_unstable();
    boundary.dedup();
    // drop dangles: sliver tails thinner than the probe leave chains that end at a node with no way out (or
    // start at one with no way in)
    let mut indeg = vec![0usize; nodes.len()];
    let mut outdeg = vec![0usize; nodes.len()];
    for &(a, b) in &boundary {
        outdeg[a] += 1;
        indeg[b] += 1;
    }
    let mut alive = vec![true; boundary.len()];
    let mut ins: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut outs: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (k, &(a, b)) in boundary.iter().enumerate() {
        outs[a].push(k);
        ins[b].push(k);
    }
    let mut queue: Vec<usize> = (0..nodes.len())
        .filter(|&v| (indeg[v] == 0) != (outdeg[v] == 0))
        .collect();
    while let Some(v) = queue.pop() {
        let dead: Vec<usize> = if outdeg[v] == 0 {
            ins[v].clone()
        } else if indeg[v] == 0 {
            outs[v].clone()
        } else {
            Vec::new()
        };
        for k in dead {
            if !alive[k] {
                continue;
            }
            alive[k] = false;
            let (a, b) = boundary[k];
            outdeg[a] -= 1;
            indeg[b] -= 1;
            for u in [a, b] {
                if (indeg[u] == 0) != (outdeg[u] == 0) {
                    queue.push(u);
                }
            }
        }
    }
    let boundary: Vec<(usize, usize)> = boundary
        .into_iter()
        .zip(alive)
        .filter(|(_, a)| *a)
        .map(|(e, _)| e)
        .collect();

    // chain into rings, turning as far left as possible; a walk that comes back to one of its nodes closes
    // a ring there, and a walk that runs out of edges leaves a dangling chain behind
    let mut out_edges: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (k, &(a, _)) in boundary.iter().enumerate() {
        out_edges[a].push(k);
    }
    let mut used = vec![false; boundary.len()];
    let mut best: Option<(f64, Vec<Vec2>)> = None;
    let mut keep = |ring: Vec<Vec2>| {
        let area = signed_area(&ring);
        if ring.len() >= 3 && area > 0.0 && best.as_ref().is_none_or(|(a, _)| area > *a) {
            best = Some((area, ring));
        }
    };
    let mut at: HashMap<usize, usize> = HashMap::new(); // node -> its index in the walk
    for start in 0..boundary.len() {
        if used[start] {
            continue;
        }
        let mut path: Vec<usize> = Vec::new();
        at.clear();
        let mut e = start;
        loop {
            used[e] = true;
            let (a, b) = boundary[e];
            at.insert(a, path.len());
            path.push(e);
            if let Some(&k) = at.get(&b) {
                let cycle: Vec<usize> = path.drain(k..).collect();
                for &c in &cycle {
                    at.remove(&boundary[c].0);
                }
                keep(cycle.iter().map(|&c| nodes[boundary[c].0]).collect());
            }
            let din = nodes[b] - nodes[a];
            let next = out_edges[b]
                .iter()
                .copied()
                .filter(|&k| !used[k])
                .max_by(|&p, &q| {
                    let turn = |k: usize| {
                        let dout = nodes[boundary[k].1] - nodes[b];
                        cross(din, dout).atan2(din.x * dout.x + din.y * dout.y)
                    };
                    turn(p).total_cmp(&turn(q))
                });
            match next {
                Some(k) => e = k,
                None => break,
            }
        }
    }
    best.map(|(_, r)| r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Vec2> {
        vec![
            Vec2::new(x0, y0),
            Vec2::new(x1, y0),
            Vec2::new(x1, y1),
            Vec2::new(x0, y1),
        ]
    }

    #[test]
    fn overlapping_and_abutting_rectangles() {
        // two overlapping squares and one sharing an edge with the second
        let u = union_exterior(&[
            rect(0.0, 0.0, 2.0, 2.0),
            rect(1.0, 1.0, 3.0, 3.0),
            rect(3.0, 1.0, 4.0, 3.0),
        ])
        .unwrap();
        assert!(
            (signed_area(&u) - (4.0 + 4.0 - 1.0 + 2.0)).abs() < 1e-12,
            "{u:?}"
        );
        // a square inside another adds nothing
        let u = union_exterior(&[rect(0.0, 0.0, 4.0, 4.0), rect(1.0, 1.0, 2.0, 2.0)]).unwrap();
        assert!((signed_area(&u) - 16.0).abs() < 1e-12);
        // squares touching at a corner: the larger
        let u = union_exterior(&[rect(0.0, 0.0, 1.0, 1.0), rect(1.0, 1.0, 3.0, 3.0)]).unwrap();
        assert!((signed_area(&u) - 4.0).abs() < 1e-12);
    }

    #[test]
    fn bow_ties_keep_the_lobes_turning_like_the_ring() {
        // GEOS buffer(0): the canonical bow-tie keeps its clockwise lobe (zero net area counts as clockwise)
        let pt = |x, y| Vec2::new(x, y);
        let u = union_exterior(&[vec![pt(0.0, 0.0), pt(1.0, 1.0), pt(1.0, 0.0), pt(0.0, 1.0)]])
            .unwrap();
        assert!((signed_area(&u) - 0.25).abs() < 1e-12);
        assert!(u.iter().all(|p| p.x >= 0.5 - 1e-12), "{u:?}");
    }

    #[test]
    fn a_self_overlapping_ring_counts_everything_it_covers() {
        // a band folded over itself (winding 2 where it overlaps) is one region
        let pt = |x, y| Vec2::new(x, y);
        let ring = vec![
            pt(0.0, 0.0),
            pt(3.0, 0.0),
            pt(3.0, 2.0),
            pt(1.0, 2.0),
            pt(1.0, 1.0),
            pt(2.0, 1.0),
            pt(2.0, 3.0),
            pt(0.0, 3.0),
        ];
        let u = union_exterior(&[ring]).unwrap();
        // the 3 × 3 square less its top-right unit square; the overlap (winding 2) counts once
        assert!((signed_area(&u) - 8.0).abs() < 1e-12, "{u:?}");
    }
}
