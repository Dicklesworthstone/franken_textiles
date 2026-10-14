//! JPEG photo evidence: one complete frame, one unambiguous EXIF orientation, no pixel repair.
//! Unlike the trace decoder this boundary applies IFD0 Orientation (TIFF tag 274). Integer
//! transposes follow Pillow ImageOps.exif_transpose; neither interpolation nor scaling occurs.

use ftx_core::{Refusal, RefusalCode};

use super::{Bgr, decode_impl};

/// A decoded photograph in the full-resolution, EXIF-oriented click/detection frame.
#[derive(Clone, Debug)]
pub struct OrientedJpeg {
    /// Opaque BGR pixels, with EXIF orientation already applied exactly once.
    pub image: Bgr,
    /// SOF dimensions before orientation, not the EXIF thumbnail dimensions.
    pub stored_size_px: [usize; 2],
    /// Original IFD0 orientation; absent metadata uses identity without inventing a tag.
    pub exif_orientation: Option<u16>,
    /// Exact homogeneous map from stored pixel centres to oriented pixel centres (integers).
    /// Pixel (0,0) is the centre of the top-left pixel, so flips use width/height minus one.
    pub stored_to_oriented_px: [[i64; 3]; 3],
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
        .repair("export one complete opaque, oriented RGB PNG from the original photograph")
}

struct Header {
    size: [usize; 2],
    orientation: Option<u16>,
}

// Parse only IFD0: thumbnail/EXIF/GPS/MakerNote IFDs cannot override the primary image.
// Bounds cover the complete directory before reading any entry. No external offset is followed.
fn orientation(tiff: &[u8]) -> Result<Option<u16>, Refusal> {
    let bad = || invalid("JPEG photo has malformed EXIF orientation metadata");
    let little = match tiff.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Err(bad()),
    };
    let short = |at: usize| -> Result<u16, Refusal> {
        let bytes: [u8; 2] = tiff.get(at..at + 2).ok_or_else(bad)?.try_into().unwrap();
        Ok(if little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    };
    let long = |at: usize| -> Result<u32, Refusal> {
        let bytes: [u8; 4] = tiff.get(at..at + 4).ok_or_else(bad)?.try_into().unwrap();
        Ok(if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    if short(2)? != 42 {
        return Err(bad());
    }
    let offset = long(4)? as usize;
    // APP1 is at most 65533 bytes. This bound also protects usize addition on wasm32.
    if offset < 8 || offset > tiff.len().saturating_sub(2) {
        return Err(bad());
    }
    let count = usize::from(short(offset)?);
    let end = offset + 2 + count * 12 + 4;
    if end > tiff.len() {
        return Err(bad());
    }
    let mut result = None;
    for i in 0..count {
        let entry = offset + 2 + i * 12;
        if short(entry)? == 274 {
            if result.is_some() || short(entry + 2)? != 3 || long(entry + 4)? != 1 {
                return Err(bad());
            }
            let value = short(entry + 8)?;
            if !(1..=8).contains(&value) {
                return Err(bad());
            }
            result = Some(value);
        }
    }
    Ok(result)
}

// Walk the complete JPEG envelope, including all progressive scans, before decoding. Entropy
// bytes are not interpreted here; the existing decoder checks them without its repair path.
fn header(bytes: &[u8], max_pixels: usize) -> Result<Header, Refusal> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "JPEG photo exceeds 64 MiB",
        ));
    }
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(invalid("not a JPEG photograph"));
    }
    let mut pos = 2;
    let (mut size, mut exif, mut value) = (None, false, None);
    let (mut entropy, mut saw_scan) = (false, false);
    while pos < bytes.len() {
        if entropy && bytes[pos] != 0xff {
            pos += 1;
            continue;
        }
        if bytes[pos] != 0xff {
            return Err(invalid("JPEG photo has garbage between markers"));
        }
        while bytes.get(pos) == Some(&0xff) {
            pos += 1;
        }
        let marker = *bytes
            .get(pos)
            .ok_or_else(|| invalid("truncated JPEG marker"))?;
        pos += 1;
        if entropy && (marker == 0 || (0xd0..=0xd7).contains(&marker)) {
            continue;
        }
        entropy = false;
        if marker == 0xd9 {
            if !saw_scan || pos != bytes.len() {
                return Err(invalid(
                    "JPEG photo must contain exactly one complete image and no trailing payload",
                ));
            }
            return Ok(Header {
                size: size.ok_or_else(|| invalid("JPEG photo has no frame"))?,
                orientation: value,
            });
        }
        if marker == 0 || marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            return Err(invalid("unexpected standalone marker in JPEG photo"));
        }
        let length = bytes
            .get(pos..pos + 2)
            .ok_or_else(|| invalid("truncated JPEG segment"))?;
        let length = usize::from(u16::from_be_bytes(length.try_into().unwrap()));
        if length < 2 || pos + length > bytes.len() {
            return Err(invalid("invalid JPEG segment length"));
        }
        let data = &bytes[pos + 2..pos + length];
        pos += length;
        match marker {
            0xc0..=0xc2 => {
                if size.is_some() || data.len() < 6 || data[0] != 8 || !matches!(data[5], 1 | 3) {
                    return Err(invalid(
                        "JPEG photo requires one 8-bit grayscale or RGB/YCbCr frame",
                    ));
                }
                let w = usize::from(u16::from_be_bytes([data[3], data[4]]));
                let h = usize::from(u16::from_be_bytes([data[1], data[2]]));
                if w < 2 || h < 2 {
                    return Err(invalid("JPEG photo dimensions must be at least 2 by 2"));
                }
                if w.checked_mul(h).is_none_or(|n| n > max_pixels) {
                    return Err(Refusal::new(
                        RefusalCode::BudgetExceeded,
                        format!("JPEG photo exceeds {max_pixels} pixels"),
                    ));
                }
                size = Some([w, h]);
            }
            0xe1 if data.starts_with(b"Exif") => {
                if exif || saw_scan || !data.starts_with(b"Exif\0\0") {
                    return Err(invalid(
                        "JPEG photo has duplicate, late or malformed EXIF metadata",
                    ));
                }
                value = orientation(&data[6..])?;
                exif = true;
            }
            0xe1 if data
                .windows(b"Orientation".len())
                .any(|w| w == b"Orientation") =>
            {
                // Do not choose between EXIF and a potentially contradictory XMP orientation.
                return Err(invalid(
                    "JPEG photo has alternate orientation metadata; normalize it before measuring",
                ));
            }
            0xe2 if data.starts_with(b"MPF\0") => {
                return Err(invalid(
                    "multi-picture JPEG is not one identified photo; export the selected frame",
                ));
            }
            0xda => {
                if size.is_none() {
                    return Err(invalid("JPEG scan precedes its frame"));
                }
                entropy = true;
                saw_scan = true;
            }
            _ => {}
        }
    }
    Err(invalid("JPEG photo is missing its end-of-image marker"))
}

fn transform(size: [usize; 2], orientation: u16) -> ([[i64; 3]; 3], [usize; 2]) {
    let [w, h] = size.map(|n| n as i64 - 1);
    let rows = match orientation {
        2 => [[-1, 0, w], [0, 1, 0]],
        3 => [[-1, 0, w], [0, -1, h]],
        4 => [[1, 0, 0], [0, -1, h]],
        5 => [[0, 1, 0], [1, 0, 0]],
        6 => [[0, -1, h], [1, 0, 0]],
        7 => [[0, -1, h], [-1, 0, w]],
        8 => [[0, 1, 0], [-1, 0, w]],
        _ => [[1, 0, 0], [0, 1, 0]],
    };
    let out = if orientation >= 5 {
        [size[1], size[0]]
    } else {
        size
    };
    ([rows[0], rows[1], [0, 0, 1]], out)
}

/// Decode one complete JPEG and apply its EXIF orientation without resampling. The ordinary
/// [`super::decode`] remains byte-compatible with tracing's permissive, unoriented boundary.
/// Photo input refuses known entropy repair, incomplete progressive scans, ambiguous orientation,
/// multiple images and trailing payloads. EXIF data other than orientation is not retained.
///
/// # Errors
/// Malformed/unsupported images or metadata, decoder repair, or encoded/decoded budget excess.
pub fn decode_photo(bytes: &[u8], max_pixels: usize) -> Result<OrientedJpeg, Refusal> {
    let header = header(bytes, max_pixels)?;
    let mut image = decode_impl(bytes, max_pixels, true).map_err(invalid)?;
    let (matrix, [w, h]) = transform(header.size, header.orientation.unwrap_or(1));
    if header.orientation.is_some_and(|v| v != 1) {
        let mut data = vec![0; image.data.len()];
        for y in 0..image.h {
            for x in 0..image.w {
                let map = |r: usize| {
                    (matrix[r][0] * x as i64 + matrix[r][1] * y as i64 + matrix[r][2]) as usize
                };
                let dest = 3 * (map(1) * w + map(0));
                let source = 3 * (y * image.w + x);
                data[dest..dest + 3].copy_from_slice(&image.data[source..source + 3]);
            }
        }
        image = Bgr { w, h, data };
    }
    Ok(OrientedJpeg {
        image,
        stored_size_px: header.size,
        exif_orientation: header.orientation,
        stored_to_oriented_px: matrix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exif(value: u16, little: bool) -> Vec<u8> {
        let short = |v: u16| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let long = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        [
            if little { b"II" } else { b"MM" }.to_vec(),
            short(42).to_vec(),
            long(8).to_vec(),
            short(1).to_vec(),
            short(274).to_vec(),
            short(3).to_vec(),
            long(1).to_vec(),
            short(value).to_vec(),
            short(0).to_vec(),
            long(0).to_vec(),
        ]
        .concat()
    }

    #[test]
    fn exif_primary_orientation_is_endian_aware_bounded_and_unambiguous() {
        for little in [false, true] {
            for v in 1..=8 {
                let bytes = exif(v, little);
                assert_eq!(orientation(&bytes).unwrap(), Some(v));
                for end in 0..bytes.len() {
                    assert!(orientation(&bytes[..end]).is_err());
                }
            }
            for v in [0, 9, u16::MAX] {
                assert!(orientation(&exif(v, little)).is_err());
            }
        }
        let mut bytes = exif(6, true);
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(orientation(&bytes).is_err());
        let mut bytes = exif(6, true);
        bytes[12] = 4; // LONG rather than SHORT is not a valid orientation representation.
        assert!(orientation(&bytes).is_err());
        let mut bytes = exif(6, true);
        bytes[14] = 2;
        assert!(orientation(&bytes).is_err());
        let mut bytes = exif(6, true);
        bytes[8] = 2;
        let second = bytes[10..22].to_vec();
        bytes.splice(22..22, second);
        assert!(orientation(&bytes).is_err());
        let mut bytes = exif(6, true);
        bytes[10..12].copy_from_slice(&256u16.to_le_bytes()); // no Orientation tag
        assert_eq!(orientation(&bytes).unwrap(), None);
    }

    #[test]
    fn all_eight_transforms_pin_asymmetric_pixels_without_half_pixel_shifts() {
        // A B C / D E F. Expected oriented pixel order, independently written out.
        let expected = [
            "ABCDEF", "CBAFED", "FEDCBA", "DEFABC", "ADBECF", "DAEBFC", "FCEBDA", "CFBEAD",
        ];
        for (index, want) in expected.iter().enumerate() {
            let (m, [w, h]) = transform([3, 2], index as u16 + 1);
            let mut got = vec![0; w * h];
            for (i, v) in b"ABCDEF".iter().enumerate() {
                let x = (i % 3) as i64;
                let y = (i / 3) as i64;
                let u = (m[0][0] * x + m[0][1] * y + m[0][2]) as usize;
                let vpos = (m[1][0] * x + m[1][1] * y + m[1][2]) as usize;
                got[vpos * w + u] = *v;
            }
            assert_eq!(got, want.as_bytes());
        }
    }

    #[test]
    fn whole_frame_preflight_refuses_ambiguous_metadata_and_preallocates_nothing() {
        // Envelope-only fixture: the real decoder independently requires valid tables/entropy.
        let segment = |marker: u8, body: &[u8]| {
            [
                vec![0xff, marker],
                ((body.len() + 2) as u16).to_be_bytes().to_vec(),
                body.to_vec(),
            ]
            .concat()
        };
        let sof = segment(0xc0, &[8, 0, 9, 0, 7, 1, 1, 0x11, 0]);
        let sos = segment(0xda, &[1, 1, 0, 0, 63, 0]);
        let exif = segment(0xe1, &[b"Exif\0\0".to_vec(), exif(6, false)].concat());
        let jpeg = |metadata: &[u8]| {
            [
                vec![0xff, 0xd8],
                metadata.to_vec(),
                sof.clone(),
                sos.clone(),
                vec![0xff, 0xd9],
            ]
            .concat()
        };
        let bytes = jpeg(&exif);
        let info = header(&bytes, 63).unwrap();
        assert_eq!(info.size, [7, 9]);
        assert_eq!(info.orientation, Some(6));
        assert_eq!(
            header(&bytes, 62).err().unwrap().code,
            RefusalCode::BudgetExceeded
        );
        for end in 0..bytes.len() {
            assert!(header(&bytes[..end], 63).is_err());
        }
        for metadata in [
            [exif.clone(), exif.clone()].concat(),
            segment(0xe1, b"Exif"),
            segment(
                0xe1,
                b"http://ns.adobe.com/xap/1.0/\0<tiff:Orientation>1</tiff:Orientation>",
            ),
            segment(0xe2, b"MPF\0anything"),
        ] {
            assert!(header(&jpeg(&metadata), 63).is_err());
        }
        for tail in [vec![0], vec![0xff, 0xd8, 0xff, 0xd9]] {
            assert!(header(&[bytes.clone(), tail].concat(), 63).is_err());
        }
        let late = [vec![0xff, 0xd8], sof, sos, exif, vec![0xff, 0xd9]].concat();
        assert!(header(&late, 63).is_err());
    }
}
