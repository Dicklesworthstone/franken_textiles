//! Measuring a body the way a tape does (plan §9.3): a girth is the perimeter of the convex hull of the
//! horizontal section that surrounds the part's axis, because a tape bridges hollows (between the
//! shoulder blades, the small of the back, the gluteal cleft).
//!
//! The functions take any unit and answer in it. A vertex exactly at the cutting height counts as above
//! it (simulation of simplicity), so on a closed mesh every section is a set of closed loops.

use ftx_core::{Vec3, det};

use crate::landmarks::BodyFrames;

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
    /// Slab `b`'s triangles, in index order: `faces[start[b]..start[b + 1]]`.
    start: Vec<usize>,
    faces: Vec<usize>,
}

impl<'a> Slicer<'a> {
    /// Buckets `triangles` by height in slabs of `width`.
    ///
    /// # Panics
    /// Unless `width` is finite and positive.
    #[must_use]
    pub fn new(vertices: &'a [Vec3], triangles: &'a [[usize; 3]], width: f64) -> Self {
        assert!(
            width.is_finite() && width > 0.0,
            "slicer slab width {width} must be finite and positive"
        );
        let (z0, z1) = vertices
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v.z), b.max(v.z))
            });
        let slab = |z: f64| ((z - z0) / width).floor() as usize;
        let n = if z1 >= z0 { slab(z1) + 1 } else { 0 };
        // the slabs each triangle reaches; counted, then filled in triangle order (one allocation each)
        let spans: Vec<(usize, usize)> = triangles
            .iter()
            .map(|t| {
                let (lo, hi) = t
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| {
                        (a.min(vertices[v].z), b.max(vertices[v].z))
                    });
                (slab(lo), slab(hi))
            })
            .collect();
        let mut start = vec![0usize; n + 1];
        for &(lo, hi) in &spans {
            for c in &mut start[lo + 1..=hi + 1] {
                *c += 1;
            }
        }
        for b in 0..n {
            start[b + 1] += start[b];
        }
        let mut next = start.clone();
        let mut faces = vec![0; start[n]];
        for (i, &(lo, hi)) in spans.iter().enumerate() {
            for at in &mut next[lo..=hi] {
                faces[*at] = i;
                *at += 1;
            }
        }
        Self {
            vertices,
            triangles,
            z0,
            width,
            start,
            faces,
        }
    }

    /// [`section_loops`] at height `z`.
    #[must_use]
    pub fn loops(&self, z: f64) -> Vec<Vec<[f64; 2]>> {
        let b = (z - self.z0) / self.width;
        if !(b >= 0.0 && (b as usize) < self.start.len().saturating_sub(1)) {
            return Vec::new();
        }
        let b = b as usize;
        loops_of(
            self.vertices,
            self.triangles,
            self.faces[self.start[b]..self.start[b + 1]].iter().copied(),
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
    pub fn back_point(&self, z: f64, around: [f64; 2]) -> Option<f64> {
        back_of(&self.loops(z), around)
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
    // the segments at each cut edge: (edge, segment) pairs in an open-addressed table at most half
    // full; an edge's pairs all lie in the probe run from its hash, and the walk takes the lowest
    // unused segment among them (the order the segments were found in)
    let cap = (4 * segments.len()).next_power_of_two();
    let slot = |(a, b): (usize, usize)| {
        let h = (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        (h >> 32) as usize & (cap - 1)
    };
    let mut at_edge = vec![(usize::MAX, 0, 0); cap];
    for (s, seg) in segments.iter().enumerate() {
        for &(a, b) in seg {
            let mut i = slot((a, b));
            while at_edge[i].0 != usize::MAX {
                i = (i + 1) & (cap - 1);
            }
            at_edge[i] = (a, b, s);
        }
    }
    let lowest_unused = |edge: (usize, usize), used: &[bool]| {
        let (mut i, mut best) = (slot(edge), None);
        while at_edge[i].0 != usize::MAX {
            let (a, b, s) = at_edge[i];
            if (a, b) == edge && !used[s] && best.is_none_or(|t| s < t) {
                best = Some(s);
            }
            i = (i + 1) & (cap - 1);
        }
        best
    };
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
            let Some(s) = lowest_unused(edge, &used) else {
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
    let hull = convex_hull(points);
    if hull.len() < 2 {
        return 0.0;
    }
    let n = hull.len();
    (0..n)
        .map(|i| {
            let (a, b) = (hull[i], hull[(i + 1) % n]);
            det::hypot(b[0] - a[0], b[1] - a[1])
        })
        .sum()
}

/// The convex hull of `points`, counter-clockwise from the lowest-x point (monotone chain; collinear
/// points dropped).
#[must_use]
pub fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut p = points.to_vec();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 2 {
        return p;
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
    hull
}

/// Length of the polyline through `points`.
fn path_length(points: &[[f64; 2]]) -> f64 {
    points
        .windows(2)
        .map(|w| det::hypot(w[1][0] - w[0][0], w[1][1] - w[0][1]))
        .sum()
}

/// The arc of a counter-clockwise `hull` from vertex `a` to vertex `b`, going forward (`true`) or back.
fn hull_arc(hull: &[[f64; 2]], a: usize, b: usize, forward: bool) -> Vec<[f64; 2]> {
    let n = hull.len();
    let mut out = vec![hull[a]];
    let mut i = a;
    while i != b {
        i = if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        };
        out.push(hull[i]);
    }
    out
}

/// Where the edges of a closed polygon cross the line `coord = value` (`coord` 0: x, 1: y), as points.
fn crossings(poly: &[[f64; 2]], coord: usize, value: f64) -> Vec<[f64; 2]> {
    let n = poly.len();
    let other = 1 - coord;
    (0..n)
        .filter_map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            ((a[coord] >= value) != (b[coord] >= value)).then(|| {
                let t = (value - a[coord]) / (b[coord] - a[coord]);
                let mut p = [0.0; 2];
                p[coord] = value;
                p[other] = a[other] + t * (b[other] - a[other]);
                p
            })
        })
        .collect()
}

/// Neck base girth: the section perpendicular to the neck at its base (the `neck01` head), around the
/// neck axis; and, in world millimetres, the section's point furthest to the wearer's right (the side neck
/// point).
#[must_use]
pub fn neck_base(mm: &[Vec3], faces: &[[usize; 3]], frames: &BodyFrames) -> Option<(f64, Vec3)> {
    let f = frames.neck;
    let local = in_frame(mm, f.origin, f.lat, f.front, f.up);
    let loops = section_loops(&local, faces, 0.0);
    let l = outermost_around(&loops, [0.0, 0.0])?;
    let side = l.iter().copied().min_by(|a, b| a[0].total_cmp(&b[0]))?;
    Some((
        hull_perimeter(l),
        f.origin + f.lat * side[0] + f.front * side[1],
    ))
}

/// Head girth: the fullest horizontal section around the head bone's axis from 15 % to 50 % of the way
/// down from the crown to C7 (the hat band line, above the brows and over the occiput).
#[must_use]
pub fn head_girth(torso: &Slicer, frames: &BodyFrames) -> Option<f64> {
    let top = *frames.landmarks.get("head_top")?;
    let hh = top - frames.landmarks.get("c7")?;
    let axis = [frames.head_bone.x, frames.head_bone.y];
    let mut best: Option<f64> = None;
    let mut d = 0.15 * hh;
    while d <= 0.5 * hh {
        let g = torso.girth_near(frames.floor_z + top - d, axis)?;
        best = Some(best.map_or(g, |b: f64| b.max(g)));
        d += 2.0;
    }
    best
}

/// Shoulder length and across back (spec 02 catalogue, ISO 8559-1 5.4.1 and 5.4.2), with the shoulder point
/// (acromion) taken as the top of the shoulder straight above the shoulder joint. Shoulder length: along the
/// top of the shoulder in the coronal section through the joint, from the side neck point `snp` to the
/// shoulder point. Across back: the back of the horizontal section 15 mm below the shoulder point, between
/// the two shoulder points' x, as a tape bridging hollows (the convex hull of the torso loop).
#[must_use]
pub fn shoulder_and_back(
    mm: &[Vec3],
    faces: &[[usize; 3]],
    torso: &Slicer,
    frames: &BodyFrames,
    snp: Vec3,
) -> Option<(f64, f64)> {
    let j = frames.shoulder_joint_r;
    // coronal section through the joint: (x, z) in world millimetres
    let coronal = in_frame(
        mm,
        Vec3::new(0.0, j.y, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let loops = section_loops(&coronal, faces, 0.0);
    let shoulder_section = outermost_around(&loops, [j.x, j.z])?;
    // the top of the shoulder at x, below the side neck point's height (the head and ears are higher)
    let top_at = |x: f64| -> Option<f64> {
        crossings(shoulder_section, 0, x)
            .into_iter()
            .map(|p| p[1])
            .filter(|&z| z <= snp.z + 5.0)
            .fold(None, |m: Option<f64>, z| Some(m.map_or(z, |m| m.max(z))))
    };
    let acromion_z = top_at(j.x)?;
    let mut line = vec![[snp.x, snp.z]];
    let mut x = snp.x - 2.0;
    while x > j.x {
        line.push([x, top_at(x)?]);
        x -= 2.0;
    }
    line.push([j.x, acromion_z]);
    let shoulder = path_length(&line);
    // across the back
    let sections = torso.loops(acromion_z - 15.0);
    let axis = [frames.torso.origin.x, frames.torso.origin.y];
    let hull = convex_hull(outermost_around(&sections, axis)?);
    if hull.len() < 3 {
        return None;
    }
    // The opposite shoulder is mirrored about the torso axis; +y is the back.
    let at = |x: f64| -> Option<[f64; 2]> {
        crossings(&hull, 0, x)
            .into_iter()
            .max_by(|a, b| a[1].total_cmp(&b[1]))
    };
    // a on the wearer's right (−x), b on the left; the hull's back chain between them is its vertices left
    // of a → b (larger y), in order of x
    let opposite_x = 2.0 * frames.torso.origin.x - j.x;
    let (a, b) = (at(j.x.min(opposite_x))?, at(j.x.max(opposite_x))?);
    let left_of_ab =
        |p: [f64; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) > 0.0;
    let mut arc: Vec<[f64; 2]> = hull
        .iter()
        .copied()
        .filter(|&p| p[0] > a[0] && p[0] < b[0] && left_of_ab(p))
        .collect();
    arc.sort_by(|p, q| p[0].total_cmp(&q[0]));
    arc.insert(0, a);
    arc.push(b);
    Some((shoulder, path_length(&arc)))
}

/// Total crotch length (ISO 8559-1 5.4.18): from the centre-front waist down between the legs to the
/// centre-back waist, as a snug tape bridging hollows: the lower convex hull of the outermost median
/// sagittal loops containing the waist axis, within 15 mm of the centre line and below the waist.
#[must_use]
pub fn crotch_length(mm: &[Vec3], faces: &[[usize; 3]], frames: &BodyFrames) -> Option<f64> {
    let waist = frames.floor_z + frames.landmarks.get("waist")?;
    let x0 = frames.torso.origin.x;
    // sagittal sections: (y, z) in world millimetres, cut at x = x0 + h
    let sagittal = in_frame(
        mm,
        Vec3::new(x0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 0.0),
    );
    let s = Slicer::new(&sagittal, faces, 5.0);
    let mut pts: Vec<[f64; 2]> = Vec::new();
    for h in [-15.0, -7.5, 0.0, 7.5, 15.0] {
        let loops = s.loops(h);
        // A foot or offset arm can cut this plane without belonging to the waist-to-waist path.
        if let Some(l) = outermost_around(&loops, [frames.torso.origin.y, waist]) {
            pts.extend(l.iter().copied().filter(|p| p[1] <= waist));
            pts.extend(crossings(l, 1, waist));
        }
    }
    let hull = convex_hull(&pts);
    if hull.len() < 3 {
        return None;
    }
    // the hull's two corners on the waist line: front (−y) and back (+y); the tape is the lower chain
    // between them (counter-clockwise from front to back runs along the bottom)
    let on_waist: Vec<usize> = (0..hull.len())
        .filter(|&i| hull[i][1] >= waist - 1e-9)
        .collect();
    let front = *on_waist
        .iter()
        .min_by(|&&i, &&k| hull[i][0].total_cmp(&hull[k][0]))?;
    let back = *on_waist
        .iter()
        .max_by(|&&i, &&k| hull[i][0].total_cmp(&hull[k][0]))?;
    Some(path_length(&hull_arc(&hull, front, back, true)))
}

/// Outseam (ISO 8559-1 5.4.22): from the waist at the side, over the hip, to the floor, as a tape hanging
/// down the side: seen from the front, the outer edge of the wearer's-right torso and leg between crotch and
/// waist (its convex hull, from the waist section's side point to the widest point of the hip), then
/// straight down to the floor (the tape hangs; it does not follow a leg spread in the pose).
#[must_use]
pub fn outseam(torso: &Slicer, frames: &BodyFrames) -> Option<f64> {
    let waist = frames.floor_z + frames.landmarks.get("waist")?;
    let crotch = frames.floor_z + frames.landmarks.get("crotch")?;
    let x0 = frames.torso.origin.x;
    let axis = [x0, frames.torso.origin.y];
    let side = outermost_around(&torso.loops(waist), axis)?
        .iter()
        .copied()
        .min_by(|a, b| a[0].total_cmp(&b[0]))?;
    let top = [side[0], waist];
    let mut pts: Vec<[f64; 2]> = frames
        .torso_points
        .iter()
        .chain(&frames.leg_points)
        .filter(|p| p.x < x0 && p.z <= waist && p.z >= crotch)
        .map(|p| [p.x, p.z])
        .collect();
    pts.push(top);
    let hull = convex_hull(&pts);
    if hull.len() < 2 {
        return None;
    }
    let start = (0..hull.len()).find(|&i| hull[i] == top)?;
    // the widest point of the hip: the hull's smallest x (the lowest of ties); counter-clockwise from the
    // waist corner runs down the outer side
    let widest = (0..hull.len()).min_by(|&i, &k| {
        hull[i][0]
            .total_cmp(&hull[k][0])
            .then(hull[i][1].total_cmp(&hull[k][1]))
    })?;
    let arc = hull_arc(&hull, start, widest, true);
    Some(path_length(&arc) + (hull[widest][1] - frames.floor_z))
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

/// The largest y where the outermost section containing the torso axis `around` crosses x =
/// `around[0]` at height `z` (+y is the back). Missing torso sections yield `None`.
#[must_use]
pub fn back_point(
    vertices: &[Vec3],
    triangles: &[[usize; 3]],
    z: f64,
    around: [f64; 2],
) -> Option<f64> {
    back_of(&section_loops(vertices, triangles, z), around)
}

fn back_of(loops: &[Vec<[f64; 2]>], around: [f64; 2]) -> Option<f64> {
    let torso = outermost_around(loops, around)?;
    crossings(torso, 0, around[0])
        .into_iter()
        .map(|p| p[1])
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
/// with the nearest closed section edge. Collinear subdivision does not change this distance.
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
    if !around.iter().all(|x| x.is_finite()) {
        return None;
    }
    let chosen = outermost_around(loops, around).or_else(|| {
        let dist = |l: &Vec<[f64; 2]>| {
            (0..l.len())
                .map(|i| {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                    let length2 = dx * dx + dy * dy;
                    // Closest point on the closed segment; cuts through vertices can repeat a point.
                    let t = if length2 > 0.0 {
                        (((around[0] - a[0]) * dx + (around[1] - a[1]) * dy) / length2)
                            .clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    det::hypot(a[0] + t * dx - around[0], a[1] + t * dy - around[1])
                })
                .fold(f64::INFINITY, f64::min)
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
    fn qualification_across_back_excludes_disconnected_arm_sections() {
        use crate::landmarks::Frame;

        let frame = Frame {
            origin: Vec3::ZERO,
            up: Vec3::new(0.0, 0.0, 1.0),
            front: Vec3::new(0.0, -1.0, 0.0),
            lat: Vec3::new(1.0, 0.0, 0.0),
        };
        let mut frames = BodyFrames {
            torso: frame,
            arm_r: frame,
            leg_r: frame,
            floor_z: 0.0,
            tip_h: 0.0,
            upper_arm_radius: 0.0,
            arm_length: 0.0,
            landmarks: std::collections::BTreeMap::new(),
            neck: frame,
            head_bone: Vec3::ZERO,
            shoulder_joint_r: Vec3::new(-50.0, 0.0, 90.0),
            torso_points: Vec::new(),
            leg_points: Vec::new(),
        };
        for shift in [
            Vec3::ZERO,
            Vec3::new(0.0, 200.0, -125.0),
            Vec3::new(37.0, -100.0, 60.0),
            Vec3::new(-140.0, 90.0, -50.0),
        ] {
            frames.torso.origin = shift;
            frames.shoulder_joint_r = Vec3::new(-50.0, 0.0, 90.0) + shift;
            let snp = Vec3::new(-20.0, 0.0, 100.0) + shift;
            for (back_y, arm_back) in [40.0, 70.0].into_iter().flat_map(|y| {
                [None, Some(60.0), Some(160.0)]
                    .into_iter()
                    .map(move |arm| (y, arm))
            }) {
                let (mut v, mut t) = prism(&[
                    [-80.0, -40.0],
                    [80.0, -40.0],
                    [80.0, 40.0],
                    [0.0, back_y],
                    [-80.0, 40.0],
                ]);
                if let Some(y) = arm_back {
                    let (arm, faces) = prism(&[
                        [-130.0, y],
                        [-100.0, y],
                        [-100.0, y + 20.0],
                        [-130.0, y + 20.0],
                    ]);
                    let offset = v.len();
                    v.extend(arm);
                    t.extend(faces.iter().map(|f| f.map(|i| i + offset)));
                }
                for p in &mut v {
                    p.z *= 100.0;
                    *p += shift;
                }
                for _ in 0..2 {
                    let s = Slicer::new(&v, &t, 5.0);
                    // Flat shoulder: |-50 - -20| = 30 mm. The back is two straight
                    // segments meeting at (0, back_y), so use the Pythagorean length,
                    // not just the 100 mm chord between shoulder x coordinates.
                    let expected_back = 2.0 * 50.0_f64.hypot((back_y - 40.0) * 50.0 / 80.0);
                    let (shoulder, back) = shoulder_and_back(&v, &t, &s, &frames, snp)
                        .expect("closed torso with shoulder and back sections");
                    assert!((shoulder - 30.0).abs() < 1e-10);
                    assert!(
                        (back - expected_back).abs() < 1e-10,
                        "back={back}, expected={expected_back}, arm={arm_back:?}, shift={shift:?}"
                    );
                    // An unrelated closed section must not stand in for an absent torso.
                    if arm_back.is_some() {
                        let mut missing_torso = frames.clone();
                        missing_torso.torso.origin.x = 500.0;
                        assert_eq!(shoulder_and_back(&v, &t, &s, &missing_torso, snp), None);
                    }
                    t.reverse();
                }
            }
        }
    }

    #[test]
    fn qualification_shoulder_excludes_disconnected_coronal_sections() {
        for shift in [
            Vec3::ZERO,
            Vec3::new(37.0, -100.0, 60.0),
            Vec3::new(-140.0, 90.0, -50.0),
        ] {
            let mut frames = waist_frames();
            frames.torso.origin = shift;
            frames.shoulder_joint_r = Vec3::new(-50.0, 0.0, 90.0) + shift;
            let snp = Vec3::new(-20.0, 0.0, 100.0) + shift;
            for shell_z in [None, Some((1.02, 1.04)), Some((0.9, 0.95))] {
                let (mut vertices, mut faces) =
                    prism(&[[-80.0, -40.0], [80.0, -40.0], [80.0, 40.0], [-80.0, 40.0]]);
                if let Some((bottom, top)) = shell_z {
                    let (mut shell, triangles) =
                        prism(&[[-55.0, -5.0], [-15.0, -5.0], [-15.0, 5.0], [-55.0, 5.0]]);
                    for p in &mut shell {
                        p.z = bottom + p.z * (top - bottom);
                    }
                    let offset = vertices.len();
                    vertices.extend(shell);
                    faces.extend(triangles.iter().map(|f| f.map(|i| i + offset)));
                }
                for p in &mut vertices {
                    p.z *= 100.0;
                    *p += shift;
                }
                for _ in 0..2 {
                    let torso = Slicer::new(&vertices, &faces, 5.0);
                    let (shoulder, back) =
                        shoulder_and_back(&vertices, &faces, &torso, &frames, snp)
                            .expect("closed shoulder section containing the joint");
                    // The real shoulder is horizontal: 30 mm from x=-20 to x=-50.
                    // The rectangular back is 100 mm between the two shoulder axes.
                    assert!(
                        (shoulder - 30.0).abs() < 1e-10,
                        "shoulder={shoulder}, shell={shell_z:?}, shift={shift:?}"
                    );
                    assert!((back - 100.0).abs() < 1e-10);
                    let mut outside = frames.clone();
                    outside.shoulder_joint_r.z += 200.0;
                    assert_eq!(
                        shoulder_and_back(&vertices, &faces, &torso, &outside, snp),
                        None
                    );
                    faces.reverse();
                }
            }
        }
    }

    fn waist_frames() -> BodyFrames {
        use crate::landmarks::Frame;

        let frame = Frame {
            origin: Vec3::ZERO,
            up: Vec3::new(0.0, 0.0, 1.0),
            front: Vec3::new(0.0, -1.0, 0.0),
            lat: Vec3::new(1.0, 0.0, 0.0),
        };
        BodyFrames {
            torso: frame,
            arm_r: frame,
            leg_r: frame,
            floor_z: 0.0,
            tip_h: 0.0,
            upper_arm_radius: 0.0,
            arm_length: 0.0,
            landmarks: std::collections::BTreeMap::from([
                ("waist".into(), 100.0),
                ("crotch".into(), 50.0),
            ]),
            neck: frame,
            head_bone: Vec3::ZERO,
            shoulder_joint_r: Vec3::ZERO,
            torso_points: Vec::new(),
            leg_points: Vec::new(),
        }
    }

    #[test]
    fn qualification_crotch_excludes_sections_away_from_the_waist_axis() {
        let mut frames = waist_frames();
        let sagittal_prism = |base: &[[f64; 2]]| {
            let (v, t) = prism(base);
            (
                v.into_iter()
                    .map(|p| Vec3::new(40.0 * p.z - 20.0, p.x, p.y))
                    .collect::<Vec<_>>(),
                t,
            )
        };
        let extras = [
            [[-10.0, 0.0], [10.0, 0.0], [10.0, 20.0], [-10.0, 20.0]],
            [[120.0, 40.0], [150.0, 40.0], [150.0, 140.0], [120.0, 140.0]],
            [[-10.0, 60.0], [10.0, 60.0], [10.0, 130.0], [-10.0, 130.0]],
        ];
        for shift in [Vec3::ZERO, Vec3::new(200.0, 300.0, 125.0)] {
            frames.torso.origin = shift;
            frames.floor_z = shift.z;
            for mask in 0..8usize {
                let (mut v, mut t) =
                    sagittal_prism(&[[-40.0, 50.0], [40.0, 50.0], [40.0, 150.0], [-40.0, 150.0]]);
                for (index, base) in extras.iter().enumerate() {
                    if mask & (1 << index) != 0 {
                        let (part, faces) = sagittal_prism(base);
                        let offset = v.len();
                        v.extend(part);
                        t.extend(faces.iter().map(|f| f.map(|i| i + offset)));
                    }
                }
                for point in &mut v {
                    *point += shift;
                }
                for _ in 0..2 {
                    // Independent waist-to-waist path: 50 mm down, 80 mm across, 50 mm up.
                    // A lower foot, a waist-height arm or an inner shell must not lengthen it.
                    let measured = crotch_length(&v, &t, &frames).unwrap();
                    assert!(
                        (measured - 180.0).abs() < 1e-10,
                        "{measured}, mask={mask}, shift={shift:?}"
                    );
                    t.reverse();
                }
            }
            for base in &extras[..2] {
                let (mut v, t) = sagittal_prism(base);
                for point in &mut v {
                    *point += shift;
                }
                assert_eq!(crotch_length(&v, &t, &frames), None);
            }
        }
    }

    #[test]
    fn qualification_outseam_accepts_straight_and_two_vertex_profiles() {
        for shift in [Vec3::ZERO, Vec3::new(200.0, 300.0, 125.0)] {
            let mut frames = waist_frames();
            frames.torso.origin = shift;
            frames.floor_z = shift.z;
            for (bottom_scale, top_scale, expected) in [
                (1.0, 1.0, 100.0),
                (1.5, 0.5, 50.0_f64.hypot(20.0) + 50.0),
                (0.5, 1.5, 100.0),
            ] {
                let (mut v, mut t) =
                    prism(&[[-40.0, -30.0], [40.0, -30.0], [40.0, 30.0], [-40.0, 30.0]]);
                for p in &mut v {
                    let scale = bottom_scale + (top_scale - bottom_scale) * p.z;
                    *p = Vec3::new(p.x * scale, p.y, 50.0 + 100.0 * p.z) + shift;
                }
                frames.torso_points = v.clone();
                for _ in 0..2 {
                    let s = Slicer::new(&v, &t, 5.0);
                    // A hanging tape is vertical for a straight or narrowing hip; a widening
                    // hip adds its independently known diagonal, then 50 mm down to the floor.
                    let measured = outseam(&s, &frames).expect("valid two-vertex outer profile");
                    assert!(
                        (measured - expected).abs() < 1e-10,
                        "{measured} vs {expected}, shift={shift:?}"
                    );
                    let mut missing = frames.clone();
                    missing.torso.origin.x += 1000.0;
                    assert_eq!(outseam(&s, &missing), None);
                    t.reverse();
                }
            }
        }
    }

    #[test]
    fn qualification_nearest_girth_is_independent_of_collinear_subdivision() {
        // The axis is 0.25 from the 4 x 4 square, 0.75 from the 1 x 1 square.
        // Refining only the large square's right edge changes no surface or tape length.
        let coarse = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let refined = [
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 0.5],
            [4.0, 1.0],
            [4.0, 1.5],
            [4.0, 2.0],
            [4.0, 2.5],
            [4.0, 3.0],
            [4.0, 3.5],
            [4.0, 4.0],
            [0.0, 4.0],
        ];
        for base in [&coarse[..], &refined[..]] {
            let (mut v, mut t) = prism(base);
            let (other, faces) = prism(&[[5.0, 1.0], [6.0, 1.0], [6.0, 2.0], [5.0, 2.0]]);
            let offset = v.len();
            v.extend(other);
            t.extend(faces.iter().map(|f| f.map(|i| i + offset)));
            for _ in 0..2 {
                let s = Slicer::new(&v, &t, 0.125);
                for z in [0.25, 0.5, 0.75] {
                    let axis = [4.25, 1.5];
                    assert_eq!(girth(&v, &t, z, axis), None);
                    assert_eq!(
                        girth_near(&v, &t, z, axis),
                        Some(16.0),
                        "{} vertices, z={z}",
                        base.len()
                    );
                    assert_eq!(s.girth_near(z, axis), Some(16.0));
                    // Containing loops retain priority.
                    assert_eq!(s.girth_near(z, [3.9, 1.5]), Some(16.0));
                }
                t.reverse();
            }
        }
        let (mut v, mut t) = prism(&coarse);
        let (other, faces) = prism(&[[2.1, 1.9], [2.3, 1.9], [2.3, 2.1], [2.1, 2.1]]);
        let offset = v.len();
        v.extend(other);
        t.extend(faces.iter().map(|f| f.map(|i| i + offset)));
        // The inner loop is closer but does not contain the axis; the outer loop does.
        assert_eq!(girth_near(&v, &t, 0.5, [2.0, 2.0]), Some(16.0));
        assert_eq!(
            Slicer::new(&v, &t, 0.125).girth_near(0.5, [2.0, 2.0]),
            Some(16.0)
        );
    }

    #[test]
    fn qualification_sections_match_a_tapered_rectangular_prism() {
        for scale in [1.0, 100.0] {
            let (mut v, mut t) = prism(&[[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]]);
            for p in &mut v {
                if p.z == 1.0 {
                    p.x *= 2.0;
                    p.y *= 3.0;
                }
                *p = *p * scale + Vec3::new(15.0, -80.0, 8.0);
            }
            for _ in 0..2 {
                let s = Slicer::new(&v, &t, 0.125 * scale);
                for h in [0.125, 0.5, 0.875, 1.0] {
                    // At fraction h: width=2+2h, depth=1+2h, independently of triangulation.
                    let z = 8.0 + h * scale;
                    let axis = [15.0 + scale, -80.0 + 0.5 * scale];
                    let perimeter = (6.0 + 8.0 * h) * scale;
                    let loops = section_loops(&v, &t, z);
                    assert_eq!(loops.len(), 1);
                    let area = (2.0 + 2.0 * h) * (1.0 + 2.0 * h) * scale * scale;
                    assert!((polygon_area(&loops[0]).abs() - area).abs() < 1e-9);
                    assert_eq!(girth(&v, &t, z, axis), Some(perimeter));
                    assert_eq!(s.girth_near(z, axis), Some(perimeter));
                }
                t.reverse();
            }
        }
    }

    #[test]
    fn qualification_spine_path_excludes_disconnected_sections() {
        let heights = [0.125, 0.375, 0.625, 0.875];
        for extras in 0..8 {
            let (mut vertices, mut faces) =
                prism(&[[-2.0, -1.0], [2.0, -1.0], [2.0, 1.0], [-2.0, 1.0]]);
            for (bit, y0, y1, z0, z1) in [
                (1, 4.0, 5.0, 0.25, 0.75),
                (2, -5.0, -4.0, 0.0, 1.0),
                (4, -0.4, 0.4, 0.0, 1.0),
            ] {
                if extras & bit == 0 {
                    continue;
                }
                let (mut v, t) = prism(&[[-0.5, y0], [0.5, y0], [0.5, y1], [-0.5, y1]]);
                for p in &mut v {
                    p.z = z0 + (z1 - z0) * p.z;
                }
                let offset = vertices.len();
                vertices.extend(v);
                faces.extend(t.into_iter().map(|f| f.map(|i| i + offset)));
            }
            for shift in [Vec3::ZERO, Vec3::new(13.0, -17.0, 4.0)] {
                let v: Vec<_> = vertices
                    .iter()
                    .map(|p| Vec3::new(p.x, p.y + 0.5 * p.z, p.z) + shift)
                    .collect();
                for _ in 0..2 {
                    let slicer = Slicer::new(&v, &faces, 0.125);
                    let mut path = Vec::new();
                    for h in heights {
                        let z = shift.z + h;
                        let expected = shift.y + 1.0 + 0.5 * h;
                        let axis = [shift.x, shift.y];
                        let direct = back_point(&v, &faces, z, axis);
                        let cached = slicer.back_point(z, axis);
                        assert_eq!(direct, Some(expected), "extras={extras}, shift={shift:?}");
                        assert_eq!(cached, direct);
                        path.push([z, cached.unwrap()]);
                    }
                    // Independently: 0.75 high and 0.375 back, along one straight spine.
                    let expected = (0.75_f64 * 0.75 + 0.375 * 0.375).sqrt();
                    assert!((path_length(&path) - expected).abs() < 1e-12);
                    for axis in [
                        [shift.x, shift.y + 2.0],
                        [shift.x, shift.y - 2.0],
                        [f64::NAN, shift.y],
                        [shift.x, f64::INFINITY],
                    ] {
                        assert_eq!(back_point(&v, &faces, shift.z + 0.375, axis), None);
                        assert_eq!(slicer.back_point(shift.z + 0.375, axis), None);
                    }
                    faces.reverse();
                }
            }
        }
    }

    #[test]
    fn qualification_out_of_domain_slices_have_no_reading() {
        let (v, t) = prism(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let s = Slicer::new(&v, &t, 0.125);
        for z in [1.5, f64::MAX, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(section_loops(&v, &t, z).is_empty());
            assert!(s.loops(z).is_empty(), "z={z}");
            assert_eq!(s.girth(z, [0.5, 0.5]), None);
            assert_eq!(s.girth_near(z, [0.5, 0.5]), None);
            assert_eq!(s.back_point(z, [0.5, 0.5]), None);
        }
    }

    #[test]
    fn qualification_nearest_girth_requires_a_finite_axis() {
        let (v, t) = prism(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let s = Slicer::new(&v, &t, 0.125);
        for axis in [
            [f64::NAN, 0.5],
            [0.5, f64::INFINITY],
            [f64::NEG_INFINITY, 0.5],
        ] {
            assert_eq!(girth_near(&v, &t, 0.5, axis), None);
            assert_eq!(s.girth_near(0.5, axis), None);
        }
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
        assert!((back_point(&v, &t, 0.5, [0.5, 0.5]).unwrap() - 1.0).abs() < 1e-12);
        assert_eq!(back_point(&v, &t, 0.5, [3.0, 0.5]), None);
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

    /// The walk as it was first written: each cut edge's segments in a `BTreeMap`.
    fn loops_reference(vertices: &[Vec3], triangles: &[[usize; 3]], z: f64) -> Vec<Vec<[f64; 2]>> {
        let above = |i: usize| vertices[i].z >= z;
        let mut segments: Vec<[(usize, usize); 2]> = Vec::new();
        for t in triangles {
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
        let mut at_edge: std::collections::BTreeMap<(usize, usize), Vec<usize>> =
            std::collections::BTreeMap::new();
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

    /// Two lumpy closed spheres side by side, rings at shared heights: sections of none, one or two
    /// non-convex loops, and cuts exactly through vertices.
    fn lumpy_pair() -> (Vec<Vec3>, Vec<[usize; 3]>) {
        let (rings, around) = (23, 31);
        let (mut v, mut t) = (Vec::new(), Vec::new());
        for (cx, phase) in [(0.0, 0.0), (2.6, 1.7)] {
            let south = v.len();
            v.push(Vec3::new(cx, 0.0, -1.0));
            for i in 1..rings {
                let (sp, cp) = det::sin_cos(std::f64::consts::PI * i as f64 / rings as f64);
                for j in 0..around {
                    let th = 2.0 * std::f64::consts::PI * j as f64 / around as f64;
                    let (st, ct) = det::sin_cos(th);
                    let r = 1.0 + 0.25 * det::sin(3.0 * th + phase) * (2.0 * sp * cp);
                    v.push(Vec3::new(cx + r * sp * ct, r * sp * st, -cp));
                }
            }
            let north = v.len();
            v.push(Vec3::new(cx, 0.0, 1.0));
            let ring = |i: usize, j: usize| south + 1 + (i - 1) * around + j % around;
            for j in 0..around {
                t.push([south, ring(1, j + 1), ring(1, j)]);
                t.push([north, ring(rings - 1, j), ring(rings - 1, j + 1)]);
            }
            for i in 1..rings - 1 {
                for j in 0..around {
                    t.push([ring(i, j), ring(i, j + 1), ring(i + 1, j + 1)]);
                    t.push([ring(i, j), ring(i + 1, j + 1), ring(i + 1, j)]);
                }
            }
        }
        (v, t)
    }

    #[test]
    fn sorted_edge_walk_and_flat_slabs_cut_as_the_first_version() {
        let (v, t) = lumpy_pair();
        let width = 0.05;
        let s = Slicer::new(&v, &t, width);
        // the slabs: each triangle in every slab its height range reaches, in index order
        let z0 = v.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
        let slab = |z: f64| ((z - z0) / width).floor() as usize;
        let mut buckets = vec![Vec::new(); s.start.len() - 1];
        for (i, tri) in t.iter().enumerate() {
            let zs = tri.map(|k| v[k].z);
            let (lo, hi) = (zs[0].min(zs[1]).min(zs[2]), zs[0].max(zs[1]).max(zs[2]));
            for b in &mut buckets[slab(lo)..=slab(hi)] {
                b.push(i);
            }
        }
        for (b, want) in buckets.iter().enumerate() {
            assert_eq!(&s.faces[s.start[b]..s.start[b + 1]], &want[..], "slab {b}");
        }
        let mut zs: Vec<f64> = (0..400)
            .map(|k| -1.1 + 2.2 * f64::from(k) / 399.0)
            .collect();
        zs.extend(v.iter().step_by(7).map(|p| p.z));
        let mut two = 0;
        for z in zs {
            let want = loops_reference(&v, &t, z);
            two += usize::from(want.len() == 2);
            assert_eq!(section_loops(&v, &t, z), want, "z {z}");
            assert_eq!(s.loops(z), want, "slicer, z {z}");
        }
        assert!(two > 300, "{two} two-loop sections");
    }

    #[test]
    fn hull_of_collinear_and_repeated_points() {
        assert_eq!(hull_perimeter(&[]), 0.0);
        assert_eq!(hull_perimeter(&[[1.0, 1.0], [1.0, 1.0]]), 0.0);
        let line = hull_perimeter(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]]);
        assert!((line - 4.0).abs() < 1e-12, "{line}");
    }
}
