//! Bounded, work-consistent monotone response tables.
//!
//! Adapted from frankensim `crates/fs-contact/src/normal_patch/response_curve.rs`
//! (`FiniteGapResponseCurve::evaluate` and trapezoidal node-work accumulation),
//! commit fe9ae4ea1d3923b1c336ea080690451915bd3e78, MIT.
//! The source's force/approach interpolation is generalized to signed coordinates about an explicit
//! zero-response origin. No contact solver, execution context or source-identity type is imported.
//! Unlike the source, input nodes are private after validation; negative-side work is integrated
//! from zero backwards to avoid subtracting large energies near the stress-free state.

use crate::{Refusal, RefusalCode};

/// A response and its exact segment derivative and integral. Units are caller-defined:
/// `energy` has response units times coordinate units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Response {
    /// Integral of the response from zero to the requested coordinate.
    pub energy: f64,
    /// Piecewise-linear response.
    pub value: f64,
    /// Strictly positive segment slope; right-hand value at interior knots.
    pub tangent: f64,
}

/// Immutable, strictly monotone piecewise-linear response; its integral is C1, not C2 at knots.
#[derive(Clone, Debug, PartialEq)]
pub struct ResponseCurve {
    nodes: Vec<[f64; 2]>,
    slopes: Vec<f64>,
    work: Vec<f64>,
    minimum_tangent: f64,
}

impl ResponseCurve {
    /// Validate 3..4096 ordered `[coordinate, response]` knots, including exactly `[0,0]` and
    /// both negative and positive coordinates. Inputs are never sorted, normalized or clamped.
    ///
    /// # Errors
    /// Nonfinite, unordered, nonmonotone or unrepresentable data, or a missing stress-free origin.
    pub fn new(nodes: &[[f64; 2]]) -> Result<Self, Refusal> {
        let bad = || {
            Refusal::new(
                RefusalCode::OutOfRange,
                "response table needs 3..4096 finite, strictly increasing coordinate/response knots through [0,0], spanning both signs",
            )
        };
        if !(3..=4096).contains(&nodes.len())
            || nodes.iter().flatten().any(|v| !v.is_finite())
            || nodes[0][0] >= 0.0
            || nodes[nodes.len() - 1][0] <= 0.0
        {
            return Err(bad());
        }
        let zero = nodes
            .iter()
            .position(|n| *n == [0.0, 0.0])
            .ok_or_else(bad)?;
        let mut slopes = Vec::with_capacity(nodes.len() - 1);
        for pair in nodes.windows(2) {
            let dx = pair[1][0] - pair[0][0];
            let dy = pair[1][1] - pair[0][1];
            let k = dy / dx;
            if !dx.is_finite() || dx <= 0.0 || !dy.is_finite() || !k.is_finite() || k <= 0.0 {
                return Err(bad());
            }
            slopes.push(k);
        }
        let mut work = vec![0.0; nodes.len()];
        for i in zero + 1..nodes.len() {
            work[i] = work[i - 1]
                + (0.5 * nodes[i - 1][1] + 0.5 * nodes[i][1]) * (nodes[i][0] - nodes[i - 1][0]);
        }
        for i in (0..zero).rev() {
            work[i] = work[i + 1]
                - (0.5 * nodes[i][1] + 0.5 * nodes[i + 1][1]) * (nodes[i + 1][0] - nodes[i][0]);
        }
        if work.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "response table integral is not representable",
            ));
        }
        Ok(Self {
            nodes: nodes.to_vec(),
            minimum_tangent: slopes.iter().copied().fold(f64::INFINITY, f64::min),
            slopes,
            work,
        })
    }

    /// Closed coordinate domain; evaluation outside it is always refused.
    #[must_use]
    pub fn domain(&self) -> [f64; 2] {
        [self.nodes[0][0], self.nodes[self.nodes.len() - 1][0]]
    }

    /// Smallest segment tangent, useful for coupled-law stability checks.
    #[must_use]
    pub fn minimum_tangent(&self) -> f64 {
        self.minimum_tangent
    }

    /// Evaluate response, slope and work from the same segment, without extrapolation.
    ///
    /// # Errors
    /// A nonfinite coordinate, an out-of-table coordinate, or unrepresentable arithmetic.
    pub fn evaluate(&self, x: f64) -> Result<Response, Refusal> {
        let [lo, hi] = self.domain();
        if !x.is_finite() || x < lo || x > hi {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("response coordinate {x} outside characterized range [{lo}, {hi}]"),
            ));
        }
        let upper = self.nodes.partition_point(|n| n[0] <= x);
        let right = upper.clamp(1, self.nodes.len() - 1);
        let left = right - 1;
        let k = self.slopes[left];
        // Use the node toward zero as anchor, avoiding cancellation of accumulated work near zero.
        let anchor = if x < 0.0 { right } else { left };
        let dx = x - self.nodes[anchor][0];
        let value = k.mul_add(dx, self.nodes[anchor][1]);
        let energy = self.work[anchor] + self.nodes[anchor][1] * dx + 0.5 * (k * dx) * dx;
        if !value.is_finite() || !energy.is_finite() {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "response evaluation is not representable",
            ));
        }
        Ok(Response {
            energy,
            value,
            tangent: k,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_linear_known_answer_and_small_strains() {
        let c = ResponseCurve::new(&[[-2., -6.], [0., 0.], [1., 3.]]).unwrap();
        for x in [-2., -0.75, -1e-12, 0., 1e-12, 0.75, 1.] {
            let r = c.evaluate(x).unwrap();
            assert!((r.value - 3. * x).abs() <= 1e-15);
            assert!((r.energy - 1.5 * x * x).abs() <= 1e-15 * (x * x));
            assert_eq!(r.tangent, 3.);
        }
        assert_eq!(c.evaluate(0.).unwrap().energy, 0.);
    }

    #[test]
    fn nonlinear_work_tangent_and_knot_convention() {
        let c = ResponseCurve::new(&[[-1., -2.], [0., 0.], [1., 4.], [2., 12.]]).unwrap();
        assert_eq!(c.evaluate(-1.).unwrap().energy, 1.);
        assert_eq!(
            c.evaluate(1.).unwrap(),
            Response {
                energy: 2.,
                value: 4.,
                tangent: 8.
            }
        );
        assert_eq!(c.evaluate(2.).unwrap().energy, 10.);
        assert_eq!(c.minimum_tangent(), 2.);
        for x in [-0.7, 0.3, 1.6] {
            let h = 1e-6;
            let (a, b, r) = (
                c.evaluate(x - h).unwrap(),
                c.evaluate(x + h).unwrap(),
                c.evaluate(x).unwrap(),
            );
            assert!(((b.energy - a.energy) / (2. * h) - r.value).abs() < 2e-9);
            assert!(((b.value - a.value) / (2. * h) - r.tangent).abs() < 2e-9);
        }
    }

    #[test]
    fn invalid_tables_and_outside_domain_are_not_repaired() {
        for nodes in [
            vec![],
            vec![[0., 0.], [1., 1.]],
            vec![[-1., -1.], [0., 1.], [1., 2.]],
            vec![[-1., -1.], [0., 0.], [0., 1.]],
            vec![[-1., -1.], [0., 0.], [1., -1.]],
            vec![[-1., -1.], [0., 0.], [1., 0.]],
            vec![[-1., -1.], [0., 0.], [f64::INFINITY, 1.]],
            vec![[-1e308, -1e308], [0., 0.], [1e308, 1e308]],
        ] {
            assert!(ResponseCurve::new(&nodes).is_err(), "{nodes:?}");
        }
        let c = ResponseCurve::new(&[[-1., -1.], [0., 0.], [1., 1.]]).unwrap();
        for x in [-1.00001, 1.00001, f64::NAN, f64::INFINITY] {
            assert!(c.evaluate(x).is_err());
        }
    }
}
