//! Reading the PC2 animation written by [`crate::animation::pc2`] (spec 04 §9.1).
//! The cache stays borrowed; inspecting its tail does not allocate every animation frame.

use ftx_core::{Refusal, RefusalCode, Vec3};

/// Maximum encoded cache size, checked before any frame allocation.
pub const MAX_CACHE_BYTES: usize = 256 * 1024 * 1024;

fn invalid(message: &str) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// A validated, finite, little-endian version-1 point cache, in metres.
#[derive(Debug)]
pub struct PointCache<'a> {
    bytes: &'a [u8],
    points: usize,
    frames: usize,
    start: f64,
    step: f64,
}

impl<'a> PointCache<'a> {
    /// Check the header, exact payload length, numeric domains and every stored coordinate.
    /// A damaged early frame is not hidden merely because the caller asks for the last frame.
    pub fn read(bytes: &'a [u8]) -> Result<Self, Refusal> {
        if bytes.len() > MAX_CACHE_BYTES {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "PC2 exceeds the 256 MiB cache budget",
            ));
        }
        if bytes.len() < 32 || &bytes[..12] != b"POINTCACHE2\0" {
            return Err(invalid("PC2 needs a complete POINTCACHE2 header"));
        }
        let integer = |i| i32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        let scalar = |i| f64::from(f32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()));
        let (points, frames, start, step) = (integer(16), integer(28), scalar(20), scalar(24));
        if integer(12) != 1 || points <= 0 || frames <= 0 {
            return Err(invalid(
                "PC2 requires version 1 and positive point/frame counts",
            ));
        }
        if !start.is_finite() || !step.is_finite() || step <= 0.0 {
            return Err(invalid(
                "PC2 needs a finite start time and positive sample spacing",
            ));
        }
        let (points, frames) = (points as usize, frames as usize);
        let expected = points
            .checked_mul(frames)
            .and_then(|n| n.checked_mul(12))
            .and_then(|n| n.checked_add(32))
            .ok_or_else(|| invalid("PC2 dimensions overflow"))?;
        if expected != bytes.len() {
            return Err(invalid(
                "PC2 payload does not match its declared dimensions",
            ));
        }
        if bytes[32..]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|b| !f32::from_le_bytes(*b).is_finite())
        {
            return Err(Refusal::new(
                RefusalCode::NonFinite,
                "PC2 contains a non-finite coordinate",
            ));
        }
        Ok(Self {
            bytes,
            points,
            frames,
            start,
            step,
        })
    }

    /// Number of points per frame.
    #[must_use]
    pub fn points(&self) -> usize {
        self.points
    }

    /// Number of samples, including board/fold/sew frames when present.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// First sample's time in the cache's frame units (not seconds).
    #[must_use]
    pub fn start(&self) -> f64 {
        self.start
    }

    /// Spacing between samples in the cache's frame units.
    #[must_use]
    pub fn step(&self) -> f64 {
        self.step
    }

    fn point(&self, frame: usize, point: usize) -> Vec3 {
        let offset = 32 + (frame * self.points + point) * 12;
        let xyz = std::array::from_fn(|i| {
            let p = offset + 4 * i;
            f64::from(f32::from_le_bytes(self.bytes[p..p + 4].try_into().unwrap()))
        });
        Vec3::from(xyz)
    }

    /// Read one zero-based sample, without interpolation or implicit clamping.
    pub fn frame(&self, index: usize) -> Result<Vec<Vec3>, Refusal> {
        if index >= self.frames {
            return Err(invalid("PC2 frame index is outside the cache"));
        }
        Ok((0..self.points).map(|i| self.point(index, i)).collect())
    }

    /// Mean of exactly `count` tail samples in time order; callers choose the window explicitly.
    pub fn mean_tail(&self, count: usize) -> Result<Vec<Vec3>, Refusal> {
        if count == 0 || count > self.frames {
            return Err(invalid("PC2 tail length must be in 1..=frames"));
        }
        let mut mean = vec![Vec3::ZERO; self.points];
        for frame in self.frames - count..self.frames {
            for (i, p) in mean.iter_mut().enumerate() {
                *p += self.point(frame, i);
            }
        }
        for p in &mut mean {
            *p = *p / count as f64;
        }
        Ok(mean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::pc2;

    #[test]
    fn independent_wire_values_and_native_writer_round_trip() {
        let mut bytes = b"POINTCACHE2\0".to_vec();
        for word in [1_u32, 2, 2.5_f32.to_bits(), 0.5_f32.to_bits(), 2] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for x in [
            1.0_f32, 2.0, 3.0, -1.0, -2.0, -3.0, 4.0, 5.0, 6.0, 0.0, 0.5, 1.0,
        ] {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        let c = PointCache::read(&bytes).unwrap();
        assert_eq!(
            (c.points(), c.frames(), c.start(), c.step()),
            (2, 2, 2.5, 0.5)
        );
        assert_eq!(
            c.frame(0).unwrap(),
            vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(-1.0, -2.0, -3.0)]
        );
        assert_eq!(
            c.mean_tail(2).unwrap(),
            vec![Vec3::new(2.5, 3.5, 4.5), Vec3::new(-0.5, -0.75, -1.0)]
        );
        let native = pc2(&[c.frame(0).unwrap(), c.frame(1).unwrap()]);
        assert_eq!(
            PointCache::read(&native).unwrap().mean_tail(2).unwrap(),
            c.mean_tail(2).unwrap()
        );
        assert!(c.frame(2).is_err());
        assert!(c.mean_tail(0).is_err());
        assert!(c.mean_tail(3).is_err());
    }

    #[test]
    fn truncated_overlong_and_invalid_headers_never_publish_a_frame() {
        let bytes = pc2(&[vec![Vec3::ZERO]]);
        for end in 0..bytes.len() {
            assert!(PointCache::read(&bytes[..end]).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(PointCache::read(&extra).is_err());
        for (offset, value) in [
            (12, 2_u32),
            (16, 0),
            (16, u32::MAX),
            (28, 0),
            (28, i32::MAX as u32),
            (24, 0),
            (24, (-1.0_f32).to_bits()),
            (20, f32::NAN.to_bits()),
        ] {
            let mut broken = bytes.clone();
            broken[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                PointCache::read(&broken).is_err(),
                "offset {offset}: {value}"
            );
        }
    }

    #[test]
    fn finite_last_frame_cannot_hide_corrupt_earlier_coordinates() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut bytes = pc2(&[vec![Vec3::ZERO], vec![Vec3::new(1.0, 2.0, 3.0)]]);
            bytes[32..36].copy_from_slice(&value.to_le_bytes());
            assert_eq!(
                PointCache::read(&bytes).unwrap_err().code,
                RefusalCode::NonFinite
            );
        }
    }
}
