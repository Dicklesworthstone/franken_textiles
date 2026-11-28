//! Wang et al. (2021) Tight-Inclusion interval Continuous Collision Detection
//! (CCD) for finite-radius moving codimensional pairs (EE, PT, PE, PP),
//! intra-filament topological self-contact exclusion, and reduced start-pair
//! thickness offsets.

use std::collections::BTreeSet;

use super::{CodimContactError, DetMath, all_finite3, dot3, norm2_3, norm3, scale3, sub3};

/// Result of a conservative tight-inclusion CCD query over `alpha in [0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CcdStepVerdict {
    /// Guaranteed penetration-free step fraction in `(0.0, 1.0]`.
    pub alpha_max: f64,
    /// `true` if a sub-box could not be proven clear at tolerance (`alpha_max < 1.0`).
    pub collided: bool,
    /// Exact Euclidean centerline distance at `alpha = 0` in metres.
    pub initial_distance_m: f64,
    /// Number of interval sub-boxes examined.
    pub iterations: u32,
}

/// Numerical tolerances and budget for tight-inclusion CCD bisection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CcdConfig {
    /// Time bisection stopping tolerance (`> 0`).
    pub time_tol: f64,
    /// Parametric `(u, v)` bisection stopping tolerance (`> 0`).
    pub space_tol: f64,
    /// Maximum sub-boxes examined before returning the conservative lower bound.
    pub max_iters: u32,
}

impl Default for CcdConfig {
    fn default() -> Self {
        Self {
            time_tol: 1e-5,
            space_tol: 1e-4,
            max_iters: 4_096,
        }
    }
}

/// Topological metadata of a filament segment for intra-filament self-contact
/// exclusion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentTopology {
    /// Stable filament identifier.
    pub filament_id: u32,
    /// Zero-based segment index along this filament.
    pub seg_idx: usize,
    /// Total number of segments on this filament.
    pub seg_count: usize,
    /// Whether the filament closes back onto `seg_idx = 0`.
    pub closed: bool,
    /// Periodic seam shift if `closed && periodic_shift != [0, 0, 0]`.
    pub periodic_shift: [i32; 3],
    /// Cumulative rest arc length at segment start in metres.
    pub arc_start_m: f64,
    /// Cumulative rest arc length at segment end in metres.
    pub arc_end_m: f64,
    /// Total filament rest arc length in metres.
    pub total_arc_m: f64,
    /// Segment cross-sectional radius in metres.
    pub radius_m: f64,
}

/// Determine whether an intra-filament pair `(a in [0,0,0], b in image_shift_b)`
/// lies inside the topological self-contact exclusion horizon (`shared vertex`
/// or geodesic rest arc length `s_arc <= pi * (r_a + r_b)` along the same
/// filament chain).
///
/// Distinct `filament_id`s and distinct parallel periodic copies are NEVER
/// excluded.
#[must_use]
pub fn is_intra_filament_excluded(
    a: &SegmentTopology,
    b: &SegmentTopology,
    image_shift_b: [i32; 3],
) -> bool {
    if a.filament_id != b.filament_id {
        return false;
    }
    let horizon = core::f64::consts::PI * (a.radius_m + b.radius_m);
    let m_a = 0.5 * (a.arc_start_m + a.arc_end_m);
    let m_b = 0.5 * (b.arc_start_m + b.arc_end_m);

    if image_shift_b == [0, 0, 0] {
        if a.seg_idx == b.seg_idx || a.seg_idx.abs_diff(b.seg_idx) <= 1 {
            return true;
        }
        if a.closed && a.periodic_shift == [0, 0, 0] && a.seg_count > 0 {
            if (a.seg_idx == 0 && b.seg_idx + 1 == a.seg_count)
                || (b.seg_idx == 0 && a.seg_idx + 1 == a.seg_count)
            {
                return true;
            }
            let raw = (m_a - m_b).abs();
            let geo = raw.min((a.total_arc_m - raw).max(0.0));
            return geo <= horizon;
        }
        return (m_a - m_b).abs() <= horizon;
    }

    if a.closed && a.periodic_shift != [0, 0, 0] && a.seg_count > 0 {
        let ps = a.periodic_shift;
        if image_shift_b == ps {
            if a.seg_idx + 1 == a.seg_count && b.seg_idx == 0 {
                return true;
            }
            let geo = ((m_b + a.total_arc_m) - m_a).abs();
            return geo <= horizon;
        }
        if image_shift_b == [-ps[0], -ps[1], -ps[2]] {
            if a.seg_idx == 0 && b.seg_idx + 1 == a.seg_count {
                return true;
            }
            let geo = (m_a - (m_b - a.total_arc_m)).abs();
            return geo <= horizon;
        }
    }

    false
}

/// Compute the reduced start-pair thickness offset:
/// `d_offset_pair = min(d_offset, 0.5 * d0)`.
///
/// Fails closed with [`CodimContactError::InitialPenetration`] if `d0 <= 0.0`
/// or non-finite.
///
/// # Errors
/// Returns [`CodimContactError::NonFiniteInput`],
/// [`CodimContactError::InvalidParameter`], or
/// [`CodimContactError::InitialPenetration`].
pub fn reduced_start_pair_offset(d_offset: f64, d0: f64) -> Result<f64, CodimContactError> {
    if !d_offset.is_finite() {
        return Err(CodimContactError::NonFiniteInput {
            context: "reduced_start_pair_offset::d_offset",
        });
    }
    if d_offset < 0.0 {
        return Err(CodimContactError::InvalidParameter {
            detail: "d_offset must be non-negative",
        });
    }
    if !d0.is_finite() || d0 <= 0.0 {
        return Err(CodimContactError::InitialPenetration {
            min_clearance_m: d0,
        });
    }
    Ok(d_offset.min(0.5 * d0))
}

/// Conservative tight-inclusion CCD for a moving Point-Point pair
/// `(p0, p1)` from `t=0` (`start`) to `t=1` (`end`) with minimum clearance
/// `d_min`.
///
/// # Errors
/// Fails closed if inputs are non-finite, `d_min < 0`, config tolerances are
/// non-positive, or `d0 <= d_min`.
pub fn ccd_point_point<M: DetMath>(
    start: [[f64; 3]; 2],
    end: [[f64; 3]; 2],
    d_min: f64,
    config: CcdConfig,
) -> Result<CcdStepVerdict, CodimContactError> {
    validate_ccd_inputs(&start, &end, d_min, config)?;
    let d0 = norm3::<M>(sub3(start[0], start[1]));
    if d0 <= 0.0 || d0 <= d_min {
        return Err(CodimContactError::InitialPenetration {
            min_clearance_m: d0 - d_min,
        });
    }
    let v_max = norm3::<M>(sub3(end[0], start[0])) + norm3::<M>(sub3(end[1], start[1]));
    Ok(run_tight_inclusion(
        d0,
        d_min,
        v_max,
        config,
        false,
        false,
        |a, _, _| eval_pp_box(start, end, a),
    ))
}

/// Conservative tight-inclusion CCD for a moving Point-Edge pair
/// `(p0, [p1, p2])` from `t=0` (`start`) to `t=1` (`end`) with minimum
/// clearance `d_min`.
///
/// # Errors
/// Fails closed if inputs are non-finite, `d_min < 0`, config tolerances are
/// non-positive, or `d0 <= d_min`.
pub fn ccd_point_edge<M: DetMath>(
    start: [[f64; 3]; 3],
    end: [[f64; 3]; 3],
    d_min: f64,
    config: CcdConfig,
) -> Result<CcdStepVerdict, CodimContactError> {
    validate_ccd_inputs(&start, &end, d_min, config)?;
    let d0 = point_edge_dist::<M>(start[0], start[1], start[2]);
    if d0 <= 0.0 || d0 <= d_min {
        return Err(CodimContactError::InitialPenetration {
            min_clearance_m: d0 - d_min,
        });
    }
    let vp = norm3::<M>(sub3(end[0], start[0]));
    let ve = norm3::<M>(sub3(end[1], start[1])).max(norm3::<M>(sub3(end[2], start[2])));
    Ok(run_tight_inclusion(
        d0,
        d_min,
        vp + ve,
        config,
        true,
        false,
        |a, u, _| eval_pe_box(start, end, a, u),
    ))
}

/// Conservative tight-inclusion CCD for a moving Point-Triangle pair
/// `(p0, [p1, p2, p3])` from `t=0` (`start`) to `t=1` (`end`) with minimum
/// clearance `d_min`.
///
/// # Errors
/// Fails closed if inputs are non-finite, `d_min < 0`, config tolerances are
/// non-positive, or `d0 <= d_min`.
pub fn ccd_point_triangle<M: DetMath>(
    start: [[f64; 3]; 4],
    end: [[f64; 3]; 4],
    d_min: f64,
    config: CcdConfig,
) -> Result<CcdStepVerdict, CodimContactError> {
    validate_ccd_inputs(&start, &end, d_min, config)?;
    let d0 = point_triangle_dist::<M>(start[0], start[1], start[2], start[3]);
    if d0 <= 0.0 || d0 <= d_min {
        return Err(CodimContactError::InitialPenetration {
            min_clearance_m: d0 - d_min,
        });
    }
    let vp = norm3::<M>(sub3(end[0], start[0]));
    let vt = norm3::<M>(sub3(end[1], start[1]))
        .max(norm3::<M>(sub3(end[2], start[2])))
        .max(norm3::<M>(sub3(end[3], start[3])));
    Ok(run_tight_inclusion(
        d0,
        d_min,
        vp + vt,
        config,
        true,
        true,
        |a, u, v| eval_pt_box(start, end, a, u, v),
    ))
}

/// Conservative tight-inclusion CCD for a moving Edge-Edge pair
/// `([p0, p1], [p2, p3])` from `t=0` (`start`) to `t=1` (`end`) with minimum
/// clearance `d_min`.
///
/// # Errors
/// Fails closed if inputs are non-finite, `d_min < 0`, config tolerances are
/// non-positive, or `d0 <= d_min`.
pub fn ccd_edge_edge<M: DetMath>(
    start: [[f64; 3]; 4],
    end: [[f64; 3]; 4],
    d_min: f64,
    config: CcdConfig,
) -> Result<CcdStepVerdict, CodimContactError> {
    validate_ccd_inputs(&start, &end, d_min, config)?;
    let d0 = edge_edge_dist::<M>(start[0], start[1], start[2], start[3]);
    if d0 <= 0.0 || d0 <= d_min {
        return Err(CodimContactError::InitialPenetration {
            min_clearance_m: d0 - d_min,
        });
    }
    let va = norm3::<M>(sub3(end[0], start[0])).max(norm3::<M>(sub3(end[1], start[1])));
    let vb = norm3::<M>(sub3(end[2], start[2])).max(norm3::<M>(sub3(end[3], start[3])));
    Ok(run_tight_inclusion(
        d0,
        d_min,
        va + vb,
        config,
        true,
        true,
        |a, u, v| eval_ee_box(start, end, a, u, v),
    ))
}

// ---------------------------------------------------------------------------
// Exact Euclidean initial-distance helpers (for t = 0).
// ---------------------------------------------------------------------------

#[must_use]
pub(crate) fn point_edge_dist<M: DetMath>(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let e = sub3(b, a);
    let e2 = norm2_3(e);
    if e2 <= 1e-30 {
        return norm3::<M>(sub3(p, a));
    }
    let t = (dot3(sub3(p, a), e) / e2).clamp(0.0, 1.0);
    let r = sub3(sub3(p, a), scale3(e, t));
    norm3::<M>(r)
}

#[must_use]
pub(crate) fn edge_edge_dist<M: DetMath>(
    a0: [f64; 3],
    a1: [f64; 3],
    b0: [f64; 3],
    b1: [f64; 3],
) -> f64 {
    let d1 = sub3(a1, a0);
    let d2 = sub3(b1, b0);
    let r = sub3(a0, b0);
    let a = dot3(d1, d1);
    let e = dot3(d2, d2);
    let f = dot3(d2, r);
    let (s, t) = if a <= 1e-30 && e <= 1e-30 {
        (0.0, 0.0)
    } else if a <= 1e-30 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = dot3(d1, r);
        if e <= 1e-30 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = dot3(d1, d2);
            let denom = a * e - b * b;
            let mut s_val = if denom > 1e-30 * a * e {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t_nom = b * s_val + f;
            let t_val = if t_nom < 0.0 {
                s_val = (-c / a).clamp(0.0, 1.0);
                0.0
            } else if t_nom > e {
                s_val = ((b - c) / a).clamp(0.0, 1.0);
                1.0
            } else {
                t_nom / e
            };
            (s_val, t_val)
        }
    };
    let pa = [a0[0] + s * d1[0], a0[1] + s * d1[1], a0[2] + s * d1[2]];
    let pb = [b0[0] + t * d2[0], b0[1] + t * d2[1], b0[2] + t * d2[2]];
    norm3::<M>(sub3(pa, pb))
}

#[must_use]
pub(crate) fn point_triangle_dist<M: DetMath>(
    p: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
) -> f64 {
    let ab = sub3(b, a);
    let ac = sub3(c, a);
    let ap = sub3(p, a);
    let d1 = dot3(ab, ap);
    let d2 = dot3(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return norm3::<M>(ap);
    }
    let bp = sub3(p, b);
    let d3 = dot3(ab, bp);
    let d4 = dot3(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return norm3::<M>(bp);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return norm3::<M>(sub3(ap, scale3(ab, v)));
    }
    let cp = sub3(p, c);
    let d5 = dot3(ab, cp);
    let d6 = dot3(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return norm3::<M>(cp);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return norm3::<M>(sub3(ap, scale3(ac, w)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return norm3::<M>(sub3(bp, scale3(sub3(c, b), w)));
    }
    let denom = va + vb + vc;
    if denom <= 1e-30 {
        return point_edge_dist::<M>(p, a, b);
    }
    let v = vb / denom;
    let w = vc / denom;
    let closest = [
        a[0] + ab[0] * v + ac[0] * w,
        a[1] + ab[1] * v + ac[1] * w,
        a[2] + ab[2] * v + ac[2] * w,
    ];
    norm3::<M>(sub3(p, closest))
}

// ---------------------------------------------------------------------------
// Outward-rounded pure IEEE-754 interval arithmetic & tight-inclusion solver.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Ivl {
    lo: f64,
    hi: f64,
}

#[inline]
fn next_up_1ulp(x: f64) -> f64 {
    if x.is_nan() || (x.is_infinite() && x.is_sign_positive()) {
        return x;
    }
    if x == 0.0 {
        return f64::from_bits(1);
    }
    if x > 0.0 {
        f64::from_bits(x.to_bits() + 1)
    } else {
        f64::from_bits(x.to_bits() - 1)
    }
}

#[inline]
fn next_down_1ulp(x: f64) -> f64 {
    -next_up_1ulp(-x)
}

impl Ivl {
    #[inline]
    const fn point(x: f64) -> Self {
        Self { lo: x, hi: x }
    }

    #[inline]
    fn hull(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }

    #[inline]
    fn add(self, other: Self) -> Self {
        Self {
            lo: next_down_1ulp(self.lo + other.lo),
            hi: next_up_1ulp(self.hi + other.hi),
        }
    }

    #[inline]
    fn sub(self, other: Self) -> Self {
        Self {
            lo: next_down_1ulp(self.lo - other.hi),
            hi: next_up_1ulp(self.hi - other.lo),
        }
    }

    #[inline]
    fn mul(self, other: Self) -> Self {
        let p00 = self.lo * other.lo;
        let p01 = self.lo * other.hi;
        let p10 = self.hi * other.lo;
        let p11 = self.hi * other.hi;
        let lo = p00.min(p01).min(p10).min(p11);
        let hi = p00.max(p01).max(p10).max(p11);
        Self {
            lo: next_down_1ulp(lo),
            hi: next_up_1ulp(hi),
        }
    }

    #[inline]
    fn sq_lo(self) -> f64 {
        if self.lo <= 0.0 && self.hi >= 0.0 {
            0.0
        } else {
            let m = self.lo.abs().min(self.hi.abs());
            next_down_1ulp(m * m)
        }
    }
}

#[inline]
fn ivl_lerp3(a0: [f64; 3], a1: [f64; 3], t: Ivl) -> [Ivl; 3] {
    let one_m_t = Ivl::point(1.0).sub(t);
    [
        Ivl::point(a0[0]).mul(one_m_t).add(Ivl::point(a1[0]).mul(t)),
        Ivl::point(a0[1]).mul(one_m_t).add(Ivl::point(a1[1]).mul(t)),
        Ivl::point(a0[2]).mul(one_m_t).add(Ivl::point(a1[2]).mul(t)),
    ]
}

#[inline]
fn ivl_lerp_vec(p0: [Ivl; 3], p1: [Ivl; 3], u: Ivl) -> [Ivl; 3] {
    let one_m_u = Ivl::point(1.0).sub(u);
    [
        p0[0].mul(one_m_u).add(p1[0].mul(u)),
        p0[1].mul(one_m_u).add(p1[1].mul(u)),
        p0[2].mul(one_m_u).add(p1[2].mul(u)),
    ]
}

#[inline]
fn ivl_sub_vec(a: [Ivl; 3], b: [Ivl; 3]) -> [Ivl; 3] {
    [a[0].sub(b[0]), a[1].sub(b[1]), a[2].sub(b[2])]
}

#[inline]
fn hull_vec(a: [Ivl; 3], b: [Ivl; 3]) -> [Ivl; 3] {
    [a[0].hull(b[0]), a[1].hull(b[1]), a[2].hull(b[2])]
}

fn eval_pp_box(start: [[f64; 3]; 2], end: [[f64; 3]; 2], a: [f64; 2]) -> [Ivl; 3] {
    let r0 = ivl_sub_vec(
        ivl_lerp3(start[0], end[0], Ivl::point(a[0])),
        ivl_lerp3(start[1], end[1], Ivl::point(a[0])),
    );
    let r1 = ivl_sub_vec(
        ivl_lerp3(start[0], end[0], Ivl::point(a[1])),
        ivl_lerp3(start[1], end[1], Ivl::point(a[1])),
    );
    hull_vec(r0, r1)
}

fn eval_pe_box(start: [[f64; 3]; 3], end: [[f64; 3]; 3], a: [f64; 2], u: [f64; 2]) -> [Ivl; 3] {
    let mut out = None;
    for &ac in &a {
        let p0 = ivl_lerp3(start[0], end[0], Ivl::point(ac));
        let p1 = ivl_lerp3(start[1], end[1], Ivl::point(ac));
        let p2 = ivl_lerp3(start[2], end[2], Ivl::point(ac));
        for &uc in &u {
            let e = ivl_lerp_vec(p1, p2, Ivl::point(uc));
            let r = ivl_sub_vec(p0, e);
            out = Some(match out {
                None => r,
                Some(prev) => hull_vec(prev, r),
            });
        }
    }
    out.unwrap_or([Ivl::point(0.0); 3])
}

fn eval_pt_box(
    start: [[f64; 3]; 4],
    end: [[f64; 3]; 4],
    a: [f64; 2],
    u: [f64; 2],
    s: [f64; 2],
) -> [Ivl; 3] {
    let mut out = None;
    for &ac in &a {
        let ai = Ivl::point(ac);
        let p0 = ivl_lerp3(start[0], end[0], ai);
        let p1 = ivl_lerp3(start[1], end[1], ai);
        let p2 = ivl_lerp3(start[2], end[2], ai);
        let p3 = ivl_lerp3(start[3], end[3], ai);
        for &sc in &s {
            let p13 = ivl_lerp_vec(p1, p3, Ivl::point(sc));
            for &uc in &u {
                let tri = ivl_lerp_vec(p13, p2, Ivl::point(uc));
                let r = ivl_sub_vec(p0, tri);
                out = Some(match out {
                    None => r,
                    Some(prev) => hull_vec(prev, r),
                });
            }
        }
    }
    out.unwrap_or([Ivl::point(0.0); 3])
}

fn eval_ee_box(
    start: [[f64; 3]; 4],
    end: [[f64; 3]; 4],
    a: [f64; 2],
    u: [f64; 2],
    v: [f64; 2],
) -> [Ivl; 3] {
    let mut out = None;
    for &ac in &a {
        let ai = Ivl::point(ac);
        let p0 = ivl_lerp3(start[0], end[0], ai);
        let p1 = ivl_lerp3(start[1], end[1], ai);
        let p2 = ivl_lerp3(start[2], end[2], ai);
        let p3 = ivl_lerp3(start[3], end[3], ai);
        for &uc in &u {
            let pa = ivl_lerp_vec(p0, p1, Ivl::point(uc));
            for &vc in &v {
                let pb = ivl_lerp_vec(p2, p3, Ivl::point(vc));
                let r = ivl_sub_vec(pa, pb);
                out = Some(match out {
                    None => r,
                    Some(prev) => hull_vec(prev, r),
                });
            }
        }
    }
    out.unwrap_or([Ivl::point(0.0); 3])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SubBoxKey {
    a0_bits: u64,
    a1_bits: u64,
    u0_bits: u64,
    u1_bits: u64,
    v0_bits: u64,
    v1_bits: u64,
    seq: u64,
}

impl Ord for SubBoxKey {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        let a0 = f64::from_bits(self.a0_bits).total_cmp(&f64::from_bits(other.a0_bits));
        if a0 != core::cmp::Ordering::Equal {
            return a0;
        }
        let a1 = f64::from_bits(self.a1_bits).total_cmp(&f64::from_bits(other.a1_bits));
        if a1 != core::cmp::Ordering::Equal {
            return a1;
        }
        (
            self.u0_bits,
            self.u1_bits,
            self.v0_bits,
            self.v1_bits,
            self.seq,
        )
            .cmp(&(
                other.u0_bits,
                other.u1_bits,
                other.v0_bits,
                other.v1_bits,
                other.seq,
            ))
    }
}

impl PartialOrd for SubBoxKey {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl SubBoxKey {
    fn new(a: [f64; 2], u: [f64; 2], v: [f64; 2], seq: u64) -> Self {
        Self {
            a0_bits: a[0].to_bits(),
            a1_bits: a[1].to_bits(),
            u0_bits: u[0].to_bits(),
            u1_bits: u[1].to_bits(),
            v0_bits: v[0].to_bits(),
            v1_bits: v[1].to_bits(),
            seq,
        }
    }

    fn a(self) -> [f64; 2] {
        [f64::from_bits(self.a0_bits), f64::from_bits(self.a1_bits)]
    }

    fn u(self) -> [f64; 2] {
        [f64::from_bits(self.u0_bits), f64::from_bits(self.u1_bits)]
    }

    fn v(self) -> [f64; 2] {
        [f64::from_bits(self.v0_bits), f64::from_bits(self.v1_bits)]
    }
}

fn box_is_clear(r: [Ivl; 3], d_min: f64) -> bool {
    let d_min_sq_hi = next_up_1ulp(d_min * d_min);
    let sx = r[0].sq_lo();
    let sy = r[1].sq_lo();
    let sz = r[2].sq_lo();
    let sum_xy = next_down_1ulp(sx + sy);
    let sum_xyz = next_down_1ulp(sum_xy + sz);
    sum_xyz > d_min_sq_hi
}

fn run_tight_inclusion<F>(
    d0: f64,
    d_min: f64,
    v_max: f64,
    config: CcdConfig,
    has_u: bool,
    has_v: bool,
    eval_box: F,
) -> CcdStepVerdict
where
    F: Fn([f64; 2], [f64; 2], [f64; 2]) -> [Ivl; 3],
{
    if v_max <= 0.0 {
        return CcdStepVerdict {
            alpha_max: 1.0,
            collided: false,
            initial_distance_m: d0,
            iterations: 1,
        };
    }

    // Triangle-inequality Lipschitz clearance guarantee on [0, alpha_lip].
    let alpha_lip = (0.45 * (d0 - d_min) / v_max).min(1.0);
    if alpha_lip >= 1.0 {
        return CcdStepVerdict {
            alpha_max: 1.0,
            collided: false,
            initial_distance_m: d0,
            iterations: 1,
        };
    }

    let mut queue = BTreeSet::new();
    let mut seq = 1_u64;
    let u_init = if has_u { [0.0, 1.0] } else { [0.0, 0.0] };
    let v_init = if has_v { [0.0, 1.0] } else { [0.0, 0.0] };
    queue.insert(SubBoxKey::new([alpha_lip, 1.0], u_init, v_init, 0));

    let mut iters = 0_u32;
    while let Some(cur) = queue.pop_first() {
        iters += 1;
        let a = cur.a();
        let u = cur.u();
        let v = cur.v();
        let r = eval_box(a, u, v);
        if box_is_clear(r, d_min) {
            continue;
        }

        let da = a[1] - a[0];
        let du = u[1] - u[0];
        let dv = v[1] - v[0];
        let a_done = da <= config.time_tol;
        let u_done = !has_u || du <= config.space_tol;
        let v_done = !has_v || dv <= config.space_tol;

        if (a_done && u_done && v_done) || iters >= config.max_iters {
            // Every remaining box in `queue` has `box.a[0] >= a[0]`, so
            // `[0, a[0])` is provably collision-free.
            let safe = (a[0] * (1.0 - 1e-6)).max(alpha_lip).min(1.0);
            return CcdStepVerdict {
                alpha_max: safe,
                collided: true,
                initial_distance_m: d0,
                iterations: iters,
            };
        }

        // Choose widest normalized dimension to bisect.
        let na = da / config.time_tol;
        let nu = if has_u { du / config.space_tol } else { 0.0 };
        let nv = if has_v { dv / config.space_tol } else { 0.0 };

        if na >= nu && na >= nv {
            let am = 0.5 * (a[0] + a[1]);
            if am > a[0] && am < a[1] {
                queue.insert(SubBoxKey::new([a[0], am], u, v, seq));
                seq += 1;
                queue.insert(SubBoxKey::new([am, a[1]], u, v, seq));
                seq += 1;
                continue;
            }
        }
        if has_u && nu >= nv {
            let um = 0.5 * (u[0] + u[1]);
            if um > u[0] && um < u[1] {
                queue.insert(SubBoxKey::new(a, [u[0], um], v, seq));
                seq += 1;
                queue.insert(SubBoxKey::new(a, [um, u[1]], v, seq));
                seq += 1;
                continue;
            }
        }
        if has_v {
            let vm = 0.5 * (v[0] + v[1]);
            if vm > v[0] && vm < v[1] {
                queue.insert(SubBoxKey::new(a, u, [v[0], vm], seq));
                seq += 1;
                queue.insert(SubBoxKey::new(a, u, [vm, v[1]], seq));
                seq += 1;
                continue;
            }
        }

        // Sub-box reached IEEE-754 precision limit while remaining uncleared.
        let safe = (a[0] * (1.0 - 1e-6)).max(alpha_lip).min(1.0);
        return CcdStepVerdict {
            alpha_max: safe,
            collided: true,
            initial_distance_m: d0,
            iterations: iters,
        };
    }

    CcdStepVerdict {
        alpha_max: 1.0,
        collided: false,
        initial_distance_m: d0,
        iterations: iters,
    }
}

fn validate_ccd_inputs(
    start: &[[f64; 3]],
    end: &[[f64; 3]],
    d_min: f64,
    config: CcdConfig,
) -> Result<(), CodimContactError> {
    if !d_min.is_finite() || !config.time_tol.is_finite() || !config.space_tol.is_finite() {
        return Err(CodimContactError::NonFiniteInput {
            context: "ccd configuration",
        });
    }
    if d_min < 0.0 || config.time_tol <= 0.0 || config.space_tol <= 0.0 || config.max_iters == 0 {
        return Err(CodimContactError::InvalidParameter {
            detail: "CCD requires d_min >= 0, positive tolerances, and max_iters >= 1",
        });
    }
    for (&p0, &p1) in start.iter().zip(end.iter()) {
        if !all_finite3(p0) || !all_finite3(p1) {
            return Err(CodimContactError::NonFiniteInput {
                context: "CCD trajectory endpoint",
            });
        }
    }
    Ok(())
}
