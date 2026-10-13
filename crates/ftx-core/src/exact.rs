//! The exact 3D and 2D orientation predicates (Shewchuk 1997): the sign of the orientation determinant, exact
//! for finite inputs, so a geometric audit ("does this edge pass through that triangle?") never flips on
//! rounding. A floating-point filter with a proven error bound decides general-position cases; the rest
//! go to exact expansion arithmetic.
//!
//! Provenance: lifted from frankensim at commit `e76c942133e8855ce20056be9427c3f30393e123` (MIT +
//! rider): [`orient3d`] from `crates/fs-ivl/src/predicates.rs` (stage-A filter, axis-aligned shortcut,
//! exact stage), the expansion arithmetic it uses from `crates/fs-ivl/src/expansion.rs`, and the
//! error-free transformations from `crates/fs-math/src/eft.rs`. Left out: the stage telemetry, the other
//! predicates and simulation of simplicity.
//!
//! Certified domain (Shewchuk's caveat): signs are exact while coordinate differences and their
//! degree-3 products stay in the normal f64 range. Non-finite inputs and detected overflow panic rather
//! than return a sign; callers check finiteness first.
//!
//! Determinism: +, −, × and `mul_add` only (an explicit fma is correctly rounded on every target).

/// The exact sign of a determinant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    /// Determinant < 0.
    Negative,
    /// Determinant = 0 exactly.
    Zero,
    /// Determinant > 0.
    Positive,
}

impl Sign {
    fn of(x: f64) -> Self {
        if x > 0.0 {
            Self::Positive
        } else if x < 0.0 {
            Self::Negative
        } else {
            Self::Zero
        }
    }

    fn of_i32(x: i32) -> Self {
        match x.cmp(&0) {
            std::cmp::Ordering::Greater => Self::Positive,
            std::cmp::Ordering::Less => Self::Negative,
            std::cmp::Ordering::Equal => Self::Zero,
        }
    }
}

// Shewchuk's epsilon is the half-ulp 2⁻⁵³; the stage-A constant is the paper's proven forward-error
// bound for exactly this evaluation order.
const EPS: f64 = f64::EPSILON / 2.0;
const O3DERRBOUND_A: f64 = (7.0 + 56.0 * EPS) * EPS;

/// Exact sign of the 3D orientation determinant `det[a − d; b − d; c − d]`: positive iff `d` lies below
/// the plane through `a, b, c` seen counterclockwise from above (Shewchuk's convention, the negative of
/// `(b − a) × (c − a) · (d − a)`).
///
/// # Panics
/// On a non-finite coordinate or when the arithmetic leaves the finite range.
#[must_use]
pub fn orient3d(pa: [f64; 3], pb: [f64; 3], pc: [f64; 3], pd: [f64; 3]) -> Sign {
    assert!(
        [pa, pb, pc, pd].iter().flatten().all(|x| x.is_finite()),
        "orient3d requires finite coordinates"
    );
    let adx = pa[0] - pd[0];
    let bdx = pb[0] - pd[0];
    let cdx = pc[0] - pd[0];
    let ady = pa[1] - pd[1];
    let bdy = pb[1] - pd[1];
    let cdy = pc[1] - pd[1];
    let adz = pa[2] - pd[2];
    let bdz = pb[2] - pd[2];
    let cdz = pc[2] - pd[2];
    assert_finite(&[adx, bdx, cdx, ady, bdy, cdy, adz, bdz, cdz]);
    // Axis-aligned coplanarity, decided exactly: for finite floats x − y is zero iff x == y, so three
    // zero differences on one axis put all four points in one axis-aligned plane.
    if (adx == 0.0 && bdx == 0.0 && cdx == 0.0)
        || (ady == 0.0 && bdy == 0.0 && cdy == 0.0)
        || (adz == 0.0 && bdz == 0.0 && cdz == 0.0)
    {
        return Sign::Zero;
    }

    let bdxcdy = bdx * cdy;
    let cdxbdy = cdx * bdy;
    let cdxady = cdx * ady;
    let adxcdy = adx * cdy;
    let adxbdy = adx * bdy;
    let bdxady = bdx * ady;

    let det = adz * (bdxcdy - cdxbdy) + bdz * (cdxady - adxcdy) + cdz * (adxbdy - bdxady);
    let permanent = (bdxcdy.abs() + cdxbdy.abs()) * adz.abs()
        + (cdxady.abs() + adxcdy.abs()) * bdz.abs()
        + (adxbdy.abs() + bdxady.abs()) * cdz.abs();
    assert_finite(&[
        bdxcdy, cdxbdy, cdxady, adxcdy, adxbdy, bdxady, det, permanent,
    ]);
    let errbound = O3DERRBOUND_A * permanent;
    if det > errbound || -det > errbound {
        return Sign::of(det);
    }

    // Exact: the same determinant over exact two-component differences.
    let ax = diff_expansion(pa[0], pd[0]);
    let bx = diff_expansion(pb[0], pd[0]);
    let cx = diff_expansion(pc[0], pd[0]);
    let ay = diff_expansion(pa[1], pd[1]);
    let by = diff_expansion(pb[1], pd[1]);
    let cy = diff_expansion(pc[1], pd[1]);
    let az = diff_expansion(pa[2], pd[2]);
    let bz = diff_expansion(pb[2], pd[2]);
    let cz = diff_expansion(pc[2], pd[2]);

    let m_a = expansion_diff(&expansion_product(&bx, &cy), &expansion_product(&cx, &by));
    let m_b = expansion_diff(&expansion_product(&cx, &ay), &expansion_product(&ax, &cy));
    let m_c = expansion_diff(&expansion_product(&ax, &by), &expansion_product(&bx, &ay));
    let t =
        fast_expansion_sum_zeroelim(&expansion_product(&az, &m_a), &expansion_product(&bz, &m_b));
    let det_e = fast_expansion_sum_zeroelim(&t, &expansion_product(&cz, &m_c));
    Sign::of_i32(expansion_sign(&det_e))
}

const CCWERRBOUND_A: f64 = (3.0 + 16.0 * EPS) * EPS;

/// Exact sign of the 2D orientation determinant `det[a − c; b − c]`: positive iff `a, b, c` turn
/// counterclockwise (`c` left of the directed line `a → b`), Shewchuk's stage-A filter then the exact
/// expansion.
///
/// # Panics
/// On a non-finite coordinate or when the arithmetic leaves the finite range.
#[must_use]
pub fn orient2d(pa: [f64; 2], pb: [f64; 2], pc: [f64; 2]) -> Sign {
    assert!(
        [pa, pb, pc].iter().flatten().all(|x| x.is_finite()),
        "orient2d requires finite coordinates"
    );
    let detleft = (pa[0] - pc[0]) * (pb[1] - pc[1]);
    let detright = (pa[1] - pc[1]) * (pb[0] - pc[0]);
    let det = detleft - detright;
    assert_finite(&[detleft, detright, det]);
    let detsum = if detleft > 0.0 {
        if detright <= 0.0 {
            return Sign::of(det);
        }
        detleft + detright
    } else if detleft < 0.0 {
        if detright >= 0.0 {
            return Sign::of(det);
        }
        -detleft - detright
    } else {
        return Sign::of(det);
    };
    let errbound = CCWERRBOUND_A * detsum;
    if det >= errbound || -det >= errbound {
        return Sign::of(det);
    }
    let ax = diff_expansion(pa[0], pc[0]);
    let ay = diff_expansion(pa[1], pc[1]);
    let bx = diff_expansion(pb[0], pc[0]);
    let by = diff_expansion(pb[1], pc[1]);
    let det_e = expansion_diff(&expansion_product(&ax, &by), &expansion_product(&ay, &bx));
    Sign::of_i32(expansion_sign(&det_e))
}

#[track_caller]
fn assert_finite(values: &[f64]) {
    assert!(
        values.iter().all(|x| x.is_finite()),
        "orient3d arithmetic left the finite certified domain"
    );
}

// --- Error-free transformations (fs-math eft.rs) ---

/// Knuth's `TwoSum`: `(s, e)` with `s = fl(a + b)` and `s + e = a + b` exactly.
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    let e = (a - (s - bb)) + (b - bb);
    (s, e)
}

/// Dekker's `FastTwoSum`: as [`two_sum`], requiring `|a| ≥ |b|` (or a zero).
fn quick_two_sum(a: f64, b: f64) -> (f64, f64) {
    debug_assert!(
        a == 0.0 || b == 0.0 || a.abs() >= b.abs() || a.is_nan() || b.is_nan(),
        "quick_two_sum precondition |a| >= |b| violated: a={a}, b={b}"
    );
    let s = a + b;
    let e = b - (s - a);
    (s, e)
}

/// `TwoProd` via fma: `(p, e)` with `p = fl(a·b)` and `p + e = a·b` exactly.
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let e = a.mul_add(b, -p);
    (p, e)
}

// --- Expansion arithmetic (fs-ivl expansion.rs): nonzero, nonoverlapping components in increasing
// magnitude whose exact sum is the value; the empty expansion is zero. ---

/// Exact difference `(d, tail)`, `d = fl(a − b)`, `d + tail = a − b`.
fn two_diff(a: f64, b: f64) -> (f64, f64) {
    assert!(
        a.is_finite() && b.is_finite(),
        "exact difference requires finite operands: a={a}, b={b}"
    );
    let (d, tail) = two_sum(a, -b);
    assert!(
        d.is_finite() && tail.is_finite(),
        "exact difference overflowed the finite expansion domain: a={a}, b={b}"
    );
    (d, tail)
}

fn push_nonzero(h: &mut Vec<f64>, x: f64) {
    assert!(
        x.is_finite(),
        "expansion arithmetic left the finite representable domain"
    );
    if x != 0.0 {
        h.push(x);
    }
}

/// Exact sum of two expansions (Shewchuk's FAST-EXPANSION-SUM with zero elimination).
fn fast_expansion_sum_zeroelim(e: &[f64], f: &[f64]) -> Vec<f64> {
    if e.is_empty() {
        return f.to_vec();
    }
    if f.is_empty() {
        return e.to_vec();
    }
    let mut h = Vec::with_capacity(e.len() + f.len());
    let (mut ei, mut fi) = (0usize, 0usize);
    // Shewchuk's branchless |f| > |e| test: true when e[ei] has the smaller magnitude.
    let smaller_is_e = |ei: usize, fi: usize| -> bool { (f[fi] > e[ei]) == (f[fi] > -e[ei]) };
    let mut q = if smaller_is_e(ei, fi) {
        ei += 1;
        e[ei - 1]
    } else {
        fi += 1;
        f[fi - 1]
    };
    let mut first = true;
    while ei < e.len() && fi < f.len() {
        let now = if smaller_is_e(ei, fi) {
            ei += 1;
            e[ei - 1]
        } else {
            fi += 1;
            f[fi - 1]
        };
        let (qnew, hh) = if first {
            quick_two_sum(now, q) // the second-smallest component dominates the smallest
        } else {
            two_sum(q, now)
        };
        first = false;
        q = qnew;
        push_nonzero(&mut h, hh);
    }
    for &now in &e[ei..] {
        let (qnew, hh) = two_sum(q, now);
        q = qnew;
        push_nonzero(&mut h, hh);
    }
    for &now in &f[fi..] {
        let (qnew, hh) = two_sum(q, now);
        q = qnew;
        push_nonzero(&mut h, hh);
    }
    push_nonzero(&mut h, q);
    h
}

/// Exact product of an expansion by one f64 (SCALE-EXPANSION with zero elimination, fma-backed).
fn scale_expansion_zeroelim(e: &[f64], b: f64) -> Vec<f64> {
    let Some((&e0, rest)) = e.split_first() else {
        return Vec::new();
    };
    let mut h = Vec::with_capacity(2 * e.len());
    let (mut q, hh) = two_prod(e0, b);
    push_nonzero(&mut h, hh);
    for &enow in rest {
        let (product1, product0) = two_prod(enow, b);
        let (sum, hh) = two_sum(q, product0);
        push_nonzero(&mut h, hh);
        let (qnew, hh) = quick_two_sum(product1, sum); // |product1| ≥ |sum| (Shewchuk, Theorem 19)
        q = qnew;
        push_nonzero(&mut h, hh);
    }
    push_nonzero(&mut h, q);
    h
}

/// Exact product of two expansions.
fn expansion_product(a: &[f64], b: &[f64]) -> Vec<f64> {
    match a.len() {
        0 => Vec::new(),
        1 => scale_expansion_zeroelim(b, a[0]),
        _ => {
            let mid = a.len() / 2;
            let lo = expansion_product(&a[..mid], b);
            let hi = expansion_product(&a[mid..], b);
            fast_expansion_sum_zeroelim(&lo, &hi)
        }
    }
}

/// Exact `a − b`.
fn expansion_diff(a: &[f64], b: &[f64]) -> Vec<f64> {
    let neg: Vec<f64> = b.iter().map(|&x| -x).collect();
    fast_expansion_sum_zeroelim(a, &neg)
}

/// The sign of an expansion: that of its largest (last) component, 0 when empty.
fn expansion_sign(e: &[f64]) -> i32 {
    assert!(
        e.iter().all(|x| x.is_finite()),
        "expansion sign requires finite components: {e:?}"
    );
    match e.last() {
        Some(&x) if x > 0.0 => 1,
        Some(&x) if x < 0.0 => -1,
        _ => 0,
    }
}

/// An exact coordinate difference as an expansion of at most two components.
fn diff_expansion(a: f64, b: f64) -> Vec<f64> {
    let (d, tail) = two_diff(a, b);
    let mut v = Vec::with_capacity(2);
    push_nonzero(&mut v, tail);
    push_nonzero(&mut v, d);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o3d_i128(a: [i64; 3], b: [i64; 3], c: [i64; 3], d: [i64; 3]) -> i128 {
        let v = |p: [i64; 3]| {
            [
                i128::from(p[0] - d[0]),
                i128::from(p[1] - d[1]),
                i128::from(p[2] - d[2]),
            ]
        };
        let (a, b, c) = (v(a), v(b), v(c));
        a[2] * (b[0] * c[1] - c[0] * b[1])
            + b[2] * (c[0] * a[1] - a[0] * c[1])
            + c[2] * (a[0] * b[1] - b[0] * a[1])
    }

    fn lcg(seed: &mut u64) -> i64 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((*seed >> 24).cast_signed() % 41) - 20 // a small lattice forces degeneracies
    }

    /// orient2d on a lattice against an i128 oracle, and on nearly collinear points the filter cannot
    /// decide (c on the line a→b, then nudged by one ulp).
    #[test]
    fn orient2d_matches_the_oracle_and_resolves_near_collinear_points() {
        let mut seed = 0x0217_2D00_0000_0011u64;
        for _ in 0..4000 {
            let mut p = || [lcg(&mut seed), lcg(&mut seed)];
            let (a, b, c) = (p(), p(), p());
            let want = i128::from(a[0] - c[0]) * i128::from(b[1] - c[1])
                - i128::from(a[1] - c[1]) * i128::from(b[0] - c[0]);
            let f = |q: [i64; 2]| [q[0] as f64, q[1] as f64];
            assert_eq!(
                orient2d(f(a), f(b), f(c)),
                Sign::of_i32(want.signum() as i32)
            );
        }
        let (a, b) = ([0.1, 0.1], [0.7, 0.7]);
        let c = [0.3, 0.3];
        assert_eq!(orient2d(a, b, c), Sign::Zero);
        let up = [0.3, f64::from_bits(0.3f64.to_bits() + 1)];
        let down = [0.3, f64::from_bits(0.3f64.to_bits() - 1)];
        assert_eq!(orient2d(a, b, up), Sign::Positive);
        assert_eq!(orient2d(a, b, down), Sign::Negative);
    }

    /// Lifted battery: on a small integer lattice the sign equals an i128 oracle's; every other
    /// quadruple is made exactly coplanar (d = a + s(b − a) + t(c − a), s, t integers).
    #[test]
    fn lattice_battery_matches_i128_oracle() {
        let mut seed = 0x5EED_09E0_0000_003Du64;
        let mut zeros = 0;
        for round in 0..4000 {
            let mut p = || [lcg(&mut seed), lcg(&mut seed), lcg(&mut seed)];
            let (a, b, c, mut d) = (p(), p(), p(), p());
            if round % 2 == 0 {
                let (s, t) = (d[0] % 3, d[1] % 3);
                d = std::array::from_fn(|i| a[i] + s * (b[i] - a[i]) + t * (c[i] - a[i]));
            }
            let f = |q: [i64; 3]| q.map(|v| v as f64);
            let want = Sign::of_i32(o3d_i128(a, b, c, d).signum() as i32);
            assert_eq!(
                orient3d(f(a), f(b), f(c), f(d)),
                want,
                "round {round}: {a:?} {b:?} {c:?} {d:?}"
            );
            zeros += usize::from(want == Sign::Zero);
        }
        assert!(zeros > 0, "the battery hit no exact degeneracy");
    }

    /// A point exactly on a tilted plane is `Zero`, and one ulp off it is on the side it is on.
    #[test]
    fn one_ulp_off_a_tilted_plane() {
        // The plane x + y + z = 1 through three axis points; d exactly on it, then one ulp up or down
        // in z.
        let (a, b, c) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let on = [0.25, 0.25, 0.5];
        assert_eq!(orient3d(a, b, c, on), Sign::Zero);
        let up = [on[0], on[1], f64::from_bits(on[2].to_bits() + 1)];
        let down = [on[0], on[1], f64::from_bits(on[2].to_bits() - 1)];
        // (b − a) × (c − a) = (1, 1, 1) points up, so a point above has the negative sign.
        assert_eq!(orient3d(a, b, c, [1.0, 1.0, 1.0]), Sign::Negative);
        assert_eq!(orient3d(a, b, c, up), Sign::Negative);
        assert_eq!(orient3d(a, b, c, down), Sign::Positive);
    }
}
