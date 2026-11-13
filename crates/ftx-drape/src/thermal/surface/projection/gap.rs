//! Stagnant, massless gas conduction between finite-thickness textile faces.
//!
//! This is an explicitly admitted local-normal closure, not a convection, radiation or comfort
//! model. The gas coefficient is prescribed with its source/conditions; textile thickness is read
//! from the SAME frozen selections as cloth mechanics. Neither midpoint separation nor a nominal
//! fiber density is a surface air gap. Positive affine gaps are integrated as parallel conductances.

use ftx_core::{
    Refusal, Vec2, Vec3,
    exact::{Sign, orient2d_determinant},
};
use ftx_pattern::materials::Source;
use serde::{Deserialize, Serialize};

use super::clip::{Work, convex_roundoff};
use super::{ProjectionSource, invalid, positive};

/// A prescribed constant gas coefficient and explicit admission of a conduction-only approximation.
/// Conditions must state the gas/state and why this closure is being used. A source citation is not
/// evidence that natural convection, ventilation or radiation is negligible in a real garment.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conduction {
    /// Constant gas thermal conductivity in W/(m K), not an inferred textile conductivity.
    pub conductivity_w_m_k: f64,
    /// Shared source format used by material observations; no network access during a solve.
    pub source: Source,
    /// Gas composition/state, conditioning and scope of the constant-coefficient approximation.
    pub conditions: String,
    /// Explicit positive surface-gap interval in metres admitted by this model, inclusive.
    /// This is a caller-declared modeling domain, not a computed convection-onset threshold.
    pub gap_range_m: [f64; 2],
    /// Requested relative width of the numerical conductance bracket, in [1e-6, 1e-2].
    pub relative_tolerance: f64,
    /// Must be true. No material source or successful calculation silently admits this approximation.
    pub allow_conduction_only_approximation: bool,
}

impl Conduction {
    pub(super) fn validate(&self) -> Result<(), Refusal> {
        positive("gap conductivity", self.conductivity_w_m_k)?;
        let [lo, hi] = self.gap_range_m;
        if !self.allow_conduction_only_approximation
            || self.conditions.trim().is_empty()
            || self.source.reference.trim().is_empty()
            || self.source.accessed_on.trim().is_empty()
            || self.source.note.trim().is_empty()
            || !lo.is_finite()
            || lo <= 0.
            || !hi.is_finite()
            || hi < lo
            || !self.relative_tolerance.is_finite()
            || !(1e-6..=1e-2).contains(&self.relative_tolerance)
        {
            return Err(invalid(
                "stagnant gap needs source/conditions, an explicit positive domain, numerical tolerance and conduction-only consent",
            ));
        }
        Ok(())
    }
}

/// One interface's geometry and quadrature result. All areas use the projection's covered-body
/// normal plane. Numerical brackets do not represent propagated specimen or gas uncertainty.
#[derive(Clone, Debug, Serialize)]
pub struct Resolved {
    /// Owning geometric region.
    pub region: String,
    /// Inner and outer source cloth triangles, in physical layer order.
    pub cloth_triangles: [usize; 2],
    /// Smallest and largest nominal FACE separation over the entire convex region, in metres.
    pub surface_gap_m: [f64; 2],
    /// Extreme face gaps including reported thickness bounds, in metres. Admission checks these
    /// bounds too, but no probability distribution or propagated output uncertainty is implied.
    pub thickness_envelope_gap_m: [f64; 2],
    /// Inverse mean parallel conductance, in m² K/W of covered BODY area.
    /// R = 1 / mean(k/g); fixed additional interface resistance is NOT included.
    pub resistance_m2_k_w: f64,
    /// Numerical quadrature bracket for R at the NOMINAL thickness/coefficient, in m² K/W.
    /// Jensen/vertex convexity bounds are evaluated in f64, not outward-rounded interval arithmetic.
    pub numerical_resistance_bounds_m2_k_w: [f64; 2],
    /// Number of accepted integration triangles; refinement is deterministic and work-bounded.
    pub quadrature_triangles: usize,
    /// Absolute body-area change from removing roundoff-scale inward polygon corners, in m².
    /// The original projection footprint is retained; quadrature brackets cover the canonical region.
    #[serde(skip_serializing_if = "is_zero")]
    pub polygon_roundoff_m2: f64,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.
}

struct Plane {
    point: Vec3,
    normal: Vec3,
    cosine: f64,
    half: f64,
    half_bounds: [f64; 2],
}

impl Plane {
    fn new(
        source: &ProjectionSource<'_>,
        cloth: &[Vec3],
        i: usize,
        n: Vec3,
    ) -> Result<Self, Refusal> {
        let patch = &source.patches[i];
        let material = &source.layers[patch.column][0];
        let record = material.material();
        let t = positive(
            "gap sheet thickness",
            record.require("thickness", "m", material.allows_estimates())?,
        )?;
        let bounds = record.properties["thickness"].bounds.unwrap_or([t, t]);
        positive("gap lower thickness bound", bounds[0])?;
        positive("gap upper thickness bound", bounds[1])?;
        let [a, b, c] = patch.vertices.map(|i| cloth[i]);
        let normal = (b - a).cross(c - a);
        let normal = normal / positive("gap sheet normal", normal.norm())?;
        let cosine = normal.dot(n);
        let absolute = positive("gap normal cosine", cosine.abs())?;
        // Intersect the two parallel offset faces along the BODY normal, not the sheet normal.
        // Normal-distance thickness therefore contributes t/(2*|n_sheet dot n_body|).
        let half = t / (2. * absolute);
        let half_bounds = bounds.map(|v| v / (2. * absolute));
        if !half.is_finite() || half_bounds.iter().any(|v| !v.is_finite()) {
            return Err(invalid("projected textile thickness overflow"));
        }
        Ok(Self {
            point: a,
            normal,
            cosine,
            half,
            half_bounds,
        })
    }

    fn depth(&self, p: Vec3) -> f64 {
        (self.point - p).dot(self.normal) / self.cosine
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve(
    source: &ProjectionSource<'_>,
    cloth: &[Vec3],
    body: [Vec3; 3],
    polygon: &[Vec2],
    triangles: [usize; 2],
    law: &Conduction,
    work: &mut Work,
    region: &str,
) -> Result<Resolved, Refusal> {
    let [a, b, c] = body;
    let normal = (b - a).cross(c - a);
    let twice_area = positive("gap body normal", normal.norm())?;
    let n = normal / twice_area;
    let (canonical, polygon_roundoff) = convex_roundoff(polygon, work)?;
    let polygon = canonical.as_slice();
    let inner = Plane::new(source, cloth, triangles[0], n)?;
    let outer = Plane::new(source, cloth, triangles[1], n)?;
    let mut gaps = Vec::with_capacity(polygon.len());
    let mut nominal = [f64::INFINITY, f64::NEG_INFINITY];
    let mut envelope = nominal;
    for &xy in polygon {
        work.add(1)?;
        let p = a + (b - a) * xy.x + (c - a) * xy.y;
        let distance = outer.depth(p) - inner.depth(p);
        let gap = distance - inner.half - outer.half;
        let low = distance - inner.half_bounds[1] - outer.half_bounds[1];
        let high = distance - inner.half_bounds[0] - outer.half_bounds[0];
        if [gap, low, high].iter().any(|v| !v.is_finite())
            || low < law.gap_range_m[0]
            || high > law.gap_range_m[1]
        {
            return Err(invalid(format!(
                "surface gap for {region} is outside the admitted positive interval {:?} m (including thickness bounds); contact/compression/convection must not be guessed",
                law.gap_range_m
            )));
        }
        gaps.push(gap);
        nominal = [nominal[0].min(gap), nominal[1].max(gap)];
        envelope = [envelope[0].min(low), envelope[1].max(high)];
    }
    let (bounds, count) = integrate(polygon, &gaps, law, work)?;
    let mean = bounds[0] / 2. + bounds[1] / 2.;
    let resistance = positive("gap thermal resistance", 1. / mean)?;
    Ok(Resolved {
        region: region.into(),
        cloth_triangles: triangles,
        surface_gap_m: nominal,
        thickness_envelope_gap_m: envelope,
        resistance_m2_k_w: resistance,
        numerical_resistance_bounds_m2_k_w: [1. / bounds[1], 1. / bounds[0]],
        quadrature_triangles: count,
        polygon_roundoff_m2: polygon_roundoff * twice_area,
    })
}

// For positive affine gap g on a triangle, k/g is convex. Jensen gives its centroid as
// a lower average bound; the affine interpolant through vertex values gives an upper bound.
// Bisect all three edges of a rejected triangle. Each child inherits 1/4 of its parent's area.
// Local relative brackets imply the same relative bound on the positive global sum.
fn integrate(
    polygon: &[Vec2],
    gaps: &[f64],
    law: &Conduction,
    work: &mut Work,
) -> Result<([f64; 2], usize), Refusal> {
    let mut stack = Vec::new();
    let mut total = 0.;
    for j in 1..polygon.len() - 1 {
        work.add(1)?;
        let (sign, twice_area) =
            orient2d_determinant(polygon[0].into(), polygon[j].into(), polygon[j + 1].into());
        if !twice_area.is_finite()
            || sign == Sign::Negative
            || (sign == Sign::Zero && twice_area != 0.)
            || (sign == Sign::Positive && twice_area <= 0.)
        {
            return Err(invalid("gap integration requires a convex CCW region"));
        }
        if sign == Sign::Positive {
            total += twice_area;
            stack.push(([gaps[0], gaps[j], gaps[j + 1]], twice_area, 0u8));
        }
    }
    let total = positive("gap integration polygon doubled area", total)?;
    // The same robust fan areas set both numerators and denominator. Ordinary area subtraction
    // can give an exact-positive clipped sliver the wrong sign, even after hull canonicalization.
    for (_, weight, _) in &mut stack {
        *weight = positive("gap integration triangle fraction", *weight / total)?;
    }
    let mut sum = [0., 0.];
    let mut count = 0;
    while let Some(([a, b, c], weight, depth)) = stack.pop() {
        work.add(1)?;
        let lo = law.conductivity_w_m_k / (a / 3. + b / 3. + c / 3.);
        let hi = (law.conductivity_w_m_k / a) / 3.
            + (law.conductivity_w_m_k / b) / 3.
            + (law.conductivity_w_m_k / c) / 3.;
        positive("gap conductance lower bound", lo)?;
        positive("gap conductance upper bound", hi)?;
        if hi - lo <= law.relative_tolerance * lo {
            sum[0] += weight * lo;
            sum[1] += weight * hi.max(lo);
            count += 1;
        } else {
            if depth >= 24 || weight / 4. == 0. {
                return Err(invalid(
                    "gap integration did not converge within subdivision budget",
                ));
            }
            let ab = a / 2. + b / 2.;
            let bc = b / 2. + c / 2.;
            let ca = c / 2. + a / 2.;
            for child in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
                stack.push((child, weight / 4., depth + 1));
            }
        }
    }
    positive("integrated gap conductance", sum[0])?;
    positive("integrated gap conductance", sum[1])?;
    Ok((sum, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thermal::surface::{Patch, projection::clip::intersection};
    use ftx_pattern::{material_catalog::Selection, materials::Catalog};
    use serde_json::json;

    fn clipped_polygon() -> Vec<Vec2> {
        let mut polygon = vec![Vec2::ZERO, Vec2::new(1., 0.), Vec2::new(0., 1.)];
        // Three separately rounded projections of almost coincident triangle boundaries.
        // These are actual clip inputs, not a hand-edited negative quadrature weight.
        let clips = [
            [
                [0.105_995_409_376_919_18, 0.135_788_834_281_265_9],
                [0.839_251_067_023_724_4, 0.094_580_259_476_788_72],
                [0.079_829_919_221_810_61, 0.891_666_216_589_510_5],
            ],
            [
                [0.105_995_409_376_918_9, 0.135_788_834_281_266_06],
                [0.839_251_067_023_724_5, 0.094_580_259_476_788_3],
                [0.079_829_919_221_811_43, 0.891_666_216_589_510_8],
            ],
            [
                [0.105_995_409_376_919_59, 0.135_788_834_281_266_06],
                [0.839_251_067_023_724_7, 0.094_580_259_476_788_08],
                [0.079_829_919_221_810_97, 0.891_666_216_589_510_4],
            ],
        ];
        let mut work = Work::default();
        for clip in clips {
            polygon = intersection(&polygon, &clip.map(Vec2::from), &mut work).unwrap();
        }
        polygon
    }

    fn resolve_polygon(polygon: &[Vec2], slope: f64) -> Result<Resolved, Refusal> {
        let selection: Selection = serde_json::from_value(json!({
            "id":"synthetic-gap-sheet", "allow_estimates":true, "record":{
                "id":"synthetic-gap-sheet", "name":"Synthetic geometry test only", "form":"woven",
                "properties":{"thickness":{"value":0.002,"unit":"m","evidence":"estimate",
                    "source":"test","method":"Synthetic fixed face thickness"}},
                "sources":{"test":{"reference":"test:gap-roundoff","accessed_on":"2026-11-13",
                    "note":"Not measured textile data"}}
            }
        }))
        .unwrap();
        let materials = [Catalog::builtin().unwrap().resolve(&selection).unwrap()];
        let patches = [
            Patch {
                vertices: [0, 1, 2],
                reference_area_m2: 0.5,
                column: 0,
            },
            Patch {
                vertices: [3, 4, 5],
                reference_area_m2: 0.5,
                column: 0,
            },
        ];
        let source = ProjectionSource {
            vertex_count: 6,
            patches: &patches,
            layers: vec![materials.as_slice()],
            snapshot: json!({"test":"source-qualified synthetic gap geometry"}),
        };
        let body = [Vec3::ZERO, Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.)];
        let cosine = 1. / (1. + slope * slope).sqrt();
        let mut cloth = body.map(|p| Vec3::new(p.x, p.y, 0.01)).to_vec();
        cloth.extend(body.map(|p| Vec3::new(p.x, p.y, 0.015 + 0.001 / cosine + slope * p.x)));
        let law = Conduction {
            conductivity_w_m_k: 0.02,
            source: Source {
                reference: "test:gap-roundoff".into(),
                accessed_on: "2026-11-13".into(),
                note: "Synthetic mathematical gas coefficient".into(),
            },
            conditions: "Explicit synthetic conduction-only geometry check".into(),
            gap_range_m: [1e-6, 0.05],
            relative_tolerance: 1e-4,
            allow_conduction_only_approximation: true,
        };
        resolve(
            &source,
            &cloth,
            body,
            polygon,
            [0, 1],
            &law,
            &mut Work::default(),
            "clipped test region",
        )
    }

    #[test]
    fn repeated_clipping_roundoff_preserves_positive_gap_quadrature() {
        let polygon = clipped_polygon();
        assert_eq!(polygon.len(), 7);
        assert!(
            polygon[1..]
                .windows(2)
                .any(|w| { (w[0] - polygon[0]).cross(w[1] - polygon[0]) < 0. })
        );
        let original = polygon.clone();
        let flat = resolve_polygon(&polygon, 0.).unwrap();
        assert!((flat.resistance_m2_k_w - 0.2).abs() < 1e-14);
        assert!(flat.polygon_roundoff_m2 > 0. && flat.polygon_roundoff_m2 < 1e-15);
        assert_eq!(polygon, original);

        // Affine transport must not depend on which retained corner starts the polygon cycle.
        let tilted = resolve_polygon(&polygon, 0.004).unwrap();
        assert!(tilted.quadrature_triangles > 1);
        for start in 0..polygon.len() {
            let mut rotated = polygon.clone();
            rotated.rotate_left(start);
            let result = resolve_polygon(&rotated, 0.004).unwrap();
            assert_eq!(result.resistance_m2_k_w, tilted.resistance_m2_k_w);
            assert_eq!(result.surface_gap_m, tilted.surface_gap_m);
            assert_eq!(
                result.numerical_resistance_bounds_m2_k_w,
                tilted.numerical_resistance_bounds_m2_k_w
            );
        }
    }

    #[test]
    fn exact_convex_clipped_fans_keep_their_positive_area() {
        let clips = [
            [
                [0.143_247_399_944_811_28, 0.111_328_731_197_864_35],
                [0.898_078_160_511_795, 0.066_068_875_556_811_87],
                [0.127_392_531_745_136_68, 0.815_244_551_328_943],
            ],
            [
                [0.143_247_399_944_811_4, 0.111_328_731_197_862_9],
                [0.898_078_160_511_794_8, 0.066_068_875_556_812_61],
                [0.127_392_531_745_135_87, 0.815_244_551_328_942_8],
            ],
            [
                [0.143_247_399_944_813_03, 0.111_328_731_197_862_84],
                [0.898_078_160_511_795_5, 0.066_068_875_556_810_8],
                [0.127_392_531_745_136_7, 0.815_244_551_328_942],
            ],
            [
                [0.143_247_399_944_812_5, 0.111_328_731_197_863_16],
                [0.898_078_160_511_795_1, 0.066_068_875_556_811_08],
                [0.127_392_531_745_135_73, 0.815_244_551_328_943_2],
            ],
        ];
        let mut polygon = vec![Vec2::ZERO, Vec2::new(1., 0.), Vec2::new(0., 1.)];
        let mut work = Work::default();
        for clip in clips {
            polygon = intersection(&polygon, &clip.map(Vec2::from), &mut work).unwrap();
        }
        let (hull, _) = convex_roundoff(&polygon, &mut work).unwrap();
        // This surviving hull is exact-convex: its negative ordinary fan area is arithmetic
        // cancellation, not an inward corner that the canonical hull could remove.
        assert!(
            hull[1..]
                .windows(2)
                .any(|w| { (w[0] - hull[0]).cross(w[1] - hull[0]) < 0. })
        );
        let result = resolve_polygon(&polygon, 0.).unwrap();
        assert!((result.resistance_m2_k_w - 0.2).abs() < 1e-14);
        assert!(result.polygon_roundoff_m2 < 1e-15);
    }

    #[test]
    fn gap_roundoff_admission_does_not_repair_nonconvex_or_clockwise_regions() {
        let concave = [
            Vec2::ZERO,
            Vec2::new(1., 0.),
            Vec2::new(0.25, 0.25),
            Vec2::new(0., 1.),
        ];
        assert!(resolve_polygon(&concave, 0.).is_err());
        let triangle = [Vec2::ZERO, Vec2::new(1., 0.), Vec2::new(0., 1.)];
        let mut clockwise = triangle;
        clockwise.reverse();
        assert!(resolve_polygon(&clockwise, 0.).is_err());
        let ordinary = resolve_polygon(&triangle, 0.).unwrap();
        assert_eq!(ordinary.polygon_roundoff_m2, 0.);
        assert!(
            serde_json::to_value(ordinary)
                .unwrap()
                .get("polygon_roundoff_m2")
                .is_none()
        );
    }
}
