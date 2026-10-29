//! Admission of prescribed body surfaces before they become cloth colliders.
//!
//! Closure and positive signed volume do not rule out a self-intersecting pose. Reuse the existing
//! frankensim-derived broad phase and exact shared-feature predicates, with linear candidate storage
//! and an explicit work budget. This checks one supplied configuration, not an anatomical model or
//! the continuous trajectory between configurations.

use std::collections::BTreeMap;

use ftx_core::intersection::{indexed_triangle_pair, validate_mesh};
use ftx_core::{Refusal, RefusalCode, Vec3};

use crate::bvh::{QueryBvh, overlaps};
use crate::contact::Body;

// Static scene admission is bounded independently of the number of subsequent cloth steps.
pub(crate) const STATIC_SURFACE_PAIR_BUDGET: usize = 1_000_000;

/// Work performed by a completed body-surface intersection audit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceAudit {
    /// Unique overlapping triangle-box pairs actually tested, including legal shared features.
    pub pairs_tested: usize,
}

/// Refuse crossings, coincident faces and contact beyond a shared indexed vertex or edge.
/// Require coherent manifold topology and outward winding on every closed component. Open test
/// surfaces remain supported, but mixing open and closed components is refused: the existing
/// contact containment test is defined only when the entire collider is closed. Thickness and
/// anatomy remain separate contracts. No snapping, reorientation or contact epsilon is used.
///
/// Candidate pairs are visited in original face-index order, once each. The existing owning BVH
/// retains O(faces) storage instead of first collecting a potentially quadratic pair list. Exhausting
/// `max_pairs` is a refusal, never a successful partial audit. Nothing in the input is mutated.
///
/// # Errors
/// Invalid or non-finite geometry, an empty surface, a zero work budget, an intersection, or work
/// beyond `max_pairs`, pinched/non-manifold topology or inward closed components. Intersection
/// refusals identify the original pair of face indices; topology refusals identify source features.
///
/// # Panics
/// On predicate arithmetic outside `ftx_core::exact`'s documented normal-f64 domain.
pub fn audit_surface(
    vertices: &[Vec3],
    triangles: &[[usize; 3]],
    max_pairs: usize,
) -> Result<SurfaceAudit, Refusal> {
    if max_pairs == 0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "body surface audit requires a positive candidate-pair budget",
        ));
    }
    if triangles.is_empty() {
        return Err(Refusal::new(
            RefusalCode::DegenerateGeometry,
            "body surface audit requires at least one triangle",
        ));
    }
    validate_mesh(vertices, triangles)?;
    let boxes: Vec<_> = triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| vertices[i]);
            (
                Vec3::new(
                    a.x.min(b.x).min(c.x),
                    a.y.min(b.y).min(c.y),
                    a.z.min(b.z).min(c.z),
                ),
                Vec3::new(
                    a.x.max(b.x).max(c.x),
                    a.y.max(b.y).max(c.y),
                    a.z.max(b.z).max(c.z),
                ),
            )
        })
        .collect();
    let tree = QueryBvh::build(&boxes);
    let mut candidates = Vec::new();
    let mut pairs_tested = 0;
    for (a, &bounds) in boxes.iter().enumerate() {
        tree.query(bounds, &mut candidates);
        for &b in &candidates {
            if b <= a || !overlaps(bounds, boxes[b]) {
                continue;
            }
            if pairs_tested == max_pairs {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    format!("body surface audit exceeds its {max_pairs} candidate-pair budget"),
                )
                .repair("increase the explicit audit budget or use a simpler qualified collider"));
            }
            pairs_tested += 1;
            if let Some(kind) = indexed_triangle_pair(vertices, triangles[a], triangles[b]) {
                return Err(Refusal::new(
                    RefusalCode::IntersectingStart,
                    format!(
                        "body triangles {a} and {b} intersect beyond their shared mesh feature ({kind:?})"
                    ),
                )
                .repair(
                    "use a nonintersecting body pose or repair the source collider; no vertices were moved",
                ));
            }
        }
    }
    audit_topology(vertices, triangles)?;
    Ok(SurfaceAudit { pairs_tested })
}

// Triangle separation alone permits reversed faces, several fans meeting at one indexed vertex,
// and three sheets sharing an edge. Those do not provide a coherent collider boundary. Each face
// and corner is visited a bounded number of times; the maps and traversal storage are O(mesh).
fn audit_topology(vertices: &[Vec3], triangles: &[[usize; 3]]) -> Result<(), Refusal> {
    let bad = |message| Refusal::new(RefusalCode::DegenerateGeometry, message);
    let mut edges = BTreeMap::<(usize, usize), (usize, usize)>::new();
    let mut neighbours = vec![[None; 3]; triangles.len()];
    let mut first_face = vec![usize::MAX; vertices.len()];
    let mut incident = vec![0usize; vertices.len()];
    for (face, t) in triangles.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            first_face[a] = first_face[a].min(face);
            incident[a] += 1;
            let key = (a.min(b), a.max(b));
            match edges.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert((face, k));
                }
                std::collections::btree_map::Entry::Occupied(entry) => {
                    let &(other, edge) = entry.get();
                    if neighbours[other][edge].is_some() {
                        return Err(bad(format!(
                            "body edge {}–{} has more than two incident faces",
                            key.0, key.1
                        )));
                    }
                    if triangles[other][edge] == a {
                        return Err(bad(format!(
                            "body faces {other} and {face} traverse shared edge {}–{} in the same direction",
                            key.0, key.1
                        )));
                    }
                    neighbours[face][k] = Some(other);
                    neighbours[other][edge] = Some(face);
                }
            }
        }
    }

    // The incident faces of a manifold vertex form one chain (boundary) or cycle (interior).
    // Edge degree alone misses two otherwise closed shells joined only at one pinched vertex.
    let mut visited = vec![usize::MAX; triangles.len()];
    let mut stack = Vec::new();
    for (vertex, &first) in first_face.iter().enumerate() {
        if first == usize::MAX {
            continue;
        }
        stack.push(first);
        let mut count = 0;
        while let Some(face) = stack.pop() {
            if visited[face] == vertex {
                continue;
            }
            visited[face] = vertex;
            count += 1;
            let t = triangles[face];
            for k in 0..3 {
                if (t[k] == vertex || t[(k + 1) % 3] == vertex)
                    && let Some(next) = neighbours[face][k]
                {
                    stack.push(next);
                }
            }
        }
        if count != incident[vertex] {
            return Err(bad(format!(
                "body vertex {vertex} has disconnected incident face fans"
            )));
        }
    }

    let mut seen = vec![false; triangles.len()];
    let mut component = Vec::new();
    let (mut has_open, mut has_closed) = (false, false);
    for first in 0..triangles.len() {
        if seen[first] {
            continue;
        }
        component.clear();
        stack.push(first);
        let mut closed = true;
        while let Some(face) = stack.pop() {
            if seen[face] {
                continue;
            }
            seen[face] = true;
            component.push(face);
            for neighbour in neighbours[face] {
                if let Some(next) = neighbour {
                    stack.push(next);
                } else {
                    closed = false;
                }
            }
        }
        if closed {
            has_closed = true;
            // Use a component-local origin: translating a small closed mesh must not reverse
            // its volume through cancellation of large world-coordinate tetrahedra.
            let origin = vertices[triangles[first][0]];
            let mut volume = 0.0;
            for &face in &component {
                let [a, b, c] = triangles[face].map(|i| vertices[i] - origin);
                volume += a.cross(b).dot(c) / 6.0;
            }
            if !volume.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    format!("closed body component at face {first} has non-finite signed volume"),
                ));
            }
            if volume <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::InwardWound,
                    format!(
                        "closed body component at face {first} has nonpositive signed volume {volume}"
                    ),
                )
                .repair(
                    "correct the source component winding; no faces were reversed automatically",
                ));
            }
        } else {
            has_open = true;
        }
    }
    if has_open && has_closed {
        return Err(bad(
            "body mixes open and closed components; whole-body containment would be disabled"
                .to_string(),
        )
        .repair("supply a coherent closed collider or a separately scoped open test surface"));
    }
    Ok(())
}

impl Body {
    /// Audit current public surface geometry, topology and winding, not possibly stale cached boxes.
    /// This does not check cached contact topology or certify a continuous motion path.
    ///
    /// # Errors
    /// As [`audit_surface`].
    ///
    /// # Panics
    /// As [`audit_surface`].
    pub fn audit_self_intersections(&self, max_pairs: usize) -> Result<SurfaceAudit, Refusal> {
        audit_surface(&self.vertices, &self.triangles, max_pairs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetrahedron() -> (Vec<Vec3>, Vec<[usize; 3]>) {
        (
            vec![
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        )
    }

    #[test]
    fn legal_shared_features_complete_at_the_exact_budget() {
        let (vertices, triangles) = tetrahedron();
        assert_eq!(
            audit_surface(&vertices, &triangles, 6)
                .unwrap()
                .pairs_tested,
            6
        );
        assert_eq!(
            audit_surface(&vertices, &triangles, 5).unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
        let body = Body::new(vertices, triangles, 0.01).unwrap();
        assert_eq!(body.audit_self_intersections(6).unwrap().pairs_tested, 6);
    }

    #[test]
    fn reversed_face_cannot_hide_inside_points_from_contact() {
        let (vertices, mut faces) = tetrahedron();
        faces[0].swap(1, 2);
        let body = Body::new(vertices, faces, 0.01).unwrap();
        assert!(body.closed); // edge multiplicities alone still say "closed"
        let winding = body.winding_number(Vec3::new(0.1, 0.1, 0.1));
        assert!(winding > 0.0 && winding < 0.5); // an actual interior point would be missed
        let before = (body.vertices.clone(), body.triangles.clone());
        let error = body.audit_self_intersections(6).unwrap_err();
        assert_eq!(error.code, RefusalCode::DegenerateGeometry);
        assert!(error.message.contains("same direction"));
        assert_eq!((body.vertices, body.triangles), before);
    }

    #[test]
    fn every_closed_component_must_be_outward_even_when_total_volume_is_positive() {
        let (mut vertices, mut faces) = tetrahedron();
        let (small, small_faces) = tetrahedron();
        vertices.extend(
            small
                .into_iter()
                .map(|p| p * 0.5 + Vec3::new(3.0, 0.0, 0.0)),
        );
        faces.extend(
            small_faces
                .into_iter()
                .map(|[a, b, c]| [a + 4, c + 4, b + 4]),
        );
        let volume: f64 = faces
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| vertices[i]);
                a.cross(b).dot(c) / 6.0
            })
            .sum();
        assert!(volume > 0.0);
        for _ in 0..2 {
            assert_eq!(
                audit_surface(&vertices, &faces, 12).unwrap_err().code,
                RefusalCode::InwardWound
            );
            faces.reverse();
        }
        let (vertices, mut faces) = tetrahedron();
        for f in &mut faces {
            f.swap(1, 2);
        }
        assert_eq!(
            audit_surface(&vertices, &faces, 6).unwrap_err().code,
            RefusalCode::InwardWound
        );
    }

    #[test]
    fn pinched_vertex_and_branched_edge_refuse_without_geometric_crossings() {
        let (mut vertices, mut faces) = tetrahedron();
        vertices.extend([
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
        ]);
        // A second outward tetrahedron touches the first only at the shared vertex 0.
        let map = [0, 4, 5, 6];
        let (_, other_faces) = tetrahedron();
        faces.extend(
            other_faces
                .into_iter()
                .map(|[a, b, c]| [map[a], map[c], map[b]]),
        );
        let error = audit_surface(&vertices, &faces, 100).unwrap_err();
        assert_eq!(error.code, RefusalCode::DegenerateGeometry);
        assert!(error.message.contains("vertex 0"));
        assert!(error.message.contains("disconnected"));
        let error = audit_surface(&vertices, &[[0, 1, 2], [0, 4, 5]], 1).unwrap_err();
        assert_eq!(error.code, RefusalCode::DegenerateGeometry);
        assert!(error.message.contains("disconnected"));

        let vertices = [
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, -1.0, 0.0),
        ];
        let faces = [[0, 1, 2], [1, 0, 3], [0, 1, 4]];
        let error = audit_surface(&vertices, &faces, 3).unwrap_err();
        assert_eq!(error.code, RefusalCode::DegenerateGeometry);
        assert!(error.message.contains("more than two"));
    }

    #[test]
    fn open_appendage_cannot_disable_closed_body_containment() {
        let (mut vertices, mut faces) = tetrahedron();
        vertices.extend([
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(3.0, 1.0, 0.0),
        ]);
        faces.push([4, 5, 6]);
        let body = Body::new(vertices, faces, 0.01).unwrap();
        assert!(!body.closed); // otherwise this disables the whole-body inside audit
        let error = body.audit_self_intersections(6).unwrap_err();
        assert_eq!(error.code, RefusalCode::DegenerateGeometry);
        assert!(error.message.contains("mixes open and closed"));
        assert_eq!(
            audit_surface(&body.vertices[4..], &[[0, 1, 2]], 1)
                .unwrap()
                .pairs_tested,
            0
        );
    }

    #[test]
    fn outward_shells_keep_translation_order_and_disconnected_component_support() {
        for shift in [Vec3::ZERO, Vec3::new(1e9, -1e9, 1e9)] {
            let (mut vertices, mut faces) = tetrahedron();
            let (other, other_faces) = tetrahedron();
            vertices.extend(other.into_iter().map(|p| p + Vec3::new(3.0, 0.0, 0.0)));
            faces.extend(other_faces.into_iter().map(|t| t.map(|i| i + 4)));
            for p in &mut vertices {
                *p += shift;
            }
            for _ in 0..2 {
                assert_eq!(
                    audit_surface(&vertices, &faces, 12).unwrap().pairs_tested,
                    12
                );
                faces.reverse();
            }
        }
    }

    #[test]
    fn coplanar_neighbours_are_legal_but_folded_neighbours_are_not() {
        let mut vertices = vec![
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        ];
        let faces = [[0, 1, 2], [1, 0, 3]];
        assert_eq!(audit_surface(&vertices, &faces, 1).unwrap().pairs_tested, 1);
        vertices[3] = Vec3::new(0.25, 0.25, 0.0);
        let refusal = audit_surface(&vertices, &faces, 1).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::IntersectingStart);
        assert!(refusal.message.contains("body triangles 0 and 1"));
    }

    #[test]
    fn crossing_and_coincident_surfaces_are_refused_without_mutation() {
        let a = [
            Vec3::ZERO,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
        ];
        for b in [
            [
                Vec3::new(0.5, 0.5, -1.0),
                Vec3::new(0.5, 0.5, 1.0),
                Vec3::new(0.5, 1.0, 0.0),
            ],
            a,
        ] {
            let vertices: Vec<_> = a.into_iter().chain(b).collect();
            let original = vertices.clone();
            for faces in [[[0, 1, 2], [3, 4, 5]], [[5, 4, 3], [2, 1, 0]]] {
                assert_eq!(
                    audit_surface(&vertices, &faces, 1).unwrap_err().code,
                    RefusalCode::IntersectingStart
                );
                assert_eq!(vertices, original);
            }
        }
    }

    #[test]
    fn disjoint_surfaces_do_not_spend_narrow_phase_budget() {
        let mut vertices = Vec::new();
        let mut faces = Vec::new();
        for i in 0..512 {
            let base = vertices.len();
            let x = 3.0 * f64::from(i);
            vertices.extend([
                Vec3::new(x, 0.0, 0.0),
                Vec3::new(x + 1.0, 0.0, 0.0),
                Vec3::new(x, 1.0, 0.0),
            ]);
            faces.push([base, base + 1, base + 2]);
        }
        assert_eq!(audit_surface(&vertices, &faces, 1).unwrap().pairs_tested, 0);
    }

    #[test]
    fn invalid_geometry_is_refused_before_broad_phase() {
        let (mut vertices, faces) = tetrahedron();
        assert_eq!(
            audit_surface(&vertices, &faces, 0).unwrap_err().code,
            RefusalCode::OutOfRange
        );
        assert_eq!(
            audit_surface(&vertices, &[], 1).unwrap_err().code,
            RefusalCode::DegenerateGeometry
        );
        assert_eq!(
            audit_surface(&vertices, &[[0, 0, 1]], 1).unwrap_err().code,
            RefusalCode::DegenerateGeometry
        );
        assert_eq!(
            audit_surface(&vertices, &[[0, 1, 4]], 1).unwrap_err().code,
            RefusalCode::DegenerateGeometry
        );
        vertices[0].z = f64::NAN;
        assert_eq!(
            audit_surface(&vertices, &faces, 10).unwrap_err().code,
            RefusalCode::NonFinite
        );
    }
}
