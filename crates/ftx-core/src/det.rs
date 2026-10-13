//! Deterministic elementary functions (strict mode), built only from IEEE-754 arithmetic
//! (+, −, ×, ÷, `mul_add`, `sqrt`), each correctly rounded and therefore bit-identical on every
//! conforming target, native and wasm alike. No platform libm.
//!
//! Provenance: lifted from frankensim `crates/fs-math/src/det.rs` at commit
//! `f58c1acb88c7525ac5a2c3bac9531bf9a182b97b` (same author, MIT + OpenAI/Anthropic rider). The bodies
//! of [`exp`], [`expm1`], [`ln`], [`sin`], [`cos`], [`sin_cos`], [`tanh`], [`sqrt`], [`hypot`],
//! [`atan`], [`atan2`], [`asin`], [`acos`] and [`powi`] are unchanged, so their bits match frankensim's
//! cross-ISA golden hash (test `frankensim_core_golden_hash`). Changes on lifting:
//! - No Payne–Hanek path: for |x| > [`TRIG_DOMAIN`] (2²⁰ rad) the trig functions return NaN instead of
//!   reducing with 1280-bit arithmetic. Pattern and body angles are a few radians; a NaN there is a
//!   bug upstream, and every caller refuses non-finite values.
//! - Omitted: `erf`, `erfc`, `pow`, double-double and quad-double support (unused here). [`tan`] was
//!   added later from the same commit (its body over this file's reduction), for the fez's arc controls.
//! - Added: [`cube`], a correctly rounded x³ (closest to numpy's `x ** 3`, see its docs), and
//!   [`ulp_distance`] for tests.
//!
//! Algorithms (classic, chosen for pure-arithmetic implementability):
//! - `exp`/`expm1`: k = round(x/ln2) reduction with two-part ln2, degree-13 Taylor/Horner core on
//!   |r| ≤ ln2/2, exact 2^k scaling via exponent bits.
//! - `ln`: mantissa reduction to [√½, √2), atanh series in s = (m−1)/(m+1) (|s| ≤ 0.1716), two-part
//!   k·ln2 recombination.
//! - `sin`/`cos`/`sin_cos`: Cody–Waite three-part π/2 reduction (plus a fourth chunk near zeros) for
//!   |x| ≤ 2²⁰; Taylor cores on |r| ≤ π/4.
//! - `tanh`: expm1(2x)/(expm1(2x)+2) with odd symmetry by construction.
//! - `atan`: two reductions to |t| ≤ tan(π/12), then a 16-term odd Taylor core; `atan2`, `asin`,
//!   `acos` build on it.

/// ULP budget for [`exp`].
pub const EXP_ULP_BUDGET: u64 = 3;
/// ULP budget for [`expm1`]. frankensim declares 3, but its test range rarely samples just above
/// ln2/2, where `exp(x) − 1` magnifies exp's error and 4 ULP is measured (x ≈ 0.3469).
pub const EXPM1_ULP_BUDGET: u64 = 4;
/// ULP budget for [`ln`].
pub const LN_ULP_BUDGET: u64 = 3;
/// ULP budget for [`sin`] within |x| ≤ [`TRIG_DOMAIN`].
pub const SIN_ULP_BUDGET: u64 = 3;
/// ULP budget for [`cos`] within |x| ≤ [`TRIG_DOMAIN`].
pub const COS_ULP_BUDGET: u64 = 3;
/// ULP budget for [`tanh`].
pub const TANH_ULP_BUDGET: u64 = 5;
/// ULP budget for [`atan`] and [`atan2`].
pub const ATAN_ULP_BUDGET: u64 = 4;
/// ULP budget for [`asin`] and [`acos`]: atan2's budget plus the rounding of the factored
/// complement √((1−x)(1+x)).
pub const ASIN_ULP_BUDGET: u64 = 6;
/// ULP budget for [`hypot`].
pub const HYPOT_ULP_BUDGET: u64 = 2;

/// Largest |x| the trig functions reduce (2²⁰); beyond it they return NaN.
pub const TRIG_DOMAIN: f64 = 1_048_576.0;

// Exact bit patterns (fdlibm heritage). The hi parts have ≥ 20 trailing zero mantissa bits, so
// k·LN2_HI (|k| ≤ 2¹⁰) and j·PIO2_* (|j| ≤ 2²⁰) are exact products. Decimal literals are not
// acceptable here: they round to neighbouring doubles without the trailing zeros.
const LN2_HI: f64 = f64::from_bits(0x3FE6_2E42_FEE0_0000); // 6.9314718036912382e-1
const LN2_LO: f64 = f64::from_bits(0x3DEA_39EF_3579_3C76); // 1.9082149292705877e-10
const LOG2_E: f64 = std::f64::consts::LOG2_E;

/// e^x.
#[must_use]
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x > 709.782_712_893_384 {
        return f64::INFINITY;
    }
    if x < -745.133_219_101_941_1 {
        return 0.0;
    }
    let k = (x * LOG2_E).round();
    // r = x − k·ln2, in two parts to keep |r| ≤ ln2/2 accurate.
    let r = (-k).mul_add(LN2_LO, (-k).mul_add(LN2_HI, x));
    scale_by_2k(exp_core(r), k as i64)
}

/// e^x − 1, accurate near zero.
#[must_use]
pub fn expm1(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x > 709.782_712_893_384 {
        return f64::INFINITY;
    }
    if x < -37.0 {
        return -1.0; // e^x < 2⁻⁵³: relative to 1 it has vanished
    }
    if x.abs() < 0.5 * std::f64::consts::LN_2 {
        return expm1_core(x); // no reduction: keeps the small-x accuracy
    }
    exp(x) - 1.0
}

/// Natural logarithm.
#[must_use]
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }
    // Normalize subnormals, then split x = 2^k · m with m ∈ [√½, √2).
    let (x, sub_k) = if x < f64::MIN_POSITIVE {
        (x * 9_007_199_254_740_992.0, -53i64) // ×2⁵³
    } else {
        (x, 0)
    };
    let bits = x.to_bits();
    let mut k = i64::try_from((bits >> 52) & 0x7FF).unwrap_or(0) - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > std::f64::consts::SQRT_2 {
        m *= 0.5;
        k += 1;
    }
    let k = k + sub_k;
    // atanh series: ln m = 2s(1 + s²/3 + s⁴/5 + …), s = (m−1)/(m+1).
    let s = (m - 1.0) / (m + 1.0);
    let z = s * s;
    // Terms through z⁹/19: truncation z¹⁰/21 ≈ 2e-17 relative at |s| ≤ 0.1716.
    let poly = z.mul_add(
        z.mul_add(
            z.mul_add(
                z.mul_add(
                    z.mul_add(
                        z.mul_add(
                            z.mul_add(
                                z.mul_add(z.mul_add(1.0 / 19.0, 1.0 / 17.0), 1.0 / 15.0),
                                1.0 / 13.0,
                            ),
                            1.0 / 11.0,
                        ),
                        1.0 / 9.0,
                    ),
                    1.0 / 7.0,
                ),
                1.0 / 5.0,
            ),
            1.0 / 3.0,
        ),
        1.0,
    );
    let kf = k as f64;
    kf.mul_add(LN2_LO, (2.0 * s).mul_add(poly, kf * LN2_HI))
}

/// sin(x); NaN for |x| > [`TRIG_DOMAIN`].
#[must_use]
pub fn sin(x: f64) -> f64 {
    let Some((r, quadrant)) = reduce_pio2(x) else {
        return f64::NAN;
    };
    match quadrant {
        0 => sin_core(r),
        1 => cos_core(r),
        2 => -sin_core(r),
        _ => -cos_core(r),
    }
}

/// cos(x); NaN for |x| > [`TRIG_DOMAIN`].
#[must_use]
pub fn cos(x: f64) -> f64 {
    let Some((r, quadrant)) = reduce_pio2(x) else {
        return f64::NAN;
    };
    match quadrant {
        0 => cos_core(r),
        1 => -sin_core(r),
        2 => -cos_core(r),
        _ => sin_core(r),
    }
}

/// `(sin(x), cos(x))` with one shared range reduction; each component is bit-identical to [`sin`]
/// and [`cos`].
#[must_use]
pub fn sin_cos(x: f64) -> (f64, f64) {
    let Some((r, quadrant)) = reduce_pio2(x) else {
        return (f64::NAN, f64::NAN);
    };
    let sine = sin_core(r);
    let cosine = cos_core(r);
    match quadrant {
        0 => (sine, cosine),
        1 => (cosine, -sine),
        2 => (-sine, -cosine),
        _ => (-cosine, sine),
    }
}

/// tan(x) = sin/cos of the shared reduction (frankensim `fs-math` `det::tan`, same commit); NaN for
/// |x| > [`TRIG_DOMAIN`].
#[must_use]
pub fn tan(x: f64) -> f64 {
    let Some((r, quadrant)) = reduce_pio2(x) else {
        return f64::NAN;
    };
    let s = sin_core(r);
    let c = cos_core(r);
    if quadrant & 1 == 0 { s / c } else { -(c / s) }
}

/// tanh(x). Odd symmetry holds bitwise: the magnitude is computed once and the sign re-applied.
#[must_use]
pub fn tanh(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    let a = x.abs();
    let mag = if a > 20.0 {
        1.0
    } else {
        // tanh a = t / (t + 2), t = expm1(2a): accurate at every scale.
        let t = expm1(2.0 * a);
        t / (t + 2.0)
    };
    if x.is_sign_negative() { -mag } else { mag }
}

/// √x. IEEE-754 requires correct rounding of sqrt, so the hardware instruction is the
/// deterministic implementation (0 ULP).
#[must_use]
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// √(x² + y²), overflow/underflow-safe and deterministic (unlike `f64::hypot`, whose platform libm
/// differs across ISAs). Symmetric bitwise. `hypot(±∞, y) = +∞` for every `y`, including NaN; any
/// other NaN input yields NaN.
#[must_use]
pub fn hypot(x: f64, y: f64) -> f64 {
    if x.is_infinite() || y.is_infinite() {
        return f64::INFINITY;
    }
    // Must precede the max-ordering: `NaN >= b` is false, so hypot(NaN, 0.0) would return 0.0.
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let (a, b) = (x.abs(), y.abs());
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    if hi == 0.0 {
        return 0.0;
    }
    let r = lo / hi;
    hi * sqrt(r.mul_add(r, 1.0))
}

/// x³, correctly rounded: the exact cube is formed as a double-double and rounded once.
///
/// numpy's `x ** 3` calls the platform `pow`, which is not correctly rounded and differs between
/// platforms (on macOS it disagrees with the correctly rounded cube on 65 of 40,760 Bézier
/// parameters, by one ULP). The correctly rounded value is the closest deterministic stand-in.
#[must_use]
pub fn cube(x: f64) -> f64 {
    let p = x * x;
    let pe = x.mul_add(x, -p); // x² = p + pe exactly
    let r = p * x;
    let re = p.mul_add(x, -r); // p·x = r + re exactly
    r + pe.mul_add(x, re)
}

/// x^n by binary exponentiation (one rounding per multiply; not correctly rounded). n = 0 returns
/// 1.0 for every x, matching `f64::powi`.
#[must_use]
pub fn powi(x: f64, n: i32) -> f64 {
    fn unsigned(mut a: f64, mut b: u64) -> f64 {
        let mut r = 1.0f64;
        loop {
            if b & 1 == 1 {
                r *= a;
            }
            b /= 2;
            if b == 0 {
                break;
            }
            a *= a;
        }
        r
    }

    let b = i64::from(n).unsigned_abs();
    let positive = unsigned(x, b);
    if n >= 0 {
        return positive;
    }
    let reciprocal = 1.0 / positive;
    if reciprocal == 0.0 && x.is_finite() && x != 0.0 {
        return unsigned(1.0 / x, b);
    }
    reciprocal
}

// π/2 and π/6 as hi/lo pairs (hi = nearest f64; lo = the residual).
const PI_2_HI: f64 = std::f64::consts::FRAC_PI_2;
const PI_2_LO: f64 = 6.123_233_995_736_766e-17;
const PI_6_HI: f64 = std::f64::consts::FRAC_PI_6;
const PI_6_LO: f64 = -1.643_486_876_741_777e-17;
/// tan(π/12) rounded up (the reduction threshold; overlap is harmless).
const TAN_PI_12: f64 = 0.267_949_192_431_123_05;
const SQRT_3: f64 = 1.732_050_807_568_877_2;
const SQRT_3_LO: f64 = 1.020_508_405_819_248_3e-16;

/// atan(x). Odd bitwise (sign folded at entry). atan(±∞) = ±π/2.
#[must_use]
pub fn atan(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    let sign = if x.is_sign_negative() { -1.0 } else { 1.0 };
    let a = x.abs();
    if a.is_infinite() {
        return sign * (PI_2_HI + PI_2_LO);
    }
    // a > 1 → π/2 − atan(1/a).
    let (invert, t0) = if a > 1.0 { (true, 1.0 / a) } else { (false, a) };
    // Second reduction to |t| ≤ tan(π/12): atan(t) = π/6 + atan((√3·t − 1)/(√3 + t)).
    let (rotated, t) = if t0 > TAN_PI_12 {
        let num = SQRT_3.mul_add(t0, -1.0) + SQRT_3_LO * t0;
        (true, num / (SQRT_3 + t0))
    } else {
        (false, t0)
    };
    let base = atan_core(t);
    let mut v = if rotated {
        (PI_6_HI + base) + PI_6_LO
    } else {
        base
    };
    if invert {
        v = (PI_2_HI - v) + PI_2_LO;
    }
    sign * v
}

/// atan2(y, x) with IEEE special-case conventions. Sign symmetries hold bitwise.
#[must_use]
pub fn atan2(y: f64, x: f64) -> f64 {
    const PI_HI: f64 = std::f64::consts::PI;
    const PI_LO: f64 = 1.224_646_799_147_353_2e-16;
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let sign = if y.is_sign_negative() { -1.0 } else { 1.0 };
    let ay = y.abs();
    if ay == 0.0 {
        return if x.is_sign_negative() {
            sign * (PI_HI + PI_LO)
        } else {
            sign * 0.0
        };
    }
    if x == 0.0 {
        return sign * (PI_2_HI + PI_2_LO);
    }
    if x.is_infinite() {
        return match (x > 0.0, ay.is_infinite()) {
            (true, true) => sign * (PI_2_HI / 2.0 + PI_2_LO / 2.0), // π/4
            (true, false) => sign * 0.0,
            (false, true) => sign * 3.0f64.mul_add(PI_2_HI / 2.0, 3.0 * (PI_2_LO / 2.0)), // 3π/4
            (false, false) => sign * (PI_HI + PI_LO),
        };
    }
    if ay.is_infinite() {
        return sign * (PI_2_HI + PI_2_LO);
    }
    let base = atan(ay / x.abs());
    if x > 0.0 {
        sign * base
    } else {
        sign * ((PI_HI - base) + PI_LO)
    }
}

/// asin(x) via `atan2(x, √((1−x)(1+x)))`; the factored complement stays conditioned at |x| → 1.
/// NaN outside [−1, 1].
#[must_use]
pub fn asin(x: f64) -> f64 {
    if x.is_nan() || x.abs() > 1.0 {
        return f64::NAN;
    }
    atan2(x, ((1.0 - x) * (1.0 + x)).sqrt())
}

/// acos(x) via `atan2(√((1−x)(1+x)), x)`. acos(1) = +0 and acos(−1) = π exactly. NaN outside
/// [−1, 1].
#[must_use]
pub fn acos(x: f64) -> f64 {
    if x.is_nan() || x.abs() > 1.0 {
        return f64::NAN;
    }
    atan2(((1.0 - x) * (1.0 + x)).sqrt(), x)
}

/// ULP distance between two finite floats (bit patterns ordered as integers).
#[must_use]
pub fn ulp_distance(a: f64, b: f64) -> u64 {
    fn ordered(x: f64) -> i64 {
        #[allow(clippy::cast_possible_wrap)] // the wrap is the ordering trick
        let b = x.to_bits() as i64;
        if b < 0 {
            i64::MIN.wrapping_add(b.wrapping_neg())
        } else {
            b
        }
    }
    ordered(a).abs_diff(ordered(b))
}

// ---------------------------------------------------------------------------------------------
// Cores (pure Horner/mul_add: no table lookups, no platform calls).
// ---------------------------------------------------------------------------------------------

/// exp on |r| ≤ ln2/2 ≈ 0.347: Taylor to r¹³ (tail < 4e-17 relative).
fn exp_core(r: f64) -> f64 {
    expm1_core(r) + 1.0
}

/// expm1 on |r| ≤ ln2/2: r·(1 + r/2 + r²/6 + …), Horner in r.
fn expm1_core(r: f64) -> f64 {
    const C: [f64; 12] = [
        1.0 / 2.0,
        1.0 / 6.0,
        1.0 / 24.0,
        1.0 / 120.0,
        1.0 / 720.0,
        1.0 / 5_040.0,
        1.0 / 40_320.0,
        1.0 / 362_880.0,
        1.0 / 3_628_800.0,
        1.0 / 39_916_800.0,
        1.0 / 479_001_600.0,
        1.0 / 6_227_020_800.0,
    ];
    let mut p = C[11];
    for c in C[..11].iter().rev() {
        p = p.mul_add(r, *c);
    }
    r.mul_add(r * p, r) // r + r²·(poly) keeps the leading term exact
}

/// sin on |r| ≤ π/4: r − r³·P(r²), Taylor through r¹⁷.
fn sin_core(r: f64) -> f64 {
    let z = r * r;
    let p = z.mul_add(
        z.mul_add(
            z.mul_add(
                z.mul_add(
                    z.mul_add(
                        z.mul_add(
                            z.mul_add(-1.0 / 355_687_428_096_000.0, 1.0 / 1_307_674_368_000.0),
                            -1.0 / 6_227_020_800.0,
                        ),
                        1.0 / 39_916_800.0,
                    ),
                    -1.0 / 362_880.0,
                ),
                1.0 / 5_040.0,
            ),
            -1.0 / 120.0,
        ),
        1.0 / 6.0,
    );
    (-(z * r)).mul_add(p, r) // r − r³·P(z), leading term exact
}

/// cos on |r| ≤ π/4: 1 − z/2 + z²·(1/24 + z·Q(z)), Taylor through r¹⁶.
fn cos_core(r: f64) -> f64 {
    let z = r * r;
    let q = z.mul_add(
        z.mul_add(
            z.mul_add(
                z.mul_add(
                    z.mul_add(1.0 / 20_922_789_888_000.0, -1.0 / 87_178_291_200.0),
                    1.0 / 479_001_600.0,
                ),
                -1.0 / 3_628_800.0,
            ),
            1.0 / 40_320.0,
        ),
        -1.0 / 720.0,
    );
    let tail = z.mul_add(q, 1.0 / 24.0);
    (z * z).mul_add(tail, 1.0 - 0.5 * z)
}

/// Cody–Waite π/2 reduction: `(r, quadrant mod 4)`, or `None` for non-finite x and |x| beyond
/// [`TRIG_DOMAIN`].
fn reduce_pio2(x: f64) -> Option<(f64, u8)> {
    // fdlibm's 33-bit split of π/2 as exact bit patterns (j·PIO2_* exact for |j| ≤ 2²⁰).
    const PIO2_HI: f64 = f64::from_bits(0x3FF9_21FB_5440_0000);
    const PIO2_MID: f64 = f64::from_bits(0x3DD0_B461_1A60_0000);
    const PIO2_LO: f64 = f64::from_bits(0x3BA3_198A_2E00_0000);
    // Fourth chunk (fdlibm pio2_3t): matters only where the reduced r is near zero (x ≈ kπ).
    const PIO2_LO2: f64 = 8.478_427_660_368_9e-32;
    const TWO_OVER_PI: f64 = std::f64::consts::FRAC_2_PI;
    if !x.is_finite() || x.abs() > TRIG_DOMAIN {
        return None;
    }
    let j = (x * TWO_OVER_PI).round();
    let r = (-j).mul_add(
        PIO2_LO2,
        (-j).mul_add(PIO2_LO, (-j).mul_add(PIO2_MID, (-j).mul_add(PIO2_HI, x))),
    );
    let q = ((j as i64) & 3) as u8;
    Some((r, q))
}

/// Exact ×2^k via exponent arithmetic, handling under/overflow (two-step scaling at the extremes).
fn scale_by_2k(v: f64, k: i64) -> f64 {
    let clamp = k.clamp(-2000, 2000);
    let mut v = v;
    let mut remaining = clamp;
    while remaining != 0 {
        let step = remaining.clamp(-1000, 1000);
        v *= f64::from_bits(((1023 + step) as u64) << 52);
        remaining -= step;
    }
    v
}

/// Odd Taylor core for atan on |t| ≤ tan(π/12): 16 terms of t·Σ (−1)ᵏ·z^k/(2k+1).
fn atan_core(t: f64) -> f64 {
    let z = t * t;
    let mut p = -1.0 / 31.0;
    let coeffs = [
        29.0, 27.0, 25.0, 23.0, 21.0, 19.0, 17.0, 15.0, 13.0, 11.0, 9.0, 7.0, 5.0, 3.0,
    ];
    let mut sign = 1.0;
    for &d in &coeffs {
        p = z.mul_add(p, sign / d);
        sign = -sign;
    }
    let p = z.mul_add(p, 1.0);
    t * p
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCG_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
    const LCG_INCREMENT: u64 = 1_442_695_040_888_963_407;

    fn lcg(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(LCG_MULTIPLIER)
            .wrapping_add(LCG_INCREMENT);
        ((*seed >> 11) as f64) / ((1_u64 << 53) as f64)
    }

    fn fnv1a64(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// frankensim's strict-core fingerprint (fs-math `tests/conformance.rs`), verified there on
    /// aarch64-apple and x86-64. Equal bits prove the lift is faithful.
    #[test]
    fn frankensim_core_golden_hash() {
        let mut outputs = Vec::new();
        let mut seed = 0x0DD_BA11_u64;
        for _ in 0..25_000 {
            let t = lcg(&mut seed);
            let x = (t - 0.5) * 1.0e4;
            for value in [
                exp(x * 0.05),
                expm1(x * 0.05),
                ln(t + 1.0e-9),
                sin(x),
                cos(x),
                tanh(x * 0.01),
                sqrt(t * 1.0e6),
            ] {
                outputs.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        assert_eq!(fnv1a64(&outputs), 0xeb79_cab7_a016_43e5);
    }

    fn max_ulp(
        range: (f64, f64),
        n: usize,
        f: impl Fn(f64) -> f64,
        oracle: impl Fn(f64) -> f64,
    ) -> u64 {
        let mut seed = 42;
        let mut worst = 0;
        for _ in 0..n {
            let x = range.0 + (range.1 - range.0) * lcg(&mut seed);
            worst = worst.max(ulp_distance(f(x), oracle(x)));
        }
        worst
    }

    #[test]
    fn budgets_hold_against_platform_libm() {
        assert!(max_ulp((-700.0, 700.0), 50_000, exp, f64::exp) <= EXP_ULP_BUDGET);
        assert!(max_ulp((-1.0, 1.0), 50_000, expm1, f64::exp_m1) <= EXPM1_ULP_BUDGET);
        assert!(max_ulp((0.3466, 0.5), 50_000, expm1, f64::exp_m1) <= EXPM1_ULP_BUDGET);
        assert!(max_ulp((-1e-8, 1e-8), 50_000, expm1, f64::exp_m1) <= 1);
        assert!(max_ulp((1e-300, 1e300), 50_000, ln, f64::ln) <= LN_ULP_BUDGET);
        assert!(max_ulp((1e-6, 10.0), 50_000, ln, f64::ln) <= LN_ULP_BUDGET);
        assert!(max_ulp((-1e5, 1e5), 50_000, sin, f64::sin) <= SIN_ULP_BUDGET);
        assert!(max_ulp((-1e5, 1e5), 50_000, cos, f64::cos) <= COS_ULP_BUDGET);
        assert!(max_ulp((-30.0, 30.0), 50_000, tanh, f64::tanh) <= TANH_ULP_BUDGET);
        assert!(max_ulp((-1e3, 1e3), 50_000, atan, f64::atan) <= ATAN_ULP_BUDGET);
        assert!(max_ulp((-1.0, 1.0), 50_000, asin, f64::asin) <= ASIN_ULP_BUDGET);
        assert!(max_ulp((-1.0, 1.0), 50_000, acos, f64::acos) <= ASIN_ULP_BUDGET);
        let mut seed = 7;
        for _ in 0..50_000 {
            let y = (lcg(&mut seed) - 0.5) * 200.0;
            let x = (lcg(&mut seed) - 0.5) * 200.0;
            assert!(
                ulp_distance(atan2(y, x), y.atan2(x)) <= ATAN_ULP_BUDGET,
                "atan2({y}, {x})"
            );
            assert!(
                ulp_distance(hypot(x, y), x.hypot(y)) <= HYPOT_ULP_BUDGET,
                "hypot({x}, {y})"
            );
        }
    }

    #[test]
    fn sin_cos_is_bitwise_sin_and_cos() {
        let mut seed = 3;
        for _ in 0..10_000 {
            let x = (lcg(&mut seed) - 0.5) * 1e4;
            let (s, c) = sin_cos(x);
            assert_eq!(s.to_bits(), sin(x).to_bits());
            assert_eq!(c.to_bits(), cos(x).to_bits());
        }
    }

    #[test]
    fn tan_is_sin_over_cos_and_tracks_the_platform() {
        let mut seed = 5;
        for _ in 0..10_000 {
            let x = (lcg(&mut seed) - 0.5) * 20.0;
            let t = tan(x);
            // det-ok: the platform tan is the oracle here
            let want = x.tan();
            assert!(
                ulp_distance(t, want) <= 4 || (t - want).abs() <= 1e-15 * want.abs().max(1.0),
                "{x}"
            );
        }
        assert_eq!(tan(0.0).to_bits(), 0.0f64.to_bits());
        assert!((tan(std::f64::consts::FRAC_PI_4) - 1.0).abs() < 2e-16);
        assert!(tan(f64::INFINITY).is_nan() && tan(TRIG_DOMAIN * 2.0).is_nan());
    }

    #[test]
    fn trig_refuses_outside_its_domain() {
        assert!(sin(TRIG_DOMAIN * 2.0).is_nan());
        assert!(cos(f64::INFINITY).is_nan());
        assert!(sin_cos(f64::NAN).0.is_nan());
        assert_eq!(sin(TRIG_DOMAIN).to_bits(), sin(TRIG_DOMAIN).to_bits());
        assert!(sin(TRIG_DOMAIN).is_finite());
    }

    #[test]
    fn special_values() {
        assert_eq!(atan2(0.0, -1.0), std::f64::consts::PI);
        assert_eq!(acos(-1.0), std::f64::consts::PI);
        assert_eq!(acos(1.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(asin(1.0), std::f64::consts::FRAC_PI_2);
        assert!(hypot(f64::NAN, 0.0).is_nan());
        assert_eq!(hypot(f64::INFINITY, f64::NAN), f64::INFINITY);
        assert_eq!(hypot(3.0, 4.0), 5.0);
        assert_eq!(powi(2.0, 10), 1024.0);
        assert_eq!(powi(2.0, -2), 0.25);
        assert_eq!(ln(1.0), 0.0);
        assert_eq!(exp(0.0), 1.0);
    }

    /// The cube is exact whenever x³ is representable, and otherwise within half an ULP of a
    /// 128-bit-integer reference on dyadic inputs.
    #[test]
    fn cube_is_correctly_rounded() {
        assert_eq!(cube(3.0), 27.0);
        assert_eq!(cube(-0.5), -0.125);
        assert_eq!(cube(0.0).to_bits(), 0.0f64.to_bits());
        // x = m / 2^20 with m < 2^20: x³ = m³ / 2^60, m³ < 2^60 is exact in u128 and its correctly
        // rounded f64 is (m³ as f64) / 2^60 because the u128 → f64 conversion rounds to nearest even.
        let mut seed = 11;
        for _ in 0..20_000 {
            let m = (lcg(&mut seed) * f64::from(1u32 << 20)) as u64;
            let x = m as f64 / f64::from(1u32 << 20);
            let exact = (u128::from(m).pow(3)) as f64 / 2f64.powi(60);
            assert_eq!(cube(x), exact, "m = {m}");
        }
    }
}
