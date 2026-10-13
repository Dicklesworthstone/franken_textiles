//! OpenCV 5.0 thresholds for 8-bit images (`thresh.cpp`): Otsu's threshold (`getThreshVal_Otsu`) and the
//! binary threshold.

use super::image::Gray;

/// Otsu's threshold: the gray level maximising the between-class variance, OpenCV's loop in double.
#[must_use]
pub fn otsu(img: &Gray) -> f64 {
    let mut h = [0u64; 256];
    for &v in &img.data {
        h[usize::from(v)] += 1;
    }
    let scale = 1.0 / (img.data.len() as f64);
    let mut mu = 0.0;
    for (i, &n) in h.iter().enumerate() {
        mu += i as f64 * n as f64;
    }
    mu *= scale;
    let (mut mu1, mut q1, mut max_sigma, mut max_val) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let eps = f64::from(f32::EPSILON);
    for (i, &n) in h.iter().enumerate() {
        let p_i = n as f64 * scale;
        mu1 *= q1;
        q1 += p_i;
        let q2 = 1.0 - q1;
        if q1.min(q2) < eps || q1.max(q2) > 1.0 - eps {
            continue;
        }
        mu1 = (mu1 + i as f64 * p_i) / q1;
        let mu2 = (mu - q1 * mu1) / q2;
        let sigma = q1 * q2 * (mu1 - mu2) * (mu1 - mu2);
        if sigma > max_sigma {
            max_sigma = sigma;
            max_val = i as f64;
        }
    }
    max_val
}

/// `threshold(img, t, 255, THRESH_BINARY)` for 8 bits: 255 where the value exceeds ⌊t⌋.
#[must_use]
pub fn binary(img: &Gray, t: f64) -> Gray {
    let t = t.floor();
    let data = img
        .data
        .iter()
        .map(|&v| if f64::from(v) > t { 255 } else { 0 })
        .collect();
    Gray {
        w: img.w,
        h: img.h,
        data,
    }
}

/// `threshold(img, 0, 255, THRESH_BINARY + THRESH_OTSU)`: the threshold and the mask.
#[must_use]
pub fn otsu_binary(img: &Gray) -> (f64, Gray) {
    let t = otsu(img);
    (t, binary(img, t))
}
