//! GEOS 3.14.1's buffer from the raw curves on (`BufferBuilder::buffer` after `BufferCurveSetBuilder`): the
//! curves noded (`MCIndexNoder` with `IntersectionAdder`; `NodedSegmentString`, `SegmentNodeList` with its
//! collapse nodes, `SegmentNode` ordered by `SegmentPointComparator`), split into edges, coincident edges
//! merged with their depth deltas summed (`insertUniqueEdge`), the planar graph (`PlanarGraph`, `NodeMap`
//! in coordinate order, `DirectedEdgeStar` in `EdgeEnd::compareDirection` order), the subgraphs
//! (`BufferSubgraph`: depth first from the lowest node, in descending order of their rightmost x), the
//! result edges (depth ≥ 1 on the right, ≤ 0 on the left) and the polygons (`PolygonBuilder`: maximal
//! rings from the linked result edges, minimal rings where a ring meets itself, shells and holes by
//! `Orientation::isCCW`, free holes in the smallest shell containing them). Depths are GEOS's: the
//! subgraph's rightmost edge (`RightmostEdgeFinder`) takes the depth outside it (`SubgraphDepthLocater`
//! over the subgraphs already built), and the depths spread around each node's star
//! (`DirectedEdgeStar::computeDepths`). The result's rings, their starts and their order are GEOS's. A
//! topology GEOS refuses (its `TopologyException`) is an error here. For GEOS's reduced-precision retry the
//! same graph is built from the curves snap-rounded instead (`ScaledNoder` around `SnapRoundingNoder`:
//! `HotPixel`, `HotPixelIndex`, `SnapRoundingIntersectionAdder`).

use std::cmp::Ordering;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, HashMap};

use ftx_core::Vec2;

use crate::buffer::{dist, orientation, point_to_segment, ring_signed_area, segment_intersections};

/// A polygon: its shell and holes, each closed (first point repeated).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polygon {
    /// The exterior ring.
    pub shell: Vec<Vec2>,
    /// The interior rings.
    pub holes: Vec<Vec<Vec2>>,
}

impl Polygon {
    /// A polygon without holes.
    #[must_use]
    pub fn new(shell: Vec<Vec2>) -> Self {
        Self {
            shell,
            holes: Vec::new(),
        }
    }

    /// GEOS `Polygon::getArea`: the shell's area less the holes'.
    #[must_use]
    pub fn area(&self) -> f64 {
        self.holes
            .iter()
            .fold(ring_signed_area(&self.shell).abs(), |a, h| {
                a - ring_signed_area(h).abs()
            })
    }
}

/// A raw curve (closed) and its depth delta: the depth on its left less the depth on its right.
pub(crate) struct Curve {
    pub pts: Vec<Vec2>,
    pub delta: i32,
}

fn coord_cmp(a: Vec2, b: Vec2) -> Ordering {
    // `Coordinate::compareTo` / `CoordinateLessThan`: x, then y
    a.x.partial_cmp(&b.x)
        .unwrap_or(Ordering::Equal)
        .then(a.y.partial_cmp(&b.y).unwrap_or(Ordering::Equal))
}

fn key(p: Vec2) -> (u64, u64) {
    ((p.x + 0.0).to_bits(), (p.y + 0.0).to_bits()) // −0.0 and 0.0 are one coordinate
}

// ============================================================================================ noding

#[derive(Clone, Copy, Debug)]
struct SegNode {
    pt: Vec2,
    seg: usize,
    octant: i32,
    interior: bool,
}

/// `Octant::octant` (GEOS's `safeOctant`: a zero-length segment is octant 0).
fn octant(p0: Vec2, p1: Vec2) -> i32 {
    let (dx, dy) = (p1.x - p0.x, p1.y - p0.y);
    if dx == 0.0 && dy == 0.0 {
        return 0;
    }
    let (adx, ady) = (dx.abs(), dy.abs());
    match (dx >= 0.0, dy >= 0.0, adx >= ady) {
        (true, true, true) => 0,
        (true, true, false) => 1,
        (true, false, true) => 7,
        (true, false, false) => 6,
        (false, true, true) => 3,
        (false, true, false) => 2,
        (false, false, true) => 4,
        (false, false, false) => 5,
    }
}

/// `SegmentPointComparator::compare`: the order of two points along a segment of this octant.
fn compare_along(octant: i32, p0: Vec2, p1: Vec2) -> Ordering {
    if p0 == p1 {
        return Ordering::Equal;
    }
    let sign = |a: f64, b: f64| i32::from(a > b) - i32::from(a < b);
    let (xs, ys) = (sign(p0.x, p1.x), sign(p0.y, p1.y));
    let (s0, s1) = match octant {
        0 => (xs, ys),
        1 => (ys, xs),
        2 => (ys, -xs),
        3 => (-xs, ys),
        4 => (-xs, -ys),
        5 => (-ys, -xs),
        6 => (-ys, xs),
        _ => (xs, -ys),
    };
    s0.cmp(&0).then(s1.cmp(&0))
}

/// `SegmentNode::compareTo`.
fn compare_nodes(a: &SegNode, b: &SegNode) -> Ordering {
    a.seg.cmp(&b.seg).then_with(|| {
        if a.pt == b.pt {
            Ordering::Equal
        } else if !a.interior {
            Ordering::Less // a segment's start point sorts first
        } else if !b.interior {
            Ordering::Greater
        } else {
            compare_along(a.octant, a.pt, b.pt)
        }
    })
}

/// A curve's `SegmentNodeList`.
struct NodeList<'a> {
    pts: &'a [Vec2],
    nodes: Vec<SegNode>,
}

impl<'a> NodeList<'a> {
    fn new(pts: &'a [Vec2]) -> Self {
        Self {
            pts,
            nodes: Vec::new(),
        }
    }

    fn add(&mut self, pt: Vec2, seg: usize) {
        let octant = if seg + 1 < self.pts.len() {
            octant(self.pts[seg], self.pts[seg + 1])
        } else {
            -1
        };
        self.nodes.push(SegNode {
            pt,
            seg,
            octant,
            interior: pt != self.pts[seg],
        });
    }

    /// `NodedSegmentString::addIntersection`: a point on a segment's end vertex belongs to the next one.
    fn add_intersection(&mut self, pt: Vec2, seg: usize) {
        let seg = if seg + 1 < self.pts.len() && pt == self.pts[seg + 1] {
            seg + 1
        } else {
            seg
        };
        self.add(pt, seg);
    }

    fn prepare(&mut self) {
        self.nodes.sort_by(compare_nodes);
        self.nodes
            .dedup_by(|b, a| compare_nodes(a, b) == Ordering::Equal);
    }

    /// `NodedSegmentString::getNodedCoordinates` (`SegmentNodeList::getSplitCoordinates`): the string with its
    /// nodes in, endpoints added but not the collapses, repeated points dropped.
    fn split_coordinates(mut self) -> Vec<Vec2> {
        let pts = self.pts;
        let last = pts.len() - 1;
        self.add(pts[0], 0);
        self.add(pts[last], last);
        self.prepare();
        let mut out = vec![self.nodes[0].pt];
        for w in self.nodes.windows(2) {
            for p in split_edge(pts, w[0], w[1]) {
                if out.last() != Some(&p) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// `SegmentNodeList::addSplitEdges`: the endpoints and the collapses (`A B A`) become nodes too.
    fn split_edges(mut self) -> Vec<Vec<Vec2>> {
        let pts = self.pts;
        let last = pts.len() - 1;
        self.add(pts[0], 0);
        self.add(pts[last], last);
        self.prepare();
        let mut collapsed = Vec::new();
        for w in self.nodes.windows(2) {
            let (e0, e1) = (w[0], w[1]);
            if e0.pt == e1.pt {
                let between = (e1.seg - e0.seg).saturating_sub(usize::from(!e1.interior));
                if between == 1 {
                    collapsed.push(e0.seg + 1);
                }
            }
        }
        for i in 0..pts.len().saturating_sub(2) {
            if pts[i] == pts[i + 2] {
                collapsed.push(i + 1);
            }
        }
        for v in collapsed {
            self.add(pts[v], v);
        }
        self.prepare();
        self.nodes
            .windows(2)
            .map(|w| split_edge(pts, w[0], w[1]))
            .collect()
    }
}

/// `SegmentNodeList::createSplitEdgePts`: the edge between two consecutive nodes.
fn split_edge(pts: &[Vec2], e0: SegNode, e1: SegNode) -> Vec<Vec2> {
    if e0.seg == e1.seg {
        return vec![e0.pt, e1.pt];
    }
    let use_end = e1.interior || e1.pt != pts[e1.seg];
    let mut out = vec![e0.pt];
    out.extend_from_slice(&pts[e0.seg + 1..=e1.seg]);
    if use_end {
        out.push(e1.pt);
    }
    out
}

/// Every pair of segments (string, segment) whose envelopes come within `tol` of each other, as the noder
/// meets it: the earlier string, or the earlier segment, first.
fn segment_pairs(
    strings: &[&[Vec2]],
    tol: f64,
    mut visit: impl FnMut((usize, usize), (usize, usize)),
) {
    let mut segs: Vec<(usize, usize, [f64; 4])> = Vec::new();
    for (c, pts) in strings.iter().enumerate() {
        for (i, w) in pts.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            segs.push((
                c,
                i,
                [a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y)],
            ));
        }
    }
    segs.sort_by(|a, b| a.2[0].total_cmp(&b.2[0]).then((a.0, a.1).cmp(&(b.0, b.1))));
    for k in 0..segs.len() {
        for l in k + 1..segs.len() {
            let (s, t) = (segs[k], segs[l]);
            if t.2[0] > s.2[1] + tol {
                break;
            }
            if t.2[3] < s.2[2] - tol || t.2[2] > s.2[3] + tol {
                continue;
            }
            if (s.0, s.1) < (t.0, t.1) {
                visit((s.0, s.1), (t.0, t.1));
            } else {
                visit((t.0, t.1), (s.0, s.1));
            }
        }
    }
}

/// `MCIndexNoder` + `IntersectionAdder` over every pair of segments whose envelopes meet; the curves' split
/// edges.
fn node(curves: &[Curve]) -> Vec<Vec<Vec<Vec2>>> {
    let strings: Vec<&[Vec2]> = curves.iter().map(|c| c.pts.as_slice()).collect();
    let mut lists: Vec<NodeList> = strings.iter().map(|s| NodeList::new(s)).collect();
    segment_pairs(&strings, 0.0, |a, b| {
        let (pa, pb) = (strings[a.0], strings[b.0]);
        let (n, x) = segment_intersections(pa[a.1], pa[a.1 + 1], pb[b.1], pb[b.1 + 1]);
        // adjacent segments of one curve share a vertex: not a node when it is all they share
        if n == 0 || (a.0 == b.0 && n == 1 && a.1.abs_diff(b.1) == 1) {
            return;
        }
        for &p in &x[..usize::from(n)] {
            lists[a.0].add_intersection(p, a.1);
            lists[b.0].add_intersection(p, b.1);
        }
    });
    lists.into_iter().map(NodeList::split_edges).collect()
}

// ===================================================================================== snap-rounding

/// GEOS `util::round` (`java_math_round`): to the nearest integer, halves up.
pub(crate) fn java_round(v: f64) -> f64 {
    let n = v.trunc();
    let f = (v - n).abs();
    if v >= 0.0 {
        if f < 0.5 {
            v.floor()
        } else if f > 0.5 {
            v.ceil()
        } else {
            n + 1.0
        }
    } else if f < 0.5 {
        v.ceil()
    } else if f > 0.5 {
        v.floor()
    } else {
        n
    }
}

fn round_pt(p: Vec2) -> Vec2 {
    Vec2::new(java_round(p.x), java_round(p.y))
}

/// `HotPixelIndex` at scale 1: one pixel per integer point, a node once two points or an intersection
/// fall in it. The pixels a query visits do not depend on GEOS's shuffled KdTree, nor does the result.
#[derive(Default)]
struct Pixels {
    at: BTreeMap<(i64, i64), usize>,
    pts: Vec<Vec2>,
    node: Vec<bool>,
}

impl Pixels {
    /// `HotPixelIndex::add`: a second point in a pixel makes it a node.
    fn add(&mut self, p: Vec2) -> usize {
        let r = round_pt(p);
        match self.at.entry((r.x as i64, r.y as i64)) {
            Entry::Occupied(e) => {
                self.node[*e.get()] = true;
                *e.get()
            }
            Entry::Vacant(e) => {
                e.insert(self.pts.len());
                self.pts.push(r);
                self.node.push(false);
                self.pts.len() - 1
            }
        }
    }

    /// `HotPixelIndex::query`: the pixels within a pixel of the segment's envelope.
    fn near(&self, p0: Vec2, p1: Vec2) -> Vec<usize> {
        let lo = |a: f64, b: f64| (a.min(b) - 1.0).floor() as i64;
        let hi = |a: f64, b: f64| (a.max(b) + 1.0).ceil() as i64;
        let (y0, y1) = (lo(p0.y, p1.y), hi(p0.y, p1.y));
        self.at
            .range((lo(p0.x, p1.x), i64::MIN)..=(hi(p0.x, p1.x), i64::MAX))
            .filter(|((_, y), _)| (y0..=y1).contains(y))
            .map(|(_, &h)| h)
            .collect()
    }
}

/// `HotPixel::intersects(p)`: the pixel around `h`, its top and right sides open.
fn pixel_has(h: Vec2, p: Vec2) -> bool {
    !(p.x >= h.x + 0.5 || p.x < h.x - 0.5 || p.y >= h.y + 0.5 || p.y < h.y - 0.5)
}

/// `HotPixel::intersectsScaled`: whether the segment meets the pixel around `h`.
fn pixel_meets(h: Vec2, p0: Vec2, p1: Vec2) -> bool {
    // oriented in positive x
    let (p, q) = if p0.x > p1.x { (p1, p0) } else { (p0, p1) };
    let (maxx, minx, maxy, miny) = (h.x + 0.5, h.x - 0.5, h.y + 0.5, h.y - 0.5);
    if p.x.min(q.x) >= maxx || p.x.max(q.x) < minx || p.y.min(q.y) >= maxy || p.y.max(q.y) < miny {
        return false;
    }
    // vertical or horizontal segments meet the interior or the left or bottom side
    if p.x == q.x || p.y == q.y {
        return true;
    }
    let ul = orientation(p, q, Vec2::new(minx, maxy));
    if ul == 0 {
        return p.y >= q.y; // an upward segment through the corner misses the interior
    }
    let ur = orientation(p, q, Vec2::new(maxx, maxy));
    if ur == 0 {
        return p.y <= q.y;
    }
    if ul != ur {
        return true; // crosses the top side
    }
    let ll = orientation(p, q, Vec2::new(minx, miny));
    if ll == 0 || ll != ul {
        return true; // the lower left corner, or across the left side
    }
    let lr = orientation(p, q, Vec2::new(maxx, miny));
    if lr == 0 {
        return p.y >= q.y;
    }
    ll != lr || lr != ur // across the bottom or the right side
}

/// `SnapRoundingIntersectionAdder::isNearSegmentInterior`.
fn near_interior(p: Vec2, p0: Vec2, p1: Vec2, tol: f64) -> bool {
    dist(p, p0) >= tol && dist(p, p1) >= tol && point_to_segment(p, p0, p1) < tol
}

/// `BufferOp::bufferFixedPrecision`'s noder: `ScaledNoder` (the curves onto the grid of `scale`, repeated
/// points dropped, and back after) around a `SnapRoundingNoder` at scale 1, which puts hot pixels on the
/// intersections (`SnapRoundingIntersectionAdder`, near vertices within a hundredth of a pixel too) and the
/// vertices, rounds every curve and nodes it at the pixels it crosses. The surviving curves' depth deltas
/// and split edges.
fn snap_round(curves: &[Curve], scale: f64) -> Vec<(i32, Vec<Vec<Vec2>>)> {
    let scaled = scale != 1.0;
    let strings: Vec<Vec<Vec2>> = curves
        .iter()
        .map(|c| {
            if !scaled {
                return c.pts.clone();
            }
            let mut pts: Vec<Vec2> = c
                .pts
                .iter()
                .map(|p| Vec2::new(java_round(p.x * scale), java_round(p.y * scale)))
                .collect();
            pts.dedup();
            pts
        })
        .collect();
    // addIntersectionPixels: MCIndexNoder at the nearness tolerance (a pixel over INTERSECTION_NEARNESS_FACTOR)
    let tol = 1.0 / 100.0;
    let refs: Vec<&[Vec2]> = strings.iter().map(Vec::as_slice).collect();
    let mut lists: Vec<NodeList> = refs.iter().map(|s| NodeList::new(s)).collect();
    let mut nodes = Vec::new();
    segment_pairs(&refs, tol, |a, b| {
        let (sa, sb) = (refs[a.0], refs[b.0]);
        let (a0, a1, b0, b1) = (sa[a.1], sa[a.1 + 1], sb[b.1], sb[b.1 + 1]);
        let (n, x) = segment_intersections(a0, a1, b0, b1);
        let x = &x[..usize::from(n)];
        let interior = |s: Vec2, e: Vec2| x.iter().any(|&p| p != s && p != e);
        if n > 0 && (interior(a0, a1) || interior(b0, b1)) {
            for &p in x {
                nodes.push(p);
                lists[a.0].add_intersection(p, a.1);
                lists[b.0].add_intersection(p, b.1);
            }
            return;
        }
        // a vertex very near the other segment's interior counts as an intersection
        for (p, s0, s1, (c, i)) in [
            (a0, b0, b1, b),
            (a1, b0, b1, b),
            (b0, a0, a1, a),
            (b1, a0, a1, a),
        ] {
            if near_interior(p, s0, s1, tol) {
                nodes.push(p);
                lists[c].add_intersection(p, i);
            }
        }
    });
    let mut pixels = Pixels::default();
    for &p in &nodes {
        let h = pixels.add(p);
        pixels.node[h] = true;
    }
    for s in &strings {
        for &p in s {
            pixels.add(p);
        }
    }
    // computeSnaps: each curve with its intersections, rounded, noded at the pixels its segments cross
    let noded: Vec<Vec<Vec2>> = lists.into_iter().map(NodeList::split_coordinates).collect();
    let rounded: Vec<Vec<Vec2>> = noded
        .iter()
        .map(|pts| {
            let mut r: Vec<Vec2> = pts.iter().map(|&p| round_pt(p)).collect();
            r.dedup();
            r
        })
        .collect();
    let mut snapped: Vec<(i32, NodeList)> = Vec::new();
    for ((pts, round), curve) in noded.iter().zip(&rounded).zip(curves) {
        if round.len() <= 1 {
            continue; // collapsed to a point
        }
        let mut list = NodeList::new(round);
        let mut seg = 0;
        for w in pts.windows(2) {
            if round_pt(w[1]) == round[seg] {
                continue; // the segment collapsed
            }
            for h in pixels.near(w[0], w[1]) {
                let hp = pixels.pts[h];
                // a pixel that only holds one of the segment's own vertices is not a node (yet)
                if !pixels.node[h] && (pixel_has(hp, w[0]) || pixel_has(hp, w[1])) {
                    continue;
                }
                if pixel_meets(hp, w[0], w[1]) {
                    list.add_intersection(hp, seg);
                    pixels.node[h] = true;
                }
            }
            seg += 1;
        }
        snapped.push((curve.delta, list));
    }
    // addVertexNodeSnaps: the vertices in pixels that became nodes
    for (_, list) in &mut snapped {
        let pts = list.pts;
        for (i, &p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
            if let Some(&h) = pixels.at.get(&(p.x as i64, p.y as i64))
                && pixels.node[h]
                && pixels.pts[h] == p
            {
                list.add_intersection(p, i);
            }
        }
    }
    snapped
        .into_iter()
        .map(|(delta, list)| {
            let mut edges = list.split_edges();
            if scaled {
                for p in edges.iter_mut().flatten() {
                    *p = Vec2::new(p.x / scale + 0.0, p.y / scale + 0.0);
                }
            }
            (delta, edges)
        })
        .collect()
}

// ============================================================================================= graph

const NONE: usize = usize::MAX;

struct Edge {
    pts: Vec<Vec2>,
    delta: i32,
}

/// A directed edge: `2 e` runs along edge `e`, `2 e + 1` against it.
struct De {
    node: usize,
    p0: Vec2,
    p1: Vec2,
    quadrant: i32,
    /// Depth on the left and on the right (`UNSET` until assigned).
    left: i32,
    right: i32,
    visited: bool,
    in_result: bool,
    ring: usize,
    min_ring: usize,
    next: usize,
    next_min: usize,
}

/// GEOS's unassigned depth.
const UNSET: i32 = -999;

/// Whether directed edge `d` runs along its edge.
fn forward(d: usize) -> bool {
    d & 1 == 0
}

struct Node {
    pt: Vec2,
    star: Vec<usize>,
    result_area: Vec<usize>,
}

struct Ring {
    pts: Vec<Vec2>,
    start: usize,
    hole: bool,
    holes: Vec<usize>,
    shell: usize,
}

/// `Quadrant::quadrant`: NE 0, NW 1, SW 2, SE 3.
fn quadrant(dx: f64, dy: f64) -> i32 {
    match (dx >= 0.0, dy >= 0.0) {
        (true, true) => 0,
        (false, true) => 1,
        (false, false) => 2,
        (true, false) => 3,
    }
}

/// `EdgeEnd::compareDirection`: counter-clockwise from the +x axis.
fn compare_direction(a: &De, b: &De) -> Ordering {
    let (da, db) = (a.p1 - a.p0, b.p1 - b.p0);
    if da.x == db.x && da.y == db.y {
        return Ordering::Equal;
    }
    a.quadrant
        .cmp(&b.quadrant)
        .then_with(|| orientation(b.p0, b.p1, a.p1).cmp(&0))
}

/// GEOS `Orientation::isCCW` (the ring walk, not the area).
fn is_ccw(ring: &[Vec2]) -> bool {
    let n = ring.len().saturating_sub(1);
    if n < 3 {
        return false;
    }
    let mut up_hi = ring[0];
    let mut up_lo = Vec2::new(f64::NAN, f64::NAN);
    let mut prev_y = up_hi.y;
    let mut i_up_hi = 0;
    for (i, p) in ring.iter().enumerate().take(n + 1).skip(1) {
        if p.y > prev_y && p.y >= up_hi.y {
            i_up_hi = i;
            up_hi = *p;
            up_lo = ring[i - 1];
        }
        prev_y = p.y;
    }
    if i_up_hi == 0 {
        return false;
    }
    let mut i_down_lo = i_up_hi;
    loop {
        i_down_lo = (i_down_lo + 1) % n;
        if i_down_lo == i_up_hi || ring[i_down_lo].y != up_hi.y {
            break;
        }
    }
    let down_lo = ring[i_down_lo];
    let down_hi = ring[if i_down_lo > 0 { i_down_lo - 1 } else { n - 1 }];
    if up_hi == down_hi {
        if up_lo == up_hi || down_lo == up_hi || up_lo == down_lo {
            return false;
        }
        orientation(up_lo, up_hi, down_lo) == 1
    } else {
        down_hi.x - up_hi.x < 0.0
    }
}

/// `EdgeRing::addPoints`.
fn add_points(pts: &mut Vec<Vec2>, edge: &[Vec2], forward: bool, first: bool) {
    let skip = usize::from(!first);
    if forward {
        pts.extend(edge.iter().skip(skip));
    } else {
        pts.extend(edge.iter().rev().skip(skip));
    }
}

struct Graph {
    edges: Vec<Edge>,
    des: Vec<De>,
    nodes: Vec<Node>,
    rings: Vec<Ring>,
}

impl Graph {
    fn new(edges: Vec<Edge>) -> Self {
        let mut des = Vec::with_capacity(2 * edges.len());
        let mut nodes: Vec<Node> = Vec::new();
        let mut at: HashMap<(u64, u64), usize> = HashMap::new();
        for e in &edges {
            let n = e.pts.len();
            for (p0, p1) in [(e.pts[0], e.pts[1]), (e.pts[n - 1], e.pts[n - 2])] {
                let node = *at.entry(key(p0)).or_insert_with(|| {
                    nodes.push(Node {
                        pt: p0,
                        star: Vec::new(),
                        result_area: Vec::new(),
                    });
                    nodes.len() - 1
                });
                let d = des.len();
                des.push(De {
                    node,
                    p0,
                    p1,
                    quadrant: quadrant(p1.x - p0.x, p1.y - p0.y),
                    left: UNSET,
                    right: UNSET,
                    visited: false,
                    in_result: false,
                    ring: NONE,
                    min_ring: NONE,
                    next: NONE,
                    next_min: NONE,
                });
                // the star is a set ordered by direction: an end in an existing direction is not added
                let star = &nodes[node].star;
                if let Err(pos) = star.binary_search_by(|&o| compare_direction(&des[o], &des[d])) {
                    nodes[node].star.insert(pos, d);
                }
            }
        }
        Self {
            edges,
            des,
            nodes,
            rings: Vec::new(),
        }
    }

    /// `DirectedEdgeStar::linkResultDirectedEdges`.
    fn link_result(&mut self, node: usize) -> Result<(), String> {
        let area: Vec<usize> = self.nodes[node]
            .star
            .iter()
            .copied()
            .filter(|&d| self.des[d].in_result || self.des[d ^ 1].in_result)
            .collect();
        let (mut first_out, mut incoming) = (NONE, NONE);
        for &out in &area {
            let inc = out ^ 1;
            if first_out == NONE && self.des[out].in_result {
                first_out = out;
            }
            if incoming == NONE {
                if self.des[inc].in_result {
                    incoming = inc;
                }
            } else if self.des[out].in_result {
                self.des[incoming].next = out;
                incoming = NONE;
            }
        }
        if incoming != NONE {
            if first_out == NONE {
                let p = self.nodes[node].pt;
                return Err(format!("buffer: no outgoing edge at ({}, {})", p.x, p.y));
            }
            self.des[incoming].next = first_out;
        }
        self.nodes[node].result_area = area;
        Ok(())
    }

    /// `DirectedEdgeStar::linkMinimalDirectedEdges` (clockwise: the result edges in reverse).
    fn link_minimal(&mut self, node: usize, ring: usize) {
        let (mut first_out, mut incoming) = (NONE, NONE);
        for &out in self.nodes[node].result_area.iter().rev() {
            let inc = out ^ 1;
            if first_out == NONE && self.des[out].ring == ring {
                first_out = out;
            }
            if incoming == NONE {
                if self.des[inc].ring == ring {
                    incoming = inc;
                }
            } else if self.des[out].ring == ring {
                self.des[incoming].next_min = out;
                incoming = NONE;
            }
        }
        if incoming != NONE {
            self.des[incoming].next_min = first_out;
        }
    }

    /// `EdgeRing::computePoints` and `computeRing` along `next` (maximal) or `next_min` (minimal) links.
    fn ring(&mut self, start: usize, minimal: bool) -> Result<usize, String> {
        let id = self.rings.len();
        let mut pts = Vec::new();
        let mut d = start;
        let mut first = true;
        loop {
            let mark = if minimal {
                &mut self.des[d].min_ring
            } else {
                &mut self.des[d].ring
            };
            if *mark == id {
                return Err("buffer: a directed edge visited twice while building a ring".into());
            }
            *mark = id;
            add_points(&mut pts, &self.edges[d / 2].pts, forward(d), first);
            first = false;
            d = if minimal {
                self.des[d].next_min
            } else {
                self.des[d].next
            };
            if d == NONE {
                return Err("buffer: a ring with a missing link".into());
            }
            if d == start {
                break;
            }
        }
        let hole = is_ccw(&pts);
        self.rings.push(Ring {
            pts,
            start,
            hole,
            holes: Vec::new(),
            shell: NONE,
        });
        Ok(id)
    }

    /// The directed edges of maximal ring `r`, from its start.
    fn ring_edges(&self, r: usize) -> Vec<usize> {
        let start = self.rings[r].start;
        let mut out = vec![start];
        let mut d = self.des[start].next;
        while d != start {
            out.push(d);
            d = self.des[d].next;
        }
        out
    }

    /// `EdgeRing::getMaxNodeDegree`: twice the most edges of the ring leaving one node.
    fn max_node_degree(&self, r: usize) -> usize {
        self.ring_edges(r)
            .iter()
            .map(|&d| {
                self.nodes[self.des[d].node]
                    .star
                    .iter()
                    .filter(|&&o| self.des[o].ring == r)
                    .count()
            })
            .max()
            .unwrap_or(0)
            * 2
    }

    fn set_shell(&mut self, hole: usize, shell: usize) {
        self.rings[hole].shell = shell;
        self.rings[shell].holes.push(hole);
    }

    /// `DirectedEdge::setDepth`: an assigned depth never changes.
    fn set_depth(&mut self, d: usize, left: bool, v: i32) -> Result<(), String> {
        let slot = if left {
            &mut self.des[d].left
        } else {
            &mut self.des[d].right
        };
        if *slot != UNSET && *slot != v {
            let p = self.des[d].p0;
            return Err(format!(
                "buffer: assigned depths do not match at ({}, {})",
                p.x, p.y
            ));
        }
        *slot = v;
        Ok(())
    }

    /// `DirectedEdge::setEdgeDepths`: one side's depth, the other's from the edge's depth delta.
    fn set_edge_depths(&mut self, d: usize, left: bool, v: i32) -> Result<(), String> {
        let delta = self.edges[d / 2].delta * if forward(d) { 1 } else { -1 };
        self.set_depth(d, left, v)?;
        self.set_depth(d, !left, v + if left { -delta } else { delta })
    }

    /// `BufferSubgraph::copySymDepths`.
    fn copy_sym(&mut self, d: usize) -> Result<(), String> {
        let (left, right) = (self.des[d].left, self.des[d].right);
        self.set_depth(d ^ 1, true, right)?;
        self.set_depth(d ^ 1, false, left)
    }

    /// `DirectedEdgeStar::computeDepths(de)`: the depths counter-clockwise round the node from `de`.
    fn star_depths(&mut self, node: usize, de: usize) -> Result<(), String> {
        let star = self.nodes[node].star.clone();
        let p = self.nodes[node].pt;
        let pos = star
            .iter()
            .position(|&o| compare_direction(&self.des[o], &self.des[de]) == Ordering::Equal)
            .ok_or_else(|| {
                format!(
                    "buffer: an edge missing from its node at ({}, {})",
                    p.x, p.y
                )
            })?;
        let target = self.des[de].right;
        let mut depth = self.des[de].left;
        for &d in star[pos + 1..].iter().chain(&star[..pos]) {
            self.set_edge_depths(d, false, depth)?;
            depth = self.des[d].left;
        }
        if depth != target {
            return Err(format!("buffer: depth mismatch at ({}, {})", p.x, p.y));
        }
        Ok(())
    }

    /// `BufferSubgraph::computeNodeDepth`.
    fn node_depth(&mut self, node: usize) -> Result<(), String> {
        let star = self.nodes[node].star.clone();
        let Some(start) = star
            .iter()
            .copied()
            .find(|&d| self.des[d].visited || self.des[d ^ 1].visited)
        else {
            let p = self.nodes[node].pt;
            return Err(format!(
                "buffer: unable to find an edge to compute depths at ({}, {})",
                p.x, p.y
            ));
        };
        self.star_depths(node, start)?;
        for d in star {
            self.des[d].visited = true;
            self.copy_sym(d)?;
        }
        Ok(())
    }

    /// `BufferSubgraph::computeDepth`: the outside depth on the oriented rightmost edge's right, spread
    /// breadth first over the subgraph's nodes.
    fn subgraph_depths(
        &mut self,
        dir_edges: &[usize],
        start: usize,
        outside: i32,
    ) -> Result<(), String> {
        for &d in dir_edges {
            self.des[d].visited = false;
        }
        self.set_edge_depths(start, false, outside)?;
        self.copy_sym(start)?;
        let mut seen = vec![false; self.nodes.len()];
        let first = self.des[start].node;
        let mut queue = std::collections::VecDeque::from([first]);
        seen[first] = true;
        self.des[start].visited = true;
        while let Some(n) = queue.pop_front() {
            self.node_depth(n)?;
            for d in self.nodes[n].star.clone() {
                if self.des[d ^ 1].visited {
                    continue;
                }
                let adj = self.des[d ^ 1].node;
                if !seen[adj] {
                    seen[adj] = true;
                    queue.push_back(adj);
                }
            }
        }
        Ok(())
    }
}

const LEFT: i32 = 1;
const RIGHT: i32 = 2;

/// `DirectedEdgeStar::getRightmostEdge`.
fn star_rightmost(g: &Graph, node: usize) -> Option<usize> {
    let star = &g.nodes[node].star;
    let (&d0, &dn) = (star.first()?, star.last()?);
    if star.len() == 1 {
        return Some(d0);
    }
    let north = |d: usize| g.des[d].quadrant <= 1;
    let dy = |d: usize| g.des[d].p1.y - g.des[d].p0.y;
    match (north(d0), north(dn)) {
        (true, true) => Some(d0),
        (false, false) => Some(dn),
        _ if dy(d0) != 0.0 => Some(d0),
        _ if dy(dn) != 0.0 => Some(dn),
        _ => None,
    }
}

/// `RightmostEdgeFinder::findEdge`: the subgraph's rightmost coordinate, and the edge there oriented to
/// have the outside on its right.
fn rightmost(g: &Graph, dir_edges: &[usize]) -> Result<(usize, Vec2), String> {
    let check = |d: usize, found: &mut Option<(usize, usize, Vec2)>| {
        let pts = &g.edges[d / 2].pts;
        for (i, &p) in pts.iter().enumerate().take(pts.len() - 1) {
            if found.is_none_or(|f| p.x > f.2.x) {
                *found = Some((d, i, p));
            }
        }
    };
    let mut found = None;
    for &d in dir_edges {
        if forward(d) {
            check(d, &mut found);
        }
    }
    let (mut de, mut index, mut coord) =
        found.ok_or_else(|| "buffer: no forward edges in a subgraph".to_string())?;
    if index == 0 {
        de = star_rightmost(g, g.des[de].node)
            .ok_or_else(|| "buffer: no rightmost edge at a node".to_string())?;
        if !forward(de) {
            de ^= 1;
            index = g.edges[de / 2].pts.len() - 1;
        }
    } else {
        let pts = &g.edges[de / 2].pts;
        let (prev, next) = (pts[index - 1], pts[index + 1]);
        let o = orientation(coord, next, prev);
        if (prev.y < coord.y && next.y < coord.y && o == 1)
            || (prev.y > coord.y && next.y > coord.y && o == -1)
        {
            index -= 1;
        }
    }
    let side_of =
        |i: Option<usize>| -> i32 {
            let pts = &g.edges[de / 2].pts;
            match i {
                Some(i) if i + 1 < pts.len() && pts[i].y != pts[i + 1].y => {
                    if pts[i].y < pts[i + 1].y { RIGHT } else { LEFT }
                }
                _ => -1,
            }
        };
    let mut side = side_of(Some(index));
    if side < 0 {
        side = side_of(index.checked_sub(1));
    }
    if side < 0 {
        // GEOS rescans the edge for its rightmost coordinate
        let mut again = None;
        check(de, &mut again);
        if let Some(f) = again {
            coord = f.2;
        }
    }
    Ok((if side == LEFT { de ^ 1 } else { de }, coord))
}

/// `SubgraphDepthLocater::getDepth`: the depth at `p` from the subgraphs already built (the left depth
/// of the nearest segment a ray to the right of `p` meets, `DepthSegment` order).
fn outside_depth(g: &Graph, built: &[&[usize]], p: Vec2) -> i32 {
    let mut segs: Vec<(Vec2, Vec2, i32)> = Vec::new();
    for dir_edges in built {
        let env = dir_edges.iter().fold(
            [
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ],
            |e, &d| {
                let pts = &g.edges[d / 2].pts;
                let f = envelope(&pts[..pts.len() - 1]);
                [
                    e[0].min(f[0]),
                    e[1].max(f[1]),
                    e[2].min(f[2]),
                    e[3].max(f[3]),
                ]
            },
        );
        if p.y < env[2] || p.y > env[3] || p.x > env[1] {
            continue;
        }
        for &d in dir_edges.iter().filter(|&&d| forward(d)) {
            let pts = &g.edges[d / 2].pts;
            let env = envelope(pts);
            if p.y < env[2] || p.y > env[3] || p.x > env[1] {
                continue;
            }
            for w in pts.windows(2) {
                let (low, high, swap) = if w[0].y > w[1].y {
                    (w[1], w[0], true)
                } else {
                    (w[0], w[1], false)
                };
                if low.x.max(high.x) < p.x
                    || low.y == high.y
                    || p.y < low.y
                    || p.y > high.y
                    || orientation(low, high, p) == -1
                {
                    continue;
                }
                segs.push((low, high, if swap { g.des[d].right } else { g.des[d].left }));
            }
        }
    }
    let mut best: Option<usize> = None;
    for i in 0..segs.len() {
        if best.is_none_or(|b| depth_segment_cmp(&segs[i], &segs[b]) == Ordering::Less) {
            best = Some(i);
        }
    }
    best.map_or(0, |b| segs[b].2)
}

/// `DepthSegment::compareTo` on upward segments.
fn depth_segment_cmp(a: &(Vec2, Vec2, i32), b: &(Vec2, Vec2, i32)) -> Ordering {
    let ext = |s: &(Vec2, Vec2, i32)| {
        (
            s.0.x.min(s.1.x),
            s.0.x.max(s.1.x),
            s.0.y.min(s.1.y),
            s.0.y.max(s.1.y),
        )
    };
    let (ea, eb) = (ext(a), ext(b));
    if ea.0 > eb.1 {
        return Ordering::Greater;
    }
    if ea.1 < eb.0 {
        return Ordering::Less;
    }
    if ea.2 > eb.3 {
        return Ordering::Greater;
    }
    if ea.3 < eb.2 {
        return Ordering::Less;
    }
    let in_extent = |p: Vec2, s: &(Vec2, Vec2, i32), e: (f64, f64, f64, f64)| -> i32 {
        if p.y >= e.2 && p.y <= e.3 {
            return -orientation(s.0, s.1, p); // left −1, right 1
        }
        0
    };
    for c in [
        in_extent(a.0, b, eb),
        in_extent(a.1, b, eb),
        -in_extent(b.0, a, ea),
        -in_extent(b.1, a, ea),
    ] {
        if c != 0 {
            return c.cmp(&0);
        }
    }
    if ea.3 != eb.3 {
        return ea.3.partial_cmp(&eb.3).unwrap_or(Ordering::Equal);
    }
    if a.0.y == a.1.y && b.0.y == b.1.y && ea.0 != eb.0 {
        return ea.0.partial_cmp(&eb.0).unwrap_or(Ordering::Equal);
    }
    Ordering::Equal
}

fn envelope(pts: &[Vec2]) -> [f64; 4] {
    pts.iter().fold(
        [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ],
        |e, p| [e[0].min(p.x), e[1].max(p.x), e[2].min(p.y), e[3].max(p.y)],
    )
}

fn env_covers(a: [f64; 4], b: [f64; 4]) -> bool {
    b[0] >= a[0] && b[1] <= a[1] && b[2] >= a[2] && b[3] <= a[3]
}

/// `RayCrossingCounter`: inside or on the ring (`true`), outside (`false`).
fn in_or_on_ring(p: Vec2, ring: &[Vec2]) -> bool {
    let mut crossings = 0;
    for w in ring.windows(2) {
        let (p1, p2) = (w[0], w[1]);
        if p1.x < p.x && p2.x < p.x {
            continue;
        }
        if p == p2 {
            return true;
        }
        if p1.y == p.y && p2.y == p.y {
            if p.x >= p1.x.min(p2.x) && p.x <= p1.x.max(p2.x) {
                return true;
            }
            continue;
        }
        if (p1.y > p.y && p2.y <= p.y) || (p2.y > p.y && p1.y <= p.y) {
            let mut o = orientation(p1, p2, p);
            if o == 0 {
                return true;
            }
            if p2.y < p1.y {
                o = -o;
            }
            if o == 1 {
                crossings += 1;
            }
        }
    }
    crossings % 2 == 1
}

/// `PolygonBuilder::findEdgeRingContaining`: the smallest shell around the hole.
fn containing_shell(g: &Graph, hole: usize, shells: &[usize]) -> usize {
    let test = &g.rings[hole].pts;
    let test_env = envelope(test);
    let (mut best, mut best_env) = (NONE, [0.0; 4]);
    for &s in shells {
        let ring = &g.rings[s].pts;
        let env = envelope(ring);
        if env == test_env || !env_covers(env, test_env) {
            continue;
        }
        let Some(&p) = test.iter().find(|p| !ring.contains(p)) else {
            continue;
        };
        if in_or_on_ring(p, ring) && (best == NONE || env_covers(best_env, env)) {
            (best, best_env) = (s, env);
        }
    }
    best
}

/// The polygons GEOS's `BufferBuilder` builds from these raw curves, in its order.
pub(crate) fn polygons(curves: &[Curve]) -> Result<Vec<Polygon>, String> {
    build(curves.iter().map(|c| c.delta).zip(node(curves)).collect())
}

/// The same with the curves snap-rounded onto the grid of `scale` (`BufferOp::bufferFixedPrecision`).
pub(crate) fn polygons_snap_rounded(curves: &[Curve], scale: f64) -> Result<Vec<Polygon>, String> {
    build(snap_round(curves, scale))
}

/// `BufferBuilder::buffer` from the noded curves (depth delta, split edges) on.
fn build(noded: Vec<(i32, Vec<Vec<Vec2>>)>) -> Result<Vec<Polygon>, String> {
    // noded edges, coincident ones merged (`computeNodedEdges`, `insertUniqueEdge`)
    let mut edges: Vec<Edge> = Vec::new();
    let mut seen: HashMap<Vec<(u64, u64)>, usize> = HashMap::new();
    for (delta, split) in noded {
        for mut pts in split {
            pts.dedup();
            if pts.len() < 2 {
                continue;
            }
            // `OrientedCoordinateArray`: an edge and its reverse are one edge
            let n = pts.len();
            let forward = (0..n / 2)
                .map(|i| coord_cmp(pts[i], pts[n - 1 - i]))
                .find(|o| *o != Ordering::Equal)
                .is_none_or(|o| o == Ordering::Less);
            let canon: Vec<(u64, u64)> = if forward {
                pts.iter().map(|&p| key(p)).collect()
            } else {
                pts.iter().rev().map(|&p| key(p)).collect()
            };
            match seen.get(&canon) {
                Some(&k) => {
                    let same = edges[k].pts == pts;
                    edges[k].delta += if same { delta } else { -delta };
                }
                None => {
                    seen.insert(canon, edges.len());
                    edges.push(Edge { pts, delta });
                }
            }
        }
    }
    if edges.is_empty() {
        return Ok(Vec::new());
    }
    let mut g = Graph::new(edges);
    // subgraphs: depth first from each unvisited node in coordinate order
    let mut order: Vec<usize> = (0..g.nodes.len()).collect();
    order.sort_by(|&a, &b| coord_cmp(g.nodes[a].pt, g.nodes[b].pt));
    let mut visited = vec![false; g.nodes.len()];
    let mut subgraphs: Vec<(usize, Vec2, Vec<usize>, Vec<usize>)> = Vec::new();
    for &start in &order {
        if visited[start] {
            continue;
        }
        let (mut nodes, mut dir_edges, mut stack) = (Vec::new(), Vec::new(), vec![start]);
        while let Some(n) = stack.pop() {
            visited[n] = true;
            nodes.push(n);
            for &d in &g.nodes[n].star {
                dir_edges.push(d);
                let other = g.des[d ^ 1].node;
                if !visited[other] {
                    stack.push(other);
                }
            }
        }
        let (oriented, right) = rightmost(&g, &dir_edges)?;
        subgraphs.push((oriented, right, nodes, dir_edges));
    }
    // shells before the holes they contain: `std::sort` with `BufferSubgraphGT` (rightmost x strictly
    // greater; −0.0 ties 0.0), so ties land where libc++'s introsort puts them
    let mut order: Vec<usize> = (0..subgraphs.len()).collect();
    crate::libcxx::sort(&mut order, |&a, &b| subgraphs[a].1.x > subgraphs[b].1.x);
    let mut slots: Vec<_> = subgraphs.into_iter().map(Some).collect();
    let subgraphs: Vec<(usize, Vec2, Vec<usize>, Vec<usize>)> =
        order.iter().filter_map(|&k| slots[k].take()).collect();
    let mut shells: Vec<usize> = Vec::new();
    let mut built: Vec<&[usize]> = Vec::new();
    for (oriented, right, nodes, dir_edges) in &subgraphs {
        let outside = outside_depth(&g, &built, *right);
        g.subgraph_depths(dir_edges, *oriented, outside)?;
        // result edges: interior on the right, exterior on the left
        for &d in dir_edges {
            if g.des[d].right >= 1 && g.des[d].left <= 0 {
                g.des[d].in_result = true;
            }
        }
        built.push(dir_edges);
        for &n in nodes {
            g.link_result(n)?;
        }
        let mut maximal = Vec::new();
        for &d in dir_edges {
            if g.des[d].in_result && g.des[d].ring == NONE {
                maximal.push(g.ring(d, false)?);
            }
        }
        let (mut free, mut plain) = (Vec::new(), Vec::new());
        for &r in &maximal {
            if g.max_node_degree(r) > 2 {
                let ring_edges = g.ring_edges(r);
                for &d in &ring_edges {
                    g.link_minimal(g.des[d].node, r);
                }
                let mut minimal = Vec::new();
                for &d in &ring_edges {
                    if g.des[d].min_ring == NONE {
                        minimal.push(g.ring(d, true)?);
                    }
                }
                let shell: Vec<usize> = minimal
                    .iter()
                    .copied()
                    .filter(|&m| !g.rings[m].hole)
                    .collect();
                match shell[..] {
                    [] => free.extend(minimal),
                    [s] => {
                        for &m in &minimal {
                            if g.rings[m].hole {
                                g.set_shell(m, s);
                            }
                        }
                        shells.push(s);
                    }
                    _ => return Err("buffer: found two shells in one minimal ring set".into()),
                }
            } else {
                plain.push(r);
            }
        }
        for r in plain {
            if g.rings[r].hole {
                free.push(r);
            } else {
                shells.push(r);
            }
        }
        for h in free {
            if g.rings[h].shell == NONE {
                let s = containing_shell(&g, h, &shells);
                if s != NONE {
                    g.set_shell(h, s);
                }
            }
        }
    }
    Ok(shells
        .iter()
        .map(|&s| Polygon {
            shell: g.rings[s].pts.clone(),
            holes: g.rings[s]
                .holes
                .iter()
                .map(|&h| g.rings[h].pts.clone())
                .collect(),
        })
        .collect())
}
