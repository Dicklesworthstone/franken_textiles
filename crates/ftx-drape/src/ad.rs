//! Second-order forward-mode automatic differentiation over `N` variables: a [`Jet`] carries a value,
//! its gradient and its (symmetric) Hessian, so a contact energy written once as plain arithmetic yields
//! the exact gradient and Hessian the Newton solver needs. Contact pairs have at most four points
//! (`N = 12`).

use std::ops::{Add, Div, Mul, Neg, Sub};

/// Value, gradient and Hessian of a function of `N` variables.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jet<const N: usize> {
    /// Value.
    pub v: f64,
    /// Gradient.
    pub g: [f64; N],
    /// Hessian (row-major, symmetric).
    pub h: [[f64; N]; N],
}

impl<const N: usize> Jet<N> {
    /// A constant.
    #[must_use]
    pub fn constant(v: f64) -> Self {
        Self {
            v,
            g: [0.0; N],
            h: [[0.0; N]; N],
        }
    }

    /// Variable `i` with value `v`.
    #[must_use]
    pub fn var(v: f64, i: usize) -> Self {
        let mut j = Self::constant(v);
        j.g[i] = 1.0;
        j
    }

    /// Applies a scalar function with value `f`, first derivative `d1` and second derivative `d2` at
    /// this jet's value (the chain rule to second order).
    #[must_use]
    pub fn chain(&self, f: f64, d1: f64, d2: f64) -> Self {
        let mut out = Self::constant(f);
        for i in 0..N {
            out.g[i] = d1 * self.g[i];
            for j in 0..N {
                out.h[i][j] = d1 * self.h[i][j] + d2 * self.g[i] * self.g[j];
            }
        }
        out
    }

    /// Square root (for positive values).
    #[must_use]
    pub fn sqrt(&self) -> Self {
        let s = self.v.sqrt();
        self.chain(s, 0.5 / s, -0.25 / (s * self.v))
    }

    /// Natural logarithm (deterministic `ftx_core::det::ln`), for positive values.
    #[must_use]
    pub fn ln(&self) -> Self {
        self.chain(
            ftx_core::det::ln(self.v),
            1.0 / self.v,
            -1.0 / (self.v * self.v),
        )
    }
}

impl<const N: usize> Add for Jet<N> {
    type Output = Self;
    fn add(mut self, o: Self) -> Self {
        self.v += o.v;
        for i in 0..N {
            self.g[i] += o.g[i];
            for j in 0..N {
                self.h[i][j] += o.h[i][j];
            }
        }
        self
    }
}

impl<const N: usize> Sub for Jet<N> {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        self + (-o)
    }
}

impl<const N: usize> Neg for Jet<N> {
    type Output = Self;
    fn neg(mut self) -> Self {
        self.v = -self.v;
        for (g, row) in self.g.iter_mut().zip(self.h.iter_mut()) {
            *g = -*g;
            for h in row.iter_mut() {
                *h = -*h;
            }
        }
        self
    }
}

impl<const N: usize> Mul for Jet<N> {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        let mut out = Self::constant(self.v * o.v);
        for i in 0..N {
            out.g[i] = self.g[i] * o.v + self.v * o.g[i];
            for j in 0..N {
                out.h[i][j] = self.h[i][j] * o.v
                    + self.v * o.h[i][j]
                    + self.g[i] * o.g[j]
                    + o.g[i] * self.g[j];
            }
        }
        out
    }
}

impl<const N: usize> Mul<f64> for Jet<N> {
    type Output = Self;
    fn mul(mut self, s: f64) -> Self {
        self.v *= s;
        for (g, row) in self.g.iter_mut().zip(self.h.iter_mut()) {
            *g *= s;
            for h in row.iter_mut() {
                *h *= s;
            }
        }
        self
    }
}

impl<const N: usize> Div for Jet<N> {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        // a / b = a · (1/b), with 1/b by the chain rule.
        let inv = o.chain(1.0 / o.v, -1.0 / (o.v * o.v), 2.0 / (o.v * o.v * o.v));
        self * inv
    }
}

/// Value and gradient of a function of `N` variables (first-order forward mode).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dual<const N: usize> {
    /// Value.
    pub v: f64,
    /// Gradient.
    pub g: [f64; N],
}

impl<const N: usize> Add for Dual<N> {
    type Output = Self;
    fn add(mut self, o: Self) -> Self {
        self.v += o.v;
        for i in 0..N {
            self.g[i] += o.g[i];
        }
        self
    }
}

impl<const N: usize> Sub for Dual<N> {
    type Output = Self;
    fn sub(mut self, o: Self) -> Self {
        self.v -= o.v;
        for i in 0..N {
            self.g[i] -= o.g[i];
        }
        self
    }
}

#[allow(clippy::suspicious_arithmetic_impl)] // the product rule
impl<const N: usize> Mul for Dual<N> {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self {
            v: self.v * o.v,
            g: std::array::from_fn(|i| self.g[i] * o.v + self.v * o.g[i]),
        }
    }
}

#[allow(clippy::suspicious_arithmetic_impl)] // the quotient rule
impl<const N: usize> Div for Dual<N> {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        let q = self.v / o.v;
        Self {
            v: q,
            g: std::array::from_fn(|i| (self.g[i] - q * o.g[i]) / o.v),
        }
    }
}

/// The arithmetic the distance formulas need, implemented by `f64`, [`Dual`] and [`Jet`], so each
/// formula is written once and evaluated at the order a caller needs.
pub trait Scalar:
    Copy + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self>
{
    /// Coordinate `i` of point `k` (a variable, or a constant when `fixed`).
    fn coord(v: f64, k: usize, i: usize, fixed: bool) -> Self;
    /// The value.
    fn value(&self) -> f64;
}

impl Scalar for f64 {
    fn coord(v: f64, _: usize, _: usize, _: bool) -> Self {
        v
    }
    fn value(&self) -> f64 {
        *self
    }
}

impl<const N: usize> Scalar for Dual<N> {
    fn coord(v: f64, k: usize, i: usize, fixed: bool) -> Self {
        let mut d = Self { v, g: [0.0; N] };
        if !fixed {
            d.g[3 * k + i] = 1.0;
        }
        d
    }
    fn value(&self) -> f64 {
        self.v
    }
}

impl<const N: usize> Scalar for Jet<N> {
    fn coord(v: f64, k: usize, i: usize, fixed: bool) -> Self {
        if fixed {
            Self::constant(v)
        } else {
            Self::var(v, 3 * k + i)
        }
    }
    fn value(&self) -> f64 {
        self.v
    }
}

/// A 3-vector of scalars.
#[derive(Clone, Copy, Debug)]
pub struct JVec<T>(pub [T; 3]);

impl<T: Scalar> JVec<T> {
    /// Point `k` of a pair: variables `3k, 3k+1, 3k+2` (or constants when `fixed`).
    #[must_use]
    pub fn point(p: ftx_core::Vec3, k: usize, fixed: bool) -> Self {
        let c = [p.x, p.y, p.z];
        Self(std::array::from_fn(|i| T::coord(c[i], k, i, fixed)))
    }

    /// Difference.
    #[must_use]
    pub fn sub(&self, o: &Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] - o.0[i]))
    }

    /// Dot product.
    #[must_use]
    pub fn dot(&self, o: &Self) -> T {
        self.0[0] * o.0[0] + self.0[1] * o.0[1] + self.0[2] * o.0[2]
    }

    /// Cross product.
    #[must_use]
    pub fn cross(&self, o: &Self) -> Self {
        let (a, b) = (&self.0, &o.0);
        Self([
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// f(x, y) = x²y / (1 + y) + √x·ln(y) against hand derivatives.
    #[test]
    fn jet_matches_hand_derivatives() {
        let (x0, y0) = (1.7, 2.3);
        let (x, y) = (Jet::<2>::var(x0, 0), Jet::<2>::var(y0, 1));
        let one = Jet::constant(1.0);
        let f = x * x * y / (one + y) + x.sqrt() * y.ln();
        let want_v = x0 * x0 * y0 / (1.0 + y0) + x0.sqrt() * y0.ln();
        let fx = 2.0 * x0 * y0 / (1.0 + y0) + 0.5 / x0.sqrt() * y0.ln();
        let fy = x0 * x0 / ((1.0 + y0) * (1.0 + y0)) + x0.sqrt() / y0;
        let fxx = 2.0 * y0 / (1.0 + y0) - 0.25 * x0.powf(-1.5) * y0.ln();
        let fxy = 2.0 * x0 / ((1.0 + y0) * (1.0 + y0)) + 0.5 / (x0.sqrt() * y0);
        let fyy = -2.0 * x0 * x0 / (1.0 + y0).powi(3) - x0.sqrt() / (y0 * y0);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12 * b.abs().max(1.0);
        assert!(close(f.v, want_v));
        assert!(close(f.g[0], fx) && close(f.g[1], fy));
        assert!(
            close(f.h[0][0], fxx)
                && close(f.h[0][1], fxy)
                && close(f.h[1][0], fxy)
                && close(f.h[1][1], fyy)
        );
    }
}
