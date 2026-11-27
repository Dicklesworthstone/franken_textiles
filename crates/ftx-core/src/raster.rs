//! Zero-dependency target raster POD (Seam 2: `ftx-trace` decodes PNG/JPEG/WebP into
//! [`TargetRaster`] without pulling image decoders into `ftx-pattern` or `ftx-drape`).
//!
//! Straight (un-premultiplied) sRGB pixels paired with an explicit `alpha_weight` mask in `[0, 1]`
//! allow inverse dyeing and motif mapping to weight foreground pixels cleanly without alpha-over-white
//! halo artifacts.

use serde::{Deserialize, Serialize};

use crate::receipt::Sha256;
use crate::{Refusal, RefusalCode};

/// Decoded target motif raster in top-to-bottom row-major order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TargetRaster {
    /// Width in pixels (`>= 1`).
    pub width: u32,
    /// Height in pixels (`>= 1`).
    pub height: u32,
    /// Straight (un-premultiplied) sRGB triplets, length `width * height`.
    pub srgb_u8: Vec<[u8; 3]>,
    /// Per-pixel foreground weight in `[0.0, 1.0]` (`0.0` = transparent / ignored in loss,
    /// `1.0` = opaque target), length `width * height`.
    pub alpha_weight: Vec<f32>,
    /// SHA-256 digest of the source image bytes (or canonical pixel buffer for synthetic rasters).
    pub source_sha256: [u8; 32],
}

impl TargetRaster {
    /// Constructs a validated [`TargetRaster`] from straight sRGB pixels and alpha weights.
    ///
    /// # Errors
    /// [`RefusalCode::DegenerateGeometry`] for zero dimensions, [`RefusalCode::InvalidDocument`]
    /// for mismatched buffer lengths, or [`RefusalCode::OutOfRange`] / [`RefusalCode::NonFinite`]
    /// if any alpha weight is non-finite or outside `[0.0, 1.0]`.
    pub fn new(
        width: u32,
        height: u32,
        srgb_u8: Vec<[u8; 3]>,
        mut alpha_weight: Vec<f32>,
        source_sha256: [u8; 32],
    ) -> Result<Self, Refusal> {
        if width == 0 || height == 0 {
            return Err(Refusal::new(
                RefusalCode::DegenerateGeometry,
                "target raster must have nonzero width and height",
            ));
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| {
                Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "target raster pixel count overflow",
                )
            })?;
        if srgb_u8.len() != expected || alpha_weight.len() != expected {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "target raster buffers ({}, {}) do not match {width}x{height} ({expected} pixels)",
                    srgb_u8.len(),
                    alpha_weight.len()
                ),
            ));
        }
        for (idx, a) in alpha_weight.iter_mut().enumerate() {
            if !a.is_finite() {
                return Err(Refusal::new(
                    RefusalCode::NonFinite,
                    format!("target raster alpha_weight[{idx}] is non-finite"),
                ));
            }
            if !(0.0..=1.0).contains(a) {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("target raster alpha_weight[{idx}] = {a} is outside [0, 1]"),
                ));
            }
            if *a == 0.0 {
                *a = 0.0_f32;
            }
        }
        Ok(Self {
            width,
            height,
            srgb_u8,
            alpha_weight,
            source_sha256,
        })
    }

    /// Constructs a synthetic opaque [`TargetRaster`] and hashes its canonical pixel payload.
    ///
    /// # Errors
    /// Same as [`Self::new`].
    pub fn from_opaque_srgb(
        width: u32,
        height: u32,
        srgb_u8: Vec<[u8; 3]>,
    ) -> Result<Self, Refusal> {
        let n = srgb_u8.len();
        let alpha_weight = vec![1.0_f32; n];
        let mut h = Sha256::new();
        h.update(&width.to_le_bytes());
        h.update(&height.to_le_bytes());
        for px in &srgb_u8 {
            h.update(px);
        }
        Self::new(width, height, srgb_u8, alpha_weight, h.finalize())
    }

    /// Pixel count (`width * height`).
    #[must_use]
    pub const fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Pixel at `(x, y)` with `0 <= x < width`, `0 <= y < height`.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<([u8; 3], f32)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = y as usize * self.width as usize + x as usize;
        Some((self.srgb_u8[idx], self.alpha_weight[idx]))
    }

    /// Lowercase hex encoding of [`Self::source_sha256`].
    #[must_use]
    pub fn source_sha256_hex(&self) -> String {
        self.source_sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Fraction of pixels with `alpha_weight > 0.0`.
    #[must_use]
    pub fn foreground_fraction(&self) -> f64 {
        let fg = self.alpha_weight.iter().filter(|&&a| a > 0.0).count();
        fg as f64 / self.pixel_count() as f64
    }

    /// Canonical lowercase-hex SHA-256 over dimensions, sRGB triplets, normalized alpha weights
    /// (`-0.0 -> +0.0`), and [`Self::source_sha256`].
    #[must_use]
    pub fn canonical_sha256(&self) -> String {
        let mut h = Sha256::new();
        h.update(b"ftx-target-raster/1\0");
        h.update(&self.width.to_le_bytes());
        h.update(&self.height.to_le_bytes());
        h.update(&self.source_sha256);
        for px in &self.srgb_u8 {
            h.update(px);
        }
        for &a in &self.alpha_weight {
            let norm = if a == 0.0 { 0.0_f32 } else { a };
            h.update(&norm.to_bits().to_le_bytes());
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_dimensions_and_alpha_bounds() {
        let r = TargetRaster::from_opaque_srgb(2, 2, vec![[255, 0, 0]; 4]).unwrap();
        assert_eq!(r.pixel_count(), 4);
        assert_eq!(r.pixel(1, 1), Some(([255, 0, 0], 1.0)));
        assert_eq!(r.pixel(2, 0), None);
        assert!((r.foreground_fraction() - 1.0).abs() < 1e-12);
        assert_eq!(r.canonical_sha256().len(), 64);

        let neg_zero =
            TargetRaster::new(1, 1, vec![[10, 20, 30]], vec![-0.0_f32], [7; 32]).unwrap();
        let pos_zero = TargetRaster::new(1, 1, vec![[10, 20, 30]], vec![0.0_f32], [7; 32]).unwrap();
        assert_eq!(neg_zero.alpha_weight[0].to_bits(), 0.0_f32.to_bits());
        assert_eq!(neg_zero.canonical_sha256(), pos_zero.canonical_sha256());

        let err = TargetRaster::new(1, 1, vec![[0, 0, 0]], vec![1.5], [0; 32]).unwrap_err();
        assert_eq!(err.code, RefusalCode::OutOfRange);

        let err = TargetRaster::new(1, 1, vec![[0, 0, 0]], vec![f32::NAN], [0; 32]).unwrap_err();
        assert_eq!(err.code, RefusalCode::NonFinite);
    }
}
