//! Published degree-at-most-three thermal fits. Do not resample a fit into fictitious measurements.

use serde::{Deserialize, Serialize};

use super::{Refusal, invalid};

/// A fit in Celsius powers: `a0 + a1*T + a2*T² + a3*T³`. Values use the containing series' unit;
/// coefficient n therefore has that unit divided by °Cⁿ. Higher orders are deliberately unsupported.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolynomialFit {
    /// Inclusive characterized temperature domain, degrees Celsius; never inferred from coefficients.
    pub domain_c: [f64; 2],
    /// Coefficients in increasing power order. Trailing zeros encode constant/linear/quadratic fits.
    pub coefficients: [f64; 4],
}

impl PolynomialFit {
    pub(super) fn value(&self, t: f64) -> f64 {
        let [a, b, c, d] = self.coefficients;
        ((d * t + c) * t + b) * t + a
    }

    fn check_value(&self, t: f64) -> Result<(), Refusal> {
        let magnitude = self
            .coefficients
            .iter()
            .rev()
            .fold(0.0, |v, a| v * t.abs() + a.abs());
        let value = self.value(t);
        // Near-cancellation is not a reliable proof of positivity. Reject such laws rather than
        // allowing a small rounded positive value to conceal a nonpositive local minimum.
        if !magnitude.is_finite() || !value.is_finite() || value <= 64.0 * f64::EPSILON * magnitude
        {
            return Err(invalid(
                "thermal polynomial must be positive without numerical cancellation over its entire domain",
            ));
        }
        Ok(())
    }

    pub(super) fn validate(&self) -> Result<(), Refusal> {
        let [lo, hi] = self.domain_c;
        if !lo.is_finite()
            || !hi.is_finite()
            || lo < -273.15
            || lo >= hi
            || !(hi - lo).is_finite()
            || self.coefficients.iter().any(|x| !x.is_finite())
        {
            return Err(invalid("invalid thermal polynomial coefficients or domain"));
        }
        self.check_value(lo)?;
        self.check_value(hi)?;
        // Check EVERY possible interior extremum, not just a temperature sampling grid. Scale the
        // derivative before its discriminant; the stable q formula avoids cancellation of a root.
        let [_, c, b, a] = self.coefficients;
        let scale = a.abs().max(b.abs()).max(c.abs());
        if scale == 0.0 {
            return Ok(());
        }
        let (a, b, c) = (3.0 * (a / scale), 2.0 * (b / scale), c / scale);
        let mut roots = [f64::NAN; 2];
        if a == 0.0 {
            if b != 0.0 {
                roots[0] = -c / b;
            }
        } else {
            let discriminant = b * b - 4.0 * a * c;
            let round = 32.0 * f64::EPSILON * (b * b + (4.0 * a * c).abs());
            if discriminant >= -round {
                let q = -0.5 * (b + discriminant.max(0.0).sqrt().copysign(b));
                if q == 0.0 {
                    roots[0] = -b / (2.0 * a);
                } else {
                    roots = [q / a, c / q];
                }
            }
        }
        for t in roots {
            if t > lo && t < hi {
                self.check_value(t)?;
            }
        }
        Ok(())
    }

    pub(super) fn integral(&self, from: f64, to: f64) -> Result<f64, Refusal> {
        // Simpson integration is exact for this degree bound, and works on the local increment.
        // It avoids subtracting large primitive values, and preserves antisymmetry on reversal.
        let lo = from.min(to);
        let hi = from.max(to);
        let mid = lo / 2.0 + hi / 2.0;
        let mean = self.value(lo) / 6.0 + self.value(mid) * (2.0 / 3.0) + self.value(hi) / 6.0;
        let result = (hi - lo) * mean;
        if !result.is_finite() || result < 0.0 {
            return Err(invalid("thermal polynomial enthalpy overflow"));
        }
        Ok(if to < from { -result } else { result })
    }
}
