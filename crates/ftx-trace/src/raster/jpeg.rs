//! JPEG decoding as the reference's `load_image` sees it. Pillow 12.3 opens a JPEG with its bundled
//! libjpeg-turbo 3.1.4.1 at the library's defaults (accurate integer IDCT, fancy upsampling, block smoothing
//! allowed) and asks for `L`, `RGB` or (four components) `CMYK`, which it unpacks inverted ("CMYK;I", Adobe's
//! convention) and converts with its own `cmyk2rgb`. This is a port of the path that takes:
//! - the marker reader, `jdmarker.c`;
//! - sequential and progressive Huffman decoding into a full coefficient buffer (`jdhuff.c`, `jdphuff.c`,
//!   `jdcoefct.c`);
//! - the islow IDCT and its wrapping range limit (`jidctint.c`, `jdmaster.c`);
//! - the upsamplers, with edge-replicated context rows (`jdsample.c`, `jdmainct.c`);
//! - the YCbCr→RGB tables (`jdcolor.c`). They give the same values as the Arm Neon code Pillow runs on
//!   arm64 for every Cb and Cr. YCCK goes to CMYK through them (`ycck_cmyk_convert`).
//!
//! Damaged data is handled the way libjpeg handles it:
//! - zero bits are supplied after a premature marker, and the rest of the segment is left zero;
//! - restart markers are resynchronised;
//! - garbage before a marker is skipped;
//! - a file that ends inside the data it needs is refused, as Pillow refuses it ("image file is truncated").
//!
//! Refused here, because they are not ported:
//! - arithmetic, lossless and hierarchical files.
//!
//! Pillow feeds libjpeg 64 KiB blocks. After the last row of a single-scan file, markers past the block
//! that held the scan's last byte are never read, so they cannot fail the file. A tables-only datastream
//! before the image is read for its tables, as Pillow re-reads the header.
//!
//! Pillow's own header walk refuses some things libjpeg would take, and they are refused here too:
//! - precision other than 8;
//! - component counts other than 1, 3 and 4;
//! - a TEM marker before the first scan;
//! - a short JFIF or Adobe marker, an ICC_PROFILE chunk without its count byte, a Photoshop resource cut
//!   off after its code, an EXIF `XResolution` of length 1 with `ResolutionUnit`, and an MPF entry under 16 bytes.
//!
//! Dequantized coefficients beyond 16 bits follow the C code; Pillow's Neon IDCT on arm64 wraps them at 16
//! bits. Valid files never have them, but damaged files often do (one in ten randomly damaged files in
//! testing), and then the pixels differ from Pillow's there (they match with `JSIMD_FORCENONE=1`).
//!
//! Provenance: libjpeg-turbo 3.1.4.1 `src/` (IJG License and Modified BSD; `LICENSE-libjpeg-turbo.md` in this
//! crate). This software is based in part on the work of the Independent JPEG Group.

use super::image::Bgr;

mod photo;
pub use photo::{OrientedJpeg, decode_photo};

/// The refusal for a file that ends inside data the decoder needs (Pillow: "image file is truncated").
const TRUNCATED: &str = "JPEG: the file is truncated";
/// jdhuff.h `MIN_GET_BITS` for a 64-bit bit buffer.
const MIN_GET_BITS: i32 = 57;
/// Pillow's `ImageFile.MAXBLOCK`, the size of the reads that feed the decoder.
const MAXBLOCK: usize = 65536;

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
    // Persistent across restarts/scans. Photo evidence must not use libjpeg's repair path.
    repaired: bool,
    strict: bool,
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
                self.repaired = true;
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
            self.repaired = true;
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
            self.repaired = true;
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
            self.repaired = true;
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
            self.repaired = true;
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
    /// `coef_bits` before the component's latest scan (jdphuff.c's `prev_coef_bit_ptr`), which block
    /// smoothing uses for the rows that scan never reached.
    prev_known: [i32; 64],
}

/// The colour space libjpeg guesses (jdapimin.c `default_decompress_parms`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Space {
    Gray,
    Ycc,
    Rgb,
    Cmyk,
    Ycck,
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
    /// `input_scan_number`: the SOS markers read so far.
    scans: usize,
    /// jdcoefct.c's `last_good_iMCU_row`: the iMCU row of the last MCU a scan began with data left.
    last_good: usize,
    /// Pillow's `icclist`: the ICC_PROFILE APP2 payloads since the last SOF.
    icc: Vec<&'a [u8]>,
    /// Whether JFIF APP0 already set `info["dpi"]` (`jfif_unit` 1 or 2).
    jfif_dpi: bool,
    /// Pillow's `info["exif"]`: concatenated `Exif\0\0` APP1 payloads before the first SOS.
    exif: Vec<u8>,
    /// Pillow's `info["mp"]`: the last `MPF\0` APP2 payload before the first SOS.
    mpf: Option<&'a [u8]>,
}

/// Decodes a JPEG as Pillow 12.3 does for `Image.open(f).convert("RGB")`, returned as OpenCV's BGR.
///
/// # Errors
/// A damaged or truncated file, an image over `max_pixels`, or a variant not ported (see the module).
pub fn decode(bytes: &[u8], max_pixels: usize) -> Result<Bgr, String> {
    decode_impl(bytes, max_pixels, false)
}

fn decode_impl(bytes: &[u8], max_pixels: usize, strict: bool) -> Result<Bgr, String> {
    let mut d = Decoder {
        r: Reader {
            data: bytes,
            pos: 0,
            marker: 0,
            buf: 0,
            left: 0,
            insufficient: false,
            repaired: false,
            strict,
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
        scans: 0,
        last_good: 0,
        icc: Vec::new(),
        jfif_dpi: false,
        exif: Vec::new(),
        mpf: None,
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
        // Every row is out by now, and jpeg_finish_decompress only looks for EOI in what Pillow has fed
        // it: ImageFile.load reads MAXBLOCK blocks from the start, and the last row came out once the block
        // holding the scan's last byte arrived. Pillow accepts a suspension past that block.
        let fed = d.r.pos.div_ceil(MAXBLOCK) * MAXBLOCK;
        if !strict {
            d.r.data = &d.r.data[..fed.min(d.r.data.len())];
        }
        match d.read_markers() {
            Ok(true) => return Err("JPEG: a second scan in a single-scan file".into()),
            Ok(false) => {}
            Err(e) if e == TRUNCATED && !strict => {}
            Err(e) => return Err(e),
        }
    }
    // Photo evidence takes no smoothed (estimated) coefficients either: a progressive file libjpeg would
    // block-smooth has incomplete scans.
    if strict
        && (d.r.repaired
            || d.comps.iter().any(|c| c.quant.is_none())
            || (d.progressive
                && (d.comps.iter().any(|c| c.known.iter().any(|&b| b < 0)) || d.smoothing_ok())))
    {
        return Err("JPEG photo requires complete scans without damaged-data recovery".into());
    }
    Ok(d.output(space))
}

impl<'a> Decoder<'a> {
    /// jdmarker.c `read_markers`: tables up to the next SOS (true) or EOI (false).
    fn read_markers(&mut self) -> Result<bool, String> {
        loop {
            if self.r.marker == 0 {
                if self.saw_soi {
                    self.r.next_marker()?;
                } else {
                    // first_marker: the datastream starts with SOI, right here
                    if (self.r.byte()?, self.r.byte()?) != (0xFF, 0xD8) {
                        return Err("not a JPEG file".into());
                    }
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
                0xD9 if self.in_headers => {
                    if !self.comps.is_empty() {
                        return Err("JPEG: an SOF marker without SOS".into());
                    }
                    // A tables-only datastream (jdinput.c consume_markers): jpeg_read_header returns
                    // JPEG_HEADER_TABLES_ONLY, and Pillow calls it again. The tables stay, and the next
                    // datastream must start with its SOI.
                    self.saw_soi = false;
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
                m @ 0xE0..=0xEF => self.appn(m)?,
                0xFE | 0xDC => {
                    let len = self.r.word()? - 2;
                    self.r.skip(len);
                }
                0xD0..=0xD7 => self.r.repaired = true,
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
        if !matches!(n, 1 | 3 | 4) {
            return Err(format!("JPEG: cannot handle {n}-layer images"));
        }
        // JpegImagePlugin.SOF sorts the ICC_PROFILE chunks and reads the first one's chunk count
        self.icc.sort_unstable();
        if self.icc.first().is_some_and(|c| c.len() <= 13) {
            return Err("JPEG: Pillow cannot read the ICC_PROFILE marker".into());
        }
        self.icc.clear();
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
                prev_known: [0; 64],
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
        if self.in_headers {
            if !self.jfif_dpi && !self.exif.is_empty() && exif_dpi_raises(&self.exif) {
                return Err("JPEG: Pillow cannot read the EXIF marker".into());
            }
            if self.mpf.is_some_and(mpf_raises) {
                return Err("JPEG: Pillow cannot read the MPF marker".into());
            }
        }
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

    /// jdmarker.c `get_interesting_appn` for JFIF (APP0) and Adobe (APP14), `skip_variable` for the
    /// others. Before the first scan, Pillow's header walk reads them too ([`Self::pillow_app`]).
    fn appn(&mut self, m: u8) -> Result<(), String> {
        let mut len = self.r.word()? - 2;
        if self.in_headers {
            self.pillow_app(m, len)?;
        }
        if m != 0xE0 && m != 0xEE {
            self.r.skip(len);
            return Ok(());
        }
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

    /// Pillow's `JpegImagePlugin.APP` on the `len` bytes at the reader: what it raises makes `Image.open`
    /// fail, though libjpeg would decode the file.
    fn pillow_app(&mut self, m: u8, len: i64) -> Result<(), String> {
        let data: &'a [u8] = self.r.data;
        let at = self.r.pos;
        // ImageFile._safe_read
        let s = data
            .get(at..at + usize::try_from(len).unwrap_or(0))
            .ok_or(TRUNCATED)?;
        let unreadable = |what: &str| Err(format!("JPEG: Pillow cannot read the {what} marker"));
        match m {
            0xE0 if s.starts_with(b"JFIF") => {
                if s.len() < 7 {
                    return unreadable("JFIF");
                }
                if s.len() >= 12 && matches!(s[7], 1 | 2) {
                    self.jfif_dpi = true;
                }
                Ok(())
            }
            0xE1 if s.starts_with(b"Exif\0\0") => {
                if self.exif.is_empty() {
                    self.exif.extend_from_slice(s);
                } else {
                    self.exif.extend_from_slice(&s[6..]);
                }
                Ok(())
            }
            0xEE if s.starts_with(b"Adobe") && s.len() < 7 => unreadable("Adobe"),
            0xED if s.starts_with(b"Photoshop 3.0\0") && photoshop_raises(s) => {
                unreadable("Photoshop")
            }
            0xE2 if s.starts_with(b"ICC_PROFILE\0") => {
                self.icc.push(s);
                Ok(())
            }
            0xE2 if s.starts_with(b"MPF\0") => {
                self.mpf = Some(&s[4..]);
                Ok(())
            }
            _ => Ok(()),
        }
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
        let imcu_rows = h.div_ceil(mv * 8);
        for c in &mut self.comps {
            c.wib = (w * c.h).div_ceil(mh * 8);
            c.hib = (h * c.v).div_ceil(mv * 8);
            c.dw = (w * c.h).div_ceil(mh);
            c.dh = (h * c.v).div_ceil(mv);
            c.stride = c.wib.next_multiple_of(c.h);
            c.coef = vec![[0; 64]; c.stride * c.hib.next_multiple_of(c.v).max(imcu_rows * c.v)];
        }
        self.multi_scan = self.scan.len() < self.comps.len() || self.progressive;
        for c in &self.comps {
            upsampler(c, mh, mv)?;
        }
        let space = if self.comps.len() == 1 {
            Space::Gray
        } else if self.comps.len() == 4 {
            // Adobe transform 0 is CMYK, anything else is taken for YCCK; no Adobe marker, plain CMYK
            match self.adobe {
                Some(0) | None => Space::Cmyk,
                Some(_) => Space::Ycck,
            }
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
        self.scans += 1;
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
            last_good,
            ..
        } = self;
        (r.left, r.buf, r.insufficient) = (0, 0, false);
        let (ss, se, al, interval) = (*ss, *se, i32::from(*al), *restart_interval);
        let (p1, m1) = (1i32 << al, -1i32 << al);
        // MCU rows per iMCU row: a component's v in a non-interleaved scan, else one
        let mcu_rows = if n == 1 { comps[scan[0]].v } else { 1 };
        let mut togo = interval;
        let mut last_dc = [0i32; 4];
        let mut eobrun = 0u32;
        for my in 0..mcus_y {
            for mx in 0..mcus_x {
                // consume_data, before decode_mcu and its restart
                if !r.insufficient {
                    *last_good = my / mcu_rows;
                }
                if interval != 0 {
                    if togo == 0 {
                        if eobrun != 0 {
                            r.repaired = true;
                        }
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
                                    if k >= 64 || s > 10 {
                                        r.repaired = true;
                                    }
                                    block[NATURAL[k]] = extend(r.bits(s)?, s) as i16;
                                } else if run == 15 {
                                    k += 15;
                                    r.repaired |= k >= 64;
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
                                    r.repaired |= k > se;
                                    let v = extend(r.bits(s)?, s);
                                    block[NATURAL[k]] = v.wrapping_shl(al as u32) as i16;
                                } else if run == 15 {
                                    k += 15;
                                    r.repaired |= k > se;
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
                if r.strict && r.repaired {
                    return Err(
                        "JPEG photo requires complete scans without damaged-data recovery".into(),
                    );
                }
            }
        }
        r.repaired |= eobrun != 0;
        Ok(())
    }

    /// The entropy decoder's `start_pass`: jdhuff.c `start_pass_huff_decoder` or jdphuff.c
    /// `start_pass_phuff_decoder` (its scan checks, and `coef_bits`; its warnings are ignored as Pillow does).
    fn start_pass(&mut self) -> Result<Mode, String> {
        let table = |d: &Self, class: usize, slot: usize| {
            Derived::new(d.dht[class].get(slot).and_then(Option::as_ref), class == 0)
        };
        if !self.progressive {
            self.r.repaired |= self.ss != 0 || self.se != 63 || self.ah != 0 || self.al != 0;
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
            let known = &self.comps[ci].known;
            self.r.repaired |= (!dc_band && known[0] < 0)
                || known[ss..=se]
                    .iter()
                    .any(|&v| v != if ah == 0 { -1 } else { i32::from(ah) });
            let c = &mut self.comps[ci];
            for k in ss.min(1)..=se.max(9) {
                c.prev_known[k] = if self.scans > 1 { c.known[k] } else { 0 };
            }
            c.known[ss..=se].fill(i32::from(al));
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

    /// IDCT (with jdcoefct.c `decompress_smooth_data` when `smoothing_ok`), upsampling and colour conversion
    /// of the whole image.
    fn output(&self, space: Space) -> Bgr {
        let (w, h) = (self.w, self.h);
        let smooth = self.progressive && self.smoothing_ok();
        let total_imcu_rows = h.div_ceil(self.max_v * 8);
        let full: Vec<Vec<u8>> = self
            .comps
            .iter()
            .map(|c| {
                let pw = c.wib * 8;
                let mut plane = vec![0u8; pw * c.hib * 8];
                let q = c.quant.map_or([0; 64], |q| q.map(|v| v as i16));
                if smooth && c.wib > 0 {
                    // smoothing_ok's latches: rows past the last good one take the previous scan's bits
                    let mut latch = [0; 10];
                    latch.copy_from_slice(&c.known[..10]);
                    let mut prev = [-1; 10];
                    if self.scans > 1 {
                        prev[1..].copy_from_slice(&c.prev_known[1..10]);
                    }
                    for imcu_row in 0..total_imcu_rows {
                        let bits = if imcu_row > self.last_good {
                            &prev
                        } else {
                            &latch
                        };
                        let block_rows = if imcu_row + 1 < total_imcu_rows {
                            c.v
                        } else {
                            match c.hib % c.v {
                                0 => c.v,
                                r => r,
                            }
                        };
                        let last_row = total_imcu_rows * block_rows - 1;
                        for br in 0..block_rows {
                            let by = imcu_row * c.v + br;
                            if by >= c.hib {
                                continue;
                            }
                            let r_idx = imcu_row * block_rows + br;
                            let r_m1 = if r_idx > 0 { by - 1 } else { by };
                            let r_m2 = if r_idx > 1 { by - 2 } else { r_m1 };
                            let r_p1 = if r_idx < last_row { by + 1 } else { by };
                            let r_p2 = if r_idx + 1 < last_row { by + 2 } else { r_p1 };
                            let rows = [r_m2, r_m1, by, r_p1, r_p2];
                            for bx in 0..c.wib {
                                let ws = smooth_block(c, rows, bx, bits);
                                let at = by * 8 * pw + bx * 8;
                                idct_islow(&ws, &q, &mut plane[at..], pw);
                            }
                        }
                    }
                } else {
                    for by in 0..c.hib {
                        for bx in 0..c.wib {
                            let at = by * 8 * pw + bx * 8;
                            idct_islow(&c.coef[by * c.stride + bx], &q, &mut plane[at..], pw);
                        }
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
                for (i, px) in pixels {
                    let [r, g, b] = ycc_rgb(full[0][i], full[1][i], full[2][i]);
                    *px = [b, g, r];
                }
            }
            Space::Cmyk | Space::Ycck => {
                // libjpeg's CMYK output (jdcolor.c: copied, or ycck_cmyk_convert's inverted RGB with K as it
                // is), Pillow's "CMYK;I" unpacker (every byte inverted) and Convert.c cmyk2rgb
                let muldiv255 = |a: i64, b: i64| {
                    let t = a * b + 128;
                    ((t >> 8) + t) >> 8
                };
                for (i, px) in pixels {
                    let cmy = if space == Space::Ycck {
                        ycc_rgb(full[0][i], full[1][i], full[2][i]).map(|v| 255 - v)
                    } else {
                        [full[0][i], full[1][i], full[2][i]]
                    };
                    // inverted: c' = 255 − c and nk = 255 − k' = k
                    let nk = i64::from(full[3][i]);
                    let rgb =
                        cmy.map(|v| (nk - muldiv255(255 - i64::from(v), nk)).clamp(0, 255) as u8);
                    *px = [rgb[2], rgb[1], rgb[0]];
                }
            }
        }
        Bgr { w, h, data }
    }
}

/// jdcoefct.c `decompress_smooth_data` on one block at `(rows[2], bx)` using its 5×5 DC neighbourhood and
/// the latched `coef_bits` of its row.
fn smooth_block(c: &Comp, rows: [usize; 5], bx: usize, bits: &[i32; 10]) -> [i16; 64] {
    let mut ws = c.coef[rows[2] * c.stride + bx];
    let Some(q) = c.quant else { return ws };
    let last_col = c.wib - 1;
    let cols = [
        bx.saturating_sub(2),
        bx.saturating_sub(1),
        bx,
        (bx + 1).min(last_col),
        (bx + 2).min(last_col),
    ];
    let dc = |ry: usize, cx: usize| i32::from(c.coef[rows[ry] * c.stride + cols[cx]][0]);
    let (dc01, dc02, dc03, dc04, dc05) = (dc(0, 0), dc(0, 1), dc(0, 2), dc(0, 3), dc(0, 4));
    let (dc06, dc07, dc08, dc09, dc10) = (dc(1, 0), dc(1, 1), dc(1, 2), dc(1, 3), dc(1, 4));
    let (dc11, dc12, dc13, dc14, dc15) = (dc(2, 0), dc(2, 1), dc(2, 2), dc(2, 3), dc(2, 4));
    let (dc16, dc17, dc18, dc19, dc20) = (dc(3, 0), dc(3, 1), dc(3, 2), dc(3, 3), dc(3, 4));
    let (dc21, dc22, dc23, dc24, dc25) = (dc(4, 0), dc(4, 1), dc(4, 2), dc(4, 3), dc(4, 4));

    let more = bits[1..].iter().all(|&b| b == -1);
    let q00 = i64::from(q[0]);
    let apply = |ws: &mut [i16; 64], pos: usize, al: i32, num: i32| {
        if al != 0 && ws[pos] == 0 {
            let prod = i64::from(num) * q00;
            let div = i64::from(q[pos]) << 8;
            let half = i64::from(q[pos]) << 7;
            let clamp = |mut p: i32| {
                if al > 0 && p >= (1 << al) {
                    p = (1 << al) - 1;
                }
                p
            };
            ws[pos] = if prod >= 0 {
                clamp(((prod + half) / div) as i32) as i16
            } else {
                (-clamp(((half - prod) / div) as i32)) as i16
            };
        }
    };

    // Q01 (pos 1, zig 1)
    let num01 = if more {
        3 * (dc10 + dc15 + dc20 - dc06 - dc11 - dc16)
            + (dc04 + dc05 + dc24 + dc25 - dc01 - dc02 - dc21 - dc22)
            + 13 * (dc07 + dc17 - dc09 - dc19)
            + 38 * (dc12 - dc14)
    } else {
        50 * (dc12 - dc14) - 7 * (dc11 - dc15)
    };
    apply(&mut ws, 1, bits[1], num01);

    // Q10 (pos 8, zig 2)
    let num10 = if more {
        3 * (dc22 + dc23 + dc24 - dc02 - dc03 - dc04)
            + (dc16 + dc21 + dc20 + dc25 - dc01 - dc06 - dc05 - dc10)
            + 13 * (dc07 + dc09 - dc17 - dc19)
            + 38 * (dc08 - dc18)
    } else {
        50 * (dc08 - dc18) - 7 * (dc03 - dc23)
    };
    apply(&mut ws, 8, bits[2], num10);

    // Q20 (pos 16, zig 3)
    let num20 = if more {
        (dc03 + dc23) + 7 * (dc08 + dc18) + 2 * (dc07 + dc09 + dc17 + dc19)
            - 14 * dc13
            - 5 * (dc12 + dc14)
    } else {
        13 * (dc08 + dc18) - 24 * dc13 - (dc03 + dc23)
    };
    apply(&mut ws, 16, bits[3], num20);

    // Q11 (pos 9, zig 4)
    let diag = dc07 + dc19 - dc09 - dc17;
    let num11 = if more {
        9 * diag + (dc05 + dc21 - dc01 - dc25)
    } else {
        10 * diag + (dc04 + dc10 + dc16 + dc22) - (dc02 + dc06 + dc20 + dc24)
    };
    apply(&mut ws, 9, bits[4], num11);

    // Q02 (pos 2, zig 5)
    let num02 = if more {
        (dc11 + dc15) + 7 * (dc12 + dc14) + 2 * (dc07 + dc09 + dc17 + dc19)
            - 14 * dc13
            - 5 * (dc08 + dc18)
    } else {
        13 * (dc12 + dc14) - 24 * dc13 - (dc11 + dc15)
    };
    apply(&mut ws, 2, bits[5], num02);

    if more {
        // Q03 (pos 3, zig 6)
        apply(
            &mut ws,
            3,
            bits[6],
            2 * (dc12 - dc14) + (dc07 + dc17 - dc09 - dc19),
        );
        // Q12 (pos 10, zig 7)
        apply(
            &mut ws,
            10,
            bits[7],
            3 * (dc18 - dc08) + (dc07 + dc09 - dc17 - dc19),
        );
        // Q21 (pos 17, zig 8)
        apply(
            &mut ws,
            17,
            bits[8],
            3 * (dc14 - dc12) + (dc07 + dc17 - dc09 - dc19),
        );
        // Q30 (pos 24, zig 9)
        apply(
            &mut ws,
            24,
            bits[9],
            2 * (dc08 - dc18) + (dc07 + dc09 - dc17 - dc19),
        );
        // DC (pos 0)
        let sum256 = 152 * dc13
            + 42 * (dc08 + dc12 + dc14 + dc18)
            + 6 * (dc07 + dc09 + dc17 + dc19
                - (dc02 + dc04 + dc06 + dc10 + dc16 + dc20 + dc22 + dc24))
            - 8 * (dc03 + dc11 + dc15 + dc23)
            - 2 * (dc01 + dc05 + dc21 + dc25);
        let n = i64::from(sum256);
        let div = q00 << 8;
        ws[0] = if n * q00 >= 0 {
            (((n + 128) * q00) / div) as i16
        } else {
            (-(((128 - n) * q00) / div)) as i16
        };
    }
    ws
}

type TiffEntry<'a> = (u16, u16, usize, &'a [u8]);

/// Parses a standard 32-bit TIFF IFD0 header and returns `(little_endian, entries)` where each entry is
/// `(tag, typ, count, value_bytes)`, stopping quietly on truncated entries as Pillow's `ImageFileDirectory_v2.load`
/// does.
fn parse_tiff_ifd(data: &[u8]) -> Option<(bool, Vec<TiffEntry<'_>>)> {
    let head = data.get(..8)?;
    if !matches!(
        &head[..4],
        b"MM\x00*" | b"II*\x00" | b"MM*\x00" | b"II\x00*"
    ) {
        return None;
    }
    let le = head.starts_with(b"II");
    let u16_at = |b: &[u8]| {
        if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        }
    };
    let u32_at = |b: &[u8]| {
        if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize
        }
    };
    let next = u32_at(&head[4..8]);
    let count_bytes = data.get(next..next.checked_add(2)?)?;
    let tag_count = usize::from(u16_at(count_bytes));
    let mut tags: Vec<(u16, u16, usize, &[u8])> = Vec::new();
    for i in 0..tag_count {
        let off = next.checked_add(2)?.checked_add(i.checked_mul(12)?)?;
        let Some(ent) = data.get(off..off + 12) else {
            break;
        };
        let tag = u16_at(&ent[0..2]);
        let typ = u16_at(&ent[2..4]);
        let count = u32_at(&ent[4..8]);
        let unit = match typ {
            1 | 2 | 6 | 7 => 1usize,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 | 16 => 8,
            _ => continue,
        };
        let Some(size) = count.checked_mul(unit) else {
            continue;
        };
        if size == 0 {
            continue;
        }
        let val = if size <= 4 {
            &ent[8..8 + size]
        } else {
            let voff = u32_at(&ent[8..12]);
            let Some(s) = voff.checked_add(size).and_then(|e| data.get(voff..e)) else {
                continue;
            };
            s
        };
        if let Some(slot) = tags.iter_mut().find(|t| t.0 == tag) {
            *slot = (tag, typ, count, val);
        } else {
            tags.push((tag, typ, count, val));
        }
    }
    Some((le, tags))
}

/// Whether Pillow's `JpegImageFile._read_dpi_from_exif` raises an uncaught `IndexError`: `ResolutionUnit`
/// (`0x0128`) is present and `XResolution` (`0x011A`) unpacks to a 1-byte `bytes` or a 0/1-char numeric `str`.
fn exif_dpi_raises(mut exif: &[u8]) -> bool {
    while let Some(rest) = exif.strip_prefix(b"Exif\0\0") {
        exif = rest;
    }
    let Some((_, tags)) = parse_tiff_ifd(exif) else {
        return false;
    };
    if !tags.iter().any(|t| t.0 == 0x0128) {
        return false;
    }
    let Some(&(_, typ, _, val)) = tags.iter().find(|t| t.0 == 0x011A) else {
        return false;
    };
    match typ {
        1 | 7 => val.len() == 1,
        2 => {
            let s = val.strip_suffix(b"\0").unwrap_or(val);
            s.is_empty() || (s.len() == 1 && s[0].is_ascii_digit())
        }
        _ => false,
    }
}

/// Whether Pillow's `_getmp` raises an uncaught `struct.error` when unpacking `0xB002` (`MPEntry`) for
/// `0xB001` (`NumberOfImages`) entries of 16 bytes each.
fn mpf_raises(mp: &[u8]) -> bool {
    let Some((le, tags)) = parse_tiff_ifd(mp) else {
        return false;
    };
    let Some(&(_, qtyp, _, qval)) = tags.iter().find(|t| t.0 == 0xB001) else {
        return false;
    };
    let Some(&(_, etyp, _, eval)) = tags.iter().find(|t| t.0 == 0xB002) else {
        return false;
    };
    if !matches!(etyp, 1 | 7) {
        return false;
    }
    let quant: i64 = match qtyp {
        6 => i64::from(qval[0] as i8),
        3 => i64::from(if le {
            u16::from_le_bytes([qval[0], qval[1]])
        } else {
            u16::from_be_bytes([qval[0], qval[1]])
        }),
        8 => i64::from(if le {
            i16::from_le_bytes([qval[0], qval[1]])
        } else {
            i16::from_be_bytes([qval[0], qval[1]])
        }),
        4 | 13 => i64::from(if le {
            u32::from_le_bytes([qval[0], qval[1], qval[2], qval[3]])
        } else {
            u32::from_be_bytes([qval[0], qval[1], qval[2], qval[3]])
        }),
        9 => i64::from(if le {
            i32::from_le_bytes([qval[0], qval[1], qval[2], qval[3]])
        } else {
            i32::from_be_bytes([qval[0], qval[1], qval[2], qval[3]])
        }),
        _ => return false,
    };
    quant > 0 && (eval.len() as i64) < quant.saturating_mul(16)
}

/// jdcolor.c's YCbCr → RGB (build_ycc_rgb_table: FIX(x) = round(x·2¹⁶), x = Cb or Cr − 128), range-limited.
fn ycc_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let fix = |x: i64, k: i64| (k * x + (1 << 15)) >> 16;
    let (y, cb, cr) = (i64::from(y), i64::from(cb) - 128, i64::from(cr) - 128);
    let r = y + fix(cr, 91_881);
    let g = y + ((-22_554 * cb + (1 << 15) - 46_802 * cr) >> 16);
    let b = y + fix(cb, 116_130);
    [r, g, b].map(|v| v.clamp(0, 255) as u8)
}

/// Whether Pillow's walk over a Photoshop APP13 payload `s` raises the IndexError it does not catch: a
/// resource that ends right after its 8BIM signature and code, with no name length. A `struct.error`
/// (any other short read) ends the walk quietly.
fn photoshop_raises(s: &[u8]) -> bool {
    let be = |at: usize, n: usize| {
        s.get(at..at + n)
            .map(|b| b.iter().fold(0usize, |v, &x| v << 8 | usize::from(x)))
    };
    let mut offset = 14;
    while s.get(offset..offset + 4) == Some(b"8BIM") {
        let Some(code) = be(offset + 4, 2) else {
            return false;
        };
        let Some(&name_len) = s.get(offset + 6) else {
            return true;
        };
        offset += 7 + usize::from(name_len);
        offset += offset & 1;
        let Some(size) = be(offset, 4) else {
            return false;
        };
        offset += 4;
        // ResolutionInfo is unpacked from its first 14 bytes
        if code == 0x03ED && s.len().saturating_sub(offset).min(size) < 14 {
            return false;
        }
        offset += size;
        offset += offset & 1;
    }
    false
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
                r.repaired |= s != 1;
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
                r.repaired |= k > se;
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

#[cfg(test)]
mod tests {
    use super::*;
    use ftx_core::receipt::sha256_hex;

    fn from_hex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                let d = |b: u8| match b {
                    b'0'..=b'9' => b - b'0',
                    b'a'..=b'f' => b - b'a' + 10,
                    b'A'..=b'F' => b - b'A' + 10,
                    _ => 0,
                };
                (d(c[0]) << 4) | d(c[1])
            })
            .collect()
    }

    #[test]
    fn progressive_block_smoothing_matches_pillow_across_partial_scans() {
        const SUB_0_HEX: &str = "ffd8ffe000104a46494600010100000100010000ffdb00430006040506050406060506070706080a100a0a09090a140e0f0c1017141818171416161a1d251f1a1b231c1616202c20232627292a29191f2d302d283025282928ffdb0043010707070a080a130a0a13281a161a2828282828282828282828282828282828282828282828282828282828282828282828282828282828282828282828282828ffc2001108001d002503011100021101031101ffc400190000020301000000000000000000000000050600030402ffc4001a010100030101010000000000000000000005020304060107ffda000c03010002100310000001718c64aa0faf9ba256dd4f592ddf8e61e4ce0180d1285b891f456c3d7ce723676a2d762b6285d8eb0078b6e0419b731bbc3fffc4001c1000020203010100000000000000000000000301210211130431ffda00080101000105025cd332193b217127083b6a19e821db9465b3431b4d75a997e4ccc73a68cfa98b451d723fffc4001c1100020301010101000000000000000000000102030411122132ffda0008010301013f01a6af28d32f28bec7d32f5b337e474f989b224aaf4ccb9f8417170bfe234af4caf3759565e16478cd5364df646589e1245f1fa7ffc4001f1100020202020301000000000000000000000201030411132105123114ffda0008010201013f0165d8a827435d107eb8388e3d1737a99393304def2221674644ec6c697917c791f0bdcd6e4aa95fa3c68fffc400161000030000000000000000000000000000203140ffda0008010100063f020507ffc4001b10010100020301000000000000000000000100113110214161ffda0008010100013f21020e22529c029f7bdc80c0c6a43a49ea577b4f71cd6ef73059208d17ffda000c03010002000300000010fb459c6f3481ff006fffc4001811010101010100000000000000000000000100112151ffda0008010301013f105d59a519260649dc3812767975b09a89844916d910db9908b88fb0ae470e2bffc400191100030101010000000000000000000000000121113151ffda0008010201013f10736b45e88944b46b7d1698b0331c763c9a25ad278584e225511bf475175186043623ffc4001f100100020104030100000000000000000001001121314161915171d1e1ffda0008010100013f101865a97f695c691b225dd71d477659e62e15dabe4c8d9a63300513dc20c5b610286dc5efe22020cc476db32f3b66177d6a5a66a378c36dfa869f0cb2e5c5c2d51fb2907820ac3a733fffd9";
        const SOS_0: [(usize, &str); 9] = [
            (
                326,
                "3d1bceb19efdf4e2958724a184c16a75c07f5492798620ae2dc58221dc52e90b",
            ),
            (
                402,
                "a899d0c2599e1129520b0fbb2664793f4481f0f124c0eee7032b3c793355ae38",
            ),
            (
                496,
                "0a51769295429a10be7bc4a0c82d6152731a16782c769231972323b69e2ac42e",
            ),
            (
                572,
                "196b41a86c1bee12f04146a1d9b43430101735a443d74c47260b9329062251ed",
            ),
            (
                613,
                "4917fafdc31472ce2dfd4007d1c09fd1b3f9c9fdbefd5c7582b395c8e9ee6688",
            ),
            (
                651,
                "f4aa96a594cbe7caec1c9f87d0d352cc052405e92006e41168670d255bd5a62c",
            ),
            (
                700,
                "8cd751ed7255a438b060374df0936b5f5635ed81fb957cb6c42882ad0572b462",
            ),
            (
                768,
                "7b4f4e9ded7873d035fd514aba46b659449f947f5e4e929b2e4de7ebfd41130c",
            ),
            (
                845,
                "109cba86b4b054271784d6feb74abe17193ae20104be4e033cd43288bc4bfe24",
            ),
        ];
        let s0 = from_hex(SUB_0_HEX);
        for (k, (cut, want)) in SOS_0.iter().enumerate() {
            let mut part = s0[..*cut].to_vec();
            part.extend_from_slice(&[0xFF, 0xD9]);
            let bgr = decode(&part, 40_000_000).unwrap();
            assert_eq!(sha256_hex(&bgr.data), *want, "sub=0 k={}", k + 1);
        }
        // Cut in the middle of a scan: the rows that scan never reached smooth with the previous scan's
        // coef_bits (jdcoefct.c's last_good_iMCU_row). Pillow 12.3 with JSIMD_FORCENONE=1; 592 it refuses.
        const MID_0: [(usize, &str); 7] = [
            (
                364,
                "8897011a0d8cd0e71ab4e37e6605095f42820635b8e52963af96f0a0c367c3f5",
            ),
            (
                449,
                "4e4b9b9e1931f92b58c29a19cd5fc2d777b3555fce3e5f55d6de7f93f9a229ad",
            ),
            (
                534,
                "b1893184007188c6148b14c157eb98dba4fbc99291a072f66c1dce53a7c4784c",
            ),
            (
                632,
                "6ebd3297ddf3a6a05ed543bc85dd9d6ca0bc74a7dda5355ade68f6d76e2aa229",
            ),
            (
                734,
                "103b7f8f609c64521c26ca6e30093d50c3d76f5519a4a5d1066126cae9dc8076",
            ),
            (
                806,
                "b1475fd3fc3fc921012a4c9661c0e16f99b4b7e1bddb104dd9bb3b46898874e0",
            ),
            (
                886,
                "6ee27ecf83524deffa23eda5ca48e4102cee9a00996e1c5ecab7a321bc1b21a3",
            ),
        ];
        for (cut, want) in MID_0 {
            let mut part = s0[..cut].to_vec();
            part.extend_from_slice(&[0xFF, 0xD9]);
            let bgr = decode(&part, 40_000_000).unwrap();
            assert_eq!(sha256_hex(&bgr.data), want, "sub=0 cut={cut}");
        }

        const SUB_2_HEX: &str = "ffd8ffe000104a46494600010100000100010000ffdb00430006040506050406060506070706080a100a0a09090a140e0f0c1017141818171416161a1d251f1a1b231c1616202c20232627292a29191f2d302d283025282928ffdb0043010707070a080a130a0a13281a161a2828282828282828282828282828282828282828282828282828282828282828282828282828282828282828282828282828ffc2001108001d002503012200021101031101ffc400190000030101010000000000000000000000000506040103ffc4001801000203000000000000000000000000000306000105ffda000c03010002100310000001b113633dedf05ce06b7c1a996d9109f6a06b2b0aa8aa6c49624e12bfffc4001c1000020203010100000000000000000000000301210211130431ffda00080101000105025cd332193b217127083b6a19e821db9465b3431b4d75a997e4ccc73a68cfa98b451d723fffc4001c1100020203010100000000000000000000020300010411120521ffda0008010301013f01c94da57a89499b7703e0d54f44e8af9985843aea1af453ffc4001d110001040301010000000000000000000000010203041115211451ffda0008010201013f014afdc93da4ae86e97e12e18c1d53d2ee9af89387ffc400161000030000000000000000000000000000203140ffda0008010100063f020507ffc4001b10010100020301000000000000000000000100113110214161ffda0008010100013f21020e22529c029f7bdc80c0c6a43a49ea577b4f71cd6ef73059208d17ffda000c03010002000300000010e7802438ff00ffc400191100020301000000000000000000000000000110115121ffda0008010301013f10a4b204779052086791708fffc400191100030101010000000000000000000000000111214151ffda0008010201013f1073a133e0d2f03d023461ce3fffc4001f100100020104030100000000000000000001001121314161915171d1e1ffda0008010100013f101865a97f695c691b225dd71d477659e62e15dabe4c8d9a63300513dc20c5b610286dc5efe22020cc476db32f3b66177d6a5a66a378c36dfa869f0cb2e5c5c2d51fb2907820ac3a733fffd9";
        const SOS_2: [(usize, &str); 9] = [
            (
                302,
                "bec8fa5ca7d8893a173e3196099c010adc65499e99809ef1ad5b76573c032635",
            ),
            (
                378,
                "4d3412cf99e6bab829e29da42b1db796170df32ddd39ece66bf7f5351b43159e",
            ),
            (
                442,
                "82bd54ba726220ebaf2d0fa54c60d0b3b7ce63001f350ed332fdcdc63263c2b0",
            ),
            (
                496,
                "55b09dea21ad11725ecc5374458edf8bf07998a21e3f00eb0c5d5079cb88fe30",
            ),
            (
                537,
                "4292eb50b99eab229c70b26da25e55a90c9d67a35c25b42c37382ad0a3b2980b",
            ),
            (
                575,
                "8d79c8c084953041a6aa4b04bb003b6e5919428a19044a807b27951332716100",
            ),
            (
                622,
                "3892df4f46377707ef3d3a91311c746779ef1ddaf2b42569bad6b7f894ddf52c",
            ),
            (
                670,
                "ec32c17f6b69915fc6a6b4ab82264ed1d1911d7b3e1a9fedb330a27c66ac6f0a",
            ),
            (
                725,
                "4433ae6f068944d3044717335634bd79d31f9a00bb5012f1411af3fcc289b23f",
            ),
        ];
        let s2 = from_hex(SUB_2_HEX);
        for (k, (cut, want)) in SOS_2.iter().enumerate() {
            let mut part = s2[..*cut].to_vec();
            part.extend_from_slice(&[0xFF, 0xD9]);
            let bgr = decode(&part, 40_000_000).unwrap();
            assert_eq!(sha256_hex(&bgr.data), *want, "sub=2 k={}", k + 1);
        }

        // Refuses EXIF XResolution of length 1 (BYTE) with ResolutionUnit when JFIF has no DPI unit (unit=0)
        let exif_bad = b"Exif\0\0II*\x00\x08\x00\x00\x00\x02\x00\x1a\x01\x01\x00\x01\x00\x00\x00\x48\x00\x00\x00\x28\x01\x03\x00\x01\x00\x00\x00\x02\x00\x00\x00\x00\x00\x00\x00";
        let mut with_exif_bad = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, (exif_bad.len() + 2) as u8];
        with_exif_bad.extend_from_slice(exif_bad);
        with_exif_bad.extend_from_slice(&s0[2..]);
        assert!(decode(&with_exif_bad, 40_000_000).is_err());

        // Refuses MPF with MPEntry under 16 bytes, accepts 16-byte MPEntry
        let mpf_bad = b"MPF\0II*\x00\x08\x00\x00\x00\x02\x00\x01\xb0\x04\x00\x01\x00\x00\x00\x01\x00\x00\x00\x02\xb0\x07\x00\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let mut with_mpf_bad = vec![0xFF, 0xD8, 0xFF, 0xE2, 0x00, (mpf_bad.len() + 2) as u8];
        with_mpf_bad.extend_from_slice(mpf_bad);
        with_mpf_bad.extend_from_slice(&s0[2..]);
        assert!(decode(&with_mpf_bad, 40_000_000).is_err());

        let mpf_ok = b"MPF\0II*\x00\x08\x00\x00\x00\x02\x00\x01\xb0\x04\x00\x01\x00\x00\x00\x01\x00\x00\x00\x02\xb0\x07\x00\x10\x00\x00\x00\x26\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let mut with_mpf_ok = vec![0xFF, 0xD8, 0xFF, 0xE2, 0x00, (mpf_ok.len() + 2) as u8];
        with_mpf_ok.extend_from_slice(mpf_ok);
        with_mpf_ok.extend_from_slice(&s0[2..]);
        assert!(decode(&with_mpf_ok, 40_000_000).is_ok());
    }
}
