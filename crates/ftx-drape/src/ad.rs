//! Second-order forward-mode automatic differentiation over `N` variables: a [`Jet`] carries a value,
//! its gradient and its (symmetric) Hessian, so a contact energy written once as plain arithmetic yields
//! the exact gradient and Hessian the Newton solver needs. Contact pairs have at most four points
//! (`N = 12`).
//!
//! Provenance: [`Dual2`], [`second_directional`], [`GradCheckReport`] and [`gradcheck`] are lifted and
//! adapted from `frankensim` `crates/fs-ad/src/dual.rs` and `crates/fs-ad/src/gradcheck.rs` at commit
//! `1d17e0772c4b6a9d21a5be650928b15b2410f8f9` as an independent derivative and Hessian oracle for
//! [`Jet`] (KAT 1).

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

impl<const N: usize> Dual<N> {
    /// A constant (zero derivative).
    #[must_use]
    pub fn constant(v: f64) -> Self {
        Self { v, g: [0.0; N] }
    }

    /// Variable `i` with value `v`.
    #[must_use]
    pub fn var(v: f64, i: usize) -> Self {
        let mut d = Self::constant(v);
        d.g[i] = 1.0;
        d
    }

    /// First-order chain rule: given `f(v)` and `f'(v)`, returns `(f, f' · g)`.
    #[must_use]
    pub fn chain(&self, f: f64, d1: f64) -> Self {
        Self {
            v: f,
            g: std::array::from_fn(|i| d1 * self.g[i]),
        }
    }

    /// Square root (for positive values).
    #[must_use]
    pub fn sqrt(&self) -> Self {
        let s = self.v.sqrt();
        self.chain(s, 0.5 / s)
    }

    /// Natural logarithm (deterministic `ftx_core::det::ln`), for positive values.
    #[must_use]
    pub fn ln(&self) -> Self {
        self.chain(ftx_core::det::ln(self.v), 1.0 / self.v)
    }
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

impl<const N: usize> Neg for Dual<N> {
    type Output = Self;
    fn neg(mut self) -> Self {
        self.v = -self.v;
        for g in &mut self.g {
            *g = -*g;
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

impl<const N: usize> Mul<f64> for Dual<N> {
    type Output = Self;
    fn mul(mut self, s: f64) -> Self {
        self.v *= s;
        for g in &mut self.g {
            *g *= s;
        }
        self
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

/// Nested forward-mode dual number (`Dual<Dual<f64, N>, N>`), lifted from `frankensim`
/// `crates/fs-ad/src/dual.rs`.
///
/// Every operation applies only the first-order [`Dual`] rules with [`Dual<N>`] as the scalar, so
/// second derivatives `g[i].g[j] = ∂²f/(∂x_i ∂x_j)` emerge from nested first-order product,
/// quotient and chain rules without sharing [`Jet`]'s second-order Hessian formulas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dual2<const N: usize> {
    /// Inner dual (primal value `v.v` and gradient `v.g`).
    pub v: Dual<N>,
    /// Outer dual derivative lanes (`g[i].v == ∂f/∂x_i`, `g[i].g[j] == ∂²f/(∂x_i ∂x_j)`).
    pub g: [Dual<N>; N],
}

impl<const N: usize> Dual2<N> {
    /// A constant (zero first and second derivatives).
    #[must_use]
    pub fn constant(v: f64) -> Self {
        Self {
            v: Dual::constant(v),
            g: [Dual::constant(0.0); N],
        }
    }

    /// Variable `i` with value `v`.
    #[must_use]
    pub fn var(v: f64, i: usize) -> Self {
        let mut g = [Dual::constant(0.0); N];
        g[i] = Dual::constant(1.0);
        Self {
            v: Dual::var(v, i),
            g,
        }
    }

    /// Nested first-order chain rule: given dual value `f` and dual first derivative `d1`,
    /// multiplies each outer dual lane by `d1`.
    #[must_use]
    pub fn chain(&self, f: Dual<N>, d1: Dual<N>) -> Self {
        Self {
            v: f,
            g: std::array::from_fn(|i| self.g[i] * d1),
        }
    }

    /// Square root via first-order dual chain rule (`d√x = 1/(2√x)` differentiated by `Dual<N>`).
    #[must_use]
    pub fn sqrt(&self) -> Self {
        let s = self.v.sqrt();
        self.chain(s, Dual::constant(0.5) / s)
    }

    /// Natural logarithm via first-order dual chain rule (`d ln x = 1/x` differentiated by `Dual<N>`).
    #[must_use]
    pub fn ln(&self) -> Self {
        self.chain(self.v.ln(), Dual::constant(1.0) / self.v)
    }

    /// Extracted gradient `∇f`.
    #[must_use]
    pub fn grad(&self) -> [f64; N] {
        self.v.g
    }

    /// Extracted Hessian `∇²f`.
    #[must_use]
    pub fn hess(&self) -> [[f64; N]; N] {
        std::array::from_fn(|i| self.g[i].g)
    }
}

impl<const N: usize> Add for Dual2<N> {
    type Output = Self;
    fn add(mut self, o: Self) -> Self {
        self.v = self.v + o.v;
        for i in 0..N {
            self.g[i] = self.g[i] + o.g[i];
        }
        self
    }
}

impl<const N: usize> Sub for Dual2<N> {
    type Output = Self;
    fn sub(mut self, o: Self) -> Self {
        self.v = self.v - o.v;
        for i in 0..N {
            self.g[i] = self.g[i] - o.g[i];
        }
        self
    }
}

impl<const N: usize> Neg for Dual2<N> {
    type Output = Self;
    fn neg(mut self) -> Self {
        self.v = -self.v;
        for g in &mut self.g {
            *g = -*g;
        }
        self
    }
}

#[allow(clippy::suspicious_arithmetic_impl)] // nested product rule
impl<const N: usize> Mul for Dual2<N> {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self {
            v: self.v * o.v,
            g: std::array::from_fn(|i| self.g[i] * o.v + self.v * o.g[i]),
        }
    }
}

impl<const N: usize> Mul<f64> for Dual2<N> {
    type Output = Self;
    fn mul(mut self, s: f64) -> Self {
        self.v = self.v * s;
        for g in &mut self.g {
            *g = *g * s;
        }
        self
    }
}

#[allow(clippy::suspicious_arithmetic_impl)] // nested quotient rule
impl<const N: usize> Div for Dual2<N> {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        let q = self.v / o.v;
        Self {
            v: q,
            g: std::array::from_fn(|i| (self.g[i] - q * o.g[i]) / o.v),
        }
    }
}

/// Exact second directional derivative `d²/dt² f(x + t·dir)` via single-lane nested duals
/// (`Dual2<1>`): returns `(f(x), ∇f(x)·dir, dirᵀ ∇²f(x) dir)`.
#[must_use]
pub fn second_directional<const M: usize>(
    x: [f64; M],
    dir: [f64; M],
    f: impl Fn([Dual2<1>; M]) -> Dual2<1>,
) -> (f64, f64, f64) {
    let vars = std::array::from_fn(|i| Dual2 {
        v: Dual {
            v: x[i],
            g: [dir[i]],
        },
        g: [Dual {
            v: dir[i],
            g: [0.0],
        }],
    });
    let out = f(vars);
    (out.v.v, out.v.g[0], out.g[0].g[0])
}

/// Verdict and evidence from a forward-dual vs central-difference gradient check
/// (`frankensim` `crates/fs-ad/src/gradcheck.rs`).
#[derive(Debug, Clone)]
pub struct GradCheckReport {
    /// Analytic dual gradient, or its validated finite prefix on refusal.
    pub grad: Vec<f64>,
    /// Central finite-difference gradient, or its validated finite prefix on refusal.
    pub fd: Vec<f64>,
    /// Maximum relative error `|grad − fd| / max(|grad|, |fd|, 1)`, or `+∞` on refusal.
    pub max_rel_err: f64,
    /// Component achieving `max_rel_err`, or `usize::MAX` on global preflight refusal.
    pub worst_index: usize,
    /// Tolerance used.
    pub tol: f64,
    /// Gate verdict.
    pub pass: bool,
}

fn fail_closed(grad: Vec<f64>, fd: Vec<f64>, worst_index: usize, tol: f64) -> GradCheckReport {
    GradCheckReport {
        grad,
        fd,
        max_rel_err: f64::INFINITY,
        worst_index,
        tol,
        pass: false,
    }
}

/// Checks the forward-dual gradient of `f` at `x` against central differences at
/// `h = 6e-6 · max(|x_k|, 1)`. Non-finite inputs, tolerances or intermediates fail closed.
#[must_use]
pub fn gradcheck<const N: usize, F>(f: F, x: [f64; N], tol: f64) -> GradCheckReport
where
    F: Fn([Dual<N>; N]) -> Dual<N>,
{
    if N == 0 || !tol.is_finite() || tol < 0.0 {
        return fail_closed(Vec::new(), Vec::new(), usize::MAX, tol);
    }
    if let Some(index) = x.iter().position(|v| !v.is_finite()) {
        return fail_closed(Vec::new(), Vec::new(), index, tol);
    }
    let vars = std::array::from_fn(|i| Dual::<N>::var(x[i], i));
    let out = f(vars);
    if !out.v.is_finite() {
        return fail_closed(Vec::new(), Vec::new(), usize::MAX, tol);
    }
    let mut grad = Vec::with_capacity(N);
    for (index, value) in out.g.into_iter().enumerate() {
        if !value.is_finite() {
            return fail_closed(grad, Vec::new(), index, tol);
        }
        grad.push(value);
    }
    let eval = |xs: [f64; N]| -> Dual<N> { f(std::array::from_fn(|i| Dual::constant(xs[i]))) };
    let mut fd = Vec::with_capacity(N);
    for k in 0..N {
        let scale = x[k].abs().max(1.0);
        let h = 6e-6 * scale;
        let denom = 2.0 * h;
        if !scale.is_finite() || !h.is_finite() || h <= 0.0 || !denom.is_finite() || denom <= 0.0 {
            return fail_closed(grad, fd, k, tol);
        }
        let (mut xp, mut xm) = (x, x);
        xp[k] += h;
        xm[k] -= h;
        if !xp[k].is_finite()
            || !xm[k].is_finite()
            || xp[k].to_bits() == x[k].to_bits()
            || xm[k].to_bits() == x[k].to_bits()
            || xp[k].to_bits() == xm[k].to_bits()
        {
            return fail_closed(grad, fd, k, tol);
        }
        let (plus, minus) = (eval(xp), eval(xm));
        if !plus.v.is_finite() || !minus.v.is_finite() {
            return fail_closed(grad, fd, k, tol);
        }
        let deriv = (plus.v - minus.v) / denom;
        if !deriv.is_finite() {
            return fail_closed(grad, fd, k, tol);
        }
        fd.push(deriv);
    }
    let (mut max_rel_err, mut worst_index) = (0.0f64, 0usize);
    for k in 0..N {
        let scale = grad[k].abs().max(fd[k].abs()).max(1.0);
        let rel = (grad[k] - fd[k]).abs() / scale;
        if !rel.is_finite() {
            return fail_closed(grad, fd, k, tol);
        }
        if rel > max_rel_err {
            max_rel_err = rel;
            worst_index = k;
        }
    }
    let complete = grad.len() == N && fd.len() == N;
    GradCheckReport {
        grad,
        fd,
        max_rel_err,
        worst_index,
        tol,
        pass: complete && max_rel_err.is_finite() && max_rel_err <= tol,
    }
}

/// The arithmetic the distance formulas need, implemented by `f64`, [`Dual`], [`Dual2`] and [`Jet`],
/// so each formula is written once and evaluated at the order a caller needs.
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

impl<const N: usize> Scalar for Dual2<N> {
    fn coord(v: f64, k: usize, i: usize, fixed: bool) -> Self {
        if fixed {
            Self::constant(v)
        } else {
            Self::var(v, 3 * k + i)
        }
    }
    fn value(&self) -> f64 {
        self.v.v
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

    /// f(x, y) = x²y / (1 + y) + √x·ln(y) against hand derivatives and nested `Dual2`.
    #[test]
    fn jet_matches_hand_derivatives_and_nested_dual2() {
        let (x0, y0) = (1.7, 2.3);
        let (x, y) = (Jet::<2>::var(x0, 0), Jet::<2>::var(y0, 1));
        let one = Jet::constant(1.0);
        let f = x * x * y / (one + y) + x.sqrt() * y.ln();

        let (dx, dy) = (Dual2::<2>::var(x0, 0), Dual2::<2>::var(y0, 1));
        let done = Dual2::constant(1.0);
        let df = dx * dx * dy / (done + dy) + dx.sqrt() * dy.ln();

        let want_v = x0 * x0 * y0 / (1.0 + y0) + x0.sqrt() * y0.ln();
        let fx = 2.0 * x0 * y0 / (1.0 + y0) + 0.5 / x0.sqrt() * y0.ln();
        let fy = x0 * x0 / ((1.0 + y0) * (1.0 + y0)) + x0.sqrt() / y0;
        let fxx = 2.0 * y0 / (1.0 + y0) - 0.25 * x0.powf(-1.5) * y0.ln();
        let fxy = 2.0 * x0 / ((1.0 + y0) * (1.0 + y0)) + 0.5 / (x0.sqrt() * y0);
        let fyy = -2.0 * x0 * x0 / (1.0 + y0).powi(3) - x0.sqrt() / (y0 * y0);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12 * b.abs().max(1.0);
        assert!(close(f.v, want_v) && close(df.value(), want_v));
        assert!(close(f.g[0], fx) && close(f.g[1], fy));
        assert!(
            close(f.h[0][0], fxx)
                && close(f.h[0][1], fxy)
                && close(f.h[1][0], fxy)
                && close(f.h[1][1], fyy)
        );
        let dh = df.hess();
        for i in 0..2 {
            assert!(close(f.g[i], df.grad()[i]));
            for j in 0..2 {
                assert!(close(f.h[i][j], dh[i][j]));
            }
        }
        let dir = [0.6, -0.8];
        let (v_dir, g_dir, h_dir) = second_directional([x0, y0], dir, |[x, y]| {
            let one = Dual2::constant(1.0);
            x * x * y / (one + y) + x.sqrt() * y.ln()
        });
        assert!(close(v_dir, f.v));
        assert!(close(g_dir, f.g[0] * dir[0] + f.g[1] * dir[1]));
        let want_h_dir = f.h[0][0] * dir[0] * dir[0]
            + 2.0 * f.h[0][1] * dir[0] * dir[1]
            + f.h[1][1] * dir[1] * dir[1];
        assert!(close(h_dir, want_h_dir));
    }

    #[test]
    fn gradcheck_passes_valid_dual_and_catches_leaked_primal() {
        let ok = gradcheck(
            |[x, y]: [Dual<2>; 2]| {
                let one = Dual::constant(1.0);
                x * x * y / (one + y) + x.sqrt() * y.ln()
            },
            [1.7, 2.3],
            5e-6,
        );
        assert!(ok.pass && ok.max_rel_err < 1e-8, "{ok:?}");

        let bad = gradcheck(|[x]: [Dual<1>; 1]| Dual::constant(x.v) * x, [1.3], 5e-6);
        assert!(!bad.pass && bad.max_rel_err > 0.3, "{bad:?}");
    }
}
