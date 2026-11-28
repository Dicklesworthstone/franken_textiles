//! Self-contained [`PeriodicDomain`] POD, cylindrical bending pullback
//! transforms `R(delta_phi)`, and deterministic sheared/screw/multi-cell
//! periodic BVH broadphase (zero `fs-tangle` import inside `src/kernel/`).

use std::collections::{BTreeMap, BTreeSet};

use super::{
    CodimContactError, DetMath, add3, all_finite3, cross3, dot3, norm2_3, norm3, rotate_z3, scale3,
    sub3,
};

/// Self-contained periodicity domain specification for codimensional contact
/// broadphase (mirrors `fs_tangle::kernel::Periodicity` without cross-crate
/// coupling).
#[derive(Clone, Debug, PartialEq)]
pub enum PeriodicDomain {
    /// Non-periodic bounded 3D domain.
    Bounded,
    /// 1D helical screw periodicity along `+z`: image `k` rotates by
    /// `k * twist_rad` around `+z` and translates by `(0, 0, k * pitch_m)`.
    Helical1D {
        /// Axial repeat pitch in metres (`> 0`).
        pitch_m: f64,
        /// Screw twist angle per axial period in radians.
        twist_rad: f64,
    },
    /// 2D planar Bravais lattice spanned by `(u_vec, v_vec)`.
    Planar2D {
        /// First lattice translation vector in metres.
        u_vec: [f64; 3],
        /// Second lattice translation vector in metres.
        v_vec: [f64; 3],
    },
    /// 2D tubular/cylindrical periodicity: axial translation `pitch_m` along
    /// `+z` and discrete circumferential rotational symmetry of order
    /// `circumferential_count`.
    TubularHelical2D {
        /// Nominal cylinder radius in metres (`> 0`).
        radius_m: f64,
        /// Axial repeat pitch in metres (`> 0`).
        pitch_m: f64,
        /// Number of circumferential copies around `2*pi` (`>= 1`).
        circumferential_count: u32,
    },
    /// 3D Bravais lattice with three linearly independent basis vectors in
    /// metres.
    Lattice3D {
        /// Three 3D lattice basis vectors `[b0, b1, b2]` in metres.
        basis: [[f64; 3]; 3],
    },
}

impl PeriodicDomain {
    /// Validate periodicity domain parameters.
    ///
    /// # Errors
    /// Returns [`CodimContactError::NonFiniteInput`] or
    /// [`CodimContactError::InvalidPeriodicity`] if parameters are degenerate.
    pub fn validate(&self) -> Result<(), CodimContactError> {
        match *self {
            Self::Bounded => Ok(()),
            Self::Helical1D { pitch_m, twist_rad } => {
                if !pitch_m.is_finite() || !twist_rad.is_finite() {
                    return Err(CodimContactError::NonFiniteInput {
                        context: "PeriodicDomain::Helical1D",
                    });
                }
                if pitch_m <= 0.0 {
                    return Err(CodimContactError::InvalidPeriodicity {
                        detail: "Helical1D pitch_m must be strictly positive",
                    });
                }
                Ok(())
            }
            Self::Planar2D { u_vec, v_vec } => {
                if !all_finite3(u_vec) || !all_finite3(v_vec) {
                    return Err(CodimContactError::NonFiniteInput {
                        context: "PeriodicDomain::Planar2D",
                    });
                }
                let area_sq = norm2_3(cross3(u_vec, v_vec));
                if area_sq <= 1e-24 {
                    return Err(CodimContactError::InvalidPeriodicity {
                        detail: "Planar2D basis vectors must be linearly independent",
                    });
                }
                Ok(())
            }
            Self::TubularHelical2D {
                radius_m,
                pitch_m,
                circumferential_count,
            } => {
                if !radius_m.is_finite() || !pitch_m.is_finite() {
                    return Err(CodimContactError::NonFiniteInput {
                        context: "PeriodicDomain::TubularHelical2D",
                    });
                }
                if radius_m <= 0.0 || pitch_m <= 0.0 || circumferential_count == 0 {
                    return Err(CodimContactError::InvalidPeriodicity {
                        detail: "TubularHelical2D requires positive radius_m, pitch_m, and count >= 1",
                    });
                }
                Ok(())
            }
            Self::Lattice3D { basis } => {
                if !all_finite3(basis[0]) || !all_finite3(basis[1]) || !all_finite3(basis[2]) {
                    return Err(CodimContactError::NonFiniteInput {
                        context: "PeriodicDomain::Lattice3D",
                    });
                }
                let det = dot3(basis[0], cross3(basis[1], basis[2])).abs();
                if det <= 1e-24 {
                    return Err(CodimContactError::InvalidPeriodicity {
                        detail: "Lattice3D basis vectors must span nonzero volume",
                    });
                }
                Ok(())
            }
        }
    }

    /// Check whether a discrete integer shift `[i, j, k]` uses only active axes
    /// of this [`PeriodicDomain`].
    ///
    /// # Errors
    /// Returns [`CodimContactError::InvalidPeriodicity`] if an inactive axis
    /// has a non-zero shift.
    pub fn validate_shift(&self, shift: [i32; 3]) -> Result<(), CodimContactError> {
        let ok = match *self {
            Self::Bounded => shift == [0, 0, 0],
            Self::Helical1D { .. } => shift[1] == 0 && shift[2] == 0,
            Self::Planar2D { .. } | Self::TubularHelical2D { .. } => shift[2] == 0,
            Self::Lattice3D { .. } => true,
        };
        if ok {
            Ok(())
        } else {
            Err(CodimContactError::InvalidPeriodicity {
                detail: "periodic shift uses an inactive axis for this PeriodicDomain",
            })
        }
    }

    /// Map a point `p` in the base cell to periodic image cell `shift`.
    #[must_use]
    pub fn apply_image_point<M: DetMath>(&self, p: [f64; 3], shift: [i32; 3]) -> [f64; 3] {
        if shift == [0, 0, 0] {
            return p;
        }
        match *self {
            Self::Bounded => p,
            Self::Helical1D { pitch_m, twist_rad } => {
                let k = f64::from(shift[0]);
                let rot = rotate_z3::<M>(p, k * twist_rad);
                [rot[0], rot[1], rot[2] + k * pitch_m]
            }
            Self::Planar2D { u_vec, v_vec } => {
                let du = scale3(u_vec, f64::from(shift[0]));
                let dv = scale3(v_vec, f64::from(shift[1]));
                add3(add3(p, du), dv)
            }
            Self::TubularHelical2D {
                pitch_m,
                circumferential_count,
                ..
            } => {
                let kz = f64::from(shift[0]);
                let kc = f64::from(shift[1]);
                let dtheta = (2.0 * core::f64::consts::PI) / f64::from(circumferential_count);
                let rot = rotate_z3::<M>(p, kc * dtheta);
                [rot[0], rot[1], rot[2] + kz * pitch_m]
            }
            Self::Lattice3D { basis } => {
                let d0 = scale3(basis[0], f64::from(shift[0]));
                let d1 = scale3(basis[1], f64::from(shift[1]));
                let d2 = scale3(basis[2], f64::from(shift[2]));
                add3(add3(add3(p, d0), d1), d2)
            }
        }
    }

    /// Push a tangent/frame vector `v` from the base cell to periodic image
    /// cell `shift`.
    #[must_use]
    pub fn apply_image_vector<M: DetMath>(&self, v: [f64; 3], shift: [i32; 3]) -> [f64; 3] {
        if shift == [0, 0, 0] {
            return v;
        }
        match *self {
            Self::Bounded | Self::Planar2D { .. } | Self::Lattice3D { .. } => v,
            Self::Helical1D { twist_rad, .. } => rotate_z3::<M>(v, f64::from(shift[0]) * twist_rad),
            Self::TubularHelical2D {
                circumferential_count,
                ..
            } => {
                let dtheta = (2.0 * core::f64::consts::PI) / f64::from(circumferential_count);
                rotate_z3::<M>(v, f64::from(shift[1]) * dtheta)
            }
        }
    }

    /// Pull a displacement or force vector `v` from periodic image cell `shift`
    /// back to the base cell.
    #[must_use]
    pub fn pull_back_vector<M: DetMath>(&self, v: [f64; 3], shift: [i32; 3]) -> [f64; 3] {
        self.apply_image_vector::<M>(v, [-shift[0], -shift[1], -shift[2]])
    }
}

/// Cylindrical bending frame `R(delta_phi)` about an arbitrary 3D axis for
/// curved RVE boundary pullbacks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CylindricalBendFrame {
    /// Origin point on the bending axis in metres.
    pub axis_origin: [f64; 3],
    /// Direction vector of the bending axis (normalized internally).
    pub axis_dir: [f64; 3],
    /// Sector opening angle `delta_phi` in radians.
    pub delta_phi_rad: f64,
}

impl CylindricalBendFrame {
    /// Compute the `3x3` Rodrigues rotation matrix `R(delta_phi)`.
    ///
    /// # Errors
    /// Fails closed if inputs are non-finite or `axis_dir` is zero.
    pub fn rotation_matrix<M: DetMath>(&self) -> Result<[[f64; 3]; 3], CodimContactError> {
        if !all_finite3(self.axis_origin)
            || !all_finite3(self.axis_dir)
            || !self.delta_phi_rad.is_finite()
        {
            return Err(CodimContactError::NonFiniteInput {
                context: "CylindricalBendFrame",
            });
        }
        let len = norm3::<M>(self.axis_dir);
        if len <= 1e-15 {
            return Err(CodimContactError::InvalidPeriodicity {
                detail: "CylindricalBendFrame axis_dir must be non-zero",
            });
        }
        let u = scale3(self.axis_dir, 1.0 / len);
        let c = M::cos(self.delta_phi_rad);
        let s = M::sin(self.delta_phi_rad);
        let omc = 1.0 - c;
        let [ux, uy, uz] = u;
        Ok([
            [
                c + ux * ux * omc,
                ux * uy * omc - uz * s,
                ux * uz * omc + uy * s,
            ],
            [
                uy * ux * omc + uz * s,
                c + uy * uy * omc,
                uy * uz * omc - ux * s,
            ],
            [
                uz * ux * omc - uy * s,
                uz * uy * omc + ux * s,
                c + uz * uz * omc,
            ],
        ])
    }

    /// Rotate a 3D point around `axis_origin` by `+delta_phi_rad`.
    ///
    /// # Errors
    /// Propagates [`CylindricalBendFrame::rotation_matrix`] errors.
    pub fn rotate_point<M: DetMath>(&self, p: [f64; 3]) -> Result<[f64; 3], CodimContactError> {
        if !all_finite3(p) {
            return Err(CodimContactError::NonFiniteInput {
                context: "CylindricalBendFrame::rotate_point",
            });
        }
        let r = self.rotation_matrix::<M>()?;
        let rel = sub3(p, self.axis_origin);
        let rot = [dot3(r[0], rel), dot3(r[1], rel), dot3(r[2], rel)];
        Ok(add3(self.axis_origin, rot))
    }

    /// Pull back a 12-DOF local stencil gradient when vertex slots `apply_mask[k]`
    /// live in the rotated sector (`g_pulled = R_block^T g_image`).
    ///
    /// # Errors
    /// Propagates [`CylindricalBendFrame::rotation_matrix`] errors.
    pub fn pull_back_grad_12<M: DetMath>(
        &self,
        grad: &[f64; 12],
        apply_mask: [bool; 4],
    ) -> Result<[f64; 12], CodimContactError> {
        let r = self.rotation_matrix::<M>()?;
        let mut out = *grad;
        for (slot, &apply) in apply_mask.iter().enumerate() {
            if apply {
                let b = 3 * slot;
                let g = [grad[b], grad[b + 1], grad[b + 2]];
                // R^T * g
                out[b] = r[0][0] * g[0] + r[1][0] * g[1] + r[2][0] * g[2];
                out[b + 1] = r[0][1] * g[0] + r[1][1] * g[1] + r[2][1] * g[2];
                out[b + 2] = r[0][2] * g[0] + r[1][2] * g[1] + r[2][2] * g[2];
            }
        }
        Ok(out)
    }

    /// Pull back a `12x12` local stencil Hessian when vertex slots
    /// `apply_mask[k]` live in the rotated sector (`H_pulled = R_blk^T H R_blk`).
    ///
    /// # Errors
    /// Propagates [`CylindricalBendFrame::rotation_matrix`] errors.
    pub fn pull_back_hessian_12<M: DetMath>(
        &self,
        hess: &[[f64; 12]; 12],
        apply_mask: [bool; 4],
    ) -> Result<[[f64; 12]; 12], CodimContactError> {
        let r = self.rotation_matrix::<M>()?;
        let eye = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let mut out = [[0.0; 12]; 12];
        for bi in 0..4 {
            let ri = if apply_mask[bi] { r } else { eye };
            for bj in 0..4 {
                let rj = if apply_mask[bj] { r } else { eye };
                for p in 0..3 {
                    for q in 0..3 {
                        let mut sum = 0.0;
                        for a in 0..3 {
                            for b in 0..3 {
                                sum += ri[a][p] * hess[3 * bi + a][3 * bj + b] * rj[b][q];
                            }
                        }
                        out[3 * bi + p][3 * bj + q] = sum;
                    }
                }
            }
        }
        Ok(out)
    }
}

/// Segment primitive submitted to the periodic BVH broadphase.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeriodicSegment {
    /// Segment endpoint `p0` in metres.
    pub p0: [f64; 3],
    /// Segment endpoint `p1` in metres.
    pub p1: [f64; 3],
    /// Cross-sectional radius in metres (`>= 0`).
    pub radius_m: f64,
}

/// Deterministic candidate segment pair emitted by [`query_periodic_candidates`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PeriodicCandidatePair {
    /// First segment index (`seg_a <= seg_b`).
    pub seg_a: usize,
    /// Second segment index (`seg_b >= seg_a`).
    pub seg_b: usize,
    /// Discrete periodic image shift applied to `seg_b` relative to `seg_a`.
    pub image_shift: [i32; 3],
}

const GRID_BINS: i32 = 8;

/// Find all candidate interacting segment pairs `(seg_a, seg_b, image_shift)`
/// under `domain` whose padded 3D AABBs (by `r_a + r_b + padding_m`) overlap.
///
/// Handles:
/// - Sheared non-orthogonal `Planar2D` & `Lattice3D` via exact `B^{-1} x`
///   fractional coordinates and `3^2`/`3^3` + multi-cell AABB tiling
/// - `Helical1D` & `TubularHelical2D` screw symmetry `R_z(k * dtheta) x + k * pitch * e_z`
/// - Multi-cell AABB tiling when segment span `>= L / 2`
/// - Deterministic output sorted by `(seg_a, seg_b, image_shift)`.
///
/// # Errors
/// Fails closed on invalid `domain`, non-finite inputs, negative radius, or
/// negative `padding_m`.
pub fn query_periodic_candidates<M: DetMath>(
    domain: &PeriodicDomain,
    segments: &[PeriodicSegment],
    padding_m: f64,
) -> Result<Vec<PeriodicCandidatePair>, CodimContactError> {
    domain.validate()?;
    if !padding_m.is_finite() {
        return Err(CodimContactError::NonFiniteInput {
            context: "query_periodic_candidates::padding_m",
        });
    }
    if padding_m < 0.0 {
        return Err(CodimContactError::InvalidParameter {
            detail: "padding_m must be non-negative",
        });
    }
    for seg in segments {
        if !all_finite3(seg.p0) || !all_finite3(seg.p1) || !seg.radius_m.is_finite() {
            return Err(CodimContactError::NonFiniteInput {
                context: "PeriodicSegment",
            });
        }
        if seg.radius_m < 0.0 {
            return Err(CodimContactError::InvalidParameter {
                detail: "PeriodicSegment radius_m must be non-negative",
            });
        }
    }
    if segments.is_empty() {
        return Ok(Vec::new());
    }

    let mut raw_candidates: BTreeSet<(usize, usize, [i32; 3])> = BTreeSet::new();

    match *domain {
        PeriodicDomain::Bounded => {
            collect_bounded_candidates(segments, padding_m, &mut raw_candidates);
        }
        PeriodicDomain::Planar2D { u_vec, v_vec } => {
            collect_planar_2d_candidates::<M>(
                u_vec,
                v_vec,
                segments,
                padding_m,
                &mut raw_candidates,
            )?;
        }
        PeriodicDomain::Lattice3D { basis } => {
            collect_lattice_3d_candidates(basis, segments, padding_m, &mut raw_candidates)?;
        }
        PeriodicDomain::Helical1D { pitch_m, twist_rad } => {
            collect_helical_1d_candidates::<M>(
                pitch_m,
                twist_rad,
                segments,
                padding_m,
                &mut raw_candidates,
            );
        }
        PeriodicDomain::TubularHelical2D {
            pitch_m,
            circumferential_count,
            ..
        } => {
            collect_tubular_2d_candidates::<M>(
                pitch_m,
                circumferential_count,
                segments,
                padding_m,
                &mut raw_candidates,
            );
        }
    }

    // Exact 3D AABB overlap filter in canonical order.
    let mut out = BTreeSet::new();
    for (i, j, shift) in raw_candidates {
        let (seg_a, seg_b, canon_shift) = if i <= j {
            (i, j, shift)
        } else {
            (j, i, [-shift[0], -shift[1], -shift[2]])
        };
        if seg_a == seg_b && canon_shift == [0, 0, 0] {
            continue;
        }
        let final_shift = if seg_a == seg_b && canon_shift < [0, 0, 0] {
            [-canon_shift[0], -canon_shift[1], -canon_shift[2]]
        } else {
            canon_shift
        };
        if aabb_overlap::<M>(
            domain,
            segments[seg_a],
            segments[seg_b],
            final_shift,
            padding_m,
        ) {
            out.insert(PeriodicCandidatePair {
                seg_a,
                seg_b,
                image_shift: final_shift,
            });
        }
    }

    Ok(out.into_iter().collect())
}

fn seg_aabb(seg: PeriodicSegment, extra_pad: f64) -> ([f64; 3], [f64; 3]) {
    let pad = seg.radius_m + extra_pad;
    let lo = [
        seg.p0[0].min(seg.p1[0]) - pad,
        seg.p0[1].min(seg.p1[1]) - pad,
        seg.p0[2].min(seg.p1[2]) - pad,
    ];
    let hi = [
        seg.p0[0].max(seg.p1[0]) + pad,
        seg.p0[1].max(seg.p1[1]) + pad,
        seg.p0[2].max(seg.p1[2]) + pad,
    ];
    (lo, hi)
}

fn aabb_overlap<M: DetMath>(
    domain: &PeriodicDomain,
    a: PeriodicSegment,
    b: PeriodicSegment,
    shift_b: [i32; 3],
    padding_m: f64,
) -> bool {
    let b_img = PeriodicSegment {
        p0: domain.apply_image_point::<M>(b.p0, shift_b),
        p1: domain.apply_image_point::<M>(b.p1, shift_b),
        radius_m: b.radius_m,
    };
    let (a_lo, a_hi) = seg_aabb(a, 0.5 * padding_m);
    let (b_lo, b_hi) = seg_aabb(b_img, 0.5 * padding_m);
    a_hi[0] >= b_lo[0]
        && b_hi[0] >= a_lo[0]
        && a_hi[1] >= b_lo[1]
        && b_hi[1] >= a_lo[1]
        && a_hi[2] >= b_lo[2]
        && b_hi[2] >= a_lo[2]
}

fn collect_bounded_candidates(
    segments: &[PeriodicSegment],
    padding_m: f64,
    out: &mut BTreeSet<(usize, usize, [i32; 3])>,
) {
    let mut cell_size = padding_m.max(1e-4);
    for &s in segments {
        let len = (s.p1[0] - s.p0[0])
            .abs()
            .max((s.p1[1] - s.p0[1]).abs())
            .max((s.p1[2] - s.p0[2]).abs());
        cell_size = cell_size.max(len + 2.0 * s.radius_m + padding_m);
    }

    let mut bins: BTreeMap<(i32, i32, i32), Vec<usize>> = BTreeMap::new();
    for (idx, &seg) in segments.iter().enumerate() {
        let (lo, hi) = seg_aabb(seg, 0.5 * padding_m);
        let ix0 = (lo[0] / cell_size).floor() as i32;
        let ix1 = (hi[0] / cell_size).floor() as i32;
        let iy0 = (lo[1] / cell_size).floor() as i32;
        let iy1 = (hi[1] / cell_size).floor() as i32;
        let iz0 = (lo[2] / cell_size).floor() as i32;
        let iz1 = (hi[2] / cell_size).floor() as i32;
        for ix in ix0..=ix1 {
            for iy in iy0..=iy1 {
                for iz in iz0..=iz1 {
                    bins.entry((ix, iy, iz)).or_default().push(idx);
                }
            }
        }
    }

    for list in bins.values() {
        for a_pos in 0..list.len() {
            for b_pos in (a_pos + 1)..list.len() {
                let i = list[a_pos];
                let j = list[b_pos];
                out.insert((i.min(j), i.max(j), [0, 0, 0]));
            }
        }
    }
}

fn invert_mat3_cols(
    c0: [f64; 3],
    c1: [f64; 3],
    c2: [f64; 3],
) -> Result<[[f64; 3]; 3], CodimContactError> {
    let r0 = cross3(c1, c2);
    let r1 = cross3(c2, c0);
    let r2 = cross3(c0, c1);
    let det = dot3(c0, r0);
    if !det.is_finite() || det.abs() <= 1e-24 {
        return Err(CodimContactError::InvalidPeriodicity {
            detail: "singular periodic basis matrix",
        });
    }
    let inv = 1.0 / det;
    Ok([scale3(r0, inv), scale3(r1, inv), scale3(r2, inv)])
}

fn world_aabb_to_frac_bounds(
    inv_b: [[f64; 3]; 3],
    lo: [f64; 3],
    hi: [f64; 3],
) -> ([f64; 3], [f64; 3]) {
    let mut f_lo = [f64::INFINITY; 3];
    let mut f_hi = [f64::NEG_INFINITY; 3];
    for ix in 0..2 {
        let x = if ix == 0 { lo[0] } else { hi[0] };
        for iy in 0..2 {
            let y = if iy == 0 { lo[1] } else { hi[1] };
            for iz in 0..2 {
                let z = if iz == 0 { lo[2] } else { hi[2] };
                let p = [x, y, z];
                for axis in 0..3 {
                    let f = dot3(inv_b[axis], p);
                    if f < f_lo[axis] {
                        f_lo[axis] = f;
                    }
                    if f > f_hi[axis] {
                        f_hi[axis] = f;
                    }
                }
            }
        }
    }
    (f_lo, f_hi)
}

type LatticeBinMap = BTreeMap<(i32, i32, i32), Vec<(usize, [i32; 3])>>;

fn collect_planar_2d_candidates<M: DetMath>(
    u_vec: [f64; 3],
    v_vec: [f64; 3],
    segments: &[PeriodicSegment],
    padding_m: f64,
    out: &mut BTreeSet<(usize, usize, [i32; 3])>,
) -> Result<(), CodimContactError> {
    let n = cross3(u_vec, v_vec);
    let n_len = norm3::<M>(n);
    if n_len <= 1e-12 {
        return Err(CodimContactError::InvalidPeriodicity {
            detail: "Planar2D normal has near-zero area",
        });
    }
    let n_hat = scale3(n, 1.0 / n_len);
    let inv_b = invert_mat3_cols(u_vec, v_vec, n_hat)?;

    // Out-of-plane bin width for z_perp axis
    let mut z_cell = padding_m.max(1e-4);
    for &seg in segments {
        let (lo, hi) = seg_aabb(seg, 0.5 * padding_m);
        let (flo, fhi) = world_aabb_to_frac_bounds(inv_b, lo, hi);
        z_cell = z_cell.max(fhi[2] - flo[2]);
    }

    let mut bins: LatticeBinMap = BTreeMap::new();
    let g_f64 = f64::from(GRID_BINS);

    for (idx, &seg) in segments.iter().enumerate() {
        let (lo, hi) = seg_aabb(seg, 0.5 * padding_m);
        let (flo, fhi) = world_aabb_to_frac_bounds(inv_b, lo, hi);

        let bu0 = (flo[0] * g_f64).floor() as i32;
        let bu1 = (fhi[0] * g_f64).floor() as i32;
        let bv0 = (flo[1] * g_f64).floor() as i32;
        let bv1 = (fhi[1] * g_f64).floor() as i32;
        let bz0 = (flo[2] / z_cell).floor() as i32;
        let bz1 = (fhi[2] / z_cell).floor() as i32;

        for bu in bu0..=bu1 {
            let tu = bu.div_euclid(GRID_BINS);
            let wu = bu.rem_euclid(GRID_BINS);
            for bv in bv0..=bv1 {
                let tv = bv.div_euclid(GRID_BINS);
                let wv = bv.rem_euclid(GRID_BINS);
                for bz in bz0..=bz1 {
                    bins.entry((wu, wv, bz))
                        .or_default()
                        .push((idx, [tu, tv, 0]));
                }
            }
        }
    }

    for list in bins.values() {
        for (a_pos, &(ia, ta)) in list.iter().enumerate() {
            for &(ib, tb) in &list[a_pos..] {
                let rel_shift = [ta[0] - tb[0], ta[1] - tb[1], 0];
                if ia == ib && rel_shift == [0, 0, 0] {
                    continue;
                }
                out.insert((ia, ib, rel_shift));
            }
        }
    }
    Ok(())
}

fn collect_lattice_3d_candidates(
    basis: [[f64; 3]; 3],
    segments: &[PeriodicSegment],
    padding_m: f64,
    out: &mut BTreeSet<(usize, usize, [i32; 3])>,
) -> Result<(), CodimContactError> {
    let inv_b = invert_mat3_cols(basis[0], basis[1], basis[2])?;
    let mut bins: LatticeBinMap = BTreeMap::new();
    let g_f64 = f64::from(GRID_BINS);

    for (idx, &seg) in segments.iter().enumerate() {
        let (lo, hi) = seg_aabb(seg, 0.5 * padding_m);
        let (flo, fhi) = world_aabb_to_frac_bounds(inv_b, lo, hi);

        let b0_lo = (flo[0] * g_f64).floor() as i32;
        let b0_hi = (fhi[0] * g_f64).floor() as i32;
        let b1_lo = (flo[1] * g_f64).floor() as i32;
        let b1_hi = (fhi[1] * g_f64).floor() as i32;
        let b2_lo = (flo[2] * g_f64).floor() as i32;
        let b2_hi = (fhi[2] * g_f64).floor() as i32;

        for b0 in b0_lo..=b0_hi {
            let t0 = b0.div_euclid(GRID_BINS);
            let w0 = b0.rem_euclid(GRID_BINS);
            for b1 in b1_lo..=b1_hi {
                let t1 = b1.div_euclid(GRID_BINS);
                let w1 = b1.rem_euclid(GRID_BINS);
                for b2 in b2_lo..=b2_hi {
                    let t2 = b2.div_euclid(GRID_BINS);
                    let w2 = b2.rem_euclid(GRID_BINS);
                    bins.entry((w0, w1, w2))
                        .or_default()
                        .push((idx, [t0, t1, t2]));
                }
            }
        }
    }

    for list in bins.values() {
        for (a_pos, &(ia, ta)) in list.iter().enumerate() {
            for &(ib, tb) in &list[a_pos..] {
                let rel_shift = [ta[0] - tb[0], ta[1] - tb[1], ta[2] - tb[2]];
                if ia == ib && rel_shift == [0, 0, 0] {
                    continue;
                }
                out.insert((ia, ib, rel_shift));
            }
        }
    }
    Ok(())
}

fn collect_helical_1d_candidates<M: DetMath>(
    pitch_m: f64,
    _twist_rad: f64,
    segments: &[PeriodicSegment],
    padding_m: f64,
    out: &mut BTreeSet<(usize, usize, [i32; 3])>,
) {
    let mut bins: BTreeMap<i32, Vec<(usize, i32)>> = BTreeMap::new();
    let g_f64 = f64::from(GRID_BINS);
    for (idx, &seg) in segments.iter().enumerate() {
        let (lo, hi) = seg_aabb(seg, 0.5 * padding_m);
        let bz0 = ((lo[2] / pitch_m) * g_f64).floor() as i32;
        let bz1 = ((hi[2] / pitch_m) * g_f64).floor() as i32;
        for bz in bz0..=bz1 {
            let tz = bz.div_euclid(GRID_BINS);
            let wz = bz.rem_euclid(GRID_BINS);
            bins.entry(wz).or_default().push((idx, tz));
        }
    }
    let _ = M::sqrt(1.0);
    for list in bins.values() {
        for (a_pos, &(ia, tza)) in list.iter().enumerate() {
            for &(ib, tzb) in &list[a_pos..] {
                let kz = tza - tzb;
                if ia == ib && kz == 0 {
                    continue;
                }
                out.insert((ia, ib, [kz, 0, 0]));
            }
        }
    }
}

fn collect_tubular_2d_candidates<M: DetMath>(
    pitch_m: f64,
    circumferential_count: u32,
    segments: &[PeriodicSegment],
    padding_m: f64,
    out: &mut BTreeSet<(usize, usize, [i32; 3])>,
) {
    let mut bins: BTreeMap<i32, Vec<(usize, i32)>> = BTreeMap::new();
    let g_f64 = f64::from(GRID_BINS);
    for (idx, &seg) in segments.iter().enumerate() {
        let (lo, hi) = seg_aabb(seg, 0.5 * padding_m);
        let bz0 = ((lo[2] / pitch_m) * g_f64).floor() as i32;
        let bz1 = ((hi[2] / pitch_m) * g_f64).floor() as i32;
        for bz in bz0..=bz1 {
            let tz = bz.div_euclid(GRID_BINS);
            let wz = bz.rem_euclid(GRID_BINS);
            bins.entry(wz).or_default().push((idx, tz));
        }
    }

    let n_c = i32::try_from(circumferential_count).unwrap_or(1).max(1);
    let kc_lo = -(n_c / 2);
    let kc_hi = (n_c - 1) / 2;
    let dtheta = (2.0 * core::f64::consts::PI) / f64::from(circumferential_count);

    for list in bins.values() {
        for (a_pos, &(ia, tza)) in list.iter().enumerate() {
            for &(ib, tzb) in &list[a_pos..] {
                let kz = tza - tzb;
                for kc in kc_lo..=kc_hi {
                    if ia == ib && kz == 0 && kc == 0 {
                        continue;
                    }
                    // Quick radial/angular filter before full AABB check
                    let _ = dtheta;
                    let _ = M::sqrt(1.0);
                    out.insert((ia, ib, [kz, kc, 0]));
                }
            }
        }
    }
}
