//! Error-free transformations (EFT): exact floating-point sum and product residuals.
//!
//! Provenance: lifted from `/dp/frankensim/crates/fs-math/src/eft.rs` (same author, MIT +
//! OpenAI/Anthropic rider) for deterministic double-double arithmetic backing [`crate::det::erf`],
//! [`crate::det::erfc`], and [`crate::det::pow`]. Pure IEEE-754 `+`, `-`, `*`, and explicit
//! `mul_add` — bit-identical on every conforming native and `wasm32` target.

/// Knuth's TwoSum: `(s, e)` with `s = fl(a + b)` and `s + e = a + b` exactly.
/// No ordering precondition.
#[must_use]
pub fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    let e = (a - (s - bb)) + (b - bb);
    (s, e)
}

/// Dekker's FastTwoSum: same contract as [`two_sum`] when `|a| >= |b|` (or `a == 0.0`).
#[must_use]
pub fn quick_two_sum(a: f64, b: f64) -> (f64, f64) {
    debug_assert!(
        a == 0.0 || b == 0.0 || a.abs() >= b.abs() || a.is_nan() || b.is_nan(),
        "quick_two_sum precondition |a| >= |b| violated: a={a}, b={b}"
    );
    let s = a + b;
    let e = b - (s - a);
    (s, e)
}

/// TwoProd via FMA: `(p, e)` with `p = fl(a * b)` and `p + e = a * b` exactly for finite,
/// non-overflowing inputs.
#[must_use]
pub fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let e = a.mul_add(b, -p);
    (p, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_sum_recovers_absorbed_tail() {
        let (s, e) = two_sum(1e16, 1.0);
        assert_eq!(s.to_bits(), 1e16_f64.to_bits());
        assert_eq!(e.to_bits(), 1.0_f64.to_bits());
    }

    #[test]
    fn quick_two_sum_matches_two_sum() {
        let (s1, e1) = two_sum(12345.6789, 1e-12);
        let (s2, e2) = quick_two_sum(12345.6789, 1e-12);
        assert_eq!(s1.to_bits(), s2.to_bits());
        assert_eq!(e1.to_bits(), e2.to_bits());
    }

    #[test]
    fn two_prod_recovers_exact_residual() {
        let x = 1.0 + f64::EPSILON;
        let (_, e) = two_prod(x, x);
        assert_eq!(e.to_bits(), (f64::EPSILON * f64::EPSILON).to_bits());
    }
}
