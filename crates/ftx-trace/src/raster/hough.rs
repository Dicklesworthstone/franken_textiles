//! OpenCV 5.0 `HoughLinesP` (`hough.cpp`, `HoughLinesProbabilistic`): pixels visited in the order OpenCV's
//! RNG (multiply-with-carry, seeded with all ones) draws them, the float32 accumulator, and the fixed-point
//! walks that find each segment's ends and clear its pixels.

use ftx_core::det;

use super::image::Gray;

/// OpenCV's `RNG`.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = u64::from(self.0 as u32) * 4_164_903_690 + (self.0 >> 32);
        self.0 as u32
    }
}

/// OpenCV's `j·cos + i·sin` (float32) as the reference's clang-built arm64 binary evaluates it: contracted
/// to `fma(j, cos, i·sin)`. Unfused, one accumulator bin lands differently on the filled test sheet and its
/// 1,960 segments diverge from the 618th; fused, every case matches. (An x86 build would not fuse.)
const FUSED: bool = true;

fn rho_bin(j: i64, i: i64, c: f32, s: f32) -> i32 {
    let v = if FUSED {
        (j as f32).mul_add(c, i as f32 * s)
    } else {
        j as f32 * c + i as f32 * s
    };
    v.round_ties_even() as i32
}

/// `HoughLinesP(img, rho, theta, threshold, minLineLength, maxLineGap)`: segments `[x1, y1, x2, y2]`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn hough_lines_p(
    img: &Gray,
    rho: f64,
    theta: f64,
    threshold: i32,
    min_len: f64,
    max_gap: f64,
) -> Vec<[i32; 4]> {
    const MAX_ACCUM_CELLS: usize = 64_000_000;
    if img.w == 0
        || img.h == 0
        || img.w.checked_mul(img.h) != Some(img.data.len())
        || !rho.is_finite()
        || rho <= 0.0
        || !theta.is_finite()
        || theta <= 0.0
        || !min_len.is_finite()
        || !max_gap.is_finite()
        || threshold <= 0
    {
        return Vec::new();
    }
    let (rho, theta) = (rho as f32, theta as f32);
    if !rho.is_finite() || rho <= 0.0 || !theta.is_finite() || theta <= 0.0 {
        return Vec::new();
    }
    let (line_length, line_gap) = (
        min_len.round_ties_even() as i64,
        max_gap.round_ties_even() as i64,
    );
    let irho = 1.0f32 / rho;
    let (width, height) = (img.w as i64, img.h as i64);
    let th = f64::from(theta);
    let numangle_f = ((std::f64::consts::PI - 0.0) / th).floor();
    if !numangle_f.is_finite() || numangle_f < 0.0 || numangle_f >= MAX_ACCUM_CELLS as f64 {
        return Vec::new();
    }
    let mut numangle = numangle_f as i64 + 1;
    if numangle > 1 && (std::f64::consts::PI - (numangle - 1) as f64 * th).abs() < th / 2.0 {
        numangle -= 1;
    }
    let numrho_f = ((((width + height) * 2 + 1) as f32) / rho).round_ties_even();
    if !numrho_f.is_finite() || numrho_f <= 0.0 || numrho_f > MAX_ACCUM_CELLS as f32 {
        return Vec::new();
    }
    let numrho = numrho_f as i64;
    let Some(accum_len) = usize::try_from(numangle)
        .ok()
        .and_then(|a| usize::try_from(numrho).ok().and_then(|r| a.checked_mul(r)))
        .filter(|&len| len > 0 && len <= MAX_ACCUM_CELLS)
    else {
        return Vec::new();
    };
    let mut accum = vec![0i32; accum_len];
    let trig: Vec<(f32, f32)> = (0..numangle)
        .map(|n| {
            let a = n as f64 * th;
            (
                (det::cos(a) * f64::from(irho)) as f32,
                (det::sin(a) * f64::from(irho)) as f32,
            )
        })
        .collect();
    let mut mask: Vec<u8> = img.data.iter().map(|&v| u8::from(v != 0)).collect();
    let mut nz: Vec<(i64, i64)> = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if mask[(y * width + x) as usize] != 0 {
                nz.push((x, y));
            }
        }
    }
    let mut rng = Rng(u64::MAX);
    let mut lines = Vec::new();
    let shift = 16;
    let half_rho = (numrho - 1) / 2;
    let mut count = nz.len();
    while count > 0 {
        let idx = (rng.next() % count as u32) as usize;
        let (j, i) = nz[idx];
        nz[idx] = nz[count - 1];
        count -= 1;
        if mask[(i * width + j) as usize] == 0 {
            continue;
        }
        let (mut max_val, mut max_n) = (threshold - 1, 0usize);
        for (n, &(c, s)) in trig.iter().enumerate() {
            let r = i64::from(rho_bin(j, i, c, s)) + half_rho;
            if !(0..numrho).contains(&r) {
                continue;
            }
            let cell = &mut accum[n * numrho as usize + r as usize];
            *cell += 1;
            if max_val < *cell {
                max_val = *cell;
                max_n = n;
            }
        }
        if max_val < threshold {
            continue;
        }
        let a = -trig[max_n].1;
        let b = trig[max_n].0;
        let (mut x0, mut y0) = (j, i);
        let (xflag, dx0, dy0);
        if a.abs() > b.abs() {
            xflag = true;
            dx0 = if a > 0.0 { 1 } else { -1 };
            dy0 = (b * (1 << shift) as f32 / a.abs()).round_ties_even() as i64;
            y0 = (y0 << shift) + (1 << (shift - 1));
        } else {
            xflag = false;
            dy0 = if b > 0.0 { 1 } else { -1 };
            dx0 = (a * (1 << shift) as f32 / b.abs()).round_ties_even() as i64;
            x0 = (x0 << shift) + (1 << (shift - 1));
        }
        let mut line_end = [(0i64, 0i64); 2];
        for (k, end) in line_end.iter_mut().enumerate() {
            let mut gap = 0;
            let (mut x, mut y) = (x0, y0);
            let (dx, dy) = if k > 0 { (-dx0, -dy0) } else { (dx0, dy0) };
            loop {
                let (j1, i1) = if xflag {
                    (x, y >> shift)
                } else {
                    (x >> shift, y)
                };
                if j1 < 0 || j1 >= width || i1 < 0 || i1 >= height {
                    break;
                }
                if mask[(i1 * width + j1) as usize] != 0 {
                    gap = 0;
                    *end = (j1, i1);
                } else {
                    gap += 1;
                    if gap > line_gap {
                        break;
                    }
                }
                x += dx;
                y += dy;
            }
        }
        let good = (line_end[1].0 - line_end[0].0).abs() >= line_length
            || (line_end[1].1 - line_end[0].1).abs() >= line_length;
        for (k, end) in line_end.iter().enumerate() {
            let (mut x, mut y) = (x0, y0);
            let (dx, dy) = if k > 0 { (-dx0, -dy0) } else { (dx0, dy0) };
            loop {
                let (j1, i1) = if xflag {
                    (x, y >> shift)
                } else {
                    (x >> shift, y)
                };
                let cell = (i1 * width + j1) as usize;
                if mask[cell] != 0 {
                    if good {
                        for (n, &(c, s)) in trig.iter().enumerate() {
                            let r = i64::from(rho_bin(j1, i1, c, s)) + half_rho;
                            if (0..numrho).contains(&r) {
                                accum[n * numrho as usize + r as usize] -= 1;
                            }
                        }
                    }
                    mask[cell] = 0;
                }
                if i1 == end.1 && j1 == end.0 {
                    break;
                }
                x += dx;
                y += dy;
            }
        }
        if good {
            lines.push([
                line_end[0].0 as i32,
                line_end[0].1 as i32,
                line_end[1].0 as i32,
                line_end[1].1 as i32,
            ]);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hough_lines_p_rejects_degenerate_parameters_and_rasters_without_panic() {
        let empty = Gray {
            w: 0,
            h: 0,
            data: Vec::new(),
        };
        assert!(hough_lines_p(&empty, 1.0, 0.01, 5, 1.0, 1.0).is_empty());

        let bad_len = Gray {
            w: 4,
            h: 4,
            data: vec![255; 3],
        };
        assert!(hough_lines_p(&bad_len, 1.0, 0.01, 5, 1.0, 1.0).is_empty());

        let img = Gray::filled(8, 8, 255);
        for (rho, theta, threshold) in [
            (0.0, 0.01, 5),
            (-1.0, 0.01, 5),
            (f64::NAN, 0.01, 5),
            (1.0, 0.0, 5),
            (1.0, 1e-50, 5),
            (1.0, f64::NAN, 5),
            (1.0, 0.01, 0),
            (1e-9, 1e-6, 5),
        ] {
            assert!(
                hough_lines_p(&img, rho, theta, threshold, 1.0, 1.0).is_empty(),
                "rho={rho}, theta={theta}, threshold={threshold}"
            );
        }
    }
}
