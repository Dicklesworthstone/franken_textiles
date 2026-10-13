//! Exact triangle intersections, including coplanar overlap and contact beyond a shared mesh feature.
//!
//! Provenance: adapted from frankensim `crates/fs-topo/src/intersect.rs` at commit
//! `585446d4c8ea7ff2cff96eafe21eb56e28ee0b55` (MIT + OpenAI/Anthropic rider). The plane-separation,
//! projection-axis and shared-feature decisions are lifted; `Vec3`/`usize` replace `Point3`/`u32`.
//! Unlike the upstream private segment helper, [`segment_triangle`] also decides coplanar segments,
//! so it is safe to use independently of the triangle-pair test. No epsilon decides a contact.
//!
//! These are narrow-phase predicates, not a broad phase or a thickness model. Touching is conservative:
//! exact contact is an intersection unless [`indexed_triangle_pair`] proves it is *only* the shared
//! vertex or edge. Degenerate triangles fail closed as touching; [`validate_mesh`] refuses them.
//! Callers validate before building bounding boxes, since min/max can otherwise hide a NaN.
//! The arithmetic domain is that of [`crate::exact`]: differences and predicate products must remain
//! in the normal f64 range. Units are those of the inputs; no conversion or snapping occurs.

use crate::exact::{Sign, orient2d, orient3d};
use crate::{Refusal, RefusalCode, Vec3};

/// The geometric contact class (not a cloth-clearance or fit verdict).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntersectionKind {
    /// The interiors cross strictly.
    Crossing,
    /// Boundary contact, coplanar overlap, or a degenerate primitive that cannot pass the test.
    Touching,
}

type Triangle = [[f64; 3]; 3];

fn xyz(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn project(p: [f64; 3], drop: usize) -> [f64; 2] {
    match drop {
        0 => [p[1], p[2]],
        1 => [p[2], p[0]],
        _ => [p[0], p[1]],
    }
}

// A nonzero projected orientation is an exact nonzero component of the normal. Unlike choosing the
// largest rounded cross-product component, this cannot select a collapsed projection by cancellation.
fn projection_axis(t: &Triangle) -> Option<usize> {
    (0..3).find(|&drop| {
        orient2d(
            project(t[0], drop),
            project(t[1], drop),
            project(t[2], drop),
        ) != Sign::Zero
    })
}

/// Whether a triangle has exactly zero area (including repeated corners).
///
/// # Panics
/// On non-finite coordinates or predicate arithmetic outside [`crate::exact`]'s supported domain.
#[must_use]
pub fn is_degenerate(t: [Vec3; 3]) -> bool {
    projection_axis(&t.map(xyz)).is_none()
}

/// Admit a mesh before an intersection broad phase reads its coordinates. An empty mesh is allowed.
///
/// # Errors
/// [`RefusalCode::NonFinite`] for any non-finite vertex; [`RefusalCode::DegenerateGeometry`] for an
/// out-of-range index or exactly zero-area face. This does not test topology, winding or intersections.
///
/// # Panics
/// On predicate arithmetic outside [`crate::exact`]'s supported domain.
pub fn validate_mesh(vertices: &[Vec3], triangles: &[[usize; 3]]) -> Result<(), Refusal> {
    if let Some(vertex) = vertices.iter().position(|p| !p.is_finite()) {
        return Err(Refusal::new(
            RefusalCode::NonFinite,
            format!("mesh vertex {vertex} is not finite"),
        ));
    }
    for (face, t) in triangles.iter().enumerate() {
        if let Some(vertex) = t.iter().find(|&&i| i >= vertices.len()) {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("mesh face {face} references missing vertex {vertex}"),
            ));
        }
        if is_degenerate(t.map(|i| vertices[i])) {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                format!("mesh face {face} has exactly zero area"),
            ));
        }
    }
    Ok(())
}

fn opposite(a: Sign, b: Sign) -> bool {
    matches!(
        (a, b),
        (Sign::Positive, Sign::Negative) | (Sign::Negative, Sign::Positive)
    )
}

fn inside(p: [f64; 2], t: [[f64; 2]; 3]) -> bool {
    let s = [
        orient2d(t[0], t[1], p),
        orient2d(t[1], t[2], p),
        orient2d(t[2], t[0], p),
    ];
    !(s.contains(&Sign::Positive) && s.contains(&Sign::Negative))
}

fn on_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    (0..2).all(|k| a[k].min(b[k]) <= p[k] && p[k] <= a[k].max(b[k]))
}

fn segments_meet(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let (s0, s1) = (orient2d(a, b, c), orient2d(a, b, d));
    let (s2, s3) = (orient2d(c, d, a), orient2d(c, d, b));
    (opposite(s0, s1) && opposite(s2, s3))
        || (s0 == Sign::Zero && on_segment(c, a, b))
        || (s1 == Sign::Zero && on_segment(d, a, b))
        || (s2 == Sign::Zero && on_segment(a, c, d))
        || (s3 == Sign::Zero && on_segment(b, c, d))
}

fn coplanar_segment(p: [f64; 3], q: [f64; 3], t: &Triangle) -> Option<IntersectionKind> {
    let Some(drop) = projection_axis(t) else {
        return Some(IntersectionKind::Touching);
    };
    let t = t.map(|v| project(v, drop));
    let (p, q) = (project(p, drop), project(q, drop));
    (inside(p, t) || inside(q, t) || (0..3).any(|k| segments_meet(p, q, t[k], t[(k + 1) % 3])))
        .then_some(IntersectionKind::Touching)
}

fn segment_raw(p: [f64; 3], q: [f64; 3], t: &Triangle) -> Option<IntersectionKind> {
    let s1 = orient3d(t[0], t[1], t[2], p);
    let s2 = orient3d(t[0], t[1], t[2], q);
    if s1 == s2 {
        return if s1 == Sign::Zero {
            coplanar_segment(p, q, t)
        } else {
            None
        };
    }
    let signs = [
        orient3d(p, q, t[0], t[1]),
        orient3d(p, q, t[1], t[2]),
        orient3d(p, q, t[2], t[0]),
    ];
    if signs.contains(&Sign::Positive) && signs.contains(&Sign::Negative) {
        None
    } else if signs.contains(&Sign::Zero) || s1 == Sign::Zero || s2 == Sign::Zero {
        Some(IntersectionKind::Touching)
    } else {
        Some(IntersectionKind::Crossing)
    }
}

/// Intersect a closed segment with a closed triangle, including segments lying in its plane.
/// Degenerate triangles conservatively return [`IntersectionKind::Touching`].
///
/// # Panics
/// On non-finite coordinates or predicate arithmetic outside [`crate::exact`]'s supported domain.
#[must_use]
pub fn segment_triangle(p: Vec3, q: Vec3, t: [Vec3; 3]) -> Option<IntersectionKind> {
    let t = t.map(xyz);
    if projection_axis(&t).is_none() {
        Some(IntersectionKind::Touching)
    } else {
        segment_raw(xyz(p), xyz(q), &t)
    }
}

fn plane_signs(plane: &Triangle, points: &Triangle) -> [Sign; 3] {
    points.map(|p| orient3d(plane[0], plane[1], plane[2], p))
}

fn tri_raw(a: &Triangle, b: &Triangle) -> Option<IntersectionKind> {
    let sa = plane_signs(b, a);
    if sa == [Sign::Positive; 3] || sa == [Sign::Negative; 3] {
        return None;
    }
    let sb = plane_signs(a, b);
    if sb == [Sign::Positive; 3] || sb == [Sign::Negative; 3] {
        return None;
    }
    let mut touching = false;
    for i in 0..3 {
        for hit in [
            segment_raw(a[i], a[(i + 1) % 3], b),
            segment_raw(b[i], b[(i + 1) % 3], a),
        ] {
            match hit {
                Some(IntersectionKind::Crossing) => return hit,
                Some(IntersectionKind::Touching) => touching = true,
                None => {}
            }
        }
    }
    touching.then_some(IntersectionKind::Touching)
}

/// Intersect two closed triangles. Coincident geometry is contact even with different vertex indices.
/// Degenerate triangles conservatively return [`IntersectionKind::Touching`].
///
/// # Panics
/// On non-finite coordinates or predicate arithmetic outside [`crate::exact`]'s supported domain.
#[must_use]
pub fn triangle_triangle(a: [Vec3; 3], b: [Vec3; 3]) -> Option<IntersectionKind> {
    let (a, b) = (a.map(xyz), b.map(xyz));
    if projection_axis(&a).is_none() || projection_axis(&b).is_none() {
        Some(IntersectionKind::Touching)
    } else {
        tri_raw(&a, &b)
    }
}

// A corner cone of a non-degenerate triangle has opening angle < pi. It contains this ray iff the
// ray is in both bounding half-planes. Winding may be reversed independently for either face.
fn in_cone(apex: [f64; 2], u: [f64; 2], v: [f64; 2], r: [f64; 2]) -> bool {
    let (u, v) = match orient2d(apex, u, v) {
        Sign::Positive => (u, v),
        Sign::Negative => (v, u),
        Sign::Zero => return true,
    };
    orient2d(apex, u, r) != Sign::Negative && orient2d(apex, r, v) != Sign::Negative
}

/// Test two faces of one mesh for intersection *beyond* their shared indexed feature.
///
/// Two shared vertices allow only their common edge; one allows only that vertex. Neighbours are
/// decided, never blindly skipped. Duplicate faces and coincident but differently indexed vertices
/// are contacts. Check the mesh with [`validate_mesh`] before querying candidate pairs.
///
/// # Panics
/// On out-of-range indices, non-finite coordinates or arithmetic outside [`crate::exact`]'s domain.
#[must_use]
pub fn indexed_triangle_pair(
    vertices: &[Vec3],
    ti: [usize; 3],
    tj: [usize; 3],
) -> Option<IntersectionKind> {
    let (a, b) = (ti.map(|i| xyz(vertices[i])), tj.map(|i| xyz(vertices[i])));
    let (Some(drop), Some(_)) = (projection_axis(&a), projection_axis(&b)) else {
        return Some(IntersectionKind::Touching);
    };
    let (mut shared_a, mut shared_b) = ([0; 3], [0; 3]);
    let (mut na, mut nb) = (0, 0);
    for k in 0..3 {
        if tj.contains(&ti[k]) {
            shared_a[na] = k;
            na += 1;
        }
        if ti.contains(&tj[k]) {
            shared_b[nb] = k;
            nb += 1;
        }
    }
    match na {
        0 => tri_raw(&a, &b),
        3 => Some(IntersectionKind::Touching),
        2 => {
            let ao = (0..3).find(|k| !shared_a[..2].contains(k)).unwrap();
            let bo = (0..3).find(|k| !shared_b[..2].contains(k)).unwrap();
            if orient3d(b[0], b[1], b[2], a[ao]) != Sign::Zero {
                // Distinct planes meet along precisely the line of the shared edge.
                return None;
            }
            let (p, q) = (project(a[shared_a[0]], drop), project(a[shared_a[1]], drop));
            (!opposite(
                orient2d(p, q, project(a[ao], drop)),
                orient2d(p, q, project(b[bo], drop)),
            ))
            .then_some(IntersectionKind::Touching)
        }
        _ => {
            let (pa, pb) = (shared_a[0], shared_b[0]);
            let (a1, a2) = (a[(pa + 1) % 3], a[(pa + 2) % 3]);
            let (b1, b2) = (b[(pb + 1) % 3], b[(pb + 2) % 3]);
            if orient3d(b[0], b[1], b[2], a1) != Sign::Zero
                || orient3d(b[0], b[1], b[2], a2) != Sign::Zero
            {
                // Only opposite edges can terminate an intersection beyond the shared vertex.
                let hits = [segment_raw(a1, a2, &b), segment_raw(b1, b2, &a)];
                if hits.contains(&Some(IntersectionKind::Crossing)) {
                    Some(IntersectionKind::Crossing)
                } else if hits.contains(&Some(IntersectionKind::Touching)) {
                    Some(IntersectionKind::Touching)
                } else {
                    None
                }
            } else {
                let p = project(a[pa], drop);
                let (a1, a2) = (project(a1, drop), project(a2, drop));
                let (b1, b2) = (project(b1, drop), project(b2, drop));
                (in_cone(p, a1, a2, b1)
                    || in_cone(p, a1, a2, b2)
                    || in_cone(p, b1, b2, a1)
                    || in_cone(p, b1, b2, a2))
                .then_some(IntersectionKind::Touching)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERM: [[usize; 3]; 6] = [
        [0, 1, 2],
        [1, 2, 0],
        [2, 0, 1],
        [0, 2, 1],
        [2, 1, 0],
        [1, 0, 2],
    ];

    fn triangle() -> [Vec3; 3] {
        [
            Vec3::ZERO,
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 4.0, 0.0),
        ]
    }

    #[test]
    fn coplanar_segments_include_containment_crossings_and_boundary() {
        let t = triangle();
        for (p, q, hit) in [
            ([1.0, 1.0], [2.0, 1.0], true),  // inside: no edge crossings
            ([-1.0, 2.0], [4.0, 2.0], true), // both endpoints outside
            ([1.0, 0.0], [5.0, 0.0], true),  // collinear overlap
            ([-1.0, 0.0], [0.0, 0.0], true), // shared endpoint
            ([1.0, 1.0], [1.0, 1.0], true),  // a point
            ([5.0, 0.0], [6.0, 0.0], false), // collinear disjoint
            ([4.0, 4.0], [5.0, 5.0], false),
            ([-1.0, -1.0], [-1.0, -1.0], false),
        ] {
            let (p, q) = (Vec3::new(p[0], p[1], 0.0), Vec3::new(q[0], q[1], 0.0));
            for perm in PERM {
                for (p, q) in [(p, q), (q, p)] {
                    assert_eq!(segment_triangle(p, q, perm.map(|i| t[i])).is_some(), hit);
                }
            }
        }
    }

    #[test]
    fn noncoplanar_segments_distinguish_crossing_touching_and_separation() {
        let t = triangle();
        let p = Vec3::new(1.0, 1.0, -1.0);
        let q = Vec3::new(1.0, 1.0, 1.0);
        assert_eq!(segment_triangle(p, q, t), Some(IntersectionKind::Crossing));
        assert_eq!(
            segment_triangle(p, Vec3::new(1.0, 1.0, 0.0), t),
            Some(IntersectionKind::Touching)
        );
        assert_eq!(
            segment_triangle(Vec3::new(4.0, 4.0, -1.0), Vec3::new(4.0, 4.0, 1.0), t),
            None
        );
        assert_eq!(segment_triangle(q, q * 2.0, t), None);
    }

    #[test]
    fn triangle_pairs_are_symmetric_and_winding_and_projection_independent() {
        let a = triangle();
        let cases = [
            (a.map(|v| v + Vec3::new(0.25, 0.25, 0.0)), true),
            (a.map(|v| v * 0.25 + Vec3::new(0.5, 0.5, 0.0)), true),
            (a.map(|v| v + Vec3::new(5.0, 0.0, 0.0)), false),
            (a.map(|v| v + Vec3::new(4.0, 0.0, 0.0)), true),
            (
                a.map(|v| v + Vec3::new(0.0, 0.0, 1.0 / (1u64 << 40) as f64)),
                false,
            ),
            (
                [
                    Vec3::new(1.0, 1.0, -1.0),
                    Vec3::new(1.0, 1.0, 1.0),
                    Vec3::new(2.0, 1.0, 0.0),
                ],
                true,
            ),
        ];
        for (b, hit) in cases {
            for axis in 0..3 {
                let rotate = |v: Vec3| Vec3::new(v[axis], v[(axis + 1) % 3], v[(axis + 2) % 3]);
                for pa in PERM {
                    for pb in PERM {
                        let (a, b) = (pa.map(|i| rotate(a[i])), pb.map(|i| rotate(b[i])));
                        assert_eq!(triangle_triangle(a, b).is_some(), hit);
                        assert_eq!(triangle_triangle(a, b), triangle_triangle(b, a));
                    }
                }
            }
        }
    }

    fn check_shared(vertices: &[Vec3], a: [usize; 3], b: [usize; 3], hit: bool) {
        validate_mesh(vertices, &[a, b]).unwrap();
        for pa in PERM {
            for pb in PERM {
                let (a, b) = (pa.map(|i| a[i]), pb.map(|i| b[i]));
                assert_eq!(indexed_triangle_pair(vertices, a, b).is_some(), hit);
                assert_eq!(
                    indexed_triangle_pair(vertices, a, b),
                    indexed_triangle_pair(vertices, b, a)
                );
            }
        }
    }

    #[test]
    fn shared_edge_permits_a_hinge_but_not_a_folded_back_overlap() {
        let mut x = triangle().to_vec();
        x.push(Vec3::new(1.0, -1.0, 0.0));
        check_shared(&x, [0, 1, 2], [1, 0, 3], false);
        x[3] = Vec3::new(1.0, 1.0, 1.0);
        check_shared(&x, [0, 1, 2], [1, 0, 3], false);
        x[3] = Vec3::new(1.0, 1.0, 0.0);
        check_shared(&x, [0, 1, 2], [1, 0, 3], true);
        check_shared(&x, [0, 1, 2], [2, 1, 0], true);
    }

    #[test]
    fn shared_vertex_is_not_a_blanket_exemption() {
        let mut x = triangle().to_vec();
        x.extend([Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0)]);
        check_shared(&x, [0, 1, 2], [0, 3, 4], false);
        x[3] = Vec3::new(2.0, 0.0, 0.0); // a shared ray, but not a shared indexed edge
        check_shared(&x, [0, 1, 2], [0, 3, 4], true);
        x[3] = Vec3::new(1.0, 1.0, -1.0);
        x[4] = Vec3::new(1.0, 1.0, 1.0);
        check_shared(&x, [0, 1, 2], [0, 3, 4], true);
        x[3] = Vec3::new(-1.0, -1.0, -1.0);
        x[4] = Vec3::new(-1.0, -1.0, 1.0);
        check_shared(&x, [0, 1, 2], [0, 3, 4], false);
    }

    #[test]
    fn admission_refuses_bad_indices_nonfinite_and_degenerate_faces() {
        let t = triangle();
        assert_eq!(
            validate_mesh(&t, &[[0, 1, 9]]).unwrap_err().code,
            RefusalCode::DegenerateGeometry
        );
        assert_eq!(
            validate_mesh(&t, &[[0, 1, 1]]).unwrap_err().code,
            RefusalCode::DegenerateGeometry
        );
        let line = [
            Vec3::ZERO,
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(2.0, 2.0, 2.0),
        ];
        assert!(is_degenerate(line));
        assert_eq!(triangle_triangle(line, t), Some(IntersectionKind::Touching));
        let mut x = t.to_vec();
        x.push(Vec3::new(f64::NAN, 0.0, 0.0)); // even an unreferenced NaN is refused
        assert_eq!(
            validate_mesh(&x, &[[0, 1, 2]]).unwrap_err().code,
            RefusalCode::NonFinite
        );
        assert!(validate_mesh(&[], &[]).is_ok());
    }
}
