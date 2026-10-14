//! WebP decoding as the reference's `load_image` sees it. Pillow 12.3 opens a WebP through libwebp 1.6.0's
//! `WebPAnimDecoder` (RGBA output, fancy upsampling, no dithering). It keeps the image as RGBA when
//! `WebPGetFeatures` reports alpha (`load_image` then composites it on white), and as RGB otherwise. This
//! is a port of that path:
//! - the demuxer's container rules (`src/demux/demux.c`) and `WebPGetFeatures` (`src/dec/webp_dec.c`);
//! - the lossy VP8 decoder (`src/dec/vp8_dec.c`, `tree_dec.c`, `quant_dec.c`, `frame_dec.c`,
//!   `src/dsp/dec.c`): reconstruction from unfiltered neighbours, then the loop filter in macroblock order;
//! - the alpha plane (`src/dec/alpha_dec.c`, `src/dsp/filters.c`);
//! - the output stage's fancy upsampler and YUV→RGB (`src/dec/io_dec.c`, `src/dsp/upsampling.c`,
//!   `src/dsp/yuv.h`);
//! - the lossless VP8L decoder ([`super::webp_lossless`]).
//!
//! Refused here, because it is not ported: animated files (ANIM/ANMF chunks).
//!
//! The port follows libwebp's C code. On arm64, Pillow's libwebp runs Neon transforms instead, which give
//! the same pixels except for coefficients beyond what an encoder writes (`TransformAC3`'s 16-bit
//! truncation, saturating adds), so a crafted lossy file can decode differently there.
//!
//! Provenance: libwebp 1.6.0 (BSD 3-clause; `LICENSE-libwebp.md` in this crate).

use super::image::{Bgr, over_white};
use super::webp_lossless;
use super::webp_tables::{BMODES_PROBA, COEFFS_PROBA0, COEFFS_UPDATE_PROBA};

/// The refusal for a file that ends inside data the decoder needs.
const TRUNCATED: &str = "WebP: the file is truncated";
/// format_constants.h `MAX_CHUNK_PAYLOAD`.
const MAX_CHUNK_PAYLOAD: usize = 0xFFFF_FFFF - 8 - 1;
/// VP8X flags: `ALPHA_FLAG`, `ANIMATION_FLAG` and every valid bit (`ALL_VALID_FLAGS`).
const ALPHA_FLAG: u8 = 0x10;
const ANIMATION_FLAG: u8 = 0x02;
const ALL_VALID_FLAGS: u8 = 0x3E;

fn le16(b: &[u8]) -> usize {
    usize::from(b[0]) | usize::from(b[1]) << 8
}

fn le24(b: &[u8]) -> usize {
    le16(b) | usize::from(b[2]) << 16
}

fn le32(b: &[u8]) -> usize {
    le24(b) | usize::from(b[3]) << 24
}

/// One still image: its optional ALPH payload and its VP8/VP8L payload (to the end of the padded chunk).
struct Frame<'a> {
    alpha: Option<&'a [u8]>,
    payload: &'a [u8],
    lossless: bool,
    w: usize,
    h: usize,
}

/// Decodes a WebP as Pillow 12.3 does for `Image.open(f)` and `load_image`, returned as OpenCV's BGR.
///
/// # Errors
/// A damaged or truncated file, an image over `max_pixels`, or an animated file (not ported).
pub fn decode(bytes: &[u8], max_pixels: usize) -> Result<Bgr, String> {
    // anim_decode.c: WebPAnimDecoderNew refuses what WebPGetFeatures refuses, before it demuxes
    let has_alpha = features_have_alpha(bytes).ok_or("WebP: bad file header")?;
    let (frame, canvas) = demux(bytes)?;
    let (w, h) = canvas;
    if w.checked_mul(h).is_none_or(|p| p > max_pixels) {
        return Err(format!("WebP {w}×{h}: more than {max_pixels} pixels"));
    }
    let rgba = if frame.lossless {
        let argb = webp_lossless::decode_image(frame.payload, w, h)?;
        argb.iter()
            .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8])
            .collect()
    } else {
        let alpha = frame.alpha.map(|a| decode_alpha(a, w, h)).transpose()?;
        decode_vp8(frame.payload, w, h, alpha.as_deref())?
    };
    // _webp.c: RGBA when WebPGetFeatures reports alpha, else RGBX, which Pillow reads as RGB
    let data = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| {
            let [r, g, b] = if has_alpha {
                over_white(r, g, b, a)
            } else {
                [r, g, b]
            };
            [b, g, r]
        })
        .collect();
    Ok(Bgr { w, h, data })
}

/// demux.c `WebPDemux` as `WebPAnimDecoderNew` runs it on the whole file: the RIFF header, the master
/// chunk (simple VP8/VP8L or extended VP8X) and the checks of `IsValidSimpleFormat` /
/// `IsValidExtendedFormat`. Returns the first frame and the canvas size.
fn demux(data: &[u8]) -> Result<(Frame<'_>, (usize, usize)), String> {
    if data.len() < 20 {
        return Err(TRUNCATED.into());
    }
    if &data[..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return Err("not a WebP file".into());
    }
    let riff_size = le32(&data[4..]);
    // ReadHeader takes 8 and up, but under 12 the first chunk header cannot fit (PARSE_NEED_MORE_DATA)
    if !(12..=MAX_CHUNK_PAYLOAD).contains(&riff_size) {
        return Err("WebP: bad RIFF size".into());
    }
    let end = riff_size + 8;
    if data.len() < end {
        return Err(TRUNCATED.into());
    }
    let data = &data[..end];
    match &data[12..16] {
        b"VP8 " | b"VP8L" => {
            let (mut frame, _, _) = store_frame(data, 12, end)?;
            frame.alpha = None; // a simple file has no ALPHA_FLAG: ParseSingleImage drops any ALPH
            let canvas = (frame.w, frame.h);
            Ok((frame, canvas))
        }
        b"VP8X" => extended(data, end),
        _ => Err("WebP: unknown first chunk".into()),
    }
}

/// demux.c `ParseVP8X` + `ParseVP8XChunks` for a still image.
fn extended(data: &[u8], end: usize) -> Result<(Frame<'_>, (usize, usize)), String> {
    let size = le32(&data[16..]);
    if !(10..=MAX_CHUNK_PAYLOAD).contains(&size) {
        return Err("WebP: bad VP8X chunk".into());
    }
    let padded = size + (size & 1);
    if padded > end - 20 {
        return Err("WebP: bad VP8X chunk".into());
    }
    let flags = data[20];
    let (cw, ch) = (1 + le24(&data[24..]), 1 + le24(&data[27..]));
    if (cw as u64) * (ch as u64) >= 1u64 << 32 {
        return Err("WebP: image too large".into());
    }
    let mut pos = 20 + padded;
    if end - pos < 8 {
        return Err("WebP: no image chunk".into());
    }
    let mut frame: Option<Frame<'_>> = None;
    let mut alpha_after_image = false;
    loop {
        let start = pos;
        let (fourcc, size) = (&data[pos..pos + 4], le32(&data[pos + 4..]));
        if size > MAX_CHUNK_PAYLOAD || size + (size & 1) > end - pos - 8 {
            return Err("WebP: bad chunk size".into());
        }
        match fourcc {
            b"VP8X" => return Err("WebP: a second VP8X chunk".into()),
            b"ALPH" | b"VP8 " | b"VP8L" => {
                if flags & ANIMATION_FLAG != 0 {
                    return Err("WebP: animated files are not decoded here".into());
                }
                if frame.is_some() {
                    return Err("WebP: a second image in a still file".into());
                }
                let (mut f, next, late_alpha) = store_frame(data, start, end)?;
                if flags & ALPHA_FLAG == 0 {
                    f.alpha = None;
                } else {
                    alpha_after_image = late_alpha;
                }
                frame = Some(f);
                pos = next;
            }
            b"ANIM" | b"ANMF" => return Err("WebP: animated files are not decoded here".into()),
            _ => pos += 8 + size + (size & 1),
        }
        if pos == end {
            break;
        }
        if end - pos < 8 {
            return Err(TRUNCATED.into());
        }
    }
    let frame = frame.ok_or("WebP: no image chunk")?;
    if flags & !ALL_VALID_FLAGS != 0 || alpha_after_image || (frame.w, frame.h) != (cw, ch) {
        return Err("WebP: invalid extended file".into());
    }
    Ok((frame, (cw, ch)))
}

/// demux.c `StoreFrame`: an optional ALPH, then one VP8 or VP8L chunk; stops at anything else. Returns the
/// frame, the position after it, and whether an ALPH chunk followed the image (invalid with ALPHA_FLAG).
fn store_frame(
    data: &[u8],
    mut pos: usize,
    end: usize,
) -> Result<(Frame<'_>, usize, bool), String> {
    let mut alpha: Option<&[u8]> = None;
    let mut image: Option<(&[u8], bool, usize, usize)> = None;
    let mut late_alpha = false;
    loop {
        let start = pos;
        let (fourcc, size) = (&data[pos..pos + 4], le32(&data[pos + 4..]));
        let padded = size + (size & 1);
        if size > MAX_CHUNK_PAYLOAD || padded > end - pos - 8 {
            return Err("WebP: bad chunk size".into());
        }
        let payload = &data[pos + 8..pos + 8 + padded];
        match fourcc {
            b"ALPH" if alpha.is_none() => {
                alpha = Some(&payload[..size]);
                late_alpha = image.is_some();
                pos += 8 + padded;
            }
            b"VP8L" if alpha.is_some() => {
                return Err("WebP: an ALPH chunk with VP8L, which has its own alpha".into());
            }
            b"VP8 " | b"VP8L" if image.is_none() => {
                let lossless = fourcc == b"VP8L";
                let (w, h, _) = image_features(payload, size, lossless)
                    .ok_or("WebP: bad image chunk header")?;
                image = Some((payload, lossless, w, h));
                pos += 8 + padded;
            }
            _ => {
                pos = start;
                break;
            }
        }
        if pos == end {
            break;
        }
        if end - pos < 8 {
            return Err(TRUNCATED.into());
        }
    }
    let (payload, lossless, w, h) = image.ok_or("WebP: no image chunk")?;
    let frame = Frame {
        alpha,
        payload,
        lossless,
        w,
        h,
    };
    Ok((frame, pos, late_alpha))
}

/// webp_dec.c `VP8GetInfo` / `VP8LGetInfo` on a chunk payload: width, height and VP8L's alpha bit.
fn image_features(
    payload: &[u8],
    chunk_size: usize,
    lossless: bool,
) -> Option<(usize, usize, bool)> {
    if lossless {
        if payload.len() < 5 || payload[0] != 0x2f || payload[4] >> 5 != 0 {
            return None;
        }
        let bits = le32(&payload[1..]);
        Some((
            (bits & 0x3fff) + 1,
            ((bits >> 14) & 0x3fff) + 1,
            (bits >> 28) & 1 == 1,
        ))
    } else {
        if payload.len() < 10 || payload[3..6] != [0x9d, 0x01, 0x2a] {
            return None;
        }
        let bits = le24(payload);
        let (w, h) = (le16(&payload[6..]) & 0x3fff, le16(&payload[8..]) & 0x3fff);
        let ok = bits & 1 == 0
            && (bits >> 1) & 7 <= 3
            && (bits >> 4) & 1 == 1
            && (bits >> 5) < chunk_size;
        (ok && w > 0 && h > 0).then_some((w, h, false))
    }
}

/// webp_dec.c `WebPGetFeatures(...).has_alpha` (`ParseHeadersInternal` without headers) on the whole file,
/// as Pillow passes it. None when it fails, and then `WebPAnimDecoderNew` (so Pillow) refuses the file.
/// Once a VP8X chunk is read, data that runs out (`NOT_ENOUGH_DATA`) is not a failure: the flags so far
/// are the answer.
fn features_have_alpha(data: &[u8]) -> Option<bool> {
    if data.len() < 12 {
        return None;
    }
    let mut pos = 0;
    let mut riff_size = 0;
    if &data[..4] == b"RIFF" {
        if &data[8..12] != b"WEBP" {
            return None;
        }
        riff_size = le32(&data[4..]);
        if !(12..=MAX_CHUNK_PAYLOAD).contains(&riff_size) {
            return None;
        }
        pos = 12;
    }
    let rest = &data[pos..];
    if rest.len() < 8 {
        return None;
    }
    let vp8x = &rest[..4] == b"VP8X";
    let mut has_alpha = false;
    let mut canvas = None;
    if vp8x {
        if le32(&rest[4..]) != 10 || rest.len() < 18 || riff_size == 0 {
            return None;
        }
        let flags = rest[8];
        let (w, h) = (1 + le24(&rest[12..]), 1 + le24(&rest[15..]));
        if (w as u64) * (h as u64) >= 1u64 << 32 {
            return None;
        }
        has_alpha = flags & ALPHA_FLAG != 0;
        if flags & ANIMATION_FLAG != 0 {
            return Some(has_alpha);
        }
        canvas = Some((w, h));
        pos += 18;
    }
    let mut alpha_data = false;
    // Err(true) is NOT_ENOUGH_DATA, Err(false) a bitstream error
    let parsed: Result<(), bool> = 'parse: {
        if vp8x {
            // ParseOptionalChunks: up to the VP8/VP8L chunk, within the RIFF size
            let mut total = 4 + 8 + 10;
            loop {
                let Some(c) = data.get(pos..pos + 8) else {
                    break 'parse Err(true);
                };
                let size = le32(&c[4..]);
                if size > MAX_CHUNK_PAYLOAD {
                    break 'parse Err(false);
                }
                let disk = (8 + size + 1) & !1;
                total += disk;
                if total > riff_size {
                    break 'parse Err(false);
                }
                if &c[..4] == b"VP8 " || &c[..4] == b"VP8L" {
                    break;
                }
                if data.len() - pos < disk {
                    break 'parse Err(true);
                }
                alpha_data |= &c[..4] == b"ALPH";
                pos += disk;
            }
        }
        // ParseVP8Header (a RIFF file whose first chunk is not VP8/VP8L/VP8X is refused by the demuxer)
        let Some(c) = data.get(pos..pos + 8) else {
            break 'parse Err(true);
        };
        let lossless = match &c[..4] {
            b"VP8 " => false,
            b"VP8L" => true,
            _ => break 'parse Err(false),
        };
        let size = le32(&c[4..]);
        if riff_size >= 12 && size > riff_size - 12 {
            break 'parse Err(false);
        }
        let payload = &data[pos + 8..];
        if payload.len() < if lossless { 5 } else { 10 } {
            break 'parse Err(true);
        }
        let Some((w, h, vp8l_alpha)) = image_features(payload, size, lossless) else {
            break 'parse Err(false);
        };
        if lossless {
            has_alpha = vp8l_alpha;
        }
        if canvas.is_some_and(|c| c != (w, h)) {
            break 'parse Err(false);
        }
        Ok(())
    };
    match parsed {
        Ok(()) => Some(has_alpha || alpha_data),
        Err(short) if short && vp8x => Some(has_alpha || alpha_data),
        Err(_) => None,
    }
}

// ------------------------------------------------------------------------------------------- alpha

/// alpha_dec.c `ALPHInit` + `ALPHDecode`: the alpha plane of a lossy frame (raw or VP8L-compressed,
/// then unfiltered row by row).
fn decode_alpha(data: &[u8], w: usize, h: usize) -> Result<Vec<u8>, String> {
    let bad = || "WebP: could not decode alpha data".to_string();
    if data.len() <= 1 {
        return Err(bad());
    }
    let (method, filter, pre, rsrv) = (
        data[0] & 3,
        (data[0] >> 2) & 3,
        (data[0] >> 4) & 3,
        data[0] >> 6,
    );
    if method > 1 || pre > 1 || rsrv != 0 {
        return Err(bad());
    }
    let body = &data[1..];
    let mut plane = if method == 0 {
        body.get(..w * h).ok_or_else(bad)?.to_vec()
    } else {
        webp_lossless::decode_alpha(body, w, h)?
    };
    for y in 0..h {
        let (above, row) = plane.split_at_mut(y * w);
        let prev = (y > 0).then(|| &above[(y - 1) * w..]);
        unfilter(filter, prev, &mut row[..w]);
    }
    Ok(plane)
}

/// filters.c `WebPUnfilters` in place: none, horizontal, vertical, gradient (the first row horizontal).
fn unfilter(filter: u8, prev: Option<&[u8]>, row: &mut [u8]) {
    let horizontal = |prev: Option<&[u8]>, row: &mut [u8]| {
        let mut pred = prev.map_or(0, |p| p[0]);
        for v in row.iter_mut() {
            *v = v.wrapping_add(pred);
            pred = *v;
        }
    };
    match (filter, prev) {
        (0, _) => {}
        (1, _) | (_, None) => horizontal(prev, row),
        (2, Some(p)) => row
            .iter_mut()
            .zip(p)
            .for_each(|(v, &t)| *v = v.wrapping_add(t)),
        (_, Some(p)) => {
            let (mut top_left, mut left) = (p[0], p[0]);
            for (v, &top) in row.iter_mut().zip(p) {
                let g = i32::from(left) + i32::from(top) - i32::from(top_left);
                let pred = if g & !0xff == 0 {
                    g as u8
                } else if g < 0 {
                    0
                } else {
                    255
                };
                left = v.wrapping_add(pred);
                top_left = top;
                *v = left;
            }
        }
    }
}

// --------------------------------------------------------------------------------------- bool reader

/// bit_reader_utils.h `VP8BitReader`: the boolean decoder, loading one byte at a time (`VP8LoadFinalBytes`
/// semantics: zeros past the end, `eof` set once more bits were needed than the data holds).
struct BoolReader<'a> {
    data: &'a [u8],
    pos: usize,
    value: u64,
    bits: i32,
    range: u32,
    eof: bool,
}

impl<'a> BoolReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        let mut br = Self {
            data,
            pos: 0,
            value: 0,
            bits: -8,
            range: 255 - 1,
            eof: false,
        };
        br.load();
        br
    }

    fn load(&mut self) {
        if let Some(&b) = self.data.get(self.pos) {
            self.pos += 1;
            self.bits += 8;
            self.value = (self.value << 8) | u64::from(b);
        } else if !self.eof {
            self.value <<= 8;
            self.bits += 8;
            self.eof = true;
        } else {
            self.bits = 0;
        }
    }

    /// `VP8GetBit`.
    fn bit(&mut self, prob: u32) -> bool {
        let mut range = self.range;
        if self.bits < 0 {
            self.load();
        }
        let pos = self.bits;
        let split = (range * prob) >> 8;
        let value = (self.value >> pos) as u32;
        let bit = value > split;
        if bit {
            range -= split;
            self.value -= u64::from(split + 1) << pos;
        } else {
            range = split + 1;
        }
        let shift = 7 ^ (31 - range.leading_zeros()) as i32;
        range <<= shift;
        self.bits -= shift;
        self.range = range - 1;
        bit
    }

    /// `VP8GetSigned`: `v` with a sign read at probability ½.
    fn signed(&mut self, v: i32) -> i32 {
        if self.bits < 0 {
            self.load();
        }
        let pos = self.bits;
        let split = self.range >> 1;
        let value = (self.value >> pos) as u32;
        let mask = (split.wrapping_sub(value) as i32) >> 31;
        self.bits -= 1;
        self.range = self.range.wrapping_add(mask as u32) | 1;
        self.value -= u64::from((split + 1) & mask as u32) << pos;
        (v ^ mask) - mask
    }

    /// `VP8GetValue`: `n` bits, most significant first.
    fn value(&mut self, n: u32) -> u32 {
        (0..n)
            .rev()
            .fold(0, |v, i| v | u32::from(self.bit(0x80)) << i)
    }

    fn flag(&mut self) -> bool {
        self.value(1) != 0
    }

    /// `VP8GetSignedValue`.
    fn signed_value(&mut self, n: u32) -> i32 {
        let v = self.value(n) as i32;
        if self.flag() { -v } else { v }
    }
}

// ------------------------------------------------------------------------------------------ VP8 lossy

const BPS: usize = 32;
const Y_OFF: usize = BPS + 8;
const U_OFF: usize = Y_OFF + BPS * 16 + BPS;
const V_OFF: usize = U_OFF + 16;
const YUV_SIZE: usize = BPS * 17 + BPS * 9;
/// tree_dec.c `kBands` (with its sentinel), vp8_dec.c `kZigzag`, the cat3–6 extra-bit probabilities.
const BANDS: [usize; 17] = [0, 1, 2, 3, 6, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7, 0];
const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];
const CAT3456: [&[u32]; 4] = [
    &[173, 148, 140],
    &[176, 155, 140, 135],
    &[180, 157, 141, 134, 130],
    &[254, 254, 243, 230, 196, 177, 153, 140, 133, 130, 129],
];
/// quant_dec.c `kDcTable` and `kAcTable`.
const DC_TABLE: [u8; 128] = [
    4, 5, 6, 7, 8, 9, 10, 10, 11, 12, 13, 14, 15, 16, 17, 17, 18, 19, 20, 20, 21, 21, 22, 22, 23,
    23, 24, 25, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 37, 38, 39, 40, 41, 42, 43, 44,
    45, 46, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67,
    68, 69, 70, 71, 72, 73, 74, 75, 76, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 91,
    93, 95, 96, 98, 100, 101, 102, 104, 106, 108, 110, 112, 114, 116, 118, 122, 124, 126, 128, 130,
    132, 134, 136, 138, 140, 143, 145, 148, 151, 154, 157,
];
const AC_TABLE: [u16; 128] = [
    4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28,
    29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52,
    53, 54, 55, 56, 57, 58, 60, 62, 64, 66, 68, 70, 72, 74, 76, 78, 80, 82, 84, 86, 88, 90, 92, 94,
    96, 98, 100, 102, 104, 106, 108, 110, 112, 114, 116, 119, 122, 125, 128, 131, 134, 137, 140,
    143, 146, 149, 152, 155, 158, 161, 164, 167, 170, 173, 177, 181, 185, 189, 193, 197, 201, 205,
    209, 213, 217, 221, 225, 229, 234, 239, 245, 249, 254, 259, 264, 269, 274, 279, 284,
];

/// The coefficient probabilities `[type][band][context][11]`.
type Proba = [[[[u8; 11]; 3]; 8]; 4];

/// vp8i_dec.h `VP8QuantMatrix`.
#[derive(Clone, Copy, Default)]
struct Quant {
    y1: [i32; 2],
    y2: [i32; 2],
    uv: [i32; 2],
}

/// vp8i_dec.h `VP8MBData`: one macroblock's modes and dequantized coefficients.
#[derive(Clone)]
struct MbData {
    coeffs: [i16; 384],
    is_i4x4: bool,
    imodes: [u8; 16],
    uvmode: u8,
    segment: usize,
    skip: bool,
    non_zero_y: u32,
    non_zero_uv: u32,
}

/// vp8i_dec.h `VP8FInfo`: one macroblock's loop-filter parameters.
#[derive(Clone, Copy, Default)]
struct FInfo {
    limit: i32,
    ilevel: i32,
    inner: bool,
    hev: i32,
}

/// vp8_dec.c `VP8GetHeaders` + `ParseFrame`, frame_dec.c reconstruction and filtering, io_dec.c's fancy
/// RGBA output: the frame as RGBA bytes, alpha from `alpha` or 255.
fn decode_vp8(data: &[u8], w: usize, h: usize, alpha: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let bad = |m: &str| format!("WebP: {m}");
    if data.len() < 10 {
        return Err(TRUNCATED.into());
    }
    let bits = le24(data);
    let partition_length = bits >> 5;
    if bits & 1 != 0 || (bits >> 1) & 7 > 3 || (bits >> 4) & 1 == 0 {
        return Err(bad("not a displayable key frame"));
    }
    let buf = &data[10..];
    if (le16(&data[6..]) & 0x3fff, le16(&data[8..]) & 0x3fff) != (w, h) {
        return Err(bad("frame size differs from the container's"));
    }
    let (mb_w, mb_h) = (w.div_ceil(16), h.div_ceil(16));
    if partition_length > buf.len() {
        return Err(bad("bad partition length"));
    }
    let mut br = BoolReader::new(&buf[..partition_length]);
    let rest = &buf[partition_length..];
    let _colorspace = br.flag();
    let _clamp_type = br.flag();
    // ParseSegmentHeader
    let use_segment = br.flag();
    let (mut update_map, mut absolute_delta) = (false, true);
    let (mut seg_quant, mut seg_filter) = ([0i32; 4], [0i32; 4]);
    let mut seg_probs = [255u32; 3];
    if use_segment {
        update_map = br.flag();
        if br.flag() {
            absolute_delta = br.flag();
            for q in &mut seg_quant {
                *q = if br.flag() { br.signed_value(7) } else { 0 };
            }
            for f in &mut seg_filter {
                *f = if br.flag() { br.signed_value(6) } else { 0 };
            }
        }
        if update_map {
            for p in &mut seg_probs {
                *p = if br.flag() { br.value(8) } else { 255 };
            }
        }
    }
    if br.eof {
        return Err(bad("cannot parse segment header"));
    }
    // ParseFilterHeader
    let simple = br.flag();
    let level = br.value(6) as i32;
    let sharpness = br.value(3) as i32;
    let use_lf_delta = br.flag();
    let (mut ref_lf_delta, mut mode_lf_delta) = ([0i32; 4], [0i32; 4]);
    if use_lf_delta && br.flag() {
        for d in &mut ref_lf_delta {
            if br.flag() {
                *d = br.signed_value(6);
            }
        }
        for d in &mut mode_lf_delta {
            if br.flag() {
                *d = br.signed_value(6);
            }
        }
    }
    let filter_type = if level == 0 {
        0
    } else if simple {
        1
    } else {
        2
    };
    if br.eof {
        return Err(bad("cannot parse filter header"));
    }
    // ParsePartitions
    let last_part = (1usize << br.value(2)) - 1;
    if rest.len() < 3 * last_part {
        return Err(TRUNCATED.into());
    }
    let mut parts = Vec::with_capacity(last_part + 1);
    let (mut start, mut left) = (3 * last_part, rest.len() - 3 * last_part);
    for p in 0..last_part {
        let psize = le24(&rest[3 * p..]).min(left);
        parts.push(BoolReader::new(&rest[start..start + psize]));
        start += psize;
        left -= psize;
    }
    if start >= rest.len() {
        return Err(TRUNCATED.into());
    }
    parts.push(BoolReader::new(&rest[start..]));
    // VP8ParseQuant
    let base_q0 = br.value(7) as i32;
    let mut delta = || if br.flag() { br.signed_value(4) } else { 0 };
    let (dqy1_dc, dqy2_dc, dqy2_ac, dquv_dc, dquv_ac) =
        (delta(), delta(), delta(), delta(), delta());
    let mut quant = [Quant::default(); 4];
    for (i, m) in quant.iter_mut().enumerate() {
        let q = if use_segment {
            seg_quant[i] + if absolute_delta { 0 } else { base_q0 }
        } else {
            base_q0
        };
        let clip = |v: i32, max: i32| v.clamp(0, max) as usize;
        m.y1 = [
            i32::from(DC_TABLE[clip(q + dqy1_dc, 127)]),
            i32::from(AC_TABLE[clip(q, 127)]),
        ];
        m.y2 = [
            i32::from(DC_TABLE[clip(q + dqy2_dc, 127)]) * 2,
            ((i32::from(AC_TABLE[clip(q + dqy2_ac, 127)]) * 101_581) >> 16).max(8),
        ];
        m.uv = [
            i32::from(DC_TABLE[clip(q + dquv_dc, 117)]),
            i32::from(AC_TABLE[clip(q + dquv_ac, 127)]),
        ];
    }
    let _update_proba = br.flag();
    // VP8ParseProba
    let mut proba: Proba = [[[[0; 11]; 3]; 8]; 4];
    for (t, bands) in proba.iter_mut().enumerate() {
        for (b, ctxs) in bands.iter_mut().enumerate() {
            for (c, ps) in ctxs.iter_mut().enumerate() {
                for (p, v) in ps.iter_mut().enumerate() {
                    let k = ((t * 8 + b) * 3 + c) * 11 + p;
                    *v = if br.bit(u32::from(COEFFS_UPDATE_PROBA[k])) {
                        br.value(8) as u8
                    } else {
                        COEFFS_PROBA0[k]
                    };
                }
            }
        }
    }
    let use_skip_proba = br.flag();
    let skip_p = if use_skip_proba { br.value(8) } else { 0 };
    // frame_dec.c PrecomputeFilterStrengths
    let mut fstrengths = [[FInfo::default(); 2]; 4];
    if filter_type > 0 {
        for (s, fs) in fstrengths.iter_mut().enumerate() {
            let base = if use_segment {
                seg_filter[s] + if absolute_delta { 0 } else { level }
            } else {
                level
            };
            for (i4x4, info) in fs.iter_mut().enumerate() {
                let mut lv = base;
                if use_lf_delta {
                    lv += ref_lf_delta[0];
                    if i4x4 == 1 {
                        lv += mode_lf_delta[0];
                    }
                }
                let lv = lv.clamp(0, 63);
                if lv > 0 {
                    let mut ilevel = lv;
                    if sharpness > 0 {
                        ilevel >>= if sharpness > 4 { 2 } else { 1 };
                        ilevel = ilevel.min(9 - sharpness);
                    }
                    let ilevel = ilevel.max(1);
                    info.ilevel = ilevel;
                    info.limit = 2 * lv + ilevel;
                    info.hev = if lv >= 40 {
                        2
                    } else if lv >= 15 {
                        1
                    } else {
                        0
                    };
                } else {
                    info.limit = 0;
                }
                info.inner = i4x4 == 1;
            }
        }
    }

    // ParseFrame: modes, residuals and reconstruction row by row into unfiltered planes
    let (yw, uvw) = (mb_w * 16, mb_w * 8);
    let mut ybuf = vec![0u8; yw * mb_h * 16];
    let mut ubuf = vec![0u8; uvw * mb_h * 8];
    let mut vbuf = vec![0u8; uvw * mb_h * 8];
    let mut finfo = vec![FInfo::default(); mb_w * mb_h];
    let mut nz_top = vec![(0u32, 0u32); mb_w];
    let mut intra_t = vec![0u8; 4 * mb_w];
    let mut yuv_t = vec![([0u8; 16], [0u8; 8], [0u8; 8]); mb_w];
    let blank = MbData {
        coeffs: [0; 384],
        is_i4x4: false,
        imodes: [0; 16],
        uvmode: 0,
        segment: 0,
        skip: false,
        non_zero_y: 0,
        non_zero_uv: 0,
    };
    let mut row = vec![blank; mb_w];
    let mut yuv_b = [0u8; YUV_SIZE];
    for mb_y in 0..mb_h {
        let mut intra_l = [0u8; 4];
        for (mb_x, block) in row.iter_mut().enumerate() {
            parse_intra_mode(
                &mut br,
                block,
                &mut intra_t[4 * mb_x..4 * mb_x + 4],
                &mut intra_l,
                update_map,
                &seg_probs,
                use_skip_proba,
                skip_p,
            );
        }
        if br.eof {
            return Err(bad("premature end of partition 0"));
        }
        let token = &mut parts[mb_y & last_part];
        let mut nz_left = (0u32, 0u32);
        for (mb_x, block) in row.iter_mut().enumerate() {
            let skip = if use_skip_proba { block.skip } else { false };
            let skip = if skip {
                nz_left.0 = 0;
                nz_top[mb_x].0 = 0;
                if !block.is_i4x4 {
                    nz_left.1 = 0;
                    nz_top[mb_x].1 = 0;
                }
                block.non_zero_y = 0;
                block.non_zero_uv = 0;
                true
            } else {
                parse_residuals(
                    token,
                    block,
                    &quant[block.segment],
                    &proba,
                    &mut nz_top[mb_x],
                    &mut nz_left,
                )
            };
            if filter_type > 0 {
                let mut f = fstrengths[block.segment][usize::from(block.is_i4x4)];
                f.inner |= !skip;
                finfo[mb_y * mb_w + mb_x] = f;
            }
            if token.eof {
                return Err(bad("premature end of file"));
            }
        }
        reconstruct_row(
            &row, mb_y, mb_w, mb_h, &mut yuv_b, &mut yuv_t, &mut ybuf, &mut ubuf, &mut vbuf,
        );
    }
    // the loop filter, macroblock by macroblock in decoding order (frame_dec.c DoFilter)
    if filter_type > 0 {
        for mb_y in 0..mb_h {
            for mb_x in 0..mb_w {
                let f = finfo[mb_y * mb_w + mb_x];
                if f.limit == 0 {
                    continue;
                }
                let yi = mb_y * 16 * yw + mb_x * 16;
                if filter_type == 1 {
                    if mb_x > 0 {
                        simple_filter16(&mut ybuf, yi, 1, yw, f.limit + 4);
                    }
                    if f.inner {
                        for k in 1..4 {
                            simple_filter16(&mut ybuf, yi + 4 * k, 1, yw, f.limit);
                        }
                    }
                    if mb_y > 0 {
                        simple_filter16(&mut ybuf, yi, yw, 1, f.limit + 4);
                    }
                    if f.inner {
                        for k in 1..4 {
                            simple_filter16(&mut ybuf, yi + 4 * k * yw, yw, 1, f.limit);
                        }
                    }
                } else {
                    let ci = mb_y * 8 * uvw + mb_x * 8;
                    let (lim, il, hev) = (f.limit, f.ilevel, f.hev);
                    if mb_x > 0 {
                        filter_loop(&mut ybuf, yi, 1, yw, 16, lim + 4, il, hev, true);
                        filter_loop(&mut ubuf, ci, 1, uvw, 8, lim + 4, il, hev, true);
                        filter_loop(&mut vbuf, ci, 1, uvw, 8, lim + 4, il, hev, true);
                    }
                    if f.inner {
                        for k in 1..4 {
                            filter_loop(&mut ybuf, yi + 4 * k, 1, yw, 16, lim, il, hev, false);
                        }
                        filter_loop(&mut ubuf, ci + 4, 1, uvw, 8, lim, il, hev, false);
                        filter_loop(&mut vbuf, ci + 4, 1, uvw, 8, lim, il, hev, false);
                    }
                    if mb_y > 0 {
                        filter_loop(&mut ybuf, yi, yw, 1, 16, lim + 4, il, hev, true);
                        filter_loop(&mut ubuf, ci, uvw, 1, 8, lim + 4, il, hev, true);
                        filter_loop(&mut vbuf, ci, uvw, 1, 8, lim + 4, il, hev, true);
                    }
                    if f.inner {
                        for k in 1..4 {
                            filter_loop(&mut ybuf, yi + 4 * k * yw, yw, 1, 16, lim, il, hev, false);
                        }
                        filter_loop(&mut ubuf, ci + 4 * uvw, uvw, 1, 8, lim, il, hev, false);
                        filter_loop(&mut vbuf, ci + 4 * uvw, uvw, 1, 8, lim, il, hev, false);
                    }
                }
            }
        }
    }
    Ok(fancy_rgba(&ybuf, yw, &ubuf, &vbuf, uvw, w, h, alpha))
}

/// tree_dec.c `ParseIntraMode` for one macroblock.
#[allow(clippy::too_many_arguments)]
fn parse_intra_mode(
    br: &mut BoolReader<'_>,
    block: &mut MbData,
    top: &mut [u8],
    left: &mut [u8; 4],
    update_map: bool,
    seg_probs: &[u32; 3],
    use_skip_proba: bool,
    skip_p: u32,
) {
    block.segment = if update_map {
        if br.bit(seg_probs[0]) {
            2 + usize::from(br.bit(seg_probs[2]))
        } else {
            usize::from(br.bit(seg_probs[1]))
        }
    } else {
        0
    };
    if use_skip_proba {
        block.skip = br.bit(skip_p);
    }
    block.is_i4x4 = !br.bit(145);
    if block.is_i4x4 {
        for y in 0..4 {
            let mut ymode = left[y];
            for x in 0..4 {
                let p = &BMODES_PROBA[(usize::from(top[x]) * 10 + usize::from(ymode)) * 9..][..9];
                ymode = if !br.bit(u32::from(p[0])) {
                    0
                } else if !br.bit(u32::from(p[1])) {
                    1
                } else if !br.bit(u32::from(p[2])) {
                    2
                } else if !br.bit(u32::from(p[3])) {
                    if !br.bit(u32::from(p[4])) {
                        3
                    } else if !br.bit(u32::from(p[5])) {
                        4
                    } else {
                        5
                    }
                } else if !br.bit(u32::from(p[6])) {
                    6
                } else if !br.bit(u32::from(p[7])) {
                    7
                } else if !br.bit(u32::from(p[8])) {
                    8
                } else {
                    9
                };
                top[x] = ymode;
            }
            block.imodes[4 * y..4 * y + 4].copy_from_slice(top);
            left[y] = ymode;
        }
    } else {
        // DC_PRED 0, TM_PRED 1, V_PRED 2, H_PRED 3
        let ymode = if br.bit(156) {
            if br.bit(128) { 1 } else { 3 }
        } else if br.bit(163) {
            2
        } else {
            0
        };
        block.imodes[0] = ymode;
        top.fill(ymode);
        left.fill(ymode);
    }
    block.uvmode = if !br.bit(142) {
        0
    } else if !br.bit(114) {
        2
    } else if br.bit(183) {
        1
    } else {
        3
    };
}

/// vp8_dec.c `GetLargeValue`.
fn large_value(br: &mut BoolReader<'_>, p: &[u8; 11]) -> i32 {
    let b = |br: &mut BoolReader<'_>, prob: u8| i32::from(br.bit(u32::from(prob)));
    if b(br, p[3]) == 0 {
        if b(br, p[4]) == 0 { 2 } else { 3 + b(br, p[5]) }
    } else if b(br, p[6]) == 0 {
        if b(br, p[7]) == 0 {
            5 + i32::from(br.bit(159))
        } else {
            7 + 2 * i32::from(br.bit(165)) + i32::from(br.bit(145))
        }
    } else {
        let bit1 = b(br, p[8]);
        let bit0 = b(br, p[9 + bit1 as usize]);
        let cat = (2 * bit1 + bit0) as usize;
        let v = CAT3456[cat]
            .iter()
            .fold(0, |v, &t| v + v + i32::from(br.bit(t)));
        v + 3 + (8 << cat)
    }
}

/// vp8_dec.c `GetCoeffsFast`: one block's coefficients into `out` (dezigzagged and dequantized); returns
/// the index after the last non-zero coefficient (16 when it ran to the end).
fn get_coeffs(
    br: &mut BoolReader<'_>,
    proba: &[[[u8; 11]; 3]; 8],
    ctx: usize,
    dq: [i32; 2],
    first: usize,
    out: &mut [i16],
) -> usize {
    let mut n = first;
    let mut p = &proba[BANDS[n]][ctx];
    while n < 16 {
        if !br.bit(u32::from(p[0])) {
            return n;
        }
        while !br.bit(u32::from(p[1])) {
            n += 1;
            p = &proba[BANDS[n]][0];
            if n == 16 {
                return 16;
            }
        }
        let next = &proba[BANDS[n + 1]];
        let v = if !br.bit(u32::from(p[2])) {
            p = &next[1];
            1
        } else {
            let v = large_value(br, p);
            p = &next[2];
            v
        };
        out[ZIGZAG[n]] = (br.signed(v) * dq[usize::from(n > 0)]) as i16;
        n += 1;
    }
    16
}

/// vp8_dec.c `ParseResiduals`; `top` and `left` are the `(nz, nz_dc)` contexts. Returns whether the
/// macroblock has no non-zero coefficient.
fn parse_residuals(
    br: &mut BoolReader<'_>,
    block: &mut MbData,
    q: &Quant,
    proba: &Proba,
    top: &mut (u32, u32),
    left: &mut (u32, u32),
) -> bool {
    let nz_code = |nz_coeffs: u32, nz: usize, dc_nz: bool| -> u32 {
        (nz_coeffs << 2)
            | if nz > 3 {
                3
            } else if nz > 1 {
                2
            } else {
                u32::from(dc_nz)
            }
    };
    block.coeffs = [0; 384];
    let (first, ac) = if block.is_i4x4 {
        (0, &proba[3])
    } else {
        let mut dc = [0i16; 16];
        let ctx = (top.1 + left.1) as usize;
        let nz = get_coeffs(br, &proba[1], ctx, q.y2, 0, &mut dc);
        let nonzero = u32::from(nz > 0);
        (top.1, left.1) = (nonzero, nonzero);
        if nz > 1 {
            transform_wht(&dc, &mut block.coeffs);
        } else {
            let dc0 = ((i32::from(dc[0]) + 3) >> 3) as i16;
            for i in (0..256).step_by(16) {
                block.coeffs[i] = dc0;
            }
        }
        (1, &proba[0])
    };
    let mut tnz = top.0 & 0x0f;
    let mut lnz = left.0 & 0x0f;
    let mut non_zero_y = 0u32;
    let mut dst = 0usize;
    for _ in 0..4 {
        let mut l = lnz & 1;
        let mut nz_coeffs = 0u32;
        for _ in 0..4 {
            let ctx = (l + (tnz & 1)) as usize;
            let nz = get_coeffs(br, ac, ctx, q.y1, first, &mut block.coeffs[dst..dst + 16]);
            l = u32::from(nz > first);
            tnz = (tnz >> 1) | (l << 7);
            nz_coeffs = nz_code(nz_coeffs, nz, block.coeffs[dst] != 0);
            dst += 16;
        }
        tnz >>= 4;
        lnz = (lnz >> 1) | (l << 7);
        non_zero_y = (non_zero_y << 8) | nz_coeffs;
    }
    let mut out_t_nz = tnz;
    let mut out_l_nz = lnz >> 4;
    let mut non_zero_uv = 0u32;
    for ch in [0u32, 2] {
        let mut nz_coeffs = 0u32;
        tnz = top.0 >> (4 + ch);
        lnz = left.0 >> (4 + ch);
        for _ in 0..2 {
            let mut l = lnz & 1;
            for _ in 0..2 {
                let ctx = (l + (tnz & 1)) as usize;
                let nz = get_coeffs(
                    br,
                    &proba[2],
                    ctx,
                    q.uv,
                    0,
                    &mut block.coeffs[dst..dst + 16],
                );
                l = u32::from(nz > 0);
                tnz = (tnz >> 1) | (l << 3);
                nz_coeffs = nz_code(nz_coeffs, nz, block.coeffs[dst] != 0);
                dst += 16;
            }
            tnz >>= 2;
            lnz = (lnz >> 1) | (l << 5);
        }
        non_zero_uv |= nz_coeffs << (4 * ch);
        out_t_nz |= (tnz << 4) << ch;
        out_l_nz |= (lnz & 0xf0) << ch;
    }
    // VP8MB's nz fields are uint8_t
    top.0 = out_t_nz & 0xff;
    left.0 = out_l_nz & 0xff;
    block.non_zero_y = non_zero_y;
    block.non_zero_uv = non_zero_uv;
    non_zero_y | non_zero_uv == 0
}

/// dec.c `TransformWHT_C`: the second-order transform of the 16 luma DC values.
fn transform_wht(input: &[i16; 16], out: &mut [i16; 384]) {
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let a0 = i32::from(input[i]) + i32::from(input[12 + i]);
        let a1 = i32::from(input[4 + i]) + i32::from(input[8 + i]);
        let a2 = i32::from(input[4 + i]) - i32::from(input[8 + i]);
        let a3 = i32::from(input[i]) - i32::from(input[12 + i]);
        tmp[i] = a0 + a1;
        tmp[8 + i] = a0 - a1;
        tmp[4 + i] = a3 + a2;
        tmp[12 + i] = a3 - a2;
    }
    for i in 0..4 {
        let dc = tmp[i * 4] + 3;
        let a0 = dc + tmp[3 + i * 4];
        let a1 = tmp[1 + i * 4] + tmp[2 + i * 4];
        let a2 = tmp[1 + i * 4] - tmp[2 + i * 4];
        let a3 = dc - tmp[3 + i * 4];
        let o = 64 * i;
        out[o] = ((a0 + a1) >> 3) as i16;
        out[o + 16] = ((a3 + a2) >> 3) as i16;
        out[o + 32] = ((a0 - a1) >> 3) as i16;
        out[o + 48] = ((a3 - a2) >> 3) as i16;
    }
}

// The transforms compute in i64: C's int arithmetic is exact for every valid stream; on corrupt
// coefficients its overflow is undefined, and this neither panics nor wraps.
const fn mul1(a: i64) -> i64 {
    ((a * 20091) >> 16) + a
}

const fn mul2(a: i64) -> i64 {
    (a * 35468) >> 16
}

fn clip_8b(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

fn store(b: &mut [u8], at: usize, v: i64) {
    b[at] = (i64::from(b[at]) + (v >> 3)).clamp(0, 255) as u8;
}

/// dec.c `TransformOne_C`: inverse DCT of one 4×4 block added to the prediction at `at`.
fn transform_one(input: &[i16], b: &mut [u8], at: usize) {
    let mut c = [0i64; 16];
    for i in 0..4 {
        let x = |k: usize| i64::from(input[i + k]);
        let (a, bb) = (x(0) + x(8), x(0) - x(8));
        let cc = mul2(x(4)) - mul1(x(12));
        let d = mul1(x(4)) + mul2(x(12));
        c[4 * i..4 * i + 4].copy_from_slice(&[a + d, bb + cc, bb - cc, a - d]);
    }
    for i in 0..4 {
        let dc = c[i] + 4;
        let (a, bb) = (dc + c[8 + i], dc - c[8 + i]);
        let cc = mul2(c[4 + i]) - mul1(c[12 + i]);
        let d = mul1(c[4 + i]) + mul2(c[12 + i]);
        let row = at + i * BPS;
        store(b, row, a + d);
        store(b, row + 1, bb + cc);
        store(b, row + 2, bb - cc);
        store(b, row + 3, a - d);
    }
}

/// dec.c `TransformAC3_C`: only coefficients 0, 1 and 4 are non-zero.
fn transform_ac3(input: &[i16], b: &mut [u8], at: usize) {
    let a = i64::from(input[0]) + 4;
    let (c4, d4) = (mul2(i64::from(input[4])), mul1(i64::from(input[4])));
    let (c1, d1) = (mul2(i64::from(input[1])), mul1(i64::from(input[1])));
    for (y, dc) in [a + d4, a + c4, a - c4, a - d4].into_iter().enumerate() {
        let row = at + y * BPS;
        store(b, row, dc + d1);
        store(b, row + 1, dc + c1);
        store(b, row + 2, dc - c1);
        store(b, row + 3, dc - d1);
    }
}

/// dec.c `TransformDC_C`.
fn transform_dc(input: &[i16], b: &mut [u8], at: usize) {
    let dc = i64::from(input[0]) + 4;
    for j in 0..4 {
        for i in 0..4 {
            store(b, at + i + j * BPS, dc);
        }
    }
}

/// frame_dec.c `DoTransform`: by the two bits the coefficient parser left for the block.
fn do_transform(bits: u32, input: &[i16], b: &mut [u8], at: usize) {
    match bits >> 30 {
        3 => transform_one(input, b, at),
        2 => transform_ac3(input, b, at),
        1 => transform_dc(input, b, at),
        _ => {}
    }
}

/// frame_dec.c `DoUVTransform`.
fn do_uv_transform(bits: u32, input: &[i16], b: &mut [u8], at: usize) {
    if bits & 0xff == 0 {
        return;
    }
    let offsets = [0, 4, 4 * BPS, 4 * BPS + 4];
    if bits & 0xaa != 0 {
        for (k, o) in offsets.into_iter().enumerate() {
            transform_one(&input[16 * k..], b, at + o);
        }
    } else {
        for (k, o) in offsets.into_iter().enumerate() {
            if input[16 * k] != 0 {
                transform_dc(&input[16 * k..], b, at + o);
            }
        }
    }
}

/// frame_dec.c `ReconstructRow` (single-threaded): predictions and residuals in the 32-wide work buffer,
/// then the macroblock copied into the planes.
#[allow(clippy::too_many_arguments)]
fn reconstruct_row(
    row: &[MbData],
    mb_y: usize,
    mb_w: usize,
    mb_h: usize,
    b: &mut [u8; YUV_SIZE],
    yuv_t: &mut [([u8; 16], [u8; 8], [u8; 8])],
    yp: &mut [u8],
    up: &mut [u8],
    vp: &mut [u8],
) {
    for j in 0..16 {
        b[Y_OFF + j * BPS - 1] = 129;
    }
    for j in 0..8 {
        b[U_OFF + j * BPS - 1] = 129;
        b[V_OFF + j * BPS - 1] = 129;
    }
    if mb_y > 0 {
        b[Y_OFF - BPS - 1] = 129;
        b[U_OFF - BPS - 1] = 129;
        b[V_OFF - BPS - 1] = 129;
    } else {
        b[Y_OFF - BPS - 1..Y_OFF - BPS + 20].fill(127);
        b[U_OFF - BPS - 1..U_OFF - BPS + 8].fill(127);
        b[V_OFF - BPS - 1..V_OFF - BPS + 8].fill(127);
    }
    let (yw, uvw) = (mb_w * 16, mb_w * 8);
    for (mb_x, block) in row.iter().enumerate() {
        if mb_x > 0 {
            for j in -1i32..16 {
                let r = (Y_OFF as i32 + j * BPS as i32) as usize;
                b.copy_within(r + 12..r + 16, r - 4);
            }
            for j in -1i32..8 {
                for off in [U_OFF, V_OFF] {
                    let r = (off as i32 + j * BPS as i32) as usize;
                    b.copy_within(r + 4..r + 8, r - 4);
                }
            }
        }
        if mb_y > 0 {
            b[Y_OFF - BPS..Y_OFF - BPS + 16].copy_from_slice(&yuv_t[mb_x].0);
            b[U_OFF - BPS..U_OFF - BPS + 8].copy_from_slice(&yuv_t[mb_x].1);
            b[V_OFF - BPS..V_OFF - BPS + 8].copy_from_slice(&yuv_t[mb_x].2);
        }
        let coeffs = &block.coeffs;
        let mut bits = block.non_zero_y;
        if block.is_i4x4 {
            let tr = Y_OFF - BPS + 16;
            if mb_y > 0 {
                if mb_x + 1 >= mb_w {
                    b[tr..tr + 4].fill(yuv_t[mb_x].0[15]);
                } else {
                    b[tr..tr + 4].copy_from_slice(&yuv_t[mb_x + 1].0[..4]);
                }
            }
            for k in 1..4 {
                b.copy_within(tr..tr + 4, tr + 4 * k * BPS);
            }
            for n in 0..16 {
                let at = Y_OFF + (n % 4) * 4 + (n / 4) * 4 * BPS;
                predict4(block.imodes[n], b, at);
                do_transform(bits, &coeffs[n * 16..], b, at);
                bits <<= 2;
            }
        } else {
            predict_square(check_mode(mb_x, mb_y, block.imodes[0]), b, Y_OFF, 16);
            if bits != 0 {
                for n in 0..16 {
                    let at = Y_OFF + (n % 4) * 4 + (n / 4) * 4 * BPS;
                    do_transform(bits, &coeffs[n * 16..], b, at);
                    bits <<= 2;
                }
            }
        }
        let uv_mode = check_mode(mb_x, mb_y, block.uvmode);
        predict_square(uv_mode, b, U_OFF, 8);
        predict_square(uv_mode, b, V_OFF, 8);
        do_uv_transform(block.non_zero_uv, &coeffs[256..], b, U_OFF);
        do_uv_transform(block.non_zero_uv >> 8, &coeffs[320..], b, V_OFF);
        if mb_y + 1 < mb_h {
            let t = &mut yuv_t[mb_x];
            t.0.copy_from_slice(&b[Y_OFF + 15 * BPS..Y_OFF + 15 * BPS + 16]);
            t.1.copy_from_slice(&b[U_OFF + 7 * BPS..U_OFF + 7 * BPS + 8]);
            t.2.copy_from_slice(&b[V_OFF + 7 * BPS..V_OFF + 7 * BPS + 8]);
        }
        for j in 0..16 {
            let o = (mb_y * 16 + j) * yw + mb_x * 16;
            yp[o..o + 16].copy_from_slice(&b[Y_OFF + j * BPS..Y_OFF + j * BPS + 16]);
        }
        for j in 0..8 {
            let o = (mb_y * 8 + j) * uvw + mb_x * 8;
            up[o..o + 8].copy_from_slice(&b[U_OFF + j * BPS..U_OFF + j * BPS + 8]);
            vp[o..o + 8].copy_from_slice(&b[V_OFF + j * BPS..V_OFF + j * BPS + 8]);
        }
    }
}

/// frame_dec.c `CheckMode`: the DC predictor for macroblocks without top or left neighbours.
fn check_mode(mb_x: usize, mb_y: usize, mode: u8) -> u8 {
    match (mode, mb_x == 0, mb_y == 0) {
        (0, true, true) => 6,
        (0, true, false) => 5,
        (0, false, true) => 4,
        _ => mode,
    }
}

/// dec.c's 16×16 luma and 8×8 chroma predictors: 0 DC, 1 TM, 2 VE, 3 HE, 4 DC without top, 5 DC without
/// left, 6 DC with neither.
fn predict_square(mode: u8, b: &mut [u8], at: usize, size: usize) {
    let shift = if size == 16 { 5 } else { 4 };
    let put = |b: &mut [u8], v: u8| {
        for j in 0..size {
            b[at + j * BPS..at + j * BPS + size].fill(v);
        }
    };
    let top_sum = |b: &[u8]| (0..size).map(|i| u32::from(b[at - BPS + i])).sum::<u32>();
    let left_sum = |b: &[u8]| {
        (0..size)
            .map(|j| u32::from(b[at + j * BPS - 1]))
            .sum::<u32>()
    };
    match mode {
        0 => {
            let dc = (top_sum(b) + left_sum(b) + size as u32) >> shift;
            put(b, dc as u8);
        }
        1 => true_motion(b, at, size),
        2 => {
            for j in 0..size {
                b.copy_within(at - BPS..at - BPS + size, at + j * BPS);
            }
        }
        3 => {
            for j in 0..size {
                let v = b[at + j * BPS - 1];
                b[at + j * BPS..at + j * BPS + size].fill(v);
            }
        }
        4 => {
            let dc = (left_sum(b) + (size as u32 >> 1)) >> (shift - 1);
            put(b, dc as u8);
        }
        5 => {
            let dc = (top_sum(b) + (size as u32 >> 1)) >> (shift - 1);
            put(b, dc as u8);
        }
        _ => put(b, 0x80),
    }
}

/// dec.c `TrueMotion`: top + left − top-left, clipped.
fn true_motion(b: &mut [u8], at: usize, size: usize) {
    let tl = i32::from(b[at - BPS - 1]);
    for y in 0..size {
        let left = i32::from(b[at + y * BPS - 1]);
        for x in 0..size {
            let top = i32::from(b[at - BPS + x]);
            b[at + y * BPS + x] = clip_8b(top + left - tl);
        }
    }
}

/// dec.c's 4×4 luma predictors (`VP8PredLuma4`): 0 DC, 1 TM, 2 VE, 3 HE, 4 RD, 5 VR, 6 LD, 7 VL, 8 HD,
/// 9 HU.
fn predict4(mode: u8, b: &mut [u8], at: usize) {
    let avg3 =
        |a: u8, x: u8, c: u8| ((u32::from(a) + 2 * u32::from(x) + u32::from(c) + 2) >> 2) as u8;
    let avg2 = |a: u8, x: u8| ((u32::from(a) + u32::from(x) + 1) >> 1) as u8;
    let top = |b: &[u8], i: usize| b[at - BPS + i];
    let (i_, j_, k_, l_) = (
        b[at - 1],
        b[at + BPS - 1],
        b[at + 2 * BPS - 1],
        b[at + 3 * BPS - 1],
    );
    let x_ = b[at - BPS - 1];
    let (a_, b_, c_, d_) = (top(b, 0), top(b, 1), top(b, 2), top(b, 3));
    let (e_, f_, g_, h_) = (top(b, 4), top(b, 5), top(b, 6), top(b, 7));
    let mut set = |x: usize, y: usize, v: u8| b[at + x + y * BPS] = v;
    match mode {
        0 => {
            let sum: u32 = [a_, b_, c_, d_, i_, j_, k_, l_]
                .iter()
                .map(|&v| u32::from(v))
                .sum();
            let dc = ((sum + 4) >> 3) as u8;
            for y in 0..4 {
                for x in 0..4 {
                    set(x, y, dc);
                }
            }
        }
        1 => true_motion(b, at, 4),
        2 => {
            let vals = [
                avg3(x_, a_, b_),
                avg3(a_, b_, c_),
                avg3(b_, c_, d_),
                avg3(c_, d_, e_),
            ];
            for y in 0..4 {
                for (x, &v) in vals.iter().enumerate() {
                    set(x, y, v);
                }
            }
        }
        3 => {
            let vals = [
                avg3(x_, i_, j_),
                avg3(i_, j_, k_),
                avg3(j_, k_, l_),
                avg3(k_, l_, l_),
            ];
            for (y, &v) in vals.iter().enumerate() {
                for x in 0..4 {
                    set(x, y, v);
                }
            }
        }
        4 => {
            // RD4
            set(0, 3, avg3(j_, k_, l_));
            let v = avg3(i_, j_, k_);
            set(1, 3, v);
            set(0, 2, v);
            let v = avg3(x_, i_, j_);
            set(2, 3, v);
            set(1, 2, v);
            set(0, 1, v);
            let v = avg3(a_, x_, i_);
            set(3, 3, v);
            set(2, 2, v);
            set(1, 1, v);
            set(0, 0, v);
            let v = avg3(b_, a_, x_);
            set(3, 2, v);
            set(2, 1, v);
            set(1, 0, v);
            let v = avg3(c_, b_, a_);
            set(3, 1, v);
            set(2, 0, v);
            set(3, 0, avg3(d_, c_, b_));
        }
        5 => {
            // VR4
            let v = avg2(x_, a_);
            set(0, 0, v);
            set(1, 2, v);
            let v = avg2(a_, b_);
            set(1, 0, v);
            set(2, 2, v);
            let v = avg2(b_, c_);
            set(2, 0, v);
            set(3, 2, v);
            set(3, 0, avg2(c_, d_));
            set(0, 3, avg3(k_, j_, i_));
            set(0, 2, avg3(j_, i_, x_));
            let v = avg3(i_, x_, a_);
            set(0, 1, v);
            set(1, 3, v);
            let v = avg3(x_, a_, b_);
            set(1, 1, v);
            set(2, 3, v);
            let v = avg3(a_, b_, c_);
            set(2, 1, v);
            set(3, 3, v);
            set(3, 1, avg3(b_, c_, d_));
        }
        6 => {
            // LD4
            set(0, 0, avg3(a_, b_, c_));
            let v = avg3(b_, c_, d_);
            set(1, 0, v);
            set(0, 1, v);
            let v = avg3(c_, d_, e_);
            set(2, 0, v);
            set(1, 1, v);
            set(0, 2, v);
            let v = avg3(d_, e_, f_);
            set(3, 0, v);
            set(2, 1, v);
            set(1, 2, v);
            set(0, 3, v);
            let v = avg3(e_, f_, g_);
            set(3, 1, v);
            set(2, 2, v);
            set(1, 3, v);
            let v = avg3(f_, g_, h_);
            set(3, 2, v);
            set(2, 3, v);
            set(3, 3, avg3(g_, h_, h_));
        }
        7 => {
            // VL4
            set(0, 0, avg2(a_, b_));
            let v = avg2(b_, c_);
            set(1, 0, v);
            set(0, 2, v);
            let v = avg2(c_, d_);
            set(2, 0, v);
            set(1, 2, v);
            let v = avg2(d_, e_);
            set(3, 0, v);
            set(2, 2, v);
            set(0, 1, avg3(a_, b_, c_));
            let v = avg3(b_, c_, d_);
            set(1, 1, v);
            set(0, 3, v);
            let v = avg3(c_, d_, e_);
            set(2, 1, v);
            set(1, 3, v);
            let v = avg3(d_, e_, f_);
            set(3, 1, v);
            set(2, 3, v);
            set(3, 2, avg3(e_, f_, g_));
            set(3, 3, avg3(f_, g_, h_));
        }
        8 => {
            // HD4
            let v = avg2(i_, x_);
            set(0, 0, v);
            set(2, 1, v);
            let v = avg2(j_, i_);
            set(0, 1, v);
            set(2, 2, v);
            let v = avg2(k_, j_);
            set(0, 2, v);
            set(2, 3, v);
            set(0, 3, avg2(l_, k_));
            set(3, 0, avg3(a_, b_, c_));
            set(2, 0, avg3(x_, a_, b_));
            let v = avg3(i_, x_, a_);
            set(1, 0, v);
            set(3, 1, v);
            let v = avg3(j_, i_, x_);
            set(1, 1, v);
            set(3, 2, v);
            let v = avg3(k_, j_, i_);
            set(1, 2, v);
            set(3, 3, v);
            set(1, 3, avg3(l_, k_, j_));
        }
        _ => {
            // HU4
            set(0, 0, avg2(i_, j_));
            let v = avg2(j_, k_);
            set(2, 0, v);
            set(0, 1, v);
            let v = avg2(k_, l_);
            set(2, 1, v);
            set(0, 2, v);
            set(1, 0, avg3(i_, j_, k_));
            let v = avg3(j_, k_, l_);
            set(3, 0, v);
            set(1, 1, v);
            let v = avg3(k_, l_, l_);
            set(3, 1, v);
            set(1, 2, v);
            for (x, y) in [(3, 2), (2, 2), (0, 3), (1, 3), (2, 3), (3, 3)] {
                set(x, y, l_);
            }
        }
    }
}

// ------------------------------------------------------------------------------------- loop filter

fn sclip1(v: i32) -> i32 {
    v.clamp(-128, 127)
}

fn sclip2(v: i32) -> i32 {
    v.clamp(-16, 15)
}

fn uclip(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// dec.c `DoFilter2_C`: across the edge before `p` (`step` apart), the two pixels nearest it.
fn do_filter2(b: &mut [u8], p: usize, step: usize) {
    let (p1, p0, q0, q1) = (
        i32::from(b[p - 2 * step]),
        i32::from(b[p - step]),
        i32::from(b[p]),
        i32::from(b[p + step]),
    );
    let a = 3 * (q0 - p0) + sclip1(p1 - q1);
    let a1 = sclip2((a + 4) >> 3);
    let a2 = sclip2((a + 3) >> 3);
    b[p - step] = uclip(p0 + a2);
    b[p] = uclip(q0 - a1);
}

/// dec.c `DoFilter4_C`.
fn do_filter4(b: &mut [u8], p: usize, step: usize) {
    let (p1, p0, q0, q1) = (
        i32::from(b[p - 2 * step]),
        i32::from(b[p - step]),
        i32::from(b[p]),
        i32::from(b[p + step]),
    );
    let a = 3 * (q0 - p0);
    let a1 = sclip2((a + 4) >> 3);
    let a2 = sclip2((a + 3) >> 3);
    let a3 = (a1 + 1) >> 1;
    b[p - 2 * step] = uclip(p1 + a3);
    b[p - step] = uclip(p0 + a2);
    b[p] = uclip(q0 - a1);
    b[p + step] = uclip(q1 - a3);
}

/// dec.c `DoFilter6_C`.
fn do_filter6(b: &mut [u8], p: usize, step: usize) {
    let g = |k: isize| i32::from(b[(p as isize + k * step as isize) as usize]);
    let (p2, p1, p0, q0, q1, q2) = (g(-3), g(-2), g(-1), g(0), g(1), g(2));
    let a = sclip1(3 * (q0 - p0) + sclip1(p1 - q1));
    let a1 = (27 * a + 63) >> 7;
    let a2 = (18 * a + 63) >> 7;
    let a3 = (9 * a + 63) >> 7;
    b[p - 3 * step] = uclip(p2 + a3);
    b[p - 2 * step] = uclip(p1 + a2);
    b[p - step] = uclip(p0 + a1);
    b[p] = uclip(q0 - a1);
    b[p + step] = uclip(q1 - a2);
    b[p + 2 * step] = uclip(q2 - a3);
}

/// dec.c `SimpleVFilter16_C` / `SimpleHFilter16_C`: 16 positions `along` apart, filtering across `step`.
fn simple_filter16(b: &mut [u8], p: usize, step: usize, along: usize, thresh: i32) {
    let thresh2 = 2 * thresh + 1;
    for i in 0..16 {
        let q = p + i * along;
        let (p1, p0, q0, q1) = (
            i32::from(b[q - 2 * step]),
            i32::from(b[q - step]),
            i32::from(b[q]),
            i32::from(b[q + step]),
        );
        if 4 * (p0 - q0).abs() + (p1 - q1).abs() <= thresh2 {
            do_filter2(b, q, step);
        }
    }
}

/// dec.c `FilterLoop26_C` (macroblock edges, `edge`) and `FilterLoop24_C` (inner edges).
#[allow(clippy::too_many_arguments)]
fn filter_loop(
    b: &mut [u8],
    p: usize,
    step: usize,
    along: usize,
    size: usize,
    thresh: i32,
    ithresh: i32,
    hev_thresh: i32,
    edge: bool,
) {
    let thresh2 = 2 * thresh + 1;
    for i in 0..size {
        let q = p + i * along;
        let g = |k: isize| i32::from(b[(q as isize + k * step as isize) as usize]);
        let (p3, p2, p1, p0, q0, q1, q2, q3) = (g(-4), g(-3), g(-2), g(-1), g(0), g(1), g(2), g(3));
        if 4 * (p0 - q0).abs() + (p1 - q1).abs() > thresh2 {
            continue;
        }
        let inner = [(p3, p2), (p2, p1), (p1, p0), (q3, q2), (q2, q1), (q1, q0)];
        if inner.iter().any(|&(a, c)| (a - c).abs() > ithresh) {
            continue;
        }
        if (p1 - p0).abs() > hev_thresh || (q1 - q0).abs() > hev_thresh {
            do_filter2(b, q, step);
        } else if edge {
            do_filter6(b, q, step);
        } else {
            do_filter4(b, q, step);
        }
    }
}

// ------------------------------------------------------------------------------------------- output

/// yuv.h `VP8YuvToRgb`.
fn yuv_to_rgb(y: u32, u: u32, v: u32) -> [u8; 3] {
    let (y, u, v) = (y as i32, u as i32, v as i32);
    let mult = |v: i32, c: i32| (v * c) >> 8;
    let clip8 = |v: i32| {
        if v & !((256 << 6) - 1) == 0 {
            (v >> 6) as u8
        } else if v < 0 {
            0
        } else {
            255
        }
    };
    [
        clip8(mult(y, 19077) + mult(v, 26149) - 14234),
        clip8(mult(y, 19077) - mult(u, 6419) - mult(v, 13320) + 8708),
        clip8(mult(y, 19077) + mult(u, 33050) - 17685),
    ]
}

/// upsampling.c `UpsampleRgbaLinePair`: one or two luma rows from the chroma rows above (`tu`, `tv`) and
/// below (`cu`, `cv`), written as RGBA (alpha left as is).
#[allow(clippy::too_many_arguments)]
fn upsample_pair(
    top_y: &[u8],
    bottom_y: Option<&[u8]>,
    tu: &[u8],
    tv: &[u8],
    cu: &[u8],
    cv: &[u8],
    top_dst: &mut [u8],
    mut bottom_dst: Option<&mut [u8]>,
    len: usize,
) {
    let load = |u: &[u8], v: &[u8], i: usize| u32::from(u[i]) | u32::from(v[i]) << 16;
    let put = |dst: &mut [u8], x: usize, y: u8, uv: u32| {
        dst[4 * x..4 * x + 3].copy_from_slice(&yuv_to_rgb(u32::from(y), uv & 0xff, uv >> 16));
    };
    let last_pair = (len - 1) >> 1;
    let mut tl = load(tu, tv, 0);
    let mut l = load(cu, cv, 0);
    put(top_dst, 0, top_y[0], (3 * tl + l + 0x0002_0002) >> 2);
    if let (Some(by), Some(bd)) = (bottom_y, bottom_dst.as_deref_mut()) {
        put(bd, 0, by[0], (3 * l + tl + 0x0002_0002) >> 2);
    }
    for x in 1..=last_pair {
        let t = load(tu, tv, x);
        let uv = load(cu, cv, x);
        let avg = tl + t + l + uv + 0x0008_0008;
        let diag_12 = (avg + 2 * (t + l)) >> 3;
        let diag_03 = (avg + 2 * (tl + uv)) >> 3;
        put(top_dst, 2 * x - 1, top_y[2 * x - 1], (diag_12 + tl) >> 1);
        put(top_dst, 2 * x, top_y[2 * x], (diag_03 + t) >> 1);
        if let (Some(by), Some(bd)) = (bottom_y, bottom_dst.as_deref_mut()) {
            put(bd, 2 * x - 1, by[2 * x - 1], (diag_03 + l) >> 1);
            put(bd, 2 * x, by[2 * x], (diag_12 + uv) >> 1);
        }
        tl = t;
        l = uv;
    }
    if len & 1 == 0 {
        put(
            top_dst,
            len - 1,
            top_y[len - 1],
            (3 * tl + l + 0x0002_0002) >> 2,
        );
        if let (Some(by), Some(bd)) = (bottom_y, bottom_dst) {
            put(bd, len - 1, by[len - 1], (3 * l + tl + 0x0002_0002) >> 2);
        }
    }
}

/// io_dec.c `EmitFancyRGB` over the whole frame: row 0 alone, then rows (2k−1, 2k) from chroma rows
/// (k−1, k), and for an even height the last row alone; `EmitAlphaRGB` writes the alpha plane.
#[allow(clippy::too_many_arguments)]
fn fancy_rgba(
    yp: &[u8],
    yw: usize,
    up: &[u8],
    vp: &[u8],
    uvw: usize,
    w: usize,
    h: usize,
    alpha: Option<&[u8]>,
) -> Vec<u8> {
    let mut out = vec![255u8; 4 * w * h];
    let yrow = |y: usize| &yp[y * yw..y * yw + w];
    fn chroma(p: &[u8], k: usize, uvw: usize, n: usize) -> &[u8] {
        &p[k * uvw..k * uvw + n]
    }
    let crow = |p, k| chroma(p, k, uvw, w.div_ceil(2));
    {
        let (first, _) = out.split_at_mut(4 * w);
        upsample_pair(
            yrow(0),
            None,
            crow(up, 0),
            crow(vp, 0),
            crow(up, 0),
            crow(vp, 0),
            first,
            None,
            w,
        );
    }
    let mut k = 1;
    while 2 * k < h {
        let (head, tail) = out.split_at_mut(4 * w * 2 * k);
        let top_dst = &mut head[4 * w * (2 * k - 1)..];
        let bottom_dst = &mut tail[..4 * w];
        upsample_pair(
            yrow(2 * k - 1),
            Some(yrow(2 * k)),
            crow(up, k - 1),
            crow(vp, k - 1),
            crow(up, k),
            crow(vp, k),
            top_dst,
            Some(bottom_dst),
            w,
        );
        k += 1;
    }
    if h.is_multiple_of(2) {
        let c = h / 2 - 1;
        let last = &mut out[4 * w * (h - 1)..];
        upsample_pair(
            yrow(h - 1),
            None,
            crow(up, c),
            crow(vp, c),
            crow(up, c),
            crow(vp, c),
            last,
            None,
            w,
        );
    }
    if let Some(a) = alpha {
        for (px, &v) in out.as_chunks_mut::<4>().0.iter_mut().zip(a) {
            px[3] = v;
        }
    }
    out
}
