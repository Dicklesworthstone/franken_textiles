//! Sheet-plane projective geometry. All fits are normalized before solving; rank loss and
//! projective horizons are refusals, not enormous or non-finite measurements.

use ftx_core::{Refusal, RefusalCode, det};

pub(super) type Point = [f64; 2];
type Matrix = [[f64; 3]; 3];

pub(super) fn degenerate(message: &str) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message)
        .repair("use distinct, well-spread reference points on a flat sheet")
}

#[derive(Clone, Debug)]
pub(super) struct Homography(pub Matrix);

fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

impl Homography {
    fn new(mut h: Matrix) -> Result<Self, Refusal> {
        let scale = h.iter().flatten().map(|v| v.abs()).fold(0.0, f64::max);
        if scale == 0.0 || !h.iter().flatten().all(|v| v.is_finite()) {
            return Err(degenerate("invalid homography"));
        }
        let scale = if h[2][2].abs() > 1e-12 * scale {
            h[2][2]
        } else {
            scale
        };
        for x in h.iter_mut().flatten() {
            *x /= scale;
        }
        Ok(Self(h))
    }

    pub(super) fn denominator(&self, p: Point) -> Result<f64, Refusal> {
        if !p.iter().all(|v| v.is_finite()) {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "pixel coordinates must be finite",
            ));
        }
        let h = &self.0[2];
        let w = h[0] * p[0] + h[1] * p[1] + h[2];
        let magnitude = (h[0] * p[0]).abs() + (h[1] * p[1]).abs() + h[2].abs();
        if !w.is_finite() || w.abs() <= 64.0 * f64::EPSILON * magnitude {
            return Err(degenerate("point lies on the sheet's projective horizon"));
        }
        Ok(w)
    }

    pub(super) fn map(&self, p: Point) -> Result<Point, Refusal> {
        let w = self.denominator(p)?;
        let q =
            std::array::from_fn(|i| (self.0[i][0] * p[0] + self.0[i][1] * p[1] + self.0[i][2]) / w);
        if q.iter().all(|v| v.is_finite()) {
            Ok(q)
        } else {
            Err(degenerate("homography projection overflow"))
        }
    }

    pub(super) fn jacobian(&self, p: Point) -> Result<[[f64; 2]; 2], Refusal> {
        let w = self.denominator(p)?;
        let q = self.map(p)?;
        let j: [[f64; 2]; 2] = std::array::from_fn(|i| {
            std::array::from_fn(|k| (self.0[i][k] - q[i] * self.0[2][k]) / w)
        });
        if j.iter().flatten().all(|v| v.is_finite()) {
            Ok(j)
        } else {
            Err(degenerate("homography Jacobian overflow"))
        }
    }

    pub(super) fn inverse(&self) -> Result<Self, Refusal> {
        let h = &self.0;
        let cof: Matrix = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                h[(i + 1) % 3][(j + 1) % 3] * h[(i + 2) % 3][(j + 2) % 3]
                    - h[(i + 1) % 3][(j + 2) % 3] * h[(i + 2) % 3][(j + 1) % 3]
            })
        });
        let determinant: f64 = (0..3).map(|j| h[0][j] * cof[0][j]).sum();
        if determinant == 0.0 || !determinant.is_finite() {
            return Err(degenerate("singular homography"));
        }
        Self::new(std::array::from_fn(|i| std::array::from_fn(|j| cof[j][i])))
    }
}

fn normalize(points: &[Point]) -> Result<(Vec<Point>, Matrix, Matrix), Refusal> {
    if !points.iter().flatten().all(|v| v.is_finite()) {
        return Err(degenerate("reference coordinates must be finite"));
    }
    let n = points.len() as f64;
    let c: Point = std::array::from_fn(|i| points.iter().map(|p| p[i] / n).sum());
    let spread = points
        .iter()
        .map(|p| det::hypot(p[0] - c[0], p[1] - c[1]) / n)
        .sum::<f64>();
    if spread <= f64::EPSILON || !spread.is_finite() {
        return Err(degenerate("reference points coincide"));
    }
    let s = 2.0_f64.sqrt() / spread;
    Ok((
        points
            .iter()
            .map(|p| [s * (p[0] - c[0]), s * (p[1] - c[1])])
            .collect(),
        [[s, 0.0, -s * c[0]], [0.0, s, -s * c[1]], [0.0, 0.0, 1.0]],
        [[1.0 / s, 0.0, c[0]], [0.0, 1.0 / s, c[1]], [0.0, 0.0, 1.0]],
    ))
}

/// Column-pivoted Householder QR. Unlike a minimum-norm solver, an underdetermined calibration
/// is rejected. Normalization makes the relative rank threshold meaningful for px versus mm.
fn solve(mut a: Vec<[f64; 8]>, mut b: Vec<f64>) -> Result<[f64; 8], Refusal> {
    let m = a.len();
    let mut order: [usize; 8] = std::array::from_fn(|i| i);
    let mut largest = 0.0_f64;
    for k in 0..8 {
        let norms: [f64; 8] =
            std::array::from_fn(|j| a[k..].iter().map(|r| r[j] * r[j]).sum::<f64>().sqrt());
        let pivot = (k..8)
            .max_by(|&i, &j| norms[i].total_cmp(&norms[j]).then(j.cmp(&i)))
            .unwrap();
        largest = largest.max(norms[pivot]);
        if !norms[pivot].is_finite() || norms[pivot] <= 1e-10 * largest || norms[pivot] == 0.0 {
            return Err(degenerate("reference points do not determine a homography"));
        }
        for row in &mut a {
            row.swap(k, pivot);
        }
        order.swap(k, pivot);
        let alpha = -norms[pivot].copysign(a[k][k]);
        let mut v: Vec<f64> = a[k..].iter().map(|r| r[k]).collect();
        v[0] -= alpha;
        let vv: f64 = v.iter().map(|x| x * x).sum();
        for j in k..8 {
            let dot: f64 = (k..m).map(|i| v[i - k] * a[i][j]).sum();
            for i in k..m {
                a[i][j] -= (2.0 * dot / vv) * v[i - k];
            }
        }
        let dot: f64 = (k..m).map(|i| v[i - k] * b[i]).sum();
        for i in k..m {
            b[i] -= (2.0 * dot / vv) * v[i - k];
        }
        a[k][k] = alpha;
    }
    let mut x = [0.0; 8];
    for i in (0..8).rev() {
        x[i] = (b[i] - (i + 1..8).map(|j| a[i][j] * x[j]).sum::<f64>()) / a[i][i];
    }
    let mut result = [0.0; 8];
    for i in 0..8 {
        result[order[i]] = x[i];
    }
    if !result.iter().all(|v| v.is_finite()) {
        return Err(degenerate("homography fit overflow"));
    }
    Ok(result)
}

pub(super) fn fit(src: &[Point], dst: &[Point]) -> Result<Homography, Refusal> {
    if src.len() < 4 || src.len() != dst.len() {
        return Err(degenerate("homography needs at least four paired points"));
    }
    let (src, t, _) = normalize(src)?;
    let (dst, _, u_inv) = normalize(dst)?;
    let (mut a, mut b) = (
        Vec::with_capacity(2 * src.len()),
        Vec::with_capacity(2 * src.len()),
    );
    for (&[x, y], &[u, v]) in src.iter().zip(&dst) {
        a.push([x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y]);
        b.push(u);
        a.push([0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y]);
        b.push(v);
    }
    let x = solve(a, b)?;
    let h = Homography::new(multiply(
        u_inv,
        multiply(
            [[x[0], x[1], x[2]], [x[3], x[4], x[5]], [x[6], x[7], 1.0]],
            t,
        ),
    ))?;
    h.inverse()?;
    Ok(h)
}

pub(super) fn rectangle([w, h]: Point) -> [Point; 4] {
    [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]
}

pub(super) fn convex_clockwise(p: &[Point; 4]) -> bool {
    (0..4).all(|i| {
        let (a, b, c) = (p[i], p[(i + 1) % 4], p[(i + 2) % 4]);
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        cross > 0.0 && cross.is_finite()
    })
}

#[derive(Debug)]
pub(super) struct PlaneScale {
    pub centre: Point,
    pub mm_per_px: f64,
    pub axes: Point,
    pub tilt: f64,
    pub variation: f64,
}

pub(super) fn plane_scale(h: &Homography, size: Point) -> Result<PlaneScale, Refusal> {
    let inv = h.inverse()?;
    let centre = inv.map([size[0] / 2.0, size[1] / 2.0])?;
    let scale = |p| -> Result<f64, Refusal> {
        let j = h.jacobian(p)?;
        Ok((j[0][0] * j[1][1] - j[0][1] * j[1][0]).abs().sqrt())
    };
    let j = h.jacobian(centre)?;
    let a = j[0][0] * j[0][0] + j[0][1] * j[0][1];
    let b = j[0][0] * j[1][0] + j[0][1] * j[1][1];
    let c = j[1][0] * j[1][0] + j[1][1] * j[1][1];
    let hi = ((a + c + det::hypot(a - c, 2.0 * b)) / 2.0).sqrt();
    let mmpp = scale(centre)?;
    let lo = mmpp * mmpp / hi;
    if !hi.is_finite() || !lo.is_finite() || lo <= 1e-12 * hi {
        return Err(degenerate("sheet is edge-on or has unusable scale"));
    }
    let (mut min, mut max) = (f64::INFINITY, 0.0_f64);
    let side = inv.denominator([size[0] / 2.0, size[1] / 2.0])?;
    for q in rectangle(size) {
        if inv.denominator(q)?.is_sign_positive() != side.is_sign_positive() {
            return Err(degenerate("projective horizon crosses the reference sheet"));
        }
        let s = scale(inv.map(q)?)?;
        min = min.min(s);
        max = max.max(s);
    }
    let variation = max / min - 1.0;
    if !variation.is_finite() {
        return Err(degenerate("invalid scale variation"));
    }
    Ok(PlaneScale {
        centre,
        mm_per_px: mmpp,
        axes: [hi, lo],
        tilt: det::acos((lo / hi).min(1.0)).to_degrees(),
        variation,
    })
}

/// Spec 02 A.11: calibrated-camera rectangle constraints, over the specified bounded focal grid.
pub(super) fn rectangle_residual(
    corners: &[Point; 4],
    size: Point,
    image: Point,
) -> Result<(f64, f64), Refusal> {
    let centred: Vec<Point> = corners
        .iter()
        .map(|p| [p[0] - (image[0] - 1.0) / 2.0, p[1] - (image[1] - 1.0) / 2.0])
        .collect();
    let g = fit(&rectangle(size), &centred)?.0;
    let error = |f: f64| {
        let aa = (g[0][0] * g[0][0] + g[1][0] * g[1][0]) / (f * f) + g[2][0] * g[2][0];
        let bb = (g[0][1] * g[0][1] + g[1][1] * g[1][1]) / (f * f) + g[2][1] * g[2][1];
        let ab = (g[0][0] * g[0][1] + g[1][0] * g[1][1]) / (f * f) + g[2][0] * g[2][1];
        det::hypot(ab / (aa * bb).sqrt(), 0.5 * det::ln(aa / bb))
    };
    let diag = det::hypot(image[0], image[1]);
    let grid: Vec<f64> = (0..71)
        .map(|i| diag * 0.3 * det::exp(i as f64 / 70.0 * det::ln(10.0 / 0.3)))
        .collect();
    let best = (0..71)
        .min_by(|&a, &b| error(grid[a]).total_cmp(&error(grid[b])).then(a.cmp(&b)))
        .unwrap();
    let (lo, hi) = (grid[best.saturating_sub(1)], grid[(best + 1).min(70)]);
    let (mut e, mut f) = (error(grid[best]), grid[best]);
    for i in 0..41 {
        let trial = lo * det::exp(i as f64 / 40.0 * det::ln(hi / lo));
        let residual = error(trial);
        if residual < e {
            e = residual;
            f = trial;
        }
    }
    if !e.is_finite() {
        return Err(degenerate("rectangle residual is not finite"));
    }
    Ok((e, f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projective_fit_inverse_and_jacobian() {
        let truth = Homography([
            [0.43, 0.04, -123.0],
            [-0.02, 0.51, -246.0],
            [0.00008, -0.00002, 1.0],
        ]);
        let pixels = [
            [250.0, 200.0],
            [900.0, 220.0],
            [1000.0, 1300.0],
            [180.0, 1280.0],
            [500.0, 700.0],
            [800.0, 1000.0],
        ];
        let plane: Vec<_> = pixels.iter().map(|&p| truth.map(p).unwrap()).collect();
        let h = fit(&pixels, &plane).unwrap();
        for p in [[500.0, 700.0], [1250.0, 1200.0]] {
            let q = h.map(p).unwrap();
            let expected = truth.map(p).unwrap();
            for k in 0..2 {
                assert!((q[k] - expected[k]).abs() < 1e-9);
                assert!((h.inverse().unwrap().map(q).unwrap()[k] - p[k]).abs() < 1e-8);
                let mut plus = p;
                plus[k] += 0.001;
                let mut minus = p;
                minus[k] -= 0.001;
                for j in 0..2 {
                    assert!(
                        ((h.map(plus).unwrap()[j] - h.map(minus).unwrap()[j]) / 0.002
                            - h.jacobian(p).unwrap()[j][k])
                            .abs()
                            < 1e-8
                    );
                }
            }
        }
    }

    #[test]
    fn rejects_rank_loss_and_horizon() {
        let collinear = [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0], [3.0, 3.0]];
        assert!(fit(&collinear, &rectangle([210.0, 297.0])).is_err());
        assert!(fit(&rectangle([210.0, 297.0]), &collinear).is_err());
        let h = Homography([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, -10.0]]);
        assert!(h.map([10.0, 20.0]).is_err());
    }

    #[test]
    fn square_on_scale_and_wrong_paper_match_spec_d7() {
        let p = [
            [234.1, 820.6],
            [665.9, 820.6],
            [665.9, 1379.4],
            [234.1, 1379.4],
        ];
        let h = fit(&p, &rectangle([215.9, 279.4])).unwrap();
        let s = plane_scale(&h, [215.9, 279.4]).unwrap();
        assert!((s.mm_per_px - 0.5).abs() < 1e-12);
        assert!(s.variation < 1e-12 && s.tilt < 1e-5);
        let (rr, _) = rectangle_residual(&p, [210.0, 297.0], [1500.0, 2000.0]).unwrap();
        assert!((rr - 0.088796).abs() < 1e-6, "{rr}");
    }
}
