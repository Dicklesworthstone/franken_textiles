//! Stagnant, massless gas conduction between finite-thickness textile faces.
//!
//! This is an explicitly admitted local-normal closure, not a convection, radiation or comfort
//! model. The gas coefficient is prescribed with its source/conditions; textile thickness is read
//! from the SAME frozen selections as cloth mechanics. Neither midpoint separation nor a nominal
//! fiber density is a surface air gap. Positive affine gaps are integrated as parallel conductances.

use ftx_core::{Refusal, Vec2, Vec3};
use ftx_pattern::materials::Source;
use serde::{Deserialize, Serialize};

use super::clip::{Work, area};
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
    let n = normal / positive("gap body normal", normal.norm())?;
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
    let total = positive("gap integration polygon area", area(polygon))?;
    let mut stack = Vec::new();
    for j in 1..polygon.len() - 1 {
        let weight = (polygon[j] - polygon[0]).cross(polygon[j + 1] - polygon[0]) / (2. * total);
        if weight < 0. || !weight.is_finite() {
            return Err(invalid("gap integration requires a convex CCW region"));
        }
        if weight > 0. {
            stack.push(([gaps[0], gaps[j], gaps[j + 1]], weight, 0u8));
        }
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
