//! Source-bearing nonlinear sheet response, compiled once and evaluated without allocations.
//!
//! Work-conjugate second Piola resultants versus Green strain, NOT tensile force versus engineering
//! extension. The separable hyperelastic model includes constant warp/weft coupling; it does not
//! identify yarn mechanics, irreversible deformation or a loading/unloading hysteresis loop.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use crate::materials::{Evidence, Quantity, Source};

/// Fixes work conjugacy: [E11, E22, 2 E12] versus second-Piola sheet resultants in N/m.
pub const MODEL: &str = "tabulated_green_strain/1";

/// A validated material-frame Green-strain box, independent of a constitutive interpolation.
/// Bounds constrain physical `[E11, E22, 2 E12]` before numerical compression softening.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrainDomain([[f64; 2]; 3]);

impl StrainDomain {
    /// Preserve the authored finite, ordered bounds exactly; never widen them to include zero.
    ///
    /// # Errors
    /// Nonfinite or reversed intervals.
    pub fn new(ranges: [[f64; 2]; 3]) -> Result<Self, Refusal> {
        if ranges
            .iter()
            .any(|[lo, hi]| !lo.is_finite() || !hi.is_finite() || lo > hi)
        {
            return Err(invalid(
                "Green-strain domain needs three finite ordered intervals",
            ));
        }
        Ok(Self(ranges))
    }

    /// The original closed intervals in warp, weft and engineering-shear order.
    #[must_use]
    pub fn ranges(self) -> [[f64; 2]; 3] {
        self.0
    }

    /// Whether the entire physical strain is characterized; no clamping or tolerance expansion.
    #[must_use]
    pub fn contains(self, strain: [f64; 3]) -> bool {
        strain
            .iter()
            .zip(self.0)
            .all(|(&e, [lo, hi])| e.is_finite() && e >= lo && e <= hi)
    }
}

/// Explicit interpolation authority; omitting it preserves the original cubic response.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    /// Monotone cubic stress, with continuous tangent and analytically integrated energy.
    #[default]
    MonotoneCubic,
    /// Linear stress, analytic work, right-hand tangent at interior knots. Uses the
    /// `frankensim`-derived core response kernel; energy is C1, not C2 at knots.
    PiecewiseLinear,
}

impl Interpolation {
    fn is_default(&self) -> bool {
        *self == Self::MonotoneCubic
    }
}

/// A signed material-axis stress curve at zero strain in the other two components.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StressCurve {
    /// Strictly increasing [Green strain, second Piola resultant in N/m] pairs. Include [0,0],
    /// at least one negative and positive strain, and at most 128 points. Shear strain is 2 E12.
    pub points: Vec<[f64; 2]>,
    /// Key in the enclosing material's source map.
    pub source: String,
    /// Measured, published or estimated origin of the curve.
    pub evidence: Evidence,
    /// Test/reconstruction method, including conversion from measured nominal loads when necessary.
    pub method: String,
    /// Specimen, conditioning, loading branch, rate and other qualifications.
    #[serde(default)]
    pub conditions: BTreeMap<String, String>,
}

/// Replaces the four scalar membrane properties, never adds hidden stiffness to them.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabulatedMembrane {
    /// Must be `tabulated_green_strain/1`.
    pub model: String,
    /// Interpolation changes the constitutive law and therefore its source-snapshot hash.
    #[serde(default, skip_serializing_if = "Interpolation::is_default")]
    pub interpolation: Interpolation,
    /// Warp/wale normal response, holding E22 and shear at zero.
    pub warp: StressCurve,
    /// Weft/course normal response, holding E11 and shear at zero.
    pub weft: StressCurve,
    /// Signed shear response versus gamma = 2 E12, holding normal strains at zero.
    pub shear: StressCurve,
    /// Constant cross derivative dS11/dE22 = dS22/dE11 in N/m. Zero is an explicit observation.
    pub coupling: Quantity,
}

/// Energy per reference area and its exact first/second derivatives with respect to Green strain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Response {
    /// Energy per undeformed area, J/m² (equivalently N/m).
    pub energy: f64,
    /// Work-conjugate stress [S11, S22, S12], N/m.
    pub stress: [f64; 3],
    /// Material tangent in [E11, E22, 2 E12] order, N/m.
    pub tangent: [[f64; 3]; 3],
}

#[derive(Clone, Debug, PartialEq)]
struct Interval {
    lo: f64,
    hi: f64,
    // Stress polynomial in t=(strain-lo)/(hi-lo); exact integral defines the energy.
    c: [f64; 4],
    energy_lo: f64,
}

impl Interval {
    fn integral(&self, t: f64) -> f64 {
        let [a, b, c, d] = self.c;
        (self.hi - self.lo) * t * (a + t * (b / 2.0 + t * (c / 3.0 + t * d / 4.0)))
    }

    fn evaluate(&self, x: f64) -> [f64; 3] {
        let t = (x - self.lo) / (self.hi - self.lo);
        let [a, b, c, d] = self.c;
        [
            self.energy_lo + self.integral(t),
            a + t * (b + t * (c + t * d)),
            (b + t * (2.0 * c + t * 3.0 * d)) / (self.hi - self.lo),
        ]
    }

    fn min_tangent(&self) -> f64 {
        let [_, b, c, d] = self.c;
        let h = self.hi - self.lo;
        let mut min = (b / h).min((b + 2.0 * c + 3.0 * d) / h);
        if d != 0.0 {
            let t = -c / (3.0 * d);
            if t > 0.0 && t < 1.0 {
                min = min.min((b + t * (2.0 * c + t * 3.0 * d)) / h);
            }
        }
        min
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Curve {
    // Integrate outwards from the origin on both sides, avoiding cancellation at small strains.
    positive: Vec<Interval>,
    negative: Vec<Interval>,
    range: [f64; 2],
    minimum: f64,
    origin_tangent: f64,
    // Only the explicitly selected linear mode uses this kernel. Existing cubic arithmetic stays.
    linear: Option<ftx_core::response_curve::ResponseCurve>,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn evidence(
    source: &str,
    kind: Evidence,
    method: &str,
    sources: &BTreeMap<String, Source>,
    allow: bool,
) -> Result<(), Refusal> {
    if method.trim().is_empty()
        || method.len() > 4096
        || !sources.get(source).is_some_and(|s| {
            [&s.reference, &s.note, &s.accessed_on]
                .iter()
                .all(|s| !s.trim().is_empty() && s.len() <= 4096)
        })
    {
        return Err(invalid(
            "nonlinear membrane observation needs a method and resolved source",
        ));
    }
    if kind == Evidence::Estimate && !allow {
        return Err(invalid(
            "nonlinear membrane observation is an estimate; explicit opt-in required",
        ));
    }
    Ok(())
}

impl StressCurve {
    fn compile(
        &self,
        sources: &BTreeMap<String, Source>,
        allow: bool,
        interpolation: Interpolation,
    ) -> Result<Curve, Refusal> {
        evidence(&self.source, self.evidence, &self.method, sources, allow)?;
        if self.conditions.len() > 64
            || self.conditions.iter().any(|(k, v)| {
                k.trim().is_empty() || v.trim().is_empty() || k.len() > 4096 || v.len() > 4096
            })
        {
            return Err(invalid("nonlinear curve conditioning metadata is invalid"));
        }
        let p = &self.points;
        if !(3..=128).contains(&p.len()) || p.iter().flatten().any(|v| !v.is_finite()) {
            return Err(invalid(
                "stress curve needs 3..128 finite strain/resultant pairs",
            ));
        }
        let origin = p
            .iter()
            .position(|&v| v == [0.0, 0.0])
            .ok_or_else(|| invalid("stress curve needs an explicit stress-free [0,0] origin"))?;
        if origin == 0 || origin + 1 == p.len() {
            return Err(invalid(
                "stress curve must characterize both negative and positive strain",
            ));
        }
        if interpolation == Interpolation::PiecewiseLinear {
            let linear = ftx_core::response_curve::ResponseCurve::new(p)?;
            return Ok(Curve {
                positive: Vec::new(),
                negative: Vec::new(),
                range: linear.domain(),
                minimum: linear.minimum_tangent(),
                origin_tangent: linear.evaluate(0.)?.tangent,
                linear: Some(linear),
            });
        }
        let slopes = p
            .windows(2)
            .map(|w| {
                let dx = w[1][0] - w[0][0];
                let dy = w[1][1] - w[0][1];
                let slope = dy / dx;
                if dx <= 0.0
                    || dy <= 0.0
                    || !dx.is_finite()
                    || !dy.is_finite()
                    || !slope.is_finite()
                    || slope <= 0.0
                {
                    return Err(invalid(
                        "strain and stress knots must be strictly increasing and representable",
                    ));
                }
                Ok(slope)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut derivatives = vec![slopes[0]];
        for w in slopes.windows(2) {
            // Harmonic slopes lie below twice either secant, preserving monotonicity of the cubic.
            let d = 2.0 / (1.0 / w[0] + 1.0 / w[1]);
            if !d.is_finite() || d <= 0.0 {
                return Err(invalid("stress-curve tangent is not representable"));
            }
            derivatives.push(d);
        }
        derivatives.push(*slopes.last().unwrap());
        let half = |points: Vec<[f64; 2]>, slopes: Vec<f64>| -> Result<Vec<Interval>, Refusal> {
            let mut intervals = Vec::new();
            let mut energy_lo = 0.0;
            for (i, w) in points.windows(2).enumerate() {
                let h = w[1][0] - w[0][0];
                let delta = w[1][1] - w[0][1];
                let s = Interval {
                    lo: w[0][0],
                    hi: w[1][0],
                    energy_lo,
                    c: [
                        w[0][1],
                        h * slopes[i],
                        3.0 * delta - h * (2.0 * slopes[i] + slopes[i + 1]),
                        -2.0 * delta + h * (slopes[i] + slopes[i + 1]),
                    ],
                };
                energy_lo += s.integral(1.0);
                if s.c.iter().any(|v| !v.is_finite())
                    || !energy_lo.is_finite()
                    || !s.min_tangent().is_finite()
                    || s.min_tangent() <= 0.0
                {
                    return Err(invalid(
                        "stress-curve interpolation is not strictly stable or overflows",
                    ));
                }
                intervals.push(s);
            }
            Ok(intervals)
        };
        let positive = half(p[origin..].to_vec(), derivatives[origin..].to_vec())?;
        let negative = half(
            p[..=origin].iter().rev().map(|v| [-v[0], -v[1]]).collect(),
            derivatives[..=origin].iter().rev().copied().collect(),
        )?;
        let minimum = positive
            .iter()
            .chain(&negative)
            .map(Interval::min_tangent)
            .fold(f64::INFINITY, f64::min);
        Ok(Curve {
            positive,
            negative,
            range: [p[0][0], p[p.len() - 1][0]],
            minimum,
            origin_tangent: derivatives[origin],
            linear: None,
        })
    }
}

impl Curve {
    fn evaluate(&self, strain: f64) -> [f64; 3] {
        if let Some(linear) = &self.linear {
            return linear
                .evaluate(strain)
                .map_or([f64::NAN; 3], |r| [r.energy, r.value, r.tangent]);
        }
        let (intervals, x, sign) = if strain < 0.0 {
            (&self.negative, -strain, -1.0)
        } else {
            (&self.positive, strain, 1.0)
        };
        let i = intervals
            .partition_point(|s| s.hi < x)
            .min(intervals.len() - 1);
        let [w, s, t] = intervals[i].evaluate(x);
        [w, sign * s, t]
    }
}

/// Validated immutable law, shared across triangles. It contains no IO, solver or browser types.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledMembrane {
    curves: [Curve; 3],
    coupling: f64,
    interpolation: Interpolation,
}

impl TabulatedMembrane {
    /// Compile the source-bearing curves, refusing unstable coupling throughout the full strain box.
    ///
    /// # Errors
    /// Bad schema/knots/units/sources, unapproved estimates, overflow or a nonpositive tangent.
    pub fn compile(
        &self,
        sources: &BTreeMap<String, Source>,
        allow_estimates: bool,
    ) -> Result<CompiledMembrane, Refusal> {
        if self.model != MODEL {
            return Err(invalid("unknown nonlinear membrane model"));
        }
        if !allow_estimates {
            return Err(invalid(
                "tabulated constitutive interpolation requires explicit estimate consent, independently of node evidence",
            ));
        }
        evidence(
            &self.coupling.source,
            self.coupling.evidence,
            &self.coupling.method,
            sources,
            allow_estimates,
        )?;
        let k = self.coupling.value;
        if self.coupling.unit != "N/m"
            || !k.is_finite()
            || self.coupling.conditions.len() > 64
            || self.coupling.conditions.iter().any(|(k, v)| {
                k.trim().is_empty() || v.trim().is_empty() || k.len() > 4096 || v.len() > 4096
            })
            || self
                .coupling
                .bounds
                .is_some_and(|[lo, hi]| !lo.is_finite() || !hi.is_finite() || lo > k || hi < k)
        {
            return Err(invalid(
                "membrane coupling must be a sourced finite N/m observation with valid bounds",
            ));
        }
        let curves = [
            self.warp
                .compile(sources, allow_estimates, self.interpolation)?,
            self.weft
                .compile(sources, allow_estimates, self.interpolation)?,
            self.shear
                .compile(sources, allow_estimates, self.interpolation)?,
        ];
        let stability = (k.abs() / curves[0].minimum.sqrt()) / curves[1].minimum.sqrt();
        if !stability.is_finite() || stability >= 1.0 {
            return Err(invalid(
                "nonlinear membrane coupling exceeds the minimum directional tangent; law is not stable throughout its strain range",
            ));
        }
        Ok(CompiledMembrane {
            curves,
            coupling: k,
            interpolation: self.interpolation,
        })
    }
}

impl CompiledMembrane {
    /// Interpolation actually used by this immutable compiled law.
    #[must_use]
    pub fn interpolation(&self) -> Interpolation {
        self.interpolation
    }

    /// Check physical strains before numerical gather/compression softening. Softening cannot
    /// authorize extrapolation beyond the specimen's declared range.
    ///
    /// # Errors
    /// Nonfinite or out-of-domain material-frame strain.
    pub fn check_domain(&self, strain: [f64; 3]) -> Result<(), Refusal> {
        for (i, (&e, c)) in strain.iter().zip(&self.curves).enumerate() {
            if !e.is_finite() || e < c.range[0] || e > c.range[1] {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!(
                        "membrane direction {i}: physical strain {e} outside characterized range [{}, {}]",
                        c.range[0], c.range[1]
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Checked evaluation for solver and diagnostic consumers.
    ///
    /// # Errors
    /// Outside the characterized strain box or unrepresentable response arithmetic.
    pub fn try_evaluate(&self, strain: [f64; 3]) -> Result<Response, Refusal> {
        self.check_domain(strain)?;
        self.evaluate(strain).ok_or_else(|| {
            Refusal::new(
                RefusalCode::NonFinite,
                "tabulated membrane response is not representable",
            )
        })
    }

    /// Reference E1,E2,nu12,G12 from the origin tangent. These numbers are for reporting and
    /// initialization, not a substitute for the full nonlinear law during simulation.
    ///
    /// # Errors
    /// Unrepresentable conversion of an otherwise finite tangent.
    pub fn reference_moduli(&self) -> Result<[f64; 4], Refusal> {
        let [q11, q22, c, g] = self.origin_stiffness();
        let nu = c / q22;
        let d = 1. - nu * (c / q11);
        let out = [q11 * d, q22 * d, nu, g];
        if out.iter().any(|v| !v.is_finite()) || out[0] <= 0. || out[1] <= 0. || out[3] <= 0. {
            return Err(invalid("unrepresentable tabulated reference moduli"));
        }
        Ok(out)
    }
    /// Declared raw Green-strain domain, in warp/weft/engineering-shear order. No extrapolation.
    #[must_use]
    pub fn strain_range(&self) -> [[f64; 2]; 3] {
        self.curves.each_ref().map(|c| c.range)
    }

    /// Whether a complete state is inside all three characterized strain ranges.
    #[must_use]
    pub fn contains(&self, strain: [f64; 3]) -> bool {
        strain
            .iter()
            .zip(&self.curves)
            .all(|(&e, c)| e.is_finite() && e >= c.range[0] && e <= c.range[1])
    }

    /// Small-strain [Q11,Q22,Q12,G12], used to report the tangent, not as a second constitutive law.
    #[must_use]
    pub fn origin_stiffness(&self) -> [f64; 4] {
        [
            self.curves[0].origin_tangent,
            self.curves[1].origin_tangent,
            self.coupling,
            self.curves[2].origin_tangent,
        ]
    }

    /// Exact integrated energy and derivatives. None is an unsupported trial state, not clamping.
    #[must_use]
    pub fn evaluate(&self, strain: [f64; 3]) -> Option<Response> {
        if !self.contains(strain) {
            return None;
        }
        let v: [[f64; 3]; 3] = std::array::from_fn(|i| self.curves[i].evaluate(strain[i]));
        let k = self.coupling;
        let response = Response {
            energy: v[0][0] + v[1][0] + v[2][0] + k * strain[0] * strain[1],
            stress: [v[0][1] + k * strain[1], v[1][1] + k * strain[0], v[2][1]],
            tangent: [[v[0][2], k, 0.0], [k, v[1][2], 0.0], [0.0, 0.0, v[2][2]]],
        };
        (response.energy.is_finite()
            && response.stress.iter().all(|v| v.is_finite())
            && response.tangent.iter().flatten().all(|v| v.is_finite()))
        .then_some(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (TabulatedMembrane, BTreeMap<String, Source>) {
        let v: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-nonlinear.json"
        ))
        .unwrap();
        (
            serde_json::from_value(v["membrane_response"].clone()).unwrap(),
            serde_json::from_value(v["sources"].clone()).unwrap(),
        )
    }

    #[test]
    fn source_curves_interpolate_and_stiffen() {
        let (r, sources) = fixture();
        let law = r.compile(&sources, true).unwrap();
        for (axis, curve) in [&r.warp, &r.weft, &r.shear].into_iter().enumerate() {
            for &[e, s] in &curve.points {
                let mut at = [0.; 3];
                at[axis] = e;
                assert!((law.evaluate(at).unwrap().stress[axis] - s).abs() < 1e-9);
            }
        }
        assert_eq!(law.origin_stiffness(), [200., 100., 10., 40.]);
        assert!(law.evaluate([0.3, 0., 0.]).unwrap().tangent[0][0] > 1000.);
    }

    #[test]
    fn derivative_chain_matches_differences_including_knots() {
        let (r, sources) = fixture();
        let law = r.compile(&sources, true).unwrap();
        for strain in [
            [0., 0., 0.],
            [-0.12, 0.11, -0.14],
            [0.03, 0.03, -0.03],
            [0.1, 0.4, 0.2],
        ] {
            let exact = law.evaluate(strain).unwrap();
            for i in 0..3 {
                let mut a = strain;
                let mut b = strain;
                a[i] -= 1e-6;
                b[i] += 1e-6;
                let (a, b) = (law.evaluate(a).unwrap(), law.evaluate(b).unwrap());
                assert!(((b.energy - a.energy) / 2e-6 - exact.stress[i]).abs() < 2e-5);
                for j in 0..3 {
                    assert!(
                        ((b.stress[j] - a.stress[j]) / 2e-6 - exact.tangent[j][i]).abs() < 0.03
                    );
                }
            }
        }
    }

    #[test]
    fn evidence_stability_and_domain_fail_closed() {
        let (mut r, sources) = fixture();
        assert!(r.compile(&sources, false).is_err());
        let law = r.compile(&sources, true).unwrap();
        assert!(law.evaluate([-0.501, 0., 0.]).is_none());
        assert!(law.evaluate([0., f64::NAN, 0.]).is_none());
        r.coupling.value = 1000.;
        assert!(r.compile(&sources, true).is_err());
        r.coupling.value = 10.;
        r.warp.points[3][1] = -1.;
        assert!(r.compile(&sources, true).is_err());
    }

    #[test]
    fn near_origin_energy_does_not_subtract_large_primitive_values() {
        let (r, sources) = fixture();
        let law = r.compile(&sources, true).unwrap();
        for e in [-1e-12, 1e-12] {
            let w = law.evaluate([e, 0., 0.]).unwrap().energy;
            assert!((w - 100. * e * e).abs() < 1e-30);
        }
    }

    #[test]
    fn explicit_linear_mode_uses_the_shared_work_consistent_kernel() {
        let v: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/materials/synthetic-response.json"
        ))
        .unwrap();
        let r: TabulatedMembrane =
            serde_json::from_value(v["record"]["membrane_response"].clone()).unwrap();
        let sources = serde_json::from_value(v["record"]["sources"].clone()).unwrap();
        let law = r.compile(&sources, true).unwrap();
        assert_eq!(law.interpolation(), Interpolation::PiecewiseLinear);
        for (i, c) in [&r.warp, &r.weft, &r.shear].into_iter().enumerate() {
            let numeric = ftx_core::response_curve::ResponseCurve::new(&c.points).unwrap();
            for &[e, _] in &c.points {
                let mut strain = [0.; 3];
                strain[i] = e;
                let actual = law.try_evaluate(strain).unwrap();
                let expected = numeric.evaluate(e).unwrap();
                assert_eq!(actual.energy, expected.energy);
                assert_eq!(actual.stress[i], expected.value);
                assert_eq!(actual.tangent[i][i], expected.tangent);
            }
        }
        assert!(law.check_domain([1.01, 0., 0.]).is_err());
        assert!(law.try_evaluate([0., f64::NAN, 0.]).is_err());
    }

    #[test]
    fn interpolation_is_explicit_and_observations_do_not_certify_the_interpolant() {
        let (mut r, sources) = fixture();
        let wire = serde_json::to_value(&r).unwrap();
        assert!(wire.get("interpolation").is_none());
        assert_eq!(
            r.compile(&sources, true).unwrap().interpolation(),
            Interpolation::MonotoneCubic
        );
        r.interpolation = Interpolation::PiecewiseLinear;
        assert_ne!(serde_json::to_value(&r).unwrap(), wire);
        r.warp.evidence = Evidence::Published;
        r.weft.evidence = Evidence::Published;
        r.shear.evidence = Evidence::Published;
        r.coupling.evidence = Evidence::Published;
        assert!(r.compile(&sources, false).is_err());
        let mut malformed = sources.clone();
        malformed.values_mut().next().unwrap().reference.clear();
        assert!(r.compile(&malformed, true).is_err());
        assert!(r.compile(&sources, true).is_ok());
    }
}
