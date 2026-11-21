//! Shared full-polygon clearance for roll compaction and finite hide/remnant stock.
//! Search can be approximate; acceptance checks every edge of the emitted paths.

use ftx_core::{
    Vec2,
    exact::{Sign, circle_diameter_margin, orient2d},
};

pub(crate) struct Work {
    pub used: usize,
    pub limit: usize,
    pub exhausted: bool,
    pub candidates: usize,
}
impl Work {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            used: 0,
            limit,
            exhausted: false,
            candidates: 0,
        }
    }
    pub(super) fn tick(&mut self) -> Option<()> {
        self.spend(1)
    }
    pub(super) fn spend(&mut self, count: usize) -> Option<()> {
        if count > self.limit - self.used {
            self.used = self.limit;
            self.exhausted = true;
            return None;
        }
        self.used += count;
        Some(())
    }
}
fn orient(a: Vec2, b: Vec2, c: Vec2) -> Sign {
    orient2d([a.x, a.y], [b.x, b.y], [c.x, c.y])
}
fn between(p: Vec2, a: Vec2, b: Vec2) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}
fn intersects(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let (u, v, w, z) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if (u == Sign::Zero && between(c, a, b))
        || (v == Sign::Zero && between(d, a, b))
        || (w == Sign::Zero && between(a, c, d))
        || (z == Sign::Zero && between(b, c, d))
    {
        return true;
    }
    u != Sign::Zero && v != Sign::Zero && w != Sign::Zero && z != Sign::Zero && u != v && w != z
}
fn point_distance(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let edge = b - a;
    let length2 = edge.norm2();
    if length2 == 0.0 {
        return (p - a).norm();
    }
    if !length2.is_finite() {
        return f64::NAN;
    }
    let t = ((p - a).dot(edge) / length2).clamp(0.0, 1.0);
    (p - (a + edge * t)).norm()
}
fn edges(p: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    p.iter()
        .copied()
        .zip(p.iter().copied().cycle().skip(1))
        .take(p.len())
}
fn inside(p: Vec2, ring: &[Vec2], work: &mut Work) -> Option<bool> {
    if ring.len() <= 32 {
        return inside_linear(p, ring, work);
    }
    let bvh = EdgeBvh::build(ring, work)?;
    inside_bvh(p, ring, &bvh, Vec2::ZERO, work)
}
fn inside_linear(p: Vec2, ring: &[Vec2], work: &mut Work) -> Option<bool> {
    let mut winding = 0_i32;
    for (a, b) in edges(ring) {
        work.tick()?;
        let side = orient(a, b, p);
        if side == Sign::Zero && between(p, a, b) {
            return Some(true);
        }
        if a.y <= p.y && b.y > p.y && side == Sign::Positive {
            winding += 1;
        }
        if a.y > p.y && b.y <= p.y && side == Sign::Negative {
            winding -= 1;
        }
    }
    Some(winding != 0)
}
fn edge_box(a: Vec2, b: Vec2) -> [f64; 4] {
    if !a.is_finite() || !b.is_finite() {
        return [
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::INFINITY,
        ];
    }
    [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)]
}
fn union_box(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}
fn shift_box(b: [f64; 4], offset: Vec2) -> [f64; 4] {
    if offset == Vec2::ZERO {
        return b;
    }
    if !offset.is_finite() || b.iter().any(|v| !v.is_finite()) {
        return [
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::INFINITY,
        ];
    }
    // Finite IEEE-754 addition is monotone, so shifting endpoints encloses every shifted vertex.
    [
        b[0] + offset.x,
        b[1] + offset.y,
        b[2] + offset.x,
        b[3] + offset.y,
    ]
}
/// Conservative edge-box broad-phase pruning for exact clearance/intersection queries.
/// Returns `true` only when every point pair across `a` and `b` is strictly farther than `gap`
/// (never pruning touching boxes at `gap == 0`, non-finite coordinates, or extreme overflow scales).
fn strictly_separated_boxes(a: [f64; 4], b: [f64; 4], gap: f64) -> bool {
    if !gap.is_finite() || gap < 0.0 || a.iter().chain(&b).any(|v| !v.is_finite()) {
        return false;
    }
    if gap == 0.0 {
        return a[2] < b[0] || b[2] < a[0] || a[3] < b[1] || b[3] < a[1];
    }
    let scale = a
        .iter()
        .chain(&b)
        .map(|v| v.abs())
        .fold(gap.abs().max(1.0), f64::max);
    if scale > 1e150 {
        return false;
    }
    let pad = gap + 64.0 * f64::EPSILON * scale;
    if !pad.is_finite() {
        return false;
    }
    a[2] + pad < b[0] || b[2] + pad < a[0] || a[3] + pad < b[1] || b[3] + pad < a[1]
}

#[derive(Clone, Debug)]
struct BvhNode {
    bounds: [f64; 4],
    start: u32,
    end: u32,
    left: u32,
    right: u32,
}
impl BvhNode {
    fn is_leaf(&self) -> bool {
        self.left == u32::MAX
    }
    fn count(&self) -> u32 {
        self.end - self.start
    }
}

/// Deterministic binary AABB hierarchy over contiguous closed-ring edges.
/// Used strictly for conservative broad-phase pruning; leaf checks run the exact edge predicates.
#[derive(Clone, Debug)]
pub(crate) struct EdgeBvh {
    nodes: Vec<BvhNode>,
}
impl EdgeBvh {
    pub(crate) fn build(ring: &[Vec2], work: &mut Work) -> Option<Self> {
        work.spend(ring.len().max(1))?;
        if ring.is_empty() {
            return Some(Self { nodes: Vec::new() });
        }
        let n = ring.len();
        // Single-edge leaves on small outer boundaries (e.g., 4-edge stock sheets) give razor-thin
        // wall boxes that prune interior pieces in O(1); larger curved outlines group 4 edges.
        let leaf_limit = if n <= 32 { 1 } else { (n / 1024).max(4) };
        let mut nodes = Vec::with_capacity((n / leaf_limit + 1) * 2);
        Self::build_range(ring, 0, n, leaf_limit, &mut nodes);
        Some(Self { nodes })
    }
    fn build_range(
        ring: &[Vec2],
        start: usize,
        end: usize,
        leaf_limit: usize,
        nodes: &mut Vec<BvhNode>,
    ) -> u32 {
        let index = nodes.len() as u32;
        if end - start <= leaf_limit {
            let mut bounds = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            let n = ring.len();
            for i in start..end {
                bounds = union_box(bounds, edge_box(ring[i], ring[(i + 1) % n]));
            }
            nodes.push(BvhNode {
                bounds,
                start: start as u32,
                end: end as u32,
                left: u32::MAX,
                right: u32::MAX,
            });
            return index;
        }
        nodes.push(BvhNode {
            bounds: [0.0; 4],
            start: start as u32,
            end: end as u32,
            left: u32::MAX,
            right: u32::MAX,
        });
        let mid = start + (end - start) / 2;
        let left = Self::build_range(ring, start, mid, leaf_limit, nodes);
        let right = Self::build_range(ring, mid, end, leaf_limit, nodes);
        let bounds = union_box(nodes[left as usize].bounds, nodes[right as usize].bounds);
        nodes[index as usize].bounds = bounds;
        nodes[index as usize].left = left;
        nodes[index as usize].right = right;
        index
    }
}

fn inside_bvh(
    p: Vec2,
    ring: &[Vec2],
    bvh: &EdgeBvh,
    ring_offset: Vec2,
    work: &mut Work,
) -> Option<bool> {
    if bvh.nodes.is_empty() {
        return Some(false);
    }
    let finite_p = p.is_finite();
    let root = shift_box(bvh.nodes[0].bounds, ring_offset);
    if finite_p
        && root.iter().all(|v| v.is_finite())
        && (p.x < root[0] || p.x > root[2] || p.y < root[1] || p.y > root[3])
    {
        work.tick()?;
        return Some(false);
    }
    let mut winding = 0_i32;
    let mut stack = Vec::with_capacity(32);
    stack.push(0_u32);
    let n = ring.len();
    while let Some(idx) = stack.pop() {
        work.tick()?;
        let node = &bvh.nodes[idx as usize];
        let b = shift_box(node.bounds, ring_offset);
        // An edge strictly above, below, or left of `p` cannot contain `p` or cross the +x ray.
        if finite_p && b.iter().all(|v| v.is_finite()) && (b[1] > p.y || b[3] < p.y || b[2] < p.x) {
            continue;
        }
        if node.is_leaf() {
            for i in node.start as usize..node.end as usize {
                work.tick()?;
                let a = ring[i];
                let b_pt = ring[(i + 1) % n];
                let side = orient(a, b_pt, p);
                if side == Sign::Zero && between(p, a, b_pt) {
                    return Some(true);
                }
                if a.y <= p.y && b_pt.y > p.y && side == Sign::Positive {
                    winding += 1;
                }
                if a.y > p.y && b_pt.y <= p.y && side == Sign::Negative {
                    winding -= 1;
                }
            }
        } else {
            stack.push(node.right);
            stack.push(node.left);
        }
    }
    Some(winding != 0)
}
fn separated_boxes(a: [f64; 4], b: [f64; 4], gap: f64) -> bool {
    a[2] + gap <= b[0] || b[2] + gap <= a[0] || a[3] + gap <= b[1] || b[3] + gap <= a[1]
}
fn edge_pair_clear(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2, gap: f64) -> bool {
    if intersects(a0, a1, b0, b1) {
        return false;
    }
    if gap > 0.0 {
        for d in [
            point_distance(a0, b0, b1),
            point_distance(a1, b0, b1),
            point_distance(b0, a0, a1),
            point_distance(b1, a0, a1),
        ] {
            if !d.is_finite() || d < gap {
                return false;
            }
        }
    }
    true
}
fn boundaries_clear_bvh(
    a: &[Vec2],
    a_bvh: &EdgeBvh,
    a_offset: Vec2,
    b: &[Vec2],
    b_bvh: &EdgeBvh,
    gap: f64,
    work: &mut Work,
) -> Option<bool> {
    if a_bvh.nodes.is_empty() || b_bvh.nodes.is_empty() {
        return Some(true);
    }
    let na = a.len();
    let nb = b.len();
    let mut stack = Vec::with_capacity(64);
    stack.push((0_u32, 0_u32));
    while let Some((ia, ib)) = stack.pop() {
        work.tick()?;
        let node_a = &a_bvh.nodes[ia as usize];
        let node_b = &b_bvh.nodes[ib as usize];
        if strictly_separated_boxes(shift_box(node_a.bounds, a_offset), node_b.bounds, gap) {
            continue;
        }
        match (node_a.is_leaf(), node_b.is_leaf()) {
            (true, true) => {
                for i in node_a.start as usize..node_a.end as usize {
                    let a0 = a[i];
                    let a1 = a[(i + 1) % na];
                    for j in node_b.start as usize..node_b.end as usize {
                        work.tick()?;
                        if !edge_pair_clear(a0, a1, b[j], b[(j + 1) % nb], gap) {
                            return Some(false);
                        }
                    }
                }
            }
            (false, true) => {
                stack.push((node_a.right, ib));
                stack.push((node_a.left, ib));
            }
            (true, false) => {
                stack.push((ia, node_b.right));
                stack.push((ia, node_b.left));
            }
            (false, false) => {
                if node_a.count() >= node_b.count() {
                    stack.push((node_a.right, ib));
                    stack.push((node_a.left, ib));
                } else {
                    stack.push((ia, node_b.right));
                    stack.push((ia, node_b.left));
                }
            }
        }
    }
    Some(true)
}
fn boundaries_clear(a: &[Vec2], b: &[Vec2], gap: f64, work: &mut Work) -> Option<bool> {
    if a.len().saturating_mul(b.len()) <= 64 {
        for (a0, a1) in edges(a) {
            for (b0, b1) in edges(b) {
                work.tick()?;
                if !edge_pair_clear(a0, a1, b0, b1, gap) {
                    return Some(false);
                }
            }
        }
        return Some(true);
    }
    let a_bvh = EdgeBvh::build(a, work)?;
    let b_bvh = EdgeBvh::build(b, work)?;
    boundaries_clear_bvh(a, &a_bvh, Vec2::ZERO, b, &b_bvh, gap, work)
}
// Preserve the roll kernel's conservative overlapping-box boundary-contact semantics, including gap=0.
pub(crate) fn clear(
    a: &[Vec2],
    ab: [f64; 4],
    b: &[Vec2],
    bb: [f64; 4],
    gap: f64,
    work: &mut Work,
) -> Option<bool> {
    work.tick()?;
    if separated_boxes(ab, bb, gap) {
        return Some(true);
    }
    clear_paths(a, b, gap, work)
}
// Prebuilt-BVH clearance for repeated translated candidates against fixed obstacles/placements.
#[allow(clippy::too_many_arguments)]
pub(crate) fn clear_bvh(
    a: &[Vec2],
    a_bvh: &EdgeBvh,
    a_offset: Vec2,
    ab: [f64; 4],
    b: &[Vec2],
    b_bvh: &EdgeBvh,
    bb: [f64; 4],
    gap: f64,
    work: &mut Work,
) -> Option<bool> {
    work.tick()?;
    if separated_boxes(ab, bb, gap) {
        return Some(true);
    }
    if !boundaries_clear_bvh(a, a_bvh, a_offset, b, b_bvh, gap, work)? {
        return Some(false);
    }
    Some(
        !inside_bvh(a[0], b, b_bvh, Vec2::ZERO, work)?
            && !inside_bvh(b[0], a, a_bvh, a_offset, work)?,
    )
}
// The same exact boundary/containment acceptance without a caller-supplied broad-phase box.
pub(crate) fn clear_paths(a: &[Vec2], b: &[Vec2], gap: f64, work: &mut Work) -> Option<bool> {
    if !boundaries_clear(a, b, gap, work)? {
        return Some(false);
    }
    Some(!inside(a[0], b, work)? && !inside(b[0], a, work)?)
}
// Certify strict separation without a rounded projected point or square root. Outward-rounded
// intervals enclose every operation on the authored f64 coordinates; an inconclusive comparison
// refuses clearance rather than adding an arbitrary physical tolerance. Endpoint distances use
// the core exact circle predicate. This stricter comparison belongs only to the tool sweep.
fn sweep_point_clear(p: Vec2, a: Vec2, b: Vec2, radius: f64) -> bool {
    type Bounds = [f64; 2];
    fn sub(a: Bounds, b: Bounds) -> Bounds {
        [(a[0] - b[1]).next_down(), (a[1] - b[0]).next_up()]
    }
    fn add(a: Bounds, b: Bounds) -> Bounds {
        [(a[0] + b[0]).next_down(), (a[1] + b[1]).next_up()]
    }
    fn mul(a: Bounds, b: Bounds) -> Bounds {
        let products = [a[0] * b[0], a[0] * b[1], a[1] * b[0], a[1] * b[1]];
        if products.iter().any(|v| !v.is_finite()) {
            return [f64::NEG_INFINITY, f64::INFINITY];
        }
        [
            products
                .into_iter()
                .fold(f64::INFINITY, f64::min)
                .next_down(),
            products
                .into_iter()
                .fold(f64::NEG_INFINITY, f64::max)
                .next_up(),
        ]
    }
    fn square(a: Bounds) -> Bounds {
        if a.iter().any(|v| !v.is_finite()) {
            return [0.0, f64::INFINITY];
        }
        let nearest = if a[0] > 0.0 {
            a[0]
        } else if a[1] < 0.0 {
            a[1]
        } else {
            0.0
        };
        let farthest = a[0].abs().max(a[1].abs());
        [
            (nearest * nearest).next_down().max(0.0),
            (farthest * farthest).next_up(),
        ]
    }
    let difference = |u: Vec2, v: Vec2| [sub([u.x; 2], [v.x; 2]), sub([u.y; 2], [v.y; 2])];
    let dot = |u: [Bounds; 2], v: [Bounds; 2]| add(mul(u[0], v[0]), mul(u[1], v[1]));
    let edge = difference(b, a);
    let delta = difference(p, a);
    let endpoint = if a == b || dot(delta, edge)[1] <= 0.0 {
        Some(a)
    } else if dot(difference(p, b), edge)[0] >= 0.0 {
        Some(b)
    } else {
        None
    };
    if let Some(endpoint) = endpoint {
        return circle_diameter_margin([p.x, p.y], [endpoint.x, endpoint.y], radius * 0.5)
            .is_some_and(|(sign, _)| sign == Sign::Negative);
    }
    // The infinite line also gives a conservative lower bound if the projection's interval
    // straddles an endpoint. Only a proved exterior projection uses an endpoint distance.
    let cross2 = square(sub(mul(delta[0], edge[1]), mul(delta[1], edge[0])));
    let length2 = add(square(edge[0]), square(edge[1]));
    let limit2 = mul(square([radius; 2]), length2);
    cross2[0].is_finite() && limit2[1].is_finite() && cross2[0] > limit2[1]
}
// Full circular tool sweep against a filled fixture, including stationary heads. Touching the
// envelope is not clearance. Checking endpoints alone would miss crossings between safe endpoints.
pub(super) fn swept_disk_clear(
    a: Vec2,
    b: Vec2,
    radius: f64,
    fixture: &[Vec2],
    work: &mut Work,
) -> Option<bool> {
    if inside(a, fixture, work)? || inside(b, fixture, work)? {
        return Some(false);
    }
    for (c, d) in edges(fixture) {
        work.tick()?;
        if intersects(a, b, c, d) {
            return Some(false);
        }
        for (p, u, v) in [(a, c, d), (b, c, d), (c, a, b), (d, a, b)] {
            if !sweep_point_clear(p, u, v, radius) {
                return Some(false);
            }
        }
    }
    Some(true)
}
// No edge may cross a concavity or touch the stock boundary. Vertices-inside alone is insufficient.
// Zero requested margin still forbids contact; search anchors include a numerical guard, not a clamp.
pub(crate) fn enclosed(
    inner: &[Vec2],
    outer: &[Vec2],
    margin: f64,
    work: &mut Work,
) -> Option<bool> {
    if !inside(inner[0], outer, work)? {
        return Some(false);
    }
    boundaries_clear(inner, outer, margin, work)
}
// Prebuilt-BVH enclosure check for repeated translated candidates against fixed stock/zone rings.
#[allow(clippy::too_many_arguments)]
pub(crate) fn enclosed_bvh(
    inner: &[Vec2],
    inner_bvh: &EdgeBvh,
    inner_offset: Vec2,
    outer: &[Vec2],
    outer_bvh: &EdgeBvh,
    margin: f64,
    work: &mut Work,
) -> Option<bool> {
    if !inside_bvh(inner[0], outer, outer_bvh, Vec2::ZERO, work)? {
        return Some(false);
    }
    boundaries_clear_bvh(inner, inner_bvh, inner_offset, outer, outer_bvh, margin, work)
}
// Budgeted simplicity check for authored stock. Consecutive collinear edges may continue, not retrace.
pub(crate) fn simple(ring: &[Vec2], work: &mut Work) -> Option<bool> {
    let n = ring.len();
    if n <= 16 {
        for (i, (a, b)) in edges(ring).enumerate() {
            work.tick()?;
            if a == b {
                return Some(false);
            }
            let c = ring[(i + 2) % n];
            if orient(a, b, c) == Sign::Zero && (b - a).dot(c - b) <= 0.0 {
                return Some(false);
            }
            for (j, (c, d)) in edges(ring).enumerate().skip(i + 1) {
                work.tick()?;
                if j == i + 1 || (i == 0 && j + 1 == n) {
                    continue;
                }
                if intersects(a, b, c, d) {
                    return Some(false);
                }
            }
        }
        return Some(true);
    }
    for (i, (a, b)) in edges(ring).enumerate() {
        work.tick()?;
        if a == b {
            return Some(false);
        }
        let c = ring[(i + 2) % n];
        if orient(a, b, c) == Sign::Zero && (b - a).dot(c - b) <= 0.0 {
            return Some(false);
        }
    }
    let bvh = EdgeBvh::build(ring, work)?;
    if bvh.nodes.is_empty() {
        return Some(true);
    }
    let mut stack = Vec::with_capacity(64);
    stack.push((0_u32, 0_u32));
    while let Some((ia, ib)) = stack.pop() {
        work.tick()?;
        let node_a = &bvh.nodes[ia as usize];
        let node_b = &bvh.nodes[ib as usize];
        if ia != ib && strictly_separated_boxes(node_a.bounds, node_b.bounds, 0.0) {
            continue;
        }
        if ia == ib {
            if node_a.is_leaf() {
                for i in node_a.start as usize..node_a.end as usize {
                    let a = ring[i];
                    let b = ring[(i + 1) % n];
                    for j in (i + 1)..node_a.end as usize {
                        work.tick()?;
                        if j == i + 1 || (i == 0 && j + 1 == n) {
                            continue;
                        }
                        if intersects(a, b, ring[j], ring[(j + 1) % n]) {
                            return Some(false);
                        }
                    }
                }
            } else {
                stack.push((node_a.right, node_a.right));
                stack.push((node_a.left, node_a.right));
                stack.push((node_a.left, node_a.left));
            }
            continue;
        }
        match (node_a.is_leaf(), node_b.is_leaf()) {
            (true, true) => {
                for i in node_a.start as usize..node_a.end as usize {
                    let a = ring[i];
                    let b = ring[(i + 1) % n];
                    for j in node_b.start as usize..node_b.end as usize {
                        work.tick()?;
                        let (lo, hi) = if i < j { (i, j) } else { (j, i) };
                        if hi == lo + 1 || (lo == 0 && hi + 1 == n) {
                            continue;
                        }
                        if intersects(a, b, ring[j], ring[(j + 1) % n]) {
                            return Some(false);
                        }
                    }
                }
            }
            (false, true) => {
                stack.push((node_a.right, ib));
                stack.push((node_a.left, ib));
            }
            (true, false) => {
                stack.push((ia, node_b.right));
                stack.push((ia, node_b.left));
            }
            (false, false) => {
                if node_a.count() >= node_b.count() {
                    stack.push((node_a.right, ib));
                    stack.push((node_a.left, ib));
                } else {
                    stack.push((ia, node_b.right));
                    stack.push((ia, node_b.left));
                }
            }
        }
    }
    Some(true)
}

pub(super) fn path_inside(
    path: &[Vec2],
    closed: bool,
    outer: &[Vec2],
    work: &mut Work,
) -> Option<bool> {
    if path.len() < 2 || path.iter().any(|p| !p.is_finite()) {
        return Some(false);
    }
    if !inside(path[0], outer, work)? {
        return Some(false);
    }
    let n = if closed { path.len() } else { path.len() - 1 };
    for (a, b) in edges(path).take(n) {
        for (c, d) in edges(outer) {
            work.tick()?;
            if intersects(a, b, c, d) {
                return Some(false);
            }
        }
    }
    Some(true)
}

pub(super) fn disk_inside(p: Vec2, radius: f64, outer: &[Vec2], work: &mut Work) -> Option<bool> {
    if !p.is_finite() || !radius.is_finite() || radius <= 0.0 || !inside(p, outer, work)? {
        return Some(false);
    }
    for (a, b) in edges(outer) {
        work.tick()?;
        let d = point_distance(p, a, b);
        if !d.is_finite() || d <= radius {
            return Some(false);
        }
    }
    Some(true)
}

// Qualified drilled holes use the same conservative strict distance authority as tool sweeps,
// without replacing the established stock-nesting acceptance or adding a geometric tolerance.
pub(super) fn strict_disk_inside(
    p: Vec2,
    radius: f64,
    outer: &[Vec2],
    work: &mut Work,
) -> Option<bool> {
    if !p.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
        || outer.len() < 3
        || !inside(p, outer, work)?
    {
        return Some(false);
    }
    for (a, b) in edges(outer) {
        work.tick()?;
        if !sweep_point_clear(p, a, b, radius) {
            return Some(false);
        }
    }
    Some(true)
}

// A closed cut removes its entire interior, whereas an open slit excludes only its actual edges.
pub(super) fn disk_clear_path(
    p: Vec2,
    radius: f64,
    path: &[Vec2],
    closed: bool,
    work: &mut Work,
) -> Option<bool> {
    if !p.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
        || path.len() < if closed { 3 } else { 2 }
        || (closed && inside(p, path, work)?)
    {
        return Some(false);
    }
    for (a, b) in edges(path).take(if closed { path.len() } else { path.len() - 1 }) {
        work.tick()?;
        if !sweep_point_clear(p, a, b, radius) {
            return Some(false);
        }
    }
    Some(true)
}

// Both holes use the same fixed bit, so the exact circle predicate avoids rounded radius sums.
pub(super) fn equal_disks_clear(a: Vec2, b: Vec2, radius: f64, work: &mut Work) -> Option<bool> {
    work.tick()?;
    Some(
        circle_diameter_margin([a.x, a.y], [b.x, b.y], radius)
            .is_some_and(|(sign, _)| sign == Sign::Negative),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn poly(p: &[[f64; 2]]) -> Vec<Vec2> {
        p.iter().map(|p| Vec2::new(p[0], p[1])).collect()
    }
    #[test]
    fn qualified_holes_refuse_oblique_contact_voids_and_tangent_holes() {
        let p = Vec2::new(23., 39.);
        let mut boundary = poly(&[[0., 0.], [42., 56.], [-20., 80.], [-50., 20.]]);
        // The first oblique edge is exactly 5 mm away, despite rounded projected distances.
        for _ in 0..2 {
            assert_eq!(
                strict_disk_inside(p, 5., &boundary, &mut Work::new(100)),
                Some(false)
            );
            assert_eq!(
                strict_disk_inside(p, 4.9999, &boundary, &mut Work::new(100)),
                Some(true)
            );
            boundary.reverse();
        }
        let slit = poly(&[[0., 0.], [42., 56.]]);
        assert_eq!(
            disk_clear_path(p, 5., &slit, false, &mut Work::new(100)),
            Some(false)
        );
        assert_eq!(
            disk_clear_path(p, 4.9999, &slit, false, &mut Work::new(100)),
            Some(true)
        );
        let void = poly(&[[10., 20.], [40., 20.], [40., 60.], [10., 60.]]);
        assert_eq!(
            disk_clear_path(p, 1., &void, true, &mut Work::new(100)),
            Some(false)
        );
        assert_eq!(
            equal_disks_clear(Vec2::ZERO, Vec2::new(3., 4.), 2.5, &mut Work::new(10)),
            Some(false)
        );
        assert_eq!(
            equal_disks_clear(Vec2::ZERO, Vec2::new(3., 4.), 2.4999, &mut Work::new(10)),
            Some(true)
        );
        assert_eq!(strict_disk_inside(p, 1., &void, &mut Work::new(1)), None);
    }
    #[test]
    fn swept_disk_refuses_exact_oblique_tangency_after_scaling_and_translation() {
        // |(42,56)| = 70 and cross((42,56),(23,39)) = 350: the closest corner is
        // exactly 5 mm from the segment, with an interior projection. Rounded projection
        // previously returned 5.000000000000003 and incorrectly admitted this fixture.
        let fixture = poly(&[[22., 39.], [23., 39.], [23., 40.], [22., 40.]]);
        for (scale, offset) in [
            (1.0 / 1024.0, Vec2::new(0., 0.)),
            (1.0, Vec2::new(1.0 / 1024.0, -1.0 / 2048.0)),
            (1.0, Vec2::new(524288., -524288.)),
            (1024.0, Vec2::new(-262144., 262144.)),
        ] {
            let transform = |p: Vec2| p * scale + offset;
            let obstacle: Vec<_> = fixture.iter().copied().map(transform).collect();
            let a = transform(Vec2::new(0., 0.));
            let b = transform(Vec2::new(42., 56.));
            for (start, end) in [(a, b), (b, a)] {
                for (radius, clear) in [
                    (5.0, false),
                    (5.0 - 1.0 / 16777216.0, true),
                    (5.0 + 1.0 / 16777216.0, false),
                ] {
                    assert_eq!(
                        swept_disk_clear(
                            start,
                            end,
                            radius * scale,
                            &obstacle,
                            &mut Work::new(1000)
                        ),
                        Some(clear),
                        "scale={scale}, offset={offset:?}, radius={radius}"
                    );
                }
            }
        }
    }
    #[test]
    fn swept_disk_endpoint_and_stationary_clearance_use_the_finite_segment() {
        for (a, b, fixture) in [
            (
                Vec2::new(0., 0.),
                Vec2::new(0., 0.),
                poly(&[[3., 4.], [4., 4.], [4., 5.], [3., 5.]]),
            ),
            (
                Vec2::new(0., 0.),
                Vec2::new(4., 0.),
                poly(&[[7., 4.], [8., 4.], [8., 5.], [7., 5.]]),
            ),
        ] {
            // The 3-4-5 corner is nearest the stationary head or the segment's end.
            // In the latter case the infinite line is only 4 mm away and is insufficient.
            for (start, end) in [(a, b), (b, a)] {
                assert_eq!(
                    swept_disk_clear(start, end, 5.0, &fixture, &mut Work::new(1000)),
                    Some(false)
                );
                assert_eq!(
                    swept_disk_clear(
                        start,
                        end,
                        5.0 - 1.0 / 16777216.0,
                        &fixture,
                        &mut Work::new(1000)
                    ),
                    Some(true)
                );
            }
        }
    }
    #[test]
    fn concave_stock_does_not_admit_edges_that_leave_stock_between_inside_vertices() {
        let outer = poly(&[
            [0., 0.],
            [100., 0.],
            [100., 100.],
            [70., 100.],
            [70., 40.],
            [30., 40.],
            [30., 100.],
            [0., 100.],
        ]);
        let inner = poly(&[[10., 60.], [90., 60.], [90., 80.], [10., 80.]]);
        let mut w = Work::new(10000);
        for p in &inner {
            assert_eq!(inside(*p, &outer, &mut w), Some(true));
        }
        assert_eq!(enclosed(&inner, &outer, 0., &mut w), Some(false));
        assert_eq!(path_inside(&inner[..2], false, &outer, &mut w), Some(false));
        assert_eq!(enclosed(&inner, &outer, 0., &mut Work::new(1)), None);
    }
    #[test]
    fn stock_simple_and_containment_checks_preserve_euclidean_margin_and_defect_discs() {
        let tri = poly(&[[0., 0.], [10., 0.], [0., 10.]]);
        let inner = poly(&[[1., 1.], [4., 1.], [4., 4.], [1., 4.]]);
        let mut w = Work::new(10000);
        assert_eq!(simple(&tri, &mut w), Some(true));
        assert_eq!(enclosed(&inner, &tri, 0.99, &mut w), Some(true));
        assert_eq!(enclosed(&inner, &tri, 1.01, &mut w), Some(false));
        assert_eq!(
            disk_inside(Vec2::new(4., 4.), 1.42, &tri, &mut w),
            Some(false)
        );
        assert_eq!(
            disk_inside(Vec2::new(4., 4.), 1.40, &tri, &mut w),
            Some(true)
        );
        let retraced = poly(&[[0., 0.], [10., 0.], [5., 0.], [10., 10.], [0., 10.]]);
        assert_eq!(simple(&retraced, &mut w), Some(false));
    }
    #[test]
    fn edge_bvh_matches_brute_force_on_dense_curved_concave_and_touching_polygons() {
        fn brute_boundaries_clear(a: &[Vec2], b: &[Vec2], gap: f64) -> bool {
            for (a0, a1) in edges(a) {
                for (b0, b1) in edges(b) {
                    if !edge_pair_clear(a0, a1, b0, b1, gap) {
                        return false;
                    }
                }
            }
            true
        }
        fn brute_simple(ring: &[Vec2]) -> bool {
            let n = ring.len();
            for (i, (a, b)) in edges(ring).enumerate() {
                if a == b {
                    return false;
                }
                let c = ring[(i + 2) % n];
                if orient(a, b, c) == Sign::Zero && (b - a).dot(c - b) <= 0.0 {
                    return false;
                }
                for (j, (c, d)) in edges(ring).enumerate().skip(i + 1) {
                    if j == i + 1 || (i == 0 && j + 1 == n) {
                        continue;
                    }
                    if intersects(a, b, c, d) {
                        return false;
                    }
                }
            }
            true
        }
        // Build two dense 80-vertex wavy/concave rings approximating drafted garment outlines.
        let wavy = |cx: f64, cy: f64, rx: f64, ry: f64, amp: f64, phase: f64, n: usize| -> Vec<Vec2> {
            (0..n)
                .map(|i| {
                    let t = std::f64::consts::TAU * (i as f64) / (n as f64);
                    let r = 1.0 + amp * (3.0 * t + phase).sin();
                    Vec2::new(cx + rx * r * t.cos(), cy + ry * r * t.sin())
                })
                .collect()
        };
        let a = wavy(100.0, 100.0, 45.0, 35.0, 0.22, 0.0, 80);
        let b_base = wavy(0.0, 0.0, 30.0, 40.0, 0.18, 0.7, 72);
        let sheet = poly(&[[0.0, 0.0], [300.0, 0.0], [300.0, 260.0], [0.0, 260.0]]);
        let mut w_build = Work::new(100_000);
        let a_bvh = EdgeBvh::build(&a, &mut w_build).unwrap();
        let b_bvh = EdgeBvh::build(&b_base, &mut w_build).unwrap();
        let sheet_bvh = EdgeBvh::build(&sheet, &mut w_build).unwrap();

        // Compare BVH vs brute-force across separated, grazing, touching, intersecting, and nested shifts.
        for dx in [40.0, 100.0, 155.0, 175.0, 190.0, 220.0] {
            for dy in [40.0, 100.0, 150.0, 180.0] {
                let offset = Vec2::new(dx, dy);
                let b_shifted: Vec<_> = b_base.iter().map(|p| *p + offset).collect();
                for gap in [0.0, 0.5, 4.0, 12.0] {
                    let expected = brute_boundaries_clear(&b_shifted, &a, gap);
                    let mut w1 = Work::new(1_000_000);
                    let got_direct = boundaries_clear(&b_shifted, &a, gap, &mut w1).unwrap();
                    let mut w2 = Work::new(1_000_000);
                    let got_bvh =
                        boundaries_clear_bvh(&b_shifted, &b_bvh, offset, &a, &a_bvh, gap, &mut w2)
                            .unwrap();
                    assert_eq!(got_direct, expected, "direct mismatch at ({dx},{dy}) gap={gap}");
                    assert_eq!(got_bvh, expected, "bvh mismatch at ({dx},{dy}) gap={gap}");
                    assert!(
                        w2.used < a.len() * b_base.len() / 2,
                        "expected BVH pruning to beat brute force: used={} vs {}",
                        w2.used,
                        a.len() * b_base.len()
                    );
                }
                for margin in [0.0, 10.0, 25.0] {
                    let mut wb = Work::new(1_000_000);
                    let exp_inside = inside_linear(b_shifted[0], &sheet, &mut wb).unwrap();
                    let exp_encl = exp_inside && brute_boundaries_clear(&b_shifted, &sheet, margin);
                    let mut w3 = Work::new(1_000_000);
                    let got_encl = enclosed_bvh(
                        &b_shifted,
                        &b_bvh,
                        offset,
                        &sheet,
                        &sheet_bvh,
                        margin,
                        &mut w3,
                    )
                    .unwrap();
                    assert_eq!(got_encl, exp_encl, "enclosed mismatch at ({dx},{dy}) m={margin}");
                }
            }
        }

        // Compare inside_bvh vs inside_linear across a dense grid of points on concave ring `a`.
        for ix in 0..15 {
            for iy in 0..15 {
                let p = Vec2::new(40.0 + ix as f64 * 8.5, 50.0 + iy as f64 * 7.5);
                let mut wl = Work::new(10_000);
                let mut wb = Work::new(10_000);
                let exp = inside_linear(p, &a, &mut wl).unwrap();
                let got = inside_bvh(p, &a, &a_bvh, Vec2::ZERO, &mut wb).unwrap();
                assert_eq!(got, exp, "inside mismatch at {p:?}");
            }
        }
        // Vertex-on-boundary and exact touching cases at gap = 0 must NEVER prune falsely.
        let touch_box = poly(&[
            [a[0].x, a[0].y - 10.0],
            [a[0].x + 20.0, a[0].y - 10.0],
            [a[0].x + 20.0, a[0].y],
            [a[0].x, a[0].y],
        ]);
        assert_eq!(
            boundaries_clear(&a, &touch_box, 0.0, &mut Work::new(100_000)),
            Some(false)
        );

        // Simplicity equivalence: valid dense curve, self-crossing pinched curve, and touching vertex.
        assert_eq!(brute_simple(&a), true);
        let mut ws = Work::new(100_000);
        assert_eq!(simple(&a, &mut ws), Some(true));
        assert!(ws.used < a.len() * a.len() / 2);

        let mut pinched = a.clone();
        pinched[40] = pinched[0] + Vec2::new(5.0, 0.0);
        assert_eq!(brute_simple(&pinched), false);
        assert_eq!(simple(&pinched, &mut Work::new(100_000)), Some(false));

        let mut vertex_touch = a.clone();
        vertex_touch[40] = vertex_touch[10];
        assert_eq!(brute_simple(&vertex_touch), false);
        assert_eq!(simple(&vertex_touch, &mut Work::new(100_000)), Some(false));
    }
}
