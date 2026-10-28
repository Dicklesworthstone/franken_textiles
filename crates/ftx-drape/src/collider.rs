//! Admission of prescribed body surfaces before they become cloth colliders.
//!
//! Closure and positive signed volume do not rule out a self-intersecting pose. Reuse the existing
//! frankensim-derived broad phase and exact shared-feature predicates, with linear candidate storage
//! and an explicit work budget. This checks one supplied configuration, not an anatomical model or
//! the continuous trajectory between configurations.

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
/// Open test surfaces are supported; closure, winding, volume, thickness and anatomy are separate
/// contracts. All coordinates are in the caller's units. No snapping or contact epsilon is used.
///
/// Candidate pairs are visited in original face-index order, once each. The existing owning BVH
/// retains O(faces) storage instead of first collecting a potentially quadratic pair list. Exhausting
/// `max_pairs` is a refusal, never a successful partial audit. Nothing in the input is mutated.
///
/// # Errors
/// Invalid or non-finite geometry, an empty surface, a zero work budget, an intersection, or work
/// beyond `max_pairs`. Intersection refusals identify the original pair of face indices.
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
    Ok(SurfaceAudit { pairs_tested })
}

impl Body {
    /// Audit the body's current public geometry, without relying on possibly stale cached boxes.
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
