//! Images and loading: 8-bit gray and BGR rasters, and PNG decoding with the reference's `load_image`
//! semantics (Pillow 12.3: palette, gray+alpha and RGBA images composited on white with Pillow's fixed-point
//! `alpha_composite`, 16-bit samples reduced as Pillow reduces them, the result as OpenCV's BGR).
//!
//! PNG only for now: JPEG and WebP decoders that match libjpeg-turbo and libwebp bit for bit are not here, so
//! those files are refused with a repair (convert to PNG).

use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;

/// A single-channel 8-bit image, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gray {
    /// Width (columns).
    pub w: usize,
    /// Height (rows).
    pub h: usize,
    /// `w·h` samples, row by row.
    pub data: Vec<u8>,
}

impl Gray {
    /// An image of one value.
    #[must_use]
    pub fn filled(w: usize, h: usize, v: u8) -> Self {
        Self {
            w,
            h,
            data: vec![v; w * h],
        }
    }

    /// The sample at column `x`, row `y`.
    #[must_use]
    pub fn at(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.w + x]
    }
}

/// A three-channel 8-bit image in OpenCV's BGR order, row-major and interleaved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bgr {
    /// Width (columns).
    pub w: usize,
    /// Height (rows).
    pub h: usize,
    /// `3·w·h` samples: B, G, R per pixel, row by row.
    pub data: Vec<u8>,
}

impl Bgr {
    /// The pixel at column `x`, row `y` as `[b, g, r]`.
    #[must_use]
    pub fn at(&self, x: usize, y: usize) -> [u8; 3] {
        let i = 3 * (y * self.w + x);
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }
}

/// An 8-bit grayscale PNG of `img` (no filtering, zlib level 6): the pixels `cv2.imwrite` would store, not
/// its bytes.
#[must_use]
pub fn encode_png_gray(img: &Gray) -> Vec<u8> {
    let mut raw = Vec::with_capacity((img.w + 1) * img.h);
    for row in img.data.chunks(img.w.max(1)) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
    };
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&u32::try_from(img.w).unwrap_or(0).to_be_bytes());
    ihdr.extend_from_slice(&u32::try_from(img.h).unwrap_or(0).to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    );
    chunk(b"IEND", &[]);
    out
}

/// CRC-32 (IEEE) of a chunk's type and data.
fn crc32(parts: &[&[u8]]) -> u32 {
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut c = 0xffff_ffffu32;
    for p in parts {
        for &b in *p {
            c = table[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
        }
    }
    c ^ 0xffff_ffff
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Reverses the scanline filters of one (sub)image of `rows` lines of `stride` bytes; `bpp` is the filter
/// unit (bytes per complete pixel, at least 1).
fn unfilter(data: &[u8], rows: usize, stride: usize, bpp: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; rows * stride];
    let mut pos = 0;
    for y in 0..rows {
        let kind = *data.get(pos).ok_or("truncated image data")?;
        let line = data
            .get(pos + 1..pos + 1 + stride)
            .ok_or("truncated image data")?;
        pos += 1 + stride;
        let (prev, cur) = out.split_at_mut(y * stride);
        let up = if y == 0 {
            None
        } else {
            Some(&prev[(y - 1) * stride..])
        };
        let cur = &mut cur[..stride];
        for i in 0..stride {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = up.map_or(0, |u| u[i]);
            let c = if i >= bpp {
                up.map_or(0, |u| u[i - bpp])
            } else {
                0
            };
            cur[i] = match kind {
                0 => line[i],
                1 => line[i].wrapping_add(a),
                2 => line[i].wrapping_add(b),
                3 => line[i].wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => line[i].wrapping_add(paeth(a, b, c)),
                k => return Err(format!("unknown PNG filter type {k}")),
            };
        }
    }
    Ok(out)
}

/// The unpacked samples of one row of `n` pixels (`ch` channels of `depth` bits): one value per sample,
/// 16-bit samples as `u16`.
fn samples(row: &[u8], n: usize, ch: usize, depth: u8) -> Vec<u16> {
    let count = n * ch;
    match depth {
        16 => (0..count)
            .map(|i| u16::from_be_bytes([row[2 * i], row[2 * i + 1]]))
            .collect(),
        8 => row[..count].iter().map(|&b| u16::from(b)).collect(),
        d => {
            let per = 8 / usize::from(d);
            let mask = (1u16 << d) - 1;
            (0..count)
                .map(|i| {
                    let byte = u16::from(row[i / per]);
                    let shift = 8 - usize::from(d) * (i % per + 1);
                    (byte >> shift) & mask
                })
                .collect()
        }
    }
}

/// Pillow's `alpha_composite` of a pixel over opaque white.
fn over_white(r: u8, g: u8, b: u8, a: u8) -> [u8; 3] {
    if a == 0 {
        return [255, 255, 255];
    }
    const PREC: u32 = 7;
    let div255 = |x: u32| ((x >> 8) + x) >> 8;
    let a = u32::from(a);
    let blend = 255 * (255 - a);
    let outa255 = a * 255 + blend;
    let coef1 = a * 255 * 255 * (1 << PREC) / outa255;
    let coef2 = 255 * (1 << PREC) - coef1;
    let ch = |s: u8| (div255(u32::from(s) * coef1 + 255 * coef2 + (0x80 << PREC)) >> PREC) as u8;
    [ch(r), ch(g), ch(b)]
}

/// Decodes a PNG as the reference's `load_image` sees it: BGR, alpha (palette transparency, gray+alpha,
/// RGBA) composited on white; 1-, 2- and 4-bit gray scaled to 0–255; 16-bit gray clipped at 255 and 16-bit
/// colour reduced to its high byte (Pillow's modes `I;16` and `RGB;16B`); a gray or RGB image's `tRNS`
/// colour ignored.
///
/// # Errors
/// Not a PNG (JPEG and WebP named, with the repair), a damaged file or an unsupported variant.
pub fn load_png(bytes: &[u8]) -> Result<Bgr, String> {
    load_png_with_limit(bytes, 40_000_000)
}

/// Decode with a caller's pixel budget, enforced before decompression and raster allocation.
/// The inflated byte count must exactly match the scanlines declared by IHDR (including Adam7).
/// This preserves [`load_png`]'s colour semantics; it never resizes or truncates an image.
pub fn load_png_with_limit(bytes: &[u8], max_pixels: usize) -> Result<Bgr, String> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff])
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
    {
        return Err(
            "JPEG and WebP sheets are not decoded yet: convert the image to PNG first".into(),
        );
    }
    if !bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Err("not a PNG file".into());
    }
    let (mut ihdr, mut plte, mut trns, mut idat) = (None, Vec::new(), None, Vec::new());
    let mut pos = 8;
    while bytes.len() - pos >= 12 {
        let len = be32(&bytes[pos..]) as usize;
        let kind = &bytes[pos + 4..pos + 8];
        let end = pos
            .checked_add(12)
            .and_then(|n| n.checked_add(len))
            .filter(|&n| n <= bytes.len())
            .ok_or("truncated PNG chunk")?;
        let data = bytes
            .get(pos + 8..pos + 8 + len)
            .ok_or("truncated PNG chunk")?;
        let crc = bytes
            .get(pos + 8 + len..pos + 12 + len)
            .ok_or("truncated PNG chunk")?;
        if crc32(&[kind, data]) != be32(crc) {
            return Err(format!(
                "broken PNG file (bad CRC in {})",
                String::from_utf8_lossy(kind)
            ));
        }
        match kind {
            b"IHDR" => {
                if pos != 8 || ihdr.is_some() {
                    return Err("PNG header must occur once, before image data".into());
                }
                ihdr = Some(data.to_vec());
            }
            b"PLTE" => plte = data.to_vec(),
            b"tRNS" => trns = Some(data.to_vec()),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        pos = end;
    }
    let ihdr = ihdr
        .filter(|h| h.len() == 13)
        .ok_or("PNG without a header")?;
    let (w, h) = (be32(&ihdr[0..]) as usize, be32(&ihdr[4..]) as usize);
    let (depth, color, interlace) = (ihdr[8], ihdr[9], ihdr[12]);
    let ch = match color {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        c => return Err(format!("unknown PNG colour type {c}")),
    };
    let ok_depth = match color {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => matches!(depth, 8 | 16),
    };
    if !ok_depth || w == 0 || h == 0 || interlace > 1 || ihdr[10] != 0 || ihdr[11] != 0 {
        return Err(format!(
            "unsupported PNG (bit depth {depth}, colour type {color}, interlace {interlace})"
        ));
    }
    let pixels = w
        .checked_mul(h)
        .filter(|&n| n <= max_pixels)
        .ok_or_else(|| format!("PNG exceeds the {max_pixels} pixel decoding budget"))?;
    let bits = usize::from(depth) * ch;
    let bpp = bits.div_ceil(8).max(1);
    let passes: &[(usize, usize, usize, usize)] = if interlace == 1 {
        &[
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ]
    } else {
        &[(0, 0, 1, 1)]
    };
    let mut expected = 0usize;
    for &(x0, y0, dx, dy) in passes {
        if x0 < w && y0 < h {
            let (pw, ph) = ((w - x0).div_ceil(dx), (h - y0).div_ceil(dy));
            let stride = pw
                .checked_mul(bits)
                .ok_or("PNG row size overflow")?
                .div_ceil(8);
            expected = stride
                .checked_add(1)
                .and_then(|n| n.checked_mul(ph))
                .and_then(|n| expected.checked_add(n))
                .ok_or("PNG scanline size overflow")?;
        }
    }
    let raw = decompress_to_vec_zlib_with_limit(&idat, expected)
        .map_err(|e| format!("broken or oversized PNG image data ({e:?})"))?;
    if raw.len() != expected {
        return Err("PNG scanline byte count does not match its header".into());
    }
    // Every pixel's samples, in raster order, allocated only after validating the compressed data.
    let mut px: Vec<u16> = vec![0; pixels.checked_mul(ch).ok_or("PNG sample count overflow")?];
    let mut off = 0;
    for &(x0, y0, dx, dy) in passes {
        if x0 >= w || y0 >= h {
            continue;
        }
        let (pw, ph) = ((w - x0).div_ceil(dx), (h - y0).div_ceil(dy));
        let stride = (pw * bits).div_ceil(8);
        let size = ph * (stride + 1);
        let sub = unfilter(
            raw.get(off..).ok_or("truncated image data")?,
            ph,
            stride,
            bpp,
        )?;
        off += size;
        for r in 0..ph {
            let row = samples(&sub[r * stride..(r + 1) * stride], pw, ch, depth);
            for c in 0..pw {
                let (x, y) = (x0 + c * dx, y0 + r * dy);
                px[(y * w + x) * ch..(y * w + x + 1) * ch]
                    .copy_from_slice(&row[c * ch..(c + 1) * ch]);
            }
        }
    }
    let to8 = |v: u16| -> u8 {
        match depth {
            16 => (v >> 8) as u8,
            8 => v as u8,
            4 => (v * 0x11) as u8,
            2 => (v * 0x55) as u8,
            _ => {
                if v == 0 {
                    0
                } else {
                    255
                }
            }
        }
    };
    let mut data = Vec::with_capacity(3 * w * h);
    for i in 0..w * h {
        let s = &px[i * ch..(i + 1) * ch];
        let [r, g, b] = match color {
            0 => {
                let v = if depth == 16 {
                    if s[0] > 255 { 255 } else { s[0] as u8 }
                } else {
                    to8(s[0])
                };
                [v, v, v]
            }
            2 => [to8(s[0]), to8(s[1]), to8(s[2])],
            3 => {
                let k = usize::from(s[0]);
                let rgb = plte
                    .get(3 * k..3 * k + 3)
                    .map_or([0, 0, 0], |p| [p[0], p[1], p[2]]);
                let a = trns.as_ref().and_then(|t| t.get(k).copied()).unwrap_or(255);
                over_white(rgb[0], rgb[1], rgb[2], a)
            }
            4 => {
                let v = to8(s[0]);
                over_white(v, v, v, to8(s[1]))
            }
            _ => over_white(to8(s[0]), to8(s[1]), to8(s[2]), to8(s[3])),
        };
        data.extend_from_slice(&[b, g, r]);
    }
    Ok(Bgr { w, h, data })
}

#[cfg(test)]
mod decode_budget_tests {
    use super::*;

    fn png(w: u32, h: u32, interlace: u8, raw: &[u8]) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend(w.to_be_bytes());
        header.extend(h.to_be_bytes());
        header.extend([8, 0, 0, 0, interlace]);
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(raw, 6);
        for (kind, data) in [
            (b"IHDR", header.as_slice()),
            (b"IDAT", compressed.as_slice()),
            (b"IEND", &[][..]),
        ] {
            bytes.extend((data.len() as u32).to_be_bytes());
            bytes.extend(kind);
            bytes.extend(data);
            bytes.extend(crc32(&[kind, data]).to_be_bytes());
        }
        bytes
    }

    #[test]
    fn bounded_decode_preserves_pixels_at_exact_budget() {
        let gray = Gray {
            w: 2,
            h: 2,
            data: vec![0, 80, 160, 255],
        };
        let bytes = encode_png_gray(&gray);
        let got = load_png_with_limit(&bytes, 4).unwrap();
        assert_eq!(
            got.data,
            vec![0, 0, 0, 80, 80, 80, 160, 160, 160, 255, 255, 255]
        );
        assert_eq!(got, load_png(&bytes).unwrap());
        assert!(
            load_png_with_limit(&bytes, 3)
                .unwrap_err()
                .contains("pixel decoding budget")
        );
    }

    #[test]
    fn tiny_header_cannot_expand_unbounded_data() {
        assert!(load_png_with_limit(&png(1, 1, 0, &[0; 4096]), 1).is_err());
        assert!(load_png_with_limit(&png(1, 1, 0, &[0]), 1).is_err());
        assert_eq!(
            load_png_with_limit(&png(1, 1, 0, &[0, 73]), 1)
                .unwrap()
                .data,
            [73; 3]
        );
    }

    #[test]
    fn huge_headers_and_malformed_chunk_lengths_do_not_allocate() {
        assert!(load_png_with_limit(&png(u32::MAX, u32::MAX, 0, &[]), 16_000_000).is_err());
        let mut bytes = encode_png_gray(&Gray::filled(2, 2, 80));
        bytes[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(load_png_with_limit(&bytes, 4).is_err());
    }

    #[test]
    fn adam7_budget_counts_every_pass_filter_and_packed_row() {
        let mut raw = Vec::new();
        for (x0, y0, dx, dy) in [
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ] {
            for y in (y0..5).step_by(dy) {
                raw.push(0);
                for x in (x0..5).step_by(dx) {
                    raw.push((y * 5 + x) as u8);
                }
            }
        }
        let bytes = png(5, 5, 1, &raw);
        let image = load_png_with_limit(&bytes, 25).unwrap();
        assert_eq!(
            image.data,
            (0u8..25).flat_map(|v| [v; 3]).collect::<Vec<_>>()
        );
        raw.push(0);
        assert!(load_png_with_limit(&png(5, 5, 1, &raw), 25).is_err());
    }
}
