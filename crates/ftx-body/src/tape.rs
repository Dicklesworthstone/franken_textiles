//! Measuring a body the way a tape does (plan §9.3): a girth is the perimeter of the convex hull of the
//! horizontal section that surrounds the part's axis, because a tape bridges hollows (between the
//! shoulder blades, the small of the back, the gluteal cleft).
//!
//! The functions take any unit and answer in it. A vertex exactly at the cutting height counts as above
//! it (simulation of simplicity), so on a closed mesh every section is a set of closed loops.

use std::collections::BTreeMap;

use ftx_core::Vec3;

/// The closed loops where the horizontal plane at height `z` cuts the mesh, as (x, y) polygons. A loop
/// that does not close (an open mesh) is left out.
#[must_use]
pub fn section_loops(vertices: &[Vec3], triangles: &[[usize; 3]], z: f64) -> Vec<Vec<[f64; 2]>> {
    loops_of(vertices, triangles, 0..triangles.len(), z)
}

/// Horizontal cuts of one mesh at many heights: triangles bucketed by height, so a cut visits only the
/// triangles near it. Its cuts equal [`section_loops`] (same triangles, same order).
pub struct Slicer<'a> {
    vertices: &'a [Vec3],
    triangles: &'a [[usize; 3]],
    z0: f64,
    width: f64,
    buckets: Vec<Vec<usize>>,
}

impl<'a> Slicer<'a> {
    /// Buckets `triangles` by height in slabs of `width`.
    #[must_use]
    pub fn new(vertices: &'a [Vec3], triangles: &'a [[usize; 3]], width: f64) -> Self {
        let (z0, z1) = vertices
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v.z), b.max(v.z))
            });
        let slab = |z: f64| ((z - z0) / width).floor() as usize;
        let mut buckets = vec![Vec::new(); if z1 >= z0 { slab(z1) + 1 } else { 0 }];
        for (i, t) in triangles.iter().enumerate() {
            let (lo, hi) = t
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| {
                    (a.min(vertices[v].z), b.max(vertices[v].z))
                });
            for b in &mut buckets[slab(lo)..=slab(hi)] {
                b.push(i);
            }
        }
        Self {
            vertices,
            triangles,
            z0,
            width,
            buckets,
        }
    }

    /// [`section_loops`] at height `z`.
    #[must_use]
    pub fn loops(&self, z: f64) -> Vec<Vec<[f64; 2]>> {
        let b = (z - self.z0) / self.width;
        if !(b >= 0.0 && (b as usize) < self.buckets.len()) {
            return Vec::new();
        }
        loops_of(
            self.vertices,
            self.triangles,
            self.buckets[b as usize].iter().copied(),
            z,
        )
    }

    /// [`girth`] at height `z`.
    #[must_use]
    pub fn girth(&self, z: f64, around: [f64; 2]) -> Option<f64> {
        girth_of(&self.loops(z), around)
    }

    /// [`girth_near`] at height `z`.
    #[must_use]
    pub fn girth_near(&self, z: f64, around: [f64; 2]) -> Option<f64> {
        near_of(&self.loops(z), around)
    }

    /// [`back_point`] at height `z`.
    #[must_use]
    pub fn back_point(&self, z: f64, x0: f64) -> Option<f64> {
        back_of(&self.loops(z), x0)
    }
}

fn loops_of(
    vertices: &[Vec3],
    triangles: &[[usize; 3]],
    faces: impl IntoIterator<Item = usize>,
    z: f64,
) -> Vec<Vec<[f64; 2]>> {
    let above = |i: usize| vertices[i].z >= z;
    // Each cut triangle gives one segment between two cut edges (an edge is keyed by its sorted ends).
    let mut segments: Vec<[(usize, usize); 2]> = Vec::new();
    for t in faces.into_iter().map(|f| &triangles[f]) {
        let mut cut = [(0, 0); 2];
        let mut n = 0;
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if above(a) != above(b) {
                if n < 2 {
                    cut[n] = (a.min(b), a.max(b));
                }
                n += 1;
            }
        }
        if n == 2 {
            segments.push(cut);
        }
    }
    let mut at_edge: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    for (s, seg) in segments.iter().enumerate() {
        for e in seg {
            at_edge.entry(*e).or_default().push(s);
        }
    }
    let point = |(a, b): (usize, usize)| -> [f64; 2] {
        let (pa, pb) = (vertices[a], vertices[b]);
        let t = (z - pa.z) / (pb.z - pa.z);
        [pa.x + t * (pb.x - pa.x), pa.y + t * (pb.y - pa.y)]
    };
    let mut used = vec![false; segments.len()];
    let mut loops = Vec::new();
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let first = segments[start][0];
        let mut edge = segments[start][1];
        let mut poly = vec![point(first)];
        let closed = loop {
            poly.push(point(edge));
            let Some(s) = at_edge[&edge].iter().copied().find(|&s| !used[s]) else {
                break false;
            };
            used[s] = true;
            let seg = segments[s];
            edge = if seg[0] == edge { seg[1] } else { seg[0] };
            if edge == first {
                break true;
            }
        };
        if closed && poly.len() >= 3 {
            loops.push(poly);
        }
    }
    loops
}

/// Signed area of a polygon (counter-clockwise positive).
#[must_use]
pub fn polygon_area(poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    0.5 * (0..n)
        .map(|i| {
            let (p, q) = (poly[i], poly[(i + 1) % n]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f64>()
}

/// Whether `p` lies inside the polygon (crossing number).
#[must_use]
pub fn contains(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Perimeter of the convex hull of `points` (monotone chain).
#[must_use]
pub fn hull_perimeter(points: &[[f64; 2]]) -> f64 {
    let mut p = points.to_vec();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 2 {
        return 0.0;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let chain = |pts: &mut dyn Iterator<Item = [f64; 2]>| {
        let mut c: Vec<[f64; 2]> = Vec::new();
        for q in pts {
            while c.len() >= 2 && cross(c[c.len() - 2], c[c.len() - 1], q) <= 0.0 {
                c.pop();
            }
            c.push(q);
        }
        c.pop();
        c
    };
    let mut hull = chain(&mut p.iter().copied());
    hull.extend(chain(&mut p.iter().rev().copied()));
    let n = hull.len();
    (0..n)
        .map(|i| {
            let (a, b) = (hull[i], hull[(i + 1) % n]);
            (b[0] - a[0]).hypot(b[1] - a[1])
        })
        .sum()
}

/// The vertices in a frame's coordinates: (·lat, ·front, ·up) relative to `origin`, so a section
/// perpendicular to the frame's `up` axis is a horizontal [`section_loops`] cut.
#[must_use]
pub fn in_frame(vertices: &[Vec3], origin: Vec3, lat: Vec3, front: Vec3, up: Vec3) -> Vec<Vec3> {
    vertices
        .iter()
        .map(|v| {
            let d = *v - origin;
            Vec3::new(d.dot(lat), d.dot(front), d.dot(up))
        })
        .collect()
}

/// The largest y at which the section at height `z` crosses the plane x = `x0` (the back-most point
/// of the body's median line when +y is the back), or `None` when nothing crosses it.
#[must_use]
pub fn back_point(vertices: &[Vec3], triangles: &[[usize; 3]], z: f64, x0: f64) -> Option<f64> {
    back_of(&section_loops(vertices, triangles, z), x0)
}

fn back_of(loops: &[Vec<[f64; 2]>], x0: f64) -> Option<f64> {
    loops
        .iter()
        .flat_map(|l| {
            let n = l.len();
            (0..n).filter_map(move |i| {
                let (a, b) = (l[i], l[(i + 1) % n]);
                ((a[0] >= x0) != (b[0] >= x0))
                    .then(|| a[1] + (x0 - a[0]) / (b[0] - a[0]) * (b[1] - a[1]))
            })
        })
        .fold(None, |m: Option<f64>, y| Some(m.map_or(y, |m| m.max(y))))
}

/// The tape girth at height `z` around the axis point `around` (x, y): the hull perimeter of the
/// outermost section loop that contains it, or `None` when no loop does.
#[must_use]
pub fn girth(vertices: &[Vec3], triangles: &[[usize; 3]], z: f64, around: [f64; 2]) -> Option<f64> {
    girth_of(&section_loops(vertices, triangles, z), around)
}

fn outermost_around(loops: &[Vec<[f64; 2]>], around: [f64; 2]) -> Option<&Vec<[f64; 2]>> {
    loops
        .iter()
        .filter(|l| contains(l, around))
        .max_by(|a, b| polygon_area(a).abs().total_cmp(&polygon_area(b).abs()))
}

fn girth_of(loops: &[Vec<[f64; 2]>], around: [f64; 2]) -> Option<f64> {
    outermost_around(loops, around).map(|l| hull_perimeter(l))
}

/// As [`girth`], but when no loop contains `around` (an axis that passes beside a narrow limb), the loop
/// whose vertex mean is nearest to it.
#[must_use]
pub fn girth_near(
    vertices: &[Vec3],
    triangles: &[[usize; 3]],
    z: f64,
    around: [f64; 2],
) -> Option<f64> {
    near_of(&section_loops(vertices, triangles, z), around)
}

fn near_of(loops: &[Vec<[f64; 2]>], around: [f64; 2]) -> Option<f64> {
    let chosen = outermost_around(loops, around).or_else(|| {
        let dist = |l: &Vec<[f64; 2]>| {
            let n = l.len() as f64;
            let (x, y) = l
                .iter()
                .fold((0.0, 0.0), |(x, y), p| (x + p[0] / n, y + p[1] / n));
            (x - around[0]).hypot(y - around[1])
        };
        loops.iter().min_by(|a, b| dist(a).total_cmp(&dist(b)))
    })?;
    Some(hull_perimeter(chosen))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed prism over the polygon `base` (counter-clockwise) from z = 0 to z = 1, outward-wound.
    fn prism(base: &[[f64; 2]]) -> (Vec<Vec3>, Vec<[usize; 3]>) {
        let n = base.len();
        let mut v: Vec<Vec3> = base.iter().map(|p| Vec3::new(p[0], p[1], 0.0)).collect();
        v.extend(base.iter().map(|p| Vec3::new(p[0], p[1], 1.0)));
        let (c0, c1) = (v.len(), v.len() + 1);
        let cx = base.iter().map(|p| p[0]).sum::<f64>() / n as f64;
        let cy = base.iter().map(|p| p[1]).sum::<f64>() / n as f64;
        v.push(Vec3::new(cx, cy, 0.0));
        v.push(Vec3::new(cx, cy, 1.0));
        let mut t = Vec::new();
        for i in 0..n {
            let j = (i + 1) % n;
            t.push([i, j, n + j]);
            t.push([i, n + j, n + i]);
            t.push([c0, j, i]);
            t.push([c1, n + i, n + j]);
        }
        (v, t)
    }

    #[test]
    fn a_square_prism_measures_its_perimeter() {
        let (v, t) = prism(&[[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]]);
        let loops = section_loops(&v, &t, 0.5);
        assert_eq!(loops.len(), 1);
        assert!((polygon_area(&loops[0]).abs() - 2.0).abs() < 1e-12);
        assert!((girth(&v, &t, 0.5, [1.0, 0.5]).unwrap() - 6.0).abs() < 1e-12);
        assert_eq!(girth(&v, &t, 0.5, [5.0, 0.5]), None);
        assert!(section_loops(&v, &t, 1.5).is_empty());
    }

    #[test]
    fn a_cut_through_vertices_still_closes() {
        // z = 0 is exactly the base: those vertices count as above, so the cut is just above the base
        let (v, t) = prism(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        assert!(section_loops(&v, &t, 0.0).is_empty());
        assert!((girth(&v, &t, 1e-9, [0.5, 0.5]).unwrap() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn the_tape_bridges_a_hollow() {
        // a star: the hull is the outer pentagon, far shorter than the star's own outline
        let star: Vec<[f64; 2]> = (0..10)
            .map(|i| {
                let a = std::f64::consts::PI * f64::from(i) / 5.0;
                let r = if i % 2 == 0 { 1.0 } else { 0.4 };
                [r * a.cos(), r * a.sin()]
            })
            .collect();
        let (v, t) = prism(&star);
        let g = girth(&v, &t, 0.25, [0.0, 0.0]).unwrap();
        let pentagon = 5.0 * 2.0 * (std::f64::consts::PI / 5.0).sin();
        assert!((g - pentagon).abs() < 1e-12, "{g} vs {pentagon}");
        let outline: f64 = (0..10)
            .map(|i| {
                let (a, b) = (star[i], star[(i + 1) % 10]);
                (b[0] - a[0]).hypot(b[1] - a[1])
            })
            .sum();
        assert!(g < outline);
    }

    #[test]
    fn two_bodies_pick_the_one_around_the_axis() {
        let (mut v, mut t) = prism(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let (v2, t2) = prism(&[[3.0, 0.0], [5.0, 0.0], [5.0, 2.0], [3.0, 2.0]]);
        let off = v.len();
        v.extend(v2);
        t.extend(t2.iter().map(|f| f.map(|i| i + off)));
        assert_eq!(section_loops(&v, &t, 0.5).len(), 2);
        assert!((girth(&v, &t, 0.5, [4.0, 1.0]).unwrap() - 8.0).abs() < 1e-12);
        assert!((girth(&v, &t, 0.5, [0.5, 0.5]).unwrap() - 4.0).abs() < 1e-12);
        // beside both: girth finds nothing, girth_near the closer body
        assert_eq!(girth(&v, &t, 0.5, [1.4, 0.5]), None);
        assert!((girth_near(&v, &t, 0.5, [1.4, 0.5]).unwrap() - 4.0).abs() < 1e-12);
        assert!((girth_near(&v, &t, 0.5, [2.8, 1.0]).unwrap() - 8.0).abs() < 1e-12);
        assert!((girth_near(&v, &t, 0.5, [4.0, 1.0]).unwrap() - 8.0).abs() < 1e-12);
    }

    #[test]
    fn a_tilted_prism_measured_in_its_own_frame() {
        // the unit-square prism turned 90° about x: its axis now runs along −y
        let (v, t) = prism(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let turned: Vec<Vec3> = v.iter().map(|p| Vec3::new(p.x, -p.z, p.y)).collect();
        let f = in_frame(
            &turned,
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, -1.0, 0.0),
        );
        assert!((girth(&f, &t, 0.5, [0.0, 0.0]).unwrap() - 4.0).abs() < 1e-12);
        // its back (+y in the original frame is the square's far side at y = 1)
        assert!((back_point(&v, &t, 0.5, 0.5).unwrap() - 1.0).abs() < 1e-12);
        assert_eq!(back_point(&v, &t, 0.5, 3.0), None);
    }

    #[test]
    fn the_slicer_cuts_like_section_loops() {
        let star: Vec<[f64; 2]> = (0..10)
            .map(|i| {
                let a = std::f64::consts::PI * f64::from(i) / 5.0;
                let r = if i % 2 == 0 { 1.0 } else { 0.4 };
                [r * a.cos(), r * a.sin()]
            })
            .collect();
        let (v, t) = prism(&star);
        let s = Slicer::new(&v, &t, 0.07);
        for z in [-0.5, 0.0, 1e-9, 0.13, 0.5, 0.999, 1.0, 1.5] {
            assert_eq!(s.loops(z), section_loops(&v, &t, z), "z {z}");
        }
        assert_eq!(s.girth(0.25, [0.0, 0.0]), girth(&v, &t, 0.25, [0.0, 0.0]));
    }

    #[test]
    fn hull_of_collinear_and_repeated_points() {
        assert_eq!(hull_perimeter(&[]), 0.0);
        assert_eq!(hull_perimeter(&[[1.0, 1.0], [1.0, 1.0]]), 0.0);
        let line = hull_perimeter(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]]);
        assert!((line - 4.0).abs() < 1e-12, "{line}");
    }
}
