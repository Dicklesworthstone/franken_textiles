//! Bone frames, forward kinematics and linear blend skinning (spec 05 §0, §4.3, §5).
//!
//! Operation orders follow the package (roma's Rodrigues formula, `get_bone_poses`,
//! `parallel_forward_kinematic`, `linear_blend_skinning`) so results agree to rounding.

use ftx_core::{Mat3, Vec3, det};

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
            let p = usize::try_from(parents[j]).unwrap_or(0);
            assert!(p < j, "bone {j}: parent {p} must precede it");
            transforms[p].compose(&m)
        };
        transforms.push(pose.compose(&inv[j]));
        poses.push(pose);
    }
    (poses, transforms)
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
}
