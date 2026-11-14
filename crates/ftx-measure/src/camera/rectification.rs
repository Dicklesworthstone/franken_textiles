//! A supported pinhole image from an already oriented Brown-camera raster.
//!
//! The integer virtual grid keeps the original focal lengths and image dimensions. Its largest
//! supported rectangle becomes the output; only the principal point changes with the crop.
//! Each output sample maps forward through Brown distortion into an in-image bilinear footprint.
//! Cropping or interpolation does not calibrate the camera or establish measurement accuracy.

use ftx_core::{Refusal, RefusalCode, Vec2};

use super::{Distortion, Intrinsics};
use crate::invalid;

/// Maximum number of original pixels admitted before allocating the crop-search workspace.
pub const MAX_RECTIFICATION_PIXELS: usize = 16_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Crop {
    origin: [u32; 2],
    size: [u32; 2],
}

impl Crop {
    fn better_than(self, other: Self) -> bool {
        use std::cmp::Reverse;

        let key = |crop: Self| {
            (
                Reverse(u64::from(crop.size[0]) * u64::from(crop.size[1])),
                crop.origin[1],
                crop.origin[0],
                Reverse(crop.size[0]),
                Reverse(crop.size[1]),
            )
        };
        key(self) < key(other)
    }
}

fn in_image(pixel: Vec2, size: [u32; 2]) -> bool {
    pixel.is_finite()
        && pixel.x >= 0.0
        && pixel.y >= 0.0
        && pixel.x <= f64::from(size[0] - 1)
        && pixel.y <= f64::from(size[1] - 1)
}

// The calibration has already passed Intrinsics::validate. No inverse solve, repeated domain
// certification or allocated error is needed for each of the bounded raster samples.
fn source_pixel(
    source: Intrinsics,
    size: [u32; 2],
    identity: bool,
    virtual_pixel: Vec2,
) -> Option<Vec2> {
    let normalized = Vec2::new(
        (virtual_pixel.x - source.principal_px.x) / source.focal_px.x,
        (virtual_pixel.y - source.principal_px.y) / source.focal_px.y,
    );
    if !source.distortion.inside(normalized) {
        return None;
    }
    let pixel = if identity {
        virtual_pixel
    } else {
        let distorted = source.distortion.evaluate(normalized).0;
        Vec2::new(
            source.focal_px.x * distorted.x + source.principal_px.x,
            source.focal_px.y * distorted.y + source.principal_px.y,
        )
    };
    in_image(pixel, size).then_some(pixel)
}

// Maximal rectangles ending at this row, using a monotone histogram stack. Equal heights
// replace the previous index while preserving the earlier lower-height boundary. The sentinel
// evaluates rectangles reaching the right edge. Each column is pushed/popped at most once.
fn consider_row(heights: &[u32], row: u32, stack: &mut Vec<u32>, best: &mut Option<Crop>) {
    stack.clear();
    for right in 0..=heights.len() {
        let current = heights.get(right).copied().unwrap_or(0);
        while let Some(&index) = stack.last() {
            let height = heights[index as usize];
            if height < current {
                break;
            }
            stack.pop();
            let left = stack.last().map_or(0, |&index| index as usize + 1);
            let width = right - left;
            if height >= 2 && width >= 2 {
                let candidate = Crop {
                    origin: [left as u32, row + 1 - height],
                    size: [width as u32, height],
                };
                if best.is_none_or(|old| candidate.better_than(old)) {
                    *best = Some(candidate);
                }
            }
        }
        if right < heights.len() {
            stack.push(right as u32);
        }
    }
}

/// An immutable crop and calibrated mapping for one original oriented image geometry.
#[derive(Clone, Copy, Debug)]
pub struct Rectification {
    source: Intrinsics,
    source_size: [u32; 2],
    crop: Crop,
    intrinsics: Intrinsics,
    identity: bool,
}

impl Rectification {
    /// Select the largest supported rectangle of the original-sized virtual pinhole grid.
    /// Every retained integer sample lies in the certified lens disk and has an in-image
    /// bilinear footprint. Exact boundary pixels need only their contributing source samples.
    /// Ties prefer the topmost, then leftmost, then widest rectangle. Output is at least 2 by 2.
    /// Search takes O(width * height) work and O(width) temporary storage, without a raster copy.
    ///
    /// # Errors
    /// Invalid calibration, dimensions below two, more than 16 million original pixels, or no
    /// supported rectangle. There is no padding, lens extrapolation or coordinate clamping.
    pub fn new(source: Intrinsics, image_size_px: [u32; 2]) -> Result<Self, Refusal> {
        source.validate()?;
        if image_size_px.iter().any(|&size| size < 2) {
            return Err(invalid(
                "rectification needs image dimensions of at least two",
            ));
        }
        let pixels = (image_size_px[0] as usize).checked_mul(image_size_px[1] as usize);
        if pixels.is_none_or(|count| count > MAX_RECTIFICATION_PIXELS) {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "camera rectification exceeds 16000000 original pixels",
            ));
        }
        let identity =
            source.distortion.radial == [0.0; 3] && source.distortion.tangential == [0.0; 2];
        let mut heights = vec![0_u32; image_size_px[0] as usize];
        let mut stack = Vec::with_capacity(heights.len());
        let mut best = None;
        for y in 0..image_size_px[1] {
            for (x, height) in heights.iter_mut().enumerate() {
                let virtual_pixel = Vec2::new(x as f64, f64::from(y));
                *height = if source_pixel(source, image_size_px, identity, virtual_pixel).is_some()
                {
                    *height + 1
                } else {
                    0
                };
            }
            consider_row(&heights, y, &mut stack, &mut best);
        }
        let crop = best.ok_or_else(|| {
            Refusal::new(
                RefusalCode::DegenerateGeometry,
                "camera rectification has no supported image rectangle of at least 2 by 2 pixels",
            )
        })?;
        let intrinsics = Intrinsics {
            focal_px: source.focal_px,
            principal_px: source.principal_px
                - Vec2::new(f64::from(crop.origin[0]), f64::from(crop.origin[1])),
            distortion: Distortion {
                radial: [0.0; 3],
                tangential: [0.0; 2],
                max_radius: source.distortion.max_radius,
            },
        };
        Ok(Self {
            source,
            source_size: image_size_px,
            crop,
            intrinsics,
            identity,
        })
    }

    /// Original oriented image width and height, before any crop.
    #[must_use]
    pub fn source_image_size_px(&self) -> [u32; 2] {
        self.source_size
    }

    /// Supported output width and height, with unchanged pixel scale.
    #[must_use]
    pub fn image_size_px(&self) -> [u32; 2] {
        self.crop.size
    }

    /// Top-left output pixel in the original-sized virtual pinhole grid.
    #[must_use]
    pub fn crop_origin_px(&self) -> [u32; 2] {
        self.crop.origin
    }

    /// Same focal lengths, crop-shifted principal point, and zero Brown coefficients.
    /// The original certified normalized radius is retained.
    #[must_use]
    pub fn intrinsics(&self) -> Intrinsics {
        self.intrinsics
    }

    /// Convert an original observed point into the cropped pinhole image.
    ///
    /// # Errors
    /// The point is outside the original image, cannot be inverted within the certified lens
    /// domain, or falls outside the selected crop. No discarded point is moved onto its edge.
    pub fn source_to_rectified(&self, pixel: Vec2) -> Result<Vec2, Refusal> {
        if !in_image(pixel, self.source_size) {
            return Err(invalid("source point is outside the original image"));
        }
        let virtual_pixel = self.source.undistort_pixel_with_jacobian(pixel)?.0;
        let rectified = virtual_pixel
            - Vec2::new(
                f64::from(self.crop.origin[0]),
                f64::from(self.crop.origin[1]),
            );
        if !in_image(rectified, self.crop.size) {
            return Err(invalid("source point lies outside the rectified crop"));
        }
        Ok(rectified)
    }

    /// Map a cropped pinhole point to its original Brown image coordinate.
    ///
    /// # Errors
    /// A non-finite, out-of-crop or unsupported point refuses. Integer raster support does not
    /// silently authorize lens extrapolation at arbitrary continuous coordinates.
    pub fn rectified_to_source(&self, pixel: Vec2) -> Result<Vec2, Refusal> {
        if !in_image(pixel, self.crop.size) {
            return Err(invalid("point is outside the rectified crop"));
        }
        let virtual_pixel = pixel
            + Vec2::new(
                f64::from(self.crop.origin[0]),
                f64::from(self.crop.origin[1]),
            );
        source_pixel(self.source, self.source_size, self.identity, virtual_pixel)
            .ok_or_else(|| invalid("rectified point has no supported source-image footprint"))
    }

    /// Resample tightly packed, row-major BGR bytes by bilinear interpolation, rounding each
    /// channel to its nearest byte. Identity lenses copy the retained rows exactly.
    ///
    /// # Errors
    /// The source length must equal original width * height * 3. No resized, padded, rotated or
    /// differently packed input is inferred; an unsupported raster sample refuses the operation.
    pub fn rectify_bgr(&self, pixels: &[u8]) -> Result<Vec<u8>, Refusal> {
        let [source_width, source_height] = self.source_size.map(|v| v as usize);
        if pixels.len() != source_width * source_height * 3 {
            return Err(invalid(
                "rectification needs exact original width * height * 3 BGR bytes",
            ));
        }
        let [width, height] = self.crop.size.map(|v| v as usize);
        let [left, top] = self.crop.origin.map(|v| v as usize);
        let mut output = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            if self.identity {
                let start = ((top + y) * source_width + left) * 3;
                output.extend_from_slice(&pixels[start..start + width * 3]);
                continue;
            }
            for x in 0..width {
                let virtual_pixel = Vec2::new((left + x) as f64, (top + y) as f64);
                let point = source_pixel(self.source, self.source_size, false, virtual_pixel)
                    .ok_or_else(|| invalid("rectification lost its admitted raster support"))?;
                let x0 = point.x.floor() as usize;
                let x1 = point.x.ceil() as usize;
                let y0 = point.y.floor() as usize;
                let y1 = point.y.ceil() as usize;
                let fx = point.x - x0 as f64;
                let fy = point.y - y0 as f64;
                for channel in 0..3 {
                    let value = |x, y| f64::from(pixels[(y * source_width + x) * 3 + channel]);
                    let upper = (1.0 - fx) * value(x0, y0) + fx * value(x1, y0);
                    let lower = (1.0 - fx) * value(x0, y1) + fx * value(x1, y1);
                    output.push(((1.0 - fy) * upper + fy * lower).round() as u8);
                }
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lens() -> Intrinsics {
        Intrinsics {
            focal_px: Vec2::new(15.0, 14.0),
            principal_px: Vec2::new(8.2, 5.8),
            distortion: Distortion {
                radial: [0.08, -0.01, 0.003],
                tangential: [0.002, -0.0015],
                max_radius: 1.0,
            },
        }
    }

    #[test]
    fn identity_preserves_every_byte_including_image_boundaries() {
        let source = Intrinsics {
            distortion: Distortion::default(),
            ..lens()
        };
        let rectification = Rectification::new(source, [17, 13]).unwrap();
        assert_eq!(rectification.source_image_size_px(), [17, 13]);
        assert_eq!(rectification.image_size_px(), [17, 13]);
        assert_eq!(rectification.crop_origin_px(), [0, 0]);
        let pixels: Vec<_> = (0..17 * 13 * 3).map(|i| (i % 251) as u8).collect();
        assert_eq!(rectification.rectify_bgr(&pixels).unwrap(), pixels);
        for pixel in [Vec2::ZERO, Vec2::new(16.0, 12.0), Vec2::new(8.125, 6.75)] {
            assert_eq!(rectification.source_to_rectified(pixel).unwrap(), pixel);
            assert_eq!(rectification.rectified_to_source(pixel).unwrap(), pixel);
        }
    }

    #[test]
    fn positive_radial_lens_has_the_independent_known_crop() {
        let source = Intrinsics {
            focal_px: Vec2::new(4.0, 4.0),
            principal_px: Vec2::new(4.0, 4.0),
            distortion: Distortion {
                radial: [0.2, 0.0, 0.0],
                tangential: [0.0; 2],
                max_radius: 1.5,
            },
        };
        let rectification = Rectification::new(source, [9, 9]).unwrap();
        // x or y at 0/8 maps outside the source even on its central axis. Every point in
        // [1,7]² is supported: its furthest corner maps to 0.325/7.675, inside the source.
        assert_eq!(rectification.crop_origin_px(), [1, 1]);
        assert_eq!(rectification.image_size_px(), [7, 7]);
        let derived = rectification.intrinsics();
        assert_eq!(derived.focal_px, source.focal_px);
        assert_eq!(derived.principal_px, Vec2::new(3.0, 3.0));
        assert_eq!(derived.distortion.radial, [0.0; 3]);
        assert_eq!(derived.distortion.tangential, [0.0; 2]);
        assert_eq!(derived.distortion.max_radius, 1.5);
    }

    #[test]
    fn polynomial_coordinates_and_bilinear_ramps_match_independent_values() {
        let source = lens();
        let rectification = Rectification::new(source, [17, 13]).unwrap();
        let mut pixels = Vec::new();
        for y in 0..13 {
            for x in 0..17 {
                pixels.extend([
                    (x * y) as u8,
                    (3 * x + 2 * y + 4) as u8,
                    (255 - 2 * x - 3 * y) as u8,
                ]);
            }
        }
        let output = rectification.rectify_bgr(&pixels).unwrap();
        let [width, height] = rectification.image_size_px();
        let [left, top] = rectification.crop_origin_px();
        for v in 0..height {
            for u in 0..width {
                let x = (f64::from(u + left) - 8.2) / 15.0;
                let y = (f64::from(v + top) - 5.8) / 14.0;
                let r2 = x * x + y * y;
                let radial = 1.0 + 0.08 * r2 - 0.01 * r2 * r2 + 0.003 * r2 * r2 * r2;
                let sx = 15.0 * (x * radial + 0.004 * x * y - 0.0015 * (r2 + 2.0 * x * x)) + 8.2;
                let sy = 14.0 * (y * radial + 0.002 * (r2 + 2.0 * y * y) - 0.003 * x * y) + 5.8;
                let pixel = Vec2::new(f64::from(u), f64::from(v));
                let mapped = rectification.rectified_to_source(pixel).unwrap();
                assert!((mapped - Vec2::new(sx, sy)).norm() < 1e-12);
                let restored = rectification.source_to_rectified(mapped);
                if u > 0 && u + 1 < width && v > 0 && v + 1 < height {
                    assert!((restored.unwrap() - pixel).norm() < 1e-9);
                }
                // Bilinear interpolation reproduces x*y as well as affine ramps. Quantizing
                // the independent continuous values may differ by at most half a byte.
                let expected = [
                    sx * sy,
                    3.0 * sx + 2.0 * sy + 4.0,
                    255.0 - 2.0 * sx - 3.0 * sy,
                ];
                let index = ((v * width + u) * 3) as usize;
                for channel in 0..3 {
                    assert!(
                        (f64::from(output[index + channel]) - expected[channel]).abs()
                            <= 0.500000001
                    );
                }
            }
        }
    }

    #[test]
    fn crop_search_matches_exhaustive_rectangles_and_stable_ties() {
        // Every 4-by-3 binary support mask, including holes/disconnected components. This
        // exhaustive rectangle oracle does not share the histogram's search strategy.
        for mask in 0_u32..(1_u32 << 12) {
            let supported = |x: u32, y: u32| mask & (1_u32 << (y * 4 + x)) != 0;
            let mut expected = None;
            for top in 0..2 {
                for left in 0..3 {
                    for bottom in top + 2..=3 {
                        for right in left + 2..=4 {
                            if (top..bottom).all(|y| (left..right).all(|x| supported(x, y))) {
                                let candidate = Crop {
                                    origin: [left, top],
                                    size: [right - left, bottom - top],
                                };
                                if expected.is_none_or(|old| candidate.better_than(old)) {
                                    expected = Some(candidate);
                                }
                            }
                        }
                    }
                }
            }
            let mut actual = None;
            let mut heights = [0; 4];
            let mut stack = Vec::new();
            for y in 0..3 {
                for x in 0..4 {
                    heights[x] = if supported(x as u32, y) {
                        heights[x] + 1
                    } else {
                        0
                    };
                }
                consider_row(&heights, y, &mut stack, &mut actual);
            }
            assert_eq!(actual, expected, "support mask {mask:012b}");
        }
    }

    #[test]
    fn certified_disk_can_crop_an_identity_image_without_padding() {
        let source = Intrinsics {
            focal_px: Vec2::new(1.0, 1.0),
            principal_px: Vec2::new(1.5, 1.5),
            distortion: Distortion {
                max_radius: 1.0,
                ..Distortion::default()
            },
        };
        let rectification = Rectification::new(source, [4, 4]).unwrap();
        assert_eq!(rectification.crop_origin_px(), [1, 1]);
        assert_eq!(rectification.image_size_px(), [2, 2]);
        let pixels: Vec<_> = (0..48).collect();
        let expected: Vec<_> = pixels[15..21]
            .iter()
            .chain(&pixels[27..33])
            .copied()
            .collect();
        assert_eq!(rectification.rectify_bgr(&pixels).unwrap(), expected);
        assert!(rectification.source_to_rectified(Vec2::ZERO).is_err());
        assert!(
            rectification
                .rectified_to_source(Vec2::new(1.01, 1.0))
                .is_err()
        );
        assert!(rectification.rectify_bgr(&pixels[..47]).is_err());
        assert!(rectification.rectify_bgr(&[0; 49]).is_err());
    }

    #[test]
    fn invalid_calibrations_budgets_and_absent_support_refuse() {
        for size in [[0, 2], [2, 1], [1, 10]] {
            assert!(Rectification::new(lens(), size).is_err());
        }
        for size in [[u32::MAX, u32::MAX], [4_000_001, 4]] {
            assert_eq!(
                Rectification::new(lens(), size).unwrap_err().code,
                RefusalCode::BudgetExceeded
            );
        }
        let mut source = lens();
        source.focal_px.x = 0.0;
        assert!(Rectification::new(source, [17, 13]).is_err());
        let mut source = lens();
        source.distortion.radial = [-1.0, 0.0, 0.0];
        assert!(Rectification::new(source, [17, 13]).is_err());
        let source = Intrinsics {
            focal_px: Vec2::new(1.0, 1.0),
            principal_px: Vec2::new(0.5, 0.5),
            distortion: Distortion {
                max_radius: 0.1,
                ..Distortion::default()
            },
        };
        assert_eq!(
            Rectification::new(source, [4, 4]).unwrap_err().code,
            RefusalCode::DegenerateGeometry
        );
        let rectification = Rectification::new(lens(), [17, 13]).unwrap();
        assert!(
            rectification
                .source_to_rectified(Vec2::new(f64::NAN, 0.0))
                .is_err()
        );
        assert!(
            rectification
                .source_to_rectified(Vec2::new(-0.01, 0.0))
                .is_err()
        );
        assert!(
            rectification
                .rectified_to_source(Vec2::new(f64::INFINITY, 0.0))
                .is_err()
        );
    }
}
