//! Bone frames, forward kinematics and linear blend skinning (spec 05 §0, §4.3, §5).
//!
//! Operation orders follow the package (roma's Rodrigues formula, `get_bone_poses`,
//! `parallel_forward_kinematic`, `linear_blend_skinning`) so results agree to rounding.

use ftx_core::{Mat3, Vec3, det};
use serde::{Deserialize, Serialize};

/// A homogeneous transform `[M | t]` (bottom row `0 0 0 1`), column-vector convention:
/// `p' = M·p + t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    /// Linear part (a rotation for rigid transforms).
    pub m: Mat3,
    /// Translation.
    pub t: Vec3,
}

impl Affine {
    /// The identity.
    pub const IDENTITY: Self = Self {
        m: Mat3::IDENTITY,
        t: Vec3::ZERO,
    };

    /// From the top three rows of a 4×4 matrix.
    #[must_use]
    pub fn from_rows4(r: &[[f64; 4]]) -> Self {
        Self {
            m: Mat3::from_rows([
                [r[0][0], r[0][1], r[0][2]],
                [r[1][0], r[1][1], r[1][2]],
                [r[2][0], r[2][1], r[2][2]],
            ]),
            t: Vec3::new(r[0][3], r[1][3], r[2][3]),
        }
    }

    /// The 4×4 matrix.
    #[must_use]
    pub fn to_rows4(&self) -> [[f64; 4]; 4] {
        let m = &self.m.rows;
        [
            [m[0][0], m[0][1], m[0][2], self.t.x],
            [m[1][0], m[1][1], m[1][2], self.t.y],
            [m[2][0], m[2][1], m[2][2], self.t.z],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    /// Rotation by a rotation vector (axis × angle, radians) followed by a translation.
    #[must_use]
    pub fn from_rotvec(rotvec: Vec3, t: Vec3) -> Self {
        Self {
            m: rotvec_to_rotmat(rotvec),
            t,
        }
    }

    /// `self · o` (apply `o` first).
    #[must_use]
    pub fn compose(&self, o: &Self) -> Self {
        Self {
            m: self.m * o.m,
            t: self.m.mul_vec(o.t) + self.t,
        }
    }

    /// The rigid inverse `[Mᵀ | −Mᵀ·t]`, which the package uses for every rest pose (exact only when M
    /// is orthonormal; spec 05 §0 and §9.3).
    #[must_use]
    pub fn rigid_inverse(&self) -> Self {
        let mt = self.m.transpose();
        Self {
            m: mt,
            t: -mt.mul_vec(self.t),
        }
    }

    /// Applies the transform to a point.
    #[must_use]
    pub fn apply(&self, p: Vec3) -> Vec3 {
        self.m.mul_vec(p) + self.t
    }
}

/// Rotation matrix of a rotation vector, as roma's `rotvec_to_rotmat`: Rodrigues for θ ≥ 1e-6, the
/// first-order matrix `I + [r]×` below it.
#[must_use]
pub fn rotvec_to_rotmat(r: Vec3) -> Mat3 {
    const EPS: f64 = 1e-6;
    let theta = r.norm();
    if theta < EPS {
        return Mat3::from_rows([[1.0, -r.z, r.y], [r.z, 1.0, -r.x], [-r.y, r.x, 1.0]]);
    }
    let k = r / theta.max(EPS);
    let (s, c) = det::sin_cos(theta);
    let omc = 1.0 - c;
    let (xs, ys, zs) = (k.x * s, k.y * s, k.z * s);
    let (xyc, xzc, yzc) = (k.x * k.y * omc, k.x * k.z * omc, k.y * k.z * omc);
    let (xxc, yyc, zzc) = (k.x * k.x * omc, k.y * k.y * omc, k.z * k.z * omc);
    Mat3::from_rows([
        [1.0 - yyc - zzc, xyc - zs, xzc + ys],
        [xyc + zs, 1.0 - xxc - zzc, -xs + yzc],
        [xzc - ys, xs + yzc, 1.0 - xxc - yyc],
    ])
}

/// Rest pose of each bone (spec 05 §4.3): local +Y from head to tail by the minimal rotation from Ŷ,
/// then the stored roll; the root's rotation is the identity.
#[must_use]
pub fn rest_bone_frames(
    heads: &[Vec3],
    tails: &[Vec3],
    rolls: &[Mat3],
    y_axis: Vec3,
    degenerate: Mat3,
) -> Vec<Affine> {
    heads
        .iter()
        .zip(tails)
        .zip(rolls)
        .enumerate()
        .map(|(j, ((&h, &t), roll))| {
            if j == 0 {
                return Affine {
                    m: Mat3::IDENTITY,
                    t: h,
                };
            }
            let v = t - h;
            let y = v / v.norm();
            let cross = y.cross(y_axis);
            let dot = y.x * y_axis.x + y.y * y_axis.y + y.z * y_axis.z;
            let cross_norm = cross.norm();
            let angle = det::atan2(cross_norm, dot);
            let axis = cross / cross_norm;
            let valid = ((axis.x * axis.x + axis.y * axis.y + axis.z * axis.z) - 1.0).abs() < 0.1;
            let r0 = if valid {
                rotvec_to_rotmat(axis * -angle)
            } else {
                degenerate
            };
            Affine {
                m: r0 * *roll,
                t: h,
            }
        })
        .collect()
}

/// Forward kinematics under the `local-bone` parameterization (spec 05 §5.2). Returns the world pose
/// of each bone and its skinning transform.
///
/// # Panics
/// If the slices differ in length or a parent does not precede its child.
#[must_use]
pub fn forward_kinematics(
    rest: &[Affine],
    deltas: &[Affine],
    parents: &[i64],
) -> (Vec<Affine>, Vec<Affine>) {
    assert!(rest.len() == deltas.len() && rest.len() == parents.len());
    let inv: Vec<Affine> = rest.iter().map(Affine::rigid_inverse).collect();
    let mut poses = Vec::with_capacity(rest.len());
    let mut transforms: Vec<Affine> = Vec::with_capacity(rest.len());
    for j in 0..rest.len() {
        let m = rest[j].compose(&deltas[j]);
        let pose = if parents[j] < 0 {
            inv[0].compose(&m)
        } else {
            let p = usize::try_from(parents[j]).expect("bone parent index out of range");
            assert!(p < j, "bone {j}: parent {p} must precede it");
            transforms[p].compose(&m)
        };
        transforms.push(pose.compose(&inv[j]));
        poses.push(pose);
    }
    (poses, transforms)
}

/// Skinning operator for posing a rest mesh from per-bone transforms (plan §9.2).
///
/// [`SkinningMode::LinearBlend`] reproduces upstream Anny bit-for-bit (`spec 05 §5.3`).
/// [`SkinningMode::DualQuaternion`] blends rigid bone transforms in dual-quaternion space (Kavan
/// et al., 2008 DLB), avoiding linear-blend candy-wrapper twist collapse and reducing joint-hinge
/// volume loss on deep seated/squat bends; it is opt-in and never replaces LBS Anny parity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkinningMode {
    /// Upstream Anny linear blend skinning (default).
    #[default]
    #[serde(rename = "lbs")]
    LinearBlend,
    /// Kavan et al. (2008) Dual-Quaternion Linear Blending (DLB) with largest-weight pivot alignment.
    #[serde(rename = "dqs")]
    DualQuaternion,
}

impl SkinningMode {
    /// Returns `true` for the default linear blend skinning mode.
    #[must_use]
    pub const fn is_lbs(self) -> bool {
        matches!(self, Self::LinearBlend)
    }

    /// Stable CLI/report identifier (`"lbs"` or `"dqs"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LinearBlend => "lbs",
            Self::DualQuaternion => "dqs",
        }
    }

    /// Parses `"lbs"` or `"dqs"`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "lbs" => Some(Self::LinearBlend),
            "dqs" => Some(Self::DualQuaternion),
            _ => None,
        }
    }
}

/// Converts a right-handed orthonormal rotation matrix (`SO(3)`) into a unit quaternion
/// `[w, x, y, z]` via Shepperd's branch-stable method. Non-finite, zero, singular, reflected
/// (`det <= 0`), or non-rigid scaled/sheared matrices return NaNs rather than silently fabricating
/// a bogus rotation.
fn rotmat_to_unit_quaternion(m: &Mat3) -> [f64; 4] {
    let r = &m.rows;
    if r.iter().flatten().any(|v| !v.is_finite()) {
        return [f64::NAN; 4];
    }
    let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
        - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
        + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
    if !(det.is_finite() && det > 0.5) {
        return [f64::NAN; 4];
    }
    for i in 0..3 {
        for j in i..3 {
            let dot = r[0][i] * r[0][j] + r[1][i] * r[1][j] + r[2][i] * r[2][j];
            let target = if i == j { 1.0 } else { 0.0 };
            if !dot.is_finite() || (dot - target).abs() > 1e-3 {
                return [f64::NAN; 4];
            }
        }
    }
    let trace = r[0][0] + r[1][1] + r[2][2];
    let q = if trace > 0.0 {
        let diag = trace + 1.0;
        if !(diag > 0.0 && diag.is_finite()) {
            return [f64::NAN; 4];
        }
        let s = 2.0 * diag.sqrt();
        [
            0.25 * s,
            (r[2][1] - r[1][2]) / s,
            (r[0][2] - r[2][0]) / s,
            (r[1][0] - r[0][1]) / s,
        ]
    } else if r[0][0] >= r[1][1] && r[0][0] >= r[2][2] {
        let diag = 1.0 + r[0][0] - r[1][1] - r[2][2];
        if !(diag > 0.0 && diag.is_finite()) {
            return [f64::NAN; 4];
        }
        let s = 2.0 * diag.sqrt();
        [
            (r[2][1] - r[1][2]) / s,
            0.25 * s,
            (r[0][1] + r[1][0]) / s,
            (r[0][2] + r[2][0]) / s,
        ]
    } else if r[1][1] >= r[2][2] {
        let diag = 1.0 + r[1][1] - r[0][0] - r[2][2];
        if !(diag > 0.0 && diag.is_finite()) {
            return [f64::NAN; 4];
        }
        let s = 2.0 * diag.sqrt();
        [
            (r[0][2] - r[2][0]) / s,
            (r[0][1] + r[1][0]) / s,
            0.25 * s,
            (r[1][2] + r[2][1]) / s,
        ]
    } else {
        let diag = 1.0 + r[2][2] - r[0][0] - r[1][1];
        if !(diag > 0.0 && diag.is_finite()) {
            return [f64::NAN; 4];
        }
        let s = 2.0 * diag.sqrt();
        [
            (r[1][0] - r[0][1]) / s,
            (r[0][2] + r[2][0]) / s,
            (r[1][2] + r[2][1]) / s,
            0.25 * s,
        ]
    };
    let norm = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if norm > 0.0 && norm.is_finite() {
        [q[0] / norm, q[1] / norm, q[2] / norm, q[3] / norm]
    } else {
        [f64::NAN; 4]
    }
}

/// Converts a rigid transform `(R, t)` into a unit dual quaternion `(q_r, q_d)` where
/// `q_d = 0.5 * (0, t) * q_r`.
fn affine_to_dual_quaternion(a: &Affine) -> ([f64; 4], [f64; 4]) {
    if !a.t.is_finite() {
        return ([f64::NAN; 4], [f64::NAN; 4]);
    }
    let qr = rotmat_to_unit_quaternion(&a.m);
    if qr.iter().any(|v| !v.is_finite()) {
        return ([f64::NAN; 4], [f64::NAN; 4]);
    }
    let (w, x, y, z) = (qr[0], qr[1], qr[2], qr[3]);
    let t = a.t;
    let qd = [
        -0.5 * (t.x * x + t.y * y + t.z * z),
        0.5 * (t.x * w + t.y * z - t.z * y),
        0.5 * (-t.x * z + t.y * w + t.z * x),
        0.5 * (t.x * y - t.y * x + t.z * w),
    ];
    (qr, qd)
}

/// Dual-quaternion linear blend skinning (Kavan et al., 2008; plan §9.2): converts each rigid bone
/// skinning transform to a unit dual quaternion, aligns each vertex's bone influences to the
/// hemisphere of its highest-weight bone (fixed slot tie-break for determinism), blends and
/// normalizes the dual quaternion, then applies the resulting rigid screw motion to `rest[i]`.
/// `indices` and `weights` are V × K row-major.
///
/// # Panics
/// If `indices.len() != rest.len() * k` or `weights.len() != rest.len() * k`.
#[must_use]
pub fn dual_quaternion_skinning(
    rest: &[Vec3],
    indices: &[u32],
    weights: &[f64],
    k: usize,
    transforms: &[Affine],
) -> Vec<Vec3> {
    assert_eq!(indices.len(), rest.len() * k);
    assert_eq!(weights.len(), rest.len() * k);
    if k == 0 {
        return rest.to_vec();
    }
    let dqs: Vec<([f64; 4], [f64; 4])> = transforms.iter().map(affine_to_dual_quaternion).collect();
    let nan = Vec3::new(f64::NAN, f64::NAN, f64::NAN);
    rest.iter()
        .enumerate()
        .map(|(i, &p)| {
            if !p.is_finite() {
                return nan;
            }
            let base = i * k;
            // Deterministic pivot: largest-magnitude active bone weight (earliest slot on tie).
            let mut pivot_slot: Option<usize> = None;
            let mut max_abs_w = 0.0;
            for s in 0..k {
                let w = weights[base + s];
                if !w.is_finite() {
                    return nan;
                }
                let abs_w = w.abs();
                if abs_w > max_abs_w {
                    max_abs_w = abs_w;
                    pivot_slot = Some(s);
                }
            }
            let Some(pivot) = pivot_slot else {
                return p;
            };
            let pivot_r = dqs[indices[base + pivot] as usize].0;
            if pivot_r.iter().any(|v| !v.is_finite()) {
                return nan;
            }
            let mut br = [0.0; 4];
            let mut bd = [0.0; 4];
            for s in 0..k {
                let w = weights[base + s];
                if w == 0.0 {
                    continue;
                }
                let (qr, qd) = dqs[indices[base + s] as usize];
                if qr.iter().chain(&qd).any(|v| !v.is_finite()) {
                    return nan;
                }
                let dot = pivot_r[0] * qr[0]
                    + pivot_r[1] * qr[1]
                    + pivot_r[2] * qr[2]
                    + pivot_r[3] * qr[3];
                let sw = if dot < 0.0 { -w } else { w };
                for c in 0..4 {
                    br[c] += sw * qr[c];
                    bd[c] += sw * qd[c];
                }
            }
            let norm = (br[0] * br[0] + br[1] * br[1] + br[2] * br[2] + br[3] * br[3]).sqrt();
            if !(norm > 0.0 && norm.is_finite()) {
                return nan;
            }
            let inv = 1.0 / norm;
            let qr = [br[0] * inv, br[1] * inv, br[2] * inv, br[3] * inv];
            let mut qd = [bd[0] * inv, bd[1] * inv, bd[2] * inv, bd[3] * inv];
            // Project dual part orthogonal to real part for exact unit dual quaternion.
            let proj = qr[0] * qd[0] + qr[1] * qd[1] + qr[2] * qd[2] + qr[3] * qd[3];
            for c in 0..4 {
                qd[c] -= proj * qr[c];
            }
            let qv = Vec3::new(qr[1], qr[2], qr[3]);
            let dv = Vec3::new(qd[1], qd[2], qd[3]);
            let rot = p + qv.cross(qv.cross(p) + p * qr[0]) * 2.0;
            let trans = (dv * qr[0] - qv * qd[0] + qv.cross(dv)) * 2.0;
            rot + trans
        })
        .collect()
}

/// Linear blend skinning (spec 05 §5.3): transforms blended by weight first, then applied.
/// `indices` and `weights` are V × K row-major.
#[must_use]
pub fn linear_blend_skinning(
    rest: &[Vec3],
    indices: &[u32],
    weights: &[f64],
    k: usize,
    transforms: &[Affine],
) -> Vec<Vec3> {
    rest.iter()
        .enumerate()
        .map(|(i, &x)| {
            let mut m = [[0.0; 3]; 3];
            let mut t = [0.0; 3];
            for s in 0..k {
                let w = weights[i * k + s];
                let a = &transforms[indices[i * k + s] as usize];
                for (r, row) in m.iter_mut().enumerate() {
                    for (c, cell) in row.iter_mut().enumerate() {
                        *cell += w * a.m.rows[r][c];
                    }
                }
                t[0] += w * a.t.x;
                t[1] += w * a.t.y;
                t[2] += w * a.t.z;
            }
            Affine {
                m: Mat3::from_rows(m),
                t: Vec3::from(t),
            }
            .apply(x)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rodrigues_matches_known_rotations() {
        let r = rotvec_to_rotmat(Vec3::new(0.0, 0.0, std::f64::consts::FRAC_PI_3));
        // The capture's matrix for rotvec (0, 0, 60°) (built with I + sin·K + (1 − cos)·K², so equal to
        // rounding only).
        assert!(
            (r.rows[0][0] - 0.5000000000000001).abs() < 1e-15
                && (r.rows[0][1] + 0.8660254037844386).abs() < 1e-15
        );
        let small = rotvec_to_rotmat(Vec3::new(1e-7, 0.0, 0.0));
        assert_eq!(small.rows[2][1], 1e-7);
        assert_eq!(small.rows[1][1], 1.0);
        assert_eq!(rotvec_to_rotmat(Vec3::ZERO), Mat3::IDENTITY);
    }

    #[test]
    fn rigid_inverse_composes_to_identity() {
        let a = Affine::from_rotvec(Vec3::new(0.3, -0.2, 0.9), Vec3::new(1.0, 2.0, -3.0));
        let i = a.compose(&a.rigid_inverse());
        for (r, row) in i.m.rows.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                assert!((v - if r == c { 1.0 } else { 0.0 }).abs() < 1e-15);
            }
        }
        assert!(i.t.norm() < 1e-15);
        assert_eq!(Affine::from_rows4(&a.to_rows4()), a);
    }

    /// Two bones in a chain: rotating the child about its head moves only its own vertices.
    #[test]
    fn fk_and_skinning_on_a_two_bone_chain() {
        let heads = [Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0)];
        let tails = [Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 2.0)];
        let rolls = [Mat3::IDENTITY; 2];
        let rest = rest_bone_frames(
            &heads,
            &tails,
            &rolls,
            Vec3::new(0.0, 1.0, 0.0),
            Mat3::IDENTITY,
        );
        // Bone 1 points along +Z: its local +Y is world +Z.
        assert!(
            (rest[1].m.mul_vec(Vec3::new(0.0, 1.0, 0.0)) - Vec3::new(0.0, 0.0, 1.0)).norm() < 1e-15
        );
        let deltas = [
            Affine::IDENTITY,
            Affine::from_rotvec(Vec3::new(std::f64::consts::FRAC_PI_2, 0.0, 0.0), Vec3::ZERO),
        ];
        let (poses, transforms) = forward_kinematics(&rest, &deltas, &[-1, 0]);
        assert!((poses[1].t - heads[1]).norm() < 1e-15);
        let verts = [Vec3::new(0.0, 0.0, 0.5), Vec3::new(0.0, 0.0, 2.0)];
        let out = linear_blend_skinning(&verts, &[0, 1], &[1.0, 1.0], 1, &transforms);
        assert!((out[0] - verts[0]).norm() < 1e-15);
        // The tip swings a quarter turn about the child's head (local X = world X).
        assert!(
            (out[1].z - 1.0).abs() < 1e-15 && (out[1].y.abs() - 1.0).abs() < 1e-15,
            "{:?}",
            out[1]
        );
    }

    #[test]
    fn dqs_matches_lbs_on_rigid_and_single_bone_transforms() {
        let a135 = 135.0_f64.to_radians();
        let cases = [
            Affine::IDENTITY,
            Affine::from_rotvec(Vec3::new(0.4, -0.7, 1.1), Vec3::new(0.3, -0.5, 0.8)),
            // Non-180-degree rotations with trace < 0 and nonzero w (verifies w sign in branches 2-4).
            Affine::from_rotvec(Vec3::new(a135, 0.0, 0.0), Vec3::new(0.5, -0.2, 0.7)),
            Affine::from_rotvec(Vec3::new(0.0, a135, 0.0), Vec3::new(-0.3, 0.9, -0.1)),
            Affine::from_rotvec(Vec3::new(0.0, 0.0, a135), Vec3::new(0.2, 0.4, -0.8)),
            Affine::from_rotvec(
                Vec3::new(1.0, -0.4, 0.25) * (a135 / (1.0_f64 + 0.16 + 0.0625).sqrt()),
                Vec3::new(-0.4, 0.2, 0.6),
            ),
            // 180-degree rotations around every cardinal axis and diagonal (Shepperd branches).
            Affine::from_rotvec(
                Vec3::new(std::f64::consts::PI, 0.0, 0.0),
                Vec3::new(1.0, 2.0, 3.0),
            ),
            Affine::from_rotvec(
                Vec3::new(0.0, std::f64::consts::PI, 0.0),
                Vec3::new(-1.0, 0.5, 0.2),
            ),
            Affine::from_rotvec(
                Vec3::new(0.0, 0.0, std::f64::consts::PI),
                Vec3::new(0.0, -2.0, 1.5),
            ),
            Affine::from_rotvec(
                Vec3::new(1.0, 1.0, 1.0) * (std::f64::consts::PI / 3.0_f64.sqrt()),
                Vec3::new(0.25, -0.75, 0.5),
            ),
        ];
        let pts = [
            Vec3::new(0.1, -0.2, 0.3),
            Vec3::new(-0.5, 0.4, 1.2),
            Vec3::new(0.0, 0.0, 0.0),
        ];
        for a in cases {
            let transforms = [a, a];
            let indices = [0, 1, 0, 1, 0, 1];
            let weights = [0.7, 0.3, 1.0, 0.0, 0.25, 0.75];
            let lbs = linear_blend_skinning(&pts, &indices, &weights, 2, &transforms);
            let dqs = dual_quaternion_skinning(&pts, &indices, &weights, 2, &transforms);
            for (l, d) in lbs.iter().zip(&dqs) {
                assert!(
                    (*l - *d).norm() < 1e-13,
                    "rigid mismatch: lbs={l:?} dqs={d:?}"
                );
            }
        }
    }

    #[test]
    fn dqs_edge_cases_and_non_finite_propagation() {
        let pts = [Vec3::new(0.2, -0.3, 0.5)];
        // k == 0 returns rest vertices without panicking.
        assert_eq!(
            dual_quaternion_skinning(&pts, &[], &[], 0, &[Affine::IDENTITY]),
            pts
        );
        // All-zero weights return rest vertices without indexing any bone transform.
        let bad = Affine {
            m: Mat3::from_rows([[f64::NAN; 3]; 3]),
            t: Vec3::ZERO,
        };
        assert_eq!(
            dual_quaternion_skinning(&pts, &[0, 0], &[0.0, 0.0], 2, &[bad]),
            pts
        );
        // Active non-finite, zero, reflected, scaled, or sheared transforms propagate NaN
        // instead of fabricating a rotation.
        for (label, m) in [
            ("nan", Mat3::from_rows([[f64::NAN; 3]; 3])),
            ("zero", Mat3::from_rows([[0.0; 3]; 3])),
            (
                "reflection",
                Mat3::from_rows([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            ),
            (
                "scaled",
                Mat3::from_rows([[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]]),
            ),
            (
                "sheared",
                Mat3::from_rows([[1.0, 0.5, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            ),
        ] {
            let aff = Affine { m, t: Vec3::ZERO };
            let out = dual_quaternion_skinning(&pts, &[0], &[1.0], 1, &[aff]);
            assert!(
                !out[0].is_finite(),
                "{label} transform must fail closed with NaN, got {:?}",
                out[0]
            );
        }
        // Non-finite weight propagates NaN.
        let out_w = dual_quaternion_skinning(&pts, &[0], &[f64::NAN], 1, &[Affine::IDENTITY]);
        assert!(!out_w[0].is_finite());
        // Slightly non-orthonormal f32-like bone roll (~1e-7) projects cleanly onto SO(3).
        let mut r = rotvec_to_rotmat(Vec3::new(0.3, -0.5, 0.7));
        r.rows[0][0] += 1e-7;
        r.rows[1][2] -= 1e-7;
        let approx = Affine {
            m: r,
            t: Vec3::new(0.1, -0.2, 0.3),
        };
        let lbs = linear_blend_skinning(&pts, &[0], &[1.0], 1, &[approx]);
        let dqs = dual_quaternion_skinning(&pts, &[0], &[1.0], 1, &[approx]);
        assert!((lbs[0] - dqs[0]).norm() < 1e-6);
        assert!(SkinningMode::LinearBlend.is_lbs());
        assert!(!SkinningMode::DualQuaternion.is_lbs());
        assert_eq!(SkinningMode::parse("lbs"), Some(SkinningMode::LinearBlend));
        assert_eq!(
            SkinningMode::parse("dqs"),
            Some(SkinningMode::DualQuaternion)
        );
        assert_eq!(SkinningMode::parse("other"), None);
        assert_eq!(
            serde_json::to_value(SkinningMode::LinearBlend).unwrap(),
            serde_json::json!("lbs")
        );
        assert_eq!(
            serde_json::to_value(SkinningMode::DualQuaternion).unwrap(),
            serde_json::json!("dqs")
        );
        assert_eq!(
            serde_json::from_value::<SkinningMode>(serde_json::json!("lbs")).unwrap(),
            SkinningMode::LinearBlend
        );
        assert_eq!(
            serde_json::from_value::<SkinningMode>(serde_json::json!("dqs")).unwrap(),
            SkinningMode::DualQuaternion
        );
        assert!(serde_json::from_value::<SkinningMode>(serde_json::json!("other")).is_err());
    }

    #[test]
    fn dqs_preserves_cylinder_radius_under_180_degree_candy_wrapper_twist() {
        // Two coaxial bones: bone 0 fixed, bone 1 twisted 180 degrees around +Z.
        // At the midpoint (weights 0.5 / 0.5), LBS collapses the cross-section radius to 0
        // (candy-wrapper collapse), while DQS preserves the exact unit radius.
        let transforms = [
            Affine::IDENTITY,
            Affine::from_rotvec(Vec3::new(0.0, 0.0, std::f64::consts::PI), Vec3::ZERO),
        ];
        let ring: Vec<Vec3> = (0..8)
            .map(|i| {
                let a = (i as f64) * std::f64::consts::FRAC_PI_4;
                let (s, c) = det::sin_cos(a);
                Vec3::new(c, s, 0.5)
            })
            .collect();
        let indices: Vec<u32> = (0..ring.len()).flat_map(|_| [0u32, 1]).collect();
        let weights: Vec<f64> = (0..ring.len()).flat_map(|_| [0.5_f64, 0.5]).collect();
        let lbs = linear_blend_skinning(&ring, &indices, &weights, 2, &transforms);
        let dqs = dual_quaternion_skinning(&ring, &indices, &weights, 2, &transforms);
        for (l, d) in lbs.iter().zip(&dqs) {
            let r_lbs = det::hypot(l.x, l.y);
            let r_dqs = det::hypot(d.x, d.y);
            assert!(
                r_lbs < 1e-12,
                "LBS should collapse at 180 deg twist, got {r_lbs}"
            );
            assert!(
                (r_dqs - 1.0).abs() < 1e-13,
                "DQS must preserve radius 1.0, got {r_dqs}"
            );
            assert!((d.z - 0.5).abs() < 1e-13);
        }
    }

    #[test]
    fn dqs_antipodal_alignment_and_determinism() {
        // Bone 0 at 0 deg, bone 1 at 200 deg (shortest arc is -160 deg; dot product < 0).
        let transforms = [
            Affine::IDENTITY,
            Affine::from_rotvec(
                Vec3::new(0.0, 0.0, 200.0_f64.to_radians()),
                Vec3::new(0.0, 0.0, 0.2),
            ),
        ];
        let pts = [Vec3::new(1.0, 0.0, 0.0)];
        let dqs_a = dual_quaternion_skinning(&pts, &[0, 1], &[0.5, 0.5], 2, &transforms);
        let dqs_b = dual_quaternion_skinning(&pts, &[0, 1], &[0.5, 0.5], 2, &transforms);
        assert_eq!(dqs_a, dqs_b, "DQS must be bit-deterministic");
        // Midpoint of shortest arc (0 deg to -160 deg) is -80 deg (radius 1.0, z = 0.1).
        let r = det::hypot(dqs_a[0].x, dqs_a[0].y);
        assert!((r - 1.0).abs() < 1e-13, "got radius {r}");
        assert!((dqs_a[0].z - 0.1).abs() < 1e-13, "got z {}", dqs_a[0].z);
        assert!(
            dqs_a[0].y < -0.9,
            "expected shortest-arc -80 deg, got {:?}",
            dqs_a[0]
        );
    }

    #[test]
    #[should_panic]
    fn fk_panics_on_u32_truncating_bone_parent() {
        let rest = [Affine::IDENTITY, Affine::IDENTITY];
        let deltas = [Affine::IDENTITY, Affine::IDENTITY];
        let _ = forward_kinematics(&rest, &deltas, &[-1, 1i64 << 32]);
    }
}
