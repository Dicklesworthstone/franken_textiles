//! Double-double arithmetic (~106-bit significand) built on [`crate::eft`].
//!
//! Provenance: lifted from `/dp/frankensim/crates/fs-math/src/dd.rs` (same author, MIT +
//! OpenAI/Anthropic rider) so [`crate::det::erf`] and [`crate::det::erfc`] are bit-identical
//! with FrankenSim on native and `wasm32`.

use crate::eft::{quick_two_sum, two_prod, two_sum};

/// A normalized double-double value `hi + lo` with `|lo| <= 0.5 ulp(hi)`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Dd {
    /// Leading `f64` component.
    pub hi: f64,
    /// Trailing residual component.
    pub lo: f64,
}

impl Dd {
    /// Zero.
    pub const ZERO: Self = Self { hi: 0.0, lo: 0.0 };
    /// One.
    pub const ONE: Self = Self { hi: 1.0, lo: 0.0 };

    /// Exact lift from `f64`.
    #[must_use]
    pub const fn from_f64(x: f64) -> Self {
        Self { hi: x, lo: 0.0 }
    }

    /// Construct from an unevaluated pair, renormalizing via `two_sum`.
    #[must_use]
    pub fn from_pair(hi: f64, lo: f64) -> Self {
        let (s, e) = two_sum(hi, lo);
        Self { hi: s, lo: e }
    }

    /// Round to nearest `f64`.
    #[must_use]
    pub const fn to_f64(self) -> f64 {
        self.hi
    }

    /// Exact absolute value.
    #[must_use]
    pub fn abs(self) -> Self {
        if self.hi < 0.0 || (self.hi == 0.0 && self.lo < 0.0) {
            -self
        } else {
            self
        }
    }

    /// Square root (Karp's method; relative error <= 2^-103).
    #[must_use]
    pub fn sqrt(self) -> Self {
        if self.hi == 0.0 && self.lo == 0.0 {
            return Self::ZERO;
        }
        let s = self.hi.sqrt();
        let sd = Self::from_f64(s);
        let e = (self - sd * sd).hi / (2.0 * s);
        Self::from_pair(s, e)
    }

    /// Lexicographic ordering on non-NaN values.
    #[must_use]
    #[allow(clippy::float_cmp)]
    pub fn lt(self, other: Self) -> bool {
        self.hi < other.hi || (self.hi == other.hi && self.lo < other.lo)
    }
}

impl core::ops::Neg for Dd {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            hi: -self.hi,
            lo: -self.lo,
        }
    }
}

impl core::ops::Add for Dd {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        let (s1, s2) = two_sum(self.hi, o.hi);
        let (t1, t2) = two_sum(self.lo, o.lo);
        let (s1, s2) = quick_two_sum(s1, s2 + t1);
        let (hi, lo) = quick_two_sum(s1, s2 + t2);
        Self { hi, lo }
    }
}

impl core::ops::Sub for Dd {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        self + (-o)
    }
}

impl core::ops::Mul for Dd {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        let (p1, p2) = two_prod(self.hi, o.hi);
        let p2 = p2 + (self.hi * o.lo + self.lo * o.hi);
        let (hi, lo) = quick_two_sum(p1, p2);
        Self { hi, lo }
    }
}

impl core::ops::Div for Dd {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        let q1 = self.hi / o.hi;
        let r = self - o * Self::from_f64(q1);
        let q2 = r.hi / o.hi;
        let r = r - o * Self::from_f64(q2);
        let q3 = r.hi / o.hi;
        let (s, e) = quick_two_sum(q1, q2);
        Self::from_pair(s, e + q3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_and_sqrt_accuracy() {
        let a = Dd::from_f64(1e16) + Dd::ONE - Dd::from_f64(1e16);
        assert_eq!(a.hi.to_bits(), 1.0_f64.to_bits());
        assert_eq!(a.lo.to_bits(), 0.0_f64.to_bits());

        let r2 = Dd::from_f64(2.0).sqrt();
        let diff = (r2 * r2 - Dd::from_f64(2.0)).abs();
        assert!(diff.hi < 1e-30);
    }
}
