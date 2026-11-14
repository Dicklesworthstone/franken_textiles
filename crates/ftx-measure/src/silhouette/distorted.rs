//! Certified Brown edge crossings, without replacing curved edges by chords.
//! Consistently oriented projected triangles contribute positive winding. Shared directed edges
//! cancel before projection; the remaining weighted curves still describe their complete union.

use std::{
    collections::BTreeMap,
    ops::{Add, Mul},
};

use ftx_core::{Refusal, RefusalCode, Vec2};

use super::{Axis, Camera, Scanline, SurfaceMesh, invalid};
use crate::camera::brown_projection;

const ROOT_WIDTH_PX: f64 = 1e-9;
const MAX_DEPTH: u8 = 60;
const MAX_WORK: usize = 1_000_000;
/// Ordered foreground union and its largest per-endpoint numerical enclosure radius.
pub(super) type Coverage = (Vec<[f64; 2]>, f64);
const CHOOSE: [[f64; 8]; 8] = [
    [1., 0., 0., 0., 0., 0., 0., 0.],
    [1., 1., 0., 0., 0., 0., 0., 0.],
    [1., 2., 1., 0., 0., 0., 0., 0.],
    [1., 3., 3., 1., 0., 0., 0., 0.],
    [1., 4., 6., 4., 1., 0., 0., 0.],
    [1., 5., 10., 10., 5., 1., 0., 0.],
    [1., 6., 15., 20., 15., 6., 1., 0.],
    [1., 7., 21., 35., 35., 21., 7., 1.],
];

#[derive(Clone, Copy, Debug)]
struct Bounds {
    lo: f64,
    hi: f64,
}

impl Bounds {
    fn point(value: f64) -> Self {
        Self {
            lo: value,
            hi: value,
        }
    }
    fn zero(self) -> bool {
        self.lo == 0. && self.hi == 0.
    }
    fn finite(self) -> bool {
        self.lo.is_finite() && self.hi.is_finite() && self.lo <= self.hi
    }
    fn hull(self, other: Self) -> Self {
        if !self.finite() || !other.finite() {
            return Self::point(f64::NAN);
        }
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
    fn sign(self) -> Option<i8> {
        if self.lo > 0. {
            Some(1)
        } else if self.hi < 0. {
            Some(-1)
        } else if self.zero() {
            Some(0)
        } else {
            None
        }
    }
    fn divide(self, denominator: f64) -> Self {
        if denominator == 1. || self.zero() {
            return self;
        }
        Self {
            lo: (self.lo / denominator).next_down(),
            hi: (self.hi / denominator).next_up(),
        }
    }
}

// Error-free addition and an FMA product residual retain structural zeroes. At underflow-scale
// products the residual may not be representable: widen instead of claiming exact arithmetic.
fn sum(a: f64, b: f64) -> Bounds {
    let value = a + b;
    if !a.is_finite() || !b.is_finite() || !value.is_finite() {
        return Bounds::point(f64::NAN);
    }
    let rest = value - a;
    let error = (a - (value - rest)) + (b - rest);
    Bounds {
        lo: if error < 0. { value.next_down() } else { value },
        hi: if error > 0. { value.next_up() } else { value },
    }
}
fn product(a: f64, b: f64) -> Bounds {
    if !a.is_finite() || !b.is_finite() {
        return Bounds::point(f64::NAN);
    }
    if a == 0. || b == 0. {
        return Bounds::point(0.);
    }
    let value = a * b;
    if !value.is_finite() {
        return Bounds::point(f64::NAN);
    }
    let error = a.mul_add(b, -value);
    if value.abs() < 1e-250 {
        return Bounds {
            lo: value.next_down(),
            hi: value.next_up(),
        };
    }
    Bounds {
        lo: if error < 0. { value.next_down() } else { value },
        hi: if error > 0. { value.next_up() } else { value },
    }
}
impl Add for Bounds {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            lo: sum(self.lo, other.lo).lo,
            hi: sum(self.hi, other.hi).hi,
        }
    }
}
impl Mul for Bounds {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        [
            product(self.lo, other.lo),
            product(self.lo, other.hi),
            product(self.hi, other.lo),
            product(self.hi, other.hi),
        ]
        .into_iter()
        .reduce(Bounds::hull)
        .unwrap()
    }
}

/// Coefficients of t^i (1-t)^(n-i), i.e. Bernstein coefficients multiplied by C(n,i).
/// Products, degree elevation and half subdivision then use only integer factors and powers of
/// two. No nonrepresentable 1/7 coefficient can turn an exactly coincident edge into uncertainty.
#[derive(Clone, Copy)]
struct Poly {
    degree: usize,
    coefficients: [Bounds; 8],
}

impl From<f64> for Poly {
    fn from(value: f64) -> Self {
        let mut coefficients = [Bounds::point(0.); 8];
        coefficients[0] = Bounds::point(value);
        Self {
            degree: 0,
            coefficients,
        }
    }
}
impl Poly {
    fn line(a: f64, b: f64) -> Self {
        let mut out = Self::from(a);
        out.degree = 1;
        out.coefficients[1] = Bounds::point(b);
        out
    }
    fn elevate(mut self, degree: usize) -> Self {
        while self.degree < degree {
            for i in (1..=self.degree + 1).rev() {
                self.coefficients[i] = self.coefficients[i] + self.coefficients[i - 1];
            }
            self.degree += 1;
        }
        self
    }
    fn first(self) -> Bounds {
        self.coefficients[0]
    }
    fn last(self) -> Bounds {
        self.coefficients[self.degree]
    }
    fn zero(self) -> bool {
        self.coefficients[..=self.degree].iter().all(|c| c.zero())
    }
    fn finite(self) -> bool {
        self.coefficients[..=self.degree].iter().all(|c| c.finite())
    }
    fn range(self) -> Bounds {
        (0..=self.degree)
            .map(|i| self.coefficients[i].divide(CHOOSE[self.degree][i]))
            .reduce(Bounds::hull)
            .unwrap()
    }
    // Bernstein's variation bound counts roots in the open interval. Zero endpoint factors
    // are handled separately; an uncertain coefficient never supplies a guessed sign.
    fn signs(self) -> Option<(usize, i8, i8)> {
        let (mut count, mut first, mut last) = (0, 0, 0);
        for coefficient in &self.coefficients[..=self.degree] {
            let sign = coefficient.sign()?;
            if sign == 0 {
                continue;
            }
            if first == 0 {
                first = sign;
            }
            if last != 0 && last != sign {
                count += 1;
            }
            last = sign;
        }
        Some((count, first, last))
    }
    fn split(self) -> [Self; 2] {
        let mut halves = [Self::from(0.); 2];
        for half in &mut halves {
            half.degree = self.degree;
        }
        for i in 0..=self.degree {
            for j in 0..=i {
                halves[0].coefficients[i] = halves[0].coefficients[i]
                    + self.coefficients[j] * Bounds::point(CHOOSE[self.degree - j][i - j]);
            }
            halves[0].coefficients[i] =
                halves[0].coefficients[i] * Bounds::point(1. / (1u64 << i) as f64);
            for j in i..=self.degree {
                halves[1].coefficients[i] =
                    halves[1].coefficients[i] + self.coefficients[j] * Bounds::point(CHOOSE[j][i]);
            }
            halves[1].coefficients[i] =
                halves[1].coefficients[i] * Bounds::point(1. / (1u64 << (self.degree - i)) as f64);
        }
        halves
    }
    fn remove_endpoint_roots(mut self) -> Self {
        while self.degree > 0 && self.first().zero() {
            self.coefficients.copy_within(1..=self.degree, 0);
            self.coefficients[self.degree] = Bounds::point(0.);
            self.degree -= 1;
        }
        while self.degree > 0 && self.last().zero() {
            self.degree -= 1;
        }
        self
    }
}
impl Add for Poly {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        let degree = self.degree.max(other.degree);
        let mut out = self.elevate(degree);
        let other = other.elevate(degree);
        for i in 0..=degree {
            out.coefficients[i] = out.coefficients[i] + other.coefficients[i];
        }
        out
    }
}
impl Mul for Poly {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        let mut out = Self::from(0.);
        out.degree = self.degree + other.degree;
        debug_assert!(out.degree <= 7);
        for i in 0..=self.degree {
            for j in 0..=other.degree {
                out.coefficients[i + j] =
                    out.coefficients[i + j] + self.coefficients[i] * other.coefficients[j];
            }
        }
        out
    }
}

struct Budget {
    remaining: usize,
}
impl Budget {
    fn spend(&mut self, amount: usize) -> Result<(), Refusal> {
        self.remaining = self.remaining.checked_sub(amount).ok_or_else(|| {
            Refusal::new(
                RefusalCode::BudgetExceeded,
                "Brown silhouette crossing work budget exhausted; no partial contour was published",
            )
        })?;
        Ok(())
    }
}
fn unresolved() -> Refusal {
    Refusal::new(
        RefusalCode::NotConverged,
        "Brown silhouette crossing topology or subpixel enclosure is unresolved; no contour was approximated",
    )
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Identity {
    Vertex(usize),
    Split {
        edge: usize,
        numerator: u64,
        depth: u8,
    },
    Interior {
        edge: usize,
        ordinal: usize,
    },
}
#[derive(Clone, Copy)]
struct Event {
    identity: Identity,
    position: Bounds,
    delta: i64,
}

fn endpoint(edge: usize, vertices: [usize; 2], mut numerator: u64, mut depth: u8) -> Identity {
    if numerator == 0 {
        return Identity::Vertex(vertices[0]);
    }
    if numerator == 1u64 << depth {
        return Identity::Vertex(vertices[1]);
    }
    while numerator & 1 == 0 {
        numerator >>= 1;
        depth -= 1;
    }
    Identity::Split {
        edge,
        numerator,
        depth,
    }
}

// The parent variation test proves exactly one interior crossing. Collecting all remaining
// coefficient enclosures, including both children of an uncertain split, bounds its location.
fn locate(
    across: Poly,
    along: Poly,
    depth: u8,
    budget: &mut Budget,
) -> Result<Option<Bounds>, Refusal> {
    budget.spend(1)?;
    if let Some((0, first, _)) = across.signs()
        && first != 0
    {
        return Ok(match (across.first().zero(), across.last().zero()) {
            (false, false) => None,
            (true, false) => Some(along.first()),
            (false, true) => Some(along.last()),
            (true, true) => Some(along.first().hull(along.last())),
        });
    }
    let range = along.range();
    if range.hi - range.lo <= ROOT_WIDTH_PX {
        return Ok(Some(range));
    }
    if depth >= MAX_DEPTH {
        return Err(unresolved());
    }
    let a = across.split();
    let x = along.split();
    let left = locate(a[0], x[0], depth + 1, budget)?;
    let right = locate(a[1], x[1], depth + 1, budget)?;
    Ok(match (left, right) {
        (Some(a), Some(b)) => Some(a.hull(b)),
        (a, b) => a.or(b),
    })
}

struct Crossings<'a> {
    edge: usize,
    vertices: [usize; 2],
    multiplier: i64,
    ordinal: usize,
    events: &'a mut Vec<Event>,
    budget: &'a mut Budget,
}
impl Crossings<'_> {
    fn visit(&mut self, across: Poly, along: Poly, index: u64, depth: u8) -> Result<(), Refusal> {
        self.budget.spend(1)?;
        if let Some((changes, first, last)) = across.signs()
            && changes <= 1
            && first != 0
        {
            for (is_root, side, numerator, position, direction) in [
                (across.first().zero(), first, index, along.first(), 1),
                (across.last().zero(), last, index + 1, along.last(), -1),
            ] {
                if is_root && side > 0 {
                    self.events.push(Event {
                        identity: endpoint(self.edge, self.vertices, numerator, depth),
                        position,
                        delta: self.multiplier * direction,
                    });
                }
            }
            if changes == 1 {
                let position = locate(across.remove_endpoint_roots(), along, depth, self.budget)?
                    .ok_or_else(unresolved)?;
                self.events.push(Event {
                    identity: Identity::Interior {
                        edge: self.edge,
                        ordinal: self.ordinal,
                    },
                    position,
                    delta: self.multiplier * if last > first { 1 } else { -1 },
                });
                self.ordinal += 1;
            }
            return Ok(());
        }
        if depth >= MAX_DEPTH {
            return Err(unresolved());
        }
        let a = across.split();
        let x = along.split();
        self.visit(a[0], x[0], index * 2, depth + 1)?;
        self.visit(a[1], x[1], index * 2 + 1, depth + 1)
    }
}

fn coordinate(position: Bounds, error: &mut f64) -> Result<f64, Refusal> {
    if !position.finite() || position.hi - position.lo > ROOT_WIDTH_PX {
        return Err(unresolved());
    }
    let midpoint = position.lo + (position.hi - position.lo) * 0.5;
    let radius = (midpoint - position.lo).max(position.hi - midpoint);
    *error = error.max(if radius == 0. { 0. } else { radius.next_up() });
    Ok(midpoint)
}

fn sweep(events: Vec<Event>) -> Result<Coverage, Refusal> {
    let mut identities: BTreeMap<Identity, Event> = BTreeMap::new();
    for event in events {
        if !event.position.finite() {
            return Err(unresolved());
        }
        if let Some(prior) = identities.get_mut(&event.identity) {
            prior.position = Bounds {
                lo: prior.position.lo.max(event.position.lo),
                hi: prior.position.hi.min(event.position.hi),
            };
            if !prior.position.finite() {
                return Err(unresolved());
            }
            prior.delta += event.delta;
        } else {
            identities.insert(event.identity, event);
        }
    }
    let mut events: Vec<_> = identities
        .into_values()
        .filter(|event| event.delta != 0)
        .collect();
    events.sort_by(|a, b| {
        a.position
            .lo
            .total_cmp(&b.position.lo)
            .then(a.position.hi.total_cmp(&b.position.hi))
            .then(a.identity.cmp(&b.identity))
    });
    let (mut winding, mut index, mut start, mut error) = (0i64, 0, None, 0.0);
    let mut intervals = Vec::new();
    while index < events.len() {
        let mut end = index + 1;
        let mut high = events[index].position.hi;
        while end < events.len() && events[end].position.lo <= high {
            high = high.max(events[end].position.hi);
            end += 1;
        }
        let group = &events[index..end];
        let delta: i64 = group.iter().map(|event| event.delta).sum();
        let negative: i64 = group.iter().map(|event| event.delta.min(0)).sum();
        let next = winding + delta;
        if next < 0 {
            return Err(unresolved());
        }
        let exact_point = group
            .iter()
            .all(|event| event.position.lo == high && event.position.hi == high);
        let position = if group.len() == 1 || exact_point {
            group[0].position
        } else if winding > 0 && winding + negative > 0 {
            // Every possible order stays inside the union: no boundary needs locating here.
            winding = next;
            index = end;
            continue;
        } else if winding == 0 && group.iter().all(|event| event.delta > 0) {
            Bounds {
                lo: group
                    .iter()
                    .map(|e| e.position.lo)
                    .fold(f64::INFINITY, f64::min),
                hi: group
                    .iter()
                    .map(|e| e.position.hi)
                    .fold(f64::INFINITY, f64::min),
            }
        } else if next == 0 && group.iter().all(|event| event.delta < 0) {
            Bounds {
                lo: group
                    .iter()
                    .map(|e| e.position.lo)
                    .fold(f64::NEG_INFINITY, f64::max),
                hi: high,
            }
        } else {
            return Err(unresolved());
        };
        if winding == 0 && next > 0 {
            start = Some(coordinate(position, &mut error)?);
        } else if winding > 0 && next == 0 {
            let hi = coordinate(position, &mut error)?;
            let lo = start.take().ok_or_else(unresolved)?;
            if hi <= lo {
                return Err(unresolved());
            }
            intervals.push([lo, hi]);
        }
        winding = next;
        index = end;
    }
    if winding != 0 || start.is_some() {
        return Err(unresolved());
    }
    Ok((intervals, error))
}

pub(super) fn coverage(
    mesh: &SurfaceMesh,
    camera: &Camera,
    lines: &[&Scanline],
) -> Result<Vec<Coverage>, Refusal> {
    let mut points = Vec::with_capacity(mesh.vertices_m.len());
    for &point in &mesh.vertices_m {
        camera.project(point)?;
        let q = camera.world_to_camera.mul_vec(point - camera.center_m);
        points.push(Vec2::new(q.x / q.z, q.y / q.z));
    }
    let mut edges: BTreeMap<[usize; 2], i64> = BTreeMap::new();
    for &[a, b, c] in &mesh.triangles {
        if [a, b, c].iter().any(|&v| v >= points.len()) || a == b || b == c || c == a {
            return Err(invalid("invalid silhouette triangle indices"));
        }
        let area = (mesh.vertices_m[b] - mesh.vertices_m[a])
            .cross(mesh.vertices_m[c] - mesh.vertices_m[a])
            .norm2();
        if !area.is_finite() || area == 0. {
            return Err(invalid("degenerate silhouette surface triangle"));
        }
        let dx = |i: usize| Bounds::point(points[i].x) + Bounds::point(-points[a].x);
        let dy = |i: usize| Bounds::point(points[i].y) + Bounds::point(-points[a].y);
        let projected = dx(b) * dy(c) + dy(b) * dx(c) * Bounds::point(-1.);
        let order = match projected.sign() {
            Some(0) => continue,
            Some(1) => [a, b, c],
            Some(-1) => [a, c, b],
            _ => return Err(unresolved()),
        };
        for i in 0..3 {
            let (a, b) = (order[i], order[(i + 1) % 3]);
            *edges.entry([a.min(b), a.max(b)]).or_default() += if a < b { 1 } else { -1 };
        }
    }
    let mut budget = Budget {
        remaining: MAX_WORK,
    };
    let intrinsics = camera.intrinsics;
    let mut curves = Vec::new();
    for (vertices, weight) in edges.into_iter().filter(|(_, weight)| *weight != 0) {
        budget.spend(32)?;
        let [a, b] = vertices.map(|index| points[index]);
        let [x, y] = brown_projection(
            Poly::line(a.x, b.x),
            Poly::line(a.y, b.y),
            intrinsics.distortion.radial,
            intrinsics.distortion.tangential,
        );
        let x = Poly::from(intrinsics.focal_px.x) * x + Poly::from(intrinsics.principal_px.x);
        let y = Poly::from(intrinsics.focal_px.y) * y + Poly::from(intrinsics.principal_px.y);
        if !x.finite() || !y.finite() {
            return Err(invalid("Brown silhouette polynomial overflow"));
        }
        curves.push((vertices, weight, x, y));
    }
    let mut result = Vec::with_capacity(lines.len());
    for line in lines {
        let mut events = Vec::new();
        for (edge, &(vertices, weight, x, y)) in curves.iter().enumerate() {
            let (along, across, direction) = match line.axis {
                Axis::Horizontal => (x, y, -1),
                Axis::Vertical => (y, x, 1),
            };
            let across = across + Poly::from(-line.at_px);
            if across.zero() {
                let (a, b) = (along.first(), along.last());
                let (left, right) = if a.hi < b.lo {
                    ((vertices[0], a), (vertices[1], b))
                } else if b.hi < a.lo {
                    ((vertices[1], b), (vertices[0], a))
                } else {
                    return Err(unresolved());
                };
                events.push(Event {
                    identity: Identity::Vertex(left.0),
                    position: left.1,
                    delta: 1,
                });
                events.push(Event {
                    identity: Identity::Vertex(right.0),
                    position: right.1,
                    delta: -1,
                });
            } else {
                Crossings {
                    edge,
                    vertices,
                    multiplier: weight * direction,
                    ordinal: 0,
                    events: &mut events,
                    budget: &mut budget,
                }
                .visit(across, along, 0, 0)?;
            }
        }
        result.push(sweep(events)?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_degree_crossings_keep_all_simple_roots_and_cancel_a_double_tangent() {
        let roots = [0.125, 0.375, 0.625, 0.875, 0.5, 0.5];
        let across = roots.into_iter().fold(Poly::from(1.), |polynomial, root| {
            polynomial * Poly::line(-root, 1. - root)
        });
        let mut events = Vec::new();
        let mut budget = Budget {
            remaining: MAX_WORK,
        };
        Crossings {
            edge: 0,
            vertices: [0, 1],
            multiplier: 1,
            ordinal: 0,
            events: &mut events,
            budget: &mut budget,
        }
        .visit(across, Poly::line(0., 1.), 0, 0)
        .unwrap();
        let mut joined: BTreeMap<Identity, Event> = BTreeMap::new();
        for event in events {
            joined
                .entry(event.identity)
                .and_modify(|prior| prior.delta += event.delta)
                .or_insert(event);
        }
        let mut crossings: Vec<_> = joined
            .into_values()
            .filter(|event| event.delta != 0)
            .collect();
        crossings.sort_by(|a, b| a.position.lo.total_cmp(&b.position.lo));
        assert_eq!(crossings.len(), 4);
        for (event, root) in crossings.iter().zip(&roots[..4]) {
            assert!(event.position.lo <= *root && *root <= event.position.hi);
            assert!(event.position.hi - event.position.lo <= ROOT_WIDTH_PX);
        }
        assert_eq!(
            crossings
                .iter()
                .map(|event| event.delta)
                .collect::<Vec<_>>(),
            [-1, 1, -1, 1]
        );
    }

    #[test]
    fn crossing_budget_refuses_and_adequate_work_bounds_a_non_dyadic_root() {
        let across = Poly::line(-1., 2.);
        let along = Poly::line(0., 1000.);
        let error = locate(across, along, 0, &mut Budget { remaining: 2 }).unwrap_err();
        assert_eq!(error.code, RefusalCode::BudgetExceeded);
        let result = locate(
            across,
            along,
            0,
            &mut Budget {
                remaining: MAX_WORK,
            },
        )
        .unwrap()
        .unwrap();
        assert!(result.lo <= 1000. / 3. && result.hi >= 1000. / 3.);
        assert!(result.hi - result.lo <= ROOT_WIDTH_PX);
    }

    #[test]
    fn unresolved_crossing_order_cannot_hide_a_thin_foreground_component() {
        let event = |ordinal, lo, hi, delta| Event {
            identity: Identity::Interior { edge: 0, ordinal },
            position: Bounds { lo, hi },
            delta,
        };
        let ambiguous = vec![
            event(0, 1., 1. + 5e-10, 1),
            event(1, 1. + 2e-10, 1. + 7e-10, -1),
        ];
        assert_eq!(
            sweep(ambiguous).unwrap_err().code,
            RefusalCode::NotConverged
        );
        let covered = vec![
            event(0, 0., 0., 2),
            event(1, 1., 1. + 5e-10, 1),
            event(2, 1. + 2e-10, 1. + 7e-10, -1),
            event(3, 2., 2., -2),
        ];
        assert_eq!(sweep(covered).unwrap(), (vec![[0., 2.]], 0.));
    }
}
