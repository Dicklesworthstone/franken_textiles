//! JPEG decoding as the reference's `load_image` sees it. Pillow 12.3 opens a JPEG with its bundled
//! libjpeg-turbo 3.1.4.1 at the library's defaults (accurate integer IDCT, fancy upsampling, block smoothing
//! allowed) and asks for `L` or `RGB`. This is a port of the path that takes:
//! - the marker reader, `jdmarker.c`;
//! - sequential and progressive Huffman decoding into a full coefficient buffer (`jdhuff.c`, `jdphuff.c`,
//!   `jdcoefct.c`);
//! - the islow IDCT and its wrapping range limit (`jidctint.c`, `jdmaster.c`);
//! - the upsamplers, with edge-replicated context rows (`jdsample.c`, `jdmainct.c`);
//! - the YCbCr→RGB tables (`jdcolor.c`). They give the same values as the Arm Neon code Pillow runs on
//!   arm64 for every Cb and Cr.
//!
//! Damaged data is handled the way libjpeg handles it:
//! - zero bits are supplied after a premature marker, and the rest of the segment is left zero;
//! - restart markers are resynchronised;
//! - garbage before a marker is skipped;
//! - a file that ends inside the data it needs is refused, as Pillow refuses it ("image file is truncated").
//!
//! Refused here, because they are not ported:
//! - arithmetic, lossless and hierarchical files;
//! - CMYK and YCCK (Pillow's mode `CMYK`);
//! - progressive files whose low coefficients are incomplete. libjpeg smooths those (`decompress_smooth_data`).
//!
//! Pillow's own header walk refuses three things libjpeg would take, and they are refused here too:
//! - precision other than 8;
//! - component counts other than 1, 3 and 4;
//! - a TEM marker before the first scan.
//!
//! Dequantized coefficients beyond 16 bits (only crafted files have them) follow the C code. Pillow's SIMD
//! IDCT wraps those at 16 bits instead.
//!
//! Provenance: libjpeg-turbo 3.1.4.1 `src/` (IJG License and Modified BSD; `LICENSE-libjpeg-turbo.md` in this
//! crate). This software is based in part on the work of the Independent JPEG Group.

use super::image::Bgr;

/// The refusal for a file that ends inside data the decoder needs (Pillow: "image file is truncated").
const TRUNCATED: &str = "JPEG: the file is truncated";
/// jdhuff.h `MIN_GET_BITS` for a 64-bit bit buffer.
const MIN_GET_BITS: i32 = 57;

/// jutils.c `jpeg_natural_order`, with the 16 guard entries that absorb corrupt run lengths.
const NATURAL: [usize; 80] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63,
    63, 63, 63, 63, 63, 63,
];

/// A DHT table as defined: code counts per length (`bits[1..=16]`) and the symbols.
#[derive(Clone)]
struct Huffman {
    bits: [u8; 17],
    val: [u8; 256],
}

impl Huffman {
    /// jstdhuff.c's table for `bits` and `vals`.
    fn standard(bits: [u8; 17], vals: &[u8]) -> Self {
        let mut val = [0u8; 256];
        val[..vals.len()].copy_from_slice(vals);
        Self { bits, val }
    }
}

/// jdhuff.h `d_derived_tbl`: canonical decoding tables with an 8-bit lookahead.
struct Derived {
    maxcode: [i64; 18],
    valoffset: [i64; 18],
    /// `(length << 8) | symbol` per 8-bit prefix; length 9 means "longer than 8 bits".
    lookup: [u16; 256],
    val: [u8; 256],
}

impl Derived {
    /// jdhuff.c `jpeg_make_d_derived_tbl`.
    fn new(t: Option<&Huffman>, dc: bool) -> Result<Self, String> {
        let t = t.ok_or("JPEG: a scan uses an undefined Huffman table")?;
        let bad = || "JPEG: bogus Huffman table definition".to_string();
        let mut size = [0u8; 257];
        let mut code = [0u32; 257];
        let mut p = 0usize;
        for l in 1..=16u8 {
            let i = usize::from(t.bits[usize::from(l)]);
            if p + i > 256 {
                return Err(bad());
            }
            size[p..p + i].fill(l);
            p += i;
        }
        let n = p;
        let (mut c, mut si, mut p) = (0u32, u32::from(size[0]), 0usize);
        while size[p] != 0 {
            while u32::from(size[p]) == si {
                code[p] = c;
                p += 1;
                c += 1;
            }
            if u64::from(c) >= 1u64 << si {
                return Err(bad());
            }
            c <<= 1;
            si += 1;
        }
        let mut d = Self {
            maxcode: [0; 18],
            valoffset: [0; 18],
            lookup: [9 << 8; 256],
            val: t.val,
        };
        let mut p = 0usize;
        for l in 1..=16 {
            if t.bits[l] == 0 {
                d.maxcode[l] = -1;
            } else {
                d.valoffset[l] = p as i64 - i64::from(code[p]);
                p += usize::from(t.bits[l]);
                d.maxcode[l] = i64::from(code[p - 1]);
            }
        }
        d.maxcode[17] = 0xF_FFFF;
        let mut p = 0usize;
        for l in 1..=8usize {
            for _ in 0..t.bits[l] {
                let look = (code[p] << (8 - l)) as usize;
                let entry = ((l << 8) | usize::from(t.val[p])) as u16;
                d.lookup[look..look + (1 << (8 - l))].fill(entry);
                p += 1;
            }
        }
        if dc && t.val[..n].iter().any(|&s| s > 15) {
            return Err(bad());
        }
        Ok(d)
    }
}

/// jdhuff.h `HUFF_EXTEND`: the signed value of `s` raw bits `r`.
fn extend(r: i32, s: i32) -> i32 {
    if r < 1 << (s - 1) {
        r - (1 << s) + 1
    } else {
        r
    }
}

/// The input: the byte position, libjpeg's `unread_marker` (0 for none) and the entropy decoder's bit buffer.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    marker: u8,
    buf: u64,
    left: i32,
    /// `insufficient_data`: a marker cut the segment short and zeros were supplied.
    insufficient: bool,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.data.get(self.pos).ok_or(TRUNCATED)?;
        self.pos += 1;
        Ok(b)
    }

    fn word(&mut self) -> Result<i64, String> {
        Ok(i64::from(self.byte()?) << 8 | i64::from(self.byte()?))
    }

    /// Pillow's `skip_input_data`: a skip past the end leaves the reader at the end.
    fn skip(&mut self, n: i64) {
        if n > 0 {
            let n = usize::try_from(n).unwrap_or(usize::MAX);
            self.pos = self.pos.saturating_add(n).min(self.data.len());
        }
    }

    /// jdmarker.c `next_marker`: skips to the next `FF xx` with `xx` neither 0 nor `FF`.
    fn next_marker(&mut self) -> Result<(), String> {
        loop {
            let mut c = self.byte()?;
            while c != 0xFF {
                c = self.byte()?;
            }
            loop {
                c = self.byte()?;
                if c != 0xFF {
                    break;
                }
            }
            if c != 0 {
                self.marker = c;
                return Ok(());
            }
        }
    }

    /// jdhuff.c `jpeg_fill_bit_buffer`: at least [`MIN_GET_BITS`] bits unless a marker ends the segment,
    /// after which zeros are supplied for any bits the request lacks.
    fn fill(&mut self, nbits: i32) -> Result<(), String> {
        if self.marker == 0 {
            while self.left < MIN_GET_BITS {
                let mut c = self.byte()?;
                if c == 0xFF {
                    loop {
                        c = self.byte()?;
                        if c != 0xFF {
                            break;
                        }
                    }
                    if c == 0 {
                        c = 0xFF;
                    } else {
                        self.marker = c;
                        self.zeros(nbits);
                        return Ok(());
                    }
                }
                self.buf = (self.buf << 8) | u64::from(c);
                self.left += 8;
            }
        } else {
            self.zeros(nbits);
        }
        Ok(())
    }

    fn zeros(&mut self, nbits: i32) {
        if nbits > self.left {
            self.insufficient = true;
            self.buf <<= MIN_GET_BITS - self.left;
            self.left = MIN_GET_BITS;
        }
    }

    /// `CHECK_BIT_BUFFER` then `GET_BITS`.
    fn bits(&mut self, n: i32) -> Result<i32, String> {
        if self.left < n {
            self.fill(n)?;
        }
        self.left -= n;
        Ok(((self.buf >> self.left) as i32) & ((1 << n) - 1))
    }

    /// `HUFF_DECODE`: the lookahead table when 8 bits are at hand, else `jpeg_huff_decode` bit by bit.
    fn huff(&mut self, t: &Derived) -> Result<i32, String> {
        if self.left < 8 {
            self.fill(0)?;
            if self.left < 8 {
                return self.huff_slow(t, 1);
            }
        }
        let e = t.lookup[((self.buf >> (self.left - 8)) & 0xFF) as usize];
        let nb = i32::from(e >> 8);
        if nb <= 8 {
            self.left -= nb;
            Ok(i32::from(e & 0xFF))
        } else {
            self.huff_slow(t, nb)
        }
    }

    /// jdhuff.c `jpeg_huff_decode`; a code longer than 16 bits decodes as 0, as libjpeg's does.
    fn huff_slow(&mut self, t: &Derived, min_bits: i32) -> Result<i32, String> {
        let mut l = min_bits as usize;
        let mut code = i64::from(self.bits(min_bits)?);
        while code > t.maxcode[l] {
            code = (code << 1) | i64::from(self.bits(1)?);
            l += 1;
        }
        if l > 16 {
            return Ok(0);
        }
        Ok(i32::from(t.val[(code + t.valoffset[l]) as usize & 0xFF]))
    }

    /// jdhuff.c `process_restart` with jdmarker.c `read_restart_marker`.
    fn restart(&mut self, next: &mut u8) -> Result<(), String> {
        self.left = 0;
        if self.marker == 0 {
            self.next_marker()?;
        }
        if self.marker == 0xD0 + *next {
            self.marker = 0;
        } else {
            self.resync(*next)?;
        }
        *next = (*next + 1) & 7;
        if self.marker == 0 {
            self.insufficient = false;
        }
        Ok(())
    }

    /// jdmarker.c `jpeg_resync_to_restart`: drop the marker, scan on, or leave it to end the segment.
    fn resync(&mut self, desired: u8) -> Result<(), String> {
        let rst = |k: u8| 0xD0 + (k & 7);
        loop {
            let m = self.marker;
            let ahead = m == rst(desired.wrapping_add(1)) || m == rst(desired.wrapping_add(2));
            let behind = m == rst(desired.wrapping_sub(1)) || m == rst(desired.wrapping_sub(2));
            if m < 0xC0 || ((0xD0..=0xD7).contains(&m) && !ahead && behind) {
                // an invalid marker or an earlier restart: scan on
                self.next_marker()?;
            } else if (0xD0..=0xD7).contains(&m) && !ahead {
                // the desired restart, or one too far away: drop it and decode on
                self.marker = 0;
                return Ok(());
            } else {
                // another marker, or one of the next two restarts: this segment is empty
                return Ok(());
            }
        }
    }
}

/// How a component reaches full resolution (jdsample.c `jinit_upsampler`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Up {
    Full,
    H2v1Fancy,
    H2v1,
    H1v2Fancy,
    H2v2Fancy,
    H2v2,
    /// Box replication by these horizontal and vertical factors.
    Int(usize, usize),
}

/// One frame component with its coefficient buffer.
struct Comp {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    dc: usize,
    ac: usize,
    /// Size in blocks and in samples (`width_in_blocks`, `downsampled_width`, …).
    wib: usize,
    hib: usize,
    dw: usize,
    dh: usize,
    /// Blocks per stored row: `wib` rounded up to `h`, as interleaved scans code dummy blocks.
    stride: usize,
    /// The quantization table latched at the component's first scan.
    quant: Option<[u16; 64]>,
    coef: Vec<[i16; 64]>,
    /// Progressive `coef_bits`: the lowest bit known per coefficient, −1 for none yet.
    known: [i32; 64],
}

/// The colour space libjpeg guesses (jdapimin.c `default_decompress_parms`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Space {
    Gray,
    Ycc,
    Rgb,
}

/// What a scan's MCUs decode (jdhuff.c, jdphuff.c).
enum Mode {
    Sequential(Vec<(Derived, Derived)>),
    DcFirst(Vec<Derived>),
    DcRefine,
    AcFirst(Derived),
    AcRefine(Derived),
}

struct Decoder<'a> {
    r: Reader<'a>,
    max_pixels: usize,
    saw_soi: bool,
    progressive: bool,
    in_headers: bool,
    w: usize,
    h: usize,
    comps: Vec<Comp>,
    qt: [Option<[u16; 64]>; 4],
    dht: [[Option<Huffman>; 4]; 2],
    restart_interval: usize,
    jfif: bool,
    adobe: Option<u8>,
    /// The current scan: component indices and `Ss`, `Se`, `Ah`, `Al`.
    scan: Vec<usize>,
    ss: usize,
    se: usize,
    ah: u8,
    al: u8,
    next_restart: u8,
    max_h: usize,
    max_v: usize,
    multi_scan: bool,
}

/// Decodes a JPEG as Pillow 12.3 does for `Image.open(f).convert("RGB")`, returned as OpenCV's BGR.
///
/// # Errors
/// A damaged or truncated file, an image over `max_pixels`, or a variant not ported (see the module).
pub fn decode(bytes: &[u8], max_pixels: usize) -> Result<Bgr, String> {
    let mut d = Decoder {
        r: Reader {
            data: bytes,
            pos: 0,
            marker: 0,
            buf: 0,
            left: 0,
            insufficient: false,
        },
        max_pixels,
        saw_soi: false,
        progressive: false,
        in_headers: true,
        w: 0,
        h: 0,
        comps: Vec::new(),
        qt: [None; 4],
        dht: Default::default(),
        restart_interval: 0,
        jfif: false,
        adobe: None,
        scan: Vec::new(),
        ss: 0,
        se: 0,
        ah: 0,
        al: 0,
        next_restart: 0,
        max_h: 1,
        max_v: 1,
        multi_scan: false,
    };
    if !d.read_markers()? {
        return Err("JPEG: no image in the file".into());
    }
    let space = d.setup()?;
    d.in_headers = false;
    d.decode_scan()?;
    if d.multi_scan {
        while d.read_markers()? {
            d.decode_scan()?;
        }
    } else {
        // Every row is out by now: jpeg_finish_decompress only looks for EOI, and Pillow accepts its
        // suspension at the end of the file.
        match d.read_markers() {
            Ok(true) => return Err("JPEG: a second scan in a single-scan file".into()),
            Ok(false) => {}
            Err(e) if e == TRUNCATED => {}
            Err(e) => return Err(e),
        }
    }
    if d.progressive && d.smoothing_ok() {
        return Err(
            "JPEG: a progressive file with incomplete coefficients (libjpeg's block smoothing is not ported)"
                .into(),
        );
    }
    Ok(d.output(space))
}

impl Decoder<'_> {
    /// jdmarker.c `read_markers`: tables up to the next SOS (true) or EOI (false).
    fn read_markers(&mut self) -> Result<bool, String> {
        loop {
            if self.r.marker == 0 {
                if self.saw_soi {
                    self.r.next_marker()?;
                } else {
                    if !self.r.data.starts_with(&[0xFF, 0xD8]) {
                        return Err("not a JPEG file".into());
                    }
                    self.r.pos = 2;
                    self.r.marker = 0xD8;
                }
            }
            match self.r.marker {
                0xD8 => {
                    if self.saw_soi {
                        return Err("JPEG: a second SOI marker".into());
                    }
                    self.saw_soi = true;
                    (self.restart_interval, self.jfif, self.adobe) = (0, false, None);
                }
                m @ (0xC0..=0xC2) => self.sof(m == 0xC2)?,
                0xC3 | 0xCB => return Err("JPEG: lossless JPEG is not decoded here".into()),
                0xC9 | 0xCA => return Err("JPEG: arithmetic coding is not decoded here".into()),
                0xC5..=0xC8 | 0xCD..=0xCF => {
                    return Err(format!(
                        "JPEG: unsupported JPEG process (SOF type 0x{:02x})",
                        self.r.marker
                    ));
                }
                0xDA => {
                    self.sos()?;
                    self.r.marker = 0;
                    return Ok(true);
                }
                0xD9 => {
                    self.r.marker = 0;
                    return Ok(false);
                }
                0xCC => self.dac()?,
                0xC4 => self.dht()?,
                0xDB => self.dqt()?,
                0xDD => {
                    if self.r.word()? != 4 {
                        return Err("JPEG: bogus DRI marker length".into());
                    }
                    self.restart_interval = self.r.word()? as usize;
                }
                m @ (0xE0 | 0xEE) => self.appn(m)?,
                0xE1..=0xED | 0xEF | 0xFE | 0xDC => {
                    let len = self.r.word()? - 2;
                    self.r.skip(len);
                }
                0xD0..=0xD7 => {}
                0x01 if !self.in_headers => {}
                m => return Err(format!("JPEG: unsupported marker 0x{m:02x}")),
            }
            self.r.marker = 0;
        }
    }

    /// jdmarker.c `get_sof`, with Pillow's checks on precision and component count.
    fn sof(&mut self, progressive: bool) -> Result<(), String> {
        if !self.comps.is_empty() {
            return Err("JPEG: a second SOF marker".into());
        }
        let len = self.r.word()?;
        let precision = self.r.byte()?;
        let h = self.r.word()? as usize;
        let w = self.r.word()? as usize;
        let n = usize::from(self.r.byte()?);
        if precision != 8 {
            return Err(format!("JPEG: cannot handle {precision}-bit layers"));
        }
        if h == 0 || w == 0 || n == 0 {
            return Err("JPEG: empty image".into());
        }
        if len - 8 != 3 * n as i64 {
            return Err("JPEG: bogus SOF marker length".into());
        }
        match n {
            1 | 3 => {}
            4 => {
                return Err("JPEG: CMYK images are not decoded here: convert to RGB".into());
            }
            _ => return Err(format!("JPEG: cannot handle {n}-layer images")),
        }
        if w.checked_mul(h).is_none_or(|p| p > self.max_pixels) {
            return Err(format!(
                "JPEG {w}×{h}: more than {} pixels",
                self.max_pixels
            ));
        }
        for _ in 0..n {
            let id = self.r.byte()?;
            let f = self.r.byte()?;
            let tq = usize::from(self.r.byte()?);
            self.comps.push(Comp {
                id,
                h: usize::from(f >> 4),
                v: usize::from(f & 15),
                tq,
                dc: 0,
                ac: 0,
                wib: 0,
                hib: 0,
                dw: 0,
                dh: 0,
                stride: 0,
                quant: None,
                coef: Vec::new(),
                known: [-1; 64],
            });
        }
        (self.w, self.h, self.progressive) = (w, h, progressive);
        Ok(())
    }

    /// jdmarker.c `get_sos`, including its rule that a scan lists components in frame order.
    fn sos(&mut self) -> Result<(), String> {
        if self.comps.is_empty() {
            return Err("JPEG: SOS before SOF".into());
        }
        let len = self.r.word()?;
        let n = usize::from(self.r.byte()?);
        if len != 2 * n as i64 + 6 || !(1..=4).contains(&n) {
            return Err("JPEG: bogus SOS marker length".into());
        }
        let mut slot: [Option<usize>; 4] = [None; 4];
        for i in 0..n {
            let cc = self.r.byte()?;
            let c = self.r.byte()?;
            let ci = (0..self.comps.len().min(4))
                .find(|&ci| self.comps[ci].id == cc && slot[ci].is_none())
                .ok_or_else(|| format!("JPEG: invalid component ID {cc} in SOS"))?;
            if slot[..i].contains(&Some(ci)) {
                return Err(format!("JPEG: invalid component ID {cc} in SOS"));
            }
            slot[i] = Some(ci);
            (self.comps[ci].dc, self.comps[ci].ac) = (usize::from(c >> 4), usize::from(c & 15));
        }
        self.scan = slot[..n].iter().flatten().copied().collect();
        self.ss = usize::from(self.r.byte()?);
        self.se = usize::from(self.r.byte()?);
        let a = self.r.byte()?;
        (self.ah, self.al) = (a >> 4, a & 15);
        self.next_restart = 0;
        Ok(())
    }

    /// jdmarker.c `get_dht`.
    fn dht(&mut self) -> Result<(), String> {
        let bad = || "JPEG: bogus Huffman table definition".to_string();
        let mut len = self.r.word()? - 2;
        while len > 16 {
            let index = usize::from(self.r.byte()?);
            let mut t = Huffman {
                bits: [0; 17],
                val: [0; 256],
            };
            for b in &mut t.bits[1..] {
                *b = self.r.byte()?;
            }
            let count: usize = t.bits.iter().map(|&b| usize::from(b)).sum();
            len -= 17;
            if count > 256 || count as i64 > len {
                return Err(bad());
            }
            for v in &mut t.val[..count] {
                *v = self.r.byte()?;
            }
            len -= count as i64;
            let (class, slot) = (usize::from(index & 0x10 != 0), index & !0x10);
            if slot >= 4 {
                return Err(format!("JPEG: bogus DHT index {slot}"));
            }
            self.dht[class][slot] = Some(t);
        }
        if len != 0 {
            return Err("JPEG: bogus DHT marker length".into());
        }
        Ok(())
    }

    /// jdmarker.c `get_dqt`: tables stored in natural order.
    fn dqt(&mut self) -> Result<(), String> {
        let mut len = self.r.word()? - 2;
        while len > 0 {
            let b = self.r.byte()?;
            let (wide, n) = (b >> 4 != 0, usize::from(b & 15));
            if n >= 4 {
                return Err(format!("JPEG: bogus DQT index {n}"));
            }
            let mut t = [0u16; 64];
            for &k in &NATURAL[..64] {
                t[k] = if wide {
                    self.r.word()? as u16
                } else {
                    u16::from(self.r.byte()?)
                };
            }
            self.qt[n] = Some(t);
            len -= if wide { 129 } else { 65 };
        }
        if len != 0 {
            return Err("JPEG: bogus DQT marker length".into());
        }
        Ok(())
    }

    /// jdmarker.c `get_dac`: arithmetic conditioning is validated and otherwise unused.
    fn dac(&mut self) -> Result<(), String> {
        let mut len = self.r.word()? - 2;
        while len > 0 {
            let index = self.r.byte()?;
            let val = self.r.byte()?;
            len -= 2;
            if index >= 32 {
                return Err(format!("JPEG: bogus DAC index {index}"));
            }
            if index < 16 && val & 0x0F > val >> 4 {
                return Err(format!("JPEG: bogus DAC value 0x{val:x}"));
            }
        }
        if len != 0 {
            return Err("JPEG: bogus DAC marker length".into());
        }
        Ok(())
    }

    /// jdmarker.c `get_interesting_appn`: JFIF (APP0) and Adobe (APP14) markers.
    fn appn(&mut self, m: u8) -> Result<(), String> {
        let mut len = self.r.word()? - 2;
        let take = len.clamp(0, 14) as usize;
        let mut b = [0u8; 14];
        for x in &mut b[..take] {
            *x = self.r.byte()?;
        }
        len -= take as i64;
        if m == 0xE0 && take >= 14 && b[..5] == *b"JFIF\0" {
            self.jfif = true;
        }
        if m == 0xEE && take >= 12 && b[..5] == *b"Adobe" {
            self.adobe = Some(b[11]);
        }
        self.r.skip(len);
        Ok(())
    }

    /// jdinput.c `initial_setup` and jpeg_start_decompress's checks: component geometry, the colour space,
    /// the upsamplers, and the standard Huffman tables a sequential file may omit.
    fn setup(&mut self) -> Result<Space, String> {
        if self.w > 65500 || self.h > 65500 {
            return Err("JPEG: maximum supported image dimension is 65500 pixels".into());
        }
        if self
            .comps
            .iter()
            .any(|c| !(1..=4).contains(&c.h) || !(1..=4).contains(&c.v))
        {
            return Err("JPEG: bogus sampling factors".into());
        }
        self.max_h = self.comps.iter().map(|c| c.h).max().unwrap_or(1);
        self.max_v = self.comps.iter().map(|c| c.v).max().unwrap_or(1);
        let (w, h, mh, mv) = (self.w, self.h, self.max_h, self.max_v);
        for c in &mut self.comps {
            c.wib = (w * c.h).div_ceil(mh * 8);
            c.hib = (h * c.v).div_ceil(mv * 8);
            c.dw = (w * c.h).div_ceil(mh);
            c.dh = (h * c.v).div_ceil(mv);
            c.stride = c.wib.next_multiple_of(c.h);
            c.coef = vec![[0; 64]; c.stride * c.hib.next_multiple_of(c.v)];
        }
        self.multi_scan = self.scan.len() < self.comps.len() || self.progressive;
        for c in &self.comps {
            upsampler(c, mh, mv)?;
        }
        let space = if self.comps.len() == 1 {
            Space::Gray
        } else if self.jfif {
            Space::Ycc
        } else if let Some(t) = self.adobe {
            if t == 0 { Space::Rgb } else { Space::Ycc }
        } else {
            let ids: Vec<u8> = self.comps.iter().map(|c| c.id).collect();
            if ids == b"RGB" {
                Space::Rgb
            } else {
                Space::Ycc
            }
        };
        if !self.progressive {
            const DC_LUM: [u8; 17] = [0, 0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
            const DC_CHR: [u8; 17] = [0, 0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
            const AC_LUM: [u8; 17] = [0, 0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
            const AC_CHR: [u8; 17] = [0, 0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
            let digits: Vec<u8> = (0..12).collect();
            let std = [
                (0, 0, Huffman::standard(DC_LUM, &digits)),
                (1, 0, Huffman::standard(AC_LUM, &AC_LUM_VALS)),
                (0, 1, Huffman::standard(DC_CHR, &digits)),
                (1, 1, Huffman::standard(AC_CHR, &AC_CHR_VALS)),
            ];
            for (class, slot, t) in std {
                self.dht[class][slot].get_or_insert(t);
            }
        }
        Ok(space)
    }

    /// One scan: jdinput.c `per_scan_setup` and `latch_quant_tables`, the entropy decoder's start pass, and
    /// every MCU into the coefficient buffer (jdcoefct.c `consume_data`).
    fn decode_scan(&mut self) -> Result<(), String> {
        let n = self.scan.len();
        let (mcus_x, mcus_y) = if n == 1 {
            let c = &self.comps[self.scan[0]];
            (c.wib, c.hib)
        } else {
            (
                self.w.div_ceil(self.max_h * 8),
                self.h.div_ceil(self.max_v * 8),
            )
        };
        // (position in the scan, block row and column within the MCU)
        let mut layout = Vec::new();
        for (pos, &ci) in self.scan.iter().enumerate() {
            let (bw, bh) = if n == 1 {
                (1, 1)
            } else {
                (self.comps[ci].h, self.comps[ci].v)
            };
            for dy in 0..bh {
                for dx in 0..bw {
                    layout.push((pos, dy, dx));
                }
            }
        }
        if layout.len() > 10 {
            return Err("JPEG: sampling factors too large for an interleaved scan".into());
        }
        for &ci in &self.scan {
            let c = &mut self.comps[ci];
            if c.quant.is_none() {
                c.quant = Some(self.qt.get(c.tq).copied().flatten().ok_or_else(|| {
                    format!("JPEG: quantization table 0x{:02x} was not defined", c.tq)
                })?);
            }
        }
        let mode = self.start_pass()?;
        let Self {
            r,
            comps,
            scan,
            ss,
            se,
            al,
            restart_interval,
            next_restart,
            ..
        } = self;
        (r.left, r.buf, r.insufficient) = (0, 0, false);
        let (ss, se, al, interval) = (*ss, *se, i32::from(*al), *restart_interval);
        let (p1, m1) = (1i32 << al, -1i32 << al);
        let mut togo = interval;
        let mut last_dc = [0i32; 4];
        let mut eobrun = 0u32;
        for my in 0..mcus_y {
            for mx in 0..mcus_x {
                if interval != 0 {
                    if togo == 0 {
                        r.restart(next_restart)?;
                        (last_dc, eobrun, togo) = ([0; 4], 0, interval);
                    }
                    togo -= 1;
                }
                // out of data: the rest of the segment stays as it is (zero, or earlier scans' values),
                // except that a DC refinement reads its zeros anyway
                if r.insufficient && !matches!(mode, Mode::DcRefine) {
                    continue;
                }
                for &(pos, dy, dx) in &layout {
                    let c = &mut comps[scan[pos]];
                    let idx = if n == 1 {
                        my * c.stride + mx
                    } else {
                        (my * c.v + dy) * c.stride + mx * c.h + dx
                    };
                    let block = &mut c.coef[idx];
                    match &mode {
                        Mode::Sequential(t) => {
                            let (dct, act) = &t[pos];
                            let s = r.huff(dct)?;
                            let s = if s == 0 { 0 } else { extend(r.bits(s)?, s) };
                            last_dc[pos] = last_dc[pos].wrapping_add(s);
                            block[0] = last_dc[pos] as i16;
                            let mut k = 1;
                            while k < 64 {
                                let rs = r.huff(act)?;
                                let (run, s) = (rs >> 4, rs & 15);
                                if s != 0 {
                                    k += run as usize;
                                    block[NATURAL[k]] = extend(r.bits(s)?, s) as i16;
                                } else if run == 15 {
                                    k += 15;
                                } else {
                                    break;
                                }
                                k += 1;
                            }
                        }
                        Mode::DcFirst(t) => {
                            let s = r.huff(&t[pos])?;
                            let s = if s == 0 { 0 } else { extend(r.bits(s)?, s) };
                            last_dc[pos] = last_dc[pos]
                                .checked_add(s)
                                .ok_or("JPEG: DCT coefficient out of range")?;
                            block[0] = last_dc[pos].wrapping_shl(al as u32) as i16;
                        }
                        Mode::DcRefine => {
                            if r.bits(1)? != 0 {
                                block[0] |= p1 as i16;
                            }
                        }
                        Mode::AcFirst(t) => {
                            if eobrun > 0 {
                                eobrun -= 1;
                                continue;
                            }
                            let mut k = ss;
                            while k <= se {
                                let rs = r.huff(t)?;
                                let (run, s) = (rs >> 4, rs & 15);
                                if s != 0 {
                                    k += run as usize;
                                    let v = extend(r.bits(s)?, s);
                                    block[NATURAL[k]] = v.wrapping_shl(al as u32) as i16;
                                } else if run == 15 {
                                    k += 15;
                                } else {
                                    eobrun = 1 << run;
                                    if run != 0 {
                                        eobrun += r.bits(run)? as u32;
                                    }
                                    eobrun -= 1;
                                    break;
                                }
                                k += 1;
                            }
                        }
                        Mode::AcRefine(t) => {
                            ac_refine(r, t, block, ss, se, p1, m1, &mut eobrun)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The entropy decoder's `start_pass`: jdhuff.c `start_pass_huff_decoder` or jdphuff.c
    /// `start_pass_phuff_decoder` (its scan checks, and `coef_bits`; its warnings are ignored as Pillow does).
    fn start_pass(&mut self) -> Result<Mode, String> {
        let table = |d: &Self, class: usize, slot: usize| {
            Derived::new(d.dht[class].get(slot).and_then(Option::as_ref), class == 0)
        };
        if !self.progressive {
            let t = self
                .scan
                .iter()
                .map(|&ci| {
                    let c = &self.comps[ci];
                    Ok((table(self, 0, c.dc)?, table(self, 1, c.ac)?))
                })
                .collect::<Result<Vec<_>, String>>()?;
            return Ok(Mode::Sequential(t));
        }
        let (ss, se, ah, al) = (self.ss, self.se, self.ah, self.al);
        let dc_band = ss == 0;
        let band_bad = if dc_band {
            se != 0
        } else {
            ss > se || se >= 64 || self.scan.len() != 1
        };
        let bad = band_bad || (ah != 0 && al != ah.wrapping_sub(1)) || al > 13;
        if bad {
            return Err(format!(
                "JPEG: invalid progressive parameters Ss={ss} Se={se} Ah={ah} Al={al}"
            ));
        }
        for &ci in &self.scan {
            self.comps[ci].known[ss..=se].fill(i32::from(al));
        }
        Ok(match (dc_band, ah == 0) {
            (true, true) => Mode::DcFirst(
                self.scan
                    .iter()
                    .map(|&ci| table(self, 0, self.comps[ci].dc))
                    .collect::<Result<_, _>>()?,
            ),
            (true, false) => Mode::DcRefine,
            (false, first) => {
                let t = table(self, 1, self.comps[self.scan[0]].ac)?;
                if first {
                    Mode::AcFirst(t)
                } else {
                    Mode::AcRefine(t)
                }
            }
        })
    }

    /// jdcoefct.c `smoothing_ok`: would libjpeg smooth this progressive image?
    fn smoothing_ok(&self) -> bool {
        let mut useful = false;
        for c in &self.comps {
            let Some(q) = c.quant else { return false };
            if [0, 1, 8, 16, 9, 2, 3, 10, 17, 24]
                .iter()
                .any(|&i| q[i] == 0)
                || c.known[0] < 0
            {
                return false;
            }
            useful |= c.known[1..10].iter().any(|&b| b != 0);
        }
        useful
    }

    /// IDCT, upsampling and colour conversion of the whole image.
    fn output(&self, space: Space) -> Bgr {
        let (w, h) = (self.w, self.h);
        let full: Vec<Vec<u8>> = self
            .comps
            .iter()
            .map(|c| {
                let pw = c.wib * 8;
                let mut plane = vec![0u8; pw * c.hib * 8];
                let q = c.quant.map_or([0; 64], |q| q.map(|v| v as i16));
                for by in 0..c.hib {
                    for bx in 0..c.wib {
                        let at = by * 8 * pw + bx * 8;
                        idct_islow(&c.coef[by * c.stride + bx], &q, &mut plane[at..], pw);
                    }
                }
                let up = upsampler(c, self.max_h, self.max_v).unwrap_or(Up::Full);
                upsample(&plane, pw, c.dw, c.dh, up, w, h)
            })
            .collect();
        let mut data = vec![0u8; 3 * w * h];
        let pixels = data.as_chunks_mut::<3>().0.iter_mut().enumerate();
        match space {
            Space::Gray => pixels.for_each(|(i, px)| *px = [full[0][i]; 3]),
            Space::Rgb => pixels.for_each(|(i, px)| *px = [full[2][i], full[1][i], full[0][i]]),
            Space::Ycc => {
                // jdcolor.c build_ycc_rgb_table: FIX(x) = round(x·2¹⁶), x = Cb or Cr − 128
                let fix = |x: i64, k: i64| (k * x + (1 << 15)) >> 16;
                let clamp = |v: i64| v.clamp(0, 255) as u8;
                for (i, px) in pixels {
                    let y = i64::from(full[0][i]);
                    let (cb, cr) = (i64::from(full[1][i]) - 128, i64::from(full[2][i]) - 128);
                    let r = y + fix(cr, 91_881);
                    let g = y + ((-22_554 * cb + (1 << 15) - 46_802 * cr) >> 16);
                    let b = y + fix(cb, 116_130);
                    *px = [clamp(b), clamp(g), clamp(r)];
                }
            }
        }
        Bgr { w, h, data }
    }
}

/// jdphuff.c `decode_mcu_AC_refine` for one block.
#[allow(clippy::too_many_arguments)]
fn ac_refine(
    r: &mut Reader<'_>,
    t: &Derived,
    block: &mut [i16; 64],
    ss: usize,
    se: usize,
    p1: i32,
    m1: i32,
    eobrun: &mut u32,
) -> Result<(), String> {
    let correct = |r: &mut Reader<'_>, coef: &mut i16| -> Result<(), String> {
        if r.bits(1)? != 0 && i32::from(*coef) & p1 == 0 {
            *coef = coef.wrapping_add(if *coef >= 0 { p1 } else { m1 } as i16);
        }
        Ok(())
    };
    let mut k = ss;
    if *eobrun == 0 {
        while k <= se {
            let rs = r.huff(t)?;
            let (mut run, s) = (rs >> 4, rs & 15);
            let mut new = 0;
            if s != 0 {
                new = if r.bits(1)? != 0 { p1 } else { m1 };
            } else if run != 15 {
                *eobrun = 1 << run;
                if run != 0 {
                    *eobrun += r.bits(run)? as u32;
                }
                break;
            }
            loop {
                let coef = &mut block[NATURAL[k]];
                if *coef != 0 {
                    correct(r, coef)?;
                } else {
                    run -= 1;
                    if run < 0 {
                        break;
                    }
                }
                k += 1;
                if k > se {
                    break;
                }
            }
            if new != 0 {
                block[NATURAL[k]] = new as i16;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= se {
            let coef = &mut block[NATURAL[k]];
            if *coef != 0 {
                correct(r, coef)?;
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}

/// jdsample.c `jinit_upsampler`'s choice for a component (fancy upsampling on, full-size DCT).
fn upsampler(c: &Comp, mh: usize, mv: usize) -> Result<Up, String> {
    Ok(if c.h == mh && c.v == mv {
        Up::Full
    } else if 2 * c.h == mh && c.v == mv {
        if c.dw > 2 { Up::H2v1Fancy } else { Up::H2v1 }
    } else if c.h == mh && 2 * c.v == mv {
        Up::H1v2Fancy
    } else if 2 * c.h == mh && 2 * c.v == mv {
        if c.dw > 2 { Up::H2v2Fancy } else { Up::H2v2 }
    } else if mh.is_multiple_of(c.h) && mv.is_multiple_of(c.v) {
        Up::Int(mh / c.h, mv / c.v)
    } else {
        return Err("JPEG: fractional sampling factors are not implemented".into());
    })
}

/// A component plane (`pw` samples per row, `dw`×`dh` of them real) brought to `w`×`h`. Context rows
/// beyond the top and bottom repeat the first and last real rows (jdmainct.c's wraparound pointers).
fn upsample(plane: &[u8], pw: usize, dw: usize, dh: usize, up: Up, w: usize, h: usize) -> Vec<u8> {
    let row = |y: usize| &plane[y * pw..y * pw + dw];
    let mut out = vec![0u8; w * h];
    let mut wide = vec![0u8; 2 * dw];
    for (y, o) in out.chunks_exact_mut(w).enumerate() {
        // the nearer and the further source row of a vertically fancy upsampler
        let near = y / 2;
        let far = if y % 2 == 0 {
            near.saturating_sub(1)
        } else {
            (near + 1).min(dh - 1)
        };
        match up {
            Up::Full => o.copy_from_slice(&row(y)[..w]),
            Up::H2v1 => o
                .iter_mut()
                .enumerate()
                .for_each(|(x, v)| *v = row(y)[x / 2]),
            Up::H2v2 => o
                .iter_mut()
                .enumerate()
                .for_each(|(x, v)| *v = row(near)[x / 2]),
            Up::Int(he, ve) => o
                .iter_mut()
                .enumerate()
                .for_each(|(x, v)| *v = row(y / ve)[x / he]),
            Up::H1v2Fancy => {
                let bias = if y % 2 == 0 { 1 } else { 2 };
                for ((v, &a), &b) in o.iter_mut().zip(row(near)).zip(row(far)) {
                    *v = ((3 * u32::from(a) + u32::from(b) + bias) >> 2) as u8;
                }
            }
            Up::H2v1Fancy => {
                let a = row(y);
                let n = dw;
                wide[0] = a[0];
                wide[1] = ((u32::from(a[0]) * 3 + u32::from(a[1]) + 2) >> 2) as u8;
                for c in 1..n - 1 {
                    let v = u32::from(a[c]) * 3;
                    wide[2 * c] = ((v + u32::from(a[c - 1]) + 1) >> 2) as u8;
                    wide[2 * c + 1] = ((v + u32::from(a[c + 1]) + 2) >> 2) as u8;
                }
                wide[2 * n - 2] = ((u32::from(a[n - 1]) * 3 + u32::from(a[n - 2]) + 1) >> 2) as u8;
                wide[2 * n - 1] = a[n - 1];
                o.copy_from_slice(&wide[..w]);
            }
            Up::H2v2Fancy => {
                let (a, b) = (row(near), row(far));
                let sum = |i: usize| 3 * u32::from(a[i]) + u32::from(b[i]);
                let n = dw;
                let mut this = sum(0);
                let next = sum(1);
                wide[0] = ((this * 4 + 8) >> 4) as u8;
                wide[1] = ((this * 3 + next + 7) >> 4) as u8;
                let mut last = this;
                this = next;
                for c in 1..n - 1 {
                    let next = sum(c + 1);
                    wide[2 * c] = ((this * 3 + last + 8) >> 4) as u8;
                    wide[2 * c + 1] = ((this * 3 + next + 7) >> 4) as u8;
                    (last, this) = (this, next);
                }
                wide[2 * n - 2] = ((this * 3 + last + 8) >> 4) as u8;
                wide[2 * n - 1] = ((this * 4 + 7) >> 4) as u8;
                o.copy_from_slice(&wide[..w]);
            }
        }
    }
    out
}

/// jdmaster.c `prepare_range_limit_table` as the IDCT indexes it (`IDCT_range_limit`, `RANGE_MASK`): the
/// sample for a masked 10-bit result, wrapping out-of-range values the way libjpeg does.
const fn range_limit() -> [u8; 1024] {
    let mut t = [0u8; 1024];
    let mut m = 0;
    while m < 1024 {
        t[m] = if m < 128 {
            (m + 128) as u8
        } else if m < 512 {
            255
        } else if m < 896 {
            0
        } else {
            (m - 896) as u8
        };
        m += 1;
    }
    t
}

const LIMIT: [u8; 1024] = range_limit();

/// jidctint.c `jpeg_idct_islow`: one block's samples into `out` (rows `stride` apart).
fn idct_islow(coef: &[i16; 64], q: &[i16; 64], out: &mut [u8], stride: usize) {
    const CONST_BITS: u32 = 13;
    const PASS1_BITS: u32 = 2;
    const F0_298: i64 = 2446;
    const F0_390: i64 = 3196;
    const F0_541: i64 = 4433;
    const F0_765: i64 = 6270;
    const F0_899: i64 = 7373;
    const F1_175: i64 = 9633;
    const F1_501: i64 = 12299;
    const F1_847: i64 = 15137;
    const F1_961: i64 = 16069;
    const F2_053: i64 = 16819;
    const F2_562: i64 = 20995;
    const F3_072: i64 = 25172;
    let descale = |x: i64, n: u32| (x + (1 << (n - 1))) >> n;
    // the even and odd halves of one 1-D pass over inputs i0..i7
    let butterfly = |i: [i64; 8]| -> [i64; 8] {
        let z1 = (i[2] + i[6]) * F0_541;
        let tmp2 = z1 + i[6] * -F1_847;
        let tmp3 = z1 + i[2] * F0_765;
        let tmp0 = (i[0] + i[4]) << CONST_BITS;
        let tmp1 = (i[0] - i[4]) << CONST_BITS;
        let (tmp10, tmp13, tmp11, tmp12) = (tmp0 + tmp3, tmp0 - tmp3, tmp1 + tmp2, tmp1 - tmp2);
        let (t0, t1, t2, t3) = (i[7], i[5], i[3], i[1]);
        let (z1, z2, z3, z4) = (t0 + t3, t1 + t2, t0 + t2, t1 + t3);
        let z5 = (z3 + z4) * F1_175;
        let (t0, t1, t2, t3) = (t0 * F0_298, t1 * F2_053, t2 * F3_072, t3 * F1_501);
        let (z1, z2) = (z1 * -F0_899, z2 * -F2_562);
        let (z3, z4) = (z3 * -F1_961 + z5, z4 * -F0_390 + z5);
        let (t0, t1, t2, t3) = (t0 + z1 + z3, t1 + z2 + z4, t2 + z2 + z3, t3 + z1 + z4);
        [
            tmp10 + t3,
            tmp11 + t2,
            tmp12 + t1,
            tmp13 + t0,
            tmp13 - t0,
            tmp12 - t1,
            tmp11 - t2,
            tmp10 - t3,
        ]
    };
    let mut ws = [0i32; 64];
    for col in 0..8 {
        let deq =
            |row: usize| i64::from(i32::from(coef[row * 8 + col]) * i32::from(q[row * 8 + col]));
        if (1..8).all(|row| coef[row * 8 + col] == 0) {
            let dc = (deq(0) << PASS1_BITS) as i32;
            for row in 0..8 {
                ws[row * 8 + col] = dc;
            }
            continue;
        }
        let o = butterfly(std::array::from_fn(deq));
        for (row, v) in o.iter().enumerate() {
            ws[row * 8 + col] = descale(*v, CONST_BITS - PASS1_BITS) as i32;
        }
    }
    for (row, w) in ws.as_chunks::<8>().0.iter().enumerate() {
        let o = &mut out[row * stride..row * stride + 8];
        if w[1..].iter().all(|&v| v == 0) {
            let dc = LIMIT[(descale(i64::from(w[0]), PASS1_BITS + 3) as i32 & 1023) as usize];
            o.fill(dc);
            continue;
        }
        let r = butterfly(std::array::from_fn(|k| i64::from(w[k])));
        for (v, x) in o.iter_mut().zip(r) {
            *v = LIMIT[(descale(x, CONST_BITS + PASS1_BITS + 3) as i32 & 1023) as usize];
        }
    }
}

/// jstdhuff.c `val_ac_luminance`.
const AC_LUM_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

/// jstdhuff.c `val_ac_chrominance`.
const AC_CHR_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];
