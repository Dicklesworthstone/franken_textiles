//! Fixed material regions selected in a physical pattern instance's flat rest coordinates.
//!
//! A selector owns entire retained triangles by their centroids. It never clips a triangle,
//! invents contact pressure, or follows the current spatial position of moving cloth.

use ftx_core::{
    Refusal, RefusalCode, Vec2,
    exact::{Sign, orient2d, orient2d_triangle_centroid},
};
use serde::{Deserialize, Serialize};

const MAX_COORDINATE_MM: f64 = 1_000_000.;
// Keep the exact orientation predicate's products away from underflow as well as overflow.
const MIN_NONZERO_COORDINATE_MM: f64 = 1e-100;

/// Maximum polygon-edge/retained-triangle pairs evaluated by one regional partition.
pub const MAX_REGION_EDGE_TESTS: usize = 16_777_216;

/// A bounded convex region in the physical instance's mirrored and scaled flat millimetres.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    /// Must be `whole_triangle_centroid`; boundary centroids are included.
    pub kind: String,
    /// Three to 32 distinct vertices in either winding order, without a repeated closing point.
    pub polygon_mm: Vec<Vec2>,
}

/// A validated convex selector. Validation is independent of the current draped geometry.
#[derive(Clone, Debug)]
pub struct PreparedSelector {
    polygon: Vec<Vec2>,
    winding: Sign,
    min: Vec2,
    max: Vec2,
}

fn invalid(message: &str) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn point_in_domain(point: Vec2) -> bool {
    [point.x, point.y].into_iter().all(|coordinate| {
        coordinate.is_finite()
            && coordinate.abs() <= MAX_COORDINATE_MM
            && (coordinate == 0. || coordinate.abs() >= MIN_NONZERO_COORDINATE_MM)
    })
}

impl PreparedSelector {
    /// Validate a convex polygon once. Every edge must support all its other vertices on the
    /// same side; checking only successive turns would also admit some self-crossing polygons.
    pub fn new(selector: &Selector) -> Result<Self, Refusal> {
        let polygon = &selector.polygon_mm;
        if selector.kind != "whole_triangle_centroid"
            || !(3..=32).contains(&polygon.len())
            || polygon.iter().any(|&point| !point_in_domain(point))
            || polygon
                .iter()
                .enumerate()
                .any(|(i, point)| polygon[..i].contains(point))
        {
            return Err(invalid(
                "compression regions need whole_triangle_centroid and 3..32 distinct finite flat-mm vertices within the supported coordinate domain",
            ));
        }
        let mut winding = Sign::Zero;
        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];
            for &point in polygon {
                let sign = orient2d(a.into(), b.into(), point.into());
                if sign == Sign::Zero {
                    continue;
                }
                if winding == Sign::Zero {
                    winding = sign;
                } else if winding != sign {
                    return Err(invalid(
                        "compression region polygon must be convex and non-self-crossing",
                    ));
                }
            }
        }
        if winding == Sign::Zero {
            return Err(invalid("compression region polygon has zero area"));
        }
        let mut min = polygon[0];
        let mut max = polygon[0];
        for &point in polygon.iter().skip(1) {
            min.x = min.x.min(point.x);
            min.y = min.y.min(point.y);
            max.x = max.x.max(point.x);
            max.y = max.y.max(point.y);
        }
        Ok(Self {
            polygon: polygon.clone(),
            winding,
            min,
            max,
        })
    }

    /// Select a whole, nondegenerate retained triangle by its flat centroid. Boundary inclusion
    /// is deliberate: a caller partitioning multiple regions must refuse a multiply selected patch.
    pub fn contains_triangle_centroid(&self, triangle_mm: [Vec2; 3]) -> Result<bool, Refusal> {
        triangle_centroid_mm(triangle_mm)?;
        // Use the source triangle's exact coordinate bounds, never the rounded centroid, for
        // this shortcut. A disjoint triangle box also excludes its mathematical centroid.
        if triangle_mm.iter().all(|point| point.x < self.min.x)
            || triangle_mm.iter().all(|point| point.x > self.max.x)
            || triangle_mm.iter().all(|point| point.y < self.min.y)
            || triangle_mm.iter().all(|point| point.y > self.max.y)
        {
            return Ok(false);
        }
        Ok((0..self.polygon.len()).all(|i| {
            let sign = orient2d_triangle_centroid(
                self.polygon[i].into(),
                self.polygon[(i + 1) % self.polygon.len()].into(),
                triangle_mm.map(Into::into),
            );
            sign == Sign::Zero || sign == self.winding
        }))
    }
}

/// Validate a retained flat triangle and compute its display centroid in a canonical corner order.
/// This also permits source replay to validate triangles in an uncompressed remainder.
/// Membership uses exact centroid orientation, not this rounded display value.
pub fn triangle_centroid_mm(mut triangle_mm: [Vec2; 3]) -> Result<Vec2, Refusal> {
    if triangle_mm.iter().any(|&point| !point_in_domain(point)) {
        return Err(invalid(
            "compression source triangles need finite flat-mm coordinates within the supported domain",
        ));
    }
    if orient2d(
        triangle_mm[0].into(),
        triangle_mm[1].into(),
        triangle_mm[2].into(),
    ) == Sign::Zero
    {
        return Err(invalid("compression source triangle has zero flat area"));
    }
    triangle_mm.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let [a, b, c] = triangle_mm;
    // The coordinate bound also bounds this sum. Sum before division so an exactly cancelling
    // zero-boundary centroid is not displaced by separately rounded thirds.
    let center = (a + b + c) / 3.;
    if !point_in_domain(center) {
        return Err(invalid(
            "compression source centroid leaves the supported domain",
        ));
    }
    Ok(center)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(points: &[[f64; 2]]) -> Selector {
        Selector {
            kind: "whole_triangle_centroid".into(),
            polygon_mm: points.iter().copied().map(Vec2::from).collect(),
        }
    }

    #[test]
    fn whole_centroid_semantics_include_boundary_and_ignore_triangle_clipping() {
        let prepared =
            PreparedSelector::new(&selector(&[[0., 0.], [3., 0.], [3., 3.], [0., 3.]])).unwrap();
        for points in [
            [[-2., 1.], [4., 1.], [1., 4.]],
            [[-1., 0.], [1., 0.], [0., 3.]],
            [[3., 0.], [2., 3.], [4., 3.]],
        ] {
            assert!(
                prepared
                    .contains_triangle_centroid(points.map(Vec2::from))
                    .unwrap()
            );
        }
        assert!(
            !prepared
                .contains_triangle_centroid([[2., 0.], [8., 0.], [2., 3.]].map(Vec2::from))
                .unwrap()
        );
    }

    #[test]
    fn exact_shared_diagonal_boundary_selects_both_regions_despite_rounded_thirds() {
        let triangle = [[0., 0.], [2., 0.], [0., 6.]].map(Vec2::from);
        // Its exact centroid (2/3, 2) lies on y=3x. Rounded binary64 thirds lie strictly to one
        // side under an exact point predicate, which must not conceal an overlapping selection.
        for points in [
            [[0., 0.], [1., 3.], [-4., 3.], [-4., 0.]],
            [[0., 0.], [4., 0.], [4., 3.], [1., 3.]],
        ] {
            let mut region = selector(&points);
            assert!(
                PreparedSelector::new(&region)
                    .unwrap()
                    .contains_triangle_centroid(triangle)
                    .unwrap()
            );
            region.polygon_mm.reverse();
            assert!(
                PreparedSelector::new(&region)
                    .unwrap()
                    .contains_triangle_centroid(triangle)
                    .unwrap()
            );
        }
    }

    #[test]
    fn winding_and_corner_order_do_not_change_membership() {
        let mut source = selector(&[[0., 0.], [4., 1.], [5., 4.], [1., 3.]]);
        let forward = PreparedSelector::new(&source).unwrap();
        source.polygon_mm.reverse();
        let reverse = PreparedSelector::new(&source).unwrap();
        for triangle in [
            [[0.2, 0.4], [3.8, 1.1], [1.7, 2.4]],
            [[10., 0.], [11., 1.], [12., 0.]],
        ] {
            let triangle = triangle.map(Vec2::from);
            let expected = forward.contains_triangle_centroid(triangle).unwrap();
            let center = triangle_centroid_mm(triangle).unwrap();
            for order in [
                [0, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ] {
                let reordered = order.map(|index| triangle[index]);
                assert_eq!(triangle_centroid_mm(reordered).unwrap(), center);
                assert_eq!(
                    forward.contains_triangle_centroid(reordered).unwrap(),
                    expected
                );
                assert_eq!(
                    reverse.contains_triangle_centroid(reordered).unwrap(),
                    expected
                );
            }
        }
    }

    #[test]
    fn rejects_nonconvex_crossed_repeated_and_degenerate_polygons() {
        for points in [
            vec![[0., 0.], [2., 0.]],
            vec![[0., 0.], [1., 0.], [2., 0.]],
            vec![[0., 0.], [2., 0.], [2., 2.], [0., 0.]],
            vec![[0., 0.], [2., 2.], [2., 0.], [0., 2.]],
            vec![[0., 0.], [2., 0.], [1., 1.], [2., 2.], [0., 2.]],
            vec![[0., 0.], [f64::NAN, 0.], [0., 2.]],
            vec![[0., 0.], [1_000_001., 0.], [0., 2.]],
            vec![[0., 0.], [1e-200, 0.], [0., 2.]],
        ] {
            assert!(PreparedSelector::new(&selector(&points)).is_err());
        }
        let mut wrong_kind = selector(&[[0., 0.], [2., 0.], [0., 2.]]);
        wrong_kind.kind = "clip".into();
        assert!(PreparedSelector::new(&wrong_kind).is_err());
        assert!(
            serde_json::from_value::<Selector>(serde_json::json!({
                "kind":"whole_triangle_centroid", "polygon_mm":[[0,0],[1,0],[0,1]],
                "padding_mm": 1
            }))
            .is_err()
        );
    }

    #[test]
    fn permits_collinear_boundary_vertices_and_refuses_invalid_triangles() {
        let prepared = PreparedSelector::new(&selector(&[
            [0., 0.],
            [1., 0.],
            [2., 0.],
            [2., 2.],
            [0., 2.],
        ]))
        .unwrap();
        assert!(
            prepared
                .contains_triangle_centroid([[0., 0.], [2., 0.], [0., 2.]].map(Vec2::from))
                .unwrap()
        );
        for triangle in [
            [[0., 0.], [1., 0.], [2., 0.]],
            [[0., 0.], [1., f64::INFINITY], [2., 0.]],
            [[0., 0.], [1., 0.], [2., 1_000_001.]],
        ] {
            assert!(
                prepared
                    .contains_triangle_centroid(triangle.map(Vec2::from))
                    .is_err()
            );
        }
    }
}
