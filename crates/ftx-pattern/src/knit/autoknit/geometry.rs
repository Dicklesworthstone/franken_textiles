//! Bounded annular geometry for AutoKnit. Flips use the intrinsic triangle metric;
//! a new diagonal is the length in the unfolded pair, never its spatial chord.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::intersection::{indexed_triangle_pair, validate_mesh};
use ftx_core::shaping::EaseShellMesh;
use ftx_core::sparse::SparseCoo;
use ftx_core::{Refusal, RefusalCode, Vec2, Vec3, det};

type Edge = (usize, usize);
type Incidence = BTreeMap<Edge, Vec<(usize, usize)>>;

const MAX_VERTICES: usize = 20_000;
const MAX_TRIANGLES: usize = 40_000;
const MAX_INTERSECTION_CANDIDATES: usize = 2_000_000;
const MAX_CONTOUR_FACE_VISITS: usize = 50_000_000;
const COTANGENT_ROUNDOFF: f64 = 1e-10;

fn edge(a: usize, b: usize) -> Edge {
    if a < b { (a, b) } else { (b, a) }
}

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message)
}

/// The coarea coordinate measures the contour-length-weighted mean normal
/// separation of contours. It is not a shortest geodesic or a fit prediction.
pub(super) struct AnnularField {
    pub flips: usize,
    pub min_weight: f64,
    pub height_m: f64,
    bands: Vec<CoareaBand>,
}

#[derive(Debug)]
struct CoareaBand {
    t0: f64,
    t1: f64,
    p0: f64,
    p1: f64,
    d0: f64,
    d1: f64,
    start_m: f64,
    height_m: f64,
}

impl CoareaBand {
    // Integral of (d0 + x*(d1-d0))/(p0 + x*(p1-p0)) for x in [0,f].
    // d is d(area)/dt, p is contour length. Both are linear within this band.
    fn distance(&self, f: f64) -> f64 {
        let r = f * (self.p1 - self.p0) / self.p0;
        let (a, b) = if r.abs() < 1e-4 {
            // Stable analytic limit at constant perimeter; no division by a
            // nearly zero perimeter difference and no geometry modification.
            (
                1.0 - r / 2.0 + r * r / 3.0 - r.powi(3) / 4.0 + r.powi(4) / 5.0,
                0.5 - r / 3.0 + r * r / 4.0 - r.powi(3) / 5.0 + r.powi(4) / 6.0,
            )
        } else {
            let log = det::ln(1.0 + r);
            (log / r, (r - log) / (r * r))
        };
        (self.t1 - self.t0) * f / self.p0 * (self.d0 * a + f * (self.d1 - self.d0) * b)
    }
}

impl AnnularField {
    pub(super) fn build(shell: &EaseShellMesh, max_flips: usize) -> Result<Self, Refusal> {
        let mut metric = admit_annulus(shell)?;
        let flips = metric.make_delaunay(max_flips)?;
        let mut fixed = vec![None; shell.vertices_m.len()];
        for ring in &shell.boundary_rings {
            for &v in &ring.vertices {
                fixed[v as usize] = Some(if ring.is_start { 0.0 } else { 1.0 });
            }
        }
        let (time, min_weight) = metric.harmonic_time(&fixed)?;
        let mut faces = Vec::with_capacity(metric.triangles.len());
        for &triangle in &metric.triangles {
            faces.push(SliceFace::new(triangle, &metric, &time)?);
        }

        // Sweeping all vertex levels checks every possible contour topology,
        // including small split/merge events between the eventual knit courses.
        let mut events = Vec::with_capacity(faces.len() * 3);
        for (i, face) in faces.iter().enumerate() {
            events.push((face.times[0], i, 1_i8));
            events.push((face.times[1], i, 0_i8));
            events.push((face.times[2], i, -1_i8));
        }
        events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut active = BTreeSet::new();
        let mut cursor = 0;
        let mut visits = 0usize;
        let mut bands = Vec::new();
        let mut height_m = 0.0;
        while cursor < events.len() {
            let t0 = events[cursor].0;
            while cursor < events.len() && events[cursor].0 == t0 {
                let (_, face, action) = events[cursor];
                if action == 1 {
                    active.insert(face);
                } else if action == -1 {
                    active.remove(&face);
                }
                cursor += 1;
            }
            if cursor == events.len() {
                break;
            }
            let t1 = events[cursor].0;
            visits = visits.checked_add(active.len()).ok_or_else(|| {
                Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "AutoKnit contour work overflow",
                )
            })?;
            if visits > MAX_CONTOUR_FACE_VISITS {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "AutoKnit contour topology/metric sweep exceeds its bounded face work",
                )
                .repair("supply a smaller annular mesh while preserving its measured boundaries"));
            }
            let mut links: BTreeMap<Edge, Vec<Edge>> = BTreeMap::new();
            let (mut p0, mut p1, mut d0, mut d1) = (0.0, 0.0, 0.0, 0.0);
            for &i in &active {
                let face = &faces[i];
                let mut crossings = Vec::with_capacity(2);
                for (a, b) in [
                    (face.vertices[0], face.vertices[1]),
                    (face.vertices[1], face.vertices[2]),
                    (face.vertices[2], face.vertices[0]),
                ] {
                    if (time[a] <= t0 && time[b] >= t1) || (time[b] <= t0 && time[a] >= t1) {
                        crossings.push(edge(a, b));
                    }
                }
                if crossings.len() != 2 {
                    return Err(bad("harmonic shell has a degenerate iso-contour face"));
                }
                links.entry(crossings[0]).or_default().push(crossings[1]);
                links.entry(crossings[1]).or_default().push(crossings[0]);
                let l0 = face.length(t0);
                let l1 = face.length(t1);
                p0 += l0;
                p1 += l1;
                d0 += l0 / face.gradient;
                d1 += l1 / face.gradient;
            }
            require_one_cycle(&links)?;
            if [p0, p1, d0, d1].iter().any(|v| !v.is_finite() || *v <= 0.0) {
                return Err(bad(
                    "harmonic shell has an unmeasurable contour or normal span",
                ));
            }
            let mut band = CoareaBand {
                t0,
                t1,
                p0,
                p1,
                d0,
                d1,
                start_m: height_m,
                height_m: 0.0,
            };
            band.height_m = band.distance(1.0);
            if !band.height_m.is_finite() || band.height_m <= 0.0 {
                return Err(bad(
                    "harmonic shell normal-span integration is not positive and finite",
                ));
            }
            height_m += band.height_m;
            bands.push(band);
        }
        let first = bands
            .first()
            .ok_or_else(|| bad("shell has no regular harmonic bands"))?;
        let last = bands.last().expect("nonempty bands");
        if first.t0 != 0.0 || last.t1 != 1.0 || !height_m.is_finite() {
            return Err(bad(
                "harmonic contours do not cover both declared boundaries",
            ));
        }
        for ring in &shell.boundary_rings {
            let actual = if ring.is_start { first.p0 } else { last.p1 };
            if (actual - ring.perimeter_m).abs() > 1e-8 * ring.perimeter_m.max(1.0) {
                return Err(bad(
                    "intrinsic contour does not preserve its source boundary perimeter",
                ));
            }
        }
        Ok(Self {
            flips,
            min_weight,
            height_m,
            bands,
        })
    }

    /// Returns harmonic time and metric contour length at a fraction of normal span.
    pub(super) fn contour(&self, fraction: f64) -> (f64, f64) {
        if fraction == 0.0 {
            let first = &self.bands[0];
            return (0.0, first.p0);
        }
        if fraction == 1.0 {
            let last = self.bands.last().expect("nonempty bands");
            return (1.0, last.p1);
        }
        let target = fraction * self.height_m;
        let index = self
            .bands
            .partition_point(|b| b.start_m + b.height_m < target);
        let band = &self.bands[index];
        let distance = target - band.start_m;
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..56 {
            let mid = 0.5 * (lo + hi);
            if band.distance(mid) < distance {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let f = 0.5 * (lo + hi);
        (
            band.t0 + f * (band.t1 - band.t0),
            band.p0 + f * (band.p1 - band.p0),
        )
    }
}

fn require_one_cycle(links: &BTreeMap<Edge, Vec<Edge>>) -> Result<(), Refusal> {
    if links.is_empty() || links.values().any(|neighbors| neighbors.len() != 2) {
        return Err(bad("AutoKnit iso-contour is open or branching"));
    }
    let mut reached = BTreeSet::new();
    let mut pending = vec![*links.first_key_value().expect("nonempty links").0];
    while let Some(v) = pending.pop() {
        if reached.insert(v) {
            pending.extend(&links[&v]);
        }
    }
    if reached.len() != links.len() {
        return Err(bad(
            "AutoKnit iso-contour has multiple components; branch synthesis is unsupported",
        )
        .repair("use separate annular shells or the explicit branched pullover drafter"));
    }
    Ok(())
}

struct SliceFace {
    vertices: [usize; 3],
    times: [f64; 3],
    middle_length: f64,
    gradient: f64,
}

impl SliceFace {
    fn new(mut vertices: [usize; 3], metric: &MetricMesh, time: &[f64]) -> Result<Self, Refusal> {
        vertices.sort_by(|a, b| time[*a].total_cmp(&time[*b]).then(a.cmp(b)));
        let times = vertices.map(|v| time[v]);
        let [p0, p1, p2] = metric.points(vertices)?;
        if times[0] == times[2] {
            return Err(bad(
                "harmonic time is constant on a face; its surface area cannot be sliced",
            ));
        }
        let gx = (times[1] - times[0]) / p1.x;
        let gy = (times[2] - times[0] - gx * p2.x) / p2.y;
        let gradient = det::sqrt(gx * gx + gy * gy);
        let f = (times[1] - times[0]) / (times[2] - times[0]);
        let middle_length = (p1 - (p0 + (p2 - p0) * f)).norm();
        if !gradient.is_finite()
            || gradient <= 0.0
            || !middle_length.is_finite()
            || middle_length <= 0.0
        {
            return Err(bad("harmonic face has a singular metric gradient"));
        }
        Ok(Self {
            vertices,
            times,
            middle_length,
            gradient,
        })
    }

    fn length(&self, t: f64) -> f64 {
        if t <= self.times[1] && self.times[1] > self.times[0] {
            self.middle_length * ((t - self.times[0]) / (self.times[1] - self.times[0]))
        } else {
            self.middle_length * ((self.times[2] - t) / (self.times[2] - self.times[1]))
        }
    }
}

struct MetricMesh {
    triangles: Vec<[usize; 3]>,
    lengths: BTreeMap<Edge, f64>,
    incidence: Incidence,
}

impl MetricMesh {
    fn points(&self, [a, b, c]: [usize; 3]) -> Result<[Vec2; 3], Refusal> {
        let ab = self.lengths[&edge(a, b)];
        let ac = self.lengths[&edge(a, c)];
        let bc = self.lengths[&edge(b, c)];
        let x = (ab * ab + ac * ac - bc * bc) / (2.0 * ab);
        let y2 = ac * ac - x * x;
        if !x.is_finite() || !y2.is_finite() || y2 <= 0.0 {
            return Err(bad(
                "intrinsic triangle metric is degenerate or numerically unresolved",
            ));
        }
        Ok([
            Vec2::new(0.0, 0.0),
            Vec2::new(ab, 0.0),
            Vec2::new(x, det::sqrt(y2)),
        ])
    }

    fn opposite_cotangent(&self, u: usize, v: usize, opposite: usize) -> Result<f64, Refusal> {
        let [a, b, c] = self.points([u, v, opposite])?;
        Ok((a - c).dot(b - c) / (b.x * c.y))
    }

    fn make_delaunay(&mut self, max_flips: usize) -> Result<usize, Refusal> {
        let mut pending: BTreeSet<Edge> = self.incidence.keys().copied().collect();
        let mut flips = 0;
        while let Some((u, v)) = pending.pop_first() {
            let Some(incident) = self.incidence.get(&(u, v)) else {
                continue;
            };
            if incident.len() != 2 {
                continue;
            }
            let (t0, o0) = incident[0];
            let (t1, o1) = incident[1];
            let weight = self.opposite_cotangent(u, v, o0)? + self.opposite_cotangent(u, v, o1)?;
            if weight >= -COTANGENT_ROUNDOFF {
                continue;
            }
            if flips == max_flips {
                return Err(Refusal::new(
                    RefusalCode::NotConverged,
                    "AutoKnit intrinsic Delaunay flip budget exhausted before the metric converged",
                )
                .repair("increase max_idt_flips or supply a better conditioned annular mesh"));
            }
            if o0 == o1 || self.incidence.contains_key(&edge(o0, o1)) {
                return Err(bad(
                    "non-Delaunay metric requires an unsupported non-simplicial edge flip",
                )
                .repair("remesh the annulus without changing its measured boundaries"));
            }
            let p = self.points([u, v, o0])?[2];
            let q = self.points([u, v, o1])?[2];
            let crossing = (p.x * q.y + q.x * p.y) / (p.y + q.y);
            if crossing <= 0.0 || crossing >= self.lengths[&(u, v)] {
                return Err(bad(
                    "intrinsic edge flip would leave its unfolded triangle pair",
                ));
            }
            let diagonal = det::sqrt((p.x - q.x).powi(2) + (p.y + q.y).powi(2));
            let original = self.triangles[t0];
            let forward = (0..3).any(|i| original[i] == u && original[(i + 1) % 3] == v);
            for index in [t0, t1] {
                let [a, b, c] = self.triangles[index];
                for key in [edge(a, b), edge(b, c), edge(c, a)] {
                    let incident = self.incidence.get_mut(&key).expect("existing face edge");
                    incident.retain(|(triangle, _)| *triangle != index);
                    if incident.is_empty() {
                        self.incidence.remove(&key);
                    }
                }
            }
            self.triangles[t0] = if forward { [u, o1, o0] } else { [u, o0, o1] };
            self.triangles[t1] = if forward { [v, o0, o1] } else { [v, o1, o0] };
            self.lengths.remove(&(u, v));
            self.lengths.insert(edge(o0, o1), diagonal);
            for index in [t0, t1] {
                let [a, b, c] = self.triangles[index];
                self.points([a, b, c])?;
                for (x, y, opposite) in [(a, b, c), (b, c, a), (c, a, b)] {
                    let key = edge(x, y);
                    let incident = self.incidence.entry(key).or_default();
                    incident.push((index, opposite));
                    incident.sort_unstable();
                    pending.insert(key);
                }
            }
            flips += 1;
        }
        Ok(flips)
    }

    fn harmonic_time(&self, fixed: &[Option<f64>]) -> Result<(Vec<f64>, f64), Refusal> {
        let mut variables = vec![None; fixed.len()];
        let mut n = 0;
        for (v, value) in fixed.iter().enumerate() {
            if value.is_none() {
                variables[v] = Some(n);
                n += 1;
            }
        }
        let mut time: Vec<f64> = fixed.iter().map(|v| v.unwrap_or(0.0)).collect();
        let mut matrix = SparseCoo::new(n, n);
        let mut rhs = vec![0.0; n];
        let mut min_weight = f64::INFINITY;
        for (&(a, b), incident) in &self.incidence {
            if variables[a].is_none() && variables[b].is_none() {
                continue;
            }
            let mut weight = 0.0;
            for &(_, opposite) in incident {
                weight += 0.5 * self.opposite_cotangent(a, b, opposite)?;
            }
            min_weight = min_weight.min(weight);
            if !weight.is_finite() || weight < -COTANGENT_ROUNDOFF {
                return Err(bad(
                    "intrinsic cotangent Laplacian still has a negative free-edge weight",
                ));
            }
            // Keep the actual metric weights, including signed floating-point
            // roundoff. No max(epsilon) changes the variational problem.
            match (variables[a], variables[b]) {
                (Some(i), Some(j)) => {
                    matrix.push(i, i, weight);
                    matrix.push(j, j, weight);
                    matrix.push(i, j, -weight);
                    matrix.push(j, i, -weight);
                }
                (Some(i), None) => {
                    matrix.push(i, i, weight);
                    rhs[i] += weight * time[b];
                }
                (None, Some(j)) => {
                    matrix.push(j, j, weight);
                    rhs[j] += weight * time[a];
                }
                (None, None) => {}
            }
        }
        if n > 0 {
            let solution = matrix.assemble().solve_spd(&rhs, 1e-11, 6_000)?;
            for (v, variable) in variables.iter().enumerate() {
                if let Some(i) = variable {
                    let value = solution[*i];
                    if !value.is_finite() || value <= 0.0 || value >= 1.0 {
                        return Err(bad(
                            "harmonic solution violates the strict interior maximum principle",
                        ));
                    }
                    time[v] = value;
                }
            }
        }
        Ok((time, if n == 0 { 0.0 } else { min_weight }))
    }
}

fn admit_annulus(shell: &EaseShellMesh) -> Result<MetricMesh, Refusal> {
    if shell.vertices_m.len() > MAX_VERTICES || shell.triangles.len() > MAX_TRIANGLES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "AutoKnit supports at most {MAX_VERTICES} vertices and {MAX_TRIANGLES} triangles"
            ),
        ));
    }
    if shell.vertices_m.len() < 6 || shell.triangles.len() < 4 {
        return Err(bad(
            "AutoKnit shell has too few vertices or faces for an annulus",
        ));
    }
    if shell.boundary_rings.len() != 2
        || shell.boundary_rings.iter().filter(|r| r.is_start).count() != 1
    {
        return Err(bad("AutoKnit currently requires one start and one end boundary on a connected annulus; multi-ring branch synthesis is unsupported")
            .repair("supply a sleeve/tube annulus or use the explicit branched pullover drafter"));
    }
    if shell.source_body_sha256.trim().is_empty() {
        return Err(Refusal::new(
            RefusalCode::InvalidDocument,
            "AutoKnit shell requires its source body identity",
        ));
    }
    let vertices: Vec<Vec3> = shell
        .vertices_m
        .iter()
        .map(|&[x, y, z]| Vec3::new(x, y, z))
        .collect();
    let triangles: Vec<[usize; 3]> = shell
        .triangles
        .iter()
        .map(|t| t.map(|v| v as usize))
        .collect();
    validate_mesh(&vertices, &triangles)?;
    let mut incidence: Incidence = BTreeMap::new();
    let mut directed = BTreeSet::new();
    let mut lengths = BTreeMap::new();
    let mut links = vec![Vec::new(); vertices.len()];
    for (i, &[a, b, c]) in triangles.iter().enumerate() {
        links[a].push((b, c));
        links[b].push((c, a));
        links[c].push((a, b));
        for (u, v, opposite) in [(a, b, c), (b, c, a), (c, a, b)] {
            if !directed.insert((u, v)) {
                return Err(bad(
                    "AutoKnit shell has inconsistent winding or non-manifold edges",
                ));
            }
            let key = edge(u, v);
            incidence.entry(key).or_default().push((i, opposite));
            lengths
                .entry(key)
                .or_insert_with(|| (vertices[u] - vertices[v]).norm());
        }
    }
    if incidence.values().any(|faces| faces.len() > 2) {
        return Err(bad("AutoKnit shell has a non-manifold edge"));
    }
    let actual_boundary: BTreeSet<Edge> = directed
        .iter()
        .copied()
        .filter(|&(u, v)| incidence[&edge(u, v)].len() == 1)
        .collect();
    let mut declared = BTreeSet::new();
    let mut boundary_vertices = BTreeSet::new();
    let mut names = BTreeSet::new();
    for ring in &shell.boundary_rings {
        if ring.name.trim().is_empty() || !names.insert(&ring.name) || ring.vertices.len() < 3 {
            return Err(bad(
                "AutoKnit boundary rings require unique names and at least three vertices",
            ));
        }
        let mut perimeter = 0.0;
        for i in 0..ring.vertices.len() {
            let u = ring.vertices[i] as usize;
            let v = ring.vertices[(i + 1) % ring.vertices.len()] as usize;
            if u >= vertices.len() || v >= vertices.len() || !boundary_vertices.insert(u) {
                return Err(bad(
                    "AutoKnit boundary has an out-of-range, repeated, or shared vertex",
                ));
            }
            if !actual_boundary.contains(&(u, v)) || !declared.insert((u, v)) {
                return Err(bad(
                    "AutoKnit boundary rings must follow the actual oriented boundary edges",
                ));
            }
            perimeter += lengths[&edge(u, v)];
        }
        if !ring.perimeter_m.is_finite()
            || ring.perimeter_m <= 0.0
            || (perimeter - ring.perimeter_m).abs() > 1e-8 * perimeter.max(1.0)
        {
            return Err(bad(
                "declared AutoKnit boundary perimeter disagrees with its mesh edges",
            ));
        }
    }
    if declared != actual_boundary {
        return Err(bad(
            "AutoKnit rings do not cover every source boundary edge",
        ));
    }
    if vertices.len() as i64 - incidence.len() as i64 + triangles.len() as i64 != 0 {
        return Err(bad("AutoKnit shell is not a genus-zero annulus"));
    }
    let mut adjacency = vec![Vec::new(); vertices.len()];
    for &(a, b) in incidence.keys() {
        adjacency[a].push(b);
        adjacency[b].push(a);
    }
    let mut reached = BTreeSet::new();
    let mut pending = vec![0];
    while let Some(v) = pending.pop() {
        if reached.insert(v) {
            pending.extend(&adjacency[v]);
        }
    }
    if reached.len() != vertices.len() {
        return Err(bad("AutoKnit shell has disconnected or unused vertices"));
    }
    for (v, local_edges) in links.iter().enumerate() {
        let mut neighbors: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &(a, b) in local_edges {
            neighbors.entry(a).or_default().push(b);
            neighbors.entry(b).or_default().push(a);
        }
        let ends = neighbors.values().filter(|list| list.len() == 1).count();
        let expected = if boundary_vertices.contains(&v) { 2 } else { 0 };
        if neighbors.is_empty() || ends != expected || neighbors.values().any(|list| list.len() > 2)
        {
            return Err(bad("AutoKnit shell has a non-manifold vertex link"));
        }
        let mut local = BTreeSet::new();
        let mut pending = vec![*neighbors.first_key_value().expect("nonempty vertex link").0];
        while let Some(n) = pending.pop() {
            if local.insert(n) {
                pending.extend(&neighbors[&n]);
            }
        }
        if local.len() != neighbors.len() {
            return Err(bad("AutoKnit shell has pinched vertex fans"));
        }
    }
    audit_intersections(&vertices, &triangles)?;
    let metric = MetricMesh {
        triangles,
        lengths,
        incidence,
    };
    for &triangle in &metric.triangles {
        metric.points(triangle)?;
    }
    Ok(metric)
}

fn audit_intersections(vertices: &[Vec3], triangles: &[[usize; 3]]) -> Result<(), Refusal> {
    let bounds: Vec<([f64; 3], [f64; 3])> = triangles
        .iter()
        .map(|triangle| {
            let points = triangle.map(|v| [vertices[v].x, vertices[v].y, vertices[v].z]);
            let lo = std::array::from_fn(|axis| {
                points.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min)
            });
            let hi = std::array::from_fn(|axis| {
                points
                    .iter()
                    .map(|p| p[axis])
                    .fold(f64::NEG_INFINITY, f64::max)
            });
            (lo, hi)
        })
        .collect();
    let mut order: Vec<usize> = (0..triangles.len()).collect();
    order.sort_by(|&a, &b| bounds[a].0[0].total_cmp(&bounds[b].0[0]).then(a.cmp(&b)));
    let mut active: Vec<usize> = Vec::new();
    let mut candidates = 0;
    for i in order {
        active.retain(|&j| bounds[j].1[0] >= bounds[i].0[0]);
        for &j in &active {
            candidates += 1;
            if candidates > MAX_INTERSECTION_CANDIDATES {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "AutoKnit exact source-shell intersection audit exceeded its candidate budget",
                )
                .repair("supply a smaller annular mesh"));
            }
            if (1..3).any(|axis| {
                bounds[i].1[axis] < bounds[j].0[axis] || bounds[j].1[axis] < bounds[i].0[axis]
            }) {
                continue;
            }
            if indexed_triangle_pair(vertices, triangles[i], triangles[j]).is_some() {
                return Err(bad(format!(
                    "AutoKnit source triangles {i} and {j} intersect beyond their shared mesh feature"
                )));
            }
        }
        active.push(i);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intrinsic_flip_uses_unfolded_length_not_the_spatial_chord() {
        let vertices = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.3, 0.3, 0.0),
            Vec3::new(0.8, 0.0, 0.1),
        ];
        let triangles = vec![[0, 1, 2], [1, 0, 3]];
        let mut lengths = BTreeMap::new();
        let mut incidence: Incidence = BTreeMap::new();
        for (i, &[a, b, c]) in triangles.iter().enumerate() {
            for (u, v, opposite) in [(a, b, c), (b, c, a), (c, a, b)] {
                lengths.insert(edge(u, v), (vertices[u] - vertices[v]).norm());
                incidence.entry(edge(u, v)).or_default().push((i, opposite));
            }
        }
        let mut mesh = MetricMesh {
            triangles,
            lengths,
            incidence,
        };
        assert_eq!(mesh.make_delaunay(1).unwrap(), 1);
        let expected = det::sqrt(0.5_f64.powi(2) + 0.4_f64.powi(2));
        assert!((mesh.lengths[&(2, 3)] - expected).abs() < 1e-12);
        assert!((mesh.lengths[&(2, 3)] - (vertices[2] - vertices[3]).norm()).abs() > 0.04);
        let area: f64 = mesh
            .triangles
            .iter()
            .map(|&triangle| {
                let p = mesh.points(triangle).unwrap();
                0.5 * p[1].x * p[2].y
            })
            .sum();
        assert!((area - 0.2).abs() < 1e-12);
    }
}
