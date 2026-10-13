//! A bounding-volume hierarchy over axis-aligned boxes, for the self-contact broad phase: every pair of
//! overlapping boxes between two sets, or within one set, in sorted order.
//!
//! Provenance: lifted from frankensim `crates/fs-query/src/features.rs` (`Bvh`, `build_node`,
//! `ccd_candidates`) at commit `c991e5f90288a92dd03ee817556ad67527e1e238` (same author, MIT +
//! OpenAI/Anthropic rider). Changes: boxes are `(lo, hi)` pairs the caller has already padded (no outward
//! rounding, so the pairs are exactly those the overlap test accepts and the result equals the sweep it
//! replaces); the within-one-set query visits each unordered pair of leaves once; no pair cap and no
//! cancellation. The tree's shape never reaches the result: every overlapping pair is reported once and
//! the output is sorted.

use ftx_core::Vec3;

/// Features per leaf.
const LEAF_SIZE: usize = 8;

type Aabb = (Vec3, Vec3);

#[derive(Clone, Copy, Debug)]
enum Kind {
    Leaf { start: usize, len: usize },
    Internal { left: usize, right: usize },
}

#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Aabb,
    kind: Kind,
}

/// The hierarchy: nodes, with each leaf owning a contiguous range of `order`.
#[derive(Clone, Debug)]
pub(crate) struct Bvh<'a> {
    boxes: &'a [Aabb],
    order: Vec<usize>,
    nodes: Vec<Node>,
}

/// Closed boxes overlap (touching counts).
pub(crate) fn overlaps(a: Aabb, b: Aabb) -> bool {
    !(a.1.x < b.0.x
        || a.0.x > b.1.x
        || a.1.y < b.0.y
        || a.0.y > b.1.y
        || a.1.z < b.0.z
        || a.0.z > b.1.z)
}

fn merged(a: Aabb, b: Aabb) -> Aabb {
    (
        Vec3::new(a.0.x.min(b.0.x), a.0.y.min(b.0.y), a.0.z.min(b.0.z)),
        Vec3::new(a.1.x.max(b.1.x), a.1.y.max(b.1.y), a.1.z.max(b.1.z)),
    )
}

fn widest_axis(b: Aabb) -> usize {
    let span = b.1 - b.0;
    let spans = [span.x, span.y, span.z];
    let mut k = 0;
    if spans[1] > spans[k] {
        k = 1;
    }
    if spans[2] > spans[k] {
        k = 2;
    }
    k
}

impl<'a> Bvh<'a> {
    /// Builds the hierarchy by median splits on the widest axis of each node's bounds (ties broken by
    /// index, so the permutation is a pure function of the boxes).
    pub(crate) fn build(boxes: &'a [Aabb]) -> Self {
        let mut order: Vec<usize> = (0..boxes.len()).collect();
        let mut nodes = Vec::with_capacity(boxes.len() / LEAF_SIZE * 2 + 1);
        let centres: Vec<[f64; 3]> = boxes
            .iter()
            .map(|(lo, hi)| {
                [
                    f64::midpoint(lo.x, hi.x),
                    f64::midpoint(lo.y, hi.y),
                    f64::midpoint(lo.z, hi.z),
                ]
            })
            .collect();
        if !order.is_empty() {
            build_node(boxes, &centres, &mut order, 0, &mut nodes);
        }
        Self {
            boxes,
            order,
            nodes,
        }
    }

    fn leaf(&self, start: usize, len: usize) -> &[usize] {
        &self.order[start..start + len]
    }

    /// Every `(i, j)` with `self`'s box `i` overlapping `other`'s box `j`, sorted.
    pub(crate) fn pairs(&self, other: &Bvh) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        if self.nodes.is_empty() || other.nodes.is_empty() {
            return out;
        }
        let mut stack = vec![(0, 0)];
        while let Some((na, nb)) = stack.pop() {
            let (a, b) = (&self.nodes[na], &other.nodes[nb]);
            if !overlaps(a.bounds, b.bounds) {
                continue;
            }
            match (a.kind, b.kind) {
                (Kind::Leaf { start: sa, len: la }, Kind::Leaf { start: sb, len: lb }) => {
                    for &i in self.leaf(sa, la) {
                        for &j in other.leaf(sb, lb) {
                            if overlaps(self.boxes[i], other.boxes[j]) {
                                out.push((i, j));
                            }
                        }
                    }
                }
                (Kind::Internal { left, right }, _) => {
                    stack.push((left, nb));
                    stack.push((right, nb));
                }
                (Kind::Leaf { .. }, Kind::Internal { left, right }) => {
                    stack.push((na, left));
                    stack.push((na, right));
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// Every `(i, j)` with `i < j` whose boxes overlap, sorted.
    pub(crate) fn self_pairs(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        if self.nodes.is_empty() {
            return out;
        }
        let mut stack = vec![(0, 0)];
        while let Some((na, nb)) = stack.pop() {
            let (a, b) = (&self.nodes[na], &self.nodes[nb]);
            if na == nb {
                match a.kind {
                    Kind::Leaf { start, len } => {
                        let ids = self.leaf(start, len);
                        for (k, &i) in ids.iter().enumerate() {
                            for &j in &ids[k + 1..] {
                                if overlaps(self.boxes[i], self.boxes[j]) {
                                    out.push((i.min(j), i.max(j)));
                                }
                            }
                        }
                    }
                    Kind::Internal { left, right } => {
                        stack.push((left, left));
                        stack.push((right, right));
                        stack.push((left, right));
                    }
                }
                continue;
            }
            if !overlaps(a.bounds, b.bounds) {
                continue;
            }
            match (a.kind, b.kind) {
                (Kind::Leaf { start: sa, len: la }, Kind::Leaf { start: sb, len: lb }) => {
                    for &i in self.leaf(sa, la) {
                        for &j in self.leaf(sb, lb) {
                            if overlaps(self.boxes[i], self.boxes[j]) {
                                out.push((i.min(j), i.max(j)));
                            }
                        }
                    }
                }
                (Kind::Internal { left, right }, _) => {
                    stack.push((left, nb));
                    stack.push((right, nb));
                }
                (Kind::Leaf { .. }, Kind::Internal { left, right }) => {
                    stack.push((na, left));
                    stack.push((na, right));
                }
            }
        }
        out.sort_unstable();
        out
    }
}

fn build_node(
    boxes: &[Aabb],
    centres: &[[f64; 3]],
    order: &mut [usize],
    base: usize,
    nodes: &mut Vec<Node>,
) -> usize {
    let bounds = order[1..]
        .iter()
        .fold(boxes[order[0]], |acc, &i| merged(acc, boxes[i]));
    let id = nodes.len();
    nodes.push(Node {
        bounds,
        kind: Kind::Leaf {
            start: base,
            len: order.len(),
        },
    });
    if order.len() <= LEAF_SIZE {
        return id;
    }
    let k = widest_axis(bounds);
    let mid = order.len() / 2;
    order.select_nth_unstable_by(mid, |&a, &b| {
        centres[a][k]
            .total_cmp(&centres[b][k])
            .then_with(|| a.cmp(&b))
    });
    let (lo, hi) = order.split_at_mut(mid);
    let left = build_node(boxes, centres, lo, base, nodes);
    let right = build_node(boxes, centres, hi, base + mid, nodes);
    nodes[id].kind = Kind::Internal { left, right };
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic scatter of boxes, a few thousand overlapping pairs.
    fn scatter(n: usize, seed: u64) -> Vec<Aabb> {
        let mut s = seed;
        let mut next = || {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((s >> 11) as f64) / ((1u64 << 53) as f64)
        };
        (0..n)
            .map(|_| {
                let lo = Vec3::new(next(), next() * 0.3, next() * 2.0);
                let size = Vec3::new(next() * 0.15, next() * 0.15, next() * 0.15);
                (lo, lo + size)
            })
            .collect()
    }

    #[test]
    fn pairs_equal_brute_force() {
        let a = scatter(700, 1);
        let mut b = scatter(500, 2);
        b.push(a[3]); // identical boxes
        b.push((a[5].1, a[5].1 + Vec3::new(0.01, 0.01, 0.01))); // touching at a corner
        let mut brute = Vec::new();
        for (i, &p) in a.iter().enumerate() {
            for (j, &q) in b.iter().enumerate() {
                if overlaps(p, q) {
                    brute.push((i, j));
                }
            }
        }
        assert!(brute.len() > 100);
        assert_eq!(Bvh::build(&a).pairs(&Bvh::build(&b)), brute);
    }

    #[test]
    fn self_pairs_equal_brute_force() {
        let mut a = scatter(900, 3);
        a.push(a[7]);
        let mut brute = Vec::new();
        for i in 0..a.len() {
            for j in i + 1..a.len() {
                if overlaps(a[i], a[j]) {
                    brute.push((i, j));
                }
            }
        }
        assert!(brute.len() > 100);
        assert_eq!(Bvh::build(&a).self_pairs(), brute);
        assert!(Bvh::build(&[]).self_pairs().is_empty());
        assert!(Bvh::build(&a[..1]).self_pairs().is_empty());
    }
}
