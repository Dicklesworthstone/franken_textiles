//! Small fixed-size vectors and matrices.
//!
//! Plain `f64` arithmetic in a fixed order and no fused multiply-add, so results are bit-identical
//! native and wasm. [`Vec2::norm`] is `sqrt(x·x + y·y)`, the same expression numpy's
//! `linalg.norm(axis=1)` evaluates, so polyline lengths match the reference bit for bit.

use std::ops::{Add, AddAssign, Div, Index, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// A 2D point or vector (pattern space: millimetres, x right, y up). Serialised as `[x, y]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f64; 2]", into = "[f64; 2]")]
pub struct Vec2 {
    /// x coordinate.
    pub x: f64,
    /// y coordinate.
    pub y: f64,
}

impl Vec2 {
    /// The zero vector.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    /// A vector from its coordinates.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Dot product.
    #[must_use]
    pub fn dot(self, o: Self) -> f64 {
        self.x * o.x + self.y * o.y
    }

    /// z component of the 3D cross product (positive when `o` is counter-clockwise of `self`).
    #[must_use]
    pub fn cross(self, o: Self) -> f64 {
        self.x * o.y - self.y * o.x
    }

    /// The vector rotated +90°: `(−y, x)`.
    #[must_use]
    pub fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }

    /// Euclidean length, `sqrt(x·x + y·y)`.
    #[must_use]
    pub fn norm(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Squared length.
    #[must_use]
    pub fn norm2(self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    /// Distance to another point.
    #[must_use]
    pub fn dist(self, o: Self) -> f64 {
        (o - self).norm()
    }

    /// Unit vector in the same direction; the zero vector is returned unchanged.
    #[must_use]
    pub fn normalized(self) -> Self {
        let n = self.norm();
        if n == 0.0 { self } else { self / n }
    }

    /// Both coordinates finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Mirror across the y axis (`x → −x`).
    #[must_use]
    pub fn mirror_x(self) -> Self {
        Self::new(-self.x, self.y)
    }
}

impl From<[f64; 2]> for Vec2 {
    fn from(a: [f64; 2]) -> Self {
        Self::new(a[0], a[1])
    }
}

impl From<Vec2> for [f64; 2] {
    fn from(v: Vec2) -> Self {
        [v.x, v.y]
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y)
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Self) {
        *self = *self - o;
    }
}

impl Mul<f64> for Vec2 {
    type Output = Self;
    fn mul(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s)
    }
}

impl Mul<Vec2> for f64 {
    type Output = Vec2;
    fn mul(self, v: Vec2) -> Vec2 {
        Vec2::new(self * v.x, self * v.y)
    }
}

impl Div<f64> for Vec2 {
    type Output = Self;
    fn div(self, s: f64) -> Self {
        Self::new(self.x / s, self.y / s)
    }
}

impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

/// A 3D point or vector (body and drape space: metres, z up). Serialised as `[x, y, z]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f64; 3]", into = "[f64; 3]")]
pub struct Vec3 {
    /// x coordinate.
    pub x: f64,
    /// y coordinate.
    pub y: f64,
    /// z coordinate.
    pub z: f64,
}

impl Vec3 {
    /// The zero vector.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// A vector from its coordinates.
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Dot product.
    #[must_use]
    pub fn dot(self, o: Self) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    /// Cross product.
    #[must_use]
    pub fn cross(self, o: Self) -> Self {
        Self::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    /// Euclidean length, `sqrt(x·x + y·y + z·z)`.
    #[must_use]
    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// Squared length.
    #[must_use]
    pub fn norm2(self) -> f64 {
        self.dot(self)
    }

    /// Unit vector in the same direction; the zero vector is returned unchanged.
    #[must_use]
    pub fn normalized(self) -> Self {
        let n = self.norm();
        if n == 0.0 { self } else { self / n }
    }

    /// All coordinates finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl From<[f64; 3]> for Vec3 {
    fn from(a: [f64; 3]) -> Self {
        Self::new(a[0], a[1], a[2])
    }
}

impl From<Vec3> for [f64; 3] {
    fn from(v: Vec3) -> Self {
        [v.x, v.y, v.z]
    }
}

impl Index<usize> for Vec3 {
    type Output = f64;
    fn index(&self, i: usize) -> &f64 {
        match i {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            _ => panic!("Vec3 index {i} out of range"),
        }
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, o: Self) {
        *self = *self - o;
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Mul<Vec3> for f64 {
    type Output = Vec3;
    fn mul(self, v: Vec3) -> Vec3 {
        Vec3::new(self * v.x, self * v.y, self * v.z)
    }
}

impl Div<f64> for Vec3 {
    type Output = Self;
    fn div(self, s: f64) -> Self {
        Self::new(self.x / s, self.y / s, self.z / s)
    }
}

impl Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

/// A 3×3 matrix, row-major.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mat3 {
    /// Rows.
    pub rows: [[f64; 3]; 3],
}

impl Mat3 {
    /// The identity.
    pub const IDENTITY: Self = Self {
        rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    };

    /// A matrix from its rows.
    #[must_use]
    pub const fn from_rows(rows: [[f64; 3]; 3]) -> Self {
        Self { rows }
    }

    /// A matrix from its columns.
    #[must_use]
    pub fn from_cols(c0: Vec3, c1: Vec3, c2: Vec3) -> Self {
        Self {
            rows: [[c0.x, c1.x, c2.x], [c0.y, c1.y, c2.y], [c0.z, c1.z, c2.z]],
        }
    }

    /// Column `j`.
    #[must_use]
    pub fn col(&self, j: usize) -> Vec3 {
        Vec3::new(self.rows[0][j], self.rows[1][j], self.rows[2][j])
    }

    /// Transpose.
    #[must_use]
    pub fn transpose(&self) -> Self {
        let r = &self.rows;
        Self {
            rows: [
                [r[0][0], r[1][0], r[2][0]],
                [r[0][1], r[1][1], r[2][1]],
                [r[0][2], r[1][2], r[2][2]],
            ],
        }
    }

    /// Determinant (cofactor expansion along the first row).
    #[must_use]
    pub fn det(&self) -> f64 {
        let r = &self.rows;
        r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
            - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
            + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0])
    }

    /// Matrix–vector product.
    #[must_use]
    pub fn mul_vec(&self, v: Vec3) -> Vec3 {
        let r = &self.rows;
        Vec3::new(
            r[0][0] * v.x + r[0][1] * v.y + r[0][2] * v.z,
            r[1][0] * v.x + r[1][1] * v.y + r[1][2] * v.z,
            r[2][0] * v.x + r[2][1] * v.y + r[2][2] * v.z,
        )
    }

    /// Rotation by `angle` radians about the unit `axis` (Rodrigues), with deterministic trig.
    #[must_use]
    pub fn rotation(axis: Vec3, angle: f64) -> Self {
        let (s, c) = crate::det::sin_cos(angle);
        let t = 1.0 - c;
        let Vec3 { x, y, z } = axis;
        Self {
            rows: [
                [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
                [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
                [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
            ],
        }
    }
}

impl Mul for Mat3 {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        let mut rows = [[0.0; 3]; 3];
        for (i, row) in rows.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = self.rows[i][0] * o.rows[0][j]
                    + self.rows[i][1] * o.rows[1][j]
                    + self.rows[i][2] * o.rows[2][j];
            }
        }
        Self { rows }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec2_basics() {
        let a = Vec2::new(3.0, 4.0);
        assert_eq!(a.norm(), 5.0);
        assert_eq!(a.perp(), Vec2::new(-4.0, 3.0));
        assert_eq!(a.cross(a.perp()), 25.0);
        assert_eq!(a.dot(Vec2::new(1.0, 1.0)), 7.0);
        assert_eq!(serde_json::to_string(&a).unwrap(), "[3.0,4.0]");
        assert_eq!(
            serde_json::from_str::<Vec2>("[1, 2.5]").unwrap(),
            Vec2::new(1.0, 2.5)
        );
        assert_eq!(Vec2::ZERO.normalized(), Vec2::ZERO);
    }

    #[test]
    fn vec3_and_mat3() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        assert_eq!(x.cross(y), Vec3::new(0.0, 0.0, 1.0));
        let r = Mat3::rotation(Vec3::new(0.0, 0.0, 1.0), std::f64::consts::FRAC_PI_2);
        let v = r.mul_vec(x);
        assert!((v - y).norm() < 1e-15);
        assert!((r.det() - 1.0).abs() < 1e-15);
        let rt = r.transpose() * r;
        for (i, row) in rt.rows.iter().enumerate() {
            for (j, &cell) in row.iter().enumerate() {
                let e = if i == j { 1.0 } else { 0.0 };
                assert!((cell - e).abs() < 1e-15);
            }
        }
        assert_eq!(Mat3::from_cols(x, y, x.cross(y)), Mat3::IDENTITY);
        assert_eq!(Mat3::IDENTITY.col(1), y);
    }
}
