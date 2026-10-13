//! Contact between the cloth and a static body (plan §11.2, §11.5): the IPC log barrier on the
//! thickness-offset gap, the edge–edge mollifier, a deterministic spatial-hash broad phase, additive
//! CCD (Li et al. 2021) to cap each Newton step, and lagged smoothed Coulomb friction.
//!
//! Pairs: a cloth vertex against a body triangle, a body vertex against a cloth triangle, and a cloth
//! edge against a body edge. Each touches only cloth vertices that already share an element, so contact
//! never changes the Hessian's sparsity pattern. Energies are evaluated on [`Jet`]s for exact gradients
//! and Hessians.

use std::collections::BTreeMap;

use ftx_core::exact::{Sign, orient3d};
use ftx_core::{Refusal, RefusalCode, Vec3};

use crate::ad::Jet;
use crate::distance::{
    Closest, edge_cross_squared, edge_edge_closest, point_triangle_closest, squared_distance,
    squared_distance_f64, squared_distance_grad,
};

/// Contact and friction parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactParams {
    /// Barrier activation distance on the gap (m): pairs closer than this repel.
    pub dhat: f64,
    /// Offset ξ between the cloth mid-surface and the body (half the cloth thickness, m). The barrier acts
    /// on `distance − ξ`, so clearance is measured to the cloth's real surface.
    pub thickness: f64,
    /// Barrier stiffness κ (J; scales `−(g − ĝ)² ln(g/ĝ)`, which is in m²).
    pub kappa: f64,
    /// Coulomb friction coefficient (0 = frictionless).
    pub mu: f64,
    /// Friction smoothing: slip speeds below this (m/s) are treated as sticking.
    pub eps_v: f64,
}

/// A static body: a triangle mesh (metres), closed (a whole body) or open (a test surface).
#[derive(Clone, Debug)]
pub struct Body {
    /// Vertex positions.
    pub vertices: Vec<Vec3>,
    /// Triangles (outward winding).
    pub triangles: Vec<[usize; 3]>,
    /// Unique edges.
    pub edges: Vec<[usize; 2]>,
    /// Every edge has exactly two triangles, so inside and outside are defined.
    pub closed: bool,
    cell: f64,
    tri_grid: BTreeMap<[i64; 3], Vec<usize>>,
    vert_grid: BTreeMap<[i64; 3], Vec<usize>>,
    edge_grid: BTreeMap<[i64; 3], Vec<usize>>,
}

fn cell_range(lo: Vec3, hi: Vec3, cell: f64) -> ([i64; 3], [i64; 3]) {
    let f = |v: f64| (v / cell).floor() as i64;
    ([f(lo.x), f(lo.y), f(lo.z)], [f(hi.x), f(hi.y), f(hi.z)])
}

fn aabb(points: &[Vec3]) -> (Vec3, Vec3) {
    points.iter().fold(
        (
            Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(lo, hi), p| {
            (
                Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)),
                Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)),
            )
        },
    )
}

fn insert(grid: &mut BTreeMap<[i64; 3], Vec<usize>>, id: usize, lo: Vec3, hi: Vec3, cell: f64) {
    let (a, b) = cell_range(lo, hi, cell);
    for i in a[0]..=b[0] {
        for j in a[1]..=b[1] {
            for k in a[2]..=b[2] {
                grid.entry([i, j, k]).or_default().push(id);
            }
        }
    }
}

fn query(
    grid: &BTreeMap<[i64; 3], Vec<usize>>,
    lo: Vec3,
    hi: Vec3,
    cell: f64,
    out: &mut Vec<usize>,
) {
    out.clear();
    let (a, b) = cell_range(lo, hi, cell);
    for i in a[0]..=b[0] {
        for j in a[1]..=b[1] {
            for k in a[2]..=b[2] {
                if let Some(v) = grid.get(&[i, j, k]) {
                    out.extend_from_slice(v);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
}

/// Unique edges of a triangle list, sorted.
#[must_use]
pub fn edges_of(triangles: &[[usize; 3]]) -> Vec<[usize; 2]> {
    let mut e: Vec<[usize; 2]> = triangles
        .iter()
        .flat_map(|t| {
            (0..3).map(move |k| {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                [a.min(b), a.max(b)]
            })
        })
        .collect();
    e.sort_unstable();
    e.dedup();
    e
}

impl Body {
    /// Builds the body and its broad-phase grids (cell size about the mean edge length, at least `min_cell`).
    ///
    /// # Errors
    /// [`RefusalCode::DegenerateGeometry`] for an empty mesh or an out-of-range index;
    /// [`RefusalCode::NonFinite`] for a non-finite vertex.
    pub fn new(
        vertices: Vec<Vec3>,
        triangles: Vec<[usize; 3]>,
        min_cell: f64,
    ) -> Result<Self, Refusal> {
        if triangles.is_empty() || triangles.iter().flatten().any(|&i| i >= vertices.len()) {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "the body mesh is empty or indexes a missing vertex",
            ));
        }
        if vertices.iter().any(|v| !v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "the body mesh has a non-finite vertex",
            ));
        }
        let edges = edges_of(&triangles);
        let mut uses: BTreeMap<[usize; 2], usize> = BTreeMap::new();
        for t in &triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *uses.entry([a.min(b), a.max(b)]).or_default() += 1;
            }
        }
        let closed = uses.values().all(|&n| n == 2);
        let mean = edges
            .iter()
            .map(|[a, b]| (vertices[*a] - vertices[*b]).norm())
            .sum::<f64>()
            / edges.len() as f64;
        let cell = mean.max(min_cell);
        let mut body = Self {
            vertices,
            triangles,
            edges,
            closed,
            cell,
            tri_grid: BTreeMap::new(),
            vert_grid: BTreeMap::new(),
            edge_grid: BTreeMap::new(),
        };
        for (i, t) in body.triangles.iter().enumerate() {
            let (lo, hi) = aabb(&t.map(|v| body.vertices[v]));
            insert(&mut body.tri_grid, i, lo, hi, cell);
        }
        for (i, v) in body.vertices.iter().enumerate() {
            insert(&mut body.vert_grid, i, *v, *v, cell);
        }
        for (i, e) in body.edges.iter().enumerate() {
            let (lo, hi) = aabb(&e.map(|v| body.vertices[v]));
            insert(&mut body.edge_grid, i, lo, hi, cell);
        }
        Ok(body)
    }

    /// Winding number of the surface about `p` (Van Oosterom–Strackee solid angles): about 1 inside a
    /// closed, outward-wound body and 0 outside.
    #[must_use]
    pub fn winding_number(&self, p: Vec3) -> f64 {
        let mut omega = 0.0;
        for t in &self.triangles {
            let [a, b, c] = t.map(|i| self.vertices[i] - p);
            let (la, lb, lc) = (a.norm(), b.norm(), c.norm());
            let num = a.dot(b.cross(c));
            let den = la * lb * lc + a.dot(b) * lc + a.dot(c) * lb + b.dot(c) * la;
            omega += 2.0 * ftx_core::det::atan2(num, den);
        }
        omega / (4.0 * std::f64::consts::PI)
    }
}

/// Why a cloth state is not intersection-free with the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intersection {
    /// Cloth edge `ce` passes through body triangle `bt`.
    ClothEdge {
        /// Cloth edge.
        ce: usize,
        /// Body triangle.
        bt: usize,
    },
    /// Body edge `be` passes through cloth triangle `ct`.
    BodyEdge {
        /// Cloth triangle.
        ct: usize,
        /// Body edge.
        be: usize,
    },
    /// The cloth piece holding vertex `cv` lies wholly inside the (closed) body.
    Inside {
        /// The piece's lowest-numbered vertex.
        cv: usize,
    },
}

/// Union–find root with path halving.
fn find(root: &mut [usize], mut i: usize) -> usize {
    while root[i] != i {
        root[i] = root[root[i]];
        i = root[i];
    }
    i
}

/// Exact orientation of four points (`ftx_core::exact::orient3d`).
fn orient(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> Sign {
    let v = |p: Vec3| [p.x, p.y, p.z];
    orient3d(v(a), v(b), v(c), v(d))
}

/// Whether segment `pq` meets triangle `t` (touching counts), decided with exact predicates. A segment
/// in the triangle's plane is left to the distance check, which sees it as a zero gap.
fn segment_meets_triangle(p: Vec3, q: Vec3, t: [Vec3; 3]) -> bool {
    if orient(t[0], t[1], t[2], p) == orient(t[0], t[1], t[2], q) {
        return false; // both ends on one side, or both in the plane
    }
    let s = [
        orient(p, q, t[0], t[1]),
        orient(p, q, t[1], t[2]),
        orient(p, q, t[2], t[0]),
    ];
    !(s.contains(&Sign::Positive) && s.contains(&Sign::Negative))
}

/// A contact candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pair {
    /// Cloth vertex against body triangle.
    VertexTri {
        /// Cloth vertex.
        cv: usize,
        /// Body triangle.
        bt: usize,
    },
    /// Body vertex against cloth triangle.
    TriVertex {
        /// Cloth triangle.
        ct: usize,
        /// Body vertex.
        bv: usize,
    },
    /// Cloth edge against body edge.
    EdgeEdge {
        /// Cloth edge.
        ce: usize,
        /// Body edge.
        be: usize,
    },
}

/// The cloth side of contact: its triangles and edges (with rest lengths for the mollifier).
#[derive(Clone, Debug)]
pub struct ClothTopology {
    /// Triangles.
    pub triangles: Vec<[usize; 3]>,
    /// Unique edges.
    pub edges: Vec<[usize; 2]>,
    /// Rest length of each edge (m).
    pub edge_rest: Vec<f64>,
}

/// A pair's four points: positions, which are fixed (body), and the cloth vertex behind each free one.
struct Stencil {
    pts: [Vec3; 4],
    fixed: [bool; 4],
    cloth: [Option<usize>; 4],
    mollifier_eps: Option<f64>,
}

fn stencil(pair: Pair, x: &[Vec3], body: &Body, cloth: &ClothTopology) -> Stencil {
    match pair {
        Pair::VertexTri { cv, bt } => {
            let t = body.triangles[bt];
            Stencil {
                pts: [
                    x[cv],
                    body.vertices[t[0]],
                    body.vertices[t[1]],
                    body.vertices[t[2]],
                ],
                fixed: [false, true, true, true],
                cloth: [Some(cv), None, None, None],
                mollifier_eps: None,
            }
        }
        Pair::TriVertex { ct, bv } => {
            let t = cloth.triangles[ct];
            Stencil {
                pts: [body.vertices[bv], x[t[0]], x[t[1]], x[t[2]]],
                fixed: [true, false, false, false],
                cloth: [None, Some(t[0]), Some(t[1]), Some(t[2])],
                mollifier_eps: None,
            }
        }
        Pair::EdgeEdge { ce, be } => {
            let (c, b) = (cloth.edges[ce], body.edges[be]);
            let lb = (body.vertices[b[0]] - body.vertices[b[1]]).norm2();
            let lc = cloth.edge_rest[ce] * cloth.edge_rest[ce];
            Stencil {
                pts: [x[c[0]], x[c[1]], body.vertices[b[0]], body.vertices[b[1]]],
                fixed: [false, false, true, true],
                cloth: [Some(c[0]), Some(c[1]), None, None],
                mollifier_eps: Some(1e-3 * lc * lb),
            }
        }
    }
}

fn classify(pair: Pair, pts: &[Vec3; 4]) -> (Closest, [f64; 4]) {
    match pair {
        Pair::VertexTri { .. } | Pair::TriVertex { .. } => {
            let (w, k) = point_triangle_closest(pts[0], pts[1], pts[2], pts[3]);
            (k, [1.0, -w[0], -w[1], -w[2]])
        }
        Pair::EdgeEdge { .. } => {
            let ((s, t), k) = edge_edge_closest(pts[0], pts[1], pts[2], pts[3]);
            (k, [1.0 - s, s, -(1.0 - t), -t])
        }
    }
}

/// Barrier `−κ (g − ĝ)² ln(g/ĝ)` on a gap jet (0 < g < ĝ).
fn barrier(g: Jet<12>, p: &ContactParams) -> Jet<12> {
    let r = g - Jet::constant(p.dhat);
    let l = (g * (1.0 / p.dhat)).ln();
    r * r * l * (-p.kappa)
}

/// One pair's energy: inactive (gap ≥ ĝ), penetrating (gap ≤ 0, the cloth surface touching or inside
/// the body: infinite energy), with derivatives, or as a bare value.
enum PairEnergy {
    Inactive,
    Penetrating,
    Active(Box<Jet<12>>),
    Value(f64),
}

/// The pair's gap (distance − ξ) in plain f64.
fn pair_gap(st: &Stencil, kind: Closest, p: &ContactParams) -> f64 {
    squared_distance_f64(st.pts, kind).sqrt() - p.thickness
}

/// The pair's energy value only (no derivatives): the cheap path for line searches.
fn pair_energy_value(st: &Stencil, kind: Closest, p: &ContactParams) -> PairEnergy {
    let gap = pair_gap(st, kind, p);
    if gap >= p.dhat {
        return PairEnergy::Inactive;
    }
    if gap <= 0.0 {
        return PairEnergy::Penetrating;
    }
    let r = gap - p.dhat;
    let mut e = -p.kappa * r * r * ftx_core::det::ln(gap / p.dhat);
    if let Some(eps) = st.mollifier_eps {
        let (a, b) = (st.pts[1] - st.pts[0], st.pts[3] - st.pts[2]);
        let c = a.cross(b).norm2();
        if c < eps {
            e *= (2.0 - c / eps) * c / eps;
        }
    }
    PairEnergy::Value(e)
}

fn pair_energy(st: &Stencil, kind: Closest, p: &ContactParams) -> PairEnergy {
    let gap = pair_gap(st, kind, p);
    if gap >= p.dhat {
        return PairEnergy::Inactive;
    }
    if gap <= 0.0 {
        return PairEnergy::Penetrating;
    }
    let d2 = squared_distance(st.pts, st.fixed, kind);
    let g = d2.sqrt() - Jet::constant(p.thickness);
    let mut e = barrier(g, p);
    if let Some(eps) = st.mollifier_eps {
        let c = edge_cross_squared(st.pts, st.fixed);
        if c.v < eps {
            let u = c * (1.0 / eps);
            let m = (Jet::constant(2.0) - u) * u;
            e = e * m;
        }
    }
    PairEnergy::Active(Box::new(e))
}

/// The contact state of a solver: the body, parameters, cloth topology and the friction lag.
#[derive(Clone, Debug)]
pub struct Contact {
    /// The body.
    pub body: Body,
    /// Parameters.
    pub params: ContactParams,
    /// Cloth triangles and edges.
    pub cloth: ClothTopology,
    lag: Vec<FrictionPair>,
    lag_x: Vec<Vec3>,
    lag_h: f64,
}

/// A frozen friction pair: normal force, tangent basis and contact weights from the lag state.
#[derive(Clone, Copy, Debug)]
struct FrictionPair {
    pair: Pair,
    beta: [f64; 4],
    tangent: [Vec3; 2],
    lambda: f64,
}

/// Contribution of contact to the solver: energy (∞ if penetrating), and per-pair local gradients and
/// Hessians over cloth vertices.
pub(crate) struct ContactTerms {
    pub(crate) energy: f64,
    pub(crate) blocks: Vec<Block>,
}

/// A local gradient and Hessian over the cloth coordinates of one pair: `(vertex, component)` per row.
pub(crate) struct Block {
    pub(crate) dofs: Vec<(usize, usize)>,
    pub(crate) grad: Vec<f64>,
    pub(crate) hess: Vec<f64>,
    /// Already positive semidefinite (no projection needed).
    pub(crate) psd: bool,
}

/// Restricts a 12-coordinate pair block to the cloth's coordinates (the body is static).
fn compact(
    cloth: [Option<usize>; 4],
    grad: &[f64; 12],
    hess: impl Fn(usize, usize) -> f64,
    psd: bool,
) -> Block {
    let rows: Vec<usize> = (0..12).filter(|&a| cloth[a / 3].is_some()).collect();
    Block {
        dofs: rows
            .iter()
            .filter_map(|&a| cloth[a / 3].map(|v| (v, a % 3)))
            .collect(),
        grad: rows.iter().map(|&a| grad[a]).collect(),
        hess: rows
            .iter()
            .flat_map(|&a| rows.iter().map(move |&b| (a, b)))
            .map(|(a, b)| hess(a, b))
            .collect(),
        psd,
    }
}

/// Barrier value and its first two derivatives in the gap: b, b', b'' (0 < g < ĝ).
fn barrier_f64(g: f64, p: &ContactParams) -> (f64, f64, f64) {
    let r = g - p.dhat;
    let l = ftx_core::det::ln(g / p.dhat);
    (
        -p.kappa * r * r * l,
        -p.kappa * (2.0 * r * l + r * r / g),
        -p.kappa * (2.0 * l + 4.0 * r / g - r * r / (g * g)),
    )
}

fn tangent_basis(n: Vec3) -> [Vec3; 2] {
    let a = if n.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let t1 = n.cross(a).normalized();
    [t1, n.cross(t1)]
}

impl Contact {
    /// A contact state for a cloth with these triangles and rest positions (2D material coordinates
    /// lifted with z = 0 are fine: only edge lengths are used).
    #[must_use]
    pub fn new(
        body: Body,
        params: ContactParams,
        triangles: Vec<[usize; 3]>,
        edge_rest: impl Fn(usize, usize) -> f64,
    ) -> Self {
        let edges = edges_of(&triangles);
        let rest = edges.iter().map(|&[a, b]| edge_rest(a, b)).collect();
        Self {
            body,
            params,
            cloth: ClothTopology {
                triangles,
                edges,
                edge_rest: rest,
            },
            lag: Vec::new(),
            lag_x: Vec::new(),
            lag_h: 1.0,
        }
    }

    /// Candidate pairs whose primitives' boxes (from `x` to `x + p`, if given) come within `pad` of each other.
    pub(crate) fn candidates(&self, x: &[Vec3], p: Option<&[Vec3]>, pad: f64) -> Vec<Pair> {
        let b = &self.body;
        let mut out = Vec::new();
        let mut buf = Vec::new();
        let span = |ids: &[usize]| -> (Vec3, Vec3) {
            let mut pts: Vec<Vec3> = ids.iter().map(|&i| x[i]).collect();
            if let Some(p) = p {
                pts.extend(ids.iter().map(|&i| x[i] + p[i]));
            }
            let (lo, hi) = aabb(&pts);
            (lo - Vec3::new(pad, pad, pad), hi + Vec3::new(pad, pad, pad))
        };
        for cv in 0..x.len() {
            let (lo, hi) = span(&[cv]);
            query(&b.tri_grid, lo, hi, b.cell, &mut buf);
            out.extend(buf.iter().map(|&bt| Pair::VertexTri { cv, bt }));
        }
        for (ct, t) in self.cloth.triangles.iter().enumerate() {
            let (lo, hi) = span(t);
            query(&b.vert_grid, lo, hi, b.cell, &mut buf);
            out.extend(buf.iter().map(|&bv| Pair::TriVertex { ct, bv }));
        }
        for (ce, e) in self.cloth.edges.iter().enumerate() {
            let (lo, hi) = span(e);
            query(&b.edge_grid, lo, hi, b.cell, &mut buf);
            out.extend(buf.iter().map(|&be| Pair::EdgeEdge { ce, be }));
        }
        out
    }

    /// Smallest gap (distance − ξ) over all candidate pairs within `range` (m), or `None` if none.
    #[must_use]
    pub fn min_gap(&self, x: &[Vec3], range: f64) -> Option<f64> {
        self.candidates(x, None, range + self.params.thickness)
            .into_iter()
            .map(|pair| {
                let st = stencil(pair, x, &self.body, &self.cloth);
                let (kind, _) = classify(pair, &st.pts);
                pair_gap(&st, kind, &self.params)
            })
            .reduce(f64::min)
    }

    /// The first way the cloth at `x` crosses the body, or `None` when it is intersection-free: an edge
    /// of either mesh through a triangle of the other (exact predicates), or (for a closed body) a
    /// cloth piece wholly inside it. Near-contact without crossing is [`Contact::min_gap`]'s business.
    ///
    /// # Panics
    /// On a non-finite position (check first, as [`crate::Solver::with_contact`] does).
    #[must_use]
    pub fn first_intersection(&self, x: &[Vec3]) -> Option<Intersection> {
        let b = &self.body;
        let mut buf = Vec::new();
        for (ce, e) in self.cloth.edges.iter().enumerate() {
            let (lo, hi) = aabb(&e.map(|i| x[i]));
            query(&b.tri_grid, lo, hi, b.cell, &mut buf);
            if let Some(&bt) = buf.iter().find(|&&bt| {
                segment_meets_triangle(x[e[0]], x[e[1]], b.triangles[bt].map(|i| b.vertices[i]))
            }) {
                return Some(Intersection::ClothEdge { ce, bt });
            }
        }
        for (ct, t) in self.cloth.triangles.iter().enumerate() {
            let tri = t.map(|i| x[i]);
            let (lo, hi) = aabb(&tri);
            query(&b.edge_grid, lo, hi, b.cell, &mut buf);
            if let Some(&be) = buf.iter().find(|&&be| {
                let [p, q] = b.edges[be].map(|i| b.vertices[i]);
                segment_meets_triangle(p, q, tri)
            }) {
                return Some(Intersection::BodyEdge { ct, be });
            }
        }
        if b.closed {
            // Nothing crosses, so each piece is wholly inside or outside: test one vertex per piece.
            let mut root: Vec<usize> = (0..x.len()).collect();
            for t in &self.cloth.triangles {
                for k in 1..3 {
                    let (ra, rb) = (find(&mut root, t[0]), find(&mut root, t[k]));
                    root[ra.max(rb)] = ra.min(rb);
                }
            }
            let used: std::collections::BTreeSet<usize> =
                self.cloth.triangles.iter().flatten().copied().collect();
            for cv in used {
                if find(&mut root, cv) == cv && b.winding_number(x[cv]).abs() > 0.5 {
                    return Some(Intersection::Inside { cv });
                }
            }
        }
        None
    }

    /// Barrier (and, during a time step, friction) energy and its derivative blocks at `x`.
    pub(crate) fn terms(&self, x: &[Vec3], with_derivatives: bool, friction: bool) -> ContactTerms {
        let p = &self.params;
        let mut terms = ContactTerms {
            energy: 0.0,
            blocks: Vec::new(),
        };
        for pair in self.candidates(x, None, p.dhat + p.thickness) {
            let st = stencil(pair, x, &self.body, &self.cloth);
            let (kind, _) = classify(pair, &st.pts);
            if !with_derivatives {
                match pair_energy_value(&st, kind, p) {
                    PairEnergy::Inactive => {}
                    PairEnergy::Penetrating => {
                        terms.energy = f64::INFINITY;
                        return terms;
                    }
                    PairEnergy::Value(e) => terms.energy += e,
                    PairEnergy::Active(e) => terms.energy += e.v,
                }
                continue;
            }
            let gap = pair_gap(&st, kind, p);
            if gap >= p.dhat {
                continue;
            }
            if gap <= 0.0 {
                terms.energy = f64::INFINITY;
                return terms;
            }
            let mollified = st.mollifier_eps.is_some_and(|eps| {
                let (a, b) = (st.pts[1] - st.pts[0], st.pts[3] - st.pts[2]);
                a.cross(b).norm2() < eps
            });
            if mollified {
                // Near-parallel edges: the full Hessian of mollifier × barrier, projected later.
                if let PairEnergy::Active(e) = pair_energy(&st, kind, p) {
                    terms.energy += e.v;
                    terms
                        .blocks
                        .push(compact(st.cloth, &e.g, |a, b| e.h[a][b], false));
                }
                continue;
            }
            // Gauss–Newton: keep b''·∇g∇gᵀ, drop the indefinite b'·∇²g (exact gradient, PSD Hessian;
            // exact for a point against a plane).
            let d2 = squared_distance_grad(st.pts, st.fixed, kind);
            let d = d2.v.sqrt();
            let dg: [f64; 12] = std::array::from_fn(|i| d2.g[i] / (2.0 * d));
            let (b, b1, b2) = barrier_f64(d - p.thickness, p);
            terms.energy += b;
            let grad: [f64; 12] = std::array::from_fn(|i| b1 * dg[i]);
            terms
                .blocks
                .push(compact(st.cloth, &grad, |i, j| b2 * dg[i] * dg[j], true));
        }
        if friction && p.mu > 0.0 {
            for f in &self.lag {
                let (e, g, h) = self.friction_local(f, x);
                terms.energy += e;
                if with_derivatives {
                    let st = stencil(f.pair, x, &self.body, &self.cloth);
                    terms
                        .blocks
                        .push(compact(st.cloth, &g, |a, b| h[a * 12 + b], true));
                }
            }
        }
        terms
    }

    /// Freezes the friction pairs (normal forces, tangent bases, contact weights) at `x` for a step of
    /// size `h` starting at `x_start` (IPC lagged friction).
    pub(crate) fn update_lag(&mut self, x_start: &[Vec3], x: &[Vec3], h: f64) {
        self.lag.clear();
        self.lag_x = x_start.to_vec();
        self.lag_h = h;
        if self.params.mu <= 0.0 {
            return;
        }
        let p = self.params;
        // Normal forces, tangents and contact weights from `x` (the latest iterate); slip is measured
        // from the start of the step.
        for pair in self.candidates(x, None, p.dhat + p.thickness) {
            let st = stencil(pair, x, &self.body, &self.cloth);
            let (kind, beta) = classify(pair, &st.pts);
            let d2 = squared_distance_f64(st.pts, kind);
            let d = d2.sqrt();
            let gap = d - p.thickness;
            if !(gap > 0.0 && gap < p.dhat) {
                continue;
            }
            // Normal force λ = −∂b/∂gap = κ·[2r·ln(g/ĝ) + r²/g], r = g − ĝ < 0 (positive inside the
            // barrier's range; the mollifier is left out of the friction magnitude, as in IPC).
            let r = gap - p.dhat;
            let lambda = p.kappa * (2.0 * r * ftx_core::det::ln(gap / p.dhat) + r * r / gap);
            if lambda.is_nan() || lambda <= 0.0 {
                continue;
            }
            let ca: Vec3 = (0..4)
                .filter(|&i| beta[i] > 0.0)
                .map(|i| st.pts[i] * beta[i])
                .fold(Vec3::ZERO, |a, b| a + b);
            let cb: Vec3 = (0..4)
                .filter(|&i| beta[i] < 0.0)
                .map(|i| st.pts[i] * -beta[i])
                .fold(Vec3::ZERO, |a, b| a + b);
            let n = (ca - cb).normalized();
            self.lag.push(FrictionPair {
                pair,
                beta,
                tangent: tangent_basis(n),
                lambda,
            });
        }
    }

    /// Smoothed Coulomb friction `μλ f₀(|u|)`, u the tangential relative displacement since the lag state.
    fn friction_local(&self, f: &FrictionPair, x: &[Vec3]) -> (f64, [f64; 12], Box<[f64; 144]>) {
        let st_now = stencil(f.pair, x, &self.body, &self.cloth);
        let st_lag = stencil(f.pair, &self.lag_x, &self.body, &self.cloth);
        let rel: Vec3 = (0..4)
            .map(|i| (st_now.pts[i] - st_lag.pts[i]) * f.beta[i])
            .fold(Vec3::ZERO, |a, b| a + b);
        let u = [f.tangent[0].dot(rel), f.tangent[1].dot(rel)];
        let y = (u[0] * u[0] + u[1] * u[1]).sqrt();
        let eps = self.params.eps_v * self.lag_h;
        let c = self.params.mu * f.lambda;
        let (f0, f1_over_y, hess_coef) = if y < eps {
            (
                -y * y * y / (3.0 * eps * eps) + y * y / eps + eps / 3.0,
                -y / (eps * eps) + 2.0 / eps,
                -1.0 / (eps * eps * y.max(1e-300)),
            )
        } else {
            (y, 1.0 / y, -1.0 / (y * y * y))
        };
        // ∂D/∂u = c·f₁(y)/y·u; ∂²D/∂u² = c·[(f₁/y)·I + hess_coef·u uᵀ] (PSD for y ≤ ε and beyond).
        let gu = [c * f1_over_y * u[0], c * f1_over_y * u[1]];
        let mut hu = [[c * f1_over_y, 0.0], [0.0, c * f1_over_y]];
        if y > 0.0 {
            for a in 0..2 {
                for b in 0..2 {
                    hu[a][b] += c * hess_coef * u[a] * u[b];
                }
            }
        }
        // u = A·x_local with A (2 × 12) = Tᵀ [β₀I β₁I β₂I β₃I].
        let a: [[f64; 12]; 2] =
            std::array::from_fn(|r| std::array::from_fn(|j| f.beta[j / 3] * f.tangent[r][j % 3]));
        let mut g = [0.0; 12];
        let mut h = Box::new([0.0; 144]);
        for i in 0..12 {
            g[i] = a[0][i] * gu[0] + a[1][i] * gu[1];
            for j in 0..12 {
                let mut s = 0.0;
                for r in 0..2 {
                    for q in 0..2 {
                        s += a[r][i] * hu[r][q] * a[q][j];
                    }
                }
                h[i * 12 + j] = s;
            }
        }
        (c * f0, g, h)
    }

    /// The largest fraction α ∈ (0, 1] of the step `p` that keeps every pair's gap positive (additive
    /// CCD: no gap closes below a tenth of its current value).
    pub(crate) fn step_bound(&self, x: &[Vec3], p: &[Vec3]) -> f64 {
        let mut alpha: f64 = 1.0;
        for pair in self.candidates(x, Some(p), self.params.thickness) {
            let st = stencil(pair, x, &self.body, &self.cloth);
            let dp: [Vec3; 4] = std::array::from_fn(|i| st.cloth[i].map_or(Vec3::ZERO, |v| p[v]));
            alpha = alpha.min(accd(pair, st.pts, dp, self.params.thickness, alpha));
        }
        alpha
    }
}

/// Additive CCD (Li et al. 2021, Algorithm 1, s = 0.1): a time t ∈ (0, t_max] before which the pair's
/// gap stays above 10 % of its start value along the linear motion `pts + t·dp`, or `t_max` if it does.
#[must_use]
pub fn accd(pair: Pair, pts: [Vec3; 4], dp: [Vec3; 4], xi: f64, t_max: f64) -> f64 {
    let s = 0.1; // C-IPC's choice: a step may close a gap to 10 % of its start value
    let mean = dp.iter().fold(Vec3::ZERO, |a, b| a + *b) / 4.0;
    let rel: [Vec3; 4] = dp.map(|d| d - mean);
    let (ga, gb): (&[Vec3], &[Vec3]) = match pair {
        Pair::VertexTri { .. } | Pair::TriVertex { .. } => (&rel[..1], &rel[1..]),
        Pair::EdgeEdge { .. } => (&rel[..2], &rel[2..]),
    };
    let lp = ga.iter().map(|v| v.norm()).fold(0.0, f64::max)
        + gb.iter().map(|v| v.norm()).fold(0.0, f64::max);
    if lp == 0.0 {
        return t_max;
    }
    let dist2 = |q: &[Vec3; 4]| -> f64 {
        let (kind, _) = classify(pair, q);
        squared_distance_f64(*q, kind)
    };
    let gap_of = |d2: f64| (d2 - xi * xi) / (d2.sqrt() + xi);
    let mut q = pts;
    let mut d2 = dist2(&q);
    let target = s * gap_of(d2);
    let mut t = 0.0;
    let mut tl = (1.0 - s) * gap_of(d2) / lp;
    for _ in 0..10_000 {
        for (qi, ri) in q.iter_mut().zip(&rel) {
            *qi += *ri * tl;
        }
        d2 = dist2(&q);
        if t > 0.0 && gap_of(d2) < target {
            return t;
        }
        t += tl;
        if t > t_max {
            return t_max;
        }
        tl = 0.9 * gap_of(d2) / lp;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::Vec2;

    fn params() -> ContactParams {
        ContactParams {
            dhat: 1e-3,
            thickness: 5e-4,
            kappa: 10.0,
            mu: 0.3,
            eps_v: 1e-3,
        }
    }

    /// KAT 1, barrier: zero with zero slope at the activation gap, infinite at contact, and the pair
    /// energies' gradients and Hessians equal central differences for every pair type, including the
    /// mollified edge–edge case.
    #[test]
    fn barrier_and_pair_calculus() {
        let p = params();
        let at = |g: f64| barrier(Jet::<12>::var(g, 0), &p);
        let edge = at(p.dhat * (1.0 - 1e-9));
        assert!(
            edge.v.abs() < 1e-20 && edge.g[0].abs() < 1e-9,
            "C¹ at ĝ: {} {}",
            edge.v,
            edge.g[0]
        );
        assert!(at(1e-9).g[0] < -1e-3, "repels near contact");
        // Barrier derivatives against differences of the value.
        for g in [1e-5, 1e-4, 5e-4, 9e-4] {
            let (b, h) = (at(g), 1e-9);
            let fd = (at(g + h).v - at(g - h).v) / (2.0 * h);
            let fd2 = (at(g + h).g[0] - at(g - h).g[0]) / (2.0 * h);
            assert!((fd - b.g[0]).abs() < 1e-6 * b.g[0].abs(), "b' at {g}");
            assert!(
                (fd2 - b.h[0][0]).abs() < 1e-5 * b.h[0][0].abs(),
                "b'' at {g}"
            );
        }
        // A body "tent" whose apex pokes up under a two-triangle cloth patch about 1.1 mm above the
        // base, with one cloth edge nearly parallel to a body edge (mollifier active).
        let body = Body::new(
            vec![
                Vec3::new(-0.01, -0.01, 0.0),
                Vec3::new(0.01, -0.01, 0.0),
                Vec3::new(0.0, 0.012, 0.0),
                Vec3::new(0.0005, 0.0003, 0.0002),
            ],
            vec![[0, 1, 3], [1, 2, 3], [2, 0, 3]],
            1e-3,
        )
        .unwrap();
        let cloth_x = vec![
            Vec3::new(0.001, 0.0005, 0.0012),
            Vec3::new(-0.004, 0.003, 0.0011),
            Vec3::new(0.004, -0.002, 0.0013),
            Vec3::new(-0.006, -0.00636, 0.0011), // edge 3–0 within ~0.1° of body edge 0–3
        ];
        let tris = vec![[0, 1, 2], [3, 0, 1]];
        let contact = Contact::new(body, p, tris, |a, b| (cloth_x[a] - cloth_x[b]).norm());
        let mut seen = std::collections::BTreeSet::new();
        let mut mollified = 0;
        for pair in contact.candidates(&cloth_x, None, p.dhat + p.thickness) {
            let energy = |x: &[Vec3]| -> Option<Jet<12>> {
                let st = stencil(pair, x, &contact.body, &contact.cloth);
                let (kind, _) = classify(pair, &st.pts);
                match pair_energy(&st, kind, &p) {
                    PairEnergy::Active(e) => Some(*e),
                    _ => None,
                }
            };
            let Some(e) = energy(&cloth_x) else { continue };
            let st = stencil(pair, &cloth_x, &contact.body, &contact.cloth);
            seen.insert(match pair {
                Pair::VertexTri { .. } => 0,
                Pair::TriVertex { .. } => 1,
                Pair::EdgeEdge { .. } => 2,
            });
            let gscale = e.g.iter().fold(1e-12f64, |m, g| m.max(g.abs()));
            if st
                .mollifier_eps
                .is_some_and(|eps| edge_cross_squared(st.pts, st.fixed).v < eps)
            {
                mollified += 1;
            } else {
                // The solver's path (first-order dual distance, closed-form barrier) gives the jet's
                // value and gradient to 1e-8 relative.
                let (kind, _) = classify(pair, &st.pts);
                let d2 = squared_distance_grad(st.pts, st.fixed, kind);
                let d = d2.v.sqrt();
                let (b, b1, _) = barrier_f64(d - p.thickness, &p);
                assert!((b - e.v).abs() <= 1e-8 * e.v.abs(), "{pair:?} value");
                for i in 0..12 {
                    let gi = b1 * d2.g[i] / (2.0 * d);
                    assert!(
                        (gi - e.g[i]).abs() <= 1e-8 * gscale,
                        "{pair:?} solver grad[{i}]: {gi} vs {}",
                        e.g[i]
                    );
                }
            }
            let h = 1e-9;
            let hscale = e.h.iter().flatten().fold(1e-12f64, |m, g| m.max(g.abs()));
            for (k, cv) in st.cloth.iter().enumerate() {
                let Some(v) = *cv else { continue };
                for c in 0..3 {
                    let bump = |d: f64| {
                        let mut x = cloth_x.clone();
                        match c {
                            0 => x[v].x += d,
                            1 => x[v].y += d,
                            _ => x[v].z += d,
                        }
                        x
                    };
                    let (Some(ep), Some(em)) = (energy(&bump(h)), energy(&bump(-h))) else {
                        continue;
                    };
                    let i = 3 * k + c;
                    let fd = (ep.v - em.v) / (2.0 * h);
                    assert!(
                        (fd - e.g[i]).abs() < 1e-5 * gscale,
                        "{pair:?} grad[{i}]: {fd} vs {}",
                        e.g[i]
                    );
                    for j in 0..12 {
                        let fdh = (ep.g[j] - em.g[j]) / (2.0 * h);
                        assert!(
                            (fdh - e.h[j][i]).abs() < 1e-4 * hscale,
                            "{pair:?} hess[{j}][{i}]: {fdh} vs {}",
                            e.h[j][i]
                        );
                    }
                }
            }
        }
        assert_eq!(seen.len(), 3, "every pair type active");
        assert!(mollified > 0, "a mollified edge pair was checked");
    }

    /// The start audit finds each way a cloth can cross a closed body, passes a clear one, and
    /// `with_contact` refuses a crossing start whose gaps are all positive.
    #[test]
    fn start_intersections_are_found() {
        let s = 0.1;
        let tetra = || {
            Body::new(
                vec![
                    Vec3::ZERO,
                    Vec3::new(s, 0.0, 0.0),
                    Vec3::new(0.0, s, 0.0),
                    Vec3::new(0.0, 0.0, s),
                ],
                vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
                0.01,
            )
            .unwrap()
        };
        assert!(tetra().closed);
        assert!((tetra().winding_number(Vec3::new(0.01, 0.01, 0.01)) - 1.0).abs() < 1e-12);
        assert!(tetra().winding_number(Vec3::new(0.2, 0.2, 0.2)).abs() < 1e-12);
        let audit = |x: Vec<Vec3>| {
            let c = Contact::new(tetra(), params(), vec![[0, 1, 2]], |a, b| {
                (x[a] - x[b]).norm()
            });
            c.first_intersection(&x)
        };
        // Clear, above the apex.
        let clear = vec![
            Vec3::new(0.0, 0.0, 0.2),
            Vec3::new(s, 0.0, 0.2),
            Vec3::new(0.0, s, 0.2),
        ];
        assert_eq!(audit(clear), None);
        // A cloth edge through the bottom face.
        let edge_through = vec![
            Vec3::new(0.02, 0.02, -0.05),
            Vec3::new(0.02, 0.02, 0.05),
            Vec3::new(0.5, 0.5, -0.05),
        ];
        assert!(matches!(
            audit(edge_through),
            Some(Intersection::ClothEdge { .. })
        ));
        // The apex poking through a large cloth triangle at half height: no cloth edge crosses, and
        // every cloth vertex is far from the body.
        let pierced = vec![
            Vec3::new(-1.0, -1.0, 0.05),
            Vec3::new(1.0, -1.0, 0.05),
            Vec3::new(0.0, 1.0, 0.05),
        ];
        assert!(matches!(
            audit(pierced.clone()),
            Some(Intersection::BodyEdge { ct: 0, .. })
        ));
        // Wholly inside.
        let inside = vec![
            Vec3::new(0.01, 0.01, 0.01),
            Vec3::new(0.02, 0.01, 0.01),
            Vec3::new(0.01, 0.02, 0.01),
        ];
        assert_eq!(audit(inside), Some(Intersection::Inside { cv: 0 }));
        // The solver refuses the pierced start although every gap is positive.
        let rest: Vec<Vec2> = pierced.iter().map(|p| Vec2::new(p.x, p.y)).collect();
        let model = crate::ClothModel::new(
            rest,
            &[[0, 1, 2]],
            crate::Membrane::orthotropic(1000.0, 1000.0, 0.0, 100.0, 0.05),
            1e-5,
            0.2,
        )
        .unwrap();
        let solver = crate::Solver::new(model, pierced, &[[false; 3]; 3], Vec3::ZERO);
        let err = solver.with_contact(tetra(), params()).unwrap_err();
        assert_eq!(err.code, RefusalCode::IntersectingStart);
    }

    /// Additive CCD never lets a moving point pass a triangle, and returns the full step when the
    /// motion stays clear.
    #[test]
    fn accd_stops_before_contact() {
        let tri = [
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let pair = Pair::VertexTri { cv: 0, bt: 0 };
        let pts = [Vec3::new(0.0, 0.0, 0.01), tri[0], tri[1], tri[2]];
        // Moving straight through the plane: impact when the gap (z − ξ) reaches zero at t = 0.9.
        let xi = 1e-3;
        let t = accd(
            pair,
            pts,
            [
                Vec3::new(0.0, 0.0, -0.02),
                Vec3::ZERO,
                Vec3::ZERO,
                Vec3::ZERO,
            ],
            xi,
            1.0,
        );
        assert!(t > 0.0 && t < 0.45, "t = {t}");
        let z = 0.01 - 0.02 * t;
        assert!(z - xi > 0.0, "still clear at the returned time");
        // Sideways motion keeps the gap: full step.
        let t = accd(
            pair,
            pts,
            [Vec3::new(0.3, 0.0, 0.0), Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
            xi,
            1.0,
        );
        assert_eq!(t, 1.0);
    }
}
