//! OpenCV 5.0 thresholds for 8-bit images (`thresh.cpp`): Otsu's threshold (`getThreshVal_Otsu`) and the
//! binary threshold. Adaptive Gaussian foreground extraction follows spec 04 §10.2 step 6.

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

/// Gaussian adaptive foreground mask for tracing (spec 04 §10.2): odd block size
/// `max(15, min(width, height) / 20 | 1)`, binary comparison and C = -10.
/// The caller must invert the channel first when foreground is dark.
///
/// Uses replicated borders, separable float32 convolution and nearest-even conversion of
/// the local mean to u8 before comparing. This is the floating-point adaptiveThreshold
/// path, not GaussianBlur's different fixed-point u8 path. Kernel construction and reduction
/// order are fixed on native/wasm; architecture-specific OpenCV HAL rounding is not emulated.
/// One float32 scratch raster is allocated, not a block-sized window per pixel.
///
/// # Errors
/// An empty/malformed raster or more than 40 million pixels (the PNG decoder's ceiling).
pub fn adaptive_gaussian(img: &Gray) -> Result<Gray, String> {
    if img.w == 0 || img.h == 0 || img.w.checked_mul(img.h) != Some(img.data.len()) {
        return Err("adaptive threshold needs a nonempty, correctly sized grayscale raster".into());
    }
    if img.data.len() > 40_000_000 {
        return Err("adaptive threshold exceeds the 40000000-pixel image budget".into());
    }
    let block = ((img.w.min(img.h) / 20) | 1).max(15);
    let radius = block / 2;
    // The standard default Gaussian sigma is 0.15 * block + 0.35. Build symmetric
    // coefficients from the outside inward, then normalize once in double precision.
    let sigma = (block as f64).mul_add(0.15, 0.35);
    let exponent_scale = -0.125 / (sigma * sigma);
    let mut weights = Vec::with_capacity(radius);
    let mut sum = 0.0;
    for i in 0..radius {
        let x = (2 * i) as f64 + 1.0 - block as f64;
        let weight = ftx_core::det::exp(x * x * exponent_scale);
        sum += weight;
        weights.push(weight);
    }
    let normalization = 1.0 / (2.0 * sum + 1.0);
    let center = normalization as f32;
    let weights: Vec<f32> = weights
        .into_iter()
        .rev()
        .map(|w| (w * normalization) as f32)
        .collect();
    let mut horizontal = vec![0.0f32; img.data.len()];
    for (source, dest) in img
        .data
        .chunks_exact(img.w)
        .zip(horizontal.chunks_exact_mut(img.w))
    {
        for (x, value) in dest.iter_mut().enumerate() {
            let mut mean = f32::from(source[x]) * center;
            for (i, &weight) in weights.iter().enumerate() {
                let distance = i + 1;
                let left = x.saturating_sub(distance);
                let right = (x + distance).min(img.w - 1);
                mean += weight * (f32::from(source[left]) + f32::from(source[right]));
            }
            *value = mean;
        }
    }
    let mut data = vec![0; img.data.len()];
    // Accumulate row-wise so the column pass is contiguous too; no transposed copy is needed.
    let mut row = vec![0.0f32; img.w];
    for y in 0..img.h {
        let start = y * img.w;
        for (x, mean) in row.iter_mut().enumerate() {
            *mean = horizontal[start + x] * center;
        }
        for (i, &weight) in weights.iter().enumerate() {
            let distance = i + 1;
            let top = y.saturating_sub(distance) * img.w;
            let bottom = (y + distance).min(img.h - 1) * img.w;
            for (x, mean) in row.iter_mut().enumerate() {
                *mean += weight * (horizontal[top + x] + horizontal[bottom + x]);
            }
        }
        for (x, mean) in row.iter().enumerate() {
            let local = mean.round_ties_even() as u8;
            data[start + x] = if i16::from(img.data[start + x]) - i16::from(local) > 10 {
                255
            } else {
                0
            };
        }
    }
    Ok(Gray {
        w: img.w,
        h: img.h,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_uniform_and_narrow_rasters_have_no_foreground() {
        for (w, h) in [(1, 1), (1, 31), (29, 1), (7, 5), (97, 113)] {
            for level in [0, 1, 127, 254, 255] {
                let img = Gray::filled(w, h, level);
                assert_eq!(adaptive_gaussian(&img).unwrap(), Gray::filled(w, h, 0));
            }
        }
    }

    #[test]
    fn adaptive_is_deterministic_and_translation_of_levels_preserves_mask() {
        let mut img = Gray::filled(83, 61, 0);
        for (i, p) in img.data.iter_mut().enumerate() {
            *p = ((i * 37 + i / 83 * 13) % 120) as u8;
        }
        let mask = adaptive_gaussian(&img).unwrap();
        assert!(mask.data.contains(&255) && mask.data.contains(&0));
        assert_eq!(mask, adaptive_gaussian(&img).unwrap());
        img.data.iter_mut().for_each(|p| *p += 63);
        assert_eq!(mask, adaptive_gaussian(&img).unwrap());
    }

    #[test]
    fn malformed_adaptive_images_refuse_without_indexing_or_allocation() {
        for (w, h, data) in [(0, 0, vec![]), (2, 2, vec![0; 3]), (usize::MAX, 2, vec![])] {
            assert!(adaptive_gaussian(&Gray { w, h, data }).is_err());
        }
    }
}
