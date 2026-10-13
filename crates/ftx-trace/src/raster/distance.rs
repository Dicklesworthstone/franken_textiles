//! OpenCV 5.0 `distanceTransform(img, DIST_L2, 3 | 5)` (`distransform.cpp`: the integer chamfer passes in
//! 16-bit fixed point, converted to float32) and `connectedComponentsWithStats` with 8-connectivity
//! (labels numbered as OpenCV's block-based scan numbers them).

use std::collections::VecDeque;

use super::image::Gray;

/// A float32 image, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct GrayF {
    /// Width.
    pub w: usize,
    /// Height.
    pub h: usize,
    /// `w·h` values.
    pub data: Vec<f32>,
}

/// `CV_FLT_TO_FIX(x, 16)`: `cvRound(x · 2¹⁶)` in float32.
fn fix(x: f32) -> u32 {
    (x * 65536.0).round_ties_even() as u32
}

/// `distanceTransform(img, DIST_L2, mask)` with `mask` 3 (weights 0.955, 1.3693) or 5 (1, 1.4, 2.1969): the
/// chamfer distance of every non-zero pixel to the nearest zero pixel.
///
/// # Panics
/// If `mask` is neither 3 nor 5.
#[must_use]
pub fn distance_l2(img: &Gray, mask: u8) -> GrayF {
    assert!(mask == 3 || mask == 5, "mask size 3 or 5");
    let border = if mask == 3 { 1 } else { 2 };
    let (hv, diag, long) = if mask == 3 {
        (fix(0.955), fix(1.3693), 0)
    } else {
        (fix(1.0), fix(1.4), fix(2.1969))
    };
    let dist_max = u32::MAX - if mask == 3 { diag } else { long };
    let (w, h) = (img.w, img.h);
    let step = w + 2 * border;
    let mut t = vec![dist_max; step * (h + 2 * border)];
    let at = |r: usize, c: isize| -> usize { r * step + (border as isize + c) as usize };
    // forward pass (rows border..border+h)
    for i in 0..h {
        let r = i + border;
        for k in 1..=border {
            t[at(r, -(k as isize))] = dist_max;
            t[at(r, (w + k - 1) as isize)] = dist_max;
        }
        let mut left = dist_max;
        for j in 0..w {
            let jj = j as isize;
            if img.data[i * w + j] == 0 {
                t[at(r, jj)] = 0;
                left = 0;
                continue;
            }
            let mut m = if mask == 3 {
                (t[at(r - 1, jj - 1)] + diag)
                    .min(t[at(r - 1, jj)] + hv)
                    .min(t[at(r - 1, jj + 1)] + diag)
            } else {
                (t[at(r - 2, jj - 1)] + long)
                    .min(t[at(r - 2, jj + 1)] + long)
                    .min(t[at(r - 1, jj - 2)] + long)
                    .min(t[at(r - 1, jj - 1)] + diag)
                    .min(t[at(r - 1, jj)] + hv)
                    .min(t[at(r - 1, jj + 1)] + diag)
                    .min(t[at(r - 1, jj + 2)] + long)
            };
            m = m.min(left + hv).min(dist_max);
            t[at(r, jj)] = m;
            left = m;
        }
    }
    // backward pass
    let scale = 1.0f32 / 65536.0;
    let mut out = vec![0.0f32; w * h];
    for i in (0..h).rev() {
        let r = i + border;
        let mut right = dist_max;
        for j in (0..w).rev() {
            let jj = j as isize;
            let mut t0 = t[at(r, jj)];
            if t0 > hv {
                let cands: &[u32] = &if mask == 3 {
                    [
                        t[at(r + 1, jj + 1)] + diag,
                        t[at(r + 1, jj)] + hv,
                        t[at(r + 1, jj - 1)] + diag,
                        right + hv,
                        u32::MAX,
                        u32::MAX,
                        u32::MAX,
                        u32::MAX,
                    ]
                } else {
                    [
                        t[at(r + 2, jj + 1)] + long,
                        t[at(r + 2, jj - 1)] + long,
                        t[at(r + 1, jj + 2)] + long,
                        t[at(r + 1, jj + 1)] + diag,
                        t[at(r + 1, jj)] + hv,
                        t[at(r + 1, jj - 1)] + diag,
                        t[at(r + 1, jj - 2)] + long,
                        right + hv,
                    ]
                };
                for &c in cands {
                    if c < t0 {
                        t0 = c;
                    }
                }
                t[at(r, jj)] = t0;
            }
            right = t[at(r, jj)];
        }
        for j in 0..w {
            out[i * w + j] = t[at(r, j as isize)] as f32 * scale;
        }
    }
    GrayF { w, h, data: out }
}

/// One connected component's statistics: bounding box and pixel count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    /// Left column.
    pub x: usize,
    /// Top row.
    pub y: usize,
    /// Width.
    pub w: usize,
    /// Height.
    pub h: usize,
    /// Pixels.
    pub area: usize,
}

/// `connectedComponentsWithStats(img, connectivity=8)`: labels (0 = background, 1.. in the order of each
/// component's first 2×2 block in block-raster order, as OpenCV's Spaghetti scan numbers them) and per-label
/// statistics (index 0 is the background).
#[must_use]
pub fn components8(img: &Gray) -> (Vec<u32>, Vec<Stats>) {
    let (w, h) = (img.w, img.h);
    let mut lab = vec![0u32; w * h];
    let mut first_block: Vec<(usize, usize)> = Vec::new();
    let mut n = 0u32;
    let mut queue = VecDeque::new();
    for y in 0..h {
        for x in 0..w {
            if img.data[y * w + x] == 0 || lab[y * w + x] != 0 {
                continue;
            }
            n += 1;
            lab[y * w + x] = n;
            let mut fb = (y / 2, x / 2);
            queue.push_back((x, y));
            while let Some((cx, cy)) = queue.pop_front() {
                fb = fb.min((cy / 2, cx / 2));
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let (nx, ny) = (cx as i64 + dx, cy as i64 + dy);
                        if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                            continue;
                        }
                        let k = ny as usize * w + nx as usize;
                        if img.data[k] != 0 && lab[k] == 0 {
                            lab[k] = n;
                            queue.push_back((nx as usize, ny as usize));
                        }
                    }
                }
            }
            first_block.push(fb);
        }
    }
    // renumber by first block
    let mut order: Vec<u32> = (1..=n).collect();
    order.sort_by_key(|&l| first_block[(l - 1) as usize]);
    let mut remap = vec![0u32; n as usize + 1];
    for (new, &old) in order.iter().enumerate() {
        remap[old as usize] = new as u32 + 1;
    }
    let mut stats = vec![
        Stats {
            x: usize::MAX,
            y: usize::MAX,
            w: 0,
            h: 0,
            area: 0
        };
        n as usize + 1
    ];
    let mut ext = vec![(0usize, 0usize); n as usize + 1];
    for y in 0..h {
        for x in 0..w {
            let l = remap[lab[y * w + x] as usize] as usize;
            lab[y * w + x] = l as u32;
            let s = &mut stats[l];
            s.x = s.x.min(x);
            s.y = s.y.min(y);
            ext[l] = (ext[l].0.max(x), ext[l].1.max(y));
            s.area += 1;
        }
    }
    for (s, e) in stats.iter_mut().zip(&ext) {
        if s.area > 0 {
            s.w = e.0 - s.x + 1;
            s.h = e.1 - s.y + 1;
        }
    }
    (lab, stats)
}
