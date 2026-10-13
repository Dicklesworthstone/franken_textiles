//! OpenCV 5.0 morphology on 8-bit images (`morph.dispatch.cpp`): elliptical structuring elements, erosion and
//! dilation with OpenCV's default border (pixels outside the image never count), and `morphologyEx`'s open,
//! close and gradient. Minima and maxima round nothing, so any correct implementation gives OpenCV's bytes.

use super::image::Gray;

/// A structuring element: `w × h` flags, anchor at the centre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kernel {
    /// Width.
    pub w: usize,
    /// Height.
    pub h: usize,
    /// Row-major flags.
    pub on: Vec<bool>,
}

impl Kernel {
    /// `getStructuringElement(MORPH_ELLIPSE, (w, h))`.
    #[must_use]
    pub fn ellipse(w: usize, h: usize) -> Self {
        let (r, c) = ((h / 2) as i64, (w / 2) as i64);
        let inv_r2 = if r == 0 { 0.0 } else { 1.0 / (r * r) as f64 };
        let mut on = vec![false; w * h];
        for i in 0..h {
            let dy = i as i64 - r;
            if dy.abs() <= r {
                let dx = (c as f64 * (((r * r - dy * dy) as f64) * inv_r2).sqrt()).round_ties_even()
                    as i64;
                let (j1, j2) = (
                    (c - dx).max(0) as usize,
                    ((c + dx + 1).min(w as i64)) as usize,
                );
                on[i * w + j1..i * w + j2]
                    .iter_mut()
                    .for_each(|v| *v = true);
            }
        }
        Self { w, h, on }
    }

    /// `np.ones((w, h))`.
    #[must_use]
    pub fn rect(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            on: vec![true; w * h],
        }
    }

    /// The runs `(dy, dx0, dx1)` of set flags per row, relative to the anchor (`dx1` exclusive).
    fn runs(&self) -> Vec<(i64, i64, i64)> {
        let (ay, ax) = ((self.h / 2) as i64, (self.w / 2) as i64);
        let mut out = Vec::new();
        for i in 0..self.h {
            let mut j = 0;
            while j < self.w {
                if self.on[i * self.w + j] {
                    let start = j;
                    while j < self.w && self.on[i * self.w + j] {
                        j += 1;
                    }
                    out.push((i as i64 - ay, start as i64 - ax, j as i64 - ax));
                } else {
                    j += 1;
                }
            }
        }
        out
    }
}

fn apply(img: &Gray, k: &Kernel, dilate: bool) -> Gray {
    let (w, h) = (img.w as i64, img.h as i64);
    let runs = k.runs();
    let mut data = vec![0u8; img.data.len()];
    for y in 0..h {
        for x in 0..w {
            let mut acc: Option<u8> = None;
            for &(dy, dx0, dx1) in &runs {
                let yy = y + dy;
                if yy < 0 || yy >= h {
                    continue;
                }
                let (a, b) = ((x + dx0).max(0), (x + dx1).min(w));
                if a >= b {
                    continue;
                }
                let row = &img.data[(yy * w + a) as usize..(yy * w + b) as usize];
                let v = if dilate {
                    *row.iter().max().unwrap_or(&0)
                } else {
                    *row.iter().min().unwrap_or(&255)
                };
                acc = Some(match acc {
                    None => v,
                    Some(c) if dilate => c.max(v),
                    Some(c) => c.min(v),
                });
            }
            // a kernel that reaches no pixel: the default border (max for erosion, min for dilation)
            data[(y * w + x) as usize] = acc.unwrap_or(if dilate { 0 } else { 255 });
        }
    }
    Gray {
        w: img.w,
        h: img.h,
        data,
    }
}

/// `erode(img, k)`.
#[must_use]
pub fn erode(img: &Gray, k: &Kernel) -> Gray {
    apply(img, k, false)
}

/// `dilate(img, k)`.
#[must_use]
pub fn dilate(img: &Gray, k: &Kernel) -> Gray {
    apply(img, k, true)
}

/// `morphologyEx(img, MORPH_CLOSE, k)`: dilate, then erode.
#[must_use]
pub fn close(img: &Gray, k: &Kernel) -> Gray {
    erode(&dilate(img, k), k)
}

/// `morphologyEx(img, MORPH_OPEN, k)`: erode, then dilate.
#[must_use]
pub fn open(img: &Gray, k: &Kernel) -> Gray {
    dilate(&erode(img, k), k)
}

/// `morphologyEx(img, MORPH_GRADIENT, k)`: dilation minus erosion.
#[must_use]
pub fn gradient(img: &Gray, k: &Kernel) -> Gray {
    let (d, e) = (dilate(img, k), erode(img, k));
    let data = d
        .data
        .iter()
        .zip(&e.data)
        .map(|(a, b)| a.saturating_sub(*b))
        .collect();
    Gray {
        w: img.w,
        h: img.h,
        data,
    }
}
