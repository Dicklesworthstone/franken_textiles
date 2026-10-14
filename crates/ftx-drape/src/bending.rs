//! Grain-frame orthotropic quadratic panel bending. Each six-node patch reproduces quadratic
//! polynomials exactly and annihilates affine maps. Under quasi-isometry its vector Hessian is the
//! second fundamental form times the surface normal. This is NOT a finite-stretch Koiter shell:
//! nonuniform in-plane deformation can also store energy. No dihedral-angle direction multiplier.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::{Refusal, RefusalCode, Vec2, Vec3};
use ftx_pattern::material_bending::BendingTensor;

use crate::elements::Local;

fn bad(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message).repair(
        "refine the source panel mesh; directional bending requires a resolved quadratic patch",
    )
}

/// A fixed rest-space curvature stencil integrated over one source triangle's full area.
/// The last three vertices extend the first triangle. Solver indices may repeat after seam welding.
#[derive(Clone, Debug)]
pub struct BendingPatch {
    /// Six solver vertices, in the same order as the local gradient and Hessian.
    pub v: [usize; 6],
    weights: [[f64; 6]; 3],
    gram: [[f64; 6]; 6],
    area: f64,
    tensor: BendingTensor,
}

fn quadratic(p: Vec2) -> [f64; 3] {
    [0.5 * p.x * p.x, 0.5 * p.y * p.y, p.x * p.y]
}

// Affine interpolation at a candidate, and the quadratic residual after that interpolation.
fn residual(base: [Vec2; 3], p: Vec2) -> ([f64; 3], [f64; 3]) {
    let (a, b, r) = (base[1] - base[0], base[2] - base[0], p - base[0]);
    let det = a.cross(b);
    let (u, v) = (r.cross(b) / det, a.cross(r) / det);
    let bary = [1.0 - u - v, u, v];
    let q = quadratic(p);
    let row = std::array::from_fn(|j| {
        q[j] - (0..3).map(|i| bary[i] * quadratic(base[i])[j]).sum::<f64>()
    });
    (row, bary)
}

fn inverse(a: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let mut b = [[0.0; 6]; 3];
    let norm = a
        .iter()
        .map(|r| r.iter().map(|x| x.abs()).sum::<f64>())
        .fold(0.0, f64::max);
    if !norm.is_finite() || norm == 0.0 {
        return None;
    }
    for i in 0..3 {
        b[i][..3].copy_from_slice(&a[i]);
        b[i][i + 3] = 1.0;
    }
    for j in 0..3 {
        let pivot = (j..3).max_by(|&i, &k| b[i][j].abs().total_cmp(&b[k][j].abs()))?;
        b.swap(j, pivot);
        let d = b[j][j];
        if !d.is_finite() || d.abs() < 1e-12 * norm {
            return None;
        }
        for entry in &mut b[j] {
            *entry /= d;
        }
        for i in 0..3 {
            if i != j {
                let v = b[i][j];
                for k in 0..6 {
                    b[i][k] -= v * b[j][k];
                }
            }
        }
    }
    let out: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| b[i][j + 3]));
    let inv_norm = out
        .iter()
        .map(|r| r.iter().map(|x| x.abs()).sum::<f64>())
        .fold(0.0, f64::max);
    (inv_norm.is_finite() && norm * inv_norm <= 1e6).then_some(out)
}

impl BendingPatch {
    /// Construct in the source panel's material coordinates (metres), with +u along grain.
    /// The represented area is the first triangle's rest area, not the six-node convex hull.
    ///
    /// # Errors
    /// Non-finite, degenerate or ill-conditioned quadratic rest geometry, or coefficient overflow.
    pub fn new(v: [usize; 6], rest: [Vec2; 6], tensor: BendingTensor) -> Result<Self, Refusal> {
        if rest.iter().any(|p| !p.is_finite()) {
            return Err(bad("non-finite bending rest coordinates"));
        }
        let origin = rest[0];
        let scale = (rest[1] - origin).norm().max((rest[2] - origin).norm());
        let area = 0.5 * (rest[1] - origin).cross(rest[2] - origin).abs();
        if !scale.is_finite() || !area.is_finite() || scale <= 0.0 || area <= 0.0 {
            return Err(bad("degenerate bending triangle"));
        }
        let points = rest.map(|p| (p - origin) / scale);
        let base = [points[0], points[1], points[2]];
        let rows = std::array::from_fn(|i| residual(base, points[i + 3]).0);
        let inv = inverse(rows).ok_or_else(|| {
            bad("bending patch cannot resolve independent warp/weft/twist curvatures")
        })?;
        let mut weights = [[0.0; 6]; 3];
        for k in 0..3 {
            // Engineering curvature doubles the mixed derivative, exactly as the tensor contract.
            let factor = if k == 2 { 2.0 } else { 1.0 };
            for j in 0..3 {
                let c = inv[k][j] * factor / (scale * scale);
                weights[k][j + 3] = c;
                let bary = residual(base, points[j + 3]).1;
                for i in 1..3 {
                    weights[k][i] -= c * bary[i];
                }
            }
            weights[k][0] = -weights[k][1..].iter().sum::<f64>();
        }
        let d = tensor.matrix();
        let mut gram = [[0.0; 6]; 6];
        for i in 0..6 {
            for j in 0..=i {
                let mut value = 0.0;
                for k in 0..3 {
                    for l in 0..3 {
                        value += weights[k][i] * d[k][l] * weights[l][j];
                    }
                }
                gram[i][j] = area * value;
                gram[j][i] = gram[i][j];
            }
        }
        if weights
            .iter()
            .flatten()
            .chain(gram.iter().flatten())
            .any(|v| !v.is_finite())
        {
            return Err(bad("bending coefficient overflow"));
        }
        Ok(Self {
            v,
            weights,
            gram,
            area,
            tensor,
        })
    }

    /// Vector-valued [x_uu,x_vv,2x_uv], in 1/m. Absolute positions cancel before accumulation.
    #[must_use]
    pub fn curvature(&self, x: &[Vec3]) -> [Vec3; 3] {
        std::array::from_fn(|k| {
            (1..6).fold(Vec3::ZERO, |acc, i| {
                acc + (x[self.v[i]] - x[self.v[0]]) * self.weights[k][i]
            })
        })
    }

    /// Panel energy in joules, objective under rigid rotations and zero for affine flat maps.
    #[must_use]
    pub fn energy(&self, x: &[Vec3]) -> f64 {
        let c = self.curvature(x);
        let d = self.tensor.matrix();
        let mut e = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                e += d[i][j] * c[i].dot(c[j]);
            }
        }
        0.5 * self.area * e
    }

    /// Exact energy/gradient/constant PSD Hessian of this quasi-isometric quadratic model.
    #[must_use]
    pub fn local(&self, x: &[Vec3]) -> Local<18, 324> {
        let c = self.curvature(x);
        let d = self.tensor.matrix();
        let moment: [Vec3; 3] =
            std::array::from_fn(|i| (0..3).fold(Vec3::ZERO, |acc, j| acc + c[j] * d[i][j]));
        let mut grad = [0.0; 18];
        let mut hess = [0.0; 324];
        for i in 0..6 {
            let g =
                (0..3).fold(Vec3::ZERO, |acc, k| acc + moment[k] * self.weights[k][i]) * self.area;
            for k in 0..3 {
                grad[3 * i + k] = g[k];
            }
            for j in 0..6 {
                for k in 0..3 {
                    hess[(3 * i + k) * 18 + 3 * j + k] = self.gram[i][j];
                }
            }
        }
        Local {
            energy: self.energy(x),
            grad,
            hess,
            psd: true,
        }
    }
}

fn choose(base: [usize; 3], rest: &[Vec2], candidates: &BTreeSet<usize>) -> Option<[usize; 6]> {
    let origin = rest[base[0]];
    let scale = (rest[base[1]] - origin)
        .norm()
        .max((rest[base[2]] - origin).norm());
    let points = base.map(|i| (rest[i] - origin) / scale);
    let mut orthogonal = Vec::<[f64; 3]>::new();
    let mut chosen = Vec::new();
    for _ in 0..3 {
        let mut best = None;
        let mut score = 0.0;
        for &i in candidates {
            if base.contains(&i) || chosen.contains(&i) {
                continue;
            }
            let p = (rest[i] - origin) / scale;
            let (mut row, _) = residual(points, p);
            for q in &orthogonal {
                let dot = row.iter().zip(q).map(|(a, b)| a * b).sum::<f64>();
                for k in 0..3 {
                    row[k] -= dot * q[k];
                }
            }
            let n = row.iter().map(|x| x * x).sum::<f64>().sqrt();
            // Prefer independent local information, not a distant high-magnitude quadratic.
            let s = n / (1.0 + p.norm2());
            if s > score && s.is_finite() {
                score = s;
                best = Some((i, row.map(|v| v / n)));
            }
        }
        if score < 1e-9 {
            return None;
        }
        let (i, q) = best?;
        chosen.push(i);
        orthogonal.push(q);
    }
    Some([base[0], base[1], base[2], chosen[0], chosen[1], chosen[2]])
}

/// Build patches on UNWELDED source connectivity. A seam, slit or separate layer is not a neighbor
/// merely because vertices coincide in 3D or become welded. Boundary patches extend inward.
pub(crate) fn build(
    rest: &[Vec2],
    tris: &[[usize; 3]],
    weld: &[usize],
    laws: &[Option<BendingTensor>],
) -> Result<Vec<BendingPatch>, Refusal> {
    if tris.len() != laws.len()
        || rest.len() != weld.len()
        || rest.iter().any(|p| !p.is_finite())
        || tris.iter().flatten().any(|&i| i >= rest.len())
    {
        return Err(bad("invalid source connectivity for panel bending"));
    }
    let mut edges = BTreeMap::<(usize, usize), Vec<usize>>::new();
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            edges.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    let mut neighbors = vec![Vec::new(); tris.len()];
    for uses in edges.values() {
        match uses.as_slice() {
            [a, b] => {
                if laws[*a].is_some() != laws[*b].is_some() {
                    return Err(bad(
                        "one connected source panel cannot mix scalar hinges and tensor patches; split at an explicit seam",
                    ));
                }
                neighbors[*a].push(*b);
                neighbors[*b].push(*a);
            }
            [_] => {}
            _ => return Err(bad("non-manifold source panel")),
        }
    }
    let mut patches = Vec::new();
    for (i, law) in laws.iter().enumerate() {
        let Some(law) = law else {
            continue;
        };
        let mut visited = BTreeSet::from([i]);
        let mut frontier = vec![i];
        let mut candidates = BTreeSet::new();
        let mut patch = None;
        for _ in 0..3 {
            let mut next = Vec::new();
            for t in frontier {
                for &other in &neighbors[t] {
                    if visited.insert(other) {
                        next.push(other);
                        candidates.extend(tris[other]);
                    }
                }
            }
            if candidates.len() > 96 {
                break;
            }
            if let Some(v) = choose(tris[i], rest, &candidates)
                && let Ok(p) = BendingPatch::new(v.map(|j| weld[j]), v.map(|j| rest[j]), *law)
            {
                patch = Some(p);
                break;
            }
            frontier = next;
        }
        patches.push(patch.ok_or_else(|| bad(format!("source triangle {i} lacks a well-conditioned six-node panel bending patch within three edge rings")))?);
    }
    Ok(patches)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest() -> [Vec2; 6] {
        [(0., 0.), (1., 0.), (0., 1.), (1., 1.), (2., 0.), (0., 2.)].map(|(x, y)| Vec2::new(x, y))
    }
    fn tensor() -> BendingTensor {
        BendingTensor::new(2.0, 5.0, 0.4, 3.0).unwrap()
    }
    fn patch() -> BendingPatch {
        BendingPatch::new([0, 1, 2, 3, 4, 5], rest(), tensor()).unwrap()
    }
    fn surface() -> Vec<Vec3> {
        rest()
            .iter()
            .map(|p| {
                Vec3::new(
                    p.x,
                    p.y,
                    0.01 * p.x * p.x + 0.02 * p.y * p.y - 0.015 * p.x * p.y,
                )
            })
            .collect()
    }
    fn close(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} != {b}");
    }

    #[test]
    fn independent_quadratic_curvatures_and_twist_energy() {
        let p = patch();
        let x = surface();
        let c = p.curvature(&x);
        for (a, b) in c.iter().zip([0.02, 0.04, -0.03]) {
            close(a.z, b, 1e-15);
            close(a.x, 0.0, 1e-15);
            close(a.y, 0.0, 1e-15);
        }
        let exact = 0.25
            * (2.0 * 0.02 * 0.02 + 5.0 * 0.04 * 0.04 + 2.0 * 0.4 * 0.02 * 0.04 + 3.0 * 0.03 * 0.03);
        close(p.energy(&x), exact, 1e-15);
        let flat: Vec<_> = rest()
            .iter()
            .map(|p| Vec3::new(2.0 * p.x + 0.3 * p.y, -p.x + p.y, 0.5 * p.x))
            .collect();
        close(p.energy(&flat), 0.0, 1e-28);
        let rotated: Vec<_> = x
            .iter()
            .map(|p| Vec3::new(p.z + 10.0, p.y - 5.0, -p.x + 3.0))
            .collect();
        close(p.energy(&rotated), exact, 1e-13);
    }

    #[test]
    fn exact_nodal_derivatives_and_nonnegative_quadratic_form() {
        let p = patch();
        let x = surface();
        let l = p.local(&x);
        let eps = 1e-5;
        for i in 0..18 {
            let mut plus = x.clone();
            let mut minus = x.clone();
            let delta = [
                Vec3::new(eps, 0., 0.),
                Vec3::new(0., eps, 0.),
                Vec3::new(0., 0., eps),
            ][i % 3];
            plus[i / 3] += delta;
            minus[i / 3] -= delta;
            close(
                l.grad[i],
                (p.energy(&plus) - p.energy(&minus)) / (2.0 * eps),
                1e-9,
            );
            let (lp, lm) = (p.local(&plus), p.local(&minus));
            for j in 0..18 {
                close(
                    l.hess[j * 18 + i],
                    (lp.grad[j] - lm.grad[j]) / (2.0 * eps),
                    1e-8,
                );
            }
        }
        for seed in 0..40 {
            let d: [f64; 18] = std::array::from_fn(|i| ((i * 13 + seed * 7) % 29) as f64 - 14.0);
            let value = (0..18)
                .map(|i| {
                    (0..18)
                        .map(|j| d[i] * l.hess[i * 18 + j] * d[j])
                        .sum::<f64>()
                })
                .sum::<f64>();
            assert!(value >= -1e-9);
        }
    }

    #[test]
    fn boundaries_refine_inward_and_source_slits_are_not_bridged() {
        let (rest, tris) = crate::cloth::grid(0.1, 0.1, 5, 5, Vec2::ZERO);
        let weld: Vec<_> = (0..rest.len()).collect();
        let laws = vec![Some(tensor()); tris.len()];
        let patches = build(&rest, &tris, &weld, &laws).unwrap();
        assert_eq!(patches.len(), tris.len());
        let x: Vec<_> = rest
            .iter()
            .map(|p| Vec3::new(p.x, p.y, p.x * p.x / 2.0))
            .collect();
        for p in patches {
            let c = p.curvature(&x);
            close(c[0].z, 1.0, 1e-10);
            close(c[1].z, 0.0, 1e-10);
            close(c[2].z, 0.0, 1e-10);
        }
        // Two geometrically touching, disconnected one-triangle panels cannot borrow support.
        let rest = vec![
            Vec2::ZERO,
            Vec2::new(1., 0.),
            Vec2::new(0., 1.),
            Vec2::new(1., 0.),
            Vec2::new(1., 1.),
            Vec2::new(0., 1.),
        ];
        assert!(
            build(
                &rest,
                &[[0, 1, 2], [3, 4, 5]],
                &[0, 1, 2, 1, 3, 2],
                &[Some(tensor()); 2]
            )
            .is_err()
        );
    }

    #[test]
    fn unresolved_and_nonfinite_patches_refuse_instead_of_defaulting() {
        let mut r = rest();
        r[5] = r[4];
        assert!(BendingPatch::new([0, 1, 2, 3, 4, 5], r, tensor()).is_err());
        r = rest();
        r[5].x = f64::NAN;
        assert!(BendingPatch::new([0, 1, 2, 3, 4, 5], r, tensor()).is_err());
    }
}
